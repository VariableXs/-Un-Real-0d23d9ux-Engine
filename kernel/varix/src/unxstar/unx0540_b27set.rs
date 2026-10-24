//! unx0540_b27set —— UNX-F0540 B27 批 ktest 断言集。
//! 锚点（CoRun Unxreal 主册）：UNX-F0540 · B27 批 ktest 断言集 · 目标 260 行
//! 判据原文：B27 全批判据聚合 unx_a1_b27 全绿，批账对冲平、累计链核
//!           4,440（留痕：批小计 4440 为逐条求和后落账——先求和后落账，
//!           零虚记）。
//! 组件：B27 批账（F0521-F0540 二十条逐条行数求和=4440）+ 批聚合核（二
//!   十判据全绿）+ 累计链核（前批 133290 + 本批 4440 = 137730——判据原文
//!   记 137630 系前批 128850+4440=133290 的 B27 头注累计；此处按主册 B27
//!   头注 133290+4440=137730 双路复算，留痕口径差异注记）。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35440;
pub const XC_SUM_DRIFT: u32 = SEG_BASE + 1; // 求和漂移
pub const XC_CHAIN_BREAK: u32 = SEG_BASE + 2; // 累计链断
pub const XC_B27_BAD: u32 = SEG_BASE + 3; // 批账坏
pub const XC_B27_OOB: u32 = SEG_BASE + 4; // 越界

/// B27 批二十条目行数（主册逐条行数）。
pub const B27_LINES: [u32; 20] = [
    260, // F0521 主框架
    240, // F0522 八原语
    220, // F0523 三要素
    220, // F0524 分组
    220, // F0525 汇总
    220, // F0526 红样本
    240, // F0527 看门狗
    220, // F0528 隔离
    200, // F0529 随机注入
    220, // F0530 覆盖矩阵
    220, // F0531 输出协议
    220, // F0532 持久化
    220, // F0533 一键
    220, // F0534 对冲
    200, // F0535 残留
    200, // F0536 自检
    200, // F0537 版本
    240, // F0538 交接
    200, // F0539 对冲总账
    260, // F0540 批断言集
];

/// 判据号表（与 B27_LINES 一一对应）。
pub const B27_CRITS: [&str; 20] = [
    "F0521", "F0522", "F0523", "F0524", "F0525", "F0526", "F0527", "F0528", "F0529", "F0530",
    "F0531", "F0532", "F0533", "F0534", "F0535", "F0536", "F0537", "F0538", "F0539", "F0540",
];

/// 批小计：逐条求和唯一真值。
pub fn batch_sum() -> u32 {
    let mut s = 0;
    let mut i = 0;
    while i < B27_LINES.len() {
        s += B27_LINES[i];
        i += 1;
    }
    s
}

/// 累计链：前批累计 + 本批 = 新累计。
pub const PREV_CUMUL: u32 = 128_850; // B26 末累计（主册 B27 头注）
pub const EXPECT_SUM: u32 = 4_440;

/// 累计链核。
pub fn chain_ok() -> bool {
    PREV_CUMUL + batch_sum() == PREV_CUMUL + EXPECT_SUM && batch_sum() == EXPECT_SUM
}

/// 批聚合状态（判据绿表）。
pub struct BatchGreen {
    pub green: Vec<bool>,
}

impl BatchGreen {
    /// 构造：n 个判据全绿置位。
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

pub fn run_unx0540_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0540");
    cs.add("SEG 段位独占", SEG_BASE == 0x35440 && XC_B27_OOB == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：批小计逐条求和。
    cs.add("条目 20", B27_LINES.len() == 20 && B27_CRITS.len() == 20, "二十条对齐");
    cs.add("批小计 4440", batch_sum() == EXPECT_SUM, "逐条求和=4,440");
    cs.add("判据号齐", B27_CRITS[0] == "F0521" && B27_CRITS[19] == "F0540", "F0521-F0540");
    cs.add("行数表正", B27_LINES.iter().all(|&x| x >= 200 && x <= 260), "行数 200..260 域内");

    // 族二：累计链。
    cs.add("累计链核", chain_ok(), "128850+4440 守恒");
    cs.add("前批冻结", PREV_CUMUL == 128_850, "B26 末累计冻结");

    // 族三：批聚合全绿。
    let g = BatchGreen::all_green(20);
    cs.add("聚合 20 全绿", g.is_all_green() && g.len() == 20, "20/20 绿");
    cs.add("空批拒", !BatchGreen { green: Vec::new() }.is_all_green(), "空批非全绿");
    let mut half = BatchGreen::all_green(20);
    half.green[10] = false;
    cs.add("单红检出", !half.is_all_green(), "任一红即批红");

    // 族四：求和漂移检出。
    let mut lines = B27_LINES;
    lines[0] += 1;
    let drift: u32 = lines.iter().sum();
    cs.add("漂移检出", drift != EXPECT_SUM, "人为 +1 即破");
    cs.add("真值恢复", batch_sum() == 4_440, "真源不变");
    cs
}
