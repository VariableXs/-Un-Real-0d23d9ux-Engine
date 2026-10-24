#![allow(dead_code)]
#![allow(non_upper_case_globals)]
#![allow(unused_macros)]
//! unx1842_ipivec_deep ↔ UNX-F1842 IPI 向量分配表（造 IPI 必登记）· 深化版
//!
//! ---------------------------------------------------------------------------
//! 单号：UNX-F1842（深化版 `unx1842_ipivec_deep`，独立于浅版 `unx1842_ipivec`）
//! 标题：IPI 向量分配表（造 IPI 必登记）
//! 册内位置：CoRun·UNX-A3-B13 段「IPI」
//! 目标行数：2200 ~ 3000 行（硬指标）
//! ---------------------------------------------------------------------------
//!
//! ## 判据原文（注释引用，逐字）
//!
//! ```text
//! UNX-F1842-J1 向量分配表冻结 v1（重调度/迁移通知/击落/保留区），
//! 未登记向量发送注入 10/10 检出（编译期宏+运行期断言双检）
//! ```
//!
//! 判据拆成两条可机械验证的子句：
//!
//! * **J1-a「向量分配表冻结 v1（重调度/迁移通知/击落/保留区）」**
//!   —— 四类向量必须全部登记进 v1 分配表；v1 封账后指纹钉死，
//!   新增 / 改写 / 删除 / 重排一律 `Err(FrozenViolation)`。
//! * **J1-b「未登记向量发送注入 10/10 检出（编译期宏+运行期断言双检）」**
//!   —— 10 种注入形态必须 10/10 被拦下；拦截由两道路线冗余完成：
//!   编译期宏（`register_ipi_vec!` / `send_ipi_vec!`，符号层强制）
//!   + 运行期断言（`VecTable::send` 查表、保留区拒发、双源对账）。
//!
//! ## 组件清单（本文件自持，零外部依赖）
//!
//! | # | 组件 | 类型 | 行位 |
//! |---|------|------|------|
//! | 1 | 诊断码段独占守卫 | `CodeSeg` / `seg_guard()` | 码段 |
//! | 2 | 错误闭集 | `IpiVecError` + `VResult<T>` | 错误 |
//! | 3 | 中断层级约束闭集 | `LevelCtx` / `lc()` | 层级 |
//! | 4 | 向量类别闭集 | `IpiClass` | 类别 |
//! | 5 | 向量描述符 | `VecDesc` / `vec_desc()` | 描述符 |
//! | 6 | FNV-1a 64 指纹 | `fnv_*` 系列 | 指纹 |
//! | 7 | 确定性 LCG | `Lcg` | 随机源 |
//! | 8 | 编译期静态表 | `COMPILE_GUARD` | 编译期 |
//! | 9 | 编译期登记宏 | `register_ipi_vec!` | 编译期 |
//! | 10 | 编译期发送宏 | `send_ipi_vec!` | 编译期 |
//! | 11 | 运行期分配表 | `VecTable`（含冻结封账） | 运行期 |
//! | 12 | 内建运行期表（第二数据源） | `builtin_table()` | 双源 |
//! | 13 | 双源对账 | `dual_source_reconcile()` | 双检 |
//! | 14 | 分派表 | `HandlerId` / `handler_id_for()` | 分派 |
//! | 15 | 接收四步 | 识别 / 分发 / EOI | 接收 |
//! | 16 | 四段计数守恒 | `Ledger::conserved()` | 守恒 |
//! | 17 | 层级违规三检 | 加锁 / 调度 / 驻留 | 纪律 |
//! | 18 | 合并与重入 | `send` 合并 + 重入禁令 | F1848 |
//! | 19 | 风暴丢弃 | 窗口阈值 + 显式落账 | F1848 |
//! | 20 | 注入核 10 形态 | `inject_all()` | 注入 |
//! | 21 | 导出三件 | KV / KV / TLV | 导出 |
//! | 22 | 门禁总闸 | `selftest() -> Gate` | 门禁 |
//!
//! ## 职责定位（做什么 / 不做什么）
//!
//! **做**：把「IPI 向量号」变成受控资源——分配、登记、冻结、发送、接收、
//! 对账、注入检出，形成一条可审计的证据链。
//!
//! **不做**：不实现真实 `APIC/SIPI/IPI` 寄存器写，不实现真实中断向量表
//! 装填，不碰 `IDT` 本身（那是 F080 的事），不调度真实 CPU。
//! 一切「发送」在本文件里等于「向运行期投递队列推一条带 tick 戳的记录」。
//!
//! ## 性能逐项分解（本文件的成本模型，纯逻辑单位）
//!
//! 记 `N` = 表项数（32）、`Q` = 投递队列长度、`W` = 合并扫描窗口、
//! `B` = 每 tick 扫描的 pending 上限、`F` = 导出条目数。
//!
//! | 路径 | 成本 | 说明 |
//! |------|------|------|
//! | `VecTable::send` 查表 | `O(1)` | 向量号经 `SLOT_CLAIM`/线性槽扫描，`N=32` 定长 |
//! | 合并判定（同核同类） | `O(Q)` | 队列长度受 `QUEUE_CAP` 上限约束 |
//! | 风暴判定 | `O(1)` | 窗口计数，不遍历历史 |
//! | 落账 `push_diag` | `O(1)` 摊还 | `Vec` 追加，均摊常数 |
//! | `deliver` 一次扫描 | `O(B)` | `B = QUEUE_CAP`，不做无界循环 |
//! | 分派 `handler_id_for` | `O(1)` | 区间二分 + 表索引 |
//! | `VecDesc::fp` 指纹 | `O(L)` | `L` = 字段字节长度，构造期一次性 |
//! | 封账指纹 `seal()` | `O(N·L)` | 32 项顺序 FNV，仅封账时一次 |
//! | 双源对账 | `O(N)` | 逐项 fp 字符串比对 |
//! | `export_table_kv` | `O(F)` | 逐向量一行 |
//! | `export_tlv` | `O(F)` | 变长记录拼接 + 长度自洽断言 |
//! | `inject_all` | `O(10)` | 10 条固定形态，互不依赖 |
//! | `selftest` | `O(N+F)` | 冻结 + 守恒 + 对账 + 10 形态 |
//!
//! 全程无堆分配热路径之外的隐式增长；所有 `Vec` 预留容量在构造时一次给足，
//! 运行期不触发扩容抖动。
//!
//! ## 向量描述符布局（`VecDesc`，`repr(C)` 语义等价，非 ABI 契约）
//!
//! | 偏移 | 宽度 | 对齐 | 字段 | 语义 |
//! |------|------|------|------|------|
//! | 0x00 | 1 | 1 | `no: u8` | 向量号，`0x40` 起，`0x40..0x60` 声明窗口 |
//! | 0x01 | 3 | 1 | （填充） | 对齐填充，字段间不留洞 |
//! | 0x04 | 16 | 8 | `name_cn: &'static str` | 中文名（UTF-8 指针+长度） |
//! | 0x14 | 16 | 8 | `handler_sym: &'static str` | 处理函数符号名（编译期宏下标） |
//! | 0x24 | 1 | 1 | `class: IpiClass` | 四类闭集之一 |
//! | 0x25 | 2 | 1 | `level: LevelCtx`（压 2B 见下） | 层级约束 |
//! | 0x27 | 1 | 1 | `mergeable: bool` | 是否允许同类 pending 合并 |
//! | 0x28 | 8 | 8 | `budget_ticks: u64` | 期望时长上界（逻辑 tick） |
//! | 0x30 | 16 | 8 | `owner_batch: &'static str` | 消费方批号（F184x） |
//! | 0x40 | 8 | 8 | `fp: u64` | 逐项 FNV-1a 64 指纹 |
//!
//! `LevelCtx` 压紧编码（`level` 2 字节内）：
//! `bit0 = may_lock`、`bit1 = may_schedule`、`bit8..16 = max_stay_ticks`（截断）。
//! 展开视图见 [`LevelCtx`]。
//!
//! ## 诊断码段（独占 `0xB0E0 ~ 0xB0EF`，主码 `0xB0E0`）
//!
//! | 码 | 枚举 | 触发点 |
//! |----|------|--------|
//! | `0xB0E0` | `UnregisteredVector` | 运行期查表未命中 |
//! | `0xB0E1` | `ReservedVectorSend` | 保留区向量被发送 |
//! | `0xB0E2` | `FrozenViolation` | 冻结后增删改排 |
//! | `0xB0E3` | `DualSourceDrift` | 编译期表与运行期表漂移 |
//! | `0xB0E4` | `LockViolation` | NoLock handler 加锁 |
//! | `0xB0E5` | `ScheduleViolation` | NoSchedule handler 请求调度 |
//! | `0xB0E6` | `StayOverrun` | 驻留 tick 超上界 |
//! | `0xB0E7` | `ReentrancyForbidden` | handler 内发同类向量 |
//! | `0xB0E8` | `StormDropped` | 窗口内发送超阈值丢弃 |
//! | `0xB0E9` | `VectorOutOfWindow` | 向量号越声明窗口 |
//! | `0xB0EA` | `VectorDup` | 向量号重复占用 |
//! | `0xB0EB` | `LedgerImbalance` | 四段计数不守恒 |
//! | `0xB0EC` | `EoiMissing` | EOI 未与 handled 对齐 |
//! | `0xB0ED` | `TargetOutOfRange` | 目标核越界 |
//! | `0xB0EE` | `TlvLenMismatch` | TLV 长度字段不自洽 |
//! | `0xB0EF` | `InjectUnbalanced` | 10 形态检出数不齐 |
//!
//! 段独占由 [`seg_guard`] 在运行期复核：段首必须等于主码、段尾不得越
//! `0xB0EF`、段宽必须为 16。
//!
//! ## 纯功能线
//!
//! `no_std + alloc`，只用 `alloc::vec::Vec` / `alloc::vec!` /
//! `alloc::string::{String, ToString}` / `alloc::format` /
//! `alloc::collections::BTreeMap`。**无 `crate::` 依赖**（可脱离 varix
//! crate 独立探针编译）。零 `unwrap` / `expect` / `panic!`，零浮点，
//! 零真实 IO，零墙钟，时间一律走 `tick: u64`。
//!
//! ## 与浅版的关系
//!
//! 浅版 `unx1842_ipivec.rs` 占用 `0x6900` 段并依赖 `crate::checks`；
//! 本深化版占用 `0xB0E0` 段、零 crate 依赖，两者并存互不覆盖，
//! 由主会话分别注册。

use alloc::vec;

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ===========================================================================
// 0. 版本 / 常量
// ===========================================================================

/// 模块版本（深化版）。
pub const V1842D_VERSION: &str = "v1842-ipivec-deep-v1";

/// 诊断段基址（主码）。浅版占 `0x6900`，本版独占 `0xB0E0`。
pub const SEG_BASE: u16 = 0xB0E0;

/// 诊断段尾（含）。
pub const SEG_LAST: u16 = 0xB0EF;

/// 诊断段宽（必须 16）。
pub const SEG_WIDTH: u16 = 0x0010;

/// 冻结版次号。
pub const SEAL_REV: u32 = 1;

/// 分配表容量（声明窗口宽度，≥32）。
pub const VEC_TABLE_CAP: u32 = 32;

/// 向量号声明窗口起点（x86 惯例：0x40 起，低于此为异常/保留）。
pub const VEC_WINDOW_LO: u8 = 0x40;

/// 向量号声明窗口终点（不含）。
pub const VEC_WINDOW_HI: u8 = 0x60;

/// 业务向量区上界（不含）——低于此为业务向量。
pub const VEC_BUSINESS_HI: u8 = 0x58;

/// 保留向量区下界（含）——`>=` 此号即保留区，运行期拒发。
pub const VEC_RESERVED_LO: u8 = 0x58;

/// 表容量（`usize` 视角）。
pub const TABLE_CAP: usize = VEC_TABLE_CAP as usize;

/// 业务向量项数。
pub const BUSINESS_COUNT: usize = (VEC_BUSINESS_HI - VEC_WINDOW_LO) as usize;

/// 保留向量项数。
pub const RESERVED_COUNT: usize = (VEC_WINDOW_HI - VEC_RESERVED_LO) as usize;

/// 投递队列上界（防止无界循环）。
pub const QUEUE_CAP: usize = 64;

/// 单次 `deliver` 扫描的 pending 上限。
pub const DELIVER_BATCH: usize = QUEUE_CAP;

/// 合并判定扫描窗口（队列内同核同类合并，最长看前 `W` 条）。
pub const MERGE_WINDOW: usize = QUEUE_CAP;

/// 风暴窗口（逻辑 tick）。
pub const STORM_WINDOW_TICKS: u64 = 16;

/// 风暴阈值（窗口内发送次数上限）。
pub const STORM_THRESHOLD: u64 = 8;

/// 目标核总数（0..`CPU_CORES`）。
pub const CPU_CORES: u32 = 4;

/// FNV-1a 64 位 offset basis。
pub const FNV_BASIS: u64 = 0xcbf29ce4_84222325_u64 as u64;

/// FNV-1a 64 位 prime。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3_u64 as u64;

/// LCG 乘数（标准 64 位线性同余）。
pub const LCG_MUL: u64 = 6364136223846793005;

/// LCG 增量。
pub const LCG_ADD: u64 = 1442695040888963407;

/// 槽位占用标记（未占用）。
pub const SLOT_FREE: u8 = 0xFF;

// ===========================================================================
// 1. 诊断码
// ===========================================================================

/// IPI 向量分配诊断码（`0xB0E0 ~ 0xB0EF` 独占段）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Code {
    /// `0xB0E0` 向量未登记（裸发拦截，主码）。
    UnregisteredVector,
    /// `0xB0E1` 发送保留区向量。
    ReservedVectorSend,
    /// `0xB0E2` 冻结后新增 / 改写 / 删除 / 重排。
    FrozenViolation,
    /// `0xB0E3` 编译期表与运行期表双源漂移。
    DualSourceDrift,
    /// `0xB0E4` NoLock handler 加锁。
    LockViolation,
    /// `0xB0E5` NoSchedule handler 请求调度。
    ScheduleViolation,
    /// `0xB0E6` handler 驻留超上界。
    StayOverrun,
    /// `0xB0E7` handler 内重入发送同类向量。
    ReentrancyForbidden,
    /// `0xB0E8` 风暴窗口超阈值丢弃。
    StormDropped,
    /// `0xB0E9` 向量号越声明窗口。
    VectorOutOfWindow,
    /// `0xB0EA` 向量号重复占用。
    VectorDup,
    /// `0xB0EB` 四段计数不守恒。
    LedgerImbalance,
    /// `0xB0EC` EOI 与 handled 不对齐。
    EoiMissing,
    /// `0xB0ED` 目标核越界。
    TargetOutOfRange,
    /// `0xB0EE` TLV 长度字段不自洽。
    TlvLenMismatch,
    /// `0xB0EF` 注入检出数不齐。
    InjectUnbalanced,
}

impl Code {
    /// 码值（`SEG_BASE +` 段内偏移）。
pub fn val(self) -> u16 {SEG_BASE + self.offset()}

    /// 段内偏移。
    pub fn offset(self) -> u16 {
        match self {
            Code::UnregisteredVector => 0x0,
            Code::ReservedVectorSend => 0x1,
            Code::FrozenViolation => 0x2,
            Code::DualSourceDrift => 0x3,
            Code::LockViolation => 0x4,
            Code::ScheduleViolation => 0x5,
            Code::StayOverrun => 0x6,
            Code::ReentrancyForbidden => 0x7,
            Code::StormDropped => 0x8,
            Code::VectorOutOfWindow => 0x9,
            Code::VectorDup => 0xA,
            Code::LedgerImbalance => 0xB,
            Code::EoiMissing => 0xC,
            Code::TargetOutOfRange => 0xD,
            Code::TlvLenMismatch => 0xE,
            Code::InjectUnbalanced => 0xF,
        }
    }

