//! 设置页的内容：每页包含哪些设置项和分组。

use winct_i18n::t;

use super::*;

impl SettingsPage {
    /// 返回这一页的各组设置项，组内的项从上到下排列。
    ///
    /// 格式化页中每种语言占一行，`listed` 是要列出的语言。
    pub fn sections(self, listed: &Listed) -> Vec<Section> {
        let plain = |items: Vec<Setting>| Section { title: None, items };
        match self {
            SettingsPage::General => vec![
                Section {
                    title: Some(t!("settings-group-language")),
                    items: vec![Setting::Language],
                },
                Section {
                    title: Some(t!("settings-group-settings-file")),
                    items: vec![Setting::ExportSettings, Setting::ImportSettings],
                },
            ],
            SettingsPage::Appearance => {
                // 系统标题栏只在其他平台列出：macOS 上标题栏总是和内容融为一体。平滑滚动只在
                // macOS 上列出：其他平台上滚轮本来就一格一格立即滚动。
                let mut window = vec![Setting::StatusBar, Setting::Animations];
                if cfg!(target_os = "macos") {
                    window.push(Setting::SmoothScrolling);
                }
                window.push(Setting::RestoreWindow);
                if !cfg!(target_os = "macos") {
                    window.push(Setting::SystemTitleBar);
                }
                vec![
                    plain(vec![Setting::Theme]),
                    plain(vec![Setting::Zoom]),
                    plain(vec![Setting::UiFont, Setting::UiSize, Setting::FontWeight]),
                    plain(window),
                    plain(vec![
                        Setting::BackgroundImage,
                        Setting::BackgroundFit,
                        Setting::BackgroundOpacity,
                        Setting::BackgroundBlur,
                    ]),
                    Section {
                        title: Some(t!("settings-appearance-custom")),
                        items: vec![
                            Setting::PanelOpacity,
                            Setting::AccentColor,
                            Setting::CornerScale,
                            Setting::AnimationSpeed,
                        ],
                    },
                    Section {
                        title: Some(t!("settings-light-theme")),
                        items: vec![Setting::LightTheme],
                    },
                    Section {
                        title: Some(t!("settings-dark-theme")),
                        items: vec![Setting::DarkTheme],
                    },
                ]
            }
            SettingsPage::Editor => vec![
                plain(vec![
                    Setting::EditorFont,
                    Setting::EditorSize,
                    Setting::TabWidth,
                ]),
                plain(vec![
                    Setting::LineNumbers,
                    Setting::WordWrap,
                    Setting::WholeLines,
                    Setting::Minimap,
                    Setting::StickyScroll,
                    Setting::SmoothCaret,
                    Setting::RenderWhitespace,
                ]),
                plain(vec![Setting::Outline, Setting::CompactFolders]),
                plain(vec![
                    Setting::RemoteImages,
                    Setting::TextWidth,
                    Setting::MarkdownLineNumbers,
                    Setting::MarkdownShortcuts,
                    Setting::MarkdownFocus,
                    Setting::MarkdownTypewriter,
                    Setting::MarkdownSelectionToolbar,
                ]),
                plain(vec![Setting::LatexPath]),
                plain(vec![
                    Setting::VimMode,
                    Setting::VimClipboard,
                    Setting::VimRelativeNumber,
                    Setting::VimYankHighlight,
                ]),
                plain(
                    listed
                        .vim_keys
                        .iter()
                        .copied()
                        .map(Setting::VimKey)
                        .collect(),
                ),
                plain(vec![
                    Setting::AutoSave,
                    Setting::AutoSaveTyping,
                    Setting::AutoSaveDelay,
                    Setting::HotExit,
                    Setting::ConfirmClose,
                    Setting::QuitOnLastWindowClose,
                    Setting::RestoreFolder,
                    Setting::DragAndDrop,
                    Setting::FullTabNames,
                    Setting::DropFolder,
                    Setting::PreviewOnClick,
                    Setting::FindInFiles,
                ]),
            ],
            SettingsPage::Terminal => {
                let mut behavior =
                    vec![Setting::TerminalCopyOnSelect, Setting::TerminalConfirmClose];
                // 其他平台的 Alt 一律视为 Meta，不列出该项。
                if cfg!(target_os = "macos") {
                    behavior.push(Setting::TerminalOptionAsMeta);
                }
                vec![
                    plain(vec![
                        Setting::TerminalShell,
                        Setting::TerminalPrompt,
                        Setting::TerminalShellIntegration,
                        Setting::TerminalApplicationPriority,
                    ]),
                    plain(vec![
                        Setting::TerminalPosition,
                        Setting::TerminalTabPosition,
                        Setting::TerminalFont,
                        Setting::TerminalSize,
                        Setting::TerminalFontWeight,
                    ]),
                    plain(behavior),
                ]
            }
            SettingsPage::Git => vec![
                plain(vec![Setting::GitPath, Setting::GitOpenWorktree]),
                plain(vec![Setting::GitStagingArea, Setting::GitTree]),
                plain(vec![
                    Setting::GitCombinedDiff,
                    Setting::DiffInline,
                    Setting::DiffCollapse,
                    Setting::GitInlineBlame,
                ]),
                plain(vec![
                    Setting::GitPullMode,
                    Setting::GitConfirmPush,
                    Setting::GitRememberCredentials,
                    Setting::GitAutoFetch,
                ]),
            ],
            SettingsPage::Lsp => vec![
                Section {
                    title: Some(t!("settings-lsp-detection")),
                    items: vec![Setting::LspDetect, Setting::LspDetectDirs],
                },
                Section {
                    title: Some(t!("settings-lsp-servers")),
                    items: listed
                        .lsp
                        .iter()
                        .flat_map(|&id| {
                            [
                                Setting::LspServer(id),
                                Setting::LspAuto(id),
                                Setting::LspServerEnabled(id),
                                Setting::LspServerEnv(id),
                                Setting::LspServerCwd(id),
                                Setting::LspDetectNames(id),
                            ]
                        })
                        .chain([Setting::AddLspLanguage])
                        .collect(),
                },
            ],
            SettingsPage::Plugins => {
                let mut sections = vec![plain(vec![Setting::PluginPaths, Setting::ReloadPlugins])];
                for plugin in &listed.plugins {
                    let mut items = vec![Setting::PluginEnabled(plugin.name)];
                    for &(command, title) in &plugin.commands {
                        items.push(Setting::PluginCommand(command, title));
                    }
                    for &(key, title, kind) in &plugin.settings {
                        items.push(match kind {
                            winct_config::PluginSettingKind::Bool => {
                                Setting::PluginBool(plugin.id, key, title)
                            }
                            winct_config::PluginSettingKind::String => {
                                Setting::PluginValue(plugin.id, key, title)
                            }
                        });
                    }
                    sections.push(Section {
                        title: Some(plugin.name),
                        items,
                    });
                }
                sections
            }
            SettingsPage::Modules => vec![plain(vec![
                Setting::GitModule,
                Setting::TerminalModule,
                Setting::RemoteModule,
            ])],
            SettingsPage::Format => {
                let mut sections = vec![
                    plain(vec![
                        Setting::FormatOnSave,
                        Setting::FixOnSave,
                        Setting::SkipOnSave,
                    ]),
                    Section {
                        title: Some(t!("settings-section-format-tools")),
                        items: listed
                            .format
                            .iter()
                            .map(|&id| Setting::FormatCommand(id))
                            .chain([Setting::AddLanguage])
                            .collect(),
                    },
                ];
                // 没有列出的语言时只有「添加语言…」，直接跟在组标题下面。
                sections.push(Section {
                    title: Some(t!("settings-section-fix-tools")),
                    items: listed
                        .fix
                        .iter()
                        .map(|&id| Setting::FixCommand(id))
                        .chain([Setting::AddFixLanguage])
                        .collect(),
                });
                sections
            }
            SettingsPage::Keys => Vec::new(),
            SettingsPage::About => vec![
                plain(vec![Setting::Version]),
                plain(vec![
                    Setting::Updates,
                    Setting::CheckUpdates,
                    Setting::SystemProxy,
                ]),
                plain(vec![
                    Setting::ReleaseNotes,
                    Setting::Website,
                    Setting::Repository,
                    Setting::Community,
                    Setting::License,
                ]),
            ],
            _ => vec![],
        }
    }
}

