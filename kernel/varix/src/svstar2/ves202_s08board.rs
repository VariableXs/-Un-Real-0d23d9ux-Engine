#![allow(dead_code)]
//! VE-F3948 · 无障碍指标看板
//!
//! 全域无障碍指标看板（复用看板模式全域化）。指标集六项：断链率/延迟
//! P95/覆盖率/等价通过/豁免数/参与度——口径公开单源。看板视图顺序红线
//! 全域化六用：用户指标必须顶层。诚实红线：粉饰 → P0。权限复用。
//! 错误路径：顺序违→修正；粉饰→P0；口径分叉→单源；越权→断言。
//! 性能：指标 O(集)；顺序 O(六用)；权限 O(复用)；诚实 O(审计)。
//! 跨批对接：F2955/F3125/F3925 复用单源声明；全域指标对端；F3308 审计。
//! 无隐私面。

use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x0D80）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x0D80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S8HCode(pub u16);

impl S8HCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const ORDER_VIOLATION: Self = Self(SEG_BASE + 1);
    pub const WHITEWASH: Self = Self(SEG_BASE + 2);
    pub const CALIBER_SPLIT: Self = Self(SEG_BASE + 3);
    pub const OVER_REACH: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::ORDER_VIOLATION => "顺序违→修正",
            Self::WHITEWASH => "粉饰→P0",
            Self::CALIBER_SPLIT => "口径分叉→单源",
            Self::OVER_REACH => "越权→断言",
            _ => "未知",
        }
    }
}

// ── 指标集六项 ───────────────────────────────────────────────────────

/// 指标名（六项）。
pub const METRIC_NAMES: [&str; 6] = [
    "断链率", "延迟P95", "覆盖率", "等价通过", "豁免数", "参与度",
];

/// 单域指标快照（万分比整型口径，避免浮点）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricSnap {
    /// 断链率（万分比，越低越好）。
    pub break_rate_ppm: u32,
    /// 延迟 P95（毫秒，越低越好）。
    pub latency_p95_ms: u32,
    /// 覆盖率（万分比，10000 = 100%）。
    pub cover_ppm: u32,
    /// 等价通过率（万分比）。
    pub equiv_ppm: u32,
    /// 豁免数（个）。
    pub waivers: u32,
    /// 参与度（万分比）。
    pub engage_ppm: u32,
}

impl MetricSnap {
    /// 口径公开：返回指标名 → 值表（单源口径）。
    pub fn caliber(&self) -> [(&'static str, u32); 6] {
        [
            ("断链率", self.break_rate_ppm),
            ("延迟P95", self.latency_p95_ms),
            ("覆盖率", self.cover_ppm),
            ("等价通过", self.equiv_ppm),
            ("豁免数", self.waivers),
            ("参与度", self.engage_ppm),
        ]
    }

    /// 健康判定：覆盖率必须 100%，断链率必须 0。
    pub fn healthy(&self) -> bool {
        self.cover_ppm == 10_000 && self.break_rate_ppm == 0
    }
}

// ── 看板视图顺序（用户指标顶层）──────────────────────────────────────

/// 看板视图条目：指标 + 层级（0 = 用户顶层）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewItem {
    pub metric: &'static str,
    /// 层级：0 = 用户顶层，越大越靠内层。
    pub level: u8,
}

/// 顺序红线：用户指标（覆盖率/等价通过/参与度）必须在顶层。
#[derive(Debug, Clone)]
pub struct ViewOrder {
    pub items: Vec<ViewItem>,
}

pub const USER_TOP_METRICS: [&str; 3] = ["覆盖率", "等价通过", "参与度"];

impl ViewOrder {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn push(&mut self, metric: &'static str, level: u8) {
        self.items.push(ViewItem { metric, level });
    }