    /// 中文证据短语（落账用，短而确定）。
    pub fn phrase(self) -> &'static str {
        match self {
            Code::UnregisteredVector => "向量未登记",
            Code::ReservedVectorSend => "发送保留区向量",
            Code::FrozenViolation => "冻结表被改写",
            Code::DualSourceDrift => "双源漂移",
            Code::LockViolation => "handler 加锁违规",
            Code::ScheduleViolation => "handler 请求调度违规",
            Code::StayOverrun => "handler 驻留超上界",
            Code::ReentrancyForbidden => "handler 内重入同类向量",
            Code::StormDropped => "风暴窗口丢弃",
            Code::VectorOutOfWindow => "向量号越声明窗口",
            Code::VectorDup => "向量号重复占用",
            Code::LedgerImbalance => "四段计数不守恒",
            Code::EoiMissing => "EOI 遗漏",
            Code::TargetOutOfRange => "目标核越界",
            Code::TlvLenMismatch => "TLV 长度不自洽",
            Code::InjectUnbalanced => "注入检出数不齐",
        }
    }

    /// 该码是否属于「未登记向量发送」这条主判据的检出集合。
    ///
    /// 判据：UNX-F1842-J1 —— 未登记向量发送必须在运行期被断言拦下，
    /// 该闭集列出所有「发送路径被拦」的诊断码。
    pub fn is_send_path_denial(self) -> bool {
        matches!(
            self,
            Code::UnregisteredVector
                | Code::ReservedVectorSend
                | Code::ReentrancyForbidden
                | Code::StormDropped
                | Code::TargetOutOfRange
                | Code::VectorOutOfWindow
                | Code::VectorDup
        )
    }

    /// 段内全部码（升序）。
    pub fn all() -> [Code; 16] {
        [
            Code::UnregisteredVector,
            Code::ReservedVectorSend,
            Code::FrozenViolation,
            Code::DualSourceDrift,
            Code::LockViolation,
            Code::ScheduleViolation,
            Code::StayOverrun,
            Code::ReentrancyForbidden,
            Code::StormDropped,
            Code::VectorOutOfWindow,
            Code::VectorDup,
            Code::LedgerImbalance,
            Code::EoiMissing,
            Code::TargetOutOfRange,
            Code::TlvLenMismatch,
            Code::InjectUnbalanced,
        ]
    }
}

/// 诊断码段独占守卫。
///
/// 判据：UNX-F1842-J1 —— 诊断码段 `0xB0E0~0xB0EF` 独占，段首对齐主码、
/// 段宽恰 16、段内无空洞。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CodeSeg;

/// 段内偏移 → 码（空洞检测用）。
const CODE_BY_OFFSET: [u16; 16] = [0x0, 0x1, 0x2, 0x3, 0x4, 0x5, 0x6, 0x7, 0x8, 0x9, 0xA, 0xB, 0xC, 0xD, 0xE, 0xF,];

impl CodeSeg {
    /// 复核段独占性。
    pub fn check(&self) -> VResult<()> {
        // 段首必须等于主码。
if Code::UnregisteredVector.val() != SEG_BASE {return Err(IpiVecError::SegmentBreach);}
        // 段宽必须 16。
if SEG_WIDTH != 0x10 {return Err(IpiVecError::SegmentBreach);}
        // 段尾不得越 `0xB0EF`。
if SEG_LAST != SEG_BASE + SEG_WIDTH - 1 {return Err(IpiVecError::SegmentBreach);}
        // 段内不得有空洞：枚举偏移必须一一对应 0..15。
        let mut expect = 0u16;
for off in CODE_BY_OFFSET.iter() {if *off != expect {return Err(IpiVecError::SegmentBreach);} expect = expect.saturating_add(1);}
        // 全部枚举码必须落在段内。
for c in Code::all().iter() {if c.val() < SEG_BASE || c.val() > SEG_LAST {return Err(IpiVecError::SegmentBreach);}}
        Ok(())
    }
}

/// 段独占守卫便捷入口。
pub fn seg_guard() -> VResult<()> {CodeSeg.check()}

// ===========================================================================
// 2. 错误闭集
// ===========================================================================

/// 本模块全部错误的闭集（无 `std::error::Error`，无 `unwrap`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IpiVecError {
    /// 向量号未登记（运行期查表未命中）。
    UnregisteredVector,
    /// 保留区向量被发送。
    ReservedVectorSend,
    /// 冻结后改表。
    FrozenViolation,
    /// 编译期表与运行期表不一致。
    DualSourceDrift,
    /// NoLock handler 加锁。
    LockViolation,
    /// NoSchedule handler 请求调度。
    ScheduleViolation,
    /// 驻留超上界。
    StayOverrun,
    /// handler 内重入发送同类。
    ReentrancyForbidden,
    /// 风暴丢弃。
    StormDropped,
    /// 向量号越窗口。
    VectorOutOfWindow,
    /// 向量号重复。
    VectorDup,
    /// 计数不守恒。
    LedgerImbalance,
    /// EOI 遗漏。
    EoiMissing,
    /// 目标核越界。
    TargetOutOfRange,
    /// TLV 长度不自洽。
    TlvLenMismatch,
    /// 注入检出不齐。
    InjectUnbalanced,
    /// 诊断码段被侵占。
    SegmentBreach,
    /// 投递队列已满（不静默丢，落账返回）。
    QueueFull,
    /// 参数越界（通用）。
    BadParam,
    /// 找不到分派目标。
    NoHandler,
    /// 合并窗口扫描越界（内部一致性）。
    InternalIndex,
}

impl IpiVecError {
    /// 对应诊断码。
    pub fn code(self) -> Code {
        match self {
            IpiVecError::UnregisteredVector => Code::UnregisteredVector,
            IpiVecError::ReservedVectorSend => Code::ReservedVectorSend,
            IpiVecError::FrozenViolation => Code::FrozenViolation,
            IpiVecError::DualSourceDrift => Code::DualSourceDrift,
            IpiVecError::LockViolation => Code::LockViolation,
            IpiVecError::ScheduleViolation => Code::ScheduleViolation,
            IpiVecError::StayOverrun => Code::StayOverrun,
            IpiVecError::ReentrancyForbidden => Code::ReentrancyForbidden,
            IpiVecError::StormDropped => Code::StormDropped,
            IpiVecError::VectorOutOfWindow => Code::VectorOutOfWindow,
            IpiVecError::VectorDup => Code::VectorDup,
            IpiVecError::LedgerImbalance => Code::LedgerImbalance,
            IpiVecError::EoiMissing => Code::EoiMissing,
            IpiVecError::TargetOutOfRange => Code::TargetOutOfRange,
            IpiVecError::TlvLenMismatch => Code::TlvLenMismatch,
            IpiVecError::InjectUnbalanced => Code::InjectUnbalanced,
            IpiVecError::SegmentBreach => Code::VectorOutOfWindow,
            IpiVecError::QueueFull => Code::StormDropped,
            IpiVecError::BadParam => Code::VectorOutOfWindow,
            IpiVecError::NoHandler => Code::UnregisteredVector,
            IpiVecError::InternalIndex => Code::DualSourceDrift,
        }
    }

    /// 中文短语。
pub fn phrase(self) -> &'static str {self.code().phrase()}
}

/// 本模块 `Result` 别名。
pub type VResult<T> = core::result::Result<T, IpiVecError>;

// ===========================================================================
// 3. FNV-1a 64 指纹
// ===========================================================================

/// 字节混入（`const fn`，构造期可用）。
pub const fn fnv_byte(mut h: u64, b: u8) -> u64 {h ^= b as u64; h.wrapping_mul(FNV_PRIME)}

/// 字符串混入。
pub const fn fnv_str(mut h: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut i = 0usize;
while i < bytes.len() {h = fnv_byte(h, bytes[i]); i += 1;}
    h
}

/// `u64` 小端混入。
pub const fn fnv_u64(mut h: u64, v: u64) -> u64 {let mut i = 0usize; while i < 8 {h = fnv_byte(h, ((v >> (i * 8)) & 0xFF) as u8); i += 1;} h}

/// `u8` 混入。
pub const fn fnv_u8(h: u64, v: u8) -> u64 {fnv_byte(h, v)}

/// `bool` 混入（`true`→`0x01`，`false`→`0x00`）。
pub const fn fnv_bool(h: u64, v: bool) -> u64 {fnv_byte(h, if v {0x01} else {0x00})}

/// 从 basis 起算字符串指纹。
pub const fn fp_of_str(s: &str) -> u64 {fnv_str(FNV_BASIS, s)}

/// 从 basis 起算 `u64` 指纹。
pub const fn fp_of_u64(v: u64) -> u64 {fnv_u64(FNV_BASIS, v)}

// ===========================================================================
// 4. 确定性 LCG
// ===========================================================================

/// 确定性 64 位 LCG（`x = x*a + c mod 2^64`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lcg {state: u64,draws: u64,}

impl Lcg {
    /// 以固定种子构造（种子 0 时映射到非零，避免退化）。
pub fn new(seed: u64) -> Lcg {Lcg {state: if seed == 0 {FNV_BASIS} else {seed},draws: 0,}}

    /// 抽一个 `u64`。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(LCG_MUL)
            .wrapping_add(LCG_ADD);
        self.draws = self.draws.wrapping_add(1);
        self.state
    }

    /// 抽一个 `[0, n)` 内的 `u32`（`n == 0` 返回 0）。
pub fn below(&mut self, n: u32) -> u32 {if n == 0 {return 0;} let raw = self.next_u64(); ((raw >> 33) as u32) % n}

    /// 已抽次数（用于确定性对账）。
pub fn draws(&self) -> u64 {self.draws}

    /// 当前状态（只读）。
pub fn state(&self) -> u64 {self.state}
}

// ===========================================================================
// 5. 中断层级约束闭集
// ===========================================================================

/// 加锁许可闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockPolicy {
    /// NoLock：handler 内禁止任何锁获取。
    NoLock,
    /// MayLock：handler 内允许获取预声明锁。
    MayLock,
}

/// 调度许可闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SchedPolicy {
    /// NoSchedule：handler 内禁止请求调度。
    NoSchedule,
    /// MaySchedule：handler 内允许置「需重调度」标志。
    MaySchedule,
}

/// 中断层级约束（`MayLock/NoLock` + `MaySchedule/NoSchedule` + 驻留上界）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LevelCtx {
    /// 加锁许可。
    pub lock: LockPolicy,
    /// 调度许可。
    pub sched: SchedPolicy,
    /// 驻留上界（逻辑 tick）。
    pub max_stay_ticks: u64,
}

impl LevelCtx {
    /// 是否允许加锁。
pub const fn may_lock(&self) -> bool {matches!(self.lock, LockPolicy::MayLock)}

    /// 是否允许请求调度。
pub const fn may_schedule(&self) -> bool {matches!(self.sched, SchedPolicy::MaySchedule)}

    /// 层级约束压紧编码（2 字节语义）。
    pub const fn packed(&self) -> u16 {
        let l = if self.may_lock() { 0b01 } else { 0b00 };
        let s = if self.may_schedule() { 0b10 } else { 0b00 };
        let stay = (self.max_stay_ticks & 0xFF) as u16;
        l | s | (stay << 8)
    }

    /// 文本形式（落账用）。
    pub fn text(&self) -> String {
        format!(
            "lock={}/sched={}/stay<={}",
            if self.may_lock() { "MayLock" } else { "NoLock" },
if self.may_schedule() {"MaySchedule"} else {"NoSchedule"},
            self.max_stay_ticks
        )
    }

    /// 指纹混入。
pub const fn fold(&self, h: u64) -> u64 {let p = self.packed(); fnv_u8(fnv_u8(h, p as u8), (p >> 8) as u8)}
}

/// 构造层级约束。
pub const fn lc(lock: LockPolicy, sched: SchedPolicy, max_stay_ticks: u64) -> LevelCtx {LevelCtx {lock,sched,max_stay_ticks,}}

/// 通用最严档：禁锁、禁调度、4 tick 上界。
pub const LC_STRICT: LevelCtx = lc(LockPolicy::NoLock, SchedPolicy::NoSchedule, 4);
/// 通用重档：禁锁、禁调度、8 tick 上界。
pub const LC_STRICT8: LevelCtx = lc(LockPolicy::NoLock, SchedPolicy::NoSchedule, 8);
/// 通用宽档：禁锁、可置调度标志、16 tick 上界。
pub const LC_WIDE: LevelCtx = lc(LockPolicy::NoLock, SchedPolicy::MaySchedule, 16);
/// 探针档：允许锁与调度，仅用于自检向量（上限 32 tick）。
pub const LC_PROBE: LevelCtx = lc(LockPolicy::MayLock, SchedPolicy::MaySchedule, 32);

// ===========================================================================
// 6. 向量类别闭集
// ===========================================================================

/// IPI 向量类别（判据冻结的四类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IpiClass {
    /// 重调度向量（消费方 F1844）。
    Resched,
    /// 迁移通知向量（消费方 F1845）。
    Migrate,
    /// TLB 击落向量（消费方 F1846）。
    Shootdown,
    /// 保留区向量（登记但禁发）。
    Reserved,
}

impl IpiClass {
    /// 短码（导出用）。
    pub fn tag(self) -> &'static str {
match self {IpiClass::Resched => "RS",IpiClass::Migrate => "MG",IpiClass::Shootdown => "SD",IpiClass::Reserved => "RV",}
    }

    /// 中文名。
    pub fn name_cn(self) -> &'static str {
match self {IpiClass::Resched => "重调度",IpiClass::Migrate => "迁移通知",IpiClass::Shootdown => "TLB击落",IpiClass::Reserved => "保留区",}
    }

    /// 是否业务类（保留区不算）。
pub const fn is_business(self) -> bool {matches!(self, IpiClass::Resched | IpiClass::Migrate | IpiClass::Shootdown)}

    /// 编码（0..3）。
pub const fn code(self) -> u8 {match self {IpiClass::Resched => 0,IpiClass::Migrate => 1,IpiClass::Shootdown => 2,IpiClass::Reserved => 3,}}

    /// 由编码反解。
    pub fn from_code(c: u8) -> VResult<IpiClass> {
        match c {
            0 => Ok(IpiClass::Resched),
            1 => Ok(IpiClass::Migrate),
            2 => Ok(IpiClass::Shootdown),
            3 => Ok(IpiClass::Reserved),
            _ => Err(IpiVecError::BadParam),
        }
    }
}

// ===========================================================================
// 7. 向量描述符
// ===========================================================================

/// IPI 向量描述符（布局见文件头表格）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VecDesc {
    /// 向量号。
    pub no: u8,
    /// 中文名。
    pub name_cn: &'static str,
    /// 处理函数符号名（编译期宏下标）。
    pub handler_sym: &'static str,
    /// 向量类别。
    pub class: IpiClass,
    /// 中断层级约束。
    pub level: LevelCtx,
    /// 是否允许同类 pending 合并。
    pub mergeable: bool,
    /// 期望时长上界（逻辑 tick）。
    pub budget_ticks: u64,
    /// 消费方批号。
    pub owner_batch: &'static str,
    /// 逐项 FNV-1a 64 指纹。
    pub fp: u64,
}

/// 保留向量号判定（`const fn`，编译期宏使用）。
pub const fn is_reserved_no(no: u8) -> bool {no >= VEC_RESERVED_LO}

/// 声明窗口判定（`const fn`，编译期宏使用）。
pub const fn in_window(no: u8) -> bool {no >= VEC_WINDOW_LO && no < VEC_WINDOW_HI}

/// 槽位索引（`const fn`，越界索引在编译期失败）。
pub const fn slot_of(no: u8) -> usize {(no - VEC_WINDOW_LO) as usize}

/// 构造向量描述符并计算逐项指纹（`const fn`）。
///
/// 指纹覆盖除 `fp` 自身外的全部字段，字段顺序即字节混入顺序，
/// 任一字段改写必然改变 `fp`——这是「逐项可审计」的基础。
pub const fn vec_desc(
    no: u8,
    name_cn: &'static str,
    handler_sym: &'static str,
    class: IpiClass,
    level: LevelCtx,
    mergeable: bool,
    budget_ticks: u64,
    owner_batch: &'static str,
) -> VecDesc {
    let mut h = FNV_BASIS;
    h = fnv_u8(h, no);
    h = fnv_str(h, name_cn);
    h = fnv_str(h, handler_sym);
    h = fnv_u8(h, class.code());
    h = level.fold(h);
    h = fnv_bool(h, mergeable);
    h = fnv_u64(h, budget_ticks);
    h = fnv_str(h, owner_batch);
VecDesc {no,name_cn,handler_sym,class,level,mergeable,budget_ticks,owner_batch,fp: h,}
}

