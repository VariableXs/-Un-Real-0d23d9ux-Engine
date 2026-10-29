//! UNX-F48705 —— I·AI-86 无红线违例证明（年度例行签字位）
//! 判据原文：UNX-F48705-J1 证明在册；判据锚定 §AI-61 §9。
//! 判据落地（drill 场景）：R1 三线证明在册；R2 零违例断言；
//! R3 签字位就绪；R4 违例注入检出；R5 证明指纹；R6 重复登记拒；
//! R7 查询；R8 呈现行；R9 年度口径；R10 证明闭包。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：红线三线 = 引导 / 硬件 / 行为；无违例证明 = 三线
//! 各一份年度签字证明，violation 恒 0；违例注入一律检出即红。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0F80;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_LINE: u16 = SEG_BASE + 1; // 红线越出三线闭集
const E_VIOL: u16 = SEG_BASE + 2; // 违例未检出
const E_SIGN: u16 = SEG_BASE + 3; // 签字位缺失
const E_DUP: u16 = SEG_BASE + 4; // 重复登记
const E_NOTFOUND: u16 = SEG_BASE + 5; // 查无证明
const E_FP: u16 = SEG_BASE + 6; // 指纹漂移
const E_LINE2: u16 = SEG_BASE + 7; // 呈现行缺段
const E_COUNT: u16 = SEG_BASE + 8; // 证明数不足 3

/// 红线三线闭集。
pub const L_BOOT: u8 = 1; // 引导线
pub const L_HW: u8 = 2; // 硬件线
pub const L_BEHAV: u8 = 3; // 行为线
/// 证明数。
pub const PROOF_N: usize = 3;
/// 年度口径（冻结）。
pub const YEAR: u32 = 2_026;

/// FNV-1a 64 位。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 红线闭集判定。
pub fn line_ok(l: u8) -> bool {
    l == L_BOOT || l == L_HW || l == L_BEHAV
}

/// 一份无违例证明。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NoViolProof {
    pub line: u8,
    pub violations: u32, // 恒 0
    pub signed: bool,    // 签字位
    pub year: u32,       // 年度口径
    pub sig: u64,        // 签字摘要
}

impl NoViolProof {
    /// 证明合法：三线闭集 + 零违例 + 已签 + 年度冻结 + 摘要一致。
    pub fn sane(&self) -> Result<(), u16> {
        if !line_ok(self.line) {
            return Err(E_LINE);
        }
        if self.violations != 0 {
            return Err(E_VIOL);
        }
        if !self.signed {
            return Err(E_SIGN);
        }
        if self.year != YEAR {
            return Err(E_LINE);
        }
        let want = NoViolProof::make_sig(self.line, self.year);
        if self.sig != want {
            return Err(E_SIGN);
        }
        Ok(())
    }

    /// 签字摘要：线 + 年度混签。
    pub fn make_sig(line: u8, year: u32) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.push(line);
        buf.extend_from_slice(&year.to_le_bytes());
        fnv1a64(&buf)
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.push(self.line);
        buf.extend_from_slice(&self.violations.to_le_bytes());
        buf.push(self.signed as u8);
        buf.extend_from_slice(&self.year.to_le_bytes());
        fnv1a64(&buf)
    }
}

/// 证明账。
pub struct ProofLedger {
    pub items: Vec<NoViolProof>,
}

impl ProofLedger {
    pub fn new() -> ProofLedger {
        ProofLedger { items: vec![] }
    }

    /// 登记。
    pub fn register(&mut self, p: NoViolProof) -> Result<(), u16> {
        p.sane()?;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].line == p.line {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.items.push(p);
        Ok(())
    }

