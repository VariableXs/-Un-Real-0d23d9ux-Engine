//! UNX-F10319 联签面中检（十八项对平）（0x1b260 段）
//!
//! 锚点：UNX-F10319-J1 十八项对平——中检面逐项甲乙双方以名册指纹
//! 对报，指纹不等即异报单列，十八项全平才算中检过。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：中检册（十八项：项号+项名登记）+ 双报核（甲/乙各以名册指纹
//! 独立报，重报即拒）+ 对平核（双报指纹逐项相等）+ 注入核（异报指纹/
//! 单侧漏报/重报/重号四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b260 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b260;
pub const MC_DUP: u32 = SEG_BASE + 0x01; // 项号重号
pub const MC_EMPTY: u32 = SEG_BASE + 0x02; // 项名空
pub const MC_CAP: u32 = SEG_BASE + 0x03; // 册超容
pub const MC_NOMID: u32 = SEG_BASE + 0x04; // 未知项报指纹
pub const MC_REREPORT: u32 = SEG_BASE + 0x05; // 已报再报
pub const MC_SEALED: u32 = SEG_BASE + 0x06; // 封检后报

/// 中检项数锚（十八项）。
pub const ITEMS_NEEDED: usize = 18;
/// 中检册定容。
pub const CAP: usize = 24;

/// 名册指纹（FNV-1a 32 位，序敏感）。
pub fn digest(name: &str) -> u32 {
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

/// 单条中检项。
#[derive(Clone)]
pub struct MidRow {
    pub mid: u32,
    pub name: String,
    pub a_dg: Option<u32>,
    pub b_dg: Option<u32>,
}

impl MidRow {
    /// 双报齐备。
    pub fn both(&self) -> bool {
        self.a_dg.is_some() && self.b_dg.is_some()
    }

    /// 对平：双报且指纹相等。
    pub fn agree(&self) -> bool {
        self.a_dg == self.b_dg && self.both()
    }
}

/// 联签面中检册。
pub struct MidCheck {
    pub rows: Vec<MidRow>,
    pub sealed: bool,
}

impl MidCheck {
    pub fn new() -> MidCheck {
        MidCheck { rows: Vec::new(), sealed: false }
    }

    fn pos(&self, mid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].mid == mid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 登记中检项：封检、重号、空名、超容均拒。
    pub fn enroll(&mut self, mid: u32, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(MC_SEALED);
        }
        if mid == 0 || self.pos(mid).is_some() {
            return Err(MC_DUP);
        }
        if name.is_empty() {
            return Err(MC_EMPTY);
        }
        if self.rows.len() >= CAP {
            return Err(MC_CAP);
        }
        self.rows.push(MidRow { mid, name: name.to_string(), a_dg: None, b_dg: None });
        Ok(())
    }

    /// 甲侧报指纹。
    pub fn a_report(&mut self, mid: u32, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(MC_SEALED);
        }
        match self.pos(mid) {
            None => Err(MC_NOMID),
            Some(k) => {
                if self.rows[k].a_dg.is_some() {
                    return Err(MC_REREPORT);
                }
                self.rows[k].a_dg = Some(digest(name));
                Ok(())
            }
        }
    }

    /// 乙侧报指纹。
    pub fn b_report(&mut self, mid: u32, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(MC_SEALED);
        }
        match self.pos(mid) {
            None => Err(MC_NOMID),
            Some(k) => {
                if self.rows[k].b_dg.is_some() {
                    return Err(MC_REREPORT);
                }
                self.rows[k].b_dg = Some(digest(name));
                Ok(())
            }
        }
    }

    /// 异报项清单（双报但指纹不等）。
    pub fn mismatched(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.rows.len() {
            let r = &self.rows[k];
            if r.both() && !r.agree() {
                v.push(r.mid);
            }
            k += 1;
        }
        v
    }

    /// 单侧漏报清单。
    pub fn half_reported(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.rows.len() {
            if !self.rows[k].both() {
                v.push(self.rows[k].mid);
            }
            k += 1;
        }
        v
    }

    /// 中检过：十八项齐、全双报、零异报。
    pub fn flat(&self) -> bool {
        self.rows.len() == ITEMS_NEEDED
            && self.half_reported().is_empty()
            && self.mismatched().is_empty()
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 中检册指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b2600;
        h = h.wrapping_mul(31).wrapping_add(self.rows.len() as u32);
        let mut k = 0usize;
        while k < self.rows.len() {
            let r = &self.rows[k];
            h = h.wrapping_mul(31).wrapping_add(r.mid);
            h = h.wrapping_mul(31).wrapping_add(r.a_dg.unwrap_or(0));
            h = h.wrapping_mul(31).wrapping_add(r.b_dg.unwrap_or(0));
            let bytes = r.name.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = h.wrapping_mul(31).wrapping_add(bytes[b] as u32);
                b += 1;
            }
            k += 1;
        }
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut m = MidCheck::new();
    let _ = m.enroll(1, "异报项");
    let _ = m.enroll(2, "齐报项");
    // 1) 异报指纹（乙报错名）必入异报清单
    let _ = m.a_report(1, "同册名");
    let _ = m.b_report(1, "另册名");
    got[0] = m.mismatched() == [1u32] && !m.rows[0].agree();
    // 2) 单侧漏报必入漏报清单
    let _ = m.a_report(2, "齐");
    let _ = m.b_report(2, "齐");
    let _ = m.enroll(3, "漏报项");
    let _ = m.a_report(3, "漏");
    got[1] = m.half_reported() == [3u32];
    // 3) 重报必拒
    got[2] = m.a_report(1, "再报").err() == Some(MC_REREPORT)
        && m.b_report(2, "再报").err() == Some(MC_REREPORT);
    // 4) 未知项报指纹必拒
    got[3] = m.a_report(99, "x").err() == Some(MC_NOMID);
    got
}

