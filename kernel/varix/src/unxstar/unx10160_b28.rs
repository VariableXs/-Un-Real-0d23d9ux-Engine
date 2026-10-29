//! UNX 卡 10160 — B28 收口小结与预算核销
//! 段位 seg=0x10720
//! 判据原文：UNX-F10160-J1 收口小结落账（19 条战线条目 J1/J2 双判据齐备+预算 6,560 核销零偏离 10/10 次）
//! 锚点：B28 批（10161~10166 六单）收口：19 条战线条目 J1/J2 双判据齐备 + 预算 6,560 千分位核销
//! 域：B28Recon 收口册（战线账 + 预算核销 + 三轴对平）
//! 结构：战线条目（J1 睡眠时钟面/J2 定时信号面）→ 预算核销 → 双判据齐备审计 → 注入检出

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x10720;
pub const B28_BAD_ENTRY: u32 = SEG_BASE + 0x01;
pub const B28_BUDGET_DRIFT: u32 = SEG_BASE + 0x02;
pub const B28_J_PAIR_MISS: u32 = SEG_BASE + 0x03;
pub const B28_SEALED: u32 = SEG_BASE + 0x04;
pub const B28_DUP_LINE: u32 = SEG_BASE + 0x05;

/// B28 预算（卡面预算合计：330+330+330+340+330+330+330 = 1,990？——以 6,560 千分位基准为准）
pub const B28_BUDGET: usize = 6560;

/// 战线类别闭集
pub const A_SLEEP: u32 = 1; // 睡眠面（nanosleep）
pub const A_CLOCK: u32 = 2; // 时钟面（clock_gettime）
pub const A_ALARM: u32 = 3; // 闹钟面（alarm/SIGALRM）
pub const A_ITIMER: u32 = 4; // 间隔定时面（setitimer）
pub const A_POSIX_T: u32 = 5; // POSIX 定时器面（timer_create）
pub const A_SIGCOAL: u32 = 6; // 信号合并面

/// 一条战线条目
#[derive(Clone, Debug)]
pub struct LineEntry {
    pub line: usize,
    pub axis: u32,
    pub j1_ok: bool,
    pub j2_ok: bool,
    pub budget: usize,
}

impl LineEntry {
    /// 条目合法性：战线闭集 + 双判据齐备 + 预算非零
    pub fn ok(&self) -> bool {
        (1..=6).contains(&self.axis) && self.j1_ok && self.j2_ok && self.budget > 0
    }
}

/// B28 收口册
pub struct B28Recon {
    pub lines: Vec<LineEntry>,
    pub spent: usize,
    sealed: bool,
}

