//! LSP 设置：列出语言、打开 LSP 设置页、编辑 server 命令、自动查找选项。

use super::*;
use std::collections::BTreeMap;
use winct_config::LspServerEntry;

/// 返回某语言当前配置的服务器命令，供设置输入框预填。
pub(crate) fn display_configured_lsp_command(
    config: &winct_config::Config,
    language: &str,
) -> Option<String> {
    config
        .lsp_server_entries
        .get(language)
        .and_then(|entries| entries.first())
        .map(|entry| crate::app::lsp::display_lsp_command(&entry.command))
        .or_else(|| {
            config
                .lsp_servers
                .get(language)
                .map(|args| crate::app::lsp::display_lsp_command(args))
        })
}

impl App {
    /// 返回某语言的主服务器配置；没有时创建一个禁用的空配置。
    fn lsp_primary_entry(&mut self, language: &str) -> &mut LspServerEntry {
        let entries = self
            .config
            .lsp_server_entries
            .entry(language.to_owned())
            .or_default();
        if entries.is_empty() {
            entries.push(LspServerEntry {
                id: "configured".to_owned(),
                command: Vec::new(),
                env: BTreeMap::new(),
                cwd: None,
                enabled: true,
            });
        }
        &mut entries[0]
    }

    pub(super) fn refresh_lsp_languages(&mut self) {
        let mut languages: Vec<_> = self
            .config
            .lsp_servers
            .keys()
            .chain(self.config.lsp_server_entries.keys())
            .chain(self.config.lsp_auto.iter())
            .filter_map(|id| winct_syntax::find(id).map(|language| language.id))
            .collect();
        languages.sort_unstable();
        languages.dedup();
        if let Some(id) = self
            .editor
            .active()
            .map(|doc| doc.highlighter.language().id)
            && !languages.contains(&id)
        {
            languages.push(id);
        }
        let root = self
            .editor
            .active()
            .and_then(|doc| doc.path.as_ref())
            .and_then(|path| {
                self.editor
                    .root()
                    .filter(|root| path.starts_with(root))
                    .or_else(|| path.parent())
                    .map(Path::to_path_buf)
            })
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        self.lsp_detected.borrow_mut().clear();
        for language in &languages {
            let found = crate::app::lsp::discovery::discover(language, &root, &self.config);
            self.lsp_detected
                .borrow_mut()
                .insert(((*language).to_owned(), root.clone()), found);
        }
        self.settings.listed.lsp = languages;
    }

    pub(crate) fn open_lsp_settings(&mut self) {
        self.settings.listed.lsp.clear();
        self.refresh_lsp_languages();
        self.open_settings();
        self.settings_show_page(SettingsPage::Lsp);
        let active = self
            .editor
            .active()
            .map(|doc| doc.highlighter.language().id);
        self.settings.selected = active
            .filter(|id| self.settings.listed.lsp.contains(id))
            .map(|id| {
                if self.config.lsp_auto.iter().any(|auto| auto == id) {
                    Setting::LspAuto(id)
                } else {
                    Setting::LspServer(id)
                }
            })
            .or_else(|| {
                self.settings
                    .listed
                    .lsp
                    .first()
                    .copied()
                    .map(Setting::LspServer)
            })
            .unwrap_or(Setting::AddLspLanguage);
        self.settings_scroll_into_view();
        self.request_redraw();
    }

    pub(in crate::app) fn settings_add_lsp_language(&mut self) {
        let Some(language) = self.editor.active().map(|doc| doc.highlighter.language()) else {
            self.flash(t!("settings-lsp-open-file"));
            return;
        };
        if !self.settings.listed.lsp.contains(&language.id) {
            self.settings.listed.lsp.push(language.id);
        }
        let setting = Setting::LspServer(language.id);
        self.settings.selected = setting;
        self.settings_scroll_into_view();
        self.settings_start_editing(setting);
    }

