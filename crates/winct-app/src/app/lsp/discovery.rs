//! 在文件系统里查找语言服务器。
//!
//! 这里不读取用户配置的服务器命令，也不依赖 `PATH` 是否完整：候选程序名来自语言表和用户的
//! `lsp.detectNames`，然后在项目目录、常见安装目录、`PATH` 以及这些目录的两层子目录里真实地
//! 找可执行文件。找到后返回绝对路径，这样即使从图形界面启动 Winct 也能启动服务器。

#![allow(
    clippy::disallowed_methods,
    reason = "language server discovery intentionally inspects the local filesystem"
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use winct_config::Config;

/// 一个候选程序名及其默认参数。
#[derive(Debug, Clone, Copy)]
struct Candidate {
    /// 可执行文件名（不带扩展名）。
    name: &'static str,
    /// 默认参数，如 `--stdio`。
    args: &'static [&'static str],
}

const fn candidate(name: &'static str, args: &'static [&'static str]) -> Candidate {
    Candidate { name, args }
}

/// 各语言常见的语言服务器。顺序即优先级。
const BUILTIN: &[(&str, &[Candidate])] = &[
    ("rust", &[candidate("rust-analyzer", &[])]),
    (
        "python",
        &[
            candidate("pyright-langserver", &["--stdio"]),
            candidate("pylsp", &[]),
            candidate("pyright", &[]),
        ],
    ),
    ("go", &[candidate("gopls", &[])]),
    (
        "json",
        &[candidate("vscode-json-language-server", &["--stdio"])],
    ),
    (
        "javascript",
        &[candidate("typescript-language-server", &["--stdio"])],
    ),
    (
        "typescript",
        &[candidate("typescript-language-server", &["--stdio"])],
    ),
    (
        "tsx",
        &[candidate("typescript-language-server", &["--stdio"])],
    ),
    (
        "javascriptreact",
        &[candidate("typescript-language-server", &["--stdio"])],
    ),
    ("c", &[candidate("clangd", &[])]),
    ("cpp", &[candidate("clangd", &[])]),
    ("objc", &[candidate("clangd", &[])]),
    ("objcpp", &[candidate("clangd", &[])]),
    ("php", &[candidate("phpactor", &["language-server"])]),
    ("ruby", &[candidate("solargraph", &["stdio"])]),
    ("java", &[candidate("jdtls", &[])]),
    ("kotlin", &[candidate("kotlin-language-server", &[])]),
    ("dart", &[candidate("dart", &["language-server"])]),
    (
        "html",
        &[candidate("vscode-html-language-server", &["--stdio"])],
    ),
    (
        "css",
        &[candidate("vscode-css-language-server", &["--stdio"])],
    ),
    (
        "scss",
        &[candidate("vscode-css-language-server", &["--stdio"])],
    ),
    (
        "less",
        &[candidate("vscode-css-language-server", &["--stdio"])],
    ),
    ("yaml", &[candidate("yaml-language-server", &["--stdio"])]),
    ("lua", &[candidate("lua-language-server", &[])]),
    ("bash", &[candidate("bash-language-server", &["start"])]),
    (
        "shellscript",
        &[candidate("bash-language-server", &["start"])],
    ),
    ("sql", &[candidate("sqls", &[])]),
    (
        "dockerfile",
        &[candidate("docker-language-server", &["start"])],
    ),
    ("toml", &[candidate("taplo", &["lsp"])]),
    ("vue", &[candidate("vue-language-server", &["--stdio"])]),
    (
        "svelte",
        &[candidate("svelte-language-server", &["--stdio"])],
    ),
    ("elm", &[candidate("elm-language-server", &["--stdio"])]),
    ("clojure", &[candidate("clojure-lsp", &[])]),
    (
        "haskell",
        &[candidate("haskell-language-server-wrapper", &["--lsp"])],
    ),
    ("nix", &[candidate("nixd", &[])]),
];

/// 找到的语言服务器。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DetectedServer {
    /// 稳定标识：`detect:<程序名>`。
    pub(crate) id: String,
    /// 完整启动命令，第一项是绝对路径。
    pub(crate) command: Vec<String>,
    /// 找到它的方式，供设置和状态栏显示。
    pub(crate) source: String,
}

/// 返回某语言需要尝试的所有程序名和参数。
///
/// 用户通过 `lsp.detectNames` 写出的名字排在内建名字之前，方便覆盖默认选择。
fn candidates(language: &str, config: &Config) -> Vec<(String, Vec<String>)> {
    let mut result = Vec::new();
    if let Some(names) = config.lsp_detect_names.get(language) {
        for name in names {
            if !name.is_empty() {
                result.push((name.clone(), Vec::new()));
            }
        }
    }
    if let Some(builtin) = BUILTIN.iter().find(|(id, _)| *id == language) {
        for entry in builtin.1 {
            result.push((
                entry.name.to_owned(),
                entry.args.iter().map(|arg| (*arg).to_owned()).collect(),
            ));
        }
    }
    let mut seen = BTreeSet::new();
    result.retain(|(name, _)| seen.insert(name.clone()));
    result
}