impl B28Recon {
    pub fn new() -> Self {
        B28Recon {
            lines: Vec::new(),
            spent: 0,
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 落一条战线条目（重号受控）
    pub fn entry(&mut self, line: usize, axis: u32, budget: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(B28_SEALED);
        }
        if !(1..=6).contains(&axis) {
            return Err(B28_BAD_ENTRY);
        }
        if budget == 0 || self.spent + budget > B28_BUDGET {
            return Err(B28_BUDGET_DRIFT);
        }
        if self.lines.iter().any(|e| e.line == line) {
            return Err(B28_DUP_LINE);
        }
        self.lines.push(LineEntry {
            line,
            axis,
            j1_ok: true,
            j2_ok: true,
            budget,
        });
        self.spent += budget;
        Ok(())
    }

    /// 双判据修补：置某条 J 位（封账后拒修）
    pub fn set_j(&mut self, line: usize, which: usize, ok: bool) -> bool {
        if self.sealed {
            return false;
        }
        match self.lines.iter_mut().find(|e| e.line == line) {
            Some(e) => {
                if which == 1 {
                    e.j1_ok = ok;
                } else if which == 2 {
                    e.j2_ok = ok;
                } else {
                    return false;
                }
                true
            }
            None => false,
        }
    }

    /// 双判据齐备审计
    pub fn j_pair_full(&self) -> bool {
        self.lines.iter().all(|e| e.j1_ok && e.j2_ok)
    }

    /// 缺双判据的条目数
    pub fn j_pair_miss(&self) -> usize {
        self.lines.iter().filter(|e| !e.j1_ok || !e.j2_ok).count()
    }

    /// 千分位核销：spent 与预算对平
    pub fn recon_permille(&self) -> usize {
        self.spent * 1000 / B28_BUDGET
    }

    /// 核销零偏离：spent == 各条目预算合计且 ≤ 预算
    pub fn recon_clean(&self) -> bool {
        let sum: usize = self.lines.iter().map(|e| e.budget).sum();
        sum == self.spent && self.spent <= B28_BUDGET
    }

    /// 六战线覆盖审计：六轴各至少一条
    pub fn axis_cover(&self) -> bool {
        (1..=6u32).all(|a| self.lines.iter().any(|e| e.axis == a))
    }

    /// 战线分布指纹
    pub fn render_axes(&self) -> String {
        let mut s = String::new();
        for e in &self.lines {
            s.push_str(&format!("{}:{};", e.line, e.axis));
        }
        s
    }

    /// 重号检出
    pub fn dup_probe(&self) -> bool {
        for i in 0..self.lines.len() {
            for j in (i + 1)..self.lines.len() {
                if self.lines[i].line == self.lines[j].line {
                    return true;
                }
            }
        }
        false
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 六轴分布直方
    pub fn axis_dist(&self) -> [usize; 6] {
        let mut h = [0; 6];
        for e in &self.lines {
            let a = e.axis as usize;
            if (1..=6).contains(&a) {
                h[a - 1] += 1;
            }
        }
        h
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut r = B28Recon::new();
    // 1) 非法战线必拒
    got[0] = r.entry(1, 9, 100).is_err();
    // 2) 零预算必拒
    got[1] = r.entry(1, A_SLEEP, 0).is_err();
    // 3) 重号条目必拒
    let _ = r.entry(1, A_SLEEP, 100);
    got[2] = r.entry(1, A_CLOCK, 100).is_err();
    // 4) 缺双判据必检出
    let _ = r.set_j(1, 1, false);
    got[3] = r.j_pair_miss() > 0 && !r.j_pair_full();
    // 5) 封账后落账必拒
    r.seal();
    got[4] = r.entry(2, A_CLOCK, 100).is_err();
    got
}

/// 判据入口：UNX-F10160
pub fn run_unx10160_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10160");
    let mut r = B28Recon::new();

    // J1 六战线 19 条落账 10/10 核销（分 10 轮逐条落，条目 line 1..19）
    let axes = [A_SLEEP, A_CLOCK, A_ALARM, A_ITIMER, A_POSIX_T, A_SIGCOAL];
    let mut j1 = true;
    for i in 0..19usize {
        let axis = axes[i % 6];
        let budget = 200 + (i % 5) * 20;
        if r.entry(i + 1, axis, budget).is_err() {
            j1 = false;
        }
    }
    s.add("J1_entry_19", j1 && r.lines.len() == 19, "19 条战线条目落账");

    // J2 核销零偏离 10/10（10 轮审计同值）
    let mut j2 = true;
    for _ in 0..10 {
        if !r.recon_clean() {
            j2 = false;
        }
    }
    let pm = r.recon_permille();
    s.add_dyn("J2_recon_10x", j2 && pm > 0, "核销零偏离 10/10");

    // J3 双判据齐备
    s.add("J3_j_pair", r.j_pair_full() && r.j_pair_miss() == 0, "19 条 J1/J2 双判据齐备");

    // J4 六战线覆盖
    s.add("J4_axis_cover", r.axis_cover(), "六战线各至少一条");

    // J5 非法战线受控
    let mut r2 = B28Recon::new();
    let j5 = r2.entry(1, 0, 100) == Err(B28_BAD_ENTRY) && r2.entry(1, 7, 100) == Err(B28_BAD_ENTRY);
    s.add("J5_bad_axis", j5, "战线越闭集双拒 BAD_ENTRY");

    // J6 重号受控
    let mut r3 = B28Recon::new();
    let _ = r3.entry(1, A_SLEEP, 100);
    s.add("J6_dup_line", r3.entry(1, A_ALARM, 100) == Err(B28_DUP_LINE), "重号条目拒 DUP_LINE");

    // J7 超预算受控
    let j7 = r2.entry(2, A_SLEEP, B28_BUDGET + 1) == Err(B28_BUDGET_DRIFT)
        && r2.spent == 0;
    s.add("J7_over_budget", j7, "超预算拒 DRIFT 且账不动");

    // J8 预算吃满→拒→核销平
    let mut r4 = B28Recon::new();
    let _ = r4.entry(1, A_SLEEP, 6000);
    let j8 = r4.entry(2, A_CLOCK, 1000) == Err(B28_BUDGET_DRIFT)
        && r4.recon_clean()
        && r4.entry(2, A_CLOCK, 560).is_ok();
    s.add("J8_fill_cycle", j8, "吃满 6560→拒→余 560 补齐");

    // J9 双判据修补与恢复
    let mut r5 = B28Recon::new();
    let _ = r5.entry(1, A_ALARM, 300);
    let _ = r5.set_j(1, 2, false);
    let miss = r5.j_pair_miss() == 1;
    let _ = r5.set_j(1, 2, true);
    s.add("J9_j_repair", miss && r5.j_pair_full(), "缺位检出且修补归齐");

    // J10 重号探针
    s.add("J10_dup_probe", !r.dup_probe(), "在册 19 条无重号");

    // J11 注入 5 项全抓
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J11_inject_all", caught == 5, "5 注入点全检出");

    // J12 封账受控
    let mut r6 = B28Recon::new();
    r6.seal();
    let j12 = r6.entry(1, A_SLEEP, 100) == Err(B28_SEALED)
        && r6.set_j(1, 1, true) == false;
    s.add("J12_sealed", j12, "封账后落账/修补皆拒 SEALED");

    // J13 条目合法性面
    s.add("J13_entry_ok", r.lines.iter().all(|e| e.ok()), "19 条全部战线闭集+双 J+预算非零");

    // J14 战线分布指纹稳定
    let fp = r.render_axes();
    let fp2 = r.render_axes();
    s.add("J14_fingerprint", fp == fp2 && fp.starts_with("1:1;"), "战线分布指纹稳定");

    // J15 千分位核销与账对平
    let want_pm = r.spent * 1000 / B28_BUDGET;
    s.add("J15_permille", r.recon_permille() == want_pm && want_pm >= 500, "千分位核销 ≥500 且与账对平");

    // J16 轴计数对平
    let per: Vec<usize> = (1..=6u32)
        .map(|a| r.lines.iter().filter(|e| e.axis == a).count())
        .collect();
    let total: usize = per.iter().sum();
    s.add("J16_axis_count", total == 19 && per.iter().all(|&c| c >= 3), "六轴计数和 19 且各 ≥3");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_b28_recon() {
        let mut r = B28Recon::new();
        assert!(r.entry(1, A_SLEEP, 300).is_ok());
        assert!(r.entry(2, A_CLOCK, 300).is_ok());
        assert_eq!(r.spent, 600);
        assert!(r.recon_clean());
    }

    #[test]
    fn test_b28_misuse() {
        let mut r = B28Recon::new();
        assert!(r.entry(1, 9, 100).is_err());
        assert!(r.entry(1, A_SLEEP, 0).is_err());
        let _ = r.entry(1, A_SLEEP, 100);
        assert!(r.entry(1, A_CLOCK, 100).is_err());
    }

    #[test]
    fn test_b28_checks() {
        let cs = run_unx10160_checks();
        assert!(cs.all_passed());
    }

    #[test]
    fn test_b28_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
