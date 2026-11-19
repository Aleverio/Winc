//! 直接填写的设置项（路径、文件名）在配置中的读写。

use crate::app::App;
use crate::layout::Setting;

impl App {
    /// 直接填写的设置项（见 [`Setting::is_text`]）在配置中的值。
    pub(in crate::app) fn text_setting(&self, setting: Setting) -> &Option<String> {
        match setting {
            Setting::MarkdownImageFolder => &self.config.markdown_image_folder,
            Setting::MarkdownImageName => &self.config.markdown_image_name,
            _ => &self.config.git_path,
        }
    }

    /// 写回直接填写的设置项 `setting`（`Git 程序` 之外的），`value` 为 `None` 时用默认值。
    pub(super) fn set_text_setting(&mut self, setting: Setting, value: Option<String>) {
        let slot = match setting {
            Setting::MarkdownImageFolder => &mut self.config.markdown_image_folder,
            Setting::MarkdownImageName => &mut self.config.markdown_image_name,
            _ => return,
        };
        if *slot != value {
            *slot = value;
            self.save_config();
            self.request_redraw();
        }
    }
}
