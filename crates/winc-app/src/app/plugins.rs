//! 插件命令的运行：信任确认、占位符展开、后台执行和结果提示。
//!
//! 插件只是一段声明：程序、参数、工作目录和环境变量。这里不加载任何插件代码，只有用户显式
//! 启用的插件才能贡献命令，命令内容变化后指纹变化，会再次询问是否信任。

use std::path::PathBuf;

use winc_config::{PluginCommand, PluginInput, PluginManifest, PluginRegistry};
use winc_editor::plugin::{Outcome, Request};
use winc_i18n::t;

use super::*;
use crate::app::{AlertButton, NoticeKind};

impl App {
    /// 按目标找到命中的插件和命令。
    fn plugin_command(&self, target: &PluginTarget) -> Option<(&PluginManifest, &PluginCommand)> {
        let plugin = self
            .plugins
            .plugins
            .iter()
            .find(|manifest| manifest.id == target.plugin)?;
        let command = plugin
            .commands
            .iter()
            .find(|command| command.id == target.command)?;
        Some((plugin, command))
    }

    /// 一条命令当前的信任指纹：命令 id 带上插件 id，避免两个插件的同名命令共用信任记录。
    fn fingerprint(plugin: &PluginManifest, command: &PluginCommand) -> String {
        let mut fingerprinted = command.clone();
        fingerprinted.id = PluginRegistry::command_id(plugin, command);
        format!("{:x}", PluginManifest::fingerprint(&fingerprinted))
    }

    /// 运行一条插件命令：按注册表下标定位，第一次运行先请求信任。
    pub(in crate::app) fn run_plugin_command(&mut self, plugin: usize, command: usize) {
        let Some((manifest, command)) = self.plugins.plugins.get(plugin).and_then(|manifest| {
            manifest
                .commands
                .get(command)
                .map(|command| (manifest, command))
        }) else {
            return;
        };
        let target = PluginTarget {
            plugin: manifest.id.clone(),
            command: command.id.clone(),
        };
        let fingerprint = Self::fingerprint(manifest, command);
        if !self.config.plugin_trusted.iter().any(|t| t == &fingerprint) {
            self.ask_plugin_trust(&target, fingerprint);
            return;
        }
        self.launch_plugin(&target);
    }

    /// 从设置页的命令 id（`<插件 id>.<命令 id>`）找到当前注册的插件和命令并运行。
    pub(in crate::app) fn run_plugin_command_id(&mut self, id: &str) {
        for (plugin, manifest) in self.plugins.plugins.iter().enumerate() {
            if let Some(command) = manifest
                .commands
                .iter()
                .position(|command| PluginRegistry::command_id(manifest, command) == id)
            {
                self.run_plugin_command(plugin, command);
                return;
            }
        }
    }

    /// 弹出信任确认框：展示插件名、命令和参数，用户同意后记住指纹并运行。
    fn ask_plugin_trust(&mut self, target: &PluginTarget, fingerprint: String) {
        let Some((plugin, command)) = self.plugin_command(target) else {
            return;
        };
        let message = t!("plugin-trust-title", name = plugin.name.as_str());
        let args = command.args.join(" ");
        let detail = t!(
            "plugin-trust-detail",
            command = command.program.as_str(),
            args = args.as_str(),
        );
        let asked = target.clone();
        self.show_alert(
            &message,
            &detail,
            vec![
                AlertButton::new(t!("plugin-trust-action")),
                AlertButton::cancel(t!("common-cancel")),
            ],
            None,
            move |index, _| {
                (index == 0)
                    .then(|| UserEvent::PluginApproved(asked.clone(), fingerprint.to_owned()))
            },
        );
    }

    /// 用户在信任确认框中同意：记住指纹，然后运行。
    pub(in crate::app) fn plugin_approved(&mut self, target: &PluginTarget, fingerprint: &str) {
        let Some((plugin, command)) = self.plugin_command(target) else {
            return;
        };
        // 提示之后清单若被改过，原先同意的指纹不再对应这条命令，重新询问。
        let current = Self::fingerprint(plugin, command);
        if current != fingerprint {
            self.ask_plugin_trust(target, current);
            return;
        }
        if !self.config.plugin_trusted.iter().any(|t| t == fingerprint) {
            self.config.plugin_trusted.push(fingerprint.to_owned());
            self.save_config();
        }
        self.launch_plugin(target);
    }

