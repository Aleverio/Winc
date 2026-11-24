//! `winct` 可执行文件的入口，程序的实现在 `winct-app` 中。

// 不加时 Windows 每次启动 winct 都会另开一个控制台窗口。
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    winct_app::run();
}
