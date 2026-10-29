//! UNX-F10344 wave-06 硬闸预检清单（硬闸前置项逐项核）（0x1b580 段）
//!
//! 锚点：UNX-F10344-J1 硬闸前置逐项核——硬闸前置项逐项登记、逐项预检
//! （每项只许核一次、核后即定），全项已核且全项为真方可清单清账，任一
//! 项为假或漏核即不得清。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：前置项账（项=项号+项名+预检结果 Option）+ 预检核（一次定案
//! 不得重核）+ 清账核（全核全真）+ 注入核（零号/重项/未知项/重核四路
//! 必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b580 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b580;
pub const PW_ZERO: u32 = SEG_BASE + 0x01; // 项号零
pub const PW_DUP: u32 = SEG_BASE + 0x02; // 项号重号
pub const PW_NOITEM: u32 = SEG_BASE + 0x03; // 未知项
pub const PW_RECHECK: u32 = SEG_BASE + 0x04; // 已核再核
pub const PW_CAP: u32 = SEG_BASE + 0x05; // 账超容
pub const PW_EMPTY: u32 = SEG_BASE + 0x06; // 项名空
pub const PW_NOTREADY: u32 = SEG_BASE + 0x07; // 未全核清账

/// 前置项锚（五项）。
pub const GATES_NEEDED: usize = 5;
/// 账定容。
pub const CAP: usize = 8;

/// 前置项。
#[derive(Clone)]
pub struct GateItem {
    pub gid: u32,
    pub name: String,
    pub checked: Option<bool>,
}

/// 硬闸预检清单。
pub struct WaveGates {
    pub items: Vec<GateItem>,
    pub sealed: bool,
}

impl WaveGates {
    pub fn new() -> WaveGates {
        WaveGates { items: Vec::new(), sealed: false }
    }

    fn pos(&self, gid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].gid == gid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 登记前置项。
    pub fn add(&mut self, gid: u32, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(PW_RECHECK);
        }
        if gid == 0 {
            return Err(PW_ZERO);
        }
        if self.pos(gid).is_some() {
            return Err(PW_DUP);
        }
        if name.is_empty() {
            return Err(PW_EMPTY);
        }
        if self.items.len() >= CAP {
            return Err(PW_CAP);
        }
        self.items.push(GateItem { gid, name: name.to_string(), checked: None });
        Ok(())
    }

    /// 预检一项（一次定案）。
    pub fn check(&mut self, gid: u32, ok: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(PW_RECHECK);
        }
        match self.pos(gid) {
            None => Err(PW_NOITEM),
            Some(p) => {
                if self.items[p].checked.is_some() {
                    return Err(PW_RECHECK);
                }
                self.items[p].checked = Some(ok);
                Ok(())
            }
        }
    }

    /// 漏核清单。
    pub fn unchecked(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].checked.is_none() {
                v.push(self.items[k].gid);
            }
            k += 1;
        }
        v
    }

    /// 为假清单。
    pub fn failed(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].checked == Some(false) {
                v.push(self.items[k].gid);
            }
            k += 1;
        }
        v
    }

    /// 清单清账：项数达锚、零漏核、零为假。
    pub fn clear(&self) -> bool {
        self.items.len() >= GATES_NEEDED
            && self.unchecked().is_empty()
            && self.failed().is_empty()
    }

    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(PW_RECHECK);
        }
        if !self.clear() {
            return Err(PW_NOTREADY);
        }
        self.sealed = true;
        Ok(())
    }

    /// 预检清单指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b5800;
        h = h.wrapping_mul(31).wrapping_add(self.items.len() as u32);
        let mut k = 0usize;
        while k < self.items.len() {
            let it = &self.items[k];
            h = h
                .wrapping_mul(31)
                .wrapping_add(it.gid)
                .wrapping_mul(31)
                .wrapping_add(it.checked.unwrap_or(false) as u32 + 2);
            let nb = it.name.as_bytes();
            let mut i = 0usize;
            while i < nb.len() {
                h = h.wrapping_mul(31).wrapping_add(nb[i] as u32);
                i += 1;
            }
            k += 1;
        }
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut g = WaveGates::new();
    // 1) 零号必拒
    got[0] = g.add(0, "零项").err() == Some(PW_ZERO);
    // 2) 重项必拒
    let _ = g.add(1, "脚本预检");
    got[1] = g.add(1, "重项").err() == Some(PW_DUP);
    // 3) 未知项预检必拒
    got[2] = g.check(99, true).err() == Some(PW_NOITEM);
    // 4) 重核必拒
    let _ = g.check(1, true);
    got[3] = g.check(1, false).err() == Some(PW_RECHECK);
    got
}

/// 硬闸前置五项样例。
pub fn gate_item_names() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    v.push("正演脚本锁定");
    v.push("入场确认书双签");
    v.push("预算帽核定");
    v.push("回归集就位");
    v.push("清账面冻结");
    v
}

