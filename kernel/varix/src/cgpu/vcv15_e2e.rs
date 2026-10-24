//! CGPU-F3375 · V01 联调（vcv15_e2e）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3375`
//!
//! **判据（锚点原文）**：8 条、五断言、报告、三组、判据。
//!
//! **职责定位（锚点原文）**：联调：联调（验收端到端——端到端 8 条
//! （理念/矩阵/流程/判据/环境/模拟/联动/API——断言（四维正确/四
//! 阶段闭环/两级判据/Bench 映射/API 冻结可用——五断言；报告（V01
//! 联调报告；测试（8 条/五断言/报告三组）。
//!
//! # 一、联调是什么（与单元判据的差别）
//!
//! 单元判据（vcv02~vcv14 各自的 `run_vcvNN_checks`）证明**每个零件
//! 自身正确**；联调证明**零件拼起来的整机正确**——八个兄弟模块的
//! 判据组在本模块被实际调用（不是引用数字），全绿才算端到端通过。
//! 联调另带**五断言**：四维正确（vcv03 四维闭集）、四阶段闭环
//! （vcv04 四阶段）、两级判据（vcv05 两级）、Bench 映射（vcv06 映
//! 射线）、API 冻结可用（vcv09 七函数冻结表）——这些断言读兄弟模
//! 块的公开常量与函数，是**跨模块契约的运行时对账**。
//!
//! # 二、三组结构（锚点：8 条/五断言/报告三组）
//!
//! - **八条组**：逐条调用八个兄弟判据组并断言全绿；
//! - **五断言组**：五条跨模块契约断言；
//! - **报告组**：[`e2e_report`] 生成 V01 联调报告（逐条结果 +
//!   总判定），报告非空且含「全绿」才可交付组收口（F3376）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vcv02_philosophy::run_vcv02_checks;
use super::vcv03_matrix::{run_vcv03_checks, DIM_LIMITS};
use super::vcv04_flow::{run_vcv04_checks, STAGES};
use super::vcv05_criteria::run_vcv05_checks;
use super::vcv06_benchlink::run_vcv06_checks;
use super::vcv07_env::run_vcv07_checks;
use super::vcv08_telemetry::run_vcv08_checks;
use super::vcv09_apifreeze::{frozen_table_v1, run_vcv09_checks};
use super::vcv10_manual::run_vcv10_checks;
use super::vcv11_sim::run_vcv11_checks;
use super::vcv12_tlink::run_vcv12_checks;
use super::vcv13_ext::run_vcv13_checks;
use super::vcv14_plink::run_vcv14_checks;

// ---------------------------------------------------------------------------
// 一、常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const E2E_VERSION: &str = "V15-e2e-v1";

/// 端到端八条的模块清单（锚点原文八项）。
pub const E2E_MODULES: [&str; 8] = [
    "理念", "矩阵", "流程", "判据", "环境", "模拟", "联动", "API",
];

// ---------------------------------------------------------------------------
// 二、五断言（跨模块契约的运行时对账）
// ---------------------------------------------------------------------------

/// 断言一：四维正确（矩阵四维登记上限与锚点口径一致）。
pub fn assert_dim4() -> bool {
    DIM_LIMITS == (16, 12, 8, 24)
}

/// 断言二：四阶段闭环（预备→执行→报告→处置）。
pub fn assert_flow4() -> bool {
    STAGES == 4
}

/// 断言三：两级判据（vcv05 两级体系判据组全绿）。
pub fn assert_two_level() -> bool {
    run_vcv05_checks().all_passed()
}

/// 断言四：Bench 映射可用（vcv06 分数映射判据组全绿）。
pub fn assert_bench_map() -> bool {
    run_vcv06_checks().all_passed()
}

/// 断言五：API 冻结可用（七函数冻结表不变式成立）。
pub fn assert_api_frozen() -> bool {
    frozen_table_v1().len() == 7 && run_vcv09_checks().all_passed()
}

// ---------------------------------------------------------------------------
// 三、端到端执行与报告
// ---------------------------------------------------------------------------

/// 单模块联调行（模块名 → 是否全绿 → 绿项数）。
pub struct E2eLine {
    /// 模块名。
    pub module: &'static str,
    /// 全绿与否。
    pub green: bool,
    /// 绿项数。
    pub passed: usize,
    /// 总项数。
    pub total: usize,
}

