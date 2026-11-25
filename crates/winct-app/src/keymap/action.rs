//! 命令：显示名、英文名、分组、所属模块、能否在差异和终端里执行，以及是否在命令面板中列出。

use super::*;

mod english;
mod list;
mod titles;

pub use list::ACTIONS;

/// 可以用快捷键或命令面板执行的命令。
///
/// 变体按所属分组（[`Group`]）依次排列；各命令的显示名见 [`Action::title`] 和 [`Action::english`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    ShowCommands,
    QuickOpen,
    GoToSymbol,
    GoToLine,
    GoToDefinition,
    GoToDeclaration,
    GoToTypeDefinition,
    GoToImplementation,
    ShowHover,
    OpenLink,
    FindReferences,
    TriggerCompletion,
    SignatureHelp,
    GoBack,
    GoForward,
    Settings,
    ExportSettings,
    ImportSettings,
    ReleaseNotes,
    KeyboardShortcuts,
    RecordKeys,

    NewFile,
    NewWindow,
    OpenFile,
    OpenFolder,
    /// 把一个目录加为窗口的附加目录。
    AddFolder,
    /// 把窗口中的目录另存为项目文件。
    SaveProjectAs,
    /// 选择项目文件并打开。
    OpenProject,
    RemoteConnect,
    RemoteDisconnect,
    OpenRecent,
    Save,
    SaveAs,
    CloseTab,
    CloseOtherTabs,
    CloseAllTabs,
    ReopenClosed,
    NextTab,
    PreviousTab,
    FocusTab1,
    FocusTab2,
    FocusTab3,
    FocusTab4,
    FocusTab5,
    FocusTab6,
    FocusTab7,
    FocusTab8,
    FocusTab9,
    /// 按最近使用的顺序切换标签页（见 `App::switch_tab`）。
    SwitchTab,
    /// 按最近使用的顺序反向切换标签页。
    SwitchTabBack,
    SplitRight,
    SplitDown,
    MoveToSplitRight,
    MoveToSplitDown,
    Unsplit,
    NextPane,
    PreviousPane,
    CloseFolder,
    CloseWindow,
    CopyPath,
    CopyRelativePath,
    CopyReference,
    CopyAbsoluteReference,
    RevealInFinder,
    CopyActivePath,
    CopyKeyPath,
    RevealActiveFile,
    OpenInTerminal,
    Quit,

    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    ToggleComment,
    /// Markdown 中切换粗体（`**`）。
    MarkdownBold,
    /// Markdown 中切换斜体（`*`）。
    MarkdownItalic,
    /// Markdown 中切换行内代码（`` ` ``）。
    MarkdownCode,
    /// Markdown 中切换删除线（`~~`）。
    MarkdownStrikethrough,
    /// Markdown 中切换高亮（`==`）。
    MarkdownHighlight,
    /// Markdown 中设为一级标题；已经是时改回正文。
    MarkdownHeading1,
    /// Markdown 中设为二级标题。
    MarkdownHeading2,
    /// Markdown 中设为三级标题。
    MarkdownHeading3,
    /// Markdown 中设为四级标题。
    MarkdownHeading4,
    /// Markdown 中设为五级标题。
    MarkdownHeading5,
    /// Markdown 中设为六级标题。
    MarkdownHeading6,
    /// Markdown 中标题改回正文。
    MarkdownParagraph,
    /// Markdown 中选中的各行加上或去掉一层引用。
    MarkdownQuote,
    /// Markdown 中选中的各行改为有序列表，或去掉列表记号。
    MarkdownOrderedList,
    /// Markdown 中选中的各行改为无序列表，或去掉列表记号。
    MarkdownBulletList,
    /// Markdown 中把选中的各行放进代码块，或插入一个空的代码块。
    MarkdownCodeBlock,
    /// Markdown 中把选中的各行放进公式块，或插入一个空的公式块。
    MarkdownMathBlock,
    /// Markdown 中插入两列的表格。
    MarkdownTable,
    /// Markdown 中把选中的文字做成链接；剪贴板里是网址时作为地址。
    MarkdownLink,
    /// Markdown 中插入图片；选中的文字作为替代文字。
    MarkdownImage,
    /// Markdown 中切换专注模式。
    MarkdownFocusMode,
    /// Markdown 中切换打字机模式。
    MarkdownTypewriterMode,
    /// Markdown 中粘贴为纯文本。
    MarkdownPastePlain,
    DeleteLine,
    DeleteAllLeft,
    /// 与方向键、`Home` `End`、`⌫` `⌦` 相同的命令：macOS 上两套方案按各自编辑器的默认键位绑定
    /// `⌃` 字母键（见 [`App::press_editor_key`](crate::app::App)）。
    CursorLeft,
    CursorRight,
    CursorUp,
    CursorDown,
    CursorHome,
    CursorEnd,
    /// 光标移到这一行的第 0 列、末尾：不按折行分段，也不停在首个非空白字符处。
    CursorLineStart,
    CursorLineEnd,
    DeleteLeft,
    DeleteRight,
    DeleteAllRight,
    /// 剪切光标到行尾的文字；光标在行尾时剪切换行符。
    CutToLineEnd,
    TransposeLetters,
    /// 在光标处换行，光标留在原处。
    InsertLineBreak,
    /// 把光标所在行滚到编辑区中间。
    ScrollToCenter,
    IndentLines,
    OutdentLines,
    MoveLinesUp,
    MoveLinesDown,
    CopyLinesUp,
    CopyLinesDown,
    JoinLines,
    ExpandSelection,
    ShrinkSelection,
    AddCursorAbove,
    AddCursorBelow,
    AddNextOccurrence,
    SkipToNextOccurrence,
    SelectAllOccurrences,
    RemoveSecondaryCursors,
    AddCursorsToLineEnds,
    CursorUndo,
    CursorRedo,
    ColumnSelectUp,
    ColumnSelectDown,
    ColumnSelectLeft,
    ColumnSelectRight,
    JumpToBracket,
    TransformToUppercase,
    TransformToLowercase,
    TransformToTitlecase,
    SortLinesAscending,
    SortLinesDescending,
    TrimTrailingWhitespace,
    /// 把 `\uXXXX` 转义换成字符，或把 ASCII 以外的字符写成转义；没有选区时转换整个文件。
    UnescapeUnicode,
    EscapeUnicode,
    IndentUsingSpaces,
    IndentUsingTabs,
    ConvertIndentationToSpaces,
    ConvertIndentationToTabs,
    DetectIndentation,
    ChangeEndOfLine,
    ChangeLanguageMode,
    Fold,
    Unfold,
    ToggleFold,
    FoldRecursively,
    UnfoldRecursively,
    FoldAll,
    UnfoldAll,
    FoldLevel1,
    FoldLevel2,
    FoldLevel3,
    FoldLevel4,
    FoldLevel5,
    FoldLevel6,
    FoldLevel7,
    ToggleBookmark,
    NextBookmark,
    PreviousBookmark,
    ListBookmarks,
    ClearBookmarks,
    Format,
    FormatSelection,
    Fix,

    Find,
    Replace,
    FindNext,
    FindPrevious,
    HighlightWord,
    ClearWordHighlights,
    ReplaceOne,
    ReplaceAll,
    SelectAllMatches,
    ToggleCase,
    ToggleWord,
    ToggleRegex,
    FilterLines,
    FindInFiles,
    OpenSearchDialog,
    ReplaceInFiles,
    ReplaceAllInFiles,
    NextResult,
    PreviousResult,

    RenameFile,
    DeleteFile,
    DeleteFilePermanently,
    OpenSelected,
    PreviewSelected,
    FindInFolder,

    ToggleSidebar,
    FocusSidebar,
    ShowFiles,
    ShowOutline,
    IncreaseFontSize,
    DecreaseFontSize,
    ResetFontSize,
    ZoomIn,
    ZoomOut,
    ResetZoom,
    ThemeAuto,
    ThemeLight,
    ThemeDark,
    ToggleStickyScroll,
    /// 开启或关闭 Vim 模式。
    ToggleVimMode,
    ToggleWordWrap,
    /// 显示或隐藏编辑区右侧的缩略图。
    ToggleMinimap,
    /// 显示或隐藏行号。
    ToggleLineNumbers,
    /// 显示或隐藏状态栏。
    ToggleStatusBar,
    /// 全部显示或不显示空白符。
    ToggleWhitespace,
    TogglePreview,
    /// 把当前标签设为只读或恢复为可以编辑。
    ToggleReadOnly,
    /// 把窗口里的全部标签设为只读或恢复。
    ToggleReadOnlyAll,
    NextDifference,
    PreviousDifference,
    /// 差异里的下一个文件。
    NextDiffFile,
    /// 差异里的上一个文件。
    PreviousDiffFile,
    /// 切换差异的单栏显示和并排显示。
    ToggleDiffLayout,
    GitShowChanges,
    /// 打开「未提交的更改」标签：所有未提交的更改按文件依次排在一个标签里。
    GitShowAllChanges,
    /// 点击提交视图中的文件时是否在「未提交的更改」标签中查看（`git.combinedDiff`）。
    GitToggleCombinedDiff,
    /// 更改列表：显示选中文件的差异，焦点移到差异上。
    ChangesShowDiff,
    /// 更改列表：在编辑区打开选中的文件。
    ChangesOpenSource,
    /// 更改列表：回滚选中的文件（先确认）。
    ChangesRollback,
    GitShowLog,
    GitGoToRevision,
    /// 日志：编辑选中提交的说明。
    GitRewordCommit,
    /// 日志：转到选中提交的子提交。
    GitGoToChildCommit,
    /// 日志：转到选中提交的父提交（合并提交是第一个父提交）。
    GitGoToParentCommit,
    /// 在只读标签中打开 Git 操作的输出。
    GitShowOutput,
    /// 克隆仓库。
    GitClone,
    /// 在当前目录中初始化仓库。
    GitInit,
    /// 打开提交视图并展开位置选择器。
    GitSwitchRepository,
    GitSwitchWorktree,
    GitNewWorktree,
    /// 展开分支选择器（提交视图里从分支行，否则从状态栏）。
    GitSwitchBranch,
    /// 打开新建分支的对话框（从当前分支）。
    GitNewBranch,
    /// 获取远程仓库的更新。
    GitFetch,
    /// 拉取当前分支的上游。
    GitPull,
    /// 拉取当前分支的上游并变基。
    GitPullRebase,
    /// 打开拉取对话框：选远程和分支拉取到当前分支。
    GitPullFrom,
    /// 推送当前分支（没有上游时推上去并设为上游）。
    GitPush,
    /// 打开推送对话框：选远程和目标分支推送。
    GitPushTo,
    /// 和上游同步：先拉取再推送。
    GitSync,
    /// 储藏工作区里的更改（输入说明）。
    GitStash,
    /// 把全部更改加入暂存区，未跟踪的文件除外。
    GitStageAll,
    /// 把暂存区中的全部内容移出暂存区。
    GitUnstageAll,
    /// 把当前文件的更改加入暂存区。
    GitStageFile,
    GitFileHistory,
    /// 当前文件中选中的几行的历史。
    GitSelectionHistory,
    GitBlame,
    ShowCommit,
    GitRevertChange,
    ToggleTerminal,
    ShowProblems,
    /// 显示或隐藏终端面板，与焦点位置无关。
    ToggleTerminalPanel,
    /// 启用终端模块；模块关闭时列出，使内置终端可以从命令面板找到。
    EnableTerminal,
    /// 运行当前选中的配置；运行中时重新运行。
    RunConfiguration,
    /// 停止当前选中的配置。
    StopConfiguration,
    /// 打开运行配置文件。
    EditRunConfigurations,
    /// 发送 `.http` 文件中光标所在的请求（见 `App::send_http_request`）。
    SendHttpRequest,
    ClearTerminal,
    /// 复制终端的选区。
    TerminalCopy,
    /// 把剪贴板粘贴到终端（见 `App::terminal_paste`）。
    TerminalPaste,
    NewTerminal,
    CloseTerminal,
    NextTerminal,
    PreviousTerminal,
    SplitTerminal,
    /// 终端：把正在看的标签移到正在用的一栏后面新加的一栏（见 `App::move_terminal_to_new_group`）。
    MoveTerminalToGroup,
    NextTerminalPane,
    PreviousTerminalPane,
    FocusTerminal1,
    FocusTerminal2,
    FocusTerminal3,
    FocusTerminal4,
    FocusTerminal5,
    FocusTerminal6,
    FocusTerminal7,
    FocusTerminal8,
    FocusTerminal9,
    FindInTerminal,
    MaximizeTerminal,
    /// 把终端翻到回滚缓冲的最上面。
    ScrollTerminalTop,
    /// 把终端翻回最新的输出。
    ScrollTerminalBottom,
    ToggleTerminalPosition,
    InstallShellCommand,
    UninstallShellCommand,
    RenameTerminal,
    PreviousCommand,
    NextCommand,
    FoldOutput,
    UnfoldOutput,
    NextError,
    PreviousError,
    SendToTerminal,
}

