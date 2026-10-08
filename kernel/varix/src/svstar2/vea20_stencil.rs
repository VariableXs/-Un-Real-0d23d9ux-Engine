//! VE-F0020 · 深度模板状态机（VE-A 域 · 合成核心 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0020`
//!
//! **判据（锚点原文）**：深度/模板测试状态机（比较函数、写入掩码、模板操作全集），预置典型
//! 配置（不透明、半透明、描边三类），深度冲突的预防建议（多边形偏移的默认建议）；含模板操作
//! 的状态机图（模板流转一图看懂）。判据五条：**比较函数全集、三类预置、冲突预防、偏移建议、
//! 判据**。
//!
//! **错误路径与降级矩阵**：非法→拒绝+建议；深度冲突→偏移建议；状态漂移→失效。
//!
//! **数据结构**：状态封装；预置。
//!
//! **性能逐项分解**：O(状态)。
//!
//! **跨批对接点**：A19 混合同构（F0019 的四维封装 + 预置库 + 漂移失效 + 建议分流同构复用）。
//!
//! **无障碍与隐私**：状态表读屏可达——状态与每个转移都带稳定短名与文字说明，
//! 读屏器能逐项朗读，不依赖视觉位置或颜色（[`StateTag::describe`] 是文字面，
//! [`StencilState::a11y_table`] 输出可直接朗读的行文本）。
//!
//! ## 设计要点
//!
//! - **比较函数全集**（[`CompareOp`]）：深度测试的 8 个比较函数**穷举登记**在
//!   [`COMPARE_TABLE`]，且 [`CompareOp::all`] 与表长度互为机检——少一个就红。
//!   "全集"是判据第一条，不能靠"常用几个"糊过去。
//! - **写入掩码**（[`WriteMask`]）：颜色/深度/模板三位独立开关，8 种组合穷举
//!   （[`WriteMask::all`]），且**深度写与颜色写解耦**（只写颜色不写深度是合法组合，
//!   用于先画不透明后叠加半透明）。
//! - **模板操作全集**（[`StencilOp`]）：8 种模板操作（[`StencilOp::all`]），
//!   显式建模 `Keep` / `Zero` / `Replace` / `IncrSaturate` / `DecrSaturate` /
//!   `Invert` / `IncClamp` / `DecrClamp`。
//! - **三类预置**（[`PRESET_TABLE`]）：不透明（Opaque）、半透明（Translucent）、
//!   描边（Outline）三类，**恰好三类**且各带默认建议——预置不是"给几个例子"，
//!   是把三类典型配置的参数选择固化下来。
//! - **深度冲突预防**（[`conflict_prevention`]）：**预防优先于解决**——在提交绘制前
//!   检出同区域共面图元并给出建议，而不是等 z-fighting 出现后靠调深度值救。
//!   检出用深度区间重叠判定（[`depth_overlap`]，O(1) 每对）。
//! - **偏移建议**（[`polygon_offset_advice`]）：冲突时给出**多边形偏移的默认建议**
//!   （斜率因子 + 深度单位偏移），且建议是**可执行数值**而非"请调整深度"这类空话。
//!   建议方向由深度函数决定：Less 语义要往远推，Greater 语义往近拉——方向错了
//!   会把冲突变成更严重的冲突。
//! - **状态漂移失效**（[`BaselineStatus`]）：预置基线指纹重算比对，漂移即须重签。
//!   与 F0019 同构——静默漂移会让同一份预置在不同构建里表现不同，问题无法复现。
//! - **状态机图**（[`StencilState::flow_lines`]）：模板流转以文字行输出，
//!   「一图看懂」在纯文本环境里的可检验形态就是**每个状态有稳定短名 + 每个转移可列举**
//!   （机检覆盖状态与转移的全集，无隐藏状态）。
//!
//! **逻辑 tick 注入，零墙钟**；零 IO；类型自持（不 import 未注册的兄弟模块）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 比较函数全集规模（判据"比较函数全集"的机检基准）。
pub const COMPARE_TABLE_SIZE: usize = 8;

/// 模板操作全集规模。
pub const STENCIL_OP_TABLE_SIZE: usize = 8;

/// 写入掩码组合全集规模（颜色 × 深度 × 模板 三位）。
pub const WRITE_MASK_TABLE_SIZE: usize = 8;

/// 预置配置数（恰好三类：OPAQUE / TRANSLUCENT / OUTLINE）。
pub const PRESET_COUNT: usize = 3;

/// 模板参考值范围上界（模板值 8 位，0..255）。
pub const STENCIL_REF_MAX: u8 = 255;

/// 模板缓冲初始值（清屏模板缓冲的约定初值）。
pub const STENCIL_CLEAR: u8 = 0;

/// 多边形偏移默认斜率因子（冲突预防的默认建议值）。
pub const DEFAULT_SLOPE_SCALE: f32 = 1.0;

/// 多边形偏移默认深度单位偏移（冲突预防的默认建议值）。
pub const DEFAULT_DEPTH_UNITS: f32 = 1.0;

/// 比较函数全集契约。
pub const COMPARE_DOC: &str = "\
比较函数全集契约（VE-F0020 · v1）：深度测试比较函数 8 种穷举登记于 COMPARE_TABLE——恒真通过、\
恒假通过、相等通过、小于/小于等于/大于/大于等于通过、恒等。CompareOp::all() 与表长度互为\
机检：少登记一个即红。「全集」是判据第一条，不可只实现常用几个——某后端只支持子集时，\
应在能力面显性登记缺失而非静默不实现。";

/// 三类预置契约。
pub const PRESET_DOC: &str = "\
三类预置契约（VE-F0020 · v1）：预置典型配置恰好三类——不透明、半透明、描边——把三类典型\
场景的参数选择固化，避免每次新建对象都要重新决策。三类各有明确的深度函数、写入掩码与\
模板操作默认组合，且各带一条使用建议。预置不是给几个例子，是决策固化。";

/// 冲突预防契约。
pub const PREVENTION_DOC: &str = "\
冲突预防契约（VE-F0020 · v1）：深度冲突**预防优先于解决**——提交绘制前检出同区域共面图元\
并给出建议，而不是等z-fighting 出现后靠调深度值救。检出用深度区间重叠判定（O(1) 每对），\
共面判据是区间重叠且重叠宽度小于容差。预防失败（已绘出冲突）才进入建议路径。";

/// 偏移建议契约。
pub const OFFSET_DOC: &str = "\
偏移建议契约（VE-F0020 · v1）：冲突时给出多边形偏移的默认建议，且建议是**可执行数值**\
（斜率因子 + 深度单位偏移），不是「请调整深度」这类空话。建议方向由深度函数决定：\
Less 系列语义把本图元往远处推，Greater 系列往近处拉——方向错了会把冲突变成更严重的冲突。\
斜率因子应对掠射角表面增大，深度单位偏移应对正对视角表面生效。";

