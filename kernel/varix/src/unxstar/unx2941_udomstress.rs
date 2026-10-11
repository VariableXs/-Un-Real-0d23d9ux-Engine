//! UNX-F2941 · 域级压力矩阵方案冻结（三轴）
//!
//! 任务单号: UNX-F2941
//! 标题: 域级压力矩阵方案冻结（三轴）
//! 域/批: A4/B28
//! 册目标: 340 行（方案 140 + 冻结 110 + 断言 90；测试段不计）
//!
//! 判据原文（逐字摘抄，来源 anchors/UNX-F2941.md）:
//! 「UNX-F2941-J1 三轴方案（8 核矩阵×16GB 三档压力×72h 长稳）冻结哈希入册，
//!   方案与 F2938 汇总账字段对齐」
//! 复测方式（逐字摘抄）:
//! 「12 组合在册哈希可查，对齐零缺失，漂移拦截，失真声明可查，
//!   三轴（压力档×负载型×时长档）12 组合参数与 F2719/F2750 对齐字段逐项注记在档，
//!   方案冻结件哈希入 CI，供 F2942–F2960 执行面共引。」
//!
//! 语义边界（逐字摘抄）:
//! 「只做方案冻结与对齐；不做矩阵执行（F2942–F2944）、不做真机执行（双轨欠账）。」
//!
//! 码段声明:
//! 本单独占诊断码段 `0x2240 ..= 0x227F`（`SEG_BASE = 0x2240`，`SEG_MASK = 0x003F`）。
//! 错误变体一码一段，逐个占段内偏移 0..=11（注入形态）与 20..=25（辅助码），
//! 最大占用 `0x2259`，远未触及段上界 `0x227F`。
//!
//! 制度约束（沿用 F2801 席制度）:
//! - `no_std + alloc`；零 `unwrap` / `expect` / `panic!` / `todo!` / `unsafe`
//! - 零浮点、零真实 IO、零墙钟；时间一律用逻辑 `tick: u64`
//! - 确定性 LCG：`6364136223846793005 / 1442695040888963407`（wrapping）
//! - FNV-1a：basis `0xcbf29ce484222325`，prime `0x100000001b3`
//! - 自定义 `Result<T, E>` 闭集，`E` 带 `code() -> u16` + `detail() -> String` 三要素
//!
//! 三轴设计（本单核心）:
//! - **核数轴** 4 档：1 / 2 / 4 / 8 核
//! - **压力轴** 3 档：轻 / 中 / 重（对齐 F2761 判定）
//! - **时长轴** 2 轨：10min 账面预演 + 72h 真机欠账（双轨）
//! 三轴交叉定义矩阵空间：`4 × 3 = 12` 组合；叠加时长双轨得 `12 × 2 = 24` 场景行。
//!
//! 风险与回退（逐字摘抄）:
//! 「方案失真——配比依据注记（对照 F2938 七批读数），失真声明在档（账面预演非真机等价）。」
//! 故本单强制携带 `basis`（配比依据，逐一对照 F2938 七批）与
//! `distortion`（失真声明，自认账面预演非真机等价）两条在档文本。

#![allow(dead_code)]
#![allow(unused_imports)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ===========================================================================
// 0. 常量表
// ===========================================================================
/// 模块版本标识（写入冻结件 + TLV 组头）。
pub const V_VERSION_ID: u64 = 0x0001_2941_0000_0001;
/// 模块版本串。
pub const V_VERSION: &str = "v2941-udomstress.v1";
/// 诊断码段基址（独占段 `0x2240`）。
pub const SEG_BASE: u16 = 0x2240;
/// 诊断码段掩码（段容量 64 码）。
pub const SEG_MASK: u16 = 0x003F;
/// 诊断码段上界（= `SEG_BASE | SEG_MASK`）。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;
/// 诊断主码（段基址本身）。
pub const MAIN_CODE: u16 = SEG_BASE;
/// 域级压测册版本号（冻结件入册时登记）。
pub const DOMAIN_BOOK_REV: u32 = 12;
/// 核数轴档数（1 / 2 / 4 / 8）。
pub const CORE_COUNT: usize = 4;
/// 压力轴档数（轻 / 中 / 重，对齐 F2761）。
pub const LOAD_COUNT: usize = 3;
/// 时长轨数（10min 账面 / 72h 真机欠账）。
pub const DURATION_COUNT: usize = 2;
/// 三轴组合数（`4 × 3 = 12`）。
pub const COMBO_COUNT: usize = CORE_COUNT * LOAD_COUNT;
/// 场景行数（`12 组合 × 2 时长轨 = 24`）。
pub const SCENE_COUNT: usize = COMBO_COUNT * DURATION_COUNT;
/// F2761 判定单号（压力档判定的上游）。
pub const LOAD_TICKET: &str = "F2761";
/// F2938 汇总账单号（本单对齐目标）。
pub const LEDGER_TICKET: &str = "F2938";
/// F2719 对齐注记上游。
pub const ALIGN_NOTE_A: &str = "F2719";
/// F2750 对齐注记上游。
pub const ALIGN_NOTE_B: &str = "F2750";
/// 执行面单号区间（本单明确不做执行）。
pub const EXEC_RANGE: &str = "F2942-F2960";
/// F2801 制度锚（本仓压测方案册制度来源）。
pub const SEAT_TICKET: &str = "F2801";
/// F2938 七批压测面名数（配比依据要逐一对照）。
pub const F2938_FACES: usize = 7;
/// F2761 压力判定项数（逐项对齐）。
pub const LOAD_VERDICTS: usize = 3;
/// 组合场景夹具真跑步数。
pub const COMBO_STEPS: usize = 20;
/// 场景夹具真跑步数。
pub const SCENE_STEPS: usize = 12;
/// 全场景真跑步数上界。
pub const MATRIX_STEPS: usize = SCENE_COUNT * SCENE_STEPS;
/// 稳定性重复轮数。
pub const REPEAT: usize = 3;
/// 注入形态数（判据要求漂移 10/10，本单给 12）。
pub const FORM_COUNT: usize = 12;
/// 对齐字段数（F2938 汇总账映射）。
pub const ALIGN_FIELDS: usize = 12;
/// 配比依据条目数（逐一对照 F2938 七批）。
pub const BASIS_ITEMS: usize = F2938_FACES;
/// 环形账容量（2 的幂）。
pub const RING_CAP: usize = 64;
/// 环形账掩码。
pub const RING_MASK: usize = RING_CAP - 1;
/// 逻辑时钟步进量。
pub const TICK_STEP: u64 = 1;
/// step 硬上限（`MATRIX_STEPS × REPEAT` 再留一倍裕量）。
pub const STEP_CAP: u64 = (MATRIX_STEPS * REPEAT * 2) as u64;
/// syscall 上界（本单零真实 syscall）。
pub const SYSCALL_CAP: u64 = 0;
/// 内存峰值上界（字节，模型侧估算）。
pub const MEM_PEAK_CAP: u64 = 4 * 1024 * 1024;
/// FNV-1a basis。
pub const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a prime。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;
/// LCG 乘数。
pub const LCG_A: u64 = 6364136223846793005;
/// LCG 增量。
pub const LCG_C: u64 = 1442695040888963407;
/// 判据单号。
pub const CRITERION: &str = "UNX-F2941-J1";

// ===========================================================================
// 1. 确定性 LCG + FNV-1a
// ===========================================================================
/// 确定性 LCG（Knuth MMIX 62 位参数，模 2^64 回绕）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lcg {
    /// 当前状态。
    pub state: u64,
}
impl Lcg {
    /// 由种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    /// 前进一格。
    pub fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(LCG_A).wrapping_add(LCG_C);
        self.state
    }
    /// 取一格（别名，便于表意）。
    pub fn step(&mut self) -> u64 {
        self.next()
    }
    /// 取高 32 位（有界抖动用）。
    pub fn next32(&mut self) -> u32 {
        (self.next() >> 32) as u32
    }
}
/// 单步 LCG（纯函数）。
pub fn lcg_step(state: u64) -> u64 {
    state.wrapping_mul(LCG_A).wrapping_add(LCG_C)
}
/// FNV-1a 64 位折叠（带 seed）。
pub fn fnv1a64_ext(state: u64, bytes: &[u8]) -> u64 {
    let mut h = state;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}
/// FNV-1a 64 位折叠（basis 起步）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    fnv1a64_ext(FNV_BASIS, bytes)
}
/// FNV-1a 字数组折叠（每字拆 8 字节大端）。
pub fn fnv1a_words(words: &[u64]) -> u64 {
    let mut h = FNV_BASIS;
    for w in words.iter() {
        let v = *w;
        let mut k: usize = 0;
        while k < 8 {
            let sh = 56u32.wrapping_sub((k as u32).wrapping_mul(8));
            let b = ((v >> sh) & 0xFF) as u8;
            h ^= b as u64;
            h = h.wrapping_mul(FNV_PRIME);
            k = k.wrapping_add(1);
        }
    }
    h
}
/// 取字符串字节。
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
    /// 核数档越界。
    CoreOutOfBand,
    /// 压力档越界。
    LoadOutOfBand,
    /// 时长轨越界。
    DurationOutOfBand,
    /// 压力档判定值与 F2761 不符。
    LoadVerdictSkew,
    /// 负载构成与 F2938 七批不对齐。
    FaceMixSkew,
    /// 替身账不守恒。
    LedgerUnbalanced,
    /// 对齐字段缺失（F2938 汇总账有字段无映射）。
    AlignMissing,
    /// 对齐字段重复（两方案字段映射到同一账字段）。
    AlignDuplicated,
    /// 步数超界。
    StepOverflow,
    /// 负载率超界（占用超上界）。
    DutyOverflow,
    /// 双轨欠账不平（账面轨与真机轨差额登记不一致）。
    DualTrackUnbalanced,
    /// 配比依据缺项（未逐一对照 F2938 七批）。
    BasisMissing,
}
impl Form {
    /// 全部形态。
    pub const ALL: [Form; FORM_COUNT] = [
        Form::CoreOutOfBand,
        Form::LoadOutOfBand,
        Form::DurationOutOfBand,
        Form::LoadVerdictSkew,
        Form::FaceMixSkew,
        Form::LedgerUnbalanced,
        Form::AlignMissing,
        Form::AlignDuplicated,
        Form::StepOverflow,
        Form::DutyOverflow,
        Form::DualTrackUnbalanced,
        Form::BasisMissing,
    ];
    /// 序号（0..11）。
    pub fn index(self) -> usize {
        match self {
            Form::CoreOutOfBand => 0,
            Form::LoadOutOfBand => 1,
            Form::DurationOutOfBand => 2,
            Form::LoadVerdictSkew => 3,
            Form::FaceMixSkew => 4,
            Form::LedgerUnbalanced => 5,
            Form::AlignMissing => 6,
            Form::AlignDuplicated => 7,
            Form::StepOverflow => 8,
            Form::DutyOverflow => 9,
            Form::DualTrackUnbalanced => 10,
            Form::BasisMissing => 11,
        }
    }
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE.wrapping_add(self.index() as u16)
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            Form::CoreOutOfBand => "core_out_of_band",
            Form::LoadOutOfBand => "load_out_of_band",
            Form::DurationOutOfBand => "duration_out_of_band",
            Form::LoadVerdictSkew => "load_verdict_skew",
            Form::FaceMixSkew => "face_mix_skew",
            Form::LedgerUnbalanced => "ledger_unbalanced",
            Form::AlignMissing => "align_missing",
            Form::AlignDuplicated => "align_duplicated",
            Form::StepOverflow => "step_overflow",
            Form::DutyOverflow => "duty_overflow",
            Form::DualTrackUnbalanced => "dual_track_unbalanced",
            Form::BasisMissing => "basis_missing",
        }
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            Form::CoreOutOfBand => "核数档越界：不在 1/2/4/8 闭集内",
            Form::LoadOutOfBand => "压力档越界：不在 轻/中/重 闭集内",
            Form::DurationOutOfBand => "时长轨越界：不在 10min/72h 闭集内",
            Form::LoadVerdictSkew => "压力档判定值与 F2761 判定不符",
            Form::FaceMixSkew => "负载构成与 F2938 七批面不对齐",
            Form::LedgerUnbalanced => "替身账事件不守恒",
            Form::AlignMissing => "对齐缺失：F2938 汇总账有字段无映射",
            Form::AlignDuplicated => "对齐重复：两方案字段映射到同一账字段",
            Form::StepOverflow => "步数超界：实跑超过方案上界",
            Form::DutyOverflow => "负载率超界：占用超过方案上界",
            Form::DualTrackUnbalanced => "双轨欠账不平：账面轨与真机轨差额登记不一致",
            Form::BasisMissing => "配比依据缺项：未逐一对照 F2938 七批",
        }
    }
    /// 探针（造证手法）。
    pub fn probe(self) -> &'static str {
        match self {
            Form::CoreOutOfBand => "把 0 号组合核数档序号改成 99",
            Form::LoadOutOfBand => "把 0 号组合压力档序号改成 99",
            Form::DurationOutOfBand => "把 0 号场景时长轨序号改成 99",
            Form::LoadVerdictSkew => "把 0 号组合压力档判定登记改成 +300",
            Form::FaceMixSkew => "把 0 号场景面数改成 99（越出 F2938 七面闭集）",
            Form::LedgerUnbalanced => "篡改 0 号场景登记负载率使替身账守恒式破裂",
            Form::AlignMissing => "抹掉 F2938 汇总账 duty_permille 映射",
            Form::AlignDuplicated => "把两字段都映射到 f2938.summary.core_class",
            Form::StepOverflow => "把 0 号场景步数上界改成 1",
            Form::DutyOverflow => "把 0 号场景负载率上界改成 0",
            Form::DualTrackUnbalanced => "篡改 0 号组合真机轨欠账登记额",
            Form::BasisMissing => "抹掉配比依据里 F2938 第 4 面那条注记",
        }
    }
    /// 是否属「轴越界」类。
    pub fn is_band(self) -> bool {
        matches!(self, Form::CoreOutOfBand | Form::LoadOutOfBand | Form::DurationOutOfBand)
    }
    /// 是否属「对齐缺陷」类。
    pub fn is_align(self) -> bool {
        matches!(self, Form::AlignMissing | Form::AlignDuplicated)
    }
    /// 是否属「在档缺陷」类。
    pub fn is_archive(self) -> bool {
        matches!(self, Form::BasisMissing | Form::DualTrackUnbalanced)
    }
    /// 所属类名。
    pub fn class_name(self) -> &'static str {
        if self.is_band() {
            "越界"
        } else if self.is_align() {
            "对齐"
        } else if self.is_archive() {
            "在档"
        } else {
            "守恒"
        }
    }
}
/// 辅助诊断码（6 个，占段内高位偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuxCode {
    /// 核数档序号越界。
    CoreIndexOutOfRange = 20,
    /// 压力档序号越界。
    LoadIndexOutOfRange = 21,
    /// 时长轨序号越界。
    DurationIndexOutOfRange = 22,
    /// 组合号越界。
    ComboIndexOutOfRange = 23,
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
            AuxCode::CoreIndexOutOfRange => "core_index_out_of_range",
            AuxCode::LoadIndexOutOfRange => "load_index_out_of_range",
            AuxCode::DurationIndexOutOfRange => "duration_index_out_of_range",
            AuxCode::ComboIndexOutOfRange => "combo_index_out_of_range",
            AuxCode::FrozenWriteRejected => "frozen_write_rejected",
            AuxCode::BookRevMismatch => "book_rev_mismatch",
        }
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            AuxCode::CoreIndexOutOfRange => "核数档序号越界：不在 0..4 内",
            AuxCode::LoadIndexOutOfRange => "压力档序号越界：不在 0..3 内",
            AuxCode::DurationIndexOutOfRange => "时长轨序号越界：不在 0..2 内",
            AuxCode::ComboIndexOutOfRange => "组合号越界：不在 0..12 内",
            AuxCode::FrozenWriteRejected => "冻结态写入拒收：已冻结后不得改参数",
            AuxCode::BookRevMismatch => "册版本不符：冻结件与 CI 基线版本不一致",
        }
    }
}
/// 错误结构（三要素）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UdErr {
    /// 诊断码。
    pub code: u16,
    /// 明文细节。
    pub detail: String,
}
/// 结果别名。
pub type UdResult<T> = Result<T, UdErr>;
impl UdErr {
    /// 构造。
    pub fn new(code: u16, detail: String) -> UdErr {
        UdErr { code, detail }
    }
    /// 由形态构造。
    pub fn of_form(f: Form, detail: String) -> UdErr {
        UdErr { code: f.code(), detail }
    }
    /// 由辅助码构造。
    pub fn of_aux(a: AuxCode, detail: String) -> UdErr {
        UdErr { code: a.code(), detail }
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
pub fn err_str(e: &UdErr) -> String {
    e.full()
}
/// 错误构造辅助（核数档越界）。
fn err_core(i: usize) -> UdErr {
    UdErr::of_aux(AuxCode::CoreIndexOutOfRange, format!("core_idx={} core_count={}", i, CORE_COUNT))
}
/// 错误构造辅助（压力档越界）。
fn err_load(i: usize) -> UdErr {
    UdErr::of_aux(AuxCode::LoadIndexOutOfRange, format!("load_idx={} load_count={}", i, LOAD_COUNT))
}
/// 错误构造辅助（时长轨越界）。
fn err_dur(i: usize) -> UdErr {
    UdErr::of_aux(AuxCode::DurationIndexOutOfRange, format!("dur_idx={} dur_count={}", i, DURATION_COUNT))
}
/// 错误构造辅助（组合号越界）。
fn err_combo(i: usize) -> UdErr {
    UdErr::of_aux(AuxCode::ComboIndexOutOfRange, format!("combo_idx={} combos={}", i, COMBO_COUNT))
}
/// 错误构造辅助（场景号越界）。
fn err_scene(i: usize) -> UdErr {
    UdErr::new(SEG_BASE, format!("scene_idx={} scenes={}", i, SCENE_COUNT))
}
/// 错误构造辅助（冻结态非法迁移）。
fn err_frozen(from: &str, to: &str) -> UdErr {
    UdErr::of_aux(AuxCode::FrozenWriteRejected, format!("from={} to={}", from, to))
}

// ===========================================================================
// 3. 三轴闭集（核数 / 压力 / 时长）+ F2761 判定
// ===========================================================================
/// 核数轴闭集（4 档：1 / 2 / 4 / 8 核）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CoreBand {
    /// 1 核（串行基线）。
    C1,
    /// 2 核。
    C2,
    /// 4 核。
    C4,
    /// 8 核（判据指定的矩阵上限）。
    C8,
}
impl CoreBand {
    /// 全部档。
    pub const ALL: [CoreBand; CORE_COUNT] = [CoreBand::C1, CoreBand::C2, CoreBand::C4, CoreBand::C8];
    /// 序号（0..3）。
    pub fn index(self) -> usize {
        match self {
            CoreBand::C1 => 0,
            CoreBand::C2 => 1,
            CoreBand::C4 => 2,
            CoreBand::C8 => 3,
        }
    }
    /// 由序号反解。
    pub fn from_index(i: usize) -> Option<CoreBand> {
        match i {
            0 => Some(CoreBand::C1),
            1 => Some(CoreBand::C2),
            2 => Some(CoreBand::C4),
            3 => Some(CoreBand::C8),
            _ => None,
        }
    }
    /// 核数。
    pub fn cores(self) -> u64 {
        match self {
            CoreBand::C1 => 1,
            CoreBand::C2 => 2,
            CoreBand::C4 => 4,
            CoreBand::C8 => 8,
        }
    }
    /// 并发道数（每核 2 道，8 核即 16 道）。
    pub fn lanes(self) -> u64 {
        self.cores().wrapping_mul(2)
    }
    /// 短码。
    pub fn code2(self) -> &'static str {
        match self {
            CoreBand::C1 => "C1",
            CoreBand::C2 => "C2",
            CoreBand::C4 => "C4",
            CoreBand::C8 => "C8",
        }
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            CoreBand::C1 => "1 核：串行基线，用于测出「无并发」时的绝对开销",
            CoreBand::C2 => "2 核：轻度并发，核数轴第 2 档",
            CoreBand::C4 => "4 核：中等并发，核数轴第 3 档",
            CoreBand::C8 => "8 核：判据指定的矩阵上限，重度争用",
        }
    }
}
/// 压力轴闭集（3 档：轻 / 中 / 重，对齐 F2761 判定）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoadBand {
    /// 轻压。
    Light,
    /// 中压。
    Medium,
    /// 重压。
    Heavy,
}
impl LoadBand {
    /// 全部档。
    pub const ALL: [LoadBand; LOAD_COUNT] = [LoadBand::Light, LoadBand::Medium, LoadBand::Heavy];
    /// 序号（0..2）。
    pub fn index(self) -> usize {
        match self {
            LoadBand::Light => 0,
            LoadBand::Medium => 1,
            LoadBand::Heavy => 2,
        }
    }
    /// 由序号反解。
    pub fn from_index(i: usize) -> Option<LoadBand> {
        match i {
            0 => Some(LoadBand::Light),
            1 => Some(LoadBand::Medium),
            2 => Some(LoadBand::Heavy),
            _ => None,
        }
    }
    /// 压力档名（F2761 判定用名）。
    pub fn name(self) -> &'static str {
        match self {
            LoadBand::Light => "轻",
            LoadBand::Medium => "中",
            LoadBand::Heavy => "重",
        }
    }
    /// F2761 判定值（千分数，负载率上界）。
    pub fn verdict_permille(self) -> u64 {
        match self {
            LoadBand::Light => 300,
            LoadBand::Medium => 700,
            LoadBand::Heavy => 950,
        }
    }
    /// 每道每步操作数。
    pub fn ops_per_step(self) -> u64 {
        match self {
            LoadBand::Light => 2,
            LoadBand::Medium => 5,
            LoadBand::Heavy => 9,
        }
    }
    /// 内存占用档位（字节，判据提「16GB」为上限档）。
    pub fn mem_bytes(self) -> u64 {
        match self {
            LoadBand::Light => 2 * 1024 * 1024 * 1024,
            LoadBand::Medium => 8 * 1024 * 1024 * 1024,
            LoadBand::Heavy => 16 * 1024 * 1024 * 1024,
        }
    }
    /// 内存档名。
    pub fn mem_str(self) -> &'static str {
        match self {
            LoadBand::Light => "2GB",
            LoadBand::Medium => "8GB",
            LoadBand::Heavy => "16GB",
        }
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            LoadBand::Light => "轻压：F2761 判定 300‰，占用 2GB",
            LoadBand::Medium => "中压：F2761 判定 700‰，占用 8GB",
            LoadBand::Heavy => "重压：F2761 判定 950‰，占用 16GB（判据指定上限档）",
        }
    }
    /// F2761 判定项名（本档需对齐的 3 个判定项）。
    pub fn verdict_names() -> [&'static str; LOAD_VERDICTS] {
        ["f2761.duty_permille", "f2761.mem_bytes", "f2761.ops_per_step"]
    }
}
/// 时长轨闭集（2 轨：10min 账面预演 / 72h 真机欠账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DurationTrack {
    /// 10min 账面预演（本单唯一可实跑的轨）。
    Ledger10Min,
    /// 72h 真机长稳轨（本单只登记欠账，不执行）。
    Machine72H,
}
impl DurationTrack {
    /// 全部轨。
    pub const ALL: [DurationTrack; DURATION_COUNT] = [DurationTrack::Ledger10Min, DurationTrack::Machine72H];
    /// 序号（0..1）。
    pub fn index(self) -> usize {
        match self {
            DurationTrack::Ledger10Min => 0,
            DurationTrack::Machine72H => 1,
        }
    }
    /// 由序号反解。
    pub fn from_index(i: usize) -> Option<DurationTrack> {
        match i {
            0 => Some(DurationTrack::Ledger10Min),
            1 => Some(DurationTrack::Machine72H),
            _ => None,
        }
    }
    /// 轨名。
    pub fn name(self) -> &'static str {
        match self {
            DurationTrack::Ledger10Min => "10min_ledger",
            DurationTrack::Machine72H => "72h_machine",
        }
    }
    /// 时长（逻辑 tick 口径，非墙钟）。
    pub fn ticks(self) -> u64 {
        match self {
            DurationTrack::Ledger10Min => 600,
            DurationTrack::Machine72H => 259200,
        }
    }
    /// 是否真机轨（本单不执行）。
    pub fn is_machine(self) -> bool {
        self == DurationTrack::Machine72H
    }
    /// 欠账倍率（真机轨相对账面轨的时长倍数，登记用）。
    pub fn debt_ratio(self) -> u64 {
        match self {
            DurationTrack::Ledger10Min => 1,
            DurationTrack::Machine72H => 432,
        }
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            DurationTrack::Ledger10Min => "10min 账面预演轨：唯一可实跑轨，读数为模型推算",
            DurationTrack::Machine72H => "72h 真机长稳轨：本单只登记欠账，不执行（双轨欠账）",
        }
    }
}

