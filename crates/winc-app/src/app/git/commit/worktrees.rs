//! 提交视图底部的工作树：一组可以收起的行，当前仓库的每个工作树一行。
//!
//! 一组在储藏的上面，当前仓库有多个工作树时才有；展开、收起的过渡与储藏一组共用
//! （[`Drawer`]）。点击一行打开该工作树，在新窗口还是当前窗口按设置，默认询问；悬停时右端多
//! 一个「⋯」（在此窗口或新窗口中打开、合并到当前分支、移除等），标题行悬停时右端是「新建
//! 工作树」。直接列出的行数有上限，再多时最后一行写还有几个，点击后展开工作树选择器。

use super::*;

/// 工作树一组展开时最多占几行（行），连同「另有 N 个工作树」：与储藏一组的上限（`STASH_ROWS`）
/// 相同，再多时上方的改动列表高度过小。
pub(crate) const WORKTREE_ROWS: usize = 5;

/// 工作树一组展开时标题下面的一行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorktreeLine {
    /// 第几个工作树（[`App::worktree_places`] 的下标）。
    Place(usize),
    /// 没有列出的工作树个数。
    More(usize),
}

impl App {
    /// 工作树一组展开时标题下面的各行：最多 [`WORKTREE_ROWS`] 行，放不下时最后一行是「另有
    /// N 个工作树」。当前工作树总在列出的各行之中。
    pub(crate) fn worktree_lines(&self) -> Vec<WorktreeLine> {
        let places = self.worktree_places();
        if places.len() <= WORKTREE_ROWS {
            return (0..places.len()).map(WorktreeLine::Place).collect();
        }
        let room = WORKTREE_ROWS - 1;
        let mut shown: Vec<usize> = (0..room).collect();
        if let Some(current) = places.iter().position(|p| p.current)
            && current >= room
        {
            shown[room - 1] = current;
        }
        let mut lines: Vec<WorktreeLine> = shown.into_iter().map(WorktreeLine::Place).collect();
        lines.push(WorktreeLine::More(places.len() - room));
        lines
    }

    /// 每帧绘制提交视图之前调用：工作树一组的终点高度变化（工作树增加、减少，换到另一个
    /// 仓库）时，从当前显示的高度开始过渡；仓库的状态刚读到时直接到位。
    pub(crate) fn track_worktree_height(&mut self) {
        let (settled, clock) = (self.worktree_settled_height(), self.section_clock());
        self.git.commit.worktrees.track(settled, clock);
    }

    /// 底部各组的终点高度变化时过渡用的时钟。上一帧还没有读到仓库的状态（启动、打开目录）时
    /// 为 `None`：各组本来就在那里，直接到位。
    pub(crate) fn section_clock(&mut self) -> Option<crate::app::animation::Clock> {
        if !self.git.commit.sections_known {
            return None;
        }
        self.clock(crate::app::animation::TREE_ANIMATION)
    }

    /// 每帧各组的高度都跟踪过以后调用：记下这一帧仓库的状态是否已经读到。
    pub(crate) fn sections_tracked(&mut self) {
        self.git.commit.sections_known = self.git.status.is_some();
    }

    /// 工作树一组占的高度（逻辑像素）：收着时是标题一行，展开时再加上 [`App::worktree_lines`]
    /// 各行；展开、收起的途中按时钟过渡。当前仓库只有一个工作树时为 0。
    pub(crate) fn worktree_section_height(&self) -> f32 {
        (self.git.commit.worktrees).height(self.worktree_settled_height())
    }

    /// 工作树一组不在过渡中时的高度（逻辑像素），见 [`App::worktree_section_height`]。
    fn worktree_settled_height(&self) -> f32 {
        if !self.git_has_worktrees() {
            return 0.0;
        }
        if self.git.commit.worktrees.open {
            DRAWER_CLOSED + self.worktree_lines().len() as f32 * COMMIT_ROW
        } else {
            DRAWER_CLOSED
        }
    }

    /// 工作树一组标题上的箭头转到哪儿，见 [`Drawer::chevron`]。
    pub(crate) fn worktree_chevron(&self) -> f32 {
        let span = self.worktree_lines().len() as f32 * COMMIT_ROW;
        (self.git.commit.worktrees).chevron(self.worktree_settled_height(), span)
    }

    /// 点了工作树一组的标题：收起或展开，从现在的高度过渡过去。
    fn toggle_worktrees(&mut self) {
        let from = self.worktree_section_height();
        let clock = self.clock(crate::app::animation::TREE_ANIMATION);
        self.git.commit.worktrees.toggle(from, clock);
        self.git.commit.worktrees.settled = self.worktree_settled_height();
        self.git.commit.scroll = self.git.commit.scroll.min(self.commit_max_scroll());
    }

    /// 处理工作树一组中的目标：展开、切换、菜单和新建。
    pub(super) fn press_commit_worktree(&mut self, target: CommitTarget) {
        // 「⋯」只在悬停时画出：没有它的位置时菜单弹在这一行的右端。
        let at = |wanted: CommitTarget| {
            let hit = (self.hits.commit.iter()).find(|(_, t)| *t == wanted);
            hit.map(|(rect, _)| *rect)
        };
        let rect = at(target).or_else(|| match target {
            CommitTarget::WorktreeMenu(i) => at(CommitTarget::WorktreeRow(i)),
            _ => None,
        });
        match target {
            CommitTarget::Worktrees => self.toggle_worktrees(),
            CommitTarget::WorktreeNew => self.open_worktree_dialog(),
            CommitTarget::WorktreeMore => self.open_places(false),
            CommitTarget::WorktreeRow(i) | CommitTarget::WorktreeMenu(i) => {
                let Some(place) = self.worktree_places().get(i).cloned() else {
                    return;
                };
                // 目录已不在的工作树打不开：点这一行同样弹出菜单，里面只有清理。
                let menu = matches!(target, CommitTarget::WorktreeMenu(_)) || place.missing;
                match (menu, place.worktree, rect) {
                    (true, _, Some(rect)) => {
                        let target = MenuTarget::Location(place.path, place.worktree);
                        let at = (rect.right() - rect.h, rect.bottom());
                        self.open_context_menu(target, at);
                    }
                    (false, Some(index), _) => self.open_worktree_asking(index),
                    _ => {}
                }
            }
            // 其余目标由 [`Self::press_commit`] 交给别的函数。
            _ => {}
        }
        self.request_redraw();
    }
}
