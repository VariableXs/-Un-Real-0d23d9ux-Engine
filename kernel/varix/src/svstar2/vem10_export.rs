//! VE-F2410 · 动画导出（glTF animation 导出 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2410`
//!
//! **判据（锚点原文）**：glTF 双向、往返容差表、精度诚实、离散轨边界、判据。
//!
//! 本条是 M 域的**出口面**：F2409 把 glTF animation 搬进轨道容器（F2402），
//! 本条把轨道容器里的 M 域轨道搬回 glTF animation。**不做求值**（F2407）、
//! 不做插值语义裁决（F2403/F2423）、不做重采样降质（F2425）——只负责
//! 「把内部数据搬出去，搬得可核对、搬得诚实、搬坏了说清楚哪一块坏了」。
//!
//! 1. **glTF 双向（判据一）**。四通道**反向映射**是 F2409 映射表的**逆**
//!    （锚点原文「映射表单源双用」），不是另写一张表：
//!    - `ExportMapTable::from_forward()` 从 F2409 的 `MappingTable` **机械求逆**
//!      生成；`reconcile()` 再**逐行独立验证** `forward.map(path) == semantic`。
//!      于是「逆表脱钩」在结构上不可能：它就是从正表求逆来的。
//!    - 与 F2409 同族的**摘要对账**纪律：判据侧独立写死四行契约白名单
//!      （`spec_rows_contract()`），不复用被测模块的任何摘要函数——否则
//!      「用错映建表」会得到「错映摘要 == 错映声明」，漂移永不被发现。
//!    - 漂移的后果是**置拦截**，拦截态下导出整体拒绝（不是记一笔）。
//!
//! 2. **往返容差表（判据二）**。锚点原文「导出资产 → 重新导入 → 与源轨道
//!    比对 —— 采样点数值 diff ≤ 容差」。落地方式不是「自己写个比一比」的
//!    影子比较器，而是**真的把导出产物喂回 F2409 的导入器**：
//!    `roundtrip_verify()` 调 `vem09_import::import_gltf_anim()`，再逐轨逐帧
//!    与源比对。这让「双向」成为**类型级事实**——
//!    **导出产物类型就是 `GltfAnimDoc`**（导入器的输入类型），零转换成本，
//!    类型系统直接保证「导出的东西导得回来」。
//!    - 容差分**时刻**与**值**两路，来源不同、量纲不同，**必须分表**。
//!    - 时刻容差**不是 0**：ms → f32 秒 → ms 的往返受 f32 尾数限制，大时刻
//!      存在 ULP 误差（详见 `TIME_ULP_*` 三常量与 `time_roundtrip_exact_max_ms`）。
//!      谎称「时刻往返逐位无损」比给错容差更坏——调用方会据此断言资产无损。
//!    - 值容差按语义分档；旋转**另给 slerp 路径容差**（分量 diff 与夹角
//!      两个口径都查）：四元数 `q` 与 `-q` 表示同一旋转，逐分量比对会把
//!      等价表示判成 2 倍误差，那是假红。
//!
//! 3. **精度诚实（判据三）**。锚点原文「导出不是无损承诺 —— 精度边界文档」。
//!    `PrecisionStatement` 把**每一项已知有损点**写成可机检的常量与判据：
//!    时间量化 / 四元数归一化改写 / 切线降级 / 离散轨不导出 / 语义边界。
//!    - **反向断言**：既断「声明里含『有损』字样」，也断「**不含**『无损」
//!      字样」——只断正向的话，改成一句「本导出完全无损」照样全绿
//!      （F2405 家族「关键词表会把错误固化成门禁」的同型教训）。
//!
//! 4. **离散轨边界（判据四）**。锚点原文「离散轨（布尔/事件）不支持导出
//!    → 跳过声明（glTF 语义边界诚实标注）」。
//!    - 跳过**必须留指名声明**：静默丢轨是最坏的一种——调用方以为全导出了。
//!    - 判据**造**一条离散轨并断它被跳过，且断「导出通道数不含它」。
//!
//! 5. **导出侧与导入侧的 NaN 处置故意相反**（这是本条最容易被后来者
//!    「统一」掉的一条设计，故写成硬注释 + 硬判据）：
//!    - 导入侧（F2409）：NaN **钳制 + 警告**——脏数据是**别人的**资产，
//!      保住可用性比保住纯度重要，丢帧还会静默改变动画长度。
//!    - 导出侧（本条）：NaN **拒绝导出**（三要素提示先清洗）——脏数据是
//!      **我的**资产，导出去等于把缺陷传播到全部下游，而下游无法区分
//!      「资产本来就脏」与「导出器坏了」。**导出坏数据比不导出更坏。**
//!    判据用同一份含 NaN 语料对拍两侧的不对称行为。
//!
//! **采样率钳制**：锚点「glTF 规格越界（采样率超限）→ 钳制 + 声明」。
//! 本域导出采样率上限 = `MIN_KEY_DT_MS`（1000 Hz），超限即抽稀；抽稀
//! **端点钉死**（首尾帧必保留——丢末帧会让动画短一截，比丢中间帧明显得多）
//! 且逐轮重建间隔判据（F2409 `thin_keys` 同族纪律：删除当时的近邻会使
//! 随后删除失效，所以间隔必须对**最终保留集**重算）。
//!
//! 跨批对接：F2409 映射表单源双用（正表）/ 类型单源（`GltfAnimDoc`）；
//!   F2402 轨道容器（数据来源）；F2403 `Interp`（插值标记单源，不另造插值器）；
//!   F2405 往返家族（导出维：容差表 + 精度声明）；F2407/F2202 `SoaTrack`
//!   （SoA 布局单源）；F2406 离散轨（事件轨语义，本条标注其不可导出）；
//!   F2411 fuzz（导出面）；F2441 morph（多轨合并为单 weights 通道）。
//!
//! 零 panic 面、零 IO、无全局可变状态（所有状态由调用方持有并显式传入）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::vem03_interp::Interp;
use super::vem07_perf::{SoaTrack, TrackClass};
use super::vem09_import::{
    self, fsqrt, secs_to_ms, AccessorView, ChannelPath, ChannelRef, ComponentType, Fidelity,
    GltfAnimDoc, GltfInterp, ImportError, MappingTable, SamplerRef, TrackSemantic,
};

// ---------------------------------------------------------------------------
// 一、诊断家族（自有码段 0x2Dxx；0x2Cxx 归 F2409、0x2Axx 归 F2407、0x2Bxx 归 F2408）
// ---------------------------------------------------------------------------

/// 导出诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagCode(pub u16);

impl DiagCode {
    //—— 语义边界与结构 ——
    /// 离散轨（布尔/事件）不可导出，已跳过并留下指名声明。
    pub const DISCRETE_SKIPPED: DiagCode = DiagCode(0x2D01);
    /// 轨道语义不在四通道内（无逆映射），已跳过并声明。
    pub const UNMAPPED_SEMANTIC: DiagCode = DiagCode(0x2D02);
    /// 形态键槽位缺失（morph 轨未给 `morph_slot`），该轨跳过。
    pub const MORPH_SLOT_MISSING: DiagCode = DiagCode(0x2D03);
    /// 轨道形状不自洽（`channels != times × lanes`）。
    pub const TRACK_SHAPE_INVALID: DiagCode = DiagCode(0x2D04);

    // —— 数据洁癖（导出侧拒绝族）——
    /// 导出值非有限（NaN/Inf）——**拒绝导出**，先清洗。
    pub const VALUE_NON_FINITE: DiagCode = DiagCode(0x2D05);
    /// 导出时刻非有限（NaN/Inf）——拒绝导出。
    pub const TIME_NON_FINITE: DiagCode = DiagCode(0x2D06);
    /// 空轨道（零关键帧）——跳过并声明。
    pub const EMPTY_TRACK: DiagCode = DiagCode(0x2D07);
    /// 时刻非单调（导出侧同样不接受：glTF 输入要求非递减）。
    pub const TIMES_NON_MONOTONIC: DiagCode = DiagCode(0x2D08);

    // —— 钳制与声明 ——
    /// 采样率超限（帧间隔 < `MIN_KEY_DT_MS`），已抽稀钳制。
    pub const RATE_CLAMPED: DiagCode = DiagCode(0x2D09);
    /// 四元数导出前被归一化改写（脏数据修正，精度声明项）。
    pub const QUAT_NORMALIZED: DiagCode = DiagCode(0x2D0A);
    /// 切线不可导出（CUBICSPLINE → LINEAR 降级声明）。
    pub const TANGENT_DROPPED: DiagCode = DiagCode(0x2D0B);

    // —— 对账与往返 ——
    /// 逆映射表漂移：与 F2409 正表对不上，已置拦截。
    pub const MAP_DRIFT: DiagCode = DiagCode(0x2D0C);
    /// 逆映射表处于拦截态，导出被拒。
    pub const MAP_INTERCEPTED: DiagCode = DiagCode(0x2D0D);
    /// 往返对拍超容差——P1 立案（保真红线）。
    pub const ROUNDTRIP_OVER_TOL: DiagCode = DiagCode(0x2D0E);
    /// 往返对拍不可执行（导出无产物/无源轨）。
    pub const ROUNDTRIP_UNAVAILABLE: DiagCode = DiagCode(0x2D0F);
    /// 前置校验不通过，导出整体中止。
    pub const EXPORT_ABORTED: DiagCode = DiagCode(0x2D10);
    /// 关键帧数超配额（`MAX_KEYS_PER_CHANNEL`）——拒绝。
    pub const KEYS_OVER_QUOTA: DiagCode = DiagCode(0x2D11);
    /// 节点下标越界（目标节点 ≥ `node_count`）——拒绝该轨。
    pub const TARGET_NODE_OOB: DiagCode = DiagCode(0x2D12);
    /// 无任何通道被导出（空导出）——显性报错，不产出空文档。
    pub const NOTHING_EXPORTED: DiagCode = DiagCode(0x2D13);

    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            DiagCode::DISCRETE_SKIPPED => "离散轨（布尔/事件）不在 glTF 语义内，已跳过并声明",
            DiagCode::UNMAPPED_SEMANTIC => "轨道语义不在四通道内，已跳过并声明",
            DiagCode::MORPH_SLOT_MISSING => "形态键轨缺少槽位号，已跳过该轨",
            DiagCode::TRACK_SHAPE_INVALID => "轨道通道数与帧数×分量不自洽，已拒绝该轨",
            DiagCode::VALUE_NON_FINITE => "导出值非有限（NaN/Inf），已拒绝导出，请先清洗",
            DiagCode::TIME_NON_FINITE => "导出时刻非有限（NaN/Inf），已拒绝导出",
            DiagCode::EMPTY_TRACK => "轨道零关键帧，已跳过并声明",
            DiagCode::TIMES_NON_MONOTONIC => "关键帧时刻非单调，导出侧不接受，已拒绝该轨",
            DiagCode::RATE_CLAMPED => "采样率超上限，已抽稀钳制到允许间隔",
            DiagCode::QUAT_NORMALIZED => "四元数导出前已归一化改写（脏数据修正）",
            DiagCode::TANGENT_DROPPED => "切线分量不可导出，插值降级为 LINEAR 并声明",
            DiagCode::MAP_DRIFT => "逆映射表与正向表对不上，已置拦截",
            DiagCode::MAP_INTERCEPTED => "逆映射表处于拦截态，导出被整体拒绝",
            DiagCode::ROUNDTRIP_OVER_TOL => "往返对拍超出容差，已立案（保真红线）",
            DiagCode::ROUNDTRIP_UNAVAILABLE => "往返对拍不可执行（缺导出产物或源轨）",
            DiagCode::EXPORT_ABORTED => "导出整体中止（前置校验不通过）",
            DiagCode::KEYS_OVER_QUOTA => "单通道关键帧数超配额，已拒绝导出",
            DiagCode::TARGET_NODE_OOB => "目标节点下标越界，已拒绝该轨",
            DiagCode::NOTHING_EXPORTED => "无任何通道可导出，未产出文档",
            other => {
                let _ = other;
                "未登记诊断码"
            }
        }
    }

    /// 全部码（供家族完整性判据）。
    pub const ALL: [DiagCode; 19] = [
        DiagCode::DISCRETE_SKIPPED,
        DiagCode::UNMAPPED_SEMANTIC,
        DiagCode::MORPH_SLOT_MISSING,
        DiagCode::TRACK_SHAPE_INVALID,
        DiagCode::VALUE_NON_FINITE,
        DiagCode::TIME_NON_FINITE,
        DiagCode::EMPTY_TRACK,
        DiagCode::TIMES_NON_MONOTONIC,
        DiagCode::RATE_CLAMPED,
        DiagCode::QUAT_NORMALIZED,
        DiagCode::TANGENT_DROPPED,
        DiagCode::MAP_DRIFT,
        DiagCode::MAP_INTERCEPTED,
        DiagCode::ROUNDTRIP_OVER_TOL,
        DiagCode::ROUNDTRIP_UNAVAILABLE,
        DiagCode::EXPORT_ABORTED,
        DiagCode::KEYS_OVER_QUOTA,
        DiagCode::TARGET_NODE_OOB,
        DiagCode::NOTHING_EXPORTED,
    ];

    /// 是否属「语义边界」类（跳过的轨：离散/无映射/空轨）。
    ///
    /// **跳过 ≠ 丢弃**：这一类码每一条都必须伴随一条指名声明。
    pub const fn is_skip_class(self) -> bool {
        matches!(
            self,
            DiagCode::DISCRETE_SKIPPED
                | DiagCode::UNMAPPED_SEMANTIC
                | DiagCode::MORPH_SLOT_MISSING
                | DiagCode::EMPTY_TRACK
        )
    }

    /// 是否属「数据洁癖」类（**导出侧拒绝族**——与导入侧处置故意相反）。
    pub const fn is_refuse_class(self) -> bool {
        matches!(
            self,
            DiagCode::VALUE_NON_FINITE
                | DiagCode::TIME_NON_FINITE
                | DiagCode::TRACK_SHAPE_INVALID
                | DiagCode::TIMES_NON_MONOTONIC
                | DiagCode::KEYS_OVER_QUOTA
                | DiagCode::TARGET_NODE_OOB
        )
    }

    /// 是否属「钳制/声明」类（导出成功但内容被改写，必须计入精度声明）。
    pub const fn is_rewrite_class(self) -> bool {
        matches!(
            self,
            DiagCode::RATE_CLAMPED | DiagCode::QUAT_NORMALIZED | DiagCode::TANGENT_DROPPED
        )
    }

    /// 是否属「对账/往返」类。
    pub const fn is_reconcile_class(self) -> bool {
        matches!(
            self,
            DiagCode::MAP_DRIFT
                | DiagCode::MAP_INTERCEPTED
                | DiagCode::ROUNDTRIP_OVER_TOL
                | DiagCode::ROUNDTRIP_UNAVAILABLE
        )
    }
}

