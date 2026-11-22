//! 主光标的移动动画：光标从上一位置滑到新位置。
//!
//! 曲线和时长取自 IntelliJ IDEA 的平滑光标移动：Snappy 在开始的一小段时间里走过大半距离，
//! 之后缓缓停住；Gliding 沿整段距离平滑移动。移动途中光标再次移动时，从当前画面上的位置接着
//! 走向新目标，并逐次缩短时长，使连续按键时不会落后于输入。
//!
//! 位置由绘制时算出的光标矩形给出（见 `view::editor`），本模块只负责状态和插值。视口滚动、
//! 换行或布局变化时目标随之改变：光标随文字移动，不单独播放动画。

use std::time::Duration;

use winc_config::SmoothCaret;
use winc_editor::DocumentId;
use winc_render::Rect;

use super::*;

/// 一次光标移动的时长（毫秒）。
///
/// 取 IntelliJ IDEA 的 `editor.smooth.caret.duration` 默认值 120 ms：短于侧栏等过渡，
/// 连续按键时不会明显滞后。
const CARET_MOVE: Duration = Duration::from_millis(120);

/// 连续被打断的移动依次缩短时长，最少缩到完整时长的这一比例。
const MIN_URGENCY: f32 = 0.2;

/// 每次被打断时时长乘以这一比例。
const URGENCY_DECAY: f32 = 0.8;

/// 判断两个光标位置是否画在同一处（逻辑像素）。
///
/// 小于一个像素的差别看不出来；目标每帧重新计算，不能因此把动画当成被滚动打断。
const SAME_SPOT: f32 = 0.1;

/// 光标在屏幕上的位置，以及它所在的文档、字符偏移和视口位置。
#[derive(Debug, Clone, Copy)]
pub(crate) struct CaretTarget {
    /// 光标所在的文档和字符偏移；位置改变时开始新的移动。
    pub key: (DocumentId, usize),
    /// 视口位置（显示行、横向滚动）：滚动时光标随文字移动，直接跳到目标。
    pub scroll: (f64, f32),
    /// 光标矩形（逻辑像素）；宽、高在移动中不变。
    pub rect: Rect,
}

/// 主光标的移动动画。
#[derive(Debug, Clone, Copy)]
pub(crate) struct CaretMotion {
    /// 光标所在的文档和字符偏移。
    key: (DocumentId, usize),
    /// 动画起点和当前目标（逻辑像素）。
    from: Rect,
    to: Rect,
    /// 上一帧的视口位置。
    scroll: (f64, f32),
    /// 动画时钟。
    clock: Clock,
    /// 连续被打断时缩短的时长比例，停下后恢复为 1。
    urgency: f32,
    /// 是否正在移动。
    moving: bool,
}

impl CaretMotion {
    /// 返回停在 `target` 处的状态。
    fn at(target: CaretTarget) -> Self {
        Self {
            key: target.key,
            from: target.rect,
            to: target.rect,
            scroll: target.scroll,
            clock: Clock::new(CARET_MOVE),
            urgency: 1.0,
            moving: false,
        }
    }

    /// 是否正在移动。
    pub(super) fn moving(&self) -> bool {
        self.moving && !self.clock.done()
    }

    /// 返回当前画面上的光标矩形。
    fn painted(&self, style: SmoothCaret) -> Rect {
        let t = ease(style, self.clock.linear());
        Rect::new(
            self.from.x + (self.to.x - self.from.x) * t,
            self.from.y + (self.to.y - self.from.y) * t,
            self.to.w,
            self.to.h,
        )
    }

    /// 把动画时钟固定在已过 `at` 处，仅用于测试。
    #[cfg(test)]
    pub(in crate::app) fn freeze(&mut self, at: Duration) {
        self.clock = Clock::frozen(at, self.clock.duration());
    }

    /// 动画已经结束时把光标停在目标处。
    pub(super) fn settle(&mut self) {
        if self.clock.done() {
            self.moving = false;
            self.from = self.to;
        }
    }
}