    /// 顺序违检出：用户指标不在顶层。
    pub fn violations(&self) -> Vec<&'static str> {
        USER_TOP_METRICS
            .iter()
            .filter(|m| {
                match self.items.iter().find(|i| i.metric == **m) {
                    Some(i) => i.level != 0,
                    None => true, // 缺席也算违
                }
            })
            .map(|m| *m)
            .collect()
    }

    /// 修正：全部用户指标归顶层。
    pub fn fix(&mut self) -> usize {
        let mut n = 0;
        for i in self.items.iter_mut() {
            if USER_TOP_METRICS.contains(&i.metric) && i.level != 0 {
                i.level = 0;
                n += 1;
            }
        }
        n
    }
}

impl Default for ViewOrder {
    fn default() -> Self {
        Self::new()
    }
}

// ── 诚实审计（粉饰 P0）───────────────────────────────────────────────

/// 看板展示值 vs 真实值对拍。
#[derive(Debug, Clone)]
pub struct HonestyAudit {
    /// (指标名, 展示值, 真实值)。
    pub pairs: Vec<(&'static str, u32, u32)>,
}

impl HonestyAudit {
    pub fn new() -> Self {
        Self { pairs: Vec::new() }
    }

    pub fn record(&mut self, metric: &'static str, shown: u32, truth: u32) {
        self.pairs.push((metric, shown, truth));
    }

    /// 粉饰检出：展示值好于真实值 = 粉饰（P0）。
    pub fn whitewashed(&self) -> Vec<&'static str> {
        self.pairs
            .iter()
            .filter(|(_, s, t)| s != t)
            .map(|(m, _, _)| *m)
            .collect()
    }

    /// 诚实修正：展示值回归真实值。
    pub fn truthful(&self) -> bool {
        self.pairs.iter().all(|(_, s, t)| s == t)
    }
}

impl Default for HonestyAudit {
    fn default() -> Self {
        Self::new()
    }
}

// ── 口径单源（分叉检出）──────────────────────────────────────────────

/// 口径注册表：每指标一个单源口径版本。
#[derive(Debug, Clone)]
pub struct CaliberBook {
    /// 指标名 → 口径版本。
    pub versions: Vec<(&'static str, u32)>,
}

impl CaliberBook {
    pub fn new() -> Self {
        Self { versions: Vec::new() }
    }

    pub fn declare(&mut self, metric: &'static str, ver: u32) -> S8HCode {
        if self.versions.iter().any(|(m, _)| *m == metric) {
            return S8HCode::CALIBER_SPLIT;
        }
        self.versions.push((metric, ver));
        S8HCode::OK
    }

    /// 口径分叉检出：外部口径 ≠ 单源。
    pub fn split_of(&self, metric: &str, external: u32) -> bool {
        match self.versions.iter().find(|(m, _)| *m == metric) {
            Some((_, v)) => *v != external,
            None => true,
        }
    }

