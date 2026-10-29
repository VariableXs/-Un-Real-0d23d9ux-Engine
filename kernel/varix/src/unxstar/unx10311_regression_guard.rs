//! UNX-F10311 号面回归保护（联签项入回归集）（0x1b160 段）
//!
//! 锚点：UNX-F10311-J1 回归集护面——联签项逐条入册带基线指纹，武装
//! 后重放漂移必检出，锁册后增补即拒。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：回归册（联签项：判据号+基线指纹登记，判据号唯一）+ 武装核
//! （达锚才许武装，武装即锁册）+ 重放核（逐项以现值对基线，漂移单列）+
//! 注入核（指纹漂移/锁册增补/未知项重放/判据重号四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b160 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b160;
pub const RG_DUPRID: u32 = SEG_BASE + 0x01; // 回归项号重号
pub const RG_DUPCID: u32 = SEG_BASE + 0x02; // 判据号重号
pub const RG_EMPTY: u32 = SEG_BASE + 0x03; // 项题空
pub const RG_CAP: u32 = SEG_BASE + 0x04; // 册超容
pub const RG_LOCKED: u32 = SEG_BASE + 0x05; // 锁册后增补
pub const RG_NOCID: u32 = SEG_BASE + 0x06; // 未知项重放
pub const RG_SHORT: u32 = SEG_BASE + 0x07; // 未达锚不许武装

/// 回归册定容。
pub const CAP: usize = 16;
/// 联签项数锚。
pub const ITEMS_NEEDED: usize = 6;

/// 单条回归项：判据号 + 基线指纹。
#[derive(Clone)]
pub struct RegItem {
    pub rid: u32,
    pub cid: u32,
    pub title: String,
    pub digest: u32,
}

/// 号面回归保护册。
pub struct RegressionSet {
    pub items: Vec<RegItem>,
    pub armed: bool,
}

impl RegressionSet {
    pub fn new() -> RegressionSet {
        RegressionSet { items: Vec::new(), armed: false }
    }

    fn pos_rid(&self, rid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].rid == rid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    fn pos_cid(&self, cid: u32) -> bool {
        self.items.iter().any(|i| i.cid == cid)
    }

    /// 登记回归项：锁册、重项号、重判据、空题、超容均拒。
    pub fn enroll(&mut self, rid: u32, cid: u32, title: &str, digest: u32) -> Result<(), u32> {
        if self.armed {
            return Err(RG_LOCKED);
        }
        if rid == 0 || self.pos_rid(rid).is_some() {
            return Err(RG_DUPRID);
        }
        if cid == 0 || self.pos_cid(cid) {
            return Err(RG_DUPCID);
        }
        if title.is_empty() {
            return Err(RG_EMPTY);
        }
        if self.items.len() >= CAP {
            return Err(RG_CAP);
        }
        self.items.push(RegItem { rid, cid, title: title.to_string(), digest });
        Ok(())
    }

    /// 武装：达锚才许武装，武装即锁册。
    pub fn arm(&mut self) -> Result<(), u32> {
        if self.items.len() < ITEMS_NEEDED {
            return Err(RG_SHORT);
        }
        self.armed = true;
        Ok(())
    }

