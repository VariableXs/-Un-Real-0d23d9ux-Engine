//! UNX-F10881 connect 竞态窗矩阵（对端 close 并发）（0x1c6a0 段）
//! 锚点：UNX-F10881-J1 connect 竞态窗（对端 close 并发三相位判定）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1c6a0;
pub const E_ECONNREFUSED: u32 = SEG_BASE + 1;
pub const E_ECONNRESET: u32 = SEG_BASE + 2;
pub const E_EPIPE: u32 = SEG_BASE + 3;
pub const E_EAGAIN: u32 = SEG_BASE + 4;
pub const E_OK: u32 = SEG_BASE + 5;
pub const E_SEALED: u32 = SEG_BASE + 6;

/// 竞态相位：Before（connect 前 close）/ During（connect 中 close）/ After（connect 后 close）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RacePhase {
    Before,
    During,
    After,
    None,
}

/// 服务端存活面 + 客户端连接机
pub struct RaceWindow {
    peer_alive: bool,
    backlog: usize,
    phase: RacePhase,
    established: bool,
    events: Vec<&'static str>,
}

impl RaceWindow {
    pub fn new(alive: bool, backlog: usize) -> Self {
        RaceWindow {
            peer_alive: alive,
            backlog,
            phase: RacePhase::None,
            established: false,
            events: Vec::new(),
        }
    }

    pub fn peer_alive(&self) -> bool {
        self.peer_alive
    }

    pub fn established(&self) -> bool {
        self.established
    }

    pub fn events(&self) -> usize {
        self.events.len()
    }

    /// connect 判定：相位分账
    pub fn connect(&mut self) -> Result<(), u32> {
        if !self.peer_alive && self.backlog == 0 {
            self.phase = RacePhase::Before;
            self.events.push("refused");
            return Err(E_ECONNREFUSED);
        }
        if self.backlog == 0 && self.peer_alive {
            self.phase = RacePhase::During;
            self.events.push("during");
            return Err(E_EAGAIN);
        }
        self.phase = RacePhase::After;
        self.established = true;
        self.events.push("estab");
        Ok(())
    }

    /// 对端关闭：已建立则后续写 EPIPE / 收 ECONNRESET
    pub fn peer_close(&mut self) -> bool {
        let was = self.established;
        self.peer_alive = false;
        self.established = false;
        if was {
            self.events.push("reset");
        }
        was
    }

    pub fn write_after_close(&self) -> Result<(), u32> {
        if self.peer_alive {
            return Ok(());
        }
        Err(E_EPIPE)
    }

    pub fn read_after_close(&self) -> Result<&'static str, u32> {
        if self.peer_alive {
            return Ok("data");
        }
        Err(E_ECONNRESET)
    }

    pub fn phase_of(&self) -> RacePhase {
        self.phase
    }
}

pub fn run_unx10881_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10881");
    s.add("J1-段锚", SEG_BASE == 0x1c6a0, "connect 竞态段基址 0x1c6a0");
    s.add("J2-前相位拒", {
        let mut w = RaceWindow::new(false, 0);
        w.connect() == Err(E_ECONNREFUSED)
    }, "对端先 close E_ECONNREFUSED");
    s.add("J3-前相位记账", {
        let mut w = RaceWindow::new(false, 0);
        let _ = w.connect();
        w.phase_of() == RacePhase::Before && w.last_ev() == Some("refused")
    }, "Before 相位+refused 事件");
    s.add("J4-中相位排队拒", {
        let mut w = RaceWindow::new(true, 0);
        w.connect() == Err(E_EAGAIN)
    }, "活端零排队 E_EAGAIN");
    s.add("J5-后相位连成", {
        let mut w = RaceWindow::new(true, 2);
        w.connect().is_ok() && w.established()
    }, "有排队 connect Ok");
    s.add("J6-建立后对端关", {
        let mut w = RaceWindow::new(true, 2);
        w.connect().unwrap();
        w.peer_close()
    }, "建立后 close 返回曾是建立");
    s.add("J7-关后写 EPIPE", {
        let mut w = RaceWindow::new(true, 2);
        w.connect().unwrap();
        w.peer_close();
        w.write_after_close() == Err(E_EPIPE)
    }, "关后写 E_EPIPE");
    s.add("J8-关后读 ECONNRESET", {
        let mut w = RaceWindow::new(true, 2);
        w.connect().unwrap();
        w.peer_close();
        w.read_after_close() == Err(E_ECONNRESET)
    }, "关后读 E_ECONNRESET");
    s.add("J9-活端读写正常", {
        let mut w = RaceWindow::new(true, 2);
        w.connect().unwrap();
        w.write_after_close().is_ok() && w.read_after_close() == Ok("data")
    }, "活端写 Ok 读 data");
    s.add("J10-事件账", {
        let mut w = RaceWindow::new(true, 2);
        w.connect().unwrap();
        w.peer_close();
        w.events() == 2
    }, "estab+reset 两事件");
    s.add("J11-未建立对端关", {
        let mut w = RaceWindow::new(false, 0);
        w.peer_close() == false
    }, "未建立关返 false 无事件");
    s.add("J12-相位枚举互异", {
        RacePhase::Before != RacePhase::During
            && RacePhase::During != RacePhase::After
            && RacePhase::After != RacePhase::None
    }, "四相位互异");
    s.add("J13-竞态再连", {
        let mut w = RaceWindow::new(false, 0);
        let r1 = w.connect();
        let r2 = w.connect();
        r1 == Err(E_ECONNREFUSED) && r2 == Err(E_ECONNREFUSED)
    }, "前相位重连恒拒");
    s.add("J14-排队后关前判", {
        let mut w = RaceWindow::new(true, 1);
        let ok = w.connect().is_ok();
        w.peer_close();
        let wr = w.write_after_close();
        ok && wr == Err(E_EPIPE)
    }, "连成即关写面 E_EPIPE");
    s.add("J15-关后读面单值", {
        let mut w = RaceWindow::new(true, 1);
        w.connect().unwrap();
        w.peer_close();
        w.read_after_close().unwrap_err() == E_ECONNRESET
    }, "关后读单值 ECONNRESET");
    s.add_dyn("J16-全相位序账", {
        let mut w = RaceWindow::new(false, 0);
        let r1 = w.connect();
        w.peer_alive = true;
        w.backlog = 1;
        let r2 = w.connect();
        w.peer_close();
        let r3 = w.write_after_close();
        r1.is_err() && r2.is_ok() && r3.is_err()
    }, "REFUSED→Ok→EPIPE 全账");
    s
}

impl RaceWindow {
    fn last_ev(&self) -> Option<&'static str> {
        self.events.last().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_race_phases() {
        let mut w = RaceWindow::new(false, 0);
        assert_eq!(w.connect(), Err(E_ECONNREFUSED));
        let mut v = RaceWindow::new(true, 1);
        assert!(v.connect().is_ok());
        v.peer_close();
        assert_eq!(v.write_after_close(), Err(E_EPIPE));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10881_checks();
        assert!(s.all_passed(), "UNX-F10881 判据全绿");
    }
}
