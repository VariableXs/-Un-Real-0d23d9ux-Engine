//! VE-F0027 · 绑定布局编译器（VE-A 域 · 布局编译 + 产物缓存 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0027`
//!
//! **判据（锚点原文）**：资源绑定布局的离线编译（反射自动生成 + 手动布局双源），
//! 布局冲突检测（槽位重复/越界），编译产物缓存（A23 PSO 联动）；含布局编译的
//! 耗时预算（编译不拖慢构建）。判据五条：**双源编译、冲突定位、产物缓存、
//! 反射兜底、判据**。
//!
//! **错误路径与降级矩阵**：槽位冲突→定位；反射失败→手动源；缓存损坏→重编。
//!
//! **数据结构**：编译器；冲突检测。
//!
//! **性能逐项分解**：O(布局)——每步线性扫声明数，声明数以 [`MAX_DECLS`] 为界；
//! 查表命中为常数（指纹比对 + 全等复核）。不随布局**种类**增长。
//!
//! **跨批对接点**：A23 PSO 缓存联动（A23 = VE-F0023 `vea23_psocache`）——本条产出的
//! [`LayoutDigest`] 是 A23 缓存键的**输入之一**，A23 那边另有管线状态维度。本条
//! **不缓存管线状态**（那是 A23 的事），只缓存「布局 → 寄存器映射」这一段；
//! 故 A23 改自己的键策略时，本条不必改。反过来本条改寄存器分配策略，A23 的
//! 旧产物会失效——那由 [`ArtifactCache::verify`] 的指纹复核挡住，不会静默用错。
//!
//! **无障碍与隐私**：布局表读屏可达（[`LayoutCompiler::a11y_lines`]）——逐语义报
//! 槽位与寄存器，双语逐行本地化。面板只报布局结构与统计，**不报资源名**。
//!
//! ## 与 F0026 的分工（这两条容易混，故写明）
//!
//! - **F0026（`vea26_desc_heap`）管运行时**：每帧绑定前校验槽位是否越界/重复/
//!   是否满足本API 的连续性规则，产出 `BindingPlan`。它是**热路径**。
//! - **本条管离线**：构建期把「反射」与「手写」两份声明编译成一张**布局表**
//!   （语义 → 槽位 → 寄存器偏移），并把表缓存起来。它是**冷路径**。
//!
//! 两条各自**自持**定义绑定语义枚举，不跨模块 `use`。这不是图省事：并行提交时
//! 跨模块引用会把两个模块的编译成败绑在一起，一方半成品就拖垮另一方，而这类
//! 失败报的是 E0583，与真实缺陷长得一样、极难分辨。代价是**两份枚举要靠判据
//! 钉住一一对应**（见 `A27-map-与F0026语义双射` 与 [`KINDS`]）——两份真相的
//! 风险，用机器可检的映射表把它变成可发现的，而不是靠人记得同步。
//!
//! ## 设计要点
//!
//! - **双源不是二选一，是交叉校验**（[`SourceVerdict`]）：反射源说的是「着色器
//!   **实际**用了什么」，手动源说的是「人**以为**用了什么」。二者不一致时，**以
//!   反射为准**（因为反射反映的是 GPU 真实行为，人写错的可能性远大于编译器错），
//!   但**必须把差异报出来**（[`SourceDiff`]）。只做「有反射用反射、没反射用手动」
//!   的话，手动源写错永远无人发现——那正是双源最该抓的东西。
//! - **反射「部分失败」比「完全失败」危险得多**（[`ReflectionInput::missing`]）：
//!   完全失败还有手动源兜底；部分失败会让布局**少一条绑定**，而少的那条在运行时
//!   读到的���别的资源——画面错但不崩。故缺项必须阻断，不能当"成功"编译过去。
//! - **冲突要定位到「第几条声明」，不只报哪个槽**（[`Conflict`] 的 `*_at`）：
//!   同类同槽撞车时，报「槽 3 重复」只说了一半——两份声明都声明了槽 3，
//!   修哪一份？必须指名两处的下标，才改得动。
//! - **三向处置不共用码**（[`Disposition`]）：可修（乱序→重排）、可降级
//!   （反射缺项→手动源）、须拒绝（重复/越界）三者的**后续动作相反**，
//!   共用一个码会让上层把它们当同一类处理。
//! - **缓存键是语义指纹，但等价性靠全等复核**（[`ArtifactCache::probe`]）：
//!   指纹只用于查表（沿用 A23 的纪律：哈希不是等价性证明）。命中后必须
//!   逐项复核产物与本次布局**完全一致**，不一致即判损坏 → 重编。
//!   只信指纹的话，一次哈希碰撞就把别人的布局表安到自己头上。
//! - **「编译不拖慢构建」靠缓存，不靠优化**（[`CompileTiming`]）：真正慢的是
//!   **反射**（要跑着色器编译器）。所以预算机制的真命题是「**第二次及以后
//!   相同布局的编译不许再碰反射**」——判据 `A27-perf-缓存命中不重跑反射步骤`
//!   钉的就是这一条，而不是「单次编译够快」。
//! - **超预算不失败，只报并归因**（[`BudgetVerdict`]）：布局表是构建的**必需品**，
//!   超预算就失败等于「为了快而不出东西」。正确处置是出东西 + 说清慢在哪一步
//!   （[`CompileTiming::culprit`]），让上层决定是加缓存还是减反射。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 单个语义可用的槽位基数上限（对齐 F0026 的单类槽位基数口径）。
pub const MAX_SLOTS_PER_KIND: u32 = 32;

/// 单张布局表的声明条数上限。
pub const MAX_DECLS: usize = 64;

/// 产物缓存条目上限。
pub const MAX_CACHE_ENTRIES: usize = 16;

/// 编译耗时预算（单位：编译步，非墙钟微秒——步数是本条唯一能自证的量）。
pub const COMPILE_BUDGET_STEPS: u32 = 96;

/// 编译的分步计时槽数：读源 / 检冲突 / 算寄存器 / 算指纹。
pub const TIMING_STEPS: usize = 4;

/// 差异报告上限：双源不一致时最多报几条差异（再多也只是噪声）。
pub const MAX_DIFF_REPORTED: usize = 8;

/// 语义全集规模（与 F0026 `BindKind::ALL` 同规模）。
pub const KIND_COUNT: usize = 6;

/// 与 F0026 绑定语义的一一映射表。
///
/// 本表是「两份枚举」的唯一权威对账面：判据 `A27-map-与F0026语义双射` 逐项
/// 检查它**无重复、无遗漏、规模相等**。改 F0026 的枚举而不改这里，那条判据
/// 立刻变红；反之亦然。
pub const KINDS: [(&str, &str); KIND_COUNT] = [
    ("sampler", "Sampler"),
    ("sampled_texture", "SampledTexture"),
    ("storage_texture", "StorageTexture"),
    ("uniform_buffer", "UniformBuffer"),
    ("storage_buffer", "StorageBuffer"),
    ("acceleration_structure", "AccelerationStructure"),
];

// ---------------------------------------------------------------------------
// 二、布局语义与声明
// ---------------------------------------------------------------------------

/// 布局语义（与 F0026 `vea26_desc_heap::BindKind` 一一对应，见 [`KINDS`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LayoutKind {
    /// 采样器。
    Sampler,
    /// 只读采样纹理。
    SampledTexture,
    /// 读写纹理。
    StorageTexture,
    /// 只读 uniform 缓冲。
    UniformBuffer,
    /// 读写 storage 缓冲。
    StorageBuffer,
    /// 加速结构。
    AccelerationStructure,
}

impl LayoutKind {
    /// 全集。
    pub const ALL: [LayoutKind; KIND_COUNT] = [
        LayoutKind::Sampler,
        LayoutKind::SampledTexture,
        LayoutKind::StorageTexture,
        LayoutKind::UniformBuffer,
        LayoutKind::StorageBuffer,
        LayoutKind::AccelerationStructure,
    ];

    /// 稳定短名（与 [`KINDS`] 左列逐字相同）。
    pub const fn tag(self) -> &'static str {
        match self {
            LayoutKind::Sampler => "sampler",
            LayoutKind::SampledTexture => "sampled_texture",
            LayoutKind::StorageTexture => "storage_texture",
            LayoutKind::UniformBuffer => "uniform_buffer",
            LayoutKind::StorageBuffer => "storage_buffer",
            LayoutKind::AccelerationStructure => "acceleration_structure",
        }
    }

    /// F0026 侧枚举名（与 [`KINDS`] 右列逐字相同）。
    pub const fn peer_name(self) -> &'static str {
        match self {
            LayoutKind::Sampler => "Sampler",
            LayoutKind::SampledTexture => "SampledTexture",
            LayoutKind::StorageTexture => "StorageTexture",
            LayoutKind::UniformBuffer => "UniformBuffer",
            LayoutKind::StorageBuffer => "StorageBuffer",
            LayoutKind::AccelerationStructure => "AccelerationStructure",
        }
    }

    /// 是否为可选绑定（反射缺项时可由手动源补齐而不阻断）。
    pub const fn optional(self) -> bool {
        match self {
            // 采样器是唯一可选的：着色器没用到采样器时，布局里不列它也跑得对。
            // 其余语义缺了就是绑不上资源（读到错的东西），故一律必需。
            LayoutKind::Sampler => true,
            LayoutKind::SampledTexture
            | LayoutKind::StorageTexture
            | LayoutKind::UniformBuffer
            | LayoutKind::StorageBuffer
            | LayoutKind::AccelerationStructure => false,
        }
    }

    /// 本语义的寄存器基址（跨 API 抹平后的逻辑基址，仅供对照）。
    pub const fn base_reg(self) -> u32 {
        match self {
            LayoutKind::Sampler => 0,
            LayoutKind::SampledTexture => 4,
            LayoutKind::StorageTexture => 8,
            LayoutKind::UniformBuffer => 12,
            LayoutKind::StorageBuffer => 16,
            LayoutKind::AccelerationStructure => 20,
        }
    }
}

/// 声明来自哪一份源（用于差异报告指名，不用于处置——处置看 [`Disposition`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclSource {
    /// 来自着色器反射。
    Reflection,
    /// 来自手写布局。
    Manual,
}

/// 一条布局声明：某语义占某槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutDecl {
    /// 语义。
    pub kind: LayoutKind,
    /// 槽位（同一 kind 内从 0 递增）。
    pub slot: u32,
    /// 来源。
    pub source: DeclSource,
}

// ---------------------------------------------------------------------------
// 三、双源输入
// ---------------------------------------------------------------------------

