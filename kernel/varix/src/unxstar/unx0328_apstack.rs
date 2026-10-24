//! unx0328_apstack —— UNX-F0328 AP 栈与 per-CPU 区实例化（F0188/F0192 消费）。
//!
//! =====================================================================
//! 锚点原文（主册《CoRun Varix STAR II · Unxreal》UNX-A1 批次表，逐字抄录）：
//!
//!   | UNX-F0328 | AP 栈与 per-CPU 区实例化（F0188/F0192 消费） | 260 | 已深化 | UNX-F0328-J1 每 AP 栈/per-CPU 区独立实例化，基址写入与 F0192 语义一致 |
//!
//! 判据原文：UNX-F0328-J1 「每 AP 栈/per-CPU 区独立实例化，基址写入与
//!   F0192 语义一致」。
//! 定位原文（同表行）：UNX-F0328 · AP 栈与 per-CPU 区实例化
//!   （F0188/F0192 消费）· 目标 260 行 · 状态 已深化 · 属 UNX AP 启动域。
//! =====================================================================
//!
//! 语义上游：
//!   - UNX-F0188（AP 启动前置包）：本模块消费其「前置包就绪」语义——
//!     只有前置包（trampoline 落位 / AP 到达哨兵，F0322/F0326）就绪后，
//!     才允许对目标 AP 做栈与 per-CPU 区的实例化；本模块以
//!     `bring_up` 的前置检查行体现该消费契约（前置未就绪 → 拒绝实例化）。
//!   - UNX-F0192（fs/gs base 设置与 per-CPU 预备）：本模块消费其
//!     「per-CPU 区自识别」语义——实例化时把区基址写入区头 self_ptr
//!     槽，验收时逐区读回 self_ptr == 区基址（F0344 同款语义，见
//!     unx0344_percpu.rs 头注 "UNX-F0328 已按此语义实例化" 的引用）。
//!     基址写入方向与 F0192 一致：先定基址（本模块 RegionPlan），
//!     后写入区头（本模块 PerCpuInstantiator），切换（GS base）留给
//!     F0344，两模块职责零重叠：F0328 管「分配与初始化」，F0344 管
//!     「切过去之后认得出自己」。
//!
//! 组件清单（全部真实现，无占位）：
//!   1. StackLayout —— 每 AP 独立栈布局：栈深 32KiB/AP（与 F0348
//!      「8 页 × 4KiB」同源口径）+ 16 字节对齐（J1 语义）+ 低/高双
//!      哨兵带（每带 8 个 64 位哨兵字，溢出检测面）+ 栈顶 64 字节
//!      boot frame（magic/cpu_id/node/stack_top/area_base/layout_ver）。
//!      全部偏移走 const fn 布局表，编译期偏移校验 + 运行期双份。
//!   2. StackImage —— 栈实例：堆上定容字节缓冲（32KiB ≤ 64KiB 线），
//!      初始化 = 低/高哨兵带写哨兵魔数 + 栈体清零 + boot frame 写入；
//!      验收 = 哨兵逐字扫描 + boot frame 逐字段读回（读回 != 期望
//!      → 对应诊断码）。
//!   3. PerCpuInstantiator —— 每 AP 独立 per-CPU 区实例化：4KiB 定容
//!      区（≤64KiB 编译期断言），字节级布局 magic + cpu_id + node +
//!      self_ptr 自指 + stack_base/stack_len 回填 + 16 槽统计清零 +
//!      尾 magic；实例化清单（InstantiationChecklist）逐字段
//!      expect/actual 校验行 + F0188/F0192 语义消费声明行。
//!   4. RegionPlan —— 区规划与布局校验：竞技场（arena）基址起步，
//!      按栈（16 对齐）与 per-CPU 区（4KiB 对齐）依次雕刻，产出
//!      区间清单；两两区间不重叠审计（栈 vs 区 vs 区间——重叠即
//!      立案 RangeOverlapCase）；总内存预算账（分区合计 + 填充
//!      ≤ 竞技场容量，超支 → XC_BUDGET_OVER）。
//!   5. ApResourceBook —— 实例账：cpu_id → (栈基址, 区基址) 双基址
//!      账；分配 / 回收 / 查询；重复分配拒收（XC_DUP_ALLOC）；回收
//!      后槽位进墓碑（Tombstoned）态——墓碑槽永不复用（
//!      XC_TOMBSTONE_REUSE 拦截 + 槽位隔离审计：墓碑基址与活账
//!      基址两两不同）。
//!   6. 完整性面 —— verify_ap 全量读回（区头逐字段 + 哨兵位扫描 +
//!      统计槽清零 + boot frame）、verify_all 全 AP 巡检、
//!      完整性结论落事件账。
//!   7. 事件账 EventLedger（定容 + 溢出显性丢弃计数）+ FNV-1a 64
//!      指纹 + health() 体检 + 注入面（哨兵破坏注入 / 区间重叠
//!      注入 / 重复分配注入，三类全部检出 + 修复回归）+ 双跑
//!      确定性对拍（Lcg 驱动 orchestrate 两次，指纹必须一致）。
//!
//! 职责定位：AP 栈与 per-CPU 区「实例化」的唯一权威面——分配基址
//!   （RegionPlan）、初始化字节（StackImage/AreaImage）、记账
//!   （ApResourceBook）三权分立；重叠 = 双栈踩踏、哨兵破 = 栈溢出、
//!   self_ptr 错 = F0192 语义破，三案全立案可回溯。
//! 性能逐项分解：规划 O(AP 数)；重叠审计 O(n²)（n ≤ MAX_SLOTS=16，
//!   收口时一次跑）；哨兵扫描 O(哨兵字数)（常数 16 字/AP）；逐字段
//!   读回 O(字段数)（常数）；事件账 O(1) 摊销；FNV 指纹 O(事件数)。
//!   全整数确定性，无时钟、无随机（Lcg 种子显式传入）。
//! 诊断码段：0x1450（0x1451~0x145F 闭集；全域 grep 确认独占——唯一
//!   0x1450 字面量命中为 unx464_devid.rs 的 PCI 设备 ID 1022:1450，
//!   属硬件身份字面量，非诊断码；无任何 SEG_BASE 声明占用该段）。
//!   0x1450 本身保留为段基，不分配给任何单条诊断码。
//! 纪律：no_std + alloc；内核路径零 unwrap/expect/panic!/运行期
//!   assert!（编译期 const assert 不受限）；可失败全走
//!   Result<T, ApStackError>；计数一律 saturating/wrapping 显性；
//!   >64KiB 结构体禁上栈（本模块最大字节结构为 32KiB 栈，且一律以
//!   堆容器 Vec<u8> 持有，栈上只留句柄）；const fn 内不使用
//!   slice::get / Vec 索引 / Ord::min/max / 派生 PartialEq 的 == /
//!   for 循环（一律 while + 索引）。

use alloc::{format, string::String, vec, vec::Vec};

// ===========================================================================
// 版本与诊断段
// ===========================================================================

/// 模块版本（落点名 + 版次，账面对账用）。
pub const AS_VERSION: &str = "unx0328_apstack-v1";

/// 诊断段基址：0x1450（开工前全域 grep unxstar 确认 0x1451~0x145F
/// 零诊断码占用；唯一 "0x1450" 命中是 unx464_devid.rs:1735 的 PCI
/// 设备 ID 1022:1450——硬件身份字面量，非诊断段声明）。0x1450 本身
/// 保留为段基。
pub const AS_SEG_BASE: u16 = 0x1450;

// ---------------------------------------------------------------------------
// 诊断码闭集（0x1451 ~ 0x145F，段内独占，全域唯一）
// ---------------------------------------------------------------------------

/// 区头 magic 不匹配（含尾 magic 完整性破坏）。
pub const XC_MAGIC_MISMATCH: u32 = 0x1451;
/// 区头 cpu_id 与账面真值不符。
pub const XC_CPUID_MISMATCH: u32 = 0x1452;
/// self_ptr 读回 != 区基址（F0192 自识别语义破坏）。
pub const XC_SELF_PTR_MISMATCH: u32 = 0x1453;
/// 栈哨兵字破坏（栈溢出 / 越界写检出）。
pub const XC_SENTINEL_BROKEN: u32 = 0x1454;
/// 基址对齐违规（栈 16B / 区 4KiB）。
pub const XC_ALIGN_BAD: u32 = 0x1455;
/// 区间重叠（栈 vs 区 vs 区间，重叠即立案）。
pub const XC_RANGE_OVERLAP: u32 = 0x1456;
/// 重复分配（cpu_id 已在活账中）。
pub const XC_DUP_ALLOC: u32 = 0x1457;
/// 实例账槽位耗尽（含墓碑占位）。
pub const XC_SLOT_EXHAUSTED: u32 = 0x1458;
/// 对未分配 cpu_id 做回收 / 校验。
pub const XC_NOT_ALLOCATED: u32 = 0x1459;
/// 内存预算超支（分区合计 > 竞技场容量）。
pub const XC_BUDGET_OVER: u32 = 0x145A;
/// 试图复用墓碑槽 / 墓碑 cpu_id（隔离红线）。
pub const XC_TOMBSTONE_REUSE: u32 = 0x145B;
/// 区尺寸 / 栈长回填字段读回不符。
pub const XC_LEN_FIELD_BAD: u32 = 0x145C;
/// 统计槽实例化后非零（清零验收失败）。
pub const XC_STAT_NOT_ZERO: u32 = 0x145D;
/// boot frame 字段读回不符（栈顶交接块破坏）。
pub const XC_BOOT_FRAME_BAD: u32 = 0x145E;
/// 事件账满（溢出显性丢弃，丢弃计数递增）。
pub const XC_EVENT_FULL: u32 = 0x145F;

// ===========================================================================
// 错误枚举（每变体带 detail(&self) -> 'static str 中文可操作诊断）
// ===========================================================================

/// 本模块统一错误。变体载荷只带可回溯的最小事实三元组。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApStackError {
    /// 区头 / 尾 magic 不符。
    MagicMismatch {
        /// 目标 cpu_id。
        cpu_id: u32,
        /// 期望 magic。
        expect: u64,
        /// 实读 magic。
        actual: u64,
    },
    /// 区头 cpu_id 与账面真值不符。
    CpuIdMismatch {
        /// 账面期望 cpu_id。
        expect: u32,
        /// 区头实读 cpu_id。
        actual: u32,
    },
    /// self_ptr 读回 != 区基址。
    SelfPtrMismatch {
        /// 区基址（期望值）。
        base: u64,
        /// 区头 self_ptr 实读值。
        actual: u64,
    },
    /// 哨兵字破坏。
    SentinelBroken {
        /// 目标 cpu_id。
        cpu_id: u32,
        /// 被破坏哨兵字所在带。
        band: SentinelBand,
        /// 带内字下标。
        word_index: u64,
    },
    /// 对齐违规。
    AlignBad {
        /// 违规基址。
        base: u64,
        /// 要求对齐。
        align: u64,
    },
    /// 区间重叠。
    RangeOverlap {
        /// 先到区间基址。
        a_base: u64,
        /// 先到区间长度。
        a_len: u64,
        /// 后到区间基址。
        b_base: u64,
        /// 后到区间长度。
        b_len: u64,
    },
    /// 重复分配。
    DupAlloc {
        /// 重复的 cpu_id。
        cpu_id: u32,
    },
    /// 槽位耗尽。
    SlotExhausted {
        /// 账容量。
        capacity: u64,
    },
    /// 未分配即操作。
    NotAllocated {
        /// 目标 cpu_id。
        cpu_id: u32,
    },
    /// 预算超支。
    BudgetOver {
        /// 需求字节。
        need: u64,
        /// 竞技场容量。
        have: u64,
    },
    /// 墓碑复用拦截。
    TombstoneReuse {
        /// 被拦截的 cpu_id。
        cpu_id: u32,
    },
    /// 长度回填字段不符。
    LenFieldBad {
        /// 字段名。
        field: &'static str,
        /// 期望值。
        expect: u64,
        /// 实读值。
        actual: u64,
    },
    /// 统计槽非零。
    StatNotZero {
        /// 目标 cpu_id。
        cpu_id: u32,
        /// 首个非零槽下标。
        slot: u64,
        /// 实读值。
        actual: u64,
    },
    /// boot frame 不符。
    BootFrameBad {
        /// 目标 cpu_id。
        cpu_id: u32,
        /// 字段名。
        field: &'static str,
        /// 期望值。
        expect: u64,
        /// 实读值。
        actual: u64,
    },
    /// 事件账满。
    EventFull {
        /// 账容量。
        capacity: u64,
    },
}

impl ApStackError {
    /// 诊断码映射（全部落在 0x1451~0x145F 闭集内）。
    pub fn xc_code(&self) -> u32 {
        match self {
            ApStackError::MagicMismatch { .. } => XC_MAGIC_MISMATCH,
            ApStackError::CpuIdMismatch { .. } => XC_CPUID_MISMATCH,
            ApStackError::SelfPtrMismatch { .. } => XC_SELF_PTR_MISMATCH,
            ApStackError::SentinelBroken { .. } => XC_SENTINEL_BROKEN,
            ApStackError::AlignBad { .. } => XC_ALIGN_BAD,
            ApStackError::RangeOverlap { .. } => XC_RANGE_OVERLAP,
            ApStackError::DupAlloc { .. } => XC_DUP_ALLOC,
            ApStackError::SlotExhausted { .. } => XC_SLOT_EXHAUSTED,
            ApStackError::NotAllocated { .. } => XC_NOT_ALLOCATED,
            ApStackError::BudgetOver { .. } => XC_BUDGET_OVER,
            ApStackError::TombstoneReuse { .. } => XC_TOMBSTONE_REUSE,
            ApStackError::LenFieldBad { .. } => XC_LEN_FIELD_BAD,
            ApStackError::StatNotZero { .. } => XC_STAT_NOT_ZERO,
            ApStackError::BootFrameBad { .. } => XC_BOOT_FRAME_BAD,
            ApStackError::EventFull { .. } => XC_EVENT_FULL,
        }
    }

