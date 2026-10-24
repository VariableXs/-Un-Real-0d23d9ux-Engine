//! unx0344_percpu —— UNX-F0344 AP per-CPU 区基址切换与自识别（UNX AP 启动域）。
//!
//! UNX-F0344 · AP per-CPU 区基址切换与自识别 · 诊断段 0x1400~0x140F
//! 验收锚点：UNX-F0344-J1 GS base 切到 AP 实例，self_ptr 自识别断言过（F0192 语义复用）
//! 定位：AP per-CPU 基址切换与自识别
//!      （主册《CoRun Varix STAR II · Unxreal》UNX-A1 批次表行 UNX-F0344：
//!       功能条目 "AP per-CPU 基址切换与自识别"，判据原文如上。）
//!
//! 语义上游：UNX-F0192（fs/gs base 设置与 per-CPU 预备）——本模块复用其
//!   "per-CPU 区访问经 self_ptr 自识别" 的判定语义：GS base 指向哪个区，
//!   该区头的 self_ptr 读回必须等于区基址；UNX-F0328 已按此语义实例化
//!   各 AP 的栈与 per-CPU 区，本模块负责 "切过去之后认得出自己" 的
//!   最后一道闸（BSP → AP 换防），并把切坏的区关进隔离账。
//!
//! 组件清单（全部真实现，无占位）：
//!   1. RawArea / PerCpuArea —— 每 AP 独立实例区（8KiB 定容，≤64KiB 编译期
//!      断言），字节级布局：magic + cpu_id + self_ptr 自指 + 栈基址/栈长 +
//!      16 槽统计 + 尾 magic；const 布局表 + 偏移校验（编译期 + 运行期双份）。
//!   2. GsBaseBackend / RamGsBackend —— GS base 写入面 trait 注入（抽象建模，
//!      不写真实 wrmsr；RamGsBackend 可注入写读错位故障）。
//!   3. GsBaseSwitcher —— GS base 切换语义模型：切换前验签（目标区三自洽：
//!      magic / cpu_id / self_ptr，不自洽拒切）、写后读回、切换后自识别
//!      （F0192 语义复用：self_ptr 读回 == 实例基址）、两段式状态机
//!      （BspResident → Switching → ApResident）+ 双切换冲突拦截 +
//!      BSP/AP 互斥 + 切换账（SwitchRecord 定容账本）。
//!   4. CpuIdRegistry —— cpu_id → 区基址映射表：注册制、重复 cpu_id 拒收、
//!      重复基址拒收、BSP 固定槽 0（cpu_id=0 只能由 register_bsp 占用）。
//!   5. verify_self_identity —— 自识别协议：全区扫描 self_ptr==基址，
//!      失败区进 IsolationLedger 隔离账（带诊断码），账满显性丢弃计数。
//!   6. 注入面 —— magic / cpu_id / self_ptr 三类错注入逐一检出并修复回归；
//!      GS 写读错位故障注入检出并回滚。
//!   7. 事件账 EventLedger（FNV-1a 64 指纹）+ health() 体检 +
//!      双跑确定性对拍（Lcg 驱动 orchestrate 两次，指纹必须一致）。
//!
//! 纪律：no_std + alloc；内核路径零 unwrap/expect/panic!/运行期 assert!
//!   （编译期 const assert 不受限）；可失败全走 Result<T, PerCpuError>；
//!   计数一律 saturating/wrapping 显性；>64KiB 结构体禁上栈（本模块最大
//!   结构体为 8KiB 的 PerCpuArea，远低于线，但仍建议调用方以堆容器持有
//!   成片 AP 区）；const fn 内不使用 slice::get / Vec 索引 / Ord::min/max /
//!   派生 PartialEq 的 == / for 循环（一律 while + 索引）。

use alloc::{format, string::String, vec::Vec};

/// 模块版本（落点名 + 版次，账面对账用）。
pub const PC_VERSION: &str = "unx0344_percpu-v1";

/// 诊断段基址：0x1400（开工前全域 grep unxstar + cgpu 确认 0x1400~0x140F
/// 无既有诊断段占用；既有 "0x140" 命中均为地址/种子字面量，非诊断码）。
/// 0x1400 本身保留为段基，不分配给任何单条诊断码。
pub const PC_SEG_BASE: u16 = 0x1400;

// ---------------------------------------------------------------------------
// 诊断码闭集（0x1401 ~ 0x140F，段内独占，全域唯一）
// ---------------------------------------------------------------------------

/// 区头 magic 不匹配（含尾 magic 完整性破坏）。
pub const XC_MAGIC_MISMATCH: u32 = 0x1401;
/// 区头 cpu_id 与注册真值不符。
pub const XC_CPUID_MISMATCH: u32 = 0x1402;
/// self_ptr 读回 != 实例基址（F0192 自识别失败）。
pub const XC_SELF_PTR_MISMATCH: u32 = 0x1403;
/// cpu_id / 基址对未注册（注册制拒切）。
pub const XC_NOT_REGISTERED: u32 = 0x1404;
/// 重复 cpu_id 注册拒收。
pub const XC_DUP_CPU_ID: u32 = 0x1405;
/// 注册表容量满。
pub const XC_REGISTRY_FULL: u32 = 0x1406;
/// BSP 固定槽 0 被越权占用。
pub const XC_BSP_SLOT: u32 = 0x1407;
/// 状态机非法迁移。
pub const XC_STATE_INVALID: u32 = 0x1408;
/// 双切换冲突（Switching 期间再次 begin）。
pub const XC_DOUBLE_SWITCH: u32 = 0x1409;
/// 事件账 / 切换账容量满（含显性丢弃计数）。
pub const XC_LEDGER_FULL: u32 = 0x140A;
/// 隔离账容量满（含显性丢弃计数）。
pub const XC_ISOLATE_FULL: u32 = 0x140B;
/// 双跑确定性对拍指纹不一致。
pub const XC_FINGERPRINT_MISMATCH: u32 = 0x140C;
/// 注入检出泄漏（注入后验签未报错 / 报错码不符）。
pub const XC_INJECT_LEAK: u32 = 0x140D;
/// GS base 写后读回 != 写入值（落靶失败）。
pub const XC_READBACK_MISMATCH: u32 = 0x140E;
/// 重复基址注册拒收（一个区基址只能挂一个 cpu_id）。
pub const XC_DUP_BASE: u32 = 0x140F;

// ---------------------------------------------------------------------------
// PerCpuError —— 错误闭集（每变体带中文可操作诊断 detail()）
// ---------------------------------------------------------------------------

/// 本模块唯一错误类型：所有可失败路径一律 `Result<T, PerCpuError>`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PerCpuError {
    /// 区头 magic 不匹配：expected 为期望魔数，found 为读回值。
    MagicMismatch { expected: u32, found: u32 },
    /// 区头 cpu_id 与结构体记账真值不符。
    CpuIdMismatch { expected: u32, found: u32 },
    /// self_ptr 读回与区基址不符（自识别失败，F0192 语义）。
    SelfPtrMismatch { base: u64, found: u64 },
    /// 尾部 magic 被破坏（越界写嫌疑）。
    FooterMismatch { expected: u32, found: u32 },
    /// cpu_id / 基址对未注册。
    NotRegistered { cpu_id: u32, base: u64 },
    /// 重复 cpu_id。
    DuplicateCpuId { cpu_id: u32 },
    /// 重复基址。
    DuplicateBase { base: u64 },
    /// 注册表满。
    RegistryFull { cap: usize },
    /// 越权占用 BSP 固定槽（cpu_id=0）。
    BspSlotViolation { attempted_cpu: u32 },
    /// 状态机非法迁移：need 为要求的态码，got 为当前态码。
    InvalidState { need: u8, got: u8 },
    /// 双切换冲突：pending_cpu 为在途切换的目标。
    DoubleSwitchConflict { pending_cpu: u32 },
    /// 事件账满（账内已计 dropped，调用方可选择忽略本错误）。
    LedgerFull { cap: usize },
    /// 切换账（记录簿）满（账内已计 dropped）。
    RecordsFull { cap: usize },
    /// 隔离账满（账内已计 dropped）。
    IsolateLedgerFull { cap: usize },
    /// GS base 写后读回不符。
    ReadbackMismatch { wrote: u64, read: u64 },
    /// 注入检出泄漏：注入后验签行为与预期不符。
    InjectionLeak { kind: u8, xcode: u32 },
    /// 区内偏移越界（off + size 超出区面积）。
    BadOffset { off: usize, size: usize },
    /// 对齐违规。
    BadAlign { value: u64, align: u64 },
    /// 栈配置非法（零长 / 超上限 / 尾溢出）。
    BadStackConfig,
    /// 双跑对拍指纹不一致。
    FingerprintMismatch { a: u64, b: u64 },
    /// 统计槽序号越界。
    StatSlotRange { slot: usize },
    /// 诊断码/编排自检发现段内违例。
    SegmentViolation { code: u32 },
    /// 编排不变量破坏（仅自检路径，site 为 0x0344_xxxx 定位字）。
    InvariantBroken { site: u32 },
}

impl PerCpuError {
    /// 中文可操作诊断：每变体一句，指明"哪里坏了、先查什么"。
    pub fn detail(&self) -> &'static str {
        match self {
            PerCpuError::MagicMismatch { .. } => {
                "区头 magic 不匹配：先确认实例化走的是 PerCpuArea::init（会写 \
                 PC_MAGIC），再查是否发生越界写/注入后未修复；尾 magic 一并核。"
            }
            PerCpuError::CpuIdMismatch { .. } => {
                "区头 cpu_id 与注册真值不符：核对 init 传入的 cpu_id 与 \
                 CpuIdRegistry 注册项是否同源；若有 cpu_id 注入未修复请先 repair_all。"
            }
            PerCpuError::SelfPtrMismatch { .. } => {
                "self_ptr 读回 != 区基址（F0192 自识别失败）：该区不可作为 GS \
                 切换目标；先 repair_all 修复，再重跑 verify_self_identity 全区扫描。"
            }
            PerCpuError::FooterMismatch { .. } => {
                "尾 magic 被破坏：高度怀疑越界写贯穿区体；隔离该区并审计相邻 \
                 区的栈使用量。"
            }
            PerCpuError::NotRegistered { .. } => {
                "cpu_id/基址对未注册：切换目标必须先经 CpuIdRegistry::register_ap \
                 注册（BSP 走 register_bsp），注册制不允许未登记换防。"
            }
            PerCpuError::DuplicateCpuId { .. } => {
                "重复 cpu_id：同一逻辑 CPU 只能有一个区实例；确认没有对同一 AP \
                 重复实例化，或旧实例未注销。"
            }
            PerCpuError::DuplicateBase { .. } => {
                "重复区基址：一个基址只能挂一个 cpu_id；核查区选址是否发生 \
                 重叠（F0328 实例化分配器账）。"
            }
            PerCpuError::RegistryFull { .. } => {
                "注册表满：调大 new(cap) 容量，或先清理不再使用的注册项。"
            }
            PerCpuError::BspSlotViolation { .. } => {
                "cpu_id=0 是 BSP 固定槽：AP 注册一律从 1 起；register_bsp 只能 \
                 成功一次。"
            }
            PerCpuError::InvalidState { need, got } => {
                let _ = (need, got); // 态码随 Debug 输出核，detail 文案固定
                "状态机非法迁移：按 need→got 的态码对照 SwitchState::name 排查 \
                 调用序；begin/commit/abort/back 必须成对按序。"
            }
            PerCpuError::DoubleSwitchConflict { .. } => {
                "双切换冲突：上一笔 begin 尚未 commit/abort；先收口在途切换，\
                 禁止并发第二根 begin。"
            }
            PerCpuError::LedgerFull { .. } => {
                "事件账满：事件已显性丢弃并计数（dropped）；调大容量或提高 \
                 消费频率。"
            }
            PerCpuError::RecordsFull { .. } => {
                "切换账满：记录已显性丢弃并计数；调大 records_cap。"
            }
            PerCpuError::IsolateLedgerFull { .. } => {
                "隔离账满：隔离项已显性丢弃并计数；坏区数量超出预期，先排查 \
                 系统性损坏而非只扩容。"
            }
            PerCpuError::ReadbackMismatch { .. } => {
                "GS base 写后读回不符：注入面/后端故障或寄存器写丢失；已回滚 \
                 至 BSP，复查 GsBaseBackend 实现。"
            }
            PerCpuError::InjectionLeak { .. } => {
                "注入检出泄漏：注入后验签行为与预期不符（未报错/码错/修复后仍坏）；\
                 验签面与注入面必须同源，禁止旁路写。"
            }
            PerCpuError::BadOffset { .. } => {
                "区内偏移越界：核对布局表 LAYOUT_TABLE 与 OFF_* 常量；字节编解码 \
                 前先做边界检查。"
            }
            PerCpuError::BadAlign { .. } => {
                "对齐违规：区基址/栈基址需 16 字节对齐（x86_64 per-CPU 惯例）。"
            }
            PerCpuError::BadStackConfig => {
                "栈配置非法：栈长须 >0 且 ≤64MiB，栈基+栈长不得回绕。"
            }
            PerCpuError::FingerprintMismatch { .. } => {
                "双跑对拍指纹不一致：同种子必须同指纹；排查编排路径中的非确定性 \
                 （迭代序、未初始化读、容量抖动）。"
            }
            PerCpuError::StatSlotRange { .. } => {
                "统计槽越界：槽号须 < STAT_SLOTS(16)。"
            }
            PerCpuError::SegmentViolation { .. } => {
                "诊断段自检违例：XC_* 常量必须落在 0x1401~0x140F 且两两互异。"
            }
            PerCpuError::InvariantBroken { .. } => {
                "编排不变量破坏：演练预期路径外的结果；对照 site 定位字排查 \
                 状态机/注入面回归。"
            }
        }
    }

    /// 账面诊断码映射：账本类失败落本模块诊断段；编程期 API 误用类
    /// （BadOffset / BadAlign / BadStackConfig / StatSlotRange /
    ///  SegmentViolation / InvariantBroken）映射 0，表示"不落事件账、
    /// 属调用方契约错误"。
    pub fn xcode(&self) -> u32 {
        match self {
            PerCpuError::MagicMismatch { .. } | PerCpuError::FooterMismatch { .. } => {
                XC_MAGIC_MISMATCH
            }
            PerCpuError::CpuIdMismatch { .. } => XC_CPUID_MISMATCH,
            PerCpuError::SelfPtrMismatch { .. } => XC_SELF_PTR_MISMATCH,
            PerCpuError::NotRegistered { .. } => XC_NOT_REGISTERED,
            PerCpuError::DuplicateCpuId { .. } => XC_DUP_CPU_ID,
            PerCpuError::DuplicateBase { .. } => XC_DUP_BASE,
            PerCpuError::RegistryFull { .. } => XC_REGISTRY_FULL,
            PerCpuError::BspSlotViolation { .. } => XC_BSP_SLOT,
            PerCpuError::InvalidState { .. } => XC_STATE_INVALID,
            PerCpuError::DoubleSwitchConflict { .. } => XC_DOUBLE_SWITCH,
            PerCpuError::LedgerFull { .. } | PerCpuError::RecordsFull { .. } => XC_LEDGER_FULL,
            PerCpuError::IsolateLedgerFull { .. } => XC_ISOLATE_FULL,
            PerCpuError::ReadbackMismatch { .. } => XC_READBACK_MISMATCH,
            PerCpuError::InjectionLeak { .. } => XC_INJECT_LEAK,
            PerCpuError::FingerprintMismatch { .. } => XC_FINGERPRINT_MISMATCH,
            PerCpuError::BadOffset { .. }
            | PerCpuError::BadAlign { .. }
            | PerCpuError::BadStackConfig
            | PerCpuError::StatSlotRange { .. }
            | PerCpuError::SegmentViolation { .. }
            | PerCpuError::InvariantBroken { .. } => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// 区布局：const 布局表 + 编译期偏移校验
// ---------------------------------------------------------------------------

/// 区面积：8KiB（2^13）。来源：x86_64 per-CPU 区常见量级（Linux x86_64
/// per-CPU 区通常落在 32KiB 内），取 8KiB 便于成片摆放与 64KiB 上限断言。
pub const PERCPU_AREA_SIZE: usize = 8192;

/// 区面积硬上限：64KiB。来源：任务纪律">64KB 结构体禁上栈"——区结构体
/// 必须在此线内，编译期断言钉死。
pub const PERCPU_AREA_LIMIT: usize = 64 * 1024;

/// 区头 magic：0x5643_5043 = ASCII "VCPC"（Varix CPU Per-Cpu，小端布局下
/// 字节序 'C','P','C','V'）。自造魔数，全域唯一（grep 确认无冲突）。
pub const PC_MAGIC: u32 = 0x5643_5043;

/// 统计槽数量：16 槽（u64 定容，来源：本模块 SLOT_* 闭集共 12 项，留 4 槽
/// 余量给下游 F0345/F0346 对接）。
pub const STAT_SLOTS: usize = 16;

/// 字段布局偏移（区头小端字节序）。
/// 来源：自定布局，遵循"标识在前、自指指针 8 对齐、统计居中、尾部校验收口"。
pub const OFF_MAGIC: usize = 0x00; // u32 magic
pub const OFF_CPU_ID: usize = 0x04; // u32 cpu_id
pub const OFF_SELF_PTR: usize = 0x08; // u64 self_ptr（自指：== 区基址）
pub const OFF_STACK_BASE: usize = 0x10; // u64 栈基址
pub const OFF_STACK_SIZE: usize = 0x18; // u64 栈长
pub const OFF_STATS: usize = 0x20; // 16 x u64 统计槽
pub const OFF_FOOTER: usize = PERCPU_AREA_SIZE - 4; // u32 尾 magic（越界写哨兵）

/// 布局字段标识（布局表与注入面共用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldId {
    /// 区头 magic。
    Magic,
    /// 逻辑 CPU 序号。
    CpuId,
    /// 自指指针（== 区基址）。
    SelfPtr,
    /// 栈基址。
    StackBase,
    /// 栈长。
    StackSize,
    /// 统计槽区段。
    Stats,
    /// 尾 magic。
    Footer,
}

impl FieldId {
    /// 账面字段名（诊断输出用）。
    pub fn name(self) -> &'static str {
        match self {
            FieldId::Magic => "magic",
            FieldId::CpuId => "cpu_id",
            FieldId::SelfPtr => "self_ptr",
            FieldId::StackBase => "stack_base",
            FieldId::StackSize => "stack_size",
            FieldId::Stats => "stats",
            FieldId::Footer => "footer",
        }
    }
}

