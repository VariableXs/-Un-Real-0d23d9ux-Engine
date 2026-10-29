//! UNX-F48771 C39 三类窗口闭账回归哨兵（三类窗口闭账回归·宪法加严）
//! 判据：UNX-F48771-J1 三窗闭账回归全绿；判据锚定 域内宪法加严项。
//! 纪律：三窗（vxwm/Wine/Xwayland）闭账回归哨兵，账面终值双路对拍、轮恒等、残账零容忍。

use crate::alloc::vec::Vec;
use crate::alloc::vec;

pub const SEG_BASE: u16 = 0x1F60;

const E_WIN_BAD: u16 = SEG_BASE + 1; // 窗越界
const E_LEDGER_BAD: u16 = SEG_BASE + 2; // 账目非法
const E_CLOSE_DIFF: u16 = SEG_BASE + 3; // 闭账不一致
const E_WIN_MISS: u16 = SEG_BASE + 4; // 窗缺漏
const E_RESIDUE: u16 = SEG_BASE + 5; // 残账
const E_ROUND_DIFF: u16 = SEG_BASE + 6; // 轮间漂移
const E_DUP_ENTRY: u16 = SEG_BASE + 7; // 账目重复
const E_COUNT_BREAK: u16 = SEG_BASE + 8; // 计数守恒破

/// 窗种类。
pub const W_NATIVE: u8 = 0;
pub const W_WINE: u8 = 1;
pub const W_XWAY: u8 = 2;

/// 闭账条目。
#[derive(Clone)]
pub struct CloseEntry {
    /// 窗种类。
    pub win: u8,
    /// 账键。
    pub key: u32,
    /// 闭账值（可为负语义：i64 包装）。
    pub val: i64,
}

/// 闭账哨兵。
pub struct CloseSentinel {
    entries: Vec<CloseEntry>,
    seen: Vec<(u8, u32)>,
}

impl CloseSentinel {
    pub fn new() -> CloseSentinel {
        CloseSentinel {
            entries: Vec::new(),
            seen: Vec::new(),
        }
    }

    /// 登记闭账（窗合法、键窗内唯一、值域有限）。
    pub fn put(&mut self, win: u8, key: u32, val: i64) -> Result<bool, u16> {
        if win > W_XWAY {
            return Err(E_WIN_BAD);
        }
        if key == 0 {
            return Err(E_LEDGER_BAD);
        }
        if val < -1_000_000 || val > 1_000_000 {
            return Err(E_LEDGER_BAD);
        }
        for s in self.seen.iter() {
            if s.0 == win && s.1 == key {
                return Err(E_DUP_ENTRY);
            }
        }
        self.seen.push((win, key));
        self.entries.push(CloseEntry { win, key, val });
        Ok(true)
    }

    /// 窗闭账合计。
    pub fn sum_win(&self, win: u8) -> Result<i64, u16> {
        if win > W_XWAY {
            return Err(E_WIN_BAD);
        }
        let mut t = 0i64;
        let mut hit = false;
        for e in self.entries.iter() {
            if e.win == win {
                hit = true;
                t += e.val;
            }
        }
        if !hit {
            return Err(E_WIN_MISS);
        }
        Ok(t)
    }

    /// 闭账断言：三窗合计全零。
    pub fn assert_all_zero(&self) -> Result<u32, u16> {
        let mut w = 0u8;
        let mut n = 0u32;
        while w <= W_XWAY {
            let v = self.sum_win(w)?;
            if v != 0 {
                return Err(E_RESIDUE);
            }
            n += 1;
            w += 1;
        }
        Ok(n)
    }

    /// 双路对拍：正序和 vs 逆序和。
    pub fn assert_dual(&self, win: u8) -> Result<i64, u16> {
        let a = self.sum_win(win)?;
        let mut b = 0i64;
        let mut i = self.entries.len();
        while i > 0 {
            i -= 1;
            if self.entries[i].win == win {
                b += self.entries[i].val;
            }
        }
        if a != b {
            return Err(E_CLOSE_DIFF);
        }
        Ok(a)
    }

    /// 窗覆盖守恒。
    pub fn assert_cover(&self) -> Result<(), u16> {
        let mut w = 0u8;
        while w <= W_XWAY {
            let mut hit = false;
            for e in self.entries.iter() {
                if e.win == w {
                    hit = true;
                    break;
                }
            }
            if !hit {
                return Err(E_WIN_MISS);
            }
            w += 1;
        }
        Ok(())
    }

    /// 计数守恒。
    pub fn assert_count(&self) -> Result<(), u16> {
        let mut t = 0u32;
        let mut w = 0u8;
        while w <= W_XWAY {
            let mut c = 0u32;
            for e in self.entries.iter() {
                if e.win == w {
                    c += 1;
                }
            }
            t += c;
            w += 1;
        }
        if t != self.entries.len() as u32 {
            return Err(E_COUNT_BREAK);
        }
        Ok(())
    }
}

/// 轮快照：三窗闭账合计。
pub type RoundSnap = [i64; 3];

