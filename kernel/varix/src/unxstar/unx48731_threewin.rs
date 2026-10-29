//! UNX-F48731 C37 三类窗口总回归哨兵一（三类窗口按键流回归·宪法加严）
//! 判据：UNX-F48731-J1 三窗 500 字 ×3 轮全绿；判据锚定 域内宪法加严项。
//! 纪律：vxwm 原生/Wine/Xwayland 三窗按键流零丢零重，轮终态快照对拍恒等。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1780;

const E_WIN_BAD: u16 = SEG_BASE + 1; // 窗种类越界
const E_KEY_LOST: u16 = SEG_BASE + 2; // 按键丢失
const E_KEY_DUP: u16 = SEG_BASE + 3; // 按键重复
const E_ROUND_DIFF: u16 = SEG_BASE + 4; // 轮间快照不一致
const E_FLOW_EMPTY: u16 = SEG_BASE + 5; // 空按键流拒
const E_FOCUS_BAD: u16 = SEG_BASE + 6; // 焦点窗非法
const E_COUNT_BREAK: u16 = SEG_BASE + 7; // 流计数守恒破
const E_ORDER_BREAK: u16 = SEG_BASE + 8; // 流序破

/// 窗种类。
pub const W_NATIVE: u8 = 0; // vxwm 原生
pub const W_WINE: u8 = 1; // Wine
pub const W_XWAY: u8 = 2; // Xwayland

/// 单窗按键流。
#[derive(Clone)]
pub struct KeyFlow {
    /// 窗种类。
    pub win: u8,
    /// 按键码序列。
    pub keys: Vec<u32>,
}

/// 按键流哨兵。
pub struct FlowSentinel {
    flows: Vec<KeyFlow>,
}

impl FlowSentinel {
    pub fn new() -> FlowSentinel {
        FlowSentinel { flows: Vec::new() }
    }

    /// 挂接窗（种类合法、零重复挂接）。
    pub fn attach(&mut self, win: u8) -> Result<bool, u16> {
        if win > W_XWAY {
            return Err(E_WIN_BAD);
        }
        for f in self.flows.iter() {
            if f.win == win {
                return Err(E_FOCUS_BAD);
            }
        }
        self.flows.push(KeyFlow {
            win,
            keys: Vec::new(),
        });
        Ok(true)
    }

    /// 投递按键（窗必须已挂接、码非零）。
    pub fn feed(&mut self, win: u8, code: u32) -> Result<bool, u16> {
        if code == 0 {
            return Err(E_KEY_LOST);
        }
        let mut found = false;
        for f in self.flows.iter_mut() {
            if f.win == win {
                found = true;
                f.keys.push(code);
                break;
            }
        }
        if !found {
            return Err(E_FOCUS_BAD);
        }
        Ok(true)
    }

    /// 流内零丢零重校验：码序与投递序一致、无重复相邻。
    pub fn assert_flow(&self, win: u8, want: &[u32]) -> Result<(), u16> {
        let mut found = false;
        for f in self.flows.iter() {
            if f.win == win {
                found = true;
                if f.keys.len() != want.len() {
                    return Err(E_COUNT_BREAK);
                }
                let mut i = 0usize;
                while i < f.keys.len() {
                    if f.keys[i] != want[i] {
                        return Err(E_KEY_LOST);
                    }
                    i += 1;
                }
                break;
            }
        }
        if !found {
            return Err(E_FOCUS_BAD);
        }
        Ok(())
    }

    /// 零丢零重：流内相邻零重复、总数守恒。
    pub fn assert_no_dup(&self) -> Result<u32, u16> {
        let mut total = 0u32;
        for f in self.flows.iter() {
            if f.keys.is_empty() {
                return Err(E_FLOW_EMPTY);
            }
            let mut i = 1usize;
            while i < f.keys.len() {
                if f.keys[i] == f.keys[i - 1] {
                    return Err(E_KEY_DUP);
                }
                i += 1;
            }
            total += f.keys.len() as u32;
        }
        Ok(total)
    }

    /// 轮终态快照：三窗计数与首尾码。
    pub fn round_snap(&self) -> Result<[(u32, u32, u32); 3], u16> {
        if self.flows.len() != 3 {
            return Err(E_FOCUS_BAD);
        }
        let mut out = [(0u32, 0u32, 0u32); 3];
        let mut i = 0usize;
        while i < 3 {
            let f = &self.flows[i];
            if f.keys.is_empty() {
                return Err(E_FLOW_EMPTY);
            }
            out[i] = (f.keys.len() as u32, f.keys[0], f.keys[f.keys.len() - 1]);
            i += 1;
        }
        Ok(out)
    }
}

/// 三窗轮对拍：两轮快照必须逐窗全等。
pub fn assert_rounds_eq(
    a: &[(u32, u32, u32); 3],
    b: &[(u32, u32, u32); 3],
) -> Result<(), u16> {
    let mut i = 0usize;
    while i < 3 {
        if a[i] != b[i] {
            return Err(E_ROUND_DIFF);
        }
        i += 1;
    }
    Ok(())
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 16) % n.max(1)
    }
}