/// const 布局表：(字段, 偏移, 字节数)。编译期 + 运行期双份校验的基准。
pub const LAYOUT_TABLE: [(FieldId, usize, usize); 7] = [
    (FieldId::Magic, OFF_MAGIC, 4),
    (FieldId::CpuId, OFF_CPU_ID, 4),
    (FieldId::SelfPtr, OFF_SELF_PTR, 8),
    (FieldId::StackBase, OFF_STACK_BASE, 8),
    (FieldId::StackSize, OFF_STACK_SIZE, 8),
    (FieldId::Stats, OFF_STATS, STAT_SLOTS * 8),
    (FieldId::Footer, OFF_FOOTER, 4),
];

/// 编译期布局校验：表内字段严格不重叠、不越界、首字段从 0 起。
/// const fn 内只用 while + 索引 + 原生整数比较（纪律第 3 条）。
const fn layout_table_offsets_ok() -> bool {
    // 首字段必须从 0 开始且恰为 4 字节 magic。
    if LAYOUT_TABLE[0].1 != 0 || LAYOUT_TABLE[0].2 != 4 {
        return false;
    }
    let mut i = 1;
    while i < LAYOUT_TABLE.len() {
        let prev_end = LAYOUT_TABLE[i - 1].1 + LAYOUT_TABLE[i - 1].2;
        if LAYOUT_TABLE[i].1 < prev_end {
            return false; // 重叠
        }
        if LAYOUT_TABLE[i].1 > PERCPU_AREA_SIZE || LAYOUT_TABLE[i].2 > PERCPU_AREA_SIZE {
            return false; // 离谱越界
        }
        if LAYOUT_TABLE[i].1 + LAYOUT_TABLE[i].2 > PERCPU_AREA_SIZE {
            return false; // 末端越界
        }
        i += 1;
    }
    true
}

// 编译期断言组（const assert 不受"运行期 assert 禁令"约束）。
const _: () = assert!(PERCPU_AREA_SIZE <= PERCPU_AREA_LIMIT);
const _: () = assert!(PERCPU_AREA_SIZE.is_power_of_two());
const _: () = assert!(core::mem::size_of::<RawArea>() == PERCPU_AREA_SIZE);
const _: () = assert!(OFF_SELF_PTR == 8);
const _: () = assert!(OFF_FOOTER >= OFF_STATS + STAT_SLOTS * 8);
const _: () = assert!(layout_table_offsets_ok());

/// 字段偏移查询（const fn，match 派发，不比较派生 PartialEq）。
pub const fn field_offset(id: FieldId) -> usize {
    match id {
        FieldId::Magic => OFF_MAGIC,
        FieldId::CpuId => OFF_CPU_ID,
        FieldId::SelfPtr => OFF_SELF_PTR,
        FieldId::StackBase => OFF_STACK_BASE,
        FieldId::StackSize => OFF_STACK_SIZE,
        FieldId::Stats => OFF_STATS,
        FieldId::Footer => OFF_FOOTER,
    }
}

/// 统计槽闭集（12 项 + 4 备用）。来源：本模块三条账线（切换 / 自识别 /
/// 注入）的计数需求清点。
pub const SLOT_SWITCH_BEGIN: usize = 0; // begin 成功次数
pub const SLOT_SWITCH_COMMIT: usize = 1; // commit 成功次数
pub const SLOT_SWITCH_ABORT: usize = 2; // abort 次数
pub const SLOT_SWITCH_FAIL: usize = 3; // commit 失败次数（含回滚）
pub const SLOT_SELF_ID_FAIL: usize = 4; // 自识别失败次数
pub const SLOT_INJECT_ARMED: usize = 5; // 注入布防次数
pub const SLOT_INJECT_CAUGHT: usize = 6; // 注入检出次数
pub const SLOT_INJECT_LEAK: usize = 7; // 注入泄漏次数
pub const SLOT_READBACK_FAIL: usize = 8; // GS 读回失败次数
pub const SLOT_DOUBLE_SWITCH_BLOCK: usize = 9; // 双切换拦截次数
pub const SLOT_VALIDATE_PASS: usize = 10; // 全量验签通过次数
pub const SLOT_VALIDATE_FAIL: usize = 11; // 全量验签失败次数

// ---------------------------------------------------------------------------
// RawArea —— 8KiB 字节级区体（不派生 Debug/Clone，避免 8KiB 打印/拷贝）
// ---------------------------------------------------------------------------

/// 区体：纯字节数组 + 字节级编解码。字段全部经偏移读写，
/// 杜绝 repr 依赖；偏移读写自带边界检查（零 panic 路径）。
pub struct RawArea {
    bytes: [u8; PERCPU_AREA_SIZE],
}

impl RawArea {
    /// 全零区体（const fn）。
    pub const fn zeroed() -> RawArea {
        RawArea {
            bytes: [0u8; PERCPU_AREA_SIZE],
        }
    }

    /// 只读视图（指纹/转储用）。
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    /// 小端 u32 读取（自带边界检查；const fn，手动组包不用 ?）。
    pub const fn u32_at(&self, off: usize) -> Result<u32, PerCpuError> {
        // 前一判已挡住 off 接近 usize::MAX 的回绕（off < 8192 才继续）。
        if off >= PERCPU_AREA_SIZE || off + 4 > PERCPU_AREA_SIZE {
            return Err(PerCpuError::BadOffset { off, size: 4 });
        }
        let v = (self.bytes[off] as u32)
            | ((self.bytes[off + 1] as u32) << 8)
            | ((self.bytes[off + 2] as u32) << 16)
            | ((self.bytes[off + 3] as u32) << 24);
        Ok(v)
    }

    /// 小端 u64 读取（自带边界检查；const fn）。
    pub const fn u64_at(&self, off: usize) -> Result<u64, PerCpuError> {
        if off >= PERCPU_AREA_SIZE || off + 8 > PERCPU_AREA_SIZE {
            return Err(PerCpuError::BadOffset { off, size: 8 });
        }
        let mut v: u64 = 0;
        let mut i = 0;
        while i < 8 {
            v |= (self.bytes[off + i] as u64) << (i * 8);
            i += 1;
        }
        Ok(v)
    }

    /// 小端 u32 写入（自带边界检查；const fn）。
    pub const fn set_u32(&mut self, off: usize, v: u32) -> Result<(), PerCpuError> {
        if off >= PERCPU_AREA_SIZE || off + 4 > PERCPU_AREA_SIZE {
            return Err(PerCpuError::BadOffset { off, size: 4 });
        }
        self.bytes[off] = (v & 0xFF) as u8;
        self.bytes[off + 1] = ((v >> 8) & 0xFF) as u8;
        self.bytes[off + 2] = ((v >> 16) & 0xFF) as u8;
        self.bytes[off + 3] = ((v >> 24) & 0xFF) as u8;
        Ok(())
    }

