//! UNX-F1594 域收官宣告条件机（五证据齐备才收官） · 0x79280段
//!
//! 判据原文（UNX-F1594-J1 摘录）：「宣告条件机生效（①总回归全绿 ②行数守恒
//! ③三源一致 ④移交终账双 100% ⑤风险清账零悬空，五条件硬断言），缺项宣告
//! 拦截 10/10」；完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 五证据硬断言：总回归全绿（见证数=判据数 800）/行数守恒（见证数=240000）
//!    /三源一致（见证数=3）/移交终账双 100%（见证数=2）/风险清账零悬空
//!    （见证数=0），取证时逐证校验见证数与指纹，伪造证据不入册；
//! 2. 缺项宣告拦截 10/10：五证各缺一（少取证）加五证各伪一（零指纹）共十种
//!    缺项注入，逐种必须在宣告口被拦截，缺项宣告零漏网；
//! 3. 相位机 取证中→已齐备→已宣告 单向，倒退显性拒；已宣告后取证口全拒，
//!    宣告记录一旦落档不可改写；
//! 4. 宣告记录：五证逐行（证名/见证数/指纹）+ 宣告序，落档非空可查；
//! 5. 诊断码闭集 0x79281..0x7928C，Err 用 Result<T,u32>，中文诊断文案带
//!    可操作建议。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡 B09 给定值；本批窗口 0x79200..=0x793C0）。
pub const SEG_BASE: u32 = 0x79280;

/// 宣告证据数（五条件硬断言）。
pub const EVIDENCE_COUNT: usize = 5;

/// 缺项注入种数（判据原文 10/10）。
pub const MISSING_PROBES: usize = 10;

/// 总回归全绿见证数（域判据条数）。
pub const WITNESS_GREEN: u64 = 800;

/// 行数守恒见证数（域账行数）。
pub const WITNESS_LINES: u64 = 240_000;

/// 三源一致见证数。
pub const WITNESS_TRI: u64 = 3;

/// 移交终账双 100% 见证数（两份终账各百份）。
pub const WITNESS_DOUBLE: u64 = 2;

/// 风险清账零悬空见证数。
pub const WITNESS_ZERO_HANG: u64 = 0;

/// 指纹折叠种子（FNV-1a 偏移基）。
pub const DIGEST_SEED: u64 = 0xCBF2_9CE4_8422_2325;

/// 指纹折叠质数（FNV-1a 质数）。
pub const DIGEST_PRIME: u64 = 0x100_0000_01B3;

/// 收官宣告条件机诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 相位倒退（取证中→已齐备→已宣告 单向）。
    EPhase,
    /// 重复取证（同证已录）。
    EDupMark,
    /// 证据种类册外。
    EUnknown,
    /// 伪造证据（指纹为零）。
    EForged,
    /// 见证数不符（证据数值与钦定见证不一致）。
    EWitness,
    /// 缺项宣告（五证未齐即宣告）。
    EMissing,
    /// 已宣告后取证拒。
    ESealed,
    /// 未齐备先取宣告记录。
    EOrder,
    /// 空证据册转齐备拒。
    EEmpty,
    /// 未齐备相位（齐备前置未过）。
    ENotReady,
    /// 证据名与名册错位。
    EName,
    /// 证据指纹重放（两异证同指纹）。
    EReplay,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EPhase => 0x01,
                Diag::EDupMark => 0x02,
                Diag::EUnknown => 0x03,
                Diag::EForged => 0x04,
                Diag::EWitness => 0x05,
                Diag::EMissing => 0x06,
                Diag::ESealed => 0x07,
                Diag::EOrder => 0x08,
                Diag::EEmpty => 0x09,
                Diag::ENotReady => 0x0a,
                Diag::EName => 0x0b,
                Diag::EReplay => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EPhase => "相位倒退：取证中→已齐备→已宣告 单向，倒退即拒",
            Diag::EDupMark => "重复取证：该证已录且指纹未变，勿重录覆盖",
            Diag::EUnknown => "证据种类册外：五条件按名册取证，册外证不收",
            Diag::EForged => "伪造证据：指纹为零视为空证，先重算指纹再取证",
            Diag::EWitness => "见证数不符：证据数值与钦定见证不一致，先核数再取证",
            Diag::EMissing => "缺项宣告：五条件硬断言缺一不可，逐证补齐再宣告",
            Diag::ESealed => "已宣告后取证拒：宣告已落档，改动走下一轮收官",
            Diag::EOrder => "未齐备先取记录：宣告记录在宣告后生成，先齐备再宣告",
            Diag::EEmpty => "空证据册：无证不构成齐备，先逐证取证",
            Diag::ENotReady => "未齐备相位：齐备收束未过，先补证再收束",
            Diag::EName => "证据名错位：证槽与名册错位，按名册重排证据册",
            Diag::EReplay => "证据指纹重放：两异证同指纹必有一伪，逐证重算",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EPhase,
    Diag::EDupMark,
    Diag::EUnknown,
    Diag::EForged,
    Diag::EWitness,
    Diag::EMissing,
    Diag::ESealed,
    Diag::EOrder,
    Diag::EEmpty,
    Diag::ENotReady,
    Diag::EName,
    Diag::EReplay,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x79281..0x7928C 闭集内"
}