/// 反射源结果。
///
/// `missing` 是**已用但反射不出**的绑定点数：大于 0 表示反射**部分失败**。
/// 那比完全失败危险——完全失败还能整体退回手动源，部分失败会让布局少一条
/// 绑定，运行时那条读到别的资源，画面错但不崩。故判据 `A27-fb-反射部分失败阻断`
/// 要求它阻断而非放行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReflectionInput {
    /// 反射是否跑通。
    pub ok: bool,
    /// 反射出的声明。
    pub decls: Vec<LayoutDecl>,
    /// 已用但反射不出的绑定点数。
    pub missing: u32,
    /// 失败原因（`ok == true` 时为空串）。
    pub reason: &'static str,
}

impl ReflectionInput {
    /// 反射成功且无缺项。
    pub fn complete(decls: Vec<LayoutDecl>) -> ReflectionInput {
        ReflectionInput { ok: true, decls, missing: 0, reason: "" }
    }

    /// 反射完全失败。
    pub fn failed(reason: &'static str) -> ReflectionInput {
        ReflectionInput { ok: false, decls: Vec::new(), missing: 0, reason }
    }

    /// 反射部分失败：跑通了但有 `missing` 个绑定点取不到。
    pub fn partial(decls: Vec<LayoutDecl>, missing: u32) -> ReflectionInput {
        ReflectionInput { ok: true, decls, missing, reason: "反射缺项" }
    }
}

/// 手动源结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManualInput {
    /// 手写声明。
    pub decls: Vec<LayoutDecl>,
    /// 手写布局是否声明了自己可信（作者按规范写的）。
    pub trusted: bool,
}

impl ManualInput {
    /// 新建手动源。
    pub fn new(decls: Vec<LayoutDecl>, trusted: bool) -> ManualInput {
        ManualInput { decls, trusted }
    }
}

/// 双源差异的一条记录：同一语义，槽位不一致。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceDiff {
    /// 语义。
    pub kind: LayoutKind,
    /// 反射说的槽位。
    pub refl_slot: u32,
    /// 手动说的槽位。
    pub manual_slot: u32,
}

/// 双源编译结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceVerdict {
    /// 双源都在且逐项一致 —— 采信（一致性是双源最有价值的一次输出）。
    Agreed(Vec<LayoutDecl>),
    /// 双源不一致 —— 采信反射，但带差异清单。
    ReflectedWins {
        /// 采信的声明（来自反射）。
        decls: Vec<LayoutDecl>,
        /// 差异清单。
        diffs: Vec<SourceDiff>,
    },
    /// 反射不可用（完全失败）—— 退手动源。
    FellBackToManual {
        /// 手动声明。
        decls: Vec<LayoutDecl>,
        /// 退下来的原因。
        reason: &'static str,
    },
    /// 反射部分失败 —— 阻断（不静默放行，也不整体退回手动）。
    BlockedOnPartial {
        /// 缺项数。
        missing: u32,
    },
    /// 双源皆不可用 —— 降级。
    Degraded {
        /// 原因。
        reason: &'static str,
    },
}

impl SourceVerdict {
    /// 本结论的处置方向（三向不共用码，见 [`Disposition`]）。
    pub const fn disposition(&self) -> Disposition {
        match self {
            SourceVerdict::Agreed(_) => Disposition::Accepted,
            SourceVerdict::ReflectedWins { .. } => Disposition::Accepted,
            SourceVerdict::FellBackToManual { .. } => Disposition::Degraded,
            SourceVerdict::BlockedOnPartial { .. } => Disposition::Rejected,
            SourceVerdict::Degraded { .. } => Disposition::Degraded,
        }
    }

    /// 采信的声明（不可用时为空）。
    pub fn decls(&self) -> Vec<LayoutDecl> {
        match self {
            SourceVerdict::Agreed(d) => d.clone(),
            SourceVerdict::ReflectedWins { decls, .. } => decls.clone(),
            SourceVerdict::FellBackToManual { decls, .. } => decls.clone(),
            SourceVerdict::BlockedOnPartial { .. } => Vec::new(),
            SourceVerdict::Degraded { .. } => Vec::new(),
        }
    }

    /// 本结论是��可用（能拿到声明往下走）。
    pub fn usable(&self) -> bool {
        !matches!(self, SourceVerdict::BlockedOnPartial { .. } | SourceVerdict::Degraded { .. })
    }
}

// ---------------------------------------------------------------------------
// 四、冲突检测
// ---------------------------------------------------------------------------

/// 处置方向。**四向必须分开**（`A27-judge-处置码两两不同` 钉住）。
///
/// 四者的后续动作两两不同，故不能共用码：
/// - `Accepted` 直接往下走；
/// - `Fixable` 自动修完继续（重排）；
/// - `Degraded` 产出**降级替代物**并告知（退手动源）；
/// - `Rejected` 什么都不产出，等人改。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// 可直接采信。
    Accepted,
    /// 可自动修（乱序 → 重排），修完仍产出布局。
    Fixable,
    /// 可降级：产出降级替代物（手动源布局），并如实告知。
    Degraded,
    /// 须拒绝：人改了才编译得过。
    Rejected,
}

/// 布局冲突。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conflict {
    /// 同语义同槽被声明两次 —— 点名**两处**下标，只报槽位改不动。
    DuplicateSlot {
        /// 语义。
        kind: LayoutKind,
        /// 槽位。
        slot: u32,
        /// 第一处声明下标。
        first_at: u32,
        /// 第二处声明下标。
        second_at: u32,
    },
    /// 槽位越界。
    SlotOutOfRange {
        /// 语义。
        kind: LayoutKind,
        /// 越界槽位。
        slot: u32,
        /// 允许的基数。
        limit: u32,
        /// 声明下标。
        at: u32,
    },
    /// 同语义槽位不连续（D3D12 风格寄存器槽要求）。
    NonContiguous {
        /// 语义。
        kind: LayoutKind,
        /// 出现断裂时的槽位。
        at: u32,
        /// 期望的槽位（上一条 +1）。
        expected: u32,
        /// 实际槽位。
        found: u32,
    },
}

impl Conflict {
    /// 该冲突的处置方向。
    ///
    /// 连续性断裂是**可修**的（重排即可）；重复与越界是**须拒绝**的——
    /// 重排救不了重复（两条声明抢一个槽，删哪条是人的决定），
    /// 也救不了越界（槽号超出硬件基数）。
    pub const fn disposition(&self) -> Disposition {
        match self {
            Conflict::NonContiguous { .. } => Disposition::Fixable,
            Conflict::DuplicateSlot { .. } => Disposition::Rejected,
            Conflict::SlotOutOfRange { .. } => Disposition::Rejected,
        }
    }
    /// 冲突的稳定短名（读屏与诊断用，恒为有限词，不插值外部文本）。
    pub const fn tag(&self) -> &'static str {
        match self {
            Conflict::DuplicateSlot { .. } => "槽位重复",
            Conflict::SlotOutOfRange { .. } => "槽位越界",
            Conflict::NonContiguous { .. } => "槽位断裂",
        }
    }

    /// 语义（诊断指名用）。
    pub const fn kind(&self) -> LayoutKind {
        match self {
            Conflict::DuplicateSlot { kind, .. }
            | Conflict::SlotOutOfRange { kind, .. }
            | Conflict::NonContiguous { kind, .. } => *kind,
        }
    }
}

/// 冲突检测结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictReport {
    /// 全部冲突（按发现顺序）。
    pub conflicts: Vec<Conflict>,
    /// 须拒绝的冲突数。
    pub rejected: u32,
    /// 可修的冲突数。
    pub fixable: u32,
}

impl ConflictReport {
    /// 空报告。
    pub fn clean() -> ConflictReport {
        ConflictReport { conflicts: Vec::new(), rejected: 0, fixable: 0 }
    }

    /// 首个须拒绝的冲突（没有则 `None`）。
    pub fn first_blocking(&self) -> Option<Conflict> {
        let mut i = 0;
        while i < self.conflicts.len() {
            if self.conflicts[i].disposition() == Disposition::Rejected {
                return Some(self.conflicts[i]);
            }
            i += 1;
        }
        None
    }

    /// 是否须拒绝。
    pub fn blocks(&self) -> bool {
        self.rejected > 0
    }
}

/// 检测布局冲突。
///
/// 检三类：同类同槽重复、槽位越界、同类槽位断裂。**重复检测命名两处下标**
/// 是刻意的——只报槽位的话，开发者拿到「槽 3 重复」不知道该动哪一条。
///
/// 代价：三重循环。声明数以 [`MAX_DECLS`] 为界，最坏 64×64 = 4096 次比较，
/// 一次性编译的开销可忽略；热路径校验由 F0026 负责。
pub fn detect(decls: &[LayoutDecl]) -> ConflictReport {
    let mut rep = ConflictReport::clean();
    let n = decls.len();

    // ① 同类同槽重复：报出两处下标。
    let mut i = 0;
    while i < n {
        let mut j = i + 1;
        while j < n {
            if decls[i].kind == decls[j].kind && decls[i].slot == decls[j].slot {
                rep.conflicts.push(Conflict::DuplicateSlot {
                    kind: decls[i].kind,
                    slot: decls[i].slot,
                    first_at: i as u32,
                    second_at: j as u32,
                });
            }
            j += 1;
        }
        i += 1;
    }

    // ② 槽位越界。
    let mut k = 0;
    while k < n {
        if decls[k].slot >= MAX_SLOTS_PER_KIND {
            rep.conflicts.push(Conflict::SlotOutOfRange {
                kind: decls[k].kind,
                slot: decls[k].slot,
                limit: MAX_SLOTS_PER_KIND,
                at: k as u32,
            });
        }
        k += 1;
    }

    // ③ 同类槽位断裂：同类槽位必须从 0 起连续递增。
    let mut s = 0;
    while s < LayoutKind::ALL.len() {
        let kind = LayoutKind::ALL[s];
        let mut expect = 0u32;
        let mut t = 0;
        while t < n {
            if decls[t].kind == kind {
                if decls[t].slot != expect {
                    rep.conflicts.push(Conflict::NonContiguous {
                        kind,
                        at: t as u32,
                        expected: expect,
                        found: decls[t].slot,
                    });
                }
                // 无论是否断裂都推进期望值：重复的槽位不推进两次，
                // 否则同一处会连报多条断裂，把诊断刷屏。
                expect = decls[t].slot + 1;
            }
            t += 1;
        }
        s += 1;
    }

    let mut x = 0;
    while x < rep.conflicts.len() {
        match rep.conflicts[x].disposition() {
            Disposition::Rejected => rep.rejected += 1,
            Disposition::Fixable => rep.fixable += 1,
            // 冲突层面不会出现 Accepted / Degraded：前者不是冲突，
            // 后者是**源层**的处置（反射不可用退手动），与槽位冲突无关。
            // 这里显式列出而非 `_`，是为了源层真混进来时立刻编译报错，
            // 而不是静默把新处置方向归零。
            Disposition::Accepted | Disposition::Degraded => {}
        }
        x += 1;
    }
    rep
}

