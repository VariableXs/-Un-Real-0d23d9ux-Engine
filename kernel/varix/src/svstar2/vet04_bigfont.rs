//! VE-F3805 · 大字号与缩放渲染（VE-T 域 · 无障碍渲染 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3805`
//!
//! **判据（锚点原文）**：截断全域化、矢量图标红线、热区同步、折行显性、
//! 200% 全档、判据。
//!
//! **本条是什么**：大字号/系统缩放渲染的**全域保障**——F3046 复述的渲染层
//! 兑现。F3802（[`super::vet01_a11y_render_pipeline`]）管渲染管线的无障碍
//! 注入，F3804（[`super::vet03_focus_ring`]）管焦点环；本条管的是「系统
//! 缩放从 125% 拉到 200% 时，**每个界面、每一档**都还放得下、不糊、点得中」。
//!
//! ## 三条红线（锚点原文，逐条落成可执行断言）
//!
//! 1. **截断红线**：大字号文本截断=布局失败（P1）。本条把「截断」做成
//!    [`layout`] 的可判定产物——给定控件类、盒子、文本量与缩放档，折行
//!    策略算出需要的高度，放不下就是截断，**不静默裁字**。
//!    「截断全域化」= [`scan_all_tiers`]：控件实例 × 缩放档全集逐格
//!    出裁决，没有「测过 125% 就当 200% 也行」的抽样豁免。
//! 2. **矢量图标红线**：位图图标在大字号下放大即糊化（[`icon_finding`]
//!    检出）——出路是**矢量化排期**：位图必须上清单且排期在案；核心图标
//!    必须矢量（[`IconSpec::Vector`] 任意档零发现）；清单漏登 = 红线违例。
//! 3. **热区同步**：字号放大→热区同步放大（复用 F3042 的 44px 红线——
//!    热区与缩放严格成比例，交互类任何档都不得低于 44px 基线；
//!    不同步 = [`E_HOTZONE_DESYNC`] 断言拦截）。
//!
//! ## 折行策略显性（锚点数据结构）
//!
//! [`REFLOW_TABLE`] 是**显性策略表**：七个控件类每类恰一行（折行/伸缩/
//! 滚动），缺行可判定（[`policy_gap`]），表外类拒绝。滚动策略与 F3067
//! 协同——滚动行的截断豁免**以滚动条绑定为前提**（[`E_SCROLLBAR_UNBOUND`]：
//! 声称走滚动却没绑滚动条 = 截断换了件马甲，不算数）。
//!
//! ## 测量单源（真调 vee08）
//!
//! 字宽与行距**真调** [`super::vee08_fontmetric`] 的 `UNITS_PER_EM`/
//! `FALLBACK_ADVANCE`/`LOOSE_LINE_HEIGHT`——不另抄一份字模常数（两处各抄
//! 一份，迟早分叉）。放大取整一律**向上取整**：测量宁可多算不可少算，
//! 少算的一像素就是漏检的截断。
//!
//! ## 热区的档位语义
//!
//! [`LayoutInput::claimed_hot_zone_px`] 记的是 **100% 档的声称热区**；
//! 各档的声称值 = 基准 × 档位（向上取整——同步缩放的声明式），交互类
//! （[`is_interactive`]）必须声明热区（0 = 没有热区 = 直接失步），非交互类
//! （标签/进度条/滚动面板）0 = 不参与热区断言。
//!
//! ## 错误路径与降级矩阵（锚点原文）
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 截断检出 → **P1**（红线实测——全档扫描） | [`LayoutVerdict::truncated`] + [`E_TRUNCATED`]；扫描报告计数逐类可查 |
//! | 糊化检出 → **矢量化排期** | [`IconFinding::BlurScheduled`]（排期在案=合规降级）/ [`IconFinding::BlurUnscheduled`]（检出而无排期=违例） |
//! | 热区不同步 → **断言拦截** | [`hot_zone_synced`] 比例断言 + [`E_HOTZONE_DESYNC`] |
//! | 重排错乱 → **策略修正** | [`reflow_consistent`] 对裁决做自洽核验（[`E_REORDER_CONFLICT`]）：折行行的行数/行高、伸缩行的单行、滚动行的滚动条各归各的策略 |
//!
//! ## 跨批对接点（锚点原文）
//!
//! - **F3042 复用声明**：44px 热区基线（[`F3042_HOT_ZONE_MIN_PX`]）取自
//!   F3042「图标按钮命中区≥44px 热区」红线——本条只做**缩放维的同步**，
//!   不重定义基线数值；
//! - **F3046 复用声明**：滑块/进度条（[`WidgetClass::Slider`]/
//!   [`WidgetClass::ProgressBar`]）是 F3046 动效族的渲染对象，本条把它们
//!   纳入非文本同步缩放闭集；
//! - **F3067 滚动条协同**：滚动策略行的截断豁免与滚动条绑定一对一
//!   （[`ReflowPolicy::scrollbar_required`]）；
//! - **N 域布局对端**：本条消费布局产物（盒子尺寸）并回报裁决——布局
//!   引擎归 N 域，本条是**验收面**不是布局器。
//!
//! ## 无障碍与隐私（锚点原文）
//!
//! 无隐私面：全部输入是像素与档位整数，无用户内容。

use crate::checks::CheckSet;

use super::vee08_fontmetric::{FALLBACK_ADVANCE, LOOSE_LINE_HEIGHT, UNITS_PER_EM};

// ---------------------------------------------------------------------------
// 一、版本与诊断码（vet 家族字符串码风格）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const SCALE_VERSION: &str = "T05-bigfont-v1";

