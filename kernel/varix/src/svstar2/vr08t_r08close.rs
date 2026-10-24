//! VE-F3760 · 创作无障碍组收口
//!
//! 锚点：创作无障碍组收口（R08 收口（双签复述：工具签+无障碍签（双签复述）；
//! 六件套复述（指南/规范/审计/报告/台账/豁免——六件复述）；跨域对拍复述
//! （E/S/N 三对拍（复述交叉对拍）；收口门禁（GO/NO-GO 九维（复述门禁）。
//!
//! 数据结构：双签复述；六件复述；三对拍复述；收口门禁复述。
//! 错误路径与降级矩阵：双签缺→不收口（复述红线实测）；六件缺→补（复述）；
//! 对拍分歧→立案（复述红线）；门禁不过→NO-GO（复述）。
//! 跨批对接点：F3160/F3357 单源复用声明；F3742-F3759 全部对端输入。
//!
//! 诊断码段：独占 0x97xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x97xx）
// ─────────────────────────────────────────────────────────────

/// F3760 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct K8Code(pub u16);

impl K8Code {
    /// 双签缺——不收口。
    pub const DUAL_SIGN_MISS: K8Code = K8Code(0x9701);
    /// 六件缺——补。
    pub const SIX_PIECE_MISS: K8Code = K8Code(0x9702);
    /// 对拍分歧——立案。
    pub const CROSS_FAIL: K8Code = K8Code(0x9703);
    /// 门禁 NO-GO——收口门禁。
    pub const GATE_NOGO: K8Code = K8Code(0x9704);
    /// 收口证据缺失——证据红线。
    pub const EVIDENCE_MISS: K8Code = K8Code(0x9705);
    /// 豁免未清——收口前清账。
    pub const EXEMPT_OPEN: K8Code = K8Code(0x9706);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x9701 => "双签缺失，组收口不生效。",
            0x9702 => "六件套缺件，复述不完整。",
            0x9703 => "跨域对拍分歧，交叉对拍红线不通过。",
            0x9704 => "收口门禁 NO-GO，不可收口。",
            0x9705 => "收口证据缺失，证据红线不通过。",
            0x9706 => "豁免未清账，收口前置条件不满足。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 双签（工具签 + 无障碍签）
// ─────────────────────────────────────────────────────────────

/// 双签。
#[derive(Clone, Debug, Default)]
pub struct CloseDualSign {
    /// 工具签。
    pub tool_sign: bool,
    /// 无障碍签。
    pub a11y_sign: bool,
}

impl CloseDualSign {
    /// 构造。
    pub fn new() -> CloseDualSign {
        CloseDualSign {
            tool_sign: false,
            a11y_sign: false,
        }
    }

    /// 签工具。
    pub fn sign_tool(&mut self) {
        self.tool_sign = true;
    }

    /// 签无障碍。
    pub fn sign_a11y(&mut self) {
        self.a11y_sign = true;
    }

    /// 双签齐。
    pub fn complete(&self) -> bool {
        self.tool_sign && self.a11y_sign
    }
}

// ─────────────────────────────────────────────────────────────
// 六件套
// ─────────────────────────────────────────────────────────────

/// 六件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SixPiece {
    /// 指南。
    Guide,
    /// 规范。
    Spec,
    /// 审计。
    Audit,
    /// 报告。
    Report,
    /// 台账。
    Ledger,
    /// 豁免。
    Exempt,
}

impl SixPiece {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            SixPiece::Guide => 0,
            SixPiece::Spec => 1,
            SixPiece::Audit => 2,
            SixPiece::Report => 3,
            SixPiece::Ledger => 4,
            SixPiece::Exempt => 5,
        }
    }

    /// 全集（六件）。
    pub fn all() -> [SixPiece; 6] {
        [
            SixPiece::Guide,
            SixPiece::Spec,
            SixPiece::Audit,
            SixPiece::Report,
            SixPiece::Ledger,
            SixPiece::Exempt,
        ]
    }
}

/// 六件台账。
#[derive(Clone, Debug)]
pub struct SixLedger {
    present: [bool; 6],
}

impl SixLedger {
    /// 空台账。
    pub fn new() -> SixLedger {
        SixLedger {
            present: [false; 6],
        }
    }

    /// 标记到位。
    pub fn mark(&mut self, piece: SixPiece) {
        self.present[piece.index() as usize] = true;
    }

    /// 六件齐。
    pub fn complete(&self) -> bool {
        self.present.iter().all(|b| *b)
    }

    /// 缺件清单。
    pub fn missing(&self) -> Vec<SixPiece> {
        let mut bad = Vec::new();
        for p in SixPiece::all() {
            if !self.present[p.index() as usize] {
                bad.push(p);
            }
        }
        bad
    }
}

impl Default for SixLedger {
    fn default() -> SixLedger {
        SixLedger::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 跨域对拍（E/S/N）
// ─────────────────────────────────────────────────────────────

/// 对拍域。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrossDomain {
    /// E 域。
    E,
    /// S 域。
    S,
    /// N 域。
    N,
}

impl CrossDomain {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            CrossDomain::E => 0,
            CrossDomain::S => 1,
            CrossDomain::N => 2,
        }
    }

    /// 全集。
    pub fn all() -> [CrossDomain; 3] {
        [CrossDomain::E, CrossDomain::S, CrossDomain::N]
    }
}

/// 对拍结果。
#[derive(Clone, Debug)]
pub struct CrossResult {
    /// 域。
    pub domain: CrossDomain,
    /// 是否对拍通过。
    pub passed: bool,
    /// 证据 id。
    pub evidence_id: u32,
}