    /// 小端 u64 写入（自带边界检查；const fn）。
    pub const fn set_u64(&mut self, off: usize, v: u64) -> Result<(), PerCpuError> {
        if off >= PERCPU_AREA_SIZE || off + 8 > PERCPU_AREA_SIZE {
            return Err(PerCpuError::BadOffset { off, size: 8 });
        }
        let mut i = 0;
        while i < 8 {
            self.bytes[off + i] = ((v >> (i * 8)) & 0xFF) as u8;
            i += 1;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// PerCpuArea —— 每 AP 独立实例区（8KiB，远低于 64KiB 栈禁线，仍建议堆持有）
// ---------------------------------------------------------------------------

/// 模型栈长硬上限：64MiB。来源：AP 初始栈量级惯例（x86_64 内核线程栈
/// 通常 16~64KiB；模型上限放宽到 64MiB 防误配），非硬件约束。
pub const MAX_MODEL_STACK: u64 = 64 * 1024 * 1024;

/// 每 AP 独立实例区。`base`/`cpu_id` 为结构体记账真值（ground truth），
/// 区头字段为可被注入/破坏的"读回面"；验签即真值 vs 读回面逐项对账。
pub struct PerCpuArea {
    raw: RawArea,
    base: u64,
    cpu_id: u32,
}

impl PerCpuArea {
    /// 实例化：写入区头全套字段并落尾 magic。
    /// 要求：base / stack_base 16 字节对齐；stack_size ∈ (0, 64MiB]；
    /// stack_base + stack_size 不回绕。
    pub fn init(
        base: u64,
        cpu_id: u32,
        stack_base: u64,
        stack_size: u64,
    ) -> Result<PerCpuArea, PerCpuError> {
        if base % 16 != 0 {
            return Err(PerCpuError::BadAlign { value: base, align: 16 });
        }
        if stack_base % 16 != 0 {
            return Err(PerCpuError::BadAlign { value: stack_base, align: 16 });
        }
        if stack_size == 0 || stack_size > MAX_MODEL_STACK {
            return Err(PerCpuError::BadStackConfig);
        }
        if stack_base.checked_add(stack_size).is_none() {
            return Err(PerCpuError::BadStackConfig);
        }
        let mut area = PerCpuArea {
            raw: RawArea::zeroed(),
            base,
            cpu_id,
        };
        area.raw.set_u32(OFF_MAGIC, PC_MAGIC)?;
        area.raw.set_u32(OFF_CPU_ID, cpu_id)?;
        // self_ptr 自指（F0192 语义的存储侧）：写死 == 区基址。
        area.raw.set_u64(OFF_SELF_PTR, base)?;
        area.raw.set_u64(OFF_STACK_BASE, stack_base)?;
        area.raw.set_u64(OFF_STACK_SIZE, stack_size)?;
        // 统计槽已在 zeroed() 中清零，无需逐槽写。
        area.raw.set_u32(OFF_FOOTER, PC_MAGIC)?;
        Ok(area)
    }

    /// 区基址（结构体记账真值）。
    pub fn base(&self) -> u64 {
        self.base
    }

    /// 逻辑 CPU 序号（结构体记账真值）。
    pub fn ground_cpu_id(&self) -> u32 {
        self.cpu_id
    }

    /// 区头 magic 读回。
    pub fn magic(&self) -> Result<u32, PerCpuError> {
        self.raw.u32_at(OFF_MAGIC)
    }

    /// 区头 cpu_id 读回（可被注入破坏的"读回面"）。
    pub fn cpu_id(&self) -> Result<u32, PerCpuError> {
        self.raw.u32_at(OFF_CPU_ID)
    }

    /// self_ptr 读回（自指指针，可被注入破坏）。
    pub fn self_ptr(&self) -> Result<u64, PerCpuError> {
        self.raw.u64_at(OFF_SELF_PTR)
    }

    /// 栈基址读回。
    pub fn stack_base(&self) -> Result<u64, PerCpuError> {
        self.raw.u64_at(OFF_STACK_BASE)
    }

    /// 栈长读回。
    pub fn stack_size(&self) -> Result<u64, PerCpuError> {
        self.raw.u64_at(OFF_STACK_SIZE)
    }

    /// 尾 magic 读回（越界写哨兵）。
    pub fn footer_magic(&self) -> Result<u32, PerCpuError> {
        self.raw.u32_at(OFF_FOOTER)
    }

    /// 统计槽读。
    pub fn stats_slot(&self, slot: usize) -> Result<u64, PerCpuError> {
        if slot >= STAT_SLOTS {
            return Err(PerCpuError::StatSlotRange { slot });
        }
        self.raw.u64_at(OFF_STATS + slot * 8)
    }

    /// 统计槽写。
    pub fn set_stats_slot(&mut self, slot: usize, value: u64) -> Result<(), PerCpuError> {
        if slot >= STAT_SLOTS {
            return Err(PerCpuError::StatSlotRange { slot });
        }
        self.raw.set_u64(OFF_STATS + slot * 8, value)
    }

    /// 统计槽饱和自增（saturating +1，显性防回绕），返回新值。
    pub fn bump_stats_slot(&mut self, slot: usize) -> Result<u64, PerCpuError> {
        let cur = self.stats_slot(slot)?;
        let nv = cur.saturating_add(1);
        self.raw.set_u64(OFF_STATS + slot * 8, nv)?;
        Ok(nv)
    }

    /// 三自洽验签（切换前验签闭集）：magic / cpu_id / self_ptr 逐项对账。
    /// 任一不自洽即拒切——调用方（GsBaseSwitcher::begin_switch）负责落账。
    pub fn three_way_consistency(&self) -> Result<(), PerCpuError> {
        let m = self.raw.u32_at(OFF_MAGIC)?;
        if m != PC_MAGIC {
            return Err(PerCpuError::MagicMismatch { expected: PC_MAGIC, found: m });
        }
        let c = self.raw.u32_at(OFF_CPU_ID)?;
        if c != self.cpu_id {
            return Err(PerCpuError::CpuIdMismatch { expected: self.cpu_id, found: c });
        }
        self.self_identify()
    }

    /// 自识别（F0192 语义复用）：self_ptr 读回 == 实例基址。
    /// 这是 GS base 落靶后的"认得出自己"判定，也是全区扫描的单元谓词。
    pub fn self_identify(&self) -> Result<(), PerCpuError> {
        let sp = self.raw.u64_at(OFF_SELF_PTR)?;
        if sp != self.base {
            return Err(PerCpuError::SelfPtrMismatch { base: self.base, found: sp });
        }
        Ok(())
    }

    /// 全量验签：三自洽 + 尾 magic（越界写哨兵）。统计槽不参与验签
    /// （计数属于账面而非区体身份）。
    pub fn validate(&self) -> Result<(), PerCpuError> {
        self.three_way_consistency()?;
        let f = self.raw.u32_at(OFF_FOOTER)?;
        if f != PC_MAGIC {
            return Err(PerCpuError::FooterMismatch { expected: PC_MAGIC, found: f });
        }
        Ok(())
    }

    /// 注入布防：按类别破坏区头读回面（注入面真实现，供检出演练）。
    pub fn inject(&mut self, kind: InjectionKind) -> Result<(), PerCpuError> {
        match kind {
            InjectionKind::Magic => {
                let m = self.raw.u32_at(OFF_MAGIC)?;
                self.raw.set_u32(OFF_MAGIC, m ^ 0xFFFF_FFFF)
            }
            InjectionKind::CpuId => {
                let c = self.raw.u32_at(OFF_CPU_ID)?;
                self.raw.set_u32(OFF_CPU_ID, c.wrapping_add(1))
            }
            // 单比特翻转：模拟最隐蔽的基址错位（基址 16 对齐 ⇒ 翻转低位必偏离）。
            InjectionKind::SelfPtr => {
                let s = self.raw.u64_at(OFF_SELF_PTR)?;
                self.raw.set_u64(OFF_SELF_PTR, s ^ 0x1)
            }
        }
    }

    /// 修复回归：从结构体记账真值重写全部身份字段（栈字段无注入路径，
    /// 不在修复面内）。
    pub fn repair_all(&mut self) -> Result<(), PerCpuError> {
        self.raw.set_u32(OFF_MAGIC, PC_MAGIC)?;
        self.raw.set_u32(OFF_CPU_ID, self.cpu_id)?;
        self.raw.set_u64(OFF_SELF_PTR, self.base)?;
        self.raw.set_u32(OFF_FOOTER, PC_MAGIC)?;
        Ok(())
    }

    /// 区体全字节 FNV-1a 64 指纹（8KiB 全扫，转储对拍用，热路径勿用）。
    pub fn raw_fingerprint(&self) -> u64 {
        fnv1a64_bytes(fnv1a64_init(), self.raw.as_slice())
    }

    /// 身份 + 统计 FNV-1a 64 指纹（编排对拍用：真值 + 12 槽计数全参与）。
    pub fn stats_fingerprint(&self) -> u64 {
        let mut h = fnv1a64_init();
        h = fnv1a64_u64(h, self.base);
        h = fnv1a64_u64(h, self.cpu_id as u64);
        let mut i = 0;
        while i < STAT_SLOTS {
            // 备用槽恒 0 也参与指纹——布局稳定是对拍前提。
            h = fnv1a64_u64(h, self.stats_slot(i).unwrap_or(0));
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// FNV-1a 64 —— 指纹基元（事件账 / 切换账 / 注册表 / 双跑对拍共用）
// ---------------------------------------------------------------------------

/// FNV-1a 64 位偏移基。来源：Landon Curt Noll 公布的 FNV-1a 64 官方参数
/// （offset basis 0xcbf29ce484222325）。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;
/// FNV-1a 64 位素数。来源：同上（prime 0x100000001b3）。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 指纹起点（= 空输入指纹，可测已知向量）。
pub fn fnv1a64_init() -> u64 {
    FNV_OFFSET
}

/// 单字节推进一步。
pub fn fnv1a64_step(h: u64, byte: u8) -> u64 {
    (h ^ (byte as u64)).wrapping_mul(FNV_PRIME)
}

/// 字节串推进（while + 索引，零迭代器开销路径）。
pub fn fnv1a64_bytes(mut h: u64, bytes: &[u8]) -> u64 {
    let mut i = 0;
    while i < bytes.len() {
        h = fnv1a64_step(h, bytes[i]);
        i += 1;
    }
    h
}

/// u64 值推进（小端 8 字节逐字节）。
pub fn fnv1a64_u64(h: u64, v: u64) -> u64 {
    let mut acc = h;
    let mut i = 0;
    while i < 8 {
        acc = fnv1a64_step(acc, ((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    acc
}

// ---------------------------------------------------------------------------
// EventLedger —— 事件账（定容 + 显性丢弃计数 + FNV 指纹）
// ---------------------------------------------------------------------------

/// 事件类别闭集（事件账的账页类型；code 字段挂诊断码，信息事件挂 0）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventKind {
    /// begin_switch 受理。
    SwitchBegin,
    /// commit_switch 成功落座。
    SwitchCommit,
    /// abort_switch 主动弃切。
    SwitchAbort,
    /// commit/abort 失败（验签/读回/状态机拒绝）。
    SwitchFail,
    /// begin 被状态机拦截（双切换 / 非 BspResident）。
    SwitchBlocked,
    /// 返回 BSP（AP 换防收口）。
    BackToBsp,
    /// 注册表登记。
    Register,
    /// 自识别失败（扫描期）。
    SelfIdFail,
    /// 坏区进隔离账。
    Isolated,
    /// 注入布防。
    InjectionArmed,
    /// 注入检出。
    InjectionCaught,
    /// 注入泄漏（检测面失明，最严重）。
    InjectionLeak,
    /// GS 读回失败。
    ReadbackFail,
    /// 体检执行。
    HealthCheck,
}

/// 单条事件：序号（回绕）+ 类别 + 诊断码 + cpu_id + 附加量。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedgerEvent {
    /// 全账单调序号（wrapping 回绕，跨容量复用）。
    pub seq: u32,
    /// 事件类别。
    pub kind: EventKind,
    /// 诊断码（0 = 信息事件，非 0 必须落 0x1401~0x140F）。
    pub code: u32,
    /// 关联逻辑 CPU。
    pub cpu_id: u32,
    /// 附加量（通常为区基址或计数快照）。
    pub aux: u64,
}

/// 事件账默认容量。来源：单轮编排事件峰值（8 AP × ~6 条 + 扫描/注入演练
/// ~20 条）的 2 倍余量取整。
pub const LEDGER_DEFAULT_CAP: usize = 256;

/// 定容事件账：满账不再入账、dropped 饱和计数、全账可指纹化。
pub struct EventLedger {
    events: Vec<LedgerEvent>,
    cap: usize,
    total_seen: u32,
    dropped: u32,
}

impl EventLedger {
    /// 建账（cap 至少 1，0 视为 1 防零容量死账）。
    pub fn new(cap: usize) -> EventLedger {
        EventLedger {
            events: Vec::new(),
            cap: if cap == 0 { 1 } else { cap },
            total_seen: 0,
            dropped: 0,
        }
    }

    /// 入账：序号 wrapping 单调；满账显性丢弃并饱和计数，返回 Err 供
    /// 调用方知悉（账内状态无论如何都已正确维护）。
    pub fn push(
        &mut self,
        kind: EventKind,
        code: u32,
        cpu_id: u32,
        aux: u64,
    ) -> Result<(), PerCpuError> {
        self.total_seen = self.total_seen.wrapping_add(1);
        let seq = self.total_seen;
        if self.events.len() >= self.cap {
            self.dropped = self.dropped.saturating_add(1);
            return Err(PerCpuError::LedgerFull { cap: self.cap });
        }
        self.events.push(LedgerEvent { seq, kind, code, cpu_id, aux });
        Ok(())
    }

    /// 在账事件数。
    pub fn kept_count(&self) -> usize {
        self.events.len()
    }

    /// 累计入账请求数（含被丢弃的；wrapping 回绕）。
    pub fn event_count(&self) -> u32 {
        self.total_seen
    }

    /// 显性丢弃计数。
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    /// 在账事件只读视图。
    pub fn events(&self) -> &[LedgerEvent] {
        &self.events
    }

    /// 全账 FNV-1a 64 指纹（含 dropped / total_seen 元数据）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_init();
        let mut i = 0;
        while i < self.events.len() {
            let e = &self.events[i];
            h = fnv1a64_u64(h, e.seq as u64);
            h = fnv1a64_u64(h, (e.code as u64) << 32 | (e.cpu_id as u64));
            h = fnv1a64_u64(h, e.aux);
            // 类别以判别序号参与指纹：闭集序号稳定，指纹跨跑可对拍。
            h = fnv1a64_u64(h, event_kind_ordinal(e.kind) as u64);
            i += 1;
        }
        h = fnv1a64_u64(h, self.dropped as u64);
        h = fnv1a64_u64(h, self.total_seen as u64);
        h
    }
}

/// 事件类别稳定序号（账面落账 / 指纹用，不随枚举声明顺序漂移——
/// 声明顺序即本函数的返回序，双向钉死）。
pub fn event_kind_ordinal(kind: EventKind) -> u8 {
    match kind {
        EventKind::SwitchBegin => 0,
        EventKind::SwitchCommit => 1,
        EventKind::SwitchAbort => 2,
        EventKind::SwitchFail => 3,
        EventKind::SwitchBlocked => 4,
        EventKind::BackToBsp => 5,
        EventKind::Register => 6,
        EventKind::SelfIdFail => 7,
        EventKind::Isolated => 8,
        EventKind::InjectionArmed => 9,
        EventKind::InjectionCaught => 10,
        EventKind::InjectionLeak => 11,
        EventKind::ReadbackFail => 12,
        EventKind::HealthCheck => 13,
    }
}

// ---------------------------------------------------------------------------
// CpuIdRegistry —— cpu_id → 区基址映射表（注册制 + BSP 固定槽 0）
// ---------------------------------------------------------------------------

/// 注册表默认容量。来源：64 逻辑 CPU 上限（x86_64 APIC id 常见桌面/服务器
/// 量级内取整，BSP 槽 0 占 1 席，AP 至多 63）。
pub const REGISTRY_DEFAULT_CAP: usize = 64;

/// 注册项：cpu_id + 区基址 + BSP 标记。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegistryEntry {
    /// 逻辑 CPU 序号（0 恒为 BSP）。
    pub cpu_id: u32,
    /// per-CPU 区基址。
    pub base: u64,
    /// 是否 BSP。
    pub is_bsp: bool,
}

/// cpu_id → 区基址映射表。
/// 规约：注册制（未注册不许换防）；重复 cpu_id 拒收；重复基址拒收；
/// cpu_id=0 为 BSP 固定槽，只能由 register_bsp 占用且只能占用一次。
pub struct CpuIdRegistry {
    entries: Vec<RegistryEntry>,
    cap: usize,
    reject_dup_cpu: u32,
    reject_dup_base: u32,
    reject_bsp_slot: u32,
    reject_full: u32,
    reject_align: u32,
}

impl CpuIdRegistry {
    /// 建表（cap 至少 1）。
    pub fn new(cap: usize) -> CpuIdRegistry {
        CpuIdRegistry {
            entries: Vec::new(),
            cap: if cap == 0 { 1 } else { cap },
            reject_dup_cpu: 0,
            reject_dup_base: 0,
            reject_bsp_slot: 0,
            reject_full: 0,
            reject_align: 0,
        }
    }

    /// BSP 登记：cpu_id=0 固定槽，仅允许成功一次；基址 16 对齐。
    pub fn register_bsp(&mut self, base: u64) -> Result<(), PerCpuError> {
        if base % 16 != 0 {
            self.reject_align = self.reject_align.saturating_add(1);
            return Err(PerCpuError::BadAlign { value: base, align: 16 });
        }
        if self.lookup(0).is_some() {
            self.reject_dup_cpu = self.reject_dup_cpu.saturating_add(1);
            return Err(PerCpuError::DuplicateCpuId { cpu_id: 0 });
        }
        if self.entries.len() >= self.cap {
            self.reject_full = self.reject_full.saturating_add(1);
            return Err(PerCpuError::RegistryFull { cap: self.cap });
        }
        self.entries.push(RegistryEntry { cpu_id: 0, base, is_bsp: true });
        Ok(())
    }

    /// AP 登记：cpu_id ∈ [1, u32::MAX]，cpu/基址均不得重复。
    pub fn register_ap(&mut self, cpu_id: u32, base: u64) -> Result<(), PerCpuError> {
        if cpu_id == 0 {
            self.reject_bsp_slot = self.reject_bsp_slot.saturating_add(1);
            return Err(PerCpuError::BspSlotViolation { attempted_cpu: cpu_id });
        }
        if base % 16 != 0 {
            self.reject_align = self.reject_align.saturating_add(1);
            return Err(PerCpuError::BadAlign { value: base, align: 16 });
        }
        if self.lookup(cpu_id).is_some() {
            self.reject_dup_cpu = self.reject_dup_cpu.saturating_add(1);
            return Err(PerCpuError::DuplicateCpuId { cpu_id });
        }
        if self.owner_of(base).is_some() {
            self.reject_dup_base = self.reject_dup_base.saturating_add(1);
            return Err(PerCpuError::DuplicateBase { base });
        }
        if self.entries.len() >= self.cap {
            self.reject_full = self.reject_full.saturating_add(1);
            return Err(PerCpuError::RegistryFull { cap: self.cap });
        }
        self.entries.push(RegistryEntry { cpu_id, base, is_bsp: false });
        Ok(())
    }

    /// 按 cpu_id 查区基址（线性扫，容量 ≤64 常数级）。
    pub fn lookup(&self, cpu_id: u32) -> Option<u64> {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].cpu_id == cpu_id {
                return Some(self.entries[i].base);
            }
            i += 1;
        }
        None
    }

    /// 按基址反查归属 cpu_id（一址一席的唯一性检查用）。
    pub fn owner_of(&self, base: u64) -> Option<u32> {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].base == base {
                return Some(self.entries[i].cpu_id);
            }
            i += 1;
        }
        None
    }

    /// cpu_id/基址对是否双双登记一致（切换前验签的注册制一环）。
    pub fn pair_registered(&self, cpu_id: u32, base: u64) -> bool {
        match self.lookup(cpu_id) {
            Some(b) => b == base,
            None => false,
        }
    }

    /// BSP 基址（未登记 BSP 时 None）。
    pub fn bsp_base(&self) -> Option<u64> {
        self.lookup(0)
    }

    /// BSP 是否已登记。
    pub fn bsp_registered(&self) -> bool {
        self.lookup(0).is_some()
    }

    /// 登记席数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 各类拒收计数快照（体检/对拍用）。
    pub fn reject_counts(&self) -> (u32, u32, u32, u32, u32) {
        (
            self.reject_dup_cpu,
            self.reject_dup_base,
            self.reject_bsp_slot,
            self.reject_full,
            self.reject_align,
        )
    }

    /// 只读登记视图。
    pub fn entries(&self) -> &[RegistryEntry] {
        &self.entries
    }

    /// 登记表 FNV-1a 64 指纹（对拍用：cpu/base/bsp 三元组逐项入指纹）。
    pub fn entries_fingerprint(&self) -> u64 {
        let mut h = fnv1a64_init();
        let mut i = 0;
        while i < self.entries.len() {
            h = fnv1a64_u64(h, self.entries[i].cpu_id as u64);
            h = fnv1a64_u64(h, self.entries[i].base);
            h = fnv1a64_u64(h, self.entries[i].is_bsp as u64);
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 切换状态机 —— BspResident → Switching → ApResident（BSP/AP 互斥）
// ---------------------------------------------------------------------------

/// 切换状态三态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwitchState {
    /// GS 仍在 BSP 区（换防起点/归宿）。
    BspResident,
    /// 在途切换（begin 已受理、commit 未收口；此态下第二根 begin 必拦截）。
    Switching,
    /// GS 已落 AP 区（resident 独占；再切须先返回 BSP）。
    ApResident,
}

impl SwitchState {
    /// 稳定态码（InvalidState 账面用）。
    pub fn code(self) -> u8 {
        match self {
            SwitchState::BspResident => 0,
            SwitchState::Switching => 1,
            SwitchState::ApResident => 2,
        }
    }

    /// 态名（诊断输出用）。
    pub fn name(self) -> &'static str {
        match self {
            SwitchState::BspResident => "BspResident",
            SwitchState::Switching => "Switching",
            SwitchState::ApResident => "ApResident",
        }
    }
}

/// 切换账页阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwitchPhase {
    /// begin 受理。
    Begin,
    /// commit 收口（含失败回滚记录）。
    Commit,
    /// abort 弃切。
    Abort,
    /// 返回 BSP。
    BackToBsp,
}

/// 切换账单页：一笔切换动作的完整快照（成功与失败都留账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SwitchRecord {
    /// 账内单调序号（wrapping 回绕）。
    pub seq: u32,
    /// 阶段。
    pub phase: SwitchPhase,
    /// 本页动作最终是否成功。
    pub ok: bool,
    /// 目标逻辑 CPU。
    pub target_cpu: u32,
    /// 目标区基址。
    pub target_base: u64,
    /// 动作前状态。
    pub prev_state: SwitchState,
    /// 动作后状态。
    pub new_state: SwitchState,
    /// GS base 读回值（未触后端时为当时现值）。
    pub gs_readback: u64,
    /// 诊断码（成功恒 0）。
    pub xcode: u32,
}

/// 切换器计数快照（体检/对拍用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SwitchCounters {
    /// begin 受理数。
    pub begun: u32,
    /// commit 成功数。
    pub committed: u32,
    /// abort 数。
    pub aborted: u32,
    /// commit 失败数（含回滚）。
    pub failed: u32,
    /// begin 被拦截数（双切换 / 互斥）。
    pub blocked: u32,
    /// GS 读回失败数。
    pub readback_fails: u32,
    /// 回滚写 GS 成功恢复 BSP 次数。
    pub rollback_writes: u32,
    /// 返回 BSP 次数。
    pub back_to_bsp: u32,
}

