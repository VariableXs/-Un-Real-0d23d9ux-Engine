//! UNX-F10885 sendto 显式地址矩阵（dgram 定向发送）（0x1c720 段）
//! 锚点：UNX-F10885-J1 sendto 显式地址（定向/缺地址/未知端点三态）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c720;
pub const E_ENOENT: u32 = SEG_BASE + 1;
pub const E_EDESTADDRREQ: u32 = SEG_BASE + 2;
pub const E_EISCONN: u32 = SEG_BASE + 3;
pub const E_EINVAL: u32 = SEG_BASE + 4;
pub const E_OK: u32 = SEG_BASE + 5;
pub const E_SEALED: u32 = SEG_BASE + 6;

/// 信箱端点：名字 + 收件箱
pub struct Mailbox {
    name: String,
    inbox: Vec<(String, Vec<u8>)>,
    cap: usize,
}

impl Mailbox {
    pub fn new(name: &str, cap: usize) -> Self {
        Mailbox { name: name.to_string(), inbox: Vec::new(), cap }
    }

    pub fn label(&self) -> &str {
        &self.name
    }

    pub fn depth(&self) -> usize {
        self.inbox.len()
    }

    pub fn deliver(&mut self, from: &str, payload: &[u8]) -> Result<(), u32> {
        if self.inbox.len() >= self.cap {
            return Err(E_EINVAL);
        }
        self.inbox.push((from.to_string(), payload.to_vec()));
        Ok(())
    }

    pub fn take(&mut self) -> Option<(String, Vec<u8>)> {
        if self.inbox.is_empty() {
            None
        } else {
            Some(self.inbox.remove(0))
        }
    }
}

/// 定向发送面
pub struct SendtoSock {
    default_peer: Option<String>,
    open: bool,
}

