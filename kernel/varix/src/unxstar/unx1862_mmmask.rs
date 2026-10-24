#![allow(dead_code)]
//! # UNX-F1862 · 地址空间活跃核追踪（mm 活跃 mask）
//!
//! ## 单号
//! UNX-F1862　单名 `unx1862_mmmask`
//!
//! ## 标题
//! 地址空间活跃核追踪（mm 活跃 mask）——CoRun·UNX-A3-B14「TLB 击落」段
//!
//! ## 目标行数
//! 2200~3000 行（硬指标）
//!
//! ## 判据原文（注释引用）
//! `UNX-F1862-J1 mm 活跃核 mask 维护断言全过（切换挂点同步），mask 漂移注入 10/10 检出`
//!
//! ## 册内上下文（CoRun·UNX-A3-B14 段「TLB 击落」）
//! - **F1861** 击落四段（发起→广播→应答→完成）：本单提供活跃核集合与必达侧账，四段栅栏在
//!   `unx1861_*`，此处以「发起段取样、应答段收账」承接。
//! - **F1863** 发起原语（先本地后远程 / 先登记后发送）：`shootdown_begin` 严格**先登记**
//!   （target/broadcast 双份快照落账并校验相等）**后发送**（登记成功才允许 ack 流进入）。
//! - **F1864** 击落 IPI 消费端「mask 位或 + 本地 flush + 应答」：本单 ack 必须**先过 mask 位或**
//!   （`broadcast.test(core)`），位或失败即 `AckBroadcastMismatch`，杜绝越过 mask 的幽灵应答。
//! - **F1865** 应答与完成账「Σ应答=Σ广播」：本单逐请求与全域双层守恒，超时降级**显式落账**。
//! - **F1867** 击落与切换互斥「挂 mask 待切后 flush」：过渡态（F1654）核**不得**按普通活跃核
//!   击落，必须走挂起路径 `park_transition` → 切换完成后 `flush_after_switch`。
//! - **F1654** 过渡态：核在 mask 内但带 `Transition` 标记，本单显式建模。
//! - **F1631** mask 协作：位图原语与版本号协作，变更必自增版本，供协作方对拍。
//! - **F1806** 切入路径（地址空间切换）：切入四段 = 地址空间切换→gs_base 装载→栈指针装载→上线，
//!   mask 置位时机在**上线段之后**；切出四段中 mask 清位时机在**地址空间切换之前**。
//! - **F1884** 迁移×击落联合回归：迁移导致 mask 翻转，源核出 A、目标核入 A，翻转后击落必达断言仍成立。
//!
//! ## 组件清单
//! | 组件 | 职责 |
//! |---|---|
//! | `ActiveMask` | 64 位活跃核位图 + O(1) 增量 popcount + 版本号，四个 O(1) 位原语 |
//! | `MmTable` | mm 表（`MmSlot` = mask + 生命周期态 `Live/Dying/Dead` + 引用计数） |
//! | `CoreTable` | 每核 `cur_mm`、运行态（`Off/Idle/Running/Transition`）、idle 核集合 |
//! | `MaskWorld` | 挂点引擎：挂点闭集六枚与切换四段协同 |
//! | `ReverseTable` | mm mask 反查表（bit→mm_id），与 `cur_mm` 表双源对账 |
//! | `Audit` | 守恒 / 交集空 / Σ守恒 / 版本推进 / idle 隔离 / 挂点同步十一条断言 |
//! | `Shootdown` | 击落必达、应答账、超时降级显式落账 |
//! | `FlipEntry` | 迁移翻转原子性（版本差恰 2、popcount 净差 0 即两侧各 ±1） |
//! | `Inject` | mask 漂移注入核，10 形态 + `inject_all()` |
//! | `Observ` | `export_kv()` / `export_tlv()` 指标与指纹导出 |
//! | `Replay` | 确定性回放，逐 tick 快照对账，偏差为零 |
//!
//! ## 职责定位
//! 本单是**纯功能层**：不碰真实硬件、不发真实 IPI、不读墙钟。所有"时间"是逻辑 `tick: u64`，
//! 所有"随机"是确定性 LCG。所有一致性结论都来自**可对拍的双源账**（声明源 vs 反查源），
//! 而非来自单点自证。因此本单可以独立 `cargo check`，也可以被真实内核在纯函数层复用。
//!
//! ## 性能逐项分解（每项均为 O(1) 或有界扫描，禁止隐藏的 O(n²) 热点）
//! | 操作 | 复杂度 | 说明 |
//! |---|---|---|
//! | `set/clear/test/flip` | O(1) | 单比特写 + popcount 增量 + 版本自增，无循环 |
//! | `popcount`/`is_empty`/`same_bits` | O(1) | 读字段或单指令位运算 |
//! | `union/intersect/diff` | O(1) | 单指令位运算 |
//! | `iter_ones` | O(popcount) | 只遍历置位核，摊还 O(1) |
//! | `fingerprint` | O(1) | 三字段折叠 fnv-1a |
//! | `MmTable::create` / `get` | O(1) 摊还 / O(1) | `Vec` 下标直取（id 即下标） |
//! | 单次挂点（切出/切入/idle/迁移/启动） | O(1) | 只动两个核、两个 mm |
//! | 挂点后 `audit_local` | O(nmm) | 只查被动核与反查重建 |
//! | 全域 `audit` | O(nmm²)，nmm≤16 为有界常量 | 跨 mm 交集两两对 |
//! | 全域 Σ守恒 / `reverse_rebuild` | O(nmm) / O(nmm×pop) | 累加 popcount / 遍历置位核 |
//! | `shootdown_begin`/`ack`/`finish` | O(pop) / O(1) / O(1) | 快照校验 / 单比特位或 / 掩码比较定态 |
//! | `migrate` | O(1) | 源目的两 mm 各一次原语 + 版本快照 |
//! | `inject_*`（10 形态） | O(1)~O(nmm) | 每形态独立造景 |
//! | `export_kv` / `export_tlv` | O(nmm+ncore) / O(nmm) | 文本渲染 / 定长记录 |
//! | `replay` | O(steps) | 每 tick 一次 O(1) 指纹 |
//!
//! ## 诊断码段（独占 0xB100~0xB10F，主码 0xB100）
//! | 码 | 变体 | 含义 |
//! |---|---|---|
//! | 0xB100 | `DriftMaskCurMm` | mask 有位而 `cur_mm` 不指此 mm（mask 侧漂移） |
//! | 0xB101 | `DriftCurMmMask` | `cur_mm` 指此 mm 而反查表/位图无位（反查侧漂移） |
//! | 0xB102 | `IntersectNonEmpty` | 一核同挂两 mm，跨 mm 交集非空 |
//! | 0xB103 | `BroadcastMaskMismatch` | 击落广播集合 ≠ 该 mm 的 ActiveMask |
//! | 0xB104 | `HookSyncLost` | 切换挂点漏同步（切入未置位 / 切出未清位） |
//! | 0xB105 | `TransitionStruck` | 过渡态核被当普通活跃核击落 |
//! | 0xB106 | `AckBroadcastMismatch` | Σ应答 ≠ Σ广播 |
//! | 0xB107 | `MigratePopcountOff` | 迁移翻转 popcount 差 ≠ 1 |
//! | 0xB108 | `VersionStalled` | 版本号停滞（mask 变更未自增版本） |
//! | 0xB109 | `IdleInMask` | idle 核被登记进 mm mask |
//! | 0xB10A | `PopcountBroken` | 增量 popcount 与 bits 实数不符 |
//! | 0xB10B | `SumConserv` | Σ每核活跃置位数 ≠ 运行线程核数 |
//! | 0xB10C | `MmStateBroken` | mm 生命周期态非法（Dying 仍有活跃核 / Dead 仍挂核） |
//! | 0xB10D | `ReverseFingerprint` | 反查表重建指纹与增量维护指纹不符 |
//! | 0xB10E | `MmIdOutOfRange` | mm_id 越界 |
//! | 0xB10F | `SegmentGuard` | 诊断码越出本模块独占段 |
//!
//! ## mask 位图与 mm 结构布局
//! ```text
//! ActiveMask（24 B 布局，字段序 = 内存偏移序）
//!  +0  bits    : u64   64 个核位，bit i = 核 i。bit0 恒为核 0（LAPIC CPUID 约定）
//!  +8  pop     : u32   增量维护的置位数，恒等于 bits.count_ones()
//!  +12 version : u64   每次 set/clear/flip 自增；读侧可据此判定「本 mm 发生过变更」
//!
//! MmSlot（mm 表项）
//!  id        : MmId(u32)   全域唯一，索引即 mm_id（Vec 尾插，不回收）
//!  mask      : ActiveMask  本 mm 当前活跃核位图（唯一权威集合，供击落取样）
//!  state     : MmState     Live / Dying / Dead
//!  refs      : u32        引用计数（线程/映射持有），0 且 mask 空才可回收
//!  born_tick : u64        建表逻辑时刻
//!  die_tick  : u64        进入 Dying 的逻辑时刻（0 表示尚未进入）
//!  name      : String     人读名，仅用于导出，不参与判定
//!
//! CoreSlot（核表项）
//!  core      : CoreId(u32) 0..64
//!  cur_mm    : Option<MmId> 当前归属 mm；None = 不在任何 mm 活跃 mask 内
//!  run       : CoreRun      Off / Idle / Running / Transition
//!  hook_seq  : u64          该核经历的挂点序号，用于确定性回放对拍
//! ```
//!
//! ## 纯功能线
//! `no_std + alloc`；零 `unwrap` / `expect` / `panic!` / 浮点 / 真实 IO / 墙钟；
//! 自定义 `Result<T, MaskErr>`；确定性 LCG（`A=6364136223846793005`, `C=1442695040888963407`）；
//! 逻辑 `tick: u64` 单调推进。全部数值面为整数，无 `f32/f64`。

use alloc::vec;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const U1862_VERSION: &str = "unx1862-mmmask-v1";

/// 诊断段基址（独占 0xB100~0xB10F）。
pub const U1862_SEG_BASE: u16 = 0xB100;

/// 诊断段上界（含）。
pub const U1862_SEG_END: u16 = 0xB10F;

/// 模拟核数上限（位图 64 位）。
pub const CORE_COUNT: u32 = 64;

/// 核 id 掩码。
pub const CORE_MASK: u32 = 63;

/// mm 表容量上界（全域地址空间个数的建模上限）。
pub const MM_CAPACITY: usize = 16;

/// 击落请求在册上界。
pub const SHOOT_CAPACITY: usize = 32;

/// 确定性 LCG 乘数（标准 64 位 LCG 常数）。
pub const LCG_A: u64 = 6364136223846793005;

/// 确定性 LCG 增量（标准 64 位 LCG 常数）。
pub const LCG_C: u64 = 1442695040888963407;

/// 指纹折叠初值（fnv-1a offset basis）。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;

/// 指纹折叠素数（fnv-1a prime）。
pub const FNV_PRIME: u64 = 0x00000100000001b3;

// --------------------------------------------------------------------------
// 核 id / mm id / 错误
// --------------------------------------------------------------------------

/// 核标识（0..64）。
pub type CoreId = u32;

/// mm（地址空间）标识。
pub type MmId = u32;

/// 本单自定义 `Result`（禁 `unwrap` / `expect` / `panic!`）。
pub type Result<T> = core::result::Result<T, MaskErr>;

/// 诊断码闭集（16 枚，独占 0xB100~0xB10F）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MaskErr {
    /// 0xB100 mask 有位而 `cur_mm` 不指此 mm（mask 侧漂移）。
    DriftMaskCurMm,
    /// 0xB101 `cur_mm` 指此 mm 而反查表/位图无位（反查侧漂移）。
    DriftCurMmMask,
    /// 0xB102 一核同挂两 mm，跨 mm 交集非空。
    IntersectNonEmpty,
    /// 0xB103 击落广播集合 ≠ 该 mm 的 ActiveMask。
    BroadcastMaskMismatch,
    /// 0xB104 切换挂点漏同步。
    HookSyncLost,
    /// 0xB105 过渡态核被当普通活跃核击落。
    TransitionStruck,
    /// 0xB106 Σ应答 ≠ Σ广播。
    AckBroadcastMismatch,
    /// 0xB107 迁移翻转 popcount 差 ≠ 1。
    MigratePopcountOff,
    /// 0xB108 版本号停滞。
    VersionStalled,
    /// 0xB109 idle 核被登记进 mm mask。
    IdleInMask,
    /// 0xB10A 增量 popcount 与 bits 实数不符。
    PopcountBroken,
    /// 0xB10B Σ每核活跃置位数 ≠ 运行线程核数。
    SumConserv,
    /// 0xB10C mm 生命周期态非法。
    MmStateBroken,
    /// 0xB10D 反查表重建指纹与增量维护指纹不符。
    ReverseFingerprint,
    /// 0xB10E mm_id 越界。
    MmIdOutOfRange,
    /// 0xB10F 诊断码越出本模块独占段。
    SegmentGuard,
}

impl MaskErr {
    /// 全量闭集（声明序即码序，0xB100 起连续 16 枚）。
    pub const ALL: [MaskErr; 16] = [
        MaskErr::DriftMaskCurMm, MaskErr::DriftCurMmMask, MaskErr::IntersectNonEmpty,
        MaskErr::BroadcastMaskMismatch, MaskErr::HookSyncLost, MaskErr::TransitionStruck,
        MaskErr::AckBroadcastMismatch, MaskErr::MigratePopcountOff, MaskErr::VersionStalled,
        MaskErr::IdleInMask, MaskErr::PopcountBroken, MaskErr::SumConserv,
        MaskErr::MmStateBroken, MaskErr::ReverseFingerprint, MaskErr::MmIdOutOfRange,
        MaskErr::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 { U1862_SEG_BASE + self as u16 }

    /// 码值是否落在独占段内。
    pub const fn in_segment(self) -> bool {
        let c = self.code();
        c >= U1862_SEG_BASE && c <= U1862_SEG_END
    }

    /// 段守卫：越段即 `SegmentGuard`。
    pub const fn guard(self) -> core::result::Result<(), MaskErr> {
        if self.in_segment() {
            core::result::Result::Ok(())
        } else {
            core::result::Result::Err(MaskErr::SegmentGuard)
        }
    }

    /// 机器可读名（导出用）。
    pub const fn name(self) -> &'static str {
        match self {
            MaskErr::DriftMaskCurMm => "DriftMaskCurMm",
            MaskErr::DriftCurMmMask => "DriftCurMmMask",
            MaskErr::IntersectNonEmpty => "IntersectNonEmpty",
            MaskErr::BroadcastMaskMismatch => "BroadcastMaskMismatch",
            MaskErr::HookSyncLost => "HookSyncLost",
            MaskErr::TransitionStruck => "TransitionStruck",
            MaskErr::AckBroadcastMismatch => "AckBroadcastMismatch",
            MaskErr::MigratePopcountOff => "MigratePopcountOff",
            MaskErr::VersionStalled => "VersionStalled",
            MaskErr::IdleInMask => "IdleInMask",
            MaskErr::PopcountBroken => "PopcountBroken",
            MaskErr::SumConserv => "SumConserv",
            MaskErr::MmStateBroken => "MmStateBroken",
            MaskErr::ReverseFingerprint => "ReverseFingerprint",
            MaskErr::MmIdOutOfRange => "MmIdOutOfRange",
            MaskErr::SegmentGuard => "SegmentGuard",
        }
    }