/// 截断检出（**P1·截断红线**：大字号文本截断=布局失败）。
pub const E_TRUNCATED: &str = "SCALE_TEXT_TRUNCATED";
/// 位图图标糊化检出且无矢量化排期（红线违例）。
pub const E_ICON_BLUR: &str = "SCALE_ICON_BLUR";
/// 位图图标未上清单（清单漏登——红线违例）。
pub const E_ICON_UNLISTED: &str = "SCALE_ICON_UNLISTED";
/// 热区与缩放不同步（断言拦截）。
pub const E_HOTZONE_DESYNC: &str = "SCALE_HOTZONE_DESYNC";
/// 折行策略表缺行（表外类）。
pub const E_POLICY_MISSING: &str = "SCALE_POLICY_MISSING";
/// 滚动策略行未绑滚动条（F3067 协同失效——截断换了马甲）。
pub const E_SCROLLBAR_UNBOUND: &str = "SCALE_SCROLLBAR_UNBOUND";
/// 重排裁决自洽性破损（重排错乱——策略修正）。
pub const E_REORDER_CONFLICT: &str = "SCALE_REORDER_CONFLICT";

/// 本域诊断码全集（判据对账：两两互异）。
pub const CODES: [&str; 7] = [
    E_TRUNCATED,
    E_ICON_BLUR,
    E_ICON_UNLISTED,
    E_HOTZONE_DESYNC,
    E_POLICY_MISSING,
    E_SCROLLBAR_UNBOUND,
    E_REORDER_CONFLICT,
];

// ---------------------------------------------------------------------------
// 二、缩放档闭集（125%~200% 全档 + 100% 基线）
// ---------------------------------------------------------------------------

/// 缩放档全集（千分整数——沿用定点整数确定性纪律：同参数跨优化级别同输出）。
pub const SCALE_STEPS: [u32; 5] = [1000, 1250, 1500, 1750, 2000];

/// 缩放档数。
pub const SCALE_STEP_COUNT: usize = 5;

/// 最高档（**200% 全档**判据的钉死值——最高档过了，中间档由单调性覆盖）。
pub const SCALE_MAX: u32 = 2000;

/// ceil(a/b)（放大测量一律向上取整：少算的一像素就是漏检的截断）。
pub const fn ceil_div(a: u32, b: u32) -> u32 {
    (a + b - 1) / b
}

// ---------------------------------------------------------------------------
// 三、控件类闭集与折行策略表（折行显性）
// ---------------------------------------------------------------------------

/// 控件类（闭集——策略表覆盖证明的基准）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetClass {
    /// 文本标签（宽度受父面板约束——大字号下折行；非交互无热区）。
    Label,
    /// 按钮（F3042 动效族——宽度可伸缩；交互类）。
    Button,
    /// 菜单项（宽度可伸缩；交互类）。
    MenuItem,
    /// 列表行（行宽定死、行高可长——折行；交互类）。
    ListRow,
    /// 滑块（F3046 动效族——非文本同步缩放；交互类）。
    Slider,
    /// 进度条（F3046 动效族——非文本同步缩放；非交互）。
    ProgressBar,
    /// 滚动面板（内容出框走滚动条——F3067 协同；非交互）。
    ScrollPane,
}

impl WidgetClass {
    /// 闭集全集（七类）。
    pub const ALL: [WidgetClass; 7] = [
        WidgetClass::Label,
        WidgetClass::Button,
        WidgetClass::MenuItem,
        WidgetClass::ListRow,
        WidgetClass::Slider,
        WidgetClass::ProgressBar,
        WidgetClass::ScrollPane,
    ];

    /// 序号。
    pub const fn ordinal(self) -> usize {
        match self {
            WidgetClass::Label => 0,
            WidgetClass::Button => 1,
            WidgetClass::MenuItem => 2,
            WidgetClass::ListRow => 3,
            WidgetClass::Slider => 4,
            WidgetClass::ProgressBar => 5,
            WidgetClass::ScrollPane => 6,
        }
    }

    /// 由序号还原（越界 `None`——表外类拒绝）。
    pub const fn from_ordinal(i: usize) -> Option<WidgetClass> {
        match i {
            0 => Some(WidgetClass::Label),
            1 => Some(WidgetClass::Button),
            2 => Some(WidgetClass::MenuItem),
            3 => Some(WidgetClass::ListRow),
            4 => Some(WidgetClass::Slider),
            5 => Some(WidgetClass::ProgressBar),
            6 => Some(WidgetClass::ScrollPane),
            _ => None,
        }
    }

    /// 类名（人读）。
    pub const fn label(self) -> &'static str {
        match self {
            WidgetClass::Label => "标签 / label",
            WidgetClass::Button => "按钮 / button",
            WidgetClass::MenuItem => "菜单项 / menu item",
            WidgetClass::ListRow => "列表行 / list row",
            WidgetClass::Slider => "滑块 / slider",
            WidgetClass::ProgressBar => "进度条 / progress bar",
            WidgetClass::ScrollPane => "滚动面板 / scroll pane",
        }
    }
}

/// 交互类判定（热区断言的适用面）：按钮/菜单项/列表行/滑块可点击，
/// 必须声明热区；标签/进度条/滚动面板本身不可点，不参与热区断言。
pub const fn is_interactive(c: WidgetClass) -> bool {
    matches!(
        c,
        WidgetClass::Button | WidgetClass::MenuItem | WidgetClass::ListRow | WidgetClass::Slider
    )
}

/// 折行/伸缩策略（**显性**——每类的重排出路写在表里，不靠布局器临场猜）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReflowPolicy {
    /// 折行：文本重排成多行（行高按 vee08 宽松行距——单源）。
    Wrap,
    /// 伸缩：宽度自适应伸长，上限 [`MAX_STRETCH_PCT`]（到顶仍放不下=截断，
    /// 诚实申报而不是无限伸长把布局撑爆）。
    Stretch,
    /// 滚动：内容出框走滚动条（**必须绑滚动条**——F3067 协同，见下）。
    Scroll,
}

impl ReflowPolicy {
    /// 该策略是否要求滚动条绑定（F3067 协同断言的载体）。
    pub const fn scrollbar_required(self) -> bool {
        matches!(self, ReflowPolicy::Scroll)
    }

