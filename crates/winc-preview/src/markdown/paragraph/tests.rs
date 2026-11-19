use super::*;

fn texts(lines: &[&str], kind: Kind) -> Vec<String> {
    change(lines, kind).into_iter().map(|l| l.text).collect()
}

#[test]
fn headings_are_set_replaced_and_toggled_back() {
    assert_eq!(texts(&["标题"], Kind::Heading(2)), ["## 标题"]);
    assert_eq!(texts(&["### 标题"], Kind::Heading(1)), ["# 标题"]);
    // 已经是这一级时改回正文；⌘0 总是改回正文。
    assert_eq!(texts(&["## 标题"], Kind::Heading(2)), ["标题"]);
    assert_eq!(texts(&["## 标题"], Kind::Heading(0)), ["标题"]);
    // 引用和列表里的标题写在标记之后；`#tag` 不是标题。
    assert_eq!(texts(&["> - 一项"], Kind::Heading(3)), ["> - ### 一项"]);
    assert_eq!(texts(&["#tag"], Kind::Heading(1)), ["# #tag"]);
    // 光标随内容移动。
    let line = &change(&["## 标题"], Kind::Heading(4))[0];
    assert_eq!((line.old_start, line.new_start), (3, 5));
}

#[test]
fn quotes_and_lists_toggle_on_every_selected_line() {
    assert_eq!(texts(&["一", "", "二"], Kind::Quote), ["> 一", "", "> 二"]);
    assert_eq!(texts(&["> 一", ">> 二"], Kind::Quote), ["一", "> 二"]);
    assert_eq!(texts(&["一", "二"], Kind::Ordered), ["1. 一", "2. 二"]);
    assert_eq!(texts(&["- 一", "* 二"], Kind::Ordered), ["1. 一", "2. 二"]);
    assert_eq!(texts(&["1. 一", "2. 二"], Kind::Ordered), ["一", "二"]);
    // 任务框跟着换记号，去掉列表时一起去掉。
    assert_eq!(texts(&["1. [x] 做完"], Kind::Bullet), ["- [x] 做完"]);
    assert_eq!(texts(&["- [ ] 待办"], Kind::Bullet), ["待办"]);
    // 引用里的列表保留引用和缩进。
    assert_eq!(texts(&[">   二"], Kind::Bullet), [">   - 二"]);
}

#[test]
fn blocks_and_links_place_the_caret_where_typing_continues() {
    assert_eq!(fenced("```", ""), ("```\n\n```".to_owned(), 3));
    assert_eq!(fenced("$$", "x^2"), ("$$\nx^2\n$$".to_owned(), 6));
    let (table, caret) = table();
    assert!(table.starts_with("|  |") && caret == 2);
    assert_eq!(link("", None, false), ("[]()".to_owned(), 1..1));
    assert_eq!(link("文字", None, false), ("[文字]()".to_owned(), 9..9));
    assert_eq!(
        link("图", Some("a b.png"), true),
        ("![图](<a b.png>)".to_owned(), 17..17)
    );
}

#[test]
fn link_targets_are_found_while_typing_a_path() {
    assert_eq!(link_target("见 [文档]("), Some((13, false)));
    assert_eq!(link_target("![图](img/a"), Some((7, true)));
    // 网址、锚点、已经结束的地址和行内代码里不补全。
    assert_eq!(link_target("[x](https://a"), None);
    assert_eq!(link_target("[x](http:"), None);
    // 只写了网址的开头时仍然补全：`ma`、`ht` 也是文件名的开头。
    assert_eq!(link_target("[x](ma"), Some((4, false)));
    assert_eq!(link_target("[x](#a"), None);
    assert_eq!(link_target("[x](a.md) 后面"), None);
    assert_eq!(link_target("`[x](a"), None);
    assert_eq!(link_target("没有链接"), None);
}
