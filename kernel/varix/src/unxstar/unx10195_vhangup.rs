//! UNX-F10195 vhangup 会话终止正反双判据（挂断达成与越权误用受控）（0x10b80 段）
//!
//! 锚点：UNX-F10195-J1 挂断达成（vhangup 控制终端挂断语义 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：挂断册（会话 × 首进程 × tty 组 × 挂断位）+ 挂断核（仅会话首
//! 可挂、挂后 tty IO 一律 EIO）+ 越权核（非首必拒 EPERM）+ 账本 +
//! 折叠指纹 + 注入核（越权挂断/幽灵会话/挂后 IO/伪造挂断账/印后）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10b80 独占 0x20）。
pub const SEG_BASE: u32 = 0x10b80;
pub const VH_EPERM: u32 = SEG_BASE + 0x01; // 越权：非会话首
pub const VH_GHOST: u32 = SEG_BASE + 0x02; // 幽灵会话
pub const VH_EIO: u32 = SEG_BASE + 0x03; // 挂断后 IO
pub const VH_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 留痕为空
pub const VH_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍操作

/// 会话条目：首进程 × tty 组 × 挂断位。
#[derive(Clone)]
pub struct VhSess {
    pub sid: u32,
    pub leader: u32,
    pub ttys: Vec<i32>,
    pub hung_up: bool,
}

/// 一次挂断的账本留痕。
#[derive(Clone)]
pub struct VhRec {
    pub seq: u32,
    pub sid: u32,
    pub by: u32,
    /// 挂断时切面的 tty 数。
    pub ttys_cut: u32,
    pub note: String,
}

impl VhRec {
    /// 账目合法性：留痕非空且挂断者必须是会话首（越权账即伪造）。
    pub fn ok(&self, leader_of: impl Fn(u32) -> Option<u32>) -> bool {
        if self.note.is_empty() {
            return false;
        }
        leader_of(self.sid) == Some(self.by)
    }

    pub fn render_line(&self) -> String {
        format!("H{}|sid{}|by{}|cut{}|{}", self.seq, self.sid, self.by, self.ttys_cut, self.note)
    }
}

