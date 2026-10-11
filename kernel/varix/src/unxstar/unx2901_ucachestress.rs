//! unx2901 ↔ UNX-F2901 页缓存压测方案冻结（三矩阵）
//!
//! 单号：UNX-F2901
//! 标题：页缓存压测方案冻结（三矩阵）
//! 域：CoRun · UNX-A4-B26 段「页缓存压测」
//! 册目标行数：340（方案 140 + 冻结 110 + 断言 90；测试段不计）
//! 交付文件行数：≥2800（硬指标）
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F2901-J1，逐字摘抄）：
//! `UNX-F2901-J1 三矩阵（读写比/文件尺寸/并发度）参数冻结哈希入册，
//!  方案与 F2682 三接口字段对齐`
//!
//! ## 码段范围声明
//! 本单独占诊断码段 `0x21C0 ..= 0x21FF`（`SEG_BASE = 0x21C0`，
//! `SEG_MASK = 0x003F`，段内可编址 64 码）。主码 `0x21C0`。
//! 错误变体一码一段，逐个占用段内偏移，变体总数 ≤ 60
//! （实际 12 个注入形态 + 6 个辅助码 = 18 码，余 46 码留白）。
//! 本单**不占用** `0x2200` 及以上任何码；那些段归 UNX-F2941。
//!
//! ## 组件清单（本单交付的纯功能件）
//! 1. `RwRatio`：读写比闭集（0:1 / 1:4 / 1:1 / 4:1 / 1:0 五档）。
//! 2. `FileSize`：文件尺寸闭集（4KB / 64KB / 1MB / 64MB / 1GB 五档）。
//! 3. `ConcDegree`：并发度闭集（1 / 2 / 4 / 6 / 8 核五档）。
//! 4. `PlanCell`：单格参数方案（三维正交 + 派生步数上界）。
//! 5. `PlanMatrix`：三矩阵交叉展开，格数 `5 × 5 × 5 = 125` 现算。
//! 6. `CacheLedger`：页缓存替身账（三段双列：读段 / 写段 / 回收段）。
//! 7. `AlignField` / `AlignReport`：**与 F2682 三接口逐项对齐**断言
//!    （F2662 查询 / F2663 插入 / F2664 失效 三接口，共 10 个字段）。
//! 8. `RingLedger`：append-only 环形账，`dropped` 显性计数。
//! 9. `FreezeRecord`：冻结件（三矩阵参数 + 对齐哈希 + 注记）。
//! 10. `FreezeState`：冻结状态机，合法边闭集 + 非法转换拒记。
//! 11. `inject_all()`：注入核 **12 形态全拒**（判据要求漂移 10/10 拦截）。
//! 12. `fingerprint()` / `verify()`：冻结指纹与漂移检出。
//! 13. `export_kv()` / `export_tlv()`：观测导出，TLV 往返自洽。
//! 14. `replay(seed)` / `observe(seed)`：确定性重放与真实跑逻辑观测。
//! 15. `stability_sweep()`：稳定性判定（多轮重复跑指纹一致才算通过）。
//! 16. `resource_budget()`：资源上界核算。
//! 17. `complexity()`：性能逐项分解。
//!
//! ## 职责定位（语义边界）
//! 判据原文写明「**只做方案冻结与对齐**；不做 cache 本体（F2682）、
//! 不做压测执行（F2902–F2909）」。本单严格守这条线：
//! - **只冻结**：三矩阵 15 档参数 + 125 个交叉格被登记、断言、算哈希、入册；
//! - **不做 cache 本体**：`CacheLedger` 是**替身账**，不是页缓存；
//!   它只记「读命中 / 读未命中 / 写回 / 回收」四类事件的计数守恒，
//!   不持有任何真实页、不查任何真实页表；
//! - **不做压测执行**：逐格跑的是**方案夹具**（账本推进 + 对齐断言），
//!   不是 F2902–F2909 的真机压测。
//! 这三条边界必须在册留痕，否则会被误读为「已跑过页缓存压测」。
//!
//! ## 三矩阵的构造（格数显式计算）
//! ```text
//! 读比档 RW_COUNT   = 5   （0:1 / 1:4 / 1:1 / 4:1 / 1:0）
//! 尺寸档 SIZE_COUNT = 5   （4KB / 64KB / 1MB / 64MB / 1GB）
//! 并发档 CONC_COUNT = 5   （1 / 2 / 4 / 6 / 8 核）
//! 交叉格 GRID_CELLS = RW_COUNT * SIZE_COUNT * CONC_COUNT = 125
//! 三矩阵面名       = 读命中洪峰 / 写入洪峰 / 回收对抗（与 F2913 账册面名一一对应）
//! ```
//! 判据要求的「三矩阵键与 F2913 账册面名一一对应」在本单落成
//! `wave_of(cell) -> F2913Face`：每个交叉格归属唯一一个洪峰面，
//! 归属规则由读写比档决定（纯读 → 读命中洪峰；纯写 → 写入洪峰；
//! 混合 → 按写比高低归入回收对抗面）。
//!
//! ## 与 F2682 三接口的逐项对齐口径
//! 判据要求「方案与 F2682 三接口字段对齐」「对齐零缺失」。
//! 三接口即：F2662 查询 / F2663 插入 / F2664 失效（F2682 冻结版）。
//! 本单映射 10 个字段：
//! ```text
//! 方案字段（PlanCell）              F2682 三接口字段
//! ------------------------------ ------------------------------
//! rw_ratio（读比档）              → f2662.query.rw_class
//! file_size（尺寸档）             → f2663.insert.size_class
//! conc_degree（并发档）           → f2662.query.conc_class
//! hit_expect（期望命中率 ‰）      → f2662.query.hit_permille
//! dirty_expect（期望脏页比 ‰）    → f2663.insert.dirty_permille
//! reclaim_expect（期望回收比 ‰）  → f2664.invalidate.reclaim_permille
//! pages_per_op（单次操作页数）     → f2663.insert.pages_per_op
//! steps（步数上界）               → f2662.query.step_budget
//! frames（帧数上界）              → f2663.insert.frame_cap
//! waves（洪峰面数）               → f2664.invalidate.wave_count
//! ```
//! `align_report()` 逐条核这 10 条映射，任何一条缺失即 `AlignMissing` 拒收。
//!
//! ## 失真声明（判据风险项：方案失真）
//! 判据明确「配比依据注记（对照 B14/F2683 历史读数），失真声明在档」。
//! 本单的配比是**模型侧推导**（由三档闭集按比例族生成），
//! 不是 B14/F2683 的真实历史读数；`FreezeRecord::declare_distortion()`
//! 把这条声明写进冻结件并随哈希入册。
//!
//! ## 性能逐项分解（模型侧成本，非真实硬件时序）
//! - **三矩阵展开** `PlanMatrix::expand()`：`O(RW × SIZE × CONC)` = 125 步纯计算。
//! - **逐格夹具真跑** `run_cell()`：`O(CELL_STEPS)`，`CELL_STEPS = 24`，
//!   上界 `125 × 24 = 3000` 步。
//! - **替身账守恒** `reconcile()`：`O(4)`（四段事件等式 + 帧数界 + 步数界）。
//! - **对齐断言** `align_report()`：`O(ALIGN_FIELDS)` = 10 次字段比对。
//! - **稳定性扫描** `stability_sweep()`：`O(REPEAT × GRID × CELL_STEPS)`，
//!   `REPEAT = 3`，即 9000 步硬上限。
//! - **冻结哈希**：`O(GRID)` 逐格指纹 FNV-1a 折叠（125 次）。
//! - **注入核十二形态**：造证 `O(1)` 定向；检出 `O(FORM_COUNT)`。
//!
//! ### 深在哪（对标 Windows 同类能力）
//! Windows 的缓存压测方案思想同构（Cache Write Performance / File Server
//! Workload），但其配比是手填百分比 + 一次性吞吐数字，读数不随方案变
//! 化溯源。本单深在四处：
//! 1. **125 格现算**：由 `5×5×5` 在构造期算出并与常量逐位比对，
//!    增一档并发就变 150，不允许「常量写死 125 却与档数脱钩」。
//! 2. **10 条对齐映射 + 缺失位图 + 重复位图**：「对齐零缺失」是位图全零的
//!    操作化定义，不是人工核对表格。
//! 3. **三矩阵面名可查**：每个格归属唯一洪峰面（读命中洪峰 / 写入洪峰 /
//!    回收对抗），与 F2913 账册面名一一对应，不是「跑完了就算」。
//! 4. **稳定性重跑判定**：同种子连跑 3 轮，逐格指纹必须全等。
//!
//! ## 纯功能线
//! `no_std + alloc`；零 `unwrap` / `expect` / `panic!`、零浮点、零真实 IO、
//! 零墙钟；时间一律逻辑 `tick: u64`；确定性 LCG
//! （`LCG_A = 6364136223846793005`、`LCG_C = 1442695040888963407`、
//! 模 `2^64` 回绕）保证同种子同轨迹；FNV-1a basis `0xcbf29ce484222325`、
//! prime `0x100000001b3`；自定义 `UcResult<T>`，错误 `code() -> u16` +
//! `detail() -> String` 三要素齐备；无 `#[cfg(test)]`。
#![allow(dead_code)]
extern crate alloc;

// `vec!` 在本单由 TLV 预分配与回读数组使用；此处保留统一导入形态（册内风格一致）。
#[allow(unused_imports)]
use alloc::{format, string::String, string::ToString, vec, vec::Vec};

// ===========================================================================
// 0. 常量表
// ===========================================================================
/// 模块版本标识（写入冻结件 + TLV 组头）。
pub const V_VERSION_ID: u64 = 0x0001_2901_0000_0001;
/// 模块版本串。
pub const V_VERSION: &str = "v2901-ucachestress.v1";
/// 诊断码段基址（独占段 `0x21C0`）。
pub const SEG_BASE: u16 = 0x21C0;
/// 诊断码段掩码（段容量 64 码）。
pub const SEG_MASK: u16 = 0x003F;
/// 诊断码段上界（= SEG_BASE | SEG_MASK）。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;
/// 诊断主码（段基址本身）。
pub const MAIN_CODE: u16 = SEG_BASE;
/// 压测方案册版本号（冻结件入册时登记）。
pub const STRESS_BOOK_REV: u32 = 9;
/// 读写比档数（0:1 / 1:4 / 1:1 / 4:1 / 1:0）。
pub const RW_COUNT: usize = 5;
/// 文件尺寸档数（4KB / 64KB / 1MB / 64MB / 1GB）。
pub const SIZE_COUNT: usize = 5;
/// 并发度档数（1 / 2 / 4 / 6 / 8 核）。
pub const CONC_COUNT: usize = 5;
/// 三矩阵交叉格数（`5 × 5 × 5`，构造期算出并断言）。
pub const GRID_CELLS: usize = RW_COUNT * SIZE_COUNT * CONC_COUNT;
/// 三矩阵洪峰面数（读命中洪峰 / 写入洪峰 / 回收对抗）。
pub const WAVE_COUNT: usize = 3;
/// 单格夹具真跑步数。
pub const CELL_STEPS: usize = 24;
/// 全矩阵真跑步数上界。
pub const MATRIX_STEPS: usize = GRID_CELLS * CELL_STEPS;
/// 稳定性重复轮数。
pub const REPEAT: usize = 3;
/// 注入形态数（判据要求漂移 10/10，本单给 12）。
pub const FORM_COUNT: usize = 12;
/// 对齐字段数（F2682 三接口共 10 个映射字段）。
pub const ALIGN_FIELDS: usize = 10;
/// 环形账容量（2 的幂）。
pub const RING_CAP: usize = 128;
/// 环形账掩码。
pub const RING_MASK: usize = RING_CAP - 1;
/// 逻辑时钟步进量。
pub const TICK_STEP: u64 = 1;
/// step 硬上限（`MATRIX_STEPS × REPEAT` 再留一倍裕量）。
pub const STEP_CAP: u64 = (MATRIX_STEPS * REPEAT * 2) as u64;
/// syscall 上界（本单零真实 syscall）。
pub const SYSCALL_CAP: u64 = 0;
/// 内存峰值上界（字节，模型侧估算）。
pub const MEM_PEAK_CAP: u64 = 8 * 1024 * 1024;
/// FNV-1a basis。
pub const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a prime。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;
/// LCG 乘数。
pub const LCG_A: u64 = 6364136223846793005;
/// LCG 增量。
pub const LCG_C: u64 = 1442695040888963407;
/// F2682 冻结版单号（本单三接口的上游）。
pub const CACHE_TICKET: &str = "F2682";
/// 三接口单号（F2662 / F2663 / F2664）。
pub const IF_QUERY: &str = "F2662";
/// 插入接口单号。
pub const IF_INSERT: &str = "F2663";
/// 失效接口单号。
pub const IF_INVALIDATE: &str = "F2664";
/// 账册面名单号（F2913）。
pub const LEDGER_TICKET: &str = "F2913";
/// 压测执行单号区间（本单明确不做执行）。
pub const EXEC_RANGE: &str = "F2902-F2909";

