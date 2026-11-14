use lsp_types::*;
use std::collections::{HashMap, HashSet};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child as TokioChild, Command as TokioCommand};
use tokio::sync::{broadcast, oneshot, Mutex};
use tracing::{error, info, instrument, warn};

static REQUEST_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// State machine for the LSP client lifecycle.
///
/// Tracks the lifecycle of a Language Server Protocol client connected to
/// an external language server process (e.g., rust-analyzer, pyright).
///
/// # State Diagram
///
/// ```text
///   Stopped ──► Starting ──► Initializing ──► Ready
///     ▲             │              │             │
///     │  (spawn     │  (init       │   (stop()   │
///     │   fail)     │   fail)      │   called)   │
///     │             ▼              ▼             ▼
///     │          Crashed ◄──── Crashed      ShuttingDown
///     │             ▲                            │
///     │             │ (process exit,             │
///     │             │  read loop EOF)            │
///     │             └────────── Ready ───────────┘
///     │                                          │
///     └──────────────────────────────────────────┘
/// ```
///
/// # Transitions
///
/// - `Stopped` -> `Starting` (start() called, spawning process)
/// - `Starting` -> `Initializing` (process spawned, read loop started)
/// - `Starting` -> `Crashed` (Command::spawn() failed)
/// - `Initializing` -> `Ready` (LSP initialize handshake completed)
/// - `Initializing` -> `Crashed` (initialize request failed or timed out)
/// - `Ready` -> `ShuttingDown` (stop() called, sending shutdown request)
/// - `Ready` -> `Crashed` (process exited unexpectedly, read loop EOF)
/// - `ShuttingDown` -> `Stopped` (exit notification sent, process exited)
/// - `Crashed` -> `Starting` (explicit restart attempt)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LspClientState {
    /// No language server process is running.
    Stopped,
    /// The language server process is being spawned.
    Starting,
    /// The LSP initialize handshake is in progress.
    Initializing,
    /// The language server is fully initialized and ready to handle requests.
    Ready,
    /// A shutdown request has been sent to the language server.
    ShuttingDown,
    /// The language server process exited unexpectedly or failed to start.
    Crashed,
}

type PendingResponseMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<serde_json::Value, String>>>>>;

/// A JSON-RPC notification received from the language server.
#[derive(Debug, Clone)]
pub struct LspNotification {
    /// The notification method, such as `textDocument/publishDiagnostics`.
    pub method: String,
    /// The notification parameters.
    pub params: serde_json::Value,
}

/// A JSON-RPC request initiated by the language server.
#[derive(Debug, Clone)]
pub struct LspServerRequest {
    /// The request identifier that must be echoed in the response.
    pub id: serde_json::Value,
    /// The request method.
    pub method: String,
    /// The request parameters.
    pub params: serde_json::Value,
}

/// A state-machine-based LSP client that manages a language server process.
///
/// Handles spawning the language server, performing the LSP initialize handshake,
/// sending requests and notifications, and processing responses from the server.
/// All state transitions are validated to maintain the lifecycle invariant.
///
/// # Concurrency
///
/// State is stored in `Arc<Mutex<LspClientState>>`. The process and writer fields
/// use separate `Arc<Mutex<Option<T>>>` which can be locked independently.
/// Lock ordering: state -> process -> writer. Never hold the state lock while
/// also holding process/writer locks to avoid deadlock.
#[derive(Debug)]
pub struct LspClient {
    state: Arc<Mutex<LspClientState>>,
    process: Arc<Mutex<Option<TokioChild>>>,
    writer: Arc<Mutex<Option<tokio::process::ChildStdin>>>,
    pending_responses: PendingResponseMap,
    notifications: broadcast::Sender<LspNotification>,
    server_requests: broadcast::Sender<LspServerRequest>,
    open_documents: Mutex<HashSet<Url>>,
    language_id: String,
    server_command: String,
    server_args: Vec<String>,
}

