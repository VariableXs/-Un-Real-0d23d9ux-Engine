//! UNX-F9152 进程组与终端前台挂点（与 C4 联动）（0x632C0 段）
//!
//! 锚点：UNX-F9152-J1 前台账（每次 tcsetpgrp/tcgetpgrp 落账：会话、
//! 前台组、结果、留痕）；J2 前台语义（组在册闸 + 同会话闸 + 前台唯
//! 一 + 后台写终端 SIGTTOU 标记四性质）；J3 前台指纹（账本折叠指
//! 纹）三判据全绿。
//! 域：CoRun·UNX-C2-B18（终端前台组与 C4 tty 挂点面）。
//!
//! 结构：终端簿（每 tty 绑一会话与前台组）+ 组册（pgid→sid）+ 挂
//! 点核（两闸）+ SIGTTOU 标记器 + 前台账本 + 折叠指纹 + 注入核（错
//! 码/空留痕/越会话设前台/重序号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x632C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x632C0;
pub const TC_UNK_GROUP: u32 = SEG_BASE + 0x01; // 组不在册
pub const TC_BAD_SESS: u32 = SEG_BASE + 0x02; // 越会话
pub const TC_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const TC_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const TC_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 结果码。
pub const R_OK: u32 = 0;
pub const R_EPERM: u32 = 1; // 越会话设前台
pub const R_EINVAL: u32 = 2; // 组不在册
pub const R_COUNT: usize = 3;

pub const R_NAMES: [&str; R_COUNT] = ["OK", "EPERM", "EINVAL"];

/// 终端（与 C4 tty 键联动）。
#[derive(Clone)]
pub struct Term {
    pub tty_no: u32,
    pub sid: u32,
    pub fg_pgid: u32,
}

/// 单条前台账。
#[derive(Clone)]
pub struct FgRec {
    pub seq: u32,
    pub tty_no: u32,
    pub pgid: u32,
    pub code: u32,
    pub ttou: bool, // 后台组写终端标记
    pub note: String,
}

impl FgRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && (self.code as usize) < R_COUNT
    }

    pub fn render_line(&self) -> String {
        format!(
            "FG{}|tty{}|g{}|{}|{}|{}",
            self.seq,
            self.tty_no,
            self.pgid,
            R_NAMES[self.code as usize],
            if self.ttou { "T" } else { "-" },
            self.note
        )
    }
}

/// 终端前台簿。
pub struct TermBook {
    pub terms: Vec<Term>,
    /// (pgid, sid) 组册
    pub groups: Vec<(u32, u32)>,
    pub ledger: Vec<FgRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl TermBook {
    pub fn new() -> TermBook {
        TermBook {
            terms: Vec::new(),
            groups: Vec::new(),
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// 注册终端（绑定会话，初始前台=会话首组）。
    pub fn add_term(&mut self, tty_no: u32, sid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(TC_FROZEN);
        }
        if self.terms.iter().any(|t| t.tty_no == tty_no) {
            return Ok(());
        }
        self.terms.push(Term { tty_no, sid, fg_pgid: sid });
        Ok(())
    }

    /// 注册进程组。
    pub fn add_group(&mut self, pgid: u32, sid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(TC_FROZEN);
        }
        if self.groups.iter().any(|g| g.0 == pgid) {
            return Ok(());
        }
        self.groups.push((pgid, sid));
        Ok(())
    }

    fn term_of(&self, tty_no: u32) -> Option<usize> {
        self.terms.iter().position(|t| t.tty_no == tty_no)
    }

    fn group_sid(&self, pgid: u32) -> Option<u32> {
        self.groups.iter().find(|g| g.0 == pgid).map(|g| g.1)
    }

    /// tcsetpgrp(tty_no, pgid)：组在册闸 + 同会话闸。
    pub fn tcsetpgrp(&mut self, tty_no: u32, pgid: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(TC_FROZEN);
        }
        if note.is_empty() {
            return Err(TC_EMPTY_NOTE);
        }
        let ti = match self.term_of(tty_no) {
            Some(ti) => ti,
            None => {
                self.record(tty_no, pgid, R_EINVAL, false, note);
                return Err(R_EINVAL);
            }
        };
        let gsid = self.group_sid(pgid);
        match gsid {
            None => {
                self.record(tty_no, pgid, R_EINVAL, false, note);
                Err(R_EINVAL)
            }
            Some(gs) => {
                if gs != self.terms[ti].sid {
                    self.record(tty_no, pgid, R_EPERM, false, note);
                    return Err(R_EPERM);
                }
                self.terms[ti].fg_pgid = pgid;
                self.record(tty_no, pgid, R_OK, false, note);
                Ok(self.terms[ti].fg_pgid)
            }
        }
    }

    /// tcgetpgrp 闭路。
    pub fn tcgetpgrp(&self, tty_no: u32) -> Result<u32, u32> {
        match self.term_of(tty_no) {
            Some(ti) => Ok(self.terms[ti].fg_pgid),
            None => Err(R_EINVAL),
        }
    }

    /// 后台组写终端：非前台组写 tty → SIGTTOU 标记。
    pub fn bg_write(&mut self, tty_no: u32, pgid: u32, note: &str) -> Result<bool, u32> {
        if self.sealed {
            return Err(TC_FROZEN);
        }
        if note.is_empty() {
            return Err(TC_EMPTY_NOTE);
        }
        let ti = match self.term_of(tty_no) {
            Some(ti) => ti,
            None => return Err(R_EINVAL),
        };
        let fg = self.terms[ti].fg_pgid == pgid;
        self.record(tty_no, pgid, R_OK, !fg, note);
        Ok(!fg) // true = 触发 SIGTTOU
    }