/// 返回时间曲线在 `progress`（0 到 1）处的进度。
///
/// 曲线取自 IntelliJ IDEA（`CaretEasing`）：Snappy 是先快后慢的解析曲线，Gliding 是
/// `cubic-bezier(0.25, 0.1, 0.25, 1.0)` 的近似。`Off` 不播放动画，因此直接到终点。
fn ease(style: SmoothCaret, progress: f32) -> f32 {
    let t = progress.clamp(0.0, 1.0);
    let eased = match style {
        SmoothCaret::Off => 1.0,
        SmoothCaret::Snappy => {
            let root = t.cbrt();
            3.0 * root - 3.0 * root * root + t
        }
        SmoothCaret::Gliding => t * ((((-5.4 * t + 17.6) * t - 20.6) * t + 9.0) * t + 0.4),
    };
    // 近似曲线在端点上的舍入误差不应让光标越过终点。
    eased.clamp(0.0, 1.0)
}

/// 返回两个光标位置是否画在同一处。
fn same_spot(a: Rect, b: Rect) -> bool {
    (a.x - b.x).abs() <= SAME_SPOT && (a.y - b.y).abs() <= SAME_SPOT
}

impl App {
    /// 返回光标移动的动画方式；`editor.smoothCaretMovement` 没写时不播放动画。
    pub(crate) fn smooth_caret(&self) -> SmoothCaret {
        self.config.smooth_caret_movement.unwrap_or_default()
    }

    /// 根据本帧的光标位置推进移动动画，返回本帧要绘制的光标矩形。
    ///
    /// `target` 是绘制时算出的光标位置。位置改变时从当前画面上的位置开始新的移动；视口滚动、
    /// 换了文档或光标不在屏幕上（状态已被清除）时直接跳到目标。
    pub(crate) fn caret_position(&mut self, target: CaretTarget) -> Rect {
        let style = self.smooth_caret();
        if style == SmoothCaret::Off {
            self.motion.caret_motion = None;
            return target.rect;
        }
        let Some(mut motion) = self.motion.caret_motion else {
            self.motion.caret_motion = Some(CaretMotion::at(target));
            return target.rect;
        };

        motion.settle();
        if motion.key == target.key && same_spot(motion.to, target.rect) {
            let rect = motion.painted(style);
            self.motion.caret_motion = Some(motion);
            return rect;
        }
        // 同一光标的目标变了（滚动、换行或布局变化），或换了文档：直接跳到目标。
        if motion.key == target.key
            || motion.key.0 != target.key.0
            || motion.scroll != target.scroll
        {
            self.motion.caret_motion = Some(CaretMotion::at(target));
            return target.rect;
        }

        // 光标移动到了新的位置：从当前画面上的位置接着滑过去。被打断的移动一次比一次短，
        // 连续按键时不会一直追不上输入。
        let from = motion.painted(style);
        let urgency = if motion.moving {
            (motion.urgency * URGENCY_DECAY).max(MIN_URGENCY)
        } else {
            1.0
        };
        motion.key = target.key;
        motion.from = from;
        motion.to = target.rect;
        motion.scroll = target.scroll;
        motion.clock = Clock::new(CARET_MOVE.mul_f32(urgency));
        motion.urgency = urgency;
        motion.moving = true;
        self.motion.caret_motion = Some(motion);
        from
    }

    /// 本帧没有画出主光标：清除移动动画，下一次移动直接出现在新位置。
    pub(crate) fn caret_hidden(&mut self) {
        self.motion.caret_motion = None;
    }

    /// 设置光标移动的动画方式，并写回配置。
    pub(in crate::app) fn set_smooth_caret(&mut self, style: SmoothCaret) {
        self.config.smooth_caret_movement = Some(style);
        self.motion.caret_motion = None;
        self.save_config();
        self.request_redraw();
    }
}

#[cfg(test)]
mod tests;
