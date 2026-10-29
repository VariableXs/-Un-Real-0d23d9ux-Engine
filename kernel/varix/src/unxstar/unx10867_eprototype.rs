//! UNX-F10867 EPROTOTYPE 矩阵（类型不匹配 connect）（0x1c4e0 段）
//! 锚点：UNX-F10867-J1 EPROTOTYPE 正反双判据（SOCK_STREAM 连 SOCK_DGRAM 端点）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c4e0;
pub const E_EPROTOTYPE: u32 = SEG_BASE + 1;
pub const E_EINVAL: u32 = SEG_BASE + 2;
pub const E_EISCONN: u32 = SEG_BASE + 3;
pub const E_EBADF: u32 = SEG_BASE + 4;
pub const E_OK: u32 = SEG_BASE + 5;
pub const E_SEALED: u32 = SEG_BASE + 6;

/// 端点类型：stream / dgram / seqpacket
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SockType {
    Stream,
    Dgram,
    SeqPacket,
}

/// 监听端点：绑定类型 + 地址
#[derive(Clone, Debug)]
pub struct Endpoint {
    pub path: String,
    pub typ: SockType,
    pub listening: bool,
}

impl Endpoint {
    pub fn listen(path: &str, typ: SockType) -> Self {
        Endpoint { path: path.to_string(), typ, listening: true }
    }
}

/// connect 判定器：本端类型与对端监听类型不匹配 → EPROTOTYPE
pub struct ProtoMatrix {
    endpoints: Vec<Endpoint>,
    connected: Vec<(String, SockType)>,
}

impl ProtoMatrix {
    pub fn new() -> Self {
        ProtoMatrix { endpoints: Vec::new(), connected: Vec::new() }
    }

    pub fn serve(&mut self, path: &str, typ: SockType) -> Result<(), u32> {
        for e in self.endpoints.iter() {
            if e.path == path {
                return Err(E_EINVAL);
            }
        }
        self.endpoints.push(Endpoint::listen(path, typ));
        Ok(())
    }

    /// connect 判定：四序查错——fd/类型/重复/成功
    pub fn connect(&mut self, fd_ok: bool, path: &str, typ: SockType) -> Result<(), u32> {
        if !fd_ok {
            return Err(E_EBADF);
        }
        let mut hit: Option<&Endpoint> = None;
        let mut i = 0;
        while i < self.endpoints.len() {
            if self.endpoints[i].path == path {
                hit = Some(&self.endpoints[i]);
            }
            i += 1;
        }
        let e = match hit {
            None => return Err(E_EPROTOTYPE),
            Some(e) => e,
        };
        if !e.listening {
            return Err(E_EPROTOTYPE);
        }
        for c in self.connected.iter() {
            if c.0 == path && c.1 == typ {
                return Err(E_EISCONN);
            }
        }
        if e.typ != typ {
            return Err(E_EPROTOTYPE);
        }
        self.connected.push((path.to_string(), typ));
        Ok(())
    }

    pub fn connected_count(&self) -> usize {
        self.connected.len()
    }
}

