use crate::{Language, Token, TokenKind};

/// Tree-sitter 处理的最大源文件大小（字节）。
///
/// 目前 Tree-sitter 支持 Rust、Python、JavaScript、TypeScript/TSX 和 JSON；其他语言及更大的文件仍由
/// Winct 的逐行 lexer 处理。
pub const MAX_TREE_SITTER_BYTES: usize = 256 * 1024;

pub(super) fn supports(language: &str) -> bool {
    matches!(
        language,
        "rust" | "python" | "javascript" | "typescript" | "json"
    )
}

#[derive(Debug)]
pub(super) struct TreeTokens {
    pub(super) language_id: &'static str,
    source: String,
    tree: ::tree_sitter::Tree,
    pub(super) lines: Vec<Vec<Token>>,
}

impl TreeTokens {
    pub(super) fn update(
        language: &'static Language,
        source: &str,
        previous: Option<Self>,
    ) -> Option<Self> {
        if source.len() > MAX_TREE_SITTER_BYTES {
            return None;
        }
        let previous = match previous {
            Some(previous) if previous.language_id == language.id && previous.source == source => {
                return Some(previous);
            }
            previous => previous.filter(|previous| previous.language_id == language.id),
        };

        let tree_language = grammar(language.id)?;
        let mut parser = ::tree_sitter::Parser::new();
        parser
            .set_language(&tree_language)
            .expect("bundled Tree-sitter grammar must have a compatible ABI");

        let tree = match previous {
            Some(previous) => {
                let mut tree = previous.tree;
                let edit = input_edit(&previous.source, source);
                tree.edit(&edit);
                parser
                    .parse(source, Some(&tree))
                    .expect("Tree-sitter parsing is not cancelled")
            }
            None => parser
                .parse(source, None)
                .expect("Tree-sitter parsing is not cancelled"),
        };
        let lines = collect_tokens(&tree, source, language);

        Some(Self {
            language_id: language.id,
            source: source.to_owned(),
            tree,
            lines,
        })
    }
}

fn grammar(language: &str) -> Option<::tree_sitter::Language> {
    match language {
        "rust" => Some(tree_sitter_rust::LANGUAGE.into()),
        "python" => Some(tree_sitter_python::LANGUAGE.into()),
        "javascript" => Some(tree_sitter_javascript::LANGUAGE.into()),
        "typescript" => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
        "json" => Some(tree_sitter_json::LANGUAGE.into()),
        _ => None,
    }
}

fn input_edit(old: &str, new: &str) -> ::tree_sitter::InputEdit {
    let mut start = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(old, new)| old == new)
        .count();
    while !old.is_char_boundary(start) || !new.is_char_boundary(start) {
        start -= 1;
    }

    let mut suffix = old
        .bytes()
        .rev()
        .zip(new.bytes().rev())
        .take_while(|(old, new)| old == new)
        .count()
        .min(old.len() - start)
        .min(new.len() - start);
    while !old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix) {
        suffix -= 1;
    }

    let old_end = old.len() - suffix;
    let new_end = new.len() - suffix;
    ::tree_sitter::InputEdit {
        start_byte: start,
        old_end_byte: old_end,
        new_end_byte: new_end,
        start_position: point_at(old, start),
        old_end_position: point_at(old, old_end),
        new_end_position: point_at(new, new_end),
    }
}

fn point_at(source: &str, byte: usize) -> ::tree_sitter::Point {
    let prefix = &source[..byte];
    let row = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let column = prefix
        .rfind('\n')
        .map_or(prefix.len(), |newline| prefix.len() - newline - 1);
    ::tree_sitter::Point { row, column }
}

fn collect_tokens(
    tree: &::tree_sitter::Tree,
    source: &str,
    language: &Language,
) -> Vec<Vec<Token>> {
    let ranges = line_ranges(source);
    let mut lines = vec![Vec::new(); ranges.len()];
    collect_node_tokens(tree.root_node(), source, language, &ranges, &mut lines);
    for line in &mut lines {
        line.sort_unstable_by_key(|token| (token.start, token.end));
        let mut merged: Vec<Token> = Vec::with_capacity(line.len());
        for token in line.drain(..) {
            if let Some(last) = merged.last_mut() {
                if token.start < last.end {
                    if token.kind == last.kind {
                        last.end = last.end.max(token.end);
                    }
                    continue;
                }
                if token.start == last.end && token.kind == last.kind {
                    last.end = token.end;
                    continue;
                }
            }
            merged.push(token);
        }
        *line = merged;
    }
    lines
}

fn line_ranges(source: &str) -> Vec<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'\r' {
            ranges.push((start, at));
            at += if bytes.get(at + 1) == Some(&b'\n') {
                2
            } else {
                1
            };
            start = at;
        } else if bytes[at] == b'\n' {
            ranges.push((start, at));
            at += 1;
            start = at;
        } else {
            at += 1;
        }
    }
    ranges.push((start, bytes.len()));
    ranges
}