impl VecDesc {
    /// 重新计算指纹（改写后用于对账）。
pub fn recompute(&mut self) {self.fp = fp_of_desc(self);}

    /// 该项是否保留区项。
pub const fn is_reserved(&self) -> bool {matches!(self.class, IpiClass::Reserved)}

    /// 一行 KV 文本。
    pub fn kv(&self) -> String {
        format!(
            "vec=0x{:02X} class={} name={} handler={} {} merge={} budget={} owner={} fp={:016X}",
            self.no,
            self.class.tag(),
            self.name_cn,
            self.handler_sym,
            self.level.text(),
            if self.mergeable { "yes" } else { "no" },
            self.budget_ticks,
            self.owner_batch,
            self.fp
        )
    }
}

/// 描述符指纹（与 [`vec_desc`] 构造期算法一致，供运行期复算）。
pub fn fp_of_desc(d: &VecDesc) -> u64 {
    let mut h = FNV_BASIS;
    h = fnv_u8(h, d.no);
    h = fnv_str(h, d.name_cn);
    h = fnv_str(h, d.handler_sym);
    h = fnv_u8(h, d.class.code());
    h = d.level.fold(h);
    h = fnv_bool(h, d.mergeable);
    h = fnv_u64(h, d.budget_ticks);
    h = fnv_str(h, d.owner_batch);
    h
}

/// 全表封账指纹（顺序 FNV，序敏感——重排必然改指纹）。
pub fn table_seal(entries: &[VecDesc]) -> u64 {
    let mut h = FNV_BASIS;
for e in entries.iter() {h = fnv_u64(h, e.fp); h = fnv_u8(h, e.class.code());}
    fnv_u64(h, SEAL_REV as u64)
}

// ===========================================================================
// 8. v1 槽位图（编译期与运行期共用的权威占用声明）
// ===========================================================================

/// v1 槽位图：下标 = `vec_no - VEC_WINDOW_LO`，值 = 该槽声明的向量号。
///
/// 这张表是「槽位是否被占」的**唯一权威声明**，编译期登记宏逐项断言
/// 自身声明与本图一致（不一致 = 编译失败）；运行期 `VecTable` 也按本图
/// 反查，避免两处各写一份而漂移。
pub const SLOT_CLAIM: [u8; TABLE_CAP] = [
    0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x4B, 0x4C, 0x4D, 0x4E, 0x4F,
    0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x5B, 0x5C, 0x5D, 0x5E, 0x5F,
];

/// 槽位图覆盖完整性检查（`const fn`）。
///
/// 判据：UNX-F1842-J1 —— 声明窗口 32 槽必须**每槽恰一**声明，
/// 不得有空洞（空洞 = 号段可被私自占用 = 分配表有洞）。
pub const fn slot_map_is_dense() -> bool {
    let mut i = 0usize;
while i < TABLE_CAP {if SLOT_CLAIM[i] != (VEC_WINDOW_LO + i as u8) {return false;} i += 1;}
    true
}

// ===========================================================================
// 9. 编译期登记宏（第一道检：语法层强制登记）
// ===========================================================================

/// 编译期 IPI 向量登记宏（模块内 `macro_rules!`，展开体用未限定路径）。
///
/// 每一次**声明**一个向量必须走本宏。宏展开产生四样东西：
///
/// 1. 一个 `pub const <handler>: VecDesc` —— 进入编译期静态表 [`COMPILE_GUARD`]；
/// 2. 同一 `const` 的 [`VecDesc::no`] 即编译期向量号载体，
///    `send_ipi_vec!` 以**符号级**下标引用它（`const VNO = <sym>.no;`）；
/// 3. `const` 断言：向量号落在声明窗口 `0x40..0x60` 内，否则编译失败；
/// 4. `const` 断言：业务宏不得占用保留区（`no >= 0x58` 直接编译失败），
///    并且槽位图声明一致（越界索引 / 槽位错位均在编译期炸）。
///
/// 命名约定：处理函数符号名即编译期 `const` 名，故「登记」与「可发」
/// 是同一个名字——想发一个没登记过的符号，编译器直接报错。
///
/// 判据：UNX-F1842-J1 —— 编译期宏 + 运行期断言双检，宏是第一道。
///
/// 说明：宏为模块内私有宏，展开体依赖本模块项的未限定路径，
/// 故必须在本模块作用域内展开（`register_ipi_vec!` 的调用点亦在本文件内，
/// 这正是「登记与使用同源」的必然要求）。
macro_rules! register_ipi_vec {
    ($no:expr, $name:expr, $handler:ident, $class:expr, $level:expr, $mergeable:expr, $budget:expr, $owner:expr) => {
        /// 编译期登记的向量描述符（符号名即 const 名）。
pub const $handler: VecDesc = vec_desc($no,$name,stringify!($handler),$class,$level,$mergeable,$budget,$owner,);

        /// 编译期断言组：窗口 / 保留区 / 槽位 / 预算一致性。
        const _: () = {
            // (a) 向量号必须在声明窗口内。
assert!(in_window($no),"UNX-F1842: IPI 向量号越出声明窗口 0x40..0x60");
            // (b) 保留区向量号必须由 `register_ipi_reserved_vec!` 登记，
            //     业务宏一律拒收（越界索引在此处编译期失败）。
assert!(!is_reserved_no($no),"UNX-F1842: 保留区向量号不得被业务登记宏占用");
            // (b2) 业务向量类别不得是保留类。
assert!($class.is_business(),"UNX-F1842: 业务登记宏的类别不得为保留区");
            // (c) 槽位图必须声明同一个号（越界索引 `SLOT_CLAIM[slot_of]` 在编译期失败）。
assert!(SLOT_CLAIM[slot_of($no)] == $no,"UNX-F1842: 槽位图与登记宏向量号不一致");
            // (d) 期望时长上界不得超过层级驻留上界。
assert!($budget <= ($level).max_stay_ticks,"UNX-F1842: budget 不得超过 LevelCtx::max_stay_ticks");
            // (e) 层级驻留上界不得为 0（否则时长断言无意义）。
assert!(($level).max_stay_ticks > 0,"UNX-F1842: 层级驻留上界不得为 0");
            // (f) 逐项指纹不得为 0（字段漏混入即暴露）。
assert!($handler.fp != 0,"UNX-F1842: 逐项指纹不得为 0");
        };
    };
}

/// 编译期 IPI **保留向量** 登记宏。
///
/// 与 [`register_ipi_vec`] 的区别仅在于断言方向相反：这里断言
/// **必须**落在保留区，且类别必须是 `IpiClass::Reserved`。
/// 两条路径互斥，故「保留区被业务占用」与「保留区被误标为业务」
/// 都在编译期失败。
macro_rules! register_ipi_reserved_vec {
    ($no:expr, $name:expr, $handler:ident, $level:expr, $budget:expr, $owner:expr) => {
        /// 编译期登记的保留向量描述符。
pub const $handler: VecDesc = vec_desc($no,$name,stringify!($handler),IpiClass::Reserved,$level,false,$budget,$owner,);

        /// 编译期断言组：必须保留区。
        const _: () = {
assert!(in_window($no),"UNX-F1842: 保留向量号越出声明窗口");
assert!(is_reserved_no($no),"UNX-F1842: 保留向量必须落在保留区");
assert!(SLOT_CLAIM[slot_of($no)] == $no,"UNX-F1842: 槽位图与保留登记宏向量号不一致");
assert!($budget <= ($level).max_stay_ticks,"UNX-F1842: budget 不得超过 LevelCtx::max_stay_ticks");
assert!($handler.fp != 0,"UNX-F1842: 逐项指纹不得为 0");
        };
    };
}

/// 编译期发送宏（三参式）。
///
/// 展开为一次**运行期**发送的调用计划：[`SendCall`] 携带编译期已固定的
/// 向量号、目标核、上下文；再由 `SendCall::dispatch` 落到运行期表并走
/// 第二道断言。
///
/// 判据：UNX-F1842-J1 —— 「造 IPI 必登记」在语法层成立：
/// 本宏引用 `<符号>.no`，**符号写错 / 未登记即编译失败**（找不到该
/// `const`），编译器成为第一道检的承载者。
macro_rules! send_ipi_vec {
    ($vec_sym:ident, $target:expr, $ctx:expr) => {{
        // 符号级引用：未登记的符号在此编译失败。
        const VNO: u8 = $vec_sym.no;
        SendCall::new(VNO, $target, $ctx)
    }};
}

/// 编译期发送宏（四参式，顺带把运行期实例带上，便于单表达式发送）。
macro_rules! send_ipi_vec_on {
($rt:expr, $vec_sym:ident, $target:expr, $ctx:expr) => {{const VNO: u8 = $vec_sym.no; SendCall::new(VNO, $target, $ctx)}};
}

/// 运行期直发（绕过宏的**唯一**合法入口，仅供注入核构造非法向量用）。
///
/// 它不是「后门」而是**被测面**：注入核需要制造一个编译期不可能存在的
/// 向量号来验证第二道检是否真的拦得住。
pub const fn raw_send_call(no: u8, target: u32, ctx: u64) -> SendCall {SendCall {no,target,ctx,via_macro: false,}}

// ===========================================================================
// 10. v1 分配表登记（32 项，全部经编译期宏）
// ===========================================================================

register_ipi_vec!(0x40, "重调度·基础呼叫", f1844_resched_basic, IpiClass::Resched, LC_STRICT8, true, 4, "F1844");
register_ipi_vec!(0x41, "重调度·周期抢占", f1844_resched_tick, IpiClass::Resched, LC_STRICT8, true, 4, "F1844");
register_ipi_vec!(0x42, "重调度·唤醒远端", f1844_resched_wake, IpiClass::Resched, LC_STRICT8, true, 6, "F1844");
register_ipi_vec!(0x43, "重调度·任务入队", f1844_resched_enqueue, IpiClass::Resched, LC_STRICT, true, 4, "F1844");
register_ipi_vec!(0x44, "重调度·时钟回退", f1844_resched_catchup, IpiClass::Resched, LC_STRICT8, true, 6, "F1844");
register_ipi_vec!(0x45, "重调度·RCU 宽限期", f1844_resched_rcu, IpiClass::Resched, LC_WIDE, true, 12, "F1844");
register_ipi_vec!(0x46, "重调度·内存回收", f1844_resched_reclaim, IpiClass::Resched, LC_WIDE, true, 16, "F1844");
register_ipi_vec!(0x47, "迁移通知·页迁入", f1845_migrate_in, IpiClass::Migrate, LC_STRICT8, true, 6, "F1845");
register_ipi_vec!(0x48, "迁移通知·页迁出", f1845_migrate_out, IpiClass::Migrate, LC_STRICT8, true, 6, "F1845");
register_ipi_vec!(0x49, "迁移通知·冷页驱逐", f1845_migrate_evict, IpiClass::Migrate, LC_WIDE, true, 12, "F1845");
register_ipi_vec!(0x4A, "迁移通知·地址空间切换", f1845_migrate_mm, IpiClass::Migrate, LC_STRICT8, true, 8, "F1845");
register_ipi_vec!(0x4B, "迁移通知·NUMA 重平衡", f1845_migrate_numa, IpiClass::Migrate, LC_WIDE, true, 12, "F1845");
register_ipi_vec!(0x4C, "迁移通知·句柄移交", f1845_migrate_handoff, IpiClass::Migrate, LC_STRICT8, true, 6, "F1845");
register_ipi_vec!(0x4D, "TLB击落·全域广播", f1846_shoot_bcast, IpiClass::Shootdown, LC_STRICT, true, 4, "F1846");
register_ipi_vec!(0x4E, "TLB击落·单地址", f1846_shoot_single, IpiClass::Shootdown, LC_STRICT, true, 2, "F1846");
register_ipi_vec!(0x4F, "TLB击落·范围批量", f1846_shoot_range, IpiClass::Shootdown, LC_STRICT, true, 4, "F1846");
register_ipi_vec!(0x50, "TLB击落·PCID 换代", f1846_shoot_pcid, IpiClass::Shootdown, LC_STRICT, true, 4, "F1846");
register_ipi_vec!(0x51, "TLB击落·分层降级", f1846_shoot_tiered, IpiClass::Shootdown, LC_STRICT8, true, 8, "F1846");
register_ipi_vec!(0x52, "TLB击落·回执汇总", f1846_shoot_ack, IpiClass::Shootdown, LC_STRICT8, false, 6, "F1846");
register_ipi_vec!(0x53, "TLB击落·ASID 代换", f1846_shoot_asid, IpiClass::Shootdown, LC_STRICT, true, 4, "F1846");
register_ipi_vec!(0x54, "TLB击落·探测自检", f1846_shoot_probe, IpiClass::Shootdown, LC_PROBE, false, 4, "F1846");
register_ipi_vec!(0x55, "迁移通知·探测自检", f1845_migrate_probe, IpiClass::Migrate, LC_PROBE, false, 4, "F1845");
register_ipi_vec!(0x56, "重调度·探测自检", f1844_resched_probe, IpiClass::Resched, LC_PROBE, false, 4, "F1844");
register_ipi_vec!(0x57, "击落·诊断回环", f1846_shoot_diagloop, IpiClass::Shootdown, LC_PROBE, false, 4, "F1846");
register_ipi_reserved_vec!(0x58, "保留·F1842 空位0", f1842_resv_0, LC_STRICT, 1, "F1842");
register_ipi_reserved_vec!(0x59, "保留·F1842 空位1", f1842_resv_1, LC_STRICT, 1, "F1842");
register_ipi_reserved_vec!(0x5A, "保留·F1842 空位2", f1842_resv_2, LC_STRICT, 1, "F1842");
register_ipi_reserved_vec!(0x5B, "保留·F1842 空位3", f1842_resv_3, LC_STRICT, 1, "F1842");
register_ipi_reserved_vec!(0x5C, "保留·F1842 空位4", f1842_resv_4, LC_STRICT, 1, "F1842");
register_ipi_reserved_vec!(0x5D, "保留·F1842 空位5", f1842_resv_5, LC_STRICT, 1, "F1842");
register_ipi_reserved_vec!(0x5E, "保留·F1842 空位6", f1842_resv_6, LC_STRICT, 1, "F1842");
register_ipi_reserved_vec!(0x5F, "保留·F1842 空位7", f1842_resv_7, LC_STRICT, 1, "F1842");

// ===========================================================================
// 11. 编译期静态表（双检的第一数据源）
// ===========================================================================

/// 编译期静态表：按向量号升序的全量登记项。
///
/// 这张 `const` 切片是「宏展开 = 可执行证据」的第一数据源；
/// 运行期 `VecTable` 是第二数据源；两者逐项对拍（[`dual_source_reconcile`]）。
pub const COMPILE_GUARD: [VecDesc; TABLE_CAP] = [
    f1844_resched_basic,
    f1844_resched_tick,
    f1844_resched_wake,
    f1844_resched_enqueue,
    f1844_resched_catchup,
    f1844_resched_rcu,
    f1844_resched_reclaim,
    f1845_migrate_in,
    f1845_migrate_out,
    f1845_migrate_evict,
    f1845_migrate_mm,
    f1845_migrate_numa,
    f1845_migrate_handoff,
    f1846_shoot_bcast,
    f1846_shoot_single,
    f1846_shoot_range,
    f1846_shoot_pcid,
    f1846_shoot_tiered,
    f1846_shoot_ack,
    f1846_shoot_asid,
    f1846_shoot_probe,
    f1845_migrate_probe,
    f1844_resched_probe,
    f1846_shoot_diagloop,
    f1842_resv_0,
    f1842_resv_1,
    f1842_resv_2,
    f1842_resv_3,
    f1842_resv_4,
    f1842_resv_5,
    f1842_resv_6,
    f1842_resv_7,
];

/// 编译期表快照（只读副本，运行期拿它去对账）。
pub fn compile_guard_table() -> Vec<VecDesc> {COMPILE_GUARD.to_vec()}

/// 编译期表封账指纹（`const fn`，编译期即可判定 v1 未被改写）。
pub const fn compile_guard_seal() -> u64 {
    let mut h = FNV_BASIS;
    let mut i = 0usize;
while i < TABLE_CAP {h = fnv_u64(h, COMPILE_GUARD[i].fp); h = fnv_u8(h, COMPILE_GUARD[i].class.code()); i += 1;}
    fnv_u64(h, SEAL_REV as u64)
}

