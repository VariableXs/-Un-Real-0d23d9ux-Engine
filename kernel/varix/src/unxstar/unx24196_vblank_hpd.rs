//! UNX-F24196 vblank/HPD 内核事件面与事件队列·子功能16（统计计数器）（0x41A60 段）
//!
//! 锚点：UNX-F24196-J1 统计账（分类计数逐条落账）；J2 统计语义（总
//! 数守恒 + 野类型拒 + 分类独立三性质）；J3 统计指纹（账折叠指纹）
//! 三判据全绿。
//! 域：CoRun·UNX-F5-B15（统计计数面）。
//!
//! 结构：分类计数器 + 计数核 + 守恒核 + 账本 + 折叠指纹 + 注入核（
//! 野类型/印后/溢出/负守恒/清零必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0x41A60 独占 0x20）。
pub const SEG_BASE: u32 = 0x41A60;
pub const ST_BAD_KIND: u32 = SEG_BASE + 0x01; // 野类型
pub const ST_FROZEN: u32 = SEG_BASE + 0x02; // 印后操作
pub const ST_OVERFLOW: u32 = SEG_BASE + 0x03; // 计数溢出
pub const ST_BAD_SUM: u32 = SEG_BASE + 0x04; // 守恒破坏
pub const ST_BAD_WATER: u32 = SEG_BASE + 0x05; // 单类水位越界

/// 分类计数器：8 类事件独立计数。
pub struct StCounters {
    counts: [u64; 8],
    pub total: u64,
    sealed: bool,
}

impl StCounters {
    pub const PER_KIND_CAP: u64 = u32::MAX as u64;

    pub fn new() -> StCounters {
        StCounters {
            counts: [0; 8],
            total: 0,
            sealed: false,
        }
    }

