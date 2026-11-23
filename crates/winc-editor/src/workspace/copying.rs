//! 复制文件和目录的过程：逐项复制，以及供界面读取的进度和取消的入口。

#[cfg(test)]
mod tests;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use winc_i18n::t;

use winc_backend::fs::Kind;

/// 一次复制的进度和取消的入口：执行复制的线程写入，界面线程读取和取消。
#[derive(Debug, Default)]
pub struct Copying {
    cancelled: AtomicBool,
    /// 正在复制的一项已复制的字节数。
    copied: AtomicU64,
    /// 正在复制的一项的名字；它是单个文件时还有文件的大小（字节）。
    item: Mutex<(String, Option<u64>)>,
}

impl Copying {
    /// 取消：正在复制的一项停止并删除已复制出的部分，此前复制完的各项保留。
    ///
    /// 文件在同一台机器上复制时由那台机器一次复制完，复制完这个文件之后才停止。
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// 是否已取消。
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    /// 正在复制的一项已复制的字节数。
    pub fn copied(&self) -> u64 {
        self.copied.load(Ordering::Relaxed)
    }

    /// 正在复制的一项的名字，以及它是单个文件时的大小（字节）；目录的大小事先不知道。
    pub fn item(&self) -> (String, Option<u64>) {
        self.item
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// 开始复制名为 `name` 的一项；`size` 是它作为单个文件的大小。
    fn begin(&self, name: String, size: Option<u64>) {
        *self.item.lock().unwrap_or_else(PoisonError::into_inner) = (name, size);
        self.copied.store(0, Ordering::Relaxed);
    }

    /// 记下新复制的 `bytes` 字节，返回是否继续复制。
    fn advance(&self, bytes: u64) -> bool {
        self.copied.fetch_add(bytes, Ordering::Relaxed);
        !self.is_cancelled()
    }
}

/// 复制符号链接的方式：把链接 `from` 复制为 `to`。
pub(crate) type Links = fn(&Path, &Path) -> io::Result<()>;

/// 下载目录时被跳过的一项：`name` 是相对顶层下载项的路径，`reason` 是跳过原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// 被跳过的项相对顶层下载项的路径，例如 `project/data/app.sock`。
    pub name: String,
    /// 跳过的原因。
    pub reason: String,
}

/// 一次下载中的附带记录：被跳过的各项，以及最近一次失败实际发生的路径。
///
/// `failed` 在每次进入 [`copy_tree`] 时更新为当前项，因此递归中出错时留下的是真正出错的子项，
/// 而不是最外层的顶层目录。
#[derive(Debug)]
pub(crate) struct DownloadState {
    root: PathBuf,
    pub skipped: Vec<Skipped>,
    pub failed: String,
}

impl DownloadState {
    pub(crate) fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            skipped: Vec::new(),
            failed: crate::display_name(root).into_owned(),
        }
    }

    /// 相对顶层下载项的名字，例如 `project/data/app.sock`。
    fn label(&self, path: &Path) -> String {
        match winc_backend::relative_lossy(path, &self.root) {
            Some(relative) if !relative.is_empty() => {
                // 远端路径一律以 `/` 分隔；本机路径换成 `/` 后提示里的写法才一致。
                format!(
                    "{}/{}",
                    crate::display_name(&self.root),
                    relative.replace('\\', "/")
                )
            }
            _ => crate::display_name(path).into_owned(),
        }
    }
}

/// 把 `from` 复制为尚不存在的 `to`：文件复制内容和权限，目录连同其中的内容一起复制，符号链接
/// 按 `links` 复制；进度记入 `copying`。
///
/// # Errors
///
/// 被 `copying` 取消时删除已复制出的 `to`，返回 [`io::ErrorKind::Interrupted`]；其余返回对应的
/// `io::Error`，已复制出的部分保留。
pub(crate) fn copy_item(from: &Path, to: &Path, links: Links, copying: &Copying) -> io::Result<()> {
    copy_item_inner(from, to, links, copying, None)
}

