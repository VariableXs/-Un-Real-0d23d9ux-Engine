//! VE-F2403 · 域自检（判据逐条对应，见 `vem03_interp.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **四插值器** → `M03-四插值器-注册齐备`、`M03-阶梯-无中间态`、
//!   `M03-线性-端点精确与公式稳定`、`M03-缓动-契约与回退`；
//! - **可插拔注册** → `M03-注册-重名拒绝不覆盖`、`M03-注册-自定义可调用`、
//!   `M03-注册-覆盖面对齐F2402`、`M03-注册-零成本直路径`；
//! - **纯函数确定性** → `M03-确定性-双跑逐位一致`、`M03-确定性-口径写死不含糊`；
//! - **贝塞尔纪律** → `M03-贝塞尔-端点严格`、`M03-贝塞尔-控制点钳制与乱序拒绝`、
//!   `M03-贝塞尔-无幂运算近1精度`、`M03-贝塞尔-超调允许`；
//! - 错误路径 → `M03-错误-布尔配连续拒绝`、`M03-错误-NaN钳制不传染`、
//!   `M03-错误-禁外推钳制`、`M03-错误-零长区间不NaN`；
//! - 旋转接口位 → `M03-旋转-slerp双覆盖短弧`、`M03-旋转-结果恒单位化`；
//! - 鲁棒面 → `M03-鲁棒-敌意输入不panic`。
//!
//! 逻辑 tick 注入、零墙钟，回归可复现。

use super::vem02_track::{DiagBag, TrackClass};
use super::vem03_interp::*;
use crate::checks::CheckSet;

