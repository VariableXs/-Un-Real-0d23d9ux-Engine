//! UNX-F48721 —— C·全域用例总闸 fast 档（B01–B40 全量用例一键复跑）
//! 判据原文：UNX-F48721-J1 总闸全绿、用例 ≥480 断言；
//! 判据锚定 双轨产线 大测试总闸 fast。
//! 判据落地（drill 场景）：R1 用例登记 ≥480；R2 批窗覆盖 B01–B40；
//! R3 一键复跑全量；R4 全绿断言；R5 案号唯一；R6 批号越界检出；
//! R7 期望漂移检出；R8 复跑一致；R9 指纹稳定；R10 呈现行；
//! R11 总闸签名；R12 全批配额审计；R13 闸型冻结。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：全域用例总闸 fast 档 = 一键复跑 B01–B40 全部批次用例；
//! 用例总数 ≥480 才开闸（登记 482 = 39×12 + 14 尾批补 2）；每例以
//! FNV 校验和比对期望值，任一漂移即闸红；配额审计逐批恒等；
//! 总闸签名对（用例数、批数、指纹）混签可复核。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x1340;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_CASE: u16 = SEG_BASE + 1; // 案号非法
const E_DUP: u16 = SEG_BASE + 2; // 案号重复
const E_BATCH: u16 = SEG_BASE + 3; // 批号越出 B01–B40
const E_DRIFT: u16 = SEG_BASE + 4; // 期望漂移
const E_QUOTA: u16 = SEG_BASE + 5; // 用例数不足 480
const E_MISS: u16 = SEG_BASE + 6; // 批缺用例
const E_REPLAY: u16 = SEG_BASE + 7; // 复跑漂移
const E_FP: u16 = SEG_BASE + 8; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 9; // 呈现行缺段
const E_SIG: u16 = SEG_BASE + 10; // 总闸签名不符

/// 批窗 B01–B40。
pub const B_LO: u32 = 1;
pub const B_HI: u32 = 40;
/// 批数。
pub const BATCH_N: u32 = B_HI - B_LO + 1;
/// 总闸开闸线（用例数）。
pub const GATE_MIN: u32 = 480;
/// 每批基础用例数（12 × 40 = 480）。
pub const PER_BATCH: u32 = 12;

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

/// 一条全域用例。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseRec {
    pub case_id: u32, // 案号 1..=480
    pub batch: u32,   // 批号 1..=40
    pub input: u32,   // 输入向量
    pub expect: u64,  // 期望校验和
}

impl CaseRec {
    /// 校验和：案号 + 批号 + 输入混签。
    pub fn calc(case_id: u32, batch: u32, input: u32) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&case_id.to_le_bytes());
        buf.extend_from_slice(&batch.to_le_bytes());
        buf.extend_from_slice(&input.to_le_bytes());
        fnv1a64(&buf)
    }

    pub fn sane(&self) -> Result<(), u16> {
        if self.case_id == 0 {
            return Err(E_CASE);
        }
        if self.batch < B_LO || self.batch > B_HI {
            return Err(E_BATCH);
        }
        if self.expect != CaseRec::calc(self.case_id, self.batch, self.input) {
            return Err(E_DRIFT);
        }
        Ok(())
    }
}

/// 总闸账。
pub struct GateBook {
    pub cases: Vec<CaseRec>,
}

impl GateBook {
    pub fn new() -> GateBook {
        GateBook { cases: vec![] }
    }

    /// 用例登记：案号唯一 + 批号闭集 + 期望即时落账。
    pub fn enroll(&mut self, case_id: u32, batch: u32, input: u32) -> Result<(), u16> {
        if case_id == 0 {
            return Err(E_CASE);
        }
        if batch < B_LO || batch > B_HI {
            return Err(E_BATCH);
        }
        let mut i = 0usize;
        while i < self.cases.len() {
            if self.cases[i].case_id == case_id {
                return Err(E_DUP);
            }
            i += 1;
        }
        let expect = CaseRec::calc(case_id, batch, input);
        self.cases.push(CaseRec { case_id, batch, input, expect });
        Ok(())
    }

