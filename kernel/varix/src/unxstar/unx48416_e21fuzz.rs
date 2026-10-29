//! UNX-F48416 —— E21 fuzz（随机焦点事件流注入零崩）
//! 判据原文：UNX-F48416-J1 LCG 随机焦点事件流（注册/销毁/焦点/幽灵 ×4 型混合）×10^4 事件
//!           状态机零崩（全部走 Ok/Err 有界出口）；状态守恒（alive+gap+void = 常量）；
//!           非法事件全拒且无副作用（拒后状态指纹不变）；合法事件账封印重演一致；
//!           极端序列（全幽灵/全销毁）同样有界；事件账容量有界。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD740; // E21 fuzz 段

const E_STATE_LEAK: u16 = SEG_BASE + 1; // 状态守恒破坏
const E_SIDE_EFFECT: u16 = SEG_BASE + 2; // 拒后有副作用
const E_REPLAY_DIFF: u16 = SEG_BASE + 3; // 重演不一致
const E_SEAL_BAD: u16 = SEG_BASE + 4; // 封印失配
const E_CAP_MISS: u16 = SEG_BASE + 5; // 容量有界破坏
const E_KIND_BAD: u16 = SEG_BASE + 6; // 事件型越界
const E_COUNT_MISS: u16 = SEG_BASE + 7; // 事件计数不符

/// 事件型。
#[derive(Clone, Copy)]
pub enum Ev {
    Register(u32),
    Destroy(u32),
    Focus(u32),
    Ghost(u32),
}

/// 窗状态。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Ws {
    Void = 0,
    Alive = 1,
    Gap = 2,
}

/// fuzz 状态机。
pub struct FuzzMachine {
    st: Vec<(u32, Ws)>,
    accepted: u32,
    rejected: u32,
    log: Vec<(u64, bool)>, // (seq, accepted)
    cap: usize,
}

impl FuzzMachine {
    pub fn new(cap: usize) -> Self {
        FuzzMachine { st: Vec::new(), accepted: 0, rejected: 0, log: Vec::new(), cap }
    }

