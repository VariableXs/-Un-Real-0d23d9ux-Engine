//! UNX-F10890 SO_RCVLOWAT 联动错误矩阵（wait 与 recv 低水位）（0x1c7c0 段）
//! 锚点：UNX-F10890-J1 SO_RCVLOWAT 联动（低水位阻塞唤醒面）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c7c0;
pub const E_EINVAL: u32 = SEG_BASE + 1;
pub const E_EAGAIN: u32 = SEG_BASE + 2;
pub const E_EBADF: u32 = SEG_BASE + 3;
pub const E_OK: u32 = SEG_BASE + 4;
pub const E_SEALED: u32 = SEG_BASE + 5;

pub const LOWAT_DEFAULT: usize = 1;
pub const LOWAT_MAX: usize = 1_048_576;

/// 低水位套接字：缓冲量 + lowat 阈值
pub struct LowatSock {
    buffered: usize,
    lowat: usize,
    open: bool,
    wakeups: usize,
}

impl LowatSock {
    pub fn new() -> Self {
        LowatSock { buffered: 0, lowat: LOWAT_DEFAULT, open: true, wakeups: 0 }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn lowat_of(&self) -> usize {
        self.lowat
    }

    pub fn buffered_of(&self) -> usize {
        self.buffered
    }

    pub fn wakeups(&self) -> usize {
        self.wakeups
    }

    /// setsockopt(SO_RCVLOWAT)：0 拒 / 超 MAX 拒
    pub fn set_lowat(&mut self, v: usize) -> Result<(), u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        if v == 0 || v > LOWAT_MAX {
            return Err(E_EINVAL);
        }
        self.lowat = v;
        Ok(())
    }

    pub fn fill(&mut self, n: usize) {
        self.buffered += n;
    }

    /// 可读性判定：非阻塞下 buffered < lowat → EAGAIN；达阈值 → 可读并计唤醒
    pub fn poll_readable(&mut self) -> Result<usize, u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        if self.buffered < self.lowat {
            return Err(E_EAGAIN);
        }
        self.wakeups += 1;
        Ok(self.buffered)
    }

    /// recv 消费
    pub fn recv(&mut self, n: usize) -> Result<usize, u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        if self.buffered == 0 {
            return Err(E_EAGAIN);
        }
        let take = if n > self.buffered { self.buffered } else { n };
        self.buffered -= take;
        Ok(take)
    }
}

pub fn run_unx10890_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10890");
    s.add("J1-段锚", SEG_BASE == 0x1c7c0, "RCVLOWAT 段基址 0x1c7c0");
    s.add("J2-默认低水位", {
        let k = LowatSock::new();
        k.lowat_of() == LOWAT_DEFAULT
    }, "默认 1");
    s.add("J3-零值拒", {
        let mut k = LowatSock::new();
        k.set_lowat(0) == Err(E_EINVAL)
    }, "lowat 0 E_EINVAL");
    s.add("J4-超限拒", {
        let mut k = LowatSock::new();
        k.set_lowat(LOWAT_MAX + 1) == Err(E_EINVAL)
    }, "超 1MiB E_EINVAL");
    s.add("J5-设值过", {
        let mut k = LowatSock::new();
        k.set_lowat(512).unwrap();
        k.lowat_of() == 512
    }, "lowat 512 生效");
    s.add("J6-低于阈值 EAGAIN", {
        let mut k = LowatSock::new();
        k.set_lowat(8).unwrap();
        k.fill(4);
        k.poll_readable() == Err(E_EAGAIN)
    }, "4<8 E_EAGAIN");
    s.add("J7-达阈值可读", {
        let mut k = LowatSock::new();
        k.set_lowat(8).unwrap();
        k.fill(8);
        k.poll_readable() == Ok(8)
    }, "8=8 Ok(8)");
    s.add("J8-超阈值可读", {
        let mut k = LowatSock::new();
        k.set_lowat(8).unwrap();
        k.fill(20);
        k.poll_readable() == Ok(20)
    }, "20>8 Ok(20)");
    s.add("J9-唤醒计数", {
        let mut k = LowatSock::new();
        k.set_lowat(2).unwrap();
        k.fill(3);
        k.poll_readable().unwrap();
        k.poll_readable().unwrap();
        k.wakeups() == 2
    }, "两轮唤醒计 2");
    s.add("J10-默认 lowat 零即读", {
        let mut k = LowatSock::new();
        k.fill(1);
        k.poll_readable() == Ok(1)
    }, "lowat=1 有 1 即读");
    s.add("J11-recv 消费降量", {
        let mut k = LowatSock::new();
        k.fill(10);
        k.recv(4).unwrap();
        k.buffered_of() == 6
    }, "收 4 余 6");
    s.add("J12-recv 超量截取", {
        let mut k = LowatSock::new();
        k.fill(3);
        k.recv(10) == Ok(3)
    }, "要 10 得 3");
    s.add("J13-recv 空队 EAGAIN", {
        let mut k = LowatSock::new();
        k.recv(1) == Err(E_EAGAIN)
    }, "空收 E_EAGAIN");
    s.add("J14-关后 set 拒", {
        let mut k = LowatSock::new();
        k.close();
        k.set_lowat(1) == Err(E_EBADF)
    }, "关后 E_EBADF");
    s.add("J15-关后 poll 拒", {
        let mut k = LowatSock::new();
        k.fill(9);
        k.close();
        k.poll_readable() == Err(E_EBADF)
    }, "关后 poll E_EBADF");
    s.add_dyn("J16-消费过阈值回收", {
        let mut k = LowatSock::new();
        k.set_lowat(4).unwrap();
        k.fill(6);
        let p1 = k.poll_readable();
        k.recv(4).unwrap();
        let p2 = k.poll_readable();
        p1.is_ok() && p2.is_err()
    }, "6≥4 Ok(6) 消费后 2<4 EAGAIN");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_lowat() {
        let mut k = LowatSock::new();
        k.set_lowat(4).unwrap();
        k.fill(3);
        assert_eq!(k.poll_readable(), Err(E_EAGAIN));
        k.fill(1);
        assert_eq!(k.poll_readable(), Ok(4));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10890_checks();
        assert!(s.all_passed(), "UNX-F10890 判据全绿");
    }
}
