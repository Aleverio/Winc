//! 荧光笔文字：把行内文字中的 `==文字==` 换成带底色的文字。

use std::ops::Range;

use super::{Inline, Run, Style};

/// 把 `==文字==` 换成带荧光笔底色（[`Style::MARK`]）的文字，去掉两侧的 `==`。
///
/// 规则与删除线相同：开头的 `==` 之后、收尾的 `==` 之前不能是空白，连写三个以上的 `=`
/// 不算；行内代码和公式中的不算。里面可以有粗体、链接等其他样式。去掉的标记在原文对照中
/// 留下空隙，可编辑的预览据此显示标记。
pub(super) fn highlight(inline: &mut Inline) {
    if !inline.text.contains("==") {
        return;
    }
    // 各处恰好两个 `=` 的位置，以及它能否作开头、收尾。
    let bytes = inline.text.as_bytes();
    let literal = |at: usize| {
        inline
            .runs()
            .find(|(range, _)| range.contains(&at))
            .is_some_and(|(_, style)| style.has(Style::CODE) || style.has(Style::MATH))
    };
    let space = |b: Option<&u8>| b.is_none_or(|b| b.is_ascii_whitespace());
    let mut marks = Vec::new();
    let mut at = 0;
    while let Some(found) = inline.text[at..].find('=') {
        let start = at + found;
        let end = start + bytes[start..].iter().take_while(|&&b| b == b'=').count();
        if end - start == 2 && !literal(start) {
            let opens = !space(bytes.get(end));
            let closes = start > 0 && !space(bytes.get(start - 1));
            marks.push((start, opens, closes));
        }
        at = end;
    }
    // 从前往后配对：一个开头配它之后最近的收尾，中间至少有一个字符。
    let mut pairs = Vec::new();
    let mut i = 0;
    while i < marks.len() {
        let (open, opens, _) = marks[i];
        let close = (opens.then(|| {
            marks[i + 1..]
                .iter()
                .position(|&(at, _, closes)| closes && at > open + 2)
        }))
        .flatten();
        match close {
            Some(k) => {
                pairs.push((open, marks[i + 1 + k].0));
                i += k + 2;
            }
            None => i += 1,
        }
    }
    // 从后往前改：前面的位置不受后面的删除影响。
    for (open, close) in pairs.into_iter().rev() {
        restyle(inline, open + 2..close, Style::MARK);
        cut(inline, close..close + 2);
        cut(inline, open..open + 2);
    }
}

/// 为文字中 `range` 内的各段样式加上标志位 `flags`。
fn restyle(inline: &mut Inline, range: Range<usize>, flags: u16) {
    let mut runs: Vec<Run> = Vec::with_capacity(inline.runs.len() + 2);
    let mut push = |end: usize, style: Style| match runs.last_mut() {
        Some(last) if last.style == style => last.end = end,
        _ => runs.push(Run { end, style }),
    };
    for (piece, style) in inline.runs() {
        let (from, to) = (range.start.max(piece.start), range.end.min(piece.end));
        if from >= to {
            push(piece.end, style);
            continue;
        }
        if piece.start < from {
            push(from, style);
        }
        let mut marked = style;
        marked.flags |= flags;
        push(to, marked);
        if to < piece.end {
            push(piece.end, style);
        }
    }
    inline.runs = runs;
}

/// 去掉文字中 `range` 内的字节，样式段和原文对照随之前移。
fn cut(inline: &mut Inline, range: Range<usize>) {
    let len = range.len();
    inline.text.replace_range(range.clone(), "");
    inline.map.replace(range.clone(), 0);
    let mut runs: Vec<Run> = Vec::with_capacity(inline.runs.len());
    let mut start = 0;
    for run in &inline.runs {
        let end = match run.end {
            end if end <= range.start => end,
            end if end >= range.end => end - len,
            _ => range.start,
        };
        match runs.last_mut() {
            Some(last) if last.style == run.style => last.end = end,
            _ if end > start => runs.push(Run {
                end,
                style: run.style,
            }),
            _ => {}
        }
        start = end;
    }
    inline.runs = runs;
}