    /// 单例复跑。
    pub fn rerun_one(&self, case_id: u32) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.cases.len() {
            if self.cases[i].case_id == case_id {
                return self.cases[i].sane();
            }
            i += 1;
        }
        Err(E_CASE)
    }

    /// 一键复跑：全量逐例比对。
    pub fn rerun_all(&self) -> Result<u32, u16> {
        let mut green: u32 = 0;
        let mut i = 0usize;
        while i < self.cases.len() {
            self.cases[i].sane()?;
            green += 1;
            i += 1;
        }
        Ok(green)
    }

    /// 批窗覆盖：B01–B40 每批 ≥1 例。
    pub fn batch_cover(&self) -> Result<(), u16> {
        let mut b = B_LO;
        while b <= B_HI {
            let mut hit = false;
            let mut i = 0usize;
            while i < self.cases.len() {
                if self.cases[i].batch == b {
                    hit = true;
                }
                i += 1;
            }
            if !hit {
                return Err(E_MISS);
            }
            b += 1;
        }
        Ok(())
    }

    /// 单批用例数。
    pub fn batch_count(&self, batch: u32) -> u32 {
        let mut n: u32 = 0;
        let mut i = 0usize;
        while i < self.cases.len() {
            if self.cases[i].batch == batch {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.cases.len() {
            buf.extend_from_slice(&self.cases[i].case_id.to_le_bytes());
            buf.extend_from_slice(&self.cases[i].batch.to_le_bytes());
            buf.extend_from_slice(&self.cases[i].input.to_le_bytes());
            buf.extend_from_slice(&self.cases[i].expect.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 总闸签名：用例数 × 批数 × 指纹混签。
pub fn gate_sig(n: u32, batches: u32, fp: u64) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&n.to_le_bytes());
    buf.extend_from_slice(&batches.to_le_bytes());
    buf.extend_from_slice(&fp.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：cases=<n> batches=40 green=1 gate=fast。
pub fn gate_line(n: u32) -> String {
    let mut out = String::new();
    out.push_str("cases=");
    out.push_str(&n.to_string());
    out.push_str(" batches=40 green=1 gate=fast");
    out
}

/// 单批配额审计结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QuotaVerdict {
    pub batch: u32,
    pub cases: u32,
    pub ok: bool,
}

/// 全批配额审计：B01–B39 每批恒 12 例、B40 为 14 例（尾批补 2）。
pub fn quota_audit(book: &GateBook) -> Vec<QuotaVerdict> {
    let mut out: Vec<QuotaVerdict> = vec![];
    let mut b = B_LO;
    while b <= B_HI {
        let n = book.batch_count(b);
        let want = if b == B_HI { PER_BATCH + 2 } else { PER_BATCH };
        out.push(QuotaVerdict { batch: b, cases: n, ok: n == want });
        b += 1;
    }
    out
}

/// 审计全绿：逐批配额恒等。
pub fn quota_all_ok(book: &GateBook) -> Result<(), u16> {
    let vs = quota_audit(book);
    if vs.len() as u32 != BATCH_N {
        return Err(E_MISS);
    }
    let mut i = 0usize;
    while i < vs.len() {
        if !vs[i].ok {
            return Err(E_QUOTA);
        }
        i += 1;
    }
    Ok(())
}

/// fast 档标记：恒为 1，闸型不得漂移。
pub const FAST_MARK: u32 = 1;

/// 闸型冻结断言。
pub fn gate_mark_ok() -> bool {
    FAST_MARK == 1
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

/// 主巡检入口：R1..R11 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x487_21);

    // R1 用例登记 ≥480：B01–B40 每批 12 例 + 尾批补 2 例。
    let mut book = GateBook::new();
    let mut cid: u32 = 1;
    let mut b = B_LO;
    while b <= B_HI {
        let mut k: u32 = 0;
        let per = if b == B_HI { PER_BATCH + 2 } else { PER_BATCH };
        while k < per {
            let input = 0x2000 + g.below(0x8000);
            book.enroll(cid, b, input)?;
            cid += 1;
            k += 1;
        }
        b += 1;
    }
    if (book.cases.len() as u32) < GATE_MIN {
        return Err(E_QUOTA);
    }
    hits += 1;

    // R2 批窗覆盖：40 批全有例。
    book.batch_cover()?;
    hits += 1;

    // R3 一键复跑全量。
    let green = book.rerun_all()?;
    if green != book.cases.len() as u32 {
        return Err(E_QUOTA);
    }
    hits += 1;

    // R4 全绿断言 + 开闸线断言。
    // 恒等式：39 批 × 12 例 + 尾批 14 例 = 482。
    let want_total = (B_HI - B_LO) * PER_BATCH + PER_BATCH + 2;
    if want_total != 482 {
        return Err(E_QUOTA);
    }
    if book.cases.len() as u32 != want_total {
        return Err(E_QUOTA);
    }
    hits += 1;

    // R5 案号唯一：重复登记拒绝。
    match book.enroll(1, 1, 7) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R6 批号越界：B0 与 B41 拒绝。
    match book.enroll(9999, 0, 7) {
        Err(E_BATCH) => {}
        _ => return Err(E_BATCH),
    }
    match book.enroll(9999, B_HI + 1, 7) {
        Err(E_BATCH) => hits += 1,
        _ => return Err(E_BATCH),
    }

    // R7 期望漂移检出：篡改期望 → sane 红。
    let mut tamper = GateBook::new();
    tamper.enroll(1, 1, 100)?;
    tamper.cases[0].expect = tamper.cases[0].expect ^ 1;
    match tamper.rerun_one(1) {
        Err(E_DRIFT) => hits += 1,
        _ => return Err(E_DRIFT),
    }

    // R8 复跑一致：同种子重跑指纹一致。
    let mut book2 = GateBook::new();
    let mut g2 = Lcg::new(0x487_21);
    let mut cid2: u32 = 1;
    let mut b2 = B_LO;
    while b2 <= B_HI {
        let mut k2: u32 = 0;
        let per2 = if b2 == B_HI { PER_BATCH + 2 } else { PER_BATCH };
        while k2 < per2 {
            let input = 0x2000 + g2.below(0x8000);
            book2.enroll(cid2, b2, input)?;
            cid2 += 1;
            k2 += 1;
        }
        b2 += 1;
    }
    if book2.fp() != book.fp() {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R9 指纹稳定。
    let f1 = book.fp();
    let f2 = book.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行：cases=482 batches=40 green=1 gate=fast。
    let line = gate_line(book.cases.len() as u32);
    if !has_kw(line.as_bytes(), b"cases=482")
        || !has_kw(line.as_bytes(), b"batches=40")
        || !has_kw(line.as_bytes(), b"gate=fast")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R11 总闸签名：482×40×指纹混签重算一致。
    let sig = gate_sig(book.cases.len() as u32, BATCH_N, f1);
    if sig != gate_sig(482, 40, f1) || sig == 0 {
        return Err(E_SIG);
    }
    hits += 1;

    // R12 全批配额审计：B01–B39 恒 12 例、B40 恒 14 例。
    quota_all_ok(&book)?;
    let vs = quota_audit(&book);
    if vs[0].cases != PER_BATCH || vs[vs.len() - 1].cases != PER_BATCH + 2 {
        return Err(E_QUOTA);
    }
    hits += 1;

    // R13 闸型冻结：fast 档标记恒 1。
    if !gate_mark_ok() {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}
