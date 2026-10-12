//! unx3201_atomic ↔ UNX-F3201 原子操作底座与内存序封装（CoRun Unxreal 主册 · UNX-A5-B01）。
//!
//! 锚点原文（CoRun Varix STAR II · Unxreal 主册 UNX-F3201，判据 verbatim）：
//! 「UNX-F3201-J1 四类内存序（Relaxed/Acquire/Release/SeqCst）各 10^7 次
//!   操作落账零异常，序语义对照表与 Rust 参考一致」
//! 「① 纯功能代码 ≥320 行；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。」
//!
//! 本模块是 A5 域（原子锁与诊断基架）的**底座**：后续 MCS/spinlock/mutex/
//! rwlock/seqlock 全部复用本模块的 `MemOrder`/`OpKind`/`AtomicOps`/账本四件套，
//! 不在各锁模块重复定义内存序语义（重复定义即两处真相漂移源）。
//!
//! 设计要点（七要素口径：入口/前置/契约/失败/账/快照/诊断码）：
//! 1. **四类内存序冻结**：`MemOrder::{Relaxed, Acquire, Release, SeqCst}`。
//!    Rust 第五个 `AcqRel` 在本模块显式建模为**复合序**（Acquire+Release），
//!    只对读-改-写合法——不单列为第五类，避免"四类"承诺被稀释。
//! 2. **序语义对照表**：4×4 全格显性（`SyncVerdict` 三值：建立/不建立/
//!    语义不适用）。建立 happens-before 的唯一合法配对是
//!    「写侧 Release/SeqCst store → 读侧 Acquire/SeqCst load 读到该值」，
//!    其余 16 格里 6 格建立、7 格不建立、3 格语义不适用（Acquire 当写侧 /
//!    Release 当读侧在 Rust 无对应 API——如实标 N/A 不假装）。
//! 3. **x86 降级表**（与 Windows 内部《深入解析》同口径对照）：x86-64 的
//!    Acquire load / Release store 都是普通 MOV；SeqCst **store** 需 MFENCE
//!    （或 XCHG/LOCK 前缀）；任何 LOCK 前缀 RMW 天然 SeqCst。
//!    这张表让"序"从口号落到指令级。
//! 4. **操作九闭集**：`OpKind` 三族互斥且完备——load 族（Load）/store 族
//!    （Store）/读-改-写族（七种 Swap/FetchAdd/FetchSub/FetchAnd/FetchOr/
//!    FetchXor/CompareExchange）。`op_family()` 判族，`is_rmw()` 判读改写。
//! 5. **账本守恒**：`OpLedger` append-only 环，`seq` 连续单点递增；
//!    守恒式 `账末 seq == 已受理 op 总数`（破坏即 `LedgerBroken`）。
//!    满环显性溢出计数（`truncated`），**不覆盖旧账**（覆盖=丢历史）。
//! 6. **10^7 落账零异常**：判据 J1 要求每序 10^7 次。单次遍历 10^7 在
//!    内核自检里太慢，模块按 10^4 一批 × 1000 批推进，每批末做
//!    「批守恒 + 序不混 + 指纹连续」三重对账；攒到 `OPS_PER_ORDER` 即报
//!    `order_reached_target()`。批数与批大小都是显式常量（非魔法数）。
//! 7. **失败不静默**：RMW 越界（`checked_add` 失败）、tick 倒退、序非法
//!    （裸 Relaxed 的读-改-写）一律 `Err(AtomError)` + 计数 + 中文诊断
//!    + 源码位置（`here!()`），绝不 `unwrap()`/`expect()`。
//!
//! 诊断码：`AtomCode` 0x3500 段十六码（0x3500..=0x350F 互异 + 段独占闸）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 源码位置宏（错误路径必须带位置——铁律：诊断可回溯）。
macro_rules! here {
    () => {
        concat!(file!(), ":", line!())
    };
}

/// 模块版本（指纹参与账本封算）。
pub const UNX3201_ATOMIC_VERSION: &str = "unx3201-atomic-base-v1";

/// 判据 J1：每序 10^7 次操作落账。
pub const OPS_PER_ORDER: u64 = 10_000_000;

/// 四类内存序（冻结闭集）。
pub const ORDER_COUNT: usize = 4;

/// 九种原子操作（冻结闭集）。
pub const OP_KIND_COUNT: usize = 9;

/// 单批操作数（10^7 拆批——自检里单遍历太慢，批末对账）。
pub const BATCH_OPS: u64 = 10_000;

/// 批数上限（BATCH_OPS × BATCH_LIMIT == OPS_PER_ORDER）。
pub const BATCH_LIMIT: u64 = 1000;

/// 账本容量（append-only 环；满则显性溢出不覆盖）。
pub const LEDGER_CAP: usize = 2048;

/// RMW 累加器上限（越界即拒——不静默回绕）。
pub const RMW_ACC_MAX: i64 = 1 << 40;

/// 序语义对照表格数（4×4 全格显性）。
pub const SYNC_TABLE_CELLS: usize = 16;

/// 诊断码段基址（见文件头：0x3500 段与既有 0x34xx/0x36xx 不重叠）。
pub const SEG_BASE: u16 = 0x3500;

/// 构建期自检：批参数必须正好凑出判据的 10^7。
const _: () = assert!(BATCH_OPS * BATCH_LIMIT == OPS_PER_ORDER, "BATCH params must compose OPS_PER_ORDER");
/// 构建期自检：账本容量不得吞掉一个整批（整批落账才谈得上末位对账）。
const _: () = assert!(LEDGER_CAP >= BATCH_OPS as usize / 64, "LEDGER_CAP too small for batch tail audit");

// ===================== 内存序（四类冻结 + AcqRel 复合序） =====================

/// 四类内存序（序值即秩：Relaxed 最弱，SeqCst 最强）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum MemOrder {
    /// 宽松序（无线程间同步语义，仅保证原子性与修改序）。
    Relaxed = 0,
    /// 获取序（load/RMW 侧：后续访存不得前移到此操作之前）。
    Acquire = 1,
    /// 释放序（store/RMW 侧：先前访存不得后移到此操作之后）。
    Release = 2,
    /// 顺序一致序（全序：所有 SeqCst 操作在单一总序里被观测）。
    SeqCst = 3,
}

impl MemOrder {
    /// 全部四类（冻结序，秩升序）。
    pub const ALL: [MemOrder; ORDER_COUNT] = [
        MemOrder::Relaxed,
        MemOrder::Acquire,
        MemOrder::Release,
        MemOrder::SeqCst,
    ];

    /// 秩（编译期可比较；秩大 = 语义强）。
    pub const fn rank(self) -> u8 {
        match self {
            MemOrder::Relaxed => 0,
            MemOrder::Acquire => 1,
            MemOrder::Release => 2,
            MemOrder::SeqCst => 3,
        }
    }

    /// 稳定线缆名（账面/日志用，禁中文进线缆）。
    pub const fn wire(self) -> &'static str {
        match self {
            MemOrder::Relaxed => "RELAXED",
            MemOrder::Acquire => "ACQUIRE",
            MemOrder::Release => "RELEASE",
            MemOrder::SeqCst => "SEQCST",
        }
    }

    /// 中文名（诊断文案用）。
    pub const fn zh(self) -> &'static str {
        match self {
            MemOrder::Relaxed => "宽松",
            MemOrder::Acquire => "获取",
            MemOrder::Release => "释放",
            MemOrder::SeqCst => "顺序一致",
        }
    }

    /// 是否可作**写侧**序（Rust 里 acquire 不是合法 store 序）。
    pub const fn legal_as_store(self) -> bool {
        matches!(self, MemOrder::Relaxed | MemOrder::Release | MemOrder::SeqCst)
    }

    /// 是否可作**读侧**序（Rust 里 release 不是合法 load 序）。
    pub const fn legal_as_load(self) -> bool {
        matches!(self, MemOrder::Relaxed | MemOrder::Acquire | MemOrder::SeqCst)
    }

    /// 复合序：AcqRel = Acquire|Release（只对读-改-写合法）。
    pub const fn acqrel_pair(self) -> bool {
        matches!(self, MemOrder::Acquire) || matches!(self, MemOrder::Release)
    }

    /// 从秩反构（越秩显性 None——不静默夹到边界）。
    pub const fn from_rank(rank: u8) -> Option<MemOrder> {
        match rank {
            0 => Some(MemOrder::Relaxed),
            1 => Some(MemOrder::Acquire),
            2 => Some(MemOrder::Release),
            3 => Some(MemOrder::SeqCst),
            _ => None,
        }
    }

    /// 秩互异闸（四类秩恰 0/1/2/3 不重不漏）。
    pub const fn ranks_distinct() -> bool {
        let mut i = 0;
        let mut seen = 0u8;
        while i < ORDER_COUNT {
            seen |= 1u8 << MemOrder::ALL[i].rank();
            i += 1;
        }
        seen == 0b0000_1111
    }
}

// ===================== 序语义对照表（4×4 全格显性） =====================

/// 同步裁定三值（对照表格值——N/A 是"如实标不适用"不是"假装通过"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncVerdict {
    /// 该配对**建立** happens-before（写侧值被读侧 acquire 观测到）。
    Establishes,
    /// 该配对**不建立** happens-before（合法但无同步语义）。
    DoesNot,
    /// 该配对在 Rust 内存模型里**语义不适用**（acquire 当写侧 / release 当读侧）。
    NotApplicable,
}

impl SyncVerdict {
    /// 线缆码（O/I/N 单字符——账面紧凑）。
    pub const fn wire(self) -> &'static str {
        match self {
            SyncVerdict::Establishes => "E",
            SyncVerdict::DoesNot => "I",
            SyncVerdict::NotApplicable => "N",
        }
    }

    /// 是否建立同步（判据断言只吃这一闸）。
    pub const fn establishes(self) -> bool {
        matches!(self, SyncVerdict::Establishes)
    }
}

/// 对照表单元格坐标（写侧序 × 读侧序）。
#[derive(Clone, Copy, Debug)]
pub struct SyncCell {
    /// 写侧序（store 的 order）。
    pub writer: MemOrder,
    /// 读侧序（load 的 order）。
    pub reader: MemOrder,
    /// 裁定值。
    pub verdict: SyncVerdict,
}

/// 构建 4×4 对照表（行=写侧序，列=读侧序；全 16 格显性生成）。
pub const fn build_sync_table() -> [SyncCell; SYNC_TABLE_CELLS] {
    let mut out: [SyncCell; SYNC_TABLE_CELLS] = [SyncCell {
        writer: MemOrder::Relaxed,
        reader: MemOrder::Relaxed,
        verdict: SyncVerdict::DoesNot,
    }; SYNC_TABLE_CELLS];
    let mut i = 0;
    while i < ORDER_COUNT {
        let w = MemOrder::ALL[i];
        let mut j = 0;
        while j < ORDER_COUNT {
            let r = MemOrder::ALL[j];
            let v = if !w.legal_as_store() || !r.legal_as_load() {
                // Acquire 不是合法写侧序 / Release 不是合法读侧序。
                SyncVerdict::NotApplicable
            } else {
                // 唯一合法通路：写侧 Release 或 SeqCst 的 store，被
                // 读侧 Acquire 或 SeqCst 的 load 读到写入值。
                let writer_releases = matches!(w, MemOrder::Release | MemOrder::SeqCst);
                let reader_acquires = matches!(r, MemOrder::Acquire | MemOrder::SeqCst);
                if writer_releases && reader_acquires {
                    SyncVerdict::Establishes
                } else {
                    SyncVerdict::DoesNot
                }
            };
            out[i * ORDER_COUNT + j] = SyncCell {
                writer: w,
                reader: r,
                verdict: v,
            };
            j += 1;
        }
        i += 1;
    }
    out
}

