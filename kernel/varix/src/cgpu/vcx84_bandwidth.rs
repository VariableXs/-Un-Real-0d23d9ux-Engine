//! CGPU-F3684 · 带宽演进规划（X 域 CGPU 段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3684`
//!
//! **判据（锚点原文）**：HBM、复用声明、两组、判据。
//!
//! **职责定位（锚点原文）**：带宽：带宽演进规划（HBM 演进适配——
//! 带宽实现（J 域复用——复用声明；测试（带宽/复用两组）。
//!
//! # 一、HBM 演进适配（代际阶梯）
//!
//! [`HbmGen`] 五代闭集（HBM2/HBM2E/HBM3/HBM3E/HBM4）；每代钉死
//! 三列（带宽 GB/s/容量 GB/接口位宽）——**代际参数是常量不是
//! 估计**（升级换卡时的预算重算吃这个表）。
//!
//! [`bandwidth_demand`] 算需求（吞吐×帧预算反推）；[`fit_verdict`]
//! 按代际裁决（需求≤代际带宽=装得下；超出=显性拒——不许把
//! HBM2 的卡按 HBM4 规划）。
//!
//! # 二、J 域复用声明（复用是显性登记不是口头声明）
//!
//! 带宽实现的复用源在 J 域（vcj01_powerarch 家族）。[`ReuseDecl`]
//! 三行声明（源域/源单号/复用条目）——复用条目须在册
//! （[`reuse_valid`] 空源号即拒，同 vck03 ReuseOrigin 纪律：
//! 口头复用=无复用，登记才算）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const BW_VERSION: &str = "X84-bandwidth-v1";

/// 代际带宽不足（需求超代际上限——显性拒）。
pub const E_BW_OVERFLOW: &str = "E_BW_OVERFLOW";

/// 代际不存在（五代闭集外）。
pub const E_BW_UNKNOWN_GEN: &str = "E_BW_UNKNOWN_GEN";

/// 复用源号缺失（口头复用=无复用）。
pub const E_BW_REUSE: &str = "E_BW_REUSE";

/// HBM 代际（五代闭集——演进阶梯的骨架）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HbmGen {
    /// HBM2（初代）。
    Hbm2,
    /// HBM2E。
    Hbm2E,
    /// HBM3。
    Hbm3,
    /// HBM3E。
    Hbm3E,
    /// HBM4（当前代）。
    Hbm4,
}

impl HbmGen {
    /// 五代闭集（按代际序——演进方向即序）。
    pub const ALL: [HbmGen; 5] = [
        HbmGen::Hbm2,
        HbmGen::Hbm2E,
        HbmGen::Hbm3,
        HbmGen::Hbm3E,
        HbmGen::Hbm4,
    ];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            HbmGen::Hbm2 => "HBM2",
            HbmGen::Hbm2E => "HBM2E",
            HbmGen::Hbm3 => "HBM3",
            HbmGen::Hbm3E => "HBM3E",
            HbmGen::Hbm4 => "HBM4",
        }
    }

    /// 代际带宽（GB/s——公开规格常量，升级重算吃这个）。
    pub fn bandwidth_gbps(self) -> u32 {
        match self {
            HbmGen::Hbm2 => 256,
            HbmGen::Hbm2E => 307,
            HbmGen::Hbm3 => 512,
            HbmGen::Hbm3E => 819,
            HbmGen::Hbm4 => 1_638,
        }
    }

    /// 代际容量（GB——堆栈深度规格）。
    pub fn capacity_gb(self) -> u32 {
        match self {
            HbmGen::Hbm2 => 4,
            HbmGen::Hbm2E => 8,
            HbmGen::Hbm3 => 16,
            HbmGen::Hbm3E => 24,
            HbmGen::Hbm4 => 32,
        }
    }

    /// 接口位宽（bit——物理层约束）。
    pub fn bus_width(self) -> u32 {
        match self {
            HbmGen::Hbm2 => 1_024,
            HbmGen::Hbm2E => 1_024,
            HbmGen::Hbm3 => 1_024,
            HbmGen::Hbm3E => 2_048,
            HbmGen::Hbm4 => 4_096,
        }
    }

    /// 代际序位（0-4——演进阶梯定位）。
    pub fn ordinal(self) -> usize {
        HbmGen::ALL.iter().position(|g| *g == self).unwrap_or(HbmGen::ALL.len())
    }
}

