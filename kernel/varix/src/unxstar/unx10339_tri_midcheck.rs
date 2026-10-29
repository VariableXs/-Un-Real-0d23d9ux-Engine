//! UNX-F10339 联签面中检（十八项对平）（0x1b4e0 段）
//!
//! 锚点：UNX-F10339-J1 三面网格对平——十八个中检槽逐槽由 pty/进程/
//! 基建三面各报名册指纹，三报全齐且彼此相等才算该槽对平，十八槽全平
//! 才算联签面中检过。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：网格册（槽=槽号+项名+三面指纹位）+ 三报核（重报即拒、未知槽
//! 即拒）+ 对平核（三报全等）+ 注入核（坏面号/重槽/异报/单漏四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b4e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b4e0;
pub const TM_ZERO: u32 = SEG_BASE + 0x01; // 槽号零
pub const TM_DUP: u32 = SEG_BASE + 0x02; // 槽号重号
pub const TM_NOSLOT: u32 = SEG_BASE + 0x03; // 未知槽
pub const TM_REREPORT: u32 = SEG_BASE + 0x04; // 已报再报
pub const TM_CAP: u32 = SEG_BASE + 0x05; // 册超容
pub const TM_EMPTY: u32 = SEG_BASE + 0x06; // 项名空
pub const TM_SEALED: u32 = SEG_BASE + 0x07; // 封检后报
pub const TM_FACE: u32 = SEG_BASE + 0x08; // 面号越界

/// 三面数（开区间）。
pub const FACE_N: u8 = 3;
/// 中检槽锚（十八槽）。
pub const SLOTS_NEEDED: usize = 18;
/// 册定容。
pub const CAP: usize = 24;

/// 槽指纹（FNV-1a 32 位，序敏感）。
pub fn fdigest(name: &str) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    let bytes = name.as_bytes();
    let mut k = 0usize;
    while k < bytes.len() {
        h ^= bytes[k] as u32;
        h = h.wrapping_mul(0x01000193);
        k += 1;
    }
    h
}

/// 网格槽行。
#[derive(Clone)]
pub struct GridRow {
    pub slot: u32,
    pub name: String,
    pub digs: [Option<u32>; 3],
}

impl GridRow {
    /// 三报齐备。
    pub fn full(&self) -> bool {
        let mut f = 0usize;
        while f < FACE_N as usize {
            if self.digs[f].is_none() {
                return false;
            }
            f += 1;
        }
        true
    }

    /// 对平：三报全齐且全等。
    pub fn agree(&self) -> bool {
        self.full() && self.digs[0] == self.digs[1] && self.digs[1] == self.digs[2]
    }
}

/// 联签面中检网格。
pub struct TriMidCheck {
    pub rows: Vec<GridRow>,
    pub sealed: bool,
}

impl TriMidCheck {
    pub fn new() -> TriMidCheck {
        TriMidCheck { rows: Vec::new(), sealed: false }
    }

    fn pos(&self, slot: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].slot == slot {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 开槽。
    pub fn enroll(&mut self, slot: u32, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(TM_SEALED);
        }
        if slot == 0 {
            return Err(TM_ZERO);
        }
        if self.pos(slot).is_some() {
            return Err(TM_DUP);
        }
        if name.is_empty() {
            return Err(TM_EMPTY);
        }
        if self.rows.len() >= CAP {
            return Err(TM_CAP);
        }
        self.rows.push(GridRow {
            slot,
            name: name.to_string(),
            digs: [None, None, None],
        });
        Ok(())
    }