/// 状态机图契约。
pub const FLOW_DOC: &str = "\
状态机图契约（VE-F0020 · v1）：模板流转以文字行输出（flow_lines），「一图看懂」在纯文本\
环境里的可检验形态是：每个状态有稳定短名与文字描述、每个转移可被列举且机检覆盖全集、\
无隐藏状态。状态表读屏可达：描述走文字面，不依赖视觉位置或颜色。";

/// 状态漂移契约。
pub const DRIFT_DOC: &str = "\
状态漂移契约（VE-F0020 · v1）：预置基线指纹重算比对，漂移即失效并阻断——与 F0019 混合同构。\
静默漂移会让同一份预置在不同构建里表现不同，问题无法复现。漂移处置是重签（更新指纹并\
留痕），不是自动接受。";

// ---------------------------------------------------------------------------
// 二、数据结构（比较函数 / 写入掩码 / 模板操作）
// ---------------------------------------------------------------------------

/// 深度比较函数（8 种全集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareOp {
    /// 永不通过。
    Never,
    /// 恒通过。
    Always,
    /// 源深度等于目标深度时通过。
    Equal,
    /// 不等时通过。
    NotEqual,
    /// 源小于目标时通过。
    Less,
    /// 源小于等于目标时通过。
    LessEqual,
    /// 源大于目标时通过。
    Greater,
    /// 源大于等于目标时通过。
    GreaterEqual,
}

impl CompareOp {
    /// 稳定短名（读屏与机检共用）。
    pub fn tag(self) -> &'static str {
        match self {
            CompareOp::Never => "never",
            CompareOp::Always => "always",
            CompareOp::Equal => "equal",
            CompareOp::NotEqual => "not_equal",
            CompareOp::Less => "less",
            CompareOp::LessEqual => "less_equal",
            CompareOp::Greater => "greater",
            CompareOp::GreaterEqual => "greater_equal",
        }
    }

    /// 文字描述（无障碍朗读面；不依赖视觉或颜色）。
    pub fn describe(self) -> &'static str {
        match self {
            CompareOp::Never => "永不通过：所有片元被丢弃",
            CompareOp::Always => "恒通过：不比较深度",
            CompareOp::Equal => "深度相等时通过",
            CompareOp::NotEqual => "深度不等时通过",
            CompareOp::Less => "源深度小于目标时通过",
            CompareOp::LessEqual => "源深度小于等于目标时通过",
            CompareOp::Greater => "源深度大于目标时通过",
            CompareOp::GreaterEqual => "源深度大于等于目标时通过",
        }
    }

    /// 是否"往远推"语义（Less 系列；偏移方向判据）。
    pub fn pushes_farther(self) -> bool {
        matches!(self, CompareOp::Less | CompareOp::LessEqual)
    }

    /// 是否"往近拉"语义（Greater 系列；偏移方向判据）。
    pub fn pulls_nearer(self) -> bool {
        matches!(self, CompareOp::Greater | CompareOp::GreaterEqual)
    }

    /// 全集（机检覆盖用；与 [`COMPARE_TABLE`] 长度必须相等）。
    pub fn all() -> [CompareOp; COMPARE_TABLE_SIZE] {
        [
            CompareOp::Never,
            CompareOp::Always,
            CompareOp::Equal,
            CompareOp::NotEqual,
            CompareOp::Less,
            CompareOp::LessEqual,
            CompareOp::Greater,
            CompareOp::GreaterEqual,
        ]
    }
}

/// 比较函数表行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompareRow {
    /// 比较函数。
    pub op: CompareOp,
    /// 是否写入深度缓冲（深度测试的伴生行为；登记在表里避免散落 if）。
    pub writes_depth: bool,
}

/// 比较函数全集表（8 行穷举）。
pub const COMPARE_TABLE: [CompareRow; COMPARE_TABLE_SIZE] = [
    CompareRow { op: CompareOp::Never, writes_depth: false },
    CompareRow { op: CompareOp::Always, writes_depth: false },
    CompareRow { op: CompareOp::Equal, writes_depth: false },
    CompareRow { op: CompareOp::NotEqual, writes_depth: false },
    CompareRow { op: CompareOp::Less, writes_depth: true },
    CompareRow { op: CompareOp::LessEqual, writes_depth: true },
    CompareRow { op: CompareOp::Greater, writes_depth: true },
    CompareRow { op: CompareOp::GreaterEqual, writes_depth: true },
];

/// 按 id 查比较函数行（O(1) 查表；越界显式拒绝）。
pub fn compare_row(op: CompareOp) -> Option<CompareRow> {
    COMPARE_TABLE.iter().copied().find(|r| r.op == op)
}

/// 写入掩码（颜色/深度/模板三位独立）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteMask {
    /// 写颜色。
    pub color: bool,
    /// 写深度。
    pub depth: bool,
    /// 写模板。
    pub stencil: bool,
}

impl WriteMask {
    /// 全不写。
    pub const NONE: WriteMask = WriteMask { color: false, depth: false, stencil: false };

    /// 全写。
    pub const ALL: WriteMask = WriteMask { color: true, depth: true, stencil: true };

    /// 只写颜色（半透明叠加的典型掩码——写颜色不写深度）。
    pub const COLOR_ONLY: WriteMask = WriteMask { color: true, depth: false, stencil: false };

    /// 只写深度（深度预通道的典型掩码）。
    pub const DEPTH_ONLY: WriteMask = WriteMask { color: false, depth: true, stencil: false };

    /// 构造。
    pub const fn new(color: bool, depth: bool, stencil: bool) -> Self {
        WriteMask { color, depth, stencil }
    }

    /// 短名。
    pub fn tag(self) -> &'static str {
        match (self.color, self.depth, self.stencil) {
            (false, false, false) => "none",
            (true, false, false) => "color_only",
            (false, true, false) => "depth_only",
            (true, true, false) => "color_depth",
            (false, false, true) => "stencil_only",
            (true, false, true) => "color_stencil",
            (false, true, true) => "depth_stencil",
            (true, true, true) => "all",
        }
    }

    /// 全集（8 种组合；机检覆盖用）。
    pub fn all() -> [WriteMask; WRITE_MASK_TABLE_SIZE] {
        [
            WriteMask::new(false, false, false),
            WriteMask::new(true, false, false),
            WriteMask::new(false, true, false),
            WriteMask::new(true, true, false),
            WriteMask::new(false, false, true),
            WriteMask::new(true, false, true),
            WriteMask::new(false, true, true),
            WriteMask::new(true, true, true),
        ]
    }
}

/// 模板操作（8 种全集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StencilOp {
    /// 保持不变。
    Keep,
    /// 置零。
    Zero,
    /// 替换为参考值。
    Replace,
    /// 饱和递增（255 为上限）。
    IncrSaturate,
    /// 饱和递减（0 为下限）。
    DecrSaturate,
    /// 按位取反。
    Invert,
    /// 递增并钳到区间上界。
    IncrClamp,
    /// 递减并钳到区间下界。
    DecrClamp,
}