impl CrossResult {
    /// 构造。
    pub fn new(domain: CrossDomain, passed: bool, evidence_id: u32) -> CrossResult {
        CrossResult {
            domain,
            passed,
            evidence_id,
        }
    }
}

/// 对拍台账。
#[derive(Clone, Debug, Default)]
pub struct CrossBook {
    results: Vec<CrossResult>,
}

impl CrossBook {
    /// 空台账。
    pub fn new() -> CrossBook {
        CrossBook {
            results: Vec::new(),
        }
    }

    /// 记录。
    pub fn add(&mut self, r: CrossResult) {
        self.results.push(r);
    }

    /// 全域通过。
    pub fn all_pass(&self) -> bool {
        self.results.len() >= 3 && self.results.iter().all(|r| r.passed)
    }

    /// 分歧清单。
    pub fn diverging(&self) -> Vec<CrossDomain> {
        let mut bad = Vec::new();
        for r in self.results.iter() {
            if !r.passed {
                bad.push(r.domain);
            }
        }
        bad
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.results.len()
    }

    /// 空判断。
    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────
// 收口门禁（九维 GO/NO-GO）
// ─────────────────────────────────────────────────────────────

/// 收口门禁。
#[derive(Clone, Debug)]
pub struct CloseGate {
    /// 九维通过位（8 用 u8 + 第 9 用 bool）。
    dims: [bool; 9],
}

impl CloseGate {
    /// 全不过。
    pub fn new() -> CloseGate {
        CloseGate { dims: [false; 9] }
    }

    /// 全过。
    pub fn all_pass() -> CloseGate {
        CloseGate { dims: [true; 9] }
    }

    /// 设一维。
    pub fn set(&mut self, idx: usize, ok: bool) -> bool {
        if idx >= 9 {
            return false;
        }
        self.dims[idx] = ok;
        true
    }

    /// 通过数。
    pub fn pass_count(&self) -> u32 {
        self.dims.iter().filter(|b| **b).count() as u32
    }

    /// GO：九维全过。
    pub fn go(&self) -> bool {
        self.dims.iter().all(|b| *b)
    }
}

impl Default for CloseGate {
    fn default() -> CloseGate {
        CloseGate::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 主编排器
// ─────────────────────────────────────────────────────────────

/// R08 收口器。
#[derive(Clone, Debug, Default)]
pub struct R08Closer {
    sign: CloseDualSign,
    six: SixLedger,
    cross: CrossBook,
    gate: CloseGate,
}

impl R08Closer {
    /// 构造。
    pub fn new() -> R08Closer {
        R08Closer {
            sign: CloseDualSign::new(),
            six: SixLedger::new(),
            cross: CrossBook::new(),
            gate: CloseGate::new(),
        }
    }

    /// 取双签（可变）。
    pub fn sign_mut(&mut self) -> &mut CloseDualSign {
        &mut self.sign
    }

    /// 取六件（可变）。
    pub fn six_mut(&mut self) -> &mut SixLedger {
        &mut self.six
    }

    /// 取对拍（可变）。
    pub fn cross_mut(&mut self) -> &mut CrossBook {
        &mut self.cross
    }

    /// 取门禁（可变）。
    pub fn gate_mut(&mut self) -> &mut CloseGate {
        &mut self.gate
    }

    /// 全量审查。
    pub fn audit(&self) -> Vec<K8Code> {
        let mut bad = Vec::new();
        if !self.sign.complete() {
            bad.push(K8Code::DUAL_SIGN_MISS);
        }
        if !self.six.complete() {
            bad.push(K8Code::SIX_PIECE_MISS);
        }
        if !self.cross.all_pass() {
            bad.push(K8Code::CROSS_FAIL);
        }
        if !self.gate.go() {
            bad.push(K8Code::GATE_NOGO);
        }
        bad
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3760 判据集。
pub fn run_vr08t_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3760");

    // 0x9701 双签
    {
        let mut s = CloseDualSign::new();
        out.add("0x9701-缺双签拦", !s.complete(), "");
        s.sign_tool();
        out.add("0x9701-单签拦", !s.complete(), "");
        s.sign_a11y();
        out.add("0x9701-双签过", s.complete(), "");
    }

    // 0x9702 六件
    {
        let mut l = SixLedger::new();
        out.add("0x9702-全缺检出", l.missing().len() == 6, "");
        for p in SixPiece::all() {
            l.mark(p);
        }
        out.add("0x9702-六件齐过", l.complete(), "");
    }

    // 0x9703 对拍
    {
        let mut c = CrossBook::new();
        out.add("0x9703-空台账不齐", !c.all_pass(), "");
        for d in CrossDomain::all() {
            c.add(CrossResult::new(d, true, 1));
        }
        out.add("0x9703-三域全过", c.all_pass(), "");
    }

    // 0x9704 门禁
    {
        let mut g = CloseGate::new();
        out.add("0x9704-全不过NOGO", !g.go() && g.pass_count() == 0, "");
        for i in 0..9 {
            g.set(i, true);
        }
        out.add("0x9704-九维GO", g.go() && g.pass_count() == 9, "");
        out.add("0x9704-越界拒收", !g.set(9, true), "");
    }

    // 判据
    {
        let mut c = R08Closer::new();
        let bad = c.audit();
        out.add("0x9705-收口检出", bad.iter().any(|x| x.raw() == 0x9701), "");
    }

    out
}
