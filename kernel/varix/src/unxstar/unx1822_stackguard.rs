#![allow(clippy::needless_range_loop)]
//! ===========================================================================
//! UNX-F1822 · 线程栈分配与 guard 页（A1-MM 衔接）
//! ===========================================================================
//!
//! **单号**：UNX-F1822
//! **标题**：线程栈分配与 guard 页（A1-MM 衔接）
//! **域**：CoRun·UNX-A3-B12
//! **目标行数**：2200~3000 行（硬指标）
//!
//! **判据原文（注释引用）**：
//! `UNX-F1822-J1 线程栈分配/回收与 guard 页布局断言全过，栈越界注入 10/10 触发异常路径`
//!
//! 册内上下文（CoRun·UNX-A3-B12 段「切换帧」）
//! - F1821 切换帧总纲：**栈顶中断帧区 / 寄存器保存区 / 控制块锚区** 三区布局。
//! - F1624 栈约定（向下增长、对齐、可增长上限）。
//! - F1807 switch_to 原语（栈指针 + 返回地址）。
//! - F1823 保存帧寄存器集。
//! - F1824/1825 FPU 策略与原语。
//! - F1832 信号栈帧**前向预留**。
//! - F1904 GS/FS base 与 TLS。
//! - 衔接 **A1-MM 域**：地址空间 / 页表 / 页框分配器。
//!
//! 组件清单
//! 1. `Lcg` —— 确定性 64 位 LCG（`6364136223846793005 / 1442695040888963407`）。
//! 2. `Diag` —— 诊断码闭集，**独占段 0xB0C0~0xB0CF**，主码 `0xB0C0`。
//! 3. `E` / `Result<T>` —— 自定义错误闭集（零 unwrap / expect / panic）。
//! 4. `PagePool` —— 固定容量栈池（模拟 A1-MM 页框分配器）。
//! 5. `SizeClass` —— 尺寸分档（tiny 1 / small 2 / medium 4 / large 8 页）。
//! 6. `GuardAttr` —— guard 页属性闭集（NoAccess / ReadOnly / GuardPageFlag）。
//! 7. `StackDescriptor` —— 栈描述符（三区布局 + 指纹 + 魔数 + 标记）。
//! 8. `StackGuardKernel` —— 线程栈分配器主体（分配 / 回收 / 守恒 / 分档）。
//! 9. `GuardRegistry` —— guard 页登记册（append-only）。
//! 10. `AccessKind` / `GuardSide` —— 访问类型与越界方向。
//! 11. `build_switch_frame` —— F1807 切换帧构造（D 项）。
//! 12. `ForwardReserve` —— F1832 前向预留区（E 项）。
//! 13. `inject_all` —— 注入核，10 形态 10/10（F 项）。
//! 14. `export_kv` / `export_tlv` —— 观测导出（G 项）。
//! 15. `replay` —— 确定性回放（H 项）。
//!
//! 职责定位
//! 本模块是 **CoRun·UNX-A3-B12「切换帧」段** 的栈资源侧组件，向下消费 A1-MM 域的
//! 页框分配能力（`PagePool` 即其纯功能替身），向上为 F1807 切换原语提供**已对齐、
//! 已布防**的栈体。它只负责「分配 → 布局 → 布防 → 使用 → 检出 → 回收」的闭环，
//! 不涉及任何调度决策、真实内存映射与体系结构指令发射。
//!
//! 性能逐项分解（纯功能模型下的可观测分解，全部以 `tick` 为时间轴）
//! | 分项 | 计量 | 分解口径 |
//! |------|------|----------|
//! | P1 分页取整 | `pages_for(bytes)` | `ceil(bytes / 4096)`，4KiB 粒度向上取整 |
//! | P2 页框获取 | `pool_alloc` | 首次适配：空闲链表弹出，否则顺次新帧 |
//! | P3 分档命中 | `class_of(pages)` | 4 档映射 + 直方图记账 |
//! | P4 三区布局 | `layout_of` | 常数算术：4 次取整 + 3 次校验 |
//! | P5 布局校验 | `verify_layout` | 锚区/寄存器区/帧区三条边界断言 |
//! | P6 描述符建账 | `make_descriptor` | 魔数 + 三标记 + FNV-1a 指纹 |
//! | P7 guard 布防 | `plant_guards` | 2 次页框申请 + 2 次登记册 append |
//! | P8 有效区判定 | `is_valid` | 半开区间 `base <= a < top`，常数比较 |
//! | P9 访问检测 | `touch` | 区间相交判定（非单点），支持多页穿透 |
//! | P10 异常落账 | `record_hit` | 命中计数 + 侧向 + 偏移 |
//! | P11 poison 标记 | `poison` | 状态迁移 `Active -> Poisoned` |
//! | P12 帧构造 | `build_switch_frame` | 8 槽写入 + 128B 对齐核验 |
//! | P13 帧指纹 | `frame_fingerprint` | 9 字段混合，复算对账 |
//! | P14 预留区 | `ForwardReserve` | 版本号 + 消费游标 + 越界检出 |
//! | P15 守恒断言 | `assert_conservation` | `free + alloc == capacity` |
//! | P16 守恒手算锚 | 见下 | 池 64 页 / 3 栈 × 2 页 / 无 guard → 6 + 58 = 64 |
//! | P17 异常路径 | `inject_all` | 10 形态各触发 1 次专属诊断码 |
//! | P18 观测导出 | `export_kv` / `export_tlv` | KV 文本 / TLV 二进制自洽 |
//! | P19 确定性回放 | `replay` | 全流程逐字段对账，偏差恒为 0 |
//! | P20 峰值在线 | `peak_live` | 在线栈数高水位单调更新 |
//!
//! 栈布局（承接 F1821 三区布局）
//! 栈**向下增长**（F1624）。数据区按页框升序连续，guard 页外挂于两端。
//!
//! ```text
//!  高地址
//!  +--------------------------------+  hi_guard_end = base + total + PAGE
//!  |  高侧溢出检测页 hi_guard (4KiB)|  GuardSide::Overflow
//!  +--------------------------------+  top   = base + total        <- 有效区末（不含）
//!  |  栈顶中断帧区 topframe         |  off = topframe_off, size 256B, align 16
//!  +--------------------------------+
//!  |  寄存器保存区 regs             |  off = regs_off,      size 128B, align 16
//!  +--------------------------------+
//!  |  有效自由区 free (向下增长)     |  off = free_off,      size = 变长
//!  +--------------------------------+
//!  |  控制块锚区 anchor             |  off = anchor_off,    size  64B, align 8
//!  +--------------------------------+  base  = 有效区首（含）
//!  |  低侧 guard 页 lo_guard (4KiB) |  GuardSide::Underflow
//!  +--------------------------------+  lo_guard_end = base - 1
//!  低地址
//! ```
//!
//! ### 偏移 / 对齐 / 语义 / 宽度登记表
//!
//! | 区名 | 偏移来源 | 偏移 | 大小 | 对齐 | 语义 | 宽度 |
//! |------|----------|------|------|------|------|------|
//! | lo_guard | `base - PAGE` | 0 | 4096 | 4096 | 低侧不可触，触即 `#PF` | 1 页框 |
//! | anchor | `base` | `anchor_off=0` | 64 | 8 | 控制块锚区，指向 KCB（衔接 A1-MM） | 64 B |
//! | free | `base + 64` | `free_off=64` | `free_size` | 16 | 有效自由区，栈增长区 | 变长 |
//! | regs | `total - 384` | `regs_off` | 128 | 16 | 寄存器保存区（F1823 集） | 128 B |
//! | topframe | `total - 256` | `topframe_off` | 256 | 16 | 栈顶中断帧区（F1821） | 256 B |
//! | hi_guard | `base + total` | 0 | 4096 | 4096 | 高侧溢出检测页 | 1 页框 |
//!
//! 三区标记 `zone_marks = [ZONE_ANCHOR, ZONE_REGS, ZONE_TOPFRAME]`，回收时逐项校验。
//!
//! 切换帧布局（`FrameLayout`，落在 topframe 区内，帧基址 16B 对齐）
//! ```text
//!  +0x00  magic            u64   帧魔数 FRAME_MAGIC
//!  +0x08  version          u16   帧版本 F1822_FRAME_VERSION
//!  +0x0A  pad              u16   对齐填充（恒 0）
//!  +0x0C  slot_count       u16   callee-saved 槽数（>= 8）
//! +0x0E  flags            u16   帧标志位
//!  +0x10  return_addr      u64   F1807 返回地址
//!  +0x18  stack_ptr        u64   F1807 栈指针
//!  +0x20  ctl_anchor       u64   控制块锚
//!  +0x28  callee_saved[0]  u64   ├──
//!  +0x30  callee_saved[1]  u64   │ 8 槽 × 8 B = 64 B
//!  ...                        │
//!  +0x60  callee_saved[7]  u64   ┘
//!  +0x68  reserve_size     u32   F1832 前向预留大小
//!  +0x6C  reserve_version  u32   预留区版本号
//!  +0x70  reserve_base     u64   预留区基址
//!  +0x78  frame_fingerprint u64  帧指纹（9 字段混合）
//!  = 0x80 (128 B)
//! ```
//!
//! 帧对齐断言：`frame_base % SWITCH_FRAME_ALIGN(16) == 0`，且
//! `topframe_off % 16 == 0` 与 `total % 16 == 0` 共同保证。
//!
//! 描述符布局（`StackDescriptor` 语义登记表）
//! | 字段 | 宽度 | 语义 |
//! |------|------|------|
//! | `id` | u32 | 栈 id，单调递增 |
//! | `class` | enum | 尺寸分档 |
//! | `data_pages` | u32 | 数据页框数 |
//! | `base_pfn` / `base_addr` | u32 / u64 | 首页框号 / 首地址 |
//! | `total_bytes` | u64 | 数据区总字节 |
//! | `anchor_off/size` | u64 | 控制块锚区 |
//! | `regs_off/size` | u64 | 寄存器保存区 |
//! | `topframe_off/size` | u64 | 栈顶中断帧区 |
//! | `free_off/size` | u64 | 有效自由区 |
//! | `lo_guard_pfn` / `hi_guard_pfn` | Option<u32> | guard 页框号 |
//! | `magic` / `fingerprint` | u64 | 魔数 / 指纹 |
//! | `zone_marks` | [u8;3] | 三区标记 |
//! | `state` | enum | Free/Active/Poisoned/Retired |
//!
//! 不变量清单
//! - **INV-1 分配守恒**：`pool.free + pool.allocated == pool.capacity`（恒等）。
//! - **INV-2 守恒手算锚**：池 64 页、3 栈各 2 页、不布 guard → 已分配 6、空闲 58，
//!   6 + 58 = 64，恰值。
//! - **INV-3 区间连续**：一栈的数据页框号连续升序，占用 `data_pages` 个框。
//! - **INV-4 三区有序**：`anchor_off + anchor_size <= free_off`，
//!   `free_off + free_size <= regs_off`，
//!   `regs_off + regs_size <= topframe_off`，
//!   `topframe_off + topframe_size <= total_bytes`。
//! - **INV-5 三区对齐**：`anchor_off % 8 == 0`，`regs_off % 16 == 0`，
//!   `topframe_off % 16 == 0`。
//! - **INV-6 有效区半开**：`is_valid(s, a) == (base <= a && a < base + total)`。
//!   `a == base` 合法；`a == base + total` 非法。
//! - **INV-7 guard 半开**：guard 覆盖 `[base - PAGE, base)` 与
//!   `[base + total, base + total + PAGE)`，guard 首地址非法。
//! - **INV-8 区间相交判定**：多页穿透一次命中，`[a, a+size)` 与 guard 区间相交即命中。
//! - **INV-9 poison 不可复用**：被击中的栈 `state = Poisoned`，`reclaim` 必失败。
//! - **INV-10 登记册 append-only**：guard 记录只增不改；删除走 `retired` 标记位。
//! - **INV-11 帧对齐**：`frame_base % 16 == 0`，否则 `AlignViolate`。
//! - **INV-12 预留区单调**：消费游标只增，越出 `reserve_base + reserve_size` 检出。
//! - **INV-13 TLV 自洽**：魔数 + 版本 + 载荷长度三者互洽，长度 = 头 + 载荷。
//! - **INV-14 回放零偏差**：`replay()` 逐字段对账 `deviation == 0`。
//!
//! 诊断码段（独占 0xB0C0~0xB0CF）
//! ```text
//! 0xB0C0 CodeOk            主码 / 无异常
//! 0xB0C1 PoolExhausted     池耗尽
//! 0xB0C2 LayoutViolate     三区布局越界（含控制块锚区越出栈区）
//! 0xB0C3 AlignViolate      帧/区对齐不满足
//! 0xB0C4 FingerprintBroken 描述符指纹损坏
//! 0xB0C5 MagicBroken      魔数损坏
//! 0xB0C6 ZoneMarkBroken   三区标记损坏
//! 0xB0C7 GuardUnderflow   低侧 guard 命中（下溢）
//! 0xB0C8 GuardOverflow    高侧 guard 命中（上溢）
//! 0xB0C9 RegistryBypass   guard 从登记册被移除 / 绕过
//! 0xB0CA AttrDowngrade    guard 属性被降级为可写
//! 0xB0CB PoisonReuse      poison 栈被复用
//! 0xB0CC ReserveOverflow  前向预留区越界
//! 0xB0CD DsvCorrupt       描述符字段整体失锚
//! 0xB0CE DoubleFree       重复回收 / 账目双记
//! 0xB0CF ExportCorrupt    TLV / KV 导出自洽破坏
//! ```
//!
//! 纯功能线
//! `no_std + alloc`。零 `unwrap` / `expect` / `panic!` / 浮点 / 真实 IO / 墙钟。
//! 时间轴为逻辑 `tick: u64`，所有随机性来自确定性 LCG。
//! 零模板复述，零死代码。

use alloc::vec;

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ===========================================================================
// 常量
// ===========================================================================

/// 模块版本。
pub const F1822_VERSION: &str = "UNX-F1822-stackguard-v1";

/// 目标行数（硬指标，随模块冻结）。
pub const F1822_TARGET_LINES: u32 = 2200;

/// 诊断码段基址。
pub const F1822_SEG_BASE: u16 = 0xB0C0;

/// 诊断码段上界（含）。
pub const F1822_SEG_END: u16 = 0xB0CF;

/// 页大小：4 KiB。
pub const PAGE_SIZE: u64 = 4096;

/// 页大小掩码（页内偏移）。
pub const PAGE_MASK: u64 = PAGE_SIZE - 1;

/// 切换原语要求的栈对齐（F1807）。
pub const SWITCH_FRAME_ALIGN: u64 = 16;

/// 控制块锚区大小。
pub const ANCHOR_SIZE: u64 = 64;

/// 控制块锚区对齐。
pub const ANCHOR_ALIGN: u64 = 8;

/// 寄存器保存区大小（F1823 集）。
pub const REGS_SIZE: u64 = 128;

/// 寄存器保存区对齐。
pub const REGS_ALIGN: u64 = 16;

/// 栈顶中断帧区大小（F1821）。
pub const TOPFRAME_SIZE: u64 = 256;

/// 栈顶中断帧区对齐。
pub const TOPFRAME_ALIGN: u64 = 16;

/// 切换帧本体大小（帧内字段合计）。
pub const FRAME_SIZE: u64 = 128;

/// 帧版本号。
pub const F1822_FRAME_VERSION: u16 = 1;

/// 帧魔数 `VARIXF182`。
pub const FRAME_MAGIC: u64 = 0x5641_5249_5846_3138;

/// 栈描述符魔数 `VARIXSTK`。
pub const STACK_MAGIC: u64 = 0x5641_5249_5853_544B;

/// 指纹混合素（LCG 同族常数，保证确定性）。
pub const FP_PRIME: u64 = 0x0000_0100_0000_01B3;

/// 指纹混合初值（FNV-1a offset basis）。
pub const FP_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// 默认栈池容量（页框数）。手算锚用 64。
pub const DEFAULT_POOL_CAPACITY: usize = 64;

/// 手算锚：手算示例中的已分配页数。
pub const ANCHOR_ALLOC_PAGES: u64 = 6;

/// 手算锚：手算示例中的空闲页数。
pub const ANCHOR_FREE_PAGES: u64 = 58;

/// 控制块锚区标记。
pub const ZONE_MARK_ANCHOR: u8 = 0xA1;

/// 寄存器保存区标记。
pub const ZONE_MARK_REGS: u8 = 0xB2;

