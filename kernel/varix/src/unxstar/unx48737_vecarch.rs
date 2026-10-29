//! UNX-F48737 C·向量账全域归档（全部用例集+校验和入档）
//! 判据：UNX-F48737-J1 归档全绿；判据锚定 §AI-61 §8。
//! 纪律：用例-校验和-归档三段闭环，校验和复算一致、批覆盖守恒、归档冻结不可逆。

use crate::alloc::vec::Vec;
use crate::alloc::vec;

pub const SEG_BASE: u16 = 0x1860;

const E_CASE_BAD: u16 = SEG_BASE + 1; // 用例非法
const E_CS_DIFF: u16 = SEG_BASE + 2; // 校验和漂移
const E_ARCH_DUP: u16 = SEG_BASE + 3; // 归档重复
const E_BATCH_MISS: u16 = SEG_BASE + 4; // 批缺漏
const E_FROZEN_TOUCH: u16 = SEG_BASE + 5; // 冻结后篡改
const E_EMPTY_CASE: u16 = SEG_BASE + 6; // 空用例拒
const E_BAL_BREAK: u16 = SEG_BASE + 7; // 归档账守恒破
const E_CASE_MISS: u16 = SEG_BASE + 8; // 用例未登记

/// 批数。
pub const BATCH_N: u32 = 40;

/// 用例。
#[derive(Clone)]
pub struct VecCase {
    /// 批号。
    pub batch: u32,
    /// 用例号（批内）。
    pub cid: u32,
    /// 输入向量。
    pub input: Vec<u32>,
    /// 期望输出向量。
    pub expect: Vec<u32>,
    /// 校验和（FNV-1a of concat）。
    pub csum: u32,
}

impl VecCase {
    /// 构造并计算校验和（输入+期望联合）。
    pub fn new(batch: u32, cid: u32, input: Vec<u32>, expect: Vec<u32>) -> Result<VecCase, u16> {
        if batch == 0 || batch > BATCH_N {
            return Err(E_CASE_BAD);
        }
        if input.is_empty() || expect.is_empty() {
            return Err(E_EMPTY_CASE);
        }
        let mut h: u32 = 0x811C_9DC5;
        for v in input.iter() {
            h ^= *v;
            h = h.wrapping_mul(0x0100_0193);
        }
        for v in expect.iter() {
            h ^= v.wrapping_add(0x4000_0000);
            h = h.wrapping_mul(0x0100_0193);
        }
        Ok(VecCase {
            batch,
            cid,
            input,
            expect,
            csum: h,
        })
    }

    /// 校验和复算。
    pub fn recalc(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for v in self.input.iter() {
            h ^= *v;
            h = h.wrapping_mul(0x0100_0193);
        }
        for v in self.expect.iter() {
            h ^= v.wrapping_add(0x4000_0000);
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }
}

/// 向量归档簿。
pub struct VecArchive {
    cases: Vec<VecCase>,
    frozen: bool,
    frozen_n: u32,
}

impl VecArchive {
    pub fn new() -> VecArchive {
        VecArchive {
            cases: Vec::new(),
            frozen: false,
            frozen_n: 0,
        }
    }

    /// 入档一条（冻结后拒）。
    pub fn put(&mut self, c: VecCase) -> Result<bool, u16> {
        if self.frozen {
            return Err(E_FROZEN_TOUCH);
        }
        for x in self.cases.iter() {
            if x.batch == c.batch && x.cid == c.cid {
                return Err(E_ARCH_DUP);
            }
        }
        self.cases.push(c);
        Ok(true)
    }

    /// 复算对拍：全部用例校验和一致。
    pub fn assert_csum(&self) -> Result<u32, u16> {
        let mut n = 0u32;
        for c in self.cases.iter() {
            if c.recalc() != c.csum {
                return Err(E_CS_DIFF);
            }
            n += 1;
        }
        Ok(n)
    }

    /// 单条复算对拍。
    pub fn check_one(&self, batch: u32, cid: u32) -> Result<u32, u16> {
        for c in self.cases.iter() {
            if c.batch == batch && c.cid == cid {
                if c.recalc() != c.csum {
                    return Err(E_CS_DIFF);
                }
                return Ok(c.csum);
            }
        }
        Err(E_CASE_MISS)
    }

