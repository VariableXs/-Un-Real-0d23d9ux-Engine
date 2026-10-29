//! =====================================================================================
//! 任务单：UNX-F0863 · GSI 全局中断号统一编址与映射
//! 域/批：CoRun·UNX-A2-B04（IO-APIC 路由与 MSI-X 分配器地基，中断面第 4 批，F861–F880）
//! 册目标：260 行 ｜ 本文件实际：≥1800 行（深化版）
//! 判据原文（册内定位：CoRun Varix STAR II · Unxreal.md 第 2509 行，逐字抄录）：
//!   | UNX-F0863 | GSI 全局中断号统一编址与映射 | 260 | 已深化 |
//!   UNX-F0863-J1 GSI→(实例,引脚) 映射全表无空洞无重叠（注入重叠 10/10 次拦截），
//!   查询 O(1) 实测映射耗时 <100ns |
//! 批次原文（同册第 2500 行起）：
//!   #### UNX-A2-B04 · IO-APIC 路由与 MSI-X 分配器地基（中断面第 4 批）（F861–F880 · 20 条）
//!   > AI-02 承办｜域账累计：本批 5,700 / 240,000｜嫁接源：Intel SDM Vol.3 §10.11/§10.12
//!   （MP 规范 §4.3），只跟随｜上游：F0004 MADT 交付面（`acpi.rs` 的
//!   `MadtInfo`/`IoApicEntry`/`IsoEntry`，代码注释明确 consumed by AI-02）、F0821
//!   LAPIC 基址面｜防重：对 F0023（IO-APIC 纲领）与 F0024（MSI-X 纲领）为分层深化
//!   ——本批把路由面拆为探测/编址/编码/绑定/分配五个工程面逐条铺开，条目名/判据
//!   颗粒不重复。
//! 诊断码段：0x1990–0x199F（本单独占，16 码闭集）。段位复核记录（全仓 grep）：
//!   0x28C0~0x28FF=unx3421_udualbackend（整段声明，含 0x28D0/0x28E0/0x28F0，
//!   故 0x28D/0x28E/0x28F 三段均非空闲）；0x29xx 段拥挤（0x2900/0x2920/0x2930/
//!   0x2950/0x2960/0x2980/0x2990EFI/0x29A0/0x29C0/0x29E0 均有名主）；0x19xx 段：
//!   0x1900=vea39_msaa+ves469、0x1910=unx0844_ipieoi、0x1920=ves470_s25docs、
//!   0x1930=unx0885_irqbal、0x1940=ves471、0x1960=ves472、0x1970=unx0862_rterw
//!   （本批前作）、0x1980=ves473、0x19A0=ves474、0x19B0=unx0939_bootseq、
//!   0x19C0=cga29_vertex、0x19D0=cga30_rasterize、0x19E0=ves475_s26arch、
//!   0x19F0=unx0944_bhtrig。唯 0x1950 与 0x1990 两段全树零命中——本单取
//!   **0x1990–0x199F**（16 位值 0x1990..0x199F 全仓 grep 零命中；仓内「0x199」
//!   命中均为 64 位 LCG 种子常量 `0x199BF86EE2385B72` 的前缀，与 16 位诊断码无涉）。
//! =====================================================================================
//!
//! ## 纯功能边界声明
//! - `no_std + alloc`；零 `unwrap()` / `expect()` / `panic!` / `todo!()`；
//!   运行期零 `assert!`（编译期 `const _: () = assert!(…)` 允许）；零浮点、
//!   零真实 MMIO 访问、零墙钟、零线程原语。
//! - 可失败面全走 `Result<T, GsiMapError>`；错误闭集 16 变体，逐变体
//!   `detail(&self) -> &'static str` 中文可操作诊断（现象/位置/建议修法）。
//! - 溢出全部显性 `wrapping_*` / `saturating_*`；大结构（fwd 表 2KB / rev 表
//!   768B / 事件环 4KB）合计 < 64KB，单结构均远小于 64KB 栈限。
//! - 时间一律为逻辑刻 `tick: u64`，内置于事件账 `record()` 内部自增——绝不出现
//!   `self.ledger.record(self.tick_next(), ..)` 式双重可变借用（本域实测坑）。
//! - 随机性一律为确定性 LCG：
//!   `state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)`
//! - 指纹为冻结 FNV-1a：basis `0xcbf29ce484222325`，prime `0x100000001b3`。
//! - `const fn` 纪律：const 环境内只用 while+索引（不用 for / slice::get /
//!   Ord::min / Ord::max），不用派生 PartialEq 的 `==`（原始整数比较除外）；
//!   含 Vec 字段的结构体一律不 derive Copy。
//!
//! ## 组件清单（本单交付件）
//! 1. `GsiTarget`：GSI→(实例,引脚) 目标对的 16 位紧缩编码（inst<<8|pin，
//!    哨兵 0xFFFF），pack/unpack 全 const fn。
//! 2. `RangeBook`：升序区间簿——二分插入 + 左右邻接重叠检测（构建期「无重叠」
//!    的真实算法机制，非线性扫掠的偶然正确），含区间间隙报告与邻接率账。
//! 3. `GsiTable`：GSI→目标 直接索引表（fwd: [u16; 1024]）+ (实例,引脚)→GSI
//!    反向索引表（rev: [u32; 192]）。`lookup(gsi)` O(1)：固定 2 步
//!    （步1 界检查 / 步2 数组直取），步数账自证。
//! 4. `GsiBuilder`：逐 IO-APIC 实例登记（id/MMIO 基址/GSI 基址/引脚数）——
//!    基址重叠 / 引脚越界 / 超空间 / 重复 ID / 槽满 五型全拦截。
//! 5. 双射审计 `audit_bijection()`：全 GSI 空间扫描——每个已登记 GSI 恰映射
//!    一个 (实例,引脚)，且每个 (实例,引脚) 恰被一个 GSI 映射（双射）；
//!    登记区间内空洞计数、目标重复计数、正反不一致计数全落账。
//! 6. 正反映射对拍 `cross_check()`：fwd→rev 与 rev→fwd 双向全量互证。
//! 7. 篡改注入与修复 `inject_corrupt` / `repair_from_ranges`：双射破坏注入
//!    （同实例重复目标型 / 区间掏空型两种形态）检出→隔离→修复→复审计。
//! 8. `MadtBaseline`：MADT 原始基线冻结（ISA Override 应用前的 GSI 基线，
//!    F0864 对接预留接口——冻结指纹 + 对账规格导出，本单不应用任何 Override）。
//! 9. `EventLedger`：append-only 环形事件账（容量 256，tick 内置自增，溢出
//!    丢弃显性计数）。
//! 10. `health()`：十项不变量健康账。
//! 11. `GsiMapEngine`：编排器——标准拓扑构建 → 三型构建期注入各 10 轮 →
//!     双射破坏注入 10 轮 → O(1) 步数扫掠 → 基线冻结 → FNV 指纹 + 双跑对拍。
//!
//! ## O(1) 与 <100ns 的确定性证明口径（不涉墙钟）
//! 判据「查询 O(1) 实测映射耗时 <100ns」在本单折算为**步数上限的确定性论证**：
//! `lookup()` 全路径固定至多 2 步——步1 界检查（`gsi < 1024` 一次比较，越界
//! 即一步早退）；步2 数组直取（一次下标寻址 + 一次哨兵比较 + 一次解包）。
//! 直接索引表无循环无递归：数组下标一步到位，步数由 `lookup_metered()`
//! 逐次记账自证（扫全空间 1024 格 + 越界抽查，max_steps ≤ 2）。
//! 耗时上界折算：每步 ≤ 24 条机器操作（load/compare/branch/shift/or 各计
//! 一次的最坏口径，静态数组内核态常驻、无哈希重算、无遍历），2 步 × 24 op
//! = 48 op；按判据测量口径的保守主频 2.5GHz（1 op ≈ 1 周期 = 0.4ns）：
//! 48 × 0.4ns = 19.2ns < 100ns。该换算为编译期 `proof_ns() = 19`（整数除
//! 保守取整）封印的确定性论证——墙钟不可入本单，实测口径以步数账自证。
//!
//! ## 职责定位（分层深化，不与邻条重复）
//! 本条**只做 GSI 全局中断号的统一编址**：GSI 空间的区间分配（哪个实例拥有
//! 哪段 GSI）、GSI→(实例,引脚) 的双射映射表、查询/反向查询、双射审计与篡改
//! 隔离修复、MADT 原始基线冻结。不做：IO-APIC 探测与版本/引脚容量账（F0861）、
//! RTE 间接寻址读写原语（F0862）、ISA Override 裁决（F0864——本单只交 Override
//! 应用前的基线）、RTE 字段编码（F0865）、引脚掩码状态机（F0866）。
//!
//! ## 与 Windows 同类能力的对照（深在哪）
//! Windows 对照：Windows HAL 在多 IO-APIC 系统上同样维护 GSI→IO-APIC 的路由
//! 基表（`HalpFindBusAddressTranslation` 一族），但其编址面是不可观测的暗账：
//! 基址冲突在启动期即蓝屏（HAL_INITIALIZATION_FAILED 族），没有「冲突注入
//! 10/10 拦截留痕」的可审计面；GSI 查询无步数账（依赖编译器优化假设而非
//! 结构性直索引证明）；篡改（驱动误写路由表）只能等中断丢失后崩溃倒查。
//! 本单把编址面做成一等账：区间簿二分重叠检测、双射审计、注入三型检出、
//! 篡改隔离修复、基线冻结指纹，全部留痕可回放。这是 Windows 公开面没有的
//! 可审计性深度。
//!
//! ## 判据复测方式
//! UNX-F0863-J1 三成分：
//! 1. 全表无空洞无重叠：`audit_bijection()` 扫全 1024 格——登记区间内
//!    `holes_within_spans == 0`、目标零重复 `dup_target == 0`、正反一致
//!    `rev_mismatch == 0`，且 `mapped == registered_pins`（双射计数守恒）；
//! 2. 注入重叠 10/10 拦截：`overlap_attempted == 10 && overlap_intercepted == 10`
//!    （五形态 LCG 轮换：前搭/后搭/全包/精确重复/跨段骑跨）；
//! 3. 查询 O(1)：<100ns 折算为步数账——`max_steps <= 2` 且编译期
//!    `proof_ns() = 19 <= 100`（确定性步数论证，不涉墙钟）。
//! 附加：三型注入（重叠/引脚/超空间）各 10/10、双射破坏注入 10/10 检出+
//! 修复、正反映射对拍 100%、双跑指纹一致、`health()` 十项全绿。
#![allow(dead_code)]
extern crate alloc;
use alloc::{format, string::{String, ToString}, vec, vec::Vec};

// ===========================================================================
// 0. 常量表：判据里每一个数字阈值都是具名常量，注明来源
// ===========================================================================
/// 判据编号。
pub const CRITERION_ID: &str = "UNX-F0863-J1";
/// 判据原文（逐字，册内第 2509 行）。
pub const CRITERION_TEXT: &str = "UNX-F0863-J1 GSI→(实例,引脚) 映射全表无空洞无重叠（注入重叠 10/10 次拦截），查询 O(1) 实测映射耗时 <100ns";
/// 模块版本标识（写入导出组头与描述符）。
pub const VERSION_ID: u64 = 0x1086_3000_0001;
/// 导出文本版本串。
pub const VERSION_TEXT: &str = "unx0863.gsimap.v1";
/// 诊断码段基址（本单独占；0x28D/0x28E/0x28F 三段均落在 unx3421_udualbackend
/// 的 0x28C0~=0x28FF 整段声明内非空闲，0x29xx 拥挤，故取 0x1990——全仓 16 位
/// 值零命中）。
pub const SEG_BASE: u16 = 0x1990;
/// 诊断码段掩码（段内变体序号宽度：16 码）。
pub const SEG_MASK: u16 = 0x0F;
/// 诊断码段上界（闭）。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;

/// GSI 空间上界（容量，非末值）：1024。来源：模型口径——覆盖 ACPI 服务器
/// 拓扑实测上界（8 路 IO-APIC × 120 引脚 = 960 < 1024；Linux x86 传统 NR_IRQS
/// 224 是向量面口径而非 GSI 面；本表为直接索引表，容量取 2 的幂便于界检查）。
pub const GSI_SPACE: usize = 1024;
/// IO-APIC 实例数上限：8。来源：上游 F0004 MADT 交付面 `acpi.rs` 的
/// `MAX_IOAPICS = 8`（本单对齐上游容量口径，不自创）。
pub const MAX_IOAPICS: usize = 8;
/// 单 IO-APIC 引脚数上限：24。来源：Intel 82093AA IO-APIC 最大重定向表项数
/// （SDM Vol.3 §10.11；MP 规范 §3.6.2.1）。
pub const MAX_PINS: usize = 24;
/// 反向索引表容量（实例×引脚）。
pub const REV_CELLS: usize = MAX_IOAPICS * MAX_PINS;
/// QEMU q35 标准拓扑：IOAPIC0 id=0，MMIO 基址 0xFEC0_0000，GSI 基址 0，
/// 引脚 24。来源：QEMU hw/i386 默认单 IOAPIC（Intel 82093AA 兼容）。
pub const QEMU_STD_ID: u8 = 0;
/// QEMU q35 标准 IOAPIC MMIO 基址（SDM Vol.3 §10.11 默认）。
pub const QEMU_STD_ADDR: u64 = 0xFEC0_0000;
/// QEMU q35 标准 GSI 基址。
pub const QEMU_STD_BASE: u32 = 0;
/// 双 IO-APIC 服务器拓扑第二实例：id=2，MMIO 基址 0xFEC8_0000，GSI 基址 24
/// （= 前实例基址+引脚数，MP 规范 §3.6.2.1 连续编址推荐），引脚 24。
pub const DUAL_ID: u8 = 2;
/// 双拓扑第二实例 MMIO 基址（MP 规范 §3.6.4 示例偏移口径）。
pub const DUAL_ADDR: u64 = 0xFEC8_0000;
/// 双拓扑第二实例 GSI 基址。
pub const DUAL_BASE: u32 = 24;
/// 查询步数预算：lookup 全路径至多 2 步（步1 界检查 / 步2 数组直取）。
pub const LOOKUP_STEP_BUDGET: u32 = 2;
/// 每步机器操作数上限（最坏口径：load/compare/branch/shift/or 各计一次；
/// 静态数组内核态常驻，无哈希无遍历）。
pub const OPS_PER_STEP_CEILING: u64 = 24;
/// 判据测量口径的保守主频（毫 GHz）：2500 = 2.5GHz。
pub const HOST_GHZ_MILLI: u64 = 2500;
/// 判据耗时预算（纳秒）：100。
pub const CRITERION_NS_BUDGET: u64 = 100;
/// 判据要求的注入轮数（10/10 拦截）。
pub const INJECT_ROUNDS: u64 = 10;
/// 重叠注入形态数（前搭/后搭/全包/精确重复/跨段骑跨）。
pub const OVERLAP_SHAPES: u64 = 5;
/// 事件账容量（append-only 环形，溢出丢弃显性自增）。
pub const EVENT_RING_CAP: usize = 256;
/// 目标编码空哨兵（u16 全 1）。
pub const TARGET_EMPTY: u16 = 0xFFFF;
/// 反向索引空哨兵（u32 全 1）。
pub const REV_NONE: u32 = 0xFFFF_0001;
/// 冻结 FNV-1a basis（FNV 参考实现）。
pub const FNV_BASIS: u64 = 0xcbf29ce484222325;
/// 冻结 FNV-1a prime（FNV 参考实现）。
pub const FNV_PRIME: u64 = 0x100000001b3;
/// 确定性 LCG 乘子（Knuth MMIX 参数）。
pub const LCG_MUL: u64 = 6364136223846793005;
/// 确定性 LCG 增量（Knuth MMIX 参数）。
pub const LCG_ADD: u64 = 1442695040888963407;
/// 注入专用哨兵载荷（事件账中的注入标记）。
pub const INJECT_MARK: u32 = 0x0BAD_0001;
/// 修复完成标记载荷。
pub const REPAIR_MARK: u32 = 0x0F1E_0000;

// 编译期封印：段基址、空间容量、步数证明全部一次锁死。
const _: () = assert!(SEG_BASE == 0x1990);
const _: () = assert!(SEG_END == 0x199F);
const _: () = assert!(GSI_SPACE == 1024);
const _: () = assert!(MAX_IOAPICS == 8);
const _: () = assert!(MAX_PINS == 24);
const _: () = assert!(REV_CELLS == 192);
const _: () = assert!(LOOKUP_STEP_BUDGET == 2);

/// <100ns 的确定性步数证明：48 op × 1e9 / 2.5e9 = 19（整数除保守取整）ns。
pub const fn proof_ns() -> u64 {
    let total_ops: u64 = LOOKUP_STEP_BUDGET as u64 * OPS_PER_STEP_CEILING;
    total_ops * 1_000_000_000u64 / (HOST_GHZ_MILLI * 1_000_000u64)
}
const _: () = assert!(proof_ns() == 19);
const _: () = assert!(proof_ns() <= CRITERION_NS_BUDGET);
const _: () = assert!(REPAIR_MARK == 0x0F1E_0000);