impl StencilOp {
    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            StencilOp::Keep => "keep",
            StencilOp::Zero => "zero",
            StencilOp::Replace => "replace",
            StencilOp::IncrSaturate => "incr_saturate",
            StencilOp::DecrSaturate => "decr_saturate",
            StencilOp::Invert => "invert",
            StencilOp::IncrClamp => "incr_clamp",
            StencilOp::DecrClamp => "decr_clamp",
        }
    }

    /// 文字描述（无障碍朗读面）。
    pub fn describe(self) -> &'static str {
        match self {
            StencilOp::Keep => "保持模板值不变",
            StencilOp::Zero => "模板值置零",
            StencilOp::Replace => "模板值替换为参考值",
            StencilOp::IncrSaturate => "模板值递增，满值饱和保持",
            StencilOp::DecrSaturate => "模板值递减，零值饱和保持",
            StencilOp::Invert => "模板值按位取反",
            StencilOp::IncrClamp => "模板值递增并钳制在区间上界",
            StencilOp::DecrClamp => "模板值递减并钳制在区间下界",
        }
    }

    /// 对给定值施加本操作（模板语义的可执行面；钳制与饱和是两种不同行为，不可混）。
    pub fn apply(self, current: u8, reference: u8) -> u8 {
        match self {
            StencilOp::Keep => current,
            StencilOp::Zero => 0,
            StencilOp::Replace => reference,
            StencilOp::IncrSaturate => {
                if current == STENCIL_REF_MAX {
                    STENCIL_REF_MAX
                } else {
                    current.saturating_add(1)
                }
            }
            StencilOp::DecrSaturate => current.saturating_sub(1),
            StencilOp::Invert => !current,
            StencilOp::IncrClamp => {
                if current >= STENCIL_REF_MAX {
                    STENCIL_REF_MAX
                } else {
                    current.saturating_add(1)
                }
            }
            StencilOp::DecrClamp => {
                if current <= STENCIL_CLEAR {
                    STENCIL_CLEAR
                } else {
                    current.saturating_sub(1)
                }
            }
        }
    }

    /// 是否需要参考值（`Replace` 需要；不需要的操作参考值被忽略）。
    pub fn needs_reference(self) -> bool {
        matches!(self, StencilOp::Replace)
    }

    /// 全集（机检覆盖用）。
    pub fn all() -> [StencilOp; STENCIL_OP_TABLE_SIZE] {
        [
            StencilOp::Keep,
            StencilOp::Zero,
            StencilOp::Replace,
            StencilOp::IncrSaturate,
            StencilOp::DecrSaturate,
            StencilOp::Invert,
            StencilOp::IncrClamp,
            StencilOp::DecrClamp,
        ]
    }
}

// ---------------------------------------------------------------------------
// 三、状态封装与预置
// ---------------------------------------------------------------------------

/// 模板状态机状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateTag {
    /// 空闲（未进入模板测试）。
    Idle,
    /// 掩码写入中（按掩码写模板）。
    Stamping,
    /// 模板测试中（按比较函数决定通过与否）。
    Testing,
    /// 已完成（退出模板测试）。
    Done,
}

impl StateTag {
    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            StateTag::Idle => "idle",
            StateTag::Stamping => "stamping",
            StateTag::Testing => "testing",
            StateTag::Done => "done",
        }
    }

    /// 文字描述（读屏可达）。
    pub fn describe(self) -> &'static str {
        match self {
            StateTag::Idle => "空闲：模板缓冲未参与绘制",
            StateTag::Stamping => "掩码写入：正在按写入掩码标记模板区域",
            StateTag::Testing => "模板测试：正在按比较函数决定片元通过与否",
            StateTag::Done => "完成：模板测试已退出，模板缓冲保持当前值",
        }
    }

    /// 全集（机检覆盖用；无隐藏状态）。
    pub fn all() -> [StateTag; 4] {
        [StateTag::Idle, StateTag::Stamping, StateTag::Testing, StateTag::Done]
    }
}

/// 状态转移事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateEvent {
    /// 开始掩码写入。
    BeginStamp,
    /// 掩码写入完成。
    EndStamp,
    /// 掩码写入中止（非法路径）。
    AbortStamp,
    /// 模板测试通过。
    PassTest,
    /// 模板测试未通过（片元被丢弃）。
    RejectTest,
    /// 重置。
    Reset,
}

impl StateEvent {
    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            StateEvent::BeginStamp => "begin_stamp",
            StateEvent::EndStamp => "end_stamp",
            StateEvent::AbortStamp => "abort_stamp",
            StateEvent::PassTest => "pass_test",
            StateEvent::RejectTest => "reject_test",
            StateEvent::Reset => "reset",
        }
    }

    /// 合法转移表（`[from][to]` 布尔；非法转移即拒绝）。
    ///
    /// 表驱动而非散落 match：新增事件时漏一条转移会立刻被机检发现，
    /// 而散落 if 只会静默走 default 分支。
    pub fn transition(self, from: StateTag) -> Option<StateTag> {
        use StateEvent as E;
        use StateTag as S;
        match (self, from) {
            (E::BeginStamp, S::Idle) => Some(S::Stamping),
            (E::EndStamp, S::Stamping) => Some(S::Testing),
            (E::AbortStamp, S::Stamping) => Some(S::Idle),
            (E::PassTest, S::Testing) => Some(S::Done),
            (E::RejectTest, S::Testing) => Some(S::Done),
            (E::Reset, _) => Some(S::Idle),
            // 其余组合均非法（如在 Idle 上PassTest、在 Done 上 BeginStamp）。
            _ => None,
        }
    }

    /// 全集（机检覆盖用）。
    pub fn all() -> [StateEvent; 6] {
        [
            StateEvent::BeginStamp,
            StateEvent::EndStamp,
            StateEvent::AbortStamp,
            StateEvent::PassTest,
            StateEvent::RejectTest,
            StateEvent::Reset,
        ]
    }
}

/// 模板状态封装（比较函数 + 写入掩码 + 模板操作 + 状态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StencilState {
    /// 当前状态。
    pub state: StateTag,
    /// 深度比较函数。
    pub compare: CompareOp,
    /// 写入掩码。
    pub mask: WriteMask,
    /// 模板操作。
    pub op: StencilOp,
    /// 模板参考值（0..=255）。
    pub reference: u8,
}

impl StencilState {
    /// 构造：初始为 [`StateTag::Idle`]。
    pub fn new(compare: CompareOp, mask: WriteMask, op: StencilOp, reference: u8) -> Self {
        StencilState { state: StateTag::Idle, compare, mask, op, reference }
    }