/// 可选模块。
///
/// 属于模块的命令只在模块开着时出现在命令面板里、响应快捷键。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Module {
    /// Git 模块。
    Git,
    /// 终端模块。
    Terminal,
    /// 远程模块。
    Remote,
}

impl Action {
    /// 按显示顺序聚焦第 1 至第 9 个终端的命令，供数字键绑定和编号查询共用。
    pub const TERMINAL_FOCUS: [Self; 9] = [
        Self::FocusTerminal1,
        Self::FocusTerminal2,
        Self::FocusTerminal3,
        Self::FocusTerminal4,
        Self::FocusTerminal5,
        Self::FocusTerminal6,
        Self::FocusTerminal7,
        Self::FocusTerminal8,
        Self::FocusTerminal9,
    ];

    /// 按顺序切到编辑区第 1 至第 9 个标签页的命令，供数字键绑定和编号查询共用。
    pub const TAB_FOCUS: [Self; 9] = [
        Self::FocusTab1,
        Self::FocusTab2,
        Self::FocusTab3,
        Self::FocusTab4,
        Self::FocusTab5,
        Self::FocusTab6,
        Self::FocusTab7,
        Self::FocusTab8,
        Self::FocusTab9,
    ];

    /// 返回切换标签页命令的标签下标（从 0 起），其他命令返回 `None`。
    pub fn tab_index(self) -> Option<usize> {
        Self::TAB_FOCUS.iter().position(|action| *action == self)
    }

