use super::*;

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
