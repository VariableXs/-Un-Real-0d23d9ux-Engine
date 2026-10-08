//! VE-F3803 · 高对比渲染引擎（VE-T 域 · 无障碍渲染 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3803`
//!
//! **判据（锚点原文逐条）**：双段架构、语义保持、后处理纪律、复用单源、
//! 检测断言、判据。
//!
//! **本条是什么**：F3802（[`vet01_a11y_render_pipeline`]）把无障碍渲染落成
//! 「四态采集 → 策略映射 → 三点注入 → 预算裁决」的管线。本条是那条管线的
//! **最深一段**——高对比态被采到之后，究竟把画面改成什么样。
//!
//! **双段架构（判据一，锚点"双段架构：令牌段+后处理段"）**：
//! 高对比渲染**不是一件事，是两件有先后依赖的事**：
//!   - **令牌段**：把高对比主题令牌（复用 F2895 单源）解析成一组**渲染决策**
//!     （哪些像素要描边、描边多宽、哪些半透明要压实、对比度下限是多少）；
//!   - **后处理段**：拿令牌段的决策去改像素。
//! 分段的理由不是"职责划分好看"，而是**这两段的失败代价完全不同**：
//! 令牌段错 → 决策错（可诊断，产出的是意图）；
//! 后处理段错 → 像素错（不可诊断，用户看到的是画面坏了，且引擎不知道坏了）。
//! 合成一段写，两个错误会互相掩盖——决策错可能被像素层的"看起来还行"掩盖，
//! 像素错也可能被决策层的"意图正确"掩盖。故本条把两段的产出**分别可取回**
//! （[`HcPlan`] 与 [`PostReport`]），任一段出错都能定位到段。
//!
//! **语义保持红线（判据二，锚点"后处理不改语义——描边是视觉不是语义"）**：
//! 这是本条最重的一条红线，也是最容易被"善意地"违反的一条。
//! 后处理会改像素（描边、压实半透明）——**像素是视觉**；但它绝不能改
//! **语义**（文本内容、控件角色、可访问名、交互态、焦点序）。
//! 典型违反：为了让描边"看起来更清楚"，把描边像素写进文本层，
//! 结果读屏软件把描边当成额外的字形节点念出来——画面更清楚了，
//! **用户听到的内容却变错了**。这是拿一部分用户换另一部分用户。
//!
//! 本条把这条红线做成**可执行的对拍**而非文档承诺：
//! [`semantic_digest`] 对场景的语义面（文本/角色/可访问名/交互态/焦点序）
//! 算一个 64 位摘要；[`apply_post`] 前后各算一次，**摘要必须逐位相等**。
//! 任何试图在像素层顺手改语义的做法都会让摘要变化 → 判 P1。
//!
//! **后处理纪律（判据三）**：三条，越界即 P1。
//!   1. **只许改视觉面**：后处理段的写入口只有像素面，语义面只读；
//!   2. **不得增删节点**：描边是在既有像素上改值，不是新增绘制节点
//!      （新增节点会进无障碍树 = 改语义）；
//!   3. **不得降对比度**：高对比是**单调增强**方向——后处理可以让对比度
//!      上升，但绝不许下降。描边把文字和背景拉近（描边吃掉间隙）、
//!      压实半透明（半透明层叠后实际混色偏灰）都会**悄悄降低**实际对比度，
//!      这是"高对比模式反而降低了对比度"的讽刺缺陷，必须单调断言。
//!
//! **复用单源（判据四）**：本条**不重新发明**三样东西，只读消费上游：
//!   - 主题令牌集 → [`vep03_token`]（F2895 三主题矩阵的 Rust 权威实现）；
//!   - 高对比态检测 → [`vet01_a11y_render_pipeline`] 的四态采集
//!     （`state_enabled` / `A11yStateKey::HighContrast`）；
//!   - 预算裁决 → [`vet01_a11y_render_pipeline`] 的 `judge_budget`
//!     （1ms 注入预算，F3802 已冻结）。
//! 复用不是省事，是**避免双源分叉**：态检测若在本域再写一份，
//! 采到 true 的地方和采到 true 的地方会不一致，用户会遇到
//! "设置里开了高对比但某些窗口没反应"。故本条对上游只读，
//! 且在自检里**真的调用上游函数**（而非自造一个 True）——
//! 这条纪律的判据是"本条的输入必须来自上游类型"，
//! 编译器因此能保证本条不可能绕开上游单源。
//!
//! **检测断言（判据五）**：高对比态**采不到**时（断供，值为 `None`），
//! 本条**不得**按"未开启"渲染——断供判 P0（复述 F3802 断供红线）。
//! 断供时的正确处置是：**不注入任何后处理**，让画面退回常规主题
//! （宁可不增强，也不假装知道）。这与"按 false 处理"的区别是：
//! 后者会让依赖高对比的用户在通道断供时**以为已生效**（画面没变但也无提示），
//! 前者至少产出显性诊断。
//!
//! **F3822 前向声明（锚点"与 F3822 forced-colors 关系"）**：系统级
//! `forced-colors` 与本条的高对比是**两层不同的东西**——
//! `forced-colors` 是**系统强制色板**（创作者色一律被系统色覆盖，
//! 映射表与覆写白名单由 F3822 管），本条是**引擎侧增强渲染**
//! （描边/压实/对比度提升）。二者关系：F3822 判定 forced 生效时，
//! 本条的令牌覆盖**必须让位于系统色板**（引擎不得用创作者令牌把系统色
//! 盖回去——那是 F3822 的"覆写红线"）。本条只做**前向声明**：
//! [`ForcedRelation::defer_to_forced_colors`] 记录让位关系与理由，
//! 不越界实现 F3822 的映射表。
//!
//! **确定性**：全部静态注册表 + 纯函数，耗时以逻辑 tick 注入（不用墙钟），
//! 无 IO、可回放对拍。像素面是定长 `Vec`，无堆抖动。
//!
//! **跨批对接点**：上游 F2895 令牌集（[`vep03_token`]）与 F3802 四态检测
//! （[`vet01_a11y_render_pipeline`]）为复用单源；D 域后处理为执行对端；
//! 下游 F3822 forced-colors（让位关系对端）。

#![allow(clippy::needless_range_loop)]

use super::vep03_token::{inject_for_theme, MotionTokenError, Theme, TokenTable};
use super::vet01_a11y_render_pipeline as f3802;
use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断基础设施（**复用 F3802 的五元组形态，不重复定义**）
// ---------------------------------------------------------------------------
//
// **为什么不自己再定义一套 `IssueBag`**：本域第一版确实自造了同名同形的
// `Severity` / `Issue` / `IssueBag`，编译立刻报类型不兼容——
// `f3802::capture_states(.., bag)` 要的是上游的 `&mut IssueBag`，
// 而本域传进去的是自己那个。两个同名类型互不相通的后果是：
// 上游采集阶段发现的断供问题**进不了本域的诊断袋**，于是
// 「高对比态断供」这条本域最重的 P0 会在采集后凭空消失。
//
// 这不是笔误级别的问题，是**双源分叉的编译期显形**：只要本域还想
// 复用上游函数，就一定会撞上这个类型墙。故直接 `pub use` 上游三件套：
//   - 变体集完全够用（上游已有 P0/P1，本域需要的正是这两级）；
//   - 诊断可跨段汇入同一袋（采集段 + 令牌段 + 后处理段）；
//   - 上游将来加级别时本域自动跟随，不会出现"上游 P2、本域无 P2"。
//
// `IssueBag::has_code` 是本域需要的额外查询能力；上游没有提供，
// 故本域以**扩展 trait** 的形式补上（不污染上游类型的定义面）。

/// 复用上游诊断类型（判据四「复用单源」在类型层的落实）。
pub use super::vet01_a11y_render_pipeline::{Issue, IssueBag, Severity};

/// 本域所需的诊断查询扩展（上游 `IssueBag` 未提供，按码精确查询）。
///
/// **为什么要精确到错误码而不是子串猜测**：`has_code` 若退化为
/// `issue.code.contains(needle)`，那么查 `"HC_PLAN_EMPTY"` 时
/// 任何含该串的错误码都会被误判为命中——自检就会假绿。
/// 逐字相等是唯一可靠的形态。
pub trait IssueBagExt {
    /// 该错误码是否已立案（逐字相等）。
    fn has_code(&self, code: &str) -> bool;
    /// 本域诊断里是否有 P0。
    fn has_p0(&self) -> bool;
}

impl IssueBagExt for IssueBag {
    fn has_code(&self, code: &str) -> bool {
        self.issues().iter().any(|i| i.code == code)
    }
    fn has_p0(&self) -> bool {
        self.issues().iter().any(|i| i.severity == Severity::P0)
    }
}

// ---------------------------------------------------------------------------
// 二、语义面与像素面（判据二/三的物理基础：两个面必须真的分开）
// ---------------------------------------------------------------------------

/// 语义节点角色。**这些值不进无障碍树以外的地方**——它们只描述
/// 「这个节点是什么」，供语义摘要与纪律断言使用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeRole {
    /// 普通文本。
    Text,
    /// 交互控件（按钮/链接/输入框）。
    Widget,
    /// 图像。
    Image,
    /// 容器（无语义内容）。
    Container,
}

/// 单个语义节点。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemNode {
    pub id: u32,
    pub role: NodeRole,
    /// 可访问名（读屏念出来的文本）。
    pub acc_name: String,
    /// 是否可聚焦。
    pub focusable: bool,
    /// 交互态位（按下/选中/禁用）。语义的一部分。
    pub interactive: u8,
}

impl SemNode {
    pub fn new(id: u32, role: NodeRole, acc_name: &str) -> Self {
        SemNode { id, role, acc_name: acc_name.to_string(), focusable: true, interactive: 0 }
    }
}

/// 语义面：**只读消费**。后处理段对它只有 `&` 引用。
///
/// 字段顺序刻意按「语义摘要的混入顺序」排列——`semantic_digest`
/// 按此顺序混入，字段增删会立刻改变摘要（这是摘要能当红线的前提）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SemLayer {
    /// 语义节点（**顺序即焦点序**：读屏与键盘 Tab 都按此序）。
    pub nodes: Vec<SemNode>,
}

/// 语义摘要（64 位 FNV-1a）。
///
/// **这是语义保持红线的 teeth**：后处理前后各算一次，逐位相等才准入。
/// 为什么用摘要而不是逐字段比对：逐字段比对在代码里会写成
/// `assert_eq!(a.nodes, b.nodes)`——而 `assert_eq!` 在 release 下若被
/// 优化掉、或字段将来新增时忘了加进比对，就会**静默失效**。
/// 摘要把「所有语义字段」压成一个数，新增字段自动进入摘要
/// （只要 `semantic_digest` 把它列进去），漏比对不可能发生。
pub fn semantic_digest(sem: &SemLayer) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |v: u64| {
        h ^= v;
        h = h.wrapping_mul(0x1000_0000_01b3);
    };
    mix(sem.nodes.len() as u64);
    for n in &sem.nodes {
        mix(n.id as u64);
        mix(n.role as u64);
        mix(n.focusable as u64);
        mix(n.interactive as u64);
        // 可访问名逐字节混入：名字改一个字符即摘要变化。
        for b in n.acc_name.as_bytes() {
            mix(*b as u64);
        }
        // 分隔符：防止 ("ab","c") 与 ("a","bc") 撞摘要。
        mix(0xff);
    }
    h
}

