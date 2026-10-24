#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xDB00）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xDB00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CDBCode(pub u16);

impl CDBCode {
    pub const COMPARE_DIVERGE: Self = Self(SEG_BASE);  // 对照分歧
    pub const EXPLAIN_DISTORT: Self = Self(SEG_BASE + 1); // 解释失真
    pub const INSTRUMENT_DIVERGE: Self = Self(SEG_BASE + 2); // 仪表分歧
    pub const OVERHEAD_OVER: Self = Self(SEG_BASE + 3); // 开销超限
    pub const PROTO_BREAK: Self = Self(SEG_BASE + 4);   // 协议违规

    pub fn code(self) -> u16 { self.0 }
    pub fn name(self) -> &'static str {
        match self {
            Self::COMPARE_DIVERGE => "对照分歧",
            Self::EXPLAIN_DISTORT => "解释失真",
            Self::INSTRUMENT_DIVERGE => "仪表分歧",
            Self::OVERHEAD_OVER => "开销超限",
            Self::PROTO_BREAK => "协议违规",
            _ => "未知",
        }
    }
    pub const ALL: [Self; 5] = [
        Self::COMPARE_DIVERGE, Self::EXPLAIN_DISTORT, Self::INSTRUMENT_DIVERGE,
        Self::OVERHEAD_OVER, Self::PROTO_BREAK,
    ];
}

// ── 五列仪表 ────────────────────────────────────────────────────────

/// 五列仪表：替代清单/策略命中/指令流/决策/开销。
#[derive(Debug, Clone)]
pub struct FiveColInstrument {
    pub alt_count: u32,
    pub strategy_hits: u32,
    pub instr_count: u32,
    pub decisions: u32,
    pub overhead_ms: u32,
    pub active: bool,
}

impl FiveColInstrument {
    pub fn new() -> Self {
        Self { alt_count: 0, strategy_hits: 0, instr_count: 0, decisions: 0, overhead_ms: 0, active: false }
    }

    /// 打开才采样（零常态）。
    pub fn sample(&mut self) {
        if self.active {
            self.alt_count += 1;
        }
    }

    /// 仪表一致性：计数器非负、开销合理。
    pub fn consistent(&self) -> bool {
        self.overhead_ms < 10000
    }
}

// ── 对照预览（单源）─────────────────────────────────────────────────

/// 对照模式：动效 vs 替代并排。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComparePreview {
    pub original_output: &'static str,
    pub alt_output: &'static str,
    /// 单源标记（跨域复用单源）。
    pub single_source: bool,
}

impl ComparePreview {
    /// 对照单源：两输出一致。
    pub fn match_ok(&self) -> bool {
        self.original_output == self.alt_output
    }
    /// 单源完整性。
    pub fn single_source_ok(&self) -> bool { self.single_source }
}

// ── 决策解释 ────────────────────────────────────────────────────────

/// 决策解释器（工具化可解释）。
#[derive(Debug, Clone)]
pub struct DecisionExplainer {
    pub decisions: Vec<DecisionEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionEntry {
    pub id: u32,
    pub reason: &'static str,
    pub faithful: bool,
}

impl DecisionExplainer {
    pub fn new() -> Self { Self { decisions: Vec::new() } }

    /// 解释忠实度：全部决策有忠实解释。
    pub fn all_faithful(&self) -> bool {
        self.decisions.iter().all(|d| d.faithful)
    }
}

// ── 断链扫描 ────────────────────────────────────────────────────────

/// 断链扫描器（替代清单对齐）。
#[derive(Debug, Clone)]
pub struct ChainScanner {
    pub expected: Vec<u32>,
    pub actual: Vec<u32>,
}

impl ChainScanner {
    /// 对齐完整：期望 = 实际。
    pub fn aligned(&self) -> bool { self.expected == self.actual }
}

// ── 家族二十四协议 ──────────────────────────────────────────────────

/// 家族成员。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FamilyMember {
    pub id: u8,
    pub registered: bool,
}

/// 家族二十四协议集成。
#[derive(Debug, Clone)]
pub struct Family24Protocol {
    pub members: Vec<FamilyMember>,
}

impl Family24Protocol {
    pub fn new() -> Self {
        let mut members = Vec::new();
        for i in 0..24u8 {
            members.push(FamilyMember { id: i, registered: true });
        }
        Self { members }
    }

    /// 协议完整：24 成员全部注册。
    pub fn complete(&self) -> bool {
        self.members.len() == 24 && self.members.iter().all(|m| m.registered)
    }
}

// ── 调试器会话 ──────────────────────────────────────────────────────

/// 动效替代调试器。
#[derive(Debug, Clone)]
pub struct MotionAltDebugger {
    pub instrument: FiveColInstrument,
    pub compare: ComparePreview,
    pub explainer: DecisionExplainer,
    pub scanner: ChainScanner,
    pub family: Family24Protocol,
}

