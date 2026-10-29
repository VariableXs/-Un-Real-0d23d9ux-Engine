//! UNX-F10348 豁免账终态核（双清状态核）（0x1b600 段）
//!
//! 锚点：UNX-F10348-J1 豁免双清终态——豁免账逐条带缘由立账，ADR 清与
//! 披露清两标记各只许记一次，双清齐且解锁记讫才算该条终态，全部条目
//! 终态才算豁免账终态核过。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：豁免账（条=号+缘由+双清位+解锁位）+ 立账核（缘由空即拒）+
//! 标记核（同标记重记即拒）+ 终态核（未清单可枚举）+ 注入核（零号/
//! 重条/空缘由/未知条标记四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b600 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b600;
pub const EE_ZERO: u32 = SEG_BASE + 0x01; // 条号零
pub const EE_DUP: u32 = SEG_BASE + 0x02; // 条号重号
pub const EE_NOREASON: u32 = SEG_BASE + 0x03; // 缘由空
pub const EE_REMARK: u32 = SEG_BASE + 0x04; // 同标记重记
pub const EE_NOTID: u32 = SEG_BASE + 0x05; // 未知条
pub const EE_CAP: u32 = SEG_BASE + 0x06; // 账超容

/// 豁免条锚（四条）。
pub const EXEMPTS_NEEDED: usize = 4;
/// 账定容。
pub const CAP: usize = 10;

/// 豁免条。
#[derive(Clone)]
pub struct Exempt {
    pub eid: u32,
    pub reason: String,
    pub adr_ok: bool,
    pub disc_ok: bool,
    pub unlocked: bool,
}

impl Exempt {
    /// 双清：ADR 清且披露清。
    pub fn dual_clean(&self) -> bool {
        self.adr_ok && self.disc_ok
    }

    /// 终态：双清且解锁。
    pub fn final_state(&self) -> bool {
        self.dual_clean() && self.unlocked
    }
}

/// 豁免账。
pub struct ExemptBook {
    pub entries: Vec<Exempt>,
    pub sealed: bool,
}

impl ExemptBook {
    pub fn new() -> ExemptBook {
        ExemptBook { entries: Vec::new(), sealed: false }
    }

    fn pos(&self, eid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.entries.len() {
            if self.entries[k].eid == eid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 立账。
    pub fn entry(&mut self, eid: u32, reason: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(EE_REMARK);
        }
        if eid == 0 {
            return Err(EE_ZERO);
        }
        if self.pos(eid).is_some() {
            return Err(EE_DUP);
        }
        if reason.is_empty() {
            return Err(EE_NOREASON);
        }
        if self.entries.len() >= CAP {
            return Err(EE_CAP);
        }
        self.entries.push(Exempt {
            eid,
            reason: reason.to_string(),
            adr_ok: false,
            disc_ok: false,
            unlocked: false,
        });
        Ok(())
    }

    fn mark(&mut self, eid: u32, which: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(EE_REMARK);
        }
        match self.pos(eid) {
            None => Err(EE_NOTID),
            Some(p) => {
                let already = match which {
                    0 => self.entries[p].adr_ok,
                    1 => self.entries[p].disc_ok,
                    _ => self.entries[p].unlocked,
                };
                if already {
                    return Err(EE_REMARK);
                }
                match which {
                    0 => self.entries[p].adr_ok = true,
                    1 => self.entries[p].disc_ok = true,
                    _ => self.entries[p].unlocked = true,
                }
                Ok(())
            }
        }
    }

    /// 记 ADR 清。
    pub fn adr_clear(&mut self, eid: u32) -> Result<(), u32> {
        self.mark(eid, 0)
    }

    /// 记披露清。
    pub fn disc_clear(&mut self, eid: u32) -> Result<(), u32> {
        self.mark(eid, 1)
    }

    /// 记解锁。
    pub fn unlock(&mut self, eid: u32) -> Result<(), u32> {
        self.mark(eid, 2)
    }

