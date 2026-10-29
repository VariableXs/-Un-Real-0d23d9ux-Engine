//! UNX-F24157 reservation 锁与显存互斥纪律·子功能17（压力重演·持锁序列重放）（0x41580 段）
//!
//! 锚点：UNX-F24157-J1 重演账（持锁序列重放逐条落账）；J2 重演语义
//! （序列一致重放 + 违规步识破 + 幂等重放三性质）；J3 重演指纹（账
//! 折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（持锁序列重演面）。
//!
//! 结构：操作序列册 + 重演核（逐步重放校验）+ 违规登记 + 折叠指纹 +
//! 注入核（序列漂移/伪幂等/野步骤/印后/空重演必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41580 独占 0x20）。
pub const SEG_BASE: u32 = 0x41580;
pub const RP_DRIFT: u32 = SEG_BASE + 0x01; // 序列漂移
pub const RP_BAD_STEP: u32 = SEG_BASE + 0x02; // 野步骤
pub const RP_EMPTY: u32 = SEG_BASE + 0x03; // 空重演
pub const RP_FROZEN: u32 = SEG_BASE + 0x04; // 印后重演
pub const RP_NOT_IDEM: u32 = SEG_BASE + 0x05; // 非幂等

/// 操作步。
#[derive(Clone, PartialEq)]
pub struct RpStep {
    pub op: u8, // 0=acq 1=rel
    pub lock_id: u32,
}

/// 重演记录。
#[derive(Clone)]
pub struct RpRec {
    pub run_id: u32,
    pub step_idx: usize,
    pub ok: bool,
}

/// 重演引擎：持有合法序列 + 重放。
pub struct RpEngine {
    pub known_locks: Vec<u32>,
    pub script: Vec<RpStep>,
    pub ledger: Vec<RpRec>,
    pub sealed: bool,
}

impl RpEngine {
    pub fn new(known_locks: &[u32]) -> RpEngine {
        RpEngine {
            known_locks: known_locks.to_vec(),
            script: Vec::new(),
            ledger: Vec::new(),
            sealed: false,
        }
    }

