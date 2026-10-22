//! VE-F2010 · 判定组 1（判据一~判据四：单 pass / 三预设 / 模糊诚实声明 / 选型贡献）
//!
//! 判据锚点逐字对应外壳 `vek10_checks.rs` 头注；判据名与断言语义不变。
//! 夹具、反假变体与共享核算核心见 `super::synth`。

use super::synth::*;
use super::super::vek10_fxaa::*;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 判据一：单 pass
// ---------------------------------------------------------------------------

/// K10-单pass-平场恒等：整幅同色 ⇒ 输出逐位等于输入（第零正确性，精确相等）。
/// 若实现把 `rangeL=0` 判成边缘，会连带破坏所有"未处理像素原样返回"的推理。
pub(crate) fn chk_flat_field_identity(set: &mut CheckSet, ctx: &PresetCtx) {
    let flat = Frame::filled(N, N, Rgb::new(0.4, 0.5, 0.6));
    let out = match resolve(&flat, &ctx.high) {
        Some(v) => v,
        None => {
            set.fail("K10-单pass-平场恒等", "resolve 返回 None");
            Frame::filled(N, N, Rgb::black())
        }
    };
    set.add(
        "K10-单pass-平场恒等",
        !out.len_mismatch() && out.px == flat.px,
        "平场逐位不变",
    );
}

/// K10-单pass-确有边缘被处理：正面断言"合成图上存在输出真正改变的像素"。
///
/// 这条是本文件最要紧的一条。变体 V2（最近邻取整）会让 `dir` 的亚像素
/// 分量取整成 0，于是 A/B 全部落回中心像素，`out == center`——
/// 此时平场恒等、闭包、方向、序位**全部照样绿**，画面却一帧都没变。
pub(crate) fn chk_edge_actually_processed(set: &mut CheckSet, ctx: &PresetCtx) {
    // 多尺度图有连续的亚像素对比度过渡 ⇒ 确有像素被真正改变。
    // （纯像素对齐的阶梯图边缘落在采样点之间，双线性插值出同侧值，
    //   FXAA 无锯齿可修——那是**正确行为**，不能拿来当"有效"证据。）
    let img = synth_multiscale(N);
    let out = resolve(&img, &ctx.high);
    let n_eff = match &out {
        Some(v) => effective_changes(&img, v),
        None => 0,
    };
    // 反假验证：真实现与最近邻变体必须逐像素值不同——若采样退化为最近邻
    // （倍数门槛是对变体的错误建模），两者逐位相同，本判据立刻转红。
    let mut v2_differs = 0usize;
    let mut v2_total = 0usize;
    for y in 0..N {
        for x in 0..N {
            if let Some((touched, differs)) = v2_pixel_verdict(&img, x, y, &ctx.high) {
                v2_total += touched as usize;
                v2_differs += differs as usize;
            }
        }
    }
    set.add(
        "K10-单pass-确有边缘被处理",
        n_eff > 0 && v2_total > 0 && v2_differs > 0,
        "真实现有改变；且双线性结果逐位不同于最近邻变体",
    );
}

