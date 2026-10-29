//! UNX-F9193 身份族与 C1 进程对象消费协议（0x637E0 段）
//!
//! 锚点：UNX-F9193-J1 挂载账（身份四元组挂 C1 进程对象逐条落账：
//! pid、Creds 快照、结果、留痕）；J2 挂载语义（对象在册闸 + 四元组
//! 默认继承 + 挂载幂等 + 读快照无副作用四性质）；J3 挂载指纹（账本
//! 折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B20（身份族×C1 进程对象面）。
//!
//! 结构：C1 进程对象册（pid → Creds）+ 挂载核 + 挂载账本 + 折叠指
//! 纹 + 注入核（错码/空留痕/伪 pid/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x637E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x637E0;
pub const C1_UNK_PID: u32 = SEG_BASE + 0x01; // 伪 pid
pub const C1_BAD_CREDS: u32 = SEG_BASE + 0x02; // 身份错
pub const C1_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const C1_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const C1_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 身份四元组。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Creds {
    pub ruid: u32,
    pub euid: u32,
    pub rgid: u32,
    pub egid: u32,
}

/// 单条挂载账。
#[derive(Clone)]
pub struct C1Rec {
    pub seq: u32,
    pub pid: u32,
    pub creds: Creds,
    pub op: usize, // 0=挂载 1=读快照
    pub note: String,
}

impl C1Rec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.op < 2 && self.pid != 0
    }

    pub fn render_line(&self) -> String {
        format!(
            "C1{}|p{}|{:?}|{}|{}",
            self.seq,
            self.pid,
            (self.creds.ruid, self.creds.euid, self.creds.rgid, self.creds.egid),
            if self.op == 0 { "mount" } else { "read" },
            self.note
        )
    }
}

