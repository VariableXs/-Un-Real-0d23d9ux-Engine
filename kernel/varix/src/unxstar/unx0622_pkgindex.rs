//! UNX-F0622 · 交接包索引表（域级检索）账 · 诊断段 0x14D0~0x14DF
//!
//! ═══════════════════════════════════════════════════════════════════
//! 锚点头（判据与定位原文，全文抄录）
//! ═══════════════════════════════════════════════════════════════════
//! 源文件：docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md
//! 定位原文（第 2177 行，UNX-A1-B32 · A1 域交接总装（F621–F640 · 20 条）批表内一行）：
//!
//!   | UNX-F0622 | 交接包索引表（域级检索） | 220 | 已深化 | 索引表账齐（12 行×4
//!   列+行校验和），按包号检索 10/10 用例命中一致 |
//!
//! J 判据拆解（原文抄录，逐项可达断言落点）：
//!   UNX-F0622-J1-a 索引表账齐：12 行×4 列+行校验和
//!     —— 落点 [`PkgIndexTable`]（12 行稠密槽位）+ [`PkgIndexRow`]（4 列域号/包基址/
//!        包长度/版本 + 第 5 列行校验和 FNV16 真算）+ [`PkgIndexTable::freeze`] 账齐门。
//!   UNX-F0622-J1-b 按包号检索 10/10 用例命中一致
//!     —— 落点 const 用例表 [`LOOKUP_CASES`]（10 例：8 命中 + 2 显性 Miss）+
//!        [`run_lookup_cases`] 运行时逐例核，自证计数必须 10/10。
//!
//! 上游：UNX-F0621 域交接包聚合（12 包总装——锚点 F0349/F0378/F0413/F0439/F0453/
//!   F0476/F0497/F0518/F0538/F0559/F0593/F0617 全登记 12/12，聚合索引表冻结）；
//!   本模块即该"聚合索引表冻结"的账本化落地：12 域 1:1 认领 12 锚点（见
//!   [`DOMAIN_TABLE`]，映射认领序注释于表）。
//! 下游：UNX-F0623 跨包一致性核（版本 v1 对齐时戳有序）与 UNX-F0624/F0625/F0626
//!   三族消费 mock 均经本表按包号取包基址/长度/版本，不经散表直读。
//!
//! 深度子系统清单（全为真账，无空壳）：
//!   1. [`HandoffDomain`] 12 域闭集 + const 域表 [`DOMAIN_TABLE`]（带域号/锚点）。
//!   2. [`PkgIndexRow`] 索引行：4 数据列 + 行校验和列（域号~版本四字段 FNV16 折叠真算）。
//!   3. [`PkgIndexTable`] 12 行×4 列索引表：行校验和逐行核验 + 表级校验和（FNV32）+
//!      表指纹（FNV1a64）；重复域号/域号越界/基址未对齐/零长/溢出/坏版本拒收。
//!   4. [`PkgIndexTable::lookup_by_pkg`] 按包号 O(1) 直解码 + [`PkgIndexTable::lookup_by_domain`]
//!      按域 O(12) 线性扫行双路，双路一致性有账。
//!   5. 未登记域检索显性 Miss（[`PkgIdxError::UnregisteredDomain`]），绝不返回零基址谎报。
//!   6. 快照导出：确定性序列化 [`PkgIndexTable::snapshot`] + 往返 [`PkgIndexTable::from_bytes`]
//!      + FNV1a64 指纹。
//!   7. 事件账 [`EventLedger`]（环形 64 槽，seq 单调，溢出丢弃显性计数）+ [`PkgIdxHealth`]。
//!   8. 注入面三检出：行校验和坏注入 / 重复域注入 / 越界检索注入（各落诊断码）。
//!   9. 双跑确定性对拍 [`determinism_dual`]（双建标准表：指纹与快照字节全等）。
//!  10. 自检总入口 [`run_unx0622_selfcheck`]（≥16 项逐条判据）。
//!
//! 纪律自证：完全自包含（仅 core/alloc，零 `crate::` 依赖）；no_std+alloc 内核路径
//! 零 unwrap/expect/panic!/运行期 assert!；错误全走 `Result<T, PkgIdxError>`，每变体
//! 带 `detail()` 中文可操作诊断；溢出显性 wrapping/saturating/checked；本文件全部
//! 结构体 ≤ 64KiB（编译期断言在文末）；const fn 仅纯值计算（while+索引，无 for/无
//! slice::get/无 Vec 索引/无 Ord::min/max/无派生 == 比较）。

use alloc::vec::Vec;

// ═══════════════════════════════════════════════════════════════════
// §1 诊断段独占 · 0x14D0~0x14DF（16 码全占）
// ═══════════════════════════════════════════════════════════════════
// 段占用核查（2026-10-24 全库 grep）：0x14C0 段已被 svstar2/ves447_s24perf.rs
// SEG_BASE=0x14C0 占用；0x14E0 段已被 svstar2/ves448_s24sec.rs SEG_BASE=0x14E0
// 占用；0x14D0~0x14DF 全库无任何占用 → 本账取 0x14D0。
pub const SEG_BASE: u16 = 0x14D0; // 段基（源：0x14C0/0x14E0 已占，取下一段 0x14D0）
pub const SEG_END: u16 = 0x14DF; // 段尾（含）

// 错误码（0x14D0~0x14DC，13 码）
pub const E_DOMAIN_RANGE: u16 = SEG_BASE + 0x0; // 0x14D0 域号越界（非 1..=12）
pub const E_DUP_DOMAIN: u16 = SEG_BASE + 0x1; // 0x14D1 重复域号拒收
pub const E_BASE_UNALIGNED: u16 = SEG_BASE + 0x2; // 0x14D2 包基址未 4KiB 对齐
pub const E_ZERO_LEN: u16 = SEG_BASE + 0x3; // 0x14D3 包长度为零
pub const E_LEN_OVERFLOW: u16 = SEG_BASE + 0x4; // 0x14D4 长度超限/基址+长度溢出
pub const E_VERSION_INVALID: u16 = SEG_BASE + 0x5; // 0x14D5 版本非法（零版本/主版本零）
pub const E_TABLE_INCOMPLETE: u16 = SEG_BASE + 0x6; // 0x14D6 表未账齐（<12 行）
pub const E_TABLE_FROZEN: u16 = SEG_BASE + 0x7; // 0x14D7 冻结后拒改
pub const E_BAD_PKG_NO: u16 = SEG_BASE + 0x8; // 0x14D8 包号越界（解码失败）
pub const E_UNREGISTERED: u16 = SEG_BASE + 0x9; // 0x14D9 未登记域/空槽检索 Miss
pub const E_ROW_CSUM: u16 = SEG_BASE + 0xA; // 0x14DA 行校验和不符
pub const E_TABLE_CSUM: u16 = SEG_BASE + 0xB; // 0x14DB 表级校验和不符
pub const E_SNAPSHOT: u16 = SEG_BASE + 0xC; // 0x14DC 快照结构损坏

// 事件码（0x14DD~0x14DF，3 码；非错误，仅入账）
pub const EV_REGISTER_OK: u16 = SEG_BASE + 0xD; // 0x14DD 行登记成功
pub const EV_LOOKUP_HIT: u16 = SEG_BASE + 0xE; // 0x14DE 检索命中
pub const EV_LOOKUP_MISS: u16 = SEG_BASE + 0xF; // 0x14DF 检索显性 Miss

/// 错误/事件码是否落在本段内（段独占自证用）。
pub const fn diag_code_in_seg(code: u16) -> bool {
    code >= SEG_BASE && code <= SEG_END
}

// ═══════════════════════════════════════════════════════════════════
// §2 编译期账目断言（const 期，非运行期）
// ═══════════════════════════════════════════════════════════════════
const _: () = assert!(SEG_BASE == 0x14D0 && SEG_END == 0x14DF);
const _: () = assert!(DOMAIN_COUNT == 12);
const _: () = assert!(ROW_COLS == 4);
const _: () = assert!(LOOKUP_CASES_TOTAL == 10);
const _: () = assert!(LEDGER_CAP == 64);
const _: () = assert!(PKG_NO_BASE == 0x620);
const _: () = assert!(PKG_BASE_ALIGN == 0x1000);

// ═══════════════════════════════════════════════════════════════════
// §3 哈希原语（const fn，纯值计算）
// ═══════════════════════════════════════════════════════════════════

/// FNV-1 16 位（行校验和用）。
/// 源：FNV-1 16bit offset basis 0x811C、prime 0x0193（FNV 参考实现常量）。
pub const FNV16_BASIS: u16 = 0x811C;
pub const FNV16_PRIME: u16 = 0x0193;

/// FNV-1 32 位（表级校验和用）。
/// 源：FNV-1 32bit offset basis 0x811C9DC5、prime 0x01000193。
pub const FNV32_BASIS: u32 = 0x811C_9DC5;
pub const FNV32_PRIME: u32 = 0x0100_0193;

/// FNV-1a 64 位（表指纹/快照指纹用）。
/// 源：FNV-1a 64bit offset basis 0xCBF29CE484222325、prime 0x100000001B3。
pub const FNV64_BASIS: u64 = 0xCBF2_9CE4_8422_2325;
pub const FNV64_PRIME: u64 = 0x0000_0100_0000_01B3;

/// FNV-1 16 位折叠：逐字节 异或→乘素数（wrapping 显性，防溢出未定义）。
/// const fn 纪律：while+索引，无 for、无 slice::get。
pub const fn fnv16(data: &[u8]) -> u16 {
    let mut h: u16 = FNV16_BASIS;
    let mut i: usize = 0;
    while i < data.len() {
        h ^= data[i] as u16;
        h = h.wrapping_mul(FNV16_PRIME);
        i += 1;
    }
    h
}

/// FNV-1 32 位折叠（表级校验和）。
pub const fn fnv32(data: &[u8]) -> u32 {
    let mut h: u32 = FNV32_BASIS;
    let mut i: usize = 0;
    while i < data.len() {
        h ^= data[i] as u32;
        h = h.wrapping_mul(FNV32_PRIME);
        i += 1;
    }
    h
}

/// FNV-1a 64 位折叠（表指纹/快照指纹）。
pub const fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = FNV64_BASIS;
    let mut i: usize = 0;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(FNV64_PRIME);
        i += 1;
    }
    h
}

/// u16 小端写入（调用方保证 out.len() >= off+2，仅内部固定布局使用）。
fn put_u16(out: &mut [u8], off: usize, v: u16) {
    out[off] = (v & 0xFF) as u8;
    out[off + 1] = (v >> 8) as u8;
}

/// u32 小端写入。
fn put_u32(out: &mut [u8], off: usize, v: u32) {
    let mut i: usize = 0;
    while i < 4 {
        out[off + i] = ((v >> (8 * i)) & 0xFF) as u8;
        i += 1;
    }
}

/// u64 小端写入。
fn put_u64(out: &mut [u8], off: usize, v: u64) {
    let mut i: usize = 0;
    while i < 8 {
        out[off + i] = ((v >> (8 * i)) & 0xFF) as u8;
        i += 1;
    }
}

/// u16 小端读取（调用方保证 b.len() >= off+2）。
fn get_u16(b: &[u8], off: usize) -> u16 {
    (b[off] as u16) | ((b[off + 1] as u16) << 8)
}

/// u32 小端读取。
fn get_u32(b: &[u8], off: usize) -> u32 {
    (b[off] as u32)
        | ((b[off + 1] as u32) << 8)
        | ((b[off + 2] as u32) << 16)
        | ((b[off + 3] as u32) << 24)
}

