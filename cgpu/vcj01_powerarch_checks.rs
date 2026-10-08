//! CGPU-F1441 判据层：J 域开工与功耗架构总览（锚点判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1441`
//!
//! **锚点原文八条判据 → 本层判据族**：
//!
//! | 锚点判据 | 判据族 | 要点 |
//! |---|---|---|
//! | 域使命 | `J01-MISS-*` | 四条款字面量独立对拍 |
//! | 主题映射 | `J01-THEME-*` | 五主题闭集 + 十组官方名逐字对拍 + 连续守恒独立重算 |
//! | 三处兑现核验 | `J01-INTEG-*` | 核验记录齐 + 兑现对字面量独立写死 + 指纹独立 FNV 对拍 |
//! | 五段架构 | `J01-PIPE-*` | 五段闭集 + 单向一步 + 跳段/回退双向拒绝 |
//! | 边界 | `J01-BOUND-*` | 边界字面量 + 直写拒绝/系统接口放行双向 |
//! | 采样开销 | `J01-PERF-*` | 实耗 ≤ 预算独立重算 + 采样不耗样本（读数稳定） |
//! | 风险 | `J01-RISK-*` | 三风险闭集 + 回退非空且逐条区分 |
//! | 判据 | `J01-META-*` | 条数对账/截断/码段独占 != 防自判死/码互异 |
//!
//! # 本层核心纪律：判据侧独立重算，不向被测问答案
//!
//! 十组表若只断「GROUP_PLANS.len()==10」是自证式——本层**独立写死**
//! 官方组名与区间字面量逐条对拍，且首尾连续性用判据侧重算（被测表
//! 改一个组名或挪一个边界立即红）。核验记录指纹用独立 FNV 第二实现
//! 对拍（防同源恒绿）。

// ---------------------------------------------------------------------------
// 导入
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

use super::vcj01_powerarch::*;

// ---------------------------------------------------------------------------
// 判据侧独立参照（不向被测要答案）
// ---------------------------------------------------------------------------

/// 判据侧独立写死的域使命四条款。
const EXP_MISSION: [&str; 4] = [
    "让每一毫瓦都被看见",
    "让每一毫瓦都被记账",
    "让每一毫瓦都被调度",
    "80 帧合同在功耗受限设备上的兑现保障",
];

/// 判据侧独立写死的五主题标签。
const EXP_THEMES: [&str; 5] = ["功耗感知调度", "热降档", "续航模式", "风扇联动", "功耗遥测"];

/// 判据侧独立写死的十组（官方批次标题逐字 + 区间）。
const EXP_GROUPS: [(u8, &str, u32, u32); 10] = [
    (1, "功耗遥测与域架构组", 1441, 1456),
    (2, "功耗感知调度组", 1457, 1472),
    (3, "热降档组", 1473, 1488),
    (4, "续航模式组", 1489, 1504),
    (5, "风扇与散热联动组", 1505, 1520),
    (6, "功耗预算与仲裁组", 1521, 1536),
    (7, "场景功耗策略组", 1537, 1552),
    (8, "功耗遥测与报告组", 1553, 1568),
    (9, "J 域预备与自查组", 1569, 1584),
    (10, "J 域收口组", 1585, 1600),
];

/// 判据侧独立写死的三处集成兑现对。
const EXP_SLOTS: [(u32, u32, u32); 3] = [(406, 1458, 1459), (454, 1473, 1475), (590, 1461, 1468)];

/// 判据侧独立写死的边界两条。
const EXP_BOUNDARIES: [&str; 2] = [
    "不控制硬件风扇转速直写——经系统接口联动",
    "不承诺安培级计量精度——传感器精度即上限声明",
];

/// 全部诊断码（判据侧点名）。
const ALL_CODES: [VjCode; 4] = [
    VjCode::SAMPLE_OVER_BUDGET,
    VjCode::THEME_OUT_OF_TABLE,
    VjCode::RECORDS_INCOMPLETE,
    VjCode::BOUNDARY_VIOLATION,
];

