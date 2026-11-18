//! 设置中的「远程目录映射」弹窗：盖在设置卡片之上，一行一条 `remote.localFolders`，每行是远程
//! 目录和本机目录两个输入框，本机目录可以手写，也可以用系统的对话框选择。
//!
//! 弹窗是设置浮层的一部分：打开期间设置浮层的按键、点击、滚轮都先交给它，输入框沿用设置的输入
//! 焦点（[`Focus::Setting`]）。按「保存」时逐行检查，写错时停在写错的那一栏并说明原因，配置不变；
//! 「取消」、`Esc` 放弃这次的改动。

use std::path::PathBuf;

use super::*;
use crate::app::remote::mounts::{RowError, parse_rows};
use crate::layout::{MountPart, MountsLayout, mounts_layout};

/// 弹窗的状态。
#[derive(Debug, Default)]
pub struct MountsDialog {
    /// 各行，顺序即显示的顺序；打开时按远程目录排序。
    pub rows: Vec<MountRow>,
    /// 键盘焦点所在的部件；在输入框上时键入的文字进入该输入框。
    pub focus: Option<MountPart>,
    /// 列表滚动了多少（逻辑像素）。
    pub scroll: f32,
    /// 按「保存」时发现的第一处错误；改动任一输入框后清除。
    pub error: Option<RowError>,
}

/// 弹窗中的一行。
#[derive(Debug, Default)]
pub struct MountRow {
    /// 远程目录：`主机:/路径`。
    pub remote: TextField,
    /// 本机目录。
    pub local: TextField,
}

impl MountRow {
    fn new(remote: &str, local: &str) -> MountRow {
        let mut row = MountRow::default();
        row.remote.set_text(remote);
        row.local.set_text(local);
        row
    }
}

impl MountsDialog {
    /// 键盘焦点所在的输入框；焦点在按钮上时为 `None`。
    pub(in crate::app) fn field_mut(&mut self) -> Option<&mut TextField> {
        match self.focus? {
            MountPart::Remote(i) => self.rows.get_mut(i).map(|row| &mut row.remote),
            MountPart::Local(i) => self.rows.get_mut(i).map(|row| &mut row.local),
            _ => None,
        }
    }

    /// `Tab` 依次经过的部件：每行的远程目录、本机目录、选择目录、移除，然后是「添加映射」、
    /// 「取消」和「保存」。
    fn order(&self) -> Vec<MountPart> {
        let rows = (0..self.rows.len()).flat_map(|i| {
            [
                MountPart::Remote(i),
                MountPart::Local(i),
                MountPart::Browse(i),
                MountPart::Remove(i),
            ]
        });
        rows.chain([MountPart::Add, MountPart::Cancel, MountPart::Save])
            .collect()
    }
}

impl MountPart {
    /// 部件所在的行；不在某一行上时为 `None`。
    fn row(self) -> Option<usize> {
        match self {
            MountPart::Remote(i)
            | MountPart::Local(i)
            | MountPart::Browse(i)
            | MountPart::Remove(i) => Some(i),
            _ => None,
        }
    }
}

impl App {
    /// 弹窗的布局：在设置卡片中居中。
    pub(in crate::app) fn mounts_layout(&self) -> MountsLayout {
        let (rows, scroll) = (self.settings.mounts.as_ref())
            .map_or((0, 0.0), |dialog| (dialog.rows.len(), dialog.scroll));
        mounts_layout(self.settings_layout().card, rows, scroll)
    }

