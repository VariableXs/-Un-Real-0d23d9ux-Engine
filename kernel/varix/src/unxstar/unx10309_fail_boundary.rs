//! UNX-F10309 失绿分界规则（界内/界外归因划界）（0x1b120 段）
//!
//! 锚点：UNX-F10309-J1 归因划界——失绿事件逐条裁界内/界外，界内归
//! 联签责任、界外归域外责任，事件与裁断一一对应零漏裁。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：界册（联签界内判据号清单）+ 事件册（失绿事件：判据号+侧别）+
//! 归因核（逐事件裁界内/界外，重裁即拒）+ 注入核（界外漏判/重裁/
//! 侧别非法/事件号重号四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b120 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b120;
pub const FB_DUP: u32 = SEG_BASE + 0x01; // 界内判据号重登
pub const FB_ZERO: u32 = SEG_BASE + 0x02; // 判据号为零
pub const FB_CAP: u32 = SEG_BASE + 0x03; // 册超容
pub const FB_DUPFID: u32 = SEG_BASE + 0x04; // 事件号重号
pub const FB_BADSIDE: u32 = SEG_BASE + 0x05; // 侧别非法
pub const FB_NOFID: u32 = SEG_BASE + 0x06; // 未知事件归因
pub const FB_RERULED: u32 = SEG_BASE + 0x07; // 已裁再裁

/// 侧别：1=甲侧失绿 2=乙侧失绿。
pub const SIDE_A: u8 = 1;
pub const SIDE_B: u8 = 2;

/// 界册定容。
pub const CAP: usize = 16;
/// 失绿事件数锚。
pub const EVENTS_NEEDED: usize = 6;

/// 单条失绿事件。
#[derive(Clone)]
pub struct FailEvt {
    pub fid: u32,
    pub crit: u32,
    pub side: u8,
}

/// 失绿分界核。
pub struct BoundaryJudge {
    pub scope: Vec<u32>,
    pub events: Vec<FailEvt>,
    pub ruled: Vec<(u32, bool)>,
}

impl BoundaryJudge {
    pub fn new() -> BoundaryJudge {
        BoundaryJudge { scope: Vec::new(), events: Vec::new(), ruled: Vec::new() }
    }

    /// 登记联签界内判据号：零号、重登、超容均拒。
    pub fn enroll_scope(&mut self, cid: u32) -> Result<(), u32> {
        if cid == 0 {
            return Err(FB_ZERO);
        }
        if self.scope.contains(&cid) {
            return Err(FB_DUP);
        }
        if self.scope.len() >= CAP {
            return Err(FB_CAP);
        }
        self.scope.push(cid);
        Ok(())
    }

    /// 界内判定。
    pub fn in_scope(&self, cid: u32) -> bool {
        self.scope.contains(&cid)
    }

    /// 上报失绿事件：重号、侧别非法、超容均拒。
    pub fn report(&mut self, fid: u32, crit: u32, side: u8) -> Result<(), u32> {
        if fid == 0 || self.events.iter().any(|e| e.fid == fid) {
            return Err(FB_DUPFID);
        }
        if side != SIDE_A && side != SIDE_B {
            return Err(FB_BADSIDE);
        }
        if self.events.len() >= CAP {
            return Err(FB_CAP);
        }
        self.events.push(FailEvt { fid, crit, side });
        Ok(())
    }

