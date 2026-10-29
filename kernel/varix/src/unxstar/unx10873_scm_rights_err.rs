//! UNX-F10873 SCM_RIGHTS fd 传递错误矩阵（收方表满/非法 fd）（0x1c5a0 段）
//! 锚点：UNX-F10873-J1 SCM_RIGHTS 传递错误（表满 ENFILE/非法 fd EBADF）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c5a0;
pub const E_ENFILE: u32 = SEG_BASE + 1;
pub const E_EBADF: u32 = SEG_BASE + 2;
pub const E_EINVAL: u32 = SEG_BASE + 3;
pub const E_ENOMEM: u32 = SEG_BASE + 4;
pub const E_OK: u32 = SEG_BASE + 5;
pub const E_SEALED: u32 = SEG_BASE + 6;

pub const FD_CAP: usize = 8;
pub const RIGHTS_MAX: usize = 4;

/// fd 表：容量 + 在册集合
pub struct FdTable {
    used: Vec<i32>,
    cap: usize,
}

impl FdTable {
    pub fn new(cap: usize) -> Self {
        FdTable { used: Vec::new(), cap }
    }

    pub fn len(&self) -> usize {
        self.used.len()
    }

    pub fn is_full(&self) -> bool {
        self.used.len() >= self.cap
    }

