use super::*;

/// 设置浮层的整套几何。
///
/// 绘制、悬停和点击都从这里取位置，保证三者一致。
///
/// 卡片在整个窗口中居中（它覆盖侧栏和编辑区），大小固定，换页时不变化；窗口放不下时缩小到窗口以内。
/// 左侧是分类栏，右侧上方是标题和关闭按钮，下方是这一页的各组设置项，放不下时滚动。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SettingsLayout {
    /// 设置卡片。
    pub card: Rect,
    /// 左侧的分类栏。
    pub nav: Rect,
    /// 分类栏顶部的搜索框。
    pub search: Rect,
    /// 分类栏中的每一项及其位置。
    pub pages: Vec<(SettingsPage, Rect)>,
    /// 正在显示的页。
    pub page: SettingsPage,
    /// 是否在显示搜索结果（搜索框不为空白）：此时正文是 [`search_sections`] 的各组，分类栏中
    /// 不标出当前页。
    pub searching: bool,
    /// 右侧顶部：「设置 › 外观」和关闭按钮。
    pub header: Rect,
    /// 关闭按钮。
    pub close: Rect,
    /// 标题下方可以滚动的区域。
    pub body: Rect,
    /// 每组的底色区域。
    pub groups: Vec<Rect>,
    /// 组上方的标题：标题所在行的位置和标题文字。
    pub headings: Vec<(Rect, &'static str)>,
    /// 组内行与行之间的细线。
    pub separators: Vec<Rect>,
    /// 各设置行。
    pub rows: Vec<SettingRow>,
    /// 内容的总高度（像素），用于计算最大滚动量。
    pub content_h: f32,
}

/// 一行设置：名称在左，控件靠右。
///
/// `row` 是整行，选中时的圆角底色铺满整行。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SettingRow {
    /// 这一行对应的设置项。
    pub setting: Setting,
    /// 整行的矩形。
    pub row: Rect,
    /// 控件的矩形。
    pub control: Rect,
}

impl SettingsLayout {
    /// 计算设置浮层的几何。
    ///
    /// `window` 是窗口矩形，`view` 是当前的界面状态，`measure` 返回一段说明文字的宽度（逻辑
    /// 像素），说明放不下一行时折行，该行随之加高。
    pub fn compute(
        window: Rect,
        view: &SettingsView<'_>,
        measure: &mut dyn FnMut(&str) -> f32,
    ) -> Self {
        let &SettingsView {
            page,
            query,
            languages,
            terminal,
            git,
            editing,
            scroll,
        } = view;
        let searching = !query.trim().is_empty();
        let w = SETTINGS_W.min(window.w - SETTINGS_MARGIN * 2.0).max(0.0);
        let h = SETTINGS_H.min(window.h - SETTINGS_MARGIN * 2.0).max(0.0);
        let card = Rect::new(
            (window.x + (window.w - w) / 2.0).round(),
            (window.y + (window.h - h) / 2.0).round(),
            w,
            h,
        );

        let nav = Rect::new(card.x, card.y, SETTINGS_NAV_W.min(card.w), card.h);
        // 搜索框与右侧标题行垂直居中对齐，各页列在其下。「终端」一项的高度按显示比例收放，
        // 下方各项随之滑动。
        let search = Rect::new(
            nav.x + 8.0,
            card.y + (SETTINGS_HEADER - SETTINGS_CONTROL_H) / 2.0,
            (nav.w - 16.0).max(0.0),
            SETTINGS_CONTROL_H,
        );
        let mut y = search.bottom() + 8.0;
        let pages = SettingsPage::ALL
            .into_iter()
            .filter_map(|page| {
                let share = match page {
                    SettingsPage::Terminal => terminal,
                    SettingsPage::Git => git,
                    _ => 1.0,
                };
                let h = SETTINGS_NAV_ROW * share;
                let row = Rect::new(nav.x + 8.0, y, (nav.w - 16.0).max(0.0), h);
                y += (SETTINGS_NAV_ROW + 2.0) * share;
                (share > 0.0).then_some((page, row))
            })
            .collect();

        let right = Rect::new(nav.right(), card.y, (card.w - nav.w).max(0.0), card.h);
        let header = Rect::new(right.x, right.y, right.w, SETTINGS_HEADER.min(right.h));
        let close = Rect::new(
            header.right() - 12.0 - BUTTON,
            header.y + (header.h - BUTTON) / 2.0,
            BUTTON,
            BUTTON,
        );
        let body = Rect::new(
            right.x,
            header.bottom(),
            right.w,
            (right.bottom() - header.bottom()).max(0.0),
        );

        let (x, group_w) = (
            body.x + SETTINGS_PAD,
            (body.w - SETTINGS_PAD * 2.0).max(0.0),
        );
        let mut y = body.y - scroll;
        let mut groups = Vec::new();
        let mut headings = Vec::new();
        let mut separators = Vec::new();
        let mut rows = Vec::new();

        let sections = if searching {
            let modules = Modules {
                git: git > 0.0,
                terminal: terminal > 0.0,
            };
            search_sections(query, languages, modules)
        } else {
            page.sections(languages)
        };
        for (i, section) in sections.into_iter().enumerate() {
            if i > 0 {
                y += SETTINGS_GROUP_GAP;
            }
            if let Some(title) = section.title {
                // 与组内名称左对齐。
                let line = Rect::new(x + SETTINGS_INSET, y, group_w, SETTINGS_TITLE);
                headings.push((line, title));
                y += SETTINGS_TITLE;
            }
            let (in_group, below): (Vec<Setting>, Vec<Setting>) = section
                .items
                .into_iter()
                .partition(|s| s.control() != SettingControl::Action);

            let top = y;
            for (n, setting) in in_group.into_iter().enumerate() {
                if n > 0 {
                    let inner = group_w - SETTINGS_INSET * 2.0;
                    separators.push(Rect::new(x + SETTINGS_INSET, y, inner.max(0.0), 1.0));
                }
                let editing = editing == Some(setting);
                let h = row_height(setting, Rect::new(x, y, group_w, 0.0), editing, measure);
                let row = Rect::new(x, y, group_w, h);
                let control = control_rect(setting, row, editing);
                rows.push(SettingRow {
                    setting,
                    row,
                    control,
                });
                y += h;
            }
            if y > top {
                groups.push(Rect::new(x, top, group_w, y - top));
            }

            // 组下方的文字按钮：左边与组对齐，悬停底色只覆盖按钮自身的大小。
            for setting in below {
                y += 8.0;
                let row = Rect::new(x, y, SETTINGS_LINK_W.min(group_w), SETTINGS_LINK_H);
                rows.push(SettingRow {
                    setting,
                    row,
                    control: row,
                });
                y += SETTINGS_LINK_H;
            }
        }

        SettingsLayout {
            card,
            nav,
            search,
            pages,
            page,
            searching,
            header,
            close,
            body,
            groups,
            headings,
            separators,
            rows,
            content_h: y + scroll - body.y + SETTINGS_BOTTOM,
        }
    }