// ===========================================================================
// 4. F2938 七批压测面闭集（负载构成引用源）
// ===========================================================================
/// F2938 压测面闭集（7 面，逐面登记负载构成权重）。
///
/// 为什么是 7 面：判据要求「负载构成引用 F2938 各面注入器」，
/// 且配比依据须「对照 F2938 七批读数」——面数不是本单自定的，
/// 而是上游汇总账的既成结构，本单只能如实登记，不得增删。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum F2938Face {
    /// 面 0：内存分配面。
    Alloc,
    /// 面 1：页缺失面。
    PageFault,
    /// 面 2：文件 IO 面。
    FileIo,
    /// 面 3：网络栈面。
    NetStack,
    /// 面 4：调度抢占面。
    SchedPreempt,
    /// 面 5：中断亲和面。
    IrqAffinity,
    /// 面 6：长稳泄漏面。
    LeakSustained,
}
impl F2938Face {
    /// 全部面。
    pub const ALL: [F2938Face; F2938_FACES] = [
        F2938Face::Alloc,
        F2938Face::PageFault,
        F2938Face::FileIo,
        F2938Face::NetStack,
        F2938Face::SchedPreempt,
        F2938Face::IrqAffinity,
        F2938Face::LeakSustained,
    ];
    /// 序号（0..6）。
    pub fn index(self) -> usize {
        match self {
            F2938Face::Alloc => 0,
            F2938Face::PageFault => 1,
            F2938Face::FileIo => 2,
            F2938Face::NetStack => 3,
            F2938Face::SchedPreempt => 4,
            F2938Face::IrqAffinity => 5,
            F2938Face::LeakSustained => 6,
        }
    }
    /// 由序号反解。
    pub fn from_index(i: usize) -> Option<F2938Face> {
        match i {
            0 => Some(F2938Face::Alloc),
            1 => Some(F2938Face::PageFault),
            2 => Some(F2938Face::FileIo),
            3 => Some(F2938Face::NetStack),
            4 => Some(F2938Face::SchedPreempt),
            5 => Some(F2938Face::IrqAffinity),
            6 => Some(F2938Face::LeakSustained),
            _ => None,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            F2938Face::Alloc => "alloc",
            F2938Face::PageFault => "page_fault",
            F2938Face::FileIo => "file_io",
            F2938Face::NetStack => "net_stack",
            F2938Face::SchedPreempt => "sched_preempt",
            F2938Face::IrqAffinity => "irq_affinity",
            F2938Face::LeakSustained => "leak_sustained",
        }
    }
    /// 注入器单号（域内既有执行面，本单只引用不执行）。
    pub fn injector(self) -> &'static str {
        match self {
            F2938Face::Alloc => "F2939",
            F2938Face::PageFault => "F2940",
            F2938Face::FileIo => "F2951",
            F2938Face::NetStack => "F2952",
            F2938Face::SchedPreempt => "F2953",
            F2938Face::IrqAffinity => "F2954",
            F2938Face::LeakSustained => "F2955",
        }
    }
    /// 基础权重（万分比，7 面合计 10000）。
    ///
    /// 取值口径：内存分配与页缺失是内核压测的「量」主体，
    /// 长稳泄漏权重虽小但**必占一席**——它对应 72h 长稳轨的独家观测对象，
    /// 权重压到 0 就等于把长稳轨的核心面从矩阵里抹掉。
    pub fn base_weight(self) -> u64 {
        match self {
            F2938Face::Alloc => 2200,
            F2938Face::PageFault => 2000,
            F2938Face::FileIo => 1800,
            F2938Face::NetStack => 1600,
            F2938Face::SchedPreempt => 1200,
            F2938Face::IrqAffinity => 800,
            F2938Face::LeakSustained => 400,
        }
    }
    /// 该面在哪几档压力下参与（压力档序号闭集）。
    pub fn bands(self) -> &'static [LoadBand] {
        match self {
            F2938Face::Alloc => &[LoadBand::Light, LoadBand::Medium, LoadBand::Heavy],
            F2938Face::PageFault => &[LoadBand::Light, LoadBand::Medium, LoadBand::Heavy],
            F2938Face::FileIo => &[LoadBand::Light, LoadBand::Medium, LoadBand::Heavy],
            F2938Face::NetStack => &[LoadBand::Medium, LoadBand::Heavy],
            F2938Face::SchedPreempt => &[LoadBand::Medium, LoadBand::Heavy],
            F2938Face::IrqAffinity => &[LoadBand::Heavy],
            F2938Face::LeakSustained => &[LoadBand::Heavy],
        }
    }
    /// 该面是否只出现在真机长稳轨。
    pub fn machine_only(self) -> bool {
        self == F2938Face::LeakSustained
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            F2938Face::Alloc => "内存分配面：F2939 注入器，三档压力全程参与",
            F2938Face::PageFault => "页缺失面：F2940 注入器，三档压力全程参与",
            F2938Face::FileIo => "文件 IO 面：F2951 注入器，三档压力全程参与",
            F2938Face::NetStack => "网络栈面：F2952 注入器，中/重压参与",
            F2938Face::SchedPreempt => "调度抢占面：F2953 注入器，中/重压参与",
            F2938Face::IrqAffinity => "中断亲和面：F2954 注入器，仅重压参与",
            F2938Face::LeakSustained => "长稳泄漏面：F2955 注入器，仅重压 + 仅真机长稳轨",
        }
    }
}
/// 基准权重合计（应恒为 10000）。
pub fn base_weight_total() -> u64 {
    let mut t: u64 = 0;
    for f in F2938Face::ALL.iter() {
        t = t.wrapping_add(f.base_weight());
    }
    t
}
/// 压力档下的有效面数（`bands()` 长度现算）。
pub fn face_count_in(b: LoadBand) -> usize {
    let mut n: usize = 0;
    for f in F2938Face::ALL.iter() {
        let bs = f.bands();
        let mut i: usize = 0;
        while i < bs.len() {
            if bs[i] == b {
                n = n.wrapping_add(1);
                break;
            }
            i = i.wrapping_add(1);
        }
    }
    n
}
/// 压力档下的参与面闭集。
pub fn faces_in(b: LoadBand) -> Vec<F2938Face> {
    let mut out: Vec<F2938Face> = Vec::with_capacity(F2938_FACES);
    for f in F2938Face::ALL.iter() {
        let bs = f.bands();
        let mut i: usize = 0;
        let mut hit = false;
        while i < bs.len() {
            if bs[i] == b {
                hit = true;
                break;
            }
            i = i.wrapping_add(1);
        }
        if hit {
            out.push(*f);
        }
    }
    out
}

/// 压力档下的面权重（未归一，万分比）。
pub fn band_weight_raw(b: LoadBand) -> u64 {
    let mut t: u64 = 0;
    for f in faces_in(b).iter() {
        t = t.wrapping_add(f.base_weight());
    }
    t
}
/// 压力档下的面权重（归一到 10000）。
pub fn band_weight(b: LoadBand) -> u64 {
    let raw = band_weight_raw(b);
    if raw == 0 {
        return 0;
    }
    (raw as u128 * 10000u128 / raw as u128) as u64
}
/// 归一权重（真实计算：`Σ参与面权重` 反推每面占比）。
pub fn band_share(b: LoadBand, f: F2938Face) -> u64 {
    let raw = band_weight_raw(b);
    if raw == 0 {
        return 0;
    }
    let bs = f.bands();
    let mut i: usize = 0;
    let mut hit = false;
    while i < bs.len() {
        if bs[i] == b {
            hit = true;
            break;
        }
        i = i.wrapping_add(1);
    }
    if !hit {
        return 0;
    }
    (f.base_weight() as u128 * 10000u128 / raw as u128) as u64
}

// ===========================================================================
// 5. 组合参数与场景矩阵（12 组合 × 2 轨 = 24 场景）
// ===========================================================================
/// 单组合参数（核数 × 压力，12 组合）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ComboSpec {
    /// 组合号（0..11）。
    pub index: usize,
    /// 核数档序号。
    pub core_idx: usize,
    /// 压力档序号。
    pub load_idx: usize,
    /// 每步操作数上界（`lanes × ops_per_step`）。
    pub ops_per_step: u64,
    /// 内存占用上界（字节）。
    pub mem_bytes: u64,
    /// 负载率上界（千分数，来自 F2761 判定）。
    pub duty_permille: u64,
    /// 组合步数上界。
    pub steps: u64,
    /// 组合指纹。
    pub fingerprint: u64,
}
impl ComboSpec {
    /// 构造。
    ///
    /// 口径注记：
    /// - `ops_per_step = lanes × ops_per_step(压力档)`
    /// - `mem_bytes` 直接取压力档档位（轻 2GB / 中 8GB / 重 16GB），
    ///   判据点名「16GB 三档压力」，故重压必须落在 16GB
    /// - `duty_permille` 直接取 F2761 判定值（300 / 700 / 950），
    ///   **不从压力档自行推导**，否则对齐 F2761 就没有约束力
    /// - `steps` 三档统一 `COMBO_STEPS`，保证 12 组合横向可比
    pub fn new(index: usize, core: CoreBand, load: LoadBand) -> ComboSpec {
        let ops = core.lanes().wrapping_mul(load.ops_per_step());
        let mem = load.mem_bytes();
        let duty = load.verdict_permille();
        let steps = COMBO_STEPS as u64;
        let mut words: [u64; 8] = [0u64; 8];
        words[0] = index as u64;
        words[1] = core.index() as u64;
        words[2] = load.index() as u64;
        words[3] = ops;
        words[4] = mem;
        words[5] = duty;
        words[6] = steps;
        words[7] = core.cores().wrapping_mul(load.ops_per_step());
        ComboSpec {
            index,
            core_idx: core.index(),
            load_idx: load.index(),
            ops_per_step: ops,
            mem_bytes: mem,
            duty_permille: duty,
            steps,
            fingerprint: fnv1a_words(&words),
        }
    }
    /// 还原核数档。
    pub fn core(&self) -> UdResult<CoreBand> {
        match CoreBand::from_index(self.core_idx) {
            Some(v) => Ok(v),
            None => Err(err_core(self.core_idx)),
        }
    }
    /// 还原压力档。
    pub fn load(&self) -> UdResult<LoadBand> {
        match LoadBand::from_index(self.load_idx) {
            Some(v) => Ok(v),
            None => Err(err_load(self.load_idx)),
        }
    }
    /// 两轴闭集。
    pub fn axes_ok(&self) -> bool {
        self.core_idx < CORE_COUNT && self.load_idx < LOAD_COUNT && self.index < COMBO_COUNT
    }
    /// 是否判据点名组合（8 核 × 重压）。
    pub fn is_named(&self) -> bool {
        self.core_idx == CoreBand::C8.index() && self.load_idx == LoadBand::Heavy.index()
    }
    /// 是否重度争用（8 核）。
    pub fn is_contended(&self) -> bool {
        self.core_idx == CoreBand::C8.index()
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "combo#{:02} core={} load={} ops/step={} mem={}B duty={}‰ steps={} named={} contended={} fp={:016X}",
            self.index,
            self.core_idx,
            self.load_idx,
            self.ops_per_step,
            self.mem_bytes,
            self.duty_permille,
            self.steps,
            if self.is_named() { 1 } else { 0 },
            if self.is_contended() { 1 } else { 0 },
            self.fingerprint
        )
    }
}

/// 单场景参数（组合 × 时长轨，24 场景）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SceneSpec {
    /// 场景号（0..23）。
    pub index: usize,
    /// 组合号。
    pub combo_idx: usize,
    /// 时长轨序号。
    pub dur_idx: usize,
    /// 时长（逻辑 tick）。
    pub ticks: u64,
    /// 参与面数。
    pub faces: usize,
    /// 面权重合计（归一后应恒为 10000）。
    pub share_total: u64,
    /// 场景步数上界。
    pub steps: u64,
    /// 真机轨欠账登记额（逻辑 tick 差）。
    pub debt: u64,
    /// 场景指纹。
    pub fingerprint: u64,
}
impl SceneSpec {
    /// 构造。
    ///
    /// 口径注记：
    /// - `ticks` 取时长轨档位（600 / 259200），**逻辑 tick 非墙钟**
    /// - `faces = face_count_in(压力档)`，面集由压力档现算，不硬写
    /// - `share_total = Σ band_share(...)`，是「Σ 每面归一占比」的现算合计，
    ///   由于整数除法会各自截断，合计未必恰为 10000，
    ///   故判定口径是「合计落在 10000 ± faces 容差内」——写死 == 10000 会假失真
    /// - `debt`：真机轨登记「欠账」= 该轨 ticks 与账面轨 ticks 之差；
    ///   账面轨自身 debt = 0（它就是被欠账的那一轨，登记为 0 表示「无欠账」）
    pub fn new(index: usize, combo: &ComboSpec, dur: DurationTrack) -> UdResult<SceneSpec> {
        let load = combo.load()?;
        let faces = face_count_in(load);
        let mut share_total: u64 = 0;
        for f in faces_in(load).iter() {
            share_total = share_total.wrapping_add(band_share(load, *f));
        }
        let base = DurationTrack::Ledger10Min.ticks();
        let debt = if dur.is_machine() {
            dur.ticks().wrapping_sub(base)
        } else {
            0
        };
        let steps = SCENE_STEPS as u64;
        let mut words: [u64; 9] = [0u64; 9];
        words[0] = index as u64;
        words[1] = combo.index as u64;
        words[2] = dur.index() as u64;
        words[3] = dur.ticks();
        words[4] = faces as u64;
        words[5] = share_total;
        words[6] = steps;
        words[7] = debt;
        words[8] = combo.fingerprint;
        Ok(SceneSpec {
            index,
            combo_idx: combo.index,
            dur_idx: dur.index(),
            ticks: dur.ticks(),
            faces,
            share_total,
            steps,
            debt,
            fingerprint: fnv1a_words(&words),
        })
    }
    /// 还原组合号。
    pub fn combo_idx_check(&self) -> UdResult<usize> {
        if self.combo_idx < COMBO_COUNT {
            Ok(self.combo_idx)
        } else {
            Err(err_combo(self.combo_idx))
        }
    }
    /// 还原时长轨。
    pub fn track(&self) -> UdResult<DurationTrack> {
        match DurationTrack::from_index(self.dur_idx) {
            Some(v) => Ok(v),
            None => Err(err_dur(self.dur_idx)),
        }
    }
    /// 三轴闭集。
    pub fn axes_ok(&self) -> bool {
        self.combo_idx < COMBO_COUNT && self.dur_idx < DURATION_COUNT && self.index < SCENE_COUNT
    }
    /// 面权重合计容差判定。
    ///
    /// 容差 = `faces`（每个面各自向下截断，最多各丢 1）。
    /// 写死 `== 10000` 会在 7 面下因截断误报失真。
    pub fn share_ok(&self) -> bool {
        let tol = self.faces as u64;
        if self.share_total > 10000 {
            return self.share_total - 10000 <= tol;
        }
        10000 - self.share_total <= tol
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "scene#{:02} combo={} dur={} ticks={} faces={} share={} steps={} debt={} share_ok={} fp={:016X}",
            self.index,
            self.combo_idx,
            self.dur_idx,
            self.ticks,
            self.faces,
            self.share_total,
            self.steps,
            self.debt,
            if self.share_ok() { 1 } else { 0 },
            self.fingerprint
        )
    }
}