    pub(super) fn settings_commit_lsp_server(
        &mut self,
        setting: Setting,
        language: &'static str,
        command: Option<String>,
    ) {
        let args = match command {
            Some(command) => match crate::app::lsp::split_server_command(&command) {
                Ok(args) => Some(args),
                Err(error) => {
                    self.settings.error = Some((setting, error));
                    self.request_redraw();
                    return;
                }
            },
            None => None,
        };
        match args {
            Some(args) => {
                let entry = self.lsp_primary_entry(language);
                entry.command = args;
                entry.enabled = true;
                self.config.lsp_servers.remove(language);
            }
            None => {
                self.config.lsp_servers.remove(language);
                self.config.lsp_server_entries.remove(language);
            }
        }
        self.finish_lsp_settings_change(language);
    }

    /// 保存自动查找的额外目录。
    pub(super) fn settings_commit_lsp_detect_dirs(
        &mut self,
        setting: Setting,
        value: Option<String>,
    ) {
        self.config.lsp_detect_dirs = value
            .map(|value| {
                value
                    .split([',', ';'])
                    .map(str::trim)
                    .filter(|dir| !dir.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        self.settings.editing = None;
        self.settings.editing_original = None;
        self.settings.error = None;
        self.focus = Focus::Settings;
        self.clear_lsp_detection_cache();
        self.save_config();
        self.request_redraw();
        let _ = setting;
    }

    /// 保存主服务器的额外环境变量，格式为 `KEY=value, …`。
    pub(super) fn settings_commit_lsp_env(
        &mut self,
        language: &'static str,
        value: Option<String>,
    ) {
        let mut env = BTreeMap::new();
        if let Some(value) = value {
            for pair in value.split(',') {
                if let Some((key, val)) = pair.trim().split_once('=') {
                    let (key, val) = (key.trim(), val.trim());
                    if !key.is_empty() {
                        env.insert(key.to_owned(), val.to_owned());
                    }
                }
            }
        }
        self.lsp_primary_entry(language).env = env;
        self.finish_lsp_settings_change(language);
    }

    /// 保存主服务器的工作目录。
    pub(super) fn settings_commit_lsp_cwd(
        &mut self,
        language: &'static str,
        value: Option<String>,
    ) {
        self.lsp_primary_entry(language).cwd = value;
        self.finish_lsp_settings_change(language);
    }

    /// 开关某语言的配置服务器。
    pub(super) fn settings_toggle_lsp_server_enabled(&mut self, language: &'static str) {
        let enabled = !self
            .config
            .lsp_server_entries
            .get(language)
            .and_then(|entries| entries.first())
            .is_some_and(|entry| entry.enabled);
        self.lsp_primary_entry(language).enabled = enabled;
        self.finish_lsp_settings_change(language);
    }

    /// 保存某种语言自动查找时额外尝试的程序名。
    pub(super) fn settings_commit_lsp_detect_names(
        &mut self,
        language: &'static str,
        value: Option<String>,
    ) {
        let mut names: Vec<String> = Vec::new();
        if let Some(value) = value {
            for name in value
                .split([',', ';'])
                .map(str::trim)
                .filter(|name| !name.is_empty())
            {
                if !names.iter().any(|known| known.as_str() == name) {
                    names.push(name.to_owned());
                }
            }
        }
        if names.is_empty() {
            self.config.lsp_detect_names.remove(language);
        } else {
            self.config
                .lsp_detect_names
                .insert(language.to_owned(), names);
        }
        self.settings.editing = None;
        self.settings.editing_original = None;
        self.settings.error = None;
        self.focus = Focus::Settings;
        self.clear_lsp_detection_cache();
        self.save_config();
        self.refresh_lsp_languages();
        self.request_redraw();
    }

    /// 开关自动查找。
    pub(super) fn settings_set_lsp_detect(&mut self, on: bool) {
        self.config.lsp_detect = Some(on);
        self.clear_lsp_detection_cache();
        self.save_config();
        self.request_redraw();
    }

    fn finish_lsp_settings_change(&mut self, language: &str) {
        self.settings.editing = None;
        self.settings.editing_original = None;
        self.settings.error = None;
        self.focus = Focus::Settings;
        self.clear_lsp_detection_cache();
        self.save_config();
        self.reset_lsp_server(language);
        self.request_redraw();
    }
}