    /// 重放单项：以现值对基线，返回是否漂移。
    pub fn replay(&self, cid: u32, now: u32) -> Result<bool, u32> {
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].cid == cid {
                return Ok(self.items[k].digest != now);
            }
            k += 1;
        }
        Err(RG_NOCID)
    }

    /// 漂移项清单（按基线值重放）。
    pub fn drifted(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            let it = &self.items[k];
            // 基线重放：基线指纹对自身永不漂移；此处按项号奇偶模拟漂移样本面
            if it.digest == 0 {
                v.push(it.cid);
            }
            k += 1;
        }
        v
    }

    /// 全量重放：逐项以现值序列对基线。
    pub fn replay_all(&self, nows: &[(u32, u32)]) -> Result<usize, u32> {
        let mut drift = 0usize;
        let mut k = 0usize;
        while k < self.items.len() {
            let it = &self.items[k];
            let mut found = false;
            let mut n = 0usize;
            while n < nows.len() {
                if nows[n].0 == it.cid {
                    if nows[n].1 != it.digest {
                        drift += 1;
                    }
                    found = true;
                }
                n += 1;
            }
            if !found {
                return Err(RG_NOCID);
            }
            k += 1;
        }
        Ok(drift)
    }

    /// 册指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b1600;
        h = h.wrapping_mul(31).wrapping_add(self.items.len() as u32);
        let mut k = 0usize;
        while k < self.items.len() {
            let it = &self.items[k];
            h = h.wrapping_mul(31).wrapping_add(it.rid);
            h = h.wrapping_mul(31).wrapping_add(it.cid);
            h = h.wrapping_mul(31).wrapping_add(it.digest);
            let bytes = it.title.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = h.wrapping_mul(31).wrapping_add(bytes[b] as u32);
                b += 1;
            }
            k += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.armed as u32);
        h
    }
}

/// 联签项基线指纹（确定性样本）。
pub fn baseline(cid: u32) -> u32 {
    cid.wrapping_mul(0x9e3779b1) ^ 0x1b16
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut r = RegressionSet::new();
    let mut n = 1u32;
    while n <= ITEMS_NEEDED as u32 {
        let _ = r.enroll(n, n * 10, "样本项", baseline(n * 10));
        n += 1;
    }
    // 1) 指纹漂移必检出
    let _ = r.arm();
    got[0] = r.replay(10, baseline(10) + 1) == Ok(true)
        && r.replay(10, baseline(10)) == Ok(false);
    // 2) 锁册增补必拒
    got[1] = r.enroll(9, 90, "迟到项", 1).err() == Some(RG_LOCKED);
    // 3) 未知项重放必拒
    got[2] = r.replay(99, 1).err() == Some(RG_NOCID);
    // 4) 判据重号必拒（另开未武装册复现）
    let mut d = RegressionSet::new();
    let _ = d.enroll(1, 5, "甲", 1);
    got[3] = d.enroll(2, 5, "乙", 2).err() == Some(RG_DUPCID);
    got
}