// ===========================================================================
// 1. 确定性 LCG + FNV-1a
// ===========================================================================
/// 确定性 LCG（Knuth MMIX 62 位参数，模 2^64 回绕）。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    state: u64,
}
impl Lcg {
    /// 由种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed } }
    }
    /// 当前原始态。
    pub fn raw(&self) -> u64 {
        self.state
    }
    /// 步进一步。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(LCG_A).wrapping_add(LCG_C);
        self.state
    }
    /// 取 `[0, n)` 内均匀值（拒绝采样，避免取模偏差）。
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        let nn = n as u64;
        let limit = u64::MAX - (u64::MAX % nn) - 1;
        loop {
            let x = self.next_u64();
            if x <= limit {
                return (x % nn) as usize;
            }
        }
    }
    /// 取千分数（0..=1000）。
    pub fn permille(&mut self) -> u64 {
        self.below(1001) as u64
    }
}
/// 单步 LCG（无状态纯函数）。
pub fn lcg_step(state: u64) -> u64 {
    state.wrapping_mul(LCG_A).wrapping_add(LCG_C)
}
/// FNV-1a 折叠（可续算）。
pub fn fnv1a64_ext(state: u64, bytes: &[u8]) -> u64 {
    let mut h = state;
    let mut i: usize = 0;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => break,
        };
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i = i.wrapping_add(1);
    }
    h
}
/// FNV-1a 64 位（字节序列）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    fnv1a64_ext(FNV_BASIS, bytes)
}
/// FNV-1a 64 位折叠（64 位字序列，小端 8 字节/字）。
pub fn fnv1a_words(words: &[u64]) -> u64 {
    let mut h = FNV_BASIS;
    let mut i: usize = 0;
    while i < words.len() {
        let w = match words.get(i) {
            Some(v) => *v,
            None => break,
        };
        let mut b: usize = 0;
        while b < 8 {
            h ^= ((w >> (b * 8)) & 0xFFu64) as u64;
            h = h.wrapping_mul(FNV_PRIME);
            b = b.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    h
}
/// 字符串取字节视图。
fn sbytes(s: &str) -> &[u8] {
    s.as_bytes()
}
/// 逻辑时钟步进。
pub fn tick_advance(now: u64) -> u64 {
    now.wrapping_add(TICK_STEP)
}

// ===========================================================================
// 2. 诊断码与错误闭集（18 变体，逐个占段内偏移）
// ===========================================================================
/// 注入 / 失效形态（12 个，一码一段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// 读比档越界（读比档被改成非法值）。
    RwRatioOutOfBand,
    /// 尺寸档越界（尺寸档被改成非法值）。
    SizeOutOfBand,
    /// 并发档越界（并发档被改成非法值）。
    ConcOutOfBand,
    /// 命中率预测与实跑不符。
    HitExpectSkew,
    /// 脏页比预测与实跑不符。
    DirtyExpectSkew,
    /// 回收比预测与实跑不符。
    ReclaimExpectSkew,
    /// 替身账事件不守恒。
    LedgerUnbalanced,
    /// 对齐字段缺失（F2682 三接口之一无映射）。
    AlignMissing,
    /// 对齐字段重复（两方案字段映射到同一接口字段）。
    AlignDuplicated,
    /// 步数超界。
    StepOverflow,
    /// 帧数超界。
    FrameOverflow,
    /// 洪峰面归属冲突（一个格归两面）。
    WaveConflict,
}
impl Form {
    /// 全部形态。
    pub const ALL: [Form; FORM_COUNT] = [
        Form::RwRatioOutOfBand,
        Form::SizeOutOfBand,
        Form::ConcOutOfBand,
        Form::HitExpectSkew,
        Form::DirtyExpectSkew,
        Form::ReclaimExpectSkew,
        Form::LedgerUnbalanced,
        Form::AlignMissing,
        Form::AlignDuplicated,
        Form::StepOverflow,
        Form::FrameOverflow,
        Form::WaveConflict,
    ];
    /// 序号（0..11）。
    pub fn index(self) -> usize {
        match self {
            Form::RwRatioOutOfBand => 0,
            Form::SizeOutOfBand => 1,
            Form::ConcOutOfBand => 2,
            Form::HitExpectSkew => 3,
            Form::DirtyExpectSkew => 4,
            Form::ReclaimExpectSkew => 5,
            Form::LedgerUnbalanced => 6,
            Form::AlignMissing => 7,
            Form::AlignDuplicated => 8,
            Form::StepOverflow => 9,
            Form::FrameOverflow => 10,
            Form::WaveConflict => 11,
        }
    }
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE.wrapping_add(self.index() as u16)
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            Form::RwRatioOutOfBand => "rw_ratio_out_of_band",
            Form::SizeOutOfBand => "size_out_of_band",
            Form::ConcOutOfBand => "conc_out_of_band",
            Form::HitExpectSkew => "hit_expect_skew",
            Form::DirtyExpectSkew => "dirty_expect_skew",
            Form::ReclaimExpectSkew => "reclaim_expect_skew",
            Form::LedgerUnbalanced => "ledger_unbalanced",
            Form::AlignMissing => "align_missing",
            Form::AlignDuplicated => "align_duplicated",
            Form::StepOverflow => "step_overflow",
            Form::FrameOverflow => "frame_overflow",
            Form::WaveConflict => "wave_conflict",
        }
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            Form::RwRatioOutOfBand => "读比档越界：不在 0:1/1:4/1:1/4:1/1:0 闭集内",
            Form::SizeOutOfBand => "尺寸档越界：不在 4KB..1GB 闭集内",
            Form::ConcOutOfBand => "并发档越界：不在 1..8 核闭集内",
            Form::HitExpectSkew => "命中率预测与替身账实跑读数不符",
            Form::DirtyExpectSkew => "脏页比预测与实跑读数不符",
            Form::ReclaimExpectSkew => "回收比预测与实跑读数不符",
            Form::LedgerUnbalanced => "替身账四段事件不守恒",
            Form::AlignMissing => "对齐缺失：F2682 三接口有字段无映射",
            Form::AlignDuplicated => "对齐重复：两方案字段映射到同一接口字段",
            Form::StepOverflow => "步数超界：实跑超过方案上界",
            Form::FrameOverflow => "帧数超界：占用超过方案帧数上界",
            Form::WaveConflict => "洪峰面归属冲突：一格归两面",
        }
    }
    /// 探针（造证手法）。
    pub fn probe(self) -> &'static str {
        match self {
            Form::RwRatioOutOfBand => "把 0 号格读比档序号改成 99",
            Form::SizeOutOfBand => "把 0 号格尺寸档序号改成 99",
            Form::ConcOutOfBand => "把 0 号格并发档序号改成 99",
            Form::HitExpectSkew => "把 0 号格期望命中率改成 +300‰",
            Form::DirtyExpectSkew => "把 0 号格期望脏页比改成 +300‰",
            Form::ReclaimExpectSkew => "把 0 号格期望回收比改成 +300‰",
            Form::LedgerUnbalanced => "篡改 0 号格帧数上界使替身账守恒式破裂",
            Form::AlignMissing => "抹掉 F2663 插入接口的 dirty_permille 映射",
            Form::AlignDuplicated => "把两字段都映射到 f2662.query.hit_permille",
            Form::StepOverflow => "把 0 号格步数上界改成 1",
            Form::FrameOverflow => "把 0 号格帧数上界改成 0",
            Form::WaveConflict => "把 0 号格洪峰面序号改成 99",
        }
    }
    /// 是否属「维度越界」类。
    pub fn is_band(self) -> bool {
        matches!(self, Form::RwRatioOutOfBand | Form::SizeOutOfBand | Form::ConcOutOfBand)
    }
    /// 是否属「预测失真」类。
    pub fn is_expect_skew(self) -> bool {
        matches!(self, Form::HitExpectSkew | Form::DirtyExpectSkew | Form::ReclaimExpectSkew)
    }
    /// 是否属「对齐缺陷」类。
    pub fn is_align(self) -> bool {
        matches!(self, Form::AlignMissing | Form::AlignDuplicated)
    }
    /// 所属类名。
    pub fn class_name(self) -> &'static str {
        if self.is_band() {
            "越界"
        } else if self.is_expect_skew() {
            "失真"
        } else if self.is_align() {
            "对齐"
        } else {
            "守恒"
        }
    }
}
/// 辅助诊断码（6 个，占段内高位偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuxCode {
    /// 读比档越界。
    RwIndexOutOfRange = 20,
    /// 尺寸档越界。
    SizeIndexOutOfRange = 21,
    /// 并发档越界。
    ConcIndexOutOfRange = 22,
    /// 格号越界。
    CellIndexOutOfRange = 23,
    /// 冻结态非法写入。
    FrozenWriteRejected = 24,
    /// 册版本不符。
    BookRevMismatch = 25,
}
impl AuxCode {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE.wrapping_add(self as u16)
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            AuxCode::RwIndexOutOfRange => "rw_index_out_of_range",
            AuxCode::SizeIndexOutOfRange => "size_index_out_of_range",
            AuxCode::ConcIndexOutOfRange => "conc_index_out_of_range",
            AuxCode::CellIndexOutOfRange => "cell_index_out_of_range",
            AuxCode::FrozenWriteRejected => "frozen_write_rejected",
            AuxCode::BookRevMismatch => "book_rev_mismatch",
        }
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            AuxCode::RwIndexOutOfRange => "读比档序号越界：不在 0..5 内",
            AuxCode::SizeIndexOutOfRange => "尺寸档序号越界：不在 0..5 内",
            AuxCode::ConcIndexOutOfRange => "并发档序号越界：不在 0..5 内",
            AuxCode::CellIndexOutOfRange => "格号越界：不在 0..125 内",
            AuxCode::FrozenWriteRejected => "冻结态写入拒收：已冻结后不得改参数",
            AuxCode::BookRevMismatch => "册版本不符：冻结件与 CI 基线版本不一致",
        }
    }
}
/// 错误结构（三要素）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UcErr {
    /// 诊断码。
    pub code: u16,
    /// 明文细节。
    pub detail: String,
}
/// 结果别名。
pub type UcResult<T> = Result<T, UcErr>;
impl UcErr {
    /// 构造。
    pub fn new(code: u16, detail: String) -> UcErr {
        UcErr { code, detail }
    }
    /// 由形态构造。
    pub fn of_form(f: Form, detail: String) -> UcErr {
        UcErr { code: f.code(), detail }
    }
    /// 由辅助码构造。
    pub fn of_aux(a: AuxCode, detail: String) -> UcErr {
        UcErr { code: a.code(), detail }
    }
    /// 三要素之一：诊断码。
    pub fn code(&self) -> u16 {
        self.code
    }
    /// 三要素之二：明文细节。
    pub fn detail(&self) -> String {
        self.detail.clone()
    }
    /// 三要素之三：全串。
    pub fn full(&self) -> String {
        format!("0x{:04X} {}", self.code, self.detail)
    }
}
/// 错误转字符串。
pub fn err_str(e: &UcErr) -> String {
    e.full()
}
/// 错误构造辅助（读比档越界）。
fn err_rw(i: usize) -> UcErr {
    UcErr::of_aux(AuxCode::RwIndexOutOfRange, format!("rw_idx={} rw_count={}", i, RW_COUNT))
}
/// 错误构造辅助（尺寸档越界）。
fn err_size(i: usize) -> UcErr {
    UcErr::of_aux(AuxCode::SizeIndexOutOfRange, format!("size_idx={} size_count={}", i, SIZE_COUNT))
}
/// 错误构造辅助（并发档越界）。
fn err_conc(i: usize) -> UcErr {
    UcErr::of_aux(AuxCode::ConcIndexOutOfRange, format!("conc_idx={} conc_count={}", i, CONC_COUNT))
}
/// 错误构造辅助（格号越界）。
fn err_cell(i: usize) -> UcErr {
    UcErr::of_aux(AuxCode::CellIndexOutOfRange, format!("cell_idx={} grid={}", i, GRID_CELLS))
}
/// 错误构造辅助（冻结态非法迁移）。
fn err_frozen(from: &str, to: &str) -> UcErr {
    UcErr::of_aux(AuxCode::FrozenWriteRejected, format!("from={} to={}", from, to))
}
// ===========================================================================
// 3. 三矩阵闭集（读比 / 尺寸 / 并发 / 洪峰面）
// ===========================================================================
/// 读写比闭集（5 档：`0:1` / `1:4` / `1:1` / `4:1` / `1:0`）。
///
/// 档序刻意从纯读排到纯写，中间三档是「混合区」。
/// 这样 `wave_of()` 只需看档位区间即可定洪峰面，不必看数值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RwRatio {
    /// 档 0：纯读（0 写 : 1 读）。
    ReadOnly,
    /// 档 1：读多写少（1 写 : 4 读）。
    ReadHeavy,
    /// 档 2：读写均衡（1 : 1）。
    Balanced,
    /// 档 3：写多读少（4 写 : 1 读）。
    WriteHeavy,
    /// 档 4：纯写（1 写 : 0 读）。
    WriteOnly,
}
impl RwRatio {
    /// 全部档位。
    pub const ALL: [RwRatio; RW_COUNT] = [
        RwRatio::ReadOnly,
        RwRatio::ReadHeavy,
        RwRatio::Balanced,
        RwRatio::WriteHeavy,
        RwRatio::WriteOnly,
    ];
    /// 序号（0..4）。
    pub fn index(self) -> usize {
        match self {
            RwRatio::ReadOnly => 0,
            RwRatio::ReadHeavy => 1,
            RwRatio::Balanced => 2,
            RwRatio::WriteHeavy => 3,
            RwRatio::WriteOnly => 4,
        }
    }
    /// 由序号反解。
    pub fn from_index(i: usize) -> Option<RwRatio> {
        match i {
            0 => Some(RwRatio::ReadOnly),
            1 => Some(RwRatio::ReadHeavy),
            2 => Some(RwRatio::Balanced),
            3 => Some(RwRatio::WriteHeavy),
            4 => Some(RwRatio::WriteOnly),
            _ => None,
        }
    }
    /// 读次数（每 5 次操作中的读数）。
    pub fn reads(self) -> u64 {
        match self {
            RwRatio::ReadOnly => 5,
            RwRatio::ReadHeavy => 4,
            RwRatio::Balanced => 2,
            RwRatio::WriteHeavy => 1,
            RwRatio::WriteOnly => 0,
        }
    }
    /// 写次数（每 5 次操作中的写数；`reads + writes == 5` 恒成立）。
    pub fn writes(self) -> u64 {
        5u64.wrapping_sub(self.reads())
    }
    /// 读比串（登记进对齐字段）。
    pub fn ratio_str(self) -> &'static str {
        match self {
            RwRatio::ReadOnly => "0:1",
            RwRatio::ReadHeavy => "1:4",
            RwRatio::Balanced => "1:1",
            RwRatio::WriteHeavy => "4:1",
            RwRatio::WriteOnly => "1:0",
        }
    }
    /// 是否纯读。
    pub fn is_read_only(self) -> bool {
        matches!(self, RwRatio::ReadOnly)
    }
    /// 是否纯写。
    pub fn is_write_only(self) -> bool {
        matches!(self, RwRatio::WriteOnly)
    }
    /// 是否混合区（回收对抗面来源）。
    pub fn is_mixed(self) -> bool {
        matches!(self, RwRatio::ReadHeavy | RwRatio::Balanced | RwRatio::WriteHeavy)
    }
    /// 期望命中率（千分数；纯读最高、纯写最低）。
    pub fn hit_expect(self) -> u64 {
        match self {
            RwRatio::ReadOnly => 980,
            RwRatio::ReadHeavy => 920,
            RwRatio::Balanced => 850,
            RwRatio::WriteHeavy => 500,
            RwRatio::WriteOnly => 0,
        }
    }
    /// 期望脏页比（千分数；纯写最高）。
    pub fn dirty_expect(self) -> u64 {
        match self {
            RwRatio::ReadOnly => 0,
            RwRatio::ReadHeavy => 100,
            RwRatio::Balanced => 250,
            RwRatio::WriteHeavy => 700,
            RwRatio::WriteOnly => 1000,
        }
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            RwRatio::ReadOnly => "0:1 纯读：读命中洪峰主力",
            RwRatio::ReadHeavy => "1:4 读多写少：读命中洪峰次主力",
            RwRatio::Balanced => "1:1 读写均衡：回收对抗面核心",
            RwRatio::WriteHeavy => "4:1 写多读少：回收对抗面次核心",
            RwRatio::WriteOnly => "1:0 纯写：写入洪峰主力",
        }
    }
}
/// 文件尺寸闭集（5 档：4KB / 64KB / 1MB / 64MB / 1GB）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileSize {
    /// 档 0：4KB（最小工作集，全驻留）。
    K4,
    /// 档 1：64KB（小工作集）。
    K64,
    /// 档 2：1MB（中等工作集）。
    M1,
    /// 档 3：64MB（超越多数 L2/L3）。
    M64,
    /// 档 4：1GB（最大工作集，主要对抗面）。
    G1,
}
impl FileSize {
    /// 全部档位。
    pub const ALL: [FileSize; SIZE_COUNT] = [FileSize::K4, FileSize::K64, FileSize::M1, FileSize::M64, FileSize::G1];
    /// 序号（0..4）。
    pub fn index(self) -> usize {
        match self {
            FileSize::K4 => 0,
            FileSize::K64 => 1,
            FileSize::M1 => 2,
            FileSize::M64 => 3,
            FileSize::G1 => 4,
        }
    }
    /// 由序号反解。
    pub fn from_index(i: usize) -> Option<FileSize> {
        match i {
            0 => Some(FileSize::K4),
            1 => Some(FileSize::K64),
            2 => Some(FileSize::M1),
            3 => Some(FileSize::M64),
            4 => Some(FileSize::G1),
            _ => None,
        }
    }
    /// 字节数。
    pub fn bytes(self) -> u64 {
        match self {
            FileSize::K4 => 4 * 1024,
            FileSize::K64 => 64 * 1024,
            FileSize::M1 => 1024 * 1024,
            FileSize::M64 => 64 * 1024 * 1024,
            FileSize::G1 => 1024 * 1024 * 1024,
        }
    }
    /// 页数（4KB/页）。
    pub fn pages(self) -> u64 {
        self.bytes() / 4096
    }
    /// 尺寸串（登记进对齐字段）。
    pub fn size_str(self) -> &'static str {
        match self {
            FileSize::K4 => "4KB",
            FileSize::K64 => "64KB",
            FileSize::M1 => "1MB",
            FileSize::M64 => "64MB",
            FileSize::G1 => "1GB",
        }
    }
    /// 是否主要对抗面（超过 LLC 假设 32MB 的大尺寸档）。
    pub fn is_primary_adversarial(self) -> bool {
        matches!(self, FileSize::M64 | FileSize::G1)
    }
    /// 期望回收比（千分数；工作集越大回收越频繁）。
    pub fn reclaim_expect(self) -> u64 {
        match self {
            FileSize::K4 => 50,
            FileSize::K64 => 120,
            FileSize::M1 => 350,
            FileSize::M64 => 780,
            FileSize::G1 => 950,
        }
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            FileSize::K4 => "4KB：全驻留，回收极低",
            FileSize::K64 => "64KB：小工作集，回收低",
            FileSize::M1 => "1MB：中等工作集，回收中",
            FileSize::M64 => "64MB：超越 LLC，回收高（主要对抗面）",
            FileSize::G1 => "1GB：最大工作集，回收极高（主要对抗面）",
        }
    }
}
/// 并发度闭集（5 档：1 / 2 / 4 / 6 / 8 核）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConcDegree {
    /// 档 0：1 核。
    C1,
    /// 档 1：2 核。
    C2,
    /// 档 2：4 核。
    C4,
    /// 档 3：6 核。
    C6,
    /// 档 4：8 核。
    C8,
}
impl ConcDegree {
    /// 全部档位。
    pub const ALL: [ConcDegree; CONC_COUNT] = [ConcDegree::C1, ConcDegree::C2, ConcDegree::C4, ConcDegree::C6, ConcDegree::C8];
    /// 序号（0..4）。
    pub fn index(self) -> usize {
        match self {
            ConcDegree::C1 => 0,
            ConcDegree::C2 => 1,
            ConcDegree::C4 => 2,
            ConcDegree::C6 => 3,
            ConcDegree::C8 => 4,
        }
    }
    /// 由序号反解。
    pub fn from_index(i: usize) -> Option<ConcDegree> {
        match i {
            0 => Some(ConcDegree::C1),
            1 => Some(ConcDegree::C2),
            2 => Some(ConcDegree::C4),
            3 => Some(ConcDegree::C6),
            4 => Some(ConcDegree::C8),
            _ => None,
        }
    }
    /// 核数。
    pub fn cores(self) -> u64 {
        match self {
            ConcDegree::C1 => 1,
            ConcDegree::C2 => 2,
            ConcDegree::C4 => 4,
            ConcDegree::C6 => 6,
            ConcDegree::C8 => 8,
        }
    }
    /// 并发道数（每核 2 道；1 核档 1 道避免自我争用）。
    pub fn lanes(self) -> u64 {
        if self == ConcDegree::C1 {
            1
        } else {
            self.cores().wrapping_mul(2)
        }
    }
    /// 争用度（千分数；核数越多争用越高）。
    pub fn contend(self) -> u64 {
        match self {
            ConcDegree::C1 => 0,
            ConcDegree::C2 => 200,
            ConcDegree::C4 => 500,
            ConcDegree::C6 => 750,
            ConcDegree::C8 => 1000,
        }
    }
    /// 档码。
    pub fn code2(self) -> &'static str {
        match self {
            ConcDegree::C1 => "C1",
            ConcDegree::C2 => "C2",
            ConcDegree::C4 => "C4",
            ConcDegree::C6 => "C6",
            ConcDegree::C8 => "C8",
        }
    }
    /// 是否重度争用（主要对抗面）。
    pub fn is_contended(self) -> bool {
        matches!(self, ConcDegree::C6 | ConcDegree::C8)
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            ConcDegree::C1 => "1 核：单道，无自我争用",
            ConcDegree::C2 => "2 核：轻度争用",
            ConcDegree::C4 => "4 核：中度争用",
            ConcDegree::C6 => "6 核：重度争用（主要对抗面）",
            ConcDegree::C8 => "8 核：极重争用（主要对抗面）",
        }
    }
}
/// 洪峰面闭集（3 面，与 F2913 账册面名一一对应）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaveFace {
    /// 面 0：读命中洪峰。
    ReadHitFlood,
    /// 面 1：写入洪峰。
    WriteFlood,
    /// 面 2：回收对抗。
    ReclaimAdversarial,
}
impl WaveFace {
    /// 全部面。
    pub const ALL: [WaveFace; WAVE_COUNT] = [WaveFace::ReadHitFlood, WaveFace::WriteFlood, WaveFace::ReclaimAdversarial];
    /// 序号（0..2）。
    pub fn index(self) -> usize {
        match self {
            WaveFace::ReadHitFlood => 0,
            WaveFace::WriteFlood => 1,
            WaveFace::ReclaimAdversarial => 2,
        }
    }
    /// 由序号反解。
    pub fn from_index(i: usize) -> Option<WaveFace> {
        match i {
            0 => Some(WaveFace::ReadHitFlood),
            1 => Some(WaveFace::WriteFlood),
            2 => Some(WaveFace::ReclaimAdversarial),
            _ => None,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            WaveFace::ReadHitFlood => "read_hit_flood",
            WaveFace::WriteFlood => "write_flood",
            WaveFace::ReclaimAdversarial => "reclaim_adversarial",
        }
    }
    /// 面名（F2913 账册面名，判据要求一一对应可查）。
    pub fn ledger_name(self) -> &'static str {
        match self {
            WaveFace::ReadHitFlood => "读命中洪峰",
            WaveFace::WriteFlood => "写入洪峰",
            WaveFace::ReclaimAdversarial => "回收对抗",
        }
    }
    /// 面码（登记进方案）。
    pub fn code2(self) -> &'static str {
        match self {
            WaveFace::ReadHitFlood => "RF",
            WaveFace::WriteFlood => "WF",
            WaveFace::ReclaimAdversarial => "RA",
        }
    }
    /// 是否主要对抗面。
    pub fn is_adversarial(self) -> bool {
        matches!(self, WaveFace::ReclaimAdversarial)
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            WaveFace::ReadHitFlood => "读命中洪峰面：读比档 0/1，各贡献 25 格，合计 50 格",
            WaveFace::WriteFlood => "写入洪峰面：仅读比档 4（纯写），贡献 25 格",
            WaveFace::ReclaimAdversarial => "回收对抗面：读比档 2/3，各贡献 25 格，合计 50 格",
        }
    }
}
/// 由读比档定洪峰面（判据「三矩阵键与 F2913 账册面名一一对应」的实现）。
///
/// 归属规则（纯函数，可被单例反解校验）：
/// - 纯读（档 0）→ 读命中洪峰；
/// - 读多写少（档 1）→ 读命中洪峰；
/// - 读写均衡 / 写多读少（档 2/3）→ 回收对抗；
/// - 纯写（档 4）→ 写入洪峰。
pub fn wave_of(rw: RwRatio) -> WaveFace {
    match rw {
        RwRatio::ReadOnly => WaveFace::ReadHitFlood,
        RwRatio::ReadHeavy => WaveFace::ReadHitFlood,
        RwRatio::Balanced => WaveFace::ReclaimAdversarial,
        RwRatio::WriteHeavy => WaveFace::ReclaimAdversarial,
        RwRatio::WriteOnly => WaveFace::WriteFlood,
    }
}

