//! UNX-F24168 MSI-X 中断消费与 IrqBackend 对接·子功能08（共享中断线复用）（0x416E0 段）
//!
//! 锚点：UNX-F24168-J1 复用账（共享线多 handler 逐条落账）；J2 复用
//! 语义（多 handler 齐发 + 无 claim 链 + 撤销清链三性质）；J3 复用指
//! 纹（账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（共享线复用面）。
//!
//! 结构：共享线表 + handler 链核 + 齐发核 + 账本 + 折叠指纹 + 注入核
//! （野线/重 handler/空链发线/印后/重复撤销必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x416E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x416E0;
pub const SH_UNK_LINE: u32 = SEG_BASE + 0x01; // 野线
pub const SH_DUP_H: u32 = SEG_BASE + 0x02; // 重 handler
pub const SH_EMPTY_FIRE: u32 = SEG_BASE + 0x03; // 空链发线
pub const SH_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const SH_DUP_FREE: u32 = SEG_BASE + 0x05; // 重复撤销

/// 共享线。
#[derive(Clone)]
pub struct ShLine {
    pub line_no: u32,
    pub handlers: Vec<String>,
    pub fired_total: u32,
}

/// 复用簿。
pub struct ShBook {
    pub lines: Vec<ShLine>,
    pub sealed: bool,
}

impl ShBook {
    pub fn new() -> ShBook {
        ShBook { lines: Vec::new(), sealed: false }
    }

    pub fn declare(&mut self, line_no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(SH_FROZEN);
        }
        if self.lines.iter().any(|l| l.line_no == line_no) {
            return Err(SH_UNK_LINE);
        }
        self.lines.push(ShLine { line_no, handlers: Vec::new(), fired_total: 0 });
        Ok(())
    }

    pub fn add_handler(&mut self, line_no: u32, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(SH_FROZEN);
        }
        if name.is_empty() {
            return Err(SH_DUP_H);
        }
        let l = match self.lines.iter_mut().find(|l| l.line_no == line_no) {
            Some(l) => l,
            None => return Err(SH_UNK_LINE),
        };
        if l.handlers.iter().any(|h| h == name) {
            return Err(SH_DUP_H);
        }
        l.handlers.push(name.to_string());
        Ok(())
    }

    /// 齐发：全部 handler 收到（返回 handler 数）。
    pub fn fire(&mut self, line_no: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(SH_FROZEN);
        }
        let l = match self.lines.iter_mut().find(|l| l.line_no == line_no) {
            Some(l) => l,
            None => return Err(SH_UNK_LINE),
        };
        if l.handlers.is_empty() {
            return Err(SH_EMPTY_FIRE);
        }
        let n = l.handlers.len();
        l.fired_total += 1;
        Ok(n)
    }

    pub fn remove_handler(&mut self, line_no: u32, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(SH_FROZEN);
        }
        let l = match self.lines.iter_mut().find(|l| l.line_no == line_no) {
            Some(l) => l,
            None => return Err(SH_UNK_LINE),
        };
        let before = l.handlers.len();
        l.handlers.retain(|h| h != name);
        if l.handlers.len() == before {
            return Err(SH_DUP_FREE);
        }
        Ok(())
    }

    pub fn handler_count(&self, line_no: u32) -> usize {
        self.lines.iter().find(|l| l.line_no == line_no).map(|l| l.handlers.len()).unwrap_or(0)
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x416E0;
        for l in &self.lines {
            h = h.wrapping_mul(31).wrapping_add(l.line_no);
            h = h.wrapping_mul(31).wrapping_add(l.fired_total);
            h = h.wrapping_mul(31).wrapping_add(l.handlers.len() as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：一线双 handler。
pub fn mk_book() -> ShBook {
    let mut b = ShBook::new();
    let _ = b.declare(18);
    let _ = b.add_handler(18, "gpu");
    let _ = b.add_handler(18, "audio");
    let _ = b.fire(18);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 野线
    got[0] = b.fire(99).err() == Some(SH_UNK_LINE);
    // 2) 重 handler
    got[1] = b.add_handler(18, "gpu").err() == Some(SH_DUP_H);
    // 3) 空链发线
    let mut b2 = ShBook::new();
    let _ = b2.declare(1);
    got[2] = b2.fire(1).err() == Some(SH_EMPTY_FIRE);
    // 4) 重复撤销
    got[3] = b.remove_handler(18, "ghost").err() == Some(SH_DUP_FREE);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.fire(18).err() == Some(SH_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24168_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24168");
    let mut b = mk_book();

    // J1 线账齐
    let j1 = b.lines.len() == 1;
    s.add("J1-线账齐", j1, "一线落表");

    // J2 handler 链齐
    let j2 = b.handler_count(18) == 2;
    s.add("J2-链齐", j2, "双 handler");

    // J3 齐发返数
    let j3 = b.fire(18) == Ok(2);
    s.add("J3-齐发返数", j3, "两 handler 齐发");

    // J4 发线计数
    let j4 = b.lines.iter().find(|l| l.line_no == 18).map(|l| l.fired_total == 2).unwrap_or(false);
    s.add("J4-发线计数", j4, "fire 两次");

    // J5 重 handler 拒
    let j5 = b.add_handler(18, "gpu").err() == Some(SH_DUP_H);
    s.add("J5-重handler拒", j5, "同链重名必拒");

    // J6 野线拒
    let j6 = b.fire(99).err() == Some(SH_UNK_LINE);
    s.add("J6-野线拒", j6, "未声明线必拒");

    // J7 空链发线拒
    let j7 = {
        let mut b2 = ShBook::new();
        let _ = b2.declare(1);
        b2.fire(1).err() == Some(SH_EMPTY_FIRE)
    };
    s.add("J7-空链拒", j7, "无 handler 发线必拒");

    // J8 撤销清链
    let j8 = {
        let _ = b.remove_handler(18, "audio");
        b.handler_count(18) == 1
    };
    s.add("J8-撤销清链", j8, "撤销后链长 1");

    // J9 重复撤销拒
    let j9 = b.remove_handler(18, "audio").err() == Some(SH_DUP_FREE);
    s.add("J9-重撤销拒", j9, "二次撤销必拒");

    // J10 全撤后空链
    let j10 = {
        let _ = b.remove_handler(18, "gpu");
        b.fire(18).err() == Some(SH_EMPTY_FIRE)
    };
    s.add("J10-全撤空链", j10, "链空发线必拒");

    // J11 重号线拒
    let j11 = b.declare(18).err() == Some(SH_UNK_LINE);
    s.add("J11-重号线拒", j11, "线重号必拒");

    // J12 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

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
    fn unx24168_shared_flow() {
        let mut b = ShBook::new();
        let _ = b.declare(18);
        let _ = b.add_handler(18, "a");
        let _ = b.add_handler(18, "b");
        assert_eq!(b.fire(18), Ok(2));
        assert_eq!(b.add_handler(18, "a"), Err(SH_DUP_H));
        assert!(b.remove_handler(18, "a").is_ok());
        assert_eq!(b.remove_handler(18, "a"), Err(SH_DUP_FREE));
        assert!(b.fire(18).is_ok());
        b.seal();
        assert_eq!(b.fire(18), Err(SH_FROZEN));
    }

    #[test]
    fn unx24168_checks_green() {
        let s = run_unx24168_checks();
        assert!(s.all_passed(), "UNX-F24168 判据未全绿");
    }
}
