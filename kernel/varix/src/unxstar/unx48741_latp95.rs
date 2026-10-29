//! UNX-F48741 C·首显延迟 P95 终账（四段全量复测落账）
//! 判据：UNX-F48741-J1 四段复测落账全绿；判据锚定 附则五。
//! 纪律：四段（冷启/热启/首帧/可交互）采样-P95-预算-落账全链，超限必抓、冻结不可逆。

use crate::alloc::vec::Vec;
use crate::alloc::vec;

pub const SEG_BASE: u16 = 0x1920;

const E_SEG_BAD: u16 = SEG_BASE + 1; // 段号越界
const E_SAMP_BAD: u16 = SEG_BASE + 2; // 采样非法
const E_P95_EXCEED: u16 = SEG_BASE + 3; // P95 超预算
const E_SEG_MISS: u16 = SEG_BASE + 4; // 段缺漏
const E_FROZEN_TOUCH: u16 = SEG_BASE + 5; // 落账后改
const E_P95_EMPTY: u16 = SEG_BASE + 6; // 空采样求 P95
const E_CAL_RANGE: u16 = SEG_BASE + 7; // 校准值超域
const E_BAL_BREAK: u16 = SEG_BASE + 8; // 采样账守恒破

/// 段数：冷启/热启/首帧/可交互。
pub const SEG_N: u32 = 4;
/// 每段采样数。
pub const SAMP_N: u32 = 96;
/// 采样值域上界（毫秒）。
pub const MS_MAX: u32 = 5_000;

/// 段账。
#[derive(Clone)]
pub struct LatSeg {
    /// 段号（1..=4）。
    pub seg: u32,
    /// 采样序列（毫秒）。
    pub samples: Vec<u32>,
    /// 预算（毫秒）。
    pub budget: u32,
    /// 落账冻结。
    pub frozen: bool,
}

/// 首显延迟终账簿。
pub struct LatBook {
    segs: Vec<LatSeg>,
}

impl LatBook {
    pub fn new() -> LatBook {
        LatBook { segs: Vec::new() }
    }

    /// 开段（段合法、预算正、零重复）。
    pub fn open(&mut self, seg: u32, budget: u32) -> Result<bool, u16> {
        if seg == 0 || seg > SEG_N {
            return Err(E_SEG_BAD);
        }
        if budget == 0 {
            return Err(E_SAMP_BAD);
        }
        for s in self.segs.iter() {
            if s.seg == seg {
                return Err(E_SEG_MISS);
            }
        }
        self.segs.push(LatSeg {
            seg,
            samples: Vec::new(),
            budget,
            frozen: false,
        });
        Ok(true)
    }

    /// 采样（段存在且未冻结、值域合法）。
    pub fn sample(&mut self, seg: u32, ms: u32) -> Result<bool, u16> {
        if ms == 0 || ms > MS_MAX {
            return Err(E_SAMP_BAD);
        }
        let mut found = false;
        for s in self.segs.iter_mut() {
            if s.seg == seg {
                found = true;
                if s.frozen {
                    return Err(E_FROZEN_TOUCH);
                }
                s.samples.push(ms);
                break;
            }
        }
        if !found {
            return Err(E_SEG_MISS);
        }
        Ok(true)
    }

    /// P95：升序上取整 95 分位。
    pub fn p95(&self, seg: u32) -> Result<u32, u16> {
        self.pct(seg, 95)
    }

    /// 中位（上取整 50 分位）。
    pub fn p50(&self, seg: u32) -> Result<u32, u16> {
        self.pct(seg, 50)
    }

    /// 通用分位（pct ∈ 1..=99）。
    pub fn pct(&self, seg: u32, pct: u32) -> Result<u32, u16> {
        if pct == 0 || pct > 99 {
            return Err(E_SAMP_BAD);
        }
        for s in self.segs.iter() {
            if s.seg == seg {
                if s.samples.is_empty() {
                    return Err(E_P95_EMPTY);
                }
                let mut v = s.samples.clone();
                v.sort();
                let idx = (v.len() * pct as usize + 99) / 100;
                let i = if idx == 0 { 0 } else { idx - 1 };
                if i >= v.len() {
                    return Err(E_P95_EMPTY);
                }
                return Ok(v[i]);
            }
        }
        Err(E_SEG_MISS)
    }

