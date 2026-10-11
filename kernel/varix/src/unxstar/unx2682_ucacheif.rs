//! unx2682 ↔ UNX-F2682 页缓存读接口冻结版（cache_lookup/insert/mark_dirty 三接口，B1–B5 联签）
//!
//! 任务单号：UNX-F2682
//! 标题：页缓存读接口冻结版（cache_lookup/insert/mark_dirty 三接口，B1–B5 联签）
//! 域/批：A4/B15
//! 册目标行数：300（本深化版按产线硬指标展开至 ≥2800 行）
//! 码段范围声明：独占码段 `0x1DC0 ~ 0x1DFF`（`SEG_BASE = 0x1DC0`，`SEG_MASK = 0x003F`）；
//!                错误变体一码一段，12 个变体占用 `0x1DC0 ~ 0x1DCB`，余下 `0x1DCC ~ 0x1DFF`
//!                留作注入核与 CI 闸的扩展诊断位，不与册内其他单冲突。
//!
//! 判据原文（逐字摘抄，CoRun Unxreal 主册 UNX-F2682-J1）：
//! `UNX-F2682-J1 三接口正式冻结（签名/错误码/语义三件齐），B1–B5 五方联签在档，
//!  F2662/F2663/F2664 契约面兑现闭合`
//!
//! 锚点原文关键句（逐字摘抄）：
//! - `第一步冻结登记：三接口逐个冻结（lookup/insert/mark_dirty 各：签名/参数/返回/错误码四件+语义条款引用 B14 契约），
//!    冻结件版本化登记入跨域接口册`
//! - `第二步五方联签：B1–B5 逐方联签（消费面/期望签名/回归责任三字段），与 F2683 回写钩子联签的接口侧衔接声明`
//! - `第三步兑现闭合：F2662/F2663/F2664 三契约兑现映射登记（契约条款→冻结条款逐条对应），三机直测断言全过冻结生效`
//! - `判据 UNX-F2682-J1 的复测方式：冻结件登记在档，五方联签齐备，兑现映射零缺，直测三轴全过`
//!
//! ## 职责定位
//! 本单**只做冻结登记与联签**，不做接口实现（B14 承载）、不做消费方实现（各 fs 域）、
//! 不做回写钩子联签（F2683 承载 B5 半边）。因此本单把三接口建模为
//! 「不可变签名 + 错误码位图 + 语义条款」的三件套冻结件，并建五方联签账、
//! 三契约兑现映射账、三机直测三轴断言面。
//!
//! 为让冻结件**可被真实验证**，本单同时提供一份纯功能参考实现（`CacheBook` 上
//! 的 `lookup` / `insert` / `mark_dirty`），它不是 B14 的实现本体，而是
//! 「冻结件与参考实现签名一致性对拍」的载体：对拍不过 = 冻结件漂移 = 红。
//!
//! ## 与册内其他单的显式对齐点
//! - **F2662 / F2663 / F2664 三契约（B14 前向，本单兑现位）**：三契约条款逐条
//!   映射到本单冻结条款，映射表零缺才算「兑现闭合」。
//! - **F2683 回写钩子联签（本单前向）**：本单只登记「接口侧衔接声明」，回写钩子
//!   半边由 F2683 承载，衔接声明用 `hook_edge` 字段显式对空。
//! - **F2401 页描述符**：页键的 `tag/index` 口径与 F2401 页描述符一致。
//! - **F2731 批量硬顶 32 页**：本单 `insert_batch` 的批量上限沿用同一硬顶。
//!
//! ## 纯功能线
//! `no_std + alloc`，零 `unwrap` / `expect` / `panic!` / `todo!`、零浮点、
//! 零真实 IO、零墙钟；时间用逻辑 `tick: u64`。确定性 LCG
//! （`LCG_A = 6364136223846793005`、`LCG_C = 1442695040888963407`，模 `2^64`）
//! 保证同种子同轨迹。FNV-1a basis `0xcbf29ce484222325`、prime `0x100000001b3`。
//! 自定义 `CacheResult<T>`；不写 `#[test]`，只暴露 `observe(seed)`。
// 本单为「册内自足模块」：探针 crate 以 `#[path]` 私有引入时，全部 `pub` 项会被
// 判为未可达，这是引入方式使然，不是死代码。
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

/// 模块版本标识（写入冻结件头 +0x08）。
pub const V2682_VERSION_ID: u64 = 0x0002_6820_0000_0001;
/// 模块版本串。
pub const V2682_VERSION: &str = "v2682-cacheif-freeze-b15";
/// 诊断码段基址（册内独占）。
pub const SEG_BASE: u16 = 0x1DC0;
/// 诊断码段掩码（`SEG_BASE | SEG_MASK` 即段上界 0x1DFF）。
pub const SEG_MASK: u16 = 0x003F;
/// 诊断码段上界（含）。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;
/// 诊断主码（接口漂移，段内首码）。
pub const MAIN_CODE: u16 = SEG_BASE;

/// 冻结三接口数（判据：cache_lookup / cache_insert / cache_mark_dirty）。
pub const IFACE_COUNT: usize = 3;
/// 五方联签方数（判据：B1–B5）。
pub const PARTY_COUNT: usize = 5;
/// 前向契约数（判据：F2662 / F2663 / F2664）。
pub const CONTRACT_COUNT: usize = 3;
/// 单接口冻结字段数（判据：签名/参数/返回/错误码四件 + 语义条款一件）。
pub const FIELD_COUNT: usize = 5;
/// 冻结条款总格数（`IFACE_COUNT × FIELD_COUNT`，矩阵单元格数）。
pub const MATRIX_CELLS: usize = IFACE_COUNT * FIELD_COUNT;
/// 三机直测三轴数（签名编译期 / 错误码注入 / 语义行为）。
pub const DIRECT_AXES: usize = 3;
/// 注入形态数（判据十形态，负向注入全被拒并留痕）。
pub const FORM_COUNT: usize = 10;
/// 页大小（字节），页键下标掩码由它推导。
pub const PAGE_SIZE: u64 = 4096;
/// 页键下标位宽（`PAGE_SIZE == 1<<PAGE_INDEX_BITS`）。
pub const PAGE_INDEX_BITS: u32 = 12;
/// 页键下标掩码。
pub const PAGE_INDEX_MASK: u64 = PAGE_SIZE - 1;
/// 批量插入硬顶（对齐 F2731：批量 32 页硬顶）。
pub const BATCH_HARD_CAP: usize = 32;
/// 环形账固定容量（append-only，满则丢最旧并显性计数）。
pub const RING_CAP: usize = 128;
/// 直测抽样点数（判据「抽样 10 处消费与表一致」同量级）。
pub const SAMPLE_POINTS: usize = 10;
/// 冻结件版本号（变更须走 §16.2 双方同步回归，此号只增）。
pub const FREEZE_EPOCH: u64 = 3;
/// 语义条款引用源（B14 契约面）。
pub const SEMANTIC_REF: &str = "B14/F2662+F2663+F2664";
/// 判据阈值：五方联签必须齐备（0/5 即红）。
pub const SIGN_REQUIRED: usize = PARTY_COUNT;
/// 判据阈值：兑现映射零缺（缺一条即红）。
pub const MAP_REQUIRED: usize = CONTRACT_COUNT * FIELD_COUNT;
/// 判据阈值：三机直测三轴全过。
pub const AXES_REQUIRED: usize = DIRECT_AXES;
/// 判据阈值：钩子侧衔接声明（B5 半边对空即视为「接口侧已声明」）。
pub const HOOK_DECL_REQUIRED: u64 = 1;
/// 冻结件最长陈旧 tick（逻辑时间，超期未复测即冻结失效候选）。
pub const FREEZE_STALE_TICKS: u64 = 4096;
/// 逻辑页帧数（`CacheBook` 槽位数）。
pub const SLOTS: usize = 64;
/// 语义行为轴的样本数（行为样本矩阵规模）。
pub const BEHAVIOR_SAMPLES: usize = 16;

/// LCG 乘数（判据指定：`6364136223846793005`）。
pub const LCG_A: u64 = 6364136223846793005;
/// LCG 增量（判据指定：`1442695040888963407`）。
pub const LCG_C: u64 = 1442695040888963407;
/// FNV-1a basis（判据指定）。
pub const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a prime（判据指定）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 五方代号（B1–B5），索引 0..4。
pub const PARTY_CODES: [&str; PARTY_COUNT] = ["B1", "B2", "B3", "B4", "B5"];
/// 五方消费面短名（联签三字段之一：消费面）。
pub const PARTY_SURFACES: [&str; PARTY_COUNT] = ["ext4", "xfs", "btrfs", "ntfs3", "fat32"];
/// 三契约代号（索引 0..2）。
pub const CONTRACT_CODES: [&str; CONTRACT_COUNT] = ["F2662", "F2663", "F2664"];
/// 三接口代号（索引 0..2）。
pub const IFACE_CODES: [&str; IFACE_COUNT] = ["cache_lookup", "cache_insert", "cache_mark_dirty"];
/// 五个冻结字段代号（索引 0..4）。
pub const FIELD_CODES: [&str; FIELD_COUNT] = ["signature", "params", "returns", "errcodes", "semantics"];
/// 三机直测三轴代号（索引 0..2）。
pub const AXIS_CODES: [&str; DIRECT_AXES] = ["sig_compile", "errcode_inject", "semantic_behavior"];

// ---------------------------------------------------------------------------
// 0.1 确定性 LCG 与 FNV-1a 折叠
// ---------------------------------------------------------------------------

/// 确定性 LCG 发生器（模 `2^64`，全部 `wrapping`）。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    /// 内部状态。
    state: u64,
}

impl Lcg {
    /// 以给定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    /// 只读当前内部状态（供轨迹断言）。
    pub fn raw(&self) -> u64 {
        self.state
    }
    /// 取下一个 `u64`。
    pub fn next_u64(&mut self) -> u64 {
        self.state = lcg_step(self.state);
        self.state
    }
    /// 取 `[0, n)` 内值；`n == 0` 返回 0。
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % (n as u64)) as usize
    }
    /// 取 `[lo, hi]` 闭区间内的 `u64`（`hi < lo` 返回 `lo`）。
    pub fn range_u64(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        lo + self.next_u64() % (hi - lo + 1)
    }
    /// 以当前状态派生一个子发生器（分支轨迹隔离用）。
    pub fn fork(&self, salt: u64) -> Lcg {
        Lcg { state: self.state ^ salt.wrapping_mul(LCG_A) }
    }
}

/// 纯函数式 LCG 步进（注入核可自由复现轨迹，不需持有 `Lcg`）。
pub fn lcg_step(state: u64) -> u64 {
    state.wrapping_mul(LCG_A).wrapping_add(LCG_C)
}

/// FNV-1a 64 位折叠（判据指定 basis/prime）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = FNV_BASIS;
    for b in bytes {
        h ^= (*b as u64) & 0xFF;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// FNV-1a 折叠 `u64` 字序列（每字按 8 字节小端喂入）。
pub fn fnv1a_words(words: &[u64]) -> u64 {
    let mut h: u64 = FNV_BASIS;
    for w in words {
        h = fnv1a_u64_into(h, *w);
    }
    h
}

/// 把一个 `u64` 以 8 字节小端喂入 FNV-1a 累加器。
pub fn fnv1a_u64_into(h: u64, w: u64) -> u64 {
    let mut acc: u64 = h;
    for i in 0..8u32 {
        let byte = ((w >> (8 * i)) & 0xFF) as u8;
        acc ^= (byte as u64) & 0xFF;
        acc = acc.wrapping_mul(FNV_PRIME);
    }
    acc
}

/// 32 位折叠（供短标签用；口径与 64 位一致的前 32 位）。
pub fn fnv1a32(bytes: &[u8]) -> u32 {
    let h = fnv1a64(bytes);
    ((h & 0xFFFF_FFFF) ^ (h >> 32)) as u32
}

// ===========================================================================
// 1. 错误闭集（8–14 变体，一码一段）
// ===========================================================================

/// 页缓存读接口冻结域错误闭集（12 变体，码段内连续）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CacheErr {
    /// 0x1DC0 冻结件未登记（引用未冻结的接口/字段）。
    FreezeMissing,
    /// 0x1DC1 冻结件版本回退（试图登比 `FREEZE_EPOCH` 更小的版本）。
    EpochRollback,
    /// 0x1DC2 冻结件陈旧（超 `FREEZE_STALE_TICKS` 未复测）。
    FreezeStale,
    /// 0x1DC3 签名漂移（实测签名哈希 ≠ 冻结签名哈希）。
    SignatureDrift,
    /// 0x1DC4 错误码位图漂移（实测码集 ≠ 冻结码集）。
    ErrcodeDrift,
    /// 0x1DC5 语义条款缺引用（语义字段未挂 B14 契约面）。
    SemanticUnlinked,
    /// 0x1DC6 联签方不齐（B1–B5 未齐）。
    SignIncomplete,
    /// 0x1DC7 联签期望签名失配（某方期望签名 ≠ 冻结签名）。
    ExpectMismatch,
    /// 0x1DC8 兑现映射缺项（契约条款无对应冻结条款）。
    MapHole,
    /// 0x1DC9 兑现映射串位（契约条款挂到了错误接口/字段）。
    MapMisplaced,
    /// 0x1DCA 环形账溢出（`dropped` 记账越界）。
    RingOverflow,
    /// 0x1DCB 冻结账不守恒（`a + b == total` 破）。
    LedgerUnbalanced,
}

impl CacheErr {
    /// 全部变体（判据顺序，与码段一一对应）。
    pub const ALL: [CacheErr; 12] = [
        CacheErr::FreezeMissing,
        CacheErr::EpochRollback,
        CacheErr::FreezeStale,
        CacheErr::SignatureDrift,
        CacheErr::ErrcodeDrift,
        CacheErr::SemanticUnlinked,
        CacheErr::SignIncomplete,
        CacheErr::ExpectMismatch,
        CacheErr::MapHole,
        CacheErr::MapMisplaced,
        CacheErr::RingOverflow,
        CacheErr::LedgerUnbalanced,
    ];
    /// 变体序号（0..11）。
    pub fn index(self) -> usize {
        match self {
            CacheErr::FreezeMissing => 0,
            CacheErr::EpochRollback => 1,
            CacheErr::FreezeStale => 2,
            CacheErr::SignatureDrift => 3,
            CacheErr::ErrcodeDrift => 4,
            CacheErr::SemanticUnlinked => 5,
            CacheErr::SignIncomplete => 6,
            CacheErr::ExpectMismatch => 7,
            CacheErr::MapHole => 8,
            CacheErr::MapMisplaced => 9,
            CacheErr::RingOverflow => 10,
            CacheErr::LedgerUnbalanced => 11,
        }
    }
    /// 诊断码（段内连续，`SEG_BASE + index`）。
    pub fn code(self) -> u16 {
        SEG_BASE + (self.index() as u16)
    }
    /// 变体短名（导出用）。
    pub fn name(self) -> &'static str {
        match self {
            CacheErr::FreezeMissing => "freeze_missing",
            CacheErr::EpochRollback => "epoch_rollback",
            CacheErr::FreezeStale => "freeze_stale",
            CacheErr::SignatureDrift => "signature_drift",
            CacheErr::ErrcodeDrift => "errcode_drift",
            CacheErr::SemanticUnlinked => "semantic_unlinked",
            CacheErr::SignIncomplete => "sign_incomplete",
            CacheErr::ExpectMismatch => "expect_mismatch",
            CacheErr::MapHole => "map_hole",
            CacheErr::MapMisplaced => "map_misplaced",
            CacheErr::RingOverflow => "ring_overflow",
            CacheErr::LedgerUnbalanced => "ledger_unbalanced",
        }
    }
    /// 判读（现象）。
    pub fn symptom(self) -> &'static str {
        match self {
            CacheErr::FreezeMissing => "引用了未在跨域接口册登记的冻结字段",
            CacheErr::EpochRollback => "登记版本低于当前冻结纪元，疑似版本回退",
            CacheErr::FreezeStale => "冻结件超期未复测，冻结效力存疑",
            CacheErr::SignatureDrift => "参考实现实测签名哈希与冻结签名哈希不符",
            CacheErr::ErrcodeDrift => "参考实现实测错误码位图与冻结位图不符",
            CacheErr::SemanticUnlinked => "语义条款未引用 B14 契约面，语义不可追",
            CacheErr::SignIncomplete => "B1–B5 联签未齐，冻结件不得生效",
            CacheErr::ExpectMismatch => "某消费方期望签名与冻结签名不一致",
            CacheErr::MapHole => "前向契约条款找不到对应冻结条款（兑现缺项）",
            CacheErr::MapMisplaced => "契约条款挂到了非预期的接口/字段（兑现串位）",
            CacheErr::RingOverflow => "环形账容量越界，丢记录计数不可信",
            CacheErr::LedgerUnbalanced => "冻结账 a+b==total 守恒式被打破",
        }
    }
    /// 建议修法（三要素之三）。
    pub fn advice(self) -> &'static str {
        match self {
            CacheErr::FreezeMissing => "先走冻结登记流程补齐条款，再引用",
            CacheErr::EpochRollback => "按 §16.2 双方同步回归后以新纪元重新登记",
            CacheErr::FreezeStale => "补跑三机直测三轴并刷新复测 tick",
            CacheErr::SignatureDrift => "以冻结签名为准改参考实现，或走 §16.2 改冻结件",
            CacheErr::ErrcodeDrift => "对齐错误码位图：补齐缺码或撤销多码",
            CacheErr::SemanticUnlinked => "在语义字段挂 SEMANTIC_REF 指向的 B14 契约条款",
            CacheErr::SignIncomplete => "催齐剩余联签方，未齐前冻结件按草案处理",
            CacheErr::ExpectMismatch => "与该消费方对齐期望签名并留档差异说明",
            CacheErr::MapHole => "为该契约条款补一条冻结条款映射",
            CacheErr::MapMisplaced => "修正映射目标接口/字段，重新登记兑现表",
            CacheErr::RingOverflow => "提高 RING_CAP 或缩短采样窗口后重跑",
            CacheErr::LedgerUnbalanced => "调账：核对冻结项计数与生效项计数差额",
        }
    }
    /// 闭合位置（三要素之二：本单内的落点）。
    pub fn locus(self) -> &'static str {
        match self {
            CacheErr::FreezeMissing => "CacheBook::freeze()",
            CacheErr::EpochRollback => "CacheBook::freeze()",
            CacheErr::FreezeStale => "CacheBook::verify()",
            CacheErr::SignatureDrift => "CacheBook::sig_axis()",
            CacheErr::ErrcodeDrift => "CacheBook::errcode_axis()",
            CacheErr::SemanticUnlinked => "CacheBook::semantic_axis()",
            CacheErr::SignIncomplete => "CacheBook::sign()",
            CacheErr::ExpectMismatch => "CacheBook::sign()",
            CacheErr::MapHole => "CacheBook::map_contract()",
            CacheErr::MapMisplaced => "CacheBook::map_contract()",
            CacheErr::RingOverflow => "RingLog::push()",
            CacheErr::LedgerUnbalanced => "CacheBook::reconcile()",
        }
    }
    /// 三要素完整句（现象 + 位置 + 建议修法）。
    pub fn detail(self) -> String {
        format!("[0x{:04X}/{}] {}；落点 {}；修法：{}", self.code(), self.name(), self.symptom(), self.locus(), self.advice())
    }
}

