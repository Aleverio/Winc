use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::thread;

use dscode_lsp::{LspClient, LspClientState, LspNotification};
use lsp_types::{
    Hover, HoverContents, MarkedString, PublishDiagnosticsParams, TextDocumentContentChangeEvent,
    Url,
};
use ropey::Rope;

use super::*;
use crate::app::UserEvent;
use crate::winc::WindowProxy;

struct SyncRequest {
    language: String,
    command: Vec<String>,
    root: PathBuf,
    path: PathBuf,
    revision: u64,
    text: Option<String>,
    changes: Vec<TextDocumentContentChangeEvent>,
    opened: bool,
    changed: bool,
    saved: bool,
}

struct HoverRequest {
    language: String,
    command: Vec<String>,
    root: PathBuf,
    path: PathBuf,
    line: u32,
    character: u32,
    document: winc_editor::DocumentId,
    revision: u64,
    caret: usize,
}

struct CodeActionsRequest {
    language: String,
    command: Vec<String>,
    root: PathBuf,
    path: PathBuf,
    range: lsp_types::Range,
    diagnostic: lsp_types::Diagnostic,
    document: winc_editor::DocumentId,
    revision: u64,
    index: usize,
}

enum Request {
    Sync(SyncRequest),
    Hover(HoverRequest),
    CodeActions(Box<CodeActionsRequest>),
    Reset(String, PathBuf),
}

#[derive(Clone)]
pub(crate) struct LspBridge {
    sender: Sender<Request>,
}

impl LspBridge {
    pub(crate) fn new(proxy: WindowProxy) -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::channel();
        thread::Builder::new()
            .name("winc-lsp".to_owned())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = proxy.send_event(UserEvent::LspError(format!(
                            "Could not start the LSP runtime: {error}"
                        )));
                        return;
                    }
                };
                let mut clients: HashMap<(String, PathBuf), Arc<LspClient>> = HashMap::new();
                while let Ok(request) = receiver.recv() {
                    let result = runtime.block_on(handle_request(request, &mut clients, &proxy));
                    match result {
                        Ok(Some(event)) => {
                            let _ = proxy.send_event(event);
                        }
                        Ok(None) => {}
                        Err(error) => {
                            let _ = proxy.send_event(UserEvent::LspError(error));
                        }
                    }
                }
            })?;
        Ok(Self { sender })
    }

    fn sync(&self, request: SyncRequest) -> Result<(), String> {
        self.sender
            .send(Request::Sync(request))
            .map_err(|error| error.to_string())
    }

    fn hover(&self, request: HoverRequest) -> Result<(), String> {
        self.sender
            .send(Request::Hover(request))
            .map_err(|error| error.to_string())
    }

    fn reset(&self, language: String, root: PathBuf) -> Result<(), String> {
        self.sender
            .send(Request::Reset(language, root))
            .map_err(|error| error.to_string())
    }
}