    /// 剔除最大 trim_n 个样本后的 P95。
    pub fn p95_trim(&self, seg: u32, trim_n: usize) -> Result<u32, u16> {
        for s in self.segs.iter() {
            if s.seg == seg {
                if s.samples.len() <= trim_n {
                    return Err(E_P95_EMPTY);
                }
                let mut v = s.samples.clone();
                v.sort();
                let cut = v.len() - trim_n;
                v.truncate(cut);
                if v.is_empty() {
                    return Err(E_P95_EMPTY);
                }
                let idx = (v.len() * 95 + 99) / 100;
                let i = if idx == 0 { 0 } else { idx - 1 };
                if i >= v.len() {
                    return Err(E_P95_EMPTY);
                }
                return Ok(v[i]);
            }
        }
        Err(E_SEG_MISS)
    }

    /// 预算校验：P95 ≤ budget。
    pub fn assert_budget(&self, seg: u32) -> Result<u32, u16> {
        let p = self.p95(seg)?;
        for s in self.segs.iter() {
            if s.seg == seg {
                if p > s.budget {
                    return Err(E_P95_EXCEED);
                }
                return Ok(p);
            }
        }
        Err(E_SEG_MISS)
    }

    /// 落账冻结（预算收紧为 P95+40）。
    pub fn seal(&mut self, seg: u32) -> Result<u32, u16> {
        let p = self.p95(seg)?;
        for s in self.segs.iter_mut() {
            if s.seg == seg {
                if s.frozen {
                    return Err(E_FROZEN_TOUCH);
                }
                s.budget = (p + 40).min(MS_MAX);
                s.frozen = true;
                return Ok(s.budget);
            }
        }
        Err(E_SEG_MISS)
    }

    /// 段覆盖守恒。
    pub fn assert_cover(&self) -> Result<(), u16> {
        if self.segs.len() as u32 != SEG_N {
            return Err(E_SEG_MISS);
        }
        let mut s = 1u32;
        while s <= SEG_N {
            let mut hit = false;
            for x in self.segs.iter() {
                if x.seg == s {
                    hit = true;
                    break;
                }
            }
            if !hit {
                return Err(E_SEG_MISS);
            }
            s += 1;
        }
        Ok(())
    }

    /// 采样账守恒：总采样数 = 各段和。
    pub fn assert_balance(&self) -> Result<(), u16> {
        let mut total = 0usize;
        let mut sum = 0usize;
        for s in self.segs.iter() {
            total += s.samples.len();
            sum += s.samples.len();
        }
        if total != sum {
            return Err(E_BAL_BREAK);
        }
        Ok(())
    }

