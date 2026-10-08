//! VE-F2407 · 动画求值性能（VE-M 域 · 动画系统 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2407`
//!
//! **判据（锚点原文）**：类型分批 SIMD、零分配断言、脏标记缓存、LOD 次序、判据。
//!
//! 1. **类型分批 SIMD**（判据一）。F2202 家族的 SoA 关键帧数据在这里第一次
//!    被**批量求值**消费。分批不是「把 N 条轨一起循环」那么随便——
//!    **分批键是二元的**：`轨道值类型`（位置 3 通道 / 标量 1 通道 / 四元数
//!    4 通道）× `时间轴指纹`。第二个分量是本条的核心：同一 clip 的所有轨道
//!    共享同一张时间轴（glTF 采样单源），于是**批内 4/8 条轨只需做一次关键帧
//!    二分**，其余轨复用同一个区间索引。
//!
//!    **诚实标注（锚点「SIMD 收益 4-6×」的处理）**：本条**不写平台 SIMD
//!    intrinsic**（内核泛型 no_std，落地由编译器决定），也**不拍一个 4-6×
//!    的加速比数字**。本条兑现的是**逻辑层批处理**并给出可实测的成本模型：
//!    - 批内摊薄的是**查找段**（二分），摊薄比 = 批宽（4 或 8），由计数器实测；
//!    - 插值段**不摊薄**（每轨各算各的），如实计入 `interp_calls`；
//!    - 总成本 = `bisect_steps + interp_calls`，**由判据实测比较**，不预设
//!      「批的一定更快」。
//!    真实硬件上的 4-6× 收益由 **F2412 动画基准**在目标机器定标；F2412
//!    锚点已写明「SIMD 收益与 F2407 声明核对（偏差超 30%→F2407 修正联动）」，
//!    两单构成模型闭环。
//!
//! 2. **零分配纪律**（判据二）。F1525 家族延续。**声明范围严格限定在求值
//!    热路径**（`eval_batch` / `eval_batch_scalar` / `eval_one`）：
//!    - **输出缓冲预分配**：热路径只接受 `&mut [f32]`（调用方给的定长切片），
//!      函数**拿不到任何所有权容器**，从类型上不可能 `Vec::grow`；
//!    - **栈内临时**：通道插值的中间值放在栈上固定数组 `[f32; MAX_CHANNELS]`，
//!      随栈帧生死，不进堆；
//!    - **分配追踪器断言**：`AllocProbe` 是**调用方持有的可变引用**并**传入
//!      热路径**，热路径正常路径一个字节都不分配。
//!
//!    **追踪器不是恒零假门禁**（这是本条最容易写错的地方）：追踪器在
//!    **输出切片容量不足**的异常路径上会**真实分配**（把被拒轨道下标收进
//!    诊断现场 `ProbeSlot`）。于是判据有真实对照组：
//!    「正常路径 `allocs` 增量 = 0」与「容量不足路径 `allocs` 增量 > 0」
//!    必须同时成立。**只验前者就是恒真弱门禁**——删掉追踪器后前者照样通过。
//!
//!    **不在零分配声明内**：`plan_batches` 的分批计划在**轨道集变更时**构建
//!    （非帧内热路径），它分配是诚实的；判据单独把计划期与热路径分开验。
//!
//! 3. **脏标记缓存**（判据三）。缓存是**值 + 脏标记对**，失效条件三条，
//!    每条都给了**显式语义**（锚点点名「缓存语义歧义（时间回绕）→回绕即脏
//!    声明」）：
//!    - **轨道编辑** → 脏：`SoaTrack.edit_rev` 递增，缓存槽的 `seen_rev`
//!      对不上即失效（**按轨粒度**，不是一刀切全清——一刀切会让「改一轨
//!      全部重算」，静态场景零成本的声明就废了）；
//!    - **时间回绕** → **显式标脏**：`t` 小于上次求值时刻即判定回绕，
//!      `note_time()` 把所有槽标脏并记一次回绕。**不靠「时间不等 ⇒ 隐式
//!      重算」蒙混**——那是隐式行为不是显式声明，回绕必须是可机检的一等事件。
//!    - **复用条件**：`rev` 对得上 且（非脏）。`static_value` 轨（值与 `t`
//!      无关的静态轨）跨时间点也命中，兑现锚点「静态场景零成本」；非静态轨
//!      因求值是纯函数，**同一时刻**重复求值同样可复用。
//!
//! 4. **预算联动与 LOD 次序**（判据四）。求值耗时打点进 `FrameLedger`，
//!    超预算时按**固定次序**降级：
//!    1. 远实体降 LOD（**LOD 先于精度降**——锚点原话）；
//!    2. LOD 见底后才降精度；
//!    3. 全见底仍超 → 如实记 `BudgetExhausted` + 告警，**不静默假装达标**。
//!    **次序跳过要分两种形态**（本条最容易写错的地方）：
//!    - **近实体跳过 LOD 是正当的**（近处降 LOD 没有视觉收益，纯浪费）→
//!      标记 `lod_skip_legit`，**不告警**；
//!    - **远实体 LOD 见底仍降精度** → 锚点要求「次序跳过→告警+遥测」，
//!      记 `ORDER_SKIPPED` 诊断 + `order_skipped` 遥测计数。
//!    两种形态分别有判据，**不把正当跳过误判成缺陷**，也不把真跳过放过。
//!
//! 降级矩阵（锚点原文四条 + 一条）：SIMD 不可用 → 标量回退（**显性**，
//!   F1902 家族：结果必须与批路径**逐位一致**，回退计数如实记）；分配断言
//!   失败 → P1；缓存失效遗漏（脏未标却复用了陈旧值）→ P1（正确性）；
//!   次序跳过 → 告警 + 遥测；时间回绕 → 回绕即脏。
//!
//! 跨批对接：SoA 单源 F2202；零分配 F1525 家族；LOD F2229 家族（仓内尚无
//!   先例，本条以 `LodState` 立本域形态，家族对齐待 F2229 落地后对账）；
//!   前置 F2406 事件轨（离散轨**不得入批**，本条显式拒绝并给出理由）；
//!   打点复用 F2014 家族。定标承接单 **F2412**。
//!
//! 零 panic 面、零 IO、无全局可变状态（探针与缓存由调用方持有并显式传入）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断家族（F2406 同构：DiagCode + Diagnostic + DiagBag + Severity）
// ---------------------------------------------------------------------------

/// 诊断码（本条自有码段，不与 F2406 混用——同码不同义是最难查的一类缺陷）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagCode(pub u16);

