//! UNX-F12014 进程基础优先级与优先级类语义账（0x193E0 段）
//!
//! 判据原文：优先级类闭集（Idle/BelowNormal/Normal/AboveNormal/High/
//! Realtime）各钉基础值；动态优先级必须在 [类基值, 类基值+提升上限]
//! 带内；Realtime 类禁提升（提升上限为零）；越带置级一律钳到带沿并
//! 留钳账。
//!
//! 结构：优先级账（PrioBook：类 + 动态级 + 钳账）+ 置级核（四码守卫）
//! + 提升面（boost）+ 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12014_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x193E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x193E0;
pub const PR_BAD_CLASS: u32 = SEG_BASE + 0x01; // 未知优先级类
pub const PR_REALTIME_BOOST: u32 = SEG_BASE + 0x02; // Realtime 提升禁令
pub const PR_NOCHANGE: u32 = SEG_BASE + 0x03; // 同级重置
pub const PR_CLASS_SWAP_FROZEN: u32 = SEG_BASE + 0x04; // 类切换冻结期
pub const PR_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍动作

/// 优先级类闭集（基础值为 NT 语义档钉值）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrioClass {
    Idle = 1,
    BelowNormal = 2,
    Normal = 3,
    AboveNormal = 4,
    High = 5,
    Realtime = 6,
}

/// 类基础值。
pub fn class_base(c: PrioClass) -> u8 {
    match c {
        PrioClass::Idle => 4,
        PrioClass::BelowNormal => 6,
        PrioClass::Normal => 8,
        PrioClass::AboveNormal => 10,
        PrioClass::High => 13,
        PrioClass::Realtime => 24,
    }
}

/// 类提升上限（Realtime 为零）。
pub fn class_boost_max(c: PrioClass) -> u8 {
    match c {
        PrioClass::Realtime => 0,
        _ => 2,
    }
}

/// 一笔置级/提升的账目。
#[derive(Clone)]
pub struct PrRec {
    pub seq: u32,
    pub level: u8,
    pub clamped: bool,
    pub note: String,
}

impl PrRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
}