    /// 返回终端聚焦命令的标签下标（从 0 起），其他命令返回 `None`。
    pub fn terminal_index(self) -> Option<usize> {
        Self::TERMINAL_FOCUS
            .iter()
            .position(|action| *action == self)
    }

    /// 返回命令在配置文件中的名字：变体名，首字母小写（如 `quickOpen`）。
    pub fn id(self) -> String {
        let name = format!("{self:?}");
        let mut chars = name.chars();
        chars.next().map_or_else(String::new, |first| {
            first.to_ascii_lowercase().to_string() + chars.as_str()
        })
    }

    /// 按配置文件中的名字 `id` 返回命令；不认识时返回 `None`。
    pub fn from_id(id: &str) -> Option<Action> {
        ACTIONS.iter().copied().find(|action| action.id() == id)
    }

    /// 返回命令所属的分组。
    pub fn group(self) -> Group {
        use Action::*;
        match self {
            ShowCommands
            | QuickOpen
            | GoToSymbol
            | GoToLine
            | GoToDefinition
            | GoToDeclaration
            | GoToTypeDefinition
            | GoToImplementation
            | ShowHover
            | FindReferences
            | TriggerCompletion
            | SignatureHelp
            | GoBack
            | GoForward
            | Settings
            | ExportSettings
            | ImportSettings
            | ReleaseNotes
            | KeyboardShortcuts
            | RecordKeys
            | InstallShellCommand
            | UninstallShellCommand
            | OpenLink => Group::General,
            NewFile | OpenFile | OpenFolder | OpenProject | AddFolder | SaveProjectAs
            | RemoteConnect | RemoteDisconnect | Save | SaveAs | CloseTab | CloseOtherTabs
            | CloseAllTabs | ReopenClosed | NextTab | PreviousTab | FocusTab1 | FocusTab2
            | FocusTab3 | FocusTab4 | FocusTab5 | FocusTab6 | FocusTab7 | FocusTab8 | FocusTab9
            | SwitchTab | SwitchTabBack | CloseFolder | CloseWindow | CopyPath
            | CopyRelativePath | RevealInFinder | CopyActivePath | CopyKeyPath
            | RevealActiveFile | OpenInTerminal | Quit => Group::File,
            NewWindow | OpenRecent | CopyReference | CopyAbsoluteReference => Group::File,
            Undo
            | Redo
            | Cut
            | Copy
            | Paste
            | SelectAll
            | ToggleComment
            | MarkdownBold
            | MarkdownItalic
            | MarkdownCode
            | MarkdownStrikethrough
            | MarkdownHighlight
            | MarkdownHeading1
            | MarkdownHeading2
            | MarkdownHeading3
            | MarkdownHeading4
            | MarkdownHeading5
            | MarkdownHeading6
            | MarkdownParagraph
            | MarkdownQuote
            | MarkdownOrderedList
            | MarkdownBulletList
            | MarkdownCodeBlock
            | MarkdownMathBlock
            | MarkdownTable
            | MarkdownLink
            | MarkdownImage
            | MarkdownFocusMode
            | MarkdownTypewriterMode
            | MarkdownPastePlain
            | DeleteLine
            | DeleteAllLeft
            | CursorLeft
            | CursorRight
            | CursorUp
            | CursorDown
            | CursorHome
            | CursorEnd
            | CursorLineStart
            | CursorLineEnd
            | DeleteLeft
            | DeleteRight
            | DeleteAllRight
            | CutToLineEnd
            | TransposeLetters
            | InsertLineBreak
            | ScrollToCenter
            | IndentLines
            | OutdentLines
            | MoveLinesUp
            | MoveLinesDown
            | CopyLinesUp
            | CopyLinesDown
            | JoinLines
            | ExpandSelection
            | ShrinkSelection
            | AddCursorAbove
            | AddCursorBelow
            | AddNextOccurrence
            | SkipToNextOccurrence
            | SelectAllOccurrences
            | RemoveSecondaryCursors
            | AddCursorsToLineEnds
            | CursorUndo
            | CursorRedo
            | ColumnSelectUp
            | ColumnSelectDown
            | ColumnSelectLeft
            | ColumnSelectRight
            | JumpToBracket
            | TransformToUppercase
            | TransformToLowercase
            | TransformToTitlecase
            | SortLinesAscending
            | SortLinesDescending
            | TrimTrailingWhitespace
            | UnescapeUnicode
            | EscapeUnicode
            | IndentUsingSpaces
            | IndentUsingTabs
            | ConvertIndentationToSpaces
            | ConvertIndentationToTabs
            | DetectIndentation
            | ChangeEndOfLine
            | ChangeLanguageMode
            | Fold
            | Unfold
            | ToggleFold
            | FoldRecursively
            | UnfoldRecursively
            | FoldAll
            | UnfoldAll
            | FoldLevel1
            | FoldLevel2
            | FoldLevel3
            | FoldLevel4
            | FoldLevel5
            | FoldLevel6
            | FoldLevel7
            | ToggleBookmark
            | NextBookmark
            | PreviousBookmark
            | ListBookmarks
            | ClearBookmarks
            | Format
            | FormatSelection
            | Fix => Group::Edit,
            Find | Replace | FindNext | FindPrevious | HighlightWord | ClearWordHighlights
            | ReplaceOne | ReplaceAll | SelectAllMatches | ToggleCase | ToggleWord
            | ToggleRegex | FilterLines | FindInFiles | OpenSearchDialog | ReplaceInFiles
            | ReplaceAllInFiles | NextResult | PreviousResult => Group::Find,
            RenameFile
            | DeleteFile
            | DeleteFilePermanently
            | OpenSelected
            | PreviewSelected
            | FindInFolder => Group::Tree,
            ToggleSidebar
            | FocusSidebar
            | ShowFiles
            | ShowOutline
            | IncreaseFontSize
            | DecreaseFontSize
            | ResetFontSize
            | ZoomIn
            | ZoomOut
            | ResetZoom
            | ThemeAuto
            | ThemeLight
            | ThemeDark
            | ToggleStickyScroll
            | ToggleVimMode
            | ToggleWordWrap
            | ToggleMinimap
            | ToggleLineNumbers
            | ToggleStatusBar
            | ToggleWhitespace
            | TogglePreview
            | ToggleReadOnly
            | ToggleReadOnlyAll
            | NextDifference
            | PreviousDifference
            | NextDiffFile
            | PreviousDiffFile
            | ToggleDiffLayout
            | GitShowChanges
            | GitShowAllChanges
            | GitToggleCombinedDiff
            | ChangesShowDiff
            | ChangesOpenSource
            | ChangesRollback
            | GitShowLog
            | GitGoToRevision
            | GitRewordCommit
            | GitGoToChildCommit
            | GitGoToParentCommit
            | GitShowOutput
            | GitClone
            | GitInit
            | GitSwitchRepository
            | GitSwitchWorktree
            | GitNewWorktree
            | GitSwitchBranch
            | GitNewBranch
            | GitFetch
            | GitPull
            | GitPullRebase
            | GitPullFrom
            | GitPush
            | GitPushTo
            | GitSync
            | GitStash
            | GitStageAll
            | GitUnstageAll
            | GitStageFile
            | GitFileHistory
            | GitSelectionHistory
            | GitBlame
            | ShowCommit
            | GitRevertChange
            | ToggleTerminal
            | ShowProblems
            | ToggleTerminalPanel
            | EnableTerminal
            | RunConfiguration
            | StopConfiguration
            | EditRunConfigurations
            | SendHttpRequest
            | ClearTerminal
            | TerminalCopy
            | TerminalPaste
            | NewTerminal
            | CloseTerminal
            | NextTerminal
            | PreviousTerminal
            | SplitTerminal
            | MoveTerminalToGroup
            | NextTerminalPane
            | PreviousTerminalPane
            | FocusTerminal1
            | FocusTerminal2
            | FocusTerminal3
            | FocusTerminal4
            | FocusTerminal5
            | FocusTerminal6
            | FocusTerminal7
            | FocusTerminal8
            | FocusTerminal9
            | FindInTerminal
            | MaximizeTerminal
            | ScrollTerminalTop
            | ScrollTerminalBottom
            | ToggleTerminalPosition
            | RenameTerminal
            | PreviousCommand
            | NextCommand
            | FoldOutput
            | UnfoldOutput
            | NextError
            | PreviousError
            | SendToTerminal
            | SplitRight
            | SplitDown
            | MoveToSplitRight
            | MoveToSplitDown
            | Unsplit
            | NextPane
            | PreviousPane => Group::View,
        }
    }

