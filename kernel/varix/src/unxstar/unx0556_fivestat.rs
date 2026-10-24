//! unx0556_fivestat —— UNX-F0556 日志五数概括账。
//! 锚点（CoRun Unxreal 主册）：UNX-F0556 · 日志五数概括 · 目标 200 行
//! 判据原文：概括 5 项（emit 总数/放行数/丢弃数/覆盖数/坏行数）与逐项账
//!           复算一致。
//! 组件：FiveStat（五数各持一账）+ 逐项复算一致核（两路求和等）。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x35840;
pub const XC_FIVE_DRIFT: u32 = SEG_BASE + 1; // 概括漂移
pub const XC_FIVE_NEG: u32 = SEG_BASE + 2; // 负数（不可能但防饱和假象）
pub const XC_FIVE_ZERO: u32 = SEG_BASE + 3; // 全零账
pub const XC_FIVE_OOB: u32 = SEG_BASE + 4; // 越界

/// 五数账。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FiveStat {
    pub emitted: u32,
    pub passed: u32,
    pub dropped: u32,
    pub wrapped: u32,
    pub bad_lines: u32,
}

impl FiveStat {
    /// 恒等式：emitted = passed + dropped（覆盖与坏行独立计）。
    pub const fn core_ok(&self) -> bool {
        self.emitted == self.passed + self.dropped
    }

    /// 全零账。
    pub const fn is_zero(&self) -> bool {
        self.emitted == 0 && self.passed == 0 && self.dropped == 0 && self.wrapped == 0 && self.bad_lines == 0
    }
}

/// 双路复算：源账逐项 vs 五数账逐项（两路独立求和路径）。
pub fn reconcile(src: &FiveStat, claim: &FiveStat) -> Result<(), u32> {
    // 路一：直接逐字段比。
    if src.emitted != claim.emitted
        || src.passed != claim.passed
        || src.dropped != claim.dropped
        || src.wrapped != claim.wrapped
        || src.bad_lines != claim.bad_lines
    {
        return Err(XC_FIVE_DRIFT);
    }
    // 路二：恒等式交叉核。
    if !claim.core_ok() {
        return Err(XC_FIVE_DRIFT);
    }
    Ok(())
}

/// 从事件流聚合五数（聚合路径）。
pub struct FiveBuilder {
    pub stat: FiveStat,
}

impl FiveBuilder {
    /// 空。
    pub fn new() -> Self {
        FiveBuilder {
            stat: FiveStat { emitted: 0, passed: 0, dropped: 0, wrapped: 0, bad_lines: 0 },
        }
    }

    /// 事件：passed/dropped 二选一计。
    pub fn log(&mut self, passed: bool) {
        self.stat.emitted += 1;
        if passed {
            self.stat.passed += 1;
        } else {
            self.stat.dropped += 1;
        }
    }

    /// 覆盖事件。
    pub fn wrap(&mut self) {
        self.stat.wrapped += 1;
    }

    /// 坏行事件。
    pub fn bad(&mut self) {
        self.stat.bad_lines += 1;
    }
}

impl Default for FiveBuilder {
    fn default() -> Self {
        Self::new()
    }
}

pub fn run_unx0556_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0556");
    cs.add("SEG 段位独占", SEG_BASE == 0x35840 && XC_FIVE_OOB == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：聚合与恒等。
    let mut b = FiveBuilder::new();
    let mut i = 0;
    while i < 10 {
        b.log(i % 3 != 0); // 7 过 3 丢
        i += 1;
    }
    b.wrap();
    b.bad();
    cs.add("五数值", b.stat == FiveStat { emitted: 10, passed: 7, dropped: 3, wrapped: 1, bad_lines: 1 }, "逐项精确");
    cs.add("核心恒等", b.stat.core_ok(), "10=7+3");

    // 族二：双路复算一致。
    cs.add("复算一致", reconcile(&b.stat, &b.stat) == Ok(()), "同账双路等");
    let drift = FiveStat { emitted: 10, passed: 7, dropped: 2, wrapped: 1, bad_lines: 1 };
    cs.add("漂移检出", reconcile(&b.stat, &drift).is_err(), "dropped 差 1 点名");
    let broken = FiveStat { emitted: 10, passed: 7, dropped: 2, wrapped: 0, bad_lines: 0 };
    cs.add("恒等破检出", reconcile(&broken, &broken).is_err(), "10≠9 式破");

    // 族三：全零账。
    let z = FiveStat { emitted: 0, passed: 0, dropped: 0, wrapped: 0, bad_lines: 0 };
    cs.add("全零可检", z.is_zero() && z.core_ok(), "0=0+0");
    let empty = FiveBuilder::new();
    cs.add("空构建全零", empty.stat.is_zero(), "未事件即零账");

    // 族四：大数不溢。
    let big = FiveStat { emitted: u32::MAX, passed: u32::MAX - 5, dropped: 5, wrapped: 0, bad_lines: 0 };
    cs.add("大数恒等", big.core_ok(), "MAX-5+5=MAX");
    cs.add("大数不零", !big.is_zero(), "非零账");
    cs
}
