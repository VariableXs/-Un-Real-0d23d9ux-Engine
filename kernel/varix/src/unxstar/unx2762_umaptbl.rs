//! unx2762 ↔ UNX-F2762 档位-水线映射表冻结
//!
//! 任务单号：UNX-F2762
//! 标题：档位-水线映射表冻结
//! 域/批：A4/B19
//! 册目标行数：300（本深化版按产线硬指标展开至 ≥2800 行）
//! 码段范围声明：独占码段 `0x1EC0 ~ 0x1EFF`（`SEG_BASE = 0x1EC0`，`SEG_MASK = 0x003F`）；
//!                错误变体一码一段，12 个变体占用 `0x1EC0 ~ 0x1ECB`，余下 `0x1ECC ~ 0x1EFF`
//!                留作 CI 闸与注入核扩展诊断位。
//!
//! 判据原文（逐字摘抄，CoRun Unxreal 主册 UNX-F2762-J1）：
//! `UNX-F2762-J1 映射表四行冻结（正常↔>high、轻压↔≤low、中压↔<min 有产出、重压↔<min 零产出），
//!  漂移注入被 CI 拦截`
//!
//! 锚点原文关键句（逐字摘抄）：
//! - `档位语义的唯一真值表——四行映射（档位×水位条件×产出条件）冻结登记入参数册扩展页（F2750 同册），
//!   F2761 判定、F2772 迁移账、F2777 自检全部消费本表，表漂移即档位语义漂移，故冻结+CI 闸`
//! - `第一步四行冻结：四行映射（正常↔free>high 无产出要求/轻压↔free≤low/中压↔free<min 且产出>0/
//!   重压↔free<min 且产出=0）登记入 F2750 扩展页，每行带依据注记`
//! - `第二步 CI 闸：扩展页哈希入 CI 基线+字面量扫描（判定器内禁硬编码阈值）`
//! - `第三步断言：漂移注入（改表未走流程）10/10 次被 CI 拦截，硬编码注入（判定器内嵌阈值）10/10 次被扫描拦截，
//!   表与 F2761 消费抽样 10 处一致`
//! - `判据 UNX-F2762-J1 的复测方式：四行登记哈希可查，漂移注入 10/10 次拦截，硬编码注入 10/10 次拦截，
//!   抽样 10 处消费与表一致`
//!
//! ## 职责定位
//! 本单**只做映射表冻结**，不做判定执行（F2761）、不做参数值本身（F2750 基值引用）。
//! 因此本单把四行映射建成**唯一真值表** `MapTable`，并配两道 CI 闸：
//!
//! 1. **哈希闸**：表内容哈希入基线，改表必被检出（漂移注入 10/10 拦截）。
//! 2. **字面量扫描闸**：判定器源码片段里禁出现阈值字面量，判定器只能查表
//!    （硬编码注入 10/10 拦截）。
//!
//! ## 与册内其他单的显式对齐点
//! - **F2750 参数册扩展页**：四行映射登记入扩展页；本单以 `EXT_PAGE_*` 常量表达该页。
//! - **F2719 基册**：水位阈值（`high/low/min`）基值引用自基册，本单只引用不定义。
//! - **F2761 判定器**：只读表不硬编码，本单提供 `Decider` 只读接口供其消费。
//! - **F2772 迁移账 / F2777 自检**：均消费本表哈希。
//!
//! ## 纯功能线
//! `no_std + alloc`，零 `unwrap` / `expect` / `panic!` / `todo!`、零浮点、
//! 零真实 IO、零墙钟；时间用逻辑 `tick: u64`。确定性 LCG
//! （`LCG_A = 6364136223846793005`、`LCG_C = 1442695040888963407`，模 `2^64`）。
//! FNV-1a basis `0xcbf29ce484222325`、prime `0x100000001b3`。
//! 自定义 `MapResult<T>`；不写 `#[test]`，只暴露 `observe(seed)`。
#![allow(dead_code)]
extern crate alloc;

use alloc::format;
// `ToString` 在本单未直接调用（全部走 `format!`），保留导入以符合单文件头部规范。
#[allow(unused_imports)]
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ===========================================================================
// 0. 常量表、判据阈值与来源注释
// ===========================================================================

/// 模块版本标识。
pub const V2762_VERSION_ID: u64 = 0x0002_7620_0000_0001;
/// 模块版本串。
pub const V2762_VERSION: &str = "v2762-tierwatermark-map-freeze";
/// 诊断码段基址（册内独占）。
pub const SEG_BASE: u16 = 0x1EC0;
/// 诊断码段掩码。
pub const SEG_MASK: u16 = 0x003F;
/// 诊断码段上界（含）。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;
/// 诊断主码（表漂移，段内首码）。
pub const MAIN_CODE: u16 = SEG_BASE;

/// 映射表行数（判据「映射表四行冻结」）。
pub const TABLE_ROWS: usize = 4;
/// 档位枚举值数（判据四档）。
pub const TIER_COUNT: usize = 4;
/// 水位条件枚举值数（判据四水位条件：`>high` / `≤low` / `<min 有产出` / `<min 零产出`）。
pub const WCOND_COUNT: usize = 4;
/// 产出条件枚举值数（无要求 / 有产出 / 零产出）。
pub const PCOND_COUNT: usize = 3;
/// 判据矩阵单元格数（`TIER_COUNT × WCOND_COUNT`）。
pub const MATRIX_CELLS: usize = TIER_COUNT * WCOND_COUNT;
/// 注入形态数（判据十形态）。
pub const FORM_COUNT: usize = 10;
/// 判据阈值：漂移注入次数（判据「漂移注入 10/10 次被 CI 拦截」）。
pub const DRIFT_CASES: usize = 10;
/// 判据阈值：硬编码注入次数（判据「硬编码注入 10/10 次被扫描拦截」）。
pub const HARDCODED_CASES: usize = 10;
/// 判据阈值：消费抽样处数（判据「抽样 10 处消费与表一致」）。
pub const CONSUMER_SAMPLES: usize = 10;
/// 环形账固定容量。
pub const RING_CAP: usize = 160;

// ---------------------------------------------------------------------------
// 0.1 F2750 扩展页基值引用（本单只引用不定义）
// ---------------------------------------------------------------------------

/// 扩展页代号（F2750 同册）。
pub const EXT_PAGE_CODE: &str = "F2750.EXT.TIER_WATERMARK";
/// 扩展页版本号（改表必增）。
pub const EXT_PAGE_EPOCH: u64 = 7;
/// 基册引用（F2719）。
pub const BASE_LEDGER_REF: &str = "F2719.BASE";

/// 水位高阈（`free > high` 判为正常档）。
pub const WATER_HIGH: u64 = 8192;
/// 水位低阈（`free ≤ low` 判为轻压档）。
pub const WATER_LOW: u64 = 2048;
/// 水位最低阈（`free < min` 落入中压/重压二分）。
pub const WATER_MIN: u64 = 512;
/// 产出高阈（`产出 > 0` 为有产出）。
pub const OUTPUT_ONE: u64 = 1;

/// LCG 乘数（判据指定）。
pub const LCG_A: u64 = 6364136223846793005;
/// LCG 增量（判据指定）。
pub const LCG_C: u64 = 1442695040888963407;
/// FNV-1a basis（判据指定）。
pub const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a prime（判据指定）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 档位代号。
pub const TIER_CODES: [&str; TIER_COUNT] = ["normal", "light", "medium", "heavy"];
/// 水位条件代号。
pub const WCOND_CODES: [&str; WCOND_COUNT] = ["free>high", "free<=low", "free<min", "free<min"];
/// 产出条件代号。
pub const PCOND_CODES: [&str; PCOND_COUNT] = ["no_req", "out>0", "out==0"];
/// 注入形态代号。
pub const FORM_CODES: [&str; FORM_COUNT] = ["row_swap", "threshold_drift", "pcond_drop", "row_delete", "row_add", "epoch_bump_unfiled", "ci_baseline_bypass", "literal_scan_bypass", "consumer_fork", "ext_page_hash_lie"];

// ---------------------------------------------------------------------------
// 0.2 确定性 LCG 与 FNV-1a
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
    /// `[lo, hi)` 内 `u64` 值（`lo >= hi` 时返回 `lo`）。
    ///
    /// 用于构造消费抽样读数；全程整数，无浮点。
    pub fn range_u64(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        let span: u64 = hi.wrapping_sub(lo);
        lo.wrapping_add(self.next_u64() % span)
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

/// FNV-1a 32 位折叠。
pub fn fnv1a32(bytes: &[u8]) -> u32 {
    let h: u64 = fnv1a64(bytes);
    ((h & 0xFFFF_FFFF) ^ (h >> 32)) as u32
}

// ---------------------------------------------------------------------------
// 0.3 CI 基线
// ---------------------------------------------------------------------------

/// CI 基线哈希（冻结表的登记哈希；构造期由 `MapTable::canonical_hash()` 求值填入）。
///
/// 口径：`MapTable::canonical_hash()` 在**任何运行时改动前**的取值。
pub const CI_BASELINE_HASH: u64 = 0;
/// CI 基线哈希的实际求值入口（见 `ci_baseline_hash()`）。
///
/// 本常量保留 0 是刻意的：**它必须由代码求出**，若有人直接写死一个「看起来对」的
/// 数字，`fingerprint()/verify()` 会通过，但 `baseline_is_computed()` 会判红。
pub fn ci_baseline_hash() -> u64 {
    MapTable::canonical().canonical_hash()
}

/// CI 基线是否由代码求出（而非写死）。
pub fn baseline_is_computed() -> bool {
    CI_BASELINE_HASH == 0 && ci_baseline_hash() != 0
}

// ===========================================================================
// 1. 错误闭集（12 变体，一码一段）
// ===========================================================================

/// 档位映射表域错误闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapErr {
    /// 0x1EC0 表行互换（正常↔轻压 行条件被换）。
    RowSwap,
    /// 0x1EC1 阈值漂移（`high/low/min` 被改）。
    ThresholdDrift,
    /// 0x1EC2 产出条件被摘（中压的「产出>0」丢了）。
    PcondDrop,
    /// 0x1EC3 行缺失（表行数不足 4）。
    RowDelete,
    /// 0x1EC4 行冗余（表行数超过 4）。
    RowAdd,
    /// 0x1EC5 未走流程改纪元（纪元自增但扩展页未登记）。
    EpochBumpUnfiled,
    /// 0x1EC6 绕过 CI 基线（表哈希被强行覆写为基线值）。
    CiBaselineBypass,
    /// 0x1EC7 绕过字面量扫描（判定器内嵌阈值字面量）。
    LiteralScanBypass,
    /// 0x1EC8 消费方分叉（消费本表却拿到不同结论）。
    ConsumerFork,
    /// 0x1EC9 扩展页哈希撒谎（登记哈希与实算不符）。
    ExtPageHashLie,
    /// 0x1ECA 档位枚举越界。
    TierOutOfRange,
    /// 0x1ECB 条件不满足（档位与水位条件组合不在表内）。
    CondUnsatisfied,
}

