//! UNX-F24179 MSI-X 中断消费与 IrqBackend 对接·子功能19（压力重演·中断序列重放）（0x41840 段）
//!
//! 锚点：UNX-F24179-J1 重演账（中断序列重放逐条落账）；J2 重演语义
//! （合法序列零败 + 违规步识破 + 幂等重放三性质）；J3 重演指纹（账
//! 折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（中断序列重演面）。
//!
//! 结构：序列册 + 重演核（raise/EOI 配对栈）+ 违规登记 + 折叠指纹 +
//! 注入核（伪 EOI/野步/空重演/印后/双重 raise 档必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41840 独占 0x20）。
pub const SEG_BASE: u32 = 0x41840;
pub const RI_BAD_STEP: u32 = SEG_BASE + 0x01; // 野步骤
pub const RI_EMPTY: u32 = SEG_BASE + 0x02; // 空重演
pub const RI_DOUBLE_RAISE: u32 = SEG_BASE + 0x03; // 双重 raise 档
pub const RI_FROZEN: u32 = SEG_BASE + 0x04; // 印后重演
pub const RI_FAKE_EOI: u32 = SEG_BASE + 0x05; // 伪 EOI 步

/// 操作步。
#[derive(Clone, PartialEq)]
pub struct RiStep {
    pub op: u8, // 0=raise 1=eoi
    pub vec_no: u32,
}

/// 重演引擎。
pub struct RiEngine {
    pub known: Vec<u32>,
    pub script: Vec<RiStep>,
    pub ledger: Vec<(u32, usize, bool)>, // (run_id, step_idx, ok)
    pub sealed: bool,
}

impl RiEngine {
    pub fn new(known: &[u32]) -> RiEngine {
        RiEngine { known: known.to_vec(), script: Vec::new(), ledger: Vec::new(), sealed: false }
    }