    /// 是否是在差异里导航的命令：跳到改动、切换文件。
    pub fn navigates_diff(self) -> bool {
        use Action::*;
        matches!(
            self,
            NextDifference | PreviousDifference | NextDiffFile | PreviousDiffFile
        )
    }

    /// 是否为只在编辑 Markdown 时可用的命令：行内格式、段落类型、插入块和链接，以及专注、
    /// 打字机模式。
    pub fn markdown_only(self) -> bool {
        use Action::*;
        matches!(
            self,
            MarkdownBold
                | MarkdownItalic
                | MarkdownCode
                | MarkdownStrikethrough
                | MarkdownHighlight
                | MarkdownHeading1
                | MarkdownHeading2
                | MarkdownHeading3
                | MarkdownHeading4
                | MarkdownHeading5
                | MarkdownHeading6
                | MarkdownParagraph
                | MarkdownQuote
                | MarkdownOrderedList
                | MarkdownBulletList
                | MarkdownCodeBlock
                | MarkdownMathBlock
                | MarkdownTable
                | MarkdownLink
                | MarkdownImage
                | MarkdownFocusMode
                | MarkdownTypewriterMode
                | MarkdownPastePlain
        )
    }

    /// 光标在差异标签右侧时，是否把命令交给右侧对应的源文档执行。
    ///
    /// 「编辑」分组的命令默认都交给右侧，新加的编辑命令在差异里也能直接用；另外加上保存和「选中全部
    /// 匹配」。等同于方向键、删除键的命令照真的按键处理，不在此列。排除的还有不改动文字、只作用于编
    /// 辑器视图或文件设置的命令：滚到中间、折叠与书签只对普通编辑器的视图有意义；缩进方式、缩进
    /// 转换、检测缩进、换行符和语言模式改的是整份文件的设置，不在差异里执行。
    pub fn edits_in_diff(self) -> bool {
        use Action::*;
        match self {
            Save | SelectAllMatches => true,
            Fold
            | Unfold
            | ToggleFold
            | FoldRecursively
            | UnfoldRecursively
            | FoldAll
            | UnfoldAll
            | FoldLevel1
            | FoldLevel2
            | FoldLevel3
            | FoldLevel4
            | FoldLevel5
            | FoldLevel6
            | FoldLevel7
            | ToggleBookmark
            | NextBookmark
            | PreviousBookmark
            | ListBookmarks
            | ClearBookmarks
            | IndentUsingSpaces
            | IndentUsingTabs
            | ConvertIndentationToSpaces
            | ConvertIndentationToTabs
            | DetectIndentation
            | ChangeEndOfLine
            | ChangeLanguageMode
            | CursorLeft
            | CursorRight
            | CursorUp
            | CursorDown
            | CursorHome
            | CursorEnd
            | DeleteLeft
            | DeleteRight
            | ScrollToCenter
            | MarkdownBold
            | MarkdownItalic
            | MarkdownCode
            | MarkdownStrikethrough
            | MarkdownHighlight
            | MarkdownHeading1
            | MarkdownHeading2
            | MarkdownHeading3
            | MarkdownHeading4
            | MarkdownHeading5
            | MarkdownHeading6
            | MarkdownParagraph
            | MarkdownQuote
            | MarkdownOrderedList
            | MarkdownBulletList
            | MarkdownCodeBlock
            | MarkdownMathBlock
            | MarkdownTable
            | MarkdownLink
            | MarkdownImage
            | MarkdownFocusMode
            | MarkdownTypewriterMode
            | MarkdownPastePlain => false,
            _ => self.group() == Group::Edit,
        }
    }

