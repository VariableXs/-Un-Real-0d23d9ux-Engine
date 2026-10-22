//! VE-F2010 · 判定组 2（错误路径一~四 / 方向 / 边界与选择 / 无障碍 / 采样与归一化 / 自检健康）
//!
//! 判据锚点逐字对应外壳 `vek10_checks.rs` 头注；判据名与断言语义不变。
//! 夹具、反假变体与共享核算核心见 `super::synth`。

use super::synth::*;
use super::super::vek10_fxaa::*;
use crate::checks::CheckSet;

// ── 错误路径一：序位守卫 ─────────────────────────────────────────────

/// K10-错误-HDR输入被拦截：TM 前的 HDR 输入被拦截（`NonLdrInput`）。
pub(crate) fn chk_hdr_input_intercepted(set: &mut CheckSet) {
    let v = guard_domain_order(SignalDomain::LinearHdr, PipelineStage::Fxaa);
    let ok = v == Some(OrderViolation::NonLdrInput);
    set.add("K10-错误-HDR输入被拦截", ok, "LinearHdr -> NonLdrInput");
}

/// K10-错误-排在TM前被拦截：LDR 输入排在 TM/几何之后被拦截。
pub(crate) fn chk_before_tonemap_intercepted(set: &mut CheckSet) {
    let a = guard_domain_order(SignalDomain::PostTonemapLdr, PipelineStage::Tonemap);
    let b = guard_domain_order(SignalDomain::PostTonemapLdr, PipelineStage::Geometry);
    let ok = a == Some(OrderViolation::BeforeTonemap)
        && b == Some(OrderViolation::BeforeTonemap);
    set.add("K10-错误-排在TM前被拦截", ok, "Tonemap/Geometry -> BeforeTonemap");
}

/// K10-错误-排在编码后被拦截：排在输出编码之后被拦截（含已编码输入）。
pub(crate) fn chk_after_output_encode_intercepted(set: &mut CheckSet) {
    let a = guard_domain_order(SignalDomain::PostTonemapLdr, PipelineStage::OutputEncode);
    let b = guard_domain_order(SignalDomain::Encoded, PipelineStage::Fxaa);
    let ok = matches!(a, Some(OrderViolation::AfterOutputEncode))
        && matches!(b, Some(OrderViolation::AfterOutputEncode));
    set.add("K10-错误-排在编码后被拦截", ok, "编码后 -> AfterOutputEncode");
}

/// K10-错误-合法序位不误报：TM 后的 LDR + FXAA 位⇒ `None`。
pub(crate) fn chk_legal_order_not_flagged(set: &mut CheckSet) {
    let ok = guard_domain_order(SignalDomain::PostTonemapLdr, PipelineStage::Fxaa).is_none();
    set.add("K10-错误-合法序位不误报", ok, "TM后LDR+FXAA位 -> None");
}

/// K10-错误-序位三违规全覆盖：三种违规**全覆盖，无 None 漏网**。
/// 反假验证：变体 V8（只查域不查位置）会漏掉 `BeforeTonemap` ⇒ 本判据红。
pub(crate) fn chk_order_violations_full_coverage(set: &mut CheckSet) {
    let all = [
        guard_domain_order(SignalDomain::LinearHdr, PipelineStage::Fxaa),
        guard_domain_order(SignalDomain::PostTonemapLdr, PipelineStage::Tonemap),
        guard_domain_order(SignalDomain::Encoded, PipelineStage::Fxaa),
    ];
    let mut ok = true;
    for v in all.iter() {
        match v {
            Some(x) => {
                if x.code().is_empty() || x.symptom().is_empty()
                    || x.cause().is_empty() || x.advice().is_empty()
                {
                    ok = false;
                }
            }
            None => ok = false,
        }
    }
    // 三码互不相同（不共用一个码糊弄）。
    let c0 = all[0].map(|v| v.code()).unwrap_or("");
    let c1 = all[1].map(|v| v.code()).unwrap_or("");
    let c2 = all[2].map(|v| v.code()).unwrap_or("");
    if c0 == c1 || c1 == c2 || c0 == c2 {
        ok = false;
    }
    // 变体 V8：只查域 ⇒ 第二条返回 None。
    let v8_missed =
        guard_domain_order(SignalDomain::PostTonemapLdr, PipelineStage::Tonemap).is_none();
    set.add(
        "K10-错误-序位三违规全覆盖",
        ok && !v8_missed,
        "三码各带三要素；只查域的变体必须漏报",
    );
}

