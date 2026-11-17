//! 窗口的附加目录：在打开的目录之外再显示的目录，见 [`winc_editor::Workspace::add_folder`]。
//!
//! 打开的目录决定窗口的环境（终端的起始目录、运行配置、会话）；附加的目录只参与浏览和查找，
//! 随打开的目录的会话保存。

use std::path::{Path, PathBuf};

use winc_i18n::t;

use super::files::Pick;
use crate::app::App;
use crate::app::input::context_menu::{MenuCommand, MenuEntry};

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

    /// 返回附加的目录。
    pub(in crate::app) fn extra_folders(&self) -> Vec<PathBuf> {
        (self.editor.workspace.as_ref()).map_or_else(Vec::new, |ws| ws.extra_folders().to_vec())
    }

    /// 附加的目录变化后：重新监视，保存会话，重新布局。
    fn folders_changed(&mut self) {
        self.watch_extra_folders();
        self.save_session();
        self.relayout();
        self.request_redraw();
    }
}
