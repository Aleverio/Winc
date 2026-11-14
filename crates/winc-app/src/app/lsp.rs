use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::thread;

use dscode_lsp::{LspClient, LspClientState, LspNotification, LspServerRequest};
use lsp_types::{
    Hover, HoverContents, MarkedString, PublishDiagnosticsParams, TextDocumentContentChangeEvent,
    Url,
};
use ropey::Rope;

use super::*;
use crate::app::UserEvent;
use crate::winc::WindowProxy;

pub(crate) mod completion;
mod edit;
pub(crate) mod navigation;
pub(crate) mod signature;
mod sync;
pub(crate) use sync::{
    auto_server_command, display_lsp_command, incremental_change, split_server_command,
};
#[cfg(test)]
mod tests;

struct SyncRequest {
    document: winc_editor::DocumentId,
    language: String,
    command: Vec<String>,
    root: PathBuf,
    path: PathBuf,
    revision: u64,
    text: Option<String>,
    snapshot: Rope,
    changes: Vec<TextDocumentContentChangeEvent>,
    changed: bool,
    saved: bool,
}

pub(crate) struct LspDocumentSnapshot {
    pub(in crate::app) path: PathBuf,
    pub(in crate::app) language: String,
    command: Vec<String>,
    pub(in crate::app) root: PathBuf,
    pub(in crate::app) revision: u64,
    text: Rope,
}

#[derive(Clone)]
struct CloseRequest {
    language: String,
    root: PathBuf,
    path: PathBuf,
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
    kind: HoverRequestKind,
}

#[derive(Clone, Copy)]
enum HoverRequestKind {
    Hover,
    SignatureHelp,
}

pub(super) struct CompletionRequest {
    pub(super) language: String,
    pub(super) command: Vec<String>,
    pub(super) root: PathBuf,
    pub(super) path: PathBuf,
    pub(super) line: u32,
    pub(super) character: u32,
    pub(super) document: winc_editor::DocumentId,
    pub(super) revision: u64,
    pub(super) caret: usize,
}

struct FormattingRequest {
    language: String,
    command: Vec<String>,
    root: PathBuf,
    path: PathBuf,
    document: winc_editor::DocumentId,
    revision: u64,
    options: lsp_types::FormattingOptions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NavigationKind {
    Definition,
    Declaration,
    TypeDefinition,
    Implementation,
    References,
}

pub(crate) struct NavigationRequest {
    pub(crate) language: String,
    pub(crate) command: Vec<String>,
    pub(crate) root: PathBuf,
    pub(crate) path: PathBuf,
    pub(crate) line: u32,
    pub(crate) character: u32,
    pub(crate) document: winc_editor::DocumentId,
    pub(crate) revision: u64,
    pub(crate) caret: usize,
    pub(crate) kind: NavigationKind,
}

#[derive(Debug)]
pub(crate) struct NavigationEvent {
    pub(crate) document: winc_editor::DocumentId,
    pub(crate) revision: u64,
    pub(crate) caret: usize,
    pub(crate) kind: NavigationKind,
    pub(crate) result: Result<Vec<lsp_types::Location>, String>,
}

#[derive(Debug)]
pub(crate) struct CompletionEvent {
    pub(crate) document: winc_editor::DocumentId,
    pub(crate) revision: u64,
    pub(crate) caret: usize,
    pub(crate) result: Result<Option<lsp_types::CompletionResponse>, String>,
}

pub(crate) struct CompletionPopup {
    pub(crate) document: winc_editor::DocumentId,
    pub(crate) revision: u64,
    pub(crate) caret: usize,
    pub(crate) items: Vec<lsp_types::CompletionItem>,
    pub(crate) selected: usize,
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

struct ExecuteCommandRequest {
    language: String,
    command: Vec<String>,
    root: PathBuf,
    command_name: String,
    arguments: Vec<serde_json::Value>,
}

enum Request {
    Sync(SyncRequest),
    Close(CloseRequest),
    Hover(HoverRequest),
    Completion(CompletionRequest),
    Formatting(FormattingRequest),
    Navigation(NavigationRequest),
    CodeActions(Box<CodeActionsRequest>),
    ExecuteCommand(ExecuteCommandRequest),
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
                let shutdown_proxy = proxy.clone();
                runtime.block_on(async {
                    for (_, client) in clients.drain() {
                        if let Err(error) = client.stop().await {
                            let _ = shutdown_proxy.send_event(UserEvent::LspError(format!(
                                "Could not gracefully stop an LSP server: {error}"
                            )));
                        }
                    }
                });
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

