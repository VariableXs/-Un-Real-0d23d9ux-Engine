//! VE-F2210 自检 · 粒子性能预算（VE-L 域）
//!
//! **锚点判据逐条对应**（`#VE-F2210`「三因子模型、公式公开、20% 修正门、
//! 次序对齐、判据」）：
//!
//! | 锚点判据 | 自检组 |
//! |---|---|
//! | 三因子模型 | `L10-因子-*`（可分解性：变异任一因子只动对应份，独立重算对账） |
//! | 公式公开 | `L10-公式-*`（文档非空含关键词；常数表相对关系） |
//! | 20% 修正门 | `L10-修正门-*`（双向：199 放行 / 200 立案；两侧同阈；含端点） |
//! | 次序对齐 | `L10-次序-*`（先档后率单源；跳步立案+遥测；游标不前移） |
//! | 三方裁决/表过期/判据 | `L10-裁决-*` / `L10-表-*` / `L10-判据-*` |
//!
//! **判据设计硬规矩**（承 vel08/vel09 先例）：
//! ① 期望值**判据侧独立重算**，不从被测函数反推；
//! ② 不变量类判据两头都测（违规被拒 + 合规放行）；
//! ③ 阈值钉死具体数值（200‰ 含端点，恰 200 立案）；
//! ④ 判据区零 panic 面：取值一律 match/`?`记红，不用 unwrap/expect。

use super::vel10_budget::*;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 语料（判据侧自持）
// ---------------------------------------------------------------------------

/// 判据侧独立重算：模拟成本（与被测公式同口径、写法独立）。
fn expect_sim(count: u64, attrs: u32, tier: Tier, t: &CostTable) -> u64 {
    count * attrs as u64 * t.sim_ns_per_particle_per_attr[tier.ordinal()] as u64
}

/// 判据侧独立重算：渲染成本。
fn expect_render(count: u64, form: FormFactor, tier: Tier, t: &CostTable) -> u64 {
    count * t.render_ns_per_particle[form.ordinal()][tier.ordinal()] as u64
}

/// 判据侧独立重算：排序成本。
fn expect_sort(count: u64, sort_on: bool, tier: Tier, t: &CostTable) -> u64 {
    if !sort_on {
        return 0;
    }
    let mut lg: u64 = 0;
    let mut v = count;
    while v > 1 {
        v >>= 1;
        lg += 1;
    }
    count * lg * t.sort_coef_ns_per_logn[tier.ordinal()] as u64
}

// ---------------------------------------------------------------------------
// 组一：三因子模型（可分解性 + 独立重算对账）
// ---------------------------------------------------------------------------

