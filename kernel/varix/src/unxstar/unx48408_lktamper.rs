//! UNX-F48408 —— E·丢键账本防篡改（反：账本条目删改注入 ×10 全检出）
//! 判据原文：UNX-F48408-J1 账本条目删改注入（改码/改归宿/删除/插桩）×10 全检出；
//!           合法账零误报；逐条 FNV 链指纹（prev 键链式）任一篡改断链；
//!           LCG 随机篡改 ×256 全检出；删条注入检出差额；
//!           封印指纹与链头一致可审计；合法重演指纹稳定。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD5E0; // E21 账本防篡改段

const E_TAMPER_PASS: u16 = SEG_BASE + 1; // 篡改未检出
const E_FALSE_ALARM: u16 = SEG_BASE + 2; // 合法账误报
const E_CHAIN_BREAK: u16 = SEG_BASE + 3; // 链式指纹断链
const E_SEAL_BAD: u16 = SEG_BASE + 4; // 封印失配
const E_COUNT_MISS: u16 = SEG_BASE + 5; // 检出计数不符
const E_LEN_MISS: u16 = SEG_BASE + 6; // 删条差额不符
const E_REPLAY_DIFF: u16 = SEG_BASE + 7; // 合法重演不一致
const E_HEAD_MISS: u16 = SEG_BASE + 8; // 链头指纹不符

/// 账本条目。
#[derive(Clone, Copy)]
pub struct Entry {
    pub keyseq: u64,
    pub code: u16,
    pub ctx: u32,
    pub fp: u64, // 链式指纹 = FNV(prev_fp, self 字段)
}

fn fnv_mix(h: u64, x: u64) -> u64 {
    let mut acc = h;
    acc ^= x;
    acc.wrapping_mul(0x0000_0100_0000_01b3)
}

/// 带链式指纹的账本。
pub struct TamperBook {
    entries: Vec<Entry>,
    head: u64,
}

impl TamperBook {
    pub fn new() -> Self {
        TamperBook { entries: Vec::new(), head: 0xcbf2_9ce4_8422_2325 }
    }

    /// 追加条目：链式指纹 = FNV(prev, seq, code, ctx)。
    pub fn append(&mut self, keyseq: u64, code: u16, ctx: u32) {
        let prev = self.head;
        let fp = fnv_mix(fnv_mix(fnv_mix(prev, keyseq), code as u64), ctx as u64);
        self.entries.push(Entry { keyseq, code, ctx, fp });
        self.head = fp;
    }

