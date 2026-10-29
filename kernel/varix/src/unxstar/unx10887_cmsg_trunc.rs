//! UNX-F10887 cmsg 截断矩阵（MSG_CTRUNC）（0x1c760 段）
//! 锚点：UNX-F10887-J1 cmsg 截断（MSG_CTRUNC 置位/不置位分界）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c760;
pub const E_EINVAL: u32 = SEG_BASE + 1;
pub const E_ENOMEM: u32 = SEG_BASE + 2;
pub const E_EBADF: u32 = SEG_BASE + 3;
pub const E_OK: u32 = SEG_BASE + 4;
pub const E_SEALED: u32 = SEG_BASE + 5;
pub const E_TRUNC: u32 = SEG_BASE + 6;

pub const CTRL_BUF_MAX: usize = 64;

/// 控制消息：类型 + 载荷字节
#[derive(Clone, Debug, PartialEq)]
pub struct Cmsg {
    pub level: u16,
    pub typ: u16,
    pub data: Vec<u8>,
}

impl Cmsg {
    pub fn wire_len(&self) -> usize {
        4 + self.data.len()
    }
}

/// 控制面收发器：接收缓冲有限 → 截断置 MSG_CTRUNC
pub struct CtrlChannel {
    recv_buf_cap: usize,
    open: bool,
}

pub struct RecvResult {
    pub delivered: Vec<Cmsg>,
    pub truncated: bool,
    pub bytes_kept: usize,
}

impl CtrlChannel {
    pub fn new(cap: usize) -> Self {
        CtrlChannel { recv_buf_cap: cap, open: true }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn cap_of(&self) -> usize {
        self.recv_buf_cap
    }

    /// 接收：逐条装填直到缓冲满 → 置截断
    pub fn recv_ctrl(&self, msgs: &[Cmsg]) -> Result<RecvResult, u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        let mut kept: Vec<Cmsg> = Vec::new();
        let mut used = 0;
        let mut truncated = false;
        let mut i = 0;
        while i < msgs.len() {
            let w = msgs[i].wire_len();
            if used + w > self.recv_buf_cap {
                truncated = true;
                break;
            }
            used += w;
            kept.push(msgs[i].clone());
            i += 1;
        }
        Ok(RecvResult { delivered: kept, truncated, bytes_kept: used })
    }
}

