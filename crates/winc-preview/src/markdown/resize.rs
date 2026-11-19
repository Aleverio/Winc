//! 调整图片大小时改写图片的原文：`![alt](src)` 改成带宽度的 `<img>`，`<img>` 改写宽度，
//! 去掉写明的大小时尽量改回 `![alt](src)`。

use std::ops::Range;

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use crate::convert::html_attr;

/// 写进 `<img>` 的宽度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width {
    /// 像素（逻辑像素，同 CSS 像素）。
    Px(u32),
    /// 占一行宽度的百分比。
    Percent(u32),
}

/// 把原文 `text` 中第 `nth` 张图片（从 0 起）改成宽度 `width` 的写法；`width` 为 `None` 时
/// 去掉写明的大小。
///
/// `text` 从图片原文的开头起（`![` 或 `<img`；HTML 块里的图片从块的开头起，`nth` 数块里的
/// 第几个 `<img>`），到所在顶层块的末尾。返回要替换的字节范围（相对 `text`）和新的文字；
/// 认不出、或不用改时返回 `None`。
pub fn resize(text: &str, nth: usize, width: Option<Width>) -> Option<(Range<usize>, String)> {
    if text.starts_with("![") {
        return (nth == 0).then(|| markdown(text, width)).flatten();
    }
    let at = img_tags(text).nth(nth)?;
    html(text, at, width)
}

/// `![alt](src "title")`：写宽度时改成 `<img>`；去掉大小时只去掉替代文字末尾的 `|W`。
fn markdown(text: &str, width: Option<Width>) -> Option<(Range<usize>, String)> {
    let (mut alt, mut range, mut found) = (String::new(), 0..0, None);
    for (event, at) in Parser::new_ext(text, super::options()).into_offset_iter() {
        match event {
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) if at.start == 0 => {
                range = at;
                found = Some((dest_url.into_string(), title.into_string()));
            }
            Event::Text(t) | Event::Code(t) if found.is_some() => alt.push_str(&t),
            Event::End(TagEnd::Image) if found.is_some() => break,
            _ if found.is_none() && at.start > 0 => return None,
            _ => {}
        }
    }
    let (src, title) = found?;
    let raw_alt = alt_source(&text[range.clone()])?;
    let width = match width {
        Some(width) => width,
        None => {
            let (kept, ..) = split_alt_size(&text[raw_alt.clone()])?;
            return Some((raw_alt.start..raw_alt.end, kept.to_owned()));
        }
    };
    let alt = split_alt_size(&alt).map_or(alt.as_str(), |(alt, ..)| alt);
    let mut out = format!(
        "<img src=\"{}\" alt=\"{}\"",
        html_attr(&src),
        html_attr(alt)
    );
    if !title.is_empty() {
        out.push_str(&format!(" title=\"{}\"", html_attr(&title)));
    }
    out.push_str(&format!(" width=\"{}\">", value(width)));
    Some((range, out))
}

/// `![alt](…)` 中替代文字的原文范围：`![` 之后到与之配对的 `]` 之前。
fn alt_source(image: &str) -> Option<Range<usize>> {
    let mut depth = 0usize;
    let mut chars = image.char_indices().skip(2);
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '[' => depth += 1,
            ']' if depth == 0 => return Some(2..i),
            ']' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// 依次返回 `text` 中各个 `<img …>` 标签的字节范围（含 `<` 和 `>`）。
fn img_tags(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    let lower = text.to_ascii_lowercase();
    let mut from = 0;
    std::iter::from_fn(move || {
        loop {
            let at = from + lower[from..].find("<img")?;
            from = at + 4;
            if !lower[from..].starts_with(|c: char| c.is_whitespace() || c == '/' || c == '>') {
                continue;
            }
            let end = tag_end(text, from)?;
            from = end;
            return Some(at..end);
        }
    })
}

/// 从 `from` 起找标签的结尾：引号外的第一个 `>` 之后。
fn tag_end(text: &str, from: usize) -> Option<usize> {
    let mut quote = None;
    for (i, c) in text[from..].char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '>') => return Some(from + i + 1),
            _ => {}
        }
    }
    None
}