    /// 后台运行命令，结束后以 [`UserEvent::PluginDone`] 回到事件循环。
    fn launch_plugin(&mut self, target: &PluginTarget) {
        let Some((plugin, command)) = self.plugin_command(target) else {
            return;
        };
        let Some(request) = self.plugin_request(plugin, command) else {
            self.notify(
                NoticeKind::Error,
                t!("plugin-missing-context", name = plugin.name.as_str()),
            );
            return;
        };
        let shell = self.format.formatter.shell().clone();
        let done = target.clone();
        match self.link.proxy.clone() {
            Some(proxy) => {
                winc_editor::background::spawn_blocking(move || {
                    let outcome = winc_editor::plugin::run(&request, &shell);
                    let _ = proxy.send_event(UserEvent::PluginDone(done, Box::new(outcome)));
                });
            }
            None => {
                let outcome = winc_editor::plugin::run(&request, &shell);
                self.plugin_done(target, outcome);
            }
        }
    }

    /// 展开占位符，生成一次外部命令调用。
    fn plugin_request(&self, plugin: &PluginManifest, command: &PluginCommand) -> Option<Request> {
        let doc = self.editor.active();
        let file = doc
            .and_then(|doc| doc.path.clone())
            .map(|path| path.to_string_lossy().into_owned());
        let root = self
            .editor
            .root()
            .map(|path| path.to_string_lossy().into_owned());
        let selection = doc.and_then(|doc| {
            let range = doc.selection.range();
            if range.is_empty() {
                None
            } else {
                doc.text_range(range.start, range.end)
            }
        });
        let values = self.config.plugin_values.get(&plugin.id);
        let expand = |text: &str| {
            let mut text = text.to_owned();
            for (key, value) in [
                ("file", file.as_deref()),
                ("root", root.as_deref()),
                ("selection", selection.as_deref()),
            ] {
                if let Some(value) = value {
                    text = text.replace(&format!("{{{key}}}"), value);
                }
            }
            if let Some(values) = values {
                for (key, value) in values {
                    text = text.replace(&format!("{{{key}}}"), value);
                }
            }
            text
        };
        let mut args: Vec<String> = command.args.iter().map(|arg| expand(arg)).collect();
        // `input` 是清单声明的额外上下文参数；占位符没写时也能拿到当前文件、目录或选区。
        let context = match command.input {
            PluginInput::None => None,
            PluginInput::File => Some(file.as_deref()),
            PluginInput::Root => Some(root.as_deref()),
            PluginInput::Selection => Some(selection.as_deref()),
        };
        match context {
            None => {}
            Some(Some(value)) => args.push(value.to_owned()),
            // 需要的上下文不存在时宁可不启动，也不把 `{file}` 之类的占位符原样传给外部程序。
            Some(None) => return None,
        }
        let mut env = command.env.clone();
        if let Some(values) = values {
            env.extend(
                values
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone())),
            );
        }
        let cwd = command.cwd.as_deref().map(|cwd| {
            let path = PathBuf::from(expand(cwd));
            if path.is_absolute() {
                path
            } else {
                plugin.dir.join(path)
            }
        });
        Some(Request {
            program: expand(&command.program),
            args,
            cwd: cwd.unwrap_or_else(|| plugin.dir.clone()),
            env,
        })
    }

    /// 命令运行结束：在右上角提示结果。
    pub(in crate::app) fn plugin_done(&mut self, target: &PluginTarget, outcome: Outcome) {
        let Some((plugin, command)) = self.plugin_command(target) else {
            return;
        };
        let name = plugin.name.as_str();
        if outcome.ok {
            let message = t!("plugin-done", name = name, command = command.title.as_str());
            if outcome.output.is_empty() {
                self.notify(NoticeKind::Done, message);
            } else {
                self.notify_with(
                    NoticeKind::Done,
                    message,
                    NoticeAction::ShowOutput(outcome.output),
                );
            }
        } else {
            let message = t!(
                "plugin-failed",
                name = name,
                error = outcome.output.as_str()
            );
            self.notify(NoticeKind::Error, message);
        }
    }
}
