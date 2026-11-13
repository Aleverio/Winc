//! 名字旁边的提示气泡：转到定义没有找到定义、未找到其他用法、光标处没有可以查找的名字时，
//! 在光标所在的名字上方弹出一句话，而不是写在状态栏里。
//!
//! 气泡挂在文档中的位置上，随文档滚动；停留 [`HINT_DURATION`] 后淡出，按键、点击、滚动或
//! 打开浮窗时立即淡出。

use std::time::{Duration, Instant};

use winc_core::movement;
use winc_editor::DocumentId;

use super::*;

/// 提示气泡停留的时长（经验值）：够读完一句话。
const HINT_DURATION: Duration = Duration::from_millis(3000);
const DIAGNOSTIC_HINT_DURATION: Duration = Duration::from_millis(750);

/// 名字旁边的提示气泡。
pub struct NameHint {
    /// 提示文字。
    pub text: String,
    /// 气泡挂在哪个文档上。
    pub doc: DocumentId,
    /// 气泡挂在文档中的位置（字符偏移）：光标所在名字的开头。
    pub anchor: usize,
    /// 到这一刻开始淡出。
    until: Instant,
    /// 出现的动画；结束后为 `None`。
    pub fade_in: Option<Clock>,
    /// 消失的动画；还没开始消失时为 `None`。
    pub fade_out: Option<Clock>,
}

/// Diagnostic popup retained briefly after the pointer leaves its underline or card.
pub(crate) struct DiagnosticHint {
    pub doc: DocumentId,
    pub index: usize,
    pub dismiss_at: Option<Instant>,
    pub fade_in: Option<Clock>,
    pub fade_out: Option<Clock>,
}

impl App {
    /// 在光标所在的名字上方弹出提示 `text`；没有打开文档时改为写在状态栏。
    pub(in crate::app) fn hint_at_name(&mut self, text: impl Into<String>) {
        let text = text.into();
        let Some(doc) = self.editor.active() else {
            self.flash(text);
            return;
        };
        let anchor = self.name_span().map_or_else(
            || movement::word_at(doc.text(), doc.selection.head).0,
            |s| s.0,
        );
        let id = doc.id;
        let fade_in = self.clock(POPUP_IN);
        self.jumps.name_hint = Some(NameHint {
            text,
            doc: id,
            anchor,
            until: Instant::now() + HINT_DURATION,
            fade_in,
            fade_out: None,
        });
        self.request_redraw();
    }

    /// 让提示气泡淡出；没有窗口（测试中）或动态效果关闭时直接去掉。
    pub(in crate::app) fn dismiss_hint(&mut self) {
        let Some(hint) = &self.jumps.name_hint else {
            return;
        };
        if hint.fade_out.is_some() {
            return;
        }
        match self.clock(POPUP_OUT) {
            Some(clock) => {
                if let Some(hint) = &mut self.jumps.name_hint {
                    hint.fade_out = Some(clock);
                }
            }
            None => self.jumps.name_hint = None,
        }
        self.request_redraw();
    }

    /// 停留时间到了就开始淡出；返回下一次需要检查的时刻。
    pub(in crate::app) fn hint_tick(&mut self, now: Instant) -> Option<Instant> {
        let name_until = self
            .jumps
            .name_hint
            .as_ref()
            .filter(|hint| hint.fade_out.is_none())
            .map(|hint| hint.until);
        let name_wake = if name_until.is_some_and(|until| now >= until) {
            self.dismiss_hint();
            None
        } else {
            name_until
        };

        let diagnostic_until = self
            .diagnostic_hint
            .as_ref()
            .filter(|hint| hint.fade_out.is_none())
            .and_then(|hint| hint.dismiss_at);
        let diagnostic_wake = match diagnostic_until {
            Some(until) if now >= until => {
                if let Some(clock) = self.clock(POPUP_OUT) {
                    if let Some(hint) = &mut self.diagnostic_hint {
                        hint.dismiss_at = None;
                        hint.fade_out = Some(clock);
                    }
                    self.request_redraw();
                } else {
                    self.diagnostic_hint = None;
                    self.hits.diagnostic_popup = None;
                    self.request_redraw();
                }
                None
            }
            other => other,
        };

        name_wake.into_iter().chain(diagnostic_wake).min()
    }

    pub(in crate::app) fn retain_diagnostic_hint(&mut self, now: Instant) {
        if let Some(hint) = &mut self.diagnostic_hint
            && hint.dismiss_at.is_none()
        {
            hint.dismiss_at = Some(now + DIAGNOSTIC_HINT_DURATION);
        }
    }

    pub(in crate::app) fn show_diagnostic_hint(&mut self, doc: DocumentId, index: usize) {
        let same = self
            .diagnostic_hint
            .as_ref()
            .is_some_and(|hint| hint.doc == doc && hint.index == index);
        if same {
            if let Some(hint) = &mut self.diagnostic_hint {
                hint.dismiss_at = None;
                hint.fade_out = None;
            }
        } else {
            let fade_in = self.clock(POPUP_IN);
            self.diagnostic_hint = Some(DiagnosticHint {
                doc,
                index,
                dismiss_at: None,
                fade_in,
                fade_out: None,
            });
            self.diagnostic_scroll = (0.0, 0.0);
        }
    }

    /// 清除已经结束的出现、消失动画；消失结束后去掉气泡。
    pub(in crate::app) fn settle_hint(&mut self) {
        if let Some(hint) = &mut self.jumps.name_hint {
            if hint.fade_in.is_some_and(|c| c.done()) {
                hint.fade_in = None;
            }
            if hint.fade_out.is_some_and(|c| c.done()) {
                self.jumps.name_hint = None;
            }
        }
        if let Some(hint) = &mut self.diagnostic_hint {
            if hint.fade_in.is_some_and(|c| c.done()) {
                hint.fade_in = None;
            }
            if hint.fade_out.is_some_and(|c| c.done()) {
                self.diagnostic_hint = None;
                self.hits.diagnostic_popup = None;
            }
        }
    }

    /// 提示气泡是否在动画中。
    pub(in crate::app) fn hint_animating(&self) -> bool {
        self.jumps
            .name_hint
            .as_ref()
            .is_some_and(|h| h.fade_in.is_some() || h.fade_out.is_some())
            || self
                .diagnostic_hint
                .as_ref()
                .is_some_and(|h| h.fade_in.is_some() || h.fade_out.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::Bench;
    use winc_config::Config;
    use winc_editor::{DocumentId, Editor};

    #[test]
    fn diagnostic_popup_waits_750_milliseconds_and_reentry_restarts_the_timer() {
        let mut app = App::new(
            Editor::default(),
            None,
            Bench::from_env(),
            Config::default(),
        );
        let now = Instant::now();
        let first_deadline = now + DIAGNOSTIC_HINT_DURATION;

        app.show_diagnostic_hint(DocumentId(1), 0);
        app.retain_diagnostic_hint(now);
        assert_eq!(
            app.hint_tick(first_deadline - Duration::from_millis(1)),
            Some(first_deadline)
        );

        app.show_diagnostic_hint(DocumentId(1), 0);
        assert_eq!(
            app.diagnostic_hint
                .as_ref()
                .and_then(|hint| hint.dismiss_at),
            None
        );
        let reentered_at = now + Duration::from_millis(500);
        let second_deadline = reentered_at + DIAGNOSTIC_HINT_DURATION;
        app.retain_diagnostic_hint(reentered_at);
        assert_eq!(app.hint_tick(first_deadline), Some(second_deadline));
        assert_eq!(app.hint_tick(second_deadline), None);
        assert!(app.diagnostic_hint.is_none());
    }
}