// ---------------------------------------------------------------------------
// 二、带宽需求与代际裁决
// ---------------------------------------------------------------------------

/// 带宽需求裁决（需求≤代际带宽=Fit；超出=Overflow 显性拒）。
///
/// 需求按**每帧字节×目标帧率**折算（带宽的单位是 GB/s，帧预算吃
/// 的是每帧字节——两个口径必须显式换算，混用即错）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 装得下（需求≤代际带宽）。
    Fit,
    /// 超限（需求>代际带宽——显性拒）。
    Overflow,
}

impl Verdict {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Verdict::Fit => "装得下",
            Verdict::Overflow => "超限",
        }
    }
}

/// 带宽需求（GB/s）。
///
/// `bytes_per_frame` 每帧字节、`fps` 目标帧率——折算
/// `bytes×fps / 1e9`（GB/s 口径；整数先乘后除防过早截断）。
pub fn bandwidth_demand(bytes_per_frame: u64, fps: u64) -> u64 {
    if fps == 0 {
        return 0;
    }
    bytes_per_frame
        .saturating_mul(fps)
        .saturating_div(1_000_000_000)
        .max(1)
}

/// 代际裁决（需求 vs 代际带宽；闭集外代际即拒）。
pub fn fit_verdict(gen: HbmGen, demand_gbps: u64) -> Result<Verdict, String> {
    if !HbmGen::ALL.contains(&gen) {
        return Err(format!("{}：代际 {} 不在五代闭集", E_BW_UNKNOWN_GEN, gen.zh()));
    }
    if demand_gbps > gen.bandwidth_gbps() as u64 {
        Ok(Verdict::Overflow)
    } else {
        Ok(Verdict::Fit)
    }
}

/// 演进建议（按需求挑最低够用代际——不浪费也不冒进）。
///
/// 沿代际序向上找第一个装得下的；全不够=顶层代际+显性超限
/// （不许"再忍忍"——忍出来的帧时间是用户付账）。
pub fn recommend_gen(demand_gbps: u64) -> Result<HbmGen, String> {
    for g in HbmGen::ALL.iter() {
        if demand_gbps <= g.bandwidth_gbps() as u64 {
            return Ok(*g);
        }
    }
    Err(format!(
        "{}：需求 {} GB/s 超五代闭集最高 HBM4 {}——须降负载或等新代际",
        E_BW_OVERFLOW,
        demand_gbps,
        HbmGen::Hbm4.bandwidth_gbps()
    ))
}

// ---------------------------------------------------------------------------
// 三、J 域复用声明
// ---------------------------------------------------------------------------

/// 复用声明（源域/源单号/复用条目三件）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReuseDecl {
    /// 源域标识。
    pub source_domain: &'static str,
    /// 源任务单号（空=口头复用=无复用）。
    pub origin: &'static str,
    /// 复用条目（带宽实现借用了什么）。
    pub item: &'static str,
}

/// 复用有效性（源单号非空——登记才算复用）。
///
/// 同 vck03 ReuseOrigin 纪律：没有单号的复用声明是口头声明，
/// 口头声明不构成实现依据。
pub fn reuse_valid(decl: &ReuseDecl) -> Result<(), String> {
    if decl.origin.trim().is_empty() {
        return Err(format!(
            "{}：复用条目 {} 缺源单号（源域 {}）——口头复用=无复用",
            E_BW_REUSE, decl.item, decl.source_domain
        ));
    }
    if decl.item.trim().is_empty() {
        return Err(format!("{}：复用条目名为空（源 {}）", E_BW_REUSE, decl.origin));
    }
    Ok(())
}

/// 本模块的复用声明表（J 域 vcj01_powerarch 家族）。
pub fn reuse_decls() -> [ReuseDecl; 2] {
    [
        ReuseDecl {
            source_domain: "cgpu-J",
            origin: "CGPU-F1441",
            item: "功耗域预算编排形式（功耗账户复用到带宽账户）",
        },
        ReuseDecl {
            source_domain: "cgpu-J",
            origin: "CGPU-F1444",
            item: "域预算基线校准口径（基线与实测偏差回灌）",
        },
    ]
}