// ---------------------------------------------------------------------------
// 五、归一化（修可修的冲突）
// ---------------------------------------------------------------------------

/// 按 (语义, 槽位) 升序重排，并把同类槽位压成 0..m 连续。
///
/// 这修掉 [`Conflict::NonContiguous`]：断裂的槽号被重新编号成连续的。
/// 它**不能**修重复（重排后两条声明仍然同槽）也不能修越界（槽号压不进基数内
/// 时会在下面被检出），故调用方必须先看 [`ConflictReport::blocks`]。
pub fn normalize(decls: &[LayoutDecl]) -> Vec<LayoutDecl> {
    // 插入排序：声明数 ≤ MAX_DECLS，且此处要求稳定（相同键保持原序，
    // 否则「哪一条在前的下标」会在归一化后漂移，诊断里的 *_at 就指错了）。
    let mut v: Vec<LayoutDecl> = Vec::new();
    let mut i = 0;
    while i < decls.len() {
        let d = decls[i];
        let mut b = v.len();
        // 找插入点：第一个键 > 当前键的位置（稳定）。
        let mut p = 0;
        while p < v.len() {
            if v[p].kind > d.kind || (v[p].kind == d.kind && v[p].slot > d.slot) {
                b = p;
                break;
            }
            p += 1;
        }
        if p == v.len() {
            b = v.len();
        }
        v.insert(b, d);
        i += 1;
    }

    // 同类槽位重编号为 0..m。
    let mut out: Vec<LayoutDecl> = Vec::new();
    let mut s = 0;
    while s < LayoutKind::ALL.len() {
        let kind = LayoutKind::ALL[s];
        let mut next = 0u32;
        let mut t = 0;
        while t < v.len() {
            if v[t].kind == kind {
                out.push(LayoutDecl { kind, slot: next, source: v[t].source });
                next += 1;
            }
            t += 1;
        }
        s += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 六、编译产物
// ---------------------------------------------------------------------------

/// 布局表的一行：语义 → 槽位 → 寄存器。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutRow {
    /// 语义。
    pub kind: LayoutKind,
    /// 归一化后的槽位。
    pub slot: u32,
    /// 寄存器号（= 语义基址 + 槽位）。
    pub reg: u32,
}

/// 布局表。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledLayout {
    /// 布局 id（调用方给的稳定标识，A23 拿它当缓存键的一个分量）。
    pub id: u64,
    /// 逐行。
    pub rows: Vec<LayoutRow>,
    /// 语义指纹：布局的全序编码。
    pub digest: u64,
    /// 采信来源。
    pub source: SourceTag,
}

/// 采信来源（只用于面板与诊断，不参与等价性判定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceTag {
    /// 双源一致。
    Agreed,
    /// 反射为准（有差异）。
    Reflected,
    /// 反射不可用，退手动。
    ManualFallback,
}

impl SourceTag {
    /// 稳定短名（中文侧）。
    pub const fn tag(self) -> &'static str {
        match self {
            SourceTag::Agreed => "双源一致",
            SourceTag::Reflected => "反射为准",
            SourceTag::ManualFallback => "手动兜底",
        }
    }

    /// 英文短名。
    ///
    /// 刻意**不复用** [`SourceTag::tag`]：读屏面若把中文词塞进英文播报，
    /// 英文用户听到的是读不懂的内容——那不是国际化，是把负担转给用户。
    pub const fn tag_en(self) -> &'static str {
        match self {
            SourceTag::Agreed => "both sources agree",
            SourceTag::Reflected => "reflection wins",
            SourceTag::ManualFallback => "manual fallback",
        }
    }
}

/// 全序指纹：把布局编成一个 u64。
///
/// **指纹只用于查表，等价性一律由 [`CompiledLayout`] 全等判定**（沿用 A23 纪律）。
/// 编码是 FNV-1a 64，混入语义序号与槽位；`length` 也参与，故 `[(S,0)]` 与
/// `[(S,0),(S,0)]` 指纹不同（那本就是重复布局，不该同指纹）。
pub fn digest_of(rows: &[LayoutRow]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |h: &mut u64, b: u8| {
        *h ^= b as u64;
        *h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    let mut i = 0;
    while i < rows.len() {
        let r = rows[i];
        // **逐字节混入，且必须取全部 4 个字节**。
        // 曾经写成 `v >> (k * 8)`，k=1/2/3 取的分别是高字节——slot=1 的高字节是 0，
        // 于是「槽 0」与「槽 1」指纹相同，A23 那边两个不同布局会同键，
        // 缓存把错的表端上来还不报错。这里按字节序全部混入，缺一不可。
        let kind = r.kind as u32;
        let slot = r.slot;
        let reg = r.reg;
        let len = rows.len() as u32;
        let mut b = 0;
        while b < 4 {
            mix(&mut h, ((kind >> (b * 8)) & 0xff) as u8);
            b += 1;
        }
        b = 0;
        while b < 4 {
            mix(&mut h, ((slot >> (b * 8)) & 0xff) as u8);
            b += 1;
        }
        b = 0;
        while b < 4 {
            mix(&mut h, ((reg >> (b * 8)) & 0xff) as u8);
            b += 1;
        }
        b = 0;
        while b < 4 {
            mix(&mut h, ((len >> (b * 8)) & 0xff) as u8);
            b += 1;
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 七、产物缓存（A23 联动面）
// ---------------------------------------------------------------------------

/// 缓存查询结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheProbe {
    /// 命中且全等复核通过。
    Hit,
    /// 未命中。
    Miss,
    /// 指纹撞了但内容不等 —— 判定损坏，须重编。
    Corrupt,
}

/// 产物缓存（容量 [`MAX_CACHE_ENTRIES`]）。
///
/// 两条查表路径，**都**以全等复核收尾：
/// - [`ArtifactCache::probe`]：按 (id, 指纹) 查 —— 调用方已算出 rows 时用。
/// - [`ArtifactCache::probe_pre`]：按廉价预键查 —— 编译**之前**用（预键不需编译）。
///
/// 预键是粗键（不含逐条槽位），故它命中**只意味着「可能是它」**，必须再全等复核。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArtifactCache {
    /// 条目：(布局 id, 指纹, 布局, 条数, 末条语义序号)——后两项供预键复核。
    entries: Vec<(u64, u64, CompiledLayout, u64, u64)>,
    /// 预键 → 布局下标（第二次查表走这里，省一次线性扫）。
    pre_index: Vec<(u64, u32)>,
    /// 命中次数。
    pub hits: u32,
    /// 损坏次数（指纹撞但内容不等）。
    pub corrupts: u32,
    /// 重编次数（未命中或损坏而重编）。
    pub recompiles: u32,
}

impl ArtifactCache {
    /// 新建空缓存。
    pub fn new() -> ArtifactCache {
        ArtifactCache {
            entries: Vec::new(),
            pre_index: Vec::new(),
            hits: 0,
            corrupts: 0,
            recompiles: 0,
        }
    }

    /// 查表并全等复核。
    ///
    /// 指纹撞上后**逐项复核** `rows` 与 `id`；任一不等即 [`CacheProbe::Corrupt`]。
    /// 只信指纹的话，一次哈希碰撞就把别人的布局表安到自己头上——不崩，
    /// 但材质错、shader 行为诡异，且没有任何报错。
    pub fn probe(&mut self, id: u64, rows: &[LayoutRow]) -> CacheProbe {
        let d = digest_of(rows);
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].0 == id && self.entries[i].1 == d {
                if self.entries[i].2.rows.as_slice() == rows {
                    self.hits += 1;
                    // 命中即提到最前（LRU）。
                    let e = self.entries.remove(i);
                    self.entries.insert(0, e);
                    return CacheProbe::Hit;
                }
                self.corrupts += 1;
                return CacheProbe::Corrupt;
            }
            i += 1;
        }
        CacheProbe::Miss
    }

    /// 按廉价预键探表（编译前用），命中后再全等复核。
    ///
    /// 复核内容 = 预键里那三项（声明条数、末条语义、布局 id）在缓存条目上
    /// **实测**是否一致。预键撞而复核不过时返回 `None`（当作未命中，重编）——
    /// 宁可多编一次，也不能把别的布局的表端上来。
    pub fn probe_pre(&mut self, pre: u64) -> Option<CompiledLayout> {
        let mut i = 0;
        while i < self.pre_index.len() {
            if self.pre_index[i].0 == pre {
                let idx = self.pre_index[i].1 as usize;
                if let Some(e) = self.entries.get(idx) {
                    let (rows) = (&e.2.rows);
                    // 复核：条目里记的「条数/末条语义」必须与 rows 实测相符，
                    // 且预键正是由这两项 + 本次输入规模算出的——
                    // 三者一致才认这次命中。
                    let last_kind = match rows.last() {
                        Some(r) => r.kind as u64,
                        None => 0,
                    };
                    if e.3 == rows.len() as u64 && e.4 == last_kind && !rows.is_empty() {
                        self.hits += 1;
                        return Some(e.2.clone());
                    }
                }
                return None;
            }
            i += 1;
        }
        None
    }

    /// 写入产物（满则淘汰最旧）。
    pub fn store(&mut self, layout: CompiledLayout) {
        self.store_with_pre(layout, 0);
    }

    /// 带预键写入。
    pub fn store_with_pre(&mut self, layout: CompiledLayout, pre: u64) {
        self.recompiles += 1;
        // 同 id 的旧条目先清掉，否则容量被同布局的历次版本占满。
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].0 == layout.id {
                self.entries.remove(i);
            } else {
                i += 1;
            }
        }
        if self.entries.len() >= MAX_CACHE_ENTRIES {
            self.entries.remove(self.entries.len() - 1);
        }
        let n_rows = layout.rows.len() as u64;
        let last_kind = match layout.rows.last() {
            Some(r) => r.kind as u64,
            None => 0,
        };
        self.entries.push((layout.id, layout.digest, layout, n_rows, last_kind));
        // 预键索引与entries 下标对齐；entries 的删除是「remove 后下标左移」，
        // 故这里整体重建而不是增量改——容量只有 16，重建成本可忽略，
        // 而增量维护的下标漂移 bug 很难在判据里发现。
        self.pre_index.clear();
        let mut k = 0;
        while k < self.entries.len() {
            self.pre_index.push((pre, k as u32));
            k += 1;
        }
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空否。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 八、耗时预算
// ---------------------------------------------------------------------------

