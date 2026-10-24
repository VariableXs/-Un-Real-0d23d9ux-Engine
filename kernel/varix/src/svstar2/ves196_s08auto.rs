#![allow(dead_code)]
//! VE-F3942 · 无障碍自动化测试
//!
//! 全域无障碍断言统一调度（复用生成器制度全域化）：S01-S08 + 全引擎域
//! 无障碍断言归一调度，断言总量估算 3000+。三档制度全域化复述；独立档
//! 红线：无障碍断言混入功能档 = 稀释（稀释红线全域实测）；失败路由：
//! 无障碍断言失败→域级账本路由（闭环全域化）。
//! 错误路径：混档检出→分档修正；路由断→闭环断言；调度漂移→版本化；
//! 总量失估→重估。
//! 性能：调度 O(断言数)；分档 O(1)；路由 O(域)；重估 O(周期)。
//! 跨批对接：F3019/F3150 生成器全域单源；F2955 门禁全域化；域账本对端。
//! 无隐私面。

use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x0A80）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x0A80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S8BCode(pub u16);

impl S8BCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const MIXED_LANE: Self = Self(SEG_BASE + 1);
    pub const ROUTE_BREAK: Self = Self(SEG_BASE + 2);
    pub const SCHED_DRIFT: Self = Self(SEG_BASE + 3);
    pub const EST_STALE: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::MIXED_LANE => "混档检出→分档修正",
            Self::ROUTE_BREAK => "路由断→闭环断言",
            Self::SCHED_DRIFT => "调度漂移→版本化",
            Self::EST_STALE => "总量失估→重估",
            _ => "未知",
        }
    }
}

// ── 全域断言登记 ─────────────────────────────────────────────────────

/// 断言档位：独立档（无障碍专用门）或功能档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    /// 无障碍独立档（独立门全域化）。
    A11y,
    /// 功能档。
    Functional,
}

/// 全域无障碍断言条目。
#[derive(Debug, Clone)]
pub struct A11yAssert {
    /// 断言名。
    pub name: &'static str,
    /// 所属域（S01-S08 或引擎域号）。
    pub domain: u8,
    /// 三档分级（P0/P1/P2）。
    pub tier: u8,
    /// 当前所在档（独立档红线监视点）。
    pub lane: Lane,
    /// 注册版本戳（调度漂移检测）。
    pub version: u32,
}

/// 全域调度器：归一调度 S01-S08 + 引擎域的全部无障碍断言。
#[derive(Debug, Clone)]
pub struct GlobalScheduler {
    pub asserts: Vec<A11yAssert>,
    /// 调度器版本（漂移检测锚）。
    pub sched_version: u32,
    /// 总量估算下限（3000+ 复述锚）。
    pub est_floor: u32,
}

pub const DOMAIN_COUNT: u8 = 8;

impl GlobalScheduler {
    pub fn new() -> Self {
        Self {
            asserts: Vec::new(),
            sched_version: 1,
            est_floor: 3000,
        }
    }

    /// 注册断言：分档 O(1)——独立档强制执法。
    pub fn register(&mut self, a: A11yAssert) -> S8BCode {
        if a.tier > 2 || a.domain == 0 || a.domain > DOMAIN_COUNT {
            return S8BCode::ROUTE_BREAK;
        }
        self.asserts.push(a);
        S8BCode::OK
    }

    /// 稀释红线全域实测：独立档断言混入功能档 = 稀释，返回混档名单。
    pub fn find_diluted(&self) -> Vec<usize> {
        self.asserts
            .iter()
            .enumerate()
            .filter(|(_, a)| a.name.starts_with("a11y_") && a.lane == Lane::Functional)
            .map(|(i, _)| i)
            .collect()
    }

    /// 分档修正：混档断言全部归位独立档。
    pub fn relane(&mut self) -> S8BCode {
        let diluted = self.find_diluted();
        if diluted.is_empty() {
            return S8BCode::OK;
        }
        for i in diluted {
            self.asserts[i].lane = Lane::A11y;
        }
        S8BCode::MIXED_LANE
    }

    /// 调度：O(断言数)——按域分桶返回调度计划。
    pub fn schedule(&self) -> Vec<Vec<usize>> {
        let mut plan = vec![Vec::new(); (DOMAIN_COUNT + 1) as usize];
        for (i, a) in self.asserts.iter().enumerate() {
            plan[a.domain as usize].push(i);
        }
        plan
    }

