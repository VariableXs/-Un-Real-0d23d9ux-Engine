//! UNX-F48704 —— I·AI-98 提前放行终审回执（B01–B02 放行面闭环）
//! 判据原文：UNX-F48704-J1 回执零缺件；判据锚定 附则二。
//! 判据落地（drill 场景）：R1 回执构造；R2 必备件齐；R3 缺件检出；
//! R4 回执码域；R5 闭环签认；R6 重复回执拒；R7 查询；R8 指纹稳定；
//! R9 呈现行；R10 放行面闭包。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：AI-98 提前放行终审回执覆盖 B01（审计流）与 B02
//! （签名面）两放行位；回执必备件 = 放行位签认 + 审计摘要 +
//! 闭包时间戳；缺任一件即回执不闭环。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0F60;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_ITEM: u16 = SEG_BASE + 1; // 必备件缺失
const E_GATE: u16 = SEG_BASE + 2; // 放行位越出闭集
const E_CODE: u16 = SEG_BASE + 3; // 回执码非法
const E_DUP: u16 = SEG_BASE + 4; // 重复回执
const E_NOTFOUND: u16 = SEG_BASE + 5; // 查无回执
const E_CLOSE: u16 = SEG_BASE + 6; // 闭环未成立
const E_FP: u16 = SEG_BASE + 7; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 8; // 呈现行缺段
const E_CONSERV: u16 = SEG_BASE + 9; // 计数不守恒

/// 放行位闭集。
pub const G_B01: u8 = 1; // 审计流放行位
pub const G_B02: u8 = 2; // 签名面放行位
/// 必备件数。
pub const ITEM_N: usize = 3;

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

/// 放行位闭集判定。
pub fn gate_ok(g: u8) -> bool {
    g == G_B01 || g == G_B02
}

/// 一份终审回执。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Receipt {
    pub gate: u8,
    pub code: u16,       // 回执码（段窗内）
    pub signed: bool,    // 放行位签认
    pub audit_digest: u64, // 审计摘要
    pub closed_at: u32,  // 闭环时间戳
}

impl Receipt {
    /// 必备件齐：签认 + 摘要非零 + 时间戳非零 + 码在段窗。
    pub fn items_complete(&self) -> Result<(), u16> {
        if !gate_ok(self.gate) {
            return Err(E_GATE);
        }
        if self.code == 0 || self.code <= SEG_BASE || self.code > SEG_BASE + 0x1F {
            return Err(E_CODE);
        }
        if !self.signed {
            return Err(E_ITEM);
        }
        if self.audit_digest == 0 {
            return Err(E_ITEM);
        }
        if self.closed_at == 0 {
            return Err(E_ITEM);
        }
        Ok(())
    }

    /// 闭环成立 = 必备件齐。
    pub fn closed(&self) -> Result<(), u16> {
        self.items_complete()?;
        Ok(())
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.push(self.gate);
        buf.extend_from_slice(&self.code.to_le_bytes());
        buf.push(self.signed as u8);
        buf.extend_from_slice(&self.audit_digest.to_le_bytes());
        buf.extend_from_slice(&self.closed_at.to_le_bytes());
        fnv1a64(&buf)
    }
}

/// 回执账。
pub struct ReceiptLedger {
    pub items: Vec<Receipt>,
}

impl ReceiptLedger {
    pub fn new() -> ReceiptLedger {
        ReceiptLedger { items: vec![] }
    }

    /// 登记：闭环校验 + 放行位唯一（每放行位一份回执）。
    pub fn register(&mut self, r: Receipt) -> Result<(), u16> {
        r.closed()?;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].gate == r.gate {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.items.push(r);
        Ok(())
    }