impl LspClient {
    /// Creates a new LSP client in the `Stopped` state.
    ///
    /// - `language_id` — The language identifier (e.g., "rust", "python").
    /// - `server_command` — The command to spawn the language server (e.g., "rust-analyzer").
    /// - `server_args` — Arguments to pass to the language server command.
    pub fn new(language_id: String, server_command: String, server_args: Vec<String>) -> Self {
        let (notifications, _) = broadcast::channel(128);
        let (server_requests, _) = broadcast::channel(32);
        Self {
            state: Arc::new(Mutex::new(LspClientState::Stopped)),
            process: Arc::new(Mutex::new(None)),
            writer: Arc::new(Mutex::new(None)),
            pending_responses: Arc::new(Mutex::new(HashMap::new())),
            notifications,
            server_requests,
            open_documents: Mutex::new(HashSet::new()),
            language_id,
            server_command,
            server_args,
        }
    }

    /// Subscribe to notifications sent by the language server.
    pub fn subscribe_notifications(&self) -> broadcast::Receiver<LspNotification> {
        self.notifications.subscribe()
    }

    /// Subscribe to requests initiated by the language server.
    pub fn subscribe_server_requests(&self) -> broadcast::Receiver<LspServerRequest> {
        self.server_requests.subscribe()
    }

    /// Returns whether this client was created with the given executable and arguments.
    pub fn matches_command(&self, command: &str, args: &[String]) -> bool {
        self.server_command == command && self.server_args == args
    }

    /// Returns whether the server has received `textDocument/didOpen` for this URI.
    pub async fn document_is_open(&self, uri: &Url) -> bool {
        self.open_documents.lock().await.contains(uri)
    }

    /// Validates and performs a state transition for the LSP client.
    async fn transition(&self, to: LspClientState) -> Result<(), String> {
        let mut state = self.state.lock().await;
        let valid = match *state {
            LspClientState::Stopped => matches!(to, LspClientState::Starting),
            LspClientState::Starting => {
                matches!(to, LspClientState::Initializing | LspClientState::Crashed)
            }
            LspClientState::Initializing => {
                matches!(to, LspClientState::Ready | LspClientState::Crashed)
            }
            LspClientState::Ready => {
                matches!(to, LspClientState::ShuttingDown | LspClientState::Crashed)
            }
            LspClientState::ShuttingDown => matches!(to, LspClientState::Stopped),
            LspClientState::Crashed => matches!(to, LspClientState::Starting),
        };

        if valid {
            info!(language = %self.language_id, "LSP state: {:?} -> {:?}", *state, to);
            *state = to;
            Ok(())
        } else {
            let msg = format!(
                "Invalid LSP state transition for {}: {:?} -> {:?}",
                self.language_id, *state, to
            );
            error!("{}", msg);
            Err(msg)
        }
    }

    /// Returns the current state of the LSP client.
    pub async fn get_state(&self) -> LspClientState {
        *self.state.lock().await
    }

    /// Spawns the language server process and begins the initialization sequence.
    ///
    /// Transitions from `Stopped` to `Starting` to `Initializing`. If spawning
    /// fails, transitions to `Crashed`. A background task is spawned to read
    /// responses from the server's stdout.
    ///
    /// Returns `Ok(())` if the process was spawned successfully, or an error
    /// describing the failure.
    #[instrument(skip(self))]
    pub async fn start(&self) -> Result<(), String> {
        self.transition(LspClientState::Starting).await?;

        let mut process_guard = self.process.lock().await;

        if process_guard.is_some() {
            return Ok(());
        }

        let mut cmd = TokioCommand::new(&self.server_command);
        cmd.args(&self.server_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;

            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.as_std_mut().creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                let _ = self.transition(LspClientState::Crashed).await;
                return Err(format!(
                    "Failed to start language server {}: {}",
                    self.language_id, e
                ));
            }
        };

        let stdin = child.stdin.take().ok_or("Failed to get stdin")?;
        let stdout = child.stdout.take().ok_or("Failed to get stdout")?;

