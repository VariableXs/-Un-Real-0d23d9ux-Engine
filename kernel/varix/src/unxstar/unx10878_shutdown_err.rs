//! UNX-F10878 shutdown 错误矩阵（how 值/ENOTCONN）（0x1c640 段）
//! 锚点：UNX-F10878-J1 shutdown 错误面（how 三值/未连接/半关语义）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c640;
pub const E_EINVAL: u32 = SEG_BASE + 1;
pub const E_ENOTCONN: u32 = SEG_BASE + 2;
pub const E_EBADF: u32 = SEG_BASE + 3;
pub const E_EPIPE: u32 = SEG_BASE + 4;
pub const E_OK: u32 = SEG_BASE + 5;
pub const E_SEALED: u32 = SEG_BASE + 6;

/// shutdown how 值：0=读关 1=写关 2=全关
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum How {
    Rd = 0,
    Wr = 1,
    RdWr = 2,
}

/// 半关状态
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HalfState {
    pub rd_closed: bool,
    pub wr_closed: bool,
}

impl HalfState {
    pub fn fresh() -> Self {
        HalfState { rd_closed: false, wr_closed: false }
    }
    pub fn fully(&self) -> bool {
        self.rd_closed && self.wr_closed
    }
}

/// shutdown 语义机
pub struct ShutdownSock {
    connected: bool,
    open: bool,
    state: HalfState,
    events: Vec<&'static str>,
}

impl ShutdownSock {
    pub fn new(connected: bool) -> Self {
        ShutdownSock { connected, open: true, state: HalfState::fresh(), events: Vec::new() }
    }

    pub fn close_fd(&mut self) {
        self.open = false;
    }

    pub fn state_of(&self) -> HalfState {
        self.state
    }

    pub fn events(&self) -> usize {
        self.events.len()
    }

    pub fn last_event(&self) -> Option<&'static str> {
        self.events.last().copied()
    }

    /// shutdown 判定：fd → 连接 → how 合法 → 状态推进
    pub fn shutdown(&mut self, how: How) -> Result<(), u32> {
        if !self.open {
            return Err(E_EBADF);
        }
        if !self.connected {
            return Err(E_ENOTCONN);
        }
        match how {
            How::Rd => {
                self.state.rd_closed = true;
                self.events.push("rd");
                Ok(())
            }
            How::Wr => {
                if self.state.wr_closed {
                    return Err(E_EPIPE);
                }
                self.state.wr_closed = true;
                self.events.push("wr");
                Ok(())
            }
            How::RdWr => {
                self.state.rd_closed = true;
                self.state.wr_closed = true;
                self.events.push("rdwr");
                Ok(())
            }
        }
    }

    /// 写面探测：写关后写 EPIPE
    pub fn try_write(&self) -> Result<(), u32> {
        if self.state.wr_closed {
            return Err(E_EPIPE);
        }
        Ok(())
    }

    /// 读面探测：读关后读返 0（此处以 Ok("eof") 表达）
    pub fn try_read(&self) -> Result<&'static str, u32> {
        if self.state.rd_closed {
            return Ok("eof");
        }
        Ok("data")
    }
}

pub fn run_unx10878_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10878");
    s.add("J1-段锚", SEG_BASE == 0x1c640, "shutdown 段基址 0x1c640");
    s.add("J2-how 枚举值", How::Rd as u32 == 0 && How::Wr as u32 == 1 && How::RdWr as u32 == 2, "how 三值 0/1/2");
    s.add("J3-读关推进", {
        let mut k = ShutdownSock::new(true);
        k.shutdown(How::Rd).unwrap();
        k.state_of().rd_closed && !k.state_of().wr_closed
    }, "Rd 只关读面");
    s.add("J4-写关推进", {
        let mut k = ShutdownSock::new(true);
        k.shutdown(How::Wr).unwrap();
        k.state_of().wr_closed && !k.state_of().rd_closed
    }, "Wr 只关写面");
    s.add("J5-全关推进", {
        let mut k = ShutdownSock::new(true);
        k.shutdown(How::RdWr).unwrap();
        k.state_of().fully()
    }, "RdWr 双面关");
    s.add("J6-未连接拒", {
        let mut k = ShutdownSock::new(false);
        k.shutdown(How::Wr) == Err(E_ENOTCONN)
    }, "未连接 E_ENOTCONN");
    s.add("J7-关闭 fd 拒", {
        let mut k = ShutdownSock::new(true);
        k.close_fd();
        k.shutdown(How::Wr) == Err(E_EBADF)
    }, "fd 关 E_EBADF");
    s.add("J8-EBADF 先于 ENOTCONN", {
        let mut k = ShutdownSock::new(false);
        k.close_fd();
        k.shutdown(How::Wr) == Err(E_EBADF)
    }, "关+未连 EBADF 优先");
    s.add("J9-写关后写 EPIPE", {
        let mut k = ShutdownSock::new(true);
        k.shutdown(How::Wr).unwrap();
        k.try_write() == Err(E_EPIPE)
    }, "写关后 try_write E_EPIPE");
    s.add("J10-读关后读 eof", {
        let mut k = ShutdownSock::new(true);
        k.shutdown(How::Rd).unwrap();
        k.try_read() == Ok("eof")
    }, "读关后 try_read eof");
    s.add("J11-写后重复关写 EPIPE", {
        let mut k = ShutdownSock::new(true);
        k.shutdown(How::Wr).unwrap();
        k.shutdown(How::Wr) == Err(E_EPIPE)
    }, "二次写关 E_EPIPE");
    s.add("J12-读可重复关", {
        let mut k = ShutdownSock::new(true);
        k.shutdown(How::Rd).unwrap();
        k.shutdown(How::Rd).is_ok()
    }, "重复读关幂等 Ok");
    s.add("J13-写关后全关 Ok", {
        let mut k = ShutdownSock::new(true);
        k.shutdown(How::Wr).unwrap();
        k.shutdown(How::RdWr).unwrap();
        k.state_of().fully()
    }, "写关后全关仍 Ok 达 fully");
    s.add("J14-事件序列", {
        let mut k = ShutdownSock::new(true);
        k.shutdown(How::Rd).unwrap();
        k.shutdown(How::Wr).unwrap();
        k.events() == 2 && k.last_event() == Some("wr")
    }, "两事件尾 wr");
    s.add("J15-写关不阻读", {
        let mut k = ShutdownSock::new(true);
        k.shutdown(How::Wr).unwrap();
        k.try_read() == Ok("data")
    }, "写关后读面仍 data");
    s.add_dyn("J16-半关全账", {
        let mut k = ShutdownSock::new(true);
        let w = k.shutdown(How::Wr);
        let wr = k.try_write();
        let rd = k.try_read();
        let full = k.shutdown(How::RdWr);
        w.is_ok() && wr.is_err() && rd == Ok("data") && full.is_ok()
    }, "Wr 关→EPIPE→data→全关账");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_shutdown_how() {
        let mut k = ShutdownSock::new(true);
        assert!(k.shutdown(How::Wr).is_ok());
        assert_eq!(k.try_write(), Err(E_EPIPE));
        assert_eq!(k.try_read(), Ok("data"));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10878_checks();
        assert!(s.all_passed(), "UNX-F10878 判据全绿");
    }
}