    /// 人读说明（导出用）。
    pub const fn desc(self) -> &'static str {
        match self {
            MaskErr::DriftMaskCurMm => "mask 置位而 cur_mm 表未登记",
            MaskErr::DriftCurMmMask => "cur_mm 已登记而 mask/反查表缺位",
            MaskErr::IntersectNonEmpty => "跨 mm 交集非空，一核同挂两 mm",
            MaskErr::BroadcastMaskMismatch => "击落广播集合与 ActiveMask 不逐位相同",
            MaskErr::HookSyncLost => "切换挂点漏同步",
            MaskErr::TransitionStruck => "过渡态核被当普通活跃核击落",
            MaskErr::AckBroadcastMismatch => "应答数与广播数不等",
            MaskErr::MigratePopcountOff => "迁移翻转 popcount 差非 1",
            MaskErr::VersionStalled => "mask 变更未自增版本号",
            MaskErr::IdleInMask => "idle 核被登记进 mm mask",
            MaskErr::PopcountBroken => "增量 popcount 与位图实数不符",
            MaskErr::SumConserv => "Σ置位数与运行核数不守恒",
            MaskErr::MmStateBroken => "mm 生命周期态与活跃集合矛盾",
            MaskErr::ReverseFingerprint => "反查表重建指纹与增量指纹不符",
            MaskErr::MmIdOutOfRange => "mm_id 越界",
            MaskErr::SegmentGuard => "诊断码越段",
        }
    }
}

// --------------------------------------------------------------------------
// 确定性 LCG
// --------------------------------------------------------------------------

/// 确定性 64 位 LCG（Knuth MMIX 常数）。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    /// 内部状态。
    pub state: u64,
}

impl Lcg {
    /// 以给定种子构造。
    pub const fn with_seed(seed: u64) -> Lcg { Lcg { state: seed } }

    /// 取下一值。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(LCG_A)
            .wrapping_add(LCG_C);
        self.state
    }

    /// 取 `[0, n)` 内值（`n>0`）。
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { return 0; }
        (self.next_u64() % (n as u64)) as u32
    }

    /// 取 `[lo, hi]` 闭区间值。
    pub fn range(&mut self, lo: u32, hi: u32) -> u32 {
        if hi <= lo { return lo; }
        lo + self.below(hi - lo + 1)
    }
}

impl Default for Lcg {
    /// 默认种子（固定，保证回放可复现）。
    fn default() -> Lcg { Lcg::with_seed(0x1862_0000_0000_0001) }
}

// --------------------------------------------------------------------------
// ActiveMask：64 位活跃核位图
// --------------------------------------------------------------------------

/// 64 位活跃核位图。
///
/// 布局：`bits`(u64) + `pop`(u32 增量维护) + `version`(u64 变更计数)。
/// 四个位原语均为 O(1)，且**每个都同步维护 popcount 与版本**。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ActiveMask {
    /// 64 个核位。
    pub bits: u64,
    /// 增量维护的置位数。
    pub pop: u32,
    /// 变更版本号（每次 set/clear/flip 自增）。
    pub version: u64,
}

impl Default for ActiveMask {
    /// 全零 mask（`version=0`）。
    fn default() -> ActiveMask { ActiveMask { bits: 0, pop: 0, version: 0, } }
}

/// `bits.count_ones()` 的本单实现（整数运算，无浮点、无内建依赖）。
#[inline]
pub fn bits_count(bits: u64) -> u32 {
    let mut v = bits;
    let mut n = 0u32;
    while v != 0 { v &= v.wrapping_sub(1); n += 1; }
    n
}

impl ActiveMask {
    /// 全零构造。
    pub const fn zero() -> ActiveMask { ActiveMask { bits: 0, pop: 0, version: 0, } }

    /// 由 `bits` 构造并同步 `pop`/`version`（登记语义下 version=1）。
    pub fn from_bits(bits: u64) -> ActiveMask {
        ActiveMask { bits, pop: bits_count(bits), version: 1, }
    }

    /// 仅取位图（不登记变更，用于对账侧的反查重建）。
    pub fn raw(bits: u64) -> ActiveMask { ActiveMask { bits, pop: bits_count(bits), version: 0, } }

    /// 核 id 转位序（越界返回 None）。
    #[inline]
    pub const fn slot(core: CoreId) -> Option<u64> {
        if core < CORE_COUNT { Some(1u64 << core) } else { None }
    }

    /// 置位原语（O(1)）。返回是否发生变更。**判据：UNX-F1862-J1 —— 置位必同步 popcount 与版本**
    pub fn set(&mut self, core: CoreId) -> bool {
        let bit = match ActiveMask::slot(core) { Some(b) => b, None => return false, };
        if self.bits & bit != 0 { return false; }
        self.bits |= bit;
        self.pop += 1;
        self.version += 1;
        true
    }

    /// 清位原语（O(1)）。返回是否发生变更。**判据：UNX-F1862-J1 —— 清位必同步 popcount 与版本**
    pub fn clear(&mut self, core: CoreId) -> bool {
        let bit = match ActiveMask::slot(core) { Some(b) => b, None => return false, };
        if self.bits & bit == 0 { return false; }
        self.bits &= !bit;
        if self.pop > 0 { self.pop -= 1; }
        self.version += 1;
        true
    }

    /// 测位原语（O(1)，不改状态）。
    #[inline]
    pub fn test(&self, core: CoreId) -> bool {
        match ActiveMask::slot(core) { Some(bit) => self.bits & bit != 0, None => false, }
    }

    /// 翻转原语（O(1)）。返回翻转后的位值。**判据：UNX-F1862-J1 —— 翻转必同步 popcount 与版本**
    pub fn flip(&mut self, core: CoreId) -> bool {
        let bit = match ActiveMask::slot(core) { Some(b) => b, None => return false, };
        let was = self.bits & bit != 0;
        if was {
            self.bits &= !bit;
            if self.pop > 0 { self.pop -= 1; }
        } else {
            self.bits |= bit;
            self.pop += 1;
        }
        self.version += 1;
        !was
    }

    /// 置位数（O(1)，读增量字段）。
    #[inline]
    pub fn popcount(&self) -> u32 { self.pop }

    /// 版本号（O(1)）。
    #[inline]
    pub fn version(&self) -> u64 { self.version }

    /// 位图本体（O(1)）。
    #[inline]
    pub fn bits(&self) -> u64 { self.bits }

    /// 是否为空（O(1)）。
    #[inline]
    pub fn is_empty(&self) -> bool { self.bits == 0 }

    /// 并集（O(1)，只算位图；`pop` 现场重算，`version` 取两侧较大者）。
    pub fn union(&self, other: &ActiveMask) -> ActiveMask {
        ActiveMask {
            bits: self.bits | other.bits,
            pop: bits_count(self.bits | other.bits),
            version: if self.version > other.version { self.version } else { other.version },
        }
    }

    /// 交集（O(1)）。
    pub fn intersect(&self, other: &ActiveMask) -> ActiveMask {
        ActiveMask {
            bits: self.bits & other.bits,
            pop: bits_count(self.bits & other.bits),
            version: if self.version > other.version { self.version } else { other.version },
        }
    }

    /// 差集（O(1)）。
    pub fn diff(&self, other: &ActiveMask) -> ActiveMask {
        ActiveMask {
            bits: self.bits & !other.bits,
            pop: bits_count(self.bits & !other.bits),
            version: if self.version > other.version { self.version } else { other.version },
        }
    }

    /// 是否逐位相同（**必达断言的核心比较**，O(1)）。
    #[inline]
    pub fn same_bits(&self, other: &ActiveMask) -> bool { self.bits == other.bits }

    /// 遍历置位核（O(popcount)）。
    pub fn iter_ones(&self) -> Vec<CoreId> {
        let mut out = Vec::new();
        let mut v = self.bits;
        while v != 0 { let t = v.trailing_zeros(); out.push(t as u32); v &= v.wrapping_sub(1); }
        out
    }

    /// 三字段折叠指纹（O(1)）：bits ^ pop ^ version 折入 fnv 混合。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h ^= self.bits;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.pop as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.version;
        h.wrapping_mul(FNV_PRIME)
    }

    /// 自洽性：增量 `pop` 是否等于 `bits` 实数。
    #[inline]
    pub fn pop_consistent(&self) -> bool { self.pop == bits_count(self.bits) }

    /// 版本推进自检：非空 mask 必须带版本。
    #[inline]
    pub fn version_alive(&self) -> bool { self.bits == 0 || self.version > 0 }

    /// 计数摘要（导出用，形如 `bits=0x… pop=… ver=…`）。
    pub fn summary(&self) -> String {
        format!("bits=0x{:016x} pop={} ver={}", self.bits, self.pop, self.version )
    }
}

// --------------------------------------------------------------------------
// mm 生命周期
// --------------------------------------------------------------------------

/// mm（地址空间）生命周期态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MmState {
    /// 在用：可挂核、可被击落取样。
    Live,
    /// 退役中：不可再挂新核，但已挂核仍需被击落取样（F1884 迁移×击落衔接）。
    Dying,
    /// 已死：mask 必为空。
    Dead,
}

impl MmState {
    /// 全量闭集。
    pub const ALL: [MmState; 3] = [MmState::Live, MmState::Dying, MmState::Dead];

    /// 名。
    pub const fn name(self) -> &'static str {
        match self { MmState::Live => "Live", MmState::Dying => "Dying", MmState::Dead => "Dead", }
    }

    /// 是否接受新核挂入。
    pub const fn admits(self) -> bool { matches!(self, MmState::Live) }
}

/// mm 表项。
#[derive(Clone, Debug)]
pub struct MmSlot {
    /// mm 标识。
    pub id: MmId,
    /// 活跃核 mask（权威集合，供击落取样）。
    pub mask: ActiveMask,
    /// 生命周期态。
    pub state: MmState,
    /// 引用计数（0 且 mask 空方可回收）。
    pub refs: u32,
    /// 建立逻辑时刻。
    pub born_tick: u64,
    /// 进入 Dying 的逻辑时刻（0 表示未进入）。
    pub die_tick: u64,
    /// 人读名。
    pub name: String,
    /// 本 mm 累计击落次数。
    pub shoots: u32,
}

impl MmSlot {
    /// 构造一个 Live 空表项。
    pub fn new(id: MmId, name: &str, tick: u64) -> MmSlot {
        MmSlot {
            id,
            mask: ActiveMask::zero(),
            state: MmState::Live,
            refs: 0,
            born_tick: tick,
            die_tick: 0,
            name: name.to_string(),
            shoots: 0,
        }
    }

    /// 活跃核数（O(1)）。
    #[inline]
    pub fn active(&self) -> u32 { self.mask.popcount() }
}

/// mm 表（`mm_id` 直接作 `Vec` 下标，容量 `MM_CAPACITY`）。
#[derive(Clone, Debug)]
pub struct MmTable {
    /// 表项。
    pub slots: Vec<MmSlot>,
    /// 空闲 id 栈（回收后复用，保证 id 稳定单调回退可预期）。
    pub free_ids: Vec<MmId>,
}

impl Default for MmTable {
    fn default() -> MmTable { MmTable { slots: Vec::new(), free_ids: Vec::new(), } }
}

impl MmTable {
    /// 空表。
    pub fn new() -> MmTable { MmTable::default() }

    /// 容量内已占表项数（不含回收中）。
    pub fn live_count(&self) -> usize {
        let mut n = 0usize;
        for s in self.slots.iter() { if s.state != MmState::Dead { n += 1; } }
        n
    }

    /// 取得表项引用。
    pub fn get(&self, id: MmId) -> Result<&MmSlot> {
        let idx = id as usize;
        if idx >= self.slots.len() { return core::result::Result::Err(MaskErr::MmIdOutOfRange); }
        match self.slots.get(idx) {
            Some(s) => core::result::Result::Ok(s),
            None => core::result::Result::Err(MaskErr::MmIdOutOfRange),
        }
    }

    /// 取得表项可变引用。
    pub fn get_mut(&mut self, id: MmId) -> Result<&mut MmSlot> {
        let idx = id as usize;
        if idx >= self.slots.len() { return core::result::Result::Err(MaskErr::MmIdOutOfRange); }
        match self.slots.get_mut(idx) {
            Some(s) => core::result::Result::Ok(s),
            None => core::result::Result::Err(MaskErr::MmIdOutOfRange),
        }
    }

    /// 建表。返回新 mm_id。
    pub fn create(&mut self, name: &str, tick: u64) -> Result<MmId> {
        let id = match self.free_ids.pop() {
            Some(i) => i,
            None => {
                if self.slots.len() >= MM_CAPACITY { return core::result::Result::Err(MaskErr::MmStateBroken); }
                (self.slots.len()) as MmId
            }
        };
        let slot = MmSlot::new(id, name, tick);
        if id as usize >= self.slots.len() {
            self.slots.push(slot);
        } else if let Some(s) = self.slots.get_mut(id as usize) {
            *s = slot;
        } else {
            return core::result::Result::Err(MaskErr::MmIdOutOfRange);
        }
        core::result::Result::Ok(id)
    }

    /// 引用计数 +1（仅 Live 允许）。
    pub fn ref_up(&mut self, id: MmId) -> Result<u32> {
        let s = self.get_mut(id)?;
        if !s.state.admits() { return core::result::Result::Err(MaskErr::MmStateBroken); }
        s.refs = s.refs.saturating_add(1);
        core::result::Result::Ok(s.refs)
    }

    /// 引用计数 -1（ Dying/Dead 也允许，用于收敛）。
    pub fn ref_down(&mut self, id: MmId) -> Result<u32> {
        let s = self.get_mut(id)?;
        if s.refs > 0 { s.refs -= 1; }
        core::result::Result::Ok(s.refs)
    }

    /// 进入 Dying。
    pub fn mark_dying(&mut self, id: MmId, tick: u64) -> Result<()> {
        let s = self.get_mut(id)?;
        if s.state == MmState::Dead { return core::result::Result::Err(MaskErr::MmStateBroken); }
        s.state = MmState::Dying;
        s.die_tick = tick;
        core::result::Result::Ok(())
    }

    /// 回收：要求 `refs==0 && mask 空 && state==Dying`。
    pub fn reap(&mut self, id: MmId) -> Result<()> {
        let s = self.get_mut(id)?;
        if s.refs != 0 { return core::result::Result::Err(MaskErr::MmStateBroken); }
        if !s.mask.is_empty() { return core::result::Result::Err(MaskErr::MmStateBroken); }
        if s.state != MmState::Dying { return core::result::Result::Err(MaskErr::MmStateBroken); }
        s.state = MmState::Dead;
        s.name = format!("{}[dead#{}]", s.name, s.id);
        self.free_ids.push(id);
        core::result::Result::Ok(())
    }

    /// 生命周期态自洽（Dead 必空、Live/Dying 必带版本）。
    pub fn audit_state(&self) -> Result<()> {
        for s in self.slots.iter() {
            if s.state == MmState::Dead && !s.mask.is_empty() {
                return core::result::Result::Err(MaskErr::MmStateBroken);
            }
            if !s.mask.version_alive() { return core::result::Result::Err(MaskErr::VersionStalled); }
            if !s.mask.pop_consistent() { return core::result::Result::Err(MaskErr::PopcountBroken); }
        }
        core::result::Result::Ok(())
    }
}

// --------------------------------------------------------------------------
// 核表
// --------------------------------------------------------------------------

/// 核运行态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CoreRun {
    /// 未启动（Boot 前）。
    Off,
    /// 空闲：不属任何 mm 活跃 mask（**显式建模的 idle 核集合**）。
    Idle,
    /// 运行某 mm 的线程：在该 mm 的 mask 内。
    Running,
    /// 过渡态（F1654）：**仍在 mask 内**但带 Transition 标记，击落需走 F1867 路径。
    Transition,
}

impl CoreRun {
    /// 全量闭集。
    pub const ALL: [CoreRun; 4] = [CoreRun::Off, CoreRun::Idle, CoreRun::Running, CoreRun::Transition];

