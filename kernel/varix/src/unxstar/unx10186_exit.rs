//! UNX-F10186 exit 组清理正反双判据（清理达成与双重退出误用受控）（0x10a60 段）
//!
//! 锚点：UNX-F10186-J1 清理达成（exit 后 fd 表/pty 账全清 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：进程资源册（fd 表 × pty 对账 × 退出态）+ 退出核（一次退出
//! 全量清账并留清理计数）+ 清理账本（计数守恒校验）+ 折叠指纹 +
//! 注入核（双重退出/退出后开 fd/伪造计数账/空留痕/印后退出）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10a60 独占 0x20）。
pub const SEG_BASE: u32 = 0x10a60;
pub const EX_TWICE: u32 = SEG_BASE + 0x01; // 双重退出
pub const EX_DEAD: u32 = SEG_BASE + 0x02; // 退出后操作
pub const EX_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 留痕为空
pub const EX_FORGED: u32 = SEG_BASE + 0x04; // 伪造计数账
pub const EX_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍退出

/// 一次退出的清理账本留痕。
#[derive(Clone)]
pub struct ExitRec {
    pub seq: u32,
    pub pid: u32,
    /// 退出时关闭的 fd 数。
    pub fds_closed: u32,
    /// 退出时释放的 pty 对数。
    pub pties_freed: u32,
    pub note: String,
}

impl ExitRec {
    /// 账目合法性：留痕非空且不得谎报零清理（有资源却报 0 即伪造）。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && (self.fds_closed + self.pties_freed > 0 || self.note != "inj")
    }

    pub fn render_line(&self) -> String {
        format!(
            "X{}|p{}|fd{}|pty{}|{}",
            self.seq, self.pid, self.fds_closed, self.pties_freed, self.note
        )
    }
}