fn chk_factors(s: &mut CheckSet, t: &CostTable) {
    // L10-因子-01：模拟随 count 线性（翻倍），独立重算对账（Low 档，attrs=3）。
    let e1 = estimate(1000, 3, FormFactor::Billboard, false, Tier::Low, t);
    let e2 = estimate(2000, 3, FormFactor::Billboard, false, Tier::Low, t);
    let ok = match (e1, e2) {
        (Ok(a), Ok(b)) => {
            a.sim_ns == expect_sim(1000, 3, Tier::Low, t)
                && b.sim_ns == expect_sim(2000, 3, Tier::Low, t)
                && b.sim_ns == a.sim_ns * 2
        }
        _ => false,
    };
    s.add("L10-因子-01", ok, "模拟∝数量：count 翻倍 sim 翻倍且与独立重算一致");

    // L10-因子-02：模拟随属性数线性（翻倍）。
    let ok = match (e1, estimate(1000, 6, FormFactor::Billboard, false, Tier::Low, t)) {
        (Ok(a), Ok(b)) => b.sim_ns == a.sim_ns * 2,
        _ => false,
    };
    s.add("L10-因子-02", ok, "模拟∝属性数：attrs 翻倍 sim 翻倍");

    // L10-因子-03：渲染随形态递增（billboard<mesh<trail，三档各查）。
    let mut ok = true;
    for tier in Tier::all() {
        let bb = estimate(500, 1, FormFactor::Billboard, false, tier, t);
        let me = estimate(500, 1, FormFactor::Mesh, false, tier, t);
        let tr = estimate(500, 1, FormFactor::Trail, false, tier, t);
        if let (Ok(a), Ok(b), Ok(c)) = (bb, me, tr) {
            ok = ok && a.render_ns < b.render_ns && b.render_ns < c.render_ns;
        } else {
            ok = false;
        }
    }
    s.add("L10-因子-03", ok, "渲染∝形态复杂度：三档下 billboard<mesh<trail");

    // L10-因子-04：渲染随 count 线性（翻倍），独立重算对账（trail/High）。
    let ok = match (
        estimate(1000, 2, FormFactor::Trail, false, Tier::High, t),
        estimate(2000, 2, FormFactor::Trail, false, Tier::High, t),
    ) {
        (Ok(a), Ok(b)) => {
            b.render_ns == a.render_ns * 2
                && a.render_ns == expect_render(1000, FormFactor::Trail, Tier::High, t)
        }
        _ => false,
    };
    s.add("L10-因子-04", ok, "渲染∝数量：count 翻倍 render 翻倍且与独立重算一致");

    // L10-因子-05：排序开关关闭恒 0。
    let ok = match estimate(4096, 4, FormFactor::Mesh, false, Tier::Mid, t) {
        Ok(e) => e.sort_ns == 0,
        _ => false,
    };
    s.add("L10-因子-05", ok, "排序∝开关：关闭恒 0");

    // L10-因子-06：排序开启 = N·log2(N)·系数，独立重算对账（4096 → lg=12）。
    let ok = match estimate(4096, 4, FormFactor::Mesh, true, Tier::Low, t) {
        Ok(e) => e.sort_ns == expect_sort(4096, true, Tier::Low, t) && e.sort_ns == 4096 * 12 * 2,
        _ => false,
    };
    s.add("L10-因子-06", ok, "排序∝N logN：4096×lg12×系数2=98304 与独立重算一致");

    // L10-因子-07：ilog2 整数边界（0→0,1→0,2→1,3→1,4→2,5→2）。
    let ok = ilog2_u64(0) == 0
        && ilog2_u64(1) == 0
        && ilog2_u64(2) == 1
        && ilog2_u64(3) == 1
        && ilog2_u64(4) == 2
        && ilog2_u64(5) == 2;
    s.add("L10-因子-07", ok, "ilog2_u64 边界六点独立对账");

    // L10-因子-08：total = sim+render+sort 恒等（含 sort 开）。
    let ok = match estimate(2048, 3, FormFactor::Trail, true, Tier::Mid, t) {
        Ok(e) => e.total_ns == e.sim_ns + e.render_ns + e.sort_ns,
        _ => false,
    };
    s.add("L10-因子-08", ok, "总成本=三份之和守恒");

    // L10-因子-09：count=0 合法且三份全 0（空发射器）。
    let ok = match estimate(0, 3, FormFactor::Billboard, true, Tier::Low, t) {
        Ok(e) => e.total_ns == 0 && e.sim_ns == 0 && e.render_ns == 0 && e.sort_ns == 0,
        _ => false,
    };
    s.add("L10-因子-09", ok, "空发射器预估恰为 0（合法放行侧）");

    // L10-因子-10：attrs=0 拒绝（退化输入，不钳制）。
    let ok = matches!(
        estimate(100, 0, FormFactor::Billboard, false, Tier::Low, t),
        Err(DiagCode::DEGENERATE_INPUT)
    );
    s.add("L10-因子-10", ok, "attrs=0 拒绝 DEGENERATE_INPUT");

    // L10-因子-11：溢出拒绝（巨大 count 触发 checked 失败）。
    let big = u64::MAX;
    let ok = matches!(
        estimate(big, 2, FormFactor::Trail, true, Tier::Low, t),
        Err(DiagCode::DEGENERATE_INPUT)
    );
    s.add("L10-因子-11", ok, "算术溢出拒绝 DEGENERATE_INPUT（不静默饱和）");

    // L10-因子-12：tier 变异按表（三档 sim 独立重算各自成立）。
    let mut ok = true;
    for tier in Tier::all() {
        let e = estimate(1000, 3, FormFactor::Billboard, false, tier, t);
        if let Ok(e) = e {
            ok = ok && e.sim_ns == expect_sim(1000, 3, tier, t);
        } else {
            ok = false;
        }
    }
    s.add("L10-因子-12", ok, "三档常数各按表生效（档位错位即红）");

    // L10-因子-13：归因分解——count 翻倍时 sim/render 精确×2，sort 按 nlogn 精确重算。
    let ok = match (
        estimate(1000, 3, FormFactor::Billboard, true, Tier::Low, t),
        estimate(2000, 3, FormFactor::Billboard, true, Tier::Low, t),
    ) {
        (Ok(a), Ok(b)) => {
            b.sim_ns == a.sim_ns * 2
                && b.render_ns == a.render_ns * 2
                && b.sort_ns == expect_sort(2000, true, Tier::Low, t)
        }
        _ => false,
    };
    s.add("L10-因子-13", ok, "三因子可独立归因：各份按各自公式精确变化");

    // L10-因子-14：枚举守卫——from_ordinal 越界 None（不留默认兜底）。
    let ok = Tier::from_ordinal(3).is_none()
        && FormFactor::from_ordinal(3).is_none()
        && Tier::from_ordinal(0).is_some()
        && FormFactor::from_ordinal(2).is_some();
    s.add("L10-因子-14", ok, "档位/形态枚举守卫双向（越界拒+界内收）");
}

