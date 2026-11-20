//! 日志里提交菜单上改写历史的操作：编辑提交说明、删除提交、推送此前所有提交。
//!
//! 范围和要推送的提交先在后台读出，读到以后再打开对话框或请用户确认。

use winc_editor::git::{Outgoing, RebaseAction, RebasePlan, RebaseStep};
use winc_i18n::t;

use super::branch::Then;
use super::commit::{PUSH_PREVIEW, PushIntent};
use super::form::FormKind;
use super::log::{LogConfirmed, short};
use crate::app::{App, NoticeKind};

/// 在后台读好、等用户处理的改写。
pub(in crate::app) enum Loaded {
    /// 编辑提交说明的范围：打开对话框。
    Reword(RebasePlan),
    /// 删除提交的范围：确认后删除。
    Drop(RebasePlan),
    /// 推送到提交 `oid` 为止：打开推送对话框，列出会送出的提交。
    PushUpTo {
        /// 推送还是发布。
        intent: PushIntent,
        /// 完整的提交号。
        oid: String,
        /// 会送出的提交的标题，最新的在前。
        subjects: Vec<String>,
        /// 会送出的提交数。
        total: usize,
    },
}

impl App {
    /// 读取改写提交 `oid` 的信息的范围，打开编辑提交说明的对话框。
    pub(in crate::app) fn start_reword(&mut self, oid: String) {
        self.git_operate_then(t!("git-op-rebase-load").into(), move |git, root| {
            let plan = git.reword_plan(root, &oid)?;
            Ok((String::new(), Then::History(Box::new(Loaded::Reword(plan)))))
        });
    }

    /// 读取删除提交 `oid` 要重新应用的范围，确认后删除。
    pub(in crate::app) fn start_drop_commit(&mut self, oid: String) {
        self.git_operate_then(t!("git-op-rebase-load").into(), move |git, root| {
            let plan = git.rebase_plan(root, &oid)?;
            Ok((String::new(), Then::History(Box::new(Loaded::Drop(plan)))))
        });
    }

    /// 读取推送到提交 `oid` 为止会送出的提交，打开推送对话框。
    pub(in crate::app) fn start_push_up_to(&mut self, oid: String) {
        let Some(intent) = self.push_intent() else {
            return;
        };
        self.git_operate_then(t!("git-op-rebase-load").into(), move |git, root| {
            git.on_branch(root, &oid)?;
            let target = match intent {
                PushIntent::Publish(_) => Outgoing::Publish,
                _ => Outgoing::Upstream,
            };
            let (subjects, total) = git.outgoing(root, target, &oid, PUSH_PREVIEW)?;
            let loaded = Loaded::PushUpTo {
                intent,
                oid,
                subjects,
                total,
            };
            Ok((String::new(), Then::History(Box::new(loaded))))
        });
    }

    /// 后台读好了 `loaded`。
    pub(super) fn history_loaded(&mut self, loaded: Loaded) {
        match loaded {
            Loaded::Reword(plan) => self.open_reword_dialog(plan),
            Loaded::Drop(plan) if plan.base.is_none() && plan.commits.len() == 1 => {
                self.notify(NoticeKind::Warning, t!("git-rebase-all-dropped"));
            }
            Loaded::Drop(plan) => self.confirm_log(LogConfirmed::DropCommit(Box::new(plan))),
            Loaded::PushUpTo {
                intent,
                oid,
                subjects,
                total,
            } => self.open_push_up_to(intent, oid, subjects, total),
        }
    }

    /// 打开推送到提交 `oid` 为止的推送对话框；`subjects`、`total` 是会送出的提交。
    fn open_push_up_to(
        &mut self,
        intent: PushIntent,
        oid: String,
        subjects: Vec<String>,
        total: usize,
    ) {
        if self.overlays.form_dialog.is_some() {
            return;
        }
        self.open_push_dialog(intent, subjects, total);
        if let Some(dialog) = &mut self.overlays.form_dialog
            && let FormKind::Transfer(form) = &mut dialog.kind
        {
            form.upto = Some(oid);
        }
    }

    /// 删除 `plan` 的第一个提交，之后的提交重新应用。
    pub(in crate::app) fn drop_commit(&mut self, plan: RebasePlan) {
        let steps: Vec<RebaseStep> = (plan.commits.into_iter().enumerate())
            .map(|(i, commit)| RebaseStep {
                oid: commit.oid,
                action: if i == 0 {
                    RebaseAction::Drop
                } else {
                    RebaseAction::Pick
                },
                message: None,
            })
            .collect();
        let Some(first) = steps.first() else {
            return;
        };
        let what = t!("git-op-drop-commit", commit = short(&first.oid));
        let base = plan.base;
        self.git_operate_then(what, move |git, root| {
            let stopped = git.interactive_rebase(root, base.as_deref(), &steps)?;
            let then = match stopped {
                true => Then::Warn(t!("git-rebase-stopped").to_owned()),
                false => Then::Nothing,
            };
            Ok((String::new(), then))
        });
    }
}

/// 删除提交的确认框内容：（标题，说明，确认按钮的文字）。
pub(in crate::app) fn drop_prompt(plan: &RebasePlan) -> (String, String, &'static str) {
    let subject = plan.commits.first().map_or("", |c| c.subject.as_str());
    let mut detail = t!("git-drop-commit-confirm-detail").to_owned();
    let later = plan.commits.len().saturating_sub(1);
    if later > 0 {
        detail.push('\n');
        detail.push_str(&t!("git-drop-commit-confirm-later", count = later));
    }
    if plan.pushed > 0 {
        detail.push('\n');
        detail.push_str(t!("git-drop-commit-confirm-pushed"));
    }
    (
        t!("git-drop-commit-confirm", subject = subject),
        detail,
        t!("git-drop-commit-confirm-button"),
    )
}
