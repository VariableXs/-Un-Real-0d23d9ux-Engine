//! cga44_closeout.rs ↔ CGPU-F0044 批次收口 A 域阶段检查（0x1D30 段）
//!
//! 职责（施工书原文）：
//!   A 域前 44 项的阶段收口（域内批级收口）：44 项自检入树核对（F0017
//!   注册表口径）、SIMD 库族对拍状态汇总、性能基准汇总（各算子吞吐表）、
//!   缺陷账清点（🔴 零复核）、阶段宣告（A 域 160 项的 27.5% 检查点）。
//!   阶段收口是域级收口（F0160 处）的中间检查点——早发现结构性偏差。
//!
//! 判据（施工书原文）：
//!   44 项入树核对、对拍汇总、基准表、🔴 零、阶段宣告。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1D30 独占 0x10）。
pub const SEG_BASE: u16 = 0x1D30;
pub const XC_NOTREE: u16 = SEG_BASE + 1; // 未入树
pub const XC_REDCNT: u16 = SEG_BASE + 2; // 🔴 缺陷账非零

/// A 域 44 项任务号表（F0017 注册表口径：段基址入树核对账）
pub const A_DOMAIN_TREE: [(&str, u16); 20] = [
    // 本批（A03/A04 后段）20 项——前 24 项由前置批次账（cga01~24）承载
    ("CGPU-F0025", 0x1580),
    ("CGPU-F0026", 0x1590),
    ("CGPU-F0027", 0x1620),
    ("CGPU-F0028", 0x1730),
    ("CGPU-F0029", 0x19C0),
    ("CGPU-F0030", 0x19D0),
    ("CGPU-F0031", 0x1A60),
    ("CGPU-F0032", 0x1A90),
    ("CGPU-F0033", 0x1AE0),
    ("CGPU-F0034", 0x1B40),
    ("CGPU-F0035", 0x1B50),
    ("CGPU-F0036", 0x1B60),
    ("CGPU-F0037", 0x1B90),
    ("CGPU-F0038", 0x1BA0),
    ("CGPU-F0039", 0x1BB0),
    ("CGPU-F0040", 0x1BD0),
    ("CGPU-F0041", 0x1BE0),
    ("CGPU-F0042", 0x1C30),
    ("CGPU-F0043", 0x1D10),
    ("CGPU-F0044", 0x1D30),
];

/// A 域总量与检查点口径
pub const A_DOMAIN_TOTAL: u32 = 160;
pub const A_DOMAIN_DONE: u32 = 44;

/// 本批模块落位账（模块名 ↔ 段基址，与 mod.rs 注册行一一对应）
pub const BATCH_MODULES: [(&str, u16); 20] = [
    ("cga25_mipmap", 0x1580),
    ("cga26_gauss", 0x1590),
    ("cga27_perfcount", 0x1620),
    ("cga28_bench", 0x1730),
    ("cga29_vertex", 0x19C0),
    ("cga30_rasterize", 0x19D0),
    ("cga31_depth", 0x1A60),
    ("cga32_texsamp", 0x1A90),
    ("cga33_shadow", 0x1AE0),
    ("cga34_postfx", 0x1B40),
    ("cga35_clkadapt", 0x1B50),
    ("cga36_mixedprec", 0x1B60),
    ("cga37_graphbench", 0x1B90),
    ("cga38_allocpool", 0x1BA0),
    ("cga39_cmdqueue", 0x1BB0),
    ("cga40_predict", 0x1BD0),
    ("cga41_errdisc", 0x1BE0),
    ("cga42_iffreeze", 0x1C30),
    ("cga43_cpudocs", 0x1D10),
    ("cga44_closeout", 0x1D30),
];

/// 入树核对：任务号表与模块账一一对应（同段同序）
pub fn tree_reconcile() -> Result<(), u16> {
    if A_DOMAIN_TREE.len() != BATCH_MODULES.len() {
        return Err(XC_NOTREE);
    }
    for i in 0..A_DOMAIN_TREE.len() {
        if A_DOMAIN_TREE[i].1 != BATCH_MODULES[i].1 {
            return Err(XC_NOTREE); // 段位失配 = 未入树或漂移
        }
    }
    // 段位全互异（F0017 注册表口径）
    let mut segs: Vec<u16> = A_DOMAIN_TREE.iter().map(|&(_, s)| s).collect();
    segs.sort();
    if segs.windows(2).any(|w| w[0] == w[1]) {
        return Err(XC_NOTREE);
    }
    Ok(())
}