    /// 焦点在终端里时，带 ⌘ 的全局快捷键是否还作用于这个命令。
    ///
    /// 编辑和查找的命令针对编辑区里的文档，在终端里按下不能改到看不见的文档，交给终端
    /// （粘贴由终端自己处理；复制、全选在终端里另有绑定）。按最近使用切换标签页的两个方向都
    /// 交给终端：其默认键单按 `Ctrl`，正向本来就交给终端程序。
    pub fn reaches_terminal(self) -> bool {
        let switch = matches!(self, Action::SwitchTab | Action::SwitchTabBack);
        self == Action::Paste || !switch && !matches!(self.group(), Group::Edit | Group::Find)
    }

    /// 命令属于哪个可选模块；不属于任何模块的命令总是可用。
    pub fn module(self) -> Option<Module> {
        match self {
            Action::GitShowChanges
            | Action::GitShowAllChanges
            | Action::GitToggleCombinedDiff
            | Action::ChangesShowDiff
            | Action::ChangesOpenSource
            | Action::ChangesRollback
            | Action::GitShowLog
            | Action::GitGoToRevision
            | Action::GitRewordCommit
            | Action::GitGoToChildCommit
            | Action::GitGoToParentCommit
            | Action::GitShowOutput
            | Action::GitClone
            | Action::GitInit
            | Action::GitSwitchRepository
            | Action::GitSwitchWorktree
            | Action::GitNewWorktree
            | Action::GitSwitchBranch
            | Action::GitNewBranch
            | Action::GitFetch
            | Action::GitPull
            | Action::GitPullRebase
            | Action::GitPullFrom
            | Action::GitPush
            | Action::GitPushTo
            | Action::GitSync
            | Action::GitStash
            | Action::GitStageAll
            | Action::GitUnstageAll
            | Action::GitStageFile
            | Action::GitFileHistory
            | Action::GitSelectionHistory
            | Action::GitBlame
            | Action::ShowCommit
            | Action::GitRevertChange => Some(Module::Git),
            Action::ToggleTerminal
            | Action::ToggleTerminalPanel
            | Action::RunConfiguration
            | Action::StopConfiguration
            | Action::EditRunConfigurations
            | Action::ClearTerminal
            | Action::TerminalCopy
            | Action::TerminalPaste
            | Action::NewTerminal
            | Action::CloseTerminal
            | Action::NextTerminal
            | Action::PreviousTerminal
            | Action::SplitTerminal
            | Action::MoveTerminalToGroup
            | Action::NextTerminalPane
            | Action::PreviousTerminalPane
            | Action::FocusTerminal1
            | Action::FocusTerminal2
            | Action::FocusTerminal3
            | Action::FocusTerminal4
            | Action::FocusTerminal5
            | Action::FocusTerminal6
            | Action::FocusTerminal7
            | Action::FocusTerminal8
            | Action::FocusTerminal9
            | Action::FindInTerminal
            | Action::MaximizeTerminal
            | Action::ScrollTerminalTop
            | Action::ScrollTerminalBottom
            | Action::ToggleTerminalPosition
            | Action::RenameTerminal
            | Action::PreviousCommand
            | Action::NextCommand
            | Action::FoldOutput
            | Action::UnfoldOutput
            | Action::NextError
            | Action::PreviousError
            | Action::SendToTerminal => Some(Module::Terminal),
            Action::RemoteConnect | Action::RemoteDisconnect => Some(Module::Remote),
            _ => None,
        }
    }

