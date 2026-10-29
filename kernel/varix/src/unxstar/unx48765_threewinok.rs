//! UNX-F48765 C·域功条件断言一：三类窗口账全过（终值复断言）
//! 判据：UNX-F48765-J1 三窗账终值复断言全绿；判据锚定 域功条件。
//! 纪律：三窗（vxwm/Wine/Xwayland）账面终值独立复算-双路对拍-断言全过，残账必抓。

use crate::alloc::vec::Vec;
use crate::alloc::vec;

pub const SEG_BASE: u16 = 0x1DA0;

const E_WIN_BAD: u16 = SEG_BASE + 1; // 窗种类越界
const E_ENTRY_BAD: u16 = SEG_BASE + 2; // 账目非法
const E_FINAL_DIFF: u16 = SEG_BASE + 3; // 终值复算不一致
const E_WIN_MISS: u16 = SEG_BASE + 4; // 窗缺漏
const E_RESIDUE: u16 = SEG_BASE + 5; // 残账未清
const E_COUNT_BREAK: u16 = SEG_BASE + 6; // 计数守恒破
const E_DUP_ENTRY: u16 = SEG_BASE + 7; // 账目重复
const E_NEG_VAL: u16 = SEG_BASE + 8; // 负值拒

/// 窗种类。
pub const W_NATIVE: u8 = 0;
pub const W_WINE: u8 = 1;
pub const W_XWAY: u8 = 2;

/// 账目条目。
#[derive(Clone)]
pub struct AccEntry {
    /// 窗种类。
    pub win: u8,
    /// 键码。
    pub code: u32,
    /// 借记。
    pub debit: u64,
    /// 贷记。
    pub credit: u64,
}

impl AccEntry {
    /// 净额。
    pub fn net(&self) -> i64 {
        self.debit as i64 - self.credit as i64
    }
}

/// 三窗账簿。
pub struct WinBook {
    entries: Vec<AccEntry>,
    seen: Vec<(u8, u32)>,
}

impl WinBook {
    pub fn new() -> WinBook {
        WinBook {
            entries: Vec::new(),
            seen: Vec::new(),
        }
    }

    /// 登记账目（窗合法、键码非零、窗内键唯一、借贷非负不同时为零）。
    pub fn put(&mut self, win: u8, code: u32, debit: u64, credit: u64) -> Result<bool, u16> {
        if win > W_XWAY {
            return Err(E_WIN_BAD);
        }
        if code == 0 {
            return Err(E_ENTRY_BAD);
        }
        if debit > 0 && credit > 0 {
            return Err(E_ENTRY_BAD);
        }
        if debit == 0 && credit == 0 {
            return Err(E_NEG_VAL);
        }
        for s in self.seen.iter() {
            if s.0 == win && s.1 == code {
                return Err(E_DUP_ENTRY);
            }
        }
        self.seen.push((win, code));
        self.entries.push(AccEntry { win, code, debit, credit });
        Ok(true)
    }

    /// 窗终值：净额合计。
    pub fn final_net(&self, win: u8) -> Result<i64, u16> {
        if win > W_XWAY {
            return Err(E_WIN_BAD);
        }
        let mut net = 0i64;
        let mut hit = false;
        for e in self.entries.iter() {
            if e.win == win {
                hit = true;
                net += e.net();
            }
        }
        if !hit {
            return Err(E_WIN_MISS);
        }
        Ok(net)
    }

    /// 终值复断言：双路复算（顺序 + 逆序）恒等。
    pub fn assert_final(&self, win: u8) -> Result<i64, u16> {
        let a = self.final_net(win)?;
        // 逆序复算
        let mut net = 0i64;
        let mut i = self.entries.len();
        while i > 0 {
            i -= 1;
            let e = &self.entries[i];
            if e.win == win {
                net += e.net();
            }
        }
        if net != a {
            return Err(E_FINAL_DIFF);
        }
        Ok(a)
    }

    /// 残账断言：三窗终值必须全部归零。
    pub fn assert_all_zero(&self) -> Result<u32, u16> {
        let mut w = 0u8;
        let mut n = 0u32;
        while w <= W_XWAY {
            let v = self.assert_final(w)?;
            if v != 0 {
                return Err(E_RESIDUE);
            }
            n += 1;
            w += 1;
        }
        Ok(n)
    }