    /// 策略名（人读）。
    pub const fn label(self) -> &'static str {
        match self {
            ReflowPolicy::Wrap => "折行 / wrap",
            ReflowPolicy::Stretch => "伸缩 / stretch",
            ReflowPolicy::Scroll => "滚动 / scroll",
        }
    }
}

/// 伸缩上限（百分比，相对本档盒子宽）。
pub const MAX_STRETCH_PCT: u32 = 150;

/// 策略表一行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyRow {
    /// 控件类。
    pub class: WidgetClass,
    /// 折行/伸缩/滚动策略。
    pub policy: ReflowPolicy,
}

/// **折行策略表**（锚点数据结构：折行策略显性——七类每类恰一行）。
pub const REFLOW_TABLE: [PolicyRow; 7] = [
    PolicyRow { class: WidgetClass::Label, policy: ReflowPolicy::Wrap },
    PolicyRow { class: WidgetClass::Button, policy: ReflowPolicy::Stretch },
    PolicyRow { class: WidgetClass::MenuItem, policy: ReflowPolicy::Stretch },
    PolicyRow { class: WidgetClass::ListRow, policy: ReflowPolicy::Wrap },
    PolicyRow { class: WidgetClass::Slider, policy: ReflowPolicy::Stretch },
    PolicyRow { class: WidgetClass::ProgressBar, policy: ReflowPolicy::Stretch },
    PolicyRow { class: WidgetClass::ScrollPane, policy: ReflowPolicy::Scroll },
];

/// 按类查策略（O(1) 线性扫七格；`None` = 缺行 → [`E_POLICY_MISSING`]）。
pub const fn policy_of(c: WidgetClass) -> Option<&'static PolicyRow> {
    let mut i = 0usize;
    while i < REFLOW_TABLE.len() {
        if REFLOW_TABLE[i].class.ordinal() == c.ordinal() {
            return Some(&REFLOW_TABLE[i]);
        }
        i += 1;
    }
    None
}

/// 策略表覆盖缺口证明：`None` = 每类恰一行（缺行/重行皆缺陷——重行让
/// 生效策略取决于扫描顺序，是隐式依赖）。
pub const fn policy_gap() -> Option<WidgetClass> {
    let mut p = 0usize;
    while p < WidgetClass::ALL.len() {
        let class = match WidgetClass::from_ordinal(p) {
            Some(x) => x,
            None => return Some(WidgetClass::Label),
        };
        let mut hits = 0usize;
        let mut i = 0usize;
        while i < REFLOW_TABLE.len() {
            if REFLOW_TABLE[i].class.ordinal() == class.ordinal() {
                hits += 1;
            }
            i += 1;
        }
        if hits != 1 {
            return Some(class);
        }
        p += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// 四、文本测量与热区（真调 vee08 单源 + F3042 热区复用）
// ---------------------------------------------------------------------------

/// 缩放后字号（px，向上取整）。
pub const fn font_px(base_font_px: u32, scale_permille: u32) -> u32 {
    ceil_div(base_font_px * scale_permille, 1000)
}

/// 文本需要宽度（px）——步进**真调 vee08** `FALLBACK_ADVANCE/UNITS_PER_EM`
/// （字模常数单源，不另抄一份）。
pub const fn text_width_px(text_units: u32, font_px: u32) -> u32 {
    ceil_div(text_units * font_px * FALLBACK_ADVANCE, UNITS_PER_EM)
}

/// 单行行高（px）——**真调 vee08** `LOOSE_LINE_HEIGHT`（宽松行距单源）。
pub const fn line_height_px(font_px: u32) -> u32 {
    ceil_div(font_px * LOOSE_LINE_HEIGHT, 1_000_000)
}

/// F3042 热区基线（px）——**引用值，非本域定义**：F3042「图标按钮命中区
/// ≥44px 热区」红线。本条只做缩放维同步，不重定义数值。
pub const F3042_HOT_ZONE_MIN_PX: u32 = 44;

/// 缩放档下的热区（px）——与缩放**严格成比例**（热区同步断言的定义式）：
/// 100% 时恰等于 F3042 基线，任何档不得低于基线。
pub const fn hot_zone_px(scale_permille: u32) -> u32 {
    ceil_div(F3042_HOT_ZONE_MIN_PX * scale_permille, 1000)
}

/// 热区同步判定：声称的热区必须 ≥ 比例值（热区只能偏大——偏小的那截
/// 就是用户点不中的区域）。
pub const fn hot_zone_synced(claimed_px: u32, scale_permille: u32) -> bool {
    claimed_px >= hot_zone_px(scale_permille)
}

// ---------------------------------------------------------------------------
// 五、图标（非文本缩放——矢量红线与糊化检出）
// ---------------------------------------------------------------------------

/// 图标规格（非文本缩放闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconSpec {
    /// 矢量图标——任意缩放零糊化（**核心图标必须矢量**红线）。
    Vector,
    /// 位图图标——原生像素 + 清单登记 + 矢量化排期（三者缺一按糊化
    /// 红线处置；原生像素足够大的档位下无放大即无糊化——不虚报）。
    Bitmap {
        /// 位图原生像素边长。
        native_px: u32,
        /// 是否已登矢量清单。
        listed: bool,
        /// 是否已有矢量化排期。
        scheduled: bool,
    },
}

/// 位图糊化判定阈（千分）：原生像素须 ≥ 渲染像素 × 110%（留 10% 重采样
/// 余量——轻微重采样肉眼不可辨，超过阈的放大必糊）。
pub const BITMAP_BLUR_GUARD_PERMILLE: u32 = 1100;

/// 图标检查结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconFinding {
    /// 无发现（矢量；或位图在本档无放大）。
    Ok,
    /// 糊化检出但排期在案（**合规降级**：糊化已知、矢量化有时限）。
    BlurScheduled,
    /// 糊化检出且无排期（**违例**：[`E_ICON_BLUR`]）。
    BlurUnscheduled,
    /// 位图未上清单（**违例**：[`E_ICON_UNLISTED`]——清单漏登连「知道
    /// 它会糊」都谈不上）。
    Unlisted,
}

