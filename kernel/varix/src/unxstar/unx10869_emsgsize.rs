//! UNX-F10869 EMSGSIZE 矩阵（dgram 超限）（0x1c520 段）
//! 锚点：UNX-F10869-J1 EMSGSIZE 正反双判据（报文超上限拒收+边界值过）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c520;
pub const E_EMSGSIZE: u32 = SEG_BASE + 1;
pub const E_EAGAIN: u32 = SEG_BASE + 2;
pub const E_ENOBUFS: u32 = SEG_BASE + 3;
pub const E_EBADF: u32 = SEG_BASE + 4;
pub const E_OK: u32 = SEG_BASE + 5;
pub const E_SEALED: u32 = SEG_BASE + 6;

pub const MSG_LIMIT: usize = 65_507;

/// dgram 发送缓冲
pub struct DgramSock {
    open: bool,
    queued: Vec<Vec<u8>>,
    queue_cap: usize,
}

impl DgramSock {
    pub fn new(queue_cap: usize) -> Self {
        DgramSock { open: true, queued: Vec::new(), queue_cap }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn depth(&self) -> usize {
        self.queued.len()
    }

    /// sendto 判定：fd → 超限 → 满 → Ok
    pub fn sendto(&mut self, buf: &[u8]) -> Result<(), u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        if buf.len() > MSG_LIMIT {
            return Err(E_EMSGSIZE);
        }
        if self.queued.len() >= self.queue_cap {
            return Err(E_EAGAIN);
        }
        self.queued.push(buf.to_vec());
        Ok(())
    }

    pub fn recv(&mut self) -> Option<Vec<u8>> {
        if self.queued.is_empty() {
            None
        } else {
            Some(self.queued.remove(0))
        }
    }
}

pub fn run_unx10869_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10869");
    s.add("J1-段锚", SEG_BASE == 0x1c520, "EMSGSIZE 段基址 0x1c520");
    s.add("J2-上限锚", MSG_LIMIT == 65_507, "dgram 上限 65,507");
    s.add("J3-超限拒收", {
        let mut d = DgramSock::new(8);
        let big = [0u8; MSG_LIMIT + 1];
        d.sendto(&big) == Err(E_EMSGSIZE)
    }, "65,508 字节 E_EMSGSIZE");
    s.add("J4-上限值过", {
        let mut d = DgramSock::new(8);
        let edge = [0u8; MSG_LIMIT];
        d.sendto(&edge).is_ok()
    }, "65,507 字节 Ok");
    s.add("J5-小报过", {
        let mut d = DgramSock::new(8);
        d.sendto(b"ping").is_ok()
    }, "4 字节 Ok");
    s.add("J6-超限不入队", {
        let mut d = DgramSock::new(8);
        let big = [0u8; MSG_LIMIT + 2];
        let _ = d.sendto(&big);
        d.depth() == 0
    }, "失败不入队");
    s.add("J7-上限不溢相邻", {
        let mut d = DgramSock::new(8);
        let edge = [0u8; MSG_LIMIT];
        let big = [0u8; MSG_LIMIT + 1];
        d.sendto(&edge).is_ok() && d.sendto(&big) == Err(E_EMSGSIZE)
    }, "边界 65,507/65,508 分界");
    s.add("J8-满队 EAGAIN 先后", {
        let mut d = DgramSock::new(2);
        d.sendto(b"a").unwrap();
        d.sendto(b"b").unwrap();
        let big = [0u8; MSG_LIMIT + 1];
        d.sendto(&big) == Err(E_EMSGSIZE)
    }, "满队时超限仍报 EMSGSIZE（序先）");
    s.add("J9-满队正常报 EAGAIN", {
        let mut d = DgramSock::new(1);
        d.sendto(b"a").unwrap();
        d.sendto(b"b") == Err(E_EAGAIN)
    }, "满队 E_EAGAIN");
    s.add("J10-关闭后 EBADF", {
        let mut d = DgramSock::new(8);
        d.close();
        d.sendto(b"x") == Err(E_EBADF)
    }, "关闭后 E_EBADF");
    s.add("J11-关闭先于超限", {
        let mut d = DgramSock::new(8);
        d.close();
        let big = [0u8; MSG_LIMIT + 9];
        d.sendto(&big) == Err(E_EBADF)
    }, "EBADF 优先于 EMSGSIZE");
    s.add("J12-收发守恒", {
        let mut d = DgramSock::new(8);
        d.sendto(b"m1").unwrap();
        d.sendto(b"m2").unwrap();
        d.recv().unwrap() == b"m1".to_vec() && d.recv().unwrap() == b"m2".to_vec()
    }, "FIFO 收发两报原样");
    s.add("J13-收空队 None", {
        let mut d = DgramSock::new(8);
        d.recv().is_none()
    }, "空队 recv None");
    s.add("J14-零长报可发", {
        let mut d = DgramSock::new(8);
        d.sendto(b"").is_ok() && d.depth() == 1
    }, "零长 dgram 入队");
    s.add("J15-多级容量", {
        let mut d = DgramSock::new(3);
        d.sendto(b"1").unwrap();
        d.sendto(b"2").unwrap();
        d.sendto(b"3").unwrap();
        d.sendto(b"4") == Err(E_EAGAIN)
    }, "容量 3 第 4 报 E_EAGAIN");
    s.add_dyn("J16-拒收后可恢复", {
        let mut d = DgramSock::new(8);
        let big = [0u8; MSG_LIMIT + 1];
        let _ = d.sendto(&big);
        let ok1 = d.sendto(b"ok").is_ok();
        d.close();
        let bad = d.sendto(b"ok");
        ok1 && bad.is_err()
    }, "EMS 标后小报 Ok 再 EBADF");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_msgsize_edge() {
        let mut d = DgramSock::new(8);
        let edge = [0u8; MSG_LIMIT];
        assert!(d.sendto(&edge).is_ok());
        let big = [0u8; MSG_LIMIT + 1];
        assert_eq!(d.sendto(&big), Err(E_EMSGSIZE));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10869_checks();
        assert!(s.all_passed(), "UNX-F10869 判据全绿");
    }
}