fn collect_node_tokens(
    root: ::tree_sitter::Node<'_>,
    source: &str,
    language: &Language,
    lines: &[(usize, usize)],
    output: &mut [Vec<Token>],
) {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if node.is_missing() {
            continue;
        }

        if let Some((kind, atomic)) = classify(node, source, language) {
            add_span(
                node.start_byte(),
                node.end_byte(),
                kind,
                source,
                lines,
                output,
            );
            if atomic {
                continue;
            }
        }

        let mut cursor = node.walk();
        pending.extend(node.children(&mut cursor));
    }
}

fn classify(
    node: ::tree_sitter::Node<'_>,
    source: &str,
    language: &Language,
) -> Option<(TokenKind, bool)> {
    let kind = node.kind();
    let text = source.get(node.byte_range())?;

    if kind.contains("comment") {
        return Some((TokenKind::Comment, true));
    }
    if is_string_kind(kind) {
        if has_interpolation_child(node) {
            return None;
        }
        if is_json_key(node) {
            return Some((TokenKind::Property, true));
        }
        return Some((TokenKind::String, true));
    }
    if kind == "escape_sequence" {
        return Some((TokenKind::Escape, true));
    }
    if kind.contains("integer")
        || kind.contains("float")
        || kind == "number"
        || kind == "number_literal"
    {
        return Some((TokenKind::Number, true));
    }
    if kind.contains("type_identifier")
        || kind == "primitive_type"
        || kind == "predefined_type"
        || kind == "class_name"
        || is_type_name(node)
    {
        return Some((TokenKind::Type, true));
    }
    if is_function_name(node) {
        return Some((TokenKind::Function, true));
    }
    if language.keywords.contains(&text) {
        return Some((TokenKind::Keyword, true));
    }
    if language.builtins.contains(&text) || matches!(text, "true" | "false" | "null" | "None") {
        return Some((TokenKind::Builtin, true));
    }
    None
}

fn is_string_kind(kind: &str) -> bool {
    kind.contains("string")
        || kind.contains("character_literal")
        || kind == "char_literal"
        || kind == "string_fragment"
}

fn has_interpolation_child(node: ::tree_sitter::Node<'_>) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor).any(|child| {
        let kind = child.kind();
        kind.contains("interpolation")
            || kind.contains("template_substitution")
            || kind == "substitution"
    })
}

fn is_json_key(node: ::tree_sitter::Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.kind() != "pair" {
        return false;
    }
    parent
        .child_by_field_name("key")
        .is_some_and(|key| key.start_byte() == node.start_byte())
}

fn is_function_name(node: ::tree_sitter::Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let function = matches!(
        parent.kind(),
        "function_item"
            | "function_definition"
            | "function_declaration"
            | "method_definition"
            | "method_declaration"
            | "generator_function_declaration"
    );
    function
        && parent
            .child_by_field_name("name")
            .is_some_and(|name| name.start_byte() == node.start_byte())
}

fn is_type_name(node: ::tree_sitter::Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let declaration = matches!(
        parent.kind(),
        "class_declaration"
            | "class_definition"
            | "class_specifier"
            | "enum_declaration"
            | "enum_item"
            | "interface_declaration"
            | "struct_item"
            | "struct_specifier"
            | "type_alias_declaration"
            | "type_definition"
            | "type_spec"
    );
    declaration
        && parent
            .child_by_field_name("name")
            .is_some_and(|name| name.start_byte() == node.start_byte())
}

fn add_span(
    start: usize,
    end: usize,
    kind: TokenKind,
    source: &str,
    lines: &[(usize, usize)],
    output: &mut [Vec<Token>],
) {
    if start >= end {
        return;
    }
    let mut row = lines.partition_point(|(line_start, _)| *line_start <= start);
    row = row.saturating_sub(1);
    while let Some(&(line_start, line_end)) = lines.get(row) {
        if line_start >= end {
            break;
        }
        let token_start = start.max(line_start);
        let token_end = end.min(line_end);
        if token_start < token_end {
            let prefix = source[line_start..token_start].chars().count();
            let text = source[line_start..token_end].chars().count();
            output[row].push(Token {
                start: prefix,
                end: text,
                kind,
            });
        }
        row += 1;
    }
}

