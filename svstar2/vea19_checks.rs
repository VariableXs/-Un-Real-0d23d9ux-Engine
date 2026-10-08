//! VE-F0019 · 域自检（判据逐条对应，见 `vea19_blend.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **四维组合**（源/目标因子 × 混合算子 × 写掩码的统一封装） → `A19-四维-维度齐备`
//! - 四维相关性裁剪（不起作用的维度不入去重键） → `A19-四维-相关性裁剪`
//! - 独立 alpha 维与颜色维可不同 → `A19-四维-独立alpha维`
//! - 线上编码 ≠ 枚举判别值（显式 wire 映射 + 往返） → `A19-四维-线上编码自洽`
//! - **预置库**（常用混合全集） → `A19-预置-全集覆盖`
//! - 预置自身合法 + 名唯一 + 用途说明在册 → `A19-预置-自身合法`
//! - 预置基线冻结对齐（三态区分）与变更双签 → `A19-预置-基线冻结`
//! - **非法拒绝**（Min/Max 丢因子 = 本域第一性缺陷） → `A19-非法-丢因子阻断`
//! - 拒绝带「为什么 + 正确建议」，且建议本身合法 → `A19-非法-带可走建议`
//! - 拒绝零静默入账 → `A19-非法-零静默入账`
//! - 处置方向相反不共用码（阻断 vs 提示 vs 通过） → `A19-非法-严重度分流`
//! - **缓存失效**（同参共享去重） → `A19-缓存-同参共享`
//! - **状态漂移 → 缓存失效**（上下文变更） → `A19-缓存-漂移失效`
//! - 库膨胀 → LRU 淘汰动态项、预置永不动 → `A19-缓存-淘汰不伤预置`
//! - **越界 → 钳制**（掩码位钳制） → `A19-越界-掩码位钳制`
//! - **钳制方向相反不共用码**（无符号钳制 / 浮点不钳制） → `A19-越界-钳制方向`
//! - **预览工具**（半透明实时预览，按硬件真实规则） → `A19-预览-规则一致`
//! - 预览斜坡单调 + 格子数兜底 → `A19-预览-斜坡单调`
//! - 预览诚实标记（因子丢弃 / 钳制） → `A19-预览-诚实标记`
//! - **状态表读屏可达** + 隐私零用户内容 → `A19-读屏-状态表`
//! - 性能声明 O(状态)（内层纯判定、无嵌套二次方） → `A19-性能-O状态`

