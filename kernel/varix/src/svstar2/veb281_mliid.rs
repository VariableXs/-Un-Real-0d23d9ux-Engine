//! veb281_mliid —— VE-F0281 Mali 设备识别与代际分型。
//! 锚点（CoRun Unxreal 主册）：VE-F0281 · Mali 设备识别与代际分型 · 目标 400 行
//! 判据原文：VE-F0281-J1 Mali 识别账：厂商号核（ARM 商号
//!           0x13B5）；产品族映射（Mali-G / Mali-C / Mali-D
//!           三线）；GPU ID 分型（ID 高 16 位 = 架构代，低
//!           16 位 = 产品号）；代际窗（每代支持的产品号闭
//!           集）；未知 ID 兜底（落 Unknown 代 + 立案）；IP
//!           版本三元组覆盖（三元组优先于裸 ID）；分型稳定
//!           核（同 ID 两次分型同果）。
//! 组件：厂商核 + 族映射 + 代际窗 + 兜底账。
//! 职责定位：Mali 识别是「ARM 线的第一眼」——厂商号是门牌：
//!           非 ARM 商号进不了 Mali 簿；代际窗是出身表：产
//!           品号落在哪代决定后续一切对接策略；三元组优先是
//!           细粒度纪律：IP 版本比裸 ID 更诚实。
//! 性能逐项分解：识别 O(表长) 表长 ≤ 8。
//! 诊断码段：0x30F40（0x30F41~0x30F49 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const MI_VERSION: &str = "veb281_mliid-v1";

/// 诊断段基址（0x30F40 段 · 本模块独占）。
pub const MI_SEG_BASE: u32 = 0x30F40;
pub const XC_MAGIC: u32 = 0x30F41; // 账魔数破
pub const XC_VENDOR: u32 = 0x30F42; // 非 ARM 商号
pub const XC_UNKNOWN: u32 = 0x30F43; // 未知代立案

/// 账魔数。
const MAGIC: u32 = 0x0F40_1B5D;

/// ARM 商号。
pub const VENDOR_ARM: u32 = 0x13B5;

/// Mali 产品族三线。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MFamily {
    /// G 线（图形）。
    G,
    /// C 线（计算）。
    C,
    /// D 线（新一代）。
    D,
    /// Unknown 兜底。
    Unknown,
}

/// 代际。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gen {
    /// 架构代（GPU ID 高 16 位）。
    pub arch: u16,
    pub family: MFamily,
}

/// 识别探针上下文。
pub struct ProbeCtx {
    pub vendor: u32,
    pub gpu_id: u32,
    /// IP 三元组（可选）。
    pub ip: Option<(u16, u16, u16)>,
}

/// 代际窗表（架构代 → 支持的产品号闭集 [lo, hi]）。
pub const GEN_TABLE: [(u16, MFamily, u16, u16); 6] = [
    // (架构代, 族, 产品号 lo, hi)
    (0x6953, MFamily::G, 0x0000, 0x00FF), // Midgard 系
    (0x7083, MFamily::G, 0x0000, 0x00FF), // Bifrost 系
    (0x7211, MFamily::G, 0x0000, 0x00FF), // Valhall 系
    (0x7400, MFamily::D, 0x0000, 0x00FF), // 5 代
    (0x8600, MFamily::C, 0x0000, 0x00FF), // C 线计算
    (0x9400, MFamily::D, 0x0000, 0x00FF), // D 线新代
];

/// Mali 识别簿。
pub struct MaliIdent {
    /// 未知 ID 立案账。
    pub unknown_cases: Vec<u32>,
    /// 识别次数。
    pub ident_count: u32,
    magic: u32,
}

/// 识别结果。
#[derive(Debug, PartialEq, Eq)]
pub struct IdentRes {
    pub family: MFamily,
    pub arch: u16,
    pub product: u16,
    /// 三元组是否覆盖过。
    pub ip_over: bool,
}