    /// 计数：野类型拒。
    pub fn bump(&mut self, kind: u8, n: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(ST_FROZEN);
        }
        if kind > 7 {
            return Err(ST_BAD_KIND);
        }
        if n == 0 {
            return Err(ST_BAD_WATER);
        }
        if self.counts[kind as usize] > Self::PER_KIND_CAP - n {
            return Err(ST_OVERFLOW);
        }
        self.counts[kind as usize] += n;
        self.total += n;
        Ok(self.counts[kind as usize])
    }

    pub fn count_of(&self, kind: u8) -> Option<u64> {
        if kind > 7 {
            None
        } else {
            Some(self.counts[kind as usize])
        }
    }

    /// 守恒式：Σ分类 == 总数。
    pub fn conserves(&self) -> bool {
        self.counts.iter().sum::<u64>() == self.total
    }

    /// 清零。
    pub fn reset(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(ST_FROZEN);
        }
        self.counts = [0; 8];
        self.total = 0;
        Ok(())
    }

    /// 占比（万分比）。
    pub fn share_permille(&self, kind: u8) -> Option<u64> {
        let c = self.count_of(kind)?;
        if self.total == 0 {
            return Some(0);
        }
        Some(c * 10_000 / self.total)
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41A60;
        h = h.wrapping_mul(31).wrapping_add(self.total as u32);
        for c in &self.counts {
            h = h.wrapping_mul(31).wrapping_add(*c as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准夹具：vblank×7、hpd×2、其它类各 1（共 8+4）。
pub fn mk_counters() -> StCounters {
    let mut c = StCounters::new();
    let _ = c.bump(1, 7);
    let _ = c.bump(2, 2);
    let _ = c.bump(0, 1);
    let _ = c.bump(3, 1);
    let _ = c.bump(4, 1);
    let _ = c.bump(5, 1);
    c
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 野类型
    got[0] = StCounters::new().bump(8, 1).err() == Some(ST_BAD_KIND);
    // 2) 零计数拒
    got[1] = StCounters::new().bump(1, 0).err() == Some(ST_BAD_WATER);
    // 3) 溢出拒
    let mut c = StCounters::new();
    let _ = c.bump(1, u32::MAX as u64);
    got[2] = c.bump(1, 1).err() == Some(ST_OVERFLOW);
    // 4) 清零守恒
    let mut c2 = mk_counters();
    let _ = c2.reset();
    got[3] = c2.total == 0 && c2.conserves();
    // 5) 印后操作
    let mut c3 = mk_counters();
    c3.seal();
    got[4] = c3.bump(1, 1).err() == Some(ST_FROZEN) && c3.reset().err() == Some(ST_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24196_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24196");
    let mut c = mk_counters();

    // J1 总数守恒：7+2+1+1+1+1 = 13
    let j1 = c.total == 13;
    s.add("J1-总数", j1, "计 13");

    // J2 守恒式
    let j2 = c.conserves();
    s.add("J2-守恒式", j2, "Σ=总");

    // J3 分类独立：vblank=7 hpd=2
    let j3 = c.count_of(1) == Some(7) && c.count_of(2) == Some(2);
    s.add("J3-分类独立", j3, "类间不串");

    // J4 占比：vblank 7/13 万分比 = 5384（整数除法）
    let j4 = c.share_permille(1) == Some(5384);
    s.add("J4-占比", j4, "7/13≈5384");

    // J5 野类型拒
    let j5 = c.bump(8, 1).err() == Some(ST_BAD_KIND);
    s.add("J5-野类型拒", j5, "kind>7 必拒");

    // J6 零计数拒
    let j6 = c.bump(1, 0).err() == Some(ST_BAD_WATER);
    s.add("J6-零计数拒", j6, "n=0 必拒");

    // J7 累计性
    let j7 = c.bump(1, 3) == Ok(10);
    s.add("J7-累计性", j7, "7+3=10");

    // J8 溢出拒
    let j8 = {
        let mut c2 = StCounters::new();
        let _ = c2.bump(2, u32::MAX as u64);
        c2.bump(2, 1).err() == Some(ST_OVERFLOW)
    };
    s.add("J8-溢出拒", j8, "超类容必拒");

    // J9 查询野类型 None
    let j9 = c.count_of(9).is_none() && c.share_permille(9).is_none();
    s.add("J9-查询野拒", j9, "野类型查询 None");

    // J10 清零后守恒
    let j10 = {
        let mut c2 = StCounters::new();
        let _ = c2.bump(1, 5);
        let _ = c2.reset();
        c2.total == 0 && c2.conserves() && c2.count_of(1) == Some(0)
    };
    s.add("J10-清零守恒", j10, "清零归零");

    // J11 印后操作拒
    let j11 = {
        let mut c2 = mk_counters();
        c2.seal();
        c2.bump(1, 1).err() == Some(ST_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

    // J12 指纹稳定
    let j12 = {
        let mut c2 = mk_counters();
        let a = c2.fold();
        c2.seal();
        c2.fold() == a
    };
    s.add("J12-指纹稳定", j12, "印不改指纹");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx24196_stats_flow() {
        let mut c = StCounters::new();
        assert_eq!(c.bump(1, 5), Ok(5));
        assert_eq!(c.bump(1, 2), Ok(7));
        assert_eq!(c.bump(8, 1), Err(ST_BAD_KIND));
        assert_eq!(c.bump(1, 0), Err(ST_BAD_WATER));
        assert!(c.conserves());
        assert_eq!(c.count_of(1), Some(7));
        assert_eq!(c.count_of(9), None);
        assert_eq!(c.reset(), Ok(()));
        assert_eq!(c.total, 0);
        c.seal();
        assert_eq!(c.bump(1, 1), Err(ST_FROZEN));
        assert_eq!(c.reset(), Err(ST_FROZEN));
    }

    #[test]
    fn unx24196_checks_green() {
        let s = run_unx24196_checks();
        assert!(s.all_passed(), "UNX-F24196 判据未全绿");
    }
}