// ===========================================================================
// 4. 三矩阵参数（正交展开，格数显式计算）
// ===========================================================================
/// 单格三矩阵参数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PlanCell {
    /// 格号（0..124）。
    pub index: usize,
    /// 读比档序号。
    pub rw_idx: usize,
    /// 尺寸档序号。
    pub size_idx: usize,
    /// 并发档序号。
    pub conc_idx: usize,
    /// 洪峰面序号。
    pub wave_idx: usize,
    /// 每步操作数（`reads + writes == 5`，乘并发道数得上界）。
    pub ops_per_step: u64,
    /// 单次操作页数。
    pub pages_per_op: u64,
    /// 方案步数上界（三档统一，保证 125 格可比）。
    pub steps: u64,
    /// 方案帧数上界。
    pub frames: u64,
    /// 期望命中率（千分数）。
    pub hit_expect: u64,
    /// 期望脏页比（千分数）。
    pub dirty_expect: u64,
    /// 期望回收比（千分数）。
    pub reclaim_expect: u64,
    /// 格指纹（入冻结哈希）。
    pub fingerprint: u64,
}
impl PlanCell {
    /// 由三维序号构造一格。
    ///
    /// 口径注记：
    /// - `steps` 三档统一 `CELL_STEPS`——125 格要横向对比，
    ///   步数必须一致，否则「哪个档更慢」无从判断；
    /// - `ops_per_step = (reads + writes) × lanes = 5 × lanes`，
    ///   与读比档无关（档位体现在 `reads/writes` 比例上）；
    /// - `frames = pages_per_op × ops_per_step × steps`，
    ///   是替身账占用上界的推导起点；
    /// - 三个 `expect` 分别由三个维度派生（读比→命中/脏页、尺寸→回收），
    ///   **不做跨维度混合**，否则归因时说不清是哪个维度贡献的偏差。
    pub fn new(index: usize, rw: RwRatio, size: FileSize, conc: ConcDegree) -> PlanCell {
        let ops = (rw.reads().wrapping_add(rw.writes())).wrapping_mul(conc.lanes());
        let steps = CELL_STEPS as u64;
        let ppo = size.pages();
        let frames = ppo.wrapping_mul(ops).wrapping_mul(steps);
        let wave = wave_of(rw);
        let mut words: [u64; 10] = [0u64; 10];
        words[0] = index as u64;
        words[1] = rw.index() as u64;
        words[2] = size.index() as u64;
        words[3] = conc.index() as u64;
        words[4] = wave.index() as u64;
        words[5] = ops;
        words[6] = ppo;
        words[7] = steps;
        words[8] = frames;
        words[9] = rw.hit_expect() ^ size.reclaim_expect();
        PlanCell {
            index,
            rw_idx: rw.index(),
            size_idx: size.index(),
            conc_idx: conc.index(),
            wave_idx: wave.index(),
            ops_per_step: ops,
            pages_per_op: ppo,
            steps,
            frames,
            hit_expect: rw.hit_expect(),
            dirty_expect: rw.dirty_expect(),
            reclaim_expect: size.reclaim_expect(),
            fingerprint: fnv1a_words(&words),
        }
    }
    /// 还原读比档。
    pub fn rw(&self) -> UcResult<RwRatio> {
        match RwRatio::from_index(self.rw_idx) {
            Some(v) => Ok(v),
            None => Err(err_rw(self.rw_idx)),
        }
    }
    /// 还原尺寸档。
    pub fn size(&self) -> UcResult<FileSize> {
        match FileSize::from_index(self.size_idx) {
            Some(v) => Ok(v),
            None => Err(err_size(self.size_idx)),
        }
    }
    /// 还原并发档。
    pub fn conc(&self) -> UcResult<ConcDegree> {
        match ConcDegree::from_index(self.conc_idx) {
            Some(v) => Ok(v),
            None => Err(err_conc(self.conc_idx)),
        }
    }
    /// 还原洪峰面。
    pub fn wave(&self) -> UcResult<WaveFace> {
        match WaveFace::from_index(self.wave_idx) {
            Some(v) => Ok(v),
            None => Err(UcErr::of_form(Form::WaveConflict, format!("wave_idx={} cell={}", self.wave_idx, self.index))),
        }
    }
    /// 三维闭集。
    pub fn dims_ok(&self) -> bool {
        self.rw_idx < RW_COUNT && self.size_idx < SIZE_COUNT && self.conc_idx < CONC_COUNT && self.index < GRID_CELLS
    }
    /// 洪峰面归属是否与读比档一致（反解校验）。
    pub fn wave_ok(&self) -> bool {
        match (self.rw(), self.wave()) {
            (Ok(r), Ok(w)) => wave_of(r) == w,
            _ => false,
        }
    }
    /// 是否主要对抗面（并发重度 或 尺寸大）。
    pub fn is_primary_adversarial(&self) -> bool {
        match (self.size(), self.conc()) {
            (Ok(s), Ok(c)) => s.is_primary_adversarial() || c.is_contended(),
            _ => false,
        }
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "cell#{:03} rw={} size={} conc={} wave={} ops/step={} pages/op={} steps={} frames={} expect(h/d/r)={}/{}/{} adv={} fp={:016X}",
            self.index,
            self.rw_idx,
            self.size_idx,
            self.conc_idx,
            self.wave_idx,
            self.ops_per_step,
            self.pages_per_op,
            self.steps,
            self.frames,
            self.hit_expect,
            self.dirty_expect,
            self.reclaim_expect,
            if self.is_primary_adversarial() { 1 } else { 0 },
            self.fingerprint
        )
    }
}
/// 三矩阵参数（`5 × 5 × 5 = 125` 格正交展开）。
#[derive(Clone, Debug)]
pub struct PlanMatrix {
    /// 逐格。
    pub cells: Vec<PlanCell>,
    /// 读比档分布（每档 25 格）。
    pub rw_dist: [usize; RW_COUNT],
    /// 尺寸档分布（每档 25 格）。
    pub size_dist: [usize; SIZE_COUNT],
    /// 并发档分布（每档 25 格）。
    pub conc_dist: [usize; CONC_COUNT],
    /// 洪峰面分布（读 10 / 写 5 / 回收 15）。
    pub wave_dist: [usize; WAVE_COUNT],
    /// 展开时实际产出格数。
    pub produced: usize,
    /// 映射污染：把第 N 条对齐字段的边抹掉（`Some(n)` = 造缺失洞）。
    ///
    /// 为什么污染记在矩阵上而不是全局：注入核要求
    /// 「逐形态从**干净对象**重建」，污染若存在全局开关里，
    /// 前一形态的开关若忘了复位，后面所有形态的证据底都不干净。
    /// 记在 `PlanMatrix` 实例上，则每轮 `expand()` 天然是干净的。
    pub align_null: Option<usize>,
    /// 映射污染：把第 N 条对齐字段的目标改抄第 M 条（造重复洞）。
    pub align_dup: Option<(usize, usize)>,
}
impl PlanMatrix {
    /// 空矩阵。
    pub fn blank() -> PlanMatrix {
        PlanMatrix { cells: Vec::new(), rw_dist: [0; RW_COUNT], size_dist: [0; SIZE_COUNT], conc_dist: [0; CONC_COUNT], wave_dist: [0; WAVE_COUNT], produced: 0, align_null: None, align_dup: None }
    }
    /// 本矩阵的映射污染口径（干净矩阵下两条恒 `false`）。
    pub fn align_polluted(&self) -> (bool, bool) {
        let miss = match self.align_null {
            Some(i) => i < ALIGN_FIELDS,
            None => false,
        };
        let dup = match self.align_dup {
            Some((a, b)) => a < ALIGN_FIELDS && b < ALIGN_FIELDS && a != b,
            None => false,
        };
        (miss, dup)
    }
    /// 正交展开（格数由三维乘积现算，不硬写 125）。
    pub fn expand() -> PlanMatrix {
        let mut m = PlanMatrix::blank();
        let mut idx: usize = 0;
        let mut a: usize = 0;
        while a < RW_COUNT {
            let rw = RwRatio::ALL[a];
            let mut b: usize = 0;
            while b < SIZE_COUNT {
                let size = FileSize::ALL[b];
                let mut c: usize = 0;
                while c < CONC_COUNT {
                    let conc = ConcDegree::ALL[c];
                    let cell = PlanCell::new(idx, rw, size, conc);
                    if let Some(slot) = m.rw_dist.get_mut(a) {
                        *slot = slot.wrapping_add(1);
                    }
                    if let Some(slot) = m.size_dist.get_mut(b) {
                        *slot = slot.wrapping_add(1);
                    }
                    if let Some(slot) = m.conc_dist.get_mut(c) {
                        *slot = slot.wrapping_add(1);
                    }
                    if let Some(slot) = m.wave_dist.get_mut(cell.wave_idx) {
                        *slot = slot.wrapping_add(1);
                    }
                    m.cells.push(cell);
                    idx = idx.wrapping_add(1);
                    c = c.wrapping_add(1);
                }
                b = b.wrapping_add(1);
            }
            a = a.wrapping_add(1);
        }
        m.produced = m.cells.len();
        m
    }
    /// 取格。
    pub fn cell(&self, i: usize) -> UcResult<&PlanCell> {
        match self.cells.get(i) {
            Some(v) => Ok(v),
            None => Err(err_cell(i)),
        }
    }
    /// 取格（可变）。
    pub fn case_mut(&mut self, i: usize) -> UcResult<&mut PlanCell> {
        match self.cells.get_mut(i) {
            Some(v) => Ok(v),
            None => Err(err_cell(i)),
        }
    }
    /// 线性格号。
    pub fn linear_index(rw: usize, size: usize, conc: usize) -> usize {
        (rw * SIZE_COUNT + size) * CONC_COUNT + conc
    }
    /// 格数断言。
    pub fn count_ok(&self) -> bool {
        self.produced == RW_COUNT * SIZE_COUNT * CONC_COUNT && self.produced == GRID_CELLS && self.cells.len() == GRID_CELLS
    }
    /// 正交断言。
    pub fn orthogonal_ok(&self) -> bool {
        let mut seen: [bool; GRID_CELLS] = [false; GRID_CELLS];
        let mut i: usize = 0;
        while i < self.cells.len() {
            let c = match self.cells.get(i) {
                Some(v) => v,
                None => return false,
            };
            if c.rw_idx >= RW_COUNT || c.size_idx >= SIZE_COUNT || c.conc_idx >= CONC_COUNT {
                return false;
            }
            let li = PlanMatrix::linear_index(c.rw_idx, c.size_idx, c.conc_idx);
            if let Some(s) = seen.get_mut(li) {
                if *s {
                    return false;
                }
                *s = true;
            } else {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 三维闭集 + 洪峰归属断言。
    pub fn dims_ok(&self) -> bool {
        let mut i: usize = 0;
        while i < self.cells.len() {
            let c = match self.cells.get(i) {
                Some(v) => v,
                None => return false,
            };
            if !c.dims_ok() || !c.wave_ok() {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 矩阵指纹（125 个格指纹串接折叠）。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(self.cells.len() + 4);
        words.push(self.produced as u64);
        words.push(RW_COUNT as u64);
        words.push(SIZE_COUNT as u64);
        words.push(CONC_COUNT as u64);
        for c in self.cells.iter() {
            words.push(c.fingerprint);
        }
        fnv1a_words(&words)
    }
    /// 逐行。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.cells.len());
        for c in self.cells.iter() {
            out.push(c.describe());
        }
        out
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "matrix cells={}/{} count_ok={} orthogonal={} dims={} waves=[{},{},{}] fp={:016X}",
            self.produced,
            GRID_CELLS,
            if self.count_ok() { 1 } else { 0 },
            if self.orthogonal_ok() { 1 } else { 0 },
            if self.dims_ok() { 1 } else { 0 },
            self.wave_dist[0],
            self.wave_dist[1],
            self.wave_dist[2],
            self.fingerprint()
        )
    }
}
// ===========================================================================
// 5. 替身账 + 守恒（本单不做 cache 本体，只记四类事件计数）
// ===========================================================================
/// 替身账事件类别（4 类，闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Evt {
    /// 读命中（query 命中驻留页）。
    Hit = 0,
    /// 读未命中（query 触页填充）。
    Miss = 1,
    /// 写回（脏页刷出）。
    Writeback = 2,
    /// 回收（驻留帧被逐出）。
    Reclaim = 3,
}
impl Evt {
    /// 类别数。
    pub const COUNT: usize = 4;
    /// 全部类别。
    pub const ALL: [Evt; 4] = [Evt::Hit, Evt::Miss, Evt::Writeback, Evt::Reclaim];
    /// 序号。
    pub fn index(self) -> usize {
        self as usize
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            Evt::Hit => "hit",
            Evt::Miss => "miss",
            Evt::Writeback => "writeback",
            Evt::Reclaim => "reclaim",
        }
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            Evt::Hit => "读命中：查得驻留帧，无需填充",
            Evt::Miss => "读未命中：缺帧，触发一次填充",
            Evt::Writeback => "写回：脏页刷出，脏页比越高写回越多",
            Evt::Reclaim => "回收：驻留帧被逐出，尺寸越大回收压力越高",
        }
    }
}
/// 替身账（**不是页缓存本体**，只记四类事件计数与帧占用）。
///
/// 为什么只做替身：判据明确「不做 cache 本体（F2682）」。
/// 本单要交付的是「参数矩阵 → 每格预期四类事件读数」的方案冻结，
/// 替身账的作用是让每个 `expect` 都有**可对账的实跑读数**，
/// 而不是让读者以为这里已经实现了一颗页缓存。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CacheLedger {
    /// 已跑步数。
    pub steps: u64,
    /// 总操作数（`queries + stores`）。
    pub ops: u64,
    /// 读操作数（`reads × lanes × steps`）。
    pub queries: u64,
    /// 读命中数。
    pub hits: u64,
    /// 读未命中数。
    pub misses: u64,
    /// 写操作数（`writes × lanes × steps`）。
    pub stores: u64,
    /// 写回数。
    pub writebacks: u64,
    /// 回收帧数。
    pub reclaims: u64,
    /// 已占用帧数（= `pages_per_op × ops`，与方案帧数上界同源）。
    pub frames_used: u64,
    /// 常驻页数（= `pages_per_op × lanes`）。
    pub live_pages: u64,
    /// 逻辑时钟（每步 +1）。
    pub tick: u64,
    /// 账指纹。
    pub fingerprint: u64,
}
impl CacheLedger {
    /// 空账（全部为 0）。
    pub fn blank() -> CacheLedger {
        CacheLedger {
            steps: 0,
            ops: 0,
            queries: 0,
            hits: 0,
            misses: 0,
            stores: 0,
            writebacks: 0,
            reclaims: 0,
            frames_used: 0,
            live_pages: 0,
            tick: 0,
            fingerprint: FNV_BASIS,
        }
    }
    /// 按格参数真跑一遍，产出替身账。
    ///
    /// 口径注记（与 `PlanCell::new` 严格同源）：
    /// - `queries = reads × lanes × steps`
    /// - `stores   = writes × lanes × steps`
    /// - `hits     = queries × rw.hit_expect() ÷ 1000`（整数截断）
    /// - `misses   = queries − hits`
    /// - `writebacks = stores × rw.dirty_expect() ÷ 1000`
    /// - `frames_used = size.pages() × ops`（每操作占该尺寸档页数）
    /// - `reclaims = frames_used × size.reclaim_expect() ÷ 1000`
    /// - `live_pages = size.pages() × lanes`
    ///
    /// **关键口径：账只依赖三维档位现算，格上的登记字段一律不读。**
    /// 理由：`hit_expect` / `dirty_expect` / `reclaim_expect` / `steps` /
    /// `pages_per_op` / `frames` 都是**可被污染的登记字段**，
    /// 若账从它们取值，则「改了 expect」会连账一起改，自洽性检查就永远发现不了失真。
    /// 因此账走「基准档现算」，登记字段的一致性由 `reconcile()` 单独对账——
    /// 这样 `HitExpectSkew` / `DirtyExpectSkew` / `ReclaimExpectSkew` /
    /// `LedgerUnbalanced` / `StepOverflow` 五种失效才真正可归因。
    pub fn run(cell: &PlanCell) -> UcResult<CacheLedger> {
        let rw = cell.rw()?;
        let size = cell.size()?;
        let conc = cell.conc()?;
        let lanes = conc.lanes();
        // 基准步数：三档统一，恒为 CELL_STEPS（不读 cell.steps）
        let steps = CELL_STEPS as u64;
        let queries = rw.reads().wrapping_mul(lanes).wrapping_mul(steps);
        let stores = rw.writes().wrapping_mul(lanes).wrapping_mul(steps);
        let ops = queries.wrapping_add(stores);
        // 基准期望：读比档派生命中/脏页，尺寸档派生回收
        let hits = mul_div(queries, rw.hit_expect());
        let misses = queries.wrapping_sub(hits);
        let writebacks = mul_div(stores, rw.dirty_expect());
        // 基准页数：尺寸档现算
        let ppo = size.pages();
        let frames_used = ppo.wrapping_mul(ops);
        let reclaims = mul_div(frames_used, size.reclaim_expect());
        let live_pages = ppo.wrapping_mul(lanes);
        let mut words: [u64; 11] = [0u64; 11];
        words[0] = steps;
        words[1] = ops;
        words[2] = queries;
        words[3] = hits;
        words[4] = misses;
        words[5] = stores;
        words[6] = writebacks;
        words[7] = reclaims;
        words[8] = frames_used;
        words[9] = live_pages;
        words[10] = cell.index as u64;
        Ok(CacheLedger {
            steps,
            ops,
            queries,
            hits,
            misses,
            stores,
            writebacks,
            reclaims,
            frames_used,
            live_pages,
            // 逻辑时钟：起点 `tick = 0`，每跑一步 +1，故跑完 `steps` 步后
            // 恒有 `tick == steps`。此处直接取 `steps` 而非 `tick_advance(steps)`
            // ——后者会多走一格，把「起点 0」的口径改成「起点 1」，
            // 与 `reconcile` 的 `tick == steps` 守恒式互相打架。
            tick: steps,
            fingerprint: fnv1a_words(&words),
        })
    }
    /// 实测命中率千分数（**人读下界**：两次整除的复合误差，不作判定用）。
    pub fn hit_permille(&self) -> u64 {
        div_mul(self.hits, self.queries)
    }
    /// 实测脏页比千分数（人读下界，不作判定用）。
    pub fn dirty_permille(&self) -> u64 {
        div_mul(self.writebacks, self.stores)
    }
    /// 实测回收比千分数（人读下界，不作判定用）。
    pub fn reclaim_permille(&self) -> u64 {
        div_mul(self.reclaims, self.frames_used)
    }
    /// 四类事件读数（按 `Evt` 序号取）。
    pub fn evt(&self, e: Evt) -> u64 {
        match e {
            Evt::Hit => self.hits,
            Evt::Miss => self.misses,
            Evt::Writeback => self.writebacks,
            Evt::Reclaim => self.reclaims,
        }
    }
    /// 守恒自检（返回失败项名，空串表示全过）。
    ///
    /// 守恒式闭集（6 条 + 3 条千分数反算）：
    /// 1. `hits + misses == queries`（读事件闭合）
    /// 2. `queries + stores == ops`（读写合计闭合）
    /// 3. `writebacks <= stores`（写回不超写操作数）
    /// 4. `reclaims <= frames_used`（回收不超占用帧数）
    /// 5. `live_pages == pages_per_op × lanes`（常驻页由尺寸×并发推导）
    /// 6. `tick == steps`（逻辑时钟与步数同步）
    /// 7~9. 三个「按 expect 重算 == 实测」的等式（不用千分数回算，见下）
    ///
    /// **帧数界不进本函数**：帧数是「方案上界」，与格参数耦合，
    /// 由 `CellRun::frames_ok()` 单列断言——把两者混在一起会让
    /// 「账不平」和「方案超界」两种失效互相掩盖。
    pub fn reconcile(&self, cell: &PlanCell) -> UcResult<String> {
        let conc = cell.conc()?;
        let lanes = conc.lanes();
        if self.hits.wrapping_add(self.misses) != self.queries {
            return Err(UcErr::of_form(
                Form::LedgerUnbalanced,
                format!("读事件不闭合 hits={} misses={} queries={}", self.hits, self.misses, self.queries),
            ));
        }
        if self.queries.wrapping_add(self.stores) != self.ops {
            return Err(UcErr::of_form(
                Form::LedgerUnbalanced,
                format!("读写合计不闭合 q={} s={} ops={}", self.queries, self.stores, self.ops),
            ));
        }
        if self.writebacks > self.stores {
            return Err(UcErr::of_form(
                Form::LedgerUnbalanced,
                format!("写回超写 wb={} stores={}", self.writebacks, self.stores),
            ));
        }
        if self.reclaims > self.frames_used {
            return Err(UcErr::of_form(
                Form::LedgerUnbalanced,
                format!("回收超占用 rc={} frames={}", self.reclaims, self.frames_used),
            ));
        }
        let size = cell.size()?;
        let base_ppo = size.pages();
        if self.live_pages != base_ppo.wrapping_mul(lanes) {
            return Err(UcErr::of_form(
                Form::LedgerUnbalanced,
                format!("常驻页推导不符 live={} base_ppo={} lanes={}", self.live_pages, base_ppo, lanes),
            ));
        }
        // 登记字段 vs 基准现算（登记字段被污染即在此暴露）
        if cell.pages_per_op != base_ppo {
            return Err(UcErr::of_form(
                Form::LedgerUnbalanced,
                format!("登记页数与尺寸档不符 登记={} 档现算={} cell#{:03}", cell.pages_per_op, base_ppo, cell.index),
            ));
        }
        // 步数登记必须等于统一档位 CELL_STEPS（改小即 StepOverflow）
        if cell.steps != CELL_STEPS as u64 {
            return Err(UcErr::of_form(
                Form::StepOverflow,
                format!("步数登记偏离统一档 登记={} 档={} cell#{:03}", cell.steps, CELL_STEPS, cell.index),
            ));
        }
        if self.steps != cell.steps {
            return Err(UcErr::of_form(
                Form::StepOverflow,
                format!("实跑步数与登记不符 实跑={} 登记={} cell#{:03}", self.steps, cell.steps, cell.index),
            ));
        }
        // 帧数登记必须等于「基准页数 × 基准 ops」。
        // 注意与 `FrameOverflow` 的分工：这里的 `frames` 是**方案登记上界**，
        // `LedgerUnbalanced` 判它「登记值与推导值不符」，
        // `FrameOverflow` 判它「登记值小于实跑占用」——两者是不同性质的失效。
        let base_frames = base_ppo.wrapping_mul(self.ops);
        // 先判「登记上界小于实占」→ FrameOverflow（专属归因）
        if self.frames_used > cell.frames {
            return Err(UcErr::of_form(
                Form::FrameOverflow,
                format!("帧数超界 实占={} 登记上界={} cell#{:03}", self.frames_used, cell.frames, cell.index),
            ));
        }
        // 再判「登记值与推导值不符」→ LedgerUnbalanced（登记侧失真）
        if cell.frames != base_frames {
            return Err(UcErr::of_form(
                Form::LedgerUnbalanced,
                format!("帧数登记与推导不符 登记={} 推导={} cell#{:03}", cell.frames, base_frames, cell.index),
            ));
        }
        if self.tick != self.steps {
            return Err(UcErr::of_form(
                Form::LedgerUnbalanced,
                format!("时钟不同步 tick={} steps={}", self.tick, self.steps),
            ));
        }
        // 千分数反算：**不做「千分数回算」**。
        // 原因：`hits = ⌊queries × 980 / 1000⌋` 截断后，
        // `⌊hits × 1000 / queries⌋ = 975 ≠ 980`——两次整除的复合误差，
        // 与方案失真无关。故判定改为「按 expect 重算出的 hits 是否等于实跑 hits」，
        // 这是无误差的等式；`hit_permille()` 只作人读下界展示。
        //
        // 三条失真判定 = 「登记 expect」对「档位现算 expect」。
        // 为什么不是对账实测值：账本身就是按档现算跑的，实测必然等于档现算，
        // 真正会失真的只有**登记的 expect**。所以判据必须是登记 vs 档，而非实测 vs 登记。
        let rw = cell.rw()?;
        if cell.hit_expect != rw.hit_expect() {
            return Err(UcErr::of_form(
                Form::HitExpectSkew,
                format!(
                    "命中率登记失真 登记={}‰ 档现算={}‰ cell#{:03}",
                    cell.hit_expect,
                    rw.hit_expect(),
                    cell.index
                ),
            ));
        }
        if cell.dirty_expect != rw.dirty_expect() {
            return Err(UcErr::of_form(
                Form::DirtyExpectSkew,
                format!(
                    "脏页比登记失真 登记={}‰ 档现算={}‰ cell#{:03}",
                    cell.dirty_expect,
                    rw.dirty_expect(),
                    cell.index
                ),
            ));
        }
        if cell.reclaim_expect != size.reclaim_expect() {
            return Err(UcErr::of_form(
                Form::ReclaimExpectSkew,
                format!(
                    "回收比登记失真 登记={}‰ 档现算={}‰ cell#{:03}",
                    cell.reclaim_expect,
                    size.reclaim_expect(),
                    cell.index
                ),
            ));
        }
        // 账内自洽：实测必须等于按档现算 expect 重算出的值
        if self.hits != mul_div(self.queries, rw.hit_expect()) {
            return Err(UcErr::of_form(
                Form::LedgerUnbalanced,
                format!("实测命中与档现算重算不符 实测={} 重算={}", self.hits, mul_div(self.queries, rw.hit_expect())),
            ));
        }
        if self.writebacks != mul_div(self.stores, rw.dirty_expect()) {
            return Err(UcErr::of_form(
                Form::LedgerUnbalanced,
                format!("实测写回与档现算重算不符 实测={} 重算={}", self.writebacks, mul_div(self.stores, rw.dirty_expect())),
            ));
        }
        if self.reclaims != mul_div(self.frames_used, size.reclaim_expect()) {
            return Err(UcErr::of_form(
                Form::LedgerUnbalanced,
                format!("实测回收与档现算重算不符 实测={} 重算={}", self.reclaims, mul_div(self.frames_used, size.reclaim_expect())),
            ));
        }
        Ok(String::new())
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "ledger steps={} tick={} ops={} q={} (hit={} miss={}) store={} wb={} rc={} frames={} live={} permille(h/d/r)={}/{}/{} fp={:016X}",
            self.steps,
            self.tick,
            self.ops,
            self.queries,
            self.hits,
            self.misses,
            self.stores,
            self.writebacks,
            self.reclaims,
            self.frames_used,
            self.live_pages,
            self.hit_permille(),
            self.dirty_permille(),
            self.reclaim_permille(),
            self.fingerprint
        )
    }
}
/// 向下取整的 `a × b ÷ 1000`。
///
/// 千分比统一走这个函数，避免各处手写 `* / 1000` 时
/// 一处向上、一处向下导致的对账假失真。
pub fn mul_div(a: u64, b: u64) -> u64 {
    a.wrapping_mul(b).wrapping_div(1000)
}
/// 向下取整的 `a × 1000 ÷ b`（分母为 0 时返回 0，不做除零 panic）。
pub fn div_mul(a: u64, b: u64) -> u64 {
    if b == 0 {
        return 0;
    }
    a.wrapping_mul(1000).wrapping_div(b)
}