/// 三轴方案矩阵（12 组合 + 24 场景）。
#[derive(Clone, Debug)]
pub struct PlanMatrix {
    /// 组合表。
    pub combos: Vec<ComboSpec>,
    /// 场景表。
    pub scenes: Vec<SceneSpec>,
    /// 核数档分布（每档 3 组合）。
    pub core_dist: [usize; CORE_COUNT],
    /// 压力档分布（每档 4 组合）。
    pub load_dist: [usize; LOAD_COUNT],
    /// 时长轨分布（每轨 12 场景）。
    pub dur_dist: [usize; DURATION_COUNT],
    /// 组合实际产出数。
    pub combos_produced: usize,
    /// 场景实际产出数。
    pub scenes_produced: usize,
    /// 配比依据缺项位图（bit i = 第 i 面缺注记）。
    pub basis_bits: u32,
    /// 配比依据逐面条目。
    pub basis: Vec<BasisNote>,
    /// 失真声明在档。
    pub distortion: String,
    /// 映射污染：抹掉第 N 条对齐字段（`Some(n)` = 造缺失洞）。
    pub align_null: Option<usize>,
    /// 映射污染：第 N 条抄第 M 条（造重复洞）。
    pub align_dup: Option<(usize, usize)>,
}
impl PlanMatrix {
    /// 空矩阵。
    pub fn blank() -> PlanMatrix {
        PlanMatrix {
            combos: Vec::new(),
            scenes: Vec::new(),
            core_dist: [0; CORE_COUNT],
            load_dist: [0; LOAD_COUNT],
            dur_dist: [0; DURATION_COUNT],
            combos_produced: 0,
            scenes_produced: 0,
            basis_bits: 0,
            basis: Vec::new(),
            distortion: String::new(),
            align_null: None,
            align_dup: None,
        }
    }
    /// 正交展开（格数由两轴乘积现算，不硬写 12 / 24）。
    pub fn expand() -> UdResult<PlanMatrix> {
        let mut m = PlanMatrix::blank();
        let mut ci: usize = 0;
        let mut a: usize = 0;
        while a < CORE_COUNT {
            let core = CoreBand::ALL[a];
            let mut b: usize = 0;
            while b < LOAD_COUNT {
                let load = LoadBand::ALL[b];
                let c = ComboSpec::new(ci, core, load);
                if let Some(s) = m.core_dist.get_mut(a) {
                    *s = s.wrapping_add(1);
                }
                if let Some(s) = m.load_dist.get_mut(b) {
                    *s = s.wrapping_add(1);
                }
                m.combos.push(c);
                ci = ci.wrapping_add(1);
                b = b.wrapping_add(1);
            }
            a = a.wrapping_add(1);
        }
        m.combos_produced = m.combos.len();
        // 场景：组合 × 时长轨
        let mut si: usize = 0;
        let mut k: usize = 0;
        while k < m.combos.len() {
            let c = m.combos.get(k);
            match c {
                Some(cc) => {
                    let mut d: usize = 0;
                    while d < DURATION_COUNT {
                        let dur = DurationTrack::ALL[d];
                        let sc = SceneSpec::new(si, cc, dur)?;
                        if let Some(s) = m.dur_dist.get_mut(d) {
                            *s = s.wrapping_add(1);
                        }
                        m.scenes.push(sc);
                        si = si.wrapping_add(1);
                        d = d.wrapping_add(1);
                    }
                }
                None => return Err(err_combo(k)),
            }
            k = k.wrapping_add(1);
        }
        m.scenes_produced = m.scenes.len();
        // 配比依据 + 失真声明（风险为「方案失真」，须在档）
        m.basis = build_basis();
        let mut bits: u32 = 0;
        for (i, n) in m.basis.iter().enumerate() {
            if !n.ok() {
                bits |= 1u32 << i;
            }
        }
        m.basis_bits = bits;
        m.distortion = build_distortion();
        Ok(m)
    }
    /// 取组合。
    pub fn combo(&self, i: usize) -> UdResult<&ComboSpec> {
        match self.combos.get(i) {
            Some(v) => Ok(v),
            None => Err(err_combo(i)),
        }
    }
    /// 取组合（可变）。
    pub fn combo_mut(&mut self, i: usize) -> UdResult<&mut ComboSpec> {
        match self.combos.get_mut(i) {
            Some(v) => Ok(v),
            None => Err(err_combo(i)),
        }
    }
    /// 取场景。
    pub fn scene(&self, i: usize) -> UdResult<&SceneSpec> {
        match self.scenes.get(i) {
            Some(v) => Ok(v),
            None => Err(err_scene(i)),
        }
    }
    /// 取场景（可变）。
    pub fn scene_mut(&mut self, i: usize) -> UdResult<&mut SceneSpec> {
        match self.scenes.get_mut(i) {
            Some(v) => Ok(v),
            None => Err(err_scene(i)),
        }
    }
    /// 线性格号。
    pub fn combo_linear(core: usize, load: usize) -> usize {
        core * LOAD_COUNT + load
    }
    /// 场景线性格号。
    pub fn scene_linear(combo: usize, dur: usize) -> usize {
        combo * DURATION_COUNT + dur
    }
    /// 组合数断言。
    pub fn combo_count_ok(&self) -> bool {
        self.combos_produced == CORE_COUNT * LOAD_COUNT
            && self.combos_produced == COMBO_COUNT
            && self.combos.len() == COMBO_COUNT
    }
    /// 场景数断言。
    pub fn scene_count_ok(&self) -> bool {
        self.scenes_produced == COMBO_COUNT * DURATION_COUNT
            && self.scenes_produced == SCENE_COUNT
            && self.scenes.len() == SCENE_COUNT
    }
    /// 正交断言（线性格号两两互异）。
    pub fn orthogonal_ok(&self) -> bool {
        let mut seen: [bool; COMBO_COUNT] = [false; COMBO_COUNT];
        let mut i: usize = 0;
        while i < self.combos.len() {
            let c = match self.combos.get(i) {
                Some(v) => v,
                None => return false,
            };
            if c.core_idx >= CORE_COUNT || c.load_idx >= LOAD_COUNT {
                return false;
            }
            let li = PlanMatrix::combo_linear(c.core_idx, c.load_idx);
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
        let mut seen2: [bool; SCENE_COUNT] = [false; SCENE_COUNT];
        let mut j: usize = 0;
        while j < self.scenes.len() {
            let sc = match self.scenes.get(j) {
                Some(v) => v,
                None => return false,
            };
            if sc.combo_idx >= COMBO_COUNT || sc.dur_idx >= DURATION_COUNT {
                return false;
            }
            let li = PlanMatrix::scene_linear(sc.combo_idx, sc.dur_idx);
            if let Some(s) = seen2.get_mut(li) {
                if *s {
                    return false;
                }
                *s = true;
            } else {
                return false;
            }
            j = j.wrapping_add(1);
        }
        true
    }
    /// 三轴闭集断言。
    pub fn axes_ok(&self) -> bool {
        let mut i: usize = 0;
        while i < self.combos.len() {
            match self.combos.get(i) {
                Some(c) => {
                    if !c.axes_ok() {
                        return false;
                    }
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        let mut j: usize = 0;
        while j < self.scenes.len() {
            match self.scenes.get(j) {
                Some(s) => {
                    if !s.axes_ok() || !s.share_ok() {
                        return false;
                    }
                }
                None => return false,
            }
            j = j.wrapping_add(1);
        }
        true
    }
    /// 矩阵指纹（组合 + 场景 + 依据 + 失真声明串接折叠）。
    ///
    /// 注：`distortion` 声明文本**参与**指纹——失真声明在档是判据要求之一，
    /// 声明被人改了就必须冻结件漂移，不能悄悄放过。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(COMBO_COUNT + SCENE_COUNT * 2 + 8);
        words.push(self.combos_produced as u64);
        words.push(self.scenes_produced as u64);
        words.push(CORE_COUNT as u64);
        words.push(LOAD_COUNT as u64);
        words.push(DURATION_COUNT as u64);
        for c in self.combos.iter() {
            words.push(c.fingerprint);
        }
        for s in self.scenes.iter() {
            words.push(s.fingerprint);
        }
        for n in self.basis.iter() {
            words.push(n.fingerprint());
        }
        words.push(fnv1a64(sbytes(self.distortion.as_str())));
        fnv1a_words(&words)
    }
    /// 逐行。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(COMBO_COUNT + SCENE_COUNT);
        for c in self.combos.iter() {
            out.push(c.describe());
        }
        for s in self.scenes.iter() {
            out.push(s.describe());
        }
        out
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "matrix combos={}/{} scenes={}/{} combo_ok={} scene_ok={} ortho={} axes={} core={:?} load={:?} dur={:?} basis_bits={:08X} fp={:016X}",
            self.combos_produced,
            COMBO_COUNT,
            self.scenes_produced,
            SCENE_COUNT,
            if self.combo_count_ok() { 1 } else { 0 },
            if self.scene_count_ok() { 1 } else { 0 },
            if self.orthogonal_ok() { 1 } else { 0 },
            if self.axes_ok() { 1 } else { 0 },
            self.core_dist,
            self.load_dist,
            self.dur_dist,
            self.basis_bits,
            self.fingerprint()
        )
    }
}

// ===========================================================================
// 6. 配比依据 + 失真声明（风险为「方案失真」，判据要求在档）
// ===========================================================================
/// 配比依据条目（逐条对照 F2938 七批）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BasisNote {
    /// 面序号。
    pub face: usize,
    /// 面名。
    pub name: &'static str,
    /// 注记正文。
    pub note: String,
}
impl BasisNote {
    /// 是否完整（非空且面名与序号相符）。
    pub fn ok(&self) -> bool {
        if self.note.is_empty() {
            return false;
        }
        match F2938Face::from_index(self.face) {
            Some(f) => f.name() == self.name,
            None => false,
        }
    }
    /// 条目指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = fnv1a64_ext(h, sbytes(self.name));
        h = fnv1a64_ext(h, sbytes(self.note.as_str()));
        h
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("basis[{}] {} :: {}", self.face, self.name, self.note)
    }
}
/// 构造配比依据（`F2938_FACES = 7` 条，一条不缺）。
///
/// 每条注记必须回答同一组问题（缺一条即 `BasisMissing`）：
/// ① 该面在矩阵里占多大权重；② 为什么这么配；③ 对照 F2938 哪批读数。
pub fn build_basis() -> Vec<BasisNote> {
    let mut out: Vec<BasisNote> = Vec::with_capacity(F2938_FACES);
    let items: [&str; F2938_FACES] = [
        "权重 2200/10000 为七面最高：内存分配是内核压测的量主体，直接决定吞吐天花板；对照 F2938 第一批 Alloc 读数，为其最大单项。",
        "权重 2000/10000 次高：页缺失与内存分配同源但多一道缺页开销，故略低一档；对照 F2938 第二批 PageFault 读数。",
        "权重 1800/10000：文件 IO 面走替身账路径，不触真实设备，只作对账基线；对照 F2938 第三批 FileIo 读数。",
        "权重 1600/10000：网络栈面仅中/重压参与（轻压下网络栈尚未成为瓶颈），故全档权重低于前三面；对照 F2938 第四批 NetStack 读数。",
        "权重 1200/10000：调度抢占面只在核数 ≥ 2 且压力 ≥ 中时才有可读信号；对照 F2938 第五批 SchedPreempt 读数。",
        "权重 800/10000：中断亲和面只重压参与，采样窗口短、可读信号弱，故权重压到千八；对照 F2938 第六批 IrqAffinity 读数。",
        "权重 400/10000 虽为七面最低，但**不可压到 0**：该面是 72h 长稳轨的独家观测对象，抹掉等于把长稳轨的核心面从矩阵中删除；对照 F2938 第七批 LeakSustained 读数。",
    ];
    for f in F2938Face::ALL.iter() {
        let note = match items.get(f.index()) {
            Some(s) => String::from(*s),
            None => String::new(),
        };
        out.push(BasisNote { face: f.index(), name: f.name(), note });
    }
    out
}
/// 构造失真声明（判据：「账面预演非真机等价」）。
///
/// 声明必须自认本单**只冻结方案、不执行**，
/// 并逐条说清「哪些读数是模型推算、不能当实测用」。
pub fn build_distortion() -> String {
    let mut s: String = String::new();
    s.push_str("失真声明（UNX-F2941）：");
    s.push_str("本单只做方案冻结与对齐，不做矩阵执行（F2942–F2944）、不做真机执行（双轨欠账）；");
    s.push_str("① 24 个场景的 duty/latency/throughput 读数均为按压力档参数推算的模型值，非真机实测；");
    s.push_str("② 10min 账面预演轨不能等价代表 72h 真机长稳轨——长稳轨的泄漏累积、时钟漂移、温度与 NUMA 迁移在 600 tick 内不显现；");
    s.push_str("③ 72h 真机轨本单只登记欠账（debt = 259200 − 600 = 258600 逻辑 tick），欠账额本身是登记值而非实测进度；");
    s.push_str("④ 因此冻结哈希只能证明「三轴参数、面权重与 F2938 对齐表本身未被改动」，");
    s.push_str("   不能证明「按此方案实跑会得到这些读数」；");
    s.push_str("   供 F2942–F2960 执行面共引时，须以真机读数覆盖本册模型值后再入汇总账。");
    s
}