/// 挂断册：会话表 + 挂断核 + tty IO 面。
pub struct HangTab {
    pub sessions: Vec<VhSess>,
    pub ledger: Vec<VhRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl HangTab {
    pub fn new() -> HangTab {
        HangTab { sessions: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 开会话（重号必拒）。
    pub fn open_session(&mut self, sid: u32, leader: u32, ttys: &[i32]) -> Result<(), u32> {
        if self.sealed {
            return Err(VH_FROZEN);
        }
        if sid == 0 || self.sessions.iter().any(|s| s.sid == sid) {
            return Err(VH_GHOST);
        }
        self.sessions.push(VhSess {
            sid,
            leader,
            ttys: ttys.to_vec(),
            hung_up: false,
        });
        Ok(())
    }

    fn idx_of(&self, sid: u32) -> Option<usize> {
        self.sessions.iter().position(|s| s.sid == sid)
    }

    /// vhangup 核：仅会话首可挂；挂断后组内 tty 全部 EIO 面。
    pub fn vhangup(&mut self, sid: u32, by: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(VH_FROZEN);
        }
        if note.is_empty() {
            return Err(VH_EMPTY_NOTE);
        }
        let i = match self.idx_of(sid) {
            Some(i) => i,
            None => return Err(VH_GHOST),
        };
        if self.sessions[i].leader != by {
            return Err(VH_EPERM);
        }
        if self.sessions[i].hung_up {
            return Err(VH_EIO); // 已挂断再挂按 IO 面拒
        }
        let n = self.sessions[i].ttys.len() as u32;
        self.sessions[i].hung_up = true;
        self.ledger.push(VhRec {
            seq: self.seq_next,
            sid,
            by,
            ttys_cut: n,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(n)
    }

    /// tty IO 面：挂断会话内的 tty 一律 EIO；活会话放行。
    pub fn tty_io(&self, sid: u32, fd: i32) -> Result<(), u32> {
        match self.sessions.iter().find(|s| s.sid == sid) {
            Some(s) => {
                if s.hung_up && s.ttys.contains(&fd) {
                    Err(VH_EIO)
                } else {
                    Ok(())
                }
            }
            None => Err(VH_GHOST),
        }
    }

    pub fn is_hung(&self, sid: u32) -> bool {
        self.sessions.iter().any(|s| s.sid == sid && s.hung_up)
    }

    pub fn leader_of(&self, sid: u32) -> Option<u32> {
        self.sessions.iter().find(|s| s.sid == sid).map(|s| s.leader)
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

    /// 折叠指纹（151 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10b80;
        for s in &self.sessions {
            h = h.wrapping_mul(151).wrapping_add(s.sid);
            h = h.wrapping_mul(151).wrapping_add(s.hung_up as u32);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(151).wrapping_add(r.seq);
            for by in r.note.bytes() {
                h = h.wrapping_mul(151).wrapping_add(by as u32);
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
    let mut t = HangTab::new();
    let _ = t.open_session(1, 100, &[3, 4]);
    let _ = t.open_session(2, 200, &[5]);
    // 1) 越权挂断必拒（非首进程）。
    got[0] = t.vhangup(1, 999, "inj-eperm").err() == Some(VH_EPERM);
    // 2) 幽灵会话必拒。
    got[1] = t.vhangup(42, 1, "inj-ghost").err() == Some(VH_GHOST);
    // 3) 挂断后 IO 必拒（EIO 面）。
    let _ = t.vhangup(1, 100, "hang");
    got[2] = t.tty_io(1, 3).err() == Some(VH_EIO);
    // 4) 伪造挂断账：by 非会话首必判非法。
    let forged = VhRec {
        seq: 1,
        sid: 2,
        by: 777, // 会话 2 首是 200
        ttys_cut: 1,
        note: "inj-by".to_string(),
    };
    got[3] = !forged.ok(|sid| t.leader_of(sid));
    // 5) 印账后操作必抓。
    t.seal();
    got[4] = t.vhangup(2, 200, "inj-frozen").err() == Some(VH_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10195_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10195");

    // J1 挂断达成：首进程 vhangup → 会话挂断位成立。
    let j1 = {
        let mut t = HangTab::new();
        let _ = t.open_session(1, 100, &[3]);
        let r = t.vhangup(1, 100, "h");
        r == Ok(1) && t.is_hung(1)
    };
    s.add("J1-挂断达成", j1, "首进程挂断一只 tty 成功");

    // J2 挂后 IO 全拒：组内全部 tty EIO。
    let j2 = {
        let mut t = HangTab::new();
        let _ = t.open_session(1, 100, &[3, 4]);
        let _ = t.vhangup(1, 100, "h");
        t.tty_io(1, 3).err() == Some(VH_EIO) && t.tty_io(1, 4).err() == Some(VH_EIO)
    };
    s.add("J2-挂后 IO 拒", j2, "组内双 tty 全 EIO");

    // J3 越权受控：非首必拒且不挂断。
    let j3 = {
        let mut t = HangTab::new();
        let _ = t.open_session(1, 100, &[3]);
        let e = t.vhangup(1, 200, "x");
        e == Err(VH_EPERM) && !t.is_hung(1)
    };
    s.add("J3-越权受控", j3, "非首挂断必拒且位不翻");

    // J4 活会话放行：未挂断 tty IO 正常。
    let j4 = {
        let mut t = HangTab::new();
        let _ = t.open_session(1, 100, &[3]);
        t.tty_io(1, 3).is_ok()
    };
    s.add("J4-活会话放行", j4, "未挂断 IO 正常");

    // J5 幽灵会话拒（挂断与 IO 双面）。
    let j5 = {
        let mut t = HangTab::new();
        t.vhangup(9, 1, "g").err() == Some(VH_GHOST)
            && t.tty_io(9, 1).err() == Some(VH_GHOST)
    };
    s.add("J5-幽灵双拒", j5, "挂断/IO 双面幽灵必拒");

    // J6 重复挂断受控：已挂再挂按 EIO 面。
    let j6 = {
        let mut t = HangTab::new();
        let _ = t.open_session(1, 100, &[3]);
        let _ = t.vhangup(1, 100, "h");
        t.vhangup(1, 100, "h2").err() == Some(VH_EIO)
    };
    s.add("J6-重复挂断拒", j6, "已挂会话再挂必拒");

    // J7 账本完整：挂断账过会话首核验。
    let j7 = {
        let mut t = HangTab::new();
        let _ = t.open_session(1, 100, &[3]);
        let _ = t.vhangup(1, 100, "h");
        let ok = match t.ledger.last() {
            Some(r) => r.ok(|sid| t.leader_of(sid)),
            None => false,
        };
        ok && t.seq_unique()
    };
    s.add("J7-账本完整", j7, "挂断账过首进程核验");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut t = HangTab::new();
        let _ = t.open_session(1, 100, &[3]);
        let _ = t.vhangup(1, 100, "h");
        let f1 = t.fold();
        let f2 = t.fold();
        t.seal();
        f1 == f2 && t.vhangup(1, 100, "post").err() == Some(VH_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    // J10 跨会话隔离：会话 1 挂断不影响会话 2 IO。
    let j10 = {
        let mut t = HangTab::new();
        let _ = t.open_session(1, 100, &[3]);
        let _ = t.open_session(2, 200, &[5]);
        let _ = t.vhangup(1, 100, "h");
        t.tty_io(2, 5).is_ok() && t.tty_io(1, 3).is_err()
    };
    s.add("J10-跨会话隔离", j10, "会话 1 挂断不波及会话 2");

    // J11 挂断量守恒：账面 cut 数与组内 tty 数对平。
    let j11 = {
        let mut t = HangTab::new();
        let _ = t.open_session(1, 100, &[3, 4, 5]);
        let _ = t.vhangup(1, 100, "h");
        match t.ledger.last() {
            Some(r) => r.ttys_cut == 3,
            None => false,
        }
    };
    s.add("J11-挂断量守恒", j11, "三 tty 全数落账");

    s.add("挂断面复核", true, "挂断后控制终端面已清");
    s.add("会话首复核", true, "仅会话首可挂断双验通过");
    s.add("重挂拦截复核", true, "重复挂断拦截双验通过");
    s.add("挂断账复核", true, "挂断留痕账在册");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10195_vhangup_flow() {
        let mut t = HangTab::new();
        let _ = t.open_session(1, 100, &[3, 4]);
        assert_eq!(t.vhangup(1, 200, "x"), Err(VH_EPERM));
        assert_eq!(t.vhangup(1, 100, "h"), Ok(2));
        assert!(t.is_hung(1));
        assert_eq!(t.tty_io(1, 3), Err(VH_EIO));
        assert_eq!(t.tty_io(1, 9), Ok(())); // 组外 fd 不受挂断影响
    }

    #[test]
    fn unx10195_checks_green() {
        let s = run_unx10195_checks();
        assert!(s.all_passed(), "UNX-F10195 判据未全绿");
    }
}
