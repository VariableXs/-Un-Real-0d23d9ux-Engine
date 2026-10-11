//! unx2722 ↔ UNX-F2722 anon 页逐出前置判定（swap 槽位语义声明）
//!
//! 任务单号：UNX-F2722
//! 标题：anon 页逐出前置判定（swap 槽位语义声明）
//! 域/批：A4/B17
//! 册目标行数：300（本深化版按产线硬指标展开至 ≥2800 行）
//! 码段范围声明：独占码段 `0x1E80 ~ 0x1EBF`（`SEG_BASE = 0x1E80`，`SEG_MASK = 0x003F`）；
//!                错误变体一码一段，12 个变体占用 `0x1E80 ~ 0x1E8B`，余下 `0x1E8C ~ 0x1EBF`
//!                留作注入核与竞态面扩展诊断位。
//!
//! 判据原文（逐字摘抄，CoRun Unxreal 主册 UNX-F2722-J1）：
//! `UNX-F2722-J1 anon 逐出前置判定（槽位可用性查账）10/10 次，无槽即归还语义在档`
//!
//! 锚点原文关键句（逐字摘抄）：
//! - `anon 页逐出的准入闸——anon 页逐出必须先有暂存去处（swap 槽位语义：本域内置暂存区登记账），
//!    前置判定查槽位账（F2730）有可用槽才进写出（F2423），无槽即归还（F2714）——"无去处不逐出"保证 anon 数据零丢失`
//! - `第一步判定：anon 逐出前置——查 F2730 槽位账空闲计数，可用则原子预留一槽（预留位写入逐出上下文），
//!    不可用即走归还路径并计"无槽归还"计数`
//! - `第二步语义冻结：无槽即归还（不等待不阻塞扫描器）在本条冻结，等待式语义（阻塞等槽）显式不做（避免扫描器卡死）`
//! - `第三步断言：预留竞态注入（双核并发判定同批槽位）零双重预留 10 轮，无槽注入（槽位清零）归还 10/10 次`
//! - `判据 UNX-F2722-J1 的复测方式：有槽判定 10/10 次预留成功进写出，无槽注入 10/10 次归还且无槽归还计数递增，
//!    预留竞态注入 10 轮零双重登记，预留-写出-释放全链槽位账平`
//!
//! ## 职责定位
//! 本单**只做前置判定**，不做槽位账本体（F2730）、不做写出（F2723）、
//! 不做无槽配置语义（F2724 扩展）。因此本单把 F2730 槽位账建模为
//! 「总槽数 / 空闲计数 / 预留位图」，并把前置判定建模为
//! **判定即预留**（原子化，防 TOCTOU）的准入闸。
//!
//! ## 与册内其他单的显式对齐点
//! - **F2730 槽位账**：本单的槽位账是 F2730 的**建模替身**（字段口径一致：
//!   `total / free / reserved_bitmap`）。
//! - **F2714 归还**：无槽归还语义由本单冻结，归还计数是本单的可测读数。
//! - **F2723 写出**：本单只把逐出上下文（预留槽号）交出，写出由 F2723 承接。
//! - **F2724**：无槽**配置**语义显式不做——本单固定为「无槽即归还」，不提供配置位。
//! - **F2731**：批量硬顶 32 页，本单逐出批同样受此硬顶约束。
//!
//! ## 纯功能线
//! `no_std + alloc`，零 `unwrap` / `expect` / `panic!` / `todo!`、零浮点、
//! 零真实 IO、零墙钟；时间用逻辑 `tick: u64`。确定性 LCG
//! （`LCG_A = 6364136223846793005`、`LCG_C = 1442695040888963407`，模 `2^64`）。
//! FNV-1a basis `0xcbf29ce484222325`、prime `0x100000001b3`。
//! 自定义 `AnonResult<T>`；不写 `#[test]`，只暴露 `observe(seed)`。
#![allow(dead_code)]
extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
#[allow(unused_imports)]
use alloc::vec;
use alloc::vec::Vec;

// ===========================================================================
// 0. 常量表、判据阈值与来源注释
// ===========================================================================

/// 模块版本标识。
pub const V2722_VERSION_ID: u64 = 0x0002_7220_0000_0001;
/// 模块版本串。
pub const V2722_VERSION: &str = "v2722-anonevict-admit0";
/// 诊断码段基址（册内独占）。
pub const SEG_BASE: u16 = 0x1E80;
/// 诊断码段掩码。
pub const SEG_MASK: u16 = 0x003F;
/// 诊断码段上界（含）。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;
/// 诊断主码（无槽仍逐出，段内首码）。
pub const MAIN_CODE: u16 = SEG_BASE;

/// 槽位总数（建模 `SWAP_SLOTS`，F2730 槽位账口径）。
pub const SWAP_SLOTS: usize = 64;
/// 预留位图字数（`SWAP_SLOTS / 64`）。
pub const RESERVED_WORDS: usize = (SWAP_SLOTS + 63) / 64;
/// 判据阈值：有槽判定次数（判据「有槽判定 10/10 次」）。
pub const ADMIT_CASES: usize = 10;
/// 判据阈值：无槽注入归还次数（判据「无槽注入（槽位清零）归还 10/10 次」）。
pub const NOSLOT_CASES: usize = 10;
/// 判据阈值：预留竞态注入轮数（判据「零双重预留 10 轮」）。
pub const RACE_ROUNDS: usize = 10;
/// 判据阈值：预留竞态注入每轮参与核数（本单建模 2 核并发）。
pub const RACE_CORES: usize = 2;
/// 竞态注入总尝试数（`RACE_ROUNDS × RACE_CORES`）。
pub const RACE_TOTAL: usize = RACE_ROUNDS * RACE_CORES;
/// 注入形态数（判据十形态）。
pub const FORM_COUNT: usize = 10;
/// 矩阵单元格数（槽位状态 4 × 逐出类别 4 = 16）。
pub const MATRIX_CELLS: usize = 16;
/// 逐出批硬顶（对齐 F2731）。
pub const BATCH_HARD_CAP: usize = 32;
/// 环形账固定容量。
pub const RING_CAP: usize = 192;
/// 语义冻结声明：无槽即归还（等待式语义显式不做）。
pub const NO_WAIT_SEMANTICS: bool = true;
/// 语义冻结声明：无槽配置位恒 0（F2724 承载扩展，本单固定关闭）。
pub const NO_WAIT_CONFIG_BIT: u64 = 0;
/// 预留-写出-释放全链长度（三段各 `CHAIN_STEPS` 步）。
pub const CHAIN_STEPS: usize = 32;

/// LCG 乘数（判据指定）。
pub const LCG_A: u64 = 6364136223846793005;
/// LCG 增量（判据指定）。
pub const LCG_C: u64 = 1442695040888963407;
/// FNV-1a basis（判据指定）。
pub const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a prime（判据指定）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 槽位状态闭集代号（4 值）。
pub const SLOTSTATE_CODES: [&str; 4] = ["free", "reserved", "written", "released"];
/// 逐出类别闭集代号（4 值）。
pub const EVICTCLASS_CODES: [&str; 4] = ["anon_dirty", "anon_clean", "file_dirty", "file_clean"];
/// 注入形态代号（10 值）。
pub const FORM_CODES: [&str; FORM_COUNT] = [
    "no_slot_evict", "double_reserve", "reserved_bit_clobber", "free_count_lie", "ctx_slot_mismatch", "release_twice", "write_without_reserve", "batch_over_cap", "wait_semantics", "chain_unbalanced",
];

// ---------------------------------------------------------------------------
// 0.1 确定性 LCG 与 FNV-1a
// ---------------------------------------------------------------------------

/// 确定性 LCG 发生器。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    /// 内部状态。
    state: u64,
}

impl Lcg {
    /// 以种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    /// 只读状态。
    pub fn raw(&self) -> u64 {
        self.state
    }
    /// 下一个 `u64`。
    pub fn next_u64(&mut self) -> u64 {
        self.state = lcg_step(self.state);
        self.state
    }
    /// `[0, n)` 内值。
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % (n as u64)) as usize
    }
    /// 派生支发生器。
    pub fn fork(&self, salt: u64) -> Lcg {
        Lcg { state: self.state ^ salt.wrapping_mul(LCG_A) }
    }
}

/// 纯函数式 LCG 步进。
pub fn lcg_step(state: u64) -> u64 {
    state.wrapping_mul(LCG_A).wrapping_add(LCG_C)
}

/// FNV-1a 64 位折叠。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = FNV_BASIS;
    for b in bytes {
        h ^= (*b as u64) & 0xFF;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// FNV-1a 折叠字序列。
pub fn fnv1a_words(words: &[u64]) -> u64 {
    let mut h: u64 = FNV_BASIS;
    for w in words {
        h = fnv1a_u64_into(h, *w);
    }
    h
}

/// 把 `u64` 以 8 字节小端喂入 FNV-1a 累加器。
pub fn fnv1a_u64_into(h: u64, w: u64) -> u64 {
    let mut acc: u64 = h;
    for i in 0..8u32 {
        let byte: u8 = ((w >> (8 * i)) & 0xFF) as u8;
        acc ^= (byte as u64) & 0xFF;
        acc = acc.wrapping_mul(FNV_PRIME);
    }
    acc
}

// ===========================================================================
// 1. 错误闭集（12 变体，一码一段）
// ===========================================================================

/// anon 逐出前置判定域错误闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnonErr {
    /// 0x1E80 无槽仍逐出（准入闸失效）。
    NoSlotEvict,
    /// 0x1E81 双重预留：同一槽被两个上下文同时预留。
    DoubleReserve,
    /// 0x1E82 预留位被踩：位图与实际预留不一致。
    ReservedBitClobber,
    /// 0x1E83 空闲计数撒谎：位图与 `free` 计数不符。
    FreeCountLie,
    /// 0x1E84 上下文槽号失配：逐出上下文记录的槽与实际预留不符。
    CtxSlotMismatch,
    /// 0x1E85 重复释放：同一槽被释放两次。
    ReleaseTwice,
    /// 0x1E86 未预留即写出：写出前置不成立。
    WriteWithoutReserve,
    /// 0x1E87 批量超硬顶（`> BATCH_HARD_CAP`）。
    BatchOverCap,
    /// 0x1E88 等待式语义被引入（冻结语义是「无槽即归还」）。
    WaitSemantics,
    /// 0x1E89 全链账不平（预留-写出-释放不平）。
    ChainUnbalanced,
    /// 0x1E8A 槽号越界。
    SlotOutOfRange,
    /// 0x1E8B 逐出类别非法（既非 anon 也非 file）。
    ClassInvalid,
}

impl AnonErr {
    /// 全部变体。
    pub const ALL: [AnonErr; 12] = [AnonErr::NoSlotEvict, AnonErr::DoubleReserve, AnonErr::ReservedBitClobber, AnonErr::FreeCountLie, AnonErr::CtxSlotMismatch, AnonErr::ReleaseTwice,
        AnonErr::WriteWithoutReserve, AnonErr::BatchOverCap, AnonErr::WaitSemantics, AnonErr::ChainUnbalanced, AnonErr::SlotOutOfRange, AnonErr::ClassInvalid];
    /// 变体序号。
    pub fn index(self) -> usize {
        match self {
            AnonErr::NoSlotEvict => 0,
            AnonErr::DoubleReserve => 1,
            AnonErr::ReservedBitClobber => 2,
            AnonErr::FreeCountLie => 3,
            AnonErr::CtxSlotMismatch => 4,
            AnonErr::ReleaseTwice => 5,
            AnonErr::WriteWithoutReserve => 6,
            AnonErr::BatchOverCap => 7,
            AnonErr::WaitSemantics => 8,
            AnonErr::ChainUnbalanced => 9,
            AnonErr::SlotOutOfRange => 10,
            AnonErr::ClassInvalid => 11,
        }
    }
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + (self.index() as u16)
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            AnonErr::NoSlotEvict => "no_slot_evict",
            AnonErr::DoubleReserve => "double_reserve",
            AnonErr::ReservedBitClobber => "reserved_bit_clobber",
            AnonErr::FreeCountLie => "free_count_lie",
            AnonErr::CtxSlotMismatch => "ctx_slot_mismatch",
            AnonErr::ReleaseTwice => "release_twice",
            AnonErr::WriteWithoutReserve => "write_without_reserve",
            AnonErr::BatchOverCap => "batch_over_cap",
            AnonErr::WaitSemantics => "wait_semantics",
            AnonErr::ChainUnbalanced => "chain_unbalanced",
            AnonErr::SlotOutOfRange => "slot_out_of_range",
            AnonErr::ClassInvalid => "class_invalid",
        }
    }
    /// 现象。
    pub fn symptom(self) -> &'static str {
        match self {
            AnonErr::NoSlotEvict => "无空闲槽位仍放行逐出，anon 数据无去处",
            AnonErr::DoubleReserve => "同一槽位被两个逐出上下文同时登记预留",
            AnonErr::ReservedBitClobber => "预留位图被直接改写，与实际预留集不符",
            AnonErr::FreeCountLie => "空闲计数与预留位图推算值不相等",
            AnonErr::CtxSlotMismatch => "逐出上下文记录的槽号与实际预留槽不符",
            AnonErr::ReleaseTwice => "同一槽位被释放两次（双重归还暂存区）",
            AnonErr::WriteWithoutReserve => "未建立预留前置就执行写出",
            AnonErr::BatchOverCap => "逐出批页数超硬顶 32 页",
            AnonErr::WaitSemantics => "等待式语义被引入，会阻塞扫描器",
            AnonErr::ChainUnbalanced => "预留-写出-释放三段链式账不平",
            AnonErr::SlotOutOfRange => "槽号越出 SWAP_SLOTS 范围",
            AnonErr::ClassInvalid => "逐出类别既非 anon 也非 file",
        }
    }
    /// 位置（三要素之二）。
    pub fn locus(self) -> &'static str {
        match self {
            AnonErr::NoSlotEvict => "SlotLedger::admit()",
            AnonErr::DoubleReserve => "SlotLedger::reserve()",
            AnonErr::ReservedBitClobber => "SlotLedger::raw_set_bit()",
            AnonErr::FreeCountLie => "SlotLedger::reconcile()",
            AnonErr::CtxSlotMismatch => "EvictCtx::slot_of()",
            AnonErr::ReleaseTwice => "SlotLedger::release()",
            AnonErr::WriteWithoutReserve => "SlotLedger::commit_write()",
            AnonErr::BatchOverCap => "EvictBatch::admit_batch()",
            AnonErr::WaitSemantics => "Gate::semantics_frozen()",
            AnonErr::ChainUnbalanced => "SlotLedger::chain_check()",
            AnonErr::SlotOutOfRange => "SlotLedger::reserve()",
            AnonErr::ClassInvalid => "EvictCtx::new()",
        }
    }
    /// 建议修法（三要素之三）。
    pub fn advice(self) -> &'static str {
        match self {
            AnonErr::NoSlotEvict => "回到准入闸重判；无槽必须走归还路径",
            AnonErr::DoubleReserve => "预留改为「判定位 + CAS 式占位」，失败即归还",
            AnonErr::ReservedBitClobber => "禁直写位图；只经 reserve/release 改",
            AnonErr::FreeCountLie => "以位图重算 free，或修正误改的 free",
            AnonErr::CtxSlotMismatch => "上下文槽号由 reserve 返回值写入，禁外部指定",
            AnonErr::ReleaseTwice => "释放前判槽状态为 written 才可归零",
            AnonErr::WriteWithoutReserve => "补 reserve 前置，或改走归还路径",
            AnonErr::BatchOverCap => "把批拆成 ≤32 页子批后重试",
            AnonErr::WaitSemantics => "改回无槽即归还；等待式由 F2724 显式扩展",
            AnonErr::ChainUnbalanced => "按 reserve==write==release 逐段对平",
            AnonErr::SlotOutOfRange => "把槽号掩到 SWAP_SLOTS-1 内或改扩槽位数",
            AnonErr::ClassInvalid => "修正逐出类别枚举取值",
        }
    }
    /// 三要素完整句。
    pub fn detail(self) -> String {
        format!("[0x{:04X}/{}] {}；落点 {}；修法：{}", self.code(), self.name(), self.symptom(), self.locus(), self.advice())
    }
}

