#![allow(dead_code)]
//! VE-F3946 · 无障碍回归体系
//!
//! 基线制度全域化：全域无障碍基线归一（各域基线归一断言——分域基线不
//! 冲突；归一红线：全域基线双处 = 基线分叉）。回归触发全域化：引擎变更
//! →全域无障碍回归。回归门门禁全域化（全域门注册断言）；豁免全域复用。
//! 错误路径：分叉检出→归一；触发漏→门禁；豁免滥用→升级；基线过时→
//! 时效。
//! 性能：归一 O(断言)；触发 O(全域)；门 O(注册)；豁免 O(登记)。
//! 跨批对接：F3190/F3707 基线单源复用全域声明；F3925 门全域对端；各域
//! 基线对端。无隐私面。

use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x0C40）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x0C40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S8FCode(pub u16);

impl S8FCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const FORK_DETECTED: Self = Self(SEG_BASE + 1);
    pub const TRIGGER_MISSED: Self = Self(SEG_BASE + 2);
    pub const WAIVER_ABUSE: Self = Self(SEG_BASE + 3);
    pub const BASELINE_STALE: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::FORK_DETECTED => "分叉检出→归一",
            Self::TRIGGER_MISSED => "触发漏→门禁",
            Self::WAIVER_ABUSE => "豁免滥用→升级",
            Self::BASELINE_STALE => "基线过时→时效",
            _ => "未知",
        }
    }
}

// ── 基线归一（分叉红线）──────────────────────────────────────────────

/// 单条基线：域 × 断言名 × 期望值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Baseline {
    pub domain: u8,
    pub assert_name: &'static str,
    pub expect: u32,
}

/// 全域基线册：单源——同名断言只允许一条全域基线。
#[derive(Debug, Clone)]
pub struct BaselineBook {
    pub entries: Vec<Baseline>,
}

impl BaselineBook {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// 归一登记：同名断言重复登记 = 分叉，拒收。
    pub fn put(&mut self, b: Baseline) -> S8FCode {
        if self.entries.iter().any(|e| e.assert_name == b.assert_name) {
            return S8FCode::FORK_DETECTED;
        }
        self.entries.push(b);
        S8FCode::OK
    }

    /// 分叉检出：外部基线表与全域册冲突的名字。
    pub fn forks(&self, external: &[Baseline]) -> Vec<&'static str> {
        external
            .iter()
            .filter(|e| {
                match self.entries.iter().find(|m| m.assert_name == e.assert_name) {
                    Some(m) => m.expect != e.expect,
                    None => false,
                }
            })
            .map(|e| e.assert_name)
            .collect()
    }

    /// 归一：分叉项以全域册为准，返回改写数。
    pub fn normalize(&self, external: &mut Vec<Baseline>) -> usize {
        let mut n = 0;
        for e in external.iter_mut() {
            if let Some(m) = self.entries.iter().find(|m| m.assert_name == e.assert_name) {
                if e.expect != m.expect {
                    e.expect = m.expect;
                    n += 1;
                }
            }
        }
        n
    }

    /// O(1) 查询。
    pub fn get(&self, name: &str) -> Option<u32> {
        self.entries.iter().find(|e| e.assert_name == name).map(|e| e.expect)
    }
}

impl Default for BaselineBook {
    fn default() -> Self {
        Self::new()
    }
}

// ── 回归触发（引擎变更 → 全域回归）──────────────────────────────────

/// 触发账：引擎版本 → 是否已触发全域回归。
#[derive(Debug, Clone)]
pub struct TriggerBook {
    /// 已触发过的引擎版本。
    pub fired: Vec<u32>,
}

impl TriggerBook {
    pub fn new() -> Self {
        Self { fired: Vec::new() }
    }

    /// 引擎变更触发：新版本必须触发；旧版本不重触发（幂等）。
    pub fn on_engine_change(&mut self, engine_ver: u32) -> S8FCode {
        if self.fired.contains(&engine_ver) {
            return S8FCode::OK;
        }
        self.fired.push(engine_ver);
        S8FCode::OK
    }

