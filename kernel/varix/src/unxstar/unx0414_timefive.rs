//! unx0414_timefive —— UNX-F0414 计时五数概括账。
//! 锚点（CoRun Unxreal 主册）：UNX-F0414 · 计时五数概括账 · 目标 200 行
//! 判据原文：UNX-F0414-J1 计时面用五数概括：最小值/下四分/
//!           中位/上四分/最大值（对一段回读序列）；五数有序
//!           核（min ≤ Q1 ≤ med ≤ Q3 ≤ max）；极差 = max - min
//!           与四分位距 IQR = Q3 - Q1 分开报告；离群点判定
//!           （Tukey 界：[Q1 - 1.5·IQR, Q3 + 1.5·IQR] 外为离
//!           群）立案带样本值；五数可复算（同序列同五数）。
//! 组件：样本序列 + 五数计算（排序 + 分位取值）+ 有序核 +
//!       离群点账（Tukey 界）+ 复算核。
//! 职责定位：五数概括是「计时分布的速写」——均值被长尾骗，
//!   五数不会：Q1/Q3 一眼看出离散度，IQR 是稳健尺度（少量
//!   极端值拉不动），Tukey 界的离群判定不用预设分布。计时
//!   面用它回答「回读延迟到底有多散」。
//! 性能逐项分解：排序 O(n²)（选择排序，n 小）；分位 O(1)；
//!   离群 O(n)；复算 O(n²)。
//! 诊断码段：0x21100（0x21101~0x21109 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const FV_VERSION: &str = "unx0414_timefive-v1";

/// 诊断段基址（0x21100 段 · 本模块独占）。
pub const FV_SEG_BASE: u32 = 0x21100;
pub const XC_ORDER_BAD: u32 = 0x21101; // 五数无序
pub const XC_FEW: u32 = 0x21102; // 样本不足（<4）
pub const XC_OUTLIER: u32 = 0x21103; // 离群点存在
pub const XC_RECALC: u32 = 0x21104; // 复算不一致
pub const XC_MAGIC: u32 = 0x21105; // 概括魔数破
pub const XC_NEG_IQR: u32 = 0x21106; // IQR 负值（不可能）
pub const XC_EMPTY: u32 = 0x21107; // 空序列
pub const XC_VAL_OOR: u32 = 0x21108; // 样本越域
pub const XC_EXTREME: u32 = 0x21109; // 极差核破

/// 五数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FiveNum {
    pub min: u64,
    pub q1: u64,
    pub med: u64,
    pub q3: u64,
    pub max: u64,
}

impl FiveNum {
    /// 有序核。
    pub fn ordered(&self) -> Result<(), u32> {
        if !(self.min <= self.q1 && self.q1 <= self.med && self.med <= self.q3 && self.q3 <= self.max)
        {
            return Err(XC_ORDER_BAD);
        }
        Ok(())
    }

    /// 极差。
    pub fn range(&self) -> u64 {
        self.max - self.min
    }

    /// 四分位距。
    pub fn iqr(&self) -> Result<u64, u32> {
        if self.q3 < self.q1 {
            return Err(XC_NEG_IQR);
        }
        Ok(self.q3 - self.q1)
    }
}

/// 排序副本。
fn sorted(seq: &[u64]) -> Vec<u64> {
    let mut v: Vec<u64> = Vec::new();
    for &x in seq.iter() {
        v.push(x);
    }
    for i in 0..v.len() {
        let mut m = i;
        for j in (i + 1)..v.len() {
            if v[j] < v[m] {
                m = j;
            }
        }
        v.swap(i, m);
    }
    v
}

/// 分位取值（线性插值索引模型：取有序数组按比例位置的值，
/// 简化为四分位 = 位置 (n-1)*k/4 向下取整）。
fn quantile(v: &[u64], k: u64) -> u64 {
    let n = v.len() as u64;
    let idx = ((n - 1) * k / 4) as usize;
    v[idx]
}

/// 五数计算。
pub fn five_num(seq: &[u64]) -> Result<FiveNum, u32> {
    if seq.is_empty() {
        return Err(XC_EMPTY);
    }
    if seq.len() < 4 {
        return Err(XC_FEW);
    }
    let v = sorted(seq);
    Ok(FiveNum {
        min: v[0],
        q1: quantile(&v, 1),
        med: quantile(&v, 2),
        q3: quantile(&v, 3),
        max: v[v.len() - 1],
    })
}