    /// 未终态清单。
    pub fn pending(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.entries.len() {
            if !self.entries[k].final_state() {
                v.push(self.entries[k].eid);
            }
            k += 1;
        }
        v
    }

    /// 终态核过：条数达锚且零未终态。
    pub fn final_ok(&self) -> bool {
        self.entries.len() >= EXEMPTS_NEEDED && self.pending().is_empty()
    }

    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(EE_REMARK);
        }
        if !self.final_ok() {
            return Err(EE_NOTID);
        }
        self.sealed = true;
        Ok(())
    }

    /// 豁免账指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b6000;
        h = h.wrapping_mul(31).wrapping_add(self.entries.len() as u32);
        let mut k = 0usize;
        while k < self.entries.len() {
            let e = &self.entries[k];
            h = h
                .wrapping_mul(31)
                .wrapping_add(e.eid)
                .wrapping_mul(31)
                .wrapping_add(e.adr_ok as u32)
                .wrapping_mul(31)
                .wrapping_add(e.disc_ok as u32)
                .wrapping_mul(31)
                .wrapping_add(e.unlocked as u32);
            let rb = e.reason.as_bytes();
            let mut i = 0usize;
            while i < rb.len() {
                h = h.wrapping_mul(31).wrapping_add(rb[i] as u32);
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
    let mut e = ExemptBook::new();
    // 1) 零号必拒
    got[0] = e.entry(0, "零条").err() == Some(EE_ZERO);
    // 2) 重条必拒
    let _ = e.entry(1, "时序豁免");
    got[1] = e.entry(1, "重条").err() == Some(EE_DUP);
    // 3) 未知条标记必拒
    got[2] = e.adr_clear(99).err() == Some(EE_NOTID)
        && e.unlock(99).err() == Some(EE_NOTID);
    // 4) 重记必拒
    let _ = e.adr_clear(1);
    got[3] = e.adr_clear(1).err() == Some(EE_REMARK);
    got
}

/// 豁免样例四条缘由。
pub fn exempt_reasons() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    v.push("时序窗口豁免（ADR 已清）");
    v.push("遗留兼容豁免（披露已清）");
    v.push("样例量豁免（双清在账）");
    v.push("观察项豁免（跟踪在案）");
    v
}

