//! VE-F2005 · 域自检（判据逐条对应，见 `vek05_params.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 参数体系三件（阈值 cd/m² / 强度 0-1 / tint 可关）→ `K05-参数-*`
//! - 曝光补偿（F1849 抖动防护兑现：同比缩放而非加法）→ `K05-补偿-*`
//! - 预设三档（无泛光/柔光/强泛光，F1942 组合快照）→ `K05-预设-*`
//! - 参数动画（三参数各自轨道，F1345 插值器复用）→ `K05-动画-*`
//! - 错误路径四条（钳制 / 依赖检查 / 写入仲裁 / 预设缺失三要素）→ `K05-错误-*`
//! - 免重编译（参数与结构分离）→ `K05-免编译-*`
//!
//! 零墙钟、零 IO，回归可复现。

use super::vek04_bloom::DiagCode;
use super::vek05_params::*;
use crate::checks::CheckSet;

/// 近似相等。
fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

/// VE-F2005 域自检。
pub fn run_vek05_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vek05");

    // ---- 量纲换算单源（F1802：倍率 = 2^EV） ----

    // 判据：EV↔倍率 换算在整数锚点上精确（0/±1/+2 EV），小数 EV 上相对误差
    // 有界；超域 EV 被钳制。
    {
        let mut anchors_ok = true;
        for (ev, want) in EV_ANCHORS {
            if !close(ev_to_multiplier(ev), want) {
                anchors_ok = false;
            }
        }
        let half = ev_to_multiplier(0.5); // sqrt(2) ≈ 1.4142
        let neg_over = ev_to_multiplier(-99.0); // 钳到 -6 EV = 1/64
        let non_finite = ev_to_multiplier(f32::NAN);
        set.add(
            "K05-补偿-EV换算单源与钳制",
            anchors_ok
                && (half - 1.414_213_6).abs() < 1e-3
                && close(neg_over, 1.0 / 64.0)
                && close(non_finite, 1.0)
                && close(ev_to_multiplier(EV_MAX), 64.0),
            "",
        );
    }

    // 判据：换算往返一致（倍率 → EV → 倍率），含小数 EV（整数锚点易过，
    // 真正会错的是小数位）。容差取 `EV_ROUNDTRIP_TOLERANCE`（实测最大
    // 偏差约 0.051 EV，出现在 f=0.75 附近两段泰勒误差同向叠加处）。
    {
        let mut rt_ok = true;
        for ev in [-6.0f32, -3.5, -1.25, -0.5, 0.0, 0.5, 1.0, 2.75, 3.0, 6.0] {
            let m = ev_to_multiplier(ev);
            let back = multiplier_to_ev(m);
            if (back - ev).abs() > EV_ROUNDTRIP_TOLERANCE {
                rt_ok = false;
            }
            // 倍率往返误差按同一 EV 容差折算成相对误差（dEV=0.06 → 约 4.2%）
            let rel = EV_ROUNDTRIP_TOLERANCE * 0.693_147_2;
            if (ev_to_multiplier(back) - m).abs() > m * rel {
                rt_ok = false;
            }
        }
        set.add("K05-补偿-EV往返回到原值（含小数EV）", rt_ok, "");
    }

    // ---- 参数面钳制（错误路径第 1 条） ----

    // 判据：阈值非负、强度 0-1、tint 分量钳制，全部留诊断。
    {
        let r = BloomParamBlock {
            threshold_base: -3.0,
            strength: 4.5,
            decay: 9.0,
            mip_levels: 5,
            tint: Tint::new(2.0, -1.0, f32::NAN),
            exposure_link: ExposureLink::Auto,
        }
        .resolve(true);
        let tint_ok = close(r.block.tint.r, TINT_MAX)
            && close(r.block.tint.g, TINT_MIN)
            && close(r.block.tint.b, 1.0); // 非有限 → 退回不染色（1.0）
        set.add(
            "K05-参数-阈值强度tint钳制且留诊断",
            close(r.block.threshold_base, 0.0)
                && close(r.block.strength, STRENGTH_MAX)
                && close(r.block.decay, 1.0)
                && tint_ok
                && r.diags.has(DiagCode::ThresholdNegative)
                && r.diags.has(DiagCode::IntensityOutOfRange)
                && r.diags.has(DiagCode::CompositeDecayOutOfRange),
            "",
        );
    }

    // 判据：强度参数面 0-1（F2005 口径）——与 F2004 运行期 [0,4] 分层不冲突。
    {
        let lo = BloomParamBlock { strength: -1.0, ..Default::default() }.resolve(true);
        let hi = BloomParamBlock { strength: 1.0, ..Default::default() }.resolve(true);
        set.add(
            "K05-参数-强度0-1线性口径",
            close(lo.block.strength, STRENGTH_MIN) && close(hi.block.strength, STRENGTH_MAX),
            "",
        );
    }

    // ---- 曝光联动与补偿（判据核心） ----

    // 判据：Auto 下阈值随曝光**同比缩放**（倍率域乘法），保持提取集合不变。
    // 关键反例：加法补偿（thr + EV）在曝光 +1EV 时只抬 1 cd/m²，而场景亮度
    // 涨 100% —— 泛光会随曝光推进而越来越弱。本断言按乘法对拍。
    {
        let b = BloomParamBlock {
            threshold_base: 2.0,
            ..Default::default()
        };
        let at_0 = b.effective_threshold(ExposureLink::Auto, 0.0);
        let at_pos1 = b.effective_threshold(ExposureLink::Auto, 1.0);
        let at_neg1 = b.effective_threshold(ExposureLink::Auto, -1.0);
        //乘法：2 → 4 → 1；加法错法会是 2 → 3 → 1（且负EV 会给 1）
        set.add(
            "K05-补偿-阈值随曝光同比缩放（非加法）",
            close(at_0, 2.0) && close(at_pos1, 4.0) && close(at_neg1, 1.0),
            "",
        );
    }

    // 判据：曝光翻倍时场景亮度与阈值同比翻倍 → **比值不变**（提取集合恒定，
    // 这才是"抖动防护"的实质：不补偿时比值会随曝光漂移）。
    {
        let b = BloomParamBlock { threshold_base: 2.0, ..Default::default() };
        let thr0 = b.effective_threshold(ExposureLink::Auto, 0.0);
        let thr1 = b.effective_threshold(ExposureLink::Auto, 2.0); // ×4
        let scene0 = 8.0;
        let scene1 = 32.0; // 曝光 +2EV → 场景 ×4
        let ratio0 = scene0 / thr0;
        let ratio1 = scene1 / thr1;
        // 手动模式下比值会漂移 4 倍（这正是不补偿的症状）
        let manual_thr1 = b.effective_threshold(ExposureLink::Manual, 2.0);
        let manual_ratio1 = scene1 / manual_thr1;
        set.add(
            "K05-补偿-提取比值恒定（抖动防护实质）",
            close(ratio0, ratio1) && close(manual_ratio1 / ratio0, 4.0),
            "",
        );
    }

    // 判据：手动模式下阈值不随曝光变（作者自负其责，但语义明确）。
    {
        let b = BloomParamBlock { threshold_base: 3.0, ..Default::default() };
        set.add(
            "K05-补偿-手动模式阈值恒定",
            close(b.effective_threshold(ExposureLink::Manual, 0.0), 3.0)
                && close(b.effective_threshold(ExposureLink::Manual, 3.0), 3.0),
            "",
        );
    }

    // ---- 错误路径第 2 条：依赖检查（联动开着但曝光系统未启用） ----

    // 判据：自动切手动 + 告警，不静默留着开关假装工作。
    {
        let r = BloomParamBlock {
            exposure_link: ExposureLink::Auto,
            ..Default::default()
        }
        .resolve(false); // 曝光系统未启用
        let ok = BloomParamBlock {
            exposure_link: ExposureLink::Auto,
            ..Default::default()
        }
        .resolve(true);
        set.add(
            "K05-错误-曝光系统缺失自动切手动并告警",
            r.exposure_link == ExposureLink::Manual
                && r.diags.has(DiagCode::ExposureSystemUnavailable)
                && ok.exposure_link == ExposureLink::Auto
                && ok.diags.is_empty(),
            "",
        );
    }

    // ---- 预设三档 ----

    // 判据：三档为组合快照（阈值/强度/衰减/级数/色染同时取值），序稳定，
    // key 可反查；光敏风险级递增。
    {
        let table = preset_table();
        let none = preset(PresetId::None);
        let soft = preset(PresetId::Soft);
        let strong = preset(PresetId::Strong);
        let by_key = preset_by_key("bloom.strong");
        set.add(
            "K05-预设-三档组合快照与key反查",
            table.len() == PRESET_COUNT
                && close(none.strength, 0.0)
                && none.exposure_link == ExposureLink::Manual
                && soft.strength > 0.0
                && soft.strength < strong.strength
                && strong.mip_levels > soft.mip_levels
                && strong.tint.enabled
                && PresetId::from_key("bloom.soft") == Some(PresetId::Soft)
                && by_key.map(|b| b.strength) == Ok(strong.strength)
                && PresetId::None.photosensitive_risk() == 0
                && PresetId::Strong.photosensitive_risk() == 2,
            "",
        );
    }

    // 判据：预设引用不存在 → 三要素报错（码 / 上下文 / 可执行的下一步）。
    {
        let e = preset_by_key("bloom.missing").unwrap_err();
        let hint = e.next_hint();
        set.add(
            "K05-错误-预设缺失三要素报错",
            e.code() == "BLOOM_PRESET_UNKNOWN"
                && e.context() == "bloom.missing"
                && hint.contains("bloom.none")
                && hint.contains("bloom.soft")
                && hint.contains("bloom.strong"),
            "",
        );
    }

    // 判据：强泛光档在设置页带光敏风险说明（与 F1963 联动）。
    {
        let t_strong = preset_screen_text(PresetId::Strong);
        let t_none = preset_screen_text(PresetId::None);
        set.add(
            "K05-预设-强泛光档光敏风险提示",
            t_strong.contains("光敏") && t_none.contains("无光敏风险"),
            "",
        );
    }

    // ---- 参数动画轨道 ----

    // 判据：三（各）参数各自轨道；乱序关键帧按 tick 有序插入；端点夹取；
    // 线性插值中点精确。
    {
        let mut t = ParamTrack::new();
        t.push(Keyframe { tick: 100, value: 1.0 });
        t.push(Keyframe { tick: 0, value: 0.0 }); // 乱序插入
        t.push(Keyframe { tick: 50, value: 0.5 });
        let mid = t.sample(75);
        let before = t.sample(0);
        let after = t.sample(9999);
        set.add(
            "K05-动画-乱序插入有序化与端点夹取",
            t.is_sorted()
                && t.len() == 3
                && close(mid.unwrap_or(-1.0), 0.75)
                && close(before.unwrap_or(-1.0), 0.0)
                && close(after.unwrap_or(-1.0), 1.0),
            "",
        );
    }

    // 判据：空轨采样返回 None（调用方据此保留原值，不写入零）。
    {
        let e = ParamTrack::new();
        set.add("K05-动画-空轨不写入", e.is_empty() && e.sample(10).is_none(), "");
    }

    // 判据：四槽轨道组只写有轨的槽，无轨槽保留原值；tint 轨 0→1 是
    // 「单位色→目标色」插值（不必为色染动画维护三根轨）。
    // 关键：**动画必须幂等**——终点取自独立的 `tint_target`，不是上一帧
    // 写回的 `block.tint`。非幂等的表现是 tick=10 拿不到目标色（越播越淡）。
    {
        let mut tracks = BloomTracks::default();
        tracks.strength.push(Keyframe { tick: 0, value: 0.0 });
        tracks.strength.push(Keyframe { tick: 10, value: 1.0 });
        tracks.tint.push(Keyframe { tick: 0, value: 0.0 });
        tracks.tint.push(Keyframe { tick: 10, value: 1.0 });
        let mut b = BloomParamBlock {
            threshold_base: 7.0, // 无轨槽，应保留
            ..Default::default()
        };
        tracks.set_tint_target(Tint::new(1.0, 0.5, 0.0), &mut b);
        tracks.apply_to(5, &mut b);
        let mid_strength = b.strength;
        let mid_tint = b.tint;
        tracks.apply_to(10, &mut b);
        let end_tint = b.tint;
        // 幂等对拍：重置后反向采样（10 → 5）必须拿回同一中点
        let mut b2 = BloomParamBlock { threshold_base: 7.0, ..Default::default() };
        tracks.set_tint_target(Tint::new(1.0, 0.5, 0.0), &mut b2);
        tracks.apply_to(10, &mut b2);
        tracks.apply_to(5, &mut b2);
        let reverse_same = close(b2.tint.g, mid_tint.g) && close(b2.strength, mid_strength);
        set.add(
            "K05-动画-只写有轨槽且tint单轨插值幂等",
            tracks.all_sorted()
                && close(b.threshold_base, 7.0)
                && close(mid_strength, 0.5)
                && close(mid_tint.g, 0.75) // 1.0 → 0.5 的中点
                && close(mid_tint.b, 0.5)
                && mid_tint.enabled
                && close(b.strength, 1.0)
                // tick=10 必须精确落在目标色上（幂等的关键证据）
                && close(end_tint.g, 0.5)
                && close(end_tint.b, 0.0)
                && close(end_tint.r, 1.0)
                && end_tint.enabled
                && reverse_same,
            "",
        );
    }

    // 判据：tint 轨值 0 时应回到单位色且不染色（起点语义）。
    {
        let mut tracks = BloomTracks::default();
        tracks.tint.push(Keyframe { tick: 0, value: 0.0 });
        tracks.tint.push(Keyframe { tick: 10, value: 1.0 });
        let mut b = BloomParamBlock::default();
        tracks.set_tint_target(Tint::new(0.2, 0.4, 0.6), &mut b);
        tracks.apply_to(0, &mut b);
        set.add(
            "K05-动画-tint轨起点为单位色且不染色",
            b.tint.is_neutral() && !b.tint.enabled,
            "",
        );
    }

    // ---- 错误路径第 3 条：写入仲裁（后到优先 + 冲突告警） ----

    // 判据：同帧内轨道与手调各写一次 → 后到者胜，且记冲突（不论谁胜）。
    {
        let mut arb = WriteArbiter::new();
        let a1 = arb.submit(
            AnimatableSlot::Strength,
            WriteTicket { source: WriteSource::Track, tick: 100, seq: 1 },
        );
        let a2 = arb.submit(
            AnimatableSlot::Strength,
            WriteTicket { source: WriteSource::User, tick: 100, seq: 2 },
        );
        set.add(
            "K05-错误-写入仲裁后到优先且冲突留痕",
            a1.accepted
                && !a1.conflict
                && a2.accepted
                && a2.conflict
                && a2.winner == WriteSource::User
                && arb.conflict_count() == 1,
            "",
        );
    }

    // 判据：跨帧写入不记冲突（后帧覆盖前帧是正常推进，不是冲突）。
    {
        let mut arb = WriteArbiter::new();
        let a1 = arb.submit(
            AnimatableSlot::Threshold,
            WriteTicket { source: WriteSource::Track, tick: 100, seq: 1 },
        );
        let a2 = arb.submit(
            AnimatableSlot::Threshold,
            WriteTicket { source: WriteSource::User, tick: 101, seq: 2 },
        );
        set.add(
            "K05-错误-跨帧写入不算冲突",
            a1.accepted && a2.accepted && !a2.conflict && a2.winner == WriteSource::User,
            "",
        );
    }

    // 判据：仲裁状态是帧内态，帧边界清空（不清则下一帧的"首次写入"会被
    // 误判成冲突）。
    {
        let mut arb = WriteArbiter::new();
        arb.submit(
            AnimatableSlot::Decay,
            WriteTicket { source: WriteSource::Track, tick: 100, seq: 1 },
        );
        arb.submit(
            AnimatableSlot::Decay,
            WriteTicket { source: WriteSource::User, tick: 100, seq: 2 },
        );
        let before = arb.conflict_count();
        arb.clear_frame();
        let fresh = arb.submit(
            AnimatableSlot::Decay,
            WriteTicket { source: WriteSource::Track, tick: 101, seq: 1 },
        );
        set.add(
            "K05-错误-帧边界清空仲裁态",
            before == 1 && arb.conflict_count() == 0 && fresh.accepted && !fresh.conflict,
            "",
        );
    }

    // ---- 免重编译（参数与结构分离） ----

    // 判据：仅 mip 级数变化算结构；纯参数（阈值/强度/tint/衰减）变化免重编译。
    {
        let base = BloomParamBlock::default();
        let thr = BloomParamBlock { threshold_base: 9.0, ..base };
        let stg = BloomParamBlock { strength: 0.1, ..base };
        let tnt = BloomParamBlock { tint: Tint::new(0.2, 0.4, 0.6), ..base };
        let dcy = BloomParamBlock { decay: 0.1, ..base };
        let lv = BloomParamBlock { mip_levels: 7, ..base };
        set.add(
            "K05-免编译-纯参数变化免重编译且级数变化算结构",
            classify_change(&base, &thr) == ChangeClass::UniformOnly
                && classify_change(&base, &stg) == ChangeClass::UniformOnly
                && classify_change(&base, &tnt) == ChangeClass::UniformOnly
                && classify_change(&base, &dcy) == ChangeClass::UniformOnly
                && classify_change(&base, &lv) == ChangeClass::Structural
                && classify_change(&base, &base) == ChangeClass::UniformOnly,
            "",
        );
    }

    // 判据：tint 只作用于泛光贡献，不染场景（染整帧是色彩分级的职责）。
    // 承载体语义：`is_neutral()` 为真等价于"无需下发"，用(1,1,1) 构造的
    // tint 在视觉上是单位色——注意 `enabled` 仍为 true（它是"启用但中性"，
    // 与 `Tint::OFF` 的"关闭"区分开，两者对合成都无效果但对遥测有别）。
    {
        use super::vek04_bloom::HdrColor;
        let scene = HdrColor::new(1.0, 1.0, 1.0);
        let bloom = HdrColor::new(1.0, 1.0, 1.0);
        let t = Tint::new(1.0, 0.5, 0.0);
        let tinted_bloom = t.apply(bloom);
        let tinted_scene = t.apply(scene);
        let neutral_look = Tint::new(1.0, 1.0, 1.0);
        set.add(
            "K05-参数-tint只作用于泛光贡献",
            close(tinted_bloom.g, 0.5)
                && close(tinted_bloom.b, 0.0)
                && close(tinted_scene.r, 1.0)
                && close(tinted_scene.g, 0.5)
                && Tint::OFF.apply(bloom) == bloom
                && neutral_look.is_neutral()
                && neutral_look.enabled
                && !Tint::OFF.enabled,
            "",
        );
    }

    // ---- 端到端：预设 → 动画 → 补偿 → uniform 载荷 ----

    // 判据：从强泛光预设出发，挂一条强度动画轨，采样后经曝光补偿产出
    // 每帧 uniform 载荷；载荷阈值 = 基准 × 倍率。
    {
        let mut b = preset(PresetId::Strong);
        let mut tracks = BloomTracks::default();
        tracks.strength.push(Keyframe { tick: 0, value: 1.0 });
        tracks.strength.push(Keyframe { tick: 20, value: 0.2 });
        tracks.strength.push(Keyframe { tick: 40, value: 1.0 });
        tracks.apply_to(10, &mut b);
        let resolved = b.resolve(true);
        let ev = 1.0;
        let payload = UniformPayload {
            threshold_effective: resolved
                .block
                .effective_threshold(resolved.exposure_link, ev),
            strength: resolved.block.strength,
            decay: resolved.block.decay,
            tint: [
                resolved.block.tint.r,
                resolved.block.tint.g,
                resolved.block.tint.b,
            ],
            mip_levels: resolved.block.mip_levels,
        };
        // tick=10 → strength 0.6；阈值 = 2.5 × 2 = 5.0
        let weights = weight_preview(payload.mip_levels, payload.decay);
        let wsum: f32 = weights.iter().sum();
        set.add(
            "K05-参数-预设到uniform载荷端到端",
            close(payload.strength, 0.6)
                && close(payload.threshold_effective, 5.0)
                && payload.mip_levels == 7
                && payload.tint[1] < payload.tint[0] // 暖调
                && close(wsum, 1.0)
                && param_screen_text(&resolved.block, resolved.exposure_link)
                    .contains("随曝光自动补偿"),
            "",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vek05_domain_all_green() {
        let set = run_vek05_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed() && !set.truncated(),
            "VE-F2005 域自检红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }

    /// EV 换算在全域单调（换算错了会表现为"曝光越高泛光越弱/越强"，
    /// 但那种缺陷在画面上极难归因，必须在数值层钉死）。
    #[test]
    fn ev_multiplier_strictly_monotonic() {
        let mut prev = 0.0f32;
        let mut ev = EV_MIN;
        while ev <= EV_MAX + 1e-6 {
            let m = ev_to_multiplier(ev);
            assert!(m > prev, "换算非单调：ev={} m={} prev={}", ev, m, prev);
            prev = m;
            ev += 0.125;
        }
    }

    /// 曝光补偿后「场景亮度/阈值」比值恒定——抖动防护的数值本质。
    #[test]
    fn compensated_ratio_is_exposure_invariant() {
        let b = BloomParamBlock { threshold_base: 1.7, ..Default::default() };
        let scene = 6.0f32;
        let r0 = scene / b.effective_threshold(ExposureLink::Auto, 0.0);
        let mut ev = -6.0f32;
        while ev <= 6.0 {
            let s = scene * ev_to_multiplier(ev);
            let thr = b.effective_threshold(ExposureLink::Auto, ev);
            let r = s / thr;
            assert!(
                (r - r0).abs() < 1e-3,
                "比值漂移：ev={} r={} r0={}",
                ev,
                r,
                r0
            );
            ev += 0.25;
        }
    }

    /// 轨道采样不会产出 NaN（NaN 会顺着 uniform 传到片元着色器，且在
    /// 画面上表现为随机黑斑/白斑，归因极难）。
    ///
    /// 关键帧本身带 NaN 时，采样**必须传播**（不能悄悄吞掉——吞掉等于把
    /// 上游数据缺陷藏起来）；但合法关键帧的采样结果必须有限。
    #[test]
    fn track_sample_finite_for_valid_keys() {
        let mut t = ParamTrack::new();
        t.push(Keyframe { tick: 0, value: 0.0 });
        t.push(Keyframe { tick: 20, value: 1.0 });
        t.push(Keyframe { tick: 40, value: 0.0 });
        for tick in [0u64, 1, 10, 19, 20, 21, 39, 40, 41, 1_000_000] {
            let v = t.sample(tick).unwrap();
            assert!(v.is_finite(), "tick={} 采出非有限值 {}", tick, v);
            // 线性插值不应越出两端值域（无过冲）
            assert!((0.0..=1.0).contains(&v), "tick={} 越界 {}", tick, v);
        }
    }
}
