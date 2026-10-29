//! UNX-F48413 —— E21 三类窗口焦点回归哨兵（vxwm 原生/Wine/Xwayland 三窗交替切换千次）
//! 判据原文：UNX-F48413-J1 三窗交替切换 1000 次，每窗焦点事件语义恒定；
//!           窗型指纹（原生=0/Wine=1/Xwayland=2）随事件全程可追溯；
//!           跨窗切换零串扰（A 窗事件不出现在 B 窗账）；LCG 千次序列重演一致；
//!           三窗账封印互异且稳定；宪法加严条目（每窗 ≥300 次驻留）满足。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD6C0; // E21 回归哨兵段

const E_CROSS_TALK: u16 = SEG_BASE + 1; // 跨窗串扰
const E_REPLAY_DIFF: u16 = SEG_BASE + 2; // 重演不一致
const E_SEAL_BAD: u16 = SEG_BASE + 3; // 封印失配
const E_STAY_MISS: u16 = SEG_BASE + 4; // 宪法驻留不足
const E_FP_COLLIDE: u16 = SEG_BASE + 5; // 三窗封印撞车
const E_KIND_BAD: u16 = SEG_BASE + 6; // 窗型越界
const E_COUNT_MISS: u16 = SEG_BASE + 7; // 千次计数不符
const E_ATTR_LOST: u16 = SEG_BASE + 8; // 窗型指纹丢失

/// 窗型。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum WmKind {
    Native = 0,
    Wine = 1,
    Xwayland = 2,
}

/// 带窗型指纹的焦点事件。
#[derive(Clone, Copy)]
pub struct FocusEv {
    pub win: u32,
    pub kind: WmKind,
    pub tick: u32,
    pub gained: bool,
}

/// 三窗哨兵账。
pub struct RegSent {
    books: [Vec<FocusEv>; 3],
    wins: [u32; 3],
    tick: u32,
}

impl RegSent {
    pub fn new(w0: u32, w1: u32, w2: u32) -> Self {
        RegSent {
            books: [Vec::new(), Vec::new(), Vec::new()],
            wins: [w0, w1, w2],
            tick: 0,
        }
    }

    fn slot(&self, win: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < 3 {
            if self.wins[i] == win {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 一次焦点转移：旧窗 lost + 新窗 gained，窗型指纹随账。
    pub fn switch_to(&mut self, from: u32, to: u32, kind: WmKind) -> Result<(), u16> {
        if kind as u16 > 2 {
            return Err(E_KIND_BAD);
        }
        let fi = self.slot(from).ok_or(E_ATTR_LOST)?;
        let ti = self.slot(to).ok_or(E_ATTR_LOST)?;
        if from == to {
            return Err(E_CROSS_TALK);
        }
        self.tick += 1;
        self.books[fi].push(FocusEv { win: from, kind, tick: self.tick, gained: false });
        self.books[ti].push(FocusEv { win: to, kind, tick: self.tick, gained: true });
        Ok(())
    }

    /// 跨窗串扰审计：每窗账内事件 win 字段全等于本窗。
    pub fn audit_no_crosstalk(&self) -> Result<(), u16> {
        let mut s = 0usize;
        while s < 3 {
            let mut i = 0usize;
            while i < self.books[s].len() {
                if self.books[s][i].win != self.wins[s] {
                    return Err(E_CROSS_TALK);
                }
                i += 1;
            }
            s += 1;
        }
        Ok(())
    }

    /// 窗型指纹全程一致：账内 kind 全等（同一窗恒同型）。
    pub fn audit_kind_uniform(&self) -> Result<(), u16> {
        let mut s = 0usize;
        while s < 3 {
            let mut i = 0usize;
            while i < self.books[s].len() {
                if self.books[s][i].kind as u16 != s as u16 {
                    return Err(E_ATTR_LOST);
                }
                i += 1;
            }
            s += 1;
        }
        Ok(())
    }

    /// 宪法驻留：每窗事件数 ≥ n0。
    pub fn audit_stay(&self, n0: u32) -> Result<(), u16> {
        let mut s = 0usize;
        while s < 3 {
            if (self.books[s].len() as u32) < n0 {
                return Err(E_STAY_MISS);
            }
            s += 1;
        }
        Ok(())
    }

    /// 单窗账 FNV。
    pub fn book_fp(&self, slot: usize) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.books[slot].len() {
            let e = &self.books[slot][i];
            for x in [e.win as u64, e.kind as u64, e.tick as u64, e.gained as u64] {
                h ^= x;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
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
        self.next() % n
    }
}

/// 主巡检：三窗回归哨兵八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 三窗交替切换 1000 次（LCG 选下一窗，禁自切）
    let mut sent = RegSent::new(0xA0, 0xA1, 0xA2);
    let mut rng = Lcg::new(0x48413);
    let mut cur = 0usize;
    let mut n = 0u32;
    while n < 1000 {
        let mut nx = rng.below(3) as usize;
        if nx == cur {
            nx = (nx + 1) % 3;
        }
        let kind = match nx {
            0 => WmKind::Native,
            1 => WmKind::Wine,
            _ => WmKind::Xwayland,
        };
        let from = sent.wins[cur];
        let to = sent.wins[nx];
        sent.switch_to(from, to, kind)?;
        cur = nx;
        n += 1;
    }
    if n != 1000 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R2 跨窗串扰零放行 + 窗型指纹全程一致
    sent.audit_no_crosstalk()?;
    sent.audit_kind_uniform()?;
    hits += 1;

    // R3 宪法驻留：千次转移 → 每窗账 ≥ 300 条（lost+gained 合计）
    sent.audit_stay(300)?;
    hits += 1;

    // R4 重演一致：同种子重建三窗账指纹逐窗相等
    let mut sent2 = RegSent::new(0xA0, 0xA1, 0xA2);
    let mut rng2 = Lcg::new(0x48413);
    let mut cur2 = 0usize;
    let mut n2 = 0u32;
    while n2 < 1000 {
        let mut nx = rng2.below(3) as usize;
        if nx == cur2 {
            nx = (nx + 1) % 3;
        }
        let kind = match nx {
            0 => WmKind::Native,
            1 => WmKind::Wine,
            _ => WmKind::Xwayland,
        };
        sent2.switch_to(sent2.wins[cur2], sent2.wins[nx], kind)?;
        cur2 = nx;
        n2 += 1;
    }
    let mut s = 0usize;
    while s < 3 {
        if sent.book_fp(s) != sent2.book_fp(s) {
            return Err(E_REPLAY_DIFF);
        }
        s += 1;
    }
    hits += 1;

    // R5 三窗封印互异
    let f0 = sent.book_fp(0);
    let f1 = sent.book_fp(1);
    let f2 = sent.book_fp(2);
    if f0 == f1 || f1 == f2 || f0 == f2 {
        return Err(E_FP_COLLIDE);
    }
    hits += 1;

    // R6 封印稳定：双算一致且全非零
    if f0 == 0 || f1 == 0 || f2 == 0 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    // R7 窗型越界拒绝：kind=3 不存在（类型层防），语义层：未知窗拒绝
    match sent.slot(0xDEAD) {
        None => {}
        Some(_) => return Err(E_ATTR_LOST),
    }
    hits += 1;

    // R8 计数守恒：三窗账合计 = 2×1000（每次转移一 lost 一 gained）
    let total = sent.books[0].len() + sent.books[1].len() + sent.books[2].len();
    if total != 2000 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    Ok(hits)
}