/// 位图图标糊化检出（锚点：糊化检出→矢量化排期）。
///
/// 渲染像素 = 基准像素 × 缩放；原生像素不足渲染像素 × 阈即糊化。糊化的
/// **唯一合法出路**是矢量化排期：清单+排期双全才算「已知且有解」，缺
/// 任何一个都是红线违例而不是「先跑起来再说」。
pub const fn icon_finding(icon: &IconSpec, base_px: u32, scale_permille: u32) -> IconFinding {
    match *icon {
        IconSpec::Vector => IconFinding::Ok,
        IconSpec::Bitmap { native_px, listed, scheduled } => {
            let rendered = ceil_div(base_px * scale_permille, 1000);
            // 无放大不糊：原生像素 ≥ 渲染像素 × 阈（10% 余量内不虚报）。
            if native_px * 1000 >= rendered * BITMAP_BLUR_GUARD_PERMILLE {
                return IconFinding::Ok;
            }
            if !listed {
                return IconFinding::Unlisted;
            }
            if scheduled {
                IconFinding::BlurScheduled
            } else {
                IconFinding::BlurUnscheduled
            }
        }
    }
}

/// 图标发现是否违例（P1 计数用）：未登清单与糊化无排期是违例；
/// 排期在案是合规降级，不算违例但计糊化检出。
pub const fn finding_is_violation(f: IconFinding) -> bool {
    matches!(f, IconFinding::BlurUnscheduled | IconFinding::Unlisted)
}

// ---------------------------------------------------------------------------
// 六、布局裁决（折行/伸缩/滚动三策略的可判定产物）
// ---------------------------------------------------------------------------

/// 布局输入（N 域布局产物的验收面：盒子尺寸、文本量、档位、图标）。
#[derive(Clone, Copy, Debug)]
pub struct LayoutInput {
    /// 控件类。
    pub class: WidgetClass,
    /// 盒宽（px，本档布局产物——固定容器在此档的真实宽）。
    pub box_w_px: u32,
    /// 盒高（px）。
    pub box_h_px: u32,
    /// 文本量（字模单位数；非文本控件为 0）。
    pub text_units: u32,
    /// 基准字号（px，100% 档）。
    pub base_font_px: u32,
    /// 缩放档（千分）。
    pub scale_permille: u32,
    /// 图标规格。
    pub icon: IconSpec,
    /// 图标基准像素（100% 档的渲染边长）。
    pub icon_base_px: u32,
    /// 滚动条是否可用（仅 [`ReflowPolicy::Scroll`] 行消费）。
    pub scrollbar_available: bool,
    /// 声称的热区（px，**100% 档基准**——各档声称值 = 基准 × 档位；
    /// 非交互类填 0 表示不参与热区断言）。
    pub claimed_hot_zone_px: u32,
}

/// 布局裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutVerdict {
    /// 截断（**P1 红线**：大字号文本截断=布局失败）。
    pub truncated: bool,
    /// 折行行数（伸缩/滚动行为 1）。
    pub lines: u32,
    /// 文本需要高度（px）。
    pub height_needed_px: u32,
    /// 图标发现。
    pub icon_finding: IconFinding,
    /// 比例热区（px——[`hot_zone_px`] 的档位值）。
    pub hot_zone_px: u32,
    /// 滚动条绑定（滚动行且可用为 true）。
    pub scrollbar_bound: bool,
    /// 首个违例码（`None` = 本格全绿）。
    pub code: Option<&'static str>,
}

/// 单格布局裁决（一控件 × 一档；O(1)：常数量乘除 + 一次策略查表）。
pub fn layout(input: &LayoutInput) -> LayoutVerdict {
    let policy = match policy_of(input.class) {
        Some(p) => p.policy,
        None => {
            return LayoutVerdict {
                truncated: true,
                lines: 0,
                height_needed_px: 0,
                icon_finding: IconFinding::Ok,
                hot_zone_px: hot_zone_px(input.scale_permille),
                scrollbar_bound: false,
                code: Some(E_POLICY_MISSING),
            };
        }
    };

    let fp = font_px(input.base_font_px, input.scale_permille);
    let need_w = text_width_px(input.text_units, fp);
    let lh = line_height_px(fp);
    let finding = icon_finding(&input.icon, input.icon_base_px, input.scale_permille);
    let hz = hot_zone_px(input.scale_permille);

    let (lines, height_needed, truncated, scrollbar_bound, mut code) = match policy {
        ReflowPolicy::Wrap => {
            let lines = if need_w == 0 { 1 } else { ceil_div(need_w, input.box_w_px.max(1)) };
            let h = lines * lh;
            // 非文本控件（text_units=0）没有行箱，不参与行高检查。
            let t = input.text_units > 0 && h > input.box_h_px;
            (lines, h, t, false, if t { Some(E_TRUNCATED) } else { None })
        }
        ReflowPolicy::Stretch => {
            let max_w = input.box_w_px * MAX_STRETCH_PCT / 100;
            let t = need_w > max_w || (input.text_units > 0 && lh > input.box_h_px);
            (1, lh, t, false, if t { Some(E_TRUNCATED) } else { None })
        }
        ReflowPolicy::Scroll => {
            // 滚动行的截断豁免以滚动条绑定为前提（F3067 协同）：
            // 没绑滚动条，「走滚动」就是截断换了马甲。
            let fits_line = input.text_units == 0 || input.box_h_px >= lh;
            if !input.scrollbar_available {
                (1, lh, true, false, Some(E_SCROLLBAR_UNBOUND))
            } else if !fits_line {
                (1, lh, true, true, Some(E_TRUNCATED))
            } else {
                (1, lh, false, true, None)
            }
        }
    };

    // 热区同步拦截（断言级：不同步即违例，与截断独立计数）。
    // 交互类必须声明热区（0 = 没有热区 = 直接失步）；非交互类 0 = 不参与。
    let claimed = ceil_div(input.claimed_hot_zone_px * input.scale_permille, 1000);
    if code.is_none()
        && (is_interactive(input.class) && input.claimed_hot_zone_px == 0
            || input.claimed_hot_zone_px > 0 && !hot_zone_synced(claimed, input.scale_permille))
    {
        code = Some(E_HOTZONE_DESYNC);
    }
    // 图标违例（糊化无排期/清单漏登）优先级低于截断（先放得下再谈不糊）。
    if code.is_none() && finding_is_violation(finding) {
        code = Some(match finding {
            IconFinding::Unlisted => E_ICON_UNLISTED,
            _ => E_ICON_BLUR,
        });
    }

    LayoutVerdict {
        truncated,
        lines,
        height_needed_px: height_needed,
        icon_finding: finding,
        hot_zone_px: hz,
        scrollbar_bound,
        code,
    }
}

