//! VE-F2010 · 域自检（判据逐条对应 `vek10_fxaa.rs` 头注；判据映射见 `CRITERIA_MAP`，
//! 反假变体登记表见 `NEGATIVE_VARIANTS`——两表内容自原模块头注原样迁入，零删减）。
//!
//! **本文件的核心纪律：反向变体验证（bidirectional variant testing）**——只验"基线
//! 全绿"的门禁等于没验——判据可能根本没测到东西。本文件对**每条关键判据**都配一个
//! "如果实现写错会怎样"的**反假变体**（假实现），断言真判据在假实现上**必须转红**。
//! 变体不写进生产代码，只在本文件内以局部闭包重现"错误写法"，跑完即弃（登记表见
//! `NEGATIVE_VARIANTS`）。
//!
//! **另外三条自律**：① **参考值独立重算**：判据里的期望值（如高频能量比、PS 步序列）
//! 由本文件**独立算一遍**，不调被测函数现算的值当答案；② **精确比较优先于阈值比较**：
//! 能用 `== 0.0` / `==` 就不用 `abs() < eps`；③ **拒绝路径也要测**：尺寸不足/长度不匹配/
//! 非法 wire 值/NaN 参数——只测 happy path 的门禁等于没测。
//!
//! 零墙钟、零 IO，回归可复现。`run_vek10_checks` 只做判据编排，每条判据的核算逻辑
//! 放在同文件私有 `chk_*` 函数中（单一职责）；三档预设参数经 `PresetCtx` 上下文对象
//! 收敛签名。判据名与断言语义保持不变。

/// 判据映射（锚点原文 → 自检族，自原头注迁入，逐条对应不可增删）。
#[allow(dead_code)]
pub const CRITERIA_MAP: [(&str, &str); 7] = [
    ("单 pass", "K10-单pass-*"),
    ("三预设", "K10-预设-*"),
    ("模糊诚实声明", "K10-代价-*"),
    ("选型贡献", "K10-选型-*"),
    ("错误路径四条（序位守卫/参数钳制/低配推荐/CAS 联动）", "K10-错误-*"),
    ("错误三要素（码/现象/原因/建议）", "K10-三要素-*"),
    ("无障碍声明", "K10-无障碍-*"),
];

/// 反假变体登记表（自原头注迁入）：变体 → 模拟的实现错误 → 被哪条判据抓住。
#[allow(dead_code)]
pub const NEGATIVE_VARIANTS: [(&str, &str, &str); 8] = [
    ("V1", "`edge_min` 早退写成闭区间 `|Δ| <= t`", "K10-边界-早退半开"),
    ("V2", "最近邻取整代替双线性（FXAA 空操作）", "K10-单pass-确有边缘被处理"),
    ("V3", "末端插 `mix(..., 0.5)` 制造「自然过渡」", "K10-闭包-输出取三值之一"),
    ("V4", "`dir.x`/`dir.y` 坐标轴对调", "K10-方向-垂直边缘y精确零"),
    ("V5", "三档 `edge_min` 顺序写反", "K10-预设-集合包含关系"),
    ("V6", "`lumaB < lumaM` 写成 `<=`", "K10-选择-严格小于"),
    ("V7", "`NaN` 输入不兜底（静默黑屏）", "K10-错误-NaN不静默黑屏"),
    ("V8", "序位守卫只查域不查位置", "K10-错误-序位三违规全覆盖"),
];
use super::vek10_fxaa::*;
use crate::checks::CheckSet;

mod synth;
mod verdicts1;
mod verdicts2;

pub(crate) use synth::*;
pub(crate) use verdicts1::*;
pub(crate) use verdicts2::*;

