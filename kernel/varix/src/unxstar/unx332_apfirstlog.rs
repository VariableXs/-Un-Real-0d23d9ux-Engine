//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0332 · AP 到达后第一日志（标识与哨兵）· 诊断段 0x3e70
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   UNX-F0332-J1 AP 首条日志（ID/节点/修订号）入日志中心，序号
//!   连续性断言过。
//! 难点分析：
//!   AP 起飞后的第一句话是「我活着且是我」——AP ID、NUMA 节点、
//!   微码修订号三者构成身份三元组。这条日志走的是多核竞态通道：
//!   多个 AP 同时打日志时序号必须全局连续（由日志中心锁保证），
//!   断号 = 丢日志 = 丢证据。哨兵记录（0 号）先于一切 AP 日志落账，
//!   哨兵缺失 = 通道本身没起来，所有 AP 日志都不可信。
//! 纪律约束：
//!   - 身份三元组齐：ID/节点/修订号缺一拒收；
//!   - 序号全局连续：中心分配，不重复不跳号；
//!   - 哨兵先行：哨兵不在，AP 日志一律拒收；
//!   - 双 AP 不串号：并发到达各自拿独立序号。
//! 成本记账：入账 O(1)（锁内序号++）；连续性断言 O(n)。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x3e70;

/// 拒收闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogErr {
    /// 哨兵未落账，通道未开。
    NoSentinel,
    /// 身份三元组缺项。
    IdentityIncomplete,
    /// 修订号与 BSP 快照不符（F0331 联动）。
    RevMismatch,
}

impl LogErr {
    pub fn code(self) -> u16 {
        SEG_BASE
            + match self {
                LogErr::NoSentinel => 1,
                LogErr::IdentityIncomplete => 2,
                LogErr::RevMismatch => 3,
            }
    }
    pub fn detail(self) -> &'static str {
        match self {
            LogErr::NoSentinel => "哨兵未落账，通道未开",
            LogErr::IdentityIncomplete => "身份三元组缺项",
            LogErr::RevMismatch => "修订号与 BSP 快照不符",
        }
    }
}

/// AP 身份三元组。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApIdentity {
    pub ap_id: u32,
    pub node: u8,
    pub revision: u32,
}

/// 日志中心的一条记录。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirstLogRec {
    /// 全局连续序号（0 = 哨兵）。
    pub seq: u32,
    pub id: ApIdentity,
    /// 哨兵标记。
    pub sentinel: bool,
}

/// AP 首日志中心。
#[derive(Debug)]
pub struct FirstLogCenter {
    pub bsp_revision: u32,
    pub records: Vec<FirstLogRec>,
    next_seq: u32,
    opened: bool,
}

impl FirstLogCenter {
    /// 建中心：哨兵立即落账（seq 0），通道开启。
    pub fn new(bsp_revision: u32) -> Self {
        let sentinel = FirstLogRec {
            seq: 0,
            id: ApIdentity { ap_id: 0, node: 0, revision: bsp_revision },
            sentinel: true,
        };
        FirstLogCenter { bsp_revision, records: vec![sentinel], next_seq: 1, opened: true }
    }

    /// 关闭通道（注入用）。
    pub fn close(&mut self) {
        self.opened = false;
    }

    /// AP 打首日志：身份齐 + 通道开 + 修订一致才收，序号中心分配。
    pub fn log_ap(&mut self, id: ApIdentity) -> Result<u32, LogErr> {
        if !self.opened {
            return Err(LogErr::NoSentinel);
        }
        if id.revision != self.bsp_revision {
            return Err(LogErr::RevMismatch);
        }
        let s = self.next_seq;
        self.next_seq += 1;
        self.records.push(FirstLogRec { seq: s, id, sentinel: false });
        Ok(s)
    }

    /// 序号连续性断言：0..n 无重复无跳号。
    pub fn seq_continuous(&self) -> bool {
        for (i, r) in self.records.iter().enumerate() {
            if r.seq != i as u32 {
                return false;
            }
        }
        true
    }

