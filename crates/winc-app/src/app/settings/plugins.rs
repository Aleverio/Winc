//! 插件页：从本机清单刷新插件列表，并把设置行的值读写到配置里。

use winc_config::{PluginRegistry, PluginSettingKind, intern};
use winc_i18n::t;
use winc_plugin::api::Capability;

use crate::layout::PluginRow;

use super::*;

/// 权限开关在插件设置项里的伪键前缀；不会写进普通插件值表。
const CAPABILITY_PREFIX: &str = "@capability:";

fn capability_key(name: &str) -> String {
    format!("{CAPABILITY_PREFIX}{name}")
}

fn capability_from_key(key: &str) -> Option<&str> {
    key.strip_prefix(CAPABILITY_PREFIX)
}

impl App {
    /// 重新读取清单并刷新插件页列出的行。
    ///
    /// 打开设置或改过清单路径后调用；清单里的文字通过 [`intern`] 转成 `'static`，供 [`Setting`]
    /// 的名称和命令面板的标题共用。
    pub(in crate::app) fn refresh_plugins(&mut self) {
        self.plugins = PluginRegistry::default();
        self.settings.languages.plugins.clear();
        if self.config.plugin_paths.is_empty() {
            return;
        }
        let registry = winc_config::load_plugins(&self.config.plugin_paths);
        for plugin in &registry.plugins {
            let commands = plugin
                .commands
                .iter()
                .map(|command| {
                    (
                        intern(&PluginRegistry::command_id(plugin, command)),
                        intern(&command.title),
                    )
                })
                .collect();
            let settings = plugin
                .settings
                .iter()
                .map(|setting| (intern(&setting.key), intern(&setting.title), setting.kind))
                .collect();
            self.settings.languages.plugins.push(PluginRow {
                id: intern(&plugin.id),
                name: intern(&plugin.name),
                commands,
                settings,
            });
            if let Some(wasm) = &plugin.wasm {
                let row = self
                    .settings
                    .languages
                    .plugins
                    .last_mut()
                    .expect("row was pushed");
                for name in &wasm.capabilities {
                    let Some(capability) = Capability::parse(name) else {
                        continue;
                    };
                    let key = intern(&capability_key(capability.as_str()));
                    let title = intern(&t!("plugin-capability", name = capability.as_str()));
                    row.settings.push((key, title, PluginSettingKind::Bool));
                }
            }
        }
        if !registry.problems.is_empty() {
            let detail = registry.problems.join("; ");
            self.notify(
                crate::app::NoticeKind::Warning,
                t!("plugin-manifest-problem", detail = detail.as_str()),
            );
        }
        self.plugins = registry;
        self.reload_wasm_plugins();
        let extra_commands: Vec<(String, String, String)> = self
            .wasm
            .commands()
            .iter()
            .map(|command| {
                (
                    command.plugin.clone(),
                    format!("{}.{}", command.plugin, command.id),
                    command.title.clone(),
                )
            })
            .collect();
        let extra_settings: Vec<(String, String, String, winc_config::PluginSettingKind)> = self
            .wasm
            .settings()
            .iter()
            .map(|setting| {
                (
                    setting.plugin.clone(),
                    setting.key.clone(),
                    setting.title.clone(),
                    setting.kind,
                )
            })
            .collect();
        for row in &mut self.settings.languages.plugins {
            for (plugin, id, title) in &extra_commands {
                if row.id == plugin {
                    row.commands.push((intern(id), intern(title)));
                }
            }
            for (plugin, key, title, kind) in &extra_settings {
                if row.id == plugin {
                    row.settings.push((intern(key), intern(title), *kind));
                }
            }
        }
        if !self.wasm.errors().is_empty() {
            let detail = self.wasm.errors().join("; ");
            self.notify(
                crate::app::NoticeKind::Warning,
                t!("plugin-manifest-problem", detail = detail.as_str()),
            );
        }
    }

    /// 提交插件清单路径：按逗号和换行分开，重新读取清单并刷新本页。
    pub(super) fn settings_commit_plugin_paths(&mut self, value: Option<String>) {
        let mut paths: Vec<String> = Vec::new();
        if let Some(text) = value {
            for path in text
                .split([',', '\n'])
                .map(str::trim)
                .filter(|path| !path.is_empty())
            {
                if !paths.iter().any(|known| known.as_str() == path) {
                    paths.push(path.to_owned());
                }
            }
        }
        self.config.plugin_paths = paths;
        self.settings.editing = None;
        self.settings.editing_original = None;
        self.settings.error = None;
        self.focus = Focus::Settings;
        self.refresh_plugins();
        self.save_config();
        self.request_redraw();
    }

