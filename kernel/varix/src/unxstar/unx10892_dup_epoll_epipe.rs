//! UNX-F10892 dup 注册 epoll 后 EPIPE 联动矩阵（0x1c800 段）
//! 锚点：UNX-F10892-J1 dup-epoll EPIPE 联动（fd 别名事件面+对端关）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c800;
pub const E_EPIPE: u32 = SEG_BASE + 1;
pub const E_EBADF: u32 = SEG_BASE + 2;
pub const E_EINVAL: u32 = SEG_BASE + 3;
pub const E_OK: u32 = SEG_BASE + 4;
pub const E_SEALED: u32 = SEG_BASE + 5;

/// 事件位：IN / OUT / ERR / HUP
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EvBits {
    pub inb: bool,
    pub out: bool,
    pub err: bool,
    pub hup: bool,
}

impl EvBits {
    pub fn none() -> Self {
        EvBits { inb: false, out: false, err: false, hup: false }
    }
    pub fn any(&self) -> bool {
        self.inb || self.out || self.err || self.hup
    }
}

/// fd 别名表：dup 同源 fd 共享状态
pub struct DupTable {
    aliases: Vec<(usize, usize)>,
    peer_alive: bool,
    registered: Vec<usize>,
    sealed: bool,
}

impl DupTable {
    pub fn new(peer_alive: bool) -> Self {
        DupTable { aliases: Vec::new(), peer_alive, registered: Vec::new(), sealed: false }
    }

    /// dup：new_fd 别名 old_fd
    pub fn dup(&mut self, old_fd: usize, new_fd: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if new_fd == old_fd {
            return Err(E_EINVAL);
        }
        if self.root_of(new_fd).is_some() {
            return Err(E_EINVAL);
        }
        self.aliases.push((old_fd, new_fd));
        Ok(())
    }

    /// 求根：别名链终点
    pub fn root_of(&self, fd: usize) -> Option<usize> {
        let mut cur = fd;
        let mut found = false;
        let mut i = 0;
        while i < self.aliases.len() {
            if self.aliases[i].1 == cur {
                cur = self.aliases[i].0;
                found = true;
                i = 0;
                continue;
            }
            i += 1;
        }
        if found {
            Some(cur)
        } else {
            None
        }
    }

    /// epoll 注册：fd（或其别名）登记
    pub fn epoll_add(&mut self, fd: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.root_of(fd).is_none() {
            return Err(E_EBADF);
        }
        let root = self.root_of(fd).unwrap();
        let mut i = 0;
        while i < self.registered.len() {
            if self.registered[i] == root {
                return Err(E_EINVAL);
            }
            i += 1;
        }
        self.registered.push(root);
        Ok(())
    }

    pub fn registered_count(&self) -> usize {
        self.registered.len()
    }

    /// 事件面：对端关 → 全别名组 HUP+ERR；写面 EPIPE
    pub fn poll(&self, fd: usize) -> Result<EvBits, u32> {
        if self.root_of(fd).is_none() {
            return Err(E_EBADF);
        }
        if self.peer_alive {
            return Ok(EvBits { inb: false, out: true, err: false, hup: false });
        }
        Ok(EvBits { inb: false, out: false, err: true, hup: true })
    }

    pub fn write_via(&self, fd: usize) -> Result<(), u32> {
        if self.root_of(fd).is_none() {
            return Err(E_EBADF);
        }
        if self.peer_alive {
            return Ok(());
        }
        Err(E_EPIPE)
    }