impl MotionAltDebugger {
    pub fn audit(&self) -> DebugAudit {
        DebugAudit {
            instrument_ok: self.instrument.consistent(),
            compare_ok: self.compare.match_ok() && self.compare.single_source_ok(),
            explain_ok: self.explainer.all_faithful(),
            chain_ok: self.scanner.aligned(),
            family_ok: self.family.complete(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugAudit {
    pub instrument_ok: bool,
    pub compare_ok: bool,
    pub explain_ok: bool,
    pub chain_ok: bool,
    pub family_ok: bool,
}
impl DebugAudit {
    pub fn all_green(&self) -> bool {
        self.instrument_ok && self.compare_ok && self.explain_ok && self.chain_ok && self.family_ok
    }
}

// ── 判据集 ──────────────────────────────────────────────────────────

pub fn run_f3873_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("vet4k_motaltdbg");
    fn chk(set: &mut crate::checks::CheckSet, label: &'static str, cond: bool, note: &'static str) {
        set.add(label, cond, note);
    }

    // 1 仪表一致性
    {
        let i = FiveColInstrument::new();
        chk(&mut set, "C3873-01", i.consistent(), "err");
    }
    // 2 打开才采样
    {
        let mut i = FiveColInstrument::new();
        i.sample();
        chk(&mut set, "C3873-02", i.alt_count == 0, "err");
    }
    // 3 激活后采样
    {
        let mut i = FiveColInstrument::new();
        i.active = true;
        i.sample();
        chk(&mut set, "C3873-03", i.alt_count == 1, "err");
    }
    // 4 对照一致
    {
        let c = ComparePreview { original_output: "fade", alt_output: "fade", single_source: true };
        chk(&mut set, "C3873-04", c.match_ok(), "err");
    }
    // 5 对照分歧
    {
        let c = ComparePreview { original_output: "fade", alt_output: "slide", single_source: true };
        chk(&mut set, "C3873-05", !c.match_ok(), "err");
    }
    // 6 单源完整
    {
        let c = ComparePreview { original_output: "a", alt_output: "a", single_source: true };
        chk(&mut set, "C3873-06", c.single_source_ok(), "err");
    }
    // 7 解释忠实
    {
        let mut e = DecisionExplainer::new();
        e.decisions.push(DecisionEntry { id: 1, reason: "reduce", faithful: true });
        chk(&mut set, "C3873-07", e.all_faithful(), "err");
    }
    // 8 解释失真
    {
        let mut e = DecisionExplainer::new();
        e.decisions.push(DecisionEntry { id: 1, reason: "reduce", faithful: false });
        chk(&mut set, "C3873-08", !e.all_faithful(), "err");
    }
    // 9 断链扫描对齐
    {
        let s = ChainScanner { expected: vec![1, 2, 3], actual: vec![1, 2, 3] };
        chk(&mut set, "C3873-09", s.aligned(), "err");
    }
    // 10 断链检出
    {
        let s = ChainScanner { expected: vec![1, 2, 3], actual: vec![1, 3] };
        chk(&mut set, "C3873-10", !s.aligned(), "err");
    }
    // 11 家族二十四
    {
        let f = Family24Protocol::new();
        chk(&mut set, "C3873-11", f.complete(), "err");
    }
    // 12 家族缺员
    {
        let mut f = Family24Protocol::new();
        f.members.pop();
        chk(&mut set, "C3873-12", !f.complete(), "err");
    }
    // 13 调试器全绿
    {
        let d = MotionAltDebugger {
            instrument: FiveColInstrument::new(),
            compare: ComparePreview { original_output: "a", alt_output: "a", single_source: true },
            explainer: DecisionExplainer::new(),
            scanner: ChainScanner { expected: vec![1], actual: vec![1] },
            family: Family24Protocol::new(),
        };
        chk(&mut set, "C3873-13", d.audit().all_green(), "err");
    }
    // 14 仪表开销超限
    {
        let mut i = FiveColInstrument::new();
        i.overhead_ms = 20000;
        chk(&mut set, "C3873-14", !i.consistent(), "err");
    }
    // 15 对照单源失败
    {
        let c = ComparePreview { original_output: "a", alt_output: "a", single_source: false };
        chk(&mut set, "C3873-15", !c.single_source_ok(), "err");
    }
    // 16 空解释器全忠实
    {
        let e = DecisionExplainer::new();
        chk(&mut set, "C3873-16", e.all_faithful(), "err");
    }
    // 17 家族成员 id 互异
    {
        let f = Family24Protocol::new();
        let mut ok = true;
        for i in 0..24usize {
            for j in (i + 1)..24usize {
                if f.members[i].id == f.members[j].id { ok = false; }
            }
        }
        chk(&mut set, "C3873-17", ok, "err");
    }
    // 18 零采样常开
    {
        let mut i = FiveColInstrument::new();
        for _ in 0..10 { i.sample(); }
        chk(&mut set, "C3873-18", i.alt_count == 0, "err");
    }

    set
}
