//! 语法高亮：受支持的源语言由 Tree-sitter 解析，其他语言及大型文件使用逐行 lexer。
//!
//! [`Highlighter`] 缓存逐行 lexer 状态，并为 Tree-sitter 文档增量更新语法树。编辑器按行读取 token；
//! 本 crate 不依赖绘制与主题，颜色由 `winct-render` 和 `winct-app` 决定。

mod lang;
mod lexer;
mod tree_sitter;

use std::ops::ControlFlow;
use std::sync::Arc;

/// 着色时一行最多分析的字符数，见 [`winct_core::text::MAX_LINE_CHARS`]；调用方据此跳过更长的行。
pub use winct_core::text::MAX_LINE_CHARS;
use winct_core::text::{Lines, MAX_LINE_BYTES};

pub use lang::{
    BlockString, Heredocs, Interpolation, Language, Markup, RawStrings, Variables, detect,
    detect_with_first_line, find, languages,
};
pub use lexer::{LineState, Token, TokenKind, highlight_line};
pub use tree_sitter::{MAX_TREE_SITTER_BYTES, overlay_tree_tokens};

/// 一个文档的高亮状态：逐行 lexer 状态缓存及可选的 Tree-sitter 语法树。
///
/// 只保存已经推算到的行，其余行在需要时向后推算；编辑后从修改所在行开始失效。
#[derive(Debug, Clone)]
pub struct Highlighter {
    /// 文档使用的语言规则。
    language: &'static Language,
    /// `states[i]` 是第 `i` 行行首的状态（行号从 0 起）。
    ///
    /// 长度等于已推算的行数，且不小于 1。
    states: Vec<LineState>,
    /// 大文件按 [`Self::state_near`] 推算出的一段连续行首状态：`near[i]` 是第
    /// `near_from + i` 行行首的状态，`near[0]` 从 [`Language::start_state`] 开始推算。
    ///
    /// 为空时没有可用的结果。
    near: Vec<LineState>,
    /// `near` 第一项对应的行号（从 0 起）。
    near_from: usize,
    /// 推算状态时复用的字符缓冲，避免每行分配。
    chars: Vec<char>,
    /// Tree-sitter 语法树和按行缓存的语法 token（适用于受支持的小型源文件）。
    tree_sitter: Option<Arc<tree_sitter::TreeTokens>>,
    /// 文本从上次建立语法树后发生了变化。
    tree_sitter_dirty: bool,
}

impl Highlighter {
    /// 创建只含第 0 行行首状态（[`Language::start_state`]）的高亮状态。
    pub fn new(language: &'static Language) -> Self {
        Self {
            language,
            states: vec![language.start_state()],
            near: Vec::new(),
            near_from: 0,
            chars: Vec::new(),
            tree_sitter: None,
            tree_sitter_dirty: true,
        }
    }