    /// 面报指纹。
    pub fn face_report(&mut self, slot: u32, face: u8, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(TM_SEALED);
        }
        if face >= FACE_N {
            return Err(TM_FACE);
        }
        match self.pos(slot) {
            None => Err(TM_NOSLOT),
            Some(p) => {
                if self.rows[p].digs[face as usize].is_some() {
                    return Err(TM_REREPORT);
                }
                self.rows[p].digs[face as usize] = Some(fdigest(name));
                Ok(())
            }
        }
    }

    /// 异报槽清单（三报齐但存在不等）。
    pub fn mismatched(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.rows.len() {
            let r = &self.rows[k];
            if r.full() && !r.agree() {
                v.push(r.slot);
            }
            k += 1;
        }
        v
    }

    /// 单漏槽清单（任一面未报）。
    pub fn half_reported(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.rows.len() {
            if !self.rows[k].full() {
                v.push(self.rows[k].slot);
            }
            k += 1;
        }
        v
    }

    /// 中检过：十八槽齐、全三报、零异报。
    pub fn flat(&self) -> bool {
        self.rows.len() == SLOTS_NEEDED
            && self.half_reported().is_empty()
            && self.mismatched().is_empty()
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 网格指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b4e00;
        h = h.wrapping_mul(31).wrapping_add(self.rows.len() as u32);
        let mut k = 0usize;
        while k < self.rows.len() {
            let r = &self.rows[k];
            h = h.wrapping_mul(31).wrapping_add(r.slot);
            let mut f = 0usize;
            while f < FACE_N as usize {
                h = h.wrapping_mul(31).wrapping_add(r.digs[f].unwrap_or(0));
                f += 1;
            }
            let nb = r.name.as_bytes();
            let mut i = 0usize;
            while i < nb.len() {
                h = h.wrapping_mul(31).wrapping_add(nb[i] as u32);
                i += 1;
            }
            k += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.sealed as u32);
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut m = TriMidCheck::new();
    // 1) 坏面号必拒
    let _ = m.enroll(1, "坏面槽");
    got[0] = m.face_report(1, 3, "x").err() == Some(TM_FACE);
    // 2) 重槽必拒
    let _ = m.enroll(2, "重槽");
    got[1] = m.enroll(2, "重槽又").err() == Some(TM_DUP);
    // 3) 异报必入单（三报中乙面异名）
    let _ = m.face_report(1, 0, "同名");
    let _ = m.face_report(1, 1, "异名");
    let _ = m.face_report(1, 2, "同名");
    got[2] = m.mismatched() == [1u32] && !m.rows[0].agree();
    // 4) 重报必拒
    got[3] = m.face_report(2, 0, "a").is_ok()
        && m.face_report(2, 0, "b").err() == Some(TM_REREPORT);
    got
}

/// 中检十八槽槽名样本。
pub fn slot_names() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut n = 1u32;
    while n <= SLOTS_NEEDED as u32 {
        v.push(format!("中检槽{}", n));
        n += 1;
    }
    v
}

