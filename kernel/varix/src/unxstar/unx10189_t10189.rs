//! UNX-F10189 信号组投递正反双判据（组投递达成与非法组误用受控）（0x10ac0 段）
//!
//! 锚点：UNX-F10189-J1 组投递达成（killpg 全组成员受击 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：组击册（pid × pgid × 存活位）+ killpg 核（全组投递/野组
//! ESRCH/死组 ESRCH/野信号拒）+ 回执账本（逐成员受击留痕）+
//! 折叠指纹 + 注入核（伪造回执/幽灵组/野信号/空留痕/印后投递）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x10ac0 独占 0x20）。
pub const SEG_BASE: u32 = 0x10ac0;
pub const KG_ESRCH: u32 = SEG_BASE + 0x01; // 野组/死组：无进程可击
pub const KG_BAD_SIG: u32 = SEG_BASE + 0x02; // 野信号
pub const KG_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 留痕为空
pub const KG_FORGED: u32 = SEG_BASE + 0x04; // 伪造回执
pub const KG_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍投递

/// 信号合法域 [1,31]。
pub const KS_MIN: u32 = 1;
pub const KS_MAX: u32 = 31;

/// 进程条目：pid × pgid × 存活位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KProc {
    pub pid: u32,
    pub pgid: u32,
    pub alive: bool,
}

/// 一次组投递的账本留痕。
#[derive(Clone)]
pub struct KillRec {
    pub seq: u32,
    pub pgid: u32,
    pub sig: u32,
    /// 受击成员 pid 列表（投递面回执）。
    pub hits: Vec<u32>,
    pub note: String,
}

impl KillRec {
    /// 账目合法性：留痕非空且回执成员号非零无重号。
    pub fn ok(&self) -> bool {
        if self.note.is_empty() {
            return false;
        }
        for a in 0..self.hits.len() {
            if self.hits[a] == 0 {
                return false;
            }
            for b in (a + 1)..self.hits.len() {
                if self.hits[a] == self.hits[b] {
                    return false;
                }
            }
        }
        true
    }

    pub fn render_line(&self) -> String {
        format!("K{}|g{}|sig{}|n{}|{}", self.seq, self.pgid, self.sig, self.hits.len(), self.note)
    }
}

