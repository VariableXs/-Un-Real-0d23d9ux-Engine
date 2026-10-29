//! UNX-F10193 termios 行规程正反双判据（回显达成与非法参数误用受控）（0x10b40 段）
//!
//! 锚点：UNX-F10193-J1 行规程达成（回显/规范模式语义 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：行规程（ECHO 位 × ICANON 位 × 待行缓冲 × 成行队列 × 回显流）
//! + 输入核（逐字符喂入、回车成行）+ 参数核（波特率闭集）+
//! 回显账本 + 注入核（回显谎报/野波特率/空行误用/空留痕/印后喂入）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x10b40 独占 0x20）。
pub const SEG_BASE: u32 = 0x10b40;
pub const TD_BAD_BAUD: u32 = SEG_BASE + 0x01; // 野波特率
pub const TD_NO_LINE: u32 = SEG_BASE + 0x02; // 规范模式无成行可读
pub const TD_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 留痕为空
pub const TD_ECHO_LIE: u32 = SEG_BASE + 0x04; // 回显谎报
pub const TD_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍喂入

/// 波特率闭集。
pub const BAUD_9600: u32 = 9600;
pub const BAUD_115200: u32 = 115200;

/// 回显/成行的账本留痕。
#[derive(Clone)]
pub struct TdRec {
    pub seq: u32,
    pub fed: u32,
    pub lines: u32,
    pub echoed: u32,
    pub note: String,
}

impl TdRec {
    /// 账目合法性：留痕非空；回显量不得超喂入量（回显谎报面）。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.echoed <= self.fed
    }

    pub fn render_line(&self) -> String {
        format!(
            "D{}|fed{}|ln{}|ec{}|{}",
            self.seq, self.fed, self.lines, self.echoed, self.note
        )
    }
}