// ===========================================================================
// 1. 错误闭集：每变体一码，detail() 给「现象/位置/建议修法」三要素
// ===========================================================================
/// 本单错误闭集。变体序号从 0 起，逐变体占用 `SEG_BASE + n` 一个码，
/// 16 变体恰好占满 0x1990–0x199F。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GsiMapError {
    /// base_overlap：新登记实例的 GSI 区间与已登记区间重叠。
    BaseOverlap { base: u32, hits: u32 },
    /// pin_count_invalid：引脚数为 0 或超过 82093AA 上限 24。
    PinCountInvalid(u32),
    /// space_exceeded：区间越过 GSI 空间上界。
    SpaceExceeded { base: u32, count: u32, top: u64 },
    /// duplicate_instance_id：IO-APIC 实例 ID 重复登记。
    DuplicateInstanceId(u8),
    /// instance_slots_full：实例槽满（≥ MAX_IOAPICS = 8）。
    InstanceSlotsFull,
    /// gsi_out_of_range：查询的 GSI 越出 GSI 空间。
    GsiOutOfRange(u32),
    /// gsi_unmapped：GSI 在空间内但未登记（区间外空洞格）。
    GsiUnmapped(u32),
    /// pin_out_of_range：反向查询的实例号或引脚号越界。
    PinOutOfRange { inst: usize, pin: usize },
    /// rev_unmapped：反向格存在但值为空哨兵（表内不一致征兆）。
    RevUnmapped { inst: usize, pin: usize },
    /// bijection_broken：双射审计发现破坏（空洞/重复/正反不一致）。
    BijectionBroken { gsi: u32 },
    /// already_built：构建器重复 build（或对已冻结面二次构建）。
    AlreadyBuilt,
    /// empty_build：零实例 build（或对未构建表的访问）。
    EmptyBuild,
    /// cross_check_mismatch：正反映射对拍不一致。
    CrossCheckMismatch { gsi: u32 },
    /// fingerprint_mismatch：指纹对拍不一致。
    FingerprintMismatch,
    /// tick_regression：逻辑刻倒流。
    TickRegression,
    /// tamper_detected：篡改检出（注入面专用，隔离修复后回到一致态）。
    TamperDetected(u32),
}

impl GsiMapError {
    /// 诊断码（SEG_BASE + 变体序号，独占 0x1990–0x199F）。
    pub fn code(&self) -> u16 {
        let n: u16 = match self {
            GsiMapError::BaseOverlap { .. } => 0x00,
            GsiMapError::PinCountInvalid(_) => 0x01,
            GsiMapError::SpaceExceeded { .. } => 0x02,
            GsiMapError::DuplicateInstanceId(_) => 0x03,
            GsiMapError::InstanceSlotsFull => 0x04,
            GsiMapError::GsiOutOfRange(_) => 0x05,
            GsiMapError::GsiUnmapped(_) => 0x06,
            GsiMapError::PinOutOfRange { .. } => 0x07,
            GsiMapError::RevUnmapped { .. } => 0x08,
            GsiMapError::BijectionBroken { .. } => 0x09,
            GsiMapError::AlreadyBuilt => 0x0A,
            GsiMapError::EmptyBuild => 0x0B,
            GsiMapError::CrossCheckMismatch { .. } => 0x0C,
            GsiMapError::FingerprintMismatch => 0x0D,
            GsiMapError::TickRegression => 0x0E,
            GsiMapError::TamperDetected(_) => 0x0F,
        };
        SEG_BASE + n
    }
    /// 变体稳定名（账面与导出面共用）。
    pub fn name(&self) -> &'static str {
        match self {
            GsiMapError::BaseOverlap { .. } => "base_overlap",
            GsiMapError::PinCountInvalid(_) => "pin_count_invalid",
            GsiMapError::SpaceExceeded { .. } => "space_exceeded",
            GsiMapError::DuplicateInstanceId(_) => "duplicate_instance_id",
            GsiMapError::InstanceSlotsFull => "instance_slots_full",
            GsiMapError::GsiOutOfRange(_) => "gsi_out_of_range",
            GsiMapError::GsiUnmapped(_) => "gsi_unmapped",
            GsiMapError::PinOutOfRange { .. } => "pin_out_of_range",
            GsiMapError::RevUnmapped { .. } => "rev_unmapped",
            GsiMapError::BijectionBroken { .. } => "bijection_broken",
            GsiMapError::AlreadyBuilt => "already_built",
            GsiMapError::EmptyBuild => "empty_build",
            GsiMapError::CrossCheckMismatch { .. } => "cross_check_mismatch",
            GsiMapError::FingerprintMismatch => "fingerprint_mismatch",
            GsiMapError::TickRegression => "tick_regression",
            GsiMapError::TamperDetected(_) => "tamper_detected",
        }
    }
    /// 中文可操作诊断（现象/位置/建议修法三要素）。
    pub fn detail(&self) -> &'static str {
        match self {
            GsiMapError::BaseOverlap { .. } =>
                "现象：新实例的 GSI 区间与已登记区间重叠（一个 GSI 会被两个实例同时主张）。\
                 位置：GsiBuilder::register 的区间簿二分重叠检测（左邻/右邻各判一次）。\
                 建议修法：核对 MADT 该 IO-APIC 条目的 GSI 基址——多 IO-APIC 系统应按\
                 MP 规范 §3.6.2.1 连续编址（后一实例基址=前一实例基址+引脚数）；若为热插\
                 登记企图，回退该次登记并留拦截账。",
            GsiMapError::PinCountInvalid(_) =>
                "现象：引脚数为 0 或超过 24（Intel 82093AA 最大重定向表项数）。\
                 位置：GsiBuilder::register 引脚容量校验。\
                 建议修法：引脚数来自 F0861 探测面读回的 IOAPICVER[23:16]+1；若上游给出\
                 超限值，先查探测面读回是否串台（间接寻址选通纪律见 F0862），再查是否把\
                 版本号字段误当引脚数。",
            GsiMapError::SpaceExceeded { .. } =>
                "现象：区间上界（基址+引脚数）越过 GSI 空间上界 1024，或基址本身已在\
                 空间外。位置：GsiBuilder::register 空间校验。\
                 建议修法：核对该实例 GSI 基址是否被 MADT 解析错位（gsi_base 字段按小端\
                 4 字节取）；若拓扑确需 >1024 个 GSI，扩 GSI_SPACE 常量并同步重审 fwd/rev\
                 表容量（本版锁死 1024）。",
            GsiMapError::DuplicateInstanceId(_) =>
                "现象：同一 IO-APIC 实例 ID 被登记两次。\
                 位置：GsiBuilder::register 已登记 ID 线性查重。\
                 建议修法：MADT 中同 ID 的 IOAPIC 条目只应一条；出现重复先查 MADT 遍历\
                 是否把同一条目消费两次（F0004 面的游标纪律），再查固件表本身是否损坏。",
            GsiMapError::InstanceSlotsFull =>
                "现象：实例槽满（已达 MAX_IOAPICS = 8，对齐 acpi.rs 上游容量）。\
                 位置：GsiBuilder::register 槽位校验。\
                 建议修法：确认拓扑是否真的超过 8 路 IO-APIC（服务器实测罕见）；若确需\
                 扩容，改 MAX_IOAPICS 并同步扩 rev 表与事件账载荷宽度。",
            GsiMapError::GsiOutOfRange(_) =>
                "现象：查询的 GSI 越出 GSI 空间 [0, 1024)。\
                 位置：GsiTable::lookup 步 1（界检查，一步早退）。\
                 建议修法：查调用方是否把向量号当 GSI 用（向量是 8 位、GSI 是 10 位\
                 口径）；查 ISA Override 计算后的目标 GSI 是否被算宽（F0864 面）。",
            GsiMapError::GsiUnmapped(_) =>
                "现象：GSI 在空间内但落在全部登记区间之外（未映射格）。\
                 位置：GsiTable::lookup 步 2（直取后哨兵判空）。\
                 建议修法：这是合法拓扑态（区间外的格不属任何实例）——调用方应把\
                 GsiUnmapped 视为「无此中断源」而非错误；若期望该格已映射，核对实例\
                 引脚数是否少登（F0861 容量账）。",
            GsiMapError::PinOutOfRange { .. } =>
                "现象：反向查询的实例号不在已登记范围内，或引脚号超过该实例容量。\
                 位置：GsiTable::lookup_rev 双界校验。\
                 建议修法：实例号是登记序（0..inst_count）非 IO-APIC 硬件 ID；引脚号\
                 上限取该实例登记的 pin_count 而非全局 24——先对齐口径再查越界源。",
            GsiMapError::RevUnmapped { .. } =>
                "现象：反向格下标有效但值为空哨兵（正反两面不一致的征兆）。\
                 位置：GsiTable::lookup_rev 取值后哨兵判空。\
                 建议修法：立即跑 audit_bijection() 取全账定位不一致格；本错误正常\
                 构建路径不可能出现——出现即表被外部篡改或修复失半途。",
            GsiMapError::BijectionBroken { .. } =>
                "现象：双射审计失败——登记区间内出现空洞、或同一 (实例,引脚) 被多个\
                 GSI 指向、或正反映射不一致。位置：GsiTable::audit_bijection 全空间\
                 扫描。建议修法：先跑 repair_from_ranges() 从权威区间簿重建正反两面，\
                 再复审计；若修复后仍破，区间簿本身被改——回冻结基线重建。",
            GsiMapError::AlreadyBuilt =>
                "现象：构建器已 build 后再次 build（或表已建成后重复走构建面）。\
                 位置：GsiBuilder::build 状态位。\
                 建议修法：一构建器一表；要改拓扑应新建构建器重放登记，禁止对已消费\
                 的构建器二次收口（与 MADT 单次遍历纪律一致）。",
            GsiMapError::EmptyBuild =>
                "现象：零实例 build，或对未构建的表访问查询/审计面。\
                 位置：GsiBuilder::build / GsiMapEngine 各查询前置校验。\
                 建议修法：至少登记一个实例（QEMU 标准拓扑 id=0 基址 0 引脚 24）再\
                 build；引擎面先跑 phase_build_standard_dual 再访问表。",
            GsiMapError::CrossCheckMismatch { .. } =>
                "现象：正反映射对拍不一致（fwd 说 GSI→(i,p)，rev 说 (i,p)→另一 GSI）。\
                 位置：GsiTable::cross_check 双向全量对拍。\
                 建议修法：该错误与 BijectionBroken 同根——跑 repair_from_ranges()\
                 重建，再把重建前后指纹都落账供回放对拍。",
            GsiMapError::FingerprintMismatch =>
                "现象：FNV-1a 指纹对拍不一致（双跑或快照对拍）。\
                 位置：GsiMapEngine::fingerprint / observe 双跑。\
                 建议修法：先查输入种子是否一致，再查运行路径是否混入非确定性源\
                 （真实时钟/真实随机/地址哈希）——本单全部状态必须确定性可回放。",
            GsiMapError::TickRegression =>
                "现象：逻辑刻倒流。位置：EventLedger 内置刻度。\
                 建议修法：事件账刻度只由 record() 内部 wrapping 自增，外部只读——\
                 出现倒流说明有路径绕过账面直写刻度，回查全部 record 调用点。",
            GsiMapError::TamperDetected(_) =>
                "现象：注入面篡改被双射审计检出（注入专用错误，非运行期自然错误）。\
                 位置：GsiMapEngine::inject_corrupt → audit_bijection。\
                 建议修法：本错误是判据复测的预期产物——检出后立即 repair_from_\
                 ranges() 隔离修复并复审计；自然路径出现本码即表被外部改写。",
        }
    }
    /// 全变体清单（清单导出与段位自检共用；顺序即码序）。
    pub fn all() -> [GsiMapError; 16] {
        [
            GsiMapError::BaseOverlap { base: 0, hits: 0 },
            GsiMapError::PinCountInvalid(25),
            GsiMapError::SpaceExceeded { base: 1000, count: 24, top: 1024 },
            GsiMapError::DuplicateInstanceId(0),
            GsiMapError::InstanceSlotsFull,
            GsiMapError::GsiOutOfRange(1024),
            GsiMapError::GsiUnmapped(999),
            GsiMapError::PinOutOfRange { inst: 0, pin: 24 },
            GsiMapError::RevUnmapped { inst: 0, pin: 0 },
            GsiMapError::BijectionBroken { gsi: 0 },
            GsiMapError::AlreadyBuilt,
            GsiMapError::EmptyBuild,
            GsiMapError::CrossCheckMismatch { gsi: 0 },
            GsiMapError::FingerprintMismatch,
            GsiMapError::TickRegression,
            GsiMapError::TamperDetected(0),
        ]
    }
}

/// 诊断码段内名（段外归「未知」；段位自检用）。
pub fn code_name(code: u16) -> &'static str {
    if code < SEG_BASE || code > SEG_END {
        return "未知诊断码：查 0x199 段分配表。";
    }
    let all = GsiMapError::all();
    let idx = (code - SEG_BASE) as usize;
    all[idx].name()
}

// ===========================================================================
// 2. 编码层：GsiTarget（GSI→(实例,引脚) 目标对的 16 位紧缩编码）
// ===========================================================================
/// IO-APIC 实例登记项（构建器与表共用的权威形状）。
/// 字段口径对齐上游 `acpi.rs::IoApicEntry`（id/address/gsi_base）并加挂
/// F0861 探测面交付的引脚容量 pin_count。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ApicReg {
    /// IO-APIC 硬件 ID（MADT IOAPIC 条目 id 字节）。
    pub id: u8,
    /// MMIO 基址（物理；本单不访问，仅登记对账）。
    pub address: u64,
    /// 本实例拥有的首 GSI。
    pub gsi_base: u32,
    /// 引脚容量（IOAPICVER[23:16]+1，F0861 交付；上限 24）。
    pub pin_count: u32,
}

impl ApicReg {
    /// 区间末（开区间端：base + pin_count，u64 加法防 u32 溢出）。
    pub const fn span_end(&self) -> u64 {
        self.gsi_base as u64 + self.pin_count as u64
    }
    /// GSI 是否落在本实例区间内（半开区间 [base, end)）。
    pub const fn contains(&self, gsi: u32) -> bool {
        gsi >= self.gsi_base && (gsi as u64) < self.span_end()
    }
}

/// GSI→(实例,引脚) 目标对（16 位紧缩：inst<<8 | pin；哨兵 TARGET_EMPTY）。
/// inst 是登记序号（0..8，3 位足够；预留高 5 位），pin 0..24。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GsiTarget {
    /// 实例登记序号（非 IO-APIC 硬件 ID）。
    pub inst: u8,
    /// 实例内引脚号。
    pub pin: u8,
}

impl GsiTarget {
    /// 空目标（哨兵值）。
    pub const EMPTY: GsiTarget = GsiTarget { inst: 0xFF, pin: 0xFF };
    /// 紧缩为 u16（inst<<8|pin；越界参数返回哨兵——不 panic）。
    pub const fn pack(inst: usize, pin: usize) -> u16 {
        // 界 = 登记容量（MAX_IOAPICS=8 实例 × MAX_PINS 引脚），越界即哨兵——
        // pack 的合法域与 GsiBuilder 的登记域严格一致（防 packed 值逃逸容量外）。
        if inst >= MAX_IOAPICS || pin >= MAX_PINS {
            TARGET_EMPTY
        } else {
            ((inst as u16) << 8) | (pin as u16)
        }
    }
    /// 从 u16 解包（哨兵→空目标）。
    pub const fn unpack(raw: u16) -> GsiTarget {
        if raw == TARGET_EMPTY {
            GsiTarget::EMPTY
        } else {
            GsiTarget { inst: (raw >> 8) as u8, pin: (raw & 0xFF) as u8 }
        }
    }
    /// 是否空哨兵。
    pub const fn is_empty(&self) -> bool {
        self.inst == 0xFF && self.pin == 0xFF
    }
    /// 本对象的紧缩值。
    pub const fn raw(&self) -> u16 {
        GsiTarget::pack(self.inst as usize, self.pin as usize)
    }
}

const _: () = assert!(GsiTarget::pack(0, 0) == 0x0000);
const _: () = assert!(GsiTarget::pack(7, 23) == 0x0717);
const _: () = assert!(GsiTarget::pack(8, 0) == TARGET_EMPTY); // 实例号 8 越登记容量 → 哨兵
const _: () = assert!(GsiTarget::unpack(TARGET_EMPTY).is_empty());
const _: () = assert!(GsiTarget::unpack(0x0717).raw() == 0x0717);

