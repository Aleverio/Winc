//! 改写图片大小的测试。

use super::*;

fn apply(text: &str, nth: usize, width: Option<Width>) -> String {
    let (range, new) = resize(text, nth, width).expect("应当改写");
    format!("{}{new}{}", &text[..range.start], &text[range.end..])
}

#[test]
fn markdown_images_become_html_with_a_width() {
    let text = "![截图](shot.png) 后文";
    assert_eq!(
        apply(text, 0, Some(Width::Px(320))),
        "<img src=\"shot.png\" alt=\"截图\" width=\"320\"> 后文"
    );
    // 标题、需要转义的字符、Obsidian 的 `|300` 都照顾到。
    let text = "![a \"b\" |300](<my shot.png> \"标题\")";
    assert_eq!(
        apply(text, 0, Some(Width::Percent(50))),
        "<img src=\"my shot.png\" alt=\"a &quot;b&quot;\" title=\"标题\" width=\"50%\">"
    );
    // 去掉大小：没有写大小时不改，`|300` 去掉。
    assert_eq!(resize("![a](b.png)", 0, None), None);
    assert_eq!(apply("![a |300x200](b.png)", 0, None), "![a](b.png)");
    // 认不出的写法不改。
    assert_eq!(resize("![a][ref]", 0, Some(Width::Px(10))), None);
}

#[test]
fn html_images_keep_their_other_attributes() {
    let text = "<img src=\"a.png\" alt=\"A\" width=\"100\" height=50 class=x>";
    assert_eq!(
        apply(text, 0, Some(Width::Px(240))),
        "<img src=\"a.png\" alt=\"A\" class=x width=\"240\">"
    );
    let text = "<img src='a.png' style=\"zoom: 50%; border: 0\"/>";
    assert_eq!(
        apply(text, 0, Some(Width::Px(80))),
        "<img src='a.png' style=\"border: 0\" width=\"80\"/>"
    );
    // HTML 块里的第二张图。
    let text =
        "<p align=\"center\">\n  <img src=\"a.png\">\n  <img src=\"b.png\" width=\"9\">\n</p>";
    assert_eq!(
        apply(text, 1, Some(Width::Percent(25))),
        "<p align=\"center\">\n  <img src=\"a.png\">\n  <img src=\"b.png\" width=\"25%\">\n</p>"
    );
    assert_eq!(resize(text, 2, None), None);
}

#[test]
fn removing_the_size_turns_plain_html_back_into_markdown() {
    let text = "<img src=\"my shot.png\" alt=\"a [b]\" width=\"320\">";
    assert_eq!(apply(text, 0, None), "![a \\[b\\]](<my%20shot.png>)");
    let text = "<img src=\"a.png\" alt=\"&amp;\" title=\"t\" width=\"1\">";
    assert_eq!(apply(text, 0, None), "![&](a.png \"t\")");
    // 带其他属性的保留成 HTML，只去掉大小。
    let text = "<img src=\"a.png\" class=x width=\"1\">";
    assert_eq!(apply(text, 0, None), "<img src=\"a.png\" class=x>");
    // 往返一次回到原样。
    let text = "![图](a.png)";
    let sized = apply(text, 0, Some(Width::Px(200)));
    assert_eq!(apply(&sized, 0, None), text);
}
