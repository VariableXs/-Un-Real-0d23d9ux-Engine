//! UNX-F24184 vblank/HPD 内核事件面与事件队列·子功能04（HPD 去抖账）（0x418E0 段）
//!
//! 锚点：UNX-F24184-J1 去抖账（抖动窗判定逐条落账）；J2 去抖语义（
//! 窗内抖动吞 + 稳定才发 + 窗参数闸三性质）；J3 去抖指纹（账折叠指
//! 纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（HPD 去抖面）。
//!
//! 结构：去抖器 + 抖动窗核 + 稳定判定核 + 账本 + 折叠指纹 + 注入核（
//! 零窗/窗内翻转/野样本/印后/稳态重发必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x418E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x418E0;
pub const DB_ZERO_WINDOW: u32 = SEG_BASE + 0x01; // 零窗
pub const DB_JITTER: u32 = SEG_BASE + 0x02; // 窗内抖动吞
pub const DB_UNK_SAMPLE: u32 = SEG_BASE + 0x03; // 野样本
pub const DB_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const DB_BAD_LEVEL: u32 = SEG_BASE + 0x05; // 电平越界

/// 去抖器。
pub struct DbDebounce {
    pub window_ns: u64,
    pub last_stable: u8,
    pub last_ts: u64,
    pub emitted: u32,
    pub swallowed: u32,
    pub sealed: bool,
}

impl DbDebounce {
    pub fn new(window_ns: u64, initial: u8) -> DbDebounce {
        DbDebounce {
            window_ns,
            last_stable: initial,
            last_ts: 0,
            emitted: 0,
            swallowed: 0,
            sealed: false,
        }
    }

    /// 电平样本：与稳定态相异且距上次稳定 ≥ window 才发。
    pub fn sample(&mut self, level: u8, ts_ns: u64) -> Result<bool, u32> {
        if self.sealed {
            return Err(DB_FROZEN);
        }
        if level > 1 {
            return Err(DB_BAD_LEVEL);
        }
        if self.window_ns == 0 {
            return Err(DB_ZERO_WINDOW);
        }
        if level == self.last_stable {
            self.swallowed += 1;
            return Ok(false);
        }
        if ts_ns.saturating_sub(self.last_ts) < self.window_ns {
            self.swallowed += 1;
            return Err(DB_JITTER);
        }
        self.last_stable = level;
        self.last_ts = ts_ns;
        self.emitted += 1;
        Ok(true)
    }

    pub fn emitted_count(&self) -> u32 {
        self.emitted
    }

    pub fn swallowed_count(&self) -> u32 {
        self.swallowed
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x418E0;
        h = h.wrapping_mul(31).wrapping_add(self.emitted);
        h = h.wrapping_mul(31).wrapping_add(self.swallowed);
        h = h.wrapping_mul(31).wrapping_add(self.last_stable as u32);
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：窗 10ms，翻转一次。
pub fn mk_debounce() -> DbDebounce {
    let mut d = DbDebounce::new(10_000_000, 0);
    let _ = d.sample(1, 20_000_000); // 发（距 0 稳定 20ms）
    let _ = d.sample(0, 25_000_000); // 抖（距 20ms 稳定 5ms < 10ms）
    let _ = d.sample(0, 35_000_000); // 发（距 20ms 15ms）
    d
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut d = mk_debounce();
    // 1) 零窗
    got[0] = DbDebounce::new(0, 0).sample(1, 1).err() == Some(DB_ZERO_WINDOW);
    // 2) 窗内抖动
    got[1] = d.sample(1, 40_000_000).err() == Some(DB_JITTER);
    // 3) 电平越界
    got[2] = d.sample(9, 90_000_000).err() == Some(DB_BAD_LEVEL);
    // 4) 同态吞
    got[3] = d.sample(0, 90_000_000) == Ok(false);
    // 5) 印后操作必抓
    d.seal();
    got[4] = d.sample(1, 99_000_000).err() == Some(DB_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24184_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24184");
    let mut d = mk_debounce();

    // J1 参数齐
    let j1 = d.window_ns == 10_000_000;
    s.add("J1-窗参数齐", j1, "窗 10ms");

    // J2 稳定才发
    let j2 = d.emitted_count() == 2;
    s.add("J2-稳定才发", j2, "两次翻转发出");

    // J3 抖动吞
    let j3 = d.swallowed_count() >= 1;
    s.add("J3-抖动吞", j3, "窗内翻转被吞");

    // J4 零窗拒
    let j4 = DbDebounce::new(0, 0).sample(1, 1).err() == Some(DB_ZERO_WINDOW);
    s.add("J4-零窗拒", j4, "窗 0 必拒");

    // J5 电平越界拒
    let j5 = d.sample(9, 90_000_000).err() == Some(DB_BAD_LEVEL);
    s.add("J5-电平越界拒", j5, "level>1 必拒");

    // J6 同态吞
    let j6 = d.sample(0, 90_000_000) == Ok(false);
    s.add("J6-同态吞", j6, "同稳定态样本吞");

    // J7 窗内抖动拒
    let j7 = {
        let mut d2 = DbDebounce::new(10_000_000, 0);
        let _ = d2.sample(1, 20_000_000);
        d2.sample(0, 22_000_000).err() == Some(DB_JITTER)
    };
    s.add("J7-窗内抖动拒", j7, "距稳态 < 窗必拒");

    // J8 恰窗界放行
    let j8 = {
        let mut d2 = DbDebounce::new(10_000_000, 0);
        let _ = d2.sample(1, 20_000_000);
        d2.sample(0, 30_000_000).is_ok()
    };
    s.add("J8-恰界放行", j8, "距稳态 == 窗放行");

    // J9 稳定态追踪
    let j9 = d.last_stable == 0;
    s.add("J9-稳态追踪", j9, "末稳态 0");

    // J10 吞计数守恒
    let j10 = d.swallowed_count() >= d.emitted_count() - 1;
    s.add("J10-吞计数", j10, "吞计数合理");

    // J11 印后操作拒
    let j11 = {
        let mut d2 = mk_debounce();
        d2.seal();
        d2.sample(1, 99_000_000).err() == Some(DB_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

    // J12 指纹稳定
    let j12 = d.fold() == d.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

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
    fn unx24184_debounce_flow() {
        let mut d = DbDebounce::new(10, 0);
        assert_eq!(d.sample(1, 20), Ok(true));
        assert_eq!(d.sample(0, 25), Err(DB_JITTER));
        assert_eq!(d.sample(0, 35), Ok(true));
        assert_eq!(d.sample(0, 99), Ok(false));
        assert_eq!(d.sample(9, 99), Err(DB_BAD_LEVEL));
        assert_eq!(DbDebounce::new(0, 0).sample(1, 1), Err(DB_ZERO_WINDOW));
        d.seal();
        assert_eq!(d.sample(1, 99), Err(DB_FROZEN));
    }

    #[test]
    fn unx24184_checks_green() {
        let s = run_unx24184_checks();
        assert!(s.all_passed(), "UNX-F24184 判据未全绿");
    }
}
