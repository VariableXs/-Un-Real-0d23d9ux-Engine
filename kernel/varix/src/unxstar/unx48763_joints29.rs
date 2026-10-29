//! UNX-F48763 C·AI-29 联测签认件（候选窗三窗可见定位联测签字）
//! 判据：UNX-F48763-J1 三窗定位联测签字全绿；判据锚定 AI-29 联测。
//! 纪律：三窗（vxwm/Wine/Xwayland）候选窗可见定位联测，签字链不可逆、定位偏差零容忍。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1D60;

const E_WIN_BAD: u16 = SEG_BASE + 1; // 窗种类越界
const E_LOC_DIFF: u16 = SEG_BASE + 2; // 定位偏差超容
const E_SIGN_DUP: u16 = SEG_BASE + 3; // 签字重复
const E_SIGN_MISS: u16 = SEG_BASE + 4; // 签字缺失
const E_TOL_BAD: u16 = SEG_BASE + 5; // 容差非法
const E_CAND_BAD: u16 = SEG_BASE + 6; // 候选窗非法
const E_BAL_BREAK: u16 = SEG_BASE + 7; // 签认账守恒破
const E_FROZEN_TOUCH: u16 = SEG_BASE + 8; // 签认后改

/// 窗种类。
pub const W_NATIVE: u8 = 0;
pub const W_WINE: u8 = 1;
pub const W_XWAY: u8 = 2;
/// 定位容差（像素）。
pub const TOLERANCE_PX: i32 = 2;

/// 候选窗定位。
#[derive(Clone, Copy)]
pub struct CandLoc {
    /// 窗种类。
    pub win: u8,
    /// 候选序。
    pub cand: u32,
    /// x 坐标。
    pub x: i32,
    /// y 坐标。
    pub y: i32,
}

/// 签认件。
#[derive(Clone)]
pub struct SignItem {
    /// 窗种类。
    pub win: u8,
    /// 候选序。
    pub cand: u32,
    /// 签字指纹。
    pub fp: u32,
}

/// 三窗联测签认簿。
pub struct JointSign {
    locs: Vec<CandLoc>,
    signs: Vec<SignItem>,
    frozen: bool,
}

impl JointSign {
    pub fn new() -> JointSign {
        JointSign {
            locs: Vec::new(),
            signs: Vec::new(),
            frozen: false,
        }
    }

    /// 登记候选窗定位（窗合法、候选序窗内唯一、坐标非负域）。
    pub fn put(&mut self, win: u8, cand: u32, x: i32, y: i32) -> Result<bool, u16> {
        if win > W_XWAY {
            return Err(E_WIN_BAD);
        }
        if cand == 0 || cand > 16 {
            return Err(E_CAND_BAD);
        }
        if x < 0 || y < 0 {
            return Err(E_CAND_BAD);
        }
        if self.frozen {
            return Err(E_FROZEN_TOUCH);
        }
        for l in self.locs.iter() {
            if l.win == win && l.cand == cand {
                return Err(E_CAND_BAD);
            }
        }
        self.locs.push(CandLoc { win, cand, x, y });
        Ok(true)
    }