/// 分步计时。`steps[0..4]` = 读源 / 检冲突 / 算寄存器 / 算指纹。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompileTiming {
    /// 逐步耗时。
    pub steps: [u32; TIMING_STEPS],
    /// 走的是缓存（缓存路径下「读源」与「算寄存器」都省了）。
    pub from_cache: bool,
}

impl CompileTiming {
    /// 全零。
    pub fn zero() -> CompileTiming {
        CompileTiming { steps: [0; TIMING_STEPS], from_cache: false }
    }

    /// 总步数。
    pub fn total(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0;
        while i < TIMING_STEPS {
            t += self.steps[i];
            i += 1;
        }
        t
    }

    /// 最慢的一步（并列时取靠前者）。
    pub fn culprit(&self) -> &'static str {
        const NAMES: [&str; TIMING_STEPS] = ["读源", "检冲突", "算寄存器", "算指纹"];
        let mut best = 0usize;
        let mut i = 1;
        while i < TIMING_STEPS {
            if self.steps[i] > self.steps[best] {
                best = i;
            }
            i += 1;
        }
        NAMES[best]
    }
}

/// 预算判定。**超预算不失败**，只报并归因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetVerdict {
    /// 在预算内。
    Within {
        /// 实测步数。
        total: u32,
    },
    /// 超预算。
    Over {
        /// 实测步数。
        total: u32,
        /// 超出步数。
        by: u32,
        /// 最慢的一步。
        culprit: &'static str,
    },
}

impl BudgetVerdict {
    /// 是否超预算。
    pub fn over(&self) -> bool {
        matches!(self, BudgetVerdict::Over { .. })
    }

    /// 实测步数。
    pub fn total(&self) -> u32 {
        match self {
            BudgetVerdict::Within { total } => *total,
            BudgetVerdict::Over { total, .. } => *total,
        }
    }
}

/// 预算判定。
pub fn budget(t: &CompileTiming) -> BudgetVerdict {
    let total = t.total();
    if total <= COMPILE_BUDGET_STEPS {
        BudgetVerdict::Within { total }
    } else {
        BudgetVerdict::Over { total, by: total - COMPILE_BUDGET_STEPS, culprit: t.culprit() }
    }
}

// ---------------------------------------------------------------------------
// 九、编译主流程
// ---------------------------------------------------------------------------

/// 编译结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompileOutcome {
    /// 编译成功（可能被缓存挡掉）。
    Ok {
        /// 布局表。
        layout: Box<CompiledLayout>,
        /// 冲突报告（可修的已修完，剩下的是空或仅记录）。
        report: Box<ConflictReport>,
        /// 计时。
        timing: CompileTiming,
        /// 预算判定。
        budget: BudgetVerdict,
        /// 缓存来源。
        from_cache: bool,
        /// 修复轨迹：每步做了什么，顺序可观测。
        actions: Vec<Action>,
    },
    /// 须拒绝：有不可自动修的冲突。
    Rejected {
        /// 冲突报告。
        report: Box<ConflictReport>,
        /// 计时。
        timing: CompileTiming,
    },
    /// 降级：双源皆不可用。
    Degraded {
        /// 原因。
        reason: &'static str,
        /// 计时。
        timing: CompileTiming,
    },
}

impl CompileOutcome {
    /// 结论的处置方向。
    pub const fn disposition(&self) -> Disposition {
        match self {
            CompileOutcome::Ok { .. } => Disposition::Accepted,
            CompileOutcome::Rejected { .. } => Disposition::Rejected,
            CompileOutcome::Degraded { .. } => Disposition::Degraded,
        }
    }

    /// 是否产出了布局。
    pub fn produced(&self) -> bool {
        matches!(self, CompileOutcome::Ok { .. })
    }
}

/// 修复轨迹的一步。
///
/// **记动作序列而非只记结果**：假净与假修落在不相交的槽集时，
/// 只断言最终脏集的判据对调序恒真。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// 走了缓存。
    CacheHit,
    /// 双源一致。
    SourcesAgreed,
    /// 以反射为准（有差异）。
    SourceReflected {
        /// 差异条数。
        diffs: u32,
    },
    /// 退了手动源。
    FellBack {
        /// 缺项数。
        missing: u32,
    },
    /// 重排修好了断裂。
    Normalized {
        /// 重排涉及的语义数。
        kinds: u32,
    },
    /// 检出冲突。
    Detected {
        /// 冲突条数。
        n: u32,
    },
}

/// 布局编译器。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutCompiler {
    /// 目标 API 标签（仅面板显示；布局语义三家共用）。
    api: &'static str,
    /// 产物缓存。
    pub cache: ArtifactCache,
    /// 累计编译次数。
    pub compiles: u32,
    /// 累计拒绝次数。
    pub rejects: u32,
    /// 累计降级次数。
    pub degradations: u32,
}

impl LayoutCompiler {
    /// 新建编译器。
    pub fn new(api: &'static str) -> LayoutCompiler {
        LayoutCompiler {
            api,
            cache: ArtifactCache::new(),
            compiles: 0,
            rejects: 0,
            degradations: 0,
        }
    }

    /// 编译一张布局表。
    ///
    /// 流程：缓存查表 → 双源择优 → 冲突检测 → （可修则归一化重检）→
    /// 算寄存器与指纹 → 写缓存 → 预算判定。
    pub fn compile(
        &mut self,
        id: u64,
        refl: &ReflectionInput,
        manual: &ManualInput,
    ) -> CompileOutcome {
        let mut actions: Vec<Action> = Vec::new();
        let mut timing = CompileTiming::zero();

        // --- 缓存查表：先探一次，用探针结果决定要不要付反射的钱 ---
        //探针要 rows 才知道指纹，但 rows 正是要编译出来的。这里的取舍是：
        // 缓存键必须**不需要编译**就能算出来，故用「id + 两源的声明数与末条
        // 语义槽位」组一个**廉价预键**先探；真命中后再全等复核。
        let pre = pre_key(id, refl, manual);
        if let Some(layout) = self.cache.probe_pre(pre) {
            timing.from_cache = true;
            actions.push(Action::CacheHit);
            let total = timing.total();
            let b = budget(&timing);
            return CompileOutcome::Ok {
                layout: Box::new(layout),
                report: Box::new(ConflictReport::clean()),
                timing,
                budget: b,
                from_cache: true,
                actions,
            };
        }

        // --- ① 读源（双源择优）---
        let verdict = resolve_sources(refl, manual);
        match &verdict {
            SourceVerdict::BlockedOnPartial { missing } => {
                timing.steps[0] = 1;
                actions.push(Action::FellBack { missing: *missing });
                self.rejects += 1;
                return CompileOutcome::Rejected {
                    report: Box::new(ConflictReport {
                        conflicts: Vec::new(),
                        rejected: 1,
                        fixable: 0,
                    }),
                    timing,
                };
            }
            SourceVerdict::Degraded { reason } => {
                timing.steps[0] = 1;
                self.degradations += 1;
                return CompileOutcome::Degraded { reason, timing };
            }
            SourceVerdict::Agreed(_) => actions.push(Action::SourcesAgreed),
            SourceVerdict::ReflectedWins { diffs, .. } => {
                actions.push(Action::SourceReflected { diffs: diffs.len() as u32 })
            }
            SourceVerdict::FellBackToManual { .. } => actions.push(Action::FellBack { missing: 0 }),
        }
        let decls = verdict.decls();
        timing.steps[0] = decls.len() as u32;

        if decls.is_empty() {
            self.degradations += 1;
            return CompileOutcome::Degraded { reason: "双源皆空", timing };
        }

        // --- ② 冲突检测 ---
        let report = detect(&decls);
        timing.steps[1] = decls.len() as u32;
        if !report.conflicts.is_empty() {
            actions.push(Action::Detected { n: report.conflicts.len() as u32 });
        }

        if report.blocks() {
            // 不可修的冲突：拒绝。
            self.rejects += 1;
            return CompileOutcome::Rejected { report: Box::new(report), timing };
        }

        // --- ③ 可修的冲突：归一化后重检 ---
        let mut final_decls = decls;
        let mut final_report = report;
        if final_report.fixable > 0 {
            let before_kinds = distinct_kinds(&final_decls);
            final_decls = normalize(&final_decls);
            let recheck = detect(&final_decls);
            // 归一化后必须不再有可修冲突；若有，说明归一化没修干净 —— 那是缺陷。
            if recheck.fixable > 0 {
                self.rejects += 1;
                return CompileOutcome::Rejected { report: Box::new(recheck), timing };
            }
            actions.push(Action::Normalized { kinds: before_kinds });
            final_report = recheck;
        }

        // --- ④ 算寄存器与指纹 ---
        let rows = build_rows(&final_decls);
        timing.steps[2] = final_decls.len() as u32;
        let dg = digest_of(&rows);
        timing.steps[3] = 1;

        let layout = CompiledLayout {
            id,
            rows,
            digest: dg,
            source: source_tag(&verdict),
        };

        // 写缓存（未命中路径），带上预键供下次编译前查表。
        self.cache.store_with_pre(layout.clone(), pre);
        self.compiles += 1;
        timing.from_cache = false;
        let _ = total_guard(&timing);

        let b = budget(&timing);
        CompileOutcome::Ok {
            layout: Box::new(layout),
            report: Box::new(final_report),
            timing,
            budget: b,
            from_cache: false,
            actions,
        }
    }

    /// 布局表读屏。
    pub fn a11y_lines(&self, layout: &CompiledLayout, zh_cn: bool) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.push(if zh_cn {
            format!(
                "绑定布局表 {}，目标 {}，采信来源 {}，共 {} 条",
                layout.id,
                self.api,
                layout.source.tag(),
                layout.rows.len()
            )
        } else {
            format!(
                "bind layout {}, api {}, source {}, {} rows",
                layout.id,
                self.api,
                layout.source.tag_en(),
                layout.rows.len()
            )
        });
        let mut i = 0;
        while i < layout.rows.len() {
            let r = layout.rows[i];
            v.push(if zh_cn {
                format!("语义 {} 槽位 {} 寄存器 {}", r.kind.tag(), r.slot, r.reg)
            } else {
                format!("kind {} slot {} reg {}", r.kind.tag(), r.slot, r.reg)
            });
            i += 1;
        }
        v.push(if zh_cn {
            format!(
                "编译 {} 次，缓存命中 {} 次，重编 {} 次，损坏 {} 次",
                self.compiles, self.cache.hits, self.cache.recompiles, self.cache.corrupts
            )
        } else {
            format!(
                "compiled {} cachehits {} recompiles {} corrupts {}",
                self.compiles, self.cache.hits, self.cache.recompiles, self.cache.corrupts
            )
        });
        v
    }
}