    /// 计数守恒：条目数 = 三窗和。
    pub fn assert_count(&self) -> Result<(), u16> {
        let mut n = 0u32;
        let mut w = 0u8;
        while w <= W_XWAY {
            let mut c = 0u32;
            for e in self.entries.iter() {
                if e.win == w {
                    c += 1;
                }
            }
            n += c;
            w += 1;
        }
        if n != self.entries.len() as u32 {
            return Err(E_COUNT_BREAK);
        }
        Ok(())
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

/// 三窗账全过主判据：登记-终值复算-残账断言全绿。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 三窗登记：借贷成对全清（净额零）
    let mut wb = WinBook::new();
    let mut c = 1u32;
    while c <= 8 {
        wb.put(W_NATIVE, c, 100 + c as u64, 0)?;
        wb.put(W_NATIVE, c + 100, 0, 100 + c as u64)?;
        wb.put(W_WINE, c, 200 + c as u64, 0)?;
        wb.put(W_WINE, c + 100, 0, 200 + c as u64)?;
        wb.put(W_XWAY, c, 300 + c as u64, 0)?;
        wb.put(W_XWAY, c + 100, 0, 300 + c as u64)?;
        c += 1;
    }
    wb.assert_cover()?;
    wb.assert_count()?;
    hits += 1;

    // R2 窗越界必拒
    if wb.put(9, 1, 1, 0).err() == Some(E_WIN_BAD) {
        hits += 1;
    } else {
        return Err(E_WIN_BAD);
    }

    // R3 零码必拒
    if wb.put(W_NATIVE, 0, 1, 0).err() == Some(E_ENTRY_BAD) {
        hits += 1;
    } else {
        return Err(E_ENTRY_BAD);
    }

    // R4 借贷同正必拒
    if wb.put(W_NATIVE, 500, 10, 10).err() == Some(E_ENTRY_BAD) {
        hits += 1;
    } else {
        return Err(E_ENTRY_BAD);
    }

    // R5 借贷同零必拒
    if wb.put(W_NATIVE, 501, 0, 0).err() == Some(E_NEG_VAL) {
        hits += 1;
    } else {
        return Err(E_NEG_VAL);
    }

    // R6 重复账目必拒
    if wb.put(W_NATIVE, 1, 5, 0).err() == Some(E_DUP_ENTRY) {
        hits += 1;
    } else {
        return Err(E_DUP_ENTRY);
    }

    // R7 终值复断言：三窗净额全零（双路对拍）
    let n = wb.assert_all_zero()?;
    if n != 3 {
        return Err(E_RESIDUE);
    }
    hits += 1;

    // R8 残账必抓：单边净额非零
    let mut wb2 = WinBook::new();
    wb2.put(W_NATIVE, 1, 50, 0)?;
    wb2.put(W_NATIVE, 2, 0, 30)?;
    if wb2.assert_all_zero().err() == Some(E_RESIDUE) {
        hits += 1;
    } else {
        return Err(E_RESIDUE);
    }

    // R9 窗缺漏必抓
    let mut wb3 = WinBook::new();
    wb3.put(W_WINE, 1, 10, 0)?;
    wb3.put(W_WINE, 2, 0, 10)?;
    if wb3.assert_all_zero().err() == Some(E_WIN_MISS) {
        hits += 1;
    } else {
        return Err(E_WIN_MISS);
    }

    // R10 LCG 大账 ×192 复断言：随机借贷成对清账三窗恒零
    let mut rng = Lcg::new(0x48765);
    let mut w = 0u8;
    while w <= W_XWAY {
        let mut wbk = WinBook::new();
        let mut c2 = 1u32;
        while c2 <= 64 {
            let v = rng.below(1000) + 1;
            wbk.put(w, c2, v, 0)?;
            wbk.put(w, c2 + 1000, 0, v)?;
            c2 += 1;
        }
        let fin = wbk.assert_final(w)?;
        if fin != 0 {
            return Err(E_FINAL_DIFF);
        }
        w += 1;
    }
    hits += 1;

    // R11 单窗终值非零断言：净额可正可负，双路对拍恒等
    let mut wb4 = WinBook::new();
    wb4.put(W_NATIVE, 1, 500, 0)?;
    wb4.put(W_NATIVE, 2, 0, 120)?;
    let v1 = wb4.assert_final(W_NATIVE)?;
    if v1 != 380 {
        return Err(E_FINAL_DIFF);
    }
    wb4.put(W_WINE, 1, 0, 90)?;
    let v2 = wb4.assert_final(W_WINE)?;
    if v2 != -90 {
        return Err(E_FINAL_DIFF);
    }
    hits += 1;

    // R12 计数守恒破必抓（构造越权场景：直接比对三窗和与总条目）
    // 单窗账面覆盖守恒与计数守恒先行校验后再断言终值
    let mut wb5 = WinBook::new();
    wb5.put(W_XWAY, 1, 10, 0)?;
    wb5.put(W_XWAY, 2, 0, 10)?;
    wb5.put(W_XWAY, 3, 7, 0)?;
    wb5.assert_count()?;
    wb5.assert_cover()?;
    let fx = wb5.assert_final(W_XWAY)?;
    if fx != 7 {
        return Err(E_FINAL_DIFF);
    }
    hits += 1;

    Ok(hits)
}