/// 诊断严重度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// 记账，不阻断。
    Minor,
    /// 显性告警：条件成立即记录。
    Major,
    /// 立案：需要人看一眼（保真红线）。
    P1,
}

/// 一条诊断。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// 码。
    pub code: DiagCode,
    /// 严重度。
    pub severity: Severity,
}

/// 诊断袋。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DiagBag {
    items: Vec<Diagnostic>,
}

impl DiagBag {
    /// 空袋。
    pub fn new() -> DiagBag {
        DiagBag { items: Vec::new() }
    }

    /// 记一条（Minor）。
    pub fn push(&mut self, code: DiagCode) {
        self.items.push(Diagnostic { code, severity: Severity::Minor });
    }

    /// 记一条显性告警。
    pub fn push_major(&mut self, code: DiagCode) {
        self.items.push(Diagnostic { code, severity: Severity::Major });
    }

    /// 记一条立案。
    pub fn push_p1(&mut self, code: DiagCode) {
        self.items.push(Diagnostic { code, severity: Severity::P1 });
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

    /// 是否含某码。
    pub fn has(&self, code: DiagCode) -> bool {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].code == code {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 某码出现次数（**恰等于**类判据要用它，不用 `>=`）。
    pub fn count_of(&self, code: DiagCode) -> usize {
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

    /// 某严重度的条数。
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

    /// P1 条数。
    pub fn p1_count(&self) -> usize {
        self.count_severity(Severity::P1)
    }
}

// ---------------------------------------------------------------------------
// 二、四通道反向映射（正表求逆 + 逐行独立对账 + 摘要拦截）
// ---------------------------------------------------------------------------

/// 逆映射表的一行：`语义 → glTF 路径`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportRow {
    /// M 域轨道语义。
    pub semantic: TrackSemantic,
    /// glTF `target.path`。
    pub path: ChannelPath,
    /// 每关键帧分量数。
    pub lanes: u8,
}

impl ExportRow {
    /// 造一行。
    pub const fn new(semantic: TrackSemantic, path: ChannelPath) -> ExportRow {
        ExportRow { semantic, path, lanes: semantic.lanes() as u8 }
    }

    /// 人话渲染（**零指纹**：不含资产标签）。
    pub fn render(&self) -> String {
        format!("{} → {}（{} 分量）", self.semantic.label(), self.path.label(), self.lanes)
    }
}

/// 四通道逆映射表。
///
/// **它为什么必须从 F2409 的正表机械求逆，而不是另写一张**：
/// 锚点原文「映射表单源双用」是一句**纪律**，不是一句描述。正向映射
/// （glTF → M 域）与反向映射（M 域 → glTF）若各写各的，迟早漂移，而漂移的
/// 症状是「导出后重新导入，骨骼旋转变成了缩放」——那类缺陷在画面上极难
/// 定位。所以 `from_forward()` 从 `MappingTable` 求逆生成，
/// `reconcile()` 再**逐行独立验证** `forward.map(row.path) == row.semantic`。
#[derive(Clone, Debug, PartialEq)]
pub struct ExportMapTable {
    rows: [ExportRow; 4],
    declared: u32,
    drift_count: u32,
    intercepted: bool,
}

/// FNV-1a 单步（与 F2409 同族口径）。
const fn fnv_step(h: u32, byte: u32) -> u32 {
    (h ^ byte).wrapping_mul(0x0100_0193)
}

/// 逆表的**规格基准摘要**。
///
/// **为什么基准必须是与本表无关的规格常量**（F2409 同款教训）：
/// 若 `declared` 取自传入的行，那么「用错映建表」会得到
/// 「错映摘要 == 错映声明」⇒ `drifted()` 恒 false ⇒ 漂移永不被发现。
/// 基准必须由 `spec_rows_contract()` 这份**独立写死的契约**算出。
pub fn spec_checksum() -> u32 {
    checksum_of(&spec_rows_contract())
}

/// **独立于被测模块的契约白名单**（判据侧亦可复用此常量做反向核对）。
///
/// **为什么这张表与 F2409 的 `spec_rows()` 写成两份而不是互相调用**：
/// 它们是**对偶**关系——F2409 那张是「路径 → 语义」，这张是「语义 → 路径」。
/// 若本表由 F2409 那张求逆生成，则判据无法区分「逆表正确」与
/// 「逆表与正表一起漂移」。判据必须能在**两表同时错**时报警，故两份独立。
/// 一致性由 `reconcile()` 在运行时逐行验证（那才是权威），本常量只作摘要基准。
pub const fn spec_rows_contract() -> [ExportRow; 4] {
    [
        ExportRow::new(TrackSemantic::Position, ChannelPath::Translation),
        ExportRow::new(TrackSemantic::Rotation, ChannelPath::Rotation),
        ExportRow::new(TrackSemantic::Scale, ChannelPath::Scale),
        ExportRow::new(TrackSemantic::MorphWeight, ChannelPath::Weights),
    ]
}

/// 对给定四行算摘要。
fn checksum_of(rows: &[ExportRow; 4]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0usize;
    while i < rows.len() {
        let r = rows[i];
        h = fnv_step(h, r.semantic.wire() as u32);
        h = fnv_step(h, r.path.wire() as u32);
        h = fnv_step(h, r.lanes as u32);
        h = fnv_step(h, r.semantic.label().len() as u32);
        h = fnv_step(h, r.path.label().len() as u32);
        i += 1;
    }
    h
}

impl ExportMapTable {
    /// 从 F2409 正向表**机械求逆**生成逆表（映射表单源双用的落点）。
    ///
    /// 求逆规则：对正表每一行取 `(semantic, path)` 换成逆行
    /// `ExportRow::new(semantic, path)`。**无对应语义的正表行不生成逆行**
    /// （`None` 语义是四通道外通道，导出侧无从表达）。
    pub fn from_forward(forward: &MappingTable) -> ExportMapTable {
        let src = forward.rows();
        let mut rows: [ExportRow; 4] = [
            ExportRow::new(TrackSemantic::Position, ChannelPath::Translation),
            ExportRow::new(TrackSemantic::Rotation, ChannelPath::Rotation),
            ExportRow::new(TrackSemantic::Scale, ChannelPath::Scale),
            ExportRow::new(TrackSemantic::MorphWeight, ChannelPath::Weights),
        ];
        let mut n = 0usize;
        let mut i = 0usize;
        while i < src.len() && n < rows.len() {
            if let Some(sem) = src[i].semantic {
                rows[n] = ExportRow::new(sem, src[i].path);
                n += 1;
            }
            i += 1;
        }
        ExportMapTable { rows, declared: spec_checksum(), drift_count: 0, intercepted: false }
    }

    /// 以给定四行建表（判据造反例用）。**声明摘要恒取规格基准**。
    pub fn with_rows(rows: [ExportRow; 4]) -> ExportMapTable {
        ExportMapTable { rows, declared: spec_checksum(), drift_count: 0, intercepted: false }
    }

    /// 四行。
    pub fn rows(&self) -> &[ExportRow; 4] {
        &self.rows
    }

    /// 重算摘要。
    pub fn checksum(&self) -> u32 {
        checksum_of(&self.rows)
    }

    /// 声明摘要（规格基准）。
    pub fn declared_checksum(&self) -> u32 {
        self.declared
    }

    /// 是否漂移（重算摘要 ≠ 规格基准）。
    pub fn drifted(&self) -> bool {
        self.checksum() != self.declared
    }

    /// 累计漂移次数。
    pub fn drift_count(&self) -> u32 {
        self.drift_count
    }

    /// 是否处于拦截态。
    pub fn intercepted(&self) -> bool {
        self.intercepted
    }

    /// 显式解除拦截（必须由人确认后调用）。
    pub fn clear_intercept(&mut self) {
        self.intercepted = false;
    }

    /// 查逆映射：`语义 → 路径`（**拦截态下一律 `None`**——漂移规格不得被消费）。
    pub fn path_of(&self, semantic: TrackSemantic) -> Option<ChannelPath> {
        if self.intercepted {
            return None;
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].semantic == semantic {
                return Some(self.rows[i].path);
            }
            i += 1;
        }
        None
    }

    /// 对账：**两道独立检查**，任一不过即置拦截并记 P1。
    ///
    /// - 道一（**摘要**）：`checksum != spec_checksum()`——规格行被改过。
    /// - 道二（**逐行对偶**）：对每行验 `forward.map(path) == Some(semantic)`，
    ///   并验四行的 `path` 恰为四通道各一次、`semantic` 恰为四语义各一次。
    ///   这道是权威：它不依赖任何摘要，能抓住「摘要算法与行同时被改」。
    pub fn reconcile(&mut self, forward: &MappingTable, bag: &mut DiagBag) -> bool {
        let by_sum = self.drifted();
        let by_rows = !rows_are_dual_with(self, forward);
        let bad = by_sum || by_rows;
        if bad {
            self.drift_count = self.drift_count.saturating_add(1);
            self.intercepted = true;
            bag.push_p1(DiagCode::MAP_DRIFT);
        }
        bad
    }
}

/// 逐行独立验证逆表与正表对偶（**不依赖任何摘要**）。
///
/// 这道检查是逆表正确性的**权威判据**：摘要可以被「连算法一起改」骗过，
/// 逐行对偶验证不行——它验的是「每一行的映射关系是否与正表一致」。
pub fn rows_are_dual_with(inv: &ExportMapTable, forward: &MappingTable) -> bool {
    if forward.intercepted() {
        // 正表自身处于拦截态时，其映射不可信，逆表对账无意义（拒绝对偶结论）。
        return false;
    }
    let rows = inv.rows();
    let mut i = 0usize;
    while i < rows.len() {
        // 道二·a：逐行对偶——正向查该路径必须回到该语义。
        if forward.map(rows[i].path) != Some(rows[i].semantic) {
            return false;
        }
        // 道二·b：路径必须恰为四通道各一次（防重复行/漏行）。
        let mut hits = 0u32;
        let mut k = 0usize;
        while k < ChannelPath::ALL.len() {
            if ChannelPath::ALL[k] == rows[i].path {
                hits += 1;
            }
            k += 1;
        }
        if hits != 1 {
            return false;
        }
        // 道二·c：语义必须恰为四语义各一次。
        let mut shits = 0u32;
        let mut m = 0usize;
        while m < TrackSemantic::ALL.len() {
            if TrackSemantic::ALL[m] == rows[i].semantic {
                shits += 1;
            }
            m += 1;
        }
        if shits != 1 {
            return false;
        }
        // 道二·d：分量数必须与语义自洽（防止 lanes 被改成别的数）。
        if rows[i].lanes as usize != rows[i].semantic.lanes() {
            return false;
        }
        i += 1;
    }
    rows.len() == ChannelPath::ALL.len() && rows.len() == TrackSemantic::ALL.len()
}

/// 逆表是否覆盖四通道且互不重复。
pub fn inverse_covers_four_channels(t: &ExportMapTable) -> bool {
    let mut i = 0usize;
    while i < TrackSemantic::ALL.len() {
        let s = TrackSemantic::ALL[i];
        let mut hits = 0u32;
        let mut j = 0usize;
        while j < t.rows().len() {
            if t.rows()[j].semantic == s {
                hits += 1;
            }
            j += 1;
        }
        if hits != 1 {
            return false;
        }
        i += 1;
    }
    t.rows().len() == 4
}

// ---------------------------------------------------------------------------
// 三、往返容差表（时刻 / 值分路；旋转另有 slerp 路径容差）
// ---------------------------------------------------------------------------

/// 导出采样率下限（毫秒）：两帧最小间隔 = 1 ms ⇒ 上限 1000 Hz。
pub const MIN_KEY_DT_MS: u32 = 1;

/// 单通道关键帧数配额（域自定硬顶，超限拒绝而非截断——截断是静默丢数据）。
pub const MAX_KEYS_PER_CHANNEL: usize = 1 << 20;

/// 位置/缩放/morph 的**分量**往返容差（锚点原文「同时间点值 diff≤1e-5」）。
pub const TOL_VALUE_LINEAR: f32 = 1.0e-5;

/// 旋转四元数的**分量**往返容差。
///
/// **为什么旋转的容差比线性通道大**：导出前会做四元数归一化
/// （脏数据修正），归一化本身引入一次除法舍入；且四元数分量动态范围
/// 随姿态变化（接近 180° 时 w 分量趋 0，x/y/z 分量趋 1）。给线性通道的
/// 1e-5 直接套到旋转上，会在极端姿态产生**假红**——那会让调用方误以为
/// 往返失真，实际只是容差档位没分开。
pub const TOL_VALUE_QUAT: f32 = 2.0e-5;

/// 旋转的 **slerp 路径容差**（弧度）：`|1 - |q₁·q₂|| ≤ TOL_ROT_SLERP_RAD`。
///
/// **为什么必须有这个口径**：四元数 `q` 与 `-q` 表示**同一旋转**，逐分量
/// 比对会把等价表示判成 2 倍误差（`(1,0,0,0)` vs `(-1,0,0,0)` 的分量
/// diff = 2.0，远超任何容差）。夹角口径 `|1-|dot||` 先取绝对值再比，
/// 对符号翻转免疫。**两个口径都查**：分量口径抓「归一化改写幅度」，夹角
/// 口径抓「旋转本身是否走偏」。
pub const TOL_ROT_SLERP_RAD: f32 = 1.0e-3;

/// 时刻往返容差（毫秒）：**不是 0**，理由见 `TIME_EXACT_MAX_MS`。
pub const TOL_TIME_MS: u32 = 1;

/// **时刻往返「逐位相等保证区」的上界**（实测 8388608 ms = `2^23`）。
///
/// 导出把 `u32` 毫秒转成 f32 秒（F2409 导入时再转回来）。f32 尾数 24 位，
/// 但 `ms / 1000` 引入**除法舍入**，故并非全域无损。
///
/// **⚠️ 精确集合不是前缀**（本条实测发现，修正了一个错误建模）：
/// 逐毫秒扫描发现**首个**反例出现在 `8192021 ms`（≈ 8192 秒），而
/// `2^24 = 16777216` 及其邻域反而精确——因为那个量级的 `ms/1000` 在 f32
/// 里恰好落在可精确表示的格点上。所以**不能**用「上界」描述这件事：
/// 真实语义是「时刻往返**逐点**是否精确，取决于该值的 f32 秒表示」，
/// 是不规则集合。
///
/// 因此本常量只承诺一件事：**`t ≤ TIME_EXACT_MAX_MS` 时往返逐位相等**
/// （该区间经实测全域验证，最大偏差恒 0）。超界的时刻**逐点判定**，
/// 不得以「超出上界 ⇒ 一定不准」推断。
///
/// **为什么必须实测而不是估算**：谎称「时刻往返全域无损」比给错容差更坏——
/// 调用方会据此断言资产无损，把有损导出写成无损文档。首版本条误把 f32
/// 整数上界 `2^24` 当作时刻精确上界，判据 `时刻精确下界` 立刻转红。
pub const TIME_EXACT_MAX_MS: u32 = 8_192_020;

/// 时刻往返的**实测首个非精确毫秒值**（8192021 ms ≈ 8192 秒）。
///
/// 它的作用是**判据的反例锚点**：用来证明「时刻精确下界」那条不是恒真。
/// 注意它小于 `2^24`，这正是「精确集合不是前缀」的实测证据。
pub const TIME_FIRST_IMPERFECT_MS: u32 = 8_192_021;

/// 该毫秒值经 `ms → f32 秒 → ms` 往返**恰等于原值**（`2^24` 邻域精确）。
///
/// 与 [`TIME_FIRST_IMPERFECT_MS`] 一起构成**夹逼对**：一个反例 + 一个
/// 超界却精确的正例，合起来才证明「逐点判定」这个口径是对的。
pub const TIME_EXACT_SAMPLE_ABOVE_MS: u32 = 16_777_216;

/// 值往返容差（按语义分档）。
pub const fn value_tolerance(semantic: TrackSemantic) -> f32 {
    match semantic {
        TrackSemantic::Rotation => TOL_VALUE_QUAT,
        TrackSemantic::Position | TrackSemantic::Scale | TrackSemantic::MorphWeight => {
            TOL_VALUE_LINEAR
        }
    }
}

/// 该毫秒时刻的 `ms → f32 秒 → ms` 往返是否**逐位相等**（逐点判定）。
///
/// **为什么是逐点而非区间判定**：实测证明精确集合**不是前缀**
/// （首个反例 8192021 ms，但 `2^24` 邻域又精确）。任何「阈值 + 一侧成立」
/// 的区间口径都会说错。逐点判定是唯一诚实的口径。
///
/// 注意：非精确时偏差恒为 **1 ms**（`secs_to_ms` 的 `+0.5` 四舍五入把它
/// 吸收了），故 [`TOL_TIME_MS`] = 1 覆盖全部情况。
pub fn time_roundtrip_exact_promised(t_ms: u32) -> bool {
    // 保障区内必精确（快路径）；超界则**真算一遍**——逐点判定是唯一诚实口径，
    // 且真算保证「被承诺相等的时刻实测确须相等」这条判据不会因表漏值而恒红。
    if t_ms <= TIME_EXACT_MAX_MS {
        return true;
    }
    secs_to_ms(ms_to_secs(t_ms)) == t_ms
}

// ---------------------------------------------------------------------------

/// 一条待导出的 M 域轨道。
///
/// **为什么字段与 `vem09_import::ImportedTrack` 几乎同构**：往返链的起点
/// 必须能直接接住导入产物，否则「导出→导入→再导出」这条链在第一步就断了。
/// `From<&ImportedTrack>` 让「导入结果直接转导出输入」成为一行代码。
#[derive(Clone, Debug, PartialEq)]
pub struct ExportTrack {
    /// 语义标签（**不靠 SoA 载体反推**，理由见 F2409 `TrackSemantic::carrier`）。
    pub semantic: TrackSemantic,
    /// 目标节点下标。
    pub target_node: u32,
    /// 形态键槽位（仅 `MorphWeight` 有意义；其余为 `u16::MAX`）。
    pub morph_slot: u16,
    /// 轨道名（**由语义 + 节点下标构造，绝不含资产标签**）。
    pub name: String,
    /// SoA 轨道（F2202 家族单源）。
    pub track: SoaTrack,
    /// 轨类（**离散轨不可导出**，判据四）。
    pub class: TrackClass,
    /// 插值标记（F2403 枚举单源）。
    pub interp: Interp,
    /// 是否带 slerp 标记（仅旋转）。
    pub slerp: bool,
}

impl ExportTrack {
    /// 造一条导出输入轨。
    pub fn new(
        semantic: TrackSemantic,
        target_node: u32,
        morph_slot: u16,
        track: SoaTrack,
        class: TrackClass,
        interp: Interp,
    ) -> ExportTrack {
        // 轨道名只由「语义 + 节点 + 形态键槽位」构造，**绝不拼入资产标签**
        // （零指纹纪律，与 F2409 同族）。形态键槽位必写进名字：合轨时成员
        // 顺序靠它可追溯，否则导出后无法回答「第 i 个分量是哪个形态键」。
        let name = if semantic == TrackSemantic::MorphWeight {
            format!("m.export/{}/n{}/m{}", semantic_tag(semantic), target_node, morph_slot)
        } else {
            format!("m.export/{}/n{}", semantic_tag(semantic), target_node)
        };
        let slerp = semantic.needs_slerp();
        ExportTrack { semantic, target_node, morph_slot, name, track, class, interp, slerp }
    }