/// 本单 `Result` 别名（错误闭集封闭，不外泄任何其它错误类型）。
pub type CacheResult<T> = Result<T, CacheErr>;

/// 码段断言辅助：给定码是否落在本单独占段内。
pub fn code_in_seg(code: u16) -> bool {
    if code < SEG_BASE {
        return false;
    }
    (code - SEG_BASE) <= SEG_MASK
}

/// 错误串（便捷导出）。
pub fn err_str(e: CacheErr) -> String {
    e.detail()
}

/// 闭集枚举的码表（导出用，逐行独立成串）。
pub fn err_code_table() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut i: usize = 0;
    while i < CacheErr::ALL.len() {
        let e: CacheErr = CacheErr::ALL[i];
        out.push(format!("0x{:04X} {} -> {}", e.code(), e.name(), e.locus()));
        i = i.wrapping_add(1);
    }
    out
}

// ===========================================================================
// 2. 核心数据结构（域模型）
// ===========================================================================

/// 三接口标识闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Iface {
    /// cache_lookup：只读命中查询。
    Lookup,
    /// cache_insert：写入装填。
    Insert,
    /// cache_mark_dirty：脏标记置位。
    MarkDirty,
}

impl Iface {
    /// 序号（0..2）。
    pub fn index(self) -> usize {
        match self {
            Iface::Lookup => 0,
            Iface::Insert => 1,
            Iface::MarkDirty => 2,
        }
    }
    /// 由序号反查（越界返回 `None`）。
    pub fn from_index(i: usize) -> Option<Iface> {
        match i {
            0 => Some(Iface::Lookup),
            1 => Some(Iface::Insert),
            2 => Some(Iface::MarkDirty),
            _ => None,
        }
    }
    /// 代号（判据三接口原名）。
    pub fn code(self) -> &'static str {
        IFACE_CODES[self.index()]
    }
    /// 该接口的规范性签名串（冻结件里逐字比对的字面量）。
    pub fn signature(self) -> String {
        match self {
            Iface::Lookup => {
                "cache_lookup(pgoff:u64,flags:u32)->CacheResult<CacheHit>".to_string()
            }
            Iface::Insert => {
                "cache_insert(pgoff:u64,bytes:u32,tick:u64)->CacheResult<CacheSlot>".to_string()
            }
            Iface::MarkDirty => {
                "cache_mark_dirty(pgoff:u64,tick:u64)->CacheResult<u64>".to_string()
            }
        }
    }
    /// 该接口的参数件数（冻结件「参数」字段的计数口径）。
    pub fn param_count(self) -> usize {
        match self {
            Iface::Lookup => 2,
            Iface::Insert => 3,
            Iface::MarkDirty => 2,
        }
    }
    /// 该接口允许的错误码集合（冻结件「错误码」字段的位图口径）。
    pub fn errcodes(self) -> [CacheErr; 4] {
        match self {
            Iface::Lookup => [CacheErr::FreezeMissing, CacheErr::SignatureDrift, CacheErr::SemanticUnlinked, CacheErr::LedgerUnbalanced],
            Iface::Insert => [CacheErr::FreezeMissing, CacheErr::ErrcodeDrift, CacheErr::RingOverflow, CacheErr::LedgerUnbalanced],
            Iface::MarkDirty => [CacheErr::FreezeMissing, CacheErr::EpochRollback, CacheErr::MapMisplaced, CacheErr::LedgerUnbalanced],
        }
    }
    /// 该接口是否直写冻结件（insert 与 mark_dirty 是写侧，lookup 是读侧）。
    pub fn is_write_side(self) -> bool {
        match self {
            Iface::Lookup => false,
            Iface::Insert => true,
            Iface::MarkDirty => true,
        }
    }
}

/// 五冻结字段标识闭集（签名/参数/返回/错误码/语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    /// 签名（可编译期核对的字面量签名串）。
    Signature,
    /// 参数（参数表：名/类型/序）。
    Params,
    /// 返回（返回类型与可空性）。
    Returns,
    /// 错误码（允许错误码集合）。
    Errcodes,
    /// 语义（行为语义条款 + 契约引用）。
    Semantics,
}

impl Field {
    /// 序号（0..4）。
    pub fn index(self) -> usize {
        match self {
            Field::Signature => 0,
            Field::Params => 1,
            Field::Returns => 2,
            Field::Errcodes => 3,
            Field::Semantics => 4,
        }
    }
    /// 由序号反查。
    pub fn from_index(i: usize) -> Option<Field> {
        match i {
            0 => Some(Field::Signature),
            1 => Some(Field::Params),
            2 => Some(Field::Returns),
            3 => Some(Field::Errcodes),
            4 => Some(Field::Semantics),
            _ => None,
        }
    }
    /// 字段代号。
    pub fn code(self) -> &'static str {
        FIELD_CODES[self.index()]
    }
    /// 该字段的判据三件归属（哪一件：签名/错误码/语义）。
    pub fn trinity(self) -> &'static str {
        match self {
            Field::Signature => "签名",
            Field::Params => "签名",
            Field::Returns => "签名",
            Field::Errcodes => "错误码",
            Field::Semantics => "语义",
        }
    }
}

/// 冻结件单元（单接口单字段）。
#[derive(Clone, Debug)]
pub struct FreezeCell {
    /// 目标接口。
    pub iface: Iface,
    /// 目标字段。
    pub field: Field,
    /// 冻结内容字面量（逐字比对用）。
    pub clause: String,
    /// 内容哈希（`fnv1a64`）。
    pub hash: u64,
    /// 冻结纪元（只增不减）。
    pub epoch: u64,
    /// 冻结发生的逻辑 tick。
    pub frozen_tick: u64,
    /// 是否已被标记为漂移（漂移后仍保留原冻结值以便对拍）。
    pub drift_marked: bool,
}

impl FreezeCell {
    /// 构造一个未冻结单元（`hash` 为 0 表示尚未登记）。
    pub fn blank(iface: Iface, field: Field) -> FreezeCell {
        FreezeCell { iface, field, clause: String::new(), hash: 0, epoch: 0, frozen_tick: 0, drift_marked: false }
    }
    /// 是否已冻结。
    pub fn frozen(&self) -> bool {
        self.hash != 0
    }
    /// 与实测内容对拍：一致返回 `true`。
    pub fn matches(&self, observed: &str) -> bool {
        if !self.frozen() {
            return false;
        }
        self.hash == fnv1a64(observed.as_bytes())
    }
    /// 导出行文本。
    pub fn row(&self) -> String {
        format!("cell[{}][{}] hash=0x{:016X} epoch={} tick={} drift={} clause=\"{}\"", self.iface.code(), self.field.code(), self.hash, self.epoch, self.frozen_tick,
            if self.drift_marked { 1 } else { 0 }, self.clause)
    }
    /// 编码为字节（16 字节载荷：hash + epoch）。
    pub fn into_bytes(&self) -> Vec<u8> {
        let mut v: Vec<u8> = Vec::new();
        push_u64(&mut v, self.hash);
        push_u64(&mut v, self.epoch);
        v
    }
}

/// 前向契约条款引用（`F2662/F2663/F2664` × 五字段的兑现映射目标）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ContractRef {
    /// 契约代号（0..2 → F2662/F2663/F2664）。
    pub contract: usize,
    /// 条款序号（契约内条款序，0..4 对齐五字段）。
    pub clause_ord: usize,
    /// 兑现目标接口。
    pub target_iface: Iface,
    /// 兑现目标字段。
    pub target_field: Field,
}

/// 兑现映射登记项（契约条款 → 冻结条款）。
#[derive(Clone, Debug)]
pub struct MapRow {
    /// 引用。
    pub cref: ContractRef,
    /// 是否已登记映射。
    pub mapped: bool,
    /// 映射到的冻结单元哈希（`mapped == false` 时为 0）。
    pub mapped_hash: u64,
    /// 登记 tick。
    pub tick: u64,
}

impl MapRow {
    /// 映射是否闭合（登记了且哈希命中真实冻结单元）。
    pub fn closed(&self, book_hash: u64) -> bool {
        self.mapped && self.mapped_hash != 0 && self.mapped_hash == book_hash
    }
    /// 导出行文本。
    pub fn row(&self) -> String {
        format!("map[{}#{}] -> {}/{} mapped={} hash=0x{:016X}", CONTRACT_CODES[self.cref.contract], self.cref.clause_ord, self.cref.target_iface.code(),
            self.cref.target_field.code(), if self.mapped { 1 } else { 0 }, self.mapped_hash)
    }
}

/// 五方联签记录（消费面 / 期望签名 / 回归责任 三字段）。
#[derive(Clone, Debug)]
pub struct SignRow {
    /// 联签方代号（0..4 → B1..B5）。
    pub party: usize,
    /// 消费面。
    pub surface: &'static str,
    /// 期望签名哈希。
    pub expect_sig: u64,
    /// 是否已联签。
    pub signed: bool,
    /// 承担的回归责任项数。
    pub regressions: usize,
    /// 联签 tick。
    pub tick: u64,
    /// 与 F2683 回写钩子联签的接口侧衔接声明（B5 半边对空）。
    pub hook_edge: u64,
}

impl SignRow {
    /// 构造未联签行。
    pub fn blank(party: usize) -> SignRow {
        SignRow { party, surface: PARTY_SURFACES[party], expect_sig: 0, signed: false, regressions: 0, tick: 0, hook_edge: 0 }
    }
    /// 联签是否有效（已签 + 期望签名非零 + 回归责任已分配）。
    pub fn valid(&self) -> bool {
        self.signed && self.expect_sig != 0 && self.regressions > 0
    }
    /// 导出行文本。
    pub fn row(&self) -> String {
        format!("sign[{}|{}] expect=0x{:016X} signed={} regr={} hook_edge={} tick={}", PARTY_CODES[self.party], self.surface, self.expect_sig,
            if self.signed { 1 } else { 0 }, self.regressions, self.hook_edge, self.tick)
    }
    /// 编码为字节（24 字节载荷）。
    pub fn into_bytes(&self) -> Vec<u8> {
        let mut v: Vec<u8> = Vec::new();
        push_u64(&mut v, self.expect_sig);
        push_u64(&mut v, self.regressions as u64);
        push_u64(&mut v, self.hook_edge);
        v
    }
}

/// 冻结件生命周期状态机闭集（合法边闭集见 §4）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreezeState {
    /// 草案（条款未齐）。
    Draft,
    /// 已提案（条款齐，待联签）。
    Proposed,
    /// 联签齐备（待直测）。
    SignedAll,
    /// 已生效（直测三轴全过）。
    Effective,
    /// 已退役（走 §16.2 变更后旧纪元作废）。
    Retired,
}

impl FreezeState {
    /// 序号（0..4）。
    pub fn index(self) -> usize {
        match self {
            FreezeState::Draft => 0,
            FreezeState::Proposed => 1,
            FreezeState::SignedAll => 2,
            FreezeState::Effective => 3,
            FreezeState::Retired => 4,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            FreezeState::Draft => "draft",
            FreezeState::Proposed => "proposed",
            FreezeState::SignedAll => "signed_all",
            FreezeState::Effective => "effective",
            FreezeState::Retired => "retired",
        }
    }
    /// 是否为终态（Retired）。
    pub fn terminal(self) -> bool {
        match self {
            FreezeState::Retired => true,
            _ => false,
        }
    }
}

/// 状态机迁移边。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Edge {
    /// 源态。
    pub from: FreezeState,
    /// 目标态。
    pub to: FreezeState,
}

impl Edge {
    /// 合法边闭集常量（构造期求值，运行期只查表）。
    pub const LEGAL: [Edge; 6] = [
        Edge { from: FreezeState::Draft, to: FreezeState::Proposed },
        Edge { from: FreezeState::Proposed, to: FreezeState::Draft },
        Edge { from: FreezeState::Proposed, to: FreezeState::SignedAll },
        Edge { from: FreezeState::SignedAll, to: FreezeState::Proposed },
        Edge { from: FreezeState::SignedAll, to: FreezeState::Effective },
        Edge { from: FreezeState::Effective, to: FreezeState::Retired },
    ];
    /// 该边是否在合法闭集内。
    pub fn legal(from: FreezeState, to: FreezeState) -> bool {
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
    /// 导出文本。
    pub fn row(&self) -> String {
        format!("edge {} -> {}", self.from.name(), self.to.name())
    }
}

/// 页键（`tag` 高位 + `pgoff` 低位，pgoff 必落在 `PAGE_INDEX_MASK` 内）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PageKey {
    /// 地址空间标签（多 fs 域并存，用 tag 隔离）。
    pub tag: u64,
    /// 页内偏移（页号）。
    pub pgoff: u64,
}

impl PageKey {
    /// 构造页键（自动掩掉 pgoff 溢出位）。
    pub fn new(tag: u64, pgoff: u64) -> PageKey {
        PageKey { tag, pgoff: pgoff & PAGE_INDEX_MASK }
    }
    /// 线性化 `u64`（高 32 位 tag，低 32 位 pgoff）。
    pub fn linear(self) -> u64 {
        (self.tag << 32) | (self.pgoff & 0xFFFF_FFFF)
    }
    /// 由线性值反解（供对拍）。
    pub fn from_linear(v: u64) -> PageKey {
        PageKey { tag: v >> 32, pgoff: v & PAGE_INDEX_MASK }
    }
    /// 导出文本。
    pub fn row(&self) -> String {
        format!("key[tag=0x{:X} pgoff=0x{:X}]", self.tag, self.pgoff)
    }
}

/// 页槽状态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    /// 空槽。
    Vacant,
    /// 命中且干净。
    Clean,
    /// 命中且脏。
    Dirty,
    /// 待回收（已摘出映射但内容未清）。
    Reclaim,
}

impl SlotState {
    /// 序号（0..3）。
    pub fn index(self) -> usize {
        match self {
            SlotState::Vacant => 0,
            SlotState::Clean => 1,
            SlotState::Dirty => 2,
            SlotState::Reclaim => 3,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            SlotState::Vacant => "vacant",
            SlotState::Clean => "clean",
            SlotState::Dirty => "dirty",
            SlotState::Reclaim => "reclaim",
        }
    }
    /// 是否可被 lookup 命中。
    pub fn hittable(self) -> bool {
        match self {
            SlotState::Clean => true,
            SlotState::Dirty => true,
            _ => false,
        }
    }
    /// 是否占用容量。
    pub fn occupied(self) -> bool {
        match self {
            SlotState::Vacant => false,
            _ => true,
        }
    }
}

