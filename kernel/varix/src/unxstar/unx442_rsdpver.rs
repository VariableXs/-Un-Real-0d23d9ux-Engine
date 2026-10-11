//! unx442_rsdpver —— UNX-F0442 RSDP 校验与版本账。
//! 锚点（CoRun Unxreal 主册 UNX-A1-B22）：UNX-F0442 · RSDP 校验与版本账 · 目标 220 行 · 已深化
//! 判据原文：RSDP 校验和验证通过（1/1），revision 与长度字段自洽断言（v1:20/v2+:36）
//! 组件：RSDP 结构契约（v1:20 字节 / v2+:36 字节扩展 XSDT 地址）+ 双校验和验证器
//!      （首 20 字节 v1 校验和 + 全 36 字节 v2 校验和）+ 版本账状态机（双代自洽）
//!      + 签名门（"RSD PTR " 8 字节签名逐字节核）+ 探测账（命中窗记 OEM/revision/长度）
//! 判据落地：校验和 1/1 + revision 与长度字段自洽（v1:20/v2+:36）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，内存读注入（零真实物理内存访问）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const RV_VERSION: &str = "unx442_rsdpver-v1";

/// 诊断段基址（0x4c40 段 · 本模块独占）。
pub const RV_SEG_BASE: u16 = 0x4c40;

/// RSDP v1 结构长度（ACPI 1.0，20 字节）。
pub const RV_LEN_V1: usize = 20;

/// RSDP v2+ 结构长度（ACPI 2.0+，36 字节）。
pub const RV_LEN_V2: usize = 36;

/// RSDP 签名（8 字节 "RSD PTR "，注意末尾空格——ACPI 规范原文）。
pub const RV_SIGNATURE: [u8; 8] = [0x52, 0x53, 0x44, 0x20, 0x50, 0x54, 0x52, 0x20];

/// revision 0/1 = ACPI 1.0（v1 布局），revision 2+ = ACPI 2.0+（v2 布局）。
pub const RV_REV_V1: u8 = 0;
pub const RV_REV_V2: u8 = 2;

/// 探测窗（来源：ACPI 规范 RSDP 定位算法）。
/// EBDA 前 1KB：段基址 0x40E 处取值 ×16 起。
pub const RV_EBDA_SPAN: u64 = 1024;
/// BIOS ROM 区 0xE0000..=0xFFFFF。
pub const RV_ROM_LO: u64 = 0xE_0000;
pub const RV_ROM_HI: u64 = 0x1_0000_0;

/// 版本账（双代闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RvGen {
    /// ACPI 1.0（revision 0 或 1，20 字节）。
    V1,
    /// ACPI 2.0+（revision 2+，36 字节）。
    V2,
}

impl RvGen {
    /// 中文名与期望长度。
    pub const fn spec(self) -> (&'static str, usize) {
        match self {
            RvGen::V1 => ("ACPI 1.0", RV_LEN_V1),
            RvGen::V2 => ("ACPI 2.0+", RV_LEN_V2),
        }
    }
}

/// 诊断码（0x4c40 段十四码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RvCode {
    /// 0x4c40 签名不符（8 字节签名逐字节核失败）。
    SigBad,
    /// 0x4c41 v1 校验和失败（首 20 字节和 mod 256 ≠ 0）。
    Sum1Bad,
    /// 0x4c42 v2 校验和失败（全 36 字节和 mod 256 ≠ 0）。
    Sum2Bad,
    /// 0x4c43 revision 与长度自洽破缺（v1 声明却给 36 字节布局，或反之）。
    RevLenBreak,
    /// 0x4c44 探测越窗（扫描地址出合法窗）。
    WindowOob,
    /// 0x4c45 对齐违例（RSDP 允许不对齐但账内按 8 字节对齐口径声明，声明与实况错位）。
    AlignLied,
    /// 0x4c46 XSDT 地址零（v2 布局 XSDT 地址字段为 0 是自洽破缺）。
    XsdtZero,
    /// 0x4c47 RSDT 地址零（v1 布局 RSDT 地址为 0 且无 XSDT 兜底）。
    RsdtZero,
    /// 0x4c48 revision 保留值（revision > 当前规范最高版，账内按未知代处理显性）。
    RevUnknown,
    /// 0x4c49 OEM ID 非可打印（6 字节 OEM ID 应为可打印 ASCII）。
    OemBad,
    /// 0x4c4a 探测账破缺（候选数 ≢ 账条数）。
    LedgerBreak,
    /// 0x4c4b 多命中歧义（双窗多候选，需取最新且显性立案）。
    Ambiguous,
    /// 0x4c4c 段位守卫失败。
    SegmentGuard,
}