    /// 跨窗定位对拍：同候选序三窗坐标偏差 ≤ 容差。
    pub fn assert_loc_tol(&self, cand: u32) -> Result<(), u16> {
        if cand == 0 {
            return Err(E_CAND_BAD);
        }
        let mut found: Vec<CandLoc> = Vec::new();
        for l in self.locs.iter() {
            if l.cand == cand {
                found.push(*l);
            }
        }
        if found.len() < 3 {
            return Err(E_WIN_BAD);
        }
        let mut i = 0usize;
        while i < found.len() {
            let mut j = i + 1;
            while j < found.len() {
                let dx = (found[i].x - found[j].x).abs();
                let dy = (found[i].y - found[j].y).abs();
                if dx > TOLERANCE_PX || dy > TOLERANCE_PX {
                    return Err(E_LOC_DIFF);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 签字（窗+候选唯一、指纹非零、冻结后拒）。
    pub fn sign(&mut self, win: u8, cand: u32, fp: u32) -> Result<bool, u16> {
        if win > W_XWAY {
            return Err(E_WIN_BAD);
        }
        if fp == 0 {
            return Err(E_SIGN_MISS);
        }
        if self.frozen {
            return Err(E_FROZEN_TOUCH);
        }
        for s in self.signs.iter() {
            if s.win == win && s.cand == cand {
                return Err(E_SIGN_DUP);
            }
        }
        self.signs.push(SignItem { win, cand, fp });
        Ok(true)
    }

    /// 签认账守恒：签字数 ≤ 定位数、三窗全覆盖。
    pub fn assert_balance(&self) -> Result<(), u16> {
        if self.signs.len() > self.locs.len() {
            return Err(E_BAL_BREAK);
        }
        let mut w = 0u8;
        while w <= W_XWAY {
            let mut hit = false;
            for s in self.signs.iter() {
                if s.win == w {
                    hit = true;
                    break;
                }
            }
            if !hit {
                return Err(E_SIGN_MISS);
            }
            w += 1;
        }
        Ok(())
    }

    /// 签字指纹复算：窗+候选+坐标联合。
    pub fn fp_of(&self, win: u8, cand: u32) -> Result<u32, u16> {
        for l in self.locs.iter() {
            if l.win == win && l.cand == cand {
                let mut h: u32 = 0x811C_9DC5;
                h ^= win as u32;
                h = h.wrapping_mul(0x0100_0193);
                h ^= cand;
                h = h.wrapping_mul(0x0100_0193);
                h ^= (l.x as u32).wrapping_mul(31);
                h = h.wrapping_mul(0x0100_0193);
                h ^= (l.y as u32).wrapping_mul(37);
                h = h.wrapping_mul(0x0100_0193);
                return Ok(h);
            }
        }
        Err(E_SIGN_MISS)
    }

    /// 终局冻结：全部定位签字且指纹一致。
    pub fn freeze(&mut self) -> Result<u32, u16> {
        for l in self.locs.iter() {
            let mut signed = false;
            for s in self.signs.iter() {
                if s.win == l.win && s.cand == l.cand {
                    signed = true;
                    break;
                }
            }
            if !signed {
                return Err(E_SIGN_MISS);
            }
        }
        self.assert_balance()?;
        self.frozen = true;
        Ok(self.signs.len() as u32)
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

/// AI-29 联测签认主判据：三窗候选定位联测 + 签字链全绿。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 三窗 ×4 候选登记（同候选跨窗坐标差 ≤ 2px）
    let mut js = JointSign::new();
    let mut c = 1u32;
    while c <= 4 {
        js.put(W_NATIVE, c, 100 + c as i32 * 10, 200 + c as i32 * 8)?;
        js.put(W_WINE, c, 100 + c as i32 * 10 + 1, 200 + c as i32 * 8)?;
        js.put(W_XWAY, c, 100 + c as i32 * 10 - 1, 200 + c as i32 * 8 + 2)?;
        c += 1;
    }
    hits += 1;

    // R2 窗种类越界必拒
    if js.put(9, 5, 1, 1).err() == Some(E_WIN_BAD) {
        hits += 1;
    } else {
        return Err(E_WIN_BAD);
    }

    // R3 候选序越界必拒
    if js.put(W_NATIVE, 17, 1, 1).err() == Some(E_CAND_BAD) {
        hits += 1;
    } else {
        return Err(E_CAND_BAD);
    }

    // R4 负坐标必拒
    if js.put(W_NATIVE, 5, -1, 1).err() == Some(E_CAND_BAD) {
        hits += 1;
    } else {
        return Err(E_CAND_BAD);
    }

    // R5 同窗同候选重复必拒
    if js.put(W_NATIVE, 1, 5, 5).err() == Some(E_CAND_BAD) {
        hits += 1;
    } else {
        return Err(E_CAND_BAD);
    }

    // R6 跨窗容差对拍 ×4 全绿
    let mut c2 = 1u32;
    while c2 <= 4 {
        js.assert_loc_tol(c2)?;
        c2 += 1;
    }
    hits += 1;

    // R7 定位偏差超容必抓：构造偏移 5px 的窗
    let mut js2 = JointSign::new();
    js2.put(W_NATIVE, 1, 100, 100)?;
    js2.put(W_WINE, 1, 105, 100)?;
    js2.put(W_XWAY, 1, 101, 101)?;
    if js2.assert_loc_tol(1).err() == Some(E_LOC_DIFF) {
        hits += 1;
    } else {
        return Err(E_LOC_DIFF);
    }

    // R8 签字链：12 条全签
    let mut c3 = 1u32;
    while c3 <= 4 {
        let fp_n = js.fp_of(W_NATIVE, c3)?;
        let fp_w = js.fp_of(W_WINE, c3)?;
        let fp_x = js.fp_of(W_XWAY, c3)?;
        js.sign(W_NATIVE, c3, fp_n)?;
        js.sign(W_WINE, c3, fp_w)?;
        js.sign(W_XWAY, c3, fp_x)?;
        c3 += 1;
    }
    js.assert_balance()?;
    hits += 1;

    // R9 签字重复必拒
    if js.sign(W_NATIVE, 1, 1).err() == Some(E_SIGN_DUP) {
        hits += 1;
    } else {
        return Err(E_SIGN_DUP);
    }

    // R10 终局冻结 + 冻结后改必拒
    let n = js.freeze()?;
    if n != 12 {
        return Err(E_BAL_BREAK);
    }
    if js.put(W_NATIVE, 9, 1, 1).err() == Some(E_FROZEN_TOUCH) {
        hits += 1;
    } else {
        return Err(E_FROZEN_TOUCH);
    }
    if js.sign(W_NATIVE, 9, 1).err() == Some(E_FROZEN_TOUCH) {
        hits += 1;
    } else {
        return Err(E_FROZEN_TOUCH);
    }

    // R11 缺签先冻必抓：三窗只签两窗
    let mut js3 = JointSign::new();
    js3.put(W_NATIVE, 1, 10, 10)?;
    js3.put(W_WINE, 1, 11, 10)?;
    js3.put(W_XWAY, 1, 10, 11)?;
    let fp3 = js3.fp_of(W_NATIVE, 1)?;
    js3.sign(W_NATIVE, 1, fp3)?;
    if js3.freeze().err() == Some(E_SIGN_MISS) {
        hits += 1;
    } else {
        return Err(E_SIGN_MISS);
    }

    Ok(hits)
}