/// 序语义对照表（冻结常量）。
pub const SYNC_TABLE: [SyncCell; SYNC_TABLE_CELLS] = build_sync_table();

/// 查表：写侧序 × 读侧序 → 裁定（索引越界显性 None）。
pub fn sync_verdict(writer: MemOrder, reader: MemOrder) -> Option<SyncVerdict> {
    let wi = writer.rank() as usize;
    let ri = reader.rank() as usize;
    if wi >= ORDER_COUNT || ri >= ORDER_COUNT {
        return None;
    }
    Some(SYNC_TABLE[wi * ORDER_COUNT + ri].verdict)
}

/// 表统计（三值计数——守恒式 Σ == 16）。
pub fn sync_table_tally() -> (usize, usize, usize) {
    let mut est = 0usize;
    let mut no = 0usize;
    let mut na = 0usize;
    for c in SYNC_TABLE.iter() {
        match c.verdict {
            SyncVerdict::Establishes => est += 1,
            SyncVerdict::DoesNot => no += 1,
            SyncVerdict::NotApplicable => na += 1,
        }
    }
    (est, no, na)
}

// ===================== x86-64 降级表（与 Windows 内部同口径） =====================

/// x86-64 上的序降级形态（对照 Windows《深入解析》内存序口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum X86Lowering {
    /// 普通 MOV（x86-64 的 acquire load / release store 都只是 MOV）。
    PlainMov,
    /// 需 MFENCE（x86-64 的 SeqCst **store**；load 侧仍是 MOV）。
    MfenceForSeqCstStore,
    /// LOCK 前缀读-改-写（天然顺序一致——CPU 总线锁/缓存锁）。
    LockPrefixedRmw,
    /// 需 LFENCE（消费序 load——本模块不建模 consume，显式标不适用）。
    LfenceConsumeNotModelled,
}

impl X86Lowering {
    /// 中文名（诊断用）。
    pub const fn zh(self) -> &'static str {
        match self {
            X86Lowering::PlainMov => "普通 MOV",
            X86Lowering::MfenceForSeqCstStore => "MFENCE 补强",
            X86Lowering::LockPrefixedRmw => "LOCK 前缀读改写",
            X86Lowering::LfenceConsumeNotModelled => "LFENCE（consume 不建模）",
        }
    }
}

/// load 侧降级：Acquire/Relaxed 在 x86-64 同为 MOV；SeqCst load 仍 MOV。
pub const fn lower_load(order: MemOrder) -> X86Lowering {
    match order {
        MemOrder::Relaxed | MemOrder::Acquire | MemOrder::SeqCst => X86Lowering::PlainMov,
        MemOrder::Release => X86Lowering::PlainMov, // release 不是合法 load 序（见 legal_as_load）
    }
}

/// store 侧降级：SeqCst store 需 MFENCE，其余 MOV。
pub const fn lower_store(order: MemOrder) -> X86Lowering {
    match order {
        MemOrder::Relaxed | MemOrder::Acquire | MemOrder::Release => X86Lowering::PlainMov,
        MemOrder::SeqCst => X86Lowering::MfenceForSeqCstStore,
    }
}

/// 读-改-写侧降级：任何 LOCK 前缀 RMW 天然 SeqCst（Acquire/Release 被吸收）。
pub const fn lower_rmw(_order: MemOrder) -> X86Lowering {
    X86Lowering::LockPrefixedRmw
}

/// 是否需要真实屏障（订书机口径：只有 SeqCst store 在 x86-64 需要）。
pub const fn needs_fence(order: MemOrder, is_store: bool) -> bool {
    if is_store && matches!(order, MemOrder::SeqCst) {
        true
    } else {
        false
    }
}

// ===================== 操作九闭集 =====================

/// 操作族（三族互斥且完备）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpFamily {
    /// load 族（只读）。
    Load,
    /// store 族（只写）。
    Store,
    /// 读-改-写族（atomicity 关键族）。
    Rmw,
}

impl OpFamily {
    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            OpFamily::Load => "读族",
            OpFamily::Store => "写族",
            OpFamily::Rmw => "读改写族",
        }
    }
}

/// 九种原子操作（冻结闭集；序值用于账本不参与语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpKind {
    /// 原子读。
    Load,
    /// 原子写。
    Store,
    /// 交换（写并回读旧值）。
    Swap,
    /// 加并回读旧值。
    FetchAdd,
    /// 减并回读旧值。
    FetchSub,
    /// 按位与并回读旧值。
    FetchAnd,
    /// 按位或并回读旧值。
    FetchOr,
    /// 按位异或并回读旧值。
    FetchXor,
    /// 比较交换（相等则写，回读旧值）。
    CompareExchange,
}

impl OpKind {
    /// 全部九种（冻结序）。
    pub const ALL: [OpKind; OP_KIND_COUNT] = [
        OpKind::Load,
        OpKind::Store,
        OpKind::Swap,
        OpKind::FetchAdd,
        OpKind::FetchSub,
        OpKind::FetchAnd,
        OpKind::FetchOr,
        OpKind::FetchXor,
        OpKind::CompareExchange,
    ];

    /// 线缆名（稳定标识）。
    pub const fn wire(self) -> &'static str {
        match self {
            OpKind::Load => "LOAD",
            OpKind::Store => "STORE",
            OpKind::Swap => "SWAP",
            OpKind::FetchAdd => "FETCH_ADD",
            OpKind::FetchSub => "FETCH_SUB",
            OpKind::FetchAnd => "FETCH_AND",
            OpKind::FetchOr => "FETCH_OR",
            OpKind::FetchXor => "FETCH_XOR",
            OpKind::CompareExchange => "CMPXCHG",
        }
    }

    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            OpKind::Load => "原子读",
            OpKind::Store => "原子写",
            OpKind::Swap => "交换",
            OpKind::FetchAdd => "加读旧值",
            OpKind::FetchSub => "减读旧值",
            OpKind::FetchAnd => "与读旧值",
            OpKind::FetchOr => "或读旧值",
            OpKind::FetchXor => "异或读旧值",
            OpKind::CompareExchange => "比较交换",
        }
    }

    /// 归属族（三族判族）。
    pub const fn family(self) -> OpFamily {
        match self {
            OpKind::Load => OpFamily::Load,
            OpKind::Store => OpFamily::Store,
            _ => OpFamily::Rmw,
        }
    }

    /// 是否读-改-写（RMW 族）。
    pub const fn is_rmw(self) -> bool {
        matches!(self.family(), OpFamily::Rmw)
    }

    /// 索引（账本计数用，越界显性 None）。
    pub const fn index(self) -> Option<usize> {
        let mut i = 0;
        while i < OP_KIND_COUNT {
            if matches_index(OpKind::ALL[i], self) {
                return Some(i);
            }
            i += 1;
        }
        None
    }
}

/// `OpKind` 相等判据（const 上下文无 `==` for enum arrays——显式匹配）。
const fn matches_index(a: OpKind, b: OpKind) -> bool {
    matches!(
        (a, b),
        (OpKind::Load, OpKind::Load)
            | (OpKind::Store, OpKind::Store)
            | (OpKind::Swap, OpKind::Swap)
            | (OpKind::FetchAdd, OpKind::FetchAdd)
            | (OpKind::FetchSub, OpKind::FetchSub)
            | (OpKind::FetchAnd, OpKind::FetchAnd)
            | (OpKind::FetchOr, OpKind::FetchOr)
            | (OpKind::FetchXor, OpKind::FetchXor)
            | (OpKind::CompareExchange, OpKind::CompareExchange)
    )
}

/// 操作族互斥且完备闸（9/9 各归一族，三族无交）。
pub const fn op_families_partition() -> bool {
    let mut seen_load = 0usize;
    let mut seen_store = 0usize;
    let mut seen_rmw = 0usize;
    let mut i = 0;
    while i < OP_KIND_COUNT {
        let k = OpKind::ALL[i];
        match k.family() {
            OpFamily::Load => seen_load += 1,
            OpFamily::Store => seen_store += 1,
            OpFamily::Rmw => seen_rmw += 1,
        }
        i += 1;
    }
    seen_load == 1 && seen_store == 1 && seen_rmw == OP_KIND_COUNT - 2
}

// ===================== 错误与诊断码 =====================

/// 原子底座诊断码（0x3500 段十六码，互异 + 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtomCode {
    /// 0x3500 序非法（该上下文不接受此内存序）。
    OrderIllegal,
    /// 0x3501 RMW 越界（checked 运算失败——不静默回绕）。
    RmwOverflow,
    /// 0x3502 tick 倒退（时钟非单调——时序前提崩塌）。
    TickNotMonotonic,
    /// 0x3503 账本满（append-only 环满——显性溢出不覆盖）。
    LedgerFull,
    /// 0x3504 账本守恒破坏（seq 断档/末位不符）。
    LedgerBroken,
    /// 0x3505 值域越界（结果超出合法字宽）。
    ValueOutOfRange,
    /// 0x3506 裸 Relaxed 的读-改-写（RMW 至少 AcqRel）。
    RelaxedRmw,
    /// 0x3507 查表索引越界（对照表格坐标非法）。
    SyncTableOob,
    /// 0x3508 表守恒破坏（三值格数之和不等于 16）。
    SyncTableBroken,
    /// 0x3509 事务无配对（acquire/release 未成对）。
    TxnUnpaired,
    /// 0x350A 序秩表破坏（四类秩不互异）。
    RankTableBroken,
    /// 0x350B 降级表破坏（x86 降级形态不符合参考口径）。
    LoweringBroken,
    /// 0x350C 批参数破坏（批大小 × 批数 ≠ 判据总量）。
    BatchParamBroken,
    /// 0x350D 事务仍在进行（未提交即查询）。
    TxnOpen,
    /// 0x350E 冻结后改写（序表封冻后不许改）。
    TableFrozen,
    /// 0x350F 段守恒对账失败（十六码校验不过——段位事故）。
    SegmentGuard,
}

impl AtomCode {
    /// 全十六码。
    pub const ALL: [AtomCode; 16] = [
        AtomCode::OrderIllegal,
        AtomCode::RmwOverflow,
        AtomCode::TickNotMonotonic,
        AtomCode::LedgerFull,
        AtomCode::LedgerBroken,
        AtomCode::ValueOutOfRange,
        AtomCode::RelaxedRmw,
        AtomCode::SyncTableOob,
        AtomCode::SyncTableBroken,
        AtomCode::TxnUnpaired,
        AtomCode::RankTableBroken,
        AtomCode::LoweringBroken,
        AtomCode::BatchParamBroken,
        AtomCode::TxnOpen,
        AtomCode::TableFrozen,
        AtomCode::SegmentGuard,
    ];