/// VE-F2010 域自检。
pub fn run_vek10_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vek10");
    let ctx = PresetCtx {
        high: preset(FxaaQuality::High).params,
        med: preset(FxaaQuality::Medium).params,
        low: preset(FxaaQuality::Low).params,
    };

    // =======================================================================
    // 判据一：单 pass
    // =======================================================================
    chk_flat_field_identity(&mut set, &ctx);
    chk_edge_actually_processed(&mut set, &ctx);
    chk_output_closed_set(&mut set, &ctx);
    chk_transpose_invariance(&mut set, &ctx);
    chk_mirror_invariance(&mut set, &ctx);
    chk_no_history_dependency(&mut set, &ctx);
    chk_undersized_frame_fails(&mut set, &ctx);
    chk_len_mismatch_fails(&mut set, &ctx);

    // =======================================================================
    // 判据二：三预设
    // =======================================================================
    chk_edge_min_strictly_decreasing(&mut set, &ctx);
    chk_span_max_strictly_increasing(&mut set, &ctx);
    chk_steps_used_strictly_increasing(&mut set, &ctx);
    chk_touched_set_inclusion(&mut set, &ctx);
    chk_touched_counts_increasing(&mut set, &ctx);
    chk_ps_sequence_independent(&mut set);
    chk_active_steps_clamped(&mut set);
    chk_preset_zero_cost(&mut set);
    chk_quality_wire_roundtrip(&mut set);

    // =======================================================================
    // 判据三：模糊诚实声明（可复算证据）
    // =======================================================================
    chk_blur_energy_decay(&mut set, &ctx);
    chk_flat_field_zero_energy(&mut set, &ctx);
    chk_energy_monotonic_by_quality(&mut set);
    chk_untouched_pixels_bitwise_intact(&mut set, &ctx);
    chk_energy_stats_fail_on_tiny(&mut set);

    // =======================================================================
    // 判据四：选型贡献
    // =======================================================================
    chk_selection_row_from_impl(&mut set);
    chk_device_caps_independent(&mut set);
    chk_cost_monotonic_bounded(&mut set);
    chk_cost_linear_in_pixels(&mut set);
    chk_cost_index_self_consistent(&mut set);

    // =======================================================================
    // 错误路径一：序位守卫
    // =======================================================================
    chk_hdr_input_intercepted(&mut set);
    chk_before_tonemap_intercepted(&mut set);
    chk_after_output_encode_intercepted(&mut set);
    chk_legal_order_not_flagged(&mut set);
    chk_order_violations_full_coverage(&mut set);
    chk_pipeline_order_sane(&mut set);
    chk_domain_wires_unique(&mut set);

    // =======================================================================
    // 错误路径二：参数钳制
    // =======================================================================
    chk_extreme_params_clamped(&mut set);
    chk_legal_params_not_clamped(&mut set);
    chk_reduce_min_positive_after_clamp(&mut set);
    chk_zero_direction_finite(&mut set);
    chk_direction_clamped_by_span_max(&mut set);
    chk_frame_sanitize(&mut set);
    chk_nan_not_silent_black(&mut set, &ctx);
    chk_reversed_range_no_panic(&mut set);

    // =======================================================================
    // 错误路径三：低配推荐（非强制）
    // =======================================================================
    chk_low_budget_suggests_fxaa(&mut set);
    chk_high_budget_suggests_combo(&mut set);
    chk_taa_on_no_fxaa(&mut set);
    chk_insufficient_budget_none(&mut set);
    chk_degrade_branch_reachable(&mut set);

    // =======================================================================
    // 错误路径四：CAS 联动
    // =======================================================================
    chk_cas_coordination_warns(&mut set);
    chk_high_cas_peak_zero(&mut set);

    // =======================================================================
    // 判据五：方向正交（解析律）
    // =======================================================================
    chk_vertical_edge_dir_x_zero(&mut set, &ctx);
    chk_horizontal_edge_dir_y_zero(&mut set, &ctx);
    chk_diagonal_edge_both_nonzero(&mut set, &ctx);
    chk_direction_len_within_span(&mut set, &ctx);
    chk_direction_not_touching_boundary(&mut set, &ctx);

    // =======================================================================
    // 判据六：边界与选择（半开区间 / 严格小于）
    // =======================================================================
    chk_early_exit_half_open(&mut set);
    chk_below_threshold_early_exit(&mut set);
    chk_strict_less_selection(&mut set);
    chk_below_min_tap_a(&mut set);
    chk_above_m_tap_b(&mut set);

    // =======================================================================
    // 判据七：无障碍与声明
    // =======================================================================
    chk_a11y_registered(&mut set);
    chk_three_decls_non_empty(&mut set);
    chk_preset_params_serializable(&mut set);
    chk_fixed_format(&mut set);
    chk_rgb_basic_ops(&mut set);
    chk_rgb_saturate(&mut set);
    chk_rgb_is_finite(&mut set);

    // =======================================================================
    // 判据九：双线性权重解析律（堵V2 退化中点）
    // =======================================================================
    chk_bilinear_weights_vary(&mut set);

    // =======================================================================
    // 判据十：归一化后**不二次钳制**（堵 V5 归一化后再钳）
    // =======================================================================
    chk_no_second_clamp_after_normalize(&mut set);

    // =======================================================================
    // 判据八：自检集自身健康（防截断丢红）
    // =======================================================================
    chk_selfcheck_capacity(&mut set);

    set
}