// ---------------------------------------------------------------------------
// GsBaseBackend —— GS base 写入面 trait 注入（抽象建模，不写真实 wrmsr）
// ---------------------------------------------------------------------------

/// GS base 后端注入面：真实硬件 wrmsr(0xC0000101) 在上层适配本 trait；
/// 本模块用 RamGsBackend（纯内存影子寄存器）做语义模型与故障演练。
pub trait GsBaseBackend {
    /// 写 GS base（硬件语义：影子寄存器置值）。
    fn write(&mut self, value: u64) -> Result<(), PerCpuError>;
    /// 读 GS base（硬件语义：影子寄存器现值）。
    fn read(&self) -> u64;
    /// 后端名（账面/诊断）。
    fn name(&self) -> &'static str;
}

/// 内存影子 GS 后端：可布防一次写读错位故障（写入值低位翻转，
/// 模拟寄存器写丢失/串扰），供读回验签演练。
pub struct RamGsBackend {
    current: u64,
    writes: u64,
    fault_next: bool,
}

impl RamGsBackend {
    /// 建后端，初值通常为 BSP 区基址。
    pub fn new(initial: u64) -> RamGsBackend {
        RamGsBackend { current: initial, writes: 0, fault_next: false }
    }

    /// 布防：下一笔写值被低位翻转（错位故障一次一发）。
    pub fn arm_fault(&mut self) {
        self.fault_next = true;
    }

    /// 撤防。
    pub fn disarm_fault(&mut self) {
        self.fault_next = false;
    }

    /// 累计写次数（wrapping）。
    pub fn write_count(&self) -> u64 {
        self.writes
    }
}

impl GsBaseBackend for RamGsBackend {
    fn write(&mut self, value: u64) -> Result<(), PerCpuError> {
        self.writes = self.writes.wrapping_add(1);
        if self.fault_next {
            self.fault_next = false;
            self.current = value ^ 0x1; // 单比特错位，模拟硬件写丢失
        } else {
            self.current = value;
        }
        Ok(())
    }

    fn read(&self) -> u64 {
        self.current
    }

    fn name(&self) -> &'static str {
        "ram-gs-shadow-v1"
    }
}

// ---------------------------------------------------------------------------
// GsBaseSwitcher —— GS base 切换语义模型（两段式 + 验签 + 自识别 + 切换账）
// ---------------------------------------------------------------------------

/// 切换账默认容量。来源：编排单轮峰值（8 AP × 每.AP 最多 3 页：失败页 +
/// 重试页 + 成功页 + back 页）取整。
pub const RECORDS_DEFAULT_CAP: usize = 64;

/// 在途切换目标（Switching 态下的待决登记）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PendingTarget {
    /// 目标逻辑 CPU。
    pub cpu_id: u32,
    /// 目标区基址。
    pub base: u64,
}

/// GS base 切换器：换防全程的语义闸口。
///
/// 生命周期（单 AP）：begin_switch（预验签 + 受理）→ commit_switch
/// （写 GS → 读回 → 自识别 → 全量验签 → 落座）；任一步失败即回滚
/// GS 至 BSP 并留失败账页。BSP/AP 互斥：ApResident 下必须
/// switch_back_to_bsp 才能开下一笔；Switching 下第二根 begin 必拦截。
pub struct GsBaseSwitcher<B: GsBaseBackend> {
    backend: B,
    bsp_base: u64,
    state: SwitchState,
    pending: Option<PendingTarget>,
    resident_cpu: Option<u32>,
    records: Vec<SwitchRecord>,
    records_cap: usize,
    record_seq: u32,
    records_dropped: u32,
    counters: SwitchCounters,
    ledger: EventLedger,
}

impl<B: GsBaseBackend> GsBaseSwitcher<B> {
    /// 建切换器：bsp_base 为 BSP 区基址（回滚归宿），records_cap 为
    /// 切换账容量（至少 1）。
    pub fn new(backend: B, bsp_base: u64, records_cap: usize) -> GsBaseSwitcher<B> {
        GsBaseSwitcher {
            backend,
            bsp_base,
            state: SwitchState::BspResident,
            pending: None,
            resident_cpu: None,
            records: Vec::new(),
            records_cap: if records_cap == 0 { 1 } else { records_cap },
            record_seq: 0,
            records_dropped: 0,
            counters: SwitchCounters {
                begun: 0,
                committed: 0,
                aborted: 0,
                failed: 0,
                blocked: 0,
                readback_fails: 0,
                rollback_writes: 0,
                back_to_bsp: 0,
            },
            ledger: EventLedger::new(LEDGER_DEFAULT_CAP),
        }
    }

    /// 当前状态。
    pub fn state(&self) -> SwitchState {
        self.state
    }

    /// 当前驻留 AP（ApResident 时 Some，否则 None）。
    pub fn resident_cpu(&self) -> Option<u32> {
        self.resident_cpu
    }

    /// 在途切换目标（Switching 时 Some）。
    pub fn pending_target(&self) -> Option<PendingTarget> {
        self.pending
    }

    /// 切换账只读视图。
    pub fn records(&self) -> &[SwitchRecord] {
        &self.records
    }

    /// 切换账显性丢弃计数。
    pub fn records_dropped(&self) -> u32 {
        self.records_dropped
    }

    /// 计数快照。
    pub fn counters(&self) -> SwitchCounters {
        self.counters
    }

    /// 内嵌事件账只读视图。
    pub fn ledger(&self) -> &EventLedger {
        &self.ledger
    }

    /// 后端只读视图（回滚核账用）。
    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// 后端可变视图（故障布防用）。
    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    /// 切换账 FNV 指纹（对拍用）。
    pub fn records_fingerprint(&self) -> u64 {
        let mut h = fnv1a64_init();
        let mut i = 0;
        while i < self.records.len() {
            let r = &self.records[i];
            h = fnv1a64_u64(h, r.seq as u64);
            h = fnv1a64_u64(h, r.target_cpu as u64);
            h = fnv1a64_u64(h, r.target_base);
            h = fnv1a64_u64(h, r.gs_readback);
            h = fnv1a64_u64(
                h,
                (r.xcode as u64) << 32
                    | (switch_phase_ordinal(r.phase) as u64) << 16
                    | (r.ok as u64) << 8
                    | (r.new_state.code() as u64) << 4
                    | (r.prev_state.code() as u64),
            );
            i += 1;
        }
        h = fnv1a64_u64(h, self.records_dropped as u64);
        h = fnv1a64_u64(h, self.record_seq as u64);
        h
    }

    /// 序号推进（wrapping 单调）。
    fn next_record_seq(&mut self) -> u32 {
        self.record_seq = self.record_seq.wrapping_add(1);
        self.record_seq
    }

    /// 切换账入账（满账显性丢弃并计数，Err 不阻断主流程——账满可容忍，
    /// 账丢不可静默）。
    fn push_record(&mut self, rec: SwitchRecord) {
        if self.records.len() >= self.records_cap {
            self.records_dropped = self.records_dropped.saturating_add(1);
            return;
        }
        self.records.push(rec);
    }

    /// 失败账页组装 + 入账 + 事件落账（失败路径公共尾）。
    fn fail_record(
        &mut self,
        phase: SwitchPhase,
        target_cpu: u32,
        target_base: u64,
        prev: SwitchState,
        new_state: SwitchState,
        gs_readback: u64,
        xcode: u32,
    ) {
        let rec = SwitchRecord {
            seq: self.next_record_seq(),
            phase,
            ok: false,
            target_cpu,
            target_base,
            prev_state: prev,
            new_state,
            gs_readback,
            xcode,
        };
        self.push_record(rec);
    }

    /// 回滚：GS 尽力写回 BSP；写回后读回核账，成功恢复则计
    /// rollback_writes（失败只留读回失败计数，不再连锁报错）。
    fn rollback_to_bsp_best_effort(&mut self) {
        let _ = self.backend.write(self.bsp_base);
        if self.backend.read() == self.bsp_base {
            self.counters.rollback_writes = self.counters.rollback_writes.saturating_add(1);
        } else {
            self.counters.readback_fails = self.counters.readback_fails.saturating_add(1);
        }
        self.state = SwitchState::BspResident;
        self.pending = None;
    }

    /// 开切（受理段）：三道闸——
    ///   闸 1：Switching 态下再 begin = 双切换冲突，必拦截；
    ///   闸 2：ApResident 态下再 begin = BSP/AP 互斥违例，必拦截
    ///         （必须先 switch_back_to_bsp）；
    ///   闸 3：注册制 + 目标区三自洽验签（magic / cpu_id / self_ptr），
    ///         不自洽拒切。
    /// 全过则置 Switching 态、登记待决目标、落 Begin 账页。
    pub fn begin_switch(
        &mut self,
        registry: &CpuIdRegistry,
        area: &mut PerCpuArea,
    ) -> Result<SwitchRecord, PerCpuError> {
        let prev = self.state;
        let header_cpu = area.cpu_id().unwrap_or(0);
        let base = area.base();

        // 闸 1：双切换冲突拦截。
        if prev == SwitchState::Switching {
            self.counters.blocked = self.counters.blocked.saturating_add(1);
            let pending_cpu = self.pending.map(|p| p.cpu_id).unwrap_or(0);
            let _ = self.ledger.push(
                EventKind::SwitchBlocked,
                XC_DOUBLE_SWITCH,
                header_cpu,
                base,
            );
            self.fail_record(
                SwitchPhase::Begin,
                header_cpu,
                base,
                prev,
                prev,
                self.backend.read(),
                XC_DOUBLE_SWITCH,
            );
            return Err(PerCpuError::DoubleSwitchConflict { pending_cpu });
        }

        // 闸 2：BSP/AP 互斥。
        if prev == SwitchState::ApResident {
            self.counters.blocked = self.counters.blocked.saturating_add(1);
            let _ = self.ledger.push(
                EventKind::SwitchBlocked,
                XC_STATE_INVALID,
                header_cpu,
                base,
            );
            self.fail_record(
                SwitchPhase::Begin,
                header_cpu,
                base,
                prev,
                prev,
                self.backend.read(),
                XC_STATE_INVALID,
            );
            return Err(PerCpuError::InvalidState {
                need: SwitchState::BspResident.code(),
                got: prev.code(),
            });
        }

        // 闸 3a：注册制——cpu_id/基址对必须双登记一致。
        if !registry.pair_registered(header_cpu, base) {
            let _ = self.ledger.push(
                EventKind::SwitchFail,
                XC_NOT_REGISTERED,
                header_cpu,
                base,
            );
            self.fail_record(
                SwitchPhase::Begin,
                header_cpu,
                base,
                prev,
                prev,
                self.backend.read(),
                XC_NOT_REGISTERED,
            );
            return Err(PerCpuError::NotRegistered { cpu_id: header_cpu, base });
        }

        // 闸 3b：目标区三自洽验签（不自洽拒切）。
        if let Err(e) = area.three_way_consistency() {
            let _ = self.ledger.push(EventKind::SwitchFail, e.xcode(), header_cpu, base);
            self.fail_record(
                SwitchPhase::Begin,
                header_cpu,
                base,
                prev,
                prev,
                self.backend.read(),
                e.xcode(),
            );
            self.counters.failed = self.counters.failed.saturating_add(1);
            let _ = area.bump_stats_slot(SLOT_SWITCH_FAIL);
            return Err(e);
        }

        // 全过：受理，置在途态。
        self.state = SwitchState::Switching;
        self.pending = Some(PendingTarget { cpu_id: header_cpu, base });
        self.counters.begun = self.counters.begun.saturating_add(1);
        let _ = area.bump_stats_slot(SLOT_SWITCH_BEGIN);
        let _ = self.ledger.push(EventKind::SwitchBegin, 0, header_cpu, base);
        let rec = SwitchRecord {
            seq: self.next_record_seq(),
            phase: SwitchPhase::Begin,
            ok: true,
            target_cpu: header_cpu,
            target_base: base,
            prev_state: prev,
            new_state: SwitchState::Switching,
            gs_readback: self.backend.read(),
            xcode: 0,
        };
        self.push_record(rec);
        Ok(rec)
    }

