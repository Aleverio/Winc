//! emoji 序列：由几个字符合成一个图形的写法，以及默认按 emoji 显示的字符。
//!
//! 序列包括带变体选择符的符号（`❤` 加 U+FE0F）、带肤色的手势、用零宽连接符连起来的组合、
//! 国旗（两个区域指示符）和键帽。绘制时整个序列交给 emoji 字体合成，不逐字符绘制。

/// 变体选择符 15：要求按文字显示。
pub(crate) const VS15: char = '\u{fe0e}';
/// 变体选择符 16：要求按 emoji 显示。
pub(crate) const VS16: char = '\u{fe0f}';
/// 零宽连接符：把前后的 emoji 连成一个图形。
pub(crate) const ZWJ: char = '\u{200d}';
/// 键帽：与前面的数字、`#`、`*` 合成按键的图形。
pub(crate) const KEYCAP: char = '\u{20e3}';

/// 默认按 emoji 显示的字符所在的码点范围（含两端），按码点从小到大排列。
///
/// 取自 Unicode 16 的 `Emoji_Presentation` 属性；U+1FA70 至 U+1FAFF 整段列入，其中尚未分配的
/// 码点在任何字体中都没有字形。
pub(crate) const EMOJI_PRESENTATION: &[(u32, u32)] = &[
    (0x231A, 0x231B),
    (0x23E9, 0x23EC),
    (0x23F0, 0x23F0),
    (0x23F3, 0x23F3),
    (0x25FD, 0x25FE),
    (0x2614, 0x2615),
    (0x2648, 0x2653),
    (0x267F, 0x267F),
    (0x2693, 0x2693),
    (0x26A1, 0x26A1),
    (0x26AA, 0x26AB),
    (0x26BD, 0x26BE),
    (0x26C4, 0x26C5),
    (0x26CE, 0x26CE),
    (0x26D4, 0x26D4),
    (0x26EA, 0x26EA),
    (0x26F2, 0x26F3),
    (0x26F5, 0x26F5),
    (0x26FA, 0x26FA),
    (0x26FD, 0x26FD),
    (0x2705, 0x2705),
    (0x270A, 0x270B),
    (0x2728, 0x2728),
    (0x274C, 0x274C),
    (0x274E, 0x274E),
    (0x2753, 0x2755),
    (0x2757, 0x2757),
    (0x2795, 0x2797),
    (0x27B0, 0x27B0),
    (0x27BF, 0x27BF),
    (0x2B1B, 0x2B1C),
    (0x2B50, 0x2B50),
    (0x2B55, 0x2B55),
    (0x1F004, 0x1F004),
    (0x1F0CF, 0x1F0CF),
    (0x1F18E, 0x1F18E),
    (0x1F191, 0x1F19A),
    (0x1F1E6, 0x1F1FF),
    (0x1F201, 0x1F201),
    (0x1F21A, 0x1F21A),
    (0x1F22F, 0x1F22F),
    (0x1F232, 0x1F236),
    (0x1F238, 0x1F23A),
    (0x1F250, 0x1F251),
    (0x1F300, 0x1F320),
    (0x1F32D, 0x1F335),
    (0x1F337, 0x1F37C),
    (0x1F37E, 0x1F393),
    (0x1F3A0, 0x1F3CA),
    (0x1F3CF, 0x1F3D3),
    (0x1F3E0, 0x1F3F0),
    (0x1F3F4, 0x1F3F4),
    (0x1F3F8, 0x1F43E),
    (0x1F440, 0x1F440),
    (0x1F442, 0x1F4FC),
    (0x1F4FF, 0x1F53D),
    (0x1F54B, 0x1F54E),
    (0x1F550, 0x1F567),
    (0x1F57A, 0x1F57A),
    (0x1F595, 0x1F596),
    (0x1F5A4, 0x1F5A4),
    (0x1F5FB, 0x1F64F),
    (0x1F680, 0x1F6C5),
    (0x1F6CC, 0x1F6CC),
    (0x1F6D0, 0x1F6D2),
    (0x1F6D5, 0x1F6D7),
    (0x1F6DC, 0x1F6DF),
    (0x1F6EB, 0x1F6EC),
    (0x1F6F4, 0x1F6FC),
    (0x1F7E0, 0x1F7EB),
    (0x1F7F0, 0x1F7F0),
    (0x1F90C, 0x1F93A),
    (0x1F93C, 0x1F945),
    (0x1F947, 0x1F9FF),
    (0x1FA70, 0x1FAFF),
];

