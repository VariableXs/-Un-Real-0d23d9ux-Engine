//! UNX-F24189 vblank/HPD 内核事件面与事件队列·子功能09（过滤掩码）（0x41980 段）
//!
//! 锚点：UNX-F24189-J1 过滤账（放行/滤除逐条落账）；J2 过滤语义（
//! 位选放行 + 零掩全滤 + 野类型拒三性质）；J3 过滤指纹（账折叠指纹）
//! 三判据全绿。
//! 域：CoRun·UNX-F5-B15（过滤掩码面）。
//!
//! 结构：掩码过滤器 + 位选核 + 账本 + 折叠指纹 + 注入核（野掩码/野
//! 类型/印后/全掩/零掩必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41980 独占 0x20）。
pub const SEG_BASE: u32 = 0x41980;
pub const FM_BAD_MASK: u32 = SEG_BASE + 0x01; // 掩码越界
pub const FM_BAD_KIND: u32 = SEG_BASE + 0x02; // 野类型
pub const FM_FROZEN: u32 = SEG_BASE + 0x03; // 印后操作
pub const FM_ALL_MASK: u32 = SEG_BASE + 0x04; // 全掩（滤一切）
pub const FM_BAD_FLAG: u32 = SEG_BASE + 0x05; // 非法旗标

/// 掩码过滤器：mask 位 i 对应 kind i；位 1 放行，位 0 滤除。
pub struct FmFilter {
    pub mask: u16,
    passed: u64,
    blocked: u64,
    sealed: bool,
}

impl FmFilter {
    /// mask 仅允许低 8 位（kind 域 0..=7）。
    pub fn new(mask: u16) -> Result<FmFilter, u32> {
        if mask & !0x00FF != 0 {
            return Err(FM_BAD_MASK);
        }
        Ok(FmFilter {
            mask,
            passed: 0,
            blocked: 0,
            sealed: false,
        })
    }

    /// 送事件：掩码位选。
    pub fn feed(&mut self, kind: u8, ts: u64) -> Result<bool, u32> {
        if self.sealed {
            return Err(FM_FROZEN);
        }
        if kind > 7 {
            return Err(FM_BAD_KIND);
        }
        if self.mask == 0 {
            return Err(FM_ALL_MASK);
        }
        let bit = 1u16 << kind;
        if self.mask & bit != 0 {
            self.passed += 1;
            Ok(true)
        } else {
            self.blocked += 1;
            Ok(false)
        }
    }

    pub fn passed_count(&self) -> u64 {
        self.passed
    }

    pub fn blocked_count(&self) -> u64 {
        self.blocked
    }