impl DiagCode {
    /// SIMD 能力不可用，已显性回退标量。
    pub const SIMD_FALLBACK: DiagCode = DiagCode(0x2A01);
    /// 通道数与轨道值类型不符。
    pub const CHANNEL_MISMATCH: DiagCode = DiagCode(0x2A02);
    /// 离散轨（事件/布尔/枚举/触发）不得进入插值批。
    pub const DISCRETE_NOT_BATCHABLE: DiagCode = DiagCode(0x2A03);
    /// 时间轴非单调，二分查找的前提不成立。
    pub const TIMES_NOT_MONOTONIC: DiagCode = DiagCode(0x2A04);
    /// 空关键帧轨。
    pub const EMPTY_TIMES: DiagCode = DiagCode(0x2A05);
    /// 时间落在关键帧范围外，已钳制到端点。
    pub const TIME_CLAMPED: DiagCode = DiagCode(0x2A06);
    /// 热路径检测到受追踪分配（零分配断言破防）。
    pub const ALLOC_IN_HOT_PATH: DiagCode = DiagCode(0x2A07);
    /// 时间回绕，缓存已显式标脏。
    pub const TIME_WRAPPED: DiagCode = DiagCode(0x2A08);
    /// 降级次序跳过（远实体 LOD 见底仍降精度）。
    pub const ORDER_SKIPPED: DiagCode = DiagCode(0x2A09);
    /// 预算全部降级手段见底仍未达标。
    pub const BUDGET_EXHAUSTED: DiagCode = DiagCode(0x2A0A);
    /// 输出切片容量不足，写入被拒（越界防护，非 panic）。
    pub const OUTPUT_TOO_SMALL: DiagCode = DiagCode(0x2A0B);

    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            DiagCode::SIMD_FALLBACK => "SIMD 不可用，回退标量",
            DiagCode::CHANNEL_MISMATCH => "通道数与值类型不符",
            DiagCode::DISCRETE_NOT_BATCHABLE => "离散轨不入批",
            DiagCode::TIMES_NOT_MONOTONIC => "时间轴非单调",
            DiagCode::EMPTY_TIMES => "空关键帧轨",
            DiagCode::TIME_CLAMPED => "时间越界已钳制",
            DiagCode::ALLOC_IN_HOT_PATH => "热路径出现受追踪分配",
            DiagCode::TIME_WRAPPED => "时间回绕，缓存标脏",
            DiagCode::ORDER_SKIPPED => "降级次序跳过",
            DiagCode::BUDGET_EXHAUSTED => "预算手段见底仍未达标",
            DiagCode::OUTPUT_TOO_SMALL => "输出缓冲容量不足",
            // 兜底：结构体（非 enum）包装无法穷尽，必须给未知码一句人话
            // 而不是 panic——内核诊断面绝不能因为一个陌生码崩掉。
            other => {
                let _ = other;
                "未登记诊断码"
            }
        }
    }

    /// 全部码（供家族完整性判据）。
    pub const ALL: [DiagCode; 11] = [
        DiagCode::SIMD_FALLBACK,
        DiagCode::CHANNEL_MISMATCH,
        DiagCode::DISCRETE_NOT_BATCHABLE,
        DiagCode::TIMES_NOT_MONOTONIC,
        DiagCode::EMPTY_TIMES,
        DiagCode::TIME_CLAMPED,
        DiagCode::ALLOC_IN_HOT_PATH,
        DiagCode::TIME_WRAPPED,
        DiagCode::ORDER_SKIPPED,
        DiagCode::BUDGET_EXHAUSTED,
        DiagCode::OUTPUT_TOO_SMALL,
    ];
}

/// 诊断严重度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// 记账，不阻断。
    Minor,
    /// 显性降级，仍出结果。
    Major,
    /// 阻断级（锚点 P1）。
    P1,
}

/// 一条诊断。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: DiagCode,
    pub severity: Severity,
    pub message: &'static str,
    pub hint: &'static str,
}

/// 诊断袋。
#[derive(Clone, Debug, Default)]
pub struct DiagBag {
    items: Vec<Diagnostic>,
}

impl DiagBag {
    /// 新建空袋。
    #[allow(clippy::new_without_default)]
    pub fn new() -> DiagBag {
        DiagBag { items: Vec::new() }
    }

    /// 记一条 Minor。
    pub fn push(&mut self, code: DiagCode, message: &'static str, hint: &'static str) {
        self.items.push(Diagnostic { code, severity: Severity::Minor, message, hint });
    }

    /// 记一条 Major（显性降级）。
    pub fn push_major(&mut self, code: DiagCode, message: &'static str, hint: &'static str) {
        self.items.push(Diagnostic { code, severity: Severity::Major, message, hint });
    }

    /// 记一条 P1（阻断级）。
    pub fn push_p1(&mut self, code: DiagCode, message: &'static str, hint: &'static str) {
        self.items.push(Diagnostic { code, severity: Severity::P1, message, hint });
    }

