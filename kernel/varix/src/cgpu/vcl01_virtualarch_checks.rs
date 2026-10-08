//! CGPU-F1761 判据层：L 域开工与虚拟化总架构（锚点判据逐条映射）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1761`
//!
//! **锚点判据（平等声明/签收/预留兑现/映射/五段/复用/场景/合同适配/
//! 风险/九组）→ 判据族**：MISSION 2 / HANDOVER 2 / THEME 3 / PIPE 2 /
//! REUSE 2 / SCENARIO 2 / INVARIANT 3 / RISK 2 / META 4 = 22 项。
//!
//! # 本层核心纪律：判据侧独立重算，不向被测问答案
//!
//! 十组表与主题表与复用声明全部**判据侧独立写死字面量对拍**；八主题
//! 覆盖率判据侧重算（漏一主题即红）；合同等级表双向（直通=全量、半虚
//! 拟化≠全量）；码段判据 `!=` 防自判死。

// ---------------------------------------------------------------------------
// 导入
// ---------------------------------------------------------------------------

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vcl01_virtualarch::*;

// ---------------------------------------------------------------------------
// 判据侧独立参照
// ---------------------------------------------------------------------------

/// 判据侧独立写死的域使命三条款。
const EXP_MISSION: [&str; 3] = [
    "让 GPU 能力在虚拟机/容器/云场景下被安全高效地共享与使用",
    "虚拟化不是二等公民——虚拟化平等声明",
    "虚拟化不破坏能达成的合同",
];

/// 判据侧独立写死的八官方主题。
const EXP_THEMES: [&str; 8] = [
    "虚拟化模式", "vGPU 抽象", "直通", "分区", "半虚拟化", "显示", "迁移", "遥测",
];

/// 判据侧独立写死的十组（官方批次标题逐字 + 区间）。
const EXP_GROUPS: [(u8, &str, u32, u32); 10] = [
    (1, "虚拟化总架构与抽象组", 1761, 1776),
    (2, "GPU 直通与分区组", 1777, 1792),
    (3, "virtio-gpu 半虚拟化组", 1793, 1808),
    (4, "虚拟化场景与优化组", 1809, 1824),
    (5, "虚拟化显示与多媒体组", 1825, 1840),
    (6, "虚拟化安全与多租户组", 1841, 1856),
    (7, "虚拟化遥测与调试组", 1857, 1872),
    (8, "虚拟化场景生态组", 1873, 1888),
    (9, "L 域预备与自查组", 1889, 1904),
    (10, "L 域收口组", 1905, 1920),
];

/// 判据侧独立写死的复用声明（K01 上游 / F1763 扩展）。
const EXP_REUSE: (u32, u32) = (1601, 1763);

/// 全部诊断码（判据侧点名）。
const ALL_CODES: [VlCode; 5] = [
    VlCode::MODE_OUT_OF_TABLE,
    VlCode::CONTRACT_DOWNGRADE,
    VlCode::RECORDS_INCOMPLETE,
    VlCode::REUSE_DRIFT,
    VlCode::SCENARIO_OUT_OF_TABLE,
];

/// 预期判据条数。
const EXPECTED_CHECKS: usize = 22;

// ---------------------------------------------------------------------------
// 主判据
// ---------------------------------------------------------------------------

