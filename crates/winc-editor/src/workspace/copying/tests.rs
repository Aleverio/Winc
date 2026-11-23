use std::fs;
use std::io;
#[cfg(unix)]
use std::path::Path;

use super::*;
use crate::test_util::scratch;

#[test]
fn progress_follows_the_item_being_copied() {
    let dir = scratch("copying-progress");
    fs::write(dir.join("a.txt"), "hello").unwrap();
    fs::create_dir(dir.join("kit")).unwrap();
    fs::write(dir.join("kit/b.txt"), "abc").unwrap();
    fs::write(dir.join("kit/c.txt"), "defg").unwrap();
    let copying = Copying::default();
    let links: Links = |from, to| winc_backend::fs::copy_symlink(from, to);

    // 单个文件：名字、大小和已复制的字节数。
    copy_item(&dir.join("a.txt"), &dir.join("a2.txt"), links, &copying).unwrap();
    assert_eq!(copying.item(), ("a.txt".to_owned(), Some(5)));
    assert_eq!(copying.copied(), 5);

    // 目录：大小事先不知道，已复制的字节数从零重新累计。
    copy_item(&dir.join("kit"), &dir.join("kit2"), links, &copying).unwrap();
    assert_eq!(copying.item(), ("kit".to_owned(), None));
    assert_eq!(copying.copied(), 7);
    assert_eq!(fs::read_to_string(dir.join("kit2/c.txt")).unwrap(), "defg");
}

#[test]
fn downloading_a_directory_skips_special_items_and_keeps_normal_files() {
    let dir = scratch("download-skip-special");
    let root = dir.join("project");
    fs::create_dir_all(root.join("data")).unwrap();
    fs::write(root.join("main.rs"), "fn main() {}").unwrap();

    let copying = Copying::default();
    let links: Links = |_, _| Ok(());
    let mut state = DownloadState::new(&root);
    copy_tree(
        &root,
        &dir.join("out"),
        Kind::Dir,
        links,
        &copying,
        true,
        Some(&mut state),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(dir.join("out/main.rs")).unwrap(),
        "fn main() {}"
    );
    assert!(state.skipped.is_empty());

    // Windows 上无法建 FIFO/socket，用普通文件代替并直接传入 Kind::Other：走的是同一条跳过分支。
    fs::write(root.join("data/app.sock"), "runtime").unwrap();
    copy_tree(
        &root.join("data/app.sock"),
        &dir.join("out/data/app.sock"),
        Kind::Other,
        links,
        &copying,
        false,
        Some(&mut state),
    )
    .unwrap();

    assert!(!dir.join("out/data/app.sock").exists());
    assert_eq!(
        state.skipped,
        vec![Skipped {
            name: "project/data/app.sock".into(),
            reason: winc_i18n::t!("editor-download-skipped-special").to_owned(),
        }]
    );
}

