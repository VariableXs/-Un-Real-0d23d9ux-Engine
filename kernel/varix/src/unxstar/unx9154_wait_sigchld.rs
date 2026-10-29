//! UNX-F9154 wait 族与 SIGCHLD 联动（0x63300 段）
//!
//! 锚点：UNX-F9154-J1 联动账（每次子态变化落账：子 pid、变化类、
//! 父 SIGCHLD 置位/清位、留痕）；J2 联动语义（子亡置位 + wait 清位
//! + SIG_IGN 自动收割免僵尸 + 停/续同样置位四性质）；J3 联动指纹
//! （账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B18（wait×SIGCHLD 联动面）。
//!
//! 结构：父子对册 + pending 位簿 + 联动核（置位/清位/忽略处置）+
//! 联动账本 + 折叠指纹 + 注入核（错码/空留痕/伪置位/未知对/印后登
//! 记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63300 独占 0x20）。
pub const SEG_BASE: u32 = 0x63300;
pub const CH_UNK_PAIR: u32 = SEG_BASE + 0x01; // 未知父子对
pub const CH_BAD_EVT: u32 = SEG_BASE + 0x02; // 非法事件
pub const CH_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const CH_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const CH_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 子态变化事件闭集。
pub const EV_EXIT: usize = 0; // 子亡
pub const EV_STOP: usize = 1; // 子停（WUNTRACED 可见）
pub const EV_CONT: usize = 2; // 子续（WCONTINUED 可见）
pub const EV_COUNT: usize = 3;

pub const EV_NAMES: [&str; EV_COUNT] = ["exit", "stop", "cont"];

/// 父对 SIGCHLD 的处置。
pub const DISP_DFL: u8 = 0; // 默认：置位 pending
pub const DISP_IGN: u8 = 1; // 忽略：自动收割免僵尸
pub const DISP_COUNT: u8 = 2;

/// 单条联动账。
#[derive(Clone)]
pub struct ChldRec {
    pub seq: u32,
    pub child: u32,
    pub parent: u32,
    pub event: usize,
    pub disposition: u8,
    pub raised: bool, // SIGCHLD 是否投递
    pub note: String,
}

impl ChldRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.event < EV_COUNT && self.disposition < DISP_COUNT
    }

    pub fn render_line(&self) -> String {
        format!(
            "CH{}|c{}p{}|{}|d{}|r{}|{}",
            self.seq,
            self.child,
            self.parent,
            EV_NAMES[self.event],
            self.disposition,
            self.raised as u8,
            self.note
        )
    }
}