// ---------------------------------------------------------------------------
// 组二：公式公开与常数表
// ---------------------------------------------------------------------------

fn chk_formula(s: &mut CheckSet, t: &CostTable) {
    // L10-公式-01：文档非空。
    s.add("L10-公式-01", !FORMULA_DOC.is_empty(), "公式公开文档非空");

    // L10-公式-02：文档含三因子关键词。
    s.add(
        "L10-公式-02",
        FORMULA_DOC.contains("模拟") && FORMULA_DOC.contains("渲染") && FORMULA_DOC.contains("排序"),
        "公式文档含三因子关键词（模拟/渲染/排序）",
    );

    // L10-公式-03：文档含修正门声明。
    s.add(
        "L10-公式-03",
        FORMULA_DOC.contains("20%") && FORMULA_DOC.contains("F2212"),
        "公式文档含 20% 修正门与 F2212 回填源声明",
    );

    // L10-公式-04：同档形态相对关系 trail>mesh>billboard（常数表直查，三档）。
    let mut ok = true;
    for tier in Tier::all() {
        let i = tier.ordinal();
        ok = ok
            && t.render_ns_per_particle[2][i] > t.render_ns_per_particle[1][i]
            && t.render_ns_per_particle[1][i] > t.render_ns_per_particle[0][i];
    }
    s.add("L10-公式-04", ok, "常数表相对关系：拖尾>网格>面片（三档）");

    // L10-公式-05：同形态档位不倒挂 low>=mid>=high（模拟+渲染+排序全查）。
    let mut ok = true;
    for form in FormFactor::all() {
        let f = form.ordinal();
        ok = ok
            && t.render_ns_per_particle[f][0] >= t.render_ns_per_particle[f][1]
            && t.render_ns_per_particle[f][1] >= t.render_ns_per_particle[f][2];
    }
    ok = ok
        && t.sim_ns_per_particle_per_attr[0] >= t.sim_ns_per_particle_per_attr[1]
        && t.sim_ns_per_particle_per_attr[1] >= t.sim_ns_per_particle_per_attr[2]
        && t.sort_coef_ns_per_logn[0] >= t.sort_coef_ns_per_logn[1]
        && t.sort_coef_ns_per_logn[1] >= t.sort_coef_ns_per_logn[2];
    s.add("L10-公式-05", ok, "档位不倒挂：低档成本≥中档≥高档（三因子全查）");

    // L10-公式-06：版本号非空且前缀 L10。
    s.add(
        "L10-公式-06",
        BUDGET_VERSION.starts_with("L10") && !BUDGET_VERSION.is_empty(),
        "版本号格式 L10-*（跨版本对账锚）",
    );

    // L10-公式-07：出厂表非脏、时戳 0。
    s.add(
        "L10-公式-07",
        !t.stale && t.calibrated_at == 0,
        "calibrated() 出厂态：非脏、时戳 0",
    );

    // L10-公式-08：档名 low/mid/high（与 F2014 家族对齐，跨模块对账靠名）。
    s.add(
        "L10-公式-08",
        Tier::Low.label() == "low" && Tier::Mid.label() == "mid" && Tier::High.label() == "high",
        "档名与 F2014 家族逐字对齐",
    );

    // L10-公式-09：形态名 billboard/mesh/trail（与 F2206 RenderForm 对齐）。
    s.add(
        "L10-公式-09",
        FormFactor::Billboard.label() == "billboard"
            && FormFactor::Mesh.label() == "mesh"
            && FormFactor::Trail.label() == "trail",
        "形态名与 F2206 渲染侧逐字对齐",
    );

    // L10-公式-10：闭集长度恒 3。
    s.add(
        "L10-公式-10",
        TIER_COUNT == 3 && FORM_COUNT == 3 && Tier::all().len() == 3 && FormFactor::all().len() == 3,
        "三档三形态闭集长度（规格承诺，加变体必红）",
    );
}

