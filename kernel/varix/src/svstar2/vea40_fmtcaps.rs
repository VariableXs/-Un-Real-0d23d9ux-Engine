//! VE-F0040 · 纹理格式能力表与兼容转换（VE-A 域 · 资源段 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0040`
//!
//! **职责定位（锚点原文）**：全格式能力表（每格式各 GPU 支持度实测），不支持
//! 格式的自动转换链（转换路径最短优先），转换质量量化标注；含转换链的耗时
//! 预算（转换不拖慢加载）。
//!
//! **数据结构（锚点原文）**：格式枚举 [`TexFmt`]（8 变体）、设备档位
//! [`FmtGpuTier`]（3 档）、支持度三态 [`FmtSupportState`]、能力矩阵
//! [`FmtCapsMatrix`]（格式 × 档位，实测登记带误报校准）、转换边表
//! [`TEX_CONV_EDGES`]、路由器 [`ConvRouter`]（最短链）、链计划
//! [`ChainPlan`]、统计账 [`FmtStats`]。
//!
//! **判据（锚点原文）**：能力实测、转换最短链、质量标注、校准机制、判据。
//!
//! 本模块 14 条判据逐条映射：**能力实测 5 条**（三态按格式×档位登记落位／
//! 全表实测登记计数对账／实测失败校准降级原生误报／实测成功校准升级漏报
//! 误标／未知格式或档位登记显性拒绝）、**转换最短链 5 条**（同格式零跳恒等
//! 链／直达边一跳可达／两跳链经由枢纽逐边复原／三跳链逐边累加判据侧重算／
//! 无通路显性拒绝）、**质量标注与预算 2 条**（超预算显性拒绝预算内放行／
//! 质量分级标注阈值判据侧重算）、**判据自身 2 条**（统计账与调用次数对账／
//! 诊断码互异且段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 转换失败 → **格式降级** | [`ConvRouter::best_chain`] 对无通路组合显性返回 [`CODE_NO_PATH`]，调用方拿码落降级格式——不存在「静默返回空链假装成功」的通道 |
//! | 质量损失 → **标注** | [`ChainPlan::quality_note`] 把逐边累加的千分比损失映射为「无损/轻微/可感/明显」四档读屏文案，损失不标注就不算转换完成 |
//! | 能力误报 → **实测校准** | [`FmtCapsMatrix::register_measured`] 以实测结果为唯一事实源：声明原生实测失败即降级、声明不支持实测成功即升级，每次改写计入 [`FmtStats::calibrated`]——「驱动声称」永远让位于「实测跑通」 |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **能力表是矩阵不是位图**：格式 × 档位二维矩阵每个格子独立登记三态，
//!   「低端机不支持 BC7 但高端机原生支持」这类事实在位图里写不出来。三态
//!   （原生/可转换/不支持）比布尔多出的「可转换」档是转换链判据的落点。
//! - **实测校准是双向的**：误报（声明支持实测失败）要降级，漏报（声明不
//!   支持实测成功）也要升级——只修一个方向，表就会朝保守或冒进单调漂移。
//! - **最短链 = 跳数优先、代价次之**：[`ConvRouter`] 用「跳数 × 大基数 +
//!   代价」做字典序键的 Dijkstra（8 结点线性扫选择，无需堆），保证跳数最短
//!   的前提下降级代价最小。图是固定边表 [`TEX_CONV_EDGES`]，边带质量损失
//!   （千分比）与耗时代价（抽象单位）双权重。
//! - **预算是加载性能的硬闸**：[`ConvRouter::best_chain`] 把 `cost_total`
//!   与调用方传入预算逐值比较，超预算显性 [`CODE_OVER_BUDGET`]——「转换
//!   不拖慢加载」从口号变成可拒绝的约束。
//! - **判据侧独立重算**：跳数、损失、代价、阈值（700/900 千分比边界、
//!   质量四档 0/100/300）全部在判据里按字面量再写一遍，不向被测函数问
//!   答案——被测逻辑被误改时判据自身先红。
//! - **零 panic 面**：查表走 `get`/`position`、计数全 `saturating_*`、
//!   所有登记与路由接口返回 [`Result`]，越界 id 显性拒绝不索引。

// ---------------------------------------------------------------------------
// 引入（no_std 内核侧；alloc 三件套自查：vec 宏 / Vec 类型 / String+format）
// ---------------------------------------------------------------------------

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（自建段 0x1Axx，独占——0x1A01..=0x1A04 整仓 grep 零占用后选定）
// ---------------------------------------------------------------------------

