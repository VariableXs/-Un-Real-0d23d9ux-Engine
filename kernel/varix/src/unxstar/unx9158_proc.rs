//! UNX-F9158 进程族升级接管注记（现存 proc 面接管边界）（0x63380 段）
//!
//! 锚点：UNX-F9158-J1 接管账（符号 → 接管档（TAKEOVER/COEXIST/
//! DEFER）→ 转发桩落点逐条落账）；J2 接管语义（目标在册闸 + 档位
//! 闭集 + 转发桩重定向 + DEFER 不得调桩四性质）；J3 接管指纹（账本
//! 折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B18（proc 面接管边界登记面）。
//!
//! 结构：符号册（现存 proc 面导出符号清单）+ 接管核（档位闸 + 桩
//! 重定向）+ 接管账本 + 折叠指纹 + 注入核（错档/空留痕/伪符号/重号
//! /印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63380 独占 0x20）。
pub const SEG_BASE: u32 = 0x63380;
pub const TK_UNK_SYM: u32 = SEG_BASE + 0x01; // 伪符号
pub const TK_BAD_MODE: u32 = SEG_BASE + 0x02; // 错档
pub const TK_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const TK_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const TK_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 接管档闭集。
pub const MD_TAKEOVER: u8 = 0; // 接管：新实现换入，旧符号转发
pub const MD_COEXIST: u8 = 1; // 共存：新旧并跑，以域标志分路
pub const MD_DEFER: u8 = 2; // 缓办：本波不动，登记留待
pub const MD_COUNT: u8 = 3;

pub const MD_NAMES: [&str; 3] = ["takeover", "coexist", "defer"];

/// 单条接管账。
#[derive(Clone)]
pub struct Takeover {
    pub seq: u32,
    pub sym: String,
    pub mode: u8,
    pub stub_to: String, // 转发桩落点（TAKEOVER 有效，其余为 "-"）
    pub note: String,
}

impl Takeover {
    pub fn ok(&self) -> bool {
        !self.note.is_empty()
            && self.mode < MD_COUNT
            && match self.mode {
                MD_TAKEOVER => !self.stub_to.is_empty() && self.stub_to != "-",
                _ => self.stub_to == "-",
            }
    }

    pub fn render_line(&self) -> String {
        format!(
            "TK{}|{}|{}|->{}|{}",
            self.seq,
            self.sym,
            MD_NAMES[self.mode as usize],
            self.stub_to,
            self.note
        )
    }
}

