//! 竖向终端列表：各标签及其拆分窗格的分组、排序与悬停操作。

use super::*;
use crate::app::terminal::strip::{LIST_GAP, LIST_GROUP_GAP};
use crate::theme::TREE_ROW;

pub(super) fn draw_list(app: &mut App, frame: &mut Frame<'_>) {
    let Some(list) = app.terminal_tab_list() else {
        return;
    };
    let p = app.palette;
    let clip = list.intersect(app.layout.terminal);
    frame.rect_clipped(
        Rect::new(list.x, list.y, 1.0, list.h),
        p.separator,
        0.0,
        clip,
    );
    let Some(rows) = app.terminal_tab_rows() else {
        return;
    };
    let area = rows.inset(TAB_GAP, 0.0);
    let clip = area.intersect(clip);
    if area.w <= 0.0 || area.h <= 0.0 {
        return;
    }
    let t = &mut app.terminal;
    let step = TREE_ROW + LIST_GAP;
    t.strip.sizes.clear();
    let mut row_count = 0;
    for tab in &t.tabs {
        let pane_rows = tab.split.as_ref().map_or(0, |split| split.others.len() + 1);
        row_count += 1 + pane_rows;
        t.strip.sizes.push(TREE_ROW + pane_rows as f32 * step);
    }
    let current = t.tab().map(|tab| (tab.id, row_count, area.h));
    let active_row = t
        .tab()
        .and_then(|tab| tab.split.as_ref())
        .map_or(0, |split| split.at + 1);
    let strip = &mut t.strip;
    let gaps = strip.sizes.len().saturating_sub(1) as f32 * LIST_GROUP_GAP;
    strip.max_scroll = (strip.sizes.iter().sum::<f32>() + gaps - area.h).max(0.0);
    let mut scroll = strip.scroll;
    if strip.revealed != current {
        strip.revealed = current;
        let before: f32 = strip
            .sizes
            .iter()
            .take(t.active)
            .map(|h| h + LIST_GROUP_GAP)
            .sum();
        let top = before + active_row as f32 * step;
        scroll = scroll.min(top).max(top + TREE_ROW - area.h);
    }
    strip.scroll = scroll.clamp(0.0, strip.max_scroll);
    strip.start = area.y - drawn_scroll(strip.scroll);

    let mut rename = t.rename.take();
    let mut y = strip.start;
    let dragged = app.dragged_terminal_tab();
    let mut lifted = None;
    for i in 0..app.terminal.tabs.len() {
        let id = app.terminal.tabs[i].id;
        let h = app.terminal.strip.sizes[i];
        let group = Rect::new(area.x, y, area.w, h);
        y += h + LIST_GROUP_GAP;
        if let Some((_, y)) = dragged.filter(|(dragged, _)| *dragged == id) {
            lifted = Some((i, Rect { y, ..group }));
        } else {
            draw_group(app, frame, i, group, clip, rename.as_mut());
        }
    }
    if let Some((i, group)) = lifted {
        frame.rect_clipped(group, p.editor_bg, RADIUS_ROW, clip);
        draw_group(app, frame, i, group, clip, rename.as_mut());
    }
    app.terminal.rename = rename;
}

/// 单终端只有标签行。拆分组在标签下逐个列出窗格，顺序与面板中的排列一致。
fn draw_group(
    app: &mut App,
    frame: &mut Frame<'_>,
    index: usize,
    group: Rect,
    clip: Rect,
    rename: Option<&mut (u16, TextField)>,
) {
    let row = Rect {
        h: TREE_ROW,
        ..group
    };
    draw_row(app, frame, index, row, clip, rename);
    let tab = &app.terminal.tabs[index];
    if tab.pane_count() == 1 {
        return;
    }
    let p = app.palette;
    let indent = 17.0;
    let stem_x = group.x + 8.0;
    let branches = p.text_dim;
    let step = TREE_ROW + LIST_GAP;
    let stem_height = group.bottom() - row.bottom() - TREE_ROW / 2.0;
    frame.rect_clipped(
        Rect::new(stem_x, row.bottom(), 1.0, stem_height),
        branches,
        0.0,
        clip,
    );
    for (at, pane) in tab.split_order().into_iter().enumerate() {
        let row = Rect::new(
            group.x + indent,
            group.y + (at + 1) as f32 * step,
            (group.w - indent).max(0.0),
            TREE_ROW,
        );
        let hit = row.intersect(clip);
        if hit.w <= 0.0 || hit.h <= 0.0 {
            continue;
        }
        let branch = Rect::new(stem_x, center(row).1, row.x - stem_x - 2.0, 1.0);
        frame.rect_clipped(branch, branches, 0.0, clip);
        let active = index == app.terminal.active && pane.id == tab.id;
        let hovered = matches!(app.pointer.hover,
            Hover::TerminalHeader(HeaderHit::Pane(id) | HeaderHit::ClosePane(id)) if id == pane.id);
        draw_button(frame, p, row, clip, hovered, active);
        let close = Rect::new(
            row.right() - BUTTON_SMALL,
            row.y + (row.h - BUTTON_SMALL) / 2.0,
            BUTTON_SMALL,
            BUTTON_SMALL,
        );
        let text = Rect::new(row.x + 6.0, row.y, (close.x - row.x - 6.0).max(0.0), row.h);
        let label = format!("{}  {}", at + 1, pane.label());
        let label = truncate_end(frame, &label, app.fonts.ui, text.w);
        let color = if pane.ended.is_some() {
            p.text_dim
        } else if active {
            p.text
        } else {
            p.text_muted
        };
        ui_text(
            frame,
            app,
            text.x,
            text,
            &label,
            color,
            text.intersect(clip),
        );
        let on = app.pointer.hover == Hover::TerminalHeader(HeaderHit::ClosePane(pane.id));
        workspace::top_bar::draw_tab_close(frame, p, close, on, hit);
        app.hits
            .terminal
            .push((close.intersect(hit), HeaderHit::ClosePane(pane.id)));
        app.hits.terminal.push((hit, HeaderHit::Pane(pane.id)));
    }
}