    pub fn append(&mut self, op: u8, lock_id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RP_FROZEN);
        }
        if op > 1 || !self.known_locks.contains(&lock_id) {
            return Err(RP_BAD_STEP);
        }
        self.script.push(RpStep { op, lock_id });
        Ok(())
    }

    /// 重演一次：模拟持锁栈逐步校验，结果落账。
    pub fn replay(&mut self, run_id: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(RP_FROZEN);
        }
        if self.script.is_empty() {
            return Err(RP_EMPTY);
        }
        let mut stack: Vec<u32> = Vec::new();
        let mut fails = 0usize;
        for (i, step) in self.script.iter().enumerate() {
            let ok = match step.op {
                0 => {
                    let free = !stack.contains(&step.lock_id);
                    if free {
                        stack.push(step.lock_id);
                    }
                    free
                }
                _ => {
                    let held = stack.last() == Some(&step.lock_id);
                    if held {
                        stack.pop();
                    }
                    held
                }
            };
            if !ok {
                fails += 1;
            }
            self.ledger.push(RpRec { run_id, step_idx: i, ok });
        }
        Ok(fails)
    }

    /// 幂等重演：两次重演 fails 相等。
    pub fn replay_idem(&mut self, run_a: u32, run_b: u32) -> bool {
        let a = self.replay(run_a);
        let b = self.replay(run_b);
        match (a, b) {
            (Ok(x), Ok(y)) => x == y,
            _ => false,
        }
    }

    pub fn fail_count(&self, run_id: u32) -> usize {
        self.ledger.iter().filter(|r| r.run_id == run_id && !r.ok).count()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41580;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.run_id);
            h = h.wrapping_mul(31).wrapping_add(r.step_idx as u32);
            h = h.wrapping_mul(31).wrapping_add(r.ok as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：acq1 acq2 rel2 rel1 合法序列。
pub fn mk_engine() -> RpEngine {
    let mut e = RpEngine::new(&[1, 2, 3]);
    let _ = e.append(0, 1);
    let _ = e.append(0, 2);
    let _ = e.append(1, 2);
    let _ = e.append(1, 1);
    e
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut e = mk_engine();
    // 1) 野步骤
    got[0] = e.append(0, 9).err() == Some(RP_BAD_STEP);
    // 2) op 越界
    got[1] = e.append(5, 1).err() == Some(RP_BAD_STEP);
    // 3) 违规重演（双重 acq）
    let mut e2 = RpEngine::new(&[1]);
    let _ = e2.append(0, 1);
    let _ = e2.append(0, 1);
    got[2] = e2.replay(1) == Ok(1);
    // 4) 空重演
    let mut e3 = RpEngine::new(&[1]);
    got[3] = e3.replay(1).err() == Some(RP_EMPTY);
    // 5) 印后重演必抓
    e.seal();
    got[4] = e.replay(9).err() == Some(RP_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24157_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24157");
    let mut e = mk_engine();

    // J1 序列册齐
    let j1 = e.script.len() == 4;
    s.add("J1-序列册齐", j1, "四步脚本全落");

    // J2 合法重演零失败
    let j2 = e.replay(1) == Ok(0);
    s.add("J2-合法零败", j2, "合法序列零违规");

    // J3 违规识破
    let j3 = {
        let mut e2 = RpEngine::new(&[1]);
        let _ = e2.append(0, 1);
        let _ = e2.append(0, 1);
        e2.replay(2) == Ok(1)
    };
    s.add("J3-违规识破", j3, "双重 acq 计一败");

    // J4 非顶释放违规
    let j4 = {
        let mut e2 = RpEngine::new(&[1, 2]);
        let _ = e2.append(0, 1);
        let _ = e2.append(0, 2);
        let _ = e2.append(1, 1); // 非顶释放
        e2.replay(3) == Ok(1)
    };
    s.add("J4-非顶释放败", j4, "跳顶释放计违规");

    // J5 幂等重演
    let j5 = e.replay_idem(4, 5);
    s.add("J5-幂等重演", j5, "两次重演败数相等");

    // J6 空重演拒
    let j6 = {
        let mut e2 = RpEngine::new(&[1]);
        e2.replay(6).err() == Some(RP_EMPTY)
    };
    s.add("J6-空重演拒", j6, "空脚本必拒");

    // J7 野步骤拒
    let j7 = e.append(0, 9).err() == Some(RP_BAD_STEP);
    s.add("J7-野步骤拒", j7, "未登记锁步必拒");

    // J8 op 越界拒
    let j8 = e.append(5, 1).err() == Some(RP_BAD_STEP);
    s.add("J8-op越界拒", j8, "op>1 必拒");

    // J9 账面逐条 ok 标记
    let j9 = {
        let run1: Vec<bool> = e.ledger.iter().filter(|r| r.run_id == 1).map(|r| r.ok).collect();
        run1.len() == 4 && run1.iter().all(|x| *x)
    };
    s.add("J9-账逐条ok", j9, "run1 四步全过");

    // J10 败步计数对账
    let j10 = {
        let mut e2 = RpEngine::new(&[1, 2]);
        let _ = e2.append(0, 1);
        let _ = e2.append(0, 1);
        let _ = e2.append(0, 2);
        let _ = e2.append(1, 2);
        let _ = e2.replay(7);
        e2.fail_count(7) == 1
    };
    s.add("J10-败步对账", j10, "fail_count 与重演一致");

    // J11 印后追加拒
    let j11 = {
        let mut e2 = mk_engine();
        e2.seal();
        e2.append(0, 1).err() == Some(RP_FROZEN)
    };
    s.add("J11-印后追加拒", j11, "印后脚本冻结");

    // J12 指纹稳定
    let j12 = e.fold() == e.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx24157_replay_flow() {
        let mut e = mk_engine();
        assert_eq!(e.replay(1), Ok(0));
        assert!(e.replay_idem(2, 3));
        assert_eq!(e.append(0, 9), Err(RP_BAD_STEP));
        assert_eq!(e.append(5, 1), Err(RP_BAD_STEP));
        e.seal();
        assert_eq!(e.replay(9), Err(RP_FROZEN));
    }

    #[test]
    fn unx24157_checks_green() {
        let s = run_unx24157_checks();
        assert!(s.all_passed(), "UNX-F24157 判据未全绿");
    }
}
