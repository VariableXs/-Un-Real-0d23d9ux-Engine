//! UNX-F12076 TxFsContext 字段语义（0x19ba0 段）
//!
//! 卡面：TEB→TxFsContext——事务文件系统上下文字段语义：事务号非零唯一、
//! 状态机 Idle→Active→Prepared→{Committed|Aborted} 终态封禁。
//! 域：CoRun·UNX-D1-B04。
//!
//! 判据原文：
//! - J1 事务号非零：零事务号 begin 必拒。
//! - J2 事务号唯一：登记簿内重复事务号必拒。
//! - J3 状态机有序：Active 只能从 Idle 进入、Prepared 只能从 Active 进入。
//! - J4 提交门禁：仅 Prepared 态可 commit，违者必拒。
//! - J5 终态封禁：Committed/Aborted 后任何推进必拒。
//! - J6 中止路径：Active/Prepared 皆可 abort，Idle 不可。
//! - J7 印账冻结：seal 后一切推进必拒。
//! - J8 注入五况全抓。
//!
//! 结构：TxState 五态枚举 + TxCtx 上下文 + TxBook 登记簿（唯一性 +
//! 生命周期账）+ 折叠指纹（211 进制卷积）+ 注入核（零号/重号/错序提交/
//! 终态推进/印后开账）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19ba0 独占 0x20）。
pub const SEG_BASE: u32 = 0x19ba0;
pub const TX_ZERO_TXID: u32 = SEG_BASE + 0x01; // 零事务号
pub const TX_DUP_TXID: u32 = SEG_BASE + 0x02; // 重号
pub const TX_BAD_STATE: u32 = SEG_BASE + 0x03; // 状态机错序
pub const TX_TERMINAL: u32 = SEG_BASE + 0x04; // 终态推进
pub const TX_FROZEN: u32 = SEG_BASE + 0x05; // 印账后推进

/// 事务五态。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TxState {
    Idle = 0,
    Active = 1,
    Prepared = 2,
    Committed = 3,
    Aborted = 4,
}

impl TxState {
    /// 终态判定。
    pub fn terminal(&self) -> bool {
        matches!(self, TxState::Committed | TxState::Aborted)
    }
}

/// 事务上下文。
#[derive(Clone, Copy)]
pub struct TxCtx {
    pub txid: u32,
    pub state: TxState,
}

/// 事务登记簿。
pub struct TxBook {
    pub ctxs: Vec<TxCtx>,
    pub sealed: bool,
}

impl TxBook {
    pub fn new() -> TxBook {
        TxBook { ctxs: Vec::new(), sealed: false }
    }

    fn find(&self, txid: u32) -> Option<usize> {
        self.ctxs.iter().position(|c| c.txid == txid)
    }