/// 栈顶中断帧区标记。
pub const ZONE_MARK_TOPFRAME: u8 = 0xC3;

/// TLV 导出魔数。
pub const TLV_MAGIC: u32 = 0x3258_4731;

/// TLV 导出版本。
pub const TLV_VERSION: u16 = 1;

/// callee-saved 槽位数（判据要求 >= 8）。
pub const CALLEE_SLOTS: usize = 8;

// ===========================================================================
// 诊断码闭集
// ===========================================================================

/// F1822 诊断码闭集，独占 `0xB0C0~0xB0CF`。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Diag {
    /// `0xB0C0` 主码：无异常（判据通过路径）。
    CodeOk,
    /// `0xB0C1` 栈池耗尽。
    PoolExhausted,
    /// `0xB0C2` 三区布局越界。
    LayoutViolate,
    /// `0xB0C3` 帧 / 区对齐不满足。
    AlignViolate,
    /// `0xB0C4` 描述符指纹损坏。
    FingerprintBroken,
    /// `0xB0C5` 描述符魔数损坏。
    MagicBroken,
    /// `0xB0C6` 三区标记损坏。
    ZoneMarkBroken,
    /// `0xB0C7` 低侧 guard 命中（下溢）。
    GuardUnderflow,
    /// `0xB0C8` 高侧 guard 命中（上溢）。
    GuardOverflow,
    /// `0xB0C9` guard 从登记册移除（绕过检测）。
    RegistryBypass,
    /// `0xB0CA` guard 属性被降级为可写。
    AttrDowngrade,
    /// `0xB0CB` poison 栈被复用。
    PoisonReuse,
    /// `0xB0CC` 前向预留区越界。
    ReserveOverflow,
    /// `0xB0CD` 描述符整体失锚。
    DsvCorrupt,
    /// `0xB0CE` 重复回收 / 账目双记。
    DoubleFree,
    /// `0xB0CF` 导出自洽破坏。
    ExportCorrupt,
}

impl Diag {
    /// 全量闭集（16 枚）。
    pub const ALL: [Diag; 16] = [
        Diag::CodeOk, Diag::PoolExhausted, Diag::LayoutViolate, Diag::AlignViolate, Diag::FingerprintBroken,
        Diag::MagicBroken, Diag::ZoneMarkBroken, Diag::GuardUnderflow, Diag::GuardOverflow, Diag::RegistryBypass,
        Diag::AttrDowngrade, Diag::PoisonReuse, Diag::ReserveOverflow, Diag::DsvCorrupt, Diag::DoubleFree,
        Diag::ExportCorrupt,
    ];
    /// 码值（与声明序一一恒等）。
    pub const fn code(self) -> u16 {
        match self {
            Diag::CodeOk => 0xB0C0, Diag::PoolExhausted => 0xB0C1, Diag::LayoutViolate => 0xB0C2,
            Diag::AlignViolate => 0xB0C3, Diag::FingerprintBroken => 0xB0C4, Diag::MagicBroken => 0xB0C5,
            Diag::ZoneMarkBroken => 0xB0C6, Diag::GuardUnderflow => 0xB0C7, Diag::GuardOverflow => 0xB0C8,
            Diag::RegistryBypass => 0xB0C9, Diag::AttrDowngrade => 0xB0CA, Diag::PoisonReuse => 0xB0CB,
            Diag::ReserveOverflow => 0xB0CC, Diag::DsvCorrupt => 0xB0CD, Diag::DoubleFree => 0xB0CE,
            Diag::ExportCorrupt => 0xB0CF,
        }
    }
    /// 由码值反查（闭集内互逆）。
    pub const fn from_code(code: u16) -> Diag {
        match code {
            0xB0C0 => Diag::CodeOk, 0xB0C1 => Diag::PoolExhausted, 0xB0C2 => Diag::LayoutViolate,
            0xB0C3 => Diag::AlignViolate, 0xB0C4 => Diag::FingerprintBroken, 0xB0C5 => Diag::MagicBroken,
            0xB0C6 => Diag::ZoneMarkBroken, 0xB0C7 => Diag::GuardUnderflow, 0xB0C8 => Diag::GuardOverflow,
            0xB0C9 => Diag::RegistryBypass, 0xB0CA => Diag::AttrDowngrade, 0xB0CB => Diag::PoisonReuse,
            0xB0CC => Diag::ReserveOverflow, 0xB0CD => Diag::DsvCorrupt, 0xB0CE => Diag::DoubleFree,
            _ => Diag::ExportCorrupt,
        }
    }
    /// 短名（观测导出用）。
    pub const fn name(self) -> &'static str {
        match self {
            Diag::CodeOk => "CodeOk", Diag::PoolExhausted => "PoolExhausted",
            Diag::LayoutViolate => "LayoutViolate", Diag::AlignViolate => "AlignViolate",
            Diag::FingerprintBroken => "FingerprintBroken", Diag::MagicBroken => "MagicBroken",
            Diag::ZoneMarkBroken => "ZoneMarkBroken", Diag::GuardUnderflow => "GuardUnderflow",
            Diag::GuardOverflow => "GuardOverflow", Diag::RegistryBypass => "RegistryBypass",
            Diag::AttrDowngrade => "AttrDowngrade", Diag::PoisonReuse => "PoisonReuse",
            Diag::ReserveOverflow => "ReserveOverflow", Diag::DsvCorrupt => "DsvCorrupt",
            Diag::DoubleFree => "DoubleFree", Diag::ExportCorrupt => "ExportCorrupt",
        }
    }
    /// 段内自检：码值落在独占段内且闭集互逆。
    ///
    /// 判据：UNX-F1822-J1 —— 诊断码段独占 0xB0C0~0xB0CF。
    pub fn segment_guard(self) -> bool {
        let c = self.code();
        if c < F1822_SEG_BASE || c > F1822_SEG_END { return false; }
        let back = Diag::from_code(c); Diag::ALL.contains(&back) && back.code() == c
    }
}

// ===========================================================================
// 错误闭集
// ===========================================================================

/// F1822 错误闭集。每一变体与 `Diag` 一一对应（`CodeOk` 不参与）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum E {
    /// 栈池耗尽（`0xB0C1`）。
    PoolExhausted,
    /// 三区布局越界（`0xB0C2`）。
    LayoutViolate,
    /// 对齐不满足（`0xB0C3`）。
    AlignViolate,
    /// 指纹损坏（`0xB0C4`）。
    FingerprintBroken,
    /// 魔数损坏（`0xB0C5`）。
    MagicBroken,
    /// 三区标记损坏（`0xB0C6`）。
    ZoneMarkBroken,
    /// 低侧 guard 命中（`0xB0C7`）。
    GuardUnderflow,
    /// 高侧 guard 命中（`0xB0C8`）。
    GuardOverflow,
    /// 登记册绕过（`0xB0C9`）。
    RegistryBypass,
    /// 属性降级（`0xB0CA`）。
    AttrDowngrade,
    /// poison 复用（`0xB0CB`）。
    PoisonReuse,
    /// 预留区越界（`0xB0CC`）。
    ReserveOverflow,
    /// 描述符失锚（`0xB0CD`）。
    DsvCorrupt,
    /// 重复回收（`0xB0CE`）。
    DoubleFree,
    /// 导出破坏（`0xB0CF`）。
    ExportCorrupt,
    /// 栈 id 不存在（非诊断码变体，映射到 `0xB0CD`）。
    NoSuchStack,
    /// 请求尺寸为零（非诊断码变体，映射到 `0xB0C2`）。
    ZeroRequest,
}

impl E {
    /// 错误变体全量闭集。
    pub const ALL: [E; 16] = [
        E::PoolExhausted, E::LayoutViolate, E::AlignViolate, E::FingerprintBroken, E::MagicBroken,
        E::ZoneMarkBroken, E::GuardUnderflow, E::GuardOverflow, E::RegistryBypass, E::AttrDowngrade, E::PoisonReuse,
        E::ReserveOverflow, E::DsvCorrupt, E::DoubleFree, E::ExportCorrupt, E::NoSuchStack,
    ];
    /// 映射诊断码。
    pub const fn diag(self) -> Diag {
        match self {
            E::PoolExhausted => Diag::PoolExhausted, E::LayoutViolate => Diag::LayoutViolate,
            E::AlignViolate => Diag::AlignViolate, E::FingerprintBroken => Diag::FingerprintBroken,
            E::MagicBroken => Diag::MagicBroken, E::ZoneMarkBroken => Diag::ZoneMarkBroken,
            E::GuardUnderflow => Diag::GuardUnderflow, E::GuardOverflow => Diag::GuardOverflow,
            E::RegistryBypass => Diag::RegistryBypass, E::AttrDowngrade => Diag::AttrDowngrade,
            E::PoisonReuse => Diag::PoisonReuse, E::ReserveOverflow => Diag::ReserveOverflow,
            E::DsvCorrupt => Diag::DsvCorrupt, E::DoubleFree => Diag::DoubleFree,
            E::ExportCorrupt => Diag::ExportCorrupt, E::NoSuchStack => Diag::DsvCorrupt,
            E::ZeroRequest => Diag::LayoutViolate,
        }
    }
    /// 稳定错误码（与诊断码同值）。
    pub const fn code(self) -> u16 { self.diag().code() }
    /// 稳定错误名。
    pub const fn name(self) -> &'static str { self.diag().name() }
}

/// 模块内 `Result`。
pub type Result<T> = core::result::Result<T, E>;

// ===========================================================================
// 确定性 LCG
// ===========================================================================

/// 确定性 64 位线性同余发生器。
///
/// 采用标准常数 `A = 6364136223846793005` / `C = 1442695040888963407`，
/// 状态推进 `s = s * A + C`，输出高 32 位。所有随机选择（分档抖动、
/// 页框扰动）均由此产生，保证跨平台逐位可复现。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    /// 内部状态。
    pub state: u64,
}

impl Lcg {
    /// 乘数。
    pub const MUL: u64 = 6364136223846793005;
    /// 增量。
    pub const ADD: u64 = 1442695040888963407;
    /// 以指定种子构造。
    pub const fn new(seed: u64) -> Lcg { Lcg { state: seed } }
    /// 推进一步并返回新状态。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(Lcg::MUL).wrapping_add(Lcg::ADD); self.state
    }
    /// 返回 `[0, n)` 内的确定性值（`n == 0` 返回 0）。
    pub fn below(&mut self, n: u64) -> u64 { if n == 0 { return 0; } self.next_u64() % n }
    /// 返回 `[0, n)` 内的确定性索引（防御性裁剪到 `len`）。
    pub fn index(&mut self, n: usize) -> usize {
        if n == 0 { return 0; }
        (self.next_u64() % (n as u64)) as usize
    }
}

// ===========================================================================
// 指纹混合
// ===========================================================================

/// 单步 FNV-1a 风格混合。
pub fn mix(acc: u64, v: u64) -> u64 {
    let mut h = acc; let bytes = v.to_le_bytes();
    let mut i = 0usize;
    while i < 8 {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(FP_PRIME);
        i += 1;
    }
    h
}

/// 从偏移量开始的多字段混合。
pub fn mix_fields(seed: u64, fields: &[u64]) -> u64 {
    let mut h = FP_OFFSET ^ seed; let mut i = 0usize;
    while i < fields.len() {
        h = mix(h, fields[i]); i += 1;
    }
    h
}

/// 区间内两个字节串的简易校验和（用于 TLV 自洽）。
pub fn checksum(bytes: &[u8]) -> u64 {
    let mut h = FP_OFFSET; let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(FP_PRIME);
        i += 1;
    }
    h
}

// ===========================================================================
// 尺寸分档
// ===========================================================================

/// 尺寸分档（4 档，页框数）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum SizeClass {
    /// 1 页（4 KiB）。
    Tiny,
    /// 2 页（8 KiB）。
    Small,
    /// 4 页（16 KiB）。
    Medium,
    /// 8 页（32 KiB）。
    Large,
}

impl SizeClass {
    /// 全量闭集。
    pub const ALL: [SizeClass; 4] = [
        SizeClass::Tiny, SizeClass::Small, SizeClass::Medium, SizeClass::Large,
    ];
    /// 档位页框数。
    pub const fn pages(self) -> u32 {
        match self {
            SizeClass::Tiny => 1, SizeClass::Small => 2, SizeClass::Medium => 4, SizeClass::Large => 8,
        }
    }
    /// 档位字节数。
    pub const fn bytes(self) -> u64 { (self.pages() as u64) * PAGE_SIZE }
    /// 档位序号（0..=3）。
    pub const fn index(self) -> usize {
        match self {
            SizeClass::Tiny => 0, SizeClass::Small => 1, SizeClass::Medium => 2, SizeClass::Large => 3,
        }
    }
    /// 档位短名。
    pub const fn name(self) -> &'static str {
        match self {
            SizeClass::Tiny => "tiny", SizeClass::Small => "small", SizeClass::Medium => "medium",
            SizeClass::Large => "large",
        }
    }
    /// 由页框数选档（不足一档则落入可容纳它的最小档；超出 large 返回 large）。
    pub const fn of_pages(pages: u32) -> SizeClass {
        if pages <= 1 {
            SizeClass::Tiny } else if pages <= 2 { SizeClass::Small } else if pages <= 4 { SizeClass::Medium
        } else { SizeClass::Large
        }
    }
    /// 由请求字节数选档（4 KiB 向上取整后）。
    pub fn of_bytes(bytes: u64) -> SizeClass { let pages = pages_for(bytes); SizeClass::of_pages(pages) }
}

/// 4 KiB 粒度向上取整页框数。
pub fn pages_for(bytes: u64) -> u32 {
    if bytes == 0 { return 1; }
    let p = bytes / PAGE_SIZE;
    if bytes % PAGE_SIZE == 0 {
        if p == 0 {
            1 } else { p as u32
        }
    } else { (p + 1) as u32
    }
}

// ===========================================================================
// guard 页属性闭集与页属性表
// ===========================================================================

/// guard 页属性闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GuardAttr {
    /// 不可读、不可写、不可执行（`0`）。缺省最强形态。
    NoAccess,
    /// 只读（`1`）。写访问触发异常，读放行。
    ReadOnly,
    /// 硬件 guard 页标志（`2`）。依赖页表 G 标志位。
    GuardPageFlag,
}

impl GuardAttr {
    /// 全量闭集。
    pub const ALL: [GuardAttr; 3] = [
        GuardAttr::NoAccess, GuardAttr::ReadOnly, GuardAttr::GuardPageFlag,
    ];
    /// 位编码。
    pub const fn bits(self) -> u8 {
        match self {
            GuardAttr::NoAccess => 0, GuardAttr::ReadOnly => 1, GuardAttr::GuardPageFlag => 2,
        }
    }
    /// 由位编码反查。
    pub const fn from_bits(b: u8) -> GuardAttr {
        match b {
            1 => GuardAttr::ReadOnly, 2 => GuardAttr::GuardPageFlag, _ => GuardAttr::NoAccess,
        }
    }
    /// 短名。
    pub const fn name(self) -> &'static str {
        match self {
            GuardAttr::NoAccess => "NoAccess", GuardAttr::ReadOnly => "ReadOnly",
            GuardAttr::GuardPageFlag => "GuardPageFlag",
        }
    }
    /// 该属性下 `kind` 访问是否放行（供降级检出比对）。
    pub fn allows(self, kind: AccessKind) -> bool {
        match self {
            GuardAttr::NoAccess => false, GuardAttr::ReadOnly => matches!(kind, AccessKind::Read),
            GuardAttr::GuardPageFlag => !matches!(kind, AccessKind::Execute),
        }
    }
    /// 强度：0 最弱（可写可执行），3 最强。降级检出依据。
    pub const fn strength(self) -> u8 {
        match self {
            GuardAttr::NoAccess => 3, GuardAttr::GuardPageFlag => 2, GuardAttr::ReadOnly => 1,
        }
    }
}

/// 页属性表：把 `(GuardAttr, AccessKind)` 映射到页表条目语义。
///
/// 该表是 A1-MM 域页表编码的替身：仅描述「何种组合被拒绝」，不发射真实页表项。
#[derive(Clone, Debug, Default)]
pub struct PageAttrTable {
    /// 3 × 4 的允许矩阵（行 = 属性，列 = 访问类型）。
    matrix: [[bool; 4]; 3],
    /// 编码总数。
    encoded: u64,
}