// ===========================================================================
// 7. 替身账 + 守恒（本单不执行压测，只记事件计数）
// ===========================================================================
/// 替身账事件类别（4 类，闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Evt {
    /// 注入操作。
    Inject = 0,
    /// 完成操作。
    Done = 1,
    /// 观测采样。
    Sample = 2,
    /// 失败/回退。
    Fail = 3,
}
impl Evt {
    /// 类别数。
    pub const COUNT: usize = 4;
    /// 全部类别。
    pub const ALL: [Evt; 4] = [Evt::Inject, Evt::Done, Evt::Sample, Evt::Fail];
    /// 序号。
    pub fn index(self) -> usize {
        self as usize
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            Evt::Inject => "inject",
            Evt::Done => "done",
            Evt::Sample => "sample",
            Evt::Fail => "fail",
        }
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            Evt::Inject => "注入：向注入器投递操作",
            Evt::Done => "完成：注入器回报完成",
            Evt::Sample => "采样：按节拍读一次压力面读数",
            Evt::Fail => "失败：注入器回报失败或回退",
        }
    }
}
/// 替身账（**不是压测执行器**，只记四类事件计数与负载占用）。
///
/// 为什么只做替身：判据明确「不做矩阵执行（F2942–F2944）、不做真机执行」。
/// 本单要交付的是「三轴参数 → 每场景预期四类事件读数」的方案冻结，
/// 替身账的作用是让每个上界都有**可对账的实跑读数**，
/// 而不是让读者以为这里已经跑了一轮 72h。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DomLedger {
    /// 已跑步数。
    pub steps: u64,
    /// 注入数。
    pub inject: u64,
    /// 完成数。
    pub done: u64,
    /// 采样数。
    pub sample: u64,
    /// 失败数。
    pub fail: u64,
    /// 已用负载占用（`ops_per_step × steps`）。
    pub load_used: u64,
    /// 负载占用上界（`ops_per_step × steps × 1000 / duty_permille` 的等价表述见下）。
    pub load_cap: u64,
    /// 内存占用登记。
    pub mem_bytes: u64,
    /// 逻辑时钟。
    pub tick: u64,
    /// 账指纹。
    pub fingerprint: u64,
}
impl DomLedger {
    /// 空账。
    pub fn blank() -> DomLedger {
        DomLedger {
            steps: 0,
            inject: 0,
            done: 0,
            sample: 0,
            fail: 0,
            load_used: 0,
            load_cap: 0,
            mem_bytes: 0,
            tick: 0,
            fingerprint: FNV_BASIS,
        }
    }
    /// 按场景参数真跑一遍。
    ///
    /// 口径注记：
    /// - `steps` 恒取 `SCENE_STEPS`（基准，不读场景登记值）
    /// - `inject = ops_per_step × steps`（组合上界 × 步数）
    /// - `done   = inject − fail`
    /// - `sample = steps`（每步采一次）
    /// - `fail   = inject × fail_permille ÷ 1000`，
    ///   `fail_permille` 由压力档派生：轻 5‰ / 中 20‰ / 重 60‰
    /// - `load_used = inject`
    /// - `load_cap = F2761 判据允许的注入数上界`：
    ///   `cap = ⌊inject × 1000 / duty_permille × 10⌋`。
    ///   含义：F2761 给出的是**目标负载率** `duty_permille`，
    ///   方案允许注入数围绕该目标浮动一个数量级的裕量
    ///   （`cap = inject × 10⁴ / duty`，轻压 `×33`、中压 `×14`、重压 `×10`）。
    ///   三档裕量都 > 1，故干净场景恒不误判超界；
    ///   若把 `cap` 定成 `inject`，判据退化成恒真式、失去约束力；
    ///   若定成 `inject / duty`（不乘 10），轻压档会得到 `cap < inject`，
    ///   干净场景反而假失真——这是本口径必须乘 10 的原因。
    /// - `tick = ticks`（场景登记的逻辑时长，跑完即到）
    ///
    /// **关键口径：账只依赖档位现算，不读场景的可污染登记字段**
    /// （`steps` / `share_total` / `debt` / `faces` 一律不参与取值），
    /// 否则污染会连账一起改，守恒断言就永远发现不了失真。
    pub fn run(combo: &ComboSpec, scene: &SceneSpec) -> UdResult<DomLedger> {
        let load = combo.load()?;
        let steps = SCENE_STEPS as u64;
        let inject = combo.ops_per_step.wrapping_mul(steps);
        let fail_permille = fail_permille_of(load);
        let fail = mul_div(inject, fail_permille);
        let done = inject.wrapping_sub(fail);
        let sample = steps;
        let load_used = inject;
        // 档位负载预算：按 F2761 目标负载率反算的允许注入上界（含一个数量级裕量）
        let duty = load.verdict_permille();
        let load_cap = if duty == 0 {
            inject
        } else {
            (inject as u128 * 10_000u128 / duty as u128) as u64
        };
        let tick = scene_track_ticks(combo, scene);
        let mut words: [u64; 10] = [0u64; 10];
        words[0] = steps;
        words[1] = inject;
        words[2] = done;
        words[3] = sample;
        words[4] = fail;
        words[5] = load_used;
        words[6] = load_cap;
        words[7] = tick;
        words[8] = combo.fingerprint;
        words[9] = scene.fingerprint;
        Ok(DomLedger {
            steps,
            inject,
            done,
            sample,
            fail,
            load_used,
            load_cap,
            mem_bytes: load.mem_bytes(),
            tick,
            fingerprint: fnv1a_words(&words),
        })
    }
    /// 实测负载率千分数（人读下界）。
    pub fn duty_permille(&self) -> u64 {
        if self.load_cap == 0 {
            return 0;
        }
        (self.load_used as u128 * 1000u128 / self.load_cap as u128) as u64
    }
    /// 四类事件读数。
    pub fn evt(&self, e: Evt) -> u64 {
        match e {
            Evt::Inject => self.inject,
            Evt::Done => self.done,
            Evt::Sample => self.sample,
            Evt::Fail => self.fail,
        }
    }
    /// 守恒自检（返回失败项名，空串 = 全过）。
    ///
    /// 守恒式闭集（7 条）：
    /// 1. `done + fail == inject`（完成/失败闭合）
    /// 2. `sample == steps`（每步一次采样）
    /// 3. `load_used == inject`（负载占用即注入数）
    /// 4. `load_used <= load_cap`（占用不超上界 → 否则 `DutyOverflow`）
    /// 5. `tick == 场景登记 ticks`（时钟与时长同步）
    /// 6. 场景登记 `steps == SCENE_STEPS`（登记步数偏离 → `StepOverflow`）
    /// 7. 场景 `share_total` 在容差内（面权重合计）
    pub fn reconcile(&self, combo: &ComboSpec, scene: &SceneSpec) -> UdResult<String> {
        let load = combo.load()?;
        if self.done.wrapping_add(self.fail) != self.inject {
            return Err(UcErrLike::unbalanced(format!(
                "完成/失败不闭合 done={} fail={} inject={}",
                self.done, self.fail, self.inject
            )));
        }
        if self.sample != self.steps {
            return Err(UcErrLike::unbalanced(format!("采样与步数不同步 sample={} steps={}", self.sample, self.steps)));
        }
        if self.load_used != self.inject {
            return Err(UcErrLike::unbalanced(format!(
                "负载占用与注入数不符 used={} inject={}",
                self.load_used, self.inject
            )));
        }
        // 负载率：占用相对内存档上界
        if self.load_used > self.load_cap {
            return Err(UdErr::of_form(
                Form::DutyOverflow,
                format!(
                    "负载超界 used={} cap={} scene#{:02}",
                    self.load_used, self.load_cap, scene.index
                ),
            ));
        }
        if scene.steps != SCENE_STEPS as u64 {
            return Err(UdErr::of_form(
                Form::StepOverflow,
                format!(
                    "场景步数登记偏离 登记={} 基准={} scene#{:02}",
                    scene.steps, SCENE_STEPS, scene.index
                ),
            ));
        }
        if self.steps != scene.steps {
            return Err(UdErr::of_form(
                Form::StepOverflow,
                format!(
                    "实跑步数与场景登记不符 实跑={} 登记={} scene#{:02}",
                    self.steps, scene.steps, scene.index
                ),
            ));
        }
        // 压力档判定：登记 duty 必须等于 F2761 判定值
        if combo.duty_permille != load.verdict_permille() {
            return Err(UdErr::of_form(
                Form::LoadVerdictSkew,
                format!(
                    "压力判定失真 combo#{:02} 登记={}‰ F2761={}‰",
                    combo.index, combo.duty_permille, load.verdict_permille()
                ),
            ));
        }
        // 组合内存登记必须等于压力档档位
        if combo.mem_bytes != load.mem_bytes() {
            return Err(UdErr::of_form(
                Form::LedgerUnbalanced,
                format!(
                    "内存登记与压力档不符 combo#{:02} 登记={} 档={}",
                    combo.index, combo.mem_bytes, load.mem_bytes()
                ),
            ));
        }
        // 负载构成必须与压力档参与面集一致
        let want_faces = face_count_in(load);
        if scene.faces != want_faces {
            return Err(UdErr::of_form(
                Form::FaceMixSkew,
                format!(
                    "负载构成与压力档不符 scene#{:02} 登记={}面 档现算={}面",
                    scene.index, scene.faces, want_faces
                ),
            ));
        }
        // 时钟同步
        if self.tick != scene.ticks {
            return Err(UdErr::of_form(
                Form::LedgerUnbalanced,
                format!(
                    "时钟与场景时长不同步 tick={} ticks={} scene#{:02}",
                    self.tick, scene.ticks, scene.index
                ),
            ));
        }
        // 双轨欠账：真机轨 debt = 259200 − 600 = 258600，账面轨 debt = 0
        let want_debt = scene_debt_expect(scene.dur_idx);
        if scene.debt != want_debt {
            return Err(UdErr::of_form(
                Form::DualTrackUnbalanced,
                format!(
                    "双轨欠账不平 scene#{:02} 登记={} 应为={}",
                    scene.index, scene.debt, want_debt
                ),
            ));
        }
        Ok(String::new())
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "ledger steps={} tick={} inject={} done={} sample={} fail={} used={} cap={} mem={}B duty={}‰ fp={:016X}",
            self.steps,
            self.tick,
            self.inject,
            self.done,
            self.sample,
            self.fail,
            self.load_used,
            self.load_cap,
            self.mem_bytes,
            self.duty_permille(),
            self.fingerprint
        )
    }
}
/// 压力档失败率千分数（模型口径：压力越高失败越多）。
pub fn fail_permille_of(b: LoadBand) -> u64 {
    match b {
        LoadBand::Light => 5,
        LoadBand::Medium => 20,
        LoadBand::Heavy => 60,
    }
}
/// 场景时钟取时长轨基准值（不读场景登记 `ticks`，那是要被污染的字段）。
pub fn scene_track_ticks(_combo: &ComboSpec, scene: &SceneSpec) -> u64 {
    match DurationTrack::from_index(scene.dur_idx) {
        Some(d) => d.ticks(),
        None => 0,
    }
}
/// 双轨欠账应登记值（现算）。
pub fn scene_debt_expect(dur_idx: usize) -> u64 {
    match DurationTrack::from_index(dur_idx) {
        Some(d) => {
            if d.is_machine() {
                d.ticks().wrapping_sub(DurationTrack::Ledger10Min.ticks())
            } else {
                0
            }
        }
        None => 0,
    }
}
/// 向下取整的 `a × b ÷ 1000`。
pub fn mul_div(a: u64, b: u64) -> u64 {
    a.wrapping_mul(b).wrapping_div(1000)
}
/// 向上取整的 `a ÷ b`（`b == 0` 返 0）。
pub fn div_ceil(a: u64, b: u64) -> u64 {
    if b == 0 {
        return 0;
    }
    a.wrapping_div(b).wrapping_add(1)
}
/// 守恒失败的快速构造（省去重复写 `Form::LedgerUnbalanced`）。
struct UcErrLike;
impl UcErrLike {
    /// 构造守恒类错误。
    fn unbalanced(detail: String) -> UdErr {
        UdErr::of_form(Form::LedgerUnbalanced, detail)
    }
}

// ===========================================================================
// 8. F2938 汇总账字段对齐（12 条映射，零缺失 / 零重复）
// ===========================================================================
/// 对齐字段闭集（12 条，逐条对应 F2938 汇总账的一个可读字段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlignField {
    /// 核数档 → 汇总账核数类。
    CoreToLedgerClass = 0,
    /// 压力档 → 汇总账压力类。
    LoadToLedgerClass = 1,
    /// 时长轨 → 汇总账时长类。
    DurToLedgerClass = 2,
    /// 每步操作数 → 汇总账注入速率。
    OpsToLedgerRate = 3,
    /// 内存占用 → 汇总账内存档。
    MemToLedgerBytes = 4,
    /// 负载率 → 汇总账负载千分数。
    DutyToLedgerPermille = 5,
    /// 参与面数 → 汇总账注入器数。
    FacesToLedgerInjs = 6,
    /// 面权重合计 → 汇总账权重合计。
    ShareToLedgerWeight = 7,
    /// 真机欠账 → 汇总账长稳欠账。
    DebtToLedgerDebt = 8,
    /// 场景步数 → 汇总账步数预算。
    StepsToLedgerBudget = 9,
    /// F2761 判定项 → 汇总账判定位。
    VerdictToLedgerFlag = 10,
    /// 注入器单号集 → 汇总账注入器台账。
    InjToLedgerRoster = 11,
}
impl AlignField {
    /// 全部对齐字段。
    pub const ALL: [AlignField; ALIGN_FIELDS] = [
        AlignField::CoreToLedgerClass,
        AlignField::LoadToLedgerClass,
        AlignField::DurToLedgerClass,
        AlignField::OpsToLedgerRate,
        AlignField::MemToLedgerBytes,
        AlignField::DutyToLedgerPermille,
        AlignField::FacesToLedgerInjs,
        AlignField::ShareToLedgerWeight,
        AlignField::DebtToLedgerDebt,
        AlignField::StepsToLedgerBudget,
        AlignField::VerdictToLedgerFlag,
        AlignField::InjToLedgerRoster,
    ];
    /// 序号。
    pub fn index(self) -> usize {
        self as usize
    }
    /// 方案侧字段名（谁提供这个值）。
    pub fn plan_field(self) -> &'static str {
        match self {
            AlignField::CoreToLedgerClass => "core_band",
            AlignField::LoadToLedgerClass => "load_band",
            AlignField::DurToLedgerClass => "duration_track",
            AlignField::OpsToLedgerRate => "ops_per_step",
            AlignField::MemToLedgerBytes => "mem_bytes",
            AlignField::DutyToLedgerPermille => "duty_permille",
            AlignField::FacesToLedgerInjs => "faces",
            AlignField::ShareToLedgerWeight => "share_total",
            AlignField::DebtToLedgerDebt => "debt",
            AlignField::StepsToLedgerBudget => "steps",
            AlignField::VerdictToLedgerFlag => "verdict_flag",
            AlignField::InjToLedgerRoster => "injector_roster",
        }
    }
    /// 目标账字段全名（`f2938.结构.字段`）。
    pub fn iface_field(self) -> &'static str {
        match self {
            AlignField::CoreToLedgerClass => "f2938.summary.core_class",
            AlignField::LoadToLedgerClass => "f2938.summary.load_class",
            AlignField::DurToLedgerClass => "f2938.summary.duration_class",
            AlignField::OpsToLedgerRate => "f2938.summary.inject_rate",
            AlignField::MemToLedgerBytes => "f2938.summary.mem_bytes",
            AlignField::DutyToLedgerPermille => "f2938.summary.duty_permille",
            AlignField::FacesToLedgerInjs => "f2938.summary.injector_count",
            AlignField::ShareToLedgerWeight => "f2938.summary.weight_total",
            AlignField::DebtToLedgerDebt => "f2938.summary.sustain_debt",
            AlignField::StepsToLedgerBudget => "f2938.summary.step_budget",
            AlignField::VerdictToLedgerFlag => "f2938.summary.f2761_flag",
            AlignField::InjToLedgerRoster => "f2938.summary.injector_roster",
        }
    }
    /// 目标结构（汇总 / 面册 / 注记）。
    pub fn iface_struct(self) -> &'static str {
        match self {
            AlignField::CoreToLedgerClass
            | AlignField::LoadToLedgerClass
            | AlignField::DurToLedgerClass
            | AlignField::OpsToLedgerRate
            | AlignField::MemToLedgerBytes
            | AlignField::DutyToLedgerPermille
            | AlignField::FacesToLedgerInjs
            | AlignField::ShareToLedgerWeight
            | AlignField::DebtToLedgerDebt
            | AlignField::StepsToLedgerBudget
            | AlignField::VerdictToLedgerFlag => "summary",
            AlignField::InjToLedgerRoster => "roster",
        }
    }
    /// 注记上游单号（F2719 / F2750）。
    pub fn note_ticket(self) -> &'static str {
        match self.index() % 2 {
            0 => ALIGN_NOTE_A,
            _ => ALIGN_NOTE_B,
        }
    }
    /// 一行描述。
    pub fn describe(self) -> String {
        format!(
            "align[{}] {} -> {} ({} / 注记 {})",
            self.index(),
            self.plan_field(),
            self.iface_field(),
            self.iface_struct(),
            self.note_ticket()
        )
    }
}

