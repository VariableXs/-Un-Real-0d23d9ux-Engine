//! UNX-F10886 recvfrom 地址快照矩阵（匿名对端零长）（0x1c740 段）
//! 锚点：UNX-F10886-J1 recvfrom 地址快照（匿名零长/具名/快照一致性）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c740;
pub const E_EAGAIN: u32 = SEG_BASE + 1;
pub const E_EBADF: u32 = SEG_BASE + 2;
pub const E_EINVAL: u32 = SEG_BASE + 3;
pub const E_OK: u32 = SEG_BASE + 4;
pub const E_SEALED: u32 = SEG_BASE + 5;

/// 收到的报文：载荷 + 发方地址快照
#[derive(Clone, Debug, PartialEq)]
pub struct Datagram {
    pub from: String,
    pub payload: Vec<u8>,
}

impl Datagram {
    pub fn anonymous() -> Self {
        Datagram { from: String::new(), payload: Vec::new() }
    }
    pub fn is_anonymous(&self) -> bool {
        self.from.is_empty() && self.payload.is_empty()
    }
}

/// 接收面：报文队列 + recvfrom 快照语义
pub struct RecvfromSock {
    queue: Vec<Datagram>,
    open: bool,
}

impl RecvfromSock {
    pub fn new() -> Self {
        RecvfromSock { queue: Vec::new(), open: true }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn inject(&mut self, d: Datagram) -> Result<(), u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        self.queue.push(d);
        Ok(())
    }

    pub fn depth(&self) -> usize {
        self.queue.len()
    }

    /// recvfrom：空 → EAGAIN；出队返回快照
    pub fn recvfrom(&mut self) -> Result<Datagram, u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        if self.queue.is_empty() {
            return Err(E_EAGAIN);
        }
        Ok(self.queue.remove(0))
    }

    /// 快照一致性：同源连发两报地址快照应同
    pub fn snapshot_pair(&mut self) -> Option<(String, String)> {
        let a = self.recvfrom().ok()?;
        let b = self.recvfrom().ok()?;
        Some((a.from, b.from))
    }
}

pub fn run_unx10886_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10886");
    s.add("J1-段锚", SEG_BASE == 0x1c740, "recvfrom 快照段基址 0x1c740");
    s.add("J2-匿名构造", {
        let d = Datagram::anonymous();
        d.is_anonymous()
    }, "匿名零长真");
    s.add("J3-空队 EAGAIN", {
        let mut k = RecvfromSock::new();
        k.recvfrom() == Err(E_EAGAIN)
    }, "空队 E_EAGAIN");
    s.add("J4-具名报快照", {
        let mut k = RecvfromSock::new();
        k.inject(Datagram { from: "/sender".to_string(), payload: b"hi".to_vec() }).unwrap();
        let d = k.recvfrom().unwrap();
        d.from == "/sender" && d.payload == b"hi".to_vec()
    }, "具名快照全中");
    s.add("J5-匿名报快照", {
        let mut k = RecvfromSock::new();
        k.inject(Datagram::anonymous()).unwrap();
        let d = k.recvfrom().unwrap();
        d.is_anonymous()
    }, "匿名快照回收");
    s.add("J6-快照成对一致", {
        let mut k = RecvfromSock::new();
        k.inject(Datagram { from: "/s".to_string(), payload: b"1".to_vec() }).unwrap();
        k.inject(Datagram { from: "/s".to_string(), payload: b"2".to_vec() }).unwrap();
        k.snapshot_pair() == Some(("/s".to_string(), "/s".to_string()))
    }, "同源两报快照同");
    s.add("J7-异源快照异", {
        let mut k = RecvfromSock::new();
        k.inject(Datagram { from: "/a".to_string(), payload: b"1".to_vec() }).unwrap();
        k.inject(Datagram { from: "/b".to_string(), payload: b"2".to_vec() }).unwrap();
        k.snapshot_pair() == Some(("/a".to_string(), "/b".to_string()))
    }, "异源快照异");
    s.add("J8-关后 EAGAIN 升 EBADF", {
        let mut k = RecvfromSock::new();
        k.close();
        k.recvfrom() == Err(E_EBADF)
    }, "关后 E_EBADF");
    s.add("J9-关后注入拒", {
        let mut k = RecvfromSock::new();
        k.close();
        k.inject(Datagram::anonymous()) == Err(E_EBADF)
    }, "关后注入 E_EBADF");
    s.add("J10-注入计数", {
        let mut k = RecvfromSock::new();
        let mut n = 0;
        while n < 3 {
            k.inject(Datagram::anonymous()).unwrap();
            n += 1;
        }
        k.depth() == 3
    }, "三注 depth 3");
    s.add("J11-取空再取 EAGAIN", {
        let mut k = RecvfromSock::new();
        k.inject(Datagram::anonymous()).unwrap();
        k.recvfrom().unwrap();
        k.recvfrom() == Err(E_EAGAIN)
    }, "取空后 E_EAGAIN");
    s.add("J12-FIFO 序", {
        let mut k = RecvfromSock::new();
        k.inject(Datagram { from: "/1".to_string(), payload: b"a".to_vec() }).unwrap();
        k.inject(Datagram { from: "/2".to_string(), payload: b"b".to_vec() }).unwrap();
        let d1 = k.recvfrom().unwrap();
        let d2 = k.recvfrom().unwrap();
        d1.from == "/1" && d2.from == "/2"
    }, "两报 FIFO 全中");
    s.add("J13-快照克隆等价", {
        let d = Datagram { from: "/x".to_string(), payload: b"p".to_vec() };
        let c = d.clone();
        d == c
    }, "克隆相等");
    s.add("J14-匿名与具名不等", {
        let a = Datagram::anonymous();
        let b = Datagram { from: "/x".to_string(), payload: Vec::new() };
        a != b
    }, "匿名异于具名空载");
    s.add("J15-混合队列快照序", {
        let mut k = RecvfromSock::new();
        k.inject(Datagram::anonymous()).unwrap();
        k.inject(Datagram { from: "/n".to_string(), payload: b"z".to_vec() }).unwrap();
        let d1 = k.recvfrom().unwrap();
        let d2 = k.recvfrom().unwrap();
        d1.is_anonymous() && d2.from == "/n"
    }, "匿-具序全中");
    s.add_dyn("J16-三态快照账", {
        let mut k = RecvfromSock::new();
        let r0 = k.recvfrom();
        k.inject(Datagram::anonymous()).unwrap();
        let r1 = k.recvfrom();
        k.inject(Datagram { from: "/p".to_string(), payload: b"q".to_vec() }).unwrap();
        let r2 = k.recvfrom();
        r0.is_err() && r1.is_ok() && r2.is_ok()
    }, "EAGAIN→匿名 Ok→具名 Ok 账");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_recvfrom_snapshot() {
        let mut k = RecvfromSock::new();
        assert_eq!(k.recvfrom(), Err(E_EAGAIN));
        k.inject(Datagram { from: "/s".to_string(), payload: b"p".to_vec() }).unwrap();
        assert_eq!(k.recvfrom().unwrap().from, "/s");
    }

    #[test]
    fn checks_green() {
        let s = run_unx10886_checks();
        assert!(s.all_passed(), "UNX-F10886 判据全绿");
    }
}