    /// 收口（提交段）：写 GS → 读回核账 → 自识别（F0192）→ 全量验签 →
    /// 落座 ApResident。任一步失败即回滚 GS 至 BSP 并留失败账页。
    pub fn commit_switch(&mut self, area: &mut PerCpuArea) -> Result<SwitchRecord, PerCpuError> {
        let prev = self.state;
        let pend = match self.pending {
            Some(p) => p,
            None => {
                return Err(PerCpuError::InvalidState {
                    need: SwitchState::Switching.code(),
                    got: prev.code(),
                });
            }
        };
        let header_cpu = area.cpu_id().unwrap_or(pend.cpu_id);
        let base = area.base();

        // 状态机 + 待决目标一致性（目标不许在 begin 与 commit 之间偷换）。
        if prev != SwitchState::Switching || pend.cpu_id != header_cpu || pend.base != base {
            let _ = self
                .ledger
                .push(EventKind::SwitchFail, XC_STATE_INVALID, header_cpu, base);
            self.fail_record(
                SwitchPhase::Commit,
                header_cpu,
                base,
                prev,
                prev,
                self.backend.read(),
                XC_STATE_INVALID,
            );
            return Err(PerCpuError::InvalidState {
                need: SwitchState::Switching.code(),
                got: prev.code(),
            });
        }

        // 写 GS base（注入面 trait，真实硬件在上层适配）。
        if let Err(e) = self.backend.write(base) {
            self.rollback_to_bsp_best_effort();
            self.counters.failed = self.counters.failed.saturating_add(1);
            let _ = area.bump_stats_slot(SLOT_SWITCH_FAIL);
            let _ = self.ledger.push(EventKind::SwitchFail, e.xcode(), header_cpu, base);
            self.fail_record(
                SwitchPhase::Commit,
                header_cpu,
                base,
                prev,
                self.state,
                self.backend.read(),
                e.xcode(),
            );
            return Err(e);
        }

        // 写后读回（落靶第一道自证）。
        let rb = self.backend.read();
        if rb != base {
            self.rollback_to_bsp_best_effort();
            self.counters.readback_fails = self.counters.readback_fails.saturating_add(1);
            self.counters.failed = self.counters.failed.saturating_add(1);
            let _ = area.bump_stats_slot(SLOT_READBACK_FAIL);
            let _ = area.bump_stats_slot(SLOT_SWITCH_FAIL);
            let _ = self.ledger.push(
                EventKind::ReadbackFail,
                XC_READBACK_MISMATCH,
                header_cpu,
                base,
            );
            self.fail_record(
                SwitchPhase::Commit,
                header_cpu,
                base,
                prev,
                self.state,
                rb,
                XC_READBACK_MISMATCH,
            );
            return Err(PerCpuError::ReadbackMismatch { wrote: base, read: rb });
        }

        // 自识别（F0192 语义复用）：GS 已指向本区，self_ptr 读回必须 ==
        // 实例基址——"认得出自己"判定。
        if let Err(e) = area.self_identify() {
            self.rollback_to_bsp_best_effort();
            self.counters.failed = self.counters.failed.saturating_add(1);
            let _ = area.bump_stats_slot(SLOT_SELF_ID_FAIL);
            let _ = area.bump_stats_slot(SLOT_SWITCH_FAIL);
            let _ = self.ledger.push(EventKind::SwitchFail, e.xcode(), header_cpu, base);
            self.fail_record(
                SwitchPhase::Commit,
                header_cpu,
                base,
                prev,
                self.state,
                rb,
                e.xcode(),
            );
            return Err(e);
        }

        // 全量验签（magic / cpu_id / self_ptr / 尾 magic；防 begin 之后
        // commit 之前的窗口期破坏）。
        if let Err(e) = area.validate() {
            self.rollback_to_bsp_best_effort();
            self.counters.failed = self.counters.failed.saturating_add(1);
            let _ = area.bump_stats_slot(SLOT_VALIDATE_FAIL);
            let _ = area.bump_stats_slot(SLOT_SWITCH_FAIL);
            let _ = self.ledger.push(EventKind::SwitchFail, e.xcode(), header_cpu, base);
            self.fail_record(
                SwitchPhase::Commit,
                header_cpu,
                base,
                prev,
                self.state,
                rb,
                e.xcode(),
            );
            return Err(e);
        }

        // 落座。
        self.state = SwitchState::ApResident;
        self.resident_cpu = Some(pend.cpu_id);
        self.pending = None;
        self.counters.committed = self.counters.committed.saturating_add(1);
        let _ = area.bump_stats_slot(SLOT_SWITCH_COMMIT);
        let _ = area.bump_stats_slot(SLOT_VALIDATE_PASS);
        let _ = self.ledger.push(EventKind::SwitchCommit, 0, header_cpu, base);
        let rec = SwitchRecord {
            seq: self.next_record_seq(),
            phase: SwitchPhase::Commit,
            ok: true,
            target_cpu: header_cpu,
            target_base: base,
            prev_state: prev,
            new_state: SwitchState::ApResident,
            gs_readback: rb,
            xcode: 0,
        };
        self.push_record(rec);
        Ok(rec)
    }

    /// 主动弃切：仅 Switching 态合法；状态回 BspResident，reason 为
    /// 弃切诊断码（0 允许，表示无码弃切）。
    pub fn abort_switch(&mut self, reason: u32) -> Result<SwitchRecord, PerCpuError> {
        let prev = self.state;
        if prev != SwitchState::Switching {
            return Err(PerCpuError::InvalidState {
                need: SwitchState::Switching.code(),
                got: prev.code(),
            });
        }
        let pend = self.pending.take();
        let (target_cpu, target_base) = match pend {
            Some(p) => (p.cpu_id, p.base),
            None => (0, 0),
        };
        self.state = SwitchState::BspResident;
        self.counters.aborted = self.counters.aborted.saturating_add(1);
        let _ = self.ledger.push(EventKind::SwitchAbort, reason, target_cpu, target_base);
        let rec = SwitchRecord {
            seq: self.next_record_seq(),
            phase: SwitchPhase::Abort,
            ok: false,
            target_cpu,
            target_base,
            prev_state: prev,
            new_state: SwitchState::BspResident,
            gs_readback: self.backend.read(),
            xcode: reason,
        };
        self.push_record(rec);
        Ok(rec)
    }

    /// 返回 BSP：仅 ApResident 态合法；写 GS 回 BSP 基址并读回核账。
    pub fn switch_back_to_bsp(&mut self) -> Result<SwitchRecord, PerCpuError> {
        let prev = self.state;
        if prev != SwitchState::ApResident {
            return Err(PerCpuError::InvalidState {
                need: SwitchState::ApResident.code(),
                got: prev.code(),
            });
        }
        let leaving_cpu = self.resident_cpu.take().unwrap_or(0);
        self.backend.write(self.bsp_base)?;
        let rb = self.backend.read();
        if rb != self.bsp_base {
            self.counters.readback_fails = self.counters.readback_fails.saturating_add(1);
            let _ = self.ledger.push(
                EventKind::ReadbackFail,
                XC_READBACK_MISMATCH,
                leaving_cpu,
                self.bsp_base,
            );
            self.fail_record(
                SwitchPhase::BackToBsp,
                leaving_cpu,
                self.bsp_base,
                prev,
                SwitchState::ApResident,
                rb,
                XC_READBACK_MISMATCH,
            );
            return Err(PerCpuError::ReadbackMismatch { wrote: self.bsp_base, read: rb });
        }
        self.state = SwitchState::BspResident;
        self.resident_cpu = None;
        self.counters.back_to_bsp = self.counters.back_to_bsp.saturating_add(1);
        let _ = self.ledger.push(EventKind::BackToBsp, 0, leaving_cpu, self.bsp_base);
        let rec = SwitchRecord {
            seq: self.next_record_seq(),
            phase: SwitchPhase::BackToBsp,
            ok: true,
            target_cpu: leaving_cpu,
            target_base: self.bsp_base,
            prev_state: prev,
            new_state: SwitchState::BspResident,
            gs_readback: rb,
            xcode: 0,
        };
        self.push_record(rec);
        Ok(rec)
    }
}

/// 切换账页阶段稳定序号（指纹用，声明顺序即返回序，双向钉死）。
pub fn switch_phase_ordinal(phase: SwitchPhase) -> u8 {
    match phase {
        SwitchPhase::Begin => 0,
        SwitchPhase::Commit => 1,
        SwitchPhase::Abort => 2,
        SwitchPhase::BackToBsp => 3,
    }
}

// ---------------------------------------------------------------------------
// IsolationLedger —— 失败区隔离账
// ---------------------------------------------------------------------------

/// 隔离账默认容量。来源：32 席（超过即提示系统性损坏——正常启动流程
/// 坏区应为个位数）。
pub const ISOLATION_DEFAULT_CAP: usize = 32;

/// 单条隔离记录：哪个区（基址 + cpu_id）因什么码被关账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IsolatedArea {
    /// 隔离账内序号（wrapping 单调）。
    pub seq: u32,
    /// 被隔离区基址。
    pub base: u64,
    /// 被隔离区 cpu_id（读回面，可能已坏，0 表示读不出）。
    pub cpu_id: u32,
    /// 触发隔离的诊断码（0x1401~0x140F）。
    pub xcode: u32,
}

/// 定容隔离账：坏区只进不改、账满显性丢弃计数。
pub struct IsolationLedger {
    entries: Vec<IsolatedArea>,
    cap: usize,
    seq: u32,
    dropped: u32,
}

impl IsolationLedger {
    /// 建账（cap 至少 1）。
    pub fn new(cap: usize) -> IsolationLedger {
        IsolationLedger {
            entries: Vec::new(),
            cap: if cap == 0 { 1 } else { cap },
            seq: 0,
            dropped: 0,
        }
    }

    /// 关账一笔坏区。
    pub fn isolate(&mut self, base: u64, cpu_id: u32, xcode: u32) -> Result<(), PerCpuError> {
        self.seq = self.seq.wrapping_add(1);
        if self.entries.len() >= self.cap {
            self.dropped = self.dropped.saturating_add(1);
            return Err(PerCpuError::IsolateLedgerFull { cap: self.cap });
        }
        self.entries.push(IsolatedArea { seq: self.seq, base, cpu_id, xcode });
        Ok(())
    }

    /// 在账条数。
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// 显性丢弃计数。
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    /// 在账只读视图。
    pub fn entries(&self) -> &[IsolatedArea] {
        &self.entries
    }

    /// 隔离账 FNV 指纹（对拍用）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_init();
        let mut i = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            h = fnv1a64_u64(h, e.seq as u64);
            h = fnv1a64_u64(h, e.base);
            h = fnv1a64_u64(h, (e.xcode as u64) << 32 | (e.cpu_id as u64));
            i += 1;
        }
        h = fnv1a64_u64(h, self.dropped as u64);
        h = fnv1a64_u64(h, self.seq as u64);
        h
    }
}

/// 自识别全区扫描报告。
#[derive(Clone, Debug)]
pub struct IdentityScanReport {
    /// 扫描区数（wrapping 计）。
    pub scanned: u32,
    /// 通过区数。
    pub passed: u32,
    /// 失败区数。
    pub failed: u32,
    /// 失败清单：(基址, 诊断码)，顺序与扫描序一致。
    pub failures: Vec<(u64, u32)>,
    /// 隔离账溢出丢弃数（本次扫描期间新增）。
    pub isolate_dropped: u32,
}

impl IdentityScanReport {
    /// 扫描是否全绿。
    pub fn all_green(&self) -> bool {
        self.failed == 0 && self.isolate_dropped == 0
    }
}

/// 自识别协议（F0192 语义复用的批量面）：全区扫描 self_ptr == 实例基址
/// （外加三自洽 + 尾 magic 的全量验签），失败区进隔离账并在本区统计槽
/// 记 SELF_ID_FAIL / VALIDATE_FAIL，事件账同步落账。
pub fn verify_self_identity(
    areas: &mut [PerCpuArea],
    iso: &mut IsolationLedger,
    ledger: &mut EventLedger,
) -> IdentityScanReport {
    let mut report = IdentityScanReport {
        scanned: 0,
        passed: 0,
        failed: 0,
        failures: Vec::new(),
        isolate_dropped: 0,
    };
    let mut i = 0;
    while i < areas.len() {
        let area = &mut areas[i];
        report.scanned = report.scanned.wrapping_add(1);
        let base = area.base();
        let cpu = area.cpu_id().unwrap_or(0);
        match area.validate() {
            Ok(()) => {
                report.passed = report.passed.wrapping_add(1);
                let _ = area.bump_stats_slot(SLOT_VALIDATE_PASS);
            }
            Err(e) => {
                report.failed = report.failed.wrapping_add(1);
                report.failures.push((base, e.xcode()));
                let _ = area.bump_stats_slot(SLOT_SELF_ID_FAIL);
                let _ = area.bump_stats_slot(SLOT_VALIDATE_FAIL);
                let _ = ledger.push(EventKind::SelfIdFail, e.xcode(), cpu, base);
                let iso_res = iso.isolate(base, cpu, e.xcode());
                if iso_res.is_err() {
                    report.isolate_dropped = report.isolate_dropped.wrapping_add(1);
                } else {
                    let _ = ledger.push(EventKind::Isolated, e.xcode(), cpu, base);
                }
            }
        }
        i += 1;
    }
    report
}

// ---------------------------------------------------------------------------
// 注入面 —— magic / cpu_id / self_ptr 三类错注入逐一检出
// ---------------------------------------------------------------------------

/// 注入类别闭集（验收要求的三个注入面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjectionKind {
    /// 区头 magic 破坏（全字节取反）。
    Magic,
    /// 区头 cpu_id 错位（+1 偏移）。
    CpuId,
    /// self_ptr 单比特翻转（基址错位，最隐蔽类）。
    SelfPtr,
}

impl InjectionKind {
    /// 注入面稳定序号（账面用）。
    pub fn code(self) -> u8 {
        match self {
            InjectionKind::Magic => 0,
            InjectionKind::CpuId => 1,
            InjectionKind::SelfPtr => 2,
        }
    }

    /// 该类注入被检出时必须报的诊断码（检出面与注入面同源绑定）。
    pub fn expected_xcode(self) -> u32 {
        match self {
            InjectionKind::Magic => XC_MAGIC_MISMATCH,
            InjectionKind::CpuId => XC_CPUID_MISMATCH,
            InjectionKind::SelfPtr => XC_SELF_PTR_MISMATCH,
        }
    }

    /// 注入面名（诊断输出）。
    pub fn name(self) -> &'static str {
        match self {
            InjectionKind::Magic => "magic-corrupt",
            InjectionKind::CpuId => "cpuid-corrupt",
            InjectionKind::SelfPtr => "selfptr-corrupt",
        }
    }
}

/// 注入演练战果账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectionArmory {
    /// 布防次数。
    pub armed: u32,
    /// 检出次数。
    pub caught: u32,
    /// 泄漏次数（检出面失明，最严重）。
    pub leaked: u32,
    /// 修复回归次数。
    pub repaired: u32,
}

impl InjectionArmory {
    /// 空战果账。
    pub fn new() -> InjectionArmory {
        InjectionArmory { armed: 0, caught: 0, leaked: 0, repaired: 0 }
    }

    /// 战果是否全检出（armed == caught 且零泄漏）。
    pub fn all_caught(&self) -> bool {
        self.armed == self.caught && self.leaked == 0
    }
}

/// 注入演练全套：三类注入逐次布防 → 验签必须以绑定诊断码报错 →
/// 修复 → 验签必须回归全绿。任何偏差按注入泄漏处理并即刻终止。
/// 战果同时落在区统计槽（INJECT_ARMED/CAUGHT/LEAK）与事件账。
pub fn run_injection_suite(
    area: &mut PerCpuArea,
    ledger: &mut EventLedger,
) -> Result<InjectionArmory, PerCpuError> {
    let mut armory = InjectionArmory::new();
    const KINDS: [InjectionKind; 3] = [
        InjectionKind::Magic,
        InjectionKind::CpuId,
        InjectionKind::SelfPtr,
    ];
    let mut i = 0;
    while i < KINDS.len() {
        let kind = KINDS[i];
        let cpu = area.cpu_id().unwrap_or(0);
        let base = area.base();

        // 布防。
        area.inject(kind)?;
        armory.armed = armory.armed.wrapping_add(1);
        let _ = area.bump_stats_slot(SLOT_INJECT_ARMED);
        let _ = ledger.push(EventKind::InjectionArmed, kind.expected_xcode(), cpu, base);

        // 检出判定：验签必须报绑定码。
        match area.validate() {
            Err(e) if e.xcode() == kind.expected_xcode() => {
                armory.caught = armory.caught.wrapping_add(1);
                let _ = area.bump_stats_slot(SLOT_INJECT_CAUGHT);
                let _ = ledger.push(EventKind::InjectionCaught, e.xcode(), cpu, base);
            }
            Err(e) => {
                // 检出了但码不符：检测面与注入面失配，按泄漏收口
                // （战果账随 Err 丢弃，泄漏不再做死赋值，事件/统计已留痕）。
                let _ = area.bump_stats_slot(SLOT_INJECT_LEAK);
                let _ = ledger.push(EventKind::InjectionLeak, e.xcode(), cpu, base);
                area.repair_all()?;
                return Err(PerCpuError::InjectionLeak {
                    kind: kind.code(),
                    xcode: e.xcode(),
                });
            }
            Ok(()) => {
                // 注入后验签仍过：检测面失明，最严重泄漏
                // （战果账随 Err 丢弃，事件/统计已留痕）。
                let _ = area.bump_stats_slot(SLOT_INJECT_LEAK);
                let _ = ledger.push(EventKind::InjectionLeak, 0, cpu, base);
                area.repair_all()?;
                return Err(PerCpuError::InjectionLeak { kind: kind.code(), xcode: 0 });
            }
        }

        // 修复回归：验签必须回到全绿。
        area.repair_all()?;
        armory.repaired = armory.repaired.wrapping_add(1);
        if area.validate().is_err() {
            return Err(PerCpuError::InjectionLeak {
                kind: kind.code(),
                xcode: XC_INJECT_LEAK,
            });
        }
        i += 1;
    }
    Ok(armory)
}

