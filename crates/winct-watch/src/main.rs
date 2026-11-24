//! 运行在 WSL 发行版内的文件监视程序：`winct-watch <目录>`。标准输入结束（winc 退出）时随之退出。

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(root) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("winct-watch: 需要指定目录");
        return ExitCode::FAILURE;
    };
    match winct_watch::run(&root, true) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("winct-watch: {error}");
            ExitCode::FAILURE
        }
    }
}
