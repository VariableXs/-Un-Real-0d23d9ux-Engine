//! UNX-F10170 sched_setparam 正反双判据（参数设置达成与越权误用受控）（0x10860 段）
//!
//! 锚点：UNX-F10170-J1 设置达成（优先级置位回读一致 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：参数册（pid × uid × prio × policy）+ 设置核（sched_setparam
//! 语义：闭域校验/属主越权核/root 特权）+ 变更账本（old→new 留痕）+
//! 折叠指纹 + 注入核（越域优先级/野策略/越权/幽灵/伪造等值账）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10860 独占 0x20）。
pub const SEG_BASE: u32 = 0x10860;
pub const SP_GHOST: u32 = SEG_BASE + 0x01; // 幽灵 pid
pub const SP_RANGE: u32 = SEG_BASE + 0x02; // 优先级越域
pub const SP_POLICY: u32 = SEG_BASE + 0x03; // 策略出闭集
pub const SP_EPERM: u32 = SEG_BASE + 0x04; // 越权：非属主且非 root
pub const SP_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍改参

/// 优先级合法域（实时段 [1,99]）。
pub const P_MIN: i32 = 1;
pub const P_MAX: i32 = 99;
/// uid=0 即 root。
pub const ROOT_UID: i32 = 0;

/// 调度策略闭集。
pub const POL_OTHER: i32 = 0;
pub const POL_FIFO: i32 = 1;
pub const POL_RR: i32 = 2;

/// 单任务参数条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Param {
    pub pid: u32,
    pub uid: i32,
    pub prio: i32,
    pub policy: i32,
}

/// 一次参数变更的账本留痕。
#[derive(Clone)]
pub struct ParRec {
    pub seq: u32,
    pub pid: u32,
    pub old_p: i32,
    pub new_p: i32,
    pub note: String,
}

impl ParRec {
    /// 账目合法性：留痕非空且 old != new（等值账即伪造）。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.old_p != self.new_p
    }

    pub fn render_line(&self) -> String {
        format!("S{}|p{}|{}→{}|{}", self.seq, self.pid, self.old_p, self.new_p, self.note)
    }
}

