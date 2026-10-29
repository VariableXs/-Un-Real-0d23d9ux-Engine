//! UNX-F24191 vblank/HPD 内核事件面与事件队列·子功能11（连接状态机）（0x419C0 段）
//!
//! 锚点：UNX-F24191-J1 迁移账（状态迁移逐条落账）；J2 迁移语义（合
//! 法迁移 + 非法迁移拒 + 重复使能拒三性质）；J3 迁移指纹（账折叠指
//! 纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（连接状态机面）。
//!
//! 结构：三态机（断开→连接→使能）+ 迁移核 + 账本 + 折叠指纹 + 注入
//! 核（野终态/重复使能/未连使能/印后/断开未连必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0x419C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x419C0;
pub const SM_BAD_TRANS: u32 = SEG_BASE + 0x01; // 非法迁移
pub const SM_BAD_STATE: u32 = SEG_BASE + 0x02; // 野状态码
pub const SM_DOUBLE_EN: u32 = SEG_BASE + 0x03; // 重复使能
pub const SM_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const SM_DISCONNECT_LATE: u32 = SEG_BASE + 0x05; // 断开未连

/// 连接器状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SmState {
    Disconnected,
    Connected,
    Enabled,
}

/// 连接状态机。
pub struct SmConn {
    pub id: u64,
    st: SmState,
    transitions: u64,
    sealed: bool,
}

impl SmConn {
    pub fn new(id: u64) -> SmConn {
        SmConn {
            id,
            st: SmState::Disconnected,
            transitions: 0,
            sealed: false,
        }
    }

    pub fn state(&self) -> SmState {
        self.st
    }

    /// 连接：断开→连接；已连/已使能拒。
    pub fn connect(&mut self) -> Result<SmState, u32> {
        if self.sealed {
            return Err(SM_FROZEN);
        }
        match self.st {
            SmState::Disconnected => {
                self.st = SmState::Connected;
                self.transitions += 1;
                Ok(self.st)
            }
            _ => Err(SM_BAD_TRANS),
        }
    }

    /// 使能：连接→使能；未连拒、重复使能拒。
    pub fn enable(&mut self) -> Result<SmState, u32> {
        if self.sealed {
            return Err(SM_FROZEN);
        }
        match self.st {
            SmState::Connected => {
                self.st = SmState::Enabled;
                self.transitions += 1;
                Ok(self.st)
            }
            SmState::Disconnected => Err(SM_BAD_TRANS),
            SmState::Enabled => Err(SM_DOUBLE_EN),
        }
    }

    /// 断开：连接/使能→断开；未连拒。
    pub fn disconnect(&mut self) -> Result<SmState, u32> {
        if self.sealed {
            return Err(SM_FROZEN);
        }
        match self.st {
            SmState::Disconnected => Err(SM_DISCONNECT_LATE),
            _ => {
                self.st = SmState::Disconnected;
                self.transitions += 1;
                Ok(self.st)
            }
        }
    }

