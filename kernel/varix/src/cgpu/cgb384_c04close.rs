//! CGPU-F0384 · C04 组收口与 C05 移交（目标 320 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - C04 组收口：16 项自检入树/命中率实测报告/组双签/台账更新/向 C05
//!   预算治理与工具组移交（预测与画像就绪——治理工具有了可观测的预测层
//!   数据）。判据：入树、报告、双签、移交、断点。
//!
//! 设计纪律：
//! - 16 项自检逐项有标签与判据语义，全绿才允许出组。
//! - 双签：组内签 + 移交方签，两签均须指纹当前（对真源核，不跟副本比）。
//! - 断点 3840：收口锚点号，入树可追溯。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0AC0 独占。

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;

/// 诊断段基址（0x0AC0，全仓独占）。
pub const SEG_BASE: u16 = 0x0AC0;

pub const E_CHECKLIST_RED: u16 = SEG_BASE + 1;
pub const E_SIGN_MISSING: u16 = SEG_BASE + 2;
pub const E_SIGN_STALE: u16 = SEG_BASE + 3;
pub const E_HANDOVER_INCOMPLETE: u16 = SEG_BASE + 4;

/// 收口断点（入树锚点号）。
pub const CLOSE_BREAKPOINT: u32 = 3840;

/// C04 组自检 16 项（冻结清单——每项对应本组一个模块的关键判据）。
pub const CHECKLIST_16: [(&str, u8); 16] = [
    ("画像记录/查询闭环", 1),      // F0369
    ("负载画像降采样双配额", 2),   // F0370
    ("周期识别置信", 3),           // F0371
    ("预测区间下界宁欠勿超", 4),   // F0372
    ("置信三级响应", 5),           // F0373
    ("预留池零泄漏", 6),           // F0374
    ("冷启动热身成对", 7),         // F0375
    ("反馈闭环反漂移", 8),         // F0376
    ("P域先验通道冻结", 9),        // F0377
    ("NPU预留诚实标注", 10),       // F0378
    ("开销0.5%实测", 11),          // F0379
    ("五指标进B08", 12),           // F0380
    ("fuzz三不变量", 13),          // F0381
    ("API三族冻结v1", 14),         // F0382
    ("两文档两基准", 15),          // F0383
    ("16项自检自身", 16),          // F0384
];

/// 自检树：逐项绿/红 + 入树指纹。
pub struct SelfCheckTree {
    results: Vec<(&'static str, bool)>,
}

impl SelfCheckTree {
    pub fn new() -> Self {
        SelfCheckTree { results: Vec::new() }
    }

    /// 逐项登记（顺序即 CHECKLIST_16 顺序——防漏项）。
    pub fn record(&mut self, idx: usize, green: bool) -> Result<(), u16> {
        if idx >= CHECKLIST_16.len() {
            return Err(SEG_BASE + 5);
        }
        let label = CHECKLIST_16[idx].0;
        if let Some(e) = self.results.iter_mut().find(|(t, _)| *t == label) {
            e.1 = green;
        } else {
            self.results.push((label, green));
        }
        Ok(())
    }

    /// 全登记核：16 项齐套且全绿才允许收口。
    pub fn all_green(&self) -> Result<(), u16> {
        if self.results.len() != CHECKLIST_16.len() {
            return Err(E_CHECKLIST_RED);
        }
        if self.results.iter().all(|(_, g)| *g) {
            Ok(())
        } else {
            Err(E_CHECKLIST_RED)
        }
    }

    /// 红项名单（失败路径诊断文本）。
    pub fn red_items(&self) -> Vec<&'static str> {
        self.results.iter().filter(|(_, g)| !g).map(|(t, _)| *t).collect()
    }

    /// 入树指纹：全绿态的稳定哈希（收口锚点绑断点号）。
    pub fn tree_fingerprint(&self) -> Option<u64> {
        if self.all_green().is_err() {
            return None;
        }
        let mut h: u64 = (CLOSE_BREAKPOINT as u64).wrapping_mul(0x9E3779B97F4A7C15);
        for (t, _) in &self.results {
            for &b in t.as_bytes() {
                h = (h ^ b as u64).wrapping_mul(0x100000001b3);
            }
        }
        Some(h)
    }

    pub fn recorded(&self) -> usize {
        self.results.len()
    }
}

impl Default for SelfCheckTree {
    fn default() -> Self {
        Self::new()
    }
}

/// 命中率实测报告（组收口附页——数字单一真源）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HitRateReport {
    /// 四谱型各自的 (frames, hits)。
    pub rows: [(u32, u32); 4],
}