/// 配置文件所在目录之外的常见安装目录。
fn common_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home = PathBuf::from(home);
        dirs.extend([
            home.join(".cargo/bin"),
            home.join(".local/bin"),
            home.join(".npm-global/bin"),
            home.join("bin"),
            home.join(".volta/bin"),
            home.join(".asdf/shims"),
            home.join("go/bin"),
            home.join("AppData/Local/Programs"),
            home.join("AppData/Local/Microsoft/WindowsApps"),
        ]);
    }
    for key in [
        "LOCALAPPDATA",
        "APPDATA",
        "ProgramFiles",
        "ProgramFiles(x86)",
    ] {
        if let Some(value) = std::env::var_os(key) {
            dirs.push(PathBuf::from(value));
        }
    }
    dirs.extend([
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/opt/local/bin"),
        PathBuf::from("/opt/bin"),
    ]);
    dirs
}

/// 收集要搜索的目录：项目、用户配置、常见安装位置和整个 `PATH`。
fn search_dirs(root: &Path, config: &Config) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut push = |dir: PathBuf| {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    };
    push(root.to_path_buf());
    push(root.join(".winct/lsp"));
    push(root.join("node_modules/.bin"));
    push(root.join(".bin"));
    for dir in &config.lsp_detect_dirs {
        let path = PathBuf::from(dir);
        if path.is_absolute() {
            push(path);
        } else {
            push(root.join(path));
        }
    }
    if let Some(parent) =
        winct_config::config_path().and_then(|path| path.parent().map(Path::to_path_buf))
    {
        push(parent.join("lsp"));
    }
    for dir in common_dirs() {
        push(dir);
    }
    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        if dir.is_absolute() {
            push(dir);
        }
    }
    dirs.retain(|dir| dir.is_dir());
    dirs
}

/// 把候选名按平台补全扩展名。
fn file_names(name: &str) -> Vec<String> {
    let mut names = vec![name.to_owned()];
    if cfg!(windows) {
        for extension in ["exe", "cmd", "bat", "ps1"] {
            names.push(format!("{name}.{extension}"));
        }
    }
    names
}

#[cfg(unix)]
fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(path: &Path) -> bool {
    path.is_file()
        && matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("exe" | "cmd" | "bat" | "ps1") | None
        )
}

/// 在目录里找一个候选名；找不到时再向下看两层子目录。
fn find_in(dir: &Path, name: &str) -> Option<PathBuf> {
    for file_name in file_names(name) {
        let direct = dir.join(&file_name);
        if executable(&direct) {
            return Some(direct);
        }
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return None;
    };
    let mut children: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .take(64)
        .collect();
    for level in 0..2 {
        let mut next = Vec::new();
        for child in &children {
            for file_name in file_names(name) {
                let candidate = child.join(&file_name);
                if executable(&candidate) {
                    return Some(candidate);
                }
            }
            if level == 0
                && let Ok(entries) = std::fs::read_dir(child)
            {
                next.extend(
                    entries
                        .filter_map(|entry| entry.ok())
                        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                        .map(|entry| entry.path())
                        .take(32),
                );
            }
        }
        children = next;
    }
    None
}

/// 为语言查找所有可用的语言服务器。
///
/// `root` 是当前项目根目录，用来找项目自带的服务器（例如 `node_modules/.bin`）。
pub(crate) fn discover(language: &str, root: &Path, config: &Config) -> Vec<DetectedServer> {
    if config.lsp_detect == Some(false) {
        return Vec::new();
    }
    let dirs = search_dirs(root, config);
    let mut result = Vec::new();
    for (name, args) in candidates(language, config) {
        for dir in &dirs {
            let Some(path) = find_in(dir, &name) else {
                continue;
            };
            let mut command = vec![path.to_string_lossy().into_owned()];
            command.extend(args);
            result.push(DetectedServer {
                id: format!("detect:{name}"),
                command,
                source: dir.to_string_lossy().into_owned(),
            });
            break;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_names_are_deduplicated_and_user_names_come_first() {
        let mut config = Config::default();
        config.lsp_detect_names.insert(
            "rust".to_owned(),
            vec!["my-analyzer".to_owned(), "rust-analyzer".to_owned()],
        );
        let names: Vec<_> = candidates("rust", &config)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(names, ["my-analyzer", "rust-analyzer"]);
    }

    #[test]
    fn discovers_a_custom_executable_from_the_project_directory() {
        let dir = std::env::temp_dir().join(format!("winct-lsp-discover-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let name = if cfg!(windows) {
            "my-analyzer.exe"
        } else {
            "my-analyzer"
        };
        let program = dir.join(name);
        std::fs::write(&program, b"not really an executable").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let mut config = Config::default();
        config
            .lsp_detect_names
            .insert("rust".to_owned(), vec!["my-analyzer".to_owned()]);
        config
            .lsp_detect_dirs
            .push(dir.to_string_lossy().into_owned());

        let found = discover("rust", &dir, &config);
        assert_eq!(
            found.first().map(|server| server.id.as_str()),
            Some("detect:my-analyzer")
        );
        assert_eq!(
            found
                .first()
                .and_then(|server| server.command.first())
                .map(String::as_str),
            Some(program.to_string_lossy().as_ref())
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detection_can_be_turned_off() {
        let config = Config {
            lsp_detect: Some(false),
            ..Config::default()
        };
        assert!(discover("rust", Path::new("/"), &config).is_empty());
    }
}