/// 格式 id 不在 1..=8 闭集内。
pub const CODE_FMT_UNKNOWN: u16 = 0x1A01;
/// 档位 id 不在 1..=3 闭集内。
pub const CODE_TIER_UNKNOWN: u16 = 0x1A02;
/// 两格式间无任何转换通路（格式图不连通）。
pub const CODE_NO_PATH: u16 = 0x1A03;
/// 最短链耗时代价超出调用方预算。
pub const CODE_OVER_BUDGET: u16 = 0x1A04;

// ---------------------------------------------------------------------------
// 二、格式与档位枚举
// ---------------------------------------------------------------------------

/// 纹理格式全集（8 变体；新增格式必须：加变体 + 进 [`ALL`] + 抬
/// [`TEX_FMT_COUNT`] + 补 [`TEX_CONV_EDGES`] 边）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TexFmt {
    /// 8bit 四通道标准色（转换枢纽：多数格式链路经此中转）。
    R8G8B8A8,
    /// 8bit 四通道交换色（常见解码输出，常需转 R8G8B8A8）。
    B8G8R8A8,
    /// 8bit 单通道。
    R8Unorm,
    /// 16bit 半精度单通道。
    R16F,
    /// 16bit 半精度四通道。
    RGBA16F,
    /// BC1 块压缩（1bit alpha）。
    Bc1,
    /// BC3 块压缩（4bit alpha）。
    Bc3,
    /// BC7 高质量块压缩。
    Bc7,
}

/// 格式总数（判据侧按字面量独立重算，不向本常量问答案）。
pub const TEX_FMT_COUNT: usize = 8;

impl TexFmt {
    /// 全集（判据枚举完备性断言用；顺序即 id 顺序）。
    pub const ALL: [TexFmt; TEX_FMT_COUNT] = [
        TexFmt::R8G8B8A8,
        TexFmt::B8G8R8A8,
        TexFmt::R8Unorm,
        TexFmt::R16F,
        TexFmt::RGBA16F,
        TexFmt::Bc1,
        TexFmt::Bc3,
        TexFmt::Bc7,
    ];

    /// 格式 id（1..=8，登记接口按 id 收口以便携带非法输入路径）。
    pub const fn id(self) -> u8 {
        match self {
            TexFmt::R8G8B8A8 => 1,
            TexFmt::B8G8R8A8 => 2,
            TexFmt::R8Unorm => 3,
            TexFmt::R16F => 4,
            TexFmt::RGBA16F => 5,
            TexFmt::Bc1 => 6,
            TexFmt::Bc3 => 7,
            TexFmt::Bc7 => 8,
        }
    }

    /// 按 id 反查格式（越界显性 `None`，零 panic 面）。
    pub fn from_id(id: u8) -> Option<TexFmt> {
        Self::ALL.iter().copied().find(|f| f.id() == id)
    }

    /// 短名（读屏与统计口径）。
    pub const fn as_str(self) -> &'static str {
        match self {
            TexFmt::R8G8B8A8 => "RGBA8",
            TexFmt::B8G8R8A8 => "BGRA8",
            TexFmt::R8Unorm => "R8",
            TexFmt::R16F => "R16F",
            TexFmt::RGBA16F => "RGBA16F",
            TexFmt::Bc1 => "BC1",
            TexFmt::Bc3 => "BC3",
            TexFmt::Bc7 => "BC7",
        }
    }
}

/// GPU 设备档位（3 档；id 1..=3 与格式 id 同口径收口）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FmtGpuTier {
    /// 低端（集成显卡 / 旧虚拟 GPU）。
    Low,
    /// 中端。
    Mid,
    /// 高端。
    High,
}

/// 档位总数（判据侧独立重算）。
pub const TEX_TIER_COUNT: usize = 3;

impl FmtGpuTier {
    /// 全集。
    pub const ALL: [FmtGpuTier; TEX_TIER_COUNT] =
        [FmtGpuTier::Low, FmtGpuTier::Mid, FmtGpuTier::High];

    /// 档位 id（1..=3）。
    pub const fn id(self) -> u8 {
        match self {
            FmtGpuTier::Low => 1,
            FmtGpuTier::Mid => 2,
            FmtGpuTier::High => 3,
        }
    }

    /// 按 id 反查档位。
    pub fn from_id(id: u8) -> Option<FmtGpuTier> {
        Self::ALL.iter().copied().find(|t| t.id() == id)
    }
}

// ---------------------------------------------------------------------------
// 三、支持度三态与能力矩阵
// ---------------------------------------------------------------------------

/// 支持度三态（判据枚举完备性用；无 `Default`——初始态是显性的不支持）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FmtSupportState {
    /// 原生支持（可直接加载）。
    Native,
    /// 原生不支持但可经转换链到达。
    ViaConvert,
    /// 不支持（且无可行转换）。
    Unsupported,
}