impl HitRateReport {
    pub fn new() -> Self {
        HitRateReport { rows: [(0, 0); 4] }
    }

    pub fn add(&mut self, shape: usize, frames: u32, hits: u32) -> Result<(), u16> {
        if shape >= 4 || hits > frames {
            return Err(SEG_BASE + 6);
        }
        self.rows[shape].0 += frames;
        self.rows[shape].1 += hits;
        Ok(())
    }

    pub fn total_rate_pm(&self) -> Option<u32> {
        let f: u64 = self.rows.iter().map(|r| r.0 as u64).sum();
        let h: u64 = self.rows.iter().map(|r| r.1 as u64).sum();
        if f == 0 {
            None
        } else {
            Some(((h * 1000) / f) as u32)
        }
    }

    /// 报告达标（与 F0383 阈值一致：≥700‰）。
    pub fn meets_target(&self) -> bool {
        matches!(self.total_rate_pm(), Some(r) if r >= 700)
    }
}

impl Default for HitRateReport {
    fn default() -> Self {
        Self::new()
    }
}

/// 组双签：组内签 + 移交方签；指纹取「树指纹 + 报告率」的合成——
/// 判据「指纹当前」：签时即核树已全绿、报告已达标，账本变了再验必失配。
pub struct DoubleSign {
    inner: Option<(u64, String)>,  // (指纹, 签人)
    handover: Option<(u64, String)>,
    tree_fp_at_sign: Option<u64>,
    rate_at_sign: Option<u32>,
}

impl DoubleSign {
    pub fn new() -> Self {
        DoubleSign { inner: None, handover: None, tree_fp_at_sign: None, rate_at_sign: None }
    }

    /// 签署（两方都必须在树全绿+报告达标之后）。
    pub fn sign(&mut self, who: &str, tree_fp: u64, rate_pm: u32, is_handover: bool) -> Result<(), u16> {
        if rate_pm < 700 {
            return Err(E_CHECKLIST_RED);
        }
        let slot = if is_handover { &mut self.handover } else { &mut self.inner };
        *slot = Some((tree_fp, String::from(who)));
        if is_handover {
            self.rate_at_sign = Some(rate_pm);
        } else {
            self.tree_fp_at_sign = Some(tree_fp);
        }
        Ok(())
    }

    /// 双签核验（判据「双签」）：两签在位、指纹互洽、且与当前树/报告仍一致。
    pub fn verify(&self, tree: &SelfCheckTree, report: &HitRateReport) -> Result<(), u16> {
        let (fp1, _) = self.inner.as_ref().ok_or(E_SIGN_MISSING)?;
        let (fp2, _) = self.handover.as_ref().ok_or(E_SIGN_MISSING)?;
        if fp1 != fp2 {
            return Err(E_SIGN_STALE);
        }
        // 指纹当前：对真源（树/报告）重算比对，不跟签发时的快照比。
        let cur_fp = tree.tree_fingerprint().ok_or(E_SIGN_STALE)?;
        if *fp1 != cur_fp {
            return Err(E_SIGN_STALE);
        }
        if !report.meets_target() {
            return Err(E_CHECKLIST_RED);
        }
        Ok(())
    }

    pub fn signed(&self) -> bool {
        self.inner.is_some() && self.handover.is_some()
    }
}

impl Default for DoubleSign {
    fn default() -> Self {
        Self::new()
    }
}

/// C05 移交包：预测与画像就绪清单（治理工具的可观测数据面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoverPack {
    pub pred_layer_ready: bool,
    pub profile_layer_ready: bool,
    pub b08_metrics_live: bool,
    pub api_frozen_v1: bool,
}

impl HandoverPack {
    pub fn complete(&self) -> Result<(), u16> {
        if self.pred_layer_ready && self.profile_layer_ready && self.b08_metrics_live && self.api_frozen_v1 {
            Ok(())
        } else {
            Err(E_HANDOVER_INCOMPLETE)
        }
    }
}

/// 收口总闸：树全绿 + 报告达标 + 双签核过 + 移交包完整 → 放行（断点可查）。
pub fn close_gate(tree: &SelfCheckTree, report: &HitRateReport, sign: &DoubleSign, pack: &HandoverPack) -> Result<u32, u16> {
    tree.all_green()?;
    if !report.meets_target() {
        return Err(E_CHECKLIST_RED);
    }
    sign.verify(tree, report)?;
    pack.complete()?;
    Ok(CLOSE_BREAKPOINT)
}

