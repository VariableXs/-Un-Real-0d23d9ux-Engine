//! UNX-F10194 pty 非阻塞 IO 正反双判据（非阻塞达成与误用受控）（0x10b60 段）
//!
//! 锚点：UNX-F10194-J1 非阻塞达成（O_NONBLOCK 空读立即 EAGAIN 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：非阻塞管（缓冲 × O_NONBLOCK 位 × 阻塞计数）+ 读核（空流
//! 双面：非阻塞 EAGAIN 立返 / 阻塞面计数挂起）+ 标志翻转核 +
//! 读写账本 + 注入核（阻塞面误报 EAGAIN/幽灵 fd/空留痕/伪造账/印后）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10b60 独占 0x20）。
pub const SEG_BASE: u32 = 0x10b60;
pub const NB_EAGAIN: u32 = SEG_BASE + 0x01; // 非阻塞空读立返
pub const NB_WOULD_BLOCK: u32 = SEG_BASE + 0x02; // 阻塞面挂起
pub const NB_GHOST: u32 = SEG_BASE + 0x03; // 幽灵 fd
pub const NB_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 留痕为空
pub const NB_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍读写

/// 非阻塞管：单端缓冲 + 标志位。
pub struct NbPipe {
    pub fd: i32,
    pub nonblock: bool,
    pub buf: Vec<u8>,
    /// 阻塞面挂起计数（阻塞读遇空的次数）。
    pub blocked: u32,
    /// 非阻塞空读计数（EAGAIN 次数）。
    pub agains: u32,
    pub ledger: Vec<NbRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

/// 一次读操作的账本留痕。
#[derive(Clone)]
pub struct NbRec {
    pub seq: u32,
    pub nonblock: bool,
    /// true=读到数据 / false=空流（EAGAIN 或挂起）。
    pub got: bool,
    pub n: u32,
    pub note: String,
}

impl NbRec {
    /// 账目合法性：留痕非空；空流账不得记正读量。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && (self.got || self.n == 0)
    }

    pub fn render_line(&self) -> String {
        format!(
            "N{}|nb{}|got{}|n{}|{}",
            self.seq, self.nonblock as u8, self.got as u8, self.n, self.note
        )
    }
}

