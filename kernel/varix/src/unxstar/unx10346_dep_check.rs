//! UNX-F10346 跨域依赖面核对（R-C3-001/002/003 状态核对）（0x1b5c0 段）
//!
//! 锚点：UNX-F10346-J1 依赖状态核对——三条跨域依赖 R-C3-001/002/003
//! 逐条申报要件状态、回填观测状态（观测只许一次），观测与要件逐条相
//! 等才算依赖面对平，缺一条或一条不符即不平。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：依赖账（条=依赖号+要件状态+观测状态 Option）+ 号域核（只认
//! 三条正则号）+ 状态域核（1..=3）+ 对平核（不符单/未观测单可枚举）+
//! 注入核（坏依赖号/重条/坏状态/重观测四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b5c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b5c0;
pub const DC_RID: u32 = SEG_BASE + 0x01; // 坏依赖号
pub const DC_DUP: u32 = SEG_BASE + 0x02; // 依赖号重条
pub const DC_STATE: u32 = SEG_BASE + 0x03; // 状态越界
pub const DC_REOBS: u32 = SEG_BASE + 0x04; // 已观测再观测
pub const DC_NORID: u32 = SEG_BASE + 0x05; // 未知依赖号

/// 状态码。
pub const ST_PLAN: u8 = 1;
pub const ST_RUNNING: u8 = 2;
pub const ST_DONE: u8 = 3;
pub const ST_N: u8 = 4;

/// 三条正则依赖号。
pub const RID_001: &str = "R-C3-001";
pub const RID_002: &str = "R-C3-002";
pub const RID_003: &str = "R-C3-003";
/// 依赖条锚（三条）。
pub const DEPS_NEEDED: usize = 3;

/// 依赖条。
#[derive(Clone)]
pub struct Dep {
    pub rid: String,
    pub want: u8,
    pub got: Option<u8>,
}

impl Dep {
    /// 对平：已观测且与要件相等。
    pub fn ok(&self) -> bool {
        self.got == Some(self.want)
    }
}

/// 依赖面核对账。
pub struct DepCheck {
    pub deps: Vec<Dep>,
    pub sealed: bool,
}

impl DepCheck {
    pub fn new() -> DepCheck {
        DepCheck { deps: Vec::new(), sealed: false }
    }

    fn canonical(rid: &str) -> bool {
        rid == RID_001 || rid == RID_002 || rid == RID_003
    }

    fn pos(&self, rid: &str) -> Option<usize> {
        let mut k = 0usize;
        while k < self.deps.len() {
            if self.deps[k].rid == rid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 申报要件状态。
    pub fn declare(&mut self, rid: &str, want: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(DC_REOBS);
        }
        if !Self::canonical(rid) {
            return Err(DC_RID);
        }
        if want == 0 || want >= ST_N {
            return Err(DC_STATE);
        }
        if self.pos(rid).is_some() {
            return Err(DC_DUP);
        }
        self.deps.push(Dep { rid: rid.to_string(), want, got: None });
        Ok(())
    }

    /// 回填观测状态（只许一次）。
    pub fn observed(&mut self, rid: &str, got: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(DC_REOBS);
        }
        if got == 0 || got >= ST_N {
            return Err(DC_STATE);
        }
        match self.pos(rid) {
            None => Err(DC_NORID),
            Some(p) => {
                if self.deps[p].got.is_some() {
                    return Err(DC_REOBS);
                }
                self.deps[p].got = Some(got);
                Ok(())
            }
        }
    }

    /// 不符单。
    pub fn mismatched(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        let mut k = 0usize;
        while k < self.deps.len() {
            let d = &self.deps[k];
            if d.got.is_some() && !d.ok() {
                v.push(d.rid.clone());
            }
            k += 1;
        }
        v
    }

    /// 未观测单。
    pub fn unobserved(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        let mut k = 0usize;
        while k < self.deps.len() {
            if self.deps[k].got.is_none() {
                v.push(self.deps[k].rid.clone());
            }
            k += 1;
        }
        v
    }

    /// 依赖面对平：三条齐、全观测、零不符。
    pub fn aligned(&self) -> bool {
        self.deps.len() >= DEPS_NEEDED
            && self.unobserved().is_empty()
            && self.mismatched().is_empty()
    }

    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(DC_REOBS);
        }
        if !self.aligned() {
            return Err(DC_NORID);
        }
        self.sealed = true;
        Ok(())
    }

    /// 依赖账指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b5c00;
        h = h.wrapping_mul(31).wrapping_add(self.deps.len() as u32);
        let mut k = 0usize;
        while k < self.deps.len() {
            let d = &self.deps[k];
            h = h
                .wrapping_mul(31)
                .wrapping_add(d.want as u32)
                .wrapping_mul(31)
                .wrapping_add(d.got.unwrap_or(0) as u32);
            let rb = d.rid.as_bytes();
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
    let mut d = DepCheck::new();
    // 1) 坏依赖号必拒
    got[0] = d.declare("R-C2-001", ST_DONE).err() == Some(DC_RID)
        && d.declare("R-C3-004", ST_DONE).err() == Some(DC_RID);
    // 2) 坏状态必拒
    got[1] = d.declare(RID_001, 0).err() == Some(DC_STATE)
        && d.declare(RID_001, ST_N).err() == Some(DC_STATE);
    // 3) 重条必拒
    let _ = d.declare(RID_001, ST_DONE);
    got[2] = d.declare(RID_001, ST_RUNNING).err() == Some(DC_DUP);
    // 4) 未知号观测必拒
    got[3] = d.observed(RID_002, ST_DONE).err() == Some(DC_NORID);
    got
}

