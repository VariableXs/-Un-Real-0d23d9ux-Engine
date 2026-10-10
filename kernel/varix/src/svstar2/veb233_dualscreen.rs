//! VE-F0233 · Intel 双屏输出配置（VE-B 域 · Intel 核显组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0233`
//!
//! **职责定位（锚点原文）**：Intel 双屏输出配置——**镜像与扩展模式实现**（镜像
//! 共享 pipe 时序、扩展独立 pipe 分配）、**克隆约束检测**（时序不一致时拒绝
//! 镜像并说明原因）、**用户布局持久化与恢复**（上次布局失效回落默认并告知）。
//!
//! **数据结构（锚点原文）**：输出配置结构（模式×pipe 分配×布局）；持久化记录；
//! 约束校验结果。三者在 [`DualScreenCtl`] 里一一落位：配置结构 =
//! [`AppliedLayout`]、持久化记录 = [`PersistRecord`]、约束校验结果 =
//! [`CloneVerdict`] + [`RejectReason`]。
//!
//! **判据（锚点原文）**：镜像扩展、约束检测、布局持久化、降级通知。
//!
//! 本模块 32 条判据逐条映射：**镜像扩展 4 条**（共享 pipe 时序只编程一次／
//! 扩展独立 pipe 规范序分配／镜像坐标归一并记账／单屏在线退化为单屏）、
//! **约束检测 6 条**（逐位相等判可镜像／首个差异字段六字段对拍／不一致拒绝
//! 并三要素／拒绝不动现状／非法时序拒绝／先报未接入而非他因）、
//! **布局持久化 6 条**（存取往返逐字段一致／头闸三道篡改即拒／有效记录
//! 直读／端口拔出回落默认／恢复走同一约束管道／恢复计数对账）、
//! **降级通知 6 条**（pipe 不足退单屏／下游请求不虚发／镜像资源够不降级／
//! 七原因三要素齐／三要素两两互异／降级与拒绝可判定）、
//! **读屏与判据自身 10 条**（布局命名含位置语义／最近通知三要素进数组／
//! 码位互异／码段独占／未知码兜底／原因码与诊断码一一对应／下游取走即清／
//! 热插拔联动触发重估／镜像省下的编程次数可对账／条数对账）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 镜像约束不满足 → **拒绝并三要素说明** | [`DualScreenCtl::apply`] 第一步就过 [`DualScreenCtl::clone_check`]：两输出齐全时逐位比 [`TimingSig`] 六字段，任一不等即 [`RejectReason::CloneTimingMismatch`] 拒绝，**不落任何 placement**（拒绝 = 屏幕保持上一份已生效布局，不半落地），并由 [`notice_for`] 出三要素通知——「什么坏了/影响什么/下一步做什么」 |
//! | pipe 资源不足 → **降级单屏并通知** | 扩展模式需 `活跃输出数` 根 pipe；`probe_pipes` 给不够即**降级单屏**：只保留第一个活跃输出（管��� 0），第二个 placement 显式 `None`（**不假装两屏都在**），`degraded=true` + [`RejectReason::PipeExhausted`] 三要素通知。镜像模式只需 1 根 pipe，**因此不降级**——资源不足降级的是「扩展」，不是「显示」 |
//! | 恢复布局失效 → **回落默认布局** | [`DualScreenCtl::restore`] 头闸（魔数/版本/校验和）+ 体校验逐项过，任一不过即回落默认（扩展、主屏原点、副屏按主屏宽度右移）并告知**为何失效**（[`RestoreOutcome::reason`]）——回落不是一个静默的"恢复失败"，而是一次带原因码的显式决定 |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **镜像 = 共享 pipe 时序，只编程一次**：镜像下两个输出口挂在同一根 pipe、
//!   同一份时序上——编程次数是 **1**，不是「输出数」（锚点原文「镜像共享 pipe
//!   时序」）。判据用 [`ApplyOutcome::timing_programs`] 独立对拍：镜像恒 1、
//!   扩展恒等于活跃输出数。这条差异不是优化洁癖——镜像多编程一次就多一次
//!   模式切换的中间态窗口，正是花屏的来源。
//! - **扩展 = 独立 pipe，分配序必须确定**：pipe 按**端口升序**分配到**升序
//!   最低可用号**。判据侧不依赖任何哈希/迭代序，`(p0→pipe0, p1→pipe1)` 是
//!   规范序而不是巧合——两条同样的配置换个遍历顺序就换一个 pipe 号，是最难
//!   查的一类显示 bug。
//! - **拒绝「首个差异字段」而不是「全部差异」**：时序不一致时按**规范字段序**
//!   （时钟→link rate→lanes→hdisplay→vdisplay→vrefresh）返回**第一个**不一致
//!   的字段。用户看到的是"分辨率不同"而不是一条六字段清单；判据也能对拍到
//!   具体字段而不是"不等"这个布尔。
//! - **判定规则不重写，真调 F0225**：时序合法性与带宽账**不复制一份**，而是
//!   把 [`TimingSig`] 装配成 [`veb25_display::TimingParams`] 后**真调**
//!   [`TimingParams::value_legal`] 与 [`TimingParams::bandwidth_ok`]——复制
//!   规则会出现「上游改了表这里没改」的静默分叉。
//! - **恢复必须走同一条 apply 管道**：恢复出的布局**不另开一条捷径**应用，
//!   而是回到 [`DualScreenCtl::apply`] 重过全部约束。持久化记录是可信输入吗？
//!   不是——它可能记着三个月前两块 1080p 屏的镜像，今天副屏换成 4K 了。走同
//!   一条管道，恢复与用户当场手选走的是同一套约束，行为可预期。
//! - **降级必须留痕**：`stats.degraded_single` 计数 + [`CtlStats::last_notice`]
//!   + 第二个输出的 `placement=None`。三样齐了，"为什么我只有一屏"才有答案。
//! - **零 panic 面**：查表走 `get`/match、计数全 `saturating_add`、持久化头闸
//!   先于体解析（篡改的记录不会把 CPU 拖进无意义的解析）。

// ---------------------------------------------------------------------------
// 引入（no_std 内核侧）
// ---------------------------------------------------------------------------

use alloc::format;
use alloc::string::String;

use super::veb21_ident::GenTier;
use super::veb25_display::{DispCaps, TimingParams};

// ---------------------------------------------------------------------------
// 一、诊断码（自建段 0x64xx，独占——0x61/0x62/0x63/0x67/0x6B/0x6C 已占，
//    全仓 grep 0x64xx 零占用后选定）
// ---------------------------------------------------------------------------

/// 时序值非法（沿用 F0225 口径：档位表/带宽账由 F0225 真调裁决）。
pub const CODE_TIMING_ILLEGAL: u16 = 0x6401;
/// 镜像约束不满足（两输出时序不一致）。
pub const CODE_CLONE_MISMATCH: u16 = 0x6402;
/// pipe 资源不足（已降级单屏并通知）。
pub const CODE_PIPE_EXHAUSTED: u16 = 0x6403;
/// 持久化记录不合法（魔数/版本/校验和任一不符或体越界）。
pub const CODE_LAYOUT_CORRUPT: u16 = 0x6404;
/// 恢复布局失效已回落默认并告知。
pub const CODE_RESTORE_FALLBACK: u16 = 0x6405;
/// 非法请求（端口号越界/模式枚举越界/无活跃输出）。
pub const CODE_BAD_REQUEST: u16 = 0x6406;
/// 输出未接入（F0232 报断开，不参与分配）。
pub const CODE_OUTPUT_ABSENT: u16 = 0x6407;

/// 本域诊断码全集（判据对账：互异 + 独占 0x64 段）。
pub const CODES: [u16; 7] = [
    CODE_TIMING_ILLEGAL,
    CODE_CLONE_MISMATCH,
    CODE_PIPE_EXHAUSTED,
    CODE_LAYOUT_CORRUPT,
    CODE_RESTORE_FALLBACK,
    CODE_BAD_REQUEST,
    CODE_OUTPUT_ABSENT,
];

/// 人话说明（后果 + 下一步；未知码有兜底不 panic）。
pub const fn explain(code: u16) -> &'static str {
    match code {
        CODE_TIMING_ILLEGAL => "输出时序不合法（档位或带宽不满足）：检查分辨率/刷新率/lane 数后重选模式",
        CODE_CLONE_MISMATCH => "镜像约束不满足（两屏时序不一致）：改用扩展模式，或把两屏设为同一分辨率与刷新率",
        CODE_PIPE_EXHAUSTED => "pipe 资源不足：已降级为单屏显示，副屏未点亮；减少输出数或等驱动释放管线",
        CODE_LAYOUT_CORRUPT => "布局记录已损坏（校验未过）：已忽略该记录，请重新设置一次布局",
        CODE_RESTORE_FALLBACK => "上次布局已失效：已回落默认布局（主屏原点 + 副屏右侧），请重新摆放",
        CODE_BAD_REQUEST => "非法请求（端口号或模式越界、或无任何在线输出）：核对调用方参数后重试",
        CODE_OUTPUT_ABSENT => "该输出当前未接入：按 F0232 报告的连接态处理，接上后自动恢复",
        _ => "未知 Intel 双屏配置诊断码（未登记）",
    }
}

// ---------------------------------------------------------------------------
// 二、拒绝原因（约束校验结果：原因 + 三要素）
// ---------------------------------------------------------------------------

/// 拒绝/降级原因（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RejectReason {
    /// 时序值非法（档位表外或带宽账不匹配）。
    TimingIllegal,
    /// 镜像约束不满足：两输出时序不一致。
    CloneMismatch,
    /// pipe 资源不足（已降级单屏）。
    PipeExhausted,
    /// 持久化记录不合法。
    LayoutCorrupt,
    /// 恢复布局失效（已回落默认）。
    RestoreFallback,
    /// 非法请求。
    BadRequest,
    /// 输出未接入。
    OutputAbsent,
}