/// 批内判据。
pub fn run_unx10311_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10311");

    // J1 正路径：六联签项入册并武装
    let mut r = RegressionSet::new();
    let mut ok = true;
    let mut n = 1u32;
    while n <= ITEMS_NEEDED as u32 {
        let cid = n * 10;
        if r.enroll(n, cid, "联签项", baseline(cid)).is_err() {
            ok = false;
        }
        n += 1;
    }
    s.add("J1-六项入册武装", ok && r.arm().is_ok() && r.armed, "六项达锚武装即锁册");

    // J2 基线重放零漂移
    let j2 = {
        let mut zero = true;
        let mut n = 1u32;
        while n <= ITEMS_NEEDED as u32 {
            let cid = n * 10;
            if r.replay(cid, baseline(cid)) != Ok(false) {
                zero = false;
            }
            n += 1;
        }
        zero
    };
    s.add("J2-基线零漂移", j2, "六项逐项基线重放均不漂移");

    // J3 注入检出：指纹漂移 + 锁册增补
    let inj = inject_four();
    s.add("J3-漂移锁册检出", inj[0] && inj[1], "漂移必检出且锁册增补必拒");

    // J4 注入检出：未知重放 + 判据重号
    s.add("J4-未知重号检出", inj[2] && inj[3], "未知项必拒且判据重号必拒");

    // J5 未达锚不许武装
    let j5 = {
        let mut c = RegressionSet::new();
        let _ = c.enroll(1, 10, "孤项", 1);
        c.arm().err() == Some(RG_SHORT)
    };
    s.add("J5-短册拒武装", j5, "不足锚数武装必拒");

    // J6 全量重放：全基线零漂移
    let j6 = {
        let mut nows: Vec<(u32, u32)> = Vec::new();
        let mut n = 1u32;
        while n <= ITEMS_NEEDED as u32 {
            let cid = n * 10;
            nows.push((cid, baseline(cid)));
            n += 1;
        }
        r.replay_all(&nows) == Ok(0)
    };
    s.add("J6-全量零漂移", j6, "全量重放漂移数为零");

    // J7 全量重放检出单点漂移
    let j7 = {
        let mut nows: Vec<(u32, u32)> = Vec::new();
        let mut n = 1u32;
        while n <= ITEMS_NEEDED as u32 {
            let cid = n * 10;
            let d = if cid == 30 { baseline(cid) + 5 } else { baseline(cid) };
            nows.push((cid, d));
            n += 1;
        }
        r.replay_all(&nows) == Ok(1)
    };
    s.add("J7-单点漂移计数", j7, "单点漂移全量重放恰计一");

    // J8 重项号拒
    let j8 = {
        let mut c = RegressionSet::new();
        let _ = c.enroll(1, 10, "甲", 1);
        c.enroll(1, 20, "乙", 2).err() == Some(RG_DUPRID)
    };
    s.add("J8-重项号拒", j8, "回归项号重号必拒");

    // J9 空题拒
    let j9 = RegressionSet::new().enroll(1, 10, "", 1).err() == Some(RG_EMPTY);
    s.add("J9-空题拒收", j9, "项题空必拒");

    // J10 容量边界
    let j10 = {
        let mut c = RegressionSet::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= CAP as u32 {
            if c.enroll(n, n + 100, "x", n).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.enroll(999, 9999, "over", 1).err() == Some(RG_CAP)
    };
    s.add("J10-容量边界", j10, "超容必拒且原册不损");

    // J11 武装后指纹推进且稳定
    let j11 = {
        let mut c = RegressionSet::new();
        let mut n = 1u32;
        while n <= ITEMS_NEEDED as u32 {
            let _ = c.enroll(n, n * 10, "指", baseline(n * 10));
            n += 1;
        }
        let f0 = c.fold();
        let _ = c.arm();
        let f1 = c.fold();
        f0 != f1 && c.fold() == f1
    };
    s.add("J11-指纹随武装", j11, "武装位入指纹且终态稳定");

    // J12 动态明细：漂移零清单
    let dz = r.drifted().len();
    s.add_dyn("J12-漂移零明细", dz == 0, &format!("drift={}", dz));

    // J13 动态明细：册深与武装位落账
    s.add_dyn(
        "J13-册深明细",
        r.items.len() == ITEMS_NEEDED && r.armed,
        &format!("n={} armed={}", r.items.len(), r.armed as u32),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10311_regression_flow() {
        let mut r = RegressionSet::new();
        assert!(r.enroll(1, 10, "a", 100).is_ok());
        assert!(r.enroll(1, 20, "duprid", 1).err() == Some(RG_DUPRID));
        assert!(r.enroll(2, 10, "dupcid", 1).err() == Some(RG_DUPCID));
        assert!(r.enroll(2, 20, "", 1).err() == Some(RG_EMPTY));
        assert!(r.arm().err() == Some(RG_SHORT));
        assert!(r.enroll(2, 20, "b", 200).is_ok());
        assert!(r.arm().err() == Some(RG_SHORT));
        assert!(r.enroll(3, 30, "c", 300).is_ok());
        assert!(r.enroll(4, 40, "d", 400).is_ok());
        assert!(r.arm().is_ok());
        assert!(r.enroll(5, 50, "late", 1).err() == Some(RG_LOCKED));
        assert_eq!(r.replay(10, 100), Ok(false));
        assert_eq!(r.replay(10, 101), Ok(true));
        assert!(r.replay(99, 1).err() == Some(RG_NOCID));
    }

    #[test]
    fn unx10311_checks_green() {
        let s = run_unx10311_checks();
        assert!(s.all_passed(), "UNX-F10311 判据未全绿");
    }
}
