//! 设置浮层的状态类型：选中项、编辑中的控件、动画与缓存。

use crate::app::*;
use crate::keymap;
use crate::layout::ToolLanguages;

use super::{FormatRow, shortcuts::KeyEdit};

/// 设置浮层的状态。
#[derive(Debug, Default)]
pub struct SettingsState {
    /// 浮层是否打开。
    ///
    /// 打开时覆盖整个窗口，键盘和鼠标都先交给浮层。
    pub open: bool,
    /// 当前选中的设置项，由键盘上下键或鼠标点击选中；右侧显示的是该设置项所在的那一页。
    pub selected: Setting,
    /// 是否用键盘移动过选中项。
    ///
    /// 只有此时才给选中的那一行铺底色，使用鼠标时不显示。
    pub keyboard: bool,
    /// 正在编辑字体名或格式化命令时，对应的设置项和输入框的当前内容。
    pub editing: Option<(Setting, TextField)>,
    /// 开始编辑前的字体设置；按 `Esc` 或关闭浮层时退回该值。
    pub editing_original: Option<String>,
    /// 字体名无法识别等错误：出错的设置项和原因，原因用错误色写在该行输入框的左侧。
    ///
    /// 离开输入框后，输入退回原值，原因仍留在该行，换页、再次编辑或关闭浮层时清除。
    pub error: Option<(Setting, String)>,
    /// 当前页内容的纵向滚动位置（逻辑像素），换页时回到顶部。
    pub scroll: f32,
    /// 鼠标悬停的部件。
    pub hover: Option<SettingsPart>,
    /// 取值与默认不同的设置项：名称后面有「恢复默认」。打开设置和配置变化时重新算。
    pub changed: Vec<Setting>,
    /// 格式化页列出的语言。
    ///
    /// 打开设置时确定，打开期间只会增加，不会减少，已列出的语言也不换位置。
    pub languages: ToolLanguages,
    /// 格式化页每一行（格式化工具、修复工具）上显示的内容。
    pub tool_rows: Vec<(Setting, FormatRow)>,
    /// 正在后台查询 WSL 发行版中装了哪些工具，查完后重新计算各行。
    pub probing: bool,
    /// 分类栏顶部的搜索框：按设置项的名称和说明搜索，不为空白时正文换成搜索结果。
    pub search: TextField,
    /// 「关于」页顶部的产品图标：渲染时的边长（物理像素）和 RGBA 像素；关闭浮层时释放。
    ///
    /// 渲染失败时像素为空，不再重试，图标不显示。
    pub logo: Option<(u32, Vec<u8>)>,
    /// 这次显示「关于」页是否已经播放过图标的动画；离开该页或重新打开设置后再播放一次。
    pub logo_played: bool,
    /// 快捷键页的搜索框：按命令名或按键搜索。
    pub keys_query: TextField,
    /// 是否正在录制按键：按下的键写入搜索框，不执行。
    pub recording: bool,
    /// 搜索框中的内容是否为录制的按键（按键搜索，而不是按名称搜索）。
    pub keys_recorded: bool,
    /// 「快捷键」页中正在修改的一行。
    pub(crate) key_edit: Option<KeyEdit>,
    /// 「快捷键」页中一行展开、收起的过渡：同时收起的另一行和时钟；展开的是 `key_edit` 的一行。
    pub key_slide: Option<(Option<keymap::Action>, super::super::animation::Clock)>,
    /// 「导入…」菜单列出的来源，打开菜单时查找。
    pub(crate) import_sources: Vec<crate::keymap::import::Source>,
    /// 浮层正在出现（`true`）或消失（`false`）的动画：出现时遮罩淡入，卡片整张淡入、从 95%
    /// 放大到原大；消失时整张淡出、略微缩小。
    ///
    /// 消失期间 `open` 已经是 `false`，键盘和鼠标已经交回主界面，此时仍需绘制最后一帧。
    pub motion: Option<(bool, super::super::animation::Clock)>,
    /// 「模式」换了以后，凸起的那一段从原来那一段（`ThemeChoice`）滑过去。
    pub mode_slide: Option<(ThemeChoice, super::super::animation::Clock)>,
    /// 刚切换的开关（`Setting`）：圆钮滑到另一侧，底色渐变到新状态的颜色。
    pub switch_slide: Option<(Setting, super::super::animation::Clock)>,
    /// 出现、消失动画用的快照，动画开头画一次存下来，动画结束就释放。
    pub cache: Option<DialogCache>,
    /// 设置页滚动或拖动期间保存的窗口画面（含遮罩），用于代替逐帧重绘浮层下方的界面。
    pub scroll_base: Option<winct_render::Snapshot>,
    /// 最近一次滚动或拖动的时刻；两者都未进行时为 `None`。
    pub scrolled_at: Option<Instant>,
    /// 卡片相对窗口中央的偏移（逻辑像素），拖动标题行时改变；窗口存在期间保留，再次打开时
    /// 卡片仍在原处。
    pub offset: (f32, f32),
}

/// 设置浮层出现、消失的动画用的两张快照：动画开头画一次，途中每帧直接拿来用。
#[derive(Debug)]
pub struct DialogCache {
    /// 浮层底下的整个窗口（主界面，不含浮层）。
    pub base: winct_render::Snapshot,
    /// 画好的卡片：原大、不透明，不含阴影。
    pub card: winct_render::Snapshot,
}