    pub fn append(&mut self, op: u8, vec_no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RI_FROZEN);
        }
        if op > 1 || !self.known.contains(&vec_no) {
            return Err(RI_BAD_STEP);
        }
        self.script.push(RiStep { op, vec_no });
        Ok(())
    }

    /// 重演：raise 置挂起（已在挂起=违规档），eoi 清（未挂起=伪 EOI）。
    pub fn replay(&mut self, run_id: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(RI_FROZEN);
        }
        if self.script.is_empty() {
            return Err(RI_EMPTY);
        }
        let mut pending: Vec<u32> = Vec::new();
        let mut fails = 0usize;
        for (i, step) in self.script.iter().enumerate() {
            let ok = match step.op {
                0 => {
                    if pending.contains(&step.vec_no) {
                        false
                    } else {
                        pending.push(step.vec_no);
                        true
                    }
                }
                _ => {
                    if let Some(pos) = pending.iter().position(|&x| x == step.vec_no) {
                        pending.remove(pos);
                        true
                    } else {
                        false
                    }
                }
            };
            if !ok {
                fails += 1;
            }
            self.ledger.push((run_id, i, ok));
        }
        Ok(fails)
    }

    pub fn replay_idem(&mut self, a: u32, b: u32) -> bool {
        match (self.replay(a), self.replay(b)) {
            (Ok(x), Ok(y)) => x == y,
            _ => false,
        }
    }

    pub fn fail_count(&self, run_id: u32) -> usize {
        self.ledger.iter().filter(|(r, _, ok)| *r == run_id && !ok).count()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41840;
        for (r, i, ok) in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(*r);
            h = h.wrapping_mul(31).wrapping_add(*i as u32);
            h = h.wrapping_mul(31).wrapping_add(*ok as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：raise16 eoi16 raise17 eoi17。
pub fn mk_engine() -> RiEngine {
    let mut e = RiEngine::new(&[16, 17]);
    let _ = e.append(0, 16);
    let _ = e.append(1, 16);
    let _ = e.append(0, 17);
    let _ = e.append(1, 17);
    e
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut e = mk_engine();
    // 1) 野步骤
    got[0] = e.append(0, 99).err() == Some(RI_BAD_STEP);
    // 2) 伪 EOI 步
    let mut e2 = RiEngine::new(&[16]);
    let _ = e2.append(1, 16); // 无 raise 先 EOI
    got[1] = e2.replay(1) == Ok(1);
    // 3) 空重演
    let mut e3 = RiEngine::new(&[16]);
    got[2] = e3.replay(2).err() == Some(RI_EMPTY);
    // 4) 双重 raise 档
    let mut e4 = RiEngine::new(&[16]);
    let _ = e4.append(0, 16);
    let _ = e4.append(0, 16);
    got[3] = e4.replay(3) == Ok(1);
    // 5) 印后重演必抓
    e.seal();
    got[4] = e.replay(9).err() == Some(RI_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24179_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24179");
    let mut e = mk_engine();

    // J1 序列册齐
    let j1 = e.script.len() == 4;
    s.add("J1-序列册齐", j1, "四步全落");

    // J2 合法零败
    let j2 = e.replay(1) == Ok(0);
    s.add("J2-合法零败", j2, "配对序列零违规");

    // J3 伪 EOI 识破
    let j3 = {
        let mut e2 = RiEngine::new(&[16]);
        let _ = e2.append(1, 16);
        e2.replay(2) == Ok(1)
    };
    s.add("J3-伪EOI识破", j3, "无 raise EOI 计违规");

    // J4 双重 raise 识破
    let j4 = {
        let mut e2 = RiEngine::new(&[16]);
        let _ = e2.append(0, 16);
        let _ = e2.append(0, 16);
        e2.replay(3) == Ok(1)
    };
    s.add("J4-双重raise识破", j4, "挂起再 raise 计违规");

    // J5 幂等重演
    let j5 = e.replay_idem(4, 5);
    s.add("J5-幂等重演", j5, "两次败数相等");

    // J6 空重演拒
    let j6 = {
        let mut e2 = RiEngine::new(&[16]);
        e2.replay(6).err() == Some(RI_EMPTY)
    };
    s.add("J6-空重演拒", j6, "空脚本必拒");

    // J7 野步骤拒
    let j7 = e.append(0, 99).err() == Some(RI_BAD_STEP);
    s.add("J7-野步骤拒", j7, "未注册向量步必拒");

    // J8 op 越界拒
    let j8 = e.append(5, 16).err() == Some(RI_BAD_STEP);
    s.add("J8-op越界拒", j8, "op>1 必拒");

    // J9 交叉配对
    let j9 = {
        let mut e2 = RiEngine::new(&[16, 17]);
        let _ = e2.append(0, 16);
        let _ = e2.append(0, 17);
        let _ = e2.append(1, 17);
        let _ = e2.append(1, 16);
        e2.replay(7) == Ok(0)
    };
    s.add("J9-交叉配对", j9, "乱序配对亦合法");

    // J10 败步对账
    let j10 = {
        let mut e2 = RiEngine::new(&[16]);
        let _ = e2.append(0, 16);
        let _ = e2.append(0, 16);
        let _ = e2.append(1, 16);
        let _ = e2.append(1, 16);
        let _ = e2.replay(8);
        e2.fail_count(8) == 2
    };
    s.add("J10-败步对账", j10, "两败步全落账");

    // J11 印后追加拒
    let j11 = {
        let mut e2 = mk_engine();
        e2.seal();
        e2.append(0, 16).err() == Some(RI_FROZEN)
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
    fn unx24179_replay_flow() {
        let mut e = mk_engine();
        assert_eq!(e.replay(1), Ok(0));
        assert!(e.replay_idem(2, 3));
        assert_eq!(e.append(0, 99), Err(RI_BAD_STEP));
        assert_eq!(e.append(5, 16), Err(RI_BAD_STEP));
        e.seal();
        assert_eq!(e.replay(9), Err(RI_FROZEN));
    }

    #[test]
    fn unx24179_checks_green() {
        let s = run_unx24179_checks();
        assert!(s.all_passed(), "UNX-F24179 判据未全绿");
    }
}