/// 将 Tree-sitter token 覆盖到 lexer 结果上；已有的专用 token 类别优先保留。
pub fn overlay_tree_tokens(tokens: &mut Vec<Token>, tree: &[Token], char_len: usize) {
    if tree.is_empty() || char_len == 0 {
        return;
    }

    let mut overlays = Vec::with_capacity(tree.len());
    for token in tree.iter().copied() {
        let end = token.end.min(char_len);
        if token.start >= end {
            continue;
        }
        let mut segments = vec![(token.start, end)];
        for existing in tokens
            .iter()
            .copied()
            .filter(|existing| existing.kind != TokenKind::Text && existing.kind != token.kind)
        {
            let mut remaining = Vec::with_capacity(segments.len() + 1);
            for (start, end) in segments {
                if existing.end <= start || existing.start >= end {
                    remaining.push((start, end));
                    continue;
                }
                if start < existing.start {
                    remaining.push((start, existing.start));
                }
                if existing.end < end {
                    remaining.push((existing.end, end));
                }
            }
            segments = remaining;
            if segments.is_empty() {
                break;
            }
        }
        overlays.extend(segments.into_iter().map(|(start, end)| Token {
            start,
            end,
            kind: token.kind,
        }));
    }

    let mut result = Vec::with_capacity(tokens.len() + overlays.len());
    for token in tokens.iter().copied() {
        let end = token.end.min(char_len);
        if token.start >= end {
            continue;
        }
        let mut cursor = token.start;
        for overlay in overlays.iter().copied() {
            let overlay_end = overlay.end;
            if overlay_end <= cursor {
                continue;
            }
            if overlay.start >= end {
                break;
            }
            if overlay.start > cursor {
                result.push(Token {
                    start: cursor,
                    end: overlay.start.min(end),
                    kind: token.kind,
                });
            }
            cursor = cursor.max(overlay_end.min(end));
            if cursor >= end {
                break;
            }
        }
        if cursor < end {
            result.push(Token {
                start: cursor,
                end,
                kind: token.kind,
            });
        }
    }
    result.extend(overlays);
    result.sort_unstable_by_key(|token| (token.start, token.end));
    *tokens = result;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn tokens(language: &str, source: &str) -> Vec<Vec<Token>> {
        let language = crate::find(language).unwrap();
        TreeTokens::update(language, source, None).unwrap().lines
    }

    #[test]
    #[ignore = "repeatable local Tree-sitter latency benchmark"]
    fn tree_sitter_update_benchmark() {
        let language = crate::find("rust").unwrap();
        let mut source = (0..5_000)
            .map(|line| format!("fn item_{line}() {{ let value = {line}; }}\n"))
            .collect::<String>();
        let source_bytes = source.len();
        assert!(source_bytes <= MAX_TREE_SITTER_BYTES);
        let mut previous = TreeTokens::update(language, &source, None);
        let mut cold = Vec::new();
        let mut incremental = Vec::new();

        for iteration in 0..21 {
            let started = Instant::now();
            let _tree = TreeTokens::update(language, &source, None).unwrap();
            cold.push(started.elapsed());

            let replace = iteration % 2 == 0;
            let next_source = if replace {
                source.replacen("let value = 0", "let value = 1", 1)
            } else {
                source.replacen("let value = 1", "let value = 0", 1)
            };
            let started = Instant::now();
            previous = TreeTokens::update(language, &next_source, previous);
            incremental.push(started.elapsed());
            source = next_source;
        }

        cold.sort_unstable();
        incremental.sort_unstable();
        println!(
            "Tree-sitter Rust ({source_bytes} bytes): cold median={:?}, incremental median={:?}",
            cold[cold.len() / 2],
            incremental[incremental.len() / 2]
        );
    }

    #[test]
    fn rust_ast_classifies_declarations_and_multiline_comments() {
        let lines = tokens(
            "rust",
            "fn greet(name: &str) {\n    /* note\n       more */\n    let value = 42;\n}\n",
        );
        assert!(
            lines[0]
                .iter()
                .any(|token| token.kind == TokenKind::Function)
        );
        assert_eq!(
            lines[1]
                .iter()
                .find(|token| token.kind == TokenKind::Comment)
                .map(|token| token.start),
            Some(4)
        );
        assert!(
            lines[2]
                .iter()
                .any(|token| token.kind == TokenKind::Comment)
        );
        assert!(lines[3].iter().any(|token| token.kind == TokenKind::Number));
    }

    #[test]
    fn token_lines_follow_lf_crlf_and_cr_document_boundaries() {
        let lines = tokens("rust", "fn first() {}\rfn second() {}\r\nfn third() {}\n");
        assert_eq!(lines.len(), 4);
        assert!(
            lines[0]
                .iter()
                .any(|token| token.kind == TokenKind::Function)
        );
        assert!(
            lines[1]
                .iter()
                .any(|token| token.kind == TokenKind::Function)
        );
        assert!(
            lines[2]
                .iter()
                .any(|token| token.kind == TokenKind::Function)
        );
        assert!(lines[3].is_empty());
    }

    #[test]
    fn python_strings_keep_interpolation_code_visible() {
        let source = "message = f\"hello {name}\"\n";
        let lines = tokens("python", source);
        assert!(lines[0].iter().any(|token| token.kind == TokenKind::String));
        let name = source[..source.find("{name}").unwrap() + 1].chars().count();
        assert!(!lines[0].iter().any(|token| token.kind == TokenKind::String
            && token.start <= name
            && name < token.end));
    }

    #[test]
    fn json_object_keys_are_properties() {
        let source = "{\"名前\": \"李\", \"count\": 2}";
        let lines = tokens("json", source);
        assert_eq!(
            lines[0]
                .iter()
                .filter(|token| token.kind == TokenKind::Property)
                .count(),
            2
        );
        assert!(lines[0].iter().any(|token| token.kind == TokenKind::Number));
        let key = lines[0]
            .iter()
            .find(|token| token.kind == TokenKind::Property)
            .unwrap();
        assert_eq!(
            source
                .chars()
                .skip(key.start)
                .take(key.end - key.start)
                .collect::<String>(),
            "\"名前\""
        );
    }

    #[test]
    fn javascript_and_typescript_parse_template_and_type_nodes() {
        let javascript_source = "function greet(name) { return `hello ${name}`; }\n";
        let javascript = tokens("javascript", javascript_source);
        assert!(
            javascript[0]
                .iter()
                .any(|token| token.kind == TokenKind::Function)
        );
        let interpolation_name = javascript_source
            [..javascript_source.find("${name}").unwrap() + 2]
            .chars()
            .count();
        assert!(!javascript[0].iter().any(|token| {
            token.kind == TokenKind::String
                && token.start <= interpolation_name
                && interpolation_name < token.end
        }));

        let typescript = tokens(
            "typescript",
            "interface User { name: string }\nfunction load(): User { return { name: \"A\" }; }\n",
        );
        assert!(
            typescript[0]
                .iter()
                .any(|token| token.kind == TokenKind::Type)
        );
        assert!(
            typescript[1]
                .iter()
                .any(|token| token.kind == TokenKind::Function)
        );
        let tsx = tokens("typescript", "const App = () => <div />;\n");
        assert!(tsx[0].iter().any(|token| token.kind == TokenKind::Keyword));
    }

    #[test]
    fn incremental_edit_matches_fresh_parse_and_handles_unicode() {
        let language = crate::find("rust").unwrap();
        for (old, new) in [
            (
                "fn hi() { let x = \"é\"; }\n",
                "fn hi() { let x = \"你好\"; }\n",
            ),
            (
                "fn first() {}\r\nfn second() {}\r\n",
                "fn first() {\r\n}\r\nfn second() {}\r\n",
            ),
            ("fn first() {}\nfn second() {}\n", "fn second() {}\n"),
            ("fn empty() {}\n", ""),
        ] {
            let previous = TreeTokens::update(language, old, None).unwrap();
            let incremental = TreeTokens::update(language, new, Some(previous)).unwrap();
            let fresh = TreeTokens::update(language, new, None).unwrap();
            assert_eq!(incremental.lines, fresh.lines, "{old:?} -> {new:?}");
        }
    }

    #[test]
    fn overlay_splits_existing_tokens_at_tree_boundaries() {
        let mut tokens = vec![Token {
            start: 0,
            end: 8,
            kind: TokenKind::Text,
        }];
        overlay_tree_tokens(
            &mut tokens,
            &[Token {
                start: 2,
                end: 5,
                kind: TokenKind::Function,
            }],
            8,
        );
        assert_eq!(
            tokens,
            [
                Token {
                    start: 0,
                    end: 2,
                    kind: TokenKind::Text,
                },
                Token {
                    start: 2,
                    end: 5,
                    kind: TokenKind::Function,
                },
                Token {
                    start: 5,
                    end: 8,
                    kind: TokenKind::Text,
                },
            ]
        );
    }

    #[test]
    fn tree_string_overlay_preserves_existing_escape_classification() {
        let mut tokens = vec![
            Token {
                start: 0,
                end: 3,
                kind: TokenKind::String,
            },
            Token {
                start: 3,
                end: 5,
                kind: TokenKind::Escape,
            },
            Token {
                start: 5,
                end: 8,
                kind: TokenKind::String,
            },
        ];
        overlay_tree_tokens(
            &mut tokens,
            &[Token {
                start: 0,
                end: 8,
                kind: TokenKind::String,
            }],
            8,
        );
        assert!(tokens.contains(&Token {
            start: 3,
            end: 5,
            kind: TokenKind::Escape,
        }));
        assert!(
            !tokens.iter().any(|token| {
                token.kind == TokenKind::String && token.start < 5 && token.end > 3
            })
        );
    }
}
