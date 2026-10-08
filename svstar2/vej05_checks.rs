//! VE-F1805 · 域自检（判据逐条对应，见 `vej05_spotlight.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 内外锥（内<外约束，导入期+运行期双检） → `J05-内外锥-双检拒绝`
//! - 半影 smoothstep → `J05-半影-smoothstep对拍`
//! - 衰减合成（距离×锥形两级相乘） → `J05-合成-两级相乘`
//! - IES 预留（接口位+RESERVED） → `J05-IES-预留接口位`
//! - 显性报错（三要素+版本边界） → `J05-IES-引用未实现显性报错`
//! - 方向零向量→归一化告警；锥角超180°→钳制 → `J05-防护-零向量回退与锥角钳制`
//! - LUT 可选优化（采样对拍实时） → `J05-LUT-与实时smoothstep一致`
//! - 前向兼容约束在册 → `J05-IES-前向兼容约束`
//! - F1828 视锥贴合消费锥参数（参数可读面） → `J05-内外锥-双检拒绝`（字段直读）
//!
//! 逻辑 tick 注入、零墙钟，回归可复现。

use super::vej04_pointlight::FalloffCurve;
use super::vej05_spotlight::*;
use crate::checks::CheckSet;

/// 标准聚光：-Z 朝向，内 0.2 / 外 0.8，range 100。
fn spot() -> SpotLight {
    SpotLight::new(
        (0.0, 0.0, 0.0),
        (0.0, 0.0, -1.0),
        0.2,
        0.8,
        100.0,
        1.0,
        FalloffCurve::Physical,
        IesSlot::none(),
    )
    .unwrap()
    .0
}

