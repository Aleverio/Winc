//! 「远程目录映射」弹窗的绘制：盖在设置卡片上的一张小卡片，状态和操作见 `app::settings::mounts`，
//! 几何见 [`crate::layout::mounts_layout`]。

use winc_i18n::t;
use winc_render::{Frame, Rect};

use super::*;
use crate::layout::{MountPart, SettingsLayout, mounts_layout};

/// 本机目录输入框空着时的示例：随平台给出常见的挂载位置。
const LOCAL_EXAMPLE: &str = if cfg!(windows) {
    r"Z:\"
} else if cfg!(target_os = "macos") {
    "/Volumes/devbox"
} else {
    "/mnt/devbox"
};

/// 绘制弹窗，返回有焦点的输入框中光标的矩形（供输入法定位）。
pub(super) fn draw(
    app: &mut App,
    frame: &mut Frame<'_>,
    settings: &SettingsLayout,
) -> Option<Rect> {
    let p = app.palette;
    let Some(dialog) = &app.settings.mounts else {
        return None;
    };
    let layout = mounts_layout(settings.card, dialog.rows.len(), dialog.scroll);
    let focus = dialog.focus.filter(|_| app.focus == Focus::Setting);
    let error = dialog.error.clone();
    let hover = match app.settings.hover {
        Some(SettingsPart::Mount(part)) => Some(part),
        _ => None,
    };

    // 压暗下面的设置卡片，弹窗靠阴影浮起。
    frame.rounded(settings.card, p.scrim, RADIUS_CARD);
    crate::view::draw_card_frame(frame, p, layout.card, RADIUS_CARD);
    let card = layout.card;
    frame.rounded(card, p.editor_bg, RADIUS_CARD);

    ui_text(
        frame,
        app,
        layout.title.x,
        layout.title,
        t!("settings-mounts-title"),
        p.text,
        card,
    );
    let closing = hover == Some(MountPart::Close);
    crate::view::draw_button(frame, p, layout.close, card, closing, false);
    icons::close(frame, center(layout.close), 7.0, p.text_muted, card);
    let help = ellipsize(
        frame,
        t!("settings-mounts-help"),
        app.fonts.ui_small,
        layout.help.w,
    );
    ui_text_small(
        frame,
        app,
        layout.help.x,
        layout.help,
        &help,
        p.text_muted,
        card,
    );
    for (rect, text) in [
        (layout.remote_head, t!("settings-mounts-remote")),
        (layout.local_head, t!("settings-mounts-local")),
    ] {
        ui_text_small(frame, app, rect.x + 2.0, rect, text, p.text_dim, card);
    }

    // 各行在列表区域内滚动，超出的部分裁掉。
    let list = layout.list;
    let mut caret = None;
    for (i, row) in layout.rows.iter().enumerate() {
        if row.remote.bottom() < list.y || row.remote.y > list.bottom() {
            continue;
        }
        let wrong = |local: bool| {
            error
                .as_ref()
                .is_some_and(|e| e.row == i && e.local == local)
        };
        for (rect, part, local) in [
            (row.remote, MountPart::Remote(i), false),
            (row.local, MountPart::Local(i), true),
        ] {
            let focused = focus == Some(part);
            caret = draw_field(app, frame, rect, part, focused, wrong(local), list).or(caret);
        }
        let browse = row.browse;
        draw_icon_button(
            app,
            frame,
            browse,
            hover == Some(MountPart::Browse(i)),
            focus == Some(MountPart::Browse(i)),
            list,
        );
        icons::folder_small(frame, center(browse), p.text_muted, list);
        let remove = row.remove;
        draw_icon_button(
            app,
            frame,
            remove,
            hover == Some(MountPart::Remove(i)),
            focus == Some(MountPart::Remove(i)),
            list,
        );
        icons::close(frame, center(remove), 6.0, p.text_muted, list);
    }

    // 「添加映射」：与格式化页「添加语言…」相同，加号和文字用强调色。
    let add = layout.add;
    let adding = hover == Some(MountPart::Add);
    if focus == Some(MountPart::Add) {
        focus_ring(frame, p, add, card);
    }
    crate::view::draw_button(frame, p, add, card, adding, false);
    let icon = (add.x + 14.0, add.y + add.h / 2.0);
    icons::plus(frame, icon, 9.0, p.accent, card);
    ui_text(
        frame,
        app,
        add.x + 26.0,
        add,
        t!("settings-mounts-add"),
        p.accent,
        card,
    );

    if let Some(error) = &error {
        let room = layout.cancel.x - 16.0 - layout.error.x;
        let text = ellipsize(frame, &error.message, app.fonts.ui_small, room);
        let at = Rect::new(layout.error.x, layout.cancel.y, room, layout.cancel.h);
        ui_text_small(frame, app, at.x, at, &text, p.error, card);
    }
    for (rect, part, text, role) in [
        (
            layout.cancel,
            MountPart::Cancel,
            t!("settings-mounts-cancel"),
            ButtonRole::Normal,
        ),
        (
            layout.save,
            MountPart::Save,
            t!("settings-mounts-save"),
            ButtonRole::Primary,
        ),
    ] {
        if focus == Some(part) {
            focus_ring(frame, p, rect, card);
        }
        crate::view::draw_action_button(app, frame, rect, text, role, true, hover == Some(part));
    }
    caret
}