async fn handle_request(
    request: Request,
    clients: &mut HashMap<(String, PathBuf), Arc<LspClient>>,
    proxy: &WindowProxy,
) -> Result<Option<UserEvent>, String> {
    match request {
        Request::Sync(SyncRequest {
            language,
            command,
            root,
            path,
            revision,
            text,
            changes,
            opened,
            changed,
            saved,
        }) => {
            let client = get_client(&language, &command, &root, clients, proxy).await?;
            let uri = file_uri(&path)?;
            if opened {
                client
                    .did_open(
                        uri.clone(),
                        language,
                        revision.min(i32::MAX as u64) as i32,
                        text.unwrap_or_default(),
                    )
                    .await
                    .map_err(|error| format!("LSP document open failed: {error}"))?;
            } else if changed {
                client
                    .did_change_with_changes(uri, revision.min(i32::MAX as u64) as i32, changes)
                    .await
                    .map_err(|error| format!("LSP document sync failed: {error}"))?;
            }
            if saved {
                client
                    .did_save(file_uri(&path)?)
                    .await
                    .map_err(|error| format!("LSP document save notification failed: {error}"))?;
            }
            Ok(None)
        }
        Request::Hover(HoverRequest {
            language,
            command,
            root,
            path,
            line,
            character,
            document,
            revision,
            caret,
        }) => {
            let result = async {
                let client = get_client(&language, &command, &root, clients, proxy).await?;
                let hover = client
                    .hover(file_uri(&path)?, line, character)
                    .await
                    .map_err(|error| format!("LSP hover failed: {error}"))?;
                Ok::<_, String>(hover.map(hover_text))
            }
            .await;
            Ok(Some(UserEvent::LspHover(document, revision, caret, result)))
        }
        Request::CodeActions(request) => {
            let CodeActionsRequest {
                language,
                command,
                root,
                path,
                range,
                diagnostic,
                document,
                revision,
                index,
            } = *request;
            let requested_range = range;
            let requested_message = diagnostic.message.clone();
            let result = async {
                let client = get_client(&language, &command, &root, clients, proxy).await?;
                let actions = client
                    .code_actions(file_uri(&path)?, range, vec![diagnostic])
                    .await
                    .map_err(|error| format!("LSP quick-fix request failed: {error}"))?;
                Ok::<_, String>(
                    actions
                        .unwrap_or_default()
                        .into_iter()
                        .map(|action| match action {
                            lsp_types::CodeActionOrCommand::CodeAction(action) => action.title,
                            lsp_types::CodeActionOrCommand::Command(command) => command.title,
                        })
                        .collect(),
                )
            }
            .await;
            Ok(Some(UserEvent::LspCodeActions(
                document,
                revision,
                index,
                requested_range,
                requested_message,
                result,
            )))
        }
        Request::Reset(language, root) => {
            if let Some(client) = clients.remove(&(language.clone(), root.clone())) {
                client.stop().await.map_err(|error| {
                    format!("Could not stop the {language} language server: {error}")
                })?;
            }
            let _ = proxy.send_event(UserEvent::LspState(language, root, None));
            Ok(None)
        }
    }
}

async fn get_client(
    language: &str,
    command: &[String],
    root: &Path,
    clients: &mut HashMap<(String, PathBuf), Arc<LspClient>>,
    proxy: &WindowProxy,
) -> Result<Arc<LspClient>, String> {
    let key = (language.to_owned(), root.to_path_buf());
    if let Some(client) = clients.get(&key).cloned() {
        if client.get_state().await != LspClientState::Crashed {
            return Ok(client);
        }
        clients.remove(&key);
        let _ = client.stop().await;
        let _ = proxy.send_event(UserEvent::LspState(
            language.to_owned(),
            root.to_path_buf(),
            None,
        ));
    }
    let Some((program, args)) = command.split_first() else {
        return Err(format!("No LSP command is configured for {language}"));
    };
    let client = Arc::new(LspClient::new(
        language.to_owned(),
        program.clone(),
        args.to_vec(),
    ));
    let notifications = client.subscribe_notifications();
    let _ = proxy.send_event(UserEvent::LspState(
        language.to_owned(),
        root.to_path_buf(),
        Some(Err("connecting".to_owned())),
    ));
    let startup = async {
        client
            .start()
            .await
            .map_err(|error| format!("Could not start the {language} language server: {error}"))?;
        client.initialize(file_uri(root)?).await.map_err(|error| {
            format!("Could not initialize the {language} language server: {error}")
        })?;
        client.initialized().await.map_err(|error| {
            format!("Could not initialize the {language} language server: {error}")
        })
    }
    .await;
    if let Err(error) = startup {
        let _ = proxy.send_event(UserEvent::LspState(
            language.to_owned(),
            root.to_path_buf(),
            Some(Err(error.clone())),
        ));
        return Err(error);
    }
    forward_notifications(
        &client,
        notifications,
        language.to_owned(),
        root.to_path_buf(),
        proxy.clone(),
    );
    clients.insert(key, client.clone());
    let _ = proxy.send_event(UserEvent::LspState(
        language.to_owned(),
        root.to_path_buf(),
        Some(Ok(())),
    ));
    Ok(client)
}

