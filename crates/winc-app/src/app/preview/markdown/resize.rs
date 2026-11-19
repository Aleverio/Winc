//! 可编辑的预览中调整图片的大小：右键菜单按正文宽度的比例，或拖动图片右边的把手。改写
//! 图片的原文（见 `winc_preview::markdown::resize`），只占一步撤销，光标留在原处，不切换成
//! 原文显示。

use winc_preview::markdown::resize::{self, Width};
use winc_render::Rect;
use winc_typeset::Item;

use crate::app::App;

/// 把手（细竖条）的宽度（逻辑像素）。
const HANDLE: f32 = 4.0;
/// 把手的高度范围（逻辑像素）：取图片高度的三成，夹在这两者之间。
const GRIP: (f32, f32) = (20.0, 32.0);
/// 把手离图片右边缘的距离（逻辑像素）：画在图片外面的页面底色上，不压住图片。
const HANDLE_GAP: f32 = 4.0;
/// 拖动时的框离图片边缘的距离（逻辑像素）。
pub(crate) const OUTLINE_GAP: f32 = 2.0;
/// 鼠标离图片多近时算停在图片上（逻辑像素）：移到图片外的把手上，把手不消失。
const REACH: f32 = 12.0;
/// 显示把手的最小图片宽高（逻辑像素）：更小的图片只能用右键菜单调整。
const HANDLE_ROOM: f32 = 32.0;
/// 拖动把手时图片的最小宽度（逻辑像素）。
const MIN_WIDTH: f32 = 24.0;
/// 右键菜单「图片大小」里的比例（百分数），0 表示去掉写明的大小。
pub(crate) const PRESETS: [u32; 5] = [25, 50, 75, 100, 0];

/// 上一帧画出的一张有原文可改的图片。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ImageAt {
    /// 在窗口中的范围。
    pub(crate) rect: Rect,
    /// 所在的顶层块。
    pub(crate) block: usize,
    /// 在块的各项中的序号。
    pub(crate) item: usize,
}

impl ImageAt {
    /// 把手的范围（窗口坐标）：图片右边外侧、上下居中的细竖条；图片太小时为 `None`。
    pub(crate) fn handle(&self) -> Option<Rect> {
        let r = self.rect;
        if r.w < HANDLE_ROOM || r.h < HANDLE_ROOM {
            return None;
        }
        let h = (r.h * 0.3).clamp(GRIP.0, GRIP.1).round();
        let y = (r.y + (r.h - h) / 2.0).round();
        Some(Rect::new((r.right() + HANDLE_GAP).round(), y, HANDLE, h))
    }
}

/// 正在拖动把手调整的图片。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Resizing {
    /// 图片。
    pub(crate) image: ImageAt,
    /// 按下时鼠标的横坐标。
    from: f32,
    /// 图片在正文一栏中居中：宽度两边同时变，把手跟着鼠标。
    pub(crate) centered: bool,
    /// 当前的宽度（逻辑像素）。
    pub(crate) width: f32,
}

impl App {
    /// 鼠标 `(x, y)` 下的图片（只在可编辑的预览中）。
    pub(crate) fn image_at(&self, x: f32, y: f32) -> Option<ImageAt> {
        let area = self.preview.markdown.area;
        (self.markdown_live() && area.contains(x, y))
            .then(|| {
                self.hits
                    .markdown_images
                    .iter()
                    .rev()
                    .find(|i| i.rect.contains(x, y))
            })
            .flatten()
            .copied()
    }

    /// 鼠标 `(x, y)` 停在上面（含四周一圈）的图片，显示框和把手。
    pub(crate) fn image_hovered(&self, x: f32, y: f32) -> Option<ImageAt> {
        let area = self.preview.markdown.area;
        let images = &self.hits.markdown_images;
        (self.markdown_live() && area.contains(x, y))
            .then(|| (images.iter().rev()).find(|i| i.rect.inset(-REACH, -REACH).contains(x, y)))
            .flatten()
            .copied()
    }

    /// 鼠标 `(x, y)` 下的把手所属的图片：四周放宽几像素便于点中。
    pub(crate) fn image_handle_at(&self, x: f32, y: f32) -> Option<ImageAt> {
        let image = self.image_hovered(x, y)?;
        let handle = image.handle()?.inset(-5.0, -4.0);
        handle.contains(x, y).then_some(image)
    }