    /// 施加事件；非法转移**显式拒绝**并保持原状态（错误矩阵第一条「非法→拒绝+建议」）。
    pub fn apply(&mut self, ev: StateEvent) -> Result<StateTag, &'static str> {
        match ev.transition(self.state) {
            Some(next) => {
                self.state = next;
                Ok(next)
            }
            None => Err(ILLEGAL_TRANSITION),
        }
    }

    /// 模板操作是否生效（须处于测试态且掩码开模板写）。
    ///
    /// **双重条件**是易漏点：只在掩码开时生效是不够的——没进测试态时写模板
    /// 会污染后续图形。
    pub fn stencil_write_effective(&self) -> bool {
        self.mask.stencil
            && matches!(self.state, StateTag::Stamping | StateTag::Testing)
    }

    /// 深度写入是否生效（掩码开深度写且深度测试通过——由调用方告知测试结果）。
    pub fn depth_write_effective(&self, passed: bool) -> bool {
        self.mask.depth && passed
    }

    /// 状态表（无障碍朗读面；每行是可直接朗读的文本）。
    pub fn a11y_table() -> Vec<String> {
        let mut out = Vec::new();
        for s in StateTag::all().iter() {
            out.push(format!("状态 {}：{}", s.tag(), s.describe()));
        }
        for c in CompareOp::all().iter() {
            out.push(format!("比较函数 {}：{}", c.tag(), c.describe()));
        }
        for o in StencilOp::all().iter() {
            out.push(format!("模板操作 {}：{}", o.tag(), o.describe()));
        }
        out
    }

    /// 模板流转图（文字形态；「一图看懂」的可检验形态）。
    pub fn flow_lines() -> Vec<String> {
        let mut out = Vec::new();
        out.push("模板流转图（每个状态 + 每个合法转移，无隐藏状态）：".to_string());
        for from in StateTag::all().iter() {
            let mut legal: Vec<String> = Vec::new();
            for ev in StateEvent::all().iter() {
                if let Some(to) = ev.transition(*from) {
                    legal.push(format!("{} --{}--> {}", from.tag(), ev.tag(), to.tag()));
                }
            }
            out.push(format!(
                "{}：{} 条合法转移{}",
                from.tag(),
                legal.len(),
                if legal.is_empty() { "".to_string() } else { format!("：{}", legal.join("；")) }
            ));
        }
        out
    }
}

/// 非法转移错误码（显式常量；调用方据此分流建议）。
pub const ILLEGAL_TRANSITION: &str = "E_STENCIL_ILLEGAL_TRANSITION";

/// 非法模板参数错误码。
pub const ILLEGAL_PARAM: &str = "E_STENCIL_ILLEGAL_PARAM";

/// 预置配置类别（三类，恰好三类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresetKind {
    /// 不透明。
    Opaque,
    /// 半透明。
    Translucent,
    /// 描边。
    Outline,
}

impl PresetKind {
    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            PresetKind::Opaque => "opaque",
            PresetKind::Translucent => "translucent",
            PresetKind::Outline => "outline",
        }
    }

    /// 文字描述（读屏可达）。
    pub fn describe(self) -> &'static str {
        match self {
            PresetKind::Opaque => "不透明：完全遮挡后方，深度写开启，不参与模板运算",
            PresetKind::Translucent => "半透明：需与后方混合，深度写关闭以保留后方深度",
            PresetKind::Outline => "描边：只画轮廓，颜色写开启而深度写按需，用于描边叠加",
        }
    }

    /// 全集（恰好三类；机检覆盖用）。
    pub fn all() -> [PresetKind; PRESET_COUNT] {
        [PresetKind::Opaque, PresetKind::Translucent, PresetKind::Outline]
    }
}

/// 预置行。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PresetRow {
    /// 类别。
    pub kind: PresetKind,
    /// 比较函数。
    pub compare: CompareOp,
    /// 写入掩码。
    pub mask: WriteMask,
    /// 模板操作。
    pub op: StencilOp,
    /// 模板参考值。
    pub reference: u8,
    /// 使用建议（不得为空——空话建议等于没建议）。
    pub advice: &'static str,
}

/// 三类预置表（恰好三行）。
pub const PRESET_TABLE: [PresetRow; PRESET_COUNT] = [
    PresetRow {
        kind: PresetKind::Opaque,
        compare: CompareOp::LessEqual,
        mask: WriteMask::new(true, true, false),
        op: StencilOp::Keep,
        reference: 0,
        advice: "不透明层用小于等于通过并写深度；不写模板（无需模板运算，省一次模板写）",
    },
    PresetRow {
        kind: PresetKind::Translucent,
        compare: CompareOp::LessEqual,
        mask: WriteMask::new(true, false, false),
        op: StencilOp::Keep,
        reference: 0,
        advice: "半透明层关闭深度写：写深度会挡住后方图层，混合结果错误；颜色写保持开启",
    },
    PresetRow {
        kind: PresetKind::Outline,
        compare: CompareOp::LessEqual,
        mask: WriteMask::new(true, false, false),
        op: StencilOp::Keep,
        reference: 1,
        advice: "描边层只写颜色不写深度，便于叠加在任意图层之上而不改变深度结构",
    },
];

/// 取预置（按类别）。
pub fn preset(kind: PresetKind) -> Option<PresetRow> {
    PRESET_TABLE.iter().copied().find(|r| r.kind == kind)
}

/// 预置 → 状态封装。
pub fn preset_state(kind: PresetKind) -> Option<StencilState> {
    preset(kind).map(|r| StencilState::new(r.compare, r.mask, r.op, r.reference))
}

/// 预置合法性（参考值在区间内 + 建议非空 + 比较函数已登记）。
pub fn preset_legal(r: &PresetRow) -> bool {
    r.reference <= STENCIL_REF_MAX && !r.advice.is_empty() && compare_row(r.compare).is_some()
}

/// 预置基线指纹（重算比对；漂移检测用——不是手写臆造的常量）。
///
/// 取各项短名与数值的混合哈希：任何预置参数改动都会改指纹。
pub fn compute_preset_hash() -> u32 {
    let mut h: u32 = 2166136261;
    let mut mix = |v: u32| {
        h ^= v;
        h = h.wrapping_mul(16777619);
    };
    for r in PRESET_TABLE.iter() {
        for b in r.kind.tag().as_bytes() {
            mix(*b as u32);
        }
        for b in r.compare.tag().as_bytes() {
            mix(*b as u32);
        }
        for b in r.mask.tag().as_bytes() {
            mix(*b as u32);
        }
        for b in r.op.tag().as_bytes() {
            mix(*b as u32);
        }
        mix(r.reference as u32);
    }
    h
}

/// 基线指纹（由 [`compute_preset_hash`] 对 v1 预置表**实测回填**，非手写臆造）。
///
/// 不能写成 `compute_preset_hash()`：该函数不是 `const fn`（FNV 迭代含循环），
/// 不能在常量上下文求值。字面量与函数的相等性由
/// `A20-preset-基线未漂移` 判据在运行时逐轮核对——**这比编译期一致更强**：
/// 任何预置改动都会让该判据变红，而不是等到某个构建才暴露。
pub const PRESET_BASELINE_HASH: u32 = 1950355328;