fn forward_notifications(
    client: &Arc<LspClient>,
    mut notifications: tokio::sync::broadcast::Receiver<LspNotification>,
    language: String,
    root: PathBuf,
    proxy: WindowProxy,
) {
    let client = Arc::clone(client);
    tokio::spawn(async move {
        let mut check_connection = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = check_connection.tick() => {
                    if client.get_state().await == LspClientState::Crashed {
                        let _ = proxy.send_event(UserEvent::LspState(
                            language.clone(),
                            root.clone(),
                            Some(Err("Language server stopped unexpectedly".to_owned())),
                        ));
                        return;
                    }
                }
                received = notifications.recv() => {
                    let notification = match received {
                        Ok(notification) => notification,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                    };
                    if let Some(event) = diagnostic_event(notification) {
                        let _ = proxy.send_event(event);
                    }
                }
            }
        }
    });
}

fn diagnostic_event(notification: LspNotification) -> Option<UserEvent> {
    if notification.method != "textDocument/publishDiagnostics" {
        return None;
    }
    let diagnostics: PublishDiagnosticsParams = serde_json::from_value(notification.params).ok()?;
    let path = diagnostics.uri.to_file_path().ok()?;
    Some(UserEvent::LspDiagnostics(path, diagnostics.diagnostics))
}

fn file_uri(path: &Path) -> Result<Url, String> {
    Url::from_file_path(path).map_err(|()| format!("Invalid LSP file path: {}", path.display()))
}

