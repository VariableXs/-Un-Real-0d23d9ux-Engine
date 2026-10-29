//! UNX-F24192 vblank/HPD 内核事件面与事件队列·子功能12（事件合并）（0x419E0 段）
//!
//! 锚点：UNX-F24192-J1 合并账（同型合并逐条落账）；J2 合并语义（同
//! 型合并 + 异型不并 + 窗参数闸三性质）；J3 合并指纹（账折叠指纹）
//! 三判据全绿。
//! 域：CoRun·UNX-F5-B15（事件合并面）。
//!
//! 结构：合并器 + 同型并核 + 账本 + 折叠指纹 + 注入核（零窗/野类型/
//! 印后/溢出/异型混流必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x419E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x419E0;
pub const MG_BAD_WINDOW: u32 = SEG_BASE + 0x01; // 零窗
pub const MG_BAD_KIND: u32 = SEG_BASE + 0x02; // 野类型
pub const MG_FROZEN: u32 = SEG_BASE + 0x03; // 印后操作
pub const MG_OVERFLOW: u32 = SEG_BASE + 0x04; // 合并计数溢出
pub const MG_BAD_MERGE: u32 = SEG_BASE + 0x05; // 异型混并

/// 已合并事件：同型连续事件并为一，记次数与末时戳。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct MgMerged {
    pub kind: u8,
    pub count: u32,
    pub last_ts: u64,
}

/// 合并器：同型且时戳差 ≤ 窗则并；异型开新槽。
pub struct MgMerger {
    pub window_ns: u64,
    out: Vec<MgMerged>,
    merged_away: u64,
    sealed: bool,
}

impl MgMerger {
    pub fn new(window_ns: u64) -> Result<MgMerger, u32> {
        if window_ns == 0 {
            return Err(MG_BAD_WINDOW);
        }
        Ok(MgMerger {
            window_ns,
            out: Vec::new(),
            merged_away: 0,
            sealed: false,
        })
    }

    /// 送事件：返回槽位。
    pub fn feed(&mut self, kind: u8, ts: u64) -> Result<usize, u32> {
        if self.sealed {
            return Err(MG_FROZEN);
        }
        if kind > 7 {
            return Err(MG_BAD_KIND);
        }
        if self.merged_away >= u32::MAX as u64 {
            return Err(MG_OVERFLOW);
        }
        // 找同型尾窗槽
        for i in (0..self.out.len()).rev() {
            if self.out[i].kind == kind {
                if ts.saturating_sub(self.out[i].last_ts) <= self.window_ns {
                    self.out[i].count += 1;
                    self.out[i].last_ts = ts;
                    self.merged_away += 1;
                    return Ok(i);
                }
                break; // 同型但超窗：不再向前找
            }
        }
        self.out.push(MgMerged {
            kind,
            count: 1,
            last_ts: ts,
        });
        Ok(self.out.len() - 1)
    }

    pub fn slots(&self) -> &[MgMerged] {
        &self.out
    }