// ===========================================================================
// 单元测试（宿主侧 `cargo test` 直跑）
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 数红项（`red_items()` 的第二值是条目总数，不是红项数）。
    fn red_count(set: &CheckSet) -> usize {
        let (table, count) = set.red_items();
        let mut n = 0usize;
        for i in 0..count {
            if let Some(c) = table.get(i).copied().flatten() {
                if !c.passed {
                    n += 1;
                }
            }
        }
        n
    }

    /// 自检集必须零红项且未截断。
    #[test]
    fn domain_selfcheck_all_green() {
        let set = run_vek10_checks();
        assert_eq!(red_count(&set), 0, "VE-F2010 域自检存在红项");
        assert!(!set.truncated(), "自检集溢出 MAX_CHECKS，结果被静默丢弃");
    }

    /// 平场恒等：整幅同色时输出逐位不变。
    #[test]
    fn flat_field_is_identity() {
        let f = Frame::filled(8, 8, Rgb::new(0.25, 0.5, 0.75));
        let out = resolve(&f, &preset(FxaaQuality::High).params).expect("平场应可解算");
        assert_eq!(out.px, f.px, "平场被改动了");
    }

    /// 输出取值集闭包：逐位等于 {中心, A, B} 之一。
    #[test]
    fn output_is_in_closed_set() {
        let img = synth_multiscale(12);
        let p = preset(FxaaQuality::High).params;
        for y in 0..12u32 {
            for x in 0..12u32 {
                let px = resolve_pixel(&img, x, y, &p).expect("应可解算");
                let center = img.get_clamped(x as i32, y as i32).expect("texel 应存在");
                assert!(
                    px.out == center || px.source == FxaaSource::TapA || px.source == FxaaSource::TapB,
                    "({x},{y}) 输出既非中心也非 A/B"
                );
                if !px.touched {
                    assert_eq!(px.out, center, "({x},{y}) 未处理却改变了输出");
                }
            }
        }
    }

    /// 三档 touched 集合严格包含：S(low) ⊊ S(med) ⊊ S(high)。
    #[test]
    fn preset_sets_strictly_nested() {
        let img = synth_multiscale(12);
        let lo = touched_set(&img, &preset(FxaaQuality::Low).params).expect("应可统计");
        let me = touched_set(&img, &preset(FxaaQuality::Medium).params).expect("应可统计");
        let hi = touched_set(&img, &preset(FxaaQuality::High).params).expect("应可统计");
        for i in 0..lo.len() {
            if lo[i] {
                assert!(me[i], "S(low) 不是 S(med) 的子集，位置 {i}");
                assert!(hi[i], "S(low) 不是 S(high) 的子集，位置 {i}");
            }
        }
        for i in 0..me.len() {
            if me[i] {
                assert!(hi[i], "S(med) 不是 S(high) 的子集，位置 {i}");
            }
        }
        let cl = lo.iter().filter(|b| **b).count();
        let cm = me.iter().filter(|b| **b).count();
        let ch = hi.iter().filter(|b| **b).count();
        assert!(cl < cm && cm < ch, "三档处理像素数未递增：{cl}/{cm}/{ch}");
    }

    /// 垂直边缘：`dir.x` 精确为 0（参考实现的 x 度量上下差）。
    #[test]
    fn vertical_edge_zeroes_dir_x() {
        let img = synth_vertical_edge(12);
        let p = preset(FxaaQuality::High).params;
        let t = sample_luma_diagonal(&img, 5, 6).expect("四邻域应可取");
        let d = solve_direction(&t, &p);
        assert_eq!(d.x, 0.0, "垂直边缘的 dir.x 应精确为 0，实为 {}", d.x);
        assert_ne!(d.y, 0.0, "垂直边缘的 dir.y 不应为 0");
    }

    /// 水平边缘：`dir.y` 精确为 0。
    #[test]
    fn horizontal_edge_zeroes_dir_y() {
        let img = synth_horizontal_edge(12);
        let p = preset(FxaaQuality::High).params;
        let t = sample_luma_diagonal(&img, 6, 5).expect("四邻域应可取");
        let d = solve_direction(&t, &p);
        assert_eq!(d.y, 0.0, "水平边缘的 dir.y 应精确为 0，实为 {}", d.y);
        assert_ne!(d.x, 0.0, "水平边缘的 dir.x 不应为 0");
    }

    /// 模糊代价可复算：高频图案上能量比显著小于 1。
    #[test]
    fn blur_cost_is_reproducible() {
        let img = synth_high_freq(16, 16);
        let out = resolve(&img, &preset(FxaaQuality::High).params).expect("应可解算");
        let r = high_frequency_energy_ratio(&img, &out).expect("应可统计");
        assert!(r < 0.98, "高频能量比 {r} 未体现模糊代价");
    }

    /// FXAA 确实生效（多尺度图上必有像素改变）。
    #[test]
    fn fxaa_actually_changes_pixels() {
        let img = synth_multiscale(12);
        let out = resolve(&img, &preset(FxaaQuality::High).params).expect("应可解算");
        let n = out.px.iter().zip(img.px.iter()).filter(|(a, b)| a != b).count();
        assert!(n > 0, "多尺度图上没有任何像素被改变——FXAA 退化为空操作");
    }

    /// 序位守卫：TM 前 HDR 输入必被拦截。
    #[test]
    fn hdr_input_is_rejected() {
        assert_eq!(
            guard_domain_order(SignalDomain::LinearHdr, PipelineStage::Fxaa),
            Some(OrderViolation::NonLdrInput)
        );
    }

    /// 参数钳制：`NaN`/`inf`/越界全部兜底。
    #[test]
    fn sanitize_clamps_extremes() {
        let mut p = preset(FxaaQuality::High).params;
        p.edge_min = f32::NAN;
        p.span_max = -5.0;
        p.steps_used = 99;
        let r = sanitize_params(&p);
        assert!(r.clamped);
        assert!(r.params.edge_min.is_finite());
        assert!(r.params.span_max >= 1.0);
        assert!(r.params.steps_used as usize <= FXAA_PS_MAX);
    }

    /// `NaN` 像素不导致静默黑屏：输出恒有限。
    #[test]
    fn nan_input_never_yields_nan_output() {
        let mut img = synth_multiscale(12);
        img.px[0] = Rgb::new(f32::NAN, f32::INFINITY, f32::NEG_INFINITY);
        let out = resolve(&img, &preset(FxaaQuality::High).params).expect("应可解算");
        for (i, c) in out.px.iter().enumerate() {
            assert!(c.is_finite(), "第 {i} 个像素输出非有限：{c:?}");
        }
    }

    /// 尺寸不足显式失败，不返回中心像素。
    #[test]
    fn too_small_frame_fails_loudly() {
        let tiny = Frame::filled(1, 1, Rgb::new(0.5, 0.5, 0.5));
        assert!(resolve(&tiny, &preset(FxaaQuality::High).params).is_none());
    }

    /// 转置不变（f32 容差 1e-6）。
    #[test]
    fn transpose_invariance() {
        const TOL: f32 = 1.0e-6;
        let img = synth_multiscale(12);
        let p = preset(FxaaQuality::High).params;
        let a = resolve(&img, &p).expect("应可解算");
        let b = resolve(&transpose(&img), &p).expect("应可解算");
        let t = transpose(&a);
        for (i, (x, y)) in t.px.iter().zip(b.px.iter()).enumerate() {
            let d = (x.r - y.r).abs().max((x.g - y.g).abs()).max((x.b - y.b).abs());
            assert!(d <= TOL, "第 {i} 个像素转置偏差 {d} 超过容差");
        }
    }
}
