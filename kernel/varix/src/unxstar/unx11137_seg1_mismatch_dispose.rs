//! UNX-F11137 清账段一失配处置账（终重放失配闭环）（0x1f250 段）
//!
//! 纯功能模型：段一终重放里被判「失配」的案子，逐例归因、逐例处置，
//! 处置完才许闭合。归因四类：
//!   0 规则缺口（判据没写到的面）、1 实现偏差（规则有、代码没跟上）、
//!   2 期望笔误（复算期望写错）、3 时基漂移（环境计时与判据不同步）。
//! 处置四类：
//!   1 修复（回改实现）、2 重定期望（改期望并复核实测）、
//!   3 豁免（挂注记）、4 挂起（暂不动，收口前必须清）。
//!
//! 交叉纪律（本账的硬门）：
//!   - 规则缺口与实现偏差只能走「修复」或「挂起」；
//!   - 期望笔误只能走「重定期望」，且重定值必须等于实测、不得等于旧期望；
//!   - 时基漂移只能走「豁免」或「挂起」，豁免须挂注记；
//!   - 修复必须带修复凭证号（>0）；
//!   - 登记失配前先验「确属失配」：实测等于期望者不得入案。
//!
//! 收口条件：十二案全闭合、挂起零在途、两本直方账对平。
//!
//! 拒止面：案号越界、非失配入案、未归因即处置、归因/处置越界、处置与
//! 归因不符、凭证缺失或与实测不符、重复归因、挂起未清即收口、终重放
//! 缺账、封盘后动账。

use crate::alloc::format;
use crate::checks::CheckSet;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f250;
/// 案号越界
pub const DP_NO: u32 = SEG_BASE + 0x01;
/// 未归因即处置
pub const DP_ATTR: u32 = SEG_BASE + 0x02;
/// 归因/处置越界或二者不符
pub const DP_DISP: u32 = SEG_BASE + 0x03;
/// 案序倒挂或重复登记
pub const DP_ORDER: u32 = SEG_BASE + 0x04;
/// 挂起未清即收口/终重放
pub const DP_PENDING: u32 = SEG_BASE + 0x05;
/// 凭证缺失或与实测不符
pub const DP_EVID: u32 = SEG_BASE + 0x06;
/// 实测等于期望，不构成失配
pub const DP_MISMATCH: u32 = SEG_BASE + 0x07;
/// 重复归因或重复处置
pub const DP_DUP: u32 = SEG_BASE + 0x08;
/// 终重放缺账
pub const DP_REPLAY: u32 = SEG_BASE + 0x09;
/// 账册已封盘
pub const DP_FROZEN: u32 = SEG_BASE + 0x0a;

/// 失配案数。
pub const CASES: usize = 12;
/// 归因类数。
pub const ATTRS: usize = 4;
/// 处置类数。
pub const DISPS: usize = 4;

/// 归因：规则缺口。
pub const A_RULE: u8 = 0;
/// 归因：实现偏差。
pub const A_IMPL: u8 = 1;
/// 归因：期望笔误。
pub const A_EXPECT: u8 = 2;
/// 归因：时基漂移。
pub const A_DRIFT: u8 = 3;
/// 归因名。
pub const ATTR_NAMES: [&str; ATTRS] = ["规则缺口", "实现偏差", "期望笔误", "时基漂移"];

/// 处置：修复。
pub const D_FIX: u8 = 1;
/// 处置：重定期望。
pub const D_RESET: u8 = 2;
/// 处置：豁免。
pub const D_EXEMPT: u8 = 3;
/// 处置：挂起。
pub const D_HOLD: u8 = 4;
/// 处置名。
pub const DISP_NAMES: [&str; DISPS] = ["修复", "重定期望", "豁免", "挂起"];