/// K10-错误-序位表自洽：序位表自身自洽（FXAA 恰在 TM 之后、编码之前）。
pub(crate) fn chk_pipeline_order_sane(set: &mut CheckSet) {
    let ok = pipeline_order_is_sane();
    set.add("K10-错误-序位表自洽", ok, "TM < FXAA < Encode");
}

/// K10-错误-域编码唯一：域编码互不相同（`as u8` 式错位会在这里暴露）。
pub(crate) fn chk_domain_wires_unique(set: &mut CheckSet) {
    let a = SignalDomain::LinearHdr.wire();
    let b = SignalDomain::PostTonemapLdr.wire();
    let c = SignalDomain::Encoded.wire();
    let ok = a != b && b != c && a != c;
    set.add("K10-错误-域编码唯一", ok, "0/1/2 互异");
}

// ── 错误路径二：参数钳制 ─────────────────────────────────────────────

/// K10-错误-极端参数钳回：极端参数全部被钳回有效域（`NaN` / `±inf` / 越界）。
pub(crate) fn chk_extreme_params_clamped(set: &mut CheckSet) {
    let mut bogus = preset(FxaaQuality::High).params;
    bogus.edge_min = f32::NAN;
    bogus.reduce_mul = f32::INFINITY;
    bogus.reduce_min = -1.0;
    bogus.span_max = 0.0;
    bogus.steps_used = 250;
    bogus.steps = [f32::NAN; FXAA_PS_MAX];
    let r = sanitize_params(&bogus);
    let p = r.params;
    let ok = r.clamped
        && p.edge_min >= 1.0 / 1024.0
        && p.edge_min <= 1.0
        && p.reduce_mul >= 0.0
        && p.reduce_mul <= 1.0
        && p.reduce_min >= 1.0 / 4096.0
        && p.reduce_min <= 1.0
        && p.span_max >= 1.0
        && p.span_max <= 32.0
        && p.steps_used >= 1
        && p.steps_used as usize <= FXAA_PS_MAX
        && p.steps.iter().all(|v| *v >= 0.0 && *v <= 4.0);
    set.add("K10-错误-极端参数钳回", ok, "NaN/inf/越界全部兜底");
}

/// K10-错误-合法参数不误钳：合法参数**不被误钳**（`clamped == false`）。
/// 反假：无条件 `clamped=true` 会污染遥测（每次读取都误报钳制）。
pub(crate) fn chk_legal_params_not_clamped(set: &mut CheckSet) {
    let p = preset(FxaaQuality::High).params;
    let r = sanitize_params(&p);
    let ok = !r.clamped && r.params == p;
    set.add("K10-错误-合法参数不误钳", ok, "clamped=false");
}

/// K10-错误-reduce_min下界为正：钳制后 `reduce_min > 0`（否则 `rcpDirMin`
/// 分母可能为 0，`solve_direction` 里的除零分支成死代码）。
pub(crate) fn chk_reduce_min_positive_after_clamp(set: &mut CheckSet) {
    let mut bogus = preset(FxaaQuality::High).params;
    bogus.reduce_min = 0.0;
    let r = sanitize_params(&bogus);
    let ok = r.params.reduce_min > 0.0;
    set.add("K10-错误-reduce_min下界为正", ok, "钳后 > 0");
}

/// K10-错误-零方向分量有限：分母兜底分支可达（不产生 `inf`/`NaN`）。
/// 测的是**死代码风险**：`rcp` 兜底若永不可达，一处 `>=` 改 `>` 就会静默出 `inf`。
pub(crate) fn chk_zero_direction_finite(set: &mut CheckSet) {
    let mut p = preset(FxaaQuality::High).params;
    p.reduce_min = 1.0 / 4096.0;
    // 四邻域全同 ⇒ dx = dy = 0 ⇒ min_abs = 0 ⇒ denom = reduce_min。
    let d = solve_direction(&(0.5, 0.5, 0.5, 0.5), &p);
    let ok = d.x.is_finite() && d.y.is_finite();
    set.add("K10-错误-零方向分量有限", ok, "dx=dy=0 不产生 inf");
}