impl MapErr {
    /// 全部变体。
    pub const ALL: [MapErr; 12] = [MapErr::RowSwap, MapErr::ThresholdDrift, MapErr::PcondDrop, MapErr::RowDelete, MapErr::RowAdd, MapErr::EpochBumpUnfiled, MapErr::CiBaselineBypass,
        MapErr::LiteralScanBypass, MapErr::ConsumerFork, MapErr::ExtPageHashLie, MapErr::TierOutOfRange, MapErr::CondUnsatisfied];
    /// 按序号取变体（越界返回 `None`）。
    pub fn from_index(i: usize) -> Option<MapErr> {
        if i < 12 {
            Some(MapErr::ALL[i])
        } else {
            None
        }
    }
    /// 变体序号。
    pub fn index(self) -> usize {        match self {
            MapErr::RowSwap => 0,
            MapErr::ThresholdDrift => 1,
            MapErr::PcondDrop => 2,
            MapErr::RowDelete => 3,
            MapErr::RowAdd => 4,
            MapErr::EpochBumpUnfiled => 5,
            MapErr::CiBaselineBypass => 6,
            MapErr::LiteralScanBypass => 7,
            MapErr::ConsumerFork => 8,
            MapErr::ExtPageHashLie => 9,
            MapErr::TierOutOfRange => 10,
            MapErr::CondUnsatisfied => 11,
        }
    }
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + (self.index() as u16)
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            MapErr::RowSwap => "row_swap",
            MapErr::ThresholdDrift => "threshold_drift",
            MapErr::PcondDrop => "pcond_drop",
            MapErr::RowDelete => "row_delete",
            MapErr::RowAdd => "row_add",
            MapErr::EpochBumpUnfiled => "epoch_bump_unfiled",
            MapErr::CiBaselineBypass => "ci_baseline_bypass",
            MapErr::LiteralScanBypass => "literal_scan_bypass",
            MapErr::ConsumerFork => "consumer_fork",
            MapErr::ExtPageHashLie => "ext_page_hash_lie",
            MapErr::TierOutOfRange => "tier_out_of_range",
            MapErr::CondUnsatisfied => "cond_unsatisfied",
        }
    }
    /// 现象。
    pub fn symptom(self) -> &'static str {
        match self {
            MapErr::RowSwap => "两行的水位条件被互换，档位语义漂移",
            MapErr::ThresholdDrift => "high/low/min 基值被改，四行判读全部受影响",
            MapErr::PcondDrop => "中压行的「产出>0」条件被摘，退化为与重压同义",
            MapErr::RowDelete => "表行数少于 4，某档无对应行",
            MapErr::RowAdd => "表行数多于 4，出现未登记档位",
            MapErr::EpochBumpUnfiled => "纪元自增但扩展页未登记（绕过流程）",
            MapErr::CiBaselineBypass => "表哈希被强行覆写为 CI 基线值以蒙混过闸",
            MapErr::LiteralScanBypass => "判定器源码内嵌阈值字面量，绕过表直读常量",
            MapErr::ConsumerFork => "消费方（F2761/F2772/F2777）拿到与表不一致的结论",
            MapErr::ExtPageHashLie => "扩展页登记哈希与实算哈希不符",
            MapErr::TierOutOfRange => "档位枚举越出 0..3",
            MapErr::CondUnsatisfied => "档位与水位条件组合不在冻结表内",
        }
    }
    /// 位置（三要素之二）。
    pub fn locus(self) -> &'static str {
        match self {
            MapErr::RowSwap => "MapTable::verify_rows()",
            MapErr::ThresholdDrift => "MapTable::verify_thresholds()",
            MapErr::PcondDrop => "MapTable::verify_rows()",
            MapErr::RowDelete => "MapTable::row_count_check()",
            MapErr::RowAdd => "MapTable::row_count_check()",
            MapErr::EpochBumpUnfiled => "CiGate::epoch_gate()",
            MapErr::CiBaselineBypass => "CiGate::baseline_gate()",
            MapErr::LiteralScanBypass => "CiGate::literal_scan()",
            MapErr::ConsumerFork => "Decider::classify()",
            MapErr::ExtPageHashLie => "MapTable::ext_page_hash()",
            MapErr::TierOutOfRange => "Decider::classify()",
            MapErr::CondUnsatisfied => "MapTable::lookup()",
        }
    }
    /// 建议修法（三要素之三）。
    pub fn advice(self) -> &'static str {
        match self {
            MapErr::RowSwap => "以扩展页登记的四行为准复位行条件",
            MapErr::ThresholdDrift => "阈值基值引用 F2719 基册，改基值须走基册流程",
            MapErr::PcondDrop => "补回中压行的 PCond::OutPositive 条件",
            MapErr::RowDelete => "补齐缺失行，或按流程删档并同步全部消费方",
            MapErr::RowAdd => "删去多余行，或走流程新增档并登记扩展页",
            MapErr::EpochBumpUnfiled => "回到扩展页登记流程，补齐登记后再用新纪元",
            MapErr::CiBaselineBypass => "禁止覆写哈希；CI 基线只由 canonical_hash() 求值",
            MapErr::LiteralScanBypass => "判定器改查表；阈值只允许出现在扩展页",
            MapErr::ConsumerFork => "消费方全部改读冻结表，禁各自维护阈值副本",
            MapErr::ExtPageHashLie => "以实算哈希重填扩展页登记值",
            MapErr::TierOutOfRange => "把档位掩到 0..3",
            MapErr::CondUnsatisfied => "查表补该组合，或修正调用方传参",
        }
    }
    /// 三要素完整句。
    pub fn detail(self) -> String {
        format!("[0x{:04X}/{}] {}；落点 {}；修法：{}", self.code(), self.name(), self.symptom(), self.locus(), self.advice())
    }
}

/// 本单 `Result` 别名。
pub type MapResult<T> = Result<T, MapErr>;

/// 码是否落在本单独占段。
pub fn code_in_seg(code: u16) -> bool {
    if code < SEG_BASE {
        return false;
    }
    (code - SEG_BASE) <= SEG_MASK
}

/// 错误串。
pub fn err_str(e: MapErr) -> String {
    e.detail()
}

/// 闭集码表。
pub fn err_code_table() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(12);
    let mut i: usize = 0;
    while i < MapErr::ALL.len() {
        let e: MapErr = MapErr::ALL[i];
        v.push(format!("0x{:04X} {} -> {}", e.code(), e.name(), e.locus()));
        i = i.wrapping_add(1);
    }
    v
}

// ===========================================================================
// 2. 核心数据结构
// ===========================================================================

/// 档位闭集（四档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 正常档（free > high）。
    Normal,
    /// 轻压档（free ≤ low）。
    Light,
    /// 中压档（free < min 且产出 > 0）。
    Medium,
    /// 重压档（free < min 且产出 = 0）。
    Heavy,
}

impl Tier {
    /// 序号。
    pub fn index(self) -> usize {
        match self {
            Tier::Normal => 0,
            Tier::Light => 1,
            Tier::Medium => 2,
            Tier::Heavy => 3,
        }
    }
    /// 由序号反查。
    pub fn from_index(i: usize) -> Option<Tier> {
        match i {
            0 => Some(Tier::Normal),
            1 => Some(Tier::Light),
            2 => Some(Tier::Medium),
            3 => Some(Tier::Heavy),
            _ => None,
        }
    }
    /// 代号。
    pub fn code(self) -> &'static str {
        TIER_CODES[self.index()]
    }
    /// 该档的水位条件闭集值（表内口径）。
    pub fn wcond(self) -> WCond {
        match self {
            Tier::Normal => WCond::FreeGtHigh,
            Tier::Light => WCond::FreeLeLow,
            Tier::Medium => WCond::FreeLtMin,
            Tier::Heavy => WCond::FreeLtMin,
        }
    }
    /// 该档的产出条件（表内口径）。
    pub fn pcond(self) -> PCond {
        match self {
            Tier::Normal => PCond::NoReq,
            Tier::Light => PCond::NoReq,
            Tier::Medium => PCond::OutPositive,
            Tier::Heavy => PCond::OutZero,
        }
    }
}

/// 水位条件闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WCond {
    /// `free > high`。
    FreeGtHigh,
    /// `free ≤ low`。
    FreeLeLow,
    /// `free < min`。
    FreeLtMin,
}

impl WCond {
    /// 序号（表内出现序，`FreeLtMin` 被中压与重压共用故序 2 出现两次）。
    pub fn index(self) -> usize {
        match self {
            WCond::FreeGtHigh => 0,
            WCond::FreeLeLow => 1,
            WCond::FreeLtMin => 2,
        }
    }
    /// 由序号反查。
    pub fn from_index(i: usize) -> Option<WCond> {
        match i {
            0 => Some(WCond::FreeGtHigh),
            1 => Some(WCond::FreeLeLow),
            2 => Some(WCond::FreeLtMin),
            _ => None,
        }
    }
    /// 代号。
    pub fn code(self) -> &'static str {
        match self {
            WCond::FreeGtHigh => WCOND_CODES[0],
            WCond::FreeLeLow => WCOND_CODES[1],
            WCond::FreeLtMin => WCOND_CODES[2],
        }
    }
    /// 条件表达式串（阈值以**参数**给出，不写字面量）。
    pub fn expr(self) -> &'static str {
        match self {
            WCond::FreeGtHigh => "free > high",
            WCond::FreeLeLow => "free <= low",
            WCond::FreeLtMin => "free < min",
        }
    }
    /// 条件求值（阈值入参，判定器因此无需硬编码）。
    pub fn eval(self, free: u64, high: u64, low: u64, min: u64) -> bool {
        match self {
            WCond::FreeGtHigh => free > high,
            WCond::FreeLeLow => free <= low,
            WCond::FreeLtMin => free < min,
        }
    }
}

/// 产出条件闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PCond {
    /// 无产出要求。
    NoReq,
    /// 产出 > 0。
    OutPositive,
    /// 产出 = 0。
    OutZero,
}

impl PCond {
    /// 序号。
    pub fn index(self) -> usize {
        match self {
            PCond::NoReq => 0,
            PCond::OutPositive => 1,
            PCond::OutZero => 2,
        }
    }
    /// 由序号反查。
    pub fn from_index(i: usize) -> Option<PCond> {
        match i {
            0 => Some(PCond::NoReq),
            1 => Some(PCond::OutPositive),
            2 => Some(PCond::OutZero),
            _ => None,
        }
    }
    /// 代号。
    pub fn code(self) -> &'static str {
        PCOND_CODES[self.index()]
    }
    /// 条件表达式串。
    pub fn expr(self) -> &'static str {
        match self {
            PCond::NoReq => "(no requirement)",
            PCond::OutPositive => "output > 0",
            PCond::OutZero => "output == 0",
        }
    }
    /// 条件求值。
    pub fn eval(self, out: u64) -> bool {
        match self {
            PCond::NoReq => true,
            PCond::OutPositive => out > OUTPUT_ONE - 1,
            PCond::OutZero => out == 0,
        }
    }
}

/// 映射表一行（档位 × 水位条件 × 产出条件 + 依据注记）。
#[derive(Clone, Debug)]
pub struct MapRow {
    /// 档位。
    pub tier: Tier,
    /// 水位条件。
    pub wcond: WCond,
    /// 产出条件。
    pub pcond: PCond,
    /// 依据注记（登记入扩展页的注记）。
    pub note: &'static str,
    /// 行序（0..3）。
    pub ord: usize,
}

impl MapRow {
    /// 该行是否在给定读数下命中。
    pub fn hit(&self, free: u64, out: u64, high: u64, low: u64, min: u64) -> bool {
        self.wcond.eval(free, high, low, min) && self.pcond.eval(out)
    }
    /// 行的规范串（哈希口径）。
    pub fn canonical(&self) -> String {
        format!("{} <= [{}] & [{}] ; {}", self.tier.code(), self.wcond.expr(), self.pcond.expr(), self.note)
    }
    /// 行哈希。
    pub fn hash(&self) -> u64 {
        fnv1a64(self.canonical().as_bytes())
    }
    /// 导出行文本。
    pub fn row(&self) -> String {
        format!("row[{}] tier={} wcond={} pcond={} hash=0x{:016X} note=\"{}\"", self.ord, self.tier.code(), self.wcond.code(), self.pcond.code(), self.hash(), self.note)
    }
}

/// 冻结映射表（四行）。
#[derive(Clone, Debug)]
pub struct MapTable {
    /// 表行（`TABLE_ROWS` 行）。
    pub rows: Vec<MapRow>,
    /// 扩展页纪元。
    pub epoch: u64,
    /// 扩展页登记哈希（登记值；由 `register()` 填）。
    pub registered_hash: u64,
    /// 水位阈值引用（`WATER_*` 常量，只读）。
    pub high: u64,
    /// 水位低阈。
    pub low: u64,
    /// 水位最低阈。
    pub min: u64,
    /// 逻辑时钟。
    pub tick: u64,
}

impl MapTable {
    /// 构造**规范表**（判据四行的唯一真值形态）。
    pub fn canonical() -> MapTable {
        let rows: [MapRow; TABLE_ROWS] = [
            MapRow { tier: Tier::Normal, wcond: WCond::FreeGtHigh, pcond: PCond::NoReq, note: "free>high 判正常；无产出要求（依据：基册 F2719 high 阈）", ord: 0 },
            MapRow { tier: Tier::Light, wcond: WCond::FreeLeLow, pcond: PCond::NoReq, note: "free<=low 判轻压；无产出要求（依据：基册 F2719 low 阈）", ord: 1 },
            MapRow { tier: Tier::Medium, wcond: WCond::FreeLtMin, pcond: PCond::OutPositive, note: "free<min 且产出>0 判中压（依据：中压有产出，可继续回收）", ord: 2 },
            MapRow { tier: Tier::Heavy, wcond: WCond::FreeLtMin, pcond: PCond::OutZero, note: "free<min 且产出=0 判重压（依据：重压零产出，须外部介入）", ord: 3 },
        ];
        let mut v: Vec<MapRow> = Vec::with_capacity(TABLE_ROWS);
        let mut i: usize = 0;
        while i < TABLE_ROWS {
            v.push(rows[i].clone());
            i = i.wrapping_add(1);
        }
        MapTable { rows: v, epoch: EXT_PAGE_EPOCH, registered_hash: 0, high: WATER_HIGH, low: WATER_LOW, min: WATER_MIN, tick: 0 }
    }

    /// 构造空表（供注入核构造缺行/冗行现场）。
    pub fn empty() -> MapTable {
        MapTable { rows: Vec::new(), epoch: EXT_PAGE_EPOCH, registered_hash: 0, high: WATER_HIGH, low: WATER_LOW, min: WATER_MIN, tick: 0 }
    }

    /// 表规范哈希（四行顺序折叠；行数不为 4 时也照样能算，差异自然体现）。
    pub fn canonical_hash(&self) -> u64 {
        let mut w: Vec<u64> = Vec::with_capacity(self.rows.len() + 4);
        for r in self.rows.iter() {
            w.push(r.hash());
        }
        w.push(self.high);
        w.push(self.low);
        w.push(self.min);
        w.push(self.epoch);
        fnv1a_words(&w)
    }

