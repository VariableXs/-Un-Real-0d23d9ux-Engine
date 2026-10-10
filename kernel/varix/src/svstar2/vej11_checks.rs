//! VE-F1811 · 域自检（判据逐条对应，见 `vej11_lightbudget.rs` 头注）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1811`
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **五型成本表**（五型×三档常数，单调/正值/字面锚）→ `C11-TABLE-*`
//! - **三档上限**（16/32/64 与硬件档映射，模型自洽不变量）→ `C11-CAP-*`
//! - **20% 修正门**（bps 整数口径，恰阈双向边界）→ `C11-GATE-*`
//! - **延迟位诚实**（预留不静默+指路替代）→ `C11-DEFER-*`
//! - **预算联动**（超预算告警+两支建议+强制态遥测标记，全非崩溃路径）→ `C11-BUDGET-*`
//! - **ADR 重定标流**（无 ADR 拒绝/落账/去 stale）→ `C11-REBASE-*`
//! - **加固判据**（码段独占/文本可读/溢出显性）→ `C11-SEG-*`/`C11-TEXT-*`/`C11-OVF-*`
//!
//! ## 判据侧设计纪律（为什么这些判据抓得住真缺陷）
//!
//! 1. **断数值本身，不断派生布尔**：恰阈边界用可整除的基线
//!    （est=10000/measured=12000）让 bps 恰为 2000——「超」是严格大于，
//!    实现写成 `>=` 就在这里翻红。
//! 2. **判据侧独立重算**：混合语料的逐列贡献（含面光并入聚光列的
//!    70U）按字面量手算对拍——实现把面光漏算/双算，恒等式立刻红。
//! 3. **诚实标注可机检**：定标来源必须同时含 `F1814`（数据源前向对接）
//!    与 `ADR`（回填通道）——把声明值冒充实测的实现写不出这两词。
//! 4. **模型自洽不变量**：满档并发全方向光不超本档预算——上限表与
//!    预算表若被分别改动打架，此恒等式当红（一处定义两处引用的对账面）。
//! 5. **溢出不是数字**：`u32::MAX` 规模语料必须 Err 而非回绕小数——
//!    回绕会把天文成本洗成预算内，预算门就瞎了。

use alloc::vec::Vec;

use super::vej11_lightbudget::*;
use crate::checks::CheckSet;

/// 混合语料：1 方向光 + 2 点光 + 3 聚光 + 2 面光 + 1 探针 + 1 IBL。
fn corpus_mixed() -> ClassCensus {
    ClassCensus {
        directional: 1,
        point: 2,
        spot: 3,
        area: 2,
        probe: 1,
        ibl: 1,
    }
}