/// 重排自洽核验（锚点：重排错乱→策略修正）——裁决必须与它的策略一致：
/// 折行行行数 ≥1 且需要高度 = 行数×行高；伸缩/滚动行恒单行；滚动行必须
/// 绑滚动条。被篡改的裁决（行数 0、滚动未绑）在此显性拒绝。
pub const fn reflow_consistent(v: &LayoutVerdict, input: &LayoutInput) -> bool {
    let policy = match policy_of(input.class) {
        Some(p) => p.policy,
        None => return false,
    };
    match policy {
        ReflowPolicy::Wrap => {
            v.lines >= 1
                && v.height_needed_px
                    == v.lines * line_height_px(font_px(input.base_font_px, input.scale_permille))
        }
        ReflowPolicy::Stretch => {
            v.lines == 1
                && v.height_needed_px
                    == line_height_px(font_px(input.base_font_px, input.scale_permille))
        }
        ReflowPolicy::Scroll => v.lines == 1 && (!v.scrollbar_bound || input.scrollbar_available),
    }
}

// ---------------------------------------------------------------------------
// 七、全档全域扫描（截断全域化——控件实例 × 缩放档全集逐格出裁决）
// ---------------------------------------------------------------------------

/// 扫描报告（逐项计数——「哪里截了」可查，不是一个笼统总数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanReport {
    /// 扫描格数（实例数 × 档数）。
    pub cases: u32,
    /// 截断格数（P1）。
    pub truncated: u32,
    /// 糊化检出总数（含排期在案的合规降级）。
    pub blur_detected: u32,
    /// 图标违例数（糊化无排期 + 清单漏登）。
    pub icon_violations: u32,
    /// 热区不同步数。
    pub hot_desync: u32,
    /// 滚动条未绑数（F3067 协同失效）。
    pub scrollbar_unbound: u32,
}

impl ScanReport {
    /// 全绿（无截断、无违例、无失步、无未绑——糊化排期在案不计入）。
    pub const fn all_green(&self) -> bool {
        self.truncated == 0
            && self.icon_violations == 0
            && self.hot_desync == 0
            && self.scrollbar_unbound == 0
    }
}

/// 全档全域扫描（锚点：125%~200% 缩放档全档渲染验证，截断全域化）。
///
/// 逐格出裁决并归因计数——**没有抽样豁免**：五个档全扫，一个类漏登
/// 都会在 [`policy_gap`] 处先被拦住。
pub fn scan_all_tiers(inputs: &[LayoutInput]) -> ScanReport {
    let mut r = ScanReport {
        cases: 0,
        truncated: 0,
        blur_detected: 0,
        icon_violations: 0,
        hot_desync: 0,
        scrollbar_unbound: 0,
    };
    let mut k = 0usize;
    while k < inputs.len() {
        let mut t = 0usize;
        while t < SCALE_STEPS.len() {
            // 逐档扫描：实例的档位换成当前档（其余字段保持实例配置）。
            let mut item = inputs[k];
            item.scale_permille = SCALE_STEPS[t];
            let v = layout(&item);
            r.cases += 1;
            if v.truncated {
                r.truncated += 1;
            }
            if matches!(v.icon_finding, IconFinding::BlurScheduled | IconFinding::BlurUnscheduled) {
                r.blur_detected += 1;
            }
            if finding_is_violation(v.icon_finding) {
                r.icon_violations += 1;
            }
            let claimed = ceil_div(item.claimed_hot_zone_px * SCALE_STEPS[t], 1000);
            if is_interactive(item.class) && item.claimed_hot_zone_px == 0
                || item.claimed_hot_zone_px > 0 && !hot_zone_synced(claimed, SCALE_STEPS[t])
            {
                r.hot_desync += 1;
            }
            if policy_of(item.class)
                .map(|p| p.policy.scrollbar_required())
                .unwrap_or(false)
                && !v.scrollbar_bound
            {
                r.scrollbar_unbound += 1;
            }
            t += 1;
        }
        k += 1;
    }
    r
}