/// 基线期望表。
pub const BASE_EXPECTED: [u32; CASES] = [22, 13, 111, 90, 92, 97, 22, 13, 111, 90, 92, 97];
/// 基线实测表（逐例均与期望不等）。
pub const BASE_GOT: [u32; CASES] = [0, 0, 4, 91, 0, 95, 0, 0, 0, 0, 0, 3];
/// 基线归因表。
pub const BASE_ATTR: [u8; CASES] = [1, 1, 0, 2, 3, 1, 0, 1, 1, 2, 3, 1];
/// 基线处置表。
pub const BASE_DISP: [u8; CASES] = [1, 1, 1, 2, 3, 1, 1, 1, 1, 2, 3, 1];
/// 基线凭证基底：修复落 PATCH_BASE+案号，豁免落 NOTE_BASE+案号。
pub const PATCH_BASE: u32 = 0x3700;
/// 豁免注记基底。
pub const NOTE_BASE: u32 = 0x3a00;
/// 归因直方基线：[规则缺口, 实现偏差, 期望笔误, 时基漂移]。
pub const ATTR_HIST: [u32; ATTRS] = [2, 6, 2, 2];
/// 处置直方基线：[修复, 重定期望, 豁免, 挂起]。
pub const DISP_HIST: [u32; DISPS] = [8, 2, 2, 0];
/// 挂起处置在直方中的位序。
pub const HOLD_POS: usize = 3;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 归因是否在闭集内。
pub fn attr_ok(a: u8) -> bool {
    (a as usize) < ATTRS
}

/// 处置是否在闭集内（1..=4）。
pub fn disp_ok(d: u8) -> bool {
    d >= D_FIX && d <= D_HOLD
}

/// 交叉纪律：处置是否与归因相符。
pub fn disp_allowed(attr: u8, disp: u8) -> bool {
    match attr {
        A_RULE | A_IMPL => disp == D_FIX || disp == D_HOLD,
        A_EXPECT => disp == D_RESET,
        A_DRIFT => disp == D_EXEMPT || disp == D_HOLD,
        _ => false,
    }
}

/// 归因名。
pub fn attr_name(a: u8) -> &'static str {
    match ATTR_NAMES.get(a as usize) {
        Some(n) => n,
        None => "越界",
    }
}

/// 处置名。
pub fn disp_name(d: u8) -> &'static str {
    if !disp_ok(d) {
        return "越界";
    }
    match DISP_NAMES.get((d - 1) as usize) {
        Some(n) => n,
        None => "越界",
    }
}

/// 是否确属失配。
pub fn is_mismatch(expected: u32, got: u32) -> bool {
    expected != got
}

/// 处置所需的凭证是否合规。
pub fn evidence_ok(disp: u8, evidence: u32, expected: u32, got: u32) -> bool {
    match disp {
        D_FIX => evidence != 0,
        D_RESET => evidence == got && evidence != expected,
        D_EXEMPT => evidence != 0,
        D_HOLD => true,
        _ => false,
    }
}

/// 一例失配案。
#[derive(Clone, Copy, Debug)]
pub struct MismatchCase {
    /// 案号（1..=12）。
    pub no: usize,
    expected: u32,
    got: u32,
    attr: u8,
    disp: u8,
    evidence: u32,
    attr_seen: bool,
    recorded: bool,
    closed: bool,
}

impl MismatchCase {
    /// 空白案卷。
    pub const fn new(no: usize) -> MismatchCase {
        MismatchCase {
            no,
            expected: 0,
            got: 0,
            attr: 0,
            disp: 0,
            evidence: 0,
            attr_seen: false,
            recorded: false,
            closed: false,
        }
    }

    /// 登记：实测等于期望者不得入案。
    pub fn record(&mut self, expected: u32, got: u32) -> Result<(), u32> {
        if self.recorded || self.closed {
            return Err(DP_DUP);
        }
        if !is_mismatch(expected, got) {
            return Err(DP_MISMATCH);
        }
        self.expected = expected;
        self.got = got;
        self.recorded = true;
        Ok(())
    }

    /// 归因。
    pub fn attribute(&mut self, a: u8) -> Result<(), u32> {
        if !self.recorded {
            return Err(DP_ORDER);
        }
        if self.closed {
            return Err(DP_DUP);
        }
        if !attr_ok(a) {
            return Err(DP_DISP);
        }
        if self.attr_seen || self.disp != 0 {
            return Err(DP_DUP);
        }
        self.attr = a;
        self.attr_seen = true;
        Ok(())
    }