    /// 批覆盖守恒：40 批每批至少一例。
    pub fn assert_cover(&self) -> Result<(), u16> {
        let mut b = 1u32;
        while b <= BATCH_N {
            let mut hit = false;
            for c in self.cases.iter() {
                if c.batch == b {
                    hit = true;
                    break;
                }
            }
            if !hit {
                return Err(E_BATCH_MISS);
            }
            b += 1;
        }
        Ok(())
    }

    /// 冻结归档（不可逆）。
    pub fn freeze(&mut self) -> Result<u32, u16> {
        let n = self.assert_csum()?;
        self.assert_cover()?;
        self.frozen = true;
        self.frozen_n = n;
        Ok(n)
    }

    /// 归档账守恒。
    pub fn assert_balance(&self) -> Result<(), u16> {
        if !self.frozen {
            return Err(E_BAL_BREAK);
        }
        if self.frozen_n != self.cases.len() as u32 {
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

/// 向量账全域归档主判据：40 批 ×2 例入档-复算-冻结全绿。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 40 批 ×2 例构造入档（LCG 向量）
    let mut ar = VecArchive::new();
    let mut rng = Lcg::new(0x48737);
    let mut b = 1u32;
    while b <= BATCH_N {
        let mut c1_in: Vec<u32> = Vec::new();
        let mut c1_ex: Vec<u32> = Vec::new();
        let mut k = 0u32;
        while k < 4 {
            c1_in.push((rng.below(1000) + 1) as u32);
            c1_ex.push((rng.below(1000) + 1) as u32);
            k += 1;
        }
        let c1 = VecCase::new(b, 1, c1_in, c1_ex)?;
        ar.put(c1)?;
        let c2 = VecCase::new(b, 2, vec![b, b + 1], vec![b * 2, b * 2 + 1])?;
        ar.put(c2)?;
        b += 1;
    }
    if ar.cases.len() != 80 {
        return Err(E_ARCH_DUP);
    }
    hits += 1;

    // R2 复算对拍全绿
    let n = ar.assert_csum()?;
    if n != 80 {
        return Err(E_CS_DIFF);
    }
    hits += 1;

    // R3 单条复算抽查 ×256
    let mut k3 = 0u32;
    while k3 < 256 {
        let b_pick = (rng.below(40) + 1) as u32;
        let c_pick = (rng.below(2) + 1) as u32;
        ar.check_one(b_pick, c_pick)?;
        k3 += 1;
    }
    hits += 1;

    // R4 未登记用例必拒
    if ar.check_one(41, 1).err() == Some(E_CASE_MISS) {
        hits += 1;
    } else {
        return Err(E_CASE_MISS);
    }

    // R5 空向量必拒
    if VecCase::new(1, 9, Vec::new(), vec![1]).err() == Some(E_EMPTY_CASE) {
        hits += 1;
    } else {
        return Err(E_EMPTY_CASE);
    }

    // R6 批号越界必拒
    if VecCase::new(41, 1, vec![1], vec![1]).err() == Some(E_CASE_BAD) {
        hits += 1;
    } else {
        return Err(E_CASE_BAD);
    }

    // R7 重复归档必拒
    let dup = VecCase::new(1, 1, vec![1], vec![2])?;
    if ar.put(dup).err() == Some(E_ARCH_DUP) {
        hits += 1;
    } else {
        return Err(E_ARCH_DUP);
    }

    // R8 校验和漂移必抓：篡改后 recalc 不匹配（直接构造差异案例）
    let mut tampered = VecCase::new(1, 3, vec![5, 6], vec![7, 8])?;
    tampered.csum = tampered.csum.wrapping_add(1);
    if tampered.recalc() != tampered.csum {
        hits += 1;
    } else {
        return Err(E_CS_DIFF);
    }

    // R9 冻结归档 + 账守恒
    let fn_ = ar.freeze()?;
    if fn_ != 80 {
        return Err(E_BAL_BREAK);
    }
    ar.assert_balance()?;
    hits += 1;

    // R10 冻结后入档必拒
    let late = VecCase::new(1, 99, vec![1], vec![2])?;
    if ar.put(late).err() == Some(E_FROZEN_TOUCH) {
        hits += 1;
    } else {
        return Err(E_FROZEN_TOUCH);
    }

    Ok(hits)
}