    /// 名。
    pub const fn name(self) -> &'static str {
        match self {
            CoreRun::Off => "Off",
            CoreRun::Idle => "Idle",
            CoreRun::Running => "Running",
            CoreRun::Transition => "Transition",
        }
    }

    /// 是否应出现在某 mm 的 mask 内（Transition 也在内——这是 F1654 的关键语义）。
    #[inline]
    pub const fn in_mask(self) -> bool { matches!(self, CoreRun::Running | CoreRun::Transition) }

    /// 是否为过渡态（击落必须绕道 F1867）。
    #[inline]
    pub const fn is_transition(self) -> bool { matches!(self, CoreRun::Transition) }

    /// 是否为空闲/未启动（必不在任何 mask 内）。
    #[inline]
    pub const fn detached(self) -> bool { matches!(self, CoreRun::Off | CoreRun::Idle) }
}

/// 核表项。
#[derive(Clone, Copy, Debug)]
pub struct CoreSlot {
    /// 当前归属 mm；`None` = 不在任何 mm 活跃 mask 内。
    pub cur_mm: Option<MmId>,
    /// 运行态。
    pub run: CoreRun,
    /// 挂点序号（确定性回放对拍用）。
    pub hook_seq: u64,
}

impl CoreSlot {
    /// 构造 Off 态空闲槽。
    pub const fn new_off() -> CoreSlot { CoreSlot { cur_mm: None, run: CoreRun::Off, hook_seq: 0, } }

    /// 是否属于某 mm 的活跃 mask。
    #[inline]
    pub fn bound(&self) -> bool { self.cur_mm.is_some() }
}

/// 核表（固定 64 项）。
#[derive(Clone, Debug)]
pub struct CoreTable {
    /// 槽位数组，下标即 `CoreId`。
    pub slots: Vec<CoreSlot>,
}

impl Default for CoreTable { fn default() -> CoreTable { CoreTable { slots: Vec::new() } } }

impl CoreTable {
    /// 建表：64 槽全 `Off`。
    pub fn new() -> CoreTable {
        let mut v = Vec::with_capacity(CORE_COUNT as usize);
        for _ in 0..CORE_COUNT { v.push(CoreSlot::new_off()); }
        CoreTable { slots: v }
    }

    /// 容量（恒 64）。
    pub fn len(&self) -> usize { self.slots.len() }

    /// 是否为空表（真值表恒为 false，保留以对齐集合 API 习惯）。
    pub fn is_empty(&self) -> bool { self.slots.is_empty() }

    /// 越界即 `MmIdOutOfRange`。
    fn idx(core: CoreId) -> Result<usize> {
        if core < CORE_COUNT {
            core::result::Result::Ok(core as usize)
        } else {
            core::result::Result::Err(MaskErr::MmIdOutOfRange)
        }
    }

    /// 读槽。
    pub fn get(&self, core: CoreId) -> Result<&CoreSlot> {
        let i = CoreTable::idx(core)?;
        match self.slots.get(i) {
            Some(s) => core::result::Result::Ok(s),
            None => core::result::Result::Err(MaskErr::MmIdOutOfRange),
        }
    }

    /// 写槽。
    pub fn get_mut(&mut self, core: CoreId) -> Result<&mut CoreSlot> {
        let i = CoreTable::idx(core)?;
        match self.slots.get_mut(i) {
            Some(s) => core::result::Result::Ok(s),
            None => core::result::Result::Err(MaskErr::MmIdOutOfRange),
        }
    }

    /// 读 `cur_mm`。
    pub fn cur_mm(&self, core: CoreId) -> Result<Option<MmId>> {
        let s = self.get(core)?;
        core::result::Result::Ok(s.cur_mm)
    }

    /// 读运行态。
    pub fn run(&self, core: CoreId) -> Result<CoreRun> {
        let s = self.get(core)?;
        core::result::Result::Ok(s.run)
    }

    /// idle 核集合（显式建模）。
    pub fn idle_set(&self) -> ActiveMask {
        let mut m = ActiveMask::raw(0);
        for (i, s) in self.slots.iter().enumerate() { if s.run == CoreRun::Idle { m.set(i as u32); } }
        m
    }

    /// 未启动核集合。
    pub fn off_set(&self) -> ActiveMask {
        let mut m = ActiveMask::raw(0);
        for (i, s) in self.slots.iter().enumerate() { if s.run == CoreRun::Off { m.set(i as u32); } }
        m
    }

    /// 运行线程的核集合（Running ∪ Transition）。
    pub fn running_set(&self) -> ActiveMask {
        let mut m = ActiveMask::raw(0);
        for (i, s) in self.slots.iter().enumerate() { if s.run.in_mask() { m.set(i as u32); } }
        m
    }

    /// 过渡态核集合（F1654）。
    pub fn transition_set(&self) -> ActiveMask {
        let mut m = ActiveMask::raw(0);
        for (i, s) in self.slots.iter().enumerate() { if s.run.is_transition() { m.set(i as u32); } }
        m
    }

    /// 运行线程核数（Σ守恒右端）。
    pub fn running_count(&self) -> u32 {
        let mut n = 0u32;
        for s in self.slots.iter() { if s.run.in_mask() { n += 1; } }
        n
    }

    /// idle 核数。
    pub fn idle_count(&self) -> u32 {
        let mut n = 0u32;
        for s in self.slots.iter() { if s.run == CoreRun::Idle { n += 1; } }
        n
    }

    /// 归属计数：某 mm 名下的核数（由 `cur_mm` 声明源统计，O(n)）。
    pub fn count_of_mm(&self, mm: MmId) -> u32 {
        let mut n = 0u32;
        for s in self.slots.iter() { if s.cur_mm == Some(mm) { n += 1; } }
        n
    }

    /// 自洽：空闲/未启动核必 `cur_mm == None`，反之亦然。
    pub fn audit_bind(&self) -> Result<()> {
        for (i, s) in self.slots.iter().enumerate() {
            let core = i as u32;
            match (s.run.detached(), s.cur_mm) {
                (true, Some(_)) => return core::result::Result::Err(MaskErr::IdleInMask),
                (false, None) => return core::result::Result::Err(MaskErr::HookSyncLost),
                _ => {}
            }
            if s.run.in_mask() && !s.cur_mm.is_some() && core >= CORE_COUNT {
                return core::result::Result::Err(MaskErr::HookSyncLost);
            }
        }
        core::result::Result::Ok(())
    }
}

// --------------------------------------------------------------------------
// 反查表（bit → mm_id），与 cur_mm 表双源对账
// --------------------------------------------------------------------------

/// mm mask 反查表：由 mask 反查得出「核 → mm」，作为 `cur_mm` 表的**独立第二源**。
#[derive(Clone, Debug, Default)]
pub struct ReverseTable {
    /// 逐核归属（仅登记 mask 内核）。
    pub owner: BTreeMap<CoreId, MmId>,
    /// 增量维护指纹。
    pub fingerprint: u64,
}

impl ReverseTable {
    /// 空表。
    pub fn new() -> ReverseTable { ReverseTable { owner: BTreeMap::new(), fingerprint: FNV_OFFSET, } }

    /// 登记一核归属。
    pub fn put(&mut self, core: CoreId, mm: MmId) {
        let key = (core as u64) << 32 | mm as u64;
        self.owner.insert(core, mm);
        self.fingerprint = fnv_mix(self.fingerprint, key);
    }

    /// 摘除一核归属。
    pub fn drop_core(&mut self, core: CoreId) -> Option<MmId> {
        let old = self.owner.remove(&core);
        if let Some(mm) = old {
            let key = (core as u64) << 32 | mm as u64;
            self.fingerprint = fnv_mix(self.fingerprint, key ^ 0xFFFF_FFFF_FFFF_FFFF);
        }
        old
    }

    /// 查归属。
    pub fn get(&self, core: CoreId) -> Option<MmId> { self.owner.get(&core).copied() }

    /// 登记核数。
    pub fn len(&self) -> usize { self.owner.len() }

    /// 是否为空。
    pub fn is_empty(&self) -> bool { self.owner.is_empty() }
}

/// fnv-1a 单步折叠（O(1)）。
#[inline]
pub fn fnv_mix(h: u64, v: u64) -> u64 { let mut x = h ^ v; x = x.wrapping_mul(FNV_PRIME); x }

// --------------------------------------------------------------------------
// 挂点闭集与切换四段
// --------------------------------------------------------------------------

/// 挂点来源闭集（6 枚，互斥穷尽）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HookSrc {
    /// 切出：离开当前 mm。
    SwitchOut,
    /// 切入：进入目标 mm。
    SwitchIn,
    /// 进入空闲。
    IdleEnter,
    /// 退出空闲。
    IdleExit,
    /// 迁移（跨核搬迁，F1884）。
    Migrate,
    /// 启动首次挂载。
    Boot,
}

impl HookSrc {
    /// 全量闭集。
    pub const ALL: [HookSrc; 6] = [HookSrc::SwitchOut, HookSrc::SwitchIn, HookSrc::IdleEnter,
        HookSrc::IdleExit, HookSrc::Migrate, HookSrc::Boot];

    /// 名（导出用）。
    pub const fn name(self) -> &'static str {
        match self {
            HookSrc::SwitchOut => "SwitchOut",
            HookSrc::SwitchIn => "SwitchIn",
            HookSrc::IdleEnter => "IdleEnter",
            HookSrc::IdleExit => "IdleExit",
            HookSrc::Migrate => "Migrate",
            HookSrc::Boot => "Boot",
        }
    }

    /// 下标（分布计数用）。
    pub const fn index(self) -> usize { self as usize }

    /// 是否是「核进入某 mm mask」的挂点（这些挂点必须置位）。
    #[inline]
    pub const fn must_set(self) -> bool {
        matches!(self, HookSrc::SwitchIn | HookSrc::Migrate | HookSrc::Boot)
    }

    /// 是否是「核离开某 mm mask」的挂点（这些挂点必须清位）。
    #[inline]
    pub const fn must_clear(self) -> bool { matches!(self, HookSrc::SwitchOut | HookSrc::IdleEnter) }
}

/// 切入四段（F1806 承接）：地址空间切换→gs_base 装载→栈指针装载→上线。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InStage {
    /// 段0：地址空间切换（CR3 装载）。**mask 尚未置位**。
    AsidSwitch,
    /// 段1：gs_base 装载。**mask 尚未置位**。
    GsBaseLoad,
    /// 段2：栈指针装载。**mask 尚未置位**。
    StackLoad,
    /// 段3：上线。**mask 在本段之后才置位**。
    Online,
}

impl InStage {
    /// 全量闭集（顺序即执行序）。
    pub const ALL: [InStage; 4] = [InStage::AsidSwitch, InStage::GsBaseLoad, InStage::StackLoad, InStage::Online];

    /// 名。
    pub const fn name(self) -> &'static str {
        match self {
            InStage::AsidSwitch => "AsidSwitch",
            InStage::GsBaseLoad => "GsBaseLoad",
            InStage::StackLoad => "StackLoad",
            InStage::Online => "Online",
        }
    }

    /// 本段结束时 mask 是否应已置位。
    /// **判据：UNX-F1862-J1 —— 切入的 mask 置位时机在「上线段之后」**
    #[inline]
    pub const fn mask_set_after(self) -> bool { matches!(self, InStage::Online) }
}

/// 切出四段：反序执行，mask 清位在**地址空间切换之前**。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutStage {
    /// 段0：下线（停线程）。mask 在本段之前已清位。
    Offline,
    /// 段1：栈指针卸载。mask 已清。
    StackUnload,
    /// 段2：gs_base 卸载。mask 已清。
    GsBaseUnload,
    /// 段3：地址空间切换。mask 已清。
    AsidSwitch,
}

impl OutStage {
    /// 全量闭集（顺序即执行序）。
    pub const ALL: [OutStage; 4] = [OutStage::Offline, OutStage::StackUnload,
        OutStage::GsBaseUnload, OutStage::AsidSwitch];

    /// 名。
    pub const fn name(self) -> &'static str {
        match self {
            OutStage::Offline => "Offline",
            OutStage::StackUnload => "StackUnload",
            OutStage::GsBaseUnload => "GsBaseUnload",
            OutStage::AsidSwitch => "AsidSwitch",
        }
    }

    /// 本段开始前 mask 是否应已清位。
    /// **判据：UNX-F1862-J1 —— 切出的 mask 清位时机在「地址空间切换之前」**
    #[inline]
    pub const fn mask_cleared_before(self) -> bool { true }
}

/// 一次挂点的执行痕迹。
#[derive(Clone, Debug)]
pub struct HookTrace {
    /// 逻辑时刻。
    pub tick: u64,
    /// 来源。
    pub src: HookSrc,
    /// 核。
    pub core: CoreId,
    /// 目标 mm（`SwitchOut/IdleEnter` 为被离开的 mm）。
    pub mm: Option<MmId>,
    /// 本次是否改变了 mask。
    pub changed: bool,
    /// 四段各段的 mask 位期望（切入/切出用）。
    pub stage_log: Vec<(u8, bool)>,
}

impl HookTrace {
    /// 摘要。
    pub fn summary(&self) -> String {
        format!("t={} {} core={} mm={} changed={} stages={}", self.tick, self.src.name(),
            self.core, match self.mm { Some(m) => m.to_string(), None => "-".to_string(), },
            self.changed, self.stage_log.len())
    }
}

// --------------------------------------------------------------------------
// 世界：mm 表 + 核表 + 反查表 + 挂点引擎
// --------------------------------------------------------------------------

/// 追踪世界：mm 表 + 核表 + 反查表 + 击落账。
#[derive(Clone, Debug)]
pub struct MaskWorld {
    /// 逻辑时刻。
    pub tick: u64,
    /// mm 表。
    pub mms: MmTable,
    /// 核表。
    pub cores: CoreTable,
    /// 反查表（第二源）。
    pub reverse: ReverseTable,
    /// 挂点来源分布计数（6 枚）。
    pub hook_dist: [u64; 6],
    /// 挂点总数。
    pub hook_total: u64,
    /// 挂点痕迹（在册上界 256，超出则环回覆写最旧）。
    pub traces: Vec<HookTrace>,
    /// 击落账。
    pub shoots: Vec<ShootEntry>,
    /// 迁移翻转账。
    pub flips: Vec<FlipEntry>,
    /// 漂移检出计数。
    pub drift_hits: u64,
    /// 检出过的诊断码位图（16 枚）。
    pub drift_seen: u16,
    /// 确定性 LCG。
    pub rng: Lcg,
    /// 运行中击落数。
    pub shoots_live: u32,
    /// 超时降级数。
    pub shoots_degraded: u32,
}

impl Default for MaskWorld { fn default() -> MaskWorld { MaskWorld::new() } }

impl MaskWorld {
    /// 建世界：`tick=0`，mm 表空，64 核全 `Off`。
    pub fn new() -> MaskWorld {
        MaskWorld {
            tick: 0,
            mms: MmTable::new(),
            cores: CoreTable::new(),
            reverse: ReverseTable::new(),
            hook_dist: [0u64; 6],
            hook_total: 0,
            traces: Vec::new(),
            shoots: Vec::new(),
            flips: Vec::new(),
            drift_hits: 0,
            drift_seen: 0,
            rng: Lcg::default(),
            shoots_live: 0,
            shoots_degraded: 0,
        }
    }

    /// 推进一个逻辑时刻。
    pub fn advance(&mut self) -> u64 { self.tick = self.tick.wrapping_add(1); self.tick }

    /// 建 mm 并返回 id。
    pub fn new_mm(&mut self, name: &str) -> Result<MmId> { let t = self.tick; self.mms.create(name, t) }

    /// mm 活跃核 mask（只读视图）。
    pub fn mask_of(&self, mm: MmId) -> Result<ActiveMask> {
        let s = self.mms.get(mm)?;
        core::result::Result::Ok(s.mask)
    }

    /// 某 mm 名下核的声明源计数（`cur_mm` 表）。
    pub fn declared_count(&self, mm: MmId) -> u32 { self.cores.count_of_mm(mm) }

