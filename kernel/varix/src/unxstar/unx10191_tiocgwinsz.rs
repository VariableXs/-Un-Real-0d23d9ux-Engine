//! UNX-F10191 TIOCGWINSZ 正反双判据（窗口尺寸达成与非法请求误用受控）（0x10b00 段）
//!
//! 锚点：UNX-F10191-J1 尺寸达成（TIOCSWINSZ 置→TIOCGWINSZ 回读一致
//! 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：终端窗册（fd × rows/cols/xpixel/ypixel 四元组）+ 请求分发核
//! （TIOCSWINSZ/TIOCGWINSZ 闭集/野请求码拒）+ 回读账本 + 折叠指纹 +
//! 注入核（野请求码/幽灵 fd/零行列/伪造回读账/印后请求）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10b00 独占 0x20）。
pub const SEG_BASE: u32 = 0x10b00;
pub const TW_NO_FD: u32 = SEG_BASE + 0x01; // 幽灵 fd
pub const TW_BAD_REQ: u32 = SEG_BASE + 0x02; // 野请求码
pub const TW_BAD_SIZE: u32 = SEG_BASE + 0x03; // 零行列
pub const TW_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 留痕为空
pub const TW_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍请求

/// ioctl 请求码闭集。
pub const REQ_SET_WINSZ: u32 = 0x5414; // TIOCSWINSZ
pub const REQ_GET_WINSZ: u32 = 0x5413; // TIOCGWINSZ

/// 行列上界（0x7FFF 与 Linux winsize 域一致）。
pub const RC_MAX: u32 = 0x7FFF;

/// 窗口尺寸四元组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WinSize {
    pub rows: u32,
    pub cols: u32,
    pub xpixel: u32,
    pub ypixel: u32,
}

/// 终端窗条目。
#[derive(Clone, Copy)]
pub struct TtyWin {
    pub fd: i32,
    pub size: WinSize,
    pub open: bool,
}

/// 一次窗口请求的账本留痕。
#[derive(Clone)]
pub struct TwRec {
    pub seq: u32,
    pub fd: i32,
    pub req: u32,
    pub note: String,
}

impl TwRec {
    /// 账目合法性：留痕非空且请求码在闭集。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && (self.req == REQ_SET_WINSZ || self.req == REQ_GET_WINSZ)
    }

    pub fn render_line(&self) -> String {
        format!("W{}|fd{}|req{}|{}", self.seq, self.fd, self.req, self.note)
    }
}

