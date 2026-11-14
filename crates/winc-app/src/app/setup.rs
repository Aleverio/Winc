//! 首次设置：第一次运行时开始页只显示语言、外观、主题和模块几项，点击「完成」后换回开始操作
//! 和最近目录。页面出现之前播放一段启动动画：应用图标在窗口中央淡入、停留片刻后淡出，之后
//! 标题和各行逐个淡入。

use std::time::Duration;

use super::animation::stage;
use super::*;

/// 启动动画的时长（毫秒）：图标淡入、停留、淡出，之后内容淡入，各阶段占的比例见
/// [`BUILD_END`]、[`FADE`] 和 [`CONTENT_START`]。
///
/// 取 2200 毫秒时，图标淡入用 1.1 秒，停留约 0.26 秒，淡出与内容
/// 的淡入共约 0.84 秒。
pub(in crate::app) const INTRO: Duration = Duration::from_millis(2200);

/// 图标淡入完成时的进度（占整段动画的比例）。
const BUILD_END: f32 = 0.50;

/// 图标淡出的起止进度：之前图标完整地停留在窗口中央。
const FADE: (f32, f32) = (0.62, 0.78);

/// 内容开始淡入的进度；之前页面上只有图标，各控件不响应点击。
pub(crate) const CONTENT_START: f32 = 0.70;

/// 图标淡出途中向上移动的距离（逻辑像素）。
const SPLASH_RISE: f32 = 12.0;

/// 点击「完成」之后从「首次设置」换到开始页的时长（毫秒）：前段「首次设置」淡出，后段开始页
/// 淡入，各自的起止见 [`outro_opacity`]。
///
/// 取自调参，没有在真实窗口中观察过：淡出约 170 毫秒、淡入约 210 毫秒，与设置浮层出现的时长
/// 相近。两段不重叠：两个页面的标题位置不同，同时半透明时标题叠成两层。
pub(in crate::app) const OUTRO: Duration = Duration::from_millis(380);

/// 应用图标，与各平台安装包的图标同一份。
pub(crate) const ICON: &str = include_str!("../../assets/icon.svg");

impl App {
    /// 开始页是否正在显示「首次设置」：首次设置尚未完成，且没有打开目录，也没有打开的标签。
    ///
    /// 是否完成记在状态中（`State::setup_pending`），各窗口和各进程看到的是同一份。
    pub(crate) fn setup_shown(&self) -> bool {
        self.state.setup_pending && self.editor.workspace.is_none() && self.editor.active.is_none()
    }

    /// 窗口建好后调用：开始页显示「首次设置」时播放启动动画。
    ///
    /// 动态效果关闭时没有时钟，页面直接显示最终的样子。
    pub(in crate::app) fn start_setup_intro(&mut self) {
        if self.setup_shown() {
            self.motion.setup_intro = self.clock(INTRO);
        }
    }

    /// 处理开始页显示「首次设置」时的按键 `key`：回车等同于点击「完成」。返回是否已处理。
    ///
    /// 启动动画期间不处理：此时各项尚未显示，启动应用时仍按着的回车不应结束首次设置。按住不放
    /// 产生的重复按键由调用方排除。
    pub(in crate::app) fn setup_key(&mut self, key: &winit::keyboard::Key) -> bool {
        use winit::keyboard::{Key, NamedKey};
        let shown = self.setup_shown() && self.motion.setup_intro.is_none();
        let finish = shown && *key == Key::Named(NamedKey::Enter);
        if finish {
            self.finish_setup();
        }
        finish
    }

    /// 结束首次设置：开始页换回开始操作和最近目录，之后的启动不再显示。
    ///
    /// 「首次设置」先淡出，开始页随后淡入（见 [`OUTRO`]）；动态效果关闭时直接换过去。
    pub(in crate::app) fn finish_setup(&mut self) {
        self.motion.setup_intro = None;
        self.motion.setup_outro = self.clock(OUTRO);
        self.update_state(|state| state.setup_pending = false);
        self.request_redraw();
    }
}

/// 返回 `t` 在 `start` 到 `end` 之间的线性进度，0 到 1。
fn span(t: f32, start: f32, end: f32) -> f32 {
    ((t - start) / (end - start)).clamp(0.0, 1.0)
}

/// 启动动画进行到某一时刻时窗口中央图标的样子。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Splash {
    /// 图标淡入的进度，0 到 1（见 [`logo_frame`]）。
    pub build: f32,
    /// 图标的不透明度，0 到 1。
    pub opacity: f32,
    /// 图标比最终位置高出的距离（逻辑像素）：淡出途中向上移动。
    pub rise: f32,
}

/// 返回启动动画进行到 `t`（0 到 1，未加缓动）时窗口中央图标的样子；图标已经淡出时返回 `None`。
pub(crate) fn splash(t: f32) -> Option<Splash> {
    let fade = stage(t, FADE.0, FADE.1);
    (fade < 1.0).then(|| Splash {
        build: span(t, 0.0, BUILD_END),
        opacity: 1.0 - fade,
        rise: SPLASH_RISE * fade,
    })
}

/// 返回淡入进度为 `build`（0 到 1）时应用图标的 RGBA 像素，边长 `size`（物理像素）。
///
/// `build` 为 1 时与应用图标相同。
pub(crate) fn logo_frame(size: u32, build: f32) -> Vec<u8> {
    let opacity = stage(build, 0.0, 1.0);
    let Ok(picture) = winc_preview::svg::load(ICON, None, None) else {
        return Vec::new();
    };
    let mut rgba = picture.render(size, size).rgba;
    for pixel in rgba.chunks_exact_mut(4) {
        pixel[3] = (f32::from(pixel[3]) * opacity).round() as u8;
    }
    rgba
}

/// 返回结束首次设置的过渡进行到 `t`（0 到 1，未加缓动）时，「首次设置」和开始页各自的不透明度，
/// 0 到 1：前者在前 45% 淡出，后者在其后淡入。
pub(crate) fn outro_opacity(t: f32) -> (f32, f32) {
    (1.0 - stage(t, 0.0, 0.45), stage(t, 0.45, 1.0))
}

/// 返回启动动画进行到 `t`（0 到 1，未加缓动）时第 `index` 项内容的不透明度，0 到 1。
///
/// 内容从 [`CONTENT_START`] 起逐个淡入：标题是第 0 项，其后是说明、各行和「完成」按钮，每一项
/// 比前一项晚开始。
pub(crate) fn content_opacity(t: f32, index: usize) -> f32 {
    let start = CONTENT_START + index as f32 * 0.022;
    stage(t, start, start + 0.12)
}

#[cfg(test)]
mod tests;