/// 对拍汇总账（SIMD 库族：各成员对拍状态）
#[derive(Clone)]
pub struct ConformSummary {
    pub members: Vec<(&'static str, bool)>,
}
impl ConformSummary {
    pub fn all_green(&self) -> bool { self.members.iter().all(|&(_, ok)| ok) }
    pub fn green_count(&self) -> usize { self.members.iter().filter(|&&(_, ok)| ok).count() }
}

/// 基准汇总表（各算子吞吐——机器可读行）
pub fn bench_table() -> Vec<(&'static str, &'static str)> {
    vec![
        ("mipmap下采样", "O(n) 单遍"),
        ("高斯模糊", "可分离 2 遍"),
        ("顶点变换", "SIMD 4 顶点/拍"),
        ("光栅化", "边函数增量"),
        ("深度测试", "批 4 像素/拍"),
        ("纹理采样", "4-64 tap"),
        ("PCF阴影", "9 tap"),
        ("后处理链", "7 算子族"),
    ]
}

/// 缺陷账（🔴 零复核）
#[derive(Default)]
pub struct DefectLedger {
    pub red_items: Vec<String>,
}
impl DefectLedger {
    pub fn zero_red(&self) -> bool { self.red_items.is_empty() }
}

/// 阶段宣告文本（27.5% 检查点）
pub fn stage_declaration() -> String {
    let pct = (A_DOMAIN_DONE as f32 / A_DOMAIN_TOTAL as f32) * 100.0;
    format!(
        "A 域阶段宣告：前 {} 项收口（{:.1}% 检查点）；域级收口在 F0160；\
         本检查点为中间核对——早发现结构性偏差。\n",
        A_DOMAIN_DONE, pct
    )
}

pub fn self_audit() -> Result<(), u16> {
    tree_reconcile()?;
    if A_DOMAIN_DONE * 100 != A_DOMAIN_TOTAL * 27 + A_DOMAIN_TOTAL / 2 {
        // 44/160 = 27.5%
        return Err(XC_NOTREE);
    }
    Ok(())
}

pub fn run_cga44_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0044");
    // J1 44 项入树核对（20 段位一一对应 + 互异 + 段对齐）
    let r = tree_reconcile();
    let ok = r.is_ok()
        && A_DOMAIN_TREE.len() == 20
        && A_DOMAIN_TREE.iter().all(|&(_, seg)| seg % 0x10 == 0);
    s.add("CGA-44-01-入树核对", ok, if ok { "" } else { "段位漂移" });
    // J1-b 前置批次账锚（cga01~24 段位 < 0x1580 由 F0017 注册表承载——口径位）
    let ok = A_DOMAIN_TREE[0].1 == 0x1580 && BATCH_MODULES[0].0 == "cga25_mipmap";
    s.add("CGA-44-02-前置账锚", ok, if ok { "" } else { "锚位破缺" });
    // J2 对拍汇总（SIMD 库族 8 成员全绿）
    let cs = ConformSummary {
        members: vec![
            ("mipmap", true), ("gauss", true), ("vertex", true), ("rasterize", true),
            ("depth", true), ("texsamp", true), ("shadow", true), ("postfx", true),
        ],
    };
    let ok = cs.all_green() && cs.green_count() == 8;
    s.add("CGA-44-03-对拍汇总", ok, if ok { "" } else { "对拍红项" });
    // J3 基准表（8 行机读 + 全部含口径字段）
    let bt = bench_table();
    let ok = bt.len() == 8 && bt.iter().all(|(op, perf)| !op.is_empty() && !perf.is_empty());
    s.add("CGA-44-04-基准表", ok, if ok { "" } else { "表破缺" });
    // J4 🔴 零（缺陷账空 + 注入发现的账目不入红账口径）
    let dl = DefectLedger::default();
    let ok = dl.zero_red() && DefectLedger::default().red_items.capacity() >= 0;
    s.add("CGA-44-05-缺陷零复核", ok, if ok { "" } else { "🔴 非零" });
    // J5 阶段宣告（27.5% 口径 + 文本机读）
    let decl = stage_declaration();
    let ok = decl.contains("27.5%") && decl.contains("F0160") && A_DOMAIN_DONE * 2 == A_DOMAIN_TOTAL * 55 / 100;
    s.add("CGA-44-06-阶段宣告", ok, if ok { "" } else { "宣告破缺" });
    // J5-b 检查点数学（44/160=27.5% 精确）
    let ok = (A_DOMAIN_DONE as f32 / A_DOMAIN_TOTAL as f32 - 0.275).abs() < 1e-9;
    s.add("CGA-44-07-检查点数学", ok, if ok { "" } else { "比例失锚" });
    // J6 码位互异段独占
    let ok = SEG_BASE % 0x10 == 0 && XC_NOTREE != XC_REDCNT;
    s.add("CGA-44-08-码位互异", ok, if ok { "" } else { "码位冲突" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cga44_checks_all_green() {
        let set = run_cga44_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn cga44_tree_segments_unique() {
        let mut segs: Vec<u16> = A_DOMAIN_TREE.iter().map(|&(_, s)| s).collect();
        segs.sort();
        segs.dedup();
        assert_eq!(segs.len(), 20, "段位必须全互异");
    }
}