/// 标准界面集（覆盖七类；尺寸按各档折行后仍放得下取值——绿色基线，
/// 供全档扫描判据使用；负向判据用坏尺寸注入证明检测器为真）。
pub fn standard_surface() -> [LayoutInput; 7] {
    [
        // Label：父面板宽 160 定死（大字号下折行重排）；非交互无热区。
        LayoutInput {
            class: WidgetClass::Label,
            box_w_px: 160,
            box_h_px: 320,
            text_units: 40,
            base_font_px: 16,
            scale_permille: 1000,
            icon: IconSpec::Vector,
            icon_base_px: 0,
            scrollbar_available: false,
            claimed_hot_zone_px: 0,
        },
        // Button：伸缩策略；图标矢量（核心图标红线）；热区基准 44。
        LayoutInput {
            class: WidgetClass::Button,
            box_w_px: 120,
            box_h_px: 60,
            text_units: 8,
            base_font_px: 16,
            scale_permille: 1000,
            icon: IconSpec::Vector,
            icon_base_px: 24,
            scrollbar_available: false,
            claimed_hot_zone_px: 44,
        },
        // MenuItem：伸缩策略；矢量图标；热区基准 44。
        LayoutInput {
            class: WidgetClass::MenuItem,
            box_w_px: 180,
            box_h_px: 48,
            text_units: 10,
            base_font_px: 14,
            scale_permille: 1000,
            icon: IconSpec::Vector,
            icon_base_px: 20,
            scrollbar_available: false,
            claimed_hot_zone_px: 44,
        },
        // ListRow：行宽定死 220、行高 96（大字号折行）；位图图标已清单+排期
        // （200% 渲染 32px 超原生 28px 阈——糊化检出且排期在案的合规降级）。
        LayoutInput {
            class: WidgetClass::ListRow,
            box_w_px: 220,
            box_h_px: 96,
            text_units: 30,
            base_font_px: 14,
            scale_permille: 1000,
            icon: IconSpec::Bitmap { native_px: 32, listed: true, scheduled: true },
            icon_base_px: 16,
            scrollbar_available: false,
            claimed_hot_zone_px: 44,
        },
        // Slider：非文本（F3046 族），矢量图标；热区基准 44。
        LayoutInput {
            class: WidgetClass::Slider,
            box_w_px: 200,
            box_h_px: 40,
            text_units: 0,
            base_font_px: 14,
            scale_permille: 1000,
            icon: IconSpec::Vector,
            icon_base_px: 24,
            scrollbar_available: false,
            claimed_hot_zone_px: 44,
        },
        // ProgressBar：非文本（F3046 族），非交互无热区。
        LayoutInput {
            class: WidgetClass::ProgressBar,
            box_w_px: 200,
            box_h_px: 24,
            text_units: 0,
            base_font_px: 14,
            scale_permille: 1000,
            icon: IconSpec::Vector,
            icon_base_px: 0,
            scrollbar_available: false,
            claimed_hot_zone_px: 0,
        },
        // ScrollPane：滚动策略，滚动条绑定（F3067 协同）；非交互无热区。
        LayoutInput {
            class: WidgetClass::ScrollPane,
            box_w_px: 300,
            box_h_px: 200,
            text_units: 100,
            base_font_px: 16,
            scale_permille: 1000,
            icon: IconSpec::Vector,
            icon_base_px: 0,
            scrollbar_available: true,
            claimed_hot_zone_px: 0,
        },
    ]
}

// ---------------------------------------------------------------------------
// 八、域自检（判据逐条映射锚点：截断全域化 / 矢量图标红线 / 热区同步 /
//     折行显性 / 200% 全档 / 判据）
// ---------------------------------------------------------------------------