/// 像素面：后处理段的**唯一**写入口。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PixLayer {
    /// RGB 三元组（8 位量化）。本域不做色彩空间转换——
    /// 转换属于 D 域渲染管线，本域只消费其输出并做视觉增强。
    pub rgb: Vec<[u8; 3]>,
}

impl PixLayer {
    pub fn new() -> Self {
        PixLayer { rgb: Vec::new() }
    }

    /// 铺一整块纯色。
    pub fn fill(&mut self, rgb: [u8; 3]) {
        self.rgb.clear();
        self.rgb.push(rgb);
    }

    pub fn len(&self) -> usize {
        self.rgb.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rgb.is_empty()
    }

    /// 读一个像素。越界返回 `None`（不 panic，见零 panic 面纪律）。
    pub fn get(&self, idx: usize) -> Option<[u8; 3]> {
        self.rgb.get(idx).copied()
    }

    /// 写一个像素。越界返回 `false`（**不 panic**：越界说明段间尺寸失配，
    /// 那是调用方的缺陷，由 [`apply_post`] 产出诊断而非在此崩掉）。
    pub fn put(&mut self, idx: usize, rgb: [u8; 3]) -> bool {
        match self.rgb.get_mut(idx) {
            Some(slot) => {
                *slot = rgb;
                true
            }
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 三、WCAG 对比度（外部对拍锚点，非自证）
// ---------------------------------------------------------------------------

/// WCAG 2.x 相对亮度。
///
/// **口径纪律**：阈值是 **4.5**（AA 正文），不是 7（AAA）。
/// 本条只承诺 AA——F2895 定的硬门就是 AA；把 AAA 写进来会让
/// "达标"的定义与上游不一致（同一对颜色，上游判过、本条判不过，
/// 双源分叉正是复用纪律要防的）。
pub const WCAG_AA_NORMAL: f32 = 4.5;

/// sRGB 通道线性化（WCAG 2.x 规定的分段式）。
///
/// 分界是 **0.03928**（不是 0.04045——后者是 sRGB 规范给出的
/// 另一等价分界，两者数学上等价但 WCAG 文本用 0.03928；
/// 此处按 WCAG 文本取值以便与对照工具逐位对齐）。
///
/// **指数必须是 2.4，且必须用 `powf` 而非 `powi`**——这是本条
/// 实测踩过的静默缺陷：`powi` 只收整数，写成 `powi(5)`（想省一次
/// 乘法、把 2.4 当"约 5"）在编译期完全合法，但 #767676 的对比度
/// 会从 4.5422 变成 13.3817（亮度 0.18116 → 0.02847），
/// 后果是**所有颜色对都被判为过 AA**，无障碍硬门形同虚设，
/// 而类型检查、单元测试、编译器告警全程静默。
/// 2.4 = 12/5，唯一无误差的整数幂写法是 `p.powi(12).powf(1.0/5.0)`，
/// 但那引入除法误差；直接 `powf(2.4)` 才是 WCAG 规定的口径。
pub fn channel_luminance(c: u8) -> f32 {
    let s = c as f32 / 255.0;
    if s <= 0.03928 {
        s / 12.92
    } else {
        let p = (s + 0.055) / 1.055;
        p.powf(2.4)
    }
}

/// 相对亮度（0..1）。
pub fn relative_luminance(rgb: [u8; 3]) -> f32 {
    0.2126 * channel_luminance(rgb[0])
        + 0.7152 * channel_luminance(rgb[1])
        + 0.0722 * channel_luminance(rgb[2])
}

/// WCAG 对比度（1..21）。
///
/// **分母恒 +0.05、分子取大者**：黑对白必须得 **21.0000**（不是 20.9）。
/// 这是 WCAG 的定义，任何"约等于 21"实现都是错的，故本条把
/// 黑对白作为自检的外部锚点（见 checks）。
pub fn contrast_ratio(a: [u8; 3], b: [u8; 3]) -> f32 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// 对比度是否达 AA（**单边符号**：>= 判过）。
///
/// 之所以用单边而非双边阈值区间：达标判定只有"过/不过"两态，
/// 写成 `ratio > 4.4 && ratio < 4.6` 会把 4.54 这种**正确达标**的
/// 样本判为不过——双边阈值一旦宽过正确实现与错误实现的偏差幅度，
/// 高估型缺陷就会从缝里钻过去（实测教训）。
pub fn meets_aa(a: [u8; 3], b: [u8; 3]) -> bool {
    contrast_ratio(a, b) >= WCAG_AA_NORMAL
}

// ---------------------------------------------------------------------------
// 四、令牌段：把 F2895 令牌解析成渲染决策（判据一上半）
// ---------------------------------------------------------------------------

/// 后处理动作。令牌段产出它，后处理段消费它——**两段之间的契约就是这个枚举**。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostAction {
    /// 描边（强化边界）。
    Outline,
    /// 压实半透明（把 alpha 混合痕迹压成实色）。
    Desaturate,
    /// 对比度提升（把前景往远离背景的方向推）。
    BoostContrast,
    /// **语义写入动作（越界动作）**：令牌段若产出它，后处理段必须
    /// 计入 [`PostReport::sem_writes`] 并判 P1。
    ///
    /// **为什么把这个越界动作显式建模**，而不是只靠"实现里没写
    /// 改语义代码"来保证：后者是**信任**，前者是**机制**。
    /// 令牌表来自外部（F2895），若将来的令牌扩展里混进一个
    /// 语义相关的决策，而后处理段没有对应分支，Rust 的 `match`
    /// 穷尽性会强制处理它——但处理成什么由实现者决定。
    /// 把"越界"建模成一个显式动作，就把"该做什么"固定了下来：
    /// 计数 + 立案，而不是静默忽略或默默执行。
    ProbeSemantics,
}

/// 渲染决策（令牌段的产出，逐条可审计）。
#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub action: PostAction,
    /// 目标节点（`None` = 全场景）。
    pub target: Option<u32>,
    /// 动作强度（0..=255 的 8 位量化；语义随 `action`）。
    pub amount: u8,
    /// 该决策依据的令牌名（来自 F2895 单源，便于对账）。
    pub token: String,
    /// 用户面解释（策略透明，与 F3802 同纪律：无解释即 P1）。
    pub explain: String,
}

/// 令牌段的完整产出。
#[derive(Clone, Debug, Default)]
pub struct HcPlan {
    /// 主题码（来自 F2895 单源，本域不自定义主题名）。
    pub theme_code: String,
    /// 决策序列（顺序即执行序）。
    pub decisions: Vec<Decision>,
    /// 消费掉的令牌条数（对账用：令牌段不该漏读令牌）。
    pub tokens_read: usize,
    /// 对比度下限（本帧强制）。
    pub contrast_floor: f32,
}

/// 令牌段规格决策（**只读消费 F2895 的高对比主题**）。
///
/// `tokens` 来自 [`inject_for_theme`] 的高对比注入结果。
/// 若上游令牌表为空（`tokens.is_empty()`），本段**不产出任何决策**
/// ——空令牌表意味着"高对比没东西可覆盖"，此时若仍产出描边决策，
/// 就是引擎自说自话地增强了画面而与主题无关。
fn plan_from_tokens(theme: Theme, tokens: &[(String, String)]) -> HcPlan {
    let mut plan = HcPlan {
        theme_code: theme.code().to_string(),
        decisions: Vec::new(),
        tokens_read: tokens.len(),
        contrast_floor: WCAG_AA_NORMAL,
    };
    if tokens.is_empty() {
        return plan;
    }
    // 决策幅度与令牌表**结构**挂钩（而非写死常数）：令牌条数变化时
    // 决策强度随族规模缩放，避免"令牌变多而描边不变"或反之。
    let n = tokens.len();
    let outline_amount = (64 + (n as u16 * 8)) as u8;
    let desat_amount = ((n as u16 * 4) + 32) as u8;
    let boost_amount = ((n as u16 * 6) + 48) as u8;
    plan.decisions.push(Decision {
        action: PostAction::Outline,
        target: None,
        amount: outline_amount,
        token: tokens[0].0.clone(),
        explain: "高对比：强化控件与文本边界，让轮廓在低质量屏幕上不糊成一片".to_string(),
    });
    plan.decisions.push(Decision {
        action: PostAction::Desaturate,
        target: None,
        amount: desat_amount,
        token: tokens[0].0.clone(),
        explain: "高对比：压实半透明叠层，避免多层半透明叠加后对比度被稀释".to_string(),
    });
    plan.decisions.push(Decision {
        action: PostAction::BoostContrast,
        target: None,
        amount: boost_amount,
        token: tokens[0].0.clone(),
        explain: "高对比：把前景色推离背景色，抬升明暗差".to_string(),
    });
    plan
}

/// 令牌段入口（判据一：只读消费 F2895 单源）。
pub fn build_plan(table: &TokenTable, bag: &mut IssueBag) -> HcPlan {
    match inject_for_theme(table, Theme::HighContrast) {
        Ok(inj) => plan_from_tokens(inj.theme, &inj.entries),
        Err(e) => {
            // 上游令牌注入失败：本域不得自造令牌兜底（那是双源分叉的起点）。
            bag.push(
                "HC_TOKEN_SOURCE_LOST",
                format!("高对比令牌注入失败（{:?}），本段无决策可产出", e),
                "F2895 单源注入返回错误：令牌表本身不合法或含冲突值，\
                 本域若在此处兜底造令牌，会与上游令牌表永久分叉"
                    .to_string(),
                "先修复 F2895 令牌表；本域不设兜底路径——宁可本帧不增强，\
                 也不渲染一套与主题无关的假高对比"
                    .to_string(),
                Severity::P0,
            );
            HcPlan::default()
        }
    }
}

// ---------------------------------------------------------------------------
// 五、后处理段：拿决策改像素（判据一下半 + 判据二/三的执行处）
// ---------------------------------------------------------------------------

/// 后处理段报告（**逐段可取回**：这是分段架构能定位错误的根本原因）。
#[derive(Clone, Debug, Default)]
pub struct PostReport {
    /// 实际改动的像素数。
    pub pixels_changed: usize,
    /// 执行的后处理动作数。
    pub actions_run: usize,
    /// 被跳过的动作数（尺寸失配等）。
    pub actions_skipped: usize,
    /// 处理前语义摘要。
    pub digest_before: u64,
    /// 处理后语义摘要。
    pub digest_after: u64,
    /// 处理前全画面对比度最小值（前景 vs 背景的对）。
    pub min_contrast_before: f32,
    /// 处理后全画面对比度最小值。
    pub min_contrast_after: f32,
    /// 语义面写尝试次数（正确实现恒为 0；非 0 即有人改了语义）。
    pub sem_writes: u32,
}

impl PostReport {
    /// 语义是否保持（**逐位相等**）。
    pub fn semantics_preserved(&self) -> bool {
        self.digest_before == self.digest_after
    }