    pub fn peer_close(&mut self) {
        self.peer_alive = false;
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

pub fn run_unx10892_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10892");
    s.add("J1-段锚", SEG_BASE == 0x1c800, "dup-epoll EPIPE 段基址 0x1c800");
    s.add("J2-dup 别名建", {
        let mut d = DupTable::new(true);
        d.dup(3, 7).unwrap();
        d.root_of(7) == Some(3)
    }, "7 别名根 3");
    s.add("J3-自 dup 拒", {
        let mut d = DupTable::new(true);
        d.dup(3, 3) == Err(E_EINVAL)
    }, "同号 E_EINVAL");
    s.add("J4-占号 dup 拒", {
        let mut d = DupTable::new(true);
        d.dup(3, 7).unwrap();
        d.dup(4, 7) == Err(E_EINVAL)
    }, "7 已占 E_EINVAL");
    s.add("J5-链式 dup 根一致", {
        let mut d = DupTable::new(true);
        d.dup(3, 7).unwrap();
        d.dup(7, 9).unwrap();
        d.root_of(9) == Some(3)
    }, "9→7→3 根 3");
    s.add("J6-未注册 fd poll 拒", {
        let d = DupTable::new(true);
        d.poll(5) == Err(E_EBADF)
    }, "孤 fd E_EBADF");
    s.add("J7-epoll 注册过", {
        let mut d = DupTable::new(true);
        d.dup(3, 7).unwrap();
        d.epoll_add(7).unwrap();
        d.registered_count() == 1
    }, "别名注册计 1");
    s.add("J8-重复注册拒", {
        let mut d = DupTable::new(true);
        d.dup(3, 7).unwrap();
        d.epoll_add(7).unwrap();
        d.epoll_add(3) == Err(E_EINVAL)
    }, "同根重注 E_EINVAL");
    s.add("J9-孤 fd 注册拒", {
        let mut d = DupTable::new(true);
        d.epoll_add(42) == Err(E_EBADF)
    }, "孤号 E_EBADF");
    s.add("J10-活端 OUT 事件", {
        let mut d = DupTable::new(true);
        d.dup(3, 7).unwrap();
        let e = d.poll(7).unwrap();
        e.out && !e.hup && e.any()
    }, "活端 OUT 真");
    s.add("J11-对端关 HUP+ERR", {
        let mut d = DupTable::new(true);
        d.dup(3, 7).unwrap();
        d.peer_close();
        let e = d.poll(7).unwrap();
        e.hup && e.err
    }, "关后 HUP ERR 双真");
    s.add("J12-关后经别名写 EPIPE", {
        let mut d = DupTable::new(true);
        d.dup(3, 7).unwrap();
        d.peer_close();
        d.write_via(7) == Err(E_EPIPE)
    }, "别名写 E_EPIPE");
    s.add("J13-活端经别名写 Ok", {
        let mut d = DupTable::new(true);
        d.dup(3, 7).unwrap();
        d.write_via(7).is_ok()
    }, "活端别名写 Ok");
    s.add("J14-事件四态无冗余", {
        let a = EvBits::none();
        !a.any()
    }, "全零事件 any 假");
    s.add("J15-多别名共享事件", {
        let mut d = DupTable::new(true);
        d.dup(3, 7).unwrap();
        d.dup(3, 8).unwrap();
        d.peer_close();
        d.poll(7).unwrap().hup && d.poll(8).unwrap().hup
    }, "7/8 两别名同 HUP");
    s.add_dyn("J16-联动全账", {
        let mut d = DupTable::new(true);
        d.dup(3, 7).unwrap();
        d.epoll_add(7).unwrap();
        let w0 = d.write_via(7);
        d.peer_close();
        let w1 = d.write_via(7);
        let e = d.poll(7).unwrap();
        w0.is_ok() && w1.is_err() && e.hup
    }, "Ok→EPIPE→HUP 联动账");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_dup_epoll_epipe() {
        let mut d = DupTable::new(true);
        d.dup(3, 7).unwrap();
        d.epoll_add(7).unwrap();
        d.peer_close();
        assert_eq!(d.write_via(7), Err(E_EPIPE));
        assert!(d.poll(7).unwrap().hup);
    }

    #[test]
    fn checks_green() {
        let s = run_unx10892_checks();
        assert!(s.all_passed(), "UNX-F10892 判据全绿");
    }
}