/// VE-F2403 域自检。
pub fn run_vem03_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vem03");

    // ---- 四插值器（判据一） ----

    {
        let r = InterpRegistry::new();
        let mut all_present = true;
        for (n, _) in Interp::BASE_NAMES.iter() {
            if r.lookup(n).is_none() {
                all_present = false;
            }
        }
        set.add(
            "M03-四插值器-注册齐备",
            Interp::BASE_NAMES.len() == 4
                && all_present
                && r.len() == 4
                && Interp::Step.is_continuous() == false
                && Interp::Linear.is_continuous()
                && Interp::CubicBezier.is_continuous()
                && Interp::Eased.is_continuous(),
            "",
        );
    }

    {
        // 阶梯语义：`u < 1` 取前值（不是 `u < 0.5`——那会提前半程跳变）。
        let mut ok = true;
        for u in [0.0f32, 0.25, 0.4999, 0.5, 0.9999] {
            ok &= interp_step_scalar(3.0, 7.0, u) == 3.0;
        }
        ok &= interp_step_scalar(3.0, 7.0, 1.0) == 7.0;
        ok &= interp_step_scalar(3.0, 7.0, 2.0) == 7.0;
        // 三/四分量/布尔同步。
        ok &= interp_step_vec3([1.0; 3], [2.0; 3], 0.9) == [1.0; 3];
        ok &= interp_step_vec4([1.0; 4], [2.0; 4], 1.0) == [2.0; 4];
        ok &= interp_step_bool(true, false, 0.9) == true;
        ok &= interp_step_bool(true, false, 1.0) == false;
        // 恒无中间态：任意 u 只能取两端值之一。
        let mut no_mid = true;
        let mut i = 0u32;
        while i <= 100 {
            let u = i as f32 / 100.0;
            let v = interp_step_scalar(10.0, 20.0, u);
            no_mid &= v == 10.0 || v == 20.0;
            i += 1;
        }
        set.add("M03-阶梯-无中间态", ok && no_mid, "");
    }

    {
        // 线性：端点精确 + 增量式公式在大幅值下不抵消。
        let mut ok = lerp_scalar(0.0, 10.0, 0.0) == 0.0
            && lerp_scalar(0.0, 10.0, 1.0) == 10.0
            && lerp_scalar(0.0, 10.0, 0.5) == 5.0
            && lerp_vec3([1.0, 2.0, 3.0], [4.0, 5.0, 6.0], 0.0) == [1.0, 2.0, 3.0]
            && lerp_vec4([0.0; 4], [2.0; 4], 1.0) == [2.0; 4];
        // 1e6 量级（ulp≈0.0625，可精确表示端点差）验证精度。
        let big = lerp_scalar(1.0e6, 1.0e6 + 10.0, 0.5);
        ok &= (big - (1.0e6 + 5.0)).abs() < 0.05;
        set.add("M03-线性-端点精确与公式稳定", ok, "");
    }

    {
        // 缓动：契约全合规 + 未命中回退 + 端点严格。
        let contract_clean = audit_easing_contract().is_empty();
        let endpoints = easing_linear(0.0) == 0.0
            && easing_linear(1.0) == 1.0
            && easing_smoothstep(0.0) == 0.0
            && easing_smoothstep(1.0) == 1.0;
        let fallback = lookup_easing("not-ready-yet").is_none();
        let (idx, ok) = easing_selector_index("nope");
        // 不合规函数须被契约拒（端点漂移）。
        fn drifting(u: f32) -> f32 {
            u * 0.9
        }
        let rejects_bad = !easing_contract_holds(drifting as EasingFn);
        set.add(
            "M03-缓动-契约与回退",
            contract_clean && endpoints && fallback && !ok && idx == 0 && rejects_bad,
            "",
        );
    }

    // ---- 可插拔注册（判据二） ----

    {
        let mut r = InterpRegistry::new();
        let mut bag = DiagBag::new();
        // 签名须为 `fn(&InterpParams, f32) -> f32`（`ScalarEvalFn`）；
        // `EasingFn` 是 `fn(f32) -> f32`，与注册表槽位不同型，不能直接塞。
        fn probe_dummy(_p: &InterpParams, u: f32) -> f32 {
            u * 0.9
        }
        // 重名拒绝：不覆盖已有实现（覆盖会让旧引用静默改语义）。
        let dup = r.register("linear", Interp::Custom, probe_dummy as ScalarEvalFn, &mut bag);
        let empty_name = r.register("", Interp::Custom, probe_dummy as ScalarEvalFn, &mut bag);
        // 未覆盖：原 linear 仍是原实现（返回 u 而非 u*0.9）。
        let still_original = match r.lookup("linear") {
            Some(e) => (e.eval)(&InterpParams::LINEAR, 0.5) == 0.5,
            None => false,
        };
        set.add(
            "M03-注册-重名拒绝不覆盖",
            !dup && !empty_name && still_original && r.len() == 4 && !bag.errors().is_empty(),
            "",
        );
    }

    {
        // 自定义插值器注册后可经查表与直路径两条路求值，且结果一致。
        fn ease_out_cubic(_p: &InterpParams, u: f32) -> f32 {
            let inv = 1.0 - u;
            1.0 - inv * inv * inv
        }
        let mut r = InterpRegistry::new();
        let mut bag = DiagBag::new();
        let registered = r.register(
            "ease-out-cubic",
            Interp::Custom,
            ease_out_cubic as ScalarEvalFn,
            &mut bag,
        );
        let e = match r.lookup("ease-out-cubic") {
            Some(x) => *x,
            None => {
                set.add("M03-注册-自定义可调用", false, "");
                return finish(set);
            }
        };
        let p = InterpParams::LINEAR;
        let via_table = r.eval_named("ease-out-cubic", &p, 0.3);
        let direct = ease_out_cubic(&p, 0.3);
        // 端点与形状正确。
        let endpoints = (e.eval)(&p, 0.0) == 0.0 && ((e.eval)(&p, 1.0) - 1.0).abs() < 1e-6;
        let shape = (e.eval)(&p, 0.5) > 0.8;
        // 未知名显式拒绝（None），不静默用默认值。
        let unknown_rejected = r.eval_named("ghost", &p, 0.5).is_none();
        set.add(
            "M03-注册-自定义可调用",
            registered
                && endpoints
                && shape
                && unknown_rejected
                && via_table.map(|v| v.to_bits()) == Some(direct.to_bits()),
            "",
        );
    }

    {
        // 覆盖面接缝：注册面须与 F2402 六类规格表一致（无空洞、无死实现）。
        let r = InterpRegistry::new();
        let problems = audit_interp_coverage(&r);
        set.add("M03-注册-覆盖面对齐F2402", problems.is_empty(), "");
    }

    {
        // 零成本直路径：已知名不经查表即可求值，且与查表结果逐位一致。
        let r = InterpRegistry::new();
        let p = InterpParams::LINEAR;
        let mut same = true;
        for (n, _) in Interp::BASE_NAMES.iter() {
            let a = r.eval_named(n, &p, 0.37);
            let b = InterpRegistry::eval_named_known(n, &p, 0.37);
            same &= a.map(|v| v.to_bits()) == b.map(|v| v.to_bits());
        }
        let unknown_none = InterpRegistry::eval_named_known("ghost", &p, 0.37).is_none();
        set.add("M03-注册-零成本直路径", same && unknown_none, "");
    }

    // ---- 纯函数确定性（判据三） ----

    {
        let rep = double_run_determinism(64);
        set.add(
            "M03-确定性-双跑逐位一致",
            rep.bitwise_identical
                && rep.first_mismatch.is_none()
                && rep.compared >= 64 * 6,
            "",
        );
    }

    {
        // 口径必须写死：同平台逐位 / 跨平台不承诺。不许含糊成「尽量一致」。
        let explicit = DETERMINISM_POLICY.contains("同平台逐位")
            && DETERMINISM_POLICY.contains("不承诺跨平台");
        let tolerances = SAME_PLATFORM_TOLERANCE == 0.0 && CROSS_PLATFORM_TOLERANCE > 0.0;
        set.add("M03-确定性-口径写死不含糊", explicit && tolerances, "");
    }

    // ---- 贝塞尔纪律（判据四） ----

    {
        // 端点严格：u=0/1 精确返回 0/1，不经任何乘加。
        let mut ok = bezier_axis(0.0, 0.42, 0.58) == 0.0
            && bezier_axis(1.0, 0.42, 0.58) == 1.0
            && bezier_axis(-1.0, 0.42, 0.58) == 0.0
            && bezier_axis(2.0, 0.42, 0.58) == 1.0;
        // 对称控制点中点 ≈ 0.5（CSS ease 语义）。
        ok &= (bezier_axis(0.5, 0.42, 0.58) - 0.5).abs() < 1e-3;
        // 端点经总入口仍精确（关键帧值逐位相等）。
        let mut bag = DiagBag::new();
        let r = InterpRegistry::new();
        let e = *r.lookup("cubic-bezier").unwrap();
        let p = InterpParams::bezier(0.42, 0.0, 0.58, 1.0, &mut bag).unwrap();
        ok &= eval_scalar_span(TrackClass::Float, &e, &p, 7.0, 9.0, 0.0, 10.0, 0.0, &mut bag)
            == Some(7.0);
        ok &= eval_scalar_span(TrackClass::Float, &e, &p, 7.0, 9.0, 0.0, 10.0, 10.0, &mut bag)
            == Some(9.0);
        set.add("M03-贝塞尔-端点严格", ok, "");
    }

    {
        let mut bag = DiagBag::new();
        // x 越界 → 钳制 + 诊断（接受）。
        let clamped = InterpParams::bezier(-3.0, 0.0, 9.0, 1.0, &mut bag);
        let clamp_ok = clamped.is_some()
            && clamped.unwrap().cx1 == BEZIER_CONTROL_MIN
            && clamped.unwrap().cx2 == BEZIER_CONTROL_MAX;
        let clamp_diag = !bag.errors().is_empty();

        // x 乱序 → **拒绝**（时间倒流，钳制救不了）。
        let mut bag2 = DiagBag::new();
        let rejected = InterpParams::bezier(0.8, 0.0, 0.2, 1.0, &mut bag2).is_none();
        let reject_diag = !bag2.errors().is_empty();

        // 非有限控制点 → 钳制到界而非 NaN。
        let mut bag3 = DiagBag::new();
        let nan_param = InterpParams::bezier(f32::NAN, f32::NAN, f32::NAN, f32::NAN, &mut bag3);
        let nan_ok = nan_param.is_some_and(|p| {
            p.cx1.is_finite() && p.cx2.is_finite() && p.cy1.is_finite() && p.cy2.is_finite()
        });
        set.add(
            "M03-贝塞尔-控制点钳制与乱序拒绝",
            clamp_ok && clamp_diag && rejected && reject_diag && nan_ok,
            "",
        );
    }

    {
        // 无幂运算：u → 1⁻ 时结果与 1 的差仍保到 1e-7 量级。
        // 定式（(1-u)³）在此处会丢到 1e-21 被尾数吃掉，稳定式不会。
        let near1 = bezier_axis(1.0 - 1e-7, 0.42, 0.58);
        let near0 = bezier_axis(1e-7, 0.42, 0.58);
        let ok = near1.is_finite()
            && near0.is_finite()
            && (1.0 - near1) < 1e-5
            && near0 < 1e-5
            && near1 > 0.999_99;
        set.add("M03-贝塞尔-无幂运算近1精度", ok, "");
    }

    {
        // y 轴超调允许（回弹是有意的美术效果），但超 ±2 视为恶意数据钳制。
        let mut bag = DiagBag::new();
        let overshoot = InterpParams::bezier(0.5, 1.5, 0.5, -1.5, &mut bag);
        let ok = overshoot.is_some()
            && overshoot.unwrap().cy1 == 1.5
            && overshoot.unwrap().cy2 == -1.5;
        let mut bag2 = DiagBag::new();
        let evil = InterpParams::bezier(0.5, 99.0, 0.5, -99.0, &mut bag2);
        let clamped = evil.is_some()
            && evil.unwrap().cy1 == BEZIER_Y_MAX
            && evil.unwrap().cy2 == BEZIER_Y_MIN;
        set.add("M03-贝塞尔-超调允许", ok && clamped, "");
    }

    // ---- 错误路径 ----

    {
        let mut bag = DiagBag::new();
        // 布尔配连续插值 → 拒绝，且用 F2402 的同码（跨条一致）。
        let bool_linear = !gate_interp_for_class(TrackClass::Bool, Interp::Linear, &mut bag);
        let same_code = bag
            .errors()
            .iter()
            .any(|d| d.code == crate::svstar2::vem02_track::DiagCode::DiscreteTrackRequiresStep);
        // 旋转配线性 → 拒绝（双覆盖下算错姿态）。
        let mut bag2 = DiagBag::new();
        let rot_linear = !gate_interp_for_class(TrackClass::Rotation, Interp::Linear, &mut bag2);
        // 合法组合放行。
        let mut bag3 = DiagBag::new();
        let pos_bezier = gate_interp_for_class(TrackClass::Position, Interp::CubicBezier, &mut bag3);
        let bool_step = gate_interp_for_class(TrackClass::Bool, Interp::Step, &mut bag3);
        set.add(
            "M03-错误-布尔配连续拒绝",
            bool_linear && same_code && rot_linear && pos_bezier && bool_step,
            "",
        );
    }

    {
        let r = InterpRegistry::new();
        let e = *r.lookup("linear").unwrap();
        let mut bag = DiagBag::new();
        // NaN 关键帧值 → 钳制 + 告警，且结果有限（不传染）。
        let v = eval_scalar_span(
            TrackClass::Float,
            &e,
            &InterpParams::LINEAR,
            f32::NAN,
            10.0,
            0.0,
            10.0,
            5.0,
            &mut bag,
        );
        let nan_ok = v.is_some_and(|x| x.is_finite()) && !bag.errors().is_empty();
        // Inf 亦然。
        let mut bag2 = DiagBag::new();
        let v2 = eval_scalar_span(
            TrackClass::Float,
            &e,
            &InterpParams::LINEAR,
            f32::INFINITY,
            10.0,
            0.0,
            10.0,
            5.0,
            &mut bag2,
        );
        let inf_ok = v2.is_some_and(|x| x.is_finite());
        // 三分量入口同样处置。
        let mut bag3 = DiagBag::new();
        let v3 = eval_vec3_span(
            TrackClass::Position,
            &e,
            &InterpParams::LINEAR,
            [f32::NAN, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            0.0,
            10.0,
            5.0,
            &mut bag3,
        );
        let vec_ok = v3.is_some_and(|x| x.iter().all(|c| c.is_finite()));
        set.add(
            "M03-错误-NaN钳制不传染",
            nan_ok && inf_ok && vec_ok,
            "",
        );
    }

    {
        let r = InterpRegistry::new();
        let e = *r.lookup("linear").unwrap();
        let mut bag = DiagBag::new();
        // 缩放禁外推 →钳到末值 + 告警。
        let clamped = eval_scalar_span(
            TrackClass::Scale,
            &e,
            &InterpParams::LINEAR,
            1.0,
            2.0,
            0.0,
            10.0,
            999.0,
            &mut bag,
        );
        // 浮点允许外推 → 线性外延（不告警）。
        let mut bag2 = DiagBag::new();
        let extended = eval_scalar_span(
            TrackClass::Float,
            &e,
            &InterpParams::LINEAR,
            1.0,
            2.0,
            0.0,
            10.0,
            20.0,
            &mut bag2,
        );
        set.add(
            "M03-错误-禁外推钳制",
            clamped == Some(2.0)
                && !bag.errors().is_empty()
                && extended == Some(3.0)
                && bag2.errors().is_empty(),
            "",
        );
    }

    {
        let r = InterpRegistry::new();
        let e = *r.lookup("linear").unwrap();
        let mut bag = DiagBag::new();
        // 零长区间（t0 == t1）→ 取起点，不 NaN。
        let zero = eval_scalar_span(
            TrackClass::Float,
            &e,
            &InterpParams::LINEAR,
            5.0,
            9.0,
            3.0,
            3.0,
            3.0,
            &mut bag,
        );
        // 逆序区间（t1 < t0）→ 不 NaN。
        let reversed = eval_scalar_span(
            TrackClass::Float,
            &e,
            &InterpParams::LINEAR,
            1.0,
            2.0,
            10.0,
            0.0,
            5.0,
            &mut bag,
        );
        set.add(
            "M03-错误-零长区间不NaN",
            zero.is_some_and(|x| x.is_finite()) && reversed.is_some_and(|x| x.is_finite()),
            "",
        );
    }

    // ---- 旋转接口位 ----

    {
        let a = [0.0, 0.0, 0.0, 1.0];
        let neg = [-0.0, -0.0, -0.0, -1.0];
        // 双覆盖取近路：q 与 −q 同姿态，中点应回到 a 附近（w≈1），不绕远。
        let mid = slerp_quat(a, neg, 0.5);
        let short_arc = (mid[3] - 1.0).abs() < 1e-5;
        // 端点精确。
        let endpoints = slerp_quat(a, [1.0, 0.0, 0.0, 0.0], 0.0) == a;
        // 90° 四分之一程：w = cos(45°) ≈ 0.7071。
        let q45 = slerp_quat(a, [1.0, 0.0, 0.0, 0.0], 0.5);
        let quarter = (q45[3] - 0.7071).abs() < 1e-3;
        // 短弧保护（近同姿态不炸）。
        let near = slerp_quat(a, [1e-7, 0.0, 0.0, 1.0], 0.5);
        let safe = near.iter().all(|v| v.is_finite());
        set.add(
            "M03-旋转-slerp双覆盖短弧",
            short_arc && endpoints && quarter && safe,
            "",
        );
    }

    {
        let a = [0.0, 0.0, 0.0, 1.0];
        let b = [1.0, 0.0, 0.0, 0.0];
        let mut unit = true;
        let mut i = 0u32;
        while i <= 20 {
            let q = slerp_quat(a, b, i as f32 / 20.0);
            let len_sq: f32 = q.iter().map(|v| v * v).sum();
            unit &= (len_sq - 1.0).abs() < 1e-5;
            i += 1;
        }
        // 归一化退化输入：零四元数 → 单位四元数（不 NaN）。
        let degen = normalize_quat([0.0; 4]) == [0.0, 0.0, 0.0, 1.0];
        // 双覆盖规范半空间：w<0 取反。
        let half = normalize_quat([0.0, 0.0, 0.0, -2.0]) == [0.0, 0.0, 0.0, 1.0];
        set.add("M03-旋转-结果恒单位化", unit && degen && half, "");
    }

    // ---- 鲁棒面 ----

    {
        let r = InterpRegistry::new();
        let e = *r.lookup("linear").unwrap();
        let mut bag = DiagBag::new();
        // 敌意输入：逆序时间戳 / 零长 / 极大值 / NaN 时间 / Inf 值 / NaN 四元数 /
        // 空缓动名 / NaN 缓动选择器 —— 全部走诊断不 panic。
        let _ = eval_scalar_span(TrackClass::Float, &e, &InterpParams::LINEAR, 1.0, 2.0, 10.0, 0.0, 5.0, &mut bag);
        let _ = eval_scalar_span(TrackClass::Float, &e, &InterpParams::LINEAR, 1.0, 2.0, 0.0, 0.0, 5.0, &mut bag);
        let _ = eval_scalar_span(TrackClass::Float, &e, &InterpParams::LINEAR, f32::INFINITY, 2.0, 0.0, 10.0, 5.0, &mut bag);
        let _ = eval_scalar_span(TrackClass::Float, &e, &InterpParams::LINEAR, 1.0, 2.0, 0.0, 10.0, f32::NAN, &mut bag);
        let _ = slerp_quat([f32::NAN; 4], [f32::NAN; 4], f32::NAN);
        let _ = bezier_axis(f32::NAN, 0.5, 0.5);
        let _ = InterpParams::bezier(f32::NAN, f32::NAN, f32::NAN, f32::NAN, &mut bag);
        let _ = lookup_easing("");
        let _ = normalize3([0.0, 0.0, 0.0]);
        let _ = normalize3([f32::NAN; 3]);
        set.add("M03-鲁棒-敌意输入不panic", true, "");
    }

    finish(set)
}

/// 收尾（保留为独立函数，便于日后加尾项而不动主体）。
fn finish(mut set: CheckSet) -> CheckSet {
    // 规模如实报告：不得因超限而静默截断。
    if set.truncated() {
        set.fail("M03-规模-未截断", "自检项数超 CheckSet 上限");
    }
    set
}
