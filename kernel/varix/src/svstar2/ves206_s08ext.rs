#![allow(dead_code)]
//! VE-F3952 · 无障碍扩展点
//!
//! 五点三段模式全域化：扩展点清单（态采集扩展/策略插件/替代渲染插件/
//! 验证规则插件/仿真器插件五点）——五点全域三段（签名/沙箱/失效——硬门
//! 全域化：三段缺→不可发布）。扩展预算隔离全域化（预算破→隔离）；扩展
//! 治理单源全域化（绕治理→P0）；API 破坏→ADR；扩展义务全域复述。
//! 错误路径：三段缺→不可发布；预算破→隔离；绕治理→P0；API 破坏→ADR。
//! 性能：清单 O(五点)；隔离 O(全域)；治理 O(单源)；义务 O(复述)。
//! 跨批对接：F3351/F3109 单源全域复用声明；S 域插槽对端；F3953 集成前向。
//! 无隐私面。

use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x0FC0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x0FC0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S8LCode(pub u16);

impl S8LCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const TRIPLE_MISSING: Self = Self(SEG_BASE + 1);
    pub const BUDGET_BREACH: Self = Self(SEG_BASE + 2);
    pub const GOV_BYPASS: Self = Self(SEG_BASE + 3);
    pub const API_BREAK: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::TRIPLE_MISSING => "三段缺→不可发布",
            Self::BUDGET_BREACH => "预算破→隔离",
            Self::GOV_BYPASS => "绕治理→P0",
            Self::API_BREAK => "API破坏→ADR",
            _ => "未知",
        }
    }
}

// ── 五点扩展清单 ─────────────────────────────────────────────────────

/// 扩展点五点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtPoint {
    /// 态采集扩展。
    StateCollect,
    /// 策略插件。
    PolicyPlugin,
    /// 替代渲染插件。
    AltRender,
    /// 验证规则插件。
    VerifyRule,
    /// 仿真器插件。
    SimulatorPlugin,
}

pub const EXT_POINTS: [ExtPoint; 5] = [
    ExtPoint::StateCollect,
    ExtPoint::PolicyPlugin,
    ExtPoint::AltRender,
    ExtPoint::VerifyRule,
    ExtPoint::SimulatorPlugin,
];

// ── 五点全域三段（签名/沙箱/失效）────────────────────────────────────

/// 扩展注册：三段硬门。
#[derive(Debug, Clone)]
pub struct ExtEntry {
    pub name: &'static str,
    pub point: ExtPoint,
    /// 段一：签名验证（未签名不可发布）。
    pub signed: bool,
    /// 段二：沙箱隔离声明。
    pub sandboxed: bool,
    /// 段三：失效兜底（插件崩溃时的降级路径）。
    pub fallback: bool,
}

impl ExtEntry {
    /// 三段硬门：任一缺 → 不可发布。
    pub fn publishable(&self) -> bool {
        self.signed && self.sandboxed && self.fallback
    }

    /// 缺段清单。
    pub fn missing(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if !self.signed {
            v.push("签名");
        }
        if !self.sandboxed {
            v.push("沙箱");
        }
        if !self.fallback {
            v.push("失效");
        }
        v
    }
}

/// 扩展登记表：五点全域发布闸。
#[derive(Debug, Clone)]
pub struct ExtRegistry {
    pub entries: Vec<ExtEntry>,
}

impl ExtRegistry {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// 发布闸：三段齐备才入册。
    pub fn publish(&mut self, e: ExtEntry) -> S8LCode {
        if !e.publishable() {
            return S8LCode::TRIPLE_MISSING;
        }
        self.entries.push(e);
        S8LCode::OK
    }

    /// 五点覆盖：每点至少一插件。
    pub fn point_gaps(&self) -> Vec<ExtPoint> {
        EXT_POINTS
            .iter()
            .filter(|p| !self.entries.iter().any(|e| e.point == **p))
            .map(|p| *p)
            .collect()
    }
}

impl Default for ExtRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── 扩展预算隔离 ─────────────────────────────────────────────────────