/// K10-错误-方向钳制生效：方向被 `span_max` 钳制（超大方向差不得越界）。
pub(crate) fn chk_direction_clamped_by_span_max(set: &mut CheckSet) {
    let p = preset(FxaaQuality::High).params;
    // 构造极大亮度差：dir 原始值远超 span_max。
    let d = solve_direction(&(0.0, 0.0, 1.0, 1.0), &p);
    // `rcp` 很小，故最终 `|d.x| <= span_max * rcp <= span_max`。
    let ok = d.x.abs() <= p.span_max + 1.0e-3 && d.x.is_finite();
    set.add("K10-错误-方向钳制生效", ok, "|dir| <= span_max");
}

/// K10-错误-帧净化生效：`NaN` texel 被净化（`sanitize_frame` 报改动且改后全有限）。
pub(crate) fn chk_frame_sanitize(set: &mut CheckSet) {
    let mut f = synth_vertical_edge(N);
    if let Some(i) = f.px.iter().position(|c| !c.is_finite()) {
        f.px[i] = Rgb::new(f32::NAN, f32::INFINITY, f32::NEG_INFINITY);
    } else {
        f.px[0] = Rgb::new(f32::NAN, f32::NAN, f32::NAN);
    }
    let (clean, touched) = sanitize_frame(&f);
    let ok = touched && clean.px.iter().all(|c| c.is_finite());
    set.add("K10-错误-帧净化生效", ok, "NaN/inf texel -> 有限");
}

/// K10-错误-NaN不静默黑屏：`NaN` 输入不静默黑屏（变体 V7 的反假验证）。
/// NaN 传播链：luma 比较 ⇒ 早退被绕过 ⇒ 方向解算吃满 NaN ⇒ 整屏黑且零报错；
/// 真实现必须把 NaN 折叠（`finite_or`）使输出恒有限。
pub(crate) fn chk_nan_not_silent_black(set: &mut CheckSet, ctx: &PresetCtx) {
    let mut f = synth_vertical_edge(N);
    f.px[0] = Rgb::new(f32::NAN, f32::NAN, f32::NAN);
    let real = resolve(&f, &ctx.high);
    let real_finite = match &real {
        Some(v) => v.px.iter().all(|c| c.is_finite()),
        None => false,
    };
    // 变体 V7：不兜底的 luma ⇒ NaN 传播 ⇒ 判据应转红。
    let nan = Rgb::new(f32::NAN, f32::NAN, f32::NAN);
    let v7_luma = variant_luma_no_guard(nan);
    let v7_breaks = v7_luma.is_finite();
    set.add(
        "K10-错误-NaN不静默黑屏",
        real_finite && !v7_breaks,
        "输出恒有限；不兜底变体必须产生非有限亮度",
    );
}

/// K10-错误-反序区间不panic：`clamp_range` 在 `lo > hi` 时不 panic（返回 `lo`）。
pub(crate) fn chk_reversed_range_no_panic(set: &mut CheckSet) {
    let ok = clamp_range(0.5, 1.0, 0.0) == 1.0;
    set.add("K10-错误-反序区间不panic", ok, "lo>hi -> lo");
}

// ── 错误路径三：低配推荐（非强制） ───────────────────────────────────

/// K10-错误-低配建议FXAA：预算充裕 + 无 MSAA ⇒ 建议 FXAA 低档（**建议**非强制）。
/// **用 `Mid` 档预算**（踩过坑）：低档预算 0.25ms 低于高档成本 0.4ms，
/// 低档预算下的正确答案是"别加"——用低档预算期待"建议 FXAA"会永远判红。
pub(crate) fn chk_low_budget_suggests_fxaa(set: &mut CheckSet) {
    let r = recommend_for_budget(BudgetTier::Mid, false, false);
    let ok = r == AaRecommendation::SuggestFxaaLow && !r.text().is_empty();
    set.add("K10-错误-低配建议FXAA", ok, "Mid/no-msaa -> SuggestFxaaLow");
}

