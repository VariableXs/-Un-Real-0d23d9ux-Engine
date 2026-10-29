//! UNX-F10895 ERR 事件与 SO_ERROR 时序矩阵（0x1c860 段）
//! 锚点：UNX-F10895-J1 ERR 事件与 SO_ERROR 时序（事件先/读后清/双读一致性）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c860;
pub const E_EAGAIN: u32 = SEG_BASE + 1;
pub const E_EBADF: u32 = SEG_BASE + 2;
pub const E_EINVAL: u32 = SEG_BASE + 3;
pub const E_OK: u32 = SEG_BASE + 4;
pub const E_SEALED: u32 = SEG_BASE + 5;

/// 就绪事件：ERR / IN / OUT
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ev {
    Err,
    In,
    Out,
}

/// ERR 事件时序机：事件触发 → 首次 SO_ERROR 取码清挂 → 二次读 None
pub struct ErrTimeline {
    armed: Option<Ev>,
    err_code: Option<u32>,
    err_reads: usize,
    open: bool,
    sealed: bool,
}

impl ErrTimeline {
    pub fn new() -> Self {
        ErrTimeline { armed: None, err_code: None, err_reads: 0, open: true, sealed: false }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    /// 内核触发 ERR 事件并挂错误码
    pub fn raise_err(&mut self, code: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if !self.open {
            return Err(E_EBADF);
        }
        if self.armed.is_some() {
            return Err(E_EINVAL);
        }
        self.armed = Some(Ev::Err);
        self.err_code = Some(code);
        Ok(())
    }

    pub fn raise_plain(&mut self, ev: Ev) -> Result<(), u32> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if !self.open {
            return Err(E_EBADF);
        }
        if self.armed.is_some() {
            return Err(E_EINVAL);
        }
        self.armed = Some(ev);
        Ok(())
    }

    pub fn armed_ev(&self) -> Option<Ev> {
        self.armed
    }

    /// SO_ERROR：取码一次清挂；事件与码同时清
    pub fn so_error(&mut self) -> Result<Option<u32>, u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        self.err_reads += 1;
        let c = self.err_code;
        if self.err_code.is_some() {
            self.err_code = None;
            self.armed = None;
        }
        Ok(c)
    }

    pub fn err_reads(&self) -> usize {
        self.err_reads
    }

    /// 事件与码一致性：ERR 事件必挂码，普通事件必无码
    pub fn consistent(&self) -> bool {
        match self.armed {
            Some(Ev::Err) => self.err_code.is_some(),
            Some(_) => self.err_code.is_none(),
            None => self.err_code.is_none(),
        }
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

pub fn run_unx10895_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10895");
    s.add("J1-段锚", SEG_BASE == 0x1c860, "ERR 时序段基址 0x1c860");
    s.add("J2-触发挂码", {
        let mut t = ErrTimeline::new();
        t.raise_err(104).unwrap();
        t.armed_ev() == Some(Ev::Err) && t.consistent()
    }, "ERR 事件挂 104 一致");
    s.add("J3-首读取码", {
        let mut t = ErrTimeline::new();
        t.raise_err(111).unwrap();
        t.so_error() == Ok(Some(111))
    }, "首读 Some(111)");
    s.add("J4-读后清挂", {
        let mut t = ErrTimeline::new();
        t.raise_err(111).unwrap();
        t.so_error().unwrap();
        t.armed_ev().is_none() && t.so_error() == Ok(None)
    }, "读后事件清二次 None");
    s.add("J5-双读一致账", {
        let mut t = ErrTimeline::new();
        t.raise_err(103).unwrap();
        let a = t.so_error();
        let b = t.so_error();
        a != b && a == Ok(Some(103)) && b == Ok(None)
    }, "首 Some 次 None");
    s.add("J6-读数计", {
        let mut t = ErrTimeline::new();
        t.raise_err(1).unwrap();
        t.so_error().unwrap();
        t.so_error().unwrap();
        t.so_error().unwrap();
        t.err_reads() == 3
    }, "三读计 3");
    s.add("J7-普通事件无码", {
        let mut t = ErrTimeline::new();
        t.raise_plain(Ev::In).unwrap();
        t.armed_ev() == Some(Ev::In) && t.so_error() == Ok(None) && t.consistent()
    }, "In 事件窥错 None 一致");
    s.add("J8-普通事件不因读清", {
        let mut t = ErrTimeline::new();
        t.raise_plain(Ev::Out).unwrap();
        t.so_error().unwrap();
        t.armed_ev() == Some(Ev::Out)
    }, "Out 事件读后仍在");
    s.add("J9-一致性恒真", {
        let mut a = ErrTimeline::new();
        let mut b = ErrTimeline::new();
        let mut c = ErrTimeline::new();
        let mut d = ErrTimeline::new();
        a.raise_plain(Ev::In).unwrap();
        b.raise_err(9).unwrap();
        c.raise_err(9).unwrap();
        c.so_error().unwrap();
        a.consistent() && b.consistent() && c.consistent() && d.consistent()
    }, "四态一致性全真");
    s.add("J10-重复触发拒", {
        let mut t = ErrTimeline::new();
        t.raise_err(1).unwrap();
        t.raise_err(2) == Err(E_EINVAL)
    }, "重触发 E_EINVAL");
    s.add("J11-错后普通触发拒", {
        let mut t = ErrTimeline::new();
        t.raise_err(1).unwrap();
        t.raise_plain(Ev::In) == Err(E_EINVAL)
    }, "挂起期新事件 E_EINVAL");
    s.add("J12-清挂后可再触发", {
        let mut t = ErrTimeline::new();
        t.raise_err(1).unwrap();
        t.so_error().unwrap();
        t.raise_err(2).is_ok()
    }, "清挂后再挂 Ok");
    s.add("J13-关后触发拒", {
        let mut t = ErrTimeline::new();
        t.close();
        t.raise_err(1) == Err(E_EBADF)
    }, "关后触发 E_EBADF");
    s.add("J14-关后读拒", {
        let mut t = ErrTimeline::new();
        t.raise_err(1).unwrap();
        t.close();
        t.so_error() == Err(E_EBADF)
    }, "关后读 E_EBADF");
    s.add("J15-封账拒触发", {
        let mut t = ErrTimeline::new();
        t.seal();
        t.raise_err(1) == Err(E_SEALED)
    }, "封账 E_SEALED");
    s.add_dyn("J16-全序账", {
        let mut t = ErrTimeline::new();
        let r0 = t.so_error();
        t.raise_err(E_ECONNRESET_HINT).unwrap();
        let r1 = t.so_error();
        let r2 = t.so_error();
        r0 == Ok(None) && r1 != Ok(None) && r2 == Ok(None)
    }, "None→Some→None 全序");
    s
}

const E_ECONNRESET_HINT: u32 = 104;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_err_timeline() {
        let mut t = ErrTimeline::new();
        assert_eq!(t.so_error(), Ok(None));
        t.raise_err(104).unwrap();
        assert_eq!(t.so_error(), Ok(Some(104)));
        assert_eq!(t.so_error(), Ok(None));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10895_checks();
        assert!(s.all_passed(), "UNX-F10895 判据全绿");
    }
}