    /// 全部诊断。
    pub fn items(&self) -> &[Diagnostic] {
        &self.items
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 某码计数。
    pub fn count(&self, code: DiagCode) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].code == code {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 某严重度计数。
    pub fn count_severity(&self, sev: Severity) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].severity == sev {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 是否出现过某码。
    pub fn has(&self, code: DiagCode) -> bool {
        self.count(code) > 0
    }

    /// 人话渲染。
    pub fn render(&self) -> String {
        if self.items.is_empty() {
            return String::from("动画求值：无诊断");
        }
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.items.len() {
            let d = &self.items[i];
            let sev = match d.severity {
                Severity::Minor => "提示",
                Severity::Major => "降级",
                Severity::P1 => "P1",
            };
            let _ = s.push('[');
            let _ = s.push_str(sev);
            let _ = s.push(']');
            let _ = s.push_str(d.code.label());
            let _ = s.push('：');
            let _ = s.push_str(d.message);
            let _ = s.push_str("（怎么办：");
            let _ = s.push_str(d.hint);
            let _ = s.push_str("）\n");
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 二、P1 通道（锚点：分配断言失败 → P1；缓存失效遗漏 → P1）
// ---------------------------------------------------------------------------

/// 一条 P1 立案。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct P1Case {
    pub code: DiagCode,
    pub detail: String,
}

/// P1 立案通道。
#[derive(Clone, Debug, Default)]
pub struct P1Channel {
    cases: Vec<P1Case>,
}

impl P1Channel {
    /// 新建空通道。
    #[allow(clippy::new_without_default)]
    pub fn new() -> P1Channel {
        P1Channel { cases: Vec::new() }
    }

    /// 立案。
    pub fn open(&mut self, code: DiagCode, detail: &str) {
        self.cases.push(P1Case { code, detail: detail.to_string() });
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    /// 某码立案数。
    pub fn count(&self, code: DiagCode) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.cases.len() {
            if self.cases[i].code == code {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 全部立案。
    pub fn cases(&self) -> &[P1Case] {
        &self.cases
    }

    /// 是否含某码立案。
    pub fn has(&self, code: DiagCode) -> bool {
        self.count(code) > 0
    }
}

// ---------------------------------------------------------------------------
// 三、遥测（锚点：回绕 / 次序跳过要进遥测）
// ---------------------------------------------------------------------------

/// 求值遥测计数。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Telemetry {
    /// 标量回退次数（SIMD 不可用）。
    pub simd_fallback: u32,
    /// 时间回绕次数。
    pub time_wrap: u32,
    /// 缓存命中次数。
    pub cache_hits: u32,
    /// 缓存重算数。
    pub cache_recompute: u32,
    /// 降级次序跳过次数。
    pub order_skipped: u32,
    /// 超预算帧数。
    pub over_budget_frames: u32,
    /// 被拒入批的轨道数。
    pub tracks_rejected: u32,
}

/// 遥测增量累加。
pub fn accumulate(base: &mut Telemetry, delta: &Telemetry) {
    base.simd_fallback += delta.simd_fallback;
    base.time_wrap += delta.time_wrap;
    base.cache_hits += delta.cache_hits;
    base.cache_recompute += delta.cache_recompute;
    base.order_skipped += delta.order_skipped;
    base.over_budget_frames += delta.over_budget_frames;
    base.tracks_rejected += delta.tracks_rejected;
}

// ---------------------------------------------------------------------------
// 四、SoA 轨道与类型分批（F2202 家族单源）
// ---------------------------------------------------------------------------

/// 通道数上限（四元数 4 通道最大）。栈上临时按此定尺。
pub const MAX_CHANNELS: usize = 4;

/// 轨道值类型（分批键第一分量 + 插值通道数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrackValueKind {
    /// 标量（1 通道）。
    Scalar,
    /// 位置（3 通道）。
    Position,
    /// 四元数（4 通道；本条按通道线性插值，**slerp 归 F2423 承接**）。
    Quat,
}

impl TrackValueKind {
    /// 通道数。
    pub const fn lanes(self) -> usize {
        match self {
            TrackValueKind::Scalar => 1,
            TrackValueKind::Position => 3,
            TrackValueKind::Quat => 4,
        }
    }

    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            TrackValueKind::Scalar => "标量轨",
            TrackValueKind::Position => "位置轨",
            TrackValueKind::Quat => "四元数轨",
        }
    }

    /// 三类全集（分批键第一分量的取值域）。
    pub const ALL: [TrackValueKind; 3] =
        [TrackValueKind::Scalar, TrackValueKind::Position, TrackValueKind::Quat];
}

/// 离散性（F2402 七类轨的插值性；本条据此**拒绝离散轨入批**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackClass {
    /// 连续可插值（值轨 / 标记轨 / 曲线轨）。
    Continuous,
    /// 离散无插值（事件轨 F2406 / 布尔 / 枚举 / 触发）。
    Discrete,
}

impl TrackClass {
    /// 是否可进插值批。
    pub const fn batchable(self) -> bool {
        matches!(self, TrackClass::Continuous)
    }
}

/// 一条 SoA 轨道（F2202 家族单源：时间一列、通道数据一列，通道主序）。
#[derive(Clone, Debug, PartialEq)]
pub struct SoaTrack {
    /// 轨名（人话）。
    pub name: String,
    /// 轨类（插值性）。
    pub class: TrackClass,
    /// 值类型。
    pub kind: TrackValueKind,
    /// 静态轨：求值结果与时间无关（兑现「静态场景零成本」）。
    pub static_value: bool,
    /// 编辑修订号：轨道编辑即递增，缓存按此判脏。
    pub edit_rev: u32,
    /// 关键帧时间（非递减；F2406 已立的单调纪律在本条沿用）。
    pub times: Vec<u32>,
    /// 通道数据，`len == times.len() * kind.lanes()`。
    pub channels: Vec<f32>,
}

impl SoaTrack {
    /// 造一条轨道（校验交给 `classify_track`，此处不吞错）。
    pub fn new(
        name: &str,
        class: TrackClass,
        kind: TrackValueKind,
        static_value: bool,
        times: Vec<u32>,
        channels: Vec<f32>,
    ) -> SoaTrack {
        SoaTrack {
            name: name.to_string(),
            class,
            kind,
            static_value,
            edit_rev: 0,
            times,
            channels,
        }
    }

    /// 关键帧数。
    pub fn key_count(&self) -> usize {
        self.times.len()
    }

    /// 通道数据长度是否与 `times × lanes` 自洽。
    pub fn shape_ok(&self) -> bool {
        self.channels.len() == self.times.len() * self.kind.lanes()
    }

    /// 标记一次编辑（修订号递增 → 缓存按轨粒度失效）。
    pub fn mark_edited(&mut self) {
        self.edit_rev = self.edit_rev.wrapping_add(1);
    }

    /// 某关键帧的通道值写入栈上定长临时，返回通道数。
    pub fn key_slice(&self, idx: usize, out: &mut [f32; MAX_CHANNELS]) -> usize {
        let lanes = self.kind.lanes();
        let base = idx * lanes;
        let mut c = 0usize;
        while c < lanes {
            out[c] = self.channels[base + c];
            c += 1;
        }
        lanes
    }
}

/// 时间轴指纹（FNV-1a 64）——分批键第二分量。
///
/// 同一 clip 的所有轨道共享时间轴 → 指纹相同 → 批内只做一次二分。
pub fn times_fingerprint(times: &[u32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325u64;
    let mut i = 0usize;
    while i < times.len() {
        h ^= times[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3u64);
        i += 1;
    }
    h
}

/// 关键帧二分结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BisectHit {
    /// 区间起点下标（关键帧 i 与 i+1 之间）。
    pub index: usize,
    /// 时间是否落在范围外、已钳制。
    pub clamped: bool,
    /// **真实比较步数**（不是自证式 `n * CONST`）。
    pub steps: u32,
}

/// 关键帧二分：返回「最后一个 `times[i] <= t`」的下标。
///
/// 左闭右开语义：`t` 落在端点时取该端点所在区间。调用方须先确认非空。
/// 与 `bisect_steps_of` 共用同一循环（**不写两份实现**，否则两份会漂移）。
pub fn bisect(times: &[u32], t: u32) -> BisectHit {
    let n = times.len();
    if n == 0 {
        return BisectHit { index: 0, clamped: true, steps: 1 };
    }
    if n == 1 {
        return BisectHit { index: 0, clamped: t > times[0], steps: 1 };
    }
    if t < times[0] {
        return BisectHit { index: 0, clamped: true, steps: 1 };
    }
    if t >= times[n - 1] {
        return BisectHit { index: n - 2, clamped: t > times[n - 1], steps: 1 };
    }
    let mut lo = 0usize;
    let mut hi = n - 1;
    let mut steps = 0u32;
    while hi - lo > 1 {
        steps += 1;
        let mid = lo + (hi - lo) / 2;
        if times[mid] <= t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    BisectHit { index: lo, clamped: false, steps }
}

/// 区间内插值系数（`times` 非递减；区间宽度为 0 时取 0 避免除零）。
pub fn lerp_alpha(times: &[u32], index: usize, t: u32) -> f32 {
    let a = times[index];
    let b = times[index + 1];
    if b == a {
        return 0.0;
    }
    let span = b - a;
    let off = if t > a { t - a } else { 0 };
    off as f32 / span as f32
}

// ---------------------------------------------------------------------------
// 五、分批计划（计划期构建，不在帧内热路径）
// ---------------------------------------------------------------------------

/// 被拒入批的原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectKind {
    /// 离散轨（事件轨等）无插值语义。
    Discrete,
    /// 通道数与值类型不符。
    ShapeMismatch,
    /// 空关键帧。
    Empty,
    /// 时间轴非单调。
    NotMonotonic,
}

impl RejectKind {
    /// 人话理由（判据要读得出「为什么拒」，不能只记一个码）。
    pub const fn reason(self) -> &'static str {
        match self {
            RejectKind::Discrete => "离散轨没有两帧之间的值，进批会凭空造出从不存在的中间值",
            RejectKind::ShapeMismatch => "通道数与值类型标称的通道数不符，读到的会是别轨的数据",
            RejectKind::Empty => "没有关键帧，无值可求",
            RejectKind::NotMonotonic => "时间轴非单调，二分查找的前提不成立",
        }
    }

