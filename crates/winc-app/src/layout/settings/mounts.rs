//! 「远程目录映射」弹窗的几何：标题、说明、列标题，每条映射一行（远程目录、本机目录两个输入框，
//! 选择目录、移除两个按钮），列表下方的「添加映射」，以及底部的错误说明和两个按钮。
//!
//! 弹窗盖在设置卡片之上，行多时列表在自己的区域内滚动，标题和按钮不动。

use super::*;

/// 弹窗最宽的宽度（逻辑像素）：两列路径都放得下常见的长度。
const MOUNTS_WIDTH: f32 = 680.0;
/// 弹窗四周的内边距（逻辑像素）。
const MOUNTS_PAD: f32 = 20.0;
/// 列表中一行的高度（逻辑像素）：输入框（`SETTINGS_CONTROL_H`）上下各留 4。
pub const MOUNT_ROW_H: f32 = SETTINGS_CONTROL_H + 8.0;
/// 选择目录、移除按钮的边长（逻辑像素），与输入框同高。
const MOUNT_BUTTON: f32 = SETTINGS_CONTROL_H;
/// 列表最多同时显示的行数，再多时在列表内滚动。
const MOUNT_VISIBLE: usize = 6;
/// 标题、说明、列标题、「添加映射」、错误说明和底部按钮各自的高度（逻辑像素）。
const TITLE_H: f32 = 24.0;
const HELP_H: f32 = 20.0;
const HEAD_H: f32 = 18.0;
const ADD_H: f32 = 30.0;
const ERROR_H: f32 = 24.0;
const BUTTONS_H: f32 = 30.0;

/// 弹窗中可以点的部件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountPart {
    /// 第几行的远程目录输入框。
    Remote(usize),
    /// 第几行的本机目录输入框。
    Local(usize),
    /// 第几行本机目录右边的按钮：用系统的对话框选择目录。
    Browse(usize),
    /// 第几行行尾的「✕」：移除这一行。
    Remove(usize),
    /// 列表下方的「添加映射」。
    Add,
    /// 右上角的关闭按钮，同「取消」。
    Close,
    /// 底部的「取消」。
    Cancel,
    /// 底部的「保存」。
    Save,
    /// 弹窗外面或弹窗里没有部件的地方：点了什么也不做，下层的设置不响应。
    Backdrop,
}

/// 列表中一行的各个部件。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MountRowRects {
    pub remote: Rect,
    pub local: Rect,
    pub browse: Rect,
    pub remove: Rect,
}

/// 弹窗的布局。
#[derive(Debug, Clone, PartialEq)]
pub struct MountsLayout {
    /// 弹窗卡片。
    pub card: Rect,
    pub close: Rect,
    pub title: Rect,
    /// 标题下面的一行说明。
    pub help: Rect,
    /// 两列的列标题。
    pub remote_head: Rect,
    pub local_head: Rect,
    /// 列表的可见区域：各行在其中滚动，超出部分不绘制、不可点。
    pub list: Rect,
    /// 各行的部件，已按 `scroll` 上移。
    pub rows: Vec<MountRowRects>,
    pub add: Rect,
    /// 底部按钮上方写错误说明的一行。
    pub error: Rect,
    pub cancel: Rect,
    pub save: Rect,
    /// 列表最多能滚动多少（逻辑像素）。
    pub max_scroll: f32,
}

