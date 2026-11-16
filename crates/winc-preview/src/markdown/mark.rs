//! Markdown 的高亮语法（`==文字==`）：把成对标记之间的文字标上 [`Style::MARK`]，标记本身
//! 不进文字。
//!
//! pulldown-cmark 不认识 `==`，标记原样留在行内文字里；这里在文字排好之后去掉它们，样式段
//! 和原文对照随之修剪，光标、选区和删除仍然作用于原文。可编辑的预览中，光标触及一段高亮时
//! 显示两个标记（见 [`super::reveal`]）。

use std::ops::Range;

use super::{Inline, Run, SourceMap, Style};

/// 一对高亮标记：开头和收尾的 `==` 在文字中的范围。
pub(super) type Pair = [Range<usize>; 2];

/// 返回 `text` 中 `==高亮==` 的一对对标记，按先后排列。
///
/// 标记必须单独成对（前后不是 `=`），中间的文字不为空，且两端没有空白。`inside` 判断位置
/// 是否在行内代码或公式里：那里的 `=` 是文字。
pub(super) fn pairs(text: &str, inside: impl Fn(usize) -> bool) -> Vec<Pair> {
    let bytes = text.as_bytes();
    let mut marks = Vec::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'=' && bytes[i + 1] == b'=' && !inside(i) {
            let alone = (i == 0 || bytes[i - 1] != b'=') && bytes.get(i + 2) != Some(&b'=');
            if alone {
                marks.push(i);
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    let mut out = Vec::new();
    let mut k = 0;
    while k + 1 < marks.len() {
        let (open, close) = (marks[k], marks[k + 1]);
        let content = &text[open + 2..close];
        if !content.is_empty()
            && !content.starts_with(char::is_whitespace)
            && !content.ends_with(char::is_whitespace)
        {
            out.push([open..open + 2, close..close + 2]);
            k += 2;
        } else {
            k += 1;
        }
    }
    out
}

/// 把 `inline` 里的 `==高亮==` 标成 [`Style::MARK`]，并去掉两个标记。
///
/// `source` 是原文；`inline.map` 的位置相对于它。原文里写作 `\==` 的是转义的文字，不当作
/// 标记（`\=` 解析之后文字里是 `=`，只看文字分不出来）。
pub(super) fn apply(inline: &mut Inline, source: &str) {
    if !inline.text.contains('=') {
        return;
    }
    let styles: Vec<(Range<usize>, Style)> = inline.runs().collect();
    let style_at = |at: usize| {
        styles
            .iter()
            .find(|(range, _)| range.contains(&at))
            .map_or(Style::default(), |&(_, style)| style)
    };
    let found = pairs(&inline.text, |at| {
        let style = style_at(at);
        style.has(Style::CODE) || style.has(Style::MATH)
    });
    // 转义的标记留在文字里，当作普通文字。
    let found: Vec<Pair> = found
        .into_iter()
        .filter(|pair| {
            pair.iter()
                .all(|mark| in_source(inline, source, mark.start))
        })
        .collect();
    if found.is_empty() {
        return;
    }

    // 去掉的标记和保留的文字段。
    let removed: Vec<Range<usize>> = found.iter().flatten().cloned().collect();
    let mut keep = Vec::with_capacity(removed.len() + 1);
    let mut at = 0;
    for range in &removed {
        if at < range.start {
            keep.push(at..range.start);
        }
        at = range.end;
    }
    if at < inline.text.len() {
        keep.push(at..inline.text.len());
    }

    // 文字：去掉标记，保留的段按原顺序接起来。
    let old = std::mem::take(&mut inline.text);
    let mut text = String::with_capacity(old.len());
    for range in &keep {
        text.push_str(&old[range.clone()]);
    }

    // 样式段：按保留的段切开，落在标记之间的一段加上高亮。
    let mut runs: Vec<Run> = Vec::with_capacity(styles.len() + found.len());
    let mut end = 0;
    for piece in &keep {
        let mut start = piece.start;
        for (range, style) in &styles {
            let (from, to) = (start.max(range.start), piece.end.min(range.end));
            if from >= to {
                continue;
            }
            let mut style = *style;
            if found
                .iter()
                .any(|pair| pair[0].end <= from && to <= pair[1].start)
            {
                style.flags |= Style::MARK;
            }
            end += to - from;
            match runs.last_mut() {
                Some(last) if last.style == style => last.end = end,
                _ => runs.push(Run { end, style }),
            }
            start = to;
            if start >= piece.end {
                break;
            }
        }
    }

    // 原文对照：每个保留的段取它与各段的交集。
    let shift = |at: usize| {
        let cut: usize = removed
            .iter()
            .filter(|range| range.start < at)
            .map(|range| range.end.min(at) - range.start)
            .sum();
        at - cut
    };
    let mut map = SourceMap::default();
    for anchor in &inline.map.0 {
        let (from, to) = (
            anchor.text as usize,
            anchor.text as usize + anchor.len as usize,
        );
        if from == to {
            map.push(shift(from), anchor.source as usize, 0);
            continue;
        }
        for piece in &keep {
            let (start, end) = (from.max(piece.start), to.min(piece.end));
            if start >= end {
                continue;
            }
            map.push(
                shift(start),
                anchor.source as usize + (start - from),
                end - start,
            );
        }
    }

    inline.text = text;
    inline.runs = runs;
    inline.map = map;
}

/// `at` 处的 `==` 在原文中是否也是紧挨着的两个 `=`，且不是 `\==` 转义出来的。
fn in_source(inline: &Inline, source: &str, at: usize) -> bool {
    let Some(start) = inline.map.source(at) else {
        return false;
    };
    let bytes = source.as_bytes();
    bytes.get(start) == Some(&b'=')
        && bytes.get(start + 1) == Some(&b'=')
        && (start == 0 || bytes[start - 1] != b'\\')
}
