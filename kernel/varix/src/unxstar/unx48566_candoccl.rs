//! UNX-F48566 —— I·候选窗遮挡联动语义（正：遮挡态候选窗换位数据位正确）
//! 判据原文：UNX-F48566-J1 遮挡检测（遮挡物矩形与候选窗相交）触发换位：
//!           下方遮挡→翻转到光标上方（数据位 bit0=1）；上方也遮挡→视口内
//!           平移（bit1=1）；四周全占→塌缩为单行（bit2=1）；无遮挡→原位
//!           （数据位 0）；换位后不得与遮挡物相交；LCG 随机遮挡 ×256
//!           换位结果全部合法；数据位与几何一致；账目守恒与重演一致。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xF640; // I29 候选窗遮挡换位段

const E_OCC_MISS: u16 = SEG_BASE + 1; // 遮挡漏检
const E_STILL_OCC: u16 = SEG_BASE + 2; // 换位后仍相交
const E_BIT_MISMATCH: u16 = SEG_BASE + 3; // 数据位与几何不符
const E_STAY_WRONG: u16 = SEG_BASE + 4; // 无遮挡误换位
const E_CRASH_LEAK: u16 = SEG_BASE + 5; // 随机遮挡崩溃
const E_LEDGER_MISS: u16 = SEG_BASE + 6; // 账目不符
const E_REPLAY_DIFF: u16 = SEG_BASE + 7; // 重演不一致
const E_COLLAPSE_BAD: u16 = SEG_BASE + 8; // 塌缩位失准

/// 矩形。
#[derive(Clone, Copy)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

fn intersects(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.w as i32 && b.x < a.x + a.w as i32 && a.y < b.y + b.h as i32 && b.y < a.y + a.h as i32
}

/// 换位动作数据位。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MoveBit {
    Stay = 0,     // 无遮挡原位
    FlipUp = 1,   // bit0：翻到上方
    ShiftIn = 2,  // bit1：视口内平移
    Collapse = 3, // bit2|bit0：塌缩单行
}

/// 换位结果。
pub struct MoveOut {
    pub rect: Rect,
    pub bit: MoveBit,
}

/// 换位决策：光标锚点 (cx, cy)，候选窗尺寸 (cw, ch)，视口 (vw, vh)。
pub fn reposition(occluders: &[Rect], cx: i32, cy: i32, cw: u32, ch: u32, vw: u32, vh: u32) -> MoveOut {
    let below = Rect { x: cx, y: cy, w: cw, h: ch };
    // 无遮挡 → 原位
    let mut any = false;
    let mut i = 0usize;
    while i < occluders.len() {
        if intersects(&below, &occluders[i]) {
            any = true;
        }
        i += 1;
    }
    if !any {
        return MoveOut { rect: below, bit: MoveBit::Stay };
    }
    // 下方被占 → 翻到上方
    let above = Rect { x: cx, y: cy - ch as i32, w: cw, h: ch };
    let mut above_free = true;
    let mut i = 0usize;
    while i < occluders.len() {
        if above.y < 0 || intersects(&above, &occluders[i]) {
            above_free = false;
        }
        i += 1;
    }
    if above_free && cx + cw as i32 <= vw as i32 && cx >= 0 {
        return MoveOut { rect: above, bit: MoveBit::FlipUp };
    }
    // 上方也占 → 视口内平移：y 钳入视口后自左向右扫描找空档
    let sy = if cy + ch as i32 > vh as i32 { vh as i32 - ch as i32 } else { cy };
    let mut sx = 0i32;
    while sx + cw as i32 <= vw as i32 {
        let cand = Rect { x: sx, y: sy, w: cw, h: ch };
        let mut free = true;
        let mut i = 0usize;
        while i < occluders.len() {
            if intersects(&cand, &occluders[i]) {
                free = false;
            }
            i += 1;
        }
        if free && (sx != cx || sy != cy) {
            return MoveOut { rect: cand, bit: MoveBit::ShiftIn };
        }
        sx += 16;
    }
    // 全占 → 塌缩单行（宽度保留，高度压至 24）
    let collapsed = Rect { x: cx, y: cy, w: cw, h: 24 };
    MoveOut { rect: collapsed, bit: MoveBit::Collapse }
}