/// 在设置卡片 `within` 中居中放置有 `rows` 行的弹窗，列表滚动了 `scroll`。
pub fn mounts_layout(within: Rect, rows: usize, scroll: f32) -> MountsLayout {
    let w = MOUNTS_WIDTH.min(within.w - 32.0).max(320.0);
    let fixed =
        MOUNTS_PAD * 2.0 + TITLE_H + HELP_H + 12.0 + HEAD_H + 8.0 + ADD_H + ERROR_H + BUTTONS_H;
    // 窗口很矮时列表让出高度，至少留一行。
    let wanted = rows.clamp(1, MOUNT_VISIBLE) as f32 * MOUNT_ROW_H;
    let list_h = wanted.min(within.h - 24.0 - fixed).max(MOUNT_ROW_H);
    let h = fixed + list_h;
    let card = Rect::new(
        (within.x + (within.w - w) / 2.0).round(),
        (within.y + (within.h - h) / 2.0).max(within.y).round(),
        w,
        h,
    );
    let x0 = card.x + MOUNTS_PAD;
    let inner = w - MOUNTS_PAD * 2.0;
    let title = Rect::new(x0, card.y + MOUNTS_PAD, inner - MOUNT_BUTTON, TITLE_H);
    let close = Rect::new(
        card.right() - MOUNTS_PAD - MOUNT_BUTTON + 6.0,
        title.y + (TITLE_H - MOUNT_BUTTON) / 2.0,
        MOUNT_BUTTON,
        MOUNT_BUTTON,
    );
    let help = Rect::new(x0, title.bottom(), inner, HELP_H);

    // 两个输入框分去按钮以外的宽度：本机目录稍宽一些，右边紧跟选择目录的按钮。
    let paths = inner - MOUNT_BUTTON * 2.0 - 8.0 * 2.0 - 4.0;
    let remote_w = (paths * 0.48).round();
    let local_w = paths - remote_w;
    let local_x = x0 + remote_w + 8.0;
    let head_y = help.bottom() + 12.0;
    let remote_head = Rect::new(x0, head_y, remote_w, HEAD_H);
    let local_head = Rect::new(local_x, head_y, local_w, HEAD_H);
    let list = Rect::new(x0 - 2.0, remote_head.bottom() + 4.0, inner + 4.0, list_h);
    let max_scroll = (rows as f32 * MOUNT_ROW_H - list_h).max(0.0);
    let scroll = scroll.clamp(0.0, max_scroll);
    let field_y = (MOUNT_ROW_H - SETTINGS_CONTROL_H) / 2.0;
    let rows = (0..rows)
        .map(|i| {
            let y = list.y + i as f32 * MOUNT_ROW_H - scroll + field_y;
            let local = Rect::new(local_x, y, local_w, SETTINGS_CONTROL_H);
            let browse = Rect::new(local.right() + 4.0, y, MOUNT_BUTTON, MOUNT_BUTTON);
            MountRowRects {
                remote: Rect::new(x0, y, remote_w, SETTINGS_CONTROL_H),
                local,
                browse,
                remove: Rect::new(browse.right() + 8.0, y, MOUNT_BUTTON, MOUNT_BUTTON),
            }
        })
        .collect();
    let add = Rect::new(x0 - 6.0, list.bottom() + 4.0, 160.0, ADD_H);
    let error = Rect::new(x0, add.bottom(), inner, ERROR_H);
    let buttons_y = error.bottom();
    let save = Rect::new(
        card.right() - MOUNTS_PAD - DIALOG_BUTTON_W,
        buttons_y,
        DIALOG_BUTTON_W,
        BUTTONS_H,
    );
    let cancel = Rect::new(
        save.x - 8.0 - DIALOG_BUTTON_W,
        buttons_y,
        DIALOG_BUTTON_W,
        BUTTONS_H,
    );
    MountsLayout {
        card,
        close,
        title,
        help,
        remote_head,
        local_head,
        list,
        rows,
        add,
        error,
        cancel,
        save,
        max_scroll,
    }
}

impl MountsLayout {
    /// 返回 (`x`, `y`) 处的部件；弹窗外面和弹窗里的空白处是 [`MountPart::Backdrop`]。
    pub fn part_at(&self, x: f32, y: f32) -> MountPart {
        if !self.card.contains(x, y) {
            return MountPart::Backdrop;
        }
        let fixed = [
            (self.close, MountPart::Close),
            (self.cancel, MountPart::Cancel),
            (self.save, MountPart::Save),
            (self.add, MountPart::Add),
        ];
        if let Some(&(_, part)) = fixed.iter().find(|(rect, _)| rect.contains(x, y)) {
            return part;
        }
        if !self.list.contains(x, y) {
            return MountPart::Backdrop;
        }
        for (i, row) in self.rows.iter().enumerate() {
            let parts = [
                (row.remote, MountPart::Remote(i)),
                (row.local, MountPart::Local(i)),
                (row.browse, MountPart::Browse(i)),
                (row.remove, MountPart::Remove(i)),
            ];
            if let Some(&(_, part)) = parts.iter().find(|(rect, _)| rect.contains(x, y)) {
                return part;
            }
        }
        MountPart::Backdrop
    }

    /// 第 `row` 行在列表中完整可见时需要的滚动量（当前为 `scroll`）。
    pub fn scroll_to(&self, row: usize, scroll: f32) -> f32 {
        let top = row as f32 * MOUNT_ROW_H;
        let bottom = top + MOUNT_ROW_H;
        let scroll = if top < scroll {
            top
        } else if bottom > scroll + self.list.h {
            bottom - self.list.h
        } else {
            scroll
        };
        scroll.clamp(0.0, self.max_scroll)
    }
}
