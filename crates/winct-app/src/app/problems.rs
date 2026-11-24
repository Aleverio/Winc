use super::*;

/// 底部 Problems 面板的状态。
#[derive(Default)]
pub(crate) struct ProblemsPanel {
    /// Problems 面板当前已选中；与终端面板互斥。
    pub open: bool,
    /// 列表顶部的滚动位置（逻辑像素）。
    pub scroll: f32,
}

pub(crate) struct ProblemEntry {
    pub document: DocumentId,
    pub diagnostic: usize,
    pub file: String,
    pub line: u32,
    pub character: u32,
    pub severity: Option<lsp_types::DiagnosticSeverity>,
    pub message: String,
}

impl App {
    pub(in crate::app) fn toggle_problems(&mut self) {
        if self.problems.open {
            self.hide_bottom_panel();
        } else {
            self.select_problems_panel();
        }
    }

    pub(in crate::app) fn select_problems_panel(&mut self) {
        if self.problems.open {
            return;
        }
        let from = self.terminal_shown().0;
        self.finish_terminal_rename(true);
        if self.terminal.height <= 0.0 {
            self.terminal.height = terminal::DEFAULT_HEIGHT;
        }
        self.terminal.open = false;
        self.problems.open = true;
        self.problems.scroll = 0.0;
        if self.focus.in_terminal() {
            self.focus = Focus::Editor;
        }
        self.animate_terminal_panel(from);
        self.relayout();
        self.request_redraw();
    }

    pub(in crate::app) fn hide_bottom_panel(&mut self) {
        if !self.terminal.open && !self.problems.open {
            return;
        }
        let from = self.terminal_shown().0;
        self.terminal.open = false;
        self.problems.open = false;
        self.finish_terminal_rename(true);
        if self.focus.in_terminal() {
            self.focus = Focus::Editor;
        }
        self.animate_terminal_panel(from);
        self.relayout();
        self.request_redraw();
    }

    pub(crate) fn problem_entries(&self) -> Vec<ProblemEntry> {
        let mut entries = Vec::new();
        for doc in &self.editor.documents {
            let Some(path) = doc.path.as_ref() else {
                continue;
            };
            let Some(diagnostics) = self.lsp_diagnostics.get(path) else {
                continue;
            };
            for (index, diagnostic) in diagnostics.iter().enumerate() {
                entries.push(ProblemEntry {
                    document: doc.id,
                    diagnostic: index,
                    file: doc.name().into_owned(),
                    line: diagnostic.range.start.line,
                    character: diagnostic.range.start.character,
                    severity: diagnostic.severity,
                    message: diagnostic
                        .message
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" "),
                });
            }
        }
        entries.sort_by(|a, b| {
            severity_rank(a.severity)
                .cmp(&severity_rank(b.severity))
                .then_with(|| a.file.cmp(&b.file))
                .then_with(|| a.line.cmp(&b.line))
                .then_with(|| a.character.cmp(&b.character))
        });
        entries
    }

    pub(in crate::app) fn open_problem(&mut self, document: DocumentId, index: usize) {
        let Some((tab, (line, column))) = self
            .editor
            .documents
            .iter()
            .enumerate()
            .find(|(_, doc)| doc.id == document)
            .and_then(|(tab, doc)| {
                let path = doc.path.as_ref()?;
                let diagnostic = self.lsp_diagnostics.get(path)?.get(index)?;
                let rope = doc.text().rope()?;
                let (line, column) =
                    crate::app::lsp::position_to_line_col(rope, diagnostic.range.start).ok()?;
                Some((tab, (line, column)))
            })
        else {
            return;
        };
        self.activate_tab(tab);
        self.jump_to(line, column, 0);
    }

    pub(in crate::app) fn scroll_problems(&mut self, delta: f32) {
        self.problems.scroll = (self.problems.scroll - delta).max(0.0);
        self.request_redraw();
    }
}

fn severity_rank(severity: Option<lsp_types::DiagnosticSeverity>) -> u8 {
    use lsp_types::DiagnosticSeverity as Severity;
    match severity {
        Some(Severity::ERROR) => 0,
        Some(Severity::WARNING) => 1,
        Some(Severity::INFORMATION) => 2,
        Some(Severity::HINT) => 3,
        _ => 4,
    }
}
