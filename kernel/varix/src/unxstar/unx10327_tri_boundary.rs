//! UNX-F10327 失绿分界规则（三面划界）（0x1b360 段）
//!
//! 锚点：UNX-F10327-J1 三面划界归因——界以（面号, 判据号）对枚举，
//! 界外失绿不得报入，界内失绿逐件归主（主面 0..2），全部事件归主齐
//! 才算划界核清。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：界册（面号×判据号对，界内唯一）+ 事件账（事件号+所在对+
//! 归主 Option）+ 归因核（界外拒报、未报拒归、重归即拒）+ 注入核
//! （零号/重对/界外报告/未知事件归因四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b360 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b360;
pub const BD_ZERO: u32 = SEG_BASE + 0x01; // 号为零
pub const BD_DUP: u32 = SEG_BASE + 0x02; // 重对/重事件
pub const BD_OUTSIDE: u32 = SEG_BASE + 0x03; // 界外报告
pub const BD_NOEV: u32 = SEG_BASE + 0x04; // 未知事件
pub const BD_REATTR: u32 = SEG_BASE + 0x05; // 已归再归
pub const BD_CAP: u32 = SEG_BASE + 0x06; // 账超容
pub const BD_FACE: u32 = SEG_BASE + 0x07; // 面号越界

/// 三面数（开区间）。
pub const FACE_N: u8 = 3;
/// 界册定容。
pub const SCOPE_CAP: usize = 16;
/// 事件账定容。
pub const EV_CAP: usize = 24;

/// 界内对。
#[derive(Clone)]
pub struct ScopePair {
    pub face: u8,
    pub cid: u32,
}

/// 失绿事件。
#[derive(Clone)]
pub struct Fault {
    pub ev: u32,
    pub face: u8,
    pub cid: u32,
    pub owner: Option<u8>,
}

/// 三面划界归因核。
pub struct TriBoundary {
    pub scope: Vec<ScopePair>,
    pub events: Vec<Fault>,
    pub sealed: bool,
}

impl TriBoundary {
    pub fn new() -> TriBoundary {
        TriBoundary { scope: Vec::new(), events: Vec::new(), sealed: false }
    }

    fn scoped(&self, face: u8, cid: u32) -> bool {
        let mut k = 0usize;
        while k < self.scope.len() {
            if self.scope[k].face == face && self.scope[k].cid == cid {
                return true;
            }
            k += 1;
        }
        false
    }

    fn ev_pos(&self, ev: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.events.len() {
            if self.events[k].ev == ev {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 登记界内对。
    pub fn enroll_scope(&mut self, face: u8, cid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(BD_REATTR);
        }
        if face >= FACE_N {
            return Err(BD_FACE);
        }
        if cid == 0 {
            return Err(BD_ZERO);
        }
        if self.scoped(face, cid) {
            return Err(BD_DUP);
        }
        if self.scope.len() >= SCOPE_CAP {
            return Err(BD_CAP);
        }
        self.scope.push(ScopePair { face, cid });
        Ok(())
    }

    /// 界内判定。
    pub fn in_scope(&self, face: u8, cid: u32) -> bool {
        self.scoped(face, cid)
    }

    /// 报失绿事件：界外必拒。
    pub fn report(&mut self, ev: u32, face: u8, cid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(BD_REATTR);
        }
        if ev == 0 {
            return Err(BD_ZERO);
        }
        if face >= FACE_N {
            return Err(BD_FACE);
        }
        if self.ev_pos(ev).is_some() {
            return Err(BD_DUP);
        }
        if !self.scoped(face, cid) {
            return Err(BD_OUTSIDE);
        }
        if self.events.len() >= EV_CAP {
            return Err(BD_CAP);
        }
        self.events.push(Fault { ev, face, cid, owner: None });
        Ok(())
    }

    /// 归主：主面 0..2，未报拒归、重归即拒。
    pub fn attribute(&mut self, ev: u32, owner: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(BD_REATTR);
        }
        if owner >= FACE_N {
            return Err(BD_FACE);
        }
        match self.ev_pos(ev) {
            None => Err(BD_NOEV),
            Some(p) => {
                if self.events[p].owner.is_some() {
                    return Err(BD_REATTR);
                }
                self.events[p].owner = Some(owner);
                Ok(())
            }
        }
    }

