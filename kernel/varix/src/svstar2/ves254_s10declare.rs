//! VE-F4000 · S 域收官宣告
//!
//! 宣告：域级收官宣告——总对账（200 项实测 200/200——硬门）；八组收口
//! 8/8；三基线冻结哈希确认；硬门六重宣告（完备 / 冻结 / 零高危 / GO /
//! 长稳 / 心流——S 域六重）；20 维三 90 确认；条件核销；主题里程碑宣
//! 告；最终宣告（S 域深化与规格收官——第十六个域宣告——家族第十六位
//! 登记）。
//! 数据结构：宣告册（五段+六重）；核验链；公示件。
//! 错误路径：任一硬门未过→阻断；失实→撤回+立案；逾期→复议。
//! 性能：对账 O(项)；核验 O(六重+八组)；公示 O(1)。
//! 跨批对接：S10 全前置；T01 最终闸；家族第十六位。
//! 无障碍与隐私：宣告含双维+心流终态公示（域本色）；无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x4DC0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x4DC0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T20FCode(pub u16);

impl T20FCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const GATE_BLOCK: Self = Self(SEG_BASE + 1);
    pub const FALSEHOOD: Self = Self(SEG_BASE + 2);
    pub const OVERDUE: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::GATE_BLOCK => "硬门未过→阻断",
            Self::FALSEHOOD => "失实→撤回+立案",
            Self::OVERDUE => "逾期→复议",
            _ => "未知",
        }
    }
}

// ── 总对账（200/200 硬门）───────────────────────────────────────────

pub const S_DOMAIN_TOTAL: u16 = 200;

/// 实测对账计数器。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrepAudit {
    /// 实测通过项。
    pub passed: u16,
}

impl GrepAudit {
    pub fn new() -> Self {
        Self { passed: 0 }
    }

    pub fn count(&mut self, n: u16) {
        self.passed = n;
    }

    /// 200/200 硬门。
    pub fn full(&self) -> bool {
        self.passed == S_DOMAIN_TOTAL
    }
}

// ── 八组收口 8/8 ─────────────────────────────────────────────────────

pub const CLOSE_GROUPS: [&str; 8] = [
    "渲染", "高对比", "读屏", "动效", "认知", "输入", "性能", "工具",
];

/// 收口标记表。
#[derive(Debug, Clone)]
pub struct CloseMark {
    pub closed: Vec<&'static str>,
}

impl CloseMark {
    pub fn new() -> Self {
        Self { closed: Vec::new() }
    }

    pub fn mark(&mut self, group: &'static str) -> bool {
        if !CLOSE_GROUPS.contains(&group) || self.closed.contains(&group) {
            return false;
        }
        self.closed.push(group);
        true
    }

    /// 8/8。
    pub fn all_closed(&self) -> bool {
        self.closed.len() == 8
    }

    pub fn missing(&self) -> Vec<&'static str> {
        CLOSE_GROUPS.iter().filter(|g| !self.closed.contains(g)).map(|g| *g).collect()
    }
}

// ── 硬门六重 ─────────────────────────────────────────────────────────

/// 六重硬门。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardGate {
    /// 完备（200/200 + 8/8）。
    Complete,
    /// 冻结（三基线哈希确认）。
    Frozen,
    /// 零高危。
    ZeroCritical,
    /// GO（四段合成报告 GO）。
    Go,
    /// 长稳。
    LongStable,
    /// 心流（指标达标）。
    Flow,
}

pub const HARD_GATES: [HardGate; 6] = [
    HardGate::Complete, HardGate::Frozen, HardGate::ZeroCritical,
    HardGate::Go, HardGate::LongStable, HardGate::Flow,
];

impl HardGate {
    pub fn name(self) -> &'static str {
        match self {
            HardGate::Complete => "完备",
            HardGate::Frozen => "冻结",
            HardGate::ZeroCritical => "零高危",
            HardGate::Go => "GO",
            HardGate::LongStable => "长稳",
            HardGate::Flow => "心流",
        }
    }

    pub fn slot(self) -> usize {
        match self {
            HardGate::Complete => 0,
            HardGate::Frozen => 1,
            HardGate::ZeroCritical => 2,
            HardGate::Go => 3,
            HardGate::LongStable => 4,
            HardGate::Flow => 5,
        }
    }
}

