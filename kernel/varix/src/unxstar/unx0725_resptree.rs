//! UNX-F0725 · boot info 响应树兼容账（升级字段冻结核对）· 诊断段 0x1650
//!
//! ── 验收锚点（原文全抄） ─────────────────────────────────────────────
//! 定位：docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md
//! 行 2315，表头「| 编号 | 功能条目 | 行数 | 状态 | 判据 |」下：
//!
//! | UNX-F0725 | boot info 响应树兼容账（升级字段冻结核对） | 570 | 已深化 |
//!   UNX-F0725-J1 响应树兼容核对账齐（memory map/framebuffer/kernel address
//!   三响应逐字段冻结表核对 100%） |
//!
//! J 判据拆解（本模块逐条落地的验收口径）：
//!   J1-a 三响应闭集：RespKind 只认 MemoryMap / Framebuffer / KernelAddress
//!        三臂（Limine boot info 协议响应面域），闭集之外一律
//!        E_KIND_UNKNOWN（0x1651）拒收，不存在第四种响应。
//!   J1-b 逐字段冻结表：每响应一张 FieldFreezeTable，逐字段登记
//!        名称/偏移/宽度/语义档/冻结位；冻结字段变更即破坏兼容，
//!        冻结表版本化演进只增不改（追加登记制）。
//!   J1-c 逐字段核对 100%：RespTreeVerifier 对响应实例 vs 冻结表逐字段核，
//!        三拒显性（字段缺失 E_FIELD_MISSING / 类型不符 E_TYPE_MISMATCH /
//!        魔法数不符 E_MAGIC_MISMATCH）；核对率 ppm 整数制，
//!        10000 ppm = 100% 才算账齐（rate_ppm == 10_000）。
//!   J1-d 兼容三态：升级前响应树 vs 升级后响应树判 Compatible /
//!        AdditiveOnly（仅加法，且加法必须走升级登记，未登记加法按破坏
//!        拦截）/ Breaking（破坏即拦截立案，CompatBlocked 落事件账）。
//!
//! ── 深度子系统清单（本文件全手工定制，零注水） ───────────────────────
//!   S1 RespKind 三响应闭集 + 语义档 SemTag 九档（Revision/Count/Ptr/
//!      PhysAddr/VirtAddr/Size/AsciiMagic/EnumTag/MaskBits），每档带
//!      const 期合理性核（高半区基线/指针对齐/物理域上界/闭集枚举/ASCII 魔法）。
//!   S2 FieldFreezeTable 冻结表：memory map（头 3 行 + 条目 schema 3 行）、
//!      framebuffer（头 3 行 + 条目 schema 14 行）、kernel address（头 3 行），
//!      字段偏移/宽度按 Limine Boot Protocol 布局写死并注明来源。
//!   S3 UpgradeRegistry 升级登记账：只增不改（append-only），加法字段
//!      登记需过名重复/偏移碰撞/宽度域三关；未登记加法在树构建期
//!      E_UNREG_ADDITIVE 拦截。
//!   S4 RespTreeVerifier 逐字段核对器：头字段 + 条目 schema 逐行核 +
//!      交叉核（entry_count ↔ 实际条目数），FieldCheck 逐字段留痕。
//!   S5 RespTree 响应树快照：确定性序列化（kind+版本+名+值行序固定）+
//!      FNV-1a 64 指纹；同输入双跑指纹必等。
//!   S6 双树 diff：逐字段差异清单 + 归因（FrozenDrift/FrozenMissing/
//!      UnregisteredAdditive/EvolvableDrift/OnlyInBefore），兼容三态判定。
//!   S7 事件账 Ledger（封板制）+ health() 绿灯 + 账级 FNV 指纹。
//!   S8 注入面四路：冻结字段篡改 / 字段缺失 / 魔法数注入 / 未登记加法，
//!      每路显性检出（detected + diag + breaking）。
//!
//! ── 相邻任务对接 ─────────────────────────────────────────────────────
//!   F0721 版本台账：冻结表 table_version 与台账版本号同源。
//!   F0722 版本指纹账：本账树指纹（FNV-1a 64）可作其绑定输入。
//!   F0724 升级触发回归：本账 100% 核对是其 300+ 断言的响应面子集。
//!   F0726 必需响应缺失 panic 路径：本账 VerifyRejected 是其前置栅栏。
//!   F0727 升级前后 A/B 零差异对照：本账双跑对拍（dual_run_check）同构。
//!
//! ── 通用纪律 ─────────────────────────────────────────────────────────
//!   no_std + alloc；只 use core::… 与 use alloc::{…}，禁 use crate::…。
//!   内核路径零 unwrap/expect/panic!/运行期 assert!（编译期 const assert! 允许）。
//!   可失败 API 全走 Result<T, RespTreeError>；溢出显性 saturating/wrapping。
//!   >64KiB 结构体禁上栈（本文件全部大结构走 Vec 堆；栈上最大为
//!   FieldFreezeTable ≤ 64B 与 MagicProbe ≤ 32B，安全）。
//!   const fn 纪律：const fn 内零 slice::get / Vec 索引 / Ord::min-max /
//!   derive(PartialEq) 类型的 == / for 循环（一律 while + 索引）；
//!   含 Vec 字段的结构体禁 derive Copy。
//!
//! 零浮点、零 unwrap、零 expect、零 panic 路径（#[cfg(test)] 测试域除外）。

use core::mem::size_of;

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};

// ── 诊断码段（0x1650~0x165F 独占 0x10；先验空闲：kernel/varix/src 全树
//    grep "0x165" 仅命中 EBML u32 魔法 0x1654_AE6B（stard/mediainfo.rs 与
//    svstar2/veg03_webm_mkv.rs），非 u16 诊断码，不冲突；0x1610 段被
//    unx0687_succrate 占，0x1600 段被 unx0663_tsccross 引用，故取 0x1650）──
pub const SEG_BASE: u16 = 0x1650;

const E_SEALED: u16 = SEG_BASE + 0x0; // 0x1650 账已封板仍写
const E_KIND_UNKNOWN: u16 = SEG_BASE + 0x1; // 0x1651 响应种类越三臂闭集
const E_FIELD_MISSING: u16 = SEG_BASE + 0x2; // 0x1652 冻结表字段在实例中缺失
const E_TYPE_MISMATCH: u16 = SEG_BASE + 0x3; // 0x1653 字段语义档类型不符
const E_MAGIC_MISMATCH: u16 = SEG_BASE + 0x4; // 0x1654 魔法数/闭集不符
const E_TABLE_ABSENT: u16 = SEG_BASE + 0x5; // 0x1655 冻结表/基线树缺席
const E_DUP_TABLE: u16 = SEG_BASE + 0x6; // 0x1656 同种响应重复注册冻结表
const E_ADDITIVE_COLLISION: u16 = SEG_BASE + 0x7; // 0x1657 加法登记名重/偏移碰撞
const E_UNREG_ADDITIVE: u16 = SEG_BASE + 0x8; // 0x1658 未登记加法（载荷越覆盖面）
const E_WIDTH_BAD: u16 = SEG_BASE + 0x9; // 0x1659 字段宽度非法（0 或 >8）
const E_PAYLOAD_SHORT: u16 = SEG_BASE + 0xA; // 0x165A 载荷短于字段偏移+宽度
const E_ENTRY_SCHEMA_FAIL: u16 = SEG_BASE + 0xB; // 0x165B 条目 schema 交叉核破
const E_RATE_NOT_FULL: u16 = SEG_BASE + 0xC; // 0x165C 核对率不足 100%（<10000ppm）
const E_REPLAY_DRIFT: u16 = SEG_BASE + 0xD; // 0x165D 双跑对拍指纹漂移
const E_CAPACITY: u16 = SEG_BASE + 0xE; // 0x165E 事件账容量满（512 封顶）
const E_FIELD_UNKNOWN: u16 = SEG_BASE + 0xF; // 0x165F 按名查字段不中

/// 诊断码 → 中文可操作诊断（独占段 0x1650~0x165F 内解释，段外一律未知）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEALED => "账已封板：再写属误用，请克隆新账或走只读通道",
        E_KIND_UNKNOWN => "响应种类越三臂闭集：合法仅 memory_map/framebuffer/kernel_address",
        E_FIELD_MISSING => "冻结表字段在实例中缺失：请补齐载荷或核对偏移布局",
        E_TYPE_MISMATCH => "字段语义档类型不符：请按冻结表语义档检查取值域",
        E_MAGIC_MISMATCH => "魔法数/闭集不符：revision 越上限、枚举越闭集或高半区破",
        E_TABLE_ABSENT => "冻结表或基线树缺席：请先完成三响应 ingest 与基线冻结",
        E_DUP_TABLE => "同种响应重复注册冻结表：每种响应只许一张冻结表",
        E_ADDITIVE_COLLISION => "加法登记碰撞：字段名重复或偏移区间与既有行重叠",
        E_UNREG_ADDITIVE => "未登记加法：载荷越冻结表覆盖面，请先走升级登记",
        E_WIDTH_BAD => "字段宽度非法：合法 1..=8 字节（u64 LE 口径）",
        E_PAYLOAD_SHORT => "载荷短于字段偏移+宽度：请检查响应头完整性与截断注入",
        E_ENTRY_SCHEMA_FAIL => "条目 schema 交叉核破：条目数与计数字段不符或条目越界",
        E_RATE_NOT_FULL => "核对率不足 100%：冻结字段未全核对，拒出树快照",
        E_REPLAY_DRIFT => "双跑对拍指纹漂移：同输入两树指纹不等，确定性破",
        E_CAPACITY => "事件账容量满（512）：请封板归档后开新账",
        E_FIELD_UNKNOWN => "按名查字段不中：请核对冻结表内字段名拼写",
        _ => "未知诊断码（0x1650 段外或保留位）",
    }
}

// ── 常量（来源逐条注明） ─────────────────────────────────────────────
/// 三臂闭集臂数（判据 J1 原文：memory map/framebuffer/kernel address 三响应）。
pub const KIND_COUNT: usize = 3;
/// revision 序列魔法上限（Limine Boot Protocol 各响应 revision 实际为个位数；
/// 账面上限 64 防漂移，越线即魔法不符）。
pub const REV_CAP: u64 = 64;
/// 计数字段账面上限（Limine 请求/响应计数字段实际为数百量级；账面 4096）。
pub const COUNT_CAP: u64 = 4096;
/// 指针线性域上限：≥2^63 视为非规范地址（x86-64 线性地址 48/57 位口径保守界）。
pub const PTR_DOMAIN_CAP: u64 = 1u64 << 63;
/// 指针对齐：Limine 响应内指针字段 8 字节对齐（limine.h 数据结构口径）。
pub const PTR_ALIGN_MASK: u64 = 0x7;
/// 物理地址域上界：x86-64 最大物理位宽 52（Intel SDM Vol.3 §4.5），账面保守 2^48。
pub const PHYS_CAP: u64 = 1u64 << 48;
/// 高半区基线：Limine kernel address 的 virtual_base 落高半区
/// （Limine Boot Protocol kernel address response 布局，higher-half 基线）。
pub const VIRT_FLOOR: u64 = 0xFFFF_8000_0000_0000;
/// 单区/步长账面上限 1 TiB（memory map 区长与 framebuffer pitch 的保守界）。
pub const SIZE_CAP: u64 = 1u64 << 40;
/// memory map 条目类型闭集上界：0..=6（Limine memory map：USABLE=0,
/// RESERVED=1, ACPI_RECLAIMABLE=2, ACPI_NVS=3, BAD_MEMORY=4,
/// BOOTLOADER_REUSABLE=5, EXECUTABLE=6）。
pub const MM_TYPE_MAX: u64 = 6;
/// framebuffer memory_model ASCII 魔法 "fb\0\0\0\0\0\0" 小端 u64
/// （Limine framebuffer：memory_model 必为 "fb"）。
pub const FB_MODEL_MAGIC: u64 = 0x0000_0000_0000_6266; // 'f' | 'b'<<8
/// 位掩码档上界（bpp/掩码宽度 ≤ 64 位）。
pub const MASK_CAP: u64 = 64;
/// 响应头载荷账面口径上限 4 KiB（防止异常巨载荷）。
pub const PAYLOAD_CAP: usize = 4096;
/// 条目载荷账面口径上限 512 B（memory map 条目 24B / framebuffer 结构 80B）。
pub const ENTRY_CAP: usize = 512;
/// 核对率 ppm 基数：10000 ppm = 100%（判据「核对 100%」的整数口径）。
pub const PPM_SCALE: u64 = 10_000;
/// 冻结表基线版本（版本化演进只增不改的起点）。
pub const TABLE_VERSION_BASE: u32 = 1;

// ── S1：RespKind 三响应闭集（判据 J1-a） ─────────────────────────────
/// boot info 响应三臂闭集。闭集之外无第四臂；原始字节只认 1/2/3。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RespKind {
    /// memory map 响应（Limine memory map response）。
    MemoryMap,
    /// framebuffer 响应（Limine framebuffer response）。
    Framebuffer,
    /// kernel address 响应（Limine kernel address response）。
    KernelAddress,
}

impl RespKind {
    /// 闭集原始字节：MemoryMap=1，Framebuffer=2，KernelAddress=3。
    pub const fn code(self) -> u8 {
        match self {
            RespKind::MemoryMap => 1,
            RespKind::Framebuffer => 2,
            RespKind::KernelAddress => 3,
        }
    }

    /// 原始字节 → 闭集臂；越闭集（非 1/2/3）返回 None（对应 E_KIND_UNKNOWN）。
    pub const fn from_code(c: u8) -> Option<RespKind> {
        match c {
            1 => Some(RespKind::MemoryMap),
            2 => Some(RespKind::Framebuffer),
            3 => Some(RespKind::KernelAddress),
            _ => None,
        }
    }

    /// 下标序（0..3）→ 闭集臂；供 while 遍历全臂数。
    pub const fn from_index(i: usize) -> Option<RespKind> {
        match i {
            0 => Some(RespKind::MemoryMap),
            1 => Some(RespKind::Framebuffer),
            2 => Some(RespKind::KernelAddress),
            _ => None,
        }
    }

    /// 闭集臂稳定名（快照序列化与诊断文案用）。
    pub const fn name(self) -> &'static str {
        match self {
            RespKind::MemoryMap => "memory_map",
            RespKind::Framebuffer => "framebuffer",
            RespKind::KernelAddress => "kernel_address",
        }
    }

    /// 魔法探针：该臂的「魔法数」字段定位与越界垃圾值（注入面 S8 用）。
    /// memory map → 首条目 entry_type（越 0..=6 闭集）；
    /// framebuffer → 首条目 memory_model（越 "fb" ASCII 魔法）；
    /// kernel address → 头 virtual_base（跌破高半区基线）。
    pub const fn magic_probe(self) -> MagicProbe {
        match self {
            RespKind::MemoryMap => MagicProbe {
                in_entry: true,
                offset: 16,
                width: 8,
                garbage: MM_TYPE_MAX + 1, // 7：闭集外第一值
            },
            RespKind::Framebuffer => MagicProbe {
                in_entry: true,
                offset: 40,
                width: 8,
                garbage: u64::MAX, // 全 1：必非可打印 ASCII
            },
            RespKind::KernelAddress => MagicProbe {
                in_entry: false,
                offset: 16,
                width: 8,
                garbage: 0x0000_7FFF_0000_0000, // 跌破 VIRT_FLOOR 的低半区值
            },
        }
    }
}

/// 魔法探针定位（in_entry=true 时 offset 相对条目载荷，否则相对响应头）。
#[derive(Clone, Copy, Debug)]
pub struct MagicProbe {
    /// true：魔法定位于条目载荷；false：定位于响应头。
    pub in_entry: bool,
    /// 字节偏移。
    pub offset: u16,
    /// 字节宽（1..=8）。
    pub width: u16,
    /// 越魔法垃圾值（注入用，必被对应语义档判 MagicMismatch）。
    pub garbage: u64,
}

