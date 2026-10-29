//! UNX-F48730 C·P0 终扫一（全域 P0=0 断言）
//! 判据：UNX-F48730-J1 断言通过；判据锚定 §93。
//! 纪律：登记-清除-终扫三段闭环，面覆盖守恒、P0 残留必抓、终扫断言全域 P0=0。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1760;

const E_FACE_BAD: u16 = SEG_BASE + 1; // 面号越界
const E_ITEM_DUP: u16 = SEG_BASE + 2; // 扫描项重复
const E_P0_LEFT: u16 = SEG_BASE + 3; // P0 残留
const E_CLEAR_BAD: u16 = SEG_BASE + 4; // 清除非本项
const E_FACE_MISS: u16 = SEG_BASE + 5; // 面缺漏
const E_RESULT_BAD: u16 = SEG_BASE + 6; // 扫描结果非法
const E_BAL_BREAK: u16 = SEG_BASE + 7; // 扫描结果守恒破
const E_SWEEP_MISS: u16 = SEG_BASE + 8; // 终扫未执行

/// 面数。
pub const FACE_N: u32 = 40;
/// 结果态。
pub const R_CLEAN: u8 = 0; // 无风险
pub const R_P0: u8 = 1; // P0 风险
pub const R_CLEARED: u8 = 2; // P0 已清除

/// 扫描项。
#[derive(Clone)]
pub struct SweepItem {
    /// 面号（1..=40）。
    pub face: u32,
    /// 面内序号。
    pub seq: u32,
    /// 项名。
    pub name: String,
    /// 结果态。
    pub result: u8,
}

/// P0 终扫器。
pub struct P0Sweep {
    items: Vec<SweepItem>,
    seen: Vec<(u32, u32)>,
    swept: bool,
}

impl P0Sweep {
    pub fn new() -> P0Sweep {
        P0Sweep {
            items: Vec::new(),
            seen: Vec::new(),
            swept: false,
        }
    }

    /// 登记扫描项（面合法、面内名唯一）。
    pub fn put(&mut self, face: u32, seq: u32, name: &str) -> Result<bool, u16> {
        if face == 0 || face > FACE_N {
            return Err(E_FACE_BAD);
        }
        if name.is_empty() {
            return Err(E_FACE_MISS);
        }
        for s in self.seen.iter() {
            if s.0 == face && s.1 == seq {
                return Err(E_ITEM_DUP);
            }
        }
        self.seen.push((face, seq));
        self.items.push(SweepItem {
            face,
            seq,
            name: name.to_string(),
            result: R_CLEAN,
        });
        Ok(true)
    }

    /// 标记 P0 风险。
    pub fn mark_p0(&mut self, face: u32, seq: u32) -> Result<bool, u16> {
        let mut found = false;
        for it in self.items.iter_mut() {
            if it.face == face && it.seq == seq {
                found = true;
                if it.result == R_CLEARED {
                    return Err(E_CLEAR_BAD);
                }
                it.result = R_P0;
                break;
            }
        }
        if !found {
            return Err(E_FACE_MISS);
        }
        Ok(true)
    }

    /// 清除 P0（R_P0 → R_CLEARED）。
    pub fn clear_p0(&mut self, face: u32, seq: u32) -> Result<bool, u16> {
        let mut found = false;
        for it in self.items.iter_mut() {
            if it.face == face && it.seq == seq {
                found = true;
                if it.result != R_P0 {
                    return Err(E_CLEAR_BAD);
                }
                it.result = R_CLEARED;
                break;
            }
        }
        if !found {
            return Err(E_FACE_MISS);
        }
        Ok(true)
    }

    /// 终扫断言：全域 P0=0（无 R_P0 态）。
    pub fn assert_p0_zero(&self) -> Result<u32, u16> {
        for it in self.items.iter() {
            if it.result == R_P0 {
                return Err(E_P0_LEFT);
            }
        }
        Ok(self.items.len() as u32)
    }

    /// 面覆盖守恒：40 面每面至少一项。
    pub fn assert_face_cover(&self) -> Result<(), u16> {
        let mut f = 1u32;
        while f <= FACE_N {
            let mut hit = false;
            for it in self.items.iter() {
                if it.face == f {
                    hit = true;
                    break;
                }
            }
            if !hit {
                return Err(E_FACE_MISS);
            }
            f += 1;
        }
        Ok(())
    }

    /// 结果态守恒：全部态 ∈ {R_CLEAN, R_CLEARED}。
    pub fn assert_results(&self) -> Result<(), u16> {
        for it in self.items.iter() {
            if it.result != R_CLEAN && it.result != R_CLEARED {
                return Err(E_RESULT_BAD);
            }
        }
        Ok(())
    }

