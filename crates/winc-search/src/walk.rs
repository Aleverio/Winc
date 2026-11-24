//! 遍历工作区目录：哪些文件收录、按什么顺序排列，以及依赖目录在哪里。
//!
//! 文件树、快速打开的文件索引和搜索共用这里的规则，看到的文件相同；本机与远端主机上的服务进程
//! 也共用，远端的目录在远端遍历。

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

/// 文件树中不显示的目录和文件名（按名字精确匹配），快速打开的文件索引同样跳过。
pub const HIDDEN: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    ".idea",
    ".DS_Store",
    "__pycache__",
    ".mypy_cache",
    ".pytest_cache",
];

/// 返回遍历工作区目录的配置，快速打开的文件索引和全局搜索共用，两者看到的文件相同。
///
/// 遵守各级 `.gitignore`、`.ignore` 和全局的忽略规则，不进入指向目录的符号链接，跳过
/// [`HIDDEN`] 中的名字；指向文件的符号链接是否收录由 [`is_listed_file`] 判断。
///
/// 在版本库内时 `.gitignore` 只到版本库的根目录为止，与 Git 相同，上级目录的规则不影响版本库。
/// 不在版本库内时 `.gitignore` 同样生效：下载的源码包也不会列出 `node_modules` 这类目录。
pub fn walker(dir: &Path) -> WalkBuilder {
    let mut builder = WalkBuilder::new(dir);
    // `hidden(false)` 关闭 `ignore` 对以 `.` 开头的名字的过滤：`.github`、`.editorconfig`
    // 仍然收录，只有 [`HIDDEN`] 中的名字由 `filter_entry` 跳过。
    builder
        .hidden(false)
        .parents(true)
        .ignore(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .require_git(in_repository(dir))
        .follow_links(false)
        .filter_entry(|entry| !HIDDEN.contains(&entry.file_name().to_string_lossy().as_ref()));
    builder
}

/// 同 [`walker`]，但不遵守忽略规则，只跳过 [`HIDDEN`] 中的名字。
pub fn walker_with_ignored(dir: &Path) -> WalkBuilder {
    let mut builder = walker(dir);
    builder
        .parents(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false);
    builder
}

/// `dir` 或它的某一级上级目录是否是版本库的根目录。
fn in_repository(dir: &Path) -> bool {
    dir.ancestors().any(|dir| {
        winc_backend::fs::exists(dir.join(".git")) || winc_backend::fs::exists(dir.join(".jj"))
    })
}

/// 判断类型为 `kind` 的目录项 `path` 是否作为文件收录：普通文件，或指向普通文件的符号链接。
///
/// 只有符号链接才多做一次 stat。指向目录的符号链接不收录、不进入，避免链接成环时无限遍历，
/// 以及同一棵目录被重复收录。
pub fn is_listed_file(kind: impl Into<winc_backend::fs::Kind>, path: &Path) -> bool {
    let kind = kind.into();
    kind.is_file() || kind.is_symlink() && winc_backend::fs::is_file(path)
}

/// 依赖目录的名字：Node 的、Python 虚拟环境的、PHP 和 Go 的 `vendor`、iOS 的 `Pods`。
pub const DEPENDENCY_FOLDERS: [&str; 5] = ["node_modules", ".venv", "venv", "vendor", "Pods"];
/// 往下找依赖目录的层数：根目录下一层和两层（如 `frontend/node_modules`）。
const DEPENDENCY_DEPTH: usize = 2;

/// 返回 `dir` 下两层以内（如 `frontend/node_modules`）的依赖目录，按路径排序；不进依赖目录和隐藏的目录
/// （[`HIDDEN`]）里找，不跟随符号链接。
pub fn dependency_folders(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut level = vec![dir.to_path_buf()];
    for _ in 0..DEPENDENCY_DEPTH {
        let mut next = Vec::new();
        for parent in &level {
            let entries = winc_backend::fs::read_dir(parent)
                .into_iter()
                .flatten()
                .flatten();
            for entry in entries.filter(|entry| entry.kind.is_dir()) {
                let path = entry.path();
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
                match name.as_deref() {
                    Some(name) if DEPENDENCY_FOLDERS.contains(&name) => found.push(path),
                    Some(name) if HIDDEN.contains(&name) => {}
                    Some(_) => next.push(path),
                    None => {}
                }
            }
        }
        level = next;
    }
    found.sort();
    found
}

/// 按自然顺序比较两个名字：连续数字按数值比较，其余字符不区分大小写，因此 `file2` 排在
/// `file10` 之前。
///
/// 这是全序，只有两个名字完全相同时才返回 `Equal`：按上述规则相同时，先比较第一处数值相同、
/// 位数不同的数字（位数少的在前，`file1` 排在 `file01` 之前），再按原样逐字节比较（`A.rs`
/// 排在 `a.rs` 之前）。
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (whole_a, whole_b) = (a, b);
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    // 第一处数值相同、位数不同的数字的比较结果，规则相同时据此区分。
    let mut digits = Ordering::Equal;

    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return digits.then_with(|| whole_a.cmp(whole_b)),
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let ((x, x_len), (y, y_len)) = (take_number(&mut a), take_number(&mut b));
                match x.cmp(&y) {
                    Ordering::Equal => digits = digits.then(x_len.cmp(&y_len)),
                    other => return other,
                }
            }
            (Some(x), Some(y)) => {
                match x.to_lowercase().cmp(y.to_lowercase()) {
                    Ordering::Equal => {}
                    other => return other,
                }

                a.next();
                b.next();
            }
        }
    }
}

/// 读取 `chars` 开头的连续数字并前移迭代器，返回其数值（超过 `u128` 的范围时饱和）和位数。
fn take_number(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> (u128, usize) {
    let (mut n, mut len): (u128, usize) = (0, 0);

    while let Some(d) = chars.peek().and_then(|c| c.to_digit(10)) {
        n = n.saturating_mul(10).saturating_add(d as u128);
        len += 1;
        chars.next();
    }

    (n, len)
}

#[cfg(test)]
mod tests;