impl SendtoSock {
    pub fn new() -> Self {
        SendtoSock { default_peer: None, open: true }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn set_default(&mut self, peer: &str) {
        self.default_peer = Some(peer.to_string());
    }

    pub fn has_default(&self) -> bool {
        self.default_peer.is_some()
    }

    /// sendto 判定：关 → 无显式地址且无默认（EDESTADDRREQ）→ 端点未知（ENOENT）→ 投递
    pub fn sendto(
        &self,
        boxes: &mut Vec<Mailbox>,
        explicit: Option<&str>,
        payload: &[u8],
    ) -> Result<(), u32> {
        if !self.open {
            return Err(E_SEALED);
        }
        let target = match explicit {
            Some(a) => a,
            None => match &self.default_peer {
                None => return Err(E_EDESTADDRREQ),
                Some(d) => d.as_str(),
            },
        };
        let mut hit = false;
        let mut i = 0;
        while i < boxes.len() {
            if boxes[i].label() == target {
                hit = true;
                let from = match &self.default_peer {
                    Some(d) => d.as_str(),
                    None => "anon",
                };
                boxes[i].deliver(from, payload)?;
                break;
            }
            i += 1;
        }
        if !hit {
            return Err(E_ENOENT);
        }
        Ok(())
    }
}

pub fn run_unx10885_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10885");
    s.add("J1-段锚", SEG_BASE == 0x1c720, "sendto 定向段基址 0x1c720");
    s.add("J2-无地址无默认拒", {
        let k = SendtoSock::new();
        let mut b = Vec::new();
        b.push(Mailbox::new("m", 4));
        k.sendto(&mut b, None, b"x") == Err(E_EDESTADDRREQ)
    }, "双缺地址 E_EDESTADDRREQ");
    s.add("J3-显式地址投递", {
        let k = SendtoSock::new();
        let mut b = Vec::new();
        b.push(Mailbox::new("m", 4));
        k.sendto(&mut b, Some("m"), b"hi").is_ok() && b[0].depth() == 1
    }, "定向 m 投 1");
    s.add("J4-未知端点拒", {
        let k = SendtoSock::new();
        let mut b = Vec::new();
        b.push(Mailbox::new("m", 4));
        k.sendto(&mut b, Some("ghost"), b"hi") == Err(E_ENOENT)
    }, "未知端点 E_ENOENT");
    s.add("J5-默认地址投递", {
        let mut k = SendtoSock::new();
        k.set_default("m");
        let mut b = Vec::new();
        b.push(Mailbox::new("m", 4));
        k.sendto(&mut b, None, b"hi").is_ok() && b[0].depth() == 1
    }, "默认对端投 1");
    s.add("J6-显式覆盖默认", {
        let mut k = SendtoSock::new();
        k.set_default("m");
        let mut b = Vec::new();
        b.push(Mailbox::new("m", 4));
        b.push(Mailbox::new("n", 4));
        k.sendto(&mut b, Some("n"), b"hi").is_ok();
        b[0].depth() == 0 && b[1].depth() == 1
    }, "显式 n 优先投 n");
    s.add("J7-发方名匿名", {
        let k = SendtoSock::new();
        let mut b = Vec::new();
        b.push(Mailbox::new("m", 4));
        k.sendto(&mut b, Some("m"), b"x").unwrap();
        let (from, _) = b[0].take().unwrap();
        from == "anon"
    }, "无默认发方 anon");
    s.add("J8-发方名默认", {
        let mut k = SendtoSock::new();
        k.set_default("m");
        let mut b = Vec::new();
        b.push(Mailbox::new("m", 4));
        k.sendto(&mut b, None, b"x").unwrap();
        let (from, _) = b[0].take().unwrap();
        from == "m"
    }, "默认对端发方 m");
    s.add("J9-载荷原样", {
        let k = SendtoSock::new();
        let mut b = Vec::new();
        b.push(Mailbox::new("m", 4));
        k.sendto(&mut b, Some("m"), b"payload").unwrap();
        let (_, p) = b[0].take().unwrap();
        p == b"payload".to_vec()
    }, "载荷回读全中");
    s.add("J10-收箱满拒", {
        let k = SendtoSock::new();
        let mut b = Vec::new();
        b.push(Mailbox::new("m", 1));
        k.sendto(&mut b, Some("m"), b"1").unwrap();
        k.sendto(&mut b, Some("m"), b"2") == Err(E_EINVAL)
    }, "箱满 E_EINVAL");
    s.add("J11-关闭后拒发", {
        let mut k = SendtoSock::new();
        k.close();
        let mut b = Vec::new();
        b.push(Mailbox::new("m", 4));
        k.sendto(&mut b, Some("m"), b"x") == Err(E_SEALED)
    }, "关后 E_SEALED");
    s.add("J12-默认可查", {
        let mut k = SendtoSock::new();
        !k.has_default();
        k.set_default("m");
        k.has_default()
    }, "设默认后 has_default 真");
    s.add("J13-多端点路由", {
        let k = SendtoSock::new();
        let mut b = Vec::new();
        b.push(Mailbox::new("a", 4));
        b.push(Mailbox::new("b", 4));
        b.push(Mailbox::new("c", 4));
        k.sendto(&mut b, Some("b"), b"x").unwrap();
        b[0].depth() == 0 && b[1].depth() == 1 && b[2].depth() == 0
    }, "三端点只 b 中");
    s.add("J14-连投两报", {
        let k = SendtoSock::new();
        let mut b = Vec::new();
        b.push(Mailbox::new("m", 4));
        k.sendto(&mut b, Some("m"), b"1").unwrap();
        k.sendto(&mut b, Some("m"), b"2").unwrap();
        b[0].depth() == 2
    }, "连投 depth 2");
    s.add("J15-零长载荷投递", {
        let k = SendtoSock::new();
        let mut b = Vec::new();
        b.push(Mailbox::new("m", 4));
        k.sendto(&mut b, Some("m"), b"").is_ok();
        let (_, p) = b[0].take().unwrap();
        p.is_empty()
    }, "零长载荷回空");
    s.add_dyn("J16-定向序账", {
        let k = SendtoSock::new();
        let mut b = Vec::new();
        b.push(Mailbox::new("m", 4));
        let r1 = k.sendto(&mut b, None, b"x");
        let r2 = k.sendto(&mut b, Some("m"), b"x");
        let r3 = k.sendto(&mut b, Some("z"), b"x");
        r1.is_err() && r2.is_ok() && r3.is_err()
    }, "DESTADDRREQ→Ok→ENOENT 序账");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_sendto_addr() {
        let k = SendtoSock::new();
        let mut b = Vec::new();
        b.push(Mailbox::new("m", 4));
        assert_eq!(k.sendto(&mut b, None, b"x"), Err(E_EDESTADDRREQ));
        assert!(k.sendto(&mut b, Some("m"), b"hi").is_ok());
    }

    #[test]
    fn checks_green() {
        let s = run_unx10885_checks();
        assert!(s.all_passed(), "UNX-F10885 判据全绿");
    }
}
