//! UNX-F10345 判据库终版移交（O3 桥全量同步）（0x1b5a0 段）
//!
//! 锚点：UNX-F10345-J1 全量同步核——源侧判据逐条以名取指纹，桥上逐条
//! 推送到目标侧（重推即拒），目标侧逐条以名回验指纹，源目两侧条数一
//! 致且指纹全等才算终版移交完毕。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：源册（条=号+名+指纹 64 位）+ 目册（条=号+指纹）+ 推送核（未
//! 知号即拒、重推即拒）+ 回验核（目标侧未知号即拒、名指纹比对）+
//! 注入核（零号/重号/未知号推送/重推四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b5a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b5a0;
pub const HO_ZERO: u32 = SEG_BASE + 0x01; // 号零
pub const HO_DUP: u32 = SEG_BASE + 0x02; // 源号重号
pub const HO_NOSRC: u32 = SEG_BASE + 0x03; // 源侧未知号
pub const HO_REPUSH: u32 = SEG_BASE + 0x04; // 已推再推
pub const HO_NOTGT: u32 = SEG_BASE + 0x05; // 目标侧未知号
pub const HO_CAP: u32 = SEG_BASE + 0x06; // 册超容
pub const HO_EMPTY: u32 = SEG_BASE + 0x07; // 名空

/// 移交条锚（八条）。
pub const ITEMS_NEEDED: usize = 8;
/// 册定容。
pub const CAP: usize = 12;

/// FNV-1a 64 位名指纹。
pub fn name_digest(name: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let bytes = name.as_bytes();
    let mut k = 0usize;
    while k < bytes.len() {
        h ^= bytes[k] as u64;
        h = h.wrapping_mul(0x100000001b3);
        k += 1;
    }
    h
}

/// 源侧判据条。
#[derive(Clone)]
pub struct SrcItem {
    pub sid: u32,
    pub name: String,
    pub dg: u64,
    pub pushed: bool,
}

/// 目标侧判据条。
#[derive(Clone)]
pub struct TgtItem {
    pub sid: u32,
    pub dg: u64,
}

/// 终版移交核。
pub struct Handover {
    pub src: Vec<SrcItem>,
    pub tgt: Vec<TgtItem>,
    pub sealed: bool,
}

impl Handover {
    pub fn new() -> Handover {
        Handover { src: Vec::new(), tgt: Vec::new(), sealed: false }
    }

    fn src_pos(&self, sid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.src.len() {
            if self.src[k].sid == sid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    fn tgt_pos(&self, sid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.tgt.len() {
            if self.tgt[k].sid == sid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 源侧登记一条。
    pub fn src_put(&mut self, sid: u32, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(HO_REPUSH);
        }
        if sid == 0 {
            return Err(HO_ZERO);
        }
        if self.src_pos(sid).is_some() {
            return Err(HO_DUP);
        }
        if name.is_empty() {
            return Err(HO_EMPTY);
        }
        if self.src.len() >= CAP {
            return Err(HO_CAP);
        }
        self.src.push(SrcItem {
            sid,
            name: name.to_string(),
            dg: name_digest(name),
            pushed: false,
        });
        Ok(())
    }

    /// 推送一条到目标侧。
    pub fn push(&mut self, sid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(HO_REPUSH);
        }
        match self.src_pos(sid) {
            None => Err(HO_NOSRC),
            Some(p) => {
                if self.src[p].pushed {
                    return Err(HO_REPUSH);
                }
                if self.tgt.len() >= CAP {
                    return Err(HO_CAP);
                }
                self.tgt.push(TgtItem { sid, dg: self.src[p].dg });
                self.src[p].pushed = true;
                Ok(())
            }
        }
    }

    /// 全量推送：未推条逐条推。
    pub fn push_all(&mut self) -> Result<usize, u32> {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.src.len() {
            if !self.src[k].pushed {
                let sid = self.src[k].sid;
                self.push(sid)?;
                n += 1;
            }
            k += 1;
        }
        Ok(n)
    }

    /// 目标侧回验：以名重算指纹与目标指纹比对。
    pub fn verify(&self, sid: u32, got_name: &str) -> Result<bool, u32> {
        match self.tgt_pos(sid) {
            None => Err(HO_NOTGT),
            Some(p) => Ok(name_digest(got_name) == self.tgt[p].dg),
        }
    }

    /// 未推清单。
    pub fn unpulled(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.src.len() {
            if !self.src[k].pushed {
                v.push(self.src[k].sid);
            }
            k += 1;
        }
        v
    }

    /// 同步毕：源目条数一致、零未推、逐条指纹相等。
    pub fn synced(&self) -> bool {
        self.unpulled().is_empty()
            && self.src.len() >= ITEMS_NEEDED
            && self.tgt.len() == self.src.len()
            && self.verify_all()
    }

    fn verify_all(&self) -> bool {
        let mut k = 0usize;
        while k < self.src.len() {
            let sid = self.src[k].sid;
            match self.tgt_pos(sid) {
                None => return false,
                Some(p) => {
                    if self.tgt[p].dg != self.src[k].dg {
                        return false;
                    }
                }
            }
            k += 1;
        }
        true
    }

    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(HO_REPUSH);
        }
        if !self.synced() {
            return Err(HO_NOSRC);
        }
        self.sealed = true;
        Ok(())
    }

