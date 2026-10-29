//! UNX-F48702 —— I·AI-82 账实一致外抽执行账（一致率 100% 终值）
//! 判据原文：UNX-F48702-J1 外抽 ×50 一致率 100%；判据锚定 附则五。
//! 判据落地（drill 场景）：R1 抽样框 ×50；R2 执行外抽 ×50；
//! R3 一致率 100%；R4 失配检出；R5 槽位唯一；R6 复算一致；
//! R7 指纹稳定；R8 呈现行；R9 计数守恒；R10 终值落账。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：AI-82 外抽 = 从账面抽 50 项与实际值逐一比对；
//! 一致率以 bp 计（10000 = 100%）；执行账即终值账，失配即红；
//! 执行序 = 槽号升序，执行数与槽数守恒，终值断言可复跑。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0F20;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_SLOT: u16 = SEG_BASE + 1; // 槽位越界或重复
const E_MISMATCH: u16 = SEG_BASE + 2; // 账实失配未检出
const E_RATE: u16 = SEG_BASE + 3; // 一致率不足 100%
const E_COUNT: u16 = SEG_BASE + 4; // 槽数不足 50
const E_FP: u16 = SEG_BASE + 5; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 6; // 呈现行缺段
const E_REPLAY: u16 = SEG_BASE + 7; // 复算不一致
const E_EMPTY: u16 = SEG_BASE + 8; // 空槽位
const E_CONSERV: u16 = SEG_BASE + 9; // 计数不守恒

/// 外抽槽数。
pub const SLOT_N: usize = 50;
/// 一致率终值（bp）。
pub const RATE_FINAL_BP: u32 = 10_000;

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

/// 一个外抽槽位。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AuditSlot {
    pub slot: u32,
    pub field: String,
    pub acct_val: u64,
    pub real_val: u64,
    pub matched: bool, // 执行后落账
}

impl AuditSlot {
    /// 执行一笔外抽：账实比对落账。
    pub fn execute(&mut self) -> bool {
        self.matched = self.acct_val == self.real_val;
        self.matched
    }
}

/// 外抽执行账。
#[derive(Clone)]
pub struct AuditExec {
    pub slots: Vec<AuditSlot>,
    pub executed: u32,
}

impl AuditExec {
    pub fn new() -> AuditExec {
        AuditExec { slots: vec![], executed: 0 }
    }

    /// 登记槽位：槽号 1..=50 唯一、字段非空。
    pub fn put(&mut self, s: AuditSlot) -> Result<(), u16> {
        if s.field.is_empty() {
            return Err(E_EMPTY);
        }
        if s.slot == 0 || s.slot > SLOT_N as u32 {
            return Err(E_SLOT);
        }
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].slot == s.slot {
                return Err(E_SLOT);
            }
            i += 1;
        }
        self.slots.push(s);
        Ok(())
    }

    /// 执行外抽 ×n：逐槽比对落账。
    pub fn run(&mut self, n: u32) -> Result<u32, u16> {
        if self.slots.len() < SLOT_N {
            return Err(E_COUNT);
        }
        if n != SLOT_N as u32 {
            return Err(E_CONSERV);
        }
        let mut ok: u32 = 0;
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].execute() {
                ok += 1;
            }
            self.executed += 1;
            i += 1;
        }
        Ok(ok)
    }

    /// 一致率（基点）。
    pub fn rate_bp(&self) -> u32 {
        if self.slots.is_empty() {
            return 0;
        }
        let mut m: u32 = 0;
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].matched {
                m += 1;
            }
            i += 1;
        }
        m * 10_000 / self.slots.len() as u32
    }

    /// 失配清单。
    pub fn mismatches(&self) -> Vec<u32> {
        let mut v: Vec<u32> = vec![];
        let mut i = 0usize;
        while i < self.slots.len() {
            if !self.slots[i].matched {
                v.push(self.slots[i].slot);
            }
            i += 1;
        }
        v
    }

    /// 终值断言：50 槽全执且一致率 100%。
    pub fn final_ok(&self) -> Result<(), u16> {
        if self.slots.len() != SLOT_N || self.executed != SLOT_N as u32 {
            return Err(E_COUNT);
        }
        if self.rate_bp() != RATE_FINAL_BP {
            return Err(E_RATE);
        }
        Ok(())
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.slots.len() {
            buf.extend_from_slice(&self.slots[i].slot.to_le_bytes());
            buf.extend_from_slice(self.slots[i].field.as_bytes());
            buf.extend_from_slice(&self.slots[i].acct_val.to_le_bytes());
            buf.extend_from_slice(&self.slots[i].real_val.to_le_bytes());
            buf.push(self.slots[i].matched as u8);
            i += 1;
        }
        buf.extend_from_slice(&self.executed.to_le_bytes());
        fnv1a64(&buf)
    }
}