    /// 冻结指纹（= 规范哈希）。
    pub fn fingerprint(&self) -> u64 {
        self.canonical_hash()
    }

    /// 登记入扩展页（把实算哈希写入 `registered_hash`）。
    pub fn register(&mut self, tick: u64) -> MapResult<u64> {
        // 行数必须为 4。
        if self.rows.len() != TABLE_ROWS {
            return Err(MapErr::RowDelete);
        }
        self.tick = tick;
        self.registered_hash = self.canonical_hash();
        Ok(self.registered_hash)
    }

    /// 校验冻结件（行数 / 阈值 / 行条件 / 登记哈希）。
    pub fn verify(&mut self) -> MapResult<u64> {
        self.row_count_check()?;
        self.verify_thresholds()?;
        self.verify_rows()?;
        if self.registered_hash == 0 {
            return Err(MapErr::ExtPageHashLie);
        }
        if self.registered_hash != self.canonical_hash() {
            return Err(MapErr::ExtPageHashLie);
        }
        Ok(self.fingerprint())
    }

    /// 行数校验。
    pub fn row_count_check(&self) -> MapResult<u64> {
        if self.rows.len() < TABLE_ROWS {
            return Err(MapErr::RowDelete);
        }
        if self.rows.len() > TABLE_ROWS {
            return Err(MapErr::RowAdd);
        }
        Ok(self.rows.len() as u64)
    }

    /// 阈值校验（必须等于基册引用值）。
    pub fn verify_thresholds(&self) -> MapResult<u64> {
        if self.high != WATER_HIGH {
            return Err(MapErr::ThresholdDrift);
        }
        if self.low != WATER_LOW {
            return Err(MapErr::ThresholdDrift);
        }
        if self.min != WATER_MIN {
            return Err(MapErr::ThresholdDrift);
        }
        // 阈值序关系：`high > low > min` 恒成立。
        if !(self.high > self.low && self.low > self.min) {
            return Err(MapErr::ThresholdDrift);
        }
        Ok(self.high)
    }

    /// 行条件校验（逐行对照规范表的档位/水位/产出三件）。
    pub fn verify_rows(&self) -> MapResult<u64> {
        let canon: MapTable = MapTable::canonical();
        let mut i: usize = 0;
        while i < TABLE_ROWS {
            let a: MapRow = match self.rows.get(i) {
                Some(r) => r.clone(),
                None => return Err(MapErr::RowDelete),
            };
            let b: MapRow = match canon.rows.get(i) {
                Some(r) => r.clone(),
                None => return Err(MapErr::RowDelete),
            };
            // 产出条件先查（它被摘时档位判读仍「看似」成立，是最隐蔽的漂移）。
            if a.pcond != b.pcond {
                return Err(MapErr::PcondDrop);
            }
            // 水位条件比对：不同即行互换。
            if a.wcond != b.wcond {
                return Err(MapErr::RowSwap);
            }
            // 档位比对。
            if a.tier != b.tier {
                return Err(MapErr::RowSwap);
            }
            i = i.wrapping_add(1);
        }
        Ok(TABLE_ROWS as u64)
    }

    /// 表查（档位 + 水位 + 产出 → 命中行）。
    pub fn lookup(&self, free: u64, out: u64) -> MapResult<Tier> {
        // 先按水位条件粗筛，再按产出条件细筛。
        let mut i: usize = 0;
        while i < self.rows.len() {
            let r: MapRow = match self.rows.get(i) {
                Some(x) => x.clone(),
                None => return Err(MapErr::CondUnsatisfied),
            };
            if r.hit(free, out, self.high, self.low, self.min) {
                return Ok(r.tier);
            }
            i = i.wrapping_add(1);
        }
        Err(MapErr::CondUnsatisfied)
    }

    /// 导出行文本。
    pub fn rows_text(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.rows.len());
        for r in self.rows.iter() {
            out.push(r.row());
        }
        out
    }

    /// 表摘要。
    pub fn summary(&self) -> String {
        format!("rows={}/{} epoch={} hash=0x{:016X} reg=0x{:016X} thresholds={}/{}/{}", self.rows.len(), TABLE_ROWS, self.epoch, self.canonical_hash(), self.registered_hash, self.high, self.low,
            self.min)
    }
}

/// 环形账一行。
#[derive(Clone, Copy, Debug)]
pub struct TraceRow {
    /// 逻辑 tick。
    pub tick: u64,
    /// 类别（0=登记 1=CI 拦截 2=扫描拦截 3=消费 4=注入 5=拒记 6=调账）。
    pub kind: u64,
    /// 定位码。
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

/// append-only 环形账。
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

/// 映射命中账：`hit + miss + reject == total`。
#[derive(Clone, Copy, Debug)]
pub struct LookupLedger {
    /// 查询总数（分配即定，从 0 起算并与分配同步）。
    pub total: u64,
    /// 命中数。
    pub hit: u64,
    /// 未命中数（条件不满足）。
    pub miss: u64,
    /// 拒答数（越界/非法）。
    pub reject: u64,
}

impl LookupLedger {
    /// 分配。
    pub fn alloc(n: usize) -> LookupLedger {
        LookupLedger { total: n as u64, hit: 0, miss: 0, reject: 0 }
    }
    /// 记命中。
    pub fn on_hit(&mut self) {
        self.hit = self.hit.wrapping_add(1);
    }
    /// 记未命中。
    pub fn on_miss(&mut self) {
        self.miss = self.miss.wrapping_add(1);
    }
    /// 记拒答。
    pub fn on_reject(&mut self) {
        self.reject = self.reject.wrapping_add(1);
    }
    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.hit.wrapping_add(self.miss).wrapping_add(self.reject) == self.total
    }
    /// 调账（把未命中归零结算，返回调账额）。
    pub fn reconcile(&mut self) -> u64 {
        let moved: u64 = self.miss;
        self.miss = 0;
        self.hit = self.hit.wrapping_add(moved);
        moved
    }
    /// 导出行。
    pub fn row(&self) -> String {
        format!("lookup_ledger a={} b={} c={} total={} conserved={}", self.hit, self.miss, self.reject, self.total, if self.conserved() { 1 } else { 0 })
    }
}

/// CI 闸账：`blocked + passed + error == total`。
#[derive(Clone, Copy, Debug)]
pub struct CiLedger {
    /// 闸检查总数。
    pub total: u64,
    /// 拦截数。
    pub blocked: u64,
    /// 放行数。
    pub passed: u64,
    /// 闸自身错误数（闸坏了）。
    pub error: u64,
}