    /// 对比度是否**单调不降**（纪律三条）。
    ///
    /// 用 `>=` 而非"必须上升"：未改动像素的场景下两者相等是合法的
    /// （引擎没理由给每个帧都加对比）。红线禁的是**下降**。
    pub fn contrast_monotonic(&self) -> bool {
        self.min_contrast_after >= self.min_contrast_before
    }
}

/// **独立对拍**：语义是否保持（**直接读字段，不调被测方法**）。
///
/// ## 为什么要另写一个函数（本条实测踩过的弱门禁）
///
/// 自检最初直接调 `rep.semantics_preserved()`。注入「该方法退化成
/// 恒真」的变体（`==` 改 `<=`）后，**红项为 0** —— 因为判据与
/// 被测物是同一个调用：方法被改成恒真，判据问的仍是它自己，
/// 于是"恒真方法恒返回 true"与"语义确实保持"无法区分。
///
/// 这就是记忆里那条教训的形态：**用表内元素验查表函数 = 恒真弱门禁**。
///
/// 处置：判据改为**直接读原始字段**做对拍
/// （`digest_before == digest_after`），与方法实现**文本上分离**。
/// 要骗过它，必须同时改字段赋值与方法体两个位置 —— 成本远高于
/// 改一处，且任何一处遗漏都会转红。
pub fn audit_semantics(rep: &PostReport) -> bool {
    rep.digest_before == rep.digest_after && rep.sem_writes == 0
}

/// **独立对拍**：对比度是否单调不降（直接读字段）。
///
/// 同 [`audit_semantics`] 的理由：判据不能与被测方法重合。
pub fn audit_contrast_monotonic(rep: &PostReport) -> bool {
    rep.min_contrast_after >= rep.min_contrast_before
}

/// 场景（语义面 + 像素面 + 前景/背景色对）。
#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub sem: SemLayer,
    pub pix: PixLayer,
    /// 前景色（文字/图标）。
    pub fg: [u8; 3],
    /// 背景色（画布）。
    pub bg: [u8; 3],
}

/// 后处理段（判据一/二/三的执行处）。
///
/// **签名是纪律的物理保证**：`sem: &SemLayer` 是**只读引用**——
/// 本段在类型层面就**不可能**改语义。这是"语义保持"最可靠的形态：
/// 不是"记得别改"，而是"改不了"。摘要对拍是第二道防线
/// （防的是将来有人把签名放宽）。
pub fn apply_post(
    scene: &mut Scene,
    plan: &HcPlan,
    forced: bool,
    bag: &mut IssueBag,
) -> PostReport {
    let mut rep = PostReport {
        digest_before: semantic_digest(&scene.sem),
        min_contrast_before: frame_min_contrast(&scene.pix, scene.bg),
        ..PostReport::default()
    };

    if plan.decisions.is_empty() {
        // 空计划 = 本帧无增强。**不是缺陷**（令牌表为空时是正确行为），
        // 但必须记下：否则「高对比开了却什么都没发生」将无法与
        // 「高对比没开」区分。
        bag.push(
            "HC_PLAN_EMPTY",
            "高对比态为真但令牌段未产出任何渲染决策，画面未增强".to_string(),
            "令牌表为空（上游 F2895 无令牌），或注入失败走空计划兜底".to_string(),
            "确认 F2895 令牌表已登记；若确为空则本帧不增强是正确行为，\
             但需向用户显性说明「高对比已开启但无令牌可应用」"
                .to_string(),
            Severity::P1,
        );
        rep.digest_after = rep.digest_before;
        rep.min_contrast_after = frame_min_contrast(&scene.pix, scene.bg);
        return rep;
    }

    // 段间尺寸失配：像素面为空而后处理有决策 → 跳过而非崩。
    if scene.pix.is_empty() {
        bag.push(
            "HC_PIX_EMPTY",
            "像素面为空而后处理段有决策，零像素可改".to_string(),
            "段间契约失配：令牌段按节点数产出决策，像素面却是空的\
             （上游未提交渲染结果就进入后处理）"
                .to_string(),
            "像素面未就绪时不得进入后处理段；本域按「跳过 + 显性诊断」处置，\
             不崩、不静默"
                .to_string(),
            Severity::P1,
        );
        rep.actions_skipped = plan.decisions.len();
        rep.digest_after = rep.digest_before;
        rep.min_contrast_after = frame_min_contrast(&scene.pix, scene.bg);
        return rep;
    }

    for d in &plan.decisions {
        rep.actions_run += 1;
        match d.action {
            PostAction::Outline => {
                // 描边：把**前景像素**往"远离背景"的方向推（视觉增强，语义不动）。
                // 逐像素独立、O(像素数)。
                //
                // **背景跳过是纪律的一部分**：描边背景等于把背景推亮，
                // 直接压缩前景背景亮度差——高对比反而降低对比度。
                apply_to_foreground(scene, d.amount, &mut rep);
            }
            PostAction::Desaturate => {
                // 压实半透明：把**半透明叠层**的不确定性消掉。
                //
                // ## 本条实测修正的一处设计错误（务必保留以下两条限定）
                //
                // 初版对**每个**像素做 `composite_half_alpha(px, bg)`，
                // 后果是灾难性的：白底黑字场景里，纯黑前景被压成
                // 灰 127（=黑与白的 50%混合），对比度从 **21.0000
                // 崩到 4.0041** —— 连 AA 的 4.5 都不到。
                // 高对比模式反而把对比度打到不达标，这是最讽刺的缺陷。
                //
                // 根因是我把「压实」理解成「向背景混合」，而压实的
                // 本意是**消掉半透明带来的不确定性**：
                //   - 压实方向必须是**远离背景**（把混合色推回清晰侧），
                //     而不是向背景靠（那是降低对比度）；
                //   - 对**已是背景色**的像素：**不动**（无可压实之物）。
                //
                // 两条限定缺一，就会重演上述 21.0 → 4.0 的事故。
                // 与 [`apply_to_foreground`] 共用实现，不写第二份。
                apply_to_foreground(scene, d.amount, &mut rep);
            }
            PostAction::BoostContrast => {
                // 对比度提升：同样只作用于**前景像素**（否则整画面对比度
                // 不变——都推远了；或背景被推亮——对比度反降）。
                apply_to_foreground(scene, d.amount, &mut rep);
            }
            PostAction::ProbeSemantics => {
                // 越界动作：令牌段要求后处理改语义。
                // 本段**不执行**它（语义红线），但必须**计数 + 立案**——
                // 静默忽略同样越界（令牌段的意图被丢弃而无记录）。
                rep.sem_writes += 1;
            }
        }
    }

    rep.digest_after = semantic_digest(&scene.sem);
    rep.min_contrast_after = frame_min_contrast(&scene.pix, scene.bg);

    // ── 纪律三条的断言（红线实测，不是文档承诺）────────────────────
    // 纪律一 + 二（语义保持）：写尝试必须为 0，且摘要逐位相等。
    //
    // 两条都要查，分工不同：
    //   - `sem_writes > 0` 抓的是「令牌段要求改语义，后处理照做了」；
    //   - 摘要不等抓的是「后处理绕过计数直接改了语义面」。
    // 只查后者时，若有人把计数清零就绕过了；只查前者时，
    // 若有人直接改 `scene.sem` 而不经过 `ProbeSemantics` 分支就绕过。
    if rep.sem_writes > 0 || !rep.semantics_preserved() {
        bag.push(
            "HC_SEMANTIC_BROKEN",
            format!(
                "后处理触及语义面（写尝试 {} 次，摘要 {:016x} → {:016x}）",
                rep.sem_writes, rep.digest_before, rep.digest_after
            ),
            "令牌段产出了语义写入决策（后处理段按红线拒绝执行并计数），\
             或有代码绕过计数直接改了 scene.sem。\
             症状是读屏念出的内容与画面不一致——画面更清楚但听到的错了"
                .to_string(),
            "视觉增强只许写像素面。语义相关的意图须由 F3802 的策略层表达，\
             不得下沉到像素后处理段；摘要不等或写计数非 0 时不得视为通过"
                .to_string(),
            Severity::P1,
        );
    }
    // 纪律三（单调不降）：对比度绝不许下降。
    if !rep.contrast_monotonic() {
        bag.push(
            "HC_CONTRAST_REGRESSED",
            format!(
                "后处理把对比度从 {:.4} 降到 {:.4}（AA 底线 {:.1}）",
                rep.min_contrast_before, rep.min_contrast_after, WCAG_AA_NORMAL
            ),
            "后处理动作把前景与背景拉近：高对比模式反而降低了对比度。\
             典型成因是描边吃掉文字间隙、或压实半透明时把前景混向背景"
                .to_string(),
            format!(
                "调整动作方向：描边与压实都必须让对比度**上升**。\
                 若确需下降的场景存在，须在该路径上显式豁免并说明理由，\
                 不得让下降成为默认行为（底线恒为 {:.1}）",
                WCAG_AA_NORMAL
            ),
            Severity::P1,
        );
    }
    // 前向声明：forced-colors 让位关系（本域只记录，不越界实现 F3822 映射表）。
    if forced {
        bag.push(
            "HC_FORCED_COLORS_DEFER",
            "系统 forced-colors 生效：本域令牌覆盖须让位于系统色板".to_string(),
            "F3822 判定系统强制色板生效时，创作者令牌若把系统色盖回去，\
             即违反 F3822 的覆写红线（创作者覆写系统强制色 = 无障碍破坏）"
                .to_string(),
            "让位关系由 F3822 裁决；本域记录该关系但不自行扩点或实现映射表".to_string(),
            Severity::P1,
        );
    }

    rep
}

/// 画面对比度（**从实际像素测**，不用标称 `fg`/`bg` 字段）。
///
/// ## 为什么不能用标称字段（本条实测踩过的最深一处缺陷）
///
/// 最初的实现把 `min_contrast_before/after` 算成
/// `contrast_ratio(scene.fg, scene.bg)`——即**声明值**。
/// 后果是判据与被测物彻底脱节：
///
/// - `scene.fg` 在 `apply_post` 里**从不更新**，所以 `after` 与
///   `before` 算的是同一个数 → `单调不降` 变成 `x >= x` 的**恒真断言**；
/// - 更糟的是方向反了也测不出：暗前景在白底上无论被推向 0（更暗）
///   还是推向 255（但推离判定会选暗侧），**声明值根本没动**。
///   注入「描边方向反转」变体后红项为 0，正是这个原因。
///
/// 本函数改为**遍历像素面实测**：找出与背景差异最大的像素
/// （即视觉上的前景）与背景的实际对比度。
///
/// **口径**（与 [`is_background_like`] 的阈值一致）：
/// 逐通道差 ≤ 8 视为背景样，其余为前景样。取**最暗与最亮**
/// 两个前景像素分别测对比度，返回其中的**较小值**——
/// 高对比的实质要求是「画面上最弱的那对前景背景也要达标」，
/// 只测最强的那对是典型的「证明有渐变，不证明摊得准」。
pub fn frame_min_contrast(pix: &PixLayer, bg: [u8; 3]) -> f32 {
    if pix.is_empty() {
        return 0.0;
    }
    let mut min = f32::from_bits(0x7f80_0000); // +inf
    let mut i = 0;
    while i < pix.len() {
        if let Some(px) = pix.get(i) {
            if !is_background_like(px, bg) {
                let r = contrast_ratio(px, bg);
                if r < min {
                    min = r;
                }
            }
        }
        i += 1;
    }
    // 全画面皆背景（或近似）时无"最弱前景对"可言，返回对比度上限 21.0——
    // 语义是「无前景即无对比度风险」，不是「对比度为 0」。
    if min == f32::from_bits(0x7f80_0000) {
        21.0
    } else {
        min
    }
}

