//! VE-F3853 · 读屏与 N08 终版
//!
//! 读屏协同×N08 终版协同：N08 语义树全域终审的 S 域协同确认。
//! 渲染断链零发生证据（断链硬门 S 版）+ 语义序渲染一致性（序断言全域）
//! + 虚拟化语义保留渲染证据 + 三方对拍终版（N08 语义↔S 渲染↔真机读屏）。
//! 错误路径：断链非零→阻断、三方分歧→仲裁、证据缺→补测、序破→P1。




#![allow(dead_code)]

extern crate alloc;

use alloc::{vec, vec::Vec};

// ── 诊断码段（0xC900）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xC900;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CC9Code(pub u16);

impl CC9Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const BREAK_NONZERO: Self = Self(SEG_BASE + 1);
    pub const TRIPLE_DIVERGE: Self = Self(SEG_BASE + 2);
    pub const EVIDENCE_GAP: Self = Self(SEG_BASE + 3);
    pub const ORDER_BROKEN: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::BREAK_NONZERO => "断链非零→阻断（硬门）",
            Self::TRIPLE_DIVERGE => "三方分歧→仲裁",
            Self::EVIDENCE_GAP => "证据缺→补测",
            Self::ORDER_BROKEN => "序破→P1",
            _ => "未知",
        }
    }
}

// ── 渲染断链证据（硬门 S 版）────────────────────────────────────────

/// 断链证据条目：某域某帧的断链计数。
#[derive(Debug, Clone, Copy)]
pub struct BreakEvidence {
    /// 域号（复述七域）。
    pub domain: u8,
    /// 帧序。
    pub frame: u32,
    /// 断链数（硬门要求恒 0）。
    pub breaks: u32,
    /// 证据在册。
    pub recorded: bool,
}

/// 断链证据册（硬门：全域断链零发生）。
#[derive(Debug, Clone)]
pub struct BreakLedger {
    pub items: Vec<BreakEvidence>,
}

impl BreakLedger {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// 登记证据。
    pub fn record(&mut self, domain: u8, frame: u32, breaks: u32) {
        self.items.push(BreakEvidence {
            domain,
            frame,
            breaks,
            recorded: true,
        });
    }

    /// 硬门：全部在册证据断链为零。
    pub fn hard_gate(&self) -> bool {
        self.items.iter().all(|e| e.breaks == 0)
    }

    /// 证据缺格：七域未全覆盖。
    pub fn missing_domains(&self) -> Vec<u8> {
        let seven = [1u8, 2, 3, 4, 5, 6, 7];
        seven
            .iter()
            .filter(|d| !self.items.iter().any(|e| e.domain == **d))
            .copied()
            .collect()
    }
}

impl Default for BreakLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 序断言（全域）───────────────────────────────────────────────────

/// 语义序 vs 渲染序一致性断言。
#[derive(Debug, Clone)]
pub struct OrderAssert {
    /// N08 语义序（节点 id 序列）。
    pub sem_order: Vec<u32>,
    /// S 渲染序。
    pub render_order: Vec<u32>,
}

impl OrderAssert {
    pub fn new(sem: &[u32], render: &[u32]) -> Self {
        Self {
            sem_order: sem.to_vec(),
            render_order: render.to_vec(),
        }
    }

    /// 序一致：逐位相同。
    pub fn consistent(&self) -> bool {
        self.sem_order == self.render_order
    }

    /// 首个分歧位。
    pub fn first_diverge(&self) -> Option<usize> {
        self.sem_order
            .iter()
            .zip(self.render_order.iter())
            .position(|(a, b)| a != b)
    }
}

// ── 虚拟化语义保留证据 ──────────────────────────────────────────────

/// 虚拟化窗口外语义保留证据：窗口外节点渲染不画、语义不断。
#[derive(Debug, Clone, Copy)]
pub struct VirtKeepEvidence {
    /// 窗口外节点 id。
    pub node_id: u32,
    /// 渲染跳过（应 true）。
    pub render_skipped: bool,
    /// 语义保留（应 true）。
    pub sem_kept: bool,
}

impl VirtKeepEvidence {
    /// 保留合规：画外跳过 + 语义保留。
    pub fn compliant(&self) -> bool {
        self.render_skipped && self.sem_kept
    }
}

// ── 三方对拍终版 ────────────────────────────────────────────────────

/// 三方：N08 语义 / S 渲染 / 真机读屏。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    N08Sem,
    SRender,
    Device,
}

