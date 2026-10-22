//! VE-F1010 · 域自检外壳：只承载判据编排与自描述表；判据实现拆至子目录
//! `forms`（形态/优先/色键/截断/拒绝）与 `premul`（预乘/渐变/故障码/判据集）。
//! `set.add` 序列与拆分前逐条一致，`vef12_checks::run_vef12_checks` 公开路径零变化。
//! 判据映射与弱门禁原为头注文本，本次整改原样迁入 `CRITERIA_MAP`/`WEAK_GATES`
//! 两表（内容零删减，文件行数守恒 56 行）。

#![allow(clippy::needless_range_loop)]

mod forms;
mod premul;

use crate::checks::CheckSet;

// 判据锚点（原文见 VE-F1010 规则条目，对应 `CRITERIA_MAP` 左列）：前四项由 `forms`
// 承载，后四项由 `premul` 承载；子文件内 grid_all_true/grid_max_u32 等均为纯计算
// 骨架，判据互不调用；`run_vef12_checks` 注入顺序即判据序，与 `CRITERIA_MAP` 行序对应。

/// 锚点原文 → 自检项判据映射（自原模块头注迁入，逐条对应，不可增删）。
#[allow(dead_code)]
pub const CRITERIA_MAP: [(&str, &str); 8] = [
    ("全透明形态", "C1010-形态-*"),
    ("优先级规则", "C1010-优先-*"),
    ("精确匹配", "C1010-色键-*"),
    ("错误路径（截断告警 / 位深拒绝）", "C1010-截断-* / C1010-拒绝-*"),
    ("预乘转换正确", "C1010-预乘-*"),
    ("透明渐变质量", "C1010-渐变-*"),
    ("错误路径与三要素", "C1010-错误-*"),
    ("判据承载", "C1010-判据集-*"),
];

/// 预乘判据口径（自原头注迁入）：roundtrip ≤1 LSB 由精确分子路径全域零误差
/// 达成；量化路径上界按 F0625 单向阈值同源口径实测声明、独立重算双向钉死。
#[allow(dead_code)]
pub const PREMUL_TOLERANCE_LSB: u8 = 1;

/// 弱门禁自律（本域最易犯的五种，自原头注迁入；新增判据前先对照此表）。
#[allow(dead_code)]
pub const WEAK_GATES: [(&str, &str); 5] = [
    ("roundtrip ≤1 LSB 不能抽样", "判据真遍历 8 位全域 65536 组；抽样会漏掉高 alpha 段舍入边界"),
    ("精确匹配不能只断「键匹配自己」", "键与差 1 的邻值必须不匹配——只断正向，<= 式范围匹配会全绿通过"),
    ("优先级不能只断「返回 AlphaChannel」", "还要断告警计数——静默忽略正是锚点点名要防的事故"),
    ("码段不能只断「在本段」", "还断「不在撞段值」——撞号不报编译错，只钉前者挡不住运行期静默混流"),
    ("「两实现一致」不能在实现内部对拍", "判据层独立调两条路径逐值比，任何一条被改坏都会被抓"),
];
/// F1010 域自检入口：各判据域提取为私有函数，按原顺序逐域注入判据（调用顺序
/// 即判据序，与 `CRITERIA_MAP` 行序一一对应，不可调换）。
pub fn run_vef12_checks() -> CheckSet {
    let mut s = CheckSet::new("ve-f-f1010");
    forms::check_forms(&mut s);
    forms::check_priority(&mut s);
    forms::check_color_key(&mut s);
    forms::check_truncation(&mut s);
    forms::check_key_reject(&mut s);
    premul::check_premul(&mut s);
    premul::check_gradient(&mut s);
    premul::check_faults(&mut s);
    premul::check_judge_set(&mut s);
    s
}