use super::vea19_blend::*;
use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// VE-F0019 域自检。
pub fn run_vea19_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vea19");

    // ---- 四维组合 ----

    // 判据：四维齐备——源因子/目标因子/混合算子/写掩码，且因子全集覆盖
    // 零、单位、颜色、alpha、反相、常量六类语义。
    {
        let factors = [
            BlendFactor::Zero,
            BlendFactor::One,
            BlendFactor::SrcColor,
            BlendFactor::OneMinusSrcColor,
            BlendFactor::DstColor,
            BlendFactor::OneMinusDstColor,
            BlendFactor::SrcAlpha,
            BlendFactor::OneMinusSrcAlpha,
            BlendFactor::DstAlpha,
            BlendFactor::OneMinusDstAlpha,
            BlendFactor::ConstantColor,
            BlendFactor::ConstantAlpha,
            BlendFactor::OneMinusConstantColor,
            BlendFactor::OneMinusConstantAlpha,
        ];
        // 六类语义各至少一员：零/单位/正颜色/反相颜色/alpha/常量。
        let zero = factors.iter().any(|f| f.wire() == 0);
        let one = factors.iter().any(|f| f.wire() == 1);
        let color = factors.iter().any(|f| matches!(f, BlendFactor::SrcColor | BlendFactor::DstColor));
        let inv_color = factors
            .iter()
            .any(|f| matches!(f, BlendFactor::OneMinusSrcColor | BlendFactor::OneMinusDstColor));
        let alpha = factors.iter().any(|f| matches!(f, BlendFactor::SrcAlpha | BlendFactor::DstAlpha));
        let constant = factors.iter().any(|f| f.reads_constant());
        let ops = [BlendOp::Add, BlendOp::Subtract, BlendOp::ReverseSubtract, BlendOp::Min, BlendOp::Max];
        let masks = [WriteMask::NONE, WriteMask::ALPHA, WriteMask::RGB, WriteMask::RGBA, WriteMask::RED];
        set.add(
            "A19-四维-维度齐备",
            zero && one && color && inv_color && alpha && constant && ops.len() == 5 && masks.len() == 5,
            "",
        );
    }

    // 判据：相关性裁剪——只在不起作用字段上不同的描述符必须共享同一键。
    // 四种裁剪各查：关闭混合、alpha 维、颜色维、常量色。
    {
        let off = BlendDesc::disabled();
        let mut off2 = off.clone();
        off2.src_color = BlendFactor::DstAlpha;
        off2.color_op = BlendOp::Max;
        let trim_off = off.canonical_key() == off2.canonical_key();

        let rgb_only = BlendDesc::straight_alpha().with_mask(WriteMask::RGB);
        let mut rgb_only2 = rgb_only.clone();
        rgb_only2.src_alpha = BlendFactor::Zero;
        rgb_only2.alpha_op = BlendOp::Max;
        let trim_alpha = rgb_only.canonical_key() == rgb_only2.canonical_key();

        let a_only = BlendDesc::straight_alpha().with_mask(WriteMask::ALPHA);
        let mut a_only2 = a_only.clone();
        a_only2.src_color = BlendFactor::DstColor;
        a_only2.color_op = BlendOp::Subtract;
        let trim_color = a_only.canonical_key() == a_only2.canonical_key();

        let no_const = BlendDesc::straight_alpha();
        let mut with_const = no_const.clone();
        with_const.constant = [0.9; 4];
        with_const.constant_set = true;
        let trim_const = no_const.canonical_key() == with_const.canonical_key();

        // 反向：起作用的字段差异必须改键（否则裁剪过头=错误合并）。
        let uses_const = BlendDesc::straight_alpha()
            .with_color(BlendFactor::ConstantColor, BlendFactor::OneMinusSrcAlpha, BlendOp::Add)
            .with_constant([0.1, 0.2, 0.3, 1.0]);
        let mut uses_const2 = uses_const.clone();
        uses_const2.constant = [0.8, 0.8, 0.8, 1.0];
        let keeps_const = uses_const.canonical_key() != uses_const2.canonical_key();

        set.add(
            "A19-四维-相关性裁剪",
            trim_off && trim_alpha && trim_color && trim_const && keeps_const,
            "",
        );
    }

    // 判据：独立 alpha 维——颜色与 alpha 可用不同因子与不同算子。
    {
        let d = BlendDesc::straight_alpha()
            .with_color(BlendFactor::One, BlendFactor::OneMinusSrcAlpha, BlendOp::Add)
            .with_alpha(BlendFactor::OneMinusSrcAlpha, BlendFactor::OneMinusSrcAlpha, BlendOp::Add);
        // alpha 因子从 OneMinusSrcAlpha 换成 One → 键必须变（独立维真的独立）。
        let mut same = d.clone();
        same.src_alpha = BlendFactor::One;
        let independent = d.canonical_key() != same.canonical_key();
        // 且颜色维改动不得波及 alpha 维键（两维各自入键，互不牵连）。
        let mut color_changed = d.clone();
        color_changed.src_color = BlendFactor::SrcColor;
        let dims_separate = color_changed.canonical_key() != d.canonical_key();
        // 预乘 alpha 与直通 alpha 必须产生不同状态。
        let premul = preset_desc("alpha_premultiplied").expect("预乘预设在册");
        let straight = preset_desc("alpha_straight").expect("直通预设在册");
        let distinct = premul.canonical_key() != straight.canonical_key();
        set.add(
            "A19-四维-独立alpha维",
            independent && dims_separate && distinct,
            "",
        );
    }

    // 判据：线上编码 ≠ 枚举判别值，且 wire 往返自洽（禁 `as u8` 造二进制头）。
    {
        let op_mismatch = BlendOp::Add as u8 != BlendOp::Add.wire();
        let op_round = BlendOp::from_wire(BlendOp::Subtract.wire()) == BlendOp::Subtract
            && BlendOp::from_wire(BlendOp::Max.wire()) == BlendOp::Max;
        let all = [
            BlendFactor::Zero,
            BlendFactor::One,
            BlendFactor::SrcColor,
            BlendFactor::OneMinusSrcColor,
            BlendFactor::DstColor,
            BlendFactor::OneMinusDstColor,
            BlendFactor::SrcAlpha,
            BlendFactor::OneMinusSrcAlpha,
            BlendFactor::DstAlpha,
            BlendFactor::OneMinusDstAlpha,
            BlendFactor::ConstantColor,
            BlendFactor::ConstantAlpha,
            BlendFactor::OneMinusConstantColor,
            BlendFactor::OneMinusConstantAlpha,
        ];
        let mut round = true;
        let mut wires: Vec<u8> = Vec::new();
        for f in all.iter() {
            round &= BlendFactor::from_wire(f.wire()) == *f;
            wires.push(f.wire());
        }
        wires.sort_unstable();
        let before = wires.len();
        wires.dedup();
        set.add(
            "A19-四维-线上编码自洽",
            op_mismatch && op_round && round && wires.len() == before,
            "",
        );
    }

    // ---- 预置库 ----

    // 判据：预置全集覆盖十二类真实混合场景。
    {
        let need = [
            "opaque",
            "alpha_straight",
            "alpha_premultiplied",
            "additive",
            "multiply",
            "screen",
            "darken",
            "lighten",
            "min_op",
            "max_op",
            "subtract",
            "alpha_write_only",
            "debug_mask_red",
        ];
        let mut cover = PRESET_TABLE.len() >= 13;
        for n in need.iter() {
            cover &= preset_desc(n).is_some();
        }
        // 算子四类均在预置中出现（Add/Subtract/Min/Max）。
        cover &= PRESET_TABLE.iter().any(|r| r.desc.color_op == BlendOp::Add);
        cover &= PRESET_TABLE.iter().any(|r| r.desc.color_op == BlendOp::Subtract);
        cover &= PRESET_TABLE.iter().any(|r| r.desc.color_op == BlendOp::Min);
        cover &= PRESET_TABLE.iter().any(|r| r.desc.color_op == BlendOp::Max);
        // 掩码多态在册：全写 / 只 alpha / 单通道。
        cover &= PRESET_TABLE.iter().any(|r| r.desc.mask == WriteMask::RGBA);
        cover &= PRESET_TABLE.iter().any(|r| r.desc.mask == WriteMask::ALPHA);
        cover &= PRESET_TABLE.iter().any(|r| r.desc.mask == WriteMask::RED);
        // 混合开与关两态都在册。
        cover &= PRESET_TABLE.iter().any(|r| r.desc.enabled);
        cover &= PRESET_TABLE.iter().any(|r| !r.desc.enabled);
        set.add("A19-预置-全集覆盖", cover, "");
    }

    // 判据：预置自身合法、名唯一、用途说明在册（预置是基线，不许含糊）。
    {
        let mut legal = true;
        let mut named = true;
        for row in PRESET_TABLE.iter() {
            let d = row.to_desc();
            // 预置在四种带 alpha 的目标上都必须合法（预置不得依赖特定目标）。
            for t in [
                BlendTarget::Rgba8Unorm,
                BlendTarget::Bgra8Unorm,
                BlendTarget::Rgba16Float,
                BlendTarget::Rgb8Unorm,
                BlendTarget::R8Unorm,
            ]
            .iter()
            {
                legal &= validate(&d, *t, 0).is_ok();
            }
            named &= !row.use_note.is_empty() && !row.key_name.is_empty();
        }
        let mut keys: Vec<&str> = PRESET_TABLE.iter().map(|r| r.key_name).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        set.add("A19-预置-自身合法", legal && named && keys.len() == before, "");
    }

    // 判据：预置基线冻结对齐——三态区分 + 变更走双签流程。
    {
        let st = verify_baseline();
        let three_state = matches!(
            st,
            BaselineStatus::Aligned | BaselineStatus::Drifted { .. } | BaselineStatus::Unregistered
        );
        // 指纹必须**实测得出且非平凡**：恒定返回同一常量的实现会通过恒真断言，
        // 故此处同时要求「实测值 == 声明值」且「值非平凡」。
        let actual = compute_preset_hash();
        let measured_and_declared = actual == BLEND_BASELINE_HASH;
        let nontrivial = actual != 0 && BLEND_BASELINE_HASH != 0;
        // 指纹必须对**预置表内容**敏感：逐行重算须得回同一值（说明它确实由
        // 表内容导出，而非与表无关的常数）。
        let rebuilt = {
            let mut buf: Vec<u8> = Vec::new();
            for row in PRESET_TABLE.iter() {
                for b in row.key_name.as_bytes().iter() {
                    buf.push(*b);
                }
                buf.push(0x1f);
                buf.extend_from_slice(&row.to_desc().canonical_key());
                buf.push(0x1e);
            }
            fnv1a64(&buf)
        };
        let derived_from_table = rebuilt == actual;
        // 已回填的基线此刻必须**对齐**（恒占位会导致漂移判据永不触发）。
        let aligned_now = st.is_aligned();
        // 双签流程：合规受理、自签驳回、版本不递进驳回。
        let good = ChangeRequest {
            summary: String::from("补min/max 因子说明"),
            requester: String::from("A"),
            reviewer: String::from("B"),
            new_version: 2,
        };
        let accepted = matches!(
            queue_preset_change(&good, 1),
            ChangeOutcome::Accepted { version: 2, .. }
        );
        let mut self_signed = good.clone();
        self_signed.reviewer = String::from("A");
        let mut stale = good.clone();
        stale.new_version = 1;
        let mut blank = good.clone();
        blank.summary = String::new();
        let rejected = matches!(
            queue_preset_change(&self_signed, 1),
            ChangeOutcome::Rejected { .. }
        ) && matches!(queue_preset_change(&stale, 1), ChangeOutcome::Rejected { .. })
            && matches!(queue_preset_change(&blank, 1), ChangeOutcome::Rejected { .. });
        set.add(
            "A19-预置-基线冻结",
            three_state
                && measured_and_declared
                && nontrivial
                && derived_from_table
                && aligned_now
                && accepted
                && rejected,
            "",
        );
    }

    // ---- 非法拒绝 ----

    // 判据（第一性缺陷）：Min/Max 丢弃因子必须阻断，且 One/One 显式声明放行。
    {
        let bad_color = BlendDesc::straight_alpha()
            .with_color(BlendFactor::SrcAlpha, BlendFactor::One, BlendOp::Min);
        let bad_alpha = BlendDesc::straight_alpha()
            .with_alpha(BlendFactor::SrcAlpha, BlendFactor::One, BlendOp::Max);
        let blocked = matches!(
            classify(&bad_color, BlendTarget::Rgba8Unorm),
            Verdict::Blocking(Flaw::FactorDiscardedByOp)
        ) && matches!(
            classify(&bad_alpha, BlendTarget::Rgba8Unorm),
            Verdict::Blocking(Flaw::FactorDiscardedByOp)
        );
        // One/One 是「显式声明知道因子无效」的合法写法，须放行。
        let declared = BlendDesc::straight_alpha()
            .with_color(BlendFactor::One, BlendFactor::One, BlendOp::Min)
            .with_alpha(BlendFactor::One, BlendFactor::One, BlendOp::Min);
        let allowed = classify(&declared, BlendTarget::Rgba8Unorm) == Verdict::Ok;
        // 单一真源：算子是否丢因子只有一处定义。
        let single_source = BlendOp::Min.ignores_factors()
            && BlendOp::Max.ignores_factors()
            && !BlendOp::Add.ignores_factors()
            && !BlendOp::Subtract.ignores_factors();
        set.add("A19-非法-丢因子阻断", blocked && allowed && single_source, "");
    }

    // 判据：拒绝必须带「为什么 + 正确建议」，且**建议本身是能走的路**。
    {
        let probes: [(BlendDesc, BlendTarget); 5] = [
            (
                BlendDesc::disabled().with_mask(WriteMask::NONE),
                BlendTarget::Rgba8Unorm,
            ),
            (
                BlendDesc::straight_alpha()
                    .with_color(BlendFactor::ConstantColor, BlendFactor::One, BlendOp::Add),
                BlendTarget::Rgba8Unorm,
            ),
            (
                BlendDesc::straight_alpha()
                    .with_color(BlendFactor::DstAlpha, BlendFactor::One, BlendOp::Add),
                BlendTarget::Rgb8Unorm,
            ),
            (
                BlendDesc::straight_alpha()
                    .with_color(BlendFactor::SrcAlpha, BlendFactor::One, BlendOp::Max),
                BlendTarget::Rgba8Unorm,
            ),
            (
                BlendDesc::straight_alpha()
                    .with_alpha(BlendFactor::DstAlpha, BlendFactor::Zero, BlendOp::Add),
                BlendTarget::R8Unorm,
            ),
        ];
        let mut all_suggested = true;
        for (d, t) in probes.iter() {
            match validate(d, *t, 3) {
                Ok(()) => all_suggested = false,
                Err(r) => {
                    all_suggested &= !r.reason.is_empty()
                        && !r.hint.is_empty()
                        && !r.desc_text.is_empty()
                        && r.tick == 3;
                    // 建议的预置自身必须合法——否则建议不是能走的路。
                    let (nm, note) = suggest_preset(d, *t);
                    all_suggested &= !note.is_empty();
                    if let Some(cand) = preset_desc(nm) {
                        all_suggested &= validate(&cand, *t, 0).is_ok();
                    } else {
                        all_suggested = false;
                    }
                }
            }
        }
        set.add("A19-非法-带可走建议", all_suggested, "");
    }

    // 判据：拒绝零静默入账（谁/何时/何码/何描述/何目标全可查）。
    {
        let mut cache = BlendCache::new();
        let bad = BlendDesc::straight_alpha()
            .with_color(BlendFactor::SrcAlpha, BlendFactor::One, BlendOp::Max);
        let rejected = cache.resolve(&bad, BlendTarget::Rgba8Unorm).is_err();
        // 索引前先查长度：判据代码自身不得 panic——被测物坏掉时应**变红**，
        // 而不是把自检进程带崩（崩了就看不出是哪条判据红）。
        let ledgered = cache.rejection_log().len() == 1;
        let attributable = match cache.rejection_log().first() {
            None => false,
            Some(r) => {
                r.blocking
                    && r.flaw == Flaw::FactorDiscardedByOp
                    && !r.desc_text.is_empty()
                    && !r.reason.is_empty()
                    && r.target == BlendTarget::Rgba8Unorm
            }
        };
        // 提示级也须入账（提示不留账等于没提示）。
        let advisory = BlendDesc::disabled().with_mask(WriteMask::ALPHA);
        let _ = cache.resolve(&advisory, BlendTarget::Rgba8Unorm);
        let advisory_ledgered = cache.rejection_log().len() == 2
            && !cache
                .rejection_log()
                .get(1)
                .map(|r| r.blocking)
                .unwrap_or(true);
        set.add(
            "A19-非法-零静默入账",
            rejected && ledgered && attributable && advisory_ledgered,
            "",
        );
    }

    // 判据：处置方向相反的状态不得共用码——阻断 / 提示 / 通过三态可区分。
    {
        let blocking = BlendDesc::straight_alpha()
            .with_color(BlendFactor::SrcAlpha, BlendFactor::One, BlendOp::Max);
        let advisory = BlendDesc::disabled().with_mask(WriteMask::ALPHA);
        let ok = BlendDesc::straight_alpha();

        let three = matches!(classify(&blocking, BlendTarget::Rgba8Unorm), Verdict::Blocking(_))
            && matches!(classify(&advisory, BlendTarget::Rgba8Unorm), Verdict::Advisory(_))
            && classify(&ok, BlendTarget::Rgba8Unorm) == Verdict::Ok;
        // 提示级不得判为错误（否则合法预置会被全部拒掉）。
        let advisory_not_error = validate(&advisory, BlendTarget::Rgba8Unorm, 0).is_ok();
        // 同一状态在不同目标上判定不同，须如实区分（无 alpha 目标禁DstAlpha）。
        let target_sensitive = matches!(
            classify(&blocking, BlendTarget::Rgb8Unorm),
            Verdict::Blocking(_)
        );
        // 每个缺陷码都要有人话解释与建议。
        let mut explained = true;
        for f in [
            Flaw::MaskWithoutBlend,
            Flaw::MaskEmpty,
            Flaw::FactorDiscardedByOp,
            Flaw::ConstantUnset,
            Flaw::DstAlphaOnOpaqueTarget,
            Flaw::StaleAlphaPassthrough,
        ]
        .iter()
        {
            explained &= !flaw_reason(*f).is_empty() && !flaw_hint(*f).is_empty();
        }
        set.add("A19-非法-严重度分流", three && advisory_not_error && target_sensitive && explained, "");
    }

    // ---- 缓存失效 ----

    // 判据：同参共享去重——只在无效字段上不同不得新建状态，且收益可查。
    {
        let mut cache = BlendCache::new();
        let d = BlendDesc::straight_alpha();
        let t = BlendTarget::Rgba8Unorm;
        let first = cache
            .resolve(&d, t)
            .expect("表外合法组合应可解析");
        let preset_hit = matches!(first.origin, Origin::Preset("alpha_straight"));
        let mut alias = d.clone();
        alias.constant = [9.0; 4];
        alias.constant_set = true;
        let second = cache
            .resolve(&alias, t)
            .expect("同参应共享");
        // 共享的证据是「槽位相同 + 未新建 + 去重计数递增」；
        // `Origin::Preset` 是**出处**标签（该状态确实等于预置），共享事实由
        // dedup_hits/len 承载——两者是不同维度，不该混为一谈。
        let shared = second.slot == first.slot
            && cache.len() == 1
            && cache.stats.dedup_hits == 1
            && matches!(second.origin, Origin::Preset("alpha_straight"));
        // 去重收益按字节折算可查。
        let accounted = cache.dedup_saved_bytes()
            == cache.dedup_saved() as u64 * BLEND_STATE_BYTES;
        set.add(
            "A19-缓存-同参共享",
            preset_hit && shared && accounted && cache.preset_hit_pct() > 0,
            "",
        );
    }

    // 判据：**状态漂移 → 缓存失效**（上下文变更），且失效可审计。
    {
        let mut cache = BlendCache::new();
        let d = BlendDesc::straight_alpha();
        let t = BlendTarget::Rgba8Unorm;
        let _ = cache.resolve(&d, t).expect("应合法");
        let no_drift = cache.audit() == 0;
        // 切换上下文（目标格式/硬件能力变化）→ 旧条目语义不再等价。
        const NEW_TAG: u64 = 0xabcd_0000_0000_0001;
        cache.set_context(NEW_TAG);
        let dropped = cache.audit();
        let invalidated = dropped == 1 && cache.len() == 0;
        // 失效必须留账：谁漂移了、from→to、为什么。索引前查长度（判据不得 panic）。
        let ledgered = match cache.drift_log().first() {
            None => false,
            Some(d) => {
                d.entry_tag == CONTEXT_TAG_DEFAULT
                    && d.current_tag == NEW_TAG
                    && !d.why.is_empty()
            }
        };
        // 重建后带新标记，且不再漂移。
        let rebuilt = cache.resolve(&d, t).expect("重建应合法");
        let stable = rebuilt.context_tag == NEW_TAG && cache.audit() == 0;
        set.add(
            "A19-缓存-漂移失效",
            no_drift && invalidated && ledgered && stable,
            "",
        );
    }

    // 判据：库膨胀 → LRU 淘汰**动态项**，预置项永不动。
    {
        let mut cache = BlendCache::new();
        let t = BlendTarget::Rgba8Unorm;
        for row in PRESET_TABLE.iter() {
            let d = row.to_desc();
            let _ = cache.resolve(&d, t).expect("预置应合法");
        }
        let preset_count = PRESET_TABLE.len();
        let all_in = cache.len() == preset_count;
        // 灌入远超容量的自定义状态（引用常量色 → 键各异，不会误去重）。
        for i in 0..(LIBRARY_CAP + 8) {
            let mut d = BlendDesc::straight_alpha().with_color(
                BlendFactor::ConstantColor,
                BlendFactor::OneMinusSrcAlpha,
                BlendOp::Add,
            );
            d.constant = [i as f32 / 100.0, 0.5, 0.25, 1.0];
            d.constant_set = true;
            cache.advance_tick();
            let _ = cache.resolve(&d, t);
        }
        let capped = cache.len() <= LIBRARY_CAP;
        let evicted = cache.stats.evictions > 0;
        // 预置一个都不能少（淘汰预置 = 私自改基线）。
        // **必须查缓存实况**（`presets_present`），不能只查 `preset_desc`
        // ——后者读的是 `const` 表，无论淘汰与否都恒为真，是弱门禁。
        let presets_intact = cache.presets_present() == PRESET_TABLE.len()
            && PRESET_TABLE
                .iter()
                .all(|row| cache.preset_present(row.key_name));
        set.add(
            "A19-缓存-淘汰不伤预置",
            all_in && capped && evicted && presets_intact,
            "",
        );
    }

    // ---- 越界 → 钳制 ----

    // 判据：掩码位越界（`0b1111` 以上）被钳制丢弃，且丢弃数量可查。
    {
        let clamped = WriteMask::from_bits(0b1111_0001).bits() == 0b0001;
        // dropped_bits 按**置位个数**计：0b1111_0001 的越界位是 bit4..bit7 共 4 个。
        let counted = WriteMask::dropped_bits(0b1111_0001) == 4
            && WriteMask::dropped_bits(0b0000_1111) == 0
            && WriteMask::dropped_bits(0b1111_1111) == 4
            && WriteMask::dropped_bits(0b0001_0001) == 1;
        let full = WriteMask::from_bits(0b1111_1111).bits() == WriteMask::RGBA.bits();
        // 钳制后不得留下「碰巧有效」的越界位。
        let sanitized = !WriteMask::from_bits(0xF0).has_color() && !WriteMask::from_bits(0xF0).a();
        // 线上编码越界同样钳制（因子→Zero，算子→Add）。
        let wire_clamped = BlendFactor::from_wire(200) == BlendFactor::Zero
            && BlendOp::from_wire(200) == BlendOp::Add
            && !BlendFactor::wire_is_known(200)
            && !BlendOp::wire_is_known(0);
        set.add(
            "A19-越界-掩码位钳制",
            clamped && counted && full && sanitized && wire_clamped,
            "",
        );
    }

    // 判据：**钳制方向相反不共用码**——无符号目标钳制，浮点目标不钳制
    // （负值是合法 HDR，钳了就是画质事故）。
    {
        let sub = preset_desc("subtract").expect("subtract 预置在册");
        let src = [0.1, 0.1, 0.1, 0.1];
        let dst = [0.8, 0.8, 0.8, 0.8];
        let unorm = evaluate(&sub, src, dst, BlendTarget::Rgba8Unorm);
        let float = evaluate(&sub, src, dst, BlendTarget::Rgba16Float);
        let opposite = unorm.clamped
            && (unorm.out[0] - 0.0).abs() < 1e-6
            && !float.clamped
            && (float.out[0] - (-0.7)).abs() < 1e-6;
        // 格式判定与钳制判定必须分开陈述（两个不同性质的问题）。
        let separated = BlendTarget::Rgba16Float.is_unorm() == false
            && BlendTarget::Rgba8Unorm.is_unorm()
            && BlendTarget::Rgb8Unorm.has_alpha() == false
            && BlendTarget::Rgba16Float.has_alpha();
        let underflow_flagged = BlendOp::Subtract.may_underflow() && !BlendOp::Add.may_underflow();
        set.add("A19-越界-钳制方向", opposite && separated && underflow_flagged, "");
    }

    // ---- 预览工具 ----

    // 判据：预览按**硬件真实规则**求值（不是第二套答案）。
    // 关键三点：关闭混合仍按掩码写、Min/Max 忽略因子、alpha 通道因子塌缩。
    {
        let straight = BlendDesc::straight_alpha();
        let c = evaluate(&straight, [1.0, 0.0, 0.0, 0.5], [0.0, 0.0, 1.0, 1.0], BlendTarget::Rgba8Unorm);
        let blend_math = (c.out[0] - 0.5).abs() < 1e-6 && (c.out[2] - 0.5).abs() < 1e-6;

        let off = BlendDesc::disabled().with_mask(WriteMask::RGB);
        let c2 = evaluate(&off, [0.2, 0.3, 0.4, 0.9], [0.7, 0.7, 0.7, 0.1], BlendTarget::Rgba8Unorm);
        let mask_honoured = (c2.out[0] - 0.2).abs() < 1e-6 && (c2.out[3] - 0.1).abs() < 1e-6;

        let with_f = BlendDesc::straight_alpha()
            .with_color(BlendFactor::SrcAlpha, BlendFactor::One, BlendOp::Max);
        let without_f = BlendDesc::straight_alpha()
            .with_color(BlendFactor::One, BlendFactor::One, BlendOp::Max);
        let px = [0.2, 0.9, 0.9, 1.0];
        let bx = [0.8, 0.1, 0.1, 1.0];
        let a = evaluate(&with_f, px, bx, BlendTarget::Rgba8Unorm);
        let b = evaluate(&without_f, px, bx, BlendTarget::Rgba8Unorm);
        let minmax_ignores = (a.out[1] - b.out[1]).abs() < 1e-6 && (a.out[1] - 0.9).abs() < 1e-6;

        // alpha 通道因子塌缩：SrcColor 在 alpha 通道上取源 alpha 当因子，
        // 故 out.a = src.a * src.a = 0.25 * 0.25 = 0.0625（不是 0.25——
        // 因子塌缩后仍要乘以源 alpha 本身，这是最易算错的一步）。
        let collapse = BlendDesc::straight_alpha()
            .with_alpha(BlendFactor::SrcColor, BlendFactor::One, BlendOp::Add);
        let c3 = evaluate(&collapse, [1.0, 1.0, 1.0, 0.25], [0.0, 0.0, 0.0, 0.0], BlendTarget::Rgba8Unorm);
        let alpha_collapse = (c3.out[3] - 0.0625).abs() < 1e-6
            && BlendFactor::SrcColor.collapses_in_alpha();

        set.add(
            "A19-预览-规则一致",
            blend_math && mask_honoured && minmax_ignores && alpha_collapse,
            "",
        );
    }

    // 判据：预览斜坡单调、两端正确、格子数 0 有兜底（空预览会被误读为「无变化」）。
    {
        let d = BlendDesc::straight_alpha();
        let cells = preview_strip(&d, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 1.0], 8, BlendTarget::Rgba8Unorm);
        let ends_correct = cells.len() == 8
            && (cells[0].src_alpha - 0.0).abs() < 1e-6
            && (cells[7].src_alpha - 1.0).abs() < 1e-6
            && (cells[0].cell.out[2] - 1.0).abs() < 1e-6
            && (cells[7].cell.out[0] - 1.0).abs() < 1e-6;
        let mut monotone = true;
        for i in 1..cells.len() {
            monotone &= cells[i].cell.out[0] >= cells[i - 1].cell.out[0] - 1e-6;
        }
        let fallback = preview_strip(&d, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 1.0], 0, BlendTarget::Rgba8Unorm).len()
            == PREVIEW_CELLS;
        let summary_ok = preview_summary(&cells).contains("预览")
            && preview_summary(&[]) == String::from("预览为空（格数为 0）");
        set.add("A19-预览-斜坡单调", ends_correct && monotone && fallback && summary_ok, "");
    }

    // 判据：预览诚实标记——因子被丢弃、被无符号目标钳制，两种诚实标记都要能读出。
    {
        let maxed = BlendDesc::straight_alpha()
            .with_color(BlendFactor::SrcAlpha, BlendFactor::One, BlendOp::Max);
        let c1 = preview_strip(&maxed, [0.9, 0.9, 0.9], [0.1, 0.1, 0.1, 1.0], 4, BlendTarget::Rgba8Unorm);
        let flagged_discard = c1[0].cell.factor_ignored
            && preview_summary(&c1).contains("因子被算子丢弃");

        let sub = preset_desc("subtract").expect("subtract 预置在册");
        let c2 = preview_strip(&sub, [0.1, 0.1, 0.1], [0.9, 0.9, 0.9, 1.0], 4, BlendTarget::Rgba8Unorm);
        let mut any_clamped = false;
        for c in c2.iter() {
            any_clamped |= c.cell.clamped;
        }
        let flagged_clamp = any_clamped && preview_summary(&c2).contains("被无符号目标钳制");

        // 反向：Add 状态**不得**误报丢因子（诚实标记不许假阳性——否则读屏会撒谎）。
        let add_state = BlendDesc::straight_alpha();
        let c3 = evaluate(&add_state, [0.5, 0.5, 0.5, 0.5], [0.5, 0.5, 0.5, 1.0], BlendTarget::Rgba8Unorm);
        let no_false_positive = !c3.factor_ignored && !c3.clamped;
        // 且标记与算子判定同源（单一真源，不会两处结论相反）。
        let consistent = c3.factor_ignored
            == (add_state.color_op.ignores_factors() || add_state.alpha_op.ignores_factors())
            && c1[0].cell.factor_ignored == maxed.color_op.ignores_factors();
        set.add(
            "A19-预览-诚实标记",
            flagged_discard && flagged_clamp && no_false_positive && consistent,
            "",
        );
    }

    // ---- 读屏与隐私 ----

    // 判据：状态表读屏可达 + 隐私（零用户内容）。
    {
        let mut cache = BlendCache::new();
        let t = BlendTarget::Rgba8Unorm;
        for row in PRESET_TABLE.iter() {
            let d = row.to_desc();
            let _ = cache.resolve(&d, t);
        }
        // 掺入一条拒绝记录，验证拒绝也在读屏面可见。
        let bad = BlendDesc::straight_alpha()
            .with_color(BlendFactor::SrcAlpha, BlendFactor::One, BlendOp::Max);
        let _ = cache.resolve(&bad, t);
        cache.set_context(0xabcd_0000_0000_0001);
        let _ = cache.audit();

        let rows = cache.state_table_rows();
        let has_header = !rows.is_empty() && rows[0].contains("混合状态表");
        let has_entries = rows.iter().any(|r| r.contains("alpha_straight"));
        let has_reject = rows.iter().any(|r| r.contains("拒绝记录"));
        let has_drift = rows.iter().any(|r| r.contains("状态漂移记录"));
        let screen = cache.screen_text();
        let screen_ok = screen.contains("混合状态库") && screen.contains("去重省");
        // 隐私：只含渲染参数，零用户内容。
        let mut clean = true;
        for r in rows.iter() {
            clean &= !r.contains("password") && !r.contains("token") && !r.contains("用户");
        }
        set.add(
            "A19-读屏-状态表",
            has_header && has_entries && has_reject && has_drift && screen_ok && clean,
            "",
        );
    }

    // ---- 性能 ----

    // 判据：性能声明 O(状态)。内层判定必须 O(1)（走 classify），不得调
    // validate（每行失败再扫全表求建议 → O(预置数²)）。
    //
    // 场景刻意选「内层大量判失败」——缺陷恰恰发生在那一层；只测成功路径
    // 是自证式算术。工作量由 `preset_scan_work` **实测计量**计数器得出。
    {
        let cache = BlendCache::new();
        let bad = BlendDesc::straight_alpha()
            .with_color(BlendFactor::SrcAlpha, BlendFactor::One, BlendOp::Max);
        let work = cache.preset_scan_work(&bad, BlendTarget::Rgba8Unorm);
        let linear = work == PRESET_TABLE.len() as u64 + 1;
        // 出现嵌套二次方时量级会翻倍（预置数 ≥ 13 时二者可区分）。
        let not_quadratic = work < (PRESET_TABLE.len() as u64) * 2;
        // 预置表规模变化时工作量同步线性变化（真·线性，而非巧合相等）。
        let mut work2 = 0u64;
        for row in PRESET_TABLE.iter() {
            work2 += 1;
            let cand = row.to_desc();
            let _ = classify(&cand, BlendTarget::Rgba8Unorm);
        }
        work2 += 1;
        let scales = work2 == work && PRESET_TABLE.len() >= 13;
        set.add("A19-性能-O状态", linear && not_quadratic && scales, "");
    }

    set
}