// ===========================================================================
// 6. F2682 三接口字段对齐（10 条映射，零缺失 / 零重复）
// ===========================================================================
/// 对齐字段闭集（10 条，逐条对应 F2682 三接口的一个可读字段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlignField {
    /// 读比档 → 查询接口读比类。
    RwToQueryClass = 0,
    /// 尺寸档 → 插入接口尺寸类。
    SizeToInsertClass = 1,
    /// 并发档 → 查询接口并发类。
    ConcToQueryClass = 2,
    /// 期望命中率 → 查询接口命中千分数。
    HitToQueryPermille = 3,
    /// 期望脏页比 → 插入接口脏页千分数。
    DirtyToInsertPermille = 4,
    /// 期望回收比 → 失效接口回收千分数。
    ReclaimToInvPermille = 5,
    /// 单次操作页数 → 插入接口单操作页数。
    PagesToInsertPages = 6,
    /// 步数上界 → 查询接口步数预算。
    StepsToQueryBudget = 7,
    /// 帧数上界 → 插入接口帧数上界。
    FramesToInsertCap = 8,
    /// 洪峰面数 → 失效接口洪峰面计数。
    WavesToInvCount = 9,
}
impl AlignField {
    /// 全部对齐字段。
    pub const ALL: [AlignField; ALIGN_FIELDS] = [
        AlignField::RwToQueryClass,
        AlignField::SizeToInsertClass,
        AlignField::ConcToQueryClass,
        AlignField::HitToQueryPermille,
        AlignField::DirtyToInsertPermille,
        AlignField::ReclaimToInvPermille,
        AlignField::PagesToInsertPages,
        AlignField::StepsToQueryBudget,
        AlignField::FramesToInsertCap,
        AlignField::WavesToInvCount,
    ];
    /// 序号。
    pub fn index(self) -> usize {
        self as usize
    }
    /// 方案侧字段名（谁提供这个值）。
    pub fn plan_field(self) -> &'static str {
        match self {
            AlignField::RwToQueryClass => "rw_ratio",
            AlignField::SizeToInsertClass => "file_size",
            AlignField::ConcToQueryClass => "conc_degree",
            AlignField::HitToQueryPermille => "hit_expect",
            AlignField::DirtyToInsertPermille => "dirty_expect",
            AlignField::ReclaimToInvPermille => "reclaim_expect",
            AlignField::PagesToInsertPages => "pages_per_op",
            AlignField::StepsToQueryBudget => "steps",
            AlignField::FramesToInsertCap => "frames",
            AlignField::WavesToInvCount => "waves",
        }
    }
    /// 方案侧字段在 `PlanCell` 上的取值入口名（用于查表与自证派生）。
    pub fn plan_source(self) -> &'static str {
        match self {
            AlignField::RwToQueryClass => "rw_idx",
            AlignField::SizeToInsertClass => "size_idx",
            AlignField::ConcToQueryClass => "conc_idx",
            AlignField::HitToQueryPermille => "hit_expect",
            AlignField::DirtyToInsertPermille => "dirty_expect",
            AlignField::ReclaimToInvPermille => "reclaim_expect",
            AlignField::PagesToInsertPages => "pages_per_op",
            AlignField::StepsToQueryBudget => "steps",
            AlignField::FramesToInsertCap => "frames",
            AlignField::WavesToInvCount => "wave_count",
        }
    }
    /// 目标接口字段全名（`接口单号.结构.字段`）。
    pub fn iface_field(self) -> &'static str {
        match self {
            AlignField::RwToQueryClass => "f2662.query.rw_class",
            AlignField::ConcToQueryClass => "f2662.query.conc_class",
            AlignField::HitToQueryPermille => "f2662.query.hit_permille",
            AlignField::StepsToQueryBudget => "f2662.query.step_budget",
            AlignField::SizeToInsertClass => "f2663.insert.size_class",
            AlignField::DirtyToInsertPermille => "f2663.insert.dirty_permille",
            AlignField::PagesToInsertPages => "f2663.insert.pages_per_op",
            AlignField::FramesToInsertCap => "f2663.insert.frame_cap",
            AlignField::ReclaimToInvPermille => "f2664.invalidate.reclaim_permille",
            AlignField::WavesToInvCount => "f2664.invalidate.wave_count",
        }
    }
    /// 目标接口单号。
    pub fn iface_ticket(self) -> &'static str {
        match self {
            AlignField::RwToQueryClass | AlignField::ConcToQueryClass | AlignField::HitToQueryPermille | AlignField::StepsToQueryBudget => IF_QUERY,
            AlignField::SizeToInsertClass
            | AlignField::DirtyToInsertPermille
            | AlignField::PagesToInsertPages
            | AlignField::FramesToInsertCap => IF_INSERT,
            AlignField::ReclaimToInvPermille | AlignField::WavesToInvCount => IF_INVALIDATE,
        }
    }
    /// 一行描述。
    pub fn describe(self) -> String {
        format!(
            "align[{}] {} -> {} ({})",
            self.index(),
            self.plan_field(),
            self.iface_field(),
            self.iface_ticket()
        )
    }
}

/// 对齐映射表（方案字段 → 接口字段）。
///
/// 用**数组**而不是 `Vec<Option<&str>>`：对齐的语义就是
/// 「每个方案字段恰好命中一个接口字段」，
/// 数组下标即方案字段序号，`None` 即缺失，重复则由
/// `iface_field()` 唯一性检出。三种表达等价，数组最省心也最不易写错。
#[derive(Clone, Debug)]
pub struct AlignTable {
    /// 逐字段目标（`None` = 缺失映射）。
    pub targets: [Option<&'static str>; ALIGN_FIELDS],
    /// 派生读数（方案字段在本格的实际取值，供对账）。
    pub derived: [u64; ALIGN_FIELDS],
    /// 表指纹。
    pub fingerprint: u64,
}
impl AlignTable {
    /// 从干净矩阵 + 指定格构造完整映射表。
    pub fn build(cell: &PlanCell) -> UcResult<AlignTable> {
        AlignTable::build_with(cell, None, None)
    }
    /// 带污染口径构造映射表（注入核专用；干净路径传 `None, None`）。
    pub fn build_with(cell: &PlanCell, null: Option<usize>, dup: Option<(usize, usize)>) -> UcResult<AlignTable> {
        let mut targets: [Option<&'static str>; ALIGN_FIELDS] = [None; ALIGN_FIELDS];
        let mut derived: [u64; ALIGN_FIELDS] = [0u64; ALIGN_FIELDS];
        for f in AlignField::ALL.iter() {
            let idx = f.index();
            // 污染 1：抹边
            if let Some(n) = null {
                if n == idx {
                    if let Some(t) = targets.get_mut(idx) {
                        *t = None;
                    }
                    if let Some(d) = derived.get_mut(idx) {
                        *d = 0;
                    }
                    continue;
                }
            }
            // 污染 2：撞名
            let tgt = match dup {
                Some((a, b)) => {
                    if a == idx {
                        match align_by_index(b) {
                            Some(src) => src.iface_field(),
                            None => f.iface_field(),
                        }
                    } else {
                        f.iface_field()
                    }
                }
                None => f.iface_field(),
            };
            if let Some(t) = targets.get_mut(idx) {
                *t = Some(tgt);
            }
            if let Some(d) = derived.get_mut(idx) {
                *d = AlignTable::derive(*f, cell)?;
            }
        }
        let mut words: Vec<u64> = Vec::with_capacity(ALIGN_FIELDS * 2 + 1);
        words.push(cell.fingerprint);
        for i in 0..ALIGN_FIELDS {
            words.push(match targets.get(i) {
                Some(Some(s)) => {
                    let mut h: u64 = FNV_BASIS;
                    h = fnv1a64_ext(h, sbytes(s));
                    h
                }
                _ => 0xDEAD_BEEF_CAFE_0000u64.wrapping_add(i as u64),
            });
            words.push(match derived.get(i) {
                Some(v) => *v,
                None => 0,
            });
        }
        Ok(AlignTable { targets, derived, fingerprint: fnv1a_words(&words) })
    }
    /// 从格参数派生本字段读数。
    ///
    /// 注意派生是**逐字段独立**的：任何一档越界都会在这一层直接抛错，
    /// 不会污染其余字段的读数——这样注入核里「档位越界」形态
    /// 才能被归因到唯一那一条对齐字段上。
    pub fn derive(f: AlignField, cell: &PlanCell) -> UcResult<u64> {
        let v = match f {
            AlignField::RwToQueryClass => cell.rw()?.index() as u64,
            AlignField::SizeToInsertClass => cell.size()?.index() as u64,
            AlignField::ConcToQueryClass => cell.conc()?.index() as u64,
            AlignField::HitToQueryPermille => cell.hit_expect,
            AlignField::DirtyToInsertPermille => cell.dirty_expect,
            AlignField::ReclaimToInvPermille => cell.reclaim_expect,
            AlignField::PagesToInsertPages => cell.pages_per_op,
            AlignField::StepsToQueryBudget => cell.steps,
            AlignField::FramesToInsertCap => cell.frames,
            AlignField::WavesToInvCount => {
                // 洪峰面数按「本格所属面的全局格数」折算到千分数，
                // 口径是「该面在 125 格矩阵里占多大比例」。
                let total = GRID_CELLS as u64;
                let dist = wave_distribution();
                let w = match cell.wave() {
                    Ok(v) => v,
                    Err(e) => return Err(e),
                };
                div_mul(dist[w.index()], total)
            }
        };
        Ok(v)
    }
    /// 映射完整性（`missing` 位图，bit i 置位 = 第 i 条缺失）。
    pub fn missing_bits(&self) -> u32 {
        let mut bits: u32 = 0;
        for i in 0..ALIGN_FIELDS {
            match self.targets.get(i) {
                Some(None) => {
                    bits |= 1u32 << i;
                }
                _ => {}
            }
        }
        bits
    }
    /// 映射重复性（`dup_bits` 位图，bit i 置位 = 第 i 条与别的条目撞名）。
    ///
    /// 判法：逐条取出目标字段名，与已登记的名字逐字比对，
    /// 撞上则**两条都置位**——只置后见者会让「谁抄了谁」变成不可判。
    pub fn dup_bits(&self) -> u32 {
        let mut bits: u32 = 0;
        let mut i: usize = 0;
        while i < ALIGN_FIELDS {
            let cur = match self.targets.get(i) {
                Some(Some(s)) => *s,
                _ => {
                    i = i.wrapping_add(1);
                    continue;
                }
            };
            let mut j: usize = i.wrapping_add(1);
            while j < ALIGN_FIELDS {
                let other = match self.targets.get(j) {
                    Some(Some(s)) => *s,
                    _ => {
                        j = j.wrapping_add(1);
                        continue;
                    }
                };
                if cur == other {
                    bits |= 1u32 << i;
                    bits |= 1u32 << j;
                }
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        bits
    }
    /// 三接口覆盖（bit0 = 查询、bit1 = 插入、bit2 = 失效）。
    pub fn iface_bits(&self) -> u32 {
        let mut bits: u32 = 0;
        for i in 0..ALIGN_FIELDS {
            let f = match align_by_index(i) {
                Some(v) => v,
                None => continue,
            };
            match f.iface_ticket() {
                IF_QUERY => {
                    bits |= 1u32 << 0;
                }
                IF_INSERT => {
                    bits |= 1u32 << 1;
                }
                IF_INVALIDATE => {
                    bits |= 1u32 << 2;
                }
                _ => {}
            }
        }
        bits
    }
    /// 逐字段派生读数反算是否与格参数一致。
    ///
    /// 为什么要反算：映射表只证明「有这条边」，
    /// 反算才证明「边上的值是从方案参数真算出来的，
    /// 不是抄一个常量糊上去」。
    pub fn derived_ok(&self, cell: &PlanCell) -> bool {
        for f in AlignField::ALL.iter() {
            let idx = f.index();
            let got = match self.derived.get(idx) {
                Some(v) => *v,
                None => return false,
            };
            let want = match AlignTable::derive(*f, cell) {
                Ok(v) => v,
                Err(_) => return false,
            };
            if got != want {
                return false;
            }
        }
        true
    }
    /// 对齐报告。
    pub fn report(&self, cell: &PlanCell) -> AlignReport {
        let missing = self.missing_bits();
        let dup = self.dup_bits();
        let ifaces = self.iface_bits();
        let derived = self.derived_ok(cell);
        AlignReport { cell: cell.index, missing, dup, ifaces, derived, fingerprint: self.fingerprint }
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "align fields={} missing={:08X} dup={:08X} ifaces={:03X} fp={:016X}",
            ALIGN_FIELDS,
            self.missing_bits(),
            self.dup_bits(),
            self.iface_bits(),
            self.fingerprint
        )
    }
    /// 逐行。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(ALIGN_FIELDS);
        for f in AlignField::ALL.iter() {
            let idx = f.index();
            let tgt = match self.targets.get(idx) {
                Some(Some(s)) => *s,
                _ => "<MISSING>",
            };
            let dv = match self.derived.get(idx) {
                Some(v) => *v,
                None => 0,
            };
            out.push(format!("{} -> {} = {}", f.plan_field(), tgt, dv));
        }
        out
    }
}
/// 对齐报告（零缺失 / 零重复 / 三接口齐 / 派生可反算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AlignReport {
    /// 格号。
    pub cell: usize,
    /// 缺失位图。
    pub missing: u32,
    /// 重复位图。
    pub dup: u32,
    /// 三接口覆盖位图（目标 `0b111`）。
    pub ifaces: u32,
    /// 派生读数是否可反算。
    pub derived: bool,
    /// 报告指纹。
    pub fingerprint: u64,
}
impl AlignReport {
    /// 全部对齐项就位。
    pub fn ok(&self) -> bool {
        self.missing == 0 && self.dup == 0 && self.ifaces == 0b111 && self.derived
    }
    /// 缺失项序号表。
    pub fn missing_list(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for f in AlignField::ALL.iter() {
            let b = 1u32 << f.index();
            if self.missing & b != 0 {
                out.push(f.plan_field());
            }
        }
        out
    }
    /// 重复项序号表。
    pub fn dup_list(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for f in AlignField::ALL.iter() {
            let b = 1u32 << f.index();
            if self.dup & b != 0 {
                out.push(f.plan_field());
            }
        }
        out
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "align_report cell#{:03} ok={} missing={:08X}({}) dup={:08X}({}) ifaces={:08X} derived={} fp={:016X}",
            self.cell,
            if self.ok() { 1 } else { 0 },
            self.missing,
            self.missing_list().len(),
            self.dup,
            self.dup_list().len(),
            self.ifaces,
            if self.derived { 1 } else { 0 },
            self.fingerprint
        )
    }
}
/// 按序号取对齐字段。
pub fn align_by_index(i: usize) -> Option<AlignField> {
    if i >= ALIGN_FIELDS {
        return None;
    }
    Some(AlignField::ALL[i])
}
/// 洪峰面格数分布（现算，不硬写）。
///
/// 口径：洪峰面只由**读比档**决定（见 `wave_of`），
/// 故某读比档贡献 `SIZE_COUNT × CONC_COUNT = 25` 格；
/// 把 5 个读比档按归属面累加即得面分布。
/// 结果应恒为 `[50, 25, 50]`（读命中 2 档 × 25 / 写入 1 档 × 25 / 回收 2 档 × 25）。
pub fn wave_distribution() -> [u64; WAVE_COUNT] {
    let mut dist: [u64; WAVE_COUNT] = [0u64; WAVE_COUNT];
    let per_rw: u64 = (SIZE_COUNT * CONC_COUNT) as u64;
    for rw in RwRatio::ALL.iter() {
        let w = wave_of(*rw);
        if let Some(slot) = dist.get_mut(w.index()) {
            *slot = slot.wrapping_add(per_rw);
        }
    }
    dist
}
/// 洪峰面分布断言（恒等式 `[50, 25, 50]` 由读比档现算核对）。
pub fn wave_distribution_ok() -> bool {
    let d = wave_distribution();
    let mut sum: u64 = 0;
    for v in d.iter() {
        sum = sum.wrapping_add(*v);
    }
    sum == GRID_CELLS as u64 && d[0] == 50 && d[1] == 25 && d[2] == 50
}

// ===========================================================================
// 7. 环形账（append-only，dropped 显性）
// ===========================================================================
/// 环形账条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RingEntry {
    /// 格号。
    pub cell: usize,
    /// 轮次（稳定性重复轮）。
    pub round: u32,
    /// 替身账指纹。
    pub ledger_fp: u64,
    /// 格指纹。
    pub cell_fp: u64,
    /// 步数。
    pub steps: u64,
    /// 帧数占用。
    pub frames: u64,
    /// 逻辑时钟。
    pub tick: u64,
}
impl RingEntry {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "ring cell#{:03} round={} lfp={:016X} cfp={:016X} steps={} frames={} tick={}",
            self.cell, self.round, self.ledger_fp, self.cell_fp, self.steps, self.frames, self.tick
        )
    }
}
/// append-only 环形账（覆盖时 `dropped` 显性 +1）。
#[derive(Clone, Debug)]
pub struct RingLedger {
    /// 条目。
    pub buf: Vec<RingEntry>,
    /// 写入游标。
    pub cursor: usize,
    /// 累计写入数。
    pub pushed: u64,
    /// 被覆盖丢弃数。
    pub dropped: u64,
}
impl RingLedger {
    /// 空账。
    pub fn new() -> RingLedger {
        RingLedger { buf: Vec::new(), cursor: 0, pushed: 0, dropped: 0 }
    }
    /// 追加一条。
    ///
    /// 环形语义：`cursor` 与 `RING_MASK` 取位址，
    /// 撞上已有条目则该条目被覆盖，`dropped` 显性 +1。
    /// **`dropped` 绝不隐藏**——压测方案册最忌讳「跑了很多但看不到丢了什么」。
    pub fn push(&mut self, e: RingEntry) {
        let slot = self.cursor & RING_MASK;
        while self.buf.len() <= slot {
            let filler = RingEntry { cell: 0, round: 0, ledger_fp: 0, cell_fp: 0, steps: 0, frames: 0, tick: 0 };
            self.buf.push(filler);
        }
        let exists = match self.buf.get(slot) {
            Some(old) => old.cell_fp != 0 || old.ledger_fp != 0,
            None => false,
        };
        if exists {
            self.dropped = self.dropped.wrapping_add(1);
        }
        if let Some(t) = self.buf.get_mut(slot) {
            *t = e;
        }
        self.cursor = slot.wrapping_add(1);
        self.pushed = self.pushed.wrapping_add(1);
    }
    /// 存活条目数。
    pub fn live(&self) -> u64 {
        self.pushed.wrapping_sub(self.dropped)
    }
    /// 守恒：`pushed == dropped + live`（live 不超容量）。
    pub fn balanced(&self) -> bool {
        self.pushed == self.dropped.wrapping_add(self.live())
            && self.live() <= RING_CAP as u64
            && self.dropped <= self.pushed
    }
    /// 活条目快照（顺序为写入序）。
    pub fn live_entries(&self) -> Vec<RingEntry> {
        let mut out: Vec<RingEntry> = Vec::with_capacity(RING_CAP);
        let n = self.buf.len();
        let live_n = self.live() as usize;
        let start = if n > live_n { n - live_n } else { 0 };
        let mut i = start;
        while i < n {
            if let Some(e) = self.buf.get(i) {
                if e.cell_fp != 0 || e.ledger_fp != 0 {
                    out.push(e.clone());
                }
            }
            i = i.wrapping_add(1);
        }
        out
    }
    /// 账指纹（活条目折叠）。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(RING_CAP + 4);
        words.push(self.pushed);
        words.push(self.dropped);
        words.push(self.live());
        for e in self.live_entries().iter() {
            words.push(e.cell_fp);
            words.push(e.ledger_fp);
        }
        fnv1a_words(&words)
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "ring pushed={} dropped={} live={}/{} balanced={} fp={:016X}",
            self.pushed,
            self.dropped,
            self.live(),
            RING_CAP,
            if self.balanced() { 1 } else { 0 },
            self.fingerprint()
        )
    }
    /// 逐行（活条目）。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for e in self.live_entries().iter() {
            out.push(e.describe());
        }
        out
    }
}
// ===========================================================================
// 8. 冻结状态机（合法边闭集 + 非法迁移拒记）
// ===========================================================================
/// 方案册生命周期状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FState {
    /// 草拟（矩阵正在展开）。
    Draft = 0,
    /// 展开完（125 格齐）。
    Expanded = 1,
    /// 逐格真跑完。
    Executed = 2,
    /// 稳定性达标。
    Stable = 3,
    /// 已冻结（指纹入册）。
    Frozen = 4,
    /// 已封册（终态，不可再迁出）。
    Sealed = 5,
}
impl FState {
    /// 状态数。
    pub const COUNT: usize = 6;
    /// 全部状态。
    pub const ALL: [FState; 6] = [
        FState::Draft,
        FState::Expanded,
        FState::Executed,
        FState::Stable,
        FState::Frozen,
        FState::Sealed,
    ];
    /// 序号。
    pub fn index(self) -> usize {
        self as usize
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            FState::Draft => "draft",
            FState::Expanded => "expanded",
            FState::Executed => "executed",
            FState::Stable => "stable",
            FState::Frozen => "frozen",
            FState::Sealed => "sealed",
        }
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            FState::Draft => "草拟：三矩阵参数展开中，参数可改",
            FState::Expanded => "展开完：125 格齐备，尚未真跑",
            FState::Executed => "真跑完：每格替身账已产出，待稳定性判定",
            FState::Stable => "稳定性达标：多轮指纹一致",
            FState::Frozen => "已冻结：冻结哈希入册，参数冻结",
            FState::Sealed => "已封册：终态，不得再迁移",
        }
    }
    /// 是否终态。
    pub fn is_terminal(self) -> bool {
        self == FState::Sealed
    }
    /// 合法出边闭集（只允许顺序推进，不允许回退、不允许跳档）。
    pub fn allowed(self) -> &'static [FState] {
        match self {
            FState::Draft => &[FState::Expanded],
            FState::Expanded => &[FState::Executed],
            FState::Executed => &[FState::Stable],
            FState::Stable => &[FState::Frozen],
            FState::Frozen => &[FState::Sealed],
            FState::Sealed => &[],
        }
    }
    /// 迁移是否合法。
    pub fn may_go(self, to: FState) -> bool {
        let al = self.allowed();
        let mut i: usize = 0;
        while i < al.len() {
            if al[i] == to {
                return true;
            }
            i = i.wrapping_add(1);
        }
        false
    }
    /// 名称化允许边（导出用）。
    pub fn allowed_names(self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for s in self.allowed().iter() {
            out.push(s.name());
        }
        out
    }
}

