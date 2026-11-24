//! 列出远端主机上一个目录中的文件：在那台主机上运行一条命令，一次往返。
//!
//! 快速打开的文件表要知道目录中有哪些文件。逐个目录列出要每个目录一次往返，
//! 忽略规则的文件也都在远端，本机的遍历用不上。是 Git 仓库时用 `git`（已跟踪的和未被忽略的
//! 未跟踪文件，规则与本机的遍历相同）；不是仓库或远端没有 `git` 时用 `find`，只跳过
//! [`HIDDEN`] 中的名字。

use std::path::Path;

use winc_backend::remote::{Exec, exec};

use super::tree::HIDDEN;

/// 命令输出的字节数上限（64 MiB）：二十万个文件、每个路径数百字节以内。
const MAX_LISTING: usize = 64 << 20;

/// 列出文件最长的运行时间（60 秒）：`git status` 上限的两倍，清单还要经连接传回本机。
///
/// 超过该时间视为远端的命令失去响应，结果是 [`TimedOut`]。
const LIST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// 返回在远端运行 `git` 的命令，调用方继续追加参数。
///
/// 不运行仓库配置的 fsmonitor：列出文件发生在打开目录时，仓库的配置尚未经用户确认。
fn git() -> Exec {
    Exec::new("git").arg("-c").arg("core.fsmonitor=false")
}

/// 列出文件的命令运行超过 [`LIST_TIMEOUT`]，已被结束。
#[derive(Debug)]
pub(crate) struct TimedOut;

/// 返回远端目录 `root` 中各文件相对于它的路径（以 `/` 分隔），顺序不定。
///
/// `ignored` 为真时也列出被忽略规则排除的文件。`stop` 返回真时结束命令并返回空表，超时则返回
/// [`TimedOut`]。名字不是有效 UTF-8、或含 [`HIDDEN`] 中的名字的路径不列出。
pub(crate) fn list(
    root: &Path,
    ignored: bool,
    stop: &dyn Fn() -> bool,
) -> Result<Vec<String>, TimedOut> {
    let deadline = std::time::Instant::now() + LIST_TIMEOUT;
    let late = std::cell::Cell::new(false);
    let stop = || {
        late.set(late.get() || std::time::Instant::now() >= deadline);
        late.get() || stop()
    };
    let run = |command: Exec| exec(root, &command.limit(MAX_LISTING), &stop).ok();
    let command = git()
        .arg("ls-files")
        .arg("-z")
        .arg("--cached")
        .arg("--others");
    let command = match ignored {
        true => command,
        false => command.arg("--exclude-standard"),
    };
    // 非零的退出码是没能运行（不是仓库、没有 `git`）。被结束或超时的不再改用 `find`。
    let listing = match run(command) {
        Some(output) if output.code == Some(0) => output.stdout,
        _ if stop() => Vec::new(),
        _ => {
            // `find . ( -name A -o -name B … ) -prune -o -type f -print0`
            let mut find = Exec::new("find").arg(".").arg("(");
            for (index, name) in HIDDEN.iter().enumerate() {
                if index > 0 {
                    find = find.arg("-o");
                }
                find = find.arg("-name").arg(name);
            }
            find = find.arg(")").arg("-prune").arg("-o").arg("-type").arg("f");
            run(find.arg("-print0"))
                .map(|output| output.stdout)
                .unwrap_or_default()
        }
    };
    if late.get() {
        return Err(TimedOut);
    }
    if stop() {
        return Ok(Vec::new());
    }
    let files = (listing.split(|&byte| byte == 0))
        .filter_map(|path| std::str::from_utf8(path).ok())
        .map(|path| path.strip_prefix("./").unwrap_or(path))
        .filter(|path| !path.is_empty() && !path.split('/').any(|part| HIDDEN.contains(&part)))
        .map(str::to_owned)
        .collect();
    Ok(files)
}
