//! UNX-F10875 dgram 零长报文矩阵（0x1c5e0 段）
//! 锚点：UNX-F10875-J1 零长 dgram 正反双判据（0 长收发语义分账）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c5e0;
pub const E_EAGAIN: u32 = SEG_BASE + 1;
pub const E_EBADF: u32 = SEG_BASE + 2;
pub const E_EISCONN: u32 = SEG_BASE + 3;
pub const E_ENOTCONN: u32 = SEG_BASE + 4;
pub const E_OK: u32 = SEG_BASE + 5;
pub const E_SEALED: u32 = SEG_BASE + 6;

/// 零长报文（哨兵报，无载荷）
pub struct ZeroPacket;

/// dgram 队列：载荷报与零长报统一排队
pub struct ZeroDgram {
    queue: Vec<Option<Vec<u8>>>,
    cap: usize,
    open: bool,
    connected: bool,
}

impl ZeroDgram {
    pub fn new(cap: usize, connected: bool) -> Self {
        ZeroDgram { queue: Vec::new(), cap, open: true, connected }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn depth(&self) -> usize {
        self.queue.len()
    }

    /// send：buf 空即零长报（哨兵 None）；未连接面先判
    pub fn send(&mut self, buf: &[u8]) -> Result<(), u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        if self.connected {
            return Err(E_EISCONN);
        }
        if self.queue.len() >= self.cap {
            return Err(E_EAGAIN);
        }
        if buf.is_empty() {
            self.queue.push(None);
        } else {
            self.queue.push(Some(buf.to_vec()));
        }
        Ok(())
    }

    /// recv：零长报返回 Ok(空 vec)——与「无报 EAGAIN」分账
    pub fn recv(&mut self) -> Result<Vec<u8>, u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        if self.queue.is_empty() {
            return Err(E_EAGAIN);
        }
        match self.queue.remove(0) {
            None => Ok(Vec::new()),
            Some(v) => Ok(v),
        }
    }
}

pub fn run_unx10875_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10875");
    s.add("J1-段锚", SEG_BASE == 0x1c5e0, "零长 dgram 段基址 0x1c5e0");
    s.add("J2-零长发过", {
        let mut d = ZeroDgram::new(4, false);
        d.send(b"").is_ok() && d.depth() == 1
    }, "0 长入队 depth 1");
    s.add("J3-零长回收空 vec", {
        let mut d = ZeroDgram::new(4, false);
        d.send(b"").unwrap();
        d.recv() == Ok(Vec::new())
    }, "零长报 recv Ok(空)");
    s.add("J4-空队 EAGAIN 与零长分账", {
        let mut d = ZeroDgram::new(4, false);
        d.recv() == Err(E_EAGAIN)
    }, "未发零长报 recv E_EAGAIN");
    s.add("J5-发零长后非 EAGAIN", {
        let mut d = ZeroDgram::new(4, false);
        d.send(b"").unwrap();
        d.recv().is_ok()
    }, "发过零长 recv Ok（分界）");
    s.add("J6-零长与载荷混排", {
        let mut d = ZeroDgram::new(8, false);
        d.send(b"").unwrap();
        d.send(b"pay").unwrap();
        d.send(b"").unwrap();
        let r1 = d.recv().unwrap();
        let r2 = d.recv().unwrap();
        let r3 = d.recv().unwrap();
        r1.is_empty() && r2 == b"pay".to_vec() && r3.is_empty()
    }, "零-载-零三报 FIFO");
    s.add("J7-深度计数含零长", {
        let mut d = ZeroDgram::new(8, false);
        d.send(b"").unwrap();
        d.send(b"").unwrap();
        d.send(b"").unwrap();
        d.depth() == 3
    }, "三零长 depth 3");
    s.add("J8-零长占容量", {
        let mut d = ZeroDgram::new(2, false);
        d.send(b"").unwrap();
        d.send(b"").unwrap();
        d.send(b"") == Err(E_EAGAIN)
    }, "零长同样占满队 E_EAGAIN");
    s.add("J9-关闭后零长发 EBADF", {
        let mut d = ZeroDgram::new(4, false);
        d.close();
        d.send(b"") == Err(E_EBADF)
    }, "关闭 E_EBADF");
    s.add("J10-关闭后零长收 EBADF", {
        let mut d = ZeroDgram::new(4, false);
        d.send(b"").unwrap();
        d.close();
        d.recv() == Err(E_EBADF)
    }, "关后存报 recv E_EBADF");
    s.add("J11-已连接 send 拒", {
        let mut d = ZeroDgram::new(4, true);
        d.send(b"x") == Err(E_EISCONN)
    }, "connected 面 E_EISCONN");
    s.add("J12-零长不顶替 EISCONN 判", {
        let mut d = ZeroDgram::new(4, true);
        d.send(b"") == Err(E_EISCONN)
    }, "零长也过连接闸");
    s.add("J13-载荷回收非空", {
        let mut d = ZeroDgram::new(4, false);
        d.send(b"pl").unwrap();
        d.recv() == Ok(b"pl".to_vec())
    }, "载荷报原样回");
    s.add("J14-零长连发连收", {
        let mut d = ZeroDgram::new(8, false);
        d.send(b"").unwrap();
        d.send(b"").unwrap();
        d.send(b"").unwrap();
        d.send(b"").unwrap();
        let mut all_zero = true;
        let mut n = 0;
        while n < 4 {
            if !d.recv().unwrap().is_empty() {
                all_zero = false;
            }
            n += 1;
        }
        all_zero && d.depth() == 0
    }, "四零长连收全空且队清");
    s.add("J15-容量锚", {
        let mut d = ZeroDgram::new(3, false);
        d.send(b"").unwrap();
        d.send(b"x").unwrap();
        d.send(b"").unwrap();
        d.send(b"y") == Err(E_EAGAIN)
    }, "混排占满 3 后 E_EAGAIN");
    s.add_dyn("J16-收空再收分界", {
        let mut d = ZeroDgram::new(8, false);
        d.send(b"").unwrap();
        let r1 = d.recv();
        let r2 = d.recv();
        r1.is_ok() && r2.is_err()
    }, "首收 Ok(空) 次收 E_EAGAIN");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_zero_dgram() {
        let mut d = ZeroDgram::new(4, false);
        d.send(b"").unwrap();
        assert_eq!(d.recv(), Ok(Vec::new()));
        assert_eq!(d.recv(), Err(E_EAGAIN));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10875_checks();
        assert!(s.all_passed(), "UNX-F10875 判据全绿");
    }
}