    pub fn single_source(&self, external: &[(&'static str, u32)]) -> bool {
        external.iter().all(|(m, v)| !self.split_of(m, *v))
    }
}

impl Default for CaliberBook {
    fn default() -> Self {
        Self::new()
    }
}

// ── 权限（越权断言）──────────────────────────────────────────────────

/// 看板权限：只读消费方不得改指标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardRole {
    /// 只读消费方。
    Viewer,
    /// 指标维护方。
    Maintainer,
}

pub fn can_edit(role: BoardRole) -> bool {
    role == BoardRole::Maintainer
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves202_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves202_s08board");

    // 01 指标集六项齐备
    {
        let m = MetricSnap { break_rate_ppm: 0, latency_p95_ms: 40, cover_ppm: 10_000, equiv_ppm: 9_800, waivers: 1, engage_ppm: 9_000 };
        let cal = m.caliber();
        chk(&mut set, "C3948-01", cal.len() == 6 && cal[0].0 == "断链率" && cal[5].0 == "参与度", "err");
    }
    // 02 健康判定：覆盖 100% + 断链 0
    {
        let good = MetricSnap { break_rate_ppm: 0, latency_p95_ms: 30, cover_ppm: 10_000, equiv_ppm: 9_900, waivers: 0, engage_ppm: 8_000 };
        let bad = MetricSnap { break_rate_ppm: 12, latency_p95_ms: 30, cover_ppm: 9_900, equiv_ppm: 9_900, waivers: 0, engage_ppm: 8_000 };
        chk(&mut set, "C3948-02", good.healthy() && !bad.healthy(), "err");
    }
    // 03 顺序红线：用户指标顶层
    {
        let mut v = ViewOrder::new();
        v.push("覆盖率", 0);
        v.push("等价通过", 0);
        v.push("参与度", 0);
        v.push("断链率", 1);
        chk(&mut set, "C3948-03", v.violations().is_empty(), "err");
    }
    // 04 顺序违检出实测
    {
        let mut v = ViewOrder::new();
        v.push("覆盖率", 2);
        v.push("断链率", 0);
        chk(&mut set, "C3948-04", v.violations() == vec!["覆盖率", "等价通过", "参与度"], "err");
    }
    // 05 顺序修正归位
    {
        let mut v = ViewOrder::new();
        v.push("参与度", 3);
        v.push("断链率", 0);
        let n = v.fix();
        chk(&mut set, "C3948-05", n == 1 && v.items[0].level == 0 && v.violations() == vec!["覆盖率", "等价通过"], "err");
    }
    // 06 诚实审计：真实一致
    {
        let mut h = HonestyAudit::new();
        h.record("覆盖率", 10_000, 10_000);
        h.record("参与度", 8_500, 8_500);
        chk(&mut set, "C3948-06", h.truthful() && h.whitewashed().is_empty(), "err");
    }
    // 07 粉饰→P0 实测
    {
        let mut h = HonestyAudit::new();
        h.record("覆盖率", 10_000, 9_600);
        chk(&mut set, "C3948-07", h.whitewashed() == vec!["覆盖率"] && !h.truthful(), "err");
    }
    // 08 展示虚低也判不符（诚实是双向的）
    {
        let mut h = HonestyAudit::new();
        h.record("豁免数", 2, 5);
        chk(&mut set, "C3948-08", h.whitewashed().len() == 1, "err");
    }
    // 09 口径单源注册
    {
        let mut c = CaliberBook::new();
        chk(&mut set, "C3948-09", c.declare("断链率", 1) == S8HCode::OK && c.declare("断链率", 2) == S8HCode::CALIBER_SPLIT, "err");
    }
    // 10 口径分叉检出
    {
        let mut c = CaliberBook::new();
        c.declare("延迟P95", 3);
        chk(&mut set, "C3948-10", c.split_of("延迟P95", 4) && !c.split_of("延迟P95", 3) && c.split_of("无此项", 3), "err");
    }
    // 11 外部口径对拍单源
    {
        let mut c = CaliberBook::new();
        c.declare("覆盖率", 2);
        c.declare("参与度", 1);
        let ext = vec![("覆盖率", 2u32), ("参与度", 1u32)];
        chk(&mut set, "C3948-11", c.single_source(&ext), "err");
    }
    // 12 权限越权断言
    {
        chk(&mut set, "C3948-12", !can_edit(BoardRole::Viewer) && can_edit(BoardRole::Maintainer), "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3948-13", S8HCode::OVER_REACH.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S8HCode::OK, S8HCode::ORDER_VIOLATION, S8HCode::WHITEWASH, S8HCode::CALIBER_SPLIT, S8HCode::OVER_REACH];
        chk(&mut set, "C3948-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s08_board_all_green() {
        let s = run_ves202_checks();
        assert!(s.all_passed(), "ves202 red: {}", s.red_items_text());
    }

    #[test]
    fn test_order_full_fix() {
        let mut v = ViewOrder::new();
        for m in USER_TOP_METRICS.iter() {
            v.push(m, 5);
        }
        assert_eq!(v.violations().len(), 3);
        assert_eq!(v.fix(), 3);
        assert!(v.violations().is_empty());
    }

    #[test]
    fn test_honesty_redline() {
        let mut h = HonestyAudit::new();
        h.record("等价通过", 9_500, 9_500);
        h.record("断链率", 0, 30);
        assert_eq!(h.whitewashed(), vec!["断链率"]);
    }
}
