use std::collections::HashMap;
use std::path::Path;

use ropey::Rope;

use super::*;

type WorkspaceTextEdits = HashMap<PathBuf, (Option<i32>, Vec<lsp_types::TextEdit>)>;

impl App {
    pub(in crate::app) fn apply_lsp_workspace_edit(
        &mut self,
        edit: lsp_types::WorkspaceEdit,
    ) -> Result<(), String> {
        let edits = workspace_text_edits(edit)?;
        let mut transactions = Vec::new();
        for (path, (version, edits)) in edits {
            let Some(document) = self
                .editor
                .documents
                .iter()
                .find(|document| document.path.as_ref() == Some(&path))
            else {
                return Err(format!(
                    "Workspace edit targets a file that is not open: {}",
                    path.display()
                ));
            };
            if let Some(transaction) = lsp_edit_transaction(document, version, &edits)? {
                transactions.push((document.id, transaction));
            }
        }

        for (id, transaction) in transactions {
            let Some(document) = self.editor.document_mut(id) else {
                return Err("A document closed while applying a workspace edit".to_owned());
            };
            document.apply(transaction);
        }
        self.schedule_dirty_auto_saves();
        self.request_redraw();
        Ok(())
    }
}

fn workspace_text_edits(edit: lsp_types::WorkspaceEdit) -> Result<WorkspaceTextEdits, String> {
    use lsp_types::{DocumentChangeOperation, DocumentChanges, OneOf};

    if edit.changes.is_some() && edit.document_changes.is_some() {
        return Err("Workspace edit has both `changes` and `documentChanges`".to_owned());
    }
    let annotations = edit.change_annotations.unwrap_or_default();
    let mut documents = HashMap::new();
    if let Some(changes) = edit.changes {
        for (uri, edits) in changes {
            let path = uri
                .to_file_path()
                .map_err(|()| format!("Workspace edit has a non-file URI: {uri}"))?;
            if documents.insert(path, (None, edits)).is_some() {
                return Err("Workspace edit contains multiple edits for the same file".to_owned());
            }
        }
    }
    if let Some(changes) = edit.document_changes {
        let text_documents = match changes {
            DocumentChanges::Edits(documents) => documents,
            DocumentChanges::Operations(operations) => operations
                .into_iter()
                .map(|operation| match operation {
                    DocumentChangeOperation::Edit(document) => Ok(document),
                    DocumentChangeOperation::Op(_) => {
                        Err("Workspace edit contains unsupported file operations".to_owned())
                    }
                })
                .collect::<Result<Vec<_>, _>>()?,
        };
        for document in text_documents {
            let path = document
                .text_document
                .uri
                .to_file_path()
                .map_err(|()| "Workspace edit has a non-file URI".to_owned())?;
            let edits = document
                .edits
                .into_iter()
                .map(|edit| match edit {
                    OneOf::Left(edit) => Ok(edit),
                    OneOf::Right(edit) => {
                        if annotations
                            .get(&edit.annotation_id)
                            .is_some_and(|annotation| annotation.needs_confirmation == Some(true))
                        {
                            return Err(format!(
                                "Workspace edit requires confirmation: {}",
                                edit.annotation_id
                            ));
                        }
                        Ok(edit.text_edit)
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            if documents
                .insert(path, (document.text_document.version, edits))
                .is_some()
            {
                return Err("Workspace edit contains multiple edits for the same file".to_owned());
            }
        }
    }
    Ok(documents)
}

fn lsp_edit_transaction(
    document: &winc_editor::Document,
    version: Option<i32>,
    edits: &[lsp_types::TextEdit],
) -> Result<Option<winc_core::edit::Transaction>, String> {
    use winc_core::Selection;
    use winc_core::edit::{Change, EditKind, Transaction};

    if let Some(version) = version
        && version != document.revision.min(i32::MAX as u64) as i32
    {
        return Err(format!(
            "Workspace edit is stale for {} (server version {version}, current version {})",
            document
                .path
                .as_deref()
                .unwrap_or_else(|| Path::new(""))
                .display(),
            document.revision
        ));
    }
    if let Some(reason) = document.readonly_reason() {
        return Err(format!(
            "Workspace edit cannot modify this document: {reason}"
        ));
    }
    let Some(rope) = document.text().rope() else {
        return Err("Workspace edit cannot modify a large or non-text document".to_owned());
    };

    let mut converted = edits
        .iter()
        .map(|edit| {
            let start = lsp_position_to_char(rope, edit.range.start)?;
            let end = lsp_position_to_char(rope, edit.range.end)?;
            if start > end {
                return Err("Workspace edit has a reversed range".to_owned());
            }
            let unified = edit.new_text.replace("\r\n", "\n").replace('\r', "\n");
            let inserted = if document.line_ending.as_str() == "\n" {
                unified
            } else {
                unified.replace('\n', document.line_ending.as_str())
            };
            Ok((start, end, inserted))
        })
        .collect::<Result<Vec<_>, String>>()?;
    if converted.is_empty() {
        return Ok(None);
    }
    converted.sort_by_key(|(start, end, _)| (*start, *end));
    if converted
        .windows(2)
        .any(|pair| pair[1].0 < pair[0].1 || pair[1].0 == pair[0].0)
    {
        return Err("Workspace edit contains overlapping or ambiguous changes".to_owned());
    }

    let selection = document.selection;
    let map_position = |position: usize| {
        let mut delta = 0isize;
        for (start, end, inserted) in &converted {
            if position < *start {
                break;
            }
            let new_start = (*start as isize + delta).max(0) as usize;
            if position <= *end {
                return new_start + inserted.chars().count();
            }
            delta += inserted.chars().count() as isize - (*end - *start) as isize;
        }
        (position as isize + delta).max(0) as usize
    };
    let after = Selection::new(map_position(selection.anchor), map_position(selection.head));
    let changes = converted
        .iter()
        .rev()
        .map(|(start, end, inserted)| Change {
            start: *start,
            removed: rope.slice(*start..*end).to_string(),
            inserted: inserted.clone(),
        })
        .collect();
    Ok(Some(Transaction::new(
        changes,
        selection,
        after,
        EditKind::Other,
    )))
}

pub(super) fn lsp_position_to_char(
    rope: &Rope,
    position: lsp_types::Position,
) -> Result<usize, String> {
    let line_index = position.line as usize;
    if line_index >= rope.len_lines() {
        return Err(format!(
            "Workspace edit line {} is out of bounds",
            position.line
        ));
    }
    let line = rope.line(line_index);
    let mut content_end = line.len_chars();
    while content_end > 0 && matches!(line.char(content_end - 1), '\r' | '\n') {
        content_end -= 1;
    }
    let content = line.slice(..content_end);
    let target = position.character as usize;
    let mut units = 0;
    for (index, character) in content.chars().enumerate() {
        if units == target {
            return Ok(rope.line_to_char(line_index) + index);
        }
        let next = units + character.len_utf16();
        if target < next {
            return Err("Workspace edit position splits a UTF-16 character".to_owned());
        }
        units = next;
    }
    if units == target {
        Ok(rope.line_to_char(line_index) + content.len_chars())
    } else {
        Err(format!(
            "Workspace edit character {} is out of bounds on line {}",
            position.character, position.line
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{Position, Range};

    #[test]
    fn lsp_edit_positions_use_utf16_and_reject_split_surrogates() {
        let rope = Rope::from_str("A🦀\r\nend");
        assert_eq!(lsp_position_to_char(&rope, Position::new(0, 3)).unwrap(), 2);
        assert!(lsp_position_to_char(&rope, Position::new(0, 2)).is_err());
        assert_eq!(lsp_position_to_char(&rope, Position::new(1, 1)).unwrap(), 5);
    }

    #[test]
    fn lsp_workspace_text_edits_preserve_crlf_and_validate_versions() {
        let mut document = winc_editor::Document::from_text(winc_editor::DocumentId(1), "a\r\nb");
        document.line_ending = winc_core::LineEnding::CrLf;
        let edits = [lsp_types::TextEdit::new(
            Range::new(Position::new(0, 1), Position::new(1, 0)),
            "x\n".to_owned(),
        )];
        let transaction = lsp_edit_transaction(&document, Some(0), &edits)
            .unwrap()
            .unwrap();
        let mut rope = document.text().rope().unwrap().clone();
        transaction.apply(&mut rope);
        assert_eq!(rope.to_string(), "ax\r\nb");

        assert!(lsp_edit_transaction(&document, Some(1), &edits).is_err());
    }

    #[test]
    fn lsp_workspace_text_edits_reject_overlapping_ranges() {
        let document = winc_editor::Document::from_text(winc_editor::DocumentId(1), "abc");
        let edits = [
            lsp_types::TextEdit::new(
                Range::new(Position::new(0, 0), Position::new(0, 2)),
                "x".to_owned(),
            ),
            lsp_types::TextEdit::new(
                Range::new(Position::new(0, 1), Position::new(0, 3)),
                "y".to_owned(),
            ),
        ];
        assert!(lsp_edit_transaction(&document, None, &edits).is_err());
    }
}