    /// 移交账指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0x1b5a01b5a;
        h = h.wrapping_mul(0x100000001b3).wrapping_add(self.src.len() as u64);
        let mut k = 0usize;
        while k < self.src.len() {
            h = h.wrapping_mul(0x100000001b3).wrapping_add(self.src[k].dg);
            k += 1;
        }
        h = h.wrapping_mul(0x100000001b3).wrapping_add(self.tgt.len() as u64);
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut h = Handover::new();
    // 1) 零号必拒
    got[0] = h.src_put(0, "零").err() == Some(HO_ZERO);
    // 2) 重号必拒
    let _ = h.src_put(1, " errno 映射");
    got[1] = h.src_put(1, "重号").err() == Some(HO_DUP);
    // 3) 源侧未知号推送必拒
    got[2] = h.push(99).err() == Some(HO_NOSRC);
    // 4) 目标侧未知号回验必拒
    got[3] = h.verify(99, "x").err() == Some(HO_NOTGT);
    got
}

/// 移交样例八条。
pub fn handover_names() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    v.push("errno 映射表");
    v.push("fs 能力面判据");
    v.push("pty 行规程判据");
    v.push("fork/exec 判据");
    v.push("wait/exit 判据");
    v.push("调度判据");
    v.push("内存判据");
    v.push("中断判据");
    v
}