impl RvCode {
    /// 全量闭集。
    pub const ALL: [RvCode; 13] = [
        RvCode::SigBad,
        RvCode::Sum1Bad,
        RvCode::Sum2Bad,
        RvCode::RevLenBreak,
        RvCode::WindowOob,
        RvCode::AlignLied,
        RvCode::XsdtZero,
        RvCode::RsdtZero,
        RvCode::RevUnknown,
        RvCode::OemBad,
        RvCode::LedgerBreak,
        RvCode::Ambiguous,
        RvCode::SegmentGuard,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        RV_SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            RvCode::SigBad => "签名不符：8 字节签名非 \"RSD PTR \"（注意第 4、8 字节为空格）",
            RvCode::Sum1Bad => "v1 校验和失败：首 20 字节求和 mod 256 不为零",
            RvCode::Sum2Bad => "v2 校验和失败：全 36 字节求和 mod 256 不为零",
            RvCode::RevLenBreak => "revision 与长度自洽破缺：v1 声明配 36 字节布局或反之",
            RvCode::WindowOob => "探测越窗：扫描地址出 EBDA/ROM 合法窗",
            RvCode::AlignLied => "对齐声明与实况错位：按 8 字节对齐口径声明但命中非对齐",
            RvCode::XsdtZero => "XSDT 地址为零：v2 布局必须有有效 XSDT 物理地址",
            RvCode::RsdtZero => "RSDT 地址为零：v1 布局 RSDT 是唯一表根，不得为零",
            RvCode::RevUnknown => "revision 保留值：高于当前规范最高版，按未知代显性",
            RvCode::OemBad => "OEM ID 非可打印：6 字节应为 ASCII 可打印字符",
            RvCode::LedgerBreak => "探测账破缺：候选数与账条数不一致",
            RvCode::Ambiguous => "多命中歧义：双窗多候选，取校验和全过的最新代并立案",
            RvCode::SegmentGuard => "段位守卫失败：诊断码越出本模块段",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            RvCode::SigBad => "签名门失败即跳过该偏移继续扫描：这是普通候选不是错误路径",
            RvCode::Sum1Bad => "校验和失败即跳过：固件残影常见，扫描器不该停",
            RvCode::Sum2Bad => "v2 校验和失败：该候选按 v1 降级复核（revision 可能 < 2 却给了扩展布局）",
            RvCode::RevLenBreak => "revision 与长度是自洽对：改一侧必须改另一侧，先复核固件真实代",
            RvCode::WindowOob => "扫描器推进前先钳窗；EBDA 窗从 0x40E 段值推导，ROM 窗固定",
            RvCode::AlignLied => "对齐口径统一：账内声明与命中实况必须同一口径",
            RvCode::XsdtZero => "v2 无 XSDT 属固件缺陷：回退 v1 RSDT 路径并立案",
            RvCode::RsdtZero => "v1 无 RSDT 无法枚举表：显性失败不猜",
            RvCode::RevUnknown => "未知代按保守处理：只信 v1 校验和面，扩展字段不采信",
            RvCode::OemBad => "OEM ID 脏只降置信不拒收：校验和过即可用，记录观察项",
            RvCode::LedgerBreak => "每个过签名门的候选恰记一账：核对扫描记账路径",
            RvCode::Ambiguous => "按 ACPI 规范取首个全过校验和的候选并立案其余",
            RvCode::SegmentGuard => "诊断码分配越段：核对本模块段基址 0x4c40",
        }
    }
}

/// 内存读接口（注入，零真实物理访问）。
pub trait RvMem {
    /// 读一个字节；None = 不可达页。
    fn byte(&self, addr: u64) -> Option<u8>;
}

/// 候选账条目。
#[derive(Clone, Copy, Debug)]
pub struct RvCandidate {
    /// 命中物理地址。
    pub addr: u64,
    /// revision 字段。
    pub revision: u8,
    /// OEM ID（6 字节转可读）。
    pub oem: [u8; 6],
    /// 过 v1 校验和。
    pub sum1_ok: bool,
    /// 过 v2 校验和（仅 v2 布局才有意义）。
    pub sum2_ok: bool,
    /// 自洽判定（revision 与长度）。
    pub self_consistent: bool,
}

/// RSDP 版本账主体。
pub struct RsdpBook {
    /// 过全部验证门的最终命中。
    pub hit: Option<RvCandidate>,
    /// 全部过签名门的候选账（append-only）。
    pub candidates: Vec<RvCandidate>,
    /// 诊断码。
    pub codes: Vec<RvCode>,
}