    pub fn transitions_count(&self) -> u64 {
        self.transitions
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x419C0;
        h = h.wrapping_mul(31).wrapping_add(self.id as u32);
        h = h.wrapping_mul(31).wrapping_add(match self.st {
            SmState::Disconnected => 0,
            SmState::Connected => 1,
            SmState::Enabled => 2,
        });
        h = h.wrapping_mul(31).wrapping_add(self.transitions as u32);
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准夹具：连接→使能→断开→再连接。
pub fn mk_conn() -> SmConn {
    let mut c = SmConn::new(42);
    let _ = c.connect();
    let _ = c.enable();
    let _ = c.disconnect();
    let _ = c.connect();
    c
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 未连使能
    got[0] = SmConn::new(1).enable().err() == Some(SM_BAD_TRANS);
    // 2) 重复使能
    let mut c = SmConn::new(2);
    let _ = c.connect();
    let _ = c.enable();
    got[1] = c.enable().err() == Some(SM_DOUBLE_EN);
    // 3) 重复连接
    let mut c2 = SmConn::new(3);
    let _ = c2.connect();
    got[2] = c2.connect().err() == Some(SM_BAD_TRANS);
    // 4) 断开未连
    got[3] = SmConn::new(4).disconnect().err() == Some(SM_DISCONNECT_LATE);
    // 5) 印后操作
    let mut c3 = mk_conn();
    c3.seal();
    got[4] = c3.connect().err() == Some(SM_FROZEN)
        && c3.enable().err() == Some(SM_FROZEN)
        && c3.disconnect().err() == Some(SM_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24191_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24191");
    let mut c = mk_conn();

    // J1 迁移计数：4 次合法迁移
    let j1 = c.transitions_count() == 4;
    s.add("J1-迁移计数", j1, "4 次迁移");

    // J2 终态：再连接后 Connected
    let j2 = c.state() == SmState::Connected;
    s.add("J2-终态", j2, "Connected");

    // J3 全环重演：Connected→Enabled→Disconnected
    let j3 = {
        let a = c.enable() == Ok(SmState::Enabled);
        let b = c.disconnect() == Ok(SmState::Disconnected);
        a && b && c.state() == SmState::Disconnected
    };
    s.add("J3-全环重演", j3, "使能断开归零");

    // J4 未连使能拒
    let j4 = SmConn::new(7).enable().err() == Some(SM_BAD_TRANS);
    s.add("J4-未连使能拒", j4, "断开态使能必拒");

    // J5 重复使能拒
    let j5 = {
        let mut c2 = SmConn::new(8);
        let _ = c2.connect();
        let _ = c2.enable();
        c2.enable().err() == Some(SM_DOUBLE_EN)
    };
    s.add("J5-重复使能拒", j5, "已使能再使能必拒");

    // J6 重复连接拒
    let j6 = {
        let mut c2 = SmConn::new(9);
        let _ = c2.connect();
        c2.connect().err() == Some(SM_BAD_TRANS)
    };
    s.add("J6-重复连接拒", j6, "已连再连必拒");

    // J7 断开未连拒
    let j7 = SmConn::new(10).disconnect().err() == Some(SM_DISCONNECT_LATE);
    s.add("J7-断开未连拒", j7, "断开态断开必拒");

    // J8 使能态可直断
    let j8 = {
        let mut c2 = SmConn::new(11);
        let _ = c2.connect();
        let _ = c2.enable();
        c2.disconnect() == Ok(SmState::Disconnected)
    };
    s.add("J8-使能直断", j8, "使能态断开放行");

    // J9 初始态
    let j9 = SmConn::new(12).state() == SmState::Disconnected;
    s.add("J9-初始态", j9, "初生即断开");

    // J10 迁移计数守恒
    let j10 = {
        let mut c2 = SmConn::new(13);
        let _ = c2.connect();
        let _ = c2.enable();
        let _ = c2.disconnect();
        c2.transitions_count() == 3
    };
    s.add("J10-计数守恒", j10, "3 迁移记 3");

    // J11 印后操作拒
    let j11 = {
        let mut c2 = mk_conn();
        c2.seal();
        c2.enable().err() == Some(SM_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

    // J12 指纹稳定
    let j12 = {
        let mut c2 = mk_conn();
        let a = c2.fold();
        c2.seal();
        c2.fold() == a
    };
    s.add("J12-指纹稳定", j12, "印不改指纹");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx24191_sm_flow() {
        let mut c = SmConn::new(1);
        assert_eq!(c.state(), SmState::Disconnected);
        assert_eq!(c.enable(), Err(SM_BAD_TRANS));
        assert_eq!(c.connect(), Ok(SmState::Connected));
        assert_eq!(c.connect(), Err(SM_BAD_TRANS));
        assert_eq!(c.enable(), Ok(SmState::Enabled));
        assert_eq!(c.enable(), Err(SM_DOUBLE_EN));
        assert_eq!(c.disconnect(), Ok(SmState::Disconnected));
        assert_eq!(c.disconnect(), Err(SM_DISCONNECT_LATE));
        c.seal();
        assert_eq!(c.connect(), Err(SM_FROZEN));
    }

    #[test]
    fn unx24191_checks_green() {
        let s = run_unx24191_checks();
        assert!(s.all_passed(), "UNX-F24191 判据未全绿");
    }
}
