//! 外观自定义设置项：背景模糊、面板透明度、圆角、动画速度和强调色的读写。

use winc_config::Config;
use winc_i18n::t;

use super::*;

impl App {
    /// 返回 Stepper 类外观项当前的值和范围；不是这些项时返回 `None`。
    pub(super) fn appearance_value(
        &self,
        setting: Setting,
    ) -> Option<(f32, std::ops::RangeInclusive<f32>)> {
        Some(match setting {
            Setting::BackgroundBlur => (
                f32::from(self.config.ui_background_blur.unwrap_or(0)),
                Config::BACKGROUND_BLUR_RANGE,
            ),
            Setting::PanelOpacity => (
                f32::from(self.config.ui_panel_opacity.unwrap_or(100)),
                Config::PANEL_OPACITY_RANGE,
            ),
            Setting::CornerScale => (
                f32::from(self.config.ui_corner_scale.unwrap_or(100)),
                Config::CORNER_SCALE_RANGE,
            ),
            Setting::AnimationSpeed => (
                f32::from(self.config.ui_animation_speed.unwrap_or(100)),
                Config::ANIMATION_SPEED_RANGE,
            ),
            _ => return None,
        })
    }

    /// 调整一个外观 Stepper 项；不是这些项时返回 `false`。
    pub(super) fn step_appearance(&mut self, setting: Setting, up: bool) -> bool {
        match setting {
            Setting::BackgroundBlur => {
                let current = self.config.ui_background_blur.unwrap_or(0);
                let next = step_i32(current, 2, up, Config::BACKGROUND_BLUR_RANGE) as u8;
                self.config.ui_background_blur = (next != 0).then_some(next);
            }
            Setting::PanelOpacity => {
                let current = self.config.ui_panel_opacity.unwrap_or(100);
                let next = step_i32(current, 5, up, Config::PANEL_OPACITY_RANGE) as u8;
                self.config.ui_panel_opacity = (next != 100).then_some(next);
            }
            Setting::CornerScale => {
                let current = self.config.ui_corner_scale.unwrap_or(100);
                let next = step_i32(current, 10, up, Config::CORNER_SCALE_RANGE) as u16;
                self.config.ui_corner_scale = (next != 100).then_some(next);
                self.apply_corner_scale();
            }
            Setting::AnimationSpeed => {
                let current = self.config.ui_animation_speed.unwrap_or(100);
                let next = step_i32(current, 25, up, Config::ANIMATION_SPEED_RANGE) as u16;
                self.config.ui_animation_speed = (next != 100).then_some(next);
            }
            _ => return false,
        }
        true
    }

    /// 提交强调色输入框；不是这一项时返回 `false`。
    pub(super) fn commit_accent(&mut self, setting: Setting, value: Option<&str>) -> bool {
        if setting != Setting::AccentColor {
            return false;
        }
        let accent = value.unwrap_or("").trim_start_matches('#').to_owned();
        if !accent.is_empty()
            && (accent.len() != 6 || !accent.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            self.settings.error = Some((setting, t!("settings-accent-invalid").to_owned()));
            self.request_redraw();
            return true;
        }
        self.settings.editing = None;
        self.settings.editing_original = None;
        self.settings.error = None;
        self.focus = Focus::Settings;
        self.config.ui_accent = (!accent.is_empty()).then(|| format!("#{accent}"));
        self.apply_accent();
        self.save_config();
        self.request_redraw();
        true
    }
}

/// 按 `by` 的步长调整 `current`，并夹在 `range` 内。
fn step_i32(
    current: impl Into<i32>,
    by: i32,
    up: bool,
    range: std::ops::RangeInclusive<f32>,
) -> i32 {
    let current: i32 = current.into();
    (current + if up { by } else { -by }).clamp(*range.start() as i32, *range.end() as i32)
}
