//! unx0596_fivestat —— UNX-F0596 参数面五数概括账。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点判据原文（全文抄录）
//! 来源：docs/Varix/CoRun Varix STAR II · Unxreal.md · UNX-A1-B30 批次表：
//!
//!   | UNX-F0596 | 参数面五数概括账 | 200 | 已深化 |
//!   | 五数账齐（min/Q1/med/Q3/max 全键面），与手工基准逐位一致（1/1） |
//!
//! 链路定位：上游 F0589 行长守卫产出了 token 长度分布的原始账，本条
//!   把分布**折叠成五数**（min / Q1 / 中位 / Q3 / max）——参数面的
//!   体检单：一眼看出 token 长度形态（偏长簇 = 参数肥大预警；离群
//!   max = 界缘摩擦预警）。五数用 Tukey 界（Q1-1.5×IQR, Q3+1.5×IQR）
//!   标离群点，离群 token 逐条入预警账。四分位口径冻结为**线性插值
//!   法**（Type 7，与 R/Excel 默认一致——基准可比的前提）。
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 语义冻结（FIVE_SEMANTICS）：
//!   1. 排序：升序，稳定（等值保持入账序——可复现前提）；
//!   2. 分位：Type 7 线性插值 h = (n-1)×p，取 floor/ceil 两邻插值；
//!   3. 离群：Tukey 界外（< Q1-1.5IQR 或 > Q3+1.5IQR）入预警账；
//!   4. 零样本：五数全 0、预警账空（诚实零态，不 NaN 不 panic）。
//!
//! 组件清单：
//!   1. FiveNum           五数结构（min/Q1/med/Q3/max，f64 定容）
//!   2. percentile        Type 7 分位（升序切片 + 插值）
//!   3. five_summary      五数计算（含 IQR/Tukey 界）
//!   4. OutlierEntry      离群预警账目
//!   5. FiveBook          五数账主体：进料（长度序列）→ 五数 → 离群账
//!                        → 与手工基准逐位对拍面 → 报告/指纹
//!   6. fnv64             指纹
//!
//! 纪律：no_std + alloc（f64 经 libm 或 core 允许面——只用四则与
//!   比较，不用 libm 超集）；零 unwrap/expect/panic（测试内 assert!
//!   是断言面）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const FS_VERSION: &str = "unx0596_fivestat-v1";

/// 诊断段基址（0x4C20 段 · 本模块独占 · 择取实录：0x4C00 有 6 处命中弃用）。
pub const FS_SEG_BASE: u16 = 0x4C20;
/// 诊断段顶。
pub const FS_SEG_TOP: u16 = 0x4C2F;

// ═══════════════════════════════════════════════════════════════════════
// 1. percentile —— Type 7 线性插值分位
// ═══════════════════════════════════════════════════════════════════════

/// Type 7 分位：升序样本 `sorted`（非空）在分位 p∈[0,1] 的插值。
///
/// h = (n-1)×p；lo=floor(h)、hi=ceil(h)；结果 = s[lo] + (h-lo)×(s[hi]-s[lo])。
/// 零样本返 0.0（诚实零态）。
pub fn percentile(sorted: &[usize], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let n = sorted.len();
    if n == 1 {
        return sorted[0] as f64;
    }
    let h = (n - 1) as f64 * p;
    let lo = h.floor() as usize;
    let hi = h.ceil() as usize;
    let vlo = sorted[lo.min(n - 1)] as f64;
    let vhi = sorted[hi.min(n - 1)] as f64;
    vlo + (h - h.floor()) * (vhi - vlo)
}

/// 升序稳定排序副本（插入排序——样本量 ≤ 千级，无递归栈险）。
pub fn sorted_copy(data: &[usize]) -> Vec<usize> {
    let mut v = data.to_vec();
    for i in 1..v.len() {
        let key = v[i];
        let mut j = i;
        while j > 0 && v[j - 1] > key {
            v[j] = v[j - 1];
            j -= 1;
        }
        v[j] = key;
    }
    v
}

// ═══════════════════════════════════════════════════════════════════════
// 2. FiveNum / OutlierEntry —— 五数与离群账
// ═══════════════════════════════════════════════════════════════════════

/// 五数概括。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FiveNum {
    /// 最小值。
    pub min: f64,
    /// 下四分位。
    pub q1: f64,
    /// 中位数。
    pub med: f64,
    /// 上四分位。
    pub q3: f64,
    /// 最大值。
    pub max: f64,
}