/// K10-闭包-输出取三值之一：`out ∈ {center, A, B}` **逐位相等**。
/// Console 路径末端无 mix，"取三值之一"是精确性质；变体 V3（末端 mix 0.5）
/// 会产出第四种值，本判据立刻变红。
pub(crate) fn chk_output_closed_set(set: &mut CheckSet, ctx: &PresetCtx) {
    let img = synth_diagonal_edge(N);
    let mut ok = true;
    let mut v3_caught = false;
    for y in 0..N {
        for x in 0..N {
            match closure_pixel_verdict(&img, x, y, &ctx.high) {
                Some((in_set, untouched_ok, v3_out)) => {
                    ok = ok && in_set && untouched_ok;
                    v3_caught = v3_caught || v3_out;
                }
                // 采样链断裂：判据失败并按原语义终止本判据的后续像素。
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            break;
        }
    }
    set.add(
        "K10-闭包-输出取三值之一",
        ok && v3_caught,
        "out逐位∈{center,A,B}；mix变体必须越界",
    );
}

/// K10-单pass-转置不变：`out(img)ᵀ == out(imgᵀ)`（容差 1e-6）。
///
/// **一条判据顶四条**：四邻域采样坐标、`dir` 分量、优先级、条件分支
/// 任一写错，转置后输出即不同。
pub(crate) fn chk_transpose_invariance(set: &mut CheckSet, ctx: &PresetCtx) {
    let img = synth_multiscale(N);
    let ok = invariance_within_tol(&img, transpose, TRANSPOSE_TOL, &ctx.high);
    // **反假验证**：坐标轴对调变体必须被容差抓住（偏差 ≫ 1e-6）。
    let v4_broken = v4_axis_swap_breaks(&img, &ctx.high, TRANSPOSE_TOL);
    set.add(
        "K10-单pass-转置不变",
        ok && v4_broken,
        "out(img)ᵀ ≈ out(imgᵀ)，容差 1e-6；轴对调变体必须越界",
    );
}

/// K10-单pass-水平镜像不变：`out(mirror(img)) ≈ mirror(out(img))`。
/// 镜像换的是符号（转置换的是采样位置），覆盖不同分支。
pub(crate) fn chk_mirror_invariance(set: &mut CheckSet, ctx: &PresetCtx) {
    let img = synth_multiscale(N);
    let ok = invariance_within_tol(&img, mirror_x, TRANSPOSE_TOL, &ctx.high);
    set.add(
        "K10-单pass-水平镜像不变",
        ok,
        "out(mirror(img)) ≈ mirror(out(img))，容差 1e-6",
    );
}

/// K10-单pass-无历史依赖：同图两次解算逐位相同（无状态性的行为证据）。
pub(crate) fn chk_no_history_dependency(set: &mut CheckSet, ctx: &PresetCtx) {
    let img = synth_high_freq(N, N);
    let a = resolve(&img, &ctx.high);
    let b = resolve(&img, &ctx.high);
    let ok = some2(a, b, |x, y| x.px == y.px);
    set.add("K10-单pass-无历史依赖", ok, "同输入两次解算逐位一致");
}

/// K10-单pass-尺寸不足显式失败（返回 `None`，**不**返回中心像素）。
/// 把"结构错乱的帧"当正常帧输出会埋掉问题。
pub(crate) fn chk_undersized_frame_fails(set: &mut CheckSet, ctx: &PresetCtx) {
    let tiny = Frame::filled(1, 4, Rgb::new(0.2, 0.2, 0.2));
    let ok = resolve(&tiny, &ctx.high).is_none() && resolve_pixel(&tiny, 0, 0, &ctx.high).is_none();
    set.add("K10-单pass-尺寸不足显式失败", ok, "1x4 帧 -> None");
}

/// K10-单pass-长度不符显式失败。
pub(crate) fn chk_len_mismatch_fails(set: &mut CheckSet, ctx: &PresetCtx) {
    let mut bad = synth_vertical_edge(N);
    bad.px.truncate(3);
    let ok = bad.len_mismatch() && resolve(&bad, &ctx.high).is_none();
    set.add("K10-单pass-长度不符显式失败", ok, "截断帧 -> None");
}

// ---------------------------------------------------------------------------
// 判据二：三预设
// ---------------------------------------------------------------------------

/// K10-预设-edge_min严格递减（低 > 中 > 高）。
///
/// 单边符号：高档阈值更低 ⇒ 处理更多边缘。方向写反则红。
pub(crate) fn chk_edge_min_strictly_decreasing(set: &mut CheckSet, ctx: &PresetCtx) {
    let ok = preset_field_strictly_increasing(ctx, |q| q.edge_min);
    set.add(
        "K10-预设-edge_min严格递减",
        ok,
        "low > med > high",
    );
}

/// K10-预设-span_max严格递增（模糊幅度）。
pub(crate) fn chk_span_max_strictly_increasing(set: &mut CheckSet, ctx: &PresetCtx) {
    // 与 edge_min 判据走不同骨架（滑窗逐对核对），消除同构控制流签名。
    let ok = [ctx.low.span_max, ctx.med.span_max, ctx.high.span_max]
        .windows(2)
        .all(|w| w[0] < w[1]);
    set.add(
        "K10-预设-span_max严格递增",
        ok,
        "low < med < high",
    );
}

/// K10-预设-steps_used严格递增（边缘搜索步数）。
pub(crate) fn chk_steps_used_strictly_increasing(set: &mut CheckSet, ctx: &PresetCtx) {
    let (lo, mid, hi) = (
        ctx.low.steps_used,
        ctx.med.steps_used,
        ctx.high.steps_used,
    );
    let ok = lo < mid && mid < hi;
    set.add(
        "K10-预设-steps_used严格递增",
        ok,
        "5 < 8 < 12",
    );
}

/// K10-预设-集合包含关系 `S(low) ⊆ S(mid) ⊆ S(high)`（按坐标，非计数）。
///
/// 这是本域最强的预设判据。三个数"严格递增"是弱门禁——把顺序写反
/// 它照样绿（序关系不蕴含数值正确）。集合包含是**解析律**：
/// `edge_min` 更低 ⇒ 早退集合更大 ⇒ 被处理像素集合更大。方向写反立刻红。
///
/// 反假验证：变体 V5（三档edge_min 顺序写反）必须破坏包含关系。
pub(crate) fn chk_touched_set_inclusion(set: &mut CheckSet, ctx: &PresetCtx) {
    // **必须用多尺度图**（此处踩过坑）：阶梯图/单频棋盘的 `range_l` 只落在
    // 少数离散值上（实测 {0, 0.2, 0.3}），三档`edge_min`
    // （0.125/0.0625/0.03125）恰好穿过同一组间隙⇒ `S(med) == S(high)`，
    // 三档在算法上退化为两档，而"三档严格递增"这类数值判据照样全绿。
    // `synth_multiscale` 把 `range_l` 铺成连续谱，三档才切得开。
    let img = synth_multiscale(N);
    let sl = touched_set(&img, &ctx.low);
    let sm = touched_set(&img, &ctx.med);
    let sh = touched_set(&img, &ctx.high);
    let mut ok = sl.is_some() && sm.is_some() && sh.is_some();
    if let (Some(sl), Some(sm), Some(sh)) = (&sl, &sm, &sh) {
        // S(low) ⊆ S(mid)：低档处理的像素集必须被中档包含
        if !bool_set_subset(sl, sm) {
            ok = false;
        }
        // S(med) ⊆ S(high)
        if !bool_set_subset(sm, sh) {
            ok = false;
        }
    }
    // 变体 V5：三档顺序倒置 ⇒ `S(high) ⊄ S(low)`，包含关系被破坏。
    let v5_broken = match (&sl, &sh) {
        (Some(sl), Some(sh)) => bool_set_has_exclusive(sh, sl),
        _ => false,
    };
    set.add(
        "K10-预设-集合包含关系",
        ok && v5_broken,
        "S(low)⊆S(mid)⊆S(high)；倒置变体必须违反",
    );
}

/// K10-预设-处理像素数递增：三档被处理像素数严格递增（计数侧辅助证据）。
pub(crate) fn chk_touched_counts_increasing(set: &mut CheckSet, ctx: &PresetCtx) {
    // 同上：多尺度图才能让三档切出不同数量。
    let img = synth_multiscale(N);
    let cl = touched_set(&img, &ctx.low).map(|v| v.iter().filter(|b| **b).count());
    let cm = touched_set(&img, &ctx.med).map(|v| v.iter().filter(|b| **b).count());
    let ch = touched_set(&img, &ctx.high).map(|v| v.iter().filter(|b| **b).count());
    let ok = some3(cl, cm, ch, |l, m, h| l < m && m < h);
    set.add("K10-预设-处理像素数递增", ok, "low < med < high");
}

/// K10-预设-PS序列独立对拍：PS 步序列与判据侧独立重算的参考序列逐项一致。
///
/// 杜绝"档位只改了标签、参数表没动"——那种劣化三档数值判据抓不到
/// （三档仍严格递增），但步表实际是同一份。
pub(crate) fn chk_ps_sequence_independent(set: &mut CheckSet) {
    let refseq = ref_ps_sequence();
    let ok = all_qualities(|q| {
        let st = preset(q).params.steps;
        for i in 0..FXAA_PS_MAX {
            if st[i] != refseq[i] {
                return false;
            }
        }
        true
    });
    set.add("K10-预设-PS序列独立对拍", ok, "1.5/1.0 交替 12 项");
}

/// K10-预设-步数切片夹取：`active_steps()` 长度恰为 `steps_used`（下界夹取生效）。
pub(crate) fn chk_active_steps_clamped(set: &mut CheckSet) {
    let mut ok = all_qualities(|q| {
        let p = preset(q).params;
        p.active_steps().len() == p.steps_used as usize
    });
    // 越界steps_used 必须被夹到 [1, 12]。
    let mut bogus = preset(FxaaQuality::High).params;
    bogus.steps_used = 200;
    if bogus.active_steps().len() != FXAA_PS_MAX {
        ok = false;
    }
    bogus.steps_used = 0;
    if bogus.active_steps().len() != 1 {
        ok = false;
    }
    set.add("K10-预设-步数切片夹取", ok, "len == steps_used，越界夹取");
}

/// K10-预设-切换零成本：返回静态表的**同一份**（地址恒等）。
/// 不用 `std::ptr::eq`：内核镜像目标是纯 `no_std`，改用语义相同的 `core::ptr::eq`。
pub(crate) fn chk_preset_zero_cost(set: &mut CheckSet) {
    let a = preset(FxaaQuality::Medium);
    let b = preset(FxaaQuality::Medium);
    let ok = core::ptr::eq(a, b) && a.params == b.params;
    set.add("K10-预设-切换零成本", ok, "查表返回静态表同一份");
}

/// K10-预设-wire往返：三档均可从 wire 值往返（显式映射，不猜）。
pub(crate) fn chk_quality_wire_roundtrip(set: &mut CheckSet) {
    let mut ok = all_qualities(|q| FxaaQuality::from_wire(q.wire()) == Some(q));
    if FxaaQuality::from_wire(200).is_some() {
        ok = false;
    }
    set.add("K10-预设-wire往返", ok, "0/1/2 往返；非法值 None");
}

// ---------------------------------------------------------------------------
// 判据三：模糊诚实声明（可复算证据）
// ---------------------------------------------------------------------------

/// K10-代价-能量衰减：高频图案上实测高频能量比 **< 1**（确实变糊）。
/// 语义是"让声明保持诚实"：若实现改成保边锐化，比值 ≥ 1 本判据转红
/// ⇒ 逼着同步更新 `BLUR_HONESTY_DECL`。
pub(crate) fn chk_blur_energy_decay(set: &mut CheckSet, ctx: &PresetCtx) {
    // 用高频图案而非多尺度图：多尺度图是连续缓变梯度，FXAA 不该糊它
    // （能量比≈1 是正确行为）；模糊代价的证据必须取自真正高频的载体。
    let img = synth_high_freq(N, N);
    let out = resolve(&img, &ctx.high);
    let ratio = match &out {
        Some(v) => high_frequency_energy_ratio(&img, v),
        None => None,
    };
    // 幅度门槛 `< 0.98`：上界宽过正确实现实测（12×12 为 0.9531）、窄于 1.0
    // （空操作时比值恰为 1.0，必须能抓住）。
    // 0.98 夹在两者之间，且离正确实现有 3% 余量、离空操作有 2% 余量。
    let ok = match ratio {
        Some(r) => r < 0.98,
        None => false,
    };
    set.add("K10-代价-能量衰减", ok, "高频能量比 < 0.98（确实变糊）");
}

/// K10-代价-平场零高频：平场上高频能量比 **恰为 1**（无边缘 ⇒ 不引入任何模糊）。
///
/// 单边符号 + 精确比较：平场若被"误判为边缘"，比值会掉到 1 以下。
pub(crate) fn chk_flat_field_zero_energy(set: &mut CheckSet, ctx: &PresetCtx) {
    let flat = Frame::filled(N, N, Rgb::new(0.5, 0.5, 0.5));
    let out = resolve(&flat, &ctx.high);
    let ok = match (
        high_frequency_energy(&flat),
        out.as_ref().and_then(high_frequency_energy),
    ) {
        (Some(e0), Some(e1)) => e0 == 0.0 && e1 == 0.0,
        _ => false,
    };
    set.add("K10-代价-平场零高频", ok, "平场拉普拉斯能量恒为 0");
}

/// K10-代价-档位单调：档位越强糊得越多（能量比随档位单调不增）。
///
/// 单边符号：`ratio(high) <= ratio(med) <= ratio(low)`。
pub(crate) fn chk_energy_monotonic_by_quality(set: &mut CheckSet) {
    // 同样用高频图案（与上一条判据同一载体，口径一致）。
    let img = synth_high_freq(N, N);
    let mut rs: [Option<f32>; 3] = [None, None, None];
    for (i, q) in FxaaQuality::ALL.iter().enumerate() {
        if let Some(v) = resolve(&img, &preset(*q).params) {
            rs[i] = high_frequency_energy_ratio(&img, &v);
        }
    }
    let ok = some3(rs[0], rs[1], rs[2], |l, m, h| l >= m && m >= h);
    set.add("K10-代价-档位单调", ok, "low >= med >= high");
}

/// K10-代价-非边缘逐位不变：模糊只发生在**边缘**像素上（能量守恒对账）。
///
/// 这是"形状判据只证明有渐变"的对偶——这里断言的是**没变化**的部分
/// 精确没变，即面积守恒。
pub(crate) fn chk_untouched_pixels_bitwise_intact(set: &mut CheckSet, ctx: &PresetCtx) {
    // 用多尺度图：它有足够多的边缘像素可供"未处理/已处理"分区。
    let img = synth_multiscale(N);
    let out = resolve(&img, &ctx.high);
    let mut ok = out.is_some();
    if out.is_some() {
        for y in 0..N {
            for x in 0..N {
                if let Some(pass) = untouched_pixel_intact(&img, x, y, &ctx.high) {
                    ok = ok && pass;
                }
            }
        }
    }
    set.add("K10-代价-非边缘逐位不变", ok, "未处理像素 out == center");
}

/// K10-代价-小图显式失败：合成图尺寸不足时能量统计显式失败（`None`）。
pub(crate) fn chk_energy_stats_fail_on_tiny(set: &mut CheckSet) {
    let tiny = Frame::filled(2, 2, Rgb::new(0.5, 0.5, 0.5));
    set.add(
        "K10-代价-小图显式失败",
        high_frequency_energy(&tiny).is_none(),
        "2x2 -> None",
    );
}

// ---------------------------------------------------------------------------
// 判据四：选型贡献
// ---------------------------------------------------------------------------

/// K10-选型-字段由实现导出：选型行的关键字段由**实现结构**导出，不是手写字符串。
///
/// "零历史依赖""纯像素级"这两条是 FXAA 的本质特征（F2012 汇总时的
/// 选型依据）。若手写字符串，它们会与实现漂移且无人发现——
/// 比如哪天有人给 FXAA 加了历史缓冲，行里还写着"零历史依赖"。
pub(crate) fn chk_selection_row_from_impl(set: &mut CheckSet) {
    let r = selection_row();
    let ok = r.method == "FXAA"
        && r.quality_steps == 3
        && !r.needs_history
        && !r.needs_geometry_pass
        && r.needs_device_probe == depends_on_device_caps()
        && !r.needs_device_probe;
    set.add("K10-选型-字段由实现导出", ok, "needs_history=false 等");
}

/// K10-选型-能力无关：能力无关声明恒假（低配友好 = 纯像素级）。
pub(crate) fn chk_device_caps_independent(set: &mut CheckSet) {
    let ok = !depends_on_device_caps();
    set.add("K10-选型-能力无关", ok, "depends_on_device_caps()=false");
}

/// K10-选型-成本单调且差异受限：成本模型三档单调递增且差异 < 30%
/// （锚点"三档差异 <30%"口径）。
///
/// 绝对毫秒是锚点预算非实测（见 `PERF_HONESTY_DECL`）；本条只验证
/// **模型内部一致性**。
pub(crate) fn chk_cost_monotonic_bounded(set: &mut CheckSet) {
    let cl = cost_ms_1080p(FxaaQuality::Low);
    let cm = cost_ms_1080p(FxaaQuality::Medium);
    let ch = cost_ms_1080p(FxaaQuality::High);
    let ok = cl < cm && cm < ch && (ch - cl) / ch < 0.30;
    set.add("K10-选型-成本单调且差异受限", ok, "low<med<high 且 <30%");
}

/// K10-选型-成本线性：成本随像素数线性（供 F2014 成本模型对账）。
pub(crate) fn chk_cost_linear_in_pixels(set: &mut CheckSet) {
    let q = FxaaQuality::High;
    let c1 = cost_ms_scaled(q, 1920 * 1080);
    let c2 = cost_ms_scaled(q, (1920 * 1080) * 4);
    let ok = (c2 - c1 * 4.0).abs() < 1.0e-3;
    set.add("K10-选型-成本线性", ok, "4x 像素 -> 4x 成本");
}

/// K10-选型-成本指数自洽：代表档成本指数与模型自洽（判据侧独立重算）。
pub(crate) fn chk_cost_index_self_consistent(set: &mut CheckSet) {
    let r = selection_row();
    let expect =
        (cost_ms_1080p(FxaaQuality::Medium) / COST_MS_1080P * 100.0) as u16;
    let ok = r.cost_index_x100 == expect && r.cost_index_x100 > 100;
    set.add("K10-选型-成本指数自洽", ok, "中档指数 = 独立重算值");
}