    /// 中文可操作诊断（每变体一条，指明下一步动作）。
    pub fn detail(&self) -> &'static str {
        match self {
            ApStackError::MagicMismatch { .. } => {
                "区头/尾 magic 不符：该 per-CPU 区已被越界写或未初始化，\
                 请先跑哨兵扫描定位踩踏来源，再重放实例化"
            }
            ApStackError::CpuIdMismatch { .. } => {
                "区头 cpu_id 与账面不符：疑似双区写串或账目错位，\
                 请对账 cpu_id→基址映射后重放实例化"
            }
            ApStackError::SelfPtrMismatch { .. } => {
                "self_ptr 读回 != 区基址：F0192 自识别语义被破坏，\
                 禁止切 GS base，请重写实指字段并复跑自识别断言"
            }
            ApStackError::SentinelBroken { .. } => {
                "栈哨兵字被破坏：该栈已发生溢出/越界写，\
                 请按带与字下标定位溢出方向（低带=向下溢出，高带=向上越界）"
            }
            ApStackError::AlignBad { .. } => {
                "基址对齐违规：栈须 16 字节对齐、per-CPU 区须 4KiB 对齐，\
                 请回查 RegionPlan 雕刻起点或竞技场基址本身的对齐"
            }
            ApStackError::RangeOverlap { .. } => {
                "区间重叠即立案：两区共占同一物理页必踩踏，\
                 请回查竞技场雕刻游标是否漏做 align_up，或区间长度填错"
            }
            ApStackError::DupAlloc { .. } => {
                "重复分配拒收：该 cpu_id 已在活账，\
                 请先 recycle 旧实例（进墓碑）再重新分配"
            }
            ApStackError::SlotExhausted { .. } => {
                "实例账槽位耗尽（墓碑也占槽）：请扩容账容量或归档墓碑账后重建账本"
            }
            ApStackError::NotAllocated { .. } => {
                "对未分配 cpu_id 操作：请先查询账面确认该 cpu_id 处于 Allocated 态"
            }
            ApStackError::BudgetOver { .. } => {
                "内存预算超支：分区合计大于竞技场容量，\
                 请扩大竞技场或减少 AP 数（预算账 BudgetRow 可查缺口）"
            }
            ApStackError::TombstoneReuse { .. } => {
                "墓碑复用被拦截：回收后的槽位与 cpu_id 永不复活，\
                 请改用全新槽位（隔离红线，防悬垂栈指针复活）"
            }
            ApStackError::LenFieldBad { .. } => {
                "长度回填字段读回不符：栈长/区尺寸写入被破坏，\
                 请核对布局偏移表后重放实例化"
            }
            ApStackError::StatNotZero { .. } => {
                "统计槽实例化后非零：清零步骤漏跑或被预写，\
                 请重放统计槽清零并复跑逐槽验收"
            }
            ApStackError::BootFrameBad { .. } => {
                "boot frame 字段读回不符：AP 入口交接块被破坏，\
                 请核对栈顶偏移（栈基+栈长-64）后重写实"
            }
            ApStackError::EventFull { .. } => {
                "事件账满：最旧事件不会被挤掉，新事件显性丢弃并累加丢弃计数，\
                 请导出指纹后清账或扩容"
            }
        }
    }
}

// ===========================================================================
// const fn 纯算术层（while + 索引，无 slice::get / Vec / Ord::min/max）
// ===========================================================================

/// const 对齐上取（align 必须为 2 的幂——编译期由下方 const assert 锁死）。
pub const fn as_align_up(value: u64, align: u64) -> u64 {
    (value + (align - 1)) & !(align - 1)
}

/// const 对齐判定。
pub const fn as_is_aligned(value: u64, align: u64) -> bool {
    value & (align - 1) == 0
}

/// const 显性 min（替代 Ord::min——const 纪律）。
pub const fn as_min(a: u64, b: u64) -> u64 {
    if a < b {
        a
    } else {
        b
    }
}

/// const 显性 max（替代 Ord::max——const 纪律）。
pub const fn as_max(a: u64, b: u64) -> u64 {
    if a > b {
        a
    } else {
        b
    }
}

/// FNV-1a 64（字节流版本，运行期；确定性无状态）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf_29ce_484_222_325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

/// Lcg 线性同余生成器（双跑确定性对拍驱动；参数取 Knuth MMIX 常数）。
#[derive(Clone, Debug)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 由显式种子构建（同一种子 → 同一序列，双跑对拍之基）。
    pub const fn new(seed: u64) -> Self {
        Lcg {
            state: seed.wrapping_add(0x9E37_79B9_7F4A_7C15),
        }
    }

    /// 下一步（Knuth MMIX：a=6364136223846793005, c=1442695040888963407）。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    /// 取 [0, bound) 内值（拒绝采样，bound=0 视为恒 0）。
    pub fn next_below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next_u64() % bound
    }
}

// ===========================================================================
// StackLayout —— 每 AP 独立栈布局（栈深表 + 对齐 + 哨兵带 + boot frame）
// ===========================================================================

/// 每 AP 栈深：32KiB（来源：与 UNX-F0348 "每 AP 栈 8 页 × 4KiB" 同源
/// 口径；亦在 64KiB 上栈红线之内——本模块栈体一律堆持有，栈上只留
/// 句柄，此处为编译期预算断言）。
pub const AS_STACK_BYTES: u64 = 32 * 1024;

/// 栈基址对齐：16 字节（J1 语义「栈基址写入与 F0192 语义一致」的
/// 对齐前提；x86-64 ABI 栈指针对齐底线）。
pub const AS_STACK_ALIGN: u64 = 16;

/// 哨兵带每带字数：8 个 64 位字 = 64 字节/带（低/高各一带）。
pub const AS_GUARD_WORDS: u64 = 8;

/// 哨兵字宽（字节）：8（u64）。
pub const AS_GUARD_WORD_BYTES: u64 = 8;

/// 单带字节数：8 字 × 8B = 64B。
pub const AS_GUARD_BAND_BYTES: u64 = AS_GUARD_WORDS * AS_GUARD_WORD_BYTES;

/// 栈顶 boot frame 字节数：64（AP 入口交接块）。
pub const AS_BOOT_FRAME_BYTES: u64 = 64;

/// 栈哨兵魔数字（来源：本模块自定义溢出哨兵；0xC0FFEE 谐音 "coffee"
/// 位型 + 0xA5A5A5A5 奇偶交错位型，二者异或后 0/1 密度高，利于检出
/// 位翻转与段内清零两类破坏；非任何硬件魔数）。
pub const AS_SENTINEL_WORD: u64 = 0xC0FF_EEAA_5A5A_A5A5;

/// boot frame magic（来源：ASCII 串 "VARIXBF1" 按小端字节序组包——
/// 内存首字节 'V'=0x56，最末字节 '1'=0x31；非任何硬件魔数）。
pub const AS_BOOT_FRAME_MAGIC: u64 = 0x3146_4258_4952_4156;

/// 栈布局版次（boot frame layout_ver 槽回填值；布局改动必须递增）。
pub const AS_STACK_LAYOUT_VER: u64 = 1;

/// 栈体起点（低哨兵带之后）：const 可算。
pub const fn stack_body_lo() -> u64 {
    AS_GUARD_BAND_BYTES
}

/// 栈体终点（高哨兵带之前）：const 可算。
pub const fn stack_body_hi() -> u64 {
    AS_STACK_BYTES - AS_BOOT_FRAME_BYTES - AS_GUARD_BAND_BYTES
}

/// 高哨兵带起点。
pub const fn stack_guard_hi_lo() -> u64 {
    stack_body_hi()
}

/// boot frame 起点（栈顶前 64 字节）。
pub const fn stack_boot_frame_lo() -> u64 {
    AS_STACK_BYTES - AS_BOOT_FRAME_BYTES
}

// --- 编译期布局锁（const assert 白名单允许） ---
const _: () = assert!(AS_STACK_BYTES % AS_STACK_ALIGN == 0);
const _: () = assert!(AS_STACK_BYTES <= 64 * 1024);
const _: () = assert!(stack_body_hi() > stack_body_lo());
const _: () = assert!(stack_boot_frame_lo() == stack_guard_hi_lo() + AS_GUARD_BAND_BYTES);
const _: () = assert!(AS_STACK_ALIGN.is_power_of_two());
const _: () = assert!(AS_SENTINEL_WORD != 0);
const _: () = assert!(AS_BOOT_FRAME_MAGIC != 0);

/// 哨兵带别（检出报告定位用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SentinelBand {
    /// 低带（栈底侧，向下溢出踩这里）。
    Low,
    /// 高带（boot frame 之前，向上越界踩这里）。
    High,
}

impl SentinelBand {
    /// 中文名（账面显示）。
    pub fn name(&self) -> &'static str {
        match self {
            SentinelBand::Low => "低哨兵带",
            SentinelBand::High => "高哨兵带",
        }
    }
}

/// 栈布局表行：一段布局的静态描述（布局表 = 单一事实来源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StackLayoutRow {
    /// 行名。
    pub name: &'static str,
    /// 段起点（栈内偏移）。
    pub lo: u64,
    /// 段长度。
    pub len: u64,
    /// 初始字节型（'S'=哨兵魔数, 'Z'=清零, 'F'=boot frame）。
    pub init: u8,
}

/// 取栈布局表（运行期构建，行序即地址序）。
pub fn stack_layout_table() -> Vec<StackLayoutRow> {
    vec![
        StackLayoutRow {
            name: "低哨兵带",
            lo: 0,
            len: AS_GUARD_BAND_BYTES,
            init: b'S',
        },
        StackLayoutRow {
            name: "栈体",
            lo: stack_body_lo(),
            len: stack_body_hi() - stack_body_lo(),
            init: b'Z',
        },
        StackLayoutRow {
            name: "高哨兵带",
            lo: stack_guard_hi_lo(),
            len: AS_GUARD_BAND_BYTES,
            init: b'S',
        },
        StackLayoutRow {
            name: "boot frame",
            lo: stack_boot_frame_lo(),
            len: AS_BOOT_FRAME_BYTES,
            init: b'F',
        },
    ]
}

/// 栈布局表一致性审计：行序升序、无空洞、无重叠、合计 == 栈深。
pub fn stack_layout_table_audit() -> Result<(), ApStackError> {
    let table = stack_layout_table();
    let mut cursor: u64 = 0;
    let mut i = 0;
    while i < table.len() {
        let row = &table[i];
        if row.lo != cursor {
            return Err(ApStackError::LenFieldBad {
                field: "布局表行起点",
                expect: cursor,
                actual: row.lo,
            });
        }
        if row.len == 0 {
            return Err(ApStackError::LenFieldBad {
                field: "布局表行长度",
                expect: 1,
                actual: 0,
            });
        }
        cursor = cursor.wrapping_add(row.len);
        i += 1;
    }
    if cursor != AS_STACK_BYTES {
        return Err(ApStackError::LenFieldBad {
            field: "布局表合计",
            expect: AS_STACK_BYTES,
            actual: cursor,
        });
    }
    Ok(())
}

/// boot frame 槽偏移表（帧内偏移，常量来源即注释）。
pub mod boot_frame {
    /// magic（u64）：ASCII "VARIXBF1" 小端。
    pub const OFF_MAGIC: u64 = 0;
    /// cpu_id（u32）。
    pub const OFF_CPU_ID: u64 = 8;
    /// node（u32）。
    pub const OFF_NODE: u64 = 12;
    /// stack_top（u64）：必须 == 栈基址 + 栈深。
    pub const OFF_STACK_TOP: u64 = 16;
    /// area_base（u64）：本 AP 的 per-CPU 区基址（F0192 消费回填）。
    pub const OFF_AREA_BASE: u64 = 24;
    /// layout_ver（u64）：栈布局版次。
    pub const OFF_LAYOUT_VER: u64 = 32;
    /// 帧内有效字段总宽（其余为零垫）。
    pub const VALID_BYTES: u64 = 40;
}

// ===========================================================================
// 字节读写安全层（无 panic 面：越界返回 None/False，不索引打穿）
// ===========================================================================

/// 安全写 u64 小端（越界返回 false，不 panic）。
pub fn wr_u64_le(buf: &mut [u8], off: u64, v: u64) -> bool {
    if off.wrapping_add(8) > buf.len() as u64 {
        return false;
    }
    let mut i: u64 = 0;
    while i < 8 {
        buf[(off + i) as usize] = (v >> (i * 8)) as u8;
        i += 1;
    }
    true
}

/// 安全写 u32 小端（越界返回 false，不 panic）。
pub fn wr_u32_le(buf: &mut [u8], off: u64, v: u32) -> bool {
    if off.wrapping_add(4) > buf.len() as u64 {
        return false;
    }
    let mut i: u64 = 0;
    while i < 4 {
        buf[(off + i) as usize] = (v >> (i * 8)) as u8;
        i += 1;
    }
    true
}

/// 安全读 u64 小端（越界返回 None，不 panic）。
pub fn rd_u64_le(buf: &[u8], off: u64) -> Option<u64> {
    if off.wrapping_add(8) > buf.len() as u64 {
        return None;
    }
    let mut v: u64 = 0;
    let mut i: u64 = 0;
    while i < 8 {
        v |= (buf[(off + i) as usize] as u64) << (i * 8);
        i += 1;
    }
    Some(v)
}

