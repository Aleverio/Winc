//! 链接地址的补全列表：光标下方一张和窗口内菜单一样的卡片，每行是文件图标、文件名和淡色的
//! 所在目录，选中的一行铺强调色。放不下时改到光标上方。

use super::popup_menu::{MARGIN, ROW, ROW_INSET, band_ink};
use super::*;
use crate::app::{ICON_COLUMN, MenuIcon, POPUP_PAD};

/// 列表的最小、最大宽度（逻辑像素）。
const WIDTH: (f32, f32) = (240.0, 420.0);
/// 文件名之后到目录的间距（逻辑像素）。
const DIR_GAP: f32 = 8.0;

/// 绘制补全列表（开着、有文件时），记下卡片和各行的位置。
pub(super) fn draw_suggest(app: &mut App, frame: &mut Frame<'_>) {
    let suggest = &app.preview.markdown.suggest;
    let Some(caret) = app.ime.cursor else {
        return;
    };
    if suggest.at.is_none() || suggest.hits.is_empty() {
        return;
    }
    let rows: Vec<(String, String)> = (suggest.hits.iter())
        .map(|(path, _)| match path.rsplit_once('/') {
            Some((dir, name)) => (name.to_owned(), dir.to_owned()),
            None => (path.clone(), String::new()),
        })
        .collect();
    let (size, small) = (app.fonts.ui, app.fonts.ui_small);
    let text_x = ROW_INSET + 8.0 + ICON_COLUMN;
    let widest = (rows.iter())
        .map(|(name, dir)| {
            let dir = if dir.is_empty() {
                0.0
            } else {
                DIR_GAP + frame.measure(FontKind::Ui, small, dir)
            };
            frame.measure(FontKind::Ui, size, name) + dir
        })
        .fold(0.0, f32::max);
    let area = app.layout.editor;
    let w = (text_x + widest + 16.0)
        .clamp(WIDTH.0, WIDTH.1)
        .min(area.w - 2.0 * MARGIN);
    let h = 2.0 * POPUP_PAD + rows.len() as f32 * ROW;
    let x = (caret.x - text_x)
        .min(area.right() - MARGIN - w)
        .max(area.x + MARGIN);
    let below = caret.bottom() + 4.0;
    let y = if below + h <= area.bottom() - MARGIN {
        below
    } else {
        (caret.y - 4.0 - h).max(area.y + MARGIN)
    };
    let card = Rect::new(x.round(), y.round(), w.round(), h);
    let p = app.palette;
    draw_card(frame, p, card, RADIUS_POPUP, p.popup_bg);
    let clip = card.inset(0.0, POPUP_PAD);
    let selected = app.preview.markdown.suggest.selected;
    let mut hits = Vec::with_capacity(rows.len());
    for (i, (name, dir)) in rows.iter().enumerate() {
        let row = Rect::new(card.x, card.y + POPUP_PAD + i as f32 * ROW, card.w, ROW);
        let band = row.inset(ROW_INSET, 1.0);
        let (ink, dim) = if i == selected {
            frame.rect_clipped(band, p.accent_fill, RADIUS_ROW, clip);
            band_ink(p)
        } else {
            if app.pointer.hover == Hover::Suggest(i) {
                frame.rect_clipped(band, p.hover, RADIUS_ROW, clip);
            }
            (p.text, p.text_dim)
        };
        let (glyph, colors) = crate::theme::file_kind(name);
        let icon = MenuIcon::File(glyph, colors);
        let (iw, ih) = icon.size();
        let cy = icon_center_y(app, row, size);
        let at = (
            row.x + ROW_INSET + 8.0 + (ICON_COLUMN - 6.0 - iw) / 2.0,
            cy - ih / 2.0,
        );
        app.draw_menu_icon(frame, icon, at, clip);
        let x = row.x + text_x;
        let room = row.right() - 12.0 - x;
        let shown = ellipsize(frame, name, size, room.max(0.0));
        let end = ui_text(frame, app, x, row, &shown, ink, clip);
        let rest = row.right() - 12.0 - end - DIR_GAP;
        if !dir.is_empty() && rest > 24.0 {
            let dir = ellipsize(frame, dir, small, rest);
            ui_text_small(frame, app, end + DIR_GAP, row, &dir, dim, clip);
        }
        hits.push((row, i));
    }
    app.hits.suggest = hits;
    app.hits.suggest_card = Some(card);
}