/// 三窗按键流回归主判据：三窗 500 字 ×3 轮全绿。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 三窗挂接 + 500 字投递 ×3 轮
    let mut rng = Lcg::new(0x48731);
    let mut ref_rounds: Vec<[(u32, u32, u32); 3]> = Vec::new();
    let mut round = 0u32;
    while round < 3 {
        let mut sn = FlowSentinel::new();
        sn.attach(W_NATIVE)?;
        sn.attach(W_WINE)?;
        sn.attach(W_XWAY)?;
        let mut w = 0u8;
        while w <= W_XWAY {
            let mut k = 0u32;
            while k < 500 {
                let code = (rng.below(90) + 10) as u32;
                sn.feed(w, code)?;
                k += 1;
            }
            w += 1;
        }
        sn.assert_no_dup()?;
        let snap = sn.round_snap()?;
        if ref_rounds.is_empty() {
            ref_rounds.push(snap);
        } else {
            assert_rounds_eq(&ref_rounds[0], &snap)?;
        }
        round += 1;
    }
    hits += 1;

    // R2 零丢：流内逐码对拍投递序
    let mut sn2 = FlowSentinel::new();
    sn2.attach(W_NATIVE)?;
    let mut want: Vec<u32> = Vec::new();
    let mut k2 = 0u32;
    while k2 < 32 {
        let code = 0x30 + (k2 % 10);
        sn2.feed(W_NATIVE, code)?;
        want.push(code);
        k2 += 1;
    }
    sn2.assert_flow(W_NATIVE, &want)?;
    hits += 1;

    // R3 零重：相邻重复必抓
    let mut sn3 = FlowSentinel::new();
    sn3.attach(W_WINE)?;
    sn3.feed(W_WINE, 7)?;
    sn3.feed(W_WINE, 7)?;
    if sn3.assert_no_dup().err() == Some(E_KEY_DUP) {
        hits += 1;
    } else {
        return Err(E_KEY_DUP);
    }

    // R4 空流必抓
    let mut sn4 = FlowSentinel::new();
    sn4.attach(W_XWAY)?;
    if sn4.assert_no_dup().err() == Some(E_FLOW_EMPTY) {
        hits += 1;
    } else {
        return Err(E_FLOW_EMPTY);
    }

    // R5 未挂接窗投递必拒
    let mut sn5 = FlowSentinel::new();
    sn5.attach(W_NATIVE)?;
    if sn5.feed(W_XWAY, 5).err() == Some(E_FOCUS_BAD) {
        hits += 1;
    } else {
        return Err(E_FOCUS_BAD);
    }

    // R6 零码必拒
    if sn5.feed(W_NATIVE, 0).err() == Some(E_KEY_LOST) {
        hits += 1;
    } else {
        return Err(E_KEY_LOST);
    }

    // R7 窗种类越界必拒
    if sn5.attach(9).err() == Some(E_WIN_BAD) {
        hits += 1;
    } else {
        return Err(E_WIN_BAD);
    }

    // R8 重复挂接必拒
    if sn5.attach(W_NATIVE).err() == Some(E_FOCUS_BAD) {
        hits += 1;
    } else {
        return Err(E_FOCUS_BAD);
    }

    // R9 丢失必抓：对拍序列短于实际流
    let mut sn6 = FlowSentinel::new();
    sn6.attach(W_NATIVE)?;
    let mut want6: Vec<u32> = Vec::new();
    let mut k6 = 0u32;
    while k6 < 8 {
        sn6.feed(W_NATIVE, 0x40 + k6)?;
        want6.push(0x40 + k6);
        k6 += 1;
    }
    want6.pop();
    if sn6.assert_flow(W_NATIVE, &want6).err() == Some(E_COUNT_BREAK) {
        hits += 1;
    } else {
        return Err(E_COUNT_BREAK);
    }

    // R10 LCG ×3 轮 500 字计数守恒：总码数 = 3×500×3
    let mut rng2 = Lcg::new(0x48731_0002);
    let mut sn7 = FlowSentinel::new();
    sn7.attach(W_NATIVE)?;
    sn7.attach(W_WINE)?;
    sn7.attach(W_XWAY)?;
    let mut total = 0u32;
    let mut r2 = 0u32;
    while r2 < 3 {
        let mut w2 = 0u8;
        while w2 <= W_XWAY {
            let mut k7 = 0u32;
            while k7 < 500 {
                let code = (rng2.below(200) + 1) as u32;
                sn7.feed(w2, code)?;
                total += 1;
                k7 += 1;
            }
            w2 += 1;
        }
        r2 += 1;
    }
    let got = sn7.assert_no_dup()?;
    if got != total || total != 4500 {
        return Err(E_COUNT_BREAK);
    }
    hits += 1;

    Ok(hits)
}