fn draw_row(
    app: &mut App,
    frame: &mut Frame<'_>,
    i: usize,
    row: Rect,
    clip: Rect,
    rename: Option<&mut (u16, TextField)>,
) {
    let hit = row.intersect(clip);
    if hit.w <= 0.0 || hit.h <= 0.0 {
        app.hits.terminal.push((hit, HeaderHit::Tab(i)));
        return;
    }
    let p = app.palette;
    let tab = &app.terminal.tabs[i];
    let group = tab.pane_count() > 1;
    let active = i == app.terminal.active && !group;
    let hovered = matches!(app.pointer.hover, Hover::TerminalHeader(HeaderHit::Tab(j) | HeaderHit::Close(j)) if i == j);
    if active || hovered || group {
        let fill = if active {
            p.selected
        } else if hovered {
            p.hover
        } else {
            p.tab_fill
        };
        frame.rect_clipped(row, fill, RADIUS_ROW, clip);
    }
    let (inset, end) = if group {
        (22.0, 10.0)
    } else {
        (12.0, BUTTON_SMALL)
    };
    let text = Rect::new(row.x + inset, row.y, (row.w - inset - end).max(0.0), row.h);
    if let Some((_, field)) = rename.filter(|(id, _)| *id == tab.id) {
        let field_rect = row.inset(6.0, 2.0);
        let focused = app.focus == Focus::TerminalRename;
        let caret = frame.within(clip, |frame| {
            field_box(app, frame, field_rect, p.editor_bg, focused, false);
            draw_bar_field(app, frame, field_rect.inset(4.0, 0.0), field, "", focused)
        });
        if let Some(caret) = caret
            .map(|caret| caret.intersect(clip))
            .filter(|caret| caret.w > 0.0 && caret.h > 0.0)
        {
            app.ime.cursor = Some(caret);
        }
    } else {
        let (title, color) = if group {
            let icon = (row.x + 11.0, center(row).1);
            icons::split_scaled(frame, icon, 0.75, p.text_muted, hit);
            (t!("terminal-split-group"), p.text_muted)
        } else if tab.ended.is_some() {
            (tab.label(), p.text_dim)
        } else if active {
            (tab.label(), p.text)
        } else {
            (tab.label(), p.text_muted)
        };
        let label = truncate_end(frame, title, app.fonts.ui, text.w);
        ui_text(
            frame,
            app,
            text.x,
            text,
            &label,
            color,
            text.intersect(clip),
        );
        let dot = match tab.unseen {
            Some(true) => Some(p.error),
            Some(false) => Some(p.accent),
            None => tab.busy().map(|_| p.text_dim),
        };
        if let Some(color) = dot {
            let x = if group {
                row.right() - 8.0
            } else {
                row.x + 5.0
            };
            frame.rect_clipped(Rect::new(x, center(row).1 - 2.0, 4.0, 4.0), color, 2.0, hit);
        }
        if hovered && !group {
            let close = Rect::new(
                row.right() - BUTTON_SMALL,
                row.y + (row.h - BUTTON_SMALL) / 2.0,
                BUTTON_SMALL,
                BUTTON_SMALL,
            );
            let on = app.pointer.hover == Hover::TerminalHeader(HeaderHit::Close(i));
            workspace::top_bar::draw_tab_close(frame, p, close, on, clip);
            app.hits
                .terminal
                .push((close.intersect(clip), HeaderHit::Close(i)));
        }
    }
    app.hits.terminal.push((hit, HeaderHit::Tab(i)));
}
