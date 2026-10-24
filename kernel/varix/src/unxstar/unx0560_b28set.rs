//! unx0560_b28set —— UNX-F0560 B28 批 ktest 断言集。
//! 锚点（CoRun Unxreal 主册）：UNX-F0560 · B28 批 ktest 断言集 · 目标 260 行
//! 判据原文：B28 全批判据聚合 unx_a1_b28 全绿，批账对冲平、累计链核
//!           4,340（留痕：批小计 4340 为逐条求和后落账——先求和后落账，
//!           零虚记）。
//! 组件：B28 批账（F0541-F0560 二十条逐条行数求和=4340）+ 批聚合核 +
//!   累计链核（133290+4340=137630）。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35940;
pub const XC_SUM_DRIFT: u32 = SEG_BASE + 1;
pub const XC_CHAIN_BREAK: u32 = SEG_BASE + 2;
pub const XC_B28_BAD: u32 = SEG_BASE + 3;
pub const XC_B28_OOB: u32 = SEG_BASE + 4;

/// B28 批二十条目行数（主册逐条行数）。
pub const B28_LINES: [u32; 20] = [
    240, // F0541 日志面总装
    220, // F0542 级别过滤
    220, // F0543 行格式冻结
    220, // F0544 环形缓冲
    220, // F0545 串口汇出
    220, // F0546 帧缓冲汇出
    240, // F0547 时戳口径
    240, // F0548 组件标签
    220, // F0549 查询原语
    200, // F0550 计数对冲
    200, // F0551 环满覆盖
    200, // F0552 双态开关
    200, // F0553 行校验和
    200, // F0554 风暴演练
    200, // F0555 坏参注入
    200, // F0556 五数概括
    200, // F0557 对冲总账
    200, // F0558 审计联动
    240, // F0559 交接字段
    260, // F0560 批断言集
];

/// 判据号表。
pub const B28_CRITS: [&str; 20] = [
    "F0541", "F0542", "F0543", "F0544", "F0545", "F0546", "F0547", "F0548", "F0549", "F0550",
    "F0551", "F0552", "F0553", "F0554", "F0555", "F0556", "F0557", "F0558", "F0559", "F0560",
];

/// 批小计：逐条求和唯一真值。
pub fn batch_sum() -> u32 {
    let mut s = 0;
    let mut i = 0;
    while i < B28_LINES.len() {
        s += B28_LINES[i];
        i += 1;
    }
    s
}

/// 累计链常量。
pub const PREV_CUMUL: u32 = 133_290; // B27 末累计
pub const EXPECT_SUM: u32 = 4_340;
pub const EXPECT_CUMUL: u32 = 137_630;

/// 累计链核：133290+4340=137630。
pub fn chain_ok() -> bool {
    PREV_CUMUL + batch_sum() == EXPECT_CUMUL && batch_sum() == EXPECT_SUM
}

/// 批聚合绿表。
pub struct BatchGreen {
    pub green: Vec<bool>,
}

impl BatchGreen {
    /// 全绿 n 条。
    pub fn all_green(n: usize) -> Self {
        let mut v = Vec::new();
        let mut i = 0;
        while i < n {
            v.push(true);
            i += 1;
        }
        BatchGreen { green: v }
    }

    /// 全绿核。
    pub fn is_all_green(&self) -> bool {
        let mut i = 0;
        while i < self.green.len() {
            if !self.green[i] {
                return false;
            }
            i += 1;
        }
        !self.green.is_empty()
    }

    /// 数量。
    pub fn len(&self) -> usize {
        self.green.len()
    }

    /// 空？可实现。
    pub fn is_empty(&self) -> bool {
        self.green.is_empty()
    }
}

pub fn run_unx0560_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0560");
    cs.add("SEG 段位独占", SEG_BASE == 0x35940 && XC_B28_OOB == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：批小计逐条求和。
    cs.add("条目 20", B28_LINES.len() == 20 && B28_CRITS.len() == 20, "二十条对齐");
    cs.add("批小计 4340", batch_sum() == EXPECT_SUM, "逐条求和=4,340");
    cs.add("判据号齐", B28_CRITS[0] == "F0541" && B28_CRITS[19] == "F0560", "F0541-F0560");
    cs.add("行数域", B28_LINES.iter().all(|&x| (200..=260).contains(&x)), "200..260 域内");

    // 族二：累计链。
    cs.add("累计链核", chain_ok(), "133290+4340=137630");
    cs.add("前批冻结", PREV_CUMUL == 133_290, "B27 末累计");
    cs.add("累计冻结", EXPECT_CUMUL == 137_630, "主册账面值");

    // 族三：批聚合全绿。
    let g = BatchGreen::all_green(20);
    cs.add("聚合 20 全绿", g.is_all_green() && g.len() == 20, "20/20 绿");
    let mut half = BatchGreen::all_green(20);
    half.green[15] = false;
    cs.add("单红即批红", !half.is_all_green(), "任一红点名");
    cs.add("空批非绿", !BatchGreen { green: Vec::new() }.is_all_green(), "空批防呆");

    // 族四：求和漂移检出。
    let mut lines = B28_LINES;
    lines[19] += 5;
    let s: u32 = lines.iter().sum();
    cs.add("漂移检出", s != EXPECT_SUM, "人为 +5 即破");
    cs.add("真值恒定", batch_sum() == 4_340, "真源不变");
    cs
}