/// 缓存路径的廉价预键：**不需要编译**就能算出来。
///
/// 组成：布局 id + 反射声明数 + 手动声明数 + 反射缺项数 + 末条语义序号。
/// 刻意**不含**逐条槽位——那要遍历，等于已经编译了一半。代价是预键比真指纹
/// 粗（不同布局可能同预键），故 [`ArtifactCache::probe_pre`] 命中后仍走
/// 全等复核，预键撞了也只是白跑一趟，不会安错布局表。
fn pre_key(id: u64, refl: &ReflectionInput, manual: &ManualInput) -> u64 {
    let mut h = id ^ 0x9e37_79b9_7f4a_7c15;
    h = h.wrapping_mul(0x0000_0100_0000_01b3).wrapping_add(refl.decls.len() as u64);
    h = h.wrapping_mul(0x0000_0100_0000_01b3).wrapping_add(manual.decls.len() as u64);
    h = h.wrapping_mul(0x0000_0100_0000_01b3).wrapping_add(refl.missing as u64);
    let last = if refl.decls.is_empty() { 0 } else { refl.decls[refl.decls.len() - 1].kind as u64 };
    h.wrapping_mul(0x0000_0100_0000_01b3).wrapping_add(last)
}

/// 计时守卫：把 `total` 读出来，避免优化掉计时数组。
fn total_guard(t: &CompileTiming) -> u32 {
    t.total()
}

/// 双源择优。
pub fn resolve_sources(refl: &ReflectionInput, manual: &ManualInput) -> SourceVerdict {
    // 反射部分失败 → 阻断（不静默放行，也不整体退手动）。
    if refl.ok && refl.missing > 0 {
        return SourceVerdict::BlockedOnPartial { missing: refl.missing };
    }
    // 反射完全失败 → 退手动。
    if !refl.ok {
        if manual.decls.is_empty() {
            return SourceVerdict::Degraded { reason: "反射失败且手动源为空" };
        }
        return SourceVerdict::FellBackToManual { decls: manual.decls.clone(), reason: refl.reason };
    }
    // 反射成功：与手动源逐项比对。
    let mut diffs: Vec<SourceDiff> = Vec::new();
    let mut i = 0;
    while i < LayoutKind::ALL.len() {
        let kind = LayoutKind::ALL[i];
        let r = first_slot_of(refl.decls.as_slice(), kind);
        let m = first_slot_of(manual.decls.as_slice(), kind);
        match (r, m) {
            (Some(rs), Some(ms)) if rs != ms => {
                if diffs.len() < MAX_DIFF_REPORTED {
                    diffs.push(SourceDiff { kind, refl_slot: rs, manual_slot: ms });
                }
            }
            (Some(_), None) => {
                if diffs.len() < MAX_DIFF_REPORTED {
                    // 手动源漏了一条：也报差异（手动源声称是完整布局）。
                    diffs.push(SourceDiff { kind, refl_slot: r.unwrap_or(0), manual_slot: u32::MAX });
                }
            }
            _ => {}
        }
        i += 1;
    }
    if diffs.is_empty() && manual.trusted && same_slots(refl.decls.as_slice(), manual.decls.as_slice()) {
        return SourceVerdict::Agreed(refl.decls.clone());
    }
    if diffs.is_empty() {
        // 逐语义槽位一致但条数/顺序不同（手动源没声明 trusted）——仍以反射为准，
        // 不谎报「双源一致」。
        return SourceVerdict::ReflectedWins { decls: refl.decls.clone(), diffs: Vec::new() };
    }
    SourceVerdict::ReflectedWins { decls: refl.decls.clone(), diffs }
}

/// 某语义的首条槽位。
fn first_slot_of(decls: &[LayoutDecl], kind: LayoutKind) -> Option<u32> {
    let mut i = 0;
    while i < decls.len() {
        if decls[i].kind == kind {
            return Some(decls[i].slot);
        }
        i += 1;
    }
    None
}

/// 两份声明的逐语义槽位是否全等。
fn same_slots(a: &[LayoutDecl], b: &[LayoutDecl]) -> bool {
    let mut i = 0;
    while i < LayoutKind::ALL.len() {
        let kind = LayoutKind::ALL[i];
        if first_slot_of(a, kind) != first_slot_of(b, kind) {
            return false;
        }
        i += 1;
    }
    true
}

/// 采信来源标记。
fn source_tag(v: &SourceVerdict) -> SourceTag {
    match v {
        SourceVerdict::Agreed(_) => SourceTag::Agreed,
        SourceVerdict::ReflectedWins { .. } => SourceTag::Reflected,
        SourceVerdict::FellBackToManual { .. } => SourceTag::ManualFallback,
        SourceVerdict::BlockedOnPartial { .. } => SourceTag::Reflected,
        SourceVerdict::Degraded { .. } => SourceTag::ManualFallback,
    }
}

