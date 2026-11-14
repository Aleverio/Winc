use super::*;
use crate::app::editor::definition::{Place, PlacesKind};

impl App {
    pub(in crate::app) fn request_lsp_navigation(&mut self, kind: NavigationKind) -> bool {
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
            return false;
        };
        let command = if self.config.lsp_auto.iter().any(|id| id == &language) {
            auto_server_command(&language)
        } else {
            self.config.lsp_servers.get(&language).cloned()
        };
        let Some(command) = command else {
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
            return false;
        };
        match bridge.navigation(NavigationRequest {
            language,
            command,
            root,
            path,
            line: position.line,
            character: position.character,
            document,
            revision,
            caret,
            kind,
        }) {
            Ok(()) => true,
            Err(error) => {
                self.flash(format!("Could not request LSP navigation: {error}"));
                false
            }
        }
    }

    pub(in crate::app) fn lsp_navigation_received(&mut self, event: NavigationEvent) {
        let still_at_request = self.editor.active().is_some_and(|doc| {
            doc.id == event.document
                && doc.revision == event.revision
                && doc.selection.head == event.caret
        });
        if !still_at_request {
            return;
        }
        let name = self
            .name_span()
            .and_then(|(start, end)| {
                self.editor
                    .active()
                    .and_then(|doc| doc.text_range(start, end))
            })
            .unwrap_or_default();
        let locations = match event.result {
            Ok(locations) => locations,
            Err(_) => {
                self.fallback_navigation(event.kind);
                return;
            }
        };
        let current_line = self.editor.active().and_then(|doc| {
            (doc.id == event.document).then(|| doc.text().pos_to_line_col(event.caret).0)
        });
        if event.kind == NavigationKind::Definition
            && self.editor.active().is_some_and(|doc| {
                locations.iter().any(|location| {
                    location.uri.to_file_path().ok().as_ref() == doc.path.as_ref()
                        && Some(location.range.start.line as usize) == current_line
                })
            })
        {
            self.fallback_navigation(event.kind);
            return;
        }
        let root = self.editor.root().map(Path::to_path_buf);
        let items: Vec<_> = locations
            .into_iter()
            .filter_map(|location| {
                let path = location.uri.to_file_path().ok()?;
                let line = location.range.start.line as usize;
                let relative = root
                    .as_deref()
                    .and_then(|root| path.strip_prefix(root).ok())
                    .unwrap_or(&path)
                    .display()
                    .to_string();
                Some(Place {
                    path,
                    relative,
                    line,
                    col: None,
                    preview: format!("Line {}", line.saturating_add(1)),
                    marks: Vec::new(),
                })
            })
            .collect();
        match items.len() {
            1 if event.kind != NavigationKind::References => self.open_place(&items[0], &name),
            0 => self.fallback_navigation(event.kind),
            _ if event.kind == NavigationKind::References => {
                self.show_places(PlacesKind::Usages, name, items);
            }
            _ => self.show_places(PlacesKind::Definitions, name, items),
        }
    }

    fn fallback_navigation(&mut self, kind: NavigationKind) {
        match kind {
            NavigationKind::Definition
            | NavigationKind::Declaration
            | NavigationKind::TypeDefinition
            | NavigationKind::Implementation => self.go_to_definition(),
            NavigationKind::References => self.find_references(),
        }
    }
}
