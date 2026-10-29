//! UNX-F10870 EAGAIN dgram 满矩阵（0x1c540 段）
//! 锚点：UNX-F10870-J1 EAGAIN 正反双判据（非阻塞收发缓冲满/空面）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c540;
pub const E_EAGAIN: u32 = SEG_BASE + 1;
pub const E_EBADF: u32 = SEG_BASE + 2;
pub const E_EMSGSIZE: u32 = SEG_BASE + 3;
pub const E_EISCONN: u32 = SEG_BASE + 4;
pub const E_OK: u32 = SEG_BASE + 5;
pub const E_SEALED: u32 = SEG_BASE + 6;

pub const RCV_CAP: usize = 4;
pub const SND_CAP: usize = 2;

/// 非阻塞 dgram 双缓冲套接字
pub struct FullDgram {
    rcv: Vec<Vec<u8>>,
    snd: Vec<Vec<u8>>,
    rcv_cap: usize,
    snd_cap: usize,
    open: bool,
}

impl FullDgram {
    pub fn new() -> Self {
        FullDgram { rcv: Vec::new(), snd: Vec::new(), rcv_cap: RCV_CAP, snd_cap: SND_CAP, open: true }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    /// 非阻塞发送：对端接收满 → EAGAIN
    pub fn send(&mut self, buf: &[u8]) -> Result<(), u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        if self.snd.len() >= self.snd_cap {
            return Err(E_EAGAIN);
        }
        self.snd.push(buf.to_vec());
        Ok(())
    }

    /// 非阻塞接收：空 → EAGAIN
    pub fn recv(&mut self) -> Result<Vec<u8>, u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        if self.rcv.is_empty() {
            return Err(E_EAGAIN);
        }
        Ok(self.rcv.remove(0))
    }

    /// 对端投递（模拟内核搬运）：本端接收满 → EAGAIN
    pub fn deliver(&mut self, buf: &[u8]) -> Result<(), u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        if self.rcv.len() >= self.rcv_cap {
            return Err(E_EAGAIN);
        }
        self.rcv.push(buf.to_vec());
        Ok(())
    }

    /// 刷新发送缓冲到对端（此处自投递）
    pub fn flush(&mut self) -> usize {
        let mut moved = 0;
        while !self.snd.is_empty() {
            let pkt = self.snd.remove(0);
            match self.deliver(&pkt) {
                Ok(()) => moved += 1,
                Err(_) => {
                    self.snd.insert(0, pkt);
                    break;
                }
            }
        }
        moved
    }

    pub fn rcv_depth(&self) -> usize {
        self.rcv.len()
    }

    pub fn snd_depth(&self) -> usize {
        self.snd.len()
    }
}