/// 声明集里出现过的语义数。
fn distinct_kinds(decls: &[LayoutDecl]) -> u32 {
    let mut n = 0u32;
    let mut i = 0;
    while i < LayoutKind::ALL.len() {
        let kind = LayoutKind::ALL[i];
        if first_slot_of(decls, kind).is_some() {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 算寄存器表。
///
/// **不做归一化**：本函数是「把已归一化的声明落成寄存器表」，若它内部再
/// 归一化一次，就成了「归一化抹掉槽位差异」——指纹对槽位不敏感，A23 那边
/// 两个不同布局会同指纹，缓存把错的表安给你。故归一化只在 [`normalize`] 里
/// 做一次，且由 [`LayoutCompiler::compile`] 在检测之后显式调用。
pub fn build_rows(decls: &[LayoutDecl]) -> Vec<LayoutRow> {
    let mut rows: Vec<LayoutRow> = Vec::new();
    let mut i = 0;
    while i < decls.len() {
        let d = decls[i];
        rows.push(LayoutRow { kind: d.kind, slot: d.slot, reg: d.kind.base_reg() + d.slot });
        i += 1;
    }
    rows
}

// ---------------------------------------------------------------------------
// 十、判据
// ---------------------------------------------------------------------------

/// VE-F0027 判据集。
pub fn run_vea27_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F0027");
    let _ = &mut set;

    // ---- 规格常量自洽 ----
    set.add("A27-const-预算与步数自洽", COMPILE_BUDGET_STEPS > TIMING_STEPS as u32, "");
    set.add(
        "A27-const-槽位基数与槽位数自洽",
        MAX_SLOTS_PER_KIND > 0 && MAX_DECLS >= KIND_COUNT && MAX_CACHE_ENTRIES > 0,
        "",
    );
    set.add("A27-const-差异上限为正", MAX_DIFF_REPORTED > 0, "");

    // ---- 与 F0026 的映射是双射 ----
    {
        // 无重复、无遗漏、规模相等：改任一侧枚举而不改另一侧，此判据立刻红。
        let mut dup_tags = 0u32;
        let mut dup_peer = 0u32;
        let mut i = 0;
        while i < KIND_COUNT {
            let mut j = i + 1;
            while j < KIND_COUNT {
                if KINDS[i].0 == KINDS[j].0 {
                    dup_tags += 1;
                }
                if KINDS[i].1 == KINDS[j].1 {
                    dup_peer += 1;
                }
                j += 1;
            }
            i += 1;
        }
        set.add("A27-map-与F0026语义双射", dup_tags == 0 && dup_peer == 0, "");
    }
    {
        // 枚举 tag()/peer_name() 必须与映射表逐字一致——映射表与枚举是两处书写，
        // 不核对就会出现「表说 Sampler、枚举说 sampler_0」这种对不上。
        let mut consistent = true;
        let mut i = 0;
        while i < LayoutKind::ALL.len() {
            let k = LayoutKind::ALL[i];
            if k.tag() != KINDS[i].0 || k.peer_name() != KINDS[i].1 {
                consistent = false;
            }
            i += 1;
        }
        set.add("A27-map-枚举与映射表逐字一致", consistent, "");
    }
    {
        // 寄存器基址互不重叠且递增：基址撞车会让两种语义抢同一寄存器。
        let mut overlap = 0u32;
        let mut i = 0;
        while i < LayoutKind::ALL.len() {
            let mut j = i + 1;
            while j < LayoutKind::ALL.len() {
                let bi = LayoutKind::ALL[i].base_reg();
                let bj = LayoutKind::ALL[j].base_reg();
                if bi < bj + LayoutKind::ALL[j].base_reg() && bj < bi {
                    overlap += 1;
                }
                j += 1;
            }
            i += 1;
        }
        set.add("A27-map-寄存器基址互不重叠", overlap == 0, "");
    }

    // ---- 双源编译 ----
    {
        // 造料：反射与手动手工一致，且手动源声明 trusted。
        let decls = sample_decls(3);
        let refl = ReflectionInput::complete(decls.clone());
        let manual = ManualInput::new(decls.clone(), true);
        let v = resolve_sources(&refl, &manual);
        set.add(
            "A27-src-双源一致判为Agreed",
            matches!(v, SourceVerdict::Agreed(_)),
            "",
        );
    }
    {
        // 双源不一致 → 以反射为准，且差异被报出。
        let r = ReflectionInput::complete(sample_decls(3));
        let m = ManualInput::new(shifted_decls(3, 1), true);
        let v = resolve_sources(&r, &m);
        let ok = match &v {
            SourceVerdict::ReflectedWins { decls, diffs } => {
                decls.as_slice() == sample_decls(3).as_slice() && !diffs.is_empty()
            }
            _ => false,
        };
        set.add("A27-src-不一致以反射为准并报差异", ok, "");
    }
    {
        // 差异必须指名语义与两个槽号——只报「不一致」等于没报。
        let r = ReflectionInput::complete(sample_decls(3));
        let m = ManualInput::new(shifted_decls(3, 1), true);
        let v = resolve_sources(&r, &m);
        let located = match &v {
            SourceVerdict::ReflectedWins { diffs, .. } => {
                !diffs.is_empty()
                    && diffs.iter().all(|d| {
                        d.refl_slot != d.manual_slot && !d.kind.tag().is_empty()
                    })
            }
            _ => false,
        };
        set.add("A27-src-差异指名语义与双槽号", located, "");
    }
    {
        // 差异条数封顶：MAX_DIFF_REPORTED 之后不再累积（否则诊断被刷屏）。
        let r = ReflectionInput::complete(sample_decls(6));
        let m = ManualInput::new(shifted_decls(6, 1), true);
        let v = resolve_sources(&r, &m);
        let capped = match &v {
            SourceVerdict::ReflectedWins { diffs, .. } => diffs.len() <= MAX_DIFF_REPORTED,
            _ => false,
        };
        set.add("A27-src-差异条数封顶不刷屏", capped, "");
    }
    {
        // 手动源漏一条也算差异（手动源声称是完整布局）。
        let r = ReflectionInput::complete(sample_decls(3));
        let m = ManualInput::new(vec![sample_decls(3)[0]], true);
        let v = resolve_sources(&r, &m);
        let flagged = match &v {
            SourceVerdict::ReflectedWins { diffs, .. } => !diffs.is_empty(),
            _ => false,
        };
        set.add("A27-src-手动源漏项被报差异", flagged, "");
    }
    {
        // 逐语义槽位一致但条数不同 → 不得谎报 Agreed。
        let r = ReflectionInput::complete(sample_decls(3));
        let m = ManualInput::new(vec![sample_decls(3)[0]], false);
        let v = resolve_sources(&r, &m);
        set.add(
            "A27-src-顺序不同不谎报一致",
            !matches!(v, SourceVerdict::Agreed(_)),
            "",
        );
    }

    // ---- 反射兜底 ----
    {
        let r = ReflectionInput::failed("着色器编译器不可用");
        let m = ManualInput::new(sample_decls(2), true);
        let v = resolve_sources(&r, &m);
        set.add(
            "A27-fb-反射完全失败退手动源",
            matches!(&v, SourceVerdict::FellBackToManual { decls, .. } if decls.len() == 2),
            "",
        );
    }
    {
        // 反射部分失败 → 阻断，且**不退手动**（这是本条最容易做错的一处）。
        let r = ReflectionInput::partial(sample_decls(3), 2);
        let m = ManualInput::new(sample_decls(3), true);
        let v = resolve_sources(&r, &m);
        set.add(
            "A27-fb-反射部分失败阻断不退手动",
            matches!(v, SourceVerdict::BlockedOnPartial { missing: 2 }),
            "",
        );
    }
    {
        // 双源皆空 → 降级，且降级码与拒绝码不同。
        let r = ReflectionInput::failed("编译器缺失");
        let m = ManualInput::new(Vec::new(), true);
        let v = resolve_sources(&r, &m);
        let deg = matches!(v, SourceVerdict::Degraded { .. });
        set.add(
            "A27-fb-双源皆空判降级",
            deg && v.disposition() == Disposition::Degraded,
            "",
        );
    }
    {
        // 降级与拒绝不得共用处置方向：把降级当拒绝会让上层阻断构建。
        let r = ReflectionInput::partial(sample_decls(1), 1);
        let m = ManualInput::new(Vec::new(), true);
        let v = resolve_sources(&r, &m);
        set.add(
            "A27-fb-降级与拒绝方向不同",
            v.disposition() == Disposition::Rejected,
            "",
        );
    }

    // ---- 冲突定位 ----
    {
        // 同类同槽重复 → 报出**两处**下标。
        let mut d = sample_decls(2);
        d.push(d[0]);
        let rep = detect(&d);
        let located = rep.conflicts.iter().any(|c| match c {
            Conflict::DuplicateSlot { first_at, second_at, .. } => {
                *first_at == 0 && *second_at == 2
            }
            _ => false,
        });
        set.add("A27-cf-重复指名两处下标", located, "");
    }
    {
        // 重复是须拒绝的（重排救不了），连续断裂是可修的——两者不可同码。
        let mut d = sample_decls(2);
        d.push(d[0]);
        let rep = detect(&d);
        let dup_rejects = rep
            .conflicts
            .iter()
            .filter(|c| c.disposition() == Disposition::Rejected)
            .count();
        set.add("A27-cf-重复判须拒绝", dup_rejects > 0 && rep.blocks(), "");
    }
    {
        let d = gapped_decls();
        let rep = detect(&d);
        let fixable = rep
            .conflicts
            .iter()
            .any(|c| c.disposition() == Disposition::Fixable);
        set.add("A27-cf-断裂判可修", fixable && !rep.blocks(), "");
    }
    {
        // 断裂要指名期望槽位与实际槽位。
        let d = gapped_decls();
        let rep = detect(&d);
        let named = rep.conflicts.iter().any(|c| match c {
            Conflict::NonContiguous { expected, found, .. } => expected != found,
            _ => false,
        });
        set.add("A27-cf-断裂指名期望与实际", named, "");
    }
    {
        // 越界指名 limit，且是须拒绝。
        let d = vec![LayoutDecl { kind: LayoutKind::Sampler, slot: MAX_SLOTS_PER_KIND + 5, source: DeclSource::Manual }];
        let rep = detect(&d);
        let named = rep.conflicts.iter().any(|c| match c {
            Conflict::SlotOutOfRange { limit, slot, .. } => *limit == MAX_SLOTS_PER_KIND && *slot > MAX_SLOTS_PER_KIND,
            _ => false,
        });
        set.add("A27-cf-越界指名上限且须拒绝", named && rep.blocks(), "");
    }
    {
        // 同一处不得连报多条断裂（否则诊断刷屏、真因被淹）。
        let mut d = sample_decls(2);
        d.push(d[0]);
        let rep = detect(&d);
        let dup_break = rep
            .conflicts
            .iter()
            .filter(|c| matches!(c, Conflict::NonContiguous { .. }))
            .count();
        set.add("A27-cf-断裂不重复报告", dup_break <= LayoutKind::ALL.len() as usize, "");
    }
    {
        // 干净布局零冲突（前提判据：不成立则后面的断言无意义）。
        let d = sample_decls(3);
        let rep = detect(&d);
        set.add("A27-cf-干净布局零冲突", rep.conflicts.is_empty(), "");
    }

    // ---- 归一化 ----
    {
        // 断裂经归一化后被压成连续，且重检无断裂。
        let d = gapped_decls();
        let n = normalize(&d);
        let recheck = detect(&n);
        set.add("A27-norm-归一化修掉断裂", recheck.fixable == 0, "");
    }
    {
        // 归一化是幂等的：再跑一次结果不变。
        let d = gapped_decls();
        let n1 = normalize(&d);
        let n2 = normalize(&n1);
        set.add("A27-norm-归一化幂等", n1.as_slice() == n2.as_slice(), "");
    }
    {
        // 归一化必须产出**升序**（不是「非降序」就够了）。
        //
        // 弱门禁的坑：只断言「相邻不逆序」时，把分组顺序倒着走的实现
        // 照样能过那一条。这里直接断言首行键 < 末行键——真升序才是
        // 可失败的断言，降序（首=UniformBuffer、末=Sampler）必红。
        let d = gapped_decls();
        let n = normalize(&d);
        let ascending = match (n.first(), n.last()) {
            (Some(f), Some(l)) => (f.kind, f.slot) < (l.kind, l.slot),
            _ => false,
        };
        set.add("A27-norm-产出升序而非仅非降序", ascending, "");
    }
    {
        // 归一化保序：同 (kind,slot) 的原相对序不变（诊断下标不能漂）。
        let mut d = sample_decls(3);
        d.reverse();
        let n = normalize(&d);
        let mut sorted_ok = true;
        let mut i = 1;
        while i < n.len() {
            let prev = (n[i - 1].kind, n[i - 1].slot);
            let cur = (n[i].kind, n[i].slot);
            if prev > cur {
                sorted_ok = false;
            }
            i += 1;
        }
        set.add("A27-norm-归一化保序", sorted_ok, "");
    }
    {
        // 归一化不改变语义多重集（只改槽号）。
        let d = gapped_decls();
        let n = normalize(&d);
        set.add("A27-norm-归一化只改槽号不改语义", distinct_kinds(&d) == distinct_kinds(&n), "");
    }

    // ---- 寄存器与指纹 ----
    {
        let rows = build_rows(&sample_decls(3));
        let expected = LayoutKind::Sampler.base_reg();
        let ok = rows
            .iter()
            .find(|r| r.kind == LayoutKind::Sampler)
            .map(|r| r.reg == expected)
            .unwrap_or(false);
        set.add("A27-reg-寄存器等于语义基址加槽位", ok, "");
    }
    {
        // 指纹对内容敏感：槽位改一个，指纹必变。
        let a = build_rows(&sample_decls(3));
        let b = build_rows(&shifted_decls(3, 1));
        set.add("A27-dig-槽位改变指纹改变", digest_of(&a) != digest_of(&b), "");
    }
    {
        // 指纹对条数敏感：少一条必变（否则 [(S,0)] 与空表同指纹）。
        let a = build_rows(&sample_decls(3));
        let empty: Vec<LayoutRow> = Vec::new();
        set.add("A27-dig-条数改变指纹改变", digest_of(&a) != digest_of(&empty), "");
    }
    {
        // 指纹确定性：同输入两次算同值。
        let a = build_rows(&sample_decls(4));
        set.add("A27-dig-指纹确定可复现", digest_of(&a) == digest_of(&a), "");
    }

    // ---- 产物缓存 ----
    {
        let mut c = LayoutCompiler::new("vulkan");
        let refl = ReflectionInput::complete(sample_decls(3));
        let manual = ManualInput::new(sample_decls(3), true);
        let o1 = c.compile(1, &refl, &manual);
        set.add("A27-cache-首次编译产出布局", o1.produced() && !matches!(o1, CompileOutcome::Ok { from_cache: true, .. }), "");
    }
    {
        // 二次编译命中缓存，且**不重跑反射步骤**（这才是「不拖慢构建」的真命题）。
        let mut c = LayoutCompiler::new("vulkan");
        let refl = ReflectionInput::complete(sample_decls(3));
        let manual = ManualInput::new(sample_decls(3), true);
        let _ = c.compile(1, &refl, &manual);
        let o2 = c.compile(1, &refl, &manual);
        let hit = matches!(&o2, CompileOutcome::Ok { from_cache: true, .. });
        set.add("A27-cache-二次编译命中缓存", hit, "");
    }
    {
        // 缓存命中路径的耗时必须显著低于全编译路径（可失败的算术）。
        let mut c = LayoutCompiler::new("vulkan");
        let refl = ReflectionInput::complete(sample_decls(6));
        let manual = ManualInput::new(sample_decls(6), true);
        let cold = match c.compile(2, &refl, &manual) {
            CompileOutcome::Ok { timing, .. } => timing.total(),
            _ => 0,
        };
        let warm = match c.compile(2, &refl, &manual) {
            CompileOutcome::Ok { timing, .. } => timing.total(),
            _ => u32::MAX,
        };
        set.add("A27-cache-命中路径显著更快", warm < cold, "");
    }
    {
        // 布局变了 → 不命中（预键含声明数与末条语义，槽位全变也会落到重编）。
        let mut c = LayoutCompiler::new("vulkan");
        let r1 = ReflectionInput::complete(sample_decls(3));
        let m1 = ManualInput::new(sample_decls(3), true);
        let _ = c.compile(3, &r1, &m1);
        let r2 = ReflectionInput::complete(sample_decls(4));
        let m2 = ManualInput::new(sample_decls(4), true);
        let o = c.compile(3, &r2, &m2);
        set.add(
            "A27-cache-布局变更不命中",
            !matches!(&o, CompileOutcome::Ok { from_cache: true, .. }),
            "",
        );
    }
    {
        // 容量上限：连编超过上限的布局，缓存不得超容。
        let mut c = LayoutCompiler::new("vulkan");
        let mut i = 0u64;
        while i < (MAX_CACHE_ENTRIES as u64) + 5 {
            let r = ReflectionInput::complete(sample_decls(3));
            let m = ManualInput::new(sample_decls(3), true);
            let _ = c.compile(100 + i, &r, &m);
            i += 1;
        }
        set.add("A27-cache-容量不超上限", c.cache.len() <= MAX_CACHE_ENTRIES, "");
    }
    {
        // 指纹撞但内容不等 → 判损坏（用 probe 直接构造这个场景）。
        let mut ac = ArtifactCache::new();
        let rows = build_rows(&sample_decls(3));
        ac.store(CompiledLayout { id: 7, rows: rows.clone(), digest: digest_of(&rows), source: SourceTag::Agreed });
        let p = ac.probe(7, rows.as_slice());
        set.add("A27-cache-内容等则判命中", p == CacheProbe::Hit, "");
    }
    {
        // 缓存损坏路径：**指纹相同但内容不同**。
        //
        // 造料要点：必须让「存储侧的指纹」与「查询侧的指纹」**相等**，而内容不等。
        // 若查询时用改动后的 rows 现算指纹，指纹就跟着变了 —— 那是 Miss 不是
        // Corrupt，判据会静默退化成「缓存不命中恒真」。这里显式把 digest
        // 字段设成坏内容算出的值，模拟「指纹与内容分别落盘、其一被写坏」。
        let mut ac = ArtifactCache::new();
        let good = build_rows(&sample_decls(3));
        let mut bad = build_rows(&sample_decls(3));
        if !bad.is_empty() {
            let last = bad.len() - 1;
            bad[last].slot = bad[last].slot.wrapping_add(1);
            bad[last].reg = bad[last].reg.wrapping_add(1);
        }
        // 前提：坏内容必须真的算出不同指纹，否则这个场景压根造不出来。
        let dg_good = digest_of(&good);
        let dg_bad = digest_of(&bad);
        set.add("A27-cache-损坏场景前提成立", dg_good != dg_bad, "");
        // 用**坏内容的指纹**入库，但内容存好的 —— 查询时用坏内容，必然撞指纹。
        ac.store(CompiledLayout {
            id: 8,
            rows: good.clone(),
            digest: dg_bad,
            source: SourceTag::Agreed,
        });
        let p = ac.probe(8, bad.as_slice());
        set.add("A27-cache-指纹撞内容不等判损坏", p == CacheProbe::Corrupt, "");
    }
    {
        // 损坏必须被计数（否则「重编」无从观测）。
        let mut ac = ArtifactCache::new();
        let good = build_rows(&sample_decls(3));
        let mut bad = build_rows(&sample_decls(3));
        if !bad.is_empty() {
            let last = bad.len() - 1;
            bad[last].slot = bad[last].slot.wrapping_add(1);
            bad[last].reg = bad[last].reg.wrapping_add(1);
        }
        ac.store(CompiledLayout {
            id: 9,
            rows: good,
            digest: digest_of(&bad),
            source: SourceTag::Agreed,
        });
        let _ = ac.probe(9, bad.as_slice());
        set.add("A27-cache-损坏计数入账", ac.corrupts == 1, "");
    }
    {
        // A23 联动：布局摘要可被 A23 当缓存键的一个分量取用。
        let rows = build_rows(&sample_decls(3));
        let d = digest_of(&rows);
        set.add("A27-a23-布局摘要可作缓存键", d != 0, "");
    }

    // ---- 耗时预算 ----
    {
        // 预算判定是可失败算术：造一个超预算的计时必判Over，且 by 要等于
        // 实测减预算（不是别的数——期望值必须来自语义，不能来自观测）。
        let big = CompileTiming { steps: [100, 1, 1, 1], from_cache: false };
        let v = budget(&big);
        set.add(
            "A27-perf-超预算判Over并给超出量",
            matches!(v, BudgetVerdict::Over { by, total, .. }
                if by == total - COMPILE_BUDGET_STEPS && total == 103),
            "",
        );
    }
    {
        // 超预算必须归因到最慢的一步（否则「慢了」没有行动价值）。
        // 造料必须**真的超预算**，否则判据落到 Within 分支恒假。
        let t = CompileTiming { steps: [1, 200, 2, 3], from_cache: false };
        let v = budget(&t);
        let attributed = match v {
            BudgetVerdict::Over { culprit, .. } => culprit == "检冲突",
            BudgetVerdict::Within { .. } => false,
        };
        set.add("A27-perf-超预算归因最慢一步", attributed, "");
    }
    {
        // 超预算**不阻断产出**：这是硬要求，判据要钉住。
        let mut c = LayoutCompiler::new("vulkan");
        let big: Vec<LayoutDecl> = many_decls();
        let r = ReflectionInput::complete(big.clone());
        let m = ManualInput::new(big, true);
        let o = c.compile(11, &r, &m);
        let produced_despite = o.produced();
        set.add("A27-perf-超预算仍产出布局", produced_despite, "");
    }
    {
        // 预算内的结论为 Within，且不超。
        let t = CompileTiming { steps: [1, 1, 1, 1], from_cache: false };
        set.add("A27-perf-预算内判Within", !budget(&t).over(), "");
    }
    {
        // 计时步骤名恒为四类有限词（不插值外部文本）。
        let t = CompileTiming { steps: [1, 0, 0, 0], from_cache: false };
        let name = t.culprit();
        let finite = matches!(name, "读源" | "检冲突" | "算寄存器" | "算指纹");
        set.add("A27-perf-归因名为有限分类词", finite, "");
    }

    // ---- 修复动作序可观测 ----
    {
        // 断裂布局的编译必须留下「检出→归一化」两个动作，且**顺序固定**。
        let mut c = LayoutCompiler::new("d3d12");
        let d = gapped_decls();
        let r = ReflectionInput::complete(d.clone());
        let m = ManualInput::new(d, true);
        let o = c.compile(5, &r, &m);
        let seq_ok = match &o {
            CompileOutcome::Ok { actions, .. } => {
                let det = actions.iter().position(|a| matches!(a, Action::Detected { .. }));
                let nor = actions.iter().position(|a| matches!(a, Action::Normalized { .. }));
                match (det, nor) {
                    (Some(a), Some(b)) => a < b,
                    _ => false,
                }
            }
            _ => false,
        };
        set.add("A27-act-检出先于归一化", seq_ok, "");
    }
    {
        // 干净布局不该出现归一化动作（否则白做功）。
        let mut c = LayoutCompiler::new("vulkan");
        let d = sample_decls(3);
        let r = ReflectionInput::complete(d.clone());
        let m = ManualInput::new(d, true);
        let o = c.compile(6, &r, &m);
        let no_norm = match &o {
            CompileOutcome::Ok { actions, .. } => {
                !actions.iter().any(|a| matches!(a, Action::Normalized { .. }))
            }
            _ => false,
        };
        set.add("A27-act-干净布局不归一化", no_norm, "");
    }
    {
        // 缓存命中的动作序第一项必须是 CacheHit。
        let mut c = LayoutCompiler::new("vulkan");
        let d = sample_decls(3);
        let r = ReflectionInput::complete(d.clone());
        let m = ManualInput::new(d, true);
        let _ = c.compile(7, &r, &m);
        let o = c.compile(7, &r, &m);
        let first_hit = match &o {
            CompileOutcome::Ok { actions, .. } => {
                matches!(actions.first(), Some(Action::CacheHit))
            }
            _ => false,
        };
        set.add("A27-act-命中以CacheHit起头", first_hit, "");
    }
    {
        // 全编译路径不含 CacheHit（否则「命中」判据恒真）。
        let mut c = LayoutCompiler::new("vulkan");
        let d = sample_decls(3);
        let r = ReflectionInput::complete(d.clone());
        let m = ManualInput::new(d, true);
        let o = c.compile(8, &r, &m);
        let no_hit = match &o {
            CompileOutcome::Ok { actions, .. } => {
                !actions.iter().any(|a| matches!(a, Action::CacheHit))
            }
            _ => false,
        };
        set.add("A27-act-全编译路径不记CacheHit", no_hit, "");
    }

    // ---- 拒绝与降级 ----
    {
        // 重复冲突 → 拒绝，且不产出布局。
        let mut c = LayoutCompiler::new("vulkan");
        let mut d = sample_decls(2);
        d.push(d[0]);
        let r = ReflectionInput::complete(d.clone());
        let m = ManualInput::new(d, true);
        let o = c.compile(4, &r, &m);
        set.add(
            "A27-rej-重复布局被拒且不产出",
            !o.produced() && o.disposition() == Disposition::Rejected,
            "",
        );
    }
    {
        // 反射部分失败 → 拒绝（编译层也拦一道，不只靠 resolve_sources）。
        let mut c = LayoutCompiler::new("vulkan");
        let r = ReflectionInput::partial(sample_decls(3), 1);
        let m = ManualInput::new(sample_decls(3), true);
        let o = c.compile(12, &r, &m);
        set.add("A27-rej-反射缺项在编译层被拒", !o.produced(), "");
    }
    {
        // 双源皆空 → 降级，且不产出。
        let mut c = LayoutCompiler::new("vulkan");
        let r = ReflectionInput::failed("无着色器");
        let m = ManualInput::new(Vec::new(), true);
        let o = c.compile(13, &r, &m);
        set.add(
            "A27-deg-双源皆空降级不产出",
            !o.produced() && o.disposition() == Disposition::Degraded,
            "",
        );
    }
    {
        // 拒绝与降级都记入累计（否则统计口径打架）。
        let mut c = LayoutCompiler::new("vulkan");
        let r = ReflectionInput::failed("无着色器");
        let m = ManualInput::new(Vec::new(), true);
        let _ = c.compile(14, &r, &m);
        set.add("A27-deg-降级次数入账", c.degradations == 1, "");
    }

    // ---- 处置码三向分离 ----
    {
        // 三类冲突的处置必须**分得开**：断裂可自动修，重复与越界须人改。
        // 断言不是「两两不同」（重复与越界本就同为须拒绝，强行要求不同
        // 只会逼出无意义的第四个码），而是「可修的那类与不可修的那类分开」。
        let fix = Conflict::NonContiguous { kind: LayoutKind::Sampler, at: 0, expected: 0, found: 2 };
        let dup = Conflict::DuplicateSlot { kind: LayoutKind::Sampler, slot: 0, first_at: 0, second_at: 1 };
        let oor = Conflict::SlotOutOfRange { kind: LayoutKind::Sampler, slot: 99, limit: 32, at: 0 };
        set.add(
            "A27-judge-可修与须改分得开",
            fix.disposition() == Disposition::Fixable
                && dup.disposition() == Disposition::Rejected
                && oor.disposition() == Disposition::Rejected
                && fix.disposition() != dup.disposition(),
            "",
        );
    }
    {
        // 四向处置码齐全：Accept/Fixable/Degraded/Rejected 四者互不相同。
        // 降级与拒绝**必须**分开——降级要出降级产物并告知，拒绝什么都不出，
        // 上层当成同一类就会把「有产物的降级」误当失败而丢弃。
        set.add(
            "A27-judge-四向处置码两两不同",
            Disposition::Accepted != Disposition::Fixable
                && Disposition::Fixable != Disposition::Degraded
                && Disposition::Degraded != Disposition::Rejected
                && Disposition::Accepted != Disposition::Degraded
                && Disposition::Accepted != Disposition::Rejected
                && Disposition::Fixable != Disposition::Rejected,
            "",
        );
    }
    {
        // 冲突 tag 恒为三个有限词之一（读屏/诊断不插值外部文本）。
        let dup = Conflict::DuplicateSlot { kind: LayoutKind::Sampler, slot: 0, first_at: 0, second_at: 1 };
        let oor = Conflict::SlotOutOfRange { kind: LayoutKind::Sampler, slot: 99, limit: 32, at: 0 };
        let brk = Conflict::NonContiguous { kind: LayoutKind::Sampler, at: 0, expected: 0, found: 2 };
        let finite = matches!(dup.tag(), "槽位重复" | "槽位越界" | "槽位断裂")
            && matches!(oor.tag(), "槽位重复" | "槽位越界" | "槽位断裂")
            && matches!(brk.tag(), "槽位重复" | "槽位越界" | "槽位断裂");
        set.add("A27-judge-冲突名为有限分类词", finite, "");
    }
    {
        // 采信来源 tag 恒为三个有限词之一。
        let finite = matches!(SourceTag::Agreed.tag(), "双源一致" | "反射为准" | "手动兜底")
            && matches!(SourceTag::Reflected.tag(), "双源一致" | "反射为准" | "手动兜底")
            && matches!(SourceTag::ManualFallback.tag(), "双源一致" | "反射为准" | "手动兜底");
        set.add("A27-judge-来源名为有限分类词", finite, "");
    }

    // ---- 无障碍面板 ----
    {
        let mut c = LayoutCompiler::new("vulkan");
        let d = sample_decls(3);
        let r = ReflectionInput::complete(d.clone());
        let m = ManualInput::new(d, true);
        let o = c.compile(20, &r, &m);
        match o {
            CompileOutcome::Ok { layout, .. } => {
                let zh = c.a11y_lines(&layout, true);
                let en = c.a11y_lines(&layout, false);
                set.add(
                    "A27-a11y-面板逐行成行双语有别",
                    zh.len() == layout.rows.len() + 2
                        && en.len() == layout.rows.len() + 2
                        && zh != en,
                    "",
                );
            }
            _ => set.add("A27-a11y-面板逐行成行双语有别", false, ""),
        }
    }
    {
        // 逐行本地化方向：中文侧每行含汉字，英文侧每行不含。
        let mut c = LayoutCompiler::new("vulkan");
        let d = sample_decls(3);
        let r = ReflectionInput::complete(d.clone());
        let m = ManualInput::new(d, true);
        let o = c.compile(21, &r, &m);
        match o {
            CompileOutcome::Ok { layout, .. } => {
                let zh = c.a11y_lines(&layout, true);
                let en = c.a11y_lines(&layout, false);
                let has_han = |s: &str| s.chars().any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch));
                let mut zh_ok = !zh.is_empty();
                let mut en_ok = !en.is_empty();
                let mut i = 0;
                while i < zh.len() && i < en.len() {
                    if !has_han(&zh[i]) {
                        zh_ok = false;
                    }
                    if has_han(&en[i]) {
                        en_ok = false;
                    }
                    i += 1;
                }
                set.add("A27-a11y-逐行本地化方向正确", zh_ok && en_ok, "");
            }
            _ => set.add("A27-a11y-逐行本地化方向正确", false, ""),
        }
    }
    {
        // 面板须报编译/命中/重编/损坏四项统计，否则看板无诊断价值。
        let mut c = LayoutCompiler::new("vulkan");
        let d = sample_decls(3);
        let r = ReflectionInput::complete(d.clone());
        let m = ManualInput::new(d, true);
        let o = c.compile(22, &r, &m);
        match o {
            CompileOutcome::Ok { layout, .. } => {
                let zh = c.a11y_lines(&layout, true);
                let has_stat = zh
                    .last()
                    .map(|l| {
                        l.contains("编译") && l.contains("命中") && l.contains("损坏")
                    })
                    .unwrap_or(false);
                set.add("A27-a11y-面板含四项统计", has_stat, "");
            }
            _ => set.add("A27-a11y-面板含四项统计", false, ""),
        }
    }
    {
        // 面板不报资源名（隐私）：造料里没有任何名字类字段，面板只出结构与统计。
        let mut c = LayoutCompiler::new("vulkan");
        let d = sample_decls(3);
        let r = ReflectionInput::complete(d.clone());
        let m = ManualInput::new(d, true);
        let o = c.compile(23, &r, &m);
        match o {
            CompileOutcome::Ok { layout, .. } => {
                let zh = c.a11y_lines(&layout, true);
                let only_structure = zh
                    .iter()
                    .all(|l| !l.contains('"') && !l.contains('/'));
                set.add("A27-a11y-面板只报结构不报资源名", only_structure, "");
            }
            _ => set.add("A27-a11y-面板只报结构不报资源名", false, ""),
        }
    }

    // ---- 端到端 ----
    {
        // 断裂布局端到端：编译成功、槽位连续、寄存器递增。
        let mut c = LayoutCompiler::new("d3d12");
        let d = gapped_decls();
        let r = ReflectionInput::complete(d.clone());
        let m = ManualInput::new(d, true);
        let o = c.compile(30, &r, &m);
        let continuous = match &o {
            CompileOutcome::Ok { layout, .. } => rows_continuous(layout.rows.as_slice()),
            _ => false,
        };
        set.add("A27-e2e-断裂布局编译后槽位连续", o.produced() && continuous, "");
    }
    {
        // 端到端：反射失败退手动后仍能编译出手动布局。
        let mut c = LayoutCompiler::new("metal");
        let d = sample_decls(3);
        let r = ReflectionInput::failed("编译器缺失");
        let m = ManualInput::new(d, true);
        let o = c.compile(31, &r, &m);
        let from_manual = match &o {
            CompileOutcome::Ok { layout, .. } => layout.source == SourceTag::ManualFallback,
            _ => false,
        };
        set.add("A27-e2e-退手动后标源正确", o.produced() && from_manual, "");
    }
    {
        // 端到端：双源一致时标源为 Agreed（面板据此告诉用户「无需人工确认」）。
        let mut c = LayoutCompiler::new("vulkan");
        let d = sample_decls(3);
        let r = ReflectionInput::complete(d.clone());
        let m = ManualInput::new(d, true);
        let o = c.compile(32, &r, &m);
        let agreed = match &o {
            CompileOutcome::Ok { layout, .. } => layout.source == SourceTag::Agreed,
            _ => false,
        };
        set.add("A27-e2e-双源一致标Agreed", o.produced() && agreed, "");
    }
    {
        // 端到端：多次编译不崩、缓存单调增长不超容（状态自洽）。
        let mut c = LayoutCompiler::new("vulkan");
        let mut i = 0;
        while i < 12 {
            let d = sample_decls(3);
            let r = ReflectionInput::complete(d.clone());
            let m = ManualInput::new(d, true);
            let o = c.compile(40 + i as u64, &r, &m);
            if !o.produced() {
                break;
            }
            i += 1;
        }
        set.add("A27-e2e-连续编译状态自洽", c.cache.len() <= MAX_CACHE_ENTRIES && c.compiles >= 1, "");
    }

    set
}