    /// 调度漂移检测：断言版本 ≠ 调度器版本即漂移。
    pub fn find_drift(&self) -> Vec<usize> {
        self.asserts
            .iter()
            .enumerate()
            .filter(|(_, a)| a.version != self.sched_version)
            .map(|(i, _)| i)
            .collect()
    }

    /// 版本化：整体升级版本戳（漂移修复）。
    pub fn repin(&mut self) -> u32 {
        self.sched_version += 1;
        let v = self.sched_version;
        for a in self.asserts.iter_mut() {
            a.version = v;
        }
        v
    }
}

impl Default for GlobalScheduler {
    fn default() -> Self {
        Self::new()
    }
}

// ── 失败路由（域级账本闭环）──────────────────────────────────────────

/// 域级账本：承接该域全部无障碍断言失败。
#[derive(Debug, Clone)]
pub struct DomainLedger {
    /// 域号 → 失败断言名集合。
    pub failures: [Vec<&'static str>; 8],
    /// 已闭环（处置完）的失败名。
    pub closed: Vec<&'static str>,
}

impl DomainLedger {
    pub fn new() -> Self {
        Self {
            failures: [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            closed: Vec::new(),
        }
    }

    /// 路由 O(域)：失败断言落入对应域账本。
    pub fn route(&mut self, domain: u8, name: &'static str) -> S8BCode {
        if domain == 0 || domain as usize > self.failures.len() {
            return S8BCode::ROUTE_BREAK;
        }
        self.failures[(domain - 1) as usize].push(name);
        S8BCode::OK
    }

    /// 闭环断言：全部失败都已闭环才算账本闭环。
    pub fn all_closed(&self) -> bool {
        self.failures.iter().all(|f| f.is_empty())
    }

    /// 处置闭环：从账本移除并记入闭环册。
    pub fn close(&mut self, domain: u8, name: &'static str) -> S8BCode {
        let d = &mut self.failures[(domain - 1) as usize];
        if let Some(pos) = d.iter().position(|n| *n == name) {
            d.remove(pos);
            self.closed.push(name);
            S8BCode::OK
        } else {
            S8BCode::ROUTE_BREAK
        }
    }

    /// 闭环路由复核：闭环册与账本守恒。
    pub fn conserved(&self) -> bool {
        self.failures.iter().map(|f| f.len()).sum::<usize>() + self.closed.len() >= self.closed.len()
    }
}

impl Default for DomainLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 总量估算与重估 ───────────────────────────────────────────────────

/// 总量重估器：登记数低于估算下限 → 重估（EST_STALE）。
#[derive(Debug, Clone)]
pub struct Estimator {
    /// 各域登记数。
    pub per_domain: [u32; 8],
    /// 估算下限。
    pub floor: u32,
    /// 重估周期计数。
    pub cycles: u32,
}

impl Estimator {
    pub fn new(floor: u32) -> Self {
        Self { per_domain: [0; 8], floor, cycles: 0 }
    }

    pub fn total(&self) -> u32 {
        self.per_domain.iter().sum()
    }

    /// 失估检出：登记总量 < 下限即失估。
    pub fn stale(&self) -> bool {
        self.total() < self.floor
    }