    pub fn merged_away_count(&self) -> u64 {
        self.merged_away
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x419E0;
        h = h.wrapping_mul(31).wrapping_add(self.window_ns as u32);
        h = h.wrapping_mul(31).wrapping_add(self.merged_away as u32);
        for m in &self.out {
            h = h.wrapping_mul(31).wrapping_add(m.kind as u32);
            h = h.wrapping_mul(31).wrapping_add(m.count);
            h = h.wrapping_mul(31).wrapping_add(m.last_ts as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准夹具：窗 100，喂 vblank×3（10/50/90 并一）+ hpd×1（200）。
pub fn mk_merger() -> MgMerger {
    let mut m = MgMerger::new(100).expect("std window");
    let _ = m.feed(1, 10);
    let _ = m.feed(1, 50);
    let _ = m.feed(1, 90);
    let _ = m.feed(2, 200);
    m
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 零窗
    got[0] = MgMerger::new(0).is_err();
    // 2) 野类型
    let mut m = MgMerger::new(100).expect("w");
    got[1] = m.feed(8, 1).err() == Some(MG_BAD_KIND);
    // 3) 超窗开新槽（同型不并）
    let mut m2 = MgMerger::new(10).expect("w");
    let _ = m2.feed(1, 0);
    let r = m2.feed(1, 100).expect("slot");
    got[2] = r == 1 && m2.slots().len() == 2;
    // 4) 合并计数
    let mut m3 = MgMerger::new(100).expect("w");
    let _ = m3.feed(1, 1);
    let _ = m3.feed(1, 2);
    let _ = m3.feed(1, 3);
    got[3] = m3.merged_away_count() == 2 && m3.slots()[0].count == 3;
    // 5) 印后操作
    let mut m4 = mk_merger();
    m4.seal();
    got[4] = m4.feed(1, 999).err() == Some(MG_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24192_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24192");
    let mut m = mk_merger();

    // J1 槽数：vblank 并一 + hpd ⇒ 2 槽
    let j1 = m.slots().len() == 2;
    s.add("J1-槽数", j1, "2 槽");

    // J2 vblank 计数 3
    let j2 = m.slots()[0].count == 3;
    s.add("J2-同型并数", j2, "并 3 为 1");

    // J3 末时戳：90
    let j3 = m.slots()[0].last_ts == 90;
    s.add("J3-末时戳", j3, "槽记末时戳");

    // J4 合并计数：2 次
    let j4 = m.merged_away_count() == 2;
    s.add("J4-合并计数", j4, "并走 2 件");

    // J5 零窗拒
    let j5 = MgMerger::new(0).is_err();
    s.add("J5-零窗拒", j5, "窗 0 必拒");

    // J6 野类型拒
    let j6 = m.feed(8, 1).err() == Some(MG_BAD_KIND);
    s.add("J6-野类型拒", j6, "kind>7 必拒");

    // J7 异型不并
    let j7 = {
        let mut m2 = MgMerger::new(1000).expect("w");
        let _ = m2.feed(1, 1);
        let r = m2.feed(2, 2).expect("slot");
        r == 1 && m2.slots().len() == 2
    };
    s.add("J7-异型不并", j7, "异型开新槽");

    // J8 超窗不并
    let j8 = {
        let mut m2 = MgMerger::new(50).expect("w");
        let _ = m2.feed(1, 0);
        let r = m2.feed(1, 51).expect("slot");
        r == 1
    };
    s.add("J8-超窗不并", j8, "超窗开新槽");

    // J9 恰窗界并
    let j9 = {
        let mut m2 = MgMerger::new(50).expect("w");
        let _ = m2.feed(1, 0);
        let r = m2.feed(1, 50).expect("slot");
        r == 0
    };
    s.add("J9-恰界并", j9, "差==窗并");

    // J10 计数守恒：槽计数和 − 槽数 = 并走数
    let j10 = {
        let sum: u32 = m.slots().iter().map(|x| x.count).sum();
        (sum as u64) - m.slots().len() as u64 == m.merged_away_count()
    };
    s.add("J10-计数守恒", j10, "并走=Σcount−槽");

    // J11 印后操作拒
    let j11 = {
        let mut m2 = mk_merger();
        m2.seal();
        m2.feed(1, 999).err() == Some(MG_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

    // J12 指纹稳定
    let j12 = {
        let mut m2 = mk_merger();
        let a = m2.fold();
        m2.seal();
        m2.fold() == a
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
    fn unx24192_merge_flow() {
        let mut m = MgMerger::new(100).expect("w");
        assert_eq!(m.feed(1, 10), Ok(0));
        assert_eq!(m.feed(1, 20), Ok(0));
        assert_eq!(m.feed(2, 30), Ok(1));
        assert_eq!(m.feed(8, 40), Err(MG_BAD_KIND));
        assert!(MgMerger::new(0).is_err());
        assert_eq!(m.slots()[0].count, 2);
        assert_eq!(m.slots()[1].count, 1);
        m.seal();
        assert_eq!(m.feed(1, 50), Err(MG_FROZEN));
    }

    #[test]
    fn unx24192_checks_green() {
        let s = run_unx24192_checks();
        assert!(s.all_passed(), "UNX-F24192 判据未全绿");
    }
}