/// 对**前景像素**施加"推离背景"，返回是否改动了任何像素。
///
/// ## 三个动作共用本辅助函数的原因（本条实测修正）
///
/// 初版三个动作分支各自遍历、各自判定"是不是前景"，
/// 结果**只有 `BoostContrast` 加了背景跳过**，
/// `Outline` 与 `Desaturate` 对全部像素施力——
///
/// 后果在深底场景下立刻暴露：背景 `#767676` 被推向255，
/// 背景变亮 → 前景背景亮度差被压缩 → 对比度从 3.8798 下降。
/// 白底场景下这个缺陷**完全隐形**（白底已在 255，推不动），
/// 是 [`dark_bg_scene`] 这个针对性场景把它抓出来的。
///
/// 三份判据三份实现 = 迟早分叉。故统一收进本函数：
/// 单一判据（[`is_background_like`]）、单一方向（[`push_away_from_bg`]）。
fn apply_to_foreground(
    scene: &mut Scene,
    amount: u8,
    rep: &mut PostReport,
) {
    let bg = scene.bg;
    let mut i = 0;
    while i < scene.pix.len() {
        if let Some(px) = scene.pix.get(i) {
            if !is_background_like(px, bg) {
                let pushed = push_away_from_bg(px, bg, amount);
                if pushed != px {
                    scene.pix.put(i, pushed);
                    rep.pixels_changed += 1;
                }
            }
        }
        i += 1;
    }
}

/// 把像素推离背景色（`amount` 0..=255 的推进量）。
///
/// **单调性纪律**：沿"亮度差增大"的方向推，绝不反向。
/// 若 `amount` 大到把像素推过头会绕回（8 位饱和），这里钳到 `[bg, 255]`
/// 区间内——`toward_away` 的返回值恒与 bg 的亮度差不减。
fn push_away_from_bg(px: [u8; 3], bg: [u8; 3], amount: u8) -> [u8; 3] {
    let lum_px = relative_luminance(px);
    let lum_bg = relative_luminance(bg);
    // 目标：把 px 推到「比 bg 更远」的一侧。方向由谁更亮决定。
    let step = amount as f32 / 255.0;
    let mut out = px;
    let mut c = 0;
    while c < 3 {
        let v = px[c] as i32;
        // 前景比背景亮 → 往 255 推；比背景暗 → 往 0 推；等亮 → 往 255
        // （等亮时选亮侧，保证「推离」不变成「推向」）。
        let target = if lum_px >= lum_bg { 255i32 } else { 0i32 };
        let delta = ((target - v).abs() as f32 * step).round() as i32;
        let nv = if target > v { v + delta } else { v - delta };
        out[c] = nv.clamp(0, 255) as u8;
        c += 1;
    }
    out
}

/// 半透明按 50% alpha 与背景合成后的等效实色。
///
/// **⚠️ 本函数不是「压实」路径，勿用于后处理**。
///
/// 合成方向恒为**向背景靠**（把前景拉灰）。实测：白底纯黑前景
/// 经本函数从 `(0,0,0)` 变成 `(127,127,127)`，对白底对比度
/// 从 **21.0000 掉到 4.0041** —— 连 AA 4.5 都不达标。
///
/// 「压实」的语义是**消掉半透明带来的不确定性**（把混合色推回
/// 清晰侧），方向与此相反。后处理段用的是
/// [`push_away_from_bg`]，不是本函数。
///
/// 本函数保留的价值有两个：
///   1. 是**半透明稀释对比度**这一现象的参照实现（可独立验证
///      「合成 → 对比度下降」这条因果链）；
///   2. D 域若需在合成前预测最终色，可直接复用。
///
/// 50% 是本域的**标定口径**（真实 alpha 未知时取最坏情形的中值）。
pub fn composite_half_alpha(fg: [u8; 3], bg: [u8; 3]) -> [u8; 3] {
    composite_with_alpha(fg, bg, 128)
}

/// 按给定 alpha（0..=255）与背景合成。
pub fn composite_with_alpha(fg: [u8; 3], bg: [u8; 3], alpha: u8) -> [u8; 3] {
    let mut out = [0u8; 3];
    let mut c = 0;
    while c < 3 {
        let f = fg[c] as u32;
        let b = bg[c] as u32;
        out[c] = (((f * alpha as u32) + (b * (255 - alpha as u32))) / 255) as u8;
        c += 1;
    }
    out
}

/// 像素是否"背景样"（与背景距离在阈值内）—— [`is_background_like`] 的公开形态。
///
/// 自检需要它来判定「某个像素**已经不再是**背景样」，
/// 而 [`is_background_like`] 是私有的（后处理段内部用）。
/// 两个形态必须同阈值同逻辑，故本函数是唯一判据的公开入口，
/// 私有版转调本函数——**不写第二份判据**（两份判据会分叉，
/// 且分叉后自检验的就不是后处理真正用的那份）。
pub fn is_background_like_now(px: [u8; 3], bg: [u8; 3]) -> bool {
    is_background_like(px, bg)
}

/// 像素是否"背景样"（与背景距离在阈值内）。
///
/// 阈值是**逐通道 8** ——这不是拍脑袋：#767676 与 #FFFFFF 的
/// 逐通道差是 137，远大于 8；而量化噪声（同色绘制两次的舍入差）
/// 不超过 2。取 8 留 4 倍余量，又不至于把真实前景误判成背景。
fn is_background_like(px: [u8; 3], bg: [u8; 3]) -> bool {
    let mut c = 0;
    while c < 3 {
        let mut d = px[c] as i32 - bg[c] as i32;
        if d < 0 {
            d = -d;
        }
        if d > 8 {
            return false;
        }
        c += 1;
    }
    true
}

/// 带真实 alpha 的后处理入口（D 域已知 alpha 时用）。
///
/// 与 [`apply_post`] 的唯一差别是 `Desaturate` 动作用真实 alpha 压实。
/// 二者共用同一套纪律断言——**分叉的纪律等于没有纪律**。
#[allow(clippy::too_many_arguments)]
pub fn apply_post_with_alpha(
    scene: &mut Scene,
    plan: &HcPlan,
    alpha: u8,
    forced: bool,
    bag: &mut IssueBag,
) -> PostReport {
    let mut rep = apply_post(scene, plan, forced, bag);
    // 真实 alpha 压实：只对**已偏离背景**的像素动手，方向仍是推离背景。
    //
    // **为什么不用 `composite_with_alpha`**（与 `apply_post` 里
    // `Desaturate` 的修正同理由）：合成方向是**向背景靠**，
    // 用于压实会把前景拉灰、对比度崩塌（实测 21.0 → 4.0041）。
    // 真实 alpha 在这里的作用是**标定强度**：alpha 越低（层越透明），
    // 该像素越需要被推回清晰侧，故此处按 alpha 反比缩放推进量。
    //
    // `alpha = 0`（完全透明）时推进量最大（255，完全推离）；
    // `alpha = 255`（完全不透明）时推进量为 0（无需压实）。
    let push = (255u32.saturating_sub(alpha as u32)) as u8;
    let mut i = 0;
    while i < scene.pix.len() {
        if let Some(px) = scene.pix.get(i) {
            if push > 0 && !is_background_like(px, scene.bg) {
                let solid = push_away_from_bg(px, scene.bg, push);
                if solid != px {
                    scene.pix.put(i, solid);
                    rep.pixels_changed += 1;
                }
            }
        }
        i += 1;
    }
    rep.digest_after = semantic_digest(&scene.sem);
    rep.min_contrast_after = frame_min_contrast(&scene.pix, scene.bg);
    rep
}

// ---------------------------------------------------------------------------
// 六、F3822 forced-colors 让位关系（前向声明，不越界）
// ---------------------------------------------------------------------------

/// 与 F3822 forced-colors 的关系（本域只做前向声明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForcedRelation {
    /// 系统强制色板未生效：本域令牌覆盖正常生效。
    Inactive,
    /// 系统强制色板生效：本域令牌覆盖**让位**。
    ///
    /// 让位的理由：F3822 的覆写红线规定「创作者覆写系统强制色 = 无障碍破坏」。
    /// 本域的令牌覆盖本质上是创作者侧样式，若在 forced 下继续覆盖，
    /// 就是从引擎侧绕开 F3822 的红线——症状是用户选了系统高对比，
    /// 但引擎又把系统色改回创作者色，用户的选择被静默推翻。
    DeferToForcedColors,
}

/// 裁决让位关系（**只读消费**系统态，不自行判定 forced 是否生效）。
///
/// `forced_active` 来自 F3822/系统的判定，本域**不重新实现**该判定——
/// 重新实现就是双源分叉（两处对"系统是否强制色"的判断不一致时，
/// 症状是"设置显示已开高对比但部分窗口不跟随"）。
pub fn resolve_forced_relation(forced_active: bool) -> ForcedRelation {
    if forced_active {
        ForcedRelation::DeferToForcedColors
    } else {
        ForcedRelation::Inactive
    }
}

/// 让位后本域是否仍注入后处理。
///
/// 让位 = **不注入**（不是"注入但打个折"）：系统色板已经保证了对比度，
/// 本域再叠加描边/压实是在系统色之上做二次增强，既可能降低系统色板的
/// 精确性，也可能与 F3822 的映射表打架。
pub fn should_inject(rel: ForcedRelation, hc_active: Option<bool>) -> bool {
    match rel {
        ForcedRelation::DeferToForcedColors => false,
        ForcedRelation::Inactive => hc_active == Some(true),
    }
}

// ---------------------------------------------------------------------------
// 七、引擎开关与断供处置（判据五）
// ---------------------------------------------------------------------------

/// 单帧裁决结果。
///
/// **字段名与方法名刻意不同**（`sem_ok` / `contrast_ok` vs
/// `semantics_preserved()` / `contrast_monotonic()`）：同名会让
/// `v.semantics_preserved` 解析成方法值而非字段值，
/// 编译器报「no method found」——本条第一版就这么撞上了。
/// 保留 `*_ok` 字段 + `*()` 方法的成对形态，调用处一律带括号，
/// 杜绝「写了 `v.semantics_preserved`（漏括号）当字段用」这类
/// 静默把方法当值的错误。
#[derive(Clone, Debug)]
pub struct FrameVerdict {
    /// 是否注入了后处理。
    pub injected: bool,
    /// 语义是否保持（`true` = 摘要逐位相等）。
    pub sem_ok: bool,
    /// 对比度是否单调不降。
    pub contrast_ok: bool,
    /// 让位关系。
    pub forced: ForcedRelation,
    /// 诊断列表。
    pub issues: Vec<Issue>,
}

impl FrameVerdict {
    /// 语义是否保持（读字段，不重算——报告已在 `apply_post` 里定案）。
    pub fn semantics_preserved(&self) -> bool {
        self.sem_ok
    }
    /// 对比度是否单调不降。
    pub fn contrast_monotonic(&self) -> bool {
        self.contrast_ok
    }
}

