use super::*;
use crate::app::Hover;
use crate::app::terminal::header_height;
use crate::theme::TREE_ROW;

const PAD: f32 = 8.0;

pub(super) fn draw(app: &mut App, frame: &mut Frame<'_>, card: Rect) {
    let p = app.palette;
    let top = card.y + header_height(card);
    let body = Rect::new(
        card.x + PAD,
        top + 4.0,
        (card.w - PAD * 2.0).max(0.0),
        (card.bottom() - top - 4.0).max(0.0),
    );
    let entries = app.problem_entries();
    let content_height = entries.len() as f32 * TREE_ROW;
    let max_scroll = (content_height - body.h).max(0.0);
    app.problems.scroll = app.problems.scroll.clamp(0.0, max_scroll);
    app.hits.problems.clear();

    if entries.is_empty() {
        ui_text(
            frame,
            app,
            body.x + 8.0,
            body,
            t!("problems-empty"),
            p.text_muted,
            body,
        );
        return;
    }

    let location_x = body.x + 28.0;
    for (index, entry) in entries.iter().enumerate() {
        let row = Rect::new(
            body.x,
            body.y + index as f32 * TREE_ROW - app.problems.scroll,
            body.w,
            TREE_ROW,
        );
        let visible = row.intersect(body);
        if visible.h <= 0.0 {
            continue;
        }
        let hover = app.pointer.hover == Hover::Problem(entry.document, entry.diagnostic);
        if hover {
            frame.rect_clipped(row, p.hover, RADIUS_ROW, body);
        }
        let (mark, color) = match entry.severity {
            Some(lsp_types::DiagnosticSeverity::ERROR) => ("E", p.error),
            Some(lsp_types::DiagnosticSeverity::WARNING) => ("W", p.warning),
            Some(lsp_types::DiagnosticSeverity::INFORMATION) => ("I", p.accent),
            Some(lsp_types::DiagnosticSeverity::HINT) => ("H", p.text_muted),
            _ => ("·", p.text_muted),
        };
        let badge = Rect::new(row.x + 6.0, row.y + (TREE_ROW - 14.0) / 2.0, 14.0, 14.0);
        frame.rect_clipped(badge, color.alpha(0.16), 3.0, body);
        ui_text(frame, app, badge.x + 3.0, badge, mark, color, body);

        let location = format!(
            "{}:{}:{}",
            entry.file,
            entry.line.saturating_add(1),
            entry.character.saturating_add(1)
        );
        let location_width = frame
            .measure(FontKind::Ui, app.fonts.ui, &location)
            .min(body.w * 0.42);
        let location_rect = Rect::new(location_x, row.y, location_width, row.h);
        ui_text(
            frame,
            app,
            location_x,
            location_rect,
            &location,
            p.text_muted,
            body,
        );
        let message_x = location_rect.right() + 10.0;
        let message = Rect::new(message_x, row.y, (body.right() - message_x).max(0.0), row.h);
        ui_text(frame, app, message.x, message, &entry.message, p.text, body);
        app.hits
            .problems
            .push((visible, (entry.document, entry.diagnostic)));
    }
}