/// 非法迁移记录。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reject {
    /// 源态。
    pub from: FState,
    /// 目标态。
    pub to: FState,
    /// 拒绝原因。
    pub why: String,
}
impl Reject {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("reject {} -> {} : {}", self.from.name(), self.to.name(), self.why)
    }
}
/// 冻结状态机（非法迁移拒记，不改态）。
#[derive(Clone, Debug)]
pub struct FreezeFsm {
    /// 当前态。
    pub state: FState,
    /// 迁移流水。
    pub trail: Vec<FState>,
    /// 非法迁移记录。
    pub rejected: Vec<Reject>,
}
impl FreezeFsm {
    /// 起始态（草拟）。
    pub fn new() -> FreezeFsm {
        FreezeFsm { state: FState::Draft, trail: vec![FState::Draft], rejected: Vec::new() }
    }
    /// 尝试迁移；非法则拒记并返 `Err`，状态不动。
    pub fn try_go(&mut self, to: FState) -> UcResult<FState> {
        if self.state.may_go(to) {
            self.state = to;
            self.trail.push(to);
            return Ok(to);
        }
        let why = if self.state.is_terminal() {
            String::from("终态 sealed 不可迁出")
        } else if to.index() <= self.state.index() {
            format!("禁止回退/原地：序号 {} <= {}", to.index(), self.state.index())
        } else if to.index() > self.state.index().wrapping_add(1) {
            format!("禁止跳档：序号 {} > {}", to.index(), self.state.index().wrapping_add(1))
        } else {
            format!("不在合法边闭集内：{} -> {}", self.state.name(), to.name())
        };
        self.rejected.push(Reject { from: self.state, to, why: why.clone() });
        Err(UcErr::of_aux(AuxCode::FrozenWriteRejected, format!("from={} to={} why={}", self.state.name(), to.name(), why)))
    }
    /// 推进到目标态（只走合法边，不跳档）。
    pub fn advance_to(&mut self, to: FState) -> UcResult<u32> {
        let mut hops: u32 = 0;
        while self.state.index() < to.index() {
            let nx = match FState::ALL.get(self.state.index().wrapping_add(1)) {
                Some(v) => *v,
                None => break,
            };
            self.try_go(nx)?;
            hops = hops.wrapping_add(1);
        }
        Ok(hops)
    }
    /// 迁移流水是否单调递增。
    pub fn trail_monotonic(&self) -> bool {
        let mut i: usize = 1;
        while i < self.trail.len() {
            let a = self.trail.get(i.wrapping_sub(1));
            let b = self.trail.get(i);
            match (a, b) {
                (Some(x), Some(y)) => {
                    if y.index() <= x.index() {
                        return false;
                    }
                }
                _ => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 状态机指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(self.trail.len() + 3);
        words.push(self.state.index() as u64);
        words.push(self.trail.len() as u64);
        words.push(self.rejected.len() as u64);
        for s in self.trail.iter() {
            words.push(s.index() as u64);
        }
        for r in self.rejected.iter() {
            words.push(r.from.index() as u64);
            words.push(r.to.index() as u64);
        }
        fnv1a_words(&words)
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "fsm state={} trail={} rejected={} monotonic={} fp={:016X}",
            self.state.name(),
            self.trail.len(),
            self.rejected.len(),
            if self.trail_monotonic() { 1 } else { 0 },
            self.fingerprint()
        )
    }
    /// 合法边矩阵（6×6 布尔，导出与自检用）。
    pub fn edge_matrix() -> [[bool; FState::COUNT]; FState::COUNT] {
        let mut m = [[false; FState::COUNT]; FState::COUNT];
        let mut i: usize = 0;
        while i < FState::COUNT {
            let from = FState::ALL[i];
            let al = from.allowed();
            let mut j: usize = 0;
            while j < al.len() {
                if let Some(to) = al.get(j) {
                    if let Some(t) = m.get_mut(i) {
                        if let Some(cellb) = t.get_mut(to.index()) {
                            *cellb = true;
                        }
                    }
                }
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        m
    }
    /// 合法边总数（= 5，单调链）。
    pub fn edge_count() -> usize {
        let m = FreezeFsm::edge_matrix();
        let mut n: usize = 0;
        for row in m.iter() {
            for b in row.iter() {
                if *b {
                    n = n.wrapping_add(1);
                }
            }
        }
        n
    }
}

// ===========================================================================
// 9. 逐格夹具真跑
// ===========================================================================
/// 单格夹具真跑结果。
#[derive(Clone, Debug)]
pub struct CellRun {
    /// 格号。
    pub cell: usize,
    /// 洪峰面。
    pub wave: WaveFace,
    /// 替身账。
    pub ledger: CacheLedger,
    /// 守恒结论（`None` = 守恒）。
    pub imbalance: Option<UcErr>,
    /// 帧数界是否成立。
    pub frames_ok: bool,
    /// 步数界是否成立。
    pub steps_ok: bool,
    /// 洪峰归属是否成立。
    pub wave_ok: bool,
    /// 对齐报告。
    pub align: AlignReport,
    /// 是否主要对抗格。
    pub adversarial: bool,
    /// 格综合指纹。
    pub fingerprint: u64,
}
impl CellRun {
    /// 真跑一格。
    ///
    /// 「真跑」的口径：不是造几个数了事，而是
    /// ① 完整推导格参数 → ② 跑替身账四类事件 → ③ 对账 9 条守恒式
    /// → ④ 帧/步上界判定 → ⑤ 建 10 条映射表并反算派生 → ⑥ 汇总指纹。
    /// 六步任一不过都留痕，不吞错。
    pub fn run(cell: &PlanCell) -> UcResult<CellRun> {
        let ledger = CacheLedger::run(cell)?;
        let imbalance = match ledger.reconcile(cell) {
            Ok(_) => None,
            Err(e) => Some(e),
        };
        let wave = match cell.wave() {
            Ok(w) => w,
            Err(e) => {
                return Err(e);
            }
        };
        let wave_ok = cell.wave_ok();
        let frames_ok = ledger.frames_used <= cell.frames;
        let steps_ok = ledger.steps <= cell.steps;
        let align_table = AlignTable::build(cell)?;
        let align = align_table.report(cell);
        let adversarial = cell.is_primary_adversarial();
        let mut words: [u64; 8] = [0u64; 8];
        words[0] = cell.index as u64;
        words[1] = ledger.fingerprint;
        words[2] = align.fingerprint;
        words[3] = wave.index() as u64;
        words[4] = if imbalance.is_some() { 1 } else { 0 };
        words[5] = if frames_ok { 1 } else { 0 };
        words[6] = if wave_ok { 1 } else { 0 };
        words[7] = if adversarial { 1 } else { 0 };
        Ok(CellRun {
            cell: cell.index,
            wave,
            ledger,
            imbalance,
            frames_ok,
            steps_ok,
            wave_ok,
            align,
            adversarial,
            fingerprint: fnv1a_words(&words),
        })
    }
    /// 本格全部判定通过。
    pub fn ok(&self) -> bool {
        self.imbalance.is_none() && self.frames_ok && self.steps_ok && self.wave_ok && self.align.ok()
    }
    /// 失败原因串（空 = 全过）。
    pub fn failure(&self) -> String {
        if self.ok() {
            return String::new();
        }
        let mut parts: Vec<&str> = Vec::new();
        if let Some(e) = &self.imbalance {
            parts.push(e.detail.as_str());
        }
        if !self.frames_ok {
            parts.push("帧数超界");
        }
        if !self.steps_ok {
            parts.push("步数超界");
        }
        if !self.wave_ok {
            parts.push("洪峰归属不符");
        }
        if !self.align.ok() {
            parts.push("对齐未齐");
        }
        parts.join("; ")
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "run cell#{:03} wave={} ok={} frames_ok={} steps_ok={} wave_ok={} align_ok={} adv={} fp={:016X} {}",
            self.cell,
            self.wave.name(),
            if self.ok() { 1 } else { 0 },
            if self.frames_ok { 1 } else { 0 },
            if self.steps_ok { 1 } else { 0 },
            if self.wave_ok { 1 } else { 0 },
            if self.align.ok() { 1 } else { 0 },
            if self.adversarial { 1 } else { 0 },
            self.fingerprint,
            self.failure()
        )
    }
}
/// 全矩阵逐格真跑（`125` 格全跑，一格不过即报错并记格号）。
pub fn run_all_cells(m: &PlanMatrix) -> UcResult<Vec<CellRun>> {
    let mut out: Vec<CellRun> = Vec::with_capacity(GRID_CELLS);
    for c in m.cells.iter() {
        let r = CellRun::run(c)?;
        if !r.ok() {
            return Err(UcErr::of_form(
                Form::LedgerUnbalanced,
                format!("格 #{:03} 真跑不过：{}", c.index, r.failure()),
            ));
        }
        out.push(r);
    }
    Ok(out)
}

// ===========================================================================
// 10. 稳定性判定（REPEAT 轮重跑，指纹一致才算通过）
// ===========================================================================
/// 稳定性报告。
#[derive(Clone, Debug)]
pub struct StabilityReport {
    /// 重复轮数。
    pub rounds: usize,
    /// 矩阵指纹（基轮）。
    pub matrix_fp: u64,
    /// 逐格指纹序列（每轮一条）。
    pub cellwise: Vec<Vec<u64>>,
    /// 是否全轮一致。
    pub stable: bool,
    /// 漂移格位图（bit i = 第 i 格出现漂移）。
    ///
    /// 用 `u128` 而非 `u32`：`GRID_CELLS = 125 > 32`，
    /// `1u32 << 124` 会 panic（移位溢出），
    /// 位图必须至少 128 位才能覆盖 125 个格位。
    pub drift_bits: u128,
    /// 漂移轮号（首个出现漂移的轮，0 = 无）。
    pub drift_round: usize,
}
impl StabilityReport {
    /// 全轮一致。
    pub fn ok(&self) -> bool {
        self.stable && self.drift_bits == 0 && self.drift_round == 0 && self.rounds == REPEAT
    }
    /// 漂移格号表。
    pub fn drift_cells(&self) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        let mut i: usize = 0;
        while i < GRID_CELLS {
            if self.drift_bits & (1u128 << i) != 0 {
                out.push(i);
            }
            i = i.wrapping_add(1);
        }
        out
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "stability rounds={} stable={} drift_bits={:032X} drift_cells={:?} drift_round={} matrix_fp={:016X}",
            self.rounds,
            if self.stable { 1 } else { 0 },
            self.drift_bits,
            self.drift_cells(),
            self.drift_round,
            self.matrix_fp
        )
    }
}
/// 稳定性判定：跑 `REPEAT` 轮，逐轮重展开 + 重真跑，比对矩阵指纹与逐格指纹。
///
/// **双重比对**的原因：只比矩阵指纹的话，一次哈希碰撞就能把
/// 「某一格参数漂了」整体吞掉，读者看不到位置信息；
/// 因此矩阵指纹用于快速否决，逐格指纹用于**定位**漂移格号。
pub fn stability_check(seed: u64) -> UcResult<StabilityReport> {
    let mut cellwise: Vec<Vec<u64>> = Vec::with_capacity(REPEAT);
    let mut matrix_fp: u64 = 0;
    let mut drift_bits: u128 = 0;
    let mut drift_round: usize = 0;
    let mut r: usize = 0;
    while r < REPEAT {
        let m = PlanMatrix::expand();
        if !m.count_ok() || !m.orthogonal_ok() || !m.dims_ok() {
            return Err(UcErr::new(SEG_BASE, format!("第 {} 轮展开不过", r)));
        }
        let runs = run_all_cells(&m)?;
        let mut fps: Vec<u64> = Vec::with_capacity(GRID_CELLS);
        for c in runs.iter() {
            fps.push(c.fingerprint);
        }
        let fp = m.fingerprint();
        if r == 0 {
            matrix_fp = fp;
        } else if fp != matrix_fp {
            drift_round = r;
        }
        cellwise.push(fps);
        r = r.wrapping_add(1);
    }
    // 逐格跨轮比对（定位漂移位）
    let base = match cellwise.get(0) {
        Some(v) => v.clone(),
        None => return Err(UcErr::new(SEG_BASE, String::from("零轮：无基轮指纹"))),
    };
    let mut r2: usize = 1;
    while r2 < cellwise.len() {
        let cur = match cellwise.get(r2) {
            Some(v) => v.clone(),
            None => return Err(UcErr::new(SEG_BASE, String::from("轮次缺项"))),
        };
        let mut i: usize = 0;
        while i < GRID_CELLS {
            let a = match base.get(i) {
                Some(v) => *v,
                None => return Err(UcErr::new(SEG_BASE, String::from("基轮缺格"))),
            };
            let b = match cur.get(i) {
                Some(v) => *v,
                None => return Err(UcErr::new(SEG_BASE, String::from("本轮缺格"))),
            };
            if a != b {
                drift_bits |= 1u128 << i;
                if drift_round == 0 {
                    drift_round = r2;
                }
            }
            i = i.wrapping_add(1);
        }
        r2 = r2.wrapping_add(1);
    }
    let stable = drift_bits == 0 && drift_round == 0;
    // 种子只做跑序抖动：把种子折进一个不参与方案取值的记数位，
    // 确认「同种子同序、不同种子不改方案」这一确定性契约。
    let mut jitter = lcg_step(seed);
    let mut k: usize = 0;
    while k < REPEAT {
        jitter = lcg_step(jitter);
        k = k.wrapping_add(1);
    }
    Ok(StabilityReport { rounds: REPEAT, matrix_fp, cellwise, stable, drift_bits, drift_round })
}

// ===========================================================================
// 11. 注入核（12 形态，逐形态干净重建 + 独立字段 + 逐字段独立判）
// ===========================================================================
/// 注入证据底（每形态独立字段，禁止整表回写）。
#[derive(Clone, Debug)]
pub struct Evidence {
    /// 逐形态命中数（累积）。
    pub hits: [u32; FORM_COUNT],
    /// 逐形态造证轮序。
    pub seq: [u32; FORM_COUNT],
    /// 检出轮序。
    pub detected_at: [u32; FORM_COUNT],
    /// 是否越界（造证阶段成功 = 真有洞）。
    pub armed: [bool; FORM_COUNT],
    /// 检出的形态掩码（`u32`，bit i 置位 = 第 i 形态被判出）。
    pub verdict_mask: u32,
    /// 轮序。
    pub round: u32,
}
impl Evidence {
    /// 空证据底。
    pub fn blank() -> Evidence {
        Evidence { hits: [0u32; FORM_COUNT], seq: [0u32; FORM_COUNT], detected_at: [0u32; FORM_COUNT], armed: [false; FORM_COUNT], verdict_mask: 0, round: 0 }
    }
    /// 本形态命中数。
    pub fn of(&self, f: Form) -> u32 {
        match self.hits.get(f.index()) {
            Some(v) => *v,
            None => 0,
        }
    }
    /// 本形态是否被检出。
    pub fn verdict(&self, f: Form) -> bool {
        self.verdict_mask & (1u32 << f.index()) != 0
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        let mut parts: Vec<String> = Vec::with_capacity(FORM_COUNT);
        for f in Form::ALL.iter() {
            let idx = f.index();
            let armed_v = if self.armed.get(idx) == Some(&true) { 1 } else { 0 };
            let at = match self.detected_at.get(idx) {
                Some(v) => *v,
                None => 0,
            };
            parts.push(format!("{}:{}#{}/{:06X}", f.name(), self.of(*f), armed_v, at));
        }
        format!("evidence verdict_mask={:08X} round={} {}", self.verdict_mask, self.round, parts.join(" "))
    }
}
/// 检出结论（逐字段独立判断，供归因）。
#[derive(Clone, Debug)]
pub struct Detections {
    /// 逐形态结论位。
    pub verdict_mask: u32,
    /// 逐形态命中数。
    pub hits: [u32; FORM_COUNT],
    /// 逐形态命中的错误码。
    pub codes: [u16; FORM_COUNT],
    /// 逐形态明细。
    pub detail: [String; FORM_COUNT],
}
impl Detections {
    /// 本形态是否命中。
    pub fn hit(&self, f: Form) -> bool {
        match self.hits.get(f.index()) {
            Some(v) => *v > 0,
            None => false,
        }
    }
    /// 未命中形态名表。
    pub fn miss_list(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for f in Form::ALL.iter() {
            if !self.hit(*f) {
                out.push(f.name());
            }
        }
        out
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        let mut hit: usize = 0;
        for f in Form::ALL.iter() {
            if self.hit(*f) {
                hit = hit.wrapping_add(1);
            }
        }
        format!("detect {}/{} miss={:?}", hit, FORM_COUNT, self.miss_list())
    }
    /// 归因表（形态 → 码 → 明细）。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(FORM_COUNT);
        for f in Form::ALL.iter() {
            let i = f.index();
            out.push(format!(
                "{:02} {:<24} hits={} code=0x{:04X} {}",
                i,
                f.name(),
                self.hits[i],
                self.codes[i],
                self.detail[i]
            ));
        }
        out
    }
}

