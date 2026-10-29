//! UNX-F56175 syscall 延迟均值-方差趋势观察件（0x615E0 段）
//!
//! 锚点：UNX-F56175-J1 趋势观察账（syscall 延迟均值-方差逐窗观察：
//! 窗号 × 均值 × 方差 × 趋势向四元组落账）；J2 统计语义（整数域均值
//! = sum/n、方差 = Σ(x-μ)²/n 截断、趋势向 = 均值环比万分变化率）；
//! J3 趋势哨兵（连续两窗同向变化 ≥ 10% 判趋势确立留痕）三判据全绿。
//! 域：CoRun·UNX-F2-B40（syscall 域·趋势观察面）。
//!
//! 结构：窗口账（逐窗统计落账）+ 统计核（整数域均值/方差/变化率）+
//! 趋势哨兵（连续同向判定）+ 趋势印 + 注入核（空窗统计/变化率漂移/
//! 哨兵漏报/账后改/空账印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x615E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x615E0;
pub const TR_EMPTY_WIN: u32 = SEG_BASE + 0x01; // 空窗统计
pub const TR_RATE_DRIFT: u32 = SEG_BASE + 0x02; // 变化率漂移
pub const TR_TREND_MISS: u32 = SEG_BASE + 0x03; // 哨兵漏报
pub const TR_SEALED_WRITE: u32 = SEG_BASE + 0x04; // 印后改
pub const TR_EMPTY_SEAL: u32 = SEG_BASE + 0x05; // 空账印

/// 趋势确立阈（均值环比万分变化 ≥ 1000 = 10%）。
pub const TREND_AT: u32 = 1_000;

/// 整数域均值：sum/n（n=0 → 0 哨兵）。
pub fn mean_of(sum: u64, n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    sum / n
}

/// 整数域方差：Σ(x-μ)²/n 截断（μ 为整数均值）。
pub fn variance_of(samples: &[u64]) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    let sum: u64 = samples.iter().sum();
    let mu = sum / samples.len() as u64;
    let sq: u128 = samples
        .iter()
        .map(|x| {
            let d = if *x > mu { *x - mu } else { mu - *x };
            (d as u128) * (d as u128)
        })
        .sum();
    (sq / samples.len() as u128) as u64
}

/// 环比变化率：|new-old| × 10000 / old（old=0 → 0 哨兵）。
pub fn rate_of(old: u64, new: u64) -> u32 {
    if old == 0 {
        return 0;
    }
    let d = if new > old { new - old } else { old - new };
    ((d as u128 * 10_000) / old as u128) as u32
}

/// 趋势向：+1 升 / -1 降 / 0 平（变化 < 10% 视为平）。
pub fn trend_dir(old: u64, new: u64) -> i8 {
    if rate_of(old, new) < TREND_AT {
        return 0;
    }
    if new > old {
        1
    } else {
        -1
    }
}

/// 单窗账。
#[derive(Clone)]
pub struct TrendWindow {
    pub win_no: u32,
    pub mean: u64,
    pub variance: u64,
    pub dir: i8,
}

impl TrendWindow {
    fn render_line(&self) -> String {
        format!(
            "w{}|mean{}|var{}|dir{}",
            self.win_no, self.mean, self.variance, self.dir
        )
    }
}

/// 趋势观察账。
pub struct TrendBook {
    windows: Vec<TrendWindow>,
    sealed: bool,
}

impl TrendBook {
    pub fn new() -> Self {
        TrendBook {
            windows: Vec::new(),
            sealed: false,
        }
    }

    /// 落一窗：空窗拒 + 印后拒 + 趋势向计算。
    pub fn observe(&mut self, samples: &[u64]) -> Result<TrendWindow, u32> {
        if self.sealed {
            return Err(TR_SEALED_WRITE);
        }
        if samples.is_empty() {
            return Err(TR_EMPTY_WIN);
        }
        let sum: u64 = samples.iter().sum();
        let mean = mean_of(sum, samples.len() as u64);
        let var = variance_of(samples);
        let dir = match self.windows.last() {
            None => 0,
            Some(prev) => trend_dir(prev.mean, mean),
        };
        let win = TrendWindow {
            win_no: self.windows.len() as u32 + 1,
            mean,
            variance: var,
            dir,
        };
        self.windows.push(win.clone());
        Ok(win)
    }

    /// 趋势哨兵：连续两窗同向非零。
    pub fn trend_established(&self) -> bool {
        let w = &self.windows;
        w.len() >= 3
            && w[w.len() - 1].dir != 0
            && w[w.len() - 1].dir == w[w.len() - 2].dir
    }

    pub fn len(&self) -> usize {
        self.windows.len()
    }

    pub fn window(&self, i: usize) -> Option<&TrendWindow> {
        self.windows.get(i)
    }

