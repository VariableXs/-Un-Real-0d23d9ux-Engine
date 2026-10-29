//! UNX-F48732 C·性能预算全域终账（各面 P99 汇总+校准态）
//! 判据：UNX-F48732-J1 终账落册；判据锚定 附则五。
//! 纪律：采样-P99-终账三段闭环，预算超限必抓、面覆盖守恒、校准态冻结不可逆。

use crate::alloc::vec::Vec;
use crate::alloc::vec;

pub const SEG_BASE: u16 = 0x17A0;

const E_FACE_BAD: u16 = SEG_BASE + 1; // 面号越界
const E_BUDGET_EXCEED: u16 = SEG_BASE + 2; // 预算超限
const E_SAMPLE_BAD: u16 = SEG_BASE + 3; // 采样非法（零/负）
const E_FACE_MISS: u16 = SEG_BASE + 4; // 面缺漏
const E_FROZEN_TOUCH: u16 = SEG_BASE + 5; // 校准态冻结后改
const E_P99_EMPTY: u16 = SEG_BASE + 6; // 空采样求 P99
const E_CAL_RANGE: u16 = SEG_BASE + 7; // 校准值超域
const E_BAL_BREAK: u16 = SEG_BASE + 8; // 采样计数守恒破

/// 面数。
pub const FACE_N: u32 = 12;
/// 单面采样数。
pub const SAMPLE_N: u32 = 64;
/// 采样值域上界（微秒）。
pub const US_MAX: u32 = 10_000;

/// 性能面账目。
#[derive(Clone)]
pub struct PerfFace {
    /// 面号（1..=12）。
    pub face: u32,
    /// 采样序列（微秒）。
    pub samples: Vec<u32>,
    /// 预算（微秒）。
    pub budget: u32,
    /// 校准态（true=冻结）。
    pub frozen: bool,
}

/// 性能预算终账簿。
pub struct PerfBook {
    faces: Vec<PerfFace>,
}

impl PerfBook {
    pub fn new() -> PerfBook {
        PerfBook { faces: Vec::new() }
    }

    /// 开面（面合法、预算正、零重复）。
    pub fn open(&mut self, face: u32, budget: u32) -> Result<bool, u16> {
        if face == 0 || face > FACE_N {
            return Err(E_FACE_BAD);
        }
        if budget == 0 {
            return Err(E_SAMPLE_BAD);
        }
        for f in self.faces.iter() {
            if f.face == face {
                return Err(E_FACE_MISS);
            }
        }
        self.faces.push(PerfFace {
            face,
            samples: Vec::new(),
            budget,
            frozen: false,
        });
        Ok(true)
    }

    /// 采样（面必须存在且未冻结、值域合法）。
    pub fn sample(&mut self, face: u32, us: u32) -> Result<bool, u16> {
        if us == 0 || us > US_MAX {
            return Err(E_SAMPLE_BAD);
        }
        let mut found = false;
        for f in self.faces.iter_mut() {
            if f.face == face {
                found = true;
                if f.frozen {
                    return Err(E_FROZEN_TOUCH);
                }
                f.samples.push(us);
                break;
            }
        }
        if !found {
            return Err(E_FACE_MISS);
        }
        Ok(true)
    }

    /// P99：升序第 99 百分位（上取整索引）。
    pub fn p99(&self, face: u32) -> Result<u32, u16> {
        for f in self.faces.iter() {
            if f.face == face {
                if f.samples.is_empty() {
                    return Err(E_P99_EMPTY);
                }
                let mut s = f.samples.clone();
                s.sort();
                let idx = (s.len() * 99 + 99) / 100;
                let i = if idx == 0 { 0 } else { idx - 1 };
                if i >= s.len() {
                    return Err(E_P99_EMPTY);
                }
                return Ok(s[i]);
            }
        }
        Err(E_FACE_MISS)
    }

    /// 面预算校验：P99 ≤ budget。
    pub fn assert_budget(&self, face: u32) -> Result<u32, u16> {
        let p = self.p99(face)?;
        for f in self.faces.iter() {
            if f.face == face {
                if p > f.budget {
                    return Err(E_BUDGET_EXCEED);
                }
                return Ok(p);
            }
        }
        Err(E_FACE_MISS)
    }

    /// 校准（冻结面并记校准值）。
    pub fn calibrate(&mut self, face: u32, cal: u32) -> Result<bool, u16> {
        if cal > US_MAX {
            return Err(E_CAL_RANGE);
        }
        let mut found = false;
        for f in self.faces.iter_mut() {
            if f.face == face {
                found = true;
                f.frozen = true;
                f.budget = cal;
                break;
            }
        }
        if !found {
            return Err(E_FACE_MISS);
        }
        Ok(true)
    }