    /// 未归事件清单。
    pub fn unattributed(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.events.len() {
            if self.events[k].owner.is_none() {
                v.push(self.events[k].ev);
            }
            k += 1;
        }
        v
    }

    /// 全部事件归主齐。
    pub fn all_ruled(&self) -> bool {
        !self.events.is_empty() && self.unattributed().is_empty()
    }

    /// 某主面事件数。
    pub fn owner_count(&self, owner: u8) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.events.len() {
            if self.events[k].owner == Some(owner) {
                n += 1;
            }
            k += 1;
        }
        n
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 划界指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b3600;
        h = h.wrapping_mul(31).wrapping_add(self.scope.len() as u32);
        let mut k = 0usize;
        while k < self.scope.len() {
            h = h
                .wrapping_mul(31)
                .wrapping_add(self.scope[k].face as u32)
                .wrapping_mul(31)
                .wrapping_add(self.scope[k].cid);
            k += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.events.len() as u32);
        k = 0;
        while k < self.events.len() {
            let e = &self.events[k];
            h = h
                .wrapping_mul(31)
                .wrapping_add(e.ev)
                .wrapping_mul(31)
                .wrapping_add(e.owner.unwrap_or(9) as u32 + 1);
            k += 1;
        }
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut b = TriBoundary::new();
    // 1) 零判据号与坏面号必拒
    got[0] = b.enroll_scope(0, 0).err() == Some(BD_ZERO)
        && b.enroll_scope(3, 1).err() == Some(BD_FACE);
    // 2) 重对必拒
    let _ = b.enroll_scope(0, 5);
    got[1] = b.enroll_scope(0, 5).err() == Some(BD_DUP);
    // 3) 界外报告必拒
    got[2] = b.report(1, 1, 7).err() == Some(BD_OUTSIDE);
    // 4) 未知事件归因必拒
    got[3] = b.attribute(99, 0).err() == Some(BD_NOEV);
    got
}