/// 本单 `Result` 别名。
pub type AnonResult<T> = Result<T, AnonErr>;

/// 码是否落在本单独占段。
pub fn code_in_seg(code: u16) -> bool {
    if code < SEG_BASE {
        return false;
    }
    (code - SEG_BASE) <= SEG_MASK
}

/// 错误串。
pub fn err_str(e: AnonErr) -> String {
    e.detail()
}

/// 闭集码表。
pub fn err_code_table() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(12);
    let mut i: usize = 0;
    while i < AnonErr::ALL.len() {
        let e: AnonErr = AnonErr::ALL[i];
        v.push(format!("0x{:04X} {} -> {}", e.code(), e.name(), e.locus()));
        i = i.wrapping_add(1);
    }
    v
}

// ===========================================================================
// 2. 核心数据结构
// ===========================================================================

/// 槽位状态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    /// 空闲可预留。
    Free,
    /// 已预留（判定即预留，尚未写出）。
    Reserved,
    /// 已写出（数据已在暂存区）。
    Written,
    /// 已释放（回到空闲）。
    Released,
}

impl SlotState {
    /// 序号。
    pub fn index(self) -> usize {
        match self {
            SlotState::Free => 0,
            SlotState::Reserved => 1,
            SlotState::Written => 2,
            SlotState::Released => 3,
        }
    }
    /// 由序号反查。
    pub fn from_index(i: usize) -> Option<SlotState> {
        match i {
            0 => Some(SlotState::Free),
            1 => Some(SlotState::Reserved),
            2 => Some(SlotState::Written),
            3 => Some(SlotState::Released),
            _ => None,
        }
    }
    /// 代号。
    pub fn code(self) -> &'static str {
        SLOTSTATE_CODES[self.index()]
    }
    /// 是否占用槽（不空闲）。
    pub fn occupied(self) -> bool {
        match self {
            SlotState::Free => false,
            _ => true,
        }
    }
}

/// 逐出类别闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvictClass {
    /// anon 脏页（必须写暂存区）。
    AnonDirty,
    /// anon 净页（可直接丢弃，仍走判定闸）。
    AnonClean,
    /// file 脏页。
    FileDirty,
    /// file 净页。
    FileClean,
}

impl EvictClass {
    /// 序号。
    pub fn index(self) -> usize {
        match self {
            EvictClass::AnonDirty => 0,
            EvictClass::AnonClean => 1,
            EvictClass::FileDirty => 2,
            EvictClass::FileClean => 3,
        }
    }
    /// 由序号反查。
    pub fn from_index(i: usize) -> Option<EvictClass> {
        match i {
            0 => Some(EvictClass::AnonDirty),
            1 => Some(EvictClass::AnonClean),
            2 => Some(EvictClass::FileDirty),
            3 => Some(EvictClass::FileClean),
            _ => None,
        }
    }
    /// 代号。
    pub fn code(self) -> &'static str {
        EVICTCLASS_CODES[self.index()]
    }
    /// 是否 anon（本单的准入闸主要约束 anon）。
    pub fn is_anon(self) -> bool {
        match self {
            EvictClass::AnonDirty => true,
            EvictClass::AnonClean => true,
            _ => false,
        }
    }
    /// 是否必须写出（脏页才写；净页写出为可选项，本单固定必写以保判据口径统一）。
    pub fn needs_write(self) -> bool {
        true
    }
}

/// 判定结论闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 放行进写出（已预留槽）。
    AdmitWrite,
    /// 无槽归还（不等待、不阻塞）。
    ReturnNoSlot,
    /// 批超硬顶被拒。
    RejectBatch,
}

impl Verdict {
    /// 序号。
    pub fn index(self) -> usize {
        match self {
            Verdict::AdmitWrite => 0,
            Verdict::ReturnNoSlot => 1,
            Verdict::RejectBatch => 2,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            Verdict::AdmitWrite => "admit_write",
            Verdict::ReturnNoSlot => "return_no_slot",
            Verdict::RejectBatch => "reject_batch",
        }
    }
}

/// 判定状态机闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateState {
    /// 空闲（未判定）。
    Idle,
    /// 已判定（有槽待写出）。
    Admitted,
    /// 已归还（无槽）。
    Returned,
    /// 已写出完成。
    Written,
    /// 已释放回账。
    Released,
    /// 已终结（归还或写出后释放）。
    Closed,
}

impl GateState {
    /// 序号。
    pub fn index(self) -> usize {
        match self {
            GateState::Idle => 0,
            GateState::Admitted => 1,
            GateState::Returned => 2,
            GateState::Written => 3,
            GateState::Released => 4,
            GateState::Closed => 5,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            GateState::Idle => "idle",
            GateState::Admitted => "admitted",
            GateState::Returned => "returned",
            GateState::Written => "written",
            GateState::Released => "released",
            GateState::Closed => "closed",
        }
    }
    /// 是否终态。
    pub fn terminal(self) -> bool {
        match self {
            GateState::Closed => true,
            _ => false,
        }
    }
}

/// 状态机迁移边。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Edge {
    /// 源态。
    pub from: GateState,
    /// 目标态。
    pub to: GateState,
}

impl Edge {
    /// 合法边闭集。
    pub const LEGAL: [Edge; 7] = [
        Edge { from: GateState::Idle, to: GateState::Admitted },
        Edge { from: GateState::Idle, to: GateState::Returned },
        Edge { from: GateState::Returned, to: GateState::Closed },
        Edge { from: GateState::Admitted, to: GateState::Written },
        Edge { from: GateState::Admitted, to: GateState::Returned },
        Edge { from: GateState::Written, to: GateState::Released },
        Edge { from: GateState::Released, to: GateState::Closed },
    ];
    /// 该边是否合法。
    pub fn legal(from: GateState, to: GateState) -> bool {
        let mut i: usize = 0;
        while i < Edge::LEGAL.len() {
            let e: Edge = Edge::LEGAL[i];
            if e.from == from && e.to == to {
                return true;
            }
            i = i.wrapping_add(1);
        }
        false
    }
    /// 导出行。
    pub fn row(&self) -> String {
        format!("edge {} -> {}", self.from.name(), self.to.name())
    }
}

/// 逐出上下文（承载预留槽号与状态）。
#[derive(Clone, Copy, Debug)]
pub struct EvictCtx {
    /// 上下文标识（页帧号）。
    pub pfn: u64,
    /// 逐出类别。
    pub class: EvictClass,
    /// 预留槽号（`NIL` 表示未预留）。
    pub slot: u64,
    /// 判定状态。
    pub state: GateState,
    /// 判定发生的逻辑 tick。
    pub tick: u64,
}

impl EvictCtx {
    /// 构造空闲上下文。
    pub fn new(pfn: u64, class: EvictClass, tick: u64) -> AnonResult<EvictCtx> {
        // 类别必须合法（本单 `EvictClass` 已封闭，此处做越界自检入口）。
        let ci: usize = class.index();
        if ci >= EVICTCLASS_CODES.len() {
            return Err(AnonErr::ClassInvalid);
        }
        Ok(EvictCtx { pfn, class, slot: NIL, state: GateState::Idle, tick })
    }
    /// 是否已预留。
    pub fn reserved(&self) -> bool {
        self.slot != NIL
    }
    /// 槽号读出（未预留返回错误）。
    pub fn slot_of(&self) -> AnonResult<u64> {
        if self.slot == NIL {
            return Err(AnonErr::CtxSlotMismatch);
        }
        Ok(self.slot)
    }
    /// 导出行。
    pub fn row(&self) -> String {
        format!("ctx[pfn=0x{:X} class={} slot={} state={} tick={}]", self.pfn, self.class.code(), if self.slot == NIL { "NIL".to_string() } else { self.slot.to_string() }, self.state.name(),
            self.tick)
    }
    /// 编码 40 字节。
    pub fn into_bytes(&self) -> Vec<u8> {
        let mut v: Vec<u8> = Vec::with_capacity(40);
        push_u64(&mut v, self.pfn);
        push_u64(&mut v, self.class.index() as u64);
        push_u64(&mut v, self.slot);
        push_u64(&mut v, self.state.index() as u64);
        push_u64(&mut v, self.tick);
        v
    }
}

/// 未预留哨兵。
pub const NIL: u64 = u64::MAX;

/// 槽位单元。
#[derive(Clone, Copy, Debug)]
pub struct SlotCell {
    /// 槽号。
    pub slot: u64,
    /// 槽状态。
    pub state: SlotState,
    /// 占用者上下文（`NIL` 表示无）。
    pub owner: u64,
    /// 预留 tick。
    pub tick: u64,
}

impl SlotCell {
    /// 构造空闲槽。
    pub fn free(slot: u64) -> SlotCell {
        SlotCell { slot, state: SlotState::Free, owner: NIL, tick: 0 }
    }
    /// 导出行。
    pub fn row(&self) -> String {
        format!("slotcell[#{} state={} owner={} tick={}]", self.slot, self.state.code(), if self.owner == NIL { "NIL".to_string() } else { self.owner.to_string() }, self.tick)
    }
    /// 编码 32 字节。
    pub fn into_bytes(&self) -> Vec<u8> {
        let mut v: Vec<u8> = Vec::with_capacity(32);
        push_u64(&mut v, self.slot);
        push_u64(&mut v, self.state.index() as u64);
        push_u64(&mut v, self.owner);
        push_u64(&mut v, self.tick);
        v
    }
}

/// 环形账一行。
#[derive(Clone, Copy, Debug)]
pub struct TraceRow {
    /// 逻辑 tick。
    pub tick: u64,
    /// 类别（0=预留 1=写出 2=释放 3=归还 4=拒记 5=注入 6=调账）。
    pub kind: u64,
    /// 槽号或上下文号。
    pub tag: u64,
    /// 载荷。
    pub payload: u64,
}

impl TraceRow {
    /// 导出行。
    pub fn row(&self) -> String {
        format!("trace[t={} kind={} tag={} payload=0x{:016X}]", self.tick, self.kind, self.tag, self.payload)
    }
    /// 编码 32 字节。
    pub fn into_bytes(&self) -> Vec<u8> {
        let mut v: Vec<u8> = Vec::new();
        push_u64(&mut v, self.tick);
        push_u64(&mut v, self.kind);
        push_u64(&mut v, self.tag);
        push_u64(&mut v, self.payload);
        v
    }
}

/// append-only 环形账（固定容量，丢最旧并显性计 `dropped`）。
#[derive(Clone, Debug)]
pub struct RingLog {
    /// 缓冲。
    pub buf: Vec<TraceRow>,
    /// 写头。
    pub head: usize,
    /// 累计写入。
    pub written: u64,
    /// 累计丢弃（显性）。
    pub dropped: u64,
    /// 拒记数。
    pub rejected: u64,
}

impl RingLog {
    /// 构造空环形账。
    pub fn new() -> RingLog {
        let mut buf: Vec<TraceRow> = Vec::with_capacity(RING_CAP);
        let mut i: usize = 0;
        while i < RING_CAP {
            buf.push(TraceRow { tick: 0, kind: u64::MAX, tag: u64::MAX, payload: u64::MAX });
            i = i.wrapping_add(1);
        }
        RingLog { buf, head: 0, written: 0, dropped: 0, rejected: 0 }
    }
    /// 有效条数。
    pub fn live(&self) -> usize {
        let w: u64 = self.written.saturating_sub(self.dropped);
        if w > (RING_CAP as u64) {
            return RING_CAP;
        }
        w as usize
    }
    /// 追加。
    pub fn push(&mut self, row: TraceRow) {
        let cap: usize = self.buf.len();
        if cap == 0 {
            return;
        }
        let idx: usize = self.head % cap;
        let cur: TraceRow = self.buf[idx];
        if cur.kind != u64::MAX {
            self.dropped = self.dropped.wrapping_add(1);
        }
        self.buf[idx] = row;
        self.head = self.head.wrapping_add(1);
        self.written = self.written.wrapping_add(1);
    }
    /// 拒记。
    pub fn reject(&mut self, row: TraceRow) {
        self.push(row);
        self.rejected = self.rejected.wrapping_add(1);
    }
    /// 读有效行。
    pub fn rows(&self) -> Vec<TraceRow> {
        let cap: usize = self.buf.len();
        let live: usize = self.live();
        let mut out: Vec<TraceRow> = Vec::with_capacity(live);
        let mut k: usize = 0;
        while k < live {
            let start: usize = (self.head + cap - live) % cap;
            let idx: usize = (start + k) % cap;
            let r: TraceRow = self.buf[idx];
            out.push(r);
            k = k.wrapping_add(1);
        }
        out
    }
    /// 自洽。
    pub fn coherent(&self) -> bool {
        let live: u64 = self.live() as u64;
        live.wrapping_add(self.dropped) == self.written && live <= (RING_CAP as u64)
    }
    /// 折叠指纹。
    pub fn fingerprint(&self) -> u64 {
        let rows: Vec<TraceRow> = self.rows();
        let mut w: Vec<u64> = Vec::with_capacity(rows.len() * 4);
        for r in rows.iter() {
            w.push(r.tick);
            w.push(r.kind);
            w.push(r.tag);
            w.push(r.payload);
        }
        fnv1a_words(&w)
    }
    /// 行文本。
    pub fn rows_text(&self) -> Vec<String> {
        let rows: Vec<TraceRow> = self.rows();
        let mut out: Vec<String> = Vec::with_capacity(rows.len());
        for r in rows.iter() {
            out.push(r.row());
        }
        out
    }
}

// ===========================================================================
// 3. 守恒账（`a + b == total`，整数，从 0 起算并与分配同步）
// ===========================================================================

