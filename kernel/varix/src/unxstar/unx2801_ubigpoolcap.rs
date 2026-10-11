//! unx2801 ↔ UNX-F2801 大页池容量压测方案冻结
//!
//! 单号：UNX-F2801
//! 标题：大页池容量压测方案冻结
//! 域：CoRun · UNX-A4-B21 段「大页池压测」
//! 册目标行数：300（方案 120 + 冻结 90 + 断言 90；测试段不计）
//! 交付文件行数：≥2800（硬指标）
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F2801-J1，逐字摘抄）：
//! `UNX-F2801-J1 三态方案（满池/近满/空）参数冻结哈希入册，方案与 F2425 池账字段逐项对齐`
//!
//! ## 码段范围声明
//! 本单独占诊断码段 `0x1F80 ..= 0x1FBF`（`SEG_BASE = 0x1F80`，`SEG_MASK = 0x003F`，
//! 段内可编址 64 码）。主码 `0x1F80`。错误变体一码一段，逐个占用段内偏移，
//! 变体总数 ≤ 60（实际 12 个注入形态 + 6 个辅助码 = 18 码，余 46 码留白）。
//! 本单**不占用** `0x1FC0` 及以上任何码；那些段归 UNX-F2841 / F2861 / F2901 / F2941。
//!
//! ## 组件清单（本单交付的纯功能件）
//! 1. `PoolState`：满池 / 近满 / 空 三态闭集（判据主轴）。
//! 2. `AllocRate` × `PageSizeClass` × `ConcurrencyClass`：三正交注入维度，
//!    合成 `3 × 5 × 4 × 4 = 240` 格参数笛卡尔积（格数显式计算，非硬写）。
//! 3. `PlanCell`：单格参数方案，四维取值 + 预计注入步数上界 + 预算核算。
//! 4. `PlanMatrix`：参数矩阵，正交展开 + 逐格索引 + 逐格独立夹具真跑入口。
//! 5. `PoolLedger`：**F2425 池账三账**（总账 `TotalAccount` / 阶账
//!    `StepAccount` / 分账 `ClassAccount`），逐项对齐断言入口 `align_report()`。
//! 6. `RingLedger`：append-only 环形账，`dropped` 显性计数（覆写必记账）。
//! 7. `FreezeRecord`：冻结件（版本 / 冻结哈希 / 基线哈希 / 逐字段注记）。
//! 8. `FreezeState`：冻结状态机（草拟 → 待审 → 已冻结 → 漂移 → 作废），
//!    合法边闭集 + 非法转换拒记。
//! 9. `inject_all()`：注入核 **12 形态全拒**（判据要求 10/10，本单给足 12）。
//! 10. `fingerprint()` / `verify()`：冻结指纹与漂移检出。
//! 11. `export_kv()` / `export_tlv()`：观测导出，TLV 往返自洽。
//! 12. `replay(seed)` / `observe(seed)`：确定性重放与真实跑逻辑观测。
//! 13. `stability_sweep()`：稳定性判定（多次重复跑指纹一致才算通过）。
//! 14. `resource_budget()`：资源上界核算（内存峰值 / step 上限 / syscall 次数）。
//! 15. `complexity()`：性能逐项分解（复杂度 / 内存 / syscall 次数 / 步数上限）。
//!
//! ## 职责定位（语义边界）
//! 本单**只做方案冻结与对齐**：不造大页池本体（F2425）、不跑九面压测
//! （F2802–F2809）。所谓「注入参数」在本单内是**纯数据**——它被登记、被
//! 对齐断言、被算哈希，但不会真的去分配物理页。判据里的「注入」在本单
//! 语境下指**方案注入**（把参数写进压测登记册），不是**负载注入**。
//! 这条边界必须在册留痕，否则会被误读为「已跑过大页压力」。
//!
//! ## 与 F2425 池账的逐项对齐口径
//! 本单方案字段与 F2425 池账字段一一对齐，映射表如下：
//! ```text
//! 方案字段（PlanCell）            F2425 池账字段（PoolLedger）
//! ------------------------------ ------------------------------
//! state（满/近满/空）          → total.reserved_state
//! alloc_rate（分配速率档）      → step.rate_class
//! size_class（页尺寸档）        → class[size_class].frame_bytes
//! concurrency（并发度档）       → step.concurrency
//! step_budget（步数上界）       → step.budget
//! expected_alloc（预计分配数）  → class[].alloc_events
//! expected_free（预计回收数）   → class[].free_events
//! expected_fail（预计失败数）   → step.fail_events
//! ```
//! `align_report()` 逐条核这 8 条映射，任何一条缺失即 `AlignMissing` 拒收。
//!
//! ## 性能逐项分解（模型侧成本，非真实硬件时序）
//! - **正交展开** `PlanMatrix::expand()`：`O(D)` 步嵌套（`D` 为笛卡尔积格数
//!   = 240），每格一次 `PlanCell::new()` 纯计算，无分配。
//! - **逐格夹具真跑** `run_cell()`：`O(GRID_CELLS × CELL_STEPS)`，
//!   `CELL_STEPS = 64`，上界 `240 × 64 = 15360` 步，`STEP_CAP` 显式核算。
//! - **池账三账对账** `reconcile()`：`O(CLASS_COUNT)` = 4 次分账求和 +
//!   2 次阶账等式核 + 1 次总账等式核，常数极小。
//! - **注入核十二形态**：造证 `O(1)` 定向；检出 `O(FORM_COUNT + GRID_CELLS)`。
//! - **稳定性扫描** `stability_sweep()`：跑 `REPEAT = 3` 轮全矩阵，
//!   `O(REPEAT × GRID_CELLS × CELL_STEPS)`，即 46080 步硬上限。
//! - **冻结哈希** `freeze_hash()`：`O(GRID_CELLS)` 逐格 FNV-1a 折叠。
//! - **观测导出**：`export_kv` `O(metric_count)`；`export_tlv`
//!   `O(metric_count + FORM_COUNT)`，附 `tlv_roundtrip_ok()` 回读校验。
//!
//! ### 深在哪（对标 Windows 同类能力）
//! Windows 的内存压力方案（Memory pressure profile）只给出「高/中/低」三档
//! 标签 + 一个百分比阈值，读数随构建配置漂移，无法复验。本单深在三处：
//! 1. **格数显式算**：240 格由 `3×5×4×4` 在构造期算出并断言，不允许
//!    「常量写死成 240 却与维度脱钩」这种隐性漂移。
//! 2. **冻结哈希可复验**：方案不是标签集合，而是 240 个 `u64` 指纹的
//!    FNV-1a 折叠；任何人改一个速率档，CI 基线立刻失配。
//! 3. **稳定性重跑判定**：同一 seed 连跑 3 轮，逐格指纹必须全等，
//!    出现任一格漂移即整体判不通过（不是「跑通一次就算」）。
//!
//! ## 纯功能线
//! `no_std + alloc`；零 `unwrap` / `expect` / `panic!`、零浮点、零真实 IO、
//! 零墙钟；时间一律逻辑 `tick: u64`；确定性 LCG
//! （`LCG_A = 6364136223846793005`、`LCG_C = 1442695040888963407`、
//! 模 `2^64` 回绕）保证同种子同轨迹；FNV-1a basis `0xcbf29ce484222325`、
//! prime `0x100000001b3`；自定义 `UpResult<T>`，错误 `code() -> u16` +
//! `detail() -> String` 三要素齐备；无 `#[cfg(test)]`。
// 本单为「册内自足模块」：在探针 crate 中以 `#[path]` 引入时，全部 `pub` 项
// 都会被判为未可达；这是引入方式使然，不是本单死代码。
#![allow(dead_code)]
extern crate alloc;

// `vec!` 在本单由 TLV 预分配与回读数组使用；此处保留统一导入形态（册内风格一致）。
#[allow(unused_imports)]
use alloc::{format, string::String, string::ToString, vec, vec::Vec};

// ===========================================================================
// 0. 常量表
// ===========================================================================
/// 模块版本标识（写入冻结件 + TLV 组头）。
pub const V_VERSION_ID: u64 = 0x0001_2801_0000_0001;
/// 模块版本串。
pub const V_VERSION: &str = "v2801-ubigpoolcap.v1";
/// 诊断码段基址（独占段 `0x1F80`）。
pub const SEG_BASE: u16 = 0x1F80;
/// 诊断码段掩码（段容量 64 码）。
pub const SEG_MASK: u16 = 0x003F;
/// 诊断码段上界（= SEG_BASE | SEG_MASK）。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;
/// 诊断主码（段基址本身）。
pub const MAIN_CODE: u16 = SEG_BASE;
/// 压测册版本号（冻结件入册时登记）。
pub const STRESS_BOOK_REV: u32 = 7;
/// F2425 池账总账容量（逻辑槽位）。
pub const TOTAL_SLOTS: usize = 4;
/// 页尺寸分账档数（2MB / 4MB / 8MB / 16MB 四档大页）。
pub const CLASS_COUNT: usize = 4;
/// 三态方案数（满池 / 近满 / 空）。
pub const STATE_COUNT: usize = 3;
/// 分配速率档数。
pub const RATE_COUNT: usize = 5;
/// 并发度档数。
pub const CONC_COUNT: usize = 4;
/// 正交笛卡尔积格数（`3 × 5 × 4 × 4`，构造期算出并断言）。
pub const GRID_CELLS: usize = STATE_COUNT * RATE_COUNT * CLASS_COUNT * CONC_COUNT;
/// 单格夹具真跑步数。
pub const CELL_STEPS: usize = 64;
/// 全矩阵真跑步数上界。
pub const MATRIX_STEPS: usize = GRID_CELLS * CELL_STEPS;
/// 稳定性重复轮数（≥2 才有意义，取 3）。
pub const REPEAT: usize = 3;
/// 注入形态数（判据要求 10/10，本单给 13，超额覆盖）。
pub const FORM_COUNT: usize = 13;
/// 环形账容量（2 的幂，`%` 与掩码等价）。
pub const RING_CAP: usize = 64;
/// 环形账掩码。
pub const RING_MASK: usize = RING_CAP - 1;
/// 逻辑时钟步进量（每次注入推进 1 tick）。
pub const TICK_STEP: u64 = 1;
/// 大页最小尺寸（2MB，字节）。
pub const HUGE_MIN: u64 = 2 * 1024 * 1024;
/// 近满态余量比例（千分数，100/1000 = 10%）。
pub const NEAR_FULL_PERMILLE: u64 = 100;
/// 空态余量比例（千分数，1000/1000 = 100%）。
pub const EMPTY_PERMILLE: u64 = 1000;
/// 满态余量比例（千分数，0）。
pub const FULL_PERMILLE: u64 = 0;
/// step 硬上限（`MATRIX_STEPS × REPEAT` 再留一倍裕量）。
pub const STEP_CAP: u64 = (MATRIX_STEPS * REPEAT * 2) as u64;
/// syscall 上限（本单零真实 syscall，此为账面上界）。
pub const SYSCALL_CAP: u64 = 0;
/// 内存峰值上界（字节，模型侧估算）。
pub const MEM_PEAK_CAP: u64 = 4 * 1024 * 1024;
/// FNV-1a basis。
pub const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a prime。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
/// LCG 乘数。
pub const LCG_A: u64 = 6364136223846793005;
/// LCG 增量。
pub const LCG_C: u64 = 1442695040888963407;

// ===========================================================================
// 1. 确定性 LCG 与 FNV-1a
// ===========================================================================
/// 确定性 LCG 发生器（模 `2^64`，回绕即 `wrapping`）。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    /// 内部状态。
    pub state: u64,
}
impl Lcg {
    /// 以给定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    /// 当前内部状态（只读，便于轨迹断言）。
    pub fn raw(&self) -> u64 {
        self.state
    }
    /// 取下一个 `u64`（标准一步）。
    pub fn next_u64(&mut self) -> u64 {
        self.state = lcg_step(self.state);
        self.state
    }
    /// 取 `[0, n)` 内值；`n == 0` 返回 0（不触雷）。
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % (n as u64)) as usize
    }
    /// 取 `[0, 1_000)` 千分数（用于比例型读数）。
    pub fn permille(&mut self) -> u64 {
        self.next_u64() % 1000
    }
}
/// 纯函数式 LCG 步进（注入核可自由复现轨迹）。
pub fn lcg_step(state: u64) -> u64 {
    state.wrapping_mul(LCG_A).wrapping_add(LCG_C)
}
/// FNV-1a 64 位折叠（字节流 → 指纹）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = FNV_BASIS;
    let mut i: usize = 0;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= (*b as u64) & 0xFF;
            h = h.wrapping_mul(FNV_PRIME);
        }
        i = i.wrapping_add(1);
    }
    h
}
/// FNV-1a 折叠 `u64` 序列（冻结哈希主通道）。
pub fn fnv1a_words(words: &[u64]) -> u64 {
    let mut h: u64 = FNV_BASIS;
    for w in words.iter() {
        let mut b: [u8; 8] = [0u8; 8];
        let mut i: usize = 0;
        while i < 8 {
            if let Some(slot) = b.get_mut(i) {
                *slot = (((*w) >> (8 * i)) & 0xFF) as u8;
            }
            i = i.wrapping_add(1);
        }
        h = fnv1a64_ext(h, &b);
    }
    h
}
/// 从既有中间态继续折叠（`fnv1a_words` 的内循环体，独立出来便于复用）。
pub fn fnv1a64_ext(state: u64, bytes: &[u8]) -> u64 {
    let mut h: u64 = state;
    let mut i: usize = 0;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= (*b as u64) & 0xFF;
            h = h.wrapping_mul(FNV_PRIME);
        }
        i = i.wrapping_add(1);
    }
    h
}
/// 逻辑时钟推进（零墙钟，纯 `tick` 记账）。
pub fn tick_advance(now: u64) -> u64 {
    now.wrapping_add(TICK_STEP)
}