/// 批内判据。
pub fn run_unx10348_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10348");

    // J1 正路径：四条豁免双清解锁全终态并封账
    let mut e = ExemptBook::new();
    let reasons = exempt_reasons();
    let mut ok = true;
    let mut k = 0usize;
    while k < reasons.len() {
        if e.entry(k as u32 + 1, reasons[k]).is_err() {
            ok = false;
        }
        k += 1;
    }
    k = 0;
    while k < reasons.len() {
        let id = k as u32 + 1;
        if e.adr_clear(id).is_err() || e.disc_clear(id).is_err() || e.unlock(id).is_err() {
            ok = false;
        }
        k += 1;
    }
    let sealed = e.seal();
    s.add(
        "J1-豁免双清终态",
        ok && e.final_ok() && sealed.is_ok() && e.sealed,
        "四条豁免双清加解锁全终态并封账",
    );

    // J2 未终态清单双零
    let j2 = e.pending().is_empty() && e.entries.len() == EXEMPTS_NEEDED;
    s.add("J2-未终态双零", j2, "未终态清单空且条数达锚");

    // J3 注入检出：零号 + 重条
    let inj = inject_four();
    s.add("J3-零号重条检出", inj[0] && inj[1], "零条号与重条必拒");

    // J4 注入检出：未知条标记 + 重记
    s.add("J4-未知重记检出", inj[2] && inj[3], "未知条标记与同标记重记必拒");

    // J5 只清一侧不成双清
    let j5 = {
        let mut t = ExemptBook::new();
        let _ = t.entry(1, "半清条");
        let _ = t.adr_clear(1);
        !t.entries[0].dual_clean() && t.pending().contains(&1)
    };
    s.add("J5-半清不成", j5, "仅 ADR 清不算双清");

    // J6 双清未解锁不算终态
    let j6 = {
        let mut t = ExemptBook::new();
        let _ = t.entry(1, "未解条");
        let _ = t.adr_clear(1);
        let _ = t.disc_clear(1);
        t.entries[0].dual_clean() && !t.entries[0].final_state()
    };
    s.add("J6-未解不成终态", j6, "双清后仍须解锁才算终态");

    // J7 空缘由必拒
    let j7 = ExemptBook::new().entry(1, "").err() == Some(EE_NOREASON);
    s.add("J7-空由拒收", j7, "豁免缘由空必拒");

    // J8 披露清重记必拒
    let j8 = {
        let mut t = ExemptBook::new();
        let _ = t.entry(1, "重记条");
        let _ = t.disc_clear(1);
        t.disc_clear(1).err() == Some(EE_REMARK)
            && t.unlock(1).is_ok()
            && t.unlock(1).err() == Some(EE_REMARK)
    };
    s.add("J8-重记拒收", j8, "披露清与解锁重记均拒");

    // J9 未终态阻断封账
    let j9 = {
        let mut t = ExemptBook::new();
        let _ = t.entry(1, "挂条");
        t.seal().err() == Some(EE_NOTID)
    };
    s.add("J9-挂条拒封", j9, "存在未终态条即不得封账");

    // J10 容量边界
    let j10 = {
        let mut t = ExemptBook::new();
        let mut n = 1u32;
        let mut filled = true;
        while n <= CAP as u32 {
            let rs = "容条";
            if t.entry(n, rs).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && t.entry(999, "超").err() == Some(EE_CAP)
    };
    s.add("J10-容量边界", j10, "超容必拒且原账不损");

    // J11 指纹随标记推进且封账后稳定
    let j11 = {
        let mut t = ExemptBook::new();
        let _ = t.entry(1, "指条");
        let f0 = t.fold();
        let _ = t.adr_clear(1);
        let f1 = t.fold();
        let _ = t.disc_clear(1);
        let _ = t.unlock(1);
        let f3 = t.fold();
        f0 != f1 && f1 != f3 && t.fold() == f3
    };
    s.add("J11-指纹推进", j11, "每记一标指纹一变且终态稳定");

    // J12 动态明细：未终态计数落账
    let pd = e.pending().len();
    s.add_dyn("J12-未终态计数", pd == 0, &format!("pending={}", pd));

    // J13 动态明细：豁免账指纹落账
    let fp = e.fold();
    s.add_dyn("J13-豁免指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10348_exempt_final_flow() {
        let mut e = ExemptBook::new();
        assert!(e.entry(0, "零").err() == Some(EE_ZERO));
        assert!(e.entry(1, "").err() == Some(EE_NOREASON));
        assert!(e.entry(1, "甲条").is_ok());
        assert!(e.entry(1, "重").err() == Some(EE_DUP));
        assert!(e.adr_clear(99).err() == Some(EE_NOTID));
        assert!(e.adr_clear(1).is_ok());
        assert!(e.adr_clear(1).err() == Some(EE_REMARK));
        assert!(!e.entries[0].dual_clean());
        assert!(e.disc_clear(1).is_ok());
        assert!(e.entries[0].dual_clean());
        assert!(!e.entries[0].final_state());
        assert!(e.unlock(1).is_ok());
        assert!(e.entries[0].final_state());
        let reasons = exempt_reasons();
        let mut k = 1usize;
        while k < reasons.len() {
            let _ = e.entry(k as u32 + 1, reasons[k]);
            let _ = e.adr_clear(k as u32 + 1);
            let _ = e.disc_clear(k as u32 + 1);
            let _ = e.unlock(k as u32 + 1);
            k += 1;
        }
        assert!(e.final_ok());
        assert!(e.seal().is_ok());
        assert!(e.unlock(2).err() == Some(EE_REMARK));
    }

    #[test]
    fn unx10348_checks_green() {
        let s = run_unx10348_checks();
        assert!(s.all_passed(), "UNX-F10348 判据未全绿");
    }
}
