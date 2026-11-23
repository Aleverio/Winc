//! 在默认的文件管理器中显示并选中文件或目录。
//!
//! 经 shell 的 `SHOpenFolderAndSelectItems` 打开 `path` 所在的目录并选中它，由系统默认的文件
//! 管理器接管：资源管理器，或替换了资源管理器的第三方文件管理器（如 Directory Opus）。
//! shell 调用失败时先请默认的文件管理器打开所在的目录（没有选中项），仍失败时退回资源管理器。

// COM 与 shell 调用；每个 `unsafe` 块都有 `// SAFETY:` 说明。
#![allow(unsafe_code)]

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::UI::Shell::{ILCreateFromPathW, ILFree, SHOpenFolderAndSelectItems};
use windows::core::PCWSTR;

/// 在默认的文件管理器中显示并选中 `path`，在后台线程中完成。
///
/// 不直接启动 `explorer.exe`：那样会绕过系统的默认文件管理器，替换了资源管理器的第三方程序
/// 接不到。`SHOpenFolderAndSelectItems` 经 shell 处理，资源管理器和第三方程序都能接管；
/// 调用失败时先请默认的文件管理器打开所在的目录，仍失败时退回 `explorer /select`。
///
/// # Errors
///
/// 无法创建后台线程时返回 `io::Error`。
pub fn reveal_in_file_manager(path: &Path) -> io::Result<()> {
    // 测试中不实际打开窗口：测试会模拟按键和菜单操作，真实启动会弹出文件管理器。
    if cfg!(test) {
        return Ok(());
    }
    let path = std::path::absolute(path)?;
    std::thread::Builder::new()
        .name("winc-reveal".to_owned())
        .spawn(move || {
            // SAFETY: 本线程尚未初始化 COM；成功后在线程结束前调用一次 `CoUninitialize` 配对。
            if unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_err() {
                fallback(&path);
                return;
            }
            let shown = select(&path);
            // SAFETY: 与上面成功的 `CoInitializeEx` 配对；`select` 已释放 PIDL。
            unsafe { CoUninitialize() };
            if shown.is_err() {
                fallback(&path);
            }
        })
        .map(|_| ())
}

/// shell 调用失败时先请默认的文件管理器打开 `path` 所在的目录（不能选中该项）；没有可用的
/// 默认处理程序时再退回资源管理器，尽量不打开与用户设置无关的程序。
fn fallback(path: &Path) {
    let folder = path.parent().unwrap_or(path);
    if crate::system::open_with_default_app(folder).is_ok() {
        return;
    }
    explorer(path);
}

/// 请 shell 打开 `path` 的父目录并选中 `path`；调用前 COM 必须已初始化。
fn select(path: &Path) -> windows::core::Result<()> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: `wide` 以 NUL 结尾，在调用期间有效；返回的 PIDL 由下面的 `ILFree` 释放。
    let pidl = unsafe { ILCreateFromPathW(PCWSTR(wide.as_ptr())) };
    if pidl.is_null() {
        return Err(windows::core::Error::empty());
    }
    // SAFETY: `pidl` 是上面创建的完整 PIDL，在调用期间有效。`cidl` 为 0 时，系统打开其父目录
    // 并选中该项。
    let result = unsafe { SHOpenFolderAndSelectItems(pidl, None, 0) };
    // SAFETY: `pidl` 由 `ILCreateFromPathW` 创建，只释放一次；此后不再使用。
    unsafe { ILFree(Some(pidl.cast_const())) };
    result
}

/// 退回显式启动资源管理器；替换了资源管理器的文件管理器接不到这个调用。
fn explorer(path: &Path) {
    let mut command = Command::new("explorer");
    command.raw_arg(explorer_select(path));
    match command.spawn() {
        Ok(mut child) => {
            let _ = child.wait();
        }
        Err(e) => eprintln!("winc: 无法打开资源管理器：{e}"),
    }
}

/// 返回让资源管理器选中 `path` 的命令行参数，原样传给 `explorer`。
///
/// 引号只包含路径：整个参数加引号时 `explorer` 无法解析，会改为打开默认的目录。
fn explorer_select(path: &Path) -> OsString {
    let mut select = OsString::from("/select,\"");
    select.push(path);
    select.push("\"");
    select
}

#[cfg(test)]
mod tests {
    use super::explorer_select;
    use std::path::Path;

    #[test]
    fn explorer_fallback_quotes_the_path_on_its_own() {
        let select = explorer_select(Path::new(r"C:\My Books\a b.epub"));
        assert_eq!(select, r#"/select,"C:\My Books\a b.epub""#);
    }
}
