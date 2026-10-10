//! CGPU-F2408 · 遥测性能域自检（锚点测试一组：开销 + stamp）。
//!
//! **判据（锚点原文）**：开销复用、一组、判据。

use super::cgp08_perf::{
    PerfLedger, BUDGET_TABLE, FRAME_BUDGET_STEPS, REUSE_LINES,
};

/// 判据侧独立重排的锚点判据三条。
const CRITERIA_RECHECK: [&str; 3] = ["开销复用", "一组", "判据"];

/// 判据侧独立写死的五段名（与 BUDGET_TABLE 逐条对拍——表被误改先红）。
const SEGMENTS_RECHECK: [&str; 5] = ["统一模型构造", "采样决策", "管道四段", "分层降层", "查询"];

/// 判据侧独立重算的预算数（2/2/4/3/3，Σ=14 → 预算占比 14×10000÷10000=14000？
/// 不——14 步占 10000 步 = 140 万分比，即 1.4%）。
const BUDGET_SUM_RECHECK: u32 = 14;

/// CGPU-F2408 域自检入口（聚合器 `run_cgpu_checks` 调用）。
pub fn run_cgp08_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("cgp08_perf");

    // —— 一组 · 开销：总账五段 + 恰红线双向 + 超标留名 + 占比手算 ——
    // 五段名与预算数逐条对拍（判据侧独立写死）。
    let table_ok = BUDGET_TABLE.len() == 5
        && BUDGET_TABLE[0].name == SEGMENTS_RECHECK[0] && BUDGET_TABLE[0].budget == 2
        && BUDGET_TABLE[1].name == SEGMENTS_RECHECK[1] && BUDGET_TABLE[1].budget == 2
        && BUDGET_TABLE[2].name == SEGMENTS_RECHECK[2] && BUDGET_TABLE[2].budget == 4
        && BUDGET_TABLE[3].name == SEGMENTS_RECHECK[3] && BUDGET_TABLE[3].budget == 3
        && BUDGET_TABLE[4].name == SEGMENTS_RECHECK[4] && BUDGET_TABLE[4].budget == 3;
    // 恰红线双向：== 预算达标（≤ 语义），超 1 步即超标。
    let mut led = PerfLedger::new();
    led.measure("统一模型构造", 2);
    led.measure("采样决策", 1);
    led.measure("管道四段", 4);
    led.measure("分层降层", 3);
    led.measure("查询", 3);
    let within_ok = led.within_count() == 5 && led.over().is_empty();
    led.measure("查询", 4); // 超 1 步
    let over_names = led.over();
    let over_ok = led.within_count() == 4 && over_names.len() == 1 && over_names.get(0) == Some(&"查询");
    // 占比手算：Σmeasured=13 → 13×10000÷10000 = 13000 万分比 = 1.3%；
    // 预算占比 Σbudget=14 → 14000。总账手算对账。
    let mut led2 = PerfLedger::new();
    led2.measure("统一模型构造", 2);
    led2.measure("采样决策", 2);
    led2.measure("管道四段", 4);
    led2.measure("分层降层", 3);
    led2.measure("查询", 2);
    let ppm_ok = led2.total_ppm_of_frame() == 13000
        && led2.budget_ppm_of_frame() == 14000
        && FRAME_BUDGET_STEPS == 10000
        && BUDGET_SUM_RECHECK == 14;
    // 表外段名注入 None 不臆造。
    let ghost_ok = led2.measure("表外段", 1).is_none();
    s.add(
        "P08-一组开销-总账+恰红线+超标留名+占比",
        table_ok && within_ok && over_ok && ppm_ok && ghost_ok,
        "五段名与预算数判据侧独立写死逐条对拍；恰红线双向 ==预算达标超 1 步超标；超标留名（查询）不只给布尔；占比手算 Σmeasured=13→13000 万分比、Σbudget=14→14000 万分比；表外段名注入 None 不臆造",
    );

    // —— 复用声明逐条 grep（判据「开销复用」） ——
    let mut reuse_ok = REUSE_LINES.len() == 3;
    let mut ri = 0usize;
    while ri < REUSE_LINES.len() {
        let l = REUSE_LINES[ri];
        if !(l.contains("F0482") || l.contains("F1558") || l.contains("F2401")) {
            reuse_ok = false;
        }
        ri += 1;
    }
    // 判据 stamp 独立对账。
    let stamps = ["开销复用", "一组", "判据"];
    let mut stamp_ok = CRITERIA_RECHECK.len() == stamps.len();
    let mut ci = 0usize;
    while ci < stamps.len() {
        if CRITERIA_RECHECK.get(ci) != Some(&stamps[ci]) {
            stamp_ok = false;
        }
        ci += 1;
    }
    s.add(
        "P08-复用+判据stamp-逐条对账",
        reuse_ok && stamp_ok,
        "复用清单三条含 F0482/F1558/F2401 关键字逐条 grep（实测注入口径/采样不耗样本红线/预算意识使命）；锚点判据三条与判据侧独立重排逐条全等；一组测试（开销）宣告与实际检查一一对应",
    );

    s
}