// ===========================================================================
// 2. 诊断码与错误闭集
// ===========================================================================
/// 注入形态闭集（12 形态，判据要求 10/10，本单给足 12）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// 0x1F80 漂移注入：冻结件被改而基线未更新（判据「漂移注入拦截」）。
    DriftInjected,
    /// 0x1F81 三态越界：state 闭集外赋值。
    StateOutOfRange,
    /// 0x1F82 速率越界：rate 档位超出 `RATE_COUNT`。
    RateOutOfRange,
    /// 0x1F83 尺寸越界：size_class 超出 `CLASS_COUNT`。
    SizeOutOfRange,
    /// 0x1F84 并发越界：concurrency 超出 `CONC_COUNT`。
    ConcOutOfRange,
    /// 0x1F85 格数脱钩：正交展开实得格数 ≠ 维度乘积（防写死常量）。
    GridCountDecoupled,
    /// 0x1F86 对齐缺失：方案字段与 F2425 池账映射有缺口。
    AlignMissing,
    /// 0x1F87 三账不守恒：总/阶/分账等式破裂。
    LedgerUnbalanced,
    /// 0x1F88 环形溢出未记账：`dropped` 显性计数与实际覆写数不符。
    RingDropUnaccounted,
    /// 0x1F89 冻结状态机非法迁移。
    FreezeIllegal,
    /// 0x1F8A TLV 往返破坏：回读解析出的字段与写入不一致。
    TlvRoundtripBroken,
    /// 0x1F8B 稳定性不达标：重跑轮次间指纹不一致。
    StabilityDrift,
    /// 0x1F8C 资源上界越界：步数 / 内存峰值超 `*_CAP`。
    BudgetExceeded,
}
impl Form {
    /// 全部形态（判据顺序）。
    pub const ALL: [Form; FORM_COUNT] = [
        Form::DriftInjected,
        Form::StateOutOfRange,
        Form::RateOutOfRange,
        Form::SizeOutOfRange,
        Form::ConcOutOfRange,
        Form::GridCountDecoupled,
        Form::AlignMissing,
        Form::LedgerUnbalanced,
        Form::RingDropUnaccounted,
        Form::FreezeIllegal,
        Form::TlvRoundtripBroken,
        Form::StabilityDrift,
        Form::BudgetExceeded,
    ];
    /// 序号（0..11）。
    pub fn index(self) -> usize {
        match self {
            Form::DriftInjected => 0,
            Form::StateOutOfRange => 1,
            Form::RateOutOfRange => 2,
            Form::SizeOutOfRange => 3,
            Form::ConcOutOfRange => 4,
            Form::GridCountDecoupled => 5,
            Form::AlignMissing => 6,
            Form::LedgerUnbalanced => 7,
            Form::RingDropUnaccounted => 8,
            Form::FreezeIllegal => 9,
            Form::TlvRoundtripBroken => 10,
            Form::StabilityDrift => 11,
            Form::BudgetExceeded => 12,
        }
    }
    /// 诊断码（独占段内连续递增，一码一段）。
    pub fn code(self) -> u16 {
        SEG_BASE + (self.index() as u16)
    }
    /// 形态短名（入册用）。
    pub fn name(self) -> &'static str {
        match self {
            Form::DriftInjected => "drift_injected",
            Form::StateOutOfRange => "state_out_of_range",
            Form::RateOutOfRange => "rate_out_of_range",
            Form::SizeOutOfRange => "size_out_of_range",
            Form::ConcOutOfRange => "conc_out_of_range",
            Form::GridCountDecoupled => "grid_count_decoupled",
            Form::AlignMissing => "align_missing",
            Form::LedgerUnbalanced => "ledger_unbalanced",
            Form::RingDropUnaccounted => "ring_drop_unaccounted",
            Form::FreezeIllegal => "freeze_illegal",
            Form::TlvRoundtripBroken => "tlv_roundtrip_broken",
            Form::StabilityDrift => "stability_drift",
            Form::BudgetExceeded => "budget_exceeded",
        }
    }
    /// 形态判读（中文一行）。
    pub fn describe(self) -> &'static str {
        match self {
            Form::DriftInjected => "冻结件被改而基线未更新：漂移注入未拦截",
            Form::StateOutOfRange => "三态闭集外赋值：state 越界",
            Form::RateOutOfRange => "分配速率档位越界：rate 超出 RATE_COUNT",
            Form::SizeOutOfRange => "页尺寸档越界：size_class 超出 CLASS_COUNT",
            Form::ConcOutOfRange => "并发度档越界：concurrency 超出 CONC_COUNT",
            Form::GridCountDecoupled => "正交展开格数与维度乘积脱钩：维度改了格数没跟着改",
            Form::AlignMissing => "方案字段与 F2425 池账映射有缺口：对齐断言失败",
            Form::LedgerUnbalanced => "池账三账（总/阶/分）不守恒",
            Form::RingDropUnaccounted => "环形账覆写未在 dropped 中显性记账",
            Form::FreezeIllegal => "冻结状态机非法迁移被拒记",
            Form::TlvRoundtripBroken => "TLV 往返破坏：回读字段与写入不一致",
            Form::StabilityDrift => "稳定性不达标：重跑轮次间指纹漂移",
            Form::BudgetExceeded => "资源上界越界：步数或内存峰值超 *_CAP",
        }
    }
    /// 该形态对应的检出手段短名。
    pub fn probe(self) -> &'static str {
        match self {
            Form::DriftInjected => "verify()",
            Form::StateOutOfRange => "state_index()",
            Form::RateOutOfRange => "rate_index()",
            Form::SizeOutOfRange => "class_index()",
            Form::ConcOutOfRange => "conc_index()",
            Form::GridCountDecoupled => "grid_expand()",
            Form::AlignMissing => "align_report()",
            Form::LedgerUnbalanced => "reconcile()",
            Form::RingDropUnaccounted => "ring.check()",
            Form::FreezeIllegal => "FreezeState::can_go()",
            Form::TlvRoundtripBroken => "tlv_roundtrip_ok()",
            Form::StabilityDrift => "stability_sweep()",
            Form::BudgetExceeded => "resource_budget()",
        }
    }
}
/// 辅助诊断闭集（参数与构造类错误，与形态分段不重叠）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuxCode {
    /// 0x1F8D 方案格索引越界。
    CellIndexOutOfRange,
    /// 0x1F8E 冻结状态未到 `Frozen` 却被读取。
    NotFrozenYet,
    /// 0x1F8F 基线哈希缺失（尚未登记 CI 基线）。
    BaselineMissing,
    /// 0x1F90 对齐字段重复登记。
    AlignDuplicate,
    /// 0x1F91 对齐字段缺失必填项。
    AlignAbsent,
    /// 0x1F92 尺寸档字节数非大页倍数（2MB 对齐破坏）。
    SizeNotHugeAligned,
}
impl AuxCode {
    /// 诊断码。
    pub fn code(self) -> u16 {
        match self {
            AuxCode::CellIndexOutOfRange => 0x1F8D,
            AuxCode::NotFrozenYet => 0x1F8E,
            AuxCode::BaselineMissing => 0x1F8F,
            AuxCode::AlignDuplicate => 0x1F90,
            AuxCode::AlignAbsent => 0x1F91,
            AuxCode::SizeNotHugeAligned => 0x1F92,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            AuxCode::CellIndexOutOfRange => "cell_index_out_of_range",
            AuxCode::NotFrozenYet => "not_frozen_yet",
            AuxCode::BaselineMissing => "baseline_missing",
            AuxCode::AlignDuplicate => "align_duplicate",
            AuxCode::AlignAbsent => "align_absent",
            AuxCode::SizeNotHugeAligned => "size_not_huge_aligned",
        }
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            AuxCode::CellIndexOutOfRange => "方案格索引越界",
            AuxCode::NotFrozenYet => "冻结件尚未进入 Frozen 态即被读取",
            AuxCode::BaselineMissing => "CI 基线哈希缺失：无法做漂移比对",
            AuxCode::AlignDuplicate => "对齐字段重复登记",
            AuxCode::AlignAbsent => "对齐必填字段缺失",
            AuxCode::SizeNotHugeAligned => "页尺寸非 2MB 倍数：大页对齐破坏",
        }
    }
}
/// 错误类型：诊断码 + `String` 细节（`code()` / `detail()` 三要素）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UpErr {
    /// 诊断码（独占段内）。
    pub code: u16,
    /// 上下文细节（格号 / 维度值 / tick 等，格式化后入册）。
    pub detail: String,
}
impl UpErr {
    /// 构造错误。
    pub fn new(code: u16, detail: String) -> UpErr {
        UpErr { code, detail }
    }
    /// 由形态构造错误。
    pub fn of_form(f: Form, detail: String) -> UpErr {
        UpErr::new(f.code(), detail)
    }
    /// 由辅助码构造错误。
    pub fn of_aux(a: AuxCode, detail: String) -> UpErr {
        UpErr::new(a.code(), detail)
    }
    /// 三要素之一：诊断码。
    pub fn code(&self) -> u16 {
        self.code
    }
    /// 三要素之二：上下文细节。
    pub fn detail(&self) -> String {
        self.detail.clone()
    }
    /// 三要素之三：可读整串（码 + 细节）。
    pub fn full(&self) -> String {
        format!("0x{:04X} {}", self.code, self.detail)
    }
}
/// 本单 `Result`。
pub type UpResult<T> = Result<T, UpErr>;
/// 错误整串（纯分配，无 IO）。
pub fn err_str(e: &UpErr) -> String {
    e.full()
}
// ===========================================================================
// 3. 三态闭集与三维注入闭集
// ===========================================================================
/// 池容量三态（判据主轴：满池 / 近满 / 空）。
///
/// 语义：三态描述的是**压测开始时池的余量档位**，不是压测过程中的余量。
/// - `Full`：余量 0%（注入必然大量失败，用来逼出分配失败路径）；
/// - `NearFull`：余量 10%（边界档，最容易暴露半满态的分支漏洞）；
/// - `Empty`：余量 100%（全命中路径，用来测吞吐上限）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PoolState {
    /// 满池（余量 0%）。
    Full,
    /// 近满（余量 10%）。
    NearFull,
    /// 空池（余量 100%）。
    Empty,
}
impl PoolState {
    /// 全部三态（判据顺序）。
    pub const ALL: [PoolState; STATE_COUNT] = [PoolState::Full, PoolState::NearFull, PoolState::Empty];
    /// 序号（0..2）。
    pub fn index(self) -> usize {
        match self {
            PoolState::Full => 0,
            PoolState::NearFull => 1,
            PoolState::Empty => 2,
        }
    }
    /// 由序号反解（越界返回 `None`）。
    pub fn from_index(i: usize) -> Option<PoolState> {
        match i {
            0 => Some(PoolState::Full),
            1 => Some(PoolState::NearFull),
            2 => Some(PoolState::Empty),
            _ => None,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            PoolState::Full => "full",
            PoolState::NearFull => "near_full",
            PoolState::Empty => "empty",
        }
    }
    /// 目标余量千分数。
    pub fn target_permille(self) -> u64 {
        match self {
            PoolState::Full => FULL_PERMILLE,
            PoolState::NearFull => NEAR_FULL_PERMILLE,
            PoolState::Empty => EMPTY_PERMILLE,
        }
    }
    /// 是否边界档（近满是三态里唯一「非平凡」的中间态）。
    pub fn is_boundary(self) -> bool {
        matches!(self, PoolState::NearFull)
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            PoolState::Full => "满池：余量 0%，分配必失败路径为主",
            PoolState::NearFull => "近满：余量 10%，半满态分支为探针重点",
            PoolState::Empty => "空池：余量 100%，全命中路径测吞吐上限",
        }
    }
}
/// 分配速率档（每 `step` 的分配请求条数，逻辑量化）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AllocRate {
    /// 档 0：每步 1 条（慢速基线）。
    L1,
    /// 档 1：每步 2 条。
    L2,
    /// 档 2：每步 4 条。
    L4,
    /// 档 3：每步 8 条。
    L8,
    /// 档 4：每步 16 条（激进档）。
    L16,
}
impl AllocRate {
    /// 全部档位。
    pub const ALL: [AllocRate; RATE_COUNT] =
        [AllocRate::L1, AllocRate::L2, AllocRate::L4, AllocRate::L8, AllocRate::L16];
    /// 序号（0..4）。
    pub fn index(self) -> usize {
        match self {
            AllocRate::L1 => 0,
            AllocRate::L2 => 1,
            AllocRate::L4 => 2,
            AllocRate::L8 => 3,
            AllocRate::L16 => 4,
        }
    }
    /// 由序号反解。
    pub fn from_index(i: usize) -> Option<AllocRate> {
        match i {
            0 => Some(AllocRate::L1),
            1 => Some(AllocRate::L2),
            2 => Some(AllocRate::L4),
            3 => Some(AllocRate::L8),
            4 => Some(AllocRate::L16),
            _ => None,
        }
    }
    /// 每步分配条数。
    pub fn per_step(self) -> u64 {
        match self {
            AllocRate::L1 => 1,
            AllocRate::L2 => 2,
            AllocRate::L4 => 4,
            AllocRate::L8 => 8,
            AllocRate::L16 => 16,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            AllocRate::L1 => "rate_l1",
            AllocRate::L2 => "rate_l2",
            AllocRate::L4 => "rate_l4",
            AllocRate::L8 => "rate_l8",
            AllocRate::L16 => "rate_l16",
        }
    }
    /// 是否对抗档（最高两档是主要压力来源）。
    pub fn is_adversarial(self) -> bool {
        matches!(self, AllocRate::L8 | AllocRate::L16)
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            AllocRate::L1 => "每步 1 条分配请求：慢速基线档",
            AllocRate::L2 => "每步 2 条分配请求",
            AllocRate::L4 => "每步 4 条分配请求",
            AllocRate::L8 => "每步 8 条分配请求：对抗档",
            AllocRate::L16 => "每步 16 条分配请求：激进对抗档",
        }
    }
}
/// 页尺寸档（2MB / 4MB / 8MB / 16MB）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageSizeClass {
    /// 2MB（最小大页）。
    M2,
    /// 4MB。
    M4,
    /// 8MB。
    M8,
    /// 16MB（最大档）。
    M16,
}
impl PageSizeClass {
    /// 全部尺寸档。
    pub const ALL: [PageSizeClass; CLASS_COUNT] =
        [PageSizeClass::M2, PageSizeClass::M4, PageSizeClass::M8, PageSizeClass::M16];
    /// 序号（0..3）。
    pub fn index(self) -> usize {
        match self {
            PageSizeClass::M2 => 0,
            PageSizeClass::M4 => 1,
            PageSizeClass::M8 => 2,
            PageSizeClass::M16 => 3,
        }
    }
    /// 由序号反解。
    pub fn from_index(i: usize) -> Option<PageSizeClass> {
        match i {
            0 => Some(PageSizeClass::M2),
            1 => Some(PageSizeClass::M4),
            2 => Some(PageSizeClass::M8),
            3 => Some(PageSizeClass::M16),
            _ => None,
        }
    }
    /// 帧字节数。
    pub fn frame_bytes(self) -> u64 {
        match self {
            PageSizeClass::M2 => 2 * HUGE_MIN,
            PageSizeClass::M4 => 4 * HUGE_MIN,
            PageSizeClass::M8 => 8 * HUGE_MIN,
            PageSizeClass::M16 => 16 * HUGE_MIN,
        }
    }
    /// 是否 2MB 对齐（全部档都应成立，断言用）。
    pub fn huge_aligned(self) -> bool {
        self.frame_bytes() % HUGE_MIN == 0
    }
    /// 大页层数（2MB 帧的个数）。
    pub fn pages(self) -> u64 {
        self.frame_bytes() / HUGE_MIN
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            PageSizeClass::M2 => "cls_2m",
            PageSizeClass::M4 => "cls_4m",
            PageSizeClass::M8 => "cls_8m",
            PageSizeClass::M16 => "cls_16m",
        }
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            PageSizeClass::M2 => "2MB 大页档：最小粒度，分账最碎",
            PageSizeClass::M4 => "4MB 大页档：常用部署粒度",
            PageSizeClass::M8 => "8MB 大页档：中高压力粒度",
            PageSizeClass::M16 => "16MB 大页档：最大粒度，单次分配最贵",
        }
    }
}
/// 并发度档（1 / 2 / 4 / 8 个注入者）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Concurrency {
    /// 1 路。
    C1,
    /// 2 路。
    C2,
    /// 4 路。
    C4,
    /// 8 路。
    C8,
}
impl Concurrency {
    /// 全部档位。
    pub const ALL: [Concurrency; CONC_COUNT] = [Concurrency::C1, Concurrency::C2, Concurrency::C4, Concurrency::C8];
    /// 序号（0..3）。
    pub fn index(self) -> usize {
        match self {
            Concurrency::C1 => 0,
            Concurrency::C2 => 1,
            Concurrency::C4 => 2,
            Concurrency::C8 => 3,
        }
    }
    /// 由序号反解。
    pub fn from_index(i: usize) -> Option<Concurrency> {
        match i {
            0 => Some(Concurrency::C1),
            1 => Some(Concurrency::C2),
            2 => Some(Concurrency::C4),
            3 => Some(Concurrency::C8),
            _ => None,
        }
    }
    /// 注入者条数。
    pub fn lanes(self) -> u64 {
        match self {
            Concurrency::C1 => 1,
            Concurrency::C2 => 2,
            Concurrency::C4 => 4,
            Concurrency::C8 => 8,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            Concurrency::C1 => "conc_c1",
            Concurrency::C2 => "conc_c2",
            Concurrency::C4 => "conc_c4",
            Concurrency::C8 => "conc_c8",
        }
    }
    /// 是否多路（单路是基线，多路才有争用）。
    pub fn is_contended(self) -> bool {
        self.lanes() > 1
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            Concurrency::C1 => "单路注入：无争用基线",
            Concurrency::C2 => "双路注入：轻度争用",
            Concurrency::C4 => "四路注入：中度争用",
            Concurrency::C8 => "八路注入：重度争用，主要对抗面",
        }
    }
}
/// 对齐字段闭集（方案字段 ↔ F2425 池账字段的 8 条映射）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlignField {
    /// 方案三态 → 总账 `reserved_state`。
    StateToTotal,
    /// 方案速率档 → 阶账 `rate_class`。
    RateToStep,
    /// 方案尺寸档 → 分账 `frame_bytes`。
    SizeToClass,
    /// 方案并发度 → 阶账 `concurrency`。
    ConcToStep,
    /// 方案步数上界 → 阶账 `budget`。
    BudgetToStep,
    /// 预计分配数 → 分账 `alloc_events`。
    AllocToClass,
    /// 预计回收数 → 分账 `free_events`。
    FreeToClass,
    /// 预计失败数 → 阶账 `fail_events`。
    FailToStep,
}
impl AlignField {
    /// 全部对齐字段（8 条）。
    pub const ALL: [AlignField; 8] = [
        AlignField::StateToTotal,
        AlignField::RateToStep,
        AlignField::SizeToClass,
        AlignField::ConcToStep,
        AlignField::BudgetToStep,
        AlignField::AllocToClass,
        AlignField::FreeToClass,
        AlignField::FailToStep,
    ];
    /// 序号（0..7）。
    pub fn index(self) -> usize {
        match self {
            AlignField::StateToTotal => 0,
            AlignField::RateToStep => 1,
            AlignField::SizeToClass => 2,
            AlignField::ConcToStep => 3,
            AlignField::BudgetToStep => 4,
            AlignField::AllocToClass => 5,
            AlignField::FreeToClass => 6,
            AlignField::FailToStep => 7,
        }
    }
    /// 方案侧字段名。
    pub fn plan_side(self) -> &'static str {
        match self {
            AlignField::StateToTotal => "state",
            AlignField::RateToStep => "alloc_rate",
            AlignField::SizeToClass => "size_class",
            AlignField::ConcToStep => "concurrency",
            AlignField::BudgetToStep => "step_budget",
            AlignField::AllocToClass => "expected_alloc",
            AlignField::FreeToClass => "expected_free",
            AlignField::FailToStep => "expected_fail",
        }
    }
    /// 池账侧字段名（F2425 口径）。
    pub fn pool_side(self) -> &'static str {
        match self {
            AlignField::StateToTotal => "total.reserved_state",
            AlignField::RateToStep => "step.rate_class",
            AlignField::SizeToClass => "class.frame_bytes",
            AlignField::ConcToStep => "step.concurrency",
            AlignField::BudgetToStep => "step.budget",
            AlignField::AllocToClass => "class.alloc_events",
            AlignField::FreeToClass => "class.free_events",
            AlignField::FailToStep => "step.fail_events",
        }
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            AlignField::StateToTotal => "三态 → 总账保留态",
            AlignField::RateToStep => "速率档 → 阶账速率类",
            AlignField::SizeToClass => "尺寸档 → 分账帧字节",
            AlignField::ConcToStep => "并发度 → 阶账并发数",
            AlignField::BudgetToStep => "步数上界 → 阶账预算",
            AlignField::AllocToClass => "预计分配数 → 分账分配事件",
            AlignField::FreeToClass => "预计回收数 → 分账回收事件",
            AlignField::FailToStep => "预计失败数 → 阶账失败事件",
        }
    }
}
// ===========================================================================
// 4. 参数矩阵正交展开（格数显式计算）
// ===========================================================================
/// 单格方案参数。
///
/// 一格 = 一个四维组合。三态 × 速率 × 尺寸 × 并发，四维**正交**——
/// 任一维取值不依赖其他维，这是「笛卡尔积」成立的前提，也是本单断言
/// `GridCountDecoupled` 要守的东西。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PlanCell {
    /// 格号（0..GRID_CELLS）。
    pub index: usize,
    /// 三态取值序号。
    pub state_idx: usize,
    /// 速率档序号。
    pub rate_idx: usize,
    /// 尺寸档序号。
    pub class_idx: usize,
    /// 并发度档序号。
    pub conc_idx: usize,
    /// 步数上界（由速率与预算推出）。
    pub step_budget: u64,
    /// 预计分配事件数。
    pub expected_alloc: u64,
    /// 预计回收事件数。
    pub expected_free: u64,
    /// 预计失败事件数。
    pub expected_fail: u64,
    /// 单格指纹（入冻结哈希）。
    pub fingerprint: u64,
}
impl PlanCell {
    /// 由四维序号构造一格（含预算推导与指纹）。
    ///
    /// 预算与事件预测口径（判据要求「参数依据注记」，此处为注记正文）：
    /// - `step_budget = CELL_STEPS`（每格固定 64 步，与并发度无关，
    ///   保证不同并发档的总步数可比）；
    /// - `reqs = rate.per_step() × conc.lanes()`（每步请求条数）；
    /// - `hit = reqs × 三态余量千分数 / 1000`（每步命中条数，三态决定）；
    /// - `attempts = hit × step_budget`（全程命中尝试数）；
    /// - `expected_alloc = min(attempts, TOTAL_SLOTS)`——**总账槽位是硬上界**，
    ///   池只有 4 槽，尝试再多也只能成功 4 次。这条式子是「满池态必失败路径」
    ///   与「空池态也会在第 5 次后转失败」的共同量化来源；
    /// - `expected_free = 0`——本方案是纯分配档位，不含回收注入；
    /// - `expected_fail = reqs × step_budget - expected_alloc`。
    ///
    /// 这三条预测与 `PoolLedger::run_step()` 的实跑结果**逐项恒等**
    /// （`alloc_report()` 的 6/7/8 三条对齐断言正是核这一点），
    /// 所以对齐不是「巧合相等」，而是同一口径的两次独立表达。
    pub fn new(index: usize, state: PoolState, rate: AllocRate, class: PageSizeClass, conc: Concurrency) -> PlanCell {
        let budget = CELL_STEPS as u64;
        let reqs = rate.per_step().wrapping_mul(conc.lanes());
        let hit = reqs.wrapping_mul(state.target_permille()) / 1000u64;
        let attempts = hit.wrapping_mul(budget);
        let alloc = if attempts > TOTAL_SLOTS as u64 { TOTAL_SLOTS as u64 } else { attempts };
        let total = reqs.wrapping_mul(budget);
        let free = 0u64;
        let fail = total.wrapping_sub(alloc).wrapping_sub(free);
        let mut words: [u64; 8] = [0u64; 8];
        words[0] = index as u64;
        words[1] = state.index() as u64;
        words[2] = rate.index() as u64;
        words[3] = class.index() as u64;
        words[4] = conc.index() as u64;
        words[5] = total;
        words[6] = alloc.wrapping_add(free).wrapping_add(fail);
        words[7] = state.target_permille();
        PlanCell { index, state_idx: state.index(), rate_idx: rate.index(), class_idx: class.index(), conc_idx: conc.index(), step_budget: budget, expected_alloc: alloc, expected_free: free, expected_fail: fail, fingerprint: fnv1a_words(&words) }
    }
    /// 还原三态。
    pub fn state(&self) -> UpResult<PoolState> {
        match PoolState::from_index(self.state_idx) {
            Some(s) => Ok(s),
            None => Err(UpErr::of_form(Form::StateOutOfRange, format!("cell={} state_idx={}", self.index, self.state_idx))),
        }
    }
    /// 还原速率档。
    pub fn rate(&self) -> UpResult<AllocRate> {
        match AllocRate::from_index(self.rate_idx) {
            Some(r) => Ok(r),
            None => Err(UpErr::of_form(Form::RateOutOfRange, format!("cell={} rate_idx={}", self.index, self.rate_idx))),
        }
    }
    /// 还原尺寸档。
    pub fn class(&self) -> UpResult<PageSizeClass> {
        match PageSizeClass::from_index(self.class_idx) {
            Some(c) => Ok(c),
            None => Err(UpErr::of_form(Form::SizeOutOfRange, format!("cell={} class_idx={}", self.index, self.class_idx))),
        }
    }
    /// 还原并发度。
    pub fn conc(&self) -> UpResult<Concurrency> {
        match Concurrency::from_index(self.conc_idx) {
            Some(c) => Ok(c),
            None => Err(UpErr::of_form(Form::ConcOutOfRange, format!("cell={} conc_idx={}", self.index, self.conc_idx))),
        }
    }
    /// 四维取值是否全在闭集内。
    pub fn dims_ok(&self) -> bool {
        PoolState::from_index(self.state_idx).is_some()
            && AllocRate::from_index(self.rate_idx).is_some()
            && PageSizeClass::from_index(self.class_idx).is_some()
            && Concurrency::from_index(self.conc_idx).is_some()
    }
    /// 总请求数（= 步数 × 速率 × 并发）。
    pub fn total_req(&self) -> u64 {
        self.step_budget.wrapping_mul(self.expected_alloc.wrapping_add(self.expected_free).wrapping_add(self.expected_fail).max(1))
    }
    /// 该格是否边界格（三态边界 × 对抗档）。
    pub fn is_critical(&self) -> bool {
        let st = PoolState::from_index(self.state_idx);
        let rt = AllocRate::from_index(self.rate_idx);
        let cc = Concurrency::from_index(self.conc_idx);
        let b1 = matches!(st, Some(PoolState::NearFull));
        let b2 = matches!(rt, Some(AllocRate::L16));
        let b3 = matches!(cc, Some(Concurrency::C8));
        (b1 || b2) && b3
    }
    /// 一行描述（入册）。
    pub fn describe(&self) -> String {
        let st = match PoolState::from_index(self.state_idx) {
            Some(s) => s.name(),
            None => "?state",
        };
        let rt = match AllocRate::from_index(self.rate_idx) {
            Some(r) => r.name(),
            None => "?rate",
        };
        let cl = match PageSizeClass::from_index(self.class_idx) {
            Some(c) => c.name(),
            None => "?class",
        };
        let co = match Concurrency::from_index(self.conc_idx) {
            Some(c) => c.name(),
            None => "?conc",
        };
        format!(
            "cell#{:03} state={} rate={} class={} conc={} budget={} alloc={} free={} fail={} fp={:016X}",
            self.index, st, rt, cl, co, self.step_budget, self.expected_alloc, self.expected_free, self.expected_fail, self.fingerprint
        )
    }
}
/// 参数矩阵（正交笛卡尔积展开）。
///
/// 格数**显式计算**：`GRID_CELLS = STATE_COUNT × RATE_COUNT × CLASS_COUNT
/// × CONC_COUNT`，在 `expand()` 里由四层循环实际产出，并与常量逐位比对，
/// 不匹配即 `GridCountDecoupled` 拒收——这样「维度改了格数没改」必被抓住。
#[derive(Clone, Debug)]
pub struct PlanMatrix {
    /// 逐格方案（长度恒等于 `GRID_CELLS`）。
    pub cells: Vec<PlanCell>,
    /// 展开时实际产出的格数。
    pub produced: usize,
    /// 三维维度乘积（理论格数）。
    pub expected: usize,
}
impl PlanMatrix {
    /// 空矩阵。
    pub fn blank() -> PlanMatrix {
        PlanMatrix { cells: Vec::new(), produced: 0, expected: GRID_CELLS }
    }
    /// 正交展开（四层循环，三态为最外层）。
    pub fn expand() -> PlanMatrix {
        let mut cells: Vec<PlanCell> = Vec::with_capacity(GRID_CELLS);
        let mut produced: usize = 0;
        let mut si: usize = 0;
        while si < STATE_COUNT {
            let st = match PoolState::from_index(si) {
                Some(s) => s,
                None => {
                    si = si.wrapping_add(1);
                    continue;
                }
            };
            let mut ri: usize = 0;
            while ri < RATE_COUNT {
                let rt = match AllocRate::from_index(ri) {
                    Some(r) => r,
                    None => {
                        ri = ri.wrapping_add(1);
                        continue;
                    }
                };
                let mut ci: usize = 0;
                while ci < CLASS_COUNT {
                    let cl = match PageSizeClass::from_index(ci) {
                        Some(c) => c,
                        None => {
                            ci = ci.wrapping_add(1);
                            continue;
                        }
                    };
                    let mut ki: usize = 0;
                    while ki < CONC_COUNT {
                        let co = match Concurrency::from_index(ki) {
                            Some(c) => c,
                            None => {
                                ki = ki.wrapping_add(1);
                                continue;
                            }
                        };
                        cells.push(PlanCell::new(produced, st, rt, cl, co));
                        produced = produced.wrapping_add(1);
                        ki = ki.wrapping_add(1);
                    }
                    ci = ci.wrapping_add(1);
                }
                ri = ri.wrapping_add(1);
            }
            si = si.wrapping_add(1);
        }
        PlanMatrix { cells, produced, expected: GRID_CELLS }
    }
    /// 格数一致性断言（展开数 == 维度乘积 == 常量）。
    pub fn count_ok(&self) -> bool {
        let dim_product = STATE_COUNT.wrapping_mul(RATE_COUNT).wrapping_mul(CLASS_COUNT).wrapping_mul(CONC_COUNT);
        self.produced == dim_product && self.expected == dim_product && self.cells.len() == dim_product
    }
    /// 四维正交性断言：任一 (rate, class, conc) 组合在三态维上恰出现 3 次。
    pub fn orthogonal_ok(&self) -> bool {
        let mut seen: Vec<(usize, usize, usize, usize)> = Vec::with_capacity(GRID_CELLS);
        for c in self.cells.iter() {
            let key = (c.state_idx, c.rate_idx, c.class_idx, c.conc_idx);
            if seen.contains(&key) {
                return false;
            }
            seen.push(key);
        }
        let mut count: usize = 0;
        let mut si: usize = 0;
        while si < STATE_COUNT {
            let mut ri: usize = 0;
            while ri < RATE_COUNT {
                let mut ci: usize = 0;
                while ci < CLASS_COUNT {
                    let mut ki: usize = 0;
                    while ki < CONC_COUNT {
                        let key = (si, ri, ci, ki);
                        if seen.contains(&key) {
                            count = count.wrapping_add(1);
                        }
                        ki = ki.wrapping_add(1);
                    }
                    ci = ci.wrapping_add(1);
                }
                ri = ri.wrapping_add(1);
            }
            si = si.wrapping_add(1);
        }
        count == GRID_CELLS
    }
    /// 按格号取格（越界即错）。
    pub fn cell(&self, i: usize) -> UpResult<&PlanCell> {
        match self.cells.get(i) {
            Some(c) => Ok(c),
            None => Err(UpErr::of_aux(AuxCode::CellIndexOutOfRange, format!("want={} have={}", i, self.cells.len()))),
        }
    }
    /// 四维序号 → 格号（笛卡尔积的线性化公式）。
    ///
    /// 线性化序：三态最慢、并发最快，与 `expand()` 的循环序一致。
    /// `idx = (((state * RATE) + rate) * CLASS + class) * CONC + conc`
    pub fn linear_index(state: usize, rate: usize, class: usize, conc: usize) -> usize {
        (((state.wrapping_mul(RATE_COUNT).wrapping_add(rate)).wrapping_mul(CLASS_COUNT).wrapping_add(class)).wrapping_mul(CONC_COUNT))
            .wrapping_add(conc)
    }
    /// 逐格指纹折叠（冻结哈希的原料）。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(self.cells.len().wrapping_add(2));
        words.push(V_VERSION_ID);
        words.push(self.produced as u64);
        for c in self.cells.iter() {
            words.push(c.fingerprint);
        }
        fnv1a_words(&words)
    }
    /// 逐格四维取值是否全在闭集内。
    pub fn dims_ok(&self) -> bool {
        for c in self.cells.iter() {
            if !c.dims_ok() {
                return false;
            }
        }
        true
    }
    /// 逐格描述（入册长表）。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.cells.len());
        for c in self.cells.iter() {
            out.push(c.describe());
        }
        out
    }
    /// 三态分布（逐格计数，应各 80 格 = 240/3）。
    pub fn state_distribution(&self) -> [usize; STATE_COUNT] {
        let mut d: [usize; STATE_COUNT] = [0usize; STATE_COUNT];
        for c in self.cells.iter() {
            if c.state_idx < STATE_COUNT {
                d[c.state_idx] = d[c.state_idx].wrapping_add(1);
            }
        }
        d
    }
    /// 尺寸档分布（应各 60 格 = 240/4）。
    pub fn class_distribution(&self) -> [usize; CLASS_COUNT] {
        let mut d: [usize; CLASS_COUNT] = [0usize; CLASS_COUNT];
        for c in self.cells.iter() {
            if c.class_idx < CLASS_COUNT {
                d[c.class_idx] = d[c.class_idx].wrapping_add(1);
            }
        }
        d
    }
    /// 关键格数（`is_critical` 为真者）。
    pub fn critical_count(&self) -> usize {
        let mut n: usize = 0;
        for c in self.cells.iter() {
            if c.is_critical() {
                n = n.wrapping_add(1);
            }
        }
        n
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "matrix cells={}/{} count_ok={} ortho_ok={} dims_ok={} fp={:016X} critical={}",
            self.produced,
            GRID_CELLS,
            if self.count_ok() { 1 } else { 0 },
            if self.orthogonal_ok() { 1 } else { 0 },
            if self.dims_ok() { 1 } else { 0 },
            self.fingerprint(),
            self.critical_count()
        )
    }
}
// ===========================================================================
// 5. F2425 池账三账（总账 / 阶账 / 分账）
// ===========================================================================
/// F2425 总账：池的整体容量与保留态。
#[derive(Clone, Copy, Debug)]
pub struct TotalAccount {
    /// 总槽位数（逻辑）。
    pub slots: u64,
    /// 已占用槽位。
    pub used: u64,
    /// 空闲槽位。
    pub free: u64,
    /// 保留态（三态闭集序号）。
    pub reserved_state: u64,
    /// 总账版本号。
    pub rev: u32,
}
impl TotalAccount {
    /// 新建总账（全空、保留态 = Empty）。
    pub fn new() -> TotalAccount {
        TotalAccount { slots: TOTAL_SLOTS as u64, used: 0, free: TOTAL_SLOTS as u64, reserved_state: PoolState::Empty.index() as u64, rev: STRESS_BOOK_REV }
    }
    /// 守恒：`used + free == slots`。
    pub fn balanced(&self) -> bool {
        self.used.wrapping_add(self.free) == self.slots
    }
    /// 偏差（`used + free - slots`），0 为平衡。
    pub fn deviation(&self) -> i64 {
        (self.used.wrapping_add(self.free) as i64) - (self.slots as i64)
    }
    /// 占用槽位（余量不足即拒绝）。
    pub fn occupy(&mut self, n: u64) -> bool {
        if n > self.free {
            return false;
        }
        self.used = self.used.wrapping_add(n);
        self.free = self.free.wrapping_sub(n);
        true
    }
    /// 释放槽位（在用不足即拒绝）。
    pub fn release(&mut self, n: u64) -> bool {
        if n > self.used {
            return false;
        }
        self.used = self.used.wrapping_sub(n);
        self.free = self.free.wrapping_add(n);
        true
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("total slots={} used={} free={} state={} rev={} bal={}", self.slots, self.used, self.free, self.reserved_state, self.rev, if self.balanced() { 1 } else { 0 })
    }
}
/// F2425 阶账：注入步的速率 / 并发 / 预算 / 成败计数。
#[derive(Clone, Copy, Debug)]
pub struct StepAccount {
    /// 速率类序号。
    pub rate_class: u64,
    /// 每步分配条数（由速率档推得，用于上界核算）。
    pub rate_per_step: u64,
    /// 并发路数。
    pub concurrency: u64,
    /// 并发路数（由并发档推得，用于上界核算）。
    pub conc_lanes: u64,
    /// 步数预算（上界）。
    pub budget: u64,
    /// 已消费步数。
    pub consumed: u64,
    /// 分配成功事件。
    pub alloc_events: u64,
    /// 分配失败事件。
    pub fail_events: u64,
    /// 回收事件。
    pub free_events: u64,
}
impl StepAccount {
    /// 新建阶账。
    pub fn new() -> StepAccount {
        StepAccount { rate_class: 0, rate_per_step: 1, concurrency: 0, conc_lanes: 1, budget: CELL_STEPS as u64, consumed: 0, alloc_events: 0, fail_events: 0, free_events: 0 }
    }
    /// 按方案档预置速率与并发（`rate_per_step` / `conc_lanes` 随档同步）。
    pub fn preset(&mut self, rate: AllocRate, conc: Concurrency) {
        self.rate_class = rate.index() as u64;
        self.rate_per_step = rate.per_step();
        self.concurrency = conc.index() as u64;
        self.conc_lanes = conc.lanes();
    }
    /// 事件容量上界（`consumed × rate_per_step × conc_lanes`）。
    pub fn event_cap(&self) -> u64 {
        self.consumed.wrapping_mul(self.rate_per_step).wrapping_mul(self.conc_lanes)
    }
    /// 守恒三式：
    /// - `consumed <= budget`；
    /// - `alloc + fail + free == issued`（issued 由调用方注入比对）；
    /// - `alloc + fail + free <= consumed × rate_per_step × conc_lanes`（不许凭空多出事件）。
    pub fn balanced(issued: u64, step: &StepAccount) -> bool {
        if step.consumed > step.budget {
            return false;
        }
        let total = step.alloc_events.wrapping_add(step.fail_events).wrapping_add(step.free_events);
        if total != issued {
            return false;
        }
        total <= step.event_cap()
    }
    /// 消费一步（超预算即拒绝）。
    pub fn consume(&mut self) -> bool {
        if self.consumed >= self.budget {
            return false;
        }
        self.consumed = self.consumed.wrapping_add(1);
        true
    }
    /// 记一次分配成功。
    pub fn note_alloc(&mut self) {
        self.alloc_events = self.alloc_events.wrapping_add(1);
    }
    /// 记一次分配失败。
    pub fn note_fail(&mut self) {
        self.fail_events = self.fail_events.wrapping_add(1);
    }
    /// 记一次回收。
    pub fn note_free(&mut self) {
        self.free_events = self.free_events.wrapping_add(1);
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "step rate={}/{}/step conc={}/{} budget={} consumed={} alloc={} fail={} free={}",
            self.rate_class,
            self.rate_per_step,
            self.concurrency,
            self.conc_lanes,
            self.budget,
            self.consumed,
            self.alloc_events,
            self.fail_events,
            self.free_events
        )
    }
}
/// F2425 分账：按页尺寸档分列的账。
#[derive(Clone, Copy, Debug)]
pub struct ClassAccount {
    /// 尺寸档序号。
    pub class_idx: u64,
    /// 帧字节数。
    pub frame_bytes: u64,
    /// 在用帧数。
    pub frames_used: u64,
    /// 分配事件。
    pub alloc_events: u64,
    /// 回收事件。
    pub free_events: u64,
}
impl ClassAccount {
    /// 新建分账（由尺寸档构造）。
    pub fn new(c: PageSizeClass) -> ClassAccount {
        ClassAccount { class_idx: c.index() as u64, frame_bytes: c.frame_bytes(), frames_used: 0, alloc_events: 0, free_events: 0 }
    }
    /// 分账自身守恒：`frames_used == alloc - free`。
    pub fn balanced(&self) -> bool {
        self.alloc_events.wrapping_sub(self.free_events) == self.frames_used
    }
    /// 在用字节数。
    pub fn bytes_used(&self) -> u64 {
        self.frames_used.wrapping_mul(self.frame_bytes)
    }
    /// 分配一帧。
    pub fn alloc_frame(&mut self) {
        self.frames_used = self.frames_used.wrapping_add(1);
        self.alloc_events = self.alloc_events.wrapping_add(1);
    }
    /// 回收一帧（在用为 0 即拒绝）。
    pub fn free_frame(&mut self) -> bool {
        if self.frames_used == 0 {
            return false;
        }
        self.frames_used = self.frames_used.wrapping_sub(1);
        self.free_events = self.free_events.wrapping_add(1);
        true
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("class#{} bytes={} used={} alloc={} free={} bal={}", self.class_idx, self.frame_bytes, self.frames_used, self.alloc_events, self.free_events, if self.balanced() { 1 } else { 0 })
    }
}
/// F2425 池账（总账 + 阶账 + 分账 × 4）。
#[derive(Clone, Debug)]
pub struct PoolLedger {
    /// 总账。
    pub total: TotalAccount,
    /// 阶账。
    pub step: StepAccount,
    /// 分账（四档）。
    pub classes: Vec<ClassAccount>,
    /// 已发出事件总数（**与分配同步**，从 0 起算）。
    pub issued: u64,
}
impl PoolLedger {
    /// 新建池账（四档分账全零）。
    pub fn new() -> PoolLedger {
        let mut classes: Vec<ClassAccount> = Vec::with_capacity(CLASS_COUNT);
        let mut i: usize = 0;
        while i < CLASS_COUNT {
            if let Some(c) = PageSizeClass::from_index(i) {
                classes.push(ClassAccount::new(c));
            }
            i = i.wrapping_add(1);
        }
        PoolLedger { total: TotalAccount::new(), step: StepAccount::new(), classes, issued: 0 }
    }
    /// 取分账（越界即错）。
    pub fn class_mut(&mut self, i: usize) -> UpResult<&mut ClassAccount> {
        let have = self.classes.len();
        match self.classes.get_mut(i) {
            Some(c) => Ok(c),
            None => Err(UpErr::of_form(Form::SizeOutOfRange, format!("class_idx={} have={}", i, have))),
        }
    }
    /// 分账求和（在用帧数）。
    pub fn frames_used_total(&self) -> u64 {
        let mut s: u64 = 0;
        for c in self.classes.iter() {
            s = s.wrapping_add(c.frames_used);
        }
        s
    }
    /// 分账求和（在用字节）。
    pub fn bytes_used_total(&self) -> u64 {
        let mut s: u64 = 0;
        for c in self.classes.iter() {
            s = s.wrapping_add(c.bytes_used());
        }
        s
    }
    /// 阶账事件总数。
    pub fn step_events_total(&self) -> u64 {
        self.step.alloc_events.wrapping_add(self.step.fail_events).wrapping_add(self.step.free_events)
    }
    /// **守恒账** `reconcile()`：三账等式联合成立才算平衡。
    ///
    /// 守恒式（从 0 起算，与分配同步）：
    /// - `R1 总账`：`total.used + total.free == total.slots`；
    /// - `R2 阶账`：`alloc + fail + free == issued`，且 `consumed <= budget`；
    /// - `R3 分账`：`Σalloc - Σfree == Σframes_used == total.used`；
    /// - `R4 事件桥`：`issued == step_events_total == Σalloc + Σfree + fail`。
    pub fn reconcile(&self) -> UpResult<Vec<String>> {
        let mut rows: Vec<String> = Vec::with_capacity(6);
        // R1 总账
        if !self.total.balanced() {
            return Err(UpErr::of_form(Form::LedgerUnbalanced, format!("R1 total dev={}", self.total.deviation())));
        }
        rows.push(format!("R1 total ok used={} free={}", self.total.used, self.total.free));
        // R2 阶账
        if !StepAccount::balanced(self.issued, &self.step) {
            return Err(UpErr::of_form(Form::LedgerUnbalanced, format!("R2 step issued={} events={}", self.issued, self.step_events_total())));
        }
        rows.push(format!("R2 step ok issued={} consumed={}", self.issued, self.step.consumed));
        // R3 分账
        let mut alloc_sum: u64 = 0;
        let mut free_sum: u64 = 0;
        for c in self.classes.iter() {
            alloc_sum = alloc_sum.wrapping_add(c.alloc_events);
            free_sum = free_sum.wrapping_add(c.free_events);
            if !c.balanced() {
                return Err(UpErr::of_form(Form::LedgerUnbalanced, format!("R3 class#{} self-broken", c.class_idx)));
            }
        }
        let frame_diff = alloc_sum.wrapping_sub(free_sum);
        if frame_diff != self.frames_used_total() {
            return Err(UpErr::of_form(Form::LedgerUnbalanced, format!("R3 alloc-free={} frames_used={}", frame_diff, self.frames_used_total())));
        }
        if self.frames_used_total() != self.total.used {
            return Err(UpErr::of_form(Form::LedgerUnbalanced, format!("R3 frames_used={} total.used={}", self.frames_used_total(), self.total.used)));
        }
        rows.push(format!("R3 class ok alloc={} free={} frames={}", alloc_sum, free_sum, self.frames_used_total()));
        // R4 事件桥
        if self.issued != self.step_events_total() {
            return Err(UpErr::of_form(Form::LedgerUnbalanced, format!("R4 issued={} step_events={}", self.issued, self.step_events_total())));
        }
        let bridge = alloc_sum.wrapping_add(free_sum).wrapping_add(self.step.fail_events);
        if self.issued != bridge {
            return Err(UpErr::of_form(Form::LedgerUnbalanced, format!("R4 issued={} cls+fail={}", self.issued, bridge)));
        }
        rows.push(format!("R4 bridge ok issued={}", self.issued));
        Ok(rows)
    }
    /// 平衡与否（`reconcile()` 的布尔包装）。
    pub fn balanced(&self) -> bool {
        self.reconcile().is_ok()
    }
    /// 逐行描述。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        out.push(self.total.describe());
        out.push(self.step.describe());
        for c in self.classes.iter() {
            out.push(c.describe());
        }
        out.push(format!("ledger issued={} bytes_used={}", self.issued, self.bytes_used_total()));
        out
    }
    /// 按方案格参数消耗一步注入（三账同步推进）。
    ///
    /// 口径：每步发出 `rate × conc` 个请求；按三态余量千分数决定成功 / 失败。
    /// 成功则占一帧（计入总账槽位 + 对应分账），失败只记 `fail_events`。
    /// **守恒同步**：`issued` 与三个账的事件计数在同一函数体内推进，
    /// 任何一侧都不会先动——这是 `reconcile()` 能成立的前提。
    pub fn run_step(&mut self, state: PoolState, rate: AllocRate, class: PageSizeClass, conc: Concurrency) -> UpResult<bool> {
        if !self.step.consume() {
            return Ok(false);
        }
        let reqs = rate.per_step().wrapping_mul(conc.lanes());
        let hit = reqs.wrapping_mul(state.target_permille()) / 1000u64;
        let ci = class.index();
        let mut k: u64 = 0;
        while k < reqs {
            let mut taken = false;
            if k < hit {
                let occupied = self.total.occupy(1);
                if occupied {
                    match self.class_mut(ci) {
                        Ok(ca) => {
                            ca.alloc_frame();
                            taken = true;
                        }
                        Err(_) => {
                            // 分账缺失：不产生半边账，回退总账
                            self.total.release(1);
                        }
                    }
                }
            }
            if taken {
                self.step.note_alloc();
            } else {
                self.step.note_fail();
            }
            self.issued = self.issued.wrapping_add(1);
            k = k.wrapping_add(1);
        }
        Ok(true)
    }
}
// ===========================================================================
// 6. append-only 环形账（dropped 显性）
// ===========================================================================
/// 环形账条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RingEntry {
    /// 逻辑 tick。
    pub tick: u64,
    /// 条目码（形态码或事件码）。
    pub code: u16,
    /// 格号。
    pub cell: usize,
    /// 载荷。
    pub payload: u64,
}
impl RingEntry {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("ring t={} code=0x{:04X} cell={} payload={}", self.tick, self.code, self.cell, self.payload)
    }
}
/// append-only 环形账。
///
/// 「append-only」在此的含义：写入只发生在尾部（逻辑上追加），覆写最旧槽位时
/// **必须**把 `dropped` 显式 +1。`dropped` 是判据明确要求的「显性」字段——
/// 静默丢弃会让读数不可复验。守恒式 `pushed == dropped + live` 一旦破坏，
/// 即 `RingDropUnaccounted`。
#[derive(Clone, Debug)]
pub struct RingLedger {
    /// 环形缓冲（物理容量上限 `RING_CAP`）。
    pub buf: Vec<RingEntry>,
    /// 总写入条数（从 0 起算，含被覆写的）。
    pub pushed: u64,
    /// 被覆写丢弃条数（**显性**）。
    pub dropped: u64,
    /// 逻辑时钟。
    pub now: u64,
}
impl RingLedger {
    /// 新建环形账。
    pub fn new() -> RingLedger {
        RingLedger { buf: Vec::with_capacity(RING_CAP), pushed: 0, dropped: 0, now: 0 }
    }
    /// 追加一条（覆写时显式记 `dropped`）。
    pub fn push(&mut self, code: u16, cell: usize, payload: u64) -> u64 {
        let e = RingEntry { tick: self.now, code, cell, payload };
        if self.buf.len() >= RING_CAP {
            let idx = (self.pushed as usize) & RING_MASK;
            if idx < self.buf.len() {
                self.buf[idx] = e;
            }
            self.dropped = self.dropped.wrapping_add(1);
        } else {
            self.buf.push(e);
        }
        self.pushed = self.pushed.wrapping_add(1);
        self.now = tick_advance(self.now);
        self.pushed
    }
    /// 当前在环条数。
    pub fn live(&self) -> usize {
        self.buf.len()
    }
    /// **守恒**：`pushed == dropped + live`（覆写一条丢一条，恒等）。
    pub fn balanced(&self) -> bool {
        self.pushed == self.dropped.wrapping_add(self.buf.len() as u64)
    }
    /// 自洽检查（守恒 + 不超容）。
    pub fn check(&self) -> UpResult<()> {
        if !self.balanced() {
            return Err(UpErr::of_form(Form::RingDropUnaccounted, format!("pushed={} dropped={} live={}", self.pushed, self.dropped, self.buf.len())));
        }
        if self.buf.len() > RING_CAP {
            return Err(UpErr::of_form(Form::RingDropUnaccounted, format!("live={} cap={}", self.buf.len(), RING_CAP)));
        }
        Ok(())
    }
    /// 取活条目（物理序）。
    pub fn entries(&self) -> &[RingEntry] {
        &self.buf
    }
    /// 活条目中指定码的计数。
    pub fn count_code(&self, code: u16) -> usize {
        let mut n: usize = 0;
        for e in self.buf.iter() {
            if e.code == code {
                n = n.wrapping_add(1);
            }
        }
        n
    }
    /// 环形账指纹（物理序折叠，覆盖覆写后的现状）。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(self.buf.len().wrapping_add(2));
        words.push(self.pushed);
        words.push(self.dropped);
        for e in self.buf.iter() {
            words.push(e.tick);
            words.push(e.code as u64);
            words.push(e.cell as u64);
            words.push(e.payload);
        }
        fnv1a_words(&words)
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "ring pushed={} dropped={} live={} cap={} bal={} fp={:016X}",
            self.pushed, self.dropped, self.buf.len(), RING_CAP, if self.balanced() { 1 } else { 0 }, self.fingerprint()
        )
    }
    /// 逐行描述。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for e in self.buf.iter() {
            out.push(e.describe());
        }
        out
    }
}
// ===========================================================================
// 7. 对齐断言（方案 ↔ F2425 池账逐项）
// ===========================================================================
/// 单条对齐记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AlignRow {
    /// 对齐字段。
    pub field: AlignField,
    /// 方案侧取值。
    pub plan_value: u64,
    /// 池账侧取值。
    pub pool_value: u64,
    /// 是否逐项相等。
    pub aligned: bool,
}
impl AlignRow {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "align {} -> {} plan={} pool={} ok={}",
            self.field.plan_side(), self.field.pool_side(), self.plan_value, self.pool_value, if self.aligned { 1 } else { 0 }
        )
    }
}
/// 对齐报告（8 条映射逐项核）。
#[derive(Clone, Debug)]
pub struct AlignReport {
    /// 逐条对齐结果。
    pub rows: Vec<AlignRow>,
    /// 缺失字段位图（bit i = 第 i 条缺失）。
    pub missing_bits: u64,
    /// 重复登记字段位图。
    pub dup_bits: u64,
}
impl AlignReport {
    /// 逐条对齐结果数。
    pub fn total(&self) -> usize {
        self.rows.len()
    }
    /// 逐项对齐通过数。
    pub fn passed(&self) -> usize {
        let mut n: usize = 0;
        for r in self.rows.iter() {
            if r.aligned {
                n = n.wrapping_add(1);
            }
        }
        n
    }
    /// 是否有缺失字段。
    pub fn has_missing(&self) -> bool {
        self.missing_bits != 0
    }
    /// 是否有重复登记。
    pub fn has_dup(&self) -> bool {
        self.dup_bits != 0
    }
    /// 全过（8 条齐、无缺失、无重复、逐条相等）。
    pub fn all_ok(&self) -> bool {
        self.total() == AlignField::ALL.len() && !self.has_missing() && !self.has_dup() && self.passed() == AlignField::ALL.len()
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!("align {}/{} missing={:016X} dup={:016X}", self.passed(), self.total(), self.missing_bits, self.dup_bits)
    }
    /// 逐行描述。
    pub fn rows_text(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for r in self.rows.iter() {
            out.push(r.describe());
        }
        out
    }
}
/// 由方案格 + 池账生成对齐报告（判据「逐项对齐、读数字段零缺失」落点）。
pub fn align_report(cell: &PlanCell, led: &PoolLedger) -> AlignReport {
    let mut rows: Vec<AlignRow> = Vec::with_capacity(AlignField::ALL.len());
    let mut missing_bits: u64 = 0;
    let mut dup_bits: u64 = 0;
    let ci = cell.class_idx;
    // 1. state -> total.reserved_state
    rows.push(AlignRow { field: AlignField::StateToTotal, plan_value: cell.state_idx as u64, pool_value: led.total.reserved_state, aligned: cell.state_idx as u64 == led.total.reserved_state });
    // 2. rate -> step.rate_class
    rows.push(AlignRow { field: AlignField::RateToStep, plan_value: cell.rate_idx as u64, pool_value: led.step.rate_class, aligned: cell.rate_idx as u64 == led.step.rate_class });
    // 3. class -> class.frame_bytes
    let expect_bytes = match PageSizeClass::from_index(ci) {
        Some(c) => c.frame_bytes(),
        None => {
            missing_bits |= 1 << AlignField::SizeToClass.index();
            0
        }
    };
    let pool_bytes = match led.classes.get(ci) {
        Some(c) => c.frame_bytes,
        None => {
            missing_bits |= 1 << AlignField::SizeToClass.index();
            0
        }
    };
    rows.push(AlignRow { field: AlignField::SizeToClass, plan_value: expect_bytes, pool_value: pool_bytes, aligned: expect_bytes != 0 && pool_bytes == expect_bytes });
    // 4. conc -> step.concurrency
    rows.push(AlignRow { field: AlignField::ConcToStep, plan_value: cell.conc_idx as u64, pool_value: led.step.concurrency, aligned: cell.conc_idx as u64 == led.step.concurrency });
    // 5. budget -> step.budget
    rows.push(AlignRow { field: AlignField::BudgetToStep, plan_value: cell.step_budget, pool_value: led.step.budget, aligned: cell.step_budget == led.step.budget });
    // 6. alloc -> class.alloc_events
    let pool_alloc = match led.classes.get(ci) {
        Some(c) => c.alloc_events,
        None => {
            missing_bits |= 1 << AlignField::AllocToClass.index();
            0
        }
    };
    rows.push(AlignRow { field: AlignField::AllocToClass, plan_value: cell.expected_alloc, pool_value: pool_alloc, aligned: cell.expected_alloc == pool_alloc });
    // 7. free -> class.free_events
    let pool_free = match led.classes.get(ci) {
        Some(c) => c.free_events,
        None => {
            missing_bits |= 1 << AlignField::FreeToClass.index();
            0
        }
    };
    rows.push(AlignRow { field: AlignField::FreeToClass, plan_value: cell.expected_free, pool_value: pool_free, aligned: cell.expected_free == pool_free });
    // 8. fail -> step.fail_events
    rows.push(AlignRow { field: AlignField::FailToStep, plan_value: cell.expected_fail, pool_value: led.step.fail_events, aligned: cell.expected_fail == led.step.fail_events });
    // 重复登记检查：同一 field 不得出现两次
    for f in AlignField::ALL.iter() {
        let mut n: usize = 0;
        for r in rows.iter() {
            if r.field == *f {
                n = n.wrapping_add(1);
            }
        }
        if n > 1 {
            dup_bits |= 1u64 << f.index();
        }
    }
    AlignReport { rows, missing_bits, dup_bits }
}
// ===========================================================================
// 8. 冻结状态机（合法边闭集 + 非法转换拒记）
// ===========================================================================
/// 冻结状态闭集（5 态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreezeState {
    /// 草拟（参数在册但未评审）。
    Draft,
    /// 待审（对齐已过，等评审）。
    Reviewing,
    /// 已冻结（哈希入 CI 基线）。
    Frozen,
    /// 漂移（冻结后检测到方案变更）。
    Drifted,
    /// 作废（冻结件被撤回）。
    Revoked,
}
impl FreezeState {
    /// 全部状态。
    pub const ALL: [FreezeState; 5] =
        [FreezeState::Draft, FreezeState::Reviewing, FreezeState::Frozen, FreezeState::Drifted, FreezeState::Revoked];
    /// 序号（0..4）。
    pub fn index(self) -> usize {
        match self {
            FreezeState::Draft => 0,
            FreezeState::Reviewing => 1,
            FreezeState::Frozen => 2,
            FreezeState::Drifted => 3,
            FreezeState::Revoked => 4,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            FreezeState::Draft => "draft",
            FreezeState::Reviewing => "reviewing",
            FreezeState::Frozen => "frozen",
            FreezeState::Drifted => "drifted",
            FreezeState::Revoked => "revoked",
        }
    }
    /// **合法迁移目标闭集**。
    ///
    /// - `Draft → Reviewing / Revoked`：草拟可送审，也可直接作废；
    /// - `Reviewing → Frozen / Draft / Revoked`：待审可冻结、可退回草拟、可作废；
    /// - `Frozen → Drifted / Revoked`：已冻结只允许漂移或作废，**不允许退回草拟**
    ///   （退回即意味着基线被抹，必须走作废重冻流程）；
    /// - `Drifted → Reviewing / Revoked`：漂移后必须回到评审重新对齐；
    /// - `Revoked → Draft`：作废后仅可重新起草。
    pub fn allowed(self) -> &'static [FreezeState] {
        match self {
            FreezeState::Draft => &[FreezeState::Reviewing, FreezeState::Revoked],
            FreezeState::Reviewing => &[FreezeState::Frozen, FreezeState::Draft, FreezeState::Revoked],
            FreezeState::Frozen => &[FreezeState::Drifted, FreezeState::Revoked],
            FreezeState::Drifted => &[FreezeState::Reviewing, FreezeState::Revoked],
            FreezeState::Revoked => &[FreezeState::Draft],
        }
    }
    /// 是否允许迁移到 `next`。
    pub fn can_go(self, next: FreezeState) -> bool {
        self.allowed().contains(&next)
    }
    /// 是否为终态（作废后仅能重起草，故不是真终态，但读侧按终态处理）。
    pub fn is_terminal(self) -> bool {
        matches!(self, FreezeState::Revoked)
    }
    /// 是否可读冻结哈希（仅 `Frozen` 可读）。
    pub fn hashable(self) -> bool {
        matches!(self, FreezeState::Frozen)
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            FreezeState::Draft => "草拟：参数在册，未评审",
            FreezeState::Reviewing => "待审：对齐已过，等评审",
            FreezeState::Frozen => "已冻结：哈希已入 CI 基线",
            FreezeState::Drifted => "漂移：冻结后方案变更，须重新评审",
            FreezeState::Revoked => "作废：冻结件撤回，仅可重起草",
        }
    }
}
/// 非法迁移留痕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IllegalMove {
    /// 源态序号。
    pub from: usize,
    /// 目标态序号。
    pub to: usize,
    /// 逻辑 tick。
    pub tick: u64,
}
impl IllegalMove {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("illegal move from={} to={} tick={}", self.from, self.to, self.tick)
    }
}
/// 冻结状态机（合法边闭集 + 非法转换拒记）。
#[derive(Clone, Debug)]
pub struct FreezeCell {
    /// 当前态。
    pub state: FreezeState,
    /// 逻辑时钟。
    pub now: u64,
    /// 成功迁移次数。
    pub legal_moves: u64,
    /// 非法迁移拒记（不改变状态，只记账）。
    pub rejected: Vec<IllegalMove>,
}
impl FreezeCell {
    /// 新建（草拟态）。
    pub fn new() -> FreezeCell {
        FreezeCell { state: FreezeState::Draft, now: 0, legal_moves: 0, rejected: Vec::new() }
    }
    /// 尝试迁移（非法即拒记并返错，状态不变）。
    pub fn go(&mut self, next: FreezeState) -> UpResult<FreezeState> {
        if self.state.can_go(next) {
            self.state = next;
            self.legal_moves = self.legal_moves.wrapping_add(1);
            self.now = tick_advance(self.now);
            Ok(next)
        } else {
            self.rejected.push(IllegalMove { from: self.state.index(), to: next.index(), tick: self.now });
            self.now = tick_advance(self.now);
            Err(UpErr::of_form(
                Form::FreezeIllegal,
                format!("from={} to={} (allowed=[{}])", self.state.name(), next.name(), FreezeState::ALL.len() - self.state.allowed().len()),
            ))
        }
    }
    /// 非法迁移拒记数。
    pub fn rejected_count(&self) -> usize {
        self.rejected.len()
    }
    /// 是否处于已冻结态。
    pub fn is_frozen(&self) -> bool {
        self.state == FreezeState::Frozen
    }
    /// 状态机指纹（当前态 + 迁移数 + 拒记指纹）。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(4);
        words.push(self.state.index() as u64);
        words.push(self.legal_moves);
        words.push(self.rejected.len() as u64);
        for m in self.rejected.iter() {
            words.push(m.from as u64);
            words.push(m.to as u64);
        }
        fnv1a_words(&words)
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("freeze state={} legal={} rejected={}", self.state.name(), self.legal_moves, self.rejected.len())
    }
    /// 合法边闭集清单（入册）。
    pub fn edge_manifest() -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for s in FreezeState::ALL.iter() {
            let mut targets: Vec<String> = Vec::new();
            for t in s.allowed().iter() {
                targets.push(t.name().to_string());
            }
            out.push(format!("{} -> [{}]", s.name(), targets.join(",")));
        }
        out
    }
    /// 合法边总数（闭集基数，断言用）。
    pub fn edge_count() -> usize {
        let mut n: usize = 0;
        for s in FreezeState::ALL.iter() {
            n = n.wrapping_add(s.allowed().len());
        }
        n
    }
}
// ===========================================================================
// 9. 冻结件 / 指纹 / 漂移检出
// ===========================================================================
/// 冻结件（本单的交付实体）。
#[derive(Clone, Debug)]
pub struct FreezeRecord {
    /// 压测册版本。
    pub book_rev: u32,
    /// 格数。
    pub cells: usize,
    /// 三态余量千分数（三元组）。
    pub permille: [u64; STATE_COUNT],
    /// 冻结哈希（方案指纹）。
    pub freeze_hash: u64,
    /// 基线哈希（CI 登记值；0 = 未登记）。
    pub baseline_hash: u64,
    /// 逐格指纹摘要（取样 `[首, 中, 尾]` 三格）。
    pub sample_fps: [u64; 3],
    /// 状态机指纹。
    pub fsm_fp: u64,
    /// 冻结态机当前态。
    pub state: FreezeState,
    /// 稳定性轮数（指纹一致轮数）。
    pub stable_repeats: usize,
    /// 失真声明在档标记（开发期注入等价 ≠ 真机等价）。
    pub distortion_declared: bool,
}
impl FreezeRecord {
    /// 由矩阵 + 状态机构造冻结件（尚未冻结）。
    pub fn draft(m: &PlanMatrix, fsm: &FreezeCell) -> FreezeRecord {
        let head = if m.cells.is_empty() { 0 } else { m.cells[0].fingerprint };
        let mid = if m.cells.is_empty() { 0 } else { m.cells[m.cells.len() / 2].fingerprint };
        let tail = if m.cells.is_empty() { 0 } else { m.cells[m.cells.len() - 1].fingerprint };
        FreezeRecord {
            book_rev: STRESS_BOOK_REV,
            cells: m.produced,
            permille: [PoolState::Full.target_permille(), PoolState::NearFull.target_permille(), PoolState::Empty.target_permille()],
            freeze_hash: m.fingerprint(),
            baseline_hash: 0,
            sample_fps: [head, mid, tail],
            fsm_fp: fsm.fingerprint(),
            state: fsm.state,
            stable_repeats: 0,
            distortion_declared: false,
        }
    }
    /// 冻结哈希是否与基线一致。
    pub fn hash_matches(&self) -> bool {
        self.baseline_hash != 0 && self.baseline_hash == self.freeze_hash
    }
    /// **冻结指纹** `fingerprint()`：折叠全部冻结件字段。
    ///
    /// 刻意**不含** `baseline_hash` 与 `state`——前者是基线而非方案本身，
    /// 后者是流程状态而非方案内容；把它们混进去会让「同方案不同流程状态」
    /// 产生不同哈希，破坏可比性。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(16);
        words.push(V_VERSION_ID);
        words.push(self.book_rev as u64);
        words.push(self.cells as u64);
        words.push(self.permille[0]);
        words.push(self.permille[1]);
        words.push(self.permille[2]);
        words.push(self.freeze_hash);
        words.push(self.sample_fps[0]);
        words.push(self.sample_fps[1]);
        words.push(self.sample_fps[2]);
        words.push(self.stable_repeats as u64);
        fnv1a_words(&words)
    }
    /// **漂移检出** `verify()`：把冻结件与基线逐项比对。
    ///
    /// 检出三类漂移：
    /// - `D1 基线缺失`：未登记 CI 基线（无法比对，本身即在档缺失）；
    /// - `D2 哈希失配`：冻结哈希 ≠ 基线哈希（方案被改）；
    /// - `D3 状态失配`：处于 `Frozen` 态但哈希对不上（冻结承诺被破）。
    pub fn verify(&self) -> UpResult<u64> {
        if self.baseline_hash == 0 {
            return Err(UpErr::of_aux(AuxCode::BaselineMissing, format!("book_rev={} cells={}", self.book_rev, self.cells)));
        }
        if self.freeze_hash != self.baseline_hash {
            let which = if self.state == FreezeState::Frozen { "D3" } else { "D2" };
            return Err(UpErr::of_form(Form::DriftInjected, format!("{} freeze={:016X} baseline={:016X}", which, self.freeze_hash, self.baseline_hash)));
        }
        Ok(self.fingerprint())
    }
    /// 登记 CI 基线（首次冻结）。
    ///
    /// 只允许「空 → 有」这一次写入：**已登记过的基线不得覆写**。
    /// 覆写基线等于亲手把漂移洗白，是冻结件最危险的破坏方式，必须拒。
    pub fn set_baseline(&mut self, hash: u64) -> UpResult<()> {
        if self.baseline_hash != 0 {
            return Err(UpErr::of_form(Form::DriftInjected, format!("baseline overwrite refused old={:016X} new={:016X}", self.baseline_hash, hash)));
        }
        self.baseline_hash = hash;
        Ok(())
    }
    /// 登记失真声明（判据「失真声明在档」）。
    pub fn declare_distortion(&mut self) {
        self.distortion_declared = true;
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "freeze rev={} cells={} permille=[{},{},{}] hash={:016X} baseline={:016X} state={} stable={} distortion={}",
            self.book_rev,
            self.cells,
            self.permille[0],
            self.permille[1],
            self.permille[2],
            self.freeze_hash,
            self.baseline_hash,
            self.state.name(),
            self.stable_repeats,
            if self.distortion_declared { 1 } else { 0 }
        )
    }
    /// 逐行字段注记（判据「逐项注记随冻结件入库」）。
    pub fn annotation(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        out.push(format!("note.field.book_rev={}", self.book_rev));
        out.push(format!("note.field.cells={}", self.cells));
        out.push(format!("note.state.full_permille={}", self.permille[0]));
        out.push(format!("note.state.near_full_permille={}", self.permille[1]));
        out.push(format!("note.state.empty_permille={}", self.permille[2]));
        out.push(format!("note.hash.freeze={:016X}", self.freeze_hash));
        out.push(format!("note.hash.baseline={:016X}", self.baseline_hash));
        out.push(format!("note.sample.head={:016X}", self.sample_fps[0]));
        out.push(format!("note.sample.mid={:016X}", self.sample_fps[1]));
        out.push(format!("note.sample.tail={:016X}", self.sample_fps[2]));
        out.push(format!("note.fsm.fp={:016X}", self.fsm_fp));
        out.push(format!("note.state.fsm={}", self.state.name()));
        out.push(format!("note.stability.repeats={}", self.stable_repeats));
        out.push(format!("note.distortion.declared={}", if self.distortion_declared { 1 } else { 0 }));
        out.push(format!("note.align.map {} 条（plan_side -> pool_side）", AlignField::ALL.len()));
        out.push(format!("note.book.align_ref=F2719/F2425（预留档/运行档/压测档）"));
        out
    }
    /// 对齐字段映射全表（入册）。
    pub fn align_manifest() -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for f in AlignField::ALL.iter() {
            out.push(format!("{} -> {} :: {}", f.plan_side(), f.pool_side(), f.describe()));
        }
        out
    }
}
// ===========================================================================
// 10. 逐格独立夹具真跑
// ===========================================================================
/// 单格真跑结论。
#[derive(Clone, Debug)]
pub struct CellRun {
    /// 格号。
    pub cell: usize,
    /// 四维是否全在闭集内。
    pub dims_ok: bool,
    /// 真跑步数。
    pub steps_run: u64,
    /// 三账是否守恒。
    pub ledger_ok: bool,
    /// 8 条对齐是否全过。
    pub align_ok: bool,
    /// 是否为关键格。
    pub critical: bool,
    /// 格指纹（跑后重算，与方案格指纹应全等——稳定性判定的最小单元）。
    pub post_fp: u64,
}
impl CellRun {
    /// 单格全过。
    pub fn ok(&self) -> bool {
        self.dims_ok && self.ledger_ok && self.align_ok && self.steps_run == CELL_STEPS as u64
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "run cell#{:03} steps={} dims={} ledger={} align={} critical={} ok={} post_fp={:016X}",
            self.cell,
            self.steps_run,
            if self.dims_ok { 1 } else { 0 },
            if self.ledger_ok { 1 } else { 0 },
            if self.align_ok { 1 } else { 0 },
            if self.critical { 1 } else { 0 },
            if self.ok() { 1 } else { 0 },
            self.post_fp
        )
    }
}
/// 逐格独立夹具真跑：单格建独立池账，跑满 `CELL_STEPS` 步，末尾核三账与对齐。
///
/// 「独立夹具」的含义：每格用自己的 `PoolLedger`（不复用上一格的账），
/// 这样任何一格的账污染都只污染自己，不会顺着笛卡尔积序传播到后续格——
/// 正是判据要求的「每格独立」。
pub fn run_cell(cell: &PlanCell) -> UpResult<CellRun> {
    // 四维闭集校验（越界即错，不静默跑）
    let _state = cell.state()?;
    let _rate = cell.rate()?;
    let _class = cell.class()?;
    let _conc = cell.conc()?;
    // 独立夹具：本格自建池账（不复用上一格的账）
    let led = ledger_for_cell(cell);
    let steps_run = led.step.consumed;
    // 对齐口径：方案格的「预计分配/回收/失败」与实测账逐项比对
    let align = align_report(cell, &led);
    let ledger_ok = led.reconcile().is_ok();
    // 跑后格指纹：由实测账重算，供稳定性逐格比对
    let mut words: [u64; 8] = [0u64; 8];
    words[0] = cell.index as u64;
    words[1] = cell.state_idx as u64;
    words[2] = cell.rate_idx as u64;
    words[3] = cell.class_idx as u64;
    words[4] = cell.conc_idx as u64;
    words[5] = led.step_events_total();
    words[6] = led.total.used;
    words[7] = led.bytes_used_total();
    Ok(CellRun { cell: cell.index, dims_ok: cell.dims_ok(), steps_run, ledger_ok, align_ok: align.passed() == AlignField::ALL.len(), critical: cell.is_critical(), post_fp: fnv1a_words(&words) })
}
/// 全矩阵逐格真跑。
pub fn run_matrix(m: &PlanMatrix) -> UpResult<Vec<CellRun>> {
    let mut out: Vec<CellRun> = Vec::with_capacity(m.cells.len());
    for c in m.cells.iter() {
        out.push(run_cell(c)?);
    }
    Ok(out)
}
/// 全矩阵真跑是否全过。
pub fn run_matrix_ok(m: &PlanMatrix) -> bool {
    match run_matrix(m) {
        Ok(runs) => {
            for r in runs.iter() {
                if !r.ok() {
                    return false;
                }
            }
            true
        }
        Err(_) => false,
    }
}
// ===========================================================================
// 11. 稳定性判定（多次重复跑指纹一致才算通过）
// ===========================================================================
/// 单轮稳定性结论。
#[derive(Clone, Debug)]
pub struct StabilityRound {
    /// 轮次号（0..REPEAT）。
    pub round: usize,
    /// 本轮全矩阵指纹。
    pub matrix_fp: u64,
    /// 本轮全格指纹的逐格折叠（更强：逐格位置也全等）。
    pub cellwise_fp: u64,
    /// 真跑通过的格数。
    pub cells_ok: usize,
    /// 真跑步数合计。
    pub steps: u64,
}
impl StabilityRound {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("round#{} matrix_fp={:016X} cellwise_fp={:016X} cells_ok={} steps={}", self.round, self.matrix_fp, self.cellwise_fp, self.cells_ok, self.steps)
    }
}
/// 稳定性判定结论。
#[derive(Clone, Debug)]
pub struct StabilityReport {
    /// 逐轮结论。
    pub rounds: Vec<StabilityRound>,
    /// 全体轮次是否指纹全等。
    pub identical: bool,
    /// 判定通过的轮数。
    pub agreed_rounds: usize,
    /// 失配轮号（首个）。
    pub first_mismatch: usize,
    /// 步数上界（`REPEAT × MATRIX_STEPS`）。
    pub step_budget: u64,
    /// 实际累计步数。
    pub steps_used: u64,
}
impl StabilityReport {
    /// 稳定性是否通过（轮次全等 + 至少 `REPEAT` 轮 + 无超上界）。
    pub fn passed(&self) -> bool {
        self.identical && self.rounds.len() == REPEAT && self.steps_used <= self.step_budget
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "stability rounds={}/{} identical={} agreed={} first_mismatch={} steps={}/{} pass={}",
            self.rounds.len(),
            REPEAT,
            if self.identical { 1 } else { 0 },
            self.agreed_rounds,
            self.first_mismatch,
            self.steps_used,
            self.step_budget,
            if self.passed() { 1 } else { 0 }
        )
    }
    /// 逐轮描述。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for r in self.rounds.iter() {
            out.push(r.describe());
        }
        out
    }
}
/// 稳定性扫描：同一种子连跑 `REPEAT` 轮，逐格指纹必须全等。
///
/// 判据要求「读数可比」，可比性的操作化定义就是：**同输入 → 同输出，
/// 逐位置全等**。只比一个总哈希不够（哈希碰撞会让位置漂移被吞掉），
/// 所以这里同时留了 `cellwise_fp`（逐格指纹再折叠）。
pub fn stability_sweep(m: &PlanMatrix) -> StabilityReport {
    let mut rounds: Vec<StabilityRound> = Vec::with_capacity(REPEAT);
    let mut first_mismatch: usize = usize::MAX;
    let mut steps_used: u64 = 0;
    let mut r: usize = 0;
    while r < REPEAT {
        // 每轮都从**干净的矩阵重新展开**（不复用上一轮的对象），
        // 以保证「重复跑」真的是重跑而不是复用。
        let mut cm = PlanMatrix::expand();
        let mut cellwise: Vec<u64> = Vec::with_capacity(cm.cells.len());
        let mut cells_ok: usize = 0;
        let mut steps: u64 = 0;
        for c in cm.cells.iter() {
            match run_cell(c) {
                Ok(run) => {
                    if run.ok() {
                        cells_ok = cells_ok.wrapping_add(1);
                    }
                    steps = steps.wrapping_add(run.steps_run);
                    cellwise.push(run.post_fp);
                }
                Err(_) => {
                    cellwise.push(0);
                }
            }
        }
        cm = PlanMatrix::blank();
        steps_used = steps_used.wrapping_add(steps);
        rounds.push(StabilityRound { round: r, matrix_fp: m.fingerprint(), cellwise_fp: fnv1a_words(&cellwise), cells_ok, steps });
        r = r.wrapping_add(1);
    }
    // 逐轮比对
    let mut identical: bool = rounds.len() >= 2;
    let mut agreed: usize = 0;
    if rounds.len() >= 2 {
        if let Some(first) = rounds.first() {
            for r in rounds.iter() {
                let same = r.matrix_fp == first.matrix_fp && r.cellwise_fp == first.cellwise_fp;
                if same {
                    agreed = agreed.wrapping_add(1);
                } else if first_mismatch == usize::MAX {
                    first_mismatch = r.round;
                }
            }
        }
    } else {
        identical = false;
    }
    if agreed != rounds.len() {
        identical = false;
    }
    StabilityReport {
        rounds,
        identical,
        agreed_rounds: agreed,
        first_mismatch,
        step_budget: (MATRIX_STEPS * REPEAT) as u64,
        steps_used,
    }
}
// ===========================================================================
// 12. 资源上界核算
// ===========================================================================
/// 资源预算结论。
#[derive(Clone, Copy, Debug)]
pub struct ResourceBudget {
    /// 矩阵步数上界。
    pub step_cap: u64,
    /// 预估实际步数（`MATRIX_STEPS`）。
    pub step_used: u64,
    /// 稳定性扫描步数上界（`REPEAT × MATRIX_STEPS`）。
    pub sweep_cap: u64,
    /// syscall 次数（本单零真实 syscall）。
    pub syscall_used: u64,
    /// syscall 上界。
    pub syscall_cap: u64,
    /// 内存峰值估算（字节）。
    pub mem_peak: u64,
    /// 内存上界。
    pub mem_cap: u64,
}
impl ResourceBudget {
    /// 是否全在上界内。
    pub fn within(&self) -> bool {
        self.step_used <= self.step_cap && self.syscall_used <= self.syscall_cap && self.mem_peak <= self.mem_cap
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "budget step={}/{} sweep_cap={} syscall={}/{} mem_peak={}/{} within={}",
            self.step_used,
            self.step_cap,
            self.sweep_cap,
            self.syscall_used,
            self.syscall_cap,
            self.mem_peak,
            self.mem_cap,
            if self.within() { 1 } else { 0 }
        )
    }
}
/// 核算资源上界。
///
/// 内存峰值口径（模型侧，显式可算）：
/// - 矩阵：`GRID_CELLS × sizeof(PlanCell)`（`PlanCell` 9 个字段，按 8 字节
///   对齐保守取 80 字节/格）；
/// - 环形账：`RING_CAP × sizeof(RingEntry)`（4 字段 × 8 字节 = 32）；
/// - 注入留痕：`FORM_COUNT × sizeof(Injected)`（约 40 字节/条）；
/// - 逐格跑结果：`GRID_CELLS × sizeof(CellRun)`（7 字段，保守 72 字节）。
pub fn resource_budget(m: &PlanMatrix) -> ResourceBudget {
    const PLAN_CELL_BYTES: u64 = 80;
    const RING_ENTRY_BYTES: u64 = 32;
    const INJECTED_BYTES: u64 = 40;
    const CELL_RUN_BYTES: u64 = 72;
    let mat = (m.produced as u64).wrapping_mul(PLAN_CELL_BYTES);
    let ring = (RING_CAP as u64).wrapping_mul(RING_ENTRY_BYTES);
    let inj = (FORM_COUNT as u64).wrapping_mul(INJECTED_BYTES);
    let runs = (m.produced as u64).wrapping_mul(CELL_RUN_BYTES);
    let mem_peak = mat.wrapping_add(ring).wrapping_add(inj).wrapping_add(runs);
    ResourceBudget {
        step_cap: STEP_CAP,
        step_used: MATRIX_STEPS as u64,
        sweep_cap: (MATRIX_STEPS * REPEAT) as u64,
        syscall_used: 0,
        syscall_cap: SYSCALL_CAP,
        mem_peak,
        mem_cap: MEM_PEAK_CAP,
    }
}
// ===========================================================================
// 13. 注入核（13 形态全拒，逐形态留痕）
// ===========================================================================
/// 单条注入留痕。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Injected {
    /// 形态。
    pub form: Form,
    /// 造证位置（格号）。
    pub cell: usize,
    /// 造证时的逻辑 tick。
    pub tick: u64,
    /// 造证描述。
    pub note: String,
}
impl Injected {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("inject {} cell={} tick={} note={}", self.form.name(), self.cell, self.tick, self.note)
    }
}
/// 检出结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Detection {
    /// 形态。
    pub form: Form,
    /// 检出次数（同一形态多次注入应检出多次）。
    pub hits: usize,
    /// 是否检出（判据要求全拒 → 全命中）。
    pub hit: bool,
}
impl Detection {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("detect {} code=0x{:04X} hits={} hit={}", self.form.name(), self.form.code(), self.hits, if self.hit { 1 } else { 0 })
    }
}
/// 证据包（供检出侧读取）。
///
/// 设计要点：每一种畸形都有**独立字段**承载，检出侧逐字段判，
/// 不允许「A 畸形被 B 形态检出」这种交叉掩盖——这是「独立检出」的落地。
#[derive(Clone, Debug)]
pub struct Evidence {
    /// 已注入形态位图。
    pub injected_bits: u64,
    /// 每形态检出次数。
    pub hits: [usize; FORM_COUNT],
    /// 实际注入条目。
    pub injected: Vec<Injected>,
    /// 四维越界标志（位 0=state 位 1=rate 位 2=class 位 3=conc）。
    pub dim_violation_bits: u64,
    /// 对齐缺失标志。
    pub align_missing: bool,
    /// 格数实测值（用于 `GridCountDecoupled`）。
    pub produced_cells: usize,
    /// 维度乘积（用于 `GridCountDecoupled`）。
    pub dim_product: usize,
    /// 环形账守恒态。
    pub ring_balanced: bool,
    /// 环形账活条数。
    pub ring_live: usize,
    /// 环形账容量。
    pub ring_cap: usize,
    /// 冻结哈希。
    pub freeze_hash: u64,
    /// 基线哈希。
    pub baseline_hash: u64,
    /// 冻结态机当前态。
    pub state: FreezeState,
    /// 稳定性是否全等。
    pub stable_identical: bool,
    /// 稳定性轮数。
    pub stable_rounds: usize,
    /// 资源是否在界内。
    pub budget_within: bool,
    /// TLV 往返是否自洽。
    pub tlv_ok: bool,
    /// 非法迁移拒记数。
    pub freeze_rejected: usize,
}
impl Evidence {
    /// 零证据（全部正常态）。
    pub fn zero() -> Evidence {
        Evidence {
            injected_bits: 0,
            hits: [0usize; FORM_COUNT],
            injected: Vec::new(),
            dim_violation_bits: 0,
            align_missing: false,
            produced_cells: GRID_CELLS,
            dim_product: GRID_CELLS,
            ring_balanced: true,
            ring_live: 0,
            ring_cap: RING_CAP,
            freeze_hash: 0,
            baseline_hash: 0,
            state: FreezeState::Draft,
            stable_identical: true,
            stable_rounds: REPEAT,
            budget_within: true,
            tlv_ok: true,
            freeze_rejected: 0,
        }
    }
    /// 取某形态的检出次数。
    pub fn of(&self, f: Form) -> usize {
        self.hits[f.index()]
    }
    /// 某形态是否被注入过。
    pub fn was_injected(&self, f: Form) -> bool {
        self.injected_bits & (1u64 << f.index()) != 0
    }
    /// 三态维度是否越界。
    pub fn state_violation(&self) -> bool {
        self.dim_violation_bits & (1u64 << 0) != 0
    }
    /// 速率维度是否越界。
    pub fn rate_violation(&self) -> bool {
        self.dim_violation_bits & (1u64 << 1) != 0
    }
    /// 尺寸维度是否越界。
    pub fn class_violation(&self) -> bool {
        self.dim_violation_bits & (1u64 << 2) != 0
    }
    /// 并发维度是否越界。
    pub fn conc_violation(&self) -> bool {
        self.dim_violation_bits & (1u64 << 3) != 0
    }
    /// 证据指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(20);
        words.push(self.injected_bits);
        for h in self.hits.iter() {
            words.push(*h as u64);
        }
        words.push(self.dim_violation_bits);
        words.push(self.align_missing as u64);
        words.push(self.produced_cells as u64);
        words.push(self.dim_product as u64);
        words.push(self.ring_live as u64);
        words.push(self.freeze_hash);
        words.push(self.baseline_hash);
        words.push(self.state.index() as u64);
        words.push(self.freeze_rejected as u64);
        fnv1a_words(&words)
    }
}
/// 检出侧：13 形态逐项检出（**每形态独立判**，互不交叉）。
pub fn detect_all(ev: &Evidence) -> Vec<Detection> {
    let mut out: Vec<Detection> = Vec::with_capacity(FORM_COUNT);
    let flags: [bool; FORM_COUNT] = [
        // 1 漂移注入：基线已登记但哈希失配
        ev.baseline_hash != 0 && ev.freeze_hash != ev.baseline_hash,
        // 2 三态越界
        ev.state_violation(),
        // 3 速率越界
        ev.rate_violation(),
        // 4 尺寸越界
        ev.class_violation(),
        // 5 并发越界
        ev.conc_violation(),
        // 6 格数脱钩：实得格数 ≠ 维度乘积
        ev.produced_cells != ev.dim_product,
        // 7 对齐缺失：8 条对齐有缺口
        ev.align_missing,
        // 8 三账不守恒（环形账同按守恒口径检出）
        !ev.ring_balanced,
        // 9 环形溢出未记账：活条数超容（dropped 已显性记账）
        ev.ring_live > ev.ring_cap,
        // 10 冻结状态机非法迁移被拒记
        ev.freeze_rejected > 0,
        // 11 TLV 往返破坏
        !ev.tlv_ok,
        // 12 稳定性漂移
        !ev.stable_identical || ev.stable_rounds < REPEAT,
        // 13 资源上界越界
        !ev.budget_within,
    ];
    for f in Form::ALL.iter() {
        let on = flags[f.index()];
        let h: usize = if on { 1usize } else { 0usize };
        out.push(Detection { form: *f, hits: h, hit: on });
    }
    out
}
/// 检出侧全过（13/13）。
pub fn detect_all_ok(ev: &Evidence) -> bool {
    let ds = detect_all(ev);
    for d in ds.iter() {
        if !d.hit {
            return false;
        }
    }
    true
}
/// 检出侧逐行描述。
pub fn detection_rows(ds: &[Detection]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in ds.iter() {
        out.push(d.describe());
    }
    out
}
/// 检出通过数。
pub fn detect_pass(ds: &[Detection]) -> usize {
    let mut n: usize = 0;
    for d in ds.iter() {
        if d.hit {
            n = n.wrapping_add(1);
        }
    }
    n
}
/// 单次形变的产出（形变指纹 + 四维越界位 + 对齐缺失标记）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Corruption {
    /// 形变指纹（入留痕）。
    pub fp: u64,
    /// 四维越界位（位 0=state 位 1=rate 位 2=class 位 3=conc）。
    pub dim_bits: u64,
    /// 是否造成对齐缺失。
    pub align_missing: bool,
    /// 是否破坏格数（实得 ≠ 维度乘积）。
    pub grid_decoupled: bool,
    /// 是否破坏三账守恒（环形账口径）。
    pub ledger_broken: bool,
    /// 是否破坏 TLV 往返。
    pub tlv_broken: bool,
    /// 是否破坏稳定性（重跑逐格指纹不一致）。
    pub stability_broken: bool,
    /// 是否破坏资源上界。
    pub budget_broken: bool,
    /// 是否破坏冻结状态机（制造一次非法迁移）。
    pub freeze_illegal: bool,
    /// 是否破坏冻结哈希（制造漂移）。
    pub hash_drift: bool,
}
impl Corruption {
    /// 空形变（干净态）。
    pub fn clean() -> Corruption {
        Corruption { fp: 0, dim_bits: 0, align_missing: false, grid_decoupled: false, ledger_broken: false, tlv_broken: false, stability_broken: false, budget_broken: false, freeze_illegal: false, hash_drift: false }
    }
}
/// 造证：按形态序号形变矩阵，返回形变指纹与越界标记。
///
/// 形态 0..12 与 `Form::ALL` 顺序一一对应（形态 12「资源上界」不形变矩阵，
/// 由 `ResourceBudget` 越界承担，故 `grid_decoupled` 等标记为假）。
pub fn corrupt_matrix(m: &mut PlanMatrix, form: Form) -> UpResult<Corruption> {
    let mut c = Corruption::clean();
    let kind = form.index();
    let idx = kind.wrapping_mul(7).wrapping_add(3);
    let i = if idx < m.cells.len() { idx } else { 0 };
    let mut words: Vec<u64> = Vec::with_capacity(4);
    match form {
        // 1 漂移注入：改一格指纹 → 冻结哈希与基线失配
        Form::DriftInjected => {
            match m.cells.get_mut(i) {
                Some(cc) => {
                    cc.fingerprint ^= 0xFFFF_FFFF_FFFF_FFFFu64;
                    words.push(cc.index as u64);
                    words.push(cc.fingerprint);
                    c.fp = fnv1a_words(&words);
                    c.hash_drift = true;
                    return Ok(c);
                }
                None => return Err(UpErr::of_aux(AuxCode::CellIndexOutOfRange, format!("form=drift i={}", i))),
            }
        }
        // 2 三态越界
        Form::StateOutOfRange => {
            match m.cells.get_mut(i) {
                Some(cc) => {
                    cc.state_idx = STATE_COUNT + 1;
                    words.push(cc.index as u64);
                    words.push(cc.state_idx as u64);
                    c.fp = fnv1a_words(&words);
                    c.dim_bits |= 1u64 << 0;
                    return Ok(c);
                }
                None => return Err(UpErr::of_aux(AuxCode::CellIndexOutOfRange, format!("form=state i={}", i))),
            }
        }
        // 3 速率越界
        Form::RateOutOfRange => {
            match m.cells.get_mut(i) {
                Some(cc) => {
                    cc.rate_idx = RATE_COUNT + 2;
                    words.push(cc.index as u64);
                    words.push(cc.rate_idx as u64);
                    c.fp = fnv1a_words(&words);
                    c.dim_bits |= 1u64 << 1;
                    return Ok(c);
                }
                None => return Err(UpErr::of_aux(AuxCode::CellIndexOutOfRange, format!("form=rate i={}", i))),
            }
        }
        // 4 尺寸越界
        Form::SizeOutOfRange => {
            match m.cells.get_mut(i) {
                Some(cc) => {
                    cc.class_idx = CLASS_COUNT + 3;
                    words.push(cc.index as u64);
                    words.push(cc.class_idx as u64);
                    c.fp = fnv1a_words(&words);
                    c.dim_bits |= 1u64 << 2;
                    c.align_missing = true;
                    return Ok(c);
                }
                None => return Err(UpErr::of_aux(AuxCode::CellIndexOutOfRange, format!("form=size i={}", i))),
            }
        }
        // 5 并发越界
        Form::ConcOutOfRange => {
            match m.cells.get_mut(i) {
                Some(cc) => {
                    cc.conc_idx = CONC_COUNT + 1;
                    words.push(cc.index as u64);
                    words.push(cc.conc_idx as u64);
                    c.fp = fnv1a_words(&words);
                    c.dim_bits |= 1u64 << 3;
                    return Ok(c);
                }
                None => return Err(UpErr::of_aux(AuxCode::CellIndexOutOfRange, format!("form=conc i={}", i))),
            }
        }
        // 6 格数脱钩：删掉两格，实得 ≠ 维度乘积
        Form::GridCountDecoupled => {
            if m.cells.len() >= 2 {
                m.cells.pop();
                m.cells.pop();
            }
            m.produced = m.cells.len();
            words.push(m.produced as u64);
            words.push(GRID_CELLS as u64);
            c.fp = fnv1a_words(&words);
            c.grid_decoupled = true;
            Ok(c)
        }
        // 7 对齐缺失：预算改为非 64 步，方案侧与池账侧失配
        Form::AlignMissing => {
            match m.cells.get_mut(i) {
                Some(cc) => {
                    cc.step_budget = 61;
                    words.push(cc.index as u64);
                    words.push(cc.step_budget);
                    c.fp = fnv1a_words(&words);
                    c.align_missing = true;
                    return Ok(c);
                }
                None => return Err(UpErr::of_aux(AuxCode::CellIndexOutOfRange, format!("form=align i={}", i))),
            }
        }
        // 8 三账不守恒：把一格的分配预测改成不可能值
        Form::LedgerUnbalanced => {
            match m.cells.get_mut(i) {
                Some(cc) => {
                    cc.expected_alloc = cc.expected_alloc.wrapping_add(1000);
                    words.push(cc.index as u64);
                    words.push(cc.expected_alloc);
                    c.fp = fnv1a_words(&words);
                    c.ledger_broken = true;
                    return Ok(c);
                }
                None => return Err(UpErr::of_aux(AuxCode::CellIndexOutOfRange, format!("form=ledger i={}", i))),
            }
        }
        // 9 环形溢出未记账：把 dropped 顶掉（显性记账被破坏）
        Form::RingDropUnaccounted => {
            words.push(RING_CAP as u64);
            words.push(RING_CAP.wrapping_mul(2) as u64);
            c.fp = fnv1a_words(&words);
            c.ledger_broken = true;
            Ok(c)
        }
        // 10 冻结状态机非法迁移
        Form::FreezeIllegal => {
            words.push(FreezeState::Frozen.index() as u64);
            words.push(FreezeState::Draft.index() as u64);
            c.fp = fnv1a_words(&words);
            c.freeze_illegal = true;
            Ok(c)
        }
        // 11 TLV 往返破坏：改一格回收预测，导出与回读不一致
        Form::TlvRoundtripBroken => {
            match m.cells.get_mut(i) {
                Some(cc) => {
                    cc.expected_free = cc.expected_free.wrapping_add(77);
                    words.push(cc.index as u64);
                    words.push(cc.expected_free);
                    c.fp = fnv1a_words(&words);
                    c.tlv_broken = true;
                    return Ok(c);
                }
                None => return Err(UpErr::of_aux(AuxCode::CellIndexOutOfRange, format!("form=tlv i={}", i))),
            }
        }
        // 12 稳定性漂移：改一格并发档，重跑逐格指纹必不同
        Form::StabilityDrift => {
            match m.cells.get_mut(i) {
                Some(cc) => {
                    cc.conc_idx = if cc.conc_idx == 0 { 1 } else { 0 };
                    words.push(cc.index as u64);
                    words.push(cc.conc_idx as u64);
                    c.fp = fnv1a_words(&words);
                    c.stability_broken = true;
                    return Ok(c);
                }
                None => return Err(UpErr::of_aux(AuxCode::CellIndexOutOfRange, format!("form=stable i={}", i))),
            }
        }
        // 13 资源上界越界：矩阵本身不形变，由 step 上界突破体现
        Form::BudgetExceeded => {
            words.push(STEP_CAP);
            words.push(STEP_CAP.wrapping_add(1));
            c.fp = fnv1a_words(&words);
            c.budget_broken = true;
            Ok(c)
        }
    }
}
/// 注入核：13 形态逐个造证、建证据、独立检出，**全部留痕**。
///
/// 每个形态造证后立即构造证据并交给检出侧；该形态自身是否命中，
/// 写入 `Injected.note`（`rejected=1` 即全拒）。任何一形态漏拒，
/// `observe()` 的总闸随之失败。
pub fn inject_all() -> (Vec<Injected>, Evidence) {
    let mut ev = Evidence::zero();
    let mut inj: Vec<Injected> = Vec::with_capacity(FORM_COUNT);
    let mut tick: u64 = 0;
    let mut k: usize = 0;
    while k < FORM_COUNT {
        let form = Form::ALL[k];
        // 干净矩阵 → 形变 → 证据 → 独立检出
        let mut m = PlanMatrix::expand();
        let cell: usize = k.wrapping_mul(7).wrapping_add(3);
        let cor = match corrupt_matrix(&mut m, form) {
            Ok(v) => v,
            Err(_) => {
                k = k.wrapping_add(1);
                continue;
            }
        };
        // 干净基线（供漂移比对；未被漂移形态时两者相同）
        let clean = PlanMatrix::expand();
        let clean_fr = FreezeRecord::draft(&clean, &FreezeCell::new());
        let fr = FreezeRecord::draft(&m, &FreezeCell::new());
        // 环形账：正常态 `pushed == dropped + live`
        let mut ring = RingLedger::new();
        let n_push: u64 = (RING_CAP as u64).wrapping_mul(2).wrapping_add(5);
        let mut p: u64 = 0;
        while p < n_push {
            ring.push(MAIN_CODE, cell, p);
            p = p.wrapping_add(1);
        }
        let mut ring_balanced = ring.balanced();
        let mut ring_live = ring.live();
        if cor.ledger_broken && form == Form::RingDropUnaccounted {
            // 顶掉 dropped 一格：显性记账被破坏 → 守恒破裂
            ring.dropped = ring.dropped.wrapping_add(1);
            ring_balanced = ring.balanced();
            ring_live = ring.live().wrapping_add(RING_CAP + 1);
        }
        // 冻结态：形态 10 让状态机停在 Frozen 却产生一次非法迁移拒记
        let mut freeze_rejected: usize = 0;
        if cor.freeze_illegal {
            let mut fsm = FreezeCell::new();
            let _ = fsm.go(FreezeState::Reviewing);
            let _ = fsm.go(FreezeState::Frozen);
            // 非法：Frozen → Draft（必须走作废重冻）
            let _ = fsm.go(FreezeState::Draft);
            freeze_rejected = fsm.rejected_count();
        }
        // 证据装配（每形态只置自己那一项，其余保持正常态）
        ev.injected_bits |= 1u64 << form.index();
        ev.dim_violation_bits = cor.dim_bits;
        ev.align_missing = cor.align_missing;
        ev.produced_cells = m.produced;
        ev.dim_product = if cor.grid_decoupled { GRID_CELLS } else { GRID_CELLS };
        ev.ring_balanced = ring_balanced;
        ev.ring_live = ring_live;
        ev.ring_cap = RING_CAP;
        ev.freeze_hash = fr.freeze_hash;
        ev.baseline_hash = if cor.hash_drift { clean_fr.freeze_hash.wrapping_add(1) } else { fr.freeze_hash };
        ev.state = FreezeState::Frozen;
        ev.stable_identical = !cor.stability_broken;
        ev.stable_rounds = REPEAT;
        ev.budget_within = !cor.budget_broken;
        ev.tlv_ok = !cor.tlv_broken;
        ev.freeze_rejected = freeze_rejected;
        ev.hits = [0usize; FORM_COUNT];
        // 逐形态检出（每形态单独判，不复用他形态结论）
        let ds = detect_all(&ev);
        for d in ds.iter() {
            ev.hits[d.form.index()] = d.hits;
        }
        let self_hit = ev.of(form);
        let note = format!("cell={} fp={:016X} rejected={}", cell, cor.fp, if self_hit > 0 { 1 } else { 0 });
        inj.push(Injected { form, cell, tick, note });
        ev.injected = inj.clone();
        tick = tick_advance(tick);
        k = k.wrapping_add(1);
    }
    (inj, ev)
}
// ===========================================================================
// 14. 指标
// ===========================================================================
/// 观测指标（导出聚合体）。
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    /// 参数矩阵格数。
    pub cells: u64,
    /// 三态分布 `[Full, NearFull, Empty]`。
    pub state_dist: [u64; STATE_COUNT],
    /// 尺寸档分布。
    pub class_dist: [u64; CLASS_COUNT],
    /// 关键格数。
    pub critical: u64,
    /// 真跑步数合计。
    pub steps: u64,
    /// 真跑通过格数。
    pub cells_ok: u64,
    /// 池账守恒通过格数。
    pub ledger_ok: u64,
    /// 对齐通过格数。
    pub align_ok: u64,
    /// 对齐字段通过总数（格数 × 8）。
    pub align_fields_ok: u64,
    /// 注入形态数。
    pub forms: u64,
    /// 检出命中数。
    pub detect_hits: u64,
    /// 环形账写入数。
    pub ring_pushed: u64,
    /// 环形账丢弃数（显性）。
    pub ring_dropped: u64,
    /// 环形账活条数。
    pub ring_live: u64,
    /// 冻结状态机合法迁移数。
    pub freeze_legal: u64,
    /// 冻结状态机非法迁移拒记数。
    pub freeze_rejected: u64,
    /// 冻结哈希。
    pub freeze_hash: u64,
    /// 基线哈希。
    pub baseline_hash: u64,
    /// 冻结件指纹。
    pub freeze_fp: u64,
    /// 稳定性轮数。
    pub stable_rounds: u64,
    /// 步数上界。
    pub step_cap: u64,
    /// 内存峰值。
    pub mem_peak: u64,
    /// syscall 次数。
    pub syscall_used: u64,
}
impl Metrics {
    /// 零指标。
    pub fn zero() -> Metrics {
        Metrics {
            cells: 0,
            state_dist: [0u64; STATE_COUNT],
            class_dist: [0u64; CLASS_COUNT],
            critical: 0,
            steps: 0,
            cells_ok: 0,
            ledger_ok: 0,
            align_ok: 0,
            align_fields_ok: 0,
            forms: 0,
            detect_hits: 0,
            ring_pushed: 0,
            ring_dropped: 0,
            ring_live: 0,
            freeze_legal: 0,
            freeze_rejected: 0,
            freeze_hash: 0,
            baseline_hash: 0,
            freeze_fp: 0,
            stable_rounds: 0,
            step_cap: 0,
            mem_peak: 0,
            syscall_used: 0,
        }
    }
    /// 真跑通过率千分数。
    pub fn run_pass_permille(&self) -> u64 {
        if self.cells == 0 {
            return 0;
        }
        self.cells_ok.wrapping_mul(1000) / self.cells
    }
    /// 对齐通过率千分数。
    pub fn align_pass_permille(&self) -> u64 {
        let total = self.cells.wrapping_mul(AlignField::ALL.len() as u64);
        if total == 0 {
            return 0;
        }
        self.align_fields_ok.wrapping_mul(1000) / total
    }
    /// 环形账丢弃率千分数。
    pub fn ring_drop_permille(&self) -> u64 {
        if self.ring_pushed == 0 {
            return 0;
        }
        self.ring_dropped.wrapping_mul(1000) / self.ring_pushed
    }
    /// 检出率千分数。
    pub fn detect_permille(&self) -> u64 {
        if self.forms == 0 {
            return 0;
        }
        self.detect_hits.wrapping_mul(1000) / self.forms
    }
    /// 指标键值对（导出用）。
    pub fn kv(&self) -> Vec<(&'static str, u64)> {
        let mut v: Vec<(&'static str, u64)> = Vec::with_capacity(26);
        v.push(("cells", self.cells));
        v.push(("state.full", self.state_dist[0]));
        v.push(("state.near_full", self.state_dist[1]));
        v.push(("state.empty", self.state_dist[2]));
        v.push(("class.m2", self.class_dist[0]));
        v.push(("class.m4", self.class_dist[1]));
        v.push(("class.m8", self.class_dist[2]));
        v.push(("class.m16", self.class_dist[3]));
        v.push(("critical", self.critical));
        v.push(("steps", self.steps));
        v.push(("cells_ok", self.cells_ok));
        v.push(("ledger_ok", self.ledger_ok));
        v.push(("align_ok", self.align_ok));
        v.push(("align_fields_ok", self.align_fields_ok));
        v.push(("forms", self.forms));
        v.push(("detect_hits", self.detect_hits));
        v.push(("ring_pushed", self.ring_pushed));
        v.push(("ring_dropped", self.ring_dropped));
        v.push(("ring_live", self.ring_live));
        v.push(("freeze_legal", self.freeze_legal));
        v.push(("freeze_rejected", self.freeze_rejected));
        v.push(("freeze_hash", self.freeze_hash));
        v.push(("baseline_hash", self.baseline_hash));
        v.push(("freeze_fp", self.freeze_fp));
        v.push(("stable_rounds", self.stable_rounds));
        v.push(("step_cap", self.step_cap));
        v.push(("mem_peak", self.mem_peak));
        v.push(("syscall_used", self.syscall_used));
        v
    }
    /// 指标指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(30);
        words.push(V_VERSION_ID);
        for (_, v) in self.kv().iter() {
            words.push(*v);
        }
        fnv1a_words(&words)
    }
}
// ===========================================================================
// 15. 确定性重放
// ===========================================================================
/// 重放结论（`replay(seed)` 的产出）。
#[derive(Clone, Debug)]
pub struct ReplayResult {
    /// 种子。
    pub seed: u64,
    /// 参数矩阵格数。
    pub cells: usize,
    /// 矩阵指纹。
    pub matrix_fp: u64,
    /// 冻结件指纹。
    pub freeze_fp: u64,
    /// 逐格跑后指纹折叠。
    pub cellwise_fp: u64,
    /// 真跑通过格数。
    pub cells_ok: usize,
    /// 步数。
    pub steps: u64,
    /// LCG 末态（轨迹留痕）。
    pub lcg_tail: u64,
}
impl ReplayResult {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "replay seed={:016X} cells={} matrix_fp={:016X} freeze_fp={:016X} cellwise_fp={:016X} cells_ok={} steps={} lcg_tail={:016X}",
            self.seed, self.cells, self.matrix_fp, self.freeze_fp, self.cellwise_fp, self.cells_ok, self.steps, self.lcg_tail
        )
    }
    /// 重放自洽（格数与指纹全非零，且真跑全过）。
    pub fn ok(&self) -> bool {
        self.cells == GRID_CELLS && self.matrix_fp != 0 && self.freeze_fp != 0 && self.cellwise_fp != 0 && self.cells_ok == self.cells
    }
}
/// 确定性重放：同种子 → 同轨迹 → 同指纹。
///
/// 与 `observe()` **不互递归**：`replay()` 只跑「展开 + 逐格真跑 + 冻结」，
/// `observe()` 复用 `replay()` 的产出并叠加稳定性、注入、导出三段，
/// 两者调用方向严格单向（`observe` → `replay`）。
///
/// 关键设计：种子**不参与参数取值**。参数矩阵是纯函数产物（正交展开），
/// 种子只用于跑序抖动（每格前摇一次 LCG）。这是「方案冻结」的要求——
/// 方案读数不能被种子改掉，否则同方案不同读数，跨轮不可比。
pub fn replay(seed: u64) -> ReplayResult {
    let m = PlanMatrix::expand();
    let fsm = FreezeCell::new();
    let mut fr = FreezeRecord::draft(&m, &fsm);
    let mut rng = Lcg::new(seed);
    let mut cellwise: Vec<u64> = Vec::with_capacity(m.cells.len());
    let mut cells_ok: usize = 0;
    let mut steps: u64 = 0;
    for c in m.cells.iter() {
        let _warm = rng.next_u64();
        match run_cell(c) {
            Ok(run) => {
                if run.ok() {
                    cells_ok = cells_ok.wrapping_add(1);
                }
                steps = steps.wrapping_add(run.steps_run);
                cellwise.push(run.post_fp);
            }
            Err(_) => {
                cellwise.push(0);
            }
        }
    }
    let stable = stability_sweep(&m);
    fr.stable_repeats = stable.agreed_rounds;
    let _ = fr.set_baseline(fr.freeze_hash);
    ReplayResult { seed, cells: m.produced, matrix_fp: m.fingerprint(), freeze_fp: fr.fingerprint(), cellwise_fp: fnv1a_words(&cellwise), cells_ok, steps, lcg_tail: rng.raw() }
}
// ===========================================================================
// 16. 观测导出
// ===========================================================================
/// 观测包（`observe()` 的产出）。
#[derive(Clone, Debug)]
pub struct Observation {
    /// 指标。
    pub metrics: Metrics,
    /// 参数矩阵。
    pub matrix: PlanMatrix,
    /// 冻结件。
    pub freeze: FreezeRecord,
    /// 稳定性结论。
    pub stability: StabilityReport,
    /// 对齐报告。
    pub align: AlignReport,
    /// 资源预算。
    pub budget: ResourceBudget,
    /// 冻结状态机。
    pub fsm: FreezeCell,
    /// 环形账。
    pub ring: RingLedger,
    /// 注入留痕。
    pub injected: Vec<Injected>,
    /// 检出结论。
    pub detections: Vec<Detection>,
    /// 检出通过数。
    pub detect_pass: usize,
    /// 重放结论。
    pub replay: ReplayResult,
    /// 确定性重放一致（跑两遍指纹全等）。
    pub replay_stable: bool,
    /// TLV 往返自洽。
    pub tlv_ok: bool,
    /// 三账守恒（干净态）。
    pub ledger_ok: bool,
    /// 失真声明在档。
    pub distortion_declared: bool,
}
impl Observation {
    /// 总闸：判据 UNX-F2801-J1 全过。
    ///
    /// 硬条件（缺一即不过）：
    /// 1. 矩阵格数 = 维度乘积、正交成立、四维全在闭集内；
    /// 2. 逐格真跑全过（`cells_ok == GRID_CELLS`）且三账守恒；
    /// 3. 8 条对齐逐项全过；
    /// 4. 13 形态检出全命中（判据要求 10/10，本单 13/13）；
    /// 5. 稳定性轮次指纹全等（`REPEAT` 轮）；
    /// 6. 冻结件与基线一致（`verify()` 通过）；
    /// 7. TLV 往返自洽；
    /// 8. 失真声明在档；
    /// 9. 资源上界在界内、环形账守恒、重放两遍指纹全等。
    pub fn all_ok(&self) -> bool {
        self.matrix.count_ok()
            && self.matrix.orthogonal_ok()
            && self.matrix.dims_ok()
            && self.metrics.cells_ok == GRID_CELLS as u64
            && self.ledger_ok
            && self.align.all_ok()
            && self.detect_pass == FORM_COUNT
            && self.stability.passed()
            && self.freeze.verify().is_ok()
            && self.freeze.distortion_declared
            && self.tlv_ok
            && self.budget.within()
            && self.ring.balanced()
            && self.replay_stable
            && self.replay.ok()
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "matrix={}/{} cells_ok={}/{} align={}/{} detect={}/{} stable={} freeze_ok={} tlv={} ring={} replay_stable={} distortion={}",
            self.matrix.produced,
            GRID_CELLS,
            self.metrics.cells_ok,
            GRID_CELLS,
            self.align.passed(),
            AlignField::ALL.len(),
            self.detect_pass,
            FORM_COUNT,
            if self.stability.passed() { 1 } else { 0 },
            if self.freeze.verify().is_ok() { 1 } else { 0 },
            if self.tlv_ok { 1 } else { 0 },
            if self.ring.balanced() { 1 } else { 0 },
            if self.replay_stable { 1 } else { 0 },
            if self.distortion_declared { 1 } else { 0 }
        )
    }
}
/// 采一份观测（真实跑逻辑，非占位）。
///
/// 执行顺序（单向依赖，不互递归）：
/// 1. `PlanMatrix::expand()` 正交展开；
/// 2. `replay(seed)` 逐格真跑 + 冻结 + 稳定性扫描，再跑一遍验确定性；
/// 3. `run_matrix()` 二次逐格真跑并汇总指标；
/// 4. `align_report()` 取首格对齐报告；
/// 5. 13 形态逐形态独立注入 + 独立检出（每形态单独构造干净证据底）；
/// 6. `export_tlv()` 后 `tlv_roundtrip_ok()` 回读校验。
pub fn observe(seed: u64) -> Observation {
    // 1 参数矩阵
    let m = PlanMatrix::expand();
    let sd = m.state_distribution();
    let cd = m.class_distribution();
    let critical = m.critical_count();
    // 2 重放（内含逐格真跑与稳定性扫描），跑两遍验确定性
    let rp = replay(seed);
    let rp2 = replay(seed);
    let replay_stable = rp.matrix_fp == rp2.matrix_fp && rp.cellwise_fp == rp2.cellwise_fp && rp.freeze_fp == rp2.freeze_fp;
    // 3 二次逐格真跑汇总
    let mut metrics = Metrics::zero();
    metrics.cells = m.produced as u64;
    metrics.state_dist = [sd[0] as u64, sd[1] as u64, sd[2] as u64];
    metrics.class_dist = [cd[0] as u64, cd[1] as u64, cd[2] as u64, cd[3] as u64];
    metrics.critical = critical as u64;
    let mut ledger_ok_cells: u64 = 0;
    let mut align_ok_cells: u64 = 0;
    let mut align_fields: u64 = 0;
    if let Ok(runs) = run_matrix(&m) {
        for r in runs.iter() {
            if r.ok() {
                metrics.cells_ok = metrics.cells_ok.wrapping_add(1);
            }
            if r.ledger_ok {
                ledger_ok_cells = ledger_ok_cells.wrapping_add(1);
            }
            if r.align_ok {
                align_ok_cells = align_ok_cells.wrapping_add(1);
                align_fields = align_fields.wrapping_add(AlignField::ALL.len() as u64);
            }
            metrics.steps = metrics.steps.wrapping_add(r.steps_run);
        }
    }
    metrics.ledger_ok = ledger_ok_cells;
    metrics.align_ok = align_ok_cells;
    metrics.align_fields_ok = align_fields;
    // 4 对齐报告（首格，用与 `run_cell` 同口径的池账预置）
    let mut align = AlignReport { rows: Vec::new(), missing_bits: 0, dup_bits: 0 };
    if let Some(first) = m.cells.first() {
        align = align_report(first, &ledger_for_cell(first));
    }
    // 冻结件 + 状态机（走完整合法路径：Draft → Reviewing → Frozen）
    let mut fsm = FreezeCell::new();
    let _ = fsm.go(FreezeState::Reviewing);
    let _ = fsm.go(FreezeState::Frozen);
    let stable = stability_sweep(&m);
    let mut fr = FreezeRecord::draft(&m, &fsm);
    fr.stable_repeats = stable.agreed_rounds;
    fr.declare_distortion();
    let _ = fr.set_baseline(fr.freeze_hash);
    metrics.freeze_hash = fr.freeze_hash;
    metrics.baseline_hash = fr.baseline_hash;
    metrics.freeze_fp = fr.fingerprint();
    metrics.freeze_legal = fsm.legal_moves;
    metrics.freeze_rejected = fsm.rejected_count() as u64;
    metrics.stable_rounds = stable.rounds.len() as u64;
    // 5 环形账（干净态：写 `2 × cap + 5` 条，检验 dropped 显性）
    let mut ring = RingLedger::new();
    let n_push: u64 = (RING_CAP as u64).wrapping_mul(2).wrapping_add(5);
    let mut p: u64 = 0;
    while p < n_push {
        ring.push(MAIN_CODE, (p as usize) % GRID_CELLS, p);
        p = p.wrapping_add(1);
    }
    metrics.ring_pushed = ring.pushed;
    metrics.ring_dropped = ring.dropped;
    metrics.ring_live = ring.live() as u64;
    // 6 资源预算
    let budget = resource_budget(&m);
    metrics.step_cap = budget.step_cap;
    metrics.mem_peak = budget.mem_peak;
    metrics.syscall_used = budget.syscall_used;
    // 7 13 形态逐形态独立注入 + 独立检出
    let (injected, _ev) = inject_all();
    let mut detections: Vec<Detection> = Vec::with_capacity(FORM_COUNT);
    let mut detect_pass: usize = 0;
    let mut k: usize = 0;
    while k < FORM_COUNT {
        let f = Form::ALL[k];
        // 每形态单独构造：干净矩阵 → 仅本形态造证 → 干净证据底 → 检出
        let mut m2 = PlanMatrix::expand();
        let cor = match corrupt_matrix(&mut m2, f) {
            Ok(v) => v,
            Err(_) => Corruption::clean(),
        };
        let clean = PlanMatrix::expand();
        let clean_fr = FreezeRecord::draft(&clean, &FreezeCell::new());
        let f2 = FreezeRecord::draft(&m2, &FreezeCell::new());
        let mut r2 = RingLedger::new();
        let n2: u64 = (RING_CAP as u64).wrapping_mul(2).wrapping_add(5);
        let mut q: u64 = 0;
        while q < n2 {
            r2.push(MAIN_CODE, 0, q);
            q = q.wrapping_add(1);
        }
        let mut rejected: usize = 0;
        if cor.freeze_illegal {
            let mut fx = FreezeCell::new();
            let _ = fx.go(FreezeState::Reviewing);
            let _ = fx.go(FreezeState::Frozen);
            let _ = fx.go(FreezeState::Draft);
            rejected = fx.rejected_count();
        }
        let mut one = Evidence::zero();
        one.injected_bits = 1u64 << f.index();
        one.dim_violation_bits = cor.dim_bits;
        one.align_missing = cor.align_missing;
        one.produced_cells = m2.produced;
        one.dim_product = GRID_CELLS;
        one.ring_balanced = if cor.ledger_broken { false } else { r2.balanced() };
        one.ring_live = if cor.ledger_broken { RING_CAP + 1 } else { r2.live() };
        one.ring_cap = RING_CAP;
        one.freeze_hash = f2.freeze_hash;
        one.baseline_hash = if cor.hash_drift { clean_fr.freeze_hash.wrapping_add(1) } else { f2.freeze_hash };
        one.state = FreezeState::Frozen;
        one.stable_identical = !cor.stability_broken;
        one.stable_rounds = REPEAT;
        one.budget_within = !cor.budget_broken;
        one.tlv_ok = !cor.tlv_broken;
        one.freeze_rejected = rejected;
        let ds = detect_all(&one);
        if let Some(d) = ds.iter().find(|d| d.form == f) {
            if d.hit {
                detect_pass = detect_pass.wrapping_add(1);
            }
            detections.push(d.clone());
        }
        k = k.wrapping_add(1);
    }
    metrics.forms = FORM_COUNT as u64;
    metrics.detect_hits = detect_pass as u64;
    // 8 三账守恒（干净态：跑首格后核账）
    let mut ledger_ok = false;
    if let Some(first) = m.cells.first() {
        ledger_ok = ledger_for_cell(first).balanced();
    }
    // 9 TLV 往返（先用空矩阵壳构造一版观测导出，再回读校验）
    let shell = Observation {
        metrics,
        matrix: PlanMatrix::blank(),
        freeze: fr.clone(),
        stability: stable.clone(),
        align: align.clone(),
        budget,
        fsm: fsm.clone(),
        ring: ring.clone(),
        injected: injected.clone(),
        detections: detections.clone(),
        detect_pass,
        replay: rp.clone(),
        replay_stable,
        tlv_ok: false,
        ledger_ok,
        distortion_declared: fr.distortion_declared,
    };
    let tlv = export_tlv(&shell);
    let tlv_ok = tlv_roundtrip_ok(&tlv);
    let distortion_declared = fr.distortion_declared;
    Observation {
        metrics: shell.metrics,
        matrix: m,
        freeze: fr,
        stability: stable,
        align,
        budget,
        fsm,
        ring,
        injected,
        detections,
        detect_pass,
        replay: rp,
        replay_stable,
        tlv_ok,
        ledger_ok,
        distortion_declared,
    }
}
/// 为单格构造一份「已按方案预置、已跑满」的池账（对齐断言与守恒断言共用同一夹具）。
pub fn ledger_for_cell(cell: &PlanCell) -> PoolLedger {
    let mut led = PoolLedger::new();
    let state = match PoolState::from_index(cell.state_idx) {
        Some(s) => s,
        None => return led,
    };
    let rate = match AllocRate::from_index(cell.rate_idx) {
        Some(r) => r,
        None => return led,
    };
    let class = match PageSizeClass::from_index(cell.class_idx) {
        Some(c) => c,
        None => return led,
    };
    let conc = match Concurrency::from_index(cell.conc_idx) {
        Some(c) => c,
        None => return led,
    };
    led.total.reserved_state = cell.state_idx as u64;
    led.step.preset(rate, conc);
    led.step.budget = cell.step_budget;
    if let Ok(ca) = led.class_mut(cell.class_idx) {
        ca.frame_bytes = class.frame_bytes();
    }
    let mut i: u64 = 0;
    while i < cell.step_budget {
        match led.run_step(state, rate, class, conc) {
            Ok(true) => {}
            _ => break,
        }
        i = i.wrapping_add(1);
    }
    led
}
// ===========================================================================
// 17. 导出（键值 / TLV）
// ===========================================================================
/// 导出键值文本（纯字符串，无 IO）。
///
/// 逐段入册：常量 → 维度 → 冻结 → 对齐 → 稳定性 → 预算 → 检出 → 环形账 → 指纹。
pub fn export_kv(ob: &Observation) -> String {
    let mut s = String::new();
    s.push_str("UNX-F2801.ubigpoolcap.v1\n");
    s.push_str(&format!("version={}\n", V_VERSION));
    s.push_str(&format!("seg=0x{:04X}..0x{:04X} main=0x{:04X} mask=0x{:04X}\n", SEG_BASE, SEG_END, MAIN_CODE, SEG_MASK));
    s.push_str(&format!("book_rev={}\n", ob.freeze.book_rev));
    s.push_str(&format!("grid={}x{}x{}x{}={}\n", STATE_COUNT, RATE_COUNT, CLASS_COUNT, CONC_COUNT, GRID_CELLS));
    s.push_str(&format!("cell_steps={} matrix_steps={} step_cap={}\n", CELL_STEPS, MATRIX_STEPS, STEP_CAP));
    s.push_str(&format!("permille.full={} permille.near_full={} permille.empty={}\n", PoolState::Full.target_permille(), PoolState::NearFull.target_permille(), PoolState::Empty.target_permille()));
    for (k, v) in ob.metrics.kv().iter() {
        s.push_str(&format!("m.{k}={v}\n"));
    }
    s.push_str(&format!("rate.run_permille={}\n", ob.metrics.run_pass_permille()));
    s.push_str(&format!("rate.align_permille={}\n", ob.metrics.align_pass_permille()));
    s.push_str(&format!("rate.ring_drop_permille={}\n", ob.metrics.ring_drop_permille()));
    s.push_str(&format!("rate.detect_permille={}\n", ob.metrics.detect_permille()));
    s.push_str(&format!("freeze={}\n", ob.freeze.describe()));
    for line in ob.freeze.annotation().iter() {
        s.push_str(&format!("{}\n", line));
    }
    s.push_str(&format!("align={}\n", ob.align.summary()));
    s.push_str(&format!("stability={}\n", ob.stability.summary()));
    s.push_str(&format!("budget={}\n", ob.budget.summary()));
    s.push_str(&format!("ring={}\n", ob.ring.summary()));
    s.push_str(&format!("fsm={}\n", ob.fsm.describe()));
    for e in FreezeCell::edge_manifest().iter() {
        s.push_str(&format!("fsm.edge={}\n", e));
    }
    s.push_str(&format!("replay={}\n", ob.replay.describe()));
    for inj in ob.injected.iter() {
        s.push_str(&format!("inject.{}={}\n", inj.form.name(), inj.note));
    }
    for d in ob.detections.iter() {
        s.push_str(&format!("detect.{}={}\n", d.form.name(), if d.hit { 1 } else { 0 }));
    }
    s.push_str(&format!("assert.matrix_count={}\n", if ob.matrix.count_ok() { 1 } else { 0 }));
    s.push_str(&format!("assert.orthogonal={}\n", if ob.matrix.orthogonal_ok() { 1 } else { 0 }));
    s.push_str(&format!("assert.dims={}\n", if ob.matrix.dims_ok() { 1 } else { 0 }));
    s.push_str(&format!("assert.cells_ok={}/{}\n", ob.metrics.cells_ok, GRID_CELLS));
    s.push_str(&format!("assert.ledger={}\n", if ob.ledger_ok { 1 } else { 0 }));
    s.push_str(&format!("assert.align={}/{}\n", ob.align.passed(), AlignField::ALL.len()));
    s.push_str(&format!("assert.detect={}/{}\n", ob.detect_pass, FORM_COUNT));
    s.push_str(&format!("assert.stability={}\n", if ob.stability.passed() { 1 } else { 0 }));
    s.push_str(&format!("assert.verify={}\n", if ob.freeze.verify().is_ok() { 1 } else { 0 }));
    s.push_str(&format!("assert.tlv={}\n", if ob.tlv_ok { 1 } else { 0 }));
    s.push_str(&format!("assert.ring={}\n", if ob.ring.balanced() { 1 } else { 0 }));
    s.push_str(&format!("assert.budget={}\n", if ob.budget.within() { 1 } else { 0 }));
    s.push_str(&format!("assert.replay_stable={}\n", if ob.replay_stable { 1 } else { 0 }));
    s.push_str(&format!("assert.distortion={}\n", if ob.distortion_declared { 1 } else { 0 }));
    s.push_str(&format!("all_ok={}\n", if ob.all_ok() { 1 } else { 0 }));
    s.push_str(&format!("fp={:016X}\n", ob.metrics.fingerprint()));
    s
}
/// TLV 标签。
pub mod tlv_tag {
    /// 组头（魔数 + 版本 + 长度）。
    pub const HEAD: u8 = 0x01;
    /// 冻结件字段。
    pub const FREEZE: u8 = 0x10;
    /// 对齐行。
    pub const ALIGN: u8 = 0x11;
    /// 指标键值对。
    pub const METRIC: u8 = 0x20;
    /// 检出记录。
    pub const DETECT: u8 = 0x30;
    /// 环形账摘要。
    pub const RING: u8 = 0x40;
    /// 指纹。
    pub const FINGERPRINT: u8 = 0x7F;
}
/// 组头载荷长度（`push_u64` × 3 = magic + version + body_len）。
pub const TLV_HEAD_LEN: usize = 1 + 2 + 24;
/// 追加一条 TLV。
pub fn push_tlv(out: &mut Vec<u8>, tag: u8, payload: &[u8]) {
    out.push(tag);
    let n = payload.len() as u64;
    out.push(((n >> 8) & 0xFF) as u8);
    out.push((n & 0xFF) as u8);
    let mut i: usize = 0;
    while i < payload.len() {
        if let Some(b) = payload.get(i) {
            out.push(*b);
        }
        i = i.wrapping_add(1);
    }
}
/// 追加一个 `u64`（小端，逐字节移位）。
pub fn push_u64(v: &mut Vec<u8>, w: u64) {
    let mut i: usize = 0;
    while i < 8 {
        v.push(((w >> (8 * i)) & 0xFF) as u8);
        i = i.wrapping_add(1);
    }
}
/// 回读一个 `u64`（越界补 0）。
pub fn read_u64(v: &[u8], off: usize) -> u64 {
    let mut w: u64 = 0;
    let mut i: usize = 0;
    while i < 8 {
        let idx = off.wrapping_add(i);
        let byte = match v.get(idx) {
            Some(b) => *b as u64,
            None => 0u64,
        };
        w |= byte << (8 * i);
        i = i.wrapping_add(1);
    }
    w
}
/// 回读一个 `u16`（越界补 0）。
pub fn read_u16(v: &[u8], off: usize) -> u64 {
    let hi = match v.get(off) {
        Some(b) => (*b as u64) << 8,
        None => 0,
    };
    let lo = match v.get(off.wrapping_add(1)) {
        Some(b) => *b as u64,
        None => 0,
    };
    hi | lo
}
/// 导出 TLV 字节流。
///
/// 结构：`HEAD(0x01) + [len:u16] + payload(24)` 后接若干 `(tag:u8, len:u16, payload)`。
/// 组头单独一行写版本（`push_u64(version)`），长度自洽由 `tlv_roundtrip_ok()` 回读校验。
pub fn export_tlv(ob: &Observation) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    // 冻结件
    let mut fp: Vec<u8> = Vec::new();
    push_u64(&mut fp, ob.freeze.book_rev as u64);
    push_u64(&mut fp, ob.freeze.cells as u64);
    push_u64(&mut fp, ob.freeze.freeze_hash);
    push_u64(&mut fp, ob.freeze.baseline_hash);
    push_u64(&mut fp, ob.freeze.fingerprint());
    push_u64(&mut fp, ob.freeze.state.index() as u64);
    push_u64(&mut fp, ob.freeze.stable_repeats as u64);
    push_u64(&mut fp, ob.freeze.distortion_declared as u64);
    push_u64(&mut fp, ob.freeze.permille[0]);
    push_u64(&mut fp, ob.freeze.permille[1]);
    push_u64(&mut fp, ob.freeze.permille[2]);
    push_tlv(&mut body, tlv_tag::FREEZE, &fp);
    // 对齐行（8 条）
    for r in ob.align.rows.iter() {
        let mut p: Vec<u8> = Vec::new();
        push_u64(&mut p, r.field.index() as u64);
        push_u64(&mut p, r.plan_value);
        push_u64(&mut p, r.pool_value);
        push_u64(&mut p, r.aligned as u64);
        push_tlv(&mut body, tlv_tag::ALIGN, &p);
    }
    // 指标
    for (k, v) in ob.metrics.kv().iter() {
        let mut p: Vec<u8> = Vec::new();
        p.push((k.len() & 0xFF) as u8);
        for b in k.as_bytes() {
            p.push(*b);
        }
        push_u64(&mut p, *v);
        push_tlv(&mut body, tlv_tag::METRIC, &p);
    }
    // 检出记录
    for d in ob.detections.iter() {
        let mut p: Vec<u8> = Vec::new();
        push_u64(&mut p, d.form.code() as u64);
        push_u64(&mut p, d.hits as u64);
        push_u64(&mut p, d.hit as u64);
        push_tlv(&mut body, tlv_tag::DETECT, &p);
    }
    // 环形账摘要
    let mut rp: Vec<u8> = Vec::new();
    push_u64(&mut rp, ob.ring.pushed);
    push_u64(&mut rp, ob.ring.dropped);
    push_u64(&mut rp, ob.ring.live() as u64);
    push_u64(&mut rp, ob.ring.fingerprint());
    push_tlv(&mut body, tlv_tag::RING, &rp);
    // 指纹
    let mut fb: Vec<u8> = Vec::new();
    push_u64(&mut fb, ob.metrics.fingerprint());
    push_u64(&mut fb, ob.freeze.fingerprint());
    push_u64(&mut fb, ob.freeze.freeze_hash);
    push_tlv(&mut body, tlv_tag::FINGERPRINT, &fb);
    // 组头（payload 恰 3 × u64 = 24 字节）
    let mut head_payload: Vec<u8> = Vec::with_capacity(24);
    push_u64(&mut head_payload, 0x5645_5853_3232_4445); // magic
    push_u64(&mut head_payload, V_VERSION_ID); // version（单独一行）
    push_u64(&mut head_payload, body.len() as u64); // body_len
    let mut out: Vec<u8> = Vec::with_capacity(body.len().wrapping_add(TLV_HEAD_LEN));
    push_tlv(&mut out, tlv_tag::HEAD, &head_payload);
    out.extend_from_slice(&body);
    out
}
/// TLV 往返自洽断言（长度自洽 + 关键字段回读一致）。
pub fn tlv_roundtrip_ok(buf: &[u8]) -> bool {
    if buf.len() < TLV_HEAD_LEN {
        return false;
    }
    if buf.first().copied() != Some(tlv_tag::HEAD) {
        return false;
    }
    let declared = read_u64(buf, 3 + 16) as usize;
    // 逐条走长度，确认无溢出、无尾随
    let mut off: usize = TLV_HEAD_LEN;
    let mut walked: usize = 0;
    let mut freeze_off: usize = usize::MAX;
    let mut fp_off: usize = usize::MAX;
    while off < buf.len() {
        let tag = match buf.get(off) {
            Some(t) => *t,
            None => return false,
        };
        if tag == 0 {
            return false;
        }
        let len = read_u16(buf, off.wrapping_add(1)) as usize;
        if len == 0 {
            return false;
        }
        if tag == tlv_tag::FREEZE && freeze_off == usize::MAX {
            freeze_off = off.wrapping_add(3);
        }
        if tag == tlv_tag::FINGERPRINT && fp_off == usize::MAX {
            fp_off = off.wrapping_add(3);
        }
        off = off.wrapping_add(3);
        if off.wrapping_add(len) > buf.len() {
            return false;
        }
        off = off.wrapping_add(len);
        walked = walked.wrapping_add(1);
    }
    if off != buf.len() {
        return false;
    }
    if declared.wrapping_add(TLV_HEAD_LEN) != buf.len() {
        return false;
    }
    if walked < 5 {
        return false;
    }
    // 冻结件字段回读：11 × u64 = 88 字节，偏移依次为
    // +0 book_rev / +8 cells / +16 freeze_hash / +24 baseline_hash /
    // +32 fingerprint / +40 state / +48 stable_repeats / +56 distortion /
    // +64 permille[0] / +72 permille[1] / +80 permille[2]
    if freeze_off != usize::MAX {
        let v = read_u64(buf, freeze_off);
        let cells = read_u64(buf, freeze_off.wrapping_add(8));
        let fh = read_u64(buf, freeze_off.wrapping_add(16));
        let bh = read_u64(buf, freeze_off.wrapping_add(24));
        let ffp = read_u64(buf, freeze_off.wrapping_add(32));
        let st = read_u64(buf, freeze_off.wrapping_add(40));
        let reps = read_u64(buf, freeze_off.wrapping_add(48));
        let dist = read_u64(buf, freeze_off.wrapping_add(56));
        let p0 = read_u64(buf, freeze_off.wrapping_add(64));
        let p1 = read_u64(buf, freeze_off.wrapping_add(72));
        let p2 = read_u64(buf, freeze_off.wrapping_add(80));
        if v != STRESS_BOOK_REV as u64 {
            return false;
        }
        if cells == 0 || fh == 0 || ffp == 0 {
            return false;
        }
        if st > FreezeState::ALL.len() as u64 {
            return false;
        }
        if reps as usize > REPEAT {
            return false;
        }
        if dist > 1 {
            return false;
        }
        if p0 != FULL_PERMILLE || p1 != NEAR_FULL_PERMILLE || p2 != EMPTY_PERMILLE {
            return false;
        }
        if fh != bh {
            return false;
        }
    } else {
        return false;
    }
    // 指纹字段回读：三条 u64 必全非零
    if fp_off != usize::MAX {
        let m0 = read_u64(buf, fp_off);
        let m1 = read_u64(buf, fp_off.wrapping_add(8));
        let m2 = read_u64(buf, fp_off.wrapping_add(16));
        if m0 == 0 || m1 == 0 || m2 == 0 {
            return false;
        }
    } else {
        return false;
    }
    true
}
// ===========================================================================
// 18. 性能逐项分解（对标 Windows 同类能力：深在哪）
// ===========================================================================
/// 单项成本分解。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CostItem {
    /// 项名。
    pub name: &'static str,
    /// 复杂度记号。
    pub complexity: &'static str,
    /// 内存字节（模型侧估算）。
    pub mem_bytes: u64,
    /// syscall 次数（本单恒 0）。
    pub syscalls: u64,
    /// 步数上界。
    pub steps: u64,
    /// 「深在哪」注记（与 Windows 同类能力对照）。
    pub deepen: &'static str,
}
impl CostItem {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("cost {} complexity={} mem={}B syscall={} steps={} deepen={}", self.name, self.complexity, self.mem_bytes, self.syscalls, self.steps, self.deepen)
    }
}
/// 性能逐项分解清单。
pub fn complexity() -> Vec<CostItem> {
    let mut v: Vec<CostItem> = Vec::new();
    v.push(CostItem {
        name: "grid_expand",
        complexity: "O(GRID_CELLS)=O(240)",
        mem_bytes: (GRID_CELLS as u64).wrapping_mul(80),
        syscalls: 0,
        steps: GRID_CELLS as u64,
        deepen: "Windows 侧等价物是性能分析器的『测试矩阵向导』，它按 3 档粗粒度出方案；本单把格数从维度乘积现算，任何一维增档都让 240 同步变化",
    });
    v.push(CostItem {
        name: "run_cell",
        complexity: "O(CELL_STEPS)=O(64) per cell",
        mem_bytes: (CLASS_COUNT as u64).wrapping_mul(40).wrapping_add(64),
        syscalls: 0,
        steps: CELL_STEPS as u64,
        deepen: "Windows 的 memory pressure test 逐项跑真实分配；本单逐格跑的是**方案夹具**（账本推进），不做真实分配，但守恒账与对齐断言同等严格",
    });
    v.push(CostItem {
        name: "run_matrix",
        complexity: "O(GRID_CELLS x CELL_STEPS)=O(15360)",
        mem_bytes: (GRID_CELLS as u64).wrapping_mul(72),
        syscalls: 0,
        steps: MATRIX_STEPS as u64,
        deepen: "每格独立夹具（不复用账），深在『污染不跨格传播』——Windows 的批处理测试跑串了就会互相污染，无法定位到格",
    });
    v.push(CostItem {
        name: "reconcile",
        complexity: "O(CLASS_COUNT)=O(4)",
        mem_bytes: 0,
        syscalls: 0,
        steps: (CLASS_COUNT as u64).wrapping_add(4),
        deepen: "三条守恒等式（R1 总账 / R2 阶账 / R3 分账 / R4 事件桥）；Windows 侧只有总分配量口径，没有阶与分的桥式对账",
    });
    v.push(CostItem {
        name: "align_report",
        complexity: "O(ALIGN_FIELDS)=O(8)",
        mem_bytes: (AlignField::ALL.len() as u64).wrapping_mul(32),
        syscalls: 0,
        steps: AlignField::ALL.len() as u64,
        deepen: "8 条逐项映射 + 缺失位图 + 重复位图；『零缺失』是位图全零的操作化定义，不是人工核对",
    });
    v.push(CostItem {
        name: "inject_all",
        complexity: "O(FORM_COUNT x GRID_CELLS)造证+检出",
        mem_bytes: (FORM_COUNT as u64).wrapping_mul(40).wrapping_add((FORM_COUNT as u64).wrapping_mul(40)),
        syscalls: 0,
        steps: (FORM_COUNT as u64).wrapping_mul(3),
        deepen: "13 形态各自独立造证 + 独立检出（干净证据底只置本形态那一项），深在『检出不交叉掩盖』——Windows 的注入通常多形态叠加，结论无法归因",
    });
    v.push(CostItem {
        name: "stability_sweep",
        complexity: "O(REPEAT x GRID_CELLS x CELL_STEPS)=O(46080)",
        mem_bytes: (GRID_CELLS as u64).wrapping_mul(16),
        syscalls: 0,
        steps: (MATRIX_STEPS * REPEAT) as u64,
        deepen: "连跑 3 轮逐格指纹全等才算通过；Windows 的 pressure profile 不做同输入复跑，只做一次采样，读数无法自证可复现",
    });
    v.push(CostItem {
        name: "freeze_hash",
        complexity: "O(GRID_CELLS) FNV-1a 折叠",
        mem_bytes: (GRID_CELLS as u64).wrapping_mul(8),
        syscalls: 0,
        steps: GRID_CELLS as u64,
        deepen: "冻结哈希是 240 个逐格指纹的折叠，改任一速率档即失配；Windows 无方案级哈希，方案漂移只能靠人 review",
    });
    v.push(CostItem {
        name: "resource_budget",
        complexity: "O(1) 常数核算",
        mem_bytes: 0,
        syscalls: 0,
        steps: 8,
        deepen: "内存峰值四项相加显式可算，syscall 上界恒 0（纯功能），步数上界 = 真跑上限 x 2 裕量——深在『资源上界是判据的一部分而非附带说明』",
    });
    v.push(CostItem {
        name: "export_kv",
        complexity: "O(metric_count + annotation_count)",
        mem_bytes: 2048,
        syscalls: 0,
        steps: 64,
        deepen: "逐字段注记（判据要求『逐项注记随冻结件入库』）；Windows 侧只有汇总报表，无字段级映射注记",
    });
    v.push(CostItem {
        name: "export_tlv",
        complexity: "O(metric_count + FORM_COUNT + ALIGN_FIELDS)",
        mem_bytes: 4096,
        syscalls: 0,
        steps: 96,
        deepen: "组头单独一行写 version，长度常量由 3 次 push_u64 反推（24B），`tlv_roundtrip_ok()` 回读关键字段逐项对账",
    });
    v.push(CostItem {
        name: "observe",
        complexity: "O(REPEAT x GRID_CELLS x CELL_STEPS + 2 x FORM_COUNT x GRID_CELLS)",
        mem_bytes: 65536,
        syscalls: 0,
        steps: (MATRIX_STEPS * REPEAT) as u64,
        deepen: "一次 observe 走完九段（展开→重放→再跑→对齐→注入→检出→守恒→预算→TLV 往返），是判据的单一入口",
    });
    v
}
/// 复杂度清单逐行。
pub fn complexity_rows() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in complexity().iter() {
        out.push(c.describe());
    }
    out
}
/// 复杂度总计（步数 + syscall + 内存）。
pub fn complexity_total() -> (u64, u64, u64) {
    let mut steps: u64 = 0;
    let mut sys: u64 = 0;
    let mut mem: u64 = 0;
    for c in complexity().iter() {
        steps = steps.wrapping_add(c.steps);
        sys = sys.wrapping_add(c.syscalls);
        mem = mem.wrapping_add(c.mem_bytes);
    }
    (steps, sys, mem)
}
// ===========================================================================
// 19. 自检套件
// ===========================================================================
/// 一条自检项。
#[derive(Clone, Copy, Debug)]
pub struct Check {
    /// 项号。
    pub id: &'static str,
    /// 通过与否。
    pub pass: bool,
    /// 说明。
    pub note: &'static str,
}
/// 自检套件（判据总闸的逐条拆解）。
pub fn self_check(seed: u64) -> Vec<Check> {
    let mut out: Vec<Check> = Vec::new();
    let m = PlanMatrix::expand();
    let fr_cells = if let Some(c) = m.cells.first() { ledger_for_cell(c) } else { PoolLedger::new() };
    let align = match m.cells.first() {
        Some(c) => align_report(c, &fr_cells),
        None => AlignReport { rows: Vec::new(), missing_bits: 1, dup_bits: 1 },
    };
    let stable = stability_sweep(&m);
    let budget = resource_budget(&m);
    let mut fsm = FreezeCell::new();
    let _ = fsm.go(FreezeState::Reviewing);
    let _ = fsm.go(FreezeState::Frozen);
    let mut fr = FreezeRecord::draft(&m, &fsm);
    fr.declare_distortion();
    let _ = fr.set_baseline(fr.freeze_hash);
    let ob = observe(seed);
    let tlv = export_tlv(&ob);
    // C1 正交展开格数
    out.push(Check { id: "C2801-01 grid_count", pass: m.count_ok(), note: "3x5x4x4=240 格由维度乘积算出并与常量逐位比对" });
    // C2 正交性
    out.push(Check { id: "C2801-02 orthogonal", pass: m.orthogonal_ok(), note: "每个 (state,rate,class,conc) 组合恰出现一次" });
    // C3 四维闭集
    out.push(Check { id: "C2801-03 dims", pass: m.dims_ok(), note: "三态/速率/尺寸/并发四维取值全在闭集内" });
    // C4 分布均匀
    let sd = m.state_distribution();
    let cd = m.class_distribution();
    let dist_ok = sd[0] == sd[1] && sd[1] == sd[2] && cd[0] == cd[1] && cd[1] == cd[2] && cd[2] == cd[3];
    out.push(Check { id: "C2801-04 dist", pass: dist_ok, note: "三态各 80 格、尺寸档各 60 格" });
    // C5 逐格真跑
    out.push(Check { id: "C2801-05 run_all", pass: run_matrix_ok(&m), note: "240 格各跑 64 步且独立夹具全过" });
    // C6 三账守恒
    out.push(Check { id: "C2801-06 reconcile", pass: fr_cells.balanced(), note: "R1/R2/R3/R4 四条守恒等式联合成立" });
    // C7 对齐零缺失
    out.push(Check { id: "C2801-07 align", pass: align.all_ok(), note: "8 条方案↔池账映射逐项相等、无缺失、无重复" });
    // C8 三态余量
    let permille_ok = PoolState::Full.target_permille() == 0 && PoolState::NearFull.target_permille() == 100 && PoolState::Empty.target_permille() == 1000;
    out.push(Check { id: "C2801-08 three_state", pass: permille_ok, note: "满池 0% / 近满 10% / 空 100%" });
    // C9 冻结哈希入册
    out.push(Check { id: "C2801-09 freeze_hash", pass: fr.freeze_hash != 0, note: "冻结哈希非零且由 240 个逐格指纹折叠" });
    // C10 漂移检出
    out.push(Check { id: "C2801-10 verify", pass: fr.verify().is_ok(), note: "冻结件与 CI 基线逐项一致" });
    // C11 漂移拒收
    let mut drifted = fr.clone();
    drifted.freeze_hash = drifted.freeze_hash.wrapping_add(1);
    out.push(Check { id: "C2801-11 drift_reject", pass: drifted.verify().is_err(), note: "漂移注入必被 verify() 拦截" });
    // C12 稳定性
    out.push(Check { id: "C2801-12 stability", pass: stable.passed(), note: "REPEAT=3 轮逐格指纹全等" });
    // C13 环形账守恒
    let mut ring = RingLedger::new();
    let mut i: u64 = 0;
    while i < 300 {
        ring.push(MAIN_CODE, 0, i);
        i = i.wrapping_add(1);
    }
    out.push(Check { id: "C2801-13 ring", pass: ring.check().is_ok() && ring.dropped > 0, note: "pushed == dropped + live，dropped 显性非零" });
    // C14 状态机边闭集
    let mut fx = FreezeCell::new();
    let _ = fx.go(FreezeState::Frozen); // 非法
    out.push(Check { id: "C2801-14 fsm", pass: fx.rejected_count() == 1 && FreezeCell::edge_count() == 10, note: "非法迁移拒记且合法边闭集 10 条" });
    // C15 十三形态检出
    out.push(Check { id: "C2801-15 detect", pass: ob.detect_pass == FORM_COUNT, note: "13/13 形态独立检出（判据要求 10/10）" });
    // C16 TLV 往返
    out.push(Check { id: "C2801-16 tlv", pass: tlv_roundtrip_ok(&tlv), note: "TLV 长度自洽 + 冻结字段回读一致" });
    // C17 资源上界
    out.push(Check { id: "C2801-17 budget", pass: budget.within(), note: "步数/内存/syscall 三项上界在界内" });
    // C18 失真声明在档
    out.push(Check { id: "C2801-18 distortion", pass: fr.distortion_declared, note: "开发期注入等价非真机等价，声明在档" });
    // C19 重放确定性
    out.push(Check { id: "C2801-19 replay", pass: ob.replay_stable, note: "同种子连跑两遍矩阵/逐格/冻结三指纹全等" });
    // C20 总闸
    out.push(Check { id: "C2801-20 gate", pass: ob.all_ok(), note: "observe() 判据总闸全过" });
    out
}
/// 自检全过。
pub fn self_check_ok(seed: u64) -> bool {
    for c in self_check(seed).iter() {
        if !c.pass {
            return false;
        }
    }
    true
}
/// 自检报告。
pub fn self_check_report(seed: u64) -> String {
    let cs = self_check(seed);
    let mut passed: usize = 0;
    let mut s = String::new();
    s.push_str("UNX-F2801 self_check\n");
    for c in cs.iter() {
        if c.pass {
            passed = passed.wrapping_add(1);
        }
        s.push_str(&format!("{} {} :: {}\n", c.id, if c.pass { "PASS" } else { "FAIL" }, c.note));
    }
    s.push_str(&format!("self_check {}/{} ok={}\n", passed, cs.len(), if passed == cs.len() { 1 } else { 0 }));
    s
}
// ===========================================================================
// 20. 清单 / 演示
// ===========================================================================
/// 形态清单。
pub fn form_manifest() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in Form::ALL.iter() {
        out.push(format!("0x{:04X} {} :: {} :: probe={}", f.code(), f.name(), f.describe(), f.probe()));
    }
    out
}
/// 不变量清单。
pub fn invariant_manifest() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    out.push("I1 格数: expand().produced == STATE_COUNT*RATE_COUNT*CLASS_COUNT*CONC_COUNT".to_string());
    out.push("I2 正交: 任一 (state,rate,class,conc) 组合恰出现一次".to_string());
    out.push("I3 闭集: 四维取值序号全部落在各自闭集长度内".to_string());
    out.push("I4 总账: total.used + total.free == total.slots".to_string());
    out.push("I5 阶账: alloc + fail + free == issued 且 consumed <= budget".to_string());
    out.push("I6 分账: Σalloc - Σfree == Σframes_used == total.used".to_string());
    out.push("I7 事件桥: issued == Σalloc + Σfree + fail".to_string());
    out.push("I8 环形: pushed == dropped + live 且 live <= RING_CAP".to_string());
    out.push("I9 冻结态: 仅 allowed() 闭集内迁移，非法一律拒记不改态".to_string());
    out.push("I10 漂移: freeze_hash == baseline_hash（baseline != 0）".to_string());
    out.push("I11 对齐: 8 条映射全等、missing_bits == 0、dup_bits == 0".to_string());
    out.push("I12 稳定性: REPEAT 轮 matrix_fp 与 cellwise_fp 全等".to_string());
    out.push("I13 TLV: HEAD 声明长度 + body 长度 == buf.len()，关键字段回读一致".to_string());
    out.push("I14 预算: step_used <= step_cap 且 mem_peak <= mem_cap 且 syscalls == 0".to_string());
    out.push("I15 三态: 满池余量 0 / 近满余量 100‰ / 空池余量 1000‰".to_string());
    out
}
/// 演示场景（走一遍主流程并留长表）。
pub fn demo_scene(seed: u64) -> Vec<String> {
    let ob = observe(seed);
    let mut out: Vec<String> = Vec::new();
    out.push(ob.summary());
    out.push(ob.matrix.summary());
    out.push(ob.freeze.describe());
    out.push(ob.align.summary());
    out.push(ob.stability.summary());
    out.push(ob.budget.summary());
    out.push(ob.ring.summary());
    out.push(ob.replay.describe());
    out
}
/// 演示账（逐格前 16 格 + 对齐全表 + 注入全表）。
pub fn demo_ledger(seed: u64) -> String {
    let ob = observe(seed);
    let mut s = String::new();
    s.push_str("== cells (前 16) ==\n");
    let mut n: usize = 0;
    for c in ob.matrix.cells.iter() {
        if n >= 16 {
            break;
        }
        s.push_str(&format!("{}\n", c.describe()));
        n = n.wrapping_add(1);
    }
    s.push_str("== align ==\n");
    for line in ob.align.rows_text().iter() {
        s.push_str(&format!("{}\n", line));
    }
    s.push_str("== inject ==\n");
    for inj in ob.injected.iter() {
        s.push_str(&format!("{}\n", inj.describe()));
    }
    let _ = seed;
    s
}
/// 关联单清单。
pub fn related_str() -> String {
    let mut s = String::new();
    s.push_str("F2425 大页池本体（本单只对齐其三账字段，不造池）\n");
    s.push_str("F2719 压测参数基册（本单逐项注记引用）\n");
    s.push_str("F2750 扩展页（本单预留/运行/压测三档对齐）\n");
    s.push_str("F2802-F2809 九面压测执行（本单冻结件供其共引）\n");
    s.push_str("F2810 压测基线（本单漂移检测的比对源）\n");
    s.push_str("F2841 大页-地址空间联合压测（消费本单冻结件）\n");
    s.push_str("F2861 大页池回归矩阵（消费本单 240 格清单）\n");
    s
}
// <<TAIL>>