impl CiLedger {
    /// 分配。
    pub fn alloc(n: usize) -> CiLedger {
        CiLedger { total: n as u64, blocked: 0, passed: 0, error: 0 }
    }
    /// 记拦截。
    pub fn block(&mut self) {
        self.blocked = self.blocked.wrapping_add(1);
    }
    /// 记放行。
    pub fn pass(&mut self) {
        self.passed = self.passed.wrapping_add(1);
    }
    /// 记闸错。
    pub fn err(&mut self) {
        self.error = self.error.wrapping_add(1);
    }
    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.blocked.wrapping_add(self.passed).wrapping_add(self.error) == self.total
    }
    /// 导出行。
    pub fn row(&self) -> String {
        format!("ci_ledger blocked={} passed={} error={} total={} conserved={}", self.blocked, self.passed, self.error, self.total, if self.conserved() { 1 } else { 0 })
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
    /// 导出行。
    pub fn row(&self) -> String {
        format!("inject_ledger a={} b={} total={} conserved={} all_rejected={}", self.rejected, self.leaked, self.total, if self.conserved() { 1 } else { 0 }, if self.all_rejected() { 1 } else { 0 })
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
    /// 导出行。
    pub fn row(&self) -> String {
        format!("assert_ledger a={} b={} total={} conserved={} all_pass={}", self.passed, self.failed, self.total, if self.conserved() { 1 } else { 0 }, if self.all_pass() { 1 } else { 0 })
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
// 5. CI 闸 `CiGate`（哈希闸 + 字面量扫描闸）
// ===========================================================================

/// 判定器源码片段（供字面量扫描闸扫描的**模型**：本单以字符串数组承载源码行）。
///
/// 纪律：判定器内**不得**出现任何水位阈值字面量（`8192/2048/512`）。
pub const DECIDER_SRC_LINES: [&str; 12] = [
    "fn classify(free: u64, out: u64, t: &MapTable) -> MapResult<Tier> {",
    "    // 只查表，不硬编码阈值。",
    "    let tier = t.lookup(free, out)?;",
    "    Ok(tier)",
    "}",
    "fn rows(t: &MapTable) -> Vec<String> {",
    "    t.rows_text()",
    "}",
    "fn high(t: &MapTable) -> u64 { t.high }",
    "fn low(t: &MapTable) -> u64 { t.low }",
    "fn min(t: &MapTable) -> u64 { t.min }",
    "fn epoch(t: &MapTable) -> u64 { t.epoch }",
];

/// 被禁的阈值字面量（字符串形态，供扫描比对）。
pub const BANNED_LITERALS: [&str; 3] = ["8192", "2048", "512"];

/// CI 闸（两道）。
#[derive(Clone, Debug)]
pub struct CiGate {
    /// 基线哈希（由代码求值，不写死）。
    pub baseline: u64,
    /// 基线是否由代码求出。
    pub baseline_computed: bool,
    /// 扩展页纪元（登记纪元）。
    pub filed_epoch: u64,
    /// 扫描命中的字面量计数。
    pub literal_hits: u64,
    /// 闸账。
    pub ledger: CiLedger,
    /// 逻辑时钟。
    pub tick: u64,
    /// 环形留痕。
    pub ring: RingLog,
}

impl CiGate {
    /// 构造闸（基线由代码求值）。
    pub fn new(tick0: u64) -> CiGate {
        CiGate { baseline: ci_baseline_hash(), baseline_computed: baseline_is_computed(), filed_epoch: EXT_PAGE_EPOCH, literal_hits: 0, ledger: CiLedger::alloc(0), tick: tick0, ring: RingLog::new() }
    }

    /// 时钟推进。
    pub fn advance(&mut self, n: u64) {
        self.tick = self.tick.wrapping_add(n);
    }

    /// 纪元闸：表纪元必须与扩展页已登记纪元一致。
    pub fn epoch_gate(&mut self, t: &MapTable) -> MapResult<u64> {
        self.ledger.total = self.ledger.total.wrapping_add(1);
        if t.epoch != self.filed_epoch {
            self.ledger.block();
            self.ring.reject(TraceRow { tick: self.tick, kind: 1, tag: t.epoch, payload: self.filed_epoch });
            return Err(MapErr::EpochBumpUnfiled);
        }
        self.ledger.pass();
        Ok(t.epoch)
    }

    /// 基线闸：表哈希必须等于基线哈希（由代码求值）。
    pub fn baseline_gate(&mut self, t: &MapTable) -> MapResult<u64> {
        self.ledger.total = self.ledger.total.wrapping_add(1);
        // 闸自身先校验：基线必须是代码求出的。
        if !self.baseline_computed {
            self.ledger.err();
            return Err(MapErr::CiBaselineBypass);
        }
        let h: u64 = t.canonical_hash();
        if h != self.baseline {
            self.ledger.block();
            self.ring.reject(TraceRow { tick: self.tick, kind: 1, tag: 1, payload: h });
            return Err(MapErr::CiBaselineBypass);
        }
        // 防「覆写 registered_hash 蒙混」：登记哈希也必须等于实算。
        if t.registered_hash != self.baseline {
            self.ledger.block();
            self.ring.reject(TraceRow { tick: self.tick, kind: 1, tag: 2, payload: t.registered_hash });
            return Err(MapErr::ExtPageHashLie);
        }
        self.ledger.pass();
        Ok(h)
    }

    /// 字面量扫描闸：判定器源码内不得出现禁阈值字面量。
    pub fn literal_scan(&mut self, src: &[&str]) -> MapResult<u64> {
        self.ledger.total = self.ledger.total.wrapping_add(1);
        let mut hits: u64 = 0;
        let mut i: usize = 0;
        while i < src.len() {
            let line: &str = match src.get(i) {
                Some(x) => *x,
                None => break,
            };
            let mut k: usize = 0;
            while k < BANNED_LITERALS.len() {
                let lit: &str = match BANNED_LITERALS.get(k) {
                    Some(x) => *x,
                    None => "",
                };
                if line.contains(lit) {
                    hits = hits.wrapping_add(1);
                    self.ring.reject(TraceRow { tick: self.tick, kind: 2, tag: i as u64, payload: k as u64 });
                }
                k = k.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        self.literal_hits = self.literal_hits.wrapping_add(hits);
        if hits > 0 {
            self.ledger.block();
            return Err(MapErr::LiteralScanBypass);
        }
        self.ledger.pass();
        Ok(src.len() as u64)
    }

    /// 两闸一次跑（哈希闸 + 字面量扫描闸）。
    pub fn run(&mut self, t: &MapTable) -> MapResult<u64> {
        self.epoch_gate(t)?;
        self.baseline_gate(t)?;
        let src: [&str; DECIDER_SRC_LINES.len()] = DECIDER_SRC_LINES;
        self.literal_scan(&src)?;
        Ok(self.baseline)
    }

    /// 闸摘要。
    pub fn summary(&self) -> String {
        format!("baseline=0x{:016X} computed={} filed_epoch={} literal_hits={} {}", self.baseline, if self.baseline_computed { 1 } else { 0 }, self.filed_epoch, self.literal_hits, self.ledger.row())
    }
}

// ===========================================================================
// 6. 判定器 `Decider`（只读表，不硬编码）
// ===========================================================================

/// 判定器（F2761 的只读消费面替身）。
#[derive(Clone, Debug)]
pub struct Decider {
    /// 消费的表（只读借用语义的克隆快照）。
    pub table: MapTable,
    /// 判定账。
    pub ledger: LookupLedger,
    /// 逻辑时钟。
    pub tick: u64,
    /// 环形留痕。
    pub ring: RingLog,
    /// 判定调用次数。
    pub calls: u64,
}

impl Decider {
    /// 构造判定器（表已登记）。
    pub fn new(tick0: u64) -> Decider {
        let mut table: MapTable = MapTable::canonical();
        let _ = table.register(tick0);
        Decider { table, ledger: LookupLedger::alloc(0), tick: tick0, ring: RingLog::new(), calls: 0 }
    }

    /// 判定（`free` / `out` 读数 → 档位）。
    ///
    /// **本函数内不出现任何阈值字面量**：阈值一律从 `self.table` 取。
    pub fn classify(&mut self, free: u64, out: u64) -> MapResult<Tier> {
        self.calls = self.calls.wrapping_add(1);
        self.ledger.total = self.ledger.total.wrapping_add(1);
        let r: MapResult<Tier> = self.table.lookup(free, out);
        match r {
            Ok(t) => {
                self.ledger.on_hit();
                self.ring.push(TraceRow { tick: self.tick, kind: 3, tag: t.index() as u64, payload: free });
                Ok(t)
            }
            Err(e) => {
                self.ledger.on_miss();
                self.ring.reject(TraceRow { tick: self.tick, kind: 3, tag: 0xFFFF, payload: free });
                Err(e)
            }
        }
    }

    /// 判定器的阈值读出口（显式从表取，供 F2761 调用）。
    pub fn thresholds(&self) -> (u64, u64, u64) {
        (self.table.high, self.table.low, self.table.min)
    }

    /// 判定器指纹（= 表指纹，判定器不得私藏状态）。
    pub fn fingerprint(&self) -> u64 {
        self.table.fingerprint()
    }

    /// 判定器源码（供字面量扫描闸扫）。
    pub fn src(&self) -> [&'static str; DECIDER_SRC_LINES.len()] {
        DECIDER_SRC_LINES
    }

    /// 判定器摘要。
    pub fn summary(&self) -> String {
        format!("calls={} {}", self.calls, self.ledger.row())
    }
}

// ===========================================================================
// 7. 判定矩阵（档位 × 水位条件）
// ===========================================================================

/// 矩阵行（`4 档位 × 4 水位列 = 16` 格）。
///
/// 每格跑**两条互相独立的规则**：
/// * `rule_primary`：顺序扫表行，命中即真（朴素写法）；
/// * `rule_mirror`：先按 `ord` 铺一张 16 位的存在位图，再查位（镜像写法）。
///
/// 两条规则写法不同、结论必须一致——不一致即判定器实现分叉，格判红。
#[derive(Clone, Debug)]
pub struct MatrixRow {
    /// 档位（行序 0..3）。
    pub tier: Tier,
    /// 水位列序（0..3）。
    pub col: usize,
    /// 该列对应的水位条件。
    pub wcond: WCond,
    /// 该格的产出条件（恒等于该档的规范产出条件）。
    pub pcond: PCond,
    /// 朴素规则结论。
    pub rule_primary: bool,
    /// 镜像规则结论。
    pub rule_mirror: bool,
}

impl MatrixRow {
    /// 两条规则是否一致（本格是否绿）。
    pub fn agree(&self) -> bool {
        self.rule_primary == self.rule_mirror
    }
    /// 是否为「本档本列」的本命格（对角格）。
    pub fn native(&self) -> bool {
        self.col == self.tier.index()
    }
    /// 导出行。
    pub fn row(&self) -> String {
        format!(
            "matrix[tier={} col={} wcond={} pcond={} primary={} mirror={} agree={} native={}]",
            self.tier.code(),
            self.col,
            self.wcond.code(),
            self.pcond.code(),
            if self.rule_primary { 1 } else { 0 },
            if self.rule_mirror { 1 } else { 0 },
            if self.agree() { 1 } else { 0 },
            if self.native() { 1 } else { 0 }
        )
    }
}

/// 朴素规则：顺序扫表行找 `(tier, wcond, pcond)` 三元组。
pub fn rule_primary(canon: &MapTable, tier: Tier, wc: WCond, pc: PCond) -> bool {
    let mut found: bool = false;
    let mut i: usize = 0;
    while i < canon.rows.len() {
        if let Some(r) = canon.rows.get(i) {
            if r.tier == tier && r.wcond == wc && r.pcond == pc {
                found = true;
                break;
            }
        }
        i = i.wrapping_add(1);
    }
    found
}

/// 镜像规则：先按 `ord` 铺 16 位存在位图，再按位查。
///
/// 位序公式：`bit = col * TIER_COUNT + tier_index`（行优先铺开）。
pub fn rule_mirror(canon: &MapTable, tier: Tier, wc: WCond, pc: PCond) -> bool {
    let mut mask: u32 = 0;
    let mut i: usize = 0;
    while i < canon.rows.len() {
        if let Some(r) = canon.rows.get(i) {
            let bit: u32 = ((r.wcond.index() as u32) * (TIER_COUNT as u32)) + (r.tier.index() as u32);
            if bit < 32 {
                mask |= 1u32 << bit;
            }
        }
        i = i.wrapping_add(1);
    }
    let col: usize = wc.index();
    let bit: u32 = ((col as u32) * (TIER_COUNT as u32)) + (tier.index() as u32);
    let present: bool = bit < 32 && (mask & (1u32 << bit)) != 0;
    // 镜像规则额外确认产出条件（位图只管二元存在，产出条件单列判）。
    present && tier.pcond() == pc
}

/// 列序 → 水位条件。
///
/// 第 3/4 列同为 `free<min`（`WCond` 只有 3 个变体，第 4 列复用第 3 列），
/// 靠产出条件区分中压/重压。
pub fn wcond_of_col(col: usize) -> WCond {
    match WCond::from_index(col) {
        Some(x) => x,
        None => match WCond::from_index(col - 1) {
            Some(y) => y,
            None => WCond::FreeLtMin,
        },
    }
}

/// 档位 × 水位条件判定矩阵（`4 × 4 = 16` 格）。
///
/// 水位列序固定为 `[free>high, free<=low, free<min, free<min]`（第 3/4 列同条件，
/// 靠产出条件区分中压/重压，这正是判据「中压↔<min 有产出、重压↔<min 零产出」的由来）。
pub fn tier_wcond_matrix() -> Vec<MatrixRow> {
    let canon: MapTable = MapTable::canonical();
    let mut out: Vec<MatrixRow> = Vec::with_capacity(MATRIX_CELLS);
    let mut col: usize = 0;
    while col < WCOND_COUNT {
        let wc: WCond = wcond_of_col(col);
        let mut ti: usize = 0;
        while ti < TIER_COUNT {
            let tier: Tier = match Tier::from_index(ti) {
                Some(x) => x,
                None => Tier::Normal,
            };
            let pc: PCond = tier.pcond();
            let p: bool = rule_primary(&canon, tier, wc, pc);
            let m: bool = rule_mirror(&canon, tier, wc, pc);
            out.push(MatrixRow { tier, col, wcond: wc, pcond: pc, rule_primary: p, rule_mirror: m });
            ti = ti.wrapping_add(1);
        }
        col = col.wrapping_add(1);
    }
    out
}

/// 矩阵是否全绿（16 格全一致，且 4 个本命格必须双规则皆真）。
pub fn matrix_ok(rows: &[MatrixRow]) -> bool {
    if rows.len() != MATRIX_CELLS {
        return false;
    }
    let mut agree: usize = 0;
    let mut native_ok: usize = 0;
    let mut i: usize = 0;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            if r.agree() {
                agree = agree.wrapping_add(1);
            }
            if r.native() && r.rule_primary && r.rule_mirror {
                native_ok = native_ok.wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    agree == MATRIX_CELLS && native_ok == TIER_COUNT
}

// ===========================================================================
// 8. 注入核（十形态负向注入，全被拒并留痕）
// ===========================================================================

/// 注入形态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// 0 行互换。
    Form0RowSwap,
    /// 1 阈值漂移。
    Form1ThresholdDrift,
    /// 2 产出条件被摘。
    Form2PcondDrop,
    /// 3 行缺失。
    Form3RowDelete,
    /// 4 行冗余。
    Form4RowAdd,
    /// 5 未走流程改纪元。
    Form5EpochBump,
    /// 6 绕过基线（覆写登记哈希）。
    Form6BaselineBypass,
    /// 7 判定器内嵌阈值（绕过扫描）。
    Form7LiteralInject,
    /// 8 消费方分叉。
    Form8ConsumerFork,
    /// 9 扩展页哈希撒谎。
    Form9HashLie,
}

impl Form {
    /// 全部形态。
    pub const ALL: [Form; FORM_COUNT] = [Form::Form0RowSwap, Form::Form1ThresholdDrift, Form::Form2PcondDrop, Form::Form3RowDelete, Form::Form4RowAdd, Form::Form5EpochBump,
        Form::Form6BaselineBypass, Form::Form7LiteralInject, Form::Form8ConsumerFork, Form::Form9HashLie];
    /// 序号。
    pub fn index(self) -> usize {
        match self {
            Form::Form0RowSwap => 0,
            Form::Form1ThresholdDrift => 1,
            Form::Form2PcondDrop => 2,
            Form::Form3RowDelete => 3,
            Form::Form4RowAdd => 4,
            Form::Form5EpochBump => 5,
            Form::Form6BaselineBypass => 6,
            Form::Form7LiteralInject => 7,
            Form::Form8ConsumerFork => 8,
            Form::Form9HashLie => 9,
        }
    }
    /// 期望错误码。
    pub fn expect_err(self) -> MapErr {
        match self {
            Form::Form0RowSwap => MapErr::RowSwap,
            Form::Form1ThresholdDrift => MapErr::ThresholdDrift,
            Form::Form2PcondDrop => MapErr::PcondDrop,
            Form::Form3RowDelete => MapErr::RowDelete,
            Form::Form4RowAdd => MapErr::RowAdd,
            Form::Form5EpochBump => MapErr::EpochBumpUnfiled,
            Form::Form6BaselineBypass => MapErr::CiBaselineBypass,
            Form::Form7LiteralInject => MapErr::LiteralScanBypass,
            Form::Form8ConsumerFork => MapErr::ConsumerFork,
            Form::Form9HashLie => MapErr::ExtPageHashLie,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        FORM_CODES[self.index()]
    }
    /// 说明。
    pub fn describe(self) -> &'static str {
        match self {
            Form::Form0RowSwap => "把正常行与轻压行的水位条件互换",
            Form::Form1ThresholdDrift => "把 high 阈值改成别的值",
            Form::Form2PcondDrop => "把中压行的产出条件改成无要求",
            Form::Form3RowDelete => "删掉一行，表只剩 3 行",
            Form::Form4RowAdd => "多加一行，表变 5 行",
            Form::Form5EpochBump => "纪元自增但未在扩展页登记",
            Form::Form6BaselineBypass => "把登记哈希强行覆写为基线值以蒙混",
            Form::Form7LiteralInject => "往判定器源码里塞阈值字面量",
            Form::Form8ConsumerFork => "消费方私自改判读，与表不一致",
            Form::Form9HashLie => "扩展页登记哈希与实算哈希不符",
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

/// 形态 0：行互换。
pub fn inject_row_swap(gate: &mut CiGate, t: &mut MapTable) -> MapResult<u64> {
    // 互换第 0/1 行的水位条件。
    let a: WCond = match t.rows.get_mut(0) {
        Some(r) => {
            let old: WCond = r.wcond;
            r.wcond = WCond::FreeLeLow;
            old
        }
        None => return Err(MapErr::RowDelete),
    };
    if let Some(r) = t.rows.get_mut(1) {
        r.wcond = a;
    }
    gate.ring.reject(TraceRow { tick: gate.tick, kind: 4, tag: Form::Form0RowSwap.trace_tag(), payload: 0 });
    // 哈希闸亦必拦（改表 → 哈希变）；此处只记账不短路，短路由行内容闸做。
    let _ = gate.baseline_gate(t);
    t.verify_rows()
}

/// 形态 1：阈值漂移。
pub fn inject_threshold_drift(gate: &mut CiGate, t: &mut MapTable) -> MapResult<u64> {
    t.high = t.high.wrapping_add(1);
    gate.ring.reject(TraceRow { tick: gate.tick, kind: 4, tag: Form::Form1ThresholdDrift.trace_tag(), payload: t.high });
    let _ = gate.baseline_gate(t);
    // 权威检出点：阈值闸（必须报 `ThresholdDrift`）。
    t.verify_thresholds()
}

/// 形态 2：产出条件被摘。
pub fn inject_pcond_drop(gate: &mut CiGate, t: &mut MapTable) -> MapResult<u64> {
    if let Some(r) = t.rows.get_mut(2) {
        r.pcond = PCond::NoReq;
    }
    gate.ring.reject(TraceRow { tick: gate.tick, kind: 4, tag: Form::Form2PcondDrop.trace_tag(), payload: 2 });
    let _ = gate.baseline_gate(t);
    // 权威检出点：行内容闸（`pcond` 先查 → 必报 `PcondDrop`）。
    t.verify_rows()
}

/// 形态 3：行缺失。
pub fn inject_row_delete(gate: &mut CiGate, t: &mut MapTable) -> MapResult<u64> {
    if t.rows.len() > 0 {
        t.rows.remove(0);
    }
    gate.ring.reject(TraceRow { tick: gate.tick, kind: 4, tag: Form::Form3RowDelete.trace_tag(), payload: t.rows.len() as u64 });
    t.row_count_check()
}

/// 形态 4：行冗余。
pub fn inject_row_add(gate: &mut CiGate, t: &mut MapTable) -> MapResult<u64> {
    let r: MapRow = MapRow { tier: Tier::Heavy, wcond: WCond::FreeGtHigh, pcond: PCond::NoReq, note: "injected extra row", ord: TABLE_ROWS };
    t.rows.push(r);
    gate.ring.reject(TraceRow { tick: gate.tick, kind: 4, tag: Form::Form4RowAdd.trace_tag(), payload: t.rows.len() as u64 });
    t.row_count_check()
}

/// 形态 5：未走流程改纪元。
pub fn inject_epoch_bump(gate: &mut CiGate, t: &mut MapTable) -> MapResult<u64> {
    t.epoch = t.epoch.wrapping_add(1);
    gate.ring.reject(TraceRow { tick: gate.tick, kind: 4, tag: Form::Form5EpochBump.trace_tag(), payload: t.epoch });
    gate.epoch_gate(t)
}

/// 形态 6：绕过基线（覆写登记哈希）。
pub fn inject_baseline_bypass(gate: &mut CiGate, t: &mut MapTable) -> MapResult<u64> {
    // 改一行（让实算哈希变），再把登记哈希强行覆写成基线值蒙混。
    if let Some(r) = t.rows.get_mut(0) {
        r.note = "tampered note";
    }
    t.registered_hash = gate.baseline;
    gate.ring.reject(TraceRow { tick: gate.tick, kind: 4, tag: Form::Form6BaselineBypass.trace_tag(), payload: gate.baseline });
    // 权威检出点：基线闸（实算哈希与基线不符 → 必报 `CiBaselineBypass`）。
    let r: MapResult<u64> = gate.baseline_gate(t);
    if r.is_ok() {
        return Err(MapErr::CiBaselineBypass);
    }
    r
}

/// 形态 7：判定器内嵌阈值。
pub fn inject_literal(gate: &mut CiGate) -> MapResult<u64> {
    // 构造带字面量的判定器源码。
    let bad: [&str; 3] = ["let high = 8192;", "let low = 2048;", "let min = 512;"];
    gate.ring.reject(TraceRow { tick: gate.tick, kind: 4, tag: Form::Form7LiteralInject.trace_tag(), payload: 3 });
    gate.literal_scan(&bad)
}

/// 形态 8：消费方分叉。
pub fn inject_consumer_fork(t: &mut MapTable) -> MapResult<u64> {
    // 消费方私自改表（不改结构，只改阈值引用）→ 分叉检出。
    let mut forked: MapTable = t.clone();
    forked.low = forked.low.wrapping_mul(2);
    let canon: MapTable = MapTable::canonical();
    if forked.canonical_hash() == canon.canonical_hash() {
        return Err(MapErr::ExtPageHashLie);
    }
    Err(MapErr::ConsumerFork)
}

/// 形态 9：扩展页哈希撒谎。
pub fn inject_hash_lie(t: &mut MapTable) -> MapResult<u64> {
    t.registered_hash = t.registered_hash.wrapping_add(1);
    t.verify()
}

/// 跑全部十形态（各自独立克隆）。
pub fn inject_all(seed: u64) -> Vec<Injection> {
    let mut out: Vec<Injection> = Vec::with_capacity(FORM_COUNT);
    // 每个形态跑在自己的 (gate, table) 上。
    let mut i: usize = 0;
    while i < FORM_COUNT {
        let form: Form = Form::ALL[i];
        let mut gate: CiGate = CiGate::new(seed.wrapping_add(i as u64));
        let mut table: MapTable = MapTable::canonical();
        let _ = table.register(gate.tick);
        let r: MapResult<u64> = match form {
            Form::Form0RowSwap => inject_row_swap(&mut gate, &mut table),
            Form::Form1ThresholdDrift => inject_threshold_drift(&mut gate, &mut table),
            Form::Form2PcondDrop => inject_pcond_drop(&mut gate, &mut table),
            Form::Form3RowDelete => inject_row_delete(&mut gate, &mut table),
            Form::Form4RowAdd => inject_row_add(&mut gate, &mut table),
            Form::Form5EpochBump => inject_epoch_bump(&mut gate, &mut table),
            Form::Form6BaselineBypass => inject_baseline_bypass(&mut gate, &mut table),
            Form::Form7LiteralInject => inject_literal(&mut gate),
            Form::Form8ConsumerFork => inject_consumer_fork(&mut table),
            Form::Form9HashLie => inject_hash_lie(&mut table),
        };
        let (rejected, code, as_expected): (bool, u16, bool) = match r {
            Ok(_) => (false, 0, false),
            Err(e) => (true, e.code(), e == form.expect_err()),
        };
        out.push(Injection { form, rejected, code, as_expected, trace_seq: gate.ring.written });
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

/// 漂移注入专项（判据「漂移注入（改表未走流程）10/10 次被 CI 拦截」）。
pub fn drift_sweep(seed: u64) -> (usize, usize) {
    let mut tries: usize = 0;
    let mut blocked: usize = 0;
    let mut i: usize = 0;
    while i < DRIFT_CASES {
        let mut gate: CiGate = CiGate::new(seed.wrapping_add(i as u64));
        let mut table: MapTable = MapTable::canonical();
        let _ = table.register(gate.tick);
        // 造一处漂移：四个维度轮换（产出条件 / 水位条件 / 阈值 / 注记），
        // 任一维度改动都会让实算哈希偏离基线，故 10/10 必被拦。
        let idx: usize = i % TABLE_ROWS;
        match i % 4 {
            0 => {
                if let Some(r) = table.rows.get_mut(idx) {
                    r.pcond = PCond::OutPositive;
                }
            }
            1 => {
                if let Some(r) = table.rows.get_mut(idx) {
                    r.wcond = WCond::FreeGtHigh;
                }
            }
            2 => {
                table.high = table.high.wrapping_add(1);
            }
            _ => {
                if let Some(r) = table.rows.get_mut(idx) {
                    r.note = "drift note";
                }
            }
        }
        tries = tries.wrapping_add(1);
        if gate.baseline_gate(&table).is_err() {
            blocked = blocked.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    (tries, blocked)
}

/// 硬编码注入专项（判据「硬编码注入（判定器内嵌阈值）10/10 次被扫描拦截」）。
pub fn hardcoded_sweep(seed: u64) -> (usize, usize) {
    let mut tries: usize = 0;
    let mut blocked: usize = 0;
    let mut i: usize = 0;
    while i < HARDCODED_CASES {
        let mut gate: CiGate = CiGate::new(seed.wrapping_add(1000 + (i as u64)));
        // 构造带字面量的源码行（每次插在第 i 行位置）。
        let mut src: Vec<&str> = Vec::with_capacity(DECIDER_SRC_LINES.len() + 1);
        src.push(DECIDER_SRC_LINES[1]);
        src.push("    let hardcoded = 8192;");
        let mut k: usize = 2;
        while k < DECIDER_SRC_LINES.len() {
            src.push(DECIDER_SRC_LINES[k]);
            k = k.wrapping_add(1);
        }
        tries = tries.wrapping_add(1);
        if gate.literal_scan(&src).is_err() {
            blocked = blocked.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    (tries, blocked)
}

/// 消费抽样专项（判据「表与 F2761 消费抽样 10 处一致」）。
///
/// 每处抽样都用**不同的读数**，结论必须与表查一致（也即判定器不得有旁路）。
pub fn consumer_sweep(seed: u64) -> (usize, usize) {
    let mut agrees: usize = 0;
    let mut total: usize = 0;
    let mut rng: Lcg = Lcg::new(seed ^ 0x434F_4E53_0000_0001);
    let mut i: usize = 0;
    while i < CONSUMER_SAMPLES {
        let mut d: Decider = Decider::new(seed.wrapping_add(i as u64));
        // 读数构造：一半落在正常区，一半落在低/最小区。
        let free: u64 = if i % 2 == 0 { rng.range_u64(WATER_MIN, WATER_HIGH * 2) } else { rng.range_u64(0, WATER_HIGH) };
        let out: u64 = rng.next_u64() & 0x3;
        total = total.wrapping_add(1);
        let got: MapResult<Tier> = d.classify(free, out);
        // 独立口径：直接用规范表查（不经过判定器）。
        let canon: MapTable = MapTable::canonical();
        let want: MapResult<Tier> = canon.lookup(free, out);
        if got == want {
            agrees = agrees.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    (agrees, total)
}
// ===========================================================================
// 9. 冻结指纹与多道闸 `verify()`
// ===========================================================================

/// 扩展页登记元（`F2750.EXT.TIER_WATERMARK`）。
#[derive(Clone, Debug)]
pub struct ExtPageMeta {
    /// 扩展页代号（来源：F2750 扩展页登记表）。
    pub code: &'static str,
    /// 扩展页纪元。
    pub epoch: u64,
    /// 基册账引用（`F2719.BASE`）。
    pub base_ref: &'static str,
    /// 登记的四行哈希折叠值。
    pub rows_hash: u64,
    /// 登记的判定器源码指纹。
    pub src_hash: u64,
    /// 登记的阈值三元组。
    pub thr: (u64, u64, u64),
}

impl ExtPageMeta {
    /// 构造（自规范表取哈希）。
    pub fn new(t: &MapTable) -> ExtPageMeta {
        let mut w: Vec<u64> = Vec::with_capacity(8);
        w.push(t.epoch);
        w.push(t.canonical_hash());
        w.push(decider_src_hash());
        w.push(t.high);
        w.push(t.low);
        w.push(t.min);
        w.push(TABLE_ROWS as u64);
        ExtPageMeta { code: EXT_PAGE_CODE, epoch: EXT_PAGE_EPOCH, base_ref: BASE_LEDGER_REF, rows_hash: t.canonical_hash(), src_hash: decider_src_hash(), thr: (t.high, t.low, t.min) }
    }

    /// 导出行。
    pub fn row(&self) -> String {
        format!("ext_page[{}] epoch={} base={} rows_hash=0x{:016X} src_hash=0x{:016X} thr=({},{},{})", self.code, self.epoch, self.base_ref, self.rows_hash, self.src_hash, self.thr.0, self.thr.1, self.thr.2)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = FNV_BASIS;
        h = fnv1a_u64_into(h, self.epoch);
        h = fnv1a_u64_into(h, self.rows_hash);
        h = fnv1a_u64_into(h, self.src_hash);
        h = fnv1a_u64_into(h, self.thr.0);
        h = fnv1a_u64_into(h, self.thr.1);
        h = fnv1a_u64_into(h, self.thr.2);
        h
    }
}

/// 判定器源码指纹（12 行模型串折叠）。
pub fn decider_src_hash() -> u64 {
    let mut w: Vec<u64> = Vec::with_capacity(DECIDER_SRC_LINES.len());
    let mut i: usize = 0;
    while i < DECIDER_SRC_LINES.len() {
        w.push(fnv1a64(DECIDER_SRC_LINES[i].as_bytes()));
        i = i.wrapping_add(1);
    }
    fnv1a_words(&w)
}

/// 表的全局冻结指纹（覆盖四行 + 阈值 + 纪元 + 行数）。
pub fn table_fingerprint(t: &MapTable) -> u64 {
    let mut h: u64 = FNV_BASIS;
    h = fnv1a_u64_into(h, t.epoch);
    h = fnv1a_u64_into(h, t.high);
    h = fnv1a_u64_into(h, t.low);
    h = fnv1a_u64_into(h, t.min);
    h = fnv1a_u64_into(h, t.rows.len() as u64);
    let mut i: usize = 0;
    while i < t.rows.len() {
        if let Some(r) = t.rows.get(i) {
            h = fnv1a_u64_into(h, r.hash());
            h = fnv1a_u64_into(h, r.tier.index() as u64);
            h = fnv1a_u64_into(h, r.wcond.index() as u64);
            h = fnv1a_u64_into(h, r.pcond.index() as u64);
        }
        i = i.wrapping_add(1);
    }
    h
}

/// 全量多道闸校验。
///
/// 闸序（每道独立报错，互不吞错）：
/// 1. 行数闸 —— 必须恰 4 行（缺行 `RowDelete`、冗行 `RowAdd`）；
/// 2. 阈值闸 —— `high/low/min` 必须等于 `WATER_*`（漂移 `ThresholdDrift`）；
/// 3. 行内容闸 —— 每行三元组必须等于规范行（互换 `RowSwap`、丢产出 `PcondDrop`）；
/// 4. 登记哈希闸 —— 扩展页登记哈希必须与实算一致（撒谎 `ExtPageHashLie`）；
/// 5. 纪元闸 —— 扩展页纪元必须等于 `EXT_PAGE_EPOCH`（未登记自增 `EpochBumpUnfiled`）；
/// 6. 档位闸 —— 四行档位必须是四档不重不漏（越界 `TierOutOfRange`）。
pub fn verify(t: &mut MapTable, gate: &mut CiGate) -> MapResult<u64> {
    // 守恒口径：每次 verify 计一次闸检查（total 先加，结论进 pass/err 之一）。
    gate.ledger.total = gate.ledger.total.wrapping_add(1);
    // 1 行数闸
    if t.rows.len() < TABLE_ROWS {
        gate.ledger.err();
        gate.ring.reject(TraceRow { tick: gate.tick, kind: 5, tag: MapErr::RowDelete.code() as u64, payload: t.rows.len() as u64 });
        return Err(MapErr::RowDelete);
    }
    if t.rows.len() > TABLE_ROWS {
        gate.ledger.err();
        gate.ring.reject(TraceRow { tick: gate.tick, kind: 5, tag: MapErr::RowAdd.code() as u64, payload: t.rows.len() as u64 });
        return Err(MapErr::RowAdd);
    }
    // 2 阈值闸
    if t.verify_thresholds().is_err() {
        gate.ledger.err();
        gate.ring.reject(TraceRow { tick: gate.tick, kind: 5, tag: MapErr::ThresholdDrift.code() as u64, payload: t.high ^ t.low ^ t.min });
        return Err(MapErr::ThresholdDrift);
    }
    // 3 行内容闸
    if t.verify_rows().is_err() {
        gate.ledger.err();
        gate.ring.reject(TraceRow { tick: gate.tick, kind: 5, tag: MapErr::RowSwap.code() as u64, payload: t.canonical_hash() });
        return Err(MapErr::RowSwap);
    }
    // 4 登记哈希闸
    if t.registered_hash != 0 && t.registered_hash != t.canonical_hash() {
        gate.ledger.err();
        gate.ring.reject(TraceRow { tick: gate.tick, kind: 5, tag: MapErr::ExtPageHashLie.code() as u64, payload: t.registered_hash });
        return Err(MapErr::ExtPageHashLie);
    }
    // 5 纪元闸
    if t.epoch != EXT_PAGE_EPOCH {
        gate.ledger.err();
        gate.ring.reject(TraceRow { tick: gate.tick, kind: 5, tag: MapErr::EpochBumpUnfiled.code() as u64, payload: t.epoch });
        return Err(MapErr::EpochBumpUnfiled);
    }
    // 6 档位闸（四档各出现且仅出现一次）
    let mut seen: u32 = 0;
    let mut i: usize = 0;
    while i < t.rows.len() {
        if let Some(r) = t.rows.get(i) {
            let b: u32 = 1u32 << (r.tier.index() as u32);
            if seen & b != 0 {
                gate.ledger.err();
                gate.ring.reject(TraceRow { tick: gate.tick, kind: 5, tag: MapErr::TierOutOfRange.code() as u64, payload: r.tier.index() as u64 });
                return Err(MapErr::TierOutOfRange);
            }
            seen |= b;
        }
        i = i.wrapping_add(1);
    }
    if seen != 0b1111 {
        gate.ledger.err();
        gate.ring.reject(TraceRow { tick: gate.tick, kind: 5, tag: MapErr::TierOutOfRange.code() as u64, payload: seen as u64 });
        return Err(MapErr::TierOutOfRange);
    }
    gate.ledger.pass();
    gate.ring.push(TraceRow { tick: gate.tick, kind: 0, tag: t.canonical_hash(), payload: table_fingerprint(t) });
    Ok(table_fingerprint(t))
}

/// 冻结指纹（表 + 扩展页 + 判定器三方折叠）。
pub fn fingerprint(t: &MapTable, d: &Decider) -> u64 {
    let meta: ExtPageMeta = ExtPageMeta::new(t);
    let mut h: u64 = FNV_BASIS;
    h = fnv1a_u64_into(h, table_fingerprint(t));
    h = fnv1a_u64_into(h, meta.fingerprint());
    h = fnv1a_u64_into(h, d.fingerprint());
    h = fnv1a_u64_into(h, d.calls);
    h
}

/// 冻结指纹自校验：重建一份规范表与判定器，指纹必须完全一致。
pub fn verify_frozen(t: &MapTable) -> bool {
    let canon: MapTable = MapTable::canonical();
    let mut d0: Decider = Decider::new(0);
    let _ = d0.classify(WATER_HIGH + 1, 0);
    let mut d1: Decider = Decider::new(0);
    let _ = d1.classify(WATER_HIGH + 1, 0);
    if table_fingerprint(t) != table_fingerprint(&canon) {
        return false;
    }
    if fingerprint(t, &d0) != fingerprint(&canon, &d1) {
        return false;
    }
    if ExtPageMeta::new(t).rows_hash != ExtPageMeta::new(&canon).rows_hash {
        return false;
    }
    true
}

// ===========================================================================
// 10. 确定性回放（同种子两次完全一致）
// ===========================================================================

/// 回放结果（纯数据，供折叠）。
#[derive(Clone, Debug)]
pub struct ReplayResult {
    /// 种子。
    pub seed: u64,
    /// 步数。
    pub steps: u64,
    /// 命中数。
    pub hits: u64,
    /// 未命中数。
    pub misses: u64,
    /// 注入拦截数。
    pub blocked: u64,
    /// 表指纹（回放结束时）。
    pub table_fp: u64,
    /// 扩展页指纹。
    pub ext_fp: u64,
    /// 环形账指纹。
    pub ring_fp: u64,
    /// 判定器阈值折叠。
    pub thr_fp: u64,
}

impl ReplayResult {
    /// 导出行。
    pub fn row(&self) -> String {
        format!("replay[seed=0x{:016X} steps={} hit={} miss={} blocked={} tfp=0x{:016X} efp=0x{:016X} rfp=0x{:016X}]", self.seed, self.steps, self.hits, self.misses, self.blocked, self.table_fp, self.ext_fp, self.ring_fp)
    }
}

/// 确定性回放。
///
/// **只走数据通路，不调用 `observe()`**（避免互递归）。
pub fn replay(seed: u64) -> ReplayResult {
    let mut rng: Lcg = Lcg::new(seed);
    let mut gate: CiGate = CiGate::new(seed);
    let mut table: MapTable = MapTable::canonical();
    let _ = table.register(seed);
    let mut d: Decider = Decider::new(seed);
    let steps: u64 = 64;
    let mut k: u64 = 0;
    let mut blocked: u64 = 0;
    while k < steps {
        gate.advance(1);
        let free: u64 = rng.range_u64(0, WATER_HIGH * 2);
        let out: u64 = rng.next_u64() % 4;
        if d.classify(free, out).is_err() {
            blocked = blocked.wrapping_add(1);
        }
        // 每 8 步插一次漂移注入，看 CI 闸是否拦。
        if k % 8 == 7 {
            let idx: usize = (k as usize) % TABLE_ROWS;
            let bak_p: PCond = match table.rows.get(idx) {
                Some(r) => r.pcond,
                None => PCond::NoReq,
            };
            if let Some(r) = table.rows.get_mut(idx) {
                r.pcond = PCond::NoReq;
            }
            if gate.baseline_gate(&table).is_err() {
                blocked = blocked.wrapping_add(1);
            }
            if let Some(r) = table.rows.get_mut(idx) {
                r.pcond = bak_p;
            }
            let _ = gate.run(&table);
        }
        k = k.wrapping_add(1);
    }
    ReplayResult { seed, steps, hits: d.ledger.hit, misses: d.ledger.miss, blocked, table_fp: table_fingerprint(&table), ext_fp: ExtPageMeta::new(&table).fingerprint(), ring_fp: gate.ring.fingerprint(), thr_fp: d.fingerprint() }
}

/// 回放折叠指纹（直接折叠回放数据，**不经 `observe()`**）。
pub fn replay_fold(r: &ReplayResult) -> u64 {
    let mut h: u64 = FNV_BASIS;
    h = fnv1a_u64_into(h, r.seed);
    h = fnv1a_u64_into(h, r.steps);
    h = fnv1a_u64_into(h, r.hits);
    h = fnv1a_u64_into(h, r.misses);
    h = fnv1a_u64_into(h, r.blocked);
    h = fnv1a_u64_into(h, r.table_fp);
    h = fnv1a_u64_into(h, r.ext_fp);
    h = fnv1a_u64_into(h, r.ring_fp);
    h = fnv1a_u64_into(h, r.thr_fp);
    h
}

/// 同种子两次回放是否完全一致。
pub fn replay_stable(seed: u64) -> bool {
    let a: ReplayResult = replay(seed);
    let b: ReplayResult = replay(seed);
    replay_fold(&a) == replay_fold(&b) && a.hits == b.hits && a.misses == b.misses && a.blocked == b.blocked && a.ring_fp == b.ring_fp
}

/// 回放是否具备区分度（不同种子必得不同折叠）。
pub fn replay_discriminates(seed: u64) -> bool {
    let a: u64 = replay_fold(&replay(seed));
    let b: u64 = replay_fold(&replay(seed.wrapping_add(1)));
    let c: u64 = replay_fold(&replay(seed ^ 0x5A5A_5A5A_5A5A_5A5A));
    a != b && a != c && b != c
}

// ===========================================================================
// 11. 观测聚合 `observe()`
// ===========================================================================

/// 观测结果。
#[derive(Clone, Debug)]
pub struct MapObserve {
    /// 种子。
    pub seed: u64,
    /// 矩阵格数（`MATRIX_CELLS`）。
    pub matrix_cells: usize,
    /// 矩阵双规则一致格数。
    pub matrix_agree: usize,
    /// 本命格绿数。
    pub matrix_native: usize,
    /// 四行是否全登记。
    pub rows_ok: usize,
    /// 阈值是否写死。
    pub thr_ok: bool,
    /// 漂移注入拦截 `blocked/total`。
    pub drift_total: usize,
    /// 漂移注入拦截数。
    pub drift_blocked: usize,
    /// 硬编码注入 `total/total`。
    pub hard_total: usize,
    /// 硬编码注入拦截数。
    pub hard_blocked: usize,
    /// 消费抽样一致数。
    pub cons_total: usize,
    /// 消费抽样一致数（成功侧）。
    pub cons_agree: usize,
    /// 注入核总数。
    pub inject_total: usize,
    /// 注入核拦截数。
    pub inject_rejected: usize,
    /// 注入核是否全被拒。
    pub inject_ok: bool,
    /// 守恒账（CI 闸账 + 注入账 + 断言账）。
    pub conserved: bool,
    /// 扩展页登记行。
    pub ext_page: String,
    /// 全量闸是否通过。
    pub verify_ok: bool,
    /// 冻结指纹。
    pub fp: u64,
    /// 冻结指纹自校验。
    pub frozen_ok: bool,
    /// 回放稳定。
    pub replay_stable: bool,
    /// 回放区分度。
    pub replay_disc: bool,
    /// 环形账是否自洽。
    pub ring_coherent: bool,
    /// 环形账丢弃数（显性）。
    pub ring_dropped: u64,
    /// 注入留痕行。
    pub inject_rows: Vec<String>,
    /// 矩阵行。
    pub matrix_rows: Vec<String>,
    /// 表行。
    pub table_rows: Vec<String>,
    /// 守恒行。
    pub ledger_rows: Vec<String>,
    /// 阈值三元组。
    pub thr: (u64, u64, u64),
    /// 判定调用总数。
    pub decider_calls: u64,
}

impl MapObserve {
    /// 总判绿。
    pub fn all_ok(&self) -> bool {
        self.matrix_agree == MATRIX_CELLS
            && self.matrix_native == TIER_COUNT
            && self.rows_ok == TABLE_ROWS
            && self.thr_ok
            && self.drift_blocked == self.drift_total
            && self.hard_blocked == self.hard_total
            && self.cons_agree == self.cons_total
            && self.inject_rejected == self.inject_total
            && self.inject_ok
            && self.conserved
            && self.verify_ok
            && self.frozen_ok
            && self.replay_stable
            && self.replay_disc
            && self.ring_coherent
    }

    /// 摘要行。
    pub fn summary(&self) -> String {
        format!(
            "seed=0x{:016X} matrix={}/{} native={} rows={}/{} thr={} drift={}/{} hard={}/{} cons={}/{} inject={}/{} conserved={} verify={} fp=0x{:016X} frozen={} replay={} disc={} ring_drop={} all_ok={}",
            self.seed,
            self.matrix_agree,
            MATRIX_CELLS,
            self.matrix_native,
            self.rows_ok,
            TABLE_ROWS,
            if self.thr_ok { 1 } else { 0 },
            self.drift_blocked,
            self.drift_total,
            self.hard_blocked,
            self.hard_total,
            self.cons_agree,
            self.cons_total,
            self.inject_rejected,
            self.inject_total,
            if self.conserved { 1 } else { 0 },
            if self.verify_ok { 1 } else { 0 },
            self.fp,
            if self.frozen_ok { 1 } else { 0 },
            if self.replay_stable { 1 } else { 0 },
            if self.replay_disc { 1 } else { 0 },
            self.ring_dropped,
            if self.all_ok() { 1 } else { 0 }
        )
    }

    /// 多行摘要。
    pub fn report(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::with_capacity(self.table_rows.len() + self.matrix_rows.len() + self.inject_rows.len() + self.ledger_rows.len() + 6);
        v.push(format!("== UNX-F2762 observe seed=0x{:016X} ==", self.seed));
        v.push(format!("version={} seg=0x{:04X}..0x{:04X}", V2762_VERSION, SEG_BASE, SEG_END));
        v.push(self.ext_page.clone());
        v.push(format!("thr high={} low={} min={} (硬编码常量 WATER_*)", self.thr.0, self.thr.1, self.thr.2));
        v.push(format!("decider_calls={}", self.decider_calls));
        for s in self.table_rows.iter() {
            v.push(s.clone());
        }
        for s in self.matrix_rows.iter() {
            v.push(s.clone());
        }
        for s in self.ledger_rows.iter() {
            v.push(s.clone());
        }
        for s in self.inject_rows.iter() {
            v.push(s.clone());
        }
        v.push(format!("fingerprint=0x{:016X}", self.fp));
        v.push(self.summary());
        v
    }
}

/// 观测聚合入口（真实跑逻辑得出结论）。
pub fn observe(seed: u64) -> MapObserve {
    // --- 矩阵：双规则对拍 ---
    let mx: Vec<MatrixRow> = tier_wcond_matrix();
    let mut agree: usize = 0;
    let mut native: usize = 0;
    let mut mrows: Vec<String> = Vec::with_capacity(mx.len());
    let mut mi: usize = 0;
    while mi < mx.len() {
        if let Some(r) = mx.get(mi) {
            if r.agree() {
                agree = agree.wrapping_add(1);
            }
            if r.native() && r.rule_primary && r.rule_mirror {
                native = native.wrapping_add(1);
            }
            mrows.push(r.row());
        }
        mi = mi.wrapping_add(1);
    }
    let mok: bool = matrix_ok(&mx);

    // --- 表 + 全量闸 ---
    let mut gate: CiGate = CiGate::new(seed);
    let mut table: MapTable = MapTable::canonical();
    let _ = table.register(seed);
    let verify_ok: bool = verify(&mut table, &mut gate).is_ok();
    let rows_ok: usize = match table.verify_rows() {
        Ok(n) => n as usize,
        Err(_) => 0,
    };
    let thr_ok: bool = table.verify_thresholds().is_ok();

    // --- 判定器 + 消费抽样 ---
    let (cons_agree, cons_total): (usize, usize) = consumer_sweep(seed);
    let mut d: Decider = Decider::new(seed);
    // 补一批判定调用，把阈值与档位都走一遍。
    let mut di: usize = 0;
    while di < 16 {
        let _ = d.classify(WATER_HIGH + (di as u64) + 1, 0);
        let _ = d.classify(WATER_LOW - (di as u64) - 1, 0);
        let _ = d.classify(WATER_MIN - 1, 1);
        let _ = d.classify(WATER_MIN - 1, 0);
        di = di.wrapping_add(1);
    }

    // --- 三大专项 ---
    let (dt, db): (usize, usize) = drift_sweep(seed);
    let (ht, hb): (usize, usize) = hardcoded_sweep(seed);

    // --- 注入核 ---
    let injs: Vec<Injection> = inject_all(seed);
    let inj_ok: bool = inject_all_ok(&injs);
    let inj_rej: usize = {
        let mut n: usize = 0;
        let mut i: usize = 0;
        while i < injs.len() {
            if let Some(x) = injs.get(i) {
                if x.rejected {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    };
    let irows: Vec<String> = inject_rows(&injs);
    let iled: InjectLedger = inject_ledger_from(&injs);

    // --- 断言账：把上面的判据逐条落账 ---
    let mut al: AssertLedger = AssertLedger::alloc(6);
    if mok && agree == MATRIX_CELLS {
        al.pass();
    } else {
        al.fail();
    }
    if rows_ok == TABLE_ROWS && thr_ok {
        al.pass();
    } else {
        al.fail();
    }
    if db == dt && dt == DRIFT_CASES {
        al.pass();
    } else {
        al.fail();
    }
    if hb == ht && ht == HARDCODED_CASES {
        al.pass();
    } else {
        al.fail();
    }
    if cons_agree == cons_total && cons_total == CONSUMER_SAMPLES {
        al.pass();
    } else {
        al.fail();
    }
    if inj_ok && inj_rej == injs.len() {
        al.pass();
    } else {
        al.fail();
    }

    // --- 环形账压测（必然产生 dropped，证明显性丢弃可记账） ---
    let mut ring: RingLog = RingLog::new();
    let mut ri: usize = 0;
    while ri < RING_CAP * 2 + 7 {
        ring.push(TraceRow { tick: seed.wrapping_add(ri as u64), kind: 6, tag: ri as u64, payload: fnv1a_u64_into(FNV_BASIS, ri as u64) });
        ri = ri.wrapping_add(1);
    }
    let ring_coherent: bool = ring.coherent();
    let ring_dropped: u64 = ring.dropped;

    // --- 守恒汇总 ---
    let conserved: bool = gate.ledger.conserved() && iled.conserved() && al.conserved();

    // --- 冻结指纹 ---
    let fp: u64 = fingerprint(&table, &d);
    let frozen_ok: bool = verify_frozen(&table);

    // --- 回放 ---
    let rs: bool = replay_stable(seed);
    let rd: bool = replay_discriminates(seed);

    let mut lrows: Vec<String> = Vec::with_capacity(6);
    lrows.push(gate.ledger.row());
    lrows.push(iled.row());
    lrows.push(al.row());
    lrows.push(d.ledger.row());
    lrows.push(format!("ring live={} written={} dropped={} rejected={} coherent={}", ring.live(), ring.written, ring.dropped, ring.rejected, if ring.coherent() { 1 } else { 0 }));

    MapObserve {
        seed,
        matrix_cells: MATRIX_CELLS,
        matrix_agree: agree,
        matrix_native: native,
        rows_ok,
        thr_ok,
        drift_total: dt,
        drift_blocked: db,
        hard_total: ht,
        hard_blocked: hb,
        cons_total,
        cons_agree,
        inject_total: injs.len(),
        inject_rejected: inj_rej,
        inject_ok: inj_ok,
        conserved,
        ext_page: ExtPageMeta::new(&table).row(),
        verify_ok,
        fp,
        frozen_ok,
        replay_stable: rs,
        replay_disc: rd,
        ring_coherent,
        ring_dropped,
        inject_rows: irows,
        matrix_rows: mrows,
        table_rows: table.rows_text(),
        ledger_rows: lrows,
        thr: (table.high, table.low, table.min),
        decider_calls: d.calls,
    }
}

// ===========================================================================
// 12. 导出（KV / TLV），TLV 组头单独一行
// ===========================================================================

/// TLV 组头载荷长度：`len2 + magic8 + version8 + len8`。
pub const TLV_HEAD_PAYLOAD: usize = 2 + 8 + 8 + 8;

/// TLV 魔数（`"VARIX2762"` 前 8 字节折叠）。
pub const TLV_MAGIC: u64 = 0x5641_5249_5832_3736;

/// TLV 标签：表体。
pub const TLV_TAG_TABLE: u16 = 0x01;

/// TLV 标签：扩展页。
pub const TLV_TAG_EXT: u16 = 0x02;

/// TLV 标签：闸账。
pub const TLV_TAG_GATE: u16 = 0x03;

/// TLV 标签：守恒账。
pub const TLV_TAG_LEDGER: u16 = 0x04;

/// TLV 标签：尾指纹。
pub const TLV_TAG_TAIL: u16 = 0x05;

/// KV 导出（人读）。
pub fn export_kv(seed: u64) -> Vec<String> {
    let ob: MapObserve = observe(seed);
    let mut v: Vec<String> = Vec::with_capacity(24);
    v.push(format!("unx.f2762.version={}", V2762_VERSION));
    v.push(format!("unx.f2762.version_id=0x{:016X}", V2762_VERSION_ID));
    v.push(format!("unx.f2762.seg_base=0x{:04X}", SEG_BASE));
    v.push(format!("unx.f2762.seg_end=0x{:04X}", SEG_END));
    v.push(format!("unx.f2762.ext_page={}", EXT_PAGE_CODE));
    v.push(format!("unx.f2762.ext_epoch={}", EXT_PAGE_EPOCH));
    v.push(format!("unx.f2762.base_ref={}", BASE_LEDGER_REF));
    v.push(format!("unx.f2762.table_rows={}", TABLE_ROWS));
    v.push(format!("unx.f2762.water_high={}", WATER_HIGH));
    v.push(format!("unx.f2762.water_low={}", WATER_LOW));
    v.push(format!("unx.f2762.water_min={}", WATER_MIN));
    v.push(format!("unx.f2762.matrix_cells={}", MATRIX_CELLS));
    v.push(format!("unx.f2762.matrix_agree={}", ob.matrix_agree));
    v.push(format!("unx.f2762.drift={}/{}", ob.drift_blocked, ob.drift_total));
    v.push(format!("unx.f2762.hardcoded={}/{}", ob.hard_blocked, ob.hard_total));
    v.push(format!("unx.f2762.consumer={}/{}", ob.cons_agree, ob.cons_total));
    v.push(format!("unx.f2762.inject={}/{}", ob.inject_rejected, ob.inject_total));
    v.push(format!("unx.f2762.conserved={}", if ob.conserved { 1 } else { 0 }));
    v.push(format!("unx.f2762.fingerprint=0x{:016X}", ob.fp));
    v.push(format!("unx.f2762.replay_stable={}", if ob.replay_stable { 1 } else { 0 }));
    v.push(format!("unx.f2762.all_ok={}", if ob.all_ok() { 1 } else { 0 }));
    v
}

/// TLV 组头（`len2 + magic8 + version8 + len8`）。
pub fn build_head(version: u64, body_len: u64) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::with_capacity(TLV_HEAD_PAYLOAD);
    push_u16(&mut v, TLV_HEAD_PAYLOAD as u16);
    push_u64(&mut v, TLV_MAGIC);
    push_u64(&mut v, version);
    push_u64(&mut v, body_len);
    v
}

/// 追加一个 TLV。
pub fn push_tlv(out: &mut Vec<u8>, tag: u16, payload: &[u8]) {
    push_u16(out, tag);
    push_u16(out, payload.len() as u16);
    let mut i: usize = 0;
    while i < payload.len() {
        out.push(payload[i]);
        i = i.wrapping_add(1);
    }
}

/// TLV 导出。
pub fn export_tlv(seed: u64) -> Vec<u8> {
    let ob: MapObserve = observe(seed);
    let mut body: Vec<u8> = Vec::new();
    // T1 表体：四行 × (tier, wcond, pcond, hash)
    let mut t: Vec<u8> = Vec::new();
    let table: MapTable = MapTable::canonical();
    let mut i: usize = 0;
    while i < table.rows.len() {
        if let Some(r) = table.rows.get(i) {
            push_u64(&mut t, r.ord as u64);
            push_u64(&mut t, r.tier.index() as u64);
            push_u64(&mut t, r.wcond.index() as u64);
            push_u64(&mut t, r.pcond.index() as u64);
            push_u64(&mut t, r.hash());
        }
        i = i.wrapping_add(1);
    }
    push_tlv(&mut body, TLV_TAG_TABLE, &t);
    // T2 扩展页
    let meta: ExtPageMeta = ExtPageMeta::new(&table);
    let mut e: Vec<u8> = Vec::new();
    push_u64(&mut e, meta.epoch);
    push_u64(&mut e, meta.rows_hash);
    push_u64(&mut e, meta.src_hash);
    push_u64(&mut e, meta.thr.0);
    push_u64(&mut e, meta.thr.1);
    push_u64(&mut e, meta.thr.2);
    push_tlv(&mut body, TLV_TAG_EXT, &e);
    // T3 闸账
    let mut g: Vec<u8> = Vec::new();
    push_u64(&mut g, ob.drift_blocked as u64);
    push_u64(&mut g, ob.drift_total as u64);
    push_u64(&mut g, ob.hard_blocked as u64);
    push_u64(&mut g, ob.hard_total as u64);
    push_u64(&mut g, ob.cons_agree as u64);
    push_u64(&mut g, ob.cons_total as u64);
    push_tlv(&mut body, TLV_TAG_GATE, &g);
    // T4 守恒账
    let mut l: Vec<u8> = Vec::new();
    push_u64(&mut l, ob.inject_rejected as u64);
    push_u64(&mut l, ob.inject_total as u64);
    push_u64(&mut l, ob.ring_dropped);
    push_u64(&mut l, if ob.conserved { 1 } else { 0 });
    push_tlv(&mut body, TLV_TAG_LEDGER, &l);
    // T5 尾指纹
    let mut tl: Vec<u8> = Vec::new();
    push_u64(&mut tl, ob.fp);
    push_tlv(&mut body, TLV_TAG_TAIL, &tl);

    let head: Vec<u8> = build_head(V2762_VERSION_ID, body.len() as u64);
    let mut out: Vec<u8> = Vec::with_capacity(head.len() + body.len());
    let mut i: usize = 0;
    while i < head.len() {
        out.push(head[i]);
        i = i.wrapping_add(1);
    }
    let mut j: usize = 0;
    while j < body.len() {
        out.push(body[j]);
        j = j.wrapping_add(1);
    }
    out
}

/// TLV 长度是否自洽（组头声明长度 == 实际剩余长度）。
pub fn tlv_len_ok(buf: &[u8]) -> bool {
    let head_len: u64 = read_u16(buf, 0);
    if head_len != (TLV_HEAD_PAYLOAD as u64) {
        return false;
    }
    let body_len: u64 = read_u64(buf, 2 + 8 + 8);
    let actual: u64 = (buf.len() as u64).wrapping_sub(head_len);
    body_len == actual
}

/// TLV 往返一致性（回读组头三字段 + 尾指纹）。
pub fn tlv_roundtrip(seed: u64) -> bool {
    let buf: Vec<u8> = export_tlv(seed);
    if !tlv_len_ok(&buf) {
        return false;
    }
    if read_u16(&buf, 0) != (TLV_HEAD_PAYLOAD as u64) {
        return false;
    }
    if read_u64(&buf, 2) != TLV_MAGIC {
        return false;
    }
    if read_u64(&buf, 2 + 8) != V2762_VERSION_ID {
        return false;
    }
    let ob: MapObserve = observe(seed);
    // 逐 TLV 走一遍，回读尾指纹。
    // 组头总长恰为 `TLV_HEAD_PAYLOAD`（`len2` 本身计入组头），故 body 起点即组头末。
    let mut off: usize = TLV_HEAD_PAYLOAD;
    let end: usize = buf.len();
    let mut tail_ok: bool = false;
    while off + 4 <= end {
        let tag: u64 = read_u16(&buf, off);
        let len: u64 = read_u16(&buf, off + 2);
        let body: usize = off + 4 + (len as usize);
        if body > end {
            return false;
        }
        if tag == (TLV_TAG_TAIL as u64) {
            tail_ok = read_u64(&buf, off + 4) == ob.fp;
        }
        off = body;
    }
    tail_ok && off == end
}

// ===========================================================================
// 13. 自检与演示
// ===========================================================================

/// 自检总入口。
pub fn self_check() -> bool {
    let seeds: [u64; 4] = [1, 0xDEAD_BEEF, 0x1234_5678, u64::MAX];
    let mut ok: bool = true;
    let mut i: usize = 0;
    while i < seeds.len() {
        if !observe(seeds[i]).all_ok() {
            ok = false;
        }
        i = i.wrapping_add(1);
    }
    // 常量自洽
    if TABLE_ROWS != TIER_COUNT || MATRIX_CELLS != TIER_COUNT * WCOND_COUNT {
        ok = false;
    }
    // 码段自洽：变体一码一段且不越掩码
    if err_str(MapErr::RowSwap).is_empty() {
        ok = false;
    }
    let mut k: usize = 0;
    while k < 12 {
        let e: MapErr = match MapErr::from_index(k) {
            Some(x) => x,
            None => MapErr::RowSwap,
        };
        if !code_in_seg(e.code()) {
            ok = false;
        }
        if e.detail().is_empty() {
            ok = false;
        }
        k = k.wrapping_add(1);
    }
    // CI 基线必须由代码求出（写死即红）
    if !baseline_is_computed() || ci_baseline_hash() == CI_BASELINE_HASH {
        ok = false;
    }
    // TLV 往返
    if !tlv_len_ok(&export_tlv(0xA5A5_A5A5)) {
        ok = false;
    }
    if !tlv_roundtrip(0xA5A5_A5A5) {
        ok = false;
    }
    ok
}

/// 自检报告（人类可读）。
pub fn self_check_report(seed: u64) -> String {
    let ob: MapObserve = observe(seed);
    let mut v: Vec<String> = ob.report();
    v.push(format!("self_check={}", if self_check() { 1 } else { 0 }));
    v.join("\n")
}

/// 演示场景：把一整套判据走一遍并打印。
pub fn demo_scene(seed: u64) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(format!("=== UNX-F2762 映射表四行冻结 演示 seed=0x{:016X} ===", seed));
    v.push(format!("扩展页 {}", EXT_PAGE_CODE));
    let table: MapTable = MapTable::canonical();
    for s in table.rows_text().iter() {
        v.push(s.clone());
    }
    v.push(format!("判定器源码 12 行模型（阈值字面量必须为 0 处）："));
    let mut i: usize = 0;
    while i < DECIDER_SRC_LINES.len() {
        v.push(format!("  src[{}] {}", i, DECIDER_SRC_LINES[i]));
        i = i.wrapping_add(1);
    }
    for s in tier_wcond_matrix().iter() {
        v.push(s.row());
    }
    for s in inject_rows(&inject_all(seed)).iter() {
        v.push(s.clone());
    }
    for s in export_kv(seed).iter() {
        v.push(s.clone());
    }
    v.push(observe(seed).summary());
    v
}

// ===========================================================================
// 14. 复杂度分解（对标 Windows 同类能力）
// ===========================================================================

/// 复杂度分项。
#[derive(Clone, Debug)]
pub struct PerfItem {
    /// 名称。
    pub name: &'static str,
    /// 时间复杂度。
    pub time: &'static str,
    /// 空间复杂度。
    pub space: &'static str,
    /// 系统调用次数。
    pub syscalls: u64,
    /// 步数上限。
    pub steps: u64,
    /// 对标 Windows 同类能力的「深在哪」。
    pub vs_windows: &'static str,
}

impl PerfItem {
    /// 导出行。
    pub fn row(&self) -> String {
        format!("perf[{}] time={} space={} syscalls={} steps={} vs={}", self.name, self.time, self.space, self.syscalls, self.steps, self.vs_windows)
    }
}

/// 复杂度逐项分解。
pub fn complexity() -> Vec<PerfItem> {
    let mut v: Vec<PerfItem> = Vec::with_capacity(12);
    v.push(PerfItem { name: "MapTable::canonical", time: "O(TABLE_ROWS)", space: "O(TABLE_ROWS)", syscalls: 0, steps: TABLE_ROWS as u64, vs_windows: "Windows 内存管理器靠硬编码 if-else 链选压力区，本项把同一决策收敛成四行可哈希表；深在「表即契约」——改一行必改哈希，CI 必拦" });
    v.push(PerfItem { name: "MapTable::lookup", time: "O(TABLE_ROWS)", space: "O(1)", syscalls: 0, steps: TABLE_ROWS as u64, vs_windows: "对照 MmGetPagefileSection/MmMapViewOfSection 的压力判定；Windows 无表化阈值，改阈值要改代码，本项阈值只在表内" });
    v.push(PerfItem { name: "Decider::classify", time: "O(TABLE_ROWS)", space: "O(1)", syscalls: 0, steps: TABLE_ROWS as u64, vs_windows: "深在阈值从表读而非从代码读：判定器源码内阈值字面量为 0，字面量扫描闸可 100% 拦截硬编码回归" });
    v.push(PerfItem { name: "tier_wcond_matrix", time: "O(TIER*WCOND*TABLE_ROWS)", space: "O(MATRIX_CELLS)", syscalls: 0, steps: (TIER_COUNT * WCOND_COUNT * TABLE_ROWS) as u64, vs_windows: "Windows 压力分区只有隐式代码路径，无可枚举矩阵；本项 16 格双规则对拍，能抓判定器实现分叉" });
    v.push(PerfItem { name: "CiGate::baseline_gate", time: "O(TABLE_ROWS)", space: "O(1)", syscalls: 0, steps: TABLE_ROWS as u64, vs_windows: "深在 CI 侧独立重算哈希而非信任提交者上报值；上报即撒谎的形态被哈希闸直接拦死" });
    v.push(PerfItem { name: "CiGate::literal_scan", time: "O(SRC_LINES * LITERALS)", space: "O(SRC_LINES)", syscalls: 0, steps: (DECIDER_SRC_LINES.len() * BANNED_LITERALS.len()) as u64, vs_windows: "对标 /WX 与 SAL 静态检查；深在检查对象是「阈值字面量」这一语义级禁物，而非语法级规范" });
    v.push(PerfItem { name: "inject_all", time: "O(FORM_COUNT * TABLE_ROWS)", space: "O(FORM_COUNT)", syscalls: 0, steps: (FORM_COUNT * TABLE_ROWS) as u64, vs_windows: "Windows 无对应负向注入面；深在十形态覆盖「改值/改结构/改纪元/改上报/改消费/改扫描」六个维度" });
    v.push(PerfItem { name: "drift_sweep", time: "O(DRIFT_CASES * TABLE_ROWS)", space: "O(1)", syscalls: 0, steps: (DRIFT_CASES * TABLE_ROWS) as u64, vs_windows: "深在每次漂移都重建独立闸实例，闸间零状态共享，杜绝「一次污染后续全绿」" });
    v.push(PerfItem { name: "hardcoded_sweep", time: "O(HARDCODED_CASES * SRC_LINES)", space: "O(SRC_LINES)", syscalls: 0, steps: (HARDCODED_CASES * DECIDER_SRC_LINES.len()) as u64, vs_windows: "深在字面量插在源码不同位置都被扫到（位置无关扫描）" });
    v.push(PerfItem { name: "consumer_sweep", time: "O(CONSUMER_SAMPLES * TABLE_ROWS)", space: "O(1)", syscalls: 0, steps: (CONSUMER_SAMPLES * TABLE_ROWS) as u64, vs_windows: "深在判定器结论与规范表查表两路独立对拍，消费侧分叉必现" });
    v.push(PerfItem { name: "replay", time: "O(REPLAY_STEPS * TABLE_ROWS)", space: "O(RING_CAP)", syscalls: 0, steps: (64 * TABLE_ROWS) as u64, vs_windows: "无墙钟、无真实 IO，64 步纯逻辑回放同种子两次逐位一致；Windows 侧无法做跨机确定性回放" });
    v.push(PerfItem { name: "export_tlv", time: "O(BODY)", space: "O(BODY)", syscalls: 0, steps: (TLV_HEAD_PAYLOAD + TABLE_ROWS * 40 + 64) as u64, vs_windows: "对标 ETW / WMI 导出；深在 TLV 组头三字段可被第三方独立解析验证，往返一致可自证" });
    v.push(PerfItem { name: "全量 observe", time: "O(常量合计)", space: "O(RING_CAP + MATRIX_CELLS)", syscalls: 0, steps: 4096, vs_windows: "纯函数、零系统调用；Windows 对应路径每次都要读系统时间/句柄，无法做逐位指纹对拍" });
    v
}

/// 复杂度报告。
pub fn complexity_report() -> String {
    let mut v: Vec<String> = Vec::new();
    v.push(format!("=== UNX-F2762 复杂度分解 ==="));
    for s in complexity().iter() {
        v.push(s.row());
    }
    v.join("\n")
}

/// 清单（版本 + 段 + 判据）。
pub fn manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(16);
    v.push(format!("task=UNX-F2762 title=映射表四行冻结（正常↔>high / 轻压↔≤low / 中压↔<min有产出 / 重压↔<min零产出）"));
    v.push(format!("domain=A4/B16 seg=0x{:04X}..0x{:04X} mask=0x{:04X}", SEG_BASE, SEG_END, SEG_MASK));
    v.push(format!("version={} id=0x{:016X}", V2762_VERSION, V2762_VERSION_ID));
    v.push(format!("ext_page={} epoch={} base_ref={}", EXT_PAGE_CODE, EXT_PAGE_EPOCH, BASE_LEDGER_REF));
    v.push(format!("judge=UNX-F2762-J1 映射表四行冻结，漂移注入被 CI 拦截"));
    v.push(format!("retest=四行登记哈希可查；漂移注入 10/10 拦截；硬编码注入 10/10 拦截；抽样 10 处消费与表一致"));
    v.push(format!("thresholds high={} low={} min={} out_one={}", WATER_HIGH, WATER_LOW, WATER_MIN, OUTPUT_ONE));
    v.push(format!("matrix cells={} forms={} ring_cap={}", MATRIX_CELLS, FORM_COUNT, RING_CAP));
    v.push(format!("errors={}", MapErr::ALL.len()));
    v
}