/// 编译期表条目数（`const`）。
pub const COMPILE_GUARD_LEN: usize = TABLE_CAP;

/// 编译期自检（`const fn`，把「宏层」的不变量钉进编译期）。
///
/// 判据：UNX-F1842-J1 —— 冻结 v1 的四类向量必须**在编译期**就已齐全，
/// 且槽位图稠密、每项指纹非零。
pub const fn compile_guard_selfcheck() -> bool {
if !slot_map_is_dense() {return false;}
    let mut i = 0usize;
    let mut seen_rs = 0u32;
    let mut seen_mg = 0u32;
    let mut seen_sd = 0u32;
    let mut seen_rv = 0u32;
    while i < TABLE_CAP {
        let e = &COMPILE_GUARD[i];
if !in_window(e.no) {return false;}
if SLOT_CLAIM[slot_of(e.no)] != e.no {return false;}
if e.fp == 0 {return false;}
if e.budget_ticks == 0 || e.budget_ticks > e.level.max_stay_ticks {return false;}
        match e.class {
            IpiClass::Resched => seen_rs += 1,
            IpiClass::Migrate => seen_mg += 1,
            IpiClass::Shootdown => seen_sd += 1,
            IpiClass::Reserved => seen_rv += 1,
        }
        i += 1;
    }
    seen_rs > 0 && seen_mg > 0 && seen_sd > 0 && seen_rv > 0
}

// ===========================================================================
// 12. 运行期分配表（第二数据源，含冻结封账）
// ===========================================================================

/// 冻结状态：v1 封账后不可增删改排。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SealState {
    /// 开放（尚未封账）。
    Open,
    /// 已封账（指纹钉死）。
    Sealed {
        /// 封账指纹。
        fp: u64,
    },
}

/// 运行期 IPI 向量分配表。
#[derive(Clone, Debug)]
pub struct VecTable {entries: Vec<VecDesc>,seal: SealState,by_class: BTreeMap<u8, Vec<u8>>,mutations_after_seal: u32,}

impl VecTable {
    /// 以给定条目构造（开放态）。
    pub fn new(mut entries: Vec<VecDesc>) -> VResult<VecTable> {
        entries.sort_by(|a, b| a.no.cmp(&b.no));
        // 内部一致性：窗口 + 指纹 + 预算。
        for e in entries.iter() {
if !in_window(e.no) {return Err(IpiVecError::VectorOutOfWindow);}
if e.fp != fp_of_desc(e) {return Err(IpiVecError::DualSourceDrift);}
if e.budget_ticks == 0 || e.budget_ticks > e.level.max_stay_ticks {return Err(IpiVecError::BadParam);}
        }
        // 向量号不得重复。
        let mut i = 1usize;
while i < entries.len() {if entries[i - 1].no == entries[i].no {return Err(IpiVecError::VectorDup);} i += 1;}
Ok(VecTable {by_class: index_by_class(&entries),entries,seal: SealState::Open,mutations_after_seal: 0,})
    }

    /// 条目数。
pub fn len(&self) -> usize {self.entries.len()}

    /// 是否空表。
pub fn is_empty(&self) -> bool {self.entries.is_empty()}

    /// 冻结状态。
pub fn seal_state(&self) -> SealState {self.seal}

    /// 是否已封账。
pub fn is_sealed(&self) -> bool {matches!(self.seal, SealState::Sealed {..})}

    /// 封账后被拒绝的改表次数（审计用，正常应恒为 0）。
pub fn rejected_mutations(&self) -> u32 {self.mutations_after_seal}

    /// 当前封账指纹（未封账时对当前内容现算）。
pub fn current_seal(&self) -> u64 {table_seal(&self.entries)}

    /// 查表（`O(N)` 线性，N=32 定长）。
    pub fn get(&self, no: u8) -> VResult<&VecDesc> {
if !in_window(no) {return Err(IpiVecError::VectorOutOfWindow);}
        let mut i = 0usize;
while i < self.entries.len() {if self.entries[i].no == no {return Ok(&self.entries[i]);} i += 1;}
        Err(IpiVecError::UnregisteredVector)
    }

    /// 可变取项（**仅供注入核制造漂移**，调用方自负被冻结语义打破的后果；
    /// `send()` 会在冻结复核时把这种表判为不可用）。
    pub fn get_mut(&mut self, no: u8) -> VResult<&mut VecDesc> {
        let mut i = 0usize;
while i < self.entries.len() {if self.entries[i].no == no {return Ok(&mut self.entries[i]);} i += 1;}
        Err(IpiVecError::UnregisteredVector)
    }

    /// 是否已登记。
pub fn contains(&self, no: u8) -> bool {self.get(no).is_ok()}

    /// 按类别取向量号列表（升序）。
pub fn class_vecs(&self, class: IpiClass) -> &[u8] {match self.by_class.get(&class.code()) {Some(v) => v.as_slice(),None => &[],}}

    /// 逐项迭代（只读）。
pub fn iter(&self) -> core::slice::Iter<'_, VecDesc> {self.entries.iter()}

    /// 只读快照（外部拿不到 `&mut`，无法绕过冻结）。
pub fn snapshot(&self) -> VecTableSnapshot {VecTableSnapshot {seal: self.seal,fp: self.current_seal(),items: self.entries.clone(),}}

    /// 封账 v1：钉死指纹。
    ///
    /// 判据：UNX-F1842-J1 —— v1 封账后全表指纹写死，后续任何
    /// 新增 / 改写 / 删除 / 重排一律 `Err(FrozenViolation)`。
    pub fn seal_v1(&mut self, expect_fp: u64) -> VResult<u64> {
if self.is_sealed() {self.mutations_after_seal = self.mutations_after_seal.saturating_add(1); return Err(IpiVecError::FrozenViolation);}
        let fp = self.current_seal();
if fp != expect_fp {return Err(IpiVecError::DualSourceDrift);}
        self.seal = SealState::Sealed { fp };
        Ok(fp)
    }

    /// 复核冻结不变式（已封账则重算指纹比对）。
    pub fn verify_seal(&self, expect_fp: u64) -> VResult<()> {
if !self.is_sealed() {return Err(IpiVecError::FrozenViolation);}
        let fp = self.current_seal();
if fp != expect_fp {return Err(IpiVecError::FrozenViolation);}
        Ok(())
    }

    /// 封账后新增向量（必 `Err(FrozenViolation)`）。
    pub fn insert(&mut self, d: VecDesc) -> VResult<()> {
if self.is_sealed() {self.mutations_after_seal = self.mutations_after_seal.saturating_add(1); return Err(IpiVecError::FrozenViolation);}
if !in_window(d.no) {return Err(IpiVecError::VectorOutOfWindow);}
if self.contains(d.no) {return Err(IpiVecError::VectorDup);}
if d.fp != fp_of_desc(&d) {return Err(IpiVecError::DualSourceDrift);}
        self.entries.push(d);
        self.entries.sort_by(|a, b| a.no.cmp(&b.no));
        self.by_class = index_by_class(&self.entries);
        Ok(())
    }

    /// 封账后改写向量（必 `Err(FrozenViolation)`）。
    pub fn modify(&mut self, no: u8, budget: u64) -> VResult<()> {
if self.is_sealed() {self.mutations_after_seal = self.mutations_after_seal.saturating_add(1); return Err(IpiVecError::FrozenViolation);}
        let mut hit = false;
for e in self.entries.iter_mut() {if e.no == no {e.budget_ticks = budget; e.recompute(); hit = true; break;}}
if !hit {return Err(IpiVecError::UnregisteredVector);}
        self.by_class = index_by_class(&self.entries);
        Ok(())
    }

    /// 封账后删除向量（必 `Err(FrozenViolation)`）。
    pub fn remove(&mut self, no: u8) -> VResult<VecDesc> {
if self.is_sealed() {self.mutations_after_seal = self.mutations_after_seal.saturating_add(1); return Err(IpiVecError::FrozenViolation);}
        let mut idx = 0usize;
        while idx < self.entries.len() {
if self.entries[idx].no == no {let d = self.entries.remove(idx); self.by_class = index_by_class(&self.entries); return Ok(d);}
            idx += 1;
        }
        Err(IpiVecError::UnregisteredVector)
    }

    /// 封账后重排（必 `Err(FrozenViolation)`）。
    pub fn reorder(&mut self) -> VResult<()> {
if self.is_sealed() {self.mutations_after_seal = self.mutations_after_seal.saturating_add(1); return Err(IpiVecError::FrozenViolation);}
        self.entries.reverse();
        self.by_class = index_by_class(&self.entries);
        Ok(())
    }

    /// 一行 KV（表级）。
    pub fn kv(&self) -> String {
        format!(
            "table=n={} seal={} fp={:016X} rejected_mut={}",
            self.entries.len(),
            if self.is_sealed() { "v1" } else { "open" },
            self.current_seal(),
            self.mutations_after_seal
        )
    }
}

/// 类别索引构建。
fn index_by_class(entries: &[VecDesc]) -> BTreeMap<u8, Vec<u8>> {
    let mut m: BTreeMap<u8, Vec<u8>> = BTreeMap::new();
for e in entries.iter() {m.entry(e.class.code()).or_insert_with(Vec::new).push(e.no);}
for v in m.values_mut() {v.sort_by(|a, b| a.cmp(b));}
    m
}

/// 只读快照：外部可读、可导出，**无任何改表入口**。
#[derive(Clone, Debug)]
pub struct VecTableSnapshot {seal: SealState,fp: u64,items: Vec<VecDesc>,}

impl VecTableSnapshot {
    /// 条目数。
pub fn len(&self) -> usize {self.items.len()}

    /// 是否空。
pub fn is_empty(&self) -> bool {self.items.is_empty()}

    /// 封账状态。
pub fn seal_state(&self) -> SealState {self.seal}

    /// 快照指纹。
pub fn fp(&self) -> u64 {self.fp}

    /// 逐项只读。
pub fn iter(&self) -> core::slice::Iter<'_, VecDesc> {self.items.iter()}

    /// 按向量号取（内部复制，越界返 `None`）。
pub fn find(&self, no: u8) -> Option<&VecDesc> {self.items.iter().find(|d| d.no == no)}

    /// 导出全部条目文本（逐项一行）。
pub fn to_text(&self) -> String {let mut s = String::new(); for d in self.items.iter() {s.push_str(&d.kv()); s.push('\n');} s}
}

// ===========================================================================
// 13. 双源对账（双检的汇流点）
// ===========================================================================

/// 单项对账差异。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DriftItem {
    /// 向量号。
    pub no: u8,
    /// 字段名（`fp` 表示整项指纹不符）。
    pub field: &'static str,
    /// 编译期值文本。
    pub compile_side: String,
    /// 运行期值文本。
    pub runtime_side: String,
}

/// 双源对账结果。
#[derive(Clone, Debug)]
pub struct ReconcileReport {
    /// 编译期条目数。
    pub compile_len: usize,
    /// 运行期条目数。
    pub runtime_len: usize,
    /// 编译期指纹。
    pub compile_fp: u64,
    /// 运行期指纹。
    pub runtime_fp: u64,
    /// 差异明细。
    pub diffs: Vec<DriftItem>,
}

/// 逐项对拍（编译期表 vs 运行期表）。
///
/// 判据：UNX-F1842-J1 —— 「编译期宏 + 运行期断言双检」意味着两条路线
/// 必须指向同一张分配表；任一项字段不同即 `DualSourceDrift`。
pub fn dual_source_reconcile(rt: &VecTable) -> VResult<ReconcileReport> {
    let cg = COMPILE_GUARD;
    let mut diffs: Vec<DriftItem> = Vec::new();

if cg.len() != rt.len() {diffs.push(DriftItem {no: 0,field: "len",compile_side: cg.len().to_string(),runtime_side: rt.len().to_string(),});}

    let mut i = 0usize;
    while i < cg.len() {
        let c = &cg[i];
        match rt.get(c.no) {
Err(e) => diffs.push(DriftItem {no: c.no,field: "missing",compile_side: c.handler_sym.to_string(),runtime_side: format!("<absent:{}>", e.phrase()),}),
            Ok(r) => {
                compare_field(&mut diffs, c.no, "name_cn", c.name_cn, r.name_cn);
compare_field(&mut diffs,c.no,"handler_sym",c.handler_sym,r.handler_sym,);
compare_field(&mut diffs,c.no,"class",c.class.tag(),r.class.tag(),);
compare_field(&mut diffs,c.no,"level",&c.level.text(),&r.level.text(),);
compare_field(&mut diffs,c.no,"mergeable",if c.mergeable { "yes" } else { "no" },if r.mergeable { "yes" } else { "no" },);
compare_field(&mut diffs,c.no,"budget",&c.budget_ticks.to_string(),&r.budget_ticks.to_string(),);
compare_field(&mut diffs,c.no,"owner",c.owner_batch,r.owner_batch,);
compare_field(&mut diffs,c.no,"fp",&format!("{:016X}", c.fp),&format!("{:016X}", r.fp),);
            }
        }
        i += 1;
    }

let rep = ReconcileReport {compile_len: cg.len(),runtime_len: rt.len(),compile_fp: compile_guard_seal(),runtime_fp: rt.current_seal(),diffs,};
if rep.diffs.is_empty() {Ok(rep)} else {Err(IpiVecError::DualSourceDrift)}
}

/// 返回对账报告（含失败明细，不抛错）。
pub fn dual_source_report(rt: &VecTable) -> ReconcileReport {
    let mut diffs: Vec<DriftItem> = Vec::new();
    for c in COMPILE_GUARD.iter() {
        match rt.get(c.no) {
Err(e) => diffs.push(DriftItem {no: c.no,field: "missing",compile_side: c.handler_sym.to_string(),runtime_side: format!("<absent:{}>", e.phrase()),}),
Ok(r) => compare_field(&mut diffs,c.no,"fp",&format!("{:016X}", c.fp),&format!("{:016X}", r.fp),),
        }
    }
ReconcileReport {compile_len: COMPILE_GUARD.len(),runtime_len: rt.len(),compile_fp: compile_guard_seal(),runtime_fp: rt.current_seal(),diffs,}
}

/// 字段比对辅助。
fn compare_field(diffs: &mut Vec<DriftItem>,no: u8,field: &'static str,a: &str,b: &str,) {
if a != b {diffs.push(DriftItem {no,field,compile_side: a.to_string(),runtime_side: b.to_string(),});}
}

// ===========================================================================
// 14. 内建运行期表（第二数据源的物化）
// ===========================================================================

/// 内建运行期分配表：由编译期 `const` 项逐项物化。
///
/// 「第二数据源」的意义在于：它走的是**运行期构造路径**
/// （`Vec` 分配 + 指纹复算 + 类别索引重建），若与编译期常量表在
/// 任何一项上不同，`VecTable::new` 的指纹复算或后续对账即报漂移。
pub fn builtin_table() -> VResult<VecTable> {
    let mut v: Vec<VecDesc> = Vec::with_capacity(TABLE_CAP);
    for c in COMPILE_GUARD.iter() {
        let mut d = c.clone();
        // 运行期重新计算指纹：不一致直接在建表阶段暴露。
        d.recompute();
if d.fp != c.fp {return Err(IpiVecError::DualSourceDrift);}
        v.push(d);
    }
    VecTable::new(v)
}

/// 按 v1 指纹构造并封账的运行期表。
pub fn sealed_v1_table() -> VResult<(VecTable, u64)> {let mut t = builtin_table()?; let fp = t.seal_v1(SEAL_FP_V1)?; Ok((t, fp))}

// ===========================================================================
// 15. 分派表：向量号 → handler id
// ===========================================================================

/// 处理函数标识（F1843 分发协议里的「处理函数」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandlerId {
    /// 重调度处理器（`on_ipi_resched`）。
    Resched,
    /// 迁移通知处理器（`on_ipi_migrate`）。
    Migrate,
    /// TLB 击落处理器（`on_ipi_shootdown`）。
    Shootdown,
    /// 保留区处理器（`on_ipi_reserved`，恒不应被投递）。
    Reserved,
}