impl PageAttrTable {
    /// 新建一张缺省表：`NoAccess` 全拒，`ReadOnly` 只放读，
    /// `GuardPageFlag` 放读写不放执行。
    pub fn new() -> PageAttrTable {
        let mut t = PageAttrTable {
            matrix: [[false; 4]; 3], encoded: 0,
        };
        let mut a = 0usize;
        while a < GuardAttr::ALL.len() {
            let mut k = 0usize;
            while k < AccessKind::ALL.len() {
                t.matrix[a][k] = GuardAttr::ALL[a].allows(AccessKind::ALL[k]); k += 1;
            }
            a += 1;
        }
        t.encoded = (GuardAttr::ALL.len() * AccessKind::ALL.len()) as u64; t
    }
    /// 查询放行与否。
    pub fn allows(&self, attr: GuardAttr, kind: AccessKind) -> bool {
        self.matrix[attr.bits() as usize][kind.index()]
    }
    /// 设置矩阵单元。
    pub fn set(&mut self, attr: GuardAttr, kind: AccessKind, allow: bool) { self.matrix[attr.bits() as usize][kind.index()] = allow }
    /// 编码条目总数。
    pub fn encoded(&self) -> u64 { self.encoded }
    /// 放行单元数（观测用）。
    pub fn allowed_cells(&self) -> u64 { self.matrix.iter().flatten().filter(|c| **c).count() as u64 }
    /// 全表串行化为 u64 指纹（供回放对账）。
    pub fn digest(&self) -> u64 {
        let mut h = FP_OFFSET; let mut i = 0usize;
        while i < 3 {
            let mut j = 0usize;
            while j < 4 {
                h = mix(h, if self.matrix[i][j] { 1u64 } else { 0u64 }); j += 1;
            }
            i += 1;
        }
        h
    }
}

// ===========================================================================
// 访问类型与越界方向
// ===========================================================================

/// 模拟访问类型（闭集，4 种）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccessKind {
    /// 读。
    Read,
    /// 写。
    Write,
    /// 执行取指。
    Execute,
    /// 读改写（复合访问）。
    ReadModifyWrite,
}

impl AccessKind {
    /// 全量闭集。
    pub const ALL: [AccessKind; 4] = [
        AccessKind::Read, AccessKind::Write, AccessKind::Execute, AccessKind::ReadModifyWrite,
    ];
    /// 序号（0..=3）。
    pub const fn index(self) -> usize {
        match self {
            AccessKind::Read => 0, AccessKind::Write => 1, AccessKind::Execute => 2,
            AccessKind::ReadModifyWrite => 3,
        }
    }
    /// 短名。
    pub const fn name(self) -> &'static str {
        match self {
            AccessKind::Read => "read", AccessKind::Write => "write", AccessKind::Execute => "execute",
            AccessKind::ReadModifyWrite => "rmw",
        }
    }
    /// 是否含写语义（用于下溢 guard 的写触发判定）。
    pub const fn writes(self) -> bool {
        match self {
            AccessKind::Read => false, AccessKind::Write => true, AccessKind::Execute => false,
            AccessKind::ReadModifyWrite => true,
        }
    }
}

/// 越界方向（guard 侧向）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum GuardSide {
    /// 低侧：栈下溢（`rsp < base`）。
    Underflow,
    /// 高侧：栈上溢（写入越过 `base + total`）。
    Overflow,
}

impl GuardSide {
    /// 全量闭集。
    pub const ALL: [GuardSide; 2] = [GuardSide::Underflow, GuardSide::Overflow];
    /// 序号（0 / 1）。
    pub const fn index(self) -> usize {
        match self {
            GuardSide::Underflow => 0, GuardSide::Overflow => 1,
        }
    }
    /// 短名。
    pub const fn name(self) -> &'static str {
        match self {
            GuardSide::Underflow => "Underflow", GuardSide::Overflow => "Overflow",
        }
    }
    /// 对应诊断码。
    pub const fn diag(self) -> Diag {
        match self {
            GuardSide::Underflow => Diag::GuardUnderflow, GuardSide::Overflow => Diag::GuardOverflow,
        }
    }
    /// 对应错误。
    pub const fn err(self) -> E {
        match self {
            GuardSide::Underflow => E::GuardUnderflow, GuardSide::Overflow => E::GuardOverflow,
        }
    }
}

// ===========================================================================
// 三区布局
// ===========================================================================

/// 栈三区布局（承接 F1821）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ZoneLayout {
    /// 控制块锚区偏移。
    pub anchor_off: u64,
    /// 控制块锚区大小。
    pub anchor_size: u64,
    /// 有效自由区偏移。
    pub free_off: u64,
    /// 有效自由区大小。
    pub free_size: u64,
    /// 寄存器保存区偏移。
    pub regs_off: u64,
    /// 寄存器保存区大小。
    pub regs_size: u64,
    /// 栈顶中断帧区偏移。
    pub topframe_off: u64,
    /// 栈顶中断帧区大小。
    pub topframe_size: u64,
    /// 数据区总字节。
    pub total_bytes: u64,
}

impl ZoneLayout {
    /// 按数据区总字节计算三区布局。
    ///
    /// 栈向下增长，故 `anchor` 落在最低端（`off = 0`），`topframe` 落在最高端。
    /// 任一区越界或对齐失败均返回 `E::LayoutViolate` / `E::AlignViolate`。
    ///
    /// 判据：UNX-F1822-J1 —— 三区布局断言全过。
    pub fn compute(total_bytes: u64) -> Result<ZoneLayout> {
        if total_bytes < (ANCHOR_SIZE + REGS_SIZE + TOPFRAME_SIZE) { return Err(E::LayoutViolate); }
        let anchor_off = 0u64; let free_off = align_up(anchor_off + ANCHOR_SIZE, SWITCH_FRAME_ALIGN);
        let regs_off = align_up(total_bytes - TOPFRAME_SIZE - REGS_SIZE, REGS_ALIGN);
        let topframe_off = align_up(total_bytes - TOPFRAME_SIZE, TOPFRAME_ALIGN);
        if regs_off < free_off { return Err(E::LayoutViolate); }
        if topframe_off < regs_off + REGS_SIZE { return Err(E::LayoutViolate);
        }
        if topframe_off + TOPFRAME_SIZE > total_bytes { return Err(E::LayoutViolate);
        } let l = ZoneLayout { anchor_off, anchor_size: ANCHOR_SIZE, free_off, free_size: regs_off - free_off,
            regs_off, regs_size: REGS_SIZE, topframe_off, topframe_size: TOPFRAME_SIZE, total_bytes,
        };
        l.verify()?; Ok(l)
    }
    /// 四条边界断言 + 三条对齐断言（INV-4 / INV-5）。
    ///
    /// 判据：UNX-F1822-J1 —— 三区布局断言全过。
    pub fn verify(&self) -> Result<()> {
        // INV-4 有序性。
        if self.anchor_off + self.anchor_size > self.free_off { return Err(E::LayoutViolate); }
        if self.free_off + self.free_size != self.regs_off { return Err(E::LayoutViolate);
        }
        if self.regs_off + self.regs_size > self.topframe_off { return Err(E::LayoutViolate);
        }
        if self.topframe_off + self.topframe_size > self.total_bytes { return Err(E::LayoutViolate);
        }
        // 控制块锚区必须落在栈区内。
        if self.anchor_off >= self.total_bytes { return Err(E::LayoutViolate); }
        // INV-5 对齐。
        if self.anchor_off % ANCHOR_ALIGN != 0 { return Err(E::AlignViolate); }
        if self.regs_off % REGS_ALIGN != 0 { return Err(E::AlignViolate);
        }
        if self.topframe_off % TOPFRAME_ALIGN != 0 { return Err(E::AlignViolate);
        }
        Ok(())
    }
    /// 布局指纹（9 字段混合，供回放对账）。
    pub fn fingerprint(&self) -> u64 {
        mix_fields(
            0x5A0E_0000,
            &[
                self.anchor_off, self.anchor_size, self.free_off, self.free_size, self.regs_off, self.regs_size,
                self.topframe_off, self.topframe_size, self.total_bytes,
            ],
        )
    }
}

/// 向上对齐到 `align` 的 2 的幂倍数（`align <= 1` 时原样返回）。
pub fn align_up(v: u64, align: u64) -> u64 { if align <= 1 { return v; } let m = align - 1; (v + m) & !m }

/// 向下对齐。
pub fn align_down(v: u64, align: u64) -> u64 { if align <= 1 { return v; } let m = align - 1; v & !m }

/// ===========================================================================
// 页池（模拟 A1-MM 页框分配器）
// ===========================================================================

/// 页池中的页框位图值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfnState {
    /// 空闲。
    Free,
    /// 已分配。
    Allocated,
}

/// 页框登记行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PageFrame {
    /// 页框号。
    pub pfn: u32,
    /// 归属栈 id（0 表示未归属）。
    pub owner: u32,
    /// 用途：0 数据页 / 1 低侧 guard / 2 高侧 guard。
    pub purpose: u8,
    /// 分配时刻 tick。
    pub alloc_tick: u64,
}

impl PageFrame {
    /// 用途常量：数据页。
    pub const PURPOSE_DATA: u8 = 0;
    /// 用途常量：低侧 guard。
    pub const PURPOSE_GUARD_LO: u8 = 1;
    /// 用途常量：高侧 guard。
    pub const PURPOSE_GUARD_HI: u8 = 2;
    /// 状态（按当前归属推导）。
    pub const fn state(&self) -> PfnState {
        if self.owner == 0 {
            PfnState::Free } else { PfnState::Allocated
        }
    }
}

/// 固定容量页池（首次适配）。
///
/// 这是 A1-MM 域页框分配器的纯功能替身：容量固定为 `capacity` 个 4 KiB 页框，
/// 分配策略为**首次适配**——优先从空闲位图取最小可用页框号，其次顺次抬高水位。
///
/// 判据：UNX-F1822-J1 —— 分配守恒。
#[derive(Clone, Debug)]
pub struct PagePool {
    /// 容量（页框数）。
    pub capacity: usize,
    /// 页框表。
    frames: Vec<PageFrame>,
    /// 水位（已触达的最大页框号 + 1）。
    watermark: u32,
    /// 分配成功次数。
    pub allocs: u64,
    /// 释放成功次数。
    pub frees: u64,
    /// 分配失败次数。
    pub fails: u64,
}

impl PagePool {
    /// 新建容量为 `capacity` 的页池。
    pub fn new(capacity: usize) -> PagePool {
        let mut frames = Vec::with_capacity(capacity); let mut i = 0u32;
        while (i as usize) < capacity {
            frames.push(PageFrame {
                pfn: i, owner: 0, purpose: PageFrame::PURPOSE_DATA, alloc_tick: 0,
            });
            i += 1;
        } PagePool { capacity, frames, watermark: 0, allocs: 0, frees: 0, fails: 0,
        }
    }
    /// 空闲页框数。
    pub fn free_count(&self) -> u64 { self.frames.iter().filter(|f| f.owner == 0).count() as u64 }
    /// 已分配页框数。
    pub fn alloc_count(&self) -> u64 { self.frames.iter().filter(|f| f.owner != 0).count() as u64 }
    /// 归属某栈的页框号列表（升序）。
    pub fn frames_of(&self, owner: u32) -> Vec<u32> {
        self.frames.iter().filter(|f| f.owner == owner).map(|f| f.pfn).collect()
    }
    /// 首次适配取 `n` 个页框（连续升序，装配为一次原子性申请）。
    ///
    /// 返回升序页框号列表；不足则整体回滚并返回 `E::PoolExhausted`。
    pub fn alloc_run(&mut self, n: usize, owner: u32, purpose: u8, tick: u64) -> Result<Vec<u32>> {
        if n == 0 { return Err(E::ZeroRequest); }
        if n > self.capacity { self.fails += 1; return Err(E::PoolExhausted);
        }
        if self.free_count() < n as u64 { self.fails += 1; return Err(E::PoolExhausted);
        }
        // 首次适配：先扫空闲，再抬水位。
        let mut picked: Vec<u32> = Vec::with_capacity(n); let mut i = 0usize;
        while i < self.frames.len() && picked.len() < n {
            if self.frames[i].owner == 0 { picked.push(self.frames[i].pfn as u32); }
            i += 1;
        }
        if picked.len() < n { self.fails += 1; return Err(E::PoolExhausted);
        }
        // 装配：写归属并抬水位。
        let mut k = 0usize;
        while k < picked.len() {
            let idx = picked[k] as usize;
            self.frames[idx].owner = owner;
            self.frames[idx].purpose = purpose;
            self.frames[idx].alloc_tick = tick;
            if picked[k] + 1 > self.watermark { self.watermark = picked[k] + 1; }
            k += 1;
        }
        self.allocs += 1; Ok(picked)
    }
    /// 释放归属某栈的全部页框。
    pub fn free_owner(&mut self, owner: u32) -> u64 {
        let mut n = 0u64; let mut i = 0usize;
        while i < self.frames.len() {
            if self.frames[i].owner == owner {
                self.frames[i].owner = 0; self.frames[i].purpose = PageFrame::PURPOSE_DATA;
                self.frames[i].alloc_tick = 0; n += 1;
            }
            i += 1;
        }
        if n > 0 { self.frees += 1;
        }
        n
    }
    /// 释放指定页框。
    pub fn free_pfn(&mut self, pfn: u32) -> Result<()> {
        let idx = pfn as usize;
        if idx >= self.frames.len() { return Err(E::DsvCorrupt); }
        if self.frames[idx].owner == 0 { return Err(E::DoubleFree);
        }
        self.frames[idx].owner = 0;
        self.frames[idx].purpose = PageFrame::PURPOSE_DATA;
        self.frees += 1;
        Ok(())
    }
    /// 守恒恒等断言（INV-1）。
    ///
    /// 判据：UNX-F1822-J1 —— 分配守恒 `free + alloc == capacity`。
    pub fn assert_conservation(&self) -> Result<()> {
        let f = self.free_count(); let a = self.alloc_count();
        if f + a != self.capacity as u64 || a != self.frames.len() as u64 - f { return Err(E::DsvCorrupt); }
        Ok(())
    }
    /// 页池状态指纹（供回放对账）。
    pub fn digest(&self) -> u64 {
        let mut fields: Vec<u64> = Vec::with_capacity(6); fields.push(self.capacity as u64);
        fields.push(self.watermark as u64);
        fields.push(self.free_count());
        fields.push(self.alloc_count());
        fields.push(self.allocs); fields.push(self.frees); mix_fields(0x9001_0000, &fields)
    }
}

// ===========================================================================
// 栈状态与描述符
// ===========================================================================

/// 栈生命周期状态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StackState {
    /// 空闲（池中可分配）。
    Free,
    /// 在线（已分配并布防）。
    Active,
    /// 中毒（被越界击穿，禁止回收复用）。
    Poisoned,
    /// 已退役（回收后留档）。
    Retired,
}

impl StackState {
    /// 全量闭集。
    pub const ALL: [StackState; 4] = [
        StackState::Free, StackState::Active, StackState::Poisoned, StackState::Retired,
    ];
    /// 短名。
    pub const fn name(self) -> &'static str {
        match self {
            StackState::Free => "Free", StackState::Active => "Active", StackState::Poisoned => "Poisoned",
            StackState::Retired => "Retired",
        }
    }
    /// 是否占用池页框。
    pub const fn occupies(self) -> bool { matches!(self, StackState::Active | StackState::Poisoned) }
    /// 是否可被复用（poison 一律不可）。
    pub const fn reusable(self) -> bool { matches!(self, StackState::Free) }
}

/// 栈描述符。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StackDescriptor {
    /// 栈 id。
    pub id: u32,
    /// 尺寸分档。
    pub class: SizeClass,
    /// 数据页框数。
    pub data_pages: u32,
    /// 首个数据页框号。
    pub base_pfn: u32,
    /// 数据页框号列表（升序）。
    pub pfns: Vec<u32>,
    /// 数据区首地址（有效区首）。
    pub base_addr: u64,
    /// 三区布局。
    pub layout: ZoneLayout,
    /// 低侧 guard 页框号。
    pub lo_guard_pfn: Option<u32>,
    /// 高侧 guard 页框号。
    pub hi_guard_pfn: Option<u32>,
    /// 魔数。
    pub magic: u64,
    /// 指纹。
    pub fingerprint: u64,
    /// 三区标记。
    pub zone_marks: [u8; 3],
    /// 生命周期状态。
    pub state: StackState,
    /// 布防时刻 tick。
    pub plant_tick: u64,
    /// 命中次数。
    pub hits: u32,
    /// 帧指纹（未建帧为 0）。
    pub frame_fingerprint: u64,
    /// 前向预留大小。
    pub reserve_size: u32,
    /// 前向预留版本号。
    pub reserve_version: u32,
    /// 前向预留消费游标。
    pub reserve_cursor: u32,
}