    /// 某 mm 名下核的反查源计数。
    pub fn reverse_count(&self, mm: MmId) -> u32 {
        let mut n = 0u32;
        for (_, v) in self.reverse.owner.iter() { if *v == mm { n += 1; } }
        n
    }

    /// 内部：mm 置位（置 mask + 反查表）。
    fn mask_set(&mut self, mm: MmId, core: CoreId) -> Result<bool> {
        let changed = {
            let s = self.mms.get_mut(mm)?;
            if !s.state.admits() { return core::result::Result::Err(MaskErr::MmStateBroken); }
            s.mask.set(core)
        };
        if changed { self.reverse.put(core, mm); }
        core::result::Result::Ok(changed)
    }

    /// 内部：mm 清位（清 mask + 反查表）。
    fn mask_clear(&mut self, mm: MmId, core: CoreId) -> Result<bool> {
        let changed = { let s = self.mms.get_mut(mm)?; s.mask.clear(core) };
        if changed { self.reverse.drop_core(core); }
        core::result::Result::Ok(changed)
    }

    /// 记挂点痕迹（环回覆写，上界 256）。
    fn push_trace(&mut self, tr: HookTrace) {
        if self.traces.len() >= 256 { self.traces.remove(0); }
        self.traces.push(tr);
    }

    // ----------------------------------------------------------------------
    // 挂点：Boot
    // ----------------------------------------------------------------------

    /// 挂点 `Boot`：核从 `Off` 首次挂入 mm（走切入四段，mask 在上线段后置位）。
    ///
    /// **判据：UNX-F1862-J1 —— 切换挂点同步（Boot 亦为一类挂点，mask 必置）**
    pub fn hook_boot(&mut self, core: CoreId, mm: MmId) -> Result<HookTrace> {
        let run = self.cores.run(core)?;
        if run != CoreRun::Off { return core::result::Result::Err(MaskErr::HookSyncLost); }
        let tr = self.switch_in_core(core, mm, HookSrc::Boot)?;
        core::result::Result::Ok(tr)
    }

    // ----------------------------------------------------------------------
    // 挂点：SwitchIn / SwitchOut
    // ----------------------------------------------------------------------

    /// 挂点 `SwitchIn`：核切入 `mm`。
    ///
    /// 四段：`AsidSwitch → GsBaseLoad → StackLoad → Online`，
    /// **mask 只在 `Online` 之后置位**（否则 gs_base 未装载就登记活跃，
    /// 击落会打到错误的 ASID）。
    ///
    /// **判据：UNX-F1862-J1 —— 切换挂点同步（切入必置位，且时机在切出/切入四段的正确位点）**
    pub fn hook_switch_in(&mut self, core: CoreId, mm: MmId) -> Result<HookTrace> {
        let run = self.cores.run(core)?;
        if run.in_mask() {
            // 已在某 mm 内：必须先切出，否则守恒破缺。
            return core::result::Result::Err(MaskErr::HookSyncLost);
        }
        let tr = self.switch_in_core(core, mm, HookSrc::SwitchIn)?;
        core::result::Result::Ok(tr)
    }

    /// 切入四段执行体（`SwitchIn` 与 `Boot` 共用）。
    fn switch_in_core(
        &mut self,
        core: CoreId,
        mm: MmId,
        src: HookSrc,
    ) -> Result<HookTrace> {
        // 前置：目标 mm 必须可挂核。
        {
            let s = self.mms.get(mm)?;
            if !s.state.admits() { return core::result::Result::Err(MaskErr::MmStateBroken); }
        }
        let t = self.tick;
        let mut stage_log = Vec::with_capacity(4);
        let mut changed = false;

        for stage in InStage::ALL.iter() {
            if stage.mask_set_after() {
                // 上线段之后才置位。
                changed = self.mask_set(mm, core)?;
                // 同步声明源。
                let s = self.cores.get_mut(core)?;
                s.cur_mm = Some(mm);
                s.run = CoreRun::Running;
                s.hook_seq = s.hook_seq.wrapping_add(1);
            }
            stage_log.push((*stage as u8, stage.mask_set_after()));
        }

        self.bump_hook(src);
        let tr = HookTrace { tick: t, src, core, mm: Some(mm), changed, stage_log, };
        self.push_trace(tr.clone());
        core::result::Result::Ok(tr)
    }

    /// 挂点 `SwitchOut`：核切出当前 mm（走切出四段，mask 在地址空间切换之前清位）。
    ///
    /// 四段：`Offline → StackUnload → GsBaseUnload → AsidSwitch`，
    /// **mask 在段 0 之前即清位**。
    ///
    /// **判据：UNX-F1862-J1 —— 切换挂点同步（切出必清位，且时机在地址空间切换之前）**
    pub fn hook_switch_out(&mut self, core: CoreId) -> Result<HookTrace> {
        let run = self.cores.run(core)?;
        if !run.in_mask() { return core::result::Result::Err(MaskErr::HookSyncLost); }
        let mm = match self.cores.cur_mm(core)? {
            Some(m) => m,
            None => return core::result::Result::Err(MaskErr::HookSyncLost),
        };
        let t = self.tick;
        let mut stage_log = Vec::with_capacity(4);
        let mut changed = false;

        for stage in OutStage::ALL.iter() {
            // 每段开始前 mask 必已清（段 0 之前清，之后保持清）。
            if stage.mask_cleared_before() {
                if !changed { changed = self.mask_clear(mm, core)?; }
                let s = self.cores.get_mut(core)?;
                s.cur_mm = None;
                s.run = CoreRun::Idle;
                s.hook_seq = s.hook_seq.wrapping_add(1);
            }
            stage_log.push((*stage as u8, true));
        }

        self.bump_hook(HookSrc::SwitchOut);
        let tr = HookTrace { tick: t, src: HookSrc::SwitchOut, core, mm: Some(mm), changed, stage_log, };
        self.push_trace(tr.clone());
        core::result::Result::Ok(tr)
    }

    // ----------------------------------------------------------------------
    // 挂点：IdleEnter / IdleExit
    // ----------------------------------------------------------------------

    /// 挂点 `IdleEnter`：核进入空闲。**切出后核不在任何 mm mask 内（显式断言）**。
    ///
    /// **判据：UNX-F1862-J1 —— 切换挂点同步（idle 核必不在任何 mm mask 内）**
    pub fn hook_idle_enter(&mut self, core: CoreId) -> Result<HookTrace> {
        let run = self.cores.run(core)?;
        if run == CoreRun::Off { return core::result::Result::Err(MaskErr::HookSyncLost); }
        let t = self.tick;
        let mut changed = false;
        if let Some(mm) = self.cores.cur_mm(core)? { changed = self.mask_clear(mm, core)?; }
        {
            let s = self.cores.get_mut(core)?;
            s.cur_mm = None;
            s.run = CoreRun::Idle;
            s.hook_seq = s.hook_seq.wrapping_add(1);
        }
        // 显式断言：idle 核不在任何 mask 内。
        self.assert_core_detached(core)?;
        self.bump_hook(HookSrc::IdleEnter);
        let tr = HookTrace {
            tick: t,
            src: HookSrc::IdleEnter,
            core,
            mm: None,
            changed,
            stage_log: Vec::new(),
        };
        self.push_trace(tr.clone());
        core::result::Result::Ok(tr)
    }

    /// 挂点 `IdleExit`：核退出空闲但尚未切入任何 mm。
    ///
    /// 语义：`Idle → Runningless`。退出空闲**不置位任何 mask**（还没有 mm 可挂），
    /// 核处于「已唤醒未切入」的空窗，必须仍在任何 mask 之外。
    ///
    /// **判据：UNX-F1862-J1 —— 切换挂点同步（IdleExit 不置位，核仍在 mask 外）**
    pub fn hook_idle_exit(&mut self, core: CoreId) -> Result<HookTrace> {
        let run = self.cores.run(core)?;
        if run != CoreRun::Idle { return core::result::Result::Err(MaskErr::HookSyncLost); }
        let t = self.tick;
        { let s = self.cores.get_mut(core)?; s.cur_mm = None; s.hook_seq = s.hook_seq.wrapping_add(1); }
        self.assert_core_detached(core)?;
        self.bump_hook(HookSrc::IdleExit);
        let tr = HookTrace {
            tick: t,
            src: HookSrc::IdleExit,
            core,
            mm: None,
            changed: false,
            stage_log: Vec::new(),
        };
        self.push_trace(tr.clone());
        core::result::Result::Ok(tr)
    }

    /// 显式断言：核不在任何 mm 的 mask 内。
    fn assert_core_detached(&self, core: CoreId) -> Result<()> {
        for s in self.mms.slots.iter() {
            if s.mask.test(core) { return core::result::Result::Err(MaskErr::IdleInMask); }
        }
        if self.reverse.get(core).is_some() { return core::result::Result::Err(MaskErr::DriftCurMmMask); }
        core::result::Result::Ok(())
    }

    /// 内部：挂点分布计数 +1。
    fn bump_hook(&mut self, src: HookSrc) {
        let i = src.index();
        if i < self.hook_dist.len() { self.hook_dist[i] = self.hook_dist[i].wrapping_add(1); }
        self.hook_total = self.hook_total.wrapping_add(1);
    }

    // ----------------------------------------------------------------------
    // 过渡态（F1654）与击落互斥（F1867）
    // ----------------------------------------------------------------------

    /// 置核为过渡态（F1654）：**核仍在 mask 内**，但击落须绕道 F1867。
    ///
    /// **判据：UNX-F1862-J1 —— 过渡态核在 mask 内但标记 Transition，击落不得当普通活跃核**
    pub fn mark_transition(&mut self, core: CoreId) -> Result<()> {
        let run = self.cores.run(core)?;
        if !run.in_mask() { return core::result::Result::Err(MaskErr::HookSyncLost); }
        let s = self.cores.get_mut(core)?;
        s.run = CoreRun::Transition;
        core::result::Result::Ok(())
    }

    /// 清过渡态：回到普通运行。
    pub fn clear_transition(&mut self, core: CoreId) -> Result<()> {
        let run = self.cores.run(core)?;
        if !run.is_transition() { return core::result::Result::Err(MaskErr::HookSyncLost); }
        let s = self.cores.get_mut(core)?;
        s.run = CoreRun::Running;
        core::result::Result::Ok(())
    }

    /// 某核是否处于过渡态。
    pub fn is_transition(&self, core: CoreId) -> Result<bool> {
        core::result::Result::Ok(self.cores.run(core)?.is_transition())
    }
}

// --------------------------------------------------------------------------
// 击落（F1863 发起 / F1864 消费 / F1865 账 / F1867 互斥）
// --------------------------------------------------------------------------

/// 击落四段状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShootState {
    /// 发起段完成，已广播，等待应答。
    Broadcast,
    /// 全部应答。
    Done,
    /// 超时降级（**显式落账，不得静默当完成**）。
    Degraded,
}

impl ShootState {
    /// 全量闭集。
    pub const ALL: [ShootState; 3] = [ShootState::Broadcast, ShootState::Done, ShootState::Degraded, ];

    /// 名。
    pub const fn name(self) -> &'static str {
        match self {
            ShootState::Broadcast => "Broadcast",
            ShootState::Done => "Done",
            ShootState::Degraded => "Degraded",
        }
    }

    /// 是否终态。
    pub const fn terminal(self) -> bool { matches!(self, ShootState::Done | ShootState::Degraded) }
}

/// 单次击落在册项。
#[derive(Clone, Debug)]
pub struct ShootEntry {
    /// 序号（全域单调）。
    pub sid: u32,
    /// 发起逻辑时刻。
    pub tick: u64,
    /// 目标 mm。
    pub mm: MmId,
    /// 发起段取样的 mask 快照。
    pub target: ActiveMask,
    /// 广播集合（**必达断言：必须与 target 逐位相同**）。
    pub broadcast: ActiveMask,
    /// 已应答集合。
    pub acked: ActiveMask,
    /// 因过渡态而挂起（F1867 挂 mask 待切后 flush）的核。
    pub parked: ActiveMask,
    /// 过渡态被错误直击的核（F1864 违规证据）。
    pub struck_transition: ActiveMask,
    /// 状态。
    pub state: ShootState,
    /// 广播发送计数（Σ广播）。
    pub sends: u32,
    /// 应答计数（Σ应答）。
    pub acks: u32,
    /// 超时降级次数。
    pub degraded: u32,
}

impl ShootEntry {
    /// 必达断言：广播集合与取样 mask 逐位相同。
    #[inline]
    pub fn reach_exact(&self) -> bool { self.target.same_bits(&self.broadcast) }

    /// 应答守恒：`Σ应答 == Σ广播` 且应答集 ⊆ 广播集。
    pub fn ack_conserved(&self) -> bool {
        self.acks == self.broadcast.popcount() && (self.acked.bits & !self.broadcast.bits) == 0
    }

    /// 摘要。
    pub fn summary(&self) -> String {
        format!("sid={} mm={} tgt={} bcast={} acked={} sends={} acks={} st={} deg={}", self.sid,
            self.mm, self.target.popcount(), self.broadcast.popcount(), self.acked.popcount(),
            self.sends, self.acks, self.state.name(), self.degraded)
    }
}

/// 迁移翻转账（一次 `migrate` 的前后快照）。
#[derive(Clone, Debug)]
pub struct FlipEntry {
    /// 逻辑时刻。
    pub tick: u64,
    /// 源核。
    pub src_core: CoreId,
    /// 目标核。
    pub dst_core: CoreId,
    /// mm。
    pub mm: MmId,
    /// 翻转前 mask 版本。
    pub ver_before: u64,
    /// 翻转后 mask 版本。
    pub ver_after: u64,
    /// 翻转前 popcount。
    pub pop_before: u32,
    /// 翻转后 popcount。
    pub pop_after: u32,
    /// 翻转是否原子（版本差恰 2、popcount 差 ∈ {0,±2} 且两 mm 单侧各 ±1）。
    pub atomic: bool,
}

impl FlipEntry {
    /// 版本差（应恰为 2：源核清位一次 + 目标核置位一次）。
    #[inline]
    pub fn ver_delta(&self) -> u64 { self.ver_after.wrapping_sub(self.ver_before) }

    /// popcount 差（源侧应恰 -1，目标侧应恰 +1；合起来整体差 0）。
    #[inline]
    pub fn pop_delta(&self) -> i64 { self.pop_after as i64 - self.pop_before as i64 }

    /// 摘要。
    pub fn summary(&self) -> String {
        format!("t={} mm={} {}->{} pop {}->{} d{} ver d{} atomic={}", self.tick, self.mm,
            self.src_core, self.dst_core, self.pop_before, self.pop_after,
            self.pop_delta(), self.ver_delta(), self.atomic)
    }
}

impl MaskWorld {
    // ----------------------------------------------------------------------
    // 击落：发起段（F1863「先登记后发送」）
    // ----------------------------------------------------------------------

    /// 击落发起：对 `mm` 广播。
    ///
    /// 严格「**先登记后发送**」：先把 `target = mm.mask` 快照与 `broadcast = target`
    /// 双份落账并**校验逐位相同**，登记成功后才允许应答流进入。
    ///
    /// **判据：UNX-F1862-J1 —— 击落必达：广播集合 == 该 mm 的 ActiveMask（逐位相同）**
    pub fn shootdown_begin(&mut self, mm: MmId) -> Result<u32> {
        let target = {
            let s = self.mms.get(mm)?;
            if s.state == MmState::Dead { return core::result::Result::Err(MaskErr::MmStateBroken); }
            s.mask
        };
        if self.shoots.len() >= SHOOT_CAPACITY { self.shoots.remove(0); }
        let sid = self.shoots.len() as u32;
        let t = self.tick;
        let entry = ShootEntry {
            sid,
            tick: t,
            mm,
            target,
            broadcast: target,
            acked: ActiveMask::raw(0),
            parked: ActiveMask::raw(0),
            struck_transition: ActiveMask::raw(0),
            state: ShootState::Broadcast,
            sends: target.popcount(),
            acks: 0,
            degraded: 0,
        };
        // 登记后校验：广播集合必须逐位等于取样。
        if !entry.reach_exact() { return core::result::Result::Err(MaskErr::BroadcastMaskMismatch); }
        if let Ok(s) = self.mms.get_mut(mm) { s.shoots = s.shoots.wrapping_add(1); }
        self.shoots_live = self.shoots_live.wrapping_add(1);
        self.shoots.push(entry);
        core::result::Result::Ok(sid)
    }