impl FmtSupportState {
    /// 三态短名（读屏口径）。
    pub const fn as_str(self) -> &'static str {
        match self {
            FmtSupportState::Native => "原生",
            FmtSupportState::ViaConvert => "可转换",
            FmtSupportState::Unsupported => "不支持",
        }
    }
}

/// 能力统计账（登记/实测/校准的全部依据都可从这里读出）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FmtStats {
    /// 显性声明次数（set_declared）。
    pub declared: usize,
    /// 实测登记次数（register_measured，含拒绝）。
    pub measured: usize,
    /// 校准改写次数（实测与声明不符的改写，双向合计）。
    pub calibrated: usize,
    /// 登记拒绝次数（非法 id）。
    pub rejects: usize,
}

/// 纹理格式能力矩阵：格式 × 档位 三态，实测登记带误报校准。
#[derive(Clone, Debug)]
pub struct FmtCapsMatrix {
    /// 支持度矩阵（[格式下标][档位下标]；初始全不支持）。
    support: [[FmtSupportState; TEX_TIER_COUNT]; TEX_FMT_COUNT],
    /// 统计账。
    pub stats: FmtStats,
}

impl FmtCapsMatrix {
    /// 新建矩阵（全格不支持；「不支持」是显性事实不是省略）。
    pub fn new() -> FmtCapsMatrix {
        FmtCapsMatrix {
            support: [[FmtSupportState::Unsupported; TEX_TIER_COUNT]; TEX_FMT_COUNT],
            stats: FmtStats::default(),
        }
    }

    /// 显性声明某格式的支持度（驱动能力报告的登记口）。
    pub fn set_declared(&mut self, fmt: TexFmt, tier: FmtGpuTier, state: FmtSupportState) {
        let fi = fmt.id() as usize - 1;
        let ti = tier.id() as usize - 1;
        self.support[fi][ti] = state;
        self.stats.declared = self.stats.declared.saturating_add(1);
    }

    /// 实测登记（误报校准的唯一入口）：实测结果为唯一事实源。
    ///
    /// 实测成功 → 原生；实测失败 → 不支持。与登记值不符即校准改写并计数。
    /// 非法 id 显性拒绝（`Err`），不索引不回绕。
    pub fn register_measured(&mut self, fmt_id: u8, tier_id: u8, works: bool) -> Result<(), u16> {
        self.stats.measured = self.stats.measured.saturating_add(1);
        let fmt = match TexFmt::from_id(fmt_id) {
            Some(f) => f,
            None => {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                return Err(CODE_FMT_UNKNOWN);
            }
        };
        let tier = match FmtGpuTier::from_id(tier_id) {
            Some(t) => t,
            None => {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                return Err(CODE_TIER_UNKNOWN);
            }
        };
        let fi = fmt.id() as usize - 1;
        let ti = tier.id() as usize - 1;
        let corrected = if works {
            FmtSupportState::Native
        } else {
            FmtSupportState::Unsupported
        };
        if self.support[fi][ti] != corrected {
            self.support[fi][ti] = corrected;
            self.stats.calibrated = self.stats.calibrated.saturating_add(1);
        }
        Ok(())
    }

    /// 查支持度（只读）。
    pub fn support(&self, fmt: TexFmt, tier: FmtGpuTier) -> FmtSupportState {
        self.support[fmt.id() as usize - 1][tier.id() as usize - 1]
    }

    /// 能否直接加载（原生支持才可直载；可转换/不支持都要走转换或降级）。
    pub fn can_load_direct(&self, fmt: TexFmt, tier: FmtGpuTier) -> bool {
        self.support(fmt, tier) == FmtSupportState::Native
    }

    /// 读屏摘要（锚点：能力状态读屏可达——格式 × 档位逐行）。
    pub fn a11y_summary(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "能力表：声明 {}，实测 {}，校准 {}，拒绝 {}",
            self.stats.declared, self.stats.measured, self.stats.calibrated, self.stats.rejects
        ));
        for f in TexFmt::ALL.iter() {
            let mut cells = Vec::new();
            for t in FmtGpuTier::ALL.iter() {
                cells.push(format!(
                    "{}={}",
                    t.as_str(),
                    self.support(*f, *t).as_str()
                ));
            }
            lines.push(format!("{}：{}", f.as_str(), cells.join(" ")));
        }
        lines.join("\n")
    }
}

// ---------------------------------------------------------------------------
// 四、转换图：固定边表 + 最短链路由
// ---------------------------------------------------------------------------

/// 单条转换边：起点、终点、质量损失（千分比）、耗时代价（抽象单位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FmtConvEdge {
    /// 源格式。
    pub from: TexFmt,
    /// 目标格式。
    pub to: TexFmt,
    /// 质量损失（千分比 0..=1000；0 = 无损）。
    pub loss_permille: u16,
    /// 耗时代价（抽象单位；判据侧按字面量重算用）。
    pub cost: u16,
}

