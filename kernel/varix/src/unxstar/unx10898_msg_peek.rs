//! UNX-F10898 MSG_PEEK 错误矩阵（窥视不消费）（0x1c8c0 段）
//! 锚点：UNX-F10898-J1 MSG_PEEK 窥视（不消费/连窥一致/空窥 EAGAIN）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c8c0;
pub const E_EAGAIN: u32 = SEG_BASE + 1;
pub const E_EBADF: u32 = SEG_BASE + 2;
pub const E_EINVAL: u32 = SEG_BASE + 3;
pub const E_OK: u32 = SEG_BASE + 4;
pub const E_SEALED: u32 = SEG_BASE + 5;

/// 收报队列 + PEEK 面
pub struct PeekSock {
    queue: Vec<Vec<u8>>,
    open: bool,
    peek_hits: usize,
}

pub struct ReadOut {
    pub data: Vec<u8>,
    pub consumed: bool,
}

impl PeekSock {
    pub fn new() -> Self {
        PeekSock { queue: Vec::new(), open: true, peek_hits: 0 }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn push(&mut self, data: &[u8]) -> Result<(), u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        self.queue.push(data.to_vec());
        Ok(())
    }

    pub fn depth(&self) -> usize {
        self.queue.len()
    }

    pub fn peek_hits(&self) -> usize {
        self.peek_hits
    }

    /// recv：peek=true 窥不消费；peek=false 消费
    pub fn recv(&mut self, peek: bool) -> Result<ReadOut, u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        if self.queue.is_empty() {
            return Err(E_EAGAIN);
        }
        if peek {
            self.peek_hits += 1;
            Ok(ReadOut { data: self.queue[0].clone(), consumed: false })
        } else {
            let d = self.queue.remove(0);
            Ok(ReadOut { data: d, consumed: true })
        }
    }

    /// 连窥 k 次同一报
    pub fn peek_k(&mut self, k: usize) -> Result<Vec<Vec<u8>>, u32> {
        let mut outs = Vec::new();
        let mut n = 0;
        while n < k {
            outs.push(self.recv(true)?.data);
            n += 1;
        }
        Ok(outs)
    }
}

pub fn run_unx10898_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10898");
    s.add("J1-段锚", SEG_BASE == 0x1c8c0, "MSG_PEEK 段基址 0x1c8c0");
    s.add("J2-窥视不消费", {
        let mut k = PeekSock::new();
        k.push(b"m").unwrap();
        let r = k.recv(true).unwrap();
        r.data == b"m".to_vec() && !r.consumed && k.depth() == 1
    }, "窥后 depth 1");
    s.add("J3-消费读", {
        let mut k = PeekSock::new();
        k.push(b"m").unwrap();
        let r = k.recv(false).unwrap();
        r.consumed && k.depth() == 0
    }, "消费后 depth 0");
    s.add("J4-空窥 EAGAIN", {
        let mut k = PeekSock::new();
        k.recv(true).err() == Some(E_EAGAIN)
    }, "空队窥 E_EAGAIN");
    s.add("J5-空读 EAGAIN", {
        let mut k = PeekSock::new();
        k.recv(false).err() == Some(E_EAGAIN)
    }, "空队读 E_EAGAIN");
    s.add("J6-连窥一致", {
        let mut k = PeekSock::new();
        k.push(b"same").unwrap();
        let outs = k.peek_k(3).unwrap();
        outs.len() == 3
            && outs[0] == b"same".to_vec()
            && outs[1] == b"same".to_vec()
            && outs[2] == b"same".to_vec()
            && k.depth() == 1
    }, "三窥同文 depth 1");
    s.add("J7-窥计数", {
        let mut k = PeekSock::new();
        k.push(b"x").unwrap();
        k.peek_k(4).unwrap();
        k.peek_hits() == 4
    }, "四窥计 4");
    s.add("J8-窥后消费得同报", {
        let mut k = PeekSock::new();
        k.push(b"peeked").unwrap();
        let p = k.recv(true).unwrap();
        let c = k.recv(false).unwrap();
        p.data == c.data && c.consumed
    }, "窥读同文消费真");
    s.add("J9-窥不改序", {
        let mut k = PeekSock::new();
        k.push(b"first").unwrap();
        k.push(b"second").unwrap();
        let p = k.recv(true).unwrap();
        let c1 = k.recv(false).unwrap();
        let c2 = k.recv(false).unwrap();
        p.data == b"first".to_vec()
            && c1.data == b"first".to_vec()
            && c2.data == b"second".to_vec()
    }, "窥 first 读 first/second");
    s.add("J10-关后窥拒", {
        let mut k = PeekSock::new();
        k.push(b"x").unwrap();
        k.close();
        k.recv(true).err() == Some(E_EBADF)
    }, "关后 E_EBADF");
    s.add("J11-关后推拒", {
        let mut k = PeekSock::new();
        k.close();
        k.push(b"x") == Err(E_EBADF)
    }, "关后推 E_EBADF");
    s.add("J12-窥后队深不变账", {
        let mut k = PeekSock::new();
        k.push(b"a").unwrap();
        k.push(b"b").unwrap();
        let _ = k.recv(true).unwrap();
        let _ = k.recv(true).unwrap();
        k.depth() == 2
    }, "双窥 depth 2");
    s.add("J13-零长报窥", {
        let mut k = PeekSock::new();
        k.push(b"").unwrap();
        let r = k.recv(true).unwrap();
        r.data.is_empty() && !r.consumed
    }, "零长窥 Ok 空文");
    s.add("J14-窥尽后消费清", {
        let mut k = PeekSock::new();
        k.push(b"only").unwrap();
        k.peek_k(5).unwrap();
        k.recv(false).unwrap();
        k.depth() == 0 && k.recv(true).err() == Some(E_EAGAIN)
    }, "五窥一消后空 E_EAGAIN");
    s.add("J15-窥计数与读序无关", {
        let mut k = PeekSock::new();
        k.push(b"z").unwrap();
        k.recv(false).unwrap();
        k.peek_hits() == 0
    }, "消费读不计窥");
    s.add_dyn("J16-窥读交错账", {
        let mut k = PeekSock::new();
        k.push(b"1").unwrap();
        k.push(b"2").unwrap();
        let p1 = k.recv(true).unwrap();
        let c1 = k.recv(false).unwrap();
        let p2 = k.recv(true).unwrap();
        p1.data == b"1".to_vec() && c1.consumed && p2.data == b"2".to_vec()
    }, "窥1-消1-窥2 全真账");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_msg_peek() {
        let mut k = PeekSock::new();
        k.push(b"m").unwrap();
        let p = k.recv(true).unwrap();
        assert!(!p.consumed);
        assert_eq!(k.depth(), 1);
        let c = k.recv(false).unwrap();
        assert!(c.consumed);
    }

    #[test]
    fn checks_green() {
        let s = run_unx10898_checks();
        assert!(s.all_passed(), "UNX-F10898 判据全绿");
    }
}