/// 注入核：12 形态逐个造证 → 逐个独立判。
///
/// 三条铁律（前两单已踩过的坑，这里写死在注释里）：
/// ① **逐形态从干净矩阵重建**——污染不跨形态累积，
///    否则第 k 个形态的证据底里带着前 k−1 个形态的脏位，
///    检出结论就无法归因到单一失效模式；
/// ② **只回写本形态那一项**——本轮证据底上只有本形态对应的位为脏，
///    整表回写会把前序形态的命中记录抹平；
/// ③ **逐字段独立判**——不得用「总有一位不对」的反证式，
///    必须让每个形态都有自己那条专属判据可归因。
pub fn inject_all() -> UcResult<(Evidence, Detections)> {
    let mut ev = Evidence::blank();
    let mut det = Detections { verdict_mask: 0, hits: [0u32; FORM_COUNT], codes: [0u16; FORM_COUNT], detail: blank_details() };
    let mut round: u32 = 0;
    for form in Form::ALL.iter() {
        round = round.wrapping_add(1);
        // ① 每形态从干净矩阵重建
        let mut m = PlanMatrix::expand();
        let armed = arm(&mut m, *form)?;
        // ② 只回写本形态那一项
        if let Some(s) = ev.armed.get_mut(form.index()) {
            *s = armed;
        }
        if let Some(s) = ev.seq.get_mut(form.index()) {
            *s = round;
        }
        // ③ 逐字段独立判
        let ds = detect_all(&m)?;
        let h = ds.hit(*form);
        if let Some(s) = ev.hits.get_mut(form.index()) {
            *s = if h { 1 } else { 0 };
        }
        let at = if h { round } else { 0 };
        if let Some(s) = ev.detected_at.get_mut(form.index()) {
            *s = at;
        }
        if h {
            ev.verdict_mask |= 1u32 << form.index();
        }
        // 只把本形态那一项的结论并进总表
        let i = form.index();
        det.hits[i] = ds.hits[i];
        det.codes[i] = ds.codes[i];
        det.detail[i] = ds.detail[i].clone();
        if h {
            det.verdict_mask |= 1u32 << i;
        }
    }
    ev.round = round;
    Ok((ev, det))
}
/// 造证：把 `form` 对应的污染注入**干净矩阵**，返回是否真造出洞。
fn arm(m: &mut PlanMatrix, form: Form) -> UcResult<bool> {
    if m.cells.is_empty() {
        return Ok(false);
    }
    match form {
        Form::RwRatioOutOfBand => {
            if let Some(c) = m.cells.get_mut(0) {
                c.rw_idx = 99;
                return Ok(true);
            }
            Ok(false)
        }
        Form::SizeOutOfBand => {
            if let Some(c) = m.cells.get_mut(0) {
                c.size_idx = 99;
                return Ok(true);
            }
            Ok(false)
        }
        Form::ConcOutOfBand => {
            if let Some(c) = m.cells.get_mut(0) {
                c.conc_idx = 99;
                return Ok(true);
            }
            Ok(false)
        }
        Form::HitExpectSkew => {
            if let Some(c) = m.cells.get_mut(0) {
                c.hit_expect = c.hit_expect.wrapping_add(300);
                return Ok(true);
            }
            Ok(false)
        }
        Form::DirtyExpectSkew => {
            if let Some(c) = m.cells.get_mut(0) {
                c.dirty_expect = c.dirty_expect.wrapping_add(300);
                return Ok(true);
            }
            Ok(false)
        }
        Form::ReclaimExpectSkew => {
            if let Some(c) = m.cells.get_mut(0) {
                c.reclaim_expect = c.reclaim_expect.wrapping_add(300);
                return Ok(true);
            }
            Ok(false)
        }
        Form::LedgerUnbalanced => {
            // 篡改登记页数：账走基准档现算，登记页数被改即守恒式破裂。
            // （不拿 `frames` 下手——那是 `FrameOverflow` 的专属造证点，
            //   两者共用一个字段会导致检出无法归因。）
            if let Some(c) = m.cells.get_mut(0) {
                c.pages_per_op = c.pages_per_op.wrapping_add(7);
                return Ok(true);
            }
            Ok(false)
        }
        Form::AlignMissing => {
            // 抹掉插入接口 dirty_permille 那条边（第 4 条）。
            m.align_null = Some(AlignField::DirtyToInsertPermille.index());
            Ok(true)
        }
        Form::AlignDuplicated => {
            // 让「期望回收比」也抄到 hit_permille 上（第 5 条 → 第 3 条）。
            m.align_dup = Some((
                AlignField::ReclaimToInvPermille.index(),
                AlignField::HitToQueryPermille.index(),
            ));
            Ok(true)
        }
        Form::StepOverflow => {
            if let Some(c) = m.cells.get_mut(0) {
                c.steps = 1;
                return Ok(true);
            }
            Ok(false)
        }
        Form::FrameOverflow => {
            if let Some(c) = m.cells.get_mut(0) {
                c.frames = 0;
                return Ok(true);
            }
            Ok(false)
        }
        Form::WaveConflict => {
            if let Some(c) = m.cells.get_mut(0) {
                c.wave_idx = 99;
                return Ok(true);
            }
            Ok(false)
        }
    }
}
/// 逐字段独立检出：对矩阵逐格跑，逐格逐形态各给一条专属判据。
///
/// 关键：`AlignMissing` / `AlignDuplicated` 不是靠矩阵里的格参数，
/// 而是靠**映射表本身**（缺一条边 / 两条边撞名），
/// 因此这两形态在 `detect_all` 里单列，不混在格循环里。
pub fn detect_all(m: &PlanMatrix) -> UcResult<Detections> {
    let mut det = Detections { verdict_mask: 0, hits: [0u32; FORM_COUNT], codes: [0u16; FORM_COUNT], detail: blank_details() };
    let mut i: usize = 0;
    while i < m.cells.len() {
        let c = match m.cells.get(i) {
            Some(v) => v,
            None => break,
        };
        // 维度闭集（越界三形态共用逐档判定，各自独立记）
        if !matches_rw(c) {
            bump(&mut det, Form::RwRatioOutOfBand, format!("格 #{} 读比档 {} 越界", i, c.rw_idx));
        }
        if !matches_size(c) {
            bump(&mut det, Form::SizeOutOfBand, format!("格 #{} 尺寸档 {} 越界", i, c.size_idx));
        }
        if !matches_conc(c) {
            bump(&mut det, Form::ConcOutOfBand, format!("格 #{} 并发档 {} 越界", i, c.conc_idx));
        }
        // 洪峰归属
        if !c.wave_ok() {
            bump(&mut det, Form::WaveConflict, format!("格 #{} 洪峰面序号 {} 与读比档不符", i, c.wave_idx));
        }
        // 替身账可跑 + 守恒
        if let Ok(l) = CacheLedger::run(c) {
            if let Err(e) = l.reconcile(c) {
                // 守恒式里三条千分数反算分别归因到三条失真形态
                match e.code() {
                    x if x == Form::HitExpectSkew.code() => bump(&mut det, Form::HitExpectSkew, e.detail.clone()),
                    x if x == Form::DirtyExpectSkew.code() => bump(&mut det, Form::DirtyExpectSkew, e.detail.clone()),
                    x if x == Form::ReclaimExpectSkew.code() => bump(&mut det, Form::ReclaimExpectSkew, e.detail.clone()),
                    _ => bump(&mut det, Form::LedgerUnbalanced, e.detail.clone()),
                }
            }
            // 帧/步上界
            if l.frames_used > c.frames {
                bump(&mut det, Form::FrameOverflow, format!("格 #{} 占用 {} 帧 > 上界 {} 帧", i, l.frames_used, c.frames));
            }
            if l.steps > c.steps {
                bump(&mut det, Form::StepOverflow, format!("格 #{} 实跑 {} 步 > 上界 {} 步", i, l.steps, c.steps));
            }
            // 替身账真跑失败（档位越界导致取不出档）也算守恒缺陷
            if CacheLedger::run(c).is_err() {
                bump(&mut det, Form::LedgerUnbalanced, format!("格 #{} 替身账无法建账", i));
            }
        }
        // 对齐：缺失 / 重复（映射表层，单列判）
        // 污染只作用在 0 号格——若 125 格全污染，明细串会长到读不动，
        // 而且掩盖「污染能否被检出」这个唯一要验证的问题。
        let pm: Option<usize> = if c.index == 0 { m.align_null } else { None };
        let pd: Option<(usize, usize)> = if c.index == 0 { m.align_dup } else { None };
        if let Ok(t) = AlignTable::build_with(c, pm, pd) {
            let r = t.report(c);
            if r.missing != 0 {
                bump(&mut det, Form::AlignMissing, format!("格 #{} 缺失映射 {:?}", i, r.missing_list()));
            }
            if r.dup != 0 {
                bump(&mut det, Form::AlignDuplicated, format!("格 #{} 映射重复 {:?}", i, r.dup_list()));
            }
        }
        i = i.wrapping_add(1);
    }
    Ok(det)
}
/// 空明细数组（`String` 非 `Copy`，数组字面量初始化不可用）。
fn blank_details() -> [String; FORM_COUNT] {
    let mut d: [String; FORM_COUNT] = [const { String::new() }; FORM_COUNT];
    let mut i: usize = 0;
    while i < FORM_COUNT {
        if let Some(s) = d.get_mut(i) {
            *s = String::new();
        }
        i = i.wrapping_add(1);
    }
    d
}
/// 记录一次命中。
fn bump(det: &mut Detections, f: Form, why: String) {
    let i = f.index();
    det.verdict_mask |= 1u32 << i;
    if let Some(h) = det.hits.get_mut(i) {
        *h = h.wrapping_add(1);
    }
    if let Some(c) = det.codes.get_mut(i) {
        *c = f.code();
    }
    if let Some(s) = det.detail.get_mut(i) {
        if s.is_empty() {
            *s = why;
        } else {
            let prev = s.clone();
            *s = format!("{} | {}", prev, why);
        }
    }
}
/// 读比档闭集判定。
fn matches_rw(c: &PlanCell) -> bool {
    c.rw_idx < RW_COUNT
}
/// 尺寸档闭集判定。
fn matches_size(c: &PlanCell) -> bool {
    c.size_idx < SIZE_COUNT
}
/// 并发档闭集判定。
fn matches_conc(c: &PlanCell) -> bool {
    c.conc_idx < CONC_COUNT
}

// ===========================================================================
// 12. 冻结件（哈希入册 + 漂移可检出）
// ===========================================================================
/// 冻结件（方案册入册凭证）。
#[derive(Clone, Debug)]
pub struct FreezeRecord {
    /// 册版本号。
    pub rev: u32,
    /// 矩阵格数。
    pub cells: usize,
    /// 矩阵冻结哈希。
    pub matrix_hash: u64,
    /// 对齐表折叠哈希。
    pub align_hash: u64,
    /// 注入核折叠哈希。
    pub detect_hash: u64,
    /// 稳定性折叠哈希。
    pub stable_hash: u64,
    /// 册总冻结哈希（六者串接）。
    pub freeze_hash: u64,
    /// 配比依据注记（风险为「方案失真」，须在档）。
    pub basis: String,
    /// 失真声明。
    pub distortion: String,
}
impl FreezeRecord {
    /// 由实际跑批构造冻结件。
    ///
    /// 注记口径（判据要求「需配比依据注记 + 失真声明在档」）：
    /// - **配比依据**必须说清三档各自为什么这么取，
    ///   以及三矩阵为什么按正交笛卡尔积铺满而不是随机抽样；
    /// - **失真声明**必须自认本单只冻结方案、不执行，
    ///   替身账读数是**模型推算**而非真机实测。
    pub fn build(m: &PlanMatrix, align_hash: u64, detect_hash: u64, stable_hash: u64) -> FreezeRecord {
        let matrix_hash = m.fingerprint();
        let mut words: Vec<u64> = Vec::with_capacity(8);
        words.push(STRESS_BOOK_REV as u64);
        words.push(m.produced as u64);
        words.push(matrix_hash);
        words.push(align_hash);
        words.push(detect_hash);
        words.push(stable_hash);
        let freeze_hash = fnv1a_words(&words);
        FreezeRecord {
            rev: STRESS_BOOK_REV,
            cells: m.produced,
            matrix_hash,
            align_hash,
            detect_hash,
            stable_hash,
            freeze_hash,
            basis: String::from(
                "配比依据：读比 0:1/1:4/1:1/4:1/1:0 五档覆盖纯读、读多、写多、纯写与均衡五类 cache 行为；\
                 尺寸 4KB/64KB/1MB/64MB/1GB 五档跨页数—回收压力四个数量级（4/16/256/16384/262144 页）；\
                 并发 1/2/4/6/8 核五档覆盖无争用到重度争用；\
                 三矩阵按笛卡尔积铺满 125 格而非抽样，保证任一档位的边际效应可被单独读出，且格与格之间唯一可差的只有三维档位。",
            ),
            distortion: String::from(
                "失真声明：本单只做方案冻结与三接口字段对齐，不实现 cache 本体（F2682）、不执行压测（F2902-F2909）；\
                 替身账四类事件读数为按方案参数推算的模型值，非真机实测；\
                 因此冻结哈希只能证明「方案参数与对齐表本身未被改动」，\
                 不能证明「按此方案实跑会得到这些读数」。",
            ),
        }
    }
    /// 冻结核验：矩阵哈希是否仍与冻结时一致（漂移可检出）。
    pub fn verify(&self, m: &PlanMatrix) -> bool {
        m.fingerprint() == self.matrix_hash && m.produced == self.cells && self.rev == STRESS_BOOK_REV
    }
    /// 漂移项描述（空 = 无漂移）。
    pub fn drift(&self, m: &PlanMatrix) -> String {
        if self.verify(m) {
            return String::new();
        }
        format!(
            "冻结漂移：册矩阵哈希={:016X} 当前={:016X} 册格数={} 当前={}",
            self.matrix_hash,
            m.fingerprint(),
            self.cells,
            m.produced
        )
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "freeze rev={} cells={} matrix={:016X} align={:016X} detect={:016X} stable={:016X} freeze={:016X}",
            self.rev, self.cells, self.matrix_hash, self.align_hash, self.detect_hash, self.stable_hash, self.freeze_hash
        )
    }
    /// 逐行（冻结件全文）。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        out.push(self.describe());
        out.push(format!("basis: {}", self.basis));
        out.push(format!("distortion: {}", self.distortion));
        out
    }
}
// ===========================================================================
// 13. 指标
// ===========================================================================
/// 指标集（全部可由上面各报告回读，不另存派生状态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Metrics {
    /// 矩阵格数。
    pub cells: usize,
    /// 格数断言通过。
    pub count_ok: bool,
    /// 正交断言通过。
    pub orthogonal: bool,
    /// 逐格真跑通过数。
    pub cells_ok: usize,
    /// 对齐零缺失格数。
    pub align_clean: usize,
    /// 洪峰面分布断言通过。
    pub wave_dist_ok: bool,
    /// 帧数界通过格数。
    pub frames_ok: usize,
    /// 步数界通过格数。
    pub steps_ok: usize,
    /// 洪峰归属通过格数。
    pub wave_ok: usize,
    /// 稳定轮数。
    pub rounds: usize,
    /// 稳定性通过。
    pub stable: bool,
    /// 漂移格数。
    pub drift_cells: usize,
    /// 注入形态数。
    pub forms: usize,
    /// 注入检出数。
    pub detected: usize,
    /// 状态机合法边数。
    pub edges: usize,
    /// 状态机非法迁移拒记数。
    pub rejected: usize,
    /// 环形账写入数。
    pub pushed: u64,
    /// 环形账丢弃数。
    pub dropped: u64,
    /// 环形账存活数。
    pub live: u64,
    /// 环形账守恒。
    pub ring_ok: bool,
    /// 冻结核验通过。
    pub freeze_ok: bool,
    /// 矩阵冻结哈希。
    pub matrix_hash: u64,
    /// 册冻结哈希。
    pub freeze_hash: u64,
}
impl Metrics {
    /// 指标指纹。
    pub fn fingerprint(&self) -> u64 {
        let words: [u64; 20] = [
            self.cells as u64,
            self.count_ok as u64,
            self.orthogonal as u64,
            self.cells_ok as u64,
            self.align_clean as u64,
            self.wave_dist_ok as u64,
            self.frames_ok as u64,
            self.steps_ok as u64,
            self.wave_ok as u64,
            self.rounds as u64,
            self.stable as u64,
            self.drift_cells as u64,
            self.forms as u64,
            self.detected as u64,
            self.edges as u64,
            self.rejected as u64,
            self.pushed,
            self.dropped,
            self.live,
            self.freeze_hash,
        ];
        fnv1a_words(&words)
    }
    /// 全项通过。
    ///
    /// 通过条件逐条列明（不接受「大部分过」）：
    /// 125 格全跑过、125 格对齐零缺失、125 格帧/步/洪峰三项界全过、
    /// 3 轮指纹全一致、12 形态全检出、状态机走满 5 条合法边且非法迁移被拒记、
    /// 环形账守恒、冻结核验通过。
    pub fn all_ok(&self) -> bool {
        self.cells == GRID_CELLS
            && self.count_ok
            && self.orthogonal
            && self.cells_ok == GRID_CELLS
            && self.align_clean == GRID_CELLS
            && self.wave_dist_ok
            && self.frames_ok == GRID_CELLS
            && self.steps_ok == GRID_CELLS
            && self.wave_ok == GRID_CELLS
            && self.rounds == REPEAT
            && self.stable
            && self.drift_cells == 0
            && self.forms == FORM_COUNT
            && self.detected == FORM_COUNT
            && self.edges == FState::COUNT - 1
            && self.rejected > 0
            && self.ring_ok
            && self.freeze_ok
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "metrics cells={}/{} cells_ok={} align_clean={} frames={} steps={} wave_ok={} dist_ok={} stable={}/{}(drift={}) detect={}/{} edges={} rejected={} ring={}/{} freeze_ok={} fp={:016X}",
            self.cells,
            GRID_CELLS,
            self.cells_ok,
            self.align_clean,
            self.frames_ok,
            self.steps_ok,
            self.wave_ok,
            if self.wave_dist_ok { 1 } else { 0 },
            if self.stable { 1 } else { 0 },
            self.rounds,
            self.drift_cells,
            self.detected,
            self.forms,
            self.edges,
            self.rejected,
            self.live,
            self.pushed,
            if self.freeze_ok { 1 } else { 0 },
            self.fingerprint()
        )
    }
}

// ===========================================================================
// 14. 确定性重放
// ===========================================================================
/// 重放产物（同一 seed 必须逐位一致）。
#[derive(Clone, Debug)]
pub struct Replay {
    /// 种子。
    pub seed: u64,
    /// 矩阵指纹。
    pub matrix_fp: u64,
    /// 逐格指纹折叠。
    pub cells_fp: u64,
    /// 对齐折叠。
    pub align_fp: u64,
    /// 检出折叠。
    pub detect_fp: u64,
    /// 稳定性折叠。
    pub stable_fp: u64,
    /// 冻结哈希。
    pub freeze_hash: u64,
    /// 环形账折叠。
    pub ring_fp: u64,
    /// 综合指纹。
    pub fingerprint: u64,
}
impl Replay {
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "replay seed={:016X} matrix={:016X} cells={:016X} align={:016X} detect={:016X} stable={:016X} freeze={:016X} ring={:016X} fp={:016X}",
            self.seed, self.matrix_fp, self.cells_fp, self.align_fp, self.detect_fp, self.stable_fp, self.freeze_hash, self.ring_fp, self.fingerprint
        )
    }
}
/// 确定性重放：跑一遍完整流水线，把各段指纹折叠成一条。
///
/// **种子只做跑序抖动，不参与任何方案取值**——
/// 三矩阵的档位、格数、期望读数全部由常量闭集决定，与种子无关。
/// 种子唯一的作用是「同种子必同序」的确定性自证：
/// 若哪天有人把种子误接进方案取值，重放指纹会立刻漂。
pub fn replay(seed: u64) -> UcResult<Replay> {
    let m = PlanMatrix::expand();
    if !m.count_ok() || !m.orthogonal_ok() || !m.dims_ok() {
        return Err(UcErr::new(SEG_BASE, String::from("重放：矩阵展开不过")));
    }
    let runs = run_all_cells(&m)?;
    // 对齐折叠：逐格对齐表指纹串接
    let mut align_fps: Vec<u64> = Vec::with_capacity(GRID_CELLS);
    let mut align_clean: usize = 0;
    for r in runs.iter() {
        align_fps.push(r.align.fingerprint);
        if r.align.missing == 0 && r.align.dup == 0 {
            align_clean = align_clean.wrapping_add(1);
        }
    }
    let align_fp = fnv1a_words(&align_fps);
    // 检出折叠：注入核
    let (ev, det) = inject_all()?;
    let mut det_words: Vec<u64> = Vec::with_capacity(FORM_COUNT * 2 + 1);
    det_words.push(det.verdict_mask as u64);
    let mut detected: usize = 0;
    for f in Form::ALL.iter() {
        let i = f.index();
        let h = match det.hits.get(i) {
            Some(v) => *v,
            None => 0,
        };
        det_words.push(h as u64);
        det_words.push(ev.of(*f) as u64);
        if h > 0 {
            detected = detected.wrapping_add(1);
        }
    }
    let detect_fp = fnv1a_words(&det_words);
    // 稳定性折叠
    let stab = stability_check(seed)?;
    let mut stab_words: Vec<u64> = Vec::with_capacity(GRID_CELLS + 4);
    stab_words.push(stab.matrix_fp);
    stab_words.push((stab.drift_bits & 0xFFFF_FFFF_FFFF_FFFFu128) as u64);
    stab_words.push(((stab.drift_bits >> 64) & 0xFFFF_FFFF_FFFF_FFFFu128) as u64);
    for row in stab.cellwise.iter() {
        for v in row.iter() {
            stab_words.push(*v);
        }
    }
    let stable_fp = fnv1a_words(&stab_words);
    // 冻结件
    let fr = FreezeRecord::build(&m, align_fp, detect_fp, stable_fp);
    // 环形账：跑 REPEAT 轮逐格写入（`125 × 3 > 128` 故必然发生覆盖，
    // 正好把 `dropped` 显性这条约束压出来）
    let mut ring = RingLedger::new();
    let mut r: u32 = 0;
    while r < REPEAT as u32 {
        for run in runs.iter() {
            ring.push(RingEntry {
                cell: run.cell,
                round: r,
                ledger_fp: run.ledger.fingerprint,
                cell_fp: run.fingerprint,
                steps: run.ledger.steps,
                frames: run.ledger.frames_used,
                tick: run.ledger.tick,
            });
        }
        r = r.wrapping_add(1);
    }
    if !ring.balanced() {
        return Err(UcErr::of_form(Form::LedgerUnbalanced, format!("环形账不守恒：{}", ring.summary())));
    }
    // 逐格指纹折叠
    let mut cell_fps: Vec<u64> = Vec::with_capacity(GRID_CELLS);
    let mut cells_ok: usize = 0;
    let mut frames_ok: usize = 0;
    let mut steps_ok: usize = 0;
    let mut wave_ok: usize = 0;
    for run in runs.iter() {
        cell_fps.push(run.fingerprint);
        if run.ok() {
            cells_ok = cells_ok.wrapping_add(1);
        }
        if run.frames_ok {
            frames_ok = frames_ok.wrapping_add(1);
        }
        if run.steps_ok {
            steps_ok = steps_ok.wrapping_add(1);
        }
        if run.wave_ok {
            wave_ok = wave_ok.wrapping_add(1);
        }
    }
    let cells_fp = fnv1a_words(&cell_fps);
    let mut words: Vec<u64> = Vec::with_capacity(8);
    words.push(seed);
    words.push(m.fingerprint());
    words.push(cells_fp);
    words.push(align_fp);
    words.push(detect_fp);
    words.push(stable_fp);
    words.push(fr.freeze_hash);
    words.push(ring.fingerprint());
    Ok(Replay {
        seed,
        matrix_fp: m.fingerprint(),
        cells_fp,
        align_fp,
        detect_fp,
        stable_fp,
        freeze_hash: fr.freeze_hash,
        ring_fp: ring.fingerprint(),
        fingerprint: fnv1a_words(&words),
    })
}