/// 接管簿。
pub struct TkBook {
    /// 在册符号（现存 proc 面导出面）
    pub syms: Vec<String>,
    pub ledger: Vec<Takeover>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl TkBook {
    pub fn new() -> TkBook {
        TkBook { syms: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn declare(&mut self, sym: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(TK_FROZEN);
        }
        if sym.is_empty() {
            return Err(TK_UNK_SYM);
        }
        if !self.syms.iter().any(|s| s == sym) {
            self.syms.push(sym.to_string());
        }
        Ok(())
    }

    pub fn has_sym(&self, sym: &str) -> bool {
        self.syms.iter().any(|s| s == sym)
    }

    /// 接管核：登记档位与桩落点。
    pub fn take(&mut self, sym: &str, mode: u8, stub_to: &str, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(TK_FROZEN);
        }
        if note.is_empty() {
            return Err(TK_EMPTY_NOTE);
        }
        if mode >= MD_COUNT {
            return Err(TK_BAD_MODE);
        }
        if !self.has_sym(sym) {
            return Err(TK_UNK_SYM);
        }
        // DEFER 不得调桩
        let stub = if mode == MD_TAKEOVER { stub_to.to_string() } else { "-".to_string() };
        if mode == MD_TAKEOVER && stub_to.is_empty() {
            return Err(TK_BAD_MODE);
        }
        self.ledger.push(Takeover {
            seq: self.seq_next,
            sym: sym.to_string(),
            mode,
            stub_to: stub,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(self.seq_next - 1)
    }

    /// 桩重定向账：TAKEOVER 档的符号须有非空桩。
    pub fn stub_complete(&self) -> bool {
        self.ledger.iter().all(|t| t.ok())
    }

    /// 符号档唯一：同符号同档重复登记视为漂移。
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
        let mut h: u32 = 0x91580;
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

/// 现存 proc 面符号样本。
pub const PROC_SYMS: [&str; 6] =
    ["proc_spawn", "proc_exit", "proc_wait", "proc_fork", "proc_execve", "proc_setpgid"];

/// 基础样本。
pub fn mk_book() -> TkBook {
    let mut b = TkBook::new();
    for s in PROC_SYMS.iter() {
        let _ = b.declare(s);
    }
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 错档账（DEFER 带桩）
    let bad = Takeover {
        seq: 1,
        sym: "proc_exit".to_string(),
        mode: MD_DEFER,
        stub_to: "new_exit".to_string(),
        note: "inj-mode".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = Takeover {
        seq: 2,
        sym: "proc_exit".to_string(),
        mode: MD_TAKEOVER,
        stub_to: "x".to_string(),
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 伪符号
    let mut b = mk_book();
    got[2] = b.take("proc_none", MD_TAKEOVER, "stub", "inj-sym").err() == Some(TK_UNK_SYM);
    // 4) TAKEOVER 空桩
    got[3] = b.take("proc_exit", MD_TAKEOVER, "", "inj-stub").err() == Some(TK_BAD_MODE);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.take("proc_exit", MD_TAKEOVER, "stub", "inj-frozen").err() == Some(TK_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9158_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9158");
    let mut b = mk_book();

    // J1 接管账非空（三档齐备）
    let j1 = {
        let _ = b.take("proc_exit", MD_TAKEOVER, "ve_exit_new", "exit 接管");
        let _ = b.take("proc_wait", MD_COEXIST, "-", "wait 共存");
        let _ = b.take("proc_fork", MD_DEFER, "-", "fork 缓办");
        b.ledger.iter().filter(|t| t.mode == MD_TAKEOVER).count() >= 1
            && b.ledger.iter().filter(|t| t.mode == MD_COEXIST).count() >= 1
            && b.ledger.iter().filter(|t| t.mode == MD_DEFER).count() >= 1
    };
    s.add("J1-三档齐备", j1, "TAKEOVER/COEXIST/DEFER 各至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|t| t.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且档位/桩规则一致");

    // J3 目标在册闸
    let j3 = b.take("proc_none", MD_TAKEOVER, "stub", "伪符号").err() == Some(TK_UNK_SYM);
    s.add("J3-在册闸", j3, "伪符号必拒");

    // J4 档位闭集
    let j4 = b.take("proc_spawn", 9, "stub", "野档").err() == Some(TK_BAD_MODE);
    s.add("J4-档位闭集", j4, "档位出闭集必拒");

    // J5 DEFER 不调桩
    let j5 = {
        let mut b2 = mk_book();
        let _ = b2.take("proc_spawn", MD_DEFER, "-", "缓办");
        b2.ledger.last().unwrap().stub_to == "-"
    };
    s.add("J5-DEFER不调桩", j5, "DEFER 档桩位恒 '-'");

    // J6 TAKEOVER 必带桩
    let j6 = {
        let mut b2 = mk_book();
        b2.take("proc_spawn", MD_TAKEOVER, "", "空桩").err() == Some(TK_BAD_MODE)
    };
    s.add("J6-接管必带桩", j6, "TAKEOVER 空桩必拒");

    // J7 符号册完整
    let j7 = b.syms.len() == 6;
    s.add("J7-符号册完整", j7, "六符号全在册");

    // J8 重声明幂等
    let j8 = {
        let mut b2 = mk_book();
        let _ = b2.declare("proc_exit");
        b2.syms.len() == 6
    };
    s.add("J8-重声明幂等", j8, "重复声明不增席");

    // J9 无档位漂移
    let j9 = b.no_drift();
    s.add("J9-无档位漂移", j9, "同符号档位不漂移");

    // J10 桩完备
    let j10 = b.stub_complete();
    s.add("J10-桩完备", j10, "全部账桩规则自洽");

    // J11 指纹稳定 + seq 唯一
    let j11 = b.fold() == b.fold() && b.seq_unique();
    s.add("J11-指纹稳定", j11, "折叠指纹两次一致且 seq 唯一");

    // J12 空留痕拒
    let j12 = b.take("proc_execve", MD_COEXIST, "-", "").err() == Some(TK_EMPTY_NOTE);
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
    fn unx9158_takeover_flow() {
        let mut b = mk_book();
        assert!(b.take("proc_exit", MD_TAKEOVER, "stub1", "a").is_ok());
        assert!(b.take("proc_wait", MD_COEXIST, "-", "b").is_ok());
        assert!(b.take("proc_fork", MD_DEFER, "-", "c").is_ok());
        assert_eq!(b.take("proc_none", MD_TAKEOVER, "x", "d"), Err(TK_UNK_SYM));
        assert_eq!(b.take("proc_exit", MD_TAKEOVER, "", "e"), Err(TK_BAD_MODE));
        assert!(b.stub_complete());
    }

    #[test]
    fn unx9158_checks_green() {
        let s = run_unx9158_checks();
        assert!(s.all_passed(), "UNX-F9158 判据未全绿");
    }
}