    pub(super) fn completion(&self, request: CompletionRequest) -> Result<(), String> {
        self.sender
            .send(Request::Completion(request))
            .map_err(|error| error.to_string())
    }

    fn formatting(&self, request: FormattingRequest) -> Result<(), String> {
        self.sender
            .send(Request::Formatting(request))
            .map_err(|error| error.to_string())
    }

    fn navigation(&self, request: NavigationRequest) -> Result<(), String> {
        self.sender
            .send(Request::Navigation(request))
            .map_err(|error| error.to_string())
    }

    fn close(&self, request: CloseRequest) -> Result<(), String> {
        self.sender
            .send(Request::Close(request))
            .map_err(|error| error.to_string())
    }

    fn execute_command(&self, request: ExecuteCommandRequest) -> Result<(), String> {
        self.sender
            .send(Request::ExecuteCommand(request))
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
            document,
            language,
            command,
            root,
            path,
            revision,
            text,
            snapshot,
            changes,
            changed,
            saved,
        }) => {
            let result = async {
                let client = get_client(&language, &command, &root, clients, proxy, false).await?;
                let uri = file_uri(&path)?;
                if !client.document_is_open(&uri).await {
                    client
                        .did_open(
                            uri.clone(),
                            language.clone(),
                            revision.min(i32::MAX as u64) as i32,
                            text.unwrap_or_else(|| snapshot.to_string()),
                        )
                        .await
                        .map_err(|error| format!("LSP document open failed: {error}"))?;
                } else if changed && !changes.is_empty() {
                    client
                        .did_change_with_changes(uri, revision.min(i32::MAX as u64) as i32, changes)
                        .await
                        .map_err(|error| format!("LSP document sync failed: {error}"))?;
                }
                if saved {
                    client.did_save(file_uri(&path)?).await.map_err(|error| {
                        format!("LSP document save notification failed: {error}")
                    })?;
                }
                Ok::<_, String>(())
            }
            .await;
            if let Err(error) = result {
                let key = (language.clone(), root.clone());
                if let Some(client) = clients.remove(&key)
                    && let Err(stop_error) = client.stop().await
                {
                    let _ = proxy.send_event(UserEvent::LspError(format!(
                        "Could not stop the failed {language} language server: {stop_error}"
                    )));
                }
                let _ = proxy.send_event(UserEvent::LspSyncFailed(
                    document, revision, language, root, error,
                ));
                return Ok(None);
            }
            let _ = proxy.send_event(UserEvent::LspSyncSucceeded(document, revision));
            Ok(None)
        }
        Request::Close(CloseRequest {
            language,
            root,
            path,
        }) => {
            if let Some(client) = clients.get(&(language.clone(), root.clone())) {
                client
                    .did_close(file_uri(&path)?)
                    .await
                    .map_err(|error| format!("LSP document close failed: {error}"))?;
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
            kind,
        }) => {
            let client = get_client(&language, &command, &root, clients, proxy, true).await?;
            let uri = file_uri(&path)?;
            let proxy = proxy.clone();
            tokio::spawn(async move {
                match kind {
                    HoverRequestKind::Hover => {
                        let result = client
                            .hover(uri, line, character)
                            .await
                            .map(|hover| hover.map(hover_text))
                            .map_err(|error| format!("LSP hover failed: {error}"));
                        let _ = proxy
                            .send_event(UserEvent::LspHover(document, revision, caret, result));
                    }
                    HoverRequestKind::SignatureHelp => {
                        let result = client
                            .signature_help(uri, line, character)
                            .await
                            .map(|help| help.map(signature::signature_help_text))
                            .map_err(|error| format!("LSP signature-help request failed: {error}"));
                        let _ = proxy.send_event(UserEvent::LspSignatureHelp(
                            document, revision, caret, result,
                        ));
                    }
                }
            });
            Ok(None)
        }
        Request::Completion(CompletionRequest {
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
            let client = get_client(&language, &command, &root, clients, proxy, true).await?;
            let uri = file_uri(&path)?;
            let proxy = proxy.clone();
            tokio::spawn(async move {
                let result = client
                    .completion(
                        uri,
                        line,
                        character,
                        lsp_types::CompletionTriggerKind::INVOKED,
                        None,
                    )
                    .await;
                let _ = proxy.send_event(UserEvent::LspCompletion(Box::new(CompletionEvent {
                    document,
                    revision,
                    caret,
                    result,
                })));
            });
            Ok(None)
        }
        Request::Formatting(FormattingRequest {
            language,
            command,
            root,
            path,
            document,
            revision,
            options,
        }) => {
            let client = get_client(&language, &command, &root, clients, proxy, true).await?;
            let uri = file_uri(&path)?;
            let proxy = proxy.clone();
            tokio::spawn(async move {
                let result = client
                    .formatting(uri, options)
                    .await
                    .map_err(|error| format!("LSP formatting request failed: {error}"));
                let _ = proxy.send_event(UserEvent::LspFormatted(document, revision, result));
            });
            Ok(None)
        }
        Request::Navigation(request) => {
            let client = match get_client(
                &request.language,
                &request.command,
                &request.root,
                clients,
                proxy,
                true,
            )
            .await
            {
                Ok(client) => client,
                Err(error) => {
                    let _ = proxy.send_event(UserEvent::LspNavigation(Box::new(NavigationEvent {
                        document: request.document,
                        revision: request.revision,
                        caret: request.caret,
                        kind: request.kind,
                        result: Err(error),
                    })));
                    return Ok(None);
                }
            };
            let proxy = proxy.clone();
            tokio::spawn(async move {
                let result = async {
                    let uri = file_uri(&request.path)?;
                    match request.kind {
                        NavigationKind::Definition => {
                            client
                                .definition_locations(uri, request.line, request.character)
                                .await
                        }
                        NavigationKind::Declaration => {
                            client
                                .declaration_locations(uri, request.line, request.character)
                                .await
                        }
                        NavigationKind::TypeDefinition => {
                            client
                                .type_definition_locations(uri, request.line, request.character)
                                .await
                        }
                        NavigationKind::Implementation => {
                            client
                                .implementation_locations(uri, request.line, request.character)
                                .await
                        }
                        NavigationKind::References => {
                            client
                                .reference_locations(uri, request.line, request.character)
                                .await
                        }
                    }
                    .map_err(|error| format!("LSP navigation request failed: {error}"))
                }
                .await;
                let _ = proxy.send_event(UserEvent::LspNavigation(Box::new(NavigationEvent {
                    document: request.document,
                    revision: request.revision,
                    caret: request.caret,
                    kind: request.kind,
                    result,
                })));
            });
            Ok(None)
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
            let client = get_client(&language, &command, &root, clients, proxy, true).await?;
            let proxy = proxy.clone();
            tokio::spawn(async move {
                let result = async {
                    let actions = client
                        .code_actions(file_uri(&path)?, range, vec![diagnostic])
                        .await
                        .map_err(|error| format!("LSP quick-fix request failed: {error}"))?;
                    Ok::<_, String>(actions.unwrap_or_default())
                }
                .await;
                let _ = proxy.send_event(UserEvent::LspCodeActions(
                    document,
                    revision,
                    index,
                    requested_range,
                    requested_message,
                    result,
                ));
            });
            Ok(None)
        }
        Request::ExecuteCommand(ExecuteCommandRequest {
            language,
            command,
            root,
            command_name,
            arguments,
        }) => {
            let client = get_client(&language, &command, &root, clients, proxy, true).await?;
            let proxy = proxy.clone();
            tokio::spawn(async move {
                if let Err(error) = client
                    .execute_command(command_name.clone(), arguments)
                    .await
                {
                    let _ = proxy.send_event(UserEvent::LspError(format!(
                        "LSP command {command_name} failed: {error}"
                    )));
                }
            });
            Ok(None)
        }
        Request::Reset(language, root) => {
            if let Some(client) = clients.remove(&(language.clone(), root.clone())) {
                let result = client.stop().await;
                let _ = proxy.send_event(UserEvent::LspState(language.clone(), root.clone(), None));
                result.map_err(|error| {
                    format!("Could not stop the {language} language server: {error}")
                })?;
            } else {
                let _ = proxy.send_event(UserEvent::LspState(language, root, None));
            }
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
    report_startup_error: bool,
) -> Result<Arc<LspClient>, String> {
    let key = (language.to_owned(), root.to_path_buf());
    if let Some(client) = clients.get(&key).cloned() {
        let matches_command = command
            .split_first()
            .is_some_and(|(program, args)| client.matches_command(program, args));
        if matches_command && client.get_state().await != LspClientState::Crashed {
            return Ok(client);
        }
        clients.remove(&key);
        if let Err(error) = client.stop().await {
            let _ = proxy.send_event(UserEvent::LspError(format!(
                "Could not stop the previous {language} language server: {error}"
            )));
        }
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
    let server_requests = client.subscribe_server_requests();
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
        if report_startup_error {
            let _ = proxy.send_event(UserEvent::LspState(
                language.to_owned(),
                root.to_path_buf(),
                Some(Err(error.clone())),
            ));
        }
        return Err(error);
    }
    forward_notifications(
        &client,
        notifications,
        language.to_owned(),
        root.to_path_buf(),
        proxy.clone(),
    );
    forward_server_requests(
        &client,
        server_requests,
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

fn forward_server_requests(
    client: &Arc<LspClient>,
    mut requests: tokio::sync::broadcast::Receiver<LspServerRequest>,
    language: String,
    root: PathBuf,
    proxy: WindowProxy,
) {
    let client = Arc::clone(client);
    tokio::spawn(async move {
        loop {
            let request = match requests.recv().await {
                Ok(request) => request,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                    let _ = proxy.send_event(UserEvent::LspError(format!(
                        "Dropped {count} requests from the {language} language server"
                    )));
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            };
            let response = match request.method.as_str() {
                "workspace/applyEdit" => {
                    let result = workspace_apply_edit_response(&request.params, &proxy).await;
                    client.respond(request.id, result).await
                }
                _ => match server_request_response(&request, &root) {
                    Ok(result) => client.respond(request.id, result).await,
                    Err((code, message)) => client.respond_error(request.id, code, message).await,
                },
            };
            if let Err(error) = response {
                let _ = proxy.send_event(UserEvent::LspError(format!(
                    "Could not respond to {language} language server request: {error}"
                )));
            }
        }
    });
}

async fn workspace_apply_edit_response(
    params: &serde_json::Value,
    proxy: &WindowProxy,
) -> serde_json::Value {
    let result = async {
        let value = params
            .get("edit")
            .cloned()
            .ok_or_else(|| "Workspace edit request has no edit payload".to_owned())?;
        let edit = serde_json::from_value(value)
            .map_err(|error| format!("Invalid workspace edit: {error}"))?;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        proxy
            .send_event(UserEvent::LspApplyWorkspaceEdit(Box::new(edit), sender))
            .map_err(|_| "The editor window is no longer available".to_owned())?;
        match tokio::time::timeout(std::time::Duration::from_secs(10), receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err("The editor could not process the workspace edit".to_owned()),
            Err(_) => Err("The editor took too long to apply the workspace edit".to_owned()),
        }
    }
    .await;
    match result {
        Ok(()) => serde_json::json!({ "applied": true }),
        Err(error) => serde_json::json!({
            "applied": false,
            "failureReason": error
        }),
    }
}

fn server_request_response(
    request: &LspServerRequest,
    root: &Path,
) -> Result<serde_json::Value, (i64, String)> {
    match request.method.as_str() {
        "workspace/configuration" => {
            let count = request
                .params
                .get("items")
                .and_then(serde_json::Value::as_array)
                .map_or(0, Vec::len);
            Ok(serde_json::Value::Array(vec![
                serde_json::Value::Null;
                count
            ]))
        }
        "workspace/workspaceFolders" => {
            let uri = file_uri(root).map_err(|error| (-32603, error))?;
            let folders = vec![lsp_types::WorkspaceFolder {
                uri,
                name: root
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned(),
            }];
            serde_json::to_value(folders).map_err(|error| (-32603, error.to_string()))
        }
        "client/registerCapability"
        | "client/unregisterCapability"
        | "window/workDoneProgress/create" => Ok(serde_json::Value::Null),
        method => Err((-32601, format!("Unsupported LSP server request: {method}"))),
    }
}

pub(crate) fn code_action_title(action: &lsp_types::CodeActionOrCommand) -> &str {
    match action {
        lsp_types::CodeActionOrCommand::CodeAction(action) => &action.title,
        lsp_types::CodeActionOrCommand::Command(command) => &command.title,
    }
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

pub(crate) fn position_to_line_col(
    rope: &Rope,
    position: lsp_types::Position,
) -> Result<(usize, usize), String> {
    let character = edit::lsp_position_to_char(rope, position)?;
    let line = position.line as usize;
    Ok((line, character - rope.line_to_char(line)))
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
    pub(in crate::app) fn lsp_formatted(
        &mut self,
        document: winc_editor::DocumentId,
        revision: u64,
        result: Result<Option<Vec<lsp_types::TextEdit>>, String>,
    ) {
        let Some(doc) = self.editor.document(document) else {
            return;
        };
        if doc.revision != revision {
            return;
        }
        let Some(path) = doc.path.clone() else {
            return;
        };
        match result {
            Ok(Some(edits)) if !edits.is_empty() => {
                let Ok(uri) = file_uri(&path) else {
                    self.flash("Could not format the document: invalid file path".to_owned());
                    return;
                };
                let edit = lsp_types::WorkspaceEdit {
                    changes: Some(HashMap::from([(uri, edits)])),
                    ..Default::default()
                };
                if let Err(error) = self.apply_lsp_workspace_edit(edit) {
                    self.flash(format!("Could not apply LSP formatting edits: {error}"));
                }
            }
            Ok(_) => {}
            Err(error) => self.flash(error),
        }
    }

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
        self.lsp_sent.retain(|_, sent| sent.language != language);
        self.lsp_failed.retain(|id, _| {
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
        let configured = !self.config.lsp_servers.is_empty() || !self.config.lsp_auto.is_empty();
        if !configured && self.lsp_sent.is_empty() {
            return;
        }

        let Some(proxy) = self.link.proxy.clone() else {
            return;
        };
        if configured && self.lsp.is_none() {
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
        let mut closing = Vec::new();
        self.lsp_failed
            .retain(|id, _| self.editor.document(*id).is_some());
        for doc in &self.editor.documents {
            let Some(path) = doc.path.as_ref() else {
                continue;
            };
            if doc.is_large() {
                continue;
            }
            let language = doc.highlighter.language().id;
            if self
                .lsp_failed
                .get(&doc.id)
                .is_some_and(|revision| *revision == doc.revision)
            {
                continue;
            }
            self.lsp_failed.remove(&doc.id);
            let command = if self.config.lsp_auto.iter().any(|id| id == language) {
                auto_server_command(language)
            } else {
                self.config.lsp_servers.get(language).cloned()
            };
            let Some(command) = command else { continue };
            let Some(rope) = doc.text().rope() else {
                continue;
            };
            let root = self
                .editor
                .root()
                .filter(|root| path.starts_with(root))
                .or_else(|| path.parent())
                .unwrap_or(path)
                .to_path_buf();
            let previous = self.lsp_sent.get(&doc.id);
            let same_target = previous.is_some_and(|sent| {
                sent.path == *path
                    && sent.language == language
                    && sent.command == command
                    && sent.root == root
            });
            if let Some(sent) = previous.filter(|_| !same_target) {
                closing.push((
                    doc.id,
                    CloseRequest {
                        language: sent.language.clone(),
                        root: sent.root.clone(),
                        path: sent.path.clone(),
                    },
                ));
            }
            open.insert(doc.id);
            let changed =
                !same_target || previous.is_some_and(|sent| sent.revision != doc.revision);
            let opened = !same_target;
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
                let Some(sent) = previous else {
                    continue;
                };
                let change =
                    incremental_change(&sent.text, rope, doc.unchanged_lines_since(sent.revision));
                (None, change.into_iter().collect())
            };
            pending.push((
                doc.id,
                path.clone(),
                doc.revision,
                language.to_owned(),
                command,
                root,
                text,
                changes,
                changed,
                saved,
                rope.clone(),
            ));
        }
        for (id, sent) in &self.lsp_sent {
            if !open.contains(id) {
                closing.push((
                    *id,
                    CloseRequest {
                        language: sent.language.clone(),
                        root: sent.root.clone(),
                        path: sent.path.clone(),
                    },
                ));
            }
        }

        let Some(bridge) = self.lsp.clone() else {
            return;
        };
        let mut closed = std::collections::HashSet::new();
        for (id, request) in closing {
            if !closed.insert(id) {
                continue;
            }
            if let Err(error) = bridge.close(request.clone()) {
                self.flash(format!("Could not close the LSP document: {error}"));
            }
            if let Some(sent) = self.lsp_sent.remove(&id) {
                self.lsp_saved.remove(&id);
                self.lsp_code_actions
                    .retain(|(document, _, _), _| *document != id);
                self.lsp_code_actions_pending
                    .retain(|(document, _, _)| *document != id);
                if !self.lsp_sent.values().any(|open| open.path == sent.path) {
                    self.lsp_diagnostics.remove(&sent.path);
                }
            }
            self.lsp_failed.remove(&id);
        }
        for (
            id,
            path,
            revision,
            language,
            command,
            root,
            text,
            changes,
            changed,
            saved,
            snapshot,
        ) in pending
        {
            match bridge.sync(SyncRequest {
                document: id,
                language: language.clone(),
                command: command.clone(),
                root: root.clone(),
                path: path.clone(),
                revision,
                text,
                snapshot: snapshot.clone(),
                changes,
                changed,
                saved,
            }) {
                Ok(()) => {
                    self.lsp_sent.insert(
                        id,
                        LspDocumentSnapshot {
                            path: path.clone(),
                            language,
                            command,
                            root,
                            revision,
                            text: snapshot,
                        },
                    );
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
        let Some(server_command) = command else {
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
            command: server_command,
            root,
            path,
            line,
            character,
            document: id,
            revision,
            caret,
            kind: HoverRequestKind::Hover,
        }) {
            self.flash(format!("Could not request LSP hover: {error}"));
        }
    }

    pub(in crate::app) fn request_lsp_format(&mut self, document: winc_editor::DocumentId) -> bool {
        self.sync_lsp_documents();
        let Some((revision, path, language, options)) =
            self.editor.document(document).and_then(|doc| {
                let path = doc.path.clone()?;
                let language = doc.highlighter.language().id.to_owned();
                let options = lsp_types::FormattingOptions {
                    tab_size: doc.tab_width(self.editor.tab_width).max(1) as u32,
                    insert_spaces: !matches!(doc.indent(), winc_core::Indent::Tabs),
                    ..Default::default()
                };
                Some((doc.revision, path, language, options))
            })
        else {
            return false;
        };
        let command = if self.config.lsp_auto.iter().any(|id| id == &language) {
            auto_server_command(&language)
        } else {
            self.config.lsp_servers.get(&language).cloned()
        };
        let Some(server_command) = command else {
            return false;
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
            return true;
        };
        let request = FormattingRequest {
            language,
            command: server_command,
            root,
            path,
            document,
            revision,
            options,
        };
        if let Err(error) = bridge.formatting(request) {
            self.flash(format!("Could not request LSP formatting: {error}"));
        }
        true
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
        let Some(server_command) = command else {
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
                command: server_command,
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
        result: Result<Vec<lsp_types::CodeActionOrCommand>, String>,
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

    pub(in crate::app) fn apply_lsp_code_action(
        &mut self,
        document: winc_editor::DocumentId,
        revision: u64,
        diagnostic: usize,
        action_index: usize,
    ) {
        let Some(action) = self
            .lsp_code_actions
            .get(&(document, revision, diagnostic))
            .and_then(|actions| actions.as_ref().ok())
            .and_then(|actions| actions.get(action_index))
            .cloned()
        else {
            return;
        };
        let Some(doc) = self
            .editor
            .document(document)
            .filter(|doc| doc.revision == revision)
        else {
            return;
        };
        let Some(path) = doc.path.clone() else {
            self.flash("This code action has no file path".to_owned());
            return;
        };
        let language = doc.highlighter.language().id.to_owned();
        let root = self
            .editor
            .root()
            .filter(|root| path.starts_with(root))
            .or_else(|| path.parent())
            .unwrap_or(&path)
            .to_path_buf();
        let command = if self.config.lsp_auto.iter().any(|id| id == &language) {
            auto_server_command(&language)
        } else {
            self.config.lsp_servers.get(&language).cloned()
        };
        let Some(server_command) = command else {
            self.flash(format!("No LSP server is configured for {language}"));
            return;
        };
        let mut applied = false;
        let command_to_run = match action {
            lsp_types::CodeActionOrCommand::CodeAction(action) => {
                if let Some(disabled) = action.disabled {
                    self.flash(disabled.reason);
                    return;
                }
                if let Some(edit) = action.edit {
                    if let Err(error) = self.apply_lsp_workspace_edit(edit) {
                        self.flash(format!("Could not apply code action: {error}"));
                        return;
                    }
                    applied = true;
                }
                action.command
            }
            lsp_types::CodeActionOrCommand::Command(command) => Some(command),
        };
        if let Some(command) = command_to_run {
            let Some(bridge) = self.lsp.clone() else {
                self.flash("LSP worker is unavailable".to_owned());
                return;
            };
            if let Err(error) = bridge.execute_command(ExecuteCommandRequest {
                language,
                command: server_command,
                root,
                command_name: command.command,
                arguments: command.arguments.unwrap_or_default(),
            }) {
                self.flash(format!("Could not run LSP code action: {error}"));
                return;
            }
            applied = true;
        }
        if !applied {
            self.flash("This code action contains no applicable edit or command".to_owned());
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