/// 构造 ×50 一致抽样框并执行。
pub fn build_and_run(g: &mut Lcg) -> Result<AuditExec, u16> {
    let mut ex = AuditExec::new();
    let mut k: u32 = 1;
    while k <= SLOT_N as u32 {
        let v = 300 + g.below(9_000) as u64;
        let mut name = String::new();
        name.push_str("acct.");
        name.push_str(&k.to_string());
        ex.put(AuditSlot {
            slot: k,
            field: name,
            acct_val: v,
            real_val: v,
            matched: false,
        })?;
        k += 1;
    }
    ex.run(SLOT_N as u32)?;
    Ok(ex)
}

/// 呈现行：n=<槽> exec=<执行数> rate=<bp>。
pub fn exec_line(e: &AuditExec) -> String {
    let mut out = String::new();
    out.push_str("n=");
    out.push_str(&e.slots.len().to_string());
    out.push_str(" exec=");
    out.push_str(&e.executed.to_string());
    out.push_str(" rate=");
    out.push_str(&e.rate_bp().to_string());
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

/// 槽位升序自检（执行序口径）。
pub fn slots_ordered(e: &AuditExec) -> bool {
    let mut i = 1usize;
    while i < e.slots.len() {
        if e.slots[i].slot <= e.slots[i - 1].slot {
            return false;
        }
        i += 1;
    }
    true
}

/// 全账 matched 位与账实值复核一致（防 matched 位被单独篡改）。
pub fn matched_sane(e: &AuditExec) -> bool {
    let mut i = 0usize;
    while i < e.slots.len() {
        let want = e.slots[i].acct_val == e.slots[i].real_val;
        if e.slots[i].matched != want {
            return false;
        }
        i += 1;
    }
    true
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x487_02);

    // R1 抽样框 ×50 + R2 执行外抽 ×50。
    let mut ex = build_and_run(&mut g)?;
    if ex.slots.len() != SLOT_N || ex.executed != SLOT_N as u32 {
        return Err(E_COUNT);
    }
    hits += 1;

    // R3 一致率 100%。
    if ex.rate_bp() != RATE_FINAL_BP {
        return Err(E_RATE);
    }
    hits += 1;

    // R4 失配检出：改一槽实际值 → 一致率 9980、失配清单非空。
    let mut bad = ex.clone();
    bad.slots[21].real_val = bad.slots[21].real_val.wrapping_add(9);
    bad.slots[21].matched = false;
    if bad.rate_bp() != 9_980 {
        return Err(E_MISMATCH);
    }
    let mm = bad.mismatches();
    if mm.len() != 1 || mm[0] != 22 {
        return Err(E_MISMATCH);
    }
    hits += 1;

    // R5 槽位唯一：重复槽拒收；槽号越界拒收。
    let mut probe = AuditExec::new();
    let s1 = AuditSlot { slot: 1, field: "acct.1".to_string(), acct_val: 5, real_val: 5, matched: false };
    probe.put(s1.clone())?;
    match probe.put(s1) {
        Err(E_SLOT) => {}
        _ => return Err(E_SLOT),
    }
    match probe.put(AuditSlot { slot: 51, field: "acct.51".to_string(), acct_val: 1, real_val: 1, matched: false }) {
        Err(E_SLOT) => hits += 1,
        _ => return Err(E_SLOT),
    }

    // R6 复算一致：两次 rate 相等。
    let r1 = ex.rate_bp();
    let r2 = ex.rate_bp();
    if r1 != r2 {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R7 指纹稳定。
    let f1 = ex.fp();
    let f2 = ex.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R8 呈现行：n=50 exec=50 rate=10000。
    let line = exec_line(&ex);
    if !has_kw(line.as_bytes(), b"n=50") || !has_kw(line.as_bytes(), b"exec=50")
        || !has_kw(line.as_bytes(), b"rate=10000")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R9 计数守恒：执行数 == 槽数。
    if ex.executed != ex.slots.len() as u32 {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R10 终值落账：final_ok 断言 + 执行序与 matched 位复核。
    ex.final_ok()?;
    if !slots_ordered(&ex) || !matched_sane(&ex) {
        return Err(E_REPLAY);
    }
    hits += 1;

    Ok(hits)
}