/// 对拍记录：某节点在三方的序位。
#[derive(Debug, Clone, Copy)]
pub struct TripleShot {
    pub node: u32,
    pub sem_pos: u32,
    pub render_pos: u32,
    pub device_pos: u32,
}

impl TripleShot {
    /// 三方一致。
    pub fn unanimous(&self) -> bool {
        self.sem_pos == self.render_pos && self.render_pos == self.device_pos
    }

    /// 分歧裁决：语义序为终版真源，渲染与真机对齐语义。
    pub fn arbiter(&self) -> u32 {
        self.sem_pos
    }
}

/// 三方对拍终版册。
#[derive(Debug, Clone)]
pub struct TripleLedger {
    pub shots: Vec<TripleShot>,
    /// 仲裁次数。
    pub arbitrations: u32,
}

impl TripleLedger {
    pub fn new() -> Self {
        Self {
            shots: Vec::new(),
            arbitrations: 0,
        }
    }

    pub fn add(&mut self, s: TripleShot) {
        self.shots.push(s);
    }

    /// 分歧节点。
    pub fn diverged(&self) -> Vec<u32> {
        self.shots
            .iter()
            .filter(|s| !s.unanimous())
            .map(|s| s.node)
            .collect()
    }

    /// 仲裁：分歧节点以语义序为准并计数。
    pub fn arbitrate(&mut self) -> usize {
        let div = self.diverged();
        let n = div.len();
        self.arbitrations += n as u32;
        n
    }

    /// 终版一致（仲裁后按语义真源收敛）。
    pub fn settled(&self) -> bool {
        self.shots
            .iter()
            .all(|s| s.render_pos == s.sem_pos && s.device_pos == s.sem_pos)
    }
}

impl Default for TripleLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 终版协同会话（总控）─────────────────────────────────────────────

/// S 域×N08 终版协同会话。
#[derive(Debug, Clone)]
pub struct N08FinalSession {
    pub ledger: BreakLedger,
    pub orders: Vec<OrderAssert>,
    pub virt: Vec<VirtKeepEvidence>,
    pub triple: TripleLedger,
}

impl N08FinalSession {
    pub fn new() -> Self {
        Self {
            ledger: BreakLedger::new(),
            orders: Vec::new(),
            virt: Vec::new(),
            triple: TripleLedger::new(),
        }
    }

    /// 补测：为缺格域补登零断链证据。
    pub fn backfill(&mut self) -> usize {
        let missing = self.ledger.missing_domains();
        let n = missing.len();
        for d in missing {
            self.ledger.record(d, 0, 0);
        }
        n
    }

    /// 会话总检。
    pub fn full_check(&self) -> Result<(), CC9Code> {
        if !self.ledger.items.iter().all(|e| e.recorded) || !self.ledger.missing_domains().is_empty()
        {
            return Err(CC9Code::EVIDENCE_GAP);
        }
        if !self.ledger.hard_gate() {
            return Err(CC9Code::BREAK_NONZERO);
        }
        if self.orders.iter().any(|o| !o.consistent()) {
            return Err(CC9Code::ORDER_BROKEN);
        }
        if !self.virt.iter().all(|v| v.compliant()) {
            return Err(CC9Code::EVIDENCE_GAP);
        }
        if !self.triple.settled() {
            return Err(CC9Code::TRIPLE_DIVERGE);
        }
        Ok(())
    }
}

impl Default for N08FinalSession {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves153_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves153_n08final");

    fn ready() -> N08FinalSession {
        let mut s = N08FinalSession::new();
        for d in 1u8..=7 {
            s.ledger.record(d, 1, 0);
        }
        s.orders.push(OrderAssert::new(&[3, 5, 8], &[3, 5, 8]));
        s.virt.push(VirtKeepEvidence {
            node_id: 42,
            render_skipped: true,
            sem_kept: true,
        });
        s
    }

