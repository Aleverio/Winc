//! 声明式插件命令的执行：只启动用户显式信任的外部程序，不加载插件代码。
//!
//! 这里是界面层与子进程之间的一条窄接口：输入是已经展开好的程序、参数、工作目录和环境变量，
//! 输出是进程的状态与文本。占位符展开、信任判断和结果展示都由界面层负责。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::shell::{self, Cancel};

/// 一次插件命令调用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// 要启动的程序。
    pub program: String,
    /// 已展开的参数。
    pub args: Vec<String>,
    /// 工作目录。
    pub cwd: PathBuf,
    /// 追加的环境变量。
    pub env: BTreeMap<String, String>,
}

/// 插件命令的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// 程序是否正常退出且返回码为 0。
    pub ok: bool,
    /// 合并标准错误后的输出；失败时同时包含原因。
    pub output: String,
}

/// 运行一次插件命令，等待它结束（最长 30 秒）并收集输出。
///
/// 环境变量从用户 shell 的环境继承：图形应用启动时只拿到最小环境，用户的工具常在 `PATH` 中。
pub fn run(request: &Request, shell: &shell::Shell) -> Outcome {
    let mut command = shell.command(Path::new(&request.program));
    command.args(&request.args);
    if !request.cwd.as_os_str().is_empty() {
        command.current_dir(&request.cwd);
    }
    for (key, value) in &request.env {
        command.env(key, value);
    }
    match shell::process::run(&mut command, Vec::new(), LIMIT, &Cancel::default()) {
        Ok(output) => {
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.is_empty() {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(&stderr);
            }
            Outcome {
                ok: output.status.success(),
                output: text.trim_end().to_owned(),
            }
        }
        Err(err) => Outcome {
            ok: false,
            output: err.to_string(),
        },
    }
}

/// 插件命令的最长运行时间。
const LIMIT: Duration = Duration::from_secs(30);