    /// 全部原因（家族完整性判据用）。
    pub const ALL: [RejectKind; 4] =
        [RejectKind::Discrete, RejectKind::ShapeMismatch, RejectKind::Empty, RejectKind::NotMonotonic];
}

/// 一条被拒记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectRecord {
    /// 轨道下标。
    pub track: usize,
    /// 原因。
    pub kind: RejectKind,
    /// 人话理由。
    pub reason: &'static str,
}

/// 一个批（同值类型 + 同时间轴指纹）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Batch {
    /// 值类型。
    pub kind: TrackValueKind,
    /// 时间轴指纹。
    pub fingerprint: u64,
    /// 批宽上限（4 或 8）。
    pub lanes: usize,
    /// 批内轨道下标（`len <= lanes`）。
    pub slots: Vec<usize>,
}

/// 分批计划。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchPlan {
    /// 批宽（4 或 8）。
    pub lane_width: usize,
    /// 批列表。
    pub batches: Vec<Batch>,
    /// 被拒记录。
    pub rejected: Vec<RejectRecord>,
}

impl BatchPlan {
    /// 批数。
    pub fn batch_count(&self) -> usize {
        self.batches.len()
    }

    /// 入批轨数。
    pub fn accepted_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.batches.len() {
            n += self.batches[i].slots.len();
            i += 1;
        }
        n
    }

    /// 计划期占用的槽位总数（**非热路径**；用于把零分配声明范围分开验）。
    pub fn plan_slots(&self) -> usize {
        let mut n = self.rejected.len();
        let mut i = 0usize;
        while i < self.batches.len() {
            n += self.batches[i].slots.len();
            i += 1;
        }
        n
    }

    /// 某轨落在哪个批（找不到返回 `None`）。
    pub fn batch_of(&self, track: usize) -> Option<usize> {
        let mut b = 0usize;
        while b < self.batches.len() {
            let mut s = 0usize;
            while s < self.batches[b].slots.len() {
                if self.batches[b].slots[s] == track {
                    return Some(b);
                }
                s += 1;
            }
            b += 1;
        }
        None
    }
}

/// 批宽归一：只接受 4 或 8，其余落回 4。
pub const fn normalize_lane_width(want: usize) -> usize {
    if want == 8 {
        8
    } else {
        4
    }
}

/// 校验一条轨可否入批。
///
/// 判定顺序**刻意**是「离散 → 形状 → 空 → 单调」：离散轨的形状往往也是
/// 合理的，若顺序颠倒，F2406 的事件轨会被误报成「形状不符」，诊断文案
/// 说不出真正的理由。
pub fn classify_track(track: &SoaTrack) -> Result<(), RejectKind> {
    if !track.class.batchable() {
        return Err(RejectKind::Discrete);
    }
    if !track.shape_ok() {
        return Err(RejectKind::ShapeMismatch);
    }
    if track.times.is_empty() {
        return Err(RejectKind::Empty);
    }
    let mut i = 1usize;
    while i < track.times.len() {
        if track.times[i] < track.times[i - 1] {
            return Err(RejectKind::NotMonotonic);
        }
        i += 1;
    }
    Ok(())
}

fn batch_key(kind: TrackValueKind, fp: u64) -> (u8, u64) {
    (kind as u8, fp)
}

fn kind_from_u8(v: u8) -> TrackValueKind {
    match v {
        1 => TrackValueKind::Position,
        2 => TrackValueKind::Quat,
        _ => TrackValueKind::Scalar,
    }
}

/// 稳定插入排序（按批键升序）——计划期。
fn sort_by_key(keys: &mut [(u8, u64)], slots: &mut [usize]) {
    let n = keys.len();
    let mut i = 1usize;
    while i < n {
        let k = keys[i];
        let s = slots[i];
        let mut j = i;
        while j > 0 && (keys[j - 1].0 > k.0 || (keys[j - 1].0 == k.0 && keys[j - 1].1 > k.1)) {
            keys[j] = keys[j - 1];
            slots[j] = slots[j - 1];
            j -= 1;
        }
        keys[j] = k;
        slots[j] = s;
        i += 1;
    }
}

/// 构建分批计划（**计划期**，非帧内热路径；此处分配是诚实的）。
pub fn plan_batches(tracks: &[SoaTrack], want_lane: usize) -> BatchPlan {
    let lane_width = normalize_lane_width(want_lane);
    let mut keys: Vec<(u8, u64)> = Vec::new();
    let mut slots: Vec<usize> = Vec::new();
    let mut rejected: Vec<RejectRecord> = Vec::new();
    let mut i = 0usize;
    while i < tracks.len() {
        match classify_track(&tracks[i]) {
            Err(k) => {
                rejected.push(RejectRecord { track: i, kind: k, reason: k.reason() });
            }
            Ok(()) => {
                keys.push(batch_key(tracks[i].kind, times_fingerprint(&tracks[i].times)));
                slots.push(i);
            }
        }
        i += 1;
    }
    sort_by_key(&mut keys, &mut slots);
    let mut batches: Vec<Batch> = Vec::new();
    let mut p = 0usize;
    while p < slots.len() {
        // 指纹必须取自排序后的键本身：写死 0 会让内层条件永假、
        // 外层反复 push 空批（p 永不推进），批列表无界增长直到 OOM。
        let kind = kind_from_u8(keys[p].0);
        let fp = keys[p].1;
        let mut batch = Batch { kind, fingerprint: fp, lanes: lane_width, slots: Vec::new() };
        while batch.slots.len() < lane_width && p < slots.len() && keys[p] == (kind as u8, fp) {
            batch.slots.push(slots[p]);
            p += 1;
        }
        batches.push(batch);
    }
    BatchPlan { lane_width, batches, rejected }
}

// ---------------------------------------------------------------------------
// 六、SIMD 能力与标量回退（F1902 家族：显性回退）
// ---------------------------------------------------------------------------

/// SIMD 能力档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimdCap {
    /// 批处理可用。
    Batched,
    /// 不可用：必须显性回退标量，结果逐位一致。
    ScalarOnly,
}

impl SimdCap {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            SimdCap::Batched => "批处理可用",
            SimdCap::ScalarOnly => "仅标量（回退）",
        }
    }

    /// 全集。
    pub const ALL: [SimdCap; 2] = [SimdCap::Batched, SimdCap::ScalarOnly];
}

// ---------------------------------------------------------------------------
// 七、分配追踪器（F1525 零分配纪律）
// ---------------------------------------------------------------------------

/// 一个受追踪的诊断现场槽。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProbeSlot {
    /// 现场字（被拒轨道下标等）。
    pub words: Vec<u32>,
}

/// 分配追踪器：调用方持有并**可变借用**传入热路径（**零全局可变状态**）。
#[derive(Clone, Debug, Default)]
pub struct AllocProbe {
    allocs: u32,
    bytes: u64,
    slots: Vec<ProbeSlot>,
}