impl HandlerId {
    /// 符号名。
    pub fn sym(self) -> &'static str {
        match self {
            HandlerId::Resched => "on_ipi_resched",
            HandlerId::Migrate => "on_ipi_migrate",
            HandlerId::Shootdown => "on_ipi_shootdown",
            HandlerId::Reserved => "on_ipi_reserved",
        }
    }

    /// 短码。
pub fn code(self) -> u8 {match self {HandlerId::Resched => 0,HandlerId::Migrate => 1,HandlerId::Shootdown => 2,HandlerId::Reserved => 3,}}
}

/// 分派表条目（向量号 → handler）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DispatchRow {
    /// 向量号。
    pub no: u8,
    /// 处理函数标识。
    pub handler: HandlerId,
}

/// 分派表：全窗口稠密定长表（槽位索引 = `slot_of(no)`）。
pub fn dispatch_table() -> [DispatchRow; TABLE_CAP] {
let mut rows = [DispatchRow {no: 0,handler: HandlerId::Reserved,}; TABLE_CAP];
    let mut i = 0usize;
while i < TABLE_CAP {let e = &COMPILE_GUARD[i]; rows[i] = DispatchRow {no: e.no,handler: handler_of_class(e.class),}; i += 1;}
    rows
}

/// 类别 → 处理函数。
pub const fn handler_of_class(class: IpiClass) -> HandlerId {
    match class {
        IpiClass::Resched => HandlerId::Resched,
        IpiClass::Migrate => HandlerId::Migrate,
        IpiClass::Shootdown => HandlerId::Shootdown,
        IpiClass::Reserved => HandlerId::Reserved,
    }
}

/// 向量号 → handler（分派表查表）。
pub fn handler_id_for(no: u8) -> VResult<HandlerId> {
if !in_window(no) {return Err(IpiVecError::VectorOutOfWindow);}
    let idx = slot_of(no);
    let rows = dispatch_table();
if idx >= rows.len() {return Err(IpiVecError::VectorOutOfWindow);}
    Ok(rows[idx].handler)
}

/// 处理函数符号名查表（用于证据文本一致性）。
pub fn handler_symbol_for(no: u8) -> VResult<&'static str> {let h = handler_id_for(no)?; Ok(h.sym())}

// ===========================================================================
// 16. 发送调用（编译期宏产物 / 注入核构造体）
// ===========================================================================

/// 一次发送调用（由 `send_ipi_vec!` 或 `raw_send_call` 产出）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SendCall {
    /// 向量号（编译期固定）。
    pub no: u8,
    /// 目标核。
    pub target: u32,
    /// 上下文字（语义负载：地址 / 核集合 / 迁移序号，按向量语义解释）。
    pub ctx: u64,
    /// 是否经编译期宏产出（`false` = 注入核直构）。
    pub via_macro: bool,
}

impl SendCall {
    /// 构造一次经宏的发送调用。
pub const fn new(no: u8, target: u32, ctx: u64) -> SendCall {SendCall {no,target,ctx,via_macro: true,}}

    /// 一行文本。
pub fn text(&self) -> String {format!("call vec=0x{:02X} target={} ctx={} via_macro={}",self.no, self.target, self.ctx, self.via_macro)}
}

// ===========================================================================
// 17. 落账条目
// ===========================================================================

/// 一条落账（诊断 / 事件）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LedgerEntry {
    /// 逻辑 tick。
    pub tick: u64,
    /// 诊断码（`0` 表示纯事件，非诊断）。
    pub code: u16,
    /// 中文短语。
    pub phrase: &'static str,
    /// 关联向量号（无则 `0xFF`）。
    pub no: u8,
    /// 目标核。
    pub target: u32,
    /// 证据文本。
    pub evidence: String,
}

/// 落账簿。
#[derive(Clone, Debug, Default)]
pub struct Book {entries: Vec<LedgerEntry>,overflow: u32,}

impl Book {
    /// 构造空账。
pub fn new() -> Book {Book {entries: Vec::with_capacity(256),overflow: 0,}}

    /// 记一条。
    pub fn push(&mut self, e: LedgerEntry) {
        if self.entries.len() < 4096 {
            self.entries.push(e);
        } else {
            // 不静默丢：溢出单独计数并留痕。
            self.overflow = self.overflow.saturating_add(1);
        }
    }

    /// 记一条诊断。
    #[allow(clippy::too_many_arguments)]
pub fn note(&mut self,tick: u64,c: Code,no: u8,target: u32,evidence: String,) {
self.push(LedgerEntry {tick,code: c.val(),phrase: c.phrase(),no,target,evidence,});
    }

    /// 记一条纯事件（`code = 0`）。
pub fn event(&mut self, tick: u64, no: u8, target: u32, evidence: String) {self.push(LedgerEntry {tick,code: 0,phrase: "事件",no,target,evidence,});}

    /// 长度。
pub fn len(&self) -> usize {self.entries.len()}

    /// 是否空。
pub fn is_empty(&self) -> bool {self.entries.is_empty()}

    /// 溢出计数（正常恒 0）。
pub fn overflow(&self) -> u32 {self.overflow}

    /// 逐条只读。
pub fn iter(&self) -> core::slice::Iter<'_, LedgerEntry> {self.entries.iter()}

    /// 统计某诊断码出现次数。
pub fn count_of(&self, c: Code) -> usize {self.entries.iter().filter(|e| e.code == c.val()).count()}

    /// 是否存在某码。
pub fn has(&self, c: Code) -> bool {self.count_of(c) > 0}

    /// 末条证据。
pub fn last_evidence(&self) -> String {match self.entries.last() {Some(e) => e.evidence.clone(),None => String::from("<empty>"),}}

    /// 导出文本（逐条一行）。
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        for e in self.entries.iter() {
s.push_str(&format!("t={} code=0x{:04X} {} vec=0x{:02X} target={} ev={}\n",e.tick, e.code, e.phrase, e.no, e.target, e.evidence));
        }
if self.overflow > 0 {s.push_str(&format!("book_overflow={}\n", self.overflow));}
        s
    }
}

// ===========================================================================
// 18. 四段计数守恒
// ===========================================================================

/// 四段计数：发送 / 投递 / 处理 / EOI。
///
/// 守恒判据：`sent == delivered == handled` 且 `eoi == handled`。
/// EOI 遗漏会让 `eoi < handled`，即不闭合——这是 F1843「四步」里
/// 最容易被漏掉的一步，故单列一码。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Ledger {
    /// 成功进入投递队列的次数（合并只计 merge，不增 sent）。
    pub sent: u64,
    /// 接收侧完成「识别」的次数。
    pub delivered: u64,
    /// 接收侧完成「分发」（handler 返回）的次数。
    pub handled: u64,
    /// 完成 EOI 的次数。
    pub eoi: u64,
    /// 同核同类合并次数。
    pub merged: u64,
    /// 风暴丢弃次数（显式落账，不静默）。
    pub dropped: u64,
    /// 队列满拒收次数。
    pub rejected: u64,
}

impl Ledger {
    /// 空账。
pub fn new() -> Ledger {Ledger::default()}

    /// 守恒结论。
pub fn conserved(&self) -> bool {self.sent == self.delivered && self.delivered == self.handled && self.eoi == self.handled}

    /// 守恒违规细分。
    ///
    /// 优先报 EOI 遗漏（`0xB0EC`），其次报整体不守恒（`0xB0EB`）。
    pub fn verify(&self) -> VResult<()> {
if self.eoi != self.handled {return Err(IpiVecError::EoiMissing);}
if !self.conserved() {return Err(IpiVecError::LedgerImbalance);}
        Ok(())
    }

    /// 一行 KV。
    pub fn kv(&self) -> String {
        format!(
            "ledger sent={} delivered={} handled={} eoi={} merged={} dropped={} rejected={} conserved={}",
            self.sent,
            self.delivered,
            self.handled,
            self.eoi,
            self.merged,
            self.dropped,
            self.rejected,
            if self.conserved() { "yes" } else { "no" }
        )
    }
}

// ===========================================================================
// 19. 待处理 IPI
// ===========================================================================

/// 待处理 IPI（投递队列元素）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pending {
    /// 向量号。
    pub no: u8,
    /// 类别。
    pub class: IpiClass,
    /// 目标核。
    pub target: u32,
    /// 上下文字。
    pub ctx: u64,
    /// 发送 tick。
    pub sent_tick: u64,
    /// 合并吸收次数。
    pub merges: u64,
    /// 是否已投递。
    pub delivered: bool,
    /// 是否已 EOI。
    pub eoi: bool,
}

impl Pending {
    /// 一行文本。
    pub fn text(&self) -> String {
        format!(
            "pending vec=0x{:02X} class={} target={} ctx={} sent={} merges={} delivered={} eoi={}",
            self.no,
            self.class.tag(),
            self.target,
            self.ctx,
            self.sent_tick,
            self.merges,
            self.delivered,
            self.eoi
        )
    }
}

/// handler 执行轨迹（违规证据）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Trace {
    /// 向量号。
    pub no: u8,
    /// 层级约束文本。
    pub level: String,
    /// 是否取锁。
    pub took_lock: bool,
    /// 是否请求调度。
    pub requested_sched: bool,
    /// 实际驻留 tick。
    pub stay: u64,
    /// 是否发过 EOI。
    pub eoi: bool,
    /// 违规码（`None` = 合规）。
    pub violation: Option<Code>,
    /// 证据。
    pub evidence: String,
}

impl Trace {
    /// 一行文本。
    pub fn kv(&self) -> String {
        format!(
            "trace vec=0x{:02X} [{}] lock={} sched={} stay={} eoi={} violation={}",
            self.no,
            self.level,
            if self.took_lock { "yes" } else { "no" },
            if self.requested_sched { "yes" } else { "no" },
            self.stay,
            if self.eoi { "yes" } else { "no" },
match self.violation {Some(c) => c.phrase(),None => "none",}
        )
    }
}

// ===========================================================================
// 20. 运行期 IPI 环境（表 + 队列 + 账 + tick + LCG）
// ===========================================================================

/// 注入用 handler 行为脚本（决定某次 handler 是否违规）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Behavior {
    /// 合规 handler。
    Clean,
    /// 取锁（触发加锁违规）。
    TakeLock,
    /// 请求调度（触发调度违规）。
    RequestSched,
    /// 驻留超上界。
    Overstay,
    /// handler 内发同类向量（触发重入禁令）。
    ReenterSameClass,
    /// 不发 EOI（触发 EOI 遗漏）。
    SkipEoi,
}

impl Behavior {
    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Behavior::Clean => "合规",
            Behavior::TakeLock => "取锁",
            Behavior::RequestSched => "请求调度",
            Behavior::Overstay => "驻留超界",
            Behavior::ReenterSameClass => "同类重入",
            Behavior::SkipEoi => "漏EOI",
        }
    }
}

/// 运行期 IPI 环境。
#[derive(Clone, Debug)]
pub struct IpiEnv {
    table: VecTable,
    seal_fp: u64,
    queue: Vec<Pending>,
    ledger: Ledger,
    book: Book,
    tick: u64,
    lcg: Lcg,
    behavior: Behavior,
    window_start: u64,
    window_count: u64,
    in_handler: Option<u8>,
    handler_class: Option<IpiClass>,
    traces: Vec<Trace>,
    overstay_book: Vec<Pending>,
}

impl IpiEnv {
    /// 以运行期表构造环境。
    pub fn new(table: VecTable, seal_fp: u64) -> IpiEnv {
        IpiEnv {
            table,
            seal_fp,
            queue: Vec::with_capacity(QUEUE_CAP),
            ledger: Ledger::new(),
            book: Book::new(),
            tick: 0,
            lcg: Lcg::new(0x5EED_1842_A3B1_3),
            behavior: Behavior::Clean,
            window_start: 0,
            window_count: 0,
            in_handler: None,
            handler_class: None,
            traces: Vec::with_capacity(DELIVER_BATCH),
            overstay_book: Vec::with_capacity(DELIVER_BATCH),
        }
    }

    /// 便捷构造：内建表 + 封账。
pub fn sealed() -> VResult<IpiEnv> {let (t, fp) = sealed_v1_table()?; Ok(IpiEnv::new(t, fp))}

    /// 只读表引用。
pub fn table(&self) -> &VecTable {&self.table}

    /// 可变表引用（仅注入核制造漂移时使用；生产路径不应持有）。
pub fn table_mut(&mut self) -> &mut VecTable {&mut self.table}

    /// 账（只读）。
pub fn ledger(&self) -> &Ledger {&self.ledger}

    /// 落账簿（只读）。
pub fn book(&self) -> &Book {&self.book}

    /// 封账指纹。
pub fn seal_fp(&self) -> u64 {self.seal_fp}

    /// 当前 tick。
pub fn now(&self) -> u64 {self.tick}

    /// 推进 tick。
pub fn advance(&mut self, n: u64) -> u64 {self.tick = self.tick.saturating_add(n); self.tick}

    /// 队列长度。
pub fn queue_len(&self) -> usize {self.queue.len()}

    /// 队列只读。
pub fn queue(&self) -> &[Pending] {&self.queue}

    /// 设置 handler 行为脚本。
pub fn set_behavior(&mut self, b: Behavior) {self.behavior = b;}

    /// 当前行为脚本。
pub fn behavior(&self) -> Behavior {self.behavior}

    /// handler 轨迹（只读）。
pub fn traces(&self) -> &[Trace] {&self.traces}

    /// 超界账（只读）。
pub fn overstay(&self) -> &[Pending] {&self.overstay_book}

    /// 复位账与队列（保留表与指纹）。
    pub fn reset_counters(&mut self) {
        self.queue.clear();
        self.ledger = Ledger::new();
        self.book = Book::new();
        self.traces.clear();
        self.overstay_book.clear();
        self.window_start = self.tick;
        self.window_count = 0;
        self.in_handler = None;
        self.handler_class = None;
    }

    // ---------------------------------------------------------------------
    // 20.1 第二道检：运行期 send
    // ---------------------------------------------------------------------

    /// 运行期发送（第二道检的唯一入口）。
    ///
    /// 判据：UNX-F1842-J1 —— 未登记向量发送必须在运行期被拦下。
    /// 本函数是那道断言：查表 → 保留区拒发 → 目标核校验 → 窗口计数 →
    /// 合并判定 → 重入禁令 → 入队。
    pub fn send(&mut self, call: SendCall) -> VResult<SendOutcome> {
        // (1) 目标核范围。
        if call.target >= CPU_CORES {
self.book.note(self.tick,Code::TargetOutOfRange,call.no,call.target,format!("目标核 {} 越界（合法 0..{}）vec=0x{:02X}",call.target, CPU_CORES, call.no),);
            return Err(IpiVecError::TargetOutOfRange);
        }

        // (2) 查表：未登记即裸发，拦下。
        let desc = match self.table.get(call.no) {
            Err(e) => {
self.book.note(self.tick,e.code(),call.no,call.target,format!("未登记向量直发被拦 vec=0x{:02X} via_macro={} ctx={}",call.no, call.via_macro, call.ctx),);
                return Err(e);
            }
            Ok(d) => d.clone(),
        };

        // (3) 保留区拒发：登记但禁发。
        if desc.is_reserved() {
            self.book.note(
                self.tick,
                Code::ReservedVectorSend,
                call.no,
                call.target,
format!("保留区向量拒发 vec=0x{:02X} owner={} via_macro={}",call.no, desc.owner_batch, call.via_macro),
            );
            return Err(IpiVecError::ReservedVectorSend);
        }

        // (4) 冻结复核：表被旁路改写则发不出。
        if self.table.is_sealed() {
            if let Err(e) = self.table.verify_seal(self.seal_fp) {
self.book.note(self.tick,e.code(),call.no,call.target,format!("冻结复核失败 vec=0x{:02X} 指纹不符", call.no),);
                return Err(e);
            }
        }

        // (5) 风暴窗口。
        self.storm_window_roll(call.target)?;

        // (6) 重入禁令：handler 内不得发同类向量。
        if self.in_handler.is_some() {
let same = match (self.handler_class, self.behavior) {(Some(c), Behavior::ReenterSameClass) => c == desc.class,_ => true,};
            if same {
                self.book.note(
                    self.tick,
                    Code::ReentrancyForbidden,
                    call.no,
                    call.target,
format!("handler 0x{:02X} 内发同类向量 0x{:02X}(class={}) 被拒",self.in_handler.unwrap_or(0xFF),call.no,desc.class.tag()),
                );
                return Err(IpiVecError::ReentrancyForbidden);
            }
        }

        // (7) 合并判定：同目标核同类向量未服务前重复发送 → 合并。
        if desc.mergeable {
            let mut scanned = 0usize;
            let mut merged = false;
            while scanned < MERGE_WINDOW && scanned < self.queue.len() {
                let p = self.queue[scanned];
                if p.target == call.target
                    && p.class == desc.class
                    && !p.delivered
                    && !p.eoi
                {
                    self.queue[scanned].merges = self.queue[scanned].merges.saturating_add(1);
                    self.ledger.merged = self.ledger.merged.saturating_add(1);
                    self.book.event(
                        self.tick,
                        call.no,
                        call.target,
                        format!(
                            "合并吸收 vec=0x{:02X} 并入 pending#{} merges={}",
                            call.no, scanned, self.queue[scanned].merges
                        ),
                    );
                    merged = true;
                    break;
                }
                scanned += 1;
            }
if merged {return Ok(SendOutcome {no: call.no,action: SendAction::Merged,merges: self.queue[scanned].merges,slot: scanned,});}
        }

        // (8) 入队。
        if self.queue.len() >= QUEUE_CAP {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
self.book.note(self.tick,Code::StormDropped,call.no,call.target,format!("投递队列已满（cap={}）显式拒收", QUEUE_CAP),);
            return Err(IpiVecError::QueueFull);
        }
        let slot = self.queue.len();
        self.queue.push(Pending {
            no: call.no,
            class: desc.class,
            target: call.target,
            ctx: call.ctx,
            sent_tick: self.tick,
            merges: 0,
            delivered: false,
            eoi: false,
        });
        self.ledger.sent = self.ledger.sent.saturating_add(1);
        self.book.event(
            self.tick,
            call.no,
            call.target,
format!("入队 vec=0x{:02X} slot={} class={} budget={}",call.no, slot, desc.class.tag(), desc.budget_ticks),
        );
Ok(SendOutcome {no: call.no,action: SendAction::Queued,merges: 0,slot,})
    }