/// 五证据种类（判据原文钦定顺序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Evidence {
    /// ①总回归全绿。
    TotalGreen,
    /// ②行数守恒。
    LineConserve,
    /// ③三源一致。
    TriSource,
    /// ④移交终账双 100%。
    HandoverDouble,
    /// ⑤风险清账零悬空。
    RiskClear,
}

impl Evidence {
    /// 证名。
    pub fn name(self) -> &'static str {
        match self {
            Evidence::TotalGreen => "总回归全绿",
            Evidence::LineConserve => "行数守恒",
            Evidence::TriSource => "三源一致",
            Evidence::HandoverDouble => "移交终账双百",
            Evidence::RiskClear => "风险清账零悬空",
        }
    }

    /// 钦定见证数。
    pub fn witness(self) -> u64 {
        match self {
            Evidence::TotalGreen => WITNESS_GREEN,
            Evidence::LineConserve => WITNESS_LINES,
            Evidence::TriSource => WITNESS_TRI,
            Evidence::HandoverDouble => WITNESS_DOUBLE,
            Evidence::RiskClear => WITNESS_ZERO_HANG,
        }
    }

    /// 证槽（0..EVIDENCE_COUNT）。
    pub fn slot(self) -> usize {
        match self {
            Evidence::TotalGreen => 0,
            Evidence::LineConserve => 1,
            Evidence::TriSource => 2,
            Evidence::HandoverDouble => 3,
            Evidence::RiskClear => 4,
        }
    }
}

/// 五证据闭集数组（与判据原文序一致）。
pub const EVIDENCE_ALL: &[Evidence] = &[
    Evidence::TotalGreen,
    Evidence::LineConserve,
    Evidence::TriSource,
    Evidence::HandoverDouble,
    Evidence::RiskClear,
];

/// 宣告条件机相位（取证中→已齐备→已宣告 单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimPhase {
    /// 取证中：逐证登记。
    Gathering,
    /// 已齐备：五证硬断言全过。
    Ready,
    /// 已宣告：收官宣告落档。
    Declared,
}

impl ClaimPhase {
    /// 序数（倒退检测用）。
    pub fn ord(self) -> u8 {
        match self {
            ClaimPhase::Gathering => 0,
            ClaimPhase::Ready => 1,
            ClaimPhase::Declared => 2,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            ClaimPhase::Gathering => "取证中",
            ClaimPhase::Ready => "已齐备",
            ClaimPhase::Declared => "已宣告",
        }
    }
}

/// 已录证据（指纹 + 见证数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkedEvidence {
    /// 证据种类。
    pub kind: Evidence,
    /// 证据指纹（非零）。
    pub digest: u64,
    /// 见证数（与钦定见证一致）。
    pub witness: u64,
}

/// 收官宣告条件机。
#[derive(Clone, Debug)]
pub struct ClaimMachine {
    phase: ClaimPhase,
    marks: Vec<MarkedEvidence>,
    /// 宣告序（宣告时递增）。
    declare_seq: u32,
    /// 结构性拒绝计数。
    rejects: u32,
}

impl ClaimMachine {
    /// 新条件机（取证中）。
    pub fn new() -> ClaimMachine {
        ClaimMachine {
            phase: ClaimPhase::Gathering,
            marks: Vec::new(),
            declare_seq: 0,
            rejects: 0,
        }
    }

