//! 设置的分类：分类栏中列出的页，以及各页的标题和第一项。

use winct_i18n::t;

use super::{Listed, Setting};

/// 设置的一个分类，对应左侧分类栏中的一项和右侧的一页。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(dead_code)]
pub enum SettingsPage {
    /// 外观：主题、界面字体和字号。
    #[default]
    Appearance,
    /// 编辑器：编辑区字体和字号、Tab 宽度、保存与滚动相关的选项。
    Editor,
    /// 格式化：保存时的格式化和修复，以及各语言的工具命令。
    Format,
    /// Git：Git 模块的选项；模块关闭时不列出。
    Git,
    /// Language Server Protocol servers.
    Lsp,
    /// 终端：默认 shell、面板位置、字体、字号、字重和终端行为。
    Terminal,
    /// 模块：可选模块的开关。
    Modules,
    /// 插件：声明式外部命令的启用、信任和设置。
    Plugins,
    /// 快捷键：只列出键位，不含设置项，整页由 `KeysLayout` 单独布局。
    Keys,
    /// 关于：版本和应用内更新。
    About,
    /// Upstream-merged general page (kept for compatibility).
    General,
    /// Upstream-merged files page (kept for compatibility).
    Files,
    /// Upstream-merged markdown page (kept for compatibility).
    Markdown,
}

/// 各可选模块是否开启：决定分类栏列出哪些页。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Modules {
    pub git: bool,
    pub terminal: bool,
}

impl SettingsPage {
    /// 全部分类，顺序与分类栏从上到下的顺序一致。
    pub const ALL: [SettingsPage; 11] = [
        SettingsPage::General,
        SettingsPage::Appearance,
        SettingsPage::Editor,
        SettingsPage::Format,
        SettingsPage::Git,
        SettingsPage::Lsp,
        SettingsPage::Plugins,
        SettingsPage::Terminal,
        SettingsPage::Modules,
        SettingsPage::Keys,
        SettingsPage::About,
    ];

    /// 返回分类栏列出的分类，顺序同 [`Self::ALL`]；模块关闭时不列出它的那一页。
    pub fn listed(modules: Modules) -> impl Iterator<Item = SettingsPage> {
        Self::ALL.into_iter().filter(move |&page| match page {
            SettingsPage::Git => modules.git,
            SettingsPage::Terminal => modules.terminal,
            _ => true,
        })
    }

    /// 返回分类的标题。
    pub fn title(self) -> &'static str {
        match self {
            SettingsPage::Appearance => t!("settings-page-appearance"),
            SettingsPage::Editor => t!("settings-page-editor"),
            SettingsPage::Format => t!("settings-page-format"),
            SettingsPage::Git => t!("settings-page-git"),
            SettingsPage::Lsp => t!("settings-page-lsp"),
            SettingsPage::Plugins => t!("settings-page-plugins"),
            SettingsPage::Terminal => t!("settings-page-terminal"),
            SettingsPage::Modules => t!("settings-page-modules"),
            SettingsPage::Keys => t!("settings-page-keys"),
            SettingsPage::About => t!("settings-page-about"),
            SettingsPage::General => t!("settings-page-general"),
            SettingsPage::Files => t!("settings-page-files"),
            SettingsPage::Markdown => t!("settings-page-markdown"),
        }
    }

    /// 返回这一页的第一项；切换到这一页时选中第一项。
    pub fn first(self) -> Setting {
        match self {
            SettingsPage::Keys => Setting::KeySearch,
            page => page.sections(&Listed::default())[0].items[0],
        }
    }
}
