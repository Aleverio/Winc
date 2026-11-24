//! `install` 的测试：判断应用是否位于「应用程序」文件夹，以及应用所在的卷。

use super::*;

#[test]
fn applications_folders_include_the_user_one_and_subfolders() {
    let home = Path::new("/Users/me");
    for inside in [
        "/Applications/winct.app",
        "/Applications/Utilities/winct.app",
        "/Users/me/Applications/winct.app",
    ] {
        assert!(
            inside_applications(Path::new(inside), Some(home)),
            "{inside}"
        );
    }
    for outside in [
        "/Users/me/Downloads/winct.app",
        "/Volumes/winct/winct.app",
        "/ApplicationsOld/winct.app",
        "/Users/other/Applications/winct.app",
    ] {
        assert!(
            !inside_applications(Path::new(outside), Some(home)),
            "{outside}"
        );
    }
}

#[test]
fn the_volume_is_the_first_folder_under_volumes() {
    assert_eq!(
        volume_of(Path::new("/Volumes/winct 1/winct.app")),
        Some(PathBuf::from("/Volumes/winct 1"))
    );
    assert_eq!(volume_of(Path::new("/Users/me/Downloads/winct.app")), None);
}