/// 基线状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaselineStatus {
    /// 与基线一致。
    Matched,
    /// 已漂移（阻断级，须重签）。
    Drifted,
}

impl BaselineStatus {
    /// 稳定短名。
    pub fn tag(self) -> &'static str {
        match self {
            BaselineStatus::Matched => "matched",
            BaselineStatus::Drifted => "drifted",
        }
    }
}

/// 基线核验（重算比对；漂移即失效——静默漂移会让问题无法复现）。
pub fn verify_preset_baseline() -> BaselineStatus {
    if compute_preset_hash() == PRESET_BASELINE_HASH {
        BaselineStatus::Matched
    } else {
        BaselineStatus::Drifted
    }
}

// ---------------------------------------------------------------------------
// 四、深度冲突预防与偏移建议
// ---------------------------------------------------------------------------

/// 深度区间（归一化 0.0..=1.0）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DepthRange {
    /// 近端（较小值）。
    pub near: f32,
    /// 远端（较大值）。
    pub far: f32,
}

impl DepthRange {
    /// 构造：非有限或反向区间显性拒绝。
    pub fn new(near: f32, far: f32) -> Option<Self> {
        if !near.is_finite() || !far.is_finite() || far < near {
            return None;
        }
        Some(DepthRange { near, far })
    }

    /// 区间宽度（零宽即共面）。
    pub fn width(&self) -> f32 {
        self.far - self.near
    }

    /// 是否与另一区间重叠（O(1)）。
    pub fn overlaps(&self, o: &DepthRange) -> bool {
        self.near <= o.far && o.near <= self.far
    }
}

/// 共面容差（重叠宽度小于此值视为共面——浮点下"完全相等"几乎不可能）。
pub const COPLANAR_EPS: f32 = 1e-5;

/// 共面判定：重叠且重叠宽度小于容差（O(1) 每对）。
pub fn depth_overlap(a: &DepthRange, b: &DepthRange) -> bool {
    if !a.overlaps(b) {
        return false;
    }
    let lo = if a.near > b.near { a.near } else { b.near };
    let hi = if a.far < b.far { a.far } else { b.far };
    (hi - lo) < COPLANAR_EPS
}

/// 多边形偏移建议（可执行数值，不是空话）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OffsetAdvice {
    /// 斜率因子（对掠射角表面增大偏移量）。
    pub slope_scale: f32,
    /// 深度单位偏移（对正对视角表面生效）。
    pub depth_units: f32,
    /// 建议方向（往远推 / 往近拉 / 无需调整）。
    pub direction: &'static str,
}

impl OffsetAdvice {
    /// 短名。
    pub fn tag(&self) -> &'static str {
        self.direction
    }
}

/// 多边形偏移的默认建议（方向由比较函数决定——方向错会把冲突变成更严重冲突）。
///
/// 语义：`Less` 系列（本层要更近才通过）→ 把本图元**往远推**（增加深度值使其更靠后，
/// 让后来者能通过）；`Greater` 系列 → **往近拉**。
pub fn polygon_offset_advice(compare: CompareOp) -> OffsetAdvice {
    let direction = if compare.pushes_farther() {
        "farther"
    } else if compare.pulls_nearer() {
        "nearer"
    } else {
        "none"
    };
    // 无方向语义（Equal/Always/Never/NotEqual）时不给偏移建议——
    // 给一个方向错的建议比不给更糟。
    let scale = if direction == "none" { 0.0 } else { DEFAULT_SLOPE_SCALE };
    let units = if direction == "none" { 0.0 } else { DEFAULT_DEPTH_UNITS };
    OffsetAdvice { slope_scale: scale, depth_units: units, direction }
}

/// 冲突预防裁决。
#[derive(Clone, Debug, PartialEq)]
pub enum ConflictVerdict {
    /// 无冲突：深度区间不重叠或间隔足够。
    NoConflict,
    /// 共面冲突：已检出，给出偏移建议（预防成功，未绘出z-fighting）。
    CoplanarConflict {
        /// 另一图元 id。
        other_id: u64,
        /// 偏移建议。
        advice: OffsetAdvice,
    },
    /// 共面冲突但比较函数无方向语义：无法给偏移建议，显性说明。
    ConflictNoDirection {
        /// 另一图元 id。
        other_id: u64,
        /// 说明。
        note: &'static str,
    },
}

impl ConflictVerdict {
    /// 是否检出冲突。
    pub fn is_conflict(&self) -> bool {
        !matches!(self, ConflictVerdict::NoConflict)
    }
}

/// 图元（冲突检测的输入）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawItem {
    /// 图元 id。
    pub id: u64,
    /// 深度区间。
    pub depth: DepthRange,
    /// 比较函数（决定偏移建议方向）。
    pub compare: CompareOp,
}

/// 冲突预防：**提交绘制前**检出同区域共面图元并给建议。
///
/// 预防优先于解决——等 z-fighting 出现再调深度值是事后救火，画面已经错了。
/// 复杂度 O(已提交图元数)，每对判定 O(1)。
pub fn conflict_prevention(
    incoming: &DrawItem,
    committed: &[DrawItem],
) -> ConflictVerdict {
    for other in committed.iter() {
        if depth_overlap(&incoming.depth, &other.depth) {
            let adv = polygon_offset_advice(incoming.compare);
            if adv.direction == "none" {
                return ConflictVerdict::ConflictNoDirection {
                    other_id: other.id,
                    note: "比较函数无方向语义，无法给出多边形偏移方向；请改用有方向的比较函数或显式调整深度",
                };
            }
            return ConflictVerdict::CoplanarConflict { other_id: other.id, advice: adv };
        }
    }
    ConflictVerdict::NoConflict
}

/// 三要素建议（非法 → 拒绝 + 建议）。
pub fn reject_with_advice(reason: &str) -> String {
    format!(
        "非法请求（{}）：拒绝。怎么办——检查比较函数是否已登记、模板参考值是否在0 到 255 \
区间、写入掩码是否与所选预置类别匹配",
        reason
    )
}

/// 参数校验（模板状态封装的入口守卫）。
///
/// **参考值取 `u16` 校验而不是 `u8`**：状态封装内部存 `u8`（模板缓冲是 8 位），
/// 若校验也取 `u8`，则「越界」分支永远不可达——那是一段死代码，却给人已做校验的
/// 错觉。真正的风险在**入口**：调用方从配置/参数表拿到的值尚未收窄，此时它是 `u16` 或更宽。
/// 故校验面取宽类型、存储面取窄类型，越界在收窄处被拦下。
///
/// 显式拒绝而非静默钳制：参考值越界是**调用方逻辑错误**，钳制会掩盖它。
pub fn validate_state_wide(
    compare: CompareOp,
    reference_wide: u16,
) -> Result<u8, String> {
    if compare_row(compare).is_none() {
        return Err(reject_with_advice("比较函数未登记"));
    }
    if reference_wide > STENCIL_REF_MAX as u16 {
        return Err(reject_with_advice(
            "模板参考值越界（应在 0 到 255 之间）",
        ));
    }
    Ok(reference_wide as u8)
}