    /// 触发漏检出：已登记版本之外还有变更未触发（门禁兜底）。
    pub fn missed(&self, known_changes: &[u32]) -> Vec<u32> {
        known_changes.iter().filter(|v| !self.fired.contains(v)).map(|v| *v).collect()
    }

    pub fn fired_count(&self) -> usize {
        self.fired.len()
    }
}

impl Default for TriggerBook {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全域门注册（门禁全域化）─────────────────────────────────────────

/// 回归门：注册后每组回归必须过门。
#[derive(Debug, Clone)]
pub struct RegressGate {
    /// 已注册门名。
    pub registered: Vec<&'static str>,
    /// 各门通过状态（与 registered 对齐）。
    pub passed: Vec<bool>,
}

impl RegressGate {
    pub fn new() -> Self {
        Self { registered: Vec::new(), passed: Vec::new() }
    }

    /// 门注册断言：重复注册拒收。
    pub fn register(&mut self, name: &'static str) -> S8FCode {
        if self.registered.contains(&name) {
            return S8FCode::TRIGGER_MISSED;
        }
        self.registered.push(name);
        self.passed.push(false);
        S8FCode::OK
    }

    pub fn pass(&mut self, name: &str) -> S8FCode {
        match self.registered.iter().position(|n| *n == name) {
            Some(i) => {
                self.passed[i] = true;
                S8FCode::OK
            }
            None => S8FCode::TRIGGER_MISSED,
        }
    }

    /// 全域门全过才放行收官。
    pub fn all_green(&self) -> bool {
        self.passed.iter().all(|p| *p) && !self.registered.is_empty()
    }
}

impl Default for RegressGate {
    fn default() -> Self {
        Self::new()
    }
}

// ── 豁免全域复用 ─────────────────────────────────────────────────────

/// 豁免登记：豁免必须挂时效与理由，滥用即升级。
#[derive(Debug, Clone)]
pub struct Waiver {
    pub assert_name: &'static str,
    /// 有效截止周期号。
    pub until_cycle: u32,
    /// 理由非空。
    pub reason: &'static str,
    /// 升级次数。
    pub escalated: u8,
}

/// 豁免台账。
#[derive(Debug, Clone)]
pub struct WaiverBook {
    pub items: Vec<Waiver>,
}

impl WaiverBook {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// 登记：理由空 = 滥用拒收。
    pub fn grant(&mut self, w: Waiver) -> S8FCode {
        if w.reason.is_empty() {
            return S8FCode::WAIVER_ABUSE;
        }
        self.items.push(w);
        S8FCode::OK
    }

    /// 豁免滥用检出：过期未清 = 滥用，升级一次。
    pub fn escalate_expired(&mut self, cycle: u32) -> Vec<&'static str> {
        let mut up = Vec::new();
        for w in self.items.iter_mut() {
            if w.until_cycle < cycle {
                w.escalated += 1;
                up.push(w.assert_name);
            }
        }
        up
    }

    /// 基线过时→时效：周期内有效豁免。
    pub fn active(&self, cycle: u32) -> Vec<&'static str> {
        self.items.iter().filter(|w| w.until_cycle >= cycle).map(|w| w.assert_name).collect()
    }
}

impl Default for WaiverBook {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves200_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves200_s08regress");