        if let Some(stderr) = child.stderr.take() {
            let lang_id = self.language_id.clone();
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    warn!(language = %lang_id, "LSP stderr: {}", line);
                }
            });
        }

        *self.writer.lock().await = Some(stdin);
        *process_guard = Some(child);
        // Release process lock before transitioning state (lock ordering: state -> process)
        drop(process_guard);

        info!(language = %self.language_id, command = %self.server_command, "Started language server");

        let pending_clone = Arc::clone(&self.pending_responses);
        let state_clone = Arc::clone(&self.state);
        let language_id_clone = self.language_id.clone();
        let notifications = self.notifications.clone();
        let server_requests = self.server_requests.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut header_buf = String::new();

            loop {
                header_buf.clear();
                let mut read_err = false;
                loop {
                    let mut byte = [0u8; 1];
                    if reader.read_exact(&mut byte).await.is_err() {
                        read_err = true;
                        break;
                    }
                    header_buf.push(byte[0] as char);

                    if header_buf.ends_with("\r\n\r\n") {
                        break;
                    }

                    if header_buf.len() > 4096 {
                        warn!("LSP header too long, disconnecting");
                        read_err = true;
                        break;
                    }
                }

                if read_err {
                    break;
                }

                let mut content_length: usize = 0;
                for line in header_buf.split("\r\n") {
                    if let Some(len_str) = line.strip_prefix("Content-Length: ") {
                        content_length = len_str.trim().parse().unwrap_or(0);
                    }
                }

                if content_length == 0 {
                    continue;
                }

                let mut body = vec![0u8; content_length];
                if reader.read_exact(&mut body).await.is_err() {
                    break;
                }

                let response: serde_json::Value = match serde_json::from_slice(&body) {
                    Ok(v) => v,
                    Err(e) => {
                        warn!("Failed to parse LSP response: {}", e);
                        continue;
                    }
                };

                if let Some(method) = response.get("method").and_then(|v| v.as_str()) {
                    if let Some(id) = response.get("id") {
                        let _ = server_requests.send(LspServerRequest {
                            id: id.clone(),
                            method: method.to_owned(),
                            params: response.get("params").cloned().unwrap_or_default(),
                        });
                    } else {
                        let _ = notifications.send(LspNotification {
                            method: method.to_owned(),
                            params: response.get("params").cloned().unwrap_or_default(),
                        });
                    }
                } else if let Some(id) = response.get("id").and_then(|v| v.as_u64()) {
                    let mut pending_guard = pending_clone.lock().await;
                    if let Some(sender) = pending_guard.remove(&id) {
                        if let Some(error) = response.get("error") {
                            let _ = sender.send(Err(format!("LSP error: {}", error)));
                        } else {
                            let _ = sender.send(Ok(response));
                        }
                    }
                }
            }

            // Read loop ended — transition to Crashed if still active
            {
                let mut state = state_clone.lock().await;
                if *state == LspClientState::Ready || *state == LspClientState::Initializing {
                    info!(language = %language_id_clone, "LSP read loop ended, state -> Crashed");
                    *state = LspClientState::Crashed;
                }
            }
            // Reject all pending requests when read loop ends
            let mut pending = pending_clone.lock().await;
            for (_, sender) in pending.drain() {
                let _ = sender.send(Err("LSP server connection closed".to_string()));
            }
        });

        self.transition(LspClientState::Initializing).await?;

        Ok(())
    }

    /// Gracefully shuts down the language server and releases resources.
    ///
    /// Sends the protocol shutdown request and exit notification when the client
    /// is ready. If the server does not exit promptly, it is force-killed.
    ///
    /// Returns `Ok(())` on success.
    pub async fn stop(&self) -> Result<(), String> {
        let state = self.get_state().await;
        if state == LspClientState::Stopped {
            return Ok(());
        }
        let was_ready = state == LspClientState::Ready;
        if was_ready {
            self.transition(LspClientState::ShuttingDown).await?;
        }

        let mut shutdown_error = None;
        if was_ready {
            if let Err(error) = self
                .send_request_timeout::<_, serde_json::Value>(
                    "shutdown",
                    serde_json::json!({}),
                    std::time::Duration::from_secs(2),
                )
                .await
            {
                shutdown_error = Some(format!("LSP shutdown request failed: {error}"));
            }
            if let Err(error) = self
                .send_notification("exit", serde_json::json!({}))
                .await
            {
                shutdown_error.get_or_insert_with(|| format!("LSP exit notification failed: {error}"));
            }
        }

        let mut process_guard = self.process.lock().await;
        if let Some(mut child) = process_guard.take() {
            let exited = if was_ready && shutdown_error.is_none() {
                matches!(
                    tokio::time::timeout(
                        std::time::Duration::from_secs(2),
                        child.wait()
                    )
                    .await,
                    Ok(Ok(_))
                )
            } else {
                false
            };
            if !exited {
                let _ = child.kill().await;
                let _ = child.wait().await;
            }
            info!(language = %self.language_id, "Stopped language server");
        }
        *self.writer.lock().await = None;
        self.open_documents.lock().await.clear();
        *self.state.lock().await = LspClientState::Stopped;
        if let Some(error) = shutdown_error {
            Err(error)
        } else {
            Ok(())
        }
    }

    /// Returns `true` if the client is in `Initializing` or `Ready` state.
    pub async fn is_running(&self) -> bool {
        let state = self.state.lock().await;
        matches!(*state, LspClientState::Initializing | LspClientState::Ready)
    }

    fn next_request_id(&self) -> u64 {
        REQUEST_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
    }

    /// Sends the LSP `initialize` request with client capabilities.
    ///
    /// - `root_uri` — The root URI of the workspace to initialize.
    ///
    /// Transitions to `Ready` on success, or `Crashed` on failure.
    /// Returns the [`InitializeResult`] from the server on success.
    #[instrument(skip(self))]
    pub async fn initialize(&self, root_uri: Url) -> Result<InitializeResult, String> {
        let params = InitializeParams {
            process_id: Some(std::process::id()),
            root_uri: Some(root_uri),
            capabilities: ClientCapabilities {
                workspace: Some(WorkspaceClientCapabilities {
                    apply_edit: Some(true),
                    configuration: Some(true),
                    workspace_edit: Some(WorkspaceEditClientCapabilities {
                        document_changes: Some(true),
                        failure_handling: Some(FailureHandlingKind::Transactional),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                text_document: Some(TextDocumentClientCapabilities {
                    hover: Some(HoverClientCapabilities {
                        dynamic_registration: Some(false),
                        content_format: Some(vec![MarkupKind::Markdown, MarkupKind::PlainText]),
                    }),
                    signature_help: Some(SignatureHelpClientCapabilities::default()),
                    references: Some(ReferenceClientCapabilities {
                        dynamic_registration: Some(false),
                        ..Default::default()
                    }),
                    completion: Some(CompletionClientCapabilities {
                        dynamic_registration: Some(false),
                        completion_item: Some(CompletionItemCapability {
                            snippet_support: Some(true),
                            commit_characters_support: Some(true),
                            documentation_format: Some(vec![
                                MarkupKind::Markdown,
                                MarkupKind::PlainText,
                            ]),
                            deprecated_support: Some(true),
                            preselect_support: Some(true),
                            insert_replace_support: Some(true),
                            ..Default::default()
                        }),
                        context_support: Some(true),
                        ..Default::default()
                    }),
                    definition: Some(GotoCapability {
                        dynamic_registration: Some(false),
                        link_support: Some(false),
                    }),
                    declaration: Some(GotoCapability {
                        dynamic_registration: Some(false),
                        link_support: Some(false),
                    }),
                    type_definition: Some(GotoCapability {
                        dynamic_registration: Some(false),
                        link_support: Some(false),
                    }),
                    implementation: Some(GotoCapability {
                        dynamic_registration: Some(false),
                        link_support: Some(false),
                    }),
                    formatting: Some(DocumentFormattingClientCapabilities {
                        dynamic_registration: Some(false),
                        ..Default::default()
                    }),
                    code_action: Some(CodeActionClientCapabilities {
                        dynamic_registration: Some(false),
                        code_action_literal_support: Some(CodeActionLiteralSupport {
                            code_action_kind: CodeActionKindLiteralSupport {
                                value_set: vec!["quickfix".to_owned()],
                            },
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let result = self.send_request("initialize", params).await;
        match &result {
            Ok(_) => {
                self.transition(LspClientState::Ready).await?;
            }
            Err(_) => {
                let _ = self.transition(LspClientState::Crashed).await;
            }
        }
        result
    }

    /// Sends a `textDocument/didOpen` notification to the language server.
    ///
    /// - `uri` — The URI of the document that was opened.
    /// - `language_id` — The language identifier for the document.
    /// - `version` — The document version assigned by the editor.
    /// - `text` — The full initial content of the document.
    pub async fn did_open(
        &self, uri: Url, language_id: String, version: i32, text: String,
    ) -> Result<(), String> {
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id,
                version,
                text,
            },
        };

        self.send_notification("textDocument/didOpen", params).await?;
        self.open_documents.lock().await.insert(uri);
        Ok(())
    }

    /// Sends a `textDocument/didChange` notification with the full document content.
    ///
    /// - `uri` — The URI of the document that changed.
    /// - `version` — The new version number of the document.
    /// - `text` — The full updated content of the document.
    pub async fn did_change(&self, uri: Url, version: i32, text: String) -> Result<(), String> {
        self.did_change_with_changes(
            uri,
            version,
            vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text,
            }],
        )
        .await
    }

    /// Sends incremental `textDocument/didChange` content changes.
    ///
    /// Changes are applied by the server in order; ranged changes must use positions
    /// relative to the document state after all preceding changes in this notification.
    pub async fn did_change_with_changes(
        &self,
        uri: Url,
        version: i32,
        content_changes: Vec<TextDocumentContentChangeEvent>,
    ) -> Result<(), String> {
        let params = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier { uri, version },
            content_changes,
        };

        self.send_notification("textDocument/didChange", params).await
    }

    /// Sends a `textDocument/didSave` notification to the language server.
    ///
    /// - `uri` — The URI of the document that was saved.
    pub async fn did_save(&self, uri: Url) -> Result<(), String> {
        let params =
            DidSaveTextDocumentParams { text_document: TextDocumentIdentifier { uri }, text: None };

        self.send_notification("textDocument/didSave", params).await
    }

    /// Sends a `textDocument/hover` request to retrieve hover information.
    ///
    /// - `uri` — The URI of the document.
    /// - `line` — The zero-based line number of the position.
    /// - `character` — The zero-based character offset of the position.
    ///
    /// Returns hover information if available, or `None` if the server
    /// provides no hover data at the given position.
    pub async fn hover(
        &self, uri: Url, line: u32, character: u32,
    ) -> Result<Option<Hover>, String> {
        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position { line, character },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        self.send_request("textDocument/hover", params).await
    }

    /// Requests signature information for a document position.
    pub async fn signature_help(
        &self,
        uri: Url,
        line: u32,
        character: u32,
    ) -> Result<Option<SignatureHelp>, String> {
        self.send_request(
            "textDocument/signatureHelp",
            SignatureHelpParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: Position { line, character },
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                context: None,
            },
        )
        .await
    }

    /// Requests the definition locations for a document position.
    pub async fn definition_locations(
        &self,
        uri: Url,
        line: u32,
        character: u32,
    ) -> Result<Vec<Location>, String> {
        self.goto_locations("textDocument/definition", uri, line, character)
            .await
    }

    /// Requests the declaration locations for a document position.
    pub async fn declaration_locations(
        &self,
        uri: Url,
        line: u32,
        character: u32,
    ) -> Result<Vec<Location>, String> {
        self.goto_locations("textDocument/declaration", uri, line, character)
            .await
    }

    /// Requests the type-definition locations for a document position.
    pub async fn type_definition_locations(
        &self,
        uri: Url,
        line: u32,
        character: u32,
    ) -> Result<Vec<Location>, String> {
        self.goto_locations("textDocument/typeDefinition", uri, line, character)
            .await
    }

    /// Requests the implementation locations for a document position.
    pub async fn implementation_locations(
        &self,
        uri: Url,
        line: u32,
        character: u32,
    ) -> Result<Vec<Location>, String> {
        self.goto_locations("textDocument/implementation", uri, line, character)
            .await
    }

    async fn goto_locations(
        &self,
        method: &str,
        uri: Url,
        line: u32,
        character: u32,
    ) -> Result<Vec<Location>, String> {
        let response: Option<serde_json::Value> = self
            .send_request(
                method,
                serde_json::json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": line, "character": character }
                }),
            )
            .await?;
        let Some(response) = response else {
            return Ok(Vec::new());
        };
        let response: GotoDefinitionResponse = serde_json::from_value(response)
            .map_err(|error| format!("Invalid definition response: {error}"))?;
        Ok(match response {
            GotoDefinitionResponse::Scalar(location) => vec![location],
            GotoDefinitionResponse::Array(locations) => locations,
            GotoDefinitionResponse::Link(links) => links
                .into_iter()
                .map(|link| Location {
                    uri: link.target_uri,
                    range: link.target_selection_range,
                })
                .collect(),
        })
    }

    /// Requests reference locations for a document position.
    pub async fn reference_locations(
        &self,
        uri: Url,
        line: u32,
        character: u32,
    ) -> Result<Vec<Location>, String> {
        let locations: Option<Vec<Location>> = self
            .send_request(
                "textDocument/references",
                serde_json::json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": line, "character": character },
                    "context": { "includeDeclaration": false }
                }),
            )
            .await?;
        Ok(locations.unwrap_or_default())
    }

    /// Requests completion items for a document position.
    pub async fn completion(
        &self,
        uri: Url,
        line: u32,
        character: u32,
        trigger: CompletionTriggerKind,
        trigger_character: Option<String>,
    ) -> Result<Option<CompletionResponse>, String> {
        self.send_request(
            "textDocument/completion",
            CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: Position { line, character },
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
                context: Some(CompletionContext {
                    trigger_kind: trigger,
                    trigger_character,
                }),
            },
        )
        .await
    }

    /// Requests whole-document formatting edits.
    pub async fn formatting(
        &self,
        uri: Url,
        options: FormattingOptions,
    ) -> Result<Option<Vec<TextEdit>>, String> {
        self.send_request(
            "textDocument/formatting",
            DocumentFormattingParams {
                text_document: TextDocumentIdentifier { uri },
                options,
                work_done_progress_params: WorkDoneProgressParams::default(),
            },
        )
        .await
    }

    /// Requests compiler-provided quick fixes for a diagnostic range.
    pub async fn code_actions(
        &self, uri: Url, range: Range, diagnostics: Vec<Diagnostic>,
    ) -> Result<Option<CodeActionResponse>, String> {
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier { uri },
            range,
            context: CodeActionContext {
                diagnostics,
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        self.send_request("textDocument/codeAction", params).await
    }

    /// Executes a command returned by a code action or another LSP request.
    pub async fn execute_command(
        &self,
        command: String,
        arguments: Vec<serde_json::Value>,
    ) -> Result<serde_json::Value, String> {
        self.send_request(
            "workspace/executeCommand",
            ExecuteCommandParams {
                command,
                arguments,
                work_done_progress_params: WorkDoneProgressParams::default(),
            },
        )
        .await
    }

    /// Sends a JSON-RPC request to the language server and waits for a response.
    ///
    /// Serializes the request, writes it to the server's stdin using the
    /// LSP Content-Length framing protocol, and waits up to 30 seconds for
    /// a matching response. Returns the deserialized result on success.
    async fn send_request<P: serde::Serialize, R: serde::de::DeserializeOwned>(
        &self, method: &str, params: P,
    ) -> Result<R, String> {
        self.send_request_timeout(method, params, std::time::Duration::from_secs(30))
            .await
    }

    async fn send_request_timeout<P: serde::Serialize, R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: P,
        timeout: std::time::Duration,
    ) -> Result<R, String> {
        let id = self.next_request_id();

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let body = serde_json::to_string(&request)
            .map_err(|error| format!("Failed to serialize LSP request: {error}"))?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());

        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending_responses.lock().await;
            pending.insert(id, tx);
        }

        let write_result = {
            let mut writer_guard = self.writer.lock().await;
            match writer_guard.as_mut() {
                Some(writer) => async {
                    writer
                        .write_all(header.as_bytes())
                        .await
                        .map_err(|error| format!("Failed to write LSP header: {error}"))?;
                    writer
                        .write_all(body.as_bytes())
                        .await
                        .map_err(|error| format!("Failed to write LSP body: {error}"))
                }
                .await,
                None => Err("LSP client stdin not available".to_owned()),
            }
        };
        if let Err(error) = write_result {
            self.pending_responses.lock().await.remove(&id);
            return Err(error);
        }

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(Ok(response))) => {
                if let Some(result) = response.get("result") {
                    serde_json::from_value(result.clone())
                        .map_err(|e| format!("Failed to parse LSP response: {}", e))
                } else if let Some(error) = response.get("error") {
                    Err(format!("LSP error: {}", error))
                } else {
                    Err("Invalid LSP response".to_string())
                }
            }
            Ok(Ok(Err(e))) => Err(e),
            Ok(Err(_)) => {
                // Channel closed -- server crashed or connection lost
                let mut pending = self.pending_responses.lock().await;
                pending.remove(&id);
                Err("LSP request channel closed (server may have crashed)".to_string())
            }
            Err(_) => {
                // Timeout -- server did not respond within 30 seconds
                let mut pending = self.pending_responses.lock().await;
                pending.remove(&id);
                Err(format!(
                    "LSP request timed out after {}s",
                    timeout.as_secs_f32()
                ))
            }
        }
    }

    /// Sends a JSON-RPC notification to the language server (no response expected).
    ///
    /// Serializes the notification and writes it to the server's stdin using
    /// the LSP Content-Length framing protocol.
    async fn send_notification<P: serde::Serialize>(
        &self, method: &str, params: P,
    ) -> Result<(), String> {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });

        let mut writer_guard = self.writer.lock().await;
        let writer = writer_guard.as_mut().ok_or("LSP client stdin not available")?;

        let body = serde_json::to_string(&notification)
            .map_err(|e| format!("Failed to serialize LSP notification: {}", e))?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        writer
            .write_all(header.as_bytes())
            .await
            .map_err(|e| format!("Failed to write LSP header: {}", e))?;
        writer
            .write_all(body.as_bytes())
            .await
            .map_err(|e| format!("Failed to write LSP body: {}", e))?;

        Ok(())
    }

    /// Sends a successful response to a request initiated by the language server.
    pub async fn respond(
        &self,
        id: serde_json::Value,
        result: serde_json::Value,
    ) -> Result<(), String> {
        self.send_json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }))
        .await
    }

    /// Sends a JSON-RPC error response to a request initiated by the language server.
    pub async fn respond_error(
        &self,
        id: serde_json::Value,
        code: i64,
        message: String,
    ) -> Result<(), String> {
        self.send_json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message },
        }))
        .await
    }

    async fn send_json(&self, message: &serde_json::Value) -> Result<(), String> {
        let body = serde_json::to_string(message)
            .map_err(|error| format!("Failed to serialize LSP response: {error}"))?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        let mut writer_guard = self.writer.lock().await;
        let writer = writer_guard.as_mut().ok_or("LSP client stdin not available")?;
        writer
            .write_all(header.as_bytes())
            .await
            .map_err(|error| format!("Failed to write LSP header: {error}"))?;
        writer
            .write_all(body.as_bytes())
            .await
            .map_err(|error| format!("Failed to write LSP response: {error}"))
    }

    /// Sends the LSP `textDocument/didClose` notification.
    pub async fn did_close(&self, uri: Url) -> Result<(), String> {
        self.send_notification(
            "textDocument/didClose",
            DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
            },
        )
        .await?;
        self.open_documents.lock().await.remove(&uri);
        Ok(())
    }

    /// Gracefully shuts down the language server, then waits or force-kills it.
    #[instrument(skip(self))]
    pub async fn shutdown(&self) -> Result<(), String> {
        self.stop().await
    }

    /// Sends the `initialized` notification to the language server.
    ///
    /// Must be called after `initialize()` to complete the handshake.
    pub async fn initialized(&self) -> Result<(), String> {
        self.send_notification("initialized", serde_json::json!({})).await
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        if let Ok(mut process) = self.process.try_lock() {
            if let Some(mut child) = process.take() {
                let _ = child.start_kill();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_lsp_client_new_state() {
        let client = LspClient::new(
            "rust".to_string(),
            "rust-analyzer".to_string(),
            vec![],
        );
        let state = client.get_state().await;
        assert_eq!(state, LspClientState::Stopped);
    }

    #[tokio::test]
    async fn test_lsp_client_state_transitions() {
        // Verify that LspClientState variants exist and PartialEq works correctly.
        // Valid transitions (per the state machine):
        //   Stopped -> Starting
        //   Starting -> Initializing | Crashed
        //   Initializing -> Ready | Crashed
        //   Ready -> ShuttingDown | Crashed
        //   ShuttingDown -> Stopped
        //   Crashed -> Starting

        assert!(LspClientState::Stopped != LspClientState::Starting);
        assert!(LspClientState::Starting != LspClientState::Initializing);
        assert!(LspClientState::Initializing != LspClientState::Ready);
        assert!(LspClientState::Ready != LspClientState::ShuttingDown);
        assert!(LspClientState::ShuttingDown != LspClientState::Stopped);
        assert!(LspClientState::Crashed != LspClientState::Starting);

        // Verify that a newly created client is in Stopped state
        let client = LspClient::new(
            "python".to_string(),
            "pyright-langserver".to_string(),
            vec!["--stdio".to_string()],
        );
        assert_eq!(client.get_state().await, LspClientState::Stopped);

        // Verify that attempting to stop from Stopped state is invalid
        // (Stopped -> ShuttingDown is not a valid transition)
        let result = client.transition(LspClientState::ShuttingDown).await;
        assert!(result.is_err());

        // Verify that Starting from Stopped is valid (though it will fail
        // because there's no actual process to spawn)
        // We can't fully test start() without a real server, but we can
        // verify the transition logic via the state machine.
    }

    #[tokio::test]
    async fn test_lsp_client_is_running_stopped() {
        let client = LspClient::new(
            "rust".to_string(),
            "rust-analyzer".to_string(),
            vec![],
        );
        // A newly created client is Stopped, not running
        assert!(!client.is_running().await);
    }

    #[tokio::test]
    async fn test_lsp_client_is_running_only_active_states() {
        // is_running returns true only for Initializing and Ready
        let client = LspClient::new(
            "rust".to_string(),
            "rust-analyzer".to_string(),
            vec![],
        );
        // Stopped -> not running
        assert!(!client.is_running().await);

        // Transition to Starting -> not running (Starting is not considered "running")
        client.transition(LspClientState::Starting).await.unwrap();
        assert!(!client.is_running().await);

        // Transition to Initializing -> now running
        client.transition(LspClientState::Initializing).await.unwrap();
        assert!(client.is_running().await);

        // Crashed -> not running
        client.transition(LspClientState::Crashed).await.unwrap();
        assert!(!client.is_running().await);
    }

    #[tokio::test]
    async fn test_lsp_client_invalid_transition_crashed_to_ready() {
        let client = LspClient::new(
            "rust".to_string(),
            "rust-analyzer".to_string(),
            vec![],
        );
        // Go to Crashed state
        client.transition(LspClientState::Starting).await.unwrap();
        client.transition(LspClientState::Crashed).await.unwrap();

        // Crashed -> Ready is invalid
        let result = client.transition(LspClientState::Ready).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_lsp_client_invalid_transition_stopped_to_initializing() {
        let client = LspClient::new(
            "rust".to_string(),
            "rust-analyzer".to_string(),
            vec![],
        );
        // Stopped -> Initializing is invalid (must go through Starting)
        let result = client.transition(LspClientState::Initializing).await;
        assert!(result.is_err());
    }
}