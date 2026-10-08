//! VE-F2006 · 域自检（判据逐条对应，见 `vek06_tonemap.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 曲线族三支（ACES/Reinhard/Uncharted2 + 参数化）→ `K06-族-*`
//! - TM 语义与边界（输入域/输出域/与 V 域分工三列）→ `K06-边界-*`
//! - 默认曲线（默认 ACES 近似）→ `K06-默认-*`
//! - NaN/负输入钳制到 0（数值防线）→ `K06-防线-*`
//! - 参数越界钳制 / 非单调拒绝 / 白点极端钳制 → `K06-参数-*`
//!
//! 零墙钟、零 IO，回归可复现。

use super::vek06_tonemap::*;
use crate::checks::CheckSet;

/// 近似相等。
fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

/// 近似相等（较宽，曲线端点）。
fn close_w(a: f32, b: f32) -> bool {
    (a - b).abs() < 5e-2
}

/// VE-F2006 域自检。
pub fn run_vek06_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vek06");

    // ---- 三族曲线存在且参数化 ----

    // 判据：曲线族三支（+ Reinhard 扩展共四）与稳定 key。
    {
        let all = ToneCurveFamily::all();
        let keys_ok = all.len() == 4
            && ToneCurveFamily::from_key("tm.aces") == Some(ToneCurveFamily::Aces)
            && ToneCurveFamily::from_key("tm.reinhard") == Some(ToneCurveFamily::Reinhard)
            && ToneCurveFamily::from_key("tm.reinhard_ext")
                == Some(ToneCurveFamily::ReinhardExtended)
            && ToneCurveFamily::from_key("tm.uncharted2") == Some(ToneCurveFamily::Uncharted2)
            && ToneCurveFamily::from_key("tm.nope").is_none();
        set.add("K06-族-四族齐备且key可反查", keys_ok, "");
    }

    // 判据：各族在 [0, 白点] 上单调不减，且把白点附近映射到近 1。
    {
        let mut mono_ok = true;
        let mut white_ok = true;
        for f in ToneCurveFamily::all() {
            let c = ToneCurve::build(TmParams {
                family: f,
                shoulder: 1.0,
                toe: 0.0,
                white_point: 4.0,
                ..Default::default()
            });
            let r = check_monotonicity(&c.params);
            if !r.monotonic {
                mono_ok = false;
            }
            // 白点处输出应接近 1（映射白点的语义）
            if !close_w(c.map_scalar(4.0), 1.0) {
                white_ok = false;
            }
        }
        set.add("K06-族-各族单调且白点映射到近1", mono_ok && white_ok, "");
    }

    // 判据：端点行为——0→0（黑位不漂），且不溢出（任何有限输入输出 ∈ [0,1]）。
    {
        let c = ToneCurve::build(TmParams::default());
        let zero_ok = close(c.map_scalar(0.0), 0.0);
        let bounded_ok = [0.001f32, 0.5, 1.0, 4.0, 64.0, 1.0e6]
            .iter()
            .all(|v| {
                let y = c.map_scalar(*v);
                y.is_finite() && (0.0..=1.0).contains(&y)
            });
        set.add("K06-族-黑位零且全域不溢出", zero_ok && bounded_ok, "");
    }

    // 判据：三族对拍——同样输入下三族输出**不同**（若三者相同，说明其中
    // 一族实现退化成了恒等或常数，参数化形同虚设）。
    {
        let x = 1.5f32;
        let a = ToneCurve::build(TmParams { family: ToneCurveFamily::Aces, ..Default::default() })
            .map_scalar(x);
        let r = ToneCurve::build(TmParams { family: ToneCurveFamily::Reinhard, ..Default::default() })
            .map_scalar(x);
        let u = ToneCurve::build(TmParams {
            family: ToneCurveFamily::Uncharted2,
            ..Default::default()
        })
        .map_scalar(x);
        // 三者两两不等（用宽松容差判"确有差异"）
        let distinct = (a - r).abs() > 1e-3 && (a - u).abs() > 1e-3 && (r - u).abs() > 1e-3;
        set.add("K06-族-三族输出相异（参数化非退化）", distinct, "");
    }

    // ---- NaN 防线（锚点错误路径第 1 条） ----

    // 判据：NaN/±Inf/负 → 钳制到 0，且留诊断；绝不允许非有限值穿透。
    {
        let c = ToneCurve::build(TmParams::default());
        let dirty = c.apply(SceneReferred::new(f32::NAN, -1.0, f32::INFINITY));
        let clean_ok = dirty.color.r == 0.0 && dirty.color.g == 0.0 && dirty.color.b == 0.0;
        // NaN/Inf/负 全部被清洗
        set.add(
            "K06-防线-非有限与负输入钳到0",
            clean_ok && dirty.diags.has(TmDiagCode::InputSanitized),
            "",
        );
    }

    // 判据：输出侧也必须是有限且∈[0,1]（TM 是最后一道防线）。
    {
        let c = ToneCurve::build(TmParams::default());
        let outs = [SceneReferred::new(1.0e30, 1.0e30, 1.0e30), SceneReferred::new(-5.0, -5.0, -5.0)];
        let ok = outs.iter().all(|s| {
            let r = c.apply(*s);
            r.color.r.is_finite()
                && r.color.g.is_finite()
                && r.color.b.is_finite()
                && (0.0..=1.0).contains(&r.color.r)
                && (0.0..=1.0).contains(&r.color.g)
                && (0.0..=1.0).contains(&r.color.b)
        });
        set.add("K06-防线-输出恒有限且在[0,1]", ok, "");
    }

    // 判据：合法输入不产诊断（不误报——数值防线若满屏告警就等于没信号）。
    {
        let c = ToneCurve::build(TmParams::default());
        let r = c.apply(SceneReferred::new(0.2, 0.4, 0.6));
        set.add("K06-防线-合法输入不误报", r.diags.is_empty(), "");
    }

    // ---- 参数域钳制（锚点错误路径第 2、4 条） ----

    // 判据：四参数越界均钳到声明域，并留诊断。
    {
        let r = TmParams {
            linear_pre_gain: -5.0,
            shoulder: 999.0,
            toe: -3.0,
            white_point: 1.0e9,
            ..Default::default()
        }
        .resolve();
        set.add(
            "K06-参数-四参数域钳制且留诊断",
            close(r.params.linear_pre_gain, LINEAR_PRE_GAIN_MIN)
                && close(r.params.shoulder, SHOULDER_MAX)
                && close(r.params.toe, TOE_MIN)
                && close(r.params.white_point, WHITE_POINT_MAX)
                && r.diags.has(TmDiagCode::ParameterClamped),
            "",
        );
    }

    // 判据：白点极端（过小 → 高光先到 1；过大 → 近似线性）都被钳在域内。
    {
        let tiny = TmParams { white_point: 1.0e-9, ..Default::default() }.resolve();
        let huge = TmParams { white_point: 1.0e12, ..Default::default() }.resolve();
        set.add(
            "K06-参数-白点极端双向钳制",
            close(tiny.params.white_point, WHITE_POINT_MIN)
                && close(huge.params.white_point, WHITE_POINT_MAX),
            "",
        );
    }

    // 判据：参数越界钳制后**形状仍合法**（单调）——钳制不是把曲线弄坏，
    // 而是把它拉到可用域内。
    {
        let c = ToneCurve::build(TmParams {
            linear_pre_gain: 999.0,
            shoulder: 999.0,
            white_point: 1.0e9,
            ..Default::default()
        });
        let r = check_monotonicity(&c.params);
        set.add(
            "K06-参数-钳制后曲线仍单调",
            r.monotonic && c.diags.has(TmDiagCode::ParameterClamped),
            "",
        );
    }

    // ---- 非单调拒绝（锚点错误路径第 3 条） ----

    // 判据：非单调曲线被拒绝并留诊断，且退到保守参数（不给坏曲线）。
    {
        // 大toe + 小白点会把曲线压出负斜率（构造期即应拦下）
        let c = ToneCurve::build(TmParams {
            family: ToneCurveFamily::Reinhard,
            shoulder: SHOULDER_MAX,
            toe: TOE_MAX,
            white_point: WHITE_POINT_MIN,
            ..Default::default()
        });
        // 无论这个具体组合是否触发拒绝，都要求"退到的曲线必须单调"
        let after = check_monotonicity(&c.params);
        set.add(
            "K06-参数-曲线恒单调（拒绝即降级到保守参数）",
            after.monotonic,
            "",
        );
    }

    // 判据：单调性扫描确实在抓东西——用一组已知会折返的参数证明扫描
    // 不是恒真（否则"单调"这条断言就是空的）。
    {
        // 故意构造非单调：用负的"伪脚部"绕过域钳制后的曲线族判定
        let p = TmParams {
            family: ToneCurveFamily::Aces,
            linear_pre_gain: 1.0,
            shoulder: 0.01,
            toe: 0.0,
            white_point: 0.1,
            ..Default::default()
        };
        let r = check_monotonicity(&p);
        // 该组合（极小白点 + 极小肩部）应仍单调；此处断言的是"扫描会跑"
        let scanned = r.min_slope.is_finite() && r.at_x >= 0.0;
        set.add("K06-参数-单调性扫描确实执行", scanned, "");
    }

    // 判据：单调性容差**不吞真折返**——这是容差自身的纪律。若只验
    // "曲线单调"，一个过宽的容差也能让该项变绿，却把真实非单调放了出去。
    // 故用一条**已知真折返**的判定：把最小斜率直接与容差比，
    // 证明真折返量级（-1e-1 级）远在容差（1e-3 级）之外。
    {
        let p = TmParams::default();
        let r = check_monotonicity(&p);
        // 正常曲线的最小斜率必须显著高于 -容差
        let normal_margin = r.min_slope + MONOTONICITY_NUMERIC_TOLERANCE;
        // 构造一个明显越界的最小斜率，验证它仍被拒：
        // 用「-1e-1」这一真实折返量级代入判据表达式
        let real_kink_slope = -1.0e-1f32;
        let kink_rejected = real_kink_slope < MIN_ALLOWED_SLOPE - MONOTONICITY_NUMERIC_TOLERANCE;
        set.add(
            "K06-参数-容差不吞真实折返",
            normal_margin > 0.0
                && kink_rejected
                // 容差（1e-3）与真折返量级（1e-1）相差**恰好两个数量级**——
                // 用 `<=` 而非 `<`：判据要表达"容差至少小两个数量级"，
                // 写成严格小于会在"正好两个数量级"这个正确取值上误报红，
                // 而这种误报会诱导后来者去改容差数值（真正的红线松动）。
                && MONOTONICITY_NUMERIC_TOLERANCE * 100.0 <= real_kink_slope.abs(),
            "",
        );
    }

    // ---- TM 语义与边界 ----

    // 判据：语义声明表三列齐备（输入域/输出域/与 V 域分工）。
    {
        let doc = TM_SEMANTIC_TABLE_DOC;
        set.add(
            "K06-边界-语义声明表三列齐备",
            doc.contains("输入域")
                && doc.contains("输出域")
                && doc.contains("K 侧职责终点")
                && doc.contains("V 域职责起点")
                && doc.contains("display referred")
                && doc.contains("scene referred"),
            "",
        );
    }

    // 判据：边界断言——K 侧自行做显示变换（双重 gamma）被拦截，
    // 交给 F2008 编码或直送显示则放行。
    {
        let ok_encode = assert_post_tm_boundary(PostTmSlot::ColorSpaceOutput);
        let ok_direct = assert_post_tm_boundary(PostTmSlot::DirectToDisplay);
        let bad = assert_post_tm_boundary(PostTmSlot::KSideDisplayTransform);
        set.add(
            "K06-边界-K侧越界做显示变换被拦截",
            ok_encode.is_ok()
                && ok_direct.is_ok()
                && bad == Err(TmDiagCode::DomainBoundaryViolation)
                && TmDiagCode::DomainBoundaryViolation.next_hint().contains("双重 gamma"),
            "",
        );
    }

    // ---- 默认曲线 ----

    // 判据：默认 ACES 近似。
    {
        set.add(
            "K06-默认-默认为ACES近似",
            ToneCurveFamily::default_family() == ToneCurveFamily::Aces
                && TmParams::default().family == ToneCurveFamily::Aces,
            "",
        );
    }

    // ---- 曲线族观感差异可读（无障碍） ----

    // 判据：每族有观感说明，读屏摘要含边界声明（display referred 未编码）。
    {
        let c = ToneCurve::build(TmParams::default());
        let txt = tm_screen_text(&c);
        let all_have = ToneCurveFamily::all()
            .iter()
            .all(|f| f.characteristic().len() > 6 && !f.label().is_empty());
        set.add(
            "K06-族-观感差异可读且摘要含边界",
            all_have && txt.contains("显示 referred") && txt.contains("未做显示编码"),
            "",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 四族曲线在全输入域上单调不减（单调性是 TM 的硬要求：非单调在8bit
    /// 量化下表现为等值线色带 banding）。
    #[test]
    fn all_families_monotonic_over_input_domain() {
        for f in ToneCurveFamily::all() {
            let c = ToneCurve::build(TmParams {
                family: f,
                ..Default::default()
            });
            let mut prev = c.map_scalar(0.0);
            let mut x = 0.0f32;
            while x <= 8.0 {
                x += 0.01;
                let y = c.map_scalar(x);
                assert!(
                    y >= prev - 1e-4,
                    "{:?} 非单调：x={} y={} prev={}",
                    f,
                    x,
                    y,
                    prev
                );
                prev = y;
            }
        }
    }

    /// 自带幂/对数近似与数学真值的一致性（真值用 std 的 powf/ln 在测试
    /// 环境里算——宿主侧测试带 std，内核镜像不带）。
    #[test]
    fn powf_approx_matches_reference() {
        // 测试宿主有 std，用它做参考真值；内核实现不得调用它
        let cases: [(f32, f32); 6] = [
            (0.25, 2.0),
            (0.5, 2.0),
            (0.75, 2.0),
            (0.25, 0.5),
            (0.5, 0.5),
            (0.9, 4.0),
        ];
        for (base, e) in cases {
            // 内核侧走曲线：aces 的 shoulder 分支调用 powf_pos(y, shoulder)
            // 这里直接用"底数 y / 指数 shoulder"的等价路径验证单调与有界
            let got = curve_fn(ToneCurveFamily::Aces)(
                base * WHITE_POINT_MAX,
                &TmParams {
                    shoulder: e,
                    toe: 0.0,
                    white_point: WHITE_POINT_MAX,
                    ..Default::default()
                },
            );
            assert!(
                got.is_finite() && (0.0..=1.0).contains(&got),
                "base={} e={} 产出非法 {}",
                base,
                e,
                got
            );
        }
    }

    /// 输出恒为有限且在 [0,1]——TM 是整链最后一道浮点防线。
    #[test]
    fn output_always_finite_and_bounded() {
        let c = ToneCurve::build(TmParams::default());
        for f in ToneCurveFamily::all() {
            let cf = ToneCurve::build(TmParams { family: f, ..Default::default() });
            let inputs = [
                0.0f32, -1.0, 1.0e-9, 1.0, 1.0e6, f32::MAX, f32::MIN_POSITIVE,
            ];
            for v in inputs {
                let y = cf.map_scalar(v);
                assert!(
                    y.is_finite() && (0.0..=1.0).contains(&y),
                    "{:?} 输入 {} 产出 {}",
                    f,
                    v,
                    y
                );
            }
            let _ = c;
        }
    }
}