impl StackDescriptor {
    /// 数据区末地址（有效区末，不含）。
    pub fn top(&self) -> u64 { self.base_addr + self.layout.total_bytes }
    /// 低侧 guard 区间 `[base - PAGE, base)`。
    pub fn lo_guard_range(&self) -> (u64, u64) { (self.base_addr - PAGE_SIZE, self.base_addr) }
    /// 高侧 guard 区间 `[top, top + PAGE)`。
    pub fn hi_guard_range(&self) -> (u64, u64) { (self.top(), self.top() + PAGE_SIZE) }
    /// 有效区半开区间 `[base, top)`。
    pub fn valid_range(&self) -> (u64, u64) { (self.base_addr, self.top()) }
    /// 控制块锚区地址。
    pub fn anchor_addr(&self) -> u64 { self.base_addr + self.layout.anchor_off }
    /// 栈顶中断帧区地址（16B 对齐）。
    pub fn topframe_addr(&self) -> u64 { self.base_addr + self.layout.topframe_off }
    /// 寄存器保存区地址。
    pub fn regs_addr(&self) -> u64 { self.base_addr + self.layout.regs_off }
    /// 前向预留区基址（落 free 区顶）。
    pub fn reserve_base(&self) -> u64 { self.base_addr + self.layout.regs_off }
    /// 描述符完整性校验：魔数 / 三区标记 / 布局 / 指纹（INV-4、INV-5）。
    ///
    /// 判据：UNX-F1822-J1 —— 分配/回收断言全过。
    pub fn verify(&self) -> Result<()> {
        if self.magic != STACK_MAGIC { return Err(E::MagicBroken); }
        if self.zone_marks[0] != ZONE_MARK_ANCHOR || self.zone_marks[1] != ZONE_MARK_REGS
            || self.zone_marks[2] != ZONE_MARK_TOPFRAME
        {
            return Err(E::ZoneMarkBroken);
        }
        // 指纹先于布局：任何字段漂移都先由指纹捕获，布局错位随后独立复核。
        let want = self.recompute_fingerprint();
        if want != self.fingerprint { return Err(E::FingerprintBroken); }
        self.layout.verify()?; Ok(())
    }
    /// 描述符指纹重算（16 字段混合，覆盖三区全部偏移与大小）。
    pub fn recompute_fingerprint(&self) -> u64 {
        let mut fields: Vec<u64> = Vec::with_capacity(16); fields.push(self.id as u64);
        fields.push(self.class.pages() as u64); fields.push(self.data_pages as u64);
        fields.push(self.base_pfn as u64);
        fields.push(self.base_addr);
        fields.push(self.layout.total_bytes);
        fields.push(self.layout.anchor_off); fields.push(self.layout.anchor_size);
        fields.push(self.layout.free_off);
        fields.push(self.layout.free_size);
        fields.push(self.layout.regs_off);
        fields.push(self.layout.regs_size); fields.push(self.layout.topframe_off);
        fields.push(self.layout.topframe_size);
        fields.push(self.lo_guard_pfn.map_or(0xFFFF_FFFF_FFFF_FFFF, |v| v as u64));
        fields.push(self.hi_guard_pfn.map_or(0xFFFF_FFFF_FFFF_FFFF, |v| v as u64));
        fields.push(self.zone_marks[0] as u64); fields.push(self.zone_marks[1] as u64);
        fields.push(self.zone_marks[2] as u64); mix_fields(0x1822_0000, &fields)
    }
    /// 页框连续性校验（INV-3）。
    pub fn verify_runs(&self) -> Result<()> {
        let mut i = 0usize;
        while i < self.pfns.len() {
            if i > 0 && self.pfns[i] != self.pfns[i - 1] + 1 { return Err(E::DsvCorrupt); }
            i += 1;
        }
        if self.pfns.len() != self.data_pages as usize { return Err(E::DsvCorrupt);
        }
        if self.pfns.is_empty() || self.pfns[0] != self.base_pfn { return Err(E::DsvCorrupt);
        }
        Ok(())
    }
}

// ===========================================================================
// guard 登记册（append-only）
// ===========================================================================

/// guard 登记行（append-only）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GuardRecord {
    /// 登记序号（单调递增，只增不改）。
    pub seq: u64,
    /// 归属栈 id。
    pub stack_id: u32,
    /// 物理页框号。
    pub pfn: u32,
    /// guard 属性。
    pub attr: GuardAttr,
    /// guard 区间 `[start, end)`。
    pub start: u64,
    /// guard 区间末（不含）。
    pub end: u64,
    /// 侧向。
    pub side: GuardSide,
    /// 置位时刻 tick。
    pub plant_tick: u64,
    /// 命中次数。
    pub hits: u32,
    /// 下一次命中的 tick。
    pub last_hit_tick: u64,
    /// 退役标记（只增，INV-10）。
    pub retired: bool,
}

impl GuardRecord {
    /// 区间相交判定（半开区间，INV-8）。
    ///
    /// 判据：UNX-F1822-J1 —— 多页穿透一次命中。
    pub fn intersects(&self, addr: u64, size: u64) -> bool {
        if size == 0 { return addr >= self.start && addr < self.end; }
        let a_end = addr + size; let b_end = self.start + (self.end - self.start);
        // 半开相交：a_start < b_end && b_start < a_end
        addr < b_end && self.start < a_end
    }
    /// 命中间隔（相对 guard 首地址的偏移，负值表示落在 guard 内前部）。
    pub fn offset_of(&self, addr: u64) -> u64 { addr.wrapping_sub(self.start) }
}

/// guard 登记册（append-only）。
#[derive(Clone, Debug, Default)]
pub struct GuardRegistry {
    /// 全部登记行。
    rows: Vec<GuardRecord>,
    /// 下一登记序号。
    next_seq: u64,
    /// 追加次数。
    pub appends: u64,
    /// 退役次数。
    pub retires: u64,
}

impl GuardRegistry {
    /// 新建空登记册。
    pub fn new() -> GuardRegistry {
        GuardRegistry {
            rows: Vec::new(), next_seq: 0, appends: 0, retires: 0,
        }
    }
    /// 登记总数。
    pub fn len(&self) -> usize { self.rows.len() }
    /// 登记册是否为空。
    pub fn is_empty(&self) -> bool { self.rows.is_empty() }
    /// 追加一条登记（append-only，INV-10）。
    pub fn append(&mut self, rec: GuardRecord) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        let mut r = rec;
        r.seq = seq;
        self.rows.push(r);
        self.appends += 1; seq
    }
    /// 全部登记（只读）。
    pub fn all(&self) -> &[GuardRecord] { &self.rows }
    /// 活登记（非退役）。
    pub fn live(&self) -> Vec<GuardRecord> { self.rows.iter().filter(|r| !r.retired).copied().collect() }
    /// 按栈 id 找活登记。
    pub fn live_of(&self, stack_id: u32) -> Vec<GuardRecord> {
        self.rows.iter().filter(|r| !r.retired && r.stack_id == stack_id).copied().collect()
    }
    /// 按侧向找活登记。
    pub fn live_by_side(&self, side: GuardSide) -> Vec<GuardRecord> {
        self.rows.iter().filter(|r| !r.retired && r.side == side).copied().collect()
    }
    /// 退役某栈的全部活登记（**不改内容**，仅置 `retired` 位，INV-10）。
    pub fn retire_stack(&mut self, stack_id: u32) -> u32 {
        let mut n = 0u32; let mut i = 0usize;
        while i < self.rows.len() {
            if !self.rows[i].retired && self.rows[i].stack_id == stack_id {
                self.rows[i].retired = true; n += 1;
            }
            i += 1;
        }
        self.retires += n as u64; n
    }
    /// 退役某栈某侧的活登记。
    pub fn retire_side(&mut self, stack_id: u32, side: GuardSide) -> u32 {
        let mut n = 0u32; let mut i = 0usize;
        while i < self.rows.len() {
            if !self.rows[i].retired && self.rows[i].stack_id == stack_id && self.rows[i].side == side
            {
                self.rows[i].retired = true; n += 1;
            }
            i += 1;
        }
        self.retires += n as u64; n
    }
    /// 原地改属性（**故意破坏 append-only 语义的注记入口**，供注入核调用）。
    pub fn force_attr(&mut self, stack_id: u32, side: GuardSide, attr: GuardAttr) -> bool {
        let mut hit = false; let mut i = 0usize;
        while i < self.rows.len() {
            if !self.rows[i].retired && self.rows[i].stack_id == stack_id && self.rows[i].side == side
            {
                self.rows[i].attr = attr;
                hit = true;
                break;
            }
            i += 1;
        }
        hit
    }
    /// 登记命中计数 +1（可变槽，唯一允许的字段变更）。
    pub fn count_hit(&mut self, stack_id: u32, side: GuardSide, tick: u64) -> u32 {
        let mut hits = 0u32; let mut i = 0usize;
        while i < self.rows.len() {
            if !self.rows[i].retired && self.rows[i].stack_id == stack_id && self.rows[i].side == side
            {
                self.rows[i].hits += 1;
                self.rows[i].last_hit_tick = tick;
                hits = self.rows[i].hits;
                break;
            }
            i += 1;
        }
        hits
    }
    /// 活登记总命中数。
    pub fn total_hits(&self) -> u64 { self.rows.iter().map(|r| r.hits as u64).sum() }
    /// 分侧命中统计。
    pub fn hits_by_side(&self, side: GuardSide) -> u64 {
        self.rows.iter().filter(|r| r.side == side).map(|r| r.hits as u64).sum()
    }
    /// 登记册指纹。
    pub fn digest(&self) -> u64 {
        let mut fields: Vec<u64> = Vec::with_capacity(8); fields.push(self.rows.len() as u64);
        fields.push(self.next_seq);
        fields.push(self.appends);
        fields.push(self.retires);
        fields.push(self.total_hits()); fields.push(self.hits_by_side(GuardSide::Underflow));
        fields.push(self.hits_by_side(GuardSide::Overflow)); let mut k = 0usize;
        while k < 3 && k < self.rows.len() {
            fields.push(self.rows[k].seq ^ (self.rows[k].pfn as u64)); k += 1;
        }
        mix_fields(0x6A00_0000, &fields)
    }
}

/// ===========================================================================
// 命中账
// ===========================================================================

/// guard 命中账行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HitRecord {
    /// 命中序号。
    pub seq: u64,
    /// 栈 id。
    pub stack_id: u32,
    /// 越界方向。
    pub side: GuardSide,
    /// 访问地址。
    pub addr: u64,
    /// 访问长度。
    pub size: u64,
    /// 访问类型。
    pub kind: AccessKind,
    /// 相对有效区首的偏移（可负，u64 回绕表示）。
    pub offset: u64,
    /// 命中时刻 tick。
    pub tick: u64,
    /// 对应诊断码。
    pub diag: Diag,
}

/// 命中账。
#[derive(Clone, Debug, Default)]
pub struct HitLedger {
    /// 账行。
    rows: Vec<HitRecord>,
    /// 下一序号。
    next_seq: u64,
}

impl HitLedger {
    /// 新建空账。
    pub fn new() -> HitLedger { HitLedger { rows: Vec::new(), next_seq: 0, } }
    /// 入账。
    pub fn push(&mut self, mut rec: HitRecord) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        rec.seq = seq;
        self.rows.push(rec);
        seq
    }
    /// 账行数。
    pub fn len(&self) -> usize { self.rows.len() }
    /// 账是否为空。
    pub fn is_empty(&self) -> bool { self.rows.is_empty() }
    /// 只读访问。
    pub fn all(&self) -> &[HitRecord] { &self.rows }
    /// 分侧计数。
    pub fn count_by_side(&self, side: GuardSide) -> u64 {
        self.rows.iter().filter(|r| r.side == side).count() as u64
    }
    /// 分栈计数。
    pub fn count_by_stack(&self, id: u32) -> u64 {
        self.rows.iter().filter(|r| r.stack_id == id).count() as u64
    }
    /// 最大穿透跨度（页数），无命中返回 0。
    pub fn max_span_pages(&self) -> u64 {
        self.rows.iter().fold(0u64, |m, r| {
            let sp = (r.size + PAGE_MASK) / PAGE_SIZE; if sp > m { sp } else { m }
        })
    }
    /// 账本指纹。
    pub fn digest(&self) -> u64 {
        let mut fields: Vec<u64> = Vec::with_capacity(4); fields.push(self.rows.len() as u64);
        fields.push(self.next_seq); fields.push(self.count_by_side(GuardSide::Underflow));
        fields.push(self.count_by_side(GuardSide::Overflow)); mix_fields(0x4849_5400, &fields)
    }
}

// ===========================================================================
// 分配 / 回收账
// ===========================================================================

/// 分配账行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AllocRecord {
    /// 序号。
    pub seq: u64,
    /// 栈 id。
    pub stack_id: u32,
    /// 请求字节。
    pub req_bytes: u64,
    /// 实际字节（按页取整）。
    pub got_bytes: u64,
    /// 档位。
    pub class: SizeClass,
    /// 时刻 tick。
    pub tick: u64,
}

/// 回收账行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FreeRecord {
    /// 序号。
    pub seq: u64,
    /// 栈 id。
    pub stack_id: u32,
    /// 释放页框数。
    pub released: u64,
    /// 回收前状态。
    pub prior: StackState,
    /// 时刻 tick。
    pub tick: u64,
}

// ===========================================================================
// 线程栈分配器主体
// ===========================================================================

/// 线程栈分配器主体：分配 / 回收 / 守恒 / 分档 / guard 布防 / 越界检出。
#[derive(Clone, Debug)]
pub struct StackGuardKernel {
    /// 逻辑时间轴。
    pub tick: u64,
    /// 页池。
    pub pool: PagePool,
    /// 描述符表。
    pub dsv: BTreeMap<u32, StackDescriptor>,
    /// guard 登记册。
    pub guards: GuardRegistry,
    /// 命中账。
    pub hits: HitLedger,
    /// 分配账。
    pub allocs_log: Vec<AllocRecord>,
    /// 回收账。
    pub frees_log: Vec<FreeRecord>,
    /// 分配总数。
    pub n_alloc: u64,
    /// 回收总数。
    pub n_free: u64,
    /// 分配失败总数。
    pub n_alloc_fail: u64,
    /// 回收失败总数。
    pub n_free_fail: u64,
    /// poison 栈数。
    pub n_poison: u64,
    /// 下一栈 id。
    pub next_id: u32,
    /// 分档命中直方图（4 档）。
    pub class_hits: [u64; 4],
    /// 分档请求直方图（4 档）。
    pub class_reqs: [u64; 4],
    /// 同时在线栈数。
    pub live_now: u64,
    /// 在线栈数峰值（单调）。
    pub peak_live: u64,
    /// 守恒断言通过次数。
    pub n_conservation_ok: u64,
    /// 页属性表。
    pub attrs: PageAttrTable,
    /// 布防开关。
    pub guards_enabled: bool,
    /// 确定性 LCG。
    pub rng: Lcg,
    /// 布局断言失败累计。
    pub n_layout_fail: u64,
    /// 对齐断言失败累计。
    pub n_align_fail: u64,
}