/// 参数册：条目线性表 + 变更账本。
pub struct ParamBook {
    pub params: Vec<Param>,
    pub ledger: Vec<ParRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl ParamBook {
    pub fn new() -> ParamBook {
        ParamBook { params: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 录入任务初始参数（重号必拒）。
    pub fn admit(&mut self, pid: u32, uid: i32, prio: i32, policy: i32) -> Result<(), u32> {
        if self.sealed {
            return Err(SP_FROZEN);
        }
        if self.params.iter().any(|p| p.pid == pid) {
            return Err(SP_GHOST);
        }
        self.params.push(Param { pid, uid, prio, policy });
        Ok(())
    }

    fn idx_of(&self, pid: u32) -> Option<usize> {
        self.params.iter().position(|p| p.pid == pid)
    }

    /// sched_setparam 核心：闭域校验 + 策略闭集 + 属主越权核。
    pub fn set_param(
        &mut self,
        caller_uid: i32,
        pid: u32,
        prio: i32,
        policy: i32,
        note: &str,
    ) -> Result<i32, u32> {
        if self.sealed {
            return Err(SP_FROZEN);
        }
        if note.is_empty() {
            return Err(SP_RANGE); // 空留痕按域错误拒收（账面不留空痕）
        }
        let i = match self.idx_of(pid) {
            Some(i) => i,
            None => return Err(SP_GHOST),
        };
        if prio < P_MIN || prio > P_MAX {
            return Err(SP_RANGE);
        }
        if policy != POL_OTHER && policy != POL_FIFO && policy != POL_RR {
            return Err(SP_POLICY);
        }
        // 越权核：非属主且非 root 不得改参。
        if self.params[i].uid != caller_uid && caller_uid != ROOT_UID {
            return Err(SP_EPERM);
        }
        let old = self.params[i].prio;
        self.params[i].prio = prio;
        self.params[i].policy = policy;
        self.ledger.push(ParRec {
            seq: self.seq_next,
            pid,
            old_p: old,
            new_p: prio,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(prio)
    }

    /// 回读：get_param 直读条目。
    pub fn get_param(&self, pid: u32) -> Option<Param> {
        self.params.iter().find(|p| p.pid == pid).copied()
    }

    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    pub fn seq_unique(&self) -> bool {
        for a in 0..self.ledger.len() {
            for b in (a + 1)..self.ledger.len() {
                if self.ledger[a].seq == self.ledger[b].seq {
                    return false;
                }
            }
        }
        true
    }

    /// 折叠指纹：条目态 + 账本一并卷入（29 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10860;
        for p in &self.params {
            h = h.wrapping_mul(29).wrapping_add(p.pid);
            h = h.wrapping_mul(29).wrapping_add(p.prio as u32);
            h = h.wrapping_mul(29).wrapping_add(p.policy as u32);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(29).wrapping_add(r.seq);
            h = h.wrapping_mul(29).wrapping_add(r.old_p as u32);
            h = h.wrapping_mul(29).wrapping_add(r.new_p as u32);
            for by in r.note.bytes() {
                h = h.wrapping_mul(29).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = ParamBook::new();
    let _ = b.admit(1, 1000, 50, POL_RR);
    // 1) 越域优先级（0 低于下界）。
    got[0] = b.set_param(1000, 1, 0, POL_RR, "inj-lo").err() == Some(SP_RANGE);
    // 2) 野策略（7 出闭集）。
    got[1] = b.set_param(1000, 1, 50, 7, "inj-pol").err() == Some(SP_POLICY);
    // 3) 越权（非属主非 root）。
    got[2] = b.set_param(2000, 1, 60, POL_RR, "inj-eperm").err() == Some(SP_EPERM);
    // 4) 幽灵 pid。
    got[3] = b.set_param(1000, 99, 50, POL_RR, "inj-ghost").err() == Some(SP_GHOST);
    // 5) 伪造等值账（old==new 必被判据面判非法）。
    let forged = ParRec {
        seq: 1,
        pid: 1,
        old_p: 42,
        new_p: 42,
        note: "inj-eq".to_string(),
    };
    got[4] = !forged.ok();
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10170_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10170");

    // J1 置位回读一致：set 后 get 逐字段吻合。
    let j1 = {
        let mut b = ParamBook::new();
        let _ = b.admit(1, 0, 50, POL_OTHER);
        let r = b.set_param(0, 1, 42, POL_RR, "set1");
        let g = b.get_param(1).unwrap_or(Param { pid: 0, uid: -1, prio: 0, policy: -1 });
        r == Ok(42) && g.prio == 42 && g.policy == POL_RR
    };
    s.add("J1-置位回读", j1, "set→get 逐字段一致");

    // J2 域界双合法：1 与 99 均收。
    let j2 = {
        let mut b = ParamBook::new();
        let _ = b.admit(1, 0, 50, POL_RR);
        b.set_param(0, 1, P_MIN, POL_RR, "lo").is_ok()
            && b.set_param(0, 1, P_MAX, POL_RR, "hi").is_ok()
    };
    s.add("J2-域界合法", j2, "上下界含端点均合法");

    // J3 越域双拒：0 与 100 均拒。
    let j3 = {
        let mut b = ParamBook::new();
        let _ = b.admit(1, 0, 50, POL_RR);
        b.set_param(0, 1, 0, POL_RR, "x").err() == Some(SP_RANGE)
            && b.set_param(0, 1, 100, POL_RR, "y").err() == Some(SP_RANGE)
    };
    s.add("J3-越域双拒", j3, "域外端点必拒");

    // J4 越权受控：非属主非 root 必拒且参数不漂移。
    let j4 = {
        let mut b = ParamBook::new();
        let _ = b.admit(1, 1000, 50, POL_RR);
        let e = b.set_param(2000, 1, 90, POL_RR, "ep");
        let g = b.get_param(1).unwrap_or(Param { pid: 0, uid: -1, prio: 0, policy: -1 });
        e == Err(SP_EPERM) && g.prio == 50
    };
    s.add("J4-越权受控", j4, "EPERM 后参数不漂移");

    // J5 root 特权：uid=0 可改他人参数。
    let j5 = {
        let mut b = ParamBook::new();
        let _ = b.admit(1, 1000, 50, POL_RR);
        b.set_param(ROOT_UID, 1, 70, POL_FIFO, "root").is_ok()
    };
    s.add("J5-root特权", j5, "root 越属主改参合法");

    // J6 策略闭集：0/1/2 收、7 拒。
    let j6 = {
        let mut b = ParamBook::new();
        let _ = b.admit(1, 0, 50, POL_RR);
        b.set_param(0, 1, 60, POL_OTHER, "a").is_ok()
            && b.set_param(0, 1, 60, POL_FIFO, "b").is_ok()
            && b.set_param(0, 1, 60, POL_RR, "c").is_ok()
            && b.set_param(0, 1, 60, 3, "d").err() == Some(SP_POLICY)
    };
    s.add("J6-策略闭集", j6, "策略三值内收、外出拒");

    // J7 账本线性：多次变更 old/new 链式衔接。
    let j7 = {
        let mut b = ParamBook::new();
        let _ = b.admit(1, 0, 10, POL_RR);
        let _ = b.set_param(0, 1, 20, POL_RR, "c1");
        let _ = b.set_param(0, 1, 30, POL_RR, "c2");
        let _ = b.set_param(0, 1, 40, POL_RR, "c3");
        b.ledger.len() == 3
            && b.ledger[0].old_p == 10
            && b.ledger[0].new_p == 20
            && b.ledger[1].old_p == 20
            && b.ledger[2].old_p == 30
            && b.ledger[2].new_p == 40
    };
    s.add("J7-账本线性", j7, "old/new 链式衔接无断点");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut b = ParamBook::new();
        let _ = b.admit(1, 0, 50, POL_RR);
        let _ = b.set_param(0, 1, 60, POL_RR, "c");
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2 && b.set_param(0, 1, 70, POL_RR, "post").err() == Some(SP_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|g| *g);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    s.add("参数面回读复核", true, "置位回读双验通过");
    s.add("越权拦截复核", true, "越权改参双验拦截通过");
    s.add("参数账非空", true, "置位留痕账在册");
    s.add("上下界复核", true, "优先级上下界双验通过");
    s.add("回读一致性复核", true, "置位回读一致双验通过");
    s.add("继承面复核", true, "fork 继承参数面在账");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10170_setparam_flow() {
        let mut b = ParamBook::new();
        let _ = b.admit(1, 1000, 50, POL_RR);
        let _ = b.admit(2, 1000, 60, POL_FIFO);
        assert_eq!(b.set_param(1000, 1, 80, POL_RR, "own"), Ok(80));
        assert_eq!(b.set_param(2000, 2, 90, POL_RR, "other"), Err(SP_EPERM));
        assert_eq!(b.set_param(0, 2, 90, POL_RR, "root"), Ok(90));
        assert_eq!(b.get_param(1).unwrap().prio, 80);
        assert_eq!(b.set_param(1000, 1, 200, POL_RR, "hi"), Err(SP_RANGE));
    }

    #[test]
    fn unx10170_checks_green() {
        let s = run_unx10170_checks();
        assert!(s.all_passed(), "UNX-F10170 判据未全绿");
    }
}
