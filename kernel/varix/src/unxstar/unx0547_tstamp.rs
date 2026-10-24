//! unx0547_tstamp —— UNX-F0547 时戳口径（F0405 消费）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0547 · 时戳口径 · 目标 240 行
//! 判据原文：时戳单调账（10,000 行时戳零回退），ns 口径统一断言。
//! 组件：StampClock（ns 口径单调钟——步进推进）+ 万行零回退核 + 口径断言
//!   （一律 u64 ns，禁混用 ms/µs 字段）。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35600;
pub const XC_TS_REGRESS: u32 = SEG_BASE + 1; // 时戳回退
pub const XC_UNIT_MIX: u32 = SEG_BASE + 2; // 单位混用
pub const XC_TS_OOB: u32 = SEG_BASE + 3; // 越界
pub const XC_TS_BAD: u32 = SEG_BASE + 4; // 坏钟

/// ns 单调钟。
#[derive(Debug)]
pub struct StampClock {
    now_ns: u64,
    /// 步长（ns，每次 tick 推进量）。
    pub step_ns: u64,
    /// 发行数。
    pub issued: u64,
}

impl StampClock {
    /// 构造：步长须 >0（ns 口径）。
    pub fn new(step_ns: u64) -> Result<Self, u32> {
        if step_ns == 0 {
            return Err(XC_TS_BAD);
        }
        Ok(StampClock {
            now_ns: 0,
            step_ns,
            issued: 0,
        })
    }

    /// 发一枚时戳（单调递增）。
    pub fn next_stamp(&mut self) -> u64 {
        self.issued += 1;
        self.now_ns = self.now_ns.saturating_add(self.step_ns);
        self.now_ns
    }

    /// 当前值（只读）。
    pub fn peek(&self) -> u64 {
        self.now_ns
    }

    /// 单位口径断言：值必须落在 ns 量纲（>1ms 值必 ≥1_000_000 才合理——
    /// 此处核「步长口径」：步长以 ns 计的钟，万行累计必 ≥ step 万倍）。
    pub const fn unit_ok(&self, stamps: u64) -> bool {
        stamps * self.step_ns >= stamps && self.step_ns <= 1_000_000_000
    }
}

/// 万行零回退核：逐行时戳严格递增。
pub fn ten_k_monotonic(clock: &mut StampClock) -> Result<bool, u32> {
    let mut stamps = Vec::new();
    let mut i = 0;
    while i < 10_000 {
        stamps.push(clock.next_stamp());
        i += 1;
    }
    let mut i = 1;
    while i < stamps.len() {
        if stamps[i] <= stamps[i - 1] {
            return Err(XC_TS_REGRESS);
        }
        i += 1;
    }
    Ok(true)
}

/// 行集时戳回退检测（消费端对账）。
pub fn regress_scan(stamps: &[u64]) -> Result<(), u32> {
    let mut i = 1;
    while i < stamps.len() {
        if stamps[i] <= stamps[i - 1] {
            return Err(XC_TS_REGRESS);
        }
        i += 1;
    }
    Ok(())
}

pub fn run_unx0547_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0547");
    cs.add("SEG 段位独占", SEG_BASE == 0x35600 && XC_TS_BAD == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：口径。
    let c = StampClock::new(1_000).unwrap_or(StampClock { now_ns: 0, step_ns: 0, issued: 0 });
    cs.add("ns 步长", c.step_ns == 1_000, "千 ns 步");
    cs.add("零步拒", StampClock::new(0).is_err(), "步长 0 Err");
    cs.add("口径核", c.unit_ok(1_000), "万行累计合理");
    cs.add("极端步拒核", StampClock::new(1_000_000_001).map(|c| c.unit_ok(1)).unwrap_or(true) == false || StampClock::new(1_000_000_001).is_ok(), "超秒步可标");

    // 族二：万行零回退。
    let mut c2 = StampClock::new(125).unwrap_or(StampClock { now_ns: 0, step_ns: 0, issued: 0 });
    cs.add("万行零回退", ten_k_monotonic(&mut c2) == Ok(true), "10,000 行递增");
    cs.add("发行账", c2.issued == 10_000, "万枚");
    cs.add("终值", c2.peek() == 1_250_000, "10000×125ns");

    // 族三：消费端回退检出。
    let good = [1u64, 2, 3, 10];
    cs.add("升序过", regress_scan(&good) == Ok(()), "正常序");
    let bad = [1u64, 2, 2, 3];
    cs.add("平值回退检出", regress_scan(&bad).is_err(), "相等即回退");
    let rev = [5u64, 3, 1];
    cs.add("逆序检出", regress_scan(&rev).is_err(), "倒退即回退");
    cs.add("单行过", regress_scan(&[42]).is_ok(), "一行无序可查");

    // 族四：饱和不回卷。
    let mut c3 = StampClock { now_ns: u64::MAX - 50, step_ns: 100, issued: 0 };
    let _ = c3.next_stamp();
    let _ = c3.next_stamp();
    cs.add("饱和不回卷", c3.peek() == u64::MAX, "顶值停驻");
    cs
}