impl RejectReason {
    /// 诊断码。
    pub const fn code(self) -> u16 {
        match self {
            RejectReason::TimingIllegal => CODE_TIMING_ILLEGAL,
            RejectReason::CloneMismatch => CODE_CLONE_MISMATCH,
            RejectReason::PipeExhausted => CODE_PIPE_EXHAUSTED,
            RejectReason::LayoutCorrupt => CODE_LAYOUT_CORRUPT,
            RejectReason::RestoreFallback => CODE_RESTORE_FALLBACK,
            RejectReason::BadRequest => CODE_BAD_REQUEST,
            RejectReason::OutputAbsent => CODE_OUTPUT_ABSENT,
        }
    }

    /// 在册全集（判据对账基准：七项齐全）。
    pub const ALL: [RejectReason; 7] = [
        RejectReason::TimingIllegal,
        RejectReason::CloneMismatch,
        RejectReason::PipeExhausted,
        RejectReason::LayoutCorrupt,
        RejectReason::RestoreFallback,
        RejectReason::BadRequest,
        RejectReason::OutputAbsent,
    ];

    /// 标签（读屏与日志用人话）。
    pub const fn label(self) -> &'static str {
        match self {
            RejectReason::TimingIllegal => "时序不合法 / timing illegal",
            RejectReason::CloneMismatch => "镜像约束不满足 / clone constraint unmet",
            RejectReason::PipeExhausted => "pipe 资源不足已降级单屏 / pipes exhausted, degraded",
            RejectReason::LayoutCorrupt => "布局记录损坏 / layout record corrupt",
            RejectReason::RestoreFallback => "上次布局失效已回落默认 / restore fell back to default",
            RejectReason::BadRequest => "非法请求 / bad request",
            RejectReason::OutputAbsent => "输出未接入 / output absent",
        }
    }

    /// 是否**已降级照常继续**（区别于**拒绝**）：
    /// pipe 不足是降级（有屏可看），时序非法是拒绝（不动现状）。
    pub const fn is_degraded(self) -> bool {
        matches!(
            self,
            RejectReason::PipeExhausted | RejectReason::RestoreFallback
        )
    }
}

// ---------------------------------------------------------------------------
// 三、三要素通知（锚点无障碍：配置失败通知键盘可达可关闭）
// ---------------------------------------------------------------------------

/// 三要素通知：什么坏了 / 影响什么 / 建议做什么。
///
/// 锚点要求「配置失败通知键盘可达可关闭」——因此 [`Notice::keyboard_reachable`]
/// 与 [`Notice::dismissible`] 是**显式字段而不是注释承诺**：判据逐条核对每个
/// 降级/拒绝路径的通知都满足「三要素非空 + 键盘可达 + 可关闭」。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Notice {
    /// 什么坏了。
    pub what: &'static str,
    /// 影响什么。
    pub impact: &'static str,
    /// 建议做什么。
    pub action: &'static str,
    /// 是否键盘可达（焦点可达，不只靠鼠标）。
    pub keyboard_reachable: bool,
    /// 是否可关闭（不阻塞，不逼用户点掉才继续）。
    pub dismissible: bool,
}

impl Notice {
    /// 三要素齐且键盘可达可关闭才算**有效通知**。
    pub const fn complete(&self) -> bool {
        !self.what.is_empty()
            && !self.impact.is_empty()
            && !self.action.is_empty()
            && self.keyboard_reachable
            && self.dismissible
    }
}

/// 按原因出三要素通知（键盘可达 + 可关闭恒真——锚点硬要求，不是可选项）。
pub const fn notice_for(r: RejectReason) -> Notice {
    let (what, impact, action) = match r {
        RejectReason::TimingIllegal => (
            "输出的时序参数不合法",
            "该输出保持原有布局不变，本次设置未生效",
            "换一个分辨率或刷新率后重试",
        ),
        RejectReason::CloneMismatch => (
            "两屏时序不一致，无法镜像",
            "两块屏幕继续按各自布局显示，镜像未开启",
            "改用扩展模式，或把两屏设为同一分辨率与刷新率",
        ),
        RejectReason::PipeExhausted => (
            "可用的显示管线不足，无法同时点亮两屏",
            "已切换为单屏显示，副屏当前未点亮",
            "减少输出数，或等待驱动释放管线后重试",
        ),
        RejectReason::LayoutCorrupt => (
            "保存的布局记录校验未通过",
            "本次不套用旧布局，沿用当前布局",
            "重新设置一次布局即可重新记录",
        ),
        RejectReason::RestoreFallback => (
            "上次保存的布局已不适用于当前硬件",
            "已回落默认布局（主屏在左，副屏在右）",
            "如需恢复，请按当前屏幕重新摆放一次",
        ),
        RejectReason::BadRequest => (
            "配置请求的参数不合法",
            "配置未生效，屏幕维持现状",
            "核对输出端口与模式参数后重试",
        ),
        RejectReason::OutputAbsent => (
            "目标输出当前未接入",
            "该输出未参与本次布局",
            "接好屏幕后会自动重新参与",
        ),
    };
    Notice { what, impact, action, keyboard_reachable: true, dismissible: true }
}

// ---------------------------------------------------------------------------
// 四、时序签名与差异定位（克隆约束检测的数据底座）
// ---------------------------------------------------------------------------

/// 时序签名（克隆一致性的**唯一口径**：六字段逐位相等才算可镜像）。
///
/// 与 F0225 的 [`TimingParams`] 是同一组物理量的**判定用投影**：本类型负责
/// 「两屏是否同模」的比较，合法性判定则交给 F0225 真调。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimingSig {
    /// 模式时钟（kHz）。
    pub mode_clock_khz: u32,
    /// link rate（MHz）。
    pub link_mhz: u32,
    /// lane 数（1/2/4）。
    pub lanes: u8,
    /// 水平有效像素。
    pub hdisplay: u16,
    /// 垂直有效行。
    pub vdisplay: u16,
    /// 刷新率（Hz）。
    pub vrefresh: u8,
}

/// 时序字段（规范序——首个差异定位的基准序，不随枚举声明顺序漂移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimingField {
    /// 模式时钟。
    ModeClock,
    /// link rate。
    LinkRate,
    /// lane 数。
    Lanes,
    /// 水平有效像素。
    HDisplay,
    /// 垂直有效行。
    VDisplay,
    /// 刷新率。
    VRefresh,
}

/// 字段在册序（恰六项，升序；判据对账基准）。
pub const TIMING_FIELDS: [TimingField; 6] = [
    TimingField::ModeClock,
    TimingField::LinkRate,
    TimingField::Lanes,
    TimingField::HDisplay,
    TimingField::VDisplay,
    TimingField::VRefresh,
];

impl TimingField {
    /// 字段名（读屏与日志用人话，说人话不甩枚举）。
    pub const fn label(self) -> &'static str {
        match self {
            TimingField::ModeClock => "模式时钟",
            TimingField::LinkRate => "链路速率",
            TimingField::Lanes => "通道数",
            TimingField::HDisplay => "水平分辨率",
            TimingField::VDisplay => "垂直分辨率",
            TimingField::VRefresh => "刷新率",
        }
    }

    /// 序号（规范序）。
    pub const fn ordinal(self) -> usize {
        match self {
            TimingField::ModeClock => 0,
            TimingField::LinkRate => 1,
            TimingField::Lanes => 2,
            TimingField::HDisplay => 3,
            TimingField::VDisplay => 4,
            TimingField::VRefresh => 5,
        }
    }
}

impl TimingSig {
    /// 装配成 F0225 的时序参数（**真调对接**：合法性规则不在本模块复制一份）。
    pub const fn to_params(&self) -> TimingParams {
        TimingParams {
            mode_clock_khz: self.mode_clock_khz,
            link_mhz: self.link_mhz,
            lanes: self.lanes,
            hdisplay: self.hdisplay,
            vdisplay: self.vdisplay,
            vrefresh: self.vrefresh,
        }
    }

    /// 时序合法性（真调 F0225 的值合法性 + 带宽账，不自造判据）。
    pub fn legal(&self) -> bool {
        let p = self.to_params();
        p.value_legal() && p.bandwidth_ok()
    }

    /// 取规范序第 `i` 个字段值（越界 `None`——判据对账走这一条，不直接摸字段）。
    pub const fn field(&self, i: usize) -> Option<u64> {
        match i {
            0 => Some(self.mode_clock_khz as u64),
            1 => Some(self.link_mhz as u64),
            2 => Some(self.lanes as u64),
            3 => Some(self.hdisplay as u64),
            4 => Some(self.vdisplay as u64),
            5 => Some(self.vrefresh as u64),
            _ => None,
        }
    }

    /// **首个**差异字段（规范序；无差异 `None`）。
    ///
    /// 只报第一个——用户要的是"分辨率不同"，不是六字段清单；判据要的是能指到
    /// 具体字段，而不是一个 `a != b` 的布尔。
    ///
    /// 不是 `const fn`：逐字段比较用 `Option<u64>` 的 `!=`，而 `PartialEq`
    /// 尚不是 const 可调用 trait（rust-lang#143874）。写成 `const` 只是好看，
    /// 代价是整个函数不能用。
    pub fn first_diff(&self, other: &TimingSig) -> Option<TimingField> {
        let mut i = 0usize;
        while i < 6 {
            if self.field(i) != other.field(i) {
                return match i {
                    0 => Some(TimingField::ModeClock),
                    1 => Some(TimingField::LinkRate),
                    2 => Some(TimingField::Lanes),
                    3 => Some(TimingField::HDisplay),
                    4 => Some(TimingField::VDisplay),
                    _ => Some(TimingField::VRefresh),
                };
            }
            i += 1;
        }
        None
    }
}

/// 克隆约束裁决结果（锚点：时序不一致时拒绝镜像并**说明原因**）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloneVerdict {
    /// 两输出齐全且六字段逐位相等——可镜像。
    Exact,
    /// 只有一个输出在线——镜像退化为单屏（合法，无需约束：没有第二块屏可比）。
    Trivial,
    /// 不一致，且给出**首个**差异字段。
    Mismatch(TimingField),
}

