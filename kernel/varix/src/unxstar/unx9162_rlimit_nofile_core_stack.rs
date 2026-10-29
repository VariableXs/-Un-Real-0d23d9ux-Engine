//! UNX-F9162 RLIMIT_NOFILE/CORE/STACK 三限额逐格（0x63400 段）
//!
//! 锚点：UNX-F9162-J1 三格账（NOFILE/CORE/STACK 每格消费触发落账：
//! 资源、消耗值、限额、放行/拒收、留痕）；J2 三格语义（fd 表满拒
//! + core 禁投 + 栈顶越限拒三性质）；J3 三格指纹（账本折叠指纹）
//! 三判据全绿。
//! 域：CoRun·UNX-C2-B19（三限额消费格）。
//!
//! 结构：限额册（复用 B19 四元组骨架）+ 三消费核（open 挂点/dump
//! 挂点/栈增长挂点）+ 消费账本 + 折叠指纹 + 注入核（错码/空留痕/
//! 越限放行伪样本/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63400 独占 0x20）。
pub const SEG_BASE: u32 = 0x63400;
pub const RL2_UNK_RES: u32 = SEG_BASE + 0x01; // 伪资源
pub const RL2_BAD_LIMIT: u32 = SEG_BASE + 0x02; // 越限放行伪样本
pub const RL2_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const RL2_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const RL2_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 资源闭集（本档三格）。
pub const G_NOFILE: usize = 0;
pub const G_CORE: usize = 1;
pub const G_STACK: usize = 2;
pub const G_COUNT: usize = 3;

pub const G_NAMES: [&str; G_COUNT] = ["NOFILE", "CORE", "STACK"];

/// 单条消费账。
#[derive(Clone)]
pub struct UseRec {
    pub seq: u32,
    pub res: usize,
    pub want: u32,  // 消耗请求值
    pub limit: u32, // 当前限额
    pub allowed: bool,
    pub note: String,
}

impl UseRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.res < G_COUNT
    }

    pub fn render_line(&self) -> String {
        format!(
            "U2{}|{}|w{}<l{}|{}|{}",
            self.seq,
            G_NAMES[self.res],
            self.want,
            self.limit,
            if self.allowed { "GO" } else { "DENY" },
            self.note
        )
    }
}

/// 三格限额簿。
pub struct TriBook {
    pub limits: [u32; G_COUNT],
    /// NOFILE 消费：当前 fd 数
    pub open_fds: u32,
    pub ledger: Vec<UseRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl TriBook {
    pub fn new() -> TriBook {
        TriBook {
            limits: [1024, 0, 8 * 1024 * 1024], // 默认：fd 1024、core 禁、栈 8M
            open_fds: 3,                        // 0/1/2 预留
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    pub fn set_limit(&mut self, res: usize, v: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RL2_FROZEN);
        }
        if res >= G_COUNT {
            return Err(RL2_UNK_RES);
        }
        self.limits[res] = v;
        Ok(())
    }

    fn record(&mut self, res: usize, want: u32, allowed: bool, note: &str) {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(UseRec {
            seq,
            res,
            want,
            limit: self.limits[res],
            allowed,
            note: note.to_string(),
        });
    }

    /// NOFILE 消费核：open 挂点。fd 用尽 → 拒。
    pub fn open_fd(&mut self, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(RL2_FROZEN);
        }
        if note.is_empty() {
            return Err(RL2_EMPTY_NOTE);
        }
        let lim = self.limits[G_NOFILE];
        if self.open_fds >= lim {
            self.record(G_NOFILE, self.open_fds + 1, false, note);
            return Err(RL2_BAD_LIMIT);
        }
        self.open_fds += 1;
        self.record(G_NOFILE, self.open_fds, true, note);
        Ok(self.open_fds - 1)
    }

    pub fn close_fd(&mut self, fd: u32) -> Result<(), u32> {
        if fd >= self.open_fds {
            return Err(RL2_UNK_RES);
        }
        self.open_fds -= 1;
        Ok(())
    }

    /// CORE 消费核：dump 挂点。限额 0 → 禁投。
    pub fn core_dump(&mut self, size: u32, note: &str) -> Result<bool, u32> {
        if self.sealed {
            return Err(RL2_FROZEN);
        }
        if note.is_empty() {
            return Err(RL2_EMPTY_NOTE);
        }
        let lim = self.limits[G_CORE];
        let allowed = lim != 0 && size <= lim;
        self.record(G_CORE, size, allowed, note);
        if allowed {
            Ok(true)
        } else {
            Err(RL2_BAD_LIMIT)
        }
    }