impl NbPipe {
    pub fn new(fd: i32, nonblock: bool) -> NbPipe {
        NbPipe {
            fd,
            nonblock,
            buf: Vec::new(),
            blocked: 0,
            agains: 0,
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// 写端灌数据（空数据必拒）。
    pub fn write(&mut self, data: &[u8]) -> Result<usize, u32> {
        if self.sealed {
            return Err(NB_FROZEN);
        }
        if data.is_empty() {
            return Err(NB_EMPTY_NOTE);
        }
        for b in data {
            self.buf.push(*b);
        }
        Ok(data.len())
    }

    /// 读核：空流双面——非阻塞 EAGAIN 立返；阻塞面挂起计数。
    pub fn read(&mut self, n: usize, note: &str) -> Result<Vec<u8>, u32> {
        if self.sealed {
            return Err(NB_FROZEN);
        }
        if note.is_empty() {
            return Err(NB_EMPTY_NOTE);
        }
        if self.buf.is_empty() {
            let (got, code) = if self.nonblock {
                self.agains += 1;
                (false, NB_EAGAIN)
            } else {
                self.blocked += 1;
                (false, NB_WOULD_BLOCK)
            };
            self.ledger.push(NbRec {
                seq: self.seq_next,
                nonblock: self.nonblock,
                got,
                n: 0,
                note: note.to_string(),
            });
            self.seq_next += 1;
            return Err(code);
        }
        let take = n.min(self.buf.len());
        let mut out = Vec::new();
        let mut i = 0;
        while i < take {
            out.push(self.buf.remove(0));
            i += 1;
        }
        self.ledger.push(NbRec {
            seq: self.seq_next,
            nonblock: self.nonblock,
            got: true,
            n: out.len() as u32,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(out)
    }

    /// O_NONBLOCK 标志翻转（返回旧值）。
    pub fn set_nonblock(&mut self, on: bool) -> bool {
        let old = self.nonblock;
        self.nonblock = on;
        old
    }

    /// 无误报审计：阻塞面绝不产生 EAGAIN（again 计数只在非阻塞期增长）。
    pub fn no_false_again(&self) -> bool {
        // 审计面：账内非阻塞空读与 agains 计数一致。
        let nb_again = self.ledger.iter().filter(|r| r.nonblock && !r.got).count() as u32;
        nb_again == self.agains
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

    /// 折叠指纹（149 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10b60;
        for r in &self.ledger {
            h = h.wrapping_mul(149).wrapping_add(r.seq);
            h = h.wrapping_mul(149).wrapping_add(r.n);
            h = h.wrapping_mul(149).wrapping_add(r.got as u32);
            for by in r.note.bytes() {
                h = h.wrapping_mul(149).wrapping_add(by as u32);
            }
        }
        h = h.wrapping_mul(149).wrapping_add(self.agains);
        h = h.wrapping_mul(149).wrapping_add(self.blocked);
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
    // 1) 非阻塞空读立即 EAGAIN。
    let mut p = NbPipe::new(1, true);
    got[0] = p.read(4, "nb-empty").err() == Some(NB_EAGAIN);
    // 2) 阻塞面挂起（非 EAGAIN）。
    let mut q = NbPipe::new(2, false);
    got[1] = q.read(4, "b-empty").err() == Some(NB_WOULD_BLOCK);
    // 3) 伪造账：空流账记正读量必判非法。
    let forged = NbRec {
        seq: 1,
        nonblock: true,
        got: false,
        n: 5,
        note: "inj-n".to_string(),
    };
    got[2] = !forged.ok();
    // 4) 空写必拒。
    got[3] = p.write(b"").err() == Some(NB_EMPTY_NOTE);
    // 5) 印账后读必抓。
    p.seal();
    got[4] = p.read(1, "inj-frozen").err() == Some(NB_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10194_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10194");

    // J1 非阻塞达成：O_NONBLOCK 空读立即 EAGAIN。
    let j1 = {
        let mut p = NbPipe::new(1, true);
        p.read(8, "a").err() == Some(NB_EAGAIN) && p.agains == 1
    };
    s.add("J1-EAGAIN 立返", j1, "空流非阻塞读立即 EAGAIN");

    // J2 阻塞面受控：空流挂起计数、错误码相异。
    let j2 = {
        let mut p = NbPipe::new(1, false);
        let e = p.read(8, "b");
        e == Err(NB_WOULD_BLOCK) && p.blocked == 1 && p.agains == 0
    };
    s.add("J2-阻塞面受控", j2, "阻塞挂起计数与 EAGAIN 分账");

    // J3 有数据即读：非阻塞不误伤有数据读。
    let j3 = {
        let mut p = NbPipe::new(1, true);
        let _ = p.write(b"abcd");
        p.read(4, "c") == Ok(b"abcd".to_vec())
    };
    s.add("J3-有数据即读", j3, "非阻塞有数据读不误报");

    // J4 标志翻转：set_nonblock 回旧值且语义随翻。
    let j4 = {
        let mut p = NbPipe::new(1, true);
        let old = p.set_nonblock(false);
        let e1 = p.read(1, "r1");
        let old2 = p.set_nonblock(true);
        let e2 = p.read(1, "r2");
        // set_nonblock 返回旧值：初建非阻塞 → 翻假得 true；再翻真得 false。
        old && !old2 && e1 == Err(NB_WOULD_BLOCK) && e2 == Err(NB_EAGAIN)
    };
    s.add("J4-标志翻转", j4, "翻位后双面错误码随位切换");

    // J5 无误报审计：again 计数与账面一致（阻塞面零误报）。
    let j5 = {
        let mut p = NbPipe::new(1, false);
        let _ = p.read(1, "b1");
        let _ = p.set_nonblock(true);
        let _ = p.read(1, "n1");
        p.no_false_again() && p.agains == 1 && p.blocked == 1
    };
    s.add("J5-无误报审计", j5, "双面计数与账面逐一对平");

    // J6 部分读受控：请求超量按余量交付。
    let j6 = {
        let mut p = NbPipe::new(1, true);
        let _ = p.write(b"xy");
        p.read(10, "part") == Ok(b"xy".to_vec())
    };
    s.add("J6-部分读", j6, "超量请求按余量交付");

    // J7 账本完整：空流账 n=0 合法。
    let j7 = {
        let mut p = NbPipe::new(1, true);
        let _ = p.read(1, "e");
        let _ = p.write(b"a");
        let _ = p.read(1, "g");
        p.ledger.len() == 2 && p.ledger_ok() && p.seq_unique()
    };
    s.add("J7-账本完整", j7, "空流账 n=0 合法无重号");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut p = NbPipe::new(1, true);
        let _ = p.read(1, "a");
        let f1 = p.fold();
        let f2 = p.fold();
        p.seal();
        f1 == f2 && p.read(1, "post").err() == Some(NB_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    // J10 连续空读计数：多次 EAGAIN 逐次累加。
    let j10 = {
        let mut p = NbPipe::new(1, true);
        let _ = p.read(1, "a");
        let _ = p.read(1, "b");
        let _ = p.read(1, "c");
        p.agains == 3 && p.blocked == 0
    };
    s.add("J10-连空计数", j10, "三连空读 agains=3");

    // J11 阻塞面连挂计数：多次挂起逐次累加且零 EAGAIN。
    let j11 = {
        let mut p = NbPipe::new(1, false);
        let _ = p.read(1, "a");
        let _ = p.read(1, "b");
        p.blocked == 2 && p.agains == 0 && p.no_false_again()
    };
    s.add("J11-连挂计数", j11, "双连挂起 blocked=2 零误报");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10194_nb_flow() {
        let mut p = NbPipe::new(3, true);
        assert_eq!(p.read(1, "e"), Err(NB_EAGAIN));
        let _ = p.write(b"hi");
        assert_eq!(p.read(1, "g"), Ok(b"h".to_vec()));
        assert_eq!(p.read(1, "g"), Ok(b"i".to_vec()));
        assert_eq!(p.read(1, "e2"), Err(NB_EAGAIN));
        assert!(p.no_false_again());
    }

    #[test]
    fn unx10194_checks_green() {
        let s = run_unx10194_checks();
        assert!(s.all_passed(), "UNX-F10194 判据未全绿");
    }
}