/// 组击册：进程表 + killpg 核。
pub struct KillTab {
    pub procs: Vec<KProc>,
    pub ledger: Vec<KillRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl KillTab {
    pub fn new() -> KillTab {
        KillTab { procs: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 登记进程（重号必拒）。
    pub fn admit(&mut self, pid: u32, pgid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(KG_FROZEN);
        }
        if pid == 0 || self.procs.iter().any(|p| p.pid == pid) {
            return Err(KG_ESRCH);
        }
        self.procs.push(KProc { pid, pgid, alive: true });
        Ok(())
    }

    /// killpg 核：全组存活成员逐一受击；野组/死组/野信号必拒。
    pub fn killpg(&mut self, pgid: u32, sig: u32, note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(KG_FROZEN);
        }
        if note.is_empty() {
            return Err(KG_EMPTY_NOTE);
        }
        if sig < KS_MIN || sig > KS_MAX {
            return Err(KG_BAD_SIG);
        }
        if !self.procs.iter().any(|p| p.pgid == pgid && p.alive) {
            return Err(KG_ESRCH);
        }
        let mut hits: Vec<u32> = Vec::new();
        for p in &mut self.procs {
            if p.pgid == pgid && p.alive {
                p.alive = false; // 受击即终局（简化：信号致命）
                hits.push(p.pid);
            }
        }
        self.ledger.push(KillRec {
            seq: self.seq_next,
            pgid,
            sig,
            hits,
            note: note.to_string(),
        });
        self.seq_next += 1;
        let n = self.ledger.last().map(|r| r.hits.len()).unwrap_or(0);
        Ok(n)
    }

    /// 组存活清点。
    pub fn alive_in(&self, pgid: u32) -> usize {
        self.procs.iter().filter(|p| p.pgid == pgid && p.alive).count()
    }

    /// 回执核对：某 pid 是否在某次投递回执中（防伪造回执）。
    pub fn hit_listed(&self, seq: u32, pid: u32) -> bool {
        match self.ledger.iter().find(|r| r.seq == seq) {
            Some(r) => r.hits.contains(&pid),
            None => false,
        }
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

    /// 折叠指纹（113 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10ac0;
        for p in &self.procs {
            h = h.wrapping_mul(113).wrapping_add(p.pid);
            h = h.wrapping_mul(113).wrapping_add(p.alive as u32);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(113).wrapping_add(r.seq);
            h = h.wrapping_mul(113).wrapping_add(r.hits.len() as u32);
            for by in r.note.bytes() {
                h = h.wrapping_mul(113).wrapping_add(by as u32);
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
    let mut t = KillTab::new();
    let _ = t.admit(1, 10);
    let _ = t.admit(2, 10);
    // 1) 野组投递必拒（组 99 无人）。
    got[0] = t.killpg(99, 15, "inj-wild").err() == Some(KG_ESRCH);
    // 2) 野信号必拒。
    got[1] = t.killpg(10, 0, "inj-sig0").err() == Some(KG_BAD_SIG)
        && t.killpg(10, 64, "inj-sig64").err() == Some(KG_BAD_SIG);
    // 3) 死组投递必拒（全组已受击后再投）。
    let _ = t.killpg(10, 15, "real");
    got[2] = t.killpg(10, 15, "dead").err() == Some(KG_ESRCH);
    // 4) 伪造回执：非成员 pid 出现在回执必检出。
    let forged = KillRec {
        seq: 1,
        pgid: 10,
        sig: 15,
        hits: vec![1, 77], // 77 不在组 10
        note: "inj-hit".to_string(),
    };
    got[3] = forged.hits.contains(&77) && !t.hit_listed(forged.seq, 77);
    // 5) 印账后投递必抓。
    t.seal();
    got[4] = t.killpg(10, 15, "inj-frozen").err() == Some(KG_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10189_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10189");

    // J1 组投递全中：三成员全数受击。
    let j1 = {
        let mut t = KillTab::new();
        let _ = t.admit(1, 10);
        let _ = t.admit(2, 10);
        let _ = t.admit(3, 10);
        let n = t.killpg(10, 15, "k").unwrap_or(0);
        n == 3 && t.alive_in(10) == 0
    };
    s.add("J1-全组受击", j1, "三成员全中且组内存活归零");

    // J2 非成员不中：邻组不受波及。
    let j2 = {
        let mut t = KillTab::new();
        let _ = t.admit(1, 10);
        let _ = t.admit(2, 20);
        let _ = t.killpg(10, 9, "k");
        t.alive_in(20) == 1
    };
    s.add("J2-邻组无伤", j2, "组 20 成员不受组 10 波及");

    // J3 野组拒。
    let j3 = {
        let mut t = KillTab::new();
        t.killpg(42, 9, "w").err() == Some(KG_ESRCH)
    };
    s.add("J3-野组拒", j3, "不在册组投递必拒");

    // J4 野信号拒：0 与越上界双拒。
    let j4 = {
        let mut t = KillTab::new();
        let _ = t.admit(1, 10);
        t.killpg(10, 0, "s0").err() == Some(KG_BAD_SIG)
            && t.killpg(10, 32, "s32").err() == Some(KG_BAD_SIG)
    };
    s.add("J4-野信号拒", j4, "信号域 [1,31] 外必拒");

    // J5 死组拒：受击后再投 ESRCH。
    let j5 = {
        let mut t = KillTab::new();
        let _ = t.admit(1, 10);
        let _ = t.killpg(10, 9, "k");
        t.killpg(10, 9, "again").err() == Some(KG_ESRCH)
    };
    s.add("J5-死组拒", j5, "全灭后组按 ESRCH 拒");

    // J6 回执精确：受击名单逐一对平。
    let j6 = {
        let mut t = KillTab::new();
        let _ = t.admit(1, 10);
        let _ = t.admit(2, 10);
        let _ = t.killpg(10, 9, "k");
        let r = t.ledger.last().cloned().unwrap_or(KillRec {
            seq: 0,
            pgid: 0,
            sig: 0,
            hits: Vec::new(),
            note: String::new(),
        });
        r.hits == vec![1, 2] && t.hit_listed(r.seq, 1) && t.hit_listed(r.seq, 2)
    };
    s.add("J6-回执精确", j6, "回执名单与投递面逐一对平");

    // J7 账本完整：回执无重号合法且 seq 唯一。
    let j7 = {
        let mut t = KillTab::new();
        let _ = t.admit(1, 10);
        let _ = t.admit(2, 20);
        let _ = t.killpg(10, 9, "a");
        let _ = t.killpg(20, 9, "b");
        t.ledger.len() == 2 && t.ledger_ok() && t.seq_unique()
    };
    s.add("J7-账本完整", j7, "双组账合法无重号");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut t = KillTab::new();
        let _ = t.admit(1, 10);
        let _ = t.killpg(10, 9, "k");
        let f1 = t.fold();
        let f2 = t.fold();
        t.seal();
        f1 == f2 && t.killpg(10, 9, "post").err() == Some(KG_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    // J10 跨组连击：两组先后投递互不干扰。
    let j10 = {
        let mut t = KillTab::new();
        let _ = t.admit(1, 10);
        let _ = t.admit(2, 20);
        let a = t.killpg(10, 9, "g1");
        let b = t.killpg(20, 9, "g2");
        a == Ok(1) && b == Ok(1) && t.alive_in(10) == 0 && t.alive_in(20) == 0
    };
    s.add("J10-跨组连击", j10, "两组先后各中一员");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10189_killpg_flow() {
        let mut t = KillTab::new();
        let _ = t.admit(1, 10);
        let _ = t.admit(2, 10);
        let _ = t.admit(3, 20);
        assert_eq!(t.killpg(10, 15, "k"), Ok(2));
        assert_eq!(t.alive_in(10), 0);
        assert_eq!(t.alive_in(20), 1);
        assert_eq!(t.killpg(99, 15, "w"), Err(KG_ESRCH));
        assert_eq!(t.killpg(20, 0, "s"), Err(KG_BAD_SIG));
    }

    #[test]
    fn unx10189_checks_green() {
        let s = run_unx10189_checks();
        assert!(s.all_passed(), "UNX-F10189 判据未全绿");
    }
}