/// 字符 `c` 是否默认按 emoji 显示（见 [`EMOJI_PRESENTATION`]）；`☺`、`✔` 这类默认按文字显示的
/// 符号不算。
pub(crate) fn emoji_presentation(c: char) -> bool {
    let code = c as u32;
    code >= EMOJI_PRESENTATION[0].0
        && EMOJI_PRESENTATION
            .binary_search_by(|&(start, end)| {
                if code < start {
                    std::cmp::Ordering::Greater
                } else if code > end {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .is_ok()
}

/// 是否是肤色修饰符（U+1F3FB 至 U+1F3FF）。
fn modifier(c: char) -> bool {
    matches!(c, '\u{1f3fb}'..='\u{1f3ff}')
}

/// 是否是区域指示符（U+1F1E6 至 U+1F1FF）：两个连写表示一面国旗。
fn regional(c: char) -> bool {
    matches!(c, '\u{1f1e6}'..='\u{1f1ff}')
}

/// 是否是标签字符（U+E0020 至 U+E007F）：跟在旗帜之后表示地区旗。
fn tag(c: char) -> bool {
    matches!(c, '\u{e0020}'..='\u{e007f}')
}

/// 字符 `c` 是否可以经零宽连接符接在 emoji 之后：默认按 emoji 显示的字符，以及符号区中按
/// 文字显示的符号（`♂`、`⚕`、`❤`）。
fn joinable(c: char) -> bool {
    emoji_presentation(c) || matches!(c, '\u{2190}'..='\u{2bff}' | '\u{1f000}'..='\u{1faff}')
}

/// 字符 `next` 紧跟在 `c` 之后时是否可能与它组成序列；用于在逐字符的循环中快速排除普通文字。
#[inline]
pub(crate) fn joins(c: char, next: char) -> bool {
    next >= ZWJ
        && (matches!(next, VS15 | VS16 | ZWJ | KEYCAP)
            || modifier(next)
            || tag(next)
            || (regional(c) && regional(next)))
}

/// 返回 `chars` 开头的 emoji 序列的字符数；开头不是由几个字符组成的序列时返回 1，`chars` 为空
/// 时返回 0。
///
/// 零宽连接符只连接 emoji：印度系文字中的连接符不算。
pub fn sequence_len<I>(chars: I) -> usize
where
    I: IntoIterator<Item = char>,
    I::IntoIter: Clone,
{
    let mut chars = chars.into_iter().peekable();
    let Some(base) = chars.next() else {
        return 0;
    };
    if regional(base) {
        return 1 + usize::from(chars.peek().copied().is_some_and(regional));
    }
    let mut emoji = emoji_presentation(base);
    let mut len = 1;
    while let Some(&next) = chars.peek() {
        match next {
            VS16 => emoji = true,
            VS15 => {}
            KEYCAP if matches!(base, '0'..='9' | '#' | '*') => {}
            c if emoji && (modifier(c) || tag(c)) => {}
            ZWJ if emoji => {
                let mut ahead = chars.clone();
                ahead.next();
                if !ahead.next().is_some_and(joinable) {
                    break;
                }
                // 连接符与其后的字符一并计入。
                chars.next();
                len += 1;
            }
            _ => break,
        }
        chars.next();
        len += 1;
    }
    len
}

/// 字符 `c` 紧跟在 `previous` 之后时是否可能属于同一个 emoji 序列：`c` 是变体选择符、肤色
/// 修饰符这类只出现在序列中的字符，或 `previous` 是零宽连接符。
///
/// 逐字符排版的调用方用它排除普通文字，为真时再用 [`sequence_len`] 确认。
pub fn continues_sequence(previous: char, c: char) -> bool {
    previous == ZWJ || joins(previous, c)
}

/// 序列 `unit`（见 [`sequence_len`]）是否按 emoji 显示，由 emoji 字体合成。
///
/// 带变体选择符 15 的按文字显示；ASCII 字符只有组成键帽时才是 emoji。
pub(crate) fn presents_as_emoji(unit: &str) -> bool {
    let base = unit.chars().next().unwrap_or(' ');
    !unit.contains(VS15) && (!base.is_ascii() || unit.contains(KEYCAP))
}

/// 序列中不单独绘制的字符：变体选择符、零宽连接符和标签字符。
pub(crate) fn ignorable(c: char) -> bool {
    matches!(c, VS15 | VS16 | ZWJ) || tag(c)
}

/// 把一段文字切成绘制的单位：单个字符，或一个 emoji 序列。
pub(crate) enum Unit<'a> {
    /// 单个字符。
    Char(char),
    /// 由几个字符组成的 emoji 序列。
    Sequence(&'a str),
}

/// 返回 `text` 的各个绘制单位。
pub(crate) fn units(text: &str) -> impl Iterator<Item = Unit<'_>> {
    let mut chars = text.char_indices().peekable();
    std::iter::from_fn(move || {
        let (start, c) = chars.next()?;
        if !chars.peek().is_some_and(|&(_, next)| joins(c, next)) {
            return Some(Unit::Char(c));
        }
        let len = sequence_len(text[start..].chars());
        let mut end = start + c.len_utf8();
        for _ in 1..len {
            end += chars.next().map_or(0, |(_, c)| c.len_utf8());
        }
        Some(match len {
            1 => Unit::Char(c),
            _ => Unit::Sequence(&text[start..end]),
        })
    })
}

#[cfg(test)]
mod tests;