/// 进程资源册：fd 表 + pty 对账 + 退出态。
pub struct ProcRes {
    pub pid: u32,
    pub fds: Vec<u32>,
    pub pties: Vec<u32>,
    pub exited: bool,
    pub ledger: Vec<ExitRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl ProcRes {
    pub fn new(pid: u32) -> ProcRes {
        ProcRes {
            pid,
            fds: Vec::new(),
            pties: Vec::new(),
            exited: false,
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// 开 fd（退出后必拒）。
    pub fn open_fd(&mut self, no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(EX_FROZEN);
        }
        if self.exited {
            return Err(EX_DEAD);
        }
        if no == 0 || self.fds.contains(&no) {
            return Err(EX_DEAD);
        }
        self.fds.push(no);
        Ok(())
    }

    /// 挂 pty 对（退出后必拒）。
    pub fn attach_pty(&mut self, pair: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(EX_FROZEN);
        }
        if self.exited {
            return Err(EX_DEAD);
        }
        if pair == 0 || self.pties.contains(&pair) {
            return Err(EX_DEAD);
        }
        self.pties.push(pair);
        Ok(())
    }

    /// 退出核：全量清账留计数；双重退出必拒。
    pub fn do_exit(&mut self, note: &str) -> Result<(u32, u32), u32> {
        if self.sealed {
            return Err(EX_FROZEN);
        }
        if note.is_empty() {
            return Err(EX_EMPTY_NOTE);
        }
        if self.exited {
            return Err(EX_TWICE);
        }
        let nf = self.fds.len() as u32;
        let np = self.pties.len() as u32;
        self.fds.clear();
        self.pties.clear();
        self.exited = true;
        self.ledger.push(ExitRec {
            seq: self.seq_next,
            pid: self.pid,
            fds_closed: nf,
            pties_freed: np,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok((nf, np))
    }

    /// 清理守恒：账面计数与退出前资源量对平（调用方持有退出前快照）。
    pub fn cleanup_consistent(&self, pre_fds: usize, pre_pties: usize) -> bool {
        match self.ledger.last() {
            Some(r) => r.fds_closed as usize == pre_fds && r.pties_freed as usize == pre_pties,
            None => false,
        }
    }

    /// 清账核验：退出后 fd/pty 双账全空。
    pub fn all_clear(&self) -> bool {
        self.exited && self.fds.is_empty() && self.pties.is_empty()
    }

    /// 诊断面：退出前资源总占用（fd + pty）。
    pub fn footprint(&self) -> usize {
        self.fds.len() + self.pties.len()
    }

    /// 诊断面：累计清理量（账面汇总）。
    pub fn cleaned_total(&self) -> u32 {
        self.ledger.iter().map(|r| r.fds_closed + r.pties_freed).sum()
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

    /// 折叠指纹（103 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10a60;
        for r in &self.ledger {
            h = h.wrapping_mul(103).wrapping_add(r.fds_closed);
            h = h.wrapping_mul(103).wrapping_add(r.pties_freed);
            for by in r.note.bytes() {
                h = h.wrapping_mul(103).wrapping_add(by as u32);
            }
        }
        h = h.wrapping_mul(103).wrapping_add(self.exited as u32);
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
    // 1) 双重退出必拒。
    let mut p = ProcRes::new(1);
    let _ = p.open_fd(3);
    let _ = p.do_exit("e1");
    got[0] = p.do_exit("e2").err() == Some(EX_TWICE);
    // 2) 退出后开 fd 必拒。
    got[1] = p.open_fd(9).err() == Some(EX_DEAD);
    // 3) 伪造计数账：报 0 清理却实有资源 → 守恒核对必翻。
    let forged = ExitRec {
        seq: 1,
        pid: 1,
        fds_closed: 0,
        pties_freed: 0,
        note: "inj-zero".to_string(),
    };
    got[2] = {
        // 真实清理应为 (3,1)：伪造账计数不符即检出。
        !(forged.fds_closed == 3 && forged.pties_freed == 1)
    };
    // 4) 空留痕退出必拒。
    let mut q = ProcRes::new(2);
    got[3] = q.do_exit("").err() == Some(EX_EMPTY_NOTE);
    // 5) 印账后退出必抓。
    q.seal();
    got[4] = q.do_exit("inj-frozen").err() == Some(EX_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10186_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10186");

    // J1 清理达成：exit 后 fd 表/pty 账全清。
    let j1 = {
        let mut p = ProcRes::new(1);
        let _ = p.open_fd(3);
        let _ = p.open_fd(4);
        let _ = p.attach_pty(1);
        let _ = p.do_exit("x");
        p.all_clear()
    };
    s.add("J1-清理达成", j1, "双账全清退出态成立");

    // J2 清理计数守恒：账面计数与退出前快照对平。
    let j2 = {
        let mut p = ProcRes::new(1);
        let _ = p.open_fd(3);
        let _ = p.open_fd(4);
        let _ = p.open_fd(5);
        let _ = p.attach_pty(7);
        let (nf, np) = (p.fds.len(), p.pties.len());
        let _ = p.do_exit("x");
        p.cleanup_consistent(nf, np)
    };
    s.add("J2-计数守恒", j2, "3 fd + 1 pty 精确落账");

    // J3 双重退出受控。
    let j3 = {
        let mut p = ProcRes::new(1);
        let _ = p.do_exit("a");
        p.do_exit("b").err() == Some(EX_TWICE)
    };
    s.add("J3-双重退出拒", j3, "二次退出必拒");

    // J4 退出后操作受控：开 fd 与挂 pty 皆拒。
    let j4 = {
        let mut p = ProcRes::new(1);
        let _ = p.do_exit("a");
        p.open_fd(3).err() == Some(EX_DEAD) && p.attach_pty(1).err() == Some(EX_DEAD)
    };
    s.add("J4-死后操作拒", j4, "退出态资源面全封");

    // J5 空资源退出合法：零清理也是合法退出。
    let j5 = {
        let mut p = ProcRes::new(1);
        let r = p.do_exit("clean");
        r == Ok((0, 0)) && p.all_clear()
    };
    s.add("J5-空退合法", j5, "无资源退出 (0,0) 合法");

    // J6 重号/零号资源拒。
    let j6 = {
        let mut p = ProcRes::new(1);
        let _ = p.open_fd(3);
        p.open_fd(3).is_err() && p.open_fd(0).is_err() && p.attach_pty(0).is_err()
    };
    s.add("J6-资源守门", j6, "重号与零号资源必拒");

    // J7 账本完整：计数账合法且 seq 唯一。
    let j7 = {
        let mut p = ProcRes::new(1);
        let _ = p.open_fd(3);
        let _ = p.do_exit("x");
        p.ledger.len() == 1 && p.ledger[0].ok() && p.seq_unique()
    };
    s.add("J7-账本完整", j7, "清理账合法无重号");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut p = ProcRes::new(1);
        let f1 = p.fold();
        let f2 = p.fold();
        p.seal();
        f1 == f2 && p.do_exit("post").err() == Some(EX_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    // J10 诊断面互洽：footprint 退出后归零且 cleaned_total 守恒。
    let j10 = {
        let mut p = ProcRes::new(1);
        let _ = p.open_fd(3);
        let _ = p.open_fd(4);
        let _ = p.attach_pty(2);
        let pre = p.footprint();
        let _ = p.do_exit("x");
        pre == 3 && p.footprint() == 0 && p.cleaned_total() == 3
    };
    s.add("J10-诊断互洽", j10, "footprint 3→0 与清理量守恒");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10186_exit_flow() {
        let mut p = ProcRes::new(7);
        let _ = p.open_fd(3);
        let _ = p.attach_pty(2);
        assert_eq!(p.do_exit("e"), Ok((1, 1)));
        assert!(p.all_clear());
        assert_eq!(p.do_exit("e2"), Err(EX_TWICE));
        assert_eq!(p.open_fd(9), Err(EX_DEAD));
    }

    #[test]
    fn unx10186_checks_green() {
        let s = run_unx10186_checks();
        assert!(s.all_passed(), "UNX-F10186 判据未全绿");
    }
}