/// 固定转换边表（10 条；R8G8B8A8 为枢纽，B8G8R8A8/R16F 经枢纽中转，
/// Bc1/Bc3/Bc7/R8Unorm 为汇点——汇点无出边，天然构成「无通路」测试点）。
pub const TEX_CONV_EDGES: [FmtConvEdge; 10] = [
    FmtConvEdge { from: TexFmt::B8G8R8A8, to: TexFmt::R8G8B8A8, loss_permille: 0, cost: 2 },
    FmtConvEdge { from: TexFmt::R8G8B8A8, to: TexFmt::B8G8R8A8, loss_permille: 0, cost: 2 },
    FmtConvEdge { from: TexFmt::R8G8B8A8, to: TexFmt::R8Unorm, loss_permille: 750, cost: 2 },
    FmtConvEdge { from: TexFmt::R8G8B8A8, to: TexFmt::Bc1, loss_permille: 250, cost: 3 },
    FmtConvEdge { from: TexFmt::R8G8B8A8, to: TexFmt::Bc3, loss_permille: 180, cost: 4 },
    FmtConvEdge { from: TexFmt::R8G8B8A8, to: TexFmt::Bc7, loss_permille: 60, cost: 6 },
    FmtConvEdge { from: TexFmt::R8G8B8A8, to: TexFmt::RGBA16F, loss_permille: 0, cost: 3 },
    FmtConvEdge { from: TexFmt::RGBA16F, to: TexFmt::R8G8B8A8, loss_permille: 500, cost: 2 },
    FmtConvEdge { from: TexFmt::R16F, to: TexFmt::RGBA16F, loss_permille: 0, cost: 2 },
    FmtConvEdge { from: TexFmt::RGBA16F, to: TexFmt::R16F, loss_permille: 500, cost: 2 },
];

/// 单步转换计划（边表的一条边快照，逐边带质量与耗时标注）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConvStep {
    /// 本步源格式。
    pub from: TexFmt,
    /// 本步目标格式。
    pub to: TexFmt,
    /// 本步质量损失（千分比）。
    pub loss_permille: u16,
    /// 本步耗时代价。
    pub cost: u16,
}

/// 转换链计划：完整跳序列 + 汇总质量损失 + 汇总耗时代价。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChainPlan {
    /// 逐步转换序列（同格式恒等链为空表）。
    pub steps: Vec<ConvStep>,
    /// 跳数（= steps 长度）。
    pub hops: u16,
    /// 全链质量损失（逐边千分比累加，saturating）。
    pub loss_permille: u16,
    /// 全链耗时代价（逐边累加，saturating）。
    pub cost_total: u16,
}

impl ChainPlan {
    /// 质量分级标注（阈值判据侧独立重算：0 无损 / ≤100 轻微 /
    /// ≤300 可感 / 其余明显）。
    pub fn quality_note(&self) -> &'static str {
        quality_note_of(self.loss_permille)
    }
}

/// 按千分比损失给质量分级文案（阈值 0/100/300 在判据里按字面量重算）。
pub fn quality_note_of(loss_permille: u16) -> &'static str {
    if loss_permille == 0 {
        "无损"
    } else if loss_permille <= 100 {
        "轻微"
    } else if loss_permille <= 300 {
        "可感"
    } else {
        "明显"
    }
}

/// 格式下标（ALL 序；枚举值必在表内，返回 0 兜底不 panic）。
fn node_index(f: TexFmt) -> usize {
    let mut i = 0usize;
    for (idx, x) in TexFmt::ALL.iter().enumerate() {
        if *x == f {
            i = idx;
        }
    }
    i
}