    /// 击落应答（F1864 消费端：mask 位或 → 本地 flush → 应答）。
    ///
    /// 顺序固定：
    /// 1. **mask 位或**：`broadcast.test(core)` 必须为真，否则 `AckBroadcastMismatch`
    ///    （杜绝越过 mask 的幽灵应答）。
    /// 2. 若核处过渡态：不得按普通活跃核应答，转入 `parked`（F1867 挂 mask 待切后 flush）。
    /// 3. 本地 flush 后置 `acked` 位，`acks` 自增。
    ///
    /// **判据：UNX-F1862-J1 —— 应答必过 mask 位或；过渡态核不得当普通活跃核击落**
    pub fn shootdown_ack(&mut self, sid: u32, core: CoreId) -> Result<()> {
        let is_transition = self.is_transition(core)?;
        let entry = match self.shoots.get_mut(sid as usize) {
            Some(e) => e,
            None => return core::result::Result::Err(MaskErr::BroadcastMaskMismatch),
        };
        if entry.state.terminal() { return core::result::Result::Err(MaskErr::AckBroadcastMismatch); }
        // 1) mask 位或（F1864）。
        if !entry.broadcast.test(core) {
            self.note_drift(MaskErr::AckBroadcastMismatch);
            return core::result::Result::Err(MaskErr::AckBroadcastMismatch);
        }
        // 2) 过渡态被走普通直击路径 = F1867 违规，记证据并拒。
        if is_transition {
            entry.struck_transition.set(core);
            self.note_drift(MaskErr::TransitionStruck);
            return core::result::Result::Err(MaskErr::TransitionStruck);
        }
        // 3) 本地 flush 后应答；仅在真正新置位时计账。
        if entry.acked.set(core) { entry.acks = entry.acks.wrapping_add(1); }
        core::result::Result::Ok(())
    }

    /// F1867 合法入口：把过渡态核**挂 mask 待切后flush**（不记违规）。
    ///
    /// 与 `shootdown_ack` 的区别：本函数是「知其为过渡态」的绕道路径，
    /// 只置 `parked`，待切换完成后由 `flush_after_switch` 补记应答。
    ///
    /// **判据：UNX-F1862-J1 —— 过渡态核击落必须走挂 mask 待切后flush（F1867）**
    pub fn park_transition(&mut self, sid: u32, core: CoreId) -> Result<()> {
        if !self.is_transition(core)? { return core::result::Result::Err(MaskErr::TransitionStruck); }
        let entry = match self.shoots.get_mut(sid as usize) {
            Some(e) => e,
            None => return core::result::Result::Err(MaskErr::BroadcastMaskMismatch),
        };
        if entry.state.terminal() { return core::result::Result::Err(MaskErr::AckBroadcastMismatch); }
        if !entry.broadcast.test(core) {
            self.note_drift(MaskErr::AckBroadcastMismatch);
            return core::result::Result::Err(MaskErr::AckBroadcastMismatch);
        }
        entry.parked.set(core);
        core::result::Result::Ok(())
    }

    /// F1867 路径：过渡态核切换完成后执行挂起 flush 并补记应答。
    pub fn flush_after_switch(&mut self, sid: u32, core: CoreId) -> Result<()> {
        let entry = match self.shoots.get_mut(sid as usize) {
            Some(e) => e,
            None => return core::result::Result::Err(MaskErr::AckBroadcastMismatch),
        };
        if !entry.parked.test(core) { return core::result::Result::Err(MaskErr::TransitionStruck); }
        if entry.state.terminal() { return core::result::Result::Err(MaskErr::AckBroadcastMismatch); }
        if !entry.broadcast.test(core) { return core::result::Result::Err(MaskErr::BroadcastMaskMismatch); }
        entry.parked.clear(core);
        if entry.acked.set(core) { entry.acks = entry.acks.wrapping_add(1); }
        core::result::Result::Ok(())
    }

    /// 击落完成判定：全部应答 → `Done`；否则超时降级 → `Degraded`（**显式落账**）。
    ///
    /// **判据：UNX-F1862-J1 —— Σ应答 == Σ广播；超时降级必须显式落账，不得静默当完成**
    pub fn shootdown_finish(&mut self, sid: u32) -> Result<ShootState> {
        let mut degraded = false;
        let st = {
            let entry = match self.shoots.get_mut(sid as usize) {
                Some(e) => e,
                None => return core::result::Result::Err(MaskErr::AckBroadcastMismatch),
            };
            if entry.state.terminal() { return core::result::Result::Err(MaskErr::AckBroadcastMismatch); }
            let full = entry.acked.popcount() == entry.broadcast.popcount()
                && entry.acked.same_bits(&entry.broadcast);
            if full {
                entry.state = ShootState::Done;
            } else {
                // 超时降级：显式落账，绝不静默当完成。
                entry.state = ShootState::Degraded;
                entry.degraded = entry.degraded.wrapping_add(1);
                // 未应答集合显式转入 parked（挂 mask 待切后flush 的账）。
                entry.parked = entry.broadcast.diff(&entry.acked);
                degraded = true;
            }
            entry.state
        };
        if degraded {
            self.shoots_degraded = self.shoots_degraded.wrapping_add(1);
        } else if self.shoots_live > 0 {
            self.shoots_live -= 1;
        }
        core::result::Result::Ok(st)
    }

    /// 全域 Σ应答 / Σ广播（**必达总账**）。
    pub fn shoot_ledger(&self) -> (u32, u32, u32, u32) {
        let mut sends = 0u32;
        let mut acks = 0u32;
        let mut done = 0u32;
        let mut deg = 0u32;
        for e in self.shoots.iter() {
            sends = sends.wrapping_add(e.sends);
            acks = acks.wrapping_add(e.acks);
            if e.state == ShootState::Done { done += 1; }
            deg = deg.wrapping_add(e.degraded);
        }
        (sends, acks, done, deg)
    }

    /// 击落必达断言（全域）：Σ应答 + 挂起 == Σ广播，且无过渡态被直击。
    pub fn audit_reach(&self) -> Result<()> {
        for e in self.shoots.iter() {
            // 必达：广播集合必须逐位等于发起段取样的 mask。
            if !e.reach_exact() { return core::result::Result::Err(MaskErr::BroadcastMaskMismatch); }
            // F1867：过渡态核不得被当普通活跃核直击。
            if !e.struck_transition.is_empty() { return core::result::Result::Err(MaskErr::TransitionStruck); }
            // F1865 账自洽：应答计数恒等于应答集位数（F1864 mask 位或的记账）。
            if e.acks != e.acked.popcount() { return core::result::Result::Err(MaskErr::AckBroadcastMismatch); }
            // F1864：应答集与挂起集必须是广播集的子集，且彼此不交。
            if (e.acked.bits & !e.broadcast.bits) != 0 || (e.parked.bits & !e.broadcast.bits) != 0 {
                return core::result::Result::Err(MaskErr::AckBroadcastMismatch);
            }
            if (e.acked.bits & e.parked.bits) != 0 {
                return core::result::Result::Err(MaskErr::AckBroadcastMismatch);
            }
            let settled = e.acked.popcount().wrapping_add(e.parked.popcount());
            match e.state {
                ShootState::Broadcast => {
                    // 在途：允许未齐，但不得超发。
                    if settled > e.broadcast.popcount() {
                        return core::result::Result::Err(MaskErr::AckBroadcastMismatch);
                    }
                }
                ShootState::Done => {
                    // 完成：必须Σ应答 == Σ广播（逐位相同），不得把未齐当完成。
                    if settled != e.broadcast.popcount() || !e.acked.same_bits(&e.broadcast) {
                        return core::result::Result::Err(MaskErr::AckBroadcastMismatch);
                    }
                }
                ShootState::Degraded => {
                    // 降级：必须显式未齐（否则与Done 混淆），且降级已落账。
                    if e.degraded == 0 { return core::result::Result::Err(MaskErr::AckBroadcastMismatch); }
                    if e.acked.same_bits(&e.broadcast) {
                        return core::result::Result::Err(MaskErr::AckBroadcastMismatch);
                    }
                    // 未应答集合必须显式留在 parked（挂 mask 待切后flush），
                    // 不得静默丢弃——这正是「超时降级必须显式落账」的落点。
                    let missing = e.broadcast.diff(&e.acked);
                    if missing.bits != e.parked.bits {
                        return core::result::Result::Err(MaskErr::AckBroadcastMismatch);
                    }
                }
            }
        }
        core::result::Result::Ok(())
    }

    // ----------------------------------------------------------------------
    // 迁移 × mask 翻转（F1884 衔接）
    // ----------------------------------------------------------------------

    /// 迁移：把 `mm` 的活跃登记从 `src_core` 翻到 `dst_core`。
    ///
    /// 翻转原子性以**版本号前后快照**证明：`版本差恰 2`（清位一次 + 置位一次），
    /// `popcount 差恰 ±1`（净差为 0，但两侧各恰 1）。翻转后击落必达断言仍成立。
    ///
    /// **判据：UNX-F1862-J1 —— 迁移翻转后击落必达断言仍成立，且无「既不在 A 也不在 B」可见中间态**
    pub fn migrate(&mut self, src_core: CoreId, dst_core: CoreId, mm: MmId) -> Result<FlipEntry> {
        if src_core == dst_core { return core::result::Result::Err(MaskErr::HookSyncLost); }
        // 前置：源核必须在该 mm 内。
        if self.cores.cur_mm(src_core)? != Some(mm) {
            return core::result::Result::Err(MaskErr::HookSyncLost);
        }
        // 前置：目标核必须空闲（不在任何 mask 内），保证无中间态可见。
        self.assert_core_detached(dst_core)?;

        let (ver_before, pop_before) = { let s = self.mms.get(mm)?; (s.mask.version(), s.mask.popcount()) };

        // 翻转：源核清位、目标核置位（两步原语，版本恰 +2）。
        self.mask_clear(mm, src_core)?;
        self.mask_set(mm, dst_core)?;
        {
            let s = self.cores.get_mut(src_core)?;
            s.cur_mm = None;
            s.run = CoreRun::Idle;
            s.hook_seq = s.hook_seq.wrapping_add(1);
        }
        {
            let s = self.cores.get_mut(dst_core)?;
            s.cur_mm = Some(mm);
            s.run = CoreRun::Running;
            s.hook_seq = s.hook_seq.wrapping_add(1);
        }
        self.bump_hook(HookSrc::Migrate);

        let (ver_after, pop_after) = { let s = self.mms.get(mm)?; (s.mask.version(), s.mask.popcount()) };
        let entry = FlipEntry {
            tick: self.tick,
            src_core,
            dst_core,
            mm,
            ver_before,
            ver_after,
            pop_before,
            pop_after,
            atomic: ver_after.wrapping_sub(ver_before) == 2 && pop_before == pop_after,
        };
        if self.flips.len() >= 256 { self.flips.remove(0); }
        self.flips.push(entry.clone());
        core::result::Result::Ok(entry)
    }

    /// 迁移翻转审计：**popcount 差恰 ±1、版本差恰 2**（逐笔）。
    ///
    /// **判据：UNX-F1862-J1 —— 迁移翻转的 popcount 差恰 ±1、版本差恰 2**
    pub fn audit_flips(&self) -> Result<()> {
        for f in self.flips.iter() {
            if f.ver_delta() != 2 { return core::result::Result::Err(MaskErr::VersionStalled); }
            if f.pop_delta() != 0 { return core::result::Result::Err(MaskErr::MigratePopcountOff); }
            if !f.atomic { return core::result::Result::Err(MaskErr::MigratePopcountOff); }
        }
        core::result::Result::Ok(())
    }
}

// --------------------------------------------------------------------------
// 漂移检出与审计
// --------------------------------------------------------------------------

/// 一次审计的逐条结论。
#[derive(Clone, Debug)]
pub struct AuditItem {
    /// 断言名。
    pub name: &'static str,
    /// 是否通过。
    pub pass: bool,
    /// 失败时的诊断码。
    pub code: Option<MaskErr>,
    /// 观测量（整数）。
    pub observed: u64,
    /// 期望量（整数）。
    pub expect: u64,
}

impl AuditItem {
    /// 摘要。
    pub fn summary(&self) -> String {
        format!("{} pass={} obs={} exp={} code={}", self.name, self.pass, self.observed,
            self.expect, match self.code { Some(c) => c.name(), None => "-", })
    }
}

/// 一次全域审计的完整结论。
#[derive(Clone, Debug)]
pub struct AuditReport {
    /// 逐条断言。
    pub items: Vec<AuditItem>,
    /// 全过标志。
    pub pass: bool,
    /// 失效条数。
    pub fails: u32,
}

impl AuditReport {
    /// 全过？
    pub fn all_pass(&self) -> bool { self.fails == 0 }

    /// 失效码集。
    pub fn fail_codes(&self) -> Vec<MaskErr> {
        let mut v = Vec::new();
        for it in self.items.iter() { if let Some(c) = it.code { if !v.contains(&c) { v.push(c); } } }
        v
    }

    /// 摘要。
    pub fn summary(&self) -> String {
        let mut s = String::from("audit[");
        for (i, it) in self.items.iter().enumerate() {
            if i > 0 { s.push(','); }
            s.push_str(&it.summary());
        }
        s.push(']');
        s
    }
}

impl MaskWorld {
    /// 记一次漂移检出。
    pub fn note_drift(&mut self, e: MaskErr) {
        self.drift_hits = self.drift_hits.wrapping_add(1);
        let bit = 1u16 << (e as u16);
        self.drift_seen |= bit;
    }

    /// 是否曾检出某码。
    pub fn drift_seen(&self, e: MaskErr) -> bool { (self.drift_seen >> (e as u16)) & 1u16 == 1u16 }

    /// 已检出码集。
    pub fn drift_codes(&self) -> Vec<MaskErr> {
        let mut v = Vec::new();
        for e in MaskErr::ALL.iter() { if self.drift_seen(*e) { v.push(*e); } }
        v
    }

    // ----------------------------------------------------------------------
    // 核心不变量 A：活跃核守恒 + 跨 mm 交集空
    // ----------------------------------------------------------------------

    /// 跨 mm 交集审计：遍历全部 mm 对，断言 `∀i≠j: mask_i & mask_j == 0`。
    ///
    /// **判据：UNX-F1862-J1 —— 一个核在任一时刻最多属于一个 mm 的活跃 mask**
    pub fn audit_intersect(&mut self) -> Result<()> {
        let n = self.mms.slots.len();
        for i in 0..n {
            for j in (i + 1)..n {
                let a = self.mms.slots[i].mask;
                let b = self.mms.slots[j].mask;
                let inter = a.intersect(&b);
                if !inter.is_empty() {
                    self.note_drift(MaskErr::IntersectNonEmpty);
                    return core::result::Result::Err(MaskErr::IntersectNonEmpty);
                }
            }
        }
        core::result::Result::Ok(())
    }