// ── 语义档（SemTag）：字段语义九档，const 期合理性核 ─────────────────
/// 字段语义档。核对器的「类型不符/魔法数不符」三拒即按档裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemTag {
    /// 协议修订号：非零且 ≤ REV_CAP（越线为魔法不符）。
    Revision,
    /// 计数：非零且 ≤ COUNT_CAP（零计数按类型不符，缺条目另行交叉核）。
    Count,
    /// 指针：非零、8 对齐、落线性域（< PTR_DOMAIN_CAP）。
    Ptr,
    /// 物理地址：非零且 < PHYS_CAP。
    PhysAddr,
    /// 高半区虚拟地址：≥ VIRT_FLOOR（跌破为魔法不符：higher-half 哨兵）。
    VirtAddr,
    /// 尺寸/步长：非零且 ≤ SIZE_CAP。
    Size,
    /// 8 字节 ASCII 魔法（如 "fb"）：字节须为 0 或可打印 ASCII 且非全零。
    AsciiMagic,
    /// 闭集枚举：≤ MM_TYPE_MAX（memory map 条目类型 0..=6）。
    EnumTag,
    /// 位掩码档：≤ MASK_CAP（bpp/掩码宽度）。
    MaskBits,
}

/// 单字段合理性核结论（Plausible=过；其余对应三拒之二）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckOutcome {
    /// 合理，过。
    Plausible,
    /// 类型不符（E_TYPE_MISMATCH 路径）。
    TypeMismatch,
    /// 魔法数不符（E_MAGIC_MISMATCH 路径）。
    MagicMismatch,
}

impl SemTag {
    /// const 期合理性核：按语义档判取值域。while+索引遍历字节，零 for。
    pub const fn check(self, raw: u64) -> CheckOutcome {
        match self {
            SemTag::Revision => {
                if raw == 0 || raw > REV_CAP {
                    CheckOutcome::MagicMismatch
                } else {
                    CheckOutcome::Plausible
                }
            }
            SemTag::Count => {
                if raw == 0 || raw > COUNT_CAP {
                    CheckOutcome::TypeMismatch
                } else {
                    CheckOutcome::Plausible
                }
            }
            SemTag::Ptr => {
                if raw == 0 || (raw & PTR_ALIGN_MASK) != 0 || raw >= PTR_DOMAIN_CAP {
                    CheckOutcome::TypeMismatch
                } else {
                    CheckOutcome::Plausible
                }
            }
            SemTag::PhysAddr => {
                if raw == 0 || raw >= PHYS_CAP {
                    CheckOutcome::TypeMismatch
                } else {
                    CheckOutcome::Plausible
                }
            }
            SemTag::VirtAddr => {
                if raw < VIRT_FLOOR {
                    CheckOutcome::MagicMismatch
                } else {
                    CheckOutcome::Plausible
                }
            }
            SemTag::Size => {
                if raw == 0 || raw > SIZE_CAP {
                    CheckOutcome::TypeMismatch
                } else {
                    CheckOutcome::Plausible
                }
            }
            SemTag::AsciiMagic => {
                // 每字节须为 0 或可打印 ASCII（0x20..=0x7E），且至少一个非零。
                let mut i: u32 = 0;
                let mut ok = true;
                let mut nonzero_seen = false;
                while i < 8 {
                    let b = ((raw >> (8 * i)) & 0xFF) as u8;
                    if b != 0 {
                        nonzero_seen = true;
                        if b < 0x20 || b > 0x7E {
                            ok = false;
                        }
                    }
                    i = i.wrapping_add(1);
                }
                if !ok || !nonzero_seen {
                    CheckOutcome::MagicMismatch
                } else {
                    CheckOutcome::Plausible
                }
            }
            SemTag::EnumTag => {
                if raw > MM_TYPE_MAX {
                    CheckOutcome::MagicMismatch
                } else {
                    CheckOutcome::Plausible
                }
            }
            SemTag::MaskBits => {
                if raw > MASK_CAP {
                    CheckOutcome::TypeMismatch
                } else {
                    CheckOutcome::Plausible
                }
            }
        }
    }
}

// ── S2：冻结字段与冻结表（判据 J1-b） ────────────────────────────────
/// 冻结表单行：字段级登记（名称/偏移/宽度/语义档/冻结位）。
/// frozen=true 即冻结字段：实例与该行不一致 → 破坏兼容（拦截立案）；
/// frozen=false 即可演进字段：变更允许但落 diff 归因（EvolvableDrift）。
#[derive(Clone, Copy, Debug)]
pub struct FreezeField {
    /// 字段稳定名（快照行与 diff 的对账键）。
    pub name: &'static str,
    /// 字节偏移（响应头相对 0；条目 schema 相对条目载荷 0）。
    pub offset: u16,
    /// 字节宽（1..=8，小端 u64 口径）。
    pub width: u16,
    /// 语义档。
    pub sem: SemTag,
    /// 冻结位：true=冻结字段（变更即破坏兼容）。
    pub frozen: bool,
}

/// memory map 响应头冻结表（Limine memory map response 布局：
/// revision@0 / entry_count@8 / entries@16，各 u64）。
pub const MM_FIELDS: &[FreezeField] = &[
    FreezeField { name: "revision", offset: 0, width: 8, sem: SemTag::Revision, frozen: true },
    FreezeField { name: "entry_count", offset: 8, width: 8, sem: SemTag::Count, frozen: true },
    FreezeField { name: "entries", offset: 16, width: 8, sem: SemTag::Ptr, frozen: true },
];

/// memory map 条目 schema（Limine memory map entry：base@0 / length@8 /
/// type@16，各 u64；type 闭集 0..=6）。
pub const MM_ENTRY_FIELDS: &[FreezeField] = &[
    FreezeField { name: "base", offset: 0, width: 8, sem: SemTag::PhysAddr, frozen: true },
    FreezeField { name: "length", offset: 8, width: 8, sem: SemTag::Size, frozen: true },
    FreezeField { name: "entry_type", offset: 16, width: 8, sem: SemTag::EnumTag, frozen: true },
];

/// framebuffer 响应头冻结表（Limine framebuffer response 布局：
/// revision@0 / framebuffer_count@8 / framebuffers@16）。
pub const FB_FIELDS: &[FreezeField] = &[
    FreezeField { name: "revision", offset: 0, width: 8, sem: SemTag::Revision, frozen: true },
    FreezeField { name: "framebuffer_count", offset: 8, width: 8, sem: SemTag::Count, frozen: true },
    FreezeField { name: "framebuffers", offset: 16, width: 8, sem: SemTag::Ptr, frozen: true },
];

/// framebuffer 条目（结构体）schema（Limine framebuffer 结构布局：
/// memory_address@0 / width@8 / height@16 / pitch@24 / bpp@32 /
/// memory_model@40（8B ASCII "fb"）/ red_mask_size@56 … blue_mask_shift@61
/// （各 1B）/ edid_size@64 / edid@72。掩码与 EDID 行为可演进（frozen=false），
/// 其余冻结）。
pub const FB_ENTRY_FIELDS: &[FreezeField] = &[
    FreezeField { name: "memory_address", offset: 0, width: 8, sem: SemTag::Ptr, frozen: true },
    FreezeField { name: "width", offset: 8, width: 8, sem: SemTag::Count, frozen: true },
    FreezeField { name: "height", offset: 16, width: 8, sem: SemTag::Count, frozen: true },
    FreezeField { name: "pitch", offset: 24, width: 8, sem: SemTag::Size, frozen: true },
    FreezeField { name: "bpp", offset: 32, width: 8, sem: SemTag::MaskBits, frozen: true },
    FreezeField { name: "memory_model", offset: 40, width: 8, sem: SemTag::AsciiMagic, frozen: true },
    FreezeField { name: "red_mask_size", offset: 56, width: 1, sem: SemTag::MaskBits, frozen: true },
    FreezeField { name: "red_mask_shift", offset: 57, width: 1, sem: SemTag::MaskBits, frozen: true },
    FreezeField { name: "green_mask_size", offset: 58, width: 1, sem: SemTag::MaskBits, frozen: true },
    FreezeField { name: "green_mask_shift", offset: 59, width: 1, sem: SemTag::MaskBits, frozen: true },
    FreezeField { name: "blue_mask_size", offset: 60, width: 1, sem: SemTag::MaskBits, frozen: true },
    FreezeField { name: "blue_mask_shift", offset: 61, width: 1, sem: SemTag::MaskBits, frozen: true },
    FreezeField { name: "edid_size", offset: 64, width: 8, sem: SemTag::Size, frozen: false },
    FreezeField { name: "edid", offset: 72, width: 8, sem: SemTag::Ptr, frozen: false },
];

/// kernel address 响应头冻结表（Limine kernel address response 布局：
/// revision@0 / physical_base@8 / virtual_base@16；virtual_base 落高半区）。
pub const KA_FIELDS: &[FreezeField] = &[
    FreezeField { name: "revision", offset: 0, width: 8, sem: SemTag::Revision, frozen: true },
    FreezeField { name: "physical_base", offset: 8, width: 8, sem: SemTag::PhysAddr, frozen: true },
    FreezeField { name: "virtual_base", offset: 16, width: 8, sem: SemTag::VirtAddr, frozen: true },
];

/// kernel address 无条目 schema（空表哨兵）。
pub const KA_ENTRY_FIELDS: &[FreezeField] = &[];

/// 字段冻结表（判据 J1-b）：每响应一张，版本化演进只增不改。
/// 含 Vec 字段禁 Copy——本结构零 Vec，全部 'static 切片，可 Copy。
#[derive(Clone, Copy, Debug)]
pub struct FieldFreezeTable {
    /// 所属响应臂。
    pub kind: RespKind,
    /// 冻结表版本：演进只增不改（加法走 UpgradeRegistry 登记后升版）。
    pub table_version: u32,
    /// 响应头字段行。
    pub fields: &'static [FreezeField],
    /// 条目 schema 行（KernelAddress 为空表）。
    pub entry_fields: &'static [FreezeField],
}

impl FieldFreezeTable {
    /// const 构造。
    pub const fn new(
        kind: RespKind,
        table_version: u32,
        fields: &'static [FreezeField],
        entry_fields: &'static [FreezeField],
    ) -> Self {
        FieldFreezeTable { kind, table_version, fields, entry_fields }
    }

    /// 头字段行数（const 期 while+索引，零 for）。
    pub const fn head_len(&self) -> usize {
        self.fields.len()
    }

    /// 条目 schema 行数。
    pub const fn entry_len(&self) -> usize {
        self.entry_fields.len()
    }