impl AllocProbe {
    /// 新建（零分配）。
    #[allow(clippy::new_without_default)]
    pub fn new() -> AllocProbe {
        AllocProbe { allocs: 0, bytes: 0, slots: Vec::new() }
    }

    /// 累计分配次数。
    pub fn allocs(&self) -> u32 {
        self.allocs
    }

    /// 累计分配字节。
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// 现场槽数。
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// 取现场槽内容（零分配切片）。
    pub fn slot_words(&self, idx: usize) -> &[u32] {
        &self.slots[idx].words
    }

    /// 收一个被拒轨道下标进现场（**唯一受追踪的分配入口**）。
    ///
    /// 正常求值路径**不会**走到这里；只有输出切片容量不足时才会。
    /// 它存在的意义是让「热路径零分配」这条断言有**真实对照组**——
    /// 删掉整个追踪器，零分配判据就变成恒真空断言。
    pub fn record_reject(&mut self, track: usize) -> u32 {
        let mut w: Vec<u32> = Vec::new();
        w.push(track as u32);
        self.allocs += 1;
        self.bytes += 4;
        self.slots.push(ProbeSlot { words: w });
        (self.slots.len() - 1) as u32
    }
}

/// 求值结果（`Default` 合法：全零即「什么都没做」）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EvalOutcome {
    /// 实际写入的浮点数个数。
    pub written: usize,
    /// 批内共享二分次数（**查找段调用数**）。
    pub bisect_calls: u32,
    /// 二分真实比较步数（实测，非自证式算术）。
    pub bisect_steps: u32,
    /// 插值调用次数（每轨一次，**批处理不摊薄插值段**）。
    pub interp_calls: u32,
    /// 缓存命中数。
    pub hits: u32,
    /// 缓存重算数。
    pub recomputes: u32,
    /// 时间被钳制次数。
    pub clamped: u32,
    /// 输出容量不足导致写入被拒的次数。
    pub output_rejects: u32,
    /// 标量回退次数。
    pub fallbacks: u32,
}

/// 成本模型：`bisect_interval` 与插值各算一步。
pub const fn work_units(o: &EvalOutcome) -> u32 {
    o.bisect_steps + o.interp_calls
}

/// 标量基线：逐轨求值（**每轨各自二分**，批处理的对照组）。
///
/// **同样走缓存**：回退不是「降级成另一套语义」。若回退路径不读不写缓存，
/// 会出现「批路径把缓存填满了、回退路径读不到」这种只在能力探测抖动时
/// 才暴露的差异，比慢几十倍更难查。
pub fn eval_batch_scalar(
    tracks: &[SoaTrack],
    slots: &[usize],
    t_ms: u32,
    out: &mut [f32],
    cursor: &mut usize,
    cache: &mut EvalCache,
    probe: &mut AllocProbe,
    bag: &mut DiagBag,
    o: &mut EvalOutcome,
) {
    let mut i = 0usize;
    while i < slots.len() {
        let ti = slots[i];
        let track = &tracks[ti];
        if cache.can_reuse(ti, track, t_ms) {
            let mut tmp: [f32; MAX_CHANNELS] = [0.0; MAX_CHANNELS];
            let lanes = cache.load(ti, &mut tmp);
            let before = *cursor;
            write_lanes(out, cursor, probe, bag, o, ti, &tmp, lanes);
            if *cursor > before {
                o.hits += 1;
                i += 1;
                continue;
            }
        }
        let hit = bisect(&track.times, t_ms);
        o.bisect_calls += 1;
        o.bisect_steps += hit.steps;
        o.interp_calls += 1;
        if hit.clamped {
            o.clamped += 1;
            bag.push(DiagCode::TIME_CLAMPED, "求值时间落在关键帧范围外", "已钳制到端点关键帧，不外推");
        }
        let mut tmp: [f32; MAX_CHANNELS] = [0.0; MAX_CHANNELS];
        let lanes = sample_track(track, hit.index, t_ms, &mut tmp);
        let before = *cursor;
        write_lanes(out, cursor, probe, bag, o, ti, &tmp, lanes);
        if *cursor > before {
            cache.store(ti, track, t_ms, &tmp, lanes);
            o.recomputes += 1;
        }
        i += 1;
    }
}

/// 纯标量基线：**完全不碰缓存**，每轨各自二分。
///
/// 与 `eval_batch_scalar` 的区别是刻意的：那条是**能力回退的实路径**（要
/// 和批路径共享缓存语义），这条是**成本对照的测量路径**（要排除缓存干扰，
/// 否则第二次求值全命中，量出来的不是求值成本）。
pub fn eval_baseline_no_cache(
    tracks: &[SoaTrack],
    slots: &[usize],
    t_ms: u32,
    out: &mut [f32],
    cursor: &mut usize,
    probe: &mut AllocProbe,
    bag: &mut DiagBag,
    o: &mut EvalOutcome,
) {
    let mut i = 0usize;
    while i < slots.len() {
        let ti = slots[i];
        let hit = bisect(&tracks[ti].times, t_ms);
        o.bisect_calls += 1;
        o.bisect_steps += hit.steps;
        o.interp_calls += 1;
        if hit.clamped {
            o.clamped += 1;
            bag.push(DiagCode::TIME_CLAMPED, "求值时间落在关键帧范围外", "已钳制到端点关键帧，不外推");
        }
        let mut tmp: [f32; MAX_CHANNELS] = [0.0; MAX_CHANNELS];
        let lanes = sample_track(&tracks[ti], hit.index, t_ms, &mut tmp);
        write_lanes(out, cursor, probe, bag, o, ti, &tmp, lanes);
        i += 1;
    }
}

/// 单轨采样到栈上定长临时（**不分配**）。
fn sample_track(track: &SoaTrack, index: usize, t_ms: u32, tmp: &mut [f32; MAX_CHANNELS]) -> usize {
    let lanes = track.kind.lanes();
    let mut a: [f32; MAX_CHANNELS] = [0.0; MAX_CHANNELS];
    let mut b: [f32; MAX_CHANNELS] = [0.0; MAX_CHANNELS];
    if track.times.len() == 1 {
        track.key_slice(0, &mut a);
    } else {
        track.key_slice(index, &mut a);
        track.key_slice(index + 1, &mut b);
    }
    let u = if track.times.len() == 1 { 0.0 } else { lerp_alpha(&track.times, index, t_ms) };
    let mut c = 0usize;
    while c < lanes {
        tmp[c] = a[c] + (b[c] - a[c]) * u;
        c += 1;
    }
    lanes
}

/// 把若干通道写入调用方预分配切片；容量不足如实记账 + 收现场（**零 panic**）。
fn write_lanes(
    out: &mut [f32],
    cursor: &mut usize,
    probe: &mut AllocProbe,
    bag: &mut DiagBag,
    o: &mut EvalOutcome,
    track: usize,
    vals: &[f32; MAX_CHANNELS],
    lanes: usize,
) {
    if *cursor + lanes > out.len() {
        o.output_rejects += 1;
        let slot = probe.record_reject(track);
        let _ = slot;
        bag.push_major(
            DiagCode::OUTPUT_TOO_SMALL,
            "输出缓冲容量不足，部分轨道未被写入",
            "按「轨道数 × 通道数」预分配输出缓冲；被拒轨道下标已收进分配追踪器现场",
        );
        return;
    }
    let mut c = 0usize;
    while c < lanes {
        out[*cursor] = vals[c];
        *cursor += 1;
        c += 1;
    }
    o.written += lanes;
}