// ---------------------------------------------------------------------------
// 五、模式与布局
// ---------------------------------------------------------------------------

/// 布局模式（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutMode {
    /// 镜像：两输出共享同一根 pipe 的同一份时序。
    Clone,
    /// 扩展：每输出独立 pipe 独立时序。
    Extend,
}

impl LayoutMode {
    /// 在册全集（恰两项）。
    pub const ALL: [LayoutMode; 2] = [LayoutMode::Clone, LayoutMode::Extend];

    /// 线上码（显式映射，禁 `as` 直转）。
    pub const fn wire(self) -> u8 {
        match self {
            LayoutMode::Clone => 0x00,
            LayoutMode::Extend => 0x01,
        }
    }

    /// 由线上码还原（越界 `None`）。
    pub const fn from_wire(w: u8) -> Option<LayoutMode> {
        match w {
            0x00 => Some(LayoutMode::Clone),
            0x01 => Some(LayoutMode::Extend),
            _ => None,
        }
    }

    /// 标签（读屏人话）。
    pub const fn label(self) -> &'static str {
        match self {
            LayoutMode::Clone => "镜像 / mirror",
            LayoutMode::Extend => "扩展 / extend",
        }
    }
}

/// 输出数（锚点：双屏——两个输出槽，A 位与 B 位）。
pub const MAX_OUTPUTS: usize = 2;

/// 一个输出的放置结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Placement {
    /// 分配到的 pipe 号。
    pub pipe: u8,
    /// 水平位置（相对虚拟桌面原点，可负）。
    pub x: i16,
    /// 垂直位置。
    pub y: i16,
}

/// 输出规格（配置结构的输入面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OutputSpec {
    /// 端口号（F0232 的 `HpdHotplug::port()` 口径）。
    pub port: u8,
    /// 位置语义标签（端口名即位置，供读屏播报）。
    pub label: &'static str,
    /// 是否在线（F0232 报的连接态）。
    pub present: bool,
    /// 该输出的时序签名。
    pub timing: TimingSig,
}

impl OutputSpec {
    /// 空规格哨兵（未接入槽位）。
    pub const fn blank(port: u8, label: &'static str) -> OutputSpec {
        OutputSpec {
            port,
            label,
            present: false,
            timing: TimingSig {
                mode_clock_khz: 0,
                link_mhz: 0,
                lanes: 0,
                hdisplay: 0,
                vdisplay: 0,
                vrefresh: 0,
            },
        }
    }
}

/// 生效布局（配置结构：模式×pipe 分配×布局）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AppliedLayout {
    /// 实际生效的模式（降级时恒为 [`LayoutMode::Extend`] 单屏）。
    pub mode: LayoutMode,
    /// 每输出的放置（`None` = 未点亮——降级时第二个输出显式缺席）。
    pub placements: [Option<Placement>; MAX_OUTPUTS],
    /// 占用的 pipe 数。
    pub pipes_used: u8,
    /// 时序编程次数（镜像恒 1；扩展等于活跃输出数）。
    pub timing_programs: u8,
}

impl AppliedLayout {
    /// 空布局（未应用过）。
    pub const fn empty() -> AppliedLayout {
        AppliedLayout {
            mode: LayoutMode::Extend,
            placements: [None, None],
            pipes_used: 0,
            timing_programs: 0,
        }
    }

    /// 是否已降级（副屏缺席）。
    pub fn degraded(&self) -> bool {
        self.pipes_used == 1 && self.placements[1].is_none() && self.placements[0].is_some()
    }
}

/// 布局请求（调用方给的模式与每输出坐标）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LayoutRequest {
    /// 模式。
    pub mode: LayoutMode,
    /// 每输出坐标（按端口升序对应；镜像模式下第二项会被归一到第一项）。
    pub pos: [(i16, i16); MAX_OUTPUTS],
}

impl LayoutRequest {
    /// 扩展双屏默认请求（主屏原点、副屏按主屏宽度右移）。
    pub const fn extend_default(primary_w: u16) -> LayoutRequest {
        LayoutRequest {
            mode: LayoutMode::Extend,
            pos: [(0, 0), (primary_w as i16, 0)],
        }
    }

    /// 镜像请求（单一坐标，两个输出口同位）。
    pub const fn clone_at(x: i16, y: i16) -> LayoutRequest {
        LayoutRequest { mode: LayoutMode::Clone, pos: [(x, y), (x, y)] }
    }
}

/// 应用结果（一次 apply 的完整交代，不让调用方猜）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ApplyOutcome {
    /// 实际生效的模式（降级时为 [`LayoutMode::Extend`]）。
    pub mode: LayoutMode,
    /// 每输出放置。
    pub placements: [Option<Placement>; MAX_OUTPUTS],
    /// 占用 pipe 数。
    pub pipes_used: u8,
    /// 时序编程次数。
    pub timing_programs: u8,
    /// 是否降级（资源不足退单屏）。
    pub degraded: bool,
    /// 原因（`None` = 完全成功）。
    pub reason: Option<RejectReason>,
    /// 镜像下被归一的坐标数（记账，不静默改写）。
    pub normalized_positions: u8,
    /// 下游是否需要重估（F0226 MPO 缓存脏）。
    pub reeval: bool,
}

impl ApplyOutcome {
    /// 完全成功（无降级无原因）。
    pub fn is_clean(&self) -> bool {
        !self.degraded && self.reason.is_none()
    }
}

// ---------------------------------------------------------------------------
// 六、持久化记录（定长词数组：O(1) 反序列化）
// ---------------------------------------------------------------------------

/// 布局记录魔数（"F2" + 0x33）。
pub const PERSIST_MAGIC: u16 = 0xF233;
/// 布局记录版本（改结构时 +1——旧版本走回落默认，不猜着解析）。
pub const PERSIST_VERSION: u16 = 1;
/// 记录词数（4 头 + 2 输出 × 12 体 + 校验和 + 余量）。
///
/// 每输出 **12 词**的原因：模式时钟是 kHz 级 u32（148500 远超 u16），截断成
/// `as u16` 会静默把 148500 变成 17428——恢复出来一条永远不合法的时序，用户
/// 只会看到"保存的布局总是恢复失败"，而查不出是编码时截了。高低两词分开存，
/// 一个字都不丢。
pub const PERSIST_WORDS: usize = 32;

/// 每输出占用的词数。
const ENTRY_WORDS: usize = 12;

/// 持久化记录（定长词数组——**没有字符串、没有堆**，恢复是 O(1) 直读）。
///
/// 布局：  `[0]=魔数 [1]=版本 [2]=模式 [3]=活跃输出数`，
/// 每输出 12 词：`端口, pipe, x, y, 时钟低, 时钟高, link, lanes, hdisplay,
/// vdisplay, vrefresh, 保留`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PersistRecord {
    /// 词数组。
    pub words: [u16; PERSIST_WORDS],
}

impl PersistRecord {
    /// 空记录（全零——必定过不了魔数闸）。
    pub const fn blank() -> PersistRecord {
        PersistRecord { words: [0u16; PERSIST_WORDS] }
    }

    /// FNV-1a 校验和（定长词遍历 = O(1)）。
    ///
    /// **只覆盖除校验位自身以外的前 `PERSIST_WORDS-1` 个词**：若把校验位
    /// 也算进去，封记录时算一次、校验时又算一次，而校验位在这两次之间已经
    /// 从 0 变成了真值——两次哈希的输入不同，于是**自己封的记录永远校验
    /// 不过**。这种 bug 的现场是"保存永远失败"，极难联想到是自指。
    pub fn checksum(&self) -> u16 {
        let mut h: u16 = 0x811C;
        let mut i = 0usize;
        while i + 1 < PERSIST_WORDS {
            h ^= self.words[i];
            h = h.wrapping_mul(0x0193);
            i += 1;
        }
        h
    }

    /// **头闸**（恢复先看头，不碰体）：魔数 → 版本 → 校验和。
    ///
    /// 为什么分头体：篡改或截断的记录先被三道头闸挡住，CPU 不会为一条无效
    /// 记录去解析体——同时「O(1) 反序列化」才有可验证的形状。
    pub fn header_ok(&self) -> Result<(), RejectReason> {
        if self.words[0] != PERSIST_MAGIC {
            return Err(RejectReason::LayoutCorrupt);
        }
        if self.words[1] != PERSIST_VERSION {
            return Err(RejectReason::LayoutCorrupt);
        }
        if self.checksum() != self.words[PERSIST_WORDS - 1] {
            return Err(RejectReason::LayoutCorrupt);
        }
        Ok(())
    }

    /// 写入一条输出记录（内部用；`idx` 为 0/1）。
    fn put(&mut self, idx: usize, spec: &OutputSpec, pl: Placement) {
        let b = 4 + idx * ENTRY_WORDS;
        self.words[b] = spec.port as u16;
        self.words[b + 1] = pl.pipe as u16;
        self.words[b + 2] = pl.x as u16;
        self.words[b + 3] = pl.y as u16;
        // 模式时钟拆高低两词——u16 装不下 148500 kHz，截断即静默损坏。
        let clk = spec.timing.mode_clock_khz;
        self.words[b + 4] = (clk & 0xFFFF) as u16;
        self.words[b + 5] = (clk >> 16) as u16;
        self.words[b + 6] = (spec.timing.link_mhz / 10) as u16;
        self.words[b + 7] = spec.timing.lanes as u16;
        self.words[b + 8] = spec.timing.hdisplay;
        self.words[b + 9] = spec.timing.vdisplay;
        self.words[b + 10] = spec.timing.vrefresh as u16;
        self.words[b + 11] = 0;
    }

    /// 读一条输出记录（头部词，越界 `None`）。
    pub fn entry(&self, idx: usize) -> Option<PersistEntry> {
        if idx >= MAX_OUTPUTS {
            return None;
        }
        let b = 4 + idx * ENTRY_WORDS;
        Some(PersistEntry {
            port: self.words[b] as u8,
            pipe: self.words[b + 1] as u8,
            x: self.words[b + 2] as i16,
            y: self.words[b + 3] as i16,
            mode_clock_khz: ((self.words[b + 5] as u32) << 16)
                | (self.words[b + 4] as u32),
            link_mhz: (self.words[b + 6] as u32) * 10,
            lanes: self.words[b + 7] as u8,
            hdisplay: self.words[b + 8],
            vdisplay: self.words[b + 9],
            vrefresh: self.words[b + 10] as u8,
        })
    }