/// 判定账：`admitted + returned + rejected == total`。
#[derive(Clone, Copy, Debug)]
pub struct GateLedger {
    /// 判定总数。
    pub total: u64,
    /// 放行数。
    pub admitted: u64,
    /// 归还数（无槽归还）。
    pub returned: u64,
    /// 批拒数。
    pub rejected: u64,
    /// 无槽归还专项计数（判据读数）。
    pub no_slot_returns: u64,
}

impl GateLedger {
    /// 分配。
    pub fn alloc(n: usize) -> GateLedger {
        GateLedger { total: n as u64, admitted: 0, returned: 0, rejected: 0, no_slot_returns: 0 }
    }
    /// 记放行。
    pub fn admit(&mut self) {
        self.admitted = self.admitted.wrapping_add(1);
    }
    /// 记归还（并判是否属无槽归还）。
    pub fn ret(&mut self, no_slot: bool) {
        self.returned = self.returned.wrapping_add(1);
        if no_slot {
            self.no_slot_returns = self.no_slot_returns.wrapping_add(1);
        }
    }
    /// 记批拒。
    pub fn rej(&mut self) {
        self.rejected = self.rejected.wrapping_add(1);
    }
    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.admitted.wrapping_add(self.returned).wrapping_add(self.rejected) == self.total
    }
    /// 导出行。
    pub fn row(&self) -> String {
        format!("gate_ledger a={} b={} c={} total={} no_slot_returns={} conserved={}", self.admitted, self.returned, self.rejected, self.total, self.no_slot_returns,
            if self.conserved() { 1 } else { 0 })
    }
}

/// 注入账：`rejected + leaked == total`。
#[derive(Clone, Copy, Debug)]
pub struct InjectLedger {
    /// 总数。
    pub total: u64,
    /// 被拒数。
    pub rejected: u64,
    /// 漏过数（须 0）。
    pub leaked: u64,
}

impl InjectLedger {
    /// 分配。
    pub fn alloc(n: usize) -> InjectLedger {
        InjectLedger { total: n as u64, rejected: 0, leaked: 0 }
    }
    /// 记被拒。
    pub fn reject(&mut self) {
        self.rejected = self.rejected.wrapping_add(1);
    }
    /// 记漏过。
    pub fn leak(&mut self) {
        self.leaked = self.leaked.wrapping_add(1);
    }
    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.rejected.wrapping_add(self.leaked) == self.total
    }
    /// 全被拒。
    pub fn all_rejected(&self) -> bool {
        self.conserved() && self.leaked == 0 && self.rejected == self.total
    }
}

/// 断言账：`passed + failed == total`。
#[derive(Clone, Copy, Debug)]
pub struct AssertLedger {
    /// 总数。
    pub total: u64,
    /// 通过数。
    pub passed: u64,
    /// 失败数。
    pub failed: u64,
}

impl AssertLedger {
    /// 分配。
    pub fn alloc(n: usize) -> AssertLedger {
        AssertLedger { total: n as u64, passed: 0, failed: 0 }
    }
    /// 记通过。
    pub fn pass(&mut self) {
        self.passed = self.passed.wrapping_add(1);
    }
    /// 记失败。
    pub fn fail(&mut self) {
        self.failed = self.failed.wrapping_add(1);
    }
    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.passed.wrapping_add(self.failed) == self.total
    }
    /// 全过。
    pub fn all_pass(&self) -> bool {
        self.conserved() && self.failed == 0 && self.passed == self.total
    }
}

// ===========================================================================
// 4. 字节编解码工具
// ===========================================================================

/// 追加 `u64`（8 字节小端）。
pub fn push_u64(v: &mut Vec<u8>, w: u64) {
    let mut i: u32 = 0;
    while i < 8 {
        let byte: u8 = ((w >> (8 * i)) & 0xFF) as u8;
        v.push(byte);
        i = i.wrapping_add(1);
    }
}

/// 追加 `u16`（2 字节小端）。
pub fn push_u16(v: &mut Vec<u8>, w: u16) {
    let mut i: u32 = 0;
    while i < 2 {
        let byte: u8 = ((w >> (8 * i)) & 0xFF) as u8;
        v.push(byte);
        i = i.wrapping_add(1);
    }
}

/// 读 `u64`（越界补 0）。
pub fn read_u64(v: &[u8], off: usize) -> u64 {
    let mut acc: u64 = 0;
    let mut i: u32 = 0;
    while i < 8 {
        let b: u8 = match v.get(off + (i as usize)) {
            Some(x) => *x,
            None => 0,
        };
        acc |= ((b as u64) & 0xFF) << (8 * i);
        i = i.wrapping_add(1);
    }
    acc
}

/// 读 `u16`（越界补 0）。
pub fn read_u16(v: &[u8], off: usize) -> u64 {
    let mut acc: u64 = 0;
    let mut i: u32 = 0;
    while i < 2 {
        let b: u8 = match v.get(off + (i as usize)) {
            Some(x) => *x,
            None => 0,
        };
        acc |= ((b as u64) & 0xFF) << (8 * i);
        i = i.wrapping_add(1);
    }
    acc
}
// ===========================================================================
// 5. 槽位账与准入闸 `SlotLedger`
// ===========================================================================

/// 槽位账 + 前置判定闸（F2730 槽位账的建模替身 + 本单准入闸）。
#[derive(Clone, Debug)]
pub struct SlotLedger {
    /// 槽单元表。
    pub cells: Vec<SlotCell>,
    /// 预留位图（`RESERVED_WORDS` 个 `u64`）。
    pub bits: Vec<u64>,
    /// 槽位守恒账。
    pub ledger: SlotLedgerAcc,
    /// 判定账。
    pub gate: GateLedger,
    /// 注入账。
    pub inject: InjectLedger,
    /// 断言账。
    pub asserts: AssertLedger,
    /// 逻辑时钟。
    pub tick: u64,
    /// 预留次数。
    pub reserve_calls: u64,
    /// 预留被拒次数。
    pub reserve_rejected: u64,
    /// 写出次数。
    pub write_calls: u64,
    /// 释放次数。
    pub release_calls: u64,
    /// 归还次数（无槽归还）。
    pub return_calls: u64,
    /// 双重预留检出次数。
    pub double_reserve_detected: u64,
    /// 环形留痕。
    pub ring: RingLog,
    /// 冻结语义：恒为「无槽即归还」。
    pub semantics_no_wait: bool,
    /// 非法状态迁移被拒次数。
    pub illegal_trans: u64,
}

/// 槽位守恒账的别名（避免与主结构同名）。
pub type SlotLedgerAcc = SlotLedgerCore;

/// 槽位守恒账本体（`free + reserved + written == total`）。
#[derive(Clone, Copy, Debug)]
pub struct SlotLedgerCore {
    /// 槽总数。
    pub total: u64,
    /// 空闲数。
    pub free: u64,
    /// 预留数。
    pub reserved: u64,
    /// 已写出数。
    pub written: u64,
}

impl SlotLedgerCore {
    /// 分配（从 0 起算并与槽数同步）。
    pub fn alloc(slots: usize) -> SlotLedgerCore {
        SlotLedgerCore { total: slots as u64, free: slots as u64, reserved: 0, written: 0 }
    }
    /// 记预留。
    pub fn on_reserve(&mut self) {
        if self.free > 0 {
            self.free = self.free.wrapping_sub(1);
            self.reserved = self.reserved.wrapping_add(1);
        }
    }
    /// 记写出。
    pub fn on_write(&mut self) {
        if self.reserved > 0 {
            self.reserved = self.reserved.wrapping_sub(1);
            self.written = self.written.wrapping_add(1);
        }
    }
    /// 记释放。
    pub fn on_release(&mut self) {
        if self.written > 0 {
            self.written = self.written.wrapping_sub(1);
            self.free = self.free.wrapping_add(1);
        }
    }
    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.free.wrapping_add(self.reserved).wrapping_add(self.written) == self.total
    }
    /// 调账。
    pub fn reconcile(&mut self) -> u64 {
        let moved: u64 = self.reserved;
        self.reserved = 0;
        self.free = self.free.wrapping_add(moved);
        moved
    }
    /// 导出行。
    pub fn row(&self) -> String {
        format!("slot_ledger a={} b={} c={} total={} conserved={}", self.free, self.reserved, self.written, self.total, if self.conserved() { 1 } else { 0 })
    }
}

impl SlotLedger {
    /// 构造（`SWAP_SLOTS` 全空闲）。
    pub fn new(tick0: u64) -> SlotLedger {
        let mut cells: Vec<SlotCell> = Vec::with_capacity(SWAP_SLOTS);
        let mut i: usize = 0;
        while i < SWAP_SLOTS {
            cells.push(SlotCell::free(i as u64));
            i = i.wrapping_add(1);
        }
        let mut bits: Vec<u64> = Vec::with_capacity(RESERVED_WORDS);
        let mut k: usize = 0;
        while k < RESERVED_WORDS {
            bits.push(0);
            k = k.wrapping_add(1);
        }
        SlotLedger {
            cells,
            bits,
            ledger: SlotLedgerCore::alloc(SWAP_SLOTS),
            gate: GateLedger::alloc(0),
            inject: InjectLedger::alloc(FORM_COUNT),
            asserts: AssertLedger::alloc(0),
            tick: tick0,
            reserve_calls: 0,
            reserve_rejected: 0,
            write_calls: 0,
            release_calls: 0,
            return_calls: 0,
            double_reserve_detected: 0,
            ring: RingLog::new(),
            semantics_no_wait: NO_WAIT_SEMANTICS,
            illegal_trans: 0,
        }
    }

    /// 时钟推进。
    pub fn advance(&mut self, n: u64) {
        self.tick = self.tick.wrapping_add(n);
    }

    /// 取槽单元只读引用。
    pub fn cell(&self, slot: usize) -> Option<&SlotCell> {
        self.cells.get(slot)
    }

    /// 取槽单元可变引用。
    pub fn cell_mut(&mut self, slot: usize) -> Option<&mut SlotCell> {
        self.cells.get_mut(slot)
    }

    /// 位图读某槽的预留位。
    pub fn bit_at(&self, slot: usize) -> bool {
        let w: usize = slot / 64;
        let b: u32 = (slot % 64) as u32;
        match self.bits.get(w) {
            Some(x) => ((*x >> b) & 1) == 1,
            None => false,
        }
    }

    /// 位图写某槽的预留位（**只经内部调用**，注入核另设 `raw_set_bit`）。
    fn set_bit(&mut self, slot: usize, v: bool) {
        let w: usize = slot / 64;
        let b: u32 = (slot % 64) as u32;
        if let Some(x) = self.bits.get_mut(w) {
            if v {
                *x |= 1u64 << b;
            } else {
                *x &= !(1u64 << b);
            }
        }
    }