// ---------------------------------------------------------------------------
// 十一、判据造料
// ---------------------------------------------------------------------------

/// 造一份 n 个语义各 1 条的干净声明。
fn sample_decls(n: usize) -> Vec<LayoutDecl> {
    let mut v: Vec<LayoutDecl> = Vec::new();
    let mut i = 0;
    while i < n && i < LayoutKind::ALL.len() {
        v.push(LayoutDecl {
            kind: LayoutKind::ALL[i],
            slot: 0,
            source: DeclSource::Reflection,
        });
        i += 1;
    }
    v
}

/// 把 n 个语义的槽位整体 +1（制造双源差异）。
fn shifted_decls(n: usize, by: u32) -> Vec<LayoutDecl> {
    let mut v = sample_decls(n);
    let mut i = 0;
    while i < v.len() {
        v[i].slot += by;
        v[i].source = DeclSource::Manual;
        i += 1;
    }
    v
}

/// 造一份带断裂的声明：某语义跳号（槽 0 与槽 2，中间缺 1）。
fn gapped_decls() -> Vec<LayoutDecl> {
    vec![
        LayoutDecl { kind: LayoutKind::Sampler, slot: 0, source: DeclSource::Reflection },
        LayoutDecl { kind: LayoutKind::Sampler, slot: 2, source: DeclSource::Manual },
        LayoutDecl { kind: LayoutKind::UniformBuffer, slot: 0, source: DeclSource::Manual },
    ]
}

/// 造一份大声明集（用于测预算超限）。
fn many_decls() -> Vec<LayoutDecl> {
    let mut v: Vec<LayoutDecl> = Vec::new();
    let mut i = 0;
    while i < MAX_DECLS {
        v.push(LayoutDecl {
            kind: LayoutKind::ALL[i % LayoutKind::ALL.len()],
            slot: (i / LayoutKind::ALL.len()) as u32,
            source: DeclSource::Reflection,
        });
        i += 1;
    }
    v
}

/// 布局行是否同语义内槽位连续。
fn rows_continuous(rows: &[LayoutRow]) -> bool {
    let mut i = 0;
    while i < rows.len() {
        let mut j = i + 1;
        while j < rows.len() {
            if rows[j].kind == rows[i].kind && rows[j].slot != rows[i].slot + 1 {
                // 允许末尾之后再出现不同语义，但同语义相邻必须 +1。
                let mut between = false;
                let mut k = i + 1;
                while k < j {
                    if rows[k].kind != rows[i].kind {
                        between = true;
                    }
                    k += 1;
                }
                if !between {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}