    /// 头冻结行数（const 期逐行扫冻结位）。
    pub const fn head_frozen_len(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.fields.len() {
            if self.fields[i].frozen {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 条目冻结行数。
    pub const fn entry_frozen_len(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.entry_fields.len() {
            if self.entry_fields[i].frozen {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 头覆盖面（最大 offset+width）：未登记加法的载荷越面检查基线。
    pub const fn head_span(&self) -> usize {
        let mut span = 0usize;
        let mut i = 0usize;
        while i < self.fields.len() {
            let end = self.fields[i].offset as usize + self.fields[i].width as usize;
            if end > span {
                span = end;
            }
            i = i.wrapping_add(1);
        }
        span
    }

    /// 条目覆盖面。
    pub const fn entry_span(&self) -> usize {
        let mut span = 0usize;
        let mut i = 0usize;
        while i < self.entry_fields.len() {
            let end = self.entry_fields[i].offset as usize + self.entry_fields[i].width as usize;
            if end > span {
                span = end;
            }
            i = i.wrapping_add(1);
        }
        span
    }

    /// 按名查头字段行下标（运行期；&str 等值比较，越界安全 while）。
    pub fn find_head_by_name(&self, name: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.fields.len() {
            if self.fields[i].name == name {
                return Some(i);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 按名查条目 schema 行下标。
    pub fn find_entry_by_name(&self, name: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entry_fields.len() {
            if self.entry_fields[i].name == name {
                return Some(i);
            }
            i = i.wrapping_add(1);
        }
        None
    }
}

/// 三臂 → 各自冻结表（const 期：match 三臂，闭集外 None）。
pub const fn table_for(kind: RespKind) -> Option<FieldFreezeTable> {
    match kind {
        RespKind::MemoryMap => Some(FieldFreezeTable::new(
            RespKind::MemoryMap,
            TABLE_VERSION_BASE,
            MM_FIELDS,
            MM_ENTRY_FIELDS,
        )),
        RespKind::Framebuffer => Some(FieldFreezeTable::new(
            RespKind::Framebuffer,
            TABLE_VERSION_BASE,
            FB_FIELDS,
            FB_ENTRY_FIELDS,
        )),
        RespKind::KernelAddress => Some(FieldFreezeTable::new(
            RespKind::KernelAddress,
            TABLE_VERSION_BASE,
            KA_FIELDS,
            KA_ENTRY_FIELDS,
        )),
    }
}

// ── S3：升级登记账（append-only，加法必须走登记） ────────────────────
/// 加法登记作用域：Head=响应头新字段；Entry=条目 schema 新字段
/// （Entry 的 offset 相对条目载荷）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddScope {
    /// 响应头新字段。
    Head,
    /// 条目 schema 新字段。
    Entry,
}

impl AddScope {
    /// 稳定名。
    pub const fn name(self) -> &'static str {
        match self {
            AddScope::Head => "head",
            AddScope::Entry => "entry",
        }
    }
}

/// 单条升级登记记录：新增字段（只增不改；登记即冻结入表，版本升位）。
#[derive(Clone, Debug)]
pub struct UpgradeRecord {
    /// 所属响应臂。
    pub kind: RespKind,
    /// 加法作用域。
    pub scope: AddScope,
    /// 新增字段名（全账唯一键）。
    pub name: String,
    /// 字节偏移（Head 相对响应头；Entry 相对条目载荷）。
    pub offset: u16,
    /// 字节宽（1..=8）。
    pub width: u16,
    /// 登记后的冻结表版本（> TABLE_VERSION_BASE）。
    pub table_version_after: u32,
    /// 登记说明（升级评估留痕）。
    pub note: String,
}

/// 升级登记账：append-only。加法被允许的对偶——新增字段允许，
/// 但必须走本登记；未登记加法在树构建期 E_UNREG_ADDITIVE 拦截。
#[derive(Clone, Debug)]
pub struct UpgradeRegistry {
    records: Vec<UpgradeRecord>,
}

impl UpgradeRegistry {
    /// 空账构造。
    pub fn new() -> Self {
        UpgradeRegistry { records: Vec::new() }
    }

    /// 已登记条数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 是否为空账。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 按下标取记录（越界安全）。
    pub fn record_at(&self, i: usize) -> Option<&UpgradeRecord> {
        if i < self.records.len() {
            Some(&self.records[i])
        } else {
            None
        }
    }

    /// 同臂同名是否已登记。
    pub fn is_registered(&self, kind: RespKind, name: &str) -> bool {
        let mut i = 0usize;
        while i < self.records.len() {
            let r = &self.records[i];
            if r.kind.code() == kind.code() && r.name == name {
                return true;
            }
            i = i.wrapping_add(1);
        }
        false
    }

    /// 该臂 Head 作用域登记后的最大覆盖面（含基线冻结表面）。
    pub fn head_registered_span(&self, kind: RespKind) -> usize {
        let mut span = 0usize;
        if let Some(t) = table_for(kind) {
            span = t.head_span();
        }
        let mut i = 0usize;
        while i < self.records.len() {
            let r = &self.records[i];
            if r.kind.code() == kind.code() && r.scope.code() == AddScope::Head.code() {
                let end = r.offset as usize + r.width as usize;
                if end > span {
                    span = end;
                }
            }
            i = i.wrapping_add(1);
        }
        span
    }

    /// 该臂 Entry 作用域登记后的最大覆盖面。
    pub fn entry_registered_span(&self, kind: RespKind) -> usize {
        let mut span = 0usize;
        if let Some(t) = table_for(kind) {
            span = t.entry_span();
        }
        let mut i = 0usize;
        while i < self.records.len() {
            let r = &self.records[i];
            if r.kind.code() == kind.code() && r.scope.code() == AddScope::Entry.code() {
                let end = r.offset as usize + r.width as usize;
                if end > span {
                    span = end;
                }
            }
            i = i.wrapping_add(1);
        }
        span
    }

    /// 该臂全部登记记录的下标遍历器（运行期收集，供树构建逐条落行）。
    pub fn records_of(&self, kind: RespKind) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        let mut i = 0usize;
        while i < self.records.len() {
            if self.records[i].kind.code() == kind.code() {
                out.push(i);
            }
            i = i.wrapping_add(1);
        }
        out
    }

    /// 登记一条加法字段。三关：宽度域关 / 名重关 / 偏移碰撞关。
    /// 登记即冻结入表（append-only），版本升位由调用方给
    /// table_version_after（须 > TABLE_VERSION_BASE）。
    pub fn register_additive(
        &mut self,
        kind: RespKind,
        scope: AddScope,
        name: &str,
        offset: u16,
        width: u16,
        table_version_after: u32,
        note: &str,
    ) -> Result<(), RespTreeError> {
        // 关一：宽度域（1..=8）。
        if width == 0 || width > 8 {
            return Err(RespTreeError::WidthBad { code: E_WIDTH_BAD, width });
        }
        // 关二：名重（全账唯一，跨臂也拒同名，杜绝歧义对账）。
        let mut i = 0usize;
        while i < self.records.len() {
            if self.records[i].name == name {
                return Err(RespTreeError::AdditiveCollision {
                    code: E_ADDITIVE_COLLISION,
                    kind: kind.code(),
                    name: name.to_string(),
                });
            }
            i = i.wrapping_add(1);
        }
        // 关三：偏移区间碰撞（与同臂同作用域既有登记行两两测交）。
        let mut j = 0usize;
        while j < self.records.len() {
            let r = &self.records[j];
            if r.kind.code() == kind.code() && r.scope.code() == scope.code() {
                let a0 = offset as u32;
                let a1 = offset as u32 + width as u32;
                let b0 = r.offset as u32;
                let b1 = r.offset as u32 + r.width as u32;
                if a0 < b1 && b0 < a1 {
                    return Err(RespTreeError::AdditiveCollision {
                        code: E_ADDITIVE_COLLISION,
                        kind: kind.code(),
                        name: name.to_string(),
                    });
                }
            }
            j = j.wrapping_add(1);
        }
        // 版本升位关：登记后版本必须大于基线版本（演进只增不改）。
        if table_version_after <= TABLE_VERSION_BASE {
            return Err(RespTreeError::AdditiveCollision {
                code: E_ADDITIVE_COLLISION,
                kind: kind.code(),
                name: name.to_string(),
            });
        }
        self.records.push(UpgradeRecord {
            kind,
            scope,
            name: name.to_string(),
            offset,
            width,
            table_version_after,
            note: note.to_string(),
        });
        Ok(())
    }
}

impl AddScope {
    /// 闭集原始字节：Head=1，Entry=2。
    pub const fn code(self) -> u8 {
        match self {
            AddScope::Head => 1,
            AddScope::Entry => 2,
        }
    }
}

// ── 错误账（RespTreeError）：每变体带 detail() 中文可操作诊断 ─────────
/// 响应树兼容账错误闭集。诊断码独占 0x1650~0x165F（段尾 0x165F 为
/// E_FIELD_UNKNOWN，段满）。
#[derive(Clone, Debug)]
pub enum RespTreeError {
    /// 账已封板仍写（0x1650）。
    Sealed { code: u16 },
    /// 响应种类越三臂闭集（0x1651）。
    KindUnknown { code: u16, raw_kind: u8 },
    /// 冻结表字段在实例中缺失（0x1652）。
    FieldMissing { code: u16, kind: u8, field: String },
    /// 字段语义档类型不符（0x1653）。
    TypeMismatch { code: u16, kind: u8, field: String, observed: u64 },
    /// 魔法数/闭集不符（0x1654）。
    MagicMismatch { code: u16, kind: u8, field: String, observed: u64 },
    /// 冻结表/基线树缺席（0x1655）。
    TableAbsent { code: u16, kind: u8 },
    /// 同种响应重复注册冻结表（0x1656）。
    DupTable { code: u16, kind: u8 },
    /// 加法登记名重/偏移碰撞（0x1657）。
    AdditiveCollision { code: u16, kind: u8, name: String },
    /// 未登记加法：载荷越冻结表覆盖面（0x1658）。
    UnregAdditive { code: u16, kind: u8, observed_len: usize, allowed_span: usize },
    /// 字段宽度非法（0x1659）。
    WidthBad { code: u16, width: u16 },
    /// 载荷短于字段偏移+宽度（0x165A）。
    PayloadShort { code: u16, kind: u8, need: usize, have: usize },
    /// 条目 schema 交叉核破（0x165B）。
    EntrySchemaFail { code: u16, kind: u8, entry_index: usize, detail: String },
    /// 核对率不足 100%（0x165C）。
    RateNotFull { code: u16, kind: u8, rate_ppm: u32 },
    /// 双跑对拍指纹漂移（0x165D）。
    ReplayDrift { code: u16, fp_a: u64, fp_b: u64 },
    /// 事件账容量满（0x165E）。
    Capacity { code: u16, cap: usize },
    /// 按名查字段不中（0x165F）。
    FieldUnknown { code: u16, kind: u8, field: String },
}

impl RespTreeError {
    /// 诊断码（独占段 0x1650~0x165F）。const 期 match，零比较符陷阱。
    pub const fn diag_code(&self) -> u16 {
        match self {
            RespTreeError::Sealed { code } => *code,
            RespTreeError::KindUnknown { code, .. } => *code,
            RespTreeError::FieldMissing { code, .. } => *code,
            RespTreeError::TypeMismatch { code, .. } => *code,
            RespTreeError::MagicMismatch { code, .. } => *code,
            RespTreeError::TableAbsent { code, .. } => *code,
            RespTreeError::DupTable { code, .. } => *code,
            RespTreeError::AdditiveCollision { code, .. } => *code,
            RespTreeError::UnregAdditive { code, .. } => *code,
            RespTreeError::WidthBad { code, .. } => *code,
            RespTreeError::PayloadShort { code, .. } => *code,
            RespTreeError::EntrySchemaFail { code, .. } => *code,
            RespTreeError::RateNotFull { code, .. } => *code,
            RespTreeError::ReplayDrift { code, .. } => *code,
            RespTreeError::Capacity { code, .. } => *code,
            RespTreeError::FieldUnknown { code, .. } => *code,
        }
    }

    /// 中文可操作诊断（'static）。
    pub fn detail(&self) -> &'static str {
        diag_detail(self.diag_code())
    }
}

// ── 小端读写原语（显性定宽，越界安全返回，零 panic 路径） ────────────
/// 小端读 1..=8 字节为 u64；越界/零宽/超宽返回 None（调用方落 Missing）。
fn read_u64_le(buf: &[u8], off: usize, width: usize) -> Option<u64> {
    if width == 0 || width > 8 {
        return None;
    }
    let end = match off.checked_add(width) {
        Some(e) => e,
        None => return None,
    };
    if end > buf.len() {
        return None;
    }
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < width {
        v |= (buf[off + i] as u64) << (8 * i);
        i += 1;
    }
    Some(v)
}

/// 小端写 u64 的低 width 字节；越界/零宽/超宽返回 false（零 panic 路径）。
fn write_u64_le(buf: &mut [u8], off: usize, width: usize, val: u64) -> bool {
    if width == 0 || width > 8 {
        return false;
    }
    let end = match off.checked_add(width) {
        Some(e) => e,
        None => return false,
    };
    if end > buf.len() {
        return false;
    }
    let mut i = 0usize;
    while i < width {
        buf[off + i] = ((val >> (8 * i)) & 0xFF) as u8;
        i += 1;
    }
    true
}

// ── 响应实例：原始字节面（核对与注入的对象） ─────────────────────────
/// 响应实例原始字节面：响应头 + 可选条目载荷集
/// （memory map → 区描述条目；framebuffer → framebuffer 结构条目；
/// kernel address → 零条目）。
#[derive(Clone, Debug)]
pub struct RawResponse {
    /// 响应臂（闭集类型化，杜绝野种类）。
    pub kind: RespKind,
    /// 响应头原始字节（小端）。
    pub head: Vec<u8>,
    /// 条目载荷集（逐条目小端字节）。
    pub entries: Vec<Vec<u8>>,
}

impl RawResponse {
    /// 构造：口径关（头 ≤ PAYLOAD_CAP、条目数 ≤ COUNT_CAP、
    /// 单条目 ≤ ENTRY_CAP），越面即 E_CAPACITY 拒收。
    pub fn build(kind: RespKind, head: &[u8], entries: &[&[u8]]) -> Result<Self, RespTreeError> {
        if head.len() > PAYLOAD_CAP {
            return Err(RespTreeError::Capacity { code: E_CAPACITY, cap: PAYLOAD_CAP });
        }
        if entries.len() as u64 > COUNT_CAP {
            return Err(RespTreeError::Capacity { code: E_CAPACITY, cap: COUNT_CAP as usize });
        }
        let mut owned: Vec<Vec<u8>> = Vec::new();
        let mut i = 0usize;
        while i < entries.len() {
            if entries[i].len() > ENTRY_CAP {
                return Err(RespTreeError::Capacity { code: E_CAPACITY, cap: ENTRY_CAP });
            }
            owned.push(entries[i].to_vec());
            i += 1;
        }
        Ok(RawResponse { kind, head: head.to_vec(), entries: owned })
    }

    /// 头字节数。
    pub fn head_len(&self) -> usize {
        self.head.len()
    }

    /// 条目数。
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

// ── S4：逐字段核对器（判据 J1-c：三拒 + 100% 核对率） ────────────────
/// 核对作用域：Head=响应头字段；Entry(i)=第 i 条目 schema 字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckScope {
    /// 响应头字段。
    Head,
    /// 第 i 条目字段。
    Entry(usize),
}

impl CheckScope {
    /// 稳定名（快照序列化用）。
    pub const fn name(self) -> &'static str {
        match self {
            CheckScope::Head => "head",
            CheckScope::Entry(_) => "entry",
        }
    }

    /// 序号：Head=0，Entry(i)=i+1（序列化用；0 保留给 Head）。
    pub const fn index_code(self) -> u32 {
        match self {
            CheckScope::Head => 0,
            CheckScope::Entry(i) => (i as u32).wrapping_add(1),
        }
    }
}

/// 作用域等值（match 口径，不用 derive 类型比较符）。
fn scope_eq(a: CheckScope, b: CheckScope) -> bool {
    match (a, b) {
        (CheckScope::Head, CheckScope::Head) => true,
        (CheckScope::Entry(x), CheckScope::Entry(y)) => x == y,
        _ => false,
    }
}

/// 单字段核对结论。三拒 = Missing / TypeMismatch / MagicMismatch；
/// SchemaFail 为交叉核（条目数 ↔ 计数字段）专档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckVerdict {
    /// 过。
    Pass,
    /// 字段缺失（E_FIELD_MISSING）。
    Missing,
    /// 类型不符（E_TYPE_MISMATCH）。
    TypeMismatch,
    /// 魔法数不符（E_MAGIC_MISMATCH）。
    MagicMismatch,
    /// 交叉核破（E_ENTRY_SCHEMA_FAIL）。
    SchemaFail,
}

impl CheckVerdict {
    /// → 诊断码。
    pub const fn diag_code(self) -> u16 {
        match self {
            CheckVerdict::Pass => 0,
            CheckVerdict::Missing => E_FIELD_MISSING,
            CheckVerdict::TypeMismatch => E_TYPE_MISMATCH,
            CheckVerdict::MagicMismatch => E_MAGIC_MISMATCH,
            CheckVerdict::SchemaFail => E_ENTRY_SCHEMA_FAIL,
        }
    }

    /// 是否过。
    pub const fn is_pass(self) -> bool {
        match self {
            CheckVerdict::Pass => true,
            _ => false,
        }
    }
}

/// 单字段核对留痕：逐字段（名称/偏移/宽/冻结位/观测值/结论）。
#[derive(Clone, Debug)]
pub struct FieldCheck {
    /// 响应臂。
    pub kind: RespKind,
    /// 核对作用域。
    pub scope: CheckScope,
    /// 字段名（交叉核行为合成名，如 "entry_count(cross)"）。
    pub field: String,
    /// 字节偏移（交叉核行为 0 哨兵）。
    pub offset: u16,
    /// 字节宽。
    pub width: u16,
    /// 冻结位（交叉核行恒 true：交叉核属冻结面守卫）。
    pub frozen: bool,
    /// 观测值（Missing 时 None）。
    pub observed: Option<u64>,
    /// 核对结论。
    pub verdict: CheckVerdict,
}

/// 核对报告：逐字段留痕 + 冻结面核对率（ppm 整数制，10000=100%）。
#[derive(Clone, Debug)]
pub struct VerifyReport {
    /// 响应臂。
    pub kind: RespKind,
    /// 使用的冻结表版本。
    pub table_version: u32,
    /// 逐字段核对留痕。
    pub checks: Vec<FieldCheck>,
    /// 冻结面总行数（头冻结行 + 条目冻结行×条目数 + 交叉核守卫行）。
    pub frozen_total: usize,
    /// 冻结面通过行数。
    pub frozen_pass: usize,
    /// 全部行（含可演进行）是否全过。
    pub all_pass: bool,
    /// 核对率（ppm；10000 = 100%）。
    pub rate_ppm: u32,
    /// 首个失败诊断码（全过为 None）。
    pub failure_code: Option<u16>,
}

impl VerifyReport {
    /// 判据 J1-c 口径：核对率恰 100%（10000 ppm）且全行通过。
    pub fn is_full(&self) -> bool {
        self.rate_ppm == PPM_SCALE as u32 && self.all_pass
    }

    /// 失败行数。
    pub fn failing_len(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.checks.len() {
            if !self.checks[i].verdict.is_pass() {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// 单字段核对原语：读 → 语义档核 → 结论（越界安全，零 panic 路径）。
fn check_one(
    buf: &[u8],
    kind: RespKind,
    scope: CheckScope,
    field: &str,
    offset: u16,
    width: u16,
    sem: SemTag,
    frozen: bool,
) -> FieldCheck {
    let w = width as usize;
    let o = offset as usize;
    let end = o.checked_add(w);
    let have = match end {
        Some(e) => e <= buf.len(),
        None => false,
    };
    let missing = FieldCheck {
        kind,
        scope,
        field: field.to_string(),
        offset,
        width,
        frozen,
        observed: None,
        verdict: CheckVerdict::Missing,
    };
    if !have {
        return missing;
    }
    // 界已检，读必成；仍以 match 收口（零 unwrap 纪律）。
    let v = match read_u64_le(buf, o, w) {
        Some(v) => v,
        None => return missing,
    };
    let verdict = match sem.check(v) {
        CheckOutcome::Plausible => CheckVerdict::Pass,
        CheckOutcome::TypeMismatch => CheckVerdict::TypeMismatch,
        CheckOutcome::MagicMismatch => CheckVerdict::MagicMismatch,
    };
    FieldCheck {
        kind,
        scope,
        field: field.to_string(),
        offset,
        width,
        frozen,
        observed: Some(v),
        verdict,
    }
}

/// 逐字段核对器（无状态核面，判据 J1-c 主入口）。
pub struct RespTreeVerifier;

impl RespTreeVerifier {
    /// 核对响应实例 vs 该臂冻结表：
    ///   一拒字段缺失（偏移+宽越载荷 → E_FIELD_MISSING）；
    ///   二拒类型不符（语义档取值域破 → E_TYPE_MISMATCH）；
    ///   三拒魔法数不符（revision 越上限 / "fb" ASCII 魔法破 /
    ///   闭集枚举越 0..=6 / 高半区哨兵破 → E_MAGIC_MISMATCH）；
    ///   加一道交叉核：计数字段 ↔ 实际条目数（E_ENTRY_SCHEMA_FAIL）。
    /// Err 仅限冻结表结构损坏（宽度域破）；核对失败以 report 承载。
    pub fn verify(resp: &RawResponse) -> Result<VerifyReport, RespTreeError> {
        let table = match table_for(resp.kind) {
            Some(t) => t,
            None => {
                return Err(RespTreeError::KindUnknown {
                    code: E_KIND_UNKNOWN,
                    raw_kind: 0,
                })
            }
        };
        // 表结构自检：所有行宽 1..=8（const 表损坏防线）。
        let mut i = 0usize;
        while i < table.fields.len() {
            let w = table.fields[i].width;
            if w == 0 || w > 8 {
                return Err(RespTreeError::WidthBad { code: E_WIDTH_BAD, width: w });
            }
            i += 1;
        }
        let mut i = 0usize;
        while i < table.entry_fields.len() {
            let w = table.entry_fields[i].width;
            if w == 0 || w > 8 {
                return Err(RespTreeError::WidthBad { code: E_WIDTH_BAD, width: w });
            }
            i += 1;
        }

        let mut checks: Vec<FieldCheck> = Vec::new();
        let mut failure_code: Option<u16> = None;
        let mut frozen_total = 0usize;
        let mut frozen_pass = 0usize;
        let mut all_pass = true;
        let mut count_observed: Option<u64> = None;

        // 头字段逐行核。
        let mut i = 0usize;
        while i < table.fields.len() {
            let f = table.fields[i];
            if f.frozen {
                frozen_total += 1;
            }
            let c = check_one(
                &resp.head,
                resp.kind,
                CheckScope::Head,
                f.name,
                f.offset,
                f.width,
                f.sem,
                f.frozen,
            );
            if f.name == "entry_count" || f.name == "framebuffer_count" {
                count_observed = c.observed;
            }
            if c.verdict.is_pass() {
                if f.frozen {
                    frozen_pass += 1;
                }
            } else {
                all_pass = false;
                if failure_code.is_none() {
                    failure_code = Some(c.verdict.diag_code());
                }
            }
            checks.push(c);
            i += 1;
        }

        // 交叉核：计数字段 ↔ 实际条目数（memory map / framebuffer）。
        match resp.kind {
            RespKind::MemoryMap | RespKind::Framebuffer => {
                frozen_total += 1; // 交叉核守卫行计入冻结面
                let expect = count_observed.unwrap_or(u64::MAX); // saturating 哨兵：缺失必破
                if expect == resp.entries.len() as u64 {
                    frozen_pass += 1;
                } else {
                    all_pass = false;
                    if failure_code.is_none() {
                        failure_code = Some(E_ENTRY_SCHEMA_FAIL);
                    }
                }
                checks.push(FieldCheck {
                    kind: resp.kind,
                    scope: CheckScope::Head,
                    field: "entry_count(cross)".to_string(),
                    offset: 0,
                    width: 0,
                    frozen: true,
                    observed: Some(expect),
                    verdict: if expect == resp.entries.len() as u64 {
                        CheckVerdict::Pass
                    } else {
                        CheckVerdict::SchemaFail
                    },
                });
            }
            RespKind::KernelAddress => {
                if !resp.entries.is_empty() {
                    all_pass = false;
                    if failure_code.is_none() {
                        failure_code = Some(E_ENTRY_SCHEMA_FAIL);
                    }
                    checks.push(FieldCheck {
                        kind: resp.kind,
                        scope: CheckScope::Head,
                        field: "entries(must_be_empty)".to_string(),
                        offset: 0,
                        width: 0,
                        frozen: true,
                        observed: Some(resp.entries.len() as u64),
                        verdict: CheckVerdict::SchemaFail,
                    });
                }
            }
        }

        // 条目 schema 逐条目逐行核。
        let mut e = 0usize;
        while e < resp.entries.len() {
            let entry = &resp.entries[e];
            let mut k = 0usize;
            while k < table.entry_fields.len() {
                let f = table.entry_fields[k];
                if f.frozen {
                    frozen_total += 1;
                }
                let c = check_one(
                    entry,
                    resp.kind,
                    CheckScope::Entry(e),
                    f.name,
                    f.offset,
                    f.width,
                    f.sem,
                    f.frozen,
                );
                if c.verdict.is_pass() {
                    if f.frozen {
                        frozen_pass += 1;
                    }
                } else {
                    all_pass = false;
                    if failure_code.is_none() {
                        failure_code = Some(c.verdict.diag_code());
                    }
                }
                checks.push(c);
                k += 1;
            }
            e += 1;
        }

        // 核对率（ppm 整数制；frozen_total 恒 > 0——三臂表均有冻结行+交叉核）。
        let rate = (frozen_pass as u64)
            .saturating_mul(PPM_SCALE)
            .saturating_div(frozen_total.max(1) as u64);
        Ok(VerifyReport {
            kind: resp.kind,
            table_version: table.table_version,
            checks,
            frozen_total,
            frozen_pass,
            all_pass,
            rate_ppm: rate as u32,
            failure_code,
        })
    }
}

// ── FNV-1a 64（指纹基元，const 期 while+索引） ───────────────────────
/// FNV-1a 64 位哈希（偏移基 0xCBF29CE484222325，素数 0x100000001B3；
/// FNV 参考实现口径）。确定性对拍与账级指纹共用。
pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

// ── S5：响应树快照（确定性序列化 + 指纹） ────────────────────────────
/// 快照行：某臂某作用域某字段的冻结值留痕（对账键 = kind+scope+name）。
#[derive(Clone, Debug)]
pub struct SnapshotRow {
    /// 响应臂。
    pub kind: RespKind,
    /// 该臂冻结表版本（登记加法行取登记后版本）。
    pub table_version: u32,
    /// 作用域。
    pub scope: CheckScope,
    /// 字段名。
    pub name: String,
    /// 观测值（小端读出）。
    pub value: u64,
    /// 冻结位。
    pub frozen: bool,
}

/// 响应树快照：三臂全部核对通过行的有序集合 + FNV 指纹。
/// 同输入双跑行序确定 → 指纹必等（判据「双跑一致性」）。
#[derive(Clone, Debug)]
pub struct RespTree {
    /// 快照行（行序：臂码升序 → 表行序 → 登记行序 → 条目序）。
    pub rows: Vec<SnapshotRow>,
    /// FNV-1a 64 指纹（对确定性序列化字节）。
    pub fingerprint: u64,
}

/// 确定性序列化：行序固定 + 字段定宽（臂码 1B + 版本 4B LE +
/// 作用域码 1B + 序号 4B LE + 名长 2B LE + 名字节 + 值 8B LE）。
fn canonical_bytes(rows: &[SnapshotRow]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < rows.len() {
        let r = &rows[i];
        out.push(r.kind.code());
        out.extend_from_slice(&r.table_version.to_le_bytes());
        out.push(match r.scope {
            CheckScope::Head => 0u8,
            CheckScope::Entry(_) => 1u8,
        });
        out.extend_from_slice(&r.scope.index_code().to_le_bytes());
        let nb = r.name.as_bytes();
        out.extend_from_slice(&(nb.len() as u16).to_le_bytes());
        out.extend_from_slice(nb);
        out.extend_from_slice(&r.value.to_le_bytes());
        i += 1;
    }
    out
}

/// 从核对报告收集快照行（仅 Pass 行）。
fn rows_from_report(report: &VerifyReport) -> Vec<SnapshotRow> {
    let mut rows: Vec<SnapshotRow> = Vec::new();
    let mut i = 0usize;
    while i < report.checks.len() {
        let c = &report.checks[i];
        if c.verdict.is_pass() {
            if let Some(v) = c.observed {
                rows.push(SnapshotRow {
                    kind: report.kind,
                    table_version: report.table_version,
                    scope: c.scope,
                    name: c.field.clone(),
                    value: v,
                    frozen: c.frozen,
                });
            }
        }
        i += 1;
    }
    rows
}

/// 登记加法行落树：从原始载荷按登记行读值（越面由覆盖关前置拦截；
/// 缺席行跳过——缺席即基线无此字段，diff 归因接管）。
fn rows_from_registry(
    kind: RespKind,
    raw: &RawResponse,
    registry: &UpgradeRegistry,
) -> Vec<SnapshotRow> {
    let mut rows: Vec<SnapshotRow> = Vec::new();
    let idxs = registry.records_of(kind);
    let mut r = 0usize;
    while r < idxs.len() {
        let rec = match registry.record_at(idxs[r]) {
            Some(x) => x,
            None => {
                r += 1;
                continue;
            }
        };
        match rec.scope {
            AddScope::Head => {
                if let Some(v) = read_u64_le(&raw.head, rec.offset as usize, rec.width as usize) {
                    rows.push(SnapshotRow {
                        kind,
                        table_version: rec.table_version_after,
                        scope: CheckScope::Head,
                        name: rec.name.clone(),
                        value: v,
                        frozen: true, // 登记即冻结入表
                    });
                }
            }
            AddScope::Entry => {
                let mut e = 0usize;
                while e < raw.entries.len() {
                    if let Some(v) =
                        read_u64_le(&raw.entries[e], rec.offset as usize, rec.width as usize)
                    {
                        rows.push(SnapshotRow {
                            kind,
                            table_version: rec.table_version_after,
                            scope: CheckScope::Entry(e),
                            name: rec.name.clone(),
                            value: v,
                            frozen: true,
                        });
                    }
                    e += 1;
                }
            }
        }
        r += 1;
    }
    rows
}

/// 覆盖关：载荷字节越冻结表面 + 登记面 → 未登记加法（E_UNREG_ADDITIVE）。
fn coverage_gate(
    kind: RespKind,
    raw: &RawResponse,
    registry: &UpgradeRegistry,
) -> Result<(), RespTreeError> {
    let head_allowed = registry.head_registered_span(kind);
    if raw.head.len() > head_allowed {
        return Err(RespTreeError::UnregAdditive {
            code: E_UNREG_ADDITIVE,
            kind: kind.code(),
            observed_len: raw.head.len(),
            allowed_span: head_allowed,
        });
    }
    let entry_allowed = registry.entry_registered_span(kind);
    let mut e = 0usize;
    while e < raw.entries.len() {
        if raw.entries[e].len() > entry_allowed {
            return Err(RespTreeError::UnregAdditive {
                code: E_UNREG_ADDITIVE,
                kind: kind.code(),
                observed_len: raw.entries[e].len(),
                allowed_span: entry_allowed,
            });
        }
        e += 1;
    }
    Ok(())
}

/// 响应树快照构建（判据 J1-c 收口：三臂全 100% 才出树）。
/// 入参：三臂核对报告 + 对应原始实例 + 登记账。
pub fn build_resp_tree(
    reports: &[VerifyReport],
    raws: &[&RawResponse],
    registry: &UpgradeRegistry,
) -> Result<RespTree, RespTreeError> {
    let mut rows: Vec<SnapshotRow> = Vec::new();
    let mut idx = 0usize;
    while idx < KIND_COUNT {
        let kind = match RespKind::from_index(idx) {
            Some(k) => k,
            None => {
                return Err(RespTreeError::KindUnknown {
                    code: E_KIND_UNKNOWN,
                    raw_kind: idx as u8,
                })
            }
        };
        // 报告在场关。
        let mut rep: Option<&VerifyReport> = None;
        let mut i = 0usize;
        while i < reports.len() {
            if reports[i].kind.code() == kind.code() {
                rep = Some(&reports[i]);
            }
            i += 1;
        }
        let report = match rep {
            Some(r) => r,
            None => {
                return Err(RespTreeError::TableAbsent { code: E_TABLE_ABSENT, kind: kind.code() })
            }
        };
        if !report.is_full() {
            return Err(RespTreeError::RateNotFull {
                code: E_RATE_NOT_FULL,
                kind: kind.code(),
                rate_ppm: report.rate_ppm,
            });
        }
        // 原始实例在场关 + 覆盖关（未登记加法拦截）。
        let mut raw_found: Option<&RawResponse> = None;
        let mut i = 0usize;
        while i < raws.len() {
            if raws[i].kind.code() == kind.code() {
                raw_found = Some(raws[i]);
            }
            i += 1;
        }
        let raw = match raw_found {
            Some(r) => r,
            None => {
                return Err(RespTreeError::TableAbsent { code: E_TABLE_ABSENT, kind: kind.code() })
            }
        };
        coverage_gate(kind, raw, registry)?;
        // 落行：表核对行 → 登记加法行。
        let mut batch = rows_from_report(report);
        rows.append(&mut batch);
        let mut reg_rows = rows_from_registry(kind, raw, registry);
        rows.append(&mut reg_rows);
        idx += 1;
    }
    let cb = canonical_bytes(&rows);
    Ok(RespTree { rows, fingerprint: fnv1a64(&cb) })
}

impl RespTree {
    /// 行数。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 指纹复算一致性（树体 vs 存档指纹）。
    pub fn fingerprint_consistent(&self) -> bool {
        self.fingerprint == fnv1a64(&canonical_bytes(&self.rows))
    }

    /// 单臂行替换（注入面复树用）：以新核对报告的 Pass 行替换该臂旧行，
    /// 登记行按现原始实例重落；须 is_full，否则 E_RATE_NOT_FULL。
    pub fn with_report_rows(
        &self,
        report: &VerifyReport,
        registry: &UpgradeRegistry,
        raw_for_kind: &RawResponse,
    ) -> Result<RespTree, RespTreeError> {
        if !report.is_full() {
            return Err(RespTreeError::RateNotFull {
                code: E_RATE_NOT_FULL,
                kind: report.kind.code(),
                rate_ppm: report.rate_ppm,
            });
        }
        coverage_gate(report.kind, raw_for_kind, registry)?;
        let mut rows: Vec<SnapshotRow> = Vec::new();
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].kind.code() != report.kind.code() {
                rows.push(self.rows[i].clone());
            }
            i += 1;
        }
        let mut batch = rows_from_report(report);
        rows.append(&mut batch);
        let mut reg_rows = rows_from_registry(report.kind, raw_for_kind, registry);
        rows.append(&mut reg_rows);
        let cb = canonical_bytes(&rows);
        Ok(RespTree { rows, fingerprint: fnv1a64(&cb) })
    }

    /// 树指纹等值 + 行数等值（双跑对拍口径）。
    pub fn same_as(&self, other: &RespTree) -> bool {
        self.fingerprint == other.fingerprint && self.rows.len() == other.rows.len()
    }
}

// ── S6：双树 diff 与兼容三态（判据 J1-d） ────────────────────────────
/// 兼容三态闭集：兼容 / 仅加法兼容 / 破坏兼容（破坏即拦截立案）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompatVerdict {
    /// 兼容：逐字段零差异。
    Compatible,
    /// 仅加法兼容：登记加法与可演进漂移，冻结面零触碰。
    AdditiveOnly,
    /// 破坏兼容：冻结面被触碰或未登记加法——拦截立案。
    Breaking,
}

impl CompatVerdict {
    /// 严重度秩（overall 取最大秩）。
    pub const fn rank(self) -> u8 {
        match self {
            CompatVerdict::Compatible => 0,
            CompatVerdict::AdditiveOnly => 1,
            CompatVerdict::Breaking => 2,
        }
    }

    /// 稳定名。
    pub const fn name(self) -> &'static str {
        match self {
            CompatVerdict::Compatible => "compatible",
            CompatVerdict::AdditiveOnly => "additive_only",
            CompatVerdict::Breaking => "breaking",
        }
    }
}

/// 差异归因闭集（判据「差异逐项归因」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffCause {
    /// 冻结字段值漂移（破坏）。
    FrozenDrift,
    /// 冻结字段升级后缺失（破坏）。
    FrozenMissingAfter,
    /// 新字段未登记（破坏——加法必须走升级登记）。
    UnregAdditive,
    /// 新字段已登记（仅加法兼容）。
    RegisteredAdditive,
    /// 可演进字段值漂移（仅加法兼容，归因留痕）。
    EvolvableDrift,
    /// 可演进字段升级后消失（仅加法兼容，归因留痕）。
    OnlyInBefore,
}

impl DiffCause {
    /// 是否破坏性归因。
    pub const fn is_breaking(self) -> bool {
        match self {
            DiffCause::FrozenDrift => true,
            DiffCause::FrozenMissingAfter => true,
            DiffCause::UnregAdditive => true,
            DiffCause::RegisteredAdditive => false,
            DiffCause::EvolvableDrift => false,
            DiffCause::OnlyInBefore => false,
        }
    }

    /// 稳定名。
    pub const fn name(self) -> &'static str {
        match self {
            DiffCause::FrozenDrift => "frozen_drift",
            DiffCause::FrozenMissingAfter => "frozen_missing_after",
            DiffCause::UnregAdditive => "unregistered_additive",
            DiffCause::RegisteredAdditive => "registered_additive",
            DiffCause::EvolvableDrift => "evolvable_drift",
            DiffCause::OnlyInBefore => "only_in_before",
        }
    }
}

/// 单字段差异：before/after 观测值 + 归因。
#[derive(Clone, Debug)]
pub struct FieldDiff {
    /// 响应臂。
    pub kind: RespKind,
    /// 作用域。
    pub scope: CheckScope,
    /// 字段名（全臂缺失归因行为合成名 "*tree*"）。
    pub field: String,
    /// 升级前值（新字段 None）。
    pub before: Option<u64>,
    /// 升级后值（消失字段 None）。
    pub after: Option<u64>,
    /// 冻结位。
    pub frozen: bool,
    /// 归因。
    pub cause: DiffCause,
}

/// 单臂兼容判定。
#[derive(Clone, Debug)]
pub struct KindCompat {
    /// 响应臂。
    pub kind: RespKind,
    /// 三态结论。
    pub verdict: CompatVerdict,
    /// 逐字段差异清单。
    pub diffs: Vec<FieldDiff>,
    /// 升级前行数。
    pub rows_before: usize,
    /// 升级后行数。
    pub rows_after: usize,
}

/// 全树兼容判定（三臂汇总 + overall 取最大秩 + 立案/放行文案）。
#[derive(Clone, Debug)]
pub struct CompatAssessment {
    /// 三臂判定。
    pub kinds: Vec<KindCompat>,
    /// 总结论（最大秩）。
    pub overall: CompatVerdict,
    /// 破坏性差异项数。
    pub breaking_diffs: usize,
    /// 加法性差异项数。
    pub additive_diffs: usize,
    /// 是否拦截（Breaking 即拦截立案）。
    pub blocked: bool,
    /// 立案/放行文案。
    pub case_note: String,
}

/// 在行集中按（作用域, 字段名）找行。
fn find_row<'a>(
    rows: &[&'a SnapshotRow],
    scope: CheckScope,
    name: &str,
) -> Option<&'a SnapshotRow> {
    let mut i = 0usize;
    while i < rows.len() {
        if scope_eq(rows[i].scope, scope) && rows[i].name == name {
            return Some(rows[i]);
        }
        i += 1;
    }
    None
}

