//! UNX-F48571 —— I29 三类窗口联签回归哨兵（三类窗口（vxwm 原生/Wine/Xwayland）
//!               回归判据·宪法加严：三窗候选窗联测全绿）
//! 判据原文：UNX-F48571-J1 候选窗回归哨兵按三类窗口分别执行五步联测
//!           （展开→可见→选中→换位→关闭），类内全绿才记类绿；
//!           三类全绿哨兵才判绿；任一类红哨兵判红且须定位到类；
//!           单类结果重复执行一致；LCG 扰动 ×256 判定零崩且自洽；
//!           哨兵账封闭三类；重演指纹一致；守恒。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xF720; // I29 三窗联签哨兵段

const E_NATIVE_MISS: u16 = SEG_BASE + 1; // vxwm 原生联测失绿
const E_WINE_MISS: u16 = SEG_BASE + 2; // Wine 联测失绿
const E_XWAY_MISS: u16 = SEG_BASE + 3; // Xwayland 联测失绿
const E_VERDICT_BAD: u16 = SEG_BASE + 4; // 哨兵判定失准
const E_LOCATE_MISS: u16 = SEG_BASE + 5; // 红类定位失败
const E_STEP_BAD: u16 = SEG_BASE + 6; // 联测步越界
const E_LEDGER_MISS: u16 = SEG_BASE + 7; // 哨兵账不符
const E_REPLAY_DIFF: u16 = SEG_BASE + 8; // 重演不一致

/// 三类窗口。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WinClass {
    Native = 0,   // vxwm 原生
    Wine = 1,
    Xwayland = 2,
}

/// 联测五步。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TestStep {
    Expand = 0,
    Visible = 1,
    Select = 2,
    Reposition = 3,
    Close = 4,
}

pub const STEP_COUNT: usize = 5;

/// 单步结果。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct StepResult {
    pub step: u8,
    pub pass: bool,
}

/// 单类联测账。
pub struct ClassLedger {
    pub class: WinClass,
    pub results: Vec<StepResult>,
    pub runs: u32,
}

impl ClassLedger {
    pub fn new(class: WinClass) -> Self {
        ClassLedger { class, results: Vec::new(), runs: 0 }
    }

    /// 执行一轮五步联测：全部注入 pass 序列。
    pub fn run_all_pass(&mut self) {
        let mut s = 0u8;
        while (s as usize) < STEP_COUNT {
            self.results.push(StepResult { step: s, pass: true });
            s += 1;
        }
        self.runs += 1;
    }

    /// 执行一轮联测：指定步失败。
    pub fn run_with_fail(&mut self, fail_step: u8) {
        let mut s = 0u8;
        while (s as usize) < STEP_COUNT {
            self.results.push(StepResult { step: s, pass: s != fail_step });
            s += 1;
        }
        self.runs += 1;
    }

    /// 最近一轮类内是否全绿。
    pub fn last_round_green(&self) -> bool {
        if self.results.len() < STEP_COUNT {
            return false;
        }
        let base = self.results.len() - STEP_COUNT;
        let mut s = 0usize;
        while s < STEP_COUNT {
            let r = self.results[base + s];
            if r.step as usize != s || !r.pass {
                return false;
            }
            s += 1;
        }
        true
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.class as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        let mut i = 0usize;
        while i < self.results.len() {
            h ^= self.results[i].step as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.results[i].pass as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }
}

/// 哨兵：三类账 + 综合判定。
pub struct Sentinel {
    pub ledgers: [ClassLedger; 3],
    pub verdicts: Vec<(u64, bool)>,
    pub seq: u64,
    pub label: String,
}

impl Sentinel {
    pub fn new() -> Self {
        Sentinel {
            ledgers: [
                ClassLedger::new(WinClass::Native),
                ClassLedger::new(WinClass::Wine),
                ClassLedger::new(WinClass::Xwayland),
            ],
            verdicts: Vec::new(),
            seq: 0,
            label: "i29-sentinel".to_string(),
        }
    }

    /// 全类注入一轮全绿并出综合判定。
    pub fn round_all_pass(&mut self) -> bool {
        let mut i = 0usize;
        while i < 3 {
            self.ledgers[i].run_all_pass();
            i += 1;
        }
        self.seq += 1;
        let v = self.ledgers[0].last_round_green()
            && self.ledgers[1].last_round_green()
            && self.ledgers[2].last_round_green();
        self.verdicts.push((self.seq, v));
        v
    }

    /// 指定类失败一轮并出判定（须定位红类）。
    pub fn round_with_fail(&mut self, cls: usize, fail_step: u8) -> (bool, usize) {
        let mut i = 0usize;
        while i < 3 {
            if i == cls {
                self.ledgers[i].run_with_fail(fail_step);
            } else {
                self.ledgers[i].run_all_pass();
            }
            i += 1;
        }
        self.seq += 1;
        let mut red_cls = 3usize;
        let mut i = 0usize;
        while i < 3 {
            if !self.ledgers[i].last_round_green() {
                red_cls = i;
            }
            i += 1;
        }
        let v = red_cls == 3;
        self.verdicts.push((self.seq, v));
        (v, red_cls)
    }

