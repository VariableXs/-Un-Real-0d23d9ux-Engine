//! UNX-F10889 seqpacket 超长截断矩阵（MSG_TRUNC）（0x1c7a0 段）
//! 锚点：UNX-F10889-J1 seqpacket 超长截断（MSG_TRUNC 置位+丢尾字节账）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c7a0;
pub const E_EAGAIN: u32 = SEG_BASE + 1;
pub const E_EMSGSIZE: u32 = SEG_BASE + 2;
pub const E_EBADF: u32 = SEG_BASE + 3;
pub const E_OK: u32 = SEG_BASE + 4;
pub const E_SEALED: u32 = SEG_BASE + 5;

/// seqpacket 报文：整报交付，读端缓冲不足则截断置 MSG_TRUNC
#[derive(Clone, Debug)]
pub struct SeqPacket {
    pub payload: Vec<u8>,
}

/// seqpacket 接收端
pub struct SeqRecv {
    queue: Vec<SeqPacket>,
    open: bool,
    trunc_events: usize,
}

pub struct SeqRead {
    pub data: Vec<u8>,
    pub truncated: bool,
    pub full_len: usize,
}

impl SeqRecv {
    pub fn new() -> Self {
        SeqRecv { queue: Vec::new(), open: true, trunc_events: 0 }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn send(&mut self, payload: &[u8]) -> Result<(), u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        if payload.len() > 4096 {
            return Err(E_EMSGSIZE);
        }
        self.queue.push(SeqPacket { payload: payload.to_vec() });
        Ok(())
    }

    pub fn depth(&self) -> usize {
        self.queue.len()
    }

    pub fn trunc_events(&self) -> usize {
        self.trunc_events
    }

    /// recv：buf 不足则截断（MSG_TRUNC），整报长度经 full_len 报告
    pub fn recv(&mut self, buf_cap: usize) -> Result<SeqRead, u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        if self.queue.is_empty() {
            return Err(E_EAGAIN);
        }
        let pkt = self.queue.remove(0);
        let full = pkt.payload.len();
        if buf_cap >= full {
            Ok(SeqRead { data: pkt.payload, truncated: false, full_len: full })
        } else {
            self.trunc_events += 1;
            let mut d = pkt.payload;
            d.truncate(buf_cap);
            Ok(SeqRead { data: d, truncated: true, full_len: full })
        }
    }
}

pub fn run_unx10889_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10889");
    s.add("J1-段锚", SEG_BASE == 0x1c7a0, "seqpacket 截断段基址 0x1c7a0");
    s.add("J2-缓冲足不截", {
        let mut r = SeqRecv::new();
        r.send(b"hello").unwrap();
        let x = r.recv(16).unwrap();
        !x.truncated && x.data == b"hello".to_vec() && x.full_len == 5
    }, "16 缓冲收 5 不截");
    s.add("J3-缓冲不足截", {
        let mut r = SeqRecv::new();
        r.send(b"abcdef").unwrap();
        let x = r.recv(4).unwrap();
        x.truncated && x.data == b"abcd".to_vec() && x.full_len == 6
    }, "4 缓冲收 6 截 abcd full 6");
    s.add("J4-截断事件计", {
        let mut r = SeqRecv::new();
        r.send(b"abcdef").unwrap();
        let _ = r.recv(2).unwrap();
        r.trunc_events() == 1
    }, "一次截断计 1");
    s.add("J5-足收不计数", {
        let mut r = SeqRecv::new();
        r.send(b"ab").unwrap();
        let _ = r.recv(2).unwrap();
        r.trunc_events() == 0
    }, "恰足收零事件");
    s.add("J6-恰足边界", {
        let mut r = SeqRecv::new();
        r.send(b"abc").unwrap();
        let x = r.recv(3).unwrap();
        !x.truncated && x.data.len() == 3
    }, "3=3 恰足不截");
    s.add("J7-空队 EAGAIN", {
        let mut r = SeqRecv::new();
        r.recv(8).err() == Some(E_EAGAIN)
    }, "空队 E_EAGAIN");
    s.add("J8-关后 EBADF", {
        let mut r = SeqRecv::new();
        r.close();
        r.recv(8).err() == Some(E_EBADF)
    }, "关后 E_EBADF");
    s.add("J9-超 4096 发拒", {
        let mut r = SeqRecv::new();
        let big = [0u8; 4097];
        r.send(&big) == Err(E_EMSGSIZE)
    }, "4097 发 E_EMSGSIZE");
    s.add("J10-4096 发过", {
        let mut r = SeqRecv::new();
        let edge = [0u8; 4096];
        r.send(&edge).is_ok() && r.depth() == 1
    }, "4096 发 Ok");
    s.add("J11-整报语义", {
        let mut r = SeqRecv::new();
        r.send(b"msg1").unwrap();
        r.send(b"msg2").unwrap();
        let a = r.recv(64).unwrap();
        let b = r.recv(64).unwrap();
        a.data == b"msg1".to_vec() && b.data == b"msg2".to_vec()
    }, "整报两收不粘包");
    s.add("J12-截断不弃整报账", {
        let mut r = SeqRecv::new();
        r.send(b"12345678").unwrap();
        let x = r.recv(3).unwrap();
        x.data.len() == 3 && x.full_len == 8
    }, "截 3 报 full 8");
    s.add("J13-多截累计", {
        let mut r = SeqRecv::new();
        r.send(b"abcdefgh").unwrap();
        r.send(b"ijklmnop").unwrap();
        let _ = r.recv(1).unwrap();
        let _ = r.recv(1).unwrap();
        r.trunc_events() == 2
    }, "两截累计 2");
    s.add("J14-零缓冲全截", {
        let mut r = SeqRecv::new();
        r.send(b"x").unwrap();
        let x = r.recv(0).unwrap();
        x.truncated && x.data.is_empty() && x.full_len == 1
    }, "0 缓冲空收 full 1");
    s.add("J15-零长报不截", {
        let mut r = SeqRecv::new();
        r.send(b"").unwrap();
        let x = r.recv(0).unwrap();
        !x.truncated && x.full_len == 0
    }, "零长报 0 缓冲不截");
    s.add_dyn("J16-截后队列续读", {
        let mut r = SeqRecv::new();
        r.send(b"aaaabbbb").unwrap();
        let t = r.recv(2).unwrap();
        r.send(b"cc").unwrap();
        let n = r.recv(8).unwrap();
        t.truncated && t.data.len() == 2 && !n.truncated && n.data == b"cc".to_vec()
    }, "截后队续读不截真");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_seqpacket_trunc() {
        let mut r = SeqRecv::new();
        r.send(b"abcdef").unwrap();
        let x = r.recv(3).unwrap();
        assert!(x.truncated);
        assert_eq!(x.data, b"abc".to_vec());
        assert_eq!(x.full_len, 6);
    }

    #[test]
    fn checks_green() {
        let s = run_unx10889_checks();
        assert!(s.all_passed(), "UNX-F10889 判据全绿");
    }
}