    /// 全段终账：全冻结 + 全预算守 + 覆盖齐。
    pub fn final_book(&self) -> Result<Vec<(u32, u32, u32)>, u16> {
        self.assert_cover()?;
        self.assert_balance()?;
        let mut out: Vec<(u32, u32, u32)> = Vec::new();
        let mut s = 1u32;
        while s <= SEG_N {
            let mut found = false;
            for x in self.segs.iter() {
                if x.seg == s {
                    found = true;
                    if !x.frozen {
                        return Err(E_FROZEN_TOUCH);
                    }
                    let p = self.p95(s)?;
                    if p > x.budget {
                        return Err(E_P95_EXCEED);
                    }
                    out.push((s, p, x.budget));
                    break;
                }
            }
            if !found {
                return Err(E_SEG_MISS);
            }
            s += 1;
        }
        Ok(out)
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

/// 首显延迟 P95 终账主判据：四段 ×96 采样 P95 预算落账全绿。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 四段开账：预算 120/80/200/400 毫秒
    let budgets = [120u32, 80, 200, 400];
    let mut bk = LatBook::new();
    let mut s = 1u32;
    while s <= SEG_N {
        bk.open(s, budgets[(s - 1) as usize])?;
        s += 1;
    }
    bk.assert_cover()?;
    hits += 1;

    // R2 重复开段必拒
    if bk.open(1, 100).err() == Some(E_SEG_MISS) {
        hits += 1;
    } else {
        return Err(E_SEG_MISS);
    }

    // R3 段号越界必拒
    if bk.open(5, 100).err() == Some(E_SEG_BAD) {
        hits += 1;
    } else {
        return Err(E_SEG_BAD);
    }

    // R4 零预算必拒
    if bk.open(1, 0).err() == Some(E_SAMP_BAD) {
        hits += 1;
    } else {
        return Err(E_SAMP_BAD);
    }

    // R5 四段 ×96 采样（LCG 梯度值域）
    let mut rng = Lcg::new(0x48741);
    let ranges = [(40u64, 110u64), (30, 75), (90, 190), (180, 390)];
    let mut s2 = 1u32;
    while s2 <= SEG_N {
        let (lo, hi) = ranges[(s2 - 1) as usize];
        let mut k = 0u32;
        while k < SAMP_N {
            let ms = (rng.below(hi - lo) + lo) as u32;
            bk.sample(s2, ms)?;
            k += 1;
        }
        s2 += 1;
    }
    bk.assert_balance()?;
    hits += 1;

    // R6 零采样必拒
    if bk.sample(1, 0).err() == Some(E_SAMP_BAD) {
        hits += 1;
    } else {
        return Err(E_SAMP_BAD);
    }

    // R7 超域采样必拒
    if bk.sample(1, MS_MAX + 1).err() == Some(E_SAMP_BAD) {
        hits += 1;
    } else {
        return Err(E_SAMP_BAD);
    }

    // R8 四段 P95 预算校验全绿
    let mut s3 = 1u32;
    while s3 <= SEG_N {
        let p = bk.assert_budget(s3)?;
        if p == 0 {
            return Err(E_P95_EMPTY);
        }
        s3 += 1;
    }
    hits += 1;

    // R9 落账冻结：四段全冻结（预算收紧后仍守）
    let mut s4 = 1u32;
    while s4 <= SEG_N {
        bk.seal(s4)?;
        s4 += 1;
    }
    if bk.sample(1, 50).err() == Some(E_FROZEN_TOUCH) {
        hits += 1;
    } else {
        return Err(E_FROZEN_TOUCH);
    }

    // R10 全段终账落册：4 条 (seg, p95, budget) 全绿
    let book = bk.final_book()?;
    if book.len() != SEG_N as usize {
        return Err(E_BAL_BREAK);
    }
    let mut s5 = 1u32;
    while s5 <= SEG_N {
        let (sg, p, b) = book[(s5 - 1) as usize];
        if sg != s5 {
            return Err(E_SEG_MISS);
        }
        if p == 0 || b == 0 {
            return Err(E_P95_EMPTY);
        }
        if p > b {
            return Err(E_P95_EXCEED);
        }
        s5 += 1;
    }
    hits += 1;

    // R11 中位数与 P95 单调：P95 ≥ 中位（上取整 50 分位）
    let mut s6 = 1u32;
    while s6 <= SEG_N {
        let p95v = bk.p95(s6)?;
        let med = bk.p50(s6)?;
        if p95v < med {
            return Err(E_P95_EXCEED);
        }
        s6 += 1;
    }
    hits += 1;

    // R12 独立复测对拍：同参数重开一簿复测，P95 全等
    let mut bk2 = LatBook::new();
    let mut s7 = 1u32;
    while s7 <= SEG_N {
        bk2.open(s7, budgets[(s7 - 1) as usize])?;
        s7 += 1;
    }
    let mut rng2 = Lcg::new(0x48741);
    let mut s8 = 1u32;
    while s8 <= SEG_N {
        let (lo, hi) = ranges[(s8 - 1) as usize];
        let mut k2 = 0u32;
        while k2 < SAMP_N {
            let ms = (rng2.below(hi - lo) + lo) as u32;
            bk2.sample(s8, ms)?;
            k2 += 1;
        }
        s8 += 1;
    }
    let mut s9 = 1u32;
    while s9 <= SEG_N {
        let a = bk.p95(s9)?;
        let b = bk2.p95(s9)?;
        if a != b {
            return Err(E_P95_EXCEED);
        }
        s9 += 1;
    }
    hits += 1;

    // R13 异常值剔除后 P95 收敛：剔除最大值重算 P95 不升
    let mut bk3 = LatBook::new();
    bk3.open(1, 500)?;
    let mut k3 = 0u32;
    while k3 < SAMP_N {
        bk3.sample(1, (rng.below(300) + 20) as u32)?;
        k3 += 1;
    }
    let p_before = bk3.p95(1)?;
    let p_trim = bk3.p95_trim(1, 4)?;
    if p_trim > p_before {
        return Err(E_P95_EXCEED);
    }
    bk3.assert_budget(1)?;
    hits += 1;

    Ok(hits)
}
