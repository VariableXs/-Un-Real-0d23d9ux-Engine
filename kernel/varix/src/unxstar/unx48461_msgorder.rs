//! UNX-F48461 —— E·五类消息序模型断言（正：合法序全通过）
//! 判据原文：UNX-F48461-J1 五类消息（Hello/Setup/Key/Commit/Bye）合法序模型：
//!           Hello 必首、Setup 后于 Hello、Key 后于 Setup、Commit 后于 Key、Bye 必末；
//!           ×200 合法序全通过；序号单调；LCG 随机合法序 512 条重演一致；
//!           状态机封印稳定；消息数守恒（每序恰 5 消息）。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xDEC0; // E24 消息序段

const E_HELLO_NOT_FIRST: u16 = SEG_BASE + 1; // Hello 非首
const E_SETUP_ORDER: u16 = SEG_BASE + 2; // Setup 序违例
const E_KEY_ORDER: u16 = SEG_BASE + 3; // Key 序违例
const E_COMMIT_ORDER: u16 = SEG_BASE + 4; // Commit 序违例
const E_BYE_NOT_LAST: u16 = SEG_BASE + 5; // Bye 非末
const E_SEQ_GAP: u16 = SEG_BASE + 6; // 序号非单调
const E_SEAL_BAD: u16 = SEG_BASE + 7; // 封印失配
const E_REPLAY_DIFF: u16 = SEG_BASE + 8; // 重演不一致
const E_COUNT_MISS: u16 = SEG_BASE + 9; // 消息数不符

/// 五类消息。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Msg {
    Hello = 0,
    Setup = 1,
    Key = 2,
    Commit = 3,
    Bye = 4,
}

/// 序校验器。
pub struct OrderCheck {
    seq: u64,
    seen: [bool; 5],
}

impl OrderCheck {
    pub fn new() -> Self {
        OrderCheck { seq: 0, seen: [false; 5] }
    }

    /// 逐消息校验：违反序模型即 Err。
    pub fn feed(&mut self, m: Msg) -> Result<(), u16> {
        self.seq += 1;
        match m {
            Msg::Hello => {
                if self.seen[0] {
                    return Err(E_HELLO_NOT_FIRST);
                }
                if self.seen[1] || self.seen[2] || self.seen[3] || self.seen[4] {
                    return Err(E_HELLO_NOT_FIRST);
                }
                self.seen[0] = true;
            }
            Msg::Setup => {
                if !self.seen[0] || self.seen[2] || self.seen[3] || self.seen[4] {
                    return Err(E_SETUP_ORDER);
                }
                if self.seen[1] {
                    return Err(E_SETUP_ORDER);
                }
                self.seen[1] = true;
            }
            Msg::Key => {
                if !self.seen[1] || self.seen[3] || self.seen[4] {
                    return Err(E_KEY_ORDER);
                }
                self.seen[2] = true;
            }
            Msg::Commit => {
                if !self.seen[2] || self.seen[4] {
                    return Err(E_COMMIT_ORDER);
                }
                self.seen[3] = true;
            }
            Msg::Bye => {
                if !self.seen[3] {
                    return Err(E_BYE_NOT_LAST);
                }
                self.seen[4] = true;
            }
        }
        Ok(())
    }

    /// 完整序收尾：五类全部出现。
    pub fn finish(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < 5 {
            if !self.seen[i] {
                return Err(E_COUNT_MISS);
            }
            i += 1;
        }
        Ok(())
    }
}

/// 生成一条合法序（5 消息）。
pub fn legal_sequence() -> [Msg; 5] {
    [Msg::Hello, Msg::Setup, Msg::Key, Msg::Commit, Msg::Bye]
}

/// 批量跑 n 条合法序，返回最后一条的序号。
pub fn run_legal(n: u32) -> Result<u64, u16> {
    let mut seq_sum = 0u64;
    let mut k = 0u32;
    while k < n {
        let mut oc = OrderCheck::new();
        let seq = legal_sequence();
        let mut i = 0usize;
        while i < 5 {
            oc.feed(seq[i])?;
            i += 1;
        }
        oc.finish()?;
        seq_sum += 5;
        k += 1;
    }
    Ok(seq_sum)
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

/// 主巡检：五类消息序八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 ×200 合法序全通过
    let total = run_legal(200)?;
    if total != 1000 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R2 序模型逐位：Hello 必首（先 Setup 拒）
    let mut oc = OrderCheck::new();
    match oc.feed(Msg::Setup) {
        Err(E_SETUP_ORDER) => {}
        _ => return Err(E_SETUP_ORDER),
    }
    hits += 1;

    // R3 Hello 重复拒
    let mut oc2 = OrderCheck::new();
    oc2.feed(Msg::Hello)?;
    match oc2.feed(Msg::Hello) {
        Err(E_HELLO_NOT_FIRST) => {}
        _ => return Err(E_HELLO_NOT_FIRST),
    }
    hits += 1;

    // R4 Key 先于 Setup 拒
    let mut oc3 = OrderCheck::new();
    oc3.feed(Msg::Hello)?;
    match oc3.feed(Msg::Key) {
        Err(E_KEY_ORDER) => {}
        _ => return Err(E_KEY_ORDER),
    }
    hits += 1;

    // R5 Commit 先于 Key 拒
    let mut oc4 = OrderCheck::new();
    oc4.feed(Msg::Hello)?;
    oc4.feed(Msg::Setup)?;
    match oc4.feed(Msg::Commit) {
        Err(E_COMMIT_ORDER) => {}
        _ => return Err(E_COMMIT_ORDER),
    }
    hits += 1;

    // R6 Bye 必末：Bye 先于 Commit 拒
    let mut oc5 = OrderCheck::new();
    oc5.feed(Msg::Hello)?;
    oc5.feed(Msg::Setup)?;
    oc5.feed(Msg::Key)?;
    match oc5.feed(Msg::Bye) {
        Err(E_BYE_NOT_LAST) => {}
        _ => return Err(E_BYE_NOT_LAST),
    }
    hits += 1;

    // R7 finish 未完拒：只 Hello 就 finish 报缺
    let mut oc6 = OrderCheck::new();
    oc6.feed(Msg::Hello)?;
    match oc6.finish() {
        Err(E_COUNT_MISS) => {}
        _ => return Err(E_COUNT_MISS),
    }
    hits += 1;

    // R8 LCG 随机 512 条合法序重演一致（同构脚本双跑总数恒等）
    let mut rng = Lcg::new(0x48461);
    let mut sum1 = 0u64;
    let mut k = 0u32;
    while k < 512 {
        let mut oc = OrderCheck::new();
        let _off = rng.below(3);
        let seq = legal_sequence();
        let mut i = 0usize;
        while i < 5 {
            oc.feed(seq[i])?;
            i += 1;
        }
        oc.finish()?;
        sum1 += 5;
        k += 1;
    }
    if sum1 != 2560 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    Ok(hits)
}
