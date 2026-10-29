//! UNX-F48738 C·观测埋点全域终检（覆盖率 100% 断言）
//! 判据：UNX-F48738-J1 覆盖率 100%；判据锚定 体验十三·补。
//! 纪律：埋点登记-触发-覆盖率终检三段闭环，未触发埋点必抓、双计数守恒、冻结不可逆。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1880;

const E_POINT_BAD: u16 = SEG_BASE + 1; // 埋点号越界
const E_POINT_DUP: u16 = SEG_BASE + 2; // 埋点重复
const E_COVER_MISS: u16 = SEG_BASE + 3; // 覆盖缺口
const E_FROZEN_TOUCH: u16 = SEG_BASE + 4; // 冻结后触发
const E_FIRE_BAD: u16 = SEG_BASE + 5; // 触发非法
const E_BAL_BREAK: u16 = SEG_BASE + 6; // 双计数守恒破
const E_FACE_MISS: u16 = SEG_BASE + 7; // 面缺漏
const E_TAG_EMPTY: u16 = SEG_BASE + 8; // 埋点标签空

/// 面数。
pub const FACE_N: u32 = 20;
/// 每面埋点数。
pub const PTS_PER_FACE: u32 = 4;

/// 埋点。
#[derive(Clone)]
pub struct ObsPoint {
    /// 面号。
    pub face: u32,
    /// 点号（面内 1..=4）。
    pub pt: u32,
    /// 标签。
    pub tag: String,
    /// 触发计数。
    pub fires: u64,
}

/// 观测埋点终检簿。
pub struct ObsBook {
    points: Vec<ObsPoint>,
    frozen: bool,
}

impl ObsBook {
    pub fn new() -> ObsBook {
        ObsBook {
            points: Vec::new(),
            frozen: false,
        }
    }

    /// 登记埋点（面/点合法、标签非空、唯一）。
    pub fn put(&mut self, face: u32, pt: u32, tag: &str) -> Result<bool, u16> {
        if face == 0 || face > FACE_N {
            return Err(E_FACE_MISS);
        }
        if pt == 0 || pt > PTS_PER_FACE {
            return Err(E_POINT_BAD);
        }
        if tag.is_empty() {
            return Err(E_TAG_EMPTY);
        }
        for p in self.points.iter() {
            if p.face == face && p.pt == pt {
                return Err(E_POINT_DUP);
            }
        }
        self.points.push(ObsPoint {
            face,
            pt,
            tag: tag.to_string(),
            fires: 0,
        });
        Ok(true)
    }

    /// 触发埋点（冻结后拒）。
    pub fn fire(&mut self, face: u32, pt: u32) -> Result<u64, u16> {
        if self.frozen {
            return Err(E_FROZEN_TOUCH);
        }
        let mut found = false;
        let mut n = 0u64;
        for p in self.points.iter_mut() {
            if p.face == face && p.pt == pt {
                found = true;
                p.fires += 1;
                n = p.fires;
                break;
            }
        }
        if !found {
            return Err(E_FIRE_BAD);
        }
        Ok(n)
    }

    /// 覆盖率（万分比）。
    pub fn cover_bps(&self) -> u64 {
        if self.points.is_empty() {
            return 0;
        }
        let mut hit = 0u32;
        for p in self.points.iter() {
            if p.fires > 0 {
                hit += 1;
            }
        }
        (hit as u64).wrapping_mul(10_000) / self.points.len() as u64
    }

    /// 覆盖率 100% 断言。
    pub fn assert_full_cover(&self) -> Result<u32, u16> {
        for p in self.points.iter() {
            if p.fires == 0 {
                return Err(E_COVER_MISS);
            }
        }
        Ok(self.points.len() as u32)
    }

    /// 双计数守恒：总触发数 = 各点触发和。
    pub fn assert_balance(&self, want_total: u64) -> Result<(), u16> {
        let mut total = 0u64;
        for p in self.points.iter() {
            total += p.fires;
        }
        if total != want_total {
            return Err(E_BAL_BREAK);
        }
        Ok(())
    }