/// 中检十八项项名样本。
pub fn mid_names() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut n = 1u32;
    while n <= ITEMS_NEEDED as u32 {
        v.push(format!("中检项{}", n));
        n += 1;
    }
    v
}

/// 批内判据。
pub fn run_unx10319_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10319");

    // J1 正路径：十八项登记双报对平
    let mut m = MidCheck::new();
    let names = mid_names();
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
        let mid = k as u32 + 1;
        if m.a_report(mid, &names[k]).is_err() || m.b_report(mid, &names[k]).is_err() {
            ok = false;
        }
        k += 1;
    }
    s.add("J1-十八项对平", ok && m.flat(), "十八项双报指纹全等");

    // J2 逐项对平裁断
    let j2 = {
        let mut ok = true;
        let mut mid = 1u32;
        while mid <= ITEMS_NEEDED as u32 {
            let mut k = 0usize;
            let mut hit = false;
            while k < m.rows.len() {
                if m.rows[k].mid == mid && m.rows[k].agree() {
                    hit = true;
                }
                k += 1;
            }
            if !hit {
                ok = false;
            }
            mid += 1;
        }
        ok
    };
    s.add("J2-逐项对平", j2, "十八项逐项双报指纹相等");

    // J3 注入检出：异报指纹 + 单侧漏报
    let inj = inject_four();
    s.add("J3-异报漏报检出", inj[0] && inj[1], "异报指纹必入异报清单且漏报可枚举");

    // J4 注入检出：重报 + 未知项
    s.add("J4-重报未知检出", inj[2] && inj[3], "重报必拒且未知项必拒");

    // J5 指纹确定性：同名同指纹异名异指纹
    let j5 = digest("同一项名") == digest("同一项名") && digest("甲名") != digest("乙名");
    s.add("J5-指纹确定", j5, "名册指纹同名必等异名必异");

    // J6 重号项拒
    let j6 = {
        let mut c = MidCheck::new();
        let _ = c.enroll(1, "甲");
        c.enroll(1, "乙").err() == Some(MC_DUP)
    };
    s.add("J6-重号拒收", j6, "中检项号重号必拒");

    // J7 空项名拒
    let j7 = MidCheck::new().enroll(1, "").err() == Some(MC_EMPTY);
    s.add("J7-空名拒收", j7, "中检项名空必拒");

    // J8 封检后报拒
    let j8 = {
        let mut c = MidCheck::new();
        let _ = c.enroll(1, "x");
        c.seal();
        c.a_report(1, "x").err() == Some(MC_SEALED)
            && c.enroll(2, "y").err() == Some(MC_SEALED)
    };
    s.add("J8-封检拒报", j8, "封检后报指纹与登记均拒");

    // J9 十八项未齐不算过（十七条）
    let j9 = {
        let mut c = MidCheck::new();
        let mut n = 1u32;
        while n < ITEMS_NEEDED as u32 {
            let nm = format!("短册{}", n);
            let _ = c.enroll(n, &nm);
            let _ = c.a_report(n, &nm);
            let _ = c.b_report(n, &nm);
            n += 1;
        }
        !c.flat()
    };
    s.add("J9-短册不过", j9, "中检项缺一即不过");

    // J10 容量边界
    let j10 = {
        let mut c = MidCheck::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= CAP as u32 {
            let nm = format!("c{}", n);
            if c.enroll(n, &nm).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.enroll(999, "over").err() == Some(MC_CAP)
    };
    s.add("J10-容量边界", j10, "超容必拒且原册不损");

    // J11 指纹随双报推进且终态稳定
    let j11 = {
        let mut c = MidCheck::new();
        let _ = c.enroll(1, "指");
        let f0 = c.fold();
        let _ = c.a_report(1, "指");
        let f1 = c.fold();
        let _ = c.b_report(1, "指");
        let f2 = c.fold();
        f0 != f1 && f1 != f2 && c.fold() == f2
    };
    s.add("J11-指纹随报", j11, "每报一步指纹一步且终态稳定");

    // J12 动态明细：异报零清单
    let mz = m.mismatched().len();
    s.add_dyn("J12-异报零明细", mz == 0, &format!("mismatched={}", mz));

    // J13 动态明细：中检册指纹落账
    let fp = m.fold();
    s.add_dyn("J13-中检指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10319_midcheck_flow() {
        let mut m = MidCheck::new();
        assert!(m.enroll(1, "a").is_ok());
        assert!(m.enroll(1, "dup").err() == Some(MC_DUP));
        assert!(m.enroll(2, "").err() == Some(MC_EMPTY));
        assert!(m.a_report(9, "x").err() == Some(MC_NOMID));
        assert!(m.a_report(1, "a").is_ok());
        assert!(m.a_report(1, "a").err() == Some(MC_REREPORT));
        assert!(m.b_report(1, "a").is_ok());
        assert!(m.rows[0].agree());
        assert!(m.b_report(1, "b").err() == Some(MC_REREPORT));
        assert!(!m.flat());
        m.seal();
        assert!(m.enroll(3, "late").err() == Some(MC_SEALED));
    }

    #[test]
    fn unx10319_checks_green() {
        let s = run_unx10319_checks();
        assert!(s.all_passed(), "UNX-F10319 判据未全绿");
    }
}