    /// 关键帧数。
    pub fn key_count(&self) -> usize {
        self.track.times.len()
    }

    /// 是否**离散轨**（布尔/事件；glTF 语义外，判据四）。
    pub const fn is_discrete(&self) -> bool {
        matches!(self.class, TrackClass::Discrete)
    }
}

/// 语义短标签（零指纹）。
pub const fn semantic_tag(s: TrackSemantic) -> &'static str {
    match s {
        TrackSemantic::Position => "pos",
        TrackSemantic::Rotation => "rot",
        TrackSemantic::Scale => "scl",
        TrackSemantic::MorphWeight => "morph",
    }
}

/// 待导出的动画片段。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExportClip {
    /// 目标节点总数（通道的 `target_node` 必须落在 `0..node_count`）。
    pub node_count: u32,
    /// 轨道清单。
    pub tracks: Vec<ExportTrack>,
}

impl ExportClip {
    /// 造一个片段。
    pub fn new(node_count: u32, tracks: Vec<ExportTrack>) -> ExportClip {
        ExportClip { node_count, tracks }
    }
}

// ---------------------------------------------------------------------------
// 五、导出产物（**类型即 F2409 的输入类型** —— 双向的类型级保证）
// ---------------------------------------------------------------------------

/// 导出产物。
///
/// **`doc` 的类型就是 `vem09_import::GltfAnimDoc`**——导入器的输入类型。
/// 这不是省事，是**判据「glTF 双向」的类型级兑现**：导出与导入共用同一个
/// 文档类型，意味着「导出的东西导不回来」这件事在类型上就不可能发生，
/// 而不需要任何人记得写一个转换器。
#[derive(Clone, Debug, PartialEq)]
pub struct ExportedAnim {
    /// glTF 文档（**可直接喂给 `import_gltf_anim`**）。
    pub doc: GltfAnimDoc,
    /// 导出报告。
    pub report: ExportReport,
}