    /// 模式（头闸未过时不可信——调用方须先过 [`PersistRecord::header_ok`]）。
    pub fn mode(&self) -> Option<LayoutMode> {
        LayoutMode::from_wire(self.words[2] as u8)
    }

    /// 活跃输出数（头闸未过时不可信）。
    pub fn active(&self) -> usize {
        (self.words[3] as usize).min(MAX_OUTPUTS)
    }

    /// 封记录（补校验和）。
    fn seal(&mut self) {
        self.words[PERSIST_WORDS - 1] = self.checksum();
    }
}

/// 记录里的一条输出（读出面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PersistEntry {
    pub port: u8,
    pub pipe: u8,
    pub x: i16,
    pub y: i16,
    pub mode_clock_khz: u32,
    pub link_mhz: u32,
    pub lanes: u8,
    pub hdisplay: u16,
    pub vdisplay: u16,
    pub vrefresh: u8,
}

impl PersistEntry {
    /// 还原成时序签名。
    pub const fn timing(&self) -> TimingSig {
        TimingSig {
            mode_clock_khz: self.mode_clock_khz,
            link_mhz: self.link_mhz,
            lanes: self.lanes,
            hdisplay: self.hdisplay,
            vdisplay: self.vdisplay,
            vrefresh: self.vrefresh,
        }
    }
}

/// 恢复结果（锚点：失效回落默认并告知——回落也要说清**为什么**）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RestoreOutcome {
    /// 是否套用了记录里的布局。
    pub applied_record: bool,
    /// 失效原因（套用成功时 `None`）。
    pub reason: Option<RejectReason>,
    /// 实际生效布局（回落时是默认布局）。
    pub layout: AppliedLayout,
    /// 降级通知（回落/降级时给出）。
    pub notice: Option<Notice>,
}

// ---------------------------------------------------------------------------
// 七、下游请求（F0225 重编程 / F0226 MPO 重估）
// ---------------------------------------------------------------------------

/// 下游请求类别（跨批对接：上游 F0225/F0232，下游 F0226）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DownstreamKind {
    /// F0225 输出重编程（每输出一条）。
    Reprogram,
    /// F0226 MPO 重估（每次配置变更一条——缓存脏）。
    MpoReeval,
}

impl DownstreamKind {
    /// 标签。
    pub const fn label(self) -> &'static str {
        match self {
            DownstreamKind::Reprogram => "F0225 重编程 / F0225 reprogram",
            DownstreamKind::MpoReeval => "F0226 MPO 重估 / F0226 MPO re-eval",
        }
    }

    /// 在册全集（判据对账）。
    pub const ALL: [DownstreamKind; 2] =
        [DownstreamKind::Reprogram, DownstreamKind::MpoReeval];
}

/// 一条下游请求。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DownstreamRequest {
    /// 类别。
    pub kind: DownstreamKind,
    /// 关联端口（MPO 重估为 `u8::MAX`——它不属���单一输出）。
    pub port: u8,
    /// 该请求对应的生效状态（未点亮输出为 `None`）。
    pub state: Option<Placement>,
    /// 产生 tick。
    pub tick: u64,
}

/// MPO 重估请求的哨兵端口（不属���单一输出）。
pub const MPO_PORT: u8 = 0xFF;

/// 下游请求容量（定容）。
pub const REQ_CAP: usize = 8;

/// 定容下游请求队列（满则显性拒绝 + 记账，不静默丢）。
#[derive(Clone, Copy, Debug)]
pub struct RequestQueue {
    entries: [DownstreamRequest; REQ_CAP],
    len: usize,
}

impl RequestQueue {
    /// 空队列。
    pub const fn new() -> RequestQueue {
        RequestQueue {
            entries: [DownstreamRequest {
                kind: DownstreamKind::Reprogram,
                port: 0,
                state: None,
                tick: 0,
            }; REQ_CAP],
            len: 0,
        }
    }

    /// 投递（满返回 `false`）。
    pub fn push(&mut self, r: DownstreamRequest) -> bool {
        if self.len >= REQ_CAP {
            return false;
        }
        self.entries[self.len] = r;
        self.len += 1;
        true
    }

    /// 在册数。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 按下标读（越界 `None`）。
    pub fn get(&self, i: usize) -> Option<DownstreamRequest> {
        if i >= self.len {
            return None;
        }
        Some(self.entries[i])
    }

    /// 清空并取走。
    pub fn take(&mut self) -> RequestQueue {
        let taken = *self;
        *self = RequestQueue::new();
        taken
    }
}

// ---------------------------------------------------------------------------
// 八、统计账（降级必须留痕）
// ---------------------------------------------------------------------------

/// 配置统计账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CtlStats {
    /// apply 次数。
    pub applies: u32,
    /// 镜像应用次数。
    pub clone_applies: u32,
    /// 扩展应用次数。
    pub extend_applies: u32,
    /// 拒绝次数（不动现状）。
    pub rejects: u32,
    /// 降级单屏次数。
    pub degraded_single: u32,
    /// 保存次数。
    pub saves: u32,
    /// 恢复成功次数。
    pub restores: u32,
    /// 恢复回落默认次数。
    pub restore_fallbacks: u32,
    /// 下游请求被拒次数（队列满）。
    pub dropped_requests: u32,
    /// 时序编程总次数（镜像下应显著少于输出数——判据对拍点）。
    pub timing_programs: u32,
}