// ---------------------------------------------------------------------------
// 四、域自检
// ---------------------------------------------------------------------------

/// CGPU-F3684 域自检（判据：带宽/复用两组）。
pub fn run_vcx84_checks() -> CheckSet {
    let mut s = CheckSet::new("X84-bandwidth");

    // -- X84-HBM-01 五代闭集（演进序+三列规格非单调漏项）-----------------
    {
        let mut ok = HbmGen::ALL.len() == 5;
        let mut prev_bw = 0u32;
        let mut prev_cap = 0u32;
        for g in HbmGen::ALL.iter() {
            let bw = g.bandwidth_gbps();
            let cap = g.capacity_gb();
            if bw < prev_bw {
                ok = false;
            }
            if cap < prev_cap {
                ok = false;
            }
            prev_bw = bw;
            prev_cap = cap;
        }
        s.add(
            "X84-HBM-01 五代闭集（带宽/容量逐代不倒退——演进序单调）",
            ok && HbmGen::Hbm4.bandwidth_gbps() == 1_638,
            "锚点「HBM 演进适配」：代际规格单调演进是规划的前提",
        );
    }

    // -- X84-HBM-02 需求折算（每帧字节×帧率→GB/s）------------------------
    {
        // 4GiB×60fps ≈ 257 GB/s（4×2^30×60/1e9=257.7 取整 257）。
        let d = bandwidth_demand(4 * 1024 * 1024 * 1024, 60);
        s.add(
            "X84-HBM-02 需求折算（4GiB×60fps≈257GB/s —— 先乘后除不早截断）",
            d == 257,
            "锚点带宽口径：字节×帧率折算 GB/s，两口径显式换算",
        );
    }

    // -- X84-HBM-03 代际裁决（Fit/Overflow 两向）--------------------------
    {
        let fit = fit_verdict(HbmGen::Hbm3, 400);
        let over = fit_verdict(HbmGen::Hbm2, 400);
        s.add(
            "X84-HBM-03 代际裁决（400GB/s：HBM3 装得下/HBM2 显性超限）",
            fit == Ok(Verdict::Fit) && over == Ok(Verdict::Overflow),
            "锚点代际适配：不许把 HBM2 的卡按 HBM3 规划",
        );
    }

    // -- X84-HBM-04 演进建议（最低够用代际+全不够显性拒）-----------------
    {
        let rec = recommend_gen(600);
        let none = recommend_gen(9_999);
        s.add(
            "X84-HBM-04 演进建议（600GB/s→HBM3E；需求 9999 超闭集显性拒）",
            rec == Ok(HbmGen::Hbm3E) && none.is_err() && none.unwrap_err().contains(E_BW_OVERFLOW),
            "锚点演进规划：最低够用不浪费，全不许冒进",
        );
    }

    // -- X84-REUSE-01 复用声明两行齐备（J 域源号在册）--------------------
    {
        let decls = reuse_decls();
        let ok = decls.len() == 2 && decls.iter().all(|d| reuse_valid(d).is_ok());
        s.add(
            "X84-REUSE-01 复用声明在册（J 域 F1441/F1444 源号非空）",
            ok,
            "锚点「带宽实现（J 域复用——复用声明」",
        );
    }

    // -- X84-REUSE-02 口头复用拒收（缺源单号=无复用）---------------------
    {
        let bad = ReuseDecl {
            source_domain: "cgpu-J",
            origin: "   ",
            item: "功耗域预算编排形式",
        };
        let r = reuse_valid(&bad);
        s.add(
            "X84-REUSE-02 口头复用拒收（缺源单号=无复用——登记才算）",
            r.is_err() && r.unwrap_err().contains(E_BW_REUSE),
            "锚点复用声明纪律：同 vck03 ReuseOrigin",
        );
    }

    // -- X84-META-01 版本指纹 ----------------------------------------------
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in BW_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    };
    s.add("X84-META-01 版本指纹非零", fp != 0, "X84-bandwidth-v1");

    s
}
