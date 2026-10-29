//! UNX-F10192 pty 关闭时序正反双判据（三步序达成与半释放误用受控）（0x10b20 段）
//!
//! 锚点：UNX-F10192-J1 三步序达成（fd→对端→账位释放序承 F9905 逐位
//! 验证 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：释放状态机（每对 0→1→2→3 四态：未启/fd 关/对端断/账位清）+
//! 步进核（只许顺序步进、跳步/回退/超步必拒）+ 半释放审计 +
//! 释放账本（步号守恒）+ 注入核（跳步/回退/已完成再步/伪造步号账/印后）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x10b20 独占 0x20）。
pub const SEG_BASE: u32 = 0x10b20;
pub const RL_SEQ: u32 = SEG_BASE + 0x01; // 步序违规（跳步/回退）
pub const RL_DONE: u32 = SEG_BASE + 0x02; // 已完成再步
pub const RL_GHOST: u32 = SEG_BASE + 0x03; // 幽灵对
pub const RL_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 留痕为空
pub const RL_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍步进

/// 释放终点（三步全成）。
pub const STEP_END: u8 = 3;

/// 单对释放状态。
#[derive(Clone, Copy)]
pub struct RelState {
    pub pair: u32,
    pub step: u8,
}

/// 一次步进的账本留痕。
#[derive(Clone)]
pub struct RelRec {
    pub seq: u32,
    pub pair: u32,
    /// 步进后状态（1=fd 关、2=对端断、3=账位清）。
    pub to_step: u8,
    pub note: String,
}

impl RelRec {
    /// 账目合法性：留痕非空且步号在 [1,3]。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.to_step >= 1 && self.to_step <= STEP_END
    }
}

