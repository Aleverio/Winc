//! Markdown 写作命令的测试：源代码和可编辑的预览中设标题、切换列表和引用、插入块和链接，以及
//! `markdown.shortcuts` 关掉后源代码不再使用这些键。

use super::*;
use crate::keymap::{Action, Context};

/// 打开内容为 `text` 的 Markdown 文件，只看源代码。
fn source_doc(name: &str, text: &str) -> (App, PathBuf) {
    let dir = sample_folder(name);
    let path = dir.join("NOTES.md");
    std::fs::write(&path, text).unwrap();
    let mut app = app_with(&path, false);
    paint(&mut app);
    (app, dir)
}

/// 返回在光标处插入 `|`（有选区时在两端插入 `[`、`]`）后的文档文字。
fn marked(app: &App) -> String {
    let doc = app.editor.active().unwrap();
    let rope = doc.text().rope().unwrap();
    let (a, b) = (
        rope.char_to_byte(doc.selection.from()),
        rope.char_to_byte(doc.selection.to()),
    );
    let mut text = rope.to_string();
    if a == b {
        text.insert(a, '|');
    } else {
        text.insert(b, ']');
        text.insert(a, '[');
    }
    text
}

/// 选中第一处 `needle`。
fn select(app: &mut App, needle: &str) {
    let at = doc_text(app).find(needle).unwrap();
    let doc = app.editor.active_mut().unwrap();
    let rope = doc.text().rope().unwrap();
    let (a, b) = (rope.byte_to_char(at), rope.byte_to_char(at + needle.len()));
    doc.selection = winc_core::Selection::new(a, b);
}

#[test]
fn writing_commands_edit_the_markdown_source() {
    let (mut app, dir) = source_doc("md-writing", "标题\n\n一\n二\n\n正文\n");
    assert_eq!(app.key_context(), Context::Markdown);
    put_caret(&mut app, "题", false);
    assert!(app.run_action(Action::MarkdownHeading2));
    assert_eq!(marked(&app), "## 标|题\n\n一\n二\n\n正文\n");
    // 同一级再按一次改回正文，一次撤销回到原样。
    assert!(app.run_action(Action::MarkdownHeading2));
    assert_eq!(marked(&app), "标|题\n\n一\n二\n\n正文\n");
    app.run(winc_editor::Command::Undo);
    app.run(winc_editor::Command::Undo);
    assert_eq!(doc_text(&app), "标题\n\n一\n二\n\n正文\n");

    // 选中两行：改成有序列表，再改成引用。
    select(&mut app, "一\n二");
    assert!(app.run_action(Action::MarkdownOrderedList));
    assert_eq!(marked(&app), "标题\n\n[1. 一\n2. 二]\n\n正文\n");
    assert!(app.run_action(Action::MarkdownQuote));
    assert_eq!(marked(&app), "标题\n\n[> 1. 一\n> 2. 二]\n\n正文\n");

    // 有字的一行上插入代码块和表格：插在下面，光标在可以接着写的地方。
    put_caret(&mut app, "正文", true);
    assert!(app.run_action(Action::MarkdownCodeBlock));
    assert!(marked(&app).ends_with("正文\n\n```|\n\n```\n"));
    app.run(winc_editor::Command::Undo);
    put_caret(&mut app, "正文", true);
    assert!(app.run_action(Action::MarkdownTable));
    assert!(marked(&app).ends_with("正文\n\n| | |  |\n| --- | --- |\n|  |  |\n"));

    // 选中文字插入链接：剪贴板里是网址时作为地址。
    app.run(winc_editor::Command::Undo);
    app.clipboard.copied = Some("https://winc.si".into());
    select(&mut app, "正文");
    assert!(app.run_action(Action::MarkdownLink));
    assert!(marked(&app).ends_with("[正文](https://winc.si)|\n"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_setting_keeps_markdown_keys_to_the_editable_preview() {
    let (mut app, dir) = source_doc("md-writing-off", "正文\n");
    app.config.markdown_shortcuts = Some(false);
    assert_eq!(app.key_context(), Context::Editor);
    assert!(!app.action_available(Action::MarkdownHeading1));
    // 可编辑的预览中照常使用。
    app.set_preview_mode(PreviewMode::Preview);
    settle(&mut app);
    assert_eq!(app.key_context(), Context::Markdown);
    put_caret(&mut app, "正文", true);
    assert!(app.run_action(Action::MarkdownHeading1));
    settle(&mut app);
    assert_eq!(doc_text(&app), "# 正文\n");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 状态栏上 Markdown 文档的字数：整篇，有选区时另有选中部分；其他文件不显示。
#[test]
fn the_status_bar_counts_words_in_markdown() {
    let (mut app, dir) = source_doc(
        "md-words",
        "# 标题\n\n中文和 English 混排，见 [文档](https://x)。\n",
    );
    // 测试中没有事件循环，后台的计数在当前线程上数完。
    app.track_words();
    assert_eq!(app.word_count(), Some((11, 0)));
    select(&mut app, "中文和");
    app.track_words();
    assert_eq!(app.word_count(), Some((11, 3)));
    shoot(&mut app, "md-words");
    app.open_path(&dir.join("short.rs"));
    app.editor.load_active();
    app.track_words();
    assert_eq!(app.word_count(), None);
    let _ = std::fs::remove_dir_all(&dir);
}