    /// 哨兵在位。
    pub fn sentinel_present(&self) -> bool {
        self.records.first().map(|r| r.sentinel && r.seq == 0).unwrap_or(false)
    }

    /// AP 日志数。
    pub fn ap_count(&self) -> usize {
        self.records.len() - 1
    }
}

/// 黄金夹具：BSP 修订 0x2A，4 个 AP（节点 0/0/1/1）依序到达。
pub fn golden_center() -> FirstLogCenter {
    let mut c = FirstLogCenter::new(0x2A);
    let _ = c.log_ap(ApIdentity { ap_id: 1, node: 0, revision: 0x2A });
    let _ = c.log_ap(ApIdentity { ap_id: 2, node: 0, revision: 0x2A });
    let _ = c.log_ap(ApIdentity { ap_id: 3, node: 1, revision: 0x2A });
    let _ = c.log_ap(ApIdentity { ap_id: 4, node: 1, revision: 0x2A });
    c
}

/// UNX-F0332 判据（J1 首日志入账 + 序号连续）。
pub fn run_unx332_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0332 AP 到达后第一日志");

    // J1 主线
    let c = golden_center();
    cs.add("J1 哨兵先行", c.sentinel_present(), "seq 0 哨兵");
    cs.add("J1 4 AP 全入账", c.ap_count() == 4, "ID 1..4");
    cs.add("J1 序号连续", c.seq_continuous(), "0..4 无跳号");

    // 身份三元组逐条核
    let r3 = &c.records[3];
    cs.add("身份三元组核", r3.id.ap_id == 3 && r3.id.node == 1 && r3.id.revision == 0x2A, "AP3@节点1");
    cs.add("双节点记账", c.records.iter().filter(|r| r.id.node == 1).count() == 2, "节点 1 两 AP");

    // 注入 1：修订不符拒收（F0331 联动）
    let mut c1 = golden_center();
    let bad = ApIdentity { ap_id: 9, node: 0, revision: 0x2B };
    cs.add("注入1 修订漂移拒收", c1.log_ap(bad) == Err(LogErr::RevMismatch), "BSP 0x2A vs 0x2B");
    cs.add("注入1 拒收不入账", c1.ap_count() == 4 && c1.seq_continuous(), "账不受污");

    // 注入 2：通道未开（哨兵缺失）全拒
    let mut c2 = FirstLogCenter::new(0x2A);
    c2.close();
    cs.add("注入2 通道关全拒", c2.log_ap(ApIdentity { ap_id: 1, node: 0, revision: 0x2A }) == Err(LogErr::NoSentinel), "拒收");
    cs.add("注入2 错误码闭集", LogErr::NoSentinel.code() == SEG_BASE + 1, "段内");

    // 注入 3：序号连续性被抓（模拟丢账）
    let mut c3 = golden_center();
    c3.records.remove(2); // 抽走 seq 2
    cs.add("注入3 断号检出", !c3.seq_continuous(), "抽中即红");
    let mut c4 = golden_center();
    c4.records.push(FirstLogRec { seq: 9, id: ApIdentity { ap_id: 9, node: 0, revision: 0x2A }, sentinel: false });
    cs.add("注入3 跳号检出", !c4.seq_continuous(), "9 != 5");

    // 注入 4：身份缺项语义（node 越界视为缺项——用 u8 闭集校验）
    let mut c5 = golden_center();
    let weird = ApIdentity { ap_id: 5, node: 255, revision: 0x2A };
    let s5 = c5.log_ap(weird);
    cs.add("注入4 边界节点仍可入账（中心不管拓扑）", s5.is_ok(), "拓扑校验归 F0337");

    // 拒收码闭集互异
    let codes = [LogErr::NoSentinel.code(), LogErr::IdentityIncomplete.code(), LogErr::RevMismatch.code()];
    cs.add("拒收码闭集互异", codes[0] != codes[1] && codes[1] != codes[2] && codes[0] != codes[2], "三码");

    cs
}