    pub fn seal(&mut self) -> bool {
        if self.sealed || self.windows.len() < 2 {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render_book(&self) -> String {
        let mut out = String::new();
        for w in &self.windows {
            out.push_str(&w.render_line());
            out.push('\n');
        }
        if self.trend_established() {
            out.push_str("TREND-HIT\n");
        }
        if self.sealed {
            out.push_str("TREND-SEAL\n");
        }
        out
    }
}

impl Default for TrendBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：四窗升势（mean 500→560→630→700，每窗 ≥ 10%）。
fn standard_book() -> TrendBook {
    let mut b = TrendBook::new();
    let _ = b.observe(&[500; 10]);
    let _ = b.observe(&[560; 10]);
    let _ = b.observe(&[630; 10]);
    let _ = b.observe(&[700; 10]);
    b
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：空窗
    let mut b = TrendBook::new();
    if b.observe(&[]).err() == Some(TR_EMPTY_WIN) {
        caught += 1;
    }
    // 注 2：变化率哨兵核（+12% 判升）
    if trend_dir(500, 560) == 1 && trend_dir(500, 505) == 0 {
        caught += 1;
    }
    // 注 3：哨兵漏报（升势账必须 trend_established）
    if standard_book().trend_established() {
        caught += 1;
    }
    // 注 4：印后改
    let mut b = standard_book();
    let _ = b.seal();
    if b.observe(&[100]).err() == Some(TR_SEALED_WRITE) {
        caught += 1;
    }
    // 注 5：空账印
    let mut b = TrendBook::new();
    if !b.seal() && !b.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 趋势账；J2 统计语义；J3 趋势哨兵。
pub fn run_unx56175_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56175");
    // J1-1 四窗全落账
    let b = standard_book();
    let ok = b.len() == 4;
    s.add("56175-J1-四窗全落账", ok, if ok { "" } else { "趋势账不全" });
    // J1-2 渲染导出齐（TREND-HIT）
    let text = standard_book().render_book();
    let ok = text.contains("w1|mean500|var0|dir0") && text.contains("TREND-HIT");
    s.add("56175-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 均值核（500）
    let ok = b.window(0).map(|w| w.mean == 500).unwrap_or(false);
    s.add("56175-J2-均值核", ok, if ok { "" } else { "均值漂移" });
    // J2-2 方差核（常数样本方差 0）
    let ok = b.window(0).map(|w| w.variance == 0).unwrap_or(false);
    s.add("56175-J2-常数方差", ok, if ok { "" } else { "方差漂移" });
    // J2-3 非常数方差核（[90,110] 方差 = 100）
    let ok = variance_of(&[90, 110]) == 100;
    s.add("56175-J2-方差锚", ok, if ok { "" } else { "方差算术漂移" });
    // J2-4 变化率锚（500→560 = 1200）
    let ok = rate_of(500, 560) == 1_200;
    s.add("56175-J2-变化率锚", ok, if ok { "" } else { "变化率漂移" });
    // J2-5 零基守卫
    let ok = rate_of(0, 100) == 0;
    s.add("56175-J2-零基守卫", ok, if ok { "" } else { "除零穿透" });
    // J2-6 均值哨兵
    let ok = mean_of(0, 0) == 0;
    s.add("56175-J2-均值哨兵", ok, if ok { "" } else { "均值穿透" });
    // J3-1 趋势确立（升势）
    let ok = standard_book().trend_established();
    s.add("56175-J3-趋势确立", ok, if ok { "" } else { "哨兵漏报" });
    // J3-2 降势确立
    let mut b2 = TrendBook::new();
    let _ = b2.observe(&[700; 10]);
    let _ = b2.observe(&[630; 10]);
    let _ = b2.observe(&[560; 10]);
    let ok = b2.trend_established();
    s.add("56175-J3-降势确立", ok, if ok { "" } else { "降势漏报" });
    // J3-3 无趋势（交替）
    let mut b3 = TrendBook::new();
    let _ = b3.observe(&[500; 10]);
    let _ = b3.observe(&[560; 10]);
    let _ = b3.observe(&[500; 10]);
    let ok = !b3.trend_established();
    s.add("56175-J3-交替无势", ok, if ok { "" } else { "哨兵误报" });
    // J3-4 趋势印（两窗即可印）
    let mut b4 = standard_book();
    let ok = b4.seal() && b4.sealed();
    s.add("56175-J3-趋势印", ok, if ok { "" } else { "趋势印失败" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56175-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56175_all_checks_green() {
        let set = run_unx56175_checks();
        assert!(set.all_passed(), "UNX-F56175 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56175_stats_stable() {
        assert_eq!(variance_of(&[90, 110]), 100);
        assert_eq!(rate_of(500, 560), 1_200);
    }
}
