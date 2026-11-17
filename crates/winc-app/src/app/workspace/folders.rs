//! 窗口的附加目录：在打开的目录之外再显示的目录，见 [`winc_editor::Workspace::add_folder`]。
//!
//! 打开的目录决定窗口的环境（终端的起始目录、运行配置、会话）；附加的目录只参与浏览和查找，
//! 随打开的目录的会话保存。

use std::path::{Path, PathBuf};

use winc_i18n::t;

use super::files::Pick;
use crate::app::App;
use crate::app::input::context_menu::{MenuCommand, MenuEntry, MenuLabel};

impl App {
    /// 是否可以添加附加的目录：打开了本机的目录。
    ///
    /// 远端主机上的目录不能用本机的面板选择。
    pub(in crate::app) fn can_add_folder(&self) -> bool {
        (self.editor.root()).is_some_and(|root| !winc_backend::is_remote(root))
    }

    /// 返回文件树菜单中管理附加目录的项；`folder` 是右键点中的目录行，空白处为 `None`。
    pub(in crate::app) fn folder_items(&self, folder: Option<&Path>) -> Vec<MenuEntry> {
        let mut items = Vec::new();
        if self.can_add_folder() {
            items.push(MenuEntry::Item(MenuCommand::AddFolder, true));
        }
        let extra = self.editor.workspace.as_ref().zip(folder);
        if extra.is_some_and(|(ws, folder)| ws.extra_folders().iter().any(|f| f == folder)) {
            items.push(MenuEntry::Item(MenuCommand::RemoveFolder, true));
        }
        if !items.is_empty() {
            items.insert(0, MenuEntry::Separator);
        }
        items
    }

    /// 打开系统的面板选择要添加的目录。
    pub(in crate::app) fn ask_add_folder(&mut self) {
        if self.editor.root().is_none() {
            self.flash(t!("workspace-add-folder-none"));
        } else if self.can_add_folder() {
            self.pick(Pick::AddFolder);
        } else {
            self.notify(t!("workspace-add-folder-remote"));
        }
    }

    /// 把目录 `path` 加为附加的目录；不能添加时说明原因。
    pub(in crate::app) fn add_folder(&mut self, path: &Path) {
        let Some(workspace) = &mut self.editor.workspace else {
            return;
        };
        match workspace.add_folder(path) {
            Ok(_) => self.folders_changed(),
            Err(e) if e.kind() == std::io::ErrorKind::InvalidInput => {
                self.notify(t!("workspace-add-folder-overlaps"));
            }
            Err(e) => {
                let name = winc_editor::display_name(path);
                self.notify(t!(
                    "workspace-add-folder-failed",
                    name = name,
                    error = e.to_string()
                ));
            }
        }
    }

    /// 从窗口中移除附加的目录 `path`。
    pub(in crate::app) fn remove_folder(&mut self, path: &Path) {
        let removed = (self.editor.workspace.as_mut()).is_some_and(|ws| ws.remove_folder(path));
        if removed {
            self.folders_changed();
        }
    }

    /// 返回 `path` 所在的目录（打开的目录或附加的目录）；不在其中时返回打开的目录。
    ///
    /// 新建、改名、移动以它为界，不越出所在的目录。
    pub(in crate::app) fn folder_root(&self, path: &Path) -> Option<PathBuf> {
        let workspace = self.editor.workspace.as_ref()?;
        let folder = workspace.folder_of(path).unwrap_or(&workspace.root);
        Some(folder.to_path_buf())
    }

    /// 打开对话框给这组目录起名，输入框中是现在的名字。
    pub(in crate::app) fn ask_rename_group(&mut self) {
        let Some(workspace) = self.editor.workspace.as_ref() else {
            return;
        };
        if workspace.extra_folders().is_empty() {
            return;
        }
        let mut name = crate::app::TextField::default();
        name.set_text(workspace.alias().unwrap_or_default());
        let kind = crate::app::git::form::FormKind::RenameGroup;
        self.show_form_dialog(kind, name, crate::layout::DialogPart::Branch);
    }

    /// 把这组目录的名字改为 `name`；空白时恢复为打开的目录的名字。
    pub(in crate::app) fn rename_group(&mut self, name: &str) {
        if let Some(workspace) = &mut self.editor.workspace {
            workspace.set_alias(Some(name));
            self.folders_changed();
        }
    }

    /// 返回最近打开的目录 `folder` 上次附加的目录数和起的名字；没有附加过目录时返回 `None`。
    pub(crate) fn recent_group(&self, folder: &Path) -> Option<(usize, Option<&str>)> {
        let session = self.state.session_for(folder)?;
        (!session.folders.is_empty()).then_some((session.folders.len(), session.name.as_deref()))
    }

    /// 返回最近打开的目录 `folder` 的显示名：起了名的用它，附加过目录的在目录名后注明个数。
    pub(crate) fn recent_title(&self, folder: &Path) -> String {
        let name = winc_editor::display_name(folder);
        match self.recent_group(folder) {
            Some((_, Some(alias))) => alias.to_owned(),
            Some((count, None)) => t!("workspace-group-count", name = name, count = count),
            None => name.into_owned(),
        }
    }

    /// 返回附加的目录。
    pub(in crate::app) fn extra_folders(&self) -> Vec<PathBuf> {
        (self.editor.workspace.as_ref()).map_or_else(Vec::new, |ws| ws.extra_folders().to_vec())
    }

    /// 附加的目录或这一组的名字变化后：重新监视，记下会话，更新窗口标题，重新布局。
    fn folders_changed(&mut self) {
        self.watch_extra_folders();
        self.remember_session();
        self.update_window_title();
        self.relayout();
        self.request_redraw();
    }

    /// 在项目菜单 `menu` 的「打开目录…」之后加入「添加目录到窗口…」；已有附加的目录时再加入
    /// 给这一组起名的一项。没有打开本机的目录时不加。
    pub(in crate::app) fn add_folder_entries(&self, menu: &mut Vec<MenuEntry>) {
        let open = MenuEntry::Item(MenuCommand::OpenFolder, true);
        let Some(at) = menu.iter().position(|entry| *entry == open) else {
            return;
        };
        if !self.can_add_folder() {
            return;
        }
        menu.insert(at + 1, MenuEntry::Item(MenuCommand::AddFolder, true));
        if !self.extra_folders().is_empty() {
            menu.insert(at + 2, MenuEntry::Item(MenuCommand::RenameGroup, true));
        }
    }

    /// 返回最近目录第 `i` 项的菜单文字：标题只写目录名，完整路径放在悬停提示中（主目录显示为
    /// `~`）。
    ///
    /// 只有最近列表中另有同名目录时，才在标题下方用一行小字写明所在的上级目录；否则每一项
    /// 都会多出一行相同的上级目录，既占空间又没有信息量。
    pub(in crate::app) fn recent_label(&self, i: usize) -> MenuLabel {
        let Some(folder) = self.state.recent.get(i) else {
            return MenuLabel::plain("");
        };
        let name = winc_editor::display_name(folder);
        let same_name = |other: &PathBuf| winc_editor::display_name(other) == name;
        let ambiguous = self.state.recent.iter().filter(|p| same_name(p)).count() > 1;
        let title = self.recent_title(folder);
        MenuLabel {
            subtitle: folder
                .parent()
                .filter(|_| ambiguous)
                .map(|parent| winc_editor::display_path(parent, None)),
            tooltip: Some(winc_editor::display_path(folder, None)),
            title,
        }
    }
}