/// 轮快照对拍。
pub fn assert_rounds_eq(a: &RoundSnap, b: &RoundSnap) -> Result<(), u16> {
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

/// 三窗闭账回归主判据：三窗成对清账-双路对拍-轮恒等全绿。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 三窗 ×8 对成对清账
    let mut cs = CloseSentinel::new();
    let mut c = 1u32;
    while c <= 8 {
        cs.put(W_NATIVE, c, (100 + c as i64) * 3)?;
        cs.put(W_NATIVE, c + 100, -((100 + c as i64) * 3))?;
        cs.put(W_WINE, c, (200 + c as i64) * 3)?;
        cs.put(W_WINE, c + 100, -((200 + c as i64) * 3))?;
        cs.put(W_XWAY, c, (300 + c as i64) * 3)?;
        cs.put(W_XWAY, c + 100, -((300 + c as i64) * 3))?;
        c += 1;
    }
    cs.assert_cover()?;
    cs.assert_count()?;
    hits += 1;

    // R2 窗越界必拒
    if cs.put(9, 1, 1).err() == Some(E_WIN_BAD) {
        hits += 1;
    } else {
        return Err(E_WIN_BAD);
    }

    // R3 零键必拒
    if cs.put(W_NATIVE, 0, 1).err() == Some(E_LEDGER_BAD) {
        hits += 1;
    } else {
        return Err(E_LEDGER_BAD);
    }

    // R4 值域越界必拒
    if cs.put(W_NATIVE, 500, 2_000_000).err() == Some(E_LEDGER_BAD) {
        hits += 1;
    } else {
        return Err(E_LEDGER_BAD);
    }

    // R5 重复键必拒
    if cs.put(W_NATIVE, 1, 5).err() == Some(E_DUP_ENTRY) {
        hits += 1;
    } else {
        return Err(E_DUP_ENTRY);
    }

    // R6 闭账断言 + 双路对拍
    let n = cs.assert_all_zero()?;
    if n != 3 {
        return Err(E_RESIDUE);
    }
    let mut w = 0u8;
    while w <= W_XWAY {
        cs.assert_dual(w)?;
        w += 1;
    }
    hits += 1;

    // R7 轮快照恒等：三窗闭账合计快照 ×5 轮
    let base: RoundSnap = [
        cs.sum_win(W_NATIVE)?,
        cs.sum_win(W_WINE)?,
        cs.sum_win(W_XWAY)?,
    ];
    let mut r = 0u32;
    while r < 5 {
        let snap: RoundSnap = [
            cs.sum_win(W_NATIVE)?,
            cs.sum_win(W_WINE)?,
            cs.sum_win(W_XWAY)?,
        ];
        assert_rounds_eq(&base, &snap)?;
        r += 1;
    }
    hits += 1;

    // R8 残账必抓
    let mut cs2 = CloseSentinel::new();
    cs2.put(W_NATIVE, 1, 42)?;
    cs2.put(W_NATIVE, 2, -40)?;
    if cs2.assert_all_zero().err() == Some(E_RESIDUE) {
        hits += 1;
    } else {
        return Err(E_RESIDUE);
    }

    // R9 窗缺漏必抓
    let mut cs3 = CloseSentinel::new();
    cs3.put(W_WINE, 1, 5)?;
    cs3.put(W_WINE, 2, -5)?;
    if cs3.assert_all_zero().err() == Some(E_WIN_MISS) {
        hits += 1;
    } else {
        return Err(E_WIN_MISS);
    }

    // R10 LCG 大账 ×3 窗 ×64 对：闭账恒零 + 轮恒等
    let mut rng = Lcg::new(0x48771);
    let mut w2 = 0u8;
    while w2 <= W_XWAY {
        let mut bk = CloseSentinel::new();
        let mut c2 = 1u32;
        while c2 <= 64 {
            let v = (rng.below(10_000) + 1) as i64;
            bk.put(w2, c2, v)?;
            bk.put(w2, c2 + 1000, -v)?;
            c2 += 1;
        }
        let fin = bk.assert_dual(w2)?;
        if fin != 0 {
            return Err(E_CLOSE_DIFF);
        }
        w2 += 1;
    }
    hits += 1;

    // R11 混合符号账：正负混排双路对拍恒等
    let mut cs4 = CloseSentinel::new();
    cs4.put(W_NATIVE, 1, 500)?;
    cs4.put(W_NATIVE, 2, -300)?;
    cs4.put(W_NATIVE, 3, -200)?;
    cs4.put(W_WINE, 1, -700)?;
    cs4.put(W_WINE, 2, 700)?;
    cs4.put(W_XWAY, 1, 0)?;
    cs4.put(W_XWAY, 2, 0)?;
    let v_n = cs4.assert_dual(W_NATIVE)?;
    if v_n != 0 {
        return Err(E_CLOSE_DIFF);
    }
    let v_w = cs4.assert_dual(W_WINE)?;
    if v_w != 0 {
        return Err(E_CLOSE_DIFF);
    }
    let v_x = cs4.assert_dual(W_XWAY)?;
    if v_x != 0 {
        return Err(E_CLOSE_DIFF);
    }
    hits += 1;

    // R12 轮快照漂移必抓：构造不同合计快照
    let mut cs5 = CloseSentinel::new();
    cs5.put(W_NATIVE, 1, 10)?;
    cs5.put(W_NATIVE, 2, -10)?;
    cs5.put(W_WINE, 1, 20)?;
    cs5.put(W_WINE, 2, -20)?;
    cs5.put(W_XWAY, 1, 30)?;
    cs5.put(W_XWAY, 2, -30)?;
    let snap_a: RoundSnap = [0, 0, 0];
    let snap_b: RoundSnap = [0, 0, 1];
    if assert_rounds_eq(&snap_a, &snap_b).err() == Some(E_ROUND_DIFF) {
        hits += 1;
    } else {
        return Err(E_ROUND_DIFF);
    }

    Ok(hits)
}
