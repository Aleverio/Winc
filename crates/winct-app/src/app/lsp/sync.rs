use super::*;
use std::collections::BTreeMap;

/// 实际用于启动语言服务器的完整配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedLspServer {
    /// 稳定标识，用于状态显示。
    pub(crate) id: String,
    /// 启动命令，第一项是可执行文件。
    pub(crate) command: Vec<String>,
    /// 传给服务器的额外环境变量。
    pub(crate) env: BTreeMap<String, String>,
    /// 工作目录。
    pub(crate) cwd: Option<PathBuf>,
    /// 来源：`configured`、`detected` 或 `built-in`。
    pub(crate) source: &'static str,
}

impl App {
    /// 为某个语言和工作区选定语言服务器。
    ///
    /// 优先用户在设置里配置的服务器；勾选了自动选择的语言优先使用真实查找到的服务器；
    /// 没有显式配置的语言也会自动查找。`lsp.detect` 为 `false` 时完全不查找。
    pub(crate) fn lsp_resolve_server(
        &self,
        language: &str,
        root: &Path,
    ) -> Option<ResolvedLspServer> {
        let auto = self.config.lsp_auto.iter().any(|id| id == language);
        let configured = self
            .config
            .lsp_server_entries
            .get(language)
            .and_then(|entries| entries.iter().find(|entry| entry.enabled))
            .map(|entry| ResolvedLspServer {
                id: entry.id.clone(),
                command: entry.command.clone(),
                env: entry.env.clone(),
                cwd: entry.cwd.as_ref().map(PathBuf::from),
                source: "configured",
            })
            .or_else(|| {
                self.config
                    .lsp_servers
                    .get(language)
                    .map(|command| ResolvedLspServer {
                        id: "configured".to_owned(),
                        command: command.clone(),
                        env: BTreeMap::new(),
                        cwd: None,
                        source: "configured",
                    })
            });

        if !auto && configured.is_some() {
            return configured;
        }

        if self.config.lsp_detect != Some(false) {
            let key = (language.to_owned(), root.to_path_buf());
            let mut detected_cache = self.lsp_detected.borrow_mut();
            if !detected_cache.contains_key(&key) {
                let found = super::discovery::discover(language, root, &self.config);
                detected_cache.insert(key.clone(), found);
            }
            if let Some(detected) = detected_cache.get(&key).and_then(|list| list.first()) {
                return Some(ResolvedLspServer {
                    id: detected.id.clone(),
                    command: detected.command.clone(),
                    env: BTreeMap::new(),
                    cwd: None,
                    source: "detected",
                });
            }
        }

        configured.or_else(|| {
            auto_server_command(language).map(|command| ResolvedLspServer {
                id: "built-in".to_owned(),
                command,
                env: BTreeMap::new(),
                cwd: None,
                source: "built-in",
            })
        })
    }

    /// 配置或自动查找发生变化后，丢弃查找缓存。
    pub(crate) fn clear_lsp_detection_cache(&self) {
        self.lsp_detected.borrow_mut().clear();
    }
}

pub(crate) fn auto_server_command(language: &str) -> Option<Vec<String>> {
    let (program, args): (&str, &[&str]) = match language {
        "rust" => ("rust-analyzer", &[]),
        "python" => ("pyright-langserver", &["--stdio"]),
        "go" => ("gopls", &[]),
        "json" => ("vscode-json-language-server", &["--stdio"]),
        _ => return None,
    };
    Some(
        std::iter::once(program.to_owned())
            .chain(args.iter().map(|arg| (*arg).to_owned()))
            .collect(),
    )
}

pub(crate) fn split_server_command(command: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = None::<String>;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if let Some(arg) = current.take() {
                    args.push(arg);
                }
            }
            '\'' => {
                let arg = current.get_or_insert_with(String::new);
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => arg.push(c),
                        None => return Err("Unclosed single quote in server command".to_owned()),
                    }
                }
            }
            '"' => {
                let arg = current.get_or_insert_with(String::new);
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') if matches!(chars.peek(), Some('"' | '\\')) => {
                            arg.extend(chars.next());
                        }
                        Some(c) => arg.push(c),
                        None => return Err("Unclosed double quote in server command".to_owned()),
                    }
                }
            }
            c => current.get_or_insert_with(String::new).push(c),
        }
    }
    args.extend(current);
    if args.is_empty() {
        return Err("A language server executable is required".to_owned());
    }
    Ok(args)
}

pub(crate) fn display_lsp_command(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if arg.chars().any(char::is_whitespace) || arg.contains('"') {
                format!("\"{}\"", arg.replace('\\', "\\\\").replace('"', "\\\""))
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build one ranged LSP edit, using the known unchanged prefix and a common suffix.
pub(crate) fn incremental_change(
    old: &Rope,
    new: &Rope,
    unchanged_lines: usize,
) -> Option<TextDocumentContentChangeEvent> {
    let unchanged_lines = unchanged_lines
        .min(old.len_lines() - 1)
        .min(new.len_lines() - 1);
    let prefix_start = old.line_to_char(unchanged_lines);
    debug_assert_eq!(prefix_start, new.line_to_char(unchanged_lines));

    let mut old_prefix = old.slice(prefix_start..).chars();
    let mut new_prefix = new.slice(prefix_start..).chars();
    let mut start = prefix_start;
    while let (Some(old), Some(new)) = (old_prefix.next(), new_prefix.next()) {
        if old != new {
            break;
        }
        start += 1;
    }

    let old_tail = old.slice(start..);
    let new_tail = new.slice(start..);
    let mut old_chars = old_tail.chars_at(old_tail.len_chars()).reversed();
    let mut new_chars = new_tail.chars_at(new_tail.len_chars()).reversed();
    let mut suffix = 0;
    while let (Some(old), Some(new)) = (old_chars.next(), new_chars.next()) {
        if old != new {
            break;
        }
        suffix += 1;
    }

    let old_end = old.len_chars() - suffix;
    let new_end = new.len_chars() - suffix;
    if start == old_end && start == new_end {
        return None;
    }

    let position = |text: &Rope, char_index: usize| {
        let line = text.char_to_line(char_index);
        let line_start = text.line_to_char(line);
        lsp_types::Position::new(
            line.min(u32::MAX as usize) as u32,
            text.slice(line_start..char_index)
                .len_utf16_cu()
                .min(u32::MAX as usize) as u32,
        )
    };
    Some(TextDocumentContentChangeEvent {
        range: Some(lsp_types::Range::new(
            position(old, start),
            position(old, old_end),
        )),
        range_length: Some(
            old.slice(start..old_end)
                .len_utf16_cu()
                .min(u32::MAX as usize) as u32,
        ),
        text: new.slice(start..new_end).to_string(),
    })
}
