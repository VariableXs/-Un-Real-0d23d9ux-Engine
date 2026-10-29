//! UNX-F10877 accept 错误矩阵（EINVAL/EMFILE）（0x1c620 段）
//! 锚点：UNX-F10877-J1 accept 错误面（非监听/表满/无连接三态）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c620;
pub const E_EINVAL: u32 = SEG_BASE + 1;
pub const E_EMFILE: u32 = SEG_BASE + 2;
pub const E_EAGAIN: u32 = SEG_BASE + 3;
pub const E_EBADF: u32 = SEG_BASE + 4;
pub const E_OK: u32 = SEG_BASE + 5;
pub const E_SEALED: u32 = SEG_BASE + 6;

pub const FD_CAP: usize = 4;

/// 监听套接字：pending 连接队列 + fd 表联动
pub struct Listener {
    listening: bool,
    backlog: Vec<u16>,
    backlog_cap: usize,
    open_fds: usize,
    fd_cap: usize,
    open: bool,
}

impl Listener {
    pub fn new(listening: bool, backlog_cap: usize, fd_cap: usize) -> Self {
        Listener {
            listening,
            backlog: Vec::new(),
            backlog_cap,
            open_fds: 1,
            fd_cap,
            open: true,
        }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn fill_backlog(&mut self, n: usize) {
        let mut i = 0;
        while i < n {
            self.backlog.push(1_000 + i as u16);
            i += 1;
        }
    }

    /// accept 判定：fd 关闭 → 非监听 → 表满 → 队空 → 出队
    pub fn accept(&mut self) -> Result<u16, u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        if !self.listening {
            return Err(E_EINVAL);
        }
        if self.open_fds >= self.fd_cap {
            return Err(E_EMFILE);
        }
        if self.backlog.is_empty() {
            return Err(E_EAGAIN);
        }
        let conn = self.backlog.remove(0);
        self.open_fds += 1;
        Ok(conn)
    }

    pub fn backlog_len(&self) -> usize {
        self.backlog.len()
    }

    pub fn open_fds_of(&self) -> usize {
        self.open_fds
    }
}

pub fn run_unx10877_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10877");
    s.add("J1-段锚", SEG_BASE == 0x1c620, "accept 段基址 0x1c620");
    s.add("J2-fd 上限锚", FD_CAP == 4, "fd 表上限 4");
    s.add("J3-非监听拒", {
        let mut l = Listener::new(false, 8, FD_CAP);
        l.fill_backlog(2);
        l.accept() == Err(E_EINVAL)
    }, "非监听 E_EINVAL");
    s.add("J4-监听有队过", {
        let mut l = Listener::new(true, 8, FD_CAP);
        l.fill_backlog(2);
        l.accept() == Ok(1000)
    }, "出队首连 1000");
    s.add("J5-队空 EAGAIN", {
        let mut l = Listener::new(true, 8, FD_CAP);
        l.accept() == Err(E_EAGAIN)
    }, "空队 E_EAGAIN");
    s.add("J6-表满 EMFILE", {
        let mut l = Listener::new(true, 8, FD_CAP);
        l.fill_backlog(3);
        let r1 = l.accept();
        let r2 = l.accept();
        let r3 = l.accept();
        let r4 = l.accept();
        r1.is_ok() && r2.is_ok() && r3.is_ok() && r4 == Err(E_EMFILE)
    }, "fd 1+3=4 满，第四连 E_EMFILE");
    s.add("J7-EMFILE 先于队空", {
        let mut l = Listener::new(true, 8, FD_CAP);
        l.fill_backlog(3);
        let _ = l.accept();
        let _ = l.accept();
        let _ = l.accept();
        l.accept() == Err(E_EMFILE)
    }, "满表+空队 EMFILE 优先");
    s.add("J8-EBADF 优先一切", {
        let mut l = Listener::new(false, 8, FD_CAP);
        l.close();
        l.accept() == Err(E_EBADF)
    }, "关闭+非监听 E_EBADF");
    s.add("J9-出队计数", {
        let mut l = Listener::new(true, 8, FD_CAP);
        l.fill_backlog(4);
        let _ = l.accept();
        let _ = l.accept();
        l.backlog_len() == 2 && l.open_fds_of() == 3
    }, "两出队 backlog 2 fd 3");
    s.add("J10-背压容量", {
        let mut l = Listener::new(true, 2, 99);
        l.fill_backlog(5);
        l.backlog_len() == 5
    }, "fill 直灌 5（容量语义另行）");
    s.add("J11-逐连取号", {
        let mut l = Listener::new(true, 8, FD_CAP);
        l.fill_backlog(3);
        l.accept() == Ok(1000) && l.accept() == Ok(1001) && l.accept() == Ok(1002)
    }, "三连号 1000/1001/1002");
    s.add("J12-监听态可查", {
        let a = Listener::new(true, 8, FD_CAP);
        let b = Listener::new(false, 8, FD_CAP);
        a.listening && !b.listening
    }, "监听两态");
    s.add("J13-新 fd 计入", {
        let mut l = Listener::new(true, 8, FD_CAP);
        l.fill_backlog(1);
        let _ = l.accept();
        l.open_fds_of() == 2
    }, "accept 后 fd 2");
    s.add("J14-关闭后队存", {
        let mut l = Listener::new(true, 8, FD_CAP);
        l.fill_backlog(2);
        l.close();
        l.backlog_len() == 2 && l.accept() == Err(E_EBADF)
    }, "关后队 2 且拒收");
    s.add("J15-单队列全清", {
        let mut l = Listener::new(true, 8, 99);
        l.fill_backlog(3);
        let mut ok = true;
        let mut n = 0;
        while n < 3 {
            ok = ok && l.accept().is_ok();
            n += 1;
        }
        ok && l.accept() == Err(E_EAGAIN) && l.backlog_len() == 0
    }, "三连全清后 E_EAGAIN");
    s.add_dyn("J16-次序总账", {
        let mut l = Listener::new(false, 8, FD_CAP);
        let r1 = l.accept();
        l.listening = true;
        let r2 = l.accept();
        l.fill_backlog(1);
        let r3 = l.accept();
        r1.is_err() && r2.is_err() && r3.is_ok()
    }, "EINVAL→EAGAIN→Ok 次序");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_accept_err() {
        let mut l = Listener::new(true, 8, FD_CAP);
        assert_eq!(l.accept(), Err(E_EAGAIN));
        l.fill_backlog(1);
        assert_eq!(l.accept(), Ok(1000));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10877_checks();
        assert!(s.all_passed(), "UNX-F10877 判据全绿");
    }
}