/// 双树 diff + 兼容三态判定（判据 J1-d 主入口）。
/// 规则：
///   R1 after 行在 before 无对应 → 已登记 → RegisteredAdditive（加法）；
///                                  未登记 → UnregAdditive（破坏）。
///   R2 两边同字段值不等 → 冻结 → FrozenDrift（破坏）；
///                        可演进 → EvolvableDrift（加法）。
///   R3 before 行在 after 无对应 → 冻结 → FrozenMissingAfter（破坏）；
///                                  可演进 → OnlyInBefore（加法）。
///   R4 整臂在 after 缺行（before 有行）→ 臂级缺失（破坏）。
pub fn classify_compat(
    before: &RespTree,
    after: &RespTree,
    registry: &UpgradeRegistry,
) -> CompatAssessment {
    let mut kinds: Vec<KindCompat> = Vec::new();
    let mut breaking_diffs = 0usize;
    let mut additive_diffs = 0usize;
    let mut overall_rank = 0u8;

    let mut idx = 0usize;
    while idx < KIND_COUNT {
        let kind = match RespKind::from_index(idx) {
            Some(k) => k,
            None => {
                idx += 1;
                continue;
            }
        };
        let mut rows_b: Vec<&SnapshotRow> = Vec::new();
        let mut rows_a: Vec<&SnapshotRow> = Vec::new();
        let mut i = 0usize;
        while i < before.rows.len() {
            if before.rows[i].kind.code() == kind.code() {
                rows_b.push(&before.rows[i]);
            }
            i += 1;
        }
        let mut i = 0usize;
        while i < after.rows.len() {
            if after.rows[i].kind.code() == kind.code() {
                rows_a.push(&after.rows[i]);
            }
            i += 1;
        }

        let mut diffs: Vec<FieldDiff> = Vec::new();

        // R4：整臂缺失（before 有行而 after 全无）。
        if rows_a.is_empty() && !rows_b.is_empty() {
            diffs.push(FieldDiff {
                kind,
                scope: CheckScope::Head,
                field: "*tree*".to_string(),
                before: Some(rows_b.len() as u64),
                after: Some(0),
                frozen: true,
                cause: DiffCause::FrozenMissingAfter,
            });
        }

        // R1/R2：after 逐行对账。
        let mut i = 0usize;
        while i < rows_a.len() {
            let ra = rows_a[i];
            let hit = find_row(&rows_b, ra.scope, &ra.name);
            match hit {
                Some(rb) => {
                    if rb.value != ra.value {
                        let cause = if ra.frozen {
                            DiffCause::FrozenDrift
                        } else {
                            DiffCause::EvolvableDrift
                        };
                        diffs.push(FieldDiff {
                            kind,
                            scope: ra.scope,
                            field: ra.name.clone(),
                            before: Some(rb.value),
                            after: Some(ra.value),
                            frozen: ra.frozen,
                            cause,
                        });
                    }
                }
                None => {
                    let registered = registry.is_registered(kind, &ra.name);
                    diffs.push(FieldDiff {
                        kind,
                        scope: ra.scope,
                        field: ra.name.clone(),
                        before: None,
                        after: Some(ra.value),
                        frozen: ra.frozen,
                        cause: if registered {
                            DiffCause::RegisteredAdditive
                        } else {
                            DiffCause::UnregAdditive
                        },
                    });
                }
            }
            i += 1;
        }

        // R3：before 逐行回查（消失归因）。
        let mut i = 0usize;
        while i < rows_b.len() {
            let rb = rows_b[i];
            if find_row(&rows_a, rb.scope, &rb.name).is_none() {
                diffs.push(FieldDiff {
                    kind,
                    scope: rb.scope,
                    field: rb.name.clone(),
                    before: Some(rb.value),
                    after: None,
                    frozen: rb.frozen,
                    cause: if rb.frozen {
                        DiffCause::FrozenMissingAfter
                    } else {
                        DiffCause::OnlyInBefore
                    },
                });
            }
            i += 1;
        }

        // 臂级三态。
        let mut verdict = CompatVerdict::Compatible;
        let mut d = 0usize;
        while d < diffs.len() {
            if diffs[d].cause.is_breaking() {
                breaking_diffs += 1;
                verdict = CompatVerdict::Breaking;
            } else {
                additive_diffs += 1;
                if verdict.rank() < CompatVerdict::AdditiveOnly.rank() {
                    verdict = CompatVerdict::AdditiveOnly;
                }
            }
            d += 1;
        }
        if verdict.rank() > overall_rank {
            overall_rank = verdict.rank();
        }
        kinds.push(KindCompat {
            kind,
            verdict,
            diffs,
            rows_before: rows_b.len(),
            rows_after: rows_a.len(),
        });
        idx += 1;
    }

    let overall = match overall_rank {
        0 => CompatVerdict::Compatible,
        1 => CompatVerdict::AdditiveOnly,
        _ => CompatVerdict::Breaking,
    };
    let blocked = overall == CompatVerdict::Breaking;
    let case_note = if blocked {
        format!(
            "拦截立案：升级响应树破坏兼容（{} 项破坏性差异）——冻结面被触碰或未登记加法，放行冻结，须回退升级",
            breaking_diffs
        )
    } else {
        format!(
            "放行：升级响应树结论 {}（加法性差异 {} 项，冻结面零触碰）",
            overall.name(),
            additive_diffs
        )
    };
    CompatAssessment {
        kinds,
        overall,
        breaking_diffs,
        additive_diffs,
        blocked,
        case_note,
    }
}