// ---------------------------------------------------------------------------
// health() —— 体检（各账线联合体检 + 问题清单）
// ---------------------------------------------------------------------------

/// 体检报告：各账线快照 + 问题清单（中文可操作）+ 总评。
#[derive(Clone, Debug)]
pub struct HealthReport {
    /// 注册表席数。
    pub registry_entries: usize,
    /// BSP 槽是否已登记。
    pub bsp_registered: bool,
    /// 切换器当前状态。
    pub switch_state: SwitchState,
    /// commit 成功数。
    pub switch_committed: u32,
    /// commit 失败数。
    pub switch_failed: u32,
    /// begin 拦截数。
    pub switch_blocked: u32,
    /// 切换账在账页数。
    pub records_kept: usize,
    /// 切换账丢弃数。
    pub records_dropped: u32,
    /// 隔离账在账条数。
    pub isolated_entries: usize,
    /// 隔离账丢弃数。
    pub isolated_dropped: u32,
    /// 切换器事件账累计入账数。
    pub switch_event_count: u32,
    /// 切换器事件账丢弃数。
    pub switch_events_dropped: u32,
    /// 注入布防数（未演练时 0）。
    pub injection_armed: u32,
    /// 注入检出数。
    pub injection_caught: u32,
    /// 注入泄漏数。
    pub injection_leaked: u32,
    /// 问题清单（中文可操作诊断）。
    pub issues: Vec<&'static str>,
    /// 总评：零问题才为 true。
    pub healthy: bool,
}

impl HealthReport {
    /// 人读摘要（format 组装，诊断输出用）。
    pub fn summary(&self) -> String {
        format!(
            "unx0344 health: healthy={} reg={} bsp={} state={} commit={} fail={} \
             block={} iso={} drop(ev/rec/iso)={}/{}/{} inject(a/c/l)={}/{}/{}",
            self.healthy,
            self.registry_entries,
            self.bsp_registered,
            self.switch_state.name(),
            self.switch_committed,
            self.switch_failed,
            self.switch_blocked,
            self.isolated_entries,
            self.switch_events_dropped,
            self.records_dropped,
            self.isolated_dropped,
            self.injection_armed,
            self.injection_caught,
            self.injection_leaked,
        )
    }
}

/// 联合体检：注册表 + 切换器 + 隔离账 + 事件账（+ 可选注入战果账）。
/// 总评标准：BSP 在册、各账零丢弃、零隔离在案、切换零失败零拦截、
/// 注入零泄漏——任一不满足即 unhealthy 且问题清单给出定位。
pub fn health<B: GsBaseBackend>(
    reg: &CpuIdRegistry,
    sw: &GsBaseSwitcher<B>,
    iso: &IsolationLedger,
    ledger: &EventLedger,
    armory: Option<&InjectionArmory>,
) -> HealthReport {
    let mut issues: Vec<&'static str> = Vec::new();
    let mut healthy = true;
    let c = sw.counters();

    if !reg.bsp_registered() {
        issues.push("BSP 固定槽 0 未登记：先 register_bsp，否则无回滚归宿。");
        healthy = false;
    }
    if ledger.dropped() != 0 {
        issues.push("事件账有丢弃：调大容量或提高消费频率，账缺页会破坏对账。");
        healthy = false;
    }
    if iso.dropped() != 0 {
        issues.push("隔离账有丢弃：坏区数量超账容，先排查系统性损坏再扩容。");
        healthy = false;
    }
    if iso.entry_count() != 0 {
        issues.push("隔离账在案坏区未清零：修复后必须复扫 verify_self_identity 归零。");
        healthy = false;
    }
    if c.failed != 0 {
        issues.push("切换失败数非零：对照切换账失败页的诊断码逐页排查。");
        healthy = false;
    }
    if c.blocked != 0 {
        issues.push("begin 拦截数非零：存在双切换/互斥违例试图，审查调用序。");
        healthy = false;
    }
    if sw.records_dropped() != 0 {
        issues.push("切换账有丢弃：调大 records_cap，失败页缺账不可接受。");
        healthy = false;
    }
    if let Some(arm) = armory {
        if arm.leaked != 0 {
            issues.push("注入泄漏非零：检测面失明或失配，最高优先级排查。");
            healthy = false;
        }
        if !arm.all_caught() {
            issues.push("注入战果不全检出：armed != caught，演练覆盖有缺口。");
            healthy = false;
        }
    }
    if healthy {
        issues.push("全部账线体检通过。");
    }

    HealthReport {
        registry_entries: reg.len(),
        bsp_registered: reg.bsp_registered(),
        switch_state: sw.state(),
        switch_committed: c.committed,
        switch_failed: c.failed,
        switch_blocked: c.blocked,
        records_kept: sw.records().len(),
        records_dropped: sw.records_dropped(),
        isolated_entries: iso.entry_count(),
        isolated_dropped: iso.dropped(),
        switch_event_count: sw.ledger().event_count(),
        switch_events_dropped: sw.ledger().dropped(),
        injection_armed: armory.map(|a| a.armed).unwrap_or(0),
        injection_caught: armory.map(|a| a.caught).unwrap_or(0),
        injection_leaked: armory.map(|a| a.leaked).unwrap_or(0),
        issues,
        healthy,
    }
}

// ---------------------------------------------------------------------------
// 诊断段自检 —— XC 闭集段内独占 + 两两互异 + 段位全覆盖
// ---------------------------------------------------------------------------

/// 诊断段自检：XC_* 闭集必须全部落在 (0x1400, 0x140F] 且两两互异，
/// 并按位序铺满 0x1401~0x140F（15 席全用，不留缝）。
pub fn segment_self_check() -> Result<(), PerCpuError> {
    const CODES: [u32; 15] = [
        XC_MAGIC_MISMATCH,
        XC_CPUID_MISMATCH,
        XC_SELF_PTR_MISMATCH,
        XC_NOT_REGISTERED,
        XC_DUP_CPU_ID,
        XC_REGISTRY_FULL,
        XC_BSP_SLOT,
        XC_STATE_INVALID,
        XC_DOUBLE_SWITCH,
        XC_LEDGER_FULL,
        XC_ISOLATE_FULL,
        XC_FINGERPRINT_MISMATCH,
        XC_INJECT_LEAK,
        XC_READBACK_MISMATCH,
        XC_DUP_BASE,
    ];
    let base = PC_SEG_BASE as u32;
    // 段内性 + 段位铺满：每个码恰为 base + 1..=15 中的一席。
    let mut i = 0;
    while i < CODES.len() {
        if CODES[i] <= base || CODES[i] > base + 0x0F {
            return Err(PerCpuError::SegmentViolation { code: CODES[i] });
        }
        // 铺满检查：本码的段内偏移位不得被重复占用（两两互异的等价形）。
        let mut j = 0;
        while j < i {
            if CODES[j] == CODES[i] {
                return Err(PerCpuError::SegmentViolation { code: CODES[i] });
            }
            j += 1;
        }
        i += 1;
    }
    // 段位全覆盖：15 席互异且都在 15 席内 ⇒ 恰好铺满（鸽笼）。
    Ok(())
}

// ---------------------------------------------------------------------------
// 双跑确定性对拍 —— Lcg 驱动编排，同种子必同指纹
// ---------------------------------------------------------------------------

/// LCG 乘子。来源：Knuth《The Art of Computer Programming》Vol.2 MMIX
/// LCG 推荐参数（multiplier 6364136223846793005）。
pub const LCG_MULT: u64 = 6_364_136_223_846_793_005;
/// LCG 增量。来源：同上（increment 1442695040888963407）。
pub const LCG_INC: u64 = 1_442_695_040_888_963_407;
/// 种子搅拌盐。来源：黄金分割常数 2^64/φ（0x9E3779B97F4A7C15），
/// 用于打散弱种子。
pub const LCG_SEED_SALT: u64 = 0x9E37_79B9_7F4A_7C15;

/// 确定性伪随机源（对拍驱动，无外部熵，全整数）。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 播种（盐搅拌后入态）。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed ^ LCG_SEED_SALT }
    }

    /// 推进并取全宽值（wrapping 显性）。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(LCG_MULT).wrapping_add(LCG_INC);
        self.state
    }

    /// 取 [0, bound)（bound=0 恒返 0；取模偏斜对演练抽签无影响）。
    pub fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next_u64() % bound
    }
}

/// 编排常量：逻辑 BSP 区基址（模型地址，16 对齐）。
pub const ORCH_BSP_BASE: u64 = 0x0001_0000;
/// 编排常量：模型栈长 256KiB。
pub const ORCH_STACK_SIZE: u64 = 0x0004_0000;
/// 编排常量：AP 数上限（抽签 3~8）。
pub const ORCH_MAX_AP: usize = 8;

/// 编排战果摘要（双跑对拍的比较体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OrchSummary {
    /// AP 数。
    pub ap_count: u32,
    /// commit 成功数。
    pub committed: u32,
    /// 返回 BSP 次数。
    pub back_to_bsp: u32,
    /// 双切换拦截数。
    pub blocked: u32,
    /// 区体注入次数。
    pub injected: u32,
    /// GS 写读错位故障次数。
    pub gs_faults: u32,
    /// 隔离账在案数（演练收官应为 0）。
    pub isolated: u32,
    /// 自识别扫描区数。
    pub scanned: u32,
    /// 切换器事件账累计入账数。
    pub event_count: u32,
    /// 全账线复合 FNV 指纹。
    pub fingerprint: u64,
}

/// 全流程确定性编排（对拍被测体）：
///   1. 建注册表（BSP + N 个 AP）与各区；
///   2. 逐 AP：begin →（抽签）双切换拦截演练 →（抽签）self_ptr 注入
///      演练（commit 必拒并回滚）→（抽签）GS 写读错位故障演练
///      （commit 必拒并回滚）→ 重切收口 → back_to_bsp；
///   3. 收官：全区自识别扫描（必须全绿）+ 注入全套演练（必须全检出）
///      + 全账线复合指纹。
/// 同种子两次调用的摘要必须逐字段一致。
pub fn orchestrate(seed: u64) -> Result<OrchSummary, PerCpuError> {
    let mut rng = Lcg::new(seed);
    // AP 数 3..=8：够演互斥/拦截/注入抽签，又不至于账页爆量。
    let ap_count = 3 + rng.below((ORCH_MAX_AP - 3 + 1) as u64) as usize;

    let mut reg = CpuIdRegistry::new(REGISTRY_DEFAULT_CAP);
    reg.register_bsp(ORCH_BSP_BASE)?;
    let mut ledger = EventLedger::new(LEDGER_DEFAULT_CAP);
    let mut iso = IsolationLedger::new(ISOLATION_DEFAULT_CAP);
    let backend = RamGsBackend::new(ORCH_BSP_BASE);
    let mut sw = GsBaseSwitcher::new(backend, ORCH_BSP_BASE, RECORDS_DEFAULT_CAP);

    // 建区 + 登记（先登记后建区亦可，注册制不关心时序）。
    let mut aps: Vec<PerCpuArea> = Vec::new();
    let mut i: usize = 0;
    while i < ap_count {
        let cpu = (i as u32) + 1;
        let base = ORCH_BSP_BASE + ((i as u64 + 1) * PERCPU_AREA_SIZE as u64);
        reg.register_ap(cpu, base)?;
        aps.push(PerCpuArea::init(
            base,
            cpu,
            base + PERCPU_AREA_SIZE as u64,
            ORCH_STACK_SIZE,
        )?);
        i += 1;
    }

    let mut blocked: u32 = 0;
    let mut injected: u32 = 0;
    let mut gs_faults: u32 = 0;
    i = 0;
    while i < aps.len() {
        // —— 顶部 begin：受理在途目标 ——
        sw.begin_switch(&reg, &mut aps[i])?;

        // —— 抽签 1：Switching 态下第二根 begin，必拦（双切换冲突）——
        if rng.below(2) == 0 {
            let r = sw.begin_switch(&reg, &mut aps[i]);
            match r {
                Err(PerCpuError::DoubleSwitchConflict { .. }) => {
                    blocked = blocked.saturating_add(1);
                }
                Ok(_) => return Err(PerCpuError::InvariantBroken { site: 0x0344_0001 }),
                Err(e) => return Err(e),
            }
        }

        // —— 抽签 2：self_ptr 注入 → commit 必拒（自识别拒收 + 回滚）——
        if rng.below(3) == 0 {
            aps[i].inject(InjectionKind::SelfPtr)?;
            injected = injected.saturating_add(1);
            let r = sw.commit_switch(&mut aps[i]);
            match r {
                Err(PerCpuError::SelfPtrMismatch { .. }) => {}
                Ok(_) => return Err(PerCpuError::InvariantBroken { site: 0x0344_0002 }),
                Err(e) => {
                    aps[i].repair_all()?;
                    return Err(e);
                }
            }
            aps[i].repair_all()?;
        }

        // 若抽签 2 触发回滚（state 已归 BspResident），重新 begin。
        if sw.state() == SwitchState::BspResident {
            sw.begin_switch(&reg, &mut aps[i])?;
        }

        // —— 抽签 3：GS 写读错位故障 → commit 必拒（读回核账 + 回滚）——
        if rng.below(3) == 1 {
            sw.backend_mut().arm_fault();
            gs_faults = gs_faults.saturating_add(1);
            let r = sw.commit_switch(&mut aps[i]);
            match r {
                Err(PerCpuError::ReadbackMismatch { .. }) => {}
                Ok(_) => {
                    sw.backend_mut().disarm_fault();
                    return Err(PerCpuError::InvariantBroken { site: 0x0344_0003 });
                }
                Err(e) => {
                    sw.backend_mut().disarm_fault();
                    return Err(e);
                }
            }
            sw.backend_mut().disarm_fault();
        }

        // —— 故障重试：若已回滚则重切收口，正式落座 + 返回 BSP ——
        if sw.state() == SwitchState::BspResident {
            sw.begin_switch(&reg, &mut aps[i])?;
        }
        sw.commit_switch(&mut aps[i])?;
        sw.switch_back_to_bsp()?;
        i += 1;
    }

    // —— 收官：全区自识别扫描（全绿铁律）——
    let scan = verify_self_identity(&mut aps, &mut iso, &mut ledger);
    if scan.failed != 0 || scan.isolate_dropped != 0 {
        return Err(PerCpuError::InvariantBroken { site: 0x0344_0004 });
    }

    // —— 收官：注入全套演练（对最后一个 AP，套内自修复）——
    if let Some(last) = aps.len().checked_sub(1) {
        let armory = run_injection_suite(&mut aps[last], &mut ledger)?;
        if !armory.all_caught() || armory.armed != 3 {
            return Err(PerCpuError::InvariantBroken { site: 0x0344_0005 });
        }
    }

    // —— 复合指纹：事件账 + 注册表 + 切换账 + 计数 + 隔离账 + 各区统计 ——
    let mut fp = fnv1a64_init();
    fp = fnv1a64_u64(fp, ledger.fingerprint());
    fp = fnv1a64_u64(fp, reg.entries_fingerprint());
    fp = fnv1a64_u64(fp, sw.records_fingerprint());
    fp = fnv1a64_u64(fp, sw.counters().committed as u64);
    fp = fnv1a64_u64(fp, sw.counters().blocked as u64);
    fp = fnv1a64_u64(fp, iso.fingerprint());
    let mut k = 0;
    while k < aps.len() {
        fp = fnv1a64_u64(fp, aps[k].stats_fingerprint());
        k += 1;
    }

    Ok(OrchSummary {
        ap_count: ap_count as u32,
        committed: sw.counters().committed,
        back_to_bsp: sw.counters().back_to_bsp,
        blocked,
        injected,
        gs_faults,
        isolated: iso.entry_count() as u32,
        scanned: scan.scanned,
        event_count: ledger.event_count(),
        fingerprint: fp,
    })
}