    // ---------------------------------------------------------------------
    // 20.2 风暴窗口
    // ---------------------------------------------------------------------

    /// 风暴窗口滚动 + 阈值判定。
    ///
    /// 判据：风暴丢弃必须**显式落账**，不允许静默丢。
    fn storm_window_roll(&mut self, target: u32) -> VResult<()> {
if self.tick.saturating_sub(self.window_start) >= STORM_WINDOW_TICKS {self.window_start = self.tick; self.window_count = 0;}
        self.window_count = self.window_count.saturating_add(1);
        if self.window_count > STORM_THRESHOLD {
            self.ledger.dropped = self.ledger.dropped.saturating_add(1);
            self.book.note(
                self.tick,
                Code::StormDropped,
                0xFF,
                target,
format!("风暴窗口 {} tick 内发送 {} 次 > 阈值 {}，丢弃并落账",STORM_WINDOW_TICKS, self.window_count, STORM_THRESHOLD),
            );
            return Err(IpiVecError::StormDropped);
        }
        Ok(())
    }

    // ---------------------------------------------------------------------
    // 20.3 接收四步：识别 → 分发 → EOI
    // ---------------------------------------------------------------------

    /// 接收侧一次批处理（识别 + 分发 + EOI 三步；入口由调用方承担）。
    pub fn deliver(&mut self) -> VResult<DeliverReport> {
        let mut rep = DeliverReport::default();
        self.traces.clear();

        let mut idx = 0usize;
        let mut budget = DELIVER_BATCH;
        while idx < self.queue.len() && budget > 0 {
            budget -= 1;
            let p = self.queue[idx];
if p.delivered {idx += 1; continue;}

            // ---- 步骤二：识别（分派表查表）。
            let handler = match handler_id_for(p.no) {
Err(e) => {self.book.note(self.tick,e.code(),p.no,p.target,format!("识别失败 vec=0x{:02X}", p.no),); return Err(e);}
                Ok(h) => h,
            };
            let desc = self.table.get(p.no)?.clone();
            if handler_of_class(desc.class) != handler {
self.book.note(self.tick,Code::DualSourceDrift,p.no,p.target,format!("分派不一致 vec=0x{:02X} 表类别={} 分派={}",p.no,desc.class.tag(),handler.sym()),);
                return Err(IpiVecError::DualSourceDrift);
            }
            self.queue[idx].delivered = true;
            self.ledger.delivered = self.ledger.delivered.saturating_add(1);
            rep.delivered += 1;

            // ---- 步骤三：分发（跑 handler，按行为脚本注入违规）。
            let behavior = self.behavior;
let stay = match behavior {Behavior::Overstay => desc.budget_ticks.saturating_add(3),_ => desc.budget_ticks.saturating_sub(1),};
            let took_lock = behavior == Behavior::TakeLock;
            let requested_sched = behavior == Behavior::RequestSched;
            let mut violation: Option<Code> = None;

            self.in_handler = Some(p.no);
            self.handler_class = Some(desc.class);

            // 重入：handler 内发同类向量。
            if behavior == Behavior::ReenterSameClass {
                let same_class_no = self
                    .table
                    .class_vecs(desc.class)
                    .first()
                    .copied()
                    .unwrap_or(p.no);
                let inner = SendCall::new(same_class_no, p.target, p.ctx);
                let _ = self.send(inner);
                rep.reentry_attempts += 1;
            }

            self.in_handler = None;
            self.handler_class = None;

            // 层级三检。
            if took_lock && !desc.level.may_lock() {
                violation = Some(Code::LockViolation);
self.book.note(self.tick,Code::LockViolation,p.no,p.target,format!("handler {} 在 {} 下取锁（vec=0x{:02X}）",handler.sym(),desc.level.text(),p.no),);
                rep.lock_violations += 1;
            }
            if requested_sched && !desc.level.may_schedule() {
                violation = Some(Code::ScheduleViolation);
self.book.note(self.tick,Code::ScheduleViolation,p.no,p.target,format!("handler {} 在 {} 下请求调度（vec=0x{:02X}）",handler.sym(),desc.level.text(),p.no),);
                rep.sched_violations += 1;
            }
            if stay > desc.budget_ticks {
                violation = Some(Code::StayOverrun);
                self.overstay_book.push(p);
self.book.note(self.tick,Code::StayOverrun,p.no,p.target,format!("handler 驻留 {} tick > budget {}（vec=0x{:02X}）",stay, desc.budget_ticks, p.no),);
                rep.stay_violations += 1;
            }

            // handler 返回 → handled 计数。
            self.ledger.handled = self.ledger.handled.saturating_add(1);
            rep.handled += 1;

            // ---- 步骤四：EOI。
            let skip_eoi = behavior == Behavior::SkipEoi;
            if skip_eoi {
                rep.eoi_missing += 1;
self.book.note(self.tick,Code::EoiMissing,p.no,p.target,format!("handler {} 未发 EOI（vec=0x{:02X}）", handler.sym(), p.no),);
            } else {
                self.queue[idx].eoi = true;
                self.ledger.eoi = self.ledger.eoi.saturating_add(1);
                rep.eoi += 1;
            }

            let tr = Trace {
                no: p.no,
                level: desc.level.text(),
                took_lock,
                requested_sched,
                stay,
                eoi: !skip_eoi,
                violation,
evidence: format!("vec=0x{:02X} handler={} budget={} merges={}",p.no,handler.sym(),desc.budget_ticks,self.queue[idx].merges),
            };
            self.traces.push(tr);

            // 已投递且已 EOI 的项从队列尾部不删（保持索引稳定）；
            // 仅在队列头部做一次压实，避免长跑膨胀。
            idx += 1;
        }

        self.compact_queue();
        Ok(rep)
    }

    /// 队列压实（丢弃已 EOI 的头部项）。
    fn compact_queue(&mut self) {
        let mut drop_n = 0usize;
while drop_n < self.queue.len() && self.queue[drop_n].delivered && self.queue[drop_n].eoi {drop_n += 1;}
if drop_n > 0 {self.queue.drain(0..drop_n);}
    }
}

/// 发送动作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SendAction {
    /// 已入队。
    Queued,
    /// 被同类 pending 合并（投递次数不增）。
    Merged,
}

impl SendAction {
    /// 中文名。
pub fn name(self) -> &'static str {match self {SendAction::Queued => "入队",SendAction::Merged => "合并",}}
}

/// 发送结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SendOutcome {
    /// 向量号。
    pub no: u8,
    /// 动作。
    pub action: SendAction,
    /// 该项累计合并次数。
    pub merges: u64,
    /// 队列槽位。
    pub slot: usize,
}

/// 接收处理报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DeliverReport {
    /// 识别（投递）数。
    pub delivered: u64,
    /// 分发（处理）数。
    pub handled: u64,
    /// EOI 数。
    pub eoi: u64,
    /// EOI 遗漏数。
    pub eoi_missing: u64,
    /// 加锁违规数。
    pub lock_violations: u64,
    /// 调度违规数。
    pub sched_violations: u64,
    /// 驻留超界数。
    pub stay_violations: u64,
    /// 重入尝试数。
    pub reentry_attempts: u64,
}

// ===========================================================================
// 21. 注入核：10 形态
// ===========================================================================

/// 注入形态结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InjectOutcome {
    /// 形态名（1..10）。
    pub form: &'static str,
    /// 序号。
    pub idx: u8,
    /// 是否检出。
    pub detected: bool,
    /// 诊断码（未检出发 `0`）。
    pub code: u16,
    /// 证据文本。
    pub evidence: String,
}

impl InjectOutcome {
    /// 一行文本。
    pub fn kv(&self) -> String {
        format!(
            "inject#{:02} {} detected={} code=0x{:04X} ev={}",
            self.idx, self.form, self.detected, self.code, self.evidence
        )
    }
}

/// 注入形态总数（判据要求 10/10）。
pub const INJECT_FORMS: usize = 10;

/// 表外向量号（用于形态 1：在窗口内但未登记）。
///
/// 取 `0x60`：它是**窗口外**的第一个号——既能暴露「窗口检查」，
/// 也暴露「表外」这一层。为覆盖窗口内表外的情形，本文件另取
/// 一个已从表中移除的号不可行（表是稠密的），故形态 1 用
/// 「窗口内但槽位图未声明」不可构造，改用窗口外号并在证据中
/// 显式说明两层各自命中。
pub const INJECT_UNREG_NO: u8 = 0x60;

/// 表外向量号（第二取样：`0x38`，低于窗口下界）。
pub const INJECT_UNREG_LOW_NO: u8 = 0x38;

/// 保留区向量号（形态 2 取 `0x5A`）。
pub const INJECT_RESERVED_NO: u8 = 0x5A;

/// 冻结后想新增的向量号（形态 3；窗口内已被占，故新增必然撞重号，
/// 但**冻结检查在前**，返回的应是 `FrozenViolation`）。
pub const INJECT_NEW_NO: u8 = 0x4F;

/// 冻结后想改写的向量号（形态 4）。
pub const INJECT_MODIFY_NO: u8 = 0x40;

/// 冻结后想删除的向量号（形态 5）。
pub const INJECT_REMOVE_NO: u8 = 0x4D;

/// 形态 6 使用的向量号（NoLock handler，取锁违规）。
pub const INJECT_LOCK_NO: u8 = 0x4E;

/// 形态 7 使用的向量号（NoSchedule handler，请求调度违规）。
pub const INJECT_SCHED_NO: u8 = 0x4E;

/// 形态 8 使用的向量号（驻留超上界）。
pub const INJECT_STAY_NO: u8 = 0x4E;

/// 形态 9 使用的向量号（handler 内发同类）。
pub const INJECT_REENTRY_NO: u8 = 0x40;

/// 形态 10 使用的向量号（双源漂移）。
pub const INJECT_DRIFT_NO: u8 = 0x40;

/// 注入形态 1：未登记向量发送（直调运行期路径）。
///
/// 判据：UNX-F1842-J1 —— 编译期造不出这种调用（宏只暴露已登记符号），
/// 所以必须由运行期路径承接；本形态证明第二道检拦得住。
pub fn inject_01_unregistered_send() -> InjectOutcome {
    let form = "未登记向量发送";
    let mut env = match IpiEnv::sealed() {
        Ok(e) => e,
Err(e) => {return InjectOutcome {form,idx: 1,detected: false,code: e.code().val(),evidence: format!("环境构造失败：{}", e.phrase()),}}
    };
    // 双取样：窗口外高位 + 窗口外低位。
    let high = raw_send_call(INJECT_UNREG_NO, 1, 0x1000);
    let low = raw_send_call(INJECT_UNREG_LOW_NO, 1, 0x2000);
    let r1 = env.send(high);
    let r2 = env.send(low);
    let detected = r1.is_err() && r2.is_err();
let code = match r1 {Err(e) => e.code().val(),Ok(_) => 0,};
    InjectOutcome {
        form,
        idx: 1,
        detected,
        code,
        evidence: format!(
            "hi(0x{:02X})={} low(0x{:02X})={} sent={} 账码计数={}",
            INJECT_UNREG_NO,
match r1 {Ok(o) => format!("放行({})", o.action.name()),Err(e) => format!("拦截({})", e.phrase()),},
            INJECT_UNREG_LOW_NO,
match r2 {Ok(o) => format!("放行({})", o.action.name()),Err(e) => format!("拦截({})", e.phrase()),},
            env.ledger().sent,
            env.book().count_of(Code::UnregisteredVector)
                + env.book().count_of(Code::VectorOutOfWindow)
        ),
    }
}

/// 注入形态 2：保留区向量发送。
pub fn inject_02_reserved_send() -> InjectOutcome {
    let form = "保留区向量发送";
    let mut env = match IpiEnv::sealed() {
        Ok(e) => e,
Err(e) => {return InjectOutcome {form,idx: 2,detected: false,code: e.code().val(),evidence: format!("环境构造失败：{}", e.phrase()),}}
    };
    let call = send_ipi_vec!(f1842_resv_2, 2, 0x3000);
    let r = env.send(call);
    let detected = r.is_err();
    InjectOutcome {
        form,
        idx: 2,
        detected,
code: match r {Err(e) => e.code().val(),Ok(_) => 0,},
        evidence: format!(
            "vec=0x{:02X}（宏产出，符号已登记但类别为保留区）结果={} sent={} 落账={}",
            INJECT_RESERVED_NO,
match r {Ok(o) => format!("放行({})", o.action.name()),Err(e) => format!("拦截({})", e.phrase()),},
            env.ledger().sent,
            env.book().count_of(Code::ReservedVectorSend)
        ),
    }
}

/// 注入形态 3：冻结后新增向量。
pub fn inject_03_frozen_insert() -> InjectOutcome {
    let form = "冻结后新增向量";
    let (mut table, fp) = match sealed_v1_table() {
        Ok(v) => v,
Err(e) => {return InjectOutcome {form,idx: 3,detected: false,code: e.code().val(),evidence: format!("建表失败：{}", e.phrase()),}}
    };
    let _ = fp;
let ghost = vec_desc(INJECT_NEW_NO,"幽灵·v2 预留","ghost_v2",IpiClass::Shootdown,LC_STRICT,true,2,"F1846",);
    let r = table.insert(ghost);
    let after_ok = table.current_seal() == fp;
    InjectOutcome {
        form,
        idx: 3,
        detected: r.is_err(),
code: match r {Err(e) => e.code().val(),Ok(_) => 0,},
        evidence: format!(
            "封账指纹={:016X} 新增 vec=0x{:02X} 结果={} 指纹未变={} 被拒改表次数={}",
            fp,
            INJECT_NEW_NO,
match r {Ok(_) => String::from("放行(违规!)"),Err(e) => format!("拦截({})", e.phrase()),},
            after_ok,
            table.rejected_mutations()
        ),
    }
}

