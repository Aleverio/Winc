//! 剪贴板 HTML 转 Markdown 的测试：网页、在线文档常见的写法，和只带颜色的代码不算结构。

use super::*;

fn md(html: &str) -> String {
    from_html(html).text
}

#[test]
fn blocks_and_inline_styles_follow_the_elements() {
    let html = "<meta charset='utf-8'><!--StartFragment--><h2>安装</h2>\
        <p>先运行 <code>cargo build</code>，再看<a href=\"https://winc.si/docs\">文档</a>。</p>\
        <p><strong>注意</strong>：<em>不要</em> 用 <del>旧版</del>。<br>第二行</p>\
        <blockquote><p>引用一</p><p>引用二</p></blockquote><hr>\
        <img src=\"https://a.dev/x.png\" alt=\"图\"><!--EndFragment-->";
    let out = from_html(html);
    assert!(out.rich);
    assert_eq!(
        out.text,
        "## 安装\n\n先运行 `cargo build`，再看[文档](https://winc.si/docs)。\n\n\
         **注意**：*不要* 用 ~~旧版~~。\\\n第二行\n\n> 引用一\n>\n> 引用二\n\n---\n\n\
         ![图](https://a.dev/x.png)\n"
    );
}

#[test]
fn lists_nest_and_number() {
    let html = "<ul><li>甲<ul><li>甲一</li><li>甲二</li></ul></li><li>乙</li></ul>\
        <ol start=\"3\"><li><p>三</p></li><li>四</ol><p>后文</p>";
    assert_eq!(
        md(html),
        "- 甲\n  - 甲一\n  - 甲二\n- 乙\n\n3. 三\n4. 四\n\n后文\n"
    );
    let tasks =
        "<ul><li><input type=\"checkbox\" checked>做完</li><li><input type=checkbox>没做</li></ul>";
    assert_eq!(md(tasks), "- [x] 做完\n- [ ] 没做\n");
}

#[test]
fn tables_and_code_blocks_keep_their_shape() {
    let html = "<table><thead><tr><th>名称</th><th>说明</th></tr></thead>\
        <tbody><tr><td><p>a|b</p></td><td>一<br>二</td></tr><tr><td>c</td></tr></tbody></table>\
        <pre><code class=\"language-rust\">fn main() {\n    let x = `a`;\n}\n</code></pre>";
    assert_eq!(
        md(html),
        "| 名称 | 说明 |\n| --- | --- |\n| a\\|b | 一<br>二 |\n| c |  |\n\n\
         ```rust\nfn main() {\n    let x = `a`;\n}\n```\n"
    );
}

#[test]
fn online_documents_and_code_editors() {
    // 在线文档用整段的 `<b style="font-weight:normal">` 包着，粗体写在 `span` 的样式里。
    let docs = "<b style=\"font-weight:normal;\" id=\"docs-internal-guid-1\">\
        <p><span style=\"font-weight:700\">重点</span><span> 和 </span>\
        <span style=\"font-style:italic\">斜体</span></p></b>";
    assert_eq!(md(docs), "**重点** 和 *斜体*\n");
    // 代码编辑器复制出的只是带颜色的 `span`：没有结构，按纯文本粘贴。
    let code = "<div style=\"color: #333;white-space: pre;\"><div><span style=\"color: #00f;\">fn\
        </span> main() {}</div></div>";
    assert!(!from_html(code).rich);
    // 文字里的标记字符转义，段首像列表记号的也转义；词中间的下划线不转义。
    assert_eq!(
        md("<p>1. a*b* snake_case _x_</p>"),
        "\\1. a\\*b\\* snake_case \\_x\\_\n"
    );
    // 脚本、样式、空链接和 `data:` 图片不写出。
    let noise = "<style>p{}</style><script>x()</script><p><a href=\"#top\">顶部</a>\
        <img src=\"data:image/png;base64,AAAA\" alt=\"嵌入\"></p>";
    assert_eq!(md(noise), "顶部嵌入\n");
}