/// 下载时复制顶层项 `from`；目录内无法跨机传输的项记入 `state` 后跳过，不中断整棵目录。
pub(crate) fn download_item(
    from: &Path,
    to: &Path,
    links: Links,
    copying: &Copying,
    state: &mut DownloadState,
) -> io::Result<()> {
    copy_item_inner(from, to, links, copying, Some(state))
}

fn copy_item_inner(
    from: &Path,
    to: &Path,
    links: Links,
    copying: &Copying,
    state: Option<&mut DownloadState>,
) -> io::Result<()> {
    let meta = winc_backend::fs::symlink_metadata(from)?;
    copying.begin(
        crate::display_name(from).into_owned(),
        meta.is_file().then_some(meta.len),
    );
    let copied = copy_tree(from, to, meta.kind, links, copying, true, state);
    if copied.is_err() && copying.is_cancelled() {
        // `to` 是本次复制新建的：删除它不会碰到原有的内容。还没有建出来时删除失败，不必处理。
        let _ = winc_backend::fs::remove_all(to);
        return Err(io::ErrorKind::Interrupted.into());
    }
    copied
}

/// Windows 上复制链接指向的内容时，这些错误说明目标无法跨机传输；连接断开、权限不足等真实
/// I/O 错误不在此列，必须照常中断。
#[cfg(not(unix))]
fn untranslatable_link(error: &io::Error) -> bool {
    if matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::IsADirectory | io::ErrorKind::InvalidInput
    ) {
        return true;
    }
    // 循环链接：远端 Linux 的 ELOOP 在 Rust 1.99 里仍是 `ErrorKind::Uncategorized`（`FilesystemLoop`
    // 属不稳定的 `io_error_more`），而且本项目的远端协议只传 ErrorKind 和文字、不传 errno，所以到
    // 这里只能是 `Other` + 系统说明。各平台写法不同（Linux 有 "Too many levels of symbolic links"，
    // 也有 "Symbolic link loop"），这里只在处理符号链接项时调用，按文字识别不会误伤普通文件。
    let text = error.to_string().to_ascii_lowercase();
    text.contains("symbolic link") || text.contains("symlink loop")
}

/// 复制种类为 `kind` 的 `from`；目录逐项递归，每一项开始之前检查是否已取消。
///
/// `root` 表示这是顶层下载项本身：顶层项出错时照常报错，只有目录内部的子项才跳过。
fn copy_tree(
    from: &Path,
    to: &Path,
    kind: Kind,
    links: Links,
    copying: &Copying,
    root: bool,
    mut state: Option<&mut DownloadState>,
) -> io::Result<()> {
    if let Some(state) = state.as_deref_mut() {
        state.failed = state.label(from);
    }
    if copying.is_cancelled() {
        return Err(io::ErrorKind::Interrupted.into());
    }
    if kind.is_symlink() {
        if root {
            return links(from, to);
        }
        #[cfg(not(unix))]
        if let Some(state) = state.as_deref_mut() {
            match links(from, to) {
                Ok(()) => return Ok(()),
                Err(error) if untranslatable_link(&error) => {
                    let name = state.label(from);
                    state.skipped.push(Skipped {
                        name,
                        reason: error.to_string(),
                    });
                    return Ok(());
                }
                Err(error) => return Err(error),
            }
        }
        return links(from, to);
    }
    if kind.is_other()
        && !root
        && let Some(state) = state.as_deref_mut()
    {
        let name = state.label(from);
        state.skipped.push(Skipped {
            name,
            reason: t!("editor-download-skipped-special").to_owned(),
        });
        return Ok(());
    }
    if !kind.is_dir() {
        let watch = |bytes| copying.advance(bytes);
        return winc_backend::fs::copy_watched(from, to, &watch).map(drop);
    }
    winc_backend::fs::create_dir(to)?;
    for entry in winc_backend::fs::read_dir(from)? {
        let entry = entry?;
        copy_tree(
            &entry.path(),
            &to.join(winc_backend::carry_name(&entry.name, from, to)),
            entry.kind,
            links,
            copying,
            false,
            state.as_deref_mut(),
        )?;
    }
    Ok(())
}