    /// 处置：须先归因，处置与归因相符，凭证合规。
    pub fn dispose(&mut self, d: u8, evidence: u32) -> Result<(), u32> {
        if !self.recorded {
            return Err(DP_ORDER);
        }
        if self.closed {
            return Err(DP_DUP);
        }
        if self.disp != 0 {
            return Err(DP_DUP);
        }
        if !self.attr_seen {
            return Err(DP_ATTR);
        }
        if !disp_ok(d) || !disp_allowed(self.attr, d) {
            return Err(DP_DISP);
        }
        if !evidence_ok(d, evidence, self.expected, self.got) {
            return Err(DP_EVID);
        }
        self.disp = d;
        self.evidence = evidence;
        Ok(())
    }

    /// 是否已归因（规则缺口归因值为 0，故须另立布尔）。
    pub fn attributed(&self) -> bool {
        self.attr_seen
    }

    /// 收口：已处置且非挂起。
    pub fn close(&mut self) -> Result<(), u32> {
        if self.closed {
            return Ok(());
        }
        if self.disp == 0 || self.disp == D_HOLD {
            return Err(DP_PENDING);
        }
        self.closed = true;
        Ok(())
    }

    /// 期望值。
    pub fn expected(&self) -> u32 {
        self.expected
    }

    /// 实测值。
    pub fn got(&self) -> u32 {
        self.got
    }

    /// 归因值。
    pub fn attr(&self) -> u8 {
        self.attr
    }

    /// 处置值。
    pub fn disp(&self) -> u8 {
        self.disp
    }

    /// 凭证。
    pub fn evidence(&self) -> u32 {
        self.evidence
    }

    /// 是否在册。
    pub fn recorded(&self) -> bool {
        self.recorded
    }

    /// 是否闭合。
    pub fn closed(&self) -> bool {
        self.closed
    }

    /// 是否挂起在途。
    pub fn held(&self) -> bool {
        self.recorded && !self.closed && (self.disp == D_HOLD || self.disp == 0)
    }

    /// 案卷指纹。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFF ^ (self.no as u64);
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.expected as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.got as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= ((self.attr as u64) << 8) | (self.disp as u64);
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.evidence as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= if self.closed { 1 } else { 0 };
        h
    }
}

/// 处置账：十二案逐例归因处置。
#[derive(Clone, Debug)]
pub struct DispositionLedger {
    /// 段内基址。
    pub base: u32,
    cases: [MismatchCase; CASES],
    cursor: usize,
    rejects: u32,
    frozen: bool,
}