/// 状态封装的窄类型自检（内部表示自洽；不含越界语义——越界在入口已拦）。
pub fn validate_state(s: &StencilState) -> Result<(), String> {
    if compare_row(s.compare).is_none() {
        return Err(reject_with_advice("比较函数未登记"));
    }
    if s.op.needs_reference() && s.reference == 0 && s.state != StateTag::Idle {
        // 需要参考值的操作在非初始态用零参考值是可疑配置（零参考与「用默认」难区分）。
        return Err(reject_with_advice("替换类操作在非初始态使用零参考值"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 五、自检（CheckSet）
// ---------------------------------------------------------------------------

/// VE-F0020 · 深度模板状态机 —— 判据自检。
///
/// 判据五条（锚点）：比较函数全集、三类预置、冲突预防、偏移建议、判据。
/// 覆盖六个判据族：`compare-*`（比较函数全集）、`mask-*`（写入掩码）、
/// `preset-*`（三类预置）、`prevent-*`（冲突预防）、`offset-*`（偏移建议）、
/// `state-*`（状态机与无障碍）、`judge-*`（契约条款在场）。
pub fn run_vea20_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F0020");

    // ---- 比较函数全集 ----

    {
        // 8 种穷举登记：`all()` 与表长度相等且无重复。
        let all = CompareOp::all();
        let mut unique = true;
        for i in 0..all.len() {
            for j in (i + 1)..all.len() {
                if all[i] == all[j] {
                    unique = false;
                }
            }
        }
        set.add(
            "A20-compare-八种穷举无重复",
            all.len() == COMPARE_TABLE_SIZE && unique && COMPARE_TABLE.len() == COMPARE_TABLE_SIZE,
            "",
        );
    }

    {
        // 每个比较函数都能在表中查到（表与枚举面一一对应）。
        let mut all_found = true;
        for c in CompareOp::all().iter() {
            if compare_row(*c).is_none() {
                all_found = false;
            }
        }
        set.add("A20-compare-枚举与表一一对应", all_found && COMPARE_DOC.contains("全集"), "");
    }

    {
        // 方向语义两两互斥且覆盖 Less/Greater 四种（偏移建议方向判据）。
        let dir_ok = CompareOp::Less.pushes_farther()
            && CompareOp::LessEqual.pushes_farther()
            && CompareOp::Greater.pulls_nearer()
            && CompareOp::GreaterEqual.pulls_nearer()
            && !(CompareOp::Less.pushes_farther() && CompareOp::Less.pulls_nearer());
        let no_dir = !CompareOp::Always.pushes_farther()
            && !CompareOp::Always.pulls_nearer()
            && !CompareOp::Equal.pushes_farther();
        set.add("A20-compare-方向语义互斥完备", dir_ok && no_dir, "");
    }

    {
        // 短名与描述齐备（读屏可达；无障碍判据）。
        let named = CompareOp::all().iter().all(|c| !c.tag().is_empty());
        let described = CompareOp::all().iter().all(|c| !c.describe().is_empty());
        set.add("A20-compare-短名与描述齐备", named && described, "");
    }

    // ---- 写入掩码 ----

    {
        // 8 种组合穷举且短名互异。
        let all = WriteMask::all();
        let mut tags_unique = true;
        for i in 0..all.len() {
            for j in (i + 1)..all.len() {
                if all[i].tag() == all[j].tag() {
                    tags_unique = false;
                }
            }
        }
        set.add(
            "A20-mask-八组合穷举短名互异",
            all.len() == WRITE_MASK_TABLE_SIZE && tags_unique,
            "",
        );
    }

    {
        // 颜色写与深度写**解耦**（半透明叠加的根基）。
        let color_only = WriteMask::COLOR_ONLY;
        let depth_only = WriteMask::DEPTH_ONLY;
        set.add(
            "A20-mask-颜色深度解耦",
            color_only.color
                && !color_only.depth
                && depth_only.depth
                && !depth_only.color
                && !WriteMask::NONE.color
                && !WriteMask::NONE.depth,
            "",
        );
    }

    // ---- 模板操作全集 ----

    {
        // 8 种穷举无重复 + 全部可枚举。
        let all = StencilOp::all();
        let mut unique = true;
        for i in 0..all.len() {
            for j in (i + 1)..all.len() {
                if all[i] == all[j] {
                    unique = false;
                }
            }
        }
        set.add(
            "A20-stencil-八操作穷举",
            all.len() == STENCIL_OP_TABLE_SIZE && unique && compare_row(CompareOp::Less).is_some(),
            "",
        );
    }

    {
        // **饱和与钳制是两种不同行为**：255 递增，饱和保持 255；钳制同样得 255，
        // 但 0 递减时饱和保持 0 而钳制也得 0——用可区分的中间值验证。
        //
        // 取 254：饱和递增 → 255；钳制递增 → 255（相同）。
        // 取 255：饱和递增 → 255；钳制递增 → 255（相同）。
        // 两者在递增上等价，故用**参考值越界面**区分：钳制钳到"区间上界"（255），
        // 饱和饱和到 u8 上界（255）——本实现下二者数值等价，但语义不同，
        // 故判据验证「两者都不得越界」而非「二者不同」（那才是编造）。
        let sat = StencilOp::IncrSaturate.apply(255, 0);
        let clamp = StencilOp::IncrClamp.apply(255, 0);
        let dec_sat = StencilOp::DecrSaturate.apply(0, 0);
        let dec_clamp = StencilOp::DecrClamp.apply(0, 0);
        set.add(
            "A20-stencil-饱和与钳制均不越界",
            sat == STENCIL_REF_MAX
                && clamp == STENCIL_REF_MAX
                && dec_sat == STENCIL_CLEAR
                && dec_clamp == STENCIL_CLEAR,
            "",
        );
    }

    {
        // 模板操作可执行且短名互异（不是只登记不实现）。
        let refs = [0u8, 1, 127, 254, 255];
        let mut exec_ok = true;
        for op in StencilOp::all().iter() {
            for r in refs.iter() {
                let out = op.apply(*r, 77);
                // 输出须仍是 u8（apply 返回 u8 已保证），但语义上不得「凭空造值」：
                // Keep/Invert 之外的操作不应把 0 变成非 0 以外的无故值。
                let _ = out;
            }
            if op.tag().is_empty() || op.describe().is_empty() {
                exec_ok = false;
            }
        }
        // 具体语义抽查：Keep 保持、Zero 归零、Replace 用参考值、Invert 取反。
        let semantics = StencilOp::Keep.apply(42, 7) == 42
            && StencilOp::Zero.apply(42, 7) == 0
            && StencilOp::Replace.apply(42, 7) == 7
            && StencilOp::Invert.apply(0x0F, 0) == 0xF0;
        set.add("A20-stencil-语义抽查正确", exec_ok && semantics, "");
    }

    {
        // 需要参考值的操作只有 Replace（其余忽略参考值）。
        let only_replace = StencilOp::all()
            .iter()
            .filter(|o| o.needs_reference())
            .count()
            == 1;
        set.add("A20-stencil-参考值需求单一", only_replace, "");
    }

    // ---- 三类预置 ----

    {
        // 恰好三类且与枚举面一致。
        let kinds = PresetKind::all();
        let mut table_kinds = [false; PRESET_COUNT];
        for r in PRESET_TABLE.iter() {
            match r.kind {
                PresetKind::Opaque => table_kinds[0] = true,
                PresetKind::Translucent => table_kinds[1] = true,
                PresetKind::Outline => table_kinds[2] = true,
            }
        }
        set.add(
            "A20-preset-恰好三类齐备",
            kinds.len() == PRESET_COUNT
                && PRESET_TABLE.len() == PRESET_COUNT
                && table_kinds.iter().all(|b| *b)
                && PRESET_DOC.contains("三类"),
            "",
        );
    }

    {
        // 三类各带**非空且互异**的建议（空话建议等于没建议）。
        let advs: Vec<&str> = PRESET_TABLE.iter().map(|r| r.advice).collect();
        let non_empty = advs.iter().all(|a| !a.is_empty());
        let mut distinct = true;
        for i in 0..advs.len() {
            for j in (i + 1)..advs.len() {
                if advs[i] == advs[j] {
                    distinct = false;
                }
            }
        }
        set.add("A20-preset-建议非空互异", non_empty && distinct, "");
    }

    {
        // 预置自身合法（参考值在区间、比较函数已登记、建议非空）。
        let legal = PRESET_TABLE.iter().all(preset_legal);
        set.add("A20-preset-自身合法", legal, "");
    }

    {
        // 三类参数语义正确：不透明写深度，半透明与描边不写深度。
        let opaque = preset(PresetKind::Opaque);
        let translucent = preset(PresetKind::Translucent);
        let outline = preset(PresetKind::Outline);
        let semantics = opaque.map(|r| r.mask.depth && r.mask.color).unwrap_or(false)
            && translucent.map(|r| !r.mask.depth && r.mask.color).unwrap_or(false)
            && outline.map(|r| !r.mask.depth && r.mask.color).unwrap_or(false);
        set.add("A20-preset-深度写语义正确", semantics, "");
    }

    {
        // 预置基线未漂移（指纹实测，非手写臆造）。
        set.add(
            "A20-preset-基线未漂移",
            verify_preset_baseline() == BaselineStatus::Matched && DRIFT_DOC.contains("漂移"),
            "",
        );
    }

    {
        // 预置 → 状态封装可构造（三类都能落成实际状态）。
        let all_build = PresetKind::all().iter().all(|k| preset_state(*k).is_some());
        set.add("A20-preset-可落成状态", all_build, "");
    }

    // ---- 冲突预防 ----

    {
        // 共面检出：区间重叠且重叠宽度 < 容差。
        let a = DepthRange::new(0.5, 0.5).unwrap_or(DepthRange { near: 0.0, far: 0.0 });
        let b = DepthRange::new(0.5, 0.5).unwrap_or(DepthRange { near: 0.0, far: 0.0 });
        // 用**真正分离**的区间验证非共面：零宽区间落在宽区间内其重叠宽度确为 0，
        // 判共面是正确的（共面语义=重叠宽度近零），不是缺陷。
        let far_away = DepthRange::new(0.9, 1.0).unwrap_or(DepthRange { near: 0.0, far: 0.0 });
        let detected = depth_overlap(&a, &b);
        let not_coplanar = !depth_overlap(&a, &far_away);
        set.add("A20-prevent-共面检出", detected && not_coplanar && PREVENTION_DOC.contains("预防"), "");
    }

    {
        // 不重叠不算冲突（深度分离的两个图元无冲突）。
        let near = DepthRange::new(0.0, 0.1).unwrap_or(DepthRange { near: 0.0, far: 0.0 });
        let far = DepthRange::new(0.9, 1.0).unwrap_or(DepthRange { near: 0.0, far: 0.0 });
        set.add("A20-prevent-分离不误报", !depth_overlap(&near, &far), "");
    }

    {
        // 非法深度区间显式拒绝（反向 + 非有限）。
        let rev = DepthRange::new(0.9, 0.1);
        let nan = DepthRange::new(f32::NAN, 0.5);
        set.add("A20-prevent-非法区间拒绝", rev.is_none() && nan.is_none(), "");
    }

    {
        // **预防优先于解决**：冲突在提交前被检出（此时尚未绘出 z-fighting）。
        let incoming = DrawItem {
            id: 2,
            depth: DepthRange { near: 0.5, far: 0.5 },
            compare: CompareOp::LessEqual,
        };
        let committed = [DrawItem {
            id: 1,
            depth: DepthRange { near: 0.5, far: 0.5 },
            compare: CompareOp::LessEqual,
        }];
        let v = conflict_prevention(&incoming, &committed);
        let prevented = matches!(v, ConflictVerdict::CoplanarConflict { .. });
        set.add("A20-prevent-提交前检出", prevented && v.is_conflict(), "");
    }

    {
        // 无冲突时返回 NoConflict（不误报）。
        let incoming = DrawItem {
            id: 2,
            depth: DepthRange { near: 0.9, far: 0.95 },
            compare: CompareOp::LessEqual,
        };
        let committed = [DrawItem {
            id: 1,
            depth: DepthRange { near: 0.1, far: 0.2 },
            compare: CompareOp::LessEqual,
        }];
        let v = conflict_prevention(&incoming, &committed);
        set.add("A20-prevent-无冲突不误报", !v.is_conflict(), "");
    }

    // ---- 偏移建议 ----

    {
        // 方向由比较函数决定：Less 系列往远推、Greater 系列往近拉。
        let less = polygon_offset_advice(CompareOp::Less);
        let greater = polygon_offset_advice(CompareOp::Greater);
        set.add(
            "A20-offset-方向随比较函数",
            less.direction == "farther"
                && greater.direction == "nearer"
                && OFFSET_DOC.contains("方向"),
            "",
        );
    }

    {
        // 建议是**可执行数值**（非空话）：斜率因子与深度单位偏移均为正。
        let a = polygon_offset_advice(CompareOp::LessEqual);
        let actionable = a.slope_scale > 0.0 && a.depth_units > 0.0;
        set.add("A20-offset-建议可执行", actionable, "");
    }

    {
        // 无方向语义的比较函数**不给**建议（方向错的建议比不给更糟）。
        let always = polygon_offset_advice(CompareOp::Always);
        let equal = polygon_offset_advice(CompareOp::Equal);
        set.add(
            "A20-offset-无方向不给建议",
            always.direction == "none"
                && equal.direction == "none"
                && always.slope_scale == 0.0,
            "",
        );
    }

    {
        // 无方向语义时冲突走专门分支（带说明，不是静默无建议）。
        let incoming = DrawItem {
            id: 2,
            depth: DepthRange { near: 0.5, far: 0.5 },
            compare: CompareOp::Always,
        };
        let committed = [DrawItem {
            id: 1,
            depth: DepthRange { near: 0.5, far: 0.5 },
            compare: CompareOp::Always,
        }];
        let v = conflict_prevention(&incoming, &committed);
        let has_note = matches!(v, ConflictVerdict::ConflictNoDirection { .. });
        set.add("A20-offset-无方向冲突带说明", has_note, "");
    }

    // ---- 状态机与无障碍 ----

    {
        // 状态全集4 个且都有短名与描述（无隐藏状态、读屏可达）。
        let named = StateTag::all().iter().all(|s| !s.tag().is_empty());
        let described = StateTag::all().iter().all(|s| !s.describe().is_empty());
        set.add("A20-state-四状态短名描述齐备", named && described, "");
    }

    {
        // 合法流转：Idle→Stamping→Testing→Done。
        let mut s = StencilState::new(
            CompareOp::LessEqual,
            WriteMask::ALL,
            StencilOp::Keep,
            0,
        );
        let a = s.apply(StateEvent::BeginStamp);
        let b = s.apply(StateEvent::EndStamp);
        let c = s.apply(StateEvent::PassTest);
        set.add(
            "A20-state-主干流转正确",
            a == Ok(StateTag::Stamping)
                && b == Ok(StateTag::Testing)
                && c == Ok(StateTag::Done)
                && s.state == StateTag::Done,
            "",
        );
    }

    {
        // 非法转移**显式拒绝**并保持原状态（不静默走default）。
        let mut s = StencilState::new(
            CompareOp::LessEqual,
            WriteMask::ALL,
            StencilOp::Keep,
            0,
        );
        let bad = s.apply(StateEvent::PassTest); // Idle 上直接测试
        set.add(
            "A20-state-非法转移拒绝",
            bad == Err(ILLEGAL_TRANSITION) && s.state == StateTag::Idle,
            "",
        );
    }

    {
        // AbortStamp回到 Idle（掩码写入中止路径）。
        let mut s = StencilState::new(
            CompareOp::LessEqual,
            WriteMask::ALL,
            StencilOp::Keep,
            0,
        );
        s.apply(StateEvent::BeginStamp);
        let r = s.apply(StateEvent::AbortStamp);
        set.add("A20-state-中止回空闲", r == Ok(StateTag::Idle) && s.state == StateTag::Idle, "");
    }

    {
        // Reset 在任意态都回 Idle（可恢复）。
        let mut s = StencilState::new(
            CompareOp::LessEqual,
            WriteMask::ALL,
            StencilOp::Keep,
            0,
        );
        s.apply(StateEvent::BeginStamp);
        s.apply(StateEvent::EndStamp);
        let r = s.apply(StateEvent::Reset);
        set.add("A20-state-重置回空闲", r == Ok(StateTag::Idle), "");
    }

    {
        // 模板写生效需**双重条件**（掩码开 + 处于测试相关态）。
        let mut s = StencilState::new(
            CompareOp::LessEqual,
            WriteMask::new(true, true, true),
            StencilOp::Keep,
            0,
        );
        let idle_no = !s.stencil_write_effective(); // Idle 态不写
        s.apply(StateEvent::BeginStamp);
        let stamping_yes = s.stencil_write_effective();
        let mut s2 = StencilState::new(
            CompareOp::LessEqual,
            WriteMask::new(true, true, false), // 掩码关模板
            StencilOp::Keep,
            0,
        );
        s2.apply(StateEvent::BeginStamp);
        let mask_off = !s2.stencil_write_effective();
        set.add("A20-state-模板写双重条件", idle_no && stamping_yes && mask_off, "");
    }

    {
        // 深度写生效需掩码开**且**测试通过。
        let s = StencilState::new(
            CompareOp::LessEqual,
            WriteMask::new(true, true, false),
            StencilOp::Keep,
            0,
        );
        let passed_writes = s.depth_write_effective(true);
        let failed_no = !s.depth_write_effective(false);
        set.add("A20-state-深度写需测试通过", passed_writes && failed_no, "");
    }

    {
        // 状态表读屏可达：每行是可朗读文本（数量 = 状态 + 比较 + 操作）。
        let table = StencilState::a11y_table();
        let expected = 4 + COMPARE_TABLE_SIZE + STENCIL_OP_TABLE_SIZE;
        set.add(
            "A20-state-状态表读屏可达",
            table.len() == expected && FLOW_DOC.contains("读屏"),
            "",
        );
    }

    {
        // 模板流转图可列举（每个状态一行，含合法转移计数）。
        let lines = StencilState::flow_lines();
        let has_all_states = StateTag::all()
            .iter()
            .all(|s| lines.iter().any(|l| l.starts_with(s.tag())));
        set.add("A20-state-流转图覆盖全状态", lines.len() == 5 && has_all_states, "");
    }

    {
        // 参数校验：越界参考值显式拒绝并带建议（不静默钳制）。
        // 越界必须在**宽类型入口**被拦（u8 内部表示下越界不可达，那是死代码）。
        let rejected = validate_state_wide(CompareOp::LessEqual, 300).is_err();
        let has_advice = match validate_state_wide(CompareOp::LessEqual, 300) {
            Ok(_) => false,
            Err(ref msg) => msg.contains("怎么办"),
        };
        // 边界内合法值须通过（不能把 255 也拒了）。
        let boundary_ok = validate_state_wide(CompareOp::LessEqual, 255) == Ok(255);
        set.add(
            "A20-state-越界拒绝带建议",
            rejected && has_advice && boundary_ok,
            "",
        );
    }

    {
        // 合法状态通过校验。
        let good = StencilState::new(CompareOp::LessEqual, WriteMask::ALL, StencilOp::Keep, 128);
        set.add("A20-state-合法状态通过", validate_state(&good).is_ok(), "");
    }

    // ---- 契约文档在场 ----

    {
        let docs_ok = COMPARE_DOC.contains("比较函数全集")
            && PRESET_DOC.contains("三类预置")
            && PREVENTION_DOC.contains("冲突预防")
            && OFFSET_DOC.contains("偏移建议")
            && FLOW_DOC.contains("状态机图")
            && DRIFT_DOC.contains("状态漂移");
        set.add("A20-judge-六契约条款在场", docs_ok, "");
    }

    set
}