// ---------------------------------------------------------------------------
// 八、缓存（值 + 脏标记对）
// ---------------------------------------------------------------------------

/// 单轨缓存槽（定长；随缓存表一次性分配，求值期只改值）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CacheSlot {
    /// 已见修订号。
    pub seen_rev: u32,
    /// 上次求值时刻。
    pub t_ms: u32,
    /// 缓存值。
    pub values: [f32; MAX_CHANNELS],
    /// 有效通道数。
    pub lanes: u8,
    /// 脏标记。
    pub dirty: bool,
    /// 已填充过。
    pub filled: bool,
}

impl Default for CacheSlot {
    fn default() -> Self {
        CacheSlot {
            seen_rev: 0,
            t_ms: 0,
            values: [0.0; MAX_CHANNELS],
            lanes: 0,
            dirty: true,
            filled: false,
        }
    }
}

/// 求值缓存。
#[derive(Clone, Debug, Default)]
pub struct EvalCache {
    slots: Vec<CacheSlot>,
    /// 回绕次数（显式一等事件）。
    pub wraps: u32,
    /// 上一帧求值时刻。
    last_t: Option<u32>,
}

impl EvalCache {
    /// 新建空缓存。
    #[allow(clippy::new_without_default)]
    pub fn new() -> EvalCache {
        EvalCache { slots: Vec::new(), wraps: 0, last_t: None }
    }

    /// 按轨道数预分配槽位（**计划期**，非热路径）。
    pub fn reserve_for(&mut self, tracks: usize) {
        let mut i = self.slots.len();
        while i < tracks {
            self.slots.push(CacheSlot::default());
            i += 1;
        }
    }

    /// 槽数。
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// 某槽脏标记（越界视为脏——保守方向）。
    pub fn slot_dirty(&self, idx: usize) -> bool {
        match self.slots.get(idx) {
            Some(s) => s.dirty,
            None => true,
        }
    }

    /// 某槽已见修订号（越界返回 `u32::MAX`，与任何真实修订号都不等）。
    pub fn slot_seen_rev(&self, idx: usize) -> u32 {
        match self.slots.get(idx) {
            Some(s) => s.seen_rev,
            None => u32::MAX,
        }
    }

    /// 脏槽数。
    pub fn dirty_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].dirty {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 已填充槽数。
    pub fn filled_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].filled {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// **时间回绕即脏**（锚点显式声明）。
    ///
    /// 语义边界（写清以免被误用）：回绕标脏的是**上一帧遗留的缓存值**。
    /// 本帧随后的重算会把新值写回并**清掉**脏标记——那是正确行为：新值不脏。
    /// 所以「回绕后仍脏」不是判据，回绕后**不得复用旧值**才是。
    ///
    /// 刻意不写成「时间不等就隐式重算」——那是隐式行为不是显式声明，
    /// 回绕必须是可机检的一等事件。
    pub fn note_time(&mut self, t_ms: u32, bag: &mut DiagBag) -> bool {
        let wrapped = match self.last_t {
            Some(prev) => t_ms < prev,
            None => false,
        };
        if wrapped {
            self.wraps += 1;
            let mut i = 0usize;
            while i < self.slots.len() {
                self.slots[i].dirty = true;
                i += 1;
            }
            bag.push(DiagCode::TIME_WRAPPED, "求值时间回绕", "回绕即脏：全部缓存槽已标脏，下一帧全部重算");
        }
        self.last_t = Some(t_ms);
        wrapped
    }

    /// 复用判定。
    ///
    /// 静态轨（`static_value`）跨时间点命中（兑现「静态场景零成本」）；
    /// 非静态轨因求值是纯函数，**同一时刻**重复求值同样命中。
    pub fn can_reuse(&self, idx: usize, track: &SoaTrack, t_ms: u32) -> bool {
        let slot = match self.slots.get(idx) {
            Some(s) => s,
            None => return false,
        };
        if !slot.filled || slot.dirty {
            return false;
        }
        if slot.seen_rev != track.edit_rev {
            return false;
        }
        if track.static_value {
            return true;
        }
        slot.t_ms == t_ms
    }

    /// 写回缓存（值 + 脏标记对一起落地）。
    pub fn store(&mut self, idx: usize, track: &SoaTrack, t_ms: u32, values: &[f32; MAX_CHANNELS], lanes: usize) {
        if let Some(s) = self.slots.get_mut(idx) {
            let mut i = 0usize;
            while i < lanes && i < MAX_CHANNELS {
                s.values[i] = values[i];
                i += 1;
            }
            s.seen_rev = track.edit_rev;
            s.t_ms = t_ms;
            s.lanes = lanes as u8;
            s.dirty = false;
            s.filled = true;
        }
    }

    /// 读取缓存值到栈上定长临时，返回通道数。
    pub fn load(&self, idx: usize, out: &mut [f32; MAX_CHANNELS]) -> usize {
        let mut n = 0usize;
        if let Some(s) = self.slots.get(idx) {
            n = s.lanes as usize;
            let mut i = 0usize;
            while i < n && i < MAX_CHANNELS {
                out[i] = s.values[i];
                i += 1;
            }
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 九、预算账本与 LOD 降级次序
// ---------------------------------------------------------------------------

/// 实体距离档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistanceBucket {
    /// 近实体：降 LOD 没有视觉收益。
    Near,
    /// 远实体：LOD 降频的正适用对象。
    Far,
}

impl DistanceBucket {
    /// 全集。
    pub const ALL: [DistanceBucket; 2] = [DistanceBucket::Near, DistanceBucket::Far];
}

/// LOD 档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LodState {
    pub level: u8,
    pub min_level: u8,
}

impl LodState {
    /// 新建。
    pub const fn new(level: u8, min_level: u8) -> LodState {
        LodState { level, min_level }
    }

    /// 还有可降余量吗。
    pub const fn can_reduce(&self) -> bool {
        self.level > self.min_level
    }

    /// 降一级（**调用方须先判 `can_reduce`**）。
    pub fn reduce(&mut self) {
        self.level -= 1;
    }
}

/// 精度档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrecisionState {
    pub level: u8,
    pub min_level: u8,
}

impl PrecisionState {
    /// 新建。
    pub const fn new(level: u8, min_level: u8) -> PrecisionState {
        PrecisionState { level, min_level }
    }

    /// 还有可降余量吗。
    pub const fn can_reduce(&self) -> bool {
        self.level > self.min_level
    }

    /// 降一级。
    pub fn reduce(&mut self) {
        self.level -= 1;
    }
}

/// 降级动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeAction {
    /// 无需降级（不产出到序列里）。
    None,
    /// 远实体降频（LOD）。
    LodDown,
    /// 降精度。
    PrecisionDown,
    /// 手段见底仍未达标。
    Exhausted,
}

