//! UNX-F10181 openpty 分配正反双判据（主从对达成与耗尽误用受控）（0x109c0 段）
//!
//! 锚点：UNX-F10181-J1 分配达成（openpty 主从对创建且双向可写读 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：pty 池（主从对 × 双向缓冲 m2s/s2m × fd 编号独占分配）+
//! 分配核（openpty 池耗尽受控）+ 收发核（四向读写）+ 回环账本 +
//! 折叠指纹 + 注入核（池耗尽/幽灵 fd/断端写/空留痕/印后分配）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x109c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x109c0;
pub const PT_EXHAUST: u32 = SEG_BASE + 0x01; // 池耗尽
pub const PT_NO_FD: u32 = SEG_BASE + 0x02; // 幽灵 fd
pub const PT_CLOSED: u32 = SEG_BASE + 0x03; // 断端操作
pub const PT_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 留痕为空
pub const PT_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍分配

/// 池容量（对数）。
pub const PTY_CAP: usize = 4;

/// 主从对：双向缓冲各持一端。
#[derive(Clone)]
pub struct PtyPair {
    pub id: u32,
    pub master_fd: i32,
    pub slave_fd: i32,
    pub open: bool,
    /// master→slave 方向缓冲。
    pub m2s: Vec<u8>,
    /// slave→master 方向缓冲。
    pub s2m: Vec<u8>,
}

/// 一次分配的账本留痕。
#[derive(Clone)]
pub struct OpenRec {
    pub seq: u32,
    pub pair_id: u32,
    pub master_fd: i32,
    pub slave_fd: i32,
    pub note: String,
}

impl OpenRec {
    /// 账目合法性：留痕非空、两端 fd 必须互异。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.master_fd != self.slave_fd
    }

    pub fn render_line(&self) -> String {
        format!(
            "O{}|pair{}|m{}|s{}|{}",
            self.seq, self.pair_id, self.master_fd, self.slave_fd, self.note
        )
    }
}