/// 六重核验链。
#[derive(Debug, Clone)]
pub struct GateChain {
    /// 各门通过。
    pub passed: [bool; 6],
    /// 三基线冻结哈希（冻结门证据）。
    pub baseline_hashes: [u64; 3],
    /// 心流分。
    pub flow_score: u16,
}

pub const FLOW_GATE: u16 = 75;

impl GateChain {
    pub fn new() -> Self {
        Self { passed: [false; 6], baseline_hashes: [0; 3], flow_score: 0 }
    }

    /// 完备门：对账 + 收口。
    pub fn check_complete(&mut self, g: &GrepAudit, c: &CloseMark) -> bool {
        self.passed[HardGate::Complete.slot()] = g.full() && c.all_closed();
        self.passed[0]
    }

    /// 冻结门：三基线哈希非零且互异。
    pub fn check_frozen(&mut self) -> bool {
        let [a, b, c] = self.baseline_hashes;
        let ok = a != 0 && b != 0 && c != 0 && a != b && b != c && a != c;
        self.passed[HardGate::Frozen.slot()] = ok;
        ok
    }

    /// 零高危门。
    pub fn check_zero_critical(&mut self, criticals: u32) -> bool {
        self.passed[HardGate::ZeroCritical.slot()] = criticals == 0;
        criticals == 0
    }

    /// GO 门。
    pub fn check_go(&mut self, go: bool) -> bool {
        self.passed[HardGate::Go.slot()] = go;
        go
    }

    /// 长稳门。
    pub fn check_long_stable(&mut self, stable: bool) -> bool {
        self.passed[HardGate::LongStable.slot()] = stable;
        stable
    }

    /// 心流门。
    pub fn check_flow(&mut self) -> bool {
        self.passed[HardGate::Flow.slot()] = self.flow_score >= FLOW_GATE;
        self.passed[5]
    }

    /// 六重全过。
    pub fn all_pass(&self) -> bool {
        self.passed.iter().all(|&p| p)
    }

    /// 未过门清单。
    pub fn failed_gates(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for g in HARD_GATES.iter() {
            if !self.passed[g.slot()] {
                out.push(g.name());
            }
        }
        out
    }
}

// ── 20 维三 90 确认（F3993 结果引用）────────────────────────────────

/// 三 90 结果（由 F3993 终版评分卡传入）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Triple90 {
    /// ≥90 维数。
    pub ge90_dims: u8,
    /// 均分。
    pub avg: u32,
    /// 短板维（<85）数。
    pub low_dims: u8,
}

pub const LINE_DIMS: u8 = 18;
pub const LINE_AVG: u32 = 90;
pub const LINE_LOW: u8 = 0;

impl Triple90 {
    pub fn confirmed(&self) -> bool {
        self.ge90_dims >= LINE_DIMS && self.avg >= LINE_AVG && self.low_dims == LINE_LOW
    }
}

// ── 条件核销 ─────────────────────────────────────────────────────────

/// 核销条目（预案条件 → 已核销）。
#[derive(Debug, Clone)]
pub struct ConditionEntry {
    pub name: String,
    pub redeemed: bool,
}

#[derive(Debug, Clone)]
pub struct RedemptionBook {
    pub items: Vec<ConditionEntry>,
}

impl RedemptionBook {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn put(&mut self, name: &str) {
        self.items.push(ConditionEntry { name: String::from(name), redeemed: false });
    }

    pub fn redeem(&mut self, name: &str) -> bool {
        match self.items.iter_mut().find(|i| i.name == name) {
            Some(i) => {
                i.redeemed = true;
                true
            }
            None => false,
        }
    }

    pub fn all_redeemed(&self) -> bool {
        self.items.iter().all(|i| i.redeemed)
    }
}

// ── 宣告册（可撤回）──────────────────────────────────────────────────

/// 家族第十六位登记。
pub const FAMILY_INDEX: u8 = 16;