    /// 按下把手：开始拖动。
    pub(crate) fn start_image_resize(&mut self, image: ImageAt, x: f32) {
        let width = image.rect.w;
        let centered = (self.preview.markdown.layout.as_ref()).is_some_and(|l| {
            let column = l.left + l.key.width / 2.0;
            (image.rect.x + image.rect.w / 2.0 - column).abs() < 1.0 && width < l.key.width
        });
        self.preview.markdown.resizing = Some(Resizing {
            image,
            from: x,
            centered,
            width,
        });
        self.pointer.drag = crate::app::Drag::ImageResize;
        self.request_redraw();
    }

    /// 拖动把手：宽度跟着鼠标，不超过正文一栏。
    pub(crate) fn drag_image_resize(&mut self, x: f32) {
        let column = self
            .preview
            .markdown
            .layout
            .as_ref()
            .map_or(f32::INFINITY, |l| l.key.width);
        if let Some(r) = self.preview.markdown.resizing.as_mut() {
            let pull = if r.centered { 2.0 } else { 1.0 };
            let width = r.image.rect.w + (x - r.from) * pull;
            r.width = width.clamp(MIN_WIDTH.min(column), column);
            self.request_redraw();
        }
    }

    /// 松开把手：宽度变了时写回原文。
    pub(crate) fn finish_image_resize(&mut self) {
        if let Some(r) = self.preview.markdown.resizing.take() {
            if (r.width - r.image.rect.w).abs() >= 1.0 {
                self.resize_image(r.image, Some(Width::Px(r.width.round() as u32)));
            }
            self.request_redraw();
        }
    }

    /// 右键菜单「图片大小」中的一项：`percent` 为 0 时去掉写明的大小。
    pub(crate) fn resize_image_to(&mut self, percent: u32) {
        if let Some(image) = self.preview.markdown.menu_image.take() {
            let width = (percent > 0).then_some(Width::Percent(percent));
            self.resize_image(image, width);
        }
    }

    /// 把图片 `image` 的原文改成宽度 `width`（`None` 为去掉写明的大小），返回是否改了。
    fn resize_image(&mut self, image: ImageAt, width: Option<Width>) -> bool {
        let Some((start, end, nth)) = self.image_source(image) else {
            return false;
        };
        let Some(doc) = self.editor.active_mut() else {
            return false;
        };
        let Some(rope) = doc.text().rope() else {
            return false;
        };
        let end = end.min(rope.len_bytes());
        let text = rope.byte_slice(start.min(end)..end).to_string();
        let Some((range, new)) = resize::resize(&text, nth, width) else {
            return false;
        };
        let (a, b) = (
            rope.byte_to_char(start + range.start),
            rope.byte_to_char(start + range.end),
        );
        let delta = new.chars().count() as isize - (b - a) as isize;
        // 改写处之后的位置跟着移动，改写处之内的移到开头。
        let keep = |pos: usize| match pos {
            pos if pos <= a => pos,
            pos if pos >= b => pos.saturating_add_signed(delta),
            _ => a,
        };
        let old = doc.selection;
        let editing = self.preview.markdown.editing == Some(doc.id);
        doc.collapse_selections();
        doc.selection = winc_core::Selection::new(a, b);
        self.preview.markdown.slide = true;
        self.run(winc_editor::Command::Insert(new));
        if let Some(doc) = self.editor.active_mut() {
            doc.selection = winc_core::Selection::new(keep(old.anchor), keep(old.head));
        }
        // 改大小不算在预览里编辑：没在编辑时，光标所在的块不因此换成原文。
        if !editing
            && let (Some(source), Some(doc)) = (self.live_caret_source(), self.editor.active())
        {
            self.preview.markdown.followed = Some((doc.id, source, doc.revision));
        }
        true
    }

    /// 图片的原文：开头（字节偏移）、所在顶层块的结尾、是块里同一处原文的第几张图（HTML 块里
    /// 的图片都对应块的开头）。
    fn image_source(&self, image: ImageAt) -> Option<(usize, usize, usize)> {
        let laid = self.preview.markdown.layout.as_ref()?;
        let items = &laid.blocks.get(image.block)?.1.items;
        let source = |item: &Item| match item {
            Item::Image {
                at: Some((_, s)), ..
            } => Some(*s),
            _ => None,
        };
        let at = source(items.get(image.item)?)?;
        let nth = items[..image.item]
            .iter()
            .filter(|i| source(i) == Some(at))
            .count();
        let range = laid.ranges.get(image.block)?;
        Some((range.start + at as usize, range.end, nth))
    }
}