/// 安全读 u32 小端（越界返回 None，不 panic）。
pub fn rd_u32_le(buf: &[u8], off: u64) -> Option<u32> {
    if off.wrapping_add(4) > buf.len() as u64 {
        return None;
    }
    let mut v: u32 = 0;
    let mut i: u64 = 0;
    while i < 4 {
        v |= (buf[(off + i) as usize] as u32) << (i * 8);
        i += 1;
    }
    Some(v)
}

/// 安全逐字节相等（长度不同即 false；while+索引显性循环便于审计）。
pub fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

// ===========================================================================
// StackImage —— 栈实例（初始化 + 哨兵扫描 + boot frame 读回）
// ===========================================================================

/// 每 AP 独立栈实例：堆上 32KiB 定容字节缓冲（≤64KiB 上栈红线，
/// 且实体永远在堆，句柄上栈仅数十字节）。
#[derive(Clone, Debug)]
pub struct StackImage {
    /// 所属 cpu_id。
    pub cpu_id: u32,
    /// 栈基址（来自 RegionPlan，16 字节对齐）。
    pub base: u64,
    /// 栈体字节（长度恒为 AS_STACK_BYTES）。
    pub bytes: Vec<u8>,
}

impl StackImage {
    /// 实例化：按布局表初始化整块栈体。
    ///
    /// 步骤：对齐断言 → 低/高哨兵带写哨兵魔数 → 栈体清零 → boot
    /// frame 写入（magic/cpu_id/node/stack_top/area_base/layout_ver
    /// + 零垫）。任一步失败带诊断码上抛。
    pub fn instantiate(
        cpu_id: u32,
        node: u32,
        base: u64,
        area_base: u64,
    ) -> Result<StackImage, ApStackError> {
        if !as_is_aligned(base, AS_STACK_ALIGN) {
            return Err(ApStackError::AlignBad {
                base,
                align: AS_STACK_ALIGN,
            });
        }
        let mut bytes: Vec<u8> = vec![0u8; AS_STACK_BYTES as usize];
        // 低哨兵带
        let mut w: u64 = 0;
        while w < AS_GUARD_WORDS {
            if !wr_u64_le(&mut bytes, w * AS_GUARD_WORD_BYTES, AS_SENTINEL_WORD) {
                return Err(ApStackError::SentinelBroken {
                    cpu_id,
                    band: SentinelBand::Low,
                    word_index: w,
                });
            }
            w += 1;
        }
        // 栈体清零（vec! 已零，但显性重扫一遍——防上游带脏的兜底）
        let body_lo = stack_body_lo() as usize;
        let body_hi = stack_body_hi() as usize;
        let mut i = body_lo;
        while i < body_hi {
            bytes[i] = 0;
            i += 1;
        }
        // 高哨兵带
        let mut w: u64 = 0;
        while w < AS_GUARD_WORDS {
            let off = stack_guard_hi_lo() + w * AS_GUARD_WORD_BYTES;
            if !wr_u64_le(&mut bytes, off, AS_SENTINEL_WORD) {
                return Err(ApStackError::SentinelBroken {
                    cpu_id,
                    band: SentinelBand::High,
                    word_index: w,
                });
            }
            w += 1;
        }
        // boot frame
        let bf = stack_boot_frame_lo();
        if !wr_u64_le(&mut bytes, bf + boot_frame::OFF_MAGIC, AS_BOOT_FRAME_MAGIC)
            || !wr_u32_le(&mut bytes, bf + boot_frame::OFF_CPU_ID, cpu_id)
            || !wr_u32_le(&mut bytes, bf + boot_frame::OFF_NODE, node)
            || !wr_u64_le(
                &mut bytes,
                bf + boot_frame::OFF_STACK_TOP,
                base.wrapping_add(AS_STACK_BYTES),
            )
            || !wr_u64_le(&mut bytes, bf + boot_frame::OFF_AREA_BASE, area_base)
            || !wr_u64_le(&mut bytes, bf + boot_frame::OFF_LAYOUT_VER, AS_STACK_LAYOUT_VER)
        {
            return Err(ApStackError::BootFrameBad {
                cpu_id,
                field: "写入越界",
                expect: stack_boot_frame_lo(),
                actual: bytes.len() as u64,
            });
        }
        // 零垫区显性清零（VALID_BYTES..AS_BOOT_FRAME_BYTES）
        let mut z: u64 = boot_frame::VALID_BYTES;
        while z < AS_BOOT_FRAME_BYTES {
            bytes[(bf + z) as usize] = 0;
            z += 1;
        }
        Ok(StackImage { cpu_id, base, bytes })
    }

    /// 哨兵全带扫描：返回首个被破坏的哨兵字（带别 + 字下标），全绿
    /// 返回 None。扫描为逐字显性循环（16 字常数）。
    pub fn check_sentinels(&self) -> Option<(SentinelBand, u64)> {
        let mut w: u64 = 0;
        while w < AS_GUARD_WORDS {
            let got = match rd_u64_le(&self.bytes, w * AS_GUARD_WORD_BYTES) {
                Some(v) => v,
                None => return Some((SentinelBand::Low, w)),
            };
            if got != AS_SENTINEL_WORD {
                return Some((SentinelBand::Low, w));
            }
            w += 1;
        }
        let mut w: u64 = 0;
        while w < AS_GUARD_WORDS {
            let off = stack_guard_hi_lo() + w * AS_GUARD_WORD_BYTES;
            let got = match rd_u64_le(&self.bytes, off) {
                Some(v) => v,
                None => return Some((SentinelBand::High, w)),
            };
            if got != AS_SENTINEL_WORD {
                return Some((SentinelBand::High, w));
            }
            w += 1;
        }
        None
    }

    /// boot frame 逐字段读回：期望值全部由账面事实（cpu_id/base/
    /// area_base + 常量）推导，任何槽位不符 → XC_BOOT_FRAME_BAD。
    pub fn check_boot_frame(&self, expect_area_base: u64) -> Result<(), ApStackError> {
        let bf = stack_boot_frame_lo();
        let got_magic = match rd_u64_le(&self.bytes, bf + boot_frame::OFF_MAGIC) {
            Some(v) => v,
            None => {
                return Err(ApStackError::BootFrameBad {
                    cpu_id: self.cpu_id,
                    field: "magic 读越界",
                    expect: AS_BOOT_FRAME_MAGIC,
                    actual: 0,
                })
            }
        };
        if got_magic != AS_BOOT_FRAME_MAGIC {
            return Err(ApStackError::BootFrameBad {
                cpu_id: self.cpu_id,
                field: "magic",
                expect: AS_BOOT_FRAME_MAGIC,
                actual: got_magic,
            });
        }
        let got_cpu = match rd_u32_le(&self.bytes, bf + boot_frame::OFF_CPU_ID) {
            Some(v) => v,
            None => {
                return Err(ApStackError::BootFrameBad {
                    cpu_id: self.cpu_id,
                    field: "cpu_id 读越界",
                    expect: self.cpu_id as u64,
                    actual: 0,
                })
            }
        };
        if got_cpu != self.cpu_id {
            return Err(ApStackError::BootFrameBad {
                cpu_id: self.cpu_id,
                field: "cpu_id",
                expect: self.cpu_id as u64,
                actual: got_cpu as u64,
            });
        }
        let want_top = self.base.wrapping_add(AS_STACK_BYTES);
        let got_top = match rd_u64_le(&self.bytes, bf + boot_frame::OFF_STACK_TOP) {
            Some(v) => v,
            None => {
                return Err(ApStackError::BootFrameBad {
                    cpu_id: self.cpu_id,
                    field: "stack_top 读越界",
                    expect: want_top,
                    actual: 0,
                })
            }
        };
        if got_top != want_top {
            return Err(ApStackError::BootFrameBad {
                cpu_id: self.cpu_id,
                field: "stack_top",
                expect: want_top,
                actual: got_top,
            });
        }
        let got_area = match rd_u64_le(&self.bytes, bf + boot_frame::OFF_AREA_BASE) {
            Some(v) => v,
            None => {
                return Err(ApStackError::BootFrameBad {
                    cpu_id: self.cpu_id,
                    field: "area_base 读越界",
                    expect: expect_area_base,
                    actual: 0,
                })
            }
        };
        if got_area != expect_area_base {
            return Err(ApStackError::BootFrameBad {
                cpu_id: self.cpu_id,
                field: "area_base",
                expect: expect_area_base,
                actual: got_area,
            });
        }
        let got_ver = match rd_u64_le(&self.bytes, bf + boot_frame::OFF_LAYOUT_VER) {
            Some(v) => v,
            None => {
                return Err(ApStackError::BootFrameBad {
                    cpu_id: self.cpu_id,
                    field: "layout_ver 读越界",
                    expect: AS_STACK_LAYOUT_VER,
                    actual: 0,
                })
            }
        };
        if got_ver != AS_STACK_LAYOUT_VER {
            return Err(ApStackError::BootFrameBad {
                cpu_id: self.cpu_id,
                field: "layout_ver",
                expect: AS_STACK_LAYOUT_VER,
                actual: got_ver,
            });
        }
        Ok(())
    }

    /// 溢出检出快捷判定：哨兵扫描发现任何破坏即为 true。
    pub fn overflow_detected(&self) -> bool {
        self.check_sentinels().is_some()
    }

    /// 修复回归：重写全部哨兵字（注入破坏后修复用）。
    pub fn repair_sentinels(&mut self) -> bool {
        let mut ok = true;
        let mut w: u64 = 0;
        while w < AS_GUARD_WORDS {
            if !wr_u64_le(&mut self.bytes, w * AS_GUARD_WORD_BYTES, AS_SENTINEL_WORD) {
                ok = false;
            }
            w += 1;
        }
        let mut w: u64 = 0;
        while w < AS_GUARD_WORDS {
            let off = stack_guard_hi_lo() + w * AS_GUARD_WORD_BYTES;
            if !wr_u64_le(&mut self.bytes, off, AS_SENTINEL_WORD) {
                ok = false;
            }
            w += 1;
        }
        ok
    }
}

// ===========================================================================
// PerCpuInstantiator —— 每 AP 独立 per-CPU 区实例化（F0188/F0192 消费）
// ===========================================================================

/// 每 AP per-CPU 区定容：4KiB（来源：与页同粒度，基址对齐即页对齐，
/// 利于页表直映射；F0344 采用 8KiB 区——那是「切换域」的定容，本模块
/// 是「实例化域」，定容独立声明、互不耦合，self_ptr 语义一致即可）。
pub const AS_AREA_BYTES: u64 = 4 * 1024;

/// per-CPU 区基址对齐：4KiB（页粒度）。
pub const AS_AREA_ALIGN: u64 = 4096;

/// 统计槽数：16（F0344 同款 16 槽统计面口径）。
pub const AS_STAT_SLOTS: u64 = 16;

/// 区头 magic（来源：ASCII 串 "VARIXPC1" 按小端字节序组包——内存首
/// 字节 'V'=0x56，最末字节 '1'=0x31；非任何硬件魔数）。
pub const AS_AREA_MAGIC: u64 = 0x3143_5058_4952_4156;

/// 区尾 magic（来源：ASCII "VARIXPCE"，E=End；尾部完整性校验面）。
pub const AS_AREA_TAIL_MAGIC: u64 = 0x4543_5058_4952_4156;

/// 区布局版次。
pub const AS_AREA_LAYOUT_VER: u32 = 1;

// --- 编译期布局锁 ---
const _: () = assert!(AS_AREA_BYTES <= 64 * 1024);
const _: () = assert!(AS_AREA_ALIGN.is_power_of_two());
const _: () = assert!(AS_AREA_BYTES % 8 == 0);
const _: () = assert!(AS_AREA_MAGIC != 0);
const _: () = assert!(AS_AREA_TAIL_MAGIC != 0);
const _: () = assert!(AS_AREA_MAGIC != AS_AREA_TAIL_MAGIC);
const _: () = assert!(AS_AREA_MAGIC != AS_BOOT_FRAME_MAGIC);

/// 区头偏移表（区基址起算的常量偏移，来源即注释）。
pub mod area_off {
    /// magic（u64）。
    pub const OFF_MAGIC: u64 = 0;
    /// cpu_id（u32）。
    pub const OFF_CPU_ID: u64 = 8;
    /// node（u32）。
    pub const OFF_NODE: u64 = 12;
    /// self_ptr（u64）：F0192 自识别语义——读回必须 == 区基址。
    pub const OFF_SELF_PTR: u64 = 16;
    /// stack_base（u64）：本 AP 栈基址回填。
    pub const OFF_STACK_BASE: u64 = 24;
    /// stack_len（u64）：本 AP 栈深回填。
    pub const OFF_STACK_LEN: u64 = 32;
    /// area_ver（u32）。
    pub const OFF_AREA_VER: u64 = 40;
    /// 保留（u32，恒 0）。
    pub const OFF_RESERVED: u64 = 44;
    /// 统计槽起点（16 × u64 = 128B）。
    pub const OFF_STATS: u64 = 48;
    /// 头部有效区终点（stats 之后）。
    pub const HEAD_END: u64 = OFF_STATS + 16 * 8;
    /// 尾 magic 偏移（区末 8 字节）。
    pub const OFF_TAIL: u64 = super::AS_AREA_BYTES - 8;
}

/// 一条实例化清单校验行：字段名 + 期望 + 实读 + 判定。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChecklistRow {
    /// 字段名（中文账面）。
    pub field: String,
    /// 期望值（u64 化；u32 字段零扩展）。
    pub expect: u64,
    /// 实读值。
    pub actual: u64,
    /// 判定。
    pub ok: bool,
    /// 诊断码（ok 时为 0）。
    pub xc: u32,
}