/// 绘制一个输入框：编辑区底色，有焦点时外圈强调色，写错时外圈错误色；返回光标的矩形。
fn draw_field(
    app: &mut App,
    frame: &mut Frame<'_>,
    rect: Rect,
    part: MountPart,
    focused: bool,
    wrong: bool,
    clip: Rect,
) -> Option<Rect> {
    let p = app.palette;
    let hovered = app.settings.hover == Some(SettingsPart::Mount(part));
    frame.within(clip, |frame| {
        crate::view::field_box(app, frame, rect, p.editor_bg, focused, wrong);
        if hovered && !focused && !wrong {
            frame.rect_clipped(rect, p.hover, RADIUS_ROW, clip);
        }
    });
    let placeholder = match part {
        MountPart::Remote(_) => t!("settings-mounts-remote-placeholder"),
        _ => LOCAL_EXAMPLE,
    };
    let mut dialog = app.settings.mounts.take()?;
    let row = match part {
        MountPart::Remote(i) | MountPart::Local(i) => dialog.rows.get_mut(i),
        _ => None,
    };
    let caret = row.and_then(|row| {
        let field = match part {
            MountPart::Remote(_) => &mut row.remote,
            _ => &mut row.local,
        };
        let area = rect.inset(DIALOG_FIELD_INSET, 0.0);
        frame.within(clip, |frame| {
            crate::view::draw_bar_field(app, frame, area, field, placeholder, focused)
        })
    });
    app.settings.mounts = Some(dialog);
    caret.filter(|_| focused)
}

/// 绘制图标按钮的底色：悬停时铺浅色，有键盘焦点时加一圈强调色。
fn draw_icon_button(
    app: &App,
    frame: &mut Frame<'_>,
    rect: Rect,
    hovered: bool,
    focused: bool,
    clip: Rect,
) {
    let p = app.palette;
    if focused {
        focus_ring(frame, p, rect, clip);
    }
    crate::view::draw_button(frame, p, rect, clip, hovered, false);
}

/// 沿 `rect` 外侧画一圈强调色细边，表示键盘焦点。
fn focus_ring(frame: &mut Frame<'_>, p: &Palette, rect: Rect, clip: Rect) {
    frame.rect_clipped(rect.inset(-2.0, -2.0), p.accent, RADIUS_ROW + 2.0, clip);
    frame.rect_clipped(rect.inset(-0.5, -0.5), p.editor_bg, RADIUS_ROW + 0.5, clip);
}