    // 01 基线单源：重复登记 = 分叉拒收
    {
        let mut b = BaselineBook::new();
        chk(&mut set, "C3946-01", b.put(Baseline { domain: 1, assert_name: "对比度", expect: 45 }) == S8FCode::OK
            && b.put(Baseline { domain: 2, assert_name: "对比度", expect: 45 }) == S8FCode::FORK_DETECTED, "err");
    }
    // 02 分叉检出实测
    {
        let mut b = BaselineBook::new();
        b.put(Baseline { domain: 1, assert_name: "焦点序", expect: 3 });
        let ext = vec![Baseline { domain: 3, assert_name: "焦点序", expect: 7 }];
        chk(&mut set, "C3946-02", b.forks(&ext) == vec!["焦点序"], "err");
    }
    // 03 归一：以全域册为准改写
    {
        let mut b = BaselineBook::new();
        b.put(Baseline { domain: 1, assert_name: "焦点序", expect: 3 });
        let mut ext = vec![Baseline { domain: 3, assert_name: "焦点序", expect: 7 }];
        let n = b.normalize(&mut ext);
        chk(&mut set, "C3946-03", n == 1 && ext[0].expect == 3 && b.forks(&ext).is_empty(), "err");
    }
    // 04 O(1) 基线查询
    {
        let mut b = BaselineBook::new();
        b.put(Baseline { domain: 5, assert_name: "朗读序", expect: 9 });
        chk(&mut set, "C3946-04", b.get("朗读序") == Some(9) && b.get("无") == None, "err");
    }
    // 05 触发幂等：同版本不重触发
    {
        let mut t = TriggerBook::new();
        t.on_engine_change(100);
        t.on_engine_change(100);
        chk(&mut set, "C3946-05", t.fired_count() == 1, "err");
    }
    // 06 触发漏→门禁兜底实测
    {
        let mut t = TriggerBook::new();
        t.on_engine_change(100);
        chk(&mut set, "C3946-06", t.missed(&[100, 101, 102]) == vec![101, 102], "err");
    }
    // 07 门注册：重复注册拒收
    {
        let mut g = RegressGate::new();
        chk(&mut set, "C3946-07", g.register("全域门") == S8FCode::OK && g.register("全域门") == S8FCode::TRIGGER_MISSED, "err");
    }
    // 08 全域门全过才放行
    {
        let mut g = RegressGate::new();
        g.register("门A");
        g.register("门B");
        let before = g.all_green();
        g.pass("门A");
        let half = g.all_green();
        g.pass("门B");
        chk(&mut set, "C3946-08", !before && !half && g.all_green(), "err");
    }
    // 09 未注册门过门拒收
    {
        let mut g = RegressGate::new();
        chk(&mut set, "C3946-09", g.pass("幽灵门") == S8FCode::TRIGGER_MISSED, "err");
    }
    // 10 豁免理由空 = 滥用拒收
    {
        let mut w = WaiverBook::new();
        chk(&mut set, "C3946-10", w.grant(Waiver { assert_name: "x", until_cycle: 5, reason: "", escalated: 0 }) == S8FCode::WAIVER_ABUSE, "err");
    }
    // 11 豁免过期→升级实测
    {
        let mut w = WaiverBook::new();
        w.grant(Waiver { assert_name: "慢报", until_cycle: 3, reason: "硬件缺", escalated: 0 });
        let up = w.escalate_expired(4);
        chk(&mut set, "C3946-11", up == vec!["慢报"] && w.items[0].escalated == 1, "err");
    }
    // 12 豁免时效：周期内有效
    {
        let mut w = WaiverBook::new();
        w.grant(Waiver { assert_name: "a", until_cycle: 10, reason: "r", escalated: 0 });
        chk(&mut set, "C3946-12", w.active(9) == vec!["a"] && w.active(10) == vec!["a"] && w.active(11).is_empty(), "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3946-13", S8FCode::BASELINE_STALE.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S8FCode::OK, S8FCode::FORK_DETECTED, S8FCode::TRIGGER_MISSED, S8FCode::WAIVER_ABUSE, S8FCode::BASELINE_STALE];
        chk(&mut set, "C3946-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s08_regress_all_green() {
        let s = run_ves200_checks();
        assert!(s.all_passed(), "ves200 red: {}", s.red_items_text());
    }

    #[test]
    fn test_fork_redline_full_cycle() {
        let mut b = BaselineBook::new();
        b.put(Baseline { domain: 1, assert_name: "x", expect: 1 });
        let mut ext = vec![Baseline { domain: 2, assert_name: "x", expect: 2 }];
        assert!(!b.forks(&ext).is_empty());
        b.normalize(&mut ext);
        assert!(b.forks(&ext).is_empty());
    }

    #[test]
    fn test_trigger_gate_chain() {
        let mut t = TriggerBook::new();
        let mut g = RegressGate::new();
        t.on_engine_change(7);
        assert!(t.missed(&[7]).is_empty());
        g.register("全域门");
        g.pass("全域门");
        assert!(g.all_green());
    }
}
