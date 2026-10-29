//! UNX-F10891 accept 与 setsockopt 竞态矩阵（0x1c7e0 段）
//! 锚点：UNX-F10891-J1 accept/setsockopt 竞态（选项继承/迟到设置两序判定）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c7e0;
pub const E_EINVAL: u32 = SEG_BASE + 1;
pub const E_ENOENT: u32 = SEG_BASE + 2;
pub const E_EAGAIN: u32 = SEG_BASE + 3;
pub const E_OK: u32 = SEG_BASE + 4;
pub const E_SEALED: u32 = SEG_BASE + 5;

/// 选项：非阻塞 / PASSCRED（布尔面足矣）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SockOpts {
    pub nonblock: bool,
    pub passcred: bool,
}

impl SockOpts {
    pub fn plain() -> Self {
        SockOpts { nonblock: false, passcred: false }
    }
}

/// 监听套接字：选项 + 待连队列
pub struct OptListener {
    opts: SockOpts,
    pending: usize,
    accepted: Vec<SockOpts>,
    sealed: bool,
}

impl OptListener {
    pub fn new(opts: SockOpts) -> Self {
        OptListener { opts, pending: 0, accepted: Vec::new(), sealed: false }
    }

    pub fn opts_of(&self) -> SockOpts {
        self.opts
    }

    pub fn set_opts(&mut self, o: SockOpts) -> Result<(), u32> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.opts = o;
        Ok(())
    }

    pub fn arrival(&mut self) {
        self.pending += 1;
    }

    pub fn pending_of(&self) -> usize {
        self.pending
    }

    /// accept：无待连 EAGAIN；有则新 fd 继承当刻监听选项
    pub fn accept(&mut self) -> Result<SockOpts, u32> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.pending == 0 {
            return Err(E_EAGAIN);
        }
        self.pending -= 1;
        self.accepted.push(self.opts);
        Ok(self.opts)
    }

    pub fn accepted_opts(&self, idx: usize) -> Option<SockOpts> {
        if idx < self.accepted.len() {
            Some(self.accepted[idx])
        } else {
            None
        }
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

pub fn run_unx10891_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10891");
    s.add("J1-段锚", SEG_BASE == 0x1c7e0, "accept/sockopt 竞态段基址 0x1c7e0");
    s.add("J2-默认选项", {
        let o = SockOpts::plain();
        !o.nonblock && !o.passcred
    }, "默认双关");
    s.add("J3-空待连 accept 拒", {
        let mut l = OptListener::new(SockOpts::plain());
        l.accept() == Err(E_EAGAIN)
    }, "无连 E_EAGAIN");
    s.add("J4-到连后 accept 继承", {
        let mut l = OptListener::new(SockOpts { nonblock: true, passcred: false });
        l.arrival();
        l.accept() == Ok(SockOpts { nonblock: true, passcred: false })
    }, "继承 nonblock 真");
    s.add("J5-迟到设置不回溯", {
        let mut l = OptListener::new(SockOpts::plain());
        l.arrival();
        l.accept().unwrap();
        l.set_opts(SockOpts { nonblock: true, passcred: true }).unwrap();
        l.accepted_opts(0) == Some(SockOpts::plain())
    }, "先连后设旧 fd 仍原选项");
    s.add("J6-迟到设置影响后连", {
        let mut l = OptListener::new(SockOpts::plain());
        l.arrival();
        l.accept().unwrap();
        l.set_opts(SockOpts { nonblock: true, passcred: true }).unwrap();
        l.arrival();
        l.accept() == Ok(SockOpts { nonblock: true, passcred: true })
    }, "后连继承新选项");
    s.add("J7-两序对比账", {
        let mut a = OptListener::new(SockOpts::plain());
        a.arrival();
        let r1 = a.accept();
        a.set_opts(SockOpts { nonblock: true, passcred: false }).unwrap();
        let mut b = OptListener::new(SockOpts::plain());
        b.set_opts(SockOpts { nonblock: true, passcred: false }).unwrap();
        b.arrival();
        let r2 = b.accept();
        r1 == Ok(SockOpts::plain()) && r2 == Ok(SockOpts { nonblock: true, passcred: false })
    }, "accept 先/设先两序分账");
    s.add("J8-双待连同批选项", {
        let mut l = OptListener::new(SockOpts { nonblock: false, passcred: true });
        l.arrival();
        l.arrival();
        let o1 = l.accept().unwrap();
        let o2 = l.accept().unwrap();
        o1 == o2
    }, "同批选项一致");
    s.add("J9-待连计数", {
        let mut l = OptListener::new(SockOpts::plain());
        l.arrival();
        l.arrival();
        l.arrival();
        l.accept().unwrap();
        l.pending_of() == 2
    }, "三到一收余 2");
    s.add("J10-选项写覆盖", {
        let mut l = OptListener::new(SockOpts::plain());
        l.set_opts(SockOpts { nonblock: true, passcred: false }).unwrap();
        l.set_opts(SockOpts { nonblock: true, passcred: true }).unwrap();
        l.opts_of() == SockOpts { nonblock: true, passcred: true }
    }, "两次设值覆盖取尾");
    s.add("J11-封账拒 accept", {
        let mut l = OptListener::new(SockOpts::plain());
        l.arrival();
        l.seal();
        l.accept() == Err(E_SEALED)
    }, "封账 E_SEALED");
    s.add("J12-封账拒设值", {
        let mut l = OptListener::new(SockOpts::plain());
        l.seal();
        l.set_opts(SockOpts::plain()) == Err(E_SEALED)
    }, "封账设值 E_SEALED");
    s.add("J13-选项枚举四态", {
        let a = SockOpts { nonblock: false, passcred: false };
        let b = SockOpts { nonblock: true, passcred: false };
        let c = SockOpts { nonblock: false, passcred: true };
        let d = SockOpts { nonblock: true, passcred: true };
        a != b && b != c && c != d && a != d
    }, "四态两两抽样互异");
    s.add("J14-accept 记账", {
        let mut l = OptListener::new(SockOpts::plain());
        l.arrival();
        l.arrival();
        l.accept().unwrap();
        l.accept().unwrap();
        l.accept() == Err(E_EAGAIN) && l.accepted_opts(1).is_some()
    }, "两收后第三拒且账有位 1");
    s.add("J15-继承快照独立", {
        let mut l = OptListener::new(SockOpts::plain());
        l.arrival();
        let o = l.accept().unwrap();
        l.set_opts(SockOpts { nonblock: true, passcred: true }).unwrap();
        o == SockOpts::plain() && l.opts_of() == SockOpts { nonblock: true, passcred: true }
    }, "快照与现值分账");
    s.add_dyn("J16-三连序账", {
        let mut l = OptListener::new(SockOpts::plain());
        l.arrival();
        let r1 = l.accept();
        let r2 = l.accept();
        l.arrival();
        let r3 = l.accept();
        r1.is_ok() && r2.is_err() && r3.is_ok()
    }, "Ok→EAGAIN→Ok 三连账");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_accept_sockopt_race() {
        let mut l = OptListener::new(SockOpts::plain());
        l.arrival();
        assert_eq!(l.accept(), Ok(SockOpts::plain()));
        l.set_opts(SockOpts { nonblock: true, passcred: false }).unwrap();
        l.arrival();
        assert_eq!(l.accept(), Ok(SockOpts { nonblock: true, passcred: false }));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10891_checks();
        assert!(s.all_passed(), "UNX-F10891 判据全绿");
    }
}