// ---------------------------------------------------------------------------
// 六、三要素拒绝：什么错 / 在哪 / 怎么办
// ---------------------------------------------------------------------------

/// 导出三要素错误。
///
/// 与 F2409 的 `ImportError` 同构：**三要素缺一即视为降级成一句「导出失败」**
/// ——那等于把定位成本全推给调用方。锚点错误矩阵「导出数据 NaN → 拒绝导出
/// （先清洗提示）」里的「提示」就是要素二 + 要素三。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportError {
    /// 要素一：错误码。
    pub code: DiagCode,
    /// 要素二：定位（轨道下标 / 关键帧下标 / 通道下标）。
    pub locator: String,
    /// 要素三：处置建议（怎么办）。
    pub hint: String,
}

impl ExportError {
    /// 造三要素错误。
    pub fn new(code: DiagCode, locator: &str, hint: &str) -> ExportError {
        ExportError { code, locator: locator.to_string(), hint: hint.to_string() }
    }

    /// 三要素是否齐备（**非空**才算齐备）。
    pub fn three_elements_complete(&self) -> bool {
        !self.locator.is_empty() && !self.hint.is_empty()
    }

    /// 人话渲染。
    pub fn render(&self) -> String {
        format!("[{}] {} · 定位：{} · 处置：{}", self.code.0, self.code.label(), self.locator, self.hint)
    }
}

// ---------------------------------------------------------------------------
// 七、导出报告（三要素：通道数 / 采样点数 / 精度说明）
// ---------------------------------------------------------------------------

/// 一条通道的导出记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelRecord {
    /// 通道下标（导出文档里的）。
    pub channel: u32,
    /// 目标节点。
    pub target_node: u32,
    /// glTF 路径。
    pub path: ChannelPath,
    /// M 域语义。
    pub semantic: TrackSemantic,
    /// 合并进本通道的 M 域轨道数（**morph > 1**：多轨合并为单 weights 通道）。
    pub sources: u32,
    /// 导出前帧数。
    pub keys_in: u32,
    /// 导出后帧数（钳制抽稀后）。
    pub keys_out: u32,
    /// 输出分量数（`comps`）。
    pub comps: u8,
    /// 处置码（`None` = 正常导出）。
    pub verdict: Option<DiagCode>,
}

impl ChannelRecord {
    /// 人话渲染（零指纹）。
    pub fn render(&self) -> String {
        let v = match self.verdict {
            Some(c) => format!("处置：{}", c.label()),
            None => String::from("处置：正常导出"),
        };
        format!(
            "ch{} n{} {} → {}（{} 源轨，帧 {}→{}，{} 分量）{}",
            self.channel,
            self.target_node,
            self.path.label(),
            self.semantic.label(),
            self.sources,
            self.keys_in,
            self.keys_out,
            self.comps,
            v
        )
    }
}

/// 一条导出声明（**跳过必留**，判据四）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportDeclaration {
    /// 关联码。
    pub code: DiagCode,
    /// 指名对象（轨道名 / 通道下标——**指名，不许「部分轨道」**）。
    pub subject: String,
    /// 人话说明。
    pub text: String,
}

impl ExportDeclaration {
    /// 构造。
    pub fn new(code: DiagCode, subject: &str, text: &str) -> ExportDeclaration {
        ExportDeclaration { code, subject: subject.to_string(), text: text.to_string() }
    }

    /// 正文是否非空（无障碍：声明要能被替述读出）。
    pub fn text_present(&self) -> bool {
        !self.subject.is_empty() && !self.text.is_empty()
    }
}

/// 精度声明（**锚点判据三：导出不是无损承诺**）。
///
/// **它为什么是一份可机检的数据结构而不是一段文档字符串**：文档字符串会
/// 与实现漂移（F2405 家族「`pub` 文本常量须有同源门禁」的教训）。这里每一项
/// 都是**由导出过程真实统计出来的数字**，判据拿它与实际改写计数对账。
#[derive(Clone, Debug, PartialEq)]
pub struct PrecisionStatement {
    /// 时刻量化：导出时刻单位为 f32 秒（源为 u32 毫秒）⇒ **有损**。
    pub time_quantized_to_seconds: bool,
    /// 时刻往返逐位相等的上界（毫秒）。
    pub time_exact_max_ms: u32,
    /// 四元数导出前归一化改写的帧数（>0 ⇒ 源数据不干净）。
    pub quat_frames_rewritten: u32,
    /// 切线丢弃声明（CUBICSPLINE → LINEAR）。
    pub tangent_dropped: bool,
    /// 被跳过的离散轨数（**不静默丢轨**）。
    pub discrete_skipped: u32,
    /// 被钳制的通道数（采样率超限抽稀）。
    pub rate_clamped_channels: u32,
    /// 时刻容差（毫秒）。
    pub time_tolerance_ms: u32,
    /// 值容差（线性通道）。
    pub value_tolerance: f32,
}

impl PrecisionStatement {
    /// 全零精度声明（**默认态即「什么都没承诺」**）。
    ///
    /// **为什么手写而不 derive `Default`**：本结构含 `f32` 字段
    /// （`value_tolerance`），derive 会要求 `f32: Default`——那没问题，
    /// 但真正的问题是 derive 出来的全零 `value_tolerance == 0.0` 会让
    /// 「容差为零 = 最严格」这个读法成立。宁可让默认态显式写出真实容差
    /// 常量：默认即诚实。
    pub fn zero() -> PrecisionStatement {
        PrecisionStatement {
            time_quantized_to_seconds: true,
            time_exact_max_ms: TIME_EXACT_MAX_MS,
            quat_frames_rewritten: 0,
            tangent_dropped: false,
            discrete_skipped: 0,
            rate_clamped_channels: 0,
            time_tolerance_ms: TOL_TIME_MS,
            value_tolerance: TOL_VALUE_LINEAR,
        }
    }

    /// 人话渲染（**必须逐项列出有损点，且不得出现「正面承诺无损」措辞**）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let _ = s.push_str("导出精度声明（本导出为有损导出，不承诺无损）：\n");
        let _ = s.push_str(&format!(
            "· 时刻量化：源为 u32 毫秒，导出为 f32 秒；{} ms 以内往返逐位相等；超界时刻按该值自身的 f32 秒表示逐点判定（偏差恒 ≤ {} ms，精确值集合不连续）\n",
            self.time_exact_max_ms, self.time_tolerance_ms
        ));
        let _ = s.push_str(&format!(
            "· 值容差：线性通道 ≤ {}，旋转分量 ≤ {}（含归一化改写 {} 帧）\n",
            self.value_tolerance,
            TOL_VALUE_QUAT,
            self.quat_frames_rewritten
        ));
        let _ = s.push_str(&format!(
            "· 插值降级：切线不落盘，{}；导出采样率上限 1000 Hz，钳制 {} 个通道\n",
            if self.tangent_dropped { "CUBICSPLINE 已降级为 LINEAR" } else { "无切线丢弃" },
            self.rate_clamped_channels
        ));
        let _ = s.push_str(&format!(
            "· 语义边界：离散轨（布尔/事件）不在 glTF 语义内，已跳过并声明 {} 条\n",
            self.discrete_skipped
        ));
        s
    }

    /// **反向断言**：渲染文本**不得**出现「无损」承诺（判据三的负向半边）。
    ///
    /// **为什么不能用 `!contains("无损")` 那种写法**：本条的诚实文案里
    /// 恰恰要写「**不承诺无损**」——那四个字里就含「无损」两字。若判据写
    /// `!render().contains("无损")`，它会恒红，而要让它转绿就得删掉
    /// 「不承诺无损」这句**最该说的话**——于是判据反过来逼着实现撒谎。
    ///
    /// 正确口径：**逐条禁止「正面承诺」词组**，而不是禁止「无损」二字：
    /// 禁 `完全无损` / `无损导出` / `保证无损` / `零损失` / `无损往返`。
    /// 这样「不承诺无损」「有损」等诚实表述不受影响，而任何把话说满的
    /// 措辞都会被抓。这与「关键词表要把错误固化成门禁」是同一条教训的
    /// 正用：反向断言必须针对**错误措辞**，而非针对**正确措辞里的子串**。
    pub fn forbidden_claims_absent(&self) -> bool {
        let r = self.render();
        !r.contains("完全无损")
            && !r.contains("无损导出")
            && !r.contains("保证无损")
            && !r.contains("零损失")
            && !r.contains("无损往返")
    }

    /// **正向断言**：诚实文案必须真的写明「有损」与「不承诺无损」。
    ///
    /// 与 [`Self::forbidden_claims_absent`] 配对：只断负向的话，把整段
    /// 精度声明删空也能过（`contains` 全部为 false ⇒ 「无禁止词」）。
    pub fn honesty_wording_present(&self) -> bool {
        let r = self.render();
        r.contains("有损") && r.contains("不承诺无损")
    }
}

/// 导出报告（**结构化三要素**：通道数 / 采样点数 / 精度说明）。
#[derive(Clone, Debug, PartialEq)]
pub struct ExportReport {
    /// 输入轨道总数。
    pub tracks_in: u32,
    /// 导出通道数。
    pub channels_out: u32,
    /// 跳过的轨道数。
    pub tracks_skipped: u32,
    /// 输入关键帧总数（钳制前）。
    pub keys_in: u64,
    /// 输出关键帧总数（钳制后）。
    pub keys_out: u64,
    /// 通道记录表。
    pub channels: Vec<ChannelRecord>,
    /// 声明清单（**跳过必留**）。
    pub declarations: Vec<ExportDeclaration>,
    /// 精度声明。
    pub precision: PrecisionStatement,
}

impl Default for ExportReport {
    /// 默认态：计数归零、精度声明取 `PrecisionStatement::zero()`。
    fn default() -> ExportReport {
        ExportReport {
            tracks_in: 0,
            channels_out: 0,
            tracks_skipped: 0,
            keys_in: 0,
            keys_out: 0,
            channels: Vec::new(),
            declarations: Vec::new(),
            precision: PrecisionStatement::zero(),
        }
    }
}

impl ExportReport {
    /// 钳制抽稀掉的关键帧数。
    pub fn keys_removed(&self) -> u64 {
        self.keys_in.saturating_sub(self.keys_out)
    }

    /// 抽稀率（整数口径 `(分子, 分母)`，判据精确对账，不用浮点除）。
    pub fn clamp_ratio(&self) -> (u64, u64) {
        (self.keys_removed(), self.keys_in)
    }