    /// 是否在命令面板中列出。
    ///
    /// 不列出的命令：命令面板自身（`ShowCommands`）、`RecordKeys`、剪贴板命令、`Quit`、等同于
    /// 方向键和删除键的命令，以及
    /// 只在特定位置有意义、离开该位置就无法执行的命令（文件树中选中的项、查找条和搜索面板的
    /// 开关与翻页）。
    pub fn in_palette(self) -> bool {
        use Action::*;
        !matches!(
            self,
            ShowCommands
                | RecordKeys
                | Cut
                | Copy
                | Paste
                | TerminalCopy
                | TerminalPaste
                | FindNext
                | FindPrevious
                | ReplaceOne
                | ReplaceAll
                | SelectAllMatches
                | ColumnSelectUp
                | ColumnSelectDown
                | ColumnSelectLeft
                | ColumnSelectRight
                | CursorLeft
                | CursorRight
                | CursorUp
                | CursorDown
                | CursorHome
                | CursorEnd
                | CursorLineStart
                | CursorLineEnd
                | DeleteLeft
                | DeleteRight
                | ToggleCase
                | ToggleWord
                | ToggleRegex
                | ReplaceAllInFiles
                | NextResult
                | PreviousResult
                | RenameFile
                | DeleteFile
                | DeleteFilePermanently
                | OpenSelected
                | PreviewSelected
                | FindInFolder
                | Quit
        )
    }
}