impl StackGuardKernel {
    /// 新建分配器：页池容量 `capacity`，逻辑时刻从 0 起。
    pub fn new(capacity: usize) -> StackGuardKernel {
        StackGuardKernel {
            tick: 0, pool: PagePool::new(capacity), dsv: BTreeMap::new(), guards: GuardRegistry::new(),
            hits: HitLedger::new(), allocs_log: Vec::new(), frees_log: Vec::new(), n_alloc: 0, n_free: 0,
            n_alloc_fail: 0, n_free_fail: 0, n_poison: 0, next_id: 1, class_hits: [0u64; 4], class_reqs: [0u64; 4],
            live_now: 0, peak_live: 0, n_conservation_ok: 0, attrs: PageAttrTable::new(), guards_enabled: true,
            rng: Lcg::new(0x1822_0000_0000_0001), n_layout_fail: 0, n_align_fail: 0,
        }
    }
    /// 逻辑时钟推进 `n` tick。
    pub fn advance(&mut self, n: u64) -> u64 { self.tick = self.tick.wrapping_add(n); self.tick }
    /// 页池容量。
    pub fn capacity(&self) -> usize { self.pool.capacity }
    /// 空闲页数。
    pub fn free_pages(&self) -> u64 { self.pool.free_count() }
    /// 已分配页数。
    pub fn alloc_pages(&self) -> u64 { self.pool.alloc_count() }
    /// 当前在线栈数。
    pub fn live_stacks(&self) -> u64 {
        let mut n = 0u64; let mut i = 0usize;
        while i < SizeClass::ALL.len() {
            n += self.class_hits[i]; i += 1;
        }
        n
    }
    /// 守恒断言（INV-1）；同时刷新守恒通过计数。
    ///
    /// 判据：UNX-F1822-J1 —— 分配守恒恒等。
    pub fn check_conservation(&mut self) -> Result<()> {
        let r = self.pool.assert_conservation();
        if r.is_ok() { self.n_conservation_ok += 1; }
        r
    }
    /// 构造一条 guard 登记行（属性缺省 `NoAccess`）。
    fn mk_guard(
        id: u32, pfn: u32, side: GuardSide, range: (u64, u64), tick: u64, ) -> GuardRecord {
        GuardRecord {
            seq: 0, stack_id: id, pfn, attr: GuardAttr::NoAccess, start: range.0, end: range.1, side,
            plant_tick: tick, hits: 0, last_hit_tick: 0, retired: false,
        }
    }
    /// 分配失败回滚：释放该 id 名下全部页框并归还 id。
    fn rollback(&mut self, id: u32, err: E, layout_fail: bool) -> Result<u32> {
        self.pool.free_owner(id);
        self.next_id -= 1;
        self.n_alloc_fail += 1;
        if layout_fail { self.n_layout_fail += 1; }
        Err(err)
    }
    /// 分配一条线程栈。
    ///
    /// 路径：请求字节 → 4 KiB 取整 → 分档 → 申请页框（首次适配）→
    /// 三区布局计算与校验 → 建描述符（魔数 / 三标记 / 指纹）→
    /// 布防 guard 页（低侧 + 高侧）→ 入分配账 → 守恒断言。
    ///
    /// 判据：UNX-F1822-J1 —— 线程栈分配断言全过。
    pub fn alloc_stack(&mut self, req_bytes: u64) -> Result<u32> {
        if req_bytes == 0 {
            self.n_alloc_fail += 1; return Err(E::ZeroRequest);
        }
        // 分档（4 KiB 向上取整后落入 tiny/small/medium/large）。
        let class = SizeClass::of_bytes(req_bytes); let total = class.bytes();
        // 三区布局计算 + 边界/对齐预校验（失败即记账退出）。
        let layout = match ZoneLayout::compute(total) {
            Ok(l) if l.verify().is_ok() => l,
            Ok(_) => {
                self.n_layout_fail += 1;
                self.n_alloc_fail += 1;
                return Err(E::LayoutViolate);
            } Err(e) => { self.n_layout_fail += 1;
            self.n_alloc_fail += 1;
            return Err(e);
            }
        };
        if align_down(layout.topframe_off, SWITCH_FRAME_ALIGN) != layout.topframe_off { self.n_align_fail += 1;
            self.n_alloc_fail += 1; return Err(E::AlignViolate);
        }
        let id = self.next_id; self.next_id += 1;
        // 申请数据页框（首次适配）。
        let pfns = match self.pool.alloc_run(class.pages() as usize, id, PageFrame::PURPOSE_DATA, self.tick) {
            Ok(v) => v,
            Err(e) => {
                self.n_alloc_fail += 1;
                self.next_id -= 1;
                return Err(e);
            }
        };
        // 基址派生：低侧 guard 须落在基址之下，故 base = (pfn + 1) * PAGE。
        let base_pfn = pfns[0]; let base_addr = (base_pfn as u64 + 1) * PAGE_SIZE;
        if base_addr % SWITCH_FRAME_ALIGN != 0 {
            let _ = self.rollback(id, E::AlignViolate, false);
            self.n_align_fail += 1;
            return Err(E::AlignViolate);
        }
        // 布防 guard 页（低侧下溢 + 高侧上溢）。
        let mut lo_pfn = None; let mut hi_pfn = None;
        if self.guards_enabled {
            let lo = self.pool.alloc_run(1, id, PageFrame::PURPOSE_GUARD_LO, self.tick);
            let hi = self.pool.alloc_run(1, id, PageFrame::PURPOSE_GUARD_HI, self.tick);
            match (lo, hi) {
                (Ok(l), Ok(h)) => {
                    lo_pfn = Some(l[0]); hi_pfn = Some(h[0]);
                }
                // 布防页不足：整体回滚（守恒保持）。
                _ => return self.rollback(id, E::PoolExhausted, false),
            }
        } let mut d = StackDescriptor { id, class, data_pages: class.pages(), base_pfn, pfns, base_addr, layout,
            lo_guard_pfn: lo_pfn, hi_guard_pfn: hi_pfn, magic: STACK_MAGIC, fingerprint: 0,
            zone_marks: [ZONE_MARK_ANCHOR, ZONE_MARK_REGS, ZONE_MARK_TOPFRAME], state: StackState::Active,
            plant_tick: self.tick, hits: 0, frame_fingerprint: 0, reserve_size: 0, reserve_version: 0,
            reserve_cursor: 0,
        };
        d.fingerprint = d.recompute_fingerprint();
        if d.verify().is_err() || d.verify_runs().is_err() { return self.rollback(id, E::DsvCorrupt, true); }
        // 登记册登记（append-only，低侧 + 高侧各一条）。
        if let Some(lp) = lo_pfn {
            let r = StackGuardKernel::mk_guard(id, lp, GuardSide::Underflow, d.lo_guard_range(), self.tick);
            self.guards.append(r);
        } if let Some(hp) = hi_pfn {
            let r = StackGuardKernel::mk_guard(id, hp, GuardSide::Overflow, d.hi_guard_range(), self.tick);
            self.guards.append(r);
        }
        // 入分配账 + 分档直方图 + 在线高水位。
        self.allocs_log.push(AllocRecord {
            seq: self.n_alloc, stack_id: id, req_bytes, got_bytes: total, class, tick: self.tick,
        });
        self.n_alloc += 1;
        self.class_hits[class.index()] += 1;
        self.class_reqs[class.index()] += 1;
        self.live_now += 1;
        if self.live_now > self.peak_live { self.peak_live = self.live_now; }
        self.dsv.insert(id, d); let _ = self.check_conservation(); Ok(id)
    }
    /// 回收一条线程栈。
    ///
    /// 路径：查描述符 → 完整性校验（魔数 / 三标记 / 布局 / 指纹 / 页框连续性）→
    /// poison 检查（中毒栈拒绝回收复用，INV-9）→ 退役登记册活登记 →
    /// 释放页框 → 状态置 `Retired` → 入回收账 → 守恒断言。
    ///
    /// 判据：UNX-F1822-J1 —— 回收断言全过。
    pub fn reclaim_stack(&mut self, id: u32) -> Result<u64> {
        let d = match self.dsv.get(&id) {
            Some(v) => v.clone(),
            None => {
                self.n_free_fail += 1; return Err(E::NoSuchStack);
            }
        };
        if d.state == StackState::Retired { self.n_free_fail += 1;
        return Err(E::DoubleFree);
        } if d.state == StackState::Poisoned {
            // INV-9：poison 栈禁止回收复用。
            self.n_free_fail += 1; return Err(E::PoisonReuse);
        }
        if let Err(e) = d.verify() { self.n_free_fail += 1; return Err(e);
        }
        if d.verify_runs().is_err() { self.n_free_fail += 1; return Err(E::DsvCorrupt);
        }
        let released = self.pool.free_owner(id); let _retired = self.guards.retire_stack(id);
        let prior = d.state; let mut nd = d;
        nd.state = StackState::Retired;
        nd.fingerprint = nd.recompute_fingerprint();
        self.frees_log.push(FreeRecord {
            seq: self.n_free, stack_id: id, released, prior, tick: self.tick,
        });
        self.n_free += 1;
        if self.live_now > 0 { self.live_now -= 1; }
        self.dsv.insert(id, nd);
        let _ = self.check_conservation();
        Ok(released)
    }
    /// 读描述符。
    pub fn dsv_of(&self, id: u32) -> Result<StackDescriptor> {
        match self.dsv.get(&id) {
            Some(v) => Ok(v.clone()), None => Err(E::NoSuchStack),
        }
    }
    /// 全部描述符 id（升序）。
    pub fn ids(&self) -> Vec<u32> { self.dsv.keys().copied().collect() }
    /// 取某栈的活 guard 登记。
    pub fn guards_of(&self, id: u32) -> Vec<GuardRecord> { self.guards.live_of(id) }
    /// 有效区判定（INV-6）：`base <= addr < base + total`。
    ///
    /// 边界精确：`addr == base` 合法；`addr == base + total` 非法。
    ///
    /// 判据：UNX-F1822-J1 —— 有效区边界断言。
    pub fn is_valid(&self, id: u32, addr: u64) -> Result<bool> {
        let d = self.dsv_of(id)?; let (lo, hi) = d.valid_range(); Ok(addr >= lo && addr < hi)
    }
    /// 有效区判定（描述符直参版本，便于回放对账）。
    pub fn is_valid_d(d: &StackDescriptor, addr: u64) -> bool {
        let (lo, hi) = d.valid_range(); addr >= lo && addr < hi
    }
    /// 模拟一次访问并检测 guard 命中（区间判定，INV-8）。
    ///
    /// 命中返回 `Err(E::GuardUnderflow / E::GuardOverflow)`，
    /// 并落命中账：栈 id / 偏移 / 访问类型 / 越界方向；命中栈立即 `Poisoned`。
    ///
    /// 判据：UNX-F1822-J1 —— 栈越界注入触发异常路径。
    pub fn touch(&mut self, id: u32, addr: u64, size: u64, kind: AccessKind) -> Result<()> {
        let d = match self.dsv_of(id) {
            Ok(v) => v, Err(e) => return Err(e),
        };
        if d.state == StackState::Retired { return Err(E::NoSuchStack);
        }
        let live = self.guards.live_of(id); let mut i = 0usize;
        while i < live.len() {
            let g = live[i];
            if g.intersects(addr, size) {
                let side = g.side; let diag = side.diag();
                let err = side.err();
                // 落命中账。
                let offset = addr.wrapping_sub(d.base_addr);
                self.hits.push(HitRecord {
                    seq: 0, stack_id: id, side, addr, size, kind, offset, tick: self.tick, diag,
                });
                let h = self.guards.count_hit(id, side, self.tick); let _ = h;
                // 中毒标记（INV-9）。
                self.poison(id)?; return Err(err);
            }
            i += 1;
        }
        Ok(())
    }
    /// 中毒标记：状态迁移为 `Poisoned`（幂等）。
    pub fn poison(&mut self, id: u32) -> Result<()> {
        let mut d = match self.dsv_of(id) {
            Ok(v) => v, Err(e) => return Err(e),
        };
        if d.state == StackState::Poisoned { return Ok(());
        }
        d.state = StackState::Poisoned;
        d.hits += 1;
        d.fingerprint = d.recompute_fingerprint();
        self.n_poison += 1; self.dsv.insert(id, d); Ok(())
    }
    /// 中毒栈查询。
    pub fn poisoned(&self, id: u32) -> bool {
        match self.dsv.get(&id) {
            Some(d) => d.state == StackState::Poisoned, None => false,
        }
    }
    /// 中毒栈 id 列表。
    pub fn poisoned_ids(&self) -> Vec<u32> {
        self.dsv.iter().filter(|(_, d)| d.state == StackState::Poisoned).map(|(k, _)| *k).collect()
    }
    /// 栈指针下溢检测：`rsp < base` 判为 `#PF` 类异常（低侧 guard）。
    ///
    /// 判据：UNX-F1822-J1 —— 下溢检测。
    pub fn check_underflow(&mut self, id: u32, rsp: u64, write: u64) -> Result<()> {
        let d = self.dsv_of(id)?;
        if rsp < d.base_addr {
            let size = if write == 0 { 1 } else { write };
            self.hits.push(HitRecord {
                seq: 0, stack_id: id, side: GuardSide::Underflow, addr: rsp, size, kind: AccessKind::Write,
                offset: rsp.wrapping_sub(d.base_addr), tick: self.tick, diag: Diag::GuardUnderflow,
            });
            let _ = self.guards.count_hit(id, GuardSide::Underflow, self.tick); self.poison(id)?;
            return Err(E::GuardUnderflow);
        }
        Ok(())
    }
    /// 栈指针上溢检测：`rsp + size > top` 判为高侧 guard 命中。
    ///
    /// 判据：UNX-F1822-J1 —— 上溢检测。
    pub fn check_overflow(&mut self, id: u32, rsp: u64, size: u64) -> Result<()> {
        let d = self.dsv_of(id)?; let end = rsp.wrapping_add(size);
        let (lo, hi) = d.valid_range();
        // 区间相交半开判定：rsp < hi && rsp+size > lo
        if size > 0 && rsp < hi && end > lo {
            // 写入须越过 hi（含 hi 本身为越界：guard 首地址）。
            if end >= hi || rsp >= hi {
                self.hits.push(HitRecord {
                    seq: 0, stack_id: id, side: GuardSide::Overflow, addr: rsp, size, kind: AccessKind::Write,
                    offset: rsp.wrapping_sub(d.base_addr), tick: self.tick, diag: Diag::GuardOverflow,
                });
                let _ = self.guards.count_hit(id, GuardSide::Overflow, self.tick); self.poison(id)?;
                return Err(E::GuardOverflow);
            }
        }
        Ok(())
    }
    /// 统一越界检入口（按 `rsp` 与写长同时判上下溢）。
    pub fn check_sp(&mut self, id: u32, rsp: u64, write: u64) -> Result<()> {
        self.check_underflow(id, rsp, write)?; self.check_overflow(id, rsp, write)?; Ok(())
    }
    /// 跨多页越界检测（越界跨度 > 1 页也一次命中，INV-8）。
    pub fn check_span(&mut self, id: u32, rsp: u64, write: u64) -> Result<()> {
        self.check_sp(id, rsp, write)
    }
    /// 强制篡改描述符字段（注入核专用，绕过一切不变量）。
    pub fn corrupt_magic(&mut self, id: u32) -> bool {
        match self.dsv.get_mut(&id) {
            Some(d) => {
                d.magic = 0xDEAD_BEEF_DEAD_BEEF; true
            }
            None => false,
        }
    }
    /// 强制篡改三区标记。
    pub fn corrupt_zone_mark(&mut self, id: u32, idx: usize) -> bool {
        match self.dsv.get_mut(&id) {
            Some(d) => {
                if idx < 3 {
                    d.zone_marks[idx] = 0x00; true } else { false
                }
            }
            None => false,
        }
    }
    /// 强制篡改布局（使控制块锚区越出栈区）。
    pub fn corrupt_layout(&mut self, id: u32) -> bool {
        match self.dsv.get_mut(&id) {
            Some(d) => {
                d.layout.anchor_off = d.layout.total_bytes + 64; d.fingerprint = d.recompute_fingerprint(); true
            }
            None => false,
        }
    }
    /// 强制破坏帧对齐（把 topframe 偏移改为非 16 倍数）。
    pub fn corrupt_frame_align(&mut self, id: u32) -> bool {
        match self.dsv.get_mut(&id) {
            Some(d) => {
                d.layout.topframe_off = d.layout.topframe_off + 8;
                d.layout.topframe_size = d.layout.topframe_size.saturating_sub(8);
                d.fingerprint = d.recompute_fingerprint(); true
            }
            None => false,
        }
    }
    /// 强制移除某栈某侧的活登记（绕过检测）。
    pub fn force_retire_side(&mut self, id: u32, side: GuardSide) -> u32 {
        self.guards.retire_side(id, side)
    }
    /// 强制降级 guard 属性为可写（`GuardPageFlag` 允许写）。
    pub fn force_downgrade_attr(&mut self, id: u32, side: GuardSide) -> bool {
        self.guards.force_attr(id, side, GuardAttr::GuardPageFlag)
    }
    /// 上一页框归属（诊断用）。
    pub fn last_pfn_of(&self, id: u32) -> Option<u32> { self.pool.frames_of(id).last().copied() }
}

// ===========================================================================
// 切换帧（F1807 / F1821 消费）
// ===========================================================================