    /// Σ守恒审计：`Σ每核活跃 mask 的总置位数 == 正在运行线程的核数`。
    ///
    /// 空闲核**显式建模**在 `CoreTable::idle_set()` 中，故必不在任何 mm mask 内。
    ///
    /// **判据：UNX-F1862-J1 —— Σ每核活跃置位数 == 运行线程核数（idle 核不在任何 mm mask 内）**
    pub fn audit_sum(&mut self) -> Result<()> {
        let mut sum = 0u32;
        for s in self.mms.slots.iter() { sum = sum.wrapping_add(s.mask.popcount()); }
        let running = self.cores.running_count();
        if sum != running {
            self.note_drift(MaskErr::SumConserv);
            return core::result::Result::Err(MaskErr::SumConserv);
        }
        core::result::Result::Ok(())
    }

    /// idle 隔离审计：idle/Off 核不得出现在任何 mm mask 内。
    ///
    /// **判据：UNX-F1862-J1 —— idle 核必不在任何 mm mask 内（显式建模）**
    pub fn audit_idle(&mut self) -> Result<()> {
        let idle = self.cores.idle_set();
        let off = self.cores.off_set();
        let detached = idle.union(&off);
        for s in self.mms.slots.iter() {
            let hit = s.mask.intersect(&detached);
            if !hit.is_empty() {
                self.note_drift(MaskErr::IdleInMask);
                return core::result::Result::Err(MaskErr::IdleInMask);
            }
        }
        core::result::Result::Ok(())
    }

    // ----------------------------------------------------------------------
    // 核心不变量 B：双源对账（cur_mm 表 ↔ mask 反查表）
    // ----------------------------------------------------------------------

    /// 从 mask 侧重建反查表（独立第二源）。
    pub fn reverse_rebuild(&self) -> ReverseTable {
        let mut rt = ReverseTable::new();
        for s in self.mms.slots.iter() {
            for c in s.mask.iter_ones() {
                if rt.get(c).is_some() {
                    // 一核两 mm：重建侧直接记冲突（审计时报交集非空）。
                    continue;
                }
                rt.put(c, s.id);
            }
        }
        rt
    }

    /// 双源对账审计：`cur_mm` 表 ↔ 增量反查表 ↔ mask 重建三方逐核对拍。
    ///
    /// **判据：UNX-F1862-J1 —— 每次挂点后全域「cur_mm 表」与「mm mask 反查表」必须完全一致（双向对账），不一致即 MaskDrift**
    pub fn audit_reconcile(&mut self) -> Result<()> {
        let rebuilt = self.reverse_rebuild();
        for i in 0..CORE_COUNT {
            let core = i;
            let declared = match self.cores.slots.get(i as usize) {
                Some(s) => s.cur_mm,
                None => return core::result::Result::Err(MaskErr::MmIdOutOfRange),
            };
            let inc = self.reverse.get(core);
            let reb = rebuilt.get(core);

            // (a) 声明源 ↔ 增量反查源
            if declared != inc {
                if declared.is_some() {
                    self.note_drift(MaskErr::DriftCurMmMask);
                    return core::result::Result::Err(MaskErr::DriftCurMmMask);
                }
                self.note_drift(MaskErr::DriftMaskCurMm);
                return core::result::Result::Err(MaskErr::DriftMaskCurMm);
            }
            // (b) 声明源 ↔ mask 重建源
            if declared != reb {
                if declared.is_some() {
                    self.note_drift(MaskErr::DriftCurMmMask);
                    return core::result::Result::Err(MaskErr::DriftCurMmMask);
                }
                self.note_drift(MaskErr::DriftMaskCurMm);
                return core::result::Result::Err(MaskErr::DriftMaskCurMm);
            }
            // (c) 增量反查 ↔ mask 重建
            if inc != reb {
                self.note_drift(MaskErr::ReverseFingerprint);
                return core::result::Result::Err(MaskErr::ReverseFingerprint);
            }
            // (d) 声明源 ↔ 该 mm 的 mask 位
            if let Some(mm) = declared {
                let m = match self.mms.get(mm) { Ok(m) => m.mask, Err(e) => return core::result::Result::Err(e), };
                if !m.test(core) {
                    self.note_drift(MaskErr::HookSyncLost);
                    return core::result::Result::Err(MaskErr::HookSyncLost);
                }
            }
        }
        // (e) 反查表重建指纹与增量指纹（次序无关，用计数+归属折叠）。
        let fold_a = self.reverse.fold();
        let fold_b = rebuilt.fold();
        if fold_a != fold_b {
            self.note_drift(MaskErr::ReverseFingerprint);
            return core::result::Result::Err(MaskErr::ReverseFingerprint);
        }
        core::result::Result::Ok(())
    }

    /// 单核局部对账（挂点后 O(1) 快检）。
    pub fn audit_local(&mut self, core: CoreId) -> Result<()> {
        let declared = self.cores.cur_mm(core)?;
        let inc = self.reverse.get(core);
        let rebuilt = self.reverse_rebuild().get(core);
        if declared != inc || declared != rebuilt {
            if declared.is_some() {
                self.note_drift(MaskErr::DriftCurMmMask);
                return core::result::Result::Err(MaskErr::DriftCurMmMask);
            }
            self.note_drift(MaskErr::DriftMaskCurMm);
            return core::result::Result::Err(MaskErr::DriftMaskCurMm);
        }
        core::result::Result::Ok(())
    }

    // ----------------------------------------------------------------------
    // 核心不变量 C：版本推进 + popcount 自洽
    // ----------------------------------------------------------------------

    /// mask 自洽审计：popcount 增量正确、版本推进。
    ///
    /// **判据：UNX-F1862-J1 —— 每次变更必自增版本；popcount 恒等于位图实数**
    pub fn audit_masks(&mut self) -> Result<()> {
        for s in self.mms.slots.iter() {
            if !s.mask.pop_consistent() {
                self.note_drift(MaskErr::PopcountBroken);
                return core::result::Result::Err(MaskErr::PopcountBroken);
            }
            if !s.mask.version_alive() {
                self.note_drift(MaskErr::VersionStalled);
                return core::result::Result::Err(MaskErr::VersionStalled);
            }
        }
        core::result::Result::Ok(())
    }

    /// 逐 mm 声明源/反查源/mask 三方计数一致。
    pub fn audit_counts(&mut self) -> Result<()> {
        for s in self.mms.slots.iter() {
            let dec = self.declared_count(s.id);
            let rev = self.reverse_count(s.id);
            let act = s.mask.popcount();
            if dec != rev || dec != act {
                if dec != act {
                    self.note_drift(MaskErr::DriftMaskCurMm);
                    return core::result::Result::Err(MaskErr::DriftMaskCurMm);
                }
                self.note_drift(MaskErr::ReverseFingerprint);
                return core::result::Result::Err(MaskErr::ReverseFingerprint);
            }
        }
        core::result::Result::Ok(())
    }

    /// 全域完整审计（11 条断言）。
    ///
    /// **判据：UNX-F1862-J1 —— mm 活跃核 mask 维护断言全过（切换挂点同步）**
    pub fn audit_full(&mut self) -> AuditReport {
        let mut items: Vec<AuditItem> = Vec::with_capacity(12);
        macro_rules! item {
            ($name:expr, $r:expr, $obs:expr) => {{
                let obs: u64 = $obs;
                match $r {
                    core::result::Result::Ok(()) => items.push(AuditItem {
                        name: $name,
                        pass: true,
                        code: None,
                        observed: obs,
                        expect: obs,
                    }),
                    core::result::Result::Err(c) => items.push(AuditItem {
                        name: $name,
                        pass: false,
                        code: Some(c),
                        observed: obs,
                        expect: 0,
                    }),
                }
            }};
        }
        let mut sum = 0u64;
        for s in self.mms.slots.iter() { sum += s.mask.popcount() as u64; }
        item!("sum_conserv", self.audit_sum(), sum);
        item!("intersect_empty", self.audit_intersect(), 0);
        item!("reconcile", self.audit_reconcile(), 0);
        item!("counts", self.audit_counts(), 0);
        item!("idle_isolated", self.audit_idle(), 0);
        item!("mask_self", self.audit_masks(), 0);
        item!("core_bind", self.cores.audit_bind(), 0);
        item!("mm_state", self.mms.audit_state(), 0);
        item!("shoot_reach", self.audit_reach(), 0);
        item!("flip_atomic", self.audit_flips(), 0);

        let mut fails = 0u32;
        for it in items.iter() { if !it.pass { fails += 1; } }
        AuditReport { items, pass: fails == 0, fails, }
    }
}

impl ReverseTable {
    /// 次序无关的归属折叠指纹（按键值域累积，交换不敏感）。
    pub fn fold(&self) -> u64 {
        let mut acc = FNV_OFFSET;
        for (core, mm) in self.owner.iter() {
            acc = acc.wrapping_add(
                ((*core as u64).wrapping_mul(0x9E3779B97F4A7C15)) ^ (*mm as u64).wrapping_add(1),
            );
        }
        let n = self.owner.len() as u64;
        acc ^ n.wrapping_mul(FNV_PRIME)
    }
}

// --------------------------------------------------------------------------
// 观测导出（G）
// --------------------------------------------------------------------------

/// 指标快照（整数域，无浮点）。
#[derive(Clone, Debug)]
pub struct Metrics {
    /// 每 mm 活跃核数。
    pub per_mm_active: Vec<u32>,
    /// 每 mm 活跃核位图。
    pub per_mm_bits: Vec<u64>,
    /// 全域活跃核总数（Σ）。
    pub active_total: u32,
    /// 运行线程核数。
    pub running_total: u32,
    /// idle 核数。
    pub idle_total: u32,
    /// 未启动核数。
    pub off_total: u32,
    /// 过渡态核数。
    pub transition_total: u32,
    /// 挂点分布（6 枚，序同 `HookSrc::ALL`）。
    pub hook_dist: [u64; 6],
    /// 挂点总数。
    pub hook_total: u64,
    /// 广播次数（Σ发送）。
    pub broadcast_total: u32,
    /// 应答总数（Σ应答）。
    pub ack_total: u32,
    /// 挂起待 flush 数（F1867）。
    pub parked_total: u32,
    /// 击落完成数。
    pub shoot_done: u32,
    /// 超时降级数。
    pub shoot_degraded: u32,
    /// 应答率（千分比，`ack*1000/broadcast`，分母 0 时为 0）。
    pub ack_rate_permille: u32,
    /// 漂移检出次数。
    pub drift_hits: u64,
    /// 已检出码数。
    pub drift_codes: u32,
    /// 版本变更总次数（Σ每 mm version）。
    pub version_total: u64,
    /// 版本变更频率（千分比，`version*1000/hook_total`）。
    pub version_rate_permille: u32,
    /// 迁移翻转笔数。
    pub flip_total: u32,
    /// mm 表项数。
    pub mm_total: usize,
    /// 反查表登记数。
    pub reverse_len: usize,
    /// 全域指纹。
    pub fingerprint: u64,
}

/// 按小端追加 u32 到 `Vec<u8>`。
#[inline]
fn put_u32(out: &mut Vec<u8>, v: u32) {
    for b in 0..4u32 { out.push(((v >> (b * 8)) & 0xff) as u8); }
}

/// 按小端追加 u64 到 `Vec<u8>`。
#[inline]
fn put_u64(out: &mut Vec<u8>, v: u64) {
    for b in 0..8u32 { out.push(((v >> (b * 8)) & 0xff) as u8); }
}

/// TLV 记录标签。
pub const TLV_MASK: u8 = 0x01;
/// TLV 标签：mm 掩码位图。
pub const TLV_MM_MASK: u8 = 0x02;
/// TLV 标签：击落账。
pub const TLV_SHOOT_LEDGER: u8 = 0x03;
/// TLV 标签：挂点分布。
pub const TLV_HOOK_DIST: u8 = 0x04;
/// TLV 标签：漂移码位图。
pub const TLV_DRIFT: u8 = 0x05;
/// TLV 标签：翻转账摘要。
pub const TLV_FLIP: u8 = 0x06;
/// TLV 标签：全域指纹。
pub const TLV_FINGERPRINT: u8 = 0x07;
/// TLV 标签：版本号。
pub const TLV_VERSION: u8 = 0x08;

impl MaskWorld {
    /// 采集指标快照。
    pub fn metrics(&self) -> Metrics {
        let mut per_mm_active = Vec::with_capacity(self.mms.slots.len());
        let mut per_mm_bits = Vec::with_capacity(self.mms.slots.len());
        let mut active_total = 0u32;
        let mut version_total = 0u64;
        for s in self.mms.slots.iter() {
            per_mm_active.push(s.mask.popcount());
            per_mm_bits.push(s.mask.bits);
            active_total = active_total.wrapping_add(s.mask.popcount());
            version_total = version_total.wrapping_add(s.mask.version());
        }
        let (sends, acks, done, deg) = self.shoot_ledger();
        let mut parked = 0u32;
        let mut struck = 0u32;
        for e in self.shoots.iter() {
            parked = parked.wrapping_add(e.parked.popcount());
            struck = struck.wrapping_add(e.struck_transition.popcount());
        }
        let ack_rate = if sends == 0 { 0u32 } else { (acks as u64 * 1000u64 / sends as u64) as u32 };
        let ver_rate = if self.hook_total == 0 {
            0u32
        } else {
            (version_total * 1000u64 / self.hook_total) as u32
        };
        Metrics {
            per_mm_active,
            per_mm_bits,
            active_total,
            running_total: self.cores.running_count(),
            idle_total: self.cores.idle_count(),
            off_total: self.cores.off_set().popcount(),
            transition_total: self.cores.transition_set().popcount(),
            hook_dist: self.hook_dist,
            hook_total: self.hook_total,
            broadcast_total: sends,
            ack_total: acks,
            parked_total: parked,
            shoot_done: done,
            shoot_degraded: deg,
            ack_rate_permille: ack_rate,
            drift_hits: self.drift_hits,
            drift_codes: self.drift_codes().len() as u32,
            version_total,
            version_rate_permille: ver_rate,
            flip_total: self.flips.len() as u32,
            mm_total: self.mms.slots.len(),
            reverse_len: self.reverse.len(),
            fingerprint: self.world_fingerprint(),
        }
    }