/// VE-F1805 域自检。
pub fn run_vej05_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vej05");

    // ---- 内外锥：双检拒绝 ----

    {
        // 导入期：内≥外构造拒绝，错误三要素齐发。
        let e = SpotLight::new(
            (0.0, 0.0, 0.0),
            (0.0, 0.0, -1.0),
            1.0,
            0.5,
            10.0,
            1.0,
            FalloffCurve::Physical,
            IesSlot::none(),
        )
        .unwrap_err();
        let import_rejected = matches!(
            e,
            SpotError::ConeOrderInvalid {
                phase: "导入期",
                ..
            }
        ) && e.describe().contains("下一步");
        // 运行期：字段被改坏后第二道闸拦截；锥参数可被 F1828 直读。
        let s = spot();
        let cone_params_readable = s.inner_cone == 0.2 && s.outer_cone == 0.8;
        let bad = SpotLight {
            inner_cone: 0.9,
            outer_cone: 0.8,
            ..spot()
        };
        let runtime_rejected = matches!(
            bad.validate(),
            Err(SpotError::ConeOrderInvalid {
                phase: "运行期",
                ..
            })
        );
        let normal_ok = s.validate().is_ok();
        set.add(
            "J05-内外锥-双检拒绝",
            import_rejected && runtime_rejected && normal_ok && cone_params_readable,
            "",
        );
    }

    // ---- 半影 smoothstep 对拍 ----

    {
        let s = spot();
        // 轴上全亮；90°（>外锥 0.8）归零；θ=0.5 半影中点 ≈ 0.5。
        let axis = cone_attenuation(&s, 0.0, 0.0, -1.0);
        let outside = cone_attenuation(&s, 10.0, 0.0, 0.0);
        let mid = cone_attenuation(&s, 0.0, -0.479_425_5, -0.877_582_6);
        // 纯函数对拍：三次多项式端点与中点。
        let pure = penumbra_factor(0.0) == 1.0
            && (penumbra_factor(0.5) - 0.5).abs() < 1e-6
            && penumbra_factor(1.0).abs() < 1e-6;
        set.add(
            "J05-半影-smoothstep对拍",
            axis == 1.0 && outside == 0.0 && (mid - 0.5).abs() < 0.01 && pure,
            "",
        );
    }

    // ---- 衰减合成：两级相乘 ----

    {
        let s = spot();
        // 轴上 d=2：锥形 1 × 物理距离 1/4 = 0.25。
        let on_axis = combined_attenuation(&s, (0.0, 0.0, -2.0));
        // 外锥外：距离再近合成也是 0（锥形 × 距离，锥形为 0）。
        let off_cone = combined_attenuation(&s, (10.0, 0.0, 0.0));
        // 范围外：锥形再亮合成也是 0（距离衰减 early-out）。
        let off_range = combined_attenuation(&s, (0.0, 0.0, -200.0));
        set.add(
            "J05-合成-两级相乘",
            (on_axis - 0.25).abs() < 1e-5 && off_cone == 0.0 && off_range == 0.0,
            "",
        );
    }

    // ---- IES 预留接口位 ----

    {
        // 未引用：放行；引用 RESERVED：登记路径且解析显性报错。
        let none_ok = IesSlot::none().request_parse().is_ok();
        let slot = IesSlot::reserve("lights/stage.ies");
        let reserved = matches!(
            slot.status,
            IesStatus::Reserved { ref path } if path == "lights/stage.ies"
        );
        let err = slot.request_parse().unwrap_err();
        let explicit = matches!(err, SpotError::IesNotImplemented { .. });
        let doc = IES_FORWARD_COMPAT_DOC.contains("不得改变现有聚光参数块布局")
            && IES_FORWARD_COMPAT_DOC.contains("RESERVED");
        set.add(
            "J05-IES-预留接口位",
            none_ok && reserved && explicit && doc,
            "",
        );
    }

    // ---- 显性报错：三要素 + 版本边界 ----

    {
        let slot = IesSlot::reserve("stage.ies");
        let d = slot.request_parse().unwrap_err().describe();
        set.add(
            "J05-IES-引用未实现显性报错",
            d.contains("未实现") && d.contains("下一步") && d.contains("版本边界"),
            "",
        );
    }

    // ---- 防护：零向量回退 + 锥角钳制 + 归一化 ----

    {
        let (s, warn) = SpotLight::new(
            (0.0, 0.0, 0.0),
            (0.0, 0.0, 0.0),
            0.2,
            0.8,
            10.0,
            1.0,
            FalloffCurve::Physical,
            IesSlot::none(),
        )
        .unwrap();
        let fallback =
            s.dir == FALLBACK_DIR && warn.iter().any(|w| w.code == "W_DIR_ZERO_FALLBACK");
        let (s2, warn2) = SpotLight::new(
            (0.0, 0.0, 0.0),
            (0.0, 0.0, -1.0),
            0.2,
            7.0,
            10.0,
            1.0,
            FalloffCurve::Physical,
            IesSlot::none(),
        )
        .unwrap();
        let clamped = (s2.outer_cone - MAX_CONE_ANGLE).abs() < 1e-6
            && warn2.iter().any(|w| w.code == "W_CONE_CLAMPED");
        // 非零方向归一化到单位长度。
        let (s3, _) = SpotLight::new(
            (0.0, 0.0, 0.0),
            (0.0, 3.0, 4.0),
            0.2,
            0.8,
            10.0,
            1.0,
            FalloffCurve::Physical,
            IesSlot::none(),
        )
        .unwrap();
        let l2 = s3.dir.0 * s3.dir.0 + s3.dir.1 * s3.dir.1 + s3.dir.2 * s3.dir.2;
        set.add(
            "J05-防护-零向量回退与锥角钳制",
            fallback && clamped && (l2 - 1.0).abs() < 1e-5,
            "",
        );
    }

    // ---- LUT 与实时 smoothstep 一致 ----

    {
        let s = spot();
        let lut = SpotLut::build(&s, 64).unwrap();
        let mut all_match = true;
        let mut i = 1;
        while i < 16 {
            let theta = s.inner_cone + (s.outer_cone - s.inner_cone) * (i as f32 / 16.0);
            let live = penumbra_factor((theta - s.inner_cone) / (s.outer_cone - s.inner_cone));
            if (live - lut.sample(theta)).abs() >= 0.01 {
                all_match = false;
            }
            i += 1;
        }
        let boundary =
            lut.sample(s.inner_cone - 0.01) == 1.0 && lut.sample(s.outer_cone + 0.01) == 0.0;
        set.add(
            "J05-LUT-与实时smoothstep一致",
            all_match && boundary && SpotLut::build(&s, 1).is_err(),
            "",
        );
    }

    // ---- 强度语义（F1802 对接面：cd 直读） ----

    {
        let s = spot();
        set.add(
            "J05-对接-强度与距离衰减复用",
            s.intensity_cd == 1.0 && s.range == 100.0,
            "",
        );
    }

    // ---- 钳制与校序的次序（缺陷现场：先校序后钳制会放行自相矛盾参数块） ----

    {
        // 内锥 3.2 / 外锥 7.0：外锥钳到 π=3.14159 后，内锥(3.2)反而 > 外锥。
        // 顺序错则构造成功，半影分母为负 → smoothstep 参数越界。
        let rejected = matches!(
            SpotLight::new(
                (0.0, 0.0, 0.0),
                (0.0, 0.0, -1.0),
                3.2,
                7.0,
                10.0,
                1.0,
                FalloffCurve::Physical,
                IesSlot::none(),
            ),
            Err(SpotError::ConeOrderInvalid {
                phase: "导入期",
                ..
            })
        );
        // 对照：内锥 3.0 < π，钳制后仍合法且外锥确为 π。
        let (ok, warn) = SpotLight::new(
            (0.0, 0.0, 0.0),
            (0.0, 0.0, -1.0),
            3.0,
            7.0,
            10.0,
            1.0,
            FalloffCurve::Physical,
            IesSlot::none(),
        )
        .unwrap();
        let legit = ok.validate().is_ok()
            && (ok.outer_cone - MAX_CONE_ANGLE).abs() < 1e-6
            && warn.iter().any(|w| w.code == "W_CONE_CLAMPED");
        set.add("J05-防护-先钳制后校序", rejected && legit, "");
    }

    // ---- 非有限角（NaN/±∞）显式拒绝 ----

    {
        // NaN 与任何值比较恒 false：只靠 `>=` 校序会静默放行 NaN。
        let nan_rejected = matches!(
            SpotLight::new(
                (0.0, 0.0, 0.0),
                (0.0, 0.0, -1.0),
                f32::NAN,
                0.8,
                10.0,
                1.0,
                FalloffCurve::Physical,
                IesSlot::none(),
            ),
            Err(SpotError::ConeNotFinite {
                phase: "导入期",
                ..
            })
        );
        // ±∞ 同拒。
        let inf_rejected = SpotLight::new(
            (0.0, 0.0, 0.0),
            (0.0, 0.0, -1.0),
            0.2,
            f32::INFINITY,
            10.0,
            1.0,
            FalloffCurve::Physical,
            IesSlot::none(),
        )
        .is_err();
        // 运行期双检独立拦得住（字段 pub，外部可改坏）。
        let s = spot();
        let nan_light = SpotLight {
            inner_cone: f32::NAN,
            ..s
        };
        let runtime_nan_rejected = matches!(
            nan_light.validate(),
            Err(SpotError::ConeNotFinite {
                phase: "运行期",
                ..
            })
        );
        set.add(
            "J05-防护-非有限锥角拒绝",
            nan_rejected && inf_rejected && runtime_nan_rejected,
            "",
        );
    }

    // ---- LUT 构建期校验（坏灯不建表，步长不得为负） ----

    {
        let s = spot();
        // 采样点不足：显性拒绝且错误类型如实。
        let too_few = matches!(
            SpotLut::build(&s, 1),
            Err(SpotError::LutSamplesTooFew {
                requested: 1,
                minimum: 2
            })
        );
        // 坏灯（内>外）：拒绝而非建出负步长表。
        let bad = SpotLight {
            inner_cone: 0.9,
            outer_cone: 0.8,
            ..s.clone()
        };
        let bad_rejected = matches!(
            SpotLut::build(&bad, 8),
            Err(SpotError::ConeOrderInvalid {
                phase: "LUT构建",
                ..
            })
        );
        // 好灯仍可建表（防「一律拒绝」的过门禁假绿）。
        let good_ok = SpotLut::build(&s, 64).is_ok();
        set.add(
            "J05-LUT-构建期校验灯",
            too_few && bad_rejected && good_ok,
            "",
        );
    }

    set
}