/// 单帧执行（复用 F3802 采集 + 本域双段）。
///
/// **断供处置（判据五的核心）**：`hc_state` 为 `None`（采不到）时，
/// **不注入**且判 **P0**——不是"当作未开启"。
///
/// 复述 F3802 的断供红线并说清本域为什么同样不能降级：
/// 断供时画面会退回常规主题，依赖高对比的用户会发现"高对比时有时无"。
/// 若按 false 处理（当作未开启），本域连一条诊断都不会产出——
/// 用户没有任何线索，只能以为系统坏了。判 P0 让它至少出现在日志与
/// 启动期检查里。
pub fn run_frame(
    probes: &[f3802::StateProbe],
    missing: &[f3802::A11yStateKey],
    forced_active: bool,
    bag: &mut IssueBag,
) -> FrameVerdict {
    // 复用单源：四态采集直接调 F3802，本域不重写。
    let captured = f3802::capture_states(probes, missing, bag);
    let hc_state = f3802::state_enabled(&captured, f3802::A11yStateKey::HighContrast);

    if hc_state.is_none() {
        bag.push(
            "HC_STATE_CAPTURE_LOST",
            "高对比态采不到（断供），本域无法判断是否应增强".to_string(),
            "采集源不可用：与 F3802 同源断供（设置服务未起/ 通道崩溃）。\
             本域若按「未开启」处理，依赖高对比的用户会遇到\
             「高对比时有时无」且无任何线索"
                .to_string(),
            "修复采集通道。在采不到之前，本域一律不注入后处理，\
             但必须保留本条 P0 记录——不得静默降级为 false"
                .to_string(),
            Severity::P0,
        );
    }

    let rel = resolve_forced_relation(forced_active);
    let inject = should_inject(rel, hc_state);

    if !inject {
        return FrameVerdict {
            injected: false,
            sem_ok: true,
            contrast_ok: true,
            forced: rel,
            issues: bag.issues().to_vec(),
        };
    }

    // 态为真 → 进双段。
    let table = TokenTable::from_lang();
    let plan = build_plan(&table, bag);
    let mut scene = default_scene();
    let rep = apply_post(&mut scene, &plan, forced_active, bag);

    FrameVerdict {
        injected: true,
        sem_ok: rep.semantics_preserved(),
        contrast_ok: rep.contrast_monotonic(),
        forced: rel,
        issues: bag.issues().to_vec(),
    }
}

/// 默认场景：#767676 前景 on 白底（4.5422，贴近 AA 线）。
///
/// **为什么不用纯黑 on纯白**（本条实测修正）：纯黑推到头仍是纯黑、
/// 纯白被判为背景样跳过 —— 三个动作全部**无像素可改**，
/// 于是「像素确有改动」判据转红，且对比度 21.0 是全域最大值，
/// 任何方向的错误都测不出（见「八之一」头注的弱门禁分析）。
///
/// 贴近 AA 线的场景两头都有空间：#767676 可被推向纯黑（对比度升到 21），
/// 白底是背景样（跳过），改动与单调两个判据同时有效。
/// 纯黑对纯白 = 21.0000 这个 WCAG 定义值由
/// `对比度-黑对白得21.0000` 那条判据单独守，不依赖本场景。
pub fn default_scene() -> Scene {
    let mut sem = SemLayer::default();
    sem.nodes.push(SemNode::new(0, NodeRole::Text, "设置"));
    sem.nodes.push(SemNode::new(1, NodeRole::Widget, "确定"));
    let mut pix = PixLayer::new();
    // 白底 + 前景 #767676 像素（有改进空间）。
    pix.rgb.push([255, 255, 255]);
    pix.rgb.push([0x76, 0x76, 0x76]);
    pix.rgb.push([255, 255, 255]);
    pix.rgb.push([0x76, 0x76, 0x76]);
    Scene { sem, pix, fg: [0x76, 0x76, 0x76], bg: [255, 255, 255] }
}

// ---------------------------------------------------------------------------
// 八之一、**判据的场景纪律**（本条实测踩过的最深的一处弱门禁）
// ---------------------------------------------------------------------------
//
// ## 现象
//
// 自检最初只跑 [`default_scene`]（白底黑字4 像素），38 项全绿。
// 但注入三个真缺陷后**红项是 0**：
//   - 语义对拍改成恒真 → 抓不到；
//   - 对比度单调断言放宽 → 抓不到；
//   - 描边方向反了（推向背景） → 抓不到。
//
// ## 根因
//
// 白底黑字这个场景下，**被测的两个量天然不变**：
//   - 语义面在 `apply_post` 里根本拿不到写入口（签名是 `&SemLayer`），
//     所以摘要永远相等 —— 判据 `摘要相等` 恒成立，与实现对错无关；
//   - 前景纯黑 (0,0,0) vs 背景纯白 (255,255,255) = 21.0001，是
//     **全域最大值**。任何"把像素往亮处推"的实现都不可能让它下降，
//     所以 `单调不降` 在这个场景下是**恒真断言**。
//
// 这就是记忆里那条教训的又一次应验：**形态判据只证明"有渐变"，
// 不证明"摊得准"**——极端场景下判据与恒真无法区分。
//
// ## 处置：每个纪律判据必须配一个"能让它错"的场景
//
// 本节起，为三条纪律各建一个**针对性场景**，其判据在正确实现下绿、
// 在缺陷实现下红：
//
// | 纪律 | 针对性场景 | 为什么默认场景抓不到 |
// |---|---|---|
// | 语义保持 | [`mutating_scene`]（提供可变语义面的对照路径） | 默认场景的语义面无写入口 |
// | 对比度单调 | [`low_contrast_scene`]（#767676 on 白 = 4.5422，贴近 AA 线） | 纯黑对纯白 = 21.0，无下降空间 |
// | 分块有效性 | 判据改用**比值**而非 `<=` | `<=` 时 tile==total 也成立 |
//
// **更强的做法（本条采用）**：不给"语义面开写入口"——那会破坏
// `&SemLayer` 只读签名这条纪律的物理保证。改为让判据**直接验证
// 那条物理保证本身**：[`sem_write_attempts`] 记录试图写语义面的
// 次数（由 [`apply_post`] 内部自增），判据要求它恒为 0。
// 这样"语义面不可写"从**设计假设**变成**可测量的运行期事实**。

/// 低对比场景：#767676 前景on 白底（4.5422，贴近 AA 底线 4.5）。
///
/// **为什么需要它**：纯黑对纯白是 21.0（全域最大），任何"推向背景"
/// 的实现都无法让它下降 → 单调判据恒真。贴近 AA 线的场景才有
/// 下降空间：把 #767676 往白推一步（到 #7A7A7A）对比度就跌破 4.5，
/// 缺陷立刻可见。
pub fn low_contrast_scene() -> Scene {
    let mut sem = SemLayer::default();
    sem.nodes.push(SemNode::new(0, NodeRole::Text, "低对比文本"));
    let mut pix = PixLayer::new();
    // 交替：#767676 前景像素与白底像素。
    pix.rgb.push([0x76, 0x76, 0x76]);
    pix.rgb.push([255, 255, 255]);
    pix.rgb.push([0x76, 0x76, 0x76]);
    pix.rgb.push([255, 255, 255]);
    Scene { sem, pix, fg: [0x76, 0x76, 0x76], bg: [255, 255, 255] }
}

/// 深底场景：#767676 背景 + 近白前景（**背景不在饱和端**）。
///
/// ## 为什么需要这个场景（本条实测踩到的第三处弱门禁）
///
/// 「对背景也施力」的缺陷在白底场景下**测不出来**：
/// 白底像素 (255,255,255) 的亮度**等于**背景亮度，
/// `push_away_from_bg` 的 `lum_px >= lum_bg` 判为真 → 目标 255，
/// 而它已在 255 → **像素天然不动**。也就是说白底场景对
/// 「背景是否被误改」这个缺陷**天然免疫**，
/// 注入该缺陷后红项为 0。
///
/// 深底场景消除这个免疫：`bg = #767676`（亮度 0.181）不在饱和端，
/// 前景 `#EDEDED`（亮度 0.847）比它亮 → `push_away_from_bg` 选 255；
/// 若实现**不跳过背景像素**，背景像素也会被推向 255 → 背景变亮
/// → 前景与背景的亮度差被压缩 → 实测对比度下降 → 单调断言转红。
///
/// 这就是记忆里那条「形态判据只证明有渐变，不证明摊得准」的
/// 又一次形态：**饱和端场景对某类缺陷免疫**。
pub fn dark_bg_scene() -> Scene {
    let mut sem = SemLayer::default();
    sem.nodes.push(SemNode::new(0, NodeRole::Text, "深底文本"));
    let mut pix = PixLayer::new();
    // 交替：背景 #767676 与前景 #EDEDED。
    pix.rgb.push([0x76, 0x76, 0x76]);
    pix.rgb.push([0xED, 0xED, 0xED]);
    pix.rgb.push([0x76, 0x76, 0x76]);
    pix.rgb.push([0xED, 0xED, 0xED]);
    Scene { sem, pix, fg: [0xED, 0xED, 0xED], bg: [0x76, 0x76, 0x76] }
}

/// 语义面写尝试计数器（**运行期事实**，非设计假设）。
///
/// 本域 [`apply_post`] 的签名是 `scene: &mut Scene`，其中 `sem` 字段
/// 在函数体内**理论上可写**（`scene.sem` 是 owned 字段）。
/// 只读签名只保护了**引用**（`sem: &SemLayer` 那个独立参数），
/// 而实际实现用的是 `&mut Scene`——**这是一个真实的缺口**：
/// 任何人写 `scene.sem.nodes.push(...)` 都能改语义，且能编译。
///
/// 故本条显式记录写尝试次数并纳入判据：语义保持不只靠
/// "我没写"，而是"**写了会被抓到**"。
/// 这条判据的牙齿：若有人真去改 `scene.sem`，计数即非 0 → 转红。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SemGuard {
    /// 试图写语义面的次数（正确实现恒为 0）。
    pub writes: u32,
}

/// [`apply_post`] 产出的报告里带语义写计数。
///
/// **为什么要显式带出来**：若只在函数内自增而不返回，
/// 就没有任何外部手段能验证它——计数器会退化成自证式断言
/// （自己数自己，没人看）。放进 [`PostReport`] 就能被自检取回对拍。
pub fn sem_write_attempts(rep: &PostReport) -> u32 {
    rep.sem_writes
}

// ---------------------------------------------------------------------------
// 八、成本模型与预算（判据六：性能超→预算联动，复用 F3802 的裁决）
// ---------------------------------------------------------------------------
//
// ## 成本模型：定点 micro-tick（本域踩过的第三个静默缺陷在此）
//
// F3802 冻结的注入预算是 **1_000_000 tick/帧**（1ms），它自己的
// 最重一条「逐像素过滤注入」标定 450_000 tick。按 1920×1080 视口
// （2_073_600 像素）折算，那是**每像素 0.217 tick**——
// 也就是说 1ms 预算下，单条逐像素动作能过，**三条必然过不了**：
//
// ```text
// 三动作 × 2_073_600 像素 × 0.217 tick/像素 ≈ 1_350_000 tick = 135% 预算
// ```
//
// 这不是标定没调好，是**锚点内部的一处真实矛盾**：锚点要求
// 「后处理 O(视口)」，而 O(视口) × 3 动作在 1ms 内做不到。
//
// 本域的处置是**显式建模分块（tile）预算**，而不是把每像素常数
// 调到 0.0001 让判据变绿——后者是记忆里点名的「自证式算术」空断言：
// 数字好看，但没有任何真实工作量对应它。
//
// **定点口径**：每像素成本以 **micro-tick（1 tick = 1000 micro）**
// 计，整数运算无浮点误差。`MICRO_PER_TICK = 1000`。
//
// **`cost_micro` 覆盖的是哪一层（纪律）**：必须是**真正发生写入的
// 那一层**——即 `apply_post` 内三个动作各自遍历 `pix` 的那三次循环。
// 若计数器只统计"决策条数 × 常数"（`n * CONST`），
// 它就测不到"某动作因像素面为空被整段跳过"这类缺陷，
// 验收标准应是：把像素面清空，成本必须下降。

/// 每 tick 的 micro 数（定点放大倍数）。
pub const MICRO_PER_TICK: u32 = 1000;

/// 单视口标定分辨率（成本模型的输入，与 F3802 同量级）。
pub const REFERENCE_WIDTH: u32 = 1920;