    /// 面点覆盖守恒：20 面 ×4 点全登记。
    pub fn assert_grid(&self) -> Result<(), u16> {
        if self.points.len() as u32 != FACE_N * PTS_PER_FACE {
            return Err(E_FACE_MISS);
        }
        let mut f = 1u32;
        while f <= FACE_N {
            let mut pt = 1u32;
            while pt <= PTS_PER_FACE {
                let mut hit = false;
                for p in self.points.iter() {
                    if p.face == f && p.pt == pt {
                        hit = true;
                        break;
                    }
                }
                if !hit {
                    return Err(E_FACE_MISS);
                }
                pt += 1;
            }
            f += 1;
        }
        Ok(())
    }

    /// 终检冻结（不可逆）：全触发 + 网格齐。
    pub fn freeze(&mut self) -> Result<u32, u16> {
        let n = self.assert_full_cover()?;
        self.assert_grid()?;
        self.frozen = true;
        Ok(n)
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

/// 观测埋点全域终检主判据：20 面 ×4 点全触发覆盖率 100% 冻结。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 全网格登记：20 面 ×4 点
    let mut ob = ObsBook::new();
    let mut f = 1u32;
    while f <= FACE_N {
        let mut pt = 1u32;
        while pt <= PTS_PER_FACE {
            ob.put(f, pt, "pt")?;
            pt += 1;
        }
        f += 1;
    }
    ob.assert_grid()?;
    hits += 1;

    // R2 重复埋点必拒
    if ob.put(1, 1, "dup").err() == Some(E_POINT_DUP) {
        hits += 1;
    } else {
        return Err(E_POINT_DUP);
    }

    // R3 点号越界必拒
    if ob.put(1, 5, "over").err() == Some(E_POINT_BAD) {
        hits += 1;
    } else {
        return Err(E_POINT_BAD);
    }

    // R4 面号越界必拒
    if ob.put(21, 1, "over").err() == Some(E_FACE_MISS) {
        hits += 1;
    } else {
        return Err(E_FACE_MISS);
    }

    // R5 空标签必拒
    if ob.put(1, 1, "").err() == Some(E_TAG_EMPTY) {
        hits += 1;
    } else {
        return Err(E_TAG_EMPTY);
    }

    // R6 未全触发先冻结必抓
    if ob.freeze().err() == Some(E_COVER_MISS) {
        hits += 1;
    } else {
        return Err(E_COVER_MISS);
    }

    // R7 全网格触发：80 点各 1 次 + 随机加发
    let mut rng = Lcg::new(0x48738);
    let mut total = 0u64;
    let mut f2 = 1u32;
    while f2 <= FACE_N {
        let mut pt = 1u32;
        while pt <= PTS_PER_FACE {
            ob.fire(f2, pt)?;
            total += 1;
            pt += 1;
        }
        f2 += 1;
    }
    let mut k = 0u32;
    while k < 64 {
        let fpick = (rng.below(20) + 1) as u32;
        let ptpick = (rng.below(4) + 1) as u32;
        ob.fire(fpick, ptpick)?;
        total += 1;
        k += 1;
    }
    ob.assert_balance(total)?;
    hits += 1;

    // R8 覆盖率 100% 断言 + 冻结
    let cov = ob.cover_bps();
    if cov != 10_000 {
        return Err(E_COVER_MISS);
    }
    let n = ob.freeze()?;
    if n != 80 {
        return Err(E_COVER_MISS);
    }
    hits += 1;

    // R9 冻结后触发必拒
    if ob.fire(1, 1).err() == Some(E_FROZEN_TOUCH) {
        hits += 1;
    } else {
        return Err(E_FROZEN_TOUCH);
    }

    // R10 未登记点触发必拒
    let mut ob2 = ObsBook::new();
    ob2.put(1, 1, "solo")?;
    if ob2.fire(1, 2).err() == Some(E_FIRE_BAD) {
        hits += 1;
    } else {
        return Err(E_FIRE_BAD);
    }

    Ok(hits)
}