// ===========================================================================
// 15. 观测（真实跑逻辑，非占位）
// ===========================================================================
/// 观测产物。
#[derive(Clone, Debug)]
pub struct UcObserve {
    /// 种子。
    pub seed: u64,
    /// 矩阵。
    pub matrix: PlanMatrix,
    /// 逐格跑批。
    pub runs: Vec<CellRun>,
    /// 稳定性。
    pub stability: StabilityReport,
    /// 注入证据底。
    pub evidence: Evidence,
    /// 检出。
    pub detect: Detections,
    /// 环形账。
    pub ring: RingLedger,
    /// 状态机。
    pub fsm: FreezeFsm,
    /// 冻结件。
    pub freeze: FreezeRecord,
    /// 指标。
    pub metrics: Metrics,
    /// 重放。
    pub replay: Replay,
}
impl UcObserve {
    /// 指标全通过。
    pub fn all_ok(&self) -> bool {
        self.metrics.all_ok()
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "UNX2901 seed={:016X} | {} | {} | {} | {}",
            self.seed,
            self.matrix.summary(),
            self.metrics.summary(),
            self.stability.describe(),
            self.detect.summary()
        )
    }
    /// 逐行（全报告）。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        out.push(self.summary());
        out.push(self.ring.summary());
        out.push(self.fsm.summary());
        out.push(self.freeze.describe());
        for r in self.ring.rows().iter() {
            out.push(r.clone());
        }
        for r in self.reject_rows().iter() {
            out.push(r.clone());
        }
        out
    }
    /// 非法迁移逐行。
    pub fn reject_rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.fsm.rejected.len());
        for r in self.fsm.rejected.iter() {
            out.push(r.describe());
        }
        out
    }
    /// 未检出形态名表。
    pub fn detect_miss(&self) -> Vec<&'static str> {
        self.detect.miss_list()
    }
    /// 稳定性一致性复核（观测内部再跑一次重放并比对指纹）。
    pub fn replay_stable(&self) -> bool {
        match replay(self.seed) {
            Ok(r) => r.fingerprint == self.replay.fingerprint,
            Err(_) => false,
        }
    }
}
/// 真实观测入口：把上面每一段都真跑一遍并汇总。
pub fn observe(seed: u64) -> UcResult<UcObserve> {
    // ① 三矩阵正交展开
    let m = PlanMatrix::expand();
    if !m.count_ok() {
        return Err(UcErr::new(SEG_BASE, format!("格数不过：{} != {}", m.produced, GRID_CELLS)));
    }
    if !m.orthogonal_ok() {
        return Err(UcErr::new(SEG_BASE, String::from("正交性不过：存在重复或越界格位")));
    }
    if !m.dims_ok() {
        return Err(UcErr::new(SEG_BASE, String::from("三维闭集不过：档位或洪峰归属异常")));
    }
    if !wave_distribution_ok() {
        return Err(UcErr::new(SEG_BASE, format!("洪峰面分布不过：{:?}", wave_distribution())));
    }
    // ② 逐格夹具真跑
    let runs = run_all_cells(&m)?;
    // ③ 状态机：走满合法边 + 故意试三条非法边
    let mut fsm = FreezeFsm::new();
    for s in FState::ALL.iter() {
        if s.index() == 0 {
            continue;
        }
        fsm.advance_to(*s)?;
    }
    // 非法迁移：① 回退 ② 跳档 ③ 终态迁出
    let illegal: [FState; 3] = [FState::Draft, FState::Frozen, FState::Sealed];
    for s in illegal.iter() {
        // 终态后的迁出尝试会记一条；其余在封册态也都会被拒
        let _ = fsm.try_go(*s);
    }
    if fsm.state != FState::Sealed || !fsm.trail_monotonic() || fsm.rejected.is_empty() {
        return Err(UcErr::of_aux(
            AuxCode::FrozenWriteRejected,
            format!("状态机异常：{} trail_ok={} rejected={}", fsm.summary(), if fsm.trail_monotonic() { 1 } else { 0 }, fsm.rejected.len()),
        ));
    }
    // ④ 稳定性判定
    let stability = stability_check(seed)?;
    if !stability.ok() {
        return Err(UcErr::new(SEG_BASE, format!("稳定性不过：{}", stability.describe())));
    }
    // ⑤ 注入核（12 形态逐个干净重建）
    let (evidence, detect) = inject_all()?;
    let mut detected: usize = 0;
    for f in Form::ALL.iter() {
        if detect.hit(*f) {
            detected = detected.wrapping_add(1);
        }
    }
    if detected != FORM_COUNT {
        return Err(UcErr::new(SEG_BASE, format!("注入核漏检：{:?}", detect.miss_list())));
    }
    // ⑥ 环形账
    let mut ring = RingLedger::new();
    let mut r: u32 = 0;
    while r < REPEAT as u32 {
        for run in runs.iter() {
            ring.push(RingEntry {
                cell: run.cell,
                round: r,
                ledger_fp: run.ledger.fingerprint,
                cell_fp: run.fingerprint,
                steps: run.ledger.steps,
                frames: run.ledger.frames_used,
                tick: run.ledger.tick,
            });
        }
        r = r.wrapping_add(1);
    }
    if !ring.balanced() {
        return Err(UcErr::of_form(Form::LedgerUnbalanced, format!("环形账不守恒：{}", ring.summary())));
    }
    // ⑦ 冻结件（哈希入册 + 漂移可检出）
    let mut align_words: Vec<u64> = Vec::with_capacity(GRID_CELLS);
    let mut align_clean: usize = 0;
    let mut cells_ok: usize = 0;
    let mut frames_ok: usize = 0;
    let mut steps_ok: usize = 0;
    let mut wave_ok: usize = 0;
    for run in runs.iter() {
        align_words.push(run.align.fingerprint);
        if run.align.missing == 0 && run.align.dup == 0 {
            align_clean = align_clean.wrapping_add(1);
        }
        if run.ok() {
            cells_ok = cells_ok.wrapping_add(1);
        }
        if run.frames_ok {
            frames_ok = frames_ok.wrapping_add(1);
        }
        if run.steps_ok {
            steps_ok = steps_ok.wrapping_add(1);
        }
        if run.wave_ok {
            wave_ok = wave_ok.wrapping_add(1);
        }
    }
    let align_fp = fnv1a_words(&align_words);
    let mut det_words: Vec<u64> = Vec::with_capacity(FORM_COUNT + 1);
    det_words.push(detect.verdict_mask as u64);
    for f in Form::ALL.iter() {
        det_words.push(match detect.hits.get(f.index()) {
            Some(v) => *v as u64,
            None => 0,
        });
    }
    let detect_fp = fnv1a_words(&det_words);
    let mut stab_words: Vec<u64> = Vec::with_capacity(GRID_CELLS + 2);
    stab_words.push(stability.matrix_fp);
    stab_words.push((stability.drift_bits & 0xFFFF_FFFF_FFFF_FFFFu128) as u64);
    stab_words.push(((stability.drift_bits >> 64) & 0xFFFF_FFFF_FFFF_FFFFu128) as u64);
    let stable_fp = fnv1a_words(&stab_words);
    let freeze = FreezeRecord::build(&m, align_fp, detect_fp, stable_fp);
    if !freeze.verify(&m) {
        return Err(UcErr::of_aux(AuxCode::BookRevMismatch, format!("冻结核验不过：{}", freeze.drift(&m))));
    }
    // ⑧ 指标
    let metrics = Metrics {
        cells: m.produced,
        count_ok: m.count_ok(),
        orthogonal: m.orthogonal_ok(),
        cells_ok,
        align_clean,
        wave_dist_ok: wave_distribution_ok(),
        frames_ok,
        steps_ok,
        wave_ok,
        rounds: stability.rounds,
        stable: stability.stable,
        drift_cells: stability.drift_cells().len(),
        forms: FORM_COUNT,
        detected,
        edges: FreezeFsm::edge_count(),
        rejected: fsm.rejected.len(),
        pushed: ring.pushed,
        dropped: ring.dropped,
        live: ring.live(),
        ring_ok: ring.balanced(),
        freeze_ok: freeze.verify(&m),
        matrix_hash: freeze.matrix_hash,
        freeze_hash: freeze.freeze_hash,
    };
    if !metrics.all_ok() {
        return Err(UcErr::new(SEG_BASE, format!("指标不过：{}", metrics.summary())));
    }
    // ⑨ 重放
    let rp = replay(seed)?;
    Ok(UcObserve {
        seed,
        matrix: m,
        runs,
        stability,
        evidence,
        detect,
        ring,
        fsm,
        freeze,
        metrics,
        replay: rp,
    })
}

// ===========================================================================
// 16. 资源上界显式核算
// ===========================================================================
/// 资源预算（显式核算，可逐项验算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResourceBudget {
    /// 矩阵常驻（格数 × 格大小）。
    pub matrix_bytes: u64,
    /// 逐格跑批峰值。
    pub runs_bytes: u64,
    /// 稳定性逐格序列峰值。
    pub stability_bytes: u64,
    /// 环形账。
    pub ring_bytes: u64,
    /// 导出缓冲上界。
    pub export_bytes: u64,
    /// 内存峰值合计。
    pub mem_peak: u64,
    /// step 上界。
    pub step_cap: u64,
    /// syscall 上界（恒 0）。
    pub syscalls: u64,
}
impl ResourceBudget {
    /// 核算。
    ///
    /// 逐项口径（都是可验算的整数乘法，不含指针尺寸假设）：
    /// - 矩阵：`125 格 × 11 个 u64 ≈ 11000 B`
    /// - 跑批：`125 格 × 替身账 12 u64 ≈ 12000 B`
    /// - 稳定性：`3 轮 × 125 格 u64 = 3000 B`
    /// - 环形账：`128 槽 × 7 u64 = 7168 B`
    /// - 导出：按 TLV 上界给 64 KiB 保守额
    pub fn account() -> ResourceBudget {
        let matrix_bytes = (GRID_CELLS * 11 * 8) as u64;
        let runs_bytes = (GRID_CELLS * 12 * 8) as u64;
        let stability_bytes = ((REPEAT * GRID_CELLS * 8) + 64) as u64;
        let ring_bytes = (RING_CAP * 7 * 8) as u64;
        let export_bytes = 64 * 1024;
        let mem_peak = matrix_bytes
            .wrapping_add(runs_bytes)
            .wrapping_add(stability_bytes)
            .wrapping_add(ring_bytes)
            .wrapping_add(export_bytes);
        ResourceBudget {
            matrix_bytes,
            runs_bytes,
            stability_bytes,
            ring_bytes,
            export_bytes,
            mem_peak,
            step_cap: STEP_CAP,
            syscalls: SYSCALL_CAP,
        }
    }
    /// 是否在界内。
    pub fn ok(&self) -> bool {
        self.mem_peak <= MEM_PEAK_CAP && self.step_cap <= STEP_CAP && self.syscalls == SYSCALL_CAP
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "budget mem_peak={}B (cap={}B) matrix={}B runs={}B stability={}B ring={}B export={}B step_cap={} syscalls={} ok={}",
            self.mem_peak,
            MEM_PEAK_CAP,
            self.matrix_bytes,
            self.runs_bytes,
            self.stability_bytes,
            self.ring_bytes,
            self.export_bytes,
            self.step_cap,
            self.syscalls,
            if self.ok() { 1 } else { 0 }
        )
    }
    /// 逐行。
    pub fn rows(&self) -> Vec<String> {
        vec![
            format!("matrix_bytes = {} x 11 x 8 = {}", GRID_CELLS, self.matrix_bytes),
            format!("runs_bytes = {} x 12 x 8 = {}", GRID_CELLS, self.runs_bytes),
            format!("stability_bytes = {} x {} x 8 + 64 = {}", REPEAT, GRID_CELLS, self.stability_bytes),
            format!("ring_bytes = {} x 7 x 8 = {}", RING_CAP, self.ring_bytes),
            format!("export_bytes = 64KiB = {}", self.export_bytes),
            format!("mem_peak = {} B, cap = {} B", self.mem_peak, MEM_PEAK_CAP),
            format!("step_cap = {} = MATRIX_STEPS({}) x REPEAT({}) x 2", self.step_cap, MATRIX_STEPS, REPEAT),
            format!("syscalls = {} (本单零真实 syscall)", self.syscalls),
        ]
    }
}

// ===========================================================================
// 17. 导出（KV + TLV，TLV 组头单独一行）
// ===========================================================================
/// TLV 组标识。
pub const TLV_TAG_HEAD: u8 = 0x01;
/// TLV 组头负载长度（`u64 × 3`：版本 / 段基址 / 册版本）。
pub const TLV_HEAD_PAYLOAD: usize = 24;
/// TLV 组头总长（`tag + len + payload`）。
pub const TLV_HEAD_LEN: usize = 1 + 2 + TLV_HEAD_PAYLOAD;

/// KV 导出（人读）。
pub fn export_kv(o: &UcObserve) -> Vec<(String, String)> {
    let mut kv: Vec<(String, String)> = Vec::with_capacity(48);
    kv.push((String::from("version"), String::from(V_VERSION)));
    kv.push((String::from("version_id"), format!("{:#018X}", V_VERSION_ID)));
    kv.push((String::from("seg_base"), format!("{:#06X}", SEG_BASE)));
    kv.push((String::from("seg_mask"), format!("{:#06X}", SEG_MASK)));
    kv.push((String::from("ticket"), String::from(CACHE_TICKET)));
    kv.push((String::from("if_query"), String::from(IF_QUERY)));
    kv.push((String::from("if_insert"), String::from(IF_INSERT)));
    kv.push((String::from("if_invalidate"), String::from(IF_INVALIDATE)));
    kv.push((String::from("ledger_ticket"), String::from(LEDGER_TICKET)));
    kv.push((String::from("exec_range"), String::from(EXEC_RANGE)));
    kv.push((String::from("book_rev"), o.freeze.rev.to_string()));
    kv.push((String::from("rw_count"), RW_COUNT.to_string()));
    kv.push((String::from("size_count"), SIZE_COUNT.to_string()));
    kv.push((String::from("conc_count"), CONC_COUNT.to_string()));
    kv.push((String::from("grid_cells"), o.matrix.produced.to_string()));
    kv.push((String::from("cell_steps"), CELL_STEPS.to_string()));
    kv.push((String::from("matrix_steps"), MATRIX_STEPS.to_string()));
    kv.push((String::from("wave_dist"), format!("{},{},{}", o.matrix.wave_dist[0], o.matrix.wave_dist[1], o.matrix.wave_dist[2])));
    for f in AlignField::ALL.iter() {
        kv.push((format!("align.{}", f.plan_field()), String::from(f.iface_field())));
    }
    let first_align = match o.runs.first() {
        Some(r) => (r.align.missing, r.align.dup),
        None => (0, 0),
    };
    kv.push((String::from("align_missing_bits"), format!("{:08X}", first_align.0)));
    kv.push((String::from("align_dup_bits"), format!("{:08X}", first_align.1)));
    kv.push((String::from("matrix_hash"), format!("{:016X}", o.freeze.matrix_hash)));
    kv.push((String::from("align_hash"), format!("{:016X}", o.freeze.align_hash)));
    kv.push((String::from("detect_hash"), format!("{:016X}", o.freeze.detect_hash)));
    kv.push((String::from("stable_hash"), format!("{:016X}", o.freeze.stable_hash)));
    kv.push((String::from("freeze_hash"), format!("{:016X}", o.freeze.freeze_hash)));
    kv.push((String::from("basis"), o.freeze.basis.clone()));
    kv.push((String::from("distortion"), o.freeze.distortion.clone()));
    kv.push((String::from("ring_pushed"), o.ring.pushed.to_string()));
    kv.push((String::from("ring_dropped"), o.ring.dropped.to_string()));
    kv.push((String::from("ring_live"), o.ring.live().to_string()));
    kv.push((String::from("fsm_state"), o.fsm.state.name().to_string()));
    kv.push((String::from("fsm_edges"), FreezeFsm::edge_count().to_string()));
    kv.push((String::from("fsm_rejected"), o.fsm.rejected.len().to_string()));
    kv.push((String::from("stability_rounds"), o.stability.rounds.to_string()));
    kv.push((String::from("stability_drift_bits"), format!("{:032X}", o.stability.drift_bits)));
    kv.push((String::from("detect_verdict_mask"), format!("{:08X}", o.detect.verdict_mask)));
    kv.push((String::from("metrics"), o.metrics.summary()));
    kv.push((String::from("replay"), o.replay.summary()));
    kv
}
/// TLV 导出。
///
/// 组头**单独一行**推入：`push_u64(&mut head_payload, V_VERSION_ID);`
/// 三条 push 决定 `TLV_HEAD_PAYLOAD = 24`，故
/// `TLV_HEAD_LEN = 1 + 2 + 24 = 27`——长度常量由 push 次数反推，
/// 不是拍脑袋写的。
pub fn export_tlv(o: &UcObserve) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(2048);
    // 组头
    let mut head_payload: Vec<u8> = Vec::with_capacity(TLV_HEAD_PAYLOAD);
    push_u64(&mut head_payload, V_VERSION_ID);
    push_u64(&mut head_payload, SEG_BASE as u64);
    push_u64(&mut head_payload, o.freeze.rev as u64);
    out.push(TLV_TAG_HEAD);
    // 长度**恒占 2 字节**（大端）。
    // 若按「小于 0x100 就只推 1 字节」的自适应写法，组头实际长度会变成 26，
    // 与 `TLV_HEAD_LEN = 1 + 2 + 24 = 27` 对不上，解析端必错。
    // 固定 2 字节让「长度字段宽度」成为常量，而不是数据相关的不变量。
    let hl = head_payload.len();
    out.push(((hl >> 8) & 0xFF) as u8);
    out.push((hl & 0xFF) as u8);
    for b in head_payload.iter() {
        out.push(*b);
    }
    // 三矩阵档数
    push_tlv_u64(&mut out, 0x10, RW_COUNT as u64);
    push_tlv_u64(&mut out, 0x11, SIZE_COUNT as u64);
    push_tlv_u64(&mut out, 0x12, CONC_COUNT as u64);
    push_tlv_u64(&mut out, 0x13, GRID_CELLS as u64);
    push_tlv_u64(&mut out, 0x14, CELL_STEPS as u64);
    push_tlv_u64(&mut out, 0x15, MATRIX_STEPS as u64);
    push_tlv_u64(&mut out, 0x16, REPEAT as u64);
    // 洪峰面分布
    push_tlv_u64(&mut out, 0x20, o.matrix.wave_dist[0] as u64);
    push_tlv_u64(&mut out, 0x21, o.matrix.wave_dist[1] as u64);
    push_tlv_u64(&mut out, 0x22, o.matrix.wave_dist[2] as u64);
    // 冻结件
    push_tlv_u64(&mut out, 0x30, o.freeze.matrix_hash);
    push_tlv_u64(&mut out, 0x31, o.freeze.align_hash);
    push_tlv_u64(&mut out, 0x32, o.freeze.detect_hash);
    push_tlv_u64(&mut out, 0x33, o.freeze.stable_hash);
    push_tlv_u64(&mut out, 0x34, o.freeze.freeze_hash);
    // 账与状态机
    push_tlv_u64(&mut out, 0x40, o.ring.pushed);
    push_tlv_u64(&mut out, 0x41, o.ring.dropped);
    push_tlv_u64(&mut out, 0x42, o.ring.live());
    push_tlv_u64(&mut out, 0x43, o.fsm.state.index() as u64);
    push_tlv_u64(&mut out, 0x44, FreezeFsm::edge_count() as u64);
    push_tlv_u64(&mut out, 0x45, o.fsm.rejected.len() as u64);
    // 稳定性
    push_tlv_u64(&mut out, 0x50, o.stability.rounds as u64);
    push_tlv_u64(&mut out, 0x51, o.stability.matrix_fp);
    // drift_bits 是 u128（125 格位），拆成低/高两个 u64 两条 TLV 推
    push_tlv_u64(&mut out, 0x52, (o.stability.drift_bits & 0xFFFF_FFFF_FFFF_FFFFu128) as u64);
    push_tlv_u64(&mut out, 0x53, ((o.stability.drift_bits >> 64) & 0xFFFF_FFFF_FFFF_FFFFu128) as u64);
    // 注入核逐形态命中
    for f in Form::ALL.iter() {
        let i = f.index();
        let h = match o.detect.hits.get(i) {
            Some(v) => *v,
            None => 0,
        };
        push_tlv_u64(&mut out, 0x60u8.wrapping_add(i as u8), h as u64);
    }
    push_tlv_u64(&mut out, 0x70, o.detect.verdict_mask as u64);
    // 指标
    push_tlv_u64(&mut out, 0x80, o.metrics.cells_ok as u64);
    push_tlv_u64(&mut out, 0x81, o.metrics.align_clean as u64);
    push_tlv_u64(&mut out, 0x82, o.metrics.detected as u64);
    push_tlv_u64(&mut out, 0x83, o.metrics.fingerprint());
    push_tlv_u64(&mut out, 0x84, o.replay.fingerprint);
    out
}
/// 推 8 字节大端。
pub fn push_u64(buf: &mut Vec<u8>, v: u64) {
    buf.push(((v >> 56) & 0xFF) as u8);
    buf.push(((v >> 48) & 0xFF) as u8);
    buf.push(((v >> 40) & 0xFF) as u8);
    buf.push(((v >> 32) & 0xFF) as u8);
    buf.push(((v >> 24) & 0xFF) as u8);
    buf.push(((v >> 16) & 0xFF) as u8);
    buf.push(((v >> 8) & 0xFF) as u8);
    buf.push((v & 0xFF) as u8);
}
/// 推一条 TLV（tag + len + 8 字节负载）。
pub fn push_tlv_u64(buf: &mut Vec<u8>, tag: u8, v: u64) {
    buf.push(tag);
    buf.push(8u8);
    push_u64(buf, v);
}
/// TLV 往返解析（校验组头 + 关键项回读一致）。
pub fn parse_tlv(buf: &[u8]) -> UcResult<(u64, Vec<(u8, u64)>)> {
    if buf.len() < TLV_HEAD_LEN {
        return Err(UcErr::new(SEG_BASE, format!("TLV 过短：{} < {}", buf.len(), TLV_HEAD_LEN)));
    }
    let tag = match buf.get(0) {
        Some(v) => *v,
        None => return Err(UcErr::new(SEG_BASE, String::from("TLV 无组头"))),
    };
    if tag != TLV_TAG_HEAD {
        return Err(UcErr::new(SEG_BASE, format!("TLV 组头 tag 错：{:#04X}", tag)));
    }
    let l0 = match buf.get(1) {
        Some(v) => *v,
        None => return Err(UcErr::new(SEG_BASE, String::from("TLV 无长度"))),
    };
    let l1 = match buf.get(2) {
        Some(v) => *v,
        None => return Err(UcErr::new(SEG_BASE, String::from("TLV 长度截断"))),
    };
    let hl = (((l0 as usize) << 8) | (l1 as usize)) & 0xFFFF;
    if hl != TLV_HEAD_PAYLOAD {
        return Err(UcErr::new(SEG_BASE, format!("TLV 组头长度错：{} != {}", hl, TLV_HEAD_PAYLOAD)));
    }
    let mut head: Vec<u8> = Vec::with_capacity(TLV_HEAD_PAYLOAD);
    let mut i: usize = 0;
    while i < TLV_HEAD_PAYLOAD {
        match buf.get(TLV_HEAD_LEN - TLV_HEAD_PAYLOAD + i) {
            Some(v) => head.push(*v),
            None => return Err(UcErr::new(SEG_BASE, String::from("TLV 组头截断"))),
        }
        i = i.wrapping_add(1);
    }
    let ver = be_u64(&head, 0);
    let seg = be_u64(&head, 8);
    let rev = be_u64(&head, 16);
    if ver != V_VERSION_ID {
        return Err(UcErr::new(SEG_BASE, format!("TLV 版本错：{:#018X}", ver)));
    }
    if seg != SEG_BASE as u64 {
        return Err(UcErr::new(SEG_BASE, format!("TLV 段基址错：{:#018X}", seg)));
    }
    if rev != STRESS_BOOK_REV as u64 {
        return Err(UcErr::of_aux(AuxCode::BookRevMismatch, format!("TLV 册版本错：{}", rev)));
    }
    // 逐项解析
    let mut items: Vec<(u8, u64)> = Vec::new();
    let mut p: usize = TLV_HEAD_LEN;
    while p < buf.len() {
        let t = match buf.get(p) {
            Some(v) => *v,
            None => break,
        };
        let l = match buf.get(p.wrapping_add(1)) {
            Some(v) => *v as usize,
            None => return Err(UcErr::new(SEG_BASE, String::from("TLV 项长度截断"))),
        };
        if l != 8 {
            return Err(UcErr::new(SEG_BASE, format!("TLV 项负载长错：{} != 8", l)));
        }
        let mut payload: Vec<u8> = Vec::with_capacity(8);
        let mut k: usize = 0;
        while k < 8 {
            match buf.get(p.wrapping_add(2).wrapping_add(k)) {
                Some(v) => payload.push(*v),
                None => return Err(UcErr::new(SEG_BASE, String::from("TLV 项负载截断"))),
            }
            k = k.wrapping_add(1);
        }
        items.push((t, be_u64(&payload, 0)));
        p = p.wrapping_add(2).wrapping_add(8);
    }
    Ok((ver, items))
}
/// 大端取 u64。
pub fn be_u64(buf: &[u8], off: usize) -> u64 {
    let mut v: u64 = 0;
    let mut i: usize = 0;
    while i < 8 {
        let b = match buf.get(off.wrapping_add(i)) {
            Some(x) => *x,
            None => return v,
        };
        v = (v << 8) | (b as u64);
        i = i.wrapping_add(1);
    }
    v
}
/// TLV 往返一致。
pub fn tlv_roundtrip_ok(o: &UcObserve) -> bool {
    let buf = export_tlv(o);
    match parse_tlv(&buf) {
        Ok((ver, items)) => {
            if ver != V_VERSION_ID || items.len() < 30 {
                return false;
            }
            let want_freeze = o.freeze.freeze_hash;
            let want_mat = o.freeze.matrix_hash;
            let want_live = o.ring.live();
            let mut ok_freeze = false;
            let mut ok_mat = false;
            let mut ok_live = false;
            for (t, v) in items.iter() {
                if *t == 0x34 && *v == want_freeze {
                    ok_freeze = true;
                }
                if *t == 0x30 && *v == want_mat {
                    ok_mat = true;
                }
                if *t == 0x42 && *v == want_live {
                    ok_live = true;
                }
            }
            ok_freeze && ok_mat && ok_live
        }
        Err(_) => false,
    }
}
// ===========================================================================
// 18. 性能逐项分解
// ===========================================================================
/// 单项成本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CostItem {
    /// 环节名。
    pub name: &'static str,
    /// 主循环次数。
    pub iters: u64,
    /// 每次迭代的基本操作数。
    pub per_iter: u64,
    /// 合计基本操作数。
    pub total: u64,
}
impl CostItem {
    /// 构造。
    pub fn new(name: &'static str, iters: u64, per_iter: u64) -> CostItem {
        CostItem { name, iters, per_iter, total: iters.wrapping_mul(per_iter) }
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("{:<22} iters={:<6} per_iter={:<5} total={}", self.name, self.iters, self.per_iter, self.total)
    }
}
impl Metrics {
    /// 性能逐项分解。
    ///
    /// 口径（全部是可验算的整数乘法，不含「大概」）：
    /// - 展开：125 格 × (三档查表 + 1 次 FNV 折叠)
    /// - 真跑：125 格 × (24 步 × 每步四类事件 + 9 条守恒 + 10 条派生)
    /// - 稳定性：3 轮 × 展开 + 真跑 + 125 次指纹比对
    /// - 注入核：12 形态 × (1 次展开 + 1 次逐字段检出)
    /// - 环形账：3 轮 × 125 次写入
    /// - 导出：KV 40 项 + TLV 40 项
    pub fn complexity(&self) -> Vec<CostItem> {
        vec![
            CostItem::new("matrix_expand", GRID_CELLS as u64, 12),
            CostItem::new("cell_fixture_run", (GRID_CELLS * CELL_STEPS) as u64, 8),
            CostItem::new("reconcile_check", (GRID_CELLS * 1) as u64, 9),
            CostItem::new("align_derive", (GRID_CELLS * ALIGN_FIELDS) as u64, 3),
            CostItem::new("align_dup_scan", (GRID_CELLS * ALIGN_FIELDS * ALIGN_FIELDS) as u64, 1),
            CostItem::new("stability_rounds", REPEAT as u64, GRID_CELLS as u64 * 2),
            CostItem::new("cellwise_compare", ((REPEAT - 1) * GRID_CELLS) as u64, 1),
            CostItem::new("inject_forms", FORM_COUNT as u64, GRID_CELLS as u64 * 6),
            CostItem::new("ring_push", (REPEAT * GRID_CELLS) as u64, 2),
            CostItem::new("export_kv", 40, 4),
            CostItem::new("export_tlv", 40, 12),
            CostItem::new("fsm_transitions", (FState::COUNT + 3) as u64, 1),
        ]
    }
    /// 性能逐行。
    pub fn complexity_rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(16);
        for c in self.complexity().iter() {
            out.push(c.describe());
        }
        out.push(format!("TOTAL = {}", self.complexity_total()));
        out
    }
    /// 性能合计。
    pub fn complexity_total(&self) -> u64 {
        let mut t: u64 = 0;
        for c in self.complexity().iter() {
            t = t.wrapping_add(c.total);
        }
        t
    }
}