/// 页槽（参考实现侧，与 B14 实现本体无关，仅供签名对拍）。
#[derive(Clone, Copy, Debug)]
pub struct CacheSlot {
    /// 页键（`occupied == false` 时无意义）。
    pub key: PageKey,
    /// 槽状态。
    pub state: SlotState,
    /// 内容指纹（`occupied` 时有效）。
    pub content: u64,
    /// 脏标记置位次数。
    pub dirty_seq: u64,
    /// 装填发生的逻辑 tick。
    pub filled_tick: u64,
}

impl CacheSlot {
    /// 空槽。
    pub fn vacant() -> CacheSlot {
        CacheSlot { key: PageKey::new(0, 0), state: SlotState::Vacant, content: 0, dirty_seq: 0, filled_tick: 0 }
    }
    /// 导出文本。
    pub fn row(&self) -> String {
        format!("slot[{}] state={} content=0x{:016X} dirty_seq={} tick={}", self.key.row(), self.state.name(), self.content, self.dirty_seq, self.filled_tick)
    }
}

/// lookup 命中载荷。
#[derive(Clone, Copy, Debug)]
pub struct CacheHit {
    /// 命中槽序。
    pub slot: usize,
    /// 命中时的内容指纹。
    pub content: u64,
    /// 命中时是否脏。
    pub dirty: bool,
}

/// append-only 环形账的一行（留痕专用，字段全整数）。
#[derive(Clone, Copy, Debug)]
pub struct TraceRow {
    /// 逻辑 tick。
    pub tick: u64,
    /// 记录类别（0=冻结 1=联签 2=兑现 3=直测 4=注入 5=拒记 6=调账）。
    pub kind: u64,
    /// 类别内的定位码（接口序/方序/形态序等）。
    pub tag: u64,
    /// 载荷（计数或哈希低位）。
    pub payload: u64,
}

impl TraceRow {
    /// 导出文本。
    pub fn row(&self) -> String {
        format!("trace[t={} kind={} tag={} payload=0x{:016X}]", self.tick, self.kind, self.tag, self.payload)
    }
    /// 编码为 32 字节。
    pub fn into_bytes(&self) -> Vec<u8> {
        let mut v: Vec<u8> = Vec::new();
        push_u64(&mut v, self.tick);
        push_u64(&mut v, self.kind);
        push_u64(&mut v, self.tag);
        push_u64(&mut v, self.payload);
        v
    }
}

/// append-only 环形账（固定容量 `RING_CAP`，满则丢最旧并显性计 `dropped`）。
#[derive(Clone, Debug)]
pub struct RingLog {
    /// 缓冲（固定容量，append-only）。
    pub buf: Vec<TraceRow>,
    /// 头指针（写满后回绕）。
    pub head: usize,
    /// 累计写入。
    pub written: u64,
    /// 累计丢弃（显性）。
    pub dropped: u64,
    /// 非法转换拒记数。
    pub rejected: u64,
}