/// 终端窗册：fd 表 + 请求分发核。
pub struct WinTab {
    pub ttys: Vec<TtyWin>,
    pub ledger: Vec<TwRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl WinTab {
    pub fn new() -> WinTab {
        WinTab { ttys: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 开终端（重号必拒；初始 24x80 标准窗）。
    pub fn open_tty(&mut self, fd: i32) -> Result<(), u32> {
        if self.sealed {
            return Err(TW_FROZEN);
        }
        if fd < 0 || self.ttys.iter().any(|t| t.fd == fd) {
            return Err(TW_NO_FD);
        }
        self.ttys.push(TtyWin {
            fd,
            size: WinSize { rows: 24, cols: 80, xpixel: 0, ypixel: 0 },
            open: true,
        });
        Ok(())
    }

    fn idx_of(&self, fd: i32) -> Option<usize> {
        self.ttys.iter().position(|t| t.fd == fd && t.open)
    }

    /// ioctl 分发核：SET 置尺寸 / GET 回读尺寸；野请求码必拒。
    pub fn ioctl(&mut self, fd: i32, req: u32, size: WinSize, note: &str) -> Result<WinSize, u32> {
        if self.sealed {
            return Err(TW_FROZEN);
        }
        if note.is_empty() {
            return Err(TW_EMPTY_NOTE);
        }
        let i = match self.idx_of(fd) {
            Some(i) => i,
            None => return Err(TW_NO_FD),
        };
        match req {
            REQ_SET_WINSZ => {
                if size.rows == 0 || size.cols == 0 || size.rows > RC_MAX || size.cols > RC_MAX {
                    return Err(TW_BAD_SIZE);
                }
                self.ttys[i].size = size;
                self.ledger.push(TwRec { seq: self.seq_next, fd, req, note: note.to_string() });
                self.seq_next += 1;
                Ok(size)
            }
            REQ_GET_WINSZ => {
                let cur = self.ttys[i].size;
                self.ledger.push(TwRec { seq: self.seq_next, fd, req, note: note.to_string() });
                self.seq_next += 1;
                Ok(cur)
            }
            _ => Err(TW_BAD_REQ),
        }
    }

    pub fn close_tty(&mut self, fd: i32) -> Result<(), u32> {
        match self.idx_of(fd) {
            Some(i) => {
                self.ttys[i].open = false;
                Ok(())
            }
            None => Err(TW_NO_FD),
        }
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

    /// 折叠指纹（131 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10b00;
        for t in &self.ttys {
            h = h.wrapping_mul(131).wrapping_add(t.fd as u32);
            h = h.wrapping_mul(131).wrapping_add(t.size.rows);
            h = h.wrapping_mul(131).wrapping_add(t.size.cols);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(131).wrapping_add(r.seq);
            for by in r.note.bytes() {
                h = h.wrapping_mul(131).wrapping_add(by as u32);
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
    let mut t = WinTab::new();
    let _ = t.open_tty(1);
    // 1) 野请求码必拒。
    got[0] = t
        .ioctl(1, 0x7777, WinSize { rows: 24, cols: 80, xpixel: 0, ypixel: 0 }, "inj-req")
        .err()
        == Some(TW_BAD_REQ);
    // 2) 幽灵 fd 必拒。
    got[1] = t
        .ioctl(99, REQ_GET_WINSZ, WinSize { rows: 0, cols: 0, xpixel: 0, ypixel: 0 }, "inj-fd")
        .err()
        == Some(TW_NO_FD);
    // 3) 零行列必拒。
    got[2] = t
        .ioctl(1, REQ_SET_WINSZ, WinSize { rows: 0, cols: 80, xpixel: 0, ypixel: 0 }, "inj-zero")
        .err()
        == Some(TW_BAD_SIZE);
    // 4) 伪造回读账：req 出闭集的账必判非法。
    let forged = TwRec { seq: 1, fd: 1, req: 0x9999, note: "inj-ledger".to_string() };
    got[3] = !forged.ok();
    // 5) 印账后请求必抓。
    t.seal();
    got[4] = t
        .ioctl(1, REQ_GET_WINSZ, WinSize { rows: 0, cols: 0, xpixel: 0, ypixel: 0 }, "inj-frozen")
        .err()
        == Some(TW_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10191_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10191");
    let big = WinSize { rows: 60, cols: 200, xpixel: 800, ypixel: 600 };

    // J1 置→回读一致：SET 后 GET 四元组全等。
    let j1 = {
        let mut t = WinTab::new();
        let _ = t.open_tty(1);
        let _ = t.ioctl(1, REQ_SET_WINSZ, big, "set");
        t.ioctl(1, REQ_GET_WINSZ, WinSize { rows: 0, cols: 0, xpixel: 0, ypixel: 0 }, "get")
            == Ok(big)
    };
    s.add("J1-置读一致", j1, "SET 60x200 后 GET 四元组全等");

    // J2 初始标准窗：24x80 默认值。
    let j2 = {
        let mut t = WinTab::new();
        let _ = t.open_tty(1);
        t.ioctl(1, REQ_GET_WINSZ, WinSize { rows: 0, cols: 0, xpixel: 0, ypixel: 0 }, "init")
            == Ok(WinSize { rows: 24, cols: 80, xpixel: 0, ypixel: 0 })
    };
    s.add("J2-标准初窗", j2, "初始 24x80 标准窗");

    // J3 野请求码拒。
    let j3 = {
        let mut t = WinTab::new();
        let _ = t.open_tty(1);
        t.ioctl(1, 0xDEAD, big, "wild").err() == Some(TW_BAD_REQ)
    };
    s.add("J3-野请求拒", j3, "请求码出闭集必拒");

    // J4 幽灵 fd 拒（含负 fd 与关闭后复用）。
    let j4 = {
        let mut t = WinTab::new();
        let _ = t.open_tty(1);
        let _ = t.close_tty(1);
        t.ioctl(1, REQ_GET_WINSZ, big, "closed").err() == Some(TW_NO_FD)
            && t.open_tty(-1).err() == Some(TW_NO_FD)
    };
    s.add("J4-幽灵 fd 拒", j4, "关闭后与负 fd 皆拒");

    // J5 零行列拒。
    let j5 = {
        let mut t = WinTab::new();
        let _ = t.open_tty(1);
        t.ioctl(1, REQ_SET_WINSZ, WinSize { rows: 0, cols: 0, xpixel: 0, ypixel: 0 }, "zero")
            .err()
            == Some(TW_BAD_SIZE)
    };
    s.add("J5-零行列拒", j5, "rows/cols 皆零必拒");

    // J6 多终端隔离：两 tty 各置各读互不串。
    let j6 = {
        let mut t = WinTab::new();
        let _ = t.open_tty(1);
        let _ = t.open_tty(2);
        let _ = t.ioctl(1, REQ_SET_WINSZ, big, "t1");
        let g2 = t.ioctl(2, REQ_GET_WINSZ, big, "t2");
        g2 == Ok(WinSize { rows: 24, cols: 80, xpixel: 0, ypixel: 0 })
    };
    s.add("J6-多端隔离", j6, "tty2 仍持标准窗不串");

    // J7 账本完整：SET/GET 混排全合法。
    let j7 = {
        let mut t = WinTab::new();
        let _ = t.open_tty(1);
        let _ = t.ioctl(1, REQ_SET_WINSZ, big, "a");
        let _ = t.ioctl(1, REQ_GET_WINSZ, big, "b");
        t.ledger.len() == 2 && t.ledger_ok() && t.seq_unique()
    };
    s.add("J7-账本完整", j7, "双请求账合法无重号");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut t = WinTab::new();
        let _ = t.open_tty(1);
        let _ = t.ioctl(1, REQ_SET_WINSZ, big, "a");
        let f1 = t.fold();
        let f2 = t.fold();
        t.seal();
        f1 == f2 && t.ioctl(1, REQ_GET_WINSZ, big, "post").err() == Some(TW_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    // J10 上界含边：RC_MAX 恰好可置。
    let j10 = {
        let mut t = WinTab::new();
        let _ = t.open_tty(1);
        let edge = WinSize { rows: RC_MAX, cols: RC_MAX, xpixel: 0, ypixel: 0 };
        t.ioctl(1, REQ_SET_WINSZ, edge, "edge").is_ok()
    };
    s.add("J10-上界含边", j10, "0x7FFF 恰好收下");

    // J11 象素位守恒：置象素后回读不丢。
    let j11 = {
        let mut t = WinTab::new();
        let _ = t.open_tty(1);
        let px = WinSize { rows: 30, cols: 100, xpixel: 1920, ypixel: 1080 };
        let _ = t.ioctl(1, REQ_SET_WINSZ, px, "px");
        t.ioctl(1, REQ_GET_WINSZ, px, "pxr") == Ok(px)
    };
    s.add("J11-象素守恒", j11, "xpixel/ypixel 回读不丢");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10191_winsz_flow() {
        let mut t = WinTab::new();
        let _ = t.open_tty(1);
        let w = WinSize { rows: 50, cols: 120, xpixel: 0, ypixel: 0 };
        assert_eq!(t.ioctl(1, REQ_SET_WINSZ, w, "s"), Ok(w));
        assert_eq!(t.ioctl(1, REQ_GET_WINSZ, w, "g"), Ok(w));
        assert_eq!(t.ioctl(1, 0x1, w, "x"), Err(TW_BAD_REQ));
    }

    #[test]
    fn unx10191_checks_green() {
        let s = run_unx10191_checks();
        assert!(s.all_passed(), "UNX-F10191 判据未全绿");
    }
}