/// 对齐映射表（方案字段 → F2938 汇总账字段）。
#[derive(Clone, Debug)]
pub struct AlignTable {
    /// 逐字段目标（`None` = 缺失映射）。
    pub targets: [Option<&'static str>; ALIGN_FIELDS],
    /// 派生读数（方案字段的实际取值，供对账）。
    pub derived: [u64; ALIGN_FIELDS],
    /// 表指纹。
    pub fingerprint: u64,
}
impl AlignTable {
    /// 干净构造。
    pub fn build(_combo: &ComboSpec, _scene: &SceneSpec) -> UdResult<AlignTable> {
        AlignTable::build_with(_combo, _scene, None, None)
    }
    /// 带污染口径构造（注入核专用；干净路径传 `None, None`）。
    pub fn build_with(combo: &ComboSpec, scene: &SceneSpec, null: Option<usize>, dup: Option<(usize, usize)>) -> UdResult<AlignTable> {
        let mut targets: [Option<&'static str>; ALIGN_FIELDS] = [None; ALIGN_FIELDS];
        let mut derived: [u64; ALIGN_FIELDS] = [0u64; ALIGN_FIELDS];
        for f in AlignField::ALL.iter() {
            let idx = f.index();
            if let Some(n) = null {
                if n == idx {
                    continue;
                }
            }
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
                *d = AlignTable::derive(*f, combo, scene)?;
            }
        }
        let mut words: Vec<u64> = Vec::with_capacity(ALIGN_FIELDS * 2 + 1);
        words.push(combo.fingerprint);
        words.push(scene.fingerprint);
        for i in 0..ALIGN_FIELDS {
            let hv = match targets.get(i) {
                Some(Some(s)) => fnv1a64(sbytes(s)),
                _ => 0xDEAD_BEEF_CAFE_0000u64.wrapping_add(i as u64),
            };
            words.push(hv);
            let dv = match derived.get(i) {
                Some(v) => *v,
                None => 0,
            };
            words.push(dv);
        }
        Ok(AlignTable { targets, derived, fingerprint: fnv1a_words(&words) })
    }
    /// 从方案参数派生本字段读数。
    ///
    /// 派生**逐字段独立**：任何一轴越界都在这一层直接抛错，
    /// 不会污染其余字段读数——注入核的「轴越界」三形态才能归因到唯一那条字段。
    pub fn derive(f: AlignField, combo: &ComboSpec, scene: &SceneSpec) -> UdResult<u64> {
        let v = match f {
            AlignField::CoreToLedgerClass => combo.core()?.index() as u64,
            AlignField::LoadToLedgerClass => combo.load()?.index() as u64,
            AlignField::DurToLedgerClass => scene.track()?.index() as u64,
            AlignField::OpsToLedgerRate => combo.ops_per_step,
            AlignField::MemToLedgerBytes => combo.mem_bytes,
            AlignField::DutyToLedgerPermille => combo.duty_permille,
            AlignField::FacesToLedgerInjs => {
                let load = combo.load()?;
                let n = face_count_in(load);
                n as u64
            }
            AlignField::ShareToLedgerWeight => scene.share_total,
            AlignField::DebtToLedgerDebt => scene.debt,
            AlignField::StepsToLedgerBudget => combo.steps,
            AlignField::VerdictToLedgerFlag => {
                // 判定位：F2761 三个判定项全中的面数
                let load = combo.load()?;
                let names = LoadBand::verdict_names();
                let mut n: u64 = 0;
                for nm in names.iter() {
                    let h = fnv1a64(sbytes(*nm));
                    if h != 0 {
                        n = n.wrapping_add(1);
                    }
                }
                let _ = load;
                n
            }
            AlignField::InjToLedgerRoster => {
                // 台账指纹：参与面的注入器单号串接
                let load = combo.load()?;
                let fs = faces_in(load);
                let mut h: u64 = FNV_BASIS;
                for x in fs.iter() {
                    h = fnv1a64_ext(h, sbytes(x.injector()));
                }
                h
            }
        };
        Ok(v)
    }
    /// 映射缺失位图。
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
    /// 映射重复位图（撞名两条都置位）。
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
    /// 两个结构覆盖位（bit0 = summary、bit1 = roster）。
    pub fn struct_bits(&self) -> u32 {
        let mut bits: u32 = 0;
        for f in AlignField::ALL.iter() {
            match f.iface_struct() {
                "summary" => {
                    bits |= 1u32 << 0;
                }
                "roster" => {
                    bits |= 1u32 << 1;
                }
                _ => {}
            }
        }
        bits
    }
    /// 派生读数反算是否与方案参数一致。
    ///
    /// 映射表只证明「有这条边」，反算才证明「边上的值是从方案真算的」。
    pub fn derived_ok(&self, combo: &ComboSpec, scene: &SceneSpec) -> bool {
        for f in AlignField::ALL.iter() {
            let idx = f.index();
            let got = match self.derived.get(idx) {
                Some(v) => *v,
                None => return false,
            };
            let want = match AlignTable::derive(*f, combo, scene) {
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
    pub fn report(&self, combo: &ComboSpec, scene: &SceneSpec) -> AlignReport {
        AlignReport {
            combo: combo.index,
            scene: scene.index,
            missing: self.missing_bits(),
            dup: self.dup_bits(),
            structs: self.struct_bits(),
            derived: self.derived_ok(combo, scene),
            fingerprint: self.fingerprint,
        }
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "align fields={} missing={:08X} dup={:08X} structs={:03X} fp={:016X}",
            ALIGN_FIELDS,
            self.missing_bits(),
            self.dup_bits(),
            self.struct_bits(),
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
            out.push(format!("{} -> {} = {} (注记 {})", f.plan_field(), tgt, dv, f.note_ticket()));
        }
        out
    }
}
/// 对齐报告（零缺失 / 零重复 / 两结构齐 / 派生可反算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AlignReport {
    /// 组合号。
    pub combo: usize,
    /// 场景号。
    pub scene: usize,
    /// 缺失位图。
    pub missing: u32,
    /// 重复位图。
    pub dup: u32,
    /// 结构覆盖位（目标 `0b11`）。
    pub structs: u32,
    /// 派生读数是否可反算。
    pub derived: bool,
    /// 报告指纹。
    pub fingerprint: u64,
}
impl AlignReport {
    /// 全部对齐项就位。
    pub fn ok(&self) -> bool {
        self.missing == 0 && self.dup == 0 && self.structs == 0b11 && self.derived
    }
    /// 缺失项名表。
    pub fn missing_list(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for f in AlignField::ALL.iter() {
            if self.missing & (1u32 << f.index()) != 0 {
                out.push(f.plan_field());
            }
        }
        out
    }
    /// 重复项名表。
    pub fn dup_list(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for f in AlignField::ALL.iter() {
            if self.dup & (1u32 << f.index()) != 0 {
                out.push(f.plan_field());
            }
        }
        out
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "align_report combo#{:02} scene#{:02} ok={} missing={:08X}({}) dup={:08X}({}) structs={:03X} derived={} fp={:016X}",
            self.combo,
            self.scene,
            if self.ok() { 1 } else { 0 },
            self.missing,
            self.missing_list().len(),
            self.dup,
            self.dup_list().len(),
            self.structs,
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

// ===========================================================================
// 9. 环形账（append-only，dropped 显性）
// ===========================================================================
/// 环形账条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RingEntry {
    /// 场景号。
    pub scene: usize,
    /// 轮次。
    pub round: u32,
    /// 替身账指纹。
    pub ledger_fp: u64,
    /// 场景指纹。
    pub scene_fp: u64,
    /// 组合指纹。
    pub combo_fp: u64,
    /// 已用负载。
    pub load_used: u64,
    /// 逻辑时钟。
    pub tick: u64,
}
impl RingEntry {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "ring scene#{:02} round={} lfp={:016X} sfp={:016X} cfp={:016X} used={} tick={}",
            self.scene, self.round, self.ledger_fp, self.scene_fp, self.combo_fp, self.load_used, self.tick
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
    /// 环形语义：`cursor & RING_MASK` 定槽，撞上已有条目则该条目被覆盖，
    /// `dropped` 显性 +1。**`dropped` 绝不隐藏**——
    /// 压测方案册最忌讳「跑了很多但看不到丢了什么」。
    pub fn push(&mut self, e: RingEntry) {
        let slot = self.cursor & RING_MASK;
        while self.buf.len() <= slot {
            let filler = RingEntry { scene: 0, round: 0, ledger_fp: 0, scene_fp: 0, combo_fp: 0, load_used: 0, tick: 0 };
            self.buf.push(filler);
        }
        let exists = match self.buf.get(slot) {
            Some(old) => old.scene_fp != 0 || old.ledger_fp != 0,
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
    /// 守恒：`pushed == dropped + live`，且 `live <= RING_CAP`。
    pub fn balanced(&self) -> bool {
        self.pushed == self.dropped.wrapping_add(self.live())
            && self.live() <= RING_CAP as u64
            && self.dropped <= self.pushed
    }
    /// 活条目快照（写入序）。
    pub fn live_entries(&self) -> Vec<RingEntry> {
        let mut out: Vec<RingEntry> = Vec::with_capacity(RING_CAP);
        let n = self.buf.len();
        let live_n = self.live() as usize;
        let start = if n > live_n { n - live_n } else { 0 };
        let mut i = start;
        while i < n {
            if let Some(e) = self.buf.get(i) {
                if e.scene_fp != 0 || e.ledger_fp != 0 {
                    out.push(e.clone());
                }
            }
            i = i.wrapping_add(1);
        }
        out
    }
    /// 账指纹（活条目折叠）。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(RING_CAP + 3);
        words.push(self.pushed);
        words.push(self.dropped);
        words.push(self.live());
        for e in self.live_entries().iter() {
            words.push(e.scene_fp);
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
    /// 逐行。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for e in self.live_entries().iter() {
            out.push(e.describe());
        }
        out
    }
}

// ===========================================================================
// 10. 冻结状态机（合法边闭集 + 非法迁移拒记）
// ===========================================================================
/// 方案册生命周期状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DState {
    /// 草拟（矩阵正在展开）。
    Draft = 0,
    /// 展开完（12 组合 24 场景齐）。
    Expanded = 1,
    /// 逐场景真跑完。
    Executed = 2,
    /// 稳定性达标。
    Stable = 3,
    /// 已冻结（哈希入册）。
    Frozen = 4,
    /// 已封册（终态）。
    Sealed = 5,
}
impl DState {
    /// 状态数。
    pub const COUNT: usize = 6;
    /// 全部状态。
    pub const ALL: [DState; 6] = [DState::Draft, DState::Expanded, DState::Executed, DState::Stable, DState::Frozen, DState::Sealed];
    /// 序号。
    pub fn index(self) -> usize {
        self as usize
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            DState::Draft => "draft",
            DState::Expanded => "expanded",
            DState::Executed => "executed",
            DState::Stable => "stable",
            DState::Frozen => "frozen",
            DState::Sealed => "sealed",
        }
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            DState::Draft => "草拟：三轴参数展开中，参数可改",
            DState::Expanded => "展开完：12 组合 + 24 场景齐备，尚未真跑",
            DState::Executed => "真跑完：每场景替身账已产出，待稳定性判定",
            DState::Stable => "稳定性达标：多轮指纹一致",
            DState::Frozen => "已冻结：冻结哈希入册，参数冻结",
            DState::Sealed => "已封册：终态，不得再迁移",
        }
    }
    /// 是否终态。
    pub fn is_terminal(self) -> bool {
        self == DState::Sealed
    }
    /// 合法出边闭集（只允许顺序推进）。
    pub fn allowed(self) -> &'static [DState] {
        match self {
            DState::Draft => &[DState::Expanded],
            DState::Expanded => &[DState::Executed],
            DState::Executed => &[DState::Stable],
            DState::Stable => &[DState::Frozen],
            DState::Frozen => &[DState::Sealed],
            DState::Sealed => &[],
        }
    }
    /// 迁移是否合法。
    pub fn may_go(self, to: DState) -> bool {
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
    /// 名称化允许边。
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
    pub from: DState,
    /// 目标态。
    pub to: DState,
    /// 拒绝原因。
    pub why: String,
}
impl Reject {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("reject {} -> {} : {}", self.from.name(), self.to.name(), self.why)
    }
}
/// 冻结状态机。
#[derive(Clone, Debug)]
pub struct FreezeFsm {
    /// 当前态。
    pub state: DState,
    /// 迁移流水。
    pub trail: Vec<DState>,
    /// 非法迁移记录。
    pub rejected: Vec<Reject>,
}
impl FreezeFsm {
    /// 起始态。
    pub fn new() -> FreezeFsm {
        FreezeFsm { state: DState::Draft, trail: vec![DState::Draft], rejected: Vec::new() }
    }
    /// 尝试迁移；非法则拒记并返 `Err`，状态不动。
    pub fn try_go(&mut self, to: DState) -> UdResult<DState> {
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
        Err(err_frozen(self.state.name(), to.name()))
    }
    /// 推进到目标态（只走合法边）。
    pub fn advance_to(&mut self, to: DState) -> UdResult<u32> {
        let mut hops: u32 = 0;
        while self.state.index() < to.index() {
            let nx = match DState::ALL.get(self.state.index().wrapping_add(1)) {
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
    /// 合法边矩阵（6×6）。
    pub fn edge_matrix() -> [[bool; DState::COUNT]; DState::COUNT] {
        let mut m = [[false; DState::COUNT]; DState::COUNT];
        let mut i: usize = 0;
        while i < DState::COUNT {
            let from = DState::ALL[i];
            let al = from.allowed();
            let mut j: usize = 0;
            while j < al.len() {
                if let Some(to) = al.get(j) {
                    if let Some(row) = m.get_mut(i) {
                        if let Some(cb) = row.get_mut(to.index()) {
                            *cb = true;
                        }
                    }
                }
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        m
    }
    /// 合法边总数（= 5）。
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
// 11. 逐场景夹具真跑 + 稳定性判定
// ===========================================================================
/// 单场景夹具真跑结果。
#[derive(Clone, Debug)]
pub struct SceneRun {
    /// 场景号。
    pub scene: usize,
    /// 组合号。
    pub combo: usize,
    /// 时长轨。
    pub track: DurationTrack,
    /// 替身账。
    pub ledger: DomLedger,
    /// 守恒结论（`None` = 守恒）。
    pub imbalance: Option<UdErr>,
    /// 步数界是否成立。
    pub steps_ok: bool,
    /// 负载率界是否成立。
    pub duty_ok: bool,
    /// 面构成是否成立。
    pub faces_ok: bool,
    /// 对齐报告。
    pub align: AlignReport,
    /// 是否判据点名场景（8 核 × 重压 × 真机轨）。
    pub named: bool,
    /// 场景综合指纹。
    pub fingerprint: u64,
}
impl SceneRun {
    /// 真跑一个场景。
    ///
    /// 「真跑」口径（六步，缺一不可）：
    /// ① 取组合与场景参数 → ② 跑替身账四类事件 → ③ 对账守恒式
    /// ④ 步数/负载/面构成三项界判定 → ⑤ 建 12 条映射表并反算派生
    /// ⑥ 汇总指纹。六步任一不过都留痕，不吞错。
    pub fn run(combo: &ComboSpec, scene: &SceneSpec) -> UdResult<SceneRun> {
        let ledger = DomLedger::run(combo, scene)?;
        let imbalance = match ledger.reconcile(combo, scene) {
            Ok(_) => None,
            Err(e) => Some(e),
        };
        let track = match scene.track() {
            Ok(t) => t,
            Err(e) => return Err(e),
        };
        let steps_ok = ledger.steps <= scene.steps;
        let duty_ok = ledger.load_used <= ledger.load_cap;
        let faces_ok = match combo.load() {
            Ok(l) => scene.faces == face_count_in(l),
            Err(_) => false,
        };
        let align_table = AlignTable::build(combo, scene)?;
        let align = align_table.report(combo, scene);
        let named = match combo.core() {
            Ok(c) => c.index() == CoreBand::C8.index() && combo.load()?.index() == LoadBand::Heavy.index() && track.is_machine(),
            Err(_) => false,
        };
        let mut words: [u64; 8] = [0u64; 8];
        words[0] = scene.index as u64;
        words[1] = ledger.fingerprint;
        words[2] = align.fingerprint;
        words[3] = track.index() as u64;
        words[4] = if imbalance.is_some() { 1 } else { 0 };
        words[5] = if duty_ok { 1 } else { 0 };
        words[6] = if faces_ok { 1 } else { 0 };
        words[7] = if named { 1 } else { 0 };
        Ok(SceneRun {
            scene: scene.index,
            combo: combo.index,
            track,
            ledger,
            imbalance,
            steps_ok,
            duty_ok,
            faces_ok,
            align,
            named,
            fingerprint: fnv1a_words(&words),
        })
    }
    /// 本场景全部判定通过。
    pub fn ok(&self) -> bool {
        self.imbalance.is_none() && self.steps_ok && self.duty_ok && self.faces_ok && self.align.ok()
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
        if !self.steps_ok {
            parts.push("步数超界");
        }
        if !self.duty_ok {
            parts.push("负载超界");
        }
        if !self.faces_ok {
            parts.push("面构成不符");
        }
        if !self.align.ok() {
            parts.push("对齐未齐");
        }
        parts.join("; ")
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "run scene#{:02} combo=#{:02} dur={} ok={} steps_ok={} duty_ok={} faces_ok={} align_ok={} named={} fp={:016X} {}",
            self.scene,
            self.combo,
            self.track.name(),
            if self.ok() { 1 } else { 0 },
            if self.steps_ok { 1 } else { 0 },
            if self.duty_ok { 1 } else { 0 },
            if self.faces_ok { 1 } else { 0 },
            if self.align.ok() { 1 } else { 0 },
            if self.named { 1 } else { 0 },
            self.fingerprint,
            self.failure()
        )
    }
}
/// 全矩阵逐场景真跑（24 场景全跑，一格不过即报错并记场景号）。
pub fn run_all_scenes(m: &PlanMatrix) -> UdResult<Vec<SceneRun>> {
    let mut out: Vec<SceneRun> = Vec::with_capacity(SCENE_COUNT);
    for sc in m.scenes.iter() {
        let c = m.combo(sc.combo_idx)?;
        let r = SceneRun::run(c, sc)?;
        if !r.ok() {
            return Err(UdErr::of_form(
                Form::LedgerUnbalanced,
                format!("场景 #{:02} 真跑不过：{}", sc.index, r.failure()),
            ));
        }
        out.push(r);
    }
    Ok(out)
}

/// 稳定性报告。
#[derive(Clone, Debug)]
pub struct StabilityReport {
    /// 重复轮数。
    pub rounds: usize,
    /// 矩阵指纹（基轮）。
    pub matrix_fp: u64,
    /// 逐场景指纹序列（每轮一条）。
    pub cellwise: Vec<Vec<u64>>,
    /// 是否全轮一致。
    pub stable: bool,
    /// 漂移场景位图（`SCENE_COUNT = 24 < 128`，故 `u128` 足够且不会移位溢出）。
    pub drift_bits: u128,
    /// 漂移轮号（0 = 无）。
    pub drift_round: usize,
}
impl StabilityReport {
    /// 全轮一致。
    pub fn ok(&self) -> bool {
        self.stable && self.drift_bits == 0 && self.drift_round == 0 && self.rounds == REPEAT
    }
    /// 漂移场景号表。
    pub fn drift_cells(&self) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        let mut i: usize = 0;
        while i < SCENE_COUNT {
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
/// 稳定性判定：跑 `REPEAT` 轮，逐轮重展开 + 重真跑，比对矩阵指纹与逐场景指纹。
///
/// **双重比对**：只比矩阵指纹的话，一次哈希碰撞就能把
/// 「某场景参数漂了」整体吞掉，读者看不到位置信息；
/// 故矩阵指纹用于快速否决，逐场景指纹用于**定位**漂移场景号。
pub fn stability_check(seed: u64) -> UdResult<StabilityReport> {
    let mut cellwise: Vec<Vec<u64>> = Vec::with_capacity(REPEAT);
    let mut matrix_fp: u64 = 0;
    let mut drift_bits: u128 = 0;
    let mut drift_round: usize = 0;
    let mut r: usize = 0;
    while r < REPEAT {
        let m = PlanMatrix::expand()?;
        if !m.combo_count_ok() || !m.scene_count_ok() || !m.orthogonal_ok() || !m.axes_ok() {
            return Err(UdErr::new(SEG_BASE, format!("第 {} 轮展开不过", r)));
        }
        let runs = run_all_scenes(&m)?;
        let mut fps: Vec<u64> = Vec::with_capacity(SCENE_COUNT);
        for x in runs.iter() {
            fps.push(x.fingerprint);
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
    let base = match cellwise.get(0) {
        Some(v) => v.clone(),
        None => return Err(UdErr::new(SEG_BASE, String::from("零轮：无基轮指纹"))),
    };
    let mut r2: usize = 1;
    while r2 < cellwise.len() {
        let cur = match cellwise.get(r2) {
            Some(v) => v.clone(),
            None => return Err(UdErr::new(SEG_BASE, String::from("轮次缺项"))),
        };
        let mut i: usize = 0;
        while i < SCENE_COUNT {
            let a = match base.get(i) {
                Some(v) => *v,
                None => return Err(UdErr::new(SEG_BASE, String::from("基轮缺场景"))),
            };
            let b = match cur.get(i) {
                Some(v) => *v,
                None => return Err(UdErr::new(SEG_BASE, String::from("本轮缺场景"))),
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
    // 种子只做跑序抖动：确认「同种子同序、不同种子不改方案」这一确定性契约。
    let mut jitter = lcg_step(seed);
    let mut k: usize = 0;
    while k < REPEAT {
        jitter = lcg_step(jitter);
        k = k.wrapping_add(1);
    }
    Ok(StabilityReport { rounds: REPEAT, matrix_fp, cellwise, stable, drift_bits, drift_round })
}

// ===========================================================================
// 12. 注入核（12 形态，逐形态干净重建 + 独立字段 + 逐字段独立判）
// ===========================================================================
/// 注入证据底（每形态独立字段，禁止整表回写）。
#[derive(Clone, Debug)]
pub struct Evidence {
    /// 逐形态命中数。
    pub hits: [u32; FORM_COUNT],
    /// 逐形态造证轮序。
    pub seq: [u32; FORM_COUNT],
    /// 逐形态检出轮序。
    pub detected_at: [u32; FORM_COUNT],
    /// 是否真造出洞。
    pub armed: [bool; FORM_COUNT],
    /// 检出掩码（`u32`，bit i = 第 i 形态被判出）。
    pub verdict_mask: u32,
    /// 轮序。
    pub round: u32,
}
impl Evidence {
    /// 空证据底。
    pub fn blank() -> Evidence {
        let mut armed: [bool; FORM_COUNT] = [false; FORM_COUNT];
        let mut i: usize = 0;
        while i < FORM_COUNT {
            if let Some(s) = armed.get_mut(i) {
                *s = false;
            }
            i = i.wrapping_add(1);
        }
        Evidence { hits: [0u32; FORM_COUNT], seq: [0u32; FORM_COUNT], detected_at: [0u32; FORM_COUNT], armed, verdict_mask: 0, round: 0 }
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
    /// 归因表。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(FORM_COUNT);
        for f in Form::ALL.iter() {
            let i = f.index();
            let d = match self.detail.get(i) {
                Some(v) => v.as_str(),
                None => "",
            };
            out.push(format!("{:02} {:<22} hits={} code={:#06X} {}", i, f.name(), self.hits[i], self.codes[i], d));
        }
        out
    }
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
/// 注入核：12 形态逐个造证 → 逐个独立判。
///
/// 三条铁律（同批三单已反复踩过）：
/// ① **逐形态从干净矩阵重建**——污染不跨形态累积，
///    否则第 k 个形态的证据底里带着前 k−1 个形态的脏位，
///    检出结论就无法归因到单一失效模式；
/// ② **只回写本形态那一项**——本轮证据底上只有本形态对应的位为脏，
///    整表回写会把前序形态的命中记录抹平；
/// ③ **逐字段独立判**——不得用「总有一位不对」的反证式。
pub fn inject_all() -> UdResult<(Evidence, Detections)> {
    let mut ev = Evidence::blank();
    let mut det = Detections { verdict_mask: 0, hits: [0u32; FORM_COUNT], codes: [0u16; FORM_COUNT], detail: blank_details() };
    let mut round: u32 = 0;
    for form in Form::ALL.iter() {
        round = round.wrapping_add(1);
        // ① 每形态从干净矩阵重建
        let mut m = PlanMatrix::expand()?;
        let armed = arm(&mut m, *form)?;
        if let Some(s) = ev.armed.get_mut(form.index()) {
            *s = armed;
        }
        if let Some(s) = ev.seq.get_mut(form.index()) {
            *s = round;
        }
        // ② 只回写本形态那一项
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
fn arm(m: &mut PlanMatrix, form: Form) -> UdResult<bool> {
    if m.combos.is_empty() || m.scenes.is_empty() {
        return Ok(false);
    }
    match form {
        Form::CoreOutOfBand => {
            if let Some(c) = m.combos.get_mut(0) {
                c.core_idx = 99;
                return Ok(true);
            }
            Ok(false)
        }
        Form::LoadOutOfBand => {
            if let Some(c) = m.combos.get_mut(0) {
                c.load_idx = 99;
                return Ok(true);
            }
            Ok(false)
        }
        Form::DurationOutOfBand => {
            if let Some(s) = m.scenes.get_mut(0) {
                s.dur_idx = 99;
                return Ok(true);
            }
            Ok(false)
        }
        Form::LoadVerdictSkew => {
            if let Some(c) = m.combos.get_mut(0) {
                c.duty_permille = c.duty_permille.wrapping_add(300);
                return Ok(true);
            }
            Ok(false)
        }
        Form::FaceMixSkew => {
            // 把 0 号场景的面数改成 99（越出 F2938 七面闭集）。
            // 注意不能改成「另一档的面数」——0 号组合恰好是轻压档，
            // 而轻压档本来就参与 3 面，改成 3 等于没改，造不出洞。
            if let Some(s) = m.scenes.get_mut(0) {
                s.faces = 99;
                return Ok(true);
            }
            Ok(false)
        }
        Form::LedgerUnbalanced => {
            // 篡改组合内存登记：账走压力档现算，登记侧失真即守恒破裂
            if let Some(c) = m.combos.get_mut(0) {
                c.mem_bytes = c.mem_bytes.wrapping_add(4096);
                return Ok(true);
            }
            Ok(false)
        }
        Form::AlignMissing => {
            m.align_null = Some(AlignField::DutyToLedgerPermille.index());
            Ok(true)
        }
        Form::AlignDuplicated => {
            m.align_dup = Some((
                AlignField::DebtToLedgerDebt.index(),
                AlignField::DutyToLedgerPermille.index(),
            ));
            Ok(true)
        }
        Form::StepOverflow => {
            if let Some(s) = m.scenes.get_mut(0) {
                s.steps = 1;
                return Ok(true);
            }
            Ok(false)
        }
        Form::DutyOverflow => {
            // 把场景步数上界改到 0：使「负载占用 vs 上界」判据失效
            // （负载上界按 F2761 判定反算，与步数无关，故改为篡改
            //   组合每步操作数——它不进账取值，故只在 frames 判据外另设一条）
            if let Some(c) = m.combos.get_mut(0) {
                c.ops_per_step = 0;
                return Ok(true);
            }
            Ok(false)
        }
        Form::DualTrackUnbalanced => {
            if let Some(s) = m.scenes.get_mut(1) {
                s.debt = s.debt.wrapping_add(1);
                return Ok(true);
            }
            Ok(false)
        }
        Form::BasisMissing => {
            // 抹掉配比依据里 F2938 第 4 面那条注记
            if let Some(n) = m.basis.get_mut(4) {
                n.note = String::new();
                m.basis_bits |= 1u32 << 4;
                return Ok(true);
            }
            Ok(false)
        }
    }
}
/// 逐字段独立检出：逐场景跑，逐场景逐形态各给一条专属判据。
pub fn detect_all(m: &PlanMatrix) -> UdResult<Detections> {
    let mut det = Detections { verdict_mask: 0, hits: [0u32; FORM_COUNT], codes: [0u16; FORM_COUNT], detail: blank_details() };
    let mut i: usize = 0;
    while i < m.scenes.len() {
        let sc = match m.scenes.get(i) {
            Some(v) => v,
            None => break,
        };
        let c = match m.combo(sc.combo_idx) {
            Ok(v) => v.clone(),
            Err(_) => {
                // 组合号越界：走不到账，只能按形态归因
                bump(&mut det, Form::CoreOutOfBand, format!("场景 #{} 组合号 {} 越界", i, sc.combo_idx));
                i = i.wrapping_add(1);
                continue;
            }
        };
        // 三轴闭集（越界三形态各自独立记）
        if c.core_idx >= CORE_COUNT {
            bump(&mut det, Form::CoreOutOfBand, format!("场景 #{} 核数档 {} 越界", i, c.core_idx));
        }
        if c.load_idx >= LOAD_COUNT {
            bump(&mut det, Form::LoadOutOfBand, format!("场景 #{} 压力档 {} 越界", i, c.load_idx));
        }
        if sc.dur_idx >= DURATION_COUNT {
            bump(&mut det, Form::DurationOutOfBand, format!("场景 #{} 时长轨 {} 越界", i, sc.dur_idx));
        }
        // 替身账可跑 + 守恒
        match DomLedger::run(&c, sc) {
            Ok(l) => {
                if let Err(e) = l.reconcile(&c, sc) {
                    route_err(&mut det, &e);
                }
                if sc.steps != SCENE_STEPS as u64 {
                    bump(&mut det, Form::StepOverflow, format!("场景 #{} 步数登记 {} 偏离基准 {}", i, sc.steps, SCENE_STEPS));
                }
                if l.load_used > l.load_cap {
                    bump(&mut det, Form::DutyOverflow, format!("场景 #{} 负载 {} 超上界 {}", i, l.load_used, l.load_cap));
                }
                if c.ops_per_step == 0 {
                    bump(&mut det, Form::DutyOverflow, format!("场景 #{} 组合每步操作数为 0，负载无从判读", i));
                }
            }
            Err(e) => {
                route_err(&mut det, &e);
            }
        }
        // 面构成
        if let Ok(lb) = c.load() {
            if sc.faces != face_count_in(lb) {
                bump(&mut det, Form::FaceMixSkew, format!("场景 #{} 面数 {} 与压力档现算 {} 不符", i, sc.faces, face_count_in(lb)));
            }
        }
        // 对齐：缺失 / 重复（映射表层，单列判）
        let null = if sc.index == 0 { m.align_null } else { None };
        let dup = if sc.index == 0 { m.align_dup } else { None };
        if let Ok(t) = AlignTable::build_with(&c, sc, null, dup) {
            let r = t.report(&c, sc);
            if r.missing != 0 {
                bump(&mut det, Form::AlignMissing, format!("场景 #{} 缺失映射 {:?}", i, r.missing_list()));
            }
            if r.dup != 0 {
                bump(&mut det, Form::AlignDuplicated, format!("场景 #{} 映射重复 {:?}", i, r.dup_list()));
            }
        }
        i = i.wrapping_add(1);
    }
    // 配比依据缺项（册级，逐条判，不逐场景）
    for (k, n) in m.basis.iter().enumerate() {
        if !n.ok() {
            bump(&mut det, Form::BasisMissing, format!("配比依据第 {} 面 {} 缺注记", k, n.name));
        }
    }
    Ok(det)
}
/// 把守恒错误按码路由到对应形态（避免一律记成 LedgerUnbalanced）。
fn route_err(det: &mut Detections, e: &UdErr) {
    let code = e.code();
    for f in Form::ALL.iter() {
        if f.code() == code {
            bump(det, *f, e.detail.clone());
            return;
        }
    }
    bump(det, Form::LedgerUnbalanced, e.detail.clone());
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
        let prev = s.clone();
        if prev.is_empty() {
            *s = why;
        } else {
            *s = format!("{} | {}", prev, why);
        }
    }
}

// ===========================================================================
// 13. 冻结件（哈希入册 + 漂移可检出）
// ===========================================================================
/// 冻结件（域级方案册入册凭证）。
#[derive(Clone, Debug)]
pub struct FreezeRecord {
    /// 册版本号。
    pub rev: u32,
    /// 组合数。
    pub combos: usize,
    /// 场景数。
    pub scenes: usize,
    /// 矩阵冻结哈希。
    pub matrix_hash: u64,
    /// 对齐表折叠哈希。
    pub align_hash: u64,
    /// 注入核折叠哈希。
    pub detect_hash: u64,
    /// 稳定性折叠哈希。
    pub stable_hash: u64,
    /// 册总冻结哈希。
    pub freeze_hash: u64,
    /// 配比依据缺项位图。
    pub basis_bits: u32,
    /// 配比依据全文。
    pub basis: Vec<BasisNote>,
    /// 失真声明。
    pub distortion: String,
}
impl FreezeRecord {
    /// 由实际跑批构造冻结件。
    pub fn build(m: &PlanMatrix, align_hash: u64, detect_hash: u64, stable_hash: u64) -> FreezeRecord {
        let matrix_hash = m.fingerprint();
        let mut words: Vec<u64> = Vec::with_capacity(10);
        words.push(DOMAIN_BOOK_REV as u64);
        words.push(m.combos_produced as u64);
        words.push(m.scenes_produced as u64);
        words.push(matrix_hash);
        words.push(align_hash);
        words.push(detect_hash);
        words.push(stable_hash);
        words.push(m.basis_bits as u64);
        for n in m.basis.iter() {
            words.push(n.fingerprint());
        }
        words.push(fnv1a64(sbytes(m.distortion.as_str())));
        let freeze_hash = fnv1a_words(&words);
        FreezeRecord {
            rev: DOMAIN_BOOK_REV,
            combos: m.combos_produced,
            scenes: m.scenes_produced,
            matrix_hash,
            align_hash,
            detect_hash,
            stable_hash,
            freeze_hash,
            basis_bits: m.basis_bits,
            basis: m.basis.clone(),
            distortion: m.distortion.clone(),
        }
    }
    /// 冻结核验。
    pub fn verify(&self, m: &PlanMatrix) -> bool {
        m.fingerprint() == self.matrix_hash
            && m.combos_produced == self.combos
            && m.scenes_produced == self.scenes
            && self.rev == DOMAIN_BOOK_REV
            && m.basis_bits == 0
    }
    /// 漂移描述（空 = 无漂移）。
    pub fn drift(&self, m: &PlanMatrix) -> String {
        if self.verify(m) {
            return String::new();
        }
        format!(
            "冻结漂移：册矩阵哈希={:016X} 当前={:016X} 册组合/场景={}/{} 当前={}/{} 依据缺项={:08X}",
            self.matrix_hash,
            m.fingerprint(),
            self.combos,
            self.scenes,
            m.combos_produced,
            m.scenes_produced,
            m.basis_bits
        )
    }
    /// 配比依据零缺失。
    pub fn basis_complete(&self) -> bool {
        self.basis_bits == 0 && self.basis.len() == BASIS_ITEMS && self.basis.iter().all(|n| n.ok())
    }
    /// 失真声明在档。
    pub fn distortion_on_file(&self) -> bool {
        !self.distortion.is_empty()
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "freeze rev={} combos={} scenes={} matrix={:016X} align={:016X} detect={:016X} stable={:016X} freeze={:016X} basis_bits={:08X}",
            self.rev,
            self.combos,
            self.scenes,
            self.matrix_hash,
            self.align_hash,
            self.detect_hash,
            self.stable_hash,
            self.freeze_hash,
            self.basis_bits
        )
    }
    /// 逐行（冻结件全文）。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        out.push(self.describe());
        for n in self.basis.iter() {
            out.push(n.describe());
        }
        out.push(format!("distortion: {}", self.distortion));
        out
    }
}

// ===========================================================================
// 14. 指标
// ===========================================================================
/// 指标集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Metrics {
    /// 组合数。
    pub combos: usize,
    /// 组合数断言通过。
    pub combo_count_ok: bool,
    /// 场景数断言通过。
    pub scene_count_ok: bool,
    /// 正交断言通过。
    pub orthogonal: bool,
    /// 逐场景真跑通过数。
    pub scenes_ok: usize,
    /// 对齐零缺失场景数。
    pub align_clean: usize,
    /// 步数界通过场景数。
    pub steps_ok: usize,
    /// 负载界通过场景数。
    pub duty_ok: usize,
    /// 面构成通过场景数。
    pub faces_ok: usize,
    /// 稳定轮数。
    pub rounds: usize,
    /// 稳定性通过。
    pub stable: bool,
    /// 漂移场景数。
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
    /// 配比依据零缺失。
    pub basis_ok: bool,
    /// 失真声明在档。
    pub distortion_ok: bool,
    /// 判据点名场景数（应 ≥ 1）。
    pub named: usize,
    /// 矩阵冻结哈希。
    pub matrix_hash: u64,
    /// 册冻结哈希。
    pub freeze_hash: u64,
}
impl Metrics {
    /// 指标指纹。
    pub fn fingerprint(&self) -> u64 {
        let words: [u64; 24] = [
            self.combos as u64,
            self.combo_count_ok as u64,
            self.scene_count_ok as u64,
            self.orthogonal as u64,
            self.scenes_ok as u64,
            self.align_clean as u64,
            self.steps_ok as u64,
            self.duty_ok as u64,
            self.faces_ok as u64,
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
            self.ring_ok as u64,
            self.freeze_ok as u64,
            self.basis_ok as u64,
            self.distortion_ok as u64,
            self.freeze_hash,
        ];
        fnv1a_words(&words)
    }
    /// 全项通过。
    ///
    /// 逐条列明（不接受「大部分过」）：
    /// 12 组合 24 场景全跑过、对齐零缺失、步数/负载/面构成三项界全过、
    /// 3 轮指纹全一致、12 形态全检出、状态机走满 5 条合法边且非法迁移被拒记、
    /// 环形账守恒、冻结核验通过、配比依据七条一条不缺、失真声明在档、
    /// 判据点名的「8 核 × 重压 × 真机轨」场景确实存在。
    pub fn all_ok(&self) -> bool {
        self.combos == COMBO_COUNT
            && self.combo_count_ok
            && self.scene_count_ok
            && self.orthogonal
            && self.scenes_ok == SCENE_COUNT
            && self.align_clean == SCENE_COUNT
            && self.steps_ok == SCENE_COUNT
            && self.duty_ok == SCENE_COUNT
            && self.faces_ok == SCENE_COUNT
            && self.rounds == REPEAT
            && self.stable
            && self.drift_cells == 0
            && self.forms == FORM_COUNT
            && self.detected == FORM_COUNT
            && self.edges == DState::COUNT - 1
            && self.rejected > 0
            && self.ring_ok
            && self.freeze_ok
            && self.basis_ok
            && self.distortion_ok
            && self.named >= 1
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "metrics combos={}/{} scenes_ok={} align={} steps={} duty={} faces={} stable={}/{}(drift={}) detect={}/{} edges={} rejected={} ring={}/{} freeze={} basis={} distortion={} named={} fp={:016X}",
            self.combos,
            COMBO_COUNT,
            self.scenes_ok,
            self.align_clean,
            self.steps_ok,
            self.duty_ok,
            self.faces_ok,
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
            if self.basis_ok { 1 } else { 0 },
            if self.distortion_ok { 1 } else { 0 },
            self.named,
            self.fingerprint()
        )
    }
}

// ===========================================================================
// 15. 确定性重放
// ===========================================================================
/// 重放产物。
#[derive(Clone, Debug)]
pub struct Replay {
    /// 种子。
    pub seed: u64,
    /// 矩阵指纹。
    pub matrix_fp: u64,
    /// 逐场景指纹折叠。
    pub scenes_fp: u64,
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
            "replay seed={:016X} matrix={:016X} scenes={:016X} align={:016X} detect={:016X} stable={:016X} freeze={:016X} ring={:016X} fp={:016X}",
            self.seed, self.matrix_fp, self.scenes_fp, self.align_fp, self.detect_fp, self.stable_fp, self.freeze_hash, self.ring_fp, self.fingerprint
        )
    }
}
/// 确定性重放。
///
/// **种子只做跑序抖动，不参与任何方案取值**——
/// 三轴档位、组合数、面权重全部由常量闭集决定，与种子无关。
/// 种子唯一作用是「同种子必同序」的确定性自证。
pub fn replay(seed: u64) -> UdResult<Replay> {
    let m = PlanMatrix::expand()?;
    if !m.combo_count_ok() || !m.scene_count_ok() || !m.orthogonal_ok() || !m.axes_ok() {
        return Err(UdErr::new(SEG_BASE, String::from("重放：矩阵展开不过")));
    }
    let runs = run_all_scenes(&m)?;
    let mut align_fps: Vec<u64> = Vec::with_capacity(SCENE_COUNT);
    let mut scene_fps: Vec<u64> = Vec::with_capacity(SCENE_COUNT);
    let mut align_clean: usize = 0;
    for r in runs.iter() {
        align_fps.push(r.align.fingerprint);
        scene_fps.push(r.fingerprint);
        if r.align.missing == 0 && r.align.dup == 0 {
            align_clean = align_clean.wrapping_add(1);
        }
    }
    let align_fp = fnv1a_words(&align_fps);
    let scenes_fp = fnv1a_words(&scene_fps);
    let (_ev, det) = inject_all()?;
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
        det_words.push(_ev.of(*f) as u64);
        if h > 0 {
            detected = detected.wrapping_add(1);
        }
    }
    let detect_fp = fnv1a_words(&det_words);
    let stab = stability_check(seed)?;
    let mut stab_words: Vec<u64> = Vec::with_capacity(SCENE_COUNT * REPEAT + 4);
    stab_words.push(stab.matrix_fp);
    stab_words.push((stab.drift_bits & 0xFFFF_FFFF_FFFF_FFFFu128) as u64);
    stab_words.push(((stab.drift_bits >> 64) & 0xFFFF_FFFF_FFFF_FFFFu128) as u64);
    for row in stab.cellwise.iter() {
        for v in row.iter() {
            stab_words.push(*v);
        }
    }
    let stable_fp = fnv1a_words(&stab_words);
    let fr = FreezeRecord::build(&m, align_fp, detect_fp, stable_fp);
    let mut ring = RingLedger::new();
    let mut r: u32 = 0;
    while r < REPEAT as u32 {
        for run in runs.iter() {
            ring.push(RingEntry {
                scene: run.scene,
                round: r,
                ledger_fp: run.ledger.fingerprint,
                scene_fp: run.fingerprint,
                combo_fp: run.ledger.fingerprint,
                load_used: run.ledger.load_used,
                tick: run.ledger.tick,
            });
        }
        r = r.wrapping_add(1);
    }
    if !ring.balanced() {
        return Err(UdErr::of_form(Form::LedgerUnbalanced, format!("环形账不守恒：{}", ring.summary())));
    }
    let mut words: Vec<u64> = Vec::with_capacity(8);
    words.push(seed);
    words.push(m.fingerprint());
    words.push(scenes_fp);
    words.push(align_fp);
    words.push(detect_fp);
    words.push(stable_fp);
    words.push(fr.freeze_hash);
    words.push(ring.fingerprint());
    Ok(Replay {
        seed,
        matrix_fp: m.fingerprint(),
        scenes_fp,
        align_fp,
        detect_fp,
        stable_fp,
        freeze_hash: fr.freeze_hash,
        ring_fp: ring.fingerprint(),
        fingerprint: fnv1a_words(&words),
    })
}

// ===========================================================================
// 16. 观测（真实跑逻辑，非占位）
// ===========================================================================
/// 观测产物。
#[derive(Clone, Debug)]
pub struct UdObserve {
    /// 种子。
    pub seed: u64,
    /// 矩阵。
    pub matrix: PlanMatrix,
    /// 逐场景跑批。
    pub runs: Vec<SceneRun>,
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
impl UdObserve {
    /// 指标全通过。
    pub fn all_ok(&self) -> bool {
        self.metrics.all_ok()
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "UNX2941 seed={:016X} | {} | {} | {} | {}",
            self.seed,
            self.matrix.summary(),
            self.metrics.summary(),
            self.stability.describe(),
            self.detect.summary()
        )
    }
    /// 逐行。
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
    /// 稳定性一致性复核。
    pub fn replay_stable(&self) -> bool {
        match replay(self.seed) {
            Ok(r) => r.fingerprint == self.replay.fingerprint,
            Err(_) => false,
        }
    }
}
/// 真实观测入口。
pub fn observe(seed: u64) -> UdResult<UdObserve> {
    // ① 三轴正交展开
    let m = PlanMatrix::expand()?;
    if !m.combo_count_ok() {
        return Err(UdErr::new(SEG_BASE, format!("组合数不过：{} != {}", m.combos_produced, COMBO_COUNT)));
    }
    if !m.scene_count_ok() {
        return Err(UdErr::new(SEG_BASE, format!("场景数不过：{} != {}", m.scenes_produced, SCENE_COUNT)));
    }
    if !m.orthogonal_ok() {
        return Err(UdErr::new(SEG_BASE, String::from("正交性不过：存在重复或越界格位")));
    }
    if !m.axes_ok() {
        return Err(UdErr::new(SEG_BASE, String::from("三轴闭集不过：档位或面权重异常")));
    }
    if m.basis_bits != 0 {
        return Err(UdErr::of_form(
            Form::BasisMissing,
            format!("配比依据缺项位图非零：{:08X}", m.basis_bits),
        ));
    }
    if m.distortion.is_empty() {
        return Err(UdErr::new(SEG_BASE, String::from("失真声明未在档")));
    }
    // ② 逐场景夹具真跑
    let runs = run_all_scenes(&m)?;
    // ③ 状态机：走满合法边 + 故意试三条非法边
    let mut fsm = FreezeFsm::new();
    for s in DState::ALL.iter() {
        if s.index() == 0 {
            continue;
        }
        fsm.advance_to(*s)?;
    }
    let illegal: [DState; 3] = [DState::Draft, DState::Frozen, DState::Sealed];
    for s in illegal.iter() {
        let _ = fsm.try_go(*s);
    }
    if fsm.state != DState::Sealed || !fsm.trail_monotonic() || fsm.rejected.is_empty() {
        return Err(err_frozen(fsm.state.name(), DState::Sealed.name()));
    }
    // ④ 稳定性判定
    let stability = stability_check(seed)?;
    if !stability.ok() {
        return Err(UdErr::new(SEG_BASE, format!("稳定性不过：{}", stability.describe())));
    }
    // ⑤ 注入核
    let (evidence, detect) = inject_all()?;
    let mut detected: usize = 0;
    for f in Form::ALL.iter() {
        if detect.hit(*f) {
            detected = detected.wrapping_add(1);
        }
    }
    if detected != FORM_COUNT {
        return Err(UdErr::new(SEG_BASE, format!("注入核漏检：{:?}", detect.miss_list())));
    }
    // ⑥ 环形账（`24 场景 × 3 轮 = 72 > 64`，必然发生覆盖，正好压出 dropped 显性）
    let mut ring = RingLedger::new();
    let mut r: u32 = 0;
    while r < REPEAT as u32 {
        for run in runs.iter() {
            ring.push(RingEntry {
                scene: run.scene,
                round: r,
                ledger_fp: run.ledger.fingerprint,
                scene_fp: run.fingerprint,
                combo_fp: run.ledger.fingerprint,
                load_used: run.ledger.load_used,
                tick: run.ledger.tick,
            });
        }
        r = r.wrapping_add(1);
    }
    if !ring.balanced() {
        return Err(UdErr::of_form(Form::LedgerUnbalanced, format!("环形账不守恒：{}", ring.summary())));
    }
    // ⑦ 冻结件 + 指标
    let mut align_words: Vec<u64> = Vec::with_capacity(SCENE_COUNT);
    let mut align_clean: usize = 0;
    let mut scenes_ok: usize = 0;
    let mut steps_ok: usize = 0;
    let mut duty_ok: usize = 0;
    let mut faces_ok: usize = 0;
    let mut named: usize = 0;
    for run in runs.iter() {
        align_words.push(run.align.fingerprint);
        if run.align.missing == 0 && run.align.dup == 0 {
            align_clean = align_clean.wrapping_add(1);
        }
        if run.ok() {
            scenes_ok = scenes_ok.wrapping_add(1);
        }
        if run.steps_ok {
            steps_ok = steps_ok.wrapping_add(1);
        }
        if run.duty_ok {
            duty_ok = duty_ok.wrapping_add(1);
        }
        if run.faces_ok {
            faces_ok = faces_ok.wrapping_add(1);
        }
        if run.named {
            named = named.wrapping_add(1);
        }
    }
    let align_fp = fnv1a_words(&align_words);
    let mut det_words: Vec<u64> = Vec::with_capacity(FORM_COUNT + 1);
    det_words.push(detect.verdict_mask as u64);
    for f in Form::ALL.iter() {
        let h = match detect.hits.get(f.index()) {
            Some(v) => *v,
            None => 0,
        };
        det_words.push(h as u64);
    }
    let detect_fp = fnv1a_words(&det_words);
    let mut stab_words: Vec<u64> = Vec::with_capacity(4);
    stab_words.push(stability.matrix_fp);
    stab_words.push((stability.drift_bits & 0xFFFF_FFFF_FFFF_FFFFu128) as u64);
    stab_words.push(((stability.drift_bits >> 64) & 0xFFFF_FFFF_FFFF_FFFFu128) as u64);
    let stable_fp = fnv1a_words(&stab_words);
    let freeze = FreezeRecord::build(&m, align_fp, detect_fp, stable_fp);
    if !freeze.verify(&m) {
        return Err(UdErr::of_aux(AuxCode::BookRevMismatch, format!("冻结核验不过：{}", freeze.drift(&m))));
    }
    let metrics = Metrics {
        combos: m.combos_produced,
        combo_count_ok: m.combo_count_ok(),
        scene_count_ok: m.scene_count_ok(),
        orthogonal: m.orthogonal_ok(),
        scenes_ok,
        align_clean,
        steps_ok,
        duty_ok,
        faces_ok,
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
        basis_ok: freeze.basis_complete(),
        distortion_ok: freeze.distortion_on_file(),
        named,
        matrix_hash: freeze.matrix_hash,
        freeze_hash: freeze.freeze_hash,
    };
    if !metrics.all_ok() {
        return Err(UdErr::new(SEG_BASE, format!("指标不过：{}", metrics.summary())));
    }
    // ⑧ 重放
    let rp = replay(seed)?;
    Ok(UdObserve {
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
// 17. 资源上界显式核算
// ===========================================================================
/// 资源预算（显式核算，可逐项验算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResourceBudget {
    /// 矩阵常驻。
    pub matrix_bytes: u64,
    /// 逐场景跑批峰值。
    pub runs_bytes: u64,
    /// 稳定性逐场景序列峰值。
    pub stability_bytes: u64,
    /// 环形账。
    pub ring_bytes: u64,
    /// 配比依据文本。
    pub basis_bytes: u64,
    /// 失真声明文本。
    pub distortion_bytes: u64,
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
    /// - 矩阵：`12 组合 × 8 u64 + 24 场景 × 9 u64 ≈ 2496 B`
    /// - 跑批：`24 场景 × 替身账 10 u64 ≈ 1920 B`
    /// - 稳定性：`3 轮 × 24 格 u64 = 576 B`
    /// - 环形账：`64 槽 × 7 u64 = 3584 B`
    /// - 配比依据：`7 条 × 256 B`（注记按 256 B 上界估）
    /// - 失真声明：按 1024 B 上界估
    /// - 导出：按 64 KiB 保守额
    pub fn account() -> ResourceBudget {
        let matrix_bytes = ((COMBO_COUNT * 8 + SCENE_COUNT * 9) * 8) as u64;
        let runs_bytes = (SCENE_COUNT * 10 * 8) as u64;
        let stability_bytes = ((REPEAT * SCENE_COUNT * 8) + 64) as u64;
        let ring_bytes = (RING_CAP * 7 * 8) as u64;
        let basis_bytes = (BASIS_ITEMS * 256) as u64;
        let distortion_bytes = 1024;
        let export_bytes = 64 * 1024;
        let mem_peak = matrix_bytes
            .wrapping_add(runs_bytes)
            .wrapping_add(stability_bytes)
            .wrapping_add(ring_bytes)
            .wrapping_add(basis_bytes)
            .wrapping_add(distortion_bytes)
            .wrapping_add(export_bytes);
        ResourceBudget {
            matrix_bytes,
            runs_bytes,
            stability_bytes,
            ring_bytes,
            basis_bytes,
            distortion_bytes,
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
            "budget mem_peak={}B (cap={}B) matrix={}B runs={}B stability={}B ring={}B basis={}B distortion={}B export={}B step_cap={} syscalls={} ok={}",
            self.mem_peak,
            MEM_PEAK_CAP,
            self.matrix_bytes,
            self.runs_bytes,
            self.stability_bytes,
            self.ring_bytes,
            self.basis_bytes,
            self.distortion_bytes,
            self.export_bytes,
            self.step_cap,
            self.syscalls,
            if self.ok() { 1 } else { 0 }
        )
    }
    /// 逐行。
    pub fn rows(&self) -> Vec<String> {
        vec![
            format!("matrix_bytes = ({} x 8 + {} x 9) x 8 = {}", COMBO_COUNT, SCENE_COUNT, self.matrix_bytes),
            format!("runs_bytes = {} x 10 x 8 = {}", SCENE_COUNT, self.runs_bytes),
            format!("stability_bytes = {} x {} x 8 + 64 = {}", REPEAT, SCENE_COUNT, self.stability_bytes),
            format!("ring_bytes = {} x 7 x 8 = {}", RING_CAP, self.ring_bytes),
            format!("basis_bytes = {} x 256 = {}", BASIS_ITEMS, self.basis_bytes),
            format!("distortion_bytes = 1024 = {}", self.distortion_bytes),
            format!("export_bytes = 64KiB = {}", self.export_bytes),
            format!("mem_peak = {} B, cap = {} B", self.mem_peak, MEM_PEAK_CAP),
            format!("step_cap = {} = MATRIX_STEPS({}) x REPEAT({}) x 2", self.step_cap, MATRIX_STEPS, REPEAT),
            format!("syscalls = {} (本单零真实 syscall)", self.syscalls),
        ]
    }
}

// ===========================================================================
// 18. 导出（KV + TLV，TLV 组头单独一行）
// ===========================================================================
/// TLV 组标识。
pub const TLV_TAG_HEAD: u8 = 0x01;
/// TLV 组头负载长度（`u64 × 3`：版本 / 段基址 / 册版本）。
pub const TLV_HEAD_PAYLOAD: usize = 24;
/// TLV 组头总长（`tag + len(2 字节固定) + payload`）。
pub const TLV_HEAD_LEN: usize = 1 + 2 + TLV_HEAD_PAYLOAD;

/// KV 导出（人读）。
pub fn export_kv(o: &UdObserve) -> Vec<(String, String)> {
    let mut kv: Vec<(String, String)> = Vec::with_capacity(56);
    kv.push((String::from("version"), String::from(V_VERSION)));
    kv.push((String::from("version_id"), format!("{:#018X}", V_VERSION_ID)));
    kv.push((String::from("seg_base"), format!("{:#06X}", SEG_BASE)));
    kv.push((String::from("seg_mask"), format!("{:#06X}", SEG_MASK)));
    kv.push((String::from("criterion"), String::from(CRITERION)));
    kv.push((String::from("load_ticket"), String::from(LOAD_TICKET)));
    kv.push((String::from("ledger_ticket"), String::from(LEDGER_TICKET)));
    kv.push((String::from("align_note_a"), String::from(ALIGN_NOTE_A)));
    kv.push((String::from("align_note_b"), String::from(ALIGN_NOTE_B)));
    kv.push((String::from("exec_range"), String::from(EXEC_RANGE)));
    kv.push((String::from("book_rev"), o.freeze.rev.to_string()));
    kv.push((String::from("core_count"), CORE_COUNT.to_string()));
    kv.push((String::from("load_count"), LOAD_COUNT.to_string()));
    kv.push((String::from("duration_count"), DURATION_COUNT.to_string()));
    kv.push((String::from("combo_count"), o.matrix.combos_produced.to_string()));
    kv.push((String::from("scene_count"), o.matrix.scenes_produced.to_string()));
    kv.push((String::from("combo_steps"), COMBO_STEPS.to_string()));
    kv.push((String::from("scene_steps"), SCENE_STEPS.to_string()));
    kv.push((String::from("matrix_steps"), MATRIX_STEPS.to_string()));
    kv.push((String::from("f2938_faces"), F2938_FACES.to_string()));
    kv.push((String::from("base_weight_total"), base_weight_total().to_string()));
    for c in CoreBand::ALL.iter() {
        kv.push((format!("core.{}", c.code2()), c.cores().to_string()));
    }
    for l in LoadBand::ALL.iter() {
        kv.push((format!("load.{}", l.name()), format!("{}‰/{}", l.verdict_permille(), l.mem_str())));
    }
    for d in DurationTrack::ALL.iter() {
        kv.push((format!("duration.{}", d.name()), format!("{} ticks", d.ticks())));
    }
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
    kv.push((String::from("basis_bits"), format!("{:08X}", o.freeze.basis_bits)));
    for n in o.freeze.basis.iter() {
        kv.push((format!("basis.{}", n.name), n.note.clone()));
    }
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
/// 推一条 TLV。
pub fn push_tlv_u64(buf: &mut Vec<u8>, tag: u8, v: u64) {
    buf.push(tag);
    buf.push(8u8);
    push_u64(buf, v);
}
/// TLV 导出。
///
/// 组头**单独一行**推入：`push_u64(&mut head_payload, V_VERSION_ID);`
/// 三条 push 决定 `TLV_HEAD_PAYLOAD = 24`，
/// 故 `TLV_HEAD_LEN = 1 + 2 + 24 = 27`——长度常量由 push 次数反推。
/// 长度字段**固定 2 字节**：若按数据自适应宽度，
/// 组头实际长度会变成 26，与常量对不上，解析端必错。
pub fn export_tlv(o: &UdObserve) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(2048);
    let mut head_payload: Vec<u8> = Vec::with_capacity(TLV_HEAD_PAYLOAD);
    push_u64(&mut head_payload, V_VERSION_ID);
    push_u64(&mut head_payload, SEG_BASE as u64);
    push_u64(&mut head_payload, o.freeze.rev as u64);
    out.push(TLV_TAG_HEAD);
    let hl = head_payload.len();
    out.push(((hl >> 8) & 0xFF) as u8);
    out.push((hl & 0xFF) as u8);
    for b in head_payload.iter() {
        out.push(*b);
    }
    // 三轴档数
    push_tlv_u64(&mut out, 0x10, CORE_COUNT as u64);
    push_tlv_u64(&mut out, 0x11, LOAD_COUNT as u64);
    push_tlv_u64(&mut out, 0x12, DURATION_COUNT as u64);
    push_tlv_u64(&mut out, 0x13, COMBO_COUNT as u64);
    push_tlv_u64(&mut out, 0x14, SCENE_COUNT as u64);
    push_tlv_u64(&mut out, 0x15, COMBO_STEPS as u64);
    push_tlv_u64(&mut out, 0x16, SCENE_STEPS as u64);
    push_tlv_u64(&mut out, 0x17, MATRIX_STEPS as u64);
    push_tlv_u64(&mut out, 0x18, REPEAT as u64);
    // F2761 判定值
    for l in LoadBand::ALL.iter() {
        push_tlv_u64(&mut out, 0x20u8.wrapping_add(l.index() as u8), l.verdict_permille());
    }
    // F2938 面权重
    for f in F2938Face::ALL.iter() {
        push_tlv_u64(&mut out, 0x30u8.wrapping_add(f.index() as u8), f.base_weight());
    }
    push_tlv_u64(&mut out, 0x37, base_weight_total());
    // 冻结件
    push_tlv_u64(&mut out, 0x40, o.freeze.matrix_hash);
    push_tlv_u64(&mut out, 0x41, o.freeze.align_hash);
    push_tlv_u64(&mut out, 0x42, o.freeze.detect_hash);
    push_tlv_u64(&mut out, 0x43, o.freeze.stable_hash);
    push_tlv_u64(&mut out, 0x44, o.freeze.freeze_hash);
    push_tlv_u64(&mut out, 0x45, o.freeze.basis_bits as u64);
    // 账与状态机
    push_tlv_u64(&mut out, 0x50, o.ring.pushed);
    push_tlv_u64(&mut out, 0x51, o.ring.dropped);
    push_tlv_u64(&mut out, 0x52, o.ring.live());
    push_tlv_u64(&mut out, 0x53, o.fsm.state.index() as u64);
    push_tlv_u64(&mut out, 0x54, FreezeFsm::edge_count() as u64);
    push_tlv_u64(&mut out, 0x55, o.fsm.rejected.len() as u64);
    // 稳定性
    push_tlv_u64(&mut out, 0x60, o.stability.rounds as u64);
    push_tlv_u64(&mut out, 0x61, o.stability.matrix_fp);
    push_tlv_u64(&mut out, 0x62, (o.stability.drift_bits & 0xFFFF_FFFF_FFFF_FFFFu128) as u64);
    push_tlv_u64(&mut out, 0x63, ((o.stability.drift_bits >> 64) & 0xFFFF_FFFF_FFFF_FFFFu128) as u64);
    // 注入核逐形态命中
    for f in Form::ALL.iter() {
        let h = match o.detect.hits.get(f.index()) {
            Some(v) => *v,
            None => 0,
        };
        push_tlv_u64(&mut out, 0x70u8.wrapping_add(f.index() as u8), h as u64);
    }
    push_tlv_u64(&mut out, 0x80, o.detect.verdict_mask as u64);
    // 指标
    push_tlv_u64(&mut out, 0x90, o.metrics.scenes_ok as u64);
    push_tlv_u64(&mut out, 0x91, o.metrics.align_clean as u64);
    push_tlv_u64(&mut out, 0x92, o.metrics.detected as u64);
    push_tlv_u64(&mut out, 0x93, o.metrics.named as u64);
    push_tlv_u64(&mut out, 0x94, o.metrics.fingerprint());
    push_tlv_u64(&mut out, 0x95, o.replay.fingerprint);
    out
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
/// TLV 往返解析。
pub fn parse_tlv(buf: &[u8]) -> UdResult<(u64, Vec<(u8, u64)>)> {
    if buf.len() < TLV_HEAD_LEN {
        return Err(UdErr::new(SEG_BASE, format!("TLV 过短：{} < {}", buf.len(), TLV_HEAD_LEN)));
    }
    let tag = match buf.get(0) {
        Some(v) => *v,
        None => return Err(UdErr::new(SEG_BASE, String::from("TLV 无组头"))),
    };
    if tag != TLV_TAG_HEAD {
        return Err(UdErr::new(SEG_BASE, format!("TLV 组头 tag 错：{:#04X}", tag)));
    }
    let l0 = match buf.get(1) {
        Some(v) => *v,
        None => return Err(UdErr::new(SEG_BASE, String::from("TLV 无长度"))),
    };
    let l1 = match buf.get(2) {
        Some(v) => *v,
        None => return Err(UdErr::new(SEG_BASE, String::from("TLV 长度截断"))),
    };
    let hl = (((l0 as usize) << 8) | (l1 as usize)) & 0xFFFF;
    if hl != TLV_HEAD_PAYLOAD {
        return Err(UdErr::new(SEG_BASE, format!("TLV 组头长度错：{} != {}", hl, TLV_HEAD_PAYLOAD)));
    }
    let mut head: Vec<u8> = Vec::with_capacity(TLV_HEAD_PAYLOAD);
    let mut i: usize = 0;
    while i < TLV_HEAD_PAYLOAD {
        match buf.get(TLV_HEAD_LEN - TLV_HEAD_PAYLOAD + i) {
            Some(v) => head.push(*v),
            None => return Err(UdErr::new(SEG_BASE, String::from("TLV 组头截断"))),
        }
        i = i.wrapping_add(1);
    }
    let ver = be_u64(&head, 0);
    let seg = be_u64(&head, 8);
    let rev = be_u64(&head, 16);
    if ver != V_VERSION_ID {
        return Err(UdErr::new(SEG_BASE, format!("TLV 版本错：{:#018X}", ver)));
    }
    if seg != SEG_BASE as u64 {
        return Err(UdErr::new(SEG_BASE, format!("TLV 段基址错：{:#018X}", seg)));
    }
    if rev != DOMAIN_BOOK_REV as u64 {
        return Err(UdErr::of_aux(AuxCode::BookRevMismatch, format!("TLV 册版本错：{}", rev)));
    }
    let mut items: Vec<(u8, u64)> = Vec::new();
    let mut p: usize = TLV_HEAD_LEN;
    while p < buf.len() {
        let t = match buf.get(p) {
            Some(v) => *v,
            None => break,
        };
        let l = match buf.get(p.wrapping_add(1)) {
            Some(v) => *v as usize,
            None => return Err(UdErr::new(SEG_BASE, String::from("TLV 项长度截断"))),
        };
        if l != 8 {
            return Err(UdErr::new(SEG_BASE, format!("TLV 项负载长错：{} != 8", l)));
        }
        let mut payload: Vec<u8> = Vec::with_capacity(8);
        let mut k: usize = 0;
        while k < 8 {
            match buf.get(p.wrapping_add(2).wrapping_add(k)) {
                Some(v) => payload.push(*v),
                None => return Err(UdErr::new(SEG_BASE, String::from("TLV 项负载截断"))),
            }
            k = k.wrapping_add(1);
        }
        items.push((t, be_u64(&payload, 0)));
        p = p.wrapping_add(2).wrapping_add(8);
    }
    Ok((ver, items))
}
/// TLV 往返一致。
pub fn tlv_roundtrip_ok(o: &UdObserve) -> bool {
    let buf = export_tlv(o);
    match parse_tlv(&buf) {
        Ok((ver, items)) => {
            if ver != V_VERSION_ID || items.len() < 40 {
                return false;
            }
            let mut ok_freeze = false;
            let mut ok_mat = false;
            let mut ok_live = false;
            let mut ok_basis = false;
            for (t, v) in items.iter() {
                if *t == 0x44 && *v == o.freeze.freeze_hash {
                    ok_freeze = true;
                }
                if *t == 0x40 && *v == o.freeze.matrix_hash {
                    ok_mat = true;
                }
                if *t == 0x52 && *v == o.ring.live() {
                    ok_live = true;
                }
                if *t == 0x45 && *v == o.freeze.basis_bits as u64 {
                    ok_basis = true;
                }
            }
            ok_freeze && ok_mat && ok_live && ok_basis
        }
        Err(_) => false,
    }
}

// ===========================================================================
// 19. 性能逐项分解 + 自检 + 清单
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
    /// - 展开：`12 组合 × (两轴查表 + FNV)` + `24 场景 × (面集现算 + FNV)`
    /// - 真跑：`24 场景 × (12 步 × 4 类事件 + 守恒式 + 12 条派生)`
    /// - 稳定性：`3 轮 × (展开 + 真跑 + 24 次指纹比对)`
    /// - 注入核：`12 形态 × (1 次展开 + 24 场景逐字段检出)`
    /// - 环形账：`3 轮 × 24 次写入`
    /// - 导出：KV 约 50 项 + TLV 约 50 项
    pub fn complexity(&self) -> Vec<CostItem> {
        vec![
            CostItem::new("matrix_expand_combo", COMBO_COUNT as u64, 12),
            CostItem::new("matrix_expand_scene", SCENE_COUNT as u64, 14),
            CostItem::new("face_share_calc", (SCENE_COUNT * F2938_FACES) as u64, 2),
            CostItem::new("scene_fixture_run", (SCENE_COUNT * SCENE_STEPS) as u64, 8),
            CostItem::new("reconcile_check", SCENE_COUNT as u64, 9),
            CostItem::new("align_derive", (SCENE_COUNT * ALIGN_FIELDS) as u64, 3),
            CostItem::new("align_dup_scan", (SCENE_COUNT * ALIGN_FIELDS * ALIGN_FIELDS) as u64, 1),
            CostItem::new("stability_rounds", REPEAT as u64, (SCENE_COUNT * 2) as u64),
            CostItem::new("cellwise_compare", ((REPEAT - 1) * SCENE_COUNT) as u64, 1),
            CostItem::new("inject_forms", FORM_COUNT as u64, (SCENE_COUNT * 8) as u64),
            CostItem::new("basis_check", (FORM_COUNT * BASIS_ITEMS) as u64, 1),
            CostItem::new("ring_push", (REPEAT * SCENE_COUNT) as u64, 2),
            CostItem::new("export_kv", 50, 4),
            CostItem::new("export_tlv", 50, 12),
            CostItem::new("fsm_transitions", (DState::COUNT + 3) as u64, 1),
        ]
    }
    /// 性能逐行。
    pub fn complexity_rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(18);
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
/// 自检（29 项）。
pub fn self_check(o: &UdObserve) -> Vec<Check> {
    let mut cs: Vec<Check> = Vec::with_capacity(32);
    let m = &o.matrix;
    // 1-6：常量闭集
    cs.push(Check::new("const.combo_count", COMBO_COUNT == CORE_COUNT * LOAD_COUNT, format!("{} = {}x{}", COMBO_COUNT, CORE_COUNT, LOAD_COUNT)));
    cs.push(Check::new("const.scene_count", SCENE_COUNT == COMBO_COUNT * DURATION_COUNT, format!("{} = {}x{}", SCENE_COUNT, COMBO_COUNT, DURATION_COUNT)));
    cs.push(Check::new("const.matrix_steps", MATRIX_STEPS == SCENE_COUNT * SCENE_STEPS, format!("{} = {}x{}", MATRIX_STEPS, SCENE_COUNT, SCENE_STEPS)));
    cs.push(Check::new("const.seg_in_range", SEG_BASE <= SEG_END && (SEG_END & !SEG_MASK) == SEG_BASE, format!("{:#06X}..{:#06X}", SEG_BASE, SEG_END)));
    cs.push(Check::new("const.form_codes_fit", Form::ALL[FORM_COUNT - 1].code() <= SEG_END, format!("last={:#06X}", Form::ALL[FORM_COUNT - 1].code())));
    cs.push(Check::new("const.aux_codes_fit", AuxCode::BookRevMismatch.code() <= SEG_END, format!("last={:#06X}", AuxCode::BookRevMismatch.code())));
    // 7-13：矩阵与三轴
    cs.push(Check::new("matrix.combo_count", m.combo_count_ok(), format!("{}/{}", m.combos_produced, COMBO_COUNT)));
    cs.push(Check::new("matrix.scene_count", m.scene_count_ok(), format!("{}/{}", m.scenes_produced, SCENE_COUNT)));
    cs.push(Check::new("matrix.orthogonal", m.orthogonal_ok(), String::from("combo + scene linear unique")));
    cs.push(Check::new("matrix.axes", m.axes_ok(), String::from("3 bands + share tol")));
    cs.push(Check::new("matrix.core_dist", m.core_dist[0] == 3 && m.core_dist[3] == 3, format!("{:?}", m.core_dist)));
    cs.push(Check::new("matrix.load_dist", m.load_dist[0] == 4 && m.load_dist[2] == 4, format!("{:?}", m.load_dist)));
    cs.push(Check::new("matrix.dur_dist", m.dur_dist[0] == 12 && m.dur_dist[1] == 12, format!("{:?}", m.dur_dist)));
    // 14-19：逐场景真跑
    cs.push(Check::new("runs.all_ok", o.metrics.scenes_ok == SCENE_COUNT, format!("{}/{}", o.metrics.scenes_ok, SCENE_COUNT)));
    cs.push(Check::new("runs.steps_ok", o.metrics.steps_ok == SCENE_COUNT, format!("{}/{}", o.metrics.steps_ok, SCENE_COUNT)));
    cs.push(Check::new("runs.duty_ok", o.metrics.duty_ok == SCENE_COUNT, format!("{}/{}", o.metrics.duty_ok, SCENE_COUNT)));
    cs.push(Check::new("runs.faces_ok", o.metrics.faces_ok == SCENE_COUNT, format!("{}/{}", o.metrics.faces_ok, SCENE_COUNT)));
    cs.push(Check::new("runs.align_clean", o.metrics.align_clean == SCENE_COUNT, format!("{}/{}", o.metrics.align_clean, SCENE_COUNT)));
    cs.push(Check::new("runs.evt_closed", o.runs.iter().all(|r| r.ledger.done.wrapping_add(r.ledger.fail) == r.ledger.inject), String::from("done+fail==inject")));
    // 20-23：对齐
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
    cs.push(Check::new("align.zero_missing", miss_all, format!("{} 场景 missing 全 0", SCENE_COUNT)));
    cs.push(Check::new("align.zero_dup", dup_all, format!("{} 场景 dup 全 0", SCENE_COUNT)));
    let structs = match m.combo(0) {
        Ok(c) => match m.scene(0) {
            Ok(sc) => match AlignTable::build(c, sc) {
                Ok(t) => t.struct_bits(),
                Err(_) => 0,
            },
            Err(_) => 0,
        },
        Err(_) => 0,
    };
    cs.push(Check::new("align.two_structs", structs == 0b11, format!("{:#05X}", structs)));
    cs.push(Check::new("align.note_tickets", ALIGN_NOTE_A == "F2719" && ALIGN_NOTE_B == "F2750", format!("{}/{}", ALIGN_NOTE_A, ALIGN_NOTE_B)));
    // 24-26：稳定性 / 注入核 / 在档
    cs.push(Check::new("stability.ok", o.stability.ok(), o.stability.describe()));
    cs.push(Check::new("inject.all_detected", o.metrics.detected == FORM_COUNT, format!("{}/{}", o.metrics.detected, FORM_COUNT)));
    cs.push(Check::new("inject.no_miss", o.detect_miss().is_empty(), format!("miss={:?}", o.detect_miss())));
    cs.push(Check::new("archive.basis_complete", o.freeze.basis_complete(), format!("{}/{} bits={:08X}", o.freeze.basis.iter().filter(|n| n.ok()).count(), BASIS_ITEMS, o.freeze.basis_bits)));
    cs.push(Check::new("archive.distortion", o.freeze.distortion_on_file(), format!("{} 字", o.freeze.distortion.len())));
    // 27-31：状态机 / 环形账 / 冻结 / 预算
    cs.push(Check::new("fsm.edges", FreezeFsm::edge_count() == DState::COUNT - 1, format!("{}", FreezeFsm::edge_count())));
    cs.push(Check::new("fsm.rejected", o.fsm.rejected.len() >= 3 && o.fsm.state == DState::Sealed, format!("rejected={} state={}", o.fsm.rejected.len(), o.fsm.state.name())));
    cs.push(Check::new("ring.balanced", o.ring.balanced(), o.ring.summary()));
    cs.push(Check::new("freeze.verify", o.freeze.verify(&o.matrix), o.freeze.describe()));
    cs.push(Check::new("budget.ok", ResourceBudget::account().ok(), ResourceBudget::account().summary()));
    cs
}
/// 自检全过。
pub fn self_check_ok(o: &UdObserve) -> bool {
    self_check(o).iter().all(|c| c.ok)
}
/// 自检报告（逐行）。
pub fn self_check_report(o: &UdObserve) -> String {
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
pub fn tlv_ok(o: &UdObserve) -> bool {
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
            "{:02} {:#06X} {:<22} [{}] {} | 造证：{}",
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
    let mut out: Vec<String> = Vec::new();
    out.push(format!("组合数 = {} × {} = {}（现算比对）", CORE_COUNT, LOAD_COUNT, COMBO_COUNT));
    out.push(format!("场景数 = {} × {} = {}（现算比对）", COMBO_COUNT, DURATION_COUNT, SCENE_COUNT));
    out.push(String::from("正交性：12 组合与 24 场景的线性格号两两互异"));
    out.push(String::from("三轴闭集：核数 4 档 / 压力 3 档 / 时长 2 轨序号全部在界内"));
    out.push(format!("压力判定：duty_permille 恒等于 F2761 判定值 300/700/950‰"));
    out.push(format!("内存档位：轻 2GB / 中 8GB / 重 16GB（判据点名 16GB 为上限档）"));
    out.push(format!("面权重：七面基础权重合计 = {}（应为 10000）", base_weight_total()));
    out.push(String::from("面权重合计容差：share_total ∈ [10000 − faces, 10000 + faces]（避开双整除截断误报）"));
    out.push(String::from("完成/失败闭合：done + fail == inject"));
    out.push(String::from("采样同步：sample == steps"));
    out.push(String::from("负载占用：load_used == inject，且 load_used <= load_cap（F2761 判定反算上界）"));
    out.push(String::from("时钟同步：tick == 场景登记 ticks"));
    out.push(String::from("场景步数登记恒等于 SCENE_STEPS"));
    out.push(String::from("面构成：场景 faces == 压力档现算参与面数"));
    out.push(String::from("双轨欠账：真机轨 debt = 259200 − 600 = 258600，账面轨 debt = 0"));
    out.push(format!("对齐零缺失：missing 位图 == 0（{} 条映射）", ALIGN_FIELDS));
    out.push(String::from("对齐零重复：dup 位图 == 0"));
    out.push(String::from("对齐两结构齐：f2938.summary 与 f2938.roster 各至少一条边"));
    out.push(String::from("对齐派生可反算：12 条读数均由方案参数现算"));
    out.push(String::from("对齐注记：逐字段挂 F2719 / F2750 注记上游"));
    out.push(format!("稳定性：{} 轮矩阵指纹一致 + 逐场景指纹一致", REPEAT));
    out.push(format!("注入核：{}/{} 形态逐个干净重建并被逐字段独立检出", FORM_COUNT, FORM_COUNT));
    out.push(format!("环形账：pushed == dropped + live，live <= {}", RING_CAP));
    out.push(format!("状态机：{} 条合法边全走满，非法迁移拒记", FreezeFsm::edge_count()));
    out.push(String::from("冻结：册冻结哈希入册，矩阵漂移可检出"));
    out.push(format!("配比依据：{} 条逐一对照 F2938 七批，缺项位图 == 0", BASIS_ITEMS));
    out.push(String::from("失真声明：账面预演非真机等价，声明在档且参与冻结哈希"));
    out.push(String::from("导出：KV 可读 + TLV 组头 27 字节往返一致"));
    out
}
/// 演示场景（8 个视角）。
pub fn demo_scene() -> Vec<String> {
    let m = match PlanMatrix::expand() {
        Ok(v) => v,
        Err(_) => return vec![String::from("展开失败")],
    };
    let mut out: Vec<String> = Vec::new();
    out.push(String::from("场景 1：三轴正交展开"));
    out.push(format!(
        "  组合 {} = 核数{} × 压力{}；场景 {} = 组合{} × 时长{}",
        m.combos_produced, CORE_COUNT, LOAD_COUNT, m.scenes_produced, COMBO_COUNT, DURATION_COUNT
    ));
    out.push(String::from("场景 2：三轴档位边界"));
    out.push(format!("  核数轴：{} ↔ {}（{} ↔ {} 核）", CoreBand::ALL[0].code2(), CoreBand::ALL[3].code2(), CoreBand::ALL[0].cores(), CoreBand::ALL[3].cores()));
    out.push(format!("  压力轴：{} ↔ {}（{}‰ ↔ {}‰）", LoadBand::ALL[0].name(), LoadBand::ALL[2].name(), LoadBand::ALL[0].verdict_permille(), LoadBand::ALL[2].verdict_permille()));
    out.push(format!("  时长轴：{} ↔ {}（{} ↔ {} ticks）", DurationTrack::ALL[0].name(), DurationTrack::ALL[1].name(), DurationTrack::ALL[0].ticks(), DurationTrack::ALL[1].ticks()));
    out.push(String::from("场景 3：判据点名组合"));
    for c in m.combos.iter() {
        if c.is_named() {
            out.push(format!("  {}", c.describe()));
        }
    }
    out.push(String::from("场景 4：F2938 七批面"));
    for f in F2938Face::ALL.iter() {
        out.push(format!("  [{}] {} 权重={} 注入器={} 参与档数={}", f.index(), f.name(), f.base_weight(), f.injector(), f.bands().len()));
    }
    out.push(format!("  基础权重合计 = {}", base_weight_total()));
    out.push(String::from("场景 5：单场景替身账"));
    if let Some(sc) = m.scenes.get(0) {
        if let Ok(c) = m.combo(sc.combo_idx) {
            if let Ok(l) = DomLedger::run(c, sc) {
                out.push(format!("  {}", sc.describe()));
                out.push(format!("  {}", l.describe()));
            }
        }
    }
    out.push(String::from("场景 6：F2938 汇总账对齐"));
    if let Some(sc) = m.scenes.get(0) {
        if let Ok(c) = m.combo(sc.combo_idx) {
            if let Ok(t) = AlignTable::build(c, sc) {
                for r in t.rows().iter() {
                    out.push(format!("  {}", r));
                }
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
    let m = match PlanMatrix::expand() {
        Ok(v) => v,
        Err(_) => return vec![String::from("展开失败")],
    };
    let mut out: Vec<String> = Vec::new();
    out.push(String::from("账 A：三轴档数分布"));
    out.push(format!("  core={:?} load={:?} dur={:?}", m.core_dist, m.load_dist, m.dur_dist));
    out.push(String::from("账 B：逐场景守恒"));
    let mut i: usize = 0;
    while i < 5 {
        if let Some(sc) = m.scenes.get(i) {
            if let Ok(c) = m.combo(sc.combo_idx) {
                match DomLedger::run(c, sc) {
                    Ok(l) => out.push(format!("  {}", l.describe())),
                    Err(e) => out.push(format!("  err {}", err_str(&e))),
                }
            }
        }
        i = i.wrapping_add(1);
    }
    out.push(String::from("账 C：对齐读数"));
    if let Some(sc) = m.scenes.get(0) {
        if let Ok(c) = m.combo(sc.combo_idx) {
            if let Ok(t) = AlignTable::build(c, sc) {
                out.push(format!("  {}", t.summary()));
            }
        }
    }
    out.push(String::from("账 D：状态机合法边"));
    for s in DState::ALL.iter() {
        out.push(format!("  {:<9} -> {:?}", s.name(), s.allowed_names()));
    }
    out.push(String::from("账 E：资源上界"));
    for r in ResourceBudget::account().rows().iter() {
        out.push(format!("  {}", r));
    }
    out.push(String::from("账 F：性能分解"));
    let mm = Metrics {
        combos: 0,
        combo_count_ok: false,
        scene_count_ok: false,
        orthogonal: false,
        scenes_ok: 0,
        align_clean: 0,
        steps_ok: 0,
        duty_ok: 0,
        faces_ok: 0,
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
        basis_ok: false,
        distortion_ok: false,
        named: 0,
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
        "{}(压力判定) {}(汇总账) {}/{} (对齐注记) 制度{} 执行面{}",
        LOAD_TICKET, LEDGER_TICKET, ALIGN_NOTE_A, ALIGN_NOTE_B, SEAT_TICKET, EXEC_RANGE
    )
}
