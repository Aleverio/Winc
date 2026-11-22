//! Plugin WASM 的安全绘制层。
//!
//! Guest 只能发出 [`DrawCommand`] 值指令；这里把它们翻译成软件渲染器的图元，并把所有坐标裁剪
//! 到 surface 的区域。坐标是相对整个窗口的逻辑像素，与 `RenderRequest` 里的 `x`/`y` 一致；
//! 插件拿不到 `Frame` 或像素缓冲区，也无法越过裁剪区绘制。

use winc_plugin::api::{DrawCommand, RenderSurface};
use winc_render::{Color, FontKind, Frame, Rect, Stroke};

use crate::app::App;

/// 一帧最多绘制的插件命令数，防止有问题的插件拖慢渲染。
const MAX_COMMANDS: usize = 512;

/// 在浮层上绘制插件生成的命令。
pub(super) fn draw_overlays(app: &mut App, frame: &mut Frame<'_>) {
    let area = app.layout.window;
    let mut commands = app.wasm_render(RenderSurface::Overlay, None, area);
    let mut cached = app.wasm.all_overlay_commands();
    commands.append(&mut cached);
    draw_commands(frame, area, &commands);
}

/// 在状态栏区域内绘制插件状态命令。
pub(super) fn draw_status_surface(app: &mut App, frame: &mut Frame<'_>) {
    let area = app.layout.status;
    if area.w <= 0.0 || area.h <= 0.0 {
        return;
    }
    let commands = app.wasm_render(RenderSurface::StatusBar, None, area);
    draw_commands(frame, area, &commands);
}

fn draw_commands(frame: &mut Frame<'_>, area: Rect, commands: &[DrawCommand]) {
    if commands.len() > MAX_COMMANDS {
        return;
    }
    frame.within(area, |frame| {
        for command in commands {
            match *command {
                DrawCommand::Rect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    radius,
                } => {
                    if !valid(x) || !valid(y) || !valid(width) || !valid(height) || !valid(radius) {
                        continue;
                    }
                    let rect = Rect::new(x, y, width.max(0.0), height.max(0.0));
                    frame.rounded(rect, color_of(color), radius.max(0.0));
                }
                DrawCommand::Text {
                    x,
                    y,
                    ref text,
                    size,
                    color,
                } => {
                    if !valid(x) || !valid(y) || !valid(size) || size <= 0.0 {
                        continue;
                    }
                    let size = size.clamp(6.0, 96.0);
                    frame.text(FontKind::Ui, size, x, y + size, text, color_of(color), area);
                }
                DrawCommand::Line {
                    x1,
                    y1,
                    x2,
                    y2,
                    color,
                    width,
                } => {
                    if !valid(x1) || !valid(y1) || !valid(x2) || !valid(y2) || !valid(width) {
                        continue;
                    }
                    frame.stroke(
                        &[Stroke::Line(x1, y1, x2, y2)],
                        width.clamp(0.5, 64.0),
                        color_of(color),
                        area,
                    );
                }
            }
        }
    });
}

fn valid(value: f32) -> bool {
    value.is_finite() && value.abs() <= 100_000.0
}

fn color_of(value: u32) -> Color {
    Color::hex(value & 0x00ff_ffff)
}