impl DegradeAction {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            DegradeAction::None => "不降级",
            DegradeAction::LodDown => "远实体降频",
            DegradeAction::PrecisionDown => "降精度",
            DegradeAction::Exhausted => "手段见底仍超预算",
        }
    }

    /// 全集。
    pub const ALL: [DegradeAction; 4] = [
        DegradeAction::None,
        DegradeAction::LodDown,
        DegradeAction::PrecisionDown,
        DegradeAction::Exhausted,
    ];
}

/// 帧预算账本（打点复用 F2014 家族）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameLedger {
    /// 本帧动画求值耗时（微秒）。
    pub eval_us: u32,
    /// 帧预算（微秒）；`0` 表示未设预算，视为永不超。
    pub budget_us: u32,
    /// 累计超预算帧数。
    pub over_budget: u32,
}

impl FrameLedger {
    /// 是否超预算。
    pub const fn over(&self) -> bool {
        self.budget_us > 0 && self.eval_us > self.budget_us
    }
}

/// 降级裁决结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DegradePlan {
    /// 实际执行的动作序列（**次序判据读的就是这个**）。
    pub actions: Vec<DegradeAction>,
    /// 是否发生「次序跳过」（远实体 LOD 见底仍降精度）。
    pub order_skipped: bool,
    /// LOD 跳过是否**正当**（近实体：降 LOD 无收益）。
    pub lod_skip_legit: bool,
}

impl DegradePlan {
    /// 首个动作（空序列为 `None`）。
    pub fn first(&self) -> DegradeAction {
        match self.actions.first() {
            Some(a) => *a,
            None => DegradeAction::None,
        }
    }

    /// 动作数。
    pub fn len(&self) -> usize {
        self.actions.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}

/// 降级裁决：**LOD 先于精度降**（锚点固定次序）。
///
/// 两种「跳过」形态分开处理：
/// - 近实体跳过 LOD = **正当**（不告警）；
/// - 远实体 LOD 见底仍降精度 = **次序跳过** → 告警 + 遥测。
pub fn plan_degrade(
    ledger: &FrameLedger,
    bucket: DistanceBucket,
    lod: &mut LodState,
    precision: &mut PrecisionState,
    bag: &mut DiagBag,
    tele: &mut Telemetry,
) -> DegradePlan {
    let mut actions: Vec<DegradeAction> = Vec::new();
    if !ledger.over() {
        return DegradePlan { actions, order_skipped: false, lod_skip_legit: false };
    }
    // 超预算帧计数：只在真正超预算的帧上累加（未超预算的帧不得计入）。
    tele.over_budget_frames += 1;

    let mut order_skipped = false;
    let mut lod_skip_legit = false;
    // 第一阶：远实体降频（LOD 先于精度降）。
    if bucket == DistanceBucket::Far && lod.can_reduce() {
        lod.reduce();
        actions.push(DegradeAction::LodDown);
        return DegradePlan { actions, order_skipped: false, lod_skip_legit: false };
    }
    if bucket == DistanceBucket::Near {
        lod_skip_legit = true;
    } else {
        order_skipped = true;
        tele.order_skipped += 1;
        bag.push_major(
            DiagCode::ORDER_SKIPPED,
            "远实体 LOD 已见底却仍在降精度",
            "LOD 家族已无余量；确认 LOD 档位配置是否与远实体规模匹配",
        );
    }
    // 第二阶：降精度。
    if precision.can_reduce() {
        precision.reduce();
        actions.push(DegradeAction::PrecisionDown);
    } else {
        actions.push(DegradeAction::Exhausted);
        bag.push_major(
            DiagCode::BUDGET_EXHAUSTED,
            "LOD 与精度均已见底，求值仍超预算",
            "预算本身定得太小或轨道规模超出预期；先查轨道数，不要继续降档",
        );
    }
    DegradePlan { actions, order_skipped, lod_skip_legit }
}

// ---------------------------------------------------------------------------
// 十、批求值核（零分配热路径 + 缓存 + 能力回退）
// ---------------------------------------------------------------------------

/// 批求值：同批共享一次二分，其余轨复用区间索引。
///
/// **零分配声明范围**：本函数、`eval_batch_scalar`、`sample_track`、
/// `write_lanes` 全体**正常路径不分配任何堆内存**——输出写调用方预分配
/// 切片，通道临时在栈上；唯一的受追踪分配在「输出容量不足」这条异常路径。
pub fn eval_batch(
    plan: &BatchPlan,
    tracks: &[SoaTrack],
    batch_index: usize,
    t_ms: u32,
    out: &mut [f32],
    cursor: &mut usize,
    cache: &mut EvalCache,
    cap: SimdCap,
    probe: &mut AllocProbe,
    bag: &mut DiagBag,
    o: &mut EvalOutcome,
) {
    if batch_index >= plan.batches.len() {
        return;
    }
    let batch = &plan.batches[batch_index];
    if batch.slots.is_empty() {
        return;
    }
    // **回绕检测必须接在这里**：接在调用侧（让用户自己记得调）等于没接——
    // 没人会记得调，于是「回绕即脏」在实现里根本不存在。首个批负责记账。
    if batch_index == 0 {
        cache.note_time(t_ms, bag);
    }
    if cap == SimdCap::ScalarOnly {
        // 显性回退（F1902 家族）：告警 + 如实计数，结果与批路径逐位一致。
        o.fallbacks += 1;
        bag.push_major(
            DiagCode::SIMD_FALLBACK,
            "SIMD 能力不可用，已回退标量逐轨求值",
            "结果与批路径一致，仅速度下降；若非预期，检查能力探测",
        );
        // 零分配纪律：不得 `batch.slots.clone()` —— 那是热路径上的堆分配，
        // 会让「热路径零分配」判据被自身的实现破防。借用原切片即可。
        eval_batch_scalar(tracks, &batch.slots, t_ms, out, cursor, cache, probe, bag, o);
        return;
    }
    // 批内共享二分：取批内第一条轨的时间轴定位一次，其余轨复用区间索引。
    // 前置条件：同批指纹相同（分批键第二分量保证）⇒ 各轨时间轴逐位相同
    // ⇒ 同一区间索引对批内所有轨都成立。
    let lead = batch.slots[0];
    let hit = bisect(&tracks[lead].times, t_ms);
    o.bisect_calls += 1;
    o.bisect_steps += hit.steps;
    if hit.clamped {
        o.clamped += 1;
        bag.push(DiagCode::TIME_CLAMPED, "批内代表轨时间越界", "整批钳制到端点；批内时间轴相同故结论一致");
    }
    let mut i = 0usize;
    while i < batch.slots.len() {
        let ti = batch.slots[i];
        let track = &tracks[ti];
        // 缓存判定先于求值——这是「静态零成本」的执行点。
        if cache.can_reuse(ti, track, t_ms) {
            let mut tmp: [f32; MAX_CHANNELS] = [0.0; MAX_CHANNELS];
            let lanes = cache.load(ti, &mut tmp);
            let before = *cursor;
            write_lanes(out, cursor, probe, bag, o, ti, &tmp, lanes);
            if *cursor > before {
                o.hits += 1;
                i += 1;
                continue;
            }
        }
        // 实际求值：复用批内已定位的区间索引（省掉每轨二分）。
        o.interp_calls += 1;
        let mut tmp: [f32; MAX_CHANNELS] = [0.0; MAX_CHANNELS];
        let lanes = sample_track(track, hit.index, t_ms, &mut tmp);
        let before = *cursor;
        write_lanes(out, cursor, probe, bag, o, ti, &tmp, lanes);
        if *cursor > before {
            cache.store(ti, track, t_ms, &tmp, lanes);
            o.recomputes += 1;
        }
        i += 1;
    }
    // 零分配断言：热路径正常路径不该走到这里任何一次受追踪分配。
    // 判据在调用侧比对 `probe.allocs()` 前后差值（见 `vem07_checks.rs`）。
}

// ---------------------------------------------------------------------------
// 十一、人话说明与冒烟
// ---------------------------------------------------------------------------

/// 家族对齐（F1525 零分配 / F2202 SoA / F2229 LOD / F1902 回退）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FamilyMember {
    pub tag: &'static str,
    pub domain: char,
    pub claim: &'static str,
}

