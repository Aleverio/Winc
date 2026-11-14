use super::*;
use lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};

#[test]
fn problems_panel_uses_lsp_diagnostics_without_starting_a_terminal() {
    let dir = sample_folder("problems-panel");
    let path = dir.join("src").join("main.rs");
    let mut app = app_with(&path, true);
    let document = app.editor.active().unwrap().id;
    app.lsp_diagnostics.insert(
        path,
        vec![Diagnostic {
            range: Range::new(Position::new(1, 12), Position::new(1, 17)),
            severity: Some(DiagnosticSeverity::ERROR),
            code: None,
            code_description: None,
            source: Some("test".to_owned()),
            message: "expected expression".to_owned(),
            related_information: None,
            tags: None,
            data: None,
        }],
    );

    app.toggle_problems();
    assert!(app.problems.open);
    assert!(!app.terminal.open);
    assert!(app.terminal.tabs.is_empty());
    assert!(app.layout.terminal.h >= crate::app::terminal::DEFAULT_HEIGHT);
    paint(&mut app);

    let (row, (hit_document, diagnostic)) = app.hits.problems[0];
    assert_eq!((hit_document, diagnostic), (document, 0));
    click(&mut app, (row.x + 12.0, row.y + row.h / 2.0));

    let active = app.editor.active().unwrap();
    assert_eq!(active.id, document);
    assert_eq!(active.selection.head, active.text().line_col_to_pos(1, 12));
    assert!(app.problems.open);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn problems_tab_switches_the_bottom_panel_without_starting_a_terminal() {
    let dir = sample_folder("problems-tab");
    let path = dir.join("src").join("main.rs");
    let mut app = app_with(&path, true);
    app.terminal.open = true;
    app.relayout();
    paint(&mut app);

    let tab = app
        .hits
        .terminal
        .iter()
        .find(|(_, hit)| *hit == crate::app::terminal::HeaderHit::ProblemsView)
        .map(|(rect, _)| *rect)
        .unwrap();
    click(&mut app, (tab.x + tab.w / 2.0, tab.y + tab.h / 2.0));

    assert!(app.problems.open);
    assert!(!app.terminal.open);
    assert!(app.terminal.tabs.is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn problems_are_sorted_by_severity_then_location() {
    let dir = sample_folder("problems-order");
    let first = dir.join("src/main.rs");
    let second = dir.join("short.rs");
    let mut app = app_with(&first, false);
    app.open_path(&second);
    app.lsp_diagnostics.insert(
        first,
        vec![Diagnostic {
            range: Range::new(Position::new(0, 0), Position::new(0, 1)),
            severity: Some(DiagnosticSeverity::WARNING),
            code: None,
            code_description: None,
            source: None,
            message: "warning".to_owned(),
            related_information: None,
            tags: None,
            data: None,
        }],
    );
    app.lsp_diagnostics.insert(
        second,
        vec![Diagnostic {
            range: Range::new(Position::new(0, 0), Position::new(0, 1)),
            severity: Some(DiagnosticSeverity::ERROR),
            code: None,
            code_description: None,
            source: None,
            message: "error".to_owned(),
            related_information: None,
            tags: None,
            data: None,
        }],
    );

    let entries = app.problem_entries();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(entries[1].severity, Some(DiagnosticSeverity::WARNING));
    std::fs::remove_dir_all(&dir).unwrap();
}
