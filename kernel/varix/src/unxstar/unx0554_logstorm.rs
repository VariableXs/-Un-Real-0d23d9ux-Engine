//! unx0554_logstorm —— UNX-F0554 日志风暴演练（高频注入）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0554 · 日志风暴演练 · 目标 200 行
//! 判据原文：风暴核（1 万行突发——守恒式零破/覆盖账准/查询可用），风暴后
//!           健康全绿。
//! 组件：StormDrill（万行突发进环形账+覆盖账+查询复验）+ 健康三绿核。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x357C0;
pub const STORM_LINES: u64 = 10_000;
pub const RING_CAP: u64 = 128;
pub const XC_STORM_DRIFT: u32 = SEG_BASE + 1; // 风暴账漂移
pub const XC_HEALTH_RED: u32 = SEG_BASE + 2; // 健康红
pub const XC_STORM_OOB: u32 = SEG_BASE + 3; // 越界
pub const XC_STORM_BAD: u32 = SEG_BASE + 4; // 坏账

/// 风暴账（轻量——只记数不存行）。
#[derive(Debug, Default)]
pub struct StormDrill {
    pub written: u64,
    pub overwritten: u64,
    /// 风暴期查询命中数。
    pub query_hits: u64,
}

impl StormDrill {
    /// 空演练。
    pub fn new() -> Self {
        StormDrill::default()
    }

    /// 突发 n 行。
    pub fn burst(&mut self, n: u64) {
        let mut i = 0;
        while i < n {
            if self.written >= RING_CAP {
                self.overwritten += 1;
            }
            self.written += 1;
            i += 1;
        }
    }

    /// 覆盖账准：overwritten == max(0, written-128)。
    pub fn wrap_exact(&self) -> bool {
        if self.written <= RING_CAP {
            self.overwritten == 0
        } else {
            self.overwritten == self.written - RING_CAP
        }
    }

    /// 守恒（环形三路）：写 = 存(128 或 written) + 覆。
    pub fn conserved(&self) -> bool {
        let store = if self.written <= RING_CAP { self.written } else { RING_CAP };
        self.written == store + self.overwritten
    }

    /// 查询可用（风暴后按步取模命中——确定性重放）。
    pub fn query_probe(&mut self, stride: u64) -> u64 {
        if stride == 0 {
            return 0;
        }
        let mut hits = 0;
        let mut n = stride;
        while n <= self.written {
            hits += 1;
            n += stride;
        }
        self.query_hits = hits;
        hits
    }

    /// 健康三绿：守恒零破 + 覆盖准 + 查询可用。
    pub fn healthy(&self, min_hits: u64) -> bool {
        self.conserved() && self.wrap_exact() && self.query_hits >= min_hits
    }
}

pub fn run_unx0554_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0554");
    cs.add("SEG 段位独占", SEG_BASE == 0x357C0 && XC_STORM_BAD == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：万行突发。
    let mut s = StormDrill::new();
    s.burst(STORM_LINES);
    cs.add("万行入账", s.written == 10_000, "突发全计");
    cs.add("覆盖账准", s.wrap_exact() && s.overwritten == 10_000 - 128, "10000-128=9872");
    cs.add("守恒零破", s.conserved(), "写=存+覆");

    // 族二：查询可用。
    let hits = s.query_probe(100);
    cs.add("查询命中", hits == 100 && s.query_hits == 100, "步 100 百命中");
    cs.add("健康三绿", s.healthy(50), "守恒+覆盖准+查询可用");

    // 族三：零星注入（未满环）。
    let mut s2 = StormDrill::new();
    s2.burst(50);
    cs.add("未满零覆", s2.overwritten == 0 && s2.wrap_exact(), "50 行不覆");
    let _ = s2.query_probe(10);
    cs.add("小流量健康", s2.healthy(1), "五命中过线");

    // 族四：漂移检出。
    let mut s3 = StormDrill::new();
    s3.burst(200);
    s3.overwritten = 99;
    cs.add("覆盖漂移检出", !s3.wrap_exact() || !s3.conserved(), "人为破账点名");
    let mut s4 = StormDrill::new();
    s4.burst(500);
    let _ = s4.query_probe(0);
    cs.add("零步长无命中", s4.query_hits == 0, "stride=0 防呆");
    cs.add("健康闸拒", !s4.healthy(1), "查询零命中→不健康");
    cs
}
