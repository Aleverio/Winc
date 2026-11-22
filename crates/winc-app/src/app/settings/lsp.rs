//! LSP 设置：列出语言、打开 LSP 设置页、编辑 server 命令。

use super::*;

impl App {
    pub(super) fn refresh_lsp_languages(&mut self) {
        let mut languages: Vec<_> = self
            .config
            .lsp_servers
            .keys()
            .chain(self.config.lsp_auto.iter())
            .filter_map(|id| winc_syntax::find(id).map(|language| language.id))
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
        self.settings.languages.lsp = languages;
    }

    pub(crate) fn open_lsp_settings(&mut self) {
        self.settings.languages.lsp.clear();
        self.refresh_lsp_languages();
        self.open_settings();
        self.settings_show_page(SettingsPage::Lsp);
        let active = self
            .editor
            .active()
            .map(|doc| doc.highlighter.language().id);
        self.settings.selected = active
            .filter(|id| self.settings.languages.lsp.contains(id))
            .map(|id| {
                if self.config.lsp_auto.iter().any(|auto| auto == id) {
                    Setting::LspAuto(id)
                } else {
                    Setting::LspServer(id)
                }
            })
            .or_else(|| {
                self.settings
                    .languages
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
        if !self.settings.languages.lsp.contains(&language.id) {
            self.settings.languages.lsp.push(language.id);
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
                self.config.lsp_servers.insert(language.to_owned(), args);
            }
            None => {
                self.config.lsp_servers.remove(language);
            }
        }
        self.settings.editing = None;
        self.settings.editing_original = None;
        self.settings.error = None;
        self.focus = Focus::Settings;
        self.save_config();
        self.reset_lsp_server(language);
        self.request_redraw();
    }
}