#[cfg(unix)]
#[test]
fn downloading_a_directory_with_a_real_socket_keeps_files_and_links() {
    use std::os::unix::fs::symlink;
    use std::os::unix::net::UnixListener;

    // Unix socket 的路径受 SUN_LEN（约 108 字节）限制，scratch 的长名字放不下。
    let dir = std::env::temp_dir().join(format!("winc374-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let root = dir.join("project");
    fs::create_dir_all(root.join("data")).unwrap();
    fs::write(root.join("main.rs"), "ok").unwrap();
    let listener = UnixListener::bind(root.join("data/app.sock")).unwrap();
    symlink("../main.rs", root.join("data/link")).unwrap();
    fs::create_dir(dir.join("out")).unwrap();

    let outcome = crate::paste::download_into(
        std::slice::from_ref(&root),
        &dir.join("out"),
        &Copying::default(),
    );
    assert!(outcome.failed.is_none(), "失败：{:?}", outcome.failed);
    assert_eq!(outcome.done, [dir.join("out/project")]);
    assert_eq!(
        outcome.skipped,
        vec![Skipped {
            name: "project/data/app.sock".into(),
            reason: winc_i18n::t!("editor-download-skipped-special").to_owned(),
        }]
    );
    assert_eq!(
        fs::read_to_string(dir.join("out/project/main.rs")).unwrap(),
        "ok"
    );
    assert_eq!(
        fs::read_link(dir.join("out/project/data/link")).unwrap(),
        Path::new("../main.rs")
    );
    drop(listener);
}

#[cfg(not(unix))]
#[test]
fn downloading_skips_untranslatable_symlinks_but_propagates_real_io_errors() {
    let dir = scratch("download-skip-links");
    let root = dir.join("project");
    fs::create_dir(&root).unwrap();
    let missing =
        |_: &std::path::Path, _: &std::path::Path| Err(io::Error::from(io::ErrorKind::NotFound));
    let denied = |_: &std::path::Path, _: &std::path::Path| {
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    };
    // 远端 ELOOP 经协议后是 Other + 系统说明，没有 raw errno；两种写法都要认得。
    let looping = |_: &std::path::Path, _: &std::path::Path| {
        Err(io::Error::other("Symbolic link loop (os error 40)"))
    };
    let looping_alt = |_: &std::path::Path, _: &std::path::Path| {
        Err(io::Error::other(
            "Too many levels of symbolic links (os error 40)",
        ))
    };
    let mut state = DownloadState::new(&root);

    copy_tree(
        &root.join("dangling"),
        &dir.join("out/dangling"),
        Kind::Symlink,
        missing,
        &Copying::default(),
        false,
        Some(&mut state),
    )
    .unwrap();
    assert_eq!(state.skipped.len(), 1);
    assert_eq!(state.skipped[0].name, "project/dangling");

    copy_tree(
        &root.join("looping"),
        &dir.join("out/looping"),
        Kind::Symlink,
        looping,
        &Copying::default(),
        false,
        Some(&mut state),
    )
    .unwrap();
    assert_eq!(state.skipped.len(), 2);
    assert_eq!(state.skipped[1].name, "project/looping");
    assert!(
        state.skipped[1]
            .reason
            .to_ascii_lowercase()
            .contains("symbolic link")
    );

    copy_tree(
        &root.join("looping-alt"),
        &dir.join("out/looping-alt"),
        Kind::Symlink,
        looping_alt,
        &Copying::default(),
        false,
        Some(&mut state),
    )
    .unwrap();
    assert_eq!(state.skipped.len(), 3);
    assert_eq!(state.skipped[2].name, "project/looping-alt");

    let error = copy_tree(
        &root.join("locked"),
        &dir.join("out/locked"),
        Kind::Symlink,
        denied,
        &Copying::default(),
        false,
        Some(&mut state),
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(state.skipped.len(), 3, "真实 I/O 错误不能记成跳过");
    assert_eq!(state.failed, "project/locked");
}

#[test]
fn a_failure_inside_a_directory_names_the_child_not_the_root() {
    let dir = scratch("download-failed-child");
    let root = dir.join("project");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("plain"), "x").unwrap();
    let links: Links = |_, _| Ok(());
    let mut state = DownloadState::new(&root);

    // 把普通文件当目录递归：create_dir 成功、read_dir 失败，失败点在这个子项。
    copy_tree(
        &root.join("plain"),
        &dir.join("out/plain"),
        Kind::Dir,
        links,
        &Copying::default(),
        false,
        Some(&mut state),
    )
    .unwrap_err();

    assert_eq!(state.failed, "project/plain");
}

#[test]
fn a_cancelled_copy_leaves_nothing_behind() {
    let dir = scratch("copying-cancel");
    fs::create_dir(dir.join("kit")).unwrap();
    fs::write(dir.join("kit/b.txt"), "abc").unwrap();
    let copying = Copying::default();
    copying.cancel();
    let links: Links = |from, to| winc_backend::fs::copy_symlink(from, to);

    let error = copy_item(&dir.join("kit"), &dir.join("kit2"), links, &copying).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    assert!(!dir.join("kit2").exists());
    // 取消不影响源。
    assert!(dir.join("kit/b.txt").exists());
}