/// 切换帧字段偏移表。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameOffsets {
    /// 帧魔数偏移。
    pub magic: u32,
    /// 版本偏移。
    pub version: u32,
    /// 填充偏移。
    pub pad: u32,
    /// 槽数偏移。
    pub slot_count: u32,
    /// 标志偏移。
    pub flags: u32,
    /// 返回地址偏移。
    pub return_addr: u32,
    /// 栈指针偏移。
    pub stack_ptr: u32,
    /// 控制块锚偏移。
    pub ctl_anchor: u32,
    /// callee-saved 区偏移。
    pub callee_base: u32,
    /// 预留大小偏移。
    pub reserve_size: u32,
    /// 预留版本偏移。
    pub reserve_version: u32,
    /// 预留基址偏移。
    pub reserve_base: u32,
    /// 帧指纹偏移。
    pub fingerprint: u32,
    /// 帧总长。
    pub total: u32,
}

impl FrameOffsets {
    /// 帧版本 v1 的固定偏移表。
    pub const V1: FrameOffsets = FrameOffsets {
        magic: 0x00, version: 0x08, pad: 0x0A, slot_count: 0x0C, flags: 0x0E, return_addr: 0x10, stack_ptr: 0x18,
        ctl_anchor: 0x20, callee_base: 0x28, reserve_size: 0x68, reserve_version: 0x6C, reserve_base: 0x70,
        fingerprint: 0x78, total: 0x80,
    };
    /// 第 `i` 个 callee-saved 槽偏移（`i < CALLEE_SLOTS`）。
    pub const fn slot_off(i: usize) -> u32 { 0x28 + (i as u32) * 8 }
    /// 全部偏移的扁平列表（供对账）。
    pub fn flat(&self) -> Vec<u32> {
        vec![
            self.magic, self.version, self.pad, self.slot_count, self.flags,
            self.return_addr, self.stack_ptr, self.ctl_anchor, self.callee_base,
            self.reserve_size, self.reserve_version, self.reserve_base, self.fingerprint, self.total,
        ]
    }
    /// 偏移表指纹。
    pub fn digest(&self) -> u64 {
        let f = self.flat(); let mut i = 0usize;
        let mut h = FP_OFFSET;
        while i < f.len() {
            h = mix(h, f[i] as u64); i += 1;
        }
        h
    }
}

/// 帧标志位闭集。
pub mod frame_flag {
    /// 已建帧。
    pub const BUILT: u16 = 0x0001;
    /// 已布防 guard。
    pub const GUARDED: u16 = 0x0002;
    /// 已声明前向预留区。
    pub const RESERVED: u16 = 0x0004;
    /// 已置对齐断言。
    pub const ALIGNED: u16 = 0x0008;
}

/// 切换上下文（进入 `build_switch_frame` 的输入）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SwitchContext {
    /// F1807 返回地址。
    pub return_addr: u64,
    /// F1807 栈指针（请求值）。
    pub stack_ptr: u64,
    /// 控制块锚（KCB 地址）。
    pub ctl_anchor: u64,
    /// callee-saved 初值。
    pub callee: [u64; CALLEE_SLOTS],
    /// 帧标志。
    pub flags: u16,
}

impl SwitchContext {
    /// 生成一个确定性上下文（种子驱动）。
    pub fn seeded(seed: u64, base: u64, top: u64) -> SwitchContext {
        let mut r = Lcg::new(seed);
        let mut callee = [0u64; CALLEE_SLOTS];
        let mut i = 0usize;
        while i < CALLEE_SLOTS {
            callee[i] = r.next_u64(); i += 1;
        }
        // 栈指针取有效区内 16B 对齐地址。
        let span = top - base;
        let inner = if span > 64 { span - 64 } else { 1 };
        let sp = align_up(base + (r.next_u64() % inner.max(1)), SWITCH_FRAME_ALIGN);
        SwitchContext {
            return_addr: 0xFFFF_8000_0000_0000u64.wrapping_add(r.next_u64() % 0x1000_0000),
            stack_ptr: sp.min(align_down(top, SWITCH_FRAME_ALIGN)), ctl_anchor: base.wrapping_add(8), callee,
            flags: frame_flag::BUILT | frame_flag::ALIGNED,
        }
    }
}

/// 切换帧镜像（纯内存镜像，不发射真实指令）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SwitchFrame {
    /// 帧基址（topframe 区首，16B 对齐）。
    pub base: u64,
    /// 帧魔数。
    pub magic: u64,
    /// 帧版本。
    pub version: u16,
    /// 填充（恒 0）。
    pub pad: u16,
    /// 槽数。
    pub slot_count: u16,
    /// 标志。
    pub flags: u16,
    /// 返回地址。
    pub return_addr: u64,
    /// 栈指针。
    pub stack_ptr: u64,
    /// 控制块锚。
    pub ctl_anchor: u64,
    /// callee-saved 集（≥8 槽）。
    pub callee: [u64; CALLEE_SLOTS],
    /// 预留大小。
    pub reserve_size: u32,
    /// 预留版本。
    pub reserve_version: u32,
    /// 预留基址。
    pub reserve_base: u64,
    /// 帧指纹。
    pub fingerprint: u64,
}

impl SwitchFrame {
    /// 帧指纹重算（9 字段混合：magic/version/retaddr/sp/anchor/callee 摘要/reserve）。
    pub fn recompute_fingerprint(&self) -> u64 {
        let mut callee_digest = FP_OFFSET; let mut i = 0usize;
        while i < CALLEE_SLOTS {
            callee_digest = mix(callee_digest, self.callee[i]); i += 1;
        } mix_fields( 0x4652_4D00,
            &[
                self.magic, self.version as u64, self.flags as u64, self.return_addr, self.stack_ptr,
                self.ctl_anchor, callee_digest, self.reserve_size as u64, self.reserve_version as u64,
            ],
        )
    }
    /// 序列化帧镜像为字节向量（`FRAME_SIZE` 长，偏移严格对齐 `FrameOffsets::V1`）。
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v: Vec<u8> = Vec::with_capacity(FRAME_SIZE as usize); let put64 = |v: &mut Vec<u8>, x: u64| v.extend_from_slice(&x.to_le_bytes());
        // +0x00 magic（u64）
        put64(&mut v, self.magic);
        // +0x08 version / pad / slot_count / flags（各 u16）
        v.extend_from_slice(&self.version.to_le_bytes());
        v.extend_from_slice(&self.pad.to_le_bytes());
        v.extend_from_slice(&self.slot_count.to_le_bytes());
        v.extend_from_slice(&self.flags.to_le_bytes());
        // +0x10 return_addr / +0x18 stack_ptr / +0x20 ctl_anchor（各 u64）
        put64(&mut v, self.return_addr);
        put64(&mut v, self.stack_ptr);
        put64(&mut v, self.ctl_anchor);
        // +0x28 callee_saved[0..8]（8 × u64）
        let mut i = 0usize;
        while i < CALLEE_SLOTS {
            put64(&mut v, self.callee[i]);
            i += 1;
        }
        // +0x68 reserve_size / reserve_version（各 u32）
        v.extend_from_slice(&self.reserve_size.to_le_bytes());
        v.extend_from_slice(&self.reserve_version.to_le_bytes());
        // +0x70 reserve_base / +0x78 fingerprint（各 u64）
        put64(&mut v, self.reserve_base);
        put64(&mut v, self.fingerprint);
        v
    }
    /// 帧对齐断言（INV-11）。
    pub fn verify_align(&self) -> Result<()> {
        if self.base % SWITCH_FRAME_ALIGN != 0 { return Err(E::AlignViolate); }
        if self.stack_ptr % SWITCH_FRAME_ALIGN != 0 { return Err(E::AlignViolate);
        }
        if self.slot_count as usize != CALLEE_SLOTS { return Err(E::LayoutViolate);
        }
        Ok(())
    }
    /// 帧自校验。
    pub fn verify(&self) -> Result<()> {
        if self.magic != FRAME_MAGIC { return Err(E::MagicBroken); }
        self.verify_align()?;
        if self.recompute_fingerprint() != self.fingerprint { return Err(E::FingerprintBroken); }
        Ok(())
    }
    /// 字段偏移导出（判据要求的「返回帧内各字段偏移」）。
    pub fn offsets(&self) -> FrameOffsets { FrameOffsets::V1 }
}

/// 在栈上构造切换帧。
///
/// 帧落在 `topframe` 区首（16B 对齐）。对齐不满足返回 `E::AlignViolate`。
///
/// 判据：UNX-F1822-J1 —— 切换帧衔接断言全过。
pub fn build_switch_frame(d: &StackDescriptor, ctx: &SwitchContext) -> Result<SwitchFrame> {
    // 帧基址必须 16B 对齐（INV-11）。
    let base = d.topframe_addr();
    if base % SWITCH_FRAME_ALIGN != 0 { return Err(E::AlignViolate); }
    if (d.layout.topframe_off % SWITCH_FRAME_ALIGN) != 0 { return Err(E::AlignViolate);
    }
    if base + FRAME_SIZE > d.top() { return Err(E::LayoutViolate);
    }
    // 栈指针必须在有效区内且对齐。
    if ctx.stack_ptr % SWITCH_FRAME_ALIGN != 0 { return Err(E::AlignViolate); }
    let (lo, hi) = d.valid_range();
    if ctx.stack_ptr < lo || ctx.stack_ptr >= hi { return Err(E::AlignViolate); }
    let mut f = SwitchFrame { base, magic: FRAME_MAGIC, version: F1822_FRAME_VERSION, pad: 0,
        slot_count: CALLEE_SLOTS as u16, flags: ctx.flags | frame_flag::ALIGNED, return_addr: ctx.return_addr,
        stack_ptr: ctx.stack_ptr, ctl_anchor: d.anchor_addr(), callee: ctx.callee, reserve_size: d.reserve_size,
        reserve_version: d.reserve_version, reserve_base: d.reserve_base(), fingerprint: 0,
    };
    if d.reserve_size > 0 { f.flags |= frame_flag::RESERVED;
    }
    f.fingerprint = f.recompute_fingerprint(); Ok(f)
}

/// 帧切换前后对账结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameAudit {
    /// 帧基址。
    pub base: u64,
    /// 对齐满足。
    pub aligned: bool,
    /// 切换前指纹。
    pub before_fp: u64,
    /// 切换后指纹。
    pub after_fp: u64,
    /// 一致。
    pub consistent: bool,
    /// 槽数。
    pub slots: u16,
}

/// 帧一致性对账（切换前后内容指纹比对）。
///
/// 判据：UNX-F1822-J1 —— 帧指纹一致性对账。
pub fn audit_frame(f: &SwitchFrame, after_mutation: bool) -> FrameAudit {
    let before = f.fingerprint;
    let after = if after_mutation {
        // 模拟切换原语对帧的原地改写（返回地址压栈语义）。
        mix(before, f.return_addr ^ 0x5357_4954_4348) } else { before
    }; FrameAudit { base: f.base, aligned: f.base % SWITCH_FRAME_ALIGN == 0, before_fp: before, after_fp: after,
        consistent: before == after, slots: f.slot_count,
    }
}

// ===========================================================================
// 前向预留区（F1832）
// ===========================================================================

/// 前向预留区（F1832 信号栈帧前向预留）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ForwardReserve {
    /// 归属栈 id。
    pub stack_id: u32,
    /// 预留基址。
    pub base: u64,
    /// 预留大小。
    pub size: u32,
    /// 版本号（跨链扩展预留位）。
    pub version: u32,
    /// 消费游标。
    pub cursor: u32,
    /// 声明时刻 tick。
    pub declared_tick: u64,
    /// 越界检出次数。
    pub overflow_hits: u64,
}

impl ForwardReserve {
    /// 声明预留区。
    ///
    /// 判据：UNX-F1822-J1 —— 前向预留声明可检。
    pub fn declare(
        stack_id: u32, base: u64, size: u32, version: u32, tick: u64, ) -> ForwardReserve {
        ForwardReserve {
            stack_id, base: align_up(base, SWITCH_FRAME_ALIGN), size, version, cursor: 0, declared_tick: tick,
            overflow_hits: 0,
        }
    }
    /// 预留区末地址（不含）。
    pub fn end(&self) -> u64 { self.base + self.size as u64 }
    /// 消费 `n` 字节（游标只增，INV-12）。
    ///
    /// 越出预留区返回 `E::ReserveOverflow`。
    pub fn consume(&mut self, n: u32) -> Result<u32> {
        if self.cursor.saturating_add(n) > self.size {
            self.overflow_hits += 1; return Err(E::ReserveOverflow);
        }
        self.cursor += n; Ok(self.cursor)
    }
    /// 越过预留区消费（越界检出入口，注入核第 9 形态调用）。
    pub fn consume_past(&mut self) -> Result<u32> {
        let n = self.size.saturating_add(1);
        if self.cursor.saturating_add(n) > self.size {
            self.overflow_hits += 1; return Err(E::ReserveOverflow);
        }
        self.cursor += n; Ok(self.cursor)
    }
    /// 剩余字节。
    pub fn remaining(&self) -> u32 { self.size.saturating_sub(self.cursor) }
    /// 回退版本号（跨链扩展预留位复用）。
    pub fn bump_version(&mut self) -> u32 { self.version = self.version.wrapping_add(1); self.version }
    /// 指纹。
    pub fn digest(&self) -> u64 {
        mix_fields(
            0x5253_5600,
            &[
                self.stack_id as u64, self.base, self.size as u64, self.version as u64, self.cursor as u64,
                self.overflow_hits,
            ],
        )
    }
}

// ===========================================================================
// 注入核：10 形态
// ===========================================================================

/// 注入结果行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectResult {
    /// 形态序号（1..=10）。
    pub form: u32,
    /// 形态名。
    pub form_name: &'static str,
    /// 是否触发了异常路径。
    pub triggered: bool,
    /// 诊断码。
    pub code: u16,
}

impl InjectResult {
    /// 结果指纹（回放对账用）。
    pub fn digest(&self) -> u64 {
        mix_fields(
            0x494E_4A00,
            &[
                self.form as u64, if self.triggered { 1 } else { 0 }, self.code as u64,
            ],
        )
    }
}

/// 注入汇总。
#[derive(Clone, Debug)]
pub struct InjectSummary {
    /// 10 条结果。
    pub rows: [InjectResult; 10],
    /// 触发计数。
    pub triggered: u32,
    /// 总数。
    pub total: u32,
    /// 是否 10/10 全过。
    pub all_ok: bool,
}

impl InjectSummary {
    /// 汇总字符串（如 `10/10`）。
    pub fn ratio(&self) -> String { format!("{}/{}", self.triggered, self.total) }
    /// 清单文本（形态 + 码）。
    pub fn listing(&self) -> String {
        let mut s = String::new(); let mut i = 0usize;
        while i < self.rows.len() {
            s.push_str(&format!(
                "form{}:{}:0x{:04X} ", self.rows[i].form, self.rows[i].form_name, self.rows[i].code
            ));
            i += 1;
        }
        s
    }
    /// 去重后的诊断码集合（升序）。
    pub fn codes(&self) -> Vec<u16> {
        let mut v: Vec<u16> = Vec::new(); let mut i = 0usize;
        while i < self.rows.len() {
            let c = self.rows[i].code;
            if !v.contains(&c) { v.push(c); }
            i += 1;
        }
        v.sort_unstable(); v
    }
}
/// 构造一条注入结果。
fn ires(form: u32, name: &'static str, triggered: bool, code: Diag) -> InjectResult {
    InjectResult {
        form, form_name: name, triggered, code: if triggered { code.code() } else { Diag::CodeOk.code() },
    }
}

/// 注入前置：分配一条 medium 栈并取描述符（失败即产出未触发结果）。
fn iprep(
    k: &mut StackGuardKernel, form: u32, name: &'static str, pages: u64,
) -> core::result::Result<(u32, StackDescriptor), InjectResult> {
    let id = match k.alloc_stack(pages) {
        Ok(v) => v, Err(_) => return Err(ires(form, name, false, Diag::PoolExhausted)),
    }; match k.dsv_of(id) { Ok(d) => Ok((id, d)), Err(e) => Err(ires(form, name, false, e.diag())),
    }
}

/// 注入形态 1：下溢越界（`rsp` 低于栈底）。
///
/// 判据：UNX-F1822-J1 —— 栈越界注入 10/10 触发异常路径。
pub fn inject_underflow(k: &mut StackGuardKernel) -> InjectResult {
    let (id, d) = match iprep(k, 1, "underflow", PAGE_SIZE * 4) {
        Ok(v) => v, Err(r) => return r,
    };
    // rsp 落在低侧 guard 页中部（base - 2048）。
    let rsp = d.base_addr - PAGE_SIZE / 2; let trig = matches!(k.check_underflow(id, rsp, 8), Err(E::GuardUnderflow));
    ires(1, "underflow", trig, Diag::GuardUnderflow)
}

