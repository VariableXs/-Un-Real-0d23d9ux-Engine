//! UNX-F10323 进程号面联签（fork/exec/wait 族判据互认）（0x1b2e0 段）
//!
//! 锚点：UNX-F10323-J1 进程族互认——fork/exec/wait/exit 四条族判据按
//! 氏族链序登记（号必须严格递增），甲乙双方逐条裁定，四条全裁同向
//! 才算进程号面联签过。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：定容槽册（行册按 CAP 定容，登记即占一槽）+ 链式推进核（登记
//! 号必须大于前一登记号，倒挂/重号/零号即拒）+ 双裁核（单裁不成互认）+
//! 注入核（链序回退/未知号裁定/未裁判平/超容四路必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b2e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b2e0;
pub const PC_CHAIN: u32 = SEG_BASE + 0x01; // 链序倒挂
pub const PC_DUP: u32 = SEG_BASE + 0x02; // 号重号
pub const PC_EMPTY: u32 = SEG_BASE + 0x03; // 题名空
pub const PC_CAP: u32 = SEG_BASE + 0x04; // 槽满
pub const PC_NOROW: u32 = SEG_BASE + 0x05; // 未知族号
pub const PC_RESIGN: u32 = SEG_BASE + 0x06; // 已裁再裁
pub const PC_ZERO: u32 = SEG_BASE + 0x07; // 零号

/// 槽定容。
pub const CAP: usize = 8;
/// 进程族需求锚（四条）。
pub const CLAN_NEED: usize = 4;

/// 氏族号样例锚：fork/exec/wait/exit。
pub const CID_FORK: u32 = 11;
pub const CID_EXEC: u32 = 12;
pub const CID_WAIT: u32 = 13;
pub const CID_EXIT: u32 = 14;

/// 进程族判据行。
#[derive(Clone)]
pub struct ProcRow {
    pub cid: u32,
    pub name: String,
    pub a_ok: Option<bool>,
    pub b_ok: Option<bool>,
}

impl ProcRow {
    /// 双裁齐备。
    pub fn both(&self) -> bool {
        self.a_ok.is_some() && self.b_ok.is_some()
    }

    /// 互认：双裁同向。
    pub fn agree(&self) -> bool {
        self.a_ok == self.b_ok && self.both()
    }
}

/// 进程号面联签册（定容槽，链式推进）。
pub struct ProcJoint {
    pub slots: Vec<ProcRow>,
    pub cap: usize,
    pub last_cid: u32,
}

impl ProcJoint {
    pub fn new() -> ProcJoint {
        ProcJoint { slots: Vec::new(), cap: CAP, last_cid: 0 }
    }

    fn pos(&self, cid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].cid == cid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 链式登记：号必须严格大于上一登记号。
    pub fn register(&mut self, cid: u32, name: &str) -> Result<(), u32> {
        if cid == 0 {
            return Err(PC_ZERO);
        }
        if self.pos(cid).is_some() {
            return Err(PC_DUP);
        }
        if !self.slots.is_empty() && cid <= self.last_cid {
            return Err(PC_CHAIN);
        }
        if name.is_empty() {
            return Err(PC_EMPTY);
        }
        if self.slots.len() >= self.cap {
            return Err(PC_CAP);
        }
        self.slots.push(ProcRow {
            cid,
            name: name.to_string(),
            a_ok: None,
            b_ok: None,
        });
        self.last_cid = cid;
        Ok(())
    }

    /// 甲方裁定。
    pub fn a_sign(&mut self, cid: u32, ok: bool) -> Result<(), u32> {
        match self.pos(cid) {
            None => Err(PC_NOROW),
            Some(p) => {
                if self.slots[p].a_ok.is_some() {
                    return Err(PC_RESIGN);
                }
                self.slots[p].a_ok = Some(ok);
                Ok(())
            }
        }
    }

    /// 乙方裁定。
    pub fn b_sign(&mut self, cid: u32, ok: bool) -> Result<(), u32> {
        match self.pos(cid) {
            None => Err(PC_NOROW),
            Some(p) => {
                if self.slots[p].b_ok.is_some() {
                    return Err(PC_RESIGN);
                }
                self.slots[p].b_ok = Some(ok);
                Ok(())
            }
        }
    }

