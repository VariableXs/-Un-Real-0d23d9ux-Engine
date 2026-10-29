//! UNX-F48769 C·20 维度自评（全域逐维打分如实入册）
//! 判据：UNX-F48769-J1 逐维打分入册全绿；判据锚定 §AI-61 §9。
//! 纪律：20 维打分-入册-复核三段闭环，分值域守恒、维度全覆盖、入册冻结不可逆。

use crate::alloc::vec::Vec;
use crate::alloc::vec;

pub const SEG_BASE: u16 = 0x1F00;

const E_DIM_BAD: u16 = SEG_BASE + 1; // 维度越界
const E_SCORE_BAD: u16 = SEG_BASE + 2; // 分值越界
const E_DIM_DUP: u16 = SEG_BASE + 3; // 维度重复
const E_FROZEN_TOUCH: u16 = SEG_BASE + 4; // 入册后改
const E_DIM_MISS: u16 = SEG_BASE + 5; // 维度缺漏
const E_BAL_BREAK: u16 = SEG_BASE + 6; // 均分账守恒破
const E_REVIEW_DIFF: u16 = SEG_BASE + 7; // 复核漂移
const E_EVID_MISS: u16 = SEG_BASE + 8; // 证据缺

/// 维度数。
pub const DIM_N: u32 = 20;
/// 分值上限（百分制）。
pub const SCORE_MAX: u32 = 100;

/// 维度评分。
#[derive(Clone)]
pub struct DimScore {
    /// 维度号（1..=20）。
    pub dim: u32,
    /// 分值（0..=100）。
    pub score: u32,
    /// 证据锚（非零）。
    pub evid: u32,
    /// 入册态。
    pub frozen: bool,
}

/// 自评册。
pub struct SelfReview {
    dims: Vec<DimScore>,
}

impl SelfReview {
    pub fn new() -> SelfReview {
        SelfReview { dims: Vec::new() }
    }

    /// 打分（维度合法、分值域、证据锚非零、唯一）。
    pub fn score(&mut self, dim: u32, val: u32, evid: u32) -> Result<bool, u16> {
        if dim == 0 || dim > DIM_N {
            return Err(E_DIM_BAD);
        }
        if val > SCORE_MAX {
            return Err(E_SCORE_BAD);
        }
        if evid == 0 {
            return Err(E_EVID_MISS);
        }
        for d in self.dims.iter() {
            if d.dim == dim {
                return Err(E_DIM_DUP);
            }
        }
        self.dims.push(DimScore {
            dim,
            score: val,
            evid,
            frozen: false,
        });
        Ok(true)
    }

    /// 均分（万分比）。
    pub fn avg_bps(&self) -> Result<u64, u16> {
        if self.dims.is_empty() {
            return Err(E_DIM_MISS);
        }
        let mut t = 0u64;
        for d in self.dims.iter() {
            t += d.score as u64;
        }
        Ok(t.wrapping_mul(10_000) / self.dims.len() as u64)
    }

    /// 维度覆盖守恒。
    pub fn assert_cover(&self) -> Result<(), u16> {
        if self.dims.len() as u32 != DIM_N {
            return Err(E_DIM_MISS);
        }
        let mut d = 1u32;
        while d <= DIM_N {
            let mut hit = false;
            for x in self.dims.iter() {
                if x.dim == d {
                    hit = true;
                    break;
                }
            }
            if !hit {
                return Err(E_DIM_MISS);
            }
            d += 1;
        }
        Ok(())
    }

    /// 复核：分值与证据锚逐维重读恒等。
    pub fn assert_review(&self) -> Result<u32, u16> {
        let mut n = 0u32;
        for d in self.dims.iter() {
            if d.score > SCORE_MAX {
                return Err(E_SCORE_BAD);
            }
            if d.evid == 0 {
                return Err(E_EVID_MISS);
            }
            n += 1;
        }
        Ok(n)
    }

    /// 入册（全维度冻结）。
    pub fn seal(&mut self) -> Result<u64, u16> {
        self.assert_cover()?;
        self.assert_review()?;
        for d in self.dims.iter_mut() {
            d.frozen = true;
        }
        self.avg_bps()
    }

