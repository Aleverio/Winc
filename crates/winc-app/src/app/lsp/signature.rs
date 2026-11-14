use super::*;

pub(in crate::app) fn signature_help_text(help: lsp_types::SignatureHelp) -> String {
    let active = help.active_signature.unwrap_or_default() as usize;
    let Some(signature) = help
        .signatures
        .get(active)
        .or_else(|| help.signatures.first())
    else {
        return String::new();
    };
    let mut text = signature.label.clone();
    if let Some(parameter) = signature.active_parameter.or(help.active_parameter) {
        text.push_str(&format!("  (parameter {})", parameter.saturating_add(1)));
    }
    if let Some(documentation) = &signature.documentation {
        let detail = match documentation {
            lsp_types::Documentation::String(text) => text.as_str(),
            lsp_types::Documentation::MarkupContent(markup) => markup.value.as_str(),
        };
        if !detail.trim().is_empty() {
            text.push_str(" — ");
            text.push_str(&detail.split_whitespace().collect::<Vec<_>>().join(" "));
        }
    }
    text
}

impl App {
    pub(in crate::app) fn request_lsp_signature_help(&mut self) {
        self.sync_lsp_documents();
        let Some((document, revision, path, language, position, caret)) =
            self.editor.active().and_then(|doc| {
                let path = doc.path.clone()?;
                let rope = doc.text().rope()?;
                let position = completion::char_to_lsp_position(rope, doc.selection.head).ok()?;
                Some((
                    doc.id,
                    doc.revision,
                    path,
                    doc.highlighter.language().id.to_owned(),
                    position,
                    doc.selection.head,
                ))
            })
        else {
            self.flash("Signature help requires an open text file".to_owned());
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
            line: position.line,
            character: position.character,
            document,
            revision,
            caret,
            kind: HoverRequestKind::SignatureHelp,
        }) {
            self.flash(format!("Could not request signature help: {error}"));
        }
    }

    pub(in crate::app) fn lsp_signature_help_received(
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
            Ok(_) => self.flash("No signature help available".to_owned()),
            Err(error) => self.flash(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_help_uses_active_signature_and_parameter() {
        let help: lsp_types::SignatureHelp = serde_json::from_value(serde_json::json!({
            "signatures": [
                { "label": "first()" },
                {
                    "label": "second(value: i32)",
                    "documentation": "The selected overload",
                    "activeParameter": 0
                }
            ],
            "activeSignature": 1
        }))
        .unwrap();
        assert_eq!(
            signature_help_text(help),
            "second(value: i32)  (parameter 1) — The selected overload"
        );
    }
}