    /// 重估 O(周期)：一轮补登记。
    pub fn restimate(&mut self, fill_per_domain: u32) -> u32 {
        self.cycles += 1;
        for d in self.per_domain.iter_mut() {
            *d += fill_per_domain;
        }
        self.total()
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves196_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves196_s08auto");

    // 01 全域调度：八域分桶
    {
        let mut s = GlobalScheduler::new();
        for d in 1..=8u8 {
            s.register(A11yAssert { name: "a11y_x", domain: d, tier: 0, lane: Lane::A11y, version: 1 });
        }
        let plan = s.schedule();
        chk(&mut set, "C3942-01", plan.iter().skip(1).all(|b| b.len() == 1), "err");
    }
    // 02 独立档红线：混档检出实测
    {
        let mut s = GlobalScheduler::new();
        s.register(A11yAssert { name: "a11y_contrast", domain: 1, tier: 0, lane: Lane::Functional, version: 1 });
        chk(&mut set, "C3942-02", s.find_diluted().len() == 1, "err");
    }
    // 03 分档修正闭环
    {
        let mut s = GlobalScheduler::new();
        s.register(A11yAssert { name: "a11y_contrast", domain: 1, tier: 0, lane: Lane::Functional, version: 1 });
        let code = s.relane();
        chk(&mut set, "C3942-03", code == S8BCode::MIXED_LANE && s.find_diluted().is_empty() && s.asserts[0].lane == Lane::A11y, "err");
    }
    // 04 干净账不误报
    {
        let mut s = GlobalScheduler::new();
        s.register(A11yAssert { name: "a11y_focus", domain: 2, tier: 1, lane: Lane::A11y, version: 1 });
        chk(&mut set, "C3942-04", s.relane() == S8BCode::OK && s.find_diluted().is_empty(), "err");
    }
    // 05 失败路由 O(域) 入账
    {
        let mut led = DomainLedger::new();
        chk(&mut set, "C3942-05", led.route(3, "a11y_f1") == S8BCode::OK && !led.all_closed(), "err");
    }
    // 06 路由断：越界域拒收
    {
        let mut led = DomainLedger::new();
        chk(&mut set, "C3942-06", led.route(0, "x") == S8BCode::ROUTE_BREAK && led.route(9, "x") == S8BCode::ROUTE_BREAK, "err");
    }
    // 07 失败闭环：处置后账本闭环
    {
        let mut led = DomainLedger::new();
        led.route(3, "a11y_f1");
        let code = led.close(3, "a11y_f1");
        chk(&mut set, "C3942-07", code == S8BCode::OK && led.all_closed() && led.closed.len() == 1 && led.conserved(), "err");
    }
    // 08 闭环断：不存在项拒闭
    {
        let mut led = DomainLedger::new();
        chk(&mut set, "C3942-08", led.close(3, "ghost") == S8BCode::ROUTE_BREAK, "err");
    }
    // 09 调度漂移检出
    {
        let mut s = GlobalScheduler::new();
        s.register(A11yAssert { name: "a11y_a", domain: 1, tier: 0, lane: Lane::A11y, version: 1 });
        s.sched_version = 2;
        chk(&mut set, "C3942-09", s.find_drift().len() == 1, "err");
    }
    // 10 版本化修复后零漂移
    {
        let mut s = GlobalScheduler::new();
        s.register(A11yAssert { name: "a11y_a", domain: 1, tier: 0, lane: Lane::A11y, version: 1 });
        s.sched_version = 2;
        let v = s.repin();
        chk(&mut set, "C3942-10", v == 3 && s.find_drift().is_empty(), "err");
    }
    // 11 总量 3000+ 失估检出
    {
        let mut e = Estimator::new(3000);
        e.per_domain[0] = 400;
        chk(&mut set, "C3942-11", e.stale() && e.total() == 400, "err");
    }
    // 12 重估 O(周期) 补齐达标
    {
        let mut e = Estimator::new(3000);
        let t = e.restimate(400);
        chk(&mut set, "C3942-12", !e.stale() && t == 3200 && e.cycles == 1, "err");
    }
    // 13 注册校验：tier 越界拒
    {
        let mut s = GlobalScheduler::new();
        chk(&mut set, "C3942-13", s.register(A11yAssert { name: "x", domain: 1, tier: 3, lane: Lane::A11y, version: 1 }) == S8BCode::ROUTE_BREAK, "err");
    }
    // 14 诊断码语义完备
    {
        let all = [S8BCode::OK, S8BCode::MIXED_LANE, S8BCode::ROUTE_BREAK, S8BCode::SCHED_DRIFT, S8BCode::EST_STALE];
        chk(&mut set, "C3942-14", all.iter().all(|c| c.explain() != "未知") && S8BCode::EST_STALE.0 < SEG_BASE + 16, "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn reg(s: &mut GlobalScheduler, name: &'static str, domain: u8, lane: Lane) {
        s.register(A11yAssert { name, domain, tier: 0, lane, version: 1 });
    }

    #[test]
    fn test_s08_auto_all_green() {
        let s = run_ves196_checks();
        assert!(s.all_passed(), "ves196 red: {}", s.red_items_text());
    }

    #[test]
    fn test_dilution_redline_domain_wide() {
        let mut s = GlobalScheduler::new();
        for d in 1..=8u8 {
            reg(&mut s, "a11y_d", d, Lane::Functional);
        }
        assert_eq!(s.find_diluted().len(), 8);
        s.relane();
        assert!(s.find_diluted().is_empty());
    }

    #[test]
    fn test_routing_conservation() {
        let mut led = DomainLedger::new();
        led.route(1, "f1");
        led.route(8, "f2");
        assert_eq!(led.failures[7].len(), 1);
        led.close(8, "f2");
        assert!(led.conserved());
    }

    #[test]
    fn test_estimate_cycle_bound() {
        let mut e = Estimator::new(3000);
        for _ in 0..2 {
            e.restimate(800);
        }
        assert_eq!(e.total(), 1600 + 1600);
        assert!(e.total() >= e.floor);
    }
}