/// 双跑确定性对拍：同种子编排两次，指纹必须一致；不一致按
/// FingerprintMismatch 报错（确定性回归的自动闸）。
pub fn determinism_check(seed: u64) -> Result<bool, PerCpuError> {
    let a = orchestrate(seed)?;
    let b = orchestrate(seed)?;
    if a.fingerprint == b.fingerprint {
        Ok(true)
    } else {
        Err(PerCpuError::FingerprintMismatch { a: a.fingerprint, b: b.fingerprint })
    }
}

// ---------------------------------------------------------------------------
// 测试（仅 host 测试路径；内核路径零 unwrap/expect/panic/assert）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const T_BASE_0: u64 = 0x0002_0000;
    const T_AREA_STEP: u64 = PERCPU_AREA_SIZE as u64;

    /// 测试用区工厂：基址按区面积步进，栈紧随区后。
    fn mk_area(idx: usize, cpu: u32) -> PerCpuArea {
        let base = T_BASE_0 + (idx as u64 + 1) * T_AREA_STEP;
        PerCpuArea::init(base, cpu, base + T_AREA_STEP, 0x1_0000).unwrap()
    }

    fn mk_env(ap: u32) -> (CpuIdRegistry, Vec<PerCpuArea>, GsBaseSwitcher<RamGsBackend>) {
        let mut reg = CpuIdRegistry::new(REGISTRY_DEFAULT_CAP);
        reg.register_bsp(ORCH_BSP_BASE).unwrap();
        let mut aps: Vec<PerCpuArea> = Vec::new();
        let mut i = 0u32;
        while i < ap {
            let base = ORCH_BSP_BASE + ((i as u64 + 1) * T_AREA_STEP);
            reg.register_ap(i + 1, base).unwrap();
            aps.push(PerCpuArea::init(base, i + 1, base + T_AREA_STEP, ORCH_STACK_SIZE).unwrap());
            i += 1;
        }
        let backend = RamGsBackend::new(ORCH_BSP_BASE);
        let sw = GsBaseSwitcher::new(backend, ORCH_BSP_BASE, RECORDS_DEFAULT_CAP);
        (reg, aps, sw)
    }

    // ---- 布局与编解码 ----

    #[test]
    fn layout_consts_and_table() {
        assert_eq!(PERCPU_AREA_SIZE, 8192);
        assert_eq!(OFF_SELF_PTR, 8);
        assert_eq!(OFF_FOOTER, PERCPU_AREA_SIZE - 4);
        assert!(layout_table_offsets_ok());
        // 运行期对 const 布局表逐行复核（与编译期断言双份）。
        let mut i = 1;
        while i < LAYOUT_TABLE.len() {
            let prev_end = LAYOUT_TABLE[i - 1].1 + LAYOUT_TABLE[i - 1].2;
            assert!(LAYOUT_TABLE[i].1 >= prev_end);
            assert!(LAYOUT_TABLE[i].1 + LAYOUT_TABLE[i].2 <= PERCPU_AREA_SIZE);
            i += 1;
        }
        assert_eq!(field_offset(FieldId::SelfPtr), OFF_SELF_PTR);
        assert_eq!(field_offset(FieldId::Footer), OFF_FOOTER);
        assert_eq!(field_offset(FieldId::Magic), OFF_MAGIC);
        assert_eq!(FieldId::Stats.name(), "stats");
    }

    #[test]
    fn byte_codec_roundtrip_and_bounds() {
        let mut raw = RawArea::zeroed();
        raw.set_u64(0x100, 0x0123_4567_89AB_CDEF).unwrap();
        assert_eq!(raw.u64_at(0x100).unwrap(), 0x0123_4567_89AB_CDEF);
        raw.set_u32(0x200, 0xDEAD_BEEF).unwrap();
        assert_eq!(raw.u32_at(0x200).unwrap(), 0xDEAD_BEEF);
        // 越界必须显性 Err，零 panic 路径。
        assert!(raw.u64_at(PERCPU_AREA_SIZE - 7).is_err());
        assert!(raw.u32_at(PERCPU_AREA_SIZE - 3).is_err());
        assert!(raw.set_u64(PERCPU_AREA_SIZE, 0).is_err());
        assert!(raw.u64_at(usize::MAX).is_err());
    }

    // ---- 三自洽验签 + 自识别 ----

    #[test]
    fn smoke_three_way_gate_and_repair() {
        let mut a = mk_area(0, 1);
        assert!(a.validate().is_ok());
        assert!(a.three_way_consistency().is_ok());
        assert!(a.self_identify().is_ok());
        assert_eq!(a.self_ptr().unwrap(), a.base());

        // magic 注入 → 三自洽报 MAGIC_MISMATCH。
        a.inject(InjectionKind::Magic).unwrap();
        match a.three_way_consistency() {
            Err(PerCpuError::MagicMismatch { expected, .. }) => assert_eq!(expected, PC_MAGIC),
            other => panic!("expect MagicMismatch, got {:?}", other),
        }
        a.repair_all().unwrap();
        assert!(a.validate().is_ok());

        // cpu_id 注入 → 报 CPUID_MISMATCH。
        a.inject(InjectionKind::CpuId).unwrap();
        assert!(matches!(
            a.three_way_consistency(),
            Err(PerCpuError::CpuIdMismatch { .. })
        ));
        a.repair_all().unwrap();

        // self_ptr 注入 → 报 SELF_PTR_MISMATCH（F0192 自识别拒收）。
        a.inject(InjectionKind::SelfPtr).unwrap();
        assert!(matches!(a.self_identify(), Err(PerCpuError::SelfPtrMismatch { .. })));
        assert!(matches!(
            a.validate(),
            Err(PerCpuError::SelfPtrMismatch { .. })
        ));
        a.repair_all().unwrap();
        assert!(a.validate().is_ok());
    }

    #[test]
    fn init_rejects_bad_config() {
        assert!(PerCpuArea::init(1, 1, 0x1000, 0x1000).is_err()); // 基址不对齐
        assert!(PerCpuArea::init(T_BASE_0, 1, 3, 0x1000).is_err()); // 栈基不对齐
        assert!(PerCpuArea::init(T_BASE_0, 1, 0x1000, 0).is_err()); // 零长栈
        assert!(PerCpuArea::init(T_BASE_0, 1, 0x1000, MAX_MODEL_STACK + 1).is_err());
        // u64::MAX 附近栈尾回绕。
        assert!(PerCpuArea::init(T_BASE_0, 1, u64::MAX - 8, 0x1000).is_err());
    }

    #[test]
    fn stats_slots_saturating() {
        let mut a = mk_area(1, 2);
        a.set_stats_slot(SLOT_SWITCH_COMMIT, u64::MAX).unwrap();
        let v = a.bump_stats_slot(SLOT_SWITCH_COMMIT).unwrap();
        assert_eq!(v, u64::MAX); // 饱和不回绕
        assert!(a.stats_slot(STAT_SLOTS).is_err()); // 越界槽显性 Err
        assert!(a.set_stats_slot(usize::MAX, 1).is_err());
    }

    // ---- 注册表 ----

    #[test]
    fn registry_bsp_fixed_and_rejects() {
        let mut reg = CpuIdRegistry::new(8);
        reg.register_bsp(ORCH_BSP_BASE).unwrap();
        // BSP 重复登记拒收。
        assert!(matches!(
            reg.register_bsp(ORCH_BSP_BASE + T_AREA_STEP),
            Err(PerCpuError::DuplicateCpuId { cpu_id: 0 })
        ));
        // AP 抢 0 号槽拒收。
        assert!(matches!(
            reg.register_ap(0, ORCH_BSP_BASE + T_AREA_STEP),
            Err(PerCpuError::BspSlotViolation { attempted_cpu: 0 })
        ));
        reg.register_ap(1, ORCH_BSP_BASE + T_AREA_STEP).unwrap();
        // 重复 cpu 拒收。
        assert!(matches!(
            reg.register_ap(1, ORCH_BSP_BASE + 2 * T_AREA_STEP),
            Err(PerCpuError::DuplicateCpuId { cpu_id: 1 })
        ));
        // 重复基址拒收。
        assert!(matches!(
            reg.register_ap(2, ORCH_BSP_BASE + T_AREA_STEP),
            Err(PerCpuError::DuplicateBase { .. })
        ));
        // 对齐拒收。
        assert!(matches!(
            reg.register_ap(3, ORCH_BSP_BASE + 1),
            Err(PerCpuError::BadAlign { .. })
        ));
        assert_eq!(reg.len(), 2);
        assert!(reg.pair_registered(1, ORCH_BSP_BASE + T_AREA_STEP));
        assert!(!reg.pair_registered(1, ORCH_BSP_BASE));
        assert_eq!(reg.lookup(1), Some(ORCH_BSP_BASE + T_AREA_STEP));
        assert_eq!(reg.owner_of(ORCH_BSP_BASE), Some(0));
        assert!(reg.bsp_registered());
        // 拒收计数全部有账（dup_cpu 计两笔：BSP 重复 + AP cpu=1 重复）。
        let (dc, db, bs, _full, al) = reg.reject_counts();
        assert_eq!(dc, 2);
        assert_eq!(db, 1);
        assert_eq!(bs, 1);
        assert_eq!(al, 1);
    }

    #[test]
    fn registry_full_reject() {
        let mut reg = CpuIdRegistry::new(2);
        reg.register_bsp(ORCH_BSP_BASE).unwrap();
        reg.register_ap(1, ORCH_BSP_BASE + T_AREA_STEP).unwrap();
        assert!(matches!(
            reg.register_ap(2, ORCH_BSP_BASE + 2 * T_AREA_STEP),
            Err(PerCpuError::RegistryFull { cap: 2 })
        ));
        let (_, _, _, full, _) = reg.reject_counts();
        assert_eq!(full, 1);
    }

    // ---- 切换状态机 ----

    #[test]
    fn smoke_double_switch_blocked() {
        let (reg, mut aps, mut sw) = mk_env(1);
        sw.begin_switch(&reg, &mut aps[0]).unwrap();
        assert_eq!(sw.state(), SwitchState::Switching);
        // Switching 态下第二根 begin 必拦。
        match sw.begin_switch(&reg, &mut aps[0]) {
            Err(PerCpuError::DoubleSwitchConflict { pending_cpu }) => {
                assert_eq!(pending_cpu, 1);
            }
            other => panic!("expect DoubleSwitchConflict, got {:?}", other),
        }
        assert_eq!(sw.counters().blocked, 1);
        assert_eq!(sw.state(), SwitchState::Switching); // 待决目标不动
        // 弃切收口回 BSP。
        sw.abort_switch(XC_DOUBLE_SWITCH).unwrap();
        assert_eq!(sw.state(), SwitchState::BspResident);
        assert!(sw.pending_target().is_none());
        // 非 Switching 态 abort 拒收。
        assert!(matches!(
            sw.abort_switch(0),
            Err(PerCpuError::InvalidState { .. })
        ));
    }

    #[test]
    fn switch_happy_path_full_cycle() {
        let (reg, mut aps, mut sw) = mk_env(2);
        let target = aps[0].base();
        sw.begin_switch(&reg, &mut aps[0]).unwrap();
        sw.commit_switch(&mut aps[0]).unwrap();
        assert_eq!(sw.state(), SwitchState::ApResident);
        assert_eq!(sw.resident_cpu(), Some(1));
        assert_eq!(sw.backend().read(), target); // GS 落靶
        assert_eq!(aps[0].stats_slot(SLOT_SWITCH_BEGIN).unwrap(), 1);
        assert_eq!(aps[0].stats_slot(SLOT_SWITCH_COMMIT).unwrap(), 1);

        // ApResident 下直接 begin 第二个 AP = 互斥违例。
        match sw.begin_switch(&reg, &mut aps[1]) {
            Err(PerCpuError::InvalidState { need, got }) => {
                assert_eq!(need, SwitchState::BspResident.code());
                assert_eq!(got, SwitchState::ApResident.code());
            }
            other => panic!("expect InvalidState, got {:?}", other),
        }

        sw.switch_back_to_bsp().unwrap();
        assert_eq!(sw.state(), SwitchState::BspResident);
        assert_eq!(sw.backend().read(), ORCH_BSP_BASE);
        assert_eq!(sw.counters().back_to_bsp, 1);

        // 第二个 AP 完整走一轮。
        sw.begin_switch(&reg, &mut aps[1]).unwrap();
        sw.commit_switch(&mut aps[1]).unwrap();
        sw.switch_back_to_bsp().unwrap();
        assert_eq!(sw.counters().committed, 2);

        // 切换账：seq 单调、阶段齐备。
        let recs = sw.records();
        assert!(recs.len() >= 6);
        let mut i = 1;
        while i < recs.len() {
            assert_eq!(recs[i].seq, recs[i - 1].seq + 1);
            i += 1;
        }
        assert!(recs.iter().all(|r| r.ok || r.xcode != 0));
        // 事件账：begin/commit/back 事件齐全。
        assert!(sw.ledger().event_count() >= 6);
    }

    #[test]
    fn commit_requires_begin_and_target_stability() {
        let (reg, mut aps, mut sw) = mk_env(2);
        // 无 begin 直接 commit 拒收。
        assert!(matches!(
            sw.commit_switch(&mut aps[0]),
            Err(PerCpuError::InvalidState { .. })
        ));
        // begin A 后 commit B（目标偷换）拒收。
        sw.begin_switch(&reg, &mut aps[0]).unwrap();
        match sw.commit_switch(&mut aps[1]) {
            Err(PerCpuError::InvalidState { need, got }) => {
                assert_eq!(need, SwitchState::Switching.code());
                assert_eq!(got, SwitchState::Switching.code());
            }
            other => panic!("expect InvalidState, got {:?}", other),
        }
        // 待决目标未被破坏，仍可对 A 正常收口。
        sw.commit_switch(&mut aps[0]).unwrap();
        assert_eq!(sw.resident_cpu(), Some(1));
    }

    #[test]
    fn unregistered_target_rejected() {
        let (reg, aps, mut sw) = mk_env(1);
        // 造一个未注册的区（注册表没登记它）。
        let mut ghost = mk_area(9, 42);
        match sw.begin_switch(&reg, &mut ghost) {
            Err(PerCpuError::NotRegistered { cpu_id, .. }) => assert_eq!(cpu_id, 42),
            other => panic!("expect NotRegistered, got {:?}", other),
        }
        // 注册制拒切不动区体：ghost 区验签依旧全绿。
        assert!(ghost.validate().is_ok());
        drop(aps);
    }

    #[test]
    fn gs_readback_fault_rollback() {
        let (reg, mut aps, mut sw) = mk_env(1);
        sw.backend_mut().arm_fault();
        sw.begin_switch(&reg, &mut aps[0]).unwrap();
        match sw.commit_switch(&mut aps[0]) {
            Err(PerCpuError::ReadbackMismatch { wrote, read }) => {
                assert_eq!(wrote, aps[0].base());
                assert_eq!(read, aps[0].base() ^ 0x1);
            }
            other => panic!("expect ReadbackMismatch, got {:?}", other),
        }
        // 已回滚：状态归 BSP，GS 读回 == BSP 基址。
        assert_eq!(sw.state(), SwitchState::BspResident);
        assert_eq!(sw.backend().read(), ORCH_BSP_BASE);
        assert_eq!(sw.counters().readback_fails, 1);
        assert_eq!(sw.counters().rollback_writes, 1);
        // 失败页在账。
        let recs = sw.records();
        assert!(recs.iter().any(|r| !r.ok && r.xcode == XC_READBACK_MISMATCH));
        // 修复路径：撤防后可正常切换。
        sw.backend_mut().disarm_fault();
        sw.begin_switch(&reg, &mut aps[0]).unwrap();
        sw.commit_switch(&mut aps[0]).unwrap();
        assert_eq!(sw.state(), SwitchState::ApResident);
    }

    // ---- 自识别扫描 + 隔离账 ----

    #[test]
    fn smoke_self_identity_scan_isolation() {
        let mut areas: Vec<PerCpuArea> = Vec::new();
        areas.push(mk_area(0, 1));
        areas.push(mk_area(1, 2));
        areas.push(mk_area(2, 3));
        areas.push(mk_area(3, 4));
        let bad_base = areas[2].base();
        areas[2].inject(InjectionKind::SelfPtr).unwrap();

        let mut iso = IsolationLedger::new(ISOLATION_DEFAULT_CAP);
        let mut ledger = EventLedger::new(LEDGER_DEFAULT_CAP);
        let report = verify_self_identity(&mut areas, &mut iso, &mut ledger);
        assert_eq!(report.scanned, 4);
        assert_eq!(report.passed, 3);
        assert_eq!(report.failed, 1);
        assert!(!report.all_green());
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].1, XC_SELF_PTR_MISMATCH);
        assert_eq!(iso.entry_count(), 1);
        assert_eq!(iso.entries()[0].xcode, XC_SELF_PTR_MISMATCH);
        assert_eq!(iso.entries()[0].base, bad_base);
        // 坏区统计槽有账。
        assert_eq!(areas[2].stats_slot(SLOT_SELF_ID_FAIL).unwrap(), 1);
        // 事件账双页：SelfIdFail + Isolated。
        assert!(ledger.events().iter().any(|e| e.kind == EventKind::SelfIdFail));
        assert!(ledger.events().iter().any(|e| e.kind == EventKind::Isolated));

        // 修复后复扫必须全绿。
        areas[2].repair_all().unwrap();
        let report2 = verify_self_identity(&mut areas, &mut iso, &mut ledger);
        assert!(report2.all_green());
        assert_eq!(report2.passed, 4);
        assert_eq!(iso.entry_count(), 1); // 历史隔离账不清洗
    }

    #[test]
    fn isolation_ledger_full_drops_openly() {
        let mut iso = IsolationLedger::new(2);
        iso.isolate(0x1000, 1, XC_MAGIC_MISMATCH).unwrap();
        iso.isolate(0x2000, 2, XC_CPUID_MISMATCH).unwrap();
        // 满账：显性 Err + dropped 计数（序号仍单调）。
        assert!(matches!(
            iso.isolate(0x3000, 3, XC_SELF_PTR_MISMATCH),
            Err(PerCpuError::IsolateLedgerFull { cap: 2 })
        ));
        assert_eq!(iso.dropped(), 1);
        assert_eq!(iso.entry_count(), 2);
        assert!(iso.fingerprint() != 0); // 指纹为实算非零
    }

    // ---- 注入面 ----

    #[test]
    fn smoke_injection_suite_all_detected() {
        let mut a = mk_area(0, 5);
        let mut ledger = EventLedger::new(LEDGER_DEFAULT_CAP);
        let armory = run_injection_suite(&mut a, &mut ledger).unwrap();
        assert_eq!(armory.armed, 3);
        assert_eq!(armory.caught, 3);
        assert_eq!(armory.leaked, 0);
        assert_eq!(armory.repaired, 3);
        assert!(armory.all_caught());
        // 区体修复回归全绿。
        assert!(a.validate().is_ok());
        // 事件账：3 对 armed/caught，零 leak。
        assert_eq!(
            ledger.events().iter().filter(|e| e.kind == EventKind::InjectionArmed).count(),
            3
        );
        assert_eq!(
            ledger.events().iter().filter(|e| e.kind == EventKind::InjectionCaught).count(),
            3
        );
        assert_eq!(
            ledger.events().iter().filter(|e| e.kind == EventKind::InjectionLeak).count(),
            0
        );
        // 区体统计槽战果有账。
        assert_eq!(a.stats_slot(SLOT_INJECT_ARMED).unwrap(), 3);
        assert_eq!(a.stats_slot(SLOT_INJECT_CAUGHT).unwrap(), 3);
        assert_eq!(a.stats_slot(SLOT_INJECT_LEAK).unwrap(), 0);
    }

    // ---- 指纹与确定性 ----

    #[test]
    fn fnv_known_vectors() {
        // FNV-1a 64 官方测试向量：空输入 = 偏移基；"a" = 0xaf63dc4c8601ec8c。
        assert_eq!(fnv1a64_init(), 0xcbf29ce484222325);
        assert_eq!(fnv1a64_step(fnv1a64_init(), b'a'), 0xaf63dc4c8601ec8c);
        // u64 推进确定性。
        let h1 = fnv1a64_u64(fnv1a64_init(), 0);
        let h2 = fnv1a64_u64(fnv1a64_init(), 0);
        assert_eq!(h1, h2);
    }

    #[test]
    fn ledger_cap_and_fingerprint_stability() {
        let mut l1 = EventLedger::new(4);
        let mut l2 = EventLedger::new(4);
        let mut i = 0;
        while i < 6 {
            let _ = l1.push(EventKind::SwitchBegin, 0, i, i as u64 * 8);
            let _ = l2.push(EventKind::SwitchBegin, 0, i, i as u64 * 8);
            i += 1;
        }
        assert_eq!(l1.kept_count(), 4);
        assert_eq!(l1.dropped(), 2);
        assert_eq!(l1.event_count(), 6);
        assert_eq!(l1.fingerprint(), l2.fingerprint());
        // 内容差异必须导致指纹差异。
        let _ = l1.push(EventKind::SwitchCommit, 0, 1, 999);
        assert_ne!(l1.fingerprint(), l2.fingerprint());
    }

    #[test]
    fn smoke_dual_run_determinism() {
        let seed = 0xA5A5_5A5A_1234_5678;
        assert_eq!(determinism_check(seed).unwrap(), true);
        // 不同种子 → 不同编排形态（至少 blocked/injected/fingerprint 之一不同；
        // 指纹必然不同，因为事件账与统计必然携带种子差分）。
        let a = orchestrate(seed).unwrap();
        let b = orchestrate(seed + 1).unwrap();
        assert_ne!(a.fingerprint, b.fingerprint);
    }

    #[test]
    fn orchestrator_invariants() {
        let s = orchestrate(0x0F0F_0F0F).unwrap();
        assert!(s.ap_count >= 3 && s.ap_count <= 8);
        assert_eq!(s.committed, s.ap_count);
        assert_eq!(s.back_to_bsp, s.ap_count);
        assert_eq!(s.scanned, s.ap_count);
        assert_eq!(s.isolated, 0);
        // 至少一部分 AP 抽到拦截演练（8 AP 规模下概率上必中；确定性种子钉死）。
        assert!(s.blocked + s.injected + s.gs_faults > 0);
        // 独立事件账收官必有注入演练 3 对 armed/caught 事件。
        assert!(s.event_count >= 6);
    }

    #[test]
    fn orchestrator_fingerprint_matches_determinism() {
        // 三种子全对拍（双跑一致铁律批量核）。
        assert_eq!(determinism_check(1).unwrap(), true);
        assert_eq!(determinism_check(0xDEAD_BEEF).unwrap(), true);
        assert_eq!(determinism_check(u64::MAX).unwrap(), true);
    }

    // ---- 诊断段自检 ----

    #[test]
    fn segment_codes_unique_and_in_segment() {
        segment_self_check().unwrap();
        // 闭集逐码核段位。
        let all = [
            XC_MAGIC_MISMATCH,
            XC_CPUID_MISMATCH,
            XC_SELF_PTR_MISMATCH,
            XC_NOT_REGISTERED,
            XC_DUP_CPU_ID,
            XC_REGISTRY_FULL,
            XC_BSP_SLOT,
            XC_STATE_INVALID,
            XC_DOUBLE_SWITCH,
            XC_LEDGER_FULL,
            XC_ISOLATE_FULL,
            XC_FINGERPRINT_MISMATCH,
            XC_INJECT_LEAK,
            XC_READBACK_MISMATCH,
            XC_DUP_BASE,
        ];
        assert_eq!(all.len(), 15);
        let base = PC_SEG_BASE as u32;
        let mut seen = [false; 16];
        let mut i = 0;
        while i < all.len() {
            assert!(all[i] > base && all[i] <= base + 0x0F);
            let slot = (all[i] - base) as usize;
            assert!(!seen[slot], "duplicate xc code {:#x}", all[i]);
            seen[slot] = true;
            i += 1;
        }
        // 0x1401~0x140F 全铺满。
        let mut k = 1;
        while k <= 0x0F {
            assert!(seen[k]);
            k += 1;
        }
    }

    // ---- 体检 ----

    #[test]
    fn health_clean_is_healthy() {
        let (reg, _aps, sw) = mk_env(2);
        let iso = IsolationLedger::new(ISOLATION_DEFAULT_CAP);
        let ledger = EventLedger::new(LEDGER_DEFAULT_CAP);
        let rep = health(&reg, &sw, &iso, &ledger, None);
        // 未演练切换时零失败零拦截零隔离 → 全绿。
        assert!(rep.healthy);
        assert!(rep.issues.len() == 1 && rep.issues[0].contains("体检通过"));
        assert!(rep.summary().contains("healthy=true"));
        assert_eq!(rep.registry_entries, 3);
        assert!(rep.bsp_registered);
    }

    #[test]
    fn health_flags_dirty_lines() {
        let mut a0 = mk_area(0, 1);
        a0.inject(InjectionKind::Magic).unwrap();
        let mut iso = IsolationLedger::new(ISOLATION_DEFAULT_CAP);
        let mut ledger = EventLedger::new(LEDGER_DEFAULT_CAP);
        let _ = verify_self_identity(&mut [a0], &mut iso, &mut ledger);

        let reg = CpuIdRegistry::new(REGISTRY_DEFAULT_CAP);
        let backend = RamGsBackend::new(ORCH_BSP_BASE);
        let sw: GsBaseSwitcher<RamGsBackend> = GsBaseSwitcher::new(backend, ORCH_BSP_BASE, 4);
        let rep = health(&reg, &sw, &iso, &sw.ledger(), None);
        assert!(!rep.healthy);
        assert!(rep.issues.iter().any(|s| s.contains("BSP")));
        assert!(rep.issues.iter().any(|s| s.contains("隔离")));
        // 空表注册表也要在体检里现形。
        assert_eq!(rep.registry_entries, 0);
        drop(reg);
    }

    #[test]
    fn switcher_records_fingerprint_stable() {
        let (reg, mut aps, mut sw) = mk_env(1);
        let f0 = sw.records_fingerprint();
        sw.begin_switch(&reg, &mut aps[0]).unwrap();
        let f1 = sw.records_fingerprint();
        sw.commit_switch(&mut aps[0]).unwrap();
        sw.switch_back_to_bsp().unwrap();
        let f2 = sw.records_fingerprint();
        assert_ne!(f0, f1);
        assert_ne!(f1, f2);
        // 双建同流程 → 同指纹。
        let (_reg2, mut aps2, mut sw2) = mk_env(1);
        sw2.begin_switch(&_reg2, &mut aps2[0]).unwrap();
        sw2.commit_switch(&mut aps2[0]).unwrap();
        sw2.switch_back_to_bsp().unwrap();
        assert_eq!(f2, sw2.records_fingerprint());
    }

    #[test]
    fn error_details_and_xcodes_are_accounted() {
        // 每个变体 detail 非空、xcode 映射闭合。
        let samples: [PerCpuError; 24] = [
            PerCpuError::MagicMismatch { expected: 1, found: 2 },
            PerCpuError::CpuIdMismatch { expected: 1, found: 2 },
            PerCpuError::SelfPtrMismatch { base: 1, found: 2 },
            PerCpuError::FooterMismatch { expected: 1, found: 2 },
            PerCpuError::NotRegistered { cpu_id: 1, base: 2 },
            PerCpuError::DuplicateCpuId { cpu_id: 1 },
            PerCpuError::DuplicateBase { base: 1 },
            PerCpuError::RegistryFull { cap: 1 },
            PerCpuError::BspSlotViolation { attempted_cpu: 0 },
            PerCpuError::InvalidState { need: 0, got: 1 },
            PerCpuError::DoubleSwitchConflict { pending_cpu: 1 },
            PerCpuError::LedgerFull { cap: 1 },
            PerCpuError::RecordsFull { cap: 1 },
            PerCpuError::IsolateLedgerFull { cap: 1 },
            PerCpuError::ReadbackMismatch { wrote: 1, read: 2 },
            PerCpuError::InjectionLeak { kind: 0, xcode: 1 },
            PerCpuError::BadOffset { off: 1, size: 4 },
            PerCpuError::BadAlign { value: 1, align: 16 },
            PerCpuError::BadStackConfig,
            PerCpuError::FingerprintMismatch { a: 1, b: 2 },
            PerCpuError::StatSlotRange { slot: 99 },
            PerCpuError::SegmentViolation { code: 0 },
            PerCpuError::InvariantBroken { site: 0 },
            PerCpuError::MagicMismatch { expected: 0, found: 0 },
        ];
        let mut i = 0;
        while i < samples.len() {
            assert!(!samples[i].detail().is_empty());
            i += 1;
        }
        // 账面码映射抽查。
        assert_eq!(
            PerCpuError::MagicMismatch { expected: 0, found: 0 }.xcode(),
            XC_MAGIC_MISMATCH
        );
        assert_eq!(
            PerCpuError::ReadbackMismatch { wrote: 0, read: 0 }.xcode(),
            XC_READBACK_MISMATCH
        );
        assert_eq!(PerCpuError::BadStackConfig.xcode(), 0);
    }

    #[test]
    fn version_and_kind_ordinals_pinned() {
        assert_eq!(PC_VERSION, "unx0344_percpu-v1");
        // 事件类别序号双向钉死（账面稳定序）。
        assert_eq!(event_kind_ordinal(EventKind::SwitchBegin), 0);
        assert_eq!(event_kind_ordinal(EventKind::HealthCheck), 13);
        assert_eq!(switch_phase_ordinal(SwitchPhase::Begin), 0);
        assert_eq!(switch_phase_ordinal(SwitchPhase::BackToBsp), 3);
        assert_eq!(InjectionKind::SelfPtr.code(), 2);
        assert_eq!(InjectionKind::SelfPtr.expected_xcode(), XC_SELF_PTR_MISMATCH);
        assert_eq!(InjectionKind::Magic.name(), "magic-corrupt");
        assert_eq!(SwitchState::Switching.name(), "Switching");
    }
}