// ── 双跑确定性对拍（判据「双跑一致性」的对偶口径） ───────────────────
/// 双跑对拍报告。
#[derive(Clone, Debug)]
pub struct DualRunReport {
    /// 第一跑指纹。
    pub fingerprint_a: u64,
    /// 第二跑指纹。
    pub fingerprint_b: u64,
    /// 是否一致。
    pub identical: bool,
}

/// 双跑对拍：同输入两树指纹与行数必须全等；漂移即 E_REPLAY_DRIFT。
pub fn dual_run_compare(a: &RespTree, b: &RespTree) -> Result<DualRunReport, RespTreeError> {
    let identical = a.same_as(b);
    if identical {
        Ok(DualRunReport {
            fingerprint_a: a.fingerprint,
            fingerprint_b: b.fingerprint,
            identical: true,
        })
    } else {
        Err(RespTreeError::ReplayDrift {
            code: E_REPLAY_DRIFT,
            fp_a: a.fingerprint,
            fp_b: b.fingerprint,
        })
    }
}

// ── S7：事件账（封板制）+ health 绿灯 ────────────────────────────────
/// 账内事件闭集（子码 0x01..=0x0B，非诊断码；诊断码走 diag 字段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvKind {
    /// 实例核对通过入账。
    InstanceAccepted,
    /// 实例核对被拒（三拒之一或交叉核破）。
    VerifyRejected,
    /// 基线树冻结落档。
    BaselineFrozen,
    /// 加法字段升级登记落账。
    AdditiveRegistered,
    /// 兼容判定：Compatible。
    CompatClean,
    /// 兼容判定：AdditiveOnly（放行）。
    CompatAdditive,
    /// 兼容判定：Breaking（拦截立案）。
    CompatBlocked,
    /// 注入检出留痕。
    InjectionDetected,
    /// 双跑对拍一致。
    DualRunOk,
    /// 双跑对拍漂移。
    DualRunDrift,
    /// 账封板。
    Sealed,
}

impl EvKind {
    /// 事件子码（账内序，非诊断码）。
    pub const fn code(self) -> u16 {
        match self {
            EvKind::InstanceAccepted => 0x01,
            EvKind::VerifyRejected => 0x02,
            EvKind::BaselineFrozen => 0x03,
            EvKind::AdditiveRegistered => 0x04,
            EvKind::CompatClean => 0x05,
            EvKind::CompatAdditive => 0x06,
            EvKind::CompatBlocked => 0x07,
            EvKind::InjectionDetected => 0x08,
            EvKind::DualRunOk => 0x09,
            EvKind::DualRunDrift => 0x0A,
            EvKind::Sealed => 0x0B,
        }
    }

    /// 稳定名。
    pub const fn name(self) -> &'static str {
        match self {
            EvKind::InstanceAccepted => "instance_accepted",
            EvKind::VerifyRejected => "verify_rejected",
            EvKind::BaselineFrozen => "baseline_frozen",
            EvKind::AdditiveRegistered => "additive_registered",
            EvKind::CompatClean => "compat_clean",
            EvKind::CompatAdditive => "compat_additive",
            EvKind::CompatBlocked => "compat_blocked",
            EvKind::InjectionDetected => "injection_detected",
            EvKind::DualRunOk => "dual_run_ok",
            EvKind::DualRunDrift => "dual_run_drift",
            EvKind::Sealed => "sealed",
        }
    }
}

/// 单条事件留痕。
#[derive(Clone, Debug)]
pub struct EventRecord {
    /// 全账连续序号（1 起算，零跳号）。
    pub seq: u64,
    /// 事件种类。
    pub kind: EvKind,
    /// 关联诊断码（无关联为 0）。
    pub diag: u16,
    /// 事件说明。
    pub note: String,
}

/// 事件账：容量封顶 512，封板后拒写。
#[derive(Clone, Debug)]
pub struct Ledger {
    events: Vec<EventRecord>,
    cap: usize,
    sealed: bool,
}

impl Ledger {
    /// 容量上限（判据外账面口径：事件账 512 条封顶）。
    pub const CAP: usize = 512;

    /// 空账。
    pub fn new() -> Self {
        Ledger { events: Vec::new(), cap: Ledger::CAP, sealed: false }
    }

    /// 落一条事件；满账 E_CAPACITY、封板后 E_SEALED。返回该条序号。
    pub fn record(&mut self, kind: EvKind, diag: u16, note: &str) -> Result<u64, RespTreeError> {
        if self.sealed {
            return Err(RespTreeError::Sealed { code: E_SEALED });
        }
        if self.events.len() >= self.cap {
            return Err(RespTreeError::Capacity { code: E_CAPACITY, cap: self.cap });
        }
        let seq = (self.events.len() as u64).wrapping_add(1);
        self.events.push(EventRecord { seq, kind, diag, note: note.to_string() });
        Ok(seq)
    }

    /// 封板：先落 Sealed 事件再锁账（封板后 record 一律 E_SEALED）。
    pub fn seal(&mut self) -> Result<(), RespTreeError> {
        if self.sealed {
            return Err(RespTreeError::Sealed { code: E_SEALED });
        }
        // 事件账自身封板事件在锁位前落（self.sealed 尚为 false）。
        self.record(EvKind::Sealed, 0, "响应树兼容账封板：只读通道开启")?;
        self.sealed = true;
        Ok(())
    }

    /// 是否已封板。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 事件条数。
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 是否为空账。
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 按种类计数。
    pub fn count_kind(&self, k: EvKind) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.events.len() {
            if self.events[i].kind.code() == k.code() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 最后一条事件。
    pub fn last(&self) -> Option<&EventRecord> {
        if self.events.is_empty() {
            None
        } else {
            let n = self.events.len();
            Some(&self.events[n - 1])
        }
    }

    /// 事件账指纹：FNV over（seq 8B LE + 子码 2B LE + diag 2B LE + note 字节）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.events.len() {
            let e = &self.events[i];
            bytes.extend_from_slice(&e.seq.to_le_bytes());
            bytes.extend_from_slice(&e.kind.code().to_le_bytes());
            bytes.extend_from_slice(&e.diag.to_le_bytes());
            bytes.extend_from_slice(e.note.as_bytes());
            i += 1;
        }
        fnv1a64(&bytes)
    }
}