/// 优先级账模型。
pub struct PrioBook {
    pub class: PrioClass,
    pub level: u8,
    pub class_locked: bool,
    pub ledger: Vec<PrRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl PrioBook {
    pub fn new(c: PrioClass) -> PrioBook {
        PrioBook {
            class: c,
            level: class_base(c),
            class_locked: false,
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// 类带下沿/上沿。
    pub fn band(&self) -> (u8, u8) {
        let b = class_base(self.class);
        (b, b + class_boost_max(self.class))
    }

    /// 置级核：越带钳到带沿并留钳账；同值拒；Realtime 提升拒。
    pub fn set_level(&mut self, level: u8, note: &str) -> Result<u8, u32> {
        if self.sealed {
            return Err(PR_FROZEN);
        }
        if note.is_empty() {
            return Err(PR_BAD_CLASS);
        }
        let (lo, hi) = self.band();
        // 越下带钳到 lo。
        let mut clamped = false;
        let mut v = level;
        if v < lo {
            v = lo;
            clamped = true;
        }
        if v > hi {
            // Realtime 越上带即提升禁令（带零宽）。
            if self.class == PrioClass::Realtime {
                return Err(PR_REALTIME_BOOST);
            }
            v = hi;
            clamped = true;
        }
        if !clamped && v == self.level {
            return Err(PR_NOCHANGE);
        }
        let rec = PrRec { seq: self.seq_next, level: v, clamped, note: note.to_string() };
        if !rec.ok() {
            return Err(PR_BAD_CLASS);
        }
        self.level = v;
        self.seq_next += 1;
        self.ledger.push(rec);
        Ok(v)
    }

    /// 切类核：类锁定后不可切；切类即回类基值。
    pub fn switch_class(&mut self, c: PrioClass, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(PR_FROZEN);
        }
        if note.is_empty() {
            return Err(PR_BAD_CLASS);
        }
        if self.class_locked {
            return Err(PR_CLASS_SWAP_FROZEN);
        }
        if c == self.class {
            return Err(PR_NOCHANGE);
        }
        self.class = c;
        self.level = class_base(c);
        self.ledger.push(PrRec {
            seq: self.seq_next,
            level: self.level,
            clamped: false,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(())
    }

    /// 锁类核。
    pub fn lock_class(&mut self) {
        self.class_locked = true;
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add(self.class as u32);
        h = h.wrapping_mul(211).wrapping_add(self.level as u32);
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) Realtime 越带提升 → PR_REALTIME_BOOST（带零宽，任何 >24 即拒）。
    let mut a = PrioBook::new(PrioClass::Realtime);
    got[0] = a.set_level(25, "rt").err() == Some(PR_REALTIME_BOOST);
    // 2) 同级重置 → PR_NOCHANGE。
    let mut b = PrioBook::new(PrioClass::Normal);
    got[1] = b.set_level(8, "same").err() == Some(PR_NOCHANGE);
    // 3) 类锁定后切类 → PR_CLASS_SWAP_FROZEN。
    let mut c = PrioBook::new(PrioClass::Normal);
    c.lock_class();
    got[2] = c.switch_class(PrioClass::High, "lk").err() == Some(PR_CLASS_SWAP_FROZEN);
    // 4) 空留痕置级 → PR_BAD_CLASS。
    let mut d = PrioBook::new(PrioClass::Normal);
    got[3] = d.set_level(9, "").err() == Some(PR_BAD_CLASS);
    // 5) 印账后置级 → PR_FROZEN。
    let mut e = PrioBook::new(PrioClass::High);
    e.seal();
    got[4] = e.set_level(14, "post").err() == Some(PR_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12014_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12014");

    // J1 类基值表：六类基值逐一核对语义档钉值。
    let j1 = class_base(PrioClass::Idle) == 4
        && class_base(PrioClass::BelowNormal) == 6
        && class_base(PrioClass::Normal) == 8
        && class_base(PrioClass::AboveNormal) == 10
        && class_base(PrioClass::High) == 13
        && class_base(PrioClass::Realtime) == 24;
    s.add("J1-类基值表", j1, "六类钉值全对");

    // J2 建账初值：类基值起。
    let j2 = {
        let b = PrioBook::new(PrioClass::High);
        b.level == 13 && b.band() == (13, 15)
    };
    s.add("J2-初值类基", j2, "High 带 [13,15]");

    // J3 正路径：带内置级直接生效。
    let j3 = {
        let mut b = PrioBook::new(PrioClass::Normal);
        b.set_level(9, "up").is_ok() && b.level == 9
    };
    s.add("J3-带内置级", j3, "8→9 生效");

    // J4 越上带钳制：Normal 15 → 钳到 10 留钳账。
    let j4 = {
        let mut b = PrioBook::new(PrioClass::Normal);
        match b.set_level(15, "over") {
            Ok(v) => v == 10 && b.level == 10 && b.ledger.last().map(|r| r.clamped).unwrap_or(false),
            Err(_) => false,
        }
    };
    s.add("J4-越带钳制", j4, "15 钳到 10");

    // J5 越下带钳制：Normal 1 → 钳到 8。
    let j5 = {
        let mut b = PrioBook::new(PrioClass::Normal);
        match b.set_level(1, "under") {
            Ok(v) => v == 8 && b.ledger.last().map(|r| r.clamped).unwrap_or(false),
            Err(_) => false,
        }
    };
    s.add("J5-越下钳制", j5, "1 钳到 8");

    // J6 Realtime 零宽带：初始即基值 24，越带 25 拒。
    let j6 = {
        let mut b = PrioBook::new(PrioClass::Realtime);
        let base_ok = b.level == 24;
        let bad = b.set_level(25, "boost").err() == Some(PR_REALTIME_BOOST);
        base_ok && bad && class_boost_max(PrioClass::Realtime) == 0
    };
    s.add("J6-Realtime零宽", j6, "提升禁令");

    // J7 同级重置拒。
    let j7 = {
        let mut b = PrioBook::new(PrioClass::AboveNormal);
        let _ = b.set_level(11, "a");
        b.set_level(11, "dup").err() == Some(PR_NOCHANGE)
    };
    s.add("J7-同级拒", j7, "空变更不入账");

    // J8 切类回基值：Normal→High 后级=13。
    let j8 = {
        let mut b = PrioBook::new(PrioClass::Normal);
        let _ = b.set_level(9, "up");
        let _ = b.switch_class(PrioClass::High, "sw");
        b.class == PrioClass::High && b.level == 13
    };
    s.add("J8-切类回基", j8, "切类重置级");

    // J9 类锁定语义。
    let j9 = {
        let mut b = PrioBook::new(PrioClass::Idle);
        let _ = b.switch_class(PrioClass::Normal, "a");
        b.lock_class();
        b.switch_class(PrioClass::High, "b").err() == Some(PR_CLASS_SWAP_FROZEN)
    };
    s.add("J9-锁类拒切", j9, "锁定后不可切");

    // J10 印账冻结。
    let j10 = {
        let mut b = PrioBook::new(PrioClass::Normal);
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2 && b.set_level(9, "post").err() == Some(PR_FROZEN)
    };
    s.add("J10-指纹封账", j10, "指纹幂等且印后必拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：Normal 进程提降。
    let mut b = PrioBook::new(PrioClass::Normal);
    let _ = b.set_level(10, "sum-hi");
    let _ = b.set_level(8, "sum-lo");
    s.add("样例账三笔", b.ledger.len() >= 2, "置级逐笔留痕");
    s.add("样例账面合法", b.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12014_band_clamp() {
        let mut b = PrioBook::new(PrioClass::Normal);
        assert_eq!(b.set_level(15, "hi"), Ok(10));
        assert_eq!(b.set_level(1, "lo"), Ok(8));
        assert_eq!(b.set_level(8, "same"), Err(PR_NOCHANGE));
        assert_eq!(b.set_level(9, "ok"), Ok(9));
    }

    #[test]
    fn unx12014_checks_green() {
        let s = run_unx12014_checks();
        assert!(s.all_passed(), "UNX-F12014 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12014_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
