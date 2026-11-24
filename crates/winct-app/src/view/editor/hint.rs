//! 名字旁边的提示气泡：一张小卡片，带一个指向名字的小三角。
//!
//! 气泡在名字所在行的上方，上方放不下时在下方；出现时向名字那一侧滑出并淡入，消失时原地淡出，
//! 与位置浮窗的过渡一致。

use winct_render::Stroke;

use super::*;
use crate::app::hits::DiagnosticPopupHit;

/// 气泡的高度（逻辑像素）。
const HEIGHT: f32 = 34.0;
/// 文字左右的留白（逻辑像素）。
const PAD: f32 = 14.0;
/// 小三角的高度（逻辑像素），底边宽为它的两倍。
const POINTER: f32 = 6.0;
/// 小三角尖端与名字所在行之间的距离（逻辑像素）。
const GAP: f32 = 2.0;
/// 出现时滑动的距离（逻辑像素）。
const SLIDE: f32 = 3.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DiagnosticDetailKind {
    Heading,
    Text,
    Suggestion,
    Link,
    Code,
    CodeMarker,
}

struct DiagnosticDetail {
    kind: DiagnosticDetailKind,
    text: String,
}

struct DiagnosticDetailRow {
    kind: DiagnosticDetailKind,
    text: String,
    y: f32,
    h: f32,
    action: Option<usize>,
}

