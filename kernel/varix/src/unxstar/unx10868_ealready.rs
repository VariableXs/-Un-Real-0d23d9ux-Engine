//! UNX-F10868 EALREADY 矩阵（进行中再 connect）（0x1c500 段）
//! 锚点：UNX-F10868-J1 EALREADY 正反双判据（非阻塞 connect 进行中重复发起）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c500;
pub const E_EALREADY: u32 = SEG_BASE + 1;
pub const E_EISCONN: u32 = SEG_BASE + 2;
pub const E_EINPROGRESS: u32 = SEG_BASE + 3;
pub const E_ENOENT: u32 = SEG_BASE + 4;
pub const E_EBADF: u32 = SEG_BASE + 5;
pub const E_OK: u32 = SEG_BASE + 6;

/// 连接状态机：Idle → InProgress → Established / Failed
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConnState {
    Idle,
    InProgress,
    Established,
    Failed,
}

/// 非阻塞连接器
pub struct NonblockConn {
    state: ConnState,
    blocking: bool,
    path_exists: bool,
    attempts: usize,
}

impl NonblockConn {
    pub fn new(blocking: bool, path_exists: bool) -> Self {
        NonblockConn { state: ConnState::Idle, blocking, path_exists, attempts: 0 }
    }

    pub fn state_of(&self) -> ConnState {
        self.state
    }

    pub fn attempts(&self) -> usize {
        self.attempts
    }

    /// connect 语义：
    /// 阻塞套接字 → 一步到底 Ok / 失败；
    /// 非阻塞首连 → EINPROGRESS 且状态 InProgress；
    /// 非阻塞进行中再连 → EALREADY；
    /// InProgress 态下 finish 成功 → Established，此后再连 → EISCONN。
    pub fn connect(&mut self) -> Result<(), u32> {
        self.attempts += 1;
        if !self.path_exists {
            self.state = ConnState::Failed;
            return Err(E_ENOENT);
        }
        if self.blocking {
            self.state = ConnState::Established;
            return Ok(());
        }
        match self.state {
            ConnState::Idle => {
                self.state = ConnState::InProgress;
                Err(E_EINPROGRESS)
            }
            ConnState::InProgress => Err(E_EALREADY),
            ConnState::Established => Err(E_EISCONN),
            ConnState::Failed => Err(E_ENOENT),
        }
    }

    pub fn finish_ok(&mut self) {
        if self.state == ConnState::InProgress {
            self.state = ConnState::Established;
        }
    }

    pub fn finish_err(&mut self) {
        if self.state == ConnState::InProgress {
            self.state = ConnState::Failed;
        }
    }
}

pub fn run_unx10868_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10868");
    s.add("J1-段锚", SEG_BASE == 0x1c500, "EALREADY 段基址 0x1c500");
    s.add("J2-首连报进行中", {
        let mut c = NonblockConn::new(false, true);
        c.connect() == Err(E_EINPROGRESS)
    }, "非阻塞首连 E_EINPROGRESS");
    s.add("J3-进行中再连报已发起", {
        let mut c = NonblockConn::new(false, true);
        let _ = c.connect();
        c.connect() == Err(E_EALREADY)
    }, "InProgress 再连 E_EALREADY");
    s.add("J4-三连仍已发起", {
        let mut c = NonblockConn::new(false, true);
        let _ = c.connect();
        let _ = c.connect();
        c.connect() == Err(E_EALREADY)
    }, "第三次 E_EALREADY");
    s.add("J5-完成后再连报已连", {
        let mut c = NonblockConn::new(false, true);
        let _ = c.connect();
        c.finish_ok();
        c.connect() == Err(E_EISCONN)
    }, "Established 再连 E_EISCONN");
    s.add("J6-阻塞一步到底", {
        let mut c = NonblockConn::new(true, true);
        c.connect().is_ok() && c.state_of() == ConnState::Established
    }, "阻塞 connect Ok 直达 Established");
    s.add("J7-路径缺失报 ENOENT", {
        let mut c = NonblockConn::new(false, false);
        c.connect() == Err(E_ENOENT)
    }, "无路径 E_ENOENT");
    s.add("J8-失败态再连报 ENOENT", {
        let mut c = NonblockConn::new(false, true);
        let _ = c.connect();
        c.finish_err();
        c.connect() == Err(E_ENOENT)
    }, "Failed 态再连 E_ENOENT");
    s.add("J9-完成在中途", {
        let mut c = NonblockConn::new(false, true);
        let _ = c.connect();
        c.finish_ok();
        c.state_of() == ConnState::Established
    }, "finish_ok 后 Established");
    s.add("J10-finish 幂等", {
        let mut c = NonblockConn::new(false, true);
        let _ = c.connect();
        c.finish_ok();
        c.finish_ok();
        c.finish_err();
        c.state_of() == ConnState::Established
    }, "Established 后 finish 不降级");
    s.add("J11-尝试计数", {
        let mut c = NonblockConn::new(false, true);
        let _ = c.connect();
        let _ = c.connect();
        let _ = c.connect();
        c.attempts() == 3
    }, "三次发起计数 3");
    s.add("J12-状态机四态", {
        ConnState::Idle != ConnState::InProgress
            && ConnState::InProgress != ConnState::Established
            && ConnState::Established != ConnState::Failed
    }, "四态两两互异");
    s.add("J13-完成态次序", {
        let mut c = NonblockConn::new(false, true);
        let _ = c.connect();
        let r1 = c.connect();
        c.finish_ok();
        let r2 = c.connect();
        r1 == Err(E_EALREADY) && r2 == Err(E_EISCONN)
    }, "先 EALREADY 后 EISCONN 次序");
    s.add("J14-阻塞不计进行中", {
        let mut c = NonblockConn::new(true, true);
        c.connect().unwrap();
        c.connect() == Ok(())
    }, "阻塞重连也 Ok（无 InProgress 面）");
    s.add("J15-失败后非阻塞不回 Idle", {
        let mut c = NonblockConn::new(false, true);
        let _ = c.connect();
        c.finish_err();
        let _ = c.connect();
        c.state_of() == ConnState::Failed
    }, "Failed 恒驻留");
    s.add_dyn("J16-全链次序账", {
        let mut c = NonblockConn::new(false, true);
        let r1 = c.connect();
        let r2 = c.connect();
        c.finish_ok();
        let r3 = c.connect();
        r1.is_err() && r2.is_err() && r3.is_ok()
    }, "INPROGRESS-ALREADY-ISCONN 全链");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_already_chain() {
        let mut c = NonblockConn::new(false, true);
        assert_eq!(c.connect(), Err(E_EINPROGRESS));
        assert_eq!(c.connect(), Err(E_EALREADY));
        c.finish_ok();
        assert_eq!(c.connect(), Err(E_EISCONN));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10868_checks();
        assert!(s.all_passed(), "UNX-F10868 判据全绿");
    }
}