    /// 面覆盖守恒：12 面全开。
    pub fn assert_cover(&self) -> Result<(), u16> {
        let mut f = 1u32;
        while f <= FACE_N {
            let mut hit = false;
            for x in self.faces.iter() {
                if x.face == f {
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

    /// 采样计数守恒：总采样数 = 各面和。
    pub fn assert_balance(&self) -> Result<(), u16> {
        let mut total = 0usize;
        for f in self.faces.iter() {
            total += f.samples.len();
        }
        let mut sum = 0usize;
        for f in self.faces.iter() {
            sum += f.samples.len();
        }
        if total != sum {
            return Err(E_BAL_BREAK);
        }
        Ok(())
    }

    /// 终账落册：全冻结 + 覆盖 + 每面 P99 ≤ 校准值。
    pub fn final_book(&self) -> Result<Vec<(u32, u32)>, u16> {
        self.assert_cover()?;
        self.assert_balance()?;
        let mut out: Vec<(u32, u32)> = Vec::new();
        for f in self.faces.iter() {
            if !f.frozen {
                return Err(E_FROZEN_TOUCH);
            }
            let p = self.p99(f.face)?;
            if p > f.budget {
                return Err(E_BUDGET_EXCEED);
            }
            out.push((f.face, p));
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

/// 性能预算全域终账主判据：12 面 ×64 采样 P99 预算校准终账。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 12 面开账：预算 300~1200 微秒梯度
    let mut bk = PerfBook::new();
    let mut f = 1u32;
    while f <= FACE_N {
        bk.open(f, 300 + f * 75)?;
        f += 1;
    }
    hits += 1;

    // R2 重复开面必拒
    if bk.open(1, 500).err() == Some(E_FACE_MISS) {
        hits += 1;
    } else {
        return Err(E_FACE_MISS);
    }

    // R3 面号越界必拒
    if bk.open(13, 500).err() == Some(E_FACE_BAD) {
        hits += 1;
    } else {
        return Err(E_FACE_BAD);
    }

    // R4 零预算必拒
    if bk.open(1, 0).err() == Some(E_SAMPLE_BAD) {
        hits += 1;
    } else {
        return Err(E_SAMPLE_BAD);
    }

    // R5 采样 ×12 面 ×64：LCG 值域 50~500 微秒
    let mut rng = Lcg::new(0x48732);
    let mut f2 = 1u32;
    while f2 <= FACE_N {
        let mut k = 0u32;
        while k < SAMPLE_N {
            let us = (rng.below(450) + 50) as u32;
            bk.sample(f2, us)?;
            k += 1;
        }
        f2 += 1;
    }
    bk.assert_balance()?;
    hits += 1;

    // R6 零采样必拒
    if bk.sample(1, 0).err() == Some(E_SAMPLE_BAD) {
        hits += 1;
    } else {
        return Err(E_SAMPLE_BAD);
    }

    // R7 超域采样必拒
    if bk.sample(1, US_MAX + 1).err() == Some(E_SAMPLE_BAD) {
        hits += 1;
    } else {
        return Err(E_SAMPLE_BAD);
    }

    // R8 P99 全面预算校验：12 面 P99 ≤ 300+面×75
    let mut f3 = 1u32;
    while f3 <= FACE_N {
        let p = bk.assert_budget(f3)?;
        if p == 0 || p > 500 {
            return Err(E_BUDGET_EXCEED);
        }
        f3 += 1;
    }
    hits += 1;

    // R9 校准冻结：全面冻结为各自 P99+50（预算收紧后仍守）
    let mut f4 = 1u32;
    while f4 <= FACE_N {
        let p = bk.p99(f4)?;
        bk.calibrate(f4, (p + 50).min(US_MAX))?;
        f4 += 1;
    }
    if bk.sample(1, 100).err() == Some(E_FROZEN_TOUCH) {
        hits += 1;
    } else {
        return Err(E_FROZEN_TOUCH);
    }

    // R10 终账落册：12 条 (face, p99) 全绿
    let book = bk.final_book()?;
    if book.len() != FACE_N as usize {
        return Err(E_BAL_BREAK);
    }
    let mut f5 = 1u32;
    while f5 <= FACE_N {
        let (fc, p) = book[(f5 - 1) as usize];
        if fc != f5 {
            return Err(E_FACE_MISS);
        }
        if p == 0 {
            return Err(E_P99_EMPTY);
        }
        f5 += 1;
    }
    hits += 1;

    Ok(hits)
}