/// 行规程：ECHO/ICANON 双位 + 缓冲。
pub struct LineDisc {
    pub echo: bool,
    pub canon: bool,
    pub baud: u32,
    /// 规范模式待成行缓冲。
    pub pend: Vec<u8>,
    /// 成行队列。
    pub lines: Vec<Vec<u8>>,
    /// 回显流。
    pub out: Vec<u8>,
    pub fed: u32,
    pub ledger: Vec<TdRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl LineDisc {
    pub fn new() -> LineDisc {
        LineDisc {
            echo: true,
            canon: true,
            baud: BAUD_9600,
            pend: Vec::new(),
            lines: Vec::new(),
            out: Vec::new(),
            fed: 0,
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// set_attr：ECHO/ICANON/波特率（野波特率必拒）。
    pub fn set_attr(&mut self, echo: bool, canon: bool, baud: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(TD_FROZEN);
        }
        if baud != BAUD_9600 && baud != BAUD_115200 {
            return Err(TD_BAD_BAUD);
        }
        self.echo = echo;
        self.canon = canon;
        self.baud = baud;
        Ok(())
    }

    /// 输入核：逐字符喂入；规范模式回车成行，非规范即时成行。
    pub fn feed(&mut self, c: u8, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(TD_FROZEN);
        }
        if note.is_empty() {
            return Err(TD_EMPTY_NOTE);
        }
        self.fed += 1;
        if self.echo {
            self.out.push(c);
        }
        if self.canon {
            if c == b'\r' || c == b'\n' {
                let line = core::mem::take(&mut self.pend);
                self.lines.push(line);
            } else {
                self.pend.push(c);
            }
        } else {
            self.lines.push(vec![c]);
        }
        Ok(())
    }

    /// 读一行：规范模式无成行可读按 TD_NO_LINE 受控拒。
    pub fn read_line(&mut self) -> Result<Vec<u8>, u32> {
        if self.lines.is_empty() {
            return Err(TD_NO_LINE);
        }
        Ok(self.lines.remove(0))
    }

    /// 回显审计：echo 开则回显量==喂入量；echo 关则回显流必空。
    pub fn echo_audit(&self) -> bool {
        if self.echo {
            self.out.len() as u32 == self.fed
        } else {
            self.out.is_empty()
        }
    }

    /// 会话落账：喂入/成行/回显三量快照。
    pub fn snapshot(&mut self, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(TD_FROZEN);
        }
        if note.is_empty() {
            return Err(TD_EMPTY_NOTE);
        }
        self.ledger.push(TdRec {
            seq: self.seq_next,
            fed: self.fed,
            lines: self.lines.len() as u32,
            echoed: self.out.len() as u32,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(())
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

    /// 折叠指纹（139 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10b40;
        for r in &self.ledger {
            h = h.wrapping_mul(139).wrapping_add(r.fed);
            h = h.wrapping_mul(139).wrapping_add(r.lines);
            for by in r.note.bytes() {
                h = h.wrapping_mul(139).wrapping_add(by as u32);
            }
        }
        h = h.wrapping_mul(139).wrapping_add(self.baud);
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
    let mut d = LineDisc::new();
    // 1) 野波特率必拒。
    got[0] = d.set_attr(true, true, 2400).err() == Some(TD_BAD_BAUD);
    // 2) 规范模式无成行读必拒。
    got[1] = d.read_line().err() == Some(TD_NO_LINE);
    // 3) 回显谎报：echo 关却称有回显量（伪造账）必判非法。
    let forged = TdRec {
        seq: 1,
        fed: 0,
        lines: 0,
        echoed: 5, // fed=0 却报 5 回显
        note: "inj-echo".to_string(),
    };
    got[2] = !forged.ok();
    // 4) echo 关审计：喂入不回显。
    let _ = d.set_attr(false, true, BAUD_9600);
    let _ = d.feed(b'a', "noecho");
    got[3] = d.out.is_empty() && d.echo_audit();
    // 5) 印账后喂入必抓。
    d.seal();
    got[4] = d.feed(b'x', "inj-frozen").err() == Some(TD_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10193_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10193");

    // J1 规范模式成行："abc\r" → 行 "abc"。
    let j1 = {
        let mut d = LineDisc::new();
        for c in b"abc\r" {
            let _ = d.feed(*c, "in");
        }
        d.read_line() == Ok(b"abc".to_vec())
    };
    s.add("J1-规范成行", j1, "abc 回车成行精确");

    // J2 回显达成：echo 开时回显流与输入逐字节一致。
    let j2 = {
        let mut d = LineDisc::new();
        let _ = d.set_attr(true, true, BAUD_115200);
        for c in b"hi\r" {
            let _ = d.feed(*c, "in");
        }
        d.out == b"hi\r".to_vec() && d.echo_audit()
    };
    s.add("J2-回显达成", j2, "三字符回显逐字节一致");

    // J3 回显关闭受控：echo 关零回显。
    let j3 = {
        let mut d = LineDisc::new();
        let _ = d.set_attr(false, true, BAUD_9600);
        for c in b"xyz\r" {
            let _ = d.feed(*c, "in");
        }
        d.out.is_empty() && d.read_line() == Ok(b"xyz".to_vec())
    };
    s.add("J3-回显关闭", j3, "关回显仍成行不回显");

    // J4 非规范模式即时交付：每字符即一行。
    let j4 = {
        let mut d = LineDisc::new();
        let _ = d.set_attr(true, false, BAUD_9600);
        let _ = d.feed(b'a', "raw");
        let _ = d.feed(b'b', "raw");
        d.read_line() == Ok(b"a".to_vec()) && d.read_line() == Ok(b"b".to_vec())
    };
    s.add("J4-非规范即时", j4, "raw 模式逐字符成行");

    // J5 空行受控：连续回车成空行仍可读。
    let j5 = {
        let mut d = LineDisc::new();
        let _ = d.feed(b'\r', "cr");
        d.read_line() == Ok(Vec::new())
    };
    s.add("J5-空行受控", j5, "裸回车成空行");

    // J6 野波特率拒。
    let j6 = {
        let mut d = LineDisc::new();
        d.set_attr(true, true, 7).err() == Some(TD_BAD_BAUD)
    };
    s.add("J6-野波特率拒", j6, "波特率出闭集必拒");

    // J7 账本完整：快照账合法且量守恒。
    let j7 = {
        let mut d = LineDisc::new();
        for c in b"ab\r" {
            let _ = d.feed(*c, "in");
        }
        let _ = d.snapshot("snap");
        let r = &d.ledger[0];
        d.ledger_ok() && d.seq_unique() && r.fed == 3 && r.lines == 1 && r.echoed == 3
    };
    s.add("J7-账本守恒", j7, "fed/lines/echoed 三量对平");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut d = LineDisc::new();
        let _ = d.feed(b'q', "in");
        let f1 = d.fold();
        let f2 = d.fold();
        d.seal();
        f1 == f2 && d.feed(b'z', "post").err() == Some(TD_FROZEN)
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
    fn unx10193_termios_flow() {
        let mut d = LineDisc::new();
        for c in b"ok\r" {
            assert!(d.feed(*c, "in").is_ok());
        }
        assert_eq!(d.read_line(), Ok(b"ok".to_vec()));
        assert_eq!(d.read_line(), Err(TD_NO_LINE));
        assert_eq!(d.set_attr(true, true, 50), Err(TD_BAD_BAUD));
    }

    #[test]
    fn unx10193_checks_green() {
        let s = run_unx10193_checks();
        assert!(s.all_passed(), "UNX-F10193 判据未全绿");
    }
}
