//! UNX-F10331 R-C3-003 释放闸正演（条件逐项核+释放登记）（0x1b3e0 段）
//!
//! 锚点：UNX-F10331-J1 逐项核+释放登记——五释放条件逐条取证（取证值
//! 与要件值相等才算过条），五条全过方可登记释放（登记只许一次、登人
//! 名不空），登记在账才算闸开。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：条件账（条=要素号+要件值+取证值 Option）+ 取证核（未取不算
//! 过条、异值入失败单）+ 释放登记（五条全过方可登记，重登即拒）+
//! 注入核（坏要素号/重条/未全取即登/异值取证四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b3e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b3e0;
pub const GL_ENO: u32 = SEG_BASE + 0x01; // 要素号越界
pub const GL_DUP: u32 = SEG_BASE + 0x02; // 条重号
pub const GL_NOC: u32 = SEG_BASE + 0x03; // 未知条件
pub const GL_REREG: u32 = SEG_BASE + 0x04; // 已登再登
pub const GL_UNVER: u32 = SEG_BASE + 0x05; // 未全取即登
pub const GL_EMPTY: u32 = SEG_BASE + 0x06; // 登人名空
pub const GL_CAP: u32 = SEG_BASE + 0x07; // 账超容

/// 五条件锚。
pub const COND_N: usize = 5;
/// 账定容。
pub const CAP: usize = 6;

/// 释放条件条。
#[derive(Clone)]
pub struct LiveCond {
    pub eno: u32,
    pub want: bool,
    pub got: Option<bool>,
}

impl LiveCond {
    /// 过条：取证值与要件值相等。
    pub fn ok(&self) -> bool {
        self.got == Some(self.want)
    }

    /// 已取异值（失败条）。
    pub fn failed(&self) -> bool {
        match self.got {
            None => false,
            Some(v) => v != self.want,
        }
    }
}

/// 释放登记。
#[derive(Clone)]
pub struct ReleaseReg {
    pub who: String,
    pub tick: u32,
}

/// 释放闸正演核。
pub struct GateLive {
    pub conds: Vec<LiveCond>,
    pub reg: Option<ReleaseReg>,
}

impl GateLive {
    pub fn new() -> GateLive {
        GateLive { conds: Vec::new(), reg: None }
    }