    /// 打开弹窗，列出配置中的各项，键盘焦点在第一行的远程目录上。
    ///
    /// 还没有映射时给一个空行；打开的是远端的目录时，空行的远程目录先填上这个目录。
    pub(in crate::app) fn settings_open_mounts(&mut self) {
        self.settings_leave_field();
        let folders = &self.config.remote_local_folders;
        let mut rows: Vec<MountRow> = (folders.iter())
            .map(|(remote, local)| MountRow::new(remote, local))
            .collect();
        if rows.is_empty() {
            let here = self.editor.root().and_then(winc_backend::display);
            rows.push(MountRow::new(here.as_deref().unwrap_or(""), ""));
        }
        self.settings.mounts = Some(MountsDialog {
            rows,
            ..MountsDialog::default()
        });
        self.focus = Focus::Setting;
        self.mounts_focus(MountPart::Remote(0));
    }

    /// 关闭弹窗，放弃没有保存的改动；焦点回到设置中「远程目录映射」这一行。
    pub(in crate::app) fn close_mounts(&mut self) {
        if self.settings.mounts.take().is_none() {
            return;
        }
        self.ime.field_preedit.clear();
        self.settings.selected = Setting::RemoteFolders;
        self.settings.hover = None;
        self.focus = Focus::Settings;
        self.request_redraw();
    }

    /// 把键盘焦点移到 `part`：输入框全选其中的文字，所在的行滚动到可见。
    pub(in crate::app) fn mounts_focus(&mut self, part: MountPart) {
        let layout = self.mounts_layout();
        let Some(dialog) = self.settings.mounts.as_mut() else {
            return;
        };
        dialog.focus = Some(part);
        if let Some(field) = dialog.field_mut() {
            field.select_all();
        }
        if let Some(row) = part.row() {
            dialog.scroll = layout.scroll_to(row, dialog.scroll);
        }
        self.ime.field_preedit.clear();
        self.focus = Focus::Setting;
        self.reset_blink();
        self.request_redraw();
    }

    /// `Tab`（`forward`）或 `⇧Tab`：焦点移到下一个或上一个部件，到头后从另一端开始。
    pub(in crate::app) fn mounts_move(&mut self, forward: bool) {
        let Some(dialog) = &self.settings.mounts else {
            return;
        };
        let order = dialog.order();
        let at = dialog
            .focus
            .and_then(|f| order.iter().position(|&p| p == f));
        let next = match (at, forward) {
            (None, true) => 0,
            (None, false) => order.len() - 1,
            (Some(i), true) => (i + 1) % order.len(),
            (Some(i), false) => (i + order.len() - 1) % order.len(),
        };
        self.mounts_focus(order[next]);
    }

    /// `↑` `↓`：在输入框之间换到上一行或下一行的同一栏；没有那一行时不动。
    pub(in crate::app) fn mounts_step_row(&mut self, down: bool) {
        let Some(dialog) = &self.settings.mounts else {
            return;
        };
        let next = match dialog.focus {
            Some(MountPart::Remote(i)) => {
                (down || i > 0).then(|| MountPart::Remote(if down { i + 1 } else { i - 1 }))
            }
            Some(MountPart::Local(i)) => {
                (down || i > 0).then(|| MountPart::Local(if down { i + 1 } else { i - 1 }))
            }
            _ => None,
        };
        if let Some(part) = next.filter(|p| p.row().is_some_and(|r| r < dialog.rows.len())) {
            self.mounts_focus(part);
        }
    }

    /// 点击或按键激活 `part`（输入框以外的部件）。
    pub(in crate::app) fn mounts_activate(&mut self, part: MountPart) {
        match part {
            MountPart::Browse(i) => self.pick(Pick::MountFolder(i)),
            MountPart::Remove(i) => self.mounts_remove(i),
            MountPart::Add => self.mounts_add(),
            MountPart::Close | MountPart::Cancel => self.close_mounts(),
            MountPart::Save => self.save_mounts(),
            MountPart::Remote(_) | MountPart::Local(_) => self.mounts_focus(part),
            MountPart::Backdrop => {}
        }
    }