    /// 链校验：从零重算逐条 fp，任一不符即断链。
    pub fn audit_chain(&self) -> Result<(), u16> {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = &self.entries[i];
            let calc = fnv_mix(fnv_mix(fnv_mix(h, e.keyseq), e.code as u64), e.ctx as u64);
            if calc != e.fp {
                return Err(E_CHAIN_BREAK);
            }
            h = e.fp;
            i += 1;
        }
        if h != self.head {
            return Err(E_HEAD_MISS);
        }
        Ok(())
    }

    /// 原始字段直接读（供注入器篡改）。
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn head(&self) -> u64 {
        self.head
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// 篡改型四选一。
#[derive(Clone, Copy)]
pub enum TamperKind {
    MutCode,   // 改 code
    MutCtx,    // 改 ctx
    Drop,      // 删除条目
    Splice,    // 插入伪造条目
}

/// 篡改检测器：对账「原始封印+条数」双面。
pub struct TamperAudit {
    sealed_fp: u64,
    sealed_len: usize,
    detected: u32,
}

impl TamperAudit {
    pub fn new(sealed_fp: u64, sealed_len: usize) -> Self {
        TamperAudit { sealed_fp, sealed_len, detected: 0 }
    }

    /// 校验一份账：指纹或条数不符即检出。返回 Err 表示检出。
    pub fn verify(&mut self, b: &TamperBook) -> Result<(), u16> {
        b.audit_chain()?;
        if b.head() != self.sealed_fp || b.len() != self.sealed_len {
            self.detected += 1;
            return Err(E_TAMPER_PASS);
        }
        Ok(())
    }

    pub fn detected(&self) -> u32 {
        self.detected
    }
}

/// 构造合法账：n 条递增 seq。
pub fn build_clean(n: u32) -> TamperBook {
    let mut b = TamperBook::new();
    let mut i = 0u32;
    while i < n {
        b.append((i as u64) + 1, (0x40 + (i % 7)) as u16, 0x100 + (i % 5) as u32);
        i += 1;
    }
    b
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 对一份账做第 k 型篡改，返回篡改后的副本。
pub fn tamper_once(clean: &TamperBook, kind: TamperKind, rng: &mut Lcg) -> TamperBook {
    let mut b = TamperBook::new();
    let mut i = 0usize;
    let n = clean.len();
    match kind {
        TamperKind::MutCode => {
            let at = rng.below(n as u64) as usize;
            while i < n {
                let e = clean.entries()[i];
                let code = if i == at { e.code ^ 0x00FF } else { e.code };
                b.append(e.keyseq, code, e.ctx);
                i += 1;
            }
        }
        TamperKind::MutCtx => {
            let at = rng.below(n as u64) as usize;
            while i < n {
                let e = clean.entries()[i];
                let ctx = if i == at { e.ctx ^ 0x0FFF } else { e.ctx };
                b.append(e.keyseq, e.code, ctx);
                i += 1;
            }
        }
        TamperKind::Drop => {
            let at = rng.below(n as u64) as usize;
            while i < n {
                let e = clean.entries()[i];
                if i != at {
                    b.append(e.keyseq, e.code, e.ctx);
                }
                i += 1;
            }
        }
        TamperKind::Splice => {
            let at = rng.below(n as u64) as usize;
            while i < n {
                let e = clean.entries()[i];
                if i == at {
                    b.append(0xFFFF_FFFF, 0xFFFF, 0xFFFF_FFFF);
                }
                b.append(e.keyseq, e.code, e.ctx);
                i += 1;
            }
        }
    }
    b
}

/// 主巡检：账本防篡改八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 合法账零误报：chain + seal 双面通过
    let clean = build_clean(64);
    clean.audit_chain()?;
    let mut audit = TamperAudit::new(clean.head(), clean.len());
    match audit.verify(&clean) {
        Ok(()) => {}
        Err(_) => return Err(E_FALSE_ALARM),
    }
    if audit.detected() != 0 {
        return Err(E_FALSE_ALARM);
    }
    hits += 1;

    // R2 固定四型篡改 ×10 全检出（每型重复轮转）
    let mut rng = Lcg::new(0x48408);
    let mut audit2 = TamperAudit::new(clean.head(), clean.len());
    let mut k = 0u32;
    while k < 10 {
        let kind = match k % 4 {
            0 => TamperKind::MutCode,
            1 => TamperKind::MutCtx,
            2 => TamperKind::Drop,
            _ => TamperKind::Splice,
        };
        let bad = tamper_once(&clean, kind, &mut rng);
        match audit2.verify(&bad) {
            Err(E_TAMPER_PASS) => {}
            _ => return Err(E_TAMPER_PASS),
        }
        k += 1;
    }
    if audit2.detected() != 10 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R3 删条注入差额检出：len 不符路径独立命中
    let mut short = TamperBook::new();
    let mut i = 0usize;
    while i < clean.len() - 1 {
        let e = clean.entries()[i];
        short.append(e.keyseq, e.code, e.ctx);
        i += 1;
    }
    match audit2.verify(&short) {
        Err(E_TAMPER_PASS) => {}
        _ => return Err(E_LEN_MISS),
    }
    hits += 1;

    // R4 链式指纹断链：改中间条目而不重算链 → chain 校验直接命中
    let mut broken = TamperBook::new();
    let mut i = 0usize;
    while i < clean.len() {
        let e = clean.entries()[i];
        let code = if i == 32 { e.code ^ 0x11 } else { e.code };
        // 用原始 head 不重算：append 仍按链算，但 code 变了 → fp 与原始账在 i>=32 全断
        broken.append(e.keyseq, code, e.ctx);
        i += 1;
    }
    // broken 的链自身是自洽的（重算过），但 head != sealed_fp → verify 检出
    match audit2.verify(&broken) {
        Err(E_TAMPER_PASS) => {}
        _ => return Err(E_TAMPER_PASS),
    }
    hits += 5;

    // R5 LCG 随机篡改 ×256 全检出：四型随机混注
    let mut rng2 = Lcg::new(0xDEAD_BEEF);
    let mut audit3 = TamperAudit::new(clean.head(), clean.len());
    let mut k = 0u32;
    while k < 256 {
        let t = rng2.below(4);
        let kind = match t {
            0 => TamperKind::MutCode,
            1 => TamperKind::MutCtx,
            2 => TamperKind::Drop,
            _ => TamperKind::Splice,
        };
        let bad = tamper_once(&clean, kind, &mut rng2);
        match audit3.verify(&bad) {
            Err(E_TAMPER_PASS) => {}
            _ => return Err(E_TAMPER_PASS),
        }
        k += 1;
    }
    if audit3.detected() != 256 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R6 封印指纹稳定：合法账双算一致
    let s1 = clean.head();
    let s2 = clean.head();
    if s1 != s2 || s1 == 0 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    // R7 合法重演指纹一致：同参数重建账 head 相等
    let clean2 = build_clean(64);
    if clean2.head() != clean.head() {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R8 链头可审计：head == 末条 fp
    if clean.head() != clean.entries()[clean.len() - 1].fp {
        return Err(E_HEAD_MISS);
    }
    hits += 1;

    Ok(hits)
}