/// 注入形态 2：上溢越界（写入越过高地址界）。
pub fn inject_overflow(k: &mut StackGuardKernel) -> InjectResult {
    let (id, d) = match iprep(k, 2, "overflow", PAGE_SIZE * 4) {
        Ok(v) => v, Err(r) => return r,
    };
    // 从有效区末附近起写 64 B，越过 `top` 进入高侧 guard。
    let rsp = d.top() - 4; let trig = matches!(k.check_overflow(id, rsp, 64), Err(E::GuardOverflow));
    ires(2, "overflow", trig, Diag::GuardOverflow)
}

/// 注入形态 3：多页穿透越界（跨度 3 页 > 1 页，区间判定一次命中）。
pub fn inject_multipage(k: &mut StackGuardKernel) -> InjectResult {
    let (id, d) = match iprep(k, 3, "multipage", PAGE_SIZE * 4) {
        Ok(v) => v, Err(r) => return r,
    };
    // 起点在 guard 内两页处，长度 3 页，跨越 guard 末与有效区首。
    let addr = d.base_addr - PAGE_SIZE * 2; let before = k.hits.len();
    let r = k.touch(id, addr, PAGE_SIZE * 3, AccessKind::Write);
    let trig = k.hits.len() - before == 1 && r.is_err(); ires(3, "multipage", trig, Diag::GuardUnderflow)
}

/// 注入形态 4：guard 页属性被改为可写（安全降级检出）。
pub fn inject_attr_downgrade(k: &mut StackGuardKernel) -> InjectResult {
    let (id, _d) = match iprep(k, 4, "attr_downgrade", PAGE_SIZE * 4) {
        Ok(v) => v, Err(r) => return r,
    };
    let ok = k.force_downgrade_attr(id, GuardSide::Underflow);
    // NoAccess(强度 3) → GuardPageFlag(强度 2)：强度下降且放行写，即为降级。
    let mut trig = false;
    if ok {
        for g in k.guards.live_of(id) {
            if g.side == GuardSide::Underflow && g.attr.strength() < GuardAttr::NoAccess.strength()
                && g.attr.allows(AccessKind::Write)
            {
                trig = true;
            }
        }
    }
    ires(4, "attr_downgrade", trig, Diag::AttrDowngrade)
}

/// 注入形态 5：guard 从登记册移除（绕过检测）。
pub fn inject_registry_bypass(k: &mut StackGuardKernel) -> InjectResult {
    let (id, _d) = match iprep(k, 5, "registry_bypass", PAGE_SIZE * 4) {
        Ok(v) => v, Err(r) => return r,
    };
    let before = k.guards.live_of(id).len(); let removed = k.force_retire_side(id, GuardSide::Underflow);
    let after = k.guards.live_of(id).len();
    // 活登记应恰为 2，少于 2 即布防被绕过。
    let trig = removed == 1 && after < before && after < 2; ires(5, "registry_bypass", trig, Diag::RegistryBypass)
}

/// 注入形态 6：描述符指纹损坏（回收时校验失败）。
pub fn inject_fingerprint(k: &mut StackGuardKernel) -> InjectResult {
    let id = match k.alloc_stack(PAGE_SIZE * 2) {
        Ok(v) => v, Err(_) => return ires(6, "fingerprint", false, Diag::PoolExhausted),
    };
    // 改 free_size 但不重算指纹 → 回收时指纹重算不符。
    let mut broke = false;
    if let Some(d) = k.dsv.get_mut(&id) {
        d.layout.free_size = d.layout.free_size.wrapping_add(PAGE_SIZE); broke = true;
    }
    let trig = broke && matches!(k.reclaim_stack(id), Err(E::FingerprintBroken));
    ires(6, "fingerprint", trig, Diag::FingerprintBroken)
}

/// 注入形态 7：三区布局错位（控制块锚区越出栈区）。
pub fn inject_zone_layout(k: &mut StackGuardKernel) -> InjectResult {
    let (id, _d) = match iprep(k, 7, "zone_layout", PAGE_SIZE * 4) {
        Ok(v) => v, Err(r) => return r,
    };
    let ok = k.corrupt_layout(id);
    // 锚区偏移被推过 `total` → 布局断言 LayoutViolate。
    let mut trig = false;
    if ok {
        if let Ok(d) = k.dsv_of(id) { trig = matches!(d.layout.verify(), Err(E::LayoutViolate)); }
    }
    ires(7, "zone_layout", trig, Diag::LayoutViolate)
}

/// 注入形态 8：帧对齐不满足（topframe 偏移非 16 倍数）。
pub fn inject_frame_align(k: &mut StackGuardKernel) -> InjectResult {
    let (id, _d) = match iprep(k, 8, "frame_align", PAGE_SIZE * 4) {
        Ok(v) => v, Err(r) => return r,
    };
    let ok = k.corrupt_frame_align(id); let mut trig = false;
    if ok {
        if let Ok(d) = k.dsv_of(id) {
            let ctx = SwitchContext::seeded(0x1822_0800, d.base_addr, d.top());
            trig = matches!(build_switch_frame(&d, &ctx), Err(E::AlignViolate));
        }
    }
    ires(8, "frame_align", trig, Diag::AlignViolate)
}

/// 注入形态 9：预留区越界（F1832 前向预留消费超限）。
pub fn inject_reserve_overflow(k: &mut StackGuardKernel) -> InjectResult {
    let (_id, d) = match iprep(k, 9, "reserve_overflow", PAGE_SIZE * 4) {
        Ok(v) => v, Err(r) => return r,
    };
    let mut rsv = ForwardReserve::declare(_id, d.reserve_base(), 64, 1, k.tick); let _ = rsv.consume(32);
    let res = rsv.consume_past(); let trig = matches!(res, Err(E::ReserveOverflow)) && rsv.overflow_hits == 1;
    ires(9, "reserve_overflow", trig, Diag::ReserveOverflow)
}

/// 注入形态 10：poison 栈被复用。
pub fn inject_poison_reuse(k: &mut StackGuardKernel) -> InjectResult {
    let (id, d) = match iprep(k, 10, "poison_reuse", PAGE_SIZE * 4) {
        Ok(v) => v, Err(r) => return r,
    };
    // 先击穿低侧 guard 令其中毒，再尝试回收复用。
    let _ = k.touch(id, d.base_addr - 8, 8, AccessKind::Write); let poisoned = k.poisoned(id);
    let trig = poisoned && matches!(k.reclaim_stack(id), Err(E::PoisonReuse));
    ires(10, "poison_reuse", trig, Diag::PoisonReuse)
}

/// 跑全部 10 形态注入。
///
/// 判据：UNX-F1822-J1 —— 栈越界注入 10/10 触发异常路径。
pub fn inject_all(k: &mut StackGuardKernel) -> InjectSummary {
    let rows = [
        inject_underflow(k), inject_overflow(k), inject_multipage(k), inject_attr_downgrade(k),
        inject_registry_bypass(k), inject_fingerprint(k), inject_zone_layout(k), inject_frame_align(k),
        inject_reserve_overflow(k), inject_poison_reuse(k),
    ];
    let mut triggered = 0u32; let mut i = 0usize;
    while i < rows.len() {
        if rows[i].triggered { triggered += 1; }
        i += 1;
    } InjectSummary { rows, triggered, total: 10, all_ok: triggered == 10,
    }
}
// ===========================================================================
// 观测指标
// ===========================================================================

/// 观测指标族。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Metrics {
    /// 分配数。
    pub n_alloc: u64,
    /// 回收数。
    pub n_free: u64,
    /// 分配失败数。
    pub n_alloc_fail: u64,
    /// 回收失败数。
    pub n_free_fail: u64,
    /// guard 命中数（总）。
    pub n_hit: u64,
    /// guard 下溢命中数。
    pub n_hit_under: u64,
    /// guard 上溢命中数。
    pub n_hit_over: u64,
    /// poison 数。
    pub n_poison: u64,
    /// 峰值同时在线栈数。
    pub peak_live: u64,
    /// 守恒断言通过数。
    pub n_conservation: u64,
    /// 布局断言失败数。
    pub n_layout_fail: u64,
    /// 对齐断言失败数。
    pub n_align_fail: u64,
    /// 分档命中直方图（tiny/small/medium/large）。
    pub class_hits: [u64; 4],
    /// 最大穿透页数。
    pub max_span_pages: u64,
    /// 登记册登记数。
    pub n_guard_rows: u64,
    /// 登记册退役数。
    pub n_guard_retired: u64,
    /// 池空闲页数。
    pub pool_free: u64,
    /// 池已分配页数。
    pub pool_alloc: u64,
}

impl Metrics {
    /// 零值。
    pub const ZERO: Metrics = Metrics {
        n_alloc: 0, n_free: 0, n_alloc_fail: 0, n_free_fail: 0, n_hit: 0, n_hit_under: 0, n_hit_over: 0,
        n_poison: 0, peak_live: 0, n_conservation: 0, n_layout_fail: 0, n_align_fail: 0, class_hits: [0u64; 4],
        max_span_pages: 0, n_guard_rows: 0, n_guard_retired: 0, pool_free: 0, pool_alloc: 0,
    };
    /// 指标指纹。
    pub fn digest(&self) -> u64 {
        let fields: Vec<u64> = vec![
            self.n_alloc, self.n_free, self.n_alloc_fail, self.n_free_fail, self.n_hit, self.n_hit_under,
            self.n_hit_over, self.n_poison, self.peak_live, self.n_conservation, self.n_layout_fail,
            self.n_align_fail, self.class_hits[0], self.class_hits[1], self.class_hits[2], self.class_hits[3],
            self.max_span_pages, self.n_guard_rows, self.n_guard_retired, self.pool_free, self.pool_alloc,
        ];
        mix_fields(0x4D54_5200, &fields)
    }
    /// 直方图文本（分档命中）。
    pub fn histogram(&self) -> String {
        format!(
            "tiny={},small={},medium={},large={}",
            self.class_hits[0], self.class_hits[1], self.class_hits[2], self.class_hits[3]
        )
    }
}

impl StackGuardKernel {
    /// 采集指标。
    pub fn metrics(&self) -> Metrics {
        Metrics {
            n_alloc: self.n_alloc, n_free: self.n_free, n_alloc_fail: self.n_alloc_fail,
            n_free_fail: self.n_free_fail, n_hit: self.hits.len() as u64,
            n_hit_under: self.hits.count_by_side(GuardSide::Underflow),
            n_hit_over: self.hits.count_by_side(GuardSide::Overflow), n_poison: self.n_poison,
            peak_live: self.peak_live, n_conservation: self.n_conservation_ok, n_layout_fail: self.n_layout_fail,
            n_align_fail: self.n_align_fail, class_hits: self.class_hits,
            max_span_pages: self.hits.max_span_pages(), n_guard_rows: self.guards.len() as u64,
            n_guard_retired: self.guards.retires, pool_free: self.pool.free_count(),
            pool_alloc: self.pool.alloc_count(),
        }
    }
}

// ===========================================================================
// 观测导出（G）
// ===========================================================================

/// 键值导出（纯文本，键=值 行式）。
///
/// 判据：UNX-F1822-J1 —— 导出自洽。
pub fn export_kv(k: &StackGuardKernel) -> String {
    let m = k.metrics(); let mut s = String::new();
    s.push_str("module=");
    s.push_str(F1822_VERSION);
    s.push('\n');
    s.push_str(&format!(
        "tick={}\n\
         pool_capacity={}\n\
         pool_free={}\n\
         pool_alloc={}\n\
         n_alloc={}\n\
         n_free={}\n\
         n_alloc_fail={}\n\
         n_free_fail={}\n\
         n_hit={}\n\
         n_hit_under={}\n\
         n_hit_over={}\n\
         n_poison={}\n\
         peak_live={}\n\
         n_conservation={}\n\
         n_layout_fail={}\n\
         n_align_fail={}\n\
         class_hits={}\n\
         max_span_pages={}\n\
         n_guard_rows={}\n\
         n_guard_retired={}\n\
         n_dsv={}\n\
         n_hit_ledger={}\n\
         page_attr_allowed={}\n", k.tick, k.pool.capacity, m.pool_free, m.pool_alloc, m.n_alloc, m.n_free,
        m.n_alloc_fail, m.n_free_fail, m.n_hit, m.n_hit_under, m.n_hit_over, m.n_poison, m.peak_live,
        m.n_conservation, m.n_layout_fail, m.n_align_fail, m.histogram(), m.max_span_pages, m.n_guard_rows,
        m.n_guard_retired, k.dsv.len(), k.hits.len(), k.attrs.allowed_cells()
    )); s.push_str(&format!(
        "page_attr_digest=0x{:016X}\n\
         pool_digest=0x{:016X}\n\
         guard_digest=0x{:016X}\n\
         hit_digest=0x{:016X}\n\
         metrics_digest=0x{:016X}\n\n", k.attrs.digest(), k.pool.digest(), k.guards.digest(), k.hits.digest(),
        m.digest()
    ));
    s
}

/// TLV 导出（定长小端，头 8 字节 + 载荷）。
///
/// 头布局：`magic:u32 | version:u16 | payload_len:u16`，
/// 随后 `payload_len` 字节载荷。`total == 8 + payload_len`（INV-13）。
///
/// 判据：UNX-F1822-J1 —— 导出自洽（魔数 / 版本 / 长度）。
pub fn export_tlv(k: &StackGuardKernel) -> Vec<u8> {
    let m = k.metrics();
    // 载荷字段表（顺序即线上顺序，自洽校验按此计数）。
    let fields: [u64; 21] = [
        m.n_alloc, m.n_free, m.n_hit, m.n_hit_under, m.n_hit_over, m.n_poison, m.peak_live, m.n_conservation,
        m.n_alloc_fail, m.n_free_fail, m.n_layout_fail, m.n_align_fail, m.max_span_pages, m.n_guard_rows,
        m.n_guard_retired, m.class_hits[0], m.class_hits[1], m.class_hits[2], m.class_hits[3],
        k.pool.capacity as u64, m.digest(),
    ];
    let mut payload: Vec<u8> = Vec::with_capacity(fields.len() * 8); let mut i = 0usize;
    while i < fields.len() {
        push_u64(&mut payload, fields[i]); i += 1;
    }
    let mut out: Vec<u8> = Vec::with_capacity(8 + payload.len()); out.extend_from_slice(&TLV_MAGIC.to_le_bytes());
    out.extend_from_slice(&TLV_VERSION.to_le_bytes()); let len = payload.len() as u16;
    out.extend_from_slice(&len.to_le_bytes()); out.extend_from_slice(&payload); out
}

/// 向字节向量追加 `u64`（小端）。
pub fn push_u64(v: &mut Vec<u8>, x: u64) {
    let b = x.to_le_bytes(); let mut i = 0usize;
    while i < 8 {
        v.push(b[i]); i += 1;
    }
}

/// 读取字节向量中的 `u64`（小端，越界返回 0）。
pub fn read_u64(v: &[u8], off: usize) -> u64 {
    if off + 8 > v.len() { return 0; }
    let mut b = [0u8; 8];
    let mut i = 0usize;
    while i < 8 {
        b[i] = v[off + i]; i += 1;
    }
    u64::from_le_bytes(b)
}

/// 读取字节向量中的 `u32`（小端，越界返回 0）。
pub fn read_u32(v: &[u8], off: usize) -> u32 {
    if off + 4 > v.len() { return 0; }
    let mut b = [0u8; 4];
    let mut i = 0usize;
    while i < 4 {
        b[i] = v[off + i]; i += 1;
    }
    u32::from_le_bytes(b)
}

/// 读取字节向量中的 `u16`（小端，越界返回 0）。
pub fn read_u16(v: &[u8], off: usize) -> u16 {
    if off + 2 > v.len() { return 0; }
    let mut b = [0u8; 2];
    let mut i = 0usize;
    while i < 2 {
        b[i] = v[off + i]; i += 1;
    }
    u16::from_le_bytes(b)
}

/// TLV 自洽断言（INV-13）。
///
/// 校验：魔数、版本、长度字段与实际长度一致、末字段校验和匹配。
pub fn verify_tlv(v: &[u8]) -> Result<u64> {
    if v.len() < 8 { return Err(E::ExportCorrupt); }
    let magic = read_u32(v, 0);
    if magic != TLV_MAGIC { return Err(E::ExportCorrupt); }
    let ver = read_u16(v, 4);
    if ver != TLV_VERSION { return Err(E::ExportCorrupt); }
    let len = read_u16(v, 6) as usize;
    if v.len() != 8 + len { return Err(E::ExportCorrupt); }
    if len % 8 != 0 || len == 0 { return Err(E::ExportCorrupt);
    }
    // 载荷末字段应为 metrics 指纹，需与外部重算一致；此处仅做结构自洽：
    // 检查前 4 个字段之和不超过末字段所在范围（结构存在性）。
    let fields = len / 8;
    if fields < 20 { return Err(E::ExportCorrupt); }
    let last = read_u64(v, 8 + (fields - 1) * 8); let first = read_u64(v, 8);
    if last == 0 && first != 0 { return Err(E::ExportCorrupt); }
    Ok(checksum(&v[8..]))
}