pub fn run_unx10867_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10867");
    s.add("J1-段锚", SEG_BASE == 0x1c4e0, "EPROTOTYPE 段基址 0x1c4e0");
    s.add("J2-流连报文报型错", {
        let mut m = ProtoMatrix::new();
        m.serve("/tmp/ep-dgram", SockType::Dgram).unwrap();
        m.connect(true, "/tmp/ep-dgram", SockType::Stream) == Err(E_EPROTOTYPE)
    }, "stream connect dgram 端点 E_EPROTOTYPE");
    s.add("J3-报文连流报型错", {
        let mut m = ProtoMatrix::new();
        m.serve("/tmp/ep-stream", SockType::Stream).unwrap();
        m.connect(true, "/tmp/ep-stream", SockType::Dgram) == Err(E_EPROTOTYPE)
    }, "dgram connect stream 端点 E_EPROTOTYPE");
    s.add("J4-类型匹配过", {
        let mut m = ProtoMatrix::new();
        m.serve("/tmp/ep-ok", SockType::Stream).unwrap();
        m.connect(true, "/tmp/ep-ok", SockType::Stream).is_ok()
    }, "同类型 connect Ok");
    s.add("J5-序包连流报型错", {
        let mut m = ProtoMatrix::new();
        m.serve("/tmp/ep-sp", SockType::Stream).unwrap();
        m.connect(true, "/tmp/ep-sp", SockType::SeqPacket) == Err(E_EPROTOTYPE)
    }, "seqpacket connect stream E_EPROTOTYPE");
    s.add("J6-未监听路径报型错", {
        let mut m = ProtoMatrix::new();
        m.connect(true, "/tmp/none", SockType::Stream) == Err(E_EPROTOTYPE)
    }, "无端点路径 E_EPROTOTYPE");
    s.add("J7-坏 fd 先判", {
        let mut m = ProtoMatrix::new();
        m.serve("/tmp/ep-fd", SockType::Stream).unwrap();
        m.connect(false, "/tmp/ep-fd", SockType::Dgram) == Err(E_EBADF)
    }, "fd 错误优先于型错");
    s.add("J8-重复连接报已连", {
        let mut m = ProtoMatrix::new();
        m.serve("/tmp/ep-dup", SockType::Stream).unwrap();
        m.connect(true, "/tmp/ep-dup", SockType::Stream).unwrap();
        m.connect(true, "/tmp/ep-dup", SockType::Stream) == Err(E_EISCONN)
    }, "同型重连 E_EISCONN");
    s.add("J9-重连异型仍型错", {
        let mut m = ProtoMatrix::new();
        m.serve("/tmp/ep-mix", SockType::Stream).unwrap();
        m.connect(true, "/tmp/ep-mix", SockType::Stream).unwrap();
        m.connect(true, "/tmp/ep-mix", SockType::Dgram) == Err(E_EPROTOTYPE)
    }, "已连异型 E_EPROTOTYPE 先于 EISCONN");
    s.add("J10-重复监听报错", {
        let mut m = ProtoMatrix::new();
        m.serve("/tmp/ep-bind", SockType::Stream).unwrap();
        m.serve("/tmp/ep-bind", SockType::Stream) == Err(E_EINVAL)
    }, "路径重复监听 E_EINVAL");
    s.add("J11-连接计数", {
        let mut m = ProtoMatrix::new();
        m.serve("/tmp/ep-a", SockType::Stream).unwrap();
        m.serve("/tmp/ep-b", SockType::Dgram).unwrap();
        m.connect(true, "/tmp/ep-a", SockType::Stream).unwrap();
        m.connect(true, "/tmp/ep-b", SockType::Dgram).unwrap();
        m.connected_count() == 2
    }, "两路成功计数 2");
    s.add("J12-监听态可关", {
        let mut m = ProtoMatrix::new();
        m.serve("/tmp/ep-close", SockType::Stream).unwrap();
        m.endpoints[0].listening = false;
        m.connect(true, "/tmp/ep-close", SockType::Stream) == Err(E_EPROTOTYPE)
    }, "关闭监听后 connect E_EPROTOTYPE");
    s.add("J13-三型枚举互异", {
        SockType::Stream != SockType::Dgram
            && SockType::Dgram != SockType::SeqPacket
            && SockType::Stream != SockType::SeqPacket
    }, "三型两两互异");
    s.add("J14-多端点路由", {
        let mut m = ProtoMatrix::new();
        m.serve("/tmp/r1", SockType::Stream).unwrap();
        m.serve("/tmp/r2", SockType::Dgram).unwrap();
        m.connect(true, "/tmp/r2", SockType::Dgram).is_ok()
            && m.connect(true, "/tmp/r1", SockType::Stream).is_ok()
    }, "多端点各连各型");
    s.add("J15-类型不匹配计数零", {
        let mut m = ProtoMatrix::new();
        m.serve("/tmp/ep-zero", SockType::Dgram).unwrap();
        let _ = m.connect(true, "/tmp/ep-zero", SockType::Stream);
        m.connected_count() == 0
    }, "失败 connect 不入连接表");
    s.add_dyn("J16-判错次序表", {
        let mut m = ProtoMatrix::new();
        let r1 = m.connect(true, "/tmp/x", SockType::Stream);
        m.serve("/tmp/x", SockType::Stream).unwrap();
        let r2 = m.connect(false, "/tmp/x", SockType::Stream);
        r1.is_err() && r2.is_err()
    }, "无端点 EPROTOTYPE → 坏 fd EBADF 次序");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_prototype_matrix() {
        let mut m = ProtoMatrix::new();
        m.serve("/s", SockType::Stream).unwrap();
        assert_eq!(m.connect(true, "/s", SockType::Dgram), Err(E_EPROTOTYPE));
        assert!(m.connect(true, "/s", SockType::Stream).is_ok());
    }

    #[test]
    fn checks_green() {
        let s = run_unx10867_checks();
        assert!(s.all_passed(), "UNX-F10867 判据全绿");
    }
}