#[derive(Debug, Clone)]
pub struct Declaration {
    /// 已宣告。
    pub declared: bool,
    /// 已撤回。
    pub withdrawn: bool,
    /// 撤回原因。
    pub withdraw_reason: String,
    /// 家族位次。
    pub family_index: u8,
}

impl Declaration {
    pub fn new() -> Self {
        Self {
            declared: false,
            withdrawn: false,
            withdraw_reason: String::new(),
            family_index: FAMILY_INDEX,
        }
    }

    /// 宣告（六重全过 + 三 90 + 核销齐为前置）。
    pub fn declare(gates: &GateChain, t90: &Triple90, red: &RedemptionBook) -> T20FCode {
        if !gates.all_pass() {
            return T20FCode::GATE_BLOCK;
        }
        if !t90.confirmed() {
            return T20FCode::GATE_BLOCK;
        }
        if !red.all_redeemed() {
            return T20FCode::GATE_BLOCK;
        }
        T20FCode::OK
    }

    /// 失实撤回。
    pub fn withdraw(&mut self, reason: &str) {
        self.withdrawn = true;
        self.withdraw_reason = String::from(reason);
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves254_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves254_s10declare");

    // C4000-01 诊断码段不越窗。
    chk(&mut set, "C4000-01",
        T20FCode::OVERDUE.0 < SEG_BASE + 16 && SEG_BASE == 0x4DC0, "err");

    // C4000-02 200/200 硬门：199 拒、200 过。
    let mut g = GrepAudit::new();
    g.count(199);
    chk(&mut set, "C4000-02", !g.full(), "err");
    g.count(200);
    chk(&mut set, "C4000-02", g.full(), "err");

    // C4000-03 八组收口 8/8。
    let mut c = CloseMark::new();
    for grp in CLOSE_GROUPS.iter().take(7) {
        c.mark(grp);
    }
    chk(&mut set, "C4000-03",
        c.missing() == vec!["工具"] && !c.all_closed(), "err");
    c.mark("工具");
    chk(&mut set, "C4000-03", c.all_closed(), "err");

    // C4000-04 完备门：两硬门同核。
    let mut chain = GateChain::new();
    chain.baseline_hashes = [0xA1, 0xB2, 0xC3];
    chk(&mut set, "C4000-04", chain.check_complete(&g, &c), "err");

    // C4000-05 冻结门：三哈希非零互异。
    chk(&mut set, "C4000-05", chain.check_frozen(), "err");

    // C4000-06 零高危门：1 个高危即拒。
    chk(&mut set, "C4000-06",
        !chain.check_zero_critical(1) && !chain.passed[HardGate::ZeroCritical.slot()], "err");
    chk(&mut set, "C4000-06", chain.check_zero_critical(0), "err");

    // C4000-07 GO/长稳门。
    chk(&mut set, "C4000-07",
        chain.check_go(true) && chain.check_long_stable(true), "err");

    // C4000-08 心流门：75 线。
    chain.flow_score = 74;
    chk(&mut set, "C4000-08", !chain.check_flow(), "err");
    chain.flow_score = 82;
    chk(&mut set, "C4000-08", chain.check_flow(), "err");

    // C4000-09 未过门清单。
    let mut chain2 = GateChain::new();
    let failed = chain2.failed_gates();
    chk(&mut set, "C4000-09", failed.len() == 6 && failed[0] == "完备", "err");

    // C4000-10 三 90 确认：全过。
    let t90 = Triple90 { ge90_dims: 19, avg: 92, low_dims: 0 };
    chk(&mut set, "C4000-10", t90.confirmed(), "err");

    // C4000-11 三 90：维数不足。
    let t91 = Triple90 { ge90_dims: 17, avg: 92, low_dims: 0 };
    chk(&mut set, "C4000-11", !t91.confirmed(), "err");

    // C4000-12 三 90：有短板。
    let t92 = Triple90 { ge90_dims: 19, avg: 92, low_dims: 1 };
    chk(&mut set, "C4000-12", !t92.confirmed(), "err");

    // C4000-13 条件核销：未核销禁宣告。
    let mut red = RedemptionBook::new();
    red.put("心流预案复测");
    red.redeem("心流预案复测");
    red.put("色标文化色补齐");
    chk(&mut set, "C4000-13", !red.all_redeemed(), "err");

    // C4000-14 核销齐 → 宣告过。
    red.redeem("色标文化色补齐");
    chk(&mut set, "C4000-14",
        red.all_redeemed() && Declaration::declare(&chain, &t90, &red) == T20FCode::OK, "err");

    // C4000-15 任一硬门未过 → 阻断。
    let mut chain3 = GateChain::new();
    chain3.baseline_hashes = [5, 5, 5]; // 互异破坏
    let _ = chain3.check_complete(&g, &c);
    let t90ok = Triple90 { ge90_dims: 20, avg: 95, low_dims: 0 };
    chk(&mut set, "C4000-15",
        Declaration::declare(&chain3, &t90ok, &red) == T20FCode::GATE_BLOCK, "err");

    // C4000-16 失实撤回 + 立案语义。
    let mut decl = Declaration::new();
    decl.declared = true;
    decl.withdraw("实测对账失实：199/200 虚报 200");
    chk(&mut set, "C4000-16",
        decl.withdrawn && decl.withdraw_reason.contains("失实")
            && T20FCode::FALSEHOOD.explain().contains("撤回"), "err");

    // C4000-17 家族第十六位登记。
    chk(&mut set, "C4000-17",
        FAMILY_INDEX == 16 && Declaration::new().family_index == 16, "err");

    // C4000-18 八组名互异。
    let mut uniq = true;
    for i in 0..8 {
        for j in (i + 1)..8 {
            if CLOSE_GROUPS[i] == CLOSE_GROUPS[j] { uniq = false; }
        }
    }
    chk(&mut set, "C4000-18", uniq, "err");

    // C4000-19 六重门名互异 + 槽双射。
    let slots: Vec<usize> = HARD_GATES.iter().map(|g| g.slot()).collect();
    let mut uniq2 = true;
    for i in 0..6 {
        for j in (i + 1)..6 {
            if HARD_GATES[i].name() == HARD_GATES[j].name() || slots[i] == slots[j] {
                uniq2 = false;
            }
        }
    }
    chk(&mut set, "C4000-19", uniq2 && HARD_GATES.len() == 6, "err");

    // C4000-20 错误码互异 + 心流线 75。
    let codes = [T20FCode::OK.0, T20FCode::GATE_BLOCK.0, T20FCode::FALSEHOOD.0, T20FCode::OVERDUE.0];
    chk(&mut set, "C4000-20",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3] && FLOW_GATE == 75, "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grep_gate_200() {
        let mut g = GrepAudit::new();
        g.count(200);
        assert!(g.full());
        g.count(199);
        assert!(!g.full());
    }