// ---------------------------------------------------------------------------
// 组三：20% 修正门（两侧同阈、含端点）
// ---------------------------------------------------------------------------

fn chk_gate(s: &mut CheckSet) {
    // L10-修正门-01：阈值为 200‰（判据侧钉死，不从被测反推）。
    s.add(
        "L10-修正门-01",
        CORRECTION_GATE_PERMILLE == 200,
        "修正门阈值恰为 200‰（20%）",
    );

    // L10-修正门-02：估=测 → 偏差 0。
    s.add("L10-修正门-02", drift_permille(1000, 1000) == 0, "零偏差放行");

    // L10-修正门-03：正偏差独立重算（1100/1000 → 100‰）。
    s.add("L10-修正门-03", drift_permille(1100, 1000) == 100, "正偏差 100‰ 独立对账");

    // L10-修正门-04：偏差相对实测（1000 vs 1100 → 90‰，分母是实测）。
    // |1000-1100|=100，denom=1100 → 100000/1100=90（整数截断）。
    s.add(
        "L10-修正门-04",
        drift_permille(1000, 1100) == 90,
        "偏差分母取实测（相对实测口径）",
    );

    // L10-修正门-05：199‰ 放行（门内）。
    s.add(
        "L10-修正门-05",
        drift_permille(1199, 1000) == 199 && !needs_correction(199),
        "199‰ 放行（门内侧）",
    );

    // L10-修正门-06：恰 200‰ 立案（含端点）。
    s.add(
        "L10-修正门-06",
        drift_permille(1200, 1000) == 200 && needs_correction(200),
        "恰 200‰ 立案（含端点，不是 201）",
    );

    // L10-修正门-07：负侧 200‰ 同阈立案（两侧同阈）。
    s.add(
        "L10-修正门-07",
        drift_permille(800, 1000) == 200 && needs_correction(200),
        "模型低估 200‰ 同样立案（高估不逃逸）",
    );

    // L10-修正门-08：实测 0 而预估非 0 → 最大偏差立案。
    s.add(
        "L10-修正门-08",
        needs_correction(drift_permille(1, 0)),
        "估非 0 测 0 → 最大偏差立案（模型漏项不掩盖）",
    );

    // L10-修正门-09：双 0 放行。
    s.add(
        "L10-修正门-09",
        drift_permille(0, 0) == 0 && !needs_correction(0),
        "估测双 0 放行（除零防护不误报）",
    );

    // L10-修正门-10：reconcile 199 → None。
    s.add("L10-修正门-10", reconcile(1199, 1000).is_none(), "对账入口门内侧返 None");

    // L10-修正门-11：reconcile 200 → Some 且记录自洽。
    let ok = match reconcile(1200, 1000) {
        Some(adr) => adr.deviation_permille == 200 && adr.est_ns == 1200 && adr.measured_ns == 1000,
        None => false,
    };
    s.add("L10-修正门-11", ok, "对账入口立案记录三字段自洽");

    // L10-修正门-12：大偏差饱和不回绕（u64 巨值）。
    let ok = needs_correction(drift_permille(u64::MAX / 2, 1));
    s.add("L10-修正门-12", ok, "巨值偏差饱和处理不回绕");
}