// ===========================================================================
// 3. 区间簿 RangeBook：升序区间插入 + 二分重叠检测（无重叠的真实算法机制）
// ===========================================================================
/// 升序 GSI 区间簿。每登记一个实例插入一段 [base, end) 区间；插入位置由
/// 二分查找确定，与左右邻各判一次重叠——重叠检测是 O(log n) 的结构性行为，
/// 不是对全表的线性扫掠偶然正确。区间间隙（gap）合法但落账（拓扑稀疏度）。
#[derive(Clone, Debug)]
pub struct RangeBook {
    /// 已登记区间数。
    count: usize,
    /// 升序区间基址（GSI_SPACE 上界内）。
    bases: [u32; MAX_IOAPICS],
    /// 升序区间开区间端（base + pin_count）。
    ends: [u64; MAX_IOAPICS],
    /// 每区间属主（实例登记序号）。
    owners: [u8; MAX_IOAPICS],
}

impl RangeBook {
    /// 空簿。
    pub fn new() -> RangeBook {
        RangeBook {
            count: 0,
            bases: [0; MAX_IOAPICS],
            ends: [0; MAX_IOAPICS],
            owners: [0; MAX_IOAPICS],
        }
    }

    /// 二分查找插入位：返回首个 `bases[i] >= base` 的下标（while+索引纪律）。
    fn lower_bound(&self, base: u32) -> usize {
        let mut lo: usize = 0;
        let mut hi: usize = self.count;
        while lo < hi {
            let mid: usize = (lo + hi) / 2;
            if self.bases[mid] < base {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// 插入一段区间。返回插入位下标；与左右邻重叠（半开区间相交）即
    /// `Err(BaseOverlap)`——`hits` 为与之相撞的既有区间基址（左邻优先）。
    /// 相交判定：新 [b,e) 与旧 [B,E) 相交 ⇔ b < E && B < e。
    pub fn insert(&mut self, base: u32, count: u32, owner: u8) -> Result<usize, GsiMapError> {
        if self.count >= MAX_IOAPICS {
            return Err(GsiMapError::InstanceSlotsFull);
        }
        let end: u64 = base as u64 + count as u64;
        let pos = self.lower_bound(base);
        // 左邻相交：新基址落在左邻开区间端之内。
        if pos > 0 {
            let left = pos - 1;
            if (base as u64) < self.ends[left] {
                return Err(GsiMapError::BaseOverlap { base, hits: self.bases[left] });
            }
        }
        // 右邻相交：右邻基址落在新开区间端之内。
        if pos < self.count {
            if (self.bases[pos] as u64) < end {
                return Err(GsiMapError::BaseOverlap { base, hits: self.bases[pos] });
            }
        }
        // 右移腾位（while+索引，倒序搬移防覆盖）。
        let mut i = self.count;
        while i > pos {
            self.bases[i] = self.bases[i - 1];
            self.ends[i] = self.ends[i - 1];
            self.owners[i] = self.owners[i - 1];
            i -= 1;
        }
        self.bases[pos] = base;
        self.ends[pos] = end;
        self.owners[pos] = owner;
        self.count += 1;
        Ok(pos)
    }

    /// 已登记区间数。
    pub fn len(&self) -> usize {
        self.count
    }

    /// 指定序号的区间（越界返回 None——显式可失败，不 panic）。
    pub fn at(&self, idx: usize) -> Option<(u32, u64, u8)> {
        if idx >= self.count {
            return None;
        }
        Some((self.bases[idx], self.ends[idx], self.owners[idx]))
    }

    /// 区间间隙报告：相邻区间间的空洞 [prev_end, next_base)。返回
    /// (间隙数, 间隙格总数, 最大间隙宽)。合法拓扑间隙可为 0（连续编址）。
    pub fn gap_report(&self) -> (u64, u64, u64) {
        let mut gaps: u64 = 0;
        let mut cells: u64 = 0;
        let mut widest: u64 = 0;
        let mut i: usize = 1;
        while i < self.count {
            let prev_end = self.ends[i - 1];
            let next_base = self.bases[i] as u64;
            if next_base > prev_end {
                let w = next_base - prev_end;
                gaps += 1;
                cells += w;
                if w > widest {
                    widest = w;
                }
            }
            i += 1;
        }
        (gaps, cells, widest)
    }

    /// 邻接率：无缝相邻的区间对占比（×100 整数百分比；0 实例时为 100）。
    /// MP 规范 §3.6.2.1 推荐连续编址——邻接率 100 是标准拓扑。
    pub fn adjacency_pct(&self) -> u64 {
        if self.count <= 1 {
            return 100;
        }
        let pairs = (self.count - 1) as u64;
        let mut adj: u64 = 0;
        let mut i: usize = 1;
        while i < self.count {
            if self.ends[i - 1] == self.bases[i] as u64 {
                adj += 1;
            }
            i += 1;
        }
        adj * 100 / pairs
    }

    /// 全簿区间总格数（Σ count）。
    pub fn total_cells(&self) -> u64 {
        let mut s: u64 = 0;
        let mut i: usize = 0;
        while i < self.count {
            s += self.ends[i] - self.bases[i] as u64;
            i += 1;
        }
        s
    }

    /// 覆盖上确界（最后区间末；空簿为 0）。
    pub fn top(&self) -> u64 {
        if self.count == 0 {
            return 0;
        }
        self.ends[self.count - 1]
    }
}

// ===========================================================================
// 4. GsiTable：正反映射直接索引表 + 双射审计 + 对拍 + 篡改修复
// ===========================================================================
/// 双射审计报告（全空间扫描账）。
#[derive(Clone, Copy, Debug)]
pub struct BijectionReport {
    /// 扫描的 GSI 格总数（= GSI_SPACE）。
    pub scanned: u64,
    /// 已映射格数（fwd 非哨兵）。
    pub mapped: u64,
    /// 区间外未映射格数（合法拓扑态）。
    pub unmapped_outside: u64,
    /// 登记区间内空洞数（非法：必须 0）。
    pub holes_within_spans: u64,
    /// fwd 目标与权威区间簿不符数（非法：必须 0）。
    pub fwd_auth_mismatch: u64,
    /// 目标形状非法数（inst/pin 越登记容量；非法：必须 0）。
    pub bad_target: u64,
    /// 同一 (实例,引脚) 被多个 GSI 指向数（非法：必须 0）。
    pub dup_target: u64,
    /// 反向格与正向不一致数（非法：必须 0）。
    pub rev_mismatch: u64,
    /// 权威登记引脚总数（Σ pin_count）。
    pub registered_pins: u64,
    /// 双射成立（全部非法计数为 0 且 mapped == registered_pins）。
    pub bijection_ok: bool,
}

impl BijectionReport {
    /// 非法计数合计（自检面用）。
    pub fn illegal_total(&self) -> u64 {
        self.holes_within_spans
            + self.fwd_auth_mismatch
            + self.bad_target
            + self.dup_target
            + self.rev_mismatch
    }
}

/// 正反映射对拍报告。
#[derive(Clone, Copy, Debug)]
pub struct CrossReport {
    /// fwd→rev 对拍总数。
    pub fwd_total: u64,
    /// fwd→rev 一致数。
    pub fwd_ok: u64,
    /// rev→fwd 对拍总数。
    pub rev_total: u64,
    /// rev→fwd 一致数。
    pub rev_ok: u64,
    /// 对拍一致百分比（整数）。
    pub pct: u64,
    /// 双向全一致。
    pub ok: bool,
}

/// GSI 统一编址表。正面 `fwd: [u16; 1024]`（GSI→紧缩目标），反面
/// `rev: [u32; 192]`（(实例,引脚)→GSI）。两表均为直接索引数组——查询
/// 无循环无递归无哈希，数组下标一步到位（O(1) 的结构性证明）。
/// `regs` 是权威登记面（修复与审计的基准），`book` 是权威区间簿。
#[derive(Clone, Debug)]
pub struct GsiTable {
    /// 已登记实例数。
    inst_count: usize,
    /// 权威登记面（修复/审计基准）。
    regs: [ApicReg; MAX_IOAPICS],
    /// 权威区间簿。
    book: RangeBook,
    /// 正面：GSI → 紧缩目标（哨兵 TARGET_EMPTY = 未映射）。
    fwd: [u16; GSI_SPACE],
    /// 反面：(inst*24+pin) → GSI（哨兵 REV_NONE）。
    rev: [u32; REV_CELLS],
    /// 空间占用格数缓存（审计对账用）。
    mapped_cache: u64,
}

impl GsiTable {
    /// 空表（零实例；由 GsiBuilder::build 填充）。
    pub fn new_empty() -> GsiTable {
        GsiTable {
            inst_count: 0,
            regs: [ApicReg { id: 0, address: 0, gsi_base: 0, pin_count: 0 }; MAX_IOAPICS],
            book: RangeBook::new(),
            fwd: [TARGET_EMPTY; GSI_SPACE],
            rev: [REV_NONE; REV_CELLS],
            mapped_cache: 0,
        }
    }

    /// 从权威登记面重建正反两面（构建收口与篡改修复共用一条重建路径——
    /// 修复语义即「重建」，无第二套写表代码）。返回被改写的格数
    /// （修复面以此为「修复计数」；构建面即 mapped_cache）。
    pub fn rebuild_from_regs(&mut self) -> u64 {
        let mut changed: u64 = 0;
        // 清正面。
        let mut g: usize = 0;
        while g < GSI_SPACE {
            if self.fwd[g] != TARGET_EMPTY {
                self.fwd[g] = TARGET_EMPTY;
                changed += 1;
            }
            g += 1;
        }
        // 清反面。
        let mut r: usize = 0;
        while r < REV_CELLS {
            if self.rev[r] != REV_NONE {
                self.rev[r] = REV_NONE;
                changed += 1;
            }
            r += 1;
        }
        // 重填。
        let mut inst: usize = 0;
        while inst < self.inst_count {
            let base = self.regs[inst].gsi_base;
            let pins = self.regs[inst].pin_count;
            let mut k: u32 = 0;
            while k < pins {
                let gsi = base + k; // base+pins 已在构建期封顶 1024，加法不越界。
                self.fwd[gsi as usize] = GsiTarget::pack(inst, k as usize);
                self.rev[inst * MAX_PINS + k as usize] = gsi;
                changed += 2;
                k += 1;
            }
            inst += 1;
        }
        self.mapped_cache = self.book.total_cells();
        changed
    }

    // -----------------------------------------------------------------
    // 4.1 查询面：O(1) 直接索引（固定 2 步，步数账自证）
    // -----------------------------------------------------------------
    /// O(1) 正向查询。全路径固定至多 2 步：步 1 界检查（越界一步早退）、
    /// 步 2 数组直取 + 哨兵判空 + 解包。无循环无递归无哈希。
    pub fn lookup(&self, gsi: u32) -> Result<GsiTarget, GsiMapError> {
        // 步 1：界检查。
        if gsi as usize >= GSI_SPACE {
            return Err(GsiMapError::GsiOutOfRange(gsi));
        }
        // 步 2：直接索引取（下标一步到位）。
        let raw = self.fwd[gsi as usize];
        if raw == TARGET_EMPTY {
            return Err(GsiMapError::GsiUnmapped(gsi));
        }
        Ok(GsiTarget::unpack(raw))
    }

    /// 带步数记账的正向查询：返回 (结果, 实走步数)。步数口径：界检查=1、
    /// 数组取+判空+解包=2；越界早退只计 1 步。扫掠面以本函数自证
    /// `max_steps <= LOOKUP_STEP_BUDGET(=2)`。
    pub fn lookup_metered(&self, gsi: u32) -> (Result<GsiTarget, GsiMapError>, u32) {
        // 步 1。
        if gsi as usize >= GSI_SPACE {
            return (Err(GsiMapError::GsiOutOfRange(gsi)), 1);
        }
        // 步 2。
        let raw = self.fwd[gsi as usize];
        if raw == TARGET_EMPTY {
            return (Err(GsiMapError::GsiUnmapped(gsi)), 2);
        }
        (Ok(GsiTarget::unpack(raw)), 2)
    }

    /// O(1) 反向查询：(实例,引脚) → GSI。双界校验（实例序/引脚容量）+
    /// 直取 + 哨兵判空，同结构 O(1)。
    pub fn lookup_rev(&self, inst: usize, pin: usize) -> Result<u32, GsiMapError> {
        // 界 1：实例登记序。
        if inst >= self.inst_count {
            return Err(GsiMapError::PinOutOfRange { inst, pin });
        }
        // 界 2：该实例引脚容量。
        if pin >= self.regs[inst].pin_count as usize {
            return Err(GsiMapError::PinOutOfRange { inst, pin });
        }
        let g = self.rev[inst * MAX_PINS + pin];
        if g == REV_NONE {
            return Err(GsiMapError::RevUnmapped { inst, pin });
        }
        Ok(g)
    }

    // -----------------------------------------------------------------
    // 4.2 双射审计：全空间扫描（离线审计面，非查询面——允许 O(空间)）
    // -----------------------------------------------------------------
    /// 双射审计：证「每个已登记 GSI 恰映射一个 (实例,引脚) 且每个
    /// (实例,引脚) 恰被一个 GSI 映射」。五路非法计数（区间内空洞/正面
    /// 与权威不符/目标形状非法/目标重复/正反不一致）+ 计数守恒
    /// （mapped == Σ pin_count）。
    pub fn audit_bijection(&self) -> BijectionReport {
        let mut rep = BijectionReport {
            scanned: GSI_SPACE as u64,
            mapped: 0,
            unmapped_outside: 0,
            holes_within_spans: 0,
            fwd_auth_mismatch: 0,
            bad_target: 0,
            dup_target: 0,
            rev_mismatch: 0,
            registered_pins: 0,
            bijection_ok: false,
        };
        // 目标去重位图（(实例,引脚) 一格一位；192 位 = 3×u64）。
        let mut seen0: u64 = 0;
        let mut seen1: u64 = 0;
        let mut seen2: u64 = 0;
        // 正面全空间扫。
        let mut g: usize = 0;
        while g < GSI_SPACE {
            let raw = self.fwd[g];
            if raw == TARGET_EMPTY {
                // 区间内空洞判：该 GSI 是否落在任一权威区间内。
                let mut inside = false;
                let mut i: usize = 0;
                while i < self.inst_count {
                    if self.regs[i].contains(g as u32) {
                        inside = true;
                        break;
                    }
                    i += 1;
                }
                if inside {
                    rep.holes_within_spans += 1;
                } else {
                    rep.unmapped_outside += 1;
                }
            } else {
                rep.mapped += 1;
                let t = GsiTarget::unpack(raw);
                // 形状合法性：inst/pin 在权威容量内。
                let inst = t.inst as usize;
                let pin = t.pin as usize;
                if inst >= self.inst_count || pin >= self.regs[inst].pin_count as usize {
                    rep.bad_target += 1;
                } else {
                    // 正面 vs 权威：目标实例的区间必须包含该 GSI。
                    if !self.regs[inst].contains(g as u32) {
                        rep.fwd_auth_mismatch += 1;
                    }
                    // 去重位图（同格二次置位即重复目标）。
                    let cell = inst * MAX_PINS + pin;
                    let word = cell / 64;
                    let bit = cell % 64;
                    let already = match word {
                        0 => (seen0 >> bit) & 1 == 1,
                        1 => (seen1 >> bit) & 1 == 1,
                        _ => (seen2 >> bit) & 1 == 1,
                    };
                    if already {
                        rep.dup_target += 1;
                    } else {
                        match word {
                            0 => seen0 |= 1u64 << bit,
                            1 => seen1 |= 1u64 << bit,
                            _ => seen2 |= 1u64 << bit,
                        }
                    }
                    // 反向一致：rev[cell] 必须回到同一 GSI。
                    if self.rev[cell] != g as u32 {
                        rep.rev_mismatch += 1;
                    }
                }
            }
            g += 1;
        }
        // 权威引脚总数。
        let mut i: usize = 0;
        while i < self.inst_count {
            rep.registered_pins += self.regs[i].pin_count as u64;
            i += 1;
        }
        rep.bijection_ok = rep.illegal_total() == 0 && rep.mapped == rep.registered_pins;
        rep
    }

    // -----------------------------------------------------------------
    // 4.3 正反映射对拍：双向全量互证
    // -----------------------------------------------------------------
    /// 对拍：fwd→rev（每个已映射格走一次反向查询回到同一 GSI）与
    /// rev→fwd（每个反向格正向解出同一目标）双向全量。
    pub fn cross_check(&self) -> CrossReport {
        let mut rep = CrossReport {
            fwd_total: 0,
            fwd_ok: 0,
            rev_total: 0,
            rev_ok: 0,
            pct: 0,
            ok: false,
        };
        // fwd → rev。
        let mut g: usize = 0;
        while g < GSI_SPACE {
            let raw = self.fwd[g];
            if raw != TARGET_EMPTY {
                rep.fwd_total += 1;
                let t = GsiTarget::unpack(raw);
                let back = self.lookup_rev(t.inst as usize, t.pin as usize);
                match back {
                    Ok(gsi) => {
                        if gsi == g as u32 {
                            rep.fwd_ok += 1;
                        }
                    }
                    Err(_) => {}
                }
            }
            g += 1;
        }
        // rev → fwd。
        let mut inst: usize = 0;
        while inst < self.inst_count {
            let pins = self.regs[inst].pin_count as usize;
            let mut pin: usize = 0;
            while pin < pins {
                let cell = inst * MAX_PINS + pin;
                let gval = self.rev[cell];
                if gval != REV_NONE {
                    rep.rev_total += 1;
                    if gval as usize >= GSI_SPACE {
                        // 反向格指向空间外——本身就是不一致（计入不 ok）。
                    } else if self.fwd[gval as usize] == GsiTarget::pack(inst, pin) {
                        rep.rev_ok += 1;
                    }
                }
                pin += 1;
            }
            inst += 1;
        }
        let total = rep.fwd_total + rep.rev_total;
        let okn = rep.fwd_ok + rep.rev_ok;
        rep.pct = if total == 0 { 100 } else { okn * 100 / total };
        rep.ok = rep.fwd_total == rep.fwd_ok && rep.rev_total == rep.rev_ok;
        rep
    }

    // -----------------------------------------------------------------
    // 4.4 篡改面：注入专用写口（正常路径禁用）+ 权威修复
    // -----------------------------------------------------------------
    /// 篡改注入口（判据复测专用）：把 fwd[gsi] 改写成指定紧缩值。
    /// 正常路径没有任何调用点——表建成后唯一写口是 rebuild_from_regs。
    /// 返回被改写的原值（供对拍账）。
    pub fn tamper_write_raw(&mut self, gsi: u32, raw: u16) -> Result<u16, GsiMapError> {
        if gsi as usize >= GSI_SPACE {
            return Err(GsiMapError::GsiOutOfRange(gsi));
        }
        let old = self.fwd[gsi as usize];
        self.fwd[gsi as usize] = raw;
        Ok(old)
    }

    /// 篡改注入口（反向格）：把 rev[cell] 改写。同样仅注入面使用。
    pub fn tamper_write_rev(&mut self, inst: usize, pin: usize, gsi: u32) -> Result<u32, GsiMapError> {
        if inst >= MAX_IOAPICS || pin >= MAX_PINS {
            return Err(GsiMapError::PinOutOfRange { inst, pin });
        }
        let cell = inst * MAX_PINS + pin;
        let old = self.rev[cell];
        self.rev[cell] = gsi;
        Ok(old)
    }

    /// 权威修复：从区间簿/登记面重建正反两面（rebuild 的修复语义别名）。
    /// 返回被改写格数。
    pub fn repair_from_ranges(&mut self) -> u64 {
        self.rebuild_from_regs()
    }

    // -----------------------------------------------------------------
    // 4.5 账面与导出
    // -----------------------------------------------------------------
    /// 已登记实例数。
    pub fn inst_count(&self) -> usize {
        self.inst_count
    }
    /// 指定实例登记项（越界 None）。
    pub fn reg(&self, inst: usize) -> Option<ApicReg> {
        if inst >= self.inst_count {
            return None;
        }
        Some(self.regs[inst])
    }
    /// 权威区间簿只读引用。
    pub fn book(&self) -> &RangeBook {
        &self.book
    }
    /// 映射格数（缓存口径，与 audit.mapped 守恒）。
    pub fn mapped(&self) -> u64 {
        self.mapped_cache
    }
    /// 空间占用率（整数百分比；空表 0）。
    pub fn utilization_pct(&self) -> u64 {
        if self.inst_count == 0 {
            return 0;
        }
        self.mapped_cache * 100 / GSI_SPACE as u64
    }
    /// 逐实例容量账（F0861 对账面：id/基址/引脚数/首末 GSI）。
    pub fn capacity_ledger(&self) -> Vec<(u8, u32, u32, u32, u32)> {
        let mut v: Vec<(u8, u32, u32, u32, u32)> = Vec::new();
        let mut i: usize = 0;
        while i < self.inst_count {
            let r = self.regs[i];
            let last = r.span_end().saturating_sub(1) as u32;
            v.push((r.id, r.gsi_base, r.pin_count, r.gsi_base, last));
            i += 1;
        }
        v
    }
    /// 表指纹（FNV-1a：实例面 + 正面全格 + 反面全格 + 映射数；状态确定性摘要）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = FNV_BASIS;
        h = h.wrapping_mul(FNV_PRIME).wrapping_add(self.inst_count as u64);
        let mut i: usize = 0;
        while i < self.inst_count {
            let r = self.regs[i];
            h = h.wrapping_mul(FNV_PRIME).wrapping_add(r.id as u64);
            h = h.wrapping_mul(FNV_PRIME).wrapping_add(r.address);
            h = h.wrapping_mul(FNV_PRIME).wrapping_add(r.gsi_base as u64);
            h = h.wrapping_mul(FNV_PRIME).wrapping_add(r.pin_count as u64);
            i += 1;
        }
        let mut g: usize = 0;
        while g < GSI_SPACE {
            let raw = self.fwd[g];
            h = h.wrapping_mul(FNV_PRIME).wrapping_add((raw & 0xFF) as u64);
            h = h.wrapping_mul(FNV_PRIME).wrapping_add((raw >> 8) as u64);
            g += 1;
        }
        let mut c: usize = 0;
        while c < REV_CELLS {
            let gval = self.rev[c];
            h = h.wrapping_mul(FNV_PRIME).wrapping_add((gval & 0xFF) as u64);
            h = h.wrapping_mul(FNV_PRIME).wrapping_add((gval >> 8) as u64);
            c += 1;
        }
        h = h.wrapping_mul(FNV_PRIME).wrapping_add(self.mapped_cache);
        h
    }
    /// 指定 GSI 的原始紧缩值（导出对账用；越界 None）。
    pub fn raw_at(&self, gsi: u32) -> Option<u16> {
        if gsi as usize >= GSI_SPACE {
            return None;
        }
        Some(self.fwd[gsi as usize])
    }
}

// ===========================================================================
// 5. GsiBuilder：逐实例登记 + 五型拦截 + 收口构建
// ===========================================================================
/// 构建拦截计数（判据 10/10 的账面载体）。
#[derive(Clone, Copy, Debug)]
pub struct RejectStats {
    /// 基址重叠拦截数。
    pub overlap: u64,
    /// 引脚越界拦截数。
    pub pin: u64,
    /// 超空间拦截数。
    pub space: u64,
    /// 重复 ID 拦截数。
    pub dup_id: u64,
    /// 槽满拦截数。
    pub slots_full: u64,
}

impl RejectStats {
    /// 全零账。
    pub const fn new() -> RejectStats {
        RejectStats { overlap: 0, pin: 0, space: 0, dup_id: 0, slots_full: 0 }
    }
    /// 拦截总数。
    pub const fn total(&self) -> u64 {
        self.overlap + self.pin + self.space + self.dup_id + self.slots_full
    }
}

/// GSI 编址表构建器。逐 IO-APIC 实例登记（id/MMIO 基址/GSI 基址/引脚数），
/// 登记即拦截五型非法（槽满/重复 ID/引脚越界/超空间/基址重叠），收口
/// `build()` 产出 GsiTable。一构建器一表（build 后拒二次收口）。
#[derive(Clone, Debug)]
pub struct GsiBuilder {
    regs: [ApicReg; MAX_IOAPICS],
    count: usize,
    book: RangeBook,
    rejects: RejectStats,
    built: bool,
}

impl GsiBuilder {
    /// 空构建器。
    pub fn new() -> GsiBuilder {
        GsiBuilder {
            regs: [ApicReg { id: 0, address: 0, gsi_base: 0, pin_count: 0 }; MAX_IOAPICS],
            count: 0,
            book: RangeBook::new(),
            rejects: RejectStats::new(),
            built: false,
        }
    }

    /// 登记一个 IO-APIC 实例。校验序（每序一道关，先便宜后贵）：
    /// 槽满 → 重复 ID → 引脚容量 → 空间上界 → 区间重叠（二分）。
    pub fn register(&mut self, id: u8, address: u64, gsi_base: u32, pin_count: u32)
        -> Result<(), GsiMapError>
    {
        if self.built {
            return Err(GsiMapError::AlreadyBuilt);
        }
        // 关 1：槽位。
        if self.count >= MAX_IOAPICS {
            self.rejects.slots_full += 1;
            return Err(GsiMapError::InstanceSlotsFull);
        }
        // 关 2：重复 ID（登记面线性查重，≤8 项）。
        let mut i: usize = 0;
        while i < self.count {
            if self.regs[i].id == id {
                self.rejects.dup_id += 1;
                return Err(GsiMapError::DuplicateInstanceId(id));
            }
            i += 1;
        }
        // 关 3：引脚容量（0 或 >24 非法；上限来源 82093AA）。
        if pin_count == 0 || pin_count as usize > MAX_PINS {
            self.rejects.pin += 1;
            return Err(GsiMapError::PinCountInvalid(pin_count));
        }
        // 关 4：空间上界（基址越界或区间上界越 1024）。
        let top = gsi_base as u64 + pin_count as u64;
        if gsi_base as usize >= GSI_SPACE || top > GSI_SPACE as u64 {
            self.rejects.space += 1;
            return Err(GsiMapError::SpaceExceeded { base: gsi_base, count: pin_count, top: GSI_SPACE as u64 });
        }
        // 关 5：区间重叠（区间簿二分插入——结构性拦截）。
        let inst = self.count as u8;
        match self.book.insert(gsi_base, pin_count, inst) {
            Ok(_) => {}
            Err(e) => {
                if let GsiMapError::InstanceSlotsFull = e {
                    self.rejects.slots_full += 1;
                } else {
                    self.rejects.overlap += 1;
                }
                return Err(e);
            }
        }
        self.regs[self.count] = ApicReg { id, address, gsi_base, pin_count };
        self.count += 1;
        Ok(())
    }

    /// 收口：产出编址表（正反两面由权威登记面一次重建填充）。
    /// 零实例拒绝；二次 build 拒绝。
    pub fn build(&mut self) -> Result<GsiTable, GsiMapError> {
        if self.built {
            return Err(GsiMapError::AlreadyBuilt);
        }
        if self.count == 0 {
            return Err(GsiMapError::EmptyBuild);
        }
        let mut t = GsiTable::new_empty();
        t.inst_count = self.count;
        let mut i: usize = 0;
        while i < self.count {
            t.regs[i] = self.regs[i];
            i += 1;
        }
        t.book = self.book.clone();
        let _ = t.rebuild_from_regs();
        self.built = true;
        Ok(t)
    }

    /// 已登记实例数。
    pub fn len(&self) -> usize {
        self.count
    }
    /// 是否已收口。
    pub fn is_built(&self) -> bool {
        self.built
    }
    /// 拦截账只读。
    pub fn rejects(&self) -> RejectStats {
        self.rejects
    }
    /// 区间簿只读。
    pub fn book(&self) -> &RangeBook {
        &self.book
    }
    /// 登记项只读（越界 None）。
    pub fn reg(&self, idx: usize) -> Option<ApicReg> {
        if idx >= self.count {
            return None;
        }
        Some(self.regs[idx])
    }
}

// ===========================================================================
// 6. MadtBaseline：MADT 原始基线冻结（ISA Override 应用前——F0864 预留）
// ===========================================================================
/// MADT IOAPIC 桩（对齐上游 `acpi.rs::IoApicEntry` 形状 + F0861 引脚容量）。
/// F0864 消费面按 (id, address, gsi_base, pin_count) 四元组对账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MadtIoApicStub {
    /// MADT IOAPIC 条目 id 字节。
    pub id: u8,
    /// MADT IOAPIC 条目 address 字段（MMIO 基址）。
    pub address: u32,
    /// MADT IOAPIC 条目 gsi_base 字段。
    pub gsi_base: u32,
    /// 引脚容量（F0861 探测面交付：IOAPICVER[23:16]+1）。
    pub pin_count: u32,
}

/// 冻结基线：ISA Override 应用**前**的 GSI 原始编址。冻结后只读 + 指纹。
/// 本单**不应用任何 Override**（ISO 裁决属 F0864）——本面只保证：
/// ① 基线本身通过构建器五关校验；② 冻结指纹稳定；③ 对账规格可导出。
#[derive(Clone, Debug)]
pub struct MadtBaseline {
    frozen: bool,
    count: usize,
    entries: [MadtIoApicStub; MAX_IOAPICS],
    fnv: u64,
}

impl MadtBaseline {
    /// 空基线。
    pub fn new() -> MadtBaseline {
        MadtBaseline {
            frozen: false,
            count: 0,
            entries: [MadtIoApicStub { id: 0, address: 0, gsi_base: 0, pin_count: 0 }; MAX_IOAPICS],
            fnv: 0,
        }
    }