/// 执行端到端八条（逐模块跑判据组并收集结果行）。
pub fn run_e2e8() -> Vec<E2eLine> {
    let sets: [(&'static str, CheckSet); 8] = [
        ("理念", run_vcv02_checks()),
        ("矩阵", run_vcv03_checks()),
        ("流程", run_vcv04_checks()),
        ("判据", run_vcv05_checks()),
        ("环境", run_vcv07_checks()),
        ("模拟", run_vcv11_checks()),
        ("联动", run_vcv12_checks()),
        ("API", run_vcv09_checks()),
    ];
    sets.into_iter()
        .map(|(m, s)| E2eLine {
            module: m,
            green: s.all_passed(),
            passed: s.passed_count(),
            total: s.len(),
        })
        .collect()
}

/// V01 联调报告（组收口 F3376 文档账的素材——非空且含总判定）。
pub fn e2e_report() -> String {
    let lines = run_e2e8();
    let all_green = lines.iter().all(|l| l.green);
    let mut r = String::from("== V01 联调报告（端到端八条） ==\n");
    for l in lines.iter() {
        r.push_str(&format!(
            "{}：{}（{}/{}）\n",
            l.module,
            if l.green { "绿" } else { "红" },
            l.passed,
            l.total
        ));
    }
    let five = [
        assert_dim4(),
        assert_flow4(),
        assert_two_level(),
        assert_bench_map(),
        assert_api_frozen(),
    ];
    let five_ok = five.iter().all(|b| *b);
    r.push_str(&format!(
        "五断言：{}（四维/四阶段/两级/Bench 映射/API 冻结）\n",
        if five_ok { "全过" } else { "有挂" }
    ));
    r.push_str(if all_green && five_ok {
        "总判定：全绿——V01 端到端通过\n"
    } else {
        "总判定：有红——组收口阻断\n"
    });
    r
}

// ---------------------------------------------------------------------------
// 四、测试三组（8 条 / 五断言 / 报告）
// ---------------------------------------------------------------------------

/// CGPU-F3375 域自检：8 条、五断言、报告、三组、判据。
pub fn run_vcv15_checks() -> CheckSet {
    let mut cs = CheckSet::new("vcv15_e2e");

    // --- 八条组：逐模块全绿 ---
    let lines = run_e2e8();
    cs.add(
        "八条齐备",
        lines.len() == 8 && lines.iter().map(|l| l.module).eq(E2E_MODULES.iter().copied()),
        "锚点端到端八条",
    );
    let all_green = lines.iter().all(|l| l.green);
    let total_checks: usize = lines.iter().map(|l| l.total).sum();
    cs.add_dyn(
        "八条全绿",
        all_green && total_checks > 0,
        &format!("端到端 {} 项全绿", total_checks),
    );

    // --- 五断言组 ---
    cs.add("断言一 四维正确", assert_dim4(), "vcv03 DIM_LIMITS==(16,12,8,24)");
    cs.add("断言二 四阶段闭环", assert_flow4(), "vcv04 STAGES==4");
    cs.add("断言三 两级判据", assert_two_level(), "vcv05 两级体系全绿");
    cs.add("断言四 Bench 映射", assert_bench_map(), "vcv06 分数映射全绿");
    cs.add("断言五 API 冻结可用", assert_api_frozen(), "vcv09 七函数冻结");

    // --- 补充八条：vcv10/13/14 也在域内（全量口径） ---
    cs.add(
        "手册/扩展/P 联动全绿",
        run_vcv10_checks().all_passed()
            && run_vcv13_checks().all_passed()
            && run_vcv14_checks().all_passed(),
        "V01 全模块口径",
    );

    // --- 报告组 ---
    let rep = e2e_report();
    cs.add(
        "报告非空含总判定",
        rep.contains("V01 联调报告") && rep.contains("总判定") && rep.contains("全绿"),
        "组收口文档账素材",
    );
    cs.add(
        "报告逐条可读",
        E2E_MODULES.iter().all(|m| rep.contains(m)),
        "八个模块名全部出现在报告",
    );

    // --- 版本指纹 ---
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in E2E_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    };
    cs.add("版本指纹非零", fp != 0, E2E_VERSION);

    cs
}
