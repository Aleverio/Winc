use super::*;

pub(super) fn file_uri(path: &Path) -> Result<Url, String> {
    Url::from_file_path(path).map_err(|()| format!("Invalid LSP file path: {}", path.display()))
}

pub(super) fn hover_text(hover: Hover) -> String {
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
        let configured = !self.config.lsp_servers.is_empty()
            || !self.config.lsp_auto.is_empty()
            || !self.config.lsp_server_entries.is_empty()
            || self.config.lsp_detect != Some(false);
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
            let Some(server) = self.lsp_resolve_server(language, &root) else {
                continue;
            };
            let command = server.command.clone();
            let previous = self.lsp_sent.get(&doc.id);
            let same_target = previous.is_some_and(|sent| {
                sent.path == *path
                    && sent.language == language
                    && sent.command == command
                    && sent.env == server.env
                    && sent.cwd == server.cwd
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
                server.command,
                server.env,
                server.cwd,
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
            env,
            cwd,
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
                env: env.clone(),
                cwd: cwd.clone(),
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
                            env,
                            cwd,
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
        let resolve_root = self
            .editor
            .root()
            .filter(|root| path.starts_with(root))
            .or_else(|| path.parent())
            .unwrap_or(&path)
            .to_path_buf();
        let Some(server) = self.lsp_resolve_server(&language, &resolve_root) else {
            self.flash(format!("No LSP server is configured for {language}"));
            return;
        };
        let server_command = server.command;
        let env = server.env;
        let cwd = server.cwd;
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
            env,
            cwd,
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
        let resolve_root = self
            .editor
            .root()
            .filter(|root| path.starts_with(root))
            .or_else(|| path.parent())
            .unwrap_or(&path)
            .to_path_buf();
        let Some(server) = self.lsp_resolve_server(&language, &resolve_root) else {
            return false;
        };
        let server_command = server.command;
        let env = server.env;
        let cwd = server.cwd;
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
            env,
            cwd,
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
        let resolve_root = self
            .editor
            .root()
            .filter(|root| path.starts_with(root))
            .or_else(|| path.parent())
            .unwrap_or(&path)
            .to_path_buf();
        let Some(server) = self.lsp_resolve_server(&language, &resolve_root) else {
            self.lsp_code_actions_pending.remove(&key);
            self.lsp_code_actions.insert(key, Ok(Vec::new()));
            self.request_redraw();
            return;
        };
        let server_command = server.command;
        let env = server.env;
        let cwd = server.cwd;
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
                env,
                cwd,
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
        let resolve_root = self
            .editor
            .root()
            .filter(|root| path.starts_with(root))
            .or_else(|| path.parent())
            .unwrap_or(&path)
            .to_path_buf();
        let Some(server) = self.lsp_resolve_server(&language, &resolve_root) else {
            self.flash(format!("No LSP server is configured for {language}"));
            return;
        };
        let server_command = server.command;
        let env = server.env;
        let cwd = server.cwd;
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
                env,
                cwd,
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