/// Tukey 离群判定：返回离群样本值表。
pub fn outliers(seq: &[u64], f: &FiveNum) -> Result<Vec<u64>, u32> {
    let iqr = f.iqr()?;
    let lo = f.q1.saturating_sub(iqr * 3 / 2);
    let hi = f.q3.saturating_add(iqr * 3 / 2);
    let mut out: Vec<u64> = Vec::new();
    for &x in seq.iter() {
        if x < lo || x > hi {
            out.push(x);
        }
    }
    Ok(out)
}

/// 复算核：同序列重算五数一致。
pub fn recalc_ok(seq: &[u64]) -> Result<(), u32> {
    let a = five_num(seq)?;
    let b = five_num(seq)?;
    if a != b {
        return Err(XC_RECALC);
    }
    Ok(())
}

pub fn run_unx0414_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0414_timefive");
    // 族一：标准五数（9 样本均匀 + 一离群）。
    let seq: [u64; 9] = [10, 12, 14, 16, 18, 20, 22, 24, 100];
    let f = match five_num(&seq) {
        Ok(f) => f,
        Err(_) => {
            cs.add("五数计算", false, "拒");
            return cs;
        }
    };
    cs.add("min 10", f.min == 10, "首");
    cs.add("max 100", f.max == 100, "尾");
    cs.add("med 18", f.med == 18, "(8×2/4=4) 位");
    cs.add("q1 14", f.q1 == 14, "(8×1/4=2) 位");
    cs.add("q3 22", f.q3 == 22, "(8×3/4=6) 位");
    cs.add("有序核过", f.ordered().is_ok(), "min ≤ q1 ≤ med ≤ q3 ≤ max");
    cs.add("极差 90", f.range() == 90, "100 - 10");
    cs.add("IQR 8", f.iqr() == Ok(8), "22 - 14");
    // 族二：离群判定（Tukey 界 [14-12, 22+12] = [2, 34]）。
    let out = outliers(&seq, &f);
    cs.add("离群 1 个", out.as_ref().map(|v| v.len()) == Ok(1), "100 越上界");
    cs.add("离群值 100", out == Ok(vec![100u64]), "样本值立案");
    // 无离群版本。
    let seq2: [u64; 9] = [10, 12, 14, 16, 18, 20, 22, 24, 30];
    let f2 = match five_num(&seq2) {
        Ok(x) => x,
        Err(_) => {
            cs.add("五数2", false, "拒");
            return cs;
        }
    };
    cs.add("无离群", outliers(&seq2, &f2) == Ok(Vec::new()), "30 在界内 [2,34]");
    // 族三：复算核。
    cs.add("复算一致", recalc_ok(&seq).is_ok(), "同序列同五数");
    // 族四：参数拒绝。
    cs.add("空序列拒", matches!(five_num(&[]), Err(XC_EMPTY)), "n=0");
    let three: [u64; 3] = [1, 2, 3];
    cs.add("三样本拒", matches!(five_num(&three), Err(XC_FEW)), "n<4");
    let four: [u64; 4] = [1, 2, 3, 4];
    cs.add("四样本过", five_num(&four).is_ok(), "n=4 最小");
    let f4 = five_num(&four);
    cs.add("四样本五数",
        f4 == Ok(FiveNum { min: 1, q1: 1, med: 2, q3: 3, max: 4 }),
        "索引 0/1/2/3");
    // 族五：五数有序违例检出（构造无序五数直接核）。
    let bad = FiveNum { min: 5, q1: 3, med: 4, q3: 2, max: 1 };
    cs.add("无序检出", bad.ordered() == Err(XC_ORDER_BAD), "5 > 3");
    let neg = FiveNum { min: 1, q1: 2, med: 3, q3: 2, max: 4 };
    cs.add("IQR 负检出", neg.iqr() == Err(XC_NEG_IQR), "q3 < q1");
    // 族六：全同值序列。
    let same: [u64; 5] = [7, 7, 7, 7, 7];
    let fs = five_num(&same);
    cs.add("全同五数",
        fs == Ok(FiveNum { min: 7, q1: 7, med: 7, q3: 7, max: 7 }),
        "全 7");
    cs.add("全同 IQR 零", fs.map(|x| x.iqr()) == Ok(Ok(0)), "q3 - q1 = 0");
    cs.add("全同无离群", {
        let ff = match fs {
            Ok(x) => x,
            Err(_) => {
                cs.add("全同失败", false, "破");
                return cs;
            }
        };
        outliers(&same, &ff) == Ok(Vec::new())
    }, "界 [7, 7]");
    cs
}
