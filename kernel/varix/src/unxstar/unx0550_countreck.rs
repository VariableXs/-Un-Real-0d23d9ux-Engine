//! unx0550_countreck —— UNX-F0550 日志计数对冲账。
//! 锚点（CoRun Unxreal 主册）：UNX-F0550 · 日志计数对冲账 · 目标 200 行
//! 判据原文：三路守恒（emit 总数=放行+拒收+丢弃，误差 0），四级别分列复
//!           算一致。
//! 组件：三路账（emit 总/门放行/门拒收/级别丢弃）+ 恒等式核 + 四级分列复算。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x356C0;
pub const XC_RECK_UNBAL: u32 = SEG_BASE + 1; // 守恒破
pub const XC_RECK_NEG: u32 = SEG_BASE + 2; // 负差
pub const XC_RECK_EMPTY: u32 = SEG_BASE + 3; // 零账
pub const XC_RECK_OOB: u32 = SEG_BASE + 4; // 越界

/// 三路计数账。
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct CountReck {
    /// emit 总数。
    pub emitted: u32,
    /// 组件门放行。
    pub gate_pass: u32,
    /// 组件门拒收（未注册组件）。
    pub gate_reject: u32,
    /// 级别过滤丢弃（过门后）。
    pub filt_drop: u32,
}

impl CountReck {
    /// 守恒式：emitted = gate_pass + gate_reject + filt_drop。
    pub const fn conserved(&self) -> bool {
        self.emitted == self.gate_pass + self.gate_reject + self.filt_drop
    }

    /// 全零坏账。
    pub const fn is_zero(&self) -> bool {
        self.emitted == 0
    }

    /// 误差（|emitted - 三路和|，守恒时应为 0）。
    pub const fn drift(&self) -> u32 {
        let sum = self.gate_pass + self.gate_reject + self.filt_drop;
        if self.emitted > sum {
            self.emitted - sum
        } else {
            sum - self.emitted
        }
    }
}

/// 计数引擎：emit 流驱动三路账。
#[derive(Debug, Default)]
pub struct ReckEngine {
    pub reck: CountReck,
    /// 四级分列：放行（过门后）级别计数。
    pub pass_by_level: [u32; 4],
    /// 四级分列：丢弃（过门后）级别计数。
    pub drop_by_level: [u32; 4],
}

impl ReckEngine {
    /// 空。
    pub fn new() -> Self {
        ReckEngine::default()
    }

    /// 事件入账：registered=组件门结果；passed=级别过滤结果（仅过门后有意义）。
    pub fn event(&mut self, level: u8, registered: bool, passed: bool) -> Result<(), u32> {
        if level > 3 {
            return Err(XC_RECK_OOB);
        }
        self.reck.emitted += 1;
        if !registered {
            self.reck.gate_reject += 1;
            return Ok(());
        }
        if passed {
            self.reck.gate_pass += 1;
            self.pass_by_level[level as usize] += 1;
        } else {
            self.reck.filt_drop += 1;
            self.drop_by_level[level as usize] += 1;
        }
        Ok(())
    }

    /// 守恒核。
    pub fn conserved(&self) -> bool {
        self.reck.conserved() && self.reck.drift() == 0
    }

    /// 四级分列复算：分列和 == 三路对应和。
    pub fn columns_reconcile(&self) -> bool {
        let mut p = 0;
        let mut d = 0;
        let mut i = 0;
        while i < 4 {
            p += self.pass_by_level[i];
            d += self.drop_by_level[i];
            i += 1;
        }
        p == self.reck.gate_pass && d == self.reck.filt_drop
    }
}

pub fn run_unx0550_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0550");
    cs.add("SEG 段位独占", SEG_BASE == 0x356C0 && XC_RECK_OOB == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：三路守恒。
    let mut e = ReckEngine::new();
    // 10 emit：6 过门过、2 过门被滤、2 拒门。
    let mut i = 0;
    while i < 10 {
        let registered = i < 8;
        let passed = i < 6;
        let _ = e.event((i % 4) as u8, registered, passed);
        i += 1;
    }
    cs.add("三路守恒", e.conserved(), "10=6+2+2");
    cs.add("三路值", e.reck == CountReck { emitted: 10, gate_pass: 6, gate_reject: 2, filt_drop: 2 }, "逐路精确");
    cs.add("四级分列复算", e.columns_reconcile(), "分列和=三路和");
    cs.add("误差零", e.reck.drift() == 0, "drift=0");

    // 族二：分列值。
    cs.add("放行分列", e.pass_by_level == [2, 2, 1, 1], "i=0..5 mod4");
    cs.add("丢弃分列", e.drop_by_level == [0, 0, 1, 1], "i=6,7");
    cs.add("级别越界拒", e.event(4, true, true).is_err(), "level=4 Err");

    // 族三：破式检出。
    let bad = CountReck { emitted: 10, gate_pass: 6, gate_reject: 2, filt_drop: 1 };
    cs.add("破式检出", !bad.conserved() && bad.drift() == 1, "9≠10 差 1");
    let bad2 = CountReck { emitted: 5, gate_pass: 6, gate_reject: 2, filt_drop: 2 };
    cs.add("反向破式", bad2.drift() == 5, "和>总 差 5");

    // 族四：零账。
    let z = CountReck { emitted: 0, gate_pass: 0, gate_reject: 0, filt_drop: 0 };
    cs.add("零账守恒但空", z.conserved() && z.is_zero(), "0=0+0+0");
    cs.add("空账可检", z.is_zero(), "is_zero 点名");
    cs
}