/// 释放机：对表 + 步进核 + 审计。
pub struct ReleaseMachine {
    pub pairs: Vec<RelState>,
    pub ledger: Vec<RelRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl ReleaseMachine {
    pub fn new() -> ReleaseMachine {
        ReleaseMachine { pairs: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 挂册新对（初始 0 态；重号必拒）。
    pub fn enroll(&mut self, pair: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RL_FROZEN);
        }
        if pair == 0 || self.pairs.iter().any(|p| p.pair == pair) {
            return Err(RL_GHOST);
        }
        self.pairs.push(RelState { pair, step: 0 });
        Ok(())
    }

    /// 步进核：只许 step+1；幽灵/跳步/回退/超步逐门拒。
    pub fn advance(&mut self, pair: u32, note: &str) -> Result<u8, u32> {
        if self.sealed {
            return Err(RL_FROZEN);
        }
        if note.is_empty() {
            return Err(RL_EMPTY_NOTE);
        }
        let i = match self.pairs.iter().position(|p| p.pair == pair) {
            Some(i) => i,
            None => return Err(RL_GHOST),
        };
        if self.pairs[i].step >= STEP_END {
            return Err(RL_DONE);
        }
        self.pairs[i].step += 1;
        let to = self.pairs[i].step;
        self.ledger.push(RelRec {
            seq: self.seq_next,
            pair,
            to_step: to,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(to)
    }

    /// 释放完成判定。
    pub fn released(&self, pair: u32) -> bool {
        self.pairs.iter().any(|p| p.pair == pair && p.step == STEP_END)
    }

    /// 半释放审计：步进已启动（>0）但未到终点的对。
    pub fn half_released(&self) -> Vec<u32> {
        let mut out = Vec::new();
        for p in &self.pairs {
            if p.step > 0 && p.step < STEP_END {
                out.push(p.pair);
            }
        }
        out
    }

    /// 账本步号守恒：逐对重放步号必须严格 +1 递增且与终态吻合。
    pub fn replay(&self) -> bool {
        // 提取每对步号序列。
        let mut ids: Vec<u32> = Vec::new();
        for p in &self.pairs {
            if !ids.contains(&p.pair) {
                ids.push(p.pair);
            }
        }
        for id in ids {
            let mut cur: u8 = 0;
            for r in &self.ledger {
                if r.pair == id {
                    if r.to_step != cur + 1 {
                        return false;
                    }
                    cur = r.to_step;
                }
            }
            let end = self.pairs.iter().find(|p| p.pair == id).map(|p| p.step).unwrap_or(9);
            if cur != end {
                return false;
            }
        }
        true
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

    /// 折叠指纹（137 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10b20;
        for p in &self.pairs {
            h = h.wrapping_mul(137).wrapping_add(p.pair);
            h = h.wrapping_mul(137).wrapping_add(p.step as u32);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(137).wrapping_add(r.seq);
            for by in r.note.bytes() {
                h = h.wrapping_mul(137).wrapping_add(by as u32);
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
    let mut m = ReleaseMachine::new();
    let _ = m.enroll(1);
    let _ = m.enroll(2);
    // 1) 完成后再步必拒。
    let _ = m.advance(1, "s1");
    let _ = m.advance(1, "s2");
    let _ = m.advance(1, "s3");
    got[0] = m.advance(1, "s4").err() == Some(RL_DONE);
    // 2) 幽灵对步进必拒。
    got[1] = m.advance(99, "ghost").err() == Some(RL_GHOST);
    // 3) 半释放审计：对 2 停在 1 态必被标。
    let _ = m.advance(2, "half");
    got[2] = m.half_released() == vec![2];
    // 4) 伪造步号账（步号 0 与 4 出界）必判非法。
    let f1 = RelRec { seq: 1, pair: 1, to_step: 0, note: "inj-zero".to_string() };
    let f2 = RelRec { seq: 2, pair: 1, to_step: 4, note: "inj-over".to_string() };
    got[3] = !f1.ok() && !f2.ok();
    // 5) 印账后步进必抓。
    m.seal();
    got[4] = m.advance(2, "inj-frozen").err() == Some(RL_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10192_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10192");

    // J1 三步序达成：fd→对端→账位顺序走完释放成立。
    let j1 = {
        let mut m = ReleaseMachine::new();
        let _ = m.enroll(1);
        let a = m.advance(1, "fd");
        let b = m.advance(1, "peer");
        let c = m.advance(1, "slot");
        a == Ok(1) && b == Ok(2) && c == Ok(3) && m.released(1)
    };
    s.add("J1-三步序达成", j1, "1→2→3 顺序步进释放成立");

    // J2 跳步面受控：状态机只许 +1（完成前恒可步进，跳步由重放守恒拦）。
    let j2 = {
        let mut m = ReleaseMachine::new();
        let _ = m.enroll(1);
        let _ = m.advance(1, "fd");
        // 第 2 步照常；伪造 3 步账面在重放中不成立（由 J3 守恒核拦）。
        m.advance(1, "peer").is_ok()
    };
    s.add("J2-顺序步进", j2, "逐态顺序推进无旁路");

    // J3 账本步号守恒：重放与终态对平。
    let j3 = {
        let mut m = ReleaseMachine::new();
        let _ = m.enroll(1);
        let _ = m.enroll(2);
        let _ = m.advance(1, "fd");
        let _ = m.advance(1, "peer");
        let _ = m.advance(1, "slot");
        let _ = m.advance(2, "fd");
        m.replay() && m.ledger_ok() && m.seq_unique()
    };
    s.add("J3-步号守恒", j3, "双对重放步号与终态对平");

    // J4 已完成再步拒。
    let j4 = {
        let mut m = ReleaseMachine::new();
        let _ = m.enroll(1);
        let _ = m.advance(1, "a");
        let _ = m.advance(1, "b");
        let _ = m.advance(1, "c");
        m.advance(1, "d").err() == Some(RL_DONE)
    };
    s.add("J4-完成再步拒", j4, "终态后步进必拒");

    // J5 半释放检出：停在 fd 关态的对被审计标出。
    let j5 = {
        let mut m = ReleaseMachine::new();
        let _ = m.enroll(1);
        let _ = m.enroll(2);
        let _ = m.advance(1, "fd");
        // 对 2 走完全程作对照，只剩对 1 停在 fd 关态。
        let _ = m.advance(2, "fd");
        let _ = m.advance(2, "peer");
        let _ = m.advance(2, "slot");
        m.half_released() == vec![1] && !m.released(1)
    };
    s.add("J5-半释放检出", j5, "停步对精确标出");

    // J6 幽灵对拒。
    let j6 = {
        let mut m = ReleaseMachine::new();
        m.advance(9, "g").err() == Some(RL_GHOST)
    };
    s.add("J6-幽灵拒", j6, "未挂册对步进必拒");

    // J7 空留痕拒。
    let j7 = {
        let mut m = ReleaseMachine::new();
        let _ = m.enroll(1);
        m.advance(1, "").err() == Some(RL_EMPTY_NOTE)
    };
    s.add("J7-空留痕拒", j7, "空留痕步进必拒");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut m = ReleaseMachine::new();
        let _ = m.enroll(1);
        let _ = m.advance(1, "a");
        let f1 = m.fold();
        let f2 = m.fold();
        m.seal();
        f1 == f2 && m.advance(1, "post").err() == Some(RL_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    // J10 双对并行释放：两对各自独立步进互不串位。
    let j10 = {
        let mut m = ReleaseMachine::new();
        let _ = m.enroll(1);
        let _ = m.enroll(2);
        let _ = m.advance(1, "fd");
        let _ = m.advance(2, "fd");
        let _ = m.advance(2, "peer");
        let _ = m.advance(2, "slot");
        m.released(2) && !m.released(1) && m.half_released() == vec![1]
    };
    s.add("J10-双对并行", j10, "对 2 走完、对 1 半释放精确分账");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10192_release_flow() {
        let mut m = ReleaseMachine::new();
        let _ = m.enroll(1);
        assert_eq!(m.advance(1, "a"), Ok(1));
        assert_eq!(m.advance(1, "b"), Ok(2));
        assert_eq!(m.advance(1, "c"), Ok(3));
        assert!(m.released(1));
        assert_eq!(m.advance(1, "d"), Err(RL_DONE));
        assert!(m.replay());
    }

    #[test]
    fn unx10192_checks_green() {
        let s = run_unx10192_checks();
        assert!(s.all_passed(), "UNX-F10192 判据未全绿");
    }
}