    /// 开账：零号与重号必拒。
    pub fn begin(&mut self, txid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(TX_FROZEN);
        }
        if txid == 0 {
            return Err(TX_ZERO_TXID);
        }
        if self.find(txid).is_some() {
            return Err(TX_DUP_TXID);
        }
        self.ctxs.push(TxCtx { txid, state: TxState::Active });
        Ok(())
    }

    /// 推进：prepare 只从 Active；commit 只从 Prepared；abort 从 Active/Prepared。
    pub fn advance(&mut self, txid: u32, to: TxState) -> Result<(), u32> {
        if self.sealed {
            return Err(TX_FROZEN);
        }
        let i = match self.find(txid) {
            Some(i) => i,
            None => return Err(TX_BAD_STATE),
        };
        let cur = self.ctxs[i].state;
        if cur.terminal() {
            return Err(TX_TERMINAL);
        }
        let legal = match to {
            TxState::Prepared => cur == TxState::Active,
            TxState::Committed => cur == TxState::Prepared,
            TxState::Aborted => cur == TxState::Active || cur == TxState::Prepared,
            _ => false,
        };
        if !legal {
            return Err(TX_BAD_STATE);
        }
        self.ctxs[i].state = to;
        Ok(())
    }

    /// 某事务状态查询。
    pub fn state_of(&self, txid: u32) -> Option<TxState> {
        self.ctxs.iter().find(|c| c.txid == txid).map(|c| c.state)
    }

    /// 统计：处于指定状态的事务数。
    pub fn count_in(&self, st: TxState) -> usize {
        self.ctxs.iter().filter(|c| c.state == st).count()
    }

    /// 全部事务皆达终态。
    pub fn all_terminal(&self) -> bool {
        !self.ctxs.is_empty() && self.ctxs.iter().all(|c| c.state.terminal())
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for c in &self.ctxs {
            h = h.wrapping_mul(211).wrapping_add(c.txid);
            h = h.wrapping_mul(211).wrapping_add(c.state as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("txs={} active={}", self.ctxs.len(), self.count_in(TxState::Active))
    }

    /// 诊断面：事务渲染行。
    pub fn ctx_line(&self, txid: u32) -> String {
        match self.state_of(txid) {
            Some(st) => format!("T{}|st{}", txid, st as u16),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = TxBook::new();
    // 1) 零事务号必拒。
    got[0] = b.begin(0).err() == Some(TX_ZERO_TXID);
    // 2) 重号必拒。
    let _ = b.begin(77);
    got[1] = b.begin(77).err() == Some(TX_DUP_TXID);
    // 3) 错序提交：Active 直接 commit 必拒。
    got[2] = b.advance(77, TxState::Committed).err() == Some(TX_BAD_STATE);
    // 4) 终态推进：commit 后再 advance 必拒。
    let _ = b.advance(77, TxState::Prepared);
    let _ = b.advance(77, TxState::Committed);
    got[3] = b.advance(77, TxState::Aborted).err() == Some(TX_TERMINAL);
    // 5) 印账后开账必抓。
    b.seal();
    got[4] = b.begin(88).err() == Some(TX_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12076_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12076");

    // J1 零事务号拒。
    let j1 = {
        let mut b = TxBook::new();
        b.begin(0).err() == Some(TX_ZERO_TXID)
    };
    s.add("J1-零号拒", j1, "零事务号必拒");

    // J2 重号拒。
    let j2 = {
        let mut b = TxBook::new();
        let _ = b.begin(5);
        b.begin(5).err() == Some(TX_DUP_TXID) && b.ctxs.len() == 1
    };
    s.add("J2-重号拒", j2, "重复事务号必拒");

    // J3 状态机有序推进。
    let j3 = {
        let mut b = TxBook::new();
        let _ = b.begin(9);
        b.advance(9, TxState::Prepared).is_ok()
            && b.state_of(9) == Some(TxState::Prepared)
    };
    s.add("J3-有序推进", j3, "Active→Prepared 合法");

    // J4 提交门禁：Active 直接 commit 拒。
    let j4 = {
        let mut b = TxBook::new();
        let _ = b.begin(9);
        b.advance(9, TxState::Committed).err() == Some(TX_BAD_STATE)
    };
    s.add("J4-提交门禁", j4, "未 prepare 不可 commit");

    // J5 提交正路径。
    let j5 = {
        let mut b = TxBook::new();
        let _ = b.begin(9);
        let _ = b.advance(9, TxState::Prepared);
        b.advance(9, TxState::Committed).is_ok()
            && b.state_of(9) == Some(TxState::Committed)
    };
    s.add("J5-提交正路径", j5, "Prepared→Committed 放行");

    // J6 终态封禁。
    let j6 = {
        let mut b = TxBook::new();
        let _ = b.begin(9);
        let _ = b.advance(9, TxState::Prepared);
        let _ = b.advance(9, TxState::Aborted);
        b.advance(9, TxState::Prepared).err() == Some(TX_TERMINAL)
            && TxState::Aborted.terminal()
    };
    s.add("J6-终态封禁", j6, "Aborted 后推进必拒");

    // J7 中止路径：Active/Prepared 皆可，Idle（未开账）不可。
    let j7 = {
        let mut b = TxBook::new();
        let _ = b.begin(3);
        let _ = b.advance(3, TxState::Aborted);
        b.advance(9, TxState::Aborted).err() == Some(TX_BAD_STATE)
    };
    s.add("J7-中止路径", j7, "未开账推进必拒");

    // J8 全终态聚合判定。
    let j8 = {
        let mut b = TxBook::new();
        let _ = b.begin(3);
        let _ = b.advance(3, TxState::Prepared);
        let _ = b.advance(3, TxState::Committed);
        let _ = b.begin(4);
        let _ = b.advance(4, TxState::Aborted);
        b.all_terminal()
    };
    s.add("J8-全终态", j8, "混合终态聚合为真");

    // J9 印账冻结。
    let j9 = {
        let mut b = TxBook::new();
        let _ = b.begin(9);
        b.seal();
        b.begin(10).err() == Some(TX_FROZEN)
            && b.advance(9, TxState::Aborted).err() == Some(TX_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后皆拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = TxBook::new();
        let _ = b.begin(9);
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = TxBook::new();
    let _ = b.begin(11);
    let _ = b.begin(12);
    let _ = b.advance(11, TxState::Prepared);

    s.add("统计回读", b.count_in(TxState::Active) == 1, "一活一备");
    s.add("摘要", b.summary().contains("txs=2"), "摘要含事务数");
    s.add("渲染行", b.ctx_line(12).contains("st1"), "Active 态渲染 st1");
    s.add("缺席行", b.ctx_line(99) == "missing", "未开账显性");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12076_tx_flow() {
        let mut b = TxBook::new();
        assert_eq!(b.begin(0), Err(TX_ZERO_TXID));
        assert!(b.begin(1).is_ok());
        assert_eq!(b.begin(1), Err(TX_DUP_TXID));
        assert!(b.advance(1, TxState::Prepared).is_ok());
        assert!(b.advance(1, TxState::Committed).is_ok());
        assert_eq!(b.advance(1, TxState::Aborted), Err(TX_TERMINAL));
    }

    #[test]
    fn unx12076_checks_green() {
        let s = run_unx12076_checks();
        assert!(s.all_passed(), "UNX-F12076 判据未全绿");
    }

    #[test]
    fn tx_state_helpers() {
        assert!(TxState::Committed.terminal());
        assert!(!TxState::Prepared.terminal());
        assert_eq!(TxState::Prepared as u16, 2);
    }
}