    fn pos(&self, fid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.events.len() {
            if self.events[k].fid == fid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 归因裁断：界内真/界外假；未知事件与重裁均拒。
    pub fn attribute(&mut self, fid: u32) -> Result<bool, u32> {
        if self.ruled.iter().any(|r| r.0 == fid) {
            return Err(FB_RERULED);
        }
        match self.pos(fid) {
            None => Err(FB_NOFID),
            Some(k) => {
                let inside = self.in_scope(self.events[k].crit);
                self.ruled.push((fid, inside));
                Ok(inside)
            }
        }
    }

    /// 全裁断：事件数与裁断数一一对应。
    pub fn all_ruled(&self) -> bool {
        self.events.len() == self.ruled.len()
            && self.events.iter().all(|e| self.ruled.iter().any(|r| r.0 == e.fid))
    }

    /// 界内归因计数。
    pub fn inside_count(&self) -> usize {
        self.ruled.iter().filter(|r| r.1).count()
    }

    /// 界外归因计数。
    pub fn outside_count(&self) -> usize {
        self.ruled.iter().filter(|r| !r.1).count()
    }

    /// 界内事件清单（按登记序）。
    pub fn inside_ids(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.events.len() {
            if self.in_scope(self.events[k].crit) {
                v.push(self.events[k].fid);
            }
            k += 1;
        }
        v
    }

    /// 界外事件清单（按登记序）。
    pub fn outside_ids(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.events.len() {
            if !self.in_scope(self.events[k].crit) {
                v.push(self.events[k].fid);
            }
            k += 1;
        }
        v
    }

    /// 划界指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b1200;
        h = h.wrapping_mul(31).wrapping_add(self.scope.len() as u32);
        let mut k = 0usize;
        while k < self.scope.len() {
            h = h.wrapping_mul(31).wrapping_add(self.scope[k]);
            k += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.events.len() as u32);
        let mut e = 0usize;
        while e < self.events.len() {
            h = h.wrapping_mul(31).wrapping_add(self.events[e].fid);
            h = h.wrapping_mul(31).wrapping_add(self.events[e].crit);
            h = h.wrapping_mul(31).wrapping_add(self.events[e].side as u32);
            e += 1;
        }
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 界外判据失绿必归界外（漏判即栽赃联签）
    let mut j = BoundaryJudge::new();
    let _ = j.enroll_scope(10);
    let _ = j.report(1, 10, SIDE_A);
    let _ = j.report(2, 20, SIDE_A);
    got[0] = j.attribute(1) == Ok(true) && j.attribute(2) == Ok(false);
    // 2) 重裁必拒
    got[1] = j.attribute(1).err() == Some(FB_RERULED);
    // 3) 侧别非法必拒
    got[2] = j.report(3, 10, 7).err() == Some(FB_BADSIDE);
    // 4) 未知事件归因必拒
    got[3] = j.attribute(99).err() == Some(FB_NOFID);
    got
}

/// 批内判据。
pub fn run_unx10309_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10309");

    // J1 正路径：界内三条界外三条逐条归因
    let mut j = BoundaryJudge::new();
    let mut ok = true;
    let mut n = 1u32;
    while n <= 3 {
        if j.enroll_scope(n).is_err() {
            ok = false;
        }
        n += 1;
    }
    let _ = j.report(1, 1, SIDE_A);
    let _ = j.report(2, 2, SIDE_B);
    let _ = j.report(3, 3, SIDE_A);
    let _ = j.report(4, 10, SIDE_A);
    let _ = j.report(5, 20, SIDE_B);
    let _ = j.report(6, 30, SIDE_A);
    let mut m = 1u32;
    while m <= 6 {
        if j.attribute(m).is_err() {
            ok = false;
        }
        m += 1;
    }
    s.add("J1-六事件归因", ok && j.all_ruled() && j.inside_count() == 3 && j.outside_count() == 3,
        "界内三条界外三条逐条裁断齐备");

    // J2 界内事件清单恰为前三条
    let j2 = j.inside_ids() == [1u32, 2, 3];
    s.add("J2-界内清单", j2, "界内事件按登记序枚举");

    // J3 注入检出：界外漏判 + 重裁
    let inj = inject_four();
    s.add("J3-界外重裁检出", inj[0] && inj[1], "界外必归界外且重裁必拒");

    // J4 注入检出：侧别非法 + 未知事件
    s.add("J4-侧别未知检出", inj[2] && inj[3], "侧别非法必拒且未知事件必拒");

    // J5 界内重登拒
    let j5 = {
        let mut c = BoundaryJudge::new();
        let _ = c.enroll_scope(5);
        c.enroll_scope(5).err() == Some(FB_DUP)
    };
    s.add("J5-界内重登拒", j5, "判据号重登界册必拒");

    // J6 零号拒
    let j6 = BoundaryJudge::new().enroll_scope(0).err() == Some(FB_ZERO);
    s.add("J6-零号拒收", j6, "界内零号必拒");

    // J7 事件号重号拒
    let j7 = {
        let mut c = BoundaryJudge::new();
        let _ = c.report(1, 1, SIDE_A);
        c.report(1, 2, SIDE_B).err() == Some(FB_DUPFID)
    };
    s.add("J7-事件重号拒", j7, "事件号重号必拒");

    // J8 容量边界
    let j8 = {
        let mut c = BoundaryJudge::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= CAP as u32 {
            if c.enroll_scope(n).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.enroll_scope(999).err() == Some(FB_CAP)
    };
    s.add("J8-容量边界", j8, "界册超容必拒且原册不损");

    // J9 半裁不算全裁：只裁三条时 all_ruled 为假
    let j9 = {
        let mut c = BoundaryJudge::new();
        let _ = c.report(1, 1, SIDE_A);
        let _ = c.report(2, 9, SIDE_B);
        let _ = c.attribute(1);
        !c.all_ruled()
    };
    s.add("J9-半裁不成账", j9, "漏裁事件即划界未闭");

    // J10 归因计数与清单自洽
    let j10 = j.inside_count() + j.outside_count() == j.ruled.len()
        && j.ruled.len() == EVENTS_NEEDED;
    s.add("J10-计数自洽", j10, "界内+界外恰等于裁断总数");

    // J11 划界指纹稳定
    let j11 = j.fold() == j.fold() && j.fold() != 0;
    s.add("J11-指纹稳定", j11, "划界折叠指纹两次一致");

    // J12 动态明细：界内/界外计数落账
    s.add_dyn(
        "J12-归因计数",
        j.inside_count() == 3 && j.outside_count() == 3,
        &format!("in={} out={}", j.inside_count(), j.outside_count()),
    );

    // J13 动态明细：界册深度落账
    let sc = j.scope.len();
    s.add_dyn("J13-界册深度", sc == 3, &format!("scope={}", sc));

    // J14 动态明细：界外清单对账
    let out = j.outside_ids();
    s.add_dyn(
        "J14-界外清单",
        out == [4u32, 5, 6],
        &format!("界外事件 {:?}", out),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10309_boundary_flow() {
        let mut j = BoundaryJudge::new();
        assert!(j.enroll_scope(1).is_ok());
        assert!(j.enroll_scope(1).err() == Some(FB_DUP));
        assert!(j.enroll_scope(0).err() == Some(FB_ZERO));
        assert!(j.report(1, 1, SIDE_A).is_ok());
        assert!(j.report(1, 2, SIDE_B).err() == Some(FB_DUPFID));
        assert!(j.report(2, 1, 9).err() == Some(FB_BADSIDE));
        assert_eq!(j.attribute(1), Ok(true));
        assert!(j.attribute(1).err() == Some(FB_RERULED));
        assert!(j.attribute(9).err() == Some(FB_NOFID));
        assert!(j.report(2, 99, SIDE_B).is_ok());
        assert!(!j.all_ruled());
        assert_eq!(j.attribute(2), Ok(false));
        assert!(j.all_ruled());
    }

    #[test]
    fn unx10309_checks_green() {
        let s = run_unx10309_checks();
        assert!(s.all_passed(), "UNX-F10309 判据未全绿");
    }
}