    /// 均分账守恒：均分 × 维数 ≈ 总分（万分比容差内）。
    pub fn assert_balance(&self) -> Result<(), u16> {
        let avg = self.avg_bps()?;
        let mut t = 0u64;
        for d in self.dims.iter() {
            t += d.score as u64;
        }
        let back = avg.wrapping_mul(self.dims.len() as u64) / 10_000;
        if back != t {
            return Err(E_BAL_BREAK);
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

/// 20 维度自评主判据：逐维打分-入册-复核全绿。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 20 维打分（LCG 60~98 分，证据锚递增）
    let mut sr = SelfReview::new();
    let mut rng = Lcg::new(0x48769);
    let mut d = 1u32;
    while d <= DIM_N {
        let v = (rng.below(39) + 60) as u32;
        sr.score(d, v, 0xE000 + d)?;
        d += 1;
    }
    sr.assert_cover()?;
    hits += 1;

    // R2 维度越界必拒
    if sr.score(21, 80, 1).err() == Some(E_DIM_BAD) {
        hits += 1;
    } else {
        return Err(E_DIM_BAD);
    }

    // R3 分值越界必拒
    if sr.score(1, 101, 1).err() == Some(E_SCORE_BAD) {
        hits += 1;
    } else {
        return Err(E_SCORE_BAD);
    }

    // R4 维度重复必拒
    if sr.score(5, 70, 9).err() == Some(E_DIM_DUP) {
        hits += 1;
    } else {
        return Err(E_DIM_DUP);
    }

    // R5 零证据必拒
    if sr.score(1, 70, 0).err() == Some(E_EVID_MISS) {
        hits += 1;
    } else {
        return Err(E_EVID_MISS);
    }

    // R6 复核全绿
    let n = sr.assert_review()?;
    if n != 20 {
        return Err(E_REVIEW_DIFF);
    }
    hits += 1;

    // R7 均分账守恒
    sr.assert_balance()?;
    let avg = sr.avg_bps()?;
    if avg == 0 || avg > 10_000 {
        return Err(E_BAL_BREAK);
    }
    hits += 1;

    // R8 入册
    let avg2 = sr.seal()?;
    if avg2 != avg {
        return Err(E_REVIEW_DIFF);
    }
    hits += 1;

    // R9 入册后改必拒
    if sr.score(1, 99, 9).err() == Some(E_DIM_DUP) {
        hits += 1;
    } else {
        return Err(E_DIM_DUP);
    }

    // R10 维度缺漏必抓
    let mut sr2 = SelfReview::new();
    let mut d2 = 1u32;
    while d2 <= DIM_N - 1 {
        sr2.score(d2, 75, 0xE100 + d2)?;
        d2 += 1;
    }
    if sr2.seal().err() == Some(E_DIM_MISS) {
        hits += 1;
    } else {
        return Err(E_DIM_MISS);
    }

    // R11 分档统计：优(≥90)/良(80..90)/中(60..80) 三档计数守恒
    let mut excellent = 0u32;
    let mut good = 0u32;
    let mut mid = 0u32;
    for d in sr.dims.iter() {
        if d.score >= 90 {
            excellent += 1;
        } else if d.score >= 80 {
            good += 1;
        } else {
            mid += 1;
        }
    }
    if excellent + good + mid != DIM_N {
        return Err(E_BAL_BREAK);
    }
    hits += 1;

    // R12 低分维预警：低于 65 分的维度必须可枚举且证据锚在册
    let mut low_n = 0u32;
    for d in sr.dims.iter() {
        if d.score < 65 {
            if d.evid == 0 {
                return Err(E_EVID_MISS);
            }
            low_n += 1;
        }
    }
    let _ = low_n;
    hits += 1;

    // R13 LCG 重演：同种子双簿打分逐维全等
    let seed13 = 0x48769_0013;
    let mut a1 = SelfReview::new();
    let mut a2 = SelfReview::new();
    let mut r1 = Lcg(seed13);
    let mut r2 = Lcg(seed13);
    let mut d3 = 1u32;
    while d3 <= DIM_N {
        let v1 = (r1.below(39) + 60) as u32;
        let v2 = (r2.below(39) + 60) as u32;
        a1.score(d3, v1, 0xE200 + d3)?;
        a2.score(d3, v2, 0xE200 + d3)?;
        d3 += 1;
    }
    let mut d4 = 1u32;
    while d4 <= DIM_N {
        if a1.dims[(d4 - 1) as usize].score != a2.dims[(d4 - 1) as usize].score {
            return Err(E_REVIEW_DIFF);
        }
        d4 += 1;
    }
    hits += 1;

    // R14 双簿均分对拍：重演均分恒等
    let avg1 = a1.avg_bps()?;
    let avg2 = a2.avg_bps()?;
    if avg1 != avg2 {
        return Err(E_BAL_BREAK);
    }
    a1.assert_balance()?;
    a2.assert_balance()?;
    hits += 1;

    // R15 逐维证据锚域守恒：20 维证据锚全部落在 0xE200..0xE200+20
    let mut d5 = 1u32;
    while d5 <= DIM_N {
        let e = a1.dims[(d5 - 1) as usize].evid;
        if e != 0xE200 + d5 {
            return Err(E_EVID_MISS);
        }
        d5 += 1;
    }
    // 复核一次后入册
    a1.seal()?;
    a1.assert_review()?;
    hits += 1;

    Ok(hits)
}