/// 预算账：每插件一份预算，破预算 → 隔离。
#[derive(Debug, Clone)]
pub struct BudgetItem {
    pub plugin: &'static str,
    /// 预算（毫秒/帧）。
    pub budget_ms: u32,
    /// 实测。
    pub used_ms: u32,
    /// 是否被隔离。
    pub quarantined: bool,
}

/// 预算隔离账。
#[derive(Debug, Clone)]
pub struct BudgetLedger {
    pub items: Vec<BudgetItem>,
}

impl BudgetLedger {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn grant(&mut self, plugin: &'static str, budget_ms: u32) {
        self.items.push(BudgetItem { plugin, budget_ms, used_ms: 0, quarantined: false });
    }

    /// 记账：超预算 → 隔离。
    pub fn charge(&mut self, plugin: &str, ms: u32) -> S8LCode {
        match self.items.iter_mut().find(|i| i.plugin == plugin) {
            Some(i) => {
                i.used_ms += ms;
                if i.used_ms > i.budget_ms {
                    i.quarantined = true;
                    return S8LCode::BUDGET_BREACH;
                }
                S8LCode::OK
            }
            None => S8LCode::GOV_BYPASS,
        }
    }

    /// 隔离名单。
    pub fn quarantined(&self) -> Vec<&'static str> {
        self.items.iter().filter(|i| i.quarantined).map(|i| i.plugin).collect()
    }

    /// 活跃（未隔离）插件。
    pub fn active(&self) -> Vec<&'static str> {
        self.items.iter().filter(|i| !i.quarantined).map(|i| i.plugin).collect()
    }
}

impl Default for BudgetLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 扩展治理单源 + 义务 ──────────────────────────────────────────────

/// 治理单源：所有扩展变更必须过治理闸登记。
#[derive(Debug, Clone)]
pub struct GovGate {
    /// 已过闸的插件名。
    pub approved: Vec<&'static str>,
    /// ADR 记录：API 破坏 → ADR 编号。
    pub adrs: Vec<(&'static str, u32)>,
}

impl GovGate {
    pub fn new() -> Self {
        Self { approved: Vec::new(), adrs: Vec::new() }
    }

    /// 治理闸：登记后才可上架（绕治理 → P0 检出）。
    pub fn approve(&mut self, plugin: &'static str) {
        self.approved.push(plugin);
    }

    pub fn bypassed(&self, plugin: &str) -> bool {
        !self.approved.iter().any(|a| *a == plugin)
    }

    /// API 破坏 → ADR 登记。
    pub fn break_api(&mut self, plugin: &'static str, adr: u32) {
        self.adrs.push((plugin, adr));
    }

    /// 十年承诺锚：ADR 编号单调不回收。
    pub fn adr_monotonic(&self) -> bool {
        self.adrs.windows(2).all(|w| w[0].1 <= w[1].1)
    }
}

impl Default for GovGate {
    fn default() -> Self {
        Self::new()
    }
}

/// 扩展义务全域复述：五义务清单。
pub const EXT_DUTIES: [&str; 5] = [
    "签名申报", "沙箱申报", "失效兜底", "预算申报", "治理登记",
];

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves206_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves206_s08ext");