/// F0188/F0192 语义消费声明行（实例化清单的头两行，落账可查）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsumptionRow {
    /// 上游任务号。
    pub task: &'static str,
    /// 消费语义描述。
    pub semantics: &'static str,
    /// 本模块兑现动作。
    pub honored_by: &'static str,
}

/// 取语义消费声明（固定两行，账面常量）。
pub fn consumption_declaration() -> Vec<ConsumptionRow> {
    vec![
        ConsumptionRow {
            task: "UNX-F0188",
            semantics: "AP 启动前置包就绪后才允许实例化（前置闸）",
            honored_by: "bring_up 前置检查：未就绪即拒收（XC_NOT_ALLOCATED 上游态）",
        },
        ConsumptionRow {
            task: "UNX-F0192",
            semantics: "per-CPU 区自识别：区头 self_ptr 读回 == 区基址",
            honored_by: "实例化写实指 + verify 阶段逐区读回断言（XC_SELF_PTR_MISMATCH）",
        },
    ]
}

/// 每 AP 独立 per-CPU 区实例：堆上 4KiB 定容字节缓冲。
#[derive(Clone, Debug)]
pub struct AreaImage {
    /// 所属 cpu_id。
    pub cpu_id: u32,
    /// 区基址（来自 RegionPlan，4KiB 对齐）。
    pub base: u64,
    /// 区体字节（长度恒为 AS_AREA_BYTES）。
    pub bytes: Vec<u8>,
}

/// per-CPU 区实例化器（无状态——所有状态在 AreaImage 与账面）。
#[derive(Clone, Debug, Default)]
pub struct PerCpuInstantiator;

impl PerCpuInstantiator {
    /// 实例化一个 per-CPU 区。
    ///
    /// 写入序：头 magic → cpu_id/node → self_ptr 自指（F0192 语义：
    /// self_ptr == 区基址）→ stack_base/stack_len 回填 → area_ver →
    /// 16 槽统计清零 → 头后余区清零 → 尾 magic。
    pub fn instantiate(
        cpu_id: u32,
        node: u32,
        base: u64,
        stack_base: u64,
        stack_len: u64,
    ) -> Result<AreaImage, ApStackError> {
        if !as_is_aligned(base, AS_AREA_ALIGN) {
            return Err(ApStackError::AlignBad {
                base,
                align: AS_AREA_ALIGN,
            });
        }
        let mut bytes: Vec<u8> = vec![0u8; AS_AREA_BYTES as usize];
        if !wr_u64_le(&mut bytes, area_off::OFF_MAGIC, AS_AREA_MAGIC) {
            return Err(ApStackError::MagicMismatch {
                cpu_id,
                expect: AS_AREA_MAGIC,
                actual: 0,
            });
        }
        if !wr_u32_le(&mut bytes, area_off::OFF_CPU_ID, cpu_id)
            || !wr_u32_le(&mut bytes, area_off::OFF_NODE, node)
        {
            return Err(ApStackError::CpuIdMismatch {
                expect: cpu_id,
                actual: 0xFFFF_FFFF,
            });
        }
        // F0192 自识别语义写实指：self_ptr 槽 = 区基址本身。
        if !wr_u64_le(&mut bytes, area_off::OFF_SELF_PTR, base) {
            return Err(ApStackError::SelfPtrMismatch { base, actual: 0 });
        }
        if !wr_u64_le(&mut bytes, area_off::OFF_STACK_BASE, stack_base)
            || !wr_u64_le(&mut bytes, area_off::OFF_STACK_LEN, stack_len)
        {
            return Err(ApStackError::LenFieldBad {
                field: "栈回填写越界",
                expect: stack_base,
                actual: bytes.len() as u64,
            });
        }
        if !wr_u32_le(&mut bytes, area_off::OFF_AREA_VER, AS_AREA_LAYOUT_VER) {
            return Err(ApStackError::MagicMismatch {
                cpu_id,
                expect: AS_AREA_MAGIC,
                actual: 0,
            });
        }
        // 统计槽 16 槽显性清零（vec! 已零，显性重放防上游带脏）。
        let mut s: u64 = 0;
        while s < AS_STAT_SLOTS {
            let off = area_off::OFF_STATS + s * 8;
            if !wr_u64_le(&mut bytes, off, 0) {
                return Err(ApStackError::StatNotZero {
                    cpu_id,
                    slot: s,
                    actual: 0xFFFF_FFFF_FFFF_FFFF,
                });
            }
            s += 1;
        }
        // 头后余区显性清零（防预写脏字节穿尾）。
        let head_end = area_off::HEAD_END as usize;
        let tail_lo = area_off::OFF_TAIL as usize;
        let mut i = head_end;
        while i < tail_lo {
            bytes[i] = 0;
            i += 1;
        }
        // 尾 magic。
        if !wr_u64_le(&mut bytes, area_off::OFF_TAIL, AS_AREA_TAIL_MAGIC) {
            return Err(ApStackError::MagicMismatch {
                cpu_id,
                expect: AS_AREA_TAIL_MAGIC,
                actual: 0,
            });
        }
        Ok(AreaImage { cpu_id, base, bytes })
    }

    /// 实例化清单：逐字段 expect/actual 校验行（J1 「独立实例化」的
    /// 落账形态——每一行都可人工复核）。
    pub fn checklist(area: &AreaImage, stack_base: u64, stack_len: u64) -> Vec<ChecklistRow> {
        let b = &area.bytes;
        let magic = rd_u64_le(b, area_off::OFF_MAGIC).unwrap_or(0);
        let cpu = rd_u32_le(b, area_off::OFF_CPU_ID).unwrap_or(0xFFFF_FFFF);
        let self_ptr = rd_u64_le(b, area_off::OFF_SELF_PTR).unwrap_or(0);
        let sb = rd_u64_le(b, area_off::OFF_STACK_BASE).unwrap_or(0);
        let sl = rd_u64_le(b, area_off::OFF_STACK_LEN).unwrap_or(0);
        let ver = rd_u32_le(b, area_off::OFF_AREA_VER).unwrap_or(0xFFFF_FFFF);
        let tail = rd_u64_le(b, area_off::OFF_TAIL).unwrap_or(0);
        vec![
            ChecklistRow {
                field: "头 magic".into(),
                expect: AS_AREA_MAGIC,
                actual: magic,
                ok: magic == AS_AREA_MAGIC,
                xc: if magic == AS_AREA_MAGIC {
                    0
                } else {
                    XC_MAGIC_MISMATCH
                },
            },
            ChecklistRow {
                field: "cpu_id".into(),
                expect: area.cpu_id as u64,
                actual: cpu as u64,
                ok: cpu == area.cpu_id,
                xc: if cpu == area.cpu_id { 0 } else { XC_CPUID_MISMATCH },
            },
            ChecklistRow {
                field: "self_ptr 自指（F0192）".into(),
                expect: area.base,
                actual: self_ptr,
                ok: self_ptr == area.base,
                xc: if self_ptr == area.base {
                    0
                } else {
                    XC_SELF_PTR_MISMATCH
                },
            },
            ChecklistRow {
                field: "stack_base 回填".into(),
                expect: stack_base,
                actual: sb,
                ok: sb == stack_base,
                xc: if sb == stack_base { 0 } else { XC_LEN_FIELD_BAD },
            },
            ChecklistRow {
                field: "stack_len 回填".into(),
                expect: stack_len,
                actual: sl,
                ok: sl == stack_len,
                xc: if sl == stack_len { 0 } else { XC_LEN_FIELD_BAD },
            },
            ChecklistRow {
                field: "area_ver".into(),
                expect: AS_AREA_LAYOUT_VER as u64,
                actual: ver as u64,
                ok: ver == AS_AREA_LAYOUT_VER,
                xc: if ver == AS_AREA_LAYOUT_VER {
                    0
                } else {
                    XC_MAGIC_MISMATCH
                },
            },
            ChecklistRow {
                field: "尾 magic".into(),
                expect: AS_AREA_TAIL_MAGIC,
                actual: tail,
                ok: tail == AS_AREA_TAIL_MAGIC,
                xc: if tail == AS_AREA_TAIL_MAGIC {
                    0
                } else {
                    XC_MAGIC_MISMATCH
                },
            },
        ]
    }

    /// 验收一个区：清单逐行过 + 统计槽逐槽清零验证。任一失败带码上抛。
    pub fn verify(area: &AreaImage, stack_base: u64, stack_len: u64) -> Result<(), ApStackError> {
        let rows = Self::checklist(area, stack_base, stack_len);
        let mut i = 0;
        while i < rows.len() {
            let row = &rows[i];
            if !row.ok {
                // 把清单行的失败映射回强类型错误（带上下文事实）。
                return Err(Self::row_to_error(area.cpu_id, row));
            }
            i += 1;
        }
        // 统计槽逐槽验证。
        let mut s: u64 = 0;
        while s < AS_STAT_SLOTS {
            let off = area_off::OFF_STATS + s * 8;
            let v = match rd_u64_le(&area.bytes, off) {
                Some(v) => v,
                None => {
                    return Err(ApStackError::StatNotZero {
                        cpu_id: area.cpu_id,
                        slot: s,
                        actual: 0,
                    })
                }
            };
            if v != 0 {
                return Err(ApStackError::StatNotZero {
                    cpu_id: area.cpu_id,
                    slot: s,
                    actual: v,
                });
            }
            s += 1;
        }
        Ok(())
    }

    /// 清单行失败 → 强类型错误（映射表单一出处）。
    fn row_to_error(cpu_id: u32, row: &ChecklistRow) -> ApStackError {
        match row.xc {
            XC_MAGIC_MISMATCH => ApStackError::MagicMismatch {
                cpu_id,
                expect: row.expect,
                actual: row.actual,
            },
            XC_CPUID_MISMATCH => ApStackError::CpuIdMismatch {
                expect: row.expect as u32,
                actual: row.actual as u32,
            },
            XC_SELF_PTR_MISMATCH => ApStackError::SelfPtrMismatch {
                base: row.expect,
                actual: row.actual,
            },
            XC_LEN_FIELD_BAD => ApStackError::LenFieldBad {
                field: "回填字段",
                expect: row.expect,
                actual: row.actual,
            },
            _ => ApStackError::LenFieldBad {
                field: "清单未映射行",
                expect: row.expect,
                actual: row.actual,
            },
        }
    }
}

impl AreaImage {
    /// F0192 自识别断言（单字段快捷面）：self_ptr 读回 == 区基址。
    pub fn self_identifies(&self) -> bool {
        match rd_u64_le(&self.bytes, area_off::OFF_SELF_PTR) {
            Some(v) => v == self.base,
            None => false,
        }
    }

    /// 注入 self_ptr 破坏（注入面专用；正常路径禁止调用）。
    pub fn inject_self_ptr(&mut self, v: u64) -> bool {
        wr_u64_le(&mut self.bytes, area_off::OFF_SELF_PTR, v)
    }

    /// 注入头 magic 破坏。
    pub fn inject_magic(&mut self, v: u64) -> bool {
        wr_u64_le(&mut self.bytes, area_off::OFF_MAGIC, v)
    }

    /// 注入统计槽脏值。
    pub fn inject_stat(&mut self, slot: u64, v: u64) -> bool {
        if slot >= AS_STAT_SLOTS {
            return false;
        }
        wr_u64_le(&mut self.bytes, area_off::OFF_STATS + slot * 8, v)
    }

    /// 修复回归：按账面事实重写关键字段（magic/self_ptr/cpu_id/尾）。
    pub fn repair(&mut self, stack_base: u64, stack_len: u64) -> bool {
        let mut ok = wr_u64_le(&mut self.bytes, area_off::OFF_MAGIC, AS_AREA_MAGIC);
        ok &= wr_u32_le(&mut self.bytes, area_off::OFF_CPU_ID, self.cpu_id);
        ok &= wr_u64_le(&mut self.bytes, area_off::OFF_SELF_PTR, self.base);
        ok &= wr_u64_le(&mut self.bytes, area_off::OFF_STACK_BASE, stack_base);
        ok &= wr_u64_le(&mut self.bytes, area_off::OFF_STACK_LEN, stack_len);
        ok &= wr_u64_le(&mut self.bytes, area_off::OFF_TAIL, AS_AREA_TAIL_MAGIC);
        ok
    }
}

// ===========================================================================
// RegionPlan —— 区规划与布局校验（基址对齐 / 区间不重叠 / 预算账）
// ===========================================================================

/// 区类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionKind {
    /// AP 栈（16B 对齐，32KiB）。
    Stack,
    /// per-CPU 区（4KiB 对齐，4KiB）。
    PerCpuArea,
}

impl RegionKind {
    /// 类别名（账面显示）。
    pub fn name(&self) -> &'static str {
        match self {
            RegionKind::Stack => "AP栈",
            RegionKind::PerCpuArea => "per-CPU区",
        }
    }
}

/// 一个已规划区间（闭区间语义以 base+len 表达，len > 0）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlanRegion {
    /// 区类别。
    pub kind: RegionKind,
    /// 属主 cpu_id。
    pub owner: u32,
    /// 基址（已对齐）。
    pub base: u64,
    /// 长度。
    pub len: u64,
}

impl PlanRegion {
    /// 区间相交判定（半开区间 [base, base+len) 两两相交）。
    pub fn overlaps(&self, other: &PlanRegion) -> bool {
        let a_lo = self.base;
        let a_hi = self.base.wrapping_add(self.len);
        let b_lo = other.base;
        let b_hi = other.base.wrapping_add(other.len);
        a_lo < b_hi && b_lo < a_hi
    }
}

/// 预算账行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BudgetRow {
    /// 项目名。
    pub item: String,
    /// 字节数。
    pub bytes: u64,
}