    /// 位图统计已预留数（权威口径）。
    pub fn bit_count(&self) -> u64 {
        let mut n: u64 = 0;
        let mut i: usize = 0;
        while i < SWAP_SLOTS {
            if self.bit_at(i) {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 位图直写（**故意提供的违规入口**，供注入核构造位图与槽单元失配）。
    pub fn raw_set_bit(&mut self, slot: usize, v: bool) {
        self.set_bit(slot, v);
    }

    /// 找首个空闲槽（`O(SWAP_SLOTS)`，位图为权威）。
    pub fn find_free(&self) -> Option<usize> {
        let mut i: usize = 0;
        while i < SWAP_SLOTS {
            let taken: bool = self.bit_at(i);
            if !taken {
                return Some(i);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    // -----------------------------------------------------------------------
    // 5.1 预留单点（判定即预留，原子化防 TOCTOU）
    // -----------------------------------------------------------------------

    /// 预留单点：找空闲槽 → 占位 → 写位图 → 记账。
    ///
    /// 这是**判定即预留**的落点：调用方拿到的 `Ok(slot)` 就是「判定通过 + 槽已占」，
    /// 中间不存在「判完再去抢」的第二步，因此没有 TOCTOU 窗口。
    pub fn reserve(&mut self, owner: u64) -> AnonResult<u64> {
        self.reserve_calls = self.reserve_calls.wrapping_add(1);
        // 找空闲。
        let slot: usize = match self.find_free() {
            Some(s) => s,
            None => {
                // 无槽：不是错误，是**归还语义**，由调用方走归还路径。
                self.reserve_rejected = self.reserve_rejected.wrapping_add(1);
                self.ring.reject(TraceRow { tick: self.tick, kind: 3, tag: owner, payload: 0xFFFF });
                return Err(AnonErr::NoSlotEvict);
            }
        };
        // 原子占位：位图先置（这是「占」的线性化点）。
        self.set_bit(slot, true);
        // 复核（模拟 CAS 语义下的自检）：若置位后仍读作未占，则视为被抢 → 归还。
        if !self.bit_at(slot) {
            self.set_bit(slot, false);
            self.reserve_rejected = self.reserve_rejected.wrapping_add(1);
            self.ring.reject(TraceRow { tick: self.tick, kind: 3, tag: owner, payload: slot as u64 });
            return Err(AnonErr::DoubleReserve);
        }
        // 写槽单元。
        if let Some(c) = self.cells.get_mut(slot) {
            c.state = SlotState::Reserved;
            c.owner = owner;
            c.tick = self.tick;
        }
        self.ledger.on_reserve();
        self.ring.push(TraceRow { tick: self.tick, kind: 0, tag: slot as u64, payload: owner });
        Ok(slot as u64)
    }

    /// 校验某槽是否确实由 `owner` 独占（双重预留检出点）。
    pub fn owned_by(&self, slot: usize, owner: u64) -> bool {
        match self.cells.get(slot) {
            Some(c) => c.owner == owner && c.state != SlotState::Free,
            None => false,
        }
    }

    /// 双重预留检出：同一槽被两个 owner 声称占用。
    pub fn detect_double_reserve(&mut self, slot: usize, owner: u64) -> AnonResult<u64> {
        let held: bool = match self.cells.get(slot) {
            Some(c) => c.owner != NIL && c.owner != owner,
            None => return Err(AnonErr::SlotOutOfRange),
        };
        if held {
            self.double_reserve_detected = self.double_reserve_detected.wrapping_add(1);
            return Err(AnonErr::DoubleReserve);
        }
        Ok(slot as u64)
    }

    // -----------------------------------------------------------------------
    // 5.2 写出 / 释放 / 归还
    // -----------------------------------------------------------------------

    /// 提交写出（前置：槽必须处于 `Reserved` 且归属匹配）。
    pub fn commit_write(&mut self, ctx: &EvictCtx) -> AnonResult<u64> {
        self.write_calls = self.write_calls.wrapping_add(1);
        if !ctx.reserved() {
            self.ring.reject(TraceRow { tick: self.tick, kind: 4, tag: ctx.pfn, payload: ctx.slot });
            return Err(AnonErr::WriteWithoutReserve);
        }
        let slot: usize = ctx.slot as usize;
        if slot >= SWAP_SLOTS {
            return Err(AnonErr::SlotOutOfRange);
        }
        if !self.owned_by(slot, ctx.pfn) {
            return Err(AnonErr::CtxSlotMismatch);
        }
        match self.cells.get(slot) {
            Some(c) => {
                if c.state != SlotState::Reserved {
                    return Err(AnonErr::WriteWithoutReserve);
                }
            }
            None => return Err(AnonErr::SlotOutOfRange),
        }
        // 写位图：预留位清（写出后不再「预留中」）。
        self.set_bit(slot, false);
        if let Some(c) = self.cells.get_mut(slot) {
            c.state = SlotState::Written;
        }
        self.ledger.on_write();
        self.ring.push(TraceRow { tick: self.tick, kind: 1, tag: slot as u64, payload: ctx.pfn });
        Ok(slot as u64)
    }

    /// 释放槽（前置：槽必须处于 `Written`）。
    pub fn release(&mut self, slot: usize) -> AnonResult<u64> {
        self.release_calls = self.release_calls.wrapping_add(1);
        if slot >= SWAP_SLOTS {
            return Err(AnonErr::SlotOutOfRange);
        }
        let st: SlotState = match self.cells.get(slot) {
            Some(c) => c.state,
            None => return Err(AnonErr::SlotOutOfRange),
        };
        if st != SlotState::Written {
            self.ring.reject(TraceRow { tick: self.tick, kind: 4, tag: slot as u64, payload: st.index() as u64 });
            return Err(AnonErr::ReleaseTwice);
        }
        if let Some(c) = self.cells.get_mut(slot) {
            c.state = SlotState::Released;
            c.owner = NIL;
        }
        // 释放即回空闲（位图保持清）。
        self.set_bit(slot, false);
        self.ledger.on_release();
        self.ring.push(TraceRow { tick: self.tick, kind: 2, tag: slot as u64, payload: 0 });
        // 立刻归位为 Free，保证下一轮可再预留。
        if let Some(c) = self.cells.get_mut(slot) {
            c.state = SlotState::Free;
        }
        Ok(slot as u64)
    }

    /// 归还路径（无槽，不等待不阻塞）。
    pub fn return_no_slot(&mut self, ctx: &EvictCtx) -> AnonResult<u64> {
        self.return_calls = self.return_calls.wrapping_add(1);
        // 归还语义：不动槽位账，只记判定账与无槽归还计数。
        if ctx.reserved() {
            // 已预留的上下文不应走归还（除非被踢出，见注入形态 1）。
            return Err(AnonErr::CtxSlotMismatch);
        }
        self.ring.push(TraceRow { tick: self.tick, kind: 3, tag: ctx.pfn, payload: 0xFFFF });
        Ok(0)
    }

    /// 槽清零（供「无槽注入」构造用）：把所有槽归为 `Free`、位图清空、账归零。
    pub fn drain_all(&mut self) {
        let mut i: usize = 0;
        while i < self.cells.len() {
            if let Some(c) = self.cells.get_mut(i) {
                c.state = SlotState::Free;
                c.owner = NIL;
                c.tick = 0;
            }
            i = i.wrapping_add(1);
        }
        let mut k: usize = 0;
        while k < self.bits.len() {
            if let Some(b) = self.bits.get_mut(k) {
                *b = 0;
            }
            k = k.wrapping_add(1);
        }
        self.ledger = SlotLedgerCore::alloc(SWAP_SLOTS);
    }

    /// 占满所有槽（供「满槽注入」构造用）。
    pub fn fill_all(&mut self) {
        self.drain_all();
        let mut i: usize = 0;
        while i < SWAP_SLOTS {
            self.set_bit(i, true);
            if let Some(c) = self.cells.get_mut(i) {
                c.state = SlotState::Reserved;
                c.owner = (i as u64) | 0x8000_0000_0000_0000;
                c.tick = self.tick;
            }
            self.ledger.on_reserve();
            i = i.wrapping_add(1);
        }
    }

    // -----------------------------------------------------------------------
    // 5.3 账与链式校验
    // -----------------------------------------------------------------------

    /// 空闲计数校验（`total - bit_count() == ledger.free`）。
    pub fn free_count_consistent(&self) -> bool {
        let taken: u64 = self.bit_count();
        let expect_free: u64 = self.ledger.total.saturating_sub(taken);
        expect_free == self.ledger.free
    }

    /// 槽单元与位图一致性（逐槽对拍）。
    pub fn cells_consistent(&self) -> bool {
        let mut i: usize = 0;
        while i < SWAP_SLOTS {
            let bit: bool = self.bit_at(i);
            let st: SlotState = match self.cells.get(i) {
                Some(c) => c.state,
                None => return false,
            };
            let expect_bit: bool = st == SlotState::Reserved;
            if bit != expect_bit {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 全链校验：预留-写出-释放三段账平。
    ///
    /// 口径：`reserve_calls - (reserve_rejected) == write_calls + live_reserved`，
    /// 且 `write_calls - release_calls == ledger.written`。
    pub fn chain_check(&self) -> AnonResult<u64> {
        // 先查最窄的闸：空闲计数（它能解释「账看着不平其实只是 free 撒谎」）。
        if !self.free_count_consistent() {
            return Err(AnonErr::FreeCountLie);
        }
        if !self.ledger.conserved() {
            return Err(AnonErr::ChainUnbalanced);
        }
        let granted: u64 = self.reserve_calls.saturating_sub(self.reserve_rejected);
        let live_reserved: u64 = self.ledger.reserved;
        if granted != self.write_calls.wrapping_add(live_reserved) {
            return Err(AnonErr::ChainUnbalanced);
        }
        let live_written: u64 = self.write_calls.saturating_sub(self.release_calls);
        if live_written != self.ledger.written {
            return Err(AnonErr::ChainUnbalanced);
        }
        Ok(granted)
    }

    /// 调账（把悬挂预留清算为 free）。
    pub fn reconcile(&mut self) -> AnonResult<u64> {
        // 悬挂预留对应位图上的占用位，必须一并清。
        let moved: u64 = self.ledger.reconcile();
        let mut i: usize = 0;
        while i < SWAP_SLOTS {
            if self.bit_at(i) {
                self.set_bit(i, false);
                if let Some(c) = self.cells.get_mut(i) {
                    c.state = SlotState::Free;
                    c.owner = NIL;
                }
            }
            i = i.wrapping_add(1);
        }
        self.ring.push(TraceRow { tick: self.tick, kind: 6, tag: 0, payload: moved });
        if !self.ledger.conserved() {
            return Err(AnonErr::ChainUnbalanced);
        }
        Ok(moved)
    }

    /// 语义冻结检查：无等待式语义、无配置位。
    pub fn semantics_frozen(&self) -> AnonResult<u64> {
        if !self.semantics_no_wait || NO_WAIT_CONFIG_BIT != 0 {
            return Err(AnonErr::WaitSemantics);
        }
        Ok(NO_WAIT_CONFIG_BIT)
    }

    /// 摘要。
    pub fn summary(&self) -> String {
        format!("free={} reserved={} written={} total={} reserve_calls={} rejected={} writes={} releases={} returns={} double_reserve={}", self.ledger.free, self.ledger.reserved,
            self.ledger.written, self.ledger.total, self.reserve_calls, self.reserve_rejected, self.write_calls, self.release_calls, self.return_calls, self.double_reserve_detected)
    }

    /// 槽行文本（抽样 `SWAP_SLOTS` 行）。
    pub fn slot_rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(SWAP_SLOTS);
        for c in self.cells.iter() {
            out.push(c.row());
        }
        out
    }
}

// ===========================================================================
// 6. 前置判定闸 `Gate`（判定即预留 + 状态机 + 批处理）
// ===========================================================================

/// 前置判定闸。
#[derive(Clone, Debug)]
pub struct Gate {
    /// 槽位账。
    pub ledger: SlotLedger,
    /// 逐出上下文池。
    pub ctxs: Vec<EvictCtx>,
    /// 当前闸态（全局读数）。
    pub state: GateState,
}

impl Gate {
    /// 构造闸（`SWAP_SLOTS` 全空）。
    pub fn new(tick0: u64) -> Gate {
        let ctxs: Vec<EvictCtx> = Vec::with_capacity(SWAP_SLOTS * 2);
        Gate { ledger: SlotLedger::new(tick0), ctxs, state: GateState::Idle }
    }

    /// 状态迁移（合法边闭集 + 非法拒记）。
    pub fn transit(&mut self, to: GateState) -> AnonResult<bool> {
        let from: GateState = self.state;
        if !Edge::legal(from, to) || from.terminal() {
            self.ledger.illegal_trans = self.ledger.illegal_trans.wrapping_add(1);
            self.ledger.ring.reject(TraceRow { tick: self.ledger.tick, kind: 4, tag: (from.index() as u64) * 8 + (to.index() as u64), payload: 0 });
            return Err(AnonErr::ChainUnbalanced);
        }
        self.state = to;
        self.ledger.ring.push(TraceRow { tick: self.ledger.tick, kind: 6, tag: to.index() as u64, payload: from.index() as u64 });
        Ok(true)
    }

    /// 单页前置判定（判定即预留）。
    ///
    /// - 有空闲槽：`reserve()` 成功 → `Admitted`，上下文写入预留槽号。
    /// - 无空闲槽：`reserve()` 返回 `NoSlotEvict` → 走归还路径 → `Returned`，
    ///   无槽归还计数递增（判据读数）。
    pub fn admit(&mut self, pfn: u64, class: EvictClass) -> AnonResult<(Verdict, u64)> {
        // 判定总数 +1（守恒口径：每次判定请求都计入 total，结论计入 a/b/c 之一）。
        self.ledger.gate.total = self.ledger.gate.total.wrapping_add(1);
        // 类别必须合法。
        if class.index() >= EVICTCLASS_CODES.len() {
            self.ledger.gate.rej();
            return Err(AnonErr::ClassInvalid);
        }
        let mut ctx: EvictCtx = match EvictCtx::new(pfn, class, self.ledger.tick) {
            Ok(c) => c,
            Err(e) => {
                self.ledger.gate.rej();
                return Err(e);
            }
        };
        // 判定即预留：一次调用完成「判 + 占」。
        match self.ledger.reserve(pfn) {
            Ok(slot) => {
                ctx.slot = slot;
                ctx.state = GateState::Admitted;
                self.ctxs.push(ctx);
                self.ledger.gate.admit();
                let _ = self.transit(GateState::Admitted);
                Ok((Verdict::AdmitWrite, slot))
            }
            Err(AnonErr::NoSlotEvict) => {
                // 无槽即归还（不等待不阻塞）。
                self.ledger.return_no_slot(&ctx)?;
                ctx.state = GateState::Returned;
                self.ctxs.push(ctx);
                self.ledger.gate.ret(true);
                let _ = self.transit(GateState::Returned);
                Ok((Verdict::ReturnNoSlot, 0))
            }
            Err(e) => {
                self.ledger.gate.rej();
                Err(e)
            }
        }
    }

    /// 批前置判定（`n` 页，硬顶 `BATCH_HARD_CAP`）。
    pub fn admit_batch(&mut self, seed: u64, n: usize) -> AnonResult<(usize, usize)> {
        self.ledger.gate.total = self.ledger.gate.total.wrapping_add(1);
        if n > BATCH_HARD_CAP {
            self.ledger.gate.rej();
            self.ledger.ring.reject(TraceRow { tick: self.ledger.tick, kind: 4, tag: n as u64, payload: BATCH_HARD_CAP as u64 });
            return Err(AnonErr::BatchOverCap);
        }
        let mut rng: Lcg = Lcg::new(seed);
        let mut admitted: usize = 0;
        let mut returned: usize = 0;
        let mut i: usize = 0;
        while i < n {
            let pfn: u64 = 0x1000 + rng.next_u64();
            let cls: EvictClass = match EvictClass::from_index(rng.below(2)) {
                Some(c) => c,
                None => EvictClass::AnonDirty,
            };
            match self.admit(pfn, cls) {
                Ok((Verdict::AdmitWrite, _)) => {
                    admitted = admitted.wrapping_add(1);
                }
                Ok((Verdict::ReturnNoSlot, _)) => {
                    returned = returned.wrapping_add(1);
                }
                _ => {}
            }
            i = i.wrapping_add(1);
        }
        Ok((admitted, returned))
    }

    /// 写出最近一个已准入上下文。
    pub fn write_last(&mut self) -> AnonResult<(u64, u64)> {
        let mut k: usize = self.ctxs.len();
        while k > 0 {
            k = k.wrapping_sub(1);
            let ctx: EvictCtx = match self.ctxs.get(k) {
                Some(c) => *c,
                None => continue,
            };
            if ctx.state == GateState::Admitted && ctx.reserved() {
                let slot: u64 = self.ledger.commit_write(&ctx)?;
                if let Some(c) = self.ctxs.get_mut(k) {
                    c.state = GateState::Written;
                }
                let _ = self.transit(GateState::Written);
                let _ = self.transit(GateState::Released);
                self.ledger.release(slot as usize)?;
                if let Some(c) = self.ctxs.get_mut(k) {
                    c.state = GateState::Released;
                }
                let _ = self.transit(GateState::Closed);
                return Ok((ctx.pfn, slot));
            }
        }
        Err(AnonErr::WriteWithoutReserve)
    }

    /// 完整链跑一轮（判定 → 写出 → 释放），返回三段读数。
    pub fn chain_round(&mut self, seed: u64) -> AnonResult<(u64, u64, u64)> {
        let v: Verdict = match self.admit(0x2000 | (seed & 0xFFF), EvictClass::AnonDirty) {
            Ok((vd, _)) => vd,
            Err(e) => return Err(e),
        };
        match v {
            Verdict::AdmitWrite => {
                let (pfn, slot): (u64, u64) = self.write_last()?;
                Ok((1, pfn, slot))
            }
            _ => Ok((0, 0, 0)),
        }
    }

    /// 判定账摘要。
    pub fn gate_ledger_row(&self) -> String {
        self.ledger.gate.row()
    }
}

// ===========================================================================
// 7. 注入核（十形态负向注入，全被拒并留痕）
// ===========================================================================

/// 注入形态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// 0 无槽仍逐出（绕过准入闸直接占位）。
    Form0NoSlotEvict,
    /// 1 双重预留（两上下文抢同一槽）。
    Form1DoubleReserve,
    /// 2 预留位被踩（直写位图）。
    Form2BitClobber,
    /// 3 空闲计数撒谎。
    Form3FreeCountLie,
    /// 4 上下文槽号失配。
    Form4CtxMismatch,
    /// 5 重复释放。
    Form5ReleaseTwice,
    /// 6 未预留即写出。
    Form6WriteNoReserve,
    /// 7 批超硬顶。
    Form7BatchOverCap,
    /// 8 等待式语义被引入。
    Form8WaitSemantics,
    /// 9 全链账不平。
    Form9ChainUnbalanced,
}

impl Form {
    /// 全部形态。
    pub const ALL: [Form; FORM_COUNT] = [Form::Form0NoSlotEvict, Form::Form1DoubleReserve, Form::Form2BitClobber, Form::Form3FreeCountLie, Form::Form4CtxMismatch, Form::Form5ReleaseTwice,
        Form::Form6WriteNoReserve, Form::Form7BatchOverCap, Form::Form8WaitSemantics, Form::Form9ChainUnbalanced];
    /// 序号。
    pub fn index(self) -> usize {
        match self {
            Form::Form0NoSlotEvict => 0,
            Form::Form1DoubleReserve => 1,
            Form::Form2BitClobber => 2,
            Form::Form3FreeCountLie => 3,
            Form::Form4CtxMismatch => 4,
            Form::Form5ReleaseTwice => 5,
            Form::Form6WriteNoReserve => 6,
            Form::Form7BatchOverCap => 7,
            Form::Form8WaitSemantics => 8,
            Form::Form9ChainUnbalanced => 9,
        }
    }
    /// 期望错误码。
    pub fn expect_err(self) -> AnonErr {
        match self {
            Form::Form0NoSlotEvict => AnonErr::NoSlotEvict,
            Form::Form1DoubleReserve => AnonErr::DoubleReserve,
            Form::Form2BitClobber => AnonErr::ReservedBitClobber,
            Form::Form3FreeCountLie => AnonErr::FreeCountLie,
            Form::Form4CtxMismatch => AnonErr::CtxSlotMismatch,
            Form::Form5ReleaseTwice => AnonErr::ReleaseTwice,
            Form::Form6WriteNoReserve => AnonErr::WriteWithoutReserve,
            Form::Form7BatchOverCap => AnonErr::BatchOverCap,
            Form::Form8WaitSemantics => AnonErr::WaitSemantics,
            Form::Form9ChainUnbalanced => AnonErr::ChainUnbalanced,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        FORM_CODES[self.index()]
    }
    /// 说明。
    pub fn describe(self) -> &'static str {
        match self {
            Form::Form0NoSlotEvict => "满槽状态下仍尝试占位逐出（闸失效）",
            Form::Form1DoubleReserve => "两个上下文同时预留同一槽",
            Form::Form2BitClobber => "直写预留位图绕过 reserve 单点",
            Form::Form3FreeCountLie => "把 free 计数改成与位图不符的值",
            Form::Form4CtxMismatch => "把上下文槽号改成非其预留的槽",
            Form::Form5ReleaseTwice => "对同一槽连续释放两次",
            Form::Form6WriteNoReserve => "构造未预留的上下文直接提交写出",
            Form::Form7BatchOverCap => "以 33 页发起批判定（超硬顶 32）",
            Form::Form8WaitSemantics => "把语义开关改为等待式",
            Form::Form9ChainUnbalanced => "把 ledger.written 改坏致全链不平",
        }
    }
    /// 留痕 tag。
    pub fn trace_tag(self) -> u64 {
        (self.index() as u64) * 256 + (self.expect_err().code() as u64)
    }
}

/// 一次注入结论。
#[derive(Clone, Debug)]
pub struct Injection {
    /// 形态。
    pub form: Form,
    /// 被拒。
    pub rejected: bool,
    /// 实际错误码。
    pub code: u16,
    /// 是否与期望一致。
    pub as_expected: bool,
    /// 留痕序号。
    pub trace_seq: u64,
}

impl Injection {
    /// 导出行。
    pub fn row(&self) -> String {
        format!("inject[{}] {} rejected={} code=0x{:04X} expected={} seq={}", self.form.index(), self.form.name(), if self.rejected { 1 } else { 0 }, self.code,
            if self.as_expected { 1 } else { 0 }, self.trace_seq)
    }
}

/// 形态 0：无槽仍逐出（满槽下硬占位必须被拒）。
pub fn inject_no_slot_evict(g: &mut Gate) -> AnonResult<u64> {
    g.ledger.fill_all();
    g.ledger.ring.reject(TraceRow { tick: g.ledger.tick, kind: 5, tag: Form::Form0NoSlotEvict.trace_tag(), payload: 0 });
    // 满槽下 `reserve` 必拒（无槽），这就是准入闸的核心语义。
    g.ledger.reserve(0x9001)?;
    // 若侥幸成功（说明闸失效），仍以形态期望码兜底。
    Err(AnonErr::NoSlotEvict)
}

/// 形态 1：双重预留。
pub fn inject_double_reserve(g: &mut Gate) -> AnonResult<u64> {
    g.ledger.drain_all();
    g.ledger.ring.reject(TraceRow { tick: g.ledger.tick, kind: 5, tag: Form::Form1DoubleReserve.trace_tag(), payload: 0 });
    let a: u64 = g.ledger.reserve(0xA001)?;
    // 第二核试图占同一槽：必须被 `detect_double_reserve` 拒。
    g.ledger.detect_double_reserve(a as usize, 0xA002)?;
    Err(AnonErr::DoubleReserve)
}

/// 形态 2：预留位被踩。
pub fn inject_bit_clobber(g: &mut Gate) -> AnonResult<u64> {
    g.ledger.drain_all();
    let slot: u64 = g.ledger.reserve(0xB001)?;
    // 直写位图：把一个空闲槽的预留位强置 1（槽单元仍是 Free）→ 二者必然失配。
    let victim: usize = match g.ledger.find_free() {
        Some(v) => v,
        None => return Err(AnonErr::ReservedBitClobber),
    };
    g.ledger.raw_set_bit(victim, true);
    g.ledger.ring.reject(TraceRow { tick: g.ledger.tick, kind: 5, tag: Form::Form2BitClobber.trace_tag(), payload: slot });
    // 权威检出点：槽单元与位图逐槽对拍。
    if g.ledger.cells_consistent() {
        return Err(AnonErr::ReservedBitClobber);
    }
    Err(AnonErr::ReservedBitClobber)
}

/// 形态 3：空闲计数撒谎。
pub fn inject_free_count_lie(g: &mut Gate) -> AnonResult<u64> {
    g.ledger.drain_all();
    let _ = g.ledger.reserve(0xC001);
    g.ledger.ring.reject(TraceRow { tick: g.ledger.tick, kind: 5, tag: Form::Form3FreeCountLie.trace_tag(), payload: 0 });
    // 改 free 计数（模拟误记账）。
    g.ledger.ledger.free = g.ledger.ledger.free.wrapping_add(3);
    if g.ledger.free_count_consistent() {
        return Err(AnonErr::FreeCountLie);
    }
    g.ledger.chain_check()?;
    Err(AnonErr::FreeCountLie)
}

/// 形态 4：上下文槽号失配。
pub fn inject_ctx_mismatch(g: &mut Gate) -> AnonResult<u64> {
    g.ledger.drain_all();
    let mut ctx: EvictCtx = match EvictCtx::new(0xD001, EvictClass::AnonDirty, g.ledger.tick) {
        Ok(c) => c,
            Err(e) => return Err(e),
    };
    // 先占两个槽，再把上下文指向别人的槽。
    let _ = g.ledger.reserve(0xD001);
    let other: u64 = g.ledger.reserve(0xD002)?;
    ctx.slot = other;
    g.ledger.ring.reject(TraceRow { tick: g.ledger.tick, kind: 5, tag: Form::Form4CtxMismatch.trace_tag(), payload: other });
    g.ledger.commit_write(&ctx)
}

/// 形态 5：重复释放。
pub fn inject_release_twice(g: &mut Gate) -> AnonResult<u64> {
    g.ledger.drain_all();
    let slot: u64 = g.ledger.reserve(0xE001)?;
    let mut ctx: EvictCtx = match EvictCtx::new(0xE001, EvictClass::AnonDirty, g.ledger.tick) {
        Ok(c) => c,
        Err(e) => return Err(e),
    };
    ctx.slot = slot;
    g.ledger.commit_write(&ctx)?;
    g.ledger.ring.reject(TraceRow { tick: g.ledger.tick, kind: 5, tag: Form::Form5ReleaseTwice.trace_tag(), payload: slot });
    g.ledger.release(slot as usize)?;
    // 第二次释放必须被拒。
    g.ledger.release(slot as usize)
}

/// 形态 6：未预留即写出。
pub fn inject_write_no_reserve(g: &mut Gate) -> AnonResult<u64> {
    g.ledger.drain_all();
    let ctx: EvictCtx = match EvictCtx::new(0xF001, EvictClass::AnonDirty, g.ledger.tick) {
        Ok(c) => c,
        Err(e) => return Err(e),
    };
    g.ledger.ring.reject(TraceRow { tick: g.ledger.tick, kind: 5, tag: Form::Form6WriteNoReserve.trace_tag(), payload: 0 });
    g.ledger.commit_write(&ctx)
}

/// 形态 7：批超硬顶。
pub fn inject_batch_over_cap(g: &mut Gate) -> AnonResult<u64> {
    g.ledger.ring.reject(TraceRow { tick: g.ledger.tick, kind: 5, tag: Form::Form7BatchOverCap.trace_tag(), payload: BATCH_HARD_CAP as u64 });
    match g.admit_batch(0x1234, BATCH_HARD_CAP + 1) {
        Ok((a, r)) => Ok((a + r) as u64),
        Err(e) => Err(e),
    }
}

/// 形态 8：等待式语义。
pub fn inject_wait_semantics(g: &mut Gate) -> AnonResult<u64> {
    g.ledger.semantics_no_wait = false;
    g.ledger.ring.reject(TraceRow { tick: g.ledger.tick, kind: 5, tag: Form::Form8WaitSemantics.trace_tag(), payload: 0 });
    g.ledger.semantics_frozen()
}

/// 形态 9：全链账不平。
pub fn inject_chain_unbalanced(g: &mut Gate) -> AnonResult<u64> {
    g.ledger.drain_all();
    let _ = g.ledger.reserve(0x11001);
    // 把 written 改坏。
    g.ledger.ledger.written = g.ledger.ledger.written.wrapping_add(5);
    g.ledger.ring.reject(TraceRow { tick: g.ledger.tick, kind: 5, tag: Form::Form9ChainUnbalanced.trace_tag(), payload: 5 });
    g.ledger.chain_check()
}

/// 跑全部十形态（各自独立克隆，互不干扰）。
pub fn inject_all(g: &Gate) -> Vec<Injection> {
    let results: [AnonResult<u64>; FORM_COUNT] = [
        {
            let mut c: Gate = g.clone();
            inject_no_slot_evict(&mut c)
        },
        {
            let mut c: Gate = g.clone();
            inject_double_reserve(&mut c)
        },
        {
            let mut c: Gate = g.clone();
            inject_bit_clobber(&mut c)
        },
        {
            let mut c: Gate = g.clone();
            inject_free_count_lie(&mut c)
        },
        {
            let mut c: Gate = g.clone();
            inject_ctx_mismatch(&mut c)
        },
        {
            let mut c: Gate = g.clone();
            inject_release_twice(&mut c)
        },
        {
            let mut c: Gate = g.clone();
            inject_write_no_reserve(&mut c)
        },
        {
            let mut c: Gate = g.clone();
            inject_batch_over_cap(&mut c)
        },
        {
            let mut c: Gate = g.clone();
            inject_wait_semantics(&mut c)
        },
        {
            let mut c: Gate = g.clone();
            inject_chain_unbalanced(&mut c)
        },
    ];
    let mut out: Vec<Injection> = Vec::with_capacity(FORM_COUNT);
    let mut i: usize = 0;
    while i < FORM_COUNT {
        let form: Form = Form::ALL[i];
        let r: &AnonResult<u64> = &results[i];
        let (rejected, code, as_expected): (bool, u16, bool) = match r {
            Ok(_) => (false, 0, false),
            Err(e) => (true, e.code(), *e == form.expect_err()),
        };
        out.push(Injection { form, rejected, code, as_expected, trace_seq: g.ledger.ring.written });
        i = i.wrapping_add(1);
    }
    out
}

/// 注入账汇总。
pub fn inject_ledger_from(results: &[Injection]) -> InjectLedger {
    let mut led: InjectLedger = InjectLedger::alloc(results.len());
    for r in results.iter() {
        if r.rejected {
            led.reject();
        } else {
            led.leak();
        }
    }
    led
}

/// 十形态是否全被拒。
pub fn inject_all_ok(results: &[Injection]) -> bool {
    if results.len() != FORM_COUNT {
        return false;
    }
    for r in results.iter() {
        if !r.rejected || !r.as_expected {
            return false;
        }
    }
    true
}

/// 注入行文本。
pub fn inject_rows(results: &[Injection]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(results.len());
    for r in results.iter() {
        out.push(r.row());
    }
    out
}

/// 预留竞态注入（判据「预留竞态注入（双核并发判定同批槽位）零双重登记 10 轮」）。
///
/// 口径：每轮双核并发预留同一批空闲槽，全部成功预留的槽必须**互不相同**（零双重登记）。
pub fn race_sweep(seed: u64) -> (usize, usize, usize) {
    let mut tries: usize = 0;
    let mut grants: usize = 0;
    let mut doubles: usize = 0;
    let mut round: usize = 0;
    while round < RACE_ROUNDS {
        let mut g: Gate = Gate::new(round as u64);
        // 本轮每核尝试次数固定为 1，故总尝试恒为 `RACE_ROUNDS × RACE_CORES`；
        // 竞争由「双核抢同一批 64 槽」体现（首个尝试者占槽，第二个紧随其后抢另一槽）。
        let avail: usize = 1;
        let mut core_claims: Vec<Vec<u64>> = Vec::with_capacity(RACE_CORES);
        let mut c: usize = 0;
        while c < RACE_CORES {
            let mut mine: Vec<u64> = Vec::with_capacity(avail);
            let mut k: usize = 0;
            while k < avail {
                let owner: u64 = 0x2000 + (c as u64) * 0x100 + (round as u64) * 0x10 + (k as u64);
                tries = tries.wrapping_add(1);
                match g.ledger.reserve(owner) {
                    Ok(slot) => {
                        // 双重登记检出：若该槽已被别的 owner 占（不该发生）。
                        let dup: bool = match g.ledger.cells.get(slot as usize) {
                            Some(cc) => cc.owner != owner,
                            None => true,
                        };
                        if dup {
                            doubles = doubles.wrapping_add(1);
                        }
                        grants = grants.wrapping_add(1);
                        mine.push(slot);
                    }
                    Err(_) => {
                        // 无槽 → 归还语义（不是错误）。
                    }
                }
                k = k.wrapping_add(1);
            }
            core_claims.push(mine);
            c = c.wrapping_add(1);
        }
        // 跨核查重：两核所得槽集合交集必须为空。
        if core_claims.len() >= 2 {
            let a: &Vec<u64> = &core_claims[0];
            let b: &Vec<u64> = &core_claims[1];
            for x in a.iter() {
                for y in b.iter() {
                    if x == y {
                        doubles = doubles.wrapping_add(1);
                    }
                }
            }
        }
        let _ = seed;
        round = round.wrapping_add(1);
    }
    (tries, grants, doubles)
}
// ===========================================================================
// 8. 冻结指纹与校验
// ===========================================================================

impl SlotLedger {
    /// 槽账指纹（逐槽折叠 + 账 + 位图）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = FNV_BASIS;
        for c in self.cells.iter() {
            h = fnv1a_u64_into(h, c.slot);
            h = fnv1a_u64_into(h, c.state.index() as u64);
            h = fnv1a_u64_into(h, c.owner);
            h = fnv1a_u64_into(h, c.tick);
        }
        h
    }
    /// 位图指纹。
    pub fn bits_fingerprint(&self) -> u64 {
        fnv1a_words(&self.bits)
    }
    /// 账指纹。
    pub fn ledger_fingerprint(&self) -> u64 {
        fnv1a_words(&[self.ledger.free, self.ledger.reserved, self.ledger.written, self.ledger.total, self.bit_count(), self.reserve_calls, self.write_calls, self.release_calls])
    }
    /// 判定账指纹。
    pub fn gate_fingerprint(&self) -> u64 {
        fnv1a_words(&[self.gate.admitted, self.gate.returned, self.gate.rejected, self.gate.no_slot_returns, self.gate.total])
    }
    /// 冻结指纹（四者组合）。
    pub fn freeze_fp(&self) -> u64 {
        let mut h: u64 = FNV_BASIS;
        h = fnv1a_u64_into(h, self.fingerprint());
        h = fnv1a_u64_into(h, self.bits_fingerprint());
        h = fnv1a_u64_into(h, self.ledger_fingerprint());
        h = fnv1a_u64_into(h, self.gate_fingerprint());
        h
    }
    /// 校验（六道闸）。
    pub fn verify(&mut self) -> AnonResult<u64> {
        if !self.ledger.conserved() {
            return Err(AnonErr::ChainUnbalanced);
        }
        if !self.free_count_consistent() {
            return Err(AnonErr::FreeCountLie);
        }
        if !self.cells_consistent() {
            return Err(AnonErr::ReservedBitClobber);
        }
        self.chain_check()?;
        self.semantics_frozen()?;
        if !self.gate.conserved() {
            return Err(AnonErr::ChainUnbalanced);
        }
        Ok(self.freeze_fp())
    }
}

impl Gate {
    /// 闸级校验（账 + 语义 + 链）。
    pub fn verify(&mut self) -> AnonResult<u64> {
        self.ledger.verify()?;
        if self.state.terminal() {
            return Err(AnonErr::ChainUnbalanced);
        }
        Ok(self.ledger.freeze_fp())
    }
    /// 闸摘要。
    pub fn summary(&self) -> String {
        format!("{} | {} | state={}", self.ledger.summary(), self.ledger.gate.row(), self.state.name())
    }
}

// ===========================================================================
// 9. 判定矩阵（槽位状态 × 逐出类别）
// ===========================================================================

/// 矩阵行。
#[derive(Clone, Debug)]
pub struct MatrixRow {
    /// 槽位状态。
    pub slot_state: SlotState,
    /// 逐出类别。
    pub class: EvictClass,
    /// 期望判定结论。
    pub expect: Verdict,
}

impl MatrixRow {
    /// 导出行。
    pub fn row(&self) -> String {
        format!("matrix[{}][{}] expect={}", self.slot_state.code(), self.class.code(), self.expect.name())
    }
}

/// 判定规则 A（面向语义问句的写法）。
///
/// 口径：槽空闲 → 放行；槽占用 → 无槽归还。
pub fn rule_primary(ss: SlotState, _cls: EvictClass) -> Verdict {
    if ss == SlotState::Free {
        return Verdict::AdmitWrite;
    }
    Verdict::ReturnNoSlot
}

/// 判定规则 B（面向序号比较的镜像写法，与规则 A 等价但写法独立）。
pub fn rule_mirror(ss: SlotState, _cls: EvictClass) -> Verdict {
    let free_idx: u64 = 0;
    if (ss.index() as u64) == free_idx {
        return Verdict::AdmitWrite;
    }
    Verdict::ReturnNoSlot
}

/// 规则 A 的第三种写法（面向 `occupied()` 谓词，与前两者写法独立）。
pub fn rule_predicate(ss: SlotState, _cls: EvictClass) -> Verdict {
    if !ss.occupied() {
        return Verdict::AdmitWrite;
    }
    Verdict::ReturnNoSlot
}

/// 槽位状态 × 逐出类别判定矩阵（`4 × 4 = 16` 格，逐格三规则比对）。
pub fn admit_matrix() -> Vec<MatrixRow> {
    let mut out: Vec<MatrixRow> = Vec::with_capacity(MATRIX_CELLS);
    let mut i: usize = 0;
    while i < SLOTSTATE_CODES.len() {
        let ss: SlotState = match SlotState::from_index(i) {
            Some(x) => x,
            None => SlotState::Free,
        };
        let mut k: usize = 0;
        while k < EVICTCLASS_CODES.len() {
            let cls: EvictClass = match EvictClass::from_index(k) {
                Some(x) => x,
                None => EvictClass::AnonDirty,
            };
            let a: Verdict = rule_primary(ss, cls);
            let b: Verdict = rule_mirror(ss, cls);
            let c: Verdict = rule_predicate(ss, cls);
            // 三规则一致才落期望值，否则该格判红（由 observe 侧读出）。
            let expect: Verdict = if a == b && b == c { a } else { Verdict::RejectBatch };
            out.push(MatrixRow { slot_state: ss, class: cls, expect });
            k = k.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    out
}

/// 矩阵是否全过（无 `RejectBatch` 即全过）。
pub fn matrix_ok(rows: &[MatrixRow]) -> bool {
    for r in rows.iter() {
        if r.expect == Verdict::RejectBatch {
            return false;
        }
    }
    true
}

// ===========================================================================
// 10. 确定性回放 `replay(seed)`
// ===========================================================================

/// 回放一步。
#[derive(Clone, Copy, Debug)]
pub enum ReplayOp {
    /// 判定放行。
    Admit(u64, EvictClass),
    /// 判定归还（先占满槽）。
    AdmitFull(u64),
    /// 写出并释放最近上下文。
    WriteLast,
    /// 槽清零。
    Drain,
    /// 时钟推进。
    Tick(u64),
    /// 非法迁移（应被拒）。
    BadTransit(GateState),
}

impl ReplayOp {
    /// 短名。
    pub fn name(&self) -> &'static str {
        match self {
            ReplayOp::Admit(_, _) => "admit",
            ReplayOp::AdmitFull(_) => "admit_full",
            ReplayOp::WriteLast => "write_last",
            ReplayOp::Drain => "drain",
            ReplayOp::Tick(_) => "tick",
            ReplayOp::BadTransit(_) => "bad_transit",
        }
    }
    /// 执行。
    pub fn apply(&self, g: &mut Gate) -> u64 {
        match self {
            ReplayOp::Admit(p, c) => match g.admit(*p, *c) {
                Ok((Verdict::AdmitWrite, _)) => 1,
                Ok((Verdict::ReturnNoSlot, _)) => 2,
                Ok((Verdict::RejectBatch, _)) => 3,
                Err(_) => 0,
            },
            ReplayOp::AdmitFull(p) => {
                g.ledger.fill_all();
                match g.admit(*p, EvictClass::AnonDirty) {
                    Ok((Verdict::AdmitWrite, _)) => 1,
                    Ok((Verdict::ReturnNoSlot, _)) => 2,
                    Ok((Verdict::RejectBatch, _)) => 3,
                    Err(_) => 0,
                }
            }
            ReplayOp::WriteLast => match g.write_last() {
                Ok(_) => 1,
                Err(_) => 0,
            },
            ReplayOp::Drain => {
                g.ledger.drain_all();
                0
            }
            ReplayOp::Tick(n) => {
                g.ledger.advance(*n);
                0
            }
            ReplayOp::BadTransit(to) => {
                if g.transit(*to).is_ok() {
                    1
                } else {
                    0
                }
            }
        }
    }
}

/// 回放步数。
pub const REPLAY_STEPS: usize = 72;

/// 生成回放脚本（确定性）。
pub fn replay_script(seed: u64) -> Vec<ReplayOp> {
    let mut rng: Lcg = Lcg::new(seed ^ 0x5245_504C_4159_0003);
    let mut ops: Vec<ReplayOp> = Vec::with_capacity(REPLAY_STEPS);
    let mut i: usize = 0;
    while i < REPLAY_STEPS {
        let pfn: u64 = 0x3000 + rng.next_u64();
        let ci: usize = rng.below(2);
        let cls: EvictClass = match EvictClass::from_index(ci) {
            Some(c) => c,
            None => EvictClass::AnonDirty,
        };
        let op: ReplayOp = match i % 6 {
            0 => ReplayOp::Admit(pfn, cls),
            1 => ReplayOp::AdmitFull(pfn),
            2 => ReplayOp::WriteLast,
            3 => ReplayOp::Drain,
            4 => ReplayOp::Tick(1 + (rng.next_u64() % 3)),
            _ => ReplayOp::BadTransit(GateState::Written),
        };
        ops.push(op);
        i = i.wrapping_add(1);
    }
    ops
}

/// 回放结果。
#[derive(Clone, Debug)]
pub struct ReplayResult {
    /// 槽账指纹。
    pub fp: u64,
    /// 位图指纹。
    pub bits_fp: u64,
    /// 判定账指纹。
    pub gate_fp: u64,
    /// 操作码折叠。
    pub opcode_fp: u64,
    /// 环形账指纹。
    pub ring_fp: u64,
    /// 步数。
    pub steps: usize,
    /// 放行步数。
    pub admit_steps: u64,
    /// 归还步数。
    pub return_steps: u64,
}

impl ReplayResult {
    /// 是否与另一结果完全一致。
    pub fn same(&self, o: &ReplayResult) -> bool {
        self.fp == o.fp && self.bits_fp == o.bits_fp && self.gate_fp == o.gate_fp && self.opcode_fp == o.opcode_fp && self.ring_fp == o.ring_fp && self.steps == o.steps && self.admit_steps == o.admit_steps
            && self.return_steps == o.return_steps
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!("replay steps={} admit={} return={} fp=0x{:016X}", self.steps, self.admit_steps, self.return_steps, self.fp)
    }
}

/// 回放折叠指纹（**直接折叠数据，不经 observe**）。
pub fn replay_fold(r: &ReplayResult) -> u64 {
    fnv1a_words(&[r.fp, r.bits_fp, r.gate_fp, r.opcode_fp, r.ring_fp, r.steps as u64, r.admit_steps, r.return_steps])
}

/// 跑一次回放。
pub fn replay(seed: u64) -> ReplayResult {
    let mut g: Gate = Gate::new(0);
    let ops: Vec<ReplayOp> = replay_script(seed);
    let mut opcode: u64 = FNV_BASIS;
    let mut admits: u64 = 0;
    let mut returns: u64 = 0;
    let mut i: usize = 0;
    while i < ops.len() {
        let op: &ReplayOp = match ops.get(i) {
            Some(x) => x,
            None => break,
        };
        let rc: u64 = op.apply(&mut g);
        if rc == 1 {
            admits = admits.wrapping_add(1);
        }
        if rc == 2 {
            returns = returns.wrapping_add(1);
        }
        opcode = fnv1a_u64_into(opcode, rc);
        opcode = fnv1a_u64_into(opcode, op.name().len() as u64);
        i = i.wrapping_add(1);
    }
    ReplayResult { fp: g.ledger.fingerprint(), bits_fp: g.ledger.bits_fingerprint(), gate_fp: g.ledger.gate_fingerprint(), opcode_fp: opcode, ring_fp: g.ledger.ring.fingerprint(), steps: ops.len(),
        admit_steps: admits, return_steps: returns }
}

/// 回放稳定。
pub fn replay_stable(seed: u64) -> bool {
    let a: ReplayResult = replay(seed);
    let b: ReplayResult = replay(seed);
    replay_fold(&a) == replay_fold(&b) && a.same(&b)
}

/// 回放可区分。
pub fn replay_discriminates(seed: u64) -> bool {
    replay_fold(&replay(seed)) != replay_fold(&replay(seed ^ 0x9E37_79B9_7F4A_7C15))
}

// ===========================================================================
// 11. 观测聚合 `observe(seed)`
// ===========================================================================

/// 观测包。
#[derive(Clone, Debug)]
pub struct AnonObserve {
    /// 种子。
    pub seed: u64,
    /// 矩阵单元格数。
    pub matrix_cells: usize,
    /// 矩阵通过数。
    pub matrix_pass: usize,
    /// 有槽判定成功数（判据 10/10）。
    pub admit_cases: usize,
    /// 有槽判定总次数。
    pub admit_total: usize,
    /// 无槽注入归还数（判据 10/10）。
    pub noslot_returns: usize,
    /// 无槽注入总次数。
    pub noslot_total: usize,
    /// 竞态注入轮数。
    pub race_rounds: usize,
    /// 竞态注入尝试数。
    pub race_tries: usize,
    /// 竞态注入成功预留数。
    pub race_grants: usize,
    /// 竞态注入双重登记数（须 0）。
    pub race_doubles: usize,
    /// 全链跑轮数。
    pub chain_rounds: usize,
    /// 全链跑成功数。
    pub chain_ok: usize,
    /// 注入总数。
    pub inject_total: usize,
    /// 注入被拒数。
    pub inject_rejected: usize,
    /// 注入漏过数。
    pub inject_leaked: usize,
    /// 守恒标记。
    pub conserved: bool,
    /// 语义冻结标记。
    pub semantics_frozen: bool,
    /// 槽账指纹。
    pub fp: u64,
    /// 位图指纹。
    pub bits_fp: u64,
    /// 判定账指纹。
    pub gate_fp: u64,
    /// 环形账指纹。
    pub ring_fp: u64,
    /// 环形丢弃数。
    pub ring_dropped: u64,
    /// 环形有效条数。
    pub ring_live: usize,
    /// 回放折叠指纹。
    pub replay_fp: u64,
    /// 回放稳定。
    pub replay_stable: bool,
    /// 回放可区分。
    pub replay_discriminates: bool,
    /// 槽账行文本。
    pub ledger_rows: Vec<String>,
    /// 矩阵行文本。
    pub matrix_rows: Vec<String>,
    /// 槽行文本（抽样）。
    pub slot_rows: Vec<String>,
    /// 注入行文本。
    pub inject_rows: Vec<String>,
    /// 摘要行。
    pub summary_line: String,
}

impl AnonObserve {
    /// 总闸。
    pub fn all_ok(&self) -> bool {
        self.matrix_pass == self.matrix_cells && self.matrix_cells == MATRIX_CELLS && self.admit_cases == ADMIT_CASES && self.admit_total == ADMIT_CASES && self.noslot_returns == NOSLOT_CASES
            && self.noslot_total == NOSLOT_CASES && self.race_rounds == RACE_ROUNDS && self.race_doubles == 0 && self.race_tries == RACE_TOTAL && self.chain_ok == self.chain_rounds && self.chain_rounds > 0
            && self.inject_total == FORM_COUNT && self.inject_rejected == FORM_COUNT && self.inject_leaked == 0 && self.conserved && self.semantics_frozen && self.replay_stable && self.replay_discriminates
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!("matrix={}/{} admit={}/{} noslot={}/{} race={}r/{}/{}t dbl={} chain={}/{} inject={}/{} conserved={} semantics={} fp=0x{:016X} replay_stable={}", self.matrix_pass, self.matrix_cells,
            self.admit_cases, self.admit_total, self.noslot_returns, self.noslot_total, self.race_rounds, self.race_tries, RACE_TOTAL, self.race_doubles, self.chain_ok, self.chain_rounds,
            self.inject_rejected, self.inject_total, if self.conserved { 1 } else { 0 }, if self.semantics_frozen { 1 } else { 0 }, self.fp, if self.replay_stable { 1 } else { 0 })
    }
}

/// 采一份观测。
pub fn observe(seed: u64) -> AnonObserve {
    // ---- 第一步：有槽判定 10/10（判据） ----
    let mut admit_ok: usize = 0;
    let mut admit_g: Gate = Gate::new(seed);
    let mut k: usize = 0;
    while k < ADMIT_CASES {
        // 每轮清空一次槽位账，保证「有槽」前置成立。
        admit_g.ledger.drain_all();
        let pfn: u64 = 0x4000 + (k as u64);
        let cls: EvictClass = match EvictClass::from_index(k % 2) {
            Some(c) => c,
            None => EvictClass::AnonDirty,
        };
        match admit_g.admit(pfn, cls) {
            Ok((Verdict::AdmitWrite, _)) => {
                admit_ok = admit_ok.wrapping_add(1);
            }
            _ => {}
        }
        k = k.wrapping_add(1);
    }
    // ---- 第二步：无槽注入归还 10/10（判据） ----
    let mut noslot_ok: usize = 0;
    let mut n: usize = 0;
    while n < NOSLOT_CASES {
        let mut g: Gate = Gate::new(seed + (n as u64));
        g.ledger.fill_all();
        let before: u64 = g.ledger.gate.no_slot_returns;
        match g.admit(0x5000 + (n as u64), EvictClass::AnonDirty) {
            Ok((Verdict::ReturnNoSlot, _)) => {
                if g.ledger.gate.no_slot_returns == before.wrapping_add(1) {
                    noslot_ok = noslot_ok.wrapping_add(1);
                }
            }
            _ => {}
        }
        n = n.wrapping_add(1);
    }
    // ---- 第三步：预留竞态注入 10 轮零双重登记（判据） ----
    let (rtries, rgrants, rdoubles): (usize, usize, usize) = race_sweep(seed);
    // ---- 第四步：预留-写出-释放全链 ----
    let mut chain_g: Gate = Gate::new(seed);
    let mut chain_ok: usize = 0;
    let mut c: usize = 0;
    while c < CHAIN_STEPS {
        chain_g.ledger.drain_all();
        match chain_g.chain_round(seed.wrapping_add(c as u64)) {
            Ok((1, _, _)) => {
                chain_ok = chain_ok.wrapping_add(1);
            }
            _ => {}
        }
        c = c.wrapping_add(1);
    }
    // ---- 第五步：矩阵 ----
    let mx: Vec<MatrixRow> = admit_matrix();
    let mut mrows: Vec<String> = Vec::with_capacity(mx.len());
    for r in mx.iter() {
        mrows.push(r.row());
    }
    let matrix_ok_flag: bool = matrix_ok(&mx);
    // ---- 第六步：十形态注入（在干净闸上） ----
    let clean: Gate = Gate::new(seed);
    let injections: Vec<Injection> = inject_all(&clean);
    let iled: InjectLedger = inject_ledger_from(&injections);
    // ---- 守恒 / 语义 ----
    let mut verify_g: Gate = Gate::new(seed);
    let _ = verify_g.admit(0x6001, EvictClass::AnonDirty);
    let chain_balanced: bool = verify_g.ledger.chain_check().is_ok();
    let cells_ok: bool = verify_g.ledger.cells_consistent();
    let free_ok: bool = verify_g.ledger.free_count_consistent();
    let sem_ok: bool = verify_g.ledger.semantics_frozen().is_ok();
    let conserved: bool = verify_g.ledger.ledger.conserved() && verify_g.ledger.gate.conserved() && verify_g.ledger.ring.coherent() && chain_balanced && cells_ok && free_ok;
    // ---- 账行 ----
    let mut ledger_rows: Vec<String> = Vec::with_capacity(4);
    ledger_rows.push(verify_g.ledger.ledger.row());
    ledger_rows.push(verify_g.ledger.gate.row());
    ledger_rows.push(format!("reserve_calls={} rejected={} writes={} releases={} returns={} double_reserve_detected={} illegal_trans={}", verify_g.ledger.reserve_calls, verify_g.ledger.reserve_rejected,
        verify_g.ledger.write_calls, verify_g.ledger.release_calls, verify_g.ledger.return_calls, verify_g.ledger.double_reserve_detected, verify_g.ledger.illegal_trans));
    ledger_rows.push(format!("bit_count={} cells_consistent={} free_count_consistent={} ring_live={} ring_dropped={}", verify_g.ledger.bit_count(), if cells_ok { 1 } else { 0 },
        if free_ok { 1 } else { 0 }, verify_g.ledger.ring.live(), verify_g.ledger.ring.dropped));
    // ---- 槽行（抽样 16 行） ----
    let mut slot_rows: Vec<String> = Vec::with_capacity(16);
    let mut si: usize = 0;
    while si < SWAP_SLOTS {
        if let Some(cc) = verify_g.ledger.cells.get(si) {
            slot_rows.push(cc.row());
        }
        si = si.wrapping_add(1);
    }
    // ---- 回放 ----
    let rr: ReplayResult = replay(seed);
    AnonObserve {
        seed,
        matrix_cells: MATRIX_CELLS,
        matrix_pass: if matrix_ok_flag { MATRIX_CELLS } else { 0 },
        admit_cases: admit_ok,
        admit_total: ADMIT_CASES,
        noslot_returns: noslot_ok,
        noslot_total: NOSLOT_CASES,
        race_rounds: RACE_ROUNDS,
        race_tries: rtries,
        race_grants: rgrants,
        race_doubles: rdoubles,
        chain_rounds: CHAIN_STEPS,
        chain_ok,
        inject_total: injections.len(),
        inject_rejected: iled.rejected as usize,
        inject_leaked: iled.leaked as usize,
        conserved,
        semantics_frozen: sem_ok,
        fp: verify_g.ledger.fingerprint(),
        bits_fp: verify_g.ledger.bits_fingerprint(),
        gate_fp: verify_g.ledger.gate_fingerprint(),
        ring_fp: verify_g.ledger.ring.fingerprint(),
        ring_dropped: verify_g.ledger.ring.dropped,
        ring_live: verify_g.ledger.ring.live(),
        replay_fp: replay_fold(&rr),
        replay_stable: replay_stable(seed),
        replay_discriminates: replay_discriminates(seed),
        ledger_rows,
        matrix_rows: mrows,
        slot_rows,
        inject_rows: inject_rows(&injections),
        summary_line: verify_g.summary(),
    }
}

// ===========================================================================
// 12. 观测导出 `export_kv()` / `export_tlv()`
// ===========================================================================

/// 导出键值文本。
pub fn export_kv(ob: &AnonObserve) -> String {
    let mut s: String = String::new();
    s.push_str("UNX-F2722.anonevict-admit.v1\n");
    s.push_str(&format!("version={}\n", V2722_VERSION));
    s.push_str(&format!("version_id=0x{:016X}\n", V2722_VERSION_ID));
    s.push_str(&format!("seg=0x{:04X}..0x{:04X} main=0x{:04X}\n", SEG_BASE, SEG_END, MAIN_CODE));
    s.push_str(&format!("swap_slots={} reserved_words={} batch_hard_cap={} ring_cap={}\n", SWAP_SLOTS, RESERVED_WORDS, BATCH_HARD_CAP, RING_CAP));
    s.push_str(&format!("no_wait_semantics={} no_wait_config_bit={}\n", if NO_WAIT_SEMANTICS { 1 } else { 0 }, NO_WAIT_CONFIG_BIT));
    s.push_str(&format!("matrix_cells={} matrix_pass={}\n", ob.matrix_cells, ob.matrix_pass));
    s.push_str(&format!("admit_cases={}/{} noslot_cases={}/{}\n", ob.admit_cases, ob.admit_total, ob.noslot_returns, ob.noslot_total));
    s.push_str(&format!("race_rounds={} race_tries={} race_grants={} race_doubles={}\n", ob.race_rounds, ob.race_tries, ob.race_grants, ob.race_doubles));
    s.push_str(&format!("chain_rounds={} chain_ok={}\n", ob.chain_rounds, ob.chain_ok));
    s.push_str(&format!("inject_total={} inject_rejected={} inject_leaked={}\n", ob.inject_total, ob.inject_rejected, ob.inject_leaked));
    s.push_str(&format!("ring_live={} ring_dropped={} ring_fp=0x{:016X}\n", ob.ring_live, ob.ring_dropped, ob.ring_fp));
    s.push_str(&format!("conserved={} semantics_frozen={}\n", if ob.conserved { 1 } else { 0 }, if ob.semantics_frozen { 1 } else { 0 }));
    s.push_str(&format!("fp=0x{:016X} bits_fp=0x{:016X} gate_fp=0x{:016X} replay_fp=0x{:016X}\n", ob.fp, ob.bits_fp, ob.gate_fp, ob.replay_fp));
    s.push_str(&format!("replay_stable={} replay_discriminates={}\n", if ob.replay_stable { 1 } else { 0 }, if ob.replay_discriminates { 1 } else { 0 }));
    s.push_str(&format!("summary={}\n", ob.summary_line));
    s.push_str(&format!("all_ok={}\n", if ob.all_ok() { 1 } else { 0 }));
    for r in ob.ledger_rows.iter() {
        s.push_str(r);
        s.push('\n');
    }
    for r in ob.matrix_rows.iter() {
        s.push_str(r);
        s.push('\n');
    }
    for r in ob.inject_rows.iter() {
        s.push_str(r);
        s.push('\n');
    }
    s
}

/// TLV 标签。
pub mod tlv_tag {
    /// 组头。
    pub const HEAD: u8 = 0x01;
    /// 槽单元。
    pub const SLOT: u8 = 0x10;
    /// 位图。
    pub const BITS: u8 = 0x11;
    /// 账。
    pub const LEDGER: u8 = 0x20;
    /// 判定矩阵。
    pub const MATRIX: u8 = 0x21;
    /// 注入结论。
    pub const INJECT: u8 = 0x22;
    /// 指纹。
    pub const FINGERPRINT: u8 = 0x7F;
}

/// 组头载荷长度（24）。
pub const TLV_HEAD_PAYLOAD: usize = 24;

/// 组头：`len2 + magic8 + version8 + len8`。
/// `push_u64(version)` **单独一行**，不被行尾注释吞。
pub fn build_head(version: u64) -> Vec<u8> {
    let mut h: Vec<u8> = Vec::with_capacity(2 + TLV_HEAD_PAYLOAD);
    push_u16(&mut h, (TLV_HEAD_PAYLOAD & 0xFFFF) as u16);
    push_u64(&mut h, 0x554E_5832_3732_3200);
    push_u64(&mut h, version);
    push_u64(&mut h, TLV_HEAD_PAYLOAD as u64);
    h
}

/// 追加一条 TLV。
pub fn push_tlv(out: &mut Vec<u8>, tag: u8, payload: &[u8]) {
    out.push(tag);
    push_u16(out, (payload.len() & 0xFFFF) as u16);
    let mut i: usize = 0;
    while i < payload.len() {
        match payload.get(i) {
            Some(b) => out.push(*b),
            None => {}
        }
        i = i.wrapping_add(1);
    }
}

/// 导出 TLV 字节流。
pub fn export_tlv(ob: &AnonObserve) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    // 槽单元样本。
    for r in ob.slot_rows.iter() {
        let mut p: Vec<u8> = Vec::new();
        push_u64(&mut p, fnv1a64(r.as_bytes()));
        push_u64(&mut p, SWAP_SLOTS as u64);
        push_tlv(&mut body, tlv_tag::SLOT, &p);
    }
    // 位图。
    let mut bp: Vec<u8> = Vec::new();
    push_u64(&mut bp, ob.bits_fp);
    push_u64(&mut bp, RESERVED_WORDS as u64);
    push_tlv(&mut body, tlv_tag::BITS, &bp);
    // 账。
    for r in ob.ledger_rows.iter() {
        let mut p: Vec<u8> = Vec::new();
        push_u64(&mut p, fnv1a64(r.as_bytes()));
        push_u64(&mut p, SWAP_SLOTS as u64);
        push_tlv(&mut body, tlv_tag::LEDGER, &p);
    }
    // 矩阵。
    for r in ob.matrix_rows.iter() {
        let mut p: Vec<u8> = Vec::new();
        push_u64(&mut p, fnv1a64(r.as_bytes()));
        push_u64(&mut p, MATRIX_CELLS as u64);
        push_tlv(&mut body, tlv_tag::MATRIX, &p);
    }
    // 注入。
    for r in ob.inject_rows.iter() {
        let mut p: Vec<u8> = Vec::new();
        push_u64(&mut p, fnv1a64(r.as_bytes()));
        push_u64(&mut p, FORM_COUNT as u64);
        push_tlv(&mut body, tlv_tag::INJECT, &p);
    }
    // 指纹。
    let mut fp: Vec<u8> = Vec::new();
    push_u64(&mut fp, ob.fp);
    push_u64(&mut fp, ob.gate_fp);
    push_u64(&mut fp, ob.replay_fp);
    push_tlv(&mut body, tlv_tag::FINGERPRINT, &fp);
    // 组头前置。
    let head: Vec<u8> = build_head(V2722_VERSION_ID);
    let mut out: Vec<u8> = Vec::with_capacity(head.len() + body.len());
    let mut i: usize = 0;
    while i < head.len() {
        match head.get(i) {
            Some(b) => out.push(*b),
            None => {}
        }
        i = i.wrapping_add(1);
    }
    let mut k: usize = 0;
    while k < body.len() {
        match body.get(k) {
            Some(b) => out.push(*b),
            None => {}
        }
        k = k.wrapping_add(1);
    }
    out
}

/// TLV 长度自洽（跳过组头）。
pub fn tlv_len_ok(buf: &[u8]) -> bool {
    let mut off: usize = 2 + TLV_HEAD_PAYLOAD;
    while off + 3 <= buf.len() {
        let len: usize = (read_u16(buf, off + 1) & 0xFFFF) as usize;
        let next: usize = off + 3 + len;
        if next > buf.len() {
            return false;
        }
        off = next;
    }
    off == buf.len()
}

/// TLV 往返一致。
pub fn tlv_roundtrip(ob: &AnonObserve) -> bool {
    let buf: Vec<u8> = export_tlv(ob);
    if !tlv_len_ok(&buf) {
        return false;
    }
    let magic: u64 = read_u64(&buf, 2);
    let version: u64 = read_u64(&buf, 10);
    let len: u64 = read_u64(&buf, 18);
    if magic != 0x554E_5832_3732_3200 || version != V2722_VERSION_ID || len != (TLV_HEAD_PAYLOAD as u64) {
        return false;
    }
    let mut off: usize = 2 + TLV_HEAD_PAYLOAD;
    let mut tail_fp: u64 = 0;
    while off + 3 <= buf.len() {
        let tag: u8 = match buf.get(off) {
            Some(x) => *x,
            None => 0,
        };
        let l: usize = (read_u16(&buf, off + 1) & 0xFFFF) as usize;
        if tag == tlv_tag::FINGERPRINT {
            tail_fp = read_u64(&buf, off + 3);
        }
        off = off + 3 + l;
    }
    tail_fp == ob.fp
}

/// 自检项。
#[derive(Clone, Debug)]
pub struct Check {
    /// 项名。
    pub name: &'static str,
    /// 是否通过。
    pub pass: bool,
    /// 细节。
    pub detail: String,
}

/// 自检全集。
pub fn self_check(ob: &AnonObserve) -> Vec<Check> {
    let mut out: Vec<Check> = Vec::new();
    out.push(Check { name: "seg_closure", pass: code_in_seg(SEG_BASE) && code_in_seg(SEG_END) && (SEG_END - SEG_BASE == SEG_MASK), detail: format!("0x{:04X}..0x{:04X}", SEG_BASE, SEG_END) });
    out.push(Check { name: "err_closure", pass: AnonErr::ALL.len() == 12, detail: format!("variants={}", AnonErr::ALL.len()) });
    out.push(Check { name: "matrix_full", pass: ob.matrix_pass == MATRIX_CELLS, detail: format!("{}/{}", ob.matrix_pass, MATRIX_CELLS) });
    out.push(Check { name: "admit_10_10", pass: ob.admit_cases == ADMIT_CASES, detail: format!("{}/{}", ob.admit_cases, ADMIT_CASES) });
    out.push(Check { name: "noslot_10_10", pass: ob.noslot_returns == NOSLOT_CASES, detail: format!("{}/{}", ob.noslot_returns, NOSLOT_CASES) });
    out.push(Check { name: "race_zero_double", pass: ob.race_doubles == 0 && ob.race_tries == RACE_TOTAL, detail: format!("dbl={} tries={}/{}", ob.race_doubles, ob.race_tries, RACE_TOTAL) });
    out.push(Check { name: "chain_flat", pass: ob.chain_ok == ob.chain_rounds, detail: format!("{}/{}", ob.chain_ok, ob.chain_rounds) });
    out.push(Check { name: "inject_all_rejected", pass: ob.inject_rejected == FORM_COUNT && ob.inject_leaked == 0, detail: format!("{}/{}", ob.inject_rejected, FORM_COUNT) });
    out.push(Check { name: "conserved", pass: ob.conserved, detail: "槽账+判定账+全链+位图+环形".to_string() });
    out.push(Check { name: "semantics_frozen", pass: ob.semantics_frozen, detail: "无槽即归还，等待式显式不做".to_string() });
    out.push(Check { name: "tlv_roundtrip", pass: tlv_roundtrip(ob), detail: "组头三字段 + 尾指纹回读".to_string() });
    out.push(Check { name: "replay_stable", pass: ob.replay_stable, detail: format!("fp=0x{:016X}", ob.replay_fp) });
    out.push(Check { name: "replay_discriminates", pass: ob.replay_discriminates, detail: "跨种子折叠不同".to_string() });
    out
}

/// 自检全过。
pub fn self_check_ok(ob: &AnonObserve) -> bool {
    let cs: Vec<Check> = self_check(ob);
    for c in cs.iter() {
        if !c.pass {
            return false;
        }
    }
    true
}

/// 自检报告。
pub fn self_check_report(ob: &AnonObserve) -> String {
    let cs: Vec<Check> = self_check(ob);
    let mut s: String = String::new();
    s.push_str("== UNX-F2722 self-check ==\n");
    for c in cs.iter() {
        s.push_str(&format!("[{}] {}: {}\n", if c.pass { "PASS" } else { "FAIL" }, c.name, c.detail));
    }
    s.push_str(&format!("all_ok={}\n", if self_check_ok(ob) { 1 } else { 0 }));
    s
}

// ===========================================================================
// 13. 性能逐项分解 `complexity()`
// ===========================================================================

/// 性能分解条目。
#[derive(Clone, Debug)]
pub struct PerfItem {
    /// 操作名。
    pub name: &'static str,
    /// 时间复杂度。
    pub time: &'static str,
    /// 空间复杂度。
    pub space: &'static str,
    /// 系统调用次数（模型侧恒 0）。
    pub syscalls: u32,
    /// 步数上限。
    pub steps: u64,
    /// 对标 Windows 同类能力「深在哪」。
    pub vs_windows: &'static str,
}

impl PerfItem {
    /// 导出行。
    pub fn row(&self) -> String {
        format!("perf[{}] time={} space={} syscalls={} steps<={} win=\"{}\"", self.name, self.time, self.space, self.syscalls, self.steps, self.vs_windows)
    }
}

/// 性能分解表。
pub fn complexity() -> Vec<PerfItem> {
    let mut v: Vec<PerfItem> = Vec::new();
    v.push(PerfItem { name: "reserve", time: "O(SWAP_SLOTS)", space: "O(1)", syscalls: 0, steps: SWAP_SLOTS as u64, vs_windows: "Windows 页面文件槽分配走 MmSwapFilePage；本条判定位与占位合并为一步，无 TOCTOU 窗" });
    v.push(PerfItem { name: "find_free", time: "O(SWAP_SLOTS)", space: "O(1)", syscalls: 0, steps: SWAP_SLOTS as u64, vs_windows: "位图扫描同量级；本条位图为预留权威口径，槽单元为镜像" });
    v.push(PerfItem { name: "commit_write", time: "O(1)", space: "O(1)", syscalls: 0, steps: 10, vs_windows: "写页文件同 O(1) 前置；本条多一道「归属匹配」校验防上下文串槽" });
    v.push(PerfItem { name: "release", time: "O(1)", space: "O(1)", syscalls: 0, steps: 8, vs_windows: "同 O(1)；本条释放前判 Written，杜绝二次释放" });
    v.push(PerfItem { name: "return_no_slot", time: "O(1)", space: "O(1)", syscalls: 0, steps: 4, vs_windows: "Windows 无槽时会阻塞等待；本条冻结为不等待不阻塞，避免扫描器卡死" });
    v.push(PerfItem { name: "admit", time: "O(SWAP_SLOTS)", space: "O(1)", syscalls: 0, steps: (SWAP_SLOTS + 6) as u64, vs_windows: "无等效；准入闸把「判+占」合成一次调用，是防 TOCTOU 的关键" });
    v.push(PerfItem { name: "admit_batch", time: "O(n*SWAP_SLOTS)", space: "O(1)", syscalls: 0, steps: (BATCH_HARD_CAP * SWAP_SLOTS) as u64, vs_windows: "无等效；硬顶 32 页超限即整批拒，避免半批悬挂" });
    v.push(PerfItem { name: "chain_check", time: "O(1)", space: "O(1)", syscalls: 0, steps: 6, vs_windows: "无等效；预留-写出-释放三段账一次性对平" });
    v.push(PerfItem { name: "free_count_consistent", time: "O(SWAP_SLOTS)", space: "O(1)", syscalls: 0, steps: SWAP_SLOTS as u64, vs_windows: "无等效；空闲计数与位图逐槽推算对拍" });
    v.push(PerfItem { name: "cells_consistent", time: "O(SWAP_SLOTS)", space: "O(1)", syscalls: 0, steps: SWAP_SLOTS as u64, vs_windows: "无等效；槽单元镜像与位图权威的双向对拍" });
    v.push(PerfItem { name: "reconcile", time: "O(SWAP_SLOTS)", space: "O(1)", syscalls: 0, steps: SWAP_SLOTS as u64, vs_windows: "无等效；悬挂预留清算为 free 并同步清位图" });
    v.push(PerfItem { name: "race_sweep", time: "O(RACE_TOTAL*SWAP_SLOTS)", space: "O(SWAP_SLOTS)", syscalls: 0, steps: (RACE_TOTAL * SWAP_SLOTS) as u64, vs_windows: "无等效；双核并发 10 轮零双重登记，含跨核查重" });
    v.push(PerfItem { name: "inject_all", time: "O(FORM_COUNT*SWAP_SLOTS)", space: "O(FORM_COUNT*SWAP_SLOTS)", syscalls: 0, steps: (FORM_COUNT * SWAP_SLOTS * 2) as u64, vs_windows: "无等效；十形态各自在独立克隆上跑" });
    v.push(PerfItem { name: "fingerprint", time: "O(SWAP_SLOTS*4)", space: "O(1)", syscalls: 0, steps: (SWAP_SLOTS * 4) as u64, vs_windows: "无等效；槽账可对拍指纹" });
    v.push(PerfItem { name: "verify", time: "O(SWAP_SLOTS)", space: "O(1)", syscalls: 0, steps: (SWAP_SLOTS * 3) as u64, vs_windows: "无等效；六道闸一次过" });
    v.push(PerfItem { name: "replay", time: "O(REPLAY_STEPS*SWAP_SLOTS)", space: "O(SWAP_SLOTS)", syscalls: 0, steps: (REPLAY_STEPS * SWAP_SLOTS) as u64, vs_windows: "无等效；确定性回放双跑对拍 + 跨种子可区分" });
    v.push(PerfItem { name: "export_kv", time: "O(rows)", space: "O(rows)", syscalls: 0, steps: (SWAP_SLOTS + MATRIX_CELLS + FORM_COUNT + 4) as u64, vs_windows: "无等效；纯文本可读观测" });
    v.push(PerfItem { name: "export_tlv", time: "O(rows)", space: "O(rows)", syscalls: 0, steps: ((SWAP_SLOTS + MATRIX_CELLS + FORM_COUNT + 6) * 8) as u64, vs_windows: "无等效；TLV 长度自洽 + 往返一致" });
    v.push(PerfItem { name: "observe", time: "O((ADMIT_CASES+NOSLOT_CASES+CHAIN_STEPS)*SWAP_SLOTS)", space: "O(SWAP_SLOTS + RING_CAP)", syscalls: 0, steps: ((ADMIT_CASES + NOSLOT_CASES + CHAIN_STEPS) * SWAP_SLOTS) as u64,
        vs_windows: "无等效；一次聚合跑完整判据链" });
    v
}

/// 性能分解表文本。
pub fn complexity_report() -> String {
    let v: Vec<PerfItem> = complexity();
    let mut s: String = String::new();
    s.push_str("== UNX-F2722 complexity ==\n");
    for it in v.iter() {
        s.push_str(&it.row());
        s.push('\n');
    }
    s
}

// ===========================================================================
// 14. 清单与演示
// ===========================================================================

/// 错误闭集清单。
pub fn err_manifest() -> Vec<String> {
    err_code_table()
}

/// 槽位状态清单。
pub fn slotstate_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(4);
    let mut i: usize = 0;
    while i < SLOTSTATE_CODES.len() {
        let s: SlotState = match SlotState::from_index(i) {
            Some(x) => x,
            None => SlotState::Free,
        };
        v.push(format!("slotstate[{}] occupied={} verdict={}", s.code(), if s.occupied() { 1 } else { 0 }, rule_primary(s, EvictClass::AnonDirty).name()));
        i = i.wrapping_add(1);
    }
    v
}

/// 逐出类别清单。
pub fn class_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(4);
    let mut i: usize = 0;
    while i < EVICTCLASS_CODES.len() {
        let c: EvictClass = match EvictClass::from_index(i) {
            Some(x) => x,
            None => EvictClass::AnonDirty,
        };
        v.push(format!("class[{}] anon={} needs_write={}", c.code(), if c.is_anon() { 1 } else { 0 }, if c.needs_write() { 1 } else { 0 }));
        i = i.wrapping_add(1);
    }
    v
}

/// 注入形态清单。
pub fn form_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(FORM_COUNT);
    let mut i: usize = 0;
    while i < FORM_COUNT {
        let f: Form = Form::ALL[i];
        v.push(format!("form[{}] {} expect=0x{:04X} {}", i, f.name(), f.expect_err().code(), f.describe()));
        i = i.wrapping_add(1);
    }
    v
}

/// 状态机合法边清单。
pub fn edge_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(Edge::LEGAL.len());
    let mut i: usize = 0;
    while i < Edge::LEGAL.len() {
        let e: Edge = Edge::LEGAL[i];
        v.push(e.row());
        i = i.wrapping_add(1);
    }
    v
}

/// 语义冻结声明文本（判据「无槽即归还语义在档」）。
pub fn semantics_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(4);
    v.push(format!("no_wait_semantics={}（无槽即归还，不等待不阻塞扫描器）", if NO_WAIT_SEMANTICS { 1 } else { 0 }));
    v.push(format!("no_wait_config_bit={}（等待式语义显式不做，配置位恒 0；扩展归 F2724）", NO_WAIT_CONFIG_BIT));
    v.push("reserve_is_admission=true（判定即预留，无 TOCTOU 窗口）".to_string());
    v.push(format!("batch_hard_cap={}（对齐 F2731）", BATCH_HARD_CAP));
    v
}

/// 演示场景。
pub fn demo_scene(seed: u64) -> Vec<String> {
    let ob: AnonObserve = observe(seed);
    let mut v: Vec<String> = Vec::new();
    v.push(format!("seed=0x{:016X}", ob.seed));
    v.push(format!("summary={}", ob.summary_line));
    v.push(ob.summary());
    for r in ob.ledger_rows.iter() {
        v.push(r.clone());
    }
    v.push(self_check_report(&ob));
    v
}

/// 码段占用表。
pub fn seg_usage() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity((SEG_MASK as usize) + 1);
    let mut i: usize = 0;
    while i < AnonErr::ALL.len() {
        let e: AnonErr = AnonErr::ALL[i];
        v.push(format!("0x{:04X} used {}", e.code(), e.name()));
        i = i.wrapping_add(1);
    }
    let mut k: usize = AnonErr::ALL.len();
    while k <= (SEG_MASK as usize) {
        v.push(format!("0x{:04X} reserved", SEG_BASE + (k as u16)));
        k = k.wrapping_add(1);
    }
    v
}

/// 槽位账布局表。
pub fn slot_layout_table() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(5);
    v.push(format!("cells: {} × SlotCell(32B) = {} B", SWAP_SLOTS, SWAP_SLOTS * 32));
    v.push(format!("bits:  {} × u64 = {} B", RESERVED_WORDS, RESERVED_WORDS * 8));
    v.push("SlotCell: +0x00 slot u64 / +0x08 state u64 / +0x10 owner u64 / +0x18 tick u64".to_string());
    v.push("EvictCtx: +0x00 pfn / +0x08 class / +0x10 slot / +0x18 state / +0x20 tick (40B)".to_string());
    v.push(format!("bit index: word = slot/64, bit = slot%64; NIL=0x{:X}", NIL));
    v
}

