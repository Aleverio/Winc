use super::*;

/// 给标题生成锚点的规则：转成小写，空格换成 `-`，去掉其他标点（保留 `-`、`_`）。
#[allow(dead_code)]
pub fn slug(text: &str) -> String {
    text.chars()
        .filter_map(|c| {
            if c == ' ' {
                Some('-')
            } else if c.is_alphanumeric() || c == '-' || c == '_' {
                Some(c)
            } else {
                None
            }
        })
        .flat_map(char::to_lowercase)
        .collect()
}

/// 把只有 `[TOC]`（或 `[[TOC]]`，不分大小写）的段落换成目录：按文中标题的层级嵌套的列表，每一项
/// 链接到对应的标题（锚点见 [`slug`]）。`blocks` 是正文的顶层块，不含脚注。
///
/// 目录随标题生成，标题改变时所在块的内容随之改变；没有标题时保留原段落。
#[allow(dead_code)]
pub(super) fn contents(blocks: &mut [Block]) {
    let is_marker = |block: &Block| {
        matches!(block, Block::Paragraph(inline)
            if matches!(inline.text.trim().to_ascii_lowercase().as_str(), "[toc]" | "[[toc]]"))
    };
    if !blocks.iter().any(is_marker) {
        return;
    }
    let headings: Vec<(u8, String)> = blocks
        .iter()
        .filter_map(|block| match block {
            Block::Heading(level, inline) if !inline.text.trim().is_empty() => {
                Some((*level, inline.text.clone()))
            }
            _ => None,
        })
        .collect();
    if headings.is_empty() {
        return;
    }
    let list = Block::List(None, contents_items(&headings));
    for block in blocks.iter_mut().filter(|block| is_marker(block)) {
        *block = list.clone();
    }
}

/// 把按先后排列的（级别，标题）排成嵌套的列表项：级别比前一项高的标题排在前一项下面。
#[allow(dead_code)]
fn contents_items(headings: &[(u8, String)]) -> Vec<Item> {
    let mut items = Vec::new();
    let mut i = 0;
    while i < headings.len() {
        let (level, text) = &headings[i];
        // 级别更深的后续标题都是这一项的子项。
        let end = headings[i + 1..]
            .iter()
            .position(|(l, _)| l <= level)
            .map_or(headings.len(), |k| i + 1 + k);
        let mut inline = Inline::default();
        inline.links.push(format!("#{}", slug(text)));
        let link = u16::try_from(inline.links.len()).unwrap_or(u16::MAX);
        inline.push(
            text,
            Style {
                link,
                ..Style::default()
            },
        );
        let mut blocks = vec![Block::Paragraph(inline)];
        if end > i + 1 {
            blocks.push(Block::List(None, contents_items(&headings[i + 1..end])));
        }
        items.push(Item {
            task: None,
            task_source: 0,
            blocks,
            bare: false,
        });
        i = end;
    }
    items
}

/// 行内文字的对照再改为相对于该段文字最靠前的位置（见 [`Inline::source`]）。
#[allow(dead_code)]
pub(super) fn rebase(block: &mut Block, start: usize) {
    let inline = |inline: &mut Inline| {
        if let Some(first) = inline.map.first() {
            inline.source = first.saturating_sub(start);
            inline.map.rebase(first);
        } else {
            inline.source = inline.source.saturating_sub(start);
        }
    };
    let blocks = |blocks: &mut Vec<Block>| blocks.iter_mut().for_each(|b| rebase(b, start));
    match block {
        Block::Paragraph(i) | Block::Heading(_, i) => inline(i),
        Block::Code { map, .. } => map.rebase(start),
        Block::Quote(_, b) | Block::Footnote(_, b) => blocks(b),
        Block::List(_, items) => items.iter_mut().for_each(|item| {
            item.task_source = item.task_source.saturating_sub(start);
            blocks(&mut item.blocks);
        }),
        Block::Table(_, rows, _) => rows.iter_mut().flatten().for_each(inline),
        Block::Details {
            summary, blocks: b, ..
        } => {
            inline(summary);
            blocks(b);
        }
        Block::Math(_, source) | Block::Rule(Some(source)) => {
            *source = source.saturating_sub(start);
        }
        Block::Rule(None) => {}
    }
}