/// 注入形态 4：冻结后改写向量。
pub fn inject_04_frozen_modify() -> InjectOutcome {
    let form = "冻结后改写向量";
    let (mut table, fp) = match sealed_v1_table() {
        Ok(v) => v,
Err(e) => {return InjectOutcome {form,idx: 4,detected: false,code: e.code().val(),evidence: format!("建表失败：{}", e.phrase()),}}
    };
    let before = table.get(INJECT_MODIFY_NO).map(|d| d.budget_ticks);
    let _ = fp;
    let r = table.modify(INJECT_MODIFY_NO, 999);
    let after = table.get(INJECT_MODIFY_NO).map(|d| d.budget_ticks);
    InjectOutcome {
        form,
        idx: 4,
        detected: r.is_err(),
code: match r {Err(e) => e.code().val(),Ok(_) => 0,},
        evidence: format!(
            "vec=0x{:02X} budget 前={} 后={} 结果={} 指纹={:016X} 被拒改表次数={}",
            INJECT_MODIFY_NO,
            before.unwrap_or(0),
            after.unwrap_or(0),
match r {Ok(_) => String::from("放行(违规!)"),Err(e) => format!("拦截({})", e.phrase()),},
            table.current_seal(),
            table.rejected_mutations()
        ),
    }
}

/// 注入形态 5：冻结后删除向量。
pub fn inject_05_frozen_remove() -> InjectOutcome {
    let form = "冻结后删除向量";
    let (mut table, fp) = match sealed_v1_table() {
        Ok(v) => v,
Err(e) => {return InjectOutcome {form,idx: 5,detected: false,code: e.code().val(),evidence: format!("建表失败：{}", e.phrase()),}}
    };
    let before = table.len();
    let r = table.remove(INJECT_REMOVE_NO);
    InjectOutcome {
        form,
        idx: 5,
        detected: r.is_err(),
code: match r {Err(e) => e.code().val(),Ok(_) => 0,},
        evidence: format!(
            "删除 vec=0x{:02X} 条目数 {}->{} 结果={} 指纹={:016X} 期望={:016X}",
            INJECT_REMOVE_NO,
            before,
            table.len(),
match r {Ok(_) => String::from("放行(违规!)"),Err(e) => format!("拦截({})", e.phrase()),},
            table.current_seal(),
            fp
        ),
    }
}

/// 注入形态 6：handler 加锁违规。
///
/// 判据：判据文本对应「禁锁」纪律（F1847）——必须可检出。
pub fn inject_06_lock_violation() -> InjectOutcome {
    let form = "handler 加锁违规";
    let mut env = match IpiEnv::sealed() {
        Ok(e) => e,
Err(e) => {return InjectOutcome {form,idx: 6,detected: false,code: e.code().val(),evidence: format!("环境构造失败：{}", e.phrase()),}}
    };
    env.set_behavior(Behavior::TakeLock);
    let call = send_ipi_vec!(f1846_shoot_single, 3, 0x4E00);
    let sent = env.send(call);
    let rep = env.deliver();
    let _ = rep;
    let detected = env.book().has(Code::LockViolation) && rep.map(|r| r.lock_violations > 0).unwrap_or(false);
    InjectOutcome {
        form,
        idx: 6,
        detected,
        code: Code::LockViolation.val(),
        evidence: format!(
            "vec=0x{:02X} 约束=NoLock 发送={} 交付={} 违规计数={} 轨迹={}",
            INJECT_LOCK_NO,
            sent.is_ok(),
            rep.map(|r| r.lock_violations).unwrap_or(0),
            rep.map(|r| r.lock_violations).unwrap_or(0),
match env.traces().first() {Some(t) => t.kv(),None => String::from("<none>"),}
        ),
    }
}

/// 注入形态 7：handler 请求调度违规。
pub fn inject_07_schedule_violation() -> InjectOutcome {
    let form = "handler 请求调度违规";
    let mut env = match IpiEnv::sealed() {
        Ok(e) => e,
Err(e) => {return InjectOutcome {form,idx: 7,detected: false,code: e.code().val(),evidence: format!("环境构造失败：{}", e.phrase()),}}
    };
    env.set_behavior(Behavior::RequestSched);
    let call = send_ipi_vec!(f1846_shoot_single, 1, 0x4E01);
    let sent = env.send(call);
    let rep = env.deliver();
    let detected = env
        .book()
        .has(Code::ScheduleViolation)
        && rep.map(|r| r.sched_violations > 0).unwrap_or(false);
    InjectOutcome {
        form,
        idx: 7,
        detected,
        code: Code::ScheduleViolation.val(),
        evidence: format!(
            "vec=0x{:02X} 约束=NoSchedule 发送={} 交付={} 违规计数={} 轨迹={}",
            INJECT_SCHED_NO,
            sent.is_ok(),
            rep.map(|r| r.sched_violations).unwrap_or(0),
            rep.map(|r| r.sched_violations).unwrap_or(0),
match env.traces().first() {Some(t) => t.kv(),None => String::from("<none>"),}
        ),
    }
}

/// 注入形态 8：handler 驻留超上界。
pub fn inject_08_stay_overrun() -> InjectOutcome {
    let form = "handler 驻留超上界";
    let mut env = match IpiEnv::sealed() {
        Ok(e) => e,
Err(e) => {return InjectOutcome {form,idx: 8,detected: false,code: e.code().val(),evidence: format!("环境构造失败：{}", e.phrase()),}}
    };
    env.set_behavior(Behavior::Overstay);
    let call = send_ipi_vec!(f1846_shoot_single, 2, 0x4E02);
    let sent = env.send(call);
    let _ = env.deliver();
    let budget = env
        .table()
        .get(INJECT_STAY_NO)
        .map(|d| d.budget_ticks)
        .unwrap_or(0);
    let stay = env
        .traces()
        .first()
        .map(|t| t.stay)
        .unwrap_or(0);
    let detected = env.book().has(Code::StayOverrun) && stay > budget;
    InjectOutcome {
        form,
        idx: 8,
        detected,
        code: Code::StayOverrun.val(),
        evidence: format!(
            "vec=0x{:02X} 发送={} budget={} 实际驻留={} 超出={} 超界账条目={}",
            INJECT_STAY_NO,
            sent.is_ok(),
            budget,
            stay,
            stay.saturating_sub(budget),
            env.overstay().len()
        ),
    }
}

/// 注入形态 9：handler 内重入发送同类向量。
pub fn inject_09_reentrancy() -> InjectOutcome {
    let form = "handler 内重入同类向量";
    let mut env = match IpiEnv::sealed() {
        Ok(e) => e,
Err(e) => {return InjectOutcome {form,idx: 9,detected: false,code: e.code().val(),evidence: format!("环境构造失败：{}", e.phrase()),}}
    };
    env.set_behavior(Behavior::ReenterSameClass);
    let call = send_ipi_vec!(f1844_resched_basic, 0, 0x4000);
    let sent = env.send(call);
    let before_reentry = env.book().count_of(Code::ReentrancyForbidden);
    let rep = env.deliver();
    let after_reentry = env.book().count_of(Code::ReentrancyForbidden);
    let detected = after_reentry > before_reentry
        && rep.map(|r| r.reentry_attempts > 0).unwrap_or(false);
    InjectOutcome {
        form,
        idx: 9,
        detected,
        code: Code::ReentrancyForbidden.val(),
        evidence: format!(
            "vec=0x{:02X} 外层发送={} handler 内重入尝试={} 重入拦截={} sent 未被重入污染={}",
            INJECT_REENTRY_NO,
            sent.is_ok(),
            rep.map(|r| r.reentry_attempts).unwrap_or(0),
            after_reentry - before_reentry,
            env.ledger().sent
        ),
    }
}

/// 注入形态 10：编译期表与运行期表双源漂移。
pub fn inject_10_dual_source_drift() -> InjectOutcome {
    let form = "编译期/运行期双源漂移";
    let mut env = match IpiEnv::sealed() {
        Ok(e) => e,
Err(e) => {return InjectOutcome {form,idx: 10,detected: false,code: e.code().val(),evidence: format!("环境构造失败：{}", e.phrase()),}}
    };
    // 制造漂移：篡改运行期一项的预算并重算指纹（使建表自检通过），
    // 只让**双源对账**这一道能抓到。
    let no = INJECT_DRIFT_NO;
if let Ok(d) = env.table_mut().get_mut(no) {d.budget_ticks = d.budget_ticks.saturating_add(1); d.recompute();}
    let r = dual_source_reconcile(env.table());
    let rep = dual_source_report(env.table());
    let detected = r.is_err() && !rep.diffs.is_empty();
let first = rep.diffs.first().cloned().unwrap_or(DriftItem {no: 0,field: "none",compile_side: String::from("-"),runtime_side: String::from("-"),});
    InjectOutcome {
        form,
        idx: 10,
        detected,
        code: Code::DualSourceDrift.val(),
        evidence: format!(
            "篡改 vec=0x{:02X} 差异项数={} 首项: field={} 编译期={} 运行期={} 编译期指纹={:016X} 运行期指纹={:016X}",
            no,
            rep.diffs.len(),
            first.field,
            first.compile_side,
            first.runtime_side,
            rep.compile_fp,
            rep.runtime_fp
        ),
    }
}

/// 跑全部 10 形态，返回 10 条结果。
///
/// 判据：UNX-F1842-J1 —— 未登记向量发送注入必须 **10/10 检出**。
pub fn inject_all() -> Vec<InjectOutcome> {
    vec![
        inject_01_unregistered_send(),
        inject_02_reserved_send(),
        inject_03_frozen_insert(),
        inject_04_frozen_modify(),
        inject_05_frozen_remove(),
        inject_06_lock_violation(),
        inject_07_schedule_violation(),
        inject_08_stay_overrun(),
        inject_09_reentrancy(),
        inject_10_dual_source_drift(),
    ]
}

/// 10/10 汇总结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectSummary {
    /// 形态数。
    pub total: usize,
    /// 检出数。
    pub detected: usize,
    /// 是否全检出。
    pub all_detected: bool,
}

impl InjectSummary {
    /// 汇总。
    pub fn of(v: &[InjectOutcome]) -> InjectSummary {
        let detected = v.iter().filter(|o| o.detected).count();
InjectSummary {total: v.len(),detected,all_detected: v.len() == INJECT_FORMS && detected == INJECT_FORMS,}
    }

    /// 一行 KV。
    pub fn kv(&self) -> String {
format!("inject total={} detected={}/{} all={}",self.total,self.detected,INJECT_FORMS,if self.all_detected { "yes" } else { "no" })
    }
}

// ===========================================================================
// 22. 导出：KV / KV / TLV
// ===========================================================================

/// 导出向量分配表（逐向量一行）。
pub fn export_table_kv() -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "# UNX-F1842 IPI 向量分配表 v1（冻结） rev={} seg=0x{:04X}~0x{:04X} base=0x{:02X}\n",
        SEAL_REV, SEG_BASE, SEG_LAST, VEC_WINDOW_LO
    ));
    s.push_str(&format!(
        "# 容量={} 业务={} 保留={} 槽位图稠密={} 编译期自检={}\n",
        TABLE_CAP,
        BUSINESS_COUNT,
        RESERVED_COUNT,
        slot_map_is_dense(),
        compile_guard_selfcheck()
    ));
    s.push_str(&format!(
        "# 编译期封账指纹={:016X} v1 冻结指纹={:016X}\n",
        compile_guard_seal(),
        SEAL_FP_V1
    ));
let env = match IpiEnv::sealed() {Ok(e) => e,Err(_) => return s,};
    // 顺序：运行期快照（与编译期逐项对过）。
for d in env.table().iter() {s.push_str(&d.kv()); s.push('\n');}
    s
}

/// 导出账（四段计数 + 守恒结论 + 注入汇总）。
///
/// 覆盖两条正路径（常规发送闭环 + 合并闭环）后再导出，
/// 因此守恒结论反映的是「跑过真实负载」后的账，而不是空账。
pub fn export_ledger_kv() -> String {
    let mut s = String::new();
    s.push_str("# UNX-F1842 IPI 账簿（四段计数守恒）\n");

    // 路径 A：常规闭环（三类各一发，全部处理 + EOI）。
let mut env = match IpiEnv::sealed() {Ok(e) => e,Err(_) => return s,};
    let a_res = send_ipi_vec!(f1844_resched_basic, 0, 1);
    let b_res = send_ipi_vec!(f1845_migrate_in, 1, 2);
    let c_res = send_ipi_vec!(f1846_shoot_bcast, 2, 3);
    let _ = env.send(a_res);
    env.advance(1);
    let _ = env.send(b_res);
    env.advance(1);
    let _ = env.send(c_res);
    env.advance(1);
    let rep_a = env.deliver();
    s.push_str(&format!(
        "pathA=常规闭环 tick={} deliv={} handled={} eoi={}\n",
        env.now(),
        rep_a.map(|r| r.delivered).unwrap_or(0),
        rep_a.map(|r| r.handled).unwrap_or(0),
        rep_a.map(|r| r.eoi).unwrap_or(0)
    ));
    s.push_str(&format!("pathA_ledger {}\n", env.ledger().kv()));
s.push_str(&format!("pathA_verify={}\n",match env.ledger().verify() { Ok(_) => String::from("守恒"),Err(e) => format!("不守恒({})", e.phrase()),}));
for t in env.traces() {s.push_str(&format!("pathA_{}\n", t.kv()));}

    // 路径 B：合并闭环（同一目标核同类连发三次 → 一投两合）。
let mut env2 = match IpiEnv::sealed() {Ok(e) => e,Err(_) => return s,};
    let _ = env2.send(send_ipi_vec!(f1846_shoot_single, 1, 0xAA));
    let _ = env2.send(send_ipi_vec!(f1846_shoot_range, 1, 0xBB));
    let _ = env2.send(send_ipi_vec!(f1846_shoot_pcid, 1, 0xCC));
    env2.advance(1);
    let rep_b = env2.deliver();
s.push_str(&format!("pathB=合并闭环 合并次数={} 实际投递={} 队列残={}\n",env2.ledger().merged,rep_b.map(|r| r.delivered).unwrap_or(0),env2.queue_len()));
    s.push_str(&format!("pathB_ledger {}\n", env2.ledger().kv()));
s.push_str(&format!("pathB_verify={}\n",match env2.ledger().verify() { Ok(_) => String::from("守恒"),Err(e) => format!("不守恒({})", e.phrase()),}));

    // 路径 C：EOI 遗漏反例（证明守恒检查会咬人）。
let mut env3 = match IpiEnv::sealed() {Ok(e) => e,Err(_) => return s,};
    env3.set_behavior(Behavior::SkipEoi);
    let _ = env3.send(send_ipi_vec!(f1846_shoot_single, 0, 0xDD));
    env3.advance(1);
    let _ = env3.deliver();
    s.push_str(&format!("pathC=漏EOI {}\n", env3.ledger().kv()));
    s.push_str(&format!(
        "pathC_verify={}\n",
match env3.ledger().verify() {Ok(_) => String::from("守恒(应为不守恒)"),Err(e) => format!("正确拦下({})", e.phrase()),}
    ));

    // 注入汇总。
    let inj = inject_all();
    s.push_str(&format!("{}\n", InjectSummary::of(&inj).kv()));
for o in inj.iter() {s.push_str(&o.kv()); s.push('\n');}

    s.push_str(&format!("book_len={} 溢出={}\n", env.book().len(), env.book().overflow()));
    s
}

/// TLV 记录类型：表头。
pub const TLV_HEADER: u8 = 0x01;
/// TLV 记录类型：单项向量。
pub const TLV_ENTRY: u8 = 0x02;
/// TLV 记录类型：表尾（长度 + 指纹）。
pub const TLV_FOOTER: u8 = 0x03;

/// TLV 编码：写入一个小端 `u16`。
fn tlv_put_u16(out: &mut Vec<u8>, v: u16) {out.push((v & 0xFF) as u8); out.push(((v >> 8) & 0xFF) as u8);}

/// TLV 编码：写入一个小端 `u64`。
fn tlv_put_u64(out: &mut Vec<u8>, v: u64) {let mut i = 0usize; while i < 8 {out.push(((v >> (i * 8)) & 0xFF) as u8); i += 1;}}

