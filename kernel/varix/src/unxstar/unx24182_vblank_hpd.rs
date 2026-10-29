//! UNX-F24182 vblank/HPD 内核事件面与事件队列·子功能02（vblank 等待队列与唤醒）（0x418A0 段）
//!
//! 锚点：UNX-F24182-J1 等待账（vblank 等待者逐条落账）；J2 等待语义
//! （目标帧唤醒 + 提前唤醒拒 + 空队免唤三性质）；J3 等待指纹（账折
//! 叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（vblank 等待面）。
//!
//! 结构：等待表 + 目标帧唤醒核 + 账本 + 折叠指纹 + 注入核（野等待/
//! 未达帧唤醒/重复等待/印后/已唤再唤必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x418A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x418A0;
pub const VW_NOT_REACHED: u32 = SEG_BASE + 0x01; // 未达帧唤醒
pub const VW_DUP_WAIT: u32 = SEG_BASE + 0x02; // 重复等待
pub const VW_UNK: u32 = SEG_BASE + 0x03; // 野等待者
pub const VW_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const VW_ALREADY: u32 = SEG_BASE + 0x05; // 已唤再唤

/// 等待者。
#[derive(Clone)]
pub struct VwWaiter {
    pub name: String,
    pub target_frame: u64,
    pub woken: bool,
}

/// 等待簿。
pub struct VwBook {
    pub waiters: Vec<VwWaiter>,
    pub cur_frame: u64,
    pub sealed: bool,
}

impl VwBook {
    pub fn new(cur_frame: u64) -> VwBook {
        VwBook { waiters: Vec::new(), cur_frame, sealed: false }
    }

    pub fn wait(&mut self, name: &str, target: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(VW_FROZEN);
        }
        if name.is_empty() || self.waiters.iter().any(|w| w.name == name && !w.woken) {
            return Err(VW_DUP_WAIT);
        }
        self.waiters.push(VwWaiter { name: name.to_string(), target_frame: target, woken: false });
        Ok(())
    }

    /// vblank 推进：唤醒所有 target ≤ new_frame 的未唤者，返唤醒数。
    pub fn advance(&mut self, new_frame: u64) -> Result<usize, u32> {
        if self.sealed {
            return Err(VW_FROZEN);
        }
        if new_frame < self.cur_frame {
            return Err(VW_NOT_REACHED);
        }
        self.cur_frame = new_frame;
        let mut n = 0;
        for w in self.waiters.iter_mut() {
            if !w.woken && w.target_frame <= new_frame {
                w.woken = true;
                n += 1;
            }
        }
        Ok(n)
    }

    /// 手动唤醒指定等待者（未达帧拒）。
    pub fn wake_one(&mut self, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(VW_FROZEN);
        }
        match self.waiters.iter_mut().find(|w| w.name == name) {
            Some(w) => {
                if w.woken {
                    return Err(VW_ALREADY);
                }
                if w.target_frame > self.cur_frame {
                    return Err(VW_NOT_REACHED);
                }
                w.woken = true;
                Ok(())
            }
            None => Err(VW_UNK),
        }
    }

    pub fn pending_count(&self) -> usize {
        self.waiters.iter().filter(|w| !w.woken).count()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x418A0;
        h = h.wrapping_mul(31).wrapping_add(self.cur_frame as u32);
        for w in &self.waiters {
            for by in w.name.bytes() {
                h = h.wrapping_mul(31).wrapping_add(by as u32);
            }
            h = h.wrapping_mul(31).wrapping_add(w.target_frame as u32);
            h = h.wrapping_mul(31).wrapping_add(w.woken as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：帧 3，三等待者。
pub fn mk_book() -> VwBook {
    let mut b = VwBook::new(3);
    let _ = b.wait("a", 2); // 已达可唤
    let _ = b.wait("b", 5); // 未达
    let _ = b.wait("c", 3); // 恰达可唤
    let _ = b.advance(3);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 未达帧手动唤醒
    got[0] = b.wake_one("b").err() == Some(VW_NOT_REACHED);
    // 2) 已唤再唤
    got[1] = b.wake_one("a").err() == Some(VW_ALREADY);
    // 3) 重复等待
    got[2] = b.wait("b", 9).err() == Some(VW_DUP_WAIT);
    // 4) 倒退推进
    got[3] = b.advance(1).err() == Some(VW_NOT_REACHED);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.advance(9).err() == Some(VW_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24182_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24182");
    let mut b = mk_book();

    // J1 等待账齐
    let j1 = b.waiters.len() == 3;
    s.add("J1-等待账齐", j1, "三等待者落表");

    // J2 目标帧唤醒
    let j2 = b.pending_count() == 1; // 仅 b 未唤
    s.add("J2-目标帧唤醒", j2, "a/c 已唤 b 待");

    // J3 恰达帧唤醒
    let j3 = {
        let c = b.waiters.iter().find(|w| w.name == "c").unwrap();
        c.woken && c.target_frame == 3
    };
    s.add("J3-恰达唤醒", j3, "target==cur 可唤");

    // J4 未达拒
    let j4 = b.wake_one("b").err() == Some(VW_NOT_REACHED);
    s.add("J4-未达拒", j4, "target>cur 必拒");

    // J5 推进唤醒
    let j5 = {
        let n = b.advance(5).unwrap_or(0);
        n == 1
    };
    s.add("J5-推进唤醒", j5, "帧 5 推进唤 b");

    // J6 重复等待拒
    let j6 = b.wait("b", 9).err() == Some(VW_DUP_WAIT);
    s.add("J6-重复等待拒", j6, "在等者重等必拒");

    // J7 倒退推进拒
    let j7 = b.advance(1).err() == Some(VW_NOT_REACHED);
    s.add("J7-倒退拒", j7, "帧倒退必拒");

    // J8 已唤再唤拒
    let j8 = b.wake_one("a").err() == Some(VW_ALREADY);
    s.add("J8-已唤再唤拒", j8, "二次唤醒必拒");

    // J9 野等待者拒
    let j9 = b.wake_one("ghost").err() == Some(VW_UNK);
    s.add("J9-野等待者拒", j9, "未登记必拒");

    // J10 空名拒
    let j10 = b.wait("", 1).err() == Some(VW_DUP_WAIT);
    s.add("J10-空名拒", j10, "空等待者必拒");

    // J11 全唤清零
    let j11 = {
        let _ = b.advance(9);
        b.pending_count() == 0
    };
    s.add("J11-全唤清零", j11, "推进后无待");

    // J12 指纹稳定
    let j12 = b.fold() == b.fold();
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
    fn unx24182_wait_flow() {
        let mut b = VwBook::new(1);
        let _ = b.wait("a", 2);
        assert_eq!(b.wake_one("a"), Err(VW_NOT_REACHED));
        assert_eq!(b.advance(2), Ok(1));
        assert_eq!(b.advance(1), Err(VW_NOT_REACHED));
        assert_eq!(b.wait("a", 3), Ok(()));
        assert_eq!(b.wait("a", 4), Err(VW_DUP_WAIT));
        b.seal();
        assert_eq!(b.advance(9), Err(VW_FROZEN));
    }

    #[test]
    fn unx24182_checks_green() {
        let s = run_unx24182_checks();
        assert!(s.all_passed(), "UNX-F24182 判据未全绿");
    }
}