/// health 体检单（判据绿灯口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HealthSummary {
    /// 事件总条数。
    pub events: usize,
    /// 核对通过入账次数。
    pub accepted: usize,
    /// 核对被拒次数。
    pub rejected: usize,
    /// 拦截立案次数。
    pub blocked_cases: usize,
    /// 加法登记条数。
    pub additive_registrations: usize,
    /// 注入检出次数。
    pub injections_detected: usize,
    /// 双跑漂移次数。
    pub drift: usize,
    /// 是否已封板。
    pub sealed: bool,
    /// 绿灯：零被拒、零拦截、零漂移。
    pub is_green: bool,
}

// ── S8：注入面四路（冻结篡改 / 字段缺失 / 魔法注入 / 未登记加法） ────
/// 注入路种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjectionKind {
    /// 冻结字段篡改注入。
    FrozenTamper,
    /// 字段缺失注入（截断）。
    FieldMissing,
    /// 魔法数注入。
    BadMagic,
    /// 未登记加法注入（载荷越覆盖面）。
    UnregAdditive,
}

impl InjectionKind {
    /// 稳定名。
    pub const fn name(self) -> &'static str {
        match self {
            InjectionKind::FrozenTamper => "frozen_tamper",
            InjectionKind::FieldMissing => "field_missing",
            InjectionKind::BadMagic => "bad_magic",
            InjectionKind::UnregAdditive => "unreg_additive",
        }
    }
}

/// 注入结果：detected 必真才算栅栏有效（tests 全绿判据）。
#[derive(Clone, Debug)]
pub struct InjectionOutcome {
    /// 注入路。
    pub injection: InjectionKind,
    /// 注入目标臂。
    pub kind: RespKind,
    /// 是否检出。
    pub detected: bool,
    /// 关联诊断码（兼容三态路径为 0）。
    pub diag: u16,
    /// 检出路径中文说明。
    pub detail: &'static str,
    /// 是否破坏性拦截（破坏即立案）。
    pub breaking: bool,
}

/// 注入目标字段定位（内部）。
fn max_frozen_head_offset(table: &FieldFreezeTable) -> Option<(u16, u16)> {
    let mut best: Option<(u16, u16)> = None;
    let mut i = 0usize;
    while i < table.fields.len() {
        let f = table.fields[i];
        if f.frozen {
            let take = match best {
                None => true,
                Some((off, _)) => f.offset > off,
            };
            if take {
                best = Some((f.offset, f.width));
            }
        }
        i += 1;
    }
    best
}

// ── 总账：RespTreeAccount（S1~S8 收口） ──────────────────────────────
/// boot info 响应树兼容总账：登记账 + 三臂核对留痕 + 基线树 +
/// 事件账 + 注入面。封板后全通道只读。
#[derive(Clone, Debug)]
pub struct RespTreeAccount {
    registry: UpgradeRegistry,
    reports: Vec<VerifyReport>,
    resps: Vec<RawResponse>,
    baseline_tree: Option<RespTree>,
    ledger: Ledger,
}

impl RespTreeAccount {
    /// 空账构造。
    pub fn new() -> Self {
        RespTreeAccount {
            registry: UpgradeRegistry::new(),
            reports: Vec::new(),
            resps: Vec::new(),
            baseline_tree: None,
            ledger: Ledger::new(),
        }
    }

    /// 封板检查（封板后一切写通道拒行）。
    fn check_open(&self) -> Result<(), RespTreeError> {
        if self.ledger.is_sealed() {
            return Err(RespTreeError::Sealed { code: E_SEALED });
        }
        Ok(())
    }

    /// 登记账只读视图。
    pub fn registry(&self) -> &UpgradeRegistry {
        &self.registry
    }

    /// 事件账只读视图。
    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    /// 是否已封板。
    pub fn is_sealed(&self) -> bool {
        self.ledger.is_sealed()
    }

    /// 基线树只读视图。
    pub fn baseline(&self) -> Result<&RespTree, RespTreeError> {
        match self.baseline_tree.as_ref() {
            Some(t) => Ok(t),
            None => Err(RespTreeError::TableAbsent { code: E_TABLE_ABSENT, kind: 0 }),
        }
    }

    /// 按臂取已入账原始实例。
    fn raw_of(&self, kind: RespKind) -> Option<&RawResponse> {
        let mut i = 0usize;
        while i < self.resps.len() {
            if self.resps[i].kind.code() == kind.code() {
                return Some(&self.resps[i]);
            }
            i += 1;
        }
        None
    }

    /// 入账一臂：核对（三拒 + 交叉核）→ 全过才入账留痕。
    /// 非 full → 记 VerifyRejected 事件并回 E_RATE_NOT_FULL（携带率值）。
    pub fn ingest(&mut self, resp: &RawResponse) -> Result<VerifyReport, RespTreeError> {
        self.check_open()?;
        let report = RespTreeVerifier::verify(resp)?;
        if !report.is_full() {
            let code = report.failure_code.unwrap_or(E_RATE_NOT_FULL);
            self.ledger.record(
                EvKind::VerifyRejected,
                code,
                &format!("{} 核对被拒：核对率 {} ppm", resp.kind.name(), report.rate_ppm),
            )?;
            return Err(RespTreeError::RateNotFull {
                code: E_RATE_NOT_FULL,
                kind: resp.kind.code(),
                rate_ppm: report.rate_ppm,
            });
        }
        // 同臂重入账：替换旧留痕（升级复跑口径）。
        let mut replaced = false;
        let mut i = 0usize;
        while i < self.reports.len() {
            if self.reports[i].kind.code() == resp.kind.code() {
                self.reports[i] = report.clone();
                self.resps[i] = resp.clone();
                replaced = true;
            }
            i += 1;
        }
        if !replaced {
            self.reports.push(report.clone());
            self.resps.push(resp.clone());
        }
        self.ledger.record(
            EvKind::InstanceAccepted,
            0,
            &format!("{} 核对 100% 入账（冻结面 {}/{}）", resp.kind.name(), report.frozen_pass, report.frozen_total),
        )?;
        Ok(report)
    }

    /// 基线树冻结：三臂全 100% 才出树落档；重复冻结 E_DUP_TABLE。
    pub fn freeze_baseline(&mut self) -> Result<RespTree, RespTreeError> {
        self.check_open()?;
        if self.baseline_tree.is_some() {
            return Err(RespTreeError::DupTable { code: E_DUP_TABLE, kind: 0 });
        }
        let tree = self.build_current()?;
        self.ledger.record(
            EvKind::BaselineFrozen,
            0,
            &format!("基线树冻结：行数 {}，指纹 0x{:016X}", tree.row_count(), tree.fingerprint),
        )?;
        self.baseline_tree = Some(tree.clone());
        Ok(tree)
    }

    /// 用当前留痕构建响应树快照（不落基线；双跑与注入复树共用）。
    pub fn build_current(&self) -> Result<RespTree, RespTreeError> {
        let refs: Vec<&RawResponse> = self.resps.iter().collect();
        build_resp_tree(&self.reports, &refs, &self.registry)
    }

    /// 加法字段升级登记（三关：宽度/名重/偏移碰撞）→ 落事件账。
    pub fn register_additive(
        &mut self,
        kind: RespKind,
        scope: AddScope,
        name: &str,
        offset: u16,
        width: u16,
        table_version_after: u32,
        note: &str,
    ) -> Result<(), RespTreeError> {
        self.check_open()?;
        self.registry.register_additive(kind, scope, name, offset, width, table_version_after, note)?;
        self.ledger.record(
            EvKind::AdditiveRegistered,
            0,
            &format!("加法登记：{} {} {}@{} w{} → v{}", kind.name(), scope.name(), name, offset, width, table_version_after),
        )?;
        Ok(())
    }

    /// 兼容判定（升级后树 vs 基线树）：三态 + 拦截立案落事件账。
    pub fn classify(&mut self, after: &RespTree) -> Result<CompatAssessment, RespTreeError> {
        self.check_open()?;
        let assessment = {
            let baseline = self.baseline()?;
            classify_compat(baseline, after, &self.registry)
        };
        let ev = match assessment.overall {
            CompatVerdict::Compatible => EvKind::CompatClean,
            CompatVerdict::AdditiveOnly => EvKind::CompatAdditive,
            CompatVerdict::Breaking => EvKind::CompatBlocked,
        };
        self.ledger.record(ev, 0, &assessment.case_note)?;
        Ok(assessment)
    }

    /// 双跑确定性对拍（同当前留痕连建两树比指纹）。
    pub fn dual_run_check(&mut self) -> Result<DualRunReport, RespTreeError> {
        self.check_open()?;
        let a = self.build_current()?;
        let b = self.build_current()?;
        match dual_run_compare(&a, &b) {
            Ok(rep) => {
                self.ledger.record(
                    EvKind::DualRunOk,
                    0,
                    &format!("双跑一致：指纹 0x{:016X}", rep.fingerprint_a),
                )?;
                Ok(rep)
            }
            Err(e) => {
                self.ledger.record(
                    EvKind::DualRunDrift,
                    e.diag_code(),
                    "双跑漂移：同输入两树指纹不等，确定性破",
                )?;
                Err(e)
            }
        }
    }

    /// 账级指纹：FNV over（基线树指纹 ++ 事件账指纹）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        match self.baseline_tree.as_ref() {
            Some(t) => bytes.extend_from_slice(&t.fingerprint.to_le_bytes()),
            None => bytes.extend_from_slice(&0u64.to_le_bytes()),
        }
        bytes.extend_from_slice(&self.ledger.fingerprint().to_le_bytes());
        fnv1a64(&bytes)
    }

    /// health 体检单。
    pub fn health(&self) -> HealthSummary {
        let rejected = self.ledger.count_kind(EvKind::VerifyRejected);
        let blocked = self.ledger.count_kind(EvKind::CompatBlocked);
        let drift = self.ledger.count_kind(EvKind::DualRunDrift);
        HealthSummary {
            events: self.ledger.len(),
            accepted: self.ledger.count_kind(EvKind::InstanceAccepted),
            rejected,
            blocked_cases: blocked,
            additive_registrations: self.ledger.count_kind(EvKind::AdditiveRegistered),
            injections_detected: self.ledger.count_kind(EvKind::InjectionDetected),
            drift,
            sealed: self.ledger.is_sealed(),
            is_green: rejected == 0 && blocked == 0 && drift == 0,
        }
    }

    /// 封板。
    pub fn seal(&mut self) -> Result<(), RespTreeError> {
        self.ledger.seal()
    }

    // ── 注入面（S8）：每路必须显性检出 ──

    /// 注入一路：冻结字段篡改。 plausible 值走双树 diff 归因
    /// （FrozenDrift → Breaking）；越档值走核对三拒（MagicMismatch）。
    pub fn inject_frozen_tamper(
        &mut self,
        kind: RespKind,
        field_name: &str,
        new_value: u64,
    ) -> Result<InjectionOutcome, RespTreeError> {
        self.check_open()?;
        let table = match table_for(kind) {
            Some(t) => t,
            None => return Err(RespTreeError::KindUnknown { code: E_KIND_UNKNOWN, raw_kind: 0 }),
        };
        let idx = match table.find_head_by_name(field_name) {
            Some(i) => i,
            None => {
                return Err(RespTreeError::FieldUnknown {
                    code: E_FIELD_UNKNOWN,
                    kind: kind.code(),
                    field: field_name.to_string(),
                })
            }
        };
        let f = table.fields[idx];
        if !f.frozen {
            return Err(RespTreeError::FieldUnknown {
                code: E_FIELD_UNKNOWN,
                kind: kind.code(),
                field: field_name.to_string(),
            });
        }
        let baseline = self.baseline()?;
        let src = match self.raw_of(kind) {
            Some(r) => r,
            None => return Err(RespTreeError::TableAbsent { code: E_TABLE_ABSENT, kind: kind.code() }),
        };
        let mut raw2 = src.clone();
        if !write_u64_le(&mut raw2.head, f.offset as usize, f.width as usize, new_value) {
            return Err(RespTreeError::PayloadShort {
                code: E_PAYLOAD_SHORT,
                kind: kind.code(),
                need: f.offset as usize + f.width as usize,
                have: raw2.head.len(),
            });
        }
        let report = RespTreeVerifier::verify(&raw2)?;
        let outcome = if !report.is_full() {
            // 路径一：核对三拒显性拦截。
            InjectionOutcome {
                injection: InjectionKind::FrozenTamper,
                kind,
                detected: true,
                diag: report.failure_code.unwrap_or(E_MAGIC_MISMATCH),
                detail: "冻结字段篡改在核对期被语义档拦截（越档值）",
                breaking: true,
            }
        } else {
            // 路径二：plausible 值过核对 → 双树 diff 归因拦截。
            let after = baseline.with_report_rows(&report, &self.registry, &raw2)?;
            let assessment = classify_compat(baseline, &after, &self.registry);
            InjectionOutcome {
                injection: InjectionKind::FrozenTamper,
                kind,
                detected: assessment.blocked,
                diag: 0,
                detail: "冻结字段篡改在双树 diff 归因期被检出（FrozenDrift → Breaking）",
                breaking: assessment.blocked,
            }
        };
        self.ledger.record(
            EvKind::InjectionDetected,
            outcome.diag,
            &format!(
                "注入 {} {} 字段 {}：检出={}（diag=0x{:04X}）",
                outcome.injection.name(),
                kind.name(),
                field_name,
                outcome.detected,
                outcome.diag
            ),
        )?;
        Ok(outcome)
    }

    /// 注入二路：字段缺失（截断最后一个头冻结字段起的所有字节）。
    pub fn inject_missing_field(&mut self, kind: RespKind) -> Result<InjectionOutcome, RespTreeError> {
        self.check_open()?;
        let table = match table_for(kind) {
            Some(t) => t,
            None => return Err(RespTreeError::KindUnknown { code: E_KIND_UNKNOWN, raw_kind: 0 }),
        };
        let src = match self.raw_of(kind) {
            Some(r) => r,
            None => return Err(RespTreeError::TableAbsent { code: E_TABLE_ABSENT, kind: kind.code() }),
        };
        let (off, _w) = match max_frozen_head_offset(&table) {
            Some(x) => x,
            None => {
                return Err(RespTreeError::TableAbsent { code: E_TABLE_ABSENT, kind: kind.code() })
            }
        };
        let mut raw2 = src.clone();
        let keep = off as usize;
        if keep > raw2.head.len() {
            return Err(RespTreeError::PayloadShort {
                code: E_PAYLOAD_SHORT,
                kind: kind.code(),
                need: keep,
                have: raw2.head.len(),
            });
        }
        raw2.head.truncate(keep);
        let report = RespTreeVerifier::verify(&raw2)?;
        let fc = report.failure_code.unwrap_or(0);
        let detected = !report.is_full() && fc == E_FIELD_MISSING;
        let outcome = InjectionOutcome {
            injection: InjectionKind::FieldMissing,
            kind,
            detected,
            diag: fc,
            detail: "字段缺失注入被核对一拒（E_FIELD_MISSING）检出",
            breaking: detected,
        };
        self.ledger.record(
            EvKind::InjectionDetected,
            outcome.diag,
            &format!("注入 {} {}：检出={}", outcome.injection.name(), kind.name(), outcome.detected),
        )?;
        Ok(outcome)
    }

    /// 注入三路：魔法数注入（按臂魔法探针写越界垃圾值）。
    pub fn inject_bad_magic(&mut self, kind: RespKind) -> Result<InjectionOutcome, RespTreeError> {
        self.check_open()?;
        let probe = kind.magic_probe();
        let src = match self.raw_of(kind) {
            Some(r) => r,
            None => return Err(RespTreeError::TableAbsent { code: E_TABLE_ABSENT, kind: kind.code() }),
        };
        let mut raw2 = src.clone();
        let ok = if probe.in_entry {
            if raw2.entries.is_empty() {
                false
            } else {
                write_u64_le(&mut raw2.entries[0], probe.offset as usize, probe.width as usize, probe.garbage)
            }
        } else {
            write_u64_le(&mut raw2.head, probe.offset as usize, probe.width as usize, probe.garbage)
        };
        if !ok {
            return Err(RespTreeError::PayloadShort {
                code: E_PAYLOAD_SHORT,
                kind: kind.code(),
                need: probe.offset as usize + probe.width as usize,
                have: if probe.in_entry && !raw2.entries.is_empty() {
                    raw2.entries[0].len()
                } else {
                    raw2.head.len()
                },
            });
        }
        let report = RespTreeVerifier::verify(&raw2)?;
        let fc = report.failure_code.unwrap_or(0);
        let detected = !report.is_full() && fc == E_MAGIC_MISMATCH;
        let outcome = InjectionOutcome {
            injection: InjectionKind::BadMagic,
            kind,
            detected,
            diag: fc,
            detail: "魔法数注入被核对三拒（E_MAGIC_MISMATCH）检出",
            breaking: detected,
        };
        self.ledger.record(
            EvKind::InjectionDetected,
            outcome.diag,
            &format!("注入 {} {}：检出={}", outcome.injection.name(), kind.name(), outcome.detected),
        )?;
        Ok(outcome)
    }

    /// 注入四路：未登记加法（头尾私接 8 字节 → 覆盖关拦截）。
    pub fn inject_unreg_additive(&mut self, kind: RespKind) -> Result<InjectionOutcome, RespTreeError> {
        self.check_open()?;
        let src = match self.raw_of(kind) {
            Some(r) => r,
            None => return Err(RespTreeError::TableAbsent { code: E_TABLE_ABSENT, kind: kind.code() }),
        };
        let mut raw2 = src.clone();
        raw2.head.extend_from_slice(&[0xA5u8; 8]);
        // 临时组树：报告沿用现留痕，原始实例以注入版替换该臂。
        let mut tmp: Vec<RawResponse> = self.resps.clone();
        let mut i = 0usize;
        let mut hit = false;
        while i < tmp.len() {
            if tmp[i].kind.code() == kind.code() {
                tmp[i] = raw2.clone();
                hit = true;
            }
            i += 1;
        }
        if !hit {
            return Err(RespTreeError::TableAbsent { code: E_TABLE_ABSENT, kind: kind.code() });
        }
        let refs: Vec<&RawResponse> = tmp.iter().collect();
        let detected = match build_resp_tree(&self.reports, &refs, &self.registry) {
            Ok(_) => false,
            Err(e) => {
                if e.diag_code() == E_UNREG_ADDITIVE {
                    self.ledger.record(
                        EvKind::InjectionDetected,
                        e.diag_code(),
                        &format!("注入 unreg_additive {}：覆盖关拦截（未登记加法）", kind.name()),
                    )?;
                    true
                } else {
                    return Err(e);
                }
            }
        };
        Ok(InjectionOutcome {
            injection: InjectionKind::UnregAdditive,
            kind,
            detected,
            diag: if detected { E_UNREG_ADDITIVE } else { 0 },
            detail: "未登记加法被覆盖关拦截（载荷越冻结表面 + 登记面）",
            breaking: detected,
        })
    }
}