    /// 终扫执行（置位 swept）。
    pub fn sweep(&mut self) -> Result<u32, u16> {
        let n = self.assert_p0_zero()?;
        self.assert_face_cover()?;
        self.assert_results()?;
        self.swept = true;
        Ok(n)
    }

    /// 终扫态断言。
    pub fn assert_swept(&self) -> Result<(), u16> {
        if !self.swept {
            return Err(E_SWEEP_MISS);
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

/// P0 终扫主判据：40 面登记-P0 闭环-终扫 P0=0 断言。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 40 面 × 每面 2 项登记
    let mut sw = P0Sweep::new();
    let mut f = 1u32;
    while f <= FACE_N {
        sw.put(f, 5, "alpha")?;
        sw.put(f, 4, "beta")?;
        f += 1;
    }
    if sw.items.len() != 80 {
        return Err(E_ITEM_DUP);
    }
    hits += 1;

    // R2 重复项必拒
    if sw.put(1, 5, "alpha").err() == Some(E_ITEM_DUP) {
        hits += 1;
    } else {
        return Err(E_ITEM_DUP);
    }

    // R3 面号越界必拒
    if sw.put(41, 1, "over").err() == Some(E_FACE_BAD) {
        hits += 1;
    } else {
        return Err(E_FACE_BAD);
    }

    // R4 未清除先终扫必抓：标记 P0 不清除断言应红
    sw.mark_p0(3, 5)?;
    if sw.assert_p0_zero().err() == Some(E_P0_LEFT) {
        hits += 1;
    } else {
        return Err(E_P0_LEFT);
    }

    // R5 非 P0 态清除必拒
    if sw.clear_p0(1, 5).err() == Some(E_CLEAR_BAD) {
        hits += 1;
    } else {
        return Err(E_CLEAR_BAD);
    }

    // R6 清除闭环：P0 → CLEARED
    sw.clear_p0(3, 5)?;
    let n = sw.assert_p0_zero()?;
    if n != 80 {
        return Err(E_BAL_BREAK);
    }
    hits += 1;

    // R7 全域终扫通过 + 终扫态断言
    let swept_n = sw.sweep()?;
    if swept_n != 80 {
        return Err(E_FACE_MISS);
    }
    sw.assert_swept()?;
    hits += 1;

    // R8 面覆盖守恒：删一面（不登记）终扫必抓
    let mut sw2 = P0Sweep::new();
    let mut f2 = 1u32;
    while f2 <= FACE_N - 1 {
        sw2.put(f2, 3, "gamma")?;
        f2 += 1;
    }
    if sw2.sweep().err() == Some(E_FACE_MISS) {
        hits += 1;
    } else {
        return Err(E_FACE_MISS);
    }

    // R9 LCG 随机 P0 流 ×256：全清除终扫恒绿
    let mut rng = Lcg::new(0x48730);
    let mut k = 0u32;
    while k < 256 {
        let mut run = P0Sweep::new();
        let m = (rng.below(6) + 1) as u32;
        let mut i = 0u32;
        while i < m {
            run.put(1, (i + 1) as u32, "rngitem")?;
            if rng.below(2) == 1 {
                run.mark_p0(1, (i + 1) as u32)?;
            }
            i += 1;
        }
        for it in run.items.clone().iter() {
            if it.result == R_P0 {
                let seq = it.name.len() as u32;
                run.clear_p0(1, seq)?;
            }
        }
        run.assert_p0_zero()?;
        k += 1;
    }
    hits += 1;

    // R10 fuzz ×128：越界面与重复项全部被防御吸收
    let mut rng2 = Lcg::new(0x48730_0002);
    let mut swf = P0Sweep::new();
    let mut blocked = 0u32;
    let mut k2 = 0u32;
    while k2 < 128 {
        let pick = rng2.below(3);
        let face = (rng2.below(45)) as u32;
        if pick == 0 && (face == 0 || face > FACE_N) {
            if swf.put(face, 1, "x").is_err() {
                blocked += 1;
            }
        } else if pick == 1 {
            if swf.put(face.min(FACE_N).max(1), 1, "x").is_err() {
                blocked += 1;
            }
        } else {
            let _ = swf.put(face.min(FACE_N).max(1), (k2 % 5 + 2) as u32, "y");
        }
        k2 += 1;
    }
    if blocked < 5 {
        return Err(E_FACE_BAD);
    }
    swf.assert_results()?;
    hits += 1;

    Ok(hits)
}