/// 返回搜索 `query` 时正文列出的各组：每页中名称或说明含有 `query` 的设置项
/// （[`Setting::matches`]）合为一组，以页名为标题；页名或组标题含有 `query` 时是该页或该组的
/// 全部设置项。
///
/// 各组按页的顺序排列，组内按页内的顺序；组下方的文字按钮不参与搜索。「快捷键」页列出键位方案
/// （[`Setting::Keymap`]），以及有命令的名称或按键与 `query` 相符时的 [`Setting::KeyMatches`]。
/// `modules` 为各模块是否开启（见 [`SettingsPage::listed`]）。
pub fn search_sections(query: &str, listed: &Listed, modules: Modules) -> Vec<Section> {
    let query = query.trim().to_lowercase();
    let query = query.as_str();
    let named = move |title: &str| title.to_lowercase().contains(query);
    let found = SettingsPage::listed(modules).map(|page| {
        let whole = named(page.title());
        let items: Vec<Setting> = if page == SettingsPage::Keys {
            let keymap = (whole || Setting::Keymap.matches(query)).then_some(Setting::Keymap);
            let commands = (crate::keymap::count_matches(query) > 0).then_some(Setting::KeyMatches);
            keymap.into_iter().chain(commands).collect()
        } else {
            let items = page.sections(listed).into_iter().flat_map(|section| {
                let whole = whole || section.title.is_some_and(named);
                (section.items.into_iter())
                    .filter(|s| s.control() != SettingControl::Action)
                    .filter(move |s| whole || s.matches(query))
            });
            items.collect()
        };
        Section {
            title: Some(page.title()),
            items,
        }
    });
    let found: Vec<Section> = found.filter(|s| !s.items.is_empty()).collect();
    found
}