    /// 查询。
    pub fn lookup(&self, gate: u8) -> Result<&Receipt, u16> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].gate == gate {
                return Ok(&self.items[i]);
            }
            i += 1;
        }
        Err(E_NOTFOUND)
    }

    /// 放行面闭包：B01/B02 两回执齐且闭环。
    pub fn closure(&self) -> Result<(), u16> {
        if self.items.len() != 2 {
            return Err(E_CONSERV);
        }
        self.lookup(G_B01)?.closed()?;
        self.lookup(G_B02)?.closed()?;
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

/// 构造 ×1 闭环回执账。
pub fn build_closure(g: &mut Lcg) -> Result<ReceiptLedger, u16> {
    let mut led = ReceiptLedger::new();
    let mut gt = 1u8;
    while gt <= 2 {
        let d = (g.next_u32() as u64) << 32 | g.next_u32() as u64;
        led.register(Receipt {
            gate: gt,
            code: SEG_BASE + 0x10 + gt as u16,
            signed: true,
            audit_digest: d,
            closed_at: 0x0F60 + gt as u32,
        })?;
        gt += 1;
    }
    Ok(led)
}

/// 呈现行：gates=<n> closed=<0|1>。
pub fn ledger_line(l: &ReceiptLedger) -> String {
    let closed = match l.closure() {
        Ok(()) => 1,
        Err(_) => 0,
    };
    let mut out = String::new();
    out.push_str("gates=");
    out.push_str(&l.items.len().to_string());
    out.push_str(" closed=");
    out.push_str(&closed.to_string());
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
    let mut g = Lcg::new(0x487_04);

    // R1 回执构造：闭环回执账生成。
    let led = build_closure(&mut g)?;
    if led.items.len() != 2 {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R2 必备件齐：两回执逐件检。
    led.lookup(G_B01)?.items_complete()?;
    led.lookup(G_B02)?.items_complete()?;
    if led.items.len() != ITEM_N - 1 {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R3 缺件检出：未签认 / 零摘要 / 零时间戳各红一次。
    let base = Receipt {
        gate: G_B01,
        code: SEG_BASE + 0x10,
        signed: true,
        audit_digest: 42,
        closed_at: 7,
    };
    let mut nosig = base.clone();
    nosig.signed = false;
    match nosig.items_complete() {
        Err(E_ITEM) => {}
        _ => return Err(E_ITEM),
    }
    let mut nodig = base.clone();
    nodig.audit_digest = 0;
    match nodig.items_complete() {
        Err(E_ITEM) => {}
        _ => return Err(E_ITEM),
    }
    let mut nots = base;
    nots.closed_at = 0;
    match nots.items_complete() {
        Err(E_ITEM) => hits += 1,
        _ => return Err(E_ITEM),
    }

    // R4 回执码域：越段码拒收。
    let mut probe = ReceiptLedger::new();
    let badcode = Receipt { gate: G_B02, code: 0, signed: true, audit_digest: 9, closed_at: 1 };
    match badcode.items_complete() {
        Err(E_CODE) => hits += 1,
        _ => return Err(E_CODE),
    }

    // R5 闭环签认：closure 全过。
    led.closure()?;
    hits += 1;

    // R6 重复回执拒：同放行位第二份拒收。
    let mut led2 = build_closure(&mut g)?;
    let dup = Receipt {
        gate: G_B01,
        code: SEG_BASE + 0x11,
        signed: true,
        audit_digest: 5,
        closed_at: 9,
    };
    match led2.register(dup) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R7 查询：在册可查、查无红。
    led2.lookup(G_B02)?;
    match led2.lookup(9) {
        Err(E_NOTFOUND) => hits += 1,
        _ => return Err(E_NOTFOUND),
    }

    // R8 指纹稳定。
    let f1 = led.fp();
    let f2 = led.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R9 呈现行：gates=2 closed=1。
    let line = ledger_line(&led);
    if !has_kw(line.as_bytes(), b"gates=2") || !has_kw(line.as_bytes(), b"closed=1") {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 放行面闭包：闭集外放行位拒收。
    let outgate = Receipt { gate: 9, code: SEG_BASE + 0x12, signed: true, audit_digest: 3, closed_at: 5 };
    match outgate.items_complete() {
        Err(E_GATE) => hits += 1,
        _ => return Err(E_GATE),
    }

    Ok(hits)
}