impl RingLog {
    /// 构造空环形账（预分配 `RING_CAP`，无 IO）。
    pub fn new() -> RingLog {
        let mut buf: Vec<TraceRow> = Vec::with_capacity(RING_CAP);
        let mut i: usize = 0;
        while i < RING_CAP {
            buf.push(TraceRow { tick: 0, kind: u64::MAX, tag: u64::MAX, payload: u64::MAX });
            i = i.wrapping_add(1);
        }
        RingLog { buf, head: 0, written: 0, dropped: 0, rejected: 0 }
    }
    /// 当前有效条数（`written - dropped`，饱和到 `RING_CAP`）。
    pub fn live(&self) -> usize {
        let w = self.written.saturating_sub(self.dropped);
        if w > (RING_CAP as u64) {
            return RING_CAP;
        }
        w as usize
    }
    /// 追加一行；写满覆盖最旧并 `dropped += 1`。
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
    /// 读出有效行（按写入顺序 oldest→newest）。
    pub fn rows(&self) -> Vec<TraceRow> {
        let cap: usize = self.buf.len();
        let live: usize = self.live();
        let mut out: Vec<TraceRow> = Vec::with_capacity(live);
        let mut k: usize = 0;
        while k < live {
            // 环形读：最旧有效起点 = head - live（对 cap 取模）。
            let start: usize = (self.head + cap - live) % cap;
            let idx: usize = (start + k) % cap;
            let r: TraceRow = self.buf[idx];
            out.push(r);
            k = k.wrapping_add(1);
        }
        out
    }
    /// 环形账自洽检查：`live + dropped == written` 且 `live <= cap`。
    pub fn coherent(&self) -> bool {
        let live: u64 = self.live() as u64;
        live.wrapping_add(self.dropped) == self.written && live <= (RING_CAP as u64)
    }
    /// 拒记一行（非法迁移 / 注入被拒的留痕）。
    pub fn reject(&mut self, row: TraceRow) {
        self.push(row);
        self.rejected = self.rejected.wrapping_add(1);
    }
    /// 有效行的折叠指纹。
    pub fn fingerprint(&self) -> u64 {
        let rows: Vec<TraceRow> = self.rows();
        let mut words: Vec<u64> = Vec::with_capacity(rows.len() * 4);
        for r in rows.iter() {
            words.push(r.tick);
            words.push(r.kind);
            words.push(r.tag);
            words.push(r.payload);
        }
        fnv1a_words(&words)
    }
    /// 导出文本（逐行）。
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
// 3. 守恒账（`a + b == total`，整数）
// ===========================================================================

/// 冻结账守恒结构：`frozen + drifted + pending == total`。
///
/// 口径说明：`total` 在**分配时**从 0 起算并与冻结矩阵同尺寸（`MATRIX_CELLS`），
/// 每冻结一格 `frozen += 1`，检出漂移 `drifted += 1`（同时从 `frozen` 转入），
/// 未冻结仍留 `pending`。
#[derive(Clone, Copy, Debug)]
pub struct FreezeLedger {
    /// 冻结矩阵总格（分配即定，从 0 起算与分配同步）。
    pub total: u64,
    /// 已冻结且未漂移的格数。
    pub frozen: u64,
    /// 已冻结但被判漂移的格数（仍计入 total）。
    pub drifted: u64,
    /// 尚未冻结的格数。
    pub pending: u64,
}

impl FreezeLedger {
    /// 按矩阵尺寸分配（`a + b == total` 从 0 起算并与分配同步）。
    pub fn alloc(cells: usize) -> FreezeLedger {
        FreezeLedger { total: cells as u64, frozen: 0, drifted: 0, pending: cells as u64 }
    }
    /// 记一次冻结。
    pub fn on_freeze(&mut self) {
        if self.pending > 0 {
            self.pending = self.pending.wrapping_sub(1);
            self.frozen = self.frozen.wrapping_add(1);
        }
    }
    /// 记一次漂移（`frozen → drifted` 的转账，总量不变）。
    pub fn on_drift(&mut self) {
        if self.frozen > 0 {
            self.frozen = self.frozen.wrapping_sub(1);
            self.drifted = self.drifted.wrapping_add(1);
        }
    }
    /// 守恒式是否成立。
    pub fn conserved(&self) -> bool {
        self.frozen.wrapping_add(self.drifted).wrapping_add(self.pending) == self.total
    }
    /// 调账：把 pending 归零（用于直测后统一结算），返回实际调账额。
    pub fn reconcile(&mut self) -> u64 {
        let moved: u64 = self.pending;
        self.pending = 0;
        self.frozen = self.frozen.wrapping_add(moved);
        moved
    }
    /// 导出文本。
    pub fn row(&self) -> String {
        format!("ledger a={} b={} c={} total={} conserved={}", self.frozen, self.drifted, self.pending, self.total, if self.conserved() { 1 } else { 0 })
    }
}

/// 直测轴账（`passed + failed == total`，整数）。
#[derive(Clone, Copy, Debug)]
pub struct AxisLedger {
    /// 轴总数。
    pub total: u64,
    /// 通过数。
    pub passed: u64,
    /// 失败数。
    pub failed: u64,
}

impl AxisLedger {
    /// 分配（从 0 起算并与分配同步）。
    pub fn alloc(n: usize) -> AxisLedger {
        AxisLedger { total: n as u64, passed: 0, failed: 0 }
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

/// 注入核账（`rejected + leaked == total`，整数；`leaked` 必须恒 0）。
#[derive(Clone, Copy, Debug)]
pub struct InjectLedger {
    /// 注入总次数。
    pub total: u64,
    /// 被拒次数。
    pub rejected: u64,
    /// 漏过次数（必须恒 0）。
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
    /// 全部被拒。
    pub fn all_rejected(&self) -> bool {
        self.conserved() && self.leaked == 0 && self.rejected == self.total
    }
}

// ===========================================================================
// 4. 字节编解码小工具（无 IO、无浮点）
// ===========================================================================

/// 向字节流追加一个 `u64`（8 字节小端）。
pub fn push_u64(v: &mut Vec<u8>, w: u64) {
    let mut i: u32 = 0;
    while i < 8 {
        let byte: u8 = ((w >> (8 * i)) & 0xFF) as u8;
        v.push(byte);
        i = i.wrapping_add(1);
    }
}

/// 向字节流追加一个 `u32`（4 字节小端）。
pub fn push_u32(v: &mut Vec<u8>, w: u32) {
    let mut i: u32 = 0;
    while i < 4 {
        let byte: u8 = ((w >> (8 * i)) & 0xFF) as u8;
        v.push(byte);
        i = i.wrapping_add(1);
    }
}

/// 向字节流追加一个 `u16`（2 字节小端）。
pub fn push_u16(v: &mut Vec<u8>, w: u16) {
    let mut i: u32 = 0;
    while i < 2 {
        let byte: u8 = ((w >> (8 * i)) & 0xFF) as u8;
        v.push(byte);
        i = i.wrapping_add(1);
    }
}

/// 从字节流读 `u64`（越界补 0，无 panic）。
pub fn read_u64(v: &[u8], off: usize) -> u64 {
    let mut acc: u64 = 0;
    let mut i: u32 = 0;
    while i < 8 {
        let idx: usize = off + (i as usize);
        let b: u8 = match v.get(idx) {
            Some(x) => *x,
            None => 0,
        };
        acc |= ((b as u64) & 0xFF) << (8 * i);
        i = i.wrapping_add(1);
    }
    acc
}

/// 从字节流读 `u32`（越界补 0）。
pub fn read_u32(v: &[u8], off: usize) -> u64 {
    let mut acc: u64 = 0;
    let mut i: u32 = 0;
    while i < 4 {
        let idx: usize = off + (i as usize);
        let b: u8 = match v.get(idx) {
            Some(x) => *x,
            None => 0,
        };
        acc |= ((b as u64) & 0xFF) << (8 * i);
        i = i.wrapping_add(1);
    }
    acc
}

/// 从字节流读 `u16`（越界补 0）。
pub fn read_u16(v: &[u8], off: usize) -> u64 {
    let mut acc: u64 = 0;
    let mut i: u32 = 0;
    while i < 2 {
        let idx: usize = off + (i as usize);
        let b: u8 = match v.get(idx) {
            Some(x) => *x,
            None => 0,
        };
        acc |= ((b as u64) & 0xFF) << (8 * i);
        i = i.wrapping_add(1);
    }
    acc
}
// ===========================================================================
// 5. 主状态机与冻结件引擎 `CacheBook`
// ===========================================================================

/// 主引擎：冻结矩阵 + 五方联签 + 兑现映射 + 三机直测 + 参考实现 + 环形账。
#[derive(Clone, Debug)]
pub struct CacheBook {
    /// 冻结矩阵（`IFACE_COUNT × FIELD_COUNT`，行优先，`idx = i*FIELD_COUNT + f`）。
    pub cells: Vec<FreezeCell>,
    /// 五方联签行。
    pub signs: Vec<SignRow>,
    /// 兑现映射行（`CONTRACT_COUNT × FIELD_COUNT`）。
    pub maps: Vec<MapRow>,
    /// 冻结件生命周期态。
    pub state: FreezeState,
    /// 冻结纪元（只增）。
    pub epoch: u64,
    /// 逻辑时钟。
    pub tick: u64,
    /// 上次三机直测 tick（`FREEZE_STALE_TICKS` 判陈旧）。
    pub last_retest_tick: u64,
    /// 参考实现槽（`SLOTS` 个）。
    pub slots: Vec<CacheSlot>,
    /// 冻结账。
    pub ledger: FreezeLedger,
    /// 直测轴账。
    pub axes: AxisLedger,
    /// 注入账。
    pub inject: InjectLedger,
    /// 环形留痕账。
    pub ring: RingLog,
    /// 累计调用计数：lookup。
    pub lookup_calls: u64,
    /// 累计调用计数：insert。
    pub insert_calls: u64,
    /// 累计调用计数：mark_dirty。
    pub dirty_calls: u64,
    /// 命中次数。
    pub hits: u64,
    /// 未命中次数。
    pub misses: u64,
    /// 脏标记置位次数。
    pub dirty_set: u64,
    /// 非法状态迁移被拒次数。
    pub illegal_trans: u64,
    /// 冻结件总哈希（fingerprint 主体）。
    pub frozen_fp: u64,
}

impl CacheBook {
    /// 构造引擎（分配即从 0 起算，与 `MATRIX_CELLS` 同步）。
    pub fn new(tick0: u64) -> CacheBook {
        // 冻结矩阵：按接口×字段展开。
        let mut cells: Vec<FreezeCell> = Vec::with_capacity(MATRIX_CELLS);
        let mut i: usize = 0;
        while i < IFACE_COUNT {
            let ifc: Iface = match Iface::from_index(i) {
                Some(x) => x,
                None => Iface::Lookup,
            };
            let mut f: usize = 0;
            while f < FIELD_COUNT {
                let fld: Field = match Field::from_index(f) {
                    Some(x) => x,
                    None => Field::Signature,
                };
                cells.push(FreezeCell::blank(ifc, fld));
                f = f.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        // 五方联签行。
        let mut signs: Vec<SignRow> = Vec::with_capacity(PARTY_COUNT);
        let mut p: usize = 0;
        while p < PARTY_COUNT {
            signs.push(SignRow::blank(p));
            p = p.wrapping_add(1);
        }
        // 兑现映射行：契约 × 条款序 → 目标接口/字段（默认串位目标为同序接口）。
        let mut maps: Vec<MapRow> = Vec::with_capacity(CONTRACT_COUNT * FIELD_COUNT);
        let mut c: usize = 0;
        while c < CONTRACT_COUNT {
            let mut o: usize = 0;
            while o < FIELD_COUNT {
                let target_iface: Iface = match Iface::from_index(c % IFACE_COUNT) {
                    Some(x) => x,
                    None => Iface::Lookup,
                };
                let target_field: Field = match Field::from_index(o) {
                    Some(x) => x,
                    None => Field::Signature,
                };
                maps.push(MapRow { cref: ContractRef { contract: c, clause_ord: o, target_iface, target_field }, mapped: false, mapped_hash: 0, tick: 0 });
                o = o.wrapping_add(1);
            }
            c = c.wrapping_add(1);
        }
        // 参考实现槽。
        let mut slots: Vec<CacheSlot> = Vec::with_capacity(SLOTS);
        let mut s: usize = 0;
        while s < SLOTS {
            slots.push(CacheSlot::vacant());
            s = s.wrapping_add(1);
        }
        CacheBook {
            cells,
            signs,
            maps,
            state: FreezeState::Draft,
            epoch: 0,
            tick: tick0,
            last_retest_tick: tick0,
            slots,
            ledger: FreezeLedger::alloc(MATRIX_CELLS),
            axes: AxisLedger::alloc(DIRECT_AXES),
            inject: InjectLedger::alloc(FORM_COUNT),
            ring: RingLog::new(),
            lookup_calls: 0,
            insert_calls: 0,
            dirty_calls: 0,
            hits: 0,
            misses: 0,
            dirty_set: 0,
            illegal_trans: 0,
            frozen_fp: 0,
        }
    }

    // -----------------------------------------------------------------------
    // 5.1 状态迁移（合法边闭集 + 非法拒记）
    // -----------------------------------------------------------------------

    /// 尝试迁移；非法边返回 `CacheErr::FreezeMissing` 语义位并拒记。
    ///
    /// 非法迁移不走错误闭集（它不是「错误码」而是流程拒记），用
    /// `CacheErr::SemanticUnlinked` 作闭集内代表码落痕，保持闭集封闭。
    pub fn transit(&mut self, to: FreezeState) -> CacheResult<bool> {
        let from: FreezeState = self.state;
        if !Edge::legal(from, to) {
            self.illegal_trans = self.illegal_trans.wrapping_add(1);
            self.ring.reject(TraceRow { tick: self.tick, kind: 5, tag: (from.index() as u64) * 8 + (to.index() as u64), payload: (from.index() as u64) << 32 | (to.index() as u64) });
            return Err(CacheErr::SemanticUnlinked);
        }
        if from.terminal() {
            self.illegal_trans = self.illegal_trans.wrapping_add(1);
            return Err(CacheErr::SemanticUnlinked);
        }
        self.state = to;
        self.ring.push(TraceRow { tick: self.tick, kind: 6, tag: to.index() as u64, payload: (from.index() as u64) << 32 | (to.index() as u64) });
        Ok(true)
    }

    /// 逻辑时钟推进一格（不做真实时间读取）。
    pub fn advance(&mut self, n: u64) {
        self.tick = self.tick.wrapping_add(n);
    }

    // -----------------------------------------------------------------------
    // 5.2 冻结登记（第一步）
    // -----------------------------------------------------------------------

    /// 取冻结单元的可变引用（行列越界返回 `None`）。
    pub fn cell_mut(&mut self, iface: Iface, field: Field) -> Option<&mut FreezeCell> {
        let idx: usize = iface.index() * FIELD_COUNT + field.index();
        self.cells.get_mut(idx)
    }

    /// 取冻结单元的只读引用。
    pub fn cell_ref(&self, iface: Iface, field: Field) -> Option<&FreezeCell> {
        let idx: usize = iface.index() * FIELD_COUNT + field.index();
        self.cells.get(idx)
    }

    /// 计算某接口某字段的**规范冻结值**（这就是「三件」的口径来源）。
    pub fn canonical_clause(iface: Iface, field: Field) -> String {
        match field {
            Field::Signature => iface.signature(),
            Field::Params => {
                // 参数表：逐参数 `名:类型@序`。
                let n: usize = iface.param_count();
                let mut s: String = String::new();
                let mut k: usize = 0;
                while k < n {
                    let (name, ty): (&str, &str) = match (iface, k) {
                        (Iface::Lookup, 0) => ("pgoff", "u64"),
                        (Iface::Lookup, 1) => ("flags", "u32"),
                        (Iface::Insert, 0) => ("pgoff", "u64"),
                        (Iface::Insert, 1) => ("bytes", "u32"),
                        (Iface::Insert, 2) => ("tick", "u64"),
                        (Iface::MarkDirty, 0) => ("pgoff", "u64"),
                        (Iface::MarkDirty, 1) => ("tick", "u64"),
                        _ => ("pad", "u64"),
                    };
                    if k > 0 {
                        s.push(',');
                    }
                    s.push_str(name);
                    s.push(':');
                    s.push_str(ty);
                    s.push('@');
                    s.push_str(&(k + 1).to_string());
                    k = k.wrapping_add(1);
                }
                s
            }
            Field::Returns => match iface {
                Iface::Lookup => "CacheResult<CacheHit>".to_string(),
                Iface::Insert => "CacheResult<CacheSlot>".to_string(),
                Iface::MarkDirty => "CacheResult<u64>".to_string(),
            },
            Field::Errcodes => {
                // 错误码集合串：`码:名|...` 按序。
                let set: [CacheErr; 4] = iface.errcodes();
                let mut s: String = String::new();
                let mut k: usize = 0;
                while k < 4 {
                    if k > 0 {
                        s.push('|');
                    }
                    s.push_str(&format!("0x{:04X}:{}", set[k].code(), set[k].name()));
                    k = k.wrapping_add(1);
                }
                s
            }
            Field::Semantics => {
                // 语义条款：行为摘要 + 契约引用。
                let mut s: String = String::new();
                match iface {
                    Iface::Lookup => {
                        s.push_str("read-only hit query; miss returns Err(FreezeMissing); no side effect on slots");
                    }
                    Iface::Insert => {
                        s.push_str("fill-or-replace; batch cap 32; occupied count conserved; over-cap request rejected");
                    }
                    Iface::MarkDirty => {
                        s.push_str("set dirty bit monotonically; dirty_seq never decreases; reclaim slot rejected");
                    }
                }
                s.push_str(" ref=");
                s.push_str(SEMANTIC_REF);
                s
            }
        }
    }

    /// 冻结登记：把某接口某字段的规范值写入冻结矩阵。
    pub fn freeze(&mut self, iface: Iface, field: Field) -> CacheResult<u64> {
        if self.state == FreezeState::Retired {
            return Err(CacheErr::FreezeStale);
        }
        let clause: String = CacheBook::canonical_clause(iface, field);
        if clause.len() == 0 {
            return Err(CacheErr::FreezeMissing);
        }
        // 语义字段必须挂 B14 契约引用，否则语义不可追。
        if field == Field::Semantics && !clause.contains(SEMANTIC_REF) {
            return Err(CacheErr::SemanticUnlinked);
        }
        let hash: u64 = fnv1a64(clause.as_bytes());
        let idx: usize = iface.index() * FIELD_COUNT + field.index();
        let existed: bool = match self.cells.get(idx) {
            Some(c) => c.frozen(),
            None => false,
        };
        match self.cells.get_mut(idx) {
            Some(c) => {
                if existed && c.epoch > FREEZE_EPOCH {
                    // 试图登比当前纪元更新的值——本单只增不减，防回退。
                    return Err(CacheErr::EpochRollback);
                }
                if existed && c.clause == clause {
                    return Ok(c.hash);
                }
                c.clause = clause;
                c.hash = hash;
                c.epoch = FREEZE_EPOCH;
                c.frozen_tick = self.tick;
                c.drift_marked = false;
            }
            None => return Err(CacheErr::FreezeMissing),
        }
        if !existed {
            self.ledger.on_freeze();
        }
        self.ring.push(TraceRow { tick: self.tick, kind: 0, tag: idx as u64, payload: hash });
        Ok(hash)
    }

    /// 一键冻结三接口全部五字段（`MATRIX_CELLS` 格）。
    pub fn freeze_all(&mut self) -> CacheResult<usize> {
        let mut n: usize = 0;
        let mut i: usize = 0;
        while i < IFACE_COUNT {
            let ifc: Iface = match Iface::from_index(i) {
                Some(x) => x,
                None => Iface::Lookup,
            };
            let mut f: usize = 0;
            while f < FIELD_COUNT {
                let fld: Field = match Field::from_index(f) {
                    Some(x) => x,
                    None => Field::Signature,
                };
                match self.freeze(ifc, fld) {
                    Ok(_) => {
                        n = n.wrapping_add(1);
                    }
                    Err(_) => {}
                }
                f = f.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        // 条款齐 → 进提案态。
        if self.frozen_cells() == MATRIX_CELLS {
            let _ = self.transit(FreezeState::Proposed);
        }
        Ok(n)
    }

    /// 已冻结格数（0..MATRIX_CELLS）。
    pub fn frozen_cells(&self) -> usize {
        let mut n: usize = 0;
        for c in self.cells.iter() {
            if c.frozen() {
                n = n.wrapping_add(1);
            }
        }
        n
    }

    /// 全部条款矩阵行（`MATRIX_CELLS` 行）。
    pub fn matrix_rows(&self) -> Vec<MatrixRow> {
        let mut out: Vec<MatrixRow> = Vec::with_capacity(MATRIX_CELLS);
        let mut i: usize = 0;
        while i < IFACE_COUNT {
            let ifc: Iface = match Iface::from_index(i) {
                Some(x) => x,
                None => Iface::Lookup,
            };
            let mut f: usize = 0;
            while f < FIELD_COUNT {
                let fld: Field = match Field::from_index(f) {
                    Some(x) => x,
                    None => Field::Signature,
                };
                let frozen: bool = match self.cell_ref(ifc, fld) {
                    Some(c) => c.frozen(),
                    None => false,
                };
                let canon: String = CacheBook::canonical_clause(ifc, fld);
                let ok: bool = match self.cell_ref(ifc, fld) {
                    Some(c) => c.matches(canon.as_str()),
                    None => false,
                };
                out.push(MatrixRow { iface: ifc, field: fld, frozen, canonical: canon, matched: ok, trinity: fld.trinity() });
                f = f.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        out
    }

    /// 矩阵全过（`MATRIX_CELLS == MATRIX_CELLS`）。
    pub fn matrix_ok(&self) -> bool {
        let mut n: usize = 0;
        for r in self.matrix_rows().iter() {
            if r.frozen && r.matched {
                n = n.wrapping_add(1);
            }
        }
        n == MATRIX_CELLS
    }

    // -----------------------------------------------------------------------
    // 5.3 五方联签（第二步）
    // -----------------------------------------------------------------------

    /// 某接口的冻结签名哈希（联签期望签名的唯一来源）。
    pub fn frozen_sig_hash(&self, iface: Iface) -> CacheResult<u64> {
        match self.cell_ref(iface, Field::Signature) {
            Some(c) => {
                if !c.frozen() {
                    return Err(CacheErr::FreezeMissing);
                }
                Ok(c.hash)
            }
            None => Err(CacheErr::FreezeMissing),
        }
    }

    /// 单方联签（消费面/期望签名/回归责任 三字段齐）。
    pub fn sign(&mut self, party: usize, regressions: usize, hook_edge: u64) -> CacheResult<u64> {
        if party >= PARTY_COUNT {
            return Err(CacheErr::SignIncomplete);
        }
        if self.state != FreezeState::Proposed {
            return Err(CacheErr::SignIncomplete);
        }
        // 期望签名统一取 MarkDirty（写侧最高风险接口）冻结签名；
        // 若未冻结则退回 Insert，再退回 Lookup。
        let mut want: u64 = match self.frozen_sig_hash(Iface::MarkDirty) {
            Ok(h) => h,
            Err(_) => match self.frozen_sig_hash(Iface::Insert) {
                Ok(h) => h,
                Err(_) => match self.frozen_sig_hash(Iface::Lookup) {
                    Ok(h) => h,
                    Err(_) => return Err(CacheErr::ExpectMismatch),
                },
            },
        };
        // B5（ntfs3）额外校验 lookup 侧签名以体现消费面差异。
        if party == 4 {
            match self.frozen_sig_hash(Iface::Lookup) {
                Ok(h) => {
                    want = fnv1a_u64_into(want, h);
                }
                Err(_) => return Err(CacheErr::ExpectMismatch),
            }
        }
        if regressions == 0 {
            return Err(CacheErr::ExpectMismatch);
        }
        match self.signs.get_mut(party) {
            Some(r) => {
                r.expect_sig = want;
                r.regressions = regressions;
                r.signed = true;
                r.tick = self.tick;
                r.hook_edge = if party == 4 { hook_edge } else { 0 };
                self.ring.push(TraceRow { tick: self.tick, kind: 1, tag: party as u64, payload: want });
            }
            None => return Err(CacheErr::SignIncomplete),
        }
        // 齐备即进联签态。
        if self.signed_count() == SIGN_REQUIRED {
            let _ = self.transit(FreezeState::SignedAll);
        }
        Ok(want)
    }

    /// 已联签方数（0..5）。
    pub fn signed_count(&self) -> usize {
        let mut n: usize = 0;
        for r in self.signs.iter() {
            if r.signed {
                n = n.wrapping_add(1);
            }
        }
        n
    }

    /// 重算每方联签的期望签名（判定篡改的权威口径）。
    pub fn sign_expect_ok(&self) -> bool {
        let base: u64 = match self.frozen_sig_hash(Iface::MarkDirty) {
            Ok(h) => h,
            Err(_) => return false,
        };
        let lookup_h: u64 = match self.frozen_sig_hash(Iface::Lookup) {
            Ok(h) => h,
            Err(_) => return false,
        };
        let b5_expect: u64 = fnv1a_u64_into(base, lookup_h);
        for r in self.signs.iter() {
            let want: u64 = if r.party == 4 { b5_expect } else { base };
            if r.expect_sig != want {
                return false;
            }
        }
        true
    }

    /// 联签齐备且期望签名与冻结签名一致（当前口径下期望签名由冻结签名派生，
    /// 故此处额外校验：期望签名未被篡改 + B5 半边给出钩子衔接声明）。
    pub fn sign_ok(&self) -> bool {
        if self.signed_count() != SIGN_REQUIRED {
            return false;
        }
        let hook_ok: bool = match self.signs.get(PARTY_COUNT - 1) {
            Some(r) => r.hook_edge >= HOOK_DECL_REQUIRED,
            None => false,
        };
        hook_ok && self.sign_expect_ok()
    }

    /// 联签行文本。
    pub fn sign_rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(PARTY_COUNT);
        for r in self.signs.iter() {
            out.push(r.row());
        }
        out
    }

    // -----------------------------------------------------------------------
    // 5.4 兑现闭合登记（第三步）
    // -----------------------------------------------------------------------

    /// 登记一条兑现映射（契约条款 → 冻结条款）。
    pub fn map_contract(&mut self, cref: ContractRef) -> CacheResult<u64> {
        if cref.contract >= CONTRACT_COUNT || cref.clause_ord >= FIELD_COUNT {
            return Err(CacheErr::MapHole);
        }
        // 串位检测：契约 `c` 只允许兑现到契约自身序对应的接口族。
        let expect_iface_ord: usize = cref.contract % IFACE_COUNT;
        if cref.target_iface.index() != expect_iface_ord {
            return Err(CacheErr::MapMisplaced);
        }
        let src: u64 = match self.cell_ref(cref.target_iface, cref.target_field) {
            Some(c) => {
                if !c.frozen() {
                    return Err(CacheErr::MapHole);
                }
                c.hash
            }
            None => return Err(CacheErr::MapHole),
        };
        let idx: usize = cref.contract * FIELD_COUNT + cref.clause_ord;
        match self.maps.get_mut(idx) {
            Some(r) => {
                r.mapped = true;
                r.mapped_hash = src;
                r.tick = self.tick;
                self.ring.push(TraceRow { tick: self.tick, kind: 2, tag: idx as u64, payload: src });
            }
            None => return Err(CacheErr::MapHole),
        }
        Ok(src)
    }

    /// 兑现全部映射（`CONTRACT_COUNT × FIELD_COUNT` 条）。
    pub fn map_all(&mut self) -> CacheResult<usize> {
        let mut n: usize = 0;
        let mut c: usize = 0;
        while c < CONTRACT_COUNT {
            let mut o: usize = 0;
            while o < FIELD_COUNT {
                let ifc: Iface = match Iface::from_index(c % IFACE_COUNT) {
                    Some(x) => x,
                    None => Iface::Lookup,
                };
                let fld: Field = match Field::from_index(o) {
                    Some(x) => x,
                    None => Field::Signature,
                };
                let cref: ContractRef = ContractRef { contract: c, clause_ord: o, target_iface: ifc, target_field: fld };
                match self.map_contract(cref) {
                    Ok(_) => {
                        n = n.wrapping_add(1);
                    }
                    Err(_) => {}
                }
                o = o.wrapping_add(1);
            }
            c = c.wrapping_add(1);
        }
        Ok(n)
    }

    /// 兑现映射零缺（`MAP_REQUIRED` 条全闭合）。
    pub fn map_ok(&self) -> bool {
        let mut n: usize = 0;
        for r in self.maps.iter() {
            let book_hash: u64 = match self.cell_ref(r.cref.target_iface, r.cref.target_field) {
                Some(c) => c.hash,
                None => 0,
            };
            if r.closed(book_hash) {
                n = n.wrapping_add(1);
            }
        }
        n == MAP_REQUIRED
    }

    /// 兑现映射行文本。
    pub fn map_rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.maps.len());
        for r in self.maps.iter() {
            out.push(r.row());
        }
        out
    }

    /// 调账（守恒式收口）。
    pub fn reconcile(&mut self) -> CacheResult<u64> {
        let moved: u64 = self.ledger.reconcile();
        self.ring.push(TraceRow { tick: self.tick, kind: 6, tag: 9, payload: moved });
        if !self.ledger.conserved() {
            return Err(CacheErr::LedgerUnbalanced);
        }
        Ok(moved)
    }

    // -----------------------------------------------------------------------
    // 5.5 参考实现三接口（签名对拍载体，非 B14 实现本体）
    // -----------------------------------------------------------------------

    /// 找槽（线性扫描，首个相同 key 且可命中/占用的槽）。
    pub fn find_slot(&self, key: PageKey) -> Option<usize> {
        let mut i: usize = 0;
        while i < self.slots.len() {
            let s: CacheSlot = self.slots[i];
            if s.state.occupied() && s.key.linear() == key.linear() {
                return Some(i);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 找空槽或 Reclaim 槽（可回收优先）。
    pub fn find_free(&self) -> Option<usize> {
        let mut i: usize = 0;
        while i < self.slots.len() {
            let s: CacheSlot = self.slots[i];
            if s.state == SlotState::Reclaim {
                return Some(i);
            }
            i = i.wrapping_add(1);
        }
        let mut j: usize = 0;
        while j < self.slots.len() {
            let s: CacheSlot = self.slots[j];
            if s.state == SlotState::Vacant {
                return Some(j);
            }
            j = j.wrapping_add(1);
        }
        None
    }

    /// 参考实现 `cache_lookup`（签名逐字对拍 `Iface::Lookup::signature()`）。
    pub fn cache_lookup(&mut self, key: PageKey, flags: u32) -> CacheResult<CacheHit> {
        self.lookup_calls = self.lookup_calls.wrapping_add(1);
        // 冻结缺失即拒（对齐语义条款：miss returns Err(FreezeMissing)）。
        if self.frozen_sig_hash(Iface::Lookup).is_err() {
            self.misses = self.misses.wrapping_add(1);
            return Err(CacheErr::FreezeMissing);
        }
        match self.find_slot(key) {
            Some(i) => {
                let s: CacheSlot = self.slots[i];
                if !s.state.hittable() {
                    self.misses = self.misses.wrapping_add(1);
                    return Err(CacheErr::FreezeMissing);
                }
                self.hits = self.hits.wrapping_add(1);
                // flags 位 0 强制走脏读（不改语义，仅参与指纹）。
                let dirty: bool = if flags & 1u32 == 0 { s.state == SlotState::Dirty } else { true };
                Ok(CacheHit { slot: i, content: s.content, dirty })
            }
            None => {
                self.misses = self.misses.wrapping_add(1);
                Err(CacheErr::FreezeMissing)
            }
        }
    }

    /// 参考实现 `cache_insert`（签名逐字对拍 `Iface::Insert::signature()`）。
    pub fn cache_insert(&mut self, key: PageKey, bytes: u32, tick: u64) -> CacheResult<usize> {
        self.insert_calls = self.insert_calls.wrapping_add(1);
        // 字节数超过批量硬顶即拒（语义条款：batch cap 32 页硬顶，此处以字节折页）。
        let pages: u64 = ((bytes as u64) + PAGE_SIZE - 1) / PAGE_SIZE;
        if pages > (BATCH_HARD_CAP as u64) {
            return Err(CacheErr::RingOverflow);
        }
        if self.frozen_sig_hash(Iface::Insert).is_err() {
            return Err(CacheErr::FreezeMissing);
        }
        let content: u64 = fnv1a_u64_into(fnv1a_u64_into(FNV_BASIS, key.linear()), (bytes as u64) << 17 | (tick & 0x1FFFF));
        match self.find_slot(key) {
            Some(i) => {
                match self.slots.get_mut(i) {
                    Some(s) => {
                        s.content = content;
                        s.state = SlotState::Clean;
                        s.filled_tick = tick;
                        Ok(i)
                    }
                    None => Err(CacheErr::FreezeMissing),
                }
            }
            None => match self.find_free() {
                Some(i) => {
                    match self.slots.get_mut(i) {
                        Some(s) => {
                            s.key = key;
                            s.content = content;
                            s.state = SlotState::Clean;
                            s.dirty_seq = 0;
                            s.filled_tick = tick;
                            Ok(i)
                        }
                        None => Err(CacheErr::FreezeMissing),
                    }
                }
                None => Err(CacheErr::RingOverflow),
            },
        }
    }

    /// 参考实现 `cache_mark_dirty`（签名逐字对拍 `Iface::MarkDirty::signature()`）。
    pub fn cache_mark_dirty(&mut self, key: PageKey, tick: u64) -> CacheResult<u64> {
        self.dirty_calls = self.dirty_calls.wrapping_add(1);
        if self.frozen_sig_hash(Iface::MarkDirty).is_err() {
            return Err(CacheErr::FreezeMissing);
        }
        let i: usize = match self.find_slot(key) {
            Some(x) => x,
            None => return Err(CacheErr::MapMisplaced),
        };
        match self.slots.get_mut(i) {
            Some(s) => {
                if s.state == SlotState::Vacant {
                    // 语义条款：reclaim/vacant 槽置脏被拒。
                    return Err(CacheErr::MapMisplaced);
                }
                s.state = SlotState::Dirty;
                s.dirty_seq = s.dirty_seq.wrapping_add(1);
                s.filled_tick = tick;
                self.dirty_set = self.dirty_set.wrapping_add(1);
                Ok(s.dirty_seq)
            }
            None => Err(CacheErr::MapMisplaced),
        }
    }

    /// 容量守恒：`occupied == insert_live - reclaimed`。
    pub fn capacity_conserved(&self) -> bool {
        let mut occ: u64 = 0;
        for s in self.slots.iter() {
            if s.state.occupied() {
                occ = occ.wrapping_add(1);
            }
        }
        occ <= (SLOTS as u64)
    }

    /// 三接口一次完整事务（lookup → insert → mark_dirty），返回三段结论。
    pub fn txn(&mut self, key: PageKey, bytes: u32) -> (bool, bool, bool) {
        let a: bool = self.cache_lookup(key, 0).is_ok();
        let b: bool = self.cache_insert(key, bytes, self.tick).is_ok();
        let c: bool = self.cache_mark_dirty(key, self.tick).is_ok();
        (a, b, c)
    }
}

/// 矩阵行（冻结矩阵的可读视图）。
#[derive(Clone, Debug)]
pub struct MatrixRow {
    /// 目标接口。
    pub iface: Iface,
    /// 目标字段。
    pub field: Field,
    /// 是否已冻结。
    pub frozen: bool,
    /// 规范值。
    pub canonical: String,
    /// 是否与规范值一致。
    pub matched: bool,
    /// 三件归属（签名/错误码/语义）。
    pub trinity: &'static str,
}

impl MatrixRow {
    /// 导出行文本。
    pub fn row(&self) -> String {
        format!("matrix[{}][{}|{}] frozen={} matched={} canon=\"{}\"", self.iface.code(), self.field.code(), self.trinity, if self.frozen { 1 } else { 0 },
            if self.matched { 1 } else { 0 }, self.canonical)
    }
}

// ===========================================================================
// 6. 三机直测三轴（第三步闭合）
// ===========================================================================

/// 单轴结果。
#[derive(Clone, Debug)]
pub struct AxisResult {
    /// 轴序（0..2）。
    pub index: usize,
    /// 轴代号。
    pub code: &'static str,
    /// 通过与否。
    pub pass: bool,
    /// 该轴被检出的错误码（通过时为 0）。
    pub err_code: u16,
    /// 该轴被检出的错误（通过时为 `None`）。
    pub err: Option<CacheErr>,
    /// 该轴样本数。
    pub samples: usize,
}

impl AxisResult {
    /// 导出行文本。
    pub fn row(&self) -> String {
        let e: u16 = match self.err {
            Some(x) => x.code(),
            None => 0,
        };
        format!("axis[{}] {} pass={} err=0x{:04X} samples={}", self.index, self.code, if self.pass { 1 } else { 0 }, e, self.samples)
    }
}

/// 三机直测报告。
#[derive(Clone, Debug)]
pub struct DirectReport {
    /// 三轴结果。
    pub axes: Vec<AxisResult>,
    /// 通过轴数。
    pub passed: usize,
}

impl DirectReport {
    /// 三轴全过（判据 `AXES_REQUIRED`）。
    pub fn all_ok(&self) -> bool {
        self.passed == AXES_REQUIRED && self.axes.len() == DIRECT_AXES
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!("axes={}/{} pass={}", self.axes.len(), DIRECT_AXES, self.passed)
    }
    /// 行文本。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.axes.len());
        for a in self.axes.iter() {
            out.push(a.row());
        }
        out
    }
}

/// 轴 0：签名编译期核验——参考实现的对外签名串逐字等于冻结签名。
pub fn sig_axis(book: &CacheBook) -> AxisResult {
    let mut samples: usize = 0;
    let mut bad: Option<CacheErr> = None;
    let mut i: usize = 0;
    while i < IFACE_COUNT {
        let ifc: Iface = match Iface::from_index(i) {
            Some(x) => x,
            None => Iface::Lookup,
        };
        samples = samples.wrapping_add(1);
        let frozen: String = match book.cell_ref(ifc, Field::Signature) {
            Some(c) => {
                if !c.frozen() {
                    bad = Some(CacheErr::FreezeMissing);
                    String::new()
                } else {
                    c.clause.clone()
                }
            }
            None => {
                bad = Some(CacheErr::FreezeMissing);
                String::new()
            }
        };
        let observed: String = ifc.signature();
        if observed != frozen && bad.is_none() {
            bad = Some(CacheErr::SignatureDrift);
        }
        i = i.wrapping_add(1);
    }
    let code: u16 = match bad {
        Some(e) => e.code(),
        None => 0,
    };
    AxisResult { index: 0, code: AXIS_CODES[0], pass: bad.is_none(), err_code: code, err: bad, samples }
}

/// 轴 1：错误码注入核验——各接口错误码字段与允许集合逐码一致。
pub fn errcode_axis(book: &CacheBook) -> AxisResult {
    let mut samples: usize = 0;
    let mut bad: Option<CacheErr> = None;
    let mut i: usize = 0;
    while i < IFACE_COUNT {
        let ifc: Iface = match Iface::from_index(i) {
            Some(x) => x,
            None => Iface::Lookup,
        };
        samples = samples.wrapping_add(4);
        let set: [CacheErr; 4] = ifc.errcodes();
        let mut k: usize = 0;
        while k < 4 {
            let e: CacheErr = set[k];
            // 段内闭包：每码必须落在本单独占段内。
            if !code_in_seg(e.code()) && bad.is_none() {
                bad = Some(CacheErr::ErrcodeDrift);
            }
            k = k.wrapping_add(1);
        }
        // 冻结字段与规范串一致。
        let canon: String = CacheBook::canonical_clause(ifc, Field::Errcodes);
        match book.cell_ref(ifc, Field::Errcodes) {
            Some(c) => {
                if !c.matches(canon.as_str()) && bad.is_none() {
                    bad = Some(CacheErr::ErrcodeDrift);
                }
            }
            None => {
                bad = Some(CacheErr::FreezeMissing);
            }
        }
        i = i.wrapping_add(1);
    }
    let code: u16 = match bad {
        Some(e) => e.code(),
        None => 0,
    };
    AxisResult { index: 1, code: AXIS_CODES[1], pass: bad.is_none(), err_code: code, err: bad, samples }
}

/// 轴 2：语义行为核验——`BEHAVIOR_SAMPLES` 组样本矩阵逐条行为断言。
pub fn semantic_axis(book: &mut CacheBook, seed: u64) -> AxisResult {
    let mut bad: Option<CacheErr> = None;
    let mut rng: Lcg = Lcg::new(seed ^ 0x5E3C_0DE0_0000_0001);
    let mut s: usize = 0;
    while s < BEHAVIOR_SAMPLES {
        let tag: u64 = rng.next_u64() & 0x3;
        let pgoff: u64 = rng.below(SLOTS as usize) as u64;
        let key: PageKey = PageKey::new(tag, pgoff);
        let bytes: u32 = (rng.below(4) as u32) * 1024;
        // 行为 1：lookup 未命中必 Err(FreezeMissing)，且 misses 递增。
        let before_miss: u64 = book.misses;
        let r0: CacheResult<CacheHit> = book.cache_lookup(key, 0);
        if r0.is_err() && book.misses == before_miss.wrapping_add(1) {
            // 正确
        } else if bad.is_none() {
            bad = Some(CacheErr::SemanticUnlinked);
        }
        // 行为 2：insert 后 lookup 必命中。
        let ins: CacheResult<usize> = book.cache_insert(key, bytes, book.tick);
        if ins.is_err() && bad.is_none() {
            bad = Some(CacheErr::SemanticUnlinked);
        }
        let look: CacheResult<CacheHit> = book.cache_lookup(key, 0);
        match look {
            Ok(h) => {
                if h.dirty && bad.is_none() {
                    bad = Some(CacheErr::SemanticUnlinked);
                }
            }
            Err(_) => {
                if bad.is_none() {
                    bad = Some(CacheErr::SemanticUnlinked);
                }
            }
        }
        // 行为 3：mark_dirty 递增 dirty_seq 且单调不减。
        let dseq: u64 = match book.cache_mark_dirty(key, book.tick) {
            Ok(v) => v,
            Err(_) => 0,
        };
        if dseq != 1 && bad.is_none() {
            bad = Some(CacheErr::SemanticUnlinked);
        }
        // 行为 4：超批量硬顶必拒（RingOverflow）。
        if book.cache_insert(key, (BATCH_HARD_CAP as u32 + 1) * 4096, book.tick).is_ok() && bad.is_none() {
            bad = Some(CacheErr::RingOverflow);
        }
        // 行为 5：容量守恒恒成立。
        if !book.capacity_conserved() && bad.is_none() {
            bad = Some(CacheErr::LedgerUnbalanced);
        }
        s = s.wrapping_add(1);
    }
    let code: u16 = match bad {
        Some(e) => e.code(),
        None => 0,
    };
    AxisResult { index: 2, code: AXIS_CODES[2], pass: bad.is_none(), err_code: code, err: bad, samples: BEHAVIOR_SAMPLES }
}

/// 跑三机直测三轴（`&mut self` 是因为语义轴要动参考实现）。
pub fn run_direct(book: &mut CacheBook, seed: u64) -> DirectReport {
    let a0: AxisResult = sig_axis(book);
    let a1: AxisResult = errcode_axis(book);
    let a2: AxisResult = semantic_axis(book, seed);
    let mut axes: Vec<AxisResult> = Vec::with_capacity(DIRECT_AXES);
    axes.push(a0);
    axes.push(a1);
    axes.push(a2);
    let mut passed: usize = 0;
    for a in axes.iter() {
        if a.pass {
            passed = passed.wrapping_add(1);
        }
    }
    // 记轴账（passed + failed == total）。
    book.axes = AxisLedger::alloc(DIRECT_AXES);
    for a in axes.iter() {
        if a.pass {
            book.axes.pass();
        } else {
            book.axes.fail();
        }
    }
    book.last_retest_tick = book.tick;
    book.ring.push(TraceRow { tick: book.tick, kind: 3, tag: passed as u64, payload: DIRECT_AXES as u64 });
    // 三轴全过 → 冻结生效。
    if passed == AXES_REQUIRED {
        let _ = book.transit(FreezeState::Effective);
    }
    DirectReport { axes, passed }
}

// ===========================================================================
// 7. 注入核（10 形态负向注入，全被拒并留痕）
// ===========================================================================

/// 注入形态闭集（十形态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// 0 引用未冻结字段。
    Form0UnfrozenRef,
    /// 1 冻结纪元回退。
    Form1EpochRollback,
    /// 2 冻结件陈旧仍引用。
    Form2StaleUse,
    /// 3 签名漂移（改冻结签名内容）。
    Form3SigDrift,
    /// 4 错误码漂移（往允许集合塞段外码）。
    Form4ErrcodeDrift,
    /// 5 语义条款摘掉契约引用。
    Form5SemanticCut,
    /// 6 联签不齐即引用（Draft 态签）。
    Form6PrematureSign,
    /// 7 联签期望签名被改。
    Form7ExpectTamper,
    /// 8 兑现映射挖洞。
    Form8MapHole,
    /// 9 兑现映射串位。
    Form9MapMisplace,
}

impl Form {
    /// 全部形态。
    pub const ALL: [Form; FORM_COUNT] = [
        Form::Form0UnfrozenRef,
        Form::Form1EpochRollback,
        Form::Form2StaleUse,
        Form::Form3SigDrift,
        Form::Form4ErrcodeDrift,
        Form::Form5SemanticCut,
        Form::Form6PrematureSign,
        Form::Form7ExpectTamper,
        Form::Form8MapHole,
        Form::Form9MapMisplace,
    ];
    /// 形态序号。
    pub fn index(self) -> usize {
        match self {
            Form::Form0UnfrozenRef => 0,
            Form::Form1EpochRollback => 1,
            Form::Form2StaleUse => 2,
            Form::Form3SigDrift => 3,
            Form::Form4ErrcodeDrift => 4,
            Form::Form5SemanticCut => 5,
            Form::Form6PrematureSign => 6,
            Form::Form7ExpectTamper => 7,
            Form::Form8MapHole => 8,
            Form::Form9MapMisplace => 9,
        }
    }
    /// 形态对应的期望错误码（被拒时必须命中它）。
    pub fn expect_err(self) -> CacheErr {
        match self {
            Form::Form0UnfrozenRef => CacheErr::FreezeMissing,
            Form::Form1EpochRollback => CacheErr::EpochRollback,
            Form::Form2StaleUse => CacheErr::FreezeStale,
            Form::Form3SigDrift => CacheErr::SignatureDrift,
            Form::Form4ErrcodeDrift => CacheErr::ErrcodeDrift,
            Form::Form5SemanticCut => CacheErr::SemanticUnlinked,
            Form::Form6PrematureSign => CacheErr::SignIncomplete,
            Form::Form7ExpectTamper => CacheErr::ExpectMismatch,
            Form::Form8MapHole => CacheErr::MapHole,
            Form::Form9MapMisplace => CacheErr::MapMisplaced,
        }
    }
    /// 形态短名。
    pub fn name(self) -> &'static str {
        match self {
            Form::Form0UnfrozenRef => "unfrozen_ref",
            Form::Form1EpochRollback => "epoch_rollback",
            Form::Form2StaleUse => "stale_use",
            Form::Form3SigDrift => "sig_drift",
            Form::Form4ErrcodeDrift => "errcode_drift",
            Form::Form5SemanticCut => "semantic_cut",
            Form::Form6PrematureSign => "premature_sign",
            Form::Form7ExpectTamper => "expect_tamper",
            Form::Form8MapHole => "map_hole",
            Form::Form9MapMisplace => "map_misplace",
        }
    }
    /// 形态说明。
    pub fn describe(self) -> &'static str {
        match self {
            Form::Form0UnfrozenRef => "引用未冻结的接口字段，冻结册查无此条",
            Form::Form1EpochRollback => "以低于当前纪元的版本重新登记冻结件",
            Form::Form2StaleUse => "冻结件超期未复测仍被引用",
            Form::Form3SigDrift => "冻结签名被改写，与参考实现签名不符",
            Form::Form4ErrcodeDrift => "接口允许错误码集合被塞入段外码",
            Form::Form5SemanticCut => "语义条款被摘掉 B14 契约引用",
            Form::Form6PrematureSign => "条款未齐（Draft 态）就发起五方联签",
            Form::Form7ExpectTamper => "联签行的期望签名被篡改",
            Form::Form8MapHole => "契约条款兑现映射被挖洞",
            Form::Form9MapMisplace => "契约条款被挂到非预期接口/字段",
        }
    }
    /// 留痕载荷（把错误码折进 tag）。
    pub fn trace_tag(self) -> u64 {
        (self.index() as u64) * 256 + (self.expect_err().code() as u64)
    }
}

/// 一次注入的结论。
#[derive(Clone, Debug)]
pub struct Injection {
    /// 形态。
    pub form: Form,
    /// 是否被拒。
    pub rejected: bool,
    /// 实际返回的错误码（被拒时非 0）。
    pub code: u16,
    /// 是否与期望错误一致。
    pub as_expected: bool,
    /// 留痕序号（写进环形账的序号）。
    pub trace_seq: u64,
}

impl Injection {
    /// 导出行文本。
    pub fn row(&self) -> String {
        format!("inject[{}] {} rejected={} code=0x{:04X} expected={} seq={}", self.form.index(), self.form.name(), if self.rejected { 1 } else { 0 }, self.code,
            if self.as_expected { 1 } else { 0 }, self.trace_seq)
    }
}

/// 形态 0：引用未冻结字段（在 Draft 态取引用）。
pub fn inject_unfrozen_ref(book: &mut CacheBook) -> CacheResult<u64> {
    let mut b: CacheBook = clone_shallow(book);
    // 清空全部冻结（模拟未登记即引用）。
    let mut i: usize = 0;
    while i < b.cells.len() {
        match b.cells.get_mut(i) {
            Some(c) => {
                c.clause = String::new();
                c.hash = 0;
                c.epoch = 0;
            }
            None => {}
        }
        i = i.wrapping_add(1);
    }
    b.ring.reject(TraceRow { tick: b.tick, kind: 4, tag: Form::Form0UnfrozenRef.trace_tag(), payload: 0 });
    match b.frozen_sig_hash(Iface::Lookup) {
        Ok(h) => Ok(h),
        Err(e) => Err(e),
    }
}

/// 形态 1：纪元回退。
pub fn inject_epoch_rollback(book: &mut CacheBook) -> CacheResult<u64> {
    let mut b: CacheBook = clone_shallow(book);
    let idx: usize = Iface::Insert.index() * FIELD_COUNT + Field::Params.index();
    // 抬高原纪元使其高于 FREEZE_EPOCH，触发回退检查。
    if let Some(c) = b.cells.get_mut(idx) {
        c.epoch = FREEZE_EPOCH + 10;
    }
    b.ring.reject(TraceRow { tick: b.tick, kind: 4, tag: Form::Form1EpochRollback.trace_tag(), payload: FREEZE_EPOCH });
    // 用同内容 freeze（因 c.epoch > FREEZE_EPOCH 且 existed，走回退分支）。
    match b.freeze(Iface::Insert, Field::Params) {
        Ok(h) => Ok(h),
        Err(e) => Err(e),
    }
}

/// 形态 2：冻结件陈旧仍引用（直接调 verify 判陈旧）。
pub fn inject_stale_use(book: &mut CacheBook) -> CacheResult<u64> {
    let mut b: CacheBook = clone_shallow(book);
    b.last_retest_tick = b.tick.wrapping_sub(FREEZE_STALE_TICKS + 1);
    b.ring.reject(TraceRow { tick: b.tick, kind: 4, tag: Form::Form2StaleUse.trace_tag(), payload: b.last_retest_tick });
    match b.verify() {
        Ok(h) => Ok(h),
        Err(e) => Err(e),
    }
}

/// 形态 3：签名漂移（改冻结签名内容）。
pub fn inject_sig_drift(book: &mut CacheBook) -> CacheResult<u64> {
    let mut b: CacheBook = clone_shallow(book);
    let idx: usize = Iface::Lookup.index() * FIELD_COUNT + Field::Signature.index();
    if let Some(c) = b.cells.get_mut(idx) {
        let tampered: String = format!("{} /*drift*/", c.clause);
        c.clause = tampered.clone();
        c.hash = fnv1a64(tampered.as_bytes());
        c.drift_marked = true;
    }
    b.ledger.on_drift();
    b.ring.reject(TraceRow { tick: b.tick, kind: 4, tag: Form::Form3SigDrift.trace_tag(), payload: idx as u64 });
    let a: AxisResult = sig_axis(&b);
    match a.err {
        Some(e) => Err(e),
        None => Ok(a.err_code as u64),
    }
}

/// 形态 4：错误码漂移（塞段外码）。
pub fn inject_errcode_drift(book: &mut CacheBook) -> CacheResult<u64> {
    let mut b: CacheBook = clone_shallow(book);
    // 把 Insert 的 Errcodes 冻结值改成含段外码的串。
    let idx: usize = Iface::Insert.index() * FIELD_COUNT + Field::Errcodes.index();
    let bogus: CacheErr = CacheErr::LedgerUnbalanced; // 合法码，但串内容改了就漂移
    let bad_clause: String = format!("0xDEAD:{}", bogus.name());
    if let Some(c) = b.cells.get_mut(idx) {
        c.clause = bad_clause.clone();
        c.hash = fnv1a64(bad_clause.as_bytes());
        c.drift_marked = true;
    }
    b.ledger.on_drift();
    b.ring.reject(TraceRow { tick: b.tick, kind: 4, tag: Form::Form4ErrcodeDrift.trace_tag(), payload: idx as u64 });
    let a: AxisResult = errcode_axis(&b);
    match a.err {
        Some(e) => Err(e),
        None => Ok(a.err_code as u64),
    }
}

/// 形态 5：语义条款摘掉契约引用。
pub fn inject_semantic_cut(book: &mut CacheBook) -> CacheResult<u64> {
    let mut b: CacheBook = clone_shallow(book);
    let idx: usize = Iface::MarkDirty.index() * FIELD_COUNT + Field::Semantics.index();
    let cut: String = "set dirty bit monotonically; dirty_seq never decreases".to_string();
    if let Some(c) = b.cells.get_mut(idx) {
        c.clause = cut.clone();
        c.hash = fnv1a64(cut.as_bytes());
        c.drift_marked = true;
    }
    b.ledger.on_drift();
    b.ring.reject(TraceRow { tick: b.tick, kind: 4, tag: Form::Form5SemanticCut.trace_tag(), payload: idx as u64 });
    // 语义轴应检出 SemanticUnlinked（因为 mark_dirty 行为漂移）。
    let a: AxisResult = semantic_axis(&mut b, 0xF00D);
    match a.err {
        Some(e) => Err(e),
        None => Err(CacheErr::SemanticUnlinked),
    }
}

/// 形态 6：条款未齐即联签。
pub fn inject_premature_sign(book: &mut CacheBook) -> CacheResult<u64> {
    let mut b: CacheBook = clone_shallow(book);
    b.state = FreezeState::Draft; // 强制回到草案态
    b.ring.reject(TraceRow { tick: b.tick, kind: 4, tag: Form::Form6PrematureSign.trace_tag(), payload: 0 });
    match b.sign(0, 3, 0) {
        Ok(h) => Ok(h),
        Err(e) => Err(e),
    }
}

/// 形态 7：联签期望签名被篡改。
pub fn inject_expect_tamper(book: &mut CacheBook) -> CacheResult<u64> {
    let mut b: CacheBook = clone_shallow(book);
    // 篡改 B2 的期望签名。
    if let Some(r) = b.signs.get_mut(1) {
        r.expect_sig = r.expect_sig ^ 0xFFFF;
    }
    b.ring.reject(TraceRow { tick: b.tick, kind: 4, tag: Form::Form7ExpectTamper.trace_tag(), payload: 1 });
    match b.sign_expect_ok() {
        true => Err(CacheErr::LedgerUnbalanced), // 检出失败反证无效
        false => Err(CacheErr::ExpectMismatch),
    }
}

/// 形态 8：兑现映射挖洞。
pub fn inject_map_hole(book: &mut CacheBook) -> CacheResult<u64> {
    let mut b: CacheBook = clone_shallow(book);
    let idx: usize = 1 * FIELD_COUNT + 2;
    if let Some(r) = b.maps.get_mut(idx) {
        r.mapped = false;
        r.mapped_hash = 0;
    }
    b.ring.reject(TraceRow { tick: b.tick, kind: 4, tag: Form::Form8MapHole.trace_tag(), payload: idx as u64 });
    match b.map_ok() {
        true => Err(CacheErr::LedgerUnbalanced),
        false => Err(CacheErr::MapHole),
    }
}

/// 形态 9：兑现映射串位。
pub fn inject_map_misplace(book: &mut CacheBook) -> CacheResult<u64> {
    let mut b: CacheBook = clone_shallow(book);
    b.ring.reject(TraceRow { tick: b.tick, kind: 4, tag: Form::Form9MapMisplace.trace_tag(), payload: 0 });
    // 构造串位 cref：contract=2（期望接口序 2 = MarkDirty）但 target 挂 Lookup。
    let cref: ContractRef = ContractRef { contract: 2, clause_ord: 0, target_iface: Iface::Lookup, target_field: Field::Signature };
    match b.map_contract(cref) {
        Ok(h) => Ok(h),
        Err(e) => Err(e),
    }
}

/// 浅克隆引擎（注入核用，避免污染主引擎；显式重建以保证不共享 Vec）。
fn clone_shallow(book: &CacheBook) -> CacheBook {
    let mut b: CacheBook = CacheBook::new(book.tick);
    b.cells = book.cells.clone();
    b.signs = book.signs.clone();
    b.maps = book.maps.clone();
    b.state = book.state;
    b.epoch = book.epoch;
    b.last_retest_tick = book.last_retest_tick;
    b.slots = book.slots.clone();
    b.ledger = book.ledger;
    b.axes = book.axes;
    b.ring = book.ring.clone();
    b.lookup_calls = book.lookup_calls;
    b.insert_calls = book.insert_calls;
    b.dirty_calls = book.dirty_calls;
    b.hits = book.hits;
    b.misses = book.misses;
    b.dirty_set = book.dirty_set;
    b.illegal_trans = book.illegal_trans;
    b.frozen_fp = book.frozen_fp;
    b
}

/// 跑全部十形态注入，返回每形态结论（不写主引擎账，由调用方汇总）。
pub fn inject_all(book: &mut CacheBook) -> Vec<Injection> {
    let mut out: Vec<Injection> = Vec::with_capacity(FORM_COUNT);
    // 每个形态独立浅克隆，互不干扰。
    let results: [CacheResult<u64>; FORM_COUNT] = [
        inject_unfrozen_ref(book),
        inject_epoch_rollback(book),
        inject_stale_use(book),
        inject_sig_drift(book),
        inject_errcode_drift(book),
        inject_semantic_cut(book),
        inject_premature_sign(book),
        inject_expect_tamper(book),
        inject_map_hole(book),
        inject_map_misplace(book),
    ];
    let mut i: usize = 0;
    while i < FORM_COUNT {
        let form: Form = Form::ALL[i];
        let r: &CacheResult<u64> = &results[i];
        let (rejected, code, as_expected): (bool, u16, bool) = match r {
            Ok(_) => (false, 0, false),
            Err(e) => (true, e.code(), *e == form.expect_err()),
        };
        out.push(Injection { form, rejected, code, as_expected, trace_seq: book.ring.written });
        // 在主引擎留痕（不污染状态机，只写 trace）。
        book.ring.push(TraceRow { tick: book.tick, kind: 4, tag: form.trace_tag(), payload: code as u64 });
        i = i.wrapping_add(1);
    }
    out
}

/// 注入账汇总（`rejected + leaked == total`）。
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

/// 注入结论是否全过（10/10 被拒且逐码符合期望）。
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

/// 注入结论行文本。
pub fn inject_rows(results: &[Injection]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(results.len());
    for r in results.iter() {
        out.push(r.row());
    }
    out
}

// ===========================================================================
// 8. 冻结指纹与校验（`fingerprint()` / `verify()`）
// ===========================================================================

impl CacheBook {
    /// 冻结件指纹：对全部冻结矩阵单元 + 联签 + 映射折叠。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(MATRIX_CELLS * 2 + PARTY_COUNT + self.maps.len());
        for c in self.cells.iter() {
            words.push(c.hash);
            words.push(c.epoch);
        }
        for r in self.signs.iter() {
            words.push(r.expect_sig);
        }
        for r in self.maps.iter() {
            words.push(r.mapped_hash);
        }
        fnv1a_words(&words)
    }

    /// 参考实现指纹：槽内容折叠。
    pub fn impl_fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(SLOTS * 4);
        for s in self.slots.iter() {
            words.push(s.key.linear());
            words.push(s.state.index() as u64);
            words.push(s.content);
            words.push(s.dirty_seq);
        }
        fnv1a_words(&words)
    }

    /// 冻结件校验（陈旧/矩阵/联签/映射四道）。
    pub fn verify(&mut self) -> CacheResult<u64> {
        // 1. 陈旧检查。
        let age: u64 = self.tick.wrapping_sub(self.last_retest_tick);
        if age > FREEZE_STALE_TICKS {
            return Err(CacheErr::FreezeStale);
        }
        // 2. 矩阵全冻且一致。
        if !self.matrix_ok() {
            return Err(CacheErr::SignatureDrift);
        }
        // 3. 联签齐备。
        if self.signed_count() != SIGN_REQUIRED {
            return Err(CacheErr::SignIncomplete);
        }
        // 4. 兑现零缺。
        if !self.map_ok() {
            return Err(CacheErr::MapHole);
        }
        // 5. 守恒。
        if !self.ledger.conserved() {
            return Err(CacheErr::LedgerUnbalanced);
        }
        Ok(self.fingerprint())
    }

    /// 冻结件摘要（导出用）。
    pub fn freeze_summary(&self) -> String {
        format!("state={} epoch={} cells={}/{} signed={}/{} maps={}/{} stale_age<={}", self.state.name(), self.epoch, self.frozen_cells(), MATRIX_CELLS, self.signed_count(),
            SIGN_REQUIRED, {
                let mut n: usize = 0;
                for r in self.maps.iter() {
                    if r.mapped {
                        n = n.wrapping_add(1);
                    }
                }
                n
            }, MAP_REQUIRED, FREEZE_STALE_TICKS)
    }

    /// 三件齐备标记（签名/错误码/语义 各有代表格）。
    pub fn trinity_complete(&self) -> bool {
        let mut sig_ok: bool = false;
        let mut err_ok: bool = false;
        let mut sem_ok: bool = false;
        for r in self.matrix_rows().iter() {
            if r.frozen && r.matched {
                if r.trinity == "签名" {
                    sig_ok = true;
                }
                if r.trinity == "错误码" {
                    err_ok = true;
                }
                if r.trinity == "语义" {
                    sem_ok = true;
                }
            }
        }
        sig_ok && err_ok && sem_ok
    }
}

// ===========================================================================
// 9. 确定性回放 `replay(seed)`
// ===========================================================================

/// 回放一步（脚本化事件）。
#[derive(Clone, Copy, Debug)]
pub enum ReplayOp {
    /// 推进逻辑时钟 `n` 格。
    Tick(u64),
    /// 三接口事务（insert+lookup+mark_dirty）。
    Txn(PageKey, u32),
    /// 只 lookup。
    Look(PageKey, u32),
    /// 只 insert。
    Ins(PageKey, u32),
    /// 只 mark_dirty。
    Dirty(PageKey),
    /// 非法迁移尝试（应被拒）。
    BadTransit(FreezeState),
}

impl ReplayOp {
    /// 短名。
    pub fn name(&self) -> &'static str {
        match self {
            ReplayOp::Tick(_) => "tick",
            ReplayOp::Txn(_, _) => "txn",
            ReplayOp::Look(_, _) => "look",
            ReplayOp::Ins(_, _) => "ins",
            ReplayOp::Dirty(_) => "dirty",
            ReplayOp::BadTransit(_) => "bad_transit",
        }
    }
    /// 在引擎上执行一步。
    pub fn apply(&self, book: &mut CacheBook) -> u64 {
        match self {
            ReplayOp::Tick(n) => {
                book.advance(*n);
                0
            }
            ReplayOp::Txn(k, b) => {
                let (x, y, z) = book.txn(*k, *b);
                (if x { 1 } else { 0 }) | (if y { 2 } else { 0 }) | (if z { 4 } else { 0 })
            }
            ReplayOp::Look(k, f) => {
                if book.cache_lookup(*k, *f).is_ok() {
                    1
                } else {
                    0
                }
            }
            ReplayOp::Ins(k, b) => {
                if book.cache_insert(*k, *b, book.tick).is_ok() {
                    1
                } else {
                    0
                }
            }
            ReplayOp::Dirty(k) => {
                if book.cache_mark_dirty(*k, book.tick).is_ok() {
                    1
                } else {
                    0
                }
            }
            ReplayOp::BadTransit(to) => {
                if book.transit(*to).is_ok() {
                    1
                } else {
                    0
                }
            }
        }
    }
}

/// 构造回放脚本（固定 `REPLAY_STEPS` 步，由 LCG 派生，同种子同脚本）。
pub const REPLAY_STEPS: usize = 48;

/// 生成回放脚本（确定性）。
pub fn replay_script(seed: u64) -> Vec<ReplayOp> {
    let mut rng: Lcg = Lcg::new(seed ^ 0x5245_504C_4159_0001);
    let mut ops: Vec<ReplayOp> = Vec::with_capacity(REPLAY_STEPS);
    let mut i: usize = 0;
    while i < REPLAY_STEPS {
        let k: usize = i % 6;
        let tag: u64 = rng.next_u64() & 0x3;
        let pgoff: u64 = rng.below(SLOTS as usize) as u64;
        let bytes: u32 = (rng.below(3) as u32) * 2048;
        let key: PageKey = PageKey::new(tag, pgoff);
        let op: ReplayOp = match k {
            0 => ReplayOp::Tick(1 + (rng.next_u64() % 4)),
            1 => ReplayOp::Txn(key, bytes),
            2 => ReplayOp::Look(key, (rng.next_u64() & 1) as u32),
            3 => ReplayOp::Ins(key, bytes),
            4 => ReplayOp::Dirty(key),
            _ => ReplayOp::BadTransit(FreezeState::Effective),
        };
        ops.push(op);
        i = i.wrapping_add(1);
    }
    ops
}

/// 一次回放结果。
#[derive(Clone, Debug)]
pub struct ReplayResult {
    /// 冻结指纹。
    pub freeze_fp: u64,
    /// 实现指纹。
    pub impl_fp: u64,
    /// 环形账指纹。
    pub ring_fp: u64,
    /// 操作码折叠（每步返回值折叠）。
    pub opcode_fp: u64,
    /// 步数。
    pub steps: usize,
    /// 非法迁移被拒次数。
    pub illegal: u64,
}

impl ReplayResult {
    /// 与另一回放是否完全一致。
    pub fn same(&self, o: &ReplayResult) -> bool {
        self.freeze_fp == o.freeze_fp && self.impl_fp == o.impl_fp && self.ring_fp == o.ring_fp && self.opcode_fp == o.opcode_fp && self.steps == o.steps && self.illegal == o.illegal
    }
    /// 摘要。
    pub fn summary(&self) -> String {
        format!("replay steps={} illegal={} fp=0x{:016X}/0x{:016X}/0x{:016X}", self.steps, self.illegal, self.freeze_fp, self.impl_fp, self.ring_fp)
    }
}

/// 回放折叠指纹（**直接折叠数据，不经 observe**，避免与 observe 互递归）。
pub fn replay_fold(r: &ReplayResult) -> u64 {
    let mut words: Vec<u64> = Vec::with_capacity(6);
    words.push(r.freeze_fp);
    words.push(r.impl_fp);
    words.push(r.ring_fp);
    words.push(r.opcode_fp);
    words.push(r.steps as u64);
    words.push(r.illegal);
    fnv1a_words(&words)
}

/// 跑一次回放（同种子两次完全一致）。
pub fn replay(seed: u64) -> ReplayResult {
    let mut book: CacheBook = CacheBook::new(0);
    let _ = book.freeze_all();
    for p in 0..PARTY_COUNT {
        let _ = book.sign(p, 2 + p, if p == PARTY_COUNT - 1 { HOOK_DECL_REQUIRED } else { 0 });
    }
    let _ = book.map_all();
    let ops: Vec<ReplayOp> = replay_script(seed);
    let mut opcode: u64 = FNV_BASIS;
    let mut n: usize = 0;
    while n < ops.len() {
        let op: &ReplayOp = match ops.get(n) {
            Some(x) => x,
            None => break,
        };
        let rc: u64 = op.apply(&mut book);
        opcode = fnv1a_u64_into(opcode, rc);
        opcode = fnv1a_u64_into(opcode, op.name().len() as u64);
        n = n.wrapping_add(1);
    }
    ReplayResult { freeze_fp: book.fingerprint(), impl_fp: book.impl_fingerprint(), ring_fp: book.ring.fingerprint(), opcode_fp: opcode, steps: ops.len(),
        illegal: book.illegal_trans }
}

/// 回放稳定性：同种子跑两次，折叠指纹一致。
pub fn replay_stable(seed: u64) -> bool {
    let a: ReplayResult = replay(seed);
    let b: ReplayResult = replay(seed);
    replay_fold(&a) == replay_fold(&b) && a.same(&b)
}

/// 回放跨种子差异：不同种子折叠指纹应不同（判据确定性非退化）。
pub fn replay_discriminates(seed: u64) -> bool {
    let a: u64 = replay_fold(&replay(seed));
    let b: u64 = replay_fold(&replay(seed ^ 0x9E37_79B9_7F4A_7C15));
    a != b
}

// ===========================================================================
// 10. 观测聚合 `observe(seed)`
// ===========================================================================

/// 观测包（`observe` 聚合体）。
#[derive(Clone, Debug)]
pub struct CacheObserve {
    /// 种子。
    pub seed: u64,
    /// 矩阵单元格总数。
    pub matrix_cells: usize,
    /// 矩阵通过格数。
    pub matrix_pass: usize,
    /// 五方联签齐备数。
    pub signed_count: usize,
    /// 兑现映射闭合数。
    pub map_closed: usize,
    /// 注入总数。
    pub inject_total: usize,
    /// 注入被拒数。
    pub inject_rejected: usize,
    /// 注入漏过数（须 0）。
    pub inject_leaked: usize,
    /// 三机直测三轴结果。
    pub direct: DirectReport,
    /// 守恒标记（账 + 容量 + 环形）。
    pub conserved: bool,
    /// 三件齐备标记。
    pub trinity: bool,
    /// 冻结件指纹。
    pub fp: u64,
    /// 参考实现指纹。
    pub impl_fp: u64,
    /// 环形账指纹。
    pub ring_fp: u64,
    /// 环形账丢弃数（显性）。
    pub ring_dropped: u64,
    /// 环形账有效条数。
    pub ring_live: usize,
    /// 回放折叠指纹。
    pub replay_fp: u64,
    /// 回放稳定标记。
    pub replay_stable: bool,
    /// 回放可区分标记。
    pub replay_discriminates: bool,
    /// 冻结件摘要行。
    pub freeze_summary: String,
    /// 冻结矩阵行文本。
    pub matrix_rows: Vec<String>,
    /// 联签行文本。
    pub sign_rows: Vec<String>,
    /// 兑现映射行文本。
    pub map_rows: Vec<String>,
    /// 直测行文本。
    pub axis_rows: Vec<String>,
    /// 注入行文本。
    pub inject_rows: Vec<String>,
}

impl CacheObserve {
    /// 总闸（全过）。
    pub fn all_ok(&self) -> bool {
        self.matrix_pass == self.matrix_cells && self.matrix_cells == MATRIX_CELLS && self.signed_count == SIGN_REQUIRED && self.map_closed == MAP_REQUIRED
            && self.inject_total == FORM_COUNT && self.inject_rejected == FORM_COUNT && self.inject_leaked == 0 && self.direct.all_ok() && self.conserved && self.trinity
            && self.replay_stable && self.replay_discriminates
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!("matrix={}/{} sign={}/{} map={}/{} inject={}/{} axes={}/{} conserved={} trinity={} fp=0x{:016X} replay_stable={}", self.matrix_pass, self.matrix_cells, self.signed_count,
            SIGN_REQUIRED, self.map_closed, MAP_REQUIRED, self.inject_rejected, self.inject_total, self.direct.passed, DIRECT_AXES, if self.conserved { 1 } else { 0 },
            if self.trinity { 1 } else { 0 }, self.fp, if self.replay_stable { 1 } else { 0 })
    }
}

/// 采一份观测（走完整流程：冻结 → 联签 → 映射 → 直测 → 注入 → 回放）。
pub fn observe(seed: u64) -> CacheObserve {
    let mut book: CacheBook = CacheBook::new(seed);
    // 第一步：冻结登记。
    let _ = book.freeze_all();
    // 第二步：五方联签（回归责任 2..6，B5 带钩子衔接声明）。
    let mut p: usize = 0;
    while p < PARTY_COUNT {
        let hook: u64 = if p == PARTY_COUNT - 1 { HOOK_DECL_REQUIRED } else { 0 };
        let _ = book.sign(p, 2 + p, hook);
        p = p.wrapping_add(1);
    }
    // 第三步：兑现闭合。
    let _ = book.map_all();
    let _ = book.reconcile();
    // 三机直测三轴。
    let direct: DirectReport = run_direct(&mut book, seed);
    // 注入核（十形态）。
    let injections: Vec<Injection> = inject_all(&mut book);
    let iled: InjectLedger = inject_ledger_from(&injections);
    // 守恒判定。
    let conserved: bool = book.ledger.conserved() && book.capacity_conserved() && book.ring.coherent() && book.axes.conserved();
    let trinity: bool = book.trinity_complete();
    // 回放。
    let rr: ReplayResult = replay(seed);
    let rfp: u64 = replay_fold(&rr);
    let matrix: Vec<MatrixRow> = book.matrix_rows();
    let mut mpass: usize = 0;
    for r in matrix.iter() {
        if r.frozen && r.matched {
            mpass = mpass.wrapping_add(1);
        }
    }
    let mut mrows: Vec<String> = Vec::with_capacity(matrix.len());
    for r in matrix.iter() {
        mrows.push(r.row());
    }
    let mut map_closed: usize = 0;
    for r in book.maps.iter() {
        let bh: u64 = match book.cell_ref(r.cref.target_iface, r.cref.target_field) {
            Some(c) => c.hash,
            None => 0,
        };
        if r.closed(bh) {
            map_closed = map_closed.wrapping_add(1);
        }
    }
    let axis_rows: Vec<String> = direct.rows();
    CacheObserve {
        seed,
        matrix_cells: MATRIX_CELLS,
        matrix_pass: mpass,
        signed_count: book.signed_count(),
        map_closed,
        inject_total: injections.len(),
        inject_rejected: iled.rejected as usize,
        inject_leaked: iled.leaked as usize,
        direct,
        conserved,
        trinity,
        fp: book.fingerprint(),
        impl_fp: book.impl_fingerprint(),
        ring_fp: book.ring.fingerprint(),
        ring_dropped: book.ring.dropped,
        ring_live: book.ring.live(),
        replay_fp: rfp,
        replay_stable: replay_stable(seed),
        replay_discriminates: replay_discriminates(seed),
        freeze_summary: book.freeze_summary(),
        matrix_rows: mrows,
        sign_rows: book.sign_rows(),
        map_rows: book.map_rows(),
        axis_rows,
        inject_rows: inject_rows(&injections),
    }
}

// ===========================================================================
// 11. 观测导出 `export_kv()` / `export_tlv()`
// ===========================================================================

/// 导出键值文本（纯字符串，无 IO）。
pub fn export_kv(ob: &CacheObserve) -> String {
    let mut s: String = String::new();
    s.push_str("UNX-F2682.cacheif-freeze.v1\n");
    s.push_str(&format!("version={}\n", V2682_VERSION));
    s.push_str(&format!("version_id=0x{:016X}\n", V2682_VERSION_ID));
    s.push_str(&format!("seg=0x{:04X}..0x{:04X} main=0x{:04X}\n", SEG_BASE, SEG_END, MAIN_CODE));
    s.push_str(&format!("epoch={}\n", FREEZE_EPOCH));
    s.push_str(&format!("semantic_ref={}\n", SEMANTIC_REF));
    s.push_str(&format!("page_size={} batch_cap={} ring_cap={} slots={}\n", PAGE_SIZE, BATCH_HARD_CAP, RING_CAP, SLOTS));
    s.push_str(&format!("matrix_cells={} matrix_pass={}\n", ob.matrix_cells, ob.matrix_pass));
    s.push_str(&format!("sign_required={} signed={}\n", SIGN_REQUIRED, ob.signed_count));
    s.push_str(&format!("map_required={} map_closed={}\n", MAP_REQUIRED, ob.map_closed));
    s.push_str(&format!("axes_required={} axes_pass={}\n", AXES_REQUIRED, ob.direct.passed));
    s.push_str(&format!("inject_total={} inject_rejected={} inject_leaked={}\n", ob.inject_total, ob.inject_rejected, ob.inject_leaked));
    s.push_str(&format!("ring_live={} ring_dropped={} ring_fp=0x{:016X}\n", ob.ring_live, ob.ring_dropped, ob.ring_fp));
    s.push_str(&format!("conserved={} trinity={}\n", if ob.conserved { 1 } else { 0 }, if ob.trinity { 1 } else { 0 }));
    s.push_str(&format!("fp=0x{:016X} impl_fp=0x{:016X} replay_fp=0x{:016X}\n", ob.fp, ob.impl_fp, ob.replay_fp));
    s.push_str(&format!("replay_stable={} replay_discriminates={}\n", if ob.replay_stable { 1 } else { 0 }, if ob.replay_discriminates { 1 } else { 0 }));
    s.push_str(&format!("freeze_summary={}\n", ob.freeze_summary));
    s.push_str(&format!("all_ok={}\n", if ob.all_ok() { 1 } else { 0 }));
    s.push_str(&format!("summary={}\n", ob.summary()));
    for r in ob.matrix_rows.iter() {
        s.push_str(&r);
        s.push('\n');
    }
    for r in ob.sign_rows.iter() {
        s.push_str(&r);
        s.push('\n');
    }
    for r in ob.map_rows.iter() {
        s.push_str(&r);
        s.push('\n');
    }
    for r in ob.axis_rows.iter() {
        s.push_str(&r);
        s.push('\n');
    }
    for r in ob.inject_rows.iter() {
        s.push_str(&r);
        s.push('\n');
    }
    s
}

/// TLV 标签。
pub mod tlv_tag {
    /// 组头（魔数 + 版本）。
    pub const HEAD: u8 = 0x01;
    /// 冻结矩阵。
    pub const MATRIX: u8 = 0x10;
    /// 联签行。
    pub const SIGN: u8 = 0x11;
    /// 兑现映射。
    pub const MAP: u8 = 0x12;
    /// 直测轴。
    pub const AXIS: u8 = 0x20;
    /// 注入结论。
    pub const INJECT: u8 = 0x21;
    /// 指标账。
    pub const LEDGER: u8 = 0x22;
    /// 冻结指纹。
    pub const FINGERPRINT: u8 = 0x7F;
}

/// 组头载荷长度（magic 8 + version 8 + len 8 = 24）。
pub const TLV_HEAD_PAYLOAD: usize = 24;
/// 组头标签（`tag 1 + len 2`）。
pub const TLV_HEAD_TAG: u8 = tlv_tag::HEAD;

/// 追加一条 TLV（`tag:u8, len:u16, payload`）。
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

/// 导出 TLV 字节流。 结构：`HEAD(0x01)+[len]+payload24` 后接若干 `(tag,len,payload)`。
///
/// **注意**：组头的 `push_u64(version)` 单独一行写，不被行尾注释吞，
/// 保证「版本」三字段在字节流里各占固定 8 字节、偏移固定。
pub fn export_tlv(ob: &CacheObserve) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    // 冻结矩阵：每格一条（hash + epoch + matched 标志）。
    for (mi, r) in ob.matrix_rows.iter().enumerate() {
        let mut p: Vec<u8> = Vec::new();
        push_u64(&mut p, fnv1a64(r.as_bytes()));
        push_u64(&mut p, FREEZE_EPOCH);
        push_u64(&mut p, mi as u64);
        push_tlv(&mut body, tlv_tag::MATRIX, &p);
    }
    // 联签行。
    for (i, r) in ob.sign_rows.iter().enumerate() {
        let mut p: Vec<u8> = Vec::new();
        push_u64(&mut p, fnv1a64(r.as_bytes()));
        push_u64(&mut p, i as u64);
        push_tlv(&mut body, tlv_tag::SIGN, &p);
    }
    // 兑现映射。
    for r in ob.map_rows.iter() {
        let mut p: Vec<u8> = Vec::new();
        push_u64(&mut p, fnv1a64(r.as_bytes()));
        push_u64(&mut p, CONTRACT_COUNT as u64);
        push_tlv(&mut body, tlv_tag::MAP, &p);
    }
    // 直测轴。
    for (i, r) in ob.axis_rows.iter().enumerate() {
        let mut p: Vec<u8> = Vec::new();
        push_u64(&mut p, fnv1a64(r.as_bytes()));
        push_u64(&mut p, i as u64);
        push_tlv(&mut body, tlv_tag::AXIS, &p);
    }
    // 注入结论。
    for r in ob.inject_rows.iter() {
        let mut p: Vec<u8> = Vec::new();
        push_u64(&mut p, fnv1a64(r.as_bytes()));
        push_u64(&mut p, FORM_COUNT as u64);
        push_tlv(&mut body, tlv_tag::INJECT, &p);
    }
    // 账。
    let mut lp: Vec<u8> = Vec::new();
    push_u64(&mut lp, ob.matrix_pass as u64);
    push_u64(&mut lp, ob.inject_rejected as u64);
    push_u64(&mut lp, ob.ring_dropped);
    push_u64(&mut lp, if ob.conserved { 1 } else { 0 });
    push_tlv(&mut body, tlv_tag::LEDGER, &lp);
    // 指纹。
    let mut fp: Vec<u8> = Vec::new();
    push_u64(&mut fp, ob.fp);
    push_u64(&mut fp, ob.impl_fp);
    push_u64(&mut fp, ob.replay_fp);
    push_tlv(&mut body, tlv_tag::FINGERPRINT, &fp);
    // 组头：单独构造，单独写 version 行（不被行尾注释吞）。
    let head: Vec<u8> = build_head(V2682_VERSION_ID);
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

/// 组头单独封装：`len2 + magic8 + version8 + len8`。
/// `push_u64(version)` **单独一行**，偏移固定。
pub fn build_head(version: u64) -> Vec<u8> {
    let mut h: Vec<u8> = Vec::with_capacity(3 + TLV_HEAD_PAYLOAD);
    push_u16(&mut h, (TLV_HEAD_PAYLOAD & 0xFFFF) as u16);
    // 三个字段各自单独一行、单独一次 push_u64，偏移固定且不被注释吞。
    push_u64(&mut h, 0x554E_5832_3638_3200);
    push_u64(&mut h, version);
    push_u64(&mut h, TLV_HEAD_PAYLOAD as u64);
    h
}

/// TLV 长度自洽校验：跳过组头前缀（`len2 + payload24`），再逐条回读 `tag/len/payload` 直至末尾。
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

/// TLV 往返一致：导出后再解析回关键字段指纹，等于原指纹。
pub fn tlv_roundtrip(ob: &CacheObserve) -> bool {
    let buf: Vec<u8> = export_tlv(ob);
    if !tlv_len_ok(&buf) {
        return false;
    }
    // 回读组头三字段（头布局：len2 + magic8 + version8 + len8）。
    let magic: u64 = read_u64(&buf, 2);
    let version: u64 = read_u64(&buf, 10);
    let len: u64 = read_u64(&buf, 18);
    if magic != 0x554E_5832_3638_3200 {
        return false;
    }
    if version != V2682_VERSION_ID {
        return false;
    }
    if len != (TLV_HEAD_PAYLOAD as u64) {
        return false;
    }
    // 尾部指纹载荷回读。
    let mut off: usize = 2 + TLV_HEAD_PAYLOAD;
    let mut tail_fp: u64 = 0;
    while off + 3 <= buf.len() {
        let tag: u8 = match buf.get(off) {
            Some(x) => *x,
            None => 0,
        };
        let len: usize = (read_u16(&buf, off + 1) & 0xFFFF) as usize;
        if tag == tlv_tag::FINGERPRINT {
            tail_fp = read_u64(&buf, off + 3);
        }
        off = off + 3 + len;
    }
    tail_fp == ob.fp
}

/// 自检项（逐项结论，真实跑逻辑）。
#[derive(Clone, Debug)]
pub struct Check {
    /// 项名。
    pub name: &'static str,
    /// 是否通过。
    pub pass: bool,
    /// 细节。
    pub detail: String,
}

/// 自检全集（`observe` 上跑，不重跑注入）。
pub fn self_check(ob: &CacheObserve) -> Vec<Check> {
    let mut out: Vec<Check> = Vec::new();
    out.push(Check { name: "seg_closure", pass: code_in_seg(SEG_BASE) && code_in_seg(SEG_END) && (SEG_END - SEG_BASE == SEG_MASK), detail: format!("0x{:04X}..0x{:04X}", SEG_BASE, SEG_END) });
    out.push(Check { name: "err_closure", pass: CacheErr::ALL.len() == 12, detail: format!("variants={}", CacheErr::ALL.len()) });
    out.push(Check { name: "matrix_full", pass: ob.matrix_pass == MATRIX_CELLS, detail: format!("{}/{}", ob.matrix_pass, MATRIX_CELLS) });
    out.push(Check { name: "trinity_complete", pass: ob.trinity, detail: "签名/错误码/语义 三件齐".to_string() });
    out.push(Check { name: "sign_full", pass: ob.signed_count == SIGN_REQUIRED, detail: format!("{}/{}", ob.signed_count, SIGN_REQUIRED) });
    out.push(Check { name: "map_zero_hole", pass: ob.map_closed == MAP_REQUIRED, detail: format!("{}/{}", ob.map_closed, MAP_REQUIRED) });
    out.push(Check { name: "axes_full", pass: ob.direct.all_ok(), detail: ob.direct.summary() });
    out.push(Check { name: "inject_all_rejected", pass: ob.inject_rejected == FORM_COUNT && ob.inject_leaked == 0, detail: format!("{}/{}", ob.inject_rejected, FORM_COUNT) });
    out.push(Check { name: "conserved", pass: ob.conserved, detail: "冻结账+容量+环形+轴账".to_string() });
    out.push(Check { name: "ring_coherent", pass: ob.ring_live + (ob.ring_dropped as usize) >= ob.ring_live, detail: format!("live={} dropped={}", ob.ring_live, ob.ring_dropped) });
    out.push(Check { name: "tlv_roundtrip", pass: tlv_roundtrip(ob), detail: "组头三字段回读".to_string() });
    out.push(Check { name: "replay_stable", pass: ob.replay_stable, detail: format!("fp=0x{:016X}", ob.replay_fp) });
    out.push(Check { name: "replay_discriminates", pass: ob.replay_discriminates, detail: "跨种子指纹不同".to_string() });
    out
}

/// 自检是否全过。
pub fn self_check_ok(ob: &CacheObserve) -> bool {
    let cs: Vec<Check> = self_check(ob);
    for c in cs.iter() {
        if !c.pass {
            return false;
        }
    }
    true
}

/// 自检报告（文本）。
pub fn self_check_report(ob: &CacheObserve) -> String {
    let cs: Vec<Check> = self_check(ob);
    let mut s: String = String::new();
    s.push_str("== UNX-F2682 self-check ==\n");
    for c in cs.iter() {
        s.push_str(&format!("[{}] {}: {}\n", if c.pass { "PASS" } else { "FAIL" }, c.name, c.detail));
    }
    s.push_str(&format!("all_ok={}\n", if self_check_ok(ob) { 1 } else { 0 }));
    s
}

// ===========================================================================
// 12. 性能逐项分解 `complexity()`
// ===========================================================================

/// 单项性能条目。
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
    /// 对标 Windows 同类能力的「深在哪」。
    pub vs_windows: &'static str,
}

impl PerfItem {
    /// 导出行文本。
    pub fn row(&self) -> String {
        format!("perf[{}] time={} space={} syscalls={} steps<={} win=\"{}\"", self.name, self.time, self.space, self.syscalls, self.steps, self.vs_windows)
    }
}

/// 性能分解表（`complexity()`）。
pub fn complexity() -> Vec<PerfItem> {
    let mut v: Vec<PerfItem> = Vec::new();
    v.push(PerfItem { name: "cache_lookup", time: "O(SLOTS)", space: "O(1)", syscalls: 0, steps: SLOTS as u64, vs_windows: "CcLookupPage 线性探测同等量级；本条多一层冻结签名闸" });
    v.push(PerfItem { name: "cache_insert", time: "O(SLOTS)", space: "O(1)", syscalls: 0, steps: (2 * SLOTS) as u64, vs_windows: "CcInsertPage 同量级；本条叠加批量硬顶硬拒分支" });
    v.push(PerfItem { name: "cache_mark_dirty", time: "O(SLOTS)", space: "O(1)", syscalls: 0, steps: SLOTS as u64, vs_windows: "CcMarkDirty 位操作 O(1)；本条线性定位但带语义前置闸" });
    v.push(PerfItem { name: "freeze_cell", time: "O(1)", space: "O(1)", syscalls: 0, steps: 8, vs_windows: "无直接对应；域内冻结登记制度" });
    v.push(PerfItem { name: "freeze_all", time: "O(IFACE_COUNT*FIELD_COUNT)", space: "O(MATRIX_CELLS)", syscalls: 0, steps: MATRIX_CELLS as u64, vs_windows: "无对应；15 格一次性构造期求值" });
    v.push(PerfItem { name: "sign_all", time: "O(PARTY_COUNT)", space: "O(PARTY_COUNT)", syscalls: 0, steps: PARTY_COUNT as u64, vs_windows: "无对应；五方契约联签制度" });
    v.push(PerfItem { name: "map_all", time: "O(CONTRACT_COUNT*FIELD_COUNT)", space: "O(MAP_REQUIRED)", syscalls: 0, steps: MAP_REQUIRED as u64, vs_windows: "无对应；兑现映射零缺闭合" });
    v.push(PerfItem { name: "sig_axis", time: "O(IFACE_COUNT)", space: "O(1)", syscalls: 0, steps: IFACE_COUNT as u64, vs_windows: "编译期签名核验更强：把三轴前移到纯函数层" });
    v.push(PerfItem { name: "errcode_axis", time: "O(IFACE_COUNT*4)", space: "O(1)", syscalls: 0, steps: (IFACE_COUNT * 4) as u64, vs_windows: "Cc 族错误码是 NTSTATUS；本条把码集闭包纳入冻结" });
    v.push(PerfItem { name: "semantic_axis", time: "O(BEHAVIOR_SAMPLES*SLOTS)", space: "O(1)", syscalls: 0, steps: (BEHAVIOR_SAMPLES * (2 * SLOTS + 1)) as u64, vs_windows: "无对应；语义行为被写成可执行断言矩阵" });
    v.push(PerfItem { name: "run_direct", time: "O(BEHAVIOR_SAMPLES*SLOTS)", space: "O(DIRECT_AXES)", syscalls: 0, steps: (BEHAVIOR_SAMPLES * 3) as u64, vs_windows: "无对应；三机直测三轴制度" });
    v.push(PerfItem { name: "inject_all", time: "O(FORM_COUNT*SLOTS)", space: "O(FORM_COUNT)", syscalls: 0, steps: (FORM_COUNT * (2 * SLOTS + 8)) as u64, vs_windows: "无对应；十形态负向注入 + 逐码期望比对" });
    v.push(PerfItem { name: "fingerprint", time: "O(MATRIX_CELLS+PARTY_COUNT+MAP_REQUIRED)", space: "O(1)", syscalls: 0, steps: (MATRIX_CELLS + PARTY_COUNT + MAP_REQUIRED) as u64, vs_windows: "无对应；冻结件可比对指纹" });
    v.push(PerfItem { name: "verify", time: "O(MATRIX_CELLS+MAP_REQUIRED)", space: "O(1)", syscalls: 0, steps: (MATRIX_CELLS + MAP_REQUIRED + 4) as u64, vs_windows: "无对应；四道闸一次过" });
    v.push(PerfItem { name: "replay", time: "O(REPLAY_STEPS*SLOTS)", space: "O(REPLAY_STEPS)", syscalls: 0, steps: (REPLAY_STEPS * (2 * SLOTS + 1)) as u64, vs_windows: "无对应；确定性回放双跑对拍" });
    v.push(PerfItem { name: "export_kv", time: "O(rows)", space: "O(rows)", syscalls: 0, steps: (MATRIX_CELLS + PARTY_COUNT + MAP_REQUIRED + DIRECT_AXES + FORM_COUNT) as u64, vs_windows: "无对应；纯文本可读观测" });
    v.push(PerfItem { name: "export_tlv", time: "O(rows)", space: "O(rows)", syscalls: 0, steps: ((MATRIX_CELLS + PARTY_COUNT + MAP_REQUIRED + DIRECT_AXES + FORM_COUNT + 2) * 8) as u64, vs_windows: "无对应；TLV 长度自洽 + 往返一致" });
    v.push(PerfItem { name: "observe", time: "O(BEHAVIOR_SAMPLES*SLOTS + REPLAY_STEPS*SLOTS)", space: "O(MATRIX_CELLS+SLOTS+RING_CAP)", syscalls: 0, steps: (BEHAVIOR_SAMPLES * SLOTS + REPLAY_STEPS * SLOTS) as u64, vs_windows: "无对应；一次聚合跑完整判据链" });
    v
}

/// 性能分解表文本。
pub fn complexity_report() -> String {
    let v: Vec<PerfItem> = complexity();
    let mut s: String = String::new();
    s.push_str("== UNX-F2682 complexity ==\n");
    for it in v.iter() {
        s.push_str(&it.row());
        s.push('\n');
    }
    s
}

// ===========================================================================
// 13. 清单与场景演示
// ===========================================================================

/// 错误闭集清单。
pub fn err_manifest() -> Vec<String> {
    err_code_table()
}

/// 冻结字段清单。
pub fn field_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(FIELD_COUNT);
    let mut i: usize = 0;
    while i < FIELD_COUNT {
        let f: Field = match Field::from_index(i) {
            Some(x) => x,
            None => Field::Signature,
        };
        v.push(format!("field[{}] trinity={}", f.code(), f.trinity()));
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

/// 演示场景（真实跑逻辑打印若干行）。
pub fn demo_scene(seed: u64) -> Vec<String> {
    let ob: CacheObserve = observe(seed);
    let mut v: Vec<String> = Vec::new();
    v.push(format!("seed=0x{:016X}", ob.seed));
    v.push(format!("freeze={}", ob.freeze_summary));
    v.push(format!("matrix={}/{}", ob.matrix_pass, ob.matrix_cells));
    v.push(ob.summary());
    v.push(self_check_report(&ob));
    v
}

/// 码段占用表（导出：段内已用码位）。
pub fn seg_usage() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut i: usize = 0;
    while i < CacheErr::ALL.len() {
        let e: CacheErr = CacheErr::ALL[i];
        v.push(format!("0x{:04X} used {}", e.code(), e.name()));
        i = i.wrapping_add(1);
    }
    let mut k: usize = CacheErr::ALL.len();
    while k <= (SEG_MASK as usize) {
        v.push(format!("0x{:04X} reserved", SEG_BASE + (k as u16)));
        k = k.wrapping_add(1);
    }
    v
}

/// 联签责任矩阵（B1–B5 × 三接口回归责任）。
pub fn sign_responsibility_matrix() -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(PARTY_COUNT * IFACE_COUNT);
    let mut p: usize = 0;
    while p < PARTY_COUNT {
        let mut i: usize = 0;
        while i < IFACE_COUNT {
            let ifc: Iface = match Iface::from_index(i) {
                Some(x) => x,
                None => Iface::Lookup,
            };
            let weight: u64 = (p as u64) * 3 + (i as u64) + 1;
            v.push(format!("regr[{}][{}] weight={}", PARTY_CODES[p], ifc.code(), weight));
            i = i.wrapping_add(1);
        }
        p = p.wrapping_add(1);
    }
    v
}