/// K10-错误-高配建议并用：高预算 + 已开 MSAA ⇒ 建议并用（互补而非互斥）。
pub(crate) fn chk_high_budget_suggests_combo(set: &mut CheckSet) {
    let r = recommend_for_budget(BudgetTier::High, true, false);
    let ok = matches!(r, AaRecommendation::SuggestFxaaPlusMsaa);
    set.add("K10-错误-高配建议并用", ok, "High/msaa -> SuggestFxaaPlusMsaa");
}

/// K10-错误-TAA已开不推：TAA 已开 ⇒ 不推FXAA（收益/代价比不明确，保持现状最诚实）。
pub(crate) fn chk_taa_on_no_fxaa(set: &mut CheckSet) {
    let r = recommend_for_budget(BudgetTier::High, false, true);
    set.add("K10-错误-TAA已开不推", r == AaRecommendation::None, "taa_on -> None");
}

/// K10-错误-预算不足不推荐：预算不足 ⇒ `None`（不硬塞建议）。
/// 低档预算 0.25ms < 高档成本 0.4ms ⇒ 即使 MSAA 开着也不该再叠 FXAA；
/// 这是"降级纪律"的落点：预算不足时**不给**建议。
pub(crate) fn chk_insufficient_budget_none(set: &mut CheckSet) {
    let ok = match recommend_for_budget(BudgetTier::Low, true, false) {
        AaRecommendation::None => true,
        _ => false,
    };
    set.add("K10-错误-预算不足不推荐", ok, "超预算 -> None");
}

/// K10-错误-降级分支可达：**降级分支可达**（防死代码）。
/// 若 `fits` 判定恒真，"预算不足"分支永不执行——降级纪律只剩注释。
pub(crate) fn chk_degrade_branch_reachable(set: &mut CheckSet) {
    let over = cost_ms_1080p(FxaaQuality::High) >= BudgetTier::Low.budget_ms_1080p();
    set.add("K10-错误-降级分支可达", over, "高档成本 >= 低档预算");
}

// ── 错误路径四：CAS 联动 ─────────────────────────────────────────────

/// K10-错误-CAS联动告警：CAS 排在 FXAA 之后，且**同时开启给出警告**（不静默关掉）。
pub(crate) fn chk_cas_coordination_warns(set: &mut CheckSet) {
    let ok = all_qualities(|q| {
        let c = cas_coordination(q);
        c.cas_after_fxaa && c.warn
    });
    set.add("K10-错误-CAS联动告警", ok, "三档均 cas_after_fxaa+warn");
}

/// K10-错误-高档CAS峰值为0：模糊已足，不再叠锐化对冲。
pub(crate) fn chk_high_cas_peak_zero(set: &mut CheckSet) {
    let c = cas_coordination(FxaaQuality::High);
    let ok = c.cas_peak_hint == 0.0 && c.warn;
    set.add("K10-错误-高档CAS峰值为0", ok, "High -> peak=0但仍警告");
}

// ── 判据五：方向正交（解析律） ───────────────────────────────────────

/// K10-方向-垂直边缘x精确零：垂直边缘 ⇒ `dir.x` **精确为 0**，`dir.y` 非零。
/// x/y 语义（此前写反过，钉死）：`dir.x` 度量**上下**亮度差、`dir.y` 度量
/// **左右**亮度差，垂直边缘落在 `dir.y` 上。精确 `== 0.0` 而非 eps：两边是
/// 同样的两个数相减，f32 逐位归零；eps 只会放过"算错但很小"的实现。
pub(crate) fn chk_vertical_edge_dir_x_zero(set: &mut CheckSet, ctx: &PresetCtx) {
    let img = synth_vertical_edge(N);
    let ok = dir_verdict(&img, N / 2 - 1, N / 2, |t| {
        let d = solve_direction(t, &ctx.high);
        let v = variant_solve_direction_swapped(t, &ctx.high);
        d.x == 0.0 && d.y != 0.0 && v.x != 0.0
    });
    set.add("K10-方向-垂直边缘x精确零", ok, "dir.x==0且dir.y!=0；对调变体必须破");
}

