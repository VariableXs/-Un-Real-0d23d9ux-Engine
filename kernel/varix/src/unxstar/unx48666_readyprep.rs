//! UNX-F48666 —— I·AI-82 账实一致外抽预备（字段/数值一致率就绪位）
//! 判据原文：UNX-F48666-J1 就绪位自检通过；判据锚定 附则五。
//! 判据落地（drill 场景）：R1 抽样框 50 槽；R2 账实一致率 100%；
//! R3 失配检出；R4 槽位唯一；R5 一致率复算；R6 空槽拒收；
//! R7 复跑一致；R8 指纹稳定；R9 呈现行；R10 就绪位自检。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0AA0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_SLOT: u16 = SEG_BASE + 1; // 槽位越界或重复
const E_MISMATCH: u16 = SEG_BASE + 2; // 账实失配未检出
const E_RATE: u16 = SEG_BASE + 3; // 一致率不足 100%
const E_EMPTY: u16 = SEG_BASE + 4; // 空槽位
const E_COUNT: u16 = SEG_BASE + 5; // 槽数不足 50
const E_FP: u16 = SEG_BASE + 6; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 7; // 呈现行缺段
const E_READY: u16 = SEG_BASE + 8; // 就绪位自检失败
const E_REPLAY: u16 = SEG_BASE + 9; // 复跑不一致
const E_FIELD: u16 = SEG_BASE + 10; // 字段名非法

/// 抽样框槽数。
pub const SLOT_N: usize = 50;
/// 一致率阈值（外抽预备就绪位要求 100%）。
pub const RATE_BP: u32 = 10_000;

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

/// 一个外抽预备槽位：字段名 + 账面值 + 实际值。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PrepSlot {
    pub slot: u32,
    pub field: String,
    pub acct_val: u64,
    pub real_val: u64,
}

impl PrepSlot {
    /// 字段名合法：非空且 ≥3 字符。
    pub fn field_ok(&self) -> bool {
        self.field.len() >= 3
    }

    /// 账实一致。
    pub fn matched(&self) -> bool {
        self.acct_val == self.real_val
    }
}

/// 就绪位：抽样框 + 一致率。
#[derive(Clone)]
pub struct ReadyFrame {
    pub slots: Vec<PrepSlot>,
}

impl ReadyFrame {
    pub fn new() -> ReadyFrame {
        ReadyFrame { slots: vec![] }
    }