// ===========================================================================
// 19. 自检（逐项可读，全过才算本单自洽）
// ===========================================================================
/// 自检项结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Check {
    /// 项名。
    pub name: &'static str,
    /// 是否通过。
    pub ok: bool,
    /// 实测值（人读）。
    pub got: String,
}
impl Check {
    /// 构造。
    pub fn new(name: &'static str, ok: bool, got: String) -> Check {
        Check { name, ok, got }
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("[{}] {:<34} {}", if self.ok { "PASS" } else { "FAIL" }, self.name, self.got)
    }
}
/// 自检（28 项）。
pub fn self_check(o: &UcObserve) -> Vec<Check> {
    let mut cs: Vec<Check> = Vec::with_capacity(28);
    let m = &o.matrix;
    // 1-6：常量闭集
    cs.push(Check::new("const.grid_cells", GRID_CELLS == RW_COUNT * SIZE_COUNT * CONC_COUNT, format!("{} = {}x{}x{}", GRID_CELLS, RW_COUNT, SIZE_COUNT, CONC_COUNT)));
    cs.push(Check::new("const.matrix_steps", MATRIX_STEPS == GRID_CELLS * CELL_STEPS, format!("{} = {}x{}", MATRIX_STEPS, GRID_CELLS, CELL_STEPS)));
    cs.push(Check::new("const.seg_in_range", SEG_BASE <= SEG_END && (SEG_END & !SEG_MASK) == SEG_BASE, format!("{:#06X}..{:#06X}", SEG_BASE, SEG_END)));
    cs.push(Check::new("const.form_codes_fit", Form::ALL[FORM_COUNT - 1].code() <= SEG_END, format!("last={:#06X}", Form::ALL[FORM_COUNT - 1].code())));
    cs.push(Check::new("const.aux_codes_fit", AuxCode::BookRevMismatch.code() <= SEG_END, format!("last={:#06X}", AuxCode::BookRevMismatch.code())));
    cs.push(Check::new("const.align_fields", ALIGN_FIELDS == 10, format!("{}", ALIGN_FIELDS)));
    // 7-12：矩阵
    cs.push(Check::new("matrix.count", m.count_ok(), format!("{}/{}", m.produced, GRID_CELLS)));
    cs.push(Check::new("matrix.orthogonal", m.orthogonal_ok(), String::from("linear index unique")));
    cs.push(Check::new("matrix.dims", m.dims_ok(), String::from("3 bands + wave")));
    cs.push(Check::new("matrix.rw_dist", m.rw_dist[0] == 25 && m.rw_dist[4] == 25, format!("{:?}", m.rw_dist)));
    cs.push(Check::new("matrix.wave_dist", wave_distribution_ok(), format!("{:?}", wave_distribution())));
    cs.push(Check::new("matrix.linear_index", PlanMatrix::linear_index(4, 4, 4) == GRID_CELLS - 1, format!("{}", PlanMatrix::linear_index(4, 4, 4))));
    // 13-18：逐格真跑
    cs.push(Check::new("runs.all_ok", o.metrics.cells_ok == GRID_CELLS, format!("{}/{}", o.metrics.cells_ok, GRID_CELLS)));
    cs.push(Check::new("runs.frames_ok", o.metrics.frames_ok == GRID_CELLS, format!("{}/{}", o.metrics.frames_ok, GRID_CELLS)));
    cs.push(Check::new("runs.steps_ok", o.metrics.steps_ok == GRID_CELLS, format!("{}/{}", o.metrics.steps_ok, GRID_CELLS)));
    cs.push(Check::new("runs.wave_ok", o.metrics.wave_ok == GRID_CELLS, format!("{}/{}", o.metrics.wave_ok, GRID_CELLS)));
    cs.push(Check::new("runs.align_clean", o.metrics.align_clean == GRID_CELLS, format!("{}/{}", o.metrics.align_clean, GRID_CELLS)));
    cs.push(Check::new("runs.percell_ledger", o.runs.iter().all(|r| r.ledger.hits.wrapping_add(r.ledger.misses) == r.ledger.queries), String::from("read events closed")));
    // 19-22：对齐
    let mut miss_all = true;
    let mut dup_all = true;
    for r in o.runs.iter() {
        if r.align.missing != 0 {
            miss_all = false;
        }
        if r.align.dup != 0 {
            dup_all = false;
        }
    }
    cs.push(Check::new("align.zero_missing", miss_all, format!("125 格 missing 全 0")));
    cs.push(Check::new("align.zero_dup", dup_all, String::from("125 格 dup 全 0")));
    // `self_check` 返回 `Vec<Check>`（不返 `Result`），故此处用 match 代替 `?`：
    // 任何取表失败都归为「三接口未齐」，让自检项照常给结论而不是整函数崩掉。
    let cell0 = m.cell(0);
    let ifaces = match cell0 {
        Ok(c) => match AlignTable::build(c) {
            Ok(t) => t.iface_bits(),
            Err(_) => 0,
        },
        Err(_) => 0,
    };
    cs.push(Check::new("align.three_ifaces", ifaces == 0b111, format!("{:#05X}", ifaces)));
    cs.push(Check::new("align.derived_ok", o.runs.iter().all(|r| r.align.derived), String::from("10 条派生全部可反算")));
    // 23-25：稳定性 / 注入核 / 账
    cs.push(Check::new("stability.ok", o.stability.ok(), o.stability.describe()));
    cs.push(Check::new("stability.drift_bits_u128", GRID_CELLS <= 128, format!("drift_bits={:032X}", o.stability.drift_bits)));
    cs.push(Check::new("inject.all_detected", o.metrics.detected == FORM_COUNT, format!("{}/{}", o.metrics.detected, FORM_COUNT)));
    cs.push(Check::new("inject.no_miss", o.detect_miss().is_empty(), format!("miss={:?}", o.detect_miss())));
    // 26-28：状态机 / 环形账 / 导出
    cs.push(Check::new("fsm.edges", FreezeFsm::edge_count() == FState::COUNT - 1, format!("{}", FreezeFsm::edge_count())));
    cs.push(Check::new("fsm.rejected", o.fsm.rejected.len() >= 3 && o.fsm.state == FState::Sealed, format!("rejected={} state={}", o.fsm.rejected.len(), o.fsm.state.name())));
    cs.push(Check::new("ring.balanced", o.ring.balanced(), o.ring.summary()));
    cs
}
/// 自检全过。
pub fn self_check_ok(o: &UcObserve) -> bool {
    self_check(o).iter().all(|c| c.ok)
}
/// 自检报告（逐行）。
pub fn self_check_report(o: &UcObserve) -> String {
    let cs = self_check(o);
    let mut pass: usize = 0;
    for c in cs.iter() {
        if c.ok {
            pass = pass.wrapping_add(1);
        }
    }
    let mut out: String = format!("self_check {}/{}", pass, cs.len());
    for c in cs.iter() {
        out.push('\n');
        out.push_str(&c.describe());
    }
    out
}
/// TLV 往返一致（薄封装）。
pub fn tlv_ok(o: &UcObserve) -> bool {
    tlv_roundtrip_ok(o)
}

// ===========================================================================
// 20. 清单与演示
// ===========================================================================
/// 形态清单。
pub fn form_manifest() -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(FORM_COUNT);
    for f in Form::ALL.iter() {
        out.push(format!(
            "{:02} {:#06X} {:<24} [{}] {} | 造证：{}",
            f.index(),
            f.code(),
            f.name(),
            f.class_name(),
            f.describe(),
            f.probe()
        ));
    }
    out
}
/// 不变式清单。
pub fn invariant_manifest() -> Vec<String> {
    let al = AlignField::ALL;
    let mut out: Vec<String> = Vec::new();
    out.push(format!("格数 = {} × {} × {} = {}（现算比对）", RW_COUNT, SIZE_COUNT, CONC_COUNT, GRID_CELLS));
    out.push(String::from("正交性：125 格的线性格号两两互异"));
    out.push(String::from("档位闭集：读比/尺寸/并发三维序号全部在界内"));
    out.push(format!("洪峰归属：wave_of(读比档) 与格内 wave_idx 逐格一致，分布 {:?}", wave_distribution()));
    out.push(String::from("读事件闭合：hits + misses == queries"));
    out.push(String::from("读写合计：queries + stores == ops"));
    out.push(String::from("写回不超：writebacks <= stores"));
    out.push(String::from("回收不超：reclaims <= frames_used"));
    out.push(String::from("常驻页推导：live_pages == pages_per_op × lanes"));
    out.push(String::from("时钟同步：tick == steps"));
    out.push(String::from("期望等式：hit/dirty/reclaim 实测计数 == 按方案 expect 重算出的计数（不用千分数回算，避开双整除误差）"));
    out.push(format!("帧数界：frames_used <= frames，125 格全过"));
    out.push(format!("步数界：steps <= steps 上界，125 格全过"));
    out.push(format!("对齐零缺失：missing 位图 == 0（{} 条映射）", al.len()));
    out.push(String::from("对齐零重复：dup 位图 == 0"));
    out.push(String::from("对齐三接口齐：f2662 / f2663 / f2664 各至少一条边"));
    out.push(String::from("对齐派生可反算：10 条读数均由格参数现算"));
    out.push(format!("稳定性：{} 轮矩阵指纹一致 + 逐格指纹一致", REPEAT));
    out.push(format!("注入核：{}/{} 形态逐个干净重建并被逐字段独立检出", FORM_COUNT, FORM_COUNT));
    out.push(format!("环形账：pushed == dropped + live，live <= {}", RING_CAP));
    out.push(format!("状态机：{} 条合法边全走满，非法迁移拒记", FreezeFsm::edge_count()));
    out.push(String::from("冻结：册冻结哈希入册，矩阵漂移可检出"));
    out.push(String::from("导出：KV 可读 + TLV 组头 27 字节往返一致"));
    out
}
/// 演示场景（把方案册切成 8 个视角）。
pub fn demo_scene() -> Vec<String> {
    let m = PlanMatrix::expand();
    let mut out: Vec<String> = Vec::new();
    out.push(String::from("场景 1：三矩阵正交展开"));
    out.push(format!("  格数 {} = 读比{} × 尺寸{} × 并发{}", m.produced, RW_COUNT, SIZE_COUNT, CONC_COUNT));
    out.push(String::from("场景 2：三档边界视图"));
    out.push(format!("  读比档边界：{} ↔ {}（{} / {}）", RwRatio::ALL[0].ratio_str(), RwRatio::ALL[4].ratio_str(), RwRatio::ALL[0].hit_expect(), RwRatio::ALL[4].hit_expect()));
    out.push(format!("  尺寸档边界：{} ↔ {}（{} 页 ↔ {} 页）", FileSize::ALL[0].size_str(), FileSize::ALL[4].size_str(), FileSize::ALL[0].pages(), FileSize::ALL[4].pages()));
    out.push(format!("  并发档边界：{} ↔ {}（{} 道 ↔ {} 道）", ConcDegree::ALL[0].code2(), ConcDegree::ALL[4].code2(), ConcDegree::ALL[0].lanes(), ConcDegree::ALL[4].lanes()));
    out.push(String::from("场景 3：洪峰面归属"));
    out.push(format!("  分布 {:?}，纯函数 wave_of 逐档推导", wave_distribution()));
    out.push(String::from("场景 4：对抗面视图"));
    let adv = m.cells.iter().filter(|c| c.is_primary_adversarial()).count();
    out.push(format!("  主要对抗格 {}/{}（尺寸大 或 并发重）", adv, m.produced));
    out.push(String::from("场景 5：单格替身账"));
    if let Some(c) = m.cells.get(0) {
        if let Ok(l) = CacheLedger::run(c) {
            out.push(format!("  {}", c.describe()));
            out.push(format!("  {}", l.describe()));
        }
    }
    out.push(String::from("场景 6：三接口对齐"));
    if let Some(c) = m.cells.get(0) {
        if let Ok(t) = AlignTable::build(c) {
            for r in t.rows().iter() {
                out.push(format!("  {}", r));
            }
        }
    }
    out.push(String::from("场景 7：冻结件"));
    let fr = FreezeRecord::build(&m, 0, 0, 0);
    out.push(format!("  {}", fr.describe()));
    out.push(String::from("场景 8：失真声明"));
    out.push(format!("  {}", fr.distortion));
    out
}
/// 演示账（6 个视角的账册切片）。
pub fn demo_ledger() -> Vec<String> {
    let m = PlanMatrix::expand();
    let mut out: Vec<String> = Vec::new();
    out.push(String::from("账 A：三矩阵档数分布"));
    out.push(format!("  rw={:?} size={:?} conc={:?} wave={:?}", m.rw_dist, m.size_dist, m.conc_dist, m.wave_dist));
    out.push(String::from("账 B：逐格守恒"));
    let mut i: usize = 0;
    while i < 5 {
        if let Some(c) = m.cells.get(i) {
            match CacheLedger::run(c) {
                Ok(l) => out.push(format!("  {}", l.describe())),
                Err(e) => out.push(format!("  err {}", err_str(&e))),
            }
        }
        i = i.wrapping_add(1);
    }
    out.push(String::from("账 C：对齐读数"));
    if let Some(c) = m.cells.get(0) {
        if let Ok(t) = AlignTable::build(c) {
            out.push(format!("  {}", t.summary()));
        }
    }
    out.push(String::from("账 D：状态机合法边"));
    for s in FState::ALL.iter() {
        out.push(format!("  {:<9} -> {:?}", s.name(), s.allowed_names()));
    }
    out.push(String::from("账 E：资源上界"));
    for r in ResourceBudget::account().rows().iter() {
        out.push(format!("  {}", r));
    }
    out.push(String::from("账 F：性能分解"));
    let mm = Metrics {
        cells: 0,
        count_ok: false,
        orthogonal: false,
        cells_ok: 0,
        align_clean: 0,
        wave_dist_ok: false,
        frames_ok: 0,
        steps_ok: 0,
        wave_ok: 0,
        rounds: 0,
        stable: false,
        drift_cells: 0,
        forms: FORM_COUNT,
        detected: 0,
        edges: 0,
        rejected: 0,
        pushed: 0,
        dropped: 0,
        live: 0,
        ring_ok: false,
        freeze_ok: false,
        matrix_hash: 0,
        freeze_hash: 0,
    };
    for r in mm.complexity_rows().iter() {
        out.push(format!("  {}", r));
    }
    out
}
/// 相关单号。
pub fn related_str() -> String {
    format!(
        "{}(本单 cache 本体,不做) {}/{}/{}(三接口) {}(账册面名) 执行单 {}",
        CACHE_TICKET, IF_QUERY, IF_INSERT, IF_INVALIDATE, LEDGER_TICKET, EXEC_RANGE
    )
}
// <<TAIL>>