/// u64 小端读取。
fn get_u64(b: &[u8], off: usize) -> u64 {
    (b[off] as u64)
        | ((b[off + 1] as u64) << 8)
        | ((b[off + 2] as u64) << 16)
        | ((b[off + 3] as u64) << 24)
        | ((b[off + 4] as u64) << 32)
        | ((b[off + 5] as u64) << 40)
        | ((b[off + 6] as u64) << 48)
        | ((b[off + 7] as u64) << 56)
}

// ═══════════════════════════════════════════════════════════════════
// §4 12 域闭集 · HandoffDomain 与 const 域表
// ═══════════════════════════════════════════════════════════════════

/// 域数：12（A1 早传 12 域交接包体系，源：UNX-F0621 聚合 12/12）。
pub const DOMAIN_COUNT: usize = 12;
/// 首域号（1 起，0 保留为空槽哨兵）。
pub const DOMAIN_FIRST: u16 = 1;
/// 末域号（12 止，闭集）。
pub const DOMAIN_LAST: u16 = 12;

/// const 域表：[(域号, 域名, 源锚点)]，12 行 1:1 认领 UNX-F0621 聚合登记的
/// 12 锚点（F0349/F0378/F0413/F0439/F0453/F0476/F0497/F0518/F0538/F0559/
/// F0593/F0617，认领映射见各行注释）。
pub const DOMAIN_TABLE: [(u16, &'static str, &'static str); DOMAIN_COUNT] = [
    (1, "计时", "F0413"), // 计时域 → F0413（UNX-F0624：计时 F0413）
    (2, "ACPI", "F0453"), // ACPI 域 → F0453（F0621 聚合锚点认领）
    (3, "电源", "F0349"), // 电源域 → F0349（F0621 聚合锚点认领）
    (4, "内存", "F0378"), // 内存域 → F0378（UNX-F0625：内存 F0378）
    (5, "中断", "F0439"), // 中断域 → F0439（UNX-F0625：中断 F0439）
    (6, "PCI", "F0476"),  // PCI 域 → F0476（UNX-F0625：PCI F0476）
    (7, "显示", "F0497"), // 显示域 → F0497（UNX-F0626：显示 F0497）
    (8, "输入", "F0518"), // 输入域 → F0518（UNX-F0626：输入 F0518）
    (9, "文件卷", "F0538"), // 文件卷域 → F0538（F0621 聚合锚点认领）
    (10, "安全", "F0593"), // 安全域 → F0593（F0621 聚合锚点认领）
    (11, "日志", "F0559"), // 日志域 → F0559（UNX-F0626：日志 F0559）
    (12, "收口", "F0617"), // 收口域 → F0617（F0621 聚合锚点认领，initrd/参数收口族）
];

/// 域号闭集判定（const）：1..=12，0 为空槽哨兵不算合法域。
pub const fn domain_no_valid(no: u16) -> bool {
    no >= DOMAIN_FIRST && no <= DOMAIN_LAST
}

/// 12 域闭集枚举。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffDomain {
    /// 域 1 · 计时
    Timing,
    /// 域 2 · ACPI
    Acpi,
    /// 域 3 · 电源
    Power,
    /// 域 4 · 内存
    Memory,
    /// 域 5 · 中断
    Interrupt,
    /// 域 6 · PCI
    Pci,
    /// 域 7 · 显示
    Display,
    /// 域 8 · 输入
    Input,
    /// 域 9 · 文件卷
    FsVolume,
    /// 域 10 · 安全
    Security,
    /// 域 11 · 日志
    Log,
    /// 域 12 · 收口
    Finalize,
}

impl HandoffDomain {
    /// 闭集域数。
    pub const COUNT: usize = DOMAIN_COUNT;

    /// 域号（const）。
    pub const fn no(self) -> u16 {
        match self {
            HandoffDomain::Timing => 1,
            HandoffDomain::Acpi => 2,
            HandoffDomain::Power => 3,
            HandoffDomain::Memory => 4,
            HandoffDomain::Interrupt => 5,
            HandoffDomain::Pci => 6,
            HandoffDomain::Display => 7,
            HandoffDomain::Input => 8,
            HandoffDomain::FsVolume => 9,
            HandoffDomain::Security => 10,
            HandoffDomain::Log => 11,
            HandoffDomain::Finalize => 12,
        }
    }