/// 改写 `<img>` 标签 `text[tag]`：去掉 `width`、`height` 属性和 `style` 里的大小，写宽度时在
/// 末尾加上 `width`。去掉大小后只剩 `src`、`alt`、`title`，且标签就在原文开头时改回
/// `![alt](src)`。
fn html(text: &str, tag: Range<usize>, width: Option<Width>) -> Option<(Range<usize>, String)> {
    let raw = &text[tag.clone()];
    let inner_end = raw.len() - 1 - usize::from(raw[..raw.len() - 1].trim_end().ends_with('/'));
    let inner = &raw[4..inner_end];
    let close = &raw[inner_end..];
    let mut kept = String::new();
    let (mut src, mut alt, mut title, mut plain) = (None, String::new(), None, true);
    for attr in attributes(inner) {
        let name = attr.name.to_ascii_lowercase();
        match name.as_str() {
            "width" | "height" => continue,
            "style" => {
                let style = without_size(attr.value.as_deref().unwrap_or_default());
                if !style.is_empty() {
                    kept.push_str(&format!(" style=\"{}\"", html_attr(&style)));
                    plain = false;
                }
                continue;
            }
            "src" => src = attr.value.clone(),
            "alt" => alt = attr.value.clone().unwrap_or_default(),
            "title" => title = attr.value.clone(),
            _ => plain = false,
        }
        kept.push(' ');
        kept.push_str(attr.raw.trim());
    }
    let new = match width {
        Some(width) => format!(
            "<img{kept} width=\"{}\"{}",
            value(width),
            close.trim_start()
        ),
        None if plain && tag.start == 0 => {
            let src = super::html::decode_entities(&src?);
            let alt = super::html::decode_entities(&alt);
            let title = title.map(|t| super::html::decode_entities(&t));
            image_markdown(&alt, &src, title.as_deref())
        }
        None => format!("<img{kept}{}", close.trim_start()),
    };
    (new != raw).then_some((tag, new))
}

/// 一个 HTML 属性：原文、名字和值（已去掉引号，未解码实体）。
struct Attribute<'a> {
    raw: &'a str,
    name: &'a str,
    value: Option<String>,
}

/// 依次读出标签内部 `inner`（`<img` 之后、`>` 之前）的各个属性。
fn attributes(inner: &str) -> Vec<Attribute<'_>> {
    let bytes = inner.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let name_start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && !b"=/>".contains(&bytes[i]) {
            i += 1;
        }
        let name = &inner[name_start..i];
        let mut j = i;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        let mut value = None;
        if bytes.get(j) == Some(&b'=') {
            j += 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let (v, end) = match bytes.get(j) {
                Some(&q @ (b'"' | b'\'')) => {
                    let end = inner[j + 1..]
                        .find(q as char)
                        .map_or(inner.len(), |k| j + 1 + k);
                    (&inner[j + 1..end], (end + 1).min(inner.len()))
                }
                _ => {
                    let end = inner[j..]
                        .find(|c: char| c.is_whitespace())
                        .map_or(inner.len(), |k| j + k);
                    (&inner[j..end], end)
                }
            };
            value = Some(v.to_owned());
            i = end;
        }
        if name.is_empty() {
            // 认不出的字符（如单独的 `/`）跳过。
            i = i.max(start + 1);
            continue;
        }
        out.push(Attribute {
            raw: &inner[start..i],
            name,
            value,
        });
    }
    out
}

/// 去掉 CSS 声明 `style` 中的 `width`、`height`、`zoom`，其余原样保留。
fn without_size(style: &str) -> String {
    let kept: Vec<&str> = style
        .split(';')
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .filter(|d| {
            let name = d
                .split(':')
                .next()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase();
            !matches!(name.as_str(), "width" | "height" | "zoom")
        })
        .collect();
    kept.join("; ")
}

fn value(width: Width) -> String {
    match width {
        Width::Px(px) => px.to_string(),
        Width::Percent(p) => format!("{p}%"),
    }
}

/// 写成 `![alt](src "title")`：`alt` 中的方括号、反斜杠转义；地址含空白或括号时用 `<…>` 括起来。
fn image_markdown(alt: &str, src: &str, title: Option<&str>) -> String {
    let mut out = String::from("![");
    for c in alt.chars() {
        if matches!(c, '[' | ']' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push_str("](");
    if src.contains(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '<' | '>')) {
        out.push('<');
        out.push_str(&crate::convert::link_target(src));
        out.push('>');
    } else {
        out.push_str(src);
    }
    if let Some(title) = title {
        out.push_str(&format!(" \"{}\"", title.replace('"', "\\\"")));
    }
    out.push(')');
    out
}

/// 替代文字末尾写了尺寸时（`![说明|300](a.png)`、`![说明|300x200](a.png)`）取出尺寸，替代文字
/// 去掉这一段。
pub(super) fn alt_size(image: &mut super::Image) {
    if let Some((alt, w, h)) = split_alt_size(&image.alt) {
        image.width = Some(super::Length::Px(w));
        image.height = h;
        image.alt = alt.to_owned();
    }
}

/// 把替代文字末尾的 `|W` 或 `|WxH` 分出来：返回其前面的文字（去掉末尾空白）、宽和高；没有
/// 这样写或宽为 0 时返回 `None`。
pub(super) fn split_alt_size(alt: &str) -> Option<(&str, u32, Option<u32>)> {
    let (alt, size) = alt.rsplit_once('|')?;
    let (w, h) = match size.split_once('x') {
        Some((w, h)) => (w, Some(h)),
        None => (size, None),
    };
    let w = w.trim().parse::<u32>().ok().filter(|&w| w > 0)?;
    let h = match h.map(|h| h.trim().parse::<u32>()) {
        Some(Ok(h)) => Some(h),
        Some(Err(_)) => return None,
        None => None,
    };
    Some((alt.trim_end(), w, h))
}

#[cfg(test)]
mod tests;