/// pty 池：对表 + 分配核 + 收发核。
pub struct PtyPool {
    pub pairs: Vec<PtyPair>,
    pub fd_next: i32,
    pub ledger: Vec<OpenRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl PtyPool {
    pub fn new() -> PtyPool {
        PtyPool { pairs: Vec::new(), fd_next: 3, ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// openpty 分配核：新对双 fd，池满即耗尽拒收。
    pub fn openpty(&mut self, note: &str) -> Result<(i32, i32), u32> {
        if self.sealed {
            return Err(PT_FROZEN);
        }
        if note.is_empty() {
            return Err(PT_EMPTY_NOTE);
        }
        if self.pairs.len() >= PTY_CAP {
            return Err(PT_EXHAUST);
        }
        let id = self.pairs.len() as u32 + 1;
        let m = self.fd_next;
        let s = self.fd_next + 1;
        self.fd_next += 2;
        self.pairs.push(PtyPair {
            id,
            master_fd: m,
            slave_fd: s,
            open: true,
            m2s: Vec::new(),
            s2m: Vec::new(),
        });
        self.ledger.push(OpenRec {
            seq: self.seq_next,
            pair_id: id,
            master_fd: m,
            slave_fd: s,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok((m, s))
    }

    fn pair_by_any_fd(&self, fd: i32) -> Option<usize> {
        self.pairs
            .iter()
            .position(|p| (p.master_fd == fd || p.slave_fd == fd) && p.open)
    }

    /// 写端：向对端方向缓冲推字节（断端/幽灵 fd 必拒）。
    pub fn write(&mut self, fd: i32, b: u8) -> Result<(), u32> {
        let i = match self.pair_by_any_fd(fd) {
            Some(i) => i,
            None => return Err(PT_NO_FD),
        };
        if fd == self.pairs[i].master_fd {
            self.pairs[i].m2s.push(b);
        } else {
            self.pairs[i].s2m.push(b);
        }
        Ok(())
    }

    /// 读端：从本端方向缓冲弹出首字节（空即 EAGAIN 面，受控回 Err）。
    pub fn read(&mut self, fd: i32) -> Result<u8, u32> {
        let i = match self.pair_by_any_fd(fd) {
            Some(i) => i,
            None => return Err(PT_NO_FD),
        };
        if fd == self.pairs[i].master_fd {
            if self.pairs[i].s2m.is_empty() {
                return Err(PT_CLOSED);
            }
            Ok(self.pairs[i].s2m.remove(0))
        } else {
            if self.pairs[i].m2s.is_empty() {
                return Err(PT_CLOSED);
            }
            Ok(self.pairs[i].m2s.remove(0))
        }
    }

    /// 关闭一端（fd 失效：后续操作按幽灵拒）。
    pub fn close_fd(&mut self, fd: i32) -> Result<(), u32> {
        match self.pair_by_any_fd(fd) {
            Some(i) => {
                self.pairs[i].open = false;
                Ok(())
            }
            None => Err(PT_NO_FD),
        }
    }

    /// 主写从读回环：写 n 字节到从端并全量读回比对。
    pub fn roundtrip(&mut self, master: i32, slave: i32, n: u8) -> Result<bool, u32> {
        let mut k: u8 = 0;
        while k < n {
            self.write(master, k)?;
            k += 1;
        }
        let mut ok = true;
        let mut j: u8 = 0;
        while j < n {
            if self.read(slave)? != j {
                ok = false;
            }
            j += 1;
        }
        Ok(ok)
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

    /// 折叠指纹（79 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x109c0;
        for p in &self.pairs {
            h = h.wrapping_mul(79).wrapping_add(p.id);
            h = h.wrapping_mul(79).wrapping_add(p.master_fd as u32);
            h = h.wrapping_mul(79).wrapping_add(p.m2s.len() as u32);
            h = h.wrapping_mul(79).wrapping_add(p.s2m.len() as u32);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(79).wrapping_add(r.seq);
            for by in r.note.bytes() {
                h = h.wrapping_mul(79).wrapping_add(by as u32);
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
    let mut p = PtyPool::new();
    // 1) 池耗尽：PTY_CAP 对后继续分配必拒。
    let mut i = 0;
    while i < PTY_CAP {
        let _ = p.openpty("fill");
        i += 1;
    }
    got[0] = p.openpty("inj-exhaust").err() == Some(PT_EXHAUST);
    // 2) 幽灵 fd 读写必拒。
    got[1] = p.write(999, 1).err() == Some(PT_NO_FD);
    // 3) 断端写必拒（close 后原 fd 失效）。
    let (m, _s) = p.openpty("late").unwrap_or((0, 0));
    let _ = p.close_fd(m);
    got[2] = p.write(m, 1).err() == Some(PT_NO_FD);
    // 4) 伪造账：两端 fd 同号的分配账必判非法。
    let forged = OpenRec {
        seq: 1,
        pair_id: 1,
        master_fd: 5,
        slave_fd: 5,
        note: "inj-same-fd".to_string(),
    };
    got[3] = !forged.ok();
    // 5) 印账后分配必抓。
    p.seal();
    got[4] = p.openpty("inj-frozen").err() == Some(PT_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10181_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10181");

    // J1 分配达成：主从对创建、fd 互异、双端 open。
    let j1 = {
        let mut p = PtyPool::new();
        let r = p.openpty("a");
        match r {
            Ok((m, s2)) => {
                m != s2
                    && p.pairs.len() == 1
                    && p.pairs[0].open
                    && p.ledger[0].ok()
            }
            Err(_) => false,
        }
    };
    s.add("J1-分配达成", j1, "主从对 fd 互异且双端就绪");

    // J2 双向可写读：m→s 与 s→m 两方向回环字节一致。
    let j2 = {
        let mut p = PtyPool::new();
        let (m, s2) = p.openpty("b").unwrap_or((0, 0));
        let up = p.roundtrip(m, s2, 16);
        let down = p.roundtrip(s2, m, 16);
        up == Ok(true) && down == Ok(true)
    };
    s.add("J2-双向回环", j2, "双向十六字节回环全一致");

    // J3 耗尽受控：PTY_CAP 恰满可容、超一档必拒。
    let j3 = {
        let mut p = PtyPool::new();
        let mut i = 0;
        let mut all_ok = true;
        while i < PTY_CAP {
            if p.openpty("f").is_err() {
                all_ok = false;
            }
            i += 1;
        }
        all_ok && p.openpty("over").err() == Some(PT_EXHAUST)
    };
    s.add("J3-耗尽受控", j3, "池满四对恰收、第五对必拒");

    // J4 幽灵 fd 拒。
    let j4 = {
        let mut p = PtyPool::new();
        p.read(77).err() == Some(PT_NO_FD)
    };
    s.add("J4-幽灵 fd 拒", j4, "未分配 fd 操作必拒");

    // J5 断端受控：close 后两端皆失效。
    let j5 = {
        let mut p = PtyPool::new();
        let (m, s2) = p.openpty("c").unwrap_or((0, 0));
        let _ = p.close_fd(m);
        p.write(m, 1).err() == Some(PT_NO_FD) && p.write(s2, 1).err() == Some(PT_NO_FD)
    };
    s.add("J5-断端受控", j5, "对关闭后双端一致失效");

    // J6 多对隔离：两对缓冲互不串扰。
    let j6 = {
        let mut p = PtyPool::new();
        let (m1, s1) = p.openpty("p1").unwrap_or((0, 0));
        let (m2, s2) = p.openpty("p2").unwrap_or((0, 0));
        let _ = p.write(m1, 0xAA);
        let r2 = p.read(s2);
        let r1 = p.read(s1);
        r2 == Err(PT_CLOSED) && r1 == Ok(0xAA)
    };
    s.add("J6-多对隔离", j6, "对 1 字节不漏到对 2");

    // J7 账本完整：分配账合法且 seq 唯一。
    let j7 = {
        let mut p = PtyPool::new();
        let _ = p.openpty("a");
        let _ = p.openpty("b");
        p.ledger.len() == 2 && p.ledger_ok() && p.seq_unique()
    };
    s.add("J7-账本完整", j7, "分配账逐条合法无重号");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut p = PtyPool::new();
        let _ = p.openpty("a");
        let f1 = p.fold();
        let f2 = p.fold();
        p.seal();
        f1 == f2 && p.openpty("post").err() == Some(PT_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10181_openpty_flow() {
        let mut p = PtyPool::new();
        let (m, s2) = p.openpty("t").unwrap();
        assert_ne!(m, s2);
        assert!(p.write(m, 0x5A).is_ok());
        assert_eq!(p.read(s2), Ok(0x5A));
        assert!(p.roundtrip(m, s2, 8).unwrap_or(false));
        assert!(p.close_fd(m).is_ok());
        assert_eq!(p.write(m, 1), Err(PT_NO_FD));
    }

    #[test]
    fn unx10181_checks_green() {
        let s = run_unx10181_checks();
        assert!(s.all_passed(), "UNX-F10181 判据未全绿");
    }
}