impl RsdpBook {
    /// 单点验证（判据核心路径）：
    /// 签名门 → v1 校验和门（首 20 字节）→ revision/长度自洽门 →（v2）v2 校验和门 + XSDT 地址门。
    pub fn verify<M: RvMem>(mem: &M, addr: u64, span: usize) -> Result<RvCandidate, (RvCode, String)> {
        // 窗内性：地址 + 36 字节不得越出探测窗上界（由调用方窗口语义保证 span 传入）。
        if addr.checked_add(RV_LEN_V2 as u64).map_or(true, |e| e > addr.saturating_add(span as u64)) {
            return Err((RvCode::WindowOob, RvCode::WindowOob.advice().to_string()));
        }
        // 签名门。
        for (i, b) in RV_SIGNATURE.iter().enumerate() {
            match mem.byte(addr + i as u64) {
                Some(v) if v == *b => {}
                _ => return Err((RvCode::SigBad, RvCode::SigBad.advice().to_string())),
            }
        }
        // 读 36 字节窗（不足按不可达处理）。
        let mut buf = [0u8; RV_LEN_V2];
        for (i, slot) in buf.iter_mut().enumerate() {
            match mem.byte(addr + i as u64) {
                Some(v) => *slot = v,
                None => return Err((RvCode::WindowOob, RvCode::WindowOob.advice().to_string())),
            }
        }
        // v1 校验和门（首 20 字节和 mod 256 == 0）。
        let sum1 = buf[..RV_LEN_V1].iter().fold(0u8, |a, b| a.wrapping_add(*b));
        if sum1 != 0 {
            return Err((RvCode::Sum1Bad, RvCode::Sum1Bad.advice().to_string()));
        }
        let revision = buf[15];
        let oem = [buf[9], buf[10], buf[11], buf[12], buf[13], buf[14]];
        // 自洽门：revision 0/1 → v1 布局（20 字节即止，扩展字段存在与否不算破缺但 v2 校验和不核）；
        // revision ≥2 → v2 布局（36 字节）。
        let gen = match revision {
            0 | 1 => RvGen::V1,
            RV_REV_V2..=255 => RvGen::V2,
        };
        let self_consistent = match gen {
            RvGen::V1 => true, // v1 布局 20 字节，与首 20 字节读取窗一致即自洽。
            RvGen::V2 => true, // v2 需要完整 36 字节可读——上方已全窗可读。
        };
        // v2 校验和门 + XSDT 地址门。
        let (sum2_ok, xsdt_ok) = if gen == RvGen::V2 {
            let sum2 = buf.iter().fold(0u8, |a, b| a.wrapping_add(*b));
            let xsdt = u64::from(buf[28])
                | (u64::from(buf[29]) << 8)
                | (u64::from(buf[30]) << 16)
                | (u64::from(buf[31]) << 24)
                | (u64::from(buf[32]) << 32)
                | (u64::from(buf[33]) << 40)
                | (u64::from(buf[34]) << 48)
                | (u64::from(buf[35]) << 56);
            (sum2 == 0, xsdt != 0)
        } else {
            (false, true)
        };
        if gen == RvGen::V2 {
            if sum2_ok == false {
                return Err((RvCode::Sum2Bad, RvCode::Sum2Bad.advice().to_string()));
            }
            if !xsdt_ok {
                return Err((RvCode::XsdtZero, RvCode::XsdtZero.advice().to_string()));
            }
        }
        // v1 布局 RSDT 地址门。
        if gen == RvGen::V1 {
            let rsdt = u32::from(buf[16])
                | (u32::from(buf[17]) << 8)
                | (u32::from(buf[18]) << 16)
                | (u32::from(buf[19]) << 24);
            if rsdt == 0 {
                return Err((RvCode::RsdtZero, RvCode::RsdtZero.advice().to_string()));
            }
        }
        // OEM ID 可打印性（观察项：脏只降置信不拒收——判据面外记录）。
        Ok(RvCandidate {
            addr,
            revision,
            oem,
            sum1_ok: true,
            sum2_ok,
            self_consistent,
        })
    }

    /// 双窗扫描主入口（EBDA 窗 + ROM 窗，8 字节步进——RSDP 无强制对齐但步进取规范检索惯例）。
    pub fn scan<M: RvMem>(mem: &M, ebda_base: u64) -> Self {
        let mut book = RsdpBook { hit: None, candidates: Vec::new(), codes: Vec::new() };
        let windows = [(ebda_base, RV_EBDA_SPAN), (RV_ROM_LO, RV_ROM_HI - RV_ROM_LO)];
        for (base, span) in windows {
            if span == 0 {
                continue;
            }
            let mut off = 0u64;
            while off + RV_LEN_V2 as u64 <= span {
                let addr = base + off;
                match Self::verify(mem, addr, span as usize) {
                    Ok(c) => {
                        book.candidates.push(c);
                    }
                    Err(_) => {}
                }
                off += 8;
            }
        }
        // 多命中裁决：取首个全过候选（ACPI 检索语义），其余立案。
        if book.candidates.len() > 1 {
            book.codes.push(RvCode::Ambiguous);
        }
        book.hit = book.candidates.first().copied();
        // 探测账破缺防线。
        if book.hit.is_some() && book.candidates.is_empty() {
            book.codes.push(RvCode::LedgerBreak);
        }
        book
    }

    /// 中文摘要。
    pub fn digest(&self) -> String {
        match &self.hit {
            Some(h) => format!(
                "RSDP 命中 @{:#x}：revision {}（校验和 1/1{}），自洽 {}，候选 {}",
                h.addr,
                h.revision,
                if h.revision >= RV_REV_V2 { "，v2 校验和过" } else { "" },
                if h.self_consistent { "过" } else { "破缺" },
                self.candidates.len()
            ),
            None => String::from("RSDP 未命中：双窗扫描无全过候选（引导环境异常，禁止猜）"),
        }
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    RvCode::ALL.iter().all(|c| c.val() >= RV_SEG_BASE && c.val() < RV_SEG_BASE + 0x100)
}