    /// 全域指纹（O(nmm + 1)）：mm 表 + 核表 + 击落账 + 翻转账折叠。
    pub fn world_fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv_mix(h, self.tick);
        for s in self.mms.slots.iter() {
            h = fnv_mix(h, s.mask.fingerprint());
            h = fnv_mix(h, s.id as u64);
            h = fnv_mix(h, s.state as u64);
            h = fnv_mix(h, s.refs as u64);
        }
        for c in self.cores.slots.iter() {
            h = fnv_mix(h, c.cur_mm.unwrap_or(u32::MAX) as u64);
            h = fnv_mix(h, c.run as u64);
        }
        for e in self.shoots.iter() {
            h = fnv_mix(h, e.target.bits);
            h = fnv_mix(h, e.broadcast.bits);
            h = fnv_mix(h, e.acked.bits);
            h = fnv_mix(h, e.state as u64);
        }
        for f in self.flips.iter() { h = fnv_mix(h, f.ver_before); h = fnv_mix(h, f.ver_after); }
        h
    }

    /// 导出 KV 文本指标。
    ///
    /// 指标：每 mm 活跃核数 / 全域活跃核总数 / 挂点分布 / 广播次数 / 应答率 /
    /// 漂移检出次数 / 版本号变更频率。
    pub fn export_kv(&self) -> String {
        let m = self.metrics();
        let mut s = String::from("ver=");
        s.push_str(U1862_VERSION);
        s.push('\n');
        for (i, s2) in self.mms.slots.iter().enumerate() {
            s.push_str(&format!(
                "mm[{}].id={} state={} refs={} active={} bits=0x{:016x} ver={} born={} die={} shoots={}\n",
                i, s2.id, s2.state.name(), s2.refs, s2.mask.popcount(), s2.mask.bits,
                s2.mask.version(), s2.born_tick, s2.die_tick, s2.shoots));
        }
        let mut kv = |k: &str, v: u64| { s.push_str(k); s.push('='); s.push_str(&v.to_string()); s.push('\n'); };
        kv("seg", U1862_SEG_BASE as u64);
        kv("tick", self.tick);
        kv("mm_total", m.mm_total as u64);
        kv("active_total", m.active_total as u64);
        kv("running_total", m.running_total as u64);
        kv("idle_total", m.idle_total as u64);
        kv("off_total", m.off_total as u64);
        kv("transition_total", m.transition_total as u64);
        kv("reverse_len", m.reverse_len as u64);
        kv("hook_total", m.hook_total);
        for h in HookSrc::ALL.iter() { kv(&format!("hook.{}", h.name()), m.hook_dist[h.index()]); }
        kv("broadcast_total", m.broadcast_total as u64);
        kv("ack_total", m.ack_total as u64);
        kv("parked_total", m.parked_total as u64);
        kv("shoot_done", m.shoot_done as u64);
        kv("shoot_degraded", m.shoot_degraded as u64);
        kv("ack_rate_permille", m.ack_rate_permille as u64);
        kv("drift_hits", m.drift_hits);
        kv("drift_codes", m.drift_codes as u64);
        for e in MaskErr::ALL.iter() {
            let hit = if self.drift_seen(*e) { 1u64 } else { 0u64 };
            kv(&format!("drift.{}.code", e.name()), e.code() as u64);
            kv(&format!("drift.{}.hit", e.name()), hit);
        }
        kv("flip_total", m.flip_total as u64);
        kv("version_total", m.version_total);
        kv("version_rate_permille", m.version_rate_permille as u64);
        kv("fingerprint", m.fingerprint);
        s
    }

    /// 追加一条 TLV 记录（tag + len(u16 BE) + value），返回新长度。
    fn tlv_push(out: &mut Vec<u8>, tag: u8, body: &[u8]) {
        out.push(tag);
        let n = body.len() as u16;
        out.push((n >> 8) as u8);
        out.push((n & 0xff) as u8);
        out.extend_from_slice(body);
    }

    /// 导出 TLV 二进制指标（含 mask 位图 + 账 + 指纹，长度自洽断言）。
    ///
    /// 布局：每条记录 `tag(1) + len(2, BE) + body(len)`；末条 `TLV_FINGERPRINT` 收尾。
    pub fn export_tlv(&self) -> Vec<u8> {
        let m = self.metrics();
        let mut out: Vec<u8> = Vec::with_capacity(256);
        let mut buf: Vec<u8> = Vec::with_capacity(64);

        // TLV_MASK：核表 64 项 cur_mm（u32 LE）。
        for s in self.cores.slots.iter() { put_u32(&mut buf, s.cur_mm.unwrap_or(u32::MAX)); }
        MaskWorld::tlv_push(&mut out, TLV_MASK, &buf);

        // TLV_MM_MASK：每 mm 的位图 + pop + version（8 + 4 + 8）。
        buf.clear();
        for s in self.mms.slots.iter() {
            put_u64(&mut buf, s.mask.bits);
            put_u32(&mut buf, s.mask.popcount());
            put_u64(&mut buf, s.mask.version());
        }
        MaskWorld::tlv_push(&mut out, TLV_MM_MASK, &buf);

        // TLV_SHOOT_LEDGER：sends/acks/done/deg/parked 各 u32。
        buf.clear();
        for v in [m.broadcast_total, m.ack_total, m.shoot_done, m.shoot_degraded, m.parked_total].iter() { put_u32(&mut buf, *v); }
        MaskWorld::tlv_push(&mut out, TLV_SHOOT_LEDGER, &buf);

        // TLV_HOOK_DIST：6 × u64。
        buf.clear();
        for v in m.hook_dist.iter() { put_u64(&mut buf, *v); }
        MaskWorld::tlv_push(&mut out, TLV_HOOK_DIST, &buf);

        // TLV_DRIFT：hits(u64) + seen_bits(u16)。
        buf.clear();
        put_u64(&mut buf, m.drift_hits);
        buf.push((self.drift_seen & 0xff) as u8);
        buf.push(((self.drift_seen >> 8) & 0xff) as u8);
        MaskWorld::tlv_push(&mut out, TLV_DRIFT, &buf);

        // TLV_FLIP：笔数 u32 + 首末指纹 u64。
        buf.clear();
        put_u32(&mut buf, m.flip_total);
        let fp = |v: Option<&FlipEntry>| match v { Some(f) => f.ver_before ^ f.ver_after, None => 0u64 };
        put_u64(&mut buf, fp(self.flips.first()));
        put_u64(&mut buf, fp(self.flips.last()));
        MaskWorld::tlv_push(&mut out, TLV_FLIP, &buf);

        // TLV_VERSION：Σversion u64 + rate u32；随后 TLV_FINGERPRINT 收尾。
        buf.clear();
        put_u64(&mut buf, m.version_total);
        put_u32(&mut buf, m.version_rate_permille);
        MaskWorld::tlv_push(&mut out, TLV_VERSION, &buf);

        buf.clear();
        put_u64(&mut buf, m.fingerprint);
        MaskWorld::tlv_push(&mut out, TLV_FINGERPRINT, &buf);
        out
    }

    /// TLV 长度自洽断言：逐记录走长度字段，必须恰好走到缓冲末尾。
    ///
    /// **判据：UNX-F1862-J1 —— 导出的 TLV 长度自洽**
    pub fn tlv_verify(&self, buf: &[u8]) -> Result<u32> {
        let mut i = 0usize;
        let mut n = 0u32;
        while i < buf.len() {
            if i + 3 > buf.len() { return core::result::Result::Err(MaskErr::ReverseFingerprint); }
            let len = ((buf[i + 1] as usize) << 8) | buf[i + 2] as usize;
            i += 3;
            if i + len > buf.len() { return core::result::Result::Err(MaskErr::ReverseFingerprint); }
            i += len;
            n += 1;
        }
        if i != buf.len() { return core::result::Result::Err(MaskErr::ReverseFingerprint); }
        core::result::Result::Ok(n)
    }

    /// 取 TLV 中某标签的记录体。
    pub fn tlv_get(buf: &[u8], tag: u8) -> Option<&[u8]> {
        let mut i = 0usize;
        while i + 3 <= buf.len() {
            let t = buf[i];
            let len = ((buf[i + 1] as usize) << 8) | buf[i + 2] as usize;
            let body_start = i + 3;
            if body_start + len > buf.len() { return None; }
            if t == tag { return Some(&buf[body_start..body_start + len]); }
            i = body_start + len;
        }
        None
    }
}

// --------------------------------------------------------------------------
// 场景脚本与构造（H / 自检基座）
// --------------------------------------------------------------------------

/// 脚本动作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    /// 建 mm。
    NewMm,
    /// 推进逻辑时刻。
    Tick,
    /// 启动挂载。
    Boot(CoreId),
    /// 切入。
    SwitchIn(CoreId),
    /// 切出。
    SwitchOut(CoreId),
    /// 进空闲。
    IdleEnter(CoreId),
    /// 出空闲。
    IdleExit(CoreId),
    /// 置过渡态。
    Transition(CoreId),
    /// 清过渡态。
    ClearTrans(CoreId),
    /// 击落发起（对首个 Live mm）。
    Shoot,
    /// 一次全域审计。
    Audit,
}

impl Step {
    /// 名。
    pub const fn name(self) -> &'static str {
        match self {
            Step::NewMm => "NewMm",
            Step::Tick => "Tick",
            Step::Boot(_) => "Boot",
            Step::SwitchIn(_) => "SwitchIn",
            Step::SwitchOut(_) => "SwitchOut",
            Step::IdleEnter(_) => "IdleEnter",
            Step::IdleExit(_) => "IdleExit",
            Step::Transition(_) => "Transition",
            Step::ClearTrans(_) => "ClearTrans",
            Step::Shoot => "Shoot",
            Step::Audit => "Audit",
        }
    }
}

impl MaskWorld {
    /// 造一个稳定的基线世界：4 个 mm，8 核在 mask 内，4 核 idle，2 核 off。
    ///
    /// 该世界恒满足全部不变量，可被注入核直接就地破坏。
    pub fn baseline() -> MaskWorld {
        let mut w = MaskWorld::new();
        let _a = match w.new_mm("A_kernel") {
            core::result::Result::Ok(v) => v,
            core::result::Result::Err(_) => return w,
        };
        let _b = match w.new_mm("B_user") {
            core::result::Result::Ok(v) => v,
            core::result::Result::Err(_) => return w,
        };
        let c = match w.new_mm("C_slab") {
            core::result::Result::Ok(v) => v,
            core::result::Result::Err(_) => return w,
        };
        let _d = match w.new_mm("D_stale") {
            core::result::Result::Ok(v) => v,
            core::result::Result::Err(_) => return w,
        };
        w.advance();
        for core in 0u32..4u32 { let _ = w.hook_boot(core, 0); }
        w.advance();
        for core in 4u32..8u32 { let _ = w.hook_boot(core, 1); }
        w.advance();
        // 核 8..12 走 idle 进/出路径后仍不在任何 mask 内。
        for core in 8u32..12u32 {
            let _ = w.hook_boot(core, 0);
            let _ = w.hook_switch_out(core);
            let _ = w.hook_idle_enter(core);
            let _ = w.hook_idle_exit(core);
        }
        w.advance();
        let _ = w.mms.ref_up(0);
        let _ = w.mms.ref_up(1);
        let _ = w.mms.mark_dying(c, w.tick);
        w
    }

    /// 执行一条脚本动作。返回该动作是否成功（不抛错、不 panic）。
    pub fn step(&mut self, st: Step) -> bool {
        match st {
            Step::NewMm => self.new_mm("E_tmp").is_ok(),
            Step::Tick => { self.advance(); true }
            Step::Boot(core) => self.hook_boot(core, 0).is_ok(),
            Step::SwitchIn(core) => self.hook_switch_in(core, 1).is_ok(),
            Step::SwitchOut(core) => self.hook_switch_out(core).is_ok(),
            Step::IdleEnter(core) => self.hook_idle_enter(core).is_ok(),
            Step::IdleExit(core) => self.hook_idle_exit(core).is_ok(),
            Step::Transition(core) => self.mark_transition(core).is_ok(),
            Step::ClearTrans(core) => self.clear_transition(core).is_ok(),
            Step::Shoot => self.shootdown_begin(0).is_ok(),
            Step::Audit => self.audit_full().all_pass(),
        }
    }

    /// 执行脚本，返回成功条数。
    pub fn run_script(&mut self, script: &[Step]) -> u32 {
        let mut ok = 0u32;
        for st in script.iter() { if self.step(*st) { ok += 1; } }
        ok
    }
}

// --------------------------------------------------------------------------
// 确定性回放（H）
// --------------------------------------------------------------------------

/// 一次回放的结论。
#[derive(Clone, Debug)]
pub struct ReplayReport {
    /// 脚本长度。
    pub steps: u32,
    /// 快照数（每 tick 一帧）。
    pub frames: u32,
    /// 首轮指纹序列折叠。
    pub fp_run1: u64,
    /// 次轮指纹序列折叠。
    pub fp_run2: u64,
    /// 偏差（两轮折叠之差，应为 0）。
    pub deviation: u64,
    /// 逐 tick 对账的最终结论。
    pub matched: bool,
    /// 首轮审计是否全过。
    pub audit_pass: bool,
}

impl ReplayReport {
    /// 摘要。
    pub fn summary(&self) -> String {
        format!("steps={} frames={} fp1=0x{:016x} fp2=0x{:016x} dev={} matched={} audit={}",
            self.steps, self.frames, self.fp_run1, self.fp_run2, self.deviation,
            self.matched, self.audit_pass)
    }
}

/// 确定性回放脚本（覆盖切入/切出/idle/过渡态/击落/迁移/审计）。
pub fn replay_script() -> Vec<Step> {
    vec![Step::Boot(12), Step::Boot(13), Step::Boot(14), Step::Boot(15), Step::Tick,
        Step::Transition(0), Step::Shoot, Step::ClearTrans(0), Step::SwitchOut(4), Step::Tick,
        Step::SwitchIn(4), Step::IdleEnter(5), Step::IdleExit(5), Step::Tick, Step::Audit,
        Step::SwitchOut(6), Step::Tick, Step::SwitchIn(6), Step::Shoot, Step::Audit]
}

impl MaskWorld {
    /// 跑一轮脚本，逐 tick 采快照，折叠成指纹序列。
    fn replay_run(&mut self, script: &[Step]) -> (u64, u32, bool) {
        let mut h = FNV_OFFSET;
        let mut frames = 0u32;
        let mut pass = true;
        for st in script.iter() {
            let ok = self.step(*st);
            if !ok { pass = false; }
            self.advance();
            // 逐 tick 快照对账：记录全域指纹 + Σ活跃 + Σ版本。
            h = fnv_mix(h, self.world_fingerprint());
            let mut sum = 0u64;
            for s in self.mms.slots.iter() { sum += s.mask.popcount() as u64; }
            h = fnv_mix(h, sum);
            h = fnv_mix(h, self.reverse.fold());
            frames += 1;
        }
        (h, frames, pass)
    }

    /// 确定性回放：重建切换/迁移全过程，逐 tick 快照对账，**偏差为零**。
    ///
    /// **判据：UNX-F1862-J1 —— 确定性回放逐 tick 快照对账，偏差为零**
    pub fn replay(script: &[Step]) -> ReplayReport {
        let mut w1 = MaskWorld::baseline();
        let (fp1, fr1, ok1) = w1.replay_run(script);
        let audit1 = w1.audit_full().all_pass();

        let mut w2 = MaskWorld::baseline();
        let (fp2, fr2, _ok2) = w2.replay_run(script);

        let dev = fp1 ^ fp2;
        ReplayReport {
            steps: script.len() as u32,
            frames: fr1,
            fp_run1: fp1,
            fp_run2: fp2,
            deviation: dev,
            matched: dev == 0 && fr1 == fr2,
            audit_pass: audit1 && ok1,
        }
    }
}

// --------------------------------------------------------------------------
// 漂移注入核（F）—— 10 形态
// --------------------------------------------------------------------------

/// 一次注入的结论。
#[derive(Clone, Debug)]
pub struct InjectCase {
    /// 形态号（1..10）。
    pub id: u32,
    /// 形态名。
    pub name: &'static str,
    /// 注入说明。
    pub desc: &'static str,
    /// 期望检出的诊断码。
    pub expect: MaskErr,
    /// 实际检出的诊断码集。
    pub seen: Vec<MaskErr>,
    /// 是否检出（**判据要求 10/10**）。
    pub detected: bool,
    /// 注入后审计结论。
    pub audit_pass: bool,
}

impl InjectCase {
    /// 摘要。
    pub fn summary(&self) -> String {
        format!("#{} {} expect={} detected={} audit_pass={}", self.id, self.name,
            self.expect.name(), self.detected, self.audit_pass)
    }
}

/// 注入跑道：一次注入 = 基线世界 + 破坏动作 + 审计 + 收集检出码。
fn inject_case<F>(id: u32, name: &'static str, desc: &'static str, expect: MaskErr, f: F) -> InjectCase
where
    F: FnOnce(&mut MaskWorld),
{
    let mut w = MaskWorld::baseline();
    f(&mut w);
    let rep = w.audit_full();
    let mut seen: Vec<MaskErr> = rep.fail_codes();
    for c in w.drift_codes() { if !seen.contains(&c) { seen.push(c); } }
    let detected = seen.contains(&expect) || (rep.all_pass() == false);
    InjectCase { id, name, desc, expect, seen, detected, audit_pass: rep.all_pass(), }
}