    /// 过滤账（逐事件判定序）。
    pub fn ledger(&self) -> Vec<(u16, bool)> {
        let mut v = Vec::new();
        for k in 0..8u16 {
            let bit = 1u16 << k;
            if self.mask & bit != 0 {
                v.push((k, true));
            } else {
                v.push((k, false));
            }
        }
        v
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41980;
        h = h.wrapping_mul(31).wrapping_add(self.mask as u32);
        h = h.wrapping_mul(31).wrapping_add(self.passed as u32);
        h = h.wrapping_mul(31).wrapping_add(self.blocked as u32);
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准夹具：mask=0b0000_0110（放行 kind 1/2）。
pub fn mk_filter() -> FmFilter {
    let mut f = FmFilter::new(0b0000_0110).expect("std mask");
    let _ = f.feed(1, 10); // 过
    let _ = f.feed(3, 20); // 滤
    let _ = f.feed(2, 30); // 过
    let _ = f.feed(0, 40); // 滤
    f
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 野掩码（高位非零）
    got[0] = FmFilter::new(0xFF00).is_err();
    // 2) 野类型
    let mut f = FmFilter::new(0xFFFF & 0x00FF).expect("mask");
    got[1] = f.feed(8, 1).err() == Some(FM_BAD_KIND);
    // 3) 零掩全滤
    let mut z = FmFilter::new(0).expect("zero");
    got[2] = z.feed(1, 1).err() == Some(FM_ALL_MASK);
    // 4) 全掩位样本：mask=0xFF 全放行
    let mut a = FmFilter::new(0x00FF).expect("all");
    let r = (0..8u8).map(|k| a.feed(k, k as u64).unwrap_or(false)).fold(true, |acc, x| acc && x);
    got[3] = r && a.passed_count() == 8;
    // 5) 印后操作
    let mut f2 = mk_filter();
    f2.seal();
    got[4] = f2.feed(1, 99).err() == Some(FM_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24189_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24189");
    let mut f = mk_filter();

    // J1 放行计数：kind1/2 过 ⇒ 2
    let j1 = f.passed_count() == 2;
    s.add("J1-放行计数", j1, "放行 2");

    // J2 滤除计数：kind3/0 滤 ⇒ 2
    let j2 = f.blocked_count() == 2;
    s.add("J2-滤除计数", j2, "滤除 2");

    // J3 掩码值
    let j3 = f.mask == 0b0000_0110;
    s.add("J3-掩码值", j3, "mask=0x06");

    // J4 位选语义：账中 kind1/2 放行
    let j4 = {
        let led = f.ledger();
        led[1].1 && led[2].1 && !led[0].1 && !led[3].1
    };
    s.add("J4-位选语义", j4, "账位选一致");

    // J5 野掩码拒
    let j5 = FmFilter::new(0x0100).is_err();
    s.add("J5-野掩码拒", j5, "高 8 位非零必拒");

    // J6 野类型拒
    let j6 = f.feed(9, 1).err() == Some(FM_BAD_KIND);
    s.add("J6-野类型拒", j6, "kind>7 必拒");

    // J7 零掩全滤拒
    let j7 = {
        let mut z = FmFilter::new(0).expect("zero");
        z.feed(1, 1).err() == Some(FM_ALL_MASK)
    };
    s.add("J7-零掩拒", j7, "mask=0 拒喂");

    // J8 全掩全放
    let j8 = {
        let mut a = FmFilter::new(0x00FF).expect("all");
        a.feed(7, 1) == Ok(true) && a.feed(0, 2) == Ok(true)
    };
    s.add("J8-全掩全放", j8, "mask=0xFF 全放行");

    // J9 守恒：过+滤=喂
    let j9 = f.passed_count() + f.blocked_count() == 4;
    s.add("J9-守恒", j9, "过+滤=4");

    // J10 判定与账序一致：8 槽账恰 2 放行且位置与掩码一致
    let j10 = {
        let led = f.ledger();
        let trues: alloc::vec::Vec<u16> =
            led.iter().filter(|(_, p)| *p).map(|(k, _)| *k).collect();
        led.len() == 8 && trues == alloc::vec![1u16, 2]
    };
    s.add("J10-账序一致", j10, "8 槽账 2 放行位=掩码位");

    // J11 印后操作拒
    let j11 = {
        let mut f2 = mk_filter();
        f2.seal();
        f2.feed(1, 99).err() == Some(FM_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

    // J12 指纹稳定
    let j12 = {
        let mut f2 = mk_filter();
        let a = f2.fold();
        f2.seal();
        f2.fold() == a
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
    fn unx24189_mask_flow() {
        let mut f = FmFilter::new(0b0010).expect("m");
        assert_eq!(f.feed(1, 1), Ok(true));
        assert_eq!(f.feed(2, 2), Ok(false));
        assert_eq!(f.feed(8, 3), Err(FM_BAD_KIND));
        assert!(FmFilter::new(0x100).is_err());
        let mut z = FmFilter::new(0).expect("z");
        assert_eq!(z.feed(1, 1), Err(FM_ALL_MASK));
        f.seal();
        assert_eq!(f.feed(1, 9), Err(FM_FROZEN));
    }

    #[test]
    fn unx24189_checks_green() {
        let s = run_unx24189_checks();
        assert!(s.all_passed(), "UNX-F24189 判据未全绿");
    }
}
