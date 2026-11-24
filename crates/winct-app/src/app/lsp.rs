use std::collections::{BTreeMap, HashMap};
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
use crate::winct::WindowProxy;

pub(crate) mod completion;
pub(crate) mod discovery;
mod edit;
pub(crate) mod navigation;
mod requests;
use requests::{file_uri, hover_text};
pub(crate) mod signature;
mod sync;
#[cfg(test)]
pub(crate) use sync::auto_server_command;
pub(crate) use sync::{display_lsp_command, incremental_change, split_server_command};
#[cfg(test)]
mod tests;

struct SyncRequest {
    document: winct_editor::DocumentId,
    language: String,
    command: Vec<String>,
    env: BTreeMap<String, String>,
    cwd: Option<PathBuf>,
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
    env: BTreeMap<String, String>,
    cwd: Option<PathBuf>,
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
    env: BTreeMap<String, String>,
    cwd: Option<PathBuf>,
    root: PathBuf,
    path: PathBuf,
    line: u32,
    character: u32,
    document: winct_editor::DocumentId,
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
    pub(super) env: BTreeMap<String, String>,
    pub(super) cwd: Option<PathBuf>,
    pub(super) root: PathBuf,
    pub(super) path: PathBuf,
    pub(super) line: u32,
    pub(super) character: u32,
    pub(super) document: winct_editor::DocumentId,
    pub(super) revision: u64,
    pub(super) caret: usize,
}

struct FormattingRequest {
    language: String,
    command: Vec<String>,
    env: BTreeMap<String, String>,
    cwd: Option<PathBuf>,
    root: PathBuf,
    path: PathBuf,
    document: winct_editor::DocumentId,
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
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) root: PathBuf,
    pub(crate) path: PathBuf,
    pub(crate) line: u32,
    pub(crate) character: u32,
    pub(crate) document: winct_editor::DocumentId,
    pub(crate) revision: u64,
    pub(crate) caret: usize,
    pub(crate) kind: NavigationKind,
}

#[derive(Debug)]
pub(crate) struct NavigationEvent {
    pub(crate) document: winct_editor::DocumentId,
    pub(crate) revision: u64,
    pub(crate) caret: usize,
    pub(crate) kind: NavigationKind,
    pub(crate) result: Result<Vec<lsp_types::Location>, String>,
}

#[derive(Debug)]
pub(crate) struct CompletionEvent {
    pub(crate) document: winct_editor::DocumentId,
    pub(crate) revision: u64,
    pub(crate) caret: usize,
    pub(crate) result: Result<Option<lsp_types::CompletionResponse>, String>,
}

pub(crate) struct CompletionPopup {
    pub(crate) document: winct_editor::DocumentId,
    pub(crate) revision: u64,
    pub(crate) caret: usize,
    pub(crate) items: Vec<lsp_types::CompletionItem>,
    pub(crate) selected: usize,
}

struct CodeActionsRequest {
    language: String,
    command: Vec<String>,
    env: BTreeMap<String, String>,
    cwd: Option<PathBuf>,
    root: PathBuf,
    path: PathBuf,
    range: lsp_types::Range,
    diagnostic: lsp_types::Diagnostic,
    document: winct_editor::DocumentId,
    revision: u64,
    index: usize,
}

struct ExecuteCommandRequest {
    language: String,
    command: Vec<String>,
    env: BTreeMap<String, String>,
    cwd: Option<PathBuf>,
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
            .name("winct-lsp".to_owned())
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
            env,
            cwd,
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
                let client = get_client(
                    ClientTarget {
                        language: &language,
                        command: &command,
                        env: &env,
                        cwd: cwd.as_ref(),
                        root: &root,
                    },
                    clients,
                    proxy,
                    false,
                )
                .await?;
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
            env,
            cwd,
            root,
            path,
            line,
            character,
            document,
            revision,
            caret,
            kind,
        }) => {
            let client = get_client(
                ClientTarget {
                    language: &language,
                    command: &command,
                    env: &env,
                    cwd: cwd.as_ref(),
                    root: &root,
                },
                clients,
                proxy,
                true,
            )
            .await?;
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
            env,
            cwd,
            root,
            path,
            line,
            character,
            document,
            revision,
            caret,
        }) => {
            let client = get_client(
                ClientTarget {
                    language: &language,
                    command: &command,
                    env: &env,
                    cwd: cwd.as_ref(),
                    root: &root,
                },
                clients,
                proxy,
                true,
            )
            .await?;
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
            env,
            cwd,
            root,
            path,
            document,
            revision,
            options,
        }) => {
            let client = get_client(
                ClientTarget {
                    language: &language,
                    command: &command,
                    env: &env,
                    cwd: cwd.as_ref(),
                    root: &root,
                },
                clients,
                proxy,
                true,
            )
            .await?;
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
                ClientTarget {
                    language: &request.language,
                    command: &request.command,
                    env: &request.env,
                    cwd: request.cwd.as_ref(),
                    root: &request.root,
                },
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
                env,
                cwd,
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
            let client = get_client(
                ClientTarget {
                    language: &language,
                    command: &command,
                    env: &env,
                    cwd: cwd.as_ref(),
                    root: &root,
                },
                clients,
                proxy,
                true,
            )
            .await?;
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
            env,
            cwd,
            root,
            command_name,
            arguments,
        }) => {
            let client = get_client(
                ClientTarget {
                    language: &language,
                    command: &command,
                    env: &env,
                    cwd: cwd.as_ref(),
                    root: &root,
                },
                clients,
                proxy,
                true,
            )
            .await?;
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

/// 选择一个语言服务器客户端所需的全部参数。
struct ClientTarget<'a> {
    language: &'a str,
    command: &'a [String],
    env: &'a BTreeMap<String, String>,
    cwd: Option<&'a PathBuf>,
    root: &'a Path,
}

async fn get_client(
    target: ClientTarget<'_>,
    clients: &mut HashMap<(String, PathBuf), Arc<LspClient>>,
    proxy: &WindowProxy,
    report_startup_error: bool,
) -> Result<Arc<LspClient>, String> {
    let ClientTarget {
        language,
        command,
        env,
        cwd,
        root,
    } = target;
    let key = (language.to_owned(), root.to_path_buf());
    let effective_cwd = cwd.cloned().unwrap_or_else(|| root.to_path_buf());
    if let Some(client) = clients.get(&key).cloned() {
        let matches_command = command.split_first().is_some_and(|(program, args)| {
            client.matches_config(program, args, env, Some(&effective_cwd))
        });
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
    let client = Arc::new(
        LspClient::new(language.to_owned(), program.clone(), args.to_vec())
            .with_env(env.clone())
            .with_cwd(effective_cwd),
    );
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