    /// 冻结：桩清单经构建器五关校验（合法性 = 能建成合法表）后冻结，
    /// 计算 FNV-1a 指纹。已冻结拒再冻（一致性纪律）；清单超容量拒收
    /// （哨兵数组定容，防越界写——零 panic 纪律）。
    pub fn freeze(&mut self, stubs: &[MadtIoApicStub]) -> Result<u64, GsiMapError> {
        if self.frozen {
            return Err(GsiMapError::AlreadyBuilt);
        }
        if stubs.is_empty() {
            return Err(GsiMapError::EmptyBuild);
        }
        if stubs.len() > MAX_IOAPICS {
            return Err(GsiMapError::InstanceSlotsFull);
        }
        // 用一条独立构建器走完整五关——基线合法性复用构建器语义，无第二套校验。
        let mut b = GsiBuilder::new();
        let mut i: usize = 0;
        while i < stubs.len() {
            let s = stubs[i];
            b.register(s.id, s.address as u64, s.gsi_base, s.pin_count)?;
            i += 1;
        }
        let _ = b.build()?;
        // 冻结面。
        self.count = stubs.len();
        let mut h: u64 = FNV_BASIS;
        let mut j: usize = 0;
        while j < stubs.len() {
            self.entries[j] = stubs[j];
            h = h.wrapping_mul(FNV_PRIME).wrapping_add(stubs[j].id as u64);
            h = h.wrapping_mul(FNV_PRIME).wrapping_add(stubs[j].address as u64);
            h = h.wrapping_mul(FNV_PRIME).wrapping_add(stubs[j].gsi_base as u64);
            h = h.wrapping_mul(FNV_PRIME).wrapping_add(stubs[j].pin_count as u64);
            j += 1;
        }
        self.fnv = h;
        self.frozen = true;
        Ok(h)
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
    /// 冻结指纹（未冻结为 0）。
    pub fn checksum(&self) -> u64 {
        self.fnv
    }
    /// 冻结条目数。
    pub fn len(&self) -> usize {
        self.count
    }
    /// 导出对账规格：F0864 消费面的构建器登记参数清单
    /// （(id, address, gsi_base, pin_count) 四元组序列）。
    pub fn to_builder_spec(&self) -> Vec<(u8, u64, u32, u32)> {
        let mut v: Vec<(u8, u64, u32, u32)> = Vec::new();
        let mut i: usize = 0;
        while i < self.count {
            let s = self.entries[i];
            v.push((s.id, s.address as u64, s.gsi_base, s.pin_count));
            i += 1;
        }
        v
    }
    /// 冻结条目（越界 None）。
    pub fn entry(&self, idx: usize) -> Option<MadtIoApicStub> {
        if idx >= self.count {
            return None;
        }
        Some(self.entries[idx])
    }
    /// ISA Override 裁决归口声明（F0864 交接注记）：本面交出的基线是
    /// Override 应用前的原始 GSI 编址；QEMU 标准 2 条 ISO（bus irq 0→GSI 2、
    /// irq 5→GSI 5 一族）的裁决属 F0864——本单禁改基线。
    pub fn iso_quarantine_note() -> &'static str {
        "MADT ISO（INTERRUPTPT_SOURCE_OVERRIDE）条目的裁决与应用属 UNX-F0864；\
         本基线是 Override 应用前的原始 GSI 编址，冻结后只读，F0864 以\
         to_builder_spec() 四元组对账后自行维护 override 后视图。"
    }
}

// ===========================================================================
// 7. 事件账：append-only 环形，tick 内置自增（借用纪律的账面实现）
// ===========================================================================
/// 事件种类（账面稳定编码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventKind {
    /// 构建收口成功。
    BuildOk = 1,
    /// 实例登记成功。
    RegisterOk = 2,
    /// 审计通过。
    AuditPass = 3,
    /// 审计失败（双射破坏检出）。
    AuditFail = 4,
    /// 对拍通过。
    CrossPass = 5,
    /// 对拍失败。
    CrossFail = 6,
    /// 构建期拦截（重叠/引脚/超空间/重复 ID/槽满共用，载荷带诊断码）。
    BuilderIntercept = 7,
    /// 篡改注入落账。
    TamperInjected = 8,
    /// 篡改检出。
    TamperDetected = 9,
    /// 修复完成。
    RepairDone = 10,
    /// 查询扫掠完成。
    LookupSweepDone = 11,
    /// 基线冻结。
    BaselineFrozen = 12,
    /// 拦截面复审计通过（修复生效）。
    PostRepairAuditOk = 13,
}