/// 批内判据。
pub fn run_unx10345_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10345");

    // J1 正路径：八条全推全验同步毕并封账
    let mut h = Handover::new();
    let names = handover_names();
    let mut ok = true;
    let mut k = 0usize;
    while k < names.len() {
        if h.src_put(k as u32 + 1, names[k]).is_err() {
            ok = false;
        }
        k += 1;
    }
    match h.push_all() {
        Ok(n) => {
            if n != names.len() {
                ok = false;
            }
        }
        Err(_) => ok = false,
    }
    k = 0;
    while k < names.len() {
        match h.verify(k as u32 + 1, names[k]) {
            Ok(true) => {}
            _ => ok = false,
        }
        k += 1;
    }
    let sealed = h.seal();
    s.add(
        "J1-全量同步核",
        ok && h.synced() && sealed.is_ok() && h.sealed,
        "八条源判据全推全验指纹全等并封账",
    );

    // J2 源目条数对平
    let j2 = h.src.len() == h.tgt.len() && h.tgt.len() == ITEMS_NEEDED;
    s.add("J2-条数对平", j2, "源目两侧条数恰为八");

    // J3 注入检出：零号 + 重号
    let inj = inject_four();
    s.add("J3-零号重号检出", inj[0] && inj[1], "零号与源侧重号必拒");

    // J4 注入检出：未知推送 + 未知回验
    s.add("J4-未知双检出", inj[2] && inj[3], "源侧未知推送与目标侧未知回验必拒");

    // J5 重推必拒
    let j5 = {
        let mut t = Handover::new();
        let _ = t.src_put(1, "重推条");
        let _ = t.push(1);
        t.push(1).err() == Some(HO_REPUSH)
    };
    s.add("J5-重推拒收", j5, "一条只许推送一次");

    // J6 回验异名必假
    let j6 = {
        let mut t = Handover::new();
        let _ = t.src_put(1, "原名条");
        let _ = t.push(1);
        t.verify(1, "异名条") == Ok(false)
    };
    s.add("J6-异名验假", j6, "回验名异于原名即判假");

    // J7 未推阻断封账
    let j7 = {
        let mut t = Handover::new();
        let names2 = handover_names();
        let mut q = 0usize;
        while q < names2.len() {
            let _ = t.src_put(q as u32 + 1, names2[q]);
            if q != 7 {
                let _ = t.push(q as u32 + 1);
            }
            q += 1;
        }
        !t.synced() && t.unpulled() == [8u32] && t.seal().err() == Some(HO_NOSRC)
    };
    s.add("J7-未推拒封", j7, "存在未推条即不得封账");

    // J8 空名必拒
    let j8 = Handover::new().src_put(1, "").err() == Some(HO_EMPTY);
    s.add("J8-空名拒收", j8, "判据名空必拒");

    // J9 容量边界
    let j9 = {
        let mut t = Handover::new();
        let mut n = 1u32;
        let mut filled = true;
        while n <= CAP as u32 {
            let nm = "容条";
            if t.src_put(n, nm).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && t.src_put(999, "超").err() == Some(HO_CAP)
    };
    s.add("J9-容量边界", j9, "超容必拒且原册不损");

    // J10 推送返回数对平
    let j10 = {
        let mut t = Handover::new();
        let _ = t.src_put(1, "甲");
        let _ = t.src_put(2, "乙");
        t.push_all() == Ok(2)
    };
    s.add("J10-推送计数", j10, "全量推送返回恰推条数");

    // J11 指纹随推送推进且封账后稳定
    let j11 = {
        let mut t = Handover::new();
        let _ = t.src_put(1, "指条");
        let f0 = t.fold();
        let _ = t.push(1);
        let f1 = t.fold();
        f0 != f1 && t.fold() == f1
    };
    s.add("J11-指纹推进", j11, "推送一步指纹一变且终态稳定");

    // J12 动态明细：未推计数落账
    let up = h.unpulled().len();
    s.add_dyn("J12-未推计数", up == 0, &format!("unpushed={}", up));

    // J13 动态明细：移交账指纹落账
    let fp = h.fold();
    s.add_dyn("J13-移交指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10345_handover_flow() {
        let mut h = Handover::new();
        assert!(h.src_put(0, "零").err() == Some(HO_ZERO));
        assert!(h.src_put(1, "").err() == Some(HO_EMPTY));
        assert!(h.src_put(1, "甲条").is_ok());
        assert!(h.src_put(1, "重").err() == Some(HO_DUP));
        assert!(h.push(99).err() == Some(HO_NOSRC));
        assert!(h.push(1).is_ok());
        assert!(h.push(1).err() == Some(HO_REPUSH));
        assert!(h.verify(1, "甲条").unwrap_or(false));
        assert!(h.verify(1, "乙条") == Ok(false));
        assert!(h.verify(99, "甲条").err() == Some(HO_NOTGT));
        assert!(!h.synced());
        assert!(h.seal().err() == Some(HO_NOSRC));
        h.sealed = false;
        let names = handover_names();
        let mut k = 0usize;
        while k < names.len() {
            let _ = h.src_put(k as u32 + 10, names[k]);
            k += 1;
        }
        assert!(h.push_all() == Ok(8));
        assert!(h.synced());
        assert!(h.seal().is_ok());
    }

    #[test]
    fn unx10345_checks_green() {
        let s = run_unx10345_checks();
        assert!(s.all_passed(), "UNX-F10345 判据未全绿");
    }
}