    /// 处理弹窗打开时的点击：输入框上定位光标（双击选词、拖动选择），其余部件激活。
    pub(in crate::app) fn click_mounts(&mut self, part: MountPart, x: f32, y: f32) {
        match part {
            MountPart::Remote(_) | MountPart::Local(_) => {
                if let Some(dialog) = &mut self.settings.mounts {
                    dialog.focus = Some(part);
                }
                self.focus = Focus::Setting;
                self.click_field(x, y);
            }
            part => self.mounts_activate(part),
        }
        self.request_redraw();
    }

    /// 在列表末尾加一个空行，焦点移到它的远程目录。
    fn mounts_add(&mut self) {
        let Some(dialog) = &mut self.settings.mounts else {
            return;
        };
        dialog.rows.push(MountRow::default());
        dialog.error = None;
        let last = dialog.rows.len() - 1;
        self.mounts_focus(MountPart::Remote(last));
    }

    /// 移除第 `i` 行；只剩这一行时清空它，弹窗里总有一行可填。
    fn mounts_remove(&mut self, i: usize) {
        let Some(dialog) = &mut self.settings.mounts else {
            return;
        };
        if i >= dialog.rows.len() {
            return;
        }
        if dialog.rows.len() == 1 {
            dialog.rows[0] = MountRow::default();
        } else {
            dialog.rows.remove(i);
        }
        dialog.error = None;
        // 焦点落在原来这一行的位置（删的是最后一行时为新的最后一行）的移除按钮上，连续按空格
        // 可以一行行删下去。
        let at = i.min(dialog.rows.len() - 1);
        self.mounts_focus(MountPart::Remove(at));
    }

    /// 系统的对话框中为第 `i` 行选好了目录 `path`：填进本机目录。
    pub(in crate::app) fn mount_folder_picked(&mut self, i: usize, path: PathBuf) {
        let Some(row) = (self.settings.mounts.as_mut()).and_then(|dialog| dialog.rows.get_mut(i))
        else {
            return;
        };
        let text = path.to_string_lossy();
        row.local.set_text(&text);
        row.local.set_caret(text.len(), false);
        if let Some(dialog) = &mut self.settings.mounts {
            dialog.error = None;
            dialog.focus = Some(MountPart::Local(i));
        }
        self.focus = Focus::Setting;
        self.request_redraw();
    }

    /// 第 `i` 行本机目录里写的路径，作为系统对话框开始的位置；空着时为 `None`。
    pub(in crate::app) fn mount_folder_start(&self, i: usize) -> Option<PathBuf> {
        let dialog = self.settings.mounts.as_ref()?;
        let text = dialog.rows.get(i)?.local.text().trim();
        (!text.is_empty()).then(|| PathBuf::from(text))
    }

    /// 保存：各行没有错误时写进配置并关闭弹窗；有错误时焦点移到写错的那一栏，原因显示在底部。
    pub(in crate::app) fn save_mounts(&mut self) {
        let Some(dialog) = &self.settings.mounts else {
            return;
        };
        let rows: Vec<(&str, &str)> = (dialog.rows.iter())
            .map(|row| (row.remote.text(), row.local.text()))
            .collect();
        match parse_rows(&rows) {
            Ok(folders) => {
                if self.config.remote_local_folders != folders {
                    self.config.remote_local_folders = folders;
                    self.save_config();
                }
                self.close_mounts();
            }
            Err(error) => {
                let part = if error.local {
                    MountPart::Local(error.row)
                } else {
                    MountPart::Remote(error.row)
                };
                self.mounts_focus(part);
                if let Some(dialog) = &mut self.settings.mounts {
                    dialog.error = Some(error);
                }
            }
        }
        self.request_redraw();
    }

    /// 滚轮：列表行多时上下滚动。
    pub(in crate::app) fn mounts_scroll_by(&mut self, dy: f32) {
        let max = self.mounts_layout().max_scroll;
        if let Some(dialog) = &mut self.settings.mounts {
            dialog.scroll = (dialog.scroll.min(max) - dy).clamp(0.0, max);
            self.request_redraw();
        }
    }
}