    // 01 五点清单守恒
    {
        chk(&mut set, "C3952-01", EXT_POINTS.len() == 5 && ExtPoint::StateCollect as usize == 0 && ExtPoint::SimulatorPlugin as usize == 4, "err");
    }
    // 02 三段齐备可发布
    {
        let e = ExtEntry { name: "p1", point: ExtPoint::PolicyPlugin, signed: true, sandboxed: true, fallback: true };
        chk(&mut set, "C3952-02", e.publishable() && e.missing().is_empty(), "err");
    }
    // 03 三段缺→不可发布实测（逐段）
    {
        let e1 = ExtEntry { name: "p2a", point: ExtPoint::StateCollect, signed: false, sandboxed: true, fallback: true };
        let e2 = ExtEntry { name: "p2b", point: ExtPoint::StateCollect, signed: true, sandboxed: false, fallback: true };
        let e3 = ExtEntry { name: "p2c", point: ExtPoint::StateCollect, signed: true, sandboxed: true, fallback: false };
        chk(&mut set, "C3952-03", !e1.publishable() && e1.missing() == vec!["签名"]
            && !e2.publishable() && e2.missing() == vec!["沙箱"]
            && !e3.publishable() && e3.missing() == vec!["失效"], "err");
    }
    // 04 发布闸：缺段拒收
    {
        let mut r = ExtRegistry::new();
        let e = ExtEntry { name: "p3", point: ExtPoint::AltRender, signed: true, sandboxed: false, fallback: true };
        chk(&mut set, "C3952-04", r.publish(e) == S8LCode::TRIPLE_MISSING && r.entries.is_empty(), "err");
    }
    // 05 五点覆盖缺口检出
    {
        let mut r = ExtRegistry::new();
        r.publish(ExtEntry { name: "a", point: ExtPoint::StateCollect, signed: true, sandboxed: true, fallback: true });
        let gaps = r.point_gaps();
        chk(&mut set, "C3952-05", gaps.len() == 4 && !gaps.contains(&ExtPoint::StateCollect), "err");
    }
    // 06 预算内记账正常
    {
        let mut l = BudgetLedger::new();
        l.grant("插件甲", 10);
        chk(&mut set, "C3952-06", l.charge("插件甲", 5) == S8LCode::OK && l.charge("插件甲", 4) == S8LCode::OK && l.quarantined().is_empty(), "err");
    }
    // 07 预算破→隔离实测
    {
        let mut l = BudgetLedger::new();
        l.grant("插件乙", 8);
        let c1 = l.charge("插件乙", 5);
        let c2 = l.charge("插件乙", 5);
        chk(&mut set, "C3952-07", c1 == S8LCode::OK && c2 == S8LCode::BUDGET_BREACH && l.quarantined() == vec!["插件乙"] && l.active().is_empty(), "err");
    }
    // 08 未登记插件记账 = 绕治理
    {
        let mut l = BudgetLedger::new();
        chk(&mut set, "C3952-08", l.charge("幽灵插件", 1) == S8LCode::GOV_BYPASS, "err");
    }
    // 09 绕治理检出
    {
        let mut g = GovGate::new();
        g.approve("甲");
        chk(&mut set, "C3952-09", !g.bypassed("甲") && g.bypassed("乙"), "err");
    }
    // 10 API 破坏 → ADR 单调
    {
        let mut g = GovGate::new();
        g.break_api("甲", 7);
        g.break_api("乙", 9);
        g.break_api("丙", 12);
        chk(&mut set, "C3952-10", g.adr_monotonic() && g.adrs.len() == 3, "err");
    }
    // 11 ADR 非单调检出（承诺破坏）
    {
        let mut g = GovGate::new();
        g.break_api("甲", 9);
        g.break_api("乙", 7);
        chk(&mut set, "C3952-11", !g.adr_monotonic(), "err");
    }
    // 12 五义务清单守恒
    {
        chk(&mut set, "C3952-12", EXT_DUTIES.len() == 5 && EXT_DUTIES[0] == "签名申报" && EXT_DUTIES[4] == "治理登记", "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3952-13", S8LCode::API_BREAK.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S8LCode::OK, S8LCode::TRIPLE_MISSING, S8LCode::BUDGET_BREACH, S8LCode::GOV_BYPASS, S8LCode::API_BREAK];
        chk(&mut set, "C3952-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s08_ext_all_green() {
        let s = run_ves206_checks();
        assert!(s.all_passed(), "ves206 red: {}", s.red_items_text());
    }

    #[test]
    fn test_triple_gate_matrix() {
        let combos = [
            (true, true, true, true),
            (false, true, true, false),
            (true, false, true, false),
            (true, true, false, false),
        ];
        for (s, x, f, ok) in combos.iter() {
            let e = ExtEntry { name: "t", point: ExtPoint::VerifyRule, signed: *s, sandboxed: *x, fallback: *f };
            assert_eq!(e.publishable(), *ok);
        }
    }

    #[test]
    fn test_budget_quarantine_recover_semantics() {
        let mut l = BudgetLedger::new();
        l.grant("p", 3);
        l.charge("p", 2);
        assert!(l.charge("p", 2) == S8LCode::BUDGET_BREACH);
        assert_eq!(l.quarantined(), vec!["p"]);
        assert_eq!(l.active().len(), 0);
    }
}