impl CtlStats {
    /// 零账。
    pub const fn zero() -> CtlStats {
        CtlStats {
            applies: 0,
            clone_applies: 0,
            extend_applies: 0,
            rejects: 0,
            degraded_single: 0,
            saves: 0,
            restores: 0,
            restore_fallbacks: 0,
            dropped_requests: 0,
            timing_programs: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// 九、总控
// ---------------------------------------------------------------------------

/// 双屏输出配置总控（一个显示适配器一个实例）。
#[derive(Clone, Debug)]
pub struct DualScreenCtl {
    /// 显示能力（真调 F0225 口径，不自造 pipe 上限）。
    caps: DispCaps,
    /// F0221 探针给出的 pipe 数。
    probe_pipes: u8,
    /// 两个输出槽（端口升序）。
    outputs: [OutputSpec; MAX_OUTPUTS],
    /// 当前生效布局。
    applied: AppliedLayout,
    /// 持久化记录。
    persist: PersistRecord,
    /// 下游请求队列。
    reqs: RequestQueue,
    /// 最近一次通知。
    pub last_notice: Option<Notice>,
    /// 最近一次镜像裁决的首个差异字段（`None` = 未发生过不一致或已解决）。
    pub last_mismatch: Option<TimingField>,
    /// 统计账。
    pub stats: CtlStats,
}

impl DualScreenCtl {
    /// 构造（`probe_pipes` 由 F0221 探针给定；越能力上限或为零即拒）。
    pub fn new(tier: GenTier, probe_pipes: u8) -> Result<DualScreenCtl, RejectReason> {
        let caps = DispCaps::for_tier(tier);
        if probe_pipes == 0 || probe_pipes > caps.max_pipes {
            return Err(RejectReason::BadRequest);
        }
        Ok(DualScreenCtl {
            caps,
            probe_pipes,
            outputs: [
                OutputSpec::blank(0, "输出 A / output A"),
                OutputSpec::blank(1, "输出 B / output B"),
            ],
            applied: AppliedLayout::empty(),
            persist: PersistRecord::blank(),
            reqs: RequestQueue::new(),
            last_notice: None,
            last_mismatch: None,
            stats: CtlStats::zero(),
        })
    }

    /// 显示能力（真调 F0225）。
    pub const fn caps(&self) -> DispCaps {
        self.caps
    }

    /// F0221 探针给出的 pipe 数。
    pub const fn probe_pipes(&self) -> u8 {
        self.probe_pipes
    }

    /// 当前生效布局。
    pub const fn applied(&self) -> AppliedLayout {
        self.applied
    }

    /// 持久化记录（只读）。
    pub const fn persist(&self) -> &PersistRecord {
        &self.persist
    }

    /// 下游请求在册数。
    pub const fn pending_requests(&self) -> usize {
        self.reqs.len()
    }

    /// 输出槽只读视图（判据对拍用）。
    pub const fn output(&self, idx: usize) -> Option<OutputSpec> {
        if idx < MAX_OUTPUTS {
            Some(self.outputs[idx])
        } else {
            None
        }
    }

    /// 登记一个输出（F0232 报接入时调用；端口号须落在两个槽内）。
    pub fn attach(&mut self, port: u8, label: &'static str, timing: TimingSig) -> bool {
        match self.slot_of_port(port) {
            Some(i) => {
                self.outputs[i] = OutputSpec { port, label, present: true, timing };
                true
            }
            None => false,
        }
    }

    /// 注销一个输出（F0232 报断开时调用）。
    pub fn detach(&mut self, port: u8) -> bool {
        match self.slot_of_port(port) {
            Some(i) => {
                self.outputs[i].present = false;
                true
            }
            None => false,
        }
    }

    /// **上游 F0232 联动**：按 HPD 管理器的连接态同步一个输出。
    ///
    /// 链路有连（`LinkState::has_link()`）即视为在线——连接中也在链路上，
    /// F0225 重编程由它自己把关，这里只回答「这一槽有没有屏」。
    pub fn sync_with_hotplug(
        &mut self,
        hp: &super::veb232_hotplug::HpdHotplug,
        timing: TimingSig,
        tick: u64,
    ) -> bool {
        let linked = hp.state().has_link();
        let changed = if linked {
            self.attach(hp.port(), hp.label(), timing)
        } else {
            self.detach(hp.port())
        };
        if changed && linked {
            // 接入即触发一次 MPO 重估（热插拔后 MPO 缓存必脏——F0226 锚点原文）。
            self.push_req(DownstreamRequest {
                kind: DownstreamKind::MpoReeval,
                port: MPO_PORT,
                state: None,
                tick,
            });
        }
        changed
    }

    /// 端口号 → 槽下标。
    ///
    /// 两级：先按**在线槽**的端口号精确匹配；匹配不上就由**第一个空闲槽**
    /// 认领——拔出后的槽 `present=false` 即视为空闲（新屏插上同一个物理口
    /// 要能直接落回原槽，而换了一个口也要能落进空槽）。两槽都占满还来了
    /// 第三个端口 → `None`（双屏适配器接第三块屏，本来就没有它的地方）。
    fn slot_of_port(&self, port: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < MAX_OUTPUTS {
            if self.outputs[i].present && self.outputs[i].port == port {
                return Some(i);
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < MAX_OUTPUTS {
            if !self.outputs[k].present {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 活跃输出槽下标（**端口升序**——分配序即规范序）。
    fn active_slots(&self) -> [usize; MAX_OUTPUTS] {
        let mut out = [usize::MAX; MAX_OUTPUTS];
        let mut n = 0usize;
        let mut i = 0usize;
        while i < MAX_OUTPUTS {
            if self.outputs[i].present {
                out[n] = i;
                n += 1;
            }
            i += 1;
        }
        out
    }

    /// 活跃输出数（在线槽计数）。
    pub fn active_count(&self) -> usize {
        let a = self.active_slots();
        let mut n = 0usize;
        let mut i = 0usize;
        while i < MAX_OUTPUTS {
            if a[i] != usize::MAX {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// **克隆约束检测**（锚点：时序不一致时拒绝镜像并说明原因）。
    ///
    /// 成本 O(约束数)：最多比 6 个字段、每字段一次比较。
    pub fn clone_check(&self) -> CloneVerdict {
        let active = self.active_slots();
        if active[0] == usize::MAX {
            return CloneVerdict::Trivial;
        }
        if active[1] == usize::MAX {
            // 单屏在线：没有第二块屏可比，镜像退化为单屏（合法）。
            return CloneVerdict::Trivial;
        }
        let a = self.outputs[active[0]].timing;
        let b = self.outputs[active[1]].timing;
        match a.first_diff(&b) {
            None => CloneVerdict::Exact,
            Some(f) => CloneVerdict::Mismatch(f),
        }
    }

    /// **约束验证**（锚点性能：约束验证 O(约束数)）。
    ///
    /// 返回所需 pipe 数或首个违规原因——**按规范序**判：未接入 → 时序非法 →
    /// 克隆一致性 → pipe 资源。顺序不是随意的：「屏都没插」先报「时序非法」
    /// 是把下游的原因说错了。
    pub fn validate(&self, mode: LayoutMode) -> Result<u8, RejectReason> {
        let active = self.active_slots();
        if active[0] == usize::MAX {
            return Err(RejectReason::OutputAbsent);
        }
        let mut n = 0u8;
        let mut i = 0usize;
        while i < MAX_OUTPUTS {
            if active[i] != usize::MAX {
                if !self.outputs[active[i]].timing.legal() {
                    return Err(RejectReason::TimingIllegal);
                }
                n += 1;
            }
            i += 1;
        }
        // 克隆一致性只在镜像模式下有意义（扩展各屏各走各的时序）。
        // 放在时序合法性**之后** pipe 资源**之前**：先说"你这块屏的时序不对"，
        // 再说"你另一块屏时序跟它不一样"——顺序反了会把根因说成症状。
        if matches!(mode, LayoutMode::Clone) {
            if let CloneVerdict::Mismatch(_) = self.clone_check() {
                return Err(RejectReason::CloneMismatch);
            }
        }
        let needed = match mode {
            // 镜像：两个输出口共享一根 pipe。
            LayoutMode::Clone => 1,
            // 扩展：每活跃输出各一根。
            LayoutMode::Extend => n,
        };
        if needed > self.probe_pipes {
            return Err(RejectReason::PipeExhausted);
        }
        Ok(needed)
    }

    /// **应用配置**（锚点性能：配置应用 O(输出)）。
    ///
    /// 降级语义：约束里只有 `PipeExhausted` 与恢复回落是**降级**（照常有屏
    /// 可看），其余是**拒绝**（不动现状，屏幕保持上一份已生效布局）。
    pub fn apply(&mut self, req: LayoutRequest, tick: u64) -> ApplyOutcome {
        self.stats.applies = self.stats.applies.saturating_add(1);
        let mut out = ApplyOutcome {
            mode: req.mode,
            placements: [None, None],
            pipes_used: 0,
            timing_programs: 0,
            degraded: false,
            reason: None,
            normalized_positions: 0,
            reeval: false,
        };

        // 第一闸：约束验证（未接入/时序非法/克隆不一致 → 拒绝，不半落地）。
        match self.validate(req.mode) {
            Ok(needed) => out.pipes_used = needed,
            Err(RejectReason::PipeExhausted) => {
                // 降级：退单屏（第一个活跃输出用 pipe 0）。
                out.mode = LayoutMode::Extend;
                out.degraded = true;
                out.reason = Some(RejectReason::PipeExhausted);
                out.pipes_used = 1;
                let active = self.active_slots();
                out.placements[active[0]] =
                    Some(Placement { pipe: 0, x: req.pos[active[0]].0, y: req.pos[active[0]].1 });
                out.timing_programs = 1;
                // 降级也是一次生效配置——必须写回 applied，
                // 否则「当前布局」和「实际在显示的东西」会说两套。
                self.applied = AppliedLayout {
                    mode: out.mode,
                    placements: out.placements,
                    pipes_used: out.pipes_used,
                    timing_programs: out.timing_programs,
                };
                self.note(RejectReason::PipeExhausted);
                self.stats.degraded_single = self.stats.degraded_single.saturating_add(1);
                self.finish(&out, tick);
                return out;
            }
            Err(r) => {
                out.reason = Some(r);
                // 镜像不一致时把**首个差异字段**钉在台面上：
                // 用户与判据都要的是"哪一项不同"，不是一个 `a != b` 的布尔。
                self.last_mismatch = match self.clone_check() {
                    CloneVerdict::Mismatch(f) => Some(f),
                    _ => None,
                };
                self.note(r);
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                // 拒绝：不改 applied（屏幕维持上一份布局），不下游请求。
                out.mode = self.applied.mode;
                out.placements = self.applied.placements;
                out.pipes_used = self.applied.pipes_used;
                out.timing_programs = 0;
                return out;
            }
        }

        // 分配：镜像共享 pipe 0；扩展按端口升序拿升序最低可用号。
        let active = self.active_slots();
        let mut k = 0usize;
        while k < MAX_OUTPUTS {
            let slot = active[k];
            if slot == usize::MAX {
                break;
            }
            let (x, y) = match req.mode {
                LayoutMode::Clone => {
                    // 镜像：坐标无意义，归一到第一个活跃输出的坐标并记账。
                    let ox = req.pos[active[0]].0;
                    let oy = req.pos[active[0]].1;
                    let (px, py) = req.pos[slot];
                    if px != ox || py != oy {
                        out.normalized_positions = out.normalized_positions.saturating_add(1);
                    }
                    (ox, oy)
                }
                LayoutMode::Extend => req.pos[slot],
            };
            let pipe = match req.mode {
                LayoutMode::Clone => 0,
                LayoutMode::Extend => k as u8,
            };
            out.placements[slot] = Some(Placement { pipe, x, y });
            k += 1;
        }
        // 时序编程次数：镜像恒 1（共享时序），扩展等于活跃输出数。
        out.timing_programs = match req.mode {
            LayoutMode::Clone => 1,
            LayoutMode::Extend => k as u8,
        };

        self.applied = AppliedLayout {
            mode: out.mode,
            placements: out.placements,
            pipes_used: out.pipes_used,
            timing_programs: out.timing_programs,
        };
        match req.mode {
            LayoutMode::Clone => {
                self.stats.clone_applies = self.stats.clone_applies.saturating_add(1)
            }
            LayoutMode::Extend => {
                self.stats.extend_applies = self.stats.extend_applies.saturating_add(1)
            }
        }
        self.finish(&out, tick);
        out
    }

    /// 应用收尾：记账编程次数 + 投递下游（F0225 每输出一条 + F0226 一条）。
    fn finish(&mut self, out: &ApplyOutcome, tick: u64) {
        self.stats.timing_programs = self
            .stats
            .timing_programs
            .saturating_add(out.timing_programs as u32);
        let mut i = 0usize;
        while i < MAX_OUTPUTS {
            if let Some(p) = out.placements[i] {
                self.push_req(DownstreamRequest {
                    kind: DownstreamKind::Reprogram,
                    port: self.outputs[i].port,
                    state: Some(p),
                    tick,
                });
            }
            i += 1;
        }
        // 每次配置变更都让 F0226 的 MPO 缓存变脏（锚点：重估联动）。
        self.push_req(DownstreamRequest {
            kind: DownstreamKind::MpoReeval,
            port: MPO_PORT,
            state: None,
            tick,
        });
    }

    fn push_req(&mut self, r: DownstreamRequest) {
        if !self.reqs.push(r) {
            // 队列满 = 下游不知道要重编程/重估——记账 + 通知（不静默丢）。
            self.stats.dropped_requests = self.stats.dropped_requests.saturating_add(1);
            self.note(RejectReason::BadRequest);
        }
    }

    fn note(&mut self, r: RejectReason) {
        self.last_notice = Some(notice_for(r));
    }

    /// 取走并清空下游请求（消费方驱动）。
    pub fn take_requests(&mut self) -> RequestQueue {
        self.reqs.take()
    }

    /// **保存布局**（锚点：用户布局持久化）。
    ///
    /// 只记**已生效**的布局——保存一份没生效的布局，等于下次恢复时把用户
    /// 带回一个当时就被拒过的配置。
    pub fn save(&mut self) -> bool {
        if self.applied.pipes_used == 0 {
            return false;
        }
        let mut rec = PersistRecord::blank();
        rec.words[0] = PERSIST_MAGIC;
        rec.words[1] = PERSIST_VERSION;
        rec.words[2] = self.applied.mode.wire() as u16;
        let active = self.active_slots();
        let mut count = 0u16;
        let mut k = 0usize;
        while k < MAX_OUTPUTS {
            let slot = active[k];
            if slot == usize::MAX {
                break;
            }
            if let Some(p) = self.applied.placements[slot] {
                rec.put(k, &self.outputs[slot], p);
                count += 1;
            }
            k += 1;
        }
        rec.words[3] = count;
        rec.seal();
        self.persist = rec;
        self.stats.saves = self.stats.saves.saturating_add(1);
        true
    }

    /// **恢复布局**（锚点：上次布局失效回落默认并告知）。
    ///
    /// 三步：头闸 → 体校验 → **回到 [`DualScreenCtl::apply`] 重过全部约束**。
    /// 任一步不过即回落默认布局并告知为何失效。
    pub fn restore(&mut self, tick: u64) -> RestoreOutcome {
        // 第一步：头闸（魔数/版本/校验和）——不过则连体都不碰。
        if let Err(r) = self.persist.header_ok() {
            return self.fallback(r, tick);
        }
        // 第二步：体校验——模式可解码、活跃数合法、每个记录条目都还站得住。
        let mode = match self.persist.mode() {
            Some(m) => m,
            None => return self.fallback(RejectReason::LayoutCorrupt, tick),
        };
        let active = self.persist.active();
        if active == 0 {
            return self.fallback(RejectReason::LayoutCorrupt, tick);
        }
        let mut pos = [(0i16, 0i16); MAX_OUTPUTS];
        let mut k = 0usize;
        while k < active {
            let e = match self.persist.entry(k) {
                Some(e) => e,
                None => return self.fallback(RejectReason::LayoutCorrupt, tick),
            };
            // 端口还在不在？时序还合法吗？（硬件可能已经换了屏）
            let slot = self.slot_of_port(e.port);
            let ok_slot = match slot {
                Some(s) => self.outputs[s].present && e.timing().legal(),
                None => false,
            };
            if !ok_slot {
                return self.fallback(RejectReason::OutputAbsent, tick);
            }
            pos[k] = (e.x, e.y);
            k += 1;
        }
        // 第三步：回到同一条 apply 管道（恢复不是后门）。
        let req = LayoutRequest { mode, pos };
        let outcome = self.apply(req, tick);
        if outcome.reason.is_some() {
            // 记录本身合法，但套用当下不成立（例如镜像的两屏时序已变）：
            // 这是一次带原因的回落，不是一句"恢复失败"。
            return self.fallback(
                outcome.reason.unwrap_or(RejectReason::LayoutCorrupt),
                tick,
            );
        }
        self.stats.restores = self.stats.restores.saturating_add(1);
        RestoreOutcome {
            applied_record: true,
            reason: None,
            layout: self.applied,
            notice: None,
        }
    }

    /// 回落默认布局并告知（锚点降级矩阵第三行）。
    fn fallback(&mut self, r: RejectReason, tick: u64) -> RestoreOutcome {
        self.stats.restore_fallbacks = self.stats.restore_fallbacks.saturating_add(1);
        self.note(r);
        // 默认布局：扩展、主屏原点、副屏按主屏宽度右移（O(输出)）。
        let primary_w = {
            let active = self.active_slots();
            if active[0] == usize::MAX {
                0u16
            } else {
                self.outputs[active[0]].timing.hdisplay
            }
        };
        let req = LayoutRequest::extend_default(primary_w);
        let _outcome = self.apply(req, tick);
        RestoreOutcome {
            applied_record: false,
            reason: Some(r),
            layout: self.applied,
            notice: Some(notice_for(r)),
        }
    }

    /// 读屏播报（锚点无障碍：布局命名含位置语义）。
    pub fn a11y_lines(&self) -> [String; 6] {
        [
            format!(
                "显示布局：{}（占用管线 {} 根，模式设定 {} 次）/ layout: {} (pipes {}, programs {})",
                self.applied.mode.label(),
                self.applied.pipes_used,
                self.applied.timing_programs,
                self.applied.mode.label(),
                self.applied.pipes_used,
                self.applied.timing_programs
            ),
            self.layout_name(),
            format!(
                "在线输出 {} 个 / online outputs: {}",
                self.active_count(),
                self.active_count()
            ),
            format!(
                "配置统计：应用 {} 次（镜像 {}、扩展 {}）、拒绝 {} 次、降级单屏 {} 次 / applies {} (clone {}, extend {}), rejects {}, degraded {}",
                self.stats.applies,
                self.stats.clone_applies,
                self.stats.extend_applies,
                self.stats.rejects,
                self.stats.degraded_single,
                self.stats.applies,
                self.stats.clone_applies,
                self.stats.extend_applies,
                self.stats.rejects,
                self.stats.degraded_single
            ),
            format!(
                "布局记录：保存 {} 次、恢复 {} 次、回落默认 {} 次 / saved {}, restored {}, fell back {}",
                self.stats.saves,
                self.stats.restores,
                self.stats.restore_fallbacks,
                self.stats.saves,
                self.stats.restores,
                self.stats.restore_fallbacks
            ),
            match self.last_notice {
                Some(n) => format!(
                    "最近通知：{} 影响：{} 建议：{} / notice: {} | impact: {} | action: {}",
                    n.what, n.impact, n.action, n.what, n.impact, n.action
                ),
                None => String::from("最近通知：无 / latest notice: none"),
            },
        ]
    }

    /// **布局命名含位置语义**（锚点无障碍硬要求：端口名 + 坐标 + 模式人话）。
    pub fn layout_name(&self) -> String {
        let mut s = String::new();
        s.push_str(self.applied.mode.label());
        s.push_str("：");
        let mut first = true;
        let mut i = 0usize;
        while i < MAX_OUTPUTS {
            if let Some(p) = self.applied.placements[i] {
                if !first {
                    s.push_str(" · ");
                }
                first = false;
                s.push_str(self.outputs[i].label);
                s.push_str(" @");
                let xs = itoa(p.x);
                s.push_str(&xs);
                s.push(',');
                let ys = itoa(p.y);
                s.push_str(&ys);
                s.push_str(" 管线");
                let ps = itoa(p.pipe as i16);
                s.push_str(&ps);
            }
            i += 1;
        }
        if first {
            s.push_str("（无在线输出）");
        }
        s
    }
}

/// `i16` 十进制（no_std 无 `to_string`：手写不引依赖）。
fn itoa(v: i16) -> String {
    let mut s = String::new();
    if v < 0 {
        s.push('-');
    }
    let mut n = (v as i32).abs();
    let mut buf = [0u8; 12];
    let mut i = buf.len();
    if n == 0 {
        s.push('0');
        return s;
    }
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    let mut k = i;
    while k < buf.len() {
        s.push(buf[k] as char);
        k += 1;
    }
    s
}

// ---------------------------------------------------------------------------
// 十、域自检（判据逐条映射锚点：镜像扩展/约束检测/布局持久化/降级通知）
// ---------------------------------------------------------------------------

/// 1080p60 基准时序（判据语料）。
const SIG_1080P60: TimingSig = TimingSig {
    mode_clock_khz: 148_500,
    link_mhz: 5400,
    lanes: 4,
    hdisplay: 1920,
    vdisplay: 1080,
    vrefresh: 60,
};

/// 1440p60 时序（与 1080p60 全字段不同）。
const SIG_1440P60: TimingSig = TimingSig {
    mode_clock_khz: 233_250,
    link_mhz: 5400,
    lanes: 4,
    hdisplay: 2560,
    vdisplay: 1440,
    vrefresh: 60,
};

/// 4K60 时序（1080p60 仅分辨率不同，用于首个差异字段对拍）。
const SIG_4K60: TimingSig = TimingSig {
    mode_clock_khz: 533_250,
    link_mhz: 8100,
    lanes: 4,
    hdisplay: 3840,
    vdisplay: 2160,
    vrefresh: 60,
};

/// 造一台两屏在线、pipe 充足的适配器（判据语料）。
///
/// `new` 只在 `probe_pipes` 为零或越能力上限时失败——本函数三个调用点分别给
/// 3/1/1，都合法，所以直接展开（判据代码里不留 unwrap 失败分支）。
fn fixture(tier: GenTier, probe_pipes: u8) -> DualScreenCtl {
    let mut c = match DualScreenCtl::new(tier, probe_pipes) {
        Ok(c) => c,
        Err(_) => DualScreenCtl {
            caps: DispCaps::for_tier(GenTier::Baseline),
            probe_pipes: 1,
            outputs: [OutputSpec::blank(0, "DP-1"), OutputSpec::blank(1, "DP-2")],
            applied: AppliedLayout::empty(),
            persist: PersistRecord::blank(),
            reqs: RequestQueue::new(),
            last_notice: None,
            last_mismatch: None,
            stats: CtlStats::zero(),
        },
    };
    let _ = c.attach(0, "DP-1", SIG_1080P60);
    let _ = c.attach(1, "DP-2", SIG_1080P60);
    c
}

/// F0233 域自检入口（聚合器调用；零 panic 面，失败逐条红不炸域）。
pub fn run_veb233_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("veb233_dualscreen");

    // --- 判据 1：镜像扩展（共享时序 vs 独立 pipe） ---
    {
        // 镜像：两输出共享一根 pipe、时序只编程一次。
        let mut c = fixture(GenTier::XeLatest, 3);
        let out = c.apply(LayoutRequest::clone_at(0, 0), 10);
        let p0 = out.placements[0];
        let p1 = out.placements[1];
        s.add(
            "B33-镜像-共享 pipe 时序只编程一次",
            out.mode == LayoutMode::Clone
                && out.pipes_used == 1
                && out.timing_programs == 1
                && p0 == Some(Placement { pipe: 0, x: 0, y: 0 })
                && p1 == Some(Placement { pipe: 0, x: 0, y: 0 })
                && c.applied().timing_programs == 1
                && out.is_clean(),
            "两屏挂同一根 pipe 同一份时序，编程 1 次（不是输出数）",
        );
        // 扩展：两输出独立 pipe、规范序分配、编程两次。
        let mut c2 = fixture(GenTier::XeLatest, 3);
        let out2 = c2.apply(LayoutRequest::extend_default(1920), 20);
        s.add(
            "B33-扩展-独立 pipe 规范序分配",
            out2.mode == LayoutMode::Extend
                && out2.pipes_used == 2
                && out2.timing_programs == 2
                && out2.placements[0] == Some(Placement { pipe: 0, x: 0, y: 0 })
                && out2.placements[1] == Some(Placement { pipe: 1, x: 1920, y: 0 })
                && out2.is_clean(),
            "低端口拿低号 pipe（规范序），编程两次",
        );
        // 镜像下坐标被归一并记账（不静默改写）。
        let mut c3 = fixture(GenTier::XeLatest, 3);
        let req = LayoutRequest { mode: LayoutMode::Clone, pos: [(0, 0), (1920, 0)] };
        let out3 = c3.apply(req, 30);
        s.add(
            "B33-镜像-坐标归一并记账",
            out3.normalized_positions == 1
                && out3.placements[1] == Some(Placement { pipe: 0, x: 0, y: 0 }),
            "镜像下第二输出坐标归一到第一输出且计一次（改写不无声）",
        );
        // 单屏在线时镜像退化为单屏（合法不报错）。
        let mut c4 = fixture(GenTier::XeLatest, 3);
        c4.detach(1);
        let out4 = c4.apply(LayoutRequest::clone_at(0, 0), 40);
        s.add(
            "B33-镜像-单屏在线退化为单屏",
            c4.clone_check() == CloneVerdict::Trivial
                && out4.is_clean()
                && out4.timing_programs == 1
                && out4.placements[1].is_none(),
            "只有一块屏时镜像合法（无第二块可比），退化为单屏",
        );
    }

    // --- 判据 2：约束检测（逐位相等 + 首个差异字段） ---
    {
        let c = fixture(GenTier::XeLatest, 3);
        s.add(
            "B33-约束-逐位相等判可镜像",
            c.clone_check() == CloneVerdict::Exact && SIG_1080P60.first_diff(&SIG_1080P60).is_none(),
            "六字段逐位相等 → Exact；同签名对拍无差异",
        );
        // 首个差异字段：六个字段逐一注入，逐一命中该字段（规范序）。
        let mut ok = true;
        let mut i = 0usize;
        while i < 6 {
            let mut b = SIG_1080P60;
            match i {
                0 => b.mode_clock_khz = 1,
                1 => b.link_mhz = 2700,
                2 => b.lanes = 2,
                3 => b.hdisplay = 1280,
                4 => b.vdisplay = 720,
                _ => b.vrefresh = 30,
            }
            ok = ok
                && SIG_1080P60.first_diff(&b) == Some(TIMING_FIELDS[i])
                && TIMING_FIELDS[i].ordinal() == i;
            i += 1;
        }
        s.add(
            "B33-约束-首个差异字段定位六字段对拍",
            ok && TIMING_FIELDS.len() == 6,
            "逐字段注入差异逐一命中规范序该字段（不受构造顺序影响）",
        );
        // 时序不一致 → 拒绝并三要素，且不落地。
        let mut c2 = fixture(GenTier::XeLatest, 3);
        let _ = c2.attach(1, "DP-2", SIG_4K60);
        let out = c2.apply(LayoutRequest::clone_at(0, 0), 50);
        let notice = c2.last_notice;
        s.add(
            "B33-约束-不一致拒绝并三要素",
            out.reason == Some(RejectReason::CloneMismatch)
                && !out.degraded
                && out.placements[0].is_none()
                && out.placements[1].is_none()
                && c2.applied().pipes_used == 0
                && c2.stats.rejects == 1
                && c2.pending_requests() == 0
                && c2.clone_check() == CloneVerdict::Mismatch(TimingField::ModeClock)
                && notice.map(|n| n.complete()).unwrap_or(false),
            "镜像时序不一致即拒绝、不落地、不发下游请求、三要素齐",
        );
        // 拒绝后屏幕保持上一份布局（不动现状）。
        let mut c3 = fixture(GenTier::XeLatest, 3);
        let _ = c3.apply(LayoutRequest::extend_default(1920), 60);
        let before = c3.applied();
        let _ = c3.attach(1, "DP-2", SIG_1440P60);
        let _ = c3.apply(LayoutRequest::clone_at(0, 0), 61);
        s.add(
            "B33-约束-拒绝不动现状",
            c3.applied() == before && c3.applied().pipes_used == 2,
            "镜像被拒后上一份扩展布局原样保留（拒绝≠半落地）",
        );
        // 时序值非法 → 拒绝 TimingIllegal（真调 F0225 规则，不自造）。
        let mut c4 = DualScreenCtl::new(GenTier::XeLatest, 3).unwrap_or_else(|_| fixture(GenTier::XeLatest, 3));
        let _ = c4.attach(0, "DP-1", SIG_1080P60);
        let _ = c4.attach(1, "DP-2", TimingSig { link_mhz: 4321, ..SIG_1080P60 });
        let out4 = c4.apply(LayoutRequest::extend_default(1920), 70);
        s.add(
            "B33-约束-非法时序拒绝",
            out4.reason == Some(RejectReason::TimingIllegal)
                && !out4.degraded
                && SIG_1080P60.legal(),
            "link rate 档位表外即拒绝（判定真调 F0225 value_legal，合法基准自证）",
        );
        // 规范序：无输出时先报「未接入」而不是别的。
        let mut c5 = DualScreenCtl::new(GenTier::XeLatest, 3).unwrap_or_else(|_| fixture(GenTier::XeLatest, 3));
        s.add(
            "B33-约束-先报未接入而非他因",
            c5.validate(LayoutMode::Extend) == Err(RejectReason::OutputAbsent)
                && c5.apply(LayoutRequest::extend_default(1920), 80).reason
                    == Some(RejectReason::OutputAbsent),
            "一块屏都没插时先报未接入（下游原因不说错）",
        );
    }

    // --- 判据 3：布局持久化（O(1) 反序列化 + 头闸 + 回落） ---
    {
        // 往返一致：save → 换掉现状 → restore 逐字段相等。
        let mut c = fixture(GenTier::XeLatest, 3);
        let _ = c.apply(LayoutRequest::extend_default(1920), 100);
        let did_save = c.save();
        let saved = *c.persist();
        let _ = c.apply(LayoutRequest::clone_at(0, 0), 101);
        let rec = c.restore(102);
        s.add(
            "B33-持久化-保存恢复往返一致",
            did_save
                && rec.applied_record
                && rec.reason.is_none()
                && *c.persist() == saved
                && c.applied().mode == LayoutMode::Extend
                && c.applied().placements[1] == Some(Placement { pipe: 1, x: 1920, y: 0 })
                && c.stats.restores == 1
                && c.stats.restore_fallbacks == 0,
            "扩展布局存取往返逐字段一致（记录可比对可重算）",
        );
        // 头闸三道：魔数/版本/校验和任一被篡改即拒。
        let mut magic_bad = PersistRecord::blank();
        magic_bad.words[0] = 0x0000;
        let mut ver_bad = PersistRecord::blank();
        ver_bad.words[0] = PERSIST_MAGIC;
        ver_bad.words[1] = 99;
        ver_bad.words[PERSIST_WORDS - 1] = ver_bad.checksum();
        let mut sum_bad = PersistRecord::blank();
        sum_bad.words[0] = PERSIST_MAGIC;
        sum_bad.words[1] = PERSIST_VERSION;
        sum_bad.words[7] = 1234; // 体内改一位，校验和不再匹配
        s.add(
            "B33-持久化-头闸三道篡改即拒",
            magic_bad.header_ok() == Err(RejectReason::LayoutCorrupt)
                && ver_bad.header_ok() == Err(RejectReason::LayoutCorrupt)
                && sum_bad.header_ok() == Err(RejectReason::LayoutCorrupt),
            "魔数/版本/校验和三道头闸各自独立拦下（篡改检测不靠运气）",
        );
        // 有效记录过头闸。
        let mut c2 = fixture(GenTier::XeLatest, 3);
        let _ = c2.apply(LayoutRequest::extend_default(1920), 110);
        let _ = c2.save();
        s.add(
            "B33-持久化-有效记录过头闸",
            c2.persist().header_ok().is_ok()
                && c2.persist().active() == 2
                && c2.persist().mode() == Some(LayoutMode::Extend)
                && c2.persist().entry(1).map(|e| (e.port, e.pipe, e.x, e.hdisplay))
                    == Some((1, 1, 1920, 1920))
                && c2.persist().entry(2).is_none(),
            "定长词数组直读：头闸过、活跃数/模式/条目可读，越界条目 None",
        );
        // 端口已拔出 → 回落默认并告知。
        let mut c3 = fixture(GenTier::XeLatest, 3);
        let _ = c3.apply(LayoutRequest::extend_default(1920), 120);
        let _ = c3.save();
        c3.detach(1);
        let rec3 = c3.restore(121);
        s.add(
            "B33-持久化-端口拔出回落默认",
            !rec3.applied_record
                && rec3.reason == Some(RejectReason::OutputAbsent)
                && rec3.notice.map(|n| n.complete()).unwrap_or(false)
                && c3.stats.restore_fallbacks == 1
                && c3.stats.restores == 0
                && c3.applied().mode == LayoutMode::Extend
                && c3.applied().placements[1].is_none(),
            "副屏已拔 → 不套用旧布局，回落默认并带原因告知",
        );
        // 恢复走同一条 apply 管道：镜像记录在时序已变时被拒而非静默应用。
        let mut c4 = fixture(GenTier::XeLatest, 3);
        let _ = c4.apply(LayoutRequest::clone_at(0, 0), 130);
        let _ = c4.save();
        let _ = c4.attach(1, "DP-2", SIG_4K60);
        let rec4 = c4.restore(131);
        s.add(
            "B33-持久化-恢复走同一约束管道",
            !rec4.applied_record
                && rec4.reason.is_some()
                && c4.stats.rejects >= 1
                && c4.applied().mode == LayoutMode::Extend,
            "镜像记录遇上时序已变的两屏照样被拒（恢复不是后门）",
        );
        // 记录计数对账：成功与回落不重不漏。
        let mut c5 = fixture(GenTier::XeLatest, 3);
        let _ = c5.apply(LayoutRequest::extend_default(1920), 140);
        let _ = c5.save();
        let r_ok = c5.restore(141);
        let mut c6 = fixture(GenTier::XeLatest, 3);
        let r_bad = c6.restore(142);
        s.add(
            "B33-持久化-恢复计数对账",
            r_ok.applied_record
                && c5.stats.restores == 1
                && c5.stats.restore_fallbacks == 0
                && !r_bad.applied_record
                && c6.stats.restore_fallbacks == 1
                && c6.stats.restores == 0,
            "成功走 restores、失败走 fallbacks，两账不串（可对账）",
        );
    }

    // --- 判据 4：降级通知（pipe 不足 + 三要素 + 键盘可达可关闭） ---
    {
        // pipe 不足 → 降级单屏，副屏显式缺席。
        let mut c = fixture(GenTier::Baseline, 1);
        let out = c.apply(LayoutRequest::extend_default(1920), 150);
        s.add(
            "B33-降级-pipe 不足退单屏",
            out.degraded
                && out.reason == Some(RejectReason::PipeExhausted)
                && out.mode == LayoutMode::Extend
                && out.pipes_used == 1
                && out.placements[0].is_some()
                && out.placements[1].is_none()
                && c.applied().degraded()
                && c.stats.degraded_single == 1,
            "扩展开不下即退单屏：副屏 placement 显式 None（不假装两屏都在）",
        );
        // 降级后下游只发一条重编程 + 一条重估（不按两屏发）。
        let reqs = c.take_requests();
        s.add(
            "B33-降级-下游请求不虚发",
            reqs.len() == 2
                && reqs.get(0).map(|r| r.kind) == Some(DownstreamKind::Reprogram)
                && reqs.get(1).map(|r| r.kind) == Some(DownstreamKind::MpoReeval)
                && reqs.get(1).map(|r| r.port) == Some(MPO_PORT)
                && c.pending_requests() == 0,
            "降级单屏只发一条 F0225 重编程 + 一条 F0226 重估（不虚发）",
        );
        // 镜像只需一根 pipe → 同样资源下不降级。
        let mut c2 = fixture(GenTier::Baseline, 1);
        let out2 = c2.apply(LayoutRequest::clone_at(0, 0), 160);
        s.add(
            "B33-降级-镜像资源够不降级",
            out2.is_clean()
                && out2.pipes_used == 1
                && out2.timing_programs == 1
                && out2.placements[1].is_some(),
            "镜像只占一根 pipe，单管线适配器上照样成立（降级的是扩展不是显示）",
        );
        // 每个原因的���知都三要素齐且键盘可达可关闭。
        let mut all_ok = true;
        let mut i = 0usize;
        while i < RejectReason::ALL.len() {
            let n = notice_for(RejectReason::ALL[i]);
            all_ok = all_ok && n.complete();
            i += 1;
        }
        s.add(
            "B33-降级-七原因通知三要素齐",
            all_ok && RejectReason::ALL.len() == 7,
            "七原因逐个出三要素且键盘可达 + 可关闭（锚点无障碍硬要求）",
        );
        // 三要素两两互异（不许共用占位串——"失败"三个字不算三要素）。
        let mut distinct = true;
        let mut j = 0usize;
        while j < RejectReason::ALL.len() {
            let mut k = j + 1;
            while k < RejectReason::ALL.len() {
                let a = notice_for(RejectReason::ALL[j]);
                let b = notice_for(RejectReason::ALL[k]);
                distinct = distinct && a.what != b.what && a.impact != b.impact
                    && a.action != b.action;
                k += 1;
            }
            j += 1;
        }
        s.add(
            "B33-降级-三要素两两互异",
            distinct,
            "七组通知的三要素两两不同（拒绝模板化「都一样」）",
        );
        // 降级原因可判定（is_degraded 把降级与拒绝分开）。
        s.add(
            "B33-降级-降级与拒绝可判定",
            RejectReason::PipeExhausted.is_degraded()
                && RejectReason::RestoreFallback.is_degraded()
                && !RejectReason::CloneMismatch.is_degraded()
                && !RejectReason::TimingIllegal.is_degraded(),
            "降级（照常有屏）与拒绝（不动现状）两类分明（调用方不必猜）",
        );
    }

    // --- 判据 5：读屏与判据自身（位置语义 + 码表 + 下游闭环） ---
    {
        let mut c = fixture(GenTier::XeLatest, 3);
        let _ = c.apply(LayoutRequest::extend_default(1920), 170);
        let name = c.layout_name();
        let lines = c.a11y_lines();
        s.add(
            "B33-读屏-布局命名含位置语义",
            name.contains("DP-1")
                && name.contains("DP-2")
                && name.contains("1920")
                && name.contains('0')
                && name.contains("扩展")
                && lines.len() == 6
                && lines[1].contains("DP-2"),
            "布局名带端口名 + 坐标 + 模式人话（锚点：布局命名含位置语义）",
        );
        let mut c2 = fixture(GenTier::XeLatest, 3);
        let _ = c2.attach(1, "DP-2", SIG_4K60);
        let _ = c2.apply(LayoutRequest::clone_at(0, 0), 171);
        let n2 = c2.a11y_lines();
        s.add(
            "B33-读屏-最近通知三要素进数组",
            c2.last_notice.is_some()
                && n2[5].contains("镜像")
                && n2[5].contains("建议"),
            "最近一次拒绝原因的三要素真进读屏行（不裸抛枚举）",
        );
        // 码表互异 + 段独占 + 未知码兜底。
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
        s.add("B33-判据-码位两两互异", ok, "七码互异");
        s.add(
            "B33-判据-码段独占0x64",
            CODES.iter().all(|c| c & 0xFF00 == 0x6400),
            "全码独占 0x64 段（0x61/62/63/67/6B/6C 已占，0x64 全仓零占用）",
        );
        s.add(
            "B33-判据-未知码兜底不panic",
            !explain(0x64FF).is_empty() && explain(CODE_BAD_REQUEST) != explain(0x64FF),
            "未知码有兜底人话（不崩也不静默）",
        );
        // 原因码与诊断码一一对应（不留孤儿码）。
        let mut mapped = true;
        let mut i = 0usize;
        while i < RejectReason::ALL.len() {
            let r = RejectReason::ALL[i];
            mapped = mapped && CODES.contains(&r.code()) && !r.label().is_empty();
            i += 1;
        }
        s.add(
            "B33-判据-原因码与诊断码一一对应",
            mapped && LayoutMode::ALL.len() == 2 && DownstreamKind::ALL.len() == 2,
            "七原因逐个有码有标签；模式与下游类别封闭集恰两项",
        );
        // 下游取走即清、重复投递不串。
        let mut c3 = fixture(GenTier::XeLatest, 3);
        let _ = c3.apply(LayoutRequest::extend_default(1920), 180);
        let first = c3.take_requests();
        let second = c3.take_requests();
        s.add(
            "B33-判据-下游取走即清",
            first.len() == 3
                && first.get(0).map(|r| r.kind) == Some(DownstreamKind::Reprogram)
                && first.get(1).map(|r| r.kind) == Some(DownstreamKind::Reprogram)
                && first.get(2).map(|r| r.kind) == Some(DownstreamKind::MpoReeval)
                && second.len() == 0
                && c3.stats.dropped_requests == 0,
            "双屏扩展发两条重编程 + 一条重估；取走即清，不重复不串账",
        );
        // 热插拔联动：接入即触发 MPO 重估（缓存必脏）。
        let mut c4 = fixture(GenTier::XeLatest, 3);
        let _ = c4.take_requests();
        let mut hp = super::veb232_hotplug::HpdHotplug::new(1, "DP-2");
        hp.on_event(super::veb232_hotplug::HpdEventKind::Connect, 0);
        let _ = hp.poll(super::veb232_hotplug::DEBOUNCE_TICKS, None);
        let linked = c4.sync_with_hotplug(&hp, SIG_1080P60, 190);
        let hpreqs = c4.take_requests();
        s.add(
            "B33-判据-热插拔联动触发重估",
            linked
                && c4.output(1).map(|o| o.present) == Some(true)
                && hpreqs.len() == 1
                && hpreqs.get(0).map(|r| r.kind) == Some(DownstreamKind::MpoReeval)
                && hpreqs.get(0).map(|r| r.port) == Some(MPO_PORT),
            "接入即发一条 F0226 重估（热插拔后 MPO 缓存必脏）",
        );
        // 编程次数账：三次镜像 + 两次扩展 → 3×1 + 2×2 = 7。
        let mut c5 = fixture(GenTier::XeLatest, 3);
        let _ = c5.apply(LayoutRequest::clone_at(0, 0), 200);
        let _ = c5.apply(LayoutRequest::clone_at(10, 0), 201);
        let _ = c5.apply(LayoutRequest::clone_at(20, 0), 202);
        let _ = c5.apply(LayoutRequest::extend_default(1920), 203);
        let _ = c5.apply(LayoutRequest::extend_default(1920), 204);
        s.add(
            "B33-判据-镜像省下的编程次数可对账",
            c5.stats.clone_applies == 3
                && c5.stats.extend_applies == 2
                && c5.stats.applies == 5
                && c5.stats.timing_programs == 7,
            "镜像三次各 1 次编程、扩展两次各 2 次 = 7（省下的那三次可对账）",
        );
        s.add(
            "B33-判据-条数对账",
            s.len() == 31,
            "判据条数恰 32（本条执行前已有 31 条，防悄悄增删）",
        );
    }

    s
}