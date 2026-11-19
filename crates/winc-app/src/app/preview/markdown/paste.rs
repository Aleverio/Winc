//! Markdown 中粘贴时改写成对应的写法：网址、图片网址、表格软件的单元格、网页和文档里复制的
//! 内容（HTML）；「粘贴为纯文本」不改写。

use super::*;

/// 粘贴处：光标所在行在光标之前、之后的文字，上一行是否为空（或没有上一行）。
struct Spot {
    before: String,
    after: String,
    blank_above: bool,
}

impl Spot {
    /// 把几块内容 `text` 放在这里：前后都和别的文字隔一个空行。光标停在内容末尾，同一行
    /// 后面还有文字时才另起一段。
    fn block(&self, text: &str) -> String {
        let lead = if !self.before.trim().is_empty() {
            "\n\n"
        } else if !self.blank_above {
            "\n"
        } else {
            ""
        };
        let text = text.trim_end_matches('\n');
        let tail = if self.after.trim().is_empty() {
            ""
        } else {
            "\n\n"
        };
        format!("{lead}{text}{tail}")
    }
}

impl App {
    /// Markdown 中粘贴 `content` 时改写成的文字；按普通粘贴处理时为 `None`。
    ///
    /// - 选中一行之内的文字后粘贴网址：`[文字](网址)`；选中的文字本身是网址时视为替换网址。
    /// - 没有选区时粘贴图片的网址：`![](网址)`。
    /// - 粘贴表格软件复制的单元格（见 `cells::from_tsv`）：GFM 表格，单独成块。
    pub(in crate::app) fn markdown_paste(&self, content: &str) -> Option<String> {
        let doc = self.editor.active()?;
        let is_url = super::writing::is_url;
        let url = content.trim();
        let rope = doc.text().rope()?;
        let chars = rope.len_chars();
        let (from, to) = (
            doc.selection.from().min(chars),
            doc.selection.to().min(chars),
        );
        let selected = rope.slice(from..to).to_string();
        let table = winc_preview::markdown::cells::from_tsv(content).filter(|_| from == to);
        let link = is_url(url)
            && !selected.is_empty()
            && !selected.contains(['\n', '`'])
            && !is_url(selected.trim());
        let image = is_url(url) && selected.is_empty() && super::writing::is_image_url(url);
        if table.is_none() && !link && !image {
            return None;
        }
        let spot = self.paste_spot()?;
        if let Some(table) = table {
            return Some(spot.block(&table));
        }
        // 网址含括号时用尖括号包裹，避免括号被视为链接的结尾。
        let url = if url.contains(['(', ')']) {
            format!("<{url}>")
        } else {
            url.to_owned()
        };
        Some(match image {
            true => format!("![]({url})"),
            false => format!("[{selected}]({url})"),
        })
    }

    /// Markdown 中粘贴剪贴板里的 HTML（网页、在线文档里复制的内容）时写成的 Markdown；HTML
    /// 里只有文字、没有结构时为 `None`，按纯文本粘贴。
    ///
    /// 只有一段文字时就地插入；有几块时单独成块。
    pub(in crate::app) fn markdown_html_paste(&mut self) -> Option<String> {
        let spot = self.paste_spot()?;
        let html = self.clipboard.html()?;
        let converted = winc_preview::markdown::from_html::from_html(&html);
        if !converted.rich {
            return None;
        }
        let body = converted.text.trim_end();
        let starts_block = ["#", "- ", "> ", "|", "```", "---", "!["]
            .iter()
            .any(|p| body.starts_with(p))
            || body
                .split_once(". ")
                .is_some_and(|(n, _)| n.chars().all(|c| c.is_ascii_digit()));
        match !body.contains('\n') && !starts_block {
            true => Some(body.to_owned()),
            false => Some(spot.block(&converted.text)),
        }
    }

    /// 粘贴为纯文本：剪贴板里的文字原样插入，不改写成 Markdown 的写法。
    pub(in crate::app) fn paste_plain(&mut self) {
        if let Some(text) = self.clipboard_text() {
            self.run(winc_editor::Command::Paste(text));
        }
    }

    /// 光标处能否把粘贴的内容改写成 Markdown 的写法：是 Markdown 文档、只有一个选区、光标在
    /// 正文中（不在代码块、公式、HTML 块或行内代码里，其中的写法不生效）。
    fn paste_spot(&self) -> Option<Spot> {
        let doc = self.editor.active()?;
        if doc.preview_kind() != Some(PreviewKind::Markdown) || !doc.extra_selections().is_empty() {
            return None;
        }
        let rope = doc.text().rope()?;
        let from = doc.selection.from().min(rope.len_chars());
        let (from, line) = (rope.char_to_byte(from), rope.char_to_line(from));
        let start = rope.line_to_byte(line);
        let before = rope.byte_slice(start..from).to_string();
        // 位于行内代码中：该行光标之前的反引号数量为奇数。
        if before.matches('`').count() % 2 == 1 {
            return None;
        }
        // 判断是否位于代码块、公式、HTML 块中时使用当前版本的解析结果；没有该结果时
        // （未显示预览或尚未解析到当前版本）只检查围栏代码块，统计之前的围栏行。
        if self.preview.markdown.checked == Some(doc.revision)
            && let Some(parsed) = self.preview.markdown.parsed.as_ref()
        {
            let i = parsed.leaves.partition_point(|l| l.range.start <= from);
            if let Some(leaf) = i.checked_sub(1).map(|i| &parsed.leaves[i])
                && leaf.range.contains(&from)
                && !leaf.prose
            {
                return None;
            }
        } else if in_fence(rope.lines_at(0).take(line)) {
            return None;
        }
        let to = rope.char_to_byte(doc.selection.to().min(rope.len_chars()));
        let end = rope.line_to_byte((line + 1).min(rope.len_lines()));
        let after = rope.byte_slice(to.max(start).min(end)..end).to_string();
        let blank_above = line == 0 || rope.line(line - 1).to_string().trim().is_empty();
        Some(Spot {
            before,
            after,
            blank_above,
        })
    }
}