    #[test]
    fn six_gates_block_until_all() {
        let mut chain = GateChain::new();
        assert!(!chain.all_pass());
        let g = GrepAudit { passed: 200 };
        let mut c = CloseMark::new();
        for grp in CLOSE_GROUPS.iter() {
            c.mark(grp);
        }
        chain.baseline_hashes = [1, 2, 3];
        let _ = chain.check_complete(&g, &c);
        let _ = chain.check_frozen();
        let _ = chain.check_zero_critical(0);
        let _ = chain.check_go(true);
        let _ = chain.check_long_stable(true);
        chain.flow_score = 80;
        let _ = chain.check_flow();
        assert!(chain.all_pass());
    }

    #[test]
    fn declare_requires_redeem() {
        let chain_all = GateChain {
            passed: [true; 6],
            baseline_hashes: [1, 2, 3],
            flow_score: 80,
        };
        let t90 = Triple90 { ge90_dims: 20, avg: 95, low_dims: 0 };
        let mut red = RedemptionBook::new();
        red.put("条件甲");
        assert_eq!(Declaration::declare(&chain_all, &t90, &red), T20FCode::GATE_BLOCK);
        red.redeem("条件甲");
        assert_eq!(Declaration::declare(&chain_all, &t90, &red), T20FCode::OK);
    }
}
