use std::collections::HashMap;

use lsp_types::{
    CompletionResponse, CompletionTextEdit, InsertTextFormat, Position, TextEdit, Url,
};
use ropey::Rope;

use super::*;

impl App {
    pub(in crate::app) fn request_lsp_completion(&mut self) {
        let Some((document, revision, caret, path, language, line, character)) =
            self.editor.active().and_then(|doc| {
                let path = doc.path.clone()?;
                let rope = doc.text().rope()?;
                let caret = doc.selection.head.min(rope.len_chars());
                let position = char_to_lsp_position(rope, caret).ok()?;
                Some((
                    doc.id,
                    doc.revision,
                    caret,
                    path,
                    doc.highlighter.language().id.to_owned(),
                    position.line,
                    position.character,
                ))
            })
        else {
            self.flash("Completion requires an open text file".to_owned());
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
        let command = server.command;
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
        self.lsp_completion = None;
        if let Err(error) = bridge.completion(CompletionRequest {
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
        }) {
            self.flash(format!("Could not request LSP completion: {error}"));
        }
    }

    pub(in crate::app) fn lsp_completion_received(&mut self, event: CompletionEvent) {
        let Some(doc) = self
            .editor
            .active()
            .filter(|doc| doc.id == event.document && doc.revision == event.revision)
            .filter(|doc| doc.selection.head == event.caret)
        else {
            return;
        };
        let items = match event.result {
            Ok(Some(CompletionResponse::Array(items))) => items,
            Ok(Some(CompletionResponse::List(list))) => list.items,
            Ok(None) => Vec::new(),
            Err(error) => {
                self.flash(format!("LSP completion failed: {error}"));
                return;
            }
        };
        let Some(rope) = doc.text().rope() else {
            return;
        };
        let prefix_start = winc_core::movement::word_at(doc.text(), event.caret).0;
        let prefix = rope.slice(prefix_start..event.caret).to_string();
        let (items, selected) = order_completion_items(items, &prefix);
        if items.is_empty() {
            self.lsp_completion = None;
            self.flash("No completion suggestions".to_owned());
            return;
        }
        self.lsp_completion = Some(CompletionPopup {
            document: event.document,
            revision: event.revision,
            caret: event.caret,
            items,
            selected,
        });
        self.request_redraw();
    }

    pub(in crate::app) fn move_lsp_completion(&mut self, down: bool) -> bool {
        let Some(popup) = &mut self.lsp_completion else {
            return false;
        };
        let len = popup.items.len();
        if len == 0 {
            self.lsp_completion = None;
            return true;
        }
        popup.selected = if down {
            (popup.selected + 1) % len
        } else {
            (popup.selected + len - 1) % len
        };
        self.request_redraw();
        true
    }

    pub(in crate::app) fn accept_lsp_completion(&mut self) -> bool {
        let Some(popup) = self.lsp_completion.take() else {
            return false;
        };
        let Some(item) = popup.items.get(popup.selected).cloned() else {
            return true;
        };
        let Some(doc) = self
            .editor
            .active()
            .filter(|doc| doc.id == popup.document && doc.revision == popup.revision)
            .filter(|doc| doc.selection.head == popup.caret)
        else {
            return true;
        };
        let Some(path) = doc.path.clone() else {
            return true;
        };
        let Some(rope) = doc.text().rope() else {
            return true;
        };
        let language = doc.highlighter.language().id.to_owned();
        let root = self
            .editor
            .root()
            .filter(|root| path.starts_with(root))
            .or_else(|| path.parent())
            .unwrap_or(&path)
            .to_path_buf();
        let start = winc_core::movement::word_at(doc.text(), popup.caret).0;
        let default_edit = || -> Result<TextEdit, String> {
            Ok(TextEdit::new(
                lsp_types::Range::new(
                    char_to_lsp_position(rope, start)?,
                    char_to_lsp_position(rope, popup.caret)?,
                ),
                String::new(),
            ))
        };
        let mut edit = match item.text_edit.clone() {
            Some(CompletionTextEdit::Edit(edit)) => edit,
            Some(CompletionTextEdit::InsertAndReplace(edit)) => {
                TextEdit::new(edit.insert, edit.new_text)
            }
            None => match default_edit() {
                Ok(edit) => edit,
                Err(error) => {
                    self.flash(format!("Invalid completion range: {error}"));
                    return true;
                }
            },
        };
        let snippet = item.insert_text_format == Some(InsertTextFormat::SNIPPET);
        if item.text_edit.is_none() {
            edit.new_text = item
                .insert_text
                .clone()
                .unwrap_or_else(|| item.label.clone());
        }
        if snippet {
            edit.new_text = expand_snippet(&edit.new_text);
        }
        let mut edits = item.additional_text_edits.unwrap_or_default();
        edits.push(edit);
        let uri = match Url::from_file_path(&path) {
            Ok(uri) => uri,
            Err(()) => {
                self.flash("Completion target has an invalid file path".to_owned());
                return true;
            }
        };
        let workspace_edit = lsp_types::WorkspaceEdit {
            changes: Some(HashMap::from([(uri, edits)])),
            ..Default::default()
        };
        if let Err(error) = self.apply_lsp_workspace_edit(workspace_edit) {
            self.flash(format!("Could not apply completion: {error}"));
            return true;
        }
        if let Some(command) = item.command {
            let Some(server) = self.lsp_resolve_server(&language, &root) else {
                self.flash(format!("No LSP server is configured for {language}"));
                return true;
            };
            let server_command = server.command;
            let env = server.env;
            let cwd = server.cwd;
            let Some(bridge) = self.lsp.clone() else {
                self.flash("LSP worker is unavailable".to_owned());
                return true;
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
                self.flash(format!("Could not run completion command: {error}"));
            }
        }
        self.request_redraw();
        true
    }
}

pub(super) fn char_to_lsp_position(rope: &Rope, character: usize) -> Result<Position, String> {
    if character > rope.len_chars() {
        return Err("Completion position is out of bounds".to_owned());
    }
    let line = rope.char_to_line(character);
    let line_start = rope.line_to_char(line);
    let line_text = rope.line(line);
    let mut content_end = line_text.len_chars();
    while content_end > 0 && matches!(line_text.char(content_end - 1), '\r' | '\n') {
        content_end -= 1;
    }
    let position = character.min(line_start + content_end);
    Ok(Position::new(
        line.min(u32::MAX as usize) as u32,
        rope.slice(line_start..position)
            .len_utf16_cu()
            .min(u32::MAX as usize) as u32,
    ))
}

fn order_completion_items(
    mut items: Vec<lsp_types::CompletionItem>,
    prefix: &str,
) -> (Vec<lsp_types::CompletionItem>, usize) {
    let folded_prefix = prefix.to_lowercase();
    items.retain(|item| {
        let filter = item.filter_text.as_deref().unwrap_or(&item.label);
        folded_prefix.is_empty() || filter.to_lowercase().starts_with(&folded_prefix)
    });
    items.sort_by(|left, right| {
        left.sort_text
            .as_deref()
            .unwrap_or(&left.label)
            .cmp(right.sort_text.as_deref().unwrap_or(&right.label))
            .then_with(|| left.label.cmp(&right.label))
    });
    let selected = items
        .iter()
        .position(|item| item.preselect.unwrap_or(false))
        .unwrap_or(0);
    (items, selected)
}

fn expand_snippet(snippet: &str) -> String {
    let characters: Vec<_> = snippet.chars().collect();
    let mut expanded = String::new();
    let mut index = 0;
    while index < characters.len() {
        match characters[index] {
            '\\' if characters
                .get(index + 1)
                .is_some_and(|c| matches!(c, '$' | '\\' | '}')) =>
            {
                expanded.push(characters[index + 1]);
                index += 2;
            }
            '$' if characters.get(index + 1).is_some_and(char::is_ascii_digit) => {
                index += 2;
            }
            '$' if characters.get(index + 1) == Some(&'{') => {
                let Some(end) = characters[index + 2..].iter().position(|c| *c == '}') else {
                    expanded.push('$');
                    index += 1;
                    continue;
                };
                let end = index + 2 + end;
                let content: String = characters[index + 2..end].iter().collect();
                if let Some((placeholder, default)) = content.split_once(':') {
                    if placeholder.chars().all(|c| c.is_ascii_digit()) {
                        expanded.push_str(default);
                    } else {
                        expanded.push_str(&content);
                    }
                } else if !content.chars().all(|c| c.is_ascii_digit()) {
                    expanded.push_str(&content);
                }
                index = end + 1;
            }
            character => {
                expanded.push(character);
                index += 1;
            }
        }
    }
    expanded
}

pub(crate) fn completion_documentation(item: &lsp_types::CompletionItem) -> String {
    let mut lines = Vec::new();
    if let Some(detail) = &item.detail {
        lines.push(detail.clone());
    }
    if let Some(documentation) = &item.documentation {
        match documentation {
            lsp_types::Documentation::String(text) => lines.push(text.clone()),
            lsp_types::Documentation::MarkupContent(markup) => lines.push(markup.value.clone()),
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippets_expand_placeholders_and_escaped_metacharacters() {
        assert_eq!(
            expand_snippet(r"fn ${1:name}($2) { \$0 }$0"),
            "fn name() { $0 }"
        );
    }

    #[test]
    fn completion_positions_count_utf16_code_units() {
        let rope = Rope::from_str("🦀\r\ncall");
        assert_eq!(char_to_lsp_position(&rope, 1).unwrap(), Position::new(0, 2));
        assert_eq!(char_to_lsp_position(&rope, 3).unwrap(), Position::new(1, 0));
        assert_eq!(char_to_lsp_position(&rope, 5).unwrap(), Position::new(1, 2));
    }

    #[test]
    fn completion_items_are_prefix_filtered_sorted_and_preselected() {
        let item = |label: &str, sort_text: &str, preselect: bool| lsp_types::CompletionItem {
            label: label.to_owned(),
            sort_text: Some(sort_text.to_owned()),
            preselect: Some(preselect),
            ..Default::default()
        };
        let (items, selected) = order_completion_items(
            vec![
                item("startLater", "2", true),
                item("other", "0", false),
                item("start", "1", false),
            ],
            "sta",
        );
        assert_eq!(
            items
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            ["start", "startLater"]
        );
        assert_eq!(selected, 1);
    }
}