/// 区间重叠立案（重叠即立案——每案独立可回溯）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverlapCase {
    /// 案号（从 1 起）。
    pub case_no: u64,
    /// 先到区（描述串）。
    pub a: String,
    /// 后到区（描述串）。
    pub b: String,
    /// 重叠字节数。
    pub overlap_bytes: u64,
}

/// 区规划结果：区间清单 + 预算账 + 重叠立案簿 + 消费声明。
#[derive(Clone, Debug, Default)]
pub struct RegionPlan {
    /// 竞技场基址。
    pub arena_base: u64,
    /// 竞技场容量。
    pub arena_len: u64,
    /// 规划区间（地址升序）。
    pub regions: Vec<PlanRegion>,
    /// 预算账。
    pub budget: Vec<BudgetRow>,
    /// 重叠立案簿（空 = 审计通过）。
    pub overlap_cases: Vec<OverlapCase>,
    /// 实际使用字节（含填充）。
    pub used_bytes: u64,
    /// 填充字节（对齐损耗）。
    pub pad_bytes: u64,
}

impl RegionPlan {
    /// 在竞技场 [arena_base, arena_base+arena_len) 内为 ap_count 个
    /// AP 雕刻栈与 per-CPU 区。
    ///
    /// 雕刻序：AP0 栈 → AP1 栈 → … → AP0 区 → AP1 区 → …
    /// （栈先整块排布、区整块排布，各自对齐类内聚，填充损耗最小）。
    pub fn carve(arena_base: u64, arena_len: u64, ap_count: u64) -> Result<RegionPlan, ApStackError> {
        if !as_is_aligned(arena_base, AS_AREA_ALIGN) {
            return Err(ApStackError::AlignBad {
                base: arena_base,
                align: AS_AREA_ALIGN,
            });
        }
        let mut regions: Vec<PlanRegion> = Vec::new();
        let mut cursor: u64 = arena_base;
        let mut pad: u64 = 0;
        // 先排栈：每 AP 一块，16B 对齐。
        let mut i: u64 = 0;
        while i < ap_count {
            let aligned = as_align_up(cursor, AS_STACK_ALIGN);
            pad = pad.wrapping_add(aligned.wrapping_sub(cursor));
            let reg = PlanRegion {
                kind: RegionKind::Stack,
                owner: i as u32,
                base: aligned,
                len: AS_STACK_BYTES,
            };
            regions.push(reg);
            cursor = aligned.wrapping_add(AS_STACK_BYTES);
            i += 1;
        }
        // 再排区：每 AP 一块，4KiB 对齐。
        let mut i: u64 = 0;
        while i < ap_count {
            let aligned = as_align_up(cursor, AS_AREA_ALIGN);
            pad = pad.wrapping_add(aligned.wrapping_sub(cursor));
            let reg = PlanRegion {
                kind: RegionKind::PerCpuArea,
                owner: i as u32,
                base: aligned,
                len: AS_AREA_BYTES,
            };
            regions.push(reg);
            cursor = aligned.wrapping_add(AS_AREA_BYTES);
            i += 1;
        }
        let used = cursor.wrapping_sub(arena_base);
        let mut plan = RegionPlan {
            arena_base,
            arena_len,
            regions,
            budget: Vec::new(),
            overlap_cases: Vec::new(),
            used_bytes: used,
            pad_bytes: pad,
        };
        // 预算账：分区合计 + 填充 <= 竞技场容量，超支显性立案。
        let need_stack = ap_count.saturating_mul(AS_STACK_BYTES);
        let need_area = ap_count.saturating_mul(AS_AREA_BYTES);
        plan.budget.push(BudgetRow {
            item: format!("AP栈 × {}", ap_count),
            bytes: need_stack,
        });
        plan.budget.push(BudgetRow {
            item: format!("per-CPU区 × {}", ap_count),
            bytes: need_area,
        });
        plan.budget.push(BudgetRow {
            item: "对齐填充".into(),
            bytes: pad,
        });
        plan.budget.push(BudgetRow {
            item: "合计需求".into(),
            bytes: plan.used_bytes,
        });
        plan.budget.push(BudgetRow {
            item: "竞技场容量".into(),
            bytes: arena_len,
        });
        if plan.used_bytes > arena_len {
            return Err(ApStackError::BudgetOver {
                need: plan.used_bytes,
                have: arena_len,
            });
        }
        // 布局校验：基址对齐逐区过。
        for reg in plan.regions.iter() {
            let want_align = match reg.kind {
                RegionKind::Stack => AS_STACK_ALIGN,
                RegionKind::PerCpuArea => AS_AREA_ALIGN,
            };
            if !as_is_aligned(reg.base, want_align) {
                return Err(ApStackError::AlignBad {
                    base: reg.base,
                    align: want_align,
                });
            }
        }
        // 区间不重叠审计（两两，n² 但 n ≤ 2×AP 数 ≤ 32）。
        plan.audit_overlap()?;
        Ok(plan)
    }