    /// STACK 消费核：栈增长挂点。增长后总量越限 → 拒。
    pub fn stack_grow(&mut self, cur: u32, inc: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(RL2_FROZEN);
        }
        if note.is_empty() {
            return Err(RL2_EMPTY_NOTE);
        }
        let lim = self.limits[G_STACK];
        let total = cur.wrapping_add(inc);
        if total > lim {
            self.record(G_STACK, total, false, note);
            return Err(RL2_BAD_LIMIT);
        }
        self.record(G_STACK, total, true, note);
        Ok(total)
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
        let mut h: u32 = 0x91620;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.res as u32);
            h = h.wrapping_mul(31).wrapping_add(r.want);
            h = h.wrapping_mul(31).wrapping_add(r.allowed as u32);
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
    // 1) 伪资源格
    let bad = UseRec {
        seq: 1,
        res: 9,
        want: 1,
        limit: 1,
        allowed: true,
        note: "inj-res".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = UseRec {
        seq: 2,
        res: 0,
        want: 1,
        limit: 1,
        allowed: true,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) fd 表满拒
    let mut b = TriBook::new();
    b.limits[G_NOFILE] = 4; // 0/1/2 + 1 可用
    let _ = b.open_fd("占满");
    got[2] = b.open_fd("inj-full").err() == Some(RL2_BAD_LIMIT);
    // 4) core 禁投拒
    got[3] = b.core_dump(1024, "inj-core0").err() == Some(RL2_BAD_LIMIT);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.open_fd("inj-frozen").err() == Some(RL2_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9162_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9162");
    let mut b = TriBook::new();

    // J1 三格账非空
    let j1 = {
        let _ = b.open_fd("open 走 NOFILE 格");
        let _ = b.core_dump(0, "core 挂点");
        let _ = b.stack_grow(4096, 4096, "栈增长");
        b.ledger.iter().filter(|r| r.res == G_NOFILE).count() >= 1
            && b.ledger.iter().filter(|r| r.res == G_CORE).count() >= 1
            && b.ledger.iter().filter(|r| r.res == G_STACK).count() >= 1
    };
    s.add("J1-三格账齐", j1, "三格各至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且格闭集");

    // J3 NOFILE 表满拒
    let j3 = {
        let mut b2 = TriBook::new();
        b2.limits[G_NOFILE] = 4;
        let _ = b2.open_fd("3");
        let _ = b2.open_fd("4");
        b2.open_fd("满").err() == Some(RL2_BAD_LIMIT)
    };
    s.add("J3-NOFILE满拒", j3, "fd 表满 open 必拒");

    // J4 NOFILE close 回收
    let j4 = {
        let mut b2 = TriBook::new();
        b2.limits[G_NOFILE] = 4;
        let fd = b2.open_fd("3");
        let _ = b2.close_fd(fd.unwrap_or(0));
        b2.open_fd("复用").is_ok()
    };
    s.add("J4-close回收", j4, "close 后可再 open");

    // J5 CORE 禁投
    let j5 = {
        let mut b2 = TriBook::new();
        b2.core_dump(1024, "禁投").err() == Some(RL2_BAD_LIMIT)
    };
    s.add("J5-CORE禁投", j5, "core=0 dump 必拒");

    // J6 CORE 越限拒
    let j6 = {
        let mut b2 = TriBook::new();
        let _ = b2.set_limit(G_CORE, 4096);
        b2.core_dump(8192, "越限").err() == Some(RL2_BAD_LIMIT)
    };
    s.add("J6-CORE越限拒", j6, "dump 超限额必拒");

    // J7 CORE 合格放行
    let j7 = {
        let mut b2 = TriBook::new();
        let _ = b2.set_limit(G_CORE, 4096);
        b2.core_dump(2048, "合格") == Ok(true)
    };
    s.add("J7-CORE放行", j7, "限额内 dump 放行");

    // J8 STACK 越限拒
    let j8 = {
        let mut b2 = TriBook::new();
        b2.stack_grow(8 * 1024 * 1024 - 4096, 8192, "越限").err() == Some(RL2_BAD_LIMIT)
    };
    s.add("J8-STACK越限拒", j8, "栈顶越限必拒");

    // J9 STACK 合格放行
    let j9 = {
        let mut b2 = TriBook::new();
        b2.stack_grow(4096, 4096, "涨") == Ok(8192)
    };
    s.add("J9-STACK放行", j9, "限额内增长放行并回新总量");

    // J10 伪资源拒
    let j10 = b.set_limit(9, 1).err() == Some(RL2_UNK_RES);
    s.add("J10-伪资源拒", j10, "格出闭集必拒");

    // J11 拒收留痕
    let j11 = b.ledger.iter().any(|r| !r.allowed);
    s.add("J11-拒收留痕", j11, "DENY 路径有账");

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
    fn unx9162_tri_flow() {
        let mut b = TriBook::new();
        // NOFILE
        let fd = b.open_fd("a").unwrap();
        assert!(b.close_fd(fd).is_ok());
        // CORE
        assert_eq!(b.core_dump(1024, "b"), Err(RL2_BAD_LIMIT)); // 默认 0
        let _ = b.set_limit(G_CORE, 4096);
        assert_eq!(b.core_dump(2048, "c"), Ok(true));
        assert_eq!(b.core_dump(8192, "d"), Err(RL2_BAD_LIMIT));
        // STACK
        assert_eq!(b.stack_grow(100, 200, "e"), Ok(300));
        assert_eq!(b.stack_grow(u32::MAX - 1, 10, "f"), Err(RL2_BAD_LIMIT));
    }

    #[test]
    fn unx9162_checks_green() {
        let s = run_unx9162_checks();
        assert!(s.all_passed(), "UNX-F9162 判据未全绿");
    }
}