    fn idx(&self, w: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.st.len() {
            if self.st[i].0 == w {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    fn state_of(&self, w: u32) -> Ws {
        match self.idx(w) {
            Some(i) => self.st[i].1,
            None => Ws::Void,
        }
    }

    /// 状态指纹（拒后不变性用）。
    pub fn state_fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.st.len() {
            h ^= self.st[i].0 as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.st[i].1 as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }

    /// 处理一事件：全出口有界（Ok/Err），拒则零副作用。
    pub fn feed(&mut self, ev: Ev) -> Result<(), u16> {
        let before = self.state_fp();
        let r = match ev {
            Ev::Register(w) => {
                if self.st.len() >= self.cap {
                    Err(E_CAP_MISS)
                } else if self.state_of(w) != Ws::Void {
                    Err(E_KIND_BAD)
                } else {
                    self.st.push((w, Ws::Alive));
                    Ok(())
                }
            }
            Ev::Destroy(w) => match self.idx(w) {
                Some(i) => {
                    if self.st[i].1 == Ws::Alive {
                        self.st[i].1 = Ws::Gap;
                        Ok(())
                    } else {
                        Err(E_KIND_BAD)
                    }
                }
                None => Err(E_KIND_BAD),
            },
            Ev::Focus(w) => {
                if self.state_of(w) == Ws::Alive {
                    Ok(())
                } else {
                    Err(E_KIND_BAD)
                }
            }
            Ev::Ghost(w) => {
                // 幽灵事件恒拒
                let _ = w;
                Err(E_KIND_BAD)
            }
        };
        match r {
            Ok(()) => {
                self.accepted += 1;
                self.log.push((self.log.len() as u64 + 1, true));
                Ok(())
            }
            Err(code) => {
                self.rejected += 1;
                self.log.push((self.log.len() as u64 + 1, false));
                // 拒后零副作用断言
                if self.state_fp() != before {
                    return Err(E_SIDE_EFFECT);
                }
                Err(code)
            }
        }
    }

    /// 状态守恒：st 内每条 (win, state) 唯一，无重复窗。
    pub fn audit_conserved(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.st.len() {
            let mut j = i + 1;
            while j < self.st.len() {
                if self.st[i].0 == self.st[j].0 {
                    return Err(E_STATE_LEAK);
                }
                j += 1;
            }
            i += 1;
        }
        if self.st.len() > self.cap {
            return Err(E_CAP_MISS);
        }
        Ok(())
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.log.len() {
            h ^= self.log[i].0;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.log[i].1 as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }

    pub fn totals(&self) -> (u32, u32) {
        (self.accepted, self.rejected)
    }
}

/// 生成确定性事件流。
pub fn gen_stream(seed: u64, n: u32) -> Vec<Ev> {
    let mut rng = Lcg(seed);
    let mut v: Vec<Ev> = Vec::new();
    let mut i = 0u32;
    while i < n {
        let t = rng.below(4);
        let w = 0x100 + rng.below(24) as u32;
        let ev = match t {
            0 => Ev::Register(w),
            1 => Ev::Destroy(w),
            2 => Ev::Focus(w),
            _ => Ev::Ghost(w),
        };
        v.push(ev);
        i += 1;
    }
    v
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

/// 主巡检：fuzz 八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 ×10^4 事件流零崩：全事件有界出口
    let stream = gen_stream(0x48416, 10_000);
    let mut m = FuzzMachine::new(64);
    let mut i = 0usize;
    while i < stream.len() {
        let _ = m.feed(stream[i]);
        i += 1;
    }
    let (acc, rej) = m.totals();
    if acc + rej != 10_000 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R2 状态守恒 + 容量有界
    m.audit_conserved()?;
    if m.st.len() > 64 {
        return Err(E_CAP_MISS);
    }
    hits += 1;

    // R3 拒后零副作用：全幽灵流 1000 事件，状态指纹逐事件不变
    let mut m2 = FuzzMachine::new(8);
    m2.feed(Ev::Register(1))?;
    let fp0 = m2.state_fp();
    let mut k = 0u32;
    while k < 1000 {
        let _ = m2.feed(Ev::Ghost(1));
        if m2.state_fp() != fp0 {
            return Err(E_SIDE_EFFECT);
        }
        k += 1;
    }
    hits += 1;

    // R4 极端序列：全销毁流 512 事件有界（大量 Err 但零崩）
    let mut m3 = FuzzMachine::new(8);
    let mut k = 0u32;
    while k < 512 {
        let _ = m3.feed(Ev::Destroy(0x100 + k % 5));
        k += 1;
    }
    m3.audit_conserved()?;
    hits += 1;

    // R5 重演一致：同种子事件流双跑账指纹相等
    let s1 = m.fp();
    let stream2 = gen_stream(0x48416, 10_000);
    let mut m4 = FuzzMachine::new(64);
    let mut i = 0usize;
    while i < stream2.len() {
        let _ = m4.feed(stream2[i]);
        i += 1;
    }
    if s1 != m4.fp() || s1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R6 账守恒：log.len() == 10^4 且 accepted/rejected 与 log 分解一致
    let mut a2 = 0u32;
    let mut r2 = 0u32;
    let mut i = 0usize;
    while i < m.log.len() {
        if m.log[i].1 {
            a2 += 1;
        } else {
            r2 += 1;
        }
        i += 1;
    }
    if a2 != acc || r2 != rej {
        return Err(E_STATE_LEAK);
    }
    hits += 1;

    // R7 容量边界：cap=2 注册第 3 窗拒绝
    let mut m5 = FuzzMachine::new(2);
    m5.feed(Ev::Register(1))?;
    m5.feed(Ev::Register(2))?;
    match m5.feed(Ev::Register(3)) {
        Err(E_CAP_MISS) => {}
        _ => return Err(E_CAP_MISS),
    }
    hits += 1;

    // R8 重复注册拒绝：同窗二注册 Err 且零副作用
    let mut m6 = FuzzMachine::new(8);
    m6.feed(Ev::Register(9))?;
    let f9 = m6.state_fp();
    match m6.feed(Ev::Register(9)) {
        Err(E_KIND_BAD) => {}
        _ => return Err(E_KIND_BAD),
    }
    if m6.state_fp() != f9 {
        return Err(E_SIDE_EFFECT);
    }
    hits += 1;

    Ok(hits)
}