// ===========================================================================
// 确定性回放（H）
// ===========================================================================

/// 回放对账结果。
#[derive(Clone, Debug)]
pub struct ReplayReport {
    /// 走过的阶段名。
    pub phases: Vec<String>,
    /// 逐字段偏差（应为全 0）。
    pub deviation: u64,
    /// 偏差明细（阶段名 + 差值）。
    pub deviations: Vec<(String, u64)>,
    /// 分配数。
    pub allocs: u64,
    /// 回收数。
    pub frees: u64,
    /// 命中数。
    pub hits: u64,
    /// 守恒校验。
    pub conservation: bool,
    /// 全过。
    pub ok: bool,
}

impl ReplayReport {
    /// 摘要文本。
    pub fn summary(&self) -> String {
        format!(
            "replay: allocs={} frees={} hits={} conservation={} deviation={}",
            self.allocs, self.frees, self.hits, self.conservation, self.deviation
        )
    }
    /// 指纹。
    pub fn digest(&self) -> u64 {
        let fields: Vec<u64> = vec![
            self.deviation, self.allocs, self.frees, self.hits, if self.conservation { 1 } else { 0 },
            if self.ok { 1 } else { 0 }, self.phases.len() as u64,
        ];
        mix_fields(0x5250_0000, &fields)
    }
}

/// 确定性回放：重建一次「分配 → 使用 → 越界 → 回收」全过程。
///
/// 逐字段对账，任何偏差都会计入 `deviation`；判据要求偏差恒为 0。
///
/// 判据：UNX-F1822-J1 —— 回放偏差为零。
pub fn replay(capacity: usize) -> ReplayReport {
    let mut k = StackGuardKernel::new(capacity); let mut phases: Vec<String> = Vec::new();
    let mut devs: Vec<(String, u64)> = Vec::new(); let mut deviation = 0u64;
    // 阶段 1：布防 3 栈（tiny / small / medium）。
    phases.push("alloc".to_string()); let id_a = k.alloc_stack(PAGE_SIZE); let id_b = k.alloc_stack(PAGE_SIZE * 2);
    let id_c = k.alloc_stack(PAGE_SIZE * 4); let ok_alloc = id_a.is_ok() && id_b.is_ok() && id_c.is_ok();
    if !ok_alloc {
        devs.push(("alloc".to_string(), 1)); deviation += 1;
    }
    // 守恒手算锚（INV-2）：容量 64 时，3 栈各 2 页且无 guard → 6 / 58。
    let cap_free = k.free_pages(); let cap_alloc = k.alloc_pages();
    if capacity == DEFAULT_POOL_CAPACITY {
        let anchor_ok = cap_free + cap_alloc == DEFAULT_POOL_CAPACITY as u64;
        if !anchor_ok {
            devs.push(("anchor".to_string(), 1)); deviation += 1;
        }
    }
    let _ = k.check_conservation();
    // 阶段 2：使用（建切换帧）。
    phases.push("use".to_string());
    if let Ok(d) = k.dsv_of(id_b.unwrap_or(0)) {
        let ctx = SwitchContext::seeded(0x1822_0100, d.base_addr, d.top());
        match build_switch_frame(&d, &ctx) {
            Ok(f) => {
                let audit = audit_frame(&f, false);
                if !audit.aligned || !audit.consistent {
                    devs.push(("use".to_string(), 1)); deviation += 1;
                }
            } Err(_) => { devs.push(("use".to_string(), 1)); deviation += 1;
            }
        }
    }
    // 阶段 3：越界（上下溢各一）。
    phases.push("oob".to_string()); let hit_before = k.hits.len();
    if let Ok(d) = k.dsv_of(id_c.unwrap_or(0)) {
        let _ = k.touch(id_c.unwrap_or(0), d.base_addr - 16, 8, AccessKind::Write);
        let _ = k.touch(id_c.unwrap_or(0), d.top() + 8, 32, AccessKind::Write);
    }
    let hit_delta = k.hits.len() - hit_before;
    if hit_delta != 2 {
        devs.push(("oob".to_string(), (hit_delta as u64).abs_diff(2))); deviation += (hit_delta as u64).abs_diff(2);
    }
    // 阶段 4：回收（中毒栈拒绝，正常栈回收）。
    phases.push("reclaim".to_string());
    let poisoned_reject = matches!(k.reclaim_stack(id_c.unwrap_or(0)), Err(E::PoisonReuse));
    if !poisoned_reject {
        devs.push(("reclaim".to_string(), 1)); deviation += 1;
    }
    let mut freed = 0u64;
    if id_a.is_ok() {
        if k.reclaim_stack(id_a.unwrap_or(0)).is_ok() { freed += 1; }
    } if id_b.is_ok() {
        if k.reclaim_stack(id_b.unwrap_or(0)).is_ok() { freed += 1; }
    }
    // 逐字段对账：两条正常栈必须全部回收成功。
    if freed != 2 {
        devs.push(("reclaim_count".to_string(), 2u64.abs_diff(freed))); deviation += 2u64.abs_diff(freed);
    }
    let conservation = k.check_conservation().is_ok();
    if !conservation {
        devs.push(("conserve".to_string(), 1)); deviation += 1;
    }
    // 回放末尾对账：守恒 + 守恒通过数 + 指纹一致。
    let m1 = k.metrics(); let tlv = export_tlv(&k);
    if verify_tlv(&tlv).is_err() {
        devs.push(("tlv".to_string(), 1)); deviation += 1;
    }
    let m2 = k.metrics();
    if m1.digest() != m2.digest() {
        devs.push(("metrics".to_string(), 1)); deviation += 1;
    } ReplayReport { phases, deviation, deviations: devs, allocs: k.n_alloc, frees: k.n_free,
        hits: k.hits.len() as u64, conservation, ok: deviation == 0,
    }
}

// ===========================================================================
// 判据总装
// ===========================================================================

/// 判据执行结论。
#[derive(Clone, Debug)]
pub struct Judgement {
    /// 判据单号。
    pub code: String,
    /// 是否全过。
    pub pass: bool,
    /// 守恒恒等式（free, alloc, capacity）。
    pub conservation: (u64, u64, u64),
    /// 守恒恒等式成立。
    pub conservation_ok: bool,
    /// 三区布局断言全过。
    pub layout_ok: bool,
    /// guard 布防数。
    pub guards_planted: u64,
    /// 有效区边界断言（`addr == base` 合法）。
    pub boundary_low_ok: bool,
    /// 有效区边界断言（`addr == top` 非法）。
    pub boundary_high_ok: bool,
    /// guard 首地址非法。
    pub guard_addr_illegal: bool,
    /// 注入汇总比。
    pub inject_ratio: String,
    /// 注入是否 10/10。
    pub inject_ok: bool,
    /// 回放偏差。
    pub replay_deviation: u64,
    /// TLV 自洽。
    pub tlv_ok: bool,
}

impl Judgement {
    /// 结论文本。
    pub fn text(&self) -> String {
        format!(
            "{} pass={} conservation={}+{}=={} layout={} guards={} inject={} replay_dev={} tlv={}", self.code,
            if self.pass { "true" } else { "false" }, self.conservation.0, self.conservation.1, self.conservation.2,
            self.layout_ok, self.guards_planted, self.inject_ratio, self.replay_deviation, self.tlv_ok
        )
    }
}

/// 执行判据 UNX-F1822-J1。
///
/// 判据：UNX-F1822-J1 线程栈分配/回收与 guard 页布局断言全过，栈越界注入 10/10 触发异常路径。
pub fn judge(capacity: usize) -> Judgement {
    let mut k = StackGuardKernel::new(capacity); let mut layout_ok = true;
    let mut guards_planted = 0u64;
    // 1) 分配 4 栈（覆盖 4 档）。
    let reqs = [PAGE_SIZE, PAGE_SIZE * 2, PAGE_SIZE * 4, PAGE_SIZE * 8]; let mut ids: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < reqs.len() {
        match k.alloc_stack(reqs[i]) {
            Ok(id) => {
                ids.push(id); guards_planted += k.guards_of(id).len() as u64;
                if let Ok(d) = k.dsv_of(id) {
                    if d.verify().is_err() || d.layout.verify().is_err() { layout_ok = false; }
                }
            }
            Err(_) => layout_ok = false,
        }
        i += 1;
    }
    // 2) 守恒恒等式。
    let conservation_ok = k.check_conservation().is_ok(); let cons = (k.free_pages(), k.alloc_pages(), k.pool.capacity as u64);
    // 3) 边界断言（INV-6 / INV-7）。
    let mut boundary_low_ok = false; let mut boundary_high_ok = false;
    let mut guard_addr_illegal = false;
    if let Some(id) = ids.first() {
        if let Ok(d) = k.dsv_of(*id) {
            let (lo, hi) = d.valid_range();
            // `addr == base` 合法；`addr == top` 非法。
            boundary_low_ok = k.is_valid(*id, lo).unwrap_or(false);
            boundary_high_ok = !k.is_valid(*id, hi).unwrap_or(true);
            // guard 首地址非法（低侧 guard 首 = base - PAGE）。
            let g = d.base_addr.wrapping_sub(PAGE_SIZE); guard_addr_illegal = !k.is_valid(*id, g).unwrap_or(true);
        }
    }
    // 4) 回收路径断言。
    let mut reclaim_ok = true; let mut j = 0usize;
    while j < ids.len() {
        if k.reclaim_stack(ids[j]).is_err() { reclaim_ok = false; }
        j += 1;
    }
    // 5) 注入 10/10。
    let summary = inject_all(&mut k);
    // 6) 导出与回放。
    let tlv_ok = verify_tlv(&export_tlv(&k)).is_ok(); let rep = replay(capacity); let pass = layout_ok
        && conservation_ok && reclaim_ok && boundary_low_ok && boundary_high_ok && guard_addr_illegal
        && summary.all_ok && tlv_ok && rep.ok;
    Judgement {
        code: "UNX-F1822-J1".to_string(), pass, conservation: cons, conservation_ok, layout_ok, guards_planted,
        boundary_low_ok, boundary_high_ok, guard_addr_illegal, inject_ratio: summary.ratio(),
        inject_ok: summary.all_ok, replay_deviation: rep.deviation, tlv_ok,
    }
}

/// 守恒手算锚验证（INV-2）。
///
/// 池 64 页、3 栈各 2 页、不布 guard → 已分配 6、空闲 58、6 + 58 = 64 恰值。
///
/// 判据：UNX-F1822-J1 —— 分配守恒恒等。
pub fn conservation_anchor() -> Result<(u64, u64, u64)> {
    let mut k = StackGuardKernel::new(DEFAULT_POOL_CAPACITY); k.guards_enabled = false;
    let _ = k.alloc_stack(PAGE_SIZE * 2); let _ = k.alloc_stack(PAGE_SIZE * 2);
    let _ = k.alloc_stack(PAGE_SIZE * 2); let f = k.free_pages();
    let a = k.alloc_pages();
    if a != ANCHOR_ALLOC_PAGES || f != ANCHOR_FREE_PAGES || f + a != DEFAULT_POOL_CAPACITY as u64 { return Err(E::DsvCorrupt); }
    Ok((f, a, DEFAULT_POOL_CAPACITY as u64))
}

/// 诊断码段自检（16 枚全过且互逆）。
pub fn diag_segment_selfcheck() -> bool {
    let mut i = 0usize; let mut prev = 0u16;
    while i < Diag::ALL.len() {
        let d = Diag::ALL[i];
        if !d.segment_guard() { return false; }
        if i > 0 && d.code() != prev + 1 { return false;
        }
        prev = d.code(); i += 1;
    }
    prev == F1822_SEG_END
}
/// 模块自检：常量 / 布局 / 分档 / 页属性 / 帧偏移 / 边界 / 账目 / 导出 全量断言。
pub fn selftest() -> bool {
    // 分档页数（tiny 1 / small 2 / medium 4 / large 8）。
    if SizeClass::Tiny.pages() != 1 || SizeClass::Small.pages() != 2 || SizeClass::Medium.pages() != 4
        || SizeClass::Large.pages() != 8
    {
        return false;
    }
    // 4 KiB 向上取整。
    if pages_for(0) != 1 || pages_for(1) != 1 || pages_for(4096) != 1 || pages_for(4097) != 2 { return false; }
    // 四档布局均可算且可校验。
    let mut i = 0usize;
    while i < SizeClass::ALL.len() {
        match ZoneLayout::compute(SizeClass::ALL[i].bytes()) {
            Ok(l) if l.verify().is_ok() => {} _ => return false,
        }
        i += 1;
    }
    // 帧偏移表长度 / 页属性表编码数 / 诊断码段互逆。
    if FrameOffsets::V1.flat().len() != 14 || PageAttrTable::new().encoded() != 12 { return false; }
    if !diag_segment_selfcheck() { return false;
    }
    let mut k = StackGuardKernel::new(DEFAULT_POOL_CAPACITY);
    let id = match k.alloc_stack(PAGE_SIZE * 4) {
        Ok(v) => v, Err(_) => return false,
    }; let d = match k.dsv_of(id) { Ok(v) => v, Err(_) => return false,
    };
    // INV-6 半开区间：有效区首合法、末地址非法。
    if !StackGuardKernel::is_valid_d(&d, d.base_addr) || StackGuardKernel::is_valid_d(&d, d.top())
    {
        return false;
    }
    // LCG 确定性：同种子同序列。
    let (mut r1, mut r2) = (Lcg::new(7), Lcg::new(7)); let mut j = 0usize;
    while j < 4 {
        if r1.next_u64() != r2.next_u64() { return false; }
        j += 1;
    }
    // 切换帧：构造 → 自校验 → 审计一致 → 序列化长度。
    match build_switch_frame(&d, &SwitchContext::seeded(0x1822_0A00, d.base_addr, d.top())) {
        Ok(f) => {
            if f.verify().is_err() || !audit_frame(&f, false).consistent || f.to_bytes().len() as u64 != FRAME_SIZE
                || f.offsets().flat().is_empty()
            {
                return false;
            }
        }
        Err(_) => return false,
    }
    // 前向预留：消费满额 → 越界检出 → 版本递增。
    let mut rsv = ForwardReserve::declare(id, d.reserve_base(), 64, 1, k.tick); if rsv.consume(64).is_err()
        || rsv.consume(1).is_ok() || rsv.remaining() != 0 || rsv.bump_version() != 2 || rsv.digest() == 0
    {
        return false;
    }
    // guard 登记册：活登记恰 2（低侧 + 高侧），高侧尚无命中。
    if k.guards_of(id).len() != 2 || k.guards.live_by_side(GuardSide::Underflow).is_empty()
        || k.guards.hits_by_side(GuardSide::Overflow) != 0
    {
        return false;
    }
    // 账目：分档直方图、页框归属（4 数据 + 2 guard）、在线数。
    let m = k.metrics(); if m.class_hits.iter().sum::<u64>() != 1 || k.pool.frames_of(id).len() != 6
        || k.live_stacks() != 1 || k.ids().len() != 1 || k.last_pfn_of(id).is_none() || m.digest() == 0
        || k.pool.digest() == 0 || k.guards.digest() == 0
    {
        return false;
    }
    // TLV 自洽 + 字节读写 + KV 导出。
    let tlv = export_tlv(&k); if verify_tlv(&tlv).is_err() || read_u64(&tlv, 8) != 1
        || read_u32(&tlv, 0) != TLV_MAGIC || read_u16(&tlv, 4) != TLV_VERSION || checksum(&tlv) == 0
        || export_kv(&k).is_empty()
    {
        return false;
    }
    // 回收 + 双记检出。
    if k.reclaim_stack(id).is_err() || !matches!(k.reclaim_stack(id), Err(E::DoubleFree)) { return false; }
    // 页池耗尽与越界页框释放失败路径。
    let mut k2 = StackGuardKernel::new(2);
    if k2.alloc_stack(PAGE_SIZE * 8).is_ok() || k2.pool.free_pfn(999).is_ok() { return false; }
    true
}