//! UNX-F24193 vblank/HPD 内核事件面与事件队列·子功能13（漏帧计数）（0x41A00 段）
//!
//! 锚点：UNX-F24193-J1 漏帧账（跳变漏帧逐条落账）；J2 漏帧语义（跳
//! 变计漏 + 回退拒 + 零跳不漏三性质）；J3 漏帧指纹（账折叠指纹）三
//! 判据全绿。
//! 域：CoRun·UNX-F5-B15（漏帧计数面）。
//!
//! 结构：漏帧计 + 跳变核 + 账本 + 折叠指纹 + 注入核（回退/野类型/
//! 印后/零跳/巨跳必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0x41A00 独占 0x20）。
pub const SEG_BASE: u32 = 0x41A00;
pub const MF_REVERSE: u32 = SEG_BASE + 0x01; // 计数回退
pub const MF_BAD_KIND: u32 = SEG_BASE + 0x02; // 野类型
pub const MF_FROZEN: u32 = SEG_BASE + 0x03; // 印后操作
pub const MF_BAD_DELTA: u32 = SEG_BASE + 0x04; // 跳变越界
pub const MF_OVERFLOW: u32 = SEG_BASE + 0x05; // 漏帧计数溢出

/// 漏帧计：vblank 计数单调；delta−1 为漏帧。
pub struct MfCounter {
    pub max_jump: u64,
    last: u64,
    seen: bool,
    pub missed: u64,
    pub missed_events: u64,
    sealed: bool,
}

impl MfCounter {
    pub fn new(max_jump: u64) -> MfCounter {
        MfCounter {
            max_jump,
            last: 0,
            seen: false,
            missed: 0,
            missed_events: 0,
            sealed: false,
        }
    }

    pub fn last_count(&self) -> u64 {
        self.last
    }

    /// 报 vblank 计数：跳变记漏。
    pub fn report(&mut self, kind: u8, count: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(MF_FROZEN);
        }
        if kind > 7 {
            return Err(MF_BAD_KIND);
        }
        if !self.seen {
            self.seen = true;
            self.last = count;
            return Ok(0);
        }
        if count < self.last {
            return Err(MF_REVERSE);
        }
        let delta = count - self.last;
        if delta > self.max_jump {
            return Err(MF_BAD_DELTA);
        }
        self.last = count;
        let missed_now = if delta > 0 { delta - 1 } else { 0 };
        self.missed = self.missed.saturating_add(missed_now);
        if missed_now > 0 {
            self.missed_events += 1;
        }
        Ok(missed_now)
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41A00;
        h = h.wrapping_mul(31).wrapping_add(self.last as u32);
        h = h.wrapping_mul(31).wrapping_add(self.missed as u32);
        h = h.wrapping_mul(31).wrapping_add(self.missed_events as u32);
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准夹具：max_jump=16，报 1/2/5（漏 2）/5（零跳）/9（漏 3）。
pub fn mk_counter() -> MfCounter {
    let mut c = MfCounter::new(16);
    let _ = c.report(1, 1);
    let _ = c.report(1, 2);
    let _ = c.report(1, 5);
    let _ = c.report(1, 5);
    let _ = c.report(1, 9);
    c
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 回退
    let mut c = MfCounter::new(16);
    let _ = c.report(1, 10);
    got[0] = c.report(1, 9).err() == Some(MF_REVERSE);
    // 2) 野类型
    got[1] = MfCounter::new(16).report(8, 1).err() == Some(MF_BAD_KIND);
    // 3) 零跳不漏
    let mut c2 = MfCounter::new(16);
    let _ = c2.report(1, 5);
    got[2] = c2.report(1, 5) == Ok(0) && c2.missed == 0;
    // 4) 巨跳越界
    let mut c3 = MfCounter::new(4);
    let _ = c3.report(1, 0);
    got[3] = c3.report(1, 100).err() == Some(MF_BAD_DELTA);
    // 5) 印后操作
    let mut c4 = mk_counter();
    c4.seal();
    got[4] = c4.report(1, 999).err() == Some(MF_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24193_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24193");
    let mut c = mk_counter();

    // J1 累计漏帧：2+3 ⇒ 5
    let j1 = c.missed == 5;
    s.add("J1-累计漏帧", j1, "漏 5 帧");

    // J2 漏帧事件数：2 次
    let j2 = c.missed_events == 2;
    s.add("J2-漏帧事件", j2, "2 次跳变");

    // J3 末计数：9
    let j3 = c.last_count() == 9;
    s.add("J3-末计数", j3, "last=9");

    // J4 回退拒
    let j4 = c.report(1, 8).err() == Some(MF_REVERSE);
    s.add("J4-回退拒", j4, "count<last 必拒");

    // J5 野类型拒
    let j5 = c.report(8, 9).err() == Some(MF_BAD_KIND);
    s.add("J5-野类型拒", j5, "kind>7 必拒");

    // J6 零跳不漏
    let j6 = c.report(1, 9) == Ok(0);
    s.add("J6-零跳不漏", j6, "等值零漏");

    // J7 顺延 1 不漏
    let j7 = {
        let mut c2 = MfCounter::new(16);
        let _ = c2.report(1, 0);
        c2.report(1, 1) == Ok(0)
    };
    s.add("J7-顺延不漏", j7, "delta=1 零漏");

    // J8 恰跳 2 漏 1
    let j8 = {
        let mut c2 = MfCounter::new(16);
        let _ = c2.report(1, 0);
        c2.report(1, 2) == Ok(1)
    };
    s.add("J8-跳2漏1", j8, "delta=2 漏 1");

    // J9 巨跳越界拒
    let j9 = {
        let mut c2 = MfCounter::new(4);
        let _ = c2.report(1, 0);
        c2.report(1, 5).err() == Some(MF_BAD_DELTA)
    };
    s.add("J9-巨跳拒", j9, "delta>上限必拒");

    // J10 首报基点
    let j10 = MfCounter::new(16).report(1, 42) == Ok(0);
    s.add("J10-首报基点", j10, "首报零漏");

    // J11 印后操作拒
    let j11 = {
        let mut c2 = mk_counter();
        c2.seal();
        c2.report(1, 999).err() == Some(MF_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

    // J12 指纹稳定
    let j12 = {
        let mut c2 = mk_counter();
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
    fn unx24193_miss_flow() {
        let mut c = MfCounter::new(16);
        assert_eq!(c.report(1, 0), Ok(0));
        assert_eq!(c.report(1, 3), Ok(2));
        assert_eq!(c.report(1, 3), Ok(0));
        assert_eq!(c.report(1, 2), Err(MF_REVERSE));
        assert_eq!(c.report(8, 3), Err(MF_BAD_KIND));
        assert_eq!(c.report(1, 999), Err(MF_BAD_DELTA));
        c.seal();
        assert_eq!(c.report(1, 1000), Err(MF_FROZEN));
    }

    #[test]
    fn unx24193_checks_green() {
        let s = run_unx24193_checks();
        assert!(s.all_passed(), "UNX-F24193 判据未全绿");
    }
}