/// 批内判据。
pub fn run_unx10339_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10339");

    // J1 正路径：十八槽三面齐报全对平
    let mut m = TriMidCheck::new();
    let names = slot_names();
    let mut ok = true;
    let mut k = 0usize;
    while k < names.len() {
        if m.enroll(k as u32 + 1, &names[k]).is_err() {
            ok = false;
        }
        k += 1;
    }
    k = 0;
    while k < names.len() {
        let mut f = 0u8;
        while f < FACE_N {
            if m.face_report(k as u32 + 1, f, &names[k]).is_err() {
                ok = false;
            }
            f += 1;
        }
        k += 1;
    }
    s.add(
        "J1-三面网格对平",
        ok && m.flat() && m.mismatched().is_empty(),
        "十八槽三面指纹全等全对平",
    );

    // J2 逐槽对平裁断
    let j2 = {
        let mut all = true;
        let mut slot = 1u32;
        while slot <= SLOTS_NEEDED as u32 {
            let mut hit = false;
            let mut q = 0usize;
            while q < m.rows.len() {
                if m.rows[q].slot == slot && m.rows[q].agree() {
                    hit = true;
                }
                q += 1;
            }
            if !hit {
                all = false;
            }
            slot += 1;
        }
        all
    };
    s.add("J2-逐槽对平", j2, "十八槽逐槽三报全等");

    // J3 注入检出：坏面号 + 重槽
    let inj = inject_four();
    s.add("J3-面号重槽检出", inj[0] && inj[1], "坏面号与重槽必拒");

    // J4 注入检出：异报入单 + 重报
    s.add("J4-异报重报检出", inj[2] && inj[3], "异报必入异报单且重报必拒");

    // J5 指纹确定性：同名同指纹异名异指纹
    let j5 = fdigest("同名") == fdigest("同名") && fdigest("甲名") != fdigest("乙名");
    s.add("J5-指纹确定", j5, "名册指纹同名必等异名必异");

    // J6 未知槽报指纹必拒
    let j6 = TriMidCheck::new().face_report(7, 0, "x").err() == Some(TM_NOSLOT);
    s.add("J6-未知槽拒报", j6, "未开槽不得报指纹");

    // J7 空槽名必拒
    let j7 = TriMidCheck::new().enroll(1, "").err() == Some(TM_EMPTY);
    s.add("J7-空名拒收", j7, "槽名空必拒");

    // J8 封检后报与开槽均拒
    let j8 = {
        let mut t = TriMidCheck::new();
        let _ = t.enroll(1, "x");
        t.seal();
        t.face_report(1, 0, "x").err() == Some(TM_SEALED)
            && t.enroll(2, "y").err() == Some(TM_SEALED)
    };
    s.add("J8-封检拒报", j8, "封检后报指纹与开槽均拒");

    // J9 十七槽不成中检
    let j9 = {
        let mut t = TriMidCheck::new();
        let mut n = 1u32;
        while n < SLOTS_NEEDED as u32 {
            let nm = format!("短网格{}", n);
            let _ = t.enroll(n, &nm);
            let mut f = 0u8;
            while f < FACE_N {
                let _ = t.face_report(n, f, &nm);
                f += 1;
            }
            n += 1;
        }
        !t.flat()
    };
    s.add("J9-短网格不过", j9, "槽数缺一即不过");

    // J10 容量边界
    let j10 = {
        let mut t = TriMidCheck::new();
        let mut n = 1u32;
        let mut filled = true;
        while n <= CAP as u32 {
            let nm = format!("c{}", n);
            if t.enroll(n, &nm).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && t.enroll(999, "over").err() == Some(TM_CAP)
    };
    s.add("J10-容量边界", j10, "超容必拒且原册不损");

    // J11 指纹随报推进且封检后稳定
    let j11 = {
        let mut t = TriMidCheck::new();
        let _ = t.enroll(1, "指槽");
        let f0 = t.fold();
        let _ = t.face_report(1, 0, "指槽");
        let f1 = t.fold();
        let _ = t.face_report(1, 1, "指槽");
        let _ = t.face_report(1, 2, "指槽");
        let f3 = t.fold();
        t.seal();
        let f4 = t.fold();
        f0 != f1 && f1 != f3 && f3 != f4 && t.fold() == f4
    };
    s.add("J11-指纹推进", j11, "每报一步指纹一步且封检后稳定");

    // J12 动态明细：异报零清单
    let mz = m.mismatched().len();
    s.add_dyn("J12-异报零明细", mz == 0, &format!("mismatched={}", mz));

    // J13 动态明细：网格指纹落账
    let fp = m.fold();
    s.add_dyn("J13-网格指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10339_tri_midcheck_flow() {
        let mut m = TriMidCheck::new();
        assert!(m.enroll(0, "零").err() == Some(TM_ZERO));
        assert!(m.enroll(1, "").err() == Some(TM_EMPTY));
        assert!(m.enroll(1, "甲槽").is_ok());
        assert!(m.enroll(1, "重").err() == Some(TM_DUP));
        assert!(m.face_report(9, 0, "x").err() == Some(TM_NOSLOT));
        assert!(m.face_report(1, 3, "x").err() == Some(TM_FACE));
        assert!(m.face_report(1, 0, "甲槽").is_ok());
        assert!(m.face_report(1, 0, "再报").err() == Some(TM_REREPORT));
        assert!(!m.rows[0].full());
        assert!(m.face_report(1, 1, "甲槽").is_ok());
        assert!(m.face_report(1, 2, "甲槽").is_ok());
        assert!(m.rows[0].agree());
        assert!(!m.flat());
        m.seal();
        assert!(m.enroll(2, "迟").err() == Some(TM_SEALED));
        assert!(m.face_report(1, 0, "迟").err() == Some(TM_SEALED));
    }

    #[test]
    fn unx10339_checks_green() {
        let s = run_unx10339_checks();
        assert!(s.all_passed(), "UNX-F10339 判据未全绿");
    }
}