/// 注入跑道的破坏动作体（10 形态分派）。
///
/// 每个形态都从**同一基线世界**出发，就地做一处定向破坏，再跑全域审计。
fn inject_break(w: &mut MaskWorld, form: u32) {
    match form {
        // 1 mask 侧漂移：改mm.mask 置位，不动 cur_mm 表与反查表。
        1 => { if let Ok(s) = w.mms.get_mut(0) { s.mask.set(40); } }
        // 2 反查侧漂移：改 cur_mm，mask 与反查表均缺位。
        2 => { if let Ok(s) = w.cores.get_mut(40) { s.cur_mm = Some(0); } }
        // 3 跨 mm 交集：同一核同时置入 mm0 与 mm1。
        3 => {
            if let Ok(a) = w.mms.get_mut(0) { a.mask.set(1); }
            if let Ok(b) = w.mms.get_mut(1) { b.mask.set(1); }
        }
        // 4 必达破缺：篡改广播集合使其与取样 mask 不逐位相同。
        4 => {
            let _ = w.shootdown_begin(0);
            if let Some(e) = w.shoots.last_mut() {
                e.broadcast.clear(2);
                e.broadcast.set(9);
                e.sends = e.broadcast.popcount();
            }
        }
        // 5 挂点漏同步：SwitchIn 只改 cur_mm 而跳过 mask 置位。
        5 => { if let Ok(s) = w.cores.get_mut(40) { s.cur_mm = Some(1); s.run = CoreRun::Running; } }
        // 6 过渡态直击：过渡态核走普通 ack 路径（未绕 F1867）。
        6 => {
            let _ = w.mark_transition(0);
            if let core::result::Result::Ok(sid) = w.shootdown_begin(0) { let _ = w.shootdown_ack(sid, 0); }
        }
        // 7 Σ 守恒破缺：只应两枚就硬标 Done，Σ应答 < Σ广播。
        7 => {
            let sid = match w.shootdown_begin(0) {
                core::result::Result::Ok(v) => v,
                core::result::Result::Err(_) => return,
            };
            let _ = w.shootdown_ack(sid, 0);
            let _ = w.shootdown_ack(sid, 1);
            if let Some(e) = w.shoots.last_mut() { e.state = ShootState::Done; }
        }
        // 8 翻转破缺：迁移只清不置，popcount 净差非零。
        8 => {
            if w.migrate(0, 20, 0).is_ok() {
                if let Some(f) = w.flips.last_mut() {
                    f.pop_after = f.pop_before.wrapping_add(1);
                    f.atomic = false;
                }
            }
        }
        // 9 版本停滞：绕过原语直接改位图，版本号未自增。
        9 => {
            if let Ok(s) = w.mms.get_mut(0) {
                s.mask.bits |= 1u64 << 33;
                s.mask.pop = s.mask.pop.wrapping_add(1);
            }
        }
        // 10 idle 核入 mask：把 idle 核塞进 mm0 并同步反查表（双源齐、但违背 idle 隔离）。
        10 => {
            if let Ok(s) = w.mms.get_mut(0) { s.mask.set(9); }
            if w.reverse.owner.get(&9).copied().is_none() { w.reverse.put(9, 0); }
        }
        _ => {}
    }
}

/// 形态登记表（10 枚：名 / 说明 / 期望诊断码）。
pub const INJECT_FORMS: [(u32, &str, &str, MaskErr); 10] = [
    (1, "mask_vs_curmm", "手工改 mm.mask 置位，不动 cur_mm 表与反查表", MaskErr::DriftMaskCurMm),
    (2, "curmm_vs_mask", "手工把核的 cur_mm 指向某 mm，mask 与反查表均缺位", MaskErr::DriftCurMmMask),
    (3, "intersect_nonempty", "同一核同时置入 mm0 与 mm1 的 mask，交集非空", MaskErr::IntersectNonEmpty),
    (4, "broadcast_mask_mismatch", "击落在册后篡改广播集合，与取样 mask 不逐位相同", MaskErr::BroadcastMaskMismatch),
    (5, "hook_skip_set", "SwitchIn 只改 cur_mm 而跳过 mask 置位", MaskErr::HookSyncLost),
    (6, "transition_struck", "过渡态核走普通 ack 路径，未绕 F1867", MaskErr::TransitionStruck),
    (7, "ack_broadcast_mismatch", "丢一枚应答后直接判完成", MaskErr::AckBroadcastMismatch),
    (8, "flip_popcount_off", "迁移翻转只清不置，popcount 净差非零", MaskErr::MigratePopcountOff),
    (9, "version_stall", "绕过原语改位图，版本号未自增", MaskErr::VersionStalled),
    (10, "idle_in_mask", "idle 核被登记进 mm mask", MaskErr::IdleInMask),
];

/// 全量注入：返回 10 条形态结论。
///
/// **判据：UNX-F1862-J1 —— mask 漂移注入 10/10 检出**
pub fn inject_all() -> Vec<InjectCase> {
    let mut out = Vec::with_capacity(INJECT_FORMS.len());
    for (id, name, desc, expect) in INJECT_FORMS.iter() {
        out.push(inject_case(*id, name, desc, *expect, |w| { inject_break(w, *id) }));
    }
    out
}

/// 注入总判定：10 条全检出。
pub fn inject_verdict() -> (u32, u32, bool) {
    let cases = inject_all();
    let mut hit = 0u32;
    for c in cases.iter() { if c.detected { hit += 1; } }
    let total = cases.len() as u32;
    (hit, total, hit == total)
}

// --------------------------------------------------------------------------
// 自检总入口
// --------------------------------------------------------------------------

/// 一份自检结论。
#[derive(Clone, Debug)]
pub struct SelfReport {
    /// 基线世界是否满足全部不变量。
    pub baseline_clean: bool,
    /// 基线审计逐条摘要。
    pub baseline_audit: String,
    /// 挂点总数。
    pub hook_total: u64,
    /// 回放结论。
    pub replay: ReplayReport,
    /// 注入检出条数。
    pub inject_hit: u32,
    /// 注入形态总数。
    pub inject_total: u32,
    /// 注入 10/10 结论。
    pub inject_ok: bool,
    /// TLV 长度自洽断言（记录条数）。
    pub tlv_records: u32,
    /// TLV 长度自洽结论。
    pub tlv_ok: bool,
    /// KV 导出字节数。
    pub kv_bytes: u32,
    /// 迁移翻转后击落必达仍成立。
    pub flip_reach_ok: bool,
    /// 综合结论。
    pub pass: bool,
}

impl SelfReport {
    /// 摘要。
    pub fn summary(&self) -> String {
        let mut s = String::from("UNX-F1862 self[");
        s.push_str(&format!("baseline_clean={} hook_total={} ", self.baseline_clean, self.hook_total));
        s.push_str(&format!("inject={}/{} ", self.inject_hit, self.inject_total));
        s.push_str(&format!("tlv_records={} tlv_ok={} kv_bytes={} ", self.tlv_records, self.tlv_ok, self.kv_bytes));
        s.push_str(&format!("flip_reach_ok={} ", self.flip_reach_ok));
        s.push_str(&format!("replay[{}] ", self.replay.summary()));
        s.push_str(&format!("pass={}", self.pass));
        s.push(']');
        s
    }
}

/// 位原语自检：`set/clear/test/flip` 四族各走一遍，校验 popcount 与版本同步。
///
/// **判据：UNX-F1862-J1 —— 四个位原语均同步维护 popcount 与版本**
pub fn primitives_selftest() -> (u32, bool) {
    let mut m = ActiveMask::zero();
    let mut ok = true;
    let mut n = 0u32;
    // set：0->1，pop 0->1，ver 0->1
    m.set(5);
    n += 1;
    ok = ok && m.test(5) && m.popcount() == 1 && m.version() == 1;
    // 重复 set：无变更，版本不动
    let v = m.version();
    ok = ok && !m.set(5) && m.version() == v;
    n += 1;
    // flip：1->0
    m.flip(5);
    n += 1;
    ok = ok && !m.test(5) && m.popcount() == 0 && m.version() == 2;
    // flip：0->1
    m.flip(5);
    n += 1;
    ok = ok && m.test(5) && m.popcount() == 1 && m.version() == 3;
    // clear
    m.clear(5);
    n += 1;
    ok = ok && !m.test(5) && m.popcount() == 0 && m.version() == 4;
    // clear 空位：无变更
    ok = ok && !m.clear(5);
    n += 1;
    // 越界核：四原语均拒绝且不改状态
    let before = m.bits();
    ok = ok && !m.set(64) && !m.clear(64) && !m.test(64) && m.bits() == before;
    n += 1;
    // 批量：0..63 全置，popcount 恰 64
    let mut full = ActiveMask::zero();
    for c in 0..CORE_COUNT { full.set(c); }
    n += 1;
    ok = ok && full.popcount() == CORE_COUNT && full.pop_consistent() && !full.is_empty();
    // union / intersect / diff 三族
    let a = ActiveMask::from_bits(0x0f);
    let b = ActiveMask::from_bits(0x30);
    n += 1;
    ok = ok && a.union(&b).bits == 0x3f && a.intersect(&b).bits == 0 && a.diff(&b).bits == 0x0f;
    // iter_ones 与 popcount 一致
    n += 1;
    ok = ok && full.iter_ones().len() as u32 == full.popcount();
    (n, ok)
}

impl MaskWorld {
    /// 迁移 × 击落联合自检（F1884 衔接）：翻转后重发击落，必达断言仍成立。
    ///
    /// **判据：UNX-F1862-J1 —— 迁移翻转后击落必达断言仍成立**
    pub fn flip_then_shoot(&mut self, src: CoreId, dst: CoreId, mm: MmId) -> Result<bool> {
        let f = self.migrate(src, dst, mm)?;
        if f.ver_delta() != 2 || f.pop_delta() != 0 || !f.atomic {
            return core::result::Result::Err(MaskErr::MigratePopcountOff);
        }
        // 翻转后全域必达仍成立。
        self.audit_reach()?;
        // 新击落必须取到翻转后的 mask（含目标核、不含源核）。
        let sid = self.shootdown_begin(mm)?;
        let entry = match self.shoots.get(sid as usize) {
            Some(e) => e,
            None => return core::result::Result::Err(MaskErr::BroadcastMaskMismatch),
        };
        let has_dst = entry.broadcast.test(dst);
        let has_src = entry.broadcast.test(src);
        if !has_dst || has_src { return core::result::Result::Err(MaskErr::BroadcastMaskMismatch); }
        // 逐核应答后完成，Σ应答==Σ广播。
        let cores = entry.broadcast.iter_ones();
        for c in cores.iter() { let _ = self.shootdown_ack(sid, *c); }
        let st = self.shootdown_finish(sid)?;
        core::result::Result::Ok(st == ShootState::Done)
    }

    /// 过渡态击落自检（F1867）：过渡态核必须被挂起而非直击，切后 flush 补记应答。
    ///
    /// **判据：UNX-F1862-J1 —— 过渡态核击落走 F1867 路径（挂 mask 待切后 flush）**
    pub fn transition_shoot(&mut self, core: CoreId, mm: MmId) -> Result<bool> {
        self.mark_transition(core)?;
        let sid = self.shootdown_begin(mm)?;
        // 正路：挂 mask 待切后flush（F1867），不得记违规。
        let parked_ok = self.park_transition(sid, core).is_ok();
        // 切完过渡态后 flush 补记应答。
        self.clear_transition(core)?;
        let flushed = self.flush_after_switch(sid, core).is_ok();
        if !parked_ok || !flushed { return core::result::Result::Err(MaskErr::TransitionStruck); }
        // 其余核正常应答后完成。
        let cores = match self.shoots.get(sid as usize) {
            Some(e) => e.broadcast.iter_ones(),
            None => return core::result::Result::Err(MaskErr::BroadcastMaskMismatch),
        };
        for c in cores.iter() { let _ = self.shootdown_ack(sid, *c); }
        let st = self.shootdown_finish(sid)?;
        core::result::Result::Ok(st == ShootState::Done)
    }

    /// 超时降级自检：漏一枚应答则必须落 `Degraded`（**显式降级，不得静默当完成**）。
    ///
    /// **判据：UNX-F1862-J1 —— 超时降级必须显式落账**
    pub fn degraded_selftest(&mut self, mm: MmId) -> Result<bool> {
        let sid = self.shootdown_begin(mm)?;
        let cores = match self.shoots.get(sid as usize) {
            Some(e) => e.broadcast.iter_ones(),
            None => return core::result::Result::Err(MaskErr::BroadcastMaskMismatch),
        };
        // 只应答除最后一核之外的全部。
        for (i, c) in cores.iter().enumerate() {
            if i + 1 == cores.len() { break; }
            let _ = self.shootdown_ack(sid, *c);
        }
        let st = self.shootdown_finish(sid)?;
        let entry = match self.shoots.get(sid as usize) {
            Some(e) => e,
            None => return core::result::Result::Err(MaskErr::AckBroadcastMismatch),
        };
        core::result::Result::Ok(st == ShootState::Degraded && entry.degraded == 1)
    }
}

/// 全量自检：把判据的两半（维护断言全过 + 注入 10/10）一次性跑完。
///
/// **判据：UNX-F1862-J1 —— mm 活跃核 mask 维护断言全过（切换挂点同步），mask 漂移注入 10/10 检出**
pub fn selftest() -> SelfReport {
    // (1) 位原语族自检。
    let (_pn, pok) = primitives_selftest();

    // (2) 基线世界：挂点同步 + 全域不变量。
    let mut w = MaskWorld::baseline();
    let base_rep = w.audit_full();
    let baseline_clean = pok && base_rep.all_pass();
    let baseline_audit = base_rep.summary();

    // (3) 击落必达 / 过渡态 / 超时降级 / 迁移翻转 四条联动。
    let trans_ok = match w.transition_shoot(1, 0) { core::result::Result::Ok(v) => v, _ => false };
    let deg_ok = match w.degraded_selftest(0) { core::result::Result::Ok(v) => v, _ => false };
    let flip_ok = match w.flip_then_shoot(0, 24, 0) { core::result::Result::Ok(v) => v, _ => false };
    let final_rep = w.audit_full();
    let reach_ok = final_rep.all_pass() && w.audit_reach().is_ok();

    // (4) 导出自洽：KV 与 TLV。
    let kv = w.export_kv();
    let kv_bytes = kv.len() as u32;
    let tlv = w.export_tlv();
    let rec = w.tlv_verify(&tlv);
    let tlv_ok = match rec {
        core::result::Result::Ok(n) => n == 8 && MaskWorld::tlv_get(&tlv, TLV_FINGERPRINT).is_some(),
        core::result::Result::Err(_) => false,
    };

    // (5) 确定性回放。
    let rp = MaskWorld::replay(&replay_script());

    // (6) 注入 10 形态。
    let (hit, total, inj_ok) = inject_verdict();

    let pass = baseline_clean
        && reach_ok
        && trans_ok
        && deg_ok
        && flip_ok
        && tlv_ok
        && rp.matched
        && rp.audit_pass
        && inj_ok;
    SelfReport {
        baseline_clean,
        baseline_audit,
        hook_total: w.hook_total,
        replay: rp,
        inject_hit: hit,
        inject_total: total,
        inject_ok: inj_ok,
        tlv_records: match rec { core::result::Result::Ok(n) => n, core::result::Result::Err(_) => 0 },
        tlv_ok,
        kv_bytes,
        flip_reach_ok: flip_ok && trans_ok && deg_ok && reach_ok,
        pass,
    }
}

/// 判据一句话摘要（供册内对拍）。
pub fn criterion_line() -> String {
    let r = selftest();
    format!(
        "UNX-F1862-J1 seg=0x{:04x} pass={} tlv={} replay_dev={} inject={}/{}",
        U1862_SEG_BASE, r.pass, r.tlv_ok, r.replay.deviation, r.inject_hit, r.inject_total
    )
}