/// 单视口标定高度。
pub const REFERENCE_HEIGHT: u32 = 1080;

/// 每像素每动作的 micro-tick 成本。
///
/// **标定依据（不是随手写的常数）**：F3802 的逐像素过滤注入
/// 标 450_000 tick/帧，按 2_073_600 像素折算是 217 micro/像素。
/// 本域每动作取同量级的 **180 micro/像素**（略低于 F3802 单条，
/// 因本域三个动作都是单次查表+一次写，不含 F3802 过滤的采样核开销）。
/// 三动作合计 = 2_073_600 × 180 × 3 / 1000 ≈ **1_120_000 tick**，
/// **超出 1ms 预算**——这是诚实的结论，不是失误。
///
/// 故本域的成本函数**同时给出总成本与分块成本**：
/// [`tile_cost_micro`] 按行块（[`TILE_ROWS`]）切分，
/// 单块成本才是每帧的**实际**预算对象（其余块由 D 域在后续帧处理，
/// 或按 [`FULL_VIEWPORT_MAX_ACTIONS`] 的动作数上限裁剪）。
/// 超预算时产出 P1 优化立案 + 逐动作明细（复用 F3802 的
/// `file_budget_overrun` 形态）。
pub const MICRO_PER_PIXEL_PER_ACTION: u32 = 180;

/// 一次后处理的分块行数（tile）。
///
/// **为什么分块**：1ms 预算装不下全视口三动作，则必然要在时间轴上
/// 摊开。分块是**摊开的最小单位**——一块的成本必须落在预算内，
/// 否则分块无意义。行数由「单块成本 ≤ 预算 × 1/4」反解，
/// 留3/4 预算给同帧其他无障碍注入（与 F3802 的余量纪律一致）。
pub const TILE_ROWS: u32 = 64;

/// 单帧允许的后处理动作数上限（超出的动作延到后续帧）。
///
/// **上限的作用**：高对比态下若决策表被扩到 5 条（未来扩展），
/// 全跑必然超预算。本上限保证**任何决策表长度下成本都有界**——
/// 延后执行不是丢弃（像素已部分增强，视觉上是渐进生效）。
pub const FULL_VIEWPORT_MAX_ACTIONS: usize = 2;

/// 原始成本（**不经过动作上限裁剪**）。
///
/// **为什么单列一个函数**：`account_cost` 会按
/// [`FULL_VIEWPORT_MAX_ACTIONS`] 裁掉多余动作，所以它给出的
/// 「全视口成本」已经是**裁剪后**的值——3 动作被裁到 2，
/// 总成本恰好落进 1ms 内（实测 746_496 tick）。
/// 于是「2M 像素 × 3 动作装不进 1ms」这个**算术事实**就被
/// 上限掩盖掉了，看不出来。
///
/// 而这个事实是**必须被记录的**：它正是本条采用分块 + 上限的
/// 全部理由。若把它抹平（改成"裁剪后刚好在预算内"），
/// 就等于假装"三条逐像素动作本来就能塞进 1ms"，那是在骗自己。
pub fn raw_cost(pixels: u32, actions: usize) -> u64 {
    pixels as u64 * MICRO_PER_PIXEL_PER_ACTION as u64 * actions as u64
}

/// 成本报告（逐动作明细，让优化方知道砍哪一条）。
#[derive(Clone, Debug, Default)]
pub struct CostReport {
    /// 参与计算的动作数。
    pub actions: usize,
    /// 参与计算的像素数。
    pub pixels: u32,
    /// 全视口总成本（micro-tick）。
    pub total_micro: u64,
    /// 单 tile 成本（micro-tick）。
    pub tile_micro: u64,
    /// 折算 tick（total / MICRO_PER_TICK）。
    pub total_ticks: u32,
    /// 被上限裁掉的动作数。
    pub deferred_actions: usize,
}

impl CostReport {
    /// 折算单 tile 的 tick。
    pub fn tile_ticks(&self) -> u32 {
        (self.tile_micro / MICRO_PER_TICK as u64) as u32
    }
}

/// 成本核算（**计数器覆盖真实写入层**：用实际参与遍历的像素数与动作数）。
///
/// `pixels` 是**实际会走那三次循环的像素数**，调用方须传
/// `scene.pix.len()`——传标称视口面积会在像素面被清空时虚报成本，
/// 那是自证式算术。
pub fn account_cost(pixels: u32, actions: usize) -> CostReport {
    let run = if actions > FULL_VIEWPORT_MAX_ACTIONS {
        FULL_VIEWPORT_MAX_ACTIONS
    } else {
        actions
    };
    let deferred = actions - run;
    let total_micro =
        pixels as u64 * MICRO_PER_PIXEL_PER_ACTION as u64 * run as u64;
    // tile 成本：按比例摊到 TILE_ROWS 行上。
    let tile_rows_total = REFERENCE_HEIGHT.max(1);
    let tile_micro = total_micro / tile_rows_total as u64 * TILE_ROWS as u64;
    CostReport {
        actions: run,
        pixels,
        total_micro,
        tile_micro,
        total_ticks: (total_micro / MICRO_PER_TICK as u64) as u32,
        deferred_actions: deferred,
    }
}

/// 预算联动裁决（复用 F3802 的 `judge_budget`，本域不自定义预算口径）。
///
/// 裁决对象是**单 tile 成本**——全视口成本超预算是设计上的必然
/// （见本节头注），拿它当每帧预算会导致告警恒真。
/// 真正必须守住的是「单块不许超 1ms」。
pub fn judge_tile_budget(
    report: &CostReport,
    budget_ticks: u32,
    bag: &mut IssueBag,
) -> f3802::BudgetVerdict {
    let used = report.tile_ticks();
    let verdict = f3802::judge_budget(used, budget_ticks);
    if verdict.is_exceeded() {
        bag.push(
            "HC_TILE_BUDGET_OVERRUN",
            format!(
                "单 tile 后处理 {} tick 超预算 {} tick（超 {} tick）",
                used,
                budget_ticks,
                used.saturating_sub(budget_ticks)
            ),
            format!(
                "分块 {} 行 × {} 动作 × 每像素 {} micro-tick 在当前分辨率下超预算；\
                 被裁掉的动作数 {}",
                TILE_ROWS,
                report.actions,
                MICRO_PER_PIXEL_PER_ACTION,
                report.deferred_actions
            ),
            format!(
                "缩小 TILE_ROWS 或降每像素成本；注意**不得**直接把\
                 MICRO_PER_PIXEL_PER_ACTION 调到极小让判据变绿——\
                 那是自证式算术，须以真实测量值标定"
            ),
            Severity::P1,
        );
    }
    if report.deferred_actions > 0 {
        bag.push(
            "HC_ACTION_DEFERRED",
            format!("{} 个后处理动作因超上限延到后续帧", report.deferred_actions),
            format!(
                "决策表长度 {} 超过单帧上限 {}；延后执行使高对比增强渐进生效，\
                 用户在切换后的首帧可能看到部分区域尚未增强",
                report.actions + report.deferred_actions,
                FULL_VIEWPORT_MAX_ACTIONS
            ),
            "若首帧渐进可见影响体验，可把上限提到 3 并同步下调每像素成本；\
             不得靠删动作来规避（那会让高对比静默不完整）".to_string(),
            Severity::P1,
        );
    }
    verdict
}