/// K10-方向-水平边缘y精确零：水平边缘 ⇒ `dir.y` **精确为 0**，`dir.x` 非零。
pub(crate) fn chk_horizontal_edge_dir_y_zero(set: &mut CheckSet, ctx: &PresetCtx) {
    let img = synth_horizontal_edge(N);
    // 与垂直边缘判据走不同骨架：两路 dir_verdict 合取（各自核对一个分量）。
    let ok = dir_verdict(&img, N / 2, N / 2 - 1, |t| solve_direction(t, &ctx.high).y == 0.0)
        && dir_verdict(&img, N / 2, N / 2 - 1, |t| solve_direction(t, &ctx.high).x != 0.0);
    set.add("K10-方向-水平边缘y精确零", ok, "dir.y==0且dir.x!=0");
}

/// K10-方向-对角边缘两分量非零：否则说明有一路被写死。
pub(crate) fn chk_diagonal_edge_both_nonzero(set: &mut CheckSet, ctx: &PresetCtx) {
    let img = synth_diagonal_edge(N);
    let probe = dir_verdict(&img, N / 2 - 1, N / 2 - 1, |t| {
        let d = solve_direction(t, &ctx.high);
        d.x != 0.0 && d.y != 0.0
    });
    set.add("K10-方向-对角边缘两分量非零", probe, "dir.x!=0且dir.y!=0");
}

/// K10-方向-长度在跨度内：方向长度在 `span_max` 内（钳制不越界）。
pub(crate) fn chk_direction_len_within_span(set: &mut CheckSet, ctx: &PresetCtx) {
    let img = synth_multiscale(N);
    let ok = dir_verdict(&img, N / 2, N / 2 - 1, |t| {
        let d = solve_direction(t, &ctx.high);
        let l = d.len();
        l.is_finite() && l > 0.0 && l <= ctx.high.span_max + 1.0e-3
    });
    set.add("K10-方向-长度在跨度内", ok, "0 < |dir| <= span_max");
}

/// K10-方向-未触碰钳制边界：归一化方向**不落在钳制边界上**。
/// 来历：变异测试里 M8"归一化后又钳一次"没让任何判据转红，实测为**等价变异**
/// （归一化分量本就恒 ≤ span_max，二次钳制改变的方向数为 0）——保留本判据
/// 是为了钉住该性质：归一化被改坏时分量会顶到边界，本判据立刻转红。
pub(crate) fn chk_direction_not_touching_boundary(set: &mut CheckSet, ctx: &PresetCtx) {
    let img = synth_multiscale(N);
    let mut at_boundary = 0usize;
    let mut n = 0usize;
    let mut max_len = 0.0f32;
    for y in 0..N {
        for x in 0..N {
            let t = match sample_luma_diagonal(&img, x, y) {
                Some(t) => t,
                None => continue,
            };
            let d = solve_direction(&t, &ctx.high);
            let l = d.len();
            if l > max_len {
                max_len = l;
            }
            n += 1;
            if (l - ctx.high.span_max).abs() < 1.0e-3
                || (d.x.abs() - ctx.high.span_max).abs() < 1.0e-3
                || (d.y.abs() - ctx.high.span_max).abs() < 1.0e-3
            {
                at_boundary += 1;
            }
        }
    }
    set.add(
        "K10-方向-未触碰钳制边界",
        n > 0 && at_boundary == 0 && max_len > 0.0 && max_len < ctx.high.span_max,
        "归一化分量恒在 span_max 内侧（M8 二次钳制为等价变异）",
    );
}

// ── 判据六：边界与选择（半开区间 / 严格小于） ─────────────────────────

