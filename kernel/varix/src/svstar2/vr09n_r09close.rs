//! VE-F3774 · R09 自查组收口
//!
//! 锚点：R09 自查组收口——双签复述（自查签+验证签（双签复述）；七件套复述
//! （范围/计划/核验/对账/清点/汇总/报告——七件复述）；跨组对拍复述
//! （八组对拍（复述）；收口门禁（GO/NO-GO（复述门禁）。
//!
//! 数据结构：双签复述；七件复述；对拍复述；收口门禁复述。
//! 错误路径与降级矩阵：双签缺→不收口（复述红线实测）；七件缺→补（复述）；
//! 对拍分歧→立案（复述红线）；门禁不过→NO-GO（复述）。
//! 跨批对接点：F3761-F3773 全部对端输入；R10 衔接。
//!
//! 诊断码段：独占 0xA5xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xA5xx）
// ─────────────────────────────────────────────────────────────

/// F3774 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C9Code(pub u16);

impl C9Code {
    /// 双签缺——不收口。
    pub const DUAL_SIGN_MISS: C9Code = C9Code(0xA501);
    /// 七件缺——补。
    pub const SEVEN_MISS: C9Code = C9Code(0xA502);
    /// 对拍分歧——立案。
    pub const CROSS_FAIL: C9Code = C9Code(0xA503);
    /// 门禁 NO-GO。
    pub const GATE_NOGO: C9Code = C9Code(0xA504);
    /// 收口证据缺失。
    pub const EVIDENCE_MISS: C9Code = C9Code(0xA505);
    /// 前置未清——收口前置。
    pub const PRECOND_OPEN: C9Code = C9Code(0xA506);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xA501 => "双签缺失，R09 收口不生效。",
            0xA502 => "七件套缺件，复述不完整。",
            0xA503 => "跨组对拍分歧，交叉对拍红线不通过。",
            0xA504 => "收口门禁 NO-GO，不可收口。",
            0xA505 => "收口证据缺失，证据红线不通过。",
            0xA506 => "前置条件未清，收口前置不满足。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 双签
// ─────────────────────────────────────────────────────────────

/// R09 双签。
#[derive(Clone, Debug, Default)]
pub struct R09Sign {
    /// 自查签。
    pub self_sign: bool,
    /// 验证签。
    pub verify_sign: bool,
}

impl R09Sign {
    /// 构造。
    pub fn new() -> R09Sign {
        R09Sign {
            self_sign: false,
            verify_sign: false,
        }
    }

    /// 签自查。
    pub fn sign_self(&mut self) {
        self.self_sign = true;
    }

    /// 签验证。
    pub fn sign_verify(&mut self) {
        self.verify_sign = true;
    }

    /// 双签齐。
    pub fn complete(&self) -> bool {
        self.self_sign && self.verify_sign
    }
}

// ─────────────────────────────────────────────────────────────
// 七件套
// ─────────────────────────────────────────────────────────────

/// 七件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SevenPiece {
    /// 范围。
    Scope,
    /// 计划。
    Plan,
    /// 核验。
    Verify,
    /// 对账。
    Reconcile,
    /// 清点。
    Inventory,
    /// 汇总。
    Summary,
    /// 报告。
    Report,
}

impl SevenPiece {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            SevenPiece::Scope => 0,
            SevenPiece::Plan => 1,
            SevenPiece::Verify => 2,
            SevenPiece::Reconcile => 3,
            SevenPiece::Inventory => 4,
            SevenPiece::Summary => 5,
            SevenPiece::Report => 6,
        }
    }

    /// 全集（七件）。
    pub fn all() -> [SevenPiece; 7] {
        [
            SevenPiece::Scope,
            SevenPiece::Plan,
            SevenPiece::Verify,
            SevenPiece::Reconcile,
            SevenPiece::Inventory,
            SevenPiece::Summary,
            SevenPiece::Report,
        ]
    }
}

/// 七件台账。
#[derive(Clone, Debug)]
pub struct SevenLedger {
    present: [bool; 7],
}

impl SevenLedger {
    /// 空。
    pub fn new() -> SevenLedger {
        SevenLedger {
            present: [false; 7],
        }
    }

    /// 标记到位。
    pub fn mark(&mut self, p: SevenPiece) {
        self.present[p.index() as usize] = true;
    }

    /// 七件齐。
    pub fn complete(&self) -> bool {
        self.present.iter().all(|b| *b)
    }

    /// 缺件。
    pub fn missing(&self) -> Vec<SevenPiece> {
        let mut bad = Vec::new();
        for p in SevenPiece::all() {
            if !self.present[p.index() as usize] {
                bad.push(p);
            }
        }
        bad
    }
}

impl Default for SevenLedger {
    fn default() -> SevenLedger {
        SevenLedger::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 对拍与门禁
// ─────────────────────────────────────────────────────────────

/// 对拍结果。
#[derive(Clone, Debug, Default)]
pub struct CrossCheck {
    /// 是否通过。
    pub passed: bool,
    /// 证据 id。
    pub evidence_id: u32,
}

impl CrossCheck {
    /// 构造。
    pub fn new() -> CrossCheck {
        CrossCheck {
            passed: false,
            evidence_id: 0,
        }
    }

    /// 合规：通过 + 有证据。
    pub fn ok(&self) -> bool {
        self.passed && self.evidence_id != 0
    }
}

/// 收口门禁。
#[derive(Clone, Debug)]
pub struct R09Gate {
    /// 七维通过位。
    dims: [bool; 7],
}

impl R09Gate {
    /// 全不过。
    pub fn new() -> R09Gate {
        R09Gate { dims: [false; 7] }
    }

    /// 全过。
    pub fn all_pass() -> R09Gate {
        R09Gate { dims: [true; 7] }
    }

    /// 设一维。
    pub fn set(&mut self, idx: usize, ok: bool) -> bool {
        if idx >= 7 {
            return false;
        }
        self.dims[idx] = ok;
        true
    }

    /// GO：七维全过。
    pub fn go(&self) -> bool {
        self.dims.iter().all(|b| *b)
    }

    /// 通过数。
    pub fn pass_count(&self) -> u32 {
        self.dims.iter().filter(|b| **b).count() as u32
    }
}

impl Default for R09Gate {
    fn default() -> R09Gate {
        R09Gate::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3774 判据集。
pub fn run_vr09n_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3774");

    // 0xA501 双签
    {
        let mut s = R09Sign::new();
        out.add("0xA501-缺双签拦", !s.complete(), "");
        s.sign_self();
        out.add("0xA501-单签拦", !s.complete(), "");
        s.sign_verify();
        out.add("0xA501-双签过", s.complete(), "");
    }

    // 0xA502 七件
    {
        let mut l = SevenLedger::new();
        out.add("0xA502-全缺检出", l.missing().len() == 7, "");
        for p in SevenPiece::all() {
            l.mark(p);
        }
        out.add("0xA502-七件齐过", l.complete(), "");
    }

    // 0xA503 对拍
    {
        let mut c = CrossCheck::new();
        out.add("0xA503-空对拍拦", !c.ok(), "");
        c.passed = true;
        c.evidence_id = 1;
        out.add("0xA503-有证据过", c.ok(), "");
    }

    // 0xA504 门禁
    {
        let mut g = R09Gate::new();
        out.add("0xA504-全不过NOGO", !g.go() && g.pass_count() == 0, "");
        for i in 0..7 {
            g.set(i, true);
        }
        out.add("0xA504-七维GO", g.go() && g.pass_count() == 7, "");
        out.add("0xA504-越界拒", !g.set(7, true), "");
    }

    out
}
