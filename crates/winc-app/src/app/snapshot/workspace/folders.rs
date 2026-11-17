//! 附加目录的测试：添加和移除、文件树的顶层行、菜单、相对路径、随会话恢复。

use winit::keyboard::{Key, NamedKey};

use super::*;
use crate::app::input::context_menu::{MenuCommand, MenuEntry, MenuTarget};

/// 文件树各行的名字和缩进层级。
fn rows(app: &App) -> Vec<(String, usize)> {
    let ws = app.editor.workspace.as_ref().unwrap();
    (ws.rows().iter())
        .map(|entry| (entry.label().into_owned(), entry.depth))
        .collect()
}

/// 在 `path` 上打开文件树菜单，返回列出的命令。
fn listed(app: &mut App, path: Option<PathBuf>) -> Vec<MenuCommand> {
    app.open_context_menu(MenuTarget::Tree(path), (10.0, 10.0));
    let menu = app.overlays.context_menu.take().unwrap();
    (menu.entries.into_iter())
        .filter_map(|entry| match entry {
            MenuEntry::Item(command, _) => Some(command),
            _ => None,
        })
        .collect()
}

#[test]
fn a_folder_can_be_added_to_the_window_and_removed() {
    let main = sample_folder("folders-main").canonicalize().unwrap();
    let other = search_folder("folders-other").canonicalize().unwrap();
    let mut app = app_with(&main.join("short.rs"), false);
    paint(&mut app);
    let single = rows(&app);
    assert!(single.iter().all(|(_, depth)| *depth == 0));
    // 空白处的菜单可以添加，没有可移除的。
    let blank = listed(&mut app, None);
    assert!(blank.contains(&MenuCommand::AddFolder));
    assert!(!blank.contains(&MenuCommand::RemoveFolder));

    // 项目菜单里「打开目录…」的下一项是「添加目录到窗口…」；没有打开目录时没有这一项。
    let project = |app: &mut App| -> Vec<MenuCommand> {
        app.open_context_menu(MenuTarget::Project, (10.0, 10.0));
        let menu = app.overlays.context_menu.take().unwrap();
        (menu.entries.into_iter())
            .filter_map(|entry| match entry {
                MenuEntry::Item(command, _) => Some(command),
                _ => None,
            })
            .collect()
    };
    let menu = project(&mut app);
    let open = menu
        .iter()
        .position(|c| *c == MenuCommand::OpenFolder)
        .unwrap();
    assert_eq!(menu.get(open + 1), Some(&MenuCommand::AddFolder));
    assert!(!project(&mut blank_app(false)).contains(&MenuCommand::AddFolder));

    app.add_folder(&other);
    paint(&mut app);
    assert_eq!(app.extra_folders(), std::slice::from_ref(&other));
    let both = rows(&app);
    let tops: Vec<&str> = (both.iter().filter(|(_, depth)| *depth == 0))
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(tops, ["folders-main", "folders-other"]);
    assert!(both.iter().filter(|(_, depth)| *depth == 1).count() >= single.len());
    // 窗口的名字和打开的目录不变。
    assert_eq!(app.editor.root(), Some(main.as_path()));

    // 目录自己的一行：不能改名、删除、剪切；附加的那个可以移除。
    let on_main = listed(&mut app, Some(main.clone()));
    let on_other = listed(&mut app, Some(other.clone()));
    for menu in [&on_main, &on_other] {
        assert!(menu.contains(&MenuCommand::AddFolder));
        assert!(!menu.contains(&MenuCommand::Rename) && !menu.contains(&MenuCommand::Delete));
        assert!(!menu.contains(&MenuCommand::Cut));
    }
    assert!(!on_main.contains(&MenuCommand::RemoveFolder));
    assert!(on_other.contains(&MenuCommand::RemoveFolder));
    // 其中的文件照常：可以改名、删除，没有移除。
    let inside = other.join("src/main.rs");
    app.editor.workspace.as_mut().unwrap().reveal(&inside);
    let file = listed(&mut app, Some(inside));
    assert!(file.contains(&MenuCommand::Rename) && file.contains(&MenuCommand::Delete));
    assert!(!file.contains(&MenuCommand::RemoveFolder));

    // 相对路径相对于文件所在的目录。
    assert_eq!(
        app.relative_path(&other.join("src/main.rs")).as_deref(),
        Some("src/main.rs")
    );
    assert_eq!(
        app.relative_path(&main.join("short.rs")).as_deref(),
        Some("short.rs")
    );
    assert_eq!(app.relative_path(&other), None);

    // 删除、改名对目录自己的一行不起作用。
    app.ask_delete(&other);
    app.start_rename(&other);
    assert!(app.panels.name_input.is_none() && other.is_dir());

    // 重复添加、添加其中的子目录：说明原因，不改变列表。
    app.add_folder(&other.join("src"));
    let notice = app.notice.text.take();
    assert_eq!(
        notice.as_deref(),
        Some(winc_i18n::t!("workspace-add-folder-overlaps"))
    );
    assert_eq!(app.extra_folders().len(), 1);
    shoot(&mut app, "folders-two");

    // 有附加的目录时项目菜单里可以给这一组起名：输入框里是现在的名字，留空恢复为目录名。
    assert!(project(&mut app).contains(&MenuCommand::RenameGroup));
    app.ask_rename_group();
    assert!(app.overlays.form_dialog.is_some());
    app.focused_field().unwrap().insert("  数据平台  ");
    assert!(app.dialog_key(&Key::Named(NamedKey::Enter), "", false));
    assert!(app.overlays.form_dialog.is_none());
    assert_eq!(app.editor.workspace.as_ref().unwrap().name, "数据平台");
    app.ask_rename_group();
    assert_eq!(app.focused_field().unwrap().text(), "数据平台");
    app.close_form_dialog();

    // 改动当时就记进会话，不等退出；也不改变下次启动时打开哪个目录。
    let saved = app.state.session_for(&main).unwrap();
    assert_eq!(saved.folders, std::slice::from_ref(&other));
    assert_eq!(saved.name.as_deref(), Some("数据平台"));
    let state = app.state.clone();
    let mut again = blank_app(false);
    again.state = state.clone();
    again.open_folder_now(&main);
    assert_eq!(again.extra_folders(), std::slice::from_ref(&other));
    assert_eq!(again.editor.workspace.as_ref().unwrap().name, "数据平台");

    // 最近列表里这一组用起的名字和叠放的目录图标；没起名时在目录名后注明附加的个数。
    let mut elsewhere = blank_app(false);
    elsewhere.state = state;
    elsewhere.open_folder_now(&other);
    let index = (elsewhere.state.recent.iter())
        .position(|folder| *folder == main)
        .unwrap();
    let recent = MenuCommand::OpenRecent(index);
    assert_eq!(elsewhere.menu_label(recent).title, "数据平台");
    let group_icon = crate::app::input::context_menu::MenuIcon::Folders;
    assert_eq!(elsewhere.menu_icon_kind(recent), Some(group_icon));
    // 「打开最近」面板同样显示组名，按组名和目录本身的名字都能找到。
    for (query, marked) in [("数据", true), ("folders-main", false)] {
        elsewhere.open_recent_folders();
        elsewhere.overlays.picker.input.set_text(query);
        elsewhere.picker_refresh();
        assert_eq!(elsewhere.overlays.picker.hits, [index], "{query}");
        assert_eq!(!elsewhere.folder_matched(0).0.is_empty(), marked, "{query}");
        elsewhere.close_picker();
    }
    paint(&mut elsewhere);
    elsewhere.open_project_menu();
    shoot(&mut elsewhere, "folders-recent");
    elsewhere.overlays.context_menu = None;
    again.rename_group("");
    again.remember_session();
    assert_eq!(again.recent_title(&main), "folders-main +1");
    assert_eq!(
        again.editor.workspace.as_ref().unwrap().name,
        "folders-main"
    );

    // 移除：只从窗口中拿掉，磁盘上的内容不动，文件树回到只列打开的目录。
    app.run_tree_command(MenuCommand::RemoveFolder, Some(other.clone()));
    paint(&mut app);
    assert!(app.extra_folders().is_empty() && other.join("src/main.rs").is_file());
    // 名字属于这一组：只剩打开的目录时取消，菜单里不再有重命名。
    assert_eq!(app.editor.workspace.as_ref().unwrap().name, "folders-main");
    assert!(!project(&mut app).contains(&MenuCommand::RenameGroup));
    assert!(app.recent_group(&main).is_none());
    assert_eq!(rows(&app), single);
    let _ = std::fs::remove_dir_all(&main);
    let _ = std::fs::remove_dir_all(&other);
}
