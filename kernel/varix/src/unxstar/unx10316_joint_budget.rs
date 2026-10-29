//! UNX-F10316 联签性能预算（联跑时长/样例量上限）（0x1b200 段）
//!
//! 锚点：UNX-F10316-J1 预算双帽——联跑时长与样例量各设上限，逐笔
//! 记账越帽即拒且不入账，总账恒在双帽之内。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：预算账（笔目：名+时长+样例量）+ 双帽核（时长帽/样例帽独立
//! 溢出检测，溢出笔拒收不入账）+ 余量面（双帽余量可查）+ 注入核
//! （超时帽/超量帽/零笔/空名四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b200 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b200;
pub const PB_EMPTY: u32 = SEG_BASE + 0x01; // 笔名空
pub const PB_ZERO: u32 = SEG_BASE + 0x02; // 零时长且零样例
pub const PB_CAP: u32 = SEG_BASE + 0x03; // 账超容
pub const PB_TICKS: u32 = SEG_BASE + 0x04; // 超时长帽
pub const PB_SAMPLES: u32 = SEG_BASE + 0x05; // 超样例帽
pub const PB_SEALED: u32 = SEG_BASE + 0x06; // 封账后记账

/// 联跑时长帽（tick）。
pub const TICK_CAP: u32 = 10_000;
/// 样例量帽。
pub const SAMPLE_CAP: u32 = 4_000;
/// 预算账定容。
pub const CAP: usize = 12;

/// 单笔预算记账。
#[derive(Clone)]
pub struct BudgetLine {
    pub line: String,
    pub ticks: u32,
    pub samples: u32,
}

impl BudgetLine {
    pub fn ok(&self) -> bool {
        !self.line.is_empty() && (self.ticks > 0 || self.samples > 0)
    }
}

/// 联签性能预算账。
pub struct JointBudget {
    pub tick_cap: u32,
    pub sample_cap: u32,
    pub lines: Vec<BudgetLine>,
    pub sealed: bool,
}

impl JointBudget {
    pub fn new(tick_cap: u32, sample_cap: u32) -> JointBudget {
        JointBudget { tick_cap, sample_cap, lines: Vec::new(), sealed: false }
    }

    /// 已用时长合计。
    pub fn total_ticks(&self) -> u32 {
        let mut t: u32 = 0;
        let mut k = 0usize;
        while k < self.lines.len() {
            t = t.wrapping_add(self.lines[k].ticks);
            k += 1;
        }
        t
    }

    /// 已用样例合计。
    pub fn total_samples(&self) -> u32 {
        let mut t: u32 = 0;
        let mut k = 0usize;
        while k < self.lines.len() {
            t = t.wrapping_add(self.lines[k].samples);
            k += 1;
        }
        t
    }

    /// 记账：溢帽笔拒收不入账。
    pub fn charge(&mut self, line: &str, ticks: u32, samples: u32) -> Result<(u32, u32), u32> {
        if self.sealed {
            return Err(PB_SEALED);
        }
        if line.is_empty() {
            return Err(PB_EMPTY);
        }
        if ticks == 0 && samples == 0 {
            return Err(PB_ZERO);
        }
        if self.lines.len() >= CAP {
            return Err(PB_CAP);
        }
        let nt = self.total_ticks().wrapping_add(ticks);
        if nt > self.tick_cap {
            return Err(PB_TICKS);
        }
        let ns = self.total_samples().wrapping_add(samples);
        if ns > self.sample_cap {
            return Err(PB_SAMPLES);
        }
        self.lines.push(BudgetLine { line: line.to_string(), ticks, samples });
        Ok((nt, ns))
    }

    /// 双帽之内裁断。
    pub fn within(&self) -> bool {
        self.total_ticks() <= self.tick_cap && self.total_samples() <= self.sample_cap
    }

    /// 时长余量。
    pub fn headroom_ticks(&self) -> u32 {
        self.tick_cap.saturating_sub(self.total_ticks())
    }

    /// 样例余量。
    pub fn headroom_samples(&self) -> u32 {
        self.sample_cap.saturating_sub(self.total_samples())
    }