impl FiveNum {
    /// 四分位距。
    pub fn iqr(&self) -> f64 {
        self.q3 - self.q1
    }

    /// Tukey 下界。
    pub fn lower_fence(&self) -> f64 {
        self.q1 - 1.5 * self.iqr()
    }

    /// Tukey 上界。
    pub fn upper_fence(&self) -> f64 {
        self.q3 + 1.5 * self.iqr()
    }
}

/// 离群预警账目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutlierEntry {
    /// 离群样本值。
    pub val: usize,
    /// 离群侧（true = 上侧，false = 下侧）。
    pub upper: bool,
    /// 越界距离（相对围栏）。
    pub dist: usize,
}

// ═══════════════════════════════════════════════════════════════════════
// 3. FiveBook —— 五数账主体
// ═══════════════════════════════════════════════════════════════════════

/// 五数账。
#[derive(Clone, Debug)]
pub struct FiveBook {
    /// 原始样本（入账序留档）。
    pub samples: Vec<usize>,
    /// 五数。
    pub five: FiveNum,
    /// 离群预警账。
    pub outliers: Vec<OutlierEntry>,
    /// 样本数。
    pub n: usize,
}

impl FiveBook {
    /// 进料计算（零样本 = 全零五数 + 零离群）。
    pub fn feed(data: &[usize]) -> Self {
        if data.is_empty() {
            return FiveBook {
                samples: Vec::new(),
                five: FiveNum { min: 0.0, q1: 0.0, med: 0.0, q3: 0.0, max: 0.0 },
                outliers: Vec::new(),
                n: 0,
            };
        }
        let s = sorted_copy(data);
        let five = FiveNum {
            min: percentile(&s, 0.0),
            q1: percentile(&s, 0.25),
            med: percentile(&s, 0.5),
            q3: percentile(&s, 0.75),
            max: percentile(&s, 1.0),
        };
        let lf = five.lower_fence();
        let uf = five.upper_fence();
        let mut outliers = Vec::new();
        for &v in data {
            let fv = v as f64;
            if fv > uf {
                outliers.push(OutlierEntry {
                    val: v,
                    upper: true,
                    dist: (fv - uf).ceil() as usize,
                });
            } else if fv < lf {
                outliers.push(OutlierEntry {
                    val: v,
                    upper: false,
                    dist: (lf - fv).ceil() as usize,
                });
            }
        }
        FiveBook { samples: data.to_vec(), five, outliers, n: data.len() }
    }

    /// 与手工基准逐位对拍（容差 1e-9）。
    pub fn matches_baseline(&self, b: &FiveNum) -> bool {
        let close = |a: f64, x: f64| (a - x).abs() < 1e-9;
        close(self.five.min, b.min)
            && close(self.five.q1, b.q1)
            && close(self.five.med, b.med)
            && close(self.five.q3, b.q3)
            && close(self.five.max, b.max)
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let f = &self.five;
        let mut s = alloc::format!(
            "五数概括账 · n={} · min={} Q1={} med={} Q3={} max={} IQR={}",
            self.n,
            f.min,
            f.q1,
            f.med,
            f.q3,
            f.max,
            f.iqr()
        );
        if self.outliers.is_empty() {
            s.push_str("\n  离群 0（Tukey 界内）");
        } else {
            for o in &self.outliers {
                s.push_str(&alloc::format!(
                    "\n  离群 {}（{}侧，越 {}）",
                    o.val,
                    if o.upper { "上" } else { "下" },
                    o.dist
                ));
            }
        }
        s
    }

    /// FNV 指纹。
    pub fn fingerprint(&self) -> u64 {
        fnv64(self.report().as_bytes())
    }
}