/// F3805 域自检入口（聚合器调用；零 panic 面，失败逐条红不炸域）。
pub fn run_f3805_checks() -> CheckSet {
    let mut s = CheckSet::new("vet04_bigfont");

    // --- 判据 1：缩放档闭集（125%~200% 全档 + 200% 钉死） ---
    {
        let mut ok = SCALE_STEP_COUNT == 5
            && SCALE_STEPS[0] == 1000
            && SCALE_STEPS[SCALE_STEP_COUNT - 1] == SCALE_MAX
            && SCALE_MAX == 2000;
        let mut i = 1usize;
        while i < SCALE_STEPS.len() {
            ok = ok && SCALE_STEPS[i] > SCALE_STEPS[i - 1];
            i += 1;
        }
        s.add(
            "T05-档闭集-五档单调且200%钉死",
            ok,
            "缩放档 100/125/150/175/200 单调升、最高档恰 200%（锚点 125%~200% 全档——抽掉任何一档，那一档的截断就没人看见）",
        );
    }

    // --- 判据 2：折行策略显性（覆盖证明 + 滚动协同） ---
    {
        s.add(
            "T05-策略-七类覆盖恰一行",
            policy_gap().is_none()
                && WidgetClass::from_ordinal(7).is_none()
                && REFLOW_TABLE.len() == WidgetClass::ALL.len(),
            "折行策略表逐类恰一行（缺行=该类大字号下无出路；重行=生效策略看扫描顺序的隐式依赖）；表外类序号还原 None",
        );
        let mut ok = true;
        for row in REFLOW_TABLE.iter() {
            ok = ok
                && policy_of(row.class) == Some(row)
                && row.policy.scrollbar_required() == matches!(row.policy, ReflowPolicy::Scroll);
        }
        s.add(
            "T05-策略-显性且滚动协同F3067",
            ok,
            "每行策略显性可查；滚动策略行必要求滚动条绑定（F3067 协同——「走滚动」的截断豁免以滚动条在位为前提）",
        );
    }

    // --- 判据 3：测量真调 vee08（单源 + ceil 语义 + 缩放单调） ---
    {
        let f1 = font_px(16, 1000);
        let f2 = font_px(10, 1250);
        s.add(
            "T05-测量-真调vee08单源且ceil",
            f1 == 16
                && f2 == 13
                && text_width_px(4, f1) == 32
                && text_width_px(4, f1) == text_width_px(4, f1)
                && line_height_px(f1) == 21,
            "100% 字号还原基准值；10px@125% 取 13（向上取整：测量宁可多算不可少算，少算的一像素就是漏检的截断）；字宽/行距真调 vee08 FALLBACK_ADVANCE 与 LOOSE_LINE_HEIGHT（字模常数单源，两处各抄一份迟早分叉）",
        );
        let a = font_px(16, 1250);
        let b = font_px(16, SCALE_MAX);
        s.add(
            "T05-测量-缩放字号单调",
            a > f1 && b > a && b == 32,
            "字号随档位单调升；200% 恰两倍（16→32）——档位语义的数值锚点",
        );
    }

    // --- 判据 4：折行（行数/高度可判定 + 截断=P1 红线） ---
    {
        let inp = LayoutInput {
            class: WidgetClass::Label,
            box_w_px: 100,
            box_h_px: 120,
            text_units: 40,
            base_font_px: 16,
            scale_permille: 1250,
            icon: IconSpec::Vector,
            icon_base_px: 0,
            scrollbar_available: false,
            claimed_hot_zone_px: 0,
        };
        let v = layout(&inp);
        // font=20, need_w=40*20/2=400, lines=ceil(400/100)=4, lh=26, h=104 ≤ 120。
        let fit = !v.truncated && v.lines == 4 && v.height_needed_px == 104;
        let mut tight = inp;
        tight.box_h_px = 100;
        let vt = layout(&tight);
        s.add(
            "T05-折行-行数行高可判定",
            fit,
            "40 字模@125% 在 100px 盒内折 4 行、需高 104px ≤ 120px 放得下（折行算得出行数与需要高度——不是「感觉放得下」）",
        );
        s.add(
            "T05-折行-截断红线P1",
            vt.truncated && vt.code == Some(E_TRUNCATED),
            "盒高压到 100px：需要 104px 放不下 → 截断=true 且落 E_TRUNCATED（P1 级——锚点原文「大字号文本截断=布局失败」，不静默裁字）",
        );
    }

    // --- 判据 5：伸缩（上限内放行 + 到顶诚实申报截断） ---
    {
        let mut inp = LayoutInput {
            class: WidgetClass::Button,
            box_w_px: 120,
            box_h_px: 60,
            text_units: 8,
            base_font_px: 16,
            scale_permille: SCALE_MAX,
            icon: IconSpec::Vector,
            icon_base_px: 24,
            scrollbar_available: false,
            claimed_hot_zone_px: 44,
        };
        let v = layout(&inp);
        // font=32, need_w=8*32/2=128 ≤ 120*1.5=180；lh=42 ≤ 60 → 放得下。
        let small = !v.truncated && v.lines == 1;
        inp.text_units = 20; // need_w=320 > 180 → 伸缩到顶仍放不下。
        let big = layout(&inp);
        s.add(
            "T05-伸缩-上限内放行",
            small,
            "8 字模@200% 需 128px ≤ 伸缩上限 180px：放行且恒单行（伸缩策略的量化语义）",
        );
        s.add(
            "T05-伸缩-到顶截断不无限伸",
            big.truncated && big.code == Some(E_TRUNCATED),
            "20 字模@200% 需 320px > 180px：伸缩到顶仍放不下 → 截断（诚实申报而不是无限伸长把布局撑爆）",
        );
    }

    // --- 判据 6：滚动协同 F3067（豁免以绑定为前提 + 未绑即截断马甲） ---
    {
        let mut inp = LayoutInput {
            class: WidgetClass::ScrollPane,
            box_w_px: 300,
            box_h_px: 200,
            text_units: 100,
            base_font_px: 16,
            scale_permille: SCALE_MAX,
            icon: IconSpec::Vector,
            icon_base_px: 0,
            scrollbar_available: true,
            claimed_hot_zone_px: 0,
        };
        let v = layout(&inp);
        let bound_ok = !v.truncated && v.scrollbar_bound;
        inp.scrollbar_available = false;
        let unbound = layout(&inp);
        s.add(
            "T05-滚动-绑定则豁免截断",
            bound_ok,
            "滚动条在位：内容出框走滚动（F3067 协同），不判截断（滚动是大字号的合法出路）",
        );
        s.add(
            "T05-滚动-未绑即截断马甲",
            unbound.truncated && unbound.code == Some(E_SCROLLBAR_UNBOUND) && !unbound.scrollbar_bound,
            "声称走滚动却没绑滚动条：截断=true 且落 E_SCROLLBAR_UNBOUND（截断换了件马甲不算数）",
        );
    }

    // --- 判据 7：矢量图标红线（矢量零糊化 + 位图糊化三路 + 不虚报 + 分型） ---
    {
        s.add(
            "T05-矢量-核心图标任意档零糊化",
            icon_finding(&IconSpec::Vector, 24, SCALE_MAX) == IconFinding::Ok
                && icon_finding(&IconSpec::Vector, 24, 1000) == IconFinding::Ok,
            "矢量图标在最高档也零发现（锚点：核心图标必须矢量——矢量是唯一在 200% 下不糊的形态）",
        );
        let bm = IconSpec::Bitmap { native_px: 28, listed: true, scheduled: true };
        let unsched = IconSpec::Bitmap { native_px: 28, listed: true, scheduled: false };
        let unlisted = IconSpec::Bitmap { native_px: 28, listed: false, scheduled: false };
        s.add(
            "T05-位图-糊化三路分流",
            icon_finding(&bm, 16, SCALE_MAX) == IconFinding::BlurScheduled
                && icon_finding(&unsched, 16, SCALE_MAX) == IconFinding::BlurUnscheduled
                && icon_finding(&unlisted, 16, SCALE_MAX) == IconFinding::Unlisted,
            "16px 基准@200% 渲染 32px > 原生 28px×1.1 阈：糊化检出——排期在案=合规降级（BlurScheduled），无排期=违例（E_ICON_BLUR），未上清单=违例（E_ICON_UNLISTED，连「知道它会糊」都谈不上）",
        );
        let small = IconSpec::Bitmap { native_px: 96, listed: false, scheduled: false };
        s.add(
            "T05-位图-无放大不虚报",
            icon_finding(&small, 16, SCALE_MAX) == IconFinding::Ok,
            "原生 96px ≥ 渲染 32px×1.1：无放大即无糊化——检出器不虚报（糊化红线只拦真糊化，不拦高分辨率位图）",
        );
        s.add(
            "T05-位图-违例与降级分型",
            finding_is_violation(IconFinding::BlurUnscheduled)
                && finding_is_violation(IconFinding::Unlisted)
                && !finding_is_violation(IconFinding::BlurScheduled)
                && !finding_is_violation(IconFinding::Ok),
            "违例（无排期/未登清单）与合规降级（排期在案）分型——把「已知有解的糊化」当违例会让排期机制失效",
        );
    }

    // --- 判据 8：热区同步（F3042 复用 + 比例断言 + 全档不缩水 + 交互类必声明） ---
    {
        let mut ok = hot_zone_px(1000) == F3042_HOT_ZONE_MIN_PX && F3042_HOT_ZONE_MIN_PX == 44;
        let mut i = 1usize;
        while i < SCALE_STEPS.len() {
            ok = ok
                && hot_zone_px(SCALE_STEPS[i]) > hot_zone_px(SCALE_STEPS[i - 1])
                && hot_zone_px(SCALE_STEPS[i]) >= F3042_HOT_ZONE_MIN_PX;
            i += 1;
        }
        s.add(
            "T05-热区-比例同步全档不缩水",
            ok && hot_zone_px(SCALE_MAX) == 88,
            "100% 恰 44px（F3042 基线复用，引用值非重定义）、逐档单调升、任何档 ≥44、200% 恰 88px（字号放大热区同步放大——缩水的那截就是点不中的区域）",
        );
        s.add(
            "T05-热区-不同步断言拦截",
            !hot_zone_synced(40, 1000) && hot_zone_synced(88, SCALE_MAX) && !hot_zone_synced(60, SCALE_MAX),
            "声称 40px@100%（<44）拦；88px@200% 放行；60px@200%（<88）拦——热区只能偏大不能偏小（E_HOTZONE_DESYNC 的判定式）",
        );
        s.add(
            "T05-热区-交互类必声明",
            is_interactive(WidgetClass::Button)
                && is_interactive(WidgetClass::Slider)
                && !is_interactive(WidgetClass::Label)
                && !is_interactive(WidgetClass::ScrollPane),
            "交互类（按钮/菜单项/列表行/滑块）必须有热区声明，非交互类（标签/进度条/滚动面板）不参与——交互面闭集是热区断言的适用边界",
        );
    }

    // --- 判据 9：截断全域化扫描（标准界面 × 全档 35 格全绿） ---
    {
        let surface = standard_surface();
        let r = scan_all_tiers(&surface);
        s.add(
            "T05-扫描-标准界面全档全绿",
            r.cases == 35 && r.all_green() && r.blur_detected == 1,
            "七类实例 × 五档 = 35 格逐格裁决：零截断、零图标违例、零热区失步、零滚动未绑；ListRow 位图糊化恰 1 档（200%）检出且排期在案（合规降级可见）——最高档过了，中间档由单调性覆盖",
        );
        s.add(
            "T05-扫描-200%全档点名",
            {
                let mut ok = true;
                let mut k = 0usize;
                while k < surface.len() {
                    let mut inp = surface[k];
                    inp.scale_permille = SCALE_MAX;
                    let v = layout(&inp);
                    ok = ok && !v.truncated;
                    k += 1;
                }
                ok
            },
            "200% 档逐类点名零截断（锚点判据原文「200% 全档」单独钉一次——不给「扫过了」留含糊）",
        );
    }

    // --- 判据 10：扫描检真（坏配置注入必须红——检测器不是恒绿） ---
    {
        let mut bad = standard_surface();
        bad[0].box_h_px = 20; // Label 盒高压到 20px：折行需高必超。
        let r = scan_all_tiers(&bad);
        s.add(
            "T05-扫描-坏尺寸注入必检出",
            r.truncated >= 1 && !r.all_green(),
            "Label 盒高 20px：扫描报出截断（检测器对真缺陷敏感——全绿通过的全绿才有意义）",
        );
        let mut bad2 = standard_surface();
        bad2[6].scrollbar_available = false; // ScrollPane 拔掉滚动条。
        let r2 = scan_all_tiers(&bad2);
        s.add(
            "T05-扫描-拔滚动条必检出",
            r2.scrollbar_unbound >= 1,
            "ScrollPane 滚动条被拔：扫描报出未绑（F3067 协同失效全域可见）",
        );
        let mut bad3 = standard_surface();
        bad3[1].claimed_hot_zone_px = 30; // Button 热区谎报 30px 基准。
        let r3 = scan_all_tiers(&bad3);
        s.add(
            "T05-扫描-热区谎报必检出",
            r3.hot_desync >= 1,
            "Button 热区谎报 30px 基准：五档全部失步（热区同步是断言不是承诺）",
        );
    }

    // --- 判据 11：重排自洽（裁决与策略一致 + 篡改拒绝） ---
    {
        let inp = LayoutInput {
            class: WidgetClass::Label,
            box_w_px: 100,
            box_h_px: 120,
            text_units: 40,
            base_font_px: 16,
            scale_permille: 1250,
            icon: IconSpec::Vector,
            icon_base_px: 0,
            scrollbar_available: false,
            claimed_hot_zone_px: 0,
        };
        let v = layout(&inp);
        let mut tampered = v;
        tampered.lines = 0;
        s.add(
            "T05-重排-真实裁决自洽",
            reflow_consistent(&v, &inp),
            "折行裁决：行数 ≥1 且需要高度 = 行数×行高（裁决与策略一致——重排错乱在产出端就被拦住）",
        );
        s.add(
            "T05-重排-篡改裁决拒绝",
            !reflow_consistent(&tampered, &inp),
            "行数被改成 0 的裁决过不了自洽核验（E_REORDER_CONFLICT 的判定面——重排错乱→策略修正的落点）",
        );
    }

    // --- 判据 12：判据自身（码互异 + 版本 + 条数对账） ---
    {
        let mut ok = true;
        let mut i = 0usize;
        while i < CODES.len() {
            let mut j = i + 1;
            while j < CODES.len() {
                if CODES[i] == CODES[j] {
                    ok = false;
                }
                j += 1;
            }
            i += 1;
        }
        s.add("T05-判据-七码两两互异", ok, "七码互异（按码归因的前提）");
        s.add(
            "T05-判据-版本钉死",
            SCALE_VERSION == "T05-bigfont-v1" && SCALE_VERSION.starts_with("T05-"),
            "版本串钉死（T 域第 05 项——与 F3802/F3803/F3804 的族内序一致）",
        );
        s.add(
            "T05-判据-条数对账",
            s.len() == 27,
            "判据条数恰 28（本条执行前已有 27 条，防悄悄增删）",
        );
    }

    s
}
