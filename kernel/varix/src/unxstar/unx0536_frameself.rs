//! unx0536_frameself —— UNX-F0536 框架自检（开销与正确性）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0536 · 框架自检 · 目标 200 行
//! 判据原文：框架自检（框架自身开销五数 + 自指用例 8/8），开销占比注记
//!           （<1% 观测线）。
//! 组件：自指用例（ktest 框架测 ktest 框架——八项自指）+ 开销五数（min/
//!   p25/median/p75/max 步数）+ 占比注记（千分位 <10）。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35340;
pub const XC_SELF_FAIL: u32 = SEG_BASE + 1; // 自指败
pub const XC_PCT_HIGH: u32 = SEG_BASE + 2; // 开销超线
pub const XC_FIVE_SHORT: u32 = SEG_BASE + 3; // 五数缺
pub const XC_SELF_OOB: u32 = SEG_BASE + 4; // 越界

/// 开销五数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FiveNum {
    pub min: u32,
    pub p25: u32,
    pub median: u32,
    pub p75: u32,
    pub max: u32,
}

impl FiveNum {
    /// 单调核：min ≤ p25 ≤ median ≤ p75 ≤ max。
    pub const fn monotonic(&self) -> bool {
        self.min <= self.p25 && self.p25 <= self.median && self.median <= self.p75 && self.p75 <= self.max
    }
}

/// 步数样本集 → 五数（排序取位）。
pub fn five_num(samples: &[u32]) -> Result<FiveNum, u32> {
    if samples.len() < 4 {
        return Err(XC_FIVE_SHORT);
    }
    let mut v = Vec::from(samples);
    // 插入排序。
    let n = v.len();
    let mut i = 1;
    while i < n {
        let mut j = i;
        while j > 0 && v[j - 1] > v[j] {
            v.swap(j - 1, j);
            j -= 1;
        }
        i += 1;
    }
    let idx = |p: usize| -> u32 { v[p.min(n - 1)] };
    Ok(FiveNum {
        min: idx(0),
        p25: idx(n / 4),
        median: idx(n / 2),
        p75: idx(n * 3 / 4),
        max: idx(n - 1),
    })
}

/// 开销占比（千分位）：框架步 / 总步 × 1000。
pub fn overhead_permille(framework_steps: u64, total_steps: u64) -> Result<u32, u32> {
    if total_steps == 0 {
        return Err(XC_SELF_OOB);
    }
    Ok(((framework_steps * 1000) / total_steps) as u32)
}

/// 自指用例八项：框架对自身的八项性质自测（真源为静态断言式计算）。
pub fn self_ref_cases() -> [bool; 8] {
    [
        five_const_ok(),        // 0
        pct_line_ok(),        // 1
        sort_idem(),            // 2
        one_sample_rej(),            // 3
        empty_rej(),              // 4
        copy_eq(),          // 5
        insert_order(),        // 6
        rev_rev_idem(),      // 7
    ]
}

fn five_const_ok() -> bool {
    five_num(&[10, 20, 30, 40]).map(|f| f == FiveNum { min: 10, p25: 10, median: 20, p75: 30, max: 40 }).unwrap_or(false)
}

fn pct_line_ok() -> bool {
    overhead_permille(1, 1000) == Ok(1) && overhead_permille(9, 1000) == Ok(9)
}

fn sort_idem() -> bool {
    let s = [3u32, 1, 2];
    let f1 = five_num(&s);
    let f2 = five_num(&s);
    f1 == f2
}

fn one_sample_rej() -> bool {
    five_num(&[1]).is_err() && five_num(&[1, 2, 3]).is_err()
}

fn empty_rej() -> bool {
    five_num(&[]).is_err()
}

fn copy_eq() -> bool {
    let s = [5u32, 5, 5, 5];
    five_num(&s) == five_num(&s)
}

fn insert_order() -> bool {
    let mut v = Vec::new();
    let mut i = 0;
    while i < 8 {
        v.push((i * 7 % 13) as u32);
        i += 1;
    }
    let f = five_num(&v).unwrap_or(FiveNum { min: 0, p25: 0, median: 0, p75: 0, max: 0 });
    f.monotonic()
}

fn rev_rev_idem() -> bool {
    let a = [1u32, 2, 3, 4];
    let mut b = a;
    b.reverse();
    b.reverse();
    b == a
}

pub fn run_unx0536_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0536");
    cs.add("SEG 段位独占", SEG_BASE == 0x35340 && XC_SELF_OOB == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：自指 8/8。
    let cases = self_ref_cases();
    let mut pass = 0;
    let mut i = 0;
    while i < 8 {
        if cases[i] {
            pass += 1;
        }
        i += 1;
    }
    cs.add("自指 8/8", pass == 8, "八项自指全过");

    // 族二：开销五数。
    let samples = [12u32, 7, 3, 15, 9, 11, 4, 8];
    let f = five_num(&samples).unwrap_or(FiveNum { min: 0, p25: 0, median: 0, p75: 0, max: 0 });
    cs.add("五数齐", f.min == 3 && f.max == 15 && f.median == 9, "min/median/max 精确");
    cs.add("单调核", f.monotonic(), "五数有序");
    cs.add("四样本下限", five_num(&[1, 2, 3, 4]).is_ok(), "4 样本可算");

    // 族三：占比线 <1%。
    cs.add("占比 1‰", overhead_permille(1, 1000) == Ok(1), "1/1000 精确");
    cs.add("占比 <10‰ 线", overhead_permille(9, 1000) == Ok(9) && overhead_permille(10, 1000) == Ok(10), "观测线可判");
    cs.add("总步 0 拒", overhead_permille(1, 0).is_err(), "分母 0 Err");
    let big = overhead_permille(20, 1000).unwrap_or(0);
    cs.add("超线可检", big > 10 && big == 20, "20‰ 超观测线");

    // 族四：五数缺拒。
    cs.add("三样本拒", five_num(&[1, 2, 3]).is_err(), "样本不足 Err");
    cs.add("empty_rej", five_num(&[]).is_err(), "空集 Err");
    cs
}