impl DispositionLedger {
    /// 新建十二张空白案卷。
    pub fn new(base: u32) -> DispositionLedger {
        let mut cases = [MismatchCase::new(1); CASES];
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = cases.get_mut(i) {
                *c = MismatchCase::new(i.wrapping_add(1));
            }
            i = i.wrapping_add(1);
        }
        DispositionLedger {
            base,
            cases,
            cursor: 0,
            rejects: 0,
            frozen: false,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 案卷只读。
    pub fn case(&self, no: usize) -> Option<MismatchCase> {
        if no == 0 || no > CASES {
            return None;
        }
        self.cases.get(no - 1).copied()
    }

    /// 登记失配（案号须紧接游标）。
    pub fn record(&mut self, no: usize, expected: u32, got: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(DP_FROZEN));
        }
        if no == 0 || no > CASES {
            return Err(self.fail(DP_NO));
        }
        if let Some(c) = self.cases.get(no - 1) {
            if c.recorded() {
                return Err(self.fail(DP_DUP));
            }
        }
        if no != self.cursor.wrapping_add(1) {
            return Err(self.fail(DP_ORDER));
        }
        let r = match self.cases.get_mut(no - 1) {
            Some(c) => c.record(expected, got),
            None => Err(DP_NO),
        };
        match r {
            Ok(()) => {
                self.cursor = no;
                Ok(())
            }
            Err(e) => Err(self.fail(e)),
        }
    }

    /// 归因。
    pub fn attribute(&mut self, no: usize, a: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(DP_FROZEN));
        }
        if no == 0 || no > CASES {
            return Err(self.fail(DP_NO));
        }
        let r = match self.cases.get_mut(no - 1) {
            Some(c) => c.attribute(a),
            None => Err(DP_NO),
        };
        match r {
            Ok(()) => Ok(()),
            Err(e) => Err(self.fail(e)),
        }
    }

    /// 处置。
    pub fn dispose(&mut self, no: usize, d: u8, evidence: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(DP_FROZEN));
        }
        if no == 0 || no > CASES {
            return Err(self.fail(DP_NO));
        }
        let r = match self.cases.get_mut(no - 1) {
            Some(c) => c.dispose(d, evidence),
            None => Err(DP_NO),
        };
        match r {
            Ok(()) => Ok(()),
            Err(e) => Err(self.fail(e)),
        }
    }

    /// 案卷收口。
    pub fn close_case(&mut self, no: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(DP_FROZEN));
        }
        if no == 0 || no > CASES {
            return Err(self.fail(DP_NO));
        }
        let r = match self.cases.get_mut(no - 1) {
            Some(c) => c.close(),
            None => Err(DP_NO),
        };
        match r {
            Ok(()) => Ok(()),
            Err(e) => Err(self.fail(e)),
        }
    }

    /// 已闭合案数。
    pub fn closed_cases(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.closed() {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 在途案数（未闭合，含挂起与未处置）。
    pub fn held_cases(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if !c.closed() {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 归因直方。
    pub fn attr_hist(&self) -> [u32; ATTRS] {
        let mut h = [0u32; ATTRS];
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.recorded() {
                    if let Some(slot) = h.get_mut(c.attr() as usize) {
                        *slot = slot.wrapping_add(1);
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        h
    }

    /// 处置直方。
    pub fn disp_hist(&self) -> [u32; DISPS] {
        let mut h = [0u32; DISPS];
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                let d = c.disp();
                if d >= D_FIX && d <= D_HOLD {
                    if let Some(slot) = h.get_mut((d - 1) as usize) {
                        *slot = slot.wrapping_add(1);
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        h
    }

    /// 交叉纪律全合法。
    pub fn cross_ok(&self) -> bool {
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                if c.recorded() && !c.closed() {
                    return false;
                }
                if c.recorded() && c.disp() != D_RESET && !disp_allowed(c.attr(), c.disp()) {
                    return false;
                }
                if c.recorded() && c.disp() == D_RESET {
                    if !disp_allowed(c.attr(), c.disp()) {
                        return false;
                    }
                    if c.evidence() != c.got() || c.evidence() == c.expected() {
                        return false;
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 拒止计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否封盘。
    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// 总账指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        for b in self.base.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h ^= if self.frozen { 1 } else { 0 };
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.cursor as u64;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.rejects as u64;
        h = h.wrapping_mul(FNV_PRIME);
        let mut i = 0usize;
        while i < CASES {
            if let Some(c) = self.cases.get(i) {
                h ^= c.digest();
                h = h.wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        h
    }

    /// 封盘，幂等。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Ok(self.fold());
        }
        self.frozen = true;
        Ok(self.fold())
    }

    /// 终重放：十二案全闭、零在途、两本直方对平，然后封盘。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.closed_cases() != CASES {
            return Err(self.fail(DP_REPLAY));
        }
        if self.held_cases() != 0 {
            return Err(self.fail(DP_PENDING));
        }
        if self.attr_hist() != ATTR_HIST || self.disp_hist() != DISP_HIST {
            return Err(self.fail(DP_REPLAY));
        }
        self.freeze()
    }
}

/// 按处置类给出凭证：修复落凭证号、重定落实测值、豁免落注记号。
pub fn evidence_for(disp: u8, no: usize, got: u32) -> u32 {
    match disp {
        D_FIX => PATCH_BASE.wrapping_add(no as u32),
        D_RESET => got,
        D_EXEMPT => NOTE_BASE.wrapping_add(no as u32),
        _ => 0,
    }
}

/// 搭建处置账：十二案登记、归因、处置、收口、终重放。
pub fn build_disposition(base: u32) -> (DispositionLedger, Result<u64, u32>) {
    let mut g = DispositionLedger::new(base);
    let mut no = 1usize;
    while no <= CASES {
        let e = match BASE_EXPECTED.get(no.wrapping_sub(1)) {
            Some(v) => *v,
            None => 0,
        };
        let got = match BASE_GOT.get(no.wrapping_sub(1)) {
            Some(v) => *v,
            None => 0,
        };
        let a = match BASE_ATTR.get(no.wrapping_sub(1)) {
            Some(v) => *v,
            None => A_RULE,
        };
        let d = match BASE_DISP.get(no.wrapping_sub(1)) {
            Some(v) => *v,
            None => D_FIX,
        };
        if g.record(no, e, got).is_err() {
            return (g, Err(DP_MISMATCH));
        }
        if g.attribute(no, a).is_err() {
            return (g, Err(DP_DISP));
        }
        if g.dispose(no, d, evidence_for(d, no, got)).is_err() {
            return (g, Err(DP_EVID));
        }
        if g.close_case(no).is_err() {
            return (g, Err(DP_PENDING));
        }
        no = no.wrapping_add(1);
    }
    let out = g.replay();
    (g, out)
}

/// 四路注入：非失配入案、未归因即处置、处置与归因不符、挂起未清即收口。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    // ① 实测等于期望不得入案。
    let mut a = DispositionLedger::new(0x3800);
    out[0] = a.record(1, 22, 22).err() == Some(DP_MISMATCH) && a.rejects() == 1;

    // ② 未归因即处置落 DP_ATTR。
    let mut b = DispositionLedger::new(0x3801);
    let ok2 = b.record(1, 22, 0).is_ok();
    out[1] = ok2 && b.dispose(1, D_FIX, PATCH_BASE).err() == Some(DP_ATTR) && b.rejects() == 1;

    // ③ 期望笔误却走修复，处置与归因不符落 DP_DISP。
    let mut c = DispositionLedger::new(0x3802);
    let ok3 = c.record(1, 22, 0).is_ok() && c.attribute(1, A_EXPECT).is_ok();
    out[2] = ok3 && c.dispose(1, D_FIX, PATCH_BASE).err() == Some(DP_DISP) && c.rejects() == 1;

    // ④ 挂起未清即收口落 DP_PENDING。
    let mut d = DispositionLedger::new(0x3803);
    let mut ok4 = d.record(1, 22, 0).is_ok() && d.attribute(1, A_IMPL).is_ok();
    ok4 = ok4 && d.dispose(1, D_HOLD, 0).is_ok();
    out[3] = ok4 && d.close_case(1).err() == Some(DP_PENDING) && d.rejects() == 1;

    out
}

/// 判据集：D1~D13。
pub fn run_unx11137_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11137.seg1_mismatch_dispose");
    let (g, r) = build_disposition(0x1137);

    // D1 常量面。
    let c1 = SEG_BASE == 0x1f250
        && CASES == 12
        && ATTRS == 4
        && DISPS == 4
        && ATTR_NAMES.len() == ATTRS
        && DISP_NAMES.len() == DISPS
        && BASE_EXPECTED.len() == CASES
        && BASE_GOT.len() == CASES
        && BASE_ATTR.len() == CASES
        && BASE_DISP.len() == CASES;
    s.add("D1-常量面", c1, "十二案、四归因、四处置");

    // D2 十二案在册且逐例确属失配。
    let mut c2 = true;
    let mut i2 = 1usize;
    while i2 <= CASES {
        match g.case(i2) {
            Some(c) => {
                c2 = c2 && c.recorded() && is_mismatch(c.expected(), c.got());
            }
            None => c2 = false,
        }
        i2 = i2.wrapping_add(1);
    }
    s.add("D2-十二案在册", c2, "逐例实测≠期望");

    // D3 闭合 12/12，零在途。
    s.add_dyn(
        "D3-闭合零在途",
        g.closed_cases() == CASES && g.held_cases() == 0,
        &format!("closed={}/{} held={}", g.closed_cases(), CASES, g.held_cases()),
    );

    // D4 归因直方对平。
    s.add_dyn(
        "D4-归因直方",
        g.attr_hist() == ATTR_HIST,
        &format!("attr={:?} want={:?}", g.attr_hist(), ATTR_HIST),
    );

    // D5 处置直方对平（挂起零）。
    s.add_dyn(
        "D5-处置直方",
        g.disp_hist() == DISP_HIST && g.disp_hist()[HOLD_POS] == 0,
        &format!("disp={:?} want={:?}", g.disp_hist(), DISP_HIST),
    );

    // D6 交叉纪律全合法。
    s.add("D6-交叉纪律", g.cross_ok(), "归因与处置逐例相符、凭证合规");

    // D7 重定期望两案：重定值等于实测且异于旧期望。
    let mut c7 = 0usize;
    let mut i7 = 1usize;
    while i7 <= CASES {
        if let Some(c) = g.case(i7) {
            if c.disp() == D_RESET {
                if c.evidence() == c.got() && c.evidence() != c.expected() {
                    c7 = c7.wrapping_add(1);
                }
            }
        }
        i7 = i7.wrapping_add(1);
    }
    s.add_dyn("D7-重定期望", c7 == 2, &format!("reset_ok={}", c7));

    // D8 修复券齐：凡修复处置皆带非零凭证。
    let mut c8 = 0usize;
    let mut i8 = 1usize;
    while i8 <= CASES {
        if let Some(c) = g.case(i8) {
            if c.disp() == D_FIX && c.evidence() != 0 {
                c8 = c8.wrapping_add(1);
            }
        }
        i8 = i8.wrapping_add(1);
    }
    s.add_dyn("D8-修复券齐", c8 == DISP_HIST[0] as usize, &format!("fix={}", c8));

    // D9 终重放成立并封盘。
    s.add("D9-终重放封盘", r.is_ok() && g.frozen(), "终重放通过并封盘");

    // D10 重放与封盘幂等。
    let mut a10 = g.clone();
    let before = a10.fold();
    let again = a10.replay();
    let mut b10 = DispositionLedger::new(0x1137);
    let f1 = b10.freeze();
    let f2 = b10.freeze();
    s.add(
        "D10-幂等",
        again.is_ok() && a10.fold() == before && f1.is_ok() && f1 == f2,
        "重放二次同指纹；封盘二次同指纹",
    );

    // D11 拒止优先序。
    let mut c11 = g.clone();
    let z1 = c11.record(CASES + 1, 1, 2).err();
    let mut c11b = DispositionLedger::new(0x1138);
    let z2 = c11b.record(CASES + 1, 1, 2).err();
    s.add(
        "D11-拒止优先序",
        z1 == Some(DP_FROZEN) && z2 == Some(DP_NO),
        "封盘优先于案号；未封盘则 DP_NO",
    );

    // D12 四路注入。
    let inj = inject_four();
    s.add(
        "D12-四路注入",
        inj[0] && inj[1] && inj[2] && inj[3],
        "非失配/未归因/不符/挂起四路各自发火",
    );

    // D13 逐例归零 + 指纹灵敏度。
    let (g2, _) = build_disposition(0x1139);
    s.add(
        "D13-归零与指纹",
        g.rejects() == 0
            && g.closed_cases() == CASES
            && g.cross_ok()
            && g2.fold() != g.fold(),
        "rejects=0，十二案全闭，异基址指纹相异",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11137_flow() {
        let (g, r) = build_disposition(0x6000);
        assert!(r.is_ok());
        assert!(g.frozen());
        assert_eq!(g.closed_cases(), CASES);
        assert_eq!(g.held_cases(), 0);
        assert_eq!(g.attr_hist(), ATTR_HIST);
        assert_eq!(g.disp_hist(), DISP_HIST);
        assert_eq!(g.rejects(), 0);
        assert!(g.cross_ok());
        let first = match g.case(1) {
            Some(c) => c,
            None => return,
        };
        assert_eq!(first.attr(), A_IMPL);
        assert_eq!(first.disp(), D_FIX);
        assert_eq!(first.evidence(), PATCH_BASE + 1);
    }

    #[test]
    fn f11137_gates() {
        let mut g = DispositionLedger::new(0x6001);
        assert_eq!(g.record(2, 22, 0).err(), Some(DP_ORDER));
        assert_eq!(g.record(1, 22, 22).err(), Some(DP_MISMATCH));
        assert!(g.record(1, 22, 0).is_ok());
        assert_eq!(g.record(1, 13, 0).err(), Some(DP_DUP));
        assert_eq!(g.dispose(1, D_FIX, PATCH_BASE).err(), Some(DP_ATTR));
        assert_eq!(g.attribute(1, 9).err(), Some(DP_DISP));
        assert!(g.attribute(1, A_EXPECT).is_ok());
        assert_eq!(g.attribute(1, A_IMPL).err(), Some(DP_DUP));
        assert_eq!(g.dispose(1, D_FIX, PATCH_BASE).err(), Some(DP_DISP));
        assert_eq!(g.dispose(1, D_RESET, 1).err(), Some(DP_EVID));
        assert!(g.dispose(1, D_RESET, 0).is_ok());
        assert_eq!(g.close_case(1).ok(), Some(()));
        assert_eq!(g.rejects(), 8);
        assert!(g.freeze().is_ok());
        assert_eq!(g.attribute(2, A_RULE).err(), Some(DP_FROZEN));
        assert_eq!(g.rejects(), 9);
    }

    #[test]
    fn f11137_rules() {
        assert!(attr_ok(A_RULE));
        assert!(attr_ok(A_DRIFT));
        assert!(!attr_ok(4));
        assert!(!disp_ok(0));
        assert!(disp_ok(D_HOLD));
        assert!(!disp_ok(5));
        assert!(disp_allowed(A_RULE, D_FIX));
        assert!(disp_allowed(A_IMPL, D_HOLD));
        assert!(!disp_allowed(A_IMPL, D_EXEMPT));
        assert!(disp_allowed(A_EXPECT, D_RESET));
        assert!(disp_allowed(A_DRIFT, D_EXEMPT));
        assert!(!disp_allowed(A_DRIFT, D_FIX));
        assert!(!disp_allowed(7, D_FIX));
        assert_eq!(attr_name(A_IMPL), "实现偏差");
        assert_eq!(disp_name(D_RESET), "重定期望");
        assert_eq!(disp_name(0), "越界");
        assert_eq!(attr_name(9), "越界");
        assert!(is_mismatch(22, 0));
        assert!(!is_mismatch(22, 22));
        // 凭证面。
        assert!(evidence_ok(D_FIX, 1, 22, 0));
        assert!(!evidence_ok(D_FIX, 0, 22, 0));
        assert!(evidence_ok(D_RESET, 0, 22, 0));
        assert!(!evidence_ok(D_RESET, 22, 22, 0));
        assert!(!evidence_ok(D_EXEMPT, 0, 22, 0));
        assert!(evidence_ok(D_HOLD, 0, 22, 0));
        assert!(!evidence_ok(0, 1, 22, 0));
        // 归因值为 0（规则缺口）不混淆「未归因」。
        let mut c = MismatchCase::new(1);
        assert!(c.record(22, 0).is_ok());
        assert!(!c.attributed());
        assert!(c.attribute(A_RULE).is_ok());
        assert!(c.attributed());
        assert_eq!(c.attr(), 0);
    }

    #[test]
    fn f11137_injection() {
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn f11137_checks_green() {
        let cs = run_unx11137_checks();
        assert!(cs.all_passed(), "checks red in unx11137");
    }
}