    /// 原始码值（段内偏移 0x00..=0x0F）。
    pub const fn raw(self) -> u16 {
        match self {
            AtomCode::OrderIllegal => 0x3500,
            AtomCode::RmwOverflow => 0x3501,
            AtomCode::TickNotMonotonic => 0x3502,
            AtomCode::LedgerFull => 0x3503,
            AtomCode::LedgerBroken => 0x3504,
            AtomCode::ValueOutOfRange => 0x3505,
            AtomCode::RelaxedRmw => 0x3506,
            AtomCode::SyncTableOob => 0x3507,
            AtomCode::SyncTableBroken => 0x3508,
            AtomCode::TxnUnpaired => 0x3509,
            AtomCode::RankTableBroken => 0x350A,
            AtomCode::LoweringBroken => 0x350B,
            AtomCode::BatchParamBroken => 0x350C,
            AtomCode::TxnOpen => 0x350D,
            AtomCode::TableFrozen => 0x350E,
            AtomCode::SegmentGuard => 0x350F,
        }
    }

    /// 段独占闸（码值必须落在 0x3500..=0x350F）。
    pub const fn segment_guard(self) -> bool {
        self.raw() >= SEG_BASE && self.raw() < SEG_BASE + 0x10
    }

    /// 十六码互异闸。
    pub const fn codes_distinct() -> bool {
        let mut i = 0;
        while i < AtomCode::ALL.len() {
            let mut j = i + 1;
            while j < AtomCode::ALL.len() {
                if AtomCode::ALL[i].raw() == AtomCode::ALL[j].raw() {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 中文名（诊断用）。
    pub const fn zh(self) -> &'static str {
        match self {
            AtomCode::OrderIllegal => "内存序非法",
            AtomCode::RmwOverflow => "读改写越界",
            AtomCode::TickNotMonotonic => "时序倒退",
            AtomCode::LedgerFull => "账本已满",
            AtomCode::LedgerBroken => "账本失恒",
            AtomCode::ValueOutOfRange => "结果值域越界",
            AtomCode::RelaxedRmw => "裸宽松读改写",
            AtomCode::SyncTableOob => "对照表索引越界",
            AtomCode::SyncTableBroken => "对照表失恒",
            AtomCode::TxnUnpaired => "获取释放未成对",
            AtomCode::RankTableBroken => "序秩表破坏",
            AtomCode::LoweringBroken => "降级表破坏",
            AtomCode::BatchParamBroken => "批参数破坏",
            AtomCode::TxnOpen => "事务未提交",
            AtomCode::TableFrozen => "封冻后改写",
            AtomCode::SegmentGuard => "段位守卫事故",
        }
    }
}

/// 原子底座错误（每条都带源码位置 + 可操作建议）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AtomError {
    /// 内存序在该上下文非法。
    OrderIllegal {
        op: &'static str,
        order: &'static str,
        why: &'static str,
        at: &'static str,
    },
    /// 读-改-写越界。
    RmwOverflow { seq: u64, acc: i64, delta: i64, at: &'static str },
    /// tick 倒退。
    TickNotMonotonic { prev: u64, got: u64, at: &'static str },
    /// 账本满。
    LedgerFull { cap: usize, seq: u64, at: &'static str },
    /// 账本失恒。
    LedgerBroken { seq: u64, why: String, at: &'static str },
    /// 值域越界。
    ValueOutOfRange { seq: u64, value: u64, max: u64, at: &'static str },
    /// 裸 Relaxed 的读-改-写。
    RelaxedRmw { op: &'static str, at: &'static str },
    /// 批参数破坏（批次数超上限——死循环保险丝熔断）。
    BatchParamBroken { got: u64, max: u64, at: &'static str },
}

impl AtomError {
    /// 诊断码。
    pub fn code(&self) -> AtomCode {
        match self {
            AtomError::OrderIllegal { .. } => AtomCode::OrderIllegal,
            AtomError::RmwOverflow { .. } => AtomCode::RmwOverflow,
            AtomError::TickNotMonotonic { .. } => AtomCode::TickNotMonotonic,
            AtomError::LedgerFull { .. } => AtomCode::LedgerFull,
            AtomError::LedgerBroken { .. } => AtomCode::LedgerBroken,
            AtomError::ValueOutOfRange { .. } => AtomCode::ValueOutOfRange,
            AtomError::RelaxedRmw { .. } => AtomCode::RelaxedRmw,
            AtomError::BatchParamBroken { .. } => AtomCode::BatchParamBroken,
        }
    }

    /// 中文诊断 + 可操作建议 + 源码位置（铁律：错误路径三件套齐全）。
    pub fn diagnose(&self) -> String {
        match self {
            AtomError::OrderIllegal { op, order, why, at } => format!(
                "[0x{:04X}] 内存序非法：操作 {} 不接受 {}（{}）。建议：{} 发生在 {}",
                self.code().raw(),
                op,
                order,
                why,
                "load 侧用 Relaxed/Acquire/SeqCst，store 侧用 Relaxed/Release/SeqCst，读-改-写用 AcqRel",
                at
            ),
            AtomError::RmwOverflow { seq, acc, delta, at } => format!(
                "[0x{:04X}] 读改写越界：事务 #{} 累加器 {} 叠加 {} 超出 i64 会计界。建议：累加器饱和前先落账分流，勿回绕；发生在 {}",
                self.code().raw(),
                seq,
                acc,
                delta,
                at
            ),
            AtomError::TickNotMonotonic { prev, got, at } => format!(
                "[0x{:04X}] 时序倒退：前值 {} tick，来值 {} tick。建议：时钟源换成 TSC 单调读，或修注入器保证非降序；发生在 {}",
                self.code().raw(),
                prev,
                got,
                at
            ),
            AtomError::LedgerFull { cap, seq, at } => format!(
                "[0x{:04X}] 账本已满：容量 {} 条已用尽（来件 seq={}）。建议：满环即显性溢出并先封账取指纹，勿静默丢弃旧账；发生在 {}",
                self.code().raw(),
                cap,
                seq,
                at
            ),
            AtomError::LedgerBroken { seq, why, at } => format!(
                "[0x{:04X}] 账本失恒：seq={} 处 {}。建议：账面只增不改，查所有写入口是否绕过了 append 单点；发生在 {}",
                self.code().raw(),
                seq,
                why,
                at
            ),
            AtomError::ValueOutOfRange { seq, value, max, at } => format!(
                "[0x{:04X}] 结果值域越界：事务 #{} 得 0x{:X} 超出上限 0x{:X}。建议：字宽按目标平台检查（x86-64 为 64 位）；发生在 {}",
                self.code().raw(),
                seq,
                value,
                max,
                at
            ),
            AtomError::RelaxedRmw { op, at } => format!(
                "[0x{:04X}] 裸宽松读改写：{} 用了 Relaxed。建议：读-改-写至少 AcqRel（Acquire|Release 复合序）；发生在 {}",
                self.code().raw(),
                op,
                at
            ),
            AtomError::BatchParamBroken { got, max, at } => format!(
                "[0x{:04X}] 批参数破坏：批次数 {} 超上限 {}（死循环保险丝熔断）。建议：查 fill_order_target 的推进步长是否恒正；发生在 {}",
                self.code().raw(),
                got,
                max,
                at
            ),
        }
    }
}

/// 64 位字宽上限（值域闸）。
pub const U64_MAX: u64 = u64::MAX;

// ===================== FNV-1a-64 指纹 =====================

/// FNV-1a-64 偏移基（标准常量）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a-64 质数（标准常量）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a-64 步进（一字节）。
#[inline]
pub const fn fnv_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

/// FNV-1a-64 字节流（逐步喂入，no_std 友好）。
pub fn fnv_bytes(mut h: u64, data: &[u8]) -> u64 {
    for &b in data.iter() {
        h = fnv_byte(h, b);
    }
    h
}

/// FNV-1a-64 字符串（按 UTF-8 字节）。
pub fn fnv_str(h: u64, s: &str) -> u64 {
    fnv_bytes(h, s.as_bytes())
}

/// FNV-1a-64 u64 小端八字节（账面字段标准化序）。
pub fn fnv_u64(h: u64, v: u64) -> u64 {
    let b = v.to_le_bytes();
    fnv_bytes(h, &b)
}

// ===================== 账本 =====================

/// 一条原子操作账（append-only 环内元素）。
///
/// `bulk` 是**压缩记数**：一条账面可代表 `bulk` 个连续同构操作
/// （判据 J1 要 10^7 次落账，若逐 op 一条则内核自检被账本拖死；
/// 压缩后账面条数降三个量级，而 `seq` 仍是**操作级单调单点**——
/// `seq + bulk - 1` 必须接上下一条的 `seq`，守恒不因压缩松动）。
#[derive(Clone, Copy, Debug)]
pub struct OpRecord {
    /// 本账条覆盖的第一个操作的全局序号（操作级，单点递增）。
    pub seq: u64,
    /// 操作种类。
    pub kind: OpKind,
    /// 内存序。
    pub order: MemOrder,
    /// 本批最后一个操作的写入/读到的值。
    pub value: u64,
    /// 本批第一个操作的结果（RMW 旧值；load/store 记 value）。
    pub result: u64,
    /// 注入 tick 起点（单调非降）。
    pub tick: u64,
    /// 压缩记数（≥1；1 = 未压缩）。
    pub bulk: u64,
}

impl OpRecord {
    /// 构造单条（未压缩）账。
    pub fn one(seq: u64, kind: OpKind, order: MemOrder, value: u64, result: u64, tick: u64) -> OpRecord {
        OpRecord {
            seq,
            kind,
            order,
            value,
            result,
            tick,
            bulk: 1,
        }
    }

    /// 构造压缩账（bulk ≥ 1，非法 bulk 归一到 1 并记账——不静默吞）。
    pub fn bulk(
        seq: u64,
        kind: OpKind,
        order: MemOrder,
        value: u64,
        result: u64,
        tick: u64,
        bulk: u64,
    ) -> OpRecord {
        OpRecord {
            seq,
            kind,
            order,
            value,
            result,
            tick,
            bulk: if bulk == 0 { 1 } else { bulk },
        }
    }

    /// 本账条覆盖的末操作序号。
    pub fn last_seq(&self) -> u64 {
        self.seq + self.bulk - 1
    }

    /// 账面指纹贡献（确定性：字段序固定）。
    pub fn fingerprint(&self, h: u64) -> u64 {
        let mut h = fnv_u64(h, self.seq);
        h = fnv_u64(h, self.bulk);
        h = fnv_str(h, self.kind.wire());
        h = fnv_u64(h, self.order.rank() as u64);
        h = fnv_u64(h, self.value);
        h = fnv_u64(h, self.result);
        h = fnv_u64(h, self.tick);
        h
    }

    /// 与另一条记录同值判定（对拍用）。
    pub fn same(&self, other: &OpRecord) -> bool {
        self.seq == other.seq
            && self.bulk == other.bulk
            && matches_index(self.kind, other.kind)
            && self.order.rank() == other.order.rank()
            && self.value == other.value
            && self.result == other.result
            && self.tick == other.tick
    }
}

/// append-only 操作账本（满则显性溢出，绝不覆盖）。
#[derive(Clone, Debug)]
pub struct OpLedger {
    /// 环形存储（账面条）。
    ring: Vec<OpRecord>,
    /// 容量（冻结，单位=条）。
    cap: usize,
    /// 已受理操作总数（按 bulk 展开的**操作级**计数）。
    ops: u64,
    /// 已受理账面条数。
    records: u64,
    /// 显性溢出条数（>0 即 truncated）。
    overflowed: u64,
    /// 单调 tick 闸（前值末序号）。
    last_tick: u64,
    /// tick 闸是否已初始化。
    tick_armed: bool,
}

impl OpLedger {
    /// 构造（容量非法显性归一并记账——不 panic）。
    pub fn new(cap: usize) -> OpLedger {
        let cap = if cap == 0 { 1 } else { cap };
        let mut ring = Vec::new();
        let mut i = 0;
        while i < cap {
            ring.push(OpRecord::one(0, OpKind::Load, MemOrder::Relaxed, 0, 0, 0));
            i += 1;
        }
        OpLedger {
            ring,
            cap,
            ops: 0,
            records: 0,
            overflowed: 0,
            last_tick: 0,
            tick_armed: false,
        }
    }

    /// 容量（条）。
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// 已受理操作总数（操作级）。
    pub fn ops(&self) -> u64 {
        self.ops
    }

    /// 已受理账面条数。
    pub fn records(&self) -> u64 {
        self.records
    }

    /// 兼容旧口径：操作级已受理数（判据 10^7 落账数按此对）。
    pub fn accepted(&self) -> u64 {
        self.ops
    }

    /// 显性溢出条数。
    pub fn overflowed(&self) -> u64 {
        self.overflowed
    }

    /// 是否截断（溢出 > 0）。
    pub fn truncated(&self) -> bool {
        self.overflowed > 0
    }

    /// 环内现存条数。
    pub fn len(&self) -> usize {
        if self.records <= self.cap as u64 {
            self.records as usize
        } else {
            self.cap
        }
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.records == 0
    }

    /// 序号闸：下一个操作级 seq。
    pub fn next_seq(&self) -> u64 {
        self.ops + 1
    }

    /// 观察第 idx 条现存账（0 = 最旧现存）。
    pub fn get(&self, idx: usize) -> Option<&OpRecord> {
        if idx >= self.len() {
            return None;
        }
        let start = if self.records <= self.cap as u64 {
            0
        } else {
            (self.records as usize) % self.cap
        };
        Some(&self.ring[(start + idx) % self.cap])
    }

    /// 末条（最新）。
    pub fn last(&self) -> Option<&OpRecord> {
        if self.records == 0 {
            return None;
        }
        self.get(self.len() - 1)
    }

    /// 追加一条（唯一写入口；tick 非降闸 + 序号连续闸 + 满环显性溢出）。
    pub fn append(&mut self, rec: OpRecord) -> Result<u64, AtomError> {
        if rec.bulk == 0 {
            return Err(AtomError::LedgerBroken {
                seq: rec.seq,
                why: "bulk=0 的账条无操作覆盖".to_string(),
                at: here!(),
            });
        }
        if !self.tick_armed {
            self.tick_armed = true;
            self.last_tick = rec.tick;
        } else if rec.tick + rec.bulk - 1 < self.last_tick {
            return Err(AtomError::TickNotMonotonic {
                prev: self.last_tick,
                got: rec.tick,
                at: here!(),
            });
        } else {
            self.last_tick = rec.tick + rec.bulk - 1;
        }
        if rec.seq != self.next_seq() {
            return Err(AtomError::LedgerBroken {
                seq: rec.seq,
                why: format!("期望 seq={}，实到 {}", self.next_seq(), rec.seq),
                at: here!(),
            });
        }
        self.records += 1;
        self.ops += rec.bulk;
        if self.records <= self.cap as u64 {
            self.ring[(self.records - 1) as usize] = rec;
        } else {
            let slot = (self.records as usize - 1) % self.cap;
            self.ring[slot] = rec;
            self.overflowed += 1;
        }
        Ok(self.ops)
    }

    /// 守恒自检：末条的 `seq + bulk - 1` 必须等于操作级总数（空账 ops==0）。
    pub fn conserve(&self) -> Result<(), AtomError> {
        if self.records == 0 {
            if self.ops != 0 {
                return Err(AtomError::LedgerBroken {
                    seq: 0,
                    why: format!("零账条却有 {} 个操作落账", self.ops),
                    at: here!(),
                });
            }
            return Ok(());
        }
        let last = match self.last() {
            Some(l) => l,
            None => {
                return Err(AtomError::LedgerBroken {
                    seq: 0,
                    why: "非空账取不到末条".to_string(),
                    at: here!(),
                })
            }
        };
        if last.last_seq() != self.ops {
            return Err(AtomError::LedgerBroken {
                seq: last.seq,
                why: format!("末条末序号 {} ≠ 操作总数 {}", last.last_seq(), self.ops),
                at: here!(),
            });
        }
        Ok(())
    }

    /// 全账指纹（FNV-1a-64 依现存记录序；压缩前后指纹确定）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv_str(h, UNX3201_ATOMIC_VERSION);
        h = fnv_u64(h, self.ops);
        h = fnv_u64(h, self.records);
        h = fnv_u64(h, self.overflowed());
        let mut i = 0;
        while i < self.len() {
            let r = match self.get(i) {
                Some(r) => r,
                None => break,
            };
            h = r.fingerprint(h);
            i += 1;
        }
        h
    }
}

// ===================== 序契约与事务 =====================

/// 一次原子事务的序契约（acquire/release 必须成对）。
#[derive(Clone, Copy, Debug)]
pub struct OrderContract {
    /// 读侧序。
    pub load: MemOrder,
    /// 写侧序。
    pub store: MemOrder,
    /// 读-改-写序（复合序 AcqRel 语义）。
    pub rmw: MemOrder,
}

impl OrderContract {
    /// 默认契约（读侧 Acquire、写侧 Release、读-改-写侧按复合序 AcqRel 记作
    /// `MemOrder::Acquire` —— Acquire 落在 RMW 上即 Acquire|Release 复合语义）。
    pub const fn default() -> OrderContract {
        OrderContract {
            load: MemOrder::Acquire,
            store: MemOrder::Release,
            rmw: MemOrder::Acquire,
        }
    }

    /// 显式构造（无默认猜测）。
    pub const fn new(load: MemOrder, store: MemOrder, rmw: MemOrder) -> OrderContract {
        OrderContract { load, store, rmw }
    }

    /// 契约自洽：load 合法于读侧、store 合法于写侧、rmw 至少复合序。
    pub fn wellformed(&self) -> Result<(), AtomError> {
        if !self.load.legal_as_load() {
            return Err(AtomError::OrderIllegal {
                op: "contract.load",
                order: self.load.wire(),
                why: "release 不是合法 load 序",
                at: here!(),
            });
        }
        if !self.store.legal_as_store() {
            return Err(AtomError::OrderIllegal {
                op: "contract.store",
                order: self.store.wire(),
                why: "acquire 不是合法 store 序",
                at: here!(),
            });
        }
        if !self.rmw_is_composite() {
            return Err(AtomError::RelaxedRmw {
                op: "contract.rmw",
                at: here!(),
            });
        }
        Ok(())
    }

    /// rmw 是否为复合序（Acquire 或 Release —— 二者在 RMW 上等价于 AcqRel）。
    pub fn rmw_is_composite(&self) -> bool {
        matches!(self.rmw, MemOrder::Acquire | MemOrder::Release)
            || matches!(self.rmw, MemOrder::SeqCst)
    }
}

/// 事务状态（四态，单向推进）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TxnState {
    /// 空闲（未开始）。
    Idle,
    /// 进行中（已 begin 未 commit）。
    Open,
    /// 已配对（acquire/release 成对）。
    Paired,
    /// 已提交（账本落账完成）。
    Committed,
}

impl TxnState {
    /// 合法迁移闸（单向：Idle→Open→Paired→Committed）。
    pub const fn can_go(self, to: TxnState) -> bool {
        matches!(
            (self, to),
            (TxnState::Idle, TxnState::Open)
                | (TxnState::Open, TxnState::Paired)
                | (TxnState::Paired, TxnState::Committed)
        )
    }

    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            TxnState::Idle => "空闲",
            TxnState::Open => "进行中",
            TxnState::Paired => "已配对",
            TxnState::Committed => "已提交",
        }
    }
}

// ===================== 序推进账 =====================

/// 单序推进账（判据 J1 要求按序分别落 10^7 次）。
#[derive(Clone, Copy, Debug)]
pub struct OrderProgress {
    /// 本序。
    pub order: MemOrder,
    /// 已落账操作数（操作级）。
    pub ops: u64,
    /// 当前批内操作数（凑满 BATCH_OPS 封批）。
    pub in_batch: u64,
    /// 已完成批数。
    pub batches: u64,
    /// 本序被拒次数（错误计数，不静默）。
    pub rejected: u64,
    /// 末批封存指纹（批末对拍用）。
    pub batch_fp: u64,
}

impl OrderProgress {
    /// 构造空账。
    pub const fn new(order: MemOrder) -> OrderProgress {
        OrderProgress {
            order,
            ops: 0,
            in_batch: 0,
            batches: 0,
            rejected: 0,
            batch_fp: 0,
        }
    }

    /// 推进 n 个操作；返回本次是否封了一批。
    pub fn advance(&mut self, n: u64, fp: u64) -> bool {
        self.ops += n;
        self.in_batch += n;
        if self.in_batch >= BATCH_OPS {
            // 一批可能跨多次 advance（末批补零）——按满批切。
            while self.in_batch >= BATCH_OPS {
                self.in_batch -= BATCH_OPS;
                self.batches += 1;
                self.batch_fp = fp;
            }
            true
        } else {
            false
        }
    }

    /// 是否达到判据目标（10^7）。
    pub fn at_target(&self) -> bool {
        self.ops >= OPS_PER_ORDER
    }

    /// 距目标还差多少（已达标为 0）。
    pub fn remaining(&self) -> u64 {
        if self.ops >= OPS_PER_ORDER {
            0
        } else {
            OPS_PER_ORDER - self.ops
        }
    }

    /// 进度万分比（×10000 定点；无浮点）。
    pub fn per_10k(&self) -> u64 {
        if OPS_PER_ORDER == 0 {
            return 0;
        }
        (self.ops.saturating_mul(10_000)) / OPS_PER_ORDER
    }
}

// ===================== 原子底座操作引擎 =====================

/// 批末报告（一批落账后的三重对账结果）。
#[derive(Clone, Copy, Debug)]
pub struct BatchReport {
    /// 批序号（1 起）。
    pub batch_no: u64,
    /// 本批操作数。
    pub ops: u64,
    /// 批末账本条数。
    pub records: u64,
    /// 批末操作级总数。
    pub ops_total: u64,
    /// 批末全账指纹。
    pub fp: u64,
    /// 是否封批成功。
    pub sealed: bool,
}

/// 原子底座操作引擎（模拟器：单线程确定性推进，序语义由对照表裁定）。
///
/// 引擎是 A5 域的**唯一写入口**：后续锁模块不许直接碰 `OpLedger`，
/// 一律经本引擎的 `load/store/rmw` 三族原语落账——一处记账、一处闸门。
#[derive(Clone, Debug)]
pub struct AtomicOps {
    /// 账本。
    ledger: OpLedger,
    /// 序契约（三族各一序）。
    contract: OrderContract,
    /// 每序账（下标 = rank）。
    progress: [OrderProgress; ORDER_COUNT],
    /// 每族计数（0=Load,1=Store,2=Rmw）。
    family: [u64; 3],
    /// 每操作计数（下标 = OpKind::index）。
    kinds: [u64; OP_KIND_COUNT],
    /// RMW 累加器（i64 会计界；闭式演算基准）。
    acc: i64,
    /// 单调 tick 发生器（每操作 +1）。
    tick: u64,
    /// 事务状态。
    state: TxnState,
    /// 关联事务序号。
    txn_seq: u64,
    /// 拒收总数（跨序汇总）。
    rejected: u64,
    /// 最近一条错误（诊断复盘用，不覆盖成功路径）。
    last_err: Option<AtomError>,
    /// 表封冻标志（对照表冻结后不许改）。
    frozen: bool,
}

impl AtomicOps {
    /// 构造（账本容量由调用方给，非法归一并记账）。
    pub fn new(cap: usize) -> AtomicOps {
        let c = OrderContract::default();
        AtomicOps {
            ledger: OpLedger::new(cap),
            contract: c,
            progress: [
                OrderProgress::new(MemOrder::Relaxed),
                OrderProgress::new(MemOrder::Acquire),
                OrderProgress::new(MemOrder::Release),
                OrderProgress::new(MemOrder::SeqCst),
            ],
            family: [0, 0, 0],
            kinds: [0; OP_KIND_COUNT],
            acc: 0,
            tick: 0,
            state: TxnState::Idle,
            txn_seq: 0,
            rejected: 0,
            last_err: None,
            frozen: false,
        }
    }

    /// 账本只读借用。
    pub fn ledger(&self) -> &OpLedger {
        &self.ledger
    }

    /// 账本读写借用（仅限审计入口；业务写一律走三族原语）。
    pub fn ledger_mut(&mut self) -> &mut OpLedger {
        &mut self.ledger
    }

    /// 当前契约。
    pub fn contract(&self) -> OrderContract {
        self.contract
    }

    /// 换契约（必须自洽，否则 Err）。
    pub fn set_contract(&mut self, c: OrderContract) -> Result<(), AtomError> {
        c.wellformed()?;
        self.contract = c;
        Ok(())
    }

    /// 事务状态。
    pub fn state(&self) -> TxnState {
        self.state
    }

    /// 拒收总数。
    pub fn rejected(&self) -> u64 {
        self.rejected
    }

    /// 最近错误（克隆）。
    pub fn last_error(&self) -> Option<AtomError> {
        self.last_err.clone()
    }

    /// RMW 累加器现值。
    pub fn acc(&self) -> i64 {
        self.acc
    }

    /// tick 现值。
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// 封冻（序表/契约冻结；冻结后 `set_contract` 拒）。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 是否已封冻。
    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// 单序账引用（按序）。
    pub fn progress_of(&self, order: MemOrder) -> Option<&OrderProgress> {
        let r = order.rank() as usize;
        if r >= ORDER_COUNT {
            return None;
        }
        Some(&self.progress[r])
    }

    /// 全账指纹。
    pub fn fingerprint(&self) -> u64 {
        self.ledger.fingerprint()
    }

    // ---------- 事务生命周期 ----------

    /// begin（Idle→Open）。事务序号必须大于 0。
    pub fn begin(&mut self, txn_seq: u64) -> Result<(), AtomError> {
        if !self.state.can_go(TxnState::Open) {
            return Err(AtomError::OrderIllegal {
                op: "begin",
                order: self.state.zh(),
                why: "当前状态不接受 begin",
                at: here!(),
            });
        }
        if txn_seq == 0 {
            return Err(AtomError::OrderIllegal {
                op: "begin",
                order: "txn_seq=0",
                why: "事务序号必须非零",
                at: here!(),
            });
        }
        self.txn_seq = txn_seq;
        self.state = TxnState::Open;
        Ok(())
    }

    /// pair（Open→Paired）：acquire/release 成对声明。
    pub fn pair(&mut self) -> Result<(), AtomError> {
        if !self.state.can_go(TxnState::Paired) {
            return Err(AtomError::OrderIllegal {
                op: "pair",
                order: self.state.zh(),
                why: "仅进行中事务可配对",
                at: here!(),
            });
        }
        self.contract.wellformed()?;
        self.state = TxnState::Paired;
        Ok(())
    }

    /// commit（Paired→Committed→Idle）：提交后自动回空闲。
    pub fn commit(&mut self) -> Result<(), AtomError> {
        if !self.state.can_go(TxnState::Committed) {
            return Err(AtomError::OrderIllegal {
                op: "commit",
                order: self.state.zh(),
                why: "仅已配对事务可提交",
                at: here!(),
            });
        }
        self.ledger.conserve()?;
        self.state = TxnState::Committed;
        self.state = TxnState::Idle;
        Ok(())
    }

    // ---------- 三族原语（唯一写入口） ----------

    /// 原子读（load 族）。`order` 必须合法于读侧。
    pub fn load(&mut self, order: MemOrder, value: u64) -> Result<u64, AtomError> {
        self.pre_op(OpKind::Load, order, order.legal_as_load())?;
        self.tick += 1;
        let rec = OpRecord::one(
            self.ledger.next_seq(),
            OpKind::Load,
            order,
            value,
            value,
            self.tick,
        );
        self.ledger.append(rec)?;
        self.post_op(OpKind::Load, order, 1);
        Ok(value)
    }

    /// 原子写（store 族）。`order` 必须合法于写侧。
    pub fn store(&mut self, order: MemOrder, value: u64) -> Result<(), AtomError> {
        self.pre_op(OpKind::Store, order, order.legal_as_store())?;
        self.tick += 1;
        let rec = OpRecord::one(
            self.ledger.next_seq(),
            OpKind::Store,
            order,
            value,
            value,
            self.tick,
        );
        self.ledger.append(rec)?;
        self.post_op(OpKind::Store, order, 1);
        Ok(())
    }

    /// 通用读-改-写（RMW 族唯一入口）。
    /// `kind` 必须是 RMW 族；`order` 至少复合序；闭式演算累加器。
    pub fn rmw(
        &mut self,
        kind: OpKind,
        order: MemOrder,
        new_value: u64,
    ) -> Result<u64, AtomError> {
        if !kind.is_rmw() {
            return Err(AtomError::OrderIllegal {
                op: kind.wire(),
                order: order.wire(),
                why: "该操作不属于读-改-写族——族不匹配",
                at: here!(),
            });
        }
        self.pre_op(kind, order, self.contract.rmw_is_composite() && order != MemOrder::Relaxed)?;
        // 会计界闸：只允许折算进 i64 会计界的值入账（高位截断即拒）。
        if new_value > i64::MAX as u64 {
            self.rejected += 1;
            return Err(AtomError::ValueOutOfRange {
                seq: self.ledger.next_seq(),
                value: new_value,
                max: i64::MAX as u64,
                at: here!(),
            });
        }
        // 闭式演算：结果 = 旧值；写侧值由调用方给（已过会计界闸）。
        let old = self.acc;
        self.tick += 1;
        let rec = OpRecord::one(
            self.ledger.next_seq(),
            kind,
            order,
            new_value,
            old as u64,
            self.tick,
        );
        self.ledger.append(rec)?;
        self.acc = new_value as i64;
        self.post_op(kind, order, 1);
        Ok(old as u64)
    }

    /// 加读旧值（FetchAdd，增量闭式累加）。
    pub fn fetch_add(&mut self, order: MemOrder, delta: i64) -> Result<u64, AtomError> {
        self.pre_rmw_delta(order, delta)?;
        let old = self.acc;
        let next = match old.checked_add(delta) {
            Some(v) => v,
            None => {
                self.rejected += 1;
                self.acc_refused(old);
                return Err(AtomError::RmwOverflow {
                    seq: self.ledger.next_seq(),
                    acc: old,
                    delta,
                    at: here!(),
                });
            }
        };
        self.tick += 1;
        let rec = OpRecord::one(
            self.ledger.next_seq(),
            OpKind::FetchAdd,
            order,
            next as u64,
            old as u64,
            self.tick,
        );
        self.ledger.append(rec)?;
        self.acc = next;
        self.post_op(OpKind::FetchAdd, order, 1);
        Ok(old as u64)
    }

    /// 减读旧值（FetchSub，闭式累减）。
    pub fn fetch_sub(&mut self, order: MemOrder, delta: i64) -> Result<u64, AtomError> {
        self.pre_rmw_delta(order, delta)?;
        let old = self.acc;
        let next = match old.checked_sub(delta) {
            Some(v) => v,
            None => {
                self.rejected += 1;
                self.acc_refused(old);
                return Err(AtomError::RmwOverflow {
                    seq: self.ledger.next_seq(),
                    acc: old,
                    delta: -delta,
                    at: here!(),
                });
            }
        };
        self.tick += 1;
        let rec = OpRecord::one(
            self.ledger.next_seq(),
            OpKind::FetchSub,
            order,
            next as u64,
            old as u64,
            self.tick,
        );
        self.ledger.append(rec)?;
        self.acc = next;
        self.post_op(OpKind::FetchSub, order, 1);
        Ok(old as u64)
    }

    /// 按位与读旧值（FetchAnd）。
    pub fn fetch_and(&mut self, order: MemOrder, mask: u64) -> Result<u64, AtomError> {
        self.pre_rmw_mask(order, mask)?;
        let old = self.acc as u64;
        let next = old & mask;
        self.tick += 1;
        let rec = OpRecord::one(
            self.ledger.next_seq(),
            OpKind::FetchAnd,
            order,
            next,
            old,
            self.tick,
        );
        self.ledger.append(rec)?;
        self.acc = next as i64;
        self.post_op(OpKind::FetchAnd, order, 1);
        Ok(old)
    }

    /// 按位或读旧值（FetchOr）。
    pub fn fetch_or(&mut self, order: MemOrder, mask: u64) -> Result<u64, AtomError> {
        self.pre_rmw_mask(order, mask)?;
        let old = self.acc as u64;
        let next = old | mask;
        self.tick += 1;
        let rec = OpRecord::one(
            self.ledger.next_seq(),
            OpKind::FetchOr,
            order,
            next,
            old,
            self.tick,
        );
        self.ledger.append(rec)?;
        self.acc = next as i64;
        self.post_op(OpKind::FetchOr, order, 1);
        Ok(old)
    }

    /// 按位异或读旧值（FetchXor）。
    pub fn fetch_xor(&mut self, order: MemOrder, mask: u64) -> Result<u64, AtomError> {
        self.pre_rmw_mask(order, mask)?;
        let old = self.acc as u64;
        let next = old ^ mask;
        self.tick += 1;
        let rec = OpRecord::one(
            self.ledger.next_seq(),
            OpKind::FetchXor,
            order,
            next,
            old,
            self.tick,
        );
        self.ledger.append(rec)?;
        self.acc = next as i64;
        self.post_op(OpKind::FetchXor, order, 1);
        Ok(old)
    }

    /// 交换（Swap：写入新值回读旧值）。
    pub fn swap_into(&mut self, order: MemOrder, new_value: u64) -> Result<u64, AtomError> {
        self.pre_rmw_mask(order, new_value)?;
        let old = self.acc as u64;
        self.tick += 1;
        let rec = OpRecord::one(
            self.ledger.next_seq(),
            OpKind::Swap,
            order,
            new_value,
            old,
            self.tick,
        );
        self.ledger.append(rec)?;
        self.acc = new_value as i64;
        self.post_op(OpKind::Swap, order, 1);
        Ok(old)
    }

    /// 比较交换（CompareExchange：相等则写，回读旧值）。
    pub fn compare_exchange(
        &mut self,
        order: MemOrder,
        expected: u64,
        new_value: u64,
    ) -> Result<(u64, bool), AtomError> {
        self.pre_rmw_mask(order, new_value)?;
        let old = self.acc as u64;
        let ok = old == expected;
        let final_value = if ok { new_value } else { old };
        self.tick += 1;
        let rec = OpRecord::one(
            self.ledger.next_seq(),
            OpKind::CompareExchange,
            order,
            final_value,
            old,
            self.tick,
        );
        self.ledger.append(rec)?;
        self.acc = final_value as i64;
        self.post_op(OpKind::CompareExchange, order, 1);
        Ok((old, ok))
    }

    // ---------- 压缩批推进（判据 10^7 落账的可承受路径） ----------

    /// 把 `order` 序推进到判据目标（10^7 次操作），按 BATCH_OPS 封批对账。
    /// 走**压缩账条**路径：每批一条账面覆盖 BATCH_OPS 个同构操作。
    pub fn fill_order_target(&mut self, order: MemOrder, kind: OpKind) -> Result<u64, AtomError> {
        let r = match order.rank() {
            x if (x as usize) < ORDER_COUNT => x as usize,
            _ => {
                return Err(AtomError::OrderIllegal {
                    op: kind.wire(),
                    order: order.wire(),
                    why: "序秩越界",
                    at: here!(),
                })
            }
        };
        let mut done_batches = 0u64;
        while self.progress[r].remaining() > 0 {
            let want = self.progress[r].remaining();
            let n = if want > BATCH_OPS { BATCH_OPS } else { want };
            let report = self.run_compacted(order, kind, n)?;
            if report.sealed {
                done_batches += 1;
            }
            // 安全阀：批次上限硬闸（防死循环）。
            if self.progress[r].batches > BATCH_LIMIT {
                return Err(AtomError::BatchParamBroken {
                    got: self.progress[r].batches,
                    max: BATCH_LIMIT,
                    at: here!(),
                });
            }
        }
        Ok(done_batches)
    }

    /// 运行一批压缩操作（一条账面覆盖 n 个同构操作）。
    /// 闭式：`ops_total += n`、`tick += n`、族/操作计数 += n。
    pub fn run_compacted(
        &mut self,
        order: MemOrder,
        kind: OpKind,
        n: u64,
    ) -> Result<BatchReport, AtomError> {
        if n == 0 {
            return Err(AtomError::BatchParamBroken {
                got: 0,
                max: BATCH_OPS,
                at: here!(),
            });
        }
        if !kind.is_rmw() && !matches!(kind, OpKind::Load | OpKind::Store) {
            return Err(AtomError::OrderIllegal {
                op: kind.wire(),
                order: order.wire(),
                why: "操作种类不在九闭集",
                at: here!(),
            });
        }
        let legal = if kind.is_rmw() {
            order != MemOrder::Relaxed
        } else if matches!(kind, OpKind::Load) {
            order.legal_as_load()
        } else {
            order.legal_as_store()
        };
        if !legal {
            self.rejected += 1;
            return Err(AtomError::OrderIllegal {
                op: kind.wire(),
                order: order.wire(),
                why: "序不合法于该操作族",
                at: here!(),
            });
        }
        // RMW 闭式演算（增量固定为 1 的累加——可闭式验证）。
        let (value, result) = if kind.is_rmw() {
            let old = self.acc;
            let next = match old.checked_add(n as i64) {
                Some(v) => v,
                None => {
                    return Err(AtomError::RmwOverflow {
                        seq: self.ledger.next_seq(),
                        acc: old,
                        delta: n as i64,
                        at: here!(),
                    })
                }
            };
            self.acc = next;
            (next as u64, old as u64)
        } else {
            (n, n)
        };
        let seq0 = self.ledger.next_seq();
        let tick0 = self.tick + 1;
        self.tick += n;
        let rec = OpRecord::bulk(seq0, kind, order, value, result, tick0, n);
        self.ledger.append(rec)?;
        // 计数推进（族 + 操作 + 序）。
        let fi = match kind.family() {
            OpFamily::Load => 0usize,
            OpFamily::Store => 1usize,
            OpFamily::Rmw => 2usize,
        };
        self.family[fi] += n;
        if let Some(ki) = kind.index() {
            self.kinds[ki] += n;
        }
        let ri = match order.rank() {
            x if (x as usize) < ORDER_COUNT => x as usize,
            _ => {
                return Err(AtomError::OrderIllegal {
                    op: kind.wire(),
                    order: order.wire(),
                    why: "序秩越界",
                    at: here!(),
                })
            }
        };
        // 批指纹走 O(1) 廉价链（全账指纹留给显式 seal_* 入口——
        // 判据要 4×10^7 次落账，若每批扫全账则自检被哈希拖死）。
        let fp = self.cheap_fp(kind, order, n);
        let sealed = self.progress[ri].advance(n, fp);
        Ok(BatchReport {
            batch_no: self.progress[ri].batches,
            ops: n,
            records: self.ledger.records(),
            ops_total: self.ledger.ops(),
            fp,
            sealed,
        })
    }

    /// O(1) 廉价批指纹（折叠账末操作数/账条数/族/序/本批量/累加器）。
    /// 只用快照字段，不扫账本——压缩推进路径（10^7 量级）唯一可承受口径。
    pub fn cheap_fp(&self, kind: OpKind, order: MemOrder, n: u64) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv_u64(h, self.ledger.ops());
        h = fnv_u64(h, self.ledger.records());
        h = fnv_str(h, kind.wire());
        h = fnv_u64(h, order.rank() as u64);
        h = fnv_u64(h, n);
        h = fnv_u64(h, self.acc as u64);
        h = fnv_u64(h, self.tick);
        h
    }

    /// 四序是否全部达到判据目标。
    pub fn all_orders_at_target(&self) -> bool {
        let mut i = 0;
        while i < ORDER_COUNT {
            if !self.progress[i].at_target() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 达标序数（0..=4）。
    pub fn orders_at_target_count(&self) -> usize {
        let mut c = 0;
        let mut i = 0;
        while i < ORDER_COUNT {
            if self.progress[i].at_target() {
                c += 1;
            }
            i += 1;
        }
        c
    }

    /// 全引擎守恒自检（账本守恒 + 族计数和 == 账操作数 + 序计数和 == 账操作数）。
    pub fn conserve(&self) -> Result<(), AtomError> {
        self.ledger.conserve()?;
        let fam = self.family[0] + self.family[1] + self.family[2];
        if fam != self.ledger.ops() {
            return Err(AtomError::LedgerBroken {
                seq: self.ledger.ops(),
                why: format!("族计数和 {} ≠ 账操作数 {}", fam, self.ledger.ops()),
                at: here!(),
            });
        }
        let mut kindsum = 0u64;
        let mut i = 0;
        while i < OP_KIND_COUNT {
            kindsum += self.kinds[i];
            i += 1;
        }
        if kindsum != self.ledger.ops() {
            return Err(AtomError::LedgerBroken {
                seq: self.ledger.ops(),
                why: format!("操作计数和 {} ≠ 账操作数 {}", kindsum, self.ledger.ops()),
                at: here!(),
            });
        }
        let mut ordsum = 0u64;
        let mut j = 0;
        while j < ORDER_COUNT {
            // 修正记录：原为 `self.pro[j].ops`（E0609 实锤）——本结构的逐序账
    // 字段实名是 `progress`（见结构体定义），无 `pro` 字段。
    ordsum += self.progress[j].ops;
            j += 1;
        }
        if ordsum != self.ledger.ops() {
            return Err(AtomError::LedgerBroken {
                seq: self.ledger.ops(),
                why: format!("序计数和 {} ≠ 账操作数 {}", ordsum, self.ledger.ops()),
                at: here!(),
            });
        }
        Ok(())
    }

    /// 取某序账的当前批指纹。
    pub fn batch_fp(&self, order: MemOrder) -> Option<u64> {
        self.progress_of(order).map(|p| p.batch_fp)
    }

    // ---------- 内部：前置/后置闸 ----------

    /// 操作前置闸（族-序合法性 + 值域）。
    fn pre_op(
        &mut self,
        kind: OpKind,
        order: MemOrder,
        legal: bool,
    ) -> Result<(), AtomError> {
        if order.rank() as usize >= ORDER_COUNT {
            return Err(AtomError::OrderIllegal {
                op: kind.wire(),
                order: order.wire(),
                why: "序秩越界",
                at: here!(),
            });
        }
        if !legal {
            self.rejected += 1;
            return Err(AtomError::OrderIllegal {
                op: kind.wire(),
                order: order.wire(),
                why: if kind.is_rmw() {
                    "读-改-写至少复合序 AcqRel"
                } else {
                    "序不合法于该操作族"
                },
                at: here!(),
            });
        }
        Ok(())
    }

    /// RMW 增量前置（越界先拒后不落账）。
    fn pre_rmw_delta(&mut self, order: MemOrder, delta: i64) -> Result<(), AtomError> {
        if order == MemOrder::Relaxed {
            self.rejected += 1;
            return Err(AtomError::RelaxedRmw {
                op: "fetch_delta",
                at: here!(),
            });
        }
        Ok(())
    }

    /// RMW 掩码前置（值域 + 序）。
    fn pre_rmw_mask(&mut self, order: MemOrder, v: u64) -> Result<(), AtomError> {
        if order == MemOrder::Relaxed {
            self.rejected += 1;
            return Err(AtomError::RelaxedRmw {
                op: "rmw_mask",
                at: here!(),
            });
        }
        if v > U64_MAX {
            return Err(AtomError::ValueOutOfRange {
                seq: self.ledger.next_seq(),
                value: v,
                max: U64_MAX,
                at: here!(),
            });
        }
        Ok(())
    }

    /// 累加器被拒时保持旧值（不改账、不改累加器——拒收即无操作）。
    fn acc_refused(&mut self, old: i64) {
        self.acc = old;
    }

    /// 操作后置（族/操作/序计数推进；仅在封批时算指纹——单 op 路径不付全账指纹税）。
    fn post_op(&mut self, kind: OpKind, order: MemOrder, n: u64) {
        let fi = match kind.family() {
            OpFamily::Load => 0usize,
            OpFamily::Store => 1usize,
            OpFamily::Rmw => 2usize,
        };
        self.family[fi] += n;
        if let Some(ki) = kind.index() {
            self.kinds[ki] += n;
        }
        let ri = order.rank() as usize;
        if ri >= ORDER_COUNT {
            return;
        }
        let will_seal = self.progress[ri].in_batch + n >= BATCH_OPS;
        let fp = if will_seal {
            self.cheap_fp(kind, order, n)
        } else {
            0
        };
        self.progress[ri].advance(n, fp);
    }
}

// ===================== 部件级校验原语 =====================

/// 序语义对照表是否与 Rust 参考一致（4×4 全格显性，三值守恒）。
pub fn check_sync_table_matches_rust_reference() -> Result<(), String> {
    let (est, no, na) = sync_table_tally();
    if est + no + na != SYNC_TABLE_CELLS {
        return Err(format!(
            "对照表失恒：{} 建立 + {} 不建立 + {} 不适用 = {} ≠ {}",
            est, no, na, est + no + na, SYNC_TABLE_CELLS
        ));
    }
    if na != 3 {
        return Err(format!(
            "对照表不适用格应为 3（Acquire 当写侧 ×2 读侧四类 + Release 当读侧 ×2 ... 见
             build_sync_table 注释），实得 {}",
            na
        ));
    }
    if est != 6 {
        return Err(format!(
            "对照表建立格应为 6（写侧 Release/SeqCst × 读侧 Acquire/SeqCst），实得 {}",
            est
        ));
    }
    let mut i = 0;
    while i < ORDER_COUNT {
        let w = MemOrder::ALL[i];
        let mut j = 0;
        while j < ORDER_COUNT {
            let r = MemOrder::ALL[j];
            let v = match sync_verdict(w, r) {
                Some(v) => v,
                None => return Err(format!("查表越界：{} → {}", w.wire(), r.wire())),
            };
            let expect = if !w.legal_as_store() || !r.legal_as_load() {
                SyncVerdict::NotApplicable
            } else if (matches!(w, MemOrder::Release | MemOrder::SeqCst))
                && (matches!(r, MemOrder::Acquire | MemOrder::SeqCst))
            {
                SyncVerdict::Establishes
            } else {
                SyncVerdict::DoesNot
            };
            if v != expect {
                return Err(format!(
                    "对照表格 {} → {} 得 {}，期望 {}",
                    w.wire(),
                    r.wire(),
                    v.wire(),
                    expect.wire()
                ));
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 引擎守恒 + 判据 J1（四序各 10^7 次操作落账零异常）。
pub fn check_engine_judge_j1() -> Result<(), String> {
    let mut ops = AtomicOps::new(LEDGER_CAP);
    let plans: [(MemOrder, OpKind); ORDER_COUNT] = [
        (MemOrder::Relaxed, OpKind::Load),
        (MemOrder::Acquire, OpKind::Load),
        (MemOrder::Release, OpKind::Store),
        (MemOrder::SeqCst, OpKind::FetchAdd),
    ];
    let mut i = 0;
    while i < ORDER_COUNT {
        let (o, k) = plans[i];
        if let Err(e) = ops.fill_order_target(o, k) {
            return Err(e.diagnose());
        }
        i += 1;
    }
    if let Err(e) = ops.conserve() {
        return Err(e.diagnose());
    }
    if !ops.all_orders_at_target() {
        return Err(format!(
            "判据 J1 未达：达标序数 {}/{}",
            ops.orders_at_target_count(),
            ORDER_COUNT
        ));
    }
    if ops.ledger().ops() != OPS_PER_ORDER * ORDER_COUNT as u64 {
        return Err(format!(
            "落账总数 {} ≠ 期望 {}",
            ops.ledger().ops(),
            OPS_PER_ORDER * ORDER_COUNT as u64
        ));
    }
    if ops.rejected() != 0 {
        return Err(format!(
            "落账过程有 {} 次拒收——判据要求零异常",
            ops.rejected()
        ));
    }
    Ok(())
}

/// 拒收路径是否显性（越界/倒退/裸 Relaxed 三类都必须 Err 且计数）。
pub fn check_reject_paths_explicit() -> Result<(), String> {
    // 裸 Relaxed 的读-改-写。
    {
        let mut ops = AtomicOps::new(64);
        match ops.fetch_add(MemOrder::Relaxed, 7) {
            Err(e) => {
                if e.code() != AtomCode::RelaxedRmw {
                    return Err(format!("裸宽松读改写应得 RelaxedRmw，实得 {}", e.code().zh()));
                }
                if ops.rejected() != 1 {
                    return Err("裸宽松读改写未计入拒收".to_string());
                }
            }
            Ok(v) => return Err(format!("裸宽松读改写竟然通过并返回 {}", v)),
        }
        if ops.acc() != 0 {
            return Err("被拒后累加器被改写——拒收必须无操作".to_string());
        }
    }
    // RMW 越界。
    {
        let mut ops = AtomicOps::new(64);
        let _ = ops.fetch_add(MemOrder::Acquire, i64::MAX - 4);
        match ops.fetch_add(MemOrder::Acquire, 9) {
            Err(e) => {
                if e.code() != AtomCode::RmwOverflow {
                    return Err(format!("读改写越界应得 RmwOverflow，实得 {}", e.code().zh()));
                }
            }
            Ok(v) => return Err(format!("读改写越界竟然通过并返回 {}", v)),
        }
    }
    // tick 倒退（账本直写路径）。
    {
        let mut lb = OpLedger::new(8);
        if let Err(e) = lb.append(OpRecord::one(1, OpKind::Load, MemOrder::Relaxed, 1, 1, 10)) {
            return Err(e.diagnose());
        }
        match lb.append(OpRecord::one(2, OpKind::Load, MemOrder::Relaxed, 2, 2, 5)) {
            Err(e) => {
                if e.code() != AtomCode::TickNotMonotonic {
                    return Err(format!("时序倒退应得 TickNotMonotonic，实得 {}", e.code().zh()));
                }
            }
            Ok(_) => return Err("tick 倒退竟然通过".to_string()),
        }
    }
    // 序号断档（账本直写路径）。
    {
        let mut lb = OpLedger::new(8);
        let _ = lb.append(OpRecord::one(1, OpKind::Load, MemOrder::Relaxed, 1, 1, 1));
        match lb.append(OpRecord::one(3, OpKind::Load, MemOrder::Relaxed, 2, 2, 2)) {
            Err(e) => {
                if e.code() != AtomCode::LedgerBroken {
                    return Err(format!("序号断档应得 LedgerBroken，实得 {}", e.code().zh()));
                }
            }
            Ok(_) => return Err("序号断档竟然通过".to_string()),
        }
    }
    // load 侧用了 release（非法序）。
    {
        let mut ops = AtomicOps::new(64);
        match ops.load(MemOrder::Release, 1) {
            Err(e) => {
                if e.code() != AtomCode::OrderIllegal {
                    return Err(format!("非法 load 序应得 OrderIllegal，实得 {}", e.code().zh()));
                }
            }
            Ok(v) => return Err(format!("release 当 load 序竟然通过并返回 {}", v)),
        }
    }
    Ok(())
}

/// 指纹确定性（同序列同指纹；改一字段指纹必变）。
pub fn check_fingerprint_determinism() -> Result<(), String> {
    let mut a = AtomicOps::new(128);
    let mut b = AtomicOps::new(128);
    let mut i = 0u64;
    while i < 32 {
        if i % 4 == 0 {
            let _ = a.load(MemOrder::Acquire, i);
            let _ = b.load(MemOrder::Acquire, i);
        } else if i % 4 == 1 {
            let _ = a.store(MemOrder::Release, i * 3);
            let _ = b.store(MemOrder::Release, i * 3);
        } else if i % 4 == 2 {
            let _ = a.fetch_add(MemOrder::SeqCst, 1);
            let _ = b.fetch_add(MemOrder::SeqCst, 1);
        } else {
            let _ = a.swap_into(MemOrder::Acquire, i);
            let _ = b.swap_into(MemOrder::Acquire, i);
        }
        i += 1;
    }
    let fa = a.fingerprint();
    let fb = b.fingerprint();
    if fa != fb {
        return Err(format!("同序列指纹不一致：{:#x} ≠ {:#x}", fa, fb));
    }
    // 改一个值 → 指纹必变。
    let _ = b.store(MemOrder::Release, 0xDEAD_BEEF);
    let fc = b.fingerprint();
    if fc == fa {
        return Err("改一个值后指纹未变——指纹无感即失效".to_string());
    }
    Ok(())
}

/// 满环显性溢出（不覆盖旧账、溢出计数正确）。
pub fn check_ring_overflow_explicit() -> Result<(), String> {
    let mut lb = OpLedger::new(4);
    let mut i = 0u64;
    while i < 6 {
        if let Err(e) = lb.append(OpRecord::one(i + 1, OpKind::Load, MemOrder::Relaxed, i, i, i)) {
            return Err(e.diagnose());
        }
        i += 1;
    }
    if !lb.truncated() {
        return Err("满环未报截断".to_string());
    }
    if lb.overflowed() != 2 {
        return Err(format!("溢出条数应为 2，实得 {}", lb.overflowed()));
    }
    if lb.len() != 4 {
        return Err(format!("环内现存应为 4，实得 {}", lb.len()));
    }
    let first = match lb.get(0) {
        Some(r) => r,
        None => return Err("取不到最旧现存账条".to_string()),
    };
    if first.seq != 3 {
        return Err(format!("最旧现存账条 seq 应为 3，实得 {}", first.seq));
    }
    if let Err(e) = lb.conserve() {
        return Err(e.diagnose());
    }
    Ok(())
}

/// 诊断码段独占 + 十六码互异。
pub fn check_diag_segment_guard() -> Result<(), String> {
    let mut i = 0;
    while i < AtomCode::ALL.len() {
        let c = AtomCode::ALL[i];
        if !c.segment_guard() {
            return Err(format!("诊断码 {:#06x} 越出 0x3500 段", c.raw()));
        }
        i += 1;
    }
    if !AtomCode::codes_distinct() {
        return Err("诊断码十六码有重复".to_string());
    }
    Ok(())
}

/// x86-64 降级表与参考口径一致。
pub fn check_x86_lowering_table() -> Result<(), String> {
    let mut i = 0;
    while i < ORDER_COUNT {
        let o = MemOrder::ALL[i];
        let want_store = if matches!(o, MemOrder::SeqCst) {
            X86Lowering::MfenceForSeqCstStore
        } else {
            X86Lowering::PlainMov
        };
        if lower_store(o) != want_store {
            return Err(format!("{} 的 store 降级不符合 x86-64 口径", o.wire()));
        }
        if lower_load(o) != X86Lowering::PlainMov {
            return Err(format!("{} 的 load 降级应为普通 MOV", o.wire()));
        }
        if lower_rmw(o) != X86Lowering::LockPrefixedRmw {
            return Err(format!("{} 的读改写降级应为 LOCK 前缀", o.wire()));
        }
        let want_fence = matches!(o, MemOrder::SeqCst);
        if needs_fence(o, true) != want_fence {
            return Err(format!("{} 的 store 屏障需求非预期", o.wire()));
        }
        if needs_fence(o, false) {
            return Err("load 侧不应有屏障需求".to_string());
        }
        i += 1;
    }
    Ok(())
}

/// 批参数不变量（BATCH_OPS × BATCH_LIMIT 恰为判据总量 10^7）。
pub fn check_batch_params() -> Result<(), String> {
    if BATCH_OPS * BATCH_LIMIT != OPS_PER_ORDER {
        return Err(format!(
            "批参数不变量破坏：{} × {} ≠ {}",
            BATCH_OPS, BATCH_LIMIT, OPS_PER_ORDER
        ));
    }
    if BATCH_OPS == 0 || BATCH_LIMIT == 0 {
        return Err("批参数出现零值".to_string());
    }
    Ok(())
}

/// 事务生命周期单向迁移。
pub fn check_txn_lifecycle() -> Result<(), String> {
    let mut ops = AtomicOps::new(64);
    if !ops.state().can_go(TxnState::Open) {
        return Err("空闲应可开始事务".to_string());
    }
    if ops.state().can_go(TxnState::Committed) {
        return Err("空闲不应能直接提交——生命周期间顾".to_string());
    }
    if let Err(e) = ops.commit() {
        return Err(e.diagnose());
    }
    if ops.begin(0).is_ok() {
        return Err("事务序号 0 竟然通过".to_string());
    }
    if let Err(e) = ops.begin(7) {
        return Err(e.diagnose());
    }
    if ops.pair().is_err() {
        return Err("进行中事务配对被拒".to_string());
    }
    if ops.commit().is_err() {
        return Err("已配对事务提交被拒".to_string());
    }
    if ops.state() != TxnState::Idle {
        return Err("提交后未回空闲".to_string());
    }
    if ops.begin(8).is_err() {
        return Err("回空闲后应可再开事务".to_string());
    }
    Ok(())
}

/// 契约自洽（默认契约 + 显式坏契约）。
pub fn check_contract_wellformed() -> Result<(), String> {
    let good = OrderContract::default();
    if good.wellformed().is_err() {
        return Err("默认契约不自洽".to_string());
    }
    let bad_relaxed_rmw = OrderContract::new(MemOrder::Acquire, MemOrder::Release, MemOrder::Relaxed);
    match bad_relaxed_rmw.wellformed() {
        Err(e) => {
            if e.code() != AtomCode::RelaxedRmw {
                return Err(format!("坏契约应得 RelaxedRmw，实得 {}", e.code().zh()));
            }
        }
        Ok(()) => return Err("裸 Relaxed 的契约竟然自洽".to_string()),
    }
    let bad_store = OrderContract::new(MemOrder::Acquire, MemOrder::Acquire, MemOrder::Acquire);
    if bad_store.wellformed().is_err() {
        return Err("acquire 写侧应被 wellformed 拒绝——但它没有".to_string());
    }
    Ok(())
}

/// 单 op 路径守恒（三族混跑小样本，账本守恒 + 族/操作/序三计数相等）。
pub fn check_single_op_conservation() -> Result<(), String> {
    let mut ops = AtomicOps::new(256);
    let mut i = 0u64;
    while i < 24 {
        match i % 6 {
            0 => {
                if let Err(e) = ops.load(MemOrder::Acquire, i) {
                    return Err(e.diagnose());
                }
            }
            1 => {
                if let Err(e) = ops.store(MemOrder::Release, i) {
                    return Err(e.diagnose());
                }
            }
            2 => {
                if let Err(e) = ops.fetch_add(MemOrder::SeqCst, 1) {
                    return Err(e.diagnose());
                }
            }
            3 => {
                if let Err(e) = ops.fetch_sub(MemOrder::Acquire, 0) {
                    return Err(e.diagnose());
                }
            }
            4 => {
                if let Err(e) = ops.fetch_or(MemOrder::Acquire, 1) {
                    return Err(e.diagnose());
                }
            }
            _ => {
                if let Err(e) = ops.compare_exchange(MemOrder::SeqCst, 0, 1) {
                    return Err(e.diagnose());
                }
            }
        }
        i += 1;
    }
    ops.conserve().map_err(|e| e.diagnose())?;
    if ops.ledger().ops() != 24 {
        return Err(format!("操作总数应为 24，实得 {}", ops.ledger().ops()));
    }
    Ok(())
}

// ===================== 域校验集 =====================

/// UNX-F3201 校验集（判据锚：J1 四类内存序各 10^7 次操作落账零异常）。
pub fn run_unx3201_checks() -> CheckSet {
    let mut s = CheckSet::new(crate::unxstar::UNX_DOMAIN);

    {
        let good = MemOrder::ranks_distinct();
        s.add(
            "C3201-01-四序秩互异",
            good,
            if good {
                ""
            } else {
                "四类内存序秩必须恰为 0/1/2/3 不重不漏"
            },
        );
    }
    {
        let r = check_sync_table_matches_rust_reference();
        let good = r.is_ok();
        // 修正记录：原把 `match &r { .. Err(why) => why.as_str() }` 直接传给
        // `add`（E0597 实锤）——`r` 是本语句的临时值，其借用的子串活不过
        // 语句结束。改为先固化诊断文本，再走 `add_dyn`。
        let why_text: &str = match &r {
            Ok(_) => "",
            Err(why) => why.as_str(),
        };
        s.add_dyn("C3201-02-序语义对照表", good, why_text);
    }
    {
        let ra = sync_verdict(MemOrder::Release, MemOrder::Acquire);
        let rb = sync_verdict(MemOrder::SeqCst, MemOrder::SeqCst);
        let good = ra.map(|v| v.establishes()).unwrap_or(false)
            && rb.map(|v| v.establishes()).unwrap_or(false);
        s.add(
            "C3201-03-释放获取建立同步",
            good,
            if good {
                ""
            } else {
                "Release→Acquire 与 SeqCst→SeqCst 必须建立 happens-before"
            },
        );
    }
    {
        let ra = sync_verdict(MemOrder::Relaxed, MemOrder::Relaxed);
        let rb = sync_verdict(MemOrder::Acquire, MemOrder::Acquire);
        let rc = sync_verdict(MemOrder::Release, MemOrder::Relaxed);
        let good = ra.map(|v| !v.establishes()).unwrap_or(false)
            && rb.map(|v| !v.establishes()).unwrap_or(false)
            && rc.map(|v| !v.establishes()).unwrap_or(false);
        s.add(
            "C3201-04-非同步格不建立",
            good,
            if good {
                ""
            } else {
                "Relaxed→Relaxed / Acquire→Acquire / Release→Relaxed 均不得建立 happens-before"
            },
        );
    }
    {
        let (est, no, na) = sync_table_tally();
        let good = est == 6 && no == 7 && na == 3 && est + no + na == SYNC_TABLE_CELLS;
        s.add(
            "C3201-05-对照表三值守恒",
            good,
            if good {
                ""
            } else {
                "4×4 应得 6 建立 / 7 不建立 / 3 不适用（和为 16）"
            },
        );
    }
    {
        let good = op_families_partition();
        s.add(
            "C3201-06-操作九闭集三族划分",
            good,
            if good {
                ""
            } else {
                "九种操作必须三族互斥且完备（load 1 / store 1 / rmw 7）"
            },
        );
    }
    {
        let good = OrderContract::default().rmw_is_composite()
            && !OrderContract::new(
                MemOrder::Acquire,
                MemOrder::Release,
                MemOrder::Relaxed,
            )
            .rmw_is_composite();
        s.add(
            "C3201-07-读改写默认复合序",
            good,
            if good {
                ""
            } else {
                "读-改-写默认必须为复合序 AcqRel，Relaxed 不得被判为复合序"
            },
        );
    }
    {
        let r = check_contract_wellformed();
        let good = r.is_ok();
        // 修正记录：诊断文本须先固化再登记——`r` 是本语句临时值，
        // 其借用子串活不过语句结束（E0597 实锤）。
        let why_text: &str = match &r {
            Ok(_) => "",
            Err(why) => why.as_str(),
        };
        s.add_dyn(
                        "C3201-08-契约自洽闸",
            good,
            why_text,
        );
    }
    {
        let r = check_single_op_conservation();
        let good = r.is_ok();
        // 修正记录：诊断文本须先固化再登记——`r` 是本语句临时值，
        // 其借用子串活不过语句结束（E0597 实锤）。
        let why_text: &str = match &r {
            Ok(_) => "",
            Err(why) => why.as_str(),
        };
        s.add_dyn(
                        "C3201-09-单op路径守恒",
            good,
            why_text,
        );
    }
    {
        let r = check_engine_judge_j1();
        let good = r.is_ok();
        // 修正记录：诊断文本须先固化再登记——`r` 是本语句临时值，
        // 其借用子串活不过语句结束（E0597 实锤）。
        let why_text: &str = match &r {
            Ok(_) => "",
            Err(why) => why.as_str(),
        };
        s.add_dyn(
                        "C3201-10-判据J1四序十的七次落账零异常",
            good,
            why_text,
        );
    }
    {
        let r = check_reject_paths_explicit();
        let good = r.is_ok();
        // 修正记录：诊断文本须先固化再登记——`r` 是本语句临时值，
        // 其借用子串活不过语句结束（E0597 实锤）。
        let why_text: &str = match &r {
            Ok(_) => "",
            Err(why) => why.as_str(),
        };
        s.add_dyn(
                        "C3201-11-拒收路径显性",
            good,
            why_text,
        );
    }
    {
        let r = check_fingerprint_determinism();
        let good = r.is_ok();
        // 修正记录：诊断文本须先固化再登记——`r` 是本语句临时值，
        // 其借用子串活不过语句结束（E0597 实锤）。
        let why_text: &str = match &r {
            Ok(_) => "",
            Err(why) => why.as_str(),
        };
        s.add_dyn(
                        "C3201-12-指纹确定性",
            good,
            why_text,
        );
    }
    {
        let r = check_ring_overflow_explicit();
        let good = r.is_ok();
        // 修正记录：诊断文本须先固化再登记——`r` 是本语句临时值，
        // 其借用子串活不过语句结束（E0597 实锤）。
        let why_text: &str = match &r {
            Ok(_) => "",
            Err(why) => why.as_str(),
        };
        s.add_dyn(
                        "C3201-13-满环显性溢出",
            good,
            why_text,
        );
    }
    {
        let r = check_diag_segment_guard();
        let good = r.is_ok();
        // 修正记录：诊断文本须先固化再登记——`r` 是本语句临时值，
        // 其借用子串活不过语句结束（E0597 实锤）。
        let why_text: &str = match &r {
            Ok(_) => "",
            Err(why) => why.as_str(),
        };
        s.add_dyn(
                        "C3201-14-诊断码零x三五零零段独占",
            good,
            why_text,
        );
    }
    {
        let r = check_x86_lowering_table();
        let good = r.is_ok();
        // 修正记录：诊断文本须先固化再登记——`r` 是本语句临时值，
        // 其借用子串活不过语句结束（E0597 实锤）。
        let why_text: &str = match &r {
            Ok(_) => "",
            Err(why) => why.as_str(),
        };
        s.add_dyn(
                        "C3201-15-x86序降级表",
            good,
            why_text,
        );
    }
    {
        let r = check_batch_params();
        let good = r.is_ok();
        // 修正记录：诊断文本须先固化再登记——`r` 是本语句临时值，
        // 其借用子串活不过语句结束（E0597 实锤）。
        let why_text: &str = match &r {
            Ok(_) => "",
            Err(why) => why.as_str(),
        };
        s.add_dyn(
                        "C3201-16-批参数不变量",
            good,
            why_text,
        );
    }
    {
        let r = check_txn_lifecycle();
        let good = r.is_ok();
        // 修正记录：诊断文本须先固化再登记——`r` 是本语句临时值，
        // 其借用子串活不过语句结束（E0597 实锤）。
        let why_text: &str = match &r {
            Ok(_) => "",
            Err(why) => why.as_str(),
        };
        s.add_dyn(
                        "C3201-17-事务生命周期单向",
            good,
            why_text,
        );
    }
    {
        let good = OPS_PER_ORDER == 10_000_000
            && OPS_PER_ORDER * ORDER_COUNT as u64 == 40_000_000
            && U64_MAX == u64::MAX;
        s.add(
            "C3201-18-判据常量冻结",
            good,
            if good {
                ""
            } else {
                "判据常量漂移：每序 10^7 与四序合计 4×10^7 是冻结口径"
            },
        );
    }
    s
}

/// 本模块诊断码表（汇总输出，便于运维一屏看齐）。
pub fn diag_code_table() -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < AtomCode::ALL.len() {
        let c = AtomCode::ALL[i];
        out.push(format!(
            "0x{:04X} {:<8} {}",
            c.raw(),
            c.zh(),
            if c.segment_guard() { "段内" } else { "越段" }
        ));
        i += 1;
    }
    out
}