    /// 查询。
    pub fn lookup(&self, line: u8) -> Result<&NoViolProof, u16> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].line == line {
                return Ok(&self.items[i]);
            }
            i += 1;
        }
        Err(E_NOTFOUND)
    }

    /// 三线齐。
    pub fn complete(&self) -> Result<(), u16> {
        if self.items.len() != PROOF_N {
            return Err(E_COUNT);
        }
        let mut l = 1u8;
        while l <= 3 {
            self.lookup(l)?;
            l += 1;
        }
        Ok(())
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.items.len() {
            buf.extend_from_slice(&self.items[i].fp().to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 构造三线证明账。
pub fn build_proofs() -> Result<ProofLedger, u16> {
    let mut led = ProofLedger::new();
    let mut l = 1u8;
    while l <= 3 {
        led.register(NoViolProof {
            line: l,
            violations: 0,
            signed: true,
            year: YEAR,
            sig: NoViolProof::make_sig(l, YEAR),
        })?;
        l += 1;
    }
    Ok(led)
}

/// 线名（呈现辅助）。
pub fn line_name(l: u8) -> &'static str {
    if l == L_BOOT {
        "boot"
    } else if l == L_HW {
        "hardware"
    } else {
        "behavior"
    }
}

/// 呈现行：proofs=<n> viol=<总违例> signed=<已签数>。
pub fn ledger_line(led: &ProofLedger) -> String {
    let mut viol: u32 = 0;
    let mut signed: u32 = 0;
    let mut i = 0usize;
    while i < led.items.len() {
        viol += led.items[i].violations;
        if led.items[i].signed {
            signed += 1;
        }
        i += 1;
    }
    let mut out = String::new();
    out.push_str("proofs=");
    out.push_str(&led.items.len().to_string());
    out.push_str(" viol=");
    out.push_str(&viol.to_string());
    out.push_str(" signed=");
    out.push_str(&signed.to_string());
    out
}

/// 子串判定（朴素扫描）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x487_05);

    // R1 三线证明在册。
    let led = build_proofs()?;
    led.complete()?;
    hits += 1;

    // R2 零违例断言：逐条 violations==0。
    let mut i = 0usize;
    while i < led.items.len() {
        if led.items[i].violations != 0 {
            return Err(E_VIOL);
        }
        i += 1;
    }
    hits += 1;

    // R3 签字位就绪：逐条 signed 且签名可复算。
    let mut i = 0usize;
    while i < led.items.len() {
        led.items[i].sane()?;
        i += 1;
    }
    hits += 1;

    // R4 违例注入检出：violations=2 红；未签红；线越界红。
    let bad1 = NoViolProof {
        line: L_BOOT,
        violations: 2,
        signed: true,
        year: YEAR,
        sig: NoViolProof::make_sig(L_BOOT, YEAR),
    };
    match bad1.sane() {
        Err(E_VIOL) => {}
        _ => return Err(E_VIOL),
    }
    let bad2 = NoViolProof {
        line: L_HW,
        violations: 0,
        signed: false,
        year: YEAR,
        sig: NoViolProof::make_sig(L_HW, YEAR),
    };
    match bad2.sane() {
        Err(E_SIGN) => {}
        _ => return Err(E_SIGN),
    }
    let bad3 = NoViolProof {
        line: 9,
        violations: 0,
        signed: true,
        year: YEAR,
        sig: NoViolProof::make_sig(9, YEAR),
    };
    match bad3.sane() {
        Err(E_LINE) => hits += 1,
        _ => return Err(E_LINE),
    }

    // R5 证明指纹稳定。
    let f1 = led.fp();
    let f2 = led.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R6 重复登记拒。
    let mut led2 = build_proofs()?;
    match led2.register(NoViolProof {
        line: L_BEHAV,
        violations: 0,
        signed: true,
        year: YEAR,
        sig: NoViolProof::make_sig(L_BEHAV, YEAR),
    }) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R7 查询：在册可查、查无红。
    led2.lookup(L_BOOT)?;
    match led2.lookup(7) {
        Err(E_NOTFOUND) => hits += 1,
        _ => return Err(E_NOTFOUND),
    }

    // R8 呈现行：proofs=3 viol=0 signed=3。
    let line = ledger_line(&led);
    if !has_kw(line.as_bytes(), b"proofs=3") || !has_kw(line.as_bytes(), b"viol=0")
        || !has_kw(line.as_bytes(), b"signed=3")
    {
        return Err(E_LINE2);
    }
    hits += 1;

    // R9 年度口径：冻结 2026；线名表冻结。
    if YEAR != 2_026 {
        return Err(E_LINE);
    }
    if line_name(L_BOOT) != "boot" || line_name(L_BEHAV) != "behavior" {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 证明闭包：年度漂移检出（签名对年敏感）。
    let drift = NoViolProof {
        line: L_BOOT,
        violations: 0,
        signed: true,
        year: 2_027,
        sig: NoViolProof::make_sig(L_BOOT, YEAR),
    };
    match drift.sane() {
        Err(E_SIGN) => hits += 1,
        _ => return Err(E_SIGN),
    }
    let _ = g.below(10);

    Ok(hits)
}