/// 批内判据。
pub fn run_unx10327_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10327");

    // J1 正路径：三面划界、界内报告、逐件归主
    let mut b = TriBoundary::new();
    let mut ok = true;
    // 界：pty 面 1..3、进程面 4..6、基建面 7..9
    let mut cid = 1u32;
    while cid <= 9u32 {
        let face = ((cid - 1) / 3) as u8;
        if b.enroll_scope(face, cid).is_err() {
            ok = false;
        }
        cid += 1;
    }
    // 事件：每面各两件
    let evs = [(0u8, 1u32), (0, 2), (1, 4), (1, 5), (2, 7), (2, 8)];
    let mut k = 0usize;
    while k < evs.len() {
        if b.report(k as u32 + 1, evs[k].0, evs[k].1).is_err() {
            ok = false;
        }
        k += 1;
    }
    k = 0;
    while k < evs.len() {
        if b.attribute(k as u32 + 1, evs[k].0).is_err() {
            ok = false;
        }
        k += 1;
    }
    s.add(
        "J1-三面划界归因",
        ok && b.all_ruled() && b.unattributed().is_empty(),
        "九对界六事件全归主且零未归",
    );

    // J2 界内判定逐对核
    let j2 = b.in_scope(0, 1) && b.in_scope(1, 4) && b.in_scope(2, 9)
        && !b.in_scope(0, 9) && !b.in_scope(2, 1);
    s.add("J2-界内判定", j2, "界内对命中、跨面错对不命中");

    // J3 注入检出：零号坏面 + 重对
    let inj = inject_four();
    s.add("J3-零号重对检出", inj[0] && inj[1], "零判据号与重对必拒");

    // J4 注入检出：界外报告 + 未知事件
    s.add("J4-界外未知检出", inj[2] && inj[3], "界外报告与未知事件归因必拒");

    // J5 重事件必拒
    let j5 = {
        let mut t = TriBoundary::new();
        let _ = t.enroll_scope(0, 3);
        let _ = t.report(1, 0, 3);
        t.report(1, 0, 3).err() == Some(BD_DUP)
    };
    s.add("J5-重事件拒收", j5, "事件号重号必拒");

    // J6 重归因必拒
    let j6 = {
        let mut t = TriBoundary::new();
        let _ = t.enroll_scope(1, 2);
        let _ = t.report(1, 1, 2);
        let _ = t.attribute(1, 1);
        t.attribute(1, 2).err() == Some(BD_REATTR)
    };
    s.add("J6-重归因拒收", j6, "事件归主只许一次");

    // J7 坏主面必拒
    let j7 = {
        let mut t = TriBoundary::new();
        let _ = t.enroll_scope(2, 4);
        let _ = t.report(1, 2, 4);
        t.attribute(1, 3).err() == Some(BD_FACE)
    };
    s.add("J7-坏主面拒收", j7, "主面号越界必拒");

    // J8 未归阻断判清
    let j8 = {
        let mut t = TriBoundary::new();
        let _ = t.enroll_scope(0, 1);
        let _ = t.report(1, 0, 1);
        let _ = t.report(2, 0, 1);
        let _ = t.attribute(1, 0);
        !t.all_ruled() && t.unattributed() == [2u32]
    };
    s.add("J8-未归不清", j8, "余一件未归即不算判清");

    // J9 容量边界（界册）
    let j9 = {
        let mut t = TriBoundary::new();
        let mut n = 0u32;
        let mut filled = true;
        while (n as usize) < SCOPE_CAP {
            let face = (n % 3) as u8;
            if t.enroll_scope(face, n + 1).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && t.enroll_scope(0, 999).err() == Some(BD_CAP)
    };
    s.add("J9-界册容量", j9, "界册超容必拒且原册不损");

    // J10 空事件不算判清
    let j10 = !TriBoundary::new().all_ruled();
    s.add("J10-空事件不清", j10, "无事件不视作判清");

    // J11 指纹随归因推进
    let j11 = {
        let mut t = TriBoundary::new();
        let _ = t.enroll_scope(0, 1);
        let _ = t.report(1, 0, 1);
        let f0 = t.fold();
        let _ = t.attribute(1, 0);
        let f1 = t.fold();
        f0 != f1 && t.fold() == f1
    };
    s.add("J11-指纹推进", j11, "归因一步指纹一步且终态稳定");

    // J12 动态明细：主面事件分布
    let o0 = b.owner_count(0);
    let o1 = b.owner_count(1);
    let o2 = b.owner_count(2);
    s.add_dyn(
        "J12-归主分布",
        o0 == 2 && o1 == 2 && o2 == 2,
        &format!("o0={} o1={} o2={}", o0, o1, o2),
    );

    // J13 动态明细：划界指纹落账
    let fp = b.fold();
    s.add_dyn("J13-划界指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10327_boundary_flow() {
        let mut b = TriBoundary::new();
        assert!(b.enroll_scope(0, 0).err() == Some(BD_ZERO));
        assert!(b.enroll_scope(5, 1).err() == Some(BD_FACE));
        assert!(b.enroll_scope(0, 3).is_ok());
        assert!(b.enroll_scope(0, 3).err() == Some(BD_DUP));
        assert!(b.report(1, 1, 9).err() == Some(BD_OUTSIDE));
        assert!(b.report(0, 0, 3).err() == Some(BD_ZERO));
        assert!(b.report(1, 0, 3).is_ok());
        assert!(b.report(1, 0, 3).err() == Some(BD_DUP));
        assert!(b.attribute(99, 0).err() == Some(BD_NOEV));
        assert!(b.attribute(1, 3).err() == Some(BD_FACE));
        assert!(b.attribute(1, 0).is_ok());
        assert!(b.attribute(1, 1).err() == Some(BD_REATTR));
        assert!(b.all_ruled());
        b.seal();
        assert!(b.report(2, 0, 3).err() == Some(BD_REATTR));
    }

    #[test]
    fn unx10327_checks_green() {
        let s = run_unx10327_checks();
        assert!(s.all_passed(), "UNX-F10327 判据未全绿");
    }
}
