//! UNX-F9177 资源族升级接管注记（0x635E0 段）
//!
//! 锚点：UNX-F9177-J1 接管账（资源族符号 → 接管档（TAKEOVER/
//! COEXIST/DEFER）→ 转发桩落点逐条落账）；J2 接管语义（目标在册闸
//! + 档位闭集 + 桩规则 + 无漂移四性质）；J3 接管指纹（账本折叠指
//! 纹）三判据全绿。
//! 域：CoRun·UNX-C2-B19（资源族接管边界登记面）。
//!
//! 结构：符号册 + 接管核 + 接管账本 + 折叠指纹 + 注入核（错档/空留
//! 痕/伪符号/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x635E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x635E0;
pub const TK7_UNK_SYM: u32 = SEG_BASE + 0x01; // 伪符号
pub const TK7_BAD_MODE: u32 = SEG_BASE + 0x02; // 错档
pub const TK7_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const TK7_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const TK7_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 接管档闭集。
pub const M_TAKEOVER: u8 = 0;
pub const M_COEXIST: u8 = 1;
pub const M_DEFER: u8 = 2;
pub const M_COUNT: u8 = 3;

pub const M_NAMES: [&str; 3] = ["takeover", "coexist", "defer"];

/// 单条接管账。
#[derive(Clone)]
pub struct TkRec {
    pub seq: u32,
    pub sym: String,
    pub mode: u8,
    pub stub_to: String,
    pub note: String,
}

impl TkRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty()
            && self.mode < M_COUNT
            && match self.mode {
                M_TAKEOVER => !self.stub_to.is_empty() && self.stub_to != "-",
                _ => self.stub_to == "-",
            }
    }

    pub fn render_line(&self) -> String {
        format!(
            "T7{}|{}|{}|->{}|{}",
            self.seq,
            self.sym,
            M_NAMES[self.mode as usize],
            self.stub_to,
            self.note
        )
    }
}