// ── 编译期守恒断言（允许：const 期，非运行期） ───────────────────────
const _: () = assert!(SEG_BASE == 0x1650);
const _: () = assert!(E_FIELD_UNKNOWN == 0x165F);
const _: () = assert!(KIND_COUNT == 3);
const _: () = assert!(PPM_SCALE == 10_000);
const _: () = assert!(size_of::<FreezeField>() <= 32);
const _: () = assert!(size_of::<FieldFreezeTable>() <= 64);
const _: () = assert!(size_of::<MagicProbe>() <= 32);
const _: () = assert!(size_of::<SnapshotRow>() <= 80);
const _: () = assert!(size_of::<EventRecord>() <= 64);
const _: () = assert!(size_of::<RespKind>() <= 1);
const _: () = assert!(Ledger::CAP == 512);
const _: () = assert!(MM_FIELDS.len() == 3);
const _: () = assert!(FB_FIELDS.len() == 3);
const _: () = assert!(KA_FIELDS.len() == 3);
const _: () = assert!(FB_ENTRY_FIELDS.len() == 14);
const _: () = assert!(MM_ENTRY_FIELDS.len() == 3);

// ── 测试域（std 专用；内核路径零 unwrap/panic 纪律在测试域放开） ──────
#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    // 测试域专用（std 通道；内核路径不受影响）。
    use std::vec;

    /// 测试辅助：Ok 解包（Err 即 panic，仅测试域）。
    fn ok<T>(r: Result<T, RespTreeError>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => std::panic::panic_any(format!("expect ok, got: {} {:?}", e.detail(), e)),
        }
    }

    /// 测试辅助：断言诊断码。
    fn assert_code(r: Result<(), RespTreeError>, want: u16) {
        match r {
            Ok(_) => std::panic::panic_any(format!("expect err 0x{:04X}, got ok", want)),
            Err(e) => assert_eq!(e.diag_code(), want, "diag mismatch: {}", e.detail()),
        }
    }

    // ── 干净实例构造器（Limine 布局口径） ──

    fn put_u64(buf: &mut Vec<u8>, off: usize, v: u64) {
        while buf.len() < off + 8 {
            buf.push(0u8);
        }
        let mut i = 0usize;
        while i < 8 {
            buf[off + i] = ((v >> (8 * i)) & 0xFF) as u8;
            i += 1;
        }
    }

    fn put_u8(buf: &mut Vec<u8>, off: usize, v: u8) {
        while buf.len() < off + 1 {
            buf.push(0u8);
        }
        buf[off] = v;
    }

    fn mm_head(rev: u64, count: u64, entries: u64) -> Vec<u8> {
        let mut b: Vec<u8> = Vec::new();
        put_u64(&mut b, 0, rev);
        put_u64(&mut b, 8, count);
        put_u64(&mut b, 16, entries);
        b
    }

    fn mm_entry(base: u64, len: u64, ty: u64) -> Vec<u8> {
        let mut b: Vec<u8> = Vec::new();
        put_u64(&mut b, 0, base);
        put_u64(&mut b, 8, len);
        put_u64(&mut b, 16, ty);
        b
    }

    fn fb_head(rev: u64, count: u64, ptr: u64) -> Vec<u8> {
        let mut b: Vec<u8> = Vec::new();
        put_u64(&mut b, 0, rev);
        put_u64(&mut b, 8, count);
        put_u64(&mut b, 16, ptr);
        b
    }

    fn fb_entry(addr: u64, w: u64, h: u64, pitch: u64, bpp: u64, edid_size: u64, edid: u64) -> Vec<u8> {
        let mut b: Vec<u8> = Vec::new();
        put_u64(&mut b, 0, addr);
        put_u64(&mut b, 8, w);
        put_u64(&mut b, 16, h);
        put_u64(&mut b, 24, pitch);
        put_u64(&mut b, 32, bpp);
        // memory_model "fb\0\0\0\0\0\0"（小端 0x6266）。
        put_u64(&mut b, 40, FB_MODEL_MAGIC);
        put_u8(&mut b, 56, 8); // red_mask_size
        put_u8(&mut b, 57, 16); // red_mask_shift
        put_u8(&mut b, 58, 8); // green_mask_size
        put_u8(&mut b, 59, 8); // green_mask_shift
        put_u8(&mut b, 60, 8); // blue_mask_size
        put_u8(&mut b, 61, 0); // blue_mask_shift
        put_u64(&mut b, 64, edid_size);
        put_u64(&mut b, 72, edid);
        b
    }

    fn ka_head(rev: u64, phys: u64, virt: u64) -> Vec<u8> {
        let mut b: Vec<u8> = Vec::new();
        put_u64(&mut b, 0, rev);
        put_u64(&mut b, 8, phys);
        put_u64(&mut b, 16, virt);
        b
    }

    /// 三臂干净实例组。
    fn clean_trio() -> (RawResponse, RawResponse, RawResponse) {
        let e0 = mm_entry(0x0000_1000, 0x0001_0000, 0);
        let e1 = mm_entry(0x0010_0000, 0x0000_8000, 1);
        let mm = ok(RawResponse::build(
            RespKind::MemoryMap,
            &mm_head(1, 2, 0x0000_0000_0001_0000),
            &[&e0, &e1],
        ));
        let fb = ok(RawResponse::build(
            RespKind::Framebuffer,
            &fb_head(1, 1, 0x0000_0000_0002_0000),
            &[&fb_entry(0x0000_0000_FD00_0000, 1920, 1080, 7680, 32, 128, 0x0000_0000_0003_0000)],
        ));
        let ka = ok(RawResponse::build(
            RespKind::KernelAddress,
            &ka_head(1, 0x0010_0000, 0xFFFF_8000_0010_0000),
            &[],
        ));
        (mm, fb, ka)
    }

    // ── T1：三响应逐字段核对 100%（判据 J1-c 主判） ──
    #[test]
    fn three_responses_verify_full_100pct() {
        let (mm, fb, ka) = clean_trio();
        let rm = ok(RespTreeVerifier::verify(&mm));
        let rf = ok(RespTreeVerifier::verify(&fb));
        let rk = ok(RespTreeVerifier::verify(&ka));
        assert!(rm.is_full(), "memory_map 核对未满：{} ppm", rm.rate_ppm);
        assert!(rf.is_full(), "framebuffer 核对未满：{} ppm", rf.rate_ppm);
        assert!(rk.is_full(), "kernel_address 核对未满：{} ppm", rk.rate_ppm);
        assert_eq!(rm.rate_ppm, 10_000);
        assert_eq!(rf.rate_ppm, 10_000);
        assert_eq!(rk.rate_ppm, 10_000);
        assert_eq!(rm.failing_len(), 0);
        assert_eq!(rf.failing_len(), 0);
        assert_eq!(rk.failing_len(), 0);
        // 冻结面口径：MM 头 3 + 交叉核 1 + 条目 2×3 = 10；FB 头 3 + 交叉核 1
        // + 条目 1×12 = 16；KA 头 3（无条目无交叉核）= 3。
        assert_eq!(rm.frozen_total, 10);
        assert_eq!(rf.frozen_total, 16);
        assert_eq!(rk.frozen_total, 3);
    }

    // ── T2：全账干净流：ingest × 3 → 基线冻结 → 双跑一致 → 绿灯 ──
    #[test]
    fn clean_flow_green_and_dual_run() {
        let (mm, fb, ka) = clean_trio();
        let mut acc = RespTreeAccount::new();
        ok(acc.ingest(&mm));
        ok(acc.ingest(&fb));
        ok(acc.ingest(&ka));
        let baseline = ok(acc.freeze_baseline());
        assert!(baseline.fingerprint_consistent());
        let rep = ok(acc.dual_run_check());
        assert!(rep.identical);
        // 双跑二次构建指纹必等。
        let t2 = ok(acc.build_current());
        assert!(baseline.same_as(&t2));
        // 同树自判 → Compatible。
        let asmt = ok(acc.classify(&baseline.clone()));
        assert_eq!(asmt.overall, CompatVerdict::Compatible);
        assert!(!asmt.blocked);
        let h = acc.health();
        assert!(h.is_green, "health 非绿灯：{:?}", h);
        assert_eq!(h.accepted, 3);
        assert_eq!(h.rejected, 0);
        assert_eq!(h.blocked_cases, 0);
        assert_eq!(h.drift, 0);
    }

    // ── T3：冻结篡改拦截（plausible 值 → diff 归因 FrozenDrift） ──
    #[test]
    fn frozen_tamper_blocked_via_diff() {
        let (mm, fb, ka) = clean_trio();
        let mut acc = RespTreeAccount::new();
        ok(acc.ingest(&mm));
        ok(acc.ingest(&fb));
        ok(acc.ingest(&ka));
        ok(acc.freeze_baseline());
        let out = ok(acc.inject_frozen_tamper(RespKind::KernelAddress, "physical_base", 0x0020_0000));
        assert!(out.detected, "冻结篡改未检出");
        assert!(out.breaking);
        let h = acc.health();
        assert_eq!(h.blocked_cases, 0); // 注入走 outcome，不落 CompatBlocked
        assert_eq!(h.injections_detected, 1);
    }

    // ── T4：冻结篡改（越档值）→ 核对期魔法拦截 ──
    #[test]
    fn frozen_tamper_over_cap_magic_reject() {
        let (mm, fb, ka) = clean_trio();
        let mut acc = RespTreeAccount::new();
        ok(acc.ingest(&mm));
        ok(acc.ingest(&fb));
        ok(acc.ingest(&ka));
        ok(acc.freeze_baseline());
        let out = ok(acc.inject_frozen_tamper(RespKind::KernelAddress, "revision", REV_CAP + 1));
        assert!(out.detected);
        assert_eq!(out.diag, E_MAGIC_MISMATCH);
        assert!(out.breaking);
    }

    // ── T5：升级后树冻结字段漂移 → Breaking 拦截立案（判据 J1-d） ──
    #[test]
    fn upgrade_frozen_drift_breaks_and_blocks() {
        let (mm, fb, ka) = clean_trio();
        let mut acc = RespTreeAccount::new();
        ok(acc.ingest(&mm));
        ok(acc.ingest(&fb));
        ok(acc.ingest(&ka));
        ok(acc.freeze_baseline());
        // 升级后 kernel address physical_base 漂移（冻结字段变更即破坏）。
        let ka2 = ok(RawResponse::build(
            RespKind::KernelAddress,
            &ka_head(1, 0x0030_0000, 0xFFFF_8000_0010_0000),
            &[],
        ));
        ok(acc.ingest(&ka2));
        let after = ok(acc.build_current());
        let asmt = ok(acc.classify(&after));
        assert_eq!(asmt.overall, CompatVerdict::Breaking);
        assert!(asmt.blocked);
        assert!(asmt.breaking_diffs >= 1);
        assert!(!asmt.case_note.is_empty());
        // 逐项归因：physical_base FrozenDrift。
        let mut found = false;
        let mut i = 0usize;
        while i < asmt.kinds.len() {
            if asmt.kinds[i].kind.code() == RespKind::KernelAddress.code() {
                let mut d = 0usize;
                while d < asmt.kinds[i].diffs.len() {
                    if asmt.kinds[i].diffs[d].field == "physical_base"
                        && asmt.kinds[i].diffs[d].cause == DiffCause::FrozenDrift
                    {
                        found = true;
                    }
                    d += 1;
                }
            }
            i += 1;
        }
        assert!(found, "physical_base 漂移未归因 FrozenDrift");
        let h = acc.health();
        assert_eq!(h.blocked_cases, 1);
        assert!(!h.is_green);
    }

    // ── T6：登记加法 → AdditiveOnly 放行（加法对偶 + 登记制） ──
    #[test]
    fn registered_additive_is_additive_only() {
        let (mm, fb, ka) = clean_trio();
        let mut acc = RespTreeAccount::new();
        ok(acc.ingest(&mm));
        ok(acc.ingest(&fb));
        ok(acc.ingest(&ka));
        ok(acc.freeze_baseline());
        // 升级登记：memory map 头部加法字段 reserved0@24 w8（v2）。
        ok(acc.register_additive(
            RespKind::MemoryMap,
            AddScope::Head,
            "mm_reserved0",
            24,
            8,
            2,
            "上游 v2 新增保留位，登记后冻结入表",
        ));
        // 升级后实例：头多 8 字节且带值；条目照旧（交叉核口径不变）。
        let e0 = mm_entry(0x0000_1000, 0x0001_0000, 0);
        let e1 = mm_entry(0x0010_0000, 0x0000_8000, 1);
        let mut head2 = mm_head(1, 2, 0x0000_0000_0001_0000);
        let mut extra: Vec<u8> = Vec::new();
        put_u64(&mut extra, 0, 0x0000_0000_0000_00AA);
        head2.extend_from_slice(&extra);
        let mm2 = ok(RawResponse::build(RespKind::MemoryMap, &head2, &[&e0, &e1]));
        ok(acc.ingest(&mm2));
        let after = ok(acc.build_current());
        let asmt = ok(acc.classify(&after));
        assert_eq!(asmt.overall, CompatVerdict::AdditiveOnly, "登记加法应放行为仅加法兼容");
        assert!(!asmt.blocked);
        assert_eq!(asmt.breaking_diffs, 0);
        let h = acc.health();
        assert_eq!(h.additive_registrations, 1);
        assert!(h.is_green, "登记加法不应破坏绿灯：{:?}", h);
    }

    // ── T7：未登记加法 → 覆盖关拦截（E_UNREG_ADDITIVE） ──
    #[test]
    fn unregistered_additive_rejected() {
        let (mm, fb, ka) = clean_trio();
        let mut acc = RespTreeAccount::new();
        ok(acc.ingest(&mm));
        ok(acc.ingest(&fb));
        ok(acc.ingest(&ka));
        ok(acc.freeze_baseline());
        let out = ok(acc.inject_unreg_additive(RespKind::Framebuffer));
        assert!(out.detected, "未登记加法未拦截");
        assert_eq!(out.diag, E_UNREG_ADDITIVE);
        assert!(out.breaking);
    }

    // ── T8：字段缺失注入（三拒之一） ──
    #[test]
    fn missing_field_injection_detected() {
        let (mm, fb, ka) = clean_trio();
        let mut acc = RespTreeAccount::new();
        ok(acc.ingest(&mm));
        ok(acc.ingest(&fb));
        ok(acc.ingest(&ka));
        ok(acc.freeze_baseline());
        let out = ok(acc.inject_missing_field(RespKind::Framebuffer));
        assert!(out.detected, "字段缺失注入未检出");
        assert_eq!(out.diag, E_FIELD_MISSING);
        // KA 臂同样可检出。
        let out2 = ok(acc.inject_missing_field(RespKind::KernelAddress));
        assert!(out2.detected);
        assert_eq!(out2.diag, E_FIELD_MISSING);
    }

    // ── T9：魔法数注入（三拒之一）：三臂各一 ──
    #[test]
    fn bad_magic_injection_detected_all_kinds() {
        let (mm, fb, ka) = clean_trio();
        let mut acc = RespTreeAccount::new();
        ok(acc.ingest(&mm));
        ok(acc.ingest(&fb));
        ok(acc.ingest(&ka));
        ok(acc.freeze_baseline());
        let o1 = ok(acc.inject_bad_magic(RespKind::MemoryMap));
        assert!(o1.detected && o1.diag == E_MAGIC_MISMATCH, "MM 闭集魔法未检出");
        let o2 = ok(acc.inject_bad_magic(RespKind::Framebuffer));
        assert!(o2.detected && o2.diag == E_MAGIC_MISMATCH, "FB ASCII 魔法未检出");
        let o3 = ok(acc.inject_bad_magic(RespKind::KernelAddress));
        assert!(o3.detected && o3.diag == E_MAGIC_MISMATCH, "KA 高半区哨兵未检出");
        let h = acc.health();
        assert_eq!(h.injections_detected, 3);
    }

    // ── T10：条目 schema 核对（entry_type 越闭集 → MagicMismatch） ──
    #[test]
    fn entry_schema_closed_set_reject() {
        let e0 = mm_entry(0x1000, 0x1000, 0);
        let e1 = mm_entry(0x2000, 0x1000, 9); // 越 0..=6 闭集
        let mm = ok(RawResponse::build(
            RespKind::MemoryMap,
            &mm_head(1, 2, 0x0000_0000_0000_3000),
            &[&e0, &e1],
        ));
        let rep = ok(RespTreeVerifier::verify(&mm));
        assert!(!rep.is_full());
        assert_eq!(rep.failure_code, Some(E_MAGIC_MISMATCH));
        assert_eq!(rep.failing_len(), 1);
    }

    // ── T11：交叉核（entry_count ↔ 实际条目数不符 → SchemaFail） ──
    #[test]
    fn cross_check_count_vs_entries() {
        let e0 = mm_entry(0x1000, 0x1000, 0);
        let mm = ok(RawResponse::build(
            RespKind::MemoryMap,
            &mm_head(1, 3, 0x0000_0000_0000_1000), // 声称 3 实给 1
            &[&e0],
        ));
        let rep = ok(RespTreeVerifier::verify(&mm));
        assert!(!rep.is_full());
        assert_eq!(rep.failure_code, Some(E_ENTRY_SCHEMA_FAIL));
        // 树构建拒行（E_RATE_NOT_FULL）。
        let mut acc = RespTreeAccount::new();
        let err = match acc.ingest(&mm) {
            Ok(_) => std::panic::panic_any("expect RateNotFull"),
            Err(e) => e,
        };
        assert_eq!(err.diag_code(), E_RATE_NOT_FULL);
    }

    // ── T12：登记账三关（名重 / 偏移碰撞 / 宽度域 / 版本升位） ──
    #[test]
    fn registry_gates() {
        let mut reg = UpgradeRegistry::new();
        ok(reg.register_additive(RespKind::MemoryMap, AddScope::Head, "f_a", 24, 8, 2, "ok"));
        // 名重关。
        assert_code(
            reg.register_additive(RespKind::Framebuffer, AddScope::Head, "f_a", 80, 8, 2, "dup"),
            E_ADDITIVE_COLLISION,
        );
        // 偏移碰撞关（24..32 与 28..36 测交）。
        assert_code(
            reg.register_additive(RespKind::MemoryMap, AddScope::Head, "f_b", 28, 8, 2, "overlap"),
            E_ADDITIVE_COLLISION,
        );
        // 宽度域关。
        assert_code(
            reg.register_additive(RespKind::MemoryMap, AddScope::Head, "f_c", 40, 0, 2, "w0"),
            E_WIDTH_BAD,
        );
        assert_code(
            reg.register_additive(RespKind::MemoryMap, AddScope::Head, "f_d", 40, 9, 2, "w9"),
            E_WIDTH_BAD,
        );
        // 版本升位关（≤ 基线版本拒）。
        assert_code(
            reg.register_additive(RespKind::MemoryMap, AddScope::Head, "f_e", 40, 8, 1, "vbump"),
            E_ADDITIVE_COLLISION,
        );
        assert_eq!(reg.len(), 1);
        assert!(reg.is_registered(RespKind::MemoryMap, "f_a"));
        assert_eq!(reg.head_registered_span(RespKind::MemoryMap), 32);
    }

    // ── T13：封板制（封板后写通道全拒 E_SEALED） ──
    #[test]
    fn seal_blocks_all_writes() {
        let (mm, fb, ka) = clean_trio();
        let mut acc = RespTreeAccount::new();
        ok(acc.ingest(&mm));
        ok(acc.ingest(&fb));
        ok(acc.ingest(&ka));
        ok(acc.seal());
        assert!(acc.is_sealed());
        assert_code(acc.ingest(&mm).map(|_| ()), E_SEALED);
        assert_code(
            acc.register_additive(RespKind::MemoryMap, AddScope::Head, "x", 24, 8, 2, "n").map(|_| ()),
            E_SEALED,
        );
        let h = acc.health();
        assert!(h.sealed);
    }

    // ── T14：指纹与确定性（双树同构 + 行序扰动不存在的证明） ──
    #[test]
    fn fingerprints_deterministic() {
        let (mm, fb, ka) = clean_trio();
        let mut acc = RespTreeAccount::new();
        ok(acc.ingest(&mm));
        ok(acc.ingest(&fb));
        ok(acc.ingest(&ka));
        let t1 = ok(acc.build_current());
        let t2 = ok(acc.build_current());
        assert_eq!(t1.fingerprint, t2.fingerprint);
        assert!(t1.same_as(&t2));
        // 漂移树 → E_REPLAY_DRIFT。
        let ka2 = ok(RawResponse::build(
            RespKind::KernelAddress,
            &ka_head(2, 0x0010_0000, 0xFFFF_8000_0010_0000),
            &[],
        ));
        ok(acc.ingest(&ka2));
        let t3 = ok(acc.build_current());
        assert_ne!(t1.fingerprint, t3.fingerprint);
        match dual_run_compare(&t1, &t3) {
            Ok(_) => std::panic::panic_any("expect drift"),
            Err(e) => assert_eq!(e.diag_code(), E_REPLAY_DRIFT),
        }
    }

    // ── T15：const 冻结表口径与三臂表查表 ──
    #[test]
    fn tables_const_smoke() {
        let tm = ok_or_none(table_for(RespKind::MemoryMap));
        let tf = ok_or_none(table_for(RespKind::Framebuffer));
        let tk = ok_or_none(table_for(RespKind::KernelAddress));
        assert_eq!(tm.table_version, TABLE_VERSION_BASE);
        assert_eq!(tm.head_frozen_len(), 3);
        assert_eq!(tm.entry_frozen_len(), 3);
        assert_eq!(tf.entry_frozen_len(), 12); // 14 行中 edid_size/edid 可演进
        assert_eq!(tk.entry_len(), 0);
        assert_eq!(tm.head_span(), 24);
        assert_eq!(tf.entry_span(), 80);
        // 魔法探针口径。
        let pm = RespKind::MemoryMap.magic_probe();
        assert!(pm.in_entry && pm.offset == 16);
        let pk = RespKind::KernelAddress.magic_probe();
        assert!(!pk.in_entry && pk.offset == 16 && pk.garbage < VIRT_FLOOR);
        // 语义档 const 核抽查。
        assert!(SemTag::VirtAddr.check(VIRT_FLOOR) == CheckOutcome::Plausible);
        assert!(SemTag::VirtAddr.check(VIRT_FLOOR - 1) == CheckOutcome::MagicMismatch);
        assert!(SemTag::AsciiMagic.check(FB_MODEL_MAGIC) == CheckOutcome::Plausible);
        assert!(SemTag::EnumTag.check(6) == CheckOutcome::Plausible);
        assert!(SemTag::EnumTag.check(7) == CheckOutcome::MagicMismatch);
        assert!(SemTag::Ptr.check(0x11) == CheckOutcome::TypeMismatch); // 非对齐
    }

    fn ok_or_none(t: Option<FieldFreezeTable>) -> FieldFreezeTable {
        match t {
            Some(t) => t,
            None => std::panic::panic_any("table_for 必中三臂"),
        }
    }

    // ── T16：臂级缺失归因（after 缺整臂 → Breaking） ──
    #[test]
    fn whole_kind_absence_breaks() {
        let (mm, fb, ka) = clean_trio();
        let mut acc = RespTreeAccount::new();
        ok(acc.ingest(&mm));
        ok(acc.ingest(&fb));
        ok(acc.ingest(&ka));
        let baseline = ok(acc.freeze_baseline());
        // 构造缺 KA 臂的 after 树（基线去掉 KA 行）。
        let mut rows: Vec<SnapshotRow> = Vec::new();
        let mut i = 0usize;
        while i < baseline.rows.len() {
            if baseline.rows[i].kind.code() != RespKind::KernelAddress.code() {
                rows.push(baseline.rows[i].clone());
            }
            i += 1;
        }
        let fp = fnv1a64(&{
            // 与 canonical_bytes 同口径复算（测试域直接用树内行序）。
            let mut b: Vec<u8> = Vec::new();
            let mut j = 0usize;
            while j < rows.len() {
                let r = &rows[j];
                b.push(r.kind.code());
                b.extend_from_slice(&r.table_version.to_le_bytes());
                b.push(match r.scope {
                    CheckScope::Head => 0u8,
                    CheckScope::Entry(_) => 1u8,
                });
                b.extend_from_slice(&r.scope.index_code().to_le_bytes());
                let nb = r.name.as_bytes();
                b.extend_from_slice(&(nb.len() as u16).to_le_bytes());
                b.extend_from_slice(nb);
                b.extend_from_slice(&r.value.to_le_bytes());
                j += 1;
            }
            b
        });
        let after = RespTree { rows, fingerprint: fp };
        let asmt = classify_compat(&baseline, &after, acc.registry());
        assert_eq!(asmt.overall, CompatVerdict::Breaking);
        assert!(asmt.blocked);
    }

    // ── T17：错误 detail 全中文可操作 + 诊断码段独占性 ──
    #[test]
    fn diag_segment_exclusive_and_details() {
        // 段内 16 码全部有中文诊断。
        let mut c: u16 = SEG_BASE;
        while c <= 0x165F {
            let d = diag_detail(c);
            assert!(!d.starts_with("未知"), "0x{:04X} 缺诊断文案", c);
            c += 1;
        }
        // 段外归未知。
        assert!(diag_detail(0x164F).starts_with("未知"));
        assert!(diag_detail(0x1660).starts_with("未知"));
        // 每个错误变体 detail 非空且码在段内。
        let samples: Vec<RespTreeError> = vec![
            RespTreeError::Sealed { code: E_SEALED },
            RespTreeError::KindUnknown { code: E_KIND_UNKNOWN, raw_kind: 9 },
            RespTreeError::FieldMissing { code: E_FIELD_MISSING, kind: 1, field: "revision".to_string() },
            RespTreeError::TypeMismatch { code: E_TYPE_MISMATCH, kind: 1, field: "entries".to_string(), observed: 0 },
            RespTreeError::MagicMismatch { code: E_MAGIC_MISMATCH, kind: 2, field: "memory_model".to_string(), observed: 0xFFFF },
            RespTreeError::TableAbsent { code: E_TABLE_ABSENT, kind: 0 },
            RespTreeError::DupTable { code: E_DUP_TABLE, kind: 0 },
            RespTreeError::AdditiveCollision { code: E_ADDITIVE_COLLISION, kind: 1, name: "x".to_string() },
            RespTreeError::UnregAdditive { code: E_UNREG_ADDITIVE, kind: 1, observed_len: 32, allowed_span: 24 },
            RespTreeError::WidthBad { code: E_WIDTH_BAD, width: 9 },
            RespTreeError::PayloadShort { code: E_PAYLOAD_SHORT, kind: 3, need: 24, have: 8 },
            RespTreeError::EntrySchemaFail { code: E_ENTRY_SCHEMA_FAIL, kind: 1, entry_index: 0, detail: "count".to_string() },
            RespTreeError::RateNotFull { code: E_RATE_NOT_FULL, kind: 1, rate_ppm: 9000 },
            RespTreeError::ReplayDrift { code: E_REPLAY_DRIFT, fp_a: 1, fp_b: 2 },
            RespTreeError::Capacity { code: E_CAPACITY, cap: 512 },
            RespTreeError::FieldUnknown { code: E_FIELD_UNKNOWN, kind: 1, field: "nope".to_string() },
        ];
        let mut i = 0usize;
        while i < samples.len() {
            let e = &samples[i];
            assert!(e.diag_code() >= SEG_BASE && e.diag_code() <= 0x165F);
            assert!(!e.detail().is_empty());
            assert!(!e.detail().starts_with("未知"));
            i += 1;
        }
    }

    // ── T18：重复入账同臂替换（升级复跑口径） + 账级指纹稳定 ──
    #[test]
    fn reingest_replaces_and_fingerprint_stable() {
        let (mm, fb, ka) = clean_trio();
        let mut acc = RespTreeAccount::new();
        ok(acc.ingest(&mm));
        ok(acc.ingest(&fb));
        ok(acc.ingest(&ka));
        let t1 = ok(acc.build_current());
        ok(acc.ingest(&mm)); // 同臂重入账（同内容）→ 替换不追加
        let t2 = ok(acc.build_current());
        assert!(t1.same_as(&t2), "同内容重入账不应漂移树指纹");
        assert_eq!(acc.health().accepted, 4); // 三臂首发 + 一次替换
    }
}