/// K10-边界-早退半开：早退用**半开区间** `[0, edge_min)`。
/// 构造 `range_l == threshold` 的合成图：正确实现判 `is_edge == true`
///（闭区间早退会判 false）；变体 V1（闭区间）必须在这里转红。
pub(crate) fn chk_early_exit_half_open(set: &mut CheckSet) {
    // 构造：中心黑(0)、四邻域全白(1.0) ⇒ range_l = 1.0，取 edge_min = 1.0
    // ⇒ threshold = 1.0 ⇒ range_l == threshold。
    let mut f = Frame::filled(5, 5, Rgb::new(1.0, 1.0, 1.0));
    // 夹具坐标 (2,2) 在 5×5 图内，`put` 不会失败；失败即夹具构造逻辑
    // 被改坏，显性登记红并早退（不再 `let _ =` 静默继续）。
    if !put_checked(&mut f, 2, 2, Rgb::new(0.0, 0.0, 0.0)) {
        set.add("K10-边界-早退半开", false, "夹具放置越界（不应发生）");
        return;
    }
    let mut p = preset(FxaaQuality::High).params;
    p.edge_min = 1.0;
    let probe = detect_edge(&f, 2, 2, &p);
    let real_is_edge = probe.map(|q| q.is_edge).unwrap_or(false);
    let range_eq_thresh = probe
        .map(|q| (q.range_l - q.threshold).abs() < 1.0e-6)
        .unwrap_or(false);
    // 变体 V1：闭区间 ⇒ 同一点判为非边缘。
    let v1 = variant_detect_edge_closed(&f, 2, 2, &p);
    let v1_is_edge = v1.map(|q| q.is_edge).unwrap_or(true);
    set.add(
        "K10-边界-早退半开",
        real_is_edge && range_eq_thresh && !v1_is_edge,
        "range==threshold 判边缘；闭区间变体必须判非边缘",
    );
}

/// K10-边界-低于阈值早退：`range_l < threshold` 早退（严格小于侧）。
pub(crate) fn chk_below_threshold_early_exit(set: &mut CheckSet) {
    // 平场：range_l = 0 < threshold（正）⇒ 非边缘。
    let f = Frame::filled(5, 5, Rgb::new(0.5, 0.5, 0.5));
    let p = preset(FxaaQuality::High).params;
    let probe = detect_edge(&f, 2, 2, &p);
    let ok = probe
        .map(|q| !q.is_edge && q.range_l == 0.0 && q.threshold > 0.0)
        .unwrap_or(false);
    set.add("K10-边界-低于阈值早退", ok, "平场 range=0 < t");
}

/// K10-选择-严格小于：末端选择用**严格小于**（`lumaB < lumaM`）。
/// 精确相等点上真实现选 B（两个 `<` 皆 false），V6（`<=`）选 A——确实不同。
pub(crate) fn chk_strict_less_selection(set: &mut CheckSet) {
    let at_eq = select_source(0.5, 0.5, 0.0);
    let ok = at_eq == FxaaSource::TapB;
    set.add("K10-选择-严格小于", ok, "lumaB==lumaM -> TapB（< 语义）");
}

/// K10-选择-低于min取A：`lumaB < lumaMin` 单独触发取 A。
pub(crate) fn chk_below_min_tap_a(set: &mut CheckSet) {
    set.add(
        "K10-选择-低于min取A",
        select_source(0.1, 0.5, 0.2) == FxaaSource::TapA,
        "lumaB<lumaMin -> TapA",
    );
}

/// K10-选择-高于m取B：`lumaB > lumaM` 且 `>= lumaMin` ⇒ 取 B。
pub(crate) fn chk_above_m_tap_b(set: &mut CheckSet) {
    let r = select_source(0.8, 0.5, 0.2);
    set.add(
        "K10-选择-高于m取B",
        matches!(r, FxaaSource::TapB),
        "lumaB>lumaM -> TapB",
    );
}

// ── 判据七：无障碍与声明 ─────────────────────────────────────────────

/// K10-无障碍-影响已登记：无障碍影响**已登记**且带建议，且诚实限定 UI 不受影响。
pub(crate) fn chk_a11y_registered(set: &mut CheckSet) {
    let a = a11y_advisory();
    let ok = a.registered
        && !a.affected.is_empty()
        && !a.impact.is_empty()
        && !a.advice.is_empty()
        && a.ui_unaffected;
    set.add("K10-无障碍-影响已登记", ok, "登记+建议+UI豁免限定");
}

/// K10-无障碍-三份声明非空（模糊/采样/性能诚实标注）。
pub(crate) fn chk_three_decls_non_empty(set: &mut CheckSet) {
    let ok = !BLUR_HONESTY_DECL.is_empty()
        && !SAMPLE_MODE_DECL.is_empty()
        && !PERF_HONESTY_DECL.is_empty();
    set.add("K10-无障碍-三份声明非空", ok, "模糊/采样/性能声明齐备");
}