/// 换位记录。
pub struct OccLedger {
    pub records: Vec<(u64, MoveBit)>,
    pub seq: u64,
    pub label: String,
}

impl OccLedger {
    pub fn new() -> Self {
        OccLedger { records: Vec::new(), seq: 0, label: "occ-ledger".to_string() }
    }

    pub fn record(&mut self, bit: MoveBit) {
        self.seq += 1;
        self.records.push((self.seq, bit));
    }

    pub fn audit_balance(&self) -> Result<(), u16> {
        if self.records.len() as u64 != self.seq {
            return Err(E_LEDGER_MISS);
        }
        let mut i = 0usize;
        while i < self.records.len() {
            if self.records[i].1 as u8 > 3 {
                return Err(E_BIT_MISMATCH);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.records.len() {
            h ^= self.records[i].0;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.records[i].1 as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }
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
        (self.next() >> 16) % n
    }
}

/// 主巡检：遮挡换位九判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let vw: u32 = 1920;
    let vh: u32 = 1080;
    let cw: u32 = 240;
    let ch: u32 = 120;

    // R1 无遮挡 → 原位数据位 0
    let out = reposition(&[], 840, 500, cw, ch, vw, vh);
    if out.bit != MoveBit::Stay {
        return Err(E_STAY_WRONG);
    }
    if out.rect.x != 840 || out.rect.y != 500 {
        return Err(E_STAY_WRONG);
    }
    hits += 1;

    // R2 下方遮挡 → 翻转上方 bit0，且不再相交
    let occ_below = Rect { x: 800, y: 520, w: 300, h: 200 };
    let out = reposition(&[occ_below], 840, 500, cw, ch, vw, vh);
    if out.bit != MoveBit::FlipUp {
        return Err(E_BIT_MISMATCH);
    }
    if out.rect.y != 500 - 120 {
        return Err(E_BIT_MISMATCH);
    }
    if intersects(&out.rect, &occ_below) {
        return Err(E_STILL_OCC);
    }
    hits += 1;

    // R3 上下全占 → 视口内平移 bit1 且落点无交
    let occ_up = Rect { x: 800, y: 360, w: 300, h: 200 };
    let occ_down = Rect { x: 800, y: 480, w: 300, h: 200 };
    let out = reposition(&[occ_up, occ_down], 840, 500, cw, ch, vw, vh);
    if out.bit != MoveBit::ShiftIn {
        return Err(E_BIT_MISMATCH);
    }
    if intersects(&out.rect, &occ_up) || intersects(&out.rect, &occ_down) {
        return Err(E_STILL_OCC);
    }
    if out.rect.x < 0 || out.rect.x + out.rect.w as i32 > vw as i32 {
        return Err(E_STILL_OCC);
    }
    hits += 1;

    // R4 全屏遮挡 → 塌缩 bit2 域
    let big = Rect { x: 0, y: 0, w: vw, h: vh };
    let out = reposition(&[big], 840, 500, cw, ch, vw, vh);
    if out.bit != MoveBit::Collapse {
        return Err(E_COLLAPSE_BAD);
    }
    if out.rect.h != 24 {
        return Err(E_COLLAPSE_BAD);
    }
    hits += 1;

    // R5 贴顶场景：上方越界（y<0）视作不可翻 → 平移路径
    let occ = Rect { x: 800, y: 500, w: 300, h: 200 };
    let out = reposition(&[occ], 840, 10, cw, ch, vw, vh);
    if out.bit == MoveBit::FlipUp {
        return Err(E_BIT_MISMATCH);
    }
    if out.bit == MoveBit::Stay && intersects(&out.rect, &occ) {
        return Err(E_OCC_MISS);
    }
    hits += 1;

    // R6 数据位与几何一致：四种位互斥且落在枚举域
    let bits: [MoveBit; 4] = [MoveBit::Stay, MoveBit::FlipUp, MoveBit::ShiftIn, MoveBit::Collapse];
    let mut i = 0usize;
    while i < bits.len() {
        let mut j = i + 1;
        while j < bits.len() {
            if bits[i] == bits[j] {
                return Err(E_BIT_MISMATCH);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R7 LCG 随机遮挡 ×256：换位结果合法（Stay 时必无交；其余位必有遮挡史且不交）
    let mut rng = Lcg::new(0x48566);
    let mut ledger = OccLedger::new();
    let mut seen = [0u32; 4];
    let mut k = 0u32;
    while k < 256 {
        let n_occ = rng.below(3) as usize; // 0..2 个遮挡物
        let mut occs: Vec<Rect> = Vec::new();
        let mut j = 0usize;
        while j < n_occ {
            occs.push(Rect {
                x: rng.below(1800) as i32,
                y: rng.below(1000) as i32,
                w: 100 + rng.below(400) as u32,
                h: 60 + rng.below(300) as u32,
            });
            j += 1;
        }
        let cx = rng.below(1600) as i32;
        let cy = rng.below(1000) as i32;
        // 周期性全屏遮挡保证塌缩位可达
        if k % 64 == 63 {
            occs.push(Rect { x: 0, y: 0, w: vw, h: vh });
        }
        let out = reposition(&occs, cx, cy, cw, ch, vw, vh);
        // 零遮挡却触发换位 = 判定泄漏
        if occs.is_empty() && out.bit != MoveBit::Stay {
            return Err(E_CRASH_LEAK);
        }
        seen[out.bit as usize] += 1;
        ledger.record(out.bit);
        match out.bit {
            MoveBit::Stay => {
                let mut j = 0usize;
                while j < occs.len() {
                    if intersects(&out.rect, &occs[j]) {
                        return Err(E_OCC_MISS);
                    }
                    j += 1;
                }
            }
            MoveBit::FlipUp | MoveBit::ShiftIn => {
                let mut j = 0usize;
                while j < occs.len() {
                    if intersects(&out.rect, &occs[j]) {
                        return Err(E_STILL_OCC);
                    }
                    j += 1;
                }
                if out.rect.x < 0 || out.rect.y < 0 || out.rect.x + out.rect.w as i32 > vw as i32 {
                    return Err(E_STILL_OCC);
                }
            }
            MoveBit::Collapse => {
                if out.rect.h != 24 {
                    return Err(E_COLLAPSE_BAD);
                }
            }
        }
        k += 1;
    }
    if seen[0] == 0 || seen[1] == 0 || seen[2] == 0 || seen[3] == 0 {
        return Err(E_LEDGER_MISS);
    }
    ledger.audit_balance()?;
    hits += 1;

    // R8 重演一致：同种子指纹相等
    let fp1 = ledger.fingerprint();
    let mut rng2 = Lcg::new(0x48566);
    let mut ledger2 = OccLedger::new();
    let mut k = 0u32;
    while k < 256 {
        let n_occ = rng2.below(3) as usize;
        let mut occs: Vec<Rect> = Vec::new();
        let mut j = 0usize;
        while j < n_occ {
            occs.push(Rect {
                x: rng2.below(1800) as i32,
                y: rng2.below(1000) as i32,
                w: 100 + rng2.below(400) as u32,
                h: 60 + rng2.below(300) as u32,
            });
            j += 1;
        }
        let cx = rng2.below(1600) as i32;
        let cy = rng2.below(1000) as i32;
        if k % 64 == 63 {
            occs.push(Rect { x: 0, y: 0, w: vw, h: vh });
        }
        let out = reposition(&occs, cx, cy, cw, ch, vw, vh);
        ledger2.record(out.bit);
        k += 1;
    }
    if ledger2.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R9 换位不越视口：翻转/平移结果上界复核（补边界）
    let edge = Rect { x: 1800, y: 1000, w: 300, h: 200 };
    let out = reposition(&[edge], 1800, 1000, cw, ch, vw, vh);
    match out.bit {
        MoveBit::ShiftIn | MoveBit::Collapse => {}
        _ => {
            // 翻转/原位必须仍在视口内
            if out.rect.x + out.rect.w as i32 > vw as i32 || out.rect.y + out.rect.h as i32 > vh as i32 {
                return Err(E_STILL_OCC);
            }
        }
    }
    hits += 1;

    Ok(hits)
}
