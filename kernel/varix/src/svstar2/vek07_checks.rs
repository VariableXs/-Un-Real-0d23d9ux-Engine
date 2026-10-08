//! VE-F2007 · 域自检（判据逐条对应，见 `vek07_exposure.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 应用位兑现 → `K07-应用位-*`
//! - 眼适应过渡 → `K07-过渡-*`
//! - 量纲单源 → `K07-量纲-*`
//! - 场景重置 → `K07-场景-*`
//! - 错误路径四条（钳制/重置/冻结/互斥）→ `K07-错误-*`
//!
//! 零墙钟、零 IO，回归可复现。

use super::vek07_exposure::*;
use crate::checks::CheckSet;
// 上游依赖显式导入而非依赖 `vek07_exposure::*` 的间接带入：`use ...::*` 只
// 带出本模块**自己定义**的公开项，不带出它 `use` 进来的上游项。靠间接带入能编
// 过，但一旦本模块删掉某个不再需要的上游 import，调用方的可见性就会静默变化。
use crate::svstar2::vek05_params::{ev_to_multiplier, EV_MAX, EV_MIN, EV_NEUTRAL};
use crate::svstar2::vek06_tonemap::SceneReferred;

/// 近似相等。
fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

/// VE-F2007 域自检。
pub fn run_vek07_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vek07");

    // ---- 应用位兑现（判据一）----

    // 判据：曝光乘法在 TM 输入端——倍率 1.0 时输出逐分量等于输入（应用位透明）。
    {
        let c = SceneReferred::new(0.25, 0.5, 0.75);
        let out = apply_exposure(c, ExposureValue::neutral());
        let ok = close(out.color.r, 0.25)
            && close(out.color.g, 0.5)
            && close(out.color.b, 0.75)
            && !out.sanitized
            && close(out.exposure.multiplier, 1.0);
        set.add("K07-应用位-中性倍率下透明", ok, "");
    }

    // 判据：+2EV 应用后各分量恰为 4 倍（线性域乘法，不是加法、不是幂函数）。
    {
        let c = SceneReferred::new(0.1, 0.2, 0.4);
        let out = apply_exposure(c, ExposureValue::from_ev(2.0));
        let ok = close(out.color.r, 0.4)
            && close(out.color.g, 0.8)
            && close(out.color.b, 1.6)
            && close(out.exposure.multiplier, 4.0);
        set.add("K07-应用位-正EV为线性乘法", ok, "×4.000");
    }

    // 判据：-3EV 应用后各分量为 1/8（负 EV 不产生负值，只压暗）。
    {
        let c = SceneReferred::new(0.8, 0.8, 0.8);
        let out = apply_exposure(c, ExposureValue::from_ev(-3.0));
        let ok = close(out.color.r, 0.1)
            && close(out.color.b, 0.1)
            && out.color.r >= 0.0
            && close(out.exposure.multiplier, 0.125);
        set.add("K07-应用位-负EV仅压暗不为负", ok, "0.100");
    }

    // 判据：脏输入（NaN/Inf/负）在应用位被拦下，且报告值与实际乘法一致。
    //
    // 后半条（报告一致）不是凑数：`apply_exposure` 的防御分支一旦触发却仍把
    // 入参原样回填，遥测字段就与真实乘法脱钩。
    {
        let dirty = SceneReferred::new(f32::NAN, f32::INFINITY, -1.0);
        let out = apply_exposure(dirty, ExposureValue::from_ev(1.0));
        let clean_ok = out.color.r.is_finite() && out.color.g.is_finite()
            && out.color.r >= 0.0
            && out.color.g >= 0.0
            && out.color.b >= 0.0;
        let bad_exp = ExposureValue { ev: EV_MIN, multiplier: 0.0, clamped: false };
        let out2 = apply_exposure(SceneReferred::new(0.5, 0.5, 0.5), bad_exp);
        let report_ok = close(out2.exposure.multiplier, 1.0) && close(out2.color.r, 0.5);
        set.add(
            "K07-应用位-脏输入清洗且报告一致",
            clean_ok && report_ok && out.sanitized,
            "",
        );
    }

    // ---- 眼适应过渡（判据二）----

    // 判据：alpha 在有限 dt/tau 下恒在 [0,1]（过冲的根源就是 alpha>1）。
    {
        let mut ok = true;
        let taus = [0.02f32, 0.10, 0.40, 1.20, 3.0, 100.0];
        for tau in taus {
            for k in 0..40 {
                let dt = k as f32 * 0.01;
                let a = adapt_alpha(dt, tau);
                if !(0.0..=1.0).contains(&a) {
                    ok = false;
                }
            }
        }
        set.add("K07-过渡-alpha恒在[0,1]", ok, "");
    }

    // 判据：alpha 对 dt 严格单调不减（非单调意味着"等更久反而更慢"）。
    {
        let tau = 0.40;
        let mut ok = true;
        let mut prev = adapt_alpha(0.0, tau);
        for k in 1..200 {
            let a = adapt_alpha(k as f32 * 0.005, tau);
            if a < prev {
                ok = false;
            }
            prev = a;
        }
        set.add("K07-过渡-alpha对dt单调不减", ok, "");
    }

    // 判据：dt=0 / 非法 dt → alpha=0（曝光不动），避免 NaN 传染进当前值。
    {
        let ok = adapt_alpha(0.0, 0.4) == 0.0
            && adapt_alpha(-1.0, 0.4) == 0.0
            && adapt_alpha(0.016, 0.0) == 0.0
            && adapt_alpha(f32::NAN, 0.4) == 0.0;
        set.add("K07-过渡-非法步长不推进", ok, "");
    }

    // 判据：一阶追踪单调逼近目标，不过冲（这是过渡器的硬性质）。
    {
        let mut ok = true;
        for (start, target) in [(0.0f32, 6.0f32), (6.0, -6.0), (-6.0, 6.0), (-2.0, 3.5)] {
            let mut a = ExposureAdaptation::new(start, target, AdaptRate::Medium);
            let mut prev = a.current_ev();
            for _ in 0..400 {
                a.step(0.016);
                let cur = a.current_ev();
                // 单调性 + 不过冲：目标在高位时不许越过目标
                if target > start && (cur < prev || cur > target + 1e-4) {
                    ok = false;
                }
                if target < start && (cur > prev || cur < target - 1e-4) {
                    ok = false;
                }
                prev = cur;
            }
            if !a.settled() {
                ok = false;
            }
        }
        set.add("K07-过渡-单调逼近不过冲", ok, "");
    }

    // 判据：快/中/慢三档时间常数有序（快 < 中 < 慢）。
    {
        let f = AdaptRate::Fast.tau_seconds();
        let m = AdaptRate::Medium.tau_seconds();
        let s = AdaptRate::Slow.tau_seconds();
        set.add("K07-过渡-三档速率有序", f < m && m < s, "0.10/0.40/1.20s");
    }

    // 判据：同一目标同一 dt 下，快档比慢档更早收敛。
    {
        let fast = simulate_adaptation(0.0, 5.0, AdaptRate::Fast, 0.016, 30);
        let slow = simulate_adaptation(0.0, 5.0, AdaptRate::Slow, 0.016, 30);
        set.add("K07-过渡-快档先于慢档到位", fast > slow && fast < 5.0 && slow < 5.0, "");
    }

    // 判据：自定义秒数可配，且越界被钳（不自检则越界值会静默生效成不可预期速率）。
    {
        let ok = close(AdaptRate::Custom(0.40).tau_seconds(), RATE_MEDIUM_SECONDS)
            && close(AdaptRate::Custom(0.001).tau_seconds(), RATE_CUSTOM_MIN_SECONDS)
            && close(AdaptRate::Custom(99.0).tau_seconds(), RATE_CUSTOM_MAX_SECONDS)
            && AdaptRate::Custom(0.001).custom_clamped()
            && !AdaptRate::Custom(0.4).custom_clamped();
        set.add("K07-过渡-自定义秒数可配且钳制", ok, "");
    }

    // 判据：光敏舒适——单帧亮度变化率受限（阶跃响应最大倍率比有界）。
    //
    // **必须逐档测，不能只测慢档**：真正会越阈的是快档（实测 1.84x，逼近 2.0x），
    // 慢档（1.06x）永远绿，测它等于没测。早期版本只测慢档，恰好漏掉了唯一有
    // 风险的那一档——这类"挑最安全的样本验"的判据比没有判据更危险。
    {
        let mut ok = true;
        // 快档留 5% 余量（1.84x实测 vs 2.0x 经验阈），不贴死阈值：经验阈本身
        // 是估计值，贴死判据会在阈值被修正时集体假红。
        let budgets = [
            (AdaptRate::Fast, 1.90f32),
            (AdaptRate::Medium, 1.50),
            (AdaptRate::Slow, 1.30),
        ];
        for (rate, budget) in budgets {
            let worst = step_response_max_ratio(0.0, 6.0, rate, 0.016);
            if !(worst.is_finite() && worst < budget) {
                ok = false;
            }
        }
        set.add("K07-过渡-三档单帧亮度变化率受限", ok, "快/中/慢逐档验");
    }

    // ---- 量纲单源（判据三）----

    // 判据：双表示自洽（倍率 = 2^EV），由 F2005 单源换算保证。
    {
        let mut ok = true;
        let mut ev = EV_MIN;
        while ev <= EV_MAX {
            let v = ExposureValue::from_ev(ev);
            if !v.self_consistent() {
                ok = false;
            }
            ev += 0.25;
        }
        set.add("K07-量纲-双表示自洽", ok, "");
    }

    // 判据：往返误差在F2005 单源容差内（本条不另设容差）。
    //
    // **步长取 0.007 而非 0.125**：稀疏采样会漏掉最坏点。实测最坏值落在
    // f≈0 附近（`2^f`恰跨级数切换边界），0.125 步长恰好命中、0.25 就跳过。
    // 判据的采样密度必须匹配误差分布的尺度，否则"绿"只是没采到。
    {
        let mut worst = 0.0f32;
        let mut ev = EV_MIN;
        while ev <= EV_MAX {
            let err = ExposureValue::from_ev(ev).roundtrip_error_ev();
            if err > worst {
                worst = err;
            }
            ev += 0.007;
        }
        set.add(
            "K07-量纲-往返不超单源容差",
            worst <= EXPOSURE_EV_ROUNDTRIP_TOLERANCE,
            "容差引用F2005单源",
        );
    }

    // 判据：反解与正解对拍（倍率 → EV → 倍率，误差极小）。
    //
    // **容差必须按倍率大小取，不能统一 1e-3**：锚点倍率是 64（= 2^6），而f32 在
    // 64 附近的 1 ulp 就已经是 6e-6，统一 1e-3 尚可，但若把样本扩到更大倍率
    // （F1812 域上界之外的设计值）就会假红。这里用**相对容差** 1e-4
    // （64 上即 6.4e-3），它比 f32 的 1e-7 相对精度宽三个数量级，足以吸收
    // 级数误差，又远小于任何美术可辨的倍率差。
    {
        let mut ok = true;
        for m in [0.015625f32, 0.25, 0.5, 1.0, 2.0, 4.0, 32.0, 64.0] {
            let v = ExposureValue::from_multiplier(m);
            // 锚点倍率都是 2 的整数幂，正解在f32 上必须**精确**回到原值
            if !v.self_consistent() || (v.multiplier - m).abs() > m * 1e-4 {
                ok = false;
            }
        }
        set.add("K07-量纲-倍率反解与正解对拍", ok, "");
    }

    // 判据：EV 域钳制沿用 F1812 范围 [-6,+6]，中性 0。
    {
        let hi = ExposureValue::from_ev(99.0);
        let lo = ExposureValue::from_ev(-99.0);
        let ok = hi.ev == EV_MAX
            && lo.ev == EV_MIN
            && hi.clamped
            && lo.clamped
            && ExposureValue::from_ev(EV_NEUTRAL).multiplier == 1.0;
        set.add("K07-量纲-EV域沿用F1812范围", ok, "");
    }

    // 判据：应用位与泛光阈值联动共用同一 EV 语义（K07产出的EV 可直接喂 F2005）。
    {
        let ev = 1.5;
        let v = ExposureValue::from_ev(ev);
        let expect = ev_to_multiplier(ev);
        set.add("K07-量纲-与F2005联动同一换算", close(v.multiplier, expect), "");
    }

    // ---- 场景重置（判据四）----

    // 判据：场景切换时当前曝光**直接跳到**目标（过渡不跨场景）。
    {
        let mut d = ExpDiagBag::new();
        let mut s = ExposureState::new(1, ExposureMode::Manual, 0.0, AdaptRate::Slow);
        s.submit_manual_ev(4.0, &mut d);
        s.step(0.016); // 走一小步，让current != target
        let before = s.exposure().ev;
        s.begin_scene(2, &mut d);
        let after = s.exposure().ev;
        let ok = before < 1.0 && close(after, 4.0) && s.scene_id() == 2 && d.has(ExpDiagCode::SceneCutReset);
        set.add("K07-场景-切换直接跳目标不渐变", ok, "0→4.00 直接到位");
    }

    // 判据：新场景曝光按新场景的初始值开始，不继承旧场景的过渡中间态。
    {
        let mut d = ExpDiagBag::new();
        let mut s = ExposureState::new(1, ExposureMode::Auto, -3.0, AdaptRate::Medium);
        s.submit_auto_ev(3.0, &mut d);
        for _ in 0..10 {
            s.step(0.016);
        }
        s.begin_scene(2, &mut d);
        let ev = s.exposure().ev;
        set.add("K07-场景-不继承旧过渡中间态", close(ev, 3.0), "目标 3.00");
    }

    // ---- 错误路径 1：NaN / 极端钳制 ----

    // 判据：非有限 EV → 中性 + 显式告警（不是域端）。
    {
        let mut d = ExpDiagBag::new();
        let mut s = ExposureState::new(1, ExposureMode::Auto, 0.0, AdaptRate::Fast);
        s.submit_auto_ev(f32::NAN, &mut d);
        s.step(0.016);
        let ok = close(s.exposure().ev, EV_NEUTRAL)
            && d.has(ExpDiagCode::EvNonFinite)
            && s.exposure().multiplier == 1.0;
        set.add("K07-错误-非有限EV钳中性并告警", ok, "");
    }

    // 判据：超域 EV → 钳到域端 + 告警（+Inf 走 from_ev 的非有限分支取中性）。
    //
    // **必须推进过渡后再读值**：超域钳制改的是**目标 EV**，而 `exposure()` 读的
    // 是**当前 EV**——刚提交那一帧当前值还没动，读出来必然是旧值。
    // 早先版本正是在这里恒红，且原因（读错了对象）与现象（"钳制没生效"）
    // 隔着过渡器两层调用，肉眼极难自查。
    {
        let mut d = ExpDiagBag::new();
        let mut s = ExposureState::new(1, ExposureMode::Auto, 0.0, AdaptRate::Fast);
        s.submit_auto_ev(42.0, &mut d);
        for _ in 0..600 {
            s.step(0.016);
        }
        let ev = s.exposure().ev;
        let ok = close(ev, EV_MAX) && d.has(ExpDiagCode::EvClamped);
        set.add("K07-错误-超域EV钳域端并告警", ok, "EV=6.00");
    }

    // 判据：非法倍率（≤0）→ 钳到 EV 下界而非 0（0 倍率= 全黑是最坏输出）。
    {
        let v = ExposureValue::from_multiplier(0.0);
        let neg = ExposureValue::from_multiplier(-3.0);
        let ok = close(v.ev, EV_MIN) && close(neg.ev, EV_MIN) && v.multiplier > 0.0;
        set.add("K07-错误-非法倍率钳域下界非零", ok, "");
    }

    // ---- 错误路径 2：统计失效 → 冻结 + 告警 + 不黑屏 ----

    // 判据：统计失效 → 冻结当前值+ 告警，**曝光不归零**。
    {
        let mut d = ExpDiagBag::new();
        let mut s = ExposureState::new(1, ExposureMode::Auto, 2.0, AdaptRate::Fast);
        s.submit_auto_ev(5.0, &mut d);
        s.set_statistics_valid(false, &mut d);
        for _ in 0..600 {
            s.step(0.016);
        }
        let out = s.apply(SceneReferred::new(0.5, 0.5, 0.5));
        let ok = s.is_frozen()
            && d.has(ExpDiagCode::AutoStatisticsFrozen)
            && out.color.r > 0.0
            && !close(out.exposure.ev, EV_MIN)
            && !d.no_degradation();
        set.add(
            "K07-错误-统计失效冻结且不黑屏",
            ok,
            "非黑屏:亮度>0",
        );
    }

    // 判据：冻结期间自动输入不推进目标（冻结是真冻结，不是"慢速追踪"）。
    {
        let mut d = ExpDiagBag::new();
        let mut s = ExposureState::new(1, ExposureMode::Auto, 0.0, AdaptRate::Fast);
        s.set_statistics_valid(false, &mut d);
        s.submit_auto_ev(6.0, &mut d);
        for _ in 0..300 {
            s.step(0.016);
        }
        let ok = close(s.exposure().ev, EV_NEUTRAL) && s.is_frozen();
        set.add("K07-错误-冻结期间自动输入不推进", ok, "维持 0.000");
    }

    // 判据：统计恢复 → 解冻并继续正常追踪（不是永远冻结）。
    {
        let mut d = ExpDiagBag::new();
        let mut s = ExposureState::new(1, ExposureMode::Auto, 0.0, AdaptRate::Fast);
        s.set_statistics_valid(false, &mut d);
        s.set_statistics_valid(true, &mut d);
        s.submit_auto_ev(4.0, &mut d);
        for _ in 0..600 {
            s.step(0.016);
        }
        let ok = !s.is_frozen() && d.has(ExpDiagCode::AutoStatisticsResumed) && s.exposure().ev > 3.9;
        set.add("K07-错误-统计恢复后继续追踪", ok, "→4.00");
    }

    // 判据：同tick组合——场景切换时统计失效，重置退化为"保持"而非跳进坏目标。
    {
        let mut d = ExpDiagBag::new();
        let mut s = ExposureState::new(1, ExposureMode::Auto, 1.5, AdaptRate::Fast);
        s.set_statistics_valid(false, &mut d);
        s.submit_auto_ev(6.0, &mut d); // 冻结期间该输入应被拒
        s.begin_scene(2, &mut d);
        let ok = close(s.exposure().ev, 1.5) && d.has(ExpDiagCode::SceneCutReset);
        set.add("K07-错误-切换遇失效保持旧曝光", ok, "保持 1.50");
    }

    // ---- 错误路径 3：手动/自动互斥 ----

    // 判据：手动生效时请求切自动 → 维持手动 + 冲突告警（F1812 语义）。
    {
        let mut d = ExpDiagBag::new();
        let mut s = ExposureState::new(1, ExposureMode::Manual, 1.0, AdaptRate::Fast);
        let dec = s.set_mode(ExposureMode::Auto, &mut d);
        let ok = dec.conflicted
            && dec.effective == ExposureMode::Manual
            && s.mode() == ExposureMode::Manual
            && d.has(ExpDiagCode::ModeConflictToManual);
        set.add("K07-错误-手动优先自动禁用", ok, "");
    }

    // 判据：反向（手动 → 请求自动）不算冲突（自动是被替换而非混用）。
    {
        let mut d = ExpDiagBag::new();
        let mut s = ExposureState::new(1, ExposureMode::Auto, 0.0, AdaptRate::Fast);
        let dec = s.set_mode(ExposureMode::Manual, &mut d);
        let ok = !dec.conflicted && dec.effective == ExposureMode::Manual;
        set.add("K07-错误-切手动非冲突", ok, "");
    }

    // 判据：手动生效时自动输入被忽略并留痕（不静默丢弃 = 可观测的 J/K 不同步）。
    {
        let mut d = ExpDiagBag::new();
        let mut s = ExposureState::new(1, ExposureMode::Manual, 0.0, AdaptRate::Fast);
        s.submit_auto_ev(5.0, &mut d);
        for _ in 0..600 {
            s.step(0.016);
        }
        let ok = close(s.exposure().ev, EV_NEUTRAL) && d.has(ExpDiagCode::AutoInputIgnoredWhileManual);
        set.add("K07-错误-手动期自动输入忽略留痕", ok, "");
    }

    // 判据：手动通道不被互斥误伤（手动模式下用户手动调 EV 必须生效）。
    {
        let mut d = ExpDiagBag::new();
        let mut s = ExposureState::new(1, ExposureMode::Manual, 0.0, AdaptRate::Fast);
        s.submit_manual_ev(3.0, &mut d);
        for _ in 0..600 {
            s.step(0.016);
        }
        let ok = s.exposure().ev > 2.9;
        set.add("K07-错误-手动通道不被互斥误伤", ok, "→3.00");
    }

    // 判据：冻结期间手动输入仍生效（用户的自救通道不被冻结锁死）。
    {
        let mut d = ExpDiagBag::new();
        let mut s = ExposureState::new(1, ExposureMode::Auto, 0.0, AdaptRate::Fast);
        s.set_statistics_valid(false, &mut d);
        s.submit_manual_ev(-2.0, &mut d);
        for _ in 0..600 {
            s.step(0.016);
        }
        let ok = s.exposure().ev < -1.9;
        set.add("K07-错误-冻结期手动仍可救场", ok, "→-2.00");
    }

    // ---- 契约与性能声明 ----

    // 判据：J 管计算 / K 管应用的分工登记齐备（应用位兑现的组织面）。
    {
        let ok = vek07_contract_ok()
            && EXPOSURE_CONTRACT_DOC.iter().any(|r| r.0.contains("亮度统计"))
            && EXPOSURE_CONTRACT_DOC.iter().any(|r| r.0.contains("曝光应用"))
            && F1812_REDEMPTION_DOC.iter().any(|r| r.0.contains("眼适应"));
        set.add("K07-契约-分工与兑现登记齐备", ok, "");
    }

    // 判据：F1927 同构声明与 F1963 光敏声明在位（无障碍与跨域语义不退化）。
    {
        let ok = ADAPTATION_ISOMORPH_DECL.contains("F1927")
            && PHOTOSENSITIVE_DECL.contains("F1963")
            && PHOTOSENSITIVE_DECL.contains("无隐私面");
        set.add("K07-契约-同构与光敏声明在位", ok, "");
    }

    // 判据：过渡器状态为 O(1)（两个标量，无历史缓冲 —— 锚点性能分解字面兑现）。
    {
        let a = ExposureAdaptation::new(0.0, 1.0, AdaptRate::Medium);
        // 结构里只有 current_ev / target_ev / rate 三个标量，无 Vec/Box
        let size = core::mem::size_of::<ExposureAdaptation>();
        set.add("K07-契约-过渡器状态O(1)", size <= 16, "三标量无缓冲");
        let _ = a;
    }

    // 判据：档位key 可反查（配置面稳定标识，避免 UI 存中文标签）。
    {
        let ok = AdaptRate::from_key("adapt.fast") == Some(AdaptRate::Fast)
            && AdaptRate::from_key("adapt.medium") == Some(AdaptRate::Medium)
            && AdaptRate::from_key("adapt.slow") == Some(AdaptRate::Slow)
            && AdaptRate::from_key("adapt.custom") == Some(AdaptRate::Custom(RATE_MEDIUM_SECONDS))
            && AdaptRate::from_key("adapt.nope").is_none()
            && ExposureMode::Auto.key() == "exposure.auto";
        set.add("K07-契约-档位key可反查", ok, "");
    }

    // 判据：人读文本含模式/EV/倍率/冻结标记（设置页可读性）。
    {
        let mut d = ExpDiagBag::new();
        let mut s = ExposureState::new(7, ExposureMode::Auto, 1.0, AdaptRate::Medium);
        s.set_statistics_valid(false, &mut d);
        let txt = s.screen_text();
        // `txt` 是局部 String，不能借给 `add` 的`&'static str` 形参——判据文本
        // 只表达"该文本应含哪些关键态"，实测全文由 `settings_text_has_key_states`
        // 单测断言（那里能拿到真正的String 并做contains）。
        let ok = txt.contains("EV") && txt.contains("冻结") && txt.contains("自动");
        set.add("K07-契约-人读文本含关键态", ok, "模式/EV/倍率/冻结");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 步进系数必须恒不过冲。alpha > 1 是本条最致命的缺陷：它会让一阶追踪
    /// 变成振荡（亮度来回摆动），恰是本条要消灭的现象，且现象随帧率变化、
    /// 难以在固定步进的测试里复现。
    #[test]
    fn alpha_never_exceeds_one() {
        for tau in [0.02f32, 0.1, 0.4, 1.2, 3.0] {
            for k in 0..500 {
                let dt = k as f32 * 0.004;
                let a = adapt_alpha(dt, tau);
                assert!(
                    (0.0..=1.0).contains(&a),
                    "tau={} dt={} alpha={} 越界",
                    tau,
                    dt,
                    a
                );
            }
        }
    }

    /// 一阶追踪单调且不过冲（跨全EV 域的阶跃）。
    #[test]
    fn tracking_is_monotone_and_bounded() {
        let cases = [(0.0f32, 6.0f32), (6.0, -6.0), (-6.0, 0.0), (2.0, -5.0)];
        for (start, target) in cases {
            let mut a = ExposureAdaptation::new(start, target, AdaptRate::Medium);
            let mut prev = a.current_ev();
            for _ in 0..1000 {
                a.step(0.016);
                let cur = a.current_ev();
                if target > start {
                    assert!(cur >= prev - 1e-6, "上升段回退{}→{}", prev, cur);
                    assert!(cur <= target + 1e-4, "越过目标 {} > {}", cur, target);
                } else {
                    assert!(cur <= prev + 1e-6, "下降段回退 {}→{}", prev, cur);
                    assert!(cur >= target - 1e-4, "越过目标 {} < {}", cur, target);
                }
                prev = cur;
            }
            assert!(a.settled(), "1000 tick 未收敛（tau=0.4s, dt=16ms）");
        }
    }

    /// 冻结不等于黑屏：统计失效时曝光保持，输出亮度非零。
    ///
    /// 这条测试对应锚点错误路径第3 条里"不黑屏"三个字。它写成测试而不是
    /// 只写断言，是因为"冻结 = 置零"这个错误实现**同样能让冻结判据通过**
    /// （目标确实不再变化），只有输出亮度能区分两者。
    #[test]
    fn freeze_holds_exposure_instead_of_zeroing() {
        let mut d = ExpDiagBag::new();
        let mut s = ExposureState::new(1, ExposureMode::Auto, 2.0, AdaptRate::Fast);
        s.submit_auto_ev(5.0, &mut d);
        s.set_statistics_valid(false, &mut d);
        for _ in 0..600 {
            s.step(0.016);
        }
        assert!(s.is_frozen());
        let out = s.apply(SceneReferred::new(0.5, 0.5, 0.5));
        assert!(out.color.r > 0.0, "冻结后画面为黑：ev={}", out.exposure.ev);
        assert!(out.exposure.ev > EV_MIN, "冻结后退到域下界（近似黑）");
    }

    /// 场景切换的过渡不跨场景（当前直接等于目标）。
    #[test]
    fn scene_cut_resets_without_transition() {
        let mut d = ExpDiagBag::new();
        let mut s = ExposureState::new(1, ExposureMode::Manual, 0.0, AdaptRate::Slow);
        s.submit_manual_ev(4.0, &mut d);
        s.step(0.016);
        assert!(s.exposure().ev < 1.0, "预条件：current 尚未追上 target");
        s.begin_scene(2, &mut d);
        assert!(close(s.exposure().ev, 4.0), "切换后未直接跳到目标");
        assert!(s.scene_id() == 2);
    }

    /// 双表示在全域内自洽（倍率 = 2^EV），往返误差不超 F2005 单源容差。
    #[test]
    fn dual_representation_consistent_and_within_tolerance() {
        let mut ev = EV_MIN;
        while ev <= EV_MAX {
            let v = ExposureValue::from_ev(ev);
            assert!(v.self_consistent(), "EV {} 双表示不自洽: {:?}", ev, v);
            assert!(
                v.roundtrip_error_ev() <= EXPOSURE_EV_ROUNDTRIP_TOLERANCE,
                "EV {} 往返误差 {} 超单源容差",
                ev,
                v.roundtrip_error_ev()
            );
            ev += 0.25;
        }
    }

    /// 三档的单帧亮度变化率都在光敏预算内。
    ///
    /// 实测（60fps、6EV 阶跃）：快 1.84x / 中 1.18x / 慢 1.06x，光敏经验阈 2.0x。
    /// **快档是唯一逼近阈值的档位**，所以它必须被单独断言——只测慢档的判据
    /// 在快档越阈时依然全绿。
    #[test]
    fn all_rates_respect_photosensitivity_budget() {
        let budgets = [
            (AdaptRate::Fast, 1.90f32),
            (AdaptRate::Medium, 1.50),
            (AdaptRate::Slow, 1.30),
        ];
        for (rate, budget) in budgets {
            let worst = step_response_max_ratio(0.0, 6.0, rate, 0.016);
            assert!(
                worst.is_finite() && worst < budget,
                "{:?} 单帧亮度变化率 {:.3}x 超预算 {:.2}x",
                rate,
                worst,
                budget
            );
        }
    }

    /// 手动/自动互斥：手动优先，且手动通道不被误伤。
    #[test]
    fn manual_wins_and_manual_channel_still_works() {        let mut d = ExpDiagBag::new();
        let mut s = ExposureState::new(1, ExposureMode::Manual, 0.0, AdaptRate::Fast);
        let dec = s.set_mode(ExposureMode::Auto, &mut d);
        assert!(dec.conflicted && dec.effective == ExposureMode::Manual);

        // 自动输入被忽略
        s.submit_auto_ev(5.0, &mut d);
        for _ in 0..600 {
            s.step(0.016);
        }
        assert!(close(s.exposure().ev, EV_NEUTRAL), "自动输入未被忽略");

        // 手动输入仍生效
        s.submit_manual_ev(3.0, &mut d);
        for _ in 0..600 {
            s.step(0.016);
        }
        assert!(s.exposure().ev > 2.9, "手动通道被互斥误伤");
    }

    /// 设置页人读文本含全部关键态（模式 / EV / 倍率 / 速率 / 冻结标记）。
    ///
    /// 判据项只能检查"该含的子串都在"，因为 `CheckSet::add` 的 detail 形参是
    /// `&'static str`，装不下运行时String。真正的全文断言在这里做。
    #[test]
    fn settings_text_has_key_states() {
        let mut d = ExpDiagBag::new();
        let mut s = ExposureState::new(7, ExposureMode::Auto, 1.0, AdaptRate::Medium);
        let txt = s.screen_text();
        assert!(txt.contains("EV") && txt.contains("自动") && txt.contains("×"), "常态文本缺项: {}", txt);

        s.set_statistics_valid(false, &mut d);
        let frozen_txt = s.screen_text();
        assert!(frozen_txt.contains("冻结"), "冻结态未在文本中显性: {}", frozen_txt);

        s.set_mode(ExposureMode::Manual, &mut d);
        let manual_txt = s.screen_text();
        assert!(manual_txt.contains("手动"), "手动态未显性: {}", manual_txt);
    }
}