    /// 返回创建时传入的语言规则。
    pub fn language(&self) -> &'static Language {
        self.language
    }

    /// 返回已推算出的第 `line` 行行首的状态，不做新的推算；尚未推算到该行时返回 `None`。
    pub fn known_state(&self, line: usize) -> Option<LineState> {
        let near = || self.near.get(line.checked_sub(self.near_from)?);
        self.states.get(line).or_else(near).copied()
    }

    /// 丢弃第 `line` 行之后各行行首的状态；在第 `line` 行发生编辑后调用。
    pub fn invalidate_from(&mut self, line: usize) {
        self.states.truncate(line + 1);
        match line.checked_sub(self.near_from) {
            Some(kept) => self.near.truncate(kept + 1),
            None => self.near.clear(),
        }
        self.tree_sitter_dirty = true;
    }

    /// 是否应为当前文本更新 Tree-sitter 语法树。
    ///
    /// 对不支持的语言和超出大小上限的文件清除树缓存，继续使用逐行 lexer。
    pub fn needs_tree_sitter_update(&mut self, byte_len: usize) -> bool {
        if byte_len > MAX_TREE_SITTER_BYTES || !tree_sitter::supports(self.language.id) {
            self.tree_sitter = None;
            self.tree_sitter_dirty = false;
            return false;
        }
        self.tree_sitter_dirty
    }

    /// 使用当前源代码建立或增量更新 Tree-sitter 语法树。
    ///
    /// 超过 [`MAX_TREE_SITTER_BYTES`] 的文件由逐行 lexer 处理，避免完整源文本和语法树占用过多内存。
    pub fn update_tree_sitter(&mut self, source: &str) {
        let previous = self
            .tree_sitter
            .take()
            .and_then(|cache| Arc::try_unwrap(cache).ok());
        self.tree_sitter =
            tree_sitter::TreeTokens::update(self.language, source, previous).map(Arc::new);
        self.tree_sitter_dirty = false;
    }

    /// 返回已准备好的 Tree-sitter token；文本变更后、重建完成前返回 `None`。
    pub fn tree_sitter_tokens(&self, line: usize) -> Option<&[Token]> {
        if self.tree_sitter_dirty {
            return None;
        }
        self.tree_sitter
            .as_ref()
            .filter(|tokens| tokens.language_id == self.language.id)
            .and_then(|tokens| tokens.lines.get(line))
            .map(Vec::as_slice)
    }

    /// 返回第 `line` 行行首的状态（行号从 0 起）；本次推算后仍未到达该行时返回 `None`。
    ///
    /// 从已推算到的位置向后推算，每次最多推算 `budget` 行。返回 `None` 时，调用方先按 `Normal`
    /// 绘制，下一帧继续推算，使跳转到长文件末尾时不会阻塞单帧。`line` 超过末行时按末行计算。
    pub fn state_at(&mut self, text: &impl Lines, line: usize, budget: usize) -> Option<LineState> {
        let target = line.min(text.line_count() - 1);
        let start = self.states.len() - 1;
        let end = start + target.saturating_sub(start).min(budget);

        if end > start {
            let (language, states, chars) = (self.language, &mut self.states, &mut self.chars);

            text.visit_short_lines(start, MAX_LINE_BYTES, |index, _, content| {
                if index >= end {
                    return ControlFlow::Break(());
                }

                let state = states[states.len() - 1];
                states.push(content.map_or(state, |line| scan(language, chars, line, state)));
                ControlFlow::Continue(())
            });
        }

        self.states.get(target).copied()
    }

    /// 同 [`Self::state_at`]，但不写缓存；要推算的行数超过 `budget` 时返回 `None`。
    pub fn peek_state(&self, text: &impl Lines, line: usize, budget: usize) -> Option<LineState> {
        let target = line.min(text.line_count().saturating_sub(1));
        if let Some(state) = self.known_state(target) {
            return Some(state);
        }
        let start = self.states.len() - 1;
        if target.saturating_sub(start) > budget {
            return None;
        }
        let (mut state, mut chars) = (self.states[start], Vec::new());
        text.visit_short_lines(start, MAX_LINE_BYTES, |index, _, content| {
            if index >= target {
                return ControlFlow::Break(());
            }
            state = content.map_or(state, |line| scan(self.language, &mut chars, line, state));
            ControlFlow::Continue(())
        });
        Some(state)
    }

    /// 返回第 `line` 行行首的状态，从 `line` 之前 `lookback` 行处开始推算。
    ///
    /// 推算起点是第 0 行时按 [`Language::start_state`]（通常是 `Normal`）开始，否则按
    /// `Normal` 开始。
    ///
    /// 查看大文件末尾时无需从文件开头扫描；代价是跨度超过 `lookback` 行的块注释和多行
    /// 字符串可能着色错误。
    ///
    /// 推算出的一段状态保留在 `near` 中，`line` 落在其中时直接返回，否则从该段末尾继续推算。
    pub fn state_near(&mut self, text: &impl Lines, line: usize, lookback: usize) -> LineState {
        let end = self.near_from + self.near.len();
        if self.near.is_empty() || line < self.near_from || line > end + lookback {
            self.near_from = line.saturating_sub(lookback);
            self.near = vec![if self.near_from == 0 {
                self.language.start_state()
            } else {
                LineState::Normal
            }];
        }
        let (language, chars, near) = (self.language, &mut self.chars, &mut self.near);
        let from = self.near_from;
        if line >= from + near.len() {
            text.visit_short_lines(
                from + near.len() - 1,
                MAX_LINE_BYTES,
                |index, _, content| {
                    if index >= line {
                        return ControlFlow::Break(());
                    }
                    let state = near[near.len() - 1];
                    near.push(content.map_or(state, |line| scan(language, chars, line, state)));
                    ControlFlow::Continue(())
                },
            );
        }
        near.get(line - from).copied().unwrap_or(LineState::Normal)
    }
}

/// 推算第 `line` 行之后下一行行首的状态。
///
/// `chars` 是复用的字符缓冲，只取行首的 `MAX_LINE_CHARS` 个字符。超过该长度的行只扫描开头，
/// 此时直接返回 `state`，避免截断位置落在字符串或块注释中间时后续整个文件都按字符串着色。
fn scan(language: &Language, chars: &mut Vec<char>, line: &str, state: LineState) -> LineState {
    chars.clear();
    let mut all = line.chars();
    chars.extend(all.by_ref().take(MAX_LINE_CHARS));
    if all.next().is_some() {
        return state;
    }
    lexer::scan_line(language, chars, state)
}

#[cfg(test)]
mod tests;