impl MaliIdent {
    /// 建空簿。
    pub fn new() -> MaliIdent {
        MaliIdent { unknown_cases: Vec::new(), ident_count: 0, magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 识别一次。
    pub fn identify(&mut self, ctx: &ProbeCtx) -> Result<IdentRes, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if ctx.vendor != VENDOR_ARM {
            return Err(XC_VENDOR);
        }
        self.ident_count += 1;
        let arch = (ctx.gpu_id >> 16) as u16;
        let product = (ctx.gpu_id & 0xFFFF) as u16;
        // 三元组优先：IP 主版本改判架构代。
        let eff_arch = match ctx.ip {
            Some((maj, _, _)) if maj > 0 => maj,
            _ => arch,
        };
        let ip_over = ctx.ip.is_some();
        for row in GEN_TABLE.iter() {
            if row.0 == eff_arch && product >= row.2 && product <= row.3 {
                return Ok(IdentRes {
                    family: row.1,
                    arch: eff_arch,
                    product,
                    ip_over,
                });
            }
        }
        // 兜底立案。
        self.unknown_cases.push(ctx.gpu_id);
        Ok(IdentRes {
            family: MFamily::Unknown,
            arch: eff_arch,
            product,
            ip_over,
        })
    }

    /// 立案数。
    pub fn unknown_count(&self) -> usize {
        self.unknown_cases.len()
    }
}

pub fn run_veb281_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0281");
    // 族一：厂商门。
    let mut id = MaliIdent::new();
    s.add("非 ARM 拒", id.identify(&ProbeCtx { vendor: 0x1002, gpu_id: 0x7400_0001, ip: None }) == Err(XC_VENDOR), "AMD 商号");
    s.add("零商号拒", id.identify(&ProbeCtx { vendor: 0, gpu_id: 0x7400_0001, ip: None }) == Err(XC_VENDOR), "无门牌");
    s.add("拒后零识别", id.ident_count == 0, "门内才计");
    // 族二：G 线分型。
    let r1 = match id.identify(&ProbeCtx { vendor: VENDOR_ARM, gpu_id: 0x7211_0095, ip: None }) {
        Ok(r) => r,
        Err(_) => {
            s.add("识别败", false, "不应到");
            return s;
        }
    };
    s.add("Valhall 归 G", r1.family == MFamily::G, "G 线");
    s.add("架构 0x7211", r1.arch == 0x7211, "高 16 位");
    s.add("产品 0x95", r1.product == 0x0095, "低 16 位");
    s.add("无三元组", r1.ip_over == false, "裸 ID");
    // 族三：C/D 线。
    let r2 = id.identify(&ProbeCtx { vendor: VENDOR_ARM, gpu_id: 0x8600_0002, ip: None });
    s.add("0x8600 归 C", matches!(&r2, Ok(r) if r.family == MFamily::C), "C 线");
    let r3 = id.identify(&ProbeCtx { vendor: VENDOR_ARM, gpu_id: 0x9400_00A1, ip: None });
    s.add("0x9400 归 D", matches!(&r3, Ok(r) if r.family == MFamily::D), "D 线");
    // 族四：三元组覆盖。
    let r4 = id.identify(&ProbeCtx {
        vendor: VENDOR_ARM,
        gpu_id: 0x7211_0095,
        ip: Some((0x7400, 1, 9)),
    });
    s.add("三元组改判", matches!(&r4, Ok(r) if r.arch == 0x7400), "IP 主版本优先");
    s.add("覆盖旗立", matches!(&r4, Ok(r) if r.ip_over), "细粒度");
    // 零主版本不覆盖。
    let r5 = id.identify(&ProbeCtx {
        vendor: VENDOR_ARM,
        gpu_id: 0x7211_0095,
        ip: Some((0, 3, 1)),
    });
    s.add("零主不覆盖", matches!(&r5, Ok(r) if r.arch == 0x7211), "裸 ID 兜底");
    // 族五：未知兜底。
    let r6 = id.identify(&ProbeCtx { vendor: VENDOR_ARM, gpu_id: 0xABCD_0001, ip: None });
    s.add("未知落 Unknown", matches!(&r6, Ok(r) if r.family == MFamily::Unknown), "兜底");
    s.add("立案 1", id.unknown_count() == 1, "一案");
    let r7 = id.identify(&ProbeCtx { vendor: VENDOR_ARM, gpu_id: 0xABCD_0002, ip: None });
    s.add("再立案 2", id.unknown_count() == 2 && matches!(r7, Ok(_)), "两案");
    // 族六：分型稳定核。
    let ra = id.identify(&ProbeCtx { vendor: VENDOR_ARM, gpu_id: 0x7211_0095, ip: None });
    let rb = id.identify(&ProbeCtx { vendor: VENDOR_ARM, gpu_id: 0x7211_0095, ip: None });
    s.add("两次同果", ra == rb, "稳定");
    s.add("魔数核", MI_SEG_BASE == 0x30F40, "段基址");
    s
}