    /// 提交文本类型的插件设置项：空值表示恢复清单里的默认值。
    pub(super) fn settings_commit_plugin_setting(
        &mut self,
        plugin: &'static str,
        key: &'static str,
        value: Option<String>,
    ) {
        let default = self.plugin_setting_default(plugin, key);
        self.write_plugin_value(plugin, key, value.filter(|text| text != &default));
        self.settings.editing = None;
        self.settings.editing_original = None;
        self.settings.error = None;
        self.focus = Focus::Settings;
    }

    /// 插件设置行当前显示的值；没有保存过时使用清单里的默认值。
    pub(crate) fn plugin_setting_value(&self, plugin: &'static str, key: &'static str) -> String {
        self.config
            .plugin_values
            .get(plugin)
            .and_then(|values| values.get(key))
            .cloned()
            .unwrap_or_else(|| self.plugin_setting_default(plugin, key))
    }

    /// 布尔插件设置项当前的开关状态。
    pub(crate) fn plugin_bool_value(&self, plugin: &'static str, key: &'static str) -> bool {
        if let Some(name) = capability_from_key(key) {
            return self
                .config
                .plugin_grants
                .get(plugin)
                .is_some_and(|granted| granted.iter().any(|item| item == name));
        }
        self.plugin_setting_value(plugin, key) == "true"
    }

    /// 切换一个布尔插件设置项，并写回配置。
    pub(crate) fn set_plugin_bool_value(
        &mut self,
        plugin: &'static str,
        key: &'static str,
        on: bool,
    ) {
        let value = on.to_string();
        let default = self.plugin_setting_default(plugin, key);
        self.write_plugin_value(plugin, key, (value != default).then_some(value));
    }

    /// 清单里声明的默认值；找不到设置项时是空字符串。
    fn plugin_setting_default(&self, plugin: &str, key: &str) -> String {
        if capability_from_key(key).is_some() {
            return "false".to_owned();
        }
        self.plugins
            .plugins
            .iter()
            .find(|manifest| manifest.id == plugin)
            .and_then(|manifest| manifest.settings.iter().find(|item| item.key == key))
            .map(|item| item.default.clone())
            .or_else(|| {
                self.wasm
                    .settings()
                    .iter()
                    .find(|item| item.plugin == plugin && item.key == key)
                    .map(|item| item.default.clone())
            })
            .unwrap_or_default()
    }

    /// 写入一个插件设置项；`None` 表示恢复默认值，同时清掉空的插件表。
    fn write_plugin_value(&mut self, plugin: &str, key: &str, value: Option<String>) {
        if let Some(name) = capability_from_key(key) {
            self.set_plugin_capability(plugin, name, value.as_deref() == Some("true"));
            return;
        }
        match value {
            Some(value) => {
                self.config
                    .plugin_values
                    .entry(plugin.to_owned())
                    .or_default()
                    .insert(key.to_owned(), value);
            }
            None => {
                let empty = self
                    .config
                    .plugin_values
                    .get_mut(plugin)
                    .is_some_and(|values| {
                        values.remove(key);
                        values.is_empty()
                    });
                if empty {
                    self.config.plugin_values.remove(plugin);
                }
            }
        }
        self.save_config();
        self.request_redraw();
    }

    /// 批准或撤销一个插件 capability，并重新加载沙箱实例。
    fn set_plugin_capability(&mut self, plugin: &str, capability: &str, allowed: bool) {
        let entry = self
            .config
            .plugin_grants
            .entry(plugin.to_owned())
            .or_default();
        if allowed {
            if !entry.iter().any(|item| item == capability) {
                entry.push(capability.to_owned());
                entry.sort_unstable();
            }
        } else {
            entry.retain(|item| item != capability);
            if entry.is_empty() {
                self.config.plugin_grants.remove(plugin);
            }
        }
        self.save_config();
        self.reload_wasm_plugins();
        self.request_redraw();
    }
}