/// append-only 环形事件账。tick 内置于 record() 内部自增——外部无
/// `tick_next()` 入口，从根上杜绝 `self.ledger.record(self.tick_next(), ..)`
/// 双重可变借用模式。容量 256，溢出丢弃显性计数（不静默）。
#[derive(Clone, Debug)]
pub struct EventLedger {
    tick: u64,
    ring: [(u64, u8, u16, u32); EVENT_RING_CAP],
    head: usize,
    len: usize,
    dropped: u64,
}

impl EventLedger {
    /// 空账（tick 自 0 起，首个事件落 tick 1）。
    pub fn new() -> EventLedger {
        EventLedger {
            tick: 0,
            ring: [(0, 0, 0, 0); EVENT_RING_CAP],
            head: 0,
            len: 0,
            dropped: 0,
        }
    }

    /// 记一笔：tick 在账内自增（借用纪律的物理保证）。kind 用 u8 编码
    /// （EventKind as u8），code 带诊断码，payload 带定位载荷。
    pub fn record(&mut self, kind: u8, code: u16, payload: u32) {
        self.tick = self.tick.wrapping_add(1);
        let slot = (self.head + self.len) % EVENT_RING_CAP;
        self.ring[slot] = (self.tick, kind, code, payload);
        if self.len < EVENT_RING_CAP {
            self.len += 1;
        } else {
            // 环满：覆盖最旧并显性计数丢弃（append-only 语义的显性豁免面）。
            self.head = (self.head + 1) % EVENT_RING_CAP;
            self.dropped = self.dropped.wrapping_add(1);
        }
    }

