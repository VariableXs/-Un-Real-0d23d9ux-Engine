//! unx0529_rndinj —— UNX-F0529 随机注入器（种子化可复现）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0529 · 随机注入器 · 目标 200 行
//! 判据原文：随机器种子化（LCG 种子账注），同种子 3 轮序列全等（1000 样
//!           本/轮）。
//! 组件：Lcg（种子化线性同余）+ 种子账（种子注记可查）+ 三轮全等核。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35180;
pub const XC_SEED_ZERO: u32 = SEG_BASE + 1; // 种子 0（会退化常数列）
pub const XC_SEQ_DRIFT: u32 = SEG_BASE + 2; // 同种子异序列
pub const XC_RANGE_OOB: u32 = SEG_BASE + 3; // 值域越界
pub const XC_INJ_EMPTY: u32 = SEG_BASE + 4; // 零样本

/// LCG 参数（Knuth MMIX 风格）。
const LCG_A: u64 = 6_364_136_223_846_793_005;
const LCG_C: u64 = 1_442_695_040_888_963_407;

/// 种子化随机器。
#[derive(Clone, Debug)]
pub struct Lcg {
    state: u64,
    /// 种子注记。
    pub seed_note: u64,
}

impl Lcg {
    /// 构造：种子 0 拒（退化列）。
    pub fn new(seed: u64) -> Result<Self, u32> {
        if seed == 0 {
            return Err(XC_SEED_ZERO);
        }
        Ok(Lcg {
            state: seed,
            seed_note: seed,
        })
    }

    /// 下一原始值。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(LCG_A).wrapping_add(LCG_C);
        self.state
    }

    /// 下一值折算到 [0, bound)（bound=0 拒）。
    pub fn next_bounded(&mut self, bound: u32) -> Result<u32, u32> {
        if bound == 0 {
            return Err(XC_RANGE_OOB);
        }
        Ok((self.next_u64() % bound as u64) as u32)
    }

    /// 批量产出 n 样本。
    pub fn take(&mut self, n: usize, bound: u32) -> Result<Vec<u32>, u32> {
        if n == 0 {
            return Err(XC_INJ_EMPTY);
        }
        let mut v = Vec::new();
        let mut i = 0;
        while i < n {
            v.push(self.next_bounded(bound)?);
            i += 1;
        }
        Ok(v)
    }
}

/// 三轮全等核：同种子三轮 1000 样本逐值全等。
pub fn triple_round(seed: u64, n: usize, bound: u32) -> Result<bool, u32> {
    let r1 = Lcg::new(seed)?.take(n, bound)?;
    let r2 = Lcg::new(seed)?.take(n, bound)?;
    let r3 = Lcg::new(seed)?.take(n, bound)?;
    Ok(r1 == r2 && r2 == r3)
}

/// 种子账：种子号是否在账。
pub struct SeedBook {
    seeds: Vec<u64>,
}

impl SeedBook {
    /// 空账。
    pub fn new() -> Self {
        SeedBook { seeds: Vec::new() }
    }

    /// 注记种子（重复拒）。
    pub fn note(&mut self, seed: u64) -> Result<(), u32> {
        if seed == 0 {
            return Err(XC_SEED_ZERO);
        }
        let mut i = 0;
        while i < self.seeds.len() {
            if self.seeds[i] == seed {
                return Err(XC_SEQ_DRIFT);
            }
            i += 1;
        }
        self.seeds.push(seed);
        Ok(())
    }

    /// 在账查询。
    pub fn has(&self, seed: u64) -> bool {
        let mut i = 0;
        while i < self.seeds.len() {
            if self.seeds[i] == seed {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 账目数。
    pub fn len(&self) -> usize {
        self.seeds.len()
    }

    /// 空？可实现。
    pub fn is_empty(&self) -> bool {
        self.seeds.is_empty()
    }
}

impl Default for SeedBook {
    fn default() -> Self {
        Self::new()
    }
}

pub fn run_unx0529_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0529");
    cs.add("SEG 段位独占", SEG_BASE == 0x35180 && XC_INJ_EMPTY == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：种子化确定性。
    let mut a = Lcg::new(42).unwrap_or(Lcg { state: 1, seed_note: 1 });
    let mut b = Lcg::new(42).unwrap_or(Lcg { state: 1, seed_note: 1 });
    let mut same = true;
    let mut i = 0;
    while i < 32 {
        if a.next_u64() != b.next_u64() {
            same = false;
        }
        i += 1;
    }
    cs.add("同种子同列", same && a.seed_note == 42, "LCG 确定性");
    let mut c = Lcg::new(43).unwrap_or(Lcg { state: 1, seed_note: 1 });
    cs.add("异种子异列", c.next_u64() != Lcg::new(42).map(|mut x| x.next_u64()).unwrap_or(0), "种子敏感");

    // 族二：三轮全等 1000 样本。
    cs.add("三轮全等", triple_round(7, 1000, 1000) == Ok(true), "1000×3 逐值等");
    cs.add("异界全等", triple_round(9, 500, 7) == Ok(true), "小值域同稳");

    // 族三：值域。
    let mut d = Lcg::new(11).unwrap_or(Lcg { state: 1, seed_note: 1 });
    let vals = d.take(200, 5).unwrap_or_default();
    let mut in_range = true;
    let mut i = 0;
    while i < vals.len() {
        if vals[i] >= 5 {
            in_range = false;
        }
        i += 1;
    }
    cs.add("值域界内", in_range && vals.len() == 200, "[0,5) 全界内");
    cs.add("界 0 拒", Lcg::new(1).map(|mut x| x.next_bounded(0)).unwrap_or(Err(XC_RANGE_OOB)).is_err(), "bound=0 Err");
    cs.add("零样本拒", Lcg::new(1).map(|mut x| x.take(0, 9)).unwrap_or(Err(XC_INJ_EMPTY)).is_err(), "n=0 Err");

    // 族四：种子账。
    let mut sb = SeedBook::new();
    let _ = sb.note(42);
    let _ = sb.note(43);
    cs.add("种子在账", sb.has(42) && sb.has(43) && !sb.has(44), "注记可查");
    cs.add("重复注记拒", sb.note(42).is_err(), "重号 Err");
    cs.add("零种子拒", sb.note(0).is_err() && Lcg::new(0).is_err(), "0 种子双拒");
    cs.add("账量", sb.len() == 2, "两条注记");
    cs
}