/// 批内判据。
pub fn run_unx10344_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10344");

    // J1 正路径：五前置项登记全核为真并清账封单
    let mut g = WaveGates::new();
    let names = gate_item_names();
    let mut ok = true;
    let mut k = 0usize;
    while k < names.len() {
        if g.add(k as u32 + 1, names[k]).is_err() {
            ok = false;
        }
        k += 1;
    }
    k = 0;
    while k < names.len() {
        if g.check(k as u32 + 1, true).is_err() {
            ok = false;
        }
        k += 1;
    }
    let sealed = g.seal();
    s.add(
        "J1-硬闸前置逐项核",
        ok && g.clear() && sealed.is_ok() && g.sealed,
        "五前置项全核为真并清账封单",
    );

    // J2 漏核与为假双零
    let j2 = g.unchecked().is_empty() && g.failed().is_empty();
    s.add("J2-漏核假项双零", j2, "漏核清单与为假清单皆空");

    // J3 注入检出：零号 + 重项
    let inj = inject_four();
    s.add("J3-零号重项检出", inj[0] && inj[1], "零项号与重项必拒");

    // J4 注入检出：未知项 + 重核
    s.add("J4-未知重核检出", inj[2] && inj[3], "未知项预检与重核必拒");

    // J5 为假不得清账
    let j5 = {
        let mut t = WaveGates::new();
        let names2 = gate_item_names();
        let mut q = 0usize;
        while q < names2.len() {
            let _ = t.add(q as u32 + 1, names2[q]);
            let _ = t.check(q as u32 + 1, q != 3);
            q += 1;
        }
        !t.clear() && t.failed().contains(&4)
    };
    s.add("J5-假项不清", j5, "任一项为假即不得清账");

    // J6 漏核不得清账（缺第五项核）
    let j6 = {
        let mut t = WaveGates::new();
        let names3 = gate_item_names();
        let mut q = 0usize;
        while q < names3.len() {
            let _ = t.add(q as u32 + 1, names3[q]);
            if q != 4 {
                let _ = t.check(q as u32 + 1, true);
            }
            q += 1;
        }
        !t.clear() && t.unchecked().contains(&5)
    };
    s.add("J6-漏核不清", j6, "漏核项必枚举可得且不得清账");

    // J7 清账后预检必拒
    let j7 = {
        let mut t = WaveGates::new();
        let names4 = gate_item_names();
        let mut q = 0usize;
        while q < names4.len() {
            let _ = t.add(q as u32 + 1, names4[q]);
            let _ = t.check(q as u32 + 1, true);
            q += 1;
        }
        let _ = t.seal();
        t.check(1, true).err() == Some(PW_RECHECK)
            && t.add(9, "迟项").err() == Some(PW_RECHECK)
    };
    s.add("J7-清账拒动", j7, "清账封单后预检与登项均拒");

    // J8 空项名必拒
    let j8 = WaveGates::new().add(1, "").err() == Some(PW_EMPTY);
    s.add("J8-空名拒收", j8, "项名空必拒");

    // J9 容量边界
    let j9 = {
        let mut t = WaveGates::new();
        let mut n = 1u32;
        let mut filled = true;
        while n <= CAP as u32 {
            let nm = "容项";
            if t.add(n, nm).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && t.add(999, "超").err() == Some(PW_CAP)
    };
    s.add("J9-容量边界", j9, "超容必拒且原账不损");

    // J10 指纹随预检推进且清账后稳定
    let j10 = {
        let mut t = WaveGates::new();
        let _ = t.add(1, "指项");
        let f0 = t.fold();
        let _ = t.check(1, true);
        let f1 = t.fold();
        f0 != f1 && t.fold() == f1
    };
    s.add("J10-指纹推进", j10, "每核一项指纹一变且终态稳定");

    // J11 项数锚核
    s.add(
        "J11-项数锚核",
        g.items.len() == GATES_NEEDED,
        "前置项恰为五项",
    );

    // J12 动态明细：为假计数落账
    let fl = g.failed().len();
    s.add_dyn("J12-为假计数", fl == 0, &format!("failed={}", fl));

    // J13 动态明细：预检清单指纹落账
    let fp = g.fold();
    s.add_dyn("J13-预检指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10344_wave_gates_flow() {
        let mut g = WaveGates::new();
        assert!(g.add(0, "零").err() == Some(PW_ZERO));
        assert!(g.add(1, "").err() == Some(PW_EMPTY));
        assert!(g.add(1, "甲项").is_ok());
        assert!(g.add(1, "重").err() == Some(PW_DUP));
        assert!(g.check(99, true).err() == Some(PW_NOITEM));
        assert!(g.check(1, true).is_ok());
        assert!(g.check(1, true).err() == Some(PW_RECHECK));
        assert!(!g.clear());
        assert!(g.seal().err() == Some(PW_NOTREADY));
        let names = gate_item_names();
        let mut k = 1usize;
        while k < names.len() {
            let _ = g.add(k as u32 + 1, names[k]);
            let _ = g.check(k as u32 + 1, true);
            k += 1;
        }
        assert!(g.clear());
        assert!(g.seal().is_ok());
        assert!(g.check(2, true).err() == Some(PW_RECHECK));
    }

    #[test]
    fn unx10344_checks_green() {
        let s = run_unx10344_checks();
        assert!(s.all_passed(), "UNX-F10344 判据未全绿");
    }
}