/// 联动簿。
pub struct ChldBook {
    /// 父子对（child→parent）
    pub pairs: Vec<(u32, u32)>,
    /// 父 SIGCHLD pending 位（有变化未收割即 true）
    pub pending: Vec<(u32, bool)>,
    /// 僵尸未收割名单
    pub zombies: Vec<u32>,
    pub ledger: Vec<ChldRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl ChldBook {
    pub fn new() -> ChldBook {
        ChldBook {
            pairs: Vec::new(),
            pending: Vec::new(),
            zombies: Vec::new(),
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    pub fn bind(&mut self, child: u32, parent: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(CH_FROZEN);
        }
        if self.pairs.iter().any(|p| p.0 == child) {
            return Err(CH_UNK_PAIR);
        }
        self.pairs.push((child, parent));
        Ok(())
    }

    fn parent_of(&self, child: u32) -> Option<u32> {
        self.pairs.iter().find(|p| p.0 == child).map(|p| p.1)
    }

    fn set_pending(&mut self, parent: u32, v: bool) {
        match self.pending.iter_mut().find(|p| p.0 == parent) {
            Some(e) => e.1 = v,
            None => self.pending.push((parent, v)),
        }
    }

    pub fn pending_of(&self, parent: u32) -> bool {
        self.pending.iter().find(|p| p.0 == parent).map(|p| p.1).unwrap_or(false)
    }

    /// 联动核：子态变化 → SIGCHLD。
    /// DISP_DFL：置位 + （exit 时）僵尸登记；DISP_IGN：自动收割免僵尸。
    pub fn notify(
        &mut self,
        child: u32,
        event: usize,
        disposition: u8,
        note: &str,
    ) -> Result<bool, u32> {
        if self.sealed {
            return Err(CH_FROZEN);
        }
        if note.is_empty() {
            return Err(CH_EMPTY_NOTE);
        }
        if event >= EV_COUNT {
            return Err(CH_BAD_EVT);
        }
        let parent = match self.parent_of(child) {
            Some(p) => p,
            None => return Err(CH_UNK_PAIR),
        };
        let raised = disposition == DISP_DFL;
        if raised {
            self.set_pending(parent, true);
            if event == EV_EXIT {
                self.zombies.push(child);
            }
        }
        // IGN 处置：自动收割，无僵尸无 pending
        self.ledger.push(ChldRec {
            seq: self.seq_next,
            child,
            parent,
            event,
            disposition,
            raised,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(raised)
    }

    /// wait 收割：清位 + 摘僵尸。
    pub fn reap(&mut self, parent: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(CH_FROZEN);
        }
        if note.is_empty() {
            return Err(CH_EMPTY_NOTE);
        }
        let got = self.zombies.iter().position(|z| {
            self.parent_of(*z) == Some(parent)
        });
        match got {
            Some(i) => {
                let z = self.zombies[i];
                self.zombies.remove(i);
                self.set_pending(parent, false);
                self.ledger.push(ChldRec {
                    seq: self.seq_next,
                    child: z,
                    parent,
                    event: EV_EXIT,
                    disposition: DISP_DFL,
                    raised: false,
                    note: note.to_string(),
                });
                self.seq_next += 1;
                Ok(z)
            }
            None => Err(CH_UNK_PAIR),
        }
    }

    pub fn zombie_count(&self, parent: u32) -> usize {
        self.zombies.iter().filter(|z| self.parent_of(**z) == Some(parent)).count()
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i].seq == self.ledger[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x91540;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.child);
            h = h.wrapping_mul(31).wrapping_add(r.parent);
            h = h.wrapping_mul(31).wrapping_add(r.event as u32);
            h = h.wrapping_mul(31).wrapping_add(r.disposition as u32);
            h = h.wrapping_mul(31).wrapping_add(r.raised as u32);
            for b in r.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 基础样本：父 10 ← 子 20/21；父 11 ← 子 22。
pub fn mk_book() -> ChldBook {
    let mut b = ChldBook::new();
    let _ = b.bind(20, 10);
    let _ = b.bind(21, 10);
    let _ = b.bind(22, 11);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 非法事件
    let bad = ChldRec {
        seq: 1,
        child: 1,
        parent: 2,
        event: 9,
        disposition: 0,
        raised: false,
        note: "inj-evt".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = ChldRec {
        seq: 2,
        child: 1,
        parent: 2,
        event: 0,
        disposition: 0,
        raised: false,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 伪置位：未知子 notify
    let mut b = mk_book();
    got[2] = b.notify(99, EV_EXIT, DISP_DFL, "inj-unk").err() == Some(CH_UNK_PAIR);
    // 4) 未知收割
    got[3] = b.reap(42, "inj-reap").err() == Some(CH_UNK_PAIR);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.notify(20, EV_EXIT, DISP_DFL, "inj-frozen").err() == Some(CH_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9154_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9154");
    let mut b = mk_book();

    // J1 联动账非空
    let _ = b.notify(20, EV_EXIT, DISP_DFL, "20 亡");
    let j1 = !b.ledger.is_empty();
    s.add("J1-联动账非空", j1, "子态变化落账至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且事件/处置闭集");

    // J3 子亡置位
    let j3 = b.pending_of(10);
    s.add("J3-子亡置位", j3, "DFL 处置下子亡置父 SIGCHLD");

    // J4 僵尸登记
    let j4 = b.zombie_count(10) == 1;
    s.add("J4-僵尸登记", j4, "exit 事件登记僵尸");

    // J5 wait 清位 + 摘僵尸
    let j5 = {
        let got = b.reap(10, "收割 20");
        got == Ok(20) && !b.pending_of(10) && b.zombie_count(10) == 0
    };
    s.add("J5-wait清位", j5, "收割摘僵尸并清 SIGCHLD 位");

    // J6 IGN 自动收割免僵尸
    let j6 = {
        let mut b2 = mk_book();
        let raised = b2.notify(21, EV_EXIT, DISP_IGN, "IGN 处置").unwrap_or(true);
        !raised && !b2.pending_of(10) && b2.zombie_count(10) == 0
    };
    s.add("J6-IGN免僵尸", j6, "SIG_IGN 处置自动收割免僵尸");

    // J7 停/续也置位
    let j7 = {
        let mut b2 = mk_book();
        let _ = b2.notify(21, EV_STOP, DISP_DFL, "停");
        let s1 = b2.pending_of(10);
        let _ = b2.notify(21, EV_CONT, DISP_DFL, "续");
        s1 && b2.pending_of(10)
    };
    s.add("J7-停续置位", j7, "stop/cont 事件同置 SIGCHLD");

    // J8 非法事件
    let j8 = b.notify(20, 9, DISP_DFL, "野事件").err() == Some(CH_BAD_EVT);
    s.add("J8-非法事件", j8, "事件出闭集必拒");

    // J9 未知父子对
    let j9 = b.notify(99, EV_EXIT, DISP_DFL, "未知对").err() == Some(CH_UNK_PAIR);
    s.add("J9-未知父子对", j9, "未 bind 的子必拒");

    // J10 无僵尸收割拒（ECHILD 语义等价）
    let j10 = b.reap(11, "无僵尸").err() == Some(CH_UNK_PAIR);
    s.add("J10-无僵尸拒", j10, "无僵尸收割必拒");

    // J11 两父隔离
    let j11 = {
        let mut b2 = mk_book();
        let _ = b2.notify(20, EV_EXIT, DISP_DFL, "父10的子亡");
        b2.pending_of(10) && !b2.pending_of(11)
    };
    s.add("J11-两父隔离", j11, "SIGCHLD 位按父隔离");

    // J12 指纹稳定 + seq 唯一
    let j12 = b.fold() == b.fold() && b.seq_unique();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致且 seq 唯一");

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
    fn unx9154_link_flow() {
        let mut b = mk_book();
        assert_eq!(b.notify(20, EV_EXIT, DISP_DFL, "a"), Ok(true));
        assert!(b.pending_of(10));
        assert_eq!(b.reap(10, "b"), Ok(20));
        assert!(!b.pending_of(10));
        // IGN
        assert_eq!(b.notify(21, EV_EXIT, DISP_IGN, "c"), Ok(false));
        assert_eq!(b.zombie_count(10), 0);
        // 非法
        assert_eq!(b.notify(20, 9, DISP_DFL, "d"), Err(CH_BAD_EVT));
        assert_eq!(b.notify(99, EV_EXIT, DISP_DFL, "e"), Err(CH_UNK_PAIR));
    }

    #[test]
    fn unx9154_checks_green() {
        let s = run_unx9154_checks();
        assert!(s.all_passed(), "UNX-F9154 判据未全绿");
    }
}
