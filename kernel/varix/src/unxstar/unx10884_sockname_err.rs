//! UNX-F10884 getsockname/getpeername 未绑定矩阵（0x1c700 段）
//! 锚点：UNX-F10884-J1 sockname/peername 未绑定面（匿名/未连/绑后三态）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c700;
pub const E_ENOENT: u32 = SEG_BASE + 1;
pub const E_ENOTCONN: u32 = SEG_BASE + 2;
pub const E_EINVAL: u32 = SEG_BASE + 3;
pub const E_EBADF: u32 = SEG_BASE + 4;
pub const E_OK: u32 = SEG_BASE + 5;
pub const E_SEALED: u32 = SEG_BASE + 6;
pub const E_EISCONN: u32 = SEG_BASE + 7;

/// 套接字地址面：本地名 / 对端名 / 连接态
pub struct NameSock {
    open: bool,
    local: Option<String>,
    peer: Option<String>,
    connected: bool,
}

impl NameSock {
    pub fn new() -> Self {
        NameSock { open: true, local: None, peer: None, connected: false }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn bind(&mut self, name: &str) -> Result<(), u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        if name.is_empty() {
            return Err(E_EINVAL);
        }
        if self.local.is_some() {
            return Err(E_EINVAL);
        }
        self.local = Some(name.to_string());
        Ok(())
    }

    pub fn connect_to(&mut self, peer: &str) -> Result<(), u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        if peer.is_empty() {
            return Err(E_EINVAL);
        }
        if self.connected {
            return Err(E_EISCONN);
        }
        self.peer = Some(peer.to_string());
        self.connected = true;
        Ok(())
    }

    /// getsockname：未绑 → ENOENT（匿名语义）
    pub fn getsockname(&self) -> Result<&str, u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        match &self.local {
            None => Err(E_ENOENT),
            Some(n) => Ok(n.as_str()),
        }
    }

    /// getpeername：未连 → ENOTCONN
    pub fn getpeername(&self) -> Result<&str, u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        if !self.connected {
            return Err(E_ENOTCONN);
        }
        match &self.peer {
            None => Err(E_ENOTCONN),
            Some(n) => Ok(n.as_str()),
        }
    }

    pub fn is_bound(&self) -> bool {
        self.local.is_some()
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }
}

pub fn run_unx10884_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10884");
    s.add("J1-段锚", SEG_BASE == 0x1c700, "sockname 段基址 0x1c700");
    s.add("J2-新套未绑", {
        let k = NameSock::new();
        !k.is_bound() && !k.is_connected()
    }, "新套未绑未连");
    s.add("J3-未绑 sockname ENOENT", {
        let k = NameSock::new();
        k.getsockname() == Err(E_ENOENT)
    }, "匿名 E_ENOENT");
    s.add("J4-未连 peername ENOTCONN", {
        let k = NameSock::new();
        k.getpeername() == Err(E_ENOTCONN)
    }, "未连 E_ENOTCONN");
    s.add("J5-绑后 sockname 中", {
        let mut k = NameSock::new();
        k.bind("/tmp/me").unwrap();
        k.getsockname() == Ok("/tmp/me")
    }, "绑后名回读");
    s.add("J6-连后 peername 中", {
        let mut k = NameSock::new();
        k.connect_to("/tmp/peer").unwrap();
        k.getpeername() == Ok("/tmp/peer")
    }, "连后对端名回读");
    s.add("J7-重绑拒", {
        let mut k = NameSock::new();
        k.bind("/a").unwrap();
        k.bind("/b") == Err(E_EINVAL)
    }, "二次 bind E_EINVAL");
    s.add("J8-空名绑拒", {
        let mut k = NameSock::new();
        k.bind("") == Err(E_EINVAL)
    }, "空名 E_EINVAL");
    s.add("J9-空对端拒", {
        let mut k = NameSock::new();
        k.connect_to("") == Err(E_EINVAL)
    }, "空 peer E_EINVAL");
    s.add("J10-重连拒", {
        let mut k = NameSock::new();
        k.connect_to("/p").unwrap();
        k.connect_to("/q") == Err(E_EISCONN)
    }, "重连 E_EISCONN");
    s.add("J11-关闭后 sockname EBADF", {
        let mut k = NameSock::new();
        k.close();
        k.getsockname() == Err(E_EBADF)
    }, "关后 E_EBADF");
    s.add("J12-关闭后 peername EBADF", {
        let mut k = NameSock::new();
        k.bind("/a").unwrap();
        k.connect_to("/b").unwrap();
        k.close();
        k.getpeername() == Err(E_EBADF)
    }, "绑连齐关后 E_EBADF");
    s.add("J13-EBADF 先于匿名", {
        let mut k = NameSock::new();
        k.close();
        k.getsockname() == Err(E_EBADF)
    }, "关+匿名 EBADF 优先");
    s.add("J14-绑不隐含连", {
        let mut k = NameSock::new();
        k.bind("/a").unwrap();
        k.is_bound() && !k.is_connected() && k.getpeername() == Err(E_ENOTCONN)
    }, "绑态连面仍拒");
    s.add("J15-连不隐含绑", {
        let mut k = NameSock::new();
        k.connect_to("/b").unwrap();
        k.is_connected() && !k.is_bound() && k.getsockname() == Err(E_ENOENT)
    }, "连态名面仍匿名");
    s.add_dyn("J16-全态账", {
        let mut k = NameSock::new();
        let a_ok = k.getsockname() == Err(E_ENOENT);
        k.bind("/x").unwrap();
        let b_ok = k.getsockname().is_ok();
        k.connect_to("/y").unwrap();
        let c_ok = k.getpeername().is_ok();
        a_ok && b_ok && c_ok
    }, "ENOENT→Ok(/x)→Ok(/y) 全账");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_name_sock() {
        let mut k = NameSock::new();
        assert_eq!(k.getsockname(), Err(E_ENOENT));
        k.bind("/me").unwrap();
        assert_eq!(k.getsockname(), Ok("/me"));
        k.connect_to("/peer").unwrap();
        assert_eq!(k.getpeername(), Ok("/peer"));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10884_checks();
        assert!(s.all_passed(), "UNX-F10884 判据全绿");
    }
}
