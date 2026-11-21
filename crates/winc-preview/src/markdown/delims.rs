//! LaTeX 的公式定界符：`\(…\)` 是行内公式，`\[…\]` 和 `\begin{equation}…\end{equation}` 这类
//! 环境是单独成行的公式。
//!
//! pulldown-cmark 只认 `$…$`、`$$…$$`，`\(` 在 Markdown 里是转义的括号。解析前把定界符原地
//! 换成等长的 `$$`（环境的名字换成 `x`），字节位置不变，原文对照照常；行内代码、代码块、HTML
//! 和链接里的不换。解析出的公式按原文的定界符还原（[`restore`]）。

use std::borrow::Cow;
use std::ops::Range;

use pulldown_cmark::{Event, Parser, Tag};

/// 环境名最多几个字（`flalign*` 是 8 个）。
const MAX_NAME: usize = 16;

/// 写在正文里就是单独成行的公式的环境（可带 `*`）。
const ENVIRONMENTS: [&str; 7] = [
    "equation", "align", "alignat", "gather", "multline", "flalign", "eqnarray",
];

/// 返回把 LaTeX 定界符换成 `$$` 之后的 `text`；没有可换的定界符时原样借用。
pub(super) fn mask(text: &str) -> Cow<'_, str> {
    if !(text.contains("\\(") || text.contains("\\[") || text.contains("\\begin{")) {
        return Cow::Borrowed(text);
    }
    let skip = verbatim(text);
    let bytes = text.as_bytes();
    let mut out: Option<Vec<u8>> = None;
    let mut i = 0;
    while let Some(at) = bytes[i..].iter().position(|&b| b == b'\\').map(|at| i + at) {
        let found = (!escaped(bytes, at) && !inside(&skip, at))
            .then(|| span(text, at, &skip))
            .flatten();
        let Some((open, close)) = found else {
            i = at + 1;
            continue;
        };
        let out = out.get_or_insert_with(|| bytes.to_vec());
        out[open.clone()].fill(b'x');
        out[close.clone()].fill(b'x');
        out[open.start..open.start + 2].copy_from_slice(b"$$");
        out[close.end - 2..close.end].copy_from_slice(b"$$");
        i = close.end;
    }
    match out {
        // 只把 ASCII 换成 ASCII，仍是合法的 UTF-8。
        Some(out) => Cow::Owned(String::from_utf8(out).unwrap_or_else(|_| text.to_owned())),
        None => Cow::Borrowed(text),
    }
}

/// 还原 [`mask`] 换掉的公式：`source` 是公式在原文中的一段（含定界符），`text` 是解析出的
/// 内容。返回是否为行内公式（`\(…\)`）和公式的 LaTeX 原文；环境的公式带着 `\begin`、`\end`。
pub(super) fn restore<'a>(source: &str, text: &'a str) -> (bool, Cow<'a, str>) {
    if source.starts_with("\\(") {
        return (true, Cow::Borrowed(text));
    }
    let (Some(open), Some(close)) = (
        source
            .strip_prefix("\\begin{")
            .and_then(|_| source.find('}')),
        source.rfind("\\end{"),
    ) else {
        return (false, Cow::Borrowed(text));
    };
    // 解析出的内容去掉了两头的 `$$`，各剩下名字换成的 `x`。
    let (head, tail) = (open - 1, source.len() - close - 2);
    if head + tail > text.len() {
        return (false, Cow::Borrowed(text));
    }
    match text.get(head..text.len() - tail) {
        Some(body) => {
            let latex = format!("{}{body}{}", &source[..=open], &source[close..]);
            (false, Cow::Owned(latex))
        }
        None => (false, Cow::Borrowed(text)),
    }
}

/// 从 `at` 处的 `\` 开始的一个公式：开头和收尾定界符的范围。收尾在同一段里，内容不能为空。
fn span(text: &str, at: usize, skip: &[Range<usize>]) -> Option<(Range<usize>, Range<usize>)> {
    let rest = &text[at..];
    let (open, close) = if rest.starts_with("\\(") {
        (2, "\\)".to_owned())
    } else if rest.starts_with("\\[") {
        (2, "\\]".to_owned())
    } else {
        // 环境名很短：只在开头几个字里找 `}`，不为没写完的 `\begin{` 扫到文末。
        let name = rest.strip_prefix("\\begin{")?;
        let len = name
            .char_indices()
            .take(MAX_NAME)
            .find(|&(_, c)| c == '}')?
            .0;
        let name = &name[..len];
        if !ENVIRONMENTS.contains(&name.strip_suffix('*').unwrap_or(name)) {
            return None;
        }
        (
            "\\begin{".len() + name.len() + 1,
            format!("\\end{{{name}}}"),
        )
    };
    let start = at + open;
    // 公式不跨过空行：收尾只在这一段里找。
    let end = paragraph_end(text, start);
    let mut from = start;
    loop {
        let found = from + text[from..end].find(close.as_str())?;
        if escaped(text.as_bytes(), found) || inside(skip, found) {
            from = found + 1;
            continue;
        }
        let inner = &text[start..found];
        return (!inner.trim().is_empty()).then_some((at..start, found..found + close.len()));
    }
}

/// 从 `from` 起这一段结束的位置：下一个只有空白的行的开头；没有空行时是原文末尾。
fn paragraph_end(text: &str, from: usize) -> usize {
    let mut at = from;
    while let Some(nl) = text[at..].find('\n') {
        let line = at + nl + 1;
        let line_end = text[line..].find('\n').map_or(text.len(), |i| line + i);
        if text[line..line_end].trim().is_empty() {
            return line;
        }
        at = line;
    }
    text.len()
}

/// 原文中不换定界符的范围，按起点排列、互不重叠：行内代码、代码块、HTML、链接，以及已经用
/// `$` 写的公式。
fn verbatim(text: &str) -> Vec<Range<usize>> {
    let mut skip: Vec<Range<usize>> = Vec::new();
    for (event, range) in Parser::new_ext(text, super::options()).into_offset_iter() {
        let whole = matches!(
            event,
            Event::Start(Tag::CodeBlock(_) | Tag::HtmlBlock | Tag::Link { .. } | Tag::Image { .. })
                | Event::Code(_)
                | Event::Html(_)
                | Event::InlineHtml(_)
                | Event::InlineMath(_)
                | Event::DisplayMath(_)
        );
        // 已记下的范围里面的（代码块里的文字、链接里的行内代码）不再记。
        let nested = skip.last().is_some_and(|last| last.end >= range.end);
        if whole && !nested {
            skip.push(range);
        }
    }
    skip
}

/// `at` 是否落在 `skip`（按起点排列、互不重叠）的某个范围里。
fn inside(skip: &[Range<usize>], at: usize) -> bool {
    let k = skip.partition_point(|r| r.start <= at);
    k > 0 && skip[k - 1].contains(&at)
}

/// `at` 处的 `\` 是否被前面的 `\` 转义：前面紧挨着奇数个 `\`。
fn escaped(bytes: &[u8], at: usize) -> bool {
    bytes[..at]
        .iter()
        .rev()
        .take_while(|&&b| b == b'\\')
        .count()
        % 2
        == 1
}

#[cfg(test)]
mod tests;