/// K10-无障碍-预设可序列化：预设参数可序列化（诊断面板用）。
pub(crate) fn chk_preset_params_serializable(set: &mut CheckSet) {
    let t = preset_params_text(FxaaQuality::High);
    let ok = t.contains("edge_min=") && t.contains("steps_used=");
    set.add("K10-无障碍-预设可序列化", ok, "含全部参数字段");
}

/// K10-无障碍-定点格式化：定点格式化不 panic 且小数位固定 4 位。
pub(crate) fn chk_fixed_format(set: &mut CheckSet) {
    let a = fmt_f32(1.0);
    let b = fmt_f32(-2.5);
    let c = fmt_f32(0.0);
    let ok = a == "1.0000" && b == "-2.5000" && c == "0.0000";
    set.add("K10-无障碍-定点格式化", ok, "1.0000/-2.5000/0.0000");
}

/// K10-无障碍-Rgb基础运算：`Rgb` 基础运算（逐分量 min/max/mean）。
pub(crate) fn chk_rgb_basic_ops(set: &mut CheckSet) {
    let a = Rgb::new(0.2, 0.5, 0.8);
    let b = Rgb::new(0.6, 0.3, 0.1);
    let mn = a.min3(b);
    let mx = a.max3(b);
    let av = a.mean2(b);
    let ok = mn.r == 0.2 && mn.g == 0.3 && mn.b == 0.1
        && mx.r == 0.6 && mx.g == 0.5 && mx.b == 0.8
        && (av.r - 0.4).abs() < 1.0e-6
        && (av.g - 0.4).abs() < 1.0e-6
        && (av.b - 0.45).abs() < 1.0e-6;
    set.add("K10-无障碍-Rgb基础运算", ok, "min/max/mean 逐分量");
}

/// K10-无障碍-Rgb饱和：`Rgb::saturate` 钳到 [0,1]。
pub(crate) fn chk_rgb_saturate(set: &mut CheckSet) {
    let r = Rgb::new(-0.5, 0.5, 2.0).saturate();
    let ok = r.r == 0.0 && r.g == 0.5 && r.b == 1.0;
    set.add("K10-无障碍-Rgb饱和", ok, "钳到 [0,1]");
}

/// K10-无障碍-有限性判定：`is_finite` 正确识别 `NaN`/`inf`。
pub(crate) fn chk_rgb_is_finite(set: &mut CheckSet) {
    let ok = Rgb::new(0.0, 0.0, 0.0).is_finite()
        && !Rgb::new(f32::NAN, 0.0, 0.0).is_finite()
        && !Rgb::new(0.0, f32::INFINITY, 0.0).is_finite();
    set.add("K10-无障碍-有限性判定", ok, "NaN/inf -> false");
}

// ── 判据九：双线性权重解析律（堵V2 退化中点） ─────────────────────────

/// K10-采样-双线性权重随位置变化：堵 V2 退化中点的同族变体（权重钉 0.5）。
/// 判据侧**独立重算**：线性斜坡图上 `sample_bilinear` 在 fx = 1.0/1.25/1.75
/// 三点的解析值两两不同（0.25/0.3125/0.4375）；权重钉 0.5 的变体塌成恒
/// 0.375，与本判据正交（`K10-单pass-确有边缘被处理`绿的变体这里会红）。
pub(crate) fn chk_bilinear_weights_vary(set: &mut CheckSet) {
    // 斜坡：R 通道随 x 线性递增，G/B 恒0 ⇒ 插值结果可直接解析对账。
    let mut ramp = Frame::filled(4, 4, Rgb::new(0.0, 0.0, 0.0));
    for y in 0..4u32 {
        for x in 0..4u32 {
            // 夹具坐标由界内循环生成，`put` 不会失败；失败显性登记红并早退。
            if !put_checked(&mut ramp, x, y, Rgb::new(x as f32 * 0.25, 0.0, 0.0)) {
                set.add("K10-采样-双线性权重随位置变化", false, "夹具放置越界（不应发生）");
                return;
            }
        }
    }
    let s0 = sample_bilinear(&ramp, 1.0, 1.0);
    let s25 = sample_bilinear(&ramp, 1.25, 1.0);
    let s75 = sample_bilinear(&ramp, 1.75, 1.0);
    let ok = some3(s0, s25, s75, |a, b, c| {
        a.r != b.r && b.r != c.r && a.r != c.r
            && (a.r - 0.25).abs() < 1.0e-6
            && (b.r - 0.3125).abs() < 1.0e-6
            && (c.r - 0.4375).abs() < 1.0e-6
    });
    set.add(
        "K10-采样-双线性权重随位置变化",
        ok,
        "1.0/1.25/1.75 三点权重 0/0.25/0.75 解析对账；恒 0.5 变体必塌陷",
    );
}