    /// 登记槽位：槽号唯一 + 字段名合法 + 非空槽。
    pub fn put(&mut self, s: PrepSlot) -> Result<(), u16> {
        if s.field.is_empty() || !s.field_ok() {
            return Err(E_FIELD);
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

    /// 槽位唯一自检。
    pub fn slots_unique(&self) -> bool {
        let mut i = 0usize;
        while i < self.slots.len() {
            let mut j = i + 1;
            while j < self.slots.len() {
                if self.slots[i].slot == self.slots[j].slot {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 一致率（基点）：matched / total ×10000。
    pub fn rate_bp(&self) -> u32 {
        if self.slots.is_empty() {
            return 0;
        }
        let mut m: u32 = 0;
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].matched() {
                m += 1;
            }
            i += 1;
        }
        m * 10_000 / self.slots.len() as u32
    }

    /// 失配槽位清单。
    pub fn mismatches(&self) -> Vec<u32> {
        let mut v: Vec<u32> = vec![];
        let mut i = 0usize;
        while i < self.slots.len() {
            if !self.slots[i].matched() {
                v.push(self.slots[i].slot);
            }
            i += 1;
        }
        v
    }

    /// 就绪位自检：50 槽齐、槽唯一、一致率 100%。
    pub fn ready(&self) -> Result<(), u16> {
        if self.slots.len() != SLOT_N {
            return Err(E_COUNT);
        }
        if !self.slots_unique() {
            return Err(E_SLOT);
        }
        if self.rate_bp() != RATE_BP {
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
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 构造 50 槽一致抽样框。
pub fn build_frame(g: &mut Lcg) -> Result<ReadyFrame, u16> {
    let mut f = ReadyFrame::new();
    let mut k: u32 = 1;
    while k <= SLOT_N as u32 {
        let v = 100 + g.below(9_000) as u64;
        f.put(PrepSlot {
            slot: k,
            field: concat_field(k),
            acct_val: v,
            real_val: v,
        })?;
        k += 1;
    }
    Ok(f)
}

/// 字段名拼装：acct.<slot 三位零填充不保证，直接十进制>。
pub fn concat_field(k: u32) -> String {
    let mut out = String::new();
    out.push_str("acct.");
    out.push_str(&k.to_string());
    out
}

/// 呈现行：n=<槽数> rate=<一致率bp> mm=<失配数>。
pub fn frame_line(f: &ReadyFrame) -> String {
    let mut out = String::new();
    out.push_str("n=");
    out.push_str(&f.slots.len().to_string());
    out.push_str(" rate=");
    out.push_str(&f.rate_bp().to_string());
    out.push_str(" mm=");
    out.push_str(&f.mismatches().len().to_string());
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
    let mut g = Lcg::new(0x486_66);

    // R1 抽样框 50 槽齐。
    let frame = build_frame(&mut g)?;
    if frame.slots.len() != SLOT_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R2 账实一致率 100%。
    if frame.rate_bp() != RATE_BP {
        return Err(E_RATE);
    }
    hits += 1;

    // R3 失配检出：改一槽实际值 → 一致率下降、失配清单非空。
    let mut bad = frame.clone();
    bad.slots[7].real_val = bad.slots[7].real_val.wrapping_add(1);
    if bad.rate_bp() != 9_980 {
        return Err(E_MISMATCH);
    }
    let mm = bad.mismatches();
    if mm.len() != 1 || mm[0] != 8 {
        return Err(E_MISMATCH);
    }
    hits += 1;

    // R4 槽位唯一：重复槽号拒收。
    let mut probe = ReadyFrame::new();
    probe.put(PrepSlot { slot: 1, field: "acct.1".to_string(), acct_val: 5, real_val: 5 })?;
    match probe.put(PrepSlot { slot: 1, field: "acct.1b".to_string(), acct_val: 6, real_val: 6 }) {
        Err(E_SLOT) => hits += 1,
        _ => return Err(E_SLOT),
    }

    // R5 一致率复算：两次一致。
    let r1 = frame.rate_bp();
    let r2 = frame.rate_bp();
    if r1 != r2 {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R6 空槽拒收：槽号 0 与越界 51 均拒。
    let mut probe2 = ReadyFrame::new();
    match probe2.put(PrepSlot { slot: 0, field: "acct.0".to_string(), acct_val: 1, real_val: 1 }) {
        Err(E_SLOT) => {}
        _ => return Err(E_SLOT),
    }
    match probe2.put(PrepSlot { slot: 51, field: "acct.51".to_string(), acct_val: 1, real_val: 1 }) {
        Err(E_SLOT) => hits += 1,
        _ => return Err(E_SLOT),
    }

    // R7 复跑一致：重建抽样框指纹一致（同种子）。
    let frame2 = build_frame(&mut g2_seed())?;
    if frame2.fp() != frame.fp() {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R8 指纹稳定。
    let f1 = frame.fp();
    let f2 = frame.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R9 呈现行：n=50 rate=10000 mm=0。
    let line = frame_line(&frame);
    if !has_kw(line.as_bytes(), b"n=50") || !has_kw(line.as_bytes(), b"rate=10000")
        || !has_kw(line.as_bytes(), b"mm=0")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 就绪位自检通过。
    frame.ready()?;
    hits += 1;

    Ok(hits)
}

/// 复跑用独立种子。
pub fn g2_seed() -> Lcg {
    Lcg::new(0x486_66)
}