/// 判据侧独立实现的核验记录 FNV（与被测同口径、代码独立）。
fn alt_records_digest(records: &[IntegrationRecord]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for r in records {
        for b in r.note.as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= r.caller;
        h = h.wrapping_mul(0x0100_0193);
        h ^= r.compatible as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 预期判据条数。
const EXPECTED_CHECKS: usize = 22;

// ---------------------------------------------------------------------------
// 主判据
// ---------------------------------------------------------------------------

/// J01 域自检入口。
pub fn run_vcj01_checks() -> CheckSet {
    let mut s = CheckSet::new("cgpu-power");

    // ================= 一、域使命（J01-MISS-*） =================

    // MISS-1：四条款字面量逐字对拍。
    let mission_ok = MISSION_CLAUSES.len() == 4
        && MISSION_CLAUSES[0] == EXP_MISSION[0]
        && MISSION_CLAUSES[1] == EXP_MISSION[1]
        && MISSION_CLAUSES[2] == EXP_MISSION[2]
        && MISSION_CLAUSES[3] == EXP_MISSION[3];
    s.add("J01-MISS-域使命四条款对拍", mission_ok, "");

    // MISS-2：域界宣言（F1441 开工、F1600 收官、160 项守恒常量）。
    s.add(
        "J01-MISS-域界宣言",
        J_DOMAIN_FIRST == 1441
            && J_DOMAIN_LAST == 1600
            && J_DOMAIN_TOTAL == 160
            && J_DOMAIN_LAST - J_DOMAIN_FIRST + 1 == J_DOMAIN_TOTAL,
        "",
    );

    // ================= 二、主题映射（J01-THEME-*） =================

    // THEME-1：五主题闭集判据侧独立标签对拍。
    let mut theme_ok = PowerTheme::ALL.len() == THEME_COUNT && THEME_COUNT == 5;
    for (i, t) in PowerTheme::ALL.iter().enumerate() {
        if t.label() != EXP_THEMES[i] {
            theme_ok = false;
        }
    }
    s.add("J01-THEME-五主题闭集对拍", theme_ok, "");

    // THEME-2：十组官方组名+区间逐条对拍（判据侧独立写死）。
    let mut group_ok = GROUP_PLANS.len() == 10;
    for (i, g) in GROUP_PLANS.iter().enumerate() {
        let e = EXP_GROUPS[i];
        if g.group != e.0 || g.name != e.1 || g.first != e.2 || g.last != e.3 {
            group_ok = false;
        }
    }
    s.add("J01-THEME-十组官方名逐字对拍", group_ok, "");

    // THEME-3：连续守恒判据侧重算（首尾相接无缺口 + 总数 160 + 首尾钉死）。
    let mut cont_ok = GROUP_PLANS[0].first == 1441 && GROUP_PLANS[9].last == 1600;
    let mut total = 0u32;
    for i in 0..GROUP_PLANS.len() {
        let g = &GROUP_PLANS[i];
        total += g.last - g.first + 1;
        if i > 0 && g.first != GROUP_PLANS[i - 1].last + 1 {
            cont_ok = false;
        }
    }
    s.add(
        "J01-THEME-十组连续守恒重算",
        cont_ok && total == J_DOMAIN_TOTAL,
        "",
    );

    // ================= 三、三处兑现核验（J01-INTEG-*） =================

    // INTEG-1：三处核验记录齐且全相容（签名相容可执行——非注释）。
    let records = verify_integrations();
    let integ_ok = records.len() == 3
        && records.iter().all(|r| r.compatible)
        && records.iter().all(|r| !r.note.is_empty());
    s.add("J01-INTEG-三处核验齐且相容", integ_ok, "");

    // INTEG-2：兑现对字面量独立写死对拍（caller→targets 逐条）。
    let mut slot_ok = INTEGRATION_SLOTS.len() == 3;
    for (i, slot) in INTEGRATION_SLOTS.iter().enumerate() {
        let e = EXP_SLOTS[i];
        if slot.caller != e.0 || slot.targets != [e.1, e.2] {
            slot_ok = false;
        }
    }
    s.add("J01-INTEG-兑现对字面量对拍", slot_ok, "");

    // INTEG-3：核验记录指纹独立 FNV 对拍（防同源恒绿）。
    let digest = records_digest(&records);
    s.add(
        "J01-INTEG-记录指纹独立对拍",
        digest == alt_records_digest(&records) && digest != 0,
        "",
    );

    // INTEG-4：兑现任务对全部落在对应组区间（跨表即违诺——独立扫表）。
    let mut target_ok = true;
    for slot in INTEGRATION_SLOTS.iter() {
        for t in slot.targets {
            let planned = EXP_GROUPS.iter().any(|g| t >= g.2 && t <= g.3);
            if !planned {
                target_ok = false;
            }
        }
    }
    s.add("J01-INTEG-兑现对落组区间", target_ok, "");

    // ================= 四、五段架构（J01-PIPE-*） =================

    // PIPE-1：五段闭集 + 段下标单调。
    let mut pipe_ok = PipelineStage::ALL.len() == 5;
    for (i, st) in PipelineStage::ALL.iter().enumerate() {
        if st.index() != i || st.label().is_empty() {
            pipe_ok = false;
        }
    }
    s.add("J01-PIPE-五段闭集单调", pipe_ok, "");

    // PIPE-2：单向一步合法（相邻全部放行）。
    let mut fwd_ok = true;
    for i in 0..4 {
        if !stage_transition(PipelineStage::ALL[i], PipelineStage::ALL[i + 1]) {
            fwd_ok = false;
        }
    }
    s.add("J01-PIPE-相邻单向放行", fwd_ok, "");

    // PIPE-3：跳段与回退双向拒绝（采集直达下发=功耗事故通道）。
    let skip = stage_transition(PipelineStage::SensorAcquisition, PipelineStage::ActuatorDispatch);
    let back = stage_transition(PipelineStage::TelemetryReturn, PipelineStage::SensorAcquisition);
    s.add("J01-PIPE-跳段回退双向拒绝", !skip && !back, "");

    // ================= 五、域边界（J01-BOUND-*） =================

    // BOUND-1：边界两条字面量对拍。
    let bound_ok = DOMAIN_BOUNDARIES.len() == 2
        && DOMAIN_BOUNDARIES[0] == EXP_BOUNDARIES[0]
        && DOMAIN_BOUNDARIES[1] == EXP_BOUNDARIES[1];
    s.add("J01-BOUND-边界字面量对拍", bound_ok, "");

    // BOUND-2：直写拒绝/系统接口放行双向（行为不是口号）。
    s.add(
        "J01-BOUND-直写拒绝双向",
        !fan_request_allowed(FanControlPath::DirectWrite)
            && fan_request_allowed(FanControlPath::SystemInterface),
        "",
    );

    // ================= 六、采样开销（J01-PERF-*） =================

    // PERF-1：采样实耗 ≤ 预算（0.1ms=100 步，口径常量判据侧写死）。
    let (v1, cost) = sample_power(4200);
    s.add(
        "J01-PERF-采样开销预算内",
        cost <= SAMPLE_BUDGET_STEPS && SAMPLE_BUDGET_STEPS == 100 && STEPS_PER_MS == 1000,
        "",
    );

    // PERF-2：采样不耗样本——同输入重采读数稳定（测量不改被测量）。
    let (v2, _) = sample_power(4200);
    let (v3, _) = sample_power(4200);
    s.add("J01-PERF-重采读数稳定", v1 == v2 && v2 == v3 && v1 == 4200, "");

    // ================= 七、风险（J01-RISK-*） =================

    // RISK-1：三风险闭集（判据侧独立点名）。
    let risk_ok = RiskKind::ALL.len() == RISK_COUNT
        && RISK_COUNT == 3
        && RiskKind::ALL[0] == RiskKind::SensorMissing
        && RiskKind::ALL[1] == RiskKind::DriverInterfaceDiff
        && RiskKind::ALL[2] == RiskKind::ThermalModelDrift;
    s.add("J01-RISK-三风险闭集", risk_ok, "");

    // RISK-2：回退非空且逐条区分（三回退两两互异——同话术=没回退）。
    let f0 = RiskKind::ALL[0].fallback();
    let f1 = RiskKind::ALL[1].fallback();
    let f2 = RiskKind::ALL[2].fallback();
    s.add(
        "J01-RISK-回退非空且互异",
        !f0.is_empty() && !f1.is_empty() && !f2.is_empty() && f0 != f1 && f1 != f2 && f0 != f2,
        "",
    );

    // ================= 八、判据自检（J01-META-*） =================

    // META-2：判据容量无截断。
    s.add("J01-META-判据容量无截断", !s.truncated(), "");

    // META-3：码段独占——全部 0x52xx，且 != 0x50/0x51/0x39（防自判死）。
    let section_ok = ALL_CODES.iter().all(|c| (c.code() >> 8) == 0x52)
        && ALL_CODES.iter().all(|c| {
            let hi = c.code() >> 8;
            hi != 0x50 && hi != 0x51 && hi != 0x39
        });
    s.add("J01-META-诊断码段独占", section_ok, "");

    // META-4：码两两互异 + 人话原因非空。
    let mut code_ok = true;
    for i in 0..ALL_CODES.len() {
        for j in 0..ALL_CODES.len() {
            if i != j && ALL_CODES[i].code() == ALL_CODES[j].code() {
                code_ok = false;
            }
        }
    }
    for c in ALL_CODES {
        if c.reason().is_empty() {
            code_ok = false;
        }
    }
    s.add("J01-META-码互异原因非空", code_ok, "");

    // META-1：判据条数对账（放末位：此时 len 应为 21，加自身恰 22）。
    s.add("J01-META-判据条数对账", s.len() + 1 == EXPECTED_CHECKS, "");

    s
}