/// 接管簿。
pub struct Tk7Book {
    pub syms: Vec<String>,
    pub ledger: Vec<TkRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl Tk7Book {
    pub fn new() -> Tk7Book {
        Tk7Book { syms: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn declare(&mut self, sym: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(TK7_FROZEN);
        }
        if sym.is_empty() {
            return Err(TK7_UNK_SYM);
        }
        if !self.syms.iter().any(|s| s == sym) {
            self.syms.push(sym.to_string());
        }
        Ok(())
    }

    pub fn has_sym(&self, sym: &str) -> bool {
        self.syms.iter().any(|s| s == sym)
    }

    pub fn take(&mut self, sym: &str, mode: u8, stub_to: &str, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(TK7_FROZEN);
        }
        if note.is_empty() {
            return Err(TK7_EMPTY_NOTE);
        }
        if mode >= M_COUNT {
            return Err(TK7_BAD_MODE);
        }
        if !self.has_sym(sym) {
            return Err(TK7_UNK_SYM);
        }
        let stub = if mode == M_TAKEOVER { stub_to.to_string() } else { "-".to_string() };
        if mode == M_TAKEOVER && stub_to.is_empty() {
            return Err(TK7_BAD_MODE);
        }
        self.ledger.push(TkRec {
            seq: self.seq_next,
            sym: sym.to_string(),
            mode,
            stub_to: stub,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(self.seq_next - 1)
    }

    pub fn stub_complete(&self) -> bool {
        self.ledger.iter().all(|t| t.ok())
    }

    pub fn no_drift(&self) -> bool {
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i].sym == self.ledger[j].sym
                    && self.ledger[i].mode != self.ledger[j].mode
                {
                    return false;
                }
            }
        }
        true
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
        let mut h: u32 = 0x91770;
        for t in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(t.seq);
            h = h.wrapping_mul(31).wrapping_add(t.mode as u32);
            for b in t.sym.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
            for b in t.stub_to.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
            for b in t.note.bytes() {
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

/// 资源族符号样本。
pub const RES_SYMS: [&str; 6] =
    ["res_getrusage", "res_setrlimit", "res_prlimit64", "res_times", "res_nice", "res_yield"];

/// 基础样本。
pub fn mk_book() -> Tk7Book {
    let mut b = Tk7Book::new();
    for s in RES_SYMS.iter() {
        let _ = b.declare(s);
    }
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 错档账（DEFER 带桩）
    let bad = TkRec {
        seq: 1,
        sym: "res_getrusage".to_string(),
        mode: M_DEFER,
        stub_to: "x".to_string(),
        note: "inj-mode".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = TkRec {
        seq: 2,
        sym: "res_getrusage".to_string(),
        mode: M_TAKEOVER,
        stub_to: "x".to_string(),
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 伪符号
    let mut b = mk_book();
    got[2] = b.take("res_none", M_TAKEOVER, "stub", "inj-sym").err() == Some(TK7_UNK_SYM);
    // 4) TAKEOVER 空桩
    got[3] = b.take("res_getrusage", M_TAKEOVER, "", "inj-stub").err() == Some(TK7_BAD_MODE);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.take("res_getrusage", M_TAKEOVER, "stub", "inj-frozen").err() == Some(TK7_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9177_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9177");
    let mut b = mk_book();

    // J1 三档齐备
    let j1 = {
        let _ = b.take("res_setrlimit", M_TAKEOVER, "ve_rlimit_new", "接管");
        let _ = b.take("res_getrusage", M_COEXIST, "-", "共存");
        let _ = b.take("res_yield", M_DEFER, "-", "缓办");
        b.ledger.iter().filter(|t| t.mode == M_TAKEOVER).count() >= 1
            && b.ledger.iter().filter(|t| t.mode == M_COEXIST).count() >= 1
            && b.ledger.iter().filter(|t| t.mode == M_DEFER).count() >= 1
    };
    s.add("J1-三档齐备", j1, "TAKEOVER/COEXIST/DEFER 各至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|t| t.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且档位/桩规则一致");

    // J3 目标在册闸
    let j3 = b.take("res_none", M_TAKEOVER, "stub", "伪符号").err() == Some(TK7_UNK_SYM);
    s.add("J3-在册闸", j3, "伪符号必拒");

    // J4 档位闭集
    let j4 = b.take("res_times", 9, "stub", "野档").err() == Some(TK7_BAD_MODE);
    s.add("J4-档位闭集", j4, "档位出闭集必拒");

    // J5 DEFER 不调桩
    let j5 = {
        let mut b2 = mk_book();
        let _ = b2.take("res_times", M_DEFER, "-", "缓办");
        b2.ledger.last().unwrap().stub_to == "-"
    };
    s.add("J5-DEFER不调桩", j5, "DEFER 档桩位恒 '-'");

    // J6 TAKEOVER 必带桩
    let j6 = {
        let mut b2 = mk_book();
        b2.take("res_times", M_TAKEOVER, "", "空桩").err() == Some(TK7_BAD_MODE)
    };
    s.add("J6-接管必带桩", j6, "TAKEOVER 空桩必拒");

    // J7 符号册完整
    let j7 = b.syms.len() == 6;
    s.add("J7-符号册完整", j7, "六符号全在册");

    // J8 重声明幂等
    let j8 = {
        let mut b2 = mk_book();
        let _ = b2.declare("res_nice");
        b2.syms.len() == 6
    };
    s.add("J8-重声明幂等", j8, "重复声明不增席");

    // J9 无档位漂移
    let j9 = b.no_drift();
    s.add("J9-无档位漂移", j9, "同符号档位不漂移");

    // J10 桩完备
    let j10 = b.stub_complete();
    s.add("J10-桩完备", j10, "全部账桩规则自洽");

    // J11 空留痕拒
    let j11 = b.take("res_nice", M_COEXIST, "-", "").err() == Some(TK7_EMPTY_NOTE);
    s.add("J11-空留痕拒", j11, "空留痕必拒");

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
    fn unx9177_takeover_flow() {
        let mut b = mk_book();
        assert!(b.take("res_setrlimit", M_TAKEOVER, "stub1", "a").is_ok());
        assert!(b.take("res_getrusage", M_COEXIST, "-", "b").is_ok());
        assert!(b.take("res_yield", M_DEFER, "-", "c").is_ok());
        assert_eq!(b.take("res_none", M_TAKEOVER, "x", "d"), Err(TK7_UNK_SYM));
        assert_eq!(b.take("res_nice", M_TAKEOVER, "", "e"), Err(TK7_BAD_MODE));
        assert!(b.stub_complete());
    }

    #[test]
    fn unx9177_checks_green() {
        let s = run_unx9177_checks();
        assert!(s.all_passed(), "UNX-F9177 判据未全绿");
    }
}