// ---------------------------------------------------------------------------
// 组四：次序对齐（先档后率；跳步立案 + 遥测守恒）
// ---------------------------------------------------------------------------

fn chk_order(s: &mut CheckSet, t: &CostTable) {
    // L10-次序-01：次序单源表 = [DensityDownOne, EmissionShrink]。
    s.add(
        "L10-次序-01",
        DEGRADE_ORDER[0] == DegradeStep::DensityDownOne
            && DEGRADE_ORDER[1] == DegradeStep::EmissionShrink,
        "先档后率次序单源钉死",
    );

    // L10-次序-02：预算内不降（next → None）。
    let est = estimate(1, 1, FormFactor::Billboard, false, Tier::High, t).unwrap_or(Estimate {
        sim_ns: 0,
        render_ns: 0,
        sort_ns: 0,
        total_ns: 0,
    });
    let cur = DegradeCursor::new();
    s.add(
        "L10-次序-02",
        next_required_step(&est, u64::MAX, &cur).is_none(),
        "预算内不触发联动（合规放行侧）",
    );

    // L10-次序-03：超预算且密度未尽 → 第一步是密度降档。
    let big_est = estimate(1_000_000, 8, FormFactor::Trail, true, Tier::Low, t);
    let ok = match big_est {
        Ok(e) => next_required_step(&e, 1, &cur) == Some(DegradeStep::DensityDownOne),
        Err(_) => false,
    };
    s.add("L10-次序-03", ok, "超预算首步为密度降档（先档）");

    // L10-次序-04：合法降档执行（计数+1、零违规）。
    let mut c = DegradeCursor::new();
    let ok = apply_step(DegradeStep::DensityDownOne, &mut c) == Ok(())
        && c.density_steps_taken == 1
        && c.order_violations == 0;
    s.add("L10-次序-04", ok, "合法密度降档：计数前移、零违规");

    // L10-次序-05：三档最多降两步（DENSITY_TIERS-1）后 density_exhausted。
    let mut c = DegradeCursor::new();
    let r1 = apply_step(DegradeStep::DensityDownOne, &mut c);
    let r2 = apply_step(DegradeStep::DensityDownOne, &mut c);
    s.add(
        "L10-次序-05",
        r1 == Ok(()) && r2 == Ok(()) && c.density_exhausted() && c.density_steps_taken == 2,
        "三档两步降尽（F2217 前向契约 DENSITY_TIERS=3）",
    );

    // L10-次序-06：密度已尽再降档 → ORDER_SKIPPED + 游标不前移。
    let mut c = DegradeCursor::new();
    let _ = apply_step(DegradeStep::DensityDownOne, &mut c);
    let _ = apply_step(DegradeStep::DensityDownOne, &mut c);
    let before = c;
    let ok = apply_step(DegradeStep::DensityDownOne, &mut c) == Err(DiagCode::ORDER_SKIPPED)
        && c.density_steps_taken == before.density_steps_taken;
    s.add("L10-次序-06", ok, "密度已尽的降档请求立案且游标回滚");

    // L10-次序-07：跳步（密度未尽直接缩率）→ ORDER_SKIPPED + 全状态不前移。
    let mut c = DegradeCursor::new();
    let before = c;
    let ok = apply_step(DegradeStep::EmissionShrink, &mut c) == Err(DiagCode::ORDER_SKIPPED)
        && c.emission_permille == before.emission_permille
        && c.density_steps_taken == before.density_steps_taken;
    s.add("L10-次序-07", ok, "跳步立案且游标回滚（跳步执行结果不可信）");

    // L10-次序-08：违规/遥测守恒（每次跳步两者同步 +1）。
    let mut c = DegradeCursor::new();
    let _ = apply_step(DegradeStep::EmissionShrink, &mut c);
    let _ = apply_step(DegradeStep::EmissionShrink, &mut c);
    s.add(
        "L10-次序-08",
        c.order_violations == 2 && c.telemetry_marks == 2,
        "跳步不静默：违规计数与遥测标记同步记账",
    );

    // L10-次序-09：密度尽后超预算 → EmissionShrink（后率）。
    let ok = match big_est {
        Ok(e) => {
            let mut c = DegradeCursor::new();
            let _ = apply_step(DegradeStep::DensityDownOne, &mut c);
            let _ = apply_step(DegradeStep::DensityDownOne, &mut c);
            next_required_step(&e, 1, &c) == Some(DegradeStep::EmissionShrink)
        }
        Err(_) => false,
    };
    s.add("L10-次序-09", ok, "密度尽后第二步为发射率缩（后率）");

    // L10-次序-10：合法缩率一步 → 1000-200=800‰。
    let mut c = DegradeCursor::new();
    let _ = apply_step(DegradeStep::DensityDownOne, &mut c);
    let _ = apply_step(DegradeStep::DensityDownOne, &mut c);
    let ok = apply_step(DegradeStep::EmissionShrink, &mut c) == Ok(())
        && c.emission_permille == 800;
    s.add("L10-次序-10", ok, "缩率一步 200‰（800‰ 与独立重算一致）");

    // L10-次序-11：发射率下限 100‰（连缩五步不穿）。
    let mut c = DegradeCursor::new();
    let _ = apply_step(DegradeStep::DensityDownOne, &mut c);
    let _ = apply_step(DegradeStep::DensityDownOne, &mut c);
    for _ in 0..8 {
        let _ = apply_step(DegradeStep::EmissionShrink, &mut c);
    }
    s.add(
        "L10-次序-11",
        c.emission_permille == MIN_EMISSION_PERMILLE && c.emission_exhausted(),
        "发射率触底 100‰ 不穿（下限守卫）",
    );

    // L10-次序-12：档率全尽 → next None（预算不可满足显性，不静默硬撑）。
    let ok = match big_est {
        Ok(e) => next_required_step(&e, 1, &c).is_none(),
        Err(_) => false,
    };
    s.add("L10-次序-12", ok, "降无可降显性返 None（预算不可满足可观测）");

    // L10-次序-13：密度折半口径（64>>2=16）。
    s.add(
        "L10-次序-13",
        density_scaled_count(64, 2) == 16 && density_scaled_count(64, 0) == 64,
        "密度折半与 F2209 同口径（每步 >>1）",
    );

    // L10-次序-14：DENSITY_TIERS 钉死 3（F2217 前向契约，改档位必红）。
    s.add("L10-次序-14", DENSITY_TIERS == 3, "密度档位三级前向契约钉死");
}

