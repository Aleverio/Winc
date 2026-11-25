//! 设置页中文字输入框的编辑、字体选择和界面缩放。

use super::*;

impl App {
    /// 开始编辑文字或数值设置：输入框中是当前的设置并全选，直接键入即替换。
    pub(in crate::app) fn settings_start_editing(&mut self, setting: Setting) {
        if matches!(&self.settings.editing, Some((s, _)) if *s == setting) {
            self.focus = Focus::Setting;
            return;
        }

        self.settings_leave_field();

        let current = if setting == Setting::AutoSaveDelay {
            Some(
                self.config
                    .auto_save_delay_ms
                    .unwrap_or(Config::AUTO_SAVE_DELAY_DEFAULT_MS)
                    .to_string(),
            )
        } else {
            match tool_setting(setting) {
                Some((task, id)) => task_commands(&self.config, task).get(id).cloned(),
                None if let Setting::LspServer(id) = setting => {
                    lsp::display_configured_lsp_command(&self.config, id)
                }
                None if setting == Setting::LspDetectDirs => {
                    Some(self.config.lsp_detect_dirs.join(", "))
                }
                None if let Setting::LspServerEnv(id) = setting => self
                    .config
                    .lsp_server_entries
                    .get(id)
                    .and_then(|entries| entries.first())
                    .map(|entry| {
                        entry
                            .env
                            .iter()
                            .map(|(key, value)| format!("{key}={value}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    }),
                None if let Setting::LspServerCwd(id) = setting => self
                    .config
                    .lsp_server_entries
                    .get(id)
                    .and_then(|entries| entries.first())
                    .and_then(|entry| entry.cwd.clone()),
                None if let Setting::LspDetectNames(id) = setting => self
                    .config
                    .lsp_detect_names
                    .get(id)
                    .map(|names| names.join(", ")),
                None if setting == Setting::PluginPaths => {
                    Some(self.config.plugin_paths.join(", "))
                }
                None if let Setting::PluginValue(plugin, key, _) = setting => {
                    Some(self.plugin_setting_value(plugin, key))
                }
                None if setting == Setting::AccentColor => self.config.ui_accent.clone(),
                None if setting == Setting::TerminalShell => self.config.terminal_shell.clone(),
                None if setting == Setting::GitPath => self.config.git_path.clone(),
                None if setting == Setting::LatexPath => self.config.latex_path.clone(),
                None => self.font_setting(setting).clone(),
            }
        };
        let mut field = TextField::default();
        field.set_text(current.as_deref().unwrap_or(""));
        field.select_all();

        self.settings.selected = setting;
        self.settings.editing = Some((setting, field));
        self.settings.editing_original = current;
        self.settings.error = None;
        self.focus = Focus::Setting;
        self.reset_blink();
        self.request_redraw();
    }

    /// 返回 `page` 上配置了但用不了的字体，以及原因；没有时返回 `None`。
    ///
    /// 这样的字体在启动时已改用默认字体，但设置中仍是原来的名字，因此在该行写明原因。
    pub(super) fn broken_font(&self, page: SettingsPage) -> Option<(Setting, String)> {
        let fonts = [
            Setting::EditorFont,
            Setting::UiFont,
            Setting::TerminalFont,
            Setting::MarkdownFont,
        ];
        fonts
            .into_iter()
            .filter(|setting| setting.page() == page)
            .find_map(|setting| font_error(self.font_value(setting)).map(|error| (setting, error)))
    }

    /// 字体设置项 `setting` 在配置中的值：以逗号分隔的字体列表，未设置时为 `None`。
    pub(in crate::app) fn font_value(&self, setting: Setting) -> &Option<String> {
        match setting {
            Setting::UiFont => &self.config.ui_font,
            Setting::TerminalFont => &self.config.terminal_font,
            Setting::MarkdownFont => &self.config.markdown_font,
            _ => &self.config.editor_font,
        }
    }

    fn font_setting(&mut self, setting: Setting) -> &mut Option<String> {
        match setting {
            Setting::UiFont => &mut self.config.ui_font,
            Setting::TerminalFont => &mut self.config.terminal_font,
            Setting::MarkdownFont => &mut self.config.markdown_font,
            _ => &mut self.config.editor_font,
        }
    }

    /// 离开正在编辑的输入框：先提交；字体名不对时放弃本次输入，字体设置退回开始编辑前的值，
    /// 出错原因仍显示在该行。
    ///
    /// 换页、点击其他控件、按 `Tab` 时调用，不会因为名字不对而把用户困在输入框中；只有按 `↩`
    /// 提交时，名字不对才停留在输入框里。
    pub(in crate::app) fn settings_leave_field(&mut self) {
        if self.settings.editing.is_none() {
            return;
        }
        self.settings_commit_field();
        let error = self.settings.error.take();
        self.settings_cancel_field();
        self.settings.error = error;
    }

    /// 键入时只更新配置中的值，不改变正在使用的字体。
    ///
    /// 键入到一半的名字（例如 `Menl`）原本就找不到，每敲一个字符就换一次字体会使界面反复变化。
    /// 回车提交时才真正换字体。
    pub(in crate::app) fn settings_live_font(&mut self) {
        let Some((setting, field)) = self.settings.editing.as_ref() else {
            return;
        };

        let setting = *setting;
        if setting.control() != SettingControl::Font {
            return;
        }
        let text = field.text().trim().to_owned();
        *self.font_setting(setting) = (!text.is_empty()).then_some(text);
        self.settings.error = None;
    }

    /// 放弃编辑：字体设置退回开始编辑前的值（键入过程中配置已被改为中间状态）。
    ///
    /// 格式化命令在键入时不改配置，因此不需要退回。
    pub(in crate::app) fn settings_cancel_field(&mut self) {
        let Some((setting, _)) = self.settings.editing.take() else {
            return;
        };

        let original = self.settings.editing_original.take();
        if setting.control() == SettingControl::Font {
            *self.font_setting(setting) = original;
        }
        self.settings.error = None;

        if self.focus == Focus::Setting {
            self.focus = Focus::Settings;
        }

        self.request_redraw();
    }

    /// 提交正在编辑的字体名、命令或数值。
    pub(in crate::app) fn settings_commit_field(&mut self) {
        let Some((setting, field)) = self.settings.editing.as_ref() else {
            return;
        };
        let (setting, text) = (*setting, field.text().trim().to_owned());
        let value = (!text.is_empty()).then_some(text);

        if setting == Setting::AutoSaveDelay {
            let delay = match value.and_then(|value| value.parse::<u32>().ok()) {
                Some(delay)
                    if (Config::AUTO_SAVE_DELAY_RANGE.start().round() as u32
                        ..=Config::AUTO_SAVE_DELAY_RANGE.end().round() as u32)
                        .contains(&delay) =>
                {
                    delay
                }
                _ => {
                    self.settings.error =
                        Some((setting, t!("settings-auto-save-delay-invalid").to_owned()));
                    self.request_redraw();
                    return;
                }
            };
            self.config.auto_save_delay_ms = Some(delay);
            self.settings.editing = None;
            self.settings.editing_original = None;
            self.settings.error = None;
            self.focus = Focus::Settings;
            self.save_config();
            self.schedule_dirty_auto_saves();
            self.request_redraw();
            return;
        }

        match tool_setting(setting) {
            Some((task, id)) => self.settings_commit_command(task, id, value),
            None if let Setting::LspServer(id) = setting => {
                self.settings_commit_lsp_server(setting, id, value)
            }
            None if setting == Setting::LspDetectDirs => {
                self.settings_commit_lsp_detect_dirs(setting, value)
            }
            None if let Setting::LspServerEnv(id) = setting => {
                self.settings_commit_lsp_env(id, value)
            }
            None if let Setting::LspServerCwd(id) = setting => {
                self.settings_commit_lsp_cwd(id, value)
            }
            None if let Setting::LspDetectNames(id) = setting => {
                self.settings_commit_lsp_detect_names(id, value)
            }
            None if setting == Setting::PluginPaths => self.settings_commit_plugin_paths(value),
            None if let Setting::PluginValue(plugin, key, _) = setting => {
                self.settings_commit_plugin_setting(plugin, key, value)
            }
            None if self.commit_accent(setting, value.as_deref()) => {}
            None if setting == Setting::TerminalShell => {
                self.settings.editing = None;
                self.settings.editing_original = None;
                self.settings.error = None;
                self.focus = Focus::Settings;
                self.set_terminal_shell(value);
            }
            None if setting == Setting::GitPath => {
                self.settings.editing = None;
                self.settings.editing_original = None;
                self.settings.error = None;
                self.focus = Focus::Settings;
                self.set_git_path(value);
            }
            None if setting == Setting::LatexPath => {
                self.settings.editing = None;
                self.settings.editing_original = None;
                self.settings.error = None;
                self.focus = Focus::Settings;
                self.set_latex_path(value);
            }
            None => self.settings_commit_font(setting, value),
        }
    }

    /// 提交格式化（修复）命令并写回配置；`command` 为 `None`（清空）表示自动选择。
    ///
    /// 命令对应的程序找不到时同样保存（工具可能只装在其他项目里，或稍后才安装），
    /// 提醒显示在名字下方。
    pub(in crate::app) fn settings_commit_command(
        &mut self,
        task: Task,
        id: &'static str,
        command: Option<String>,
    ) {
        let commands = task_commands_mut(&mut self.config, task);
        match command {
            Some(command) => commands.insert(id.to_owned(), command),
            None => commands.remove(id),
        };
        self.settings.editing = None;
        self.settings.editing_original = None;
        self.settings.error = None;
        self.focus = Focus::Settings;
        self.save_config();
        self.refresh_format_rows(false);
        self.request_redraw();
    }

    /// 提交字体名：立刻换字体并写回配置。
    ///
    /// 名字无法识别就留在输入框里，配置退回原值。
    ///
    /// 清空表示使用平台默认字体，这是允许的：配置中留空即默认，输入框中显示「系统默认」。
    pub(super) fn settings_commit_font(&mut self, setting: Setting, value: Option<String>) {
        // 无法识别的字体名要在这里拦截：渲染层找不到字体时会静默回退到默认字体，
        // 用户会以为设置已生效，但画面没有变化。
        if let Some(message) = font_error(&value) {
            self.settings.error = Some((setting, message));
            self.request_redraw();
            return;
        }

        let original = self.settings.editing_original.clone();
        *self.font_setting(setting) = value;

        match self.apply_fonts() {
            Ok(()) => {
                self.settings.editing = None;
                self.settings.editing_original = None;
                self.settings.error = None;
                self.focus = Focus::Settings;
                self.save_config();
            }
            Err(message) => {
                // 无法应用时退回原值，配置中不保留无法使用的字体。
                *self.font_setting(setting) = original;
                self.settings.error = Some((setting, message));
            }
        }

        self.request_redraw();
    }

    /// 应用字体菜单中选中的字体；`None` 表示系统默认。
    ///
    /// 字体不能使用时配置保持原值，原因显示在该行。
    pub(in crate::app) fn settings_choose_font(&mut self, setting: Setting, value: Option<String>) {
        self.settings.editing_original = self.font_setting(setting).clone();
        self.settings_commit_font(setting, value);
        self.settings.editing_original = None;
    }

    /// 字体菜单中选中字体 `name`：换掉列表中的第一个字体，其后的后备字体保留（`name` 原在其中
    /// 时从中移去）。
    pub(in crate::app) fn settings_choose_primary_font(&mut self, setting: Setting, name: &str) {
        let current = self.font_value(setting).clone();
        let list = winct_config::font_list(current.as_deref());
        let rest = list.into_iter().skip(1).filter(|&n| n != name);
        let list: Vec<&str> = std::iter::once(name).chain(rest).collect();
        self.settings_choose_font(setting, Some(list.join(", ")));
    }

    /// 「后备字体」中点击字体 `name`：不在列表中时加到末尾，已在其中时移去。第一个字体不变。
    pub(in crate::app) fn settings_toggle_fallback_font(&mut self, setting: Setting, name: &str) {
        let current = self.font_value(setting).clone();
        let mut list = winct_config::font_list(current.as_deref());
        if list.first().is_none_or(|&first| first == name) {
            return;
        }
        match list.iter().position(|&n| n == name) {
            Some(at) => _ = list.remove(at),
            None => list.push(name),
        }
        self.settings_choose_font(setting, Some(list.join(", ")));
    }

    /// 放大或缩小字号（`⌘+` `⌘−`），`⌘0` 还原（`up` 为 `None`）。
    ///
    /// 焦点在终端时调整终端字号，否则调整编辑区字号，两者互不影响；还原即回到默认字号。改动
    /// 写回配置，界面字号不变。
    pub(in crate::app) fn step_font_size(&mut self, up: Option<bool>) {
        // 当前标签是 PDF 或图片时缩放预览。
        if !self.focus.in_terminal() && (self.zoom_pdf(up, None) || self.zoom_picture(up, None)) {
            return;
        }
        self.step_font_size_in(self.focus.in_terminal(), up);
    }

    /// 同 [`Self::step_font_size`]，由 `terminal` 指定调整终端还是编辑区的字号。
    pub(in crate::app) fn step_font_size_in(&mut self, terminal: bool, up: Option<bool>) {
        let pick = |fonts: ViewFonts| if terminal { fonts.terminal } else { fonts.code };
        let current = pick(ViewFonts::of(&self.config));
        let next = up.map(|up| step(current, up, Config::EDITOR_SIZE_RANGE));
        if next == Some(current) {
            let size = current.to_string();
            self.flash(if terminal {
                t!("settings-terminal-font-size-limit", size = size)
            } else {
                t!("settings-font-size-limit", size = size)
            });
            return;
        }
        if terminal {
            self.config.terminal_size = next;
        } else {
            self.config.editor_size = next;
        }

        let size = pick(ViewFonts::of(&self.config)).to_string();
        self.apply_font_sizes();
        self.save_config();
        self.flash(if terminal {
            t!("settings-terminal-font-size", size = size)
        } else {
            t!("settings-font-size", size = size)
        });
    }

    /// 按档位（[`ZOOM_STEPS`]）放大或缩小界面（`up` 为 `true` 时放大），`up` 为 `None` 时还原为
    /// 100%。
    ///
    /// 配置文件中写了档位以外的值时，移到该方向上最近的档位。改动写回配置。
    pub(in crate::app) fn step_zoom(&mut self, up: Option<bool>) {
        let current = self.config.ui_zoom.unwrap_or(100);
        let next = match up {
            Some(true) => ZOOM_STEPS.into_iter().find(|&zoom| zoom > current),
            Some(false) => ZOOM_STEPS.into_iter().rev().find(|&zoom| zoom < current),
            None => Some(100),
        };
        let Some(next) = next else {
            self.flash(t!("settings-zoom-limit", zoom = current.to_string()));
            return;
        };
        self.set_zoom(next);
        self.flash(t!("settings-zoom", zoom = next.to_string()));
    }

    /// 把界面缩放设为 `zoom`（百分比）并写回配置；100% 时从配置中去掉该项。
    pub(in crate::app) fn set_zoom(&mut self, zoom: u16) {
        self.config.ui_zoom = (zoom != 100).then_some(zoom);
        self.apply_zoom();
        self.save_config();
    }
}