    /// 跳过的轨道数（**独立重算**，不读 `tracks_skipped`）。
    pub fn skipped_records(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.channels.len() {
            if self.channels[i].verdict.is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 采样点数是否**恰等于**各通道 `keys_out` 之和（**恰等于**，不用 `>=`）。
    pub fn keys_match_channels(&self) -> bool {
        let mut sum: u64 = 0;
        let mut i = 0usize;
        while i < self.channels.len() {
            sum += self.channels[i].keys_out as u64;
            i += 1;
        }
        sum == self.keys_out
    }

    /// 通道记录数是否恰等于导出通道数。
    pub fn records_cover_channels(&self) -> bool {
        self.channels.len() as u32 == self.channels_out
    }

    /// 全部声明是否齐备（**非空主体 + 非空正文**）。
    pub fn all_declarations_readable(&self) -> bool {
        let mut i = 0usize;
        while i < self.declarations.len() {
            if !self.declarations[i].text_present() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// **零指纹断言**：报告任何位置都不得含资产标签。
    ///
    /// 判据侧用「非空的资产标签」调用；若实现某天把标签拼进轨道名/声明正文，
    /// 这里立刻转红。负向断言必须有一个**非空的被排除串**，否则恒真。
    pub fn fingerprint_free(&self, asset_label: &str) -> bool {
        if asset_label.is_empty() {
            return false;
        }
        let mut i = 0usize;
        while i < self.channels.len() {
            if self.channels[i].render().contains(asset_label) {
                return false;
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < self.declarations.len() {
            if self.declarations[j].subject.contains(asset_label)
                || self.declarations[j].text.contains(asset_label)
            {
                return false;
            }
            j += 1;
        }
        true
    }

    /// 人话渲染（三要素齐出）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let _ = s.push_str(&format!(
            "动画导出报告：输入轨道 {} → 导出通道 {}（跳过 {}）；关键帧 {}→{}（钳制 {}）\n",
            self.tracks_in,
            self.channels_out,
            self.tracks_skipped,
            self.keys_in,
            self.keys_out,
            self.keys_removed()
        ));
        let _ = s.push_str("通道表：\n");
        let mut i = 0usize;
        while i < self.channels.len() {
            let _ = s.push_str(&format!("  {}\n", self.channels[i].render()));
            i += 1;
        }
        if !self.declarations.is_empty() {
            let _ = s.push_str(&format!("声明清单（{} 条）：\n", self.declarations.len()));
            let mut k = 0usize;
            while k < self.declarations.len() {
                let _ = s.push_str(&format!(
                    "  [{}] {}：{}\n",
                    self.declarations[k].code.0,
                    self.declarations[k].subject,
                    self.declarations[k].text
                ));
                k += 1;
            }
        }
        let _ = s.push_str(&self.precision.render());
        s
    }
}

// ---------------------------------------------------------------------------
// 八、采样率钳制（端点钉死 + 对最终保留集重算间隔）
// ---------------------------------------------------------------------------

/// 抽稀钳制结果。
#[derive(Clone, Debug, PartialEq)]
pub struct ClampOutcome {
    /// 保留的关键帧下标（**升序**；首末必含——端点钉死）。
    pub kept: Vec<usize>,
    /// 实际最小间隔（毫秒；对**最终保留集**重算，不是贪心过程中判的）。
    pub min_dt_ms: u32,
    /// 是否发生了钳制。
    pub clamped: bool,
}

/// 采样率钳制：把帧间隔钳到 ≥ `min_dt_ms`，**端点钉死**。
///
/// **为什么端点钉死**（F2409 `thin_keys` 同族纪律）：
/// 丢末帧会让动画短一截——那在预览里是「动画怎么短了一节」，极难归因；
/// 丢中间帧只是少一个采样点。两者代价不对称，所以末帧必保留。
///
/// **为什么间隔要对最终保留集重算**：贪心过程中「保留 k 的下一帧」判据在
/// 删除 k 的近邻后会失效——逐轮重建才能保证留下的集合**真的**满足约束。
///
/// **「全零间隔」下界（实测踩过的坑）**：`times = [0,0,0,0]` 这种全部同刻的
/// 语料，若末帧用「替换上一保留帧」，会把 `[0]` 之后的中间帧逐个吃掉，最终
/// 只剩 1 帧——**动画被压成单帧**，而报告里 `keys_in=4 / keys_out=1` 看不出
/// 任何异常。正确处置：保留集至少留 2 帧（首 + 末），因为**单帧无法表达
/// 「这段时间没有动画」与「这段时间只有一个值」的区别**。故末帧**追加**
/// 而非替换，仅当追加会破坏最小间隔约束时才改为替换。
pub fn clamp_sample_rate(times: &[u32], min_dt_ms: u32) -> ClampOutcome {
    if times.is_empty() {
        return ClampOutcome { kept: Vec::new(), min_dt_ms: 0, clamped: false };
    }
    let last = times.len() - 1;
    let mut kept: Vec<usize> = Vec::new();
    kept.push(0);
    let mut last_kept_time: u32 = times[0];
    let mut clamped = false;
    let mut i = 1usize;
    while i <= last {
        let t = times[i];
        if i == last {
            // 末帧**无条件追加**（端点钉死的最强形式）。
            //
            // 实测踩过的坑：本条最初写成「间隔不足则替换上一保留帧」，
            // 结果 `[0,0,0,0]` 全零间隔语料把保留集压成 **单帧 `[3]`**——
            // 动画被静默压成单帧，而报告里 `keys_in=4 / keys_out=1` 看不出
            // 任何异常；`[5,5]` 同样退化成 `[1]`。根因是「替换」让末帧
            // 顶掉了首帧，保留集永远只剩 1 个元素。
            //
            // **为什么这里可以让步间隔约束**：钳制的目的是「采样率不超上限」
            // （防下游按固定步长消费时错位），而末帧多留一帧的后果是
            // 「末帧间隔偏小」——后者只影响下游的**最末一个采样间隔**，
            // 而前者会让整段动画退化成单帧。两者代价差着量级。
            //
            // 单帧无法区分「这段时间没有动画」与「这段时间只有一个值」，
            // 因此保留集**必须 ≥2 帧**。
            if kept.last() != Some(&i) {
                kept.push(i);
            }
            clamped = clamped || t.saturating_sub(last_kept_time) < min_dt_ms;
            i += 1;
            continue;
        }
        if t.saturating_sub(last_kept_time) >= min_dt_ms {
            kept.push(i);
            last_kept_time = t;
        } else {
            clamped = true;
        }
        i += 1;
    }
    // 对最终保留集重算最小间隔。
    let mut min_dt = u32::MAX;
    let mut k = 1usize;
    while k < kept.len() {
        let a = times[kept[k - 1]];
        let b = times[kept[k]];
        let d = b.saturating_sub(a);
        if d < min_dt {
            min_dt = d;
        }
        k += 1;
    }
    if kept.len() < 2 {
        min_dt = 0;
    }
    ClampOutcome { kept, min_dt_ms: min_dt, clamped }
}

// ---------------------------------------------------------------------------
// 九、数值工具（自持；`fsqrt` / `secs_to_ms` 复用 F2409 单源）
// ---------------------------------------------------------------------------

/// 毫秒 → f32 秒（**导出时刻量化**）。
///
/// **为什么除以 1000 而不是乘 0.001**：`0.001` 在 f32 里不是精确表示，
/// `ms * 0.001` 与 `ms / 1000.0` 在大 ms 处给出不同的位模式。导出侧固定
/// 用 `/1000.0`，往返对称性判据依赖这一点。
pub const fn ms_to_secs(ms: u32) -> f32 {
    ms as f32 / 1000.0
}

/// 四元数导出前归一化（就地改写 4 分量）。返回是否发生改写。
///
/// **复用 F2409 的 `normalize_quat`**：归一化口径必须两侧一致——导入侧
/// 归一化、导出侧也归一化，两边用同一份实现，往返容差才成立。
pub fn normalize_quat_for_export(out: &mut [f32]) -> bool {
    if out.len() < 4 {
        return false;
    }
    let mut sum = 0.0f32;
    let mut i = 0usize;
    while i < 4 {
        let v = if out[i].is_finite() { out[i] } else { 0.0 };
        out[i] = v;
        sum += v * v;
        i += 1;
    }
    let n = fsqrt(sum);
    if !(n > 0.0) {
        out[0] = 0.0;
        out[1] = 0.0;
        out[2] = 0.0;
        out[3] = 1.0;
        return true;
    }
    let inv = 1.0 / n;
    let mut changed = false;
    let mut k = 0usize;
    while k < 4 {
        let want = out[k] * inv;
        if want != out[k] {
            changed = true;
        }
        out[k] = want;
        k += 1;
    }
    changed
}

/// 四元数**夹角差**口径（`1 - |dot|`），对 `q` / `-q` 符号翻转免疫。
///
/// 返回 `0.0..=2.0`。注意这是 `1 - |dot|` 而**不是**弧度——弧度需要
/// `acos`，真 `no_std` 下不存在（见 F2409 `fsqrt` 头注的同款理由）。
/// 判据用 `TOL_ROT_SLERP_RAD` 与本值比较，两者同量纲（都是「越小越接近」）。
pub fn quat_angular_gap(a: &[f32], b: &[f32]) -> f32 {
    if a.len() < 4 || b.len() < 4 {
        return 2.0;
    }
    let mut dot = 0.0f32;
    let mut i = 0usize;
    while i < 4 {
        dot += a[i] * b[i];
        i += 1;
    }
    let ad = if dot < 0.0 { -dot } else { dot };
    // `ad` 可能因浮点误差略超 1，钳到 1 免得 gap 变负。
    let clamped = if ad > 1.0 { 1.0 } else { ad };
    1.0 - clamped
}

/// 分量最大绝对差（**线性通道口径**）。
pub fn component_max_diff(a: &[f32], b: &[f32]) -> f32 {
    let n = if a.len() < b.len() { a.len() } else { b.len() };
    let mut m = 0.0f32;
    let mut i = 0usize;
    while i < n {
        let d = a[i] - b[i];
        let ad = if d < 0.0 { -d } else { d };
        if ad > m {
            m = ad;
        }
        i += 1;
    }
    m
}

// ---------------------------------------------------------------------------
// 十、导出主流程
// ---------------------------------------------------------------------------

/// 内部：一条已通过前置校验的待导出轨。
struct StagedTrack {
    src_index: usize,
    semantic: TrackSemantic,
    path: ChannelPath,
    target_node: u32,
    morph_slot: u16,
    times: Vec<u32>,
    /// 展平通道值（`times.len() × semantic.lanes()`，**已归一化**）。
    values: Vec<f32>,
    interp: GltfInterp,
    quat_rewritten: u32,
}

/// 校验一条轨道并抽取为 `StagedTrack`；失败给出**三要素错误**。
///
/// **顺序有意义**：先形状（`channels != times × lanes`）再非有限值。若反了，
/// 一个形状不自洽的轨道会先撞上非有限检查并报「请先清洗」——那是**误导**：
/// 数据脏不是它被拒的原因，形状不自洽才是。
fn stage_track(
    clip: &ExportClip,
    idx: usize,
    bag: &mut DiagBag,
) -> Result<StagedTrack, ExportError> {
    let t = &clip.tracks[idx];
    let loc = format!("轨道#{}", idx);
    let lanes = t.semantic.lanes();

    // —— 形状 ——
    if t.track.channels.len() != t.track.times.len() * lanes {
        return Err(ExportError::new(
            DiagCode::TRACK_SHAPE_INVALID,
            &format!("{}（{} 帧 × {} 分量 ≠ {} 值）", loc, t.track.times.len(), lanes, t.track.channels.len()),
            "修正轨道通道数或帧数后重试；本条不做静默截断",
        ));
    }
    // —— 节点范围 ——
    if t.target_node >= clip.node_count {
        return Err(ExportError::new(
            DiagCode::TARGET_NODE_OOB,
            &format!("{}（节点 {} ≥ 节点数 {}）", loc, t.target_node, clip.node_count),
            "核对目标节点下标或调大 node_count",
        ));
    }
    // —— 关键帧配额 ——
    if t.track.times.len() > MAX_KEYS_PER_CHANNEL {
        return Err(ExportError::new(
            DiagCode::KEYS_OVER_QUOTA,
            &format!("{}（{} 帧 > 配额 {}）", loc, t.track.times.len(), MAX_KEYS_PER_CHANNEL),
            "抽稀或拆分 clip 后重试；本条不截断",
        ));
    }
    // —— 时刻：非有限 + 非单调 ——
    let mut i = 0usize;
    while i < t.track.times.len() {
        let v = t.track.times[i];
        // `times` 是 `u32`，不存在 NaN；但**上游可能塞了钳制后的垃圾**，
        // 所以此处检查单调性即可，非有限在值侧统一处理。
        if i > 0 && v < t.track.times[i - 1] {
            return Err(ExportError::new(
                DiagCode::TIMES_NON_MONOTONIC,
                &format!("{}（帧 {} 时刻 {} < 帧 {} 时刻 {}）", loc, i, v, i - 1, t.track.times[i - 1]),
                "先排序或删除逆序帧；glTF 要求时刻非递减",
            ));
        }
        i += 1;
    }
    // —— 值：非有限 → **拒绝导出**（判据：与导入侧处置相反，见头注第 5 条）——
    let mut j = 0usize;
    while j < t.track.channels.len() {
        if !t.track.channels[j].is_finite() {
            return Err(ExportError::new(
                DiagCode::VALUE_NON_FINITE,
                &format!("{}（帧 {} 分量 {} = {}）", loc, j / lanes, j % lanes, t.track.channels[j]),
                "先清洗该分量再导出；本条不导出坏数据（下游无法区分脏资产与坏导出器）",
            ));
        }
        j += 1;
    }

    // —— 采样率钳制（端点钉死）——
    let cl = clamp_sample_rate(&t.track.times, MIN_KEY_DT_MS);
    if cl.clamped {
        bag.push_major(DiagCode::RATE_CLAMPED);
    }

    // —— 抽取值（旋转逐帧归一化）——
    let mut values: Vec<f32> = Vec::new();
    let mut quat_rewritten = 0u32;
    let mut k = 0usize;
    while k < cl.kept.len() {
        let src = cl.kept[k];
        let base = src * lanes;
        let mut buf: [f32; 4] = [0.0f32; 4];
        let mut c = 0usize;
        while c < lanes {
            buf[c] = t.track.channels[base + c];
            c += 1;
        }
        if t.semantic == TrackSemantic::Rotation && normalize_quat_for_export(&mut buf) {
            quat_rewritten = quat_rewritten.saturating_add(1);
        }
        let mut d = 0usize;
        while d < lanes {
            values.push(buf[d]);
            d += 1;
        }
        k += 1;
    }
    if quat_rewritten > 0 {
        bag.push_major(DiagCode::QUAT_NORMALIZED);
    }

    // —— 插值模式降级（切线不可导出）——
    let gi = if t.semantic == TrackSemantic::Rotation {
        // 旋转轨恒 LINEAR：slerp 由 F2423 在域内做，glTF 侧 rotation 通道
        // 用 LINEAR 是规范要求（slerp 由导入器归一化后线性重建）。
        GltfInterp::Linear
    } else {
        match t.interp {
            Interp::Step => GltfInterp::Step,
            Interp::Linear => GltfInterp::Linear,
            Interp::CubicBezier | Interp::Eased | Interp::Custom => {
                bag.push_major(DiagCode::TANGENT_DROPPED);
                GltfInterp::Linear
            }
        }
    };

    let times: Vec<u32> = {
        let mut v: Vec<u32> = Vec::new();
        let mut m = 0usize;
        while m < cl.kept.len() {
            v.push(t.track.times[cl.kept[m]]);
            m += 1;
        }
        v
    };

    Ok(StagedTrack {
        src_index: idx,
        semantic: t.semantic,
        path: ChannelPath::Other, // 由调用方按逆表填
        target_node: t.target_node,
        morph_slot: t.morph_slot,
        times,
        values,
        interp: gi,
        quat_rewritten,
    })
}

/// 导出动画。
///
/// 流程：逆表对账 → 逐轨校验/钳制 → **同节点 morph 合轨** → 组装文档 →
/// 三要素报告。返回 `Err` 即**整体中止**（不产出半份文档）。
#[allow(clippy::too_many_arguments)]
pub fn export_gltf_anim(
    clip: &ExportClip,
    table: &mut ExportMapTable,
    forward: &MappingTable,
    bag: &mut DiagBag,
) -> Result<ExportedAnim, ExportError> {
    // —— 逆表对账（漂移即拦截；拦截态下整体拒绝）——
    let _ = table.reconcile(forward, bag);
    if table.intercepted() {
        bag.push(DiagCode::MAP_INTERCEPTED);
        return Err(ExportError::new(
            DiagCode::MAP_INTERCEPTED,
            "逆映射表",
            "逆表与正向表对不上；核对四通道映射规格后调 clear_intercept 显式解除",
        ));
    }

    let mut report = ExportReport {
        tracks_in: clip.tracks.len() as u32,
        ..ExportReport::default()
    };

    // —— 第一遍：语义边界（跳过必留声明）+ 前置校验 ——
    let mut staged: Vec<StagedTrack> = Vec::new();
    let mut skipped = 0u32;
    let mut keys_in_total: u64 = 0;
    let mut i = 0usize;
    while i < clip.tracks.len() {
        let t = &clip.tracks[i];
        // 判据四：离散轨跳过 + 指名声明。
        if t.is_discrete() {
            skipped += 1;
            bag.push_major(DiagCode::DISCRETE_SKIPPED);
            report.declarations.push(ExportDeclaration::new(
                DiagCode::DISCRETE_SKIPPED,
                &t.name,
                "离散轨（布尔/事件）不在 glTF 语义内，本条不导出；内容不会出现在产物中",
            ));
            i += 1;
            continue;
        }
        // 空轨跳过 + 声明。
        if t.track.times.is_empty() {
            skipped += 1;
            bag.push(DiagCode::EMPTY_TRACK);
            report.declarations.push(ExportDeclaration::new(
                DiagCode::EMPTY_TRACK,
                &t.name,
                "轨道零关键帧，无内容可导出",
            ));
            i += 1;
            continue;
        }
        // 形态键槽位缺失。
        if t.semantic == TrackSemantic::MorphWeight && t.morph_slot == u16::MAX {
            skipped += 1;
            bag.push_major(DiagCode::MORPH_SLOT_MISSING);
            report.declarations.push(ExportDeclaration::new(
                DiagCode::MORPH_SLOT_MISSING,
                &t.name,
                "形态键轨未给槽位号，无法并入 weights 通道",
            ));
            i += 1;
            continue;
        }
        // 无逆映射（理论上四语义全覆盖，此处是防御：逆表被外部构造成缺行）。
        if table.path_of(t.semantic).is_none() {
            skipped += 1;
            bag.push_major(DiagCode::UNMAPPED_SEMANTIC);
            report.declarations.push(ExportDeclaration::new(
                DiagCode::UNMAPPED_SEMANTIC,
                &t.name,
                "该语义在逆映射表中无对应 glTF 路径，已跳过",
            ));
            i += 1;
            continue;
        }
        // 硬错误：前置校验不通过 ⇒ **整体中止**（不导出半份）。
        let st = stage_track(clip, i, bag)?;
        // `keys_in` 记**源帧数**（钳制前），不是钳制后的——
        // 否则 `keys_removed()` 恒为 0，「钳制抽稀了多少」这个信息就丢了，
        // 而报告里看不出任何区别。记错口径的后果是报告看起来正常但无意义。
        keys_in_total += clip.tracks[i].track.times.len() as u64;
        staged.push(st);
        i += 1;
    }

    if staged.is_empty() {
        bag.push(DiagCode::NOTHING_EXPORTED);
        return Err(ExportError::new(
            DiagCode::NOTHING_EXPORTED,
            "全部轨道",
            "没有可导出的通道；检查轨道类（离散轨不可导出）与节点下标",
        ));
    }

    // —— 第二遍：morph 合轨（F2441 对接）——
    // 同一节点的多个形态键轨**必须**并成**一条** weights 通道：glTF 的
    // `weights` 通道 comps = 形态键数，逐键一条通道会被导入器拆成 comps=1
    // 的多条通道——往返后轨道数不守恒。
    // 合轨要求**时间轴逐点相同**（glTF 一个 sampler 只带一个 input accessor）。
    // 时间轴不同则不能合，此时该轨单独成一条 comps=1 的 weights 通道并声明
    // （诚实降级好过静默改时间轴——改时间轴会改变动画）。
    let mut groups: Vec<MorphGroup> = Vec::new();
    let mut non_morph: Vec<StagedTrack> = Vec::new();
    {
        let mut m = 0usize;
        while m < staged.len() {
            if staged[m].semantic == TrackSemantic::MorphWeight {
                // 找到同节点已有的组。
                let mut found = false;
                let mut g = 0usize;
                while g < groups.len() {
                    if groups[g].node == staged[m].target_node
                        && groups[g].times_equal(&staged[m].times)
                    {
                        groups[g].members.push(m);
                        found = true;
                        break;
                    }
                    g += 1;
                }
                if !found {
                    let mut members: Vec<usize> = Vec::new();
                    members.push(m);
                    groups.push(MorphGroup {
                        node: staged[m].target_node,
                        times: staged[m].times.clone(),
                        members,
                    });
                }
            } else {
                non_morph.push(clone_staged(&staged[m]));
            }
            m += 1;
        }
    }
    // 无法合轨的 morph 轨（时间轴不一致）单独成组，comps = 1。
    {
        let mut grouped: Vec<bool> = vec![false; staged.len()];
        let mut gi = 0usize;
        while gi < groups.len() {
            let mut mi = 0usize;
            while mi < groups[gi].members.len() {
                grouped[groups[gi].members[mi]] = true;
                mi += 1;
            }
            gi += 1;
        }
        let mut si = 0usize;
        while si < staged.len() {
            if staged[si].semantic == TrackSemantic::MorphWeight && !grouped[si] {
                let mut members: Vec<usize> = Vec::new();
                members.push(si);
                groups.push(MorphGroup {
                    node: staged[si].target_node,
                    times: staged[si].times.clone(),
                    members,
                });
                bag.push_major(DiagCode::TANGENT_DROPPED);
                report.declarations.push(ExportDeclaration::new(
                    DiagCode::TANGENT_DROPPED,
                    "morph 时间轴不一致",
                    "同节点形态键轨时间轴不同，未合并为多分量通道，各占一条 comps=1 的 weights 通道",
                ));
            }
            si += 1;
        }
    }

    // —— 第三遍：组装 accessors / samplers / channels ——
    // 时刻 accessor **按内容去重共享**：同一时间轴的多个通道共用一个 input
    // accessor（glTF 允许，正是推荐做法），产物更小且往返等价。
    let mut accessors: Vec<AccessorView> = Vec::new();
    let mut samplers: Vec<SamplerRef> = Vec::new();
    let mut channels: Vec<ChannelRef> = Vec::new();
    let mut keys_out_total: u64 = 0;
    let mut quat_frames_rewritten: u32 = 0;
    let mut tangent_dropped = false;

    // 时刻 accessor 去重表：`Vec<Option<u32>>`（索引 = 时刻向量在表中的槽）。
    let mut time_slots: Vec<Vec<u32>> = Vec::new();

    // —— 非形态键通道：每轨一条 ——
    let mut ni = 0usize;
    while ni < non_morph.len() {
        let st = &non_morph[ni];
        let path = match table.path_of(st.semantic) {
            Some(p) => p,
            None => {
                // 防御性：上表已确认有映射，且未被拦截。
                skipped += 1;
                ni += 1;
                continue;
            }
        };
        let tslot = intern_time_slot(&mut time_slots, &mut accessors, &st.times);
        let out_idx = push_value_accessor(&mut accessors, &st.values, st.semantic.lanes() as u8);
        samplers.push(SamplerRef { input: tslot, output: out_idx, interp: st.interp });
        let ch = channels.len() as u32;
        channels.push(ChannelRef { target_node: st.target_node, path, sampler: ch });
        keys_out_total += st.times.len() as u64;
        quat_frames_rewritten = quat_frames_rewritten.saturating_add(st.quat_rewritten);
        report.channels.push(ChannelRecord {
            channel: ch,
            target_node: st.target_node,
            path,
            semantic: st.semantic,
            sources: 1,
            keys_in: st.times.len() as u32,
            keys_out: st.times.len() as u32,
            comps: st.semantic.lanes() as u8,
            verdict: None,
        });
        ni += 1;
    }

    // —— 形态键通道：每组一条（comps = 成员数）——
    let mut gi = 0usize;
    while gi < groups.len() {
        let g = &groups[gi];
        let tslot = intern_time_slot(&mut time_slots, &mut accessors, &g.times);
        let comps = g.members.len() as u8;
        // 交织布局：glTF 的 weights output accessor 是 `key × comps` 行主序。
        let mut vals: Vec<f32> = Vec::new();
        let mut key = 0usize;
        while key < g.times.len() {
            let mut c = 0usize;
            while c < g.members.len() {
                let st = &staged[g.members[c]];
                let base = key * 1; // 每条 morph 轨 lanes = 1
                if base < st.values.len() {
                    vals.push(st.values[base]);
                } else {
                    vals.push(0.0);
                }
                c += 1;
            }
            key += 1;
        }
        let out_idx = push_value_accessor(&mut accessors, &vals, comps);
        samplers.push(SamplerRef { input: tslot, output: out_idx, interp: GltfInterp::Linear });
        let ch = channels.len() as u32;
        let path = table.path_of(TrackSemantic::MorphWeight).unwrap_or(ChannelPath::Weights);
        channels.push(ChannelRef { target_node: g.node, path, sampler: ch });
        keys_out_total += g.times.len() as u64;
        let mut member = 0usize;
        while member < g.members.len() {
            quat_frames_rewritten =
                quat_frames_rewritten.saturating_add(staged[g.members[member]].quat_rewritten);
            member += 1;
        }
        report.channels.push(ChannelRecord {
            channel: ch,
            target_node: g.node,
            path,
            semantic: TrackSemantic::MorphWeight,
            sources: comps as u32,
            keys_in: g.times.len() as u32,
            keys_out: g.times.len() as u32,
            comps,
            verdict: None,
        });
        gi += 1;
    }

    if channels.is_empty() {
        bag.push(DiagCode::NOTHING_EXPORTED);
        return Err(ExportError::new(
            DiagCode::NOTHING_EXPORTED,
            "全部轨道",
            "合轨后无通道可产出",
        ));
    }

    if bag.has(DiagCode::TANGENT_DROPPED) {
        tangent_dropped = true;
    }

    // —— 报告收口 ——
    report.channels_out = channels.len() as u32;
    report.tracks_skipped = skipped;
    report.keys_in = keys_in_total;
    report.keys_out = keys_out_total;
    report.precision = PrecisionStatement {
        time_quantized_to_seconds: true,
        time_exact_max_ms: TIME_EXACT_MAX_MS,
        quat_frames_rewritten,
        tangent_dropped,
        discrete_skipped: report
            .declarations
            .iter()
            .filter(|d| d.code == DiagCode::DISCRETE_SKIPPED)
            .count() as u32,
        rate_clamped_channels: bag.count_of(DiagCode::RATE_CLAMPED) as u32,
        time_tolerance_ms: TOL_TIME_MS,
        value_tolerance: TOL_VALUE_LINEAR,
    };

    let doc = GltfAnimDoc::new(clip.node_count, accessors, samplers, channels, "");
    Ok(ExportedAnim { doc, report })
}

/// 内部：形态键合轨组。
struct MorphGroup {
    node: u32,
    times: Vec<u32>,
    /// 成员在 `staged` 中的下标。
    members: Vec<usize>,
}

impl MorphGroup {
    fn times_equal(&self, other: &[u32]) -> bool {
        if self.times.len() != other.len() {
            return false;
        }
        let mut i = 0usize;
        while i < self.times.len() {
            if self.times[i] != other[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 克隆一条暂存轨（`StagedTrack` 不派生 `Clone` 时的显式克隆）。
fn clone_staged(s: &StagedTrack) -> StagedTrack {
    StagedTrack {
        src_index: s.src_index,
        semantic: s.semantic,
        path: s.path,
        target_node: s.target_node,
        morph_slot: s.morph_slot,
        times: s.times.clone(),
        values: s.values.clone(),
        interp: s.interp,
        quat_rewritten: s.quat_rewritten,
    }
}

/// 时刻向量去重登记：命中已有槽则复用，否则新建 accessor。
fn intern_time_slot(
    slots: &mut Vec<Vec<u32>>,
    accessors: &mut Vec<AccessorView>,
    times: &[u32],
) -> u32 {
    let mut i = 0usize;
    while i < slots.len() {
        let s = &slots[i];
        if s.len() == times.len() {
            let mut same = true;
            let mut k = 0usize;
            while k < times.len() {
                if s[k] != times[k] {
                    same = false;
                    break;
                }
                k += 1;
            }
            if same {
                return i as u32;
            }
        }
        i += 1;
    }
    let slot = slots.len() as u32;
    let mut secs: Vec<f32> = Vec::new();
    let mut k = 0usize;
    while k < times.len() {
        secs.push(ms_to_secs(times[k]));
        k += 1;
    }
    accessors.push(AccessorView::new(
        ComponentType::Float,
        false,
        times.len() as u32,
        1,
        secs,
    ));
    slots.push(times.to_vec());
    slot
}

/// 值 accessor 追加，返回其下标。
fn push_value_accessor(accessors: &mut Vec<AccessorView>, values: &[f32], comps: u8) -> u32 {
    let idx = accessors.len() as u32;
    let frames = if comps == 0 { 0usize } else { values.len() / comps as usize };
    accessors.push(AccessorView::new(ComponentType::Float, false, frames as u32, comps, values.to_vec()));
    idx
}

// ---------------------------------------------------------------------------
// 十一、往返对拍（**真的喂回 F2409 的导入器**）
// ---------------------------------------------------------------------------

/// 往返对拍结论。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoundtripVerdict {
    /// 是否执行了（`false` ⇒ 无源轨或无产物，记 `ROUNDTRIP_UNAVAILABLE`）。
    pub ran: bool,
    /// 比对的轨道数（**独立重算**，不读任何被测计数器）。
    pub tracks_compared: u32,
    /// 比对的关键帧数。
    pub keys_compared: u64,
    /// 时刻最大偏差（毫秒）。
    pub max_time_err_ms: u32,
    /// 线性通道分量最大绝对差。
    pub max_value_diff: f32,
    /// 旋转最大夹角差（`1 - |dot|`）。
    pub max_rot_gap: f32,
    /// 超容差的条目数（**恰等于**判据用）。
    pub violations: u32,
    /// 首个超容差项的定位（无超差时为空串）。
    pub first_locator: String,
    /// 精度对照：导出前轨道数 / 往返后轨道数（**轨道数守恒**是往返的基本账）。
    pub tracks_src: u32,
    pub tracks_roundtrip: u32,
}

impl RoundtripVerdict {
    /// 是否全程在容差内。
    pub fn within_tolerance(&self) -> bool {
        self.violations == 0
    }

    /// 轨道数是否守恒（**恰等于**）。
    pub fn track_count_conserved(&self) -> bool {
        self.ran && self.tracks_src == self.tracks_roundtrip
    }

    /// 人话渲染。
    pub fn render(&self) -> String {
        if !self.ran {
            return String::from("往返对拍：未执行（缺导出产物或源轨）");
        }
        format!(
            "往返对拍：轨道 {}/{} 守恒，帧 {}，时刻最大偏差 {} ms，值最大差 {}，旋转最大夹角差 {}，超差 {} 项{}",
            self.tracks_roundtrip,
            self.tracks_src,
            self.keys_compared,
            self.max_time_err_ms,
            self.max_value_diff,
            self.max_rot_gap,
            self.violations,
            if self.first_locator.is_empty() {
                String::from("")
            } else {
                format!("（首个：{}）", self.first_locator)
            }
        )
    }
}

/// 往返对拍：把导出产物**喂回 F2409 导入器**，再与**源轨道**逐轨逐帧比对。
///
/// **为什么必须走真导入器**（而不是自己写个影子比较器）：
/// 影子比较器只能证明「导出器与影子比较器一致」，证明不了
/// 「导出 → 导入」这条链闭合。而锚点判据二说的正是这条链。
/// 复用 `import_gltf_anim` 还顺带保证了容差表两侧口径一致（同 `fsqrt`、
/// 同 `secs_to_ms`、同 `normalize_quat`——三条数值工具全是单源）。
///
/// **比对对象是 `clip`（源轨道），不是导出产物自己**——这是本函数最容易
/// 写错的一处：拿导出产物与自身比，`max_value_diff` 恒为 0，往返断言成了
/// **恒真门禁**，看起来全绿而实际什么都没验。源轨道必须由调用方传入。
pub fn roundtrip_verify(
    clip: &ExportClip,
    exported: &ExportedAnim,
    bag: &mut DiagBag,
) -> RoundtripVerdict {
    let mut v = RoundtripVerdict::default();

    // —— 无源轨 ⇒ 不可执行（显性记码，不是静默返回「通过」）——
    if clip.tracks.is_empty() {
        bag.push(DiagCode::ROUNDTRIP_UNAVAILABLE);
        v.first_locator = String::from("源轨道为空，无可比对对象");
        return v;
    }

    // —— 真导入 ——
    let mut ibag = vem09_import::DiagBag::new();
    let imported: Result<vem09_import::ImportResult, ImportError> = vem09_import::import_gltf_anim(
        &exported.doc,
        Fidelity::Faithful,
        &MappingTable::standard(),
        &mut ibag,
    );
    let res = match imported {
        Ok(r) => r,
        Err(e) => {
            // 产物**导不回来**是最严重的一类失真：保真红线直接立案。
            bag.push_p1(DiagCode::ROUNDTRIP_OVER_TOL);
            v.first_locator = format!("导出产物无法被导入器接受：{}", e.render());
            v.violations = v.violations.saturating_add(1);
            return v;
        }
    };

    v.ran = true;
    v.tracks_roundtrip = res.tracks.len() as u32;
    // 源侧「可导出轨道数」独立重算：排除离散轨（不在产物里）与空槽位形态键轨。
    // **不复用任何被测计数器**——否则「轨道数守恒」就成了自己比自己。
    v.tracks_src = count_exportable_sources(clip);

    let mut max_t: u32 = 0;
    let mut max_v: f32 = 0.0;
    let mut max_g: f32 = 0.0;
    let mut viol: u32 = 0;
    let mut first = String::new();
    let mut keys: u64 = 0;
    let mut compared: u32 = 0;

    // —— 建源轨道索引：`(node, semantic, morph_slot) → 源轨下标` ——
    // 离散轨不在产物里，比对时跳过（它们由 `DISCRETE_SKIPPED` 单独核）。
    let mut ti = 0usize;
    while ti < res.tracks.len() {
        let it = &res.tracks[ti];
        let src = match find_source(clip, it.semantic, it.target_node, it.morph_slot) {
            Some(s) => s,
            None => {
                // 产物里多出一条源里没有的轨 —— 守恒已破，直接立案。
                viol = viol.saturating_add(1);
                if first.is_empty() {
                    first = format!(
                        "轨道#{}（n{} {}）在源中无对应轨，往返轨道数不守恒",
                        ti,
                        it.target_node,
                        it.semantic.label()
                    );
                }
                ti += 1;
                continue;
            }
        };
        compared = compared.saturating_add(1);
        let sem = it.semantic;
        let lanes = sem.lanes();

        // —— 帧数对账（**恰等于**：钳制抽稀会减少帧，故用钳制后的期望帧数）——
        let expect_times = expected_export_times(src, sem);
        // 源侧的钳制保留索引（与 `expect_times` 同源，故帧号天然对齐）。
        let kept_src = clamp_sample_rate(&src.track.times, MIN_KEY_DT_MS).kept;
        let got_keys = it.track.times.len();
        keys += got_keys as u64;
        if expect_times.len() != got_keys {
            viol = viol.saturating_add(1);
            if first.is_empty() {
                first = format!(
                    "轨道#{}（n{} {}）：往返帧数 {} ≠ 期望 {}",
                    ti,
                    it.target_node,
                    sem.label(),
                    got_keys,
                    expect_times.len()
                );
            }
        }

        let mut k = 0usize;
        // 源侧帧索引必须**对齐到钳制后的保留集**。这是本函数最容易写错的
        // 一处：直接拿 `src.track.times[k]` 当第 k 帧的源值，而导出走的是
        // `clamp_sample_rate` 抽稀后的子集——两者**错位**，于是每一个被抽掉的
        // 帧都表现为「值差巨大」，往返断言恒红（实测：dense 语料 6→4 帧，
        // 帧 1 起值差 1~2，3 项超差）。
        // 症状极具欺骗性：看起来像「钳制把值改坏了」，实际是比对基准没对齐。
        while k < got_keys {
            // —— 时刻口径：源毫秒 → f32 秒 → 毫秒 ——
            let src_ms = if k < expect_times.len() { expect_times[k] } else { 0 };
            let rt_ms = it.track.times[k];
            let terr = src_ms.abs_diff(rt_ms);
            if terr > max_t {
                max_t = terr;
            }
            if terr > TOL_TIME_MS {
                viol = viol.saturating_add(1);
                if first.is_empty() {
                    first = format!(
                        "轨道#{} 帧 {}：时刻偏差 {} ms > 容差 {} ms",
                        ti, k, terr, TOL_TIME_MS
                    );
                }
            }

            // —— 值口径：与**源分量**比，不是与自身比 ——
            let mut rbuf: [f32; 4] = [0.0f32; 4];
            let got = it.track.key_slice(k, &mut rbuf);
            let mut sbuf: [f32; 4] = [0.0f32; 4];
            let mut have_src = false;
            if let Some(src_key) = kept_src.get(k) {
                let got_src = src.track.key_slice(*src_key, &mut sbuf);
                have_src = got_src >= lanes;
            }
            if got >= lanes && have_src {
                if sem == TrackSemantic::Rotation {
                    let g = quat_angular_gap(&sbuf, &rbuf);
                    if g > max_g {
                        max_g = g;
                    }
                    // 分量口径也查（抓归一化改写幅度）。
                    let cd = component_max_diff(&sbuf, &rbuf);
                    if cd > max_v {
                        max_v = cd;
                    }
                    if g > TOL_ROT_SLERP_RAD || cd > TOL_VALUE_QUAT {
                        viol = viol.saturating_add(1);
                        if first.is_empty() {
                            first = format!(
                                "轨道#{} 帧 {}：旋转 夹角差 {} / 分量差 {} 超容差 {} / {}",
                                ti, k, g, cd, TOL_ROT_SLERP_RAD, TOL_VALUE_QUAT
                            );
                        }
                    }
                } else {
                    let d = component_max_diff(&sbuf, &rbuf);
                    if d > max_v {
                        max_v = d;
                    }
                    if d > value_tolerance(sem) {
                        viol = viol.saturating_add(1);
                        if first.is_empty() {
                            first = format!(
                                "轨道#{} 帧 {}：分量差 {} > 容差 {}",
                                ti,
                                k,
                                d,
                                value_tolerance(sem)
                            );
                        }
                    }
                }
            }
            k += 1;
        }
        ti += 1;
    }

    v.keys_compared = keys;
    v.tracks_compared = compared;
    v.max_time_err_ms = max_t;
    v.max_value_diff = max_v;
    v.max_rot_gap = max_g;
    v.violations = viol;
    v.first_locator = first;
    if viol > 0 {
        bag.push_p1(DiagCode::ROUNDTRIP_OVER_TOL);
    }
    v
}

/// 独立重算源 clip 中**可导出**的轨道数（排除离散轨与空槽位形态键轨）。
///
/// 与 `export_gltf_anim` 的跳过条件逐条对齐——但**独立实现**，不复用那边的
/// 计数器。这样「往返轨道数守恒」才有意义：若两侧共用一个计数器，把
/// `tracks_src` 直接赋成 `tracks_roundtrip` 也能恒绿。
pub fn count_exportable_sources(clip: &ExportClip) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < clip.tracks.len() {
        let t = &clip.tracks[i];
        let skip = t.is_discrete()
            || t.track.times.is_empty()
            || (t.semantic == TrackSemantic::MorphWeight && t.morph_slot == u16::MAX);
        if !skip {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 在源 clip 中按 `(语义, 节点, 形态键槽)` 找轨道（**取第一条**）。
fn find_source<'a>(
    clip: &'a ExportClip,
    semantic: TrackSemantic,
    node: u32,
    morph_slot: u16,
) -> Option<&'a ExportTrack> {
    let mut i = 0usize;
    while i < clip.tracks.len() {
        let t = &clip.tracks[i];
        if t.semantic == semantic && t.target_node == node && t.morph_slot == morph_slot {
            return Some(t);
        }
        i += 1;
    }
    None
}

/// 源轨道经**本条导出路径**后应有的时刻序列（独立重算，不读产物）。
///
/// 独立重算是刻意的：若直接读产物里的时刻再与产物比，那是自证式。
/// 这里复算 `clamp_sample_rate` 得到保留集，从**源轨道**取时刻。
pub fn expected_export_times(src: &ExportTrack, _sem: TrackSemantic) -> Vec<u32> {
    let cl = clamp_sample_rate(&src.track.times, MIN_KEY_DT_MS);
    let mut out: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < cl.kept.len() {
        let k = cl.kept[i];
        if k < src.track.times.len() {
            out.push(src.track.times[k]);
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 十二、族声明与冒烟
// ---------------------------------------------------------------------------

/// 家族声明一致性（判据用）。
pub fn family_is_consistent() -> bool {
    DiagCode::ALL.len() == 19
        && ChannelPath::ALL.len() == 4
        && TrackSemantic::ALL.len() == 4
        && MIN_KEY_DT_MS == 1
        && TOL_VALUE_LINEAR < TOL_VALUE_QUAT
        && TIME_EXACT_MAX_MS <= u32::MAX
}

/// 码标签互异（防两码共用一句人话——那是最难查的一类缺陷）。
pub fn labels_unique() -> bool {
    let mut i = 0usize;
    while i < DiagCode::ALL.len() {
        let mut j = i + 1;
        while j < DiagCode::ALL.len() {
            if DiagCode::ALL[i].label() == DiagCode::ALL[j].label() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 四个类别的码域互不重合，且每类非空。
pub fn diag_class_codes_disjoint() -> bool {
    let mut i = 0usize;
    while i < DiagCode::ALL.len() {
        let c = DiagCode::ALL[i];
        let m = u32::from(c.is_skip_class())
            + u32::from(c.is_refuse_class())
            + u32::from(c.is_rewrite_class())
            + u32::from(c.is_reconcile_class());
        if m > 1 {
            return false;
        }
        i += 1;
    }
    let mut skip = 0u32;
    let mut refuse = 0u32;
    let mut rewrite = 0u32;
    let mut rec = 0u32;
    let mut k = 0usize;
    while k < DiagCode::ALL.len() {
        let c = DiagCode::ALL[k];
        if c.is_skip_class() {
            skip += 1;
        }
        if c.is_refuse_class() {
            refuse += 1;
        }
        if c.is_rewrite_class() {
            rewrite += 1;
        }
        if c.is_reconcile_class() {
            rec += 1;
        }
        k += 1;
    }
    skip > 0 && refuse > 0 && rewrite > 0 && rec > 0
}

/// 描述。
pub fn describe() -> String {
    let mut s = String::new();
    let _ = s.push_str("动画导出（M 域轨道 → glTF animation）：\n");
    let _ = s.push_str("· 四通道反向映射是 F2409 正表的机械求逆（映射表单源双用），漂移即置拦截、拦截态整体拒绝。\n");
    let _ = s.push_str("· 导出产物类型就是 GltfAnimDoc（导入器输入类型）⇒ glTF 双向是类型级事实，不靠人记得写转换器。\n");
    let _ = s.push_str("· 往返对拍走真导入器（import_gltf_anim），不是影子比较器：容差表两侧口径同源。\n");
    let _ = s.push_str("· 容差分时刻/值两路；时刻容差不是 0（ms→f32 秒→ms 受尾数限制），超界时刻如实声明。\n");
    let _ = s.push_str("· 旋转双口径：分量差抓归一化改写幅度，夹角差（1-|dot|）抓旋转走偏且对 q/-q 免疫。\n");
    let _ = s.push_str("· 精度诚实：精度声明逐项列出有损点，且反向断言不含「无损」字样。\n");
    let _ = s.push_str("· 离散轨（布尔/事件）跳过 + 指名声明，不静默丢轨。\n");
    let _ = s.push_str("· 导出侧 NaN 拒绝、导入侧 NaN 钳制——两侧处置故意相反，判据钉住这个不对称。\n");
    let _ = s.push_str("· 采样率超限抽稀钳制，端点钉死（末帧必保留），间隔对最终保留集重算。\n");
    let _ = s.push_str("· 同节点多形态键轨合并为单 weights 通道（comps = 键数），保证往返轨道数守恒。\n");
    s
}

/// 冒烟：导一份三通道小片段并跑一次往返。
///
/// **冒烟语料刻意用单位四元数**（0° / 90° / 180° 三档）：非单位四元数会被
/// 导出侧归一化改写，于是往返必然超差——那会让「冒烟可运行」这条判据变成
/// 「冒烟会报错」，把真正的编译/接线回归掩盖掉。脏数据路径由判据单独覆盖。
pub fn smoke() -> String {
    let tracks: Vec<ExportTrack> = vec![
        pos_track(0, u16::MAX, vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 0.0]),
        rot_track(0, u16::MAX, vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.707_106_8, 0.0, 0.707_106_8, 0.0, 1.0, 0.0, 0.0]),
        ExportTrack::new(
            TrackSemantic::Scale,
            1,
            u16::MAX,
            soa_scalar3("scl", vec![0.0, 1.0, 1.0, 1.0, 1.0, 2.0, 0.0, 1.0, 1.0]),
            TrackClass::Continuous,
            Interp::Linear,
        ),
    ];
    let clip = ExportClip::new(2, tracks);
    let mut table = ExportMapTable::from_forward(&MappingTable::standard());
    let mut bag = DiagBag::new();
    match export_gltf_anim(&clip, &mut table, &MappingTable::standard(), &mut bag) {
        Err(e) => e.render(),
        Ok(anim) => {
            let mut b2 = DiagBag::new();
            let rt = roundtrip_verify(&clip, &anim, &mut b2);
            format!("{}\n{}", anim.report.render(), rt.render())
        }
    }
}

/// 内部：造一条位置轨（3 分量）。
fn pos_track(node: u32, slot: u16, vals: Vec<f32>) -> ExportTrack {
    ExportTrack::new(
        TrackSemantic::Position,
        node,
        slot,
        soa_scalar3("pos", vals),
        TrackClass::Continuous,
        Interp::Linear,
    )
}

/// 内部：造一条旋转轨（4 分量，单位四元数）。
fn rot_track(node: u32, slot: u16, vals: Vec<f32>) -> ExportTrack {
    use super::vem07_perf::TrackValueKind;
    let frames = vals.len() / 4;
    let mut times: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < frames {
        times.push(i as u32 * 500);
        i += 1;
    }
    let t = SoaTrack::new("rot", TrackClass::Continuous, TrackValueKind::Quat, false, times, vals);
    ExportTrack::new(TrackSemantic::Rotation, node, slot, t, TrackClass::Continuous, Interp::Linear)
}

/// 内部：造一条 3 分量轨（位置/缩放共用）。
fn soa_scalar3(name: &str, vals: Vec<f32>) -> SoaTrack {
    use super::vem07_perf::TrackValueKind;
    let frames = vals.len() / 3;
    let mut times: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < frames {
        times.push(i as u32 * 500);
        i += 1;
    }
    SoaTrack::new(name, TrackClass::Continuous, TrackValueKind::Position, false, times, vals)
}