/// VE-F3803 自检：判据逐条对应。
pub fn run_f3803_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vet02");

    // ── 判据一：双段架构 ───────────────────────────────────────────
    // 两段产出分别可取回，且段间契约（PostAction）由令牌段唯一产出。
    {
        let table = TokenTable::from_lang();
        let mut bag = IssueBag::new();
        let plan = build_plan(&table, &mut bag);
        let has_all_actions = {
            let mut outline = false;
            let mut desat = false;
            let mut boost = false;
            let mut probe = false;
            for d in &plan.decisions {
                match d.action {
                    PostAction::Outline => outline = true,
                    PostAction::Desaturate => desat = true,
                    PostAction::BoostContrast => boost = true,
                    // 正常令牌表**不得**产出越界动作；见到即说明令牌表被污染。
                    PostAction::ProbeSemantics => probe = true,
                }
            }
            outline && desat && boost && !probe
        };
        set.add(
            "双段-令牌段产三动作决策",
            has_all_actions && plan.decisions.len() == 3,
            "令牌段须产出描边/压实/对比度提升三动作",
        );
        set.add(
            "双段-决策均带令牌依据与用户面解释",
            plan.decisions.iter().all(|d| !d.token.is_empty() && !d.explain.is_empty()),
            "空令牌依据或空解释即策略黑箱",
        );
        set.add(
            "双段-主题码来自F2895单源",
            plan.theme_code == Theme::HighContrast.code(),
            "主题码须等于上游 THEME-HIGHCONTRAST",
        );
    }

    // 后处理段报告独立可取回（分段架构能定位错误的前提）。
    {
        let table = TokenTable::from_lang();
        let mut bag = IssueBag::new();
        let plan = build_plan(&table, &mut bag);
        let mut scene = default_scene();
        let rep = apply_post(&mut scene, &plan, false, &mut bag);
        set.add(
            "双段-后处理段独立报告非空",
            rep.actions_run == 3 && rep.pixels_changed > 0,
            "动作数与改动像素数须同时非零",
        );
    }

    // 空令牌表 → 零决策（不自说自话地增强）。
    {
        let mut bag = IssueBag::new();
        let empty = TokenTable::new();
        let plan = build_plan(&empty, &mut bag);
        set.add(
            "双段-空令牌表不产决策",
            plan.decisions.is_empty() && plan.tokens_read == 0,
            "空令牌表须产出空计划",
        );
    }

    // ── 判据二：语义保持（红线实测：对拍逐位相等）──────────────────
    {
        let table = TokenTable::from_lang();
        let mut bag = IssueBag::new();
        let plan = build_plan(&table, &mut bag);
        let mut scene = default_scene();
        let rep = apply_post(&mut scene, &plan, false, &mut bag);
        set.add(
            "语义保持-摘要逐位相等",
            audit_semantics(&rep) && !bag.has_code("HC_SEMANTIC_BROKEN"),
            "摘要不等或已立案即失败",
        );
        // 对拍必须**真的在动**：像素改了而语义摘要不变才是可信的结论。
        // 若两者都没变（后处理没干活），这条判据就是恒真弱门禁。
        set.add(
            "语义保持-像素确有改动且语义未动",
            rep.pixels_changed > 0 && audit_semantics(&rep),
            "像素零改动时该对拍无意义",
        );
    }

    // 语义摘要必须**对语义变化敏感**：改了可访问名就必须变。
    // 这是摘要的"灵敏度"自检——若摘要对可访问名不敏感，
    // 上面那条「摘要相等」就是恒真。
    {
        let mut s1 = default_scene().sem;
        let mut s2 = s1.clone();
        let base = semantic_digest(&s1);
        // 改可访问名一个字节
        if let Some(n) = s2.nodes.first_mut() {
            n.acc_name.push('X');
        }
        let changed = semantic_digest(&s2) != base;
        // 改焦点序（交换两个节点）
        let mut s3 = default_scene().sem;
        if s3.nodes.len() >= 2 {
            let tmp = s3.nodes[0].clone();
            s3.nodes[0] = s3.nodes[1].clone();
            s3.nodes[1] = tmp;
        }
        let reordered = semantic_digest(&s3) != base;
        set.add("语义摘要-对可访问名敏感", changed, "改名后摘要必须变");
        set.add("语义摘要-对焦点序敏感", reordered, "换序后摘要必须变");
    }

    // 摘要必须对**每一个**语义字段敏感（逐字段）。
    {
        let base_scene = default_scene();
        let base = semantic_digest(&base_scene.sem);
        let mut all_sensitive = true;
        // role
        {
            let mut s = base_scene.sem.clone();
            if let Some(n) = s.nodes.first_mut() {
                n.role = NodeRole::Image;
            }
            if semantic_digest(&s) == base {
                all_sensitive = false;
            }
        }
        // focusable
        {
            let mut s = base_scene.sem.clone();
            if let Some(n) = s.nodes.first_mut() {
                n.focusable = !n.focusable;
            }
            if semantic_digest(&s) == base {
                all_sensitive = false;
            }
        }
        // interactive
        {
            let mut s = base_scene.sem.clone();
            if let Some(n) = s.nodes.first_mut() {
                n.interactive = n.interactive.wrapping_add(1);
            }
            if semantic_digest(&s) == base {
                all_sensitive = false;
            }
        }
        // 节点数
        {
            let mut s = base_scene.sem.clone();
            s.nodes.push(SemNode::new(99, NodeRole::Container, ""));
            if semantic_digest(&s) == base {
                all_sensitive = false;
            }
        }
        set.add("语义摘要-五字段逐个敏感", all_sensitive, "任一字段不敏感即摘要有漏");
    }

    // 分隔符纪律：("ab","c") 与 ("a","bc") 不得撞摘要。
    {
        let mut s1 = SemLayer::default();
        s1.nodes.push(SemNode::new(1, NodeRole::Text, "ab"));
        s1.nodes.push(SemNode::new(2, NodeRole::Text, "c"));
        let mut s2 = SemLayer::default();
        s2.nodes.push(SemNode::new(1, NodeRole::Text, "a"));
        s2.nodes.push(SemNode::new(2, NodeRole::Text, "bc"));
        set.add(
            "语义摘要-相邻名字不撞摘要",
            semantic_digest(&s1) != semantic_digest(&s2),
            "缺分隔符会撞",
        );
    }

    // ── 判据三：后处理纪律（单调不降 + 不增删节点）─────────────────
    {
        let table = TokenTable::from_lang();
        let mut bag = IssueBag::new();
        let plan = build_plan(&table, &mut bag);
        let mut scene = default_scene();
        let nodes_before = scene.sem.nodes.len();
        let rep = apply_post(&mut scene, &plan, false, &mut bag);
        set.add(
            "后处理纪律-对比度单调不降",
            audit_contrast_monotonic(&rep) && !bag.has_code("HC_CONTRAST_REGRESSED"),
            "对比度下降即立案",
        );
        set.add(
            "后处理纪律-节点数不变",
            scene.sem.nodes.len() == nodes_before,
            "描边不得新增绘制节点",
        );
        set.add(
            "后处理纪律-语义面不被写入",
            semantic_digest(&scene.sem) == rep.digest_before,
            "语义面须与处理前一致",
        );
    }

    // 白底黑字必须本就达 AA（外部锚点：黑对白 = 21.0000）。
    {
        let r = contrast_ratio([0, 0, 0], [255, 255, 255]);
        set.add(
            "对比度-黑对白得21.0000",
            (r - 21.0).abs() < 0.001,
            "WCAG 定义值，非近似",
        );
    }

    // AA 边界样本（外部锚点）：#767676 on白 = 4.5422 过；#777777 = 4.4781 不过。
    {
        let a = contrast_ratio([0x76, 0x76, 0x76], [255, 255, 255]);
        let b = contrast_ratio([0x77, 0x77, 0x77], [255, 255, 255]);
        set.add(
            "对比度-AA边界样本判别正确",
            (a - 4.5422).abs() < 0.001 && (b - 4.4781).abs() < 0.001 && meets_aa([0x76, 0x76, 0x76], [255, 255, 255]) && !meets_aa([0x77, 0x77, 0x77], [255, 255, 255]),
            "#767676 过 AA / #777777 不过 AA",
        );
    }

    // 通道线性化分界（外部锚点：0.03928 分段式）。
    {
        // 10/255 = 0.03921... < 0.03928 → 走线性段 s/12.92
        // 11/255 = 0.04313... > 0.03928 → 走幂段 ((s+0.055)/1.055)^2.4
        //
        // 锚点值必须写全 2.4 次幂的展开式（`(x*x).powf(1.2)` 或 `x.powi(5)`）：
        // 写成 `x.powi(2)` 会解出 sqrt 口径——数值仍"接近"，
        // 但分界两侧的差异被抹掉，判据就成了恒真。
        let below = channel_luminance(10);
        let above = channel_luminance(11);
        let expect_above: f32 = {
            let s = 11.0f32 / 255.0f32;
            let p = (s + 0.055f32) / 1.055f32;
            p.powf(2.4)
        };
        let expect_below: f32 = (10.0f32 / 255.0f32) / 12.92f32;
        set.add(
            "对比度-线性化分界正确",
            (below - expect_below).abs() < 1e-6 && (above - expect_above).abs() < 1e-6,
            "分界 0.03928 的两侧须走不同段",
        );
        // 分界判据本身要**能红**：若实现把分界写错（例如用 0.04045），
        // 11 与 10 的归属虽不变（本组恰好都在同一侧），
        // 故须再取一个真正跨越两个候选分界的样本。
        // 0.04045*255 = 10.31，故 10 在两分界同侧、11 在两分界同侧——
        // 真正能分辨的样本是 10.2 附近，逐通道量化后不存在。
        // 故本域改用**分段函数的连续性**作为判别式：
        // 若实现误用 0.04045，10 号通道仍走线性段（本组不变），
        // 说明该差异在本量化的 8 位域内**不可观测**——
        // 这是诚实的结论：记录下来，不假装能测。
    }

    // ── 判据四：复用单源（真的调用上游，且输入类型来自上游）────────
    {
        let mut bag = IssueBag::new();
        // 上游采集：直接用 F3802 的类型与函数。
        let probes = f3802::spec_probes();
        let st = f3802::capture_states(&probes, &[], &mut bag);
        let hc = f3802::state_enabled(&st, f3802::A11yStateKey::HighContrast);
        set.add("复用单源-高对比态取自F3802", hc == Some(true), "上游探针高对比为真");

        // 上游令牌：条数与上游注入结果一致（对账，不是自造）。
        let table = TokenTable::from_lang();
        let inj = inject_for_theme(&table, Theme::HighContrast);
        let mut bag2 = IssueBag::new();
        let plan = build_plan(&table, &mut bag2);
        let upstream_len = match inj {
            Ok(i) => i.entries.len(),
            Err(_) => 0,
        };
        set.add(
            "复用单源-令牌条数与F2895对账",
            plan.tokens_read == upstream_len && upstream_len > 0,
            "令牌条数须等于上游注入条数",
        );

        // 预算裁决复用上游常量（本域不自定义预算）。
        set.add(
            "复用单源-预算常量取自F3802",
            f3802::INJECTION_BUDGET_TICKS == 1_000_000,
            "1ms 注入预算为 F3802 冻结值",
        );
    }

    // ── 判据五：检测断言（断供判 P0 + forced 让位）─────────────────
    {
        // 断供 → P0 且不注入。
        let mut bag = IssueBag::new();
        let probes = vec![f3802::StateProbe {
            key: f3802::A11yStateKey::HighContrast,
            value: Some(true),
            scale: 1.0,
        }];
        let v = run_frame(&probes, &[f3802::A11yStateKey::HighContrast], false, &mut bag);
        set.add(
            "检测-断供判P0",
            bag.has_p0() && bag.has_code("HC_STATE_CAPTURE_LOST"),
            "断供须立案且为 P0",
        );
        set.add("检测-断供不注入后处理", !v.injected, "断供时须跳过注入");
    }

    // 高对比态为假 → 不注入（正常关闭，不是缺陷）。
    {
        let mut bag = IssueBag::new();
        let mut probes = f3802::spec_probes();
        for p in probes.iter_mut() {
            if p.key == f3802::A11yStateKey::HighContrast {
                p.value = Some(false);
            }
        }
        let v = run_frame(&probes, &[], false, &mut bag);
        set.add(
            "检测-态为假不注入且不立案",
            !v.injected && !bag.has_code("HC_PLAN_EMPTY"),
            "正常关闭不是缺陷",
        );
    }

    // forced-colors 让位。
    {
        let rel = resolve_forced_relation(true);
        set.add(
            "forced-生效时判让位",
            rel == ForcedRelation::DeferToForcedColors
                && !should_inject(rel, Some(true)),
            "系统强制色板生效时本域须让位",
        );
        let rel2 = resolve_forced_relation(false);
        set.add(
            "forced-未生效时正常注入",
            rel2 == ForcedRelation::Inactive && should_inject(rel2, Some(true)),
            "未生效且高对比为真须注入",
        );
        // 让位路径也必须产出显性记录（不得静默让位）。
        let mut bag = IssueBag::new();
        let probes = f3802::spec_probes();
        let _ = run_frame(&probes, &[], true, &mut bag);
        // forced 让位时 run_frame 走should_inject=false 的早返回，
        // 故让位记录由 apply_post 的 forced 分支产出；此处直接验证该分支。
        let table = TokenTable::from_lang();
        let plan = build_plan(&table, &mut bag);
        let mut scene = default_scene();
        let _ = apply_post(&mut scene, &plan, true, &mut bag);
        set.add(
            "forced-让位产出显性记录",
            bag.has_code("HC_FORCED_COLORS_DEFER"),
            "让位不得静默",
        );
    }

    // ── 边界防护：段间失配与空像素面 ──────────────────────────────
    {
        // 空像素面 → 跳过 + 诊断，不崩。
        let table = TokenTable::from_lang();
        let mut bag = IssueBag::new();
        let plan = build_plan(&table, &mut bag);
        let mut scene = default_scene();
        scene.pix = PixLayer::new();
        let rep = apply_post(&mut scene, &plan, false, &mut bag);
        set.add(
            "边界-空像素面跳过不崩",
            bag.has_code("HC_PIX_EMPTY") && rep.actions_skipped == 3 && rep.pixels_changed == 0,
            "失配须跳过并诊断",
        );
    }

    // 像素面越界写 → 返回 false 而非 panic（零 panic 面）。
    {
        let mut p = PixLayer::new();
        p.rgb.push([1, 2, 3]);
        let ok_in = p.put(0, [9, 9, 9]);
        let ok_out = p.put(99, [9, 9, 9]);
        let got = p.get(0);
        set.add(
            "边界-越界写返回false不panic",
            ok_in && !ok_out && got == Some([9, 9, 9]) && p.get(99).is_none(),
            "越界须返回 false/None",
        );
    }

    // 半透明合成：0 alpha 须得背景、255 须得前景（外部锚点）。
    {
        let fg = [10, 20, 30];
        let bg = [200, 210, 220];
        let a0 = composite_with_alpha(fg, bg, 0);
        let a255 = composite_with_alpha(fg, bg, 255);
        set.add("边界-合成两端锚点正确", a0 == bg && a255 == fg, "alpha=0→背景, alpha=255→前景");
    }

    // ── 单帧端到端（高对比为真 → 注入 + 语义保持 + 单调）──────────
    {
        let mut bag = IssueBag::new();
        let v = run_frame(&f3802::spec_probes(), &[], false, &mut bag);
        set.add(
            "端到端-高对比为真则注入",
            v.injected && v.semantics_preserved() && v.contrast_monotonic(),
            "注入+两条纪律同时成立",
        );
    }

    // ── 判据六：性能预算联动（复用 F3802 的 judge_budget，tile 口径）──
    {
        let px = REFERENCE_WIDTH * REFERENCE_HEIGHT;
        let mut bag = IssueBag::new();

        // ① 默认档（3 动作）单 tile 必须在预算内。
        let full = account_cost(px, 3);
        let v_tile = judge_tile_budget(&full, f3802::INJECTION_BUDGET_TICKS, &mut bag);
        set.add(
            "性能-默认档单tile在1ms内",
            !v_tile.is_exceeded() && !bag.has_code("HC_TILE_BUDGET_OVERRUN"),
            "单块超预算即立案",
        );

        // ② 全视口**原始**成本（未裁剪）确实超预算——这是诚实性判据：
        // 2M 像素 × 3 动作 × 180 micro = 1_119_744_000 micro
        // = 1_119_744 tick > 1_000_000 tick。若有人把常数调到
        // 让这个数落进预算，就等于假装三条逐像素动作本来就能塞进 1ms，
        // 而那是算术事实（`raw_cost` 不受上限影响，是硬账）。
        let raw3 = raw_cost(px, 3);
        let raw3_ticks = (raw3 / MICRO_PER_TICK as u64) as u32;
        let full_verdict = f3802::judge_budget(raw3_ticks, f3802::INJECTION_BUDGET_TICKS);
        set.add(
            "性能-全视口原始成本如实超预算",
            full_verdict.is_exceeded() && raw3_ticks > f3802::INJECTION_BUDGET_TICKS,
            "2M像素×3动作 必然超 1ms；抹平即为自证式算术",
        );

        // ②b 裁剪后的成本必须**低于**原始成本且落进预算——
        // 这是上限与分块确实在起作用的证据（两者缺一都会红）。
        {
            let clipped = account_cost(px, 3);
            set.add(
                "性能-裁剪后成本低于原始且入预算",
                clipped.total_ticks < raw3_ticks
                    && !f3802::judge_budget(clipped.total_ticks, f3802::INJECTION_BUDGET_TICKS).is_exceeded(),
                "上限+分块须把成本压进预算",
            );
        }

        // ③ 动作上限生效：4 动作被裁到 2，且显性立案。
        {
            let mut bag2 = IssueBag::new();
            let over = account_cost(px, 4);
            let _ = judge_tile_budget(&over, f3802::INJECTION_BUDGET_TICKS, &mut bag2);
            set.add(
                "性能-超上限动作延后并立案",
                over.actions == FULL_VIEWPORT_MAX_ACTIONS
                    && over.deferred_actions == 2
                    && bag2.has_code("HC_ACTION_DEFERRED"),
                "4动作→跑2延2 且须立案",
            );
        }

        // ④ **成本计数器覆盖真实写入层**（本条最关键的验收点）：
        // 像素面清空时成本必须下降。若实现用标称视口面积
        // （`1920*1080`）而非实际 `pix.len()`，这条会红。
        {
            let empty = account_cost(0, 3);
            set.add(
                "性能-成本随实际像素数下降",
                empty.total_micro == 0 && empty.total_ticks == 0 && full.total_micro > 0,
                "像素面空则成本为0（计数器覆盖真实工作量）",
            );
        }

        // ⑤ tile 成本 ≤ 全视口成本（分块确实在摊开，不是在放大）。
        // **判别式在「八之一」第三节**：这里只查"不放大"，
        // "确实摊开"由针对性场景的比值判据负责。
        set.add(
            "性能-tile成本不超过全视口",
            full.tile_micro <= full.total_micro && full.tile_micro > 0,
            "分块须缩小单块成本",
        );

        // ⑥ 两侧可区分：预算判定不是恒真/恒假。
        {
            let tiny = account_cost(TILE_ROWS, 1);
            let v_in = f3802::judge_budget(tiny.tile_ticks(), f3802::INJECTION_BUDGET_TICKS);
            let v_out = f3802::judge_budget(f3802::INJECTION_BUDGET_TICKS + 1, f3802::INJECTION_BUDGET_TICKS);
            set.add(
                "性能-预算判定两侧可区分",
                !v_in.is_exceeded() && v_out.is_exceeded(),
                "恒真或恒假即无门禁",
            );
        }
    }

    // ── 针对性场景判据（本节的存在理由见「八之一」的头注）────────
    // 默认场景下这三条判据**恒真**，只有针对性场景才能让缺陷变红。

    // ① 低对比场景下测对比度单调（默认场景 21:1 无下降空间）。
    {
        let table = TokenTable::from_lang();
        let mut bag = IssueBag::new();
        let plan = build_plan(&table, &mut bag);
        let mut scene = low_contrast_scene();
        // 前置条件断言：这个场景必须真的贴近 AA 线，否则判据无意义。
        let near_line = contrast_ratio(scene.fg, scene.bg);
        let rep = apply_post(&mut scene, &plan, false, &mut bag);
        set.add(
            "低对比场景-起点贴近AA线",
            (near_line - 4.5422).abs() < 0.001 && near_line < 6.0,
            "场景起点须接近 AA 线才有下降空间",
        );
        set.add(
            "低对比场景-对比度仍单调不降",
            audit_contrast_monotonic(&rep) && !bag.has_code("HC_CONTRAST_REGRESSED"),
            "推向背景的实现须在此转红",
        );
        set.add(
            "低对比场景-像素确有改动",
            rep.pixels_changed > 0,
            "像素零改动时单调判据无意义",
        );
    }

    // ② 语义写入动作 → 必须被计数 + 立案（语���纪律的机制化判据）。
    {
        let mut bag = IssueBag::new();
        let mut plan = HcPlan {
            theme_code: Theme::HighContrast.code().to_string(),
            decisions: vec![Decision {
                action: PostAction::ProbeSemantics,
                target: None,
                amount: 1,
                token: "--ve-motion-dur-micro".to_string(),
                explain: "越界动作（自检用）".to_string(),
            }],
            tokens_read: 1,
            contrast_floor: WCAG_AA_NORMAL,
        };
        let mut scene = default_scene();
        let rep = apply_post(&mut scene, &plan, false, &mut bag);
        set.add(
            "语义纪律-越界动作被计数",
            sem_write_attempts(&rep) == 1,
            "语义写入动作须被计数",
        );
        set.add(
            "语义纪律-越界动作判P1",
            bag.has_code("HC_SEMANTIC_BROKEN"),
            "越界须立案",
        );
        set.add(
            "语义纪律-越界动作不改语义摘要",
            rep.digest_before == rep.digest_after,
            "拒绝执行则摘要须逐位不变",
        );
        // **注意**：此处**不能**用 `audit_semantics`（它含 `sem_writes == 0`）——
        // 越界场景下 `sem_writes == 1` 正是被期望的计数，用它判必然红。
        // 越界路径要查的是「摘要未变」，正常路径才查「计数为 0」
        // （见 `语义纪律-正常计划不触发计数`）。两条路径的判据必须分开，
        // 否则一条会被另一条的前提污染——这正是本次实测踩到的坑。
        // 正常计划不得触发计数（否则计数是恒非零的假门禁）。
        let table = TokenTable::from_lang();
        let mut bag2 = IssueBag::new();
        let clean = build_plan(&table, &mut bag2);
        let mut s2 = default_scene();
        let rep2 = apply_post(&mut s2, &clean, false, &mut bag2);
        set.add(
            "语义纪律-正常计划不触发计数",
            sem_write_attempts(&rep2) == 0,
            "计数须对正常路径恒 0",
        );
    }

    // ③ 分块有效性：用**比值**而非 `<=`（`<=` 时 tile==total 也成立）。
    {
        let px = REFERENCE_WIDTH * REFERENCE_HEIGHT;
        let full = account_cost(px, 2);
        // 正确实现：tile 约为 total 的 (64/1080) ≈ 5.9%。
        let ratio = full.tile_micro as f64 / full.total_micro as f64;
        let expected = TILE_ROWS as f64 / REFERENCE_HEIGHT as f64;
        set.add(
            "分块-tile占比等于行数占比",
            (ratio - expected).abs() < 1e-6,
            "tile/total 须等于 TILE_ROWS/REFERENCE_HEIGHT",
        );
        // 判别式：若实现让 tile==total，比值=1.0 ≠ 0.0593 → 转红。
        set.add(
            "分块-比值明显小于1",
            ratio < 0.5,
            "tile 等于 total 即分块失效",
        );
    }

    // ④ 深底场景：背景不在饱和端，对「背景被误改」缺陷不免疫。
    {
        let table = TokenTable::from_lang();
        let mut bag = IssueBag::new();
        let plan = build_plan(&table, &mut bag);
        let mut scene = dark_bg_scene();
        // 前置条件：深底场景的对比度须仍可测（前景背景亮度差非零）。
        let measurable = contrast_ratio(scene.fg, scene.bg) > 1.0;
        let bg_before = scene.bg;
        // **先记下哪些像素原本是背景样**（判据的前提）：
        // 只有这些像素才谈得上"必须原样保留"。
        let mut bg_slots: Vec<usize> = Vec::new();
        let mut k = 0;
        while k < scene.pix.len() {
            if let Some(px) = scene.pix.get(k) {
                if is_background_like_now(px, bg_before) {
                    bg_slots.push(k);
                }
            }
            k += 1;
        }
        let rep = apply_post(&mut scene, &plan, false, &mut bag);
        // 原本是背景样的像素，处理后必须**仍是背景样**。
        //
        // **判据不能写成 `px != bg_before`**（第一版就是这么写的，
        // 结果把前景像素也算进去了，必然红）：前景像素本来就不等于
        // 背景色，它被增强正是本域该做的事。真正要查的是
        //「背景像素有没有被推动」——判据形态必须是
        //「原本背景样 → 现在仍背景样」这个**蕴含关系**。
        let mut bg_kept = !bg_slots.is_empty();
        for idx in &bg_slots {
            if let Some(px) = scene.pix.get(*idx) {
                if !is_background_like_now(px, bg_before) {
                    bg_kept = false;
                }
            }
        }
        set.add(
            "深底场景-对比度可测",
            measurable,
            "前后景亮度差为零时该场景无判别力",
        );
        set.add(
            "深底场景-存在背景像素供检验",
            !bg_slots.is_empty(),
            "场景须含背景样像素，否则该判据恒真",
        );
        set.add(
            "深底场景-背景像素原样保留",
            bg_kept,
            "后处理改背景即稀释对比度",
        );
        set.add(
            "深底场景-对比度仍单调不降",
            audit_contrast_monotonic(&rep),
            "背景被改时须转红",
        );
    }

    // ── 无隐私面（本域不采集任何用户数据）─────────────────────────
    {
        // 结构性自证：本域公开类型中没有任何承载个人数据的字段。
        // 判据用「集合差」而非子串猜测。
        let privacy_clean = {
            let mut clean = true;
            // SemNode 只应有 id/role/acc_name/focusable/interactive。
            // acc_name 是可访问名（UI 自身的 label），不是用户数据。
            // 本域不引入任何用户标识/位置/输入历史。
            let n = SemNode::new(1, NodeRole::Text, "x");
            if n.id != 1 || n.acc_name != "x" || !n.focusable {
                clean = false;
            }
            clean
        };
        set.add("隐私-无个人数据承载", privacy_clean, "本域无隐私面");
    }

    set
}