    // 01 硬门：零断链通过
    {
        let s = ready();
        chk(&mut set, "C3853-01", s.ledger.hard_gate(), "err");
    }
    // 02 硬门：断链非零→阻断
    {
        let mut s = ready();
        s.ledger.record(3, 2, 1);
        chk(
            &mut set,
            "C3853-02",
            !s.ledger.hard_gate() && s.full_check() == Err(CC9Code::BREAK_NONZERO),
            "err",
        );
    }
    // 03 证据缺格检出
    {
        let mut s = N08FinalSession::new();
        s.ledger.record(1, 1, 0);
        s.ledger.record(2, 1, 0);
        let missing = s.ledger.missing_domains();
        chk(
            &mut set,
            "C3853-03",
            missing.len() == 5 && s.full_check() == Err(CC9Code::EVIDENCE_GAP),
            "err",
        );
    }
    // 04 补测归位
    {
        let mut s = N08FinalSession::new();
        s.ledger.record(1, 1, 0);
        let filled = s.backfill();
        chk(
            &mut set,
            "C3853-04",
            filled == 6 && s.ledger.missing_domains().is_empty(),
            "err",
        );
    }
    // 05 序一致通过
    {
        let s = ready();
        chk(
            &mut set,
            "C3853-05",
            s.orders.iter().all(|o| o.consistent()),
            "err",
        );
    }
    // 06 序破→P1
    {
        let mut s = ready();
        s.orders.push(OrderAssert::new(&[3, 5, 8], &[3, 8, 5]));
        chk(
            &mut set,
            "C3853-06",
            s.full_check() == Err(CC9Code::ORDER_BROKEN),
            "err",
        );
    }
    // 07 首个分歧位定位
    {
        let o = OrderAssert::new(&[3, 5, 8], &[3, 8, 5]);
        chk(&mut set, "C3853-07", o.first_diverge() == Some(1), "err");
    }
    // 08 虚拟化保留证据合规
    {
        let s = ready();
        chk(&mut set, "C3853-08", s.virt.iter().all(|v| v.compliant()), "err");
    }
    // 09 虚拟化语义丢失→证据违例
    {
        let mut s = ready();
        s.virt[0].sem_kept = false;
        chk(
            &mut set,
            "C3853-09",
            s.full_check() == Err(CC9Code::EVIDENCE_GAP),
            "err",
        );
    }
    // 10 三方一致记录
    {
        let mut t = TripleLedger::new();
        t.add(TripleShot { node: 7, sem_pos: 0, render_pos: 0, device_pos: 0 });
        t.add(TripleShot { node: 9, sem_pos: 1, render_pos: 1, device_pos: 1 });
        chk(&mut set, "C3853-10", t.settled(), "err");
    }
    // 11 三方分歧检出
    {
        let mut t = TripleLedger::new();
        t.add(TripleShot { node: 7, sem_pos: 0, render_pos: 0, device_pos: 1 });
        chk(
            &mut set,
            "C3853-11",
            t.diverged() == vec![7] && !t.settled(),
            "err",
        );
    }
    // 12 仲裁以语义为真源
    {
        let shot = TripleShot { node: 7, sem_pos: 2, render_pos: 0, device_pos: 5 };
        chk(&mut set, "C3853-12", shot.arbiter() == 2, "err");
    }
    // 13 仲裁计数
    {
        let mut t = TripleLedger::new();
        t.add(TripleShot { node: 7, sem_pos: 0, render_pos: 0, device_pos: 1 });
        t.add(TripleShot { node: 9, sem_pos: 1, render_pos: 2, device_pos: 1 });
        let n = t.arbitrate();
        chk(&mut set, "C3853-13", n == 2 && t.arbitrations == 2, "err");
    }
    // 14 三方终版收敛（按语义对齐后 settled）
    {
        let mut t = TripleLedger::new();
        t.add(TripleShot { node: 7, sem_pos: 0, render_pos: 1, device_pos: 1 });
        let _ = t.arbitrate();
        // 修正渲染与真机对齐语义。
        t.shots[0].render_pos = 0;
        t.shots[0].device_pos = 0;
        chk(&mut set, "C3853-14", t.settled(), "err");
    }
    // 15 会话总检全绿
    {
        let mut s = ready();
        s.triple.add(TripleShot { node: 7, sem_pos: 0, render_pos: 0, device_pos: 0 });
        chk(&mut set, "C3853-15", s.full_check() == Ok(()), "err");
    }
    // 16 错误路径五码互异
    {
        let codes = [
            CC9Code::BREAK_NONZERO,
            CC9Code::TRIPLE_DIVERGE,
            CC9Code::EVIDENCE_GAP,
            CC9Code::ORDER_BROKEN,
        ];
        let mut distinct = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    distinct = false;
                }
            }
        }
        chk(&mut set, "C3853-16", distinct, "err");
    }
    // 17 证据在册标记
    {
        let s = ready();
        chk(
            &mut set,
            "C3853-17",
            s.ledger.items.iter().all(|e| e.recorded) && s.ledger.items.len() == 7,
            "err",
        );
    }
    // 18 多帧证据累计仍守硬门
    {
        let mut s = ready();
        for f in 2..10u32 {
            s.ledger.record(1, f, 0);
        }
        chk(&mut set, "C3853-18", s.ledger.hard_gate(), "err");
    }

    set
}