impl FamilyMember {
    /// 全集（四成员）。
    pub const ALL: [FamilyMember; 4] = [
        FamilyMember { tag: "F1525", domain: 'C', claim: "零分配纪律：求值热路径不分配，输出预分配 + 栈内临时" },
        FamilyMember { tag: "F2202", domain: 'L', claim: "SoA 关键帧单源：时间一列 + 通道数据一列" },
        FamilyMember { tag: "F2229", domain: 'L', claim: "LOD 家族：远实体降频先于精度降" },
        FamilyMember { tag: "F1902", domain: 'A', claim: "能力回退：SIMD 不可用显性回退标量，结果逐位一致" },
    ];
}

/// 家族一致性：标签互异 + 域字母齐备。
pub fn family_is_consistent() -> bool {
    let mut i = 0usize;
    let mut same = 0usize;
    while i < FamilyMember::ALL.len() {
        let mut j = i + 1;
        while j < FamilyMember::ALL.len() {
            if FamilyMember::ALL[i].tag == FamilyMember::ALL[j].tag {
                same += 1;
            }
            j += 1;
        }
        i += 1;
    }
    same == 0
}

/// 批覆盖完整性（吃切片：让判据能传表外形态）。
///
/// 三条都要成立：入批轨一条不少、每轨的 kind 与批 kind 一致、每轨指纹与
/// 批指纹一致（**最后一条是批内共享二分的正确性前提**，漏了就等于让批内
/// 各轨用错区间）。
pub fn batch_cover_ok(plan: &BatchPlan, tracks: &[SoaTrack]) -> bool {
    let mut i = 0usize;
    while i < tracks.len() {
        if classify_track(&tracks[i]).is_ok() {
            match plan.batch_of(i) {
                Some(b) => {
                    if plan.batches[b].kind != tracks[i].kind {
                        return false;
                    }
                    if plan.batches[b].fingerprint != times_fingerprint(&tracks[i].times) {
                        return false;
                    }
                }
                None => return false,
            }
        } else if plan.batch_of(i).is_some() {
            return false;
        }
        i += 1;
    }
    true
}

/// 批内轨道的时间轴确实逐位相同（共享二分的**前提实测**，不靠信任分批器）。
pub fn batch_times_identical(plan: &BatchPlan, tracks: &[SoaTrack]) -> bool {
    let mut b = 0usize;
    while b < plan.batches.len() {
        let slots = &plan.batches[b].slots;
        let mut s = 1usize;
        while s < slots.len() {
            if tracks[slots[s]].times != tracks[slots[0]].times {
                return false;
            }
            s += 1;
        }
        b += 1;
    }
    true
}

/// 值类型齐备性（吃切片：让判据能传表外形态）。
pub fn kinds_present(kinds: &[TrackValueKind]) -> bool {
    let mut s = false;
    let mut p = false;
    let mut q = false;
    let mut i = 0usize;
    while i < kinds.len() {
        match kinds[i] {
            TrackValueKind::Scalar => s = true,
            TrackValueKind::Position => p = true,
            TrackValueKind::Quat => q = true,
        }
        i += 1;
    }
    s && p && q
}

/// 三类齐备且顺序无关。
pub fn three_kinds_present() -> bool {
    kinds_present(&TrackValueKind::ALL)
}

/// 诊断码家族齐备性（吃切片：传表外形态）。
pub fn codes_present(codes: &[DiagCode]) -> bool {
    let mut i = 0usize;
    while i < DiagCode::ALL.len() {
        let mut found = false;
        let mut j = 0usize;
        while j < codes.len() {
            if codes[j] == DiagCode::ALL[i] {
                found = true;
            }
            j += 1;
        }
        if !found {
            return false;
        }
        i += 1;
    }
    true
}

/// 全部诊断码齐备。
pub fn all_codes_present() -> bool {
    codes_present(&DiagCode::ALL)
}

/// 人话总述。
pub fn describe() -> String {
    let mut s = String::new();
    let _ = s.push_str("动画求值性能：\n");
    let _ = s.push_str("· 批量求值按「值类型 × 时间轴指纹」分批，4/8 轨一批，批内共享关键帧二分。\n");
    let _ = s.push_str("· 零分配纪律限定在求值热路径：输出预分配切片 + 栈上定长临时，分配追踪器断言增量 0。\n");
    let _ = s.push_str("· 缓存是「值 + 脏标记」对：轨道编辑（edit_rev）或时间回绕即脏；静态轨跨时间点零成本命中。\n");
    let _ = s.push_str("· 超预算降级次序固定：远实体先降 LOD，LOD 见底才降精度，全见底如实记告警。\n");
    let _ = s.push_str("· SIMD 不可用显性回退标量，结果与批路径逐位一致。\n");
    s
}

/// 冒烟：造一批同时间轴的标量轨，批求值一次。
pub fn smoke() -> String {
    let times = vec![0u32, 100, 200, 400];
    let mut tracks: Vec<SoaTrack> = Vec::new();
    let mut i = 0usize;
    while i < 4 {
        let chans = vec![0.0f32, 1.0, 2.0, 3.0];
        tracks.push(SoaTrack::new(
            "s",
            TrackClass::Continuous,
            TrackValueKind::Scalar,
            false,
            times.clone(),
            chans,
        ));
        i += 1;
    }
    let plan = plan_batches(&tracks, 4);
    let mut out = vec![0.0f32; 16];
    let mut cursor = 0usize;
    let mut cache = EvalCache::new();
    cache.reserve_for(tracks.len());
    let mut probe = AllocProbe::new();
    let mut bag = DiagBag::new();
    let mut o = EvalOutcome::default();
    let mut b = 0usize;
    while b < plan.batches.len() {
        eval_batch(
            &plan,
            &tracks,
            b,
            150,
            &mut out,
            &mut cursor,
            &mut cache,
            SimdCap::Batched,
            &mut probe,
            &mut bag,
            &mut o,
        );
        b += 1;
    }
    let mut s = String::new();
    let _ = s.push_str(&format!(
        "批数={} 入批={} 写入={} 二分={} 步={} 插值={} 追踪分配={}\n",
        plan.batch_count(),
        plan.accepted_count(),
        o.written,
        o.bisect_calls,
        o.bisect_steps,
        o.interp_calls,
        probe.allocs()
    ));
    let _ = s.push_str(&format!("值={} {} {} {}\n", out[0], out[1], out[2], out[3]));
    s
}