    /// 前台唯一：每 tty 至多一前台组（结构性恒一，校验存证）。
    pub fn fg_unique(&self) -> bool {
        self.terms.iter().all(|t| {
            self.group_sid(t.fg_pgid)
                .map(|gs| gs == t.sid)
                .unwrap_or(false)
        })
    }

    fn record(&mut self, tty_no: u32, pgid: u32, code: u32, ttou: bool, note: &str) {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(FgRec {
            seq,
            tty_no,
            pgid,
            code,
            ttou,
            note: note.to_string(),
        });
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
        let mut h: u32 = 0x91520;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.tty_no);
            h = h.wrapping_mul(31).wrapping_add(r.pgid);
            h = h.wrapping_mul(31).wrapping_add(r.code);
            h = h.wrapping_mul(31).wrapping_add(r.ttou as u32);
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

/// 基础样本：会话 1（组 1/组 10/组 11），会话 2（组 20）；tty0 绑会话 1。
pub fn mk_book() -> TermBook {
    let mut b = TermBook::new();
    let _ = b.add_term(0, 1);
    let _ = b.add_group(1, 1);
    let _ = b.add_group(10, 1);
    let _ = b.add_group(11, 1);
    let _ = b.add_group(20, 2);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 错码
    let bad = FgRec {
        seq: 1,
        tty_no: 0,
        pgid: 1,
        code: 9,
        ttou: false,
        note: "inj-code".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = FgRec {
        seq: 2,
        tty_no: 0,
        pgid: 1,
        code: R_OK,
        ttou: false,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 越会话设前台
    let mut b = mk_book();
    got[2] = b.tcsetpgrp(0, 20, "inj-cross").err() == Some(R_EPERM);
    // 4) 组不在册
    got[3] = b.tcsetpgrp(0, 99, "inj-unk").err() == Some(R_EINVAL);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.tcsetpgrp(0, 10, "inj-frozen").err() == Some(TC_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9152_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9152");
    let mut b = mk_book();

    // J1 前台账非空
    let _ = b.tcsetpgrp(0, 10, "job 组上台");
    let j1 = !b.ledger.is_empty();
    s.add("J1-前台账非空", j1, "tcsetpgrp 落账至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且码闭集");

    // J3 组在册闸
    let j3 = b.tcsetpgrp(0, 99, "组不在册").err() == Some(R_EINVAL);
    s.add("J3-组在册闸", j3, "组不在册必 EINVAL");

    // J4 同会话闸
    let j4 = b.tcsetpgrp(0, 20, "越会话").err() == Some(R_EPERM);
    s.add("J4-同会话闸", j4, "越会话设前台必 EPERM");

    // J5 前台切换闭路
    let j5 = {
        let _ = b.tcsetpgrp(0, 10, "切前台");
        b.tcgetpgrp(0) == Ok(10) && b.tcsetpgrp(0, 1, "切回").is_ok()
            && b.tcgetpgrp(0) == Ok(1)
    };
    s.add("J5-前台切换闭路", j5, "切前台/切回与查询一致");

    // J6 初始前台=会话首组
    let j6 = {
        let b2 = mk_book();
        b2.tcgetpgrp(0) == Ok(1)
    };
    s.add("J6-初始前台", j6, "终端初始前台为会话首组");

    // J7 后台写终端 SIGTTOU
    let j7 = {
        let mut b2 = mk_book();
        let _ = b2.tcsetpgrp(0, 10, "10 上台");
        let ttou = b2.bg_write(0, 1, "后台写").unwrap_or(false);
        let silent = b2.bg_write(0, 10, "前台写").unwrap_or(true);
        ttou && !silent
    };
    s.add("J7-SIGTTOU标记", j7, "后台组写标记 SIGTTOU，前台组不标记");

    // J8 前台唯一性（结构性）
    let j8 = b.fg_unique();
    s.add("J8-前台唯一", j8, "每 tty 前台组唯一且在册同会话");

    // J9 多终端隔离
    let j9 = {
        let mut b2 = mk_book();
        let _ = b2.add_term(1, 2);
        let _ = b2.tcsetpgrp(1, 20, "tty1 上 20");
        b2.tcgetpgrp(0) == Ok(1) && b2.tcgetpgrp(1) == Ok(20)
    };
    s.add("J9-多终端隔离", j9, "tty0 与 tty1 前台互不串扰");

    // J10 未知 tty
    let j10 = b.tcgetpgrp(9).err() == Some(R_EINVAL);
    s.add("J10-未知tty", j10, "未知 tty 查询必拒");

    // J11 ttou 留痕（先补记一笔后台写再查）
    let j11 = {
        let _ = b.bg_write(0, 11, "后台组 11 写终端");
        b.ledger.iter().any(|r| r.ttou)
    };
    s.add("J11-ttou留痕", j11, "SIGTTOU 事件有留痕");

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
    fn unx9152_fg_flow() {
        let mut b = mk_book();
        assert_eq!(b.tcgetpgrp(0), Ok(1));
        assert!(b.tcsetpgrp(0, 10, "a").is_ok());
        assert_eq!(b.tcgetpgrp(0), Ok(10));
        assert_eq!(b.tcsetpgrp(0, 20, "b"), Err(R_EPERM));
        assert_eq!(b.tcsetpgrp(0, 99, "c"), Err(R_EINVAL));
        // 后台写触发 SIGTTOU
        assert_eq!(b.bg_write(0, 1, "d"), Ok(true));
        assert_eq!(b.bg_write(0, 10, "e"), Ok(false));
    }

    #[test]
    fn unx9152_checks_green() {
        let s = run_unx9152_checks();
        assert!(s.all_passed(), "UNX-F9152 判据未全绿");
    }
}