fn hover_text(hover: Hover) -> String {
    let text = match hover.contents {
        HoverContents::Scalar(MarkedString::String(text)) => text,
        HoverContents::Scalar(MarkedString::LanguageString(value)) => {
            format!("```{}\n{}\n```", value.language, value.value)
        }
        HoverContents::Array(values) => values
            .into_iter()
            .map(|value| match value {
                MarkedString::String(text) => text,
                MarkedString::LanguageString(value) => {
                    format!("```{}\n{}\n```", value.language, value.value)
                }
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
        HoverContents::Markup(content) => content.value,
    };
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = compact.chars();
    let preview: String = chars.by_ref().take(180).collect();
    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

impl App {
    pub(in crate::app) fn reset_lsp_server(&mut self, language: &str) {
        let roots: std::collections::HashSet<_> = self
            .editor
            .documents
            .iter()
            .filter(|doc| doc.highlighter.language().id == language)
            .filter_map(|doc| doc.path.as_ref())
            .map(|path| {
                self.editor
                    .root()
                    .filter(|root| path.starts_with(root))
                    .or_else(|| path.parent())
                    .unwrap_or(path)
                    .to_path_buf()
            })
            .collect();
        if let Some(bridge) = self.lsp.as_ref() {
            let errors: Vec<_> = roots
                .into_iter()
                .filter_map(|root| bridge.reset(language.to_owned(), root).err())
                .collect();
            if !errors.is_empty() {
                self.flash(format!(
                    "Could not restart the LSP server: {}",
                    errors.join("; ")
                ));
            }
        }
        self.lsp_sent.retain(|id, _| {
            self.editor
                .document(*id)
                .is_none_or(|doc| doc.highlighter.language().id != language)
        });
        self.lsp_saved.retain(|id, _| {
            self.editor
                .document(*id)
                .is_none_or(|doc| doc.highlighter.language().id != language)
        });
        self.lsp_diagnostics.retain(|path, _| {
            !self.editor.documents.iter().any(|doc| {
                doc.path.as_ref() == Some(path) && doc.highlighter.language().id == language
            })
        });
        self.sync_lsp_documents();
    }

    pub(in crate::app) fn sync_lsp_documents(&mut self) {
        if self.config.lsp_servers.is_empty() && self.config.lsp_auto.is_empty() {
            return;
        }

        let Some(proxy) = self.link.proxy.clone() else {
            return;
        };
        if self.lsp.is_none() {
            match LspBridge::new(proxy) {
                Ok(bridge) => self.lsp = Some(bridge),
                Err(error) => {
                    self.flash(format!("Could not start the LSP worker: {error}"));
                    return;
                }
            }
        }

        let mut open = std::collections::HashSet::new();
        let mut pending = Vec::new();
        for doc in &self.editor.documents {
            let Some(path) = doc.path.as_ref() else {
                continue;
            };
            if doc.is_large() {
                continue;
            }
            let language = doc.highlighter.language().id;
            let command = if self.config.lsp_auto.iter().any(|id| id == language) {
                auto_server_command(language)
            } else {
                self.config.lsp_servers.get(language).cloned()
            };
            let Some(command) = command else { continue };
            let Some(rope) = doc.text().rope() else {
                continue;
            };
            open.insert(doc.id);
            let changed = self
                .lsp_sent
                .get(&doc.id)
                .is_none_or(|(sent_path, revision, _)| {
                    sent_path != path || *revision != doc.revision
                });
            let opened = self
                .lsp_sent
                .get(&doc.id)
                .is_none_or(|(sent_path, _, _)| sent_path != path);
            let saved = !doc.is_dirty()
                && (opened
                    || self
                        .lsp_saved
                        .get(&doc.id)
                        .is_none_or(|(saved_path, revision)| {
                            saved_path != path || *revision != doc.revision
                        }));
            if !changed && !saved {
                continue;
            }
            let (text, changes) = if opened {
                (Some(rope.to_string()), Vec::new())
            } else {
                let Some((_, sent_revision, sent_text)) = self.lsp_sent.get(&doc.id) else {
                    continue;
                };
                let change =
                    incremental_change(sent_text, rope, doc.unchanged_lines_since(*sent_revision));
                (None, change.into_iter().collect())
            };
            let root = self
                .editor
                .root()
                .filter(|root| path.starts_with(root))
                .or_else(|| path.parent())
                .unwrap_or(path)
                .to_path_buf();
            pending.push((
                doc.id,
                path.clone(),
                doc.revision,
                language.to_owned(),
                command,
                root,
                text,
                changes,
                opened,
                changed,
                saved,
                rope.clone(),
            ));
        }
        self.lsp_sent.retain(|id, _| open.contains(id));
        self.lsp_saved.retain(|id, _| open.contains(id));

        let Some(bridge) = self.lsp.clone() else {
            return;
        };
        for (
            id,
            path,
            revision,
            language,
            command,
            root,
            text,
            changes,
            opened,
            changed,
            saved,
            snapshot,
        ) in pending
        {
            match bridge.sync(SyncRequest {
                language,
                command,
                root,
                path: path.clone(),
                revision,
                text,
                changes,
                opened,
                changed,
                saved,
            }) {
                Ok(()) => {
                    self.lsp_sent.insert(id, (path.clone(), revision, snapshot));
                    if saved {
                        self.lsp_saved.insert(id, (path, revision));
                    }
                }
                Err(error) => self.flash(format!("LSP worker stopped: {error}")),
            }
        }
    }

    pub(in crate::app) fn request_lsp_hover(&mut self) {
        self.sync_lsp_documents();
        let Some((id, revision, path, language, line, character, caret)) =
            self.editor.active().and_then(|doc| {
                let path = doc.path.clone()?;
                let language = doc.highlighter.language().id.to_owned();
                let (line, column) = doc.text().pos_to_line_col(doc.selection.head);
                let line_text = doc.text().line_content(line).slice().to_string();
                let character = line_text
                    .chars()
                    .take(column)
                    .map(char::len_utf16)
                    .sum::<usize>();
                Some((
                    doc.id,
                    doc.revision,
                    path,
                    language,
                    line.min(u32::MAX as usize) as u32,
                    character.min(u32::MAX as usize) as u32,
                    doc.selection.head,
                ))
            })
        else {
            self.flash("Hover requires an open text file".to_owned());
            return;
        };
        let command = if self.config.lsp_auto.iter().any(|id| id == &language) {
            auto_server_command(&language)
        } else {
            self.config.lsp_servers.get(&language).cloned()
        };
        let Some(command) = command else {
            self.flash(format!("No LSP server is configured for {language}"));
            return;
        };
        let root = self
            .editor
            .root()
            .filter(|root| path.starts_with(root))
            .or_else(|| path.parent())
            .unwrap_or(&path)
            .to_path_buf();
        let Some(bridge) = self.lsp.clone() else {
            self.flash("LSP worker is unavailable".to_owned());
            return;
        };
        if let Err(error) = bridge.hover(HoverRequest {
            language,
            command,
            root,
            path,
            line,
            character,
            document: id,
            revision,
            caret,
        }) {
            self.flash(format!("Could not request LSP hover: {error}"));
        }
    }

    pub(in crate::app) fn request_lsp_code_actions(
        &mut self,
        document: winc_editor::DocumentId,
        index: usize,
    ) {
        let Some((revision, path, language, diagnostic)) =
            self.editor.document(document).and_then(|doc| {
                let path = doc.path.clone()?;
                let language = doc.highlighter.language().id.to_owned();
                let diagnostic = self.lsp_diagnostics.get(&path)?.get(index)?.clone();
                Some((doc.revision, path, language, diagnostic))
            })
        else {
            return;
        };
        let key = (document, revision, index);
        self.lsp_code_actions
            .retain(|(id, version, _), _| *id != document || *version == revision);
        self.lsp_code_actions_pending
            .retain(|(id, version, _)| *id != document || *version == revision);
        if self.lsp_code_actions.contains_key(&key) || !self.lsp_code_actions_pending.insert(key) {
            return;
        }
        let command = if self.config.lsp_auto.iter().any(|id| id == &language) {
            auto_server_command(&language)
        } else {
            self.config.lsp_servers.get(&language).cloned()
        };
        let Some(command) = command else {
            self.lsp_code_actions_pending.remove(&key);
            self.lsp_code_actions.insert(key, Ok(Vec::new()));
            self.request_redraw();
            return;
        };
        let root = self
            .editor
            .root()
            .filter(|root| path.starts_with(root))
            .or_else(|| path.parent())
            .unwrap_or(&path)
            .to_path_buf();
        let Some(bridge) = self.lsp.clone() else {
            self.lsp_code_actions_pending.remove(&key);
            self.lsp_code_actions.insert(
                key,
                Err("The language-server worker is unavailable.".to_owned()),
            );
            self.request_redraw();
            return;
        };
        if let Err(error) = bridge
            .sender
            .send(Request::CodeActions(Box::new(CodeActionsRequest {
                language,
                command,
                root,
                path,
                range: diagnostic.range,
                diagnostic,
                document,
                revision,
                index,
            })))
        {
            self.lsp_code_actions_pending.remove(&key);
            self.lsp_code_actions.insert(
                key,
                Err(format!("Could not request compiler suggestions: {error}")),
            );
            self.request_redraw();
        }
    }

    pub(in crate::app) fn lsp_code_actions_received(
        &mut self,
        document: winc_editor::DocumentId,
        revision: u64,
        index: usize,
        range: lsp_types::Range,
        message: String,
        result: Result<Vec<String>, String>,
    ) {
        let key = (document, revision, index);
        self.lsp_code_actions_pending.remove(&key);
        let current = self
            .editor
            .document(document)
            .filter(|doc| doc.revision == revision);
        let still_current = current.is_some_and(|doc| {
            doc.path
                .as_ref()
                .and_then(|path| self.lsp_diagnostics.get(path))
                .and_then(|diagnostics| diagnostics.get(index))
                .is_some_and(|diagnostic| {
                    diagnostic.range == range && diagnostic.message == message
                })
        });
        if still_current {
            self.lsp_code_actions.insert(key, result);
            self.request_redraw();
        }
    }

    pub(in crate::app) fn lsp_hovered(
        &mut self,
        document: winc_editor::DocumentId,
        revision: u64,
        caret: usize,
        result: Result<Option<String>, String>,
    ) {
        let still_at_request = self.editor.active().is_some_and(|doc| {
            doc.id == document && doc.revision == revision && doc.selection.head == caret
        });
        if !still_at_request {
            return;
        }
        match result {
            Ok(Some(text)) if !text.trim().is_empty() => self.hint_at_name(text),
            Ok(_) => self.flash("No hover information available".to_owned()),
            Err(error) => self.flash(error),
        }
    }
}

pub(crate) fn auto_server_command(language: &str) -> Option<Vec<String>> {
    let (program, args): (&str, &[&str]) = match language {
        "rust" => ("rust-analyzer", &[]),
        "python" => ("pyright-langserver", &["--stdio"]),
        "go" => ("gopls", &[]),
        "json" => ("vscode-json-language-server", &["--stdio"]),
        _ => return None,
    };
    Some(
        std::iter::once(program.to_owned())
            .chain(args.iter().map(|arg| (*arg).to_owned()))
            .collect(),
    )
}

/// Build one ranged LSP edit, using the known unchanged prefix and a common suffix.
fn incremental_change(
    old: &Rope,
    new: &Rope,
    unchanged_lines: usize,
) -> Option<TextDocumentContentChangeEvent> {
    let unchanged_lines = unchanged_lines
        .min(old.len_lines() - 1)
        .min(new.len_lines() - 1);
    let prefix_start = old.line_to_char(unchanged_lines);
    debug_assert_eq!(prefix_start, new.line_to_char(unchanged_lines));

    let mut old_prefix = old.slice(prefix_start..).chars();
    let mut new_prefix = new.slice(prefix_start..).chars();
    let mut start = prefix_start;
    while let (Some(old), Some(new)) = (old_prefix.next(), new_prefix.next()) {
        if old != new {
            break;
        }
        start += 1;
    }

    let old_tail = old.slice(start..);
    let new_tail = new.slice(start..);
    let mut old_chars = old_tail.chars_at(old_tail.len_chars()).reversed();
    let mut new_chars = new_tail.chars_at(new_tail.len_chars()).reversed();
    let mut suffix = 0;
    while let (Some(old), Some(new)) = (old_chars.next(), new_chars.next()) {
        if old != new {
            break;
        }
        suffix += 1;
    }

    let old_end = old.len_chars() - suffix;
    let new_end = new.len_chars() - suffix;
    if start == old_end && start == new_end {
        return None;
    }

    let position = |text: &Rope, char_index: usize| {
        let line = text.char_to_line(char_index);
        let line_start = text.line_to_char(line);
        lsp_types::Position::new(
            line.min(u32::MAX as usize) as u32,
            text.slice(line_start..char_index)
                .len_utf16_cu()
                .min(u32::MAX as usize) as u32,
        )
    };
    Some(TextDocumentContentChangeEvent {
        range: Some(lsp_types::Range::new(
            position(old, start),
            position(old, old_end),
        )),
        range_length: Some(
            old.slice(start..old_end)
                .len_utf16_cu()
                .min(u32::MAX as usize) as u32,
        ),
        text: new.slice(start..new_end).to_string(),
    })
}

#[cfg(test)]
mod command_tests {
    use super::*;
    use lsp_types::{Diagnostic, Position, Range};

    #[test]
    fn server_command_splits_arguments_without_a_shell() {
        assert_eq!(
            split_server_command(r#"clangd --compile-commands-dir "C:\work space""#).unwrap(),
            ["clangd", "--compile-commands-dir", r"C:\work space"]
        );
    }

    #[test]
    fn server_command_rejects_unclosed_quotes() {
        assert!(split_server_command(r#"clangd --config "oops"#).is_err());
    }

    #[test]
    fn publish_diagnostics_notifications_are_forwarded_to_the_document() {
        let path = std::env::temp_dir().join("winc-lsp-diagnostics.rs");
        let uri = Url::from_file_path(&path).unwrap();
        let notification = LspNotification {
            method: "textDocument/publishDiagnostics".to_owned(),
            params: serde_json::to_value(PublishDiagnosticsParams {
                uri,
                version: None,
                diagnostics: vec![Diagnostic {
                    range: Range::new(Position::new(0, 0), Position::new(0, 1)),
                    severity: Some(lsp_types::DiagnosticSeverity::ERROR),
                    code: None,
                    code_description: None,
                    source: Some("test".to_owned()),
                    message: "expected a value".to_owned(),
                    related_information: None,
                    tags: None,
                    data: None,
                }],
            })
            .unwrap(),
        };

        let Some(UserEvent::LspDiagnostics(received_path, diagnostics)) =
            diagnostic_event(notification)
        else {
            panic!("publishDiagnostics must produce a document diagnostics event");
        };
        assert_eq!(received_path, path);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].message, "expected a value");
    }

    #[test]
    fn automatic_servers_are_known_for_the_supported_defaults() {
        assert_eq!(
            auto_server_command("rust"),
            Some(vec!["rust-analyzer".to_owned()])
        );
        assert_eq!(
            auto_server_command("python"),
            Some(vec!["pyright-langserver".to_owned(), "--stdio".to_owned()])
        );
        assert!(auto_server_command("unknown").is_none());
    }

    #[test]
    fn incremental_change_uses_utf16_positions_for_unicode_edits() {
        let old = Rope::from_str("same\nA🦀Z\nunchanged");
        let new = Rope::from_str("same\nA🦀newZ\nunchanged");

        let change = incremental_change(&old, &new, 1).unwrap();
        assert_eq!(
            change.range,
            Some(lsp_types::Range::new(
                lsp_types::Position::new(1, 3),
                lsp_types::Position::new(1, 3),
            ))
        );
        assert_eq!(change.range_length, Some(0));
        assert_eq!(change.text, "new");
    }

    #[test]
    fn incremental_change_handles_line_insertions_and_deletions() {
        let old = Rope::from_str("first\nlast\n");
        let new = Rope::from_str("first\ninserted\nlast\n");

        let change = incremental_change(&old, &new, 1).unwrap();
        assert_eq!(
            change.range,
            Some(lsp_types::Range::new(
                lsp_types::Position::new(1, 0),
                lsp_types::Position::new(1, 0),
            ))
        );
        assert_eq!(change.text, "inserted\n");
    }

    #[test]
    fn incremental_change_returns_none_when_the_text_is_unchanged() {
        let text = Rope::from_str("same text\n");
        assert!(incremental_change(&text, &text, 0).is_none());
    }

    #[test]
    fn incremental_change_handles_replacement_without_a_common_prefix_or_suffix() {
        let old = Rope::from_str("abc");
        let new = Rope::from_str("xyz");

        let change = incremental_change(&old, &new, 0).unwrap();
        assert_eq!(
            change.range,
            Some(lsp_types::Range::new(
                lsp_types::Position::new(0, 0),
                lsp_types::Position::new(0, 3),
            ))
        );
        assert_eq!(change.range_length, Some(3));
        assert_eq!(change.text, "xyz");
    }

    #[test]
    fn incremental_change_handles_insertions_at_the_end() {
        let old = Rope::from_str("existing");
        let new = Rope::from_str("existing text");

        let change = incremental_change(&old, &new, 0).unwrap();
        assert_eq!(
            change.range,
            Some(lsp_types::Range::new(
                lsp_types::Position::new(0, 8),
                lsp_types::Position::new(0, 8),
            ))
        );
        assert_eq!(change.range_length, Some(0));
        assert_eq!(change.text, " text");
    }
}

pub(in crate::app) fn split_server_command(command: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = None::<String>;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if let Some(arg) = current.take() {
                    args.push(arg);
                }
            }
            '\'' => {
                let arg = current.get_or_insert_with(String::new);
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => arg.push(c),
                        None => return Err("Unclosed single quote in server command".to_owned()),
                    }
                }
            }
            '"' => {
                let arg = current.get_or_insert_with(String::new);
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') if matches!(chars.peek(), Some('"' | '\\')) => {
                            arg.extend(chars.next());
                        }
                        Some(c) => arg.push(c),
                        None => return Err("Unclosed double quote in server command".to_owned()),
                    }
                }
            }
            c => current.get_or_insert_with(String::new).push(c),
        }
    }
    args.extend(current);
    if args.is_empty() {
        return Err("A language server executable is required".to_owned());
    }
    Ok(args)
}

pub(crate) fn display_lsp_command(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if arg.chars().any(char::is_whitespace) || arg.contains('"') {
                format!("\"{}\"", arg.replace('\\', "\\\\").replace('"', "\\\""))
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{MarkupContent, MarkupKind};

    #[test]
    fn hover_content_fits_the_single_line_hint() {
        let content = Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: "```rust\nfn answer() -> u32\n```".to_owned(),
            }),
            range: None,
        };
        assert_eq!(hover_text(content), "```rust fn answer() -> u32 ```");
    }

    #[test]
    fn long_hover_content_is_truncated_at_a_character_boundary() {
        let content = Hover {
            contents: HoverContents::Scalar(MarkedString::String("🦀".repeat(181))),
            range: None,
        };
        let text = hover_text(content);
        assert_eq!(text.chars().count(), 183);
        assert!(text.ends_with("..."));
    }
}