pub fn run_unx10870_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10870");
    s.add("J1-段锚", SEG_BASE == 0x1c540, "EAGAIN dgram 段基址 0x1c540");
    s.add("J2-空收 EAGAIN", {
        let mut d = FullDgram::new();
        d.recv() == Err(E_EAGAIN)
    }, "空接收缓冲 E_EAGAIN");
    s.add("J3-满投 EAGAIN", {
        let mut d = FullDgram::new();
        let mut n = 0;
        while n < RCV_CAP {
            d.deliver(b"x").unwrap();
            n += 1;
        }
        d.deliver(b"overflow") == Err(E_EAGAIN)
    }, "投满 4 后第 5 报 E_EAGAIN");
    s.add("J4-满发 EAGAIN", {
        let mut d = FullDgram::new();
        d.send(b"a").unwrap();
        d.send(b"b").unwrap();
        d.send(b"c") == Err(E_EAGAIN)
    }, "发满 2 后第 3 报 E_EAGAIN");
    s.add("J5-投后可收", {
        let mut d = FullDgram::new();
        d.deliver(b"m").unwrap();
        d.recv() == Ok(b"m".to_vec())
    }, "投递后 recv Ok");
    s.add("J6-刷新搬运", {
        let mut d = FullDgram::new();
        d.send(b"1").unwrap();
        d.send(b"2").unwrap();
        d.flush() == 2 && d.rcv_depth() == 2 && d.snd_depth() == 0
    }, "flush 搬两报收 2 发 0");
    s.add("J7-刷新遇满回停", {
        let mut d = FullDgram::new();
        d.deliver(b"p0").unwrap();
        d.deliver(b"p0").unwrap();
        d.send(b"p1").unwrap();
        d.send(b"p2").unwrap();
        let moved = d.flush();
        moved == 2 && d.deliver(b"px") == Err(E_EAGAIN) && d.rcv_depth() == 4
    }, "预投 2 flush 搬 2 收满 4 再投 E_EAGAIN");
    s.add("J8-刷新阻塞后恢复", {
        let mut d = FullDgram::new();
        let mut n = 0;
        while n < RCV_CAP {
            d.deliver(b"fill").unwrap();
            n += 1;
        }
        d.send(b"held").unwrap();
        d.flush() == 0 && d.snd_depth() == 1
    }, "收满 flush 滞留 1");
    s.add("J9-腾空后恢复", {
        let mut d = FullDgram::new();
        let mut n = 0;
        while n < RCV_CAP {
            d.deliver(b"fill").unwrap();
            n += 1;
        }
        d.send(b"held").unwrap();
        let _ = d.flush();
        d.recv().unwrap();
        d.flush() == 1 && d.rcv_depth() == 4
    }, "收一腾一 flush 成功");
    s.add("J10-关闭后 EBADF", {
        let mut d = FullDgram::new();
        d.close();
        d.send(b"x") == Err(E_EBADF) && d.recv() == Err(E_EBADF)
    }, "关闭后收发皆 E_EBADF");
    s.add("J11-EBADF 先于 EAGAIN", {
        let mut d = FullDgram::new();
        d.close();
        d.recv() == Err(E_EBADF)
    }, "空且关 recv E_EBADF");
    s.add("J12-接收 FIFO 序", {
        let mut d = FullDgram::new();
        d.deliver(b"f").unwrap();
        d.deliver(b"s").unwrap();
        d.deliver(b"t").unwrap();
        d.recv().unwrap() == b"f".to_vec()
            && d.recv().unwrap() == b"s".to_vec()
            && d.recv().unwrap() == b"t".to_vec()
    }, "三报 FIFO 全中");
    s.add("J13-零长报入队", {
        let mut d = FullDgram::new();
        d.deliver(b"").unwrap();
        d.recv() == Ok(Vec::new())
    }, "零长报投递回收");
    s.add("J14-满发恢复", {
        let mut d = FullDgram::new();
        d.send(b"a").unwrap();
        d.send(b"b").unwrap();
        let _ = d.flush();
        d.send(b"c").is_ok()
    }, "flush 后发送位腾空再发 Ok");
    s.add("J15-刷新次序守恒", {
        let mut d = FullDgram::new();
        d.send(b"a").unwrap();
        d.send(b"b").unwrap();
        d.flush();
        d.recv().unwrap() == b"a".to_vec() && d.recv().unwrap() == b"b".to_vec()
    }, "flush 后收序同发序");
    s.add_dyn("J16-交错轮账", {
        let mut d = FullDgram::new();
        d.send(b"r1").unwrap();
        let f1 = d.flush();
        d.send(b"r2").unwrap();
        d.send(b"r3").unwrap();
        let f2 = d.flush();
        f1 == 1 && f2 == 2
    }, "两轮刷新 1-2");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_eagain_matrix() {
        let mut d = FullDgram::new();
        assert_eq!(d.recv(), Err(E_EAGAIN));
        d.send(b"a").unwrap();
        d.flush();
        assert_eq!(d.recv(), Ok(b"a".to_vec()));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10870_checks();
        assert!(s.all_passed(), "UNX-F10870 判据全绿");
    }
}