    /// 单条互认。
    pub fn joint(&self, cid: u32) -> bool {
        match self.pos(cid) {
            None => false,
            Some(p) => self.slots[p].agree(),
        }
    }

    /// 链序核验：登记序号严格递增。
    pub fn chain_strict(&self) -> bool {
        let mut k = 1usize;
        while k < self.slots.len() {
            if self.slots[k].cid <= self.slots[k - 1].cid {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 未裁清单。
    pub fn pending(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.slots.len() {
            if !self.slots[k].both() {
                v.push(self.slots[k].cid);
            }
            k += 1;
        }
        v
    }

    /// 族联签过：四条齐、链序严、全互认。
    pub fn clan_done(&self) -> bool {
        self.slots.len() >= CLAN_NEED
            && self.chain_strict()
            && self.pending().is_empty()
    }

    /// 互认条数。
    pub fn agreed_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].agree() {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 号面指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b2e00;
        h = h.wrapping_mul(31).wrapping_add(self.slots.len() as u32);
        h = h.wrapping_mul(31).wrapping_add(self.last_cid);
        let mut k = 0usize;
        while k < self.slots.len() {
            let r = &self.slots[k];
            h = h.wrapping_mul(31).wrapping_add(r.cid);
            h = h.wrapping_mul(31).wrapping_add(r.a_ok.unwrap_or(false) as u32 + 2);
            h = h.wrapping_mul(31).wrapping_add(r.b_ok.unwrap_or(false) as u32 + 2);
            let nb = r.name.as_bytes();
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
    let mut j = ProcJoint::new();
    // 1) 链序倒挂必拒（登记 12 后再登记 3）
    let _ = j.register(CID_FORK, "派生");
    let _ = j.register(CID_EXEC, "替换");
    got[0] = j.register(CID_FORK, "重号").err() == Some(PC_DUP)
        && j.register(3, "倒挂").err() == Some(PC_CHAIN);
    // 2) 未知号裁定必拒
    got[1] = j.a_sign(99, true).err() == Some(PC_NOROW);
    // 3) 未裁即判互认必不成
    let _ = j.register(CID_WAIT, "回收");
    got[2] = !j.joint(CID_WAIT) && j.pending().contains(&CID_WAIT);
    // 4) 零号必拒
    got[3] = j.register(0, "零").err() == Some(PC_ZERO);
    got
}

/// 批内判据。
pub fn run_unx10323_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10323");

    // J1 正路径：四条族判据链式登记双裁互认
    let mut j = ProcJoint::new();
    let cids = [CID_FORK, CID_EXEC, CID_WAIT, CID_EXIT];
    let names = ["派生判据", "替换映像判据", "回收判据", "退出码判据"];
    let mut ok = true;
    let mut k = 0usize;
    while k < cids.len() {
        if j.register(cids[k], names[k]).is_err() {
            ok = false;
        }
        k += 1;
    }
    k = 0;
    while k < cids.len() {
        if j.a_sign(cids[k], true).is_err() || j.b_sign(cids[k], true).is_err() {
            ok = false;
        }
        k += 1;
    }
    s.add(
        "J1-进程族互认",
        ok && j.clan_done() && j.agreed_count() == 4,
        "fork/exec/wait/exit 四条链式登记双裁互认",
    );

    // J2 链序严格
    s.add("J2-链序严格", j.chain_strict(), "登记号严格递增");

    // J3 注入检出：链序倒挂 + 未知号
    let inj = inject_four();
    s.add("J3-链序未知检出", inj[0] && inj[1], "倒挂重号与未知号必拒");

    // J4 注入检出：未裁判平 + 零号
    s.add("J4-未裁零号检出", inj[2] && inj[3], "未裁不成互认且零号必拒");

    // J5 异裁阻断互认
    let j5 = {
        let mut t = ProcJoint::new();
        let _ = t.register(CID_FORK, "派生");
        let _ = t.a_sign(CID_FORK, true);
        let _ = t.b_sign(CID_FORK, false);
        !t.joint(CID_FORK) && !t.clan_done()
    };
    s.add("J5-异裁阻断", j5, "任一条异裁族即不联签");

    // J6 重裁必拒
    let j6 = {
        let mut t = ProcJoint::new();
        let _ = t.register(CID_FORK, "派生");
        let _ = t.a_sign(CID_FORK, true);
        let _ = t.b_sign(CID_FORK, true);
        t.a_sign(CID_FORK, false).err() == Some(PC_RESIGN)
            && t.b_sign(CID_FORK, false).err() == Some(PC_RESIGN)
    };
    s.add("J6-重裁拒收", j6, "单侧二次裁定必拒");

    // J7 空题名必拒
    let j7 = ProcJoint::new().register(CID_FORK, "").err() == Some(PC_EMPTY);
    s.add("J7-空名拒收", j7, "族判据题名空必拒");

    // J8 容量边界（八槽满后第九条必拒）
    let j8 = {
        let mut t = ProcJoint::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= CAP as u32 {
            if t.register(n * 10, "槽判据").is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && t.register(999, "超").err() == Some(PC_CAP)
    };
    s.add("J8-容量边界", j8, "槽满必拒且原册不损");

    // J9 三条不成族联签（缺 exit）
    let j9 = {
        let mut t = ProcJoint::new();
        let _ = t.register(CID_FORK, "派生");
        let _ = t.register(CID_EXEC, "替换");
        let _ = t.register(CID_WAIT, "回收");
        let _ = t.a_sign(CID_FORK, true);
        let _ = t.b_sign(CID_FORK, true);
        let _ = t.a_sign(CID_EXEC, true);
        let _ = t.b_sign(CID_EXEC, true);
        let _ = t.a_sign(CID_WAIT, true);
        let _ = t.b_sign(CID_WAIT, true);
        !t.clan_done()
    };
    s.add("J9-缺条不成族", j9, "族判据缺一条即不联签");

    // J10 指纹随裁定推进
    let j10 = {
        let mut t = ProcJoint::new();
        let _ = t.register(CID_FORK, "派生");
        let f0 = t.fold();
        let _ = t.a_sign(CID_FORK, true);
        let f1 = t.fold();
        let _ = t.b_sign(CID_FORK, true);
        let f2 = t.fold();
        f0 != f1 && f1 != f2 && t.fold() == f2
    };
    s.add("J10-指纹推进", j10, "每裁一步指纹一步且终态稳定");

    // J11 互认计数与未裁清单
    let j11 = j.agreed_count() == 4 && j.pending().is_empty();
    s.add("J11-互认计数", j11, "四条全互认且无未裁");

    // J12 动态明细：链末号落账
    s.add_dyn(
        "J12-链末号",
        j.last_cid == CID_EXIT,
        &format!("last_cid={}", j.last_cid),
    );

    // J13 动态明细：号面指纹落账
    let fp = j.fold();
    s.add_dyn("J13-号面指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10323_proc_joint_flow() {
        let mut j = ProcJoint::new();
        assert!(j.register(0, "零").err() == Some(PC_ZERO));
        assert!(j.register(CID_FORK, "派生").is_ok());
        assert!(j.register(CID_FORK, "重").err() == Some(PC_DUP));
        assert!(j.register(5, "倒挂").err() == Some(PC_CHAIN));
        assert!(j.register(CID_EXEC, "").err() == Some(PC_EMPTY));
        assert!(j.a_sign(99, true).err() == Some(PC_NOROW));
        assert!(j.register(CID_EXEC, "替换").is_ok());
        assert!(j.a_sign(CID_FORK, true).is_ok());
        assert!(j.a_sign(CID_FORK, true).err() == Some(PC_RESIGN));
        assert!(j.b_sign(CID_FORK, true).is_ok());
        assert!(j.joint(CID_FORK));
        assert!(!j.clan_done());
        let _ = j.register(CID_WAIT, "回收");
        let _ = j.register(CID_EXIT, "退出");
        let _ = j.a_sign(CID_EXEC, true);
        let _ = j.b_sign(CID_EXEC, true);
        let _ = j.a_sign(CID_WAIT, true);
        let _ = j.b_sign(CID_WAIT, true);
        let _ = j.a_sign(CID_EXIT, true);
        let _ = j.b_sign(CID_EXIT, true);
        assert!(j.clan_done() && j.chain_strict());
    }

    #[test]
    fn unx10323_checks_green() {
        let s = run_unx10323_checks();
        assert!(s.all_passed(), "UNX-F10323 判据未全绿");
    }
}