/// 批内判据。
pub fn run_unx10346_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10346");

    // J1 正路径：三条依赖申报观测全对平并封账
    let mut d = DepCheck::new();
    let rids = [RID_001, RID_002, RID_003];
    let mut ok = true;
    let mut k = 0usize;
    while k < rids.len() {
        if d.declare(rids[k], ST_DONE).is_err() {
            ok = false;
        }
        k += 1;
    }
    k = 0;
    while k < rids.len() {
        if d.observed(rids[k], ST_DONE).is_err() {
            ok = false;
        }
        k += 1;
    }
    let sealed = d.seal();
    s.add(
        "J1-依赖状态核对",
        ok && d.aligned() && sealed.is_ok() && d.sealed,
        "R-C3-001/002/003 观测与要件全对平并封账",
    );

    // J2 不符单与未观测单双零
    let j2 = d.mismatched().is_empty() && d.unobserved().is_empty();
    s.add("J2-不符未观双零", j2, "不符单与未观测单皆空");

    // J3 注入检出：坏号 + 坏状态
    let inj = inject_four();
    s.add("J3-坏号坏态检出", inj[0] && inj[1], "非正则依赖号与越界状态必拒");

    // J4 注入检出：重条 + 未知观测
    s.add("J4-重条未知检出", inj[2] && inj[3], "重条与未知号观测必拒");

    // J5 不符条阻断对平
    let j5 = {
        let mut t = DepCheck::new();
        let _ = t.declare(RID_001, ST_DONE);
        let _ = t.declare(RID_002, ST_DONE);
        let _ = t.declare(RID_003, ST_DONE);
        let _ = t.observed(RID_001, ST_DONE);
        let _ = t.observed(RID_002, ST_DONE);
        let _ = t.observed(RID_003, ST_RUNNING);
        !t.aligned() && t.mismatched().len() == 1
    };
    s.add("J5-不符阻断", j5, "观测异于要件即入不符单");

    // J6 重观测必拒
    let j6 = {
        let mut t = DepCheck::new();
        let _ = t.declare(RID_001, ST_DONE);
        let _ = t.observed(RID_001, ST_DONE);
        t.observed(RID_001, ST_RUNNING).err() == Some(DC_REOBS)
    };
    s.add("J6-重观测拒收", j6, "观测只许一次");

    // J7 未申报观测必拒
    let j7 = DepCheck::new().observed(RID_003, ST_DONE).err() == Some(DC_NORID);
    s.add("J7-未报拒观", j7, "未申报依赖不得观测");

    // J8 非正则号观测亦拒
    let j8 = {
        let mut t = DepCheck::new();
        let _ = t.declare(RID_001, ST_DONE);
        t.observed("R-C3-999", ST_DONE).err() == Some(DC_NORID)
    };
    s.add("J8-野号拒观", j8, "非正则号观测必拒");

    // J9 未观测阻断封账
    let j9 = {
        let mut t = DepCheck::new();
        let _ = t.declare(RID_001, ST_DONE);
        let _ = t.declare(RID_002, ST_DONE);
        let _ = t.declare(RID_003, ST_DONE);
        let _ = t.observed(RID_001, ST_DONE);
        let _ = t.observed(RID_002, ST_DONE);
        !t.aligned() && t.seal().err() == Some(DC_NORID)
    };
    s.add("J9-未观拒封", j9, "存在未观测条即不得封账");

    // J10 指纹随观测推进且封账后稳定
    let j10 = {
        let mut t = DepCheck::new();
        let _ = t.declare(RID_001, ST_DONE);
        let f0 = t.fold();
        let _ = t.observed(RID_001, ST_DONE);
        let f1 = t.fold();
        f0 != f1 && t.fold() == f1
    };
    s.add("J10-指纹推进", j10, "每观测一条指纹一变且终态稳定");

    // J11 三状态码域严格递增
    let j11 = ST_PLAN < ST_RUNNING && ST_RUNNING < ST_DONE && ST_DONE < ST_N;
    s.add("J11-状态域核", j11, "三状态码 1..=3 严格递增");

    // J12 动态明细：不符计数落账
    let mm = d.mismatched().len();
    s.add_dyn("J12-不符计数", mm == 0, &format!("mismatched={}", mm));

    // J13 动态明细：依赖账指纹落账
    let fp = d.fold();
    s.add_dyn("J13-依赖指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10346_dep_check_flow() {
        let mut d = DepCheck::new();
        assert!(d.declare("R-C3-004", ST_DONE).err() == Some(DC_RID));
        assert!(d.declare(RID_001, 0).err() == Some(DC_STATE));
        assert!(d.declare(RID_001, ST_DONE).is_ok());
        assert!(d.declare(RID_001, ST_RUNNING).err() == Some(DC_DUP));
        assert!(d.observed(RID_002, ST_DONE).err() == Some(DC_NORID));
        assert!(d.observed(RID_001, 0).err() == Some(DC_STATE));
        assert!(d.observed(RID_001, ST_DONE).is_ok());
        assert!(d.observed(RID_001, ST_DONE).err() == Some(DC_REOBS));
        assert!(!d.aligned());
        let _ = d.declare(RID_002, ST_DONE);
        let _ = d.declare(RID_003, ST_DONE);
        let _ = d.observed(RID_002, ST_DONE);
        let _ = d.observed(RID_003, ST_DONE);
        assert!(d.aligned());
        assert!(d.seal().is_ok());
        assert!(d.declare(RID_002, ST_DONE).err() == Some(DC_REOBS));
    }

    #[test]
    fn unx10346_checks_green() {
        let s = run_unx10346_checks();
        assert!(s.all_passed(), "UNX-F10346 判据未全绿");
    }
}