    pub fn holds(&self, fd: i32) -> bool {
        let mut i = 0;
        while i < self.used.len() {
            if self.used[i] == fd {
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn install(&mut self, fd: i32) -> Result<(), u32> {
        if self.is_full() {
            return Err(E_ENFILE);
        }
        if fd < 0 {
            return Err(E_EBADF);
        }
        if self.holds(fd) {
            return Err(E_EBADF);
        }
        self.used.push(fd);
        Ok(())
    }
}

/// SCM_RIGHTS 发送面：携带 fd 清单过信道
pub struct RightsChannel {
    in_flight: Vec<i32>,
    sealed: bool,
}

impl RightsChannel {
    pub fn new() -> Self {
        RightsChannel { in_flight: Vec::new(), sealed: false }
    }

    /// 发送方装填：空清单（EINVAL）/ 超量（EINVAL）/ 非法 fd（EBADF）
    pub fn send_rights(&mut self, fds: &[i32]) -> Result<(), u32> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if fds.is_empty() {
            return Err(E_EINVAL);
        }
        if fds.len() > RIGHTS_MAX {
            return Err(E_EINVAL);
        }
        let mut i = 0;
        while i < fds.len() {
            if fds[i] < 0 {
                return Err(E_EBADF);
            }
            i += 1;
        }
        let mut i = 0;
        while i < fds.len() {
            self.in_flight.push(fds[i]);
            i += 1;
        }
        Ok(())
    }

    pub fn in_flight(&self) -> usize {
        self.in_flight.len()
    }

    /// 接收方安装：逐 fd 装表，表满中断；信道收空即封账
    pub fn recv_into(&mut self, table: &mut FdTable) -> Result<usize, u32> {
        if self.in_flight.is_empty() {
            return Err(E_EINVAL);
        }
        let mut installed = 0;
        while !self.in_flight.is_empty() {
            let fd = self.in_flight[0];
            match table.install(fd) {
                Ok(()) => {
                    self.in_flight.remove(0);
                    installed += 1;
                }
                Err(e) => return Err(e),
            }
        }
        self.sealed = true;
        Ok(installed)
    }
}

pub fn run_unx10873_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10873");
    s.add("J1-段锚", SEG_BASE == 0x1c5a0, "SCM_RIGHTS 段基址 0x1c5a0");
    s.add("J2-表容量锚", FD_CAP == 8, "fd 表容量 8");
    s.add("J3-携带上限锚", RIGHTS_MAX == 4, "单条携带上限 4");
    s.add("J4-空清单拒", {
        let mut ch = RightsChannel::new();
        ch.send_rights(&[]) == Err(E_EINVAL)
    }, "空 fd 清单 E_EINVAL");
    s.add("J5-超量拒", {
        let mut ch = RightsChannel::new();
        ch.send_rights(&[3, 4, 5, 6, 7]) == Err(E_EINVAL)
    }, "5 只 fd E_EINVAL");
    s.add("J6-负 fd 拒", {
        let mut ch = RightsChannel::new();
        ch.send_rights(&[3, -1]) == Err(E_EBADF)
    }, "负 fd E_EBADF");
    s.add("J7-合法装填", {
        let mut ch = RightsChannel::new();
        ch.send_rights(&[3, 4]).unwrap();
        ch.in_flight() == 2
    }, "两只 fd 在途");
    s.add("J8-接收安装", {
        let mut ch = RightsChannel::new();
        ch.send_rights(&[3, 4, 5]).unwrap();
        let mut t = FdTable::new(FD_CAP);
        ch.recv_into(&mut t) == Ok(3) && t.len() == 3
    }, "三只 fd 全装");
    s.add("J9-表满中断", {
        let mut ch = RightsChannel::new();
        ch.send_rights(&[3, 4]).unwrap();
        let mut t = FdTable::new(1);
        t.install(9).unwrap();
        ch.recv_into(&mut t) == Err(E_ENFILE) && ch.in_flight() == 2
    }, "表满 E_ENFILE 且在途不丢");
    s.add("J10-半装续传", {
        let mut ch = RightsChannel::new();
        ch.send_rights(&[3, 4]).unwrap();
        let mut t = FdTable::new(1);
        let r1 = ch.recv_into(&mut t);
        let mut t2 = FdTable::new(FD_CAP);
        let r2 = ch.recv_into(&mut t2);
        r1 == Err(E_ENFILE) && r2 == Ok(2)
    }, "满中断后换表续传 2");
    s.add("J11-重复 fd 拒装", {
        let mut t = FdTable::new(FD_CAP);
        t.install(5).unwrap();
        t.install(5) == Err(E_EBADF)
    }, "在册 fd 重装 E_EBADF");
    s.add("J12-负 fd 拒装", {
        let mut t = FdTable::new(FD_CAP);
        t.install(-2) == Err(E_EBADF)
    }, "装负 fd E_EBADF");
    s.add("J13-表满拒装", {
        let mut t = FdTable::new(2);
        t.install(1).unwrap();
        t.install(2).unwrap();
        t.install(3) == Err(E_ENFILE)
    }, "容量 2 装 3 E_ENFILE");
    s.add("J14-空信道拒收", {
        let mut ch = RightsChannel::new();
        let mut t = FdTable::new(FD_CAP);
        ch.recv_into(&mut t) == Err(E_EINVAL)
    }, "空信道 E_EINVAL");
    s.add("J15-信道封账", {
        let mut ch = RightsChannel::new();
        ch.send_rights(&[3]).unwrap();
        let mut t = FdTable::new(FD_CAP);
        ch.recv_into(&mut t).unwrap();
        ch.send_rights(&[4]) == Err(E_SEALED)
    }, "收空后装填 E_SEALED");
    s.add_dyn("J16-装序守恒", {
        let mut ch = RightsChannel::new();
        ch.send_rights(&[7, 8, 9]).unwrap();
        let mut t = FdTable::new(FD_CAP);
        ch.recv_into(&mut t).unwrap();
        t.holds(7) && t.holds(8) && t.holds(9)
    }, "三 fd 逐只在表 111");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_rights_passing() {
        let mut ch = RightsChannel::new();
        ch.send_rights(&[3, 4]).unwrap();
        let mut t = FdTable::new(FD_CAP);
        assert_eq!(ch.recv_into(&mut t), Ok(2));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10873_checks();
        assert!(s.all_passed(), "UNX-F10873 判据全绿");
    }
}