// ---------------------------------------------------------------------------
// 组五：三方裁决 / 表过期 / 判据自检
// ---------------------------------------------------------------------------

fn chk_arb_table_meta(s: &mut CheckSet, t: &CostTable) {
    // L10-裁决-01：用户 > 档位（用户更紧也胜——优先级不是取小）。
    let (v, a) = arbitrate(Some(5_000_000), Some(4_000_000), None);
    s.add("L10-裁决-01", v == 5_000_000 && a == Authority::User, "用户覆盖胜出（即使比档位松）");

    // L10-裁决-02：档位 > 预算（用户缺位）。
    let (v, a) = arbitrate(None, Some(4_000_000), Some(8_000_000));
    s.add("L10-裁决-02", v == 4_000_000 && a == Authority::Tier, "档位胜于预算");

    // L10-裁决-03：仅预算。
    let (v, a) = arbitrate(None, None, Some(3_000_000));
    s.add("L10-裁决-03", v == 3_000_000 && a == Authority::Budget, "仅预算时预算生效");

    // L10-裁决-04：三级全空 → DEFAULT_BUDGET_NS。
    let (v, a) = arbitrate(None, None, None);
    s.add(
        "L10-裁决-04",
        v == DEFAULT_BUDGET_NS && a == Authority::Budget,
        "全空兜底默认预算（可查非静默）",
    );

    // L10-裁决-05：默认预算钉死 8ms。
    s.add("L10-裁决-05", DEFAULT_BUDGET_NS == 8_000_000, "默认预算 8ms 钉死");

    // L10-裁决-06：生效值恒非零（可执行性）。
    let (v, _) = arbitrate(Some(0), Some(4_000_000), Some(8_000_000));
    s.add(
        "L10-裁决-06",
        arbitrate(Some(1), None, None).0 > 0 && v == 0,
        "用户给 0 就生效 0（裁决不篡改用户输入；0 预算=全降由联动处置）",
    );

    // L10-表-01：新鲜表 Ok。
    s.add("L10-表-01", t.require_fresh(0) == Ok(()), "出厂表在窗内 Ok");

    // L10-表-02：标脏 → STALE_TABLE。
    let mut t2 = CostTable::calibrated();
    t2.mark_stale();
    s.add(
        "L10-表-02",
        t2.require_fresh(0) == Err(DiagCode::STALE_TABLE),
        "脏表拒绝预估对账（过期显性）",
    );

    // L10-表-03：超窗 → STALE_TABLE（>WINDOW 严格）。
    let mut t3 = CostTable::calibrated();
    t3.calibrated_at = 100;
    let over = t3.require_fresh(100 + RECALIBRATION_WINDOW + 1);
    let at_edge = t3.require_fresh(100 + RECALIBRATION_WINDOW);
    s.add(
        "L10-表-03",
        over == Err(DiagCode::STALE_TABLE) && at_edge == Ok(()),
        "重定标窗口含端点：恰在窗端不过期，超 1 纳秒即过期",
    );

    // L10-表-04：recalibrate 清脏 + 刷时戳。
    let mut t4 = CostTable::calibrated();
    t4.mark_stale();
    t4.recalibrate(777);
    s.add(
        "L10-表-04",
        !t4.stale && t4.calibrated_at == 777 && t4.require_fresh(777) == Ok(()),
        "重定标清脏标并刷新时戳（回填流程闭环）",
    );

    // L10-判据-01：诊断码互异。
    let all = DiagCode::ALL;
    let mut ok = true;
    for i in 0..all.len() {
        for j in (i + 1)..all.len() {
            if all[i].0 == all[j].0 {
                ok = false;
            }
        }
    }
    s.add("L10-判据-01", ok, "五诊断码两两互异");

    // L10-判据-02：全码独占 0x35 段（码段判据防越段串号）。
    let mut ok = true;
    for c in all.iter() {
        if c.0 & 0xFF00 != 0x3500 {
            ok = false;
        }
    }
    s.add("L10-判据-02", ok, "诊断码独占 0x35 段");

    // L10-判据-03：判据条数对账（声明值与实际一致——防悄悄增删）。
    // 本条 s.add 执行前集内已有 62 条，加本条后恰为声明值 63。
    s.add(
        "L10-判据-03",
        declared_check_count() == 63 && s.len() == 62,
        "判据条数声明与实际对账（本条为第 63 条）",
    );
}

// ---------------------------------------------------------------------------
// 聚合（单集：62 条 ≤ MAX_CHECKS=112，无截断）
// ---------------------------------------------------------------------------

/// F2210 域自检（聚合入口，注册表用）。
pub fn run_vel10_checks() -> CheckSet {
    let t = CostTable::calibrated();
    let mut s = CheckSet::new("VE-F2210");
    chk_factors(&mut s, &t);
    chk_formula(&mut s, &t);
    chk_gate(&mut s);
    chk_order(&mut s, &t);
    chk_arb_table_meta(&mut s, &t);
    s
}