    /// 域名（const，与 DOMAIN_TABLE 一致）。
    pub const fn name(self) -> &'static str {
        match self {
            HandoffDomain::Timing => "计时",
            HandoffDomain::Acpi => "ACPI",
            HandoffDomain::Power => "电源",
            HandoffDomain::Memory => "内存",
            HandoffDomain::Interrupt => "中断",
            HandoffDomain::Pci => "PCI",
            HandoffDomain::Display => "显示",
            HandoffDomain::Input => "输入",
            HandoffDomain::FsVolume => "文件卷",
            HandoffDomain::Security => "安全",
            HandoffDomain::Log => "日志",
            HandoffDomain::Finalize => "收口",
        }
    }

    /// 源锚点标签（const，与 DOMAIN_TABLE 一致）。
    pub const fn anchor(self) -> &'static str {
        match self {
            HandoffDomain::Timing => "F0413",
            HandoffDomain::Acpi => "F0453",
            HandoffDomain::Power => "F0349",
            HandoffDomain::Memory => "F0378",
            HandoffDomain::Interrupt => "F0439",
            HandoffDomain::Pci => "F0476",
            HandoffDomain::Display => "F0497",
            HandoffDomain::Input => "F0518",
            HandoffDomain::FsVolume => "F0538",
            HandoffDomain::Security => "F0593",
            HandoffDomain::Log => "F0559",
            HandoffDomain::Finalize => "F0617",
        }
    }

    /// 域号 → 枚举（None = 越界，非 const 期使用）。
    pub fn from_no(no: u16) -> Option<HandoffDomain> {
        match no {
            1 => Some(HandoffDomain::Timing),
            2 => Some(HandoffDomain::Acpi),
            3 => Some(HandoffDomain::Power),
            4 => Some(HandoffDomain::Memory),
            5 => Some(HandoffDomain::Interrupt),
            6 => Some(HandoffDomain::Pci),
            7 => Some(HandoffDomain::Display),
            8 => Some(HandoffDomain::Input),
            9 => Some(HandoffDomain::FsVolume),
            10 => Some(HandoffDomain::Security),
            11 => Some(HandoffDomain::Log),
            12 => Some(HandoffDomain::Finalize),
            _ => None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// §5 包号编码 · O(1) 直解码
// ═══════════════════════════════════════════════════════════════════

/// 包号基（源：任务号 UNX-F0622 → 包号空间 0x620；包号 = 基 + 域号，
/// 域号 1..=12 → 包号 0x621..0x62C，检索按减法 O(1) 直解码行槽）。
pub const PKG_NO_BASE: u16 = 0x620;
/// 首包号（域 1）。
pub const PKG_NO_FIRST: u16 = PKG_NO_BASE + DOMAIN_FIRST; // 0x621
/// 末包号（域 12）。
pub const PKG_NO_LAST: u16 = PKG_NO_BASE + DOMAIN_LAST; // 0x62C

/// 包号 → 域号（const，O(1)）：越界返回 0（不合法）。
pub const fn pkg_no_to_domain(pkg_no: u16) -> u16 {
    pkg_no.wrapping_sub(PKG_NO_BASE)
}

/// 域号 → 包号（const，O(1)）。
pub const fn pkg_no_of(domain_no: u16) -> u16 {
    PKG_NO_BASE.wrapping_add(domain_no)
}

/// 包号合法判定（const）：解码域号落 1..=12 闭集。
pub const fn pkg_no_valid(pkg_no: u16) -> bool {
    domain_no_valid(pkg_no_to_domain(pkg_no))
}

// ═══════════════════════════════════════════════════════════════════
// §6 版本与对齐纪律
// ═══════════════════════════════════════════════════════════════════

/// 版本 v1 编码（源：UNX-F0623 判据——12 包版本全 v1 对齐；编码 高 8 位主版本/
/// 低 8 位次版本）。
pub const VERSION_V1: u16 = 0x0100;

/// 主版本/次版本 → 版本字段（饱和折叠，防移位溢出）。
pub const fn pkg_version(major: u16, minor: u16) -> u16 {
    ((major & 0x00FF) << 8) | (minor & 0x00FF)
}

/// 版本 → 主版本（const）。
pub const fn version_major(v: u16) -> u16 {
    v >> 8
}

/// 版本 → 次版本（const）。
pub const fn version_minor(v: u16) -> u16 {
    v & 0x00FF
}

/// 版本合法性（const）：非零且主版本 ≥ 1。
pub const fn version_valid(v: u16) -> bool {
    v != 0 && version_major(v) >= 1
}

/// 交接包基址对齐：4KiB 页（源：A1 早传交接包基址按页对齐纪律，与内核早传
/// 页粒度一致）。
pub const PKG_BASE_ALIGN: u64 = 0x1000;

/// 包基址对齐判定（const）。
pub const fn is_pkg_base_aligned(base: u64) -> bool {
    base & (PKG_BASE_ALIGN - 1) == 0
}

/// 单交接包长度上限：16 MiB（源：A1 早传内存预算单包上限，防越界长包混入索引）。
pub const PKG_LEN_CAP: u64 = 0x0100_0000;

// ═══════════════════════════════════════════════════════════════════
// §7 错误域 · PkgIdxError（每变体带中文可操作诊断 + 诊断码）
// ═══════════════════════════════════════════════════════════════════

/// 索引表域错误全集。所有可失败路径唯一出口。
#[derive(Clone, Debug)]
pub enum PkgIdxError {
    /// 域号越界（非 1..=12 闭集）。
    DomainOutOfRange(u16),
    /// 重复域号（槽位已登记）。
    DuplicateDomain(u16),
    /// 包基址未 4KiB 页对齐（含零基址）。
    BaseUnaligned(u64),
    /// 包长度为零。
    ZeroLength(u16),
    /// 长度超上限或 基址+长度 溢出（checked_add 显性拒绝）。
    LenOverflow { base: u64, len: u64 },
    /// 版本非法（零版本或主版本为零）。
    VersionInvalid(u16),
    /// 表未账齐（登记行数 < 12 即冻结/核表被拒）。
    TableIncomplete { filled: usize, need: usize },
    /// 表已冻结后拒改（登记通道关闭）。
    TableFrozen(u16),
    /// 包号越界（减基解码不落 1..=12）。
    BadPkgNo(u16),
    /// 未登记域/空槽检索——显性 Miss，绝不返回零基址谎报。
    UnregisteredDomain(u16),
    /// 行校验和不符（存储值 vs 重算值）。
    RowChecksumMismatch { slot: usize, stored: u16, computed: u16 },
    /// 表级校验和不符。
    TableChecksumMismatch { stored: u32, computed: u32 },
    /// 快照结构损坏（魔数/版本/行数/长度/域序等，附定位短语）。
    SnapshotCorrupt(&'static str),
}

impl PkgIdxError {
    /// 中文可操作诊断（每变体独占一段，含下一步动作）。
    pub fn detail(&self) -> &'static str {
        match self {
            PkgIdxError::DomainOutOfRange(d) => {
                if *d == 0 {
                    "域号为 0：0 是空槽哨兵不是合法域，请改用 DOMAIN_TABLE 中 1..=12 的域号"
                } else {
                    "域号越界：12 域闭集只收 1..=12，请查 HandoffDomain::from_no 映射后再登记"
                }
            }
            PkgIdxError::DuplicateDomain(_) => {
                "重复域号拒收：该域槽位已登记索引行，查重请先 lookup_by_domain；确需覆盖须走勘误批不得静默改写"
            }
            PkgIdxError::BaseUnaligned(b) => {
                if *b == 0 {
                    "包基址为零：零基址是谎报形态，A1 早传包必须有真实物理基址，请回溯 F0621 聚合账"
                } else {
                    "包基址未按 4KiB 页对齐：请回上游核对聚合包装载基址，禁止在索引表侧做对齐修补"
                }
            }
            PkgIdxError::ZeroLength(_) => {
                "包长度为零：空包无交接意义，请核对该域聚合包实际长度后重登记"
            }
            PkgIdxError::LenOverflow { .. } => {
                "长度超 16MiB 单包上限或 基址+长度 越顶：请拆包或核基址，索引表不做截断兜底"
            }
            PkgIdxError::VersionInvalid(_) => {
                "版本非法：须非零且主版本≥1（v1=0x0100），请用 pkg_version(1,0) 构造"
            }
            PkgIdxError::TableIncomplete { .. } => {
                "表未账齐：12 行必须全部登记后方可冻结/核表，请补齐缺失域（用 missing_domains 定位）"
            }
            PkgIdxError::TableFrozen(_) => {
                "表已冻结：登记通道已关，改动请走勘误批新表，不提供热改"
            }
            PkgIdxError::BadPkgNo(p) => {
                if pkg_no_to_domain(*p) == 0 && *p == PKG_NO_BASE {
                    "包号等于基 0x620：域号 0 不存在，包号空间为 0x621..0x62C"
                } else {
                    "包号越界：合法包号 = 0x620 + 域号(1..=12)，越界检索按显性 Miss 拒答"
                }
            }
            PkgIdxError::UnregisteredDomain(_) => {
                "未登记域检索 Miss：该域槽位为空，索引表拒绝返回零基址谎报；请先核对该域 F0621 登记状态"
            }
            PkgIdxError::RowChecksumMismatch { .. } => {
                "行校验和不符：存储校验和与四字段（域号/基址/长度/版本）FNV16 重算值不一致，行数据疑似被篡改或写坏"
            }
            PkgIdxError::TableChecksumMismatch { .. } => {
                "表级校验和不符：12 行记录流 FNV32 与冻结值不一致，表整体疑似被篡改，请重导快照"
            }
            PkgIdxError::SnapshotCorrupt(_) => {
                "快照结构损坏：魔数/格式版本/行数/长度/域序之一不合法（附定位短语见 Debug），请检查导出链路完整性"
            }
        }
    }

    /// 诊断码（0x14D0~0x14DC 错误段；事件码不在此列）。
    pub fn code(&self) -> u16 {
        match self {
            PkgIdxError::DomainOutOfRange(_) => E_DOMAIN_RANGE,
            PkgIdxError::DuplicateDomain(_) => E_DUP_DOMAIN,
            PkgIdxError::BaseUnaligned(_) => E_BASE_UNALIGNED,
            PkgIdxError::ZeroLength(_) => E_ZERO_LEN,
            PkgIdxError::LenOverflow { .. } => E_LEN_OVERFLOW,
            PkgIdxError::VersionInvalid(_) => E_VERSION_INVALID,
            PkgIdxError::TableIncomplete { .. } => E_TABLE_INCOMPLETE,
            PkgIdxError::TableFrozen(_) => E_TABLE_FROZEN,
            PkgIdxError::BadPkgNo(_) => E_BAD_PKG_NO,
            PkgIdxError::UnregisteredDomain(_) => E_UNREGISTERED,
            PkgIdxError::RowChecksumMismatch { .. } => E_ROW_CSUM,
            PkgIdxError::TableChecksumMismatch { .. } => E_TABLE_CSUM,
            PkgIdxError::SnapshotCorrupt(_) => E_SNAPSHOT,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// §8 索引行 · PkgIndexRow（4 数据列 + 行校验和列）
// ═══════════════════════════════════════════════════════════════════

/// 索引行 4 数据列（"12 行×4 列"之 4 列）。
pub const ROW_COLS: usize = 4;
/// 列名表（账面口径：域号/包基址/包长度/版本；行校验和为第 5 列="+1"）。
pub const ROW_COL_NAMES: [&str; ROW_COLS] = ["域号", "包基址", "包长度", "版本"];

/// 索引行裸字段序列化长度：域号 u16(2) + 基址 u64(8) + 长度 u64(8) + 版本 u16(2)。
pub const ROW_RAW_LEN: usize = 2 + 8 + 8 + 2; // 20

/// 空槽哨兵行（域号 0；const 期可复制）。
const EMPTY_ROW: PkgIndexRow = PkgIndexRow {
    domain: 0,
    base: 0,
    len: 0,
    version: 0,
    checksum: 0,
};

/// 一行索引：4 数据列 + 行校验和列。
///
/// 行校验和 = FNV16(域号 LE ‖ 包基址 LE ‖ 包长度 LE ‖ 版本 LE)，即"域号~版本
/// 四字段 FNV16 折叠"真算，非占位值。
#[derive(Clone, Copy, Debug)]
pub struct PkgIndexRow {
    /// 列 0 · 域号（1..=12；0 仅为空槽哨兵）。
    pub domain: u16,
    /// 列 1 · 包基址（4KiB 页对齐物理地址）。
    pub base: u64,
    /// 列 2 · 包长度（字节，≤ 16MiB）。
    pub len: u64,
    /// 列 3 · 版本（v1 = 0x0100）。
    pub version: u16,
    /// 列 4（+1）· 行校验和（FNV16，域号~版本四字段折叠）。
    pub checksum: u16,
}

impl PkgIndexRow {
    /// 构造并真算行校验和（唯一合法入口；校验和不可手填）。
    pub fn new(domain: u16, base: u64, len: u64, version: u16) -> PkgIndexRow {
        let mut raw = [0u8; ROW_RAW_LEN];
        Self::write_raw_into(&mut raw, domain, base, len, version);
        PkgIndexRow {
            domain,
            base,
            len,
            version,
            checksum: fnv16(&raw),
        }
    }

    /// 四字段小端序列化（内部；布局即行裸字节序）。
    fn write_raw_into(out: &mut [u8; ROW_RAW_LEN], domain: u16, base: u64, len: u64, version: u16) {
        put_u16(out, 0, domain);
        put_u64(out, 2, base);
        put_u64(out, 10, len);
        put_u16(out, 18, version);
    }

    /// 本行四字段裸字节（20 字节）。
    pub fn raw_bytes(&self) -> [u8; ROW_RAW_LEN] {
        let mut raw = [0u8; ROW_RAW_LEN];
        Self::write_raw_into(&mut raw, self.domain, self.base, self.len, self.version);
        raw
    }

    /// 行校验和重算（独立于存储值，供逐行核验）。
    pub fn compute_checksum(&self) -> u16 {
        fnv16(&self.raw_bytes())
    }

    /// 行校验和核验：存储值 == 重算值。
    pub fn checksum_ok(&self) -> bool {
        self.checksum == self.compute_checksum()
    }

    /// 是否空槽哨兵（域号 0）。
    pub fn is_empty(&self) -> bool {
        self.domain == 0
    }

    /// 列 0 · 域号（含空槽校验：空槽返回 Err）。
    pub fn col_domain(&self) -> Result<u16, PkgIdxError> {
        if self.is_empty() {
            Err(PkgIdxError::UnregisteredDomain(0))
        } else {
            Ok(self.domain)
        }
    }

    /// 列 1 · 包基址。
    pub fn col_base(&self) -> u64 {
        self.base
    }

    /// 列 2 · 包长度。
    pub fn col_len(&self) -> u64 {
        self.len
    }

    /// 列 3 · 版本。
    pub fn col_version(&self) -> u16 {
        self.version
    }

    /// 列 4 · 行校验和（存储值）。
    pub fn col_checksum(&self) -> u16 {
        self.checksum
    }

    /// 行记录 22 字节（裸 20 + 校验和 LE 2）——快照记录单元。
    pub fn record_bytes(&self) -> [u8; ROW_RAW_LEN + 2] {
        let mut rec = [0u8; ROW_RAW_LEN + 2];
        let raw = self.raw_bytes();
        let mut i: usize = 0;
        while i < ROW_RAW_LEN {
            rec[i] = raw[i];
            i += 1;
        }
        put_u16(&mut rec, ROW_RAW_LEN, self.checksum);
        rec
    }
}

// ═══════════════════════════════════════════════════════════════════
// §9 索引表 · PkgIndexTable（12 行稠密槽位）
// ═══════════════════════════════════════════════════════════════════

/// 一行索引 12 行表：槽位稠密编址——槽 i 恒属域号 i+1，空槽为哨兵行。
/// 这使"按包号 O(1)"成立：pkg_no - 0x620 即槽位号。
#[derive(Clone, Copy, Debug)]
pub struct PkgIndexTable {
    /// 12 行槽位（下标 i ↔ 域号 i+1）。
    rows: [PkgIndexRow; DOMAIN_COUNT],
    /// 已登记行数（账齐门槛：== 12 方可冻结）。
    filled: usize,
    /// 冻结态：冻结后拒改、表级校验和生效。
    frozen: bool,
    /// 表级校验和（冻结时定格；FNV32 over 12 行 22 字节记录流）。
    table_checksum: u32,
}

/// 检索命中（显性命中载体；Miss 走 Err，绝不返回零基址命中）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LookupHit {
    /// 域号（1..=12）。
    pub domain: u16,
    /// 包号（0x620 + 域号）。
    pub pkg_no: u16,
    /// 行槽位（域号-1）。
    pub slot: usize,
    /// 列 1 · 包基址。
    pub base: u64,
    /// 列 2 · 包长度。
    pub len: u64,
    /// 列 3 · 版本。
    pub version: u16,
    /// 列 4 · 行校验和（存储值，命中时已核验一致）。
    pub row_checksum: u16,
}

impl PkgIndexTable {
    /// 空表（const 期可构造）。
    pub const fn new() -> PkgIndexTable {
        PkgIndexTable {
            rows: [EMPTY_ROW; DOMAIN_COUNT],
            filled: 0,
            frozen: false,
            table_checksum: 0,
        }
    }

    /// 已登记行数。
    pub fn filled_count(&self) -> usize {
        self.filled
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 表级校验和（冻结值）。
    pub fn table_checksum(&self) -> u32 {
        self.table_checksum
    }

    /// 缺登记域号清单（至多 12 个；顺序域号升序）。
    pub fn missing_domains(&self) -> Vec<u16> {
        let mut out = Vec::new();
        let mut d: u16 = DOMAIN_FIRST;
        while d <= DOMAIN_LAST {
            if self.rows[(d - 1) as usize].is_empty() {
                out.push(d);
            }
            d += 1;
        }
        out
    }

    /// 只读访问第 slot 槽（越界返回 None）。
    pub fn row(&self, slot: usize) -> Option<PkgIndexRow> {
        if slot < DOMAIN_COUNT {
            Some(self.rows[slot])
        } else {
            None
        }
    }

    /// 12 行槽位快照（副本，账面齐查用）。
    pub fn rows_snapshot(&self) -> [PkgIndexRow; DOMAIN_COUNT] {
        self.rows
    }

    /// 登记一行（校验链：域号闭集 → 槽占用查重 → 冻结门 → 基址对齐/非零 →
    /// 长度非零/上限/溢出 → 版本合法）。
    ///
    /// 成功返回包号（0x620 + 域号）。
    pub fn register(&mut self, domain: u16, base: u64, len: u64, version: u16) -> Result<u16, PkgIdxError> {
        // 1) 域号闭集
        if !domain_no_valid(domain) {
            return Err(PkgIdxError::DomainOutOfRange(domain));
        }
        let slot = (domain - 1) as usize;
        // 2) 重复域号拒收（查重先于冻结门：冻结表上的重复注入也要精准命中 E_DUP_DOMAIN）
        if !self.rows[slot].is_empty() {
            return Err(PkgIdxError::DuplicateDomain(domain));
        }
        // 3) 冻结门：冻结后登记通道关闭
        if self.frozen {
            return Err(PkgIdxError::TableFrozen(domain));
        }
        // 4) 基址：非零 + 4KiB 页对齐
        if base == 0 || !is_pkg_base_aligned(base) {
            return Err(PkgIdxError::BaseUnaligned(base));
        }
        // 5) 长度：非零 + 上限 + 基址+长度 溢出（checked 显性拒绝）
        if len == 0 {
            return Err(PkgIdxError::ZeroLength(domain));
        }
        if len > PKG_LEN_CAP {
            return Err(PkgIdxError::LenOverflow { base, len });
        }
        if base.checked_add(len).is_none() {
            return Err(PkgIdxError::LenOverflow { base, len });
        }
        // 6) 版本合法（非零、主版本 ≥ 1）
        if !version_valid(version) {
            return Err(PkgIdxError::VersionInvalid(version));
        }
        // 全链通过：稠密槽位落行，行校验和真算
        self.rows[slot] = PkgIndexRow::new(domain, base, len, version);
        self.filled += 1;
        Ok(pkg_no_of(domain))
    }

    /// 冻结：12 行账齐门 → 计算并定格表级校验和。
    /// 返回表级校验和。账不齐拒绝冻结（J1-a "账齐"硬门槛）。
    pub fn freeze(&mut self) -> Result<u32, PkgIdxError> {
        if self.filled != DOMAIN_COUNT {
            return Err(PkgIdxError::TableIncomplete {
                filled: self.filled,
                need: DOMAIN_COUNT,
            });
        }
        if self.frozen {
            return Err(PkgIdxError::TableFrozen(0));
        }
        let csum = self.compute_table_checksum();
        self.table_checksum = csum;
        self.frozen = true;
        Ok(csum)
    }

    /// 12 行 22 字节记录流（槽序，空槽为全零记录——确定性来源）。
    fn record_stream(&self) -> [u8; DOMAIN_COUNT * (ROW_RAW_LEN + 2)] {
        let mut stream = [0u8; DOMAIN_COUNT * (ROW_RAW_LEN + 2)];
        let mut s: usize = 0;
        let mut i: usize = 0;
        while i < DOMAIN_COUNT {
            let rec = self.rows[i].record_bytes();
            let mut j: usize = 0;
            while j < rec.len() {
                stream[s] = rec[j];
                s += 1;
                j += 1;
            }
            i += 1;
        }
        stream
    }

    /// 表级校验和现算：FNV32 over 12 行记录流（22×12 = 264 字节）。
    pub fn compute_table_checksum(&self) -> u32 {
        fnv32(&self.record_stream())
    }

    /// 表级校验和核验：须先冻结；存储值 == 现算值。
    pub fn verify_table_checksum(&self) -> Result<(), PkgIdxError> {
        if !self.frozen {
            return Err(PkgIdxError::TableIncomplete {
                filled: self.filled,
                need: DOMAIN_COUNT,
            });
        }
        let computed = self.compute_table_checksum();
        if computed != self.table_checksum {
            return Err(PkgIdxError::TableChecksumMismatch {
                stored: self.table_checksum,
                computed,
            });
        }
        Ok(())
    }

    /// 行校验和逐行核验（J1-a "+行校验和"硬门槛）：对每个已登记行重算
    /// FNV16 并与存储值比对；空槽跳过。
    pub fn verify_rows(&self) -> Result<(), PkgIdxError> {
        let mut i: usize = 0;
        while i < DOMAIN_COUNT {
            let r = self.rows[i];
            if !r.is_empty() {
                let computed = r.compute_checksum();
                if computed != r.checksum {
                    return Err(PkgIdxError::RowChecksumMismatch {
                        slot: i,
                        stored: r.checksum,
                        computed,
                    });
                }
            }
            i += 1;
        }
        Ok(())
    }

    // ── 检索双路 ────────────────────────────────────────────────

    /// 按包号检索（O(1)）：pkg_no - 0x620 直解码槽位。
    /// 包号越界 → `BadPkgNo`；槽空 → `UnregisteredDomain`（显性 Miss）。
    pub fn lookup_by_pkg(&self, pkg_no: u16) -> Result<LookupHit, PkgIdxError> {
        if !pkg_no_valid(pkg_no) {
            return Err(PkgIdxError::BadPkgNo(pkg_no));
        }
        let slot = (pkg_no_to_domain(pkg_no) - 1) as usize;
        self.hit_from_slot(slot, pkg_no)
    }

    /// 按域检索（O(12) 线性扫行，刻意不走直解码——与 O(1) 路互为对拍）。
    /// 域号越界 → `DomainOutOfRange`；扫完全表无命中 → `UnregisteredDomain`。
    pub fn lookup_by_domain(&self, domain: u16) -> Result<LookupHit, PkgIdxError> {
        if !domain_no_valid(domain) {
            return Err(PkgIdxError::DomainOutOfRange(domain));
        }
        let mut i: usize = 0;
        while i < DOMAIN_COUNT {
            let r = self.rows[i];
            if !r.is_empty() && r.domain == domain {
                return self.hit_from_slot(i, pkg_no_of(domain));
            }
            i += 1;
        }
        Err(PkgIdxError::UnregisteredDomain(domain))
    }

    /// 槽位 → 命中载体（空槽显性 Miss）。
    fn hit_from_slot(&self, slot: usize, pkg_no: u16) -> Result<LookupHit, PkgIdxError> {
        let r = self.rows[slot];
        if r.is_empty() {
            return Err(PkgIdxError::UnregisteredDomain((slot + 1) as u16));
        }
        Ok(LookupHit {
            domain: r.domain,
            pkg_no,
            slot,
            base: r.base,
            len: r.len,
            version: r.version,
            row_checksum: r.checksum,
        })
    }

    // ── 注入与破坏（仅注入面使用；不在正常构造路径） ────────────

    /// 破坏指定槽的行校验和最低位（注入面专用；须先解冻判断由调用方负责）。
    /// 越界槽返回 DomainOutOfRange。
    pub(crate) fn corrupt_row_checksum(&mut self, slot: usize) -> Result<(), PkgIdxError> {
        if slot >= DOMAIN_COUNT {
            return Err(PkgIdxError::DomainOutOfRange((slot + 1) as u16));
        }
        self.rows[slot].checksum ^= 0x0001;
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════
// §10 检索用例 · const 用例表 + 运行时逐例核（J1-b 10/10）
// ═══════════════════════════════════════════════════════════════════

/// 检索路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LookupPath {
    /// 按包号（O(1) 直解码）。
    ByPkg(u16),
    /// 按域号（O(12) 线性扫行）。
    ByDomain(u16),
}

/// 检索用例：路径 + 期望（命中→期望域号；Miss→期望诊断码）。
#[derive(Clone, Copy, Debug)]
pub struct LookupCase {
    /// 检索路径。
    pub path: LookupPath,
    /// 期望命中（true=Hit，false=显性 Miss）。
    pub expect_hit: bool,
    /// 期望域号（命中时比对）。
    pub expect_domain: u16,
    /// 期望诊断码（Miss 时比对）。
    pub expect_code: u16,
}

/// 用例总数：10（J1-b "10/10 用例"）。
pub const LOOKUP_CASES_TOTAL: usize = 10;

/// const 用例表：8 命中（覆盖 计时/电源/内存/中断/PCI/输入/安全/收口 8 域）
/// + 2 显性 Miss（包号越界 + 域号越界）。
pub const LOOKUP_CASES: [LookupCase; LOOKUP_CASES_TOTAL] = [
    LookupCase { path: LookupPath::ByPkg(pkg_no_of(1)), expect_hit: true, expect_domain: 1, expect_code: 0 }, // 计时 0x621
    LookupCase { path: LookupPath::ByPkg(pkg_no_of(3)), expect_hit: true, expect_domain: 3, expect_code: 0 }, // 电源 0x623
    LookupCase { path: LookupPath::ByPkg(pkg_no_of(4)), expect_hit: true, expect_domain: 4, expect_code: 0 }, // 内存 0x624
    LookupCase { path: LookupPath::ByPkg(pkg_no_of(5)), expect_hit: true, expect_domain: 5, expect_code: 0 }, // 中断 0x625
    LookupCase { path: LookupPath::ByPkg(pkg_no_of(6)), expect_hit: true, expect_domain: 6, expect_code: 0 }, // PCI 0x626
    LookupCase { path: LookupPath::ByPkg(pkg_no_of(8)), expect_hit: true, expect_domain: 8, expect_code: 0 }, // 输入 0x628
    LookupCase { path: LookupPath::ByPkg(pkg_no_of(10)), expect_hit: true, expect_domain: 10, expect_code: 0 }, // 安全 0x62A
    LookupCase { path: LookupPath::ByPkg(pkg_no_of(12)), expect_hit: true, expect_domain: 12, expect_code: 0 }, // 收口 0x62C
    LookupCase { path: LookupPath::ByPkg(0x630), expect_hit: false, expect_domain: 0, expect_code: E_BAD_PKG_NO }, // 越界包号 → 0x14D8
    LookupCase { path: LookupPath::ByDomain(13), expect_hit: false, expect_domain: 0, expect_code: E_DOMAIN_RANGE }, // 越界域号 → 0x14D0
];

/// 单用例核验：命中比对域号，Miss 比对诊断码。
pub fn lookup_case_check(t: &PkgIndexTable, c: &LookupCase) -> bool {
    match c.path {
        LookupPath::ByPkg(p) => match t.lookup_by_pkg(p) {
            Ok(hit) => c.expect_hit && hit.domain == c.expect_domain,
            Err(e) => !c.expect_hit && e.code() == c.expect_code,
        },
        LookupPath::ByDomain(d) => match t.lookup_by_domain(d) {
            Ok(hit) => c.expect_hit && hit.domain == c.expect_domain,
            Err(e) => !c.expect_hit && e.code() == c.expect_code,
        },
    }
}

/// 运行时逐例核：返回 (通过数, 总数 10)。J1-b 达标线：通过数 == 10。
pub fn run_lookup_cases(t: &PkgIndexTable) -> (usize, usize) {
    let mut passed: usize = 0;
    let mut i: usize = 0;
    while i < LOOKUP_CASES.len() {
        if lookup_case_check(t, &LOOKUP_CASES[i]) {
            passed += 1;
        }
        i += 1;
    }
    (passed, LOOKUP_CASES_TOTAL)
}

// ═══════════════════════════════════════════════════════════════════
// §11 快照导出 · 确定性序列化 + 往返 + FNV 指纹
// ═══════════════════════════════════════════════════════════════════

/// 快照魔数 "VPXI"（Varix Package IndeX）。
pub const SNAP_MAGIC: [u8; 4] = [b'V', b'P', b'X', b'I'];
/// 快照格式版本（源：本账首版格式，自 1 起单调）。
pub const SNAP_FMT: u16 = 1;
/// 快照头长：魔数 4 + 格式 2 + 行数 2 + 标志 2 + 表校验和 4。
pub const SNAP_HDR_LEN: usize = 4 + 2 + 2 + 2 + 4; // 14
/// 快照记录长：行裸 20 + 校验和 2。
pub const SNAP_REC_LEN: usize = ROW_RAW_LEN + 2; // 22
/// 头标志位：bit0 = 表已冻结。
pub const SNAP_FLAG_FROZEN: u16 = 0x0001;

/// 快照产物：确定性字节流 + FNV1a64 指纹 + 行数。
/// （含 Vec 字段 → 不 derive Copy；体量 ≈ 300 字节，不上 64KiB 红线。）
#[derive(Clone, Debug)]
pub struct PkgSnapshot {
    /// 确定性序列化字节流（同表必同字节）。
    pub bytes: Vec<u8>,
    /// FNV1a64(字节流) 指纹。
    pub fingerprint: u64,
    /// 导出行数（0..=12）。
    pub row_count: usize,
}

impl PkgIndexTable {
    /// 快照导出：只导出已登记行（槽序 = 域号升序），头带冻结标志与表级
    /// 校验和（冻结值；未冻结表导出现算值作参考）。
    /// 确定性：同表状态 → 同字节流 → 同指纹（双跑对拍依据）。
    pub fn snapshot(&self) -> PkgSnapshot {
        let count = self.filled;
        let mut bytes = Vec::with_capacity(SNAP_HDR_LEN + count * SNAP_REC_LEN);
        let mut hdr = [0u8; SNAP_HDR_LEN];
        let mut i: usize = 0;
        while i < 4 {
            hdr[i] = SNAP_MAGIC[i];
            i += 1;
        }
        put_u16(&mut hdr, 4, SNAP_FMT);
        put_u16(&mut hdr, 6, count as u16);
        put_u16(&mut hdr, 8, if self.frozen { SNAP_FLAG_FROZEN } else { 0 });
        put_u32(&mut hdr, 10, if self.frozen { self.table_checksum } else { self.compute_table_checksum() });
        let mut k: usize = 0;
        while k < SNAP_HDR_LEN {
            bytes.push(hdr[k]);
            k += 1;
        }
        // 槽序导出（跳过空槽；稠密槽位天然域号升序）
        let mut s: usize = 0;
        while s < DOMAIN_COUNT {
            let r = self.rows[s];
            if !r.is_empty() {
                let rec = r.record_bytes();
                let mut j: usize = 0;
                while j < rec.len() {
                    bytes.push(rec[j]);
                    j += 1;
                }
            }
            s += 1;
        }
        PkgSnapshot {
            fingerprint: fnv1a64(&bytes),
            row_count: count,
            bytes,
        }
    }

    /// 表指纹（快捷）：FNV1a64 over 快照字节流。
    pub fn fingerprint(&self) -> u64 {
        self.snapshot().fingerprint
    }

    /// 快照往返重建：结构核（魔数/版本/行数/长度/域序）→ 行校验和逐行核 →
    /// 表级校验和核（冻结态必核）。任何一步不过即拒收，绝不"带病"重建。
    pub fn from_bytes(bytes: &[u8]) -> Result<PkgIndexTable, PkgIdxError> {
        // 结构核：头完整
        if bytes.len() < SNAP_HDR_LEN {
            return Err(PkgIdxError::SnapshotCorrupt("头截断：不足 14 字节"));
        }
        // 魔数
        if bytes[0] != SNAP_MAGIC[0] || bytes[1] != SNAP_MAGIC[1]
            || bytes[2] != SNAP_MAGIC[2] || bytes[3] != SNAP_MAGIC[3] {
            return Err(PkgIdxError::SnapshotCorrupt("魔数非 VPXI"));
        }
        // 格式版本
        let fmt = get_u16(bytes, 4);
        if fmt != SNAP_FMT {
            return Err(PkgIdxError::SnapshotCorrupt("格式版本非 1"));
        }
        // 行数
        let count = get_u16(bytes, 6) as usize;
        if count > DOMAIN_COUNT {
            return Err(PkgIdxError::SnapshotCorrupt("行数超 12 上限"));
        }
        // 标志
        let flags = get_u16(bytes, 8);
        let claim_frozen = (flags & SNAP_FLAG_FROZEN) == SNAP_FLAG_FROZEN;
        if claim_frozen && count != DOMAIN_COUNT {
            return Err(PkgIdxError::SnapshotCorrupt("冻结标志与行数矛盾：冻结须 12 行"));
        }
        // 总长一致
        let expect_len = SNAP_HDR_LEN + count * SNAP_REC_LEN;
        if bytes.len() != expect_len {
            return Err(PkgIdxError::SnapshotCorrupt("总长与行数不符"));
        }
        let stored_tsum = get_u32(bytes, 10);
        // 逐记录重建（域号严格递增核 + 闭集核）
        let mut t = PkgIndexTable::new();
        let mut prev_domain: u16 = 0;
        let mut i: usize = 0;
        while i < count {
            let off = SNAP_HDR_LEN + i * SNAP_REC_LEN;
            let domain = get_u16(bytes, off);
            if !domain_no_valid(domain) {
                return Err(PkgIdxError::SnapshotCorrupt("记录域号越界"));
            }
            if i > 0 && domain <= prev_domain {
                return Err(PkgIdxError::SnapshotCorrupt("域号非严格递增"));
            }
            let base = get_u64(bytes, off + 2);
            let len = get_u64(bytes, off + 10);
            let version = get_u16(bytes, off + 18);
            let csum = get_u16(bytes, off + 20);
            let slot = (domain - 1) as usize;
            t.rows[slot] = PkgIndexRow {
                domain,
                base,
                len,
                version,
                checksum: csum,
            };
            t.filled += 1;
            prev_domain = domain;
            i += 1;
        }
        // 行校验和逐行核（坏行注入在快照链路上必须被拦）
        t.verify_rows()?;
        // 冻结态：表级校验和必核
        if claim_frozen {
            let computed = t.compute_table_checksum();
            if computed != stored_tsum {
                return Err(PkgIdxError::TableChecksumMismatch {
                    stored: stored_tsum,
                    computed,
                });
            }
            t.table_checksum = stored_tsum;
            t.frozen = true;
        }
        Ok(t)
    }
}

// ═══════════════════════════════════════════════════════════════════
// §12 事件账 · EventLedger（环形 64 槽）
// ═══════════════════════════════════════════════════════════════════

/// 事件账容量：64（源：单表全生命周期事件量级 ~12 登记 + 检索自证 + 注入，
/// 64 槽足够且不超栈预算；环形覆盖最旧，丢弃数显性入账）。
pub const LEDGER_CAP: usize = 64;

/// 一条事件：全序号 + 诊断码 + 域号 + 辅参。
#[derive(Clone, Copy, Debug)]
pub struct PkgIdxEvent {
    /// 全序号（wrapping 单调，跨覆盖仍单调可比）。
    pub seq: u16,
    /// 诊断码（0x14DD~0x14DF 事件码，或注入检出的错误码）。
    pub code: u16,
    /// 关联域号（0 = 无域关联，如纯包号事件）。
    pub domain: u16,
    /// 辅参（包号/校验和/槽位等，按码释义）。
    pub aux: u64,
}

impl PkgIdxEvent {
    const ZERO: PkgIdxEvent = PkgIdxEvent {
        seq: 0,
        code: 0,
        domain: 0,
        aux: 0,
    };
}

/// 环形事件账：64 槽，满后覆盖最旧并显性累加丢弃计数（不静默吞事件）。
#[derive(Clone, Debug)]
pub struct EventLedger {
    buf: [PkgIdxEvent; LEDGER_CAP],
    head: usize,
    len: usize,
    dropped: u32,
    next_seq: u16,
}

impl EventLedger {
    /// 空账（const 期可构造）。
    pub const fn new() -> EventLedger {
        EventLedger {
            buf: [PkgIdxEvent::ZERO; LEDGER_CAP],
            head: 0,
            len: 0,
            dropped: 0,
            next_seq: 0,
        }
    }

    /// 入账一条（满则覆盖最旧，丢弃计数 saturating 显性累加）。返回全序号。
    pub fn push(&mut self, code: u16, domain: u16, aux: u64) -> u16 {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        let ev = PkgIdxEvent { seq, code, domain, aux };
        if self.len < LEDGER_CAP {
            let pos = (self.head + self.len) % LEDGER_CAP;
            self.buf[pos] = ev;
            self.len += 1;
        } else {
            // 满：覆盖最旧（head 前移），丢弃计数显性累加
            self.buf[self.head] = ev;
            self.head = (self.head + 1) % LEDGER_CAP;
            self.dropped = self.dropped.saturating_add(1);
        }
        seq
    }

    /// 当前在账事件数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 累计丢弃数（满后覆盖发生的次数）。
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    /// 按入账序读第 i 条（越界返回 None）。
    pub fn event(&self, i: usize) -> Option<PkgIdxEvent> {
        if i < self.len {
            Some(self.buf[(self.head + i) % LEDGER_CAP])
        } else {
            None
        }
    }

    /// 错误类事件数（码 < EV_REGISTER_OK 即错误段 0x14D0~0x14DC）。
    pub fn fault_count(&self) -> usize {
        let mut n: usize = 0;
        let mut i: usize = 0;
        while i < self.len {
            if let Some(ev) = self.event(i) {
                if ev.code < EV_REGISTER_OK {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 指定诊断码的事件计数。
    pub fn count_code(&self, code: u16) -> usize {
        let mut n: usize = 0;
        let mut i: usize = 0;
        while i < self.len {
            if let Some(ev) = self.event(i) {
                if ev.code == code {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ═══════════════════════════════════════════════════════════════════
// §13 标准表种子 · SEED_DESC（12 域登记描述符）
// ═══════════════════════════════════════════════════════════════════

/// 标准表种子：[(域号, 包基址, 包长度, 版本)] × 12。
/// 基址自 0x6200_0000 起按域递增（页对齐），长度 8KiB~64KiB 交错（覆盖
/// 对齐/上限/溢出边界之外的真实形），版本全 v1（源：UNX-F0623——12 包版本全 v1 对齐）。
pub const SEED_DESC: [(u16, u64, u64, u16); DOMAIN_COUNT] = [
    (1, 0x6200_0000, 0x0001_0000, VERSION_V1), // 计时 64KiB
    (2, 0x6201_0000, 0x0000_4000, VERSION_V1), // ACPI 16KiB
    (3, 0x6202_0000, 0x0000_8000, VERSION_V1), // 电源 32KiB
    (4, 0x6203_0000, 0x0002_0000, VERSION_V1), // 内存 128KiB
    (5, 0x6206_0000, 0x0000_2000, VERSION_V1), // 中断 8KiB
    (6, 0x6207_0000, 0x0001_8000, VERSION_V1), // PCI 96KiB
    (7, 0x6209_0000, 0x0003_0000, VERSION_V1), // 显示 192KiB
    (8, 0x620C_0000, 0x0000_4000, VERSION_V1), // 输入 16KiB
    (9, 0x620D_0000, 0x0002_8000, VERSION_V1), // 文件卷 160KiB
    (10, 0x6210_0000, 0x0000_C000, VERSION_V1), // 安全 48KiB
    (11, 0x6211_0000, 0x0000_8000, VERSION_V1), // 日志 32KiB
    (12, 0x6212_0000, 0x0001_4000, VERSION_V1), // 收口 80KiB
];

/// 由种子构建标准 12 行冻结表（唯一权威构造路径；双跑对拍两侧同用此函数）。
pub fn standard_table() -> Result<PkgIndexTable, PkgIdxError> {
    let mut t = PkgIndexTable::new();
    let mut i: usize = 0;
    while i < SEED_DESC.len() {
        let (d, b, l, v) = SEED_DESC[i];
        t.register(d, b, l, v)?;
        i += 1;
    }
    t.freeze()?;
    Ok(t)
}

// ═══════════════════════════════════════════════════════════════════
// §14 工作站 · PkgIdxStation（表 + 事件账 + 注入面 + 健康）
// ═══════════════════════════════════════════════════════════════════

/// 索引表工作站：持表 + 事件账，所有带账操作（登记/检索/注入）在此收口。
#[derive(Clone, Debug)]
pub struct PkgIdxStation {
    table: PkgIndexTable,
    ledger: EventLedger,
}

/// 双跑确定性对拍报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeterminismReport {
    /// 第一跑指纹。
    pub fingerprint_a: u64,
    /// 第二跑指纹。
    pub fingerprint_b: u64,
    /// 快照字节流全等。
    pub snapshot_equal: bool,
    /// 总判：指纹相等且字节全等。
    pub agree: bool,
}

/// 健康账（health() 产物；green 为总判）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PkgIdxHealth {
    /// 已登记行数（绿线：12）。
    pub rows_filled: usize,
    /// 冻结态（绿线：true）。
    pub frozen: bool,
    /// 行校验和逐行全过（绿线：true）。
    pub row_checksum_ok: bool,
    /// 表级校验和核验通过（绿线：true）。
    pub table_checksum_ok: bool,
    /// 检索用例通过数（绿线：10）。
    pub lookup_cases_passed: usize,
    /// 检索用例总数（恒 10）。
    pub lookup_cases_total: usize,
    /// 在账事件数。
    pub ledger_events: usize,
    /// 累计丢弃事件数（绿线：0）。
    pub ledger_dropped: u32,
    /// 错误类事件数（绿线：0）。
    pub faults: usize,
}

impl PkgIdxHealth {
    /// 总判：全绿。
    pub fn is_green(&self) -> bool {
        self.rows_filled == DOMAIN_COUNT
            && self.frozen
            && self.row_checksum_ok
            && self.table_checksum_ok
            && self.lookup_cases_passed == self.lookup_cases_total
            && self.ledger_dropped == 0
            && self.faults == 0
    }
}

impl PkgIdxStation {
    /// 空站（空表 + 空账）。
    pub const fn new() -> PkgIdxStation {
        PkgIdxStation {
            table: PkgIndexTable::new(),
            ledger: EventLedger::new(),
        }
    }

    /// 由标准种子建站（12 行登记 + 冻结 + 事件入账）。
    pub fn from_standard() -> Result<PkgIdxStation, PkgIdxError> {
        let mut st = PkgIdxStation::new();
        let mut i: usize = 0;
        while i < SEED_DESC.len() {
            let (d, b, l, v) = SEED_DESC[i];
            st.register_and_log(d, b, l, v)?;
            i += 1;
        }
        st.freeze_and_log()?;
        Ok(st)
    }

    /// 只读表。
    pub fn table(&self) -> &PkgIndexTable {
        &self.table
    }

    /// 只读事件账。
    pub fn ledger(&self) -> &EventLedger {
        &self.ledger
    }

    /// 登记并入账（成功 → EV_REGISTER_OK；失败 → 对应错误码入账后仍返回 Err）。
    pub fn register_and_log(&mut self, domain: u16, base: u64, len: u64, version: u16) -> Result<u16, PkgIdxError> {
        match self.table.register(domain, base, len, version) {
            Ok(pkg_no) => {
                self.ledger.push(EV_REGISTER_OK, domain, pkg_no as u64);
                Ok(pkg_no)
            }
            Err(e) => {
                self.ledger.push(e.code(), domain, base);
                Err(e)
            }
        }
    }

    /// 冻结并入账（表级校验和作辅参）。
    pub fn freeze_and_log(&mut self) -> Result<u32, PkgIdxError> {
        match self.table.freeze() {
            Ok(csum) => {
                self.ledger.push(EV_REGISTER_OK, 0, csum as u64);
                Ok(csum)
            }
            Err(e) => {
                self.ledger.push(e.code(), 0, self.table.filled_count() as u64);
                Err(e)
            }
        }
    }

    /// 按包号检索并入账（命中 → EV_LOOKUP_HIT；Miss → EV_LOOKUP_MISS）。
    pub fn lookup_by_pkg_and_log(&mut self, pkg_no: u16) -> Result<LookupHit, PkgIdxError> {
        match self.table.lookup_by_pkg(pkg_no) {
            Ok(hit) => {
                self.ledger.push(EV_LOOKUP_HIT, hit.domain, pkg_no as u64);
                Ok(hit)
            }
            Err(e) => {
                self.ledger.push(EV_LOOKUP_MISS, pkg_no_to_domain(pkg_no), pkg_no as u64);
                Err(e)
            }
        }
    }

    /// 按域检索并入账。
    pub fn lookup_by_domain_and_log(&mut self, domain: u16) -> Result<LookupHit, PkgIdxError> {
        match self.table.lookup_by_domain(domain) {
            Ok(hit) => {
                self.ledger.push(EV_LOOKUP_HIT, hit.domain, hit.pkg_no as u64);
                Ok(hit)
            }
            Err(e) => {
                self.ledger.push(EV_LOOKUP_MISS, domain, 0);
                Err(e)
            }
        }
    }

    // ── 注入面（三检出；每项入账，检出返回 true） ───────────────

    /// 注入一：行校验和坏注入——翻转指定包号对应行的校验和最低位，随后逐行
    /// 核验必须检出 `RowChecksumMismatch`（0x14DA）。
    /// 返回 Ok(true)=检出，Ok(false)=核验未红（账面失守），Err=注入本身被拒。
    pub fn inject_row_checksum_bad(&mut self, pkg_no: u16) -> Result<bool, PkgIdxError> {
        if !pkg_no_valid(pkg_no) {
            return Err(PkgIdxError::BadPkgNo(pkg_no));
        }
        let slot = (pkg_no_to_domain(pkg_no) - 1) as usize;
        self.table.corrupt_row_checksum(slot)?;
        let detected = self.table.verify_rows().is_err();
        if detected {
            self.ledger.push(E_ROW_CSUM, pkg_no_to_domain(pkg_no), pkg_no as u64);
        }
        Ok(detected)
    }

    /// 注入二：重复域注入——对已登记域再次登记，必须被 `DuplicateDomain`
    /// （0x14D1）拒收。
    pub fn inject_duplicate_domain(&mut self, domain: u16) -> Result<bool, PkgIdxError> {
        if !domain_no_valid(domain) {
            return Err(PkgIdxError::DomainOutOfRange(domain));
        }
        let seed = SEED_DESC[(domain - 1) as usize];
        let detected = match self.table.register(domain, seed.1, seed.2, seed.3) {
            Err(PkgIdxError::DuplicateDomain(_)) => true,
            Ok(_) => false, // 拒收失守：重复放行（账面红灯）
            Err(e) => return Err(e),
        };
        if detected {
            self.ledger.push(E_DUP_DOMAIN, domain, pkg_no_of(domain) as u64);
        }
        Ok(detected)
    }

    /// 注入三：越界检索注入——包号 0x630（域 16 解码越界），必须显性 Miss
    /// `BadPkgNo`（0x14D8），绝不返回零基址命中。
    pub fn inject_oob_lookup(&mut self) -> Result<bool, PkgIdxError> {
        let oob: u16 = PKG_NO_BASE + 0x10; // 0x630：解码域号 16，越界
        match self.table.lookup_by_pkg(oob) {
            Err(PkgIdxError::BadPkgNo(_)) => {
                self.ledger.push(EV_LOOKUP_MISS, 0, oob as u64);
                Ok(true)
            }
            Ok(_) => Ok(false), // 谎报命中：账面红灯
            Err(e) => Err(e),
        }
    }

    /// 健康账：行账齐/行校验和/表校验和/检索 10/10/事件账 全维度现核。
    pub fn health(&self) -> PkgIdxHealth {
        let row_ok = self.table.verify_rows().is_ok();
        let tsum_ok = if self.table.is_frozen() {
            self.table.verify_table_checksum().is_ok()
        } else {
            false
        };
        let (passed, total) = run_lookup_cases(&self.table);
        PkgIdxHealth {
            rows_filled: self.table.filled_count(),
            frozen: self.table.is_frozen(),
            row_checksum_ok: row_ok,
            table_checksum_ok: tsum_ok,
            lookup_cases_passed: passed,
            lookup_cases_total: total,
            ledger_events: self.ledger.len(),
            ledger_dropped: self.ledger.dropped(),
            faults: self.ledger.fault_count(),
        }
    }
}

/// 双跑确定性对拍：标准表建两跑，指纹与快照字节流必须全等。
pub fn determinism_dual() -> DeterminismReport {
    let a = standard_table();
    let b = standard_table();
    match (a, b) {
        (Ok(ta), Ok(tb)) => {
            let fa = ta.fingerprint();
            let fb = tb.fingerprint();
            let sa = ta.snapshot();
            let sb = tb.snapshot();
            let eq = sa.bytes == sb.bytes;
            DeterminismReport {
                fingerprint_a: fa,
                fingerprint_b: fb,
                snapshot_equal: eq,
                agree: fa == fb && eq,
            }
        }
        _ => DeterminismReport {
            fingerprint_a: 0,
            fingerprint_b: 0,
            snapshot_equal: false,
            agree: false,
        },
    }
}

// ═══════════════════════════════════════════════════════════════════
// §15 自检总入口 · run_unx0622_selfcheck（判据逐条可达）
// ═══════════════════════════════════════════════════════════════════

/// 一项自检结论。
#[derive(Clone, Copy, Debug)]
pub struct CheckItem {
    /// 项名（账面口径）。
    pub name: &'static str,
    /// 通过与否。
    pub pass: bool,
    /// 失败定位短语（通过时空串）。
    pub detail: &'static str,
}

/// 自检报告。
#[derive(Clone, Debug)]
pub struct CheckReport {
    /// 套名。
    pub suite: &'static str,
    /// 逐项结论（入账序）。
    pub items: Vec<CheckItem>,
}

impl CheckReport {
    /// 通过项数。
    pub fn passed(&self) -> usize {
        let mut n: usize = 0;
        let mut i: usize = 0;
        while i < self.items.len() {
            if self.items[i].pass {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 失败项数。
    pub fn failed(&self) -> usize {
        self.items.len() - self.passed()
    }

    /// 总判：全绿。
    pub fn is_green(&self) -> bool {
        self.failed() == 0
    }
}

/// UNX-F0622 自检总入口（16 项，逐条对应判据与子系统）。
pub fn run_unx0622_selfcheck() -> CheckReport {
    let mut r = CheckReport {
        suite: "UNX-F0622 交接包索引表（域级检索）",
        items: Vec::new(),
    };
    let t = match standard_table() {
        Ok(t) => t,
        Err(e) => {
            r.items.push(CheckItem {
                name: "IDX-标准表构建",
                pass: false,
                detail: e.detail(),
            });
            return r;
        }
    };

    // 1. 12 行账齐（J1-a 前半）
    let full = t.filled_count() == DOMAIN_COUNT && t.missing_domains().is_empty();
    r.items.push(CheckItem {
        name: "IDX-12行账齐",
        pass: full,
        detail: if full { "" } else { "12 行未齐" },
    });

    // 2. 12 行×4 列逐列齐（域号闭集递增 + 基址对齐 + 长度界内 + 版本 v1）
    let mut cols_ok = true;
    {
        let rows = t.rows_snapshot();
        let mut i: usize = 0;
        while i < DOMAIN_COUNT {
            let row = rows[i];
            if row.domain != (i + 1) as u16
                || !is_pkg_base_aligned(row.base)
                || row.len == 0
                || row.len > PKG_LEN_CAP
                || row.version != VERSION_V1
            {
                cols_ok = false;
            }
            i += 1;
        }
    }
    r.items.push(CheckItem {
        name: "IDX-4列逐列齐",
        pass: cols_ok,
        detail: if cols_ok { "" } else { "列账有缺" },
    });

    // 3. 行校验和逐行核验（J1-a "+行校验和"）
    let rows_ok = t.verify_rows().is_ok();
    r.items.push(CheckItem {
        name: "IDX-行校验和逐行",
        pass: rows_ok,
        detail: if rows_ok { "" } else { "行校验和有红" },
    });

    // 4. 表级校验和核验
    let tsum_ok = t.verify_table_checksum().is_ok();
    r.items.push(CheckItem {
        name: "IDX-表级校验和",
        pass: tsum_ok,
        detail: if tsum_ok { "" } else { "表校验和有红" },
    });

    // 5. 指纹稳定（同表两次导出同指纹）
    let f1 = t.fingerprint();
    let f2 = t.fingerprint();
    r.items.push(CheckItem {
        name: "IDX-指纹稳定",
        pass: f1 == f2,
        detail: if f1 == f2 { "" } else { "指纹漂移" },
    });

    // 6. 指纹敏感（坏一行即异指纹）
    let mut t_bad = t;
    let _ = t_bad.corrupt_row_checksum(3);
    let f_bad = t_bad.fingerprint();
    r.items.push(CheckItem {
        name: "IDX-指纹敏感",
        pass: f_bad != f1,
        detail: if f_bad != f1 { "" } else { "指纹不敏感" },
    });

    // 7. 检索 10/10（J1-b）
    let (passed, total) = run_lookup_cases(&t);
    r.items.push(CheckItem {
        name: "IDX-检索10/10",
        pass: passed == total && total == LOOKUP_CASES_TOTAL,
        detail: if passed == total { "" } else { "用例有失" },
    });

    // 8. 双路一致（按包号 O(1) 与按域 O(12) 对全 12 域同答）
    let mut dual_ok = true;
    {
        let mut d: u16 = DOMAIN_FIRST;
        while d <= DOMAIN_LAST {
            match (t.lookup_by_pkg(pkg_no_of(d)), t.lookup_by_domain(d)) {
                (Ok(a), Ok(b)) => {
                    if a != b {
                        dual_ok = false;
                    }
                }
                _ => dual_ok = false,
            }
            d += 1;
        }
    }
    r.items.push(CheckItem {
        name: "IDX-双路一致",
        pass: dual_ok,
        detail: if dual_ok { "" } else { "双路有分歧" },
    });

    // 9. 显性 Miss 不谎报（未登记域/越界包号均 Err，且无零基址命中形态）
    let miss1 = t.lookup_by_pkg(0x630);
    let miss2 = t.lookup_by_domain(13);
    let miss3 = PkgIndexTable::new().lookup_by_domain(5); // 空表全 Miss
    let miss_ok = match (miss1, miss2, miss3) {
        (Err(e1), Err(e2), Err(e3)) => {
            e1.code() == E_BAD_PKG_NO && e2.code() == E_DOMAIN_RANGE && e3.code() == E_UNREGISTERED
        }
        _ => false,
    };
    r.items.push(CheckItem {
        name: "IDX-显性Miss不谎报",
        pass: miss_ok,
        detail: if miss_ok { "" } else { "Miss 形态失真" },
    });

    // 10. 快照往返一致（含指纹全等）
    let snap = t.snapshot();
    let rt = PkgIndexTable::from_bytes(&snap.bytes);
    let rt_ok = match rt {
        Ok(t2) => t2.fingerprint() == snap.fingerprint && t2.fingerprint() == f1 && t2.is_frozen(),
        Err(_) => false,
    };
    r.items.push(CheckItem {
        name: "IDX-快照往返",
        pass: rt_ok,
        detail: if rt_ok { "" } else { "往返失真" },
    });

    // 11. 快照坏数据拒收（魔数/截断/坏行各一）
    let mut bad_magic = snap.bytes.clone();
    bad_magic[0] = b'X';
    let mut bad_trunc = snap.bytes.clone();
    bad_trunc.truncate(SNAP_HDR_LEN + 5 * SNAP_REC_LEN - 1);
    let mut bad_row = snap.bytes.clone();
    bad_row[SNAP_HDR_LEN + ROW_RAW_LEN] ^= 0x01; // 首行校验和翻位
    let reject_ok = match (
        PkgIndexTable::from_bytes(&bad_magic),
        PkgIndexTable::from_bytes(&bad_trunc),
        PkgIndexTable::from_bytes(&bad_row),
    ) {
        (Err(e1), Err(e2), Err(e3)) => {
            e1.code() == E_SNAPSHOT && e2.code() == E_SNAPSHOT && e3.code() == E_ROW_CSUM
        }
        _ => false,
    };
    r.items.push(CheckItem {
        name: "IDX-快照坏数据拒收",
        pass: reject_ok,
        detail: if reject_ok { "" } else { "坏快照漏收" },
    });

    // 12. 注入一：行校验和坏注入检出
    let mut st1 = match PkgIdxStation::from_standard() {
        Ok(s) => s,
        Err(e) => {
            r.items.push(CheckItem {
                name: "IDX-注入·行校验和",
                pass: false,
                detail: e.detail(),
            });
            return r;
        }
    };
    let inj1 = st1.inject_row_checksum_bad(pkg_no_of(4)).unwrap_or(false);
    r.items.push(CheckItem {
        name: "IDX-注入·行校验和",
        pass: inj1,
        detail: if inj1 { "" } else { "坏行未检出" },
    });

    // 13. 注入二：重复域注入拒收
    let mut st2 = match PkgIdxStation::from_standard() {
        Ok(s) => s,
        Err(e) => {
            r.items.push(CheckItem {
                name: "IDX-注入·重复域",
                pass: false,
                detail: e.detail(),
            });
            return r;
        }
    };
    let inj2 = st2.inject_duplicate_domain(6).unwrap_or(false);
    r.items.push(CheckItem {
        name: "IDX-注入·重复域",
        pass: inj2,
        detail: if inj2 { "" } else { "重复域漏拒" },
    });

    // 14. 注入三：越界检索注入检出
    let mut st3 = match PkgIdxStation::from_standard() {
        Ok(s) => s,
        Err(e) => {
            r.items.push(CheckItem {
                name: "IDX-注入·越界检索",
                pass: false,
                detail: e.detail(),
            });
            return r;
        }
    };
    let inj3 = st3.inject_oob_lookup().unwrap_or(false);
    r.items.push(CheckItem {
        name: "IDX-注入·越界检索",
        pass: inj3,
        detail: if inj3 { "" } else { "越界检索漏检" },
    });

    // 15. 双跑确定性对拍
    let det = determinism_dual();
    r.items.push(CheckItem {
        name: "IDX-双跑确定性",
        pass: det.agree,
        detail: if det.agree { "" } else { "双跑有差" },
    });

    // 16. 健康账全绿（干净站）+ 错误站红灯可见
    let health_green = PkgIdxStation::from_standard()
        .map(|s| s.health().is_green())
        .unwrap_or(false);
    let corrupt_red = st1.health().row_checksum_ok == false && st1.health().faults >= 1;
    r.items.push(CheckItem {
        name: "IDX-健康账",
        pass: health_green && corrupt_red,
        detail: if health_green && corrupt_red { "" } else { "健康账失真" },
    });

    // 17. 诊断段独占（0x14D0~0x14DF 全码落段 + 全错误码逐个落段）
    let mut seg_ok = diag_code_in_seg(SEG_BASE) && diag_code_in_seg(SEG_END);
    {
        let probes = [
            E_DOMAIN_RANGE, E_DUP_DOMAIN, E_BASE_UNALIGNED, E_ZERO_LEN, E_LEN_OVERFLOW,
            E_VERSION_INVALID, E_TABLE_INCOMPLETE, E_TABLE_FROZEN, E_BAD_PKG_NO,
            E_UNREGISTERED, E_ROW_CSUM, E_TABLE_CSUM, E_SNAPSHOT,
            EV_REGISTER_OK, EV_LOOKUP_HIT, EV_LOOKUP_MISS,
        ];
        let mut i: usize = 0;
        while i < probes.len() {
            if !diag_code_in_seg(probes[i]) {
                seg_ok = false;
            }
            i += 1;
        }
    }
    r.items.push(CheckItem {
        name: "IDX-诊断段独占",
        pass: seg_ok,
        detail: if seg_ok { "" } else { "诊断码越段" },
    });

    // 18. 错误诊断非空（13 变体 detail 全非空 + 诊断码全落段）
    let errs = [
        PkgIdxError::DomainOutOfRange(0),
        PkgIdxError::DuplicateDomain(1),
        PkgIdxError::BaseUnaligned(0x1234),
        PkgIdxError::ZeroLength(1),
        PkgIdxError::LenOverflow { base: 0xFFFF_F000, len: 0x2000 },
        PkgIdxError::VersionInvalid(0),
        PkgIdxError::TableIncomplete { filled: 3, need: 12 },
        PkgIdxError::TableFrozen(1),
        PkgIdxError::BadPkgNo(0x630),
        PkgIdxError::UnregisteredDomain(7),
        PkgIdxError::RowChecksumMismatch { slot: 0, stored: 0, computed: 1 },
        PkgIdxError::TableChecksumMismatch { stored: 0, computed: 1 },
        PkgIdxError::SnapshotCorrupt("探针"),
    ];
    let mut det_ok = true;
    {
        let mut i: usize = 0;
        while i < errs.len() {
            if errs[i].detail().is_empty() || !diag_code_in_seg(errs[i].code()) {
                det_ok = false;
            }
            i += 1;
        }
    }
    r.items.push(CheckItem {
        name: "IDX-错误诊断非空",
        pass: det_ok,
        detail: if det_ok { "" } else { "诊断缺失" },
    });

    // 19. 登记校验链五拒（越界域/零长/超限长/坏版本/冻结后改 全部拒收）
    let mut t5 = PkgIndexTable::new();
    let rej1 = t5.register(0, 0x6200_0000, 0x1000, VERSION_V1).is_err();
    let rej2 = t5.register(1, 0x6200_0000, 0, VERSION_V1).is_err();
    let rej3 = t5.register(1, 0x6200_0000, PKG_LEN_CAP + PKG_BASE_ALIGN, VERSION_V1).is_err();
    let rej4 = t5.register(1, 0x6200_0000, 0x1000, 0).is_err();
    let rej5 = {
        let _ = t5.register(1, 0x6200_0000, 0x1000, VERSION_V1);
        t5.frozen = true; // 直改冻结位模拟冻结后改
        t5.register(2, 0x6201_0000, 0x1000, VERSION_V1).is_err()
    };
    let rej_ok = rej1 && rej2 && rej3 && rej4 && rej5;
    r.items.push(CheckItem {
        name: "IDX-登记校验链五拒",
        pass: rej_ok,
        detail: if rej_ok { "" } else { "校验链漏拒" },
    });

    // 20. 未账齐拒冻结
    let mut t6 = PkgIndexTable::new();
    let _ = t6.register(1, 0x6200_0000, 0x1000, VERSION_V1);
    let freeze_rej = match t6.freeze() {
        Err(PkgIdxError::TableIncomplete { filled, need }) => filled == 1 && need == 12,
        _ => false,
    };
    r.items.push(CheckItem {
        name: "IDX-未账齐拒冻结",
        pass: freeze_rej,
        detail: if freeze_rej { "" } else { "欠账放冻" },
    });

    r
}

// ═══════════════════════════════════════════════════════════════════
// §16 编译期结构断言（体量红线 + 段界）
// ═══════════════════════════════════════════════════════════════════
const _: () = assert!(core::mem::size_of::<PkgIndexRow>() <= 64);
const _: () = assert!(core::mem::size_of::<PkgIndexTable>() <= 64 * 1024); // 12 行表远低于 64KiB 栈红线
const _: () = assert!(core::mem::size_of::<EventLedger>() <= 64 * 1024); // 64 槽 × 10B ≈ 640B
const _: () = assert!(core::mem::size_of::<LookupHit>() <= 64);
const _: () = assert!(SNAP_HDR_LEN == 14);
const _: () = assert!(SNAP_REC_LEN == 22);
const _: () = assert!(SNAP_HDR_LEN + DOMAIN_COUNT * SNAP_REC_LEN == 278); // 满表快照总长

// ═══════════════════════════════════════════════════════════════════
// §17 测试（仅 probe/宿主测试路径；内核路径零 assert/unwrap）
// ═══════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;

    /// 冒烟 1：12 行账齐 + 行校验和真算逐行一致。
    #[test]
    fn smoke_12_rows_and_row_checksums() {
        let t = standard_table().expect("标准表构建");
        assert_eq!(t.filled_count(), 12);
        assert!(t.is_frozen());
        assert!(t.verify_rows().is_ok());
        let rows = t.rows_snapshot();
        let mut i = 0;
        while i < DOMAIN_COUNT {
            let r = rows[i];
            assert_eq!(r.domain, (i + 1) as u16);
            assert!(r.checksum_ok(), "行 {} 校验和应自洽", i);
            // 行校验和真算性：与独立重算一致，且非占位零
            assert_eq!(r.checksum, fnv16(&r.raw_bytes()));
            assert_ne!(r.checksum, 0, "FNV16 真算不应落在零占位");
            i += 1;
        }
    }

    /// 冒烟 2：检索 10/10 用例命中一致（J1-b）。
    #[test]
    fn smoke_lookup_10_of_10() {
        let t = standard_table().expect("标准表构建");
        let (passed, total) = run_lookup_cases(&t);
        assert_eq!(total, 10);
        assert_eq!(passed, 10, "10/10 用例必须全命中");
    }

    /// 冒烟 3：双路一致 + 命中字段正确。
    #[test]
    fn smoke_dual_path_hits() {
        let t = standard_table().expect("标准表构建");
        let mut d = DOMAIN_FIRST;
        while d <= DOMAIN_LAST {
            let a = t.lookup_by_pkg(pkg_no_of(d)).expect("按包号命中");
            let b = t.lookup_by_domain(d).expect("按域命中");
            assert_eq!(a, b);
            assert_eq!(a.domain, d);
            assert_eq!(a.base, SEED_DESC[(d - 1) as usize].1);
            assert_eq!(a.len, SEED_DESC[(d - 1) as usize].2);
            assert_eq!(a.version, VERSION_V1);
            d += 1;
        }
    }

    /// 冒烟 4：未登记域显性 Miss（不返回零基址谎报）。
    #[test]
    fn smoke_explicit_miss_no_zero_base() {
        let mut t = PkgIndexTable::new();
        let _ = t.register(5, 0x6200_0000, 0x1000, VERSION_V1);
        // 按域：未登记域 7 → UnregisteredDomain
        match t.lookup_by_domain(7) {
            Err(PkgIdxError::UnregisteredDomain(d)) => assert_eq!(d, 7),
            other => panic!("应为显性 Miss，实际 {:?}", other),
        }
        // 按包号：空槽 0x621 → UnregisteredDomain（不返回零基址命中）
        match t.lookup_by_pkg(PKG_NO_FIRST) {
            Err(PkgIdxError::UnregisteredDomain(_)) => {}
            other => panic!("空槽应显性 Miss，实际 {:?}", other),
        }
        // 越界包号 → BadPkgNo
        match t.lookup_by_pkg(0x630) {
            Err(PkgIdxError::BadPkgNo(p)) => assert_eq!(p, 0x630),
            other => panic!("越界包号应 BadPkgNo，实际 {:?}", other),
        }
    }

    /// 冒烟 5：注入三检出（行校验和坏 / 重复域 / 越界检索）。
    #[test]
    fn smoke_injections_detected() {
        let mut s1 = PkgIdxStation::from_standard().expect("标准站");
        assert!(s1.inject_row_checksum_bad(pkg_no_of(4)).expect("注入一"));
        assert!(!s1.health().row_checksum_ok);
        assert!(s1.health().faults >= 1);

        let mut s2 = PkgIdxStation::from_standard().expect("标准站");
        assert!(s2.inject_duplicate_domain(6).expect("注入二"));
        assert_eq!(s2.ledger().count_code(E_DUP_DOMAIN), 1);

        let mut s3 = PkgIdxStation::from_standard().expect("标准站");
        assert!(s3.inject_oob_lookup().expect("注入三"));
        assert_eq!(s3.ledger().count_code(EV_LOOKUP_MISS), 1);
    }

    /// 冒烟 6：双跑确定性对拍。
    #[test]
    fn smoke_dual_run_determinism() {
        let rep = determinism_dual();
        assert!(rep.agree, "双跑指纹与快照字节必须全等");
        assert_eq!(rep.fingerprint_a, rep.fingerprint_b);
        assert!(rep.snapshot_equal);
    }

    /// 冒烟 7：快照往返 + 坏数据拒收。
    #[test]
    fn smoke_snapshot_roundtrip_and_reject() {
        let t = standard_table().expect("标准表构建");
        let snap = t.snapshot();
        assert_eq!(snap.row_count, 12);
        assert_eq!(snap.bytes.len(), SNAP_HDR_LEN + 12 * SNAP_REC_LEN);
        let t2 = PkgIndexTable::from_bytes(&snap.bytes).expect("往返重建");
        assert!(t2.is_frozen());
        assert_eq!(t2.fingerprint(), snap.fingerprint);
        // 坏魔数
        let mut bad = snap.bytes.clone();
        bad[2] = b'Z';
        assert!(PkgIndexTable::from_bytes(&bad).is_err());
        // 截断
        let mut cut = snap.bytes.clone();
        cut.pop();
        assert!(PkgIndexTable::from_bytes(&cut).is_err());
        // 坏行校验和
        let mut br = snap.bytes.clone();
        br[SNAP_HDR_LEN + 20] ^= 0x02;
        match PkgIndexTable::from_bytes(&br) {
            Err(PkgIdxError::RowChecksumMismatch { .. }) => {}
            other => panic!("坏行应行校验和红，实际 {:?}", other),
        }
        // 坏表级校验和
        let mut bt = snap.bytes.clone();
        bt[10] ^= 0x01;
        match PkgIndexTable::from_bytes(&bt) {
            Err(PkgIdxError::TableChecksumMismatch { .. }) => {}
            other => panic!("坏表和应表校验和红，实际 {:?}", other),
        }
    }

    /// 冒烟 8：健康账（绿站全绿 + 红站可见）。
    #[test]
    fn smoke_health() {
        let s = PkgIdxStation::from_standard().expect("标准站");
        let h = s.health();
        assert!(h.is_green());
        assert_eq!(h.rows_filled, 12);
        assert_eq!(h.lookup_cases_passed, 10);
        assert_eq!(h.faults, 0);
        // 坏站：行坏 → 红灯
        let mut bad = PkgIdxStation::from_standard().expect("标准站");
        let _ = bad.inject_row_checksum_bad(pkg_no_of(9));
        let hb = bad.health();
        assert!(!hb.is_green());
        assert!(!hb.row_checksum_ok);
    }

    /// 冒烟 9：事件账（登记 12 + 冻结 1 + 检索事件 + 序号单调 + 环形丢弃显性）。
    #[test]
    fn smoke_event_ledger() {
        let mut s = PkgIdxStation::from_standard().expect("标准站");
        let _ = s.lookup_by_pkg_and_log(pkg_no_of(1)).expect("命中");
        let _ = s.lookup_by_domain_and_log(13); // Miss 入账
        let lg = s.ledger();
        // 12 登记 + 1 冻结 + 1 命中 + 1 Miss = 15
        assert_eq!(lg.len(), 15);
        assert_eq!(lg.dropped(), 0);
        assert_eq!(lg.count_code(EV_REGISTER_OK), 13); // 12 登记 + 1 冻结
        assert_eq!(lg.count_code(EV_LOOKUP_HIT), 1);
        assert_eq!(lg.count_code(EV_LOOKUP_MISS), 1);
        // 序号全序单调
        let mut i = 1;
        while i < lg.len() {
            let a = lg.event(i - 1).expect("在账");
            let b = lg.event(i).expect("在账");
            assert!(b.seq == a.seq.wrapping_add(1));
            i += 1;
        }
        // 环形覆盖：压 70 条，丢 6 条显性入账，容量仍 64
        let mut l2 = EventLedger::new();
        let mut k = 0u32;
        while k < 70 {
            l2.push(EV_LOOKUP_HIT, (k % 12 + 1) as u16, k as u64);
            k += 1;
        }
        assert_eq!(l2.len(), LEDGER_CAP);
        assert_eq!(l2.dropped(), 6);
        // 最旧已被覆盖：首条 seq = 6
        assert_eq!(l2.event(0).expect("在账").seq, 6);
    }

    /// 冒烟 10：自检总入口全绿。
    #[test]
    fn smoke_selfcheck_green() {
        let r = run_unx0622_selfcheck();
        assert!(r.is_green(), "自检应有红：{:?}", r.items.iter().filter(|i| !i.pass).map(|i| i.name).collect::<Vec<_>>());
        assert!(r.passed() >= 19);
        assert_eq!(r.failed(), 0);
    }

    /// 冒烟 11：12 域闭集与 const 域表一致（枚举 ↔ 表双写核）。
    #[test]
    fn smoke_domain_closed_set() {
        assert_eq!(HandoffDomain::COUNT, 12);
        assert_eq!(DOMAIN_TABLE.len(), 12);
        let all = [
            HandoffDomain::Timing, HandoffDomain::Acpi, HandoffDomain::Power,
            HandoffDomain::Memory, HandoffDomain::Interrupt, HandoffDomain::Pci,
            HandoffDomain::Display, HandoffDomain::Input, HandoffDomain::FsVolume,
            HandoffDomain::Security, HandoffDomain::Log, HandoffDomain::Finalize,
        ];
        let mut i = 0;
        while i < all.len() {
            let d = all[i];
            let row = DOMAIN_TABLE[i];
            assert_eq!(row.0, d.no());
            assert_eq!(row.1, d.name());
            assert_eq!(row.2, d.anchor());
            assert_eq!(HandoffDomain::from_no(d.no()), Some(d));
            i += 1;
        }
        assert_eq!(HandoffDomain::from_no(0), None);
        assert_eq!(HandoffDomain::from_no(13), None);
        assert!(!domain_no_valid(0));
        assert!(!domain_no_valid(13));
    }

    /// 冒烟 12：包号编码 O(1) 与诊断段独占。
    #[test]
    fn smoke_pkg_no_codec_and_diag_seg() {
        assert_eq!(pkg_no_of(1), 0x621);
        assert_eq!(pkg_no_of(12), 0x62C);
        assert_eq!(pkg_no_to_domain(0x621), 1);
        assert_eq!(pkg_no_to_domain(0x62C), 12);
        assert!(pkg_no_valid(0x621));
        assert!(!pkg_no_valid(0x620));
        assert!(!pkg_no_valid(0x62D));
        assert_eq!(version_major(VERSION_V1), 1);
        assert_eq!(version_minor(VERSION_V1), 0);
        assert!(version_valid(VERSION_V1));
        assert!(!version_valid(0));
        assert!(!version_valid(0x0042));
        assert!(is_pkg_base_aligned(0x6203_0000));
        assert!(!is_pkg_base_aligned(0x6203_0801));
        // 诊断段独占：全 16 码连续落段
        let mut c = SEG_BASE;
        while c <= SEG_END {
            assert!(diag_code_in_seg(c));
            c += 1;
        }
        assert!(!diag_code_in_seg(0x14CF));
        assert!(!diag_code_in_seg(0x14E0));
    }
}