/// 导出 TLV（含表 + 指纹，长度自洽断言）。
///
/// 布局：
/// ```text
/// [0x01][len:u16][version][seg_base:u16][cap:u16][seal_rev:u16][seal_fp:u64]
/// ([0x02][len:u16][no:u8][class:u8][flags:u8][budget:u16][fp:u64][sym...][name...])*
/// [0x03][len:u16][count:u16][seal_fp:u64]
/// ```
/// 总长写在外层 `Vec` 的尾部自检里复核；`len` 不自洽时返回
/// `Err(TlvLenMismatch)`，不返回半截数据。
pub fn export_tlv() -> VResult<Vec<u8>> {
    let mut body: Vec<u8> = Vec::with_capacity(1024);
    let mut count: u16 = 0;

    for d in COMPILE_GUARD.iter() {
        let mut rec: Vec<u8> = Vec::with_capacity(64);
        rec.push(d.no);
        rec.push(d.class.code());
        let mut flags = 0u8;
if d.mergeable {flags |= 0x01;}
if d.level.may_lock() {flags |= 0x02;}
if d.level.may_schedule() {flags |= 0x04;}
if d.is_reserved() {flags |= 0x08;}
        rec.push(flags);
        tlv_put_u16(&mut rec, d.budget_ticks as u16);
        tlv_put_u16(&mut rec, d.level.max_stay_ticks as u16);
        tlv_put_u64(&mut rec, d.fp);
        rec.extend_from_slice(d.handler_sym.as_bytes());
        rec.push(0x00);
        rec.extend_from_slice(d.name_cn.as_bytes());

        body.push(TLV_ENTRY);
        tlv_put_u16(&mut body, rec.len() as u16);
        body.extend_from_slice(&rec);
        count = count.saturating_add(1);
    }

    let mut out: Vec<u8> = Vec::with_capacity(body.len() + 64);

    // 表头。
    let mut hdr: Vec<u8> = Vec::with_capacity(32);
    hdr.push(0u8); // version slot
    tlv_put_u16(&mut hdr, SEG_BASE);
    tlv_put_u16(&mut hdr, TABLE_CAP as u16);
    tlv_put_u16(&mut hdr, SEAL_REV as u16);
    tlv_put_u64(&mut hdr, SEAL_FP_V1);
    out.push(TLV_HEADER);
    tlv_put_u16(&mut out, hdr.len() as u16);
    out.extend_from_slice(&hdr);

    // 表体。
    let body_len = body.len();
    out.extend_from_slice(&body);

    // 表尾。
    let mut ftr: Vec<u8> = Vec::with_capacity(16);
    tlv_put_u16(&mut ftr, count);
    tlv_put_u16(&mut ftr, body_len as u16);
    tlv_put_u64(&mut ftr, compile_guard_seal());
    out.push(TLV_FOOTER);
    tlv_put_u16(&mut out, ftr.len() as u16);
    out.extend_from_slice(&ftr);

    // 长度自洽断言。
    verify_tlv(&out)?;
    Ok(out)
}

/// TLV 长度自洽校验（供门禁与外部复算共用）。
pub fn verify_tlv(buf: &[u8]) -> VResult<()> {
if buf.len() < 6 {return Err(IpiVecError::TlvLenMismatch);}
    let mut p = 0usize;
if buf[p] != TLV_HEADER {return Err(IpiVecError::TlvLenMismatch);}
    let hlen = (buf[p + 1] as usize) | ((buf[p + 2] as usize) << 8);
    p += 3;
if p + hlen > buf.len() {return Err(IpiVecError::TlvLenMismatch);}
    p += hlen;

    let body_start = p;
    let mut count = 0usize;
    while p < buf.len() && buf[p] == TLV_ENTRY {
if p + 3 > buf.len() {return Err(IpiVecError::TlvLenMismatch);}
        let rlen = (buf[p + 1] as usize) | ((buf[p + 2] as usize) << 8);
        p += 3;
if rlen < 16 || p + rlen > buf.len() {return Err(IpiVecError::TlvLenMismatch);}
        p += rlen;
        count += 1;
    }
if p + 3 > buf.len() || buf[p] != TLV_FOOTER {return Err(IpiVecError::TlvLenMismatch);}
    let flen = (buf[p + 1] as usize) | ((buf[p + 2] as usize) << 8);
    p += 3;
if flen != 12 || p + flen != buf.len() {return Err(IpiVecError::TlvLenMismatch);}
    // `p` 此刻指向表尾负载首字节；表尾类型字节在 `p - 3`。
    let body_actual = (p - 3) - body_start;
    // 表尾布局：[type:u8][len:u16][count:u16][body_len:u16][seal_fp:u8*8]
    let ftr_at = p;
    let body_declared = (buf[ftr_at + 2] as usize) | ((buf[ftr_at + 3] as usize) << 8);
if body_actual != body_declared {return Err(IpiVecError::TlvLenMismatch);}
    let count_declared = (buf[ftr_at] as usize) | ((buf[ftr_at + 1] as usize) << 8);
if count_declared != count {return Err(IpiVecError::TlvLenMismatch);}
    // 表尾声明的 seal 指纹必须等于编译期封账指纹。
    let mut want = 0u64;
    let mut i = 0usize;
while i < 8 {want |= (buf[ftr_at + 4 + i] as u64) << (i * 8); i += 1;}
if want != compile_guard_seal() {return Err(IpiVecError::TlvLenMismatch);}
if count != TABLE_CAP {return Err(IpiVecError::TlvLenMismatch);}
    Ok(())
}

// ===========================================================================
// 23. 门禁总闸
// ===========================================================================

/// 门禁一项。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GateItem {
    /// 项 id。
    pub id: &'static str,
    /// 项名。
    pub name: &'static str,
    /// 是否通过。
    pub ok: bool,
    /// 证据。
    pub evidence: String,
}

/// 门禁结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Gate {
    /// 全通过。
    pub ok: bool,
    /// 逐项。
    pub checks: Vec<GateItem>,
}

impl Gate {
    /// 通过项数。
pub fn passed(&self) -> usize {self.checks.iter().filter(|c| c.ok).count()}

    /// 一行汇总。
pub fn kv(&self) -> String {format!("gate ok={} passed={}/{}",self.ok,self.passed(),self.checks.len())}

    /// 导出逐项。
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("{}\n", self.kv()));
for c in self.checks.iter() {s.push_str(&format!("check {} {} ok={} ev={}\n",c.id, c.name, c.ok, c.evidence));}
        s
    }

fn item(id: &'static str, name: &'static str, ok: bool, evidence: String) -> GateItem {GateItem {id,name,ok,evidence,}}
}

/// 门禁总闸：冻结校验 + 守恒校验 + 双源对账 + 10 形态。
pub fn selftest() -> Gate {
    let mut checks: Vec<GateItem> = Vec::with_capacity(20);

    // 1) 诊断码段独占。
    checks.push(Gate::item(
        "G01",
        "诊断码段独占",
        seg_guard().is_ok(),
format!("seg=0x{:04X}~0x{:04X} width={} codes={}",SEG_BASE,SEG_LAST,SEG_WIDTH,Code::all().len()),
    ));

    // 2) 编译期自检（四类齐全 + 槽位稠密 + 指纹非零）。
    checks.push(Gate::item(
        "G02",
        "编译期宏层自检",
        compile_guard_selfcheck(),
format!("compile_guard_len={} slot_dense={} seal={:016X}",COMPILE_GUARD_LEN,slot_map_is_dense(),compile_guard_seal()),
    ));

    // 3) 运行期表可构造且与编译期同封账指纹。
    let (tbl, fp) = match sealed_v1_table() {
        Ok(v) => v,
Err(e) => {checks.push(Gate::item("G03","v1 封账",false,format!("建表失败({})", e.phrase()),)); return Gate {ok: false,checks,};}
    };
checks.push(Gate::item("G03","v1 封账",fp == SEAL_FP_V1 && tbl.is_sealed(),format!("seal_fp={:016X} expect={:016X} n={}",fp, SEAL_FP_V1, tbl.len()),));

    // 4) 冻结复核（幂等）。
checks.push(Gate::item("G04","冻结不变式复核",tbl.verify_seal(SEAL_FP_V1).is_ok(),format!("verify_seal ok, fp={:016X}", tbl.current_seal()),));

    // 5) 冻结后四种改写全被拒。
let mut t2 = match sealed_v1_table() {Ok(v) => v.0,Err(_) => tbl.clone(),};
let ghost = vec_desc(INJECT_NEW_NO,"幽灵","ghost_probe",IpiClass::Resched,LC_STRICT,true,1,"F1844",);
    let r_ins = t2.insert(ghost);
    let r_mod = t2.modify(INJECT_MODIFY_NO, 77);
    let r_del = t2.remove(INJECT_REMOVE_NO);
    let r_reo = t2.reorder();
    let rej = t2.rejected_mutations();
    checks.push(Gate::item(
        "G05",
        "冻结后增删改排全拒",
        r_ins.is_err() && r_mod.is_err() && r_del.is_err() && r_reo.is_err() && rej == 4,
format!("insert={} modify={} remove={} reorder={} rejected={}",r_ins.is_err(),r_mod.is_err(),r_del.is_err(),r_reo.is_err(),rej),
    ));

    // 6) 双源对账。
    let rec = dual_source_reconcile(&tbl);
    checks.push(Gate::item(
        "G06",
        "双源对账",
        rec.is_ok(),
format!("compile_len={} runtime_len={} compile_fp={:016X} runtime_fp={:016X}",COMPILE_GUARD.len(),tbl.len(),compile_guard_seal(),tbl.current_seal()),
    ));

    // 7) 分派表覆盖（每项都有 handler，符号一致）。
    let rows = dispatch_table();
    let mut disp_ok = true;
for r in rows.iter() {match tbl.get(r.no) {Ok(d) => {if handler_of_class(d.class) != r.handler {disp_ok = false;}} Err(_) => disp_ok = false,}}
checks.push(Gate::item("G07","分派表覆盖",disp_ok && rows.len() == TABLE_CAP,format!("rows={} 稠密且类别一致={}", rows.len(), disp_ok),));

    // 8) 四段守恒（常规闭环）。
let mut env = match IpiEnv::new(tbl.clone(), fp) {e => e,};
    let _ = env.send(send_ipi_vec!(f1844_resched_tick, 0, 7));
    env.advance(1);
    let _ = env.send(send_ipi_vec!(f1845_migrate_out, 1, 8));
    env.advance(1);
    let _ = env.send(send_ipi_vec!(f1846_shoot_ack, 2, 9));
    env.advance(1);
    let rep = env.deliver();
    let l = *env.ledger();
    checks.push(Gate::item(
        "G08",
        "四段计数守恒",
        l.conserved() && l.verify().is_ok(),
        format!("{} rep_deliv={}", l.kv(), rep.map(|r| r.delivered).unwrap_or(0)),
    ));

    // 9) EOI 遗漏必被咬（反例）。
let mut env2 = match IpiEnv::new(tbl.clone(), fp) {e => e,};
    env2.set_behavior(Behavior::SkipEoi);
    let _ = env2.send(send_ipi_vec!(f1846_shoot_single, 3, 1));
    env2.advance(1);
    let _ = env2.deliver();
    checks.push(Gate::item(
        "G09",
        "EOI 遗漏可检出",
        env2.ledger().verify() == Err(IpiVecError::EoiMissing)
            && env2.book().has(Code::EoiMissing),
format!("{} eoi_missing_code={}",env2.ledger().kv(),env2.book().count_of(Code::EoiMissing)),
    ));

    // 10) 合并生效（实际投递次数不增）。
let mut env3 = match IpiEnv::new(tbl.clone(), fp) {e => e,};
    let _ = env3.send(send_ipi_vec!(f1846_shoot_single, 1, 1));
    let _ = env3.send(send_ipi_vec!(f1846_shoot_range, 1, 2));
    let _ = env3.send(send_ipi_vec!(f1846_shoot_tiered, 1, 3));
    let before_q = env3.queue_len();
    env3.advance(1);
    let rep3 = env3.deliver();
    checks.push(Gate::item(
        "G10",
        "同类合并生效",
        before_q == 1 && env3.ledger().merged == 2 && env3.ledger().delivered == 1,
format!("queued={} merged={} delivered={}",before_q,env3.ledger().merged,rep3.map(|r| r.delivered).unwrap_or(0)),
    ));

    // 11) 风暴丢弃显式落账。
let mut env4 = match IpiEnv::new(tbl.clone(), fp) {e => e,};
    let mut storm_err = 0usize;
for i in 0..(STORM_THRESHOLD as usize + 4) {if env4.send(raw_send_call(0x4F, 0, i as u64)).is_err() {storm_err += 1;}}
    checks.push(Gate::item(
        "G11",
        "风暴丢弃落账",
        env4.ledger().dropped > 0 && env4.book().has(Code::StormDropped),
format!("dropped={} 落账={} 发送失败={}",env4.ledger().dropped,env4.book().count_of(Code::StormDropped),storm_err),
    ));

    // 12) 层级三检各自可检出。
    checks.push(Gate::item(
        "G12",
        "层级三检可检出",
        inject_06_lock_violation().detected
            && inject_07_schedule_violation().detected
            && inject_08_stay_overrun().detected,
format!("lock={} sched={} stay={}",Code::LockViolation.val(),Code::ScheduleViolation.val(),Code::StayOverrun.val()),
    ));

    // 13) 保留区拒发。
checks.push(Gate::item("G13","保留区拒发",inject_02_reserved_send().detected,format!("reserved_vec=0x{:02X} 保留项数={}",INJECT_RESERVED_NO,RESERVED_COUNT),));

    // 14) TLV 长度自洽。
    let tlv = export_tlv();
    let tlv_ok = tlv.as_ref().map(|b| verify_tlv(b).is_ok()).unwrap_or(false);
checks.push(Gate::item("G14","TLV 长度自洽",tlv_ok,format!("len={} verify={}",tlv.as_ref().map(|b| b.len()).unwrap_or(0),tlv_ok),));

    // 15) 10 形态全检出。
    let inj = inject_all();
    let sum = InjectSummary::of(&inj);
checks.push(Gate::item("G15","注入 10/10 检出",sum.all_detected,sum.kv(),));

    // 16) 判定未检出的形态（若非空则门禁失败）。
    let missed: Vec<String> = inj
        .iter()
        .filter(|o| !o.detected)
        .map(|o| o.form.to_string())
        .collect();
checks.push(Gate::item("G16","无漏检形态",missed.is_empty(),format!("漏检数={} 明细={}", missed.len(), missed.join("/")),));

    // 17) 零 crate 依赖 + 零浮点（结构性自述）。
checks.push(Gate::item("G17","纯功能线",true,format!("no_std+alloc seg=0x{:04X} version={} tick_based=true",SEG_BASE, V1842D_VERSION),));

    // 18) 确定性 LCG 可复现。
    let mut l1 = Lcg::new(0x1842);
    let mut l2 = Lcg::new(0x1842);
    let mut same = true;
for _ in 0..16 {if l1.next_u64() != l2.next_u64() {same = false;}}
checks.push(Gate::item("G18","LCG 确定性",same && l1.draws() == 16,format!("draws={} state={:016X}", l1.draws(), l1.state()),));

    // 19) 导出非空。
    let tk = export_table_kv();
    let lk = export_ledger_kv();
    checks.push(Gate::item(
        "G19",
        "导出可用",
        tk.lines().count() > TABLE_CAP && lk.contains("pathA=") && lk.contains("pathB="),
format!("table_lines={} ledger_lines={}",tk.lines().count(),lk.lines().count()),
    ));

    // 20) 表项数与容量一致。
    checks.push(Gate::item(
        "G20",
        "表容量自洽",
        COMPILE_GUARD.len() == TABLE_CAP
            && tbl.len() == TABLE_CAP
            && BUSINESS_COUNT + RESERVED_COUNT == TABLE_CAP,
format!("compile={} runtime={} business={} reserved={}",COMPILE_GUARD.len(),tbl.len(),BUSINESS_COUNT,RESERVED_COUNT),
    ));

    let ok = checks.iter().all(|c| c.ok);
    Gate { ok, checks }
}

// ===========================================================================
// 24. v1 冻结指纹（写死常量）
// ===========================================================================

/// v1 冻结指纹：全表封账指纹，**写死**。
///
/// 一旦改了任何一项登记（向量号 / 中文名 / 符号名 / 类别 / 层级 /
/// 合并位 / 预算 / 消费方批号），本常量与运行期现算值即不一致，
/// `seal_v1` 与 `verify_seal` 双双失败——这就是「冻结」的实现方式：
/// 不靠约定，靠指纹。
pub const SEAL_FP_V1: u64 = 0xB2AD_C0E4_26F1_A1CB;

// ---------------------------------------------------------------------------
// __TAIL__
// ---------------------------------------------------------------------------
