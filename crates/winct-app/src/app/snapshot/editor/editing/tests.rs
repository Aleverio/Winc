//! 编辑区其余快照测试：超长行、自动换行与 Unicode 转义。

use super::*;

/// 回归：显示行号超过 2²⁴（`f32` 无法逐一表示）时，相邻的行仍各占一行，行号不重叠。
#[test]
fn lines_past_sixteen_million_rows_each_take_their_own_row() {
    let dir = sample_folder("rows-past-f32");
    let path = dir.join("lines.txt");
    let lines = (1usize << 24) + 100_000;
    std::fs::write(&path, "\n".repeat(lines)).unwrap();
    let mut app = app_with(&path, false);
    paint(&mut app);
    // 从一个奇数行开始显示：按 `f32` 换算时奇数行并入相邻的偶数行。
    app.editor.active_mut().unwrap().scroll_line = ((1 << 24) + 50_001) as f64;
    paint(&mut app);

    let (gutter, text, lh) = (app.layout.gutter, app.layout.text, app.fonts.line_height);
    let image = app.frame.renderer.as_ref().unwrap().image().unwrap();
    let width = image.width as usize;
    let px = |v: f32| (v * SCALE) as usize;
    // 行号栏中每一行都画有行号。
    for row in 0..8 {
        let top = text.y + row as f32 * lh;
        let background = image.pixels[px(top + 1.0) * width + px(gutter.x + 1.0)];
        let ink = (px(top + 1.0)..px(top + lh - 1.0))
            .flat_map(|y| &image.pixels[y * width + px(gutter.x)..y * width + px(gutter.right())])
            .filter(|&&p| p != background)
            .count();
        assert!(ink > 0, "第 {row} 行没有行号");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// 超长行默认只显示开头的 `MAX_LINE_CHARS` 个字符：点击截断处的「显示整行」展开这一行，
/// 设置「超长行完整显示」展开所有的行；展开后可以滚动、点击到行尾，换行时按整行断开。
#[test]
fn long_lines_expand_by_clicking_or_by_the_setting() {
    let dir = sample_folder("long-lines");
    let path = dir.join("long.txt");
    let long = format!("{}{}", "a".repeat(10_000), "b".repeat(20_000));
    std::fs::write(&path, format!("{long}\nshort\n{long}\n")).unwrap();
    let mut app = app_with(&path, false);
    paint(&mut app);
    let scroll_to = |app: &mut App, x: f32| {
        app.editor.active_mut().unwrap().scroll_x = x;
        paint(app);
    };
    let lines = |app: &App| app.hits.long_lines.iter().map(|h| h.1).collect::<Vec<_>>();
    // 文本区第 `row` 行右半边是否画有内容。
    let inked = |app: &App, row: usize| {
        let (t, lh) = (app.layout.text, app.fonts.line_height);
        let image = app.frame.renderer.as_ref().unwrap().image().unwrap();
        let px = |v: f32| (v * SCALE) as usize;
        let (y0, y1) = (
            px(t.y + row as f32 * lh + 2.0),
            px(t.y + (row + 1) as f32 * lh - 2.0),
        );
        let (x0, x1) = (px(t.x + t.w / 2.0), px(t.right() - 40.0));
        let width = image.width as usize;
        let background = image.pixels[y0 * width + x1];
        (y0..y1).any(|y| {
            image.pixels[y * width + x0..y * width + x1]
                .iter()
                .any(|&p| p != background)
        })
    };

    // 滚动到截断处：两条超长行之后都有「显示整行」。
    let narrow = app.text_max_scroll_x();
    scroll_to(&mut app, narrow);
    assert_eq!(lines(&app), [0, 2]);
    let (rect, _) = app.hits.long_lines[0];
    assert!(
        rect.right() <= app.layout.text.right(),
        "「显示整行」滚动不到屏幕内"
    );
    click(&mut app, (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0));
    paint(&mut app);
    let doc = app.editor.active().unwrap();
    assert!(doc.shows_whole_line(0) && !doc.shows_whole_line(2));
    assert_eq!(lines(&app), [2]);

    // 展开的行可以滚动到行尾：行尾画有文字，点击处与字符对应；未展开的行在该处没有内容。
    let wide = app.text_max_scroll_x();
    assert!(
        wide > narrow + 19_000.0 * app.metrics.cell,
        "{narrow} {wide}"
    );
    let near_end = wide - 20.0 * app.metrics.cell;
    scroll_to(&mut app, near_end);
    assert!(inked(&app, 0) && !inked(&app, 2));
    let (t, lh) = (app.layout.text, app.fonts.line_height);
    let x = t.x + TEXT_PAD + 29_990.0 * app.metrics.cell - app.editor.active().unwrap().scroll_x;
    assert_eq!(app.pos_at(x, t.y + 0.5 * lh), Some(29_990));

    // 设置打开后所有的行都显示整行，不再有截断说明。
    app.config.whole_lines = Some(true);
    paint(&mut app);
    assert!(app.editor.active().unwrap().shows_whole_line(2));
    assert!(lines(&app).is_empty() && inked(&app, 2));
    app.config.whole_lines = None;
    paint(&mut app);
    assert!(!inked(&app, 2));

    // 换行时展开的行按整行断开，未展开的行只断开显示的部分。
    app.run_action(Action::ToggleWordWrap);
    paint(&mut app);
    let doc = app.editor.active().unwrap();
    assert!(
        doc.rows_of(0) > 2 * doc.rows_of(2),
        "{} {}",
        doc.rows_of(0),
        doc.rows_of(2)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// 展开的行长达数百万列时，行尾附近的字符仍绘制在按列算出的位置上（`f32` 在数千万像素处只能
/// 精确到几个像素）。
#[test]
fn the_far_end_of_a_whole_line_is_drawn_in_place() {
    let dir = sample_folder("long-line-far-end");
    let path = dir.join("wide.txt");
    let col = 2_999_950;
    std::fs::write(&path, format!("{}#{}\n", " ".repeat(col), " ".repeat(49))).unwrap();
    let mut app = app_with(&path, false);
    app.config.whole_lines = Some(true);
    paint(&mut app);
    app.editor.active_mut().unwrap().scroll_x = (col - 20) as f32 * app.metrics.cell;
    paint(&mut app);

    let (t, lh, cell) = (app.layout.text, app.fonts.line_height, app.metrics.cell);
    let scroll = f64::from(app.editor.active().unwrap().scroll_x);
    let expected = f64::from(t.x + TEXT_PAD) + col as f64 * f64::from(cell) - scroll;
    let image = app.frame.renderer.as_ref().unwrap().image().unwrap();
    let px = |v: f32| (v * SCALE) as usize;
    let (y0, y1) = (px(t.y + 2.0), px(t.y + lh - 2.0));
    let width = image.width as usize;
    let background = image.pixels[y0 * width + px(t.right() - 40.0)];
    let inked = |x: usize| (y0..y1).any(|y| image.pixels[y * width + x] != background);
    let left = (px(t.x)..px(t.right() - 40.0)).find(|&x| inked(x)).unwrap();
    let left = left as f64 / f64::from(SCALE);
    assert!(
        left >= expected - 0.5 && left < expected + f64::from(cell),
        "{left} {expected}"
    );
    // 点击该字符的左半边，落在它之前。
    let pos = app.pos_at(expected as f32 + 0.3 * cell, t.y + 0.5 * lh);
    assert_eq!(pos, Some(col));
    let _ = std::fs::remove_dir_all(&dir);
}

/// 自动换行时，超过 `MAX_LINE_CHARS` 个字符而不足 `MAX_LINE_BYTES` 字节的行可以通过点击「显示整行」
/// 或设置展开：截断说明消失，行尾可以绘制，点击位置与字符对应。
#[test]
fn wrapped_long_lines_expand_by_clicking_or_by_the_setting() {
    use winct_core::text::{MAX_LINE_BYTES, MAX_LINE_CHARS};
    let dir = sample_folder("long-lines-wrapped");
    // 两种样本都在两个上限之间：3 万个 ASCII 字符、1.2 万个汉字。
    let ascii = format!("{}{}", "a".repeat(10_000), "b".repeat(20_000));
    let cjk = format!("{}{}", "甲".repeat(10_000), "乙".repeat(2_000));
    for (name, long, last) in [("a.txt", &ascii, 'b'), ("c.txt", &cjk, '乙')] {
        assert!(long.chars().count() > MAX_LINE_CHARS && long.len() + 3 < MAX_LINE_BYTES);
        for by_setting in [false, true] {
            let path = dir.join(name);
            std::fs::write(&path, format!("{long}\nshort\n")).unwrap();
            let mut app = app_with(&path, false);
            app.run_action(Action::ToggleWordWrap);
            paint(&mut app);
            let notices = |app: &App| app.hits.long_lines.iter().map(|h| h.1).collect::<Vec<_>>();
            // 滚动到该行的最后几段。
            let to_end = |app: &mut App| {
                let doc = app.editor.active_mut().unwrap();
                doc.scroll_line = (doc.rows_of(0) as f64 - 3.0).max(0.0);
                paint(app);
            };
            to_end(&mut app);
            assert_eq!(notices(&app), [0], "{name}");
            let folded = app.editor.active().unwrap().rows_of(0);

            if by_setting {
                app.config.whole_lines = Some(true);
                paint(&mut app);
            } else {
                let (rect, _) = app.hits.long_lines[0];
                click(&mut app, (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0));
                paint(&mut app);
            }
            let doc = app.editor.active().unwrap();
            assert!(doc.shows_whole_line(0) && doc.rows_of(0) > folded, "{name}");
            to_end(&mut app);
            assert!(notices(&app).is_empty(), "{name} {by_setting}");

            // 最后一段的首字符位于行尾部分：点击后光标落在该字符之前。
            let doc = app.editor.active().unwrap();
            let rows = doc.rows_of(0);
            let breaks = doc.line_breaks(0);
            let start = breaks.start_of(rows - 1);
            assert!(start > MAX_LINE_CHARS, "{name}: {start}");
            let (t, lh) = (app.layout.text, app.fonts.line_height);
            let top = app.editor.active().unwrap().scroll_line;
            let y = t.y + ((rows - 1) as f64 - top) as f32 * lh + 0.5 * lh;
            let pos = app.pos_at(t.x + TEXT_PAD + 1.0, y).unwrap();
            assert_eq!(pos, start, "{name}");
            let doc = app.editor.active().unwrap();
            assert_eq!(doc.text().char_at(pos), Some(last), "{name}");
            // 选中最后一段到行尾：选区按整行的内容测量。
            let end = doc.text().line_end(0);
            app.editor.active_mut().unwrap().selection = winct_core::Selection::new(pos, end);
            paint(&mut app);
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// 「字符转为 Unicode 转义」「Unicode 转义转为字符」：没有选区时转换整个文件，可以撤销。
#[test]
fn unicode_escapes_convert_both_ways() {
    let dir = sample_folder("unicode-escapes");
    let file = dir.join("messages.properties");
    std::fs::write(&file, "greeting=你好\nemoji=😀\n").unwrap();
    let mut app = app_with(&file, false);
    let text = |app: &App| {
        app.editor
            .active()
            .unwrap()
            .text()
            .rope()
            .unwrap()
            .to_string()
    };

    assert!(app.run_action(Action::EscapeUnicode));
    assert_eq!(
        text(&app),
        "greeting=\\u4F60\\u597D\nemoji=\\uD83D\\uDE00\n"
    );
    assert!(app.run_action(Action::UnescapeUnicode));
    assert_eq!(text(&app), "greeting=你好\nemoji=😀\n");
    assert!(app.run_action(Action::Undo));
    assert_eq!(
        text(&app),
        "greeting=\\u4F60\\u597D\nemoji=\\uD83D\\uDE00\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