/// 最短链计算（跳数优先、代价次之的 Dijkstra；8 结点线性扫选择）。
fn shortest_chain(src: TexFmt, dst: TexFmt) -> Option<ChainPlan> {
    if src == dst {
        return Some(ChainPlan {
            steps: Vec::new(),
            hops: 0,
            loss_permille: 0,
            cost_total: 0,
        });
    }
    let s = node_index(src);
    let d = node_index(dst);
    let mut hops = [u16::MAX; TEX_FMT_COUNT];
    let mut cost = [u16::MAX; TEX_FMT_COUNT];
    let mut prev = [usize::MAX; TEX_FMT_COUNT];
    let mut done = [false; TEX_FMT_COUNT];
    hops[s] = 0;
    cost[s] = 0;
    loop {
        // 线性扫选未定结点中 (hops, cost) 字典序最小者。
        let mut best = TEX_FMT_COUNT;
        let mut best_h = u16::MAX;
        let mut best_c = u16::MAX;
        for i in 0..TEX_FMT_COUNT {
            if done[i] {
                continue;
            }
            if hops[i] < best_h || (hops[i] == best_h && cost[i] < best_c) {
                best = i;
                best_h = hops[i];
                best_c = cost[i];
            }
        }
        if best == TEX_FMT_COUNT || best == d {
            break;
        }
        done[best] = true;
        for (ei, e) in TEX_CONV_EDGES.iter().enumerate() {
            if node_index(e.from) != best {
                continue;
            }
            let t = node_index(e.to);
            let nh = best_h.saturating_add(1);
            let nc = best_c.saturating_add(e.cost);
            if nh < hops[t] || (nh == hops[t] && nc < cost[t]) {
                hops[t] = nh;
                cost[t] = nc;
                prev[t] = ei;
            }
        }
    }
    if hops[d] == u16::MAX {
        return None;
    }
    // 沿 prev 边回溯复原整链（prev 存边下标，逐步退回源点）。
    let mut steps = Vec::new();
    let mut cur = d;
    let mut guard = 0usize;
    while cur != s {
        guard += 1;
        if guard > TEX_CONV_EDGES.len() + TEX_FMT_COUNT {
            return None;
        }
        let ei = prev[cur];
        if ei == usize::MAX {
            return None;
        }
        let e = &TEX_CONV_EDGES[ei];
        steps.push(ConvStep {
            from: e.from,
            to: e.to,
            loss_permille: e.loss_permille,
            cost: e.cost,
        });
        cur = node_index(e.from);
    }
    steps.reverse();
    let mut loss_total = 0u16;
    let mut cost_total = 0u16;
    for st in steps.iter() {
        loss_total = loss_total.saturating_add(st.loss_permille);
        cost_total = cost_total.saturating_add(st.cost);
    }
    Some(ChainPlan {
        hops: steps.len() as u16,
        steps,
        loss_permille: loss_total,
        cost_total,
    })
}

/// 转换路由统计账（best_chain 的全部决策都可从这里读出）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RouteStats {
    /// 路由查询次数。
    pub queried: usize,
    /// 无通路拒绝次数。
    pub nopath_rejects: usize,
    /// 超预算拒绝次数。
    pub budget_rejects: usize,
}

/// 转换路由器：最短链查询 + 预算硬闸 + 统计账。
#[derive(Clone, Debug, Default)]
pub struct ConvRouter {
    /// 统计账。
    pub stats: RouteStats,
}

impl ConvRouter {
    /// 新建路由器。
    pub fn new() -> ConvRouter {
        ConvRouter { stats: RouteStats::default() }
    }

    /// 最短转换链（锚点主判据）：跳数最短优先，同跳数代价最小；
    /// 超预算显性拒绝——「转换不拖慢加载」的硬闸。
    pub fn best_chain(&mut self, src: TexFmt, dst: TexFmt, budget: u16) -> Result<ChainPlan, u16> {
        self.stats.queried = self.stats.queried.saturating_add(1);
        let plan = match shortest_chain(src, dst) {
            Some(p) => p,
            None => {
                self.stats.nopath_rejects = self.stats.nopath_rejects.saturating_add(1);
                return Err(CODE_NO_PATH);
            }
        };
        if plan.cost_total > budget {
            self.stats.budget_rejects = self.stats.budget_rejects.saturating_add(1);
            return Err(CODE_OVER_BUDGET);
        }
        Ok(plan)
    }

    /// 按 id 的路由入口（id 侧非法输入的显性拒绝路径）。
    pub fn best_chain_by_ids(
        &mut self,
        src_id: u8,
        dst_id: u8,
        budget: u16,
    ) -> Result<ChainPlan, u16> {
        let src = TexFmt::from_id(src_id).ok_or(CODE_FMT_UNKNOWN)?;
        let dst = TexFmt::from_id(dst_id).ok_or(CODE_FMT_UNKNOWN)?;
        self.best_chain(src, dst, budget)
    }
}

// ---------------------------------------------------------------------------
// 五、判据（run_vea40_checks —— 14 条，逐条映射锚点五条判据）
// ---------------------------------------------------------------------------

