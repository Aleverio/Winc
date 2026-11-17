//! 查找 `git` 程序的测试：设置的程序、找不到时的原因、本机没有 `git` 时的仓库。

use super::*;

/// 设置的程序可以是完整路径或 `PATH` 里的程序名；空白视为未设置。
#[test]
fn a_configured_program_is_used_by_path_or_by_name() {
    let shell = Shell::with_vars(std::env::vars_os().collect());
    let Some(real) = shell.find("git") else {
        return;
    };
    for program in [Some(real.to_str().unwrap()), Some("git"), Some("  "), None] {
        let git = Git::find_with(&shell, program).unwrap();
        assert!(git.local, "{program:?}");
    }
    assert_eq!(Git::find_with(&shell, real.to_str()).unwrap().program, real);
}

/// 设置的程序不存在时报告设置的写法，不退回 `PATH` 里的 `git`。
#[test]
fn a_missing_configured_program_is_reported_by_name() {
    let shell = Shell::with_vars(std::env::vars_os().collect());
    let missing = crate::test_util::scratch("no-git").join("bin").join("git");
    for program in [missing.to_str().unwrap(), "winc-no-such-git-program"] {
        let error = Git::find_with(&shell, Some(program)).unwrap_err();
        assert_eq!(error, Unavailable::NotFound(program.to_owned()));
        assert!(error.to_string().contains(program));
    }
}

/// 本机没有 `git` 时本机的仓库按未安装报告，不运行不存在的程序。
#[test]
fn a_local_repository_without_local_git_reports_not_installed() {
    let shell = Shell::with_vars(std::env::vars_os().collect());
    let Ok(mut git) = Git::find(&shell) else {
        return;
    };
    git.local = false;
    let dir = crate::test_util::scratch("no-local-git");
    let error = git.status(&dir, &Cancel::default()).unwrap_err();
    assert!(
        error.contains(&Unavailable::NotInstalled.to_string()),
        "{error}"
    );
}