    /// 笔目全合法。
    pub fn lines_ok(&self) -> bool {
        self.lines.iter().all(|l| l.ok())
    }

    /// 笔名定位（未知笔名即 None）。
    pub fn line_pos(&self, name: &str) -> Option<usize> {
        let mut k = 0usize;
        while k < self.lines.len() {
            if self.lines[k].line == name {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 单笔读数（未知笔名报 PB_CAP 位）。
    pub fn line_usage(&self, name: &str) -> Result<(u32, u32), u32> {
        match self.line_pos(name) {
            None => Err(PB_CAP),
            Some(k) => Ok((self.lines[k].ticks, self.lines[k].samples)),
        }
    }

    /// 最大时长笔名（空账即空串）。
    pub fn heaviest(&self) -> String {
        let mut best = String::new();
        let mut max: u32 = 0;
        let mut k = 0usize;
        while k < self.lines.len() {
            if self.lines[k].ticks > max {
                max = self.lines[k].ticks;
                best = self.lines[k].line.clone();
            }
            k += 1;
        }
        best
    }

    /// 笔名清单（记账序）。
    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        let mut k = 0usize;
        while k < self.lines.len() {
            v.push(self.lines[k].line.clone());
            k += 1;
        }
        v
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 预算账指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b2000;
        h = h.wrapping_mul(31).wrapping_add(self.lines.len() as u32);
        let mut k = 0usize;
        while k < self.lines.len() {
            h = h.wrapping_mul(31).wrapping_add(self.lines[k].ticks);
            h = h.wrapping_mul(31).wrapping_add(self.lines[k].samples);
            let bytes = self.lines[k].line.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = h.wrapping_mul(31).wrapping_add(bytes[b] as u32);
                b += 1;
            }
            k += 1;
        }
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut b = JointBudget::new(TICK_CAP, SAMPLE_CAP);
    // 1) 超时长帽必拒且不入账
    got[0] = b.charge("超时笔", TICK_CAP + 1, 0).err() == Some(PB_TICKS)
        && b.lines.is_empty();
    // 2) 超样例帽必拒且不入账
    got[1] = b.charge("超量笔", 0, SAMPLE_CAP + 1).err() == Some(PB_SAMPLES)
        && b.lines.is_empty();
    // 3) 零笔（时/量双零）必拒
    got[2] = b.charge("零笔", 0, 0).err() == Some(PB_ZERO);
    // 4) 空名必拒
    got[3] = b.charge("", 1, 1).err() == Some(PB_EMPTY);
    got
}

/// 批内判据。
pub fn run_unx10316_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10316");

    // J1 正路径：四笔记账合计在双帽之内
    let mut b = JointBudget::new(TICK_CAP, SAMPLE_CAP);
    let mut ok = true;
    if b.charge("联签执行", 4_000, 1_500).is_err() {
        ok = false;
    }
    if b.charge("样例回放", 3_000, 1_000).is_err() {
        ok = false;
    }
    if b.charge("对平核算", 2_000, 800).is_err() {
        ok = false;
    }
    if b.charge("哈希冻结", 1_000, 700).is_err() {
        ok = false;
    }
    s.add(
        "J1-四笔双帽内",
        ok && b.within() && b.total_ticks() == 10_000 && b.total_samples() == 4_000,
        "四笔合计 10,000 tick / 4,000 样例恰在双帽沿内",
    );

    // J2 笔目全合法
    let j2 = b.lines_ok();
    s.add("J2-笔目合法", j2, "笔名非空且时量不全零");

    // J3 注入检出：超时帽 + 超量帽
    let inj = inject_four();
    s.add("J3-双帽溢出检出", inj[0] && inj[1], "溢帽笔必拒且不入账");

    // J4 注入检出：零笔 + 空名
    s.add("J4-零笔空名检出", inj[2] && inj[3], "零笔必拒且空名必拒");

    // J5 恰帽沿可入账（总时=帽、总量=帽）
    let j5 = b.total_ticks() == TICK_CAP && b.total_samples() == SAMPLE_CAP;
    s.add("J5-帽沿可入", j5, "合计恰等帽值仍算在帽内");

    // J6 溢帽后余量归零语义（余量不可负）
    let j6 = b.headroom_ticks() == 0 && b.headroom_samples() == 0;
    s.add("J6-余量归零", j6, "用满双帽余量恰为零");

    // J7 余量语义：半账余量恰为半帽
    let j7 = {
        let mut c = JointBudget::new(TICK_CAP, SAMPLE_CAP);
        let _ = c.charge("半账", TICK_CAP / 2, SAMPLE_CAP / 2);
        c.headroom_ticks() == TICK_CAP / 2 && c.headroom_samples() == SAMPLE_CAP / 2
    };
    s.add("J7-半账余量", j7, "半账余量恰为半帽");

    // J8 封账后记账拒
    let j8 = {
        let mut c = JointBudget::new(TICK_CAP, SAMPLE_CAP);
        c.seal();
        c.charge("迟到", 1, 1).err() == Some(PB_SEALED)
    };
    s.add("J8-封账拒记", j8, "封账后记账必拒");

    // J9 容量边界
    let j9 = {
        let mut c = JointBudget::new(u32::MAX, u32::MAX);
        let mut filled = true;
        let mut n = 1u32;
        while n <= CAP as u32 {
            let nm = format!("p{}", n);
            if c.charge(&nm, 1, 1).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.charge("over", 1, 1).err() == Some(PB_CAP)
    };
    s.add("J9-容量边界", j9, "账超容必拒且原账不损");

    // J10 累进溢帽：第一笔 6k 第二笔 5k 超帽拒
    let j10 = {
        let mut c = JointBudget::new(TICK_CAP, SAMPLE_CAP);
        let _ = c.charge("先手", 6_000, 0);
        c.charge("后手", 5_000, 0).err() == Some(PB_TICKS)
    };
    s.add("J10-累进溢帽", j10, "累进超帽的后续笔必拒");

    // J11 预算账指纹稳定
    let j11 = b.fold() == b.fold() && b.fold() != 0;
    s.add("J11-指纹稳定", j11, "预算账折叠指纹两次一致");

    // J12 动态明细：双帽总账落账
    s.add_dyn(
        "J12-双帽总账",
        b.total_ticks() == TICK_CAP && b.total_samples() == SAMPLE_CAP,
        &format!("ticks={} samples={}", b.total_ticks(), b.total_samples()),
    );

    // J13 动态明细：笔数落账
    let n = b.lines.len();
    s.add_dyn("J13-笔数明细", n == 4, &format!("lines={}", n));

    // J14 动态明细：单笔读数对账
    let u0 = b.line_usage("联签执行");
    let j14 = u0 == Ok((4_000, 1_500))
        && b.line_usage("幽灵笔").err() == Some(PB_CAP)
        && b.heaviest() == "联签执行";
    s.add_dyn(
        "J14-单笔对账",
        j14,
        &format!("首笔 {:?} 最大时长笔 {}", u0, b.heaviest()),
    );

    // J15 动态明细：笔名清单落账
    let nm = b.names();
    let j15 = nm.len() == 4 && nm[0] == "联签执行" && nm[3] == "哈希冻结";
    s.add_dyn("J15-笔名清单", j15, &format!("{:?}", nm));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10316_budget_flow() {
        let mut b = JointBudget::new(TICK_CAP, SAMPLE_CAP);
        assert!(b.charge("a", 100, 50).is_ok());
        assert!(b.charge("", 1, 1).err() == Some(PB_EMPTY));
        assert!(b.charge("z", 0, 0).err() == Some(PB_ZERO));
        assert!(b.charge("t", TICK_CAP, 0).err() == Some(PB_TICKS));
        assert!(b.charge("s", 0, SAMPLE_CAP).err() == Some(PB_SAMPLES));
        assert!(b.within());
        assert!(b.headroom_ticks() == TICK_CAP - 100);
        b.seal();
        assert!(b.charge("late", 1, 1).err() == Some(PB_SEALED));
    }

    #[test]
    fn unx10316_checks_green() {
        let s = run_unx10316_checks();
        assert!(s.all_passed(), "UNX-F10316 判据未全绿");
    }
}