    /// 返回 `setting` 所在的行；没有时返回 `None`。
    pub fn row(&self, setting: Setting) -> Option<&SettingRow> {
        self.rows.iter().find(|r| r.setting == setting)
    }

    /// 返回正文的最大滚动量（像素）。
    pub fn max_scroll(&self) -> f32 {
        (self.content_h - self.body.h).max(0.0)
    }

    /// 返回 `(x, y)` 处可点击的部件；落在控件之外时返回 `None`。
    pub fn part_at(&self, x: f32, y: f32) -> Option<SettingsPart> {
        if self.close.contains(x, y) {
            return Some(SettingsPart::Close);
        }
        if self.search.contains(x, y) {
            return Some(SettingsPart::SearchField);
        }

        if let Some(&(page, _)) = self.pages.iter().find(|(_, r)| r.contains(x, y)) {
            return Some(SettingsPart::Page(page));
        }

        if !self.body.contains(x, y) {
            return None;
        }

        let row = self.rows.iter().find(|r| r.control.contains(x, y))?;

        match row.setting.control() {
            SettingControl::Font if font_menu_button(row.control).contains(x, y) => {
                Some(SettingsPart::FontMenu(row.setting))
            }
            SettingControl::Font | SettingControl::Command => {
                Some(SettingsPart::Field(row.setting))
            }
            SettingControl::Switch => Some(SettingsPart::Switch(row.setting)),
            SettingControl::Action | SettingControl::Choice | SettingControl::Button => {
                Some(SettingsPart::Action(row.setting))
            }
            SettingControl::Stepper => {
                let parts = StepperParts::new(row.control);
                if parts.minus.contains(x, y) {
                    Some(SettingsPart::Decrease(row.setting))
                } else if parts.plus.contains(x, y) {
                    Some(SettingsPart::Increase(row.setting))
                } else if row.setting == Setting::AutoSaveDelay && parts.value.contains(x, y) {
                    Some(SettingsPart::Field(row.setting))
                } else {
                    None
                }
            }
            SettingControl::Themes => (0..THEME_CHOICES.len())
                .find(|&i| theme_segment(row.control, i).contains(x, y))
                .map(|i| SettingsPart::Theme(THEME_CHOICES[i].0)),
            SettingControl::ThemeList => {
                let dark = row.setting == Setting::DarkTheme;
                (0..crate::theme::themes(dark).len())
                    .find(|&i| theme_tile(row.control, i).contains(x, y))
                    .map(|i| SettingsPart::NamedTheme(dark, i))
            }
            SettingControl::Product => (ProductParts::new(row.row).version.contains(x, y))
                .then_some(SettingsPart::Copy(row.setting)),
            SettingControl::Link => Some(SettingsPart::Link(row.setting)),
            SettingControl::Search => None,
        }
    }
}