pub fn run_vej11_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-J/F1811");

    // ------------------------------------------------------------------
    // C11-TABLE：五型成本表（判据一）
    // ------------------------------------------------------------------

    // 表维 5×3 且全正——零成本是谎，负值不存在于 u32 但查表下标要全通。
    {
        let mut dims_ok = true;
        let mut all_pos = true;
        for class in LightClass::ALL {
            for tier in HardwareTier::ALL {
                let c = cost(class, tier);
                dims_ok &= c > 0;
                all_pos &= c != 0;
            }
        }
        set.add(
            "C11-TABLE-01",
            dims_ok && all_pos && LightClass::ALL.len() == 5 && HardwareTier::ALL.len() == 3,
            "五型×三档全查表通路且常数全正（5×3=15 格）",
        );
    }

    // 三档单调：旗舰 ≤ 主流 ≤ 入门（同型硬件越强越便宜）。
    {
        let mono = LightClass::ALL.iter().all(|&c| {
            let e = cost(c, HardwareTier::Entry);
            let m = cost(c, HardwareTier::Mainstream);
            let f = cost(c, HardwareTier::Flagship);
            e >= m && m >= f
        });
        set.add("C11-TABLE-02", mono, "每型三档单调：entry≥mainstream≥flagship");
    }

    // 字面量锚（判据侧独立写死，不从表派生）——表值被改动即红。
    {
        let anchors = cost(LightClass::Point, HardwareTier::Mainstream) == 12
            && cost(LightClass::Ibl, HardwareTier::Flagship) == 14
            && cost(LightClass::Probe, HardwareTier::Entry) == 18;
        set.add("C11-TABLE-03", anchors, "字面锚：点光主流=12U / IBL旗舰=14U / 探针入门=18U");
    }

    // 主流档相对量级序：探针最便宜 < 方向 < 点 < 聚光 < IBL 最贵。
    {
        let m = |c| cost(c, HardwareTier::Mainstream);
        let ord = m(LightClass::Probe) < m(LightClass::Directional)
            && m(LightClass::Directional) < m(LightClass::Point)
            && m(LightClass::Point) < m(LightClass::Spot)
            && m(LightClass::Spot) < m(LightClass::Ibl);
        set.add("C11-TABLE-04", ord, "主流档量级序：probe<directional<point<spot<ibl");
    }

    // 诚实标注可机检：来源含 F1814 与 ADR、单位口径含 U、版本=v1。
    {
        let honest = CALIBRATION_SOURCE.contains("F1814")
            && CALIBRATION_SOURCE.contains("ADR")
            && !CALIBRATION_SOURCE.contains("measured")
            && COST_UNIT_LEGEND.contains("U =")
            && MODEL_VERSION == 1;
        set.add("C11-TABLE-05", honest, "定标来源声明含 F1814+ADR 且不含 measured（声明值不冒充实测）");
    }

    // ------------------------------------------------------------------
    // C11-CAP：三档上限（判据二）
    // ------------------------------------------------------------------

    set.add(
        "C11-CAP-01",
        CONCURRENCY_CAPS == [16, 32, 64] && CONCURRENCY_CAPS[0] < CONCURRENCY_CAPS[1]
            && CONCURRENCY_CAPS[1] < CONCURRENCY_CAPS[2],
        "并发上限字面 [16,32,64] 且严格递增",
    );

    {
        let lookup = concurrency_cap(HardwareTier::Entry) == 16
            && concurrency_cap(HardwareTier::Mainstream) == 32
            && concurrency_cap(HardwareTier::Flagship) == 64;
        set.add("C11-CAP-02", lookup, "档位→上限查表逐档字面对拍");
    }

    {
        let rt = HardwareTier::ALL
            .iter()
            .enumerate()
            .all(|(i, &t)| t.index() == i)
            && HardwareTier::from_index(3).is_none()
            && HardwareTier::from_index(1) == Some(HardwareTier::Mainstream)
            && HardwareTier::Entry.name() != HardwareTier::Mainstream.name()
            && HardwareTier::Mainstream.name() != HardwareTier::Flagship.name();
        set.add("C11-CAP-03", rt, "档位序/名唯一、越界 from_index 显性 None 不取近档");
    }

    // 模型自洽不变量：满档并发全方向光不超本档预算（上限表×预算表对账）。
    {
        let fits = HardwareTier::ALL.iter().all(|&t| {
            (concurrency_cap(t) as u64) * (cost(LightClass::Directional, t) as u64)
                <= tier_budget(t) as u64
        });
        set.add("C11-CAP-04", fits, "满档并发方向光成本 ≤ 本档预算（三档全过，上限/预算不打架）");
    }

    // ------------------------------------------------------------------
    // C11-GATE：20% 修正门（判据三）
    // ------------------------------------------------------------------

    {
        let v = deviation_gate(10_000, 10_000);
        set.add(
            "C11-GATE-01",
            matches!(v, Ok(DeviationVerdict::WithinGate { deviation_bps: 0 })),
            "零偏差 → 门内且 bps=0",
        );
    }

    // 恰 20% 不触发（「超」是严格大于）——实现写成 >= 在此翻红。
    {
        let v = deviation_gate(10_000, 12_000);
        set.add(
            "C11-GATE-02",
            matches!(v, Ok(DeviationVerdict::WithinGate { deviation_bps: 2000 })),
            "恰 2000bps=20% → 门内（严格大于口径的恰阈边界）",
        );
    }

    {
        let v = deviation_gate(10_000, 12_001);
        set.add(
            "C11-GATE-03",
            matches!(v, Ok(DeviationVerdict::RevisionCase { deviation_bps: 2001 })),
            "2001bps → 修正立项且 bps 值独立重算对拍",
        );
    }

    // 负向偏差对称计（实测低于预估同样进门控）。
    {
        let v = deviation_gate(10_000, 7_999);
        set.add(
            "C11-GATE-04",
            matches!(v, Ok(DeviationVerdict::RevisionCase { deviation_bps: 2001 })),
            "实测低于预估 2001bps 同样立项（|差| 对称）",
        );
    }

    {
        let v = deviation_gate(0, 0);
        set.add(
            "C11-GATE-05",
            matches!(v, Err(ref e) if e.code == BudgetCode::ESTIMATE_OVERFLOW && !e.message.is_empty()),
            "零预估场景显性拒绝对账（不除零 panic、不冒充通过）",
        );
    }

    set.add(
        "C11-GATE-06",
        DEVIATION_GATE_BPS == 2000,
        "门阈值常量字面 2000bps（20% 口径单源）",
    );

    // ------------------------------------------------------------------
    // C11-DEFER：延迟位诚实（判据四）
    // ------------------------------------------------------------------

    {
        let r = DeferredExtension::query();
        set.add(
            "C11-DEFER-01",
            matches!(r, Err(ref e) if e.code == BudgetCode::DEFERRED_RESERVED_ACCESS),
            "延迟渲染查询恒显性报错（预留不静默）",
        );
    }

    {
        let msg = match DeferredExtension::query() {
            Err(e) => e.message,
            Ok(_) => alloc::string::String::new(),
        };
        set.add(
            "C11-DEFER-02",
            DeferredExtension::STATE == "RESERVED"
                && msg.contains("forward")
                && msg.contains("not implemented"),
            "预留状态=RESERVED 且错误文本指路 forward 替代（三要素齐）",
        );
    }

    // ------------------------------------------------------------------
    // C11-BUDGET：预算联动（告警+两支建议+强制态标记）
    // ------------------------------------------------------------------

    // 基线：10 点光 @主流 1MPix = 10×12=120U ≤ 400U，全绿零诊断零建议。
    {
        let c = ClassCensus { point: 10, ..ClassCensus::default() };
        let e = estimate(c, 1, HardwareTier::Mainstream, false).unwrap();
        set.add(
            "C11-BUDGET-01",
            e.total_u == 120 && e.budget_u == 400 && !e.over_budget
                && e.codes.is_empty() && e.suggestions.is_empty() && !e.over_cap,
            "预算内基线：120U/400U、零诊断零建议（不过敏）",
        );
    }

    // 恰预算边界：20 方向光 @入门 = 480U == 480U → 不超（严格大于）；
    // 但 20 > 上限 16 → 并发超限告警 + 建议仍要发（渲染约束独立于预算）。
    {
        let c = ClassCensus { directional: 20, ..ClassCensus::default() };
        let e = estimate(c, 1, HardwareTier::Entry, false).unwrap();
        let codes: Vec<u16> = e.codes.iter().map(|c| c.0).collect();
        set.add(
            "C11-BUDGET-02",
            e.total_u == 480 && e.budget_u == 480 && !e.over_budget && e.over_by_u == 0
                && e.over_cap && codes.contains(&BudgetCode::CAP_EXCEEDED.0)
                && codes.contains(&BudgetCode::SUGGESTION_ISSUED.0)
                && !codes.contains(&BudgetCode::OVER_BUDGET.0),
            "恰预算不超 + 并发超限独立告警（预算门与上限门是两件事）",
        );
    }

    // 超预算：21 方向光 @入门 = 504U > 480U，超出 24U；建议=关最贵（方向光贡献 504）。
    {
        let c = ClassCensus { directional: 21, ..ClassCensus::default() };
        let e = estimate(c, 1, HardwareTier::Entry, false).unwrap();
        let drop = e.suggestions.iter().any(|s| match s {
            Suggestion::DropCostliest { class, contribution_u, text } => {
                *class == LightClass::Directional && *contribution_u == 504
                    && text.contains("directional") && !text.is_empty()
            }
            _ => false,
        });
        set.add(
            "C11-BUDGET-03",
            e.over_budget && e.over_by_u == 24 && drop,
            "超预算 24U + 关最贵建议带贡献额与人话文本",
        );
    }

    // 强制态遥测标记：超预算+forced 才有 0x3902；健康运行 forced 不制造噪声。
    {
        let c_over = ClassCensus { directional: 21, ..ClassCensus::default() };
        let e_f = estimate(c_over, 1, HardwareTier::Entry, true).unwrap();
        let c_ok = ClassCensus { point: 10, ..ClassCensus::default() };
        let e_ok = estimate(c_ok, 1, HardwareTier::Mainstream, true).unwrap();
        let has_forced = e_f.codes.contains(&BudgetCode::OVERRUN_FORCED);
        set.add(
            "C11-BUDGET-04",
            has_forced && !e_ok.codes.iter().any(|c| *c == BudgetCode::OVERRUN_FORCED),
            "强制态标记只在超预算时入账（0x3902 遥测对接 F1819）",
        );
    }

    // 混合语料逐列贡献字面手算对拍（含面光并入聚光列：5×14=70）。
    {
        let e = estimate(corpus_mixed(), 1, HardwareTier::Mainstream, false).unwrap();
        let sum = e.per_class_u.iter().fold(0u64, |a, &x| a + x);
        set.add(
            "C11-BUDGET-05",
            e.per_class_u == [10, 24, 70, 6, 24] && sum == e.total_u && e.total_u == 134
                && e.active_lights == 10,
            "混合语料逐列 10/24/70/6/24 合计 134U（面光并入聚光列独立手算）",
        );
    }

    // 像素规模线性：2 MPix 加倍（模型粒度=万像素的诚实声明可机检）。
    {
        let c = corpus_mixed();
        let e1 = estimate(c, 1, HardwareTier::Mainstream, false).unwrap();
        let e2 = estimate(c, 2, HardwareTier::Mainstream, false).unwrap();
        set.add(
            "C11-BUDGET-06",
            e2.total_u == e1.total_u * 2 && e2.budget_u == e1.budget_u * 2,
            "2MPix 预估与预算同倍放大（线性模型）",
        );
    }

    // IBL 0/1 语义：无 IBL 时 ibl 列恰为 0。
    {
        let mut c = corpus_mixed();
        c.ibl = 0;
        let e = estimate(c, 1, HardwareTier::Mainstream, false).unwrap();
        set.add(
            "C11-BUDGET-07",
            e.per_class_u[LightClass::Ibl.index()] == 0 && e.total_u == 110,
            "IBL 缺席列贡献恰 0（合计 134-24=110）",
        );
    }

    // ------------------------------------------------------------------
    // C11-REBASE：ADR 重定标流
    // ------------------------------------------------------------------

    {
        let mut r = ModelRegistry::new();
        let r1 = r.rebaseline("", "x");
        let r2 = r.rebaseline("   ", "x");
        set.add(
            "C11-REBASE-01",
            r1.is_err() && r2.is_err() && r.version == MODEL_VERSION && r.adr_chain.is_empty(),
            "空/空白 ADR 编号拒绝（版本不动、链不入）",
        );
    }

    {
        let mut r = ModelRegistry::new();
        r.mark_stale();
        let ok = r.rebaseline("VE-ADR-0042", "recalibrate from F1814 ladder");
        set.add(
            "C11-REBASE-02",
            ok.is_ok() && r.version == MODEL_VERSION + 1 && r.adr_chain.len() == 1
                && r.adr_chain[0].contains("VE-ADR-0042") && r.adr_chain[0].contains("F1814"),
            "合法重定标：版本+1、ADR 链入账含编号与摘要",
        );
    }

    {
        let mut r = ModelRegistry::new();
        r.mark_stale();
        let _ = r.rebaseline("VE-ADR-0043", "s");
        set.add(
            "C11-REBASE-03",
            !r.stale,
            "重定标落账同时清除 stale 标记（待重定标态有出口）",
        );
    }

    // ------------------------------------------------------------------
    // C11-OVF / C11-SEG / C11-TEXT：加固判据
    // ------------------------------------------------------------------

    // 溢出显性拒绝：u32::MAX 规模不回绕不 panic。
    {
        let c = ClassCensus { point: u32::MAX, ..ClassCensus::default() };
        let r = estimate(c, u32::MAX, HardwareTier::Flagship, false);
        set.add(
            "C11-OVF-01",
            matches!(r, Err(ref e) if e.code == BudgetCode::ESTIMATE_OVERFLOW && !e.message.is_empty()),
            "天文规模预估算术溢出显性拒绝（checked 面，不回绕洗白）",
        );
    }

    // 码段独占：10 码全 0x39xx、互异、与邻居段字面不等、解释全非空。
    {
        let all = [
            BudgetCode::OVER_BUDGET,
            BudgetCode::OVERRUN_FORCED,
            BudgetCode::DEVIATION_GATE,
            BudgetCode::REBASELINE_REJECTED,
            BudgetCode::DEFERRED_RESERVED_ACCESS,
            BudgetCode::CAP_EXCEEDED,
            BudgetCode::ESTIMATE_OVERFLOW,
            BudgetCode::MODEL_STALE,
            BudgetCode::SUGGESTION_ISSUED,
            BudgetCode::REBASELINED,
        ];
        let seg_ok = all.iter().all(|c| (c.0 & 0xFF00) == BudgetCode::SEGMENT << 8);
        let distinct = {
            let mut ok = true;
            for i in 0..all.len() {
                for j in (i + 1)..all.len() {
                    ok &= all[i].0 != all[j].0;
                }
            }
            ok
        };
        let explains = all.iter().all(|c| !c.explain().is_empty());
        set.add(
            "C11-SEG-01",
            seg_ok && distinct && explains && all.len() == 10,
            "10 码全 0x39xx 段互异且人话解释齐备",
        );
    }

    {
        set.add(
            "C11-SEG-02",
            BudgetCode::SEGMENT == 0x39 && 0x39 != 0x31 && 0x39 != 0x32 && 0x39 != 0x34,
            "0x39 与邻居 0x31(F1810)/0x32/0x34(F0224) 字面不等（判据侧写死）",
        );
    }

    // 建议文本可读性：非空且含动作词（无障碍：告警文本规范可读）。
    {
        let c = ClassCensus { directional: 21, ..ClassCensus::default() };
        let e = estimate(c, 1, HardwareTier::Entry, false).unwrap();
        let readable = e.suggestions.iter().all(|s| {
            let t = s.text();
            !t.is_empty() && (t.contains("drop") || t.contains("lower") || t.contains("cull"))
        });
        set.add(
            "C11-TEXT-01",
            readable && !e.suggestions.is_empty(),
            "建议文本非空且含动作词（drop/lower/cull）",
        );
    }

    // 门禁对接面与账本语义：gate_check 是 deviation_gate 的 CI 别名（同口径单源）。
    {
        let a = gate_check(10_000, 12_001);
        let b = deviation_gate(10_000, 12_001);
        set.add(
            "C11-HARDEN-01",
            a == b && matches!(a, Ok(DeviationVerdict::RevisionCase { .. })),
            "gate_check 与 deviation_gate 同口径（F1767 对接不另立标准）",
        );
    }

    // 预算表字面锚：[480,400,448]（满档方向光不变量的数值基座）。
    {
        set.add(
            "C11-HARDEN-02",
            BUDGET_PER_MPIX == [480, 400, 448] && tier_budget(HardwareTier::Mainstream) == 400,
            "预算表字面锚 [480,400,448] 逐格",
        );
    }

    set
}