/// FNV-1a 64 位。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// ═══════════════════════════════════════════════════════════════════════
// 判据面
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::CheckSet;

    /// 判据：与手工基准逐位一致（1/1）。
    ///
    /// 基准批 9 样本 [1,2,3,4,5,6,7,8,100]（Type 7 手算）：
    ///   min=1；Q1: h=(8)×0.25=2 → s[2]=3；med: h=4 → 5；
    ///   Q3: h=6 → 7；max=100。
    ///   IQR=4；Tukey 上界=7+6=13 → 100 离群（越 87）。
    #[test]
    fn matches_manual_baseline() {
        let mut set = CheckSet::new("unxstar-unx0596");

        let data = [1usize, 2, 3, 4, 5, 6, 7, 8, 100];
        let b = FiveBook::feed(&data);
        let base = FiveNum { min: 1.0, q1: 3.0, med: 5.0, q3: 7.0, max: 100.0 };

        set.add("基准·min", (b.five.min - 1.0).abs() < 1e-9, "1");
        set.add("基准·Q1", (b.five.q1 - 3.0).abs() < 1e-9, "3");
        set.add("基准·med", (b.five.med - 5.0).abs() < 1e-9, "5");
        set.add("基准·Q3", (b.five.q3 - 7.0).abs() < 1e-9, "7");
        set.add("基准·max", (b.five.max - 100.0).abs() < 1e-9, "100");
        set.add("基准·逐位对拍 1/1", b.matches_baseline(&base), "1/1");

        // 离群：100 > 上界 13。
        set.add("离群·100 检出", b.outliers.len() == 1, "1");
        set.add(
            "离群·上侧越 87",
            b.outliers.first().map_or(false, |o| o.upper && o.dist == 87),
            "87",
        );
        set.add("IQR·=4", (b.five.iqr() - 4.0).abs() < 1e-9, "4");
        set.add("围栏·上界 13", (b.five.upper_fence() - 13.0).abs() < 1e-9, "13");

        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    /// 判据：偶数样本插值 + 零样本零态 + 排序稳定性。
    #[test]
    fn even_interpolation_and_zero_state() {
        let mut set = CheckSet::new("unxstar-unx0596");

        // 偶数样本 [10,20,30,40]：med = h=1.5 → 25；Q1 = h=0.75 → 17.5；
        // Q3 = h=2.25 → 32.5。
        let b4 = FiveBook::feed(&[40usize, 10, 30, 20]); // 乱序进料
        set.add("偶数·med=25", (b4.five.med - 25.0).abs() < 1e-9, "25");
        set.add("偶数·Q1=17.5", (b4.five.q1 - 17.5).abs() < 1e-9, "17.5");
        set.add("偶数·Q3=32.5", (b4.five.q3 - 32.5).abs() < 1e-9, "32.5");
        set.add("偶数·min=10", (b4.five.min - 10.0).abs() < 1e-9, "10");
        set.add("偶数·max=40", (b4.five.max - 40.0).abs() < 1e-9, "40");

        // 零样本：诚实零态。
        let b0 = FiveBook::feed(&[]);
        set.add("零样本·五数全 0", b0.five == FiveNum { min: 0.0, q1: 0.0, med: 0.0, q3: 0.0, max: 0.0 }, "zeros");
        set.add("零样本·离群 0", b0.outliers.is_empty(), "clean");
        set.add("零样本·n=0", b0.n == 0, "0");

        // 单样本：五数同值。
        let b1 = FiveBook::feed(&[42]);
        set.add(
            "单样本·五数同值",
            (b1.five.min - 42.0).abs() < 1e-9 && (b1.five.max - 42.0).abs() < 1e-9,
            "42×5",
        );

        // 排序稳定：乱序进料与有序进料同结果。
        let ba = FiveBook::feed(&[5, 1, 3]);
        let bb = FiveBook::feed(&[1, 3, 5]);
        set.add(
            "稳定·乱序同果",
            (ba.five.med - bb.five.med).abs() < 1e-9,
            "med equal",
        );

        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    /// 判据：F0589 进料实战（行长分布五数 + 指纹稳定）。
    #[test]
    fn feed_from_len_guard() {
        let mut set = CheckSet::new("unxstar-unx0596");

        // 模拟行长分布：正常簇 10..20 + 离群 200（界缘摩擦预警形态）。
        let mut lens: Vec<usize> = Vec::new();
        for i in 10..20 {
            lens.push(i);
        }
        lens.push(200);
        let b = FiveBook::feed(&lens);

        set.add("实战·n=11", b.n == 11, "11");
        set.add("实战·min=10", (b.five.min - 10.0).abs() < 1e-9, "10");
        set.add("实战·max=200", (b.five.max - 200.0).abs() < 1e-9, "200");
        // 离群检出（200 远超正常簇 Tukey 界）。
        set.add("实战·离群 200 检出", b.outliers.iter().any(|o| o.val == 200), "200");

        // 报告 + 指纹。
        let rep = b.report();
        set.add("报告·含 IQR", rep.contains("IQR"), "iqr");
        set.add("报告·含离群", rep.contains("离群 200"), "outlier");
        let f1 = b.fingerprint();
        let f2 = b.fingerprint();
        set.add("指纹·稳定", f1 == f2, "stable");

        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