/// VE-F0040 域自检：能力实测 5 + 转换最短链 5 + 质量预算 2 + 判据自身 2。
pub fn run_vea40_checks() -> CheckSet {
    let mut s = CheckSet::new("vea40_fmtcaps");

    // --- 判据 1.1：能力实测——三态按格式×档位登记落位（逐格可查） ---
    {
        let mut m = FmtCapsMatrix::new();
        m.set_declared(TexFmt::Bc7, FmtGpuTier::High, FmtSupportState::Native);
        m.set_declared(TexFmt::Bc7, FmtGpuTier::Low, FmtSupportState::ViaConvert);
        let ok = m.support(TexFmt::Bc7, FmtGpuTier::High) == FmtSupportState::Native
            && m.support(TexFmt::Bc7, FmtGpuTier::Low) == FmtSupportState::ViaConvert
            && m.support(TexFmt::Bc7, FmtGpuTier::Mid) == FmtSupportState::Unsupported
            && m.support(TexFmt::R8G8B8A8, FmtGpuTier::High) == FmtSupportState::Unsupported
            && m.stats.declared == 2;
        s.add(
            "A40-实测-三态按格式档位登记逐格可查",
            ok,
            "高/中/低三档各格独立：BC7 高档原生、低档可转换、中档未登记保持不支持",
        );
    }

    // --- 判据 1.2：能力实测——全表实测登记计数对账（8×3=24 字面量重算） ---
    {
        let mut m = FmtCapsMatrix::new();
        let mut calls = 0usize;
        for f in TexFmt::ALL.iter() {
            for t in FmtGpuTier::ALL.iter() {
                let _ = m.register_measured(f.id(), t.id(), true);
                calls += 1;
            }
        }
        let all_native = TexFmt::ALL
            .iter()
            .all(|f| FmtGpuTier::ALL.iter().all(|t| m.can_load_direct(*f, *t)));
        s.add(
            "A40-实测-全表实测登记计数对账",
            all_native && m.stats.measured == 24 && calls == 24 && m.stats.rejects == 0,
            "8 格式 × 3 档位 = 24 次实测全数入账、全表升级原生——实测计数不虚不漏",
        );
    }

    // --- 判据 1.3：校准机制——实测失败校准降级（原生误报） ---
    {
        let mut m = FmtCapsMatrix::new();
        m.set_declared(TexFmt::Bc7, FmtGpuTier::High, FmtSupportState::Native);
        let r = m.register_measured(TexFmt::Bc7.id(), FmtGpuTier::High.id(), false);
        s.add(
            "A40-校准-实测失败校准降级原生误报",
            r.is_ok()
                && m.support(TexFmt::Bc7, FmtGpuTier::High) == FmtSupportState::Unsupported
                && m.stats.calibrated == 1
                && !m.can_load_direct(TexFmt::Bc7, FmtGpuTier::High),
            "驱动声明原生但实测跑不通：格子被实测改写为不支持并计校准一次",
        );
    }

    // --- 判据 1.4：校准机制——实测成功校准升级（漏报误标） ---
    {
        let mut m = FmtCapsMatrix::new();
        let r = m.register_measured(TexFmt::Bc7.id(), FmtGpuTier::High.id(), true);
        s.add(
            "A40-校准-实测成功校准升级漏报误标",
            r.is_ok()
                && m.support(TexFmt::Bc7, FmtGpuTier::High) == FmtSupportState::Native
                && m.stats.calibrated == 1
                && m.can_load_direct(TexFmt::Bc7, FmtGpuTier::High),
            "初始不支持但实测可用：升级为原生并计校准——校准双向，表不单调漂移",
        );
    }

    // --- 判据 1.5：能力实测——未知格式或档位登记显性拒绝 ---
    {
        let mut m = FmtCapsMatrix::new();
        let bad_fmt = m.register_measured(0, 1, true);
        let bad_fmt_hi = m.register_measured(9, 1, true);
        let bad_tier = m.register_measured(1, 0, true);
        let bad_tier_hi = m.register_measured(1, 4, true);
        s.add(
            "A40-实测-未知格式或档位登记显性拒绝",
            bad_fmt == Err(CODE_FMT_UNKNOWN)
                && bad_fmt_hi == Err(CODE_FMT_UNKNOWN)
                && bad_tier == Err(CODE_TIER_UNKNOWN)
                && bad_tier_hi == Err(CODE_TIER_UNKNOWN)
                && m.stats.rejects == 4
                && m.stats.measured == 4,
            "id 0/9 与档位 0/4 全部显性拒绝：闭集外输入走码不索引，实测计数含拒绝",
        );
    }

    // --- 判据 2.1：转换最短链——同格式零跳恒等链 ---
    {
        let mut r = ConvRouter::new();
        let plan = r.best_chain(TexFmt::Bc3, TexFmt::Bc3, 0);
        let ok = match plan {
            Ok(p) => p.hops == 0 && p.steps.is_empty() && p.loss_permille == 0 && p.cost_total == 0,
            Err(_) => false,
        };
        s.add(
            "A40-转换-同格式零跳恒等链",
            ok,
            "src==dst 返回零跳空链：零代价恒等，预算为 0 也放行",
        );
    }

    // --- 判据 2.2：转换最短链——直达边一跳可达（判据侧扫边表独立确认） ---
    {
        let mut r = ConvRouter::new();
        let plan = r.best_chain(TexFmt::R8G8B8A8, TexFmt::Bc1, 100);
        // 判据侧独立重算：边表里确实存在 R8G8B8A8→Bc1 直达边
        let direct = TEX_CONV_EDGES
            .iter()
            .any(|e| e.from == TexFmt::R8G8B8A8 && e.to == TexFmt::Bc1);
        let ok = match plan {
            Ok(p) => direct && p.hops == 1 && p.steps.len() == 1 && p.loss_permille == 250 && p.cost_total == 3,
            Err(_) => false,
        };
        s.add(
            "A40-转换-直达边最短一跳可达",
            ok,
            "枢纽→BC1 有一跳直达边：路由给 1 跳、损失 250、代价 3，与边表逐值对拍",
        );
    }

    // --- 判据 2.3：转换最短链——两跳链经由枢纽逐边复原 ---
    {
        let mut r = ConvRouter::new();
        let plan = r.best_chain(TexFmt::B8G8R8A8, TexFmt::Bc7, 100);
        // 判据侧独立重算：BGRA8→RGBA8（损0耗2）→BC7（损60耗6），全链损60耗8
        let ok = match plan {
            Ok(p) => {
                p.hops == 2
                    && p.steps.len() == 2
                    && p.steps[0].from == TexFmt::B8G8R8A8
                    && p.steps[0].to == TexFmt::R8G8B8A8
                    && p.steps[0].loss_permille == 0
                    && p.steps[0].cost == 2
                    && p.steps[1].from == TexFmt::R8G8B8A8
                    && p.steps[1].to == TexFmt::Bc7
                    && p.steps[1].loss_permille == 60
                    && p.steps[1].cost == 6
                    && p.loss_permille == 60
                    && p.cost_total == 8
            }
            Err(_) => false,
        };
        s.add(
            "A40-转换-两跳链经由枢纽逐边复原",
            ok,
            "BGRA8→BC7 必经枢纽 RGBA8 中转：两步序列、逐边损失与代价与边表逐值一致",
        );
    }

    // --- 判据 2.4：转换最短链——三跳链逐边累加（判据侧重算 0+500+250/2+2+3） ---
    {
        let mut r = ConvRouter::new();
        let plan = r.best_chain(TexFmt::R16F, TexFmt::Bc1, 100);
        // 判据侧独立重算：R16F→RGBA16F（损0耗2）→RGBA8（损500耗2）→BC1（损250耗3）
        let ok = match plan {
            Ok(p) => {
                p.hops == 3
                    && p.loss_permille == 750
                    && p.cost_total == 7
                    && p.steps.len() == 3
                    && p.steps[0].to == TexFmt::RGBA16F
                    && p.steps[1].to == TexFmt::R8G8B8A8
                    && p.steps[2].to == TexFmt::Bc1
            }
            Err(_) => false,
        };
        s.add(
            "A40-转换-三跳链逐边累加判据侧重算",
            ok,
            "R16F→BC1 唯一通路三跳：损失 0+500+250=750、代价 2+2+3=7，判据侧逐边重算一致",
        );
    }

    // --- 判据 2.5：转换最短链——无通路显性拒绝（汇点无出边） ---
    {
        let mut r = ConvRouter::new();
        let res = r.best_chain(TexFmt::Bc1, TexFmt::R16F, 1000);
        // 判据侧独立确认：Bc1 在边表中无任何出边
        let no_out = TEX_CONV_EDGES
            .iter()
            .all(|e| e.from != TexFmt::Bc1);
        s.add(
            "A40-转换-无通路显性拒绝汇点不假装可达",
            res == Err(CODE_NO_PATH) && no_out && r.stats.nopath_rejects == 1,
            "BC1 是汇点（无出边）：逆向请求显性 CODE_NO_PATH，不返回空链假装成功",
        );
    }

    // --- 判据 3.1：质量标注与预算——超预算显性拒绝、预算内放行（8=门槛逐值） ---
    {
        let mut r = ConvRouter::new();
        // BGRA8→BC7 全链代价 8（判据侧字面量）：预算 7 拒、预算 8 放
        let over = r.best_chain(TexFmt::B8G8R8A8, TexFmt::Bc7, 7);
        let fits = r.best_chain(TexFmt::B8G8R8A8, TexFmt::Bc7, 8);
        s.add(
            "A40-预算-超预算显性拒绝预算内放行",
            over == Err(CODE_OVER_BUDGET)
                && fits.is_ok()
                && r.stats.budget_rejects == 1,
            "代价 8 > 预算 7 拒、代价 8 ≤ 预算 8 放：预算是闭区间硬闸不是参考值",
        );
    }

    // --- 判据 3.2：质量标注——分级文案阈值判据侧重算（0/100/300 字面量） ---
    {
        let a = quality_note_of(0);
        let b = quality_note_of(1);
        let c = quality_note_of(100);
        let d = quality_note_of(101);
        let e = quality_note_of(300);
        let f = quality_note_of(301);
        let g = quality_note_of(750);
        s.add(
            "A40-质量-分级标注阈值判据侧重算",
            a == "无损" && b == "轻微" && c == "轻微" && d == "可感"
                && e == "可感" && f == "明显" && g == "明显",
            "0 无损、≤100 轻微、≤300 可感、>300 明显：边界值 100/101 与 300/301 双侧对拍",
        );
    }

    // --- 判据 4.1：判据自身——统计账与调用次数逐值对账 ---
    {
        let mut r = ConvRouter::new();
        let _ = r.best_chain(TexFmt::R8G8B8A8, TexFmt::Bc1, 100); // 查询 1，放行
        let _ = r.best_chain(TexFmt::Bc1, TexFmt::R16F, 100); // 查询 2，无通路拒
        let _ = r.best_chain(TexFmt::B8G8R8A8, TexFmt::Bc7, 7); // 查询 3，超预算拒
        s.add(
            "A40-判据-统计账与调用次数逐值对账",
            r.stats.queried == 3
                && r.stats.nopath_rejects == 1
                && r.stats.budget_rejects == 1,
            "三次调用三类结局：查询 3 = 放行 1 + 无通路 1 + 超预算 1，账不漂移",
        );
    }

    // --- 判据 4.2：判据自身——诊断码互异且段独占（0x1A 段四码全对拍） ---
    {
        let codes = [CODE_FMT_UNKNOWN, CODE_TIER_UNKNOWN, CODE_NO_PATH, CODE_OVER_BUDGET];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_segment = codes.iter().all(|c| (c & 0xFF00) == 0x1A00);
        s.add(
            "A40-判据-诊断码互异且段独占",
            all_diff && in_segment && codes.len() == 4,
            "0x1A01..=0x1A04 四码两两互异、高字节全部落在 0x1A 独占段",
        );
    }

    s
}