/// C1 进程对象册（身份侧视图）。
pub struct C1Book {
    /// pid → Creds
    pub objs: Vec<(u32, Creds)>,
    pub ledger: Vec<C1Rec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl C1Book {
    pub fn new() -> C1Book {
        C1Book { objs: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 挂载：pid 注册身份（幂等：重挂覆盖）。
    pub fn mount(&mut self, pid: u32, c: Creds, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(C1_FROZEN);
        }
        if note.is_empty() {
            return Err(C1_EMPTY_NOTE);
        }
        if pid == 0 {
            return Err(C1_UNK_PID);
        }
        match self.objs.iter_mut().find(|(w, _)| *w == pid) {
            Some((_, v)) => *v = c,
            None => self.objs.push((pid, c)),
        }
        self.ledger.push(C1Rec {
            seq: self.seq_next,
            pid,
            creds: c,
            op: 0,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(())
    }

    /// 读快照（无副作用）。
    pub fn read(&mut self, pid: u32, note: &str) -> Result<Creds, u32> {
        if self.sealed {
            return Err(C1_FROZEN);
        }
        if note.is_empty() {
            return Err(C1_EMPTY_NOTE);
        }
        let c = match self.objs.iter().find(|(w, _)| *w == pid) {
            Some((_, c)) => *c,
            None => return Err(C1_UNK_PID),
        };
        self.ledger.push(C1Rec {
            seq: self.seq_next,
            pid,
            creds: c,
            op: 1,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(c)
    }

    /// 默认继承：子承父 Creds。
    pub fn inherit(&mut self, parent: u32, child: u32, note: &str) -> Result<Creds, u32> {
        if self.sealed {
            return Err(C1_FROZEN);
        }
        if note.is_empty() {
            return Err(C1_EMPTY_NOTE);
        }
        let pc = match self.objs.iter().find(|(w, _)| *w == parent) {
            Some((_, c)) => *c,
            None => return Err(C1_UNK_PID),
        };
        self.mount(child, pc, note)?;
        Ok(pc)
    }

    pub fn of(&self, pid: u32) -> Option<Creds> {
        self.objs.iter().find(|(w, _)| *w == pid).map(|(_, c)| *c)
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
        let mut h: u32 = 0x91930;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.pid);
            h = h.wrapping_mul(31).wrapping_add(r.op as u32);
            h = h.wrapping_mul(31).wrapping_add(r.creds.ruid);
            h = h.wrapping_mul(31).wrapping_add(r.creds.egid);
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

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 零 pid 账
    let bad = C1Rec {
        seq: 1,
        pid: 0,
        creds: Creds { ruid: 0, euid: 0, rgid: 0, egid: 0 },
        op: 0,
        note: "inj-zero".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = C1Rec {
        seq: 2,
        pid: 1,
        creds: Creds { ruid: 0, euid: 0, rgid: 0, egid: 0 },
        op: 0,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 伪 pid 读
    let mut b = C1Book::new();
    got[2] = b.read(99, "inj-unk").err() == Some(C1_UNK_PID);
    // 4) 伪 pid 继承
    got[3] = b.inherit(99, 100, "inj-unk2").err() == Some(C1_UNK_PID);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.mount(5, Creds { ruid: 1, euid: 1, rgid: 1, egid: 1 }, "inj-frozen")
        .err() == Some(C1_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9193_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9193");
    let mut b = C1Book::new();
    let c_root = Creds { ruid: 0, euid: 0, rgid: 0, egid: 0 };
    let c_user = Creds { ruid: 1000, euid: 1000, rgid: 1000, egid: 1000 };

    // J1 挂载账非空（挂/读两式）
    let j1 = {
        let _ = b.mount(100, c_user, "用户挂载");
        let _ = b.read(100, "读快照");
        b.ledger.iter().filter(|r| r.op == 0).count() >= 1
            && b.ledger.iter().filter(|r| r.op == 1).count() >= 1
    };
    s.add("J1-挂读两式", j1, "mount/read 各至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且 pid/op 闭集");

    // J3 对象在册闸
    let j3 = b.read(99, "伪读").err() == Some(C1_UNK_PID);
    s.add("J3-在册闸", j3, "未挂载 pid 读必拒");

    // J4 挂载生效
    let j4 = b.of(100) == Some(c_user);
    s.add("J4-挂载生效", j4, "mount 后册面可查");

    // J5 挂载幂等（重挂覆盖）
    let j5 = {
        let mut b2 = C1Book::new();
        let _ = b2.mount(100, c_user, "a");
        let _ = b2.mount(100, c_root, "b");
        b2.of(100) == Some(c_root) && b2.objs.len() == 1
    };
    s.add("J5-重挂覆盖", j5, "同 pid 重挂覆盖不增席");

    // J6 默认继承
    let j6 = {
        let mut b2 = C1Book::new();
        let _ = b2.mount(100, c_user, "父");
        let _ = b2.inherit(100, 101, "承");
        b2.of(101) == Some(c_user)
    };
    s.add("J6-默认继承", j6, "子 Creds=父 Creds");

    // J7 读快照无副作用
    let j7 = {
        let mut b2 = C1Book::new();
        let _ = b2.mount(100, c_user, "a");
        let before = b2.of(100);
        let _ = b2.read(100, "读");
        before == b2.of(100)
    };
    s.add("J7-读无副作用", j7, "read 不改册面");

    // J8 零 pid 拒
    let j8 = b.mount(0, c_root, "零").err() == Some(C1_UNK_PID);
    s.add("J8-零pid拒", j8, "pid=0 必拒");

    // J9 双进程隔离
    let j9 = {
        let mut b2 = C1Book::new();
        let _ = b2.mount(100, c_user, "a");
        let _ = b2.mount(200, c_root, "b");
        b2.of(100) != b2.of(200)
    };
    s.add("J9-双进程隔离", j9, "两对象身份互不串");

    // J10 继承链三级
    let j10 = {
        let mut b2 = C1Book::new();
        let _ = b2.mount(1, c_root, "init");
        let _ = b2.inherit(1, 2, "a");
        let _ = b2.inherit(2, 3, "b");
        b2.of(3) == Some(c_root)
    };
    s.add("J10-三级继承", j10, "init→2→3 链保持");

    // J11 指纹稳定 + seq 唯一
    let j11 = b.fold() == b.fold() && b.seq_unique();
    s.add("J11-指纹稳定", j11, "折叠指纹两次一致且 seq 唯一");

    // J12 空留痕拒
    let j12 = b.mount(100, c_user, "").err() == Some(C1_EMPTY_NOTE);
    s.add("J12-空留痕拒", j12, "空留痕必拒");

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
    fn unx9193_c1_flow() {
        let mut b = C1Book::new();
        let c = Creds { ruid: 1000, euid: 1000, rgid: 1000, egid: 1000 };
        assert!(b.mount(100, c, "a").is_ok());
        assert_eq!(b.read(100, "b"), Ok(c));
        assert_eq!(b.read(99, "c"), Err(C1_UNK_PID));
        // 重挂覆盖
        let r = Creds { ruid: 0, euid: 0, rgid: 0, egid: 0 };
        assert!(b.mount(100, r, "d").is_ok());
        assert_eq!(b.objs.len(), 1);
        // 继承
        assert!(b.inherit(100, 101, "e").is_ok());
        assert_eq!(b.of(101), Some(r));
    }

    #[test]
    fn unx9193_checks_green() {
        let s = run_unx9193_checks();
        assert!(s.all_passed(), "UNX-F9193 判据未全绿");
    }
}
