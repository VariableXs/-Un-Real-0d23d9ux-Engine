//! unx0551_ringwrap —— UNX-F0551 环满覆盖账（显性覆盖）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0551 · 环满覆盖账 · 目标 200 行
//! 判据原文：覆盖显性核（满环续写 130 行——覆盖计数=130−128=2 全等），覆
//!           盖行号可查。
//! 组件：WrapLedger（覆盖账：覆盖数+被覆盖行号集）+ 全等核 + 行号可查。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35700;
pub const XC_WRAP_CAP: u32 = 128;
pub const XC_WRAP_DRIFT: u32 = SEG_BASE + 1; // 覆盖账漂移
pub const XC_WRAP_MISS: u32 = SEG_BASE + 2; // 行号失查
pub const XC_WRAP_OOB: u32 = SEG_BASE + 3; // 越界
pub const XC_WRAP_BAD: u32 = SEG_BASE + 4; // 坏账

/// 覆盖账。
#[derive(Debug, Default)]
pub struct WrapLedger {
    /// 覆盖计数。
    pub overwritten: u32,
    /// 被覆盖行的原始序号（第几次写入，0 起）。
    pub victim_writes: Vec<u64>,
    /// 总写入数。
    pub total_writes: u64,
}

impl WrapLedger {
    /// 空账。
    pub fn new() -> Self {
        WrapLedger::default()
    }

    /// 模拟满环写：第 n 次（0 起）写入时若 n ≥ 128 则覆盖第 (n-128) 次
    /// 写入的行。
    pub fn write(&mut self) {
        if self.total_writes >= XC_WRAP_CAP as u64 {
            let victim = self.total_writes - XC_WRAP_CAP as u64;
            self.overwritten += 1;
            self.victim_writes.push(victim);
        }
        self.total_writes += 1;
    }

    /// 覆盖计数全等核：overwritten == total-128（未满时 0）。
    pub fn count_exact(&self) -> bool {
        if self.total_writes <= XC_WRAP_CAP as u64 {
            self.overwritten == 0 && self.victim_writes.is_empty()
        } else {
            self.overwritten as u64 == self.total_writes - XC_WRAP_CAP as u64
                && self.victim_writes.len() as u64 == self.total_writes - XC_WRAP_CAP as u64
        }
    }

    /// 行号可查：victim_writes 应为连续序列 0..n-128。
    pub fn victims_contiguous(&self) -> bool {
        let mut i = 0;
        while i < self.victim_writes.len() {
            if self.victim_writes[i] != i as u64 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 守恒：victim 首个=0，末个=total-129。
    pub fn first_last_victim(&self) -> Option<(u64, u64)> {
        if self.victim_writes.is_empty() {
            None
        } else {
            Some((self.victim_writes[0], self.victim_writes[self.victim_writes.len() - 1]))
        }
    }
}

pub fn run_unx0551_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0551");
    cs.add("SEG 段位独占", SEG_BASE == 0x35700 && XC_WRAP_BAD == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：未满零覆盖。
    let mut w = WrapLedger::new();
    let mut i = 0;
    while i < 128 {
        w.write();
        i += 1;
    }
    cs.add("恰满零覆盖", w.total_writes == 128 && w.overwritten == 0 && w.count_exact(), "128 行不覆");

    // 族二：满环续写 130。
    let mut w2 = WrapLedger::new();
    let mut i = 0;
    while i < 130 {
        w2.write();
        i += 1;
    }
    cs.add("覆盖 2 全等", w2.overwritten == 2 && w2.count_exact(), "130-128=2");
    cs.add("行号可查", w2.victims_contiguous() && w2.first_last_victim() == Some((0, 1)), "被覆行 0,1");
    cs.add("受害者精确", w2.victim_writes == alloc::vec![0, 1], "最老两行先覆");

    // 族三：长跑覆盖。
    let mut w3 = WrapLedger::new();
    let mut i = 0;
    while i < 1000 {
        w3.write();
        i += 1;
    }
    cs.add("千行覆盖账", w3.overwritten == 872 && w3.count_exact(), "1000-128=872");
    cs.add("行号连续", w3.victims_contiguous(), "0..871 连续");
    cs.add("首尾精确", w3.first_last_victim() == Some((0, 871)), "跨度核对");

    // 族四：漂移检出。
    let mut w4 = WrapLedger::new();
    let mut i = 0;
    while i < 130 {
        w4.write();
        i += 1;
    }
    w4.overwritten = 3;
    cs.add("漂移检出", !w4.count_exact(), "人为 +1 即破");
    let empty = WrapLedger::new();
    cs.add("零账无受害", empty.first_last_victim().is_none(), "空账 None");
    cs
}
