use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::thread;

use dscode_lsp::{LspClient, LspClientState, LspNotification};
use lsp_types::{Hover, HoverContents, MarkedString, PublishDiagnosticsParams, Url};

use super::*;
use crate::app::UserEvent;
use crate::winc::WindowProxy;

struct SyncRequest {
    language: String,
    command: Vec<String>,
    root: PathBuf,
    path: PathBuf,
    revision: u64,
    text: String,
    opened: bool,
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

enum Request {
    Sync(SyncRequest),
    Hover(HoverRequest),
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
            opened,
        }) => {
            let client = get_client(&language, &command, &root, clients, proxy).await?;
            let uri = file_uri(&path)?;
            let result = if opened {
                client.did_open(uri, language, text).await
            } else {
                client
                    .did_change(uri, revision.min(i32::MAX as u64) as i32, text)
                    .await
            };
            result.map_err(|error| format!("LSP document sync failed: {error}"))?;
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
        self.lsp_diagnostics.retain(|path, _| {
            !self.editor.documents.iter().any(|doc| {
                doc.path.as_ref() == Some(path) && doc.highlighter.language().id == language
            })
        });
        self.sync_lsp_documents();
    }

    pub(in crate::app) fn sync_lsp_documents(&mut self) {
        if self.config.lsp_servers.is_empty() {
            return;
        }

        let Some(proxy) = self.proxy.clone() else {
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
            if self
                .lsp_sent
                .get(&doc.id)
                .is_some_and(|(sent_path, revision)| sent_path == path && *revision == doc.revision)
            {
                continue;
            }
            let opened = self
                .lsp_sent
                .get(&doc.id)
                .is_none_or(|(sent_path, _)| sent_path != path);
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
                rope.to_string(),
                opened,
            ));
        }
        self.lsp_sent.retain(|id, _| open.contains(id));

        let Some(bridge) = self.lsp.clone() else {
            return;
        };
        for (id, path, revision, language, command, root, text, opened) in pending {
            match bridge.sync(SyncRequest {
                language,
                command,
                root,
                path: path.clone(),
                revision,
                text,
                opened,
            }) {
                Ok(()) => {
                    self.lsp_sent.insert(id, (path, revision));
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
