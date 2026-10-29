//! UNX-F48687 —— I·联签占比登记（≥30% 机检）
//! 判据原文：UNX-F48687-J1 机检通过；判据锚定 §AI-61 §3。
//! 判据落地（drill 场景）：R1 登记 ×10；R2 占比计算；R3 阈值通过；
//! R4 低于阈值检出；R5 条目闭集；R6 机检复跑一致；R7 空账拒收；
//! R8 呈现行；R9 指纹稳定；R10 基点精度。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0D40;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_COUNT: u16 = SEG_BASE + 1; // 条目数不足 10
const E_DUP: u16 = SEG_BASE + 2; // 卡号重复
const E_RATIO: u16 = SEG_BASE + 3; // 占比低于 30%
const E_CARD: u16 = SEG_BASE + 4; // 卡号越出本批
const E_FP: u16 = SEG_BASE + 5; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 6; // 呈现行缺段
const E_REPLAY: u16 = SEG_BASE + 7; // 机检复跑不一致

/// 占比阈值（基点）。
pub const RATIO_MIN_BP: u32 = 3_000;
/// 登记条数（本批 10 卡）。
pub const ITEM_N: usize = 10;
/// 本批卡号段。
pub const CARD_LO: u32 = 48_683;
pub const CARD_HI: u32 = 48_692;

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

/// 一条占比登记项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RatioItem {
    pub card_no: u32,
    pub is_joint: bool,
}

/// 占比登记账。
pub struct RatioLedger {
    pub items: Vec<RatioItem>,
}

impl RatioLedger {
    pub fn new() -> RatioLedger {
        RatioLedger { items: vec![] }
    }

    /// 登记：卡号段内唯一。
    pub fn register(&mut self, card_no: u32, is_joint: bool) -> Result<(), u16> {
        if card_no < CARD_LO || card_no > CARD_HI {
            return Err(E_CARD);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].card_no == card_no {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.items.push(RatioItem { card_no, is_joint });
        Ok(())
    }

    /// 联签条目数。
    pub fn joint_count(&self) -> u32 {
        let mut c: u32 = 0;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].is_joint {
                c += 1;
            }
            i += 1;
        }
        c
    }

    /// 占比（基点）。
    pub fn ratio_bp(&self) -> u32 {
        if self.items.is_empty() {
            return 0;
        }
        self.joint_count() * 10_000 / self.items.len() as u32
    }

    /// 机检：条目足额且占比 ≥30%。
    pub fn machine_check(&self) -> Result<(u32, u32), u16> {
        if self.items.len() < ITEM_N {
            return Err(E_COUNT);
        }
        let r = self.ratio_bp();
        if r < RATIO_MIN_BP {
            return Err(E_RATIO);
        }
        Ok((self.joint_count(), r))
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.items.len() {
            buf.extend_from_slice(&self.items[i].card_no.to_le_bytes());
            buf.push(self.items[i].is_joint as u8);
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 呈现行。
pub fn ledger_line(l: &RatioLedger) -> String {
    let r = l.ratio_bp();
    let mut out = String::new();
    out.push_str("n=");
    out.push_str(&l.items.len().to_string());
    out.push_str(" joint=");
    out.push_str(&l.joint_count().to_string());
    out.push_str(" ratio=");
    out.push_str(&r.to_string());
    if r >= RATIO_MIN_BP {
        out.push_str(" pass=1");
    } else {
        out.push_str(" pass=0");
    }
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
    let mut g = Lcg::new(0x486_87);

    // R1 登记 ×10：前 4 条联签（40% ≥ 30%）。
    let mut led = RatioLedger::new();
    let mut i: u32 = 0;
    while i < ITEM_N as u32 {
        led.register(CARD_LO + i, i < 4)?;
        i += 1;
    }
    if led.items.len() != ITEM_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R2 占比计算：4/10 = 4000bp。
    if led.ratio_bp() != 4_000 {
        return Err(E_RATIO);
    }
    hits += 1;

    // R3 阈值通过。
    let (jc, rb) = led.machine_check()?;
    if jc != 4 || rb != 4_000 {
        return Err(E_RATIO);
    }
    hits += 1;

    // R4 低于阈值检出：2/10 = 20% 红。
    let mut low = RatioLedger::new();
    let mut i: u32 = 0;
    while i < ITEM_N as u32 {
        low.register(CARD_LO + i, i < 2)?;
        i += 1;
    }
    match low.machine_check() {
        Err(E_RATIO) => hits += 1,
        _ => return Err(E_RATIO),
    }

    // R5 条目闭集：越段与重复拒收。
    let mut probe = RatioLedger::new();
    match probe.register(CARD_LO - 1, true) {
        Err(E_CARD) => {}
        _ => return Err(E_CARD),
    }
    match probe.register(CARD_HI + 1, true) {
        Err(E_CARD) => {}
        _ => return Err(E_CARD),
    }
    probe.register(CARD_LO, true)?;
    match probe.register(CARD_LO, false) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R6 机检复跑一致。
    let a = led.machine_check()?;
    let b = led.machine_check()?;
    if a != b {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R7 空账拒收：空账占比 0 < 30%。
    let empty = RatioLedger::new();
    if empty.ratio_bp() != 0 {
        return Err(E_COUNT);
    }
    match empty.machine_check() {
        Err(E_COUNT) => hits += 1,
        _ => return Err(E_COUNT),
    }

    // R8 呈现行：n=10 joint=4 ratio=4000 pass=1。
    let line = ledger_line(&led);
    if !has_kw(line.as_bytes(), b"n=10") || !has_kw(line.as_bytes(), b"joint=4")
        || !has_kw(line.as_bytes(), b"ratio=4000") || !has_kw(line.as_bytes(), b"pass=1")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R9 指纹稳定。
    let f1 = led.fp();
    let f2 = led.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 基点精度：1/3 ≈ 3333bp。
    let mut third = RatioLedger::new();
    third.register(CARD_LO, true)?;
    third.register(CARD_LO + 1, false)?;
    third.register(CARD_LO + 2, false)?;
    if third.ratio_bp() != 3_333 {
        return Err(E_RATIO);
    }
    let _ = g.below(10);
    hits += 1;

    Ok(hits)
}