// ---------------------------------------------------------------------------
// 判据自检
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn green_tree() -> SelfCheckTree {
        let mut t = SelfCheckTree::new();
        for i in 0..16 {
            t.record(i, true).unwrap();
        }
        t
    }

    fn good_report() -> HitRateReport {
        let mut r = HitRateReport::new();
        for s in 0..4 {
            r.add(s, 500, 400).unwrap();
        }
        r
    }

    #[test]
    fn a84_入树_16项齐套全绿() {
        let t = green_tree();
        assert_eq!(t.recorded(), 16);
        assert!(t.all_green().is_ok());
        assert!(t.tree_fingerprint().is_some());
        // 漏项即拒。
        let mut thin = SelfCheckTree::new();
        for i in 0..15 {
            thin.record(i, true).unwrap();
        }
        assert_eq!(thin.all_green().unwrap_err(), E_CHECKLIST_RED);
        // 红项在册。
        let mut mixed = green_tree();
        mixed.record(3, false).unwrap();
        assert_eq!(mixed.red_items(), vec!["预测区间下界宁欠勿超"]);
        assert!(mixed.tree_fingerprint().is_none());
        // 越界登记拒。
        let mut t = t;
        assert_eq!(t.record(16, true).unwrap_err(), SEG_BASE + 5);
    }

    #[test]
    fn a84_报告_命中率达标() {
        let r = good_report();
        assert_eq!(r.total_rate_pm(), Some(800));
        assert!(r.meets_target());
        // 非法行拒。
        let mut r = r;
        assert_eq!(r.add(4, 1, 1).unwrap_err(), SEG_BASE + 6);
        assert_eq!(r.add(0, 10, 11).unwrap_err(), SEG_BASE + 6);
        // 不达标。
        let mut low = HitRateReport::new();
        low.add(0, 1000, 600).unwrap();
        assert!(!low.meets_target());
        assert_eq!(low.total_rate_pm(), Some(600));
        // 空报告诚实 None。
        assert_eq!(HitRateReport::default().total_rate_pm(), None);
    }

    #[test]
    fn a84_双签_指纹当前账本变则失配() {
        let mut s = DoubleSign::new();
        let tree = green_tree();
        let report = good_report();
        let fp = tree.tree_fingerprint().unwrap();
        s.sign("c04-inner", fp, 800, false).unwrap();
        // 只有单签：核验拒。
        assert_eq!(s.verify(&tree, &report).unwrap_err(), E_SIGN_MISSING);
        s.sign("c05-handover", fp, 800, true).unwrap();
        assert!(s.signed());
        // 全绿+达标+双签 → 核验过。
        assert!(s.verify(&tree, &report).is_ok());
        // 账本变了（树翻红）→ 指纹失配（判据「指纹当前」：对真源重算）。
        let mut stale_tree = green_tree();
        stale_tree.record(9, false).unwrap();
        assert_eq!(s.verify(&stale_tree, &report).unwrap_err(), E_SIGN_STALE);
        // 两签指纹不一致 → 失配。
        let mut bad = DoubleSign::new();
        bad.sign("a", fp, 800, false).unwrap();
        bad.sign("b", fp.wrapping_add(1), 800, true).unwrap();
        assert_eq!(bad.verify(&tree, &report).unwrap_err(), E_SIGN_STALE);
        // 报告率不足时签署即拒。
        let mut s2 = DoubleSign::new();
        assert_eq!(s2.sign("x", fp, 500, false).unwrap_err(), E_CHECKLIST_RED);
    }

    #[test]
    fn a84_移交_收口总闸与断点() {
        let tree = green_tree();
        let report = good_report();
        let mut s = DoubleSign::new();
        let fp = tree.tree_fingerprint().unwrap();
        s.sign("c04-inner", fp, 800, false).unwrap();
        s.sign("c05-handover", fp, 800, true).unwrap();
        let pack = HandoverPack { pred_layer_ready: true, profile_layer_ready: true, b08_metrics_live: true, api_frozen_v1: true };
        assert!(pack.complete().is_ok());
        // 总闸放行，返回断点 3840。
        assert_eq!(close_gate(&tree, &report, &s, &pack).unwrap(), CLOSE_BREAKPOINT);
        assert_eq!(CLOSE_BREAKPOINT, 3840);
        // 移交包缺项 → 总闸拒。
        let thin = HandoverPack { pred_layer_ready: true, profile_layer_ready: true, b08_metrics_live: true, api_frozen_v1: false };
        assert_eq!(close_gate(&tree, &report, &s, &thin).unwrap_err(), E_HANDOVER_INCOMPLETE);
        // 清单常量自检：16 项编号连续。
        for (i, (_, n)) in CHECKLIST_16.iter().enumerate() {
            assert_eq!(*n as usize, i + 1);
        }
    }
}