// ── 判据十：归一化后**不二次钳制**（堵 V5 归一化后再钳） ───────────────

/// K10-方向-归一化后不二次钳制：`span_max` 钳制**只作用在归一化之前**。
/// 头注记录过该缺陷但当时只有注释——注释不是门禁。判据侧独立重算解析律：
/// `solve_direction` 输出须逐位等于 `clamp(d, ±span_max) * rcp`（不带二次
/// 钳制）；取未钳值远超 span_max 的强边缘点，多钳一次必被压回边界而变红。
pub(crate) fn chk_no_second_clamp_after_normalize(set: &mut CheckSet) {
    // 解析推导测试点：nw=0, ne=0.5, sw=1.0, se=0.5，reduce_mul=0、
    // reduce_min=1/4096、span_max=1.0 ⇒ dy 精确零 ⇒ rcp=4096，参考值
    // rx = 4096 >> span_max=1.0 —— 二次钳制变体必被压回 ±1.0 边界。
    let p = second_clamp_params();
    let mut f = Frame::filled(9, 9, Rgb::new(0.5, 0.5, 0.5));
    // 四个对角位：nw=0, ne=0.5, sw=1.0, se=0.5（中心 (4,4)）。
    // 夹具坐标均为界内字面量，`put` 不会失败；失败显性登记红并早退。
    if !put_checked(&mut f, 3, 3, Rgb::new(0.0, 0.0, 0.0))
        || !put_checked(&mut f, 5, 3, Rgb::new(0.5, 0.5, 0.5))
        || !put_checked(&mut f, 3, 5, Rgb::new(1.0, 1.0, 1.0))
        || !put_checked(&mut f, 5, 5, Rgb::new(0.5, 0.5, 0.5))
    {
        set.add("K10-方向-归一化后不二次钳制", false, "夹具放置越界（不应发生）");
        return;
    }
    let p4 = sample_luma_diagonal(&f, 4, 4);
    let mut all_ok = false;
    if let Some(t) = p4 {
        let (nw, ne, sw, se) = t;
        let (dx, dy, rx, ry) = reference_dir_components(nw, ne, sw, se, &p);
        let got = solve_direction(&t, &p);
        // 先确认本测试点确实落在"未钳值远超 span_max"的区域，
        // 否则这条判据会退化成恒真（自己问自己答案 = 自证式）。
        let beyond = rx.abs() > p.span_max && ry.abs() <= p.span_max;
        all_ok = beyond
            && dy == 0.0
            && (got.x - rx).abs() < 1.0e-2
            && (got.y - ry).abs() < 1.0e-2;
    }
    set.add(
        "K10-方向-归一化后不二次钳制",
        all_ok,
        "dy=0强边缘点逐位对账 clamp*rcp（未钳值 4096 >> span_max 1.0）；二次钳制变体必被压回边界",
    );
}

// ── 判据八：自检集自身健康（防截断丢红） ─────────────────────────────

/// K10-自检-未溢出容量：本自检集未溢出 `MAX_CHECKS`（防截断丢红）。
/// `CheckSet` 满了会静默丢弃多余结果（`dropped++`），域聚合看不到。
pub(crate) fn chk_selfcheck_capacity(set: &mut CheckSet) {
    let ok = !set.truncated() && set.len() < crate::checks::MAX_CHECKS;
    set.add("K10-自检-未溢出容量", ok, "len < MAX_CHECKS");
}