pub fn run_unx10887_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10887");
    s.add("J1-段锚", SEG_BASE == 0x1c760, "cmsg 截断段基址 0x1c760");
    s.add("J2-缓冲锚", CTRL_BUF_MAX == 64, "控制缓冲 64");
    s.add("J3-wire 长公式", {
        let c = Cmsg { level: 1, typ: 1, data: vec![0u8; 12] };
        c.wire_len() == 16
    }, "4+12=16");
    s.add("J4-零载荷 wire", {
        let c = Cmsg { level: 1, typ: 1, data: Vec::new() };
        c.wire_len() == 4
    }, "4+0=4");
    s.add("J5-单条恰满不截", {
        let ch = CtrlChannel::new(16);
        let c = Cmsg { level: 1, typ: 1, data: vec![0u8; 12] };
        let r = ch.recv_ctrl(&[c]).unwrap();
        !r.truncated && r.delivered.len() == 1 && r.bytes_kept == 16
    }, "恰满 16 不截");
    s.add("J6-超 1 字节截", {
        let ch = CtrlChannel::new(16);
        let c = Cmsg { level: 1, typ: 1, data: vec![0u8; 13] };
        let r = ch.recv_ctrl(&[c]).unwrap();
        r.truncated && r.delivered.is_empty() && r.bytes_kept == 0
    }, "17>16 截且零交付");
    s.add("J7-两报一截", {
        let ch = CtrlChannel::new(16);
        let a = Cmsg { level: 1, typ: 1, data: vec![0u8; 12] };
        let b = Cmsg { level: 1, typ: 2, data: vec![0u8; 4] };
        let r = ch.recv_ctrl(&[a, b]).unwrap();
        r.truncated && r.delivered.len() == 1
    }, "16+8>16 留一截二");
    s.add("J8-两报恰满不截", {
        let ch = CtrlChannel::new(16);
        let a = Cmsg { level: 1, typ: 1, data: vec![0u8; 4] };
        let b = Cmsg { level: 1, typ: 2, data: vec![0u8; 8] };
        let r = ch.recv_ctrl(&[a, b]).unwrap();
        !r.truncated && r.delivered.len() == 2 && r.bytes_kept == 16
    }, "8+12=20？4+4 与 4+8=16 全收");
    s.add("J9-空集不截", {
        let ch = CtrlChannel::new(16);
        let r = ch.recv_ctrl(&[]).unwrap();
        !r.truncated && r.delivered.is_empty() && r.bytes_kept == 0
    }, "空集零截");
    s.add("J10-大缓冲宽裕", {
        let ch = CtrlChannel::new(CTRL_BUF_MAX);
        let mut set = Vec::new();
        let mut n = 0;
        while n < 8 {
            set.push(Cmsg { level: 1, typ: n as u16, data: vec![0u8; 4] });
            n += 1;
        }
        let r = ch.recv_ctrl(&set).unwrap();
        !r.truncated && r.delivered.len() == 8
    }, "8×8=64 恰满全收");
    s.add("J11-大缓冲溢出", {
        let ch = CtrlChannel::new(CTRL_BUF_MAX);
        let mut set = Vec::new();
        let mut n = 0;
        while n < 9 {
            set.push(Cmsg { level: 1, typ: n as u16, data: vec![0u8; 4] });
            n += 1;
        }
        let r = ch.recv_ctrl(&set).unwrap();
        r.truncated && r.delivered.len() == 8
    }, "9 条留 8 截尾");
    s.add("J12-恰满不截", {
        let ch = CtrlChannel::new(8);
        let a = Cmsg { level: 1, typ: 1, data: vec![0u8; 4] };
        let r = ch.recv_ctrl(&[a]).unwrap();
        r.delivered.len() == 1 && !r.truncated && r.bytes_kept == 8
    }, "8 缓冲单条 8 恰满不截");
    s.add("J13-关闭拒收", {
        let mut ch = CtrlChannel::new(16);
        ch.close();
        ch.recv_ctrl(&[]).err() == Some(E_EBADF)
    }, "关后 E_EBADF");
    s.add("J14-容量可读", {
        let a = CtrlChannel::new(16);
        let b = CtrlChannel::new(64);
        a.cap_of() == 16 && b.cap_of() == 64
    }, "两容量可读");
    s.add("J15-截位精确", {
        let ch = CtrlChannel::new(12);
        let a = Cmsg { level: 1, typ: 1, data: vec![0u8; 4] };
        let b = Cmsg { level: 1, typ: 2, data: vec![0u8; 4] };
        let r = ch.recv_ctrl(&[a, b]).unwrap();
        r.truncated && r.delivered.len() == 1 && r.bytes_kept == 8
    }, "12 缓冲两报留一截二 kept 8");
    s.add_dyn("J16-截断标记账", {
        let ch = CtrlChannel::new(16);
        let a = Cmsg { level: 1, typ: 1, data: vec![0u8; 12] };
        let b = Cmsg { level: 1, typ: 2, data: vec![0u8; 4] };
        let r1 = ch.recv_ctrl(&[a.clone()]).unwrap();
        let r2 = ch.recv_ctrl(&[a, b]).unwrap();
        !r1.truncated && r2.truncated && r2.bytes_kept == 16
    }, "16 单条不截/两条 24 截 16 账");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_cmsg_trunc() {
        let ch = CtrlChannel::new(16);
        let a = Cmsg { level: 1, typ: 1, data: vec![0u8; 12] };
        let r = ch.recv_ctrl(&[a]).unwrap();
        assert!(!r.truncated);
    }

    #[test]
    fn checks_green() {
        let s = run_unx10887_checks();
        assert!(s.all_passed(), "UNX-F10887 判据全绿");
    }
}