    fn pos(&self, eno: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.conds.len() {
            if self.conds[k].eno == eno {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 绑条件（要件值）。
    pub fn bind(&mut self, eno: u32, want: bool) -> Result<(), u32> {
        if self.reg.is_some() {
            return Err(GL_REREG);
        }
        if eno == 0 || eno > COND_N as u32 {
            return Err(GL_ENO);
        }
        if self.pos(eno).is_some() {
            return Err(GL_DUP);
        }
        if self.conds.len() >= CAP {
            return Err(GL_CAP);
        }
        self.conds.push(LiveCond { eno, want, got: None });
        Ok(())
    }

    /// 取证。
    pub fn verify(&mut self, eno: u32, got: bool) -> Result<(), u32> {
        if self.reg.is_some() {
            return Err(GL_REREG);
        }
        match self.pos(eno) {
            None => Err(GL_NOC),
            Some(p) => {
                if self.conds[p].got.is_some() {
                    return Err(GL_DUP);
                }
                self.conds[p].got = Some(got);
                Ok(())
            }
        }
    }

    /// 失败条清单（已取异值）。
    pub fn failed_list(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.conds.len() {
            if self.conds[k].failed() {
                v.push(self.conds[k].eno);
            }
            k += 1;
        }
        v
    }

    /// 未取清单。
    pub fn unverified(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.conds.len() {
            if self.conds[k].got.is_none() {
                v.push(self.conds[k].eno);
            }
            k += 1;
        }
        v
    }

    /// 条件全过：条数达锚且零未取零失败。
    pub fn conds_ok(&self) -> bool {
        self.conds.len() >= COND_N
            && self.unverified().is_empty()
            && self.failed_list().is_empty()
    }

    /// 登记释放：五条全过方可，重登即拒。
    pub fn register_release(&mut self, who: &str, tick: u32) -> Result<(), u32> {
        if self.reg.is_some() {
            return Err(GL_REREG);
        }
        if who.is_empty() {
            return Err(GL_EMPTY);
        }
        if !self.conds_ok() {
            return Err(GL_UNVER);
        }
        self.reg = Some(ReleaseReg { who: who.to_string(), tick });
        Ok(())
    }

    /// 闸开（登记在账）。
    pub fn released(&self) -> bool {
        self.reg.is_some()
    }

    /// 闸指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b3e00;
        h = h.wrapping_mul(31).wrapping_add(self.conds.len() as u32);
        let mut k = 0usize;
        while k < self.conds.len() {
            let c = &self.conds[k];
            h = h
                .wrapping_mul(31)
                .wrapping_add(c.eno)
                .wrapping_mul(31)
                .wrapping_add(c.want as u32 + 1)
                .wrapping_mul(31)
                .wrapping_add(c.got.unwrap_or(!c.want) as u32 + 1);
            k += 1;
        }
        match &self.reg {
            None => h.wrapping_mul(31),
            Some(r) => {
                h = h.wrapping_mul(31).wrapping_add(r.tick + 1);
                let wb = r.who.as_bytes();
                let mut i = 0usize;
                while i < wb.len() {
                    h = h.wrapping_mul(31).wrapping_add(wb[i] as u32);
                    i += 1;
                }
                h
            }
        }
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut g = GateLive::new();
    // 1) 坏要素号必拒
    got[0] = g.bind(0, true).err() == Some(GL_ENO)
        && g.bind(COND_N as u32 + 1, true).err() == Some(GL_ENO);
    // 2) 重条与未知条件必拒
    let _ = g.bind(1, true);
    got[1] = g.bind(1, false).err() == Some(GL_DUP)
        && g.verify(9, true).err() == Some(GL_NOC);
    // 3) 未全取即登必拒
    let _ = g.verify(1, true);
    got[2] = g.register_release("甲", 5).err() == Some(GL_UNVER);
    // 4) 异值取证入失败单
    let _ = g.bind(2, true);
    let _ = g.verify(2, false);
    got[3] = g.failed_list() == [2u32] && !g.conds[1].ok();
    got
}

/// 批内判据。
pub fn run_unx10331_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10331");

    // J1 正路径：五条件绑值取证全过并登记释放
    let mut g = GateLive::new();
    let mut ok = true;
    let mut e = 1u32;
    while e <= COND_N as u32 {
        if g.bind(e, true).is_err() {
            ok = false;
        }
        e += 1;
    }
    e = 1;
    while e <= COND_N as u32 {
        if g.verify(e, true).is_err() {
            ok = false;
        }
        e += 1;
    }
    let regd = g.register_release("值账官", 77);
    s.add(
        "J1-逐项核释放",
        ok && g.conds_ok() && regd.is_ok() && g.released(),
        "五条件全取证为真并成功登记释放",
    );

    // J2 逐条过裁断
    let j2 = {
        let mut allo = true;
        let mut q = 0usize;
        while q < g.conds.len() {
            if !g.conds[q].ok() {
                allo = false;
            }
            q += 1;
        }
        allo
    };
    s.add("J2-逐条过", j2, "五条取证值与要件值逐条相等");

    // J3 注入检出：坏号 + 重条未知条
    let inj = inject_four();
    s.add("J3-坏号重条检出", inj[0] && inj[1], "坏要素号与重条未知条必拒");

    // J4 注入检出：未全取即登 + 异值入单
    s.add("J4-未取异值检出", inj[2] && inj[3], "未全取证即登必拒且异值入失败单");

    // J5 异值阻断登记（一条为假）
    let j5 = {
        let mut t = GateLive::new();
        let mut q = 1u32;
        while q <= COND_N as u32 {
            let _ = t.bind(q, true);
            q += 1;
        }
        let _ = t.verify(1, true);
        let _ = t.verify(2, true);
        let _ = t.verify(3, false);
        let _ = t.verify(4, true);
        let _ = t.verify(5, true);
        t.register_release("乙", 1).err() == Some(GL_UNVER)
            && t.failed_list().contains(&3)
    };
    s.add("J5-异值阻断", j5, "任一条取证为假即不得登记");

    // J6 重登必拒
    let j6 = {
        let mut t = GateLive::new();
        let mut q = 1u32;
        while q <= COND_N as u32 {
            let _ = t.bind(q, true);
            let _ = t.verify(q, true);
            q += 1;
        }
        let _ = t.register_release("甲", 1);
        t.register_release("乙", 2).err() == Some(GL_REREG)
    };
    s.add("J6-重登拒收", j6, "释放登记只许一次");

    // J7 登人名空必拒
    let j7 = {
        let mut t = GateLive::new();
        let mut q = 1u32;
        while q <= COND_N as u32 {
            let _ = t.bind(q, true);
            let _ = t.verify(q, true);
            q += 1;
        }
        t.register_release("", 1).err() == Some(GL_EMPTY)
    };
    s.add("J7-空名拒登", j7, "登人名空必拒");

    // J8 登记后取证必拒
    let j8 = {
        let mut t = GateLive::new();
        let mut q = 1u32;
        while q <= COND_N as u32 {
            let _ = t.bind(q, true);
            let _ = t.verify(q, true);
            q += 1;
        }
        let _ = t.register_release("甲", 1);
        t.verify(1, true).err() == Some(GL_REREG)
            && t.bind(6, true).err() == Some(GL_ENO)
    };
    s.add("J8-登后拒动", j8, "登记后取证与绑值均拒");

    // J9 容量边界
    let j9 = {
        let mut t = GateLive::new();
        let mut q = 1u32;
        let mut filled = true;
        while q <= CAP as u32 {
            if t.bind(q, true).is_err() {
                filled = false;
            }
            q += 1;
        }
        filled && t.bind(99, true).err() == Some(GL_CAP)
    };
    s.add("J9-容量边界", j9, "超容必拒且原账不损");

    // J10 指纹随取证推进且登记后稳定
    let j10 = {
        let mut t = GateLive::new();
        let _ = t.bind(1, true);
        let f0 = t.fold();
        let _ = t.verify(1, true);
        let f1 = t.fold();
        let mut q = 2u32;
        while q <= COND_N as u32 {
            let _ = t.bind(q, true);
            let _ = t.verify(q, true);
            q += 1;
        }
        let _ = t.register_release("丙", 9);
        let f3 = t.fold();
        f0 != f1 && f1 != f3 && t.fold() == f3
    };
    s.add("J10-指纹推进", j10, "每取证一步指纹一步且登记后稳定");

    // J11 要件为假的条件须取证为假才过
    let j11 = {
        let mut t = GateLive::new();
        let _ = t.bind(3, false);
        let _ = t.verify(3, false);
        t.conds[0].ok()
    };
    s.add("J11-假件过法", j11, "要件为假时取证为假亦过条");

    // J12 动态明细：失败条计数落账
    let fl = g.failed_list().len();
    s.add_dyn("J12-失败条数", fl == 0, &format!("failed={}", fl));

    // J13 动态明细：闸指纹落账
    let fp = g.fold();
    s.add_dyn("J13-闸指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10331_gate_live_flow() {
        let mut g = GateLive::new();
        assert!(g.bind(0, true).err() == Some(GL_ENO));
        assert!(g.bind(6, true).err() == Some(GL_ENO));
        assert!(g.bind(1, true).is_ok());
        assert!(g.bind(1, false).err() == Some(GL_DUP));
        assert!(g.verify(9, true).err() == Some(GL_NOC));
        assert!(g.verify(1, false).is_ok());
        assert!(g.conds[0].failed());
        assert!(g.verify(1, true).err() == Some(GL_DUP));
        assert!(g.register_release("甲", 1).err() == Some(GL_UNVER));
        let mut q = 2u32;
        while q <= COND_N as u32 {
            let _ = g.bind(q, true);
            let _ = g.verify(q, true);
            q += 1;
        }
        // 条 1 取证为假，须重开账
        let mut g2 = GateLive::new();
        let mut q2 = 1u32;
        while q2 <= COND_N as u32 {
            let _ = g2.bind(q2, true);
            let _ = g2.verify(q2, true);
            q2 += 1;
        }
        assert!(g2.register_release("", 3).err() == Some(GL_EMPTY));
        assert!(g2.register_release("乙", 3).is_ok());
        assert!(g2.released());
        assert!(g2.register_release("丙", 4).err() == Some(GL_REREG));
    }

    #[test]
    fn unx10331_checks_green() {
        let s = run_unx10331_checks();
        assert!(s.all_passed(), "UNX-F10331 判据未全绿");
    }
}