fn wrap_diagnostic_text(
    frame: &mut Frame<'_>,
    app: &App,
    kind: DiagnosticDetailKind,
    text: &str,
    max_width: f32,
) -> Vec<String> {
    if matches!(
        kind,
        DiagnosticDetailKind::Code | DiagnosticDetailKind::CodeMarker
    ) {
        let chars = text.chars().collect::<Vec<_>>();
        let prefix_len = text
            .find("| ")
            .map_or(0, |index| text[..index + 2].chars().count());
        let prefix = chars[..prefix_len].iter().collect::<String>();
        let continuation = " ".repeat(prefix_len);
        let mut lines = Vec::new();
        let mut content = String::new();
        for character in chars.into_iter().skip(prefix_len) {
            let candidate = format!("{content}{character}");
            let indent = if lines.is_empty() {
                &prefix
            } else {
                &continuation
            };
            if !content.is_empty()
                && frame.measure(
                    FontKind::Mono,
                    app.fonts.code,
                    &format!("{indent}{candidate}"),
                ) > max_width
            {
                lines.push(format!("{indent}{content}"));
                content.clear();
            }
            content.push(character);
        }
        if !content.is_empty() || lines.is_empty() {
            let indent = if lines.is_empty() {
                &prefix
            } else {
                &continuation
            };
            lines.push(format!("{indent}{content}"));
        }
        return lines;
    }
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        if frame.measure(FontKind::Ui, app.fonts.ui, &candidate) <= max_width {
            line = candidate;
            continue;
        }
        if !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        for character in word.chars() {
            let candidate = format!("{line}{character}");
            if !line.is_empty() && frame.measure(FontKind::Ui, app.fonts.ui, &candidate) > max_width
            {
                lines.push(std::mem::take(&mut line));
            }
            line.push(character);
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn utf16_column_to_char(text: &str, column: u32) -> usize {
    let mut units = 0;
    for (index, character) in text.chars().enumerate() {
        if units >= column {
            return index;
        }
        units += character.len_utf16() as u32;
        if units >= column {
            return index + 1;
        }
    }
    text.chars().count()
}

/// 绘制名字旁的提示气泡；没有气泡、它挂的文档不是当前文档，或名字不在屏幕上（包括编辑区被
/// 铺满的终端面板遮住）时不绘制。
pub(in crate::view) fn draw_name_hint(app: &App, frame: &mut Frame<'_>) {
    let Some(hint) = &app.jumps.name_hint else {
        return;
    };
    if app.overlay_area() != app.layout.editor {
        return;
    }
    if app.editor.active().map(|d| d.id) != Some(hint.doc) {
        return;
    }
    let editor = app.layout.editor;
    let line_h = app.fonts.line_height;
    let Some((name_x, top)) = app
        .anchor_point(hint.anchor)
        .filter(|&(_, y)| y >= editor.y && y + line_h <= editor.bottom())
    else {
        return;
    };

    let p = app.palette;
    let w = frame.measure(FontKind::Ui, app.fonts.ui, &hint.text) + 2.0 * PAD;
    let right = editor.right() - SCROLLBAR_HIT - 8.0;
    let x = (name_x - 20.0).min(right - w).max(editor.x + 8.0).round();
    let above = top - GAP - POINTER - HEIGHT >= editor.y + 4.0;
    let y = if above {
        top - GAP - POINTER - HEIGHT
    } else {
        top + line_h + GAP + POINTER
    };

    // 出现：不透明度二次缓出，位置三次缓出；消失：原地淡出，二次缓入。
    let (opacity, away) = match (hint.fade_in, hint.fade_out) {
        (_, Some(out)) => {
            let t = out.linear();
            (1.0 - t * t, 0.0)
        }
        (Some(fade_in), None) => {
            let t = fade_in.linear();
            (1.0 - (1.0 - t).powi(2), (1.0 - t).powi(3))
        }
        (None, None) => (1.0, 0.0),
    };
    let toward_name = if above { 1.0 } else { -1.0 };
    let card = Rect::new(x, (y + toward_name * SLIDE * away).round(), w, HEIGHT);
    let tip_x = (name_x + 6.0)
        .clamp(card.x + 16.0, (card.right() - 16.0).max(card.x + 16.0))
        .round();

    frame.with_opacity(opacity, |frame| {
        draw_card(frame, p, card, RADIUS_POPUP, p.popup_bg);
        draw_pointer(frame, p, card, tip_x, above);
        ui_text(frame, app, card.x + PAD, card, &hint.text, p.text, card);
    });
}

/// Draw the full diagnostic details while hovered and briefly after the pointer leaves.
pub(in crate::view) fn draw_diagnostic_hint(app: &mut App, frame: &mut Frame<'_>) {
    let previous_popup = app.hits.diagnostic_popup.take();
    if app.overlays.picker.open
        || app.bars.goto_input.is_some()
        || app.panels.name_input.is_some()
        || app.popup().is_some()
        || app.overlays.alert.is_some()
    {
        return;
    }
    let target = match app.pointer.hover {
        Hover::Diagnostic(document, index) => Some((document, index)),
        _ => app.diagnostic_hint.as_ref().and_then(|hint| {
            (hint.fade_out.is_some()
                || hint
                    .dismiss_at
                    .is_some_and(|until| std::time::Instant::now() < until))
            .then_some((hint.doc, hint.index))
        }),
    };
    let Some((document, index)) = target else {
        return;
    };
    if app.editor.active().map(|doc| doc.id) != Some(document) {
        return;
    }
    let Some(doc) = app.editor.document(document) else {
        return;
    };
    let revision = doc.revision;
    let Some(path) = doc.path.as_ref() else {
        return;
    };
    let Some(diagnostic) = app
        .lsp_diagnostics
        .get(path)
        .and_then(|diagnostics| diagnostics.get(index))
    else {
        return;
    };
    let Some(anchor) = app
        .hits
        .diagnostics
        .iter()
        .find(|hit| hit.document == document && hit.index == index)
        .map(|hit| hit.rect)
        .or_else(|| {
            previous_popup
                .filter(|popup| popup.document == document && popup.index == index)
                .map(|popup| popup.rect)
        })
    else {
        return;
    };

    let severity = match diagnostic.severity {
        Some(lsp_types::DiagnosticSeverity::ERROR) => ("Error", app.palette.error),
        Some(lsp_types::DiagnosticSeverity::WARNING) => ("Warning", app.palette.warning),
        Some(lsp_types::DiagnosticSeverity::INFORMATION) => ("Information", app.palette.accent),
        Some(lsp_types::DiagnosticSeverity::HINT) => ("Hint", app.palette.text_muted),
        _ => ("Diagnostic", app.palette.text_muted),
    };
    let mut details = vec![
        DiagnosticDetail {
            kind: DiagnosticDetailKind::Heading,
            text: "Problem".to_owned(),
        },
        DiagnosticDetail {
            kind: DiagnosticDetailKind::Text,
            text: if diagnostic.message.trim().is_empty() {
                "No additional diagnostic details.".to_owned()
            } else {
                diagnostic.message.clone()
            },
        },
        DiagnosticDetail {
            kind: DiagnosticDetailKind::Heading,
            text: "Location".to_owned(),
        },
        DiagnosticDetail {
            kind: DiagnosticDetailKind::Text,
            text: format!(
                "{}:{}:{}",
                path.display(),
                diagnostic.range.start.line.saturating_add(1),
                diagnostic.range.start.character.saturating_add(1)
            ),
        },
    ];
    let diagnostic_line = diagnostic.range.start.line as usize;
    let line_count = doc.text().line_count();
    if diagnostic_line < line_count {
        let first_line = diagnostic_line.saturating_sub(1);
        let last_line = (diagnostic_line + 1).min(line_count - 1);
        let number_width = (last_line + 1).to_string().len();
        details.push(DiagnosticDetail {
            kind: DiagnosticDetailKind::Heading,
            text: "Code excerpt".to_owned(),
        });
        for line in first_line..=last_line {
            let source = doc.text().line_content(line).slice().to_string();
            let line_prefix = format!("{:>number_width$} | ", line + 1);
            details.push(DiagnosticDetail {
                kind: DiagnosticDetailKind::Code,
                text: format!("{line_prefix}{source}"),
            });
            if line == diagnostic_line {
                let start = utf16_column_to_char(&source, diagnostic.range.start.character)
                    .min(source.chars().count());
                let end = if diagnostic.range.end.line as usize == diagnostic_line {
                    utf16_column_to_char(&source, diagnostic.range.end.character)
                        .min(source.chars().count())
                } else {
                    source.chars().count()
                };
                let marker_prefix = format!("{:>number_width$} | ", "");
                let marker = format!(
                    "{marker_prefix}{}{}",
                    " ".repeat(start),
                    "^".repeat(end.saturating_sub(start).max(1))
                );
                details.push(DiagnosticDetail {
                    kind: DiagnosticDetailKind::CodeMarker,
                    text: marker,
                });
            }
        }
    }
    if let Some(source) = diagnostic.source.as_deref() {
        details.push(DiagnosticDetail {
            kind: DiagnosticDetailKind::Heading,
            text: "Source".to_owned(),
        });
        details.push(DiagnosticDetail {
            kind: DiagnosticDetailKind::Text,
            text: source.to_owned(),
        });
    }
    let code = diagnostic.code.as_ref().map(|code| match code {
        lsp_types::NumberOrString::Number(number) => number.to_string(),
        lsp_types::NumberOrString::String(value) => value.clone(),
    });
    if let Some(code) = &code {
        details.push(DiagnosticDetail {
            kind: DiagnosticDetailKind::Heading,
            text: "Diagnostic code".to_owned(),
        });
        details.push(DiagnosticDetail {
            kind: DiagnosticDetailKind::Text,
            text: code.clone(),
        });
    }
    details.push(DiagnosticDetail {
        kind: DiagnosticDetailKind::Heading,
        text: "Possible fixes".to_owned(),
    });
    match app.lsp_code_actions.get(&(document, doc.revision, index)) {
        Some(Ok(actions)) if !actions.is_empty() => {
            details.extend(actions.iter().map(|action| DiagnosticDetail {
                kind: DiagnosticDetailKind::Suggestion,
                text: format!("▶ {}", crate::app::lsp::code_action_title(action)),
            }));
        }
        Some(Ok(_)) => details.push(DiagnosticDetail {
            kind: DiagnosticDetailKind::Text,
            text: "The language server did not offer an automatic fix.".to_owned(),
        }),
        Some(Err(error)) => details.push(DiagnosticDetail {
            kind: DiagnosticDetailKind::Text,
            text: format!("Could not retrieve suggestions: {error}"),
        }),
        None if app
            .lsp_code_actions_pending
            .contains(&(document, doc.revision, index)) =>
        {
            details.push(DiagnosticDetail {
                kind: DiagnosticDetailKind::Text,
                text: "Checking suggestions with the language server...".to_owned(),
            });
        }
        None => details.push(DiagnosticDetail {
            kind: DiagnosticDetailKind::Text,
            text: "No automatic fix suggestions are available.".to_owned(),
        }),
    }
    if doc.highlighter.language().id == "rust" {
        details.extend([
            DiagnosticDetail {
                kind: DiagnosticDetailKind::Heading,
                text: "Verify after fixing".to_owned(),
            },
            DiagnosticDetail {
                kind: DiagnosticDetailKind::Text,
                text: "Check compilation, then run the project tests:".to_owned(),
            },
            DiagnosticDetail {
                kind: DiagnosticDetailKind::Code,
                text: "cargo check".to_owned(),
            },
            DiagnosticDetail {
                kind: DiagnosticDetailKind::Code,
                text: "cargo test".to_owned(),
            },
        ]);
    }
    if let Some(description) = &diagnostic.code_description {
        details.push(DiagnosticDetail {
            kind: DiagnosticDetailKind::Heading,
            text: "Documentation".to_owned(),
        });
        details.push(DiagnosticDetail {
            kind: DiagnosticDetailKind::Link,
            text: description.href.to_string(),
        });
    }
    if let Some(related) = &diagnostic.related_information {
        details.push(DiagnosticDetail {
            kind: DiagnosticDetailKind::Heading,
            text: "Related context".to_owned(),
        });
        for info in related {
            let location = &info.location;
            details.push(DiagnosticDetail {
                kind: DiagnosticDetailKind::Suggestion,
                text: format!(
                    "- {}:{}:{} - {}",
                    location.uri,
                    location.range.start.line.saturating_add(1),
                    location.range.start.character.saturating_add(1),
                    info.message,
                ),
            });
        }
    }
    let editor = app.layout.editor;
    let pad = 12.0;
    let line_h = app.fonts.line_height.max(14.0);
    let max_width = (editor.w - 24.0).max(64.0);
    let mut text_width = frame.measure(FontKind::Ui, app.fonts.ui, severity.0);
    for detail in &details {
        let (font, size) = match detail.kind {
            DiagnosticDetailKind::Code | DiagnosticDetailKind::CodeMarker => {
                (FontKind::Mono, app.fonts.code)
            }
            _ => (FontKind::Ui, app.fonts.ui),
        };
        text_width = text_width.max(frame.measure(font, size, &detail.text));
    }
    let width = (text_width.min(max_width - 2.0 * pad) + 2.0 * pad)
        .max(220.0_f32.min(max_width))
        .min(max_width);
    let max_height = (editor.h - 20.0).clamp(64.0, 360.0);
    let text_width = width - 2.0 * pad - 8.0;
    let mut rows = Vec::new();
    let mut content_height = 0.0;
    let mut next_action = 0;
    for detail in details {
        let action =
            if detail.kind == DiagnosticDetailKind::Suggestion && detail.text.starts_with("▶ ") {
                let action = next_action;
                next_action += 1;
                Some(action)
            } else {
                None
            };
        let is_heading = matches!(detail.kind, DiagnosticDetailKind::Heading);
        if is_heading && !rows.is_empty() {
            content_height += 6.0;
        }
        let row_height = match detail.kind {
            DiagnosticDetailKind::Code | DiagnosticDetailKind::CodeMarker => {
                (app.metrics.mono.ascent + app.metrics.mono.descent + 2.0).max(line_h)
            }
            _ => line_h,
        };
        for text in wrap_diagnostic_text(frame, app, detail.kind, &detail.text, text_width) {
            rows.push(DiagnosticDetailRow {
                kind: detail.kind,
                text,
                y: content_height,
                h: row_height,
                action,
            });
            content_height += row_height;
        }
    }
    let height = (content_height + 52.0)
        .min(max_height)
        .max(88.0_f32.min(max_height));
    let mut x = if anchor.x + width + 10.0 <= editor.right() {
        anchor.x + 10.0
    } else {
        anchor.x - width - 10.0
    };
    x = x.clamp(
        editor.x + 8.0,
        (editor.right() - width - 8.0).max(editor.x + 8.0),
    );
    let y = (anchor.y + 8.0).clamp(
        editor.y + 8.0,
        (editor.bottom() - height - 8.0).max(editor.y + 8.0),
    );
    let (opacity, slide) = match app
        .diagnostic_hint
        .as_ref()
        .filter(|hint| hint.doc == document && hint.index == index)
    {
        Some(hint) if let Some(clock) = hint.fade_out => {
            let progress = clock.linear();
            (1.0 - progress, progress * 4.0)
        }
        Some(hint) if let Some(clock) = hint.fade_in => {
            let progress = clock.linear();
            (1.0 - (1.0 - progress).powi(2), (1.0 - progress) * 4.0)
        }
        _ => (1.0, 0.0),
    };
    let card = Rect::new(x, y + slide, width, height);

    let header = Rect::new(card.x + pad, card.y + 6.0, card.w - pad * 2.0, line_h);
    let code = code.map(|code| format!(" · {code}")).unwrap_or_default();
    ui_text(
        frame,
        app,
        header.x - app.diagnostic_scroll.0,
        header,
        &format!("{}{code}", severity.0),
        severity.1,
        card,
    );
    let body = Rect::new(
        card.x + pad,
        header.bottom() + 4.0,
        card.w - pad * 2.0,
        (card.bottom() - header.bottom() - 16.0).max(1.0),
    );
    let max_line_width = rows
        .iter()
        .map(|row| {
            let (font, size) = match row.kind {
                DiagnosticDetailKind::Code | DiagnosticDetailKind::CodeMarker => {
                    (FontKind::Mono, app.fonts.code)
                }
                _ => (FontKind::Ui, app.fonts.ui),
            };
            frame.measure(font, size, &row.text)
        })
        .fold(0.0, f32::max);
    let max_scroll_x = (max_line_width - body.w).max(0.0);
    let max_scroll_y = (content_height - body.h).max(0.0);
    app.diagnostic_scroll.0 = app.diagnostic_scroll.0.clamp(0.0, max_scroll_x);
    app.diagnostic_scroll.1 = app.diagnostic_scroll.1.clamp(0.0, max_scroll_y);
    frame.with_opacity(opacity, |frame| {
        draw_card(frame, app.palette, card, RADIUS_POPUP, app.palette.popup_bg);
        frame.within(card, |frame| {
            for detail in &rows {
                let row = Rect::new(
                    body.x,
                    body.y + detail.y - app.diagnostic_scroll.1,
                    body.w,
                    detail.h,
                );
                if row.bottom() <= body.y || row.y >= body.bottom() {
                    continue;
                }
                if let Some(action) = detail.action {
                    app.hits
                        .lsp_code_actions
                        .push(crate::app::hits::LspCodeActionHit {
                            rect: Rect::new(body.x, row.y, body.w, row.h),
                            document,
                            revision,
                            diagnostic: index,
                            action,
                        });
                }
                let (text, color, x) = match detail.kind {
                    DiagnosticDetailKind::Heading => (
                        &detail.text,
                        app.palette.text_muted,
                        body.x - app.diagnostic_scroll.0,
                    ),
                    DiagnosticDetailKind::Text => (
                        &detail.text,
                        app.palette.text,
                        body.x - app.diagnostic_scroll.0,
                    ),
                    DiagnosticDetailKind::Suggestion => (
                        &detail.text,
                        app.palette.text,
                        body.x - app.diagnostic_scroll.0,
                    ),
                    DiagnosticDetailKind::Link => (
                        &detail.text,
                        app.palette.accent,
                        body.x - app.diagnostic_scroll.0,
                    ),
                    DiagnosticDetailKind::Code => (
                        &detail.text,
                        app.palette.text_muted,
                        body.x - app.diagnostic_scroll.0,
                    ),
                    DiagnosticDetailKind::CodeMarker => {
                        (&detail.text, severity.1, body.x - app.diagnostic_scroll.0)
                    }
                };
                match detail.kind {
                    DiagnosticDetailKind::Heading => {
                        ui_text_small(frame, app, x, row, text, color, body);
                        frame.rect(
                            Rect::new(body.x, row.bottom() - 1.0, body.w, 1.0),
                            app.palette.border,
                        );
                    }
                    DiagnosticDetailKind::Code | DiagnosticDetailKind::CodeMarker => {
                        let metrics = app.metrics.mono;
                        let baseline = row.y
                            + (row.h - (metrics.ascent + metrics.descent)) / 2.0
                            + metrics.ascent;
                        frame.text(
                            FontKind::Mono,
                            app.fonts.code,
                            x,
                            baseline,
                            text,
                            color,
                            body,
                        );
                    }
                    _ => {
                        ui_text(frame, app, x, row, text, color, body);
                    }
                }
            }
        });
    });
    app.hits.diagnostic_popup = Some(DiagnosticPopupHit {
        rect: card,
        document,
        index,
        max_scroll_x,
        max_scroll_y,
    });
}

/// 在卡片 `card` 的下边（`down` 为真）或上边绘制一个尖端横坐标为 `tip_x` 的小三角，填充与
/// 卡片相同，两条斜边描上卡片的边框色；三角的底边遮住卡片在那一段的边框，与卡片连成一体。
fn draw_pointer(frame: &mut Frame<'_>, p: &Palette, card: Rect, tip_x: f32, down: bool) {
    let edge = if down { card.bottom() } else { card.y };
    let dir = if down { 1.0 } else { -1.0 };
    // 每半个逻辑像素一条，自底边向尖端逐条收窄；第一条压住卡片的边框。
    let steps = (POINTER * 2.0) as usize;
    for i in 0..=steps {
        let d = i as f32 * 0.5;
        let half = POINTER - d;
        let y = if down { edge + d - 1.0 } else { edge - d + 0.5 };
        let strip = Rect::new(tip_x - half, y, 2.0 * half, 0.5);
        frame.rect(strip, p.popup_bg);
    }
    let tip = (tip_x, edge + dir * POINTER);
    let lines = [
        Stroke::Line(tip_x - POINTER, edge, tip.0, tip.1),
        Stroke::Line(tip.0, tip.1, tip_x + POINTER, edge),
    ];
    frame.stroke(&lines, 1.0, p.popup_border, frame.bounds());
}

pub(in crate::view) fn draw_completion_popup(app: &mut App, frame: &mut Frame<'_>) {
    let Some(popup) = app.lsp_completion.as_ref() else {
        return;
    };
    if !app.editor.active().is_some_and(|doc| {
        doc.id == popup.document
            && doc.revision == popup.revision
            && doc.selection.head == popup.caret
    }) {
        app.lsp_completion = None;
        return;
    }
    let (items, selected) = (popup.items.clone(), popup.selected);
    let editor = app.layout.editor;
    if items.is_empty() || editor.w <= 80.0 || editor.h <= 80.0 {
        return;
    }
    let line_h = app.fonts.line_height.max(18.0);
    let row_count = items.len().min(7);
    let first = selected.saturating_sub(row_count - 1);
    let visible = &items[first..(first + row_count).min(items.len())];
    let width = editor.w.clamp(180.0, 420.0);
    let docs = crate::app::lsp::completion::completion_documentation(&items[selected]);
    let detail_height = if docs.is_empty() { 0.0 } else { line_h * 1.5 };
    let height = (row_count as f32 * line_h + detail_height + 12.0).min(editor.h - 16.0);
    let card = Rect::new(editor.x + 8.0, editor.y + 8.0, width, height);
    draw_card(frame, app.palette, card, RADIUS_POPUP, app.palette.popup_bg);
    let mut y = card.y + 6.0;
    for (offset, item) in visible.iter().enumerate() {
        let index = first + offset;
        let row = Rect::new(card.x + 6.0, y, card.w - 12.0, line_h);
        if index == selected {
            frame.rounded(row, app.palette.hover, RADIUS_ROW);
        }
        let detail = item
            .label_details
            .as_ref()
            .and_then(|details| details.detail.as_deref())
            .or(item.detail.as_deref())
            .unwrap_or("");
        let label = if detail.is_empty() {
            item.label.clone()
        } else {
            format!("{}  {detail}", item.label)
        };
        ui_text(frame, app, row.x + 8.0, row, &label, app.palette.text, card);
        y += line_h;
    }
    if !docs.is_empty() && y + detail_height <= card.bottom() {
        let docs = docs.lines().next().unwrap_or_default();
        let row = Rect::new(card.x + 12.0, y + 3.0, card.w - 24.0, line_h);
        ui_text(frame, app, row.x, row, docs, app.palette.text_muted, card);
    }
}

#[cfg(test)]
mod tests {
    use super::utf16_column_to_char;

    #[test]
    fn diagnostic_code_excerpt_uses_utf16_columns() {
        assert_eq!(utf16_column_to_char("a🦀é", 0), 0);
        assert_eq!(utf16_column_to_char("a🦀é", 1), 1);
        assert_eq!(utf16_column_to_char("a🦀é", 3), 2);
        assert_eq!(utf16_column_to_char("a🦀é", 4), 3);
    }
}