    /// 相位。
    pub fn phase(&self) -> ClaimPhase {
        self.phase
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 宣告序。
    pub fn declare_seq(&self) -> u32 {
        self.declare_seq
    }

    /// 已取证数。
    pub fn marked(&self) -> usize {
        self.marks.len()
    }

    /// 取一证（种类册外/指纹零/见证数不符/重复/重放逐门拒）。
    pub fn mark(&mut self, kind: Evidence, witness: u64, digest: u64) -> Result<(), u32> {
        if self.phase == ClaimPhase::Declared {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        if !EVIDENCE_ALL.iter().any(|e| *e == kind) {
            self.rejects += 1;
            return Err(Diag::EUnknown.val());
        }
        if digest == 0 {
            self.rejects += 1;
            return Err(Diag::EForged.val());
        }
        if witness != kind.witness() {
            self.rejects += 1;
            return Err(Diag::EWitness.val());
        }
        if self.marks.iter().any(|m| m.kind == kind) {
            self.rejects += 1;
            return Err(Diag::EDupMark.val());
        }
        if self.marks.iter().any(|m| m.digest == digest) {
            self.rejects += 1;
            return Err(Diag::EReplay.val());
        }
        self.marks.push(MarkedEvidence {
            kind,
            digest,
            witness,
        });
        Ok(())
    }

    /// 按种类查已录证据。
    pub fn mark_of(&self, kind: Evidence) -> Option<&MarkedEvidence> {
        self.marks.iter().find(|m| m.kind == kind)
    }

    /// 缺项清单（未取证种类的证槽，按名册序）。
    pub fn missing(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for e in EVIDENCE_ALL {
            if self.mark_of(*e).is_none() {
                out.push(e.slot());
            }
        }
        out
    }

    /// 五条件硬断言（五证齐且逐证指纹见证齐备）。
    pub fn all_met(&self) -> bool {
        if self.marks.len() != EVIDENCE_COUNT {
            return false;
        }
        for e in EVIDENCE_ALL {
            match self.mark_of(*e) {
                Some(m) => {
                    if m.witness != e.witness() || m.digest == 0 {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }

    /// 齐备收束（五证齐才转已齐备；单向，倒退拒）。
    pub fn settle(&mut self) -> Result<(), u32> {
        if self.phase != ClaimPhase::Gathering {
            self.rejects += 1;
            return Err(Diag::EPhase.val());
        }
        if self.marks.is_empty() {
            return Err(Diag::EEmpty.val());
        }
        if !self.all_met() {
            return Err(Diag::ENotReady.val());
        }
        self.phase = ClaimPhase::Ready;
        Ok(())
    }

    /// 宣告（五条件硬断言：缺项一律拦截；齐备即宣告落档）。
    pub fn declare(&mut self) -> Result<String, u32> {
        if self.phase == ClaimPhase::Declared {
            self.rejects += 1;
            return Err(Diag::EPhase.val());
        }
        let missing = self.missing();
        if !missing.is_empty() {
            self.rejects += 1;
            return Err(Diag::EMissing.val());
        }
        if !self.all_met() {
            return Err(Diag::ENotReady.val());
        }
        self.declare_seq += 1;
        self.phase = ClaimPhase::Declared;
        Ok(self.record())
    }

    /// 宣告记录（宣告后生成；未宣告拒）。
    pub fn record(&self) -> String {
        if self.phase != ClaimPhase::Declared {
            return String::new();
        }
        let mut out = format!("收官宣告 序{}\n", self.declare_seq);
        for e in EVIDENCE_ALL {
            let m = self.mark_of(*e);
            let (digest, witness) = match m {
                Some(x) => (x.digest, x.witness),
                None => (0, 0),
            };
            out.push_str(&format!(
                "{} 见证{} 指纹{:016x}\n",
                e.name(),
                witness,
                digest
            ));
        }
        out
    }
}

/// 证据指纹（FNV-1a 整数折叠：证名 + 见证数混折）。
pub fn evidence_digest(kind: Evidence, witness: u64) -> u64 {
    let mut h = DIGEST_SEED;
    for b in kind.name().as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(DIGEST_PRIME);
    }
    let mut w = witness;
    let mut k = 0u32;
    while k < 8 {
        h ^= w & 0xFF;
        h = h.wrapping_mul(DIGEST_PRIME);
        w >>= 8;
        k += 1;
    }
    h
}

/// 标准条件机：五证齐备（取证中相位，未宣告）。
pub fn build_machine() -> ClaimMachine {
    let mut m = ClaimMachine::new();
    for e in EVIDENCE_ALL {
        let _ = m.mark(*e, e.witness(), evidence_digest(*e, e.witness()));
    }
    m
}

/// 缺项宣告拦截 10/10：十种缺项注入逐种构造，逐种必须被宣告口拦截。
/// 前五种：各少取证一证；后五种：各伪造一证（指纹为零）。
/// 返回十布尔（true=该注入被拦截）。
pub fn missing_matrix() -> [bool; MISSING_PROBES] {
    let mut out = [false; MISSING_PROBES];
    let mut probe = 0usize;
    // 前五种：少取证一证。
    while probe < EVIDENCE_COUNT {
        let mut m = ClaimMachine::new();
        let mut k = 0usize;
        while k < EVIDENCE_COUNT {
            let kind = match EVIDENCE_ALL.get(k) {
                Some(x) => *x,
                None => break,
            };
            if k != probe {
                let _ = m.mark(kind, kind.witness(), evidence_digest(kind, kind.witness()));
            }
            k += 1;
        }
        let intercepted = m.declare() == Err(Diag::EMissing.val()) && m.phase() != ClaimPhase::Declared;
        if let Some(cell) = out.get_mut(probe) {
            *cell = intercepted;
        }
        probe += 1;
    }
    // 后五种：伪造一证（零指纹取证被拒后该证缺项）。
    while probe < MISSING_PROBES {
        let forged_slot = probe - EVIDENCE_COUNT;
        let mut m = ClaimMachine::new();
        let mut k = 0usize;
        while k < EVIDENCE_COUNT {
            let kind = match EVIDENCE_ALL.get(k) {
                Some(x) => *x,
                None => break,
            };
            if k == forged_slot {
                // 伪造：零指纹取证必被取证口拒，证不入册。
                let forged = m.mark(kind, kind.witness(), 0) == Err(Diag::EForged.val());
                let _ = forged;
            } else {
                let _ = m.mark(kind, kind.witness(), evidence_digest(kind, kind.witness()));
            }
            k += 1;
        }
        let intercepted = m.declare() == Err(Diag::EMissing.val())
            && m.mark_of(match EVIDENCE_ALL.get(forged_slot) {
                Some(x) => *x,
                None => Evidence::TotalGreen,
            })
            .is_none();
        if let Some(cell) = out.get_mut(probe) {
            *cell = intercepted;
        }
        probe += 1;
    }
    out
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B09 派工卡 0x79200..=0x793C0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x79200 || SEG_BASE > 0x793C0 {
        return false;
    }
    SEG_BASE == 0x79280
}

/// 五证据闭集（不重不漏、槽位与名册对齐）。
fn evidence_closed_ok() -> bool {
    if EVIDENCE_ALL.len() != EVIDENCE_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < EVIDENCE_ALL.len() {
        let a = match EVIDENCE_ALL.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if a.slot() != i {
            return false;
        }
        let mut j = i + 1;
        while j < EVIDENCE_ALL.len() {
            let b = match EVIDENCE_ALL.get(j) {
                Some(x) => *x,
                None => return false,
            };
            if a == b {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 宣告条件机生效（五证齐备 + 宣告落档 + 记录非空）。
fn declared_ok(m: &ClaimMachine) -> bool {
    m.phase() == ClaimPhase::Declared && m.declare_seq() == 1 && !m.record().is_empty()
}

/// 缺项宣告拦截 10/10。
fn missing_all_ok() -> bool {
    missing_matrix() == [true; MISSING_PROBES]
}

/// 双跑一致（同构输入两次构建宣告记录一致且非空）。
fn dual_run_ok() -> bool {
    let mut a = build_machine();
    let mut b = build_machine();
    let _ = a.settle();
    let ra = a.declare();
    let _ = b.settle();
    let rb = b.declare();
    ra == rb && ra.is_ok()
}

/// 收官宣告判据：UNX-F1594 域收官宣告条件机。
pub fn run_unx1594_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1594");
    let mut machine = build_machine();

    s.add_dyn(
        "F1594-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1594-02 五证据闭集",
        evidence_closed_ok(),
        &format!("evidence={}/{}", EVIDENCE_ALL.len(), EVIDENCE_COUNT),
    );

    // 正常流：五证齐备硬断言全过。
    s.add_dyn(
        "F1594-03 五证齐备硬断言",
        machine.marked() == EVIDENCE_COUNT && machine.all_met() && machine.missing().is_empty(),
        &format!("marked={} phase={}", machine.marked(), machine.phase().name()),
    );

    // 取证门：伪造零指纹/见证不符逐门拒。
    let mut g = ClaimMachine::new();
    let wrong_witness = g.mark(Evidence::TotalGreen, 1, 1) == Err(Diag::EWitness.val());
    let forged = g.mark(Evidence::LineConserve, WITNESS_LINES, 0) == Err(Diag::EForged.val());
    let tri_bad = g.mark(Evidence::TriSource, 9, 3) == Err(Diag::EWitness.val());
    s.add_dyn(
        "F1594-04 取证伪造与见证逐门拒",
        wrong_witness && forged && tri_bad,
        &format!("rejects={}", g.rejects()),
    );

    // 重复取证显性拒（先取证入册，再重录同证）。
    let _ = g.mark(Evidence::TotalGreen, WITNESS_GREEN, 21);
    let dup = g.mark(Evidence::TotalGreen, WITNESS_GREEN, 22) == Err(Diag::EDupMark.val());
    s.add_dyn(
        "F1594-05 重复取证显性拒",
        dup,
        &format!("rejects={}", g.rejects()),
    );

    // 指纹重放显性拒（两异证同指纹）。
    let mut rp = ClaimMachine::new();
    let _ = rp.mark(Evidence::TotalGreen, WITNESS_GREEN, 777);
    let replay = rp.mark(Evidence::TriSource, WITNESS_TRI, 777) == Err(Diag::EReplay.val());
    s.add_dyn(
        "F1594-06 指纹重放显性拒",
        replay,
        &format!("replay_rejected={}", replay),
    );

    // 缺项宣告拦截 10/10。
    s.add_dyn(
        "F1594-07 缺项宣告拦截十种全拦",
        missing_all_ok(),
        &format!("matrix={:?}", missing_matrix()),
    );

    // 齐备收束：转已齐备；空册收束显性拒。
    let settle_ok = machine.settle() == Ok(()) && machine.phase() == ClaimPhase::Ready;
    let mut empty = ClaimMachine::new();
    let empty_rej = empty.settle() == Err(Diag::EEmpty.val());
    s.add_dyn(
        "F1594-08 齐备收束与空册拒",
        settle_ok && empty_rej,
        &format!("phase={} empty_phase={}", machine.phase().name(), empty.phase().name()),
    );

    // 宣告落档：记录非空含五证名与宣告序。
    let declared = machine.declare();
    let record_ok = match &declared {
        Ok(t) => {
            !t.is_empty()
                && t.contains("总回归全绿")
                && t.contains("行数守恒")
                && t.contains("三源一致")
                && t.contains("移交终账双百")
                && t.contains("风险清账零悬空")
        }
        Err(_) => false,
    };
    s.add_dyn(
        "F1594-09 宣告落档五证全名",
        record_ok && declared_ok(&machine),
        &format!("bytes={}", declared.as_ref().map(|t| t.len()).unwrap_or(0)),
    );

    // 已宣告后取证显性拒、重复宣告显性拒。
    let sealed = machine.mark(Evidence::TotalGreen, WITNESS_GREEN, 42) == Err(Diag::ESealed.val());
    let re_declare = machine.declare() == Err(Diag::EPhase.val());
    s.add_dyn(
        "F1594-10 已宣告后两口全拒",
        sealed && re_declare && machine.phase().ord() == 2,
        &format!("phase_ord={}", machine.phase().ord()),
    );

    // 宣告记录先决：未宣告记录为空（防记录面空跑）。
    let early_record = ClaimMachine::new().record().is_empty();
    s.add_dyn(
        "F1594-11 未宣告记录显性为空",
        early_record,
        &format!("early_empty={}", early_record),
    );

    // 双跑一致。
    s.add_dyn(
        "F1594-12 双跑一致",
        dual_run_ok(),
        &format!("dual={}", dual_run_ok()),
    );

    // 诊断码闭集。
    s.add_dyn(
        "F1594-13 诊断码闭集",
        ALL.len() == 12 && detail(SEG_BASE + 0x01) == Diag::EPhase.detail(),
        &format!("all={} seg=0x{:05x}", ALL.len(), SEG_BASE),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::EPhase.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::EReplay.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert!(detail(SEG_BASE + 0x70).starts_with("未知诊断码"));
    }

    #[test]
    fn seg_guard() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x79280);
    }

    #[test]
    fn evidence_closed_set() {
        assert_eq!(EVIDENCE_ALL.len(), EVIDENCE_COUNT);
        assert_ne!(Evidence::TotalGreen, Evidence::LineConserve);
        assert_ne!(Evidence::RiskClear, Evidence::HandoverDouble);
        for (i, e) in EVIDENCE_ALL.iter().enumerate() {
            assert_eq!(e.slot(), i);
        }
    }

    #[test]
    fn mark_gates() {
        let mut m = ClaimMachine::new();
        assert_eq!(m.mark(Evidence::TotalGreen, 7, 1), Err(Diag::EWitness.val()));
        assert_eq!(m.mark(Evidence::TotalGreen, WITNESS_GREEN, 0), Err(Diag::EForged.val()));
        assert_eq!(m.mark(Evidence::TotalGreen, WITNESS_GREEN, 5), Ok(()));
        assert_eq!(m.mark(Evidence::TotalGreen, WITNESS_GREEN, 6), Err(Diag::EDupMark.val()));
        assert_eq!(m.marked(), 1);
    }

    #[test]
    fn replay_gated() {
        let mut m = ClaimMachine::new();
        assert_eq!(m.mark(Evidence::TriSource, WITNESS_TRI, 999), Ok(()));
        assert_eq!(m.mark(Evidence::LineConserve, WITNESS_LINES, 999), Err(Diag::EReplay.val()));
    }

    #[test]
    fn missing_ten_of_ten() {
        assert_eq!(missing_matrix(), [true; MISSING_PROBES]);
    }

    #[test]
    fn declare_requires_all_five() {
        let mut m = ClaimMachine::new();
        assert_eq!(m.declare(), Err(Diag::EMissing.val()));
        // 补四证仍拒。
        for e in EVIDENCE_ALL.iter().take(EVIDENCE_COUNT - 1) {
            let _ = m.mark(*e, e.witness(), evidence_digest(*e, e.witness()));
        }
        assert_eq!(m.missing(), vec![4usize], "只缺第五证槽");
        assert_eq!(m.declare(), Err(Diag::EMissing.val()));
        let _ = m.mark(Evidence::RiskClear, WITNESS_ZERO_HANG, evidence_digest(Evidence::RiskClear, WITNESS_ZERO_HANG));
        assert!(m.declare().is_ok());
        assert_eq!(m.phase(), ClaimPhase::Declared);
    }

    #[test]
    fn settle_and_declare_monotonic() {
        let mut m = build_machine();
        assert_eq!(m.phase(), ClaimPhase::Gathering);
        assert_eq!(m.settle(), Ok(()));
        assert_eq!(m.settle(), Err(Diag::EPhase.val()), "齐备后重收束拒");
        assert!(m.declare().is_ok());
        assert_eq!(m.declare(), Err(Diag::EPhase.val()));
        assert_eq!(m.mark(Evidence::TotalGreen, WITNESS_GREEN, 1), Err(Diag::ESealed.val()));
        assert_eq!(m.phase().ord(), 2);
    }

    #[test]
    fn empty_settle_rejected() {
        let mut m = ClaimMachine::new();
        assert_eq!(m.settle(), Err(Diag::EEmpty.val()));
        assert_eq!(m.phase(), ClaimPhase::Gathering);
    }

    #[test]
    fn record_only_after_declare() {
        let m = build_machine();
        assert!(m.record().is_empty(), "未宣告记录必须为空");
        let mut m2 = build_machine();
        let _ = m2.settle();
        assert!(m2.record().is_empty());
        let _ = m2.declare();
        let rec = m2.record();
        assert!(rec.contains("收官宣告"));
        assert!(rec.contains("总回归全绿"));
    }

    #[test]
    fn witness_values_frozen() {
        assert_eq!(Evidence::TotalGreen.witness(), 800);
        assert_eq!(Evidence::LineConserve.witness(), 240_000);
        assert_eq!(Evidence::TriSource.witness(), 3);
        assert_eq!(Evidence::HandoverDouble.witness(), 2);
        assert_eq!(Evidence::RiskClear.witness(), 0);
    }

    #[test]
    fn evidence_digest_stable_and_distinct() {
        let a = evidence_digest(Evidence::TotalGreen, WITNESS_GREEN);
        let b = evidence_digest(Evidence::TotalGreen, WITNESS_GREEN);
        let c = evidence_digest(Evidence::LineConserve, WITNESS_LINES);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, 0);
    }

    #[test]
    fn dual_run_consistency() {
        assert!(dual_run_ok());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx1594_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