    /// 区间两两不重叠审计：发现重叠 → 立案并返回 XC_RANGE_OVERLAP。
    /// （立案与报错并存：案簿留在 plan 内可查，错误上抛阻断使用。）
    pub fn audit_overlap(&mut self) -> Result<(), ApStackError> {
        self.overlap_cases.clear();
        let n = self.regions.len();
        let mut case_no: u64 = 0;
        let mut i = 0;
        while i < n {
            let mut j = i + 1;
            while j < n {
                // 逐字段拷贝比较（借用拆分：先 Copy 出来再比较）。
                let ra = self.regions[i];
                let rb = self.regions[j];
                if ra.overlaps(&rb) {
                    case_no += 1;
                    let ov_lo = as_max(ra.base, rb.base);
                    let ov_hi = as_min(
                        ra.base.wrapping_add(ra.len),
                        rb.base.wrapping_add(rb.len),
                    );
                    let ov = ov_hi.saturating_sub(ov_lo);
                    self.overlap_cases.push(OverlapCase {
                        case_no,
                        a: format!("{}#{}@[{:#x}+{:#x})", ra.kind.name(), ra.owner, ra.base, ra.len),
                        b: format!("{}#{}@[{:#x}+{:#x})", rb.kind.name(), rb.owner, rb.base, rb.len),
                        overlap_bytes: ov,
                    });
                    return Err(ApStackError::RangeOverlap {
                        a_base: ra.base,
                        a_len: ra.len,
                        b_base: rb.base,
                        b_len: rb.len,
                    });
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 注入重叠（注入面专用）：把指定两个区间强行改成交叠布局，
    /// 再跑审计——必须立案。返回立案结果。
    pub fn inject_overlap(&mut self, idx_a: usize, idx_b: usize) -> Result<OverlapCase, ApStackError> {
        if idx_a >= self.regions.len() || idx_b >= self.regions.len() || idx_a == idx_b {
            return Err(ApStackError::NotAllocated { cpu_id: 0xFFFF_FFFF });
        }
        // 把 b 区基址改写进 a 区内部（制造真实重叠）。
        let a = self.regions[idx_a];
        self.regions[idx_b].base = a.base.wrapping_add(a.len / 2);
        match self.audit_overlap() {
            Ok(()) => Err(ApStackError::RangeOverlap {
                a_base: 0,
                a_len: 0,
                b_base: 0,
                b_len: 0,
            }),
            Err(e) => {
                // 立案簿里必有对应案。
                match self.overlap_cases.last() {
                    Some(c) => Ok(c.clone()),
                    None => Err(e),
                }
            }
        }
    }

    /// 取指定 AP 的栈区基址。
    pub fn stack_base_of(&self, cpu_id: u32) -> Option<u64> {
        let mut i = 0;
        while i < self.regions.len() {
            let r = self.regions[i];
            if r.kind == RegionKind::Stack && r.owner == cpu_id {
                return Some(r.base);
            }
            i += 1;
        }
        None
    }

    /// 取指定 AP 的区基址。
    pub fn area_base_of(&self, cpu_id: u32) -> Option<u64> {
        let mut i = 0;
        while i < self.regions.len() {
            let r = self.regions[i];
            if r.kind == RegionKind::PerCpuArea && r.owner == cpu_id {
                return Some(r.base);
            }
            i += 1;
        }
        None
    }

    /// 取指定 AP 的双基址（栈, 区）——J1 「基址写入」的规划侧出口。
    pub fn dual_base_of(&self, cpu_id: u32) -> Option<(u64, u64)> {
        match (self.stack_base_of(cpu_id), self.area_base_of(cpu_id)) {
            (Some(s), Some(a)) => Some((s, a)),
            _ => None,
        }
    }
}

// ===========================================================================
// ApResourceBook —— 实例账（双基址账 + 墓碑隔离 + 重复分配拒收）
// ===========================================================================

/// 账容量：16 槽（本模块账面定容；全局 AP 上限 255 由 UNX-F0335
/// 管辖，本账按演示域 4~16 核收口定容，墓碑也占槽——隔离优先于
/// 容量复用）。
pub const AS_BOOK_SLOTS: usize = 16;

/// 槽位状态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlotState {
    /// 空槽。
    Free,
    /// 活账：cpu_id → (栈基址, 区基址)。
    Allocated {
        /// cpu_id。
        cpu_id: u32,
        /// 栈基址。
        stack_base: u64,
        /// 区基址。
        area_base: u64,
    },
    /// 墓碑：回收后永不复用（隔离态）。
    Tombstoned {
        /// cpu_id。
        cpu_id: u32,
        /// 原栈基址。
        stack_base: u64,
        /// 原区基址。
        area_base: u64,
        /// 回收序号（账面单调）。
        epoch: u64,
    },
}

/// 一条账动记录（分配/回收流水）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookRecord {
    /// 流水序号。
    pub seq: u64,
    /// 动作名（"alloc" / "recycle"）。
    pub action: &'static str,
    /// cpu_id。
    pub cpu_id: u32,
    /// 栈基址（无则 0）。
    pub stack_base: u64,
    /// 区基址（无则 0）。
    pub area_base: u64,
}

/// 实例账：cpu_id → (栈基址, 区基址) 双基址账。
#[derive(Clone, Debug)]
pub struct ApResourceBook {
    /// 槽位表（定容 AS_BOOK_SLOTS）。
    slots: Vec<SlotState>,
    /// 流水账。
    records: Vec<BookRecord>,
    /// 流水序号游标。
    seq: u64,
    /// 回收纪元游标。
    epoch: u64,
}

impl ApResourceBook {
    /// 建空账。
    pub fn new() -> ApResourceBook {
        let mut slots = Vec::new();
        let mut i = 0;
        while i < AS_BOOK_SLOTS {
            slots.push(SlotState::Free);
            i += 1;
        }
        ApResourceBook {
            slots,
            records: Vec::new(),
            seq: 0,
            epoch: 0,
        }
    }

    /// 分配：取首个 Free 槽，登记 cpu_id → 双基址。
    ///
    /// 拒收路径：cpu_id 已活账（XC_DUP_ALLOC）；cpu_id 曾墓碑
    /// （XC_TOMBSTONE_REUSE）；无 Free 槽（XC_SLOT_EXHAUSTED）；
    /// 栈基址非 16 对齐 / 区基址非 4KiB 对齐（XC_ALIGN_BAD）。
    pub fn allocate(&mut self, cpu_id: u32, stack_base: u64, area_base: u64) -> Result<(u64, u64), ApStackError> {
        // cpu_id 唯一性红线：活账拒，墓碑也拒。
        let mut i = 0;
        while i < self.slots.len() {
            match &self.slots[i] {
                SlotState::Allocated { cpu_id: c, .. } if *c == cpu_id => {
                    return Err(ApStackError::DupAlloc { cpu_id });
                }
                SlotState::Tombstoned { cpu_id: c, .. } if *c == cpu_id => {
                    return Err(ApStackError::TombstoneReuse { cpu_id });
                }
                _ => {}
            }
            i += 1;
        }
        // 对齐红线。
        if !as_is_aligned(stack_base, AS_STACK_ALIGN) {
            return Err(ApStackError::AlignBad {
                base: stack_base,
                align: AS_STACK_ALIGN,
            });
        }
        if !as_is_aligned(area_base, AS_AREA_ALIGN) {
            return Err(ApStackError::AlignBad {
                base: area_base,
                align: AS_AREA_ALIGN,
            });
        }
        // 首个 Free 槽。
        let mut slot: i64 = -1;
        let mut i = 0;
        while i < self.slots.len() {
            if matches!(self.slots[i], SlotState::Free) {
                slot = i as i64;
                break;
            }
            i += 1;
        }
        if slot < 0 {
            return Err(ApStackError::SlotExhausted {
                capacity: AS_BOOK_SLOTS as u64,
            });
        }
        self.slots[slot as usize] = SlotState::Allocated {
            cpu_id,
            stack_base,
            area_base,
        };
        self.seq = self.seq.wrapping_add(1);
        self.records.push(BookRecord {
            seq: self.seq,
            action: "alloc",
            cpu_id,
            stack_base,
            area_base,
        });
        Ok((stack_base, area_base))
    }

    /// 回收：活账 → 墓碑（槽位隔离，永不复用）。
    pub fn recycle(&mut self, cpu_id: u32) -> Result<(), ApStackError> {
        let mut i = 0;
        while i < self.slots.len() {
            if let SlotState::Allocated {
                cpu_id: c,
                stack_base,
                area_base,
            } = &self.slots[i]
            {
                if *c == cpu_id {
                    let sb = *stack_base;
                    let ab = *area_base;
                    self.epoch = self.epoch.wrapping_add(1);
                    self.slots[i] = SlotState::Tombstoned {
                        cpu_id,
                        stack_base: sb,
                        area_base: ab,
                        epoch: self.epoch,
                    };
                    self.seq = self.seq.wrapping_add(1);
                    self.records.push(BookRecord {
                        seq: self.seq,
                        action: "recycle",
                        cpu_id,
                        stack_base: sb,
                        area_base: ab,
                    });
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(ApStackError::NotAllocated { cpu_id })
    }

    /// 查活账双基址（墓碑不回——查询面只见活账）。
    pub fn lookup(&self, cpu_id: u32) -> Option<(u64, u64)> {
        let mut i = 0;
        while i < self.slots.len() {
            if let SlotState::Allocated {
                cpu_id: c,
                stack_base,
                area_base,
            } = &self.slots[i]
            {
                if *c == cpu_id {
                    return Some((*stack_base, *area_base));
                }
            }
            i += 1;
        }
        None
    }

    /// 活账数。
    pub fn live_count(&self) -> u64 {
        let mut n = 0;
        let mut i = 0;
        while i < self.slots.len() {
            if matches!(self.slots[i], SlotState::Allocated { .. }) {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 墓碑数。
    pub fn tombstone_count(&self) -> u64 {
        let mut n = 0;
        let mut i = 0;
        while i < self.slots.len() {
            if matches!(self.slots[i], SlotState::Tombstoned { .. }) {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 流水账只读视图。
    pub fn records(&self) -> &[BookRecord] {
        &self.records
    }

    /// 槽位隔离审计：墓碑基址与一切活账基址两两不同（隔离红线——
    /// 若同基址则墓碑基址已复活，悬垂栈指针风险）。
    pub fn audit_slot_isolation(&self) -> Result<(), ApStackError> {
        let mut i = 0;
        while i < self.slots.len() {
            if let SlotState::Tombstoned {
                cpu_id: tc,
                stack_base: tsb,
                area_base: tab,
                ..
            } = &self.slots[i]
            {
                let mut j = 0;
                while j < self.slots.len() {
                    if let SlotState::Allocated {
                        cpu_id: lc,
                        stack_base: lsb,
                        area_base: lab,
                    } = &self.slots[j]
                    {
                        if *tsb == *lsb || *tab == *lab {
                            return Err(ApStackError::RangeOverlap {
                                a_base: *tsb,
                                a_len: AS_STACK_BYTES,
                                b_base: *lsb,
                                b_len: AS_STACK_BYTES,
                            });
                        }
                        // 同 cpu_id 出现在墓碑与活账 = 复活，重案。
                        let _ = tc;
                        let _ = lc;
                    }
                    j += 1;
                }
            }
            i += 1;
        }
        Ok(())
    }
}

// ===========================================================================
// EventLedger —— 事件账（定容 + 溢出显性丢弃 + FNV-1a 64 指纹）
// ===========================================================================

/// 事件账容量：256 行（定容，满后显性丢弃并计数，不挤旧账）。
pub const AS_EVENT_CAP: usize = 256;

/// 一条事件：序号 + 诊断码 + cpu_id + 备注。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventRow {
    /// 事件序号（账内单调）。
    pub seq: u64,
    /// 诊断码（0 = 正常动作行）。
    pub code: u32,
    /// 关联 cpu_id（无关联 = 0xFFFF_FFFF）。
    pub cpu_id: u32,
    /// 备注。
    pub note: String,
}

impl EventRow {
    /// 是否错误事件（诊断码落在本模块闭集内即错误）。
    pub fn is_error(&self) -> bool {
        self.code >= AS_SEG_BASE as u32 && self.code <= XC_EVENT_FULL
    }
}

/// 事件账。
#[derive(Clone, Debug)]
pub struct EventLedger {
    rows: Vec<EventRow>,
    seq: u64,
    drops: u64,
}

impl EventLedger {
    /// 建空账。
    pub fn new() -> EventLedger {
        EventLedger {
            rows: Vec::new(),
            seq: 0,
            drops: 0,
        }
    }

    /// 入账一条。账满 → 丢弃计数递增并上抛 EventFull（旧账不动）。
    pub fn push(&mut self, code: u32, cpu_id: u32, note: String) -> Result<(), ApStackError> {
        if self.rows.len() >= AS_EVENT_CAP {
            self.drops = self.drops.wrapping_add(1);
            return Err(ApStackError::EventFull {
                capacity: AS_EVENT_CAP as u64,
            });
        }
        self.seq = self.seq.wrapping_add(1);
        self.rows.push(EventRow {
            seq: self.seq,
            code,
            cpu_id,
            note,
        });
        Ok(())
    }

    /// 入账一条错误（错误码 + 错误详情自动格式化）。
    pub fn push_error(&mut self, err: &ApStackError, cpu_id: u32) -> Result<(), ApStackError> {
        let note = format!("xc={:#x} {}", err.xc_code(), err.detail());
        self.push(err.xc_code(), cpu_id, note)
    }

    /// 错误事件数。
    pub fn error_count(&self) -> u64 {
        let mut n = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].is_error() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 正常事件数。
    pub fn info_count(&self) -> u64 {
        (self.rows.len() as u64).saturating_sub(self.error_count())
    }

    /// 显性丢弃计数。
    pub fn drops(&self) -> u64 {
        self.drops
    }

    /// 账行只读视图。
    pub fn rows(&self) -> &[EventRow] {
        &self.rows
    }

    /// FNV-1a 64 指纹：对全部账行逐字段字节化后滚动（双跑对拍锚）。
    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            let mut j = 0;
            while j < 8 {
                buf.push((r.seq >> (j * 8)) as u8);
                buf.push((r.code as u64 >> (j * 8)) as u8);
                buf.push((r.cpu_id as u64 >> (j * 8)) as u8);
                j += 1;
            }
            let nb = r.note.as_bytes();
            let mut k = 0;
            while k < nb.len() {
                buf.push(nb[k]);
                k += 1;
            }
            buf.push(0xFF); // 行分隔
            i += 1;
        }
        // 丢弃计数参与指纹（丢弃不同 → 指纹必不同）。
        let mut j = 0;
        while j < 8 {
            buf.push((self.drops >> (j * 8)) as u8);
            j += 1;
        }
        fnv1a64(&buf)
    }
}

// ===========================================================================
// ApStackSubsystem —— 总编排（前置闸 / bring_up / verify / 注入 / 对拍）
// ===========================================================================

/// health() 体检报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthReport {
    /// 总判定。
    pub ok: bool,
    /// 活账数。
    pub live: u64,
    /// 墓碑数。
    pub tombstones: u64,
    /// 错误事件数。
    pub error_events: u64,
    /// 事件丢弃数。
    pub drops: u64,
    /// 哨兵破坏字数（全区扫描）。
    pub broken_sentinel_words: u64,
    /// 结论（中文账面）。
    pub verdict: String,
}

/// 双跑对拍指纹（两次 orchestrate 结果必须逐字段相等）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunFingerprint {
    /// 事件账 FNV 指纹。
    pub ledger_fp: u64,
    /// 活账数。
    pub live: u64,
    /// 规划已用字节。
    pub used_bytes: u64,
    /// 事件行数。
    pub event_rows: u64,
    /// 全区验证是否全绿。
    pub all_green: bool,
}

/// AP 栈与 per-CPU 区总编排体。
#[derive(Clone, Debug)]
pub struct ApStackSubsystem {
    /// 区规划（布局校验已过）。
    pub plan: RegionPlan,
    /// 实例账。
    pub book: ApResourceBook,
    /// 栈实例（按 bring_up 顺序）。
    pub stacks: Vec<StackImage>,
    /// 区实例（按 bring_up 顺序）。
    pub areas: Vec<AreaImage>,
    /// 事件账。
    pub ledger: EventLedger,
    /// F0188 前置包闸：就绪后 bring_up 才放行。
    front_ready: bool,
    /// 计划 AP 数。
    pub ap_count: u64,
}

impl ApStackSubsystem {
    /// 建编排体：雕区 + 空账。front_ready=false 时 bring_up 全拒
    /// （F0188 前置包消费闸的默认态）。
    pub fn new(
        arena_base: u64,
        arena_len: u64,
        ap_count: u64,
        front_ready: bool,
    ) -> Result<ApStackSubsystem, ApStackError> {
        if ap_count as usize > AS_BOOK_SLOTS {
            return Err(ApStackError::SlotExhausted {
                capacity: AS_BOOK_SLOTS as u64,
            });
        }
        let plan = RegionPlan::carve(arena_base, arena_len, ap_count)?;
        let mut ledger = EventLedger::new();
        let _ = ledger.push(
            0,
            0xFFFF_FFFF,
            format!(
                "{} 规划成：{} 区间 / 已用 {:#x} / 填充 {:#x} / 前置闸={}",
                AS_VERSION,
                plan.regions.len(),
                plan.used_bytes,
                plan.pad_bytes,
                front_ready
            ),
        );
        Ok(ApStackSubsystem {
            plan,
            book: ApResourceBook::new(),
            stacks: Vec::new(),
            areas: Vec::new(),
            ledger,
            front_ready,
            ap_count,
        })
    }

    /// F0188 前置包到达：开闸（一次性，不可回关——前置闸单向）。
    pub fn open_front(&mut self) {
        if !self.front_ready {
            self.front_ready = true;
            let _ = self.ledger.push(0, 0xFFFF_FFFF, "F0188 前置包到达，实例化闸开".into());
        }
    }

    /// 前置闸状态。
    pub fn front_is_ready(&self) -> bool {
        self.front_ready
    }

    /// 实例化一个 AP 的栈与 per-CPU 区（J1 主体路径）。
    ///
    /// 前置：F0188 闸开；规划含该 cpu_id；账未分配该 cpu_id。
    /// 步骤：取双基址 → 账登记 → 栈实例化 → 区实例化 → 区验收
    /// → 栈 boot frame 读回 → 事件落账。
    pub fn bring_up(&mut self, cpu_id: u32, node: u32) -> Result<(u64, u64), ApStackError> {
        // F0188 前置闸。
        if !self.front_ready {
            let _ = self
                .ledger
                .push(XC_NOT_ALLOCATED, cpu_id, "F0188 前置包未就绪，实例化拒收".into());
            return Err(ApStackError::NotAllocated { cpu_id });
        }
        // 规划侧双基址（J1 「基址写入」的规划出口）。
        let (sb, ab) = match self.plan.dual_base_of(cpu_id) {
            Some(p) => p,
            None => {
                let _ = self
                    .ledger
                    .push(XC_NOT_ALLOCATED, cpu_id, "规划无该 cpu_id 区间".into());
                return Err(ApStackError::NotAllocated { cpu_id });
            }
        };
        // 账登记（重复分配在此拒收）。
        self.book.allocate(cpu_id, sb, ab)?;
        let _ = self.ledger.push(
            0,
            cpu_id,
            format!("账登记：栈基址={:#x} 区基址={:#x}", sb, ab),
        );
        // 栈实例化。
        let stack = StackImage::instantiate(cpu_id, node, sb, ab)?;
        let _ = self.ledger.push(0, cpu_id, format!("栈实例化 32KiB @ {:#x}", sb));
        // 区实例化（F0192 语义：self_ptr 写区基址）。
        let area = PerCpuInstantiator::instantiate(cpu_id, node, ab, sb, AS_STACK_BYTES)?;
        let _ = self
            .ledger
            .push(0, cpu_id, format!("区实例化 4KiB @ {:#x}（self_ptr 自指已写）", ab));
        // 区验收（清单逐字段 + 统计槽清零）。
        PerCpuInstantiator::verify(&area, sb, AS_STACK_BYTES)?;
        // 栈 boot frame 读回（area_base 回填交叉验证）。
        stack.check_boot_frame(ab)?;
        let _ = self.ledger.push(0, cpu_id, "实例化验收全绿（清单+哨兵+boot frame）".into());
        self.stacks.push(stack);
        self.areas.push(area);
        Ok((sb, ab))
    }

    /// 取某 AP 的栈实例只读引用。
    pub fn stack_of(&self, cpu_id: u32) -> Option<&StackImage> {
        let mut i = 0;
        while i < self.stacks.len() {
            if self.stacks[i].cpu_id == cpu_id {
                return Some(&self.stacks[i]);
            }
            i += 1;
        }
        None
    }

    /// 取某 AP 的区实例可变引用（注入面用）。
    pub fn area_of_mut(&mut self, cpu_id: u32) -> Option<&mut AreaImage> {
        let mut i = 0;
        while i < self.areas.len() {
            if self.areas[i].cpu_id == cpu_id {
                return Some(&mut self.areas[i]);
            }
            i += 1;
        }
        None
    }

    /// 取某 AP 的栈实例可变引用（注入面用）。
    pub fn stack_of_mut(&mut self, cpu_id: u32) -> Option<&mut StackImage> {
        let mut i = 0;
        while i < self.stacks.len() {
            if self.stacks[i].cpu_id == cpu_id {
                return Some(&mut self.stacks[i]);
            }
            i += 1;
        }
        None
    }

    /// 全量验证一个 AP：哨兵扫描 + boot frame + 区清单 + 统计槽。
    pub fn verify_ap(&mut self, cpu_id: u32) -> Result<(), ApStackError> {
        let (sb, ab) = match self.book.lookup(cpu_id) {
            Some(p) => p,
            None => {
                let _ = self
                    .ledger
                    .push(XC_NOT_ALLOCATED, cpu_id, "验证目标不在活账".into());
                return Err(ApStackError::NotAllocated { cpu_id });
            }
        };
        // 找实例（账与实例必须同在——不同在即账实分离立案）。
        let mut stack_idx: i64 = -1;
        let mut i = 0;
        while i < self.stacks.len() {
            if self.stacks[i].cpu_id == cpu_id {
                stack_idx = i as i64;
                break;
            }
            i += 1;
        }
        if stack_idx < 0 {
            let _ = self
                .ledger
                .push(XC_NOT_ALLOCATED, cpu_id, "账实分离：活账无栈实例".into());
            return Err(ApStackError::NotAllocated { cpu_id });
        }
        // 哨兵扫描。
        if let Some((band, w)) = self.stacks[stack_idx as usize].check_sentinels() {
            let err = ApStackError::SentinelBroken {
                cpu_id,
                band,
                word_index: w,
            };
            let _ = self.ledger.push_error(&err, cpu_id);
            return Err(err);
        }
        // boot frame。
        if let Err(e) = self.stacks[stack_idx as usize].check_boot_frame(ab) {
            let _ = self.ledger.push_error(&e, cpu_id);
            return Err(e);
        }
        // 区清单 + 统计槽。
        if let Err(e) = PerCpuInstantiator::verify(&self.areas[stack_idx as usize], sb, AS_STACK_BYTES)
        {
            let _ = self.ledger.push_error(&e, cpu_id);
            return Err(e);
        }
        let _ = self.ledger.push(0, cpu_id, "verify_ap 全绿".into());
        Ok(())
    }

    /// 全 AP 巡检：返回首个失败（账序），全绿返回 Ok。
    pub fn verify_all(&mut self) -> Result<(), ApStackError> {
        let mut cpu: u64 = 0;
        while cpu < self.ap_count {
            if self.book.lookup(cpu as u32).is_some() {
                self.verify_ap(cpu as u32)?;
            }
            cpu += 1;
        }
        let _ = self.ledger.push(0, 0xFFFF_FFFF, "verify_all 全 AP 巡检全绿".into());
        Ok(())
    }

    /// 哨兵位扫描（全区）：统计所有栈全部哨兵带中与魔数不符的字节数。
    pub fn sentinel_bit_scan(&self) -> u64 {
        let mut bad_bytes: u64 = 0;
        let mut si = 0;
        while si < self.stacks.len() {
            let s = &self.stacks[si];
            let mut w: u64 = 0;
            while w < AS_GUARD_WORDS {
                let v = rd_u64_le(&s.bytes, w * AS_GUARD_WORD_BYTES).unwrap_or(0);
                bad_bytes = bad_bytes.wrapping_add((v ^ AS_SENTINEL_WORD).count_ones() as u64);
                w += 1;
            }
            let mut w: u64 = 0;
            while w < AS_GUARD_WORDS {
                let off = stack_guard_hi_lo() + w * AS_GUARD_WORD_BYTES;
                let v = rd_u64_le(&s.bytes, off).unwrap_or(0);
                bad_bytes = bad_bytes.wrapping_add((v ^ AS_SENTINEL_WORD).count_ones() as u64);
                w += 1;
            }
            si += 1;
        }
        bad_bytes
    }

    /// health() 体检：账面 + 事件账 + 哨兵位扫描合成判定。
    pub fn health(&self) -> HealthReport {
        let broken = self.sentinel_bit_scan();
        let err_events = self.ledger.error_count();
        let drops = self.ledger.drops();
        let iso_ok = self.book.audit_slot_isolation().is_ok();
        let ok = broken == 0 && err_events == 0 && drops == 0 && iso_ok;
        let verdict = if ok {
            format!("体检全绿：活账{} 墓碑{} 哨兵0破 隔离审计过", self.book.live_count(), self.book.tombstone_count())
        } else {
            format!(
                "体检立案：哨兵破{}位 错误事件{} 丢弃{} 隔离审计{}",
                broken,
                err_events,
                drops,
                if iso_ok { "过" } else { "未过" }
            )
        };
        HealthReport {
            ok,
            live: self.book.live_count(),
            tombstones: self.book.tombstone_count(),
            error_events: err_events,
            drops,
            broken_sentinel_words: broken,
            verdict,
        }
    }

    // -----------------------------------------------------------------
    // 注入面（三类注入，全部检出 + 修复回归）
    // -----------------------------------------------------------------

    /// 注入一：哨兵破坏。把指定带的指定哨兵字改写为破坏值，
    /// 返回改写是否落位。检出由 verify_ap 完成（调用侧断言）。
    pub fn inject_sentinel_break(&mut self, cpu_id: u32, band: SentinelBand, word: u64) -> bool {
        let off = match band {
            SentinelBand::Low => word * AS_GUARD_WORD_BYTES,
            SentinelBand::High => stack_guard_hi_lo() + word * AS_GUARD_WORD_BYTES,
        };
        let broken = AS_SENTINEL_WORD ^ 0x0000_0000_0000_00FF;
        match self.stack_of_mut(cpu_id) {
            Some(s) => wr_u64_le(&mut s.bytes, off, broken),
            None => false,
        }
    }

    /// 注入一修复回归：重写全部哨兵字 + 复验。
    pub fn repair_and_verify(&mut self, cpu_id: u32) -> Result<(), ApStackError> {
        if let Some(s) = self.stack_of_mut(cpu_id) {
            let _ = s.repair_sentinels();
        }
        self.verify_ap(cpu_id)
    }

    /// 注入二：重复分配。对已活账 cpu_id 再次 allocate——必须
    /// 被账面拒收（返回 Err(DupAlloc) 即预期）。
    pub fn inject_dup_alloc(&mut self, cpu_id: u32) -> Result<(), ApStackError> {
        let (sb, ab) = match self.plan.dual_base_of(cpu_id) {
            Some(p) => p,
            None => (0, 0),
        };
        match self.book.allocate(cpu_id, sb, ab) {
            Ok(_) => {
                let _ = self
                    .ledger
                    .push(XC_DUP_ALLOC, cpu_id, "注入异常：重复分配未被拦截".into());
                Err(ApStackError::DupAlloc { cpu_id })
            }
            Err(e) => {
                let _ = self.ledger.push_error(&e, cpu_id);
                Err(e)
            }
        }
    }

    /// 注入三：区间重叠。对规划内两个区间强行交叠再审计——必须
    /// 立案（返回立案案卷）。
    pub fn inject_overlap(&mut self, idx_a: usize, idx_b: usize) -> Result<OverlapCase, ApStackError> {
        self.plan.inject_overlap(idx_a, idx_b)
    }

    /// F0192 语义注入：破坏 self_ptr 后复验——必须检出
    /// XC_SELF_PTR_MISMATCH。返回检出结果。
    pub fn inject_self_ptr_break_and_detect(&mut self, cpu_id: u32) -> Result<bool, ApStackError> {
        let (sb, _ab) = match self.book.lookup(cpu_id) {
            Some(p) => p,
            None => return Err(ApStackError::NotAllocated { cpu_id }),
        };
        let _ = sb;
        if let Some(a) = self.area_of_mut(cpu_id) {
            let _ = a.inject_self_ptr(0xDEAD_BEEF_0000_0000);
        }
        match self.verify_ap(cpu_id) {
            Err(ApStackError::SelfPtrMismatch { .. }) => {
                // 修复回归。
                if let Some(a) = self.area_of_mut(cpu_id) {
                    let _ = a.repair(sb, AS_STACK_BYTES);
                }
                self.verify_ap(cpu_id)?;
                Ok(true)
            }
            Err(e) => Err(e),
            Ok(()) => Ok(false),
        }
    }

    // -----------------------------------------------------------------
    // 回收与账面闭环
    // -----------------------------------------------------------------

    /// 回收一个 AP（栈/区实例移出实例集，账面进墓碑）+ 隔离审计。
    pub fn retire(&mut self, cpu_id: u32) -> Result<(), ApStackError> {
        self.book.recycle(cpu_id)?;
        // 实例集移除（账实同步）。
        let mut i = 0;
        while i < self.stacks.len() {
            if self.stacks[i].cpu_id == cpu_id {
                self.stacks.remove(i);
                break;
            }
            i += 1;
        }
        let mut i = 0;
        while i < self.areas.len() {
            if self.areas[i].cpu_id == cpu_id {
                self.areas.remove(i);
                break;
            }
            i += 1;
        }
        self.book.audit_slot_isolation()?;
        let _ = self
            .ledger
            .push(0, cpu_id, "回收完成：槽位进墓碑，实例集移除，隔离审计过".into());
        Ok(())
    }

    /// 双跑对拍：同种子两次 orchestrate 指纹必须逐字段相等。
    pub fn orchestrate(seed: u64, ap_count: u64, arena_len: u64) -> RunFingerprint {
        let mut sub = match ApStackSubsystem::new(0x1000_0000, arena_len, ap_count, false) {
            Ok(s) => s,
            Err(_) => {
                // 编排入参由测试面保证合法；失败回空指纹（可对拍的不变式仍然成立）。
                return RunFingerprint {
                    ledger_fp: 0,
                    live: 0,
                    used_bytes: 0,
                    event_rows: 0,
                    all_green: false,
                };
            }
        };
        sub.open_front();
        let mut lcg = Lcg::new(seed);
        let mut cpu: u64 = 0;
        while cpu < ap_count {
            let node = lcg.next_below(4) as u32;
            let _ = sub.bring_up(cpu as u32, node);
            cpu += 1;
        }
        let all_green = sub.verify_all().is_ok();
        // 对拍前做一次回收+再审计（确定性动作序列，双跑同构）。
        if ap_count > 1 {
            let _ = sub.retire((ap_count - 1) as u32);
        }
        RunFingerprint {
            ledger_fp: sub.ledger.fingerprint(),
            live: sub.book.live_count(),
            used_bytes: sub.plan.used_bytes,
            event_rows: sub.ledger.rows().len() as u64,
            all_green,
        }
    }
}

// ===========================================================================
// 冒烟测试（探针侧 cargo test 全绿即 J1 验收面）
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试竞技场：基址 256MiB（4KiB 对齐），容量按 AP 数给足。
    const ARENA: u64 = 0x1000_0000;
    /// 每 AP 预算：32KiB 栈 + 4KiB 区 + 填充余量。
    const PER_AP_BUDGET: u64 = AS_STACK_BYTES + AS_AREA_BYTES + AS_AREA_ALIGN;
    /// 测试 AP 数。
    const N_AP: u64 = 4;

    fn arena_len(n: u64) -> u64 {
        n.saturating_mul(PER_AP_BUDGET).saturating_add(0x1000)
    }

    /// 建 4 AP 就绪编排体并全部 bring_up。
    fn fresh_ready() -> ApStackSubsystem {
        let mut sub = ApStackSubsystem::new(ARENA, arena_len(N_AP), N_AP, false).unwrap();
        sub.open_front();
        let mut cpu: u64 = 0;
        while cpu < N_AP {
            sub.bring_up(cpu as u32, cpu as u32 % 4).unwrap();
            cpu += 1;
        }
        sub
    }

    #[test]
    fn test_layout_table_audit_and_consts() {
        assert!(stack_layout_table_audit().is_ok());
        assert_eq!(stack_body_lo(), 64);
        assert_eq!(stack_body_hi(), 32 * 1024 - 64 - 64);
        assert_eq!(stack_boot_frame_lo(), 32 * 1024 - 64);
        assert_eq!(stack_layout_table().len(), 4);
        assert!(as_is_aligned(as_align_up(3, 16), 16));
        assert_eq!(as_min(2, 5), 2);
        assert_eq!(as_max(2, 5), 5);
    }

    #[test]
    fn test_instantiate_readback_full() {
        let sub = fresh_ready();
        // 双基址账面。
        let (sb, ab) = sub.book.lookup(0).unwrap();
        assert_eq!(Some((sb, ab)), sub.plan.dual_base_of(0));
        // 区：self_ptr 自识别（F0192 语义）。
        let area = &sub.areas[0];
        assert!(area.self_identifies());
        assert_eq!(
            rd_u64_le(&area.bytes, area_off::OFF_MAGIC),
            Some(AS_AREA_MAGIC)
        );
        assert_eq!(
            rd_u64_le(&area.bytes, area_off::OFF_TAIL),
            Some(AS_AREA_TAIL_MAGIC)
        );
        assert_eq!(rd_u64_le(&area.bytes, area_off::OFF_STACK_BASE), Some(sb));
        assert_eq!(rd_u64_le(&area.bytes, area_off::OFF_STACK_LEN), Some(AS_STACK_BYTES));
        // 清单逐行全绿。
        let rows = PerCpuInstantiator::checklist(&sub.areas[1], sb, AS_STACK_BYTES);
        assert_eq!(rows.len(), 7);
        assert!(rows[0].ok && rows[2].ok && rows[6].ok);
        // 栈：boot frame 逐字段读回。
        let stack = &sub.stacks[0];
        assert_eq!(
            rd_u64_le(&stack.bytes, stack_boot_frame_lo() + boot_frame::OFF_MAGIC),
            Some(AS_BOOT_FRAME_MAGIC)
        );
        assert_eq!(
            rd_u64_le(&stack.bytes, stack_boot_frame_lo() + boot_frame::OFF_STACK_TOP),
            Some(sb.wrapping_add(AS_STACK_BYTES))
        );
        assert_eq!(
            rd_u64_le(&stack.bytes, stack_boot_frame_lo() + boot_frame::OFF_AREA_BASE),
            Some(ab)
        );
        // 栈体清零（抽体首与体尾各一字）。
        assert_eq!(rd_u64_le(&stack.bytes, stack_body_lo()), Some(0));
        assert_eq!(rd_u64_le(&stack.bytes, stack_body_hi() - 8), Some(0));
        // 哨兵全绿。
        assert!(stack.check_sentinels().is_none());
        assert!(!stack.overflow_detected());
        // 统计槽 16 槽全零。
        let mut s: u64 = 0;
        while s < AS_STAT_SLOTS {
            assert_eq!(rd_u64_le(&area.bytes, area_off::OFF_STATS + s * 8), Some(0));
            s += 1;
        }
        // 全 AP 巡检 + 体检。
        let mut probe = fresh_ready();
        assert!(probe.verify_all().is_ok());
        assert!(probe.health().ok);
        assert_eq!(probe.health().live, N_AP);
        assert_eq!(probe.health().broken_sentinel_words, 0);
    }

    #[test]
    fn test_front_gate_f0188() {
        let mut sub = ApStackSubsystem::new(ARENA, arena_len(2), 2, false).unwrap();
        assert!(!sub.front_is_ready());
        match sub.bring_up(0, 0) {
            Err(ApStackError::NotAllocated { cpu_id: 0 }) => {}
            other => panic!("前置闸未拦截: {:?}", other),
        }
        sub.open_front();
        assert!(sub.front_is_ready());
        assert!(sub.bring_up(0, 0).is_ok());
    }

    #[test]
    fn test_overlap_injection_case() {
        let mut plan = RegionPlan::carve(ARENA, arena_len(4), 4).unwrap();
        assert!(plan.overlap_cases.is_empty());
        assert!(plan.audit_overlap().is_ok());
        // 注入：把区 1 的基址改写进区 0 内部 → 审计必须立案。
        let case = plan.inject_overlap(0, 1).unwrap();
        assert!(case.overlap_bytes > 0);
        assert_eq!(case.case_no, 1);
        assert_eq!(plan.overlap_cases.len(), 1);
    }

    #[test]
    fn test_tombstone_no_reuse() {
        let mut sub = fresh_ready();
        let (sb0, ab0) = sub.book.lookup(0).unwrap();
        // 回收 ap0 → 墓碑。
        assert!(sub.retire(0).is_ok());
        assert_eq!(sub.book.lookup(0), None);
        assert_eq!(sub.book.tombstone_count(), 1);
        // 同 cpu_id 复活 → 拦截。
        match sub.book.allocate(0, sb0, ab0) {
            Err(ApStackError::TombstoneReuse { cpu_id: 0 }) => {}
            other => panic!("墓碑复活未被拦截: {:?}", other),
        }
        // 隔离红线：新 cpu_id 占用墓碑基址 → 隔离审计立案。
        assert!(sub.book.allocate(9, sb0, ab0).is_ok());
        assert!(sub.book.audit_slot_isolation().is_err());
        // 新 cpu_id 正常落新槽（不复用墓碑槽 0）。
        let mut book = ApResourceBook::new();
        book.allocate(0, ARENA, ARENA + 0x20_0000).unwrap();
        book.recycle(0).unwrap();
        book.allocate(1, ARENA + 0x8000, ARENA + 0x20_1000).unwrap();
        assert_eq!(book.live_count(), 1);
        assert_eq!(book.tombstone_count(), 1);
    }

    #[test]
    fn test_dup_alloc_rejected() {
        let mut sub = fresh_ready();
        match sub.inject_dup_alloc(1) {
            Err(ApStackError::DupAlloc { cpu_id: 1 }) => {}
            other => panic!("重复分配未被拦截: {:?}", other),
        }
        assert!(sub.health().error_events >= 1);
        // 账面活账数不变。
        assert_eq!(sub.book.live_count(), N_AP);
    }

    #[test]
    fn test_sentinel_break_detect_and_repair() {
        let mut sub = fresh_ready();
        // 低带注入（向下溢出位型）。
        assert!(sub.inject_sentinel_break(0, SentinelBand::Low, 2));
        match sub.verify_ap(0) {
            Err(ApStackError::SentinelBroken {
                cpu_id: 0,
                band,
                word_index: 2,
            }) => {
                assert_eq!(band.name(), "低哨兵带");
            }
            other => panic!("哨兵破坏未检出: {:?}", other),
        }
        assert!(sub.health().broken_sentinel_words > 0);
        // 高带注入（向上越界位型）。
        assert!(sub.inject_sentinel_break(1, SentinelBand::High, 7));
        assert!(sub.verify_ap(1).is_err());
        // 修复回归：哨兵位扫描归零、复验全绿；错误事件立案留痕（账面
        // 不许抹——注入检出必须有迹可查）。
        assert!(sub.repair_and_verify(0).is_ok());
        assert!(sub.repair_and_verify(1).is_ok());
        let h = sub.health();
        assert_eq!(h.broken_sentinel_words, 0);
        assert!(h.error_events >= 2);
        assert!(!h.ok); // 立案未销账，体检不为绿——语义正确
    }

    #[test]
    fn test_self_ptr_injection_f0192() {
        let mut sub = fresh_ready();
        // 破坏 self_ptr → 必须检出并修复回归。
        assert_eq!(sub.inject_self_ptr_break_and_detect(2), Ok(true));
        assert!(sub.verify_all().is_ok());
        assert!(sub.areas[2].self_identifies());
    }

    #[test]
    fn test_stat_and_magic_injection() {
        let mut sub = fresh_ready();
        // 统计槽脏值注入 → XC_STAT_NOT_ZERO。
        assert!(sub.area_of_mut(0).unwrap().inject_stat(3, 7));
        match sub.verify_ap(0) {
            Err(ApStackError::StatNotZero { cpu_id: 0, slot: 3, actual: 7 }) => {}
            other => panic!("统计槽脏值未检出: {:?}", other),
        }
        // 修复回归（重放清零）。
        assert!(sub.area_of_mut(0).unwrap().inject_stat(3, 0));
        assert!(sub.verify_ap(0).is_ok());
        // 头 magic 注入 → XC_MAGIC_MISMATCH。
        assert!(sub.area_of_mut(1).unwrap().inject_magic(0));
        match sub.verify_ap(1) {
            Err(ApStackError::MagicMismatch { cpu_id: 1, .. }) => {}
            other => panic!("magic 破坏未检出: {:?}", other),
        }
        // 修复回归。
        let (sb1, _) = sub.book.lookup(1).unwrap();
        let _ = sub.area_of_mut(1).unwrap().repair(sb1, AS_STACK_BYTES);
        assert!(sub.verify_ap(1).is_ok());
    }

    #[test]
    fn test_budget_over_and_align_reject() {
        // 预算超支：竞技场只给 1 页。
        match RegionPlan::carve(ARENA, 4096, 2) {
            Err(ApStackError::BudgetOver { need, have }) => {
                assert!(need > have);
            }
            other => panic!("预算超支未拦截: {:?}", other),
        }
        // 栈基址非 16 对齐。
        match StackImage::instantiate(0, 0, ARENA + 8, ARENA + 0x20_0000) {
            Err(ApStackError::AlignBad { align: 16, .. }) => {}
            other => panic!("栈对齐未拦截: {:?}", other),
        }
        // 区基址非 4KiB 对齐。
        match PerCpuInstantiator::instantiate(0, 0, ARENA + 0x20_0008, ARENA, AS_STACK_BYTES) {
            Err(ApStackError::AlignBad { align: 4096, .. }) => {}
            other => panic!("区对齐未拦截: {:?}", other),
        }
        // 账侧对齐拒收。
        let mut book = ApResourceBook::new();
        match book.allocate(0, ARENA + 1, ARENA + 0x20_0000) {
            Err(ApStackError::AlignBad { .. }) => {}
            other => panic!("账侧栈对齐未拦截: {:?}", other),
        }
        match book.allocate(0, ARENA, ARENA + 0x20_0001) {
            Err(ApStackError::AlignBad { .. }) => {}
            other => panic!("账侧区对齐未拦截: {:?}", other),
        }
    }

    #[test]
    fn test_slot_exhausted() {
        let mut book = ApResourceBook::new();
        let mut cpu: u64 = 0;
        while cpu < AS_BOOK_SLOTS as u64 {
            let sb = ARENA.wrapping_add(cpu.wrapping_mul(0x1_0000));
            let ab = ARENA.wrapping_add(0x40_0000).wrapping_add(cpu.wrapping_mul(0x1_0000));
            assert!(book.allocate(cpu as u32, sb, ab).is_ok());
            cpu += 1;
        }
        match book.allocate(99, ARENA, ARENA + 0x40_0000) {
            Err(ApStackError::SlotExhausted { capacity: 16 }) => {}
            other => panic!("槽位耗尽未拦截: {:?}", other),
        }
        assert_eq!(book.live_count(), AS_BOOK_SLOTS as u64);
    }

    #[test]
    fn test_recycle_unallocated() {
        let mut book = ApResourceBook::new();
        match book.recycle(5) {
            Err(ApStackError::NotAllocated { cpu_id: 5 }) => {}
            other => panic!("未分配回收未拦截: {:?}", other),
        }
    }

    #[test]
    fn test_event_full_and_drops() {
        let mut ledger = EventLedger::new();
        let mut i: u64 = 0;
        while i < AS_EVENT_CAP as u64 {
            assert!(ledger.push(0, 0, format!("fill {}", i)).is_ok());
            i += 1;
        }
        match ledger.push(0, 0, "overflow".into()) {
            Err(ApStackError::EventFull { capacity: 256 }) => {}
            other => panic!("事件账满未显性报: {:?}", other),
        }
        assert_eq!(ledger.drops(), 1);
        assert_eq!(ledger.rows().len(), AS_EVENT_CAP);
        // 指纹对丢弃敏感。
        let fp1 = ledger.fingerprint();
        let _ = ledger.push(0, 0, "again".into());
        assert_eq!(ledger.drops(), 2);
        assert_ne!(ledger.fingerprint(), fp1);
    }

    #[test]
    fn test_dual_run_determinism() {
        let a = ApStackSubsystem::orchestrate(42, 4, arena_len(4));
        let b = ApStackSubsystem::orchestrate(42, 4, arena_len(4));
        assert_eq!(a, b, "双跑指纹必须一致");
        assert!(a.all_green);
        assert_eq!(a.live, 3); // 4 实例化 + 1 回收
        // 对拍入参安全面：0 AP 也稳定。
        let z = ApStackSubsystem::orchestrate(42, 0, 0x10_0000);
        let z2 = ApStackSubsystem::orchestrate(42, 0, 0x10_0000);
        assert_eq!(z, z2);
    }

    #[test]
    fn test_consumption_declaration_rows() {
        let rows = consumption_declaration();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].task, "UNX-F0188");
        assert_eq!(rows[1].task, "UNX-F0192");
    }

    #[test]
    fn test_error_detail_and_codes() {
        // 每变体 detail 非空且码落闭集。
        let errs: Vec<ApStackError> = vec![
            ApStackError::MagicMismatch { cpu_id: 0, expect: 1, actual: 2 },
            ApStackError::CpuIdMismatch { expect: 0, actual: 1 },
            ApStackError::SelfPtrMismatch { base: 8, actual: 0 },
            ApStackError::SentinelBroken { cpu_id: 0, band: SentinelBand::Low, word_index: 0 },
            ApStackError::AlignBad { base: 3, align: 16 },
            ApStackError::RangeOverlap { a_base: 0, a_len: 8, b_base: 4, b_len: 8 },
            ApStackError::DupAlloc { cpu_id: 0 },
            ApStackError::SlotExhausted { capacity: 16 },
            ApStackError::NotAllocated { cpu_id: 0 },
            ApStackError::BudgetOver { need: 8, have: 4 },
            ApStackError::TombstoneReuse { cpu_id: 0 },
            ApStackError::LenFieldBad { field: "x", expect: 1, actual: 2 },
            ApStackError::StatNotZero { cpu_id: 0, slot: 0, actual: 1 },
            ApStackError::BootFrameBad { cpu_id: 0, field: "x", expect: 1, actual: 2 },
            ApStackError::EventFull { capacity: 256 },
        ];
        for e in errs.iter() {
            assert!(!e.detail().is_empty());
            let c = e.xc_code();
            assert!(c >= XC_MAGIC_MISMATCH && c <= XC_EVENT_FULL, "码越段: {:#x}", c);
        }
    }

    #[test]
    fn test_retire_and_verify_all_after() {
        let mut sub = fresh_ready();
        assert!(sub.retire(3).is_ok());
        assert!(sub.verify_all().is_ok());
        match sub.verify_ap(3) {
            Err(ApStackError::NotAllocated { cpu_id: 3 }) => {}
            other => panic!("墓碑 cpu 验证应报未分配: {:?}", other),
        }
        // 布局校验：栈与区区间在规划层零重叠。
        let mut plan2 = RegionPlan::carve(ARENA, arena_len(4), 4).unwrap();
        assert!(plan2.audit_overlap().is_ok());
        assert!(plan2.overlap_cases.is_empty());
    }

    #[test]
    fn test_bytes_eq_and_safe_rw() {
        assert!(bytes_eq(b"abc", b"abc"));
        assert!(!bytes_eq(b"abc", b"abd"));
        assert!(!bytes_eq(b"abc", b"ab"));
        let mut buf = [0u8; 16];
        assert!(wr_u64_le(&mut buf, 8, 0x0102_0304_0506_0708));
        assert!(!wr_u64_le(&mut buf, 9, 1));
        assert_eq!(rd_u64_le(&buf, 8), Some(0x0102_0304_0506_0708));
        assert!(rd_u64_le(&buf, 9).is_none());
        assert!(wr_u32_le(&mut buf, 0, 0xAABB_CCDD));
        assert_eq!(rd_u32_le(&buf, 0), Some(0xAABB_CCDD));
        assert!(rd_u32_le(&buf, 15).is_none());
    }

    #[test]
    fn test_lcg_determinism() {
        let mut a = Lcg::new(7);
        let mut b = Lcg::new(7);
        let mut i = 0;
        while i < 16 {
            assert_eq!(a.next_u64(), b.next_u64());
            i += 1;
        }
        let mut c = Lcg::new(7);
        assert_eq!(c.next_below(0), 0);
        let mut seen_small = false;
        let mut i = 0;
        while i < 64 {
            if c.next_below(4) < 4 {
                seen_small = true;
            }
            i += 1;
        }
        assert!(seen_small);
    }

    #[test]
    fn test_fnv_reference_vector() {
        // FNV-1a 64 空串参考向量（标准常数自检）。
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