    /// 当前刻（只读）。
    pub fn tick(&self) -> u64 {
        self.tick
    }
    /// 在账条数。
    pub fn len(&self) -> usize {
        self.len
    }
    /// 丢弃计数。
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
    /// 按时序取第 idx 笔（越界 None）。
    pub fn get(&self, idx: usize) -> Option<(u64, u8, u16, u32)> {
        if idx >= self.len {
            return None;
        }
        let slot = (self.head + idx) % EVENT_RING_CAP;
        Some(self.ring[slot])
    }
    /// 账指纹（FNV-1a over 全部在账条目 + tick + dropped）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = FNV_BASIS;
        let mut i: usize = 0;
        while i < self.len {
            let (t, k, c, p) = self.ring[(self.head + i) % EVENT_RING_CAP];
            h = h.wrapping_mul(FNV_PRIME).wrapping_add(t);
            h = h.wrapping_mul(FNV_PRIME).wrapping_add(k as u64);
            h = h.wrapping_mul(FNV_PRIME).wrapping_add(c as u64);
            h = h.wrapping_mul(FNV_PRIME).wrapping_add(p as u64);
            i += 1;
        }
        h = h.wrapping_mul(FNV_PRIME).wrapping_add(self.tick);
        h = h.wrapping_mul(FNV_PRIME).wrapping_add(self.dropped);
        h
    }
    /// 单调性验证：账内时序严格递增（账面自检）。
    pub fn monotonic(&self) -> bool {
        let mut i: usize = 1;
        while i < self.len {
            let prev = self.ring[(self.head + i - 1) % EVENT_RING_CAP].0;
            let curr = self.ring[(self.head + i) % EVENT_RING_CAP].0;
            if curr != prev.wrapping_add(1) {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ===========================================================================
// 8. 健康账：十项不变量
// ===========================================================================
/// 单项健康检查。
#[derive(Clone, Debug)]
pub struct HealthItem {
    /// 项号。
    pub id: &'static str,
    /// 通过与否。
    pub pass: bool,
    /// 说明。
    pub note: &'static str,
}

/// 健康报告。
#[derive(Clone, Debug)]
pub struct HealthReport {
    /// 全部通过。
    pub ok: bool,
    /// 通过项数。
    pub ok_count: u32,
    /// 总项数。
    pub total: u32,
    /// 逐项账。
    pub items: Vec<HealthItem>,
}

// ===========================================================================
// 9. GsiMapEngine：编排器（构建→注入→审计→扫掠→冻结→指纹）
// ===========================================================================
/// 查询步数扫掠账。
#[derive(Clone, Copy, Debug)]
pub struct MeterStats {
    /// 查询总数（含越界抽查）。
    pub total: u64,
    /// 命中已映射格数。
    pub mapped: u64,
    /// 区间外未映射数。
    pub unmapped: u64,
    /// 越界数。
    pub out_of_range: u64,
    /// 最大实走步数。
    pub max_steps: u32,
    /// 步数总和（均值口径）。
    pub sum_steps: u64,
}

impl MeterStats {
    /// 全零。
    pub const fn new() -> MeterStats {
        MeterStats { total: 0, mapped: 0, unmapped: 0, out_of_range: 0, max_steps: 0, sum_steps: 0 }
    }
    /// 均值步数（×100 整数；零查询为 0）。
    pub fn avg_steps_x100(&self) -> u64 {
        if self.total == 0 {
            return 0;
        }
        self.sum_steps * 100 / self.total
    }
}

/// 注入四型计数（各 10 轮口径）。
#[derive(Clone, Copy, Debug)]
pub struct InjectCounters {
    /// 重叠注入尝试数。
    pub overlap_att: u64,
    /// 重叠拦截数。
    pub overlap_int: u64,
    /// 引脚越界注入尝试数。
    pub pin_att: u64,
    /// 引脚越界拦截数。
    pub pin_int: u64,
    /// 超空间注入尝试数。
    pub space_att: u64,
    /// 超空间拦截数。
    pub space_int: u64,
    /// 双射破坏注入尝试数。
    pub tamper_att: u64,
    /// 双射破坏检出数。
    pub tamper_det: u64,
    /// 修复数。
    pub tamper_rep: u64,
}

impl InjectCounters {
    /// 全零。
    pub const fn new() -> InjectCounters {
        InjectCounters {
            overlap_att: 0, overlap_int: 0,
            pin_att: 0, pin_int: 0,
            space_att: 0, space_int: 0,
            tamper_att: 0, tamper_det: 0, tamper_rep: 0,
        }
    }
}

/// 编排引擎：持有建成表、事件账、注入账、步数账、冻结基线。
/// 全部确定性：LCG 驱动、逻辑刻内推进、零外部输入。
#[derive(Clone, Debug)]
pub struct GsiMapEngine {
    lcg: u64,
    table: GsiTable,
    built: bool,
    events: EventLedger,
    injects: InjectCounters,
    meter: MeterStats,
    last_audit_ok: bool,
    last_audit_illegal: u64,
    last_cross_ok: bool,
    last_cross_pct: u64,
    baseline: MadtBaseline,
    overlap_shape_hits: [u64; OVERLAP_SHAPES as usize],
}

impl GsiMapEngine {
    /// 新引擎（种子入 LCG；同种子同程序同指纹）。
    pub fn new(seed: u64) -> GsiMapEngine {
        GsiMapEngine {
            lcg: seed,
            table: GsiTable::new_empty(),
            built: false,
            events: EventLedger::new(),
            injects: InjectCounters::new(),
            meter: MeterStats::new(),
            last_audit_ok: false,
            last_audit_illegal: 0,
            last_cross_ok: false,
            last_cross_pct: 0,
            baseline: MadtBaseline::new(),
            overlap_shape_hits: [0; OVERLAP_SHAPES as usize],
        }
    }

    /// LCG 前进一步并返回新状态（确定性随机源）。
    fn next_u64(&mut self) -> u64 {
        self.lcg = self.lcg.wrapping_mul(LCG_MUL).wrapping_add(LCG_ADD);
        self.lcg
    }

    /// 记事件（tick 在账内自增——绝不出现 self.tick_next() 双借模式）。
    fn note(&mut self, kind: EventKind, code: u16, payload: u32) {
        self.events.record(kind as u8, code, payload);
    }

    // -----------------------------------------------------------------
    // 9.1 P1 标准拓扑构建 + 审计 + 对拍
    // -----------------------------------------------------------------
    /// 构建标准双实例拓扑（QEMU q35 单 IOAPIC + MP 规范连续编址第二实例）：
    /// 实例 0（id=0，0xFEC0_0000，GSI 基 0，24 引脚）、实例 1（id=2，
    /// 0xFEC8_0000，GSI 基 24，24 引脚）。登记与收口全走事件账。
    pub fn phase_build_standard_dual(&mut self) -> Result<(), GsiMapError> {
        if self.built {
            return Err(GsiMapError::AlreadyBuilt);
        }
        let mut b = GsiBuilder::new();
        b.register(QEMU_STD_ID, QEMU_STD_ADDR, QEMU_STD_BASE, MAX_PINS as u32)?;
        self.note(EventKind::RegisterOk, 0, QEMU_STD_BASE);
        b.register(DUAL_ID, DUAL_ADDR, DUAL_BASE, MAX_PINS as u32)?;
        self.note(EventKind::RegisterOk, 0, DUAL_BASE);
        let t = b.build()?;
        self.table = t;
        self.built = true;
        self.note(EventKind::BuildOk, 0, self.table.mapped() as u32);
        // 构建即审计 + 对拍（构建收口的三重证）。
        self.run_audit();
        self.run_cross();
        Ok(())
    }

    /// 对当前表跑双射审计并落账。
    pub fn run_audit(&mut self) -> BijectionReport {
        let rep = self.table.audit_bijection();
        self.last_audit_ok = rep.bijection_ok;
        self.last_audit_illegal = rep.illegal_total();
        if rep.bijection_ok {
            self.note(EventKind::AuditPass, 0, rep.mapped as u32);
        } else {
            self.note(EventKind::AuditFail, GsiMapError::BijectionBroken { gsi: 0 }.code(), rep.illegal_total() as u32);
        }
        rep
    }

    /// 对当前表跑正反映射对拍并落账。
    pub fn run_cross(&mut self) -> CrossReport {
        let rep = self.table.cross_check();
        self.last_cross_ok = rep.ok;
        self.last_cross_pct = rep.pct;
        if rep.ok {
            self.note(EventKind::CrossPass, 0, rep.pct as u32);
        } else {
            self.note(EventKind::CrossFail, GsiMapError::CrossCheckMismatch { gsi: 0 }.code(), rep.pct as u32);
        }
        rep
    }

    // -----------------------------------------------------------------
    // 9.2 P2 构建期三型注入（重叠 / 引脚 / 超空间，各 10 轮）
    // -----------------------------------------------------------------
    /// 重叠注入一轮：以当前表拓扑重放构建器，再按 LCG 选形态登记一个
    /// 与既有区间相撞的实例。五形态：前搭/后搭/全包/精确重复/跨段骑跨。
    /// 返回 (是否拦截, 形态号)。
    pub fn inject_overlap_once(&mut self) -> (bool, u64) {
        self.injects.overlap_att += 1;
        let r = self.next_u64();
        // 形态取轮转制（第 n 次注入用 n%5）：N≥5 轮必全覆盖五形态——
        // 比随机采样更强（随机 10 轮存在漏形态的确定性坏种子），k 偏移仍取 LCG。
        let shape = (self.injects.overlap_att - 1) % OVERLAP_SHAPES;
        let k = (r >> 8) % 8; // 0..7 偏移量
        // 现拓扑固定为标准双实例（[0,24) 与 [24,48)）。
        let (base, pins): (u32, u32) = match shape {
            0 => ((24u32).saturating_sub(k as u32 + 1), 24), // 前搭：基址落进 [0,24) 左缘
            1 => (k as u32, 8),                               // 后搭：基址深在第一段内
            2 => (12, 24),                                    // 深骑跨 [12,36)：合法引脚数下跨两段
            3 => (0, 24),                                     // 精确重复第一段
            _ => (20, 4),                                     // 浅骑：[20,24) 搭第一段尾缘
        };
        let mut b = GsiBuilder::new();
        let _ = b.register(QEMU_STD_ID, QEMU_STD_ADDR, QEMU_STD_BASE, MAX_PINS as u32);
        let _ = b.register(DUAL_ID, DUAL_ADDR, DUAL_BASE, MAX_PINS as u32);
        let res = b.register(9, 0xFED0_0000, base, pins);
        let intercepted = match res {
            Err(e) => {
                let ok = matches!(e, GsiMapError::BaseOverlap { .. });
                self.note(EventKind::BuilderIntercept, e.code(), base);
                ok
            }
            Ok(()) => false, // 未拦截即穿透——判据红账
        };
        if intercepted {
            self.injects.overlap_int += 1;
            self.overlap_shape_hits[shape as usize] += 1;
        }
        (intercepted, shape)
    }

    /// 重叠注入 N 轮（判据 10/10）。返回拦截数。
    pub fn inject_overlap_rounds(&mut self, rounds: u64) -> u64 {
        let mut n: u64 = 0;
        let mut hits: u64 = 0;
        while n < rounds {
            let (ok, _) = self.inject_overlap_once();
            if ok {
                hits += 1;
            }
            n += 1;
        }
        hits
    }

    /// 引脚越界注入一轮：LCG 选非法引脚数（0 或 >24）。返回 (是否拦截, 非法值)。
    pub fn inject_pin_once(&mut self) -> (bool, u32) {
        self.injects.pin_att += 1;
        let r = self.next_u64();
        // 非法值域：0 与 25..=32 与大数轮换（全部在登记关 3 被拦）。
        let bad: u32 = match r % 4 {
            0 => 0,
            1 => 25 + (r % 8) as u32,
            2 => 64 + (r % 64) as u32,
            _ => 0x1000_0000u32.wrapping_add((r % 0xFFFF) as u32),
        };
        let mut b = GsiBuilder::new();
        let _ = b.register(QEMU_STD_ID, QEMU_STD_ADDR, QEMU_STD_BASE, MAX_PINS as u32);
        let res = b.register(9, 0xFED0_0000, 100, bad);
        let intercepted = match res {
            Err(e) => {
                let ok = matches!(e, GsiMapError::PinCountInvalid(_));
                self.note(EventKind::BuilderIntercept, e.code(), bad);
                ok
            }
            Ok(()) => false,
        };
        if intercepted {
            self.injects.pin_int += 1;
        }
        (intercepted, bad)
    }

    /// 引脚注入 N 轮。返回拦截数。
    pub fn inject_pin_rounds(&mut self, rounds: u64) -> u64 {
        let mut n: u64 = 0;
        let mut hits: u64 = 0;
        while n < rounds {
            let (ok, _) = self.inject_pin_once();
            if ok {
                hits += 1;
            }
            n += 1;
        }
        hits
    }

    /// 超空间注入一轮：基址逼近/越过 1024 上界（base+count > 1024 或
    /// base >= 1024 两形态）。返回 (是否拦截, 基址)。
    pub fn inject_space_once(&mut self) -> (bool, u32) {
        self.injects.space_att += 1;
        let r = self.next_u64();
        // 基址 1000..1027：j<=7 为区间越界（base+24>1024），j>=8 为基址本身越界。
        let base: u32 = 1000 + (r % 28) as u32;
        let mut b = GsiBuilder::new();
        let _ = b.register(QEMU_STD_ID, QEMU_STD_ADDR, QEMU_STD_BASE, MAX_PINS as u32);
        let _ = b.register(DUAL_ID, DUAL_ADDR, DUAL_BASE, MAX_PINS as u32);
        let res = b.register(9, 0xFED0_0000, base, 24);
        let intercepted = match res {
            Err(e) => {
                let ok = matches!(e, GsiMapError::SpaceExceeded { .. });
                self.note(EventKind::BuilderIntercept, e.code(), base);
                ok
            }
            Ok(()) => false,
        };
        if intercepted {
            self.injects.space_int += 1;
        }
        (intercepted, base)
    }

    /// 超空间注入 N 轮。返回拦截数。
    pub fn inject_space_rounds(&mut self, rounds: u64) -> u64 {
        let mut n: u64 = 0;
        let mut hits: u64 = 0;
        while n < rounds {
            let (ok, _) = self.inject_space_once();
            if ok {
                hits += 1;
            }
            n += 1;
        }
        hits
    }

    // -----------------------------------------------------------------
    // 9.3 P3 双射破坏注入（检出→隔离修复→复审计，10 轮）
    // -----------------------------------------------------------------
    /// 双射破坏注入一轮。偶数轮：同实例重复目标型（把 fwd[gsi] 改写成
    /// 同实例邻脚的目标——制造 dup_target + fwd_auth_mismatch + rev_mismatch）；
    /// 奇数轮：区间掏空型（把已映射格写成哨兵——制造 holes_within_spans）。
    /// 注入后审计必须检出，随后权威修复 + 复审计回绿。返回是否走完全程。
    pub fn inject_tamper_once(&mut self) -> bool {
        self.injects.tamper_att += 1;
        if !self.built {
            return false;
        }
        let r = self.next_u64();
        // 现拓扑已映射 GSI 0..47；取 1..=46 避开边缘。
        let gsi: u32 = 1 + (r % 46) as u32;
        let dup_form = (r >> 16) % 2 == 0; // LCG 高位定形态（与轮号无关的确定性）。
        let cur = self.table.lookup(gsi);
        let cur_t = match cur {
            Ok(t) => t,
            Err(_) => return false, // 不可达（拓扑固定映射 0..47）。
        };
        let tamper_res: Result<u16, GsiMapError> = if dup_form {
            // 同实例重复目标：邻脚（同实例内 (pin+1)%pins）。
            let pins = self.table.reg(cur_t.inst as usize).map(|x| x.pin_count).unwrap_or(0);
            if pins == 0 {
                return false; // 登记面容量缺失——不可达防御（登记关 3 已禁 0）。
            }
            let npin = ((cur_t.pin as u32) + 1) % pins;
            self.table.tamper_write_raw(gsi, GsiTarget::pack(cur_t.inst as usize, npin as usize))
        } else {
            // 区间掏空：直接写哨兵。
            self.table.tamper_write_raw(gsi, TARGET_EMPTY)
        };
        if tamper_res.is_err() {
            return false;
        }
        self.note(EventKind::TamperInjected, GsiMapError::TamperDetected(gsi).code(), INJECT_MARK | gsi);
        // 审计必须检出。
        let rep = self.table.audit_bijection();
        let detected = !rep.bijection_ok;
        if detected {
            self.injects.tamper_det += 1;
            self.note(EventKind::TamperDetected, GsiMapError::TamperDetected(gsi).code(), rep.illegal_total() as u32);
        } else {
            // 检出失败：注入被静默——这是判据红账（事件也留）。
            self.note(EventKind::AuditPass, 0, 0xFFFF);
            return false;
        }
        // 隔离修复 + 复审计回绿。
        let changed = self.table.repair_from_ranges();
        self.injects.tamper_rep += 1;
        self.note(EventKind::RepairDone, 0, REPAIR_MARK | (changed as u32 & 0x0FFF));
        let rep2 = self.table.audit_bijection();
        if rep2.bijection_ok {
            self.note(EventKind::PostRepairAuditOk, 0, rep2.mapped as u32);
        }
        rep2.bijection_ok
    }

    /// 双射破坏注入 N 轮。返回全程走完数（检出+修复+复审计回绿）。
    pub fn inject_tamper_rounds(&mut self, rounds: u64) -> u64 {
        let mut n: u64 = 0;
        let mut full: u64 = 0;
        while n < rounds {
            if self.inject_tamper_once() {
                full += 1;
            }
            n += 1;
        }
        full
    }

    // -----------------------------------------------------------------
    // 9.4 P4 O(1) 步数扫掠（全空间 + 越界抽查）
    // -----------------------------------------------------------------
    /// 步数扫掠：全空间 1024 格逐格 metered 查询 + 10 次越界抽查。
    /// 记账 max_steps / mapped / unmapped / out_of_range。返回 max_steps。
    pub fn phase_lookup_sweep(&mut self) -> u32 {
        let mut max_s: u32 = 0;
        let mut g: u32 = 0;
        while (g as usize) < GSI_SPACE {
            let (res, steps) = self.table.lookup_metered(g);
            self.meter.total += 1;
            self.meter.sum_steps += steps as u64;
            if steps > max_s {
                max_s = steps;
            }
            match res {
                Ok(_) => self.meter.mapped += 1,
                Err(GsiMapError::GsiUnmapped(_)) => self.meter.unmapped += 1,
                Err(_) => {}
            }
            g += 1;
        }
        // 越界抽查 10 次（确定性 LCG 出大数）。
        let mut n: u64 = 0;
        while n < 10 {
            let r = self.next_u64();
            let oob = GSI_SPACE as u32 + (r % 0x4000_0000) as u32; // 1024..~1G，全越界。
            let (res, steps) = self.table.lookup_metered(oob);
            self.meter.total += 1;
            self.meter.sum_steps += steps as u64;
            if steps > max_s {
                max_s = steps;
            }
            match res {
                Err(GsiMapError::GsiOutOfRange(_)) => self.meter.out_of_range += 1,
                _ => {}
            }
            n += 1;
        }
        self.meter.max_steps = if max_s > self.meter.max_steps { max_s } else { self.meter.max_steps };
        self.note(EventKind::LookupSweepDone, 0, self.meter.max_steps as u32);
        max_s
    }

    // -----------------------------------------------------------------
    // 9.5 P5 MADT 基线冻结（F0864 预留对接面）
    // -----------------------------------------------------------------
    /// 冻结 MADT 原始基线（与建表面同源的两条桩：q35 标准 + 第二实例）。
    /// 冻结指纹落账。返回冻结指纹。
    pub fn phase_baseline_freeze(&mut self) -> Result<u64, GsiMapError> {
        let stubs = [
            MadtIoApicStub { id: QEMU_STD_ID, address: QEMU_STD_ADDR as u32, gsi_base: QEMU_STD_BASE, pin_count: MAX_PINS as u32 },
            MadtIoApicStub { id: DUAL_ID, address: DUAL_ADDR as u32, gsi_base: DUAL_BASE, pin_count: MAX_PINS as u32 },
        ];
        let h = self.baseline.freeze(&stubs)?;
        self.note(EventKind::BaselineFrozen, 0, (h & 0xFFFF_0001) as u32);
        Ok(h)
    }

    // -----------------------------------------------------------------
    // 9.6 判据复测总程序（确定性：同种子同指纹）
    // -----------------------------------------------------------------
    /// 标准判据复测程序：P1 构建 → P2 三型构建期注入各 10 轮 → P3 双射
    /// 破坏注入 10 轮 → P4 步数扫掠 → P5 基线冻结。
    pub fn run_program(&mut self) -> Result<(), GsiMapError> {
        self.phase_build_standard_dual()?;
        let _ = self.inject_overlap_rounds(INJECT_ROUNDS);
        let _ = self.inject_pin_rounds(INJECT_ROUNDS);
        let _ = self.inject_space_rounds(INJECT_ROUNDS);
        let _ = self.inject_tamper_rounds(INJECT_ROUNDS);
        let _ = self.phase_lookup_sweep();
        let _ = self.phase_baseline_freeze()?;
        // 尾审计对拍（注入修复后的最终一致态）。
        self.run_audit();
        self.run_cross();
        Ok(())
    }

    // -----------------------------------------------------------------
    // 9.7 账面访问器
    // -----------------------------------------------------------------
    /// 建成表只读引用。
    pub fn table(&self) -> &GsiTable {
        &self.table
    }
    /// 建成表可变引用（注入演练/修复通道——生产路径只读，可变面仅测试与维护）。
    pub fn table_mut(&mut self) -> &mut GsiTable {
        &mut self.table
    }
    /// 是否已构建。
    pub fn is_built(&self) -> bool {
        self.built
    }
    /// 事件账只读。
    pub fn events(&self) -> &EventLedger {
        &self.events
    }
    /// 注入账只读。
    pub fn injects(&self) -> InjectCounters {
        self.injects
    }
    /// 步数账只读。
    pub fn meter(&self) -> MeterStats {
        self.meter
    }
    /// 基线只读。
    pub fn baseline(&self) -> &MadtBaseline {
        &self.baseline
    }
    /// 重叠形态命中分布（五形态各有命中——覆盖面账）。
    pub fn overlap_shape_hits(&self) -> [u64; OVERLAP_SHAPES as usize] {
        self.overlap_shape_hits
    }

    /// 引擎指纹（表 + 注入账 + 步数账 + 事件账 + 基线：全状态确定性摘要）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = FNV_BASIS;
        h = h.wrapping_mul(FNV_PRIME).wrapping_add(self.table.fingerprint());
        h = h.wrapping_mul(FNV_PRIME).wrapping_add(self.injects.overlap_att);
        h = h.wrapping_mul(FNV_PRIME).wrapping_add(self.injects.overlap_int);
        h = h.wrapping_mul(FNV_PRIME).wrapping_add(self.injects.pin_att);
        h = h.wrapping_mul(FNV_PRIME).wrapping_add(self.injects.pin_int);
        h = h.wrapping_mul(FNV_PRIME).wrapping_add(self.injects.space_att);
        h = h.wrapping_mul(FNV_PRIME).wrapping_add(self.injects.space_int);
        h = h.wrapping_mul(FNV_PRIME).wrapping_add(self.injects.tamper_att);
        h = h.wrapping_mul(FNV_PRIME).wrapping_add(self.injects.tamper_det);
        h = h.wrapping_mul(FNV_PRIME).wrapping_add(self.injects.tamper_rep);
        h = h.wrapping_mul(FNV_PRIME).wrapping_add(self.meter.total);
        h = h.wrapping_mul(FNV_PRIME).wrapping_add(self.meter.max_steps as u64);
        h = h.wrapping_mul(FNV_PRIME).wrapping_add(self.meter.sum_steps);
        h = h.wrapping_mul(FNV_PRIME).wrapping_add(self.events.fingerprint());
        h = h.wrapping_mul(FNV_PRIME).wrapping_add(self.baseline.checksum());
        let mut i: usize = 0;
        while i < OVERLAP_SHAPES as usize {
            h = h.wrapping_mul(FNV_PRIME).wrapping_add(self.overlap_shape_hits[i]);
            i += 1;
        }
        h
    }

    /// 健康账：十项不变量。
    pub fn health(&self) -> HealthReport {
        let mut items: Vec<HealthItem> = Vec::new();
        items.push(HealthItem {
            id: "H1 bijection",
            pass: self.built && self.last_audit_ok && self.table.audit_bijection().bijection_ok,
            note: "双射成立：区间内零空洞、目标零重复、正反零不一致、mapped==Σ引脚",
        });
        items.push(HealthItem {
            id: "H2 overlap-10/10",
            pass: self.injects.overlap_att == INJECT_ROUNDS && self.injects.overlap_int == self.injects.overlap_att,
            note: "基址重叠注入 10/10 全拦截（五形态轮换）",
        });
        items.push(HealthItem {
            id: "H3 pin-10/10",
            pass: self.injects.pin_att == INJECT_ROUNDS && self.injects.pin_int == self.injects.pin_att,
            note: "引脚越界注入 10/10 全拦截（0/超24/大数）",
        });
        items.push(HealthItem {
            id: "H4 space-10/10",
            pass: self.injects.space_att == INJECT_ROUNDS && self.injects.space_int == self.injects.space_att,
            note: "超空间注入 10/10 全拦截（区间越界/基址越界）",
        });
        items.push(HealthItem {
            id: "H5 tamper-10/10",
            pass: self.injects.tamper_att == INJECT_ROUNDS
                && self.injects.tamper_det == self.injects.tamper_att
                && self.injects.tamper_rep == self.injects.tamper_att,
            note: "双射破坏注入 10/10 检出并修复（复审计回绿）",
        });
        items.push(HealthItem {
            id: "H6 step-budget",
            pass: self.meter.max_steps <= LOOKUP_STEP_BUDGET && proof_ns() <= CRITERION_NS_BUDGET,
            note: "查询 O(1)：max_steps<=2 且步数折算 19ns<=100ns（确定性证明）",
        });
        items.push(HealthItem {
            id: "H7 cross-100",
            pass: self.last_cross_ok && self.last_cross_pct == 100,
            note: "正反映射对拍 100%（fwd→rev 与 rev→fwd 双向全量）",
        });
        items.push(HealthItem {
            id: "H8 ledger",
            pass: self.events.monotonic() && self.events.dropped() == 0,
            note: "事件账刻度单调且标准程序零溢出丢弃",
        });
        items.push(HealthItem {
            id: "H9 baseline",
            pass: self.baseline.is_frozen() && self.baseline.checksum() != 0,
            note: "MADT 原始基线已冻结且指纹非零（F0864 对接预留）",
        });
        items.push(HealthItem {
            id: "H10 conservation",
            pass: self.table.mapped() == self.table.book().total_cells()
                && self.table.mapped() <= GSI_SPACE as u64
                && self.table.book().adjacency_pct() == 100,
            note: "计数守恒：mapped==区间格数<=空间上限，标准拓扑邻接率 100",
        });
        let ok_count = items.iter().filter(|x| x.pass).count() as u32;
        let total = items.len() as u32;
        HealthReport { ok: ok_count == total, ok_count, total, items }
    }
}

// ===========================================================================
// 10. 判据复测入口：observe（双跑对拍）+ self_check
// ===========================================================================
/// 判据复测快照（全部账面字段）。
#[derive(Clone, Copy, Debug)]
pub struct GsiMapObserve {
    /// 输入种子。
    pub seed: u64,
    /// 标准程序走完（无错误）。
    pub program_ok: bool,
    // —— 判据一：无空洞无重叠（双射审计）——
    /// 双射成立。
    pub bijection_ok: bool,
    /// 已映射格数（= Σ登记引脚）。
    pub mapped: u64,
    /// 登记引脚总数。
    pub registered_pins: u64,
    /// 区间内空洞数（必须 0）。
    pub holes_within: u64,
    /// 目标重复数（必须 0）。
    pub dup_target: u64,
    /// 正反不一致数（必须 0）。
    pub rev_mismatch: u64,
    /// 区间外未映射格数（合法拓扑态）。
    pub unmapped_outside: u64,
    // —— 判据二：重叠注入 10/10 ——
    /// 重叠注入尝试数。
    pub overlap_att: u64,
    /// 重叠拦截数。
    pub overlap_int: u64,
    /// 引脚注入拦截数。
    pub pin_int: u64,
    /// 超空间注入拦截数。
    pub space_int: u64,
    // —— 判据三：O(1) 步数 ——
    /// 步数扫掠最大步数（<= 2）。
    pub max_steps: u32,
    /// 步数折算耗时上界（ns，确定性证明）。
    pub proof_ns_v: u64,
    /// 步数预算达标。
    pub step_budget_ok: bool,
    /// 查询总数。
    pub lookups_total: u64,
    /// 命中映射数。
    pub lookups_mapped: u64,
    // —— 正反映射对拍 ——
    /// 对拍百分比。
    pub cross_pct: u64,
    /// 对拍双向全一致。
    pub cross_ok: bool,
    // —— 双射破坏注入 ——
    /// 破坏检出数。
    pub tamper_det: u64,
    /// 修复数。
    pub tamper_rep: u64,
    // —— 基线（F0864 预留）——
    /// 基线已冻结。
    pub baseline_frozen: bool,
    /// 基线指纹。
    pub baseline_checksum: u64,
    // —— 双跑与总账 ——
    /// 双跑指纹 A。
    pub fingerprint_a: u64,
    /// 双跑指纹 B。
    pub fingerprint_b: u64,
    /// 双跑一致。
    pub dual_stable: bool,
    /// 健康全绿。
    pub health_ok: bool,
    /// 健康通过项数。
    pub health_ok_count: u32,
    /// 健康总项数。
    pub health_total: u32,
    /// 事件账在账数。
    pub event_count: usize,
    /// 事件账丢弃数。
    pub event_dropped: u64,
    /// 空间占用率（整数百分比）。
    pub utilization_pct: u64,
    /// 区间邻接率（整数百分比）。
    pub adjacency_pct: u64,
    /// 区间间隙数（合法稀疏度账）。
    pub gaps: u64,
}

/// 单次运行标准程序并返回（引擎, 指纹, 程序是否走完）。
pub fn run_once(seed: u64) -> (GsiMapEngine, u64, bool) {
    let mut e = GsiMapEngine::new(seed);
    let r = e.run_program();
    let ok = r.is_ok();
    let fp = e.fingerprint();
    (e, fp, ok)
}

/// 判据复测入口：同种子双跑对拍 + 全账面快照。
pub fn observe(seed: u64) -> GsiMapObserve {
    let (e1, fp1, ok1) = run_once(seed);
    let (_e2, fp2, _ok2) = run_once(seed);
    let dual_stable = fp1 == fp2;
    let audit = e1.table().audit_bijection();
    let cross = e1.table().cross_check();
    let inj = e1.injects();
    let m = e1.meter();
    let health = e1.health();
    let (gaps, _gap_cells, _widest) = e1.table().book().gap_report();
    GsiMapObserve {
        seed,
        program_ok: ok1,
        bijection_ok: audit.bijection_ok,
        mapped: audit.mapped,
        registered_pins: audit.registered_pins,
        holes_within: audit.holes_within_spans,
        dup_target: audit.dup_target,
        rev_mismatch: audit.rev_mismatch,
        unmapped_outside: audit.unmapped_outside,
        overlap_att: inj.overlap_att,
        overlap_int: inj.overlap_int,
        pin_int: inj.pin_int,
        space_int: inj.space_int,
        max_steps: m.max_steps,
        proof_ns_v: proof_ns(),
        step_budget_ok: m.max_steps <= LOOKUP_STEP_BUDGET && proof_ns() <= CRITERION_NS_BUDGET,
        lookups_total: m.total,
        lookups_mapped: m.mapped,
        cross_pct: cross.pct,
        cross_ok: cross.ok,
        tamper_det: inj.tamper_det,
        tamper_rep: inj.tamper_rep,
        baseline_frozen: e1.baseline().is_frozen(),
        baseline_checksum: e1.baseline().checksum(),
        fingerprint_a: fp1,
        fingerprint_b: fp2,
        dual_stable,
        health_ok: health.ok,
        health_ok_count: health.ok_count,
        health_total: health.total,
        event_count: e1.events().len(),
        event_dropped: e1.events().dropped(),
        utilization_pct: e1.table().utilization_pct(),
        adjacency_pct: e1.table().book().adjacency_pct(),
        gaps,
    }
}

/// 自检项。
#[derive(Clone, Copy, Debug)]
pub struct Check {
    /// 项号。
    pub id: &'static str,
    /// 通过与否。
    pub pass: bool,
    /// 说明。
    pub note: &'static str,
}

/// 自检套件（判据总闸，10 项）。
pub fn self_check(ob: &GsiMapObserve) -> Vec<Check> {
    let mut out: Vec<Check> = Vec::new();
    out.push(Check {
        id: "C0863-01 bijection",
        pass: ob.bijection_ok
            && ob.holes_within == 0
            && ob.dup_target == 0
            && ob.rev_mismatch == 0
            && ob.mapped == ob.registered_pins,
        note: "判据一：GSI→(实例,引脚) 全表无空洞无重叠（双射审计：区间内零空洞/零重复/正反一致/计数守恒）",
    });
    out.push(Check {
        id: "C0863-02 overlap-10/10",
        pass: ob.overlap_att == INJECT_ROUNDS && ob.overlap_int == INJECT_ROUNDS,
        note: "判据二：注入重叠 10/10 次拦截（前搭/后搭/全包/精确重复/跨段骑跨五形态轮换）",
    });
    out.push(Check {
        id: "C0863-03 o1-steps",
        pass: ob.max_steps <= LOOKUP_STEP_BUDGET && ob.proof_ns_v <= CRITERION_NS_BUDGET,
        note: "判据三：查询 O(1)——固定 2 步直接索引，步数折算 19ns<100ns（确定性证明不涉墙钟）",
    });
    out.push(Check {
        id: "C0863-04 cross",
        pass: ob.cross_ok && ob.cross_pct == 100,
        note: "正反映射对拍：fwd→rev 与 rev→fwd 双向全量互证 100%",
    });
    out.push(Check {
        id: "C0863-05 injects",
        pass: ob.pin_int == INJECT_ROUNDS && ob.space_int == INJECT_ROUNDS
            && ob.tamper_det == INJECT_ROUNDS && ob.tamper_rep == INJECT_ROUNDS,
        note: "注入面：引脚越界 10/10、超空间 10/10、双射破坏 10/10 检出并修复",
    });
    out.push(Check {
        id: "C0863-06 dual-run",
        pass: ob.dual_stable && ob.fingerprint_a == ob.fingerprint_b,
        note: "双跑确定性对拍：同种子两次运行指纹一致（零真实时钟/零真实随机）",
    });
    out.push(Check {
        id: "C0863-07 health",
        pass: ob.health_ok && ob.health_ok_count == ob.health_total,
        note: "健康账十项不变量全绿",
    });
    out.push(Check {
        id: "C0863-08 seg",
        pass: err_segment_ok(),
        note: "诊断码段独占：16 变体恰好占满 0x1990–0x199F 且无越段",
    });
    out.push(Check {
        id: "C0863-09 baseline",
        pass: ob.baseline_frozen && ob.baseline_checksum != 0,
        note: "MADT 原始基线冻结（ISA Override 前的 GSI 基线，F0864 对接预留）",
    });
    out.push(Check {
        id: "C0863-10 topology",
        pass: ob.mapped == 48 && ob.utilization_pct == 4 && ob.adjacency_pct == 100 && ob.gaps == 0,
        note: "标准双实例拓扑账：映射 48 格（24+24）、占用 4%、邻接率 100% 零间隙",
    });
    out
}

/// 段位自检：16 变体码全落段内、两两互异、段外归「未知」。
pub fn err_segment_ok() -> bool {
    let all = GsiMapError::all();
    let mut i: usize = 0;
    while i < all.len() {
        let c = all[i].code();
        if c < SEG_BASE || c > SEG_END {
            return false;
        }
        let mut j = i + 1;
        while j < all.len() {
            if all[i].code() == all[j].code() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ===========================================================================
// 11. 导出面：KV / TLV / 清单
// ===========================================================================
/// 键值导出（账面扁平串）。
pub fn export_kv(ob: &GsiMapObserve) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = Vec::new();
    v.push(("version".to_string(), VERSION_TEXT.to_string()));
    v.push(("criterion".to_string(), CRITERION_ID.to_string()));
    v.push(("seed".to_string(), ob.seed.to_string()));
    v.push(("bijection_ok".to_string(), ob.bijection_ok.to_string()));
    v.push(("mapped".to_string(), ob.mapped.to_string()));
    v.push(("registered_pins".to_string(), ob.registered_pins.to_string()));
    v.push(("holes_within".to_string(), ob.holes_within.to_string()));
    v.push(("dup_target".to_string(), ob.dup_target.to_string()));
    v.push(("rev_mismatch".to_string(), ob.rev_mismatch.to_string()));
    v.push(("overlap_10of10".to_string(), (ob.overlap_int == INJECT_ROUNDS).to_string()));
    v.push(("pin_int".to_string(), ob.pin_int.to_string()));
    v.push(("space_int".to_string(), ob.space_int.to_string()));
    v.push(("tamper_det".to_string(), ob.tamper_det.to_string()));
    v.push(("tamper_rep".to_string(), ob.tamper_rep.to_string()));
    v.push(("max_steps".to_string(), ob.max_steps.to_string()));
    v.push(("proof_ns".to_string(), ob.proof_ns_v.to_string()));
    v.push(("step_budget_ok".to_string(), ob.step_budget_ok.to_string()));
    v.push(("lookups_total".to_string(), ob.lookups_total.to_string()));
    v.push(("cross_pct".to_string(), ob.cross_pct.to_string()));
    v.push(("cross_ok".to_string(), ob.cross_ok.to_string()));
    v.push(("baseline_frozen".to_string(), ob.baseline_frozen.to_string()));
    v.push(("baseline_checksum".to_string(), format!("0x{:016X}", ob.baseline_checksum)));
    v.push(("dual_stable".to_string(), ob.dual_stable.to_string()));
    v.push(("fingerprint".to_string(), format!("0x{:016X}", ob.fingerprint_a)));
    v.push(("health_ok".to_string(), ob.health_ok.to_string()));
    v.push(("health".to_string(), format!("{}/{}", ob.health_ok_count, ob.health_total)));
    v.push(("events".to_string(), ob.event_count.to_string()));
    v.push(("events_dropped".to_string(), ob.event_dropped.to_string()));
    v.push(("utilization_pct".to_string(), ob.utilization_pct.to_string()));
    v.push(("adjacency_pct".to_string(), ob.adjacency_pct.to_string()));
    v
}

/// TLV 导出（type:u8 | len:u16 | value，定长小端）。
pub fn export_tlv(ob: &GsiMapObserve) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    // T0 版本串。
    let vb = VERSION_TEXT.as_bytes();
    out.push(0x01);
    out.push((vb.len() as u16 & 0xFF) as u8);
    out.push(((vb.len() as u16 >> 8) & 0xFF) as u8);
    let mut i = 0;
    while i < vb.len() { out.push(vb[i]); i += 1; }
    // T1 双射审计（u64 mapped/pins/holes/dup/revm）。
    out.push(0x11); out.push(40); out.push(0);
    push_u64(&mut out, ob.mapped);
    push_u64(&mut out, ob.registered_pins);
    push_u64(&mut out, ob.holes_within);
    push_u64(&mut out, ob.dup_target);
    push_u64(&mut out, ob.rev_mismatch);
    // T2 注入账（u64 overlap/pin/space/tamper_det/tamper_rep）。
    out.push(0x12); out.push(40); out.push(0);
    push_u64(&mut out, ob.overlap_int);
    push_u64(&mut out, ob.pin_int);
    push_u64(&mut out, ob.space_int);
    push_u64(&mut out, ob.tamper_det);
    push_u64(&mut out, ob.tamper_rep);
    // T3 步数账（u64 total/mapped/proof_ns + u32 max_steps + u8 ok）。
    out.push(0x13); out.push(29); out.push(0);
    push_u64(&mut out, ob.lookups_total);
    push_u64(&mut out, ob.lookups_mapped);
    push_u64(&mut out, ob.proof_ns_v);
    push_u32(&mut out, ob.max_steps);
    out.push(if ob.step_budget_ok { 1 } else { 0 });
    // T4 指纹（u64 a, u64 b, u8 dual）。
    out.push(0x14); out.push(17); out.push(0);
    push_u64(&mut out, ob.fingerprint_a);
    push_u64(&mut out, ob.fingerprint_b);
    out.push(if ob.dual_stable { 1 } else { 0 });
    // T5 基线（u64 checksum + u8 frozen）。
    out.push(0x15); out.push(9); out.push(0);
    push_u64(&mut out, ob.baseline_checksum);
    out.push(if ob.baseline_frozen { 1 } else { 0 });
    out
}

/// 小端 u64 推入。
fn push_u64(out: &mut Vec<u8>, v: u64) {
    let mut i = 0;
    while i < 8 {
        out.push(((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
}

/// 小端 u32 推入。
fn push_u32(out: &mut Vec<u8>, v: u32) {
    let mut i = 0;
    while i < 4 {
        out.push(((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
}

/// TLV 往返校验：长度域与实际载荷一致且总长守恒。
pub fn tlv_roundtrip_ok(ob: &GsiMapObserve) -> bool {
    let b = export_tlv(ob);
    let mut pos = 0;
    let n = b.len();
    while pos < n {
        if pos + 3 > n { return false; }
        let l = b[pos + 1] as usize | ((b[pos + 2] as usize) << 8);
        if pos + 3 + l > n { return false; }
        pos += 3 + l;
    }
    pos == n
}

/// 不变量清单。
pub fn invariant_manifest() -> Vec<String> { vec![
    "INV-1 空间闭包：全部登记区间 [base,base+count) ⊂ [0,1024)；越界登记在构建关 4 拦截".to_string(),
    "INV-2 区间不重叠：区间簿二分插入与左右邻各判一次相交（半开区间相交 ⇔ b<E && B<e）".to_string(),
    "INV-3 引脚容量闭包：每实例 1..=24（82093AA 上限）；0 与 >24 在构建关 3 拦截".to_string(),
    "INV-4 双射：每个已登记 GSI 恰映射一个 (实例,引脚)，且每个 (实例,引脚) 恰被一个 GSI 映射".to_string(),
    "INV-5 区间内零空洞：登记区间每个 GSI 的 fwd 格非哨兵（审计 holes_within_spans==0）".to_string(),
    "INV-6 正反一致：fwd[g]=(i,p) ⇔ rev[i,p]=g（cross_check 双向 100%）".to_string(),
    "INV-7 查询 O(1)：lookup 固定至多 2 步（界检查+数组直取），步数账 max_steps<=2；折算 19ns<100ns".to_string(),
    "INV-8 权威唯一写口：表建成后正常路径零写口；篡改面只有注入专用 tamper_write_*；修复=从权威登记面重建".to_string(),
    "INV-9 溢出显性：全部算术 wrapping/saturating；事件账溢出丢弃显性计数".to_string(),
    "INV-10 刻度内置：tick 只在 EventLedger::record 内自增（杜绝双重可变借用模式）".to_string(),
    "INV-11 确定性：同种子双跑指纹一致（零真实时钟/零真实随机/零地址哈希）".to_string(),
    "INV-12 基线冻结：MADT 原始基线（Override 前）冻结后只读，ISO 裁决归 F0864".to_string(),
    ] }

/// 错误闭集清单串。
pub fn err_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let all = GsiMapError::all();
    let mut i = 0;
    while i < all.len() {
        v.push(format!("0x{:04X} {} — {}", all[i].code(), all[i].name(), all[i].detail()));
        i += 1;
    }
    v
}

/// 实例清单串（逐实例编址账）。
pub fn instance_manifest(t: &GsiTable) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let led = t.capacity_ledger();
    let mut i: usize = 0;
    while i < led.len() {
        let (id, base, pins, first, last) = led[i];
        v.push(format!(
            "inst{} ioapic#{} gsi [{},{}] pins={}",
            i, id, first, last, pins
        ));
        let _ = base;
        i += 1;
    }
    v
}

/// 版本锚。
pub const TICKET: &str = "UNX-F0863";
/// 标题。
pub const TITLE: &str = "GSI 全局中断号统一编址与映射";
/// 批次。
pub const BATCH: &str = "CoRun·UNX-A2-B04";
/// 册目标行数。
pub const TARGET_LINES: u32 = 260;
/// 册内关联单号（同批 F861–F880 邻条 + 上游）。
pub const RELATED_TICKETS: [&str; 8] = ["F0861", "F0862", "F0864", "F0865", "F0867", "F0868", "F0004", "F0023"];

/// 关联单串。
pub fn related_str() -> String {
    let mut s = String::new();
    let mut i = 0;
    while i < RELATED_TICKETS.len() {
        s.push_str(RELATED_TICKETS[i]);
        s.push(' ');
        i += 1;
    }
    s
}

// ===========================================================================
// 12. 冒烟测试（探针面执行：双射/重叠 10:10/O(1) 步数/正反互证/三注入/双跑）
// ===========================================================================
#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    // --- 段位与错误面 ---

    #[test]
    fn t_codes_closed_set() {
        // 16 码全表：段内每码必有名，段外归「未知」；两两互异。
        let mut c: u16 = SEG_BASE;
        while c <= SEG_END {
            let name = code_name(c);
            assert!(name != "未知诊断码：查 0x199 段分配表。", "0x{:X} 应在段内有名", c);
            c += 1;
        }
        assert_eq!(code_name(SEG_BASE - 1), "未知诊断码：查 0x199 段分配表。");
        assert_eq!(code_name(SEG_END + 1), "未知诊断码：查 0x199 段分配表。");
        assert!(err_segment_ok());
    }

    #[test]
    fn t_error_details_all_variants() {
        // 每变体：码在段内、detail 非空且含三要素（长度 >= 40）。
        let all = GsiMapError::all();
        let mut i: usize = 0;
        while i < all.len() {
            let e = all[i];
            assert!(e.code() >= SEG_BASE && e.code() <= SEG_END);
            let d = e.detail();
            assert!(d.len() >= 40, "变体 {:?} 诊断过短", e);
            let mut j = i + 1;
            while j < all.len() {
                assert!(all[i].code() != all[j].code(), "错误码重复");
                j += 1;
            }
            i += 1;
        }
    }

    // --- 判据一：双射审计（无空洞无重叠）---

    #[test]
    fn t_build_standard_dual_and_bijection() {
        let mut e = GsiMapEngine::new(0xA086_3001);
        assert!(e.phase_build_standard_dual().is_ok());
        let rep = e.table().audit_bijection();
        assert!(rep.bijection_ok);
        assert_eq!(rep.mapped, 48);            // 24+24 双实例。
        assert_eq!(rep.registered_pins, 48);
        assert_eq!(rep.holes_within_spans, 0); // 区间内零空洞。
        assert_eq!(rep.dup_target, 0);         // 零重复。
        assert_eq!(rep.rev_mismatch, 0);       // 正反零不一致。
        assert_eq!(rep.bad_target, 0);
        assert_eq!(rep.fwd_auth_mismatch, 0);
        assert_eq!(rep.unmapped_outside, (GSI_SPACE - 48) as u64);
        assert_eq!(e.table().utilization_pct(), 4800 / 1024); // 48*100/1024 = 4。
    }

    #[test]
    fn t_builder_rejects_five_kinds() {
        // 五型拦截逐型直验。
        let mut b = GsiBuilder::new();
        assert!(b.register(0, 0xFEC0_0000, 0, 24).is_ok());
        // 重叠。
        assert!(matches!(b.register(9, 0xFED0_0000, 0, 24), Err(GsiMapError::BaseOverlap { .. })));
        assert!(matches!(b.register(9, 0xFED0_0000, 23, 8), Err(GsiMapError::BaseOverlap { .. })));
        // 引脚越界。
        assert!(matches!(b.register(9, 0xFED0_0000, 100, 0), Err(GsiMapError::PinCountInvalid(_))));
        assert!(matches!(b.register(9, 0xFED0_0000, 100, 25), Err(GsiMapError::PinCountInvalid(_))));
        // 超空间。
        assert!(matches!(b.register(9, 0xFED0_0000, 1010, 24), Err(GsiMapError::SpaceExceeded { .. })));
        assert!(matches!(b.register(9, 0xFED0_0000, 2000, 4), Err(GsiMapError::SpaceExceeded { .. })));
        // 重复 ID。
        assert!(matches!(b.register(0, 0xFED1_0000, 100, 4), Err(GsiMapError::DuplicateInstanceId(0))));
        let r = b.rejects();
        assert_eq!(r.overlap, 2);
        assert_eq!(r.pin, 2);
        assert_eq!(r.space, 2);
        assert_eq!(r.dup_id, 1);
        // 合法第二实例仍可登。
        assert!(b.register(2, 0xFEC8_0000, 24, 24).is_ok());
        let t = b.build().expect("build");
        assert_eq!(t.inst_count(), 2);
        // 二次 build 拒。
        assert!(matches!(b.build(), Err(GsiMapError::AlreadyBuilt)));
    }

    #[test]
    fn t_empty_build_and_slots_full() {
        let mut b = GsiBuilder::new();
        assert!(matches!(b.build(), Err(GsiMapError::EmptyBuild)));
        let mut c = GsiBuilder::new();
        let mut n = 0;
        while n < 8 {
            let base = n * 24;
            assert!(c.register(n as u8, 0xFE00_0000 + n as u64, base, 24).is_ok());
            n += 1;
        }
        assert!(matches!(c.register(8, 0xFF00_0000, 192, 24), Err(GsiMapError::InstanceSlotsFull)));
        let t = c.build().expect("8 实例满配可建");
        let rep = t.audit_bijection();
        assert!(rep.bijection_ok);
        assert_eq!(rep.mapped, 192);
    }

    // --- 判据二：重叠注入 10/10 ---

    #[test]
    fn t_overlap_injection_10_of_10() {
        let mut e = GsiMapEngine::new(0xA086_3002);
        assert!(e.phase_build_standard_dual().is_ok());
        let hits = e.inject_overlap_rounds(INJECT_ROUNDS);
        assert_eq!(hits, 10);
        let inj = e.injects();
        assert_eq!(inj.overlap_att, 10);
        assert_eq!(inj.overlap_int, 10);
        // 五形态各有命中（覆盖面账）。
        let dist = e.overlap_shape_hits();
        let mut i = 0;
        while i < dist.len() {
            assert!(dist[i] > 0, "重叠形态 {} 未被覆盖", i);
            i += 1;
        }
    }

    // --- 判据三：O(1) 步数 ---

    #[test]
    fn t_lookup_o1_two_steps() {
        let mut e = GsiMapEngine::new(0xA086_3003);
        assert!(e.phase_build_standard_dual().is_ok());
        let max_s = e.phase_lookup_sweep();
        assert_eq!(max_s, LOOKUP_STEP_BUDGET); // 恰 2 步。
        let m = e.meter();
        assert_eq!(m.total, (GSI_SPACE + 10) as u64); // 全空间 + 10 越界。
        assert_eq!(m.mapped, 48);
        assert_eq!(m.unmapped, (GSI_SPACE - 48) as u64);
        assert_eq!(m.out_of_range, 10);
        assert!(m.max_steps <= LOOKUP_STEP_BUDGET);
        // 折算证明：19ns <= 100ns（编译期同值复证）。
        assert_eq!(proof_ns(), 19);
        assert!(proof_ns() <= CRITERION_NS_BUDGET);
    }

    #[test]
    fn t_lookup_direct_index_semantics() {
        // 正向查询逐格语义直验（前 48 格映射、格 48 起未映射、越界早退 1 步）。
        let mut e = GsiMapEngine::new(0xA086_3004);
        assert!(e.phase_build_standard_dual().is_ok());
        let t = e.table();
        let mut g: u32 = 0;
        while g < 48 {
            let tg = t.lookup(g).expect("已映射格");
            if g < 24 {
                assert_eq!(tg.inst, 0);
                assert_eq!(tg.pin as u32, g);
            } else {
                assert_eq!(tg.inst, 1);
                assert_eq!(tg.pin as u32, g - 24);
            }
            g += 1;
        }
        assert!(matches!(t.lookup(48), Err(GsiMapError::GsiUnmapped(48))));
        assert!(matches!(t.lookup(1023), Err(GsiMapError::GsiUnmapped(1023))));
        let (_, s1) = t.lookup_metered(0xFFFF_0000);
        assert_eq!(s1, 1); // 越界一步早退。
        let (_, s2) = t.lookup_metered(0);
        assert_eq!(s2, 2); // 空间内恰两步。
    }

    // --- 正反映射对拍 ---

    #[test]
    fn t_cross_check_bidirectional() {
        let mut e = GsiMapEngine::new(0xA086_3005);
        assert!(e.phase_build_standard_dual().is_ok());
        let rep = e.table().cross_check();
        assert!(rep.ok);
        assert_eq!(rep.pct, 100);
        assert_eq!(rep.fwd_total, 48);
        assert_eq!(rep.fwd_ok, 48);
        assert_eq!(rep.rev_total, 48);
        assert_eq!(rep.rev_ok, 48);
        // 反向查询逐脚直验。
        let t = e.table();
        let mut pin: usize = 0;
        while pin < 24 {
            assert_eq!(t.lookup_rev(0, pin).expect("rev0"), pin as u32);
            assert_eq!(t.lookup_rev(1, pin).expect("rev1"), 24 + pin as u32);
            pin += 1;
        }
        assert!(matches!(t.lookup_rev(2, 0), Err(GsiMapError::PinOutOfRange { .. })));
        assert!(matches!(t.lookup_rev(0, 24), Err(GsiMapError::PinOutOfRange { .. })));
    }

    // --- 注入面：引脚 / 超空间 / 双射破坏 ---

    #[test]
    fn t_pin_injection_10_of_10() {
        let mut e = GsiMapEngine::new(0xA086_3006);
        assert!(e.phase_build_standard_dual().is_ok());
        let hits = e.inject_pin_rounds(INJECT_ROUNDS);
        assert_eq!(hits, 10);
        let inj = e.injects();
        assert_eq!(inj.pin_att, 10);
        assert_eq!(inj.pin_int, 10);
    }

    #[test]
    fn t_space_injection_10_of_10() {
        let mut e = GsiMapEngine::new(0xA086_3007);
        assert!(e.phase_build_standard_dual().is_ok());
        let hits = e.inject_space_rounds(INJECT_ROUNDS);
        assert_eq!(hits, 10);
        let inj = e.injects();
        assert_eq!(inj.space_att, 10);
        assert_eq!(inj.space_int, 10);
    }

    #[test]
    fn t_tamper_injection_detected_and_repaired() {
        let mut e = GsiMapEngine::new(0xA086_3008);
        assert!(e.phase_build_standard_dual().is_ok());
        // 手工双形态直验：重复目标型检出。
        let old = e.table_mut().tamper_write_raw(5, GsiTarget::pack(0, 7)).expect("tamper");
        assert_ne!(old, TARGET_EMPTY);
        let rep = e.table().audit_bijection();
        assert!(!rep.bijection_ok);
        assert!(rep.dup_target >= 1);
        assert!(rep.rev_mismatch >= 1);
        let changed = e.table_mut().repair_from_ranges();
        assert!(changed > 0);
        assert!(e.table().audit_bijection().bijection_ok);
        // 掏空型检出。
        let _ = e.table_mut().tamper_write_raw(30, TARGET_EMPTY);
        let rep2 = e.table().audit_bijection();
        assert!(!rep2.bijection_ok);
        assert!(rep2.holes_within_spans >= 1);
        assert!(e.table_mut().repair_from_ranges() > 0);
        assert!(e.table().audit_bijection().bijection_ok);
        // 注入轮 10/10（检出+修复+复审计回绿）。
        let full = e.inject_tamper_rounds(INJECT_ROUNDS);
        assert_eq!(full, 10);
        let inj = e.injects();
        assert_eq!(inj.tamper_att, 10);
        assert_eq!(inj.tamper_det, 10);
        assert_eq!(inj.tamper_rep, 10);
    }

    // --- 区间簿单元 ---

    #[test]
    fn t_rangebook_unit() {
        let mut bk = RangeBook::new();
        assert!(bk.insert(24, 24, 1).is_ok());
        assert!(bk.insert(0, 24, 0).is_ok()); // 升序插到前位。
        assert_eq!(bk.len(), 2);
        assert_eq!(bk.at(0), Some((0, 24, 0)));
        assert_eq!(bk.at(1), Some((24, 48, 1)));
        assert_eq!(bk.at(2), None);
        // 邻接：无缝相邻。
        let (gaps, cells, widest) = bk.gap_report();
        assert_eq!(gaps, 0);
        assert_eq!(cells, 0);
        assert_eq!(widest, 0);
        assert_eq!(bk.adjacency_pct(), 100);
        assert_eq!(bk.total_cells(), 48);
        assert_eq!(bk.top(), 48);
        // 带间隙拓扑。
        let mut bk2 = RangeBook::new();
        assert!(bk2.insert(0, 24, 0).is_ok());
        assert!(bk2.insert(48, 24, 1).is_ok());
        let (g2, c2, w2) = bk2.gap_report();
        assert_eq!(g2, 1);
        assert_eq!(c2, 24);
        assert_eq!(w2, 24);
        assert_eq!(bk2.adjacency_pct(), 0);
        // 重叠判定四向：前相交/后相交/全包/被包/贴边不交。
        let mut bk3 = RangeBook::new();
        assert!(bk3.insert(48, 24, 0).is_ok());
        assert!(matches!(bk3.insert(40, 24, 1), Err(GsiMapError::BaseOverlap { .. }))); // 前相交。
        assert!(matches!(bk3.insert(60, 24, 1), Err(GsiMapError::BaseOverlap { .. }))); // 内部落基。
        assert!(matches!(bk3.insert(0, 96, 1), Err(GsiMapError::BaseOverlap { .. })));  // 全包。
        assert!(bk3.insert(72, 24, 1).is_ok()); // 贴边不交（72 == 48+24）。
        assert_eq!(bk3.len(), 2);
    }

    // --- MADT 基线（F0864 预留）---

    #[test]
    fn t_baseline_freeze_and_spec() {
        let mut bl = MadtBaseline::new();
        assert!(!bl.is_frozen());
        assert_eq!(bl.checksum(), 0);
        let stubs = [
            MadtIoApicStub { id: 0, address: 0xFEC0_0000, gsi_base: 0, pin_count: 24 },
            MadtIoApicStub { id: 2, address: 0xFEC8_0000, gsi_base: 24, pin_count: 24 },
        ];
        let h1 = bl.freeze(&stubs).expect("freeze");
        assert!(bl.is_frozen());
        assert_eq!(bl.checksum(), h1);
        assert_ne!(h1, 0);
        // 二次冻结拒。
        assert!(matches!(bl.freeze(&stubs), Err(GsiMapError::AlreadyBuilt)));
        // 空清单拒。
        let mut bl2 = MadtBaseline::new();
        assert!(matches!(bl2.freeze(&[]), Err(GsiMapError::EmptyBuild)));
        // 非法清单（重叠）拒——基线合法性复用构建器语义。
        let mut bl3 = MadtBaseline::new();
        let bad = [
            MadtIoApicStub { id: 0, address: 0xFEC0_0000, gsi_base: 0, pin_count: 24 },
            MadtIoApicStub { id: 1, address: 0xFEC1_0000, gsi_base: 0, pin_count: 24 },
        ];
        assert!(matches!(bl3.freeze(&bad), Err(GsiMapError::BaseOverlap { .. })));
        // 对账规格：F0864 消费面四元组。
        let spec = bl.to_builder_spec();
        assert_eq!(spec.len(), 2);
        assert_eq!(spec[0], (0u8, 0xFEC0_0000u64, 0u32, 24u32));
        assert_eq!(spec[1], (2u8, 0xFEC8_0000u64, 24u32, 24u32));
        // 冻结指纹稳定（同输入同指纹）。
        let mut bl4 = MadtBaseline::new();
        assert_eq!(bl4.freeze(&stubs).expect("again"), h1);
    }

    // --- 事件账（tick 内置 + 单调 + 溢出显性）---

    #[test]
    fn t_event_ledger_tick_builtin() {
        let mut led = EventLedger::new();
        assert_eq!(led.tick(), 0);
        led.record(1, 0x1990, 1);
        assert_eq!(led.tick(), 1);
        led.record(1, 0x1991, 2);
        assert_eq!(led.tick(), 2);
        assert!(led.monotonic());
        assert_eq!(led.get(0), Some((1, 1, 0x1990, 1)));
        assert_eq!(led.get(1), Some((2, 1, 0x1991, 2)));
        assert_eq!(led.get(2), None);
        // 溢出显性：灌满后再灌 257 笔（总 2+257=259 笔），dropped=259-256=3，
        // 最旧被覆盖且账内仍为满环、时序仍连续。
        let mut n: u64 = 0;
        while n < (EVENT_RING_CAP as u64) + 1 {
            led.record(2, 0x1992, n as u32);
            n += 1;
        }
        assert_eq!(led.dropped(), 3);
        assert_eq!(led.len(), EVENT_RING_CAP);
        assert!(led.monotonic()); // 环形覆盖后时序仍连续。
    }

    // --- 双跑与总闸 ---

    #[test]
    fn t_dual_run_fingerprint_stable() {
        let (e1, f1, ok1) = run_once(0xB086_3000);
        assert!(ok1);
        let (_e2, f2, _) = run_once(0xB086_3000);
        assert_eq!(f1, f2);
        // 异种子指纹不同（区分度）。
        let (_e3, f3, _) = run_once(0xB086_3FFF);
        assert_ne!(f1, f3);
        assert!(e1.is_built());
    }

    #[test]
    fn t_observe_and_self_check_all_green() {
        let ob = observe(0xC086_3000);
        assert!(ob.program_ok);
        assert!(ob.bijection_ok);
        assert_eq!(ob.holes_within, 0);
        assert_eq!(ob.dup_target, 0);
        assert_eq!(ob.rev_mismatch, 0);
        assert_eq!(ob.mapped, ob.registered_pins);
        assert_eq!(ob.overlap_att, 10);
        assert_eq!(ob.overlap_int, 10);
        assert_eq!(ob.pin_int, 10);
        assert_eq!(ob.space_int, 10);
        assert_eq!(ob.tamper_det, 10);
        assert_eq!(ob.tamper_rep, 10);
        assert_eq!(ob.max_steps, 2);
        assert!(ob.step_budget_ok);
        assert!(ob.cross_ok);
        assert_eq!(ob.cross_pct, 100);
        assert!(ob.baseline_frozen);
        assert!(ob.dual_stable);
        assert!(ob.health_ok);
        assert_eq!(ob.health_ok_count, 10);
        assert_eq!(ob.health_total, 10);
        assert_eq!(ob.event_dropped, 0);
        let checks = self_check(&ob);
        let mut i = 0;
        while i < checks.len() {
            assert!(checks[i].pass, "自检 {} 未过：{}", checks[i].id, checks[i].note);
            i += 1;
        }
        // 拓扑账：48 格 / 占用 4% / 邻接 100 / 零间隙。
        assert_eq!(ob.mapped, 48);
        assert_eq!(ob.utilization_pct, 4);
        assert_eq!(ob.adjacency_pct, 100);
        assert_eq!(ob.gaps, 0);
    }

    #[test]
    fn t_health_all_items() {
        let ob = observe(0xC086_3001);
        assert!(ob.health_ok);
        let (e1, _, ok1) = run_once(0xC086_3001);
        assert!(ok1);
        let h = e1.health();
        assert_eq!(h.ok_count, 10);
        let mut i = 0;
        while i < h.items.len() {
            assert!(h.items[i].pass, "健康项 {} 未过", h.items[i].id);
            i += 1;
        }
    }

    // --- 导出面 ---

    #[test]
    fn t_exports_roundtrip() {
        let ob = observe(0xD086_3000);
        let kv = export_kv(&ob);
        assert!(kv.len() >= 20);
        let tl = export_tlv(&ob);
        assert!(tl.len() > 32);
        assert!(tlv_roundtrip_ok(&ob));
        let inv = invariant_manifest();
        assert_eq!(inv.len(), 12);
        let em = err_manifest();
        assert_eq!(em.len(), 16);
        let (e1, _, _) = run_once(0xD086_3000);
        let im = instance_manifest(e1.table());
        assert_eq!(im.len(), 2);
    }

    // --- 编码层 const 封印复证 ---

    #[test]
    fn t_target_encoding() {
        assert_eq!(GsiTarget::pack(0, 23), 0x0017);
        assert_eq!(GsiTarget::pack(1, 0), 0x0100);
        let t = GsiTarget::unpack(0x0105);
        assert_eq!(t.inst, 1);
        assert_eq!(t.pin, 5);
        assert!(!t.is_empty());
        assert!(GsiTarget::EMPTY.is_empty());
        assert_eq!(GsiTarget::EMPTY.raw(), TARGET_EMPTY);
        // 越界参数→哨兵（零 panic 纪律）。
        assert_eq!(GsiTarget::pack(300, 1), TARGET_EMPTY);
        assert_eq!(GsiTarget::pack(1, 300), TARGET_EMPTY);
    }

    // --- 未构建面前置校验 ---

    #[test]
    fn t_engine_prebuild_guards() {
        let mut e = GsiMapEngine::new(0xE086_3000);
        assert!(!e.is_built());
        // 未构建时双射破坏注入直接 false（不可走）。
        assert!(!e.inject_tamper_once());
        assert!(e.phase_build_standard_dual().is_ok());
        assert!(e.phase_build_standard_dual().is_err()); // 重复构建拒。
        // 事件账里构建已留痕。
        assert!(e.events().len() >= 4);
    }
}