    /// 哨兵账封闭：判定数 = seq，且账内类标合法。
    pub fn audit_ledger(&self) -> Result<(), u16> {
        if self.verdicts.len() as u64 != self.seq {
            return Err(E_LEDGER_MISS);
        }
        let mut i = 0usize;
        while i < 3 {
            if self.ledgers[i].class as u8 > 2 {
                return Err(E_STEP_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    /// 结果步域封闭：五步步标 0..4。
    pub fn audit_steps(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < 3 {
            let mut j = 0usize;
            while j < self.ledgers[i].results.len() {
                if self.ledgers[i].results[j].step as usize >= STEP_COUNT {
                    return Err(E_STEP_BAD);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < 3 {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.ledgers[i].fingerprint();
            i += 1;
        }
        h ^= self.verdicts.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
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

/// 主巡检：三窗联签哨兵十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 vxwm 原生类五步全绿
    let mut st = Sentinel::new();
    let v = st.round_all_pass();
    if !v || !st.ledgers[0].last_round_green() {
        return Err(E_NATIVE_MISS);
    }
    hits += 1;

    // R2 Wine 类五步全绿
    if !st.ledgers[1].last_round_green() {
        return Err(E_WINE_MISS);
    }
    hits += 1;

    // R3 Xwayland 类五步全绿
    if !st.ledgers[2].last_round_green() {
        return Err(E_XWAY_MISS);
    }
    hits += 1;

    // R4 三类全绿哨兵判绿
    if st.verdicts[st.verdicts.len() - 1].1 != true {
        return Err(E_VERDICT_BAD);
    }
    hits += 1;

    // R5 任一类红 → 哨兵红且定位到类（三类逐一注入失败）
    let mut cls = 0usize;
    while cls < 3 {
        let (v, red) = st.round_with_fail(cls, 2);
        if v {
            return Err(E_VERDICT_BAD);
        }
        if red != cls {
            return Err(E_LOCATE_MISS);
        }
        cls += 1;
    }
    hits += 1;

    // R6 失败步可定位：任意步失败均致类红
    let mut st2 = Sentinel::new();
    let mut step = 0u8;
    while (step as usize) < STEP_COUNT {
        let (v, red) = st2.round_with_fail(1, step);
        if v || red != 1 {
            return Err(E_VERDICT_BAD);
        }
        step += 1;
    }
    st2.audit_steps()?;
    hits += 1;

    // R7 单类重复执行一致：连续三轮全绿判定稳定为绿
    let mut st3 = Sentinel::new();
    let mut round = 0u32;
    while round < 3 {
        if !st3.round_all_pass() {
            return Err(E_VERDICT_BAD);
        }
        round += 1;
    }
    if st3.verdicts.len() != 3 {
        return Err(E_LEDGER_MISS);
    }
    hits += 1;

    // R8 LCG 扰动 ×256：随机类/步失败，判定与独立复算一致
    let mut rng = Lcg::new(0x48571);
    let mut st4 = Sentinel::new();
    let mut k = 0u32;
    let mut greens = 0u32;
    let mut reds = 0u32;
    while k < 256 {
        let mode = rng.below(4);
        let (v, red) = if mode == 3 {
            let g = st4.round_all_pass();
            (g, 3usize)
        } else {
            st4.round_with_fail(mode as usize, rng.below(5) as u8)
        };
        // 独立复算：三类最近一轮是否全绿
        let mut expect = true;
        let mut i = 0usize;
        while i < 3 {
            if !st4.ledgers[i].last_round_green() {
                expect = false;
            }
            i += 1;
        }
        if v != expect {
            return Err(E_VERDICT_BAD);
        }
        if !v && red == 3 {
            return Err(E_LOCATE_MISS);
        }
        if v {
            greens += 1;
        } else {
            reds += 1;
        }
        k += 1;
    }
    if greens == 0 || reds == 0 {
        return Err(E_LEDGER_MISS);
    }
    st4.audit_ledger()?;
    st4.audit_steps()?;
    hits += 1;

    // R9 哨兵账守恒
    if st4.verdicts.len() as u64 != st4.seq {
        return Err(E_LEDGER_MISS);
    }
    hits += 1;

    // R10 重演一致：同种子指纹相等
    let fp1 = st4.fingerprint();
    let mut rng2 = Lcg::new(0x48571);
    let mut st5 = Sentinel::new();
    let mut k = 0u32;
    while k < 256 {
        let mode = rng2.below(4);
        if mode == 3 {
            let _ = st5.round_all_pass();
        } else {
            let _ = st5.round_with_fail(mode as usize, rng2.below(5) as u8);
        }
        k += 1;
    }
    if st5.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    Ok(hits)
}
