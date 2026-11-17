//! 附加的文件夹：一个工作区在根目录之外再显示的目录。
//!
//! 根目录决定工作区的名称和所在的系统；附加的文件夹只参与浏览和查找。有附加的文件夹时，文件树
//! 最上一层是各个文件夹（根目录在前），各自可以收起。

use super::*;

impl Workspace {
    /// 返回附加的文件夹，按添加的顺序。
    pub fn extra_folders(&self) -> &[PathBuf] {
        &self.extra
    }

    /// 返回工作区的全部文件夹：根目录在前，其后是附加的文件夹。
    pub fn folders(&self) -> impl Iterator<Item = &Path> {
        std::iter::once(self.root.as_path()).chain(self.extra.iter().map(PathBuf::as_path))
    }

    /// 返回 `path` 所在的文件夹；不在任何一个文件夹内时返回 `None`。
    pub fn folder_of(&self, path: &Path) -> Option<&Path> {
        self.folders().find(|folder| path.starts_with(folder))
    }

    /// 返回 `path` 所在的文件夹，不在任何一个文件夹内时返回根目录。
    pub(super) fn folder_or_root(&self, path: &Path) -> &Path {
        self.folder_of(path).unwrap_or(&self.root)
    }

    /// `path` 是否为在文件树中占一行的文件夹：有附加的文件夹时的根目录和各个附加的文件夹。
    pub fn is_folder_row(&self, path: &Path) -> bool {
        !self.extra.is_empty() && self.folders().any(|folder| folder == path)
    }

    /// 把目录 `path` 加为附加的文件夹并展开，返回规范化后的路径。
    ///
    /// # Errors
    ///
    /// `path` 不是可读的目录时返回对应的 `std::io::Error`；已在某个文件夹之内、包含某个文件夹，
    /// 或与根目录不在同一个系统（本机、同一个连接）时返回 `InvalidInput`。
    pub fn add_folder(&mut self, path: &Path) -> io::Result<PathBuf> {
        let path = winc_config::files::canonicalize(path)?;
        list_dir(&path)?;
        let overlaps = |folder: &Path| path.starts_with(folder) || folder.starts_with(&path);
        let place = |path: &Path| winc_backend::locate(path).map(|at| at.target);
        if self.folders().any(overlaps) || place(&path) != place(&self.root) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        self.expanded.insert(path.clone());
        self.extra.push(path.clone());
        self.rebuild();
        Ok(path)
    }

    /// 移除附加的文件夹 `path`，返回是否移除了；根目录不能移除。
    ///
    /// 只改变工作区，不改动磁盘上的内容。
    pub fn remove_folder(&mut self, path: &Path) -> bool {
        let Some(at) = self.extra.iter().position(|folder| folder == path) else {
            return false;
        };
        self.extra.remove(at);
        self.expanded.retain(|dir| !dir.starts_with(path));
        self.children.retain(|dir, _| !dir.starts_with(path));
        self.marked.retain(|item| !item.starts_with(path));
        // 只剩根目录时它不再占一行，内容直接列出。
        if self.extra.is_empty() {
            self.expanded.insert(self.root.clone());
        }
        self.rebuild();
        true
    }

    /// 返回文件树最上一层的各个文件夹；没有附加的文件夹时返回 `None`。
    pub(super) fn folder_rows(&self) -> Option<Rc<[DirEntry]>> {
        if self.extra.is_empty() {
            return None;
        }
        let rows = self.folders().map(|folder| DirEntry {
            path: folder.to_path_buf(),
            name: segment_name(folder).into_owned(),
            kind: EntryKind::Dir,
            symlink: false,
        });
        Some(rows.collect())
    }
}