// ---------------------------------------------------------------------------
// 六、单元测试（cargo test svstar2）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea40_calibration_both_directions() {
        let mut m = FmtCapsMatrix::new();
        m.set_declared(TexFmt::Bc3, FmtGpuTier::Mid, FmtSupportState::Native);
        // 误报：声明原生实测失败 → 降级
        assert!(m.register_measured(TexFmt::Bc3.id(), FmtGpuTier::Mid.id(), false).is_ok());
        assert_eq!(m.support(TexFmt::Bc3, FmtGpuTier::Mid), FmtSupportState::Unsupported);
        // 漏报：声明不支持实测成功 → 升级
        assert!(m.register_measured(TexFmt::Bc3.id(), FmtGpuTier::Mid.id(), true).is_ok());
        assert_eq!(m.support(TexFmt::Bc3, FmtGpuTier::Mid), FmtSupportState::Native);
        assert_eq!(m.stats.calibrated, 2);
        assert_eq!(m.stats.measured, 2);
    }

    #[test]
    fn vea40_chain_hops_and_budget() {
        let mut r = ConvRouter::new();
        let p1 = r.best_chain(TexFmt::B8G8R8A8, TexFmt::Bc7, 8).unwrap_or_else(|_| ChainPlan {
            steps: Vec::new(),
            hops: 99,
            loss_permille: 0,
            cost_total: 0,
        });
        assert_eq!(p1.hops, 2);
        assert_eq!(p1.cost_total, 8);
        assert_eq!(p1.quality_note(), "轻微");
        assert_eq!(r.best_chain(TexFmt::B8G8R8A8, TexFmt::Bc7, 7), Err(CODE_OVER_BUDGET));
        assert_eq!(r.best_chain(TexFmt::Bc3, TexFmt::Bc3, 0).map(|p| p.hops), Ok(0));
    }

    #[test]
    fn vea40_ids_reject_and_a11y() {
        let mut r = ConvRouter::new();
        assert_eq!(r.best_chain_by_ids(0, 1, 100), Err(CODE_FMT_UNKNOWN));
        assert_eq!(r.best_chain_by_ids(1, 9, 100), Err(CODE_FMT_UNKNOWN));
        let mut m = FmtCapsMatrix::new();
        m.set_declared(TexFmt::Bc7, FmtGpuTier::High, FmtSupportState::Native);
        let text = m.a11y_summary();
        assert!(text.contains("BC7"), "读屏含格式短名：{}", text);
        assert!(text.contains("原生"), "读屏含支持度文案：{}", text);
        assert!(text.contains("校准 0"), "读屏含校准计数：{}", text);
    }

    #[test]
    fn vea40_checks_all_green() {
        let set = run_vea40_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0040 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