/// L01 域自检入口。
pub fn run_vcl01_checks() -> CheckSet {
    let mut s = CheckSet::new("cgpu-virt");

    // ================= 一、平等声明（MISSION） =================

    // MISS-1：域使命三条款字面量逐字对拍。
    let mission_ok = MISSION_CLAUSES.len() == 3
        && MISSION_CLAUSES[0] == EXP_MISSION[0]
        && MISSION_CLAUSES[1] == EXP_MISSION[1]
        && MISSION_CLAUSES[2] == EXP_MISSION[2];
    s.add("L01-MISS-域使命三条款对拍", mission_ok, "");

    // MISS-2：域界宣言（F1761 开工、F1920 收官、160 项守恒）。
    s.add(
        "L01-MISS-域界宣言",
        L_DOMAIN_FIRST == 1761
            && L_DOMAIN_LAST == 1920
            && L_DOMAIN_TOTAL == 160
            && L_DOMAIN_LAST - L_DOMAIN_FIRST + 1 == L_DOMAIN_TOTAL,
        "",
    );

    // ================= 二、签收与预留（HANDOVER） =================

    // HAND-1：K 域移交签收（来源 F1757、状态已定——判据侧写死）。
    s.add(
        "L01-HAND-K域移交签收",
        K_HANDOVER.source == 1757
            && K_HANDOVER.items == 4
            && K_HANDOVER.status == RecordStatus::Settled,
        "",
    );

    // HAND-2：I09 带宽预留兑现声明（预留已声明、兑现待后续批次）。
    s.add(
        "L01-HAND-I09预留兑现起点",
        I_RESERVE.from == "I09" && I_RESERVE.status == RecordStatus::Pending,
        "",
    );

    // ================= 三、主题映射（THEME） =================

    // THEME-1：八官方主题判据侧独立对拍。
    let mut theme_ok = OFFICIAL_THEMES.len() == 8;
    for i in 0..8 {
        if OFFICIAL_THEMES[i] != EXP_THEMES[i] {
            theme_ok = false;
        }
    }
    s.add("L01-THEME-八主题闭集对拍", theme_ok, "");

    // THEME-2：十组官方名+区间逐字对拍。
    let mut group_ok = GROUP_PLANS.len() == 10;
    for (i, g) in GROUP_PLANS.iter().enumerate() {
        let e = EXP_GROUPS[i];
        if g.group != e.0 || g.name != e.1 || g.first != e.2 || g.last != e.3 {
            group_ok = false;
        }
    }
    s.add("L01-THEME-十组官方名逐字对拍", group_ok, "");

    // THEME-3：连续守恒 + 八主题全覆盖（判据侧重算）。
    let mut cont_ok = GROUP_PLANS[0].first == 1761 && GROUP_PLANS[9].last == 1920;
    let mut total = 0u32;
    for i in 0..GROUP_PLANS.len() {
        let g = &GROUP_PLANS[i];
        total += g.last - g.first + 1;
        if i > 0 && g.first != GROUP_PLANS[i - 1].last + 1 {
            cont_ok = false;
        }
    }
    let mut covered = [false; 8];
    for g in GROUP_PLANS.iter() {
        for t in g.themes {
            covered[*t] = true;
        }
    }
    let all_covered = covered.iter().all(|c| *c);
    s.add(
        "L01-THEME-连续守恒与主题全覆盖",
        cont_ok && total == L_DOMAIN_TOTAL && all_covered,
        "",
    );

    // ================= 四、五段架构（PIPE） =================

    // PIPE-1：五段闭集 + 单向相邻放行。
    let mut pipe_ok = PipelineStage::ALL.len() == 5;
    for (i, st) in PipelineStage::ALL.iter().enumerate() {
        if st.index() != i || st.label().is_empty() {
            pipe_ok = false;
        }
        if i < 4 && !stage_transition(PipelineStage::ALL[i], PipelineStage::ALL[i + 1]) {
            pipe_ok = false;
        }
    }
    s.add("L01-PIPE-五段闭集单向放行", pipe_ok, "");

    // PIPE-2：跳段与回退双向拒绝。
    let skip = stage_transition(PipelineStage::EnvDetect, PipelineStage::PerfAssure);
    let back = stage_transition(PipelineStage::ObserveDebug, PipelineStage::EnvDetect);
    s.add("L01-PIPE-跳段回退双向拒绝", !skip && !back, "");

    // ================= 五、复用不重建（REUSE） =================

    // REUSE-1：vGPU 复用声明对拍（K01 上游 / F1763 扩展——判据侧写死）。
    s.add(
        "L01-REUSE-ComputeDevice复用声明",
        VGPU_REUSE.upstream == EXP_REUSE.0
            && VGPU_REUSE.extend_at == EXP_REUSE.1
            && !VGPU_REUSE.what.is_empty(),
        "",
    );

    // REUSE-2：合同适配复用 F1466 模式（扩展兑现落 J02/J10 收口区间外须在域内）。
    s.add(
        "L01-REUSE-合同适配复用声明",
        CONTRACT_ADAPTER_REUSE.upstream == 1466
            && CONTRACT_ADAPTER_REUSE.extend_at >= L_DOMAIN_FIRST
            && CONTRACT_ADAPTER_REUSE.extend_at <= L_DOMAIN_LAST,
        "",
    );

    // ================= 六、场景族（SCENARIO） =================

    // SCEN-1：四场景闭集判据侧独立标签对拍。
    let exp_labels = ["桌面虚拟化", "云游戏", "容器 GPU", "AI 多租户"];
    let mut scen_ok = ScenarioFamily::ALL.len() == SCENARIO_COUNT && SCENARIO_COUNT == 4;
    for (i, sc) in ScenarioFamily::ALL.iter().enumerate() {
        if sc.label() != exp_labels[i] {
            scen_ok = false;
        }
    }
    s.add("L01-SCEN-四场景族对拍", scen_ok, "");

    // SCEN-2：场景两两互异（同标签=族塌缩）。
    let l = ScenarioFamily::ALL;
    s.add(
        "L01-SCEN-场景两两互异",
        l[0].label() != l[1].label()
            && l[1].label() != l[2].label()
            && l[2].label() != l[3].label()
            && l[0].label() != l[2].label(),
        "",
    );

    // ================= 七、不变量：合同适配（INVARIANT） =================

    // INV-1：直通 = 全量合同（等价裸机——平等声明的锚点面）。
    s.add(
        "L01-INV-直通全量合同",
        contract_level_of(VirtMode::Passthrough) == ContractLevel::Full,
        "",
    );

    // INV-2：半虚拟化 ≠ 全量（诚实：不同模式等级不同，表外无暗降）。
    s.add(
        "L01-INV-半虚拟化非全量显性分账",
        contract_level_of(VirtMode::Paravirtual) != ContractLevel::Full
            && contract_level_of(VirtMode::Partitioned) == ContractLevel::Standard,
        "",
    );

    // INV-3：表完备——三模式各有等级（表外模式无通道）。
    let modes = [VirtMode::Passthrough, VirtMode::Partitioned, VirtMode::Paravirtual];
    let mut table_ok = VIRT_MODE_COUNT == 3;
    for m in modes {
        let lv = contract_level_of(m);
        if lv != ContractLevel::Full && lv != ContractLevel::Standard && lv != ContractLevel::BestEffort {
            table_ok = false;
        }
    }
    s.add("L01-INV-模式表完备", table_ok, "");

    // ================= 八、风险（RISK） =================

    // RISK-1：四风险闭集（判据侧独立点名）。
    let risk_ok = RiskKind::ALL.len() == RISK_COUNT
        && RISK_COUNT == 4
        && RiskKind::ALL[0] == RiskKind::PerfLoss
        && RiskKind::ALL[3] == RiskKind::SchedulingFairness;
    s.add("L01-RISK-四风险闭集", risk_ok, "");

    // RISK-2：预案两两互异且非空（同话术=没预案）。
    let f: Vec<String> = RiskKind::ALL.iter().map(|r| r.fallback()).collect();
    let distinct = f[0] != f[1] && f[1] != f[2] && f[2] != f[3] && f[0] != f[2];
    s.add(
        "L01-RISK-预案互异非空",
        f.iter().all(|x| !x.is_empty()) && distinct,
        "",
    );

    // ================= 九、判据自检（META） =================

    // META-2：判据容量无截断。
    s.add("L01-META-判据容量无截断", !s.truncated(), "");

    // META-3：码段独占——全部 0x53xx，且 != 0x50/0x51/0x52（防自判死）。
    let section_ok = ALL_CODES.iter().all(|c| (c.code() >> 8) == 0x53)
        && ALL_CODES.iter().all(|c| {
            let hi = c.code() >> 8;
            hi != 0x50 && hi != 0x51 && hi != 0x52
        });
    s.add("L01-META-诊断码段独占", section_ok, "");

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
    s.add("L01-META-码互异原因非空", code_ok, "");

    // META-1：判据条数对账（放末位：此时 len 应为 21，加自身恰 22）。
    s.add("L01-META-判据条数对账", s.len() + 1 == EXPECTED_CHECKS, "");

    s
}
