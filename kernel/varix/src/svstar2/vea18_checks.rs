//! VE-F0018 · 域自检（判据逐条对应，见 `vea18_sampler.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 采样器状态的**预置全集**（过滤/寻址/各向异性组合） → `A18-预置-全集覆盖`
//! - 预置全集自身合法性 + 名称唯一 + 用途说明在册 → `A18-预置-自身合法`
//! - **状态去重**（同参共享；不起作用字段不参与键） → `A18-去重-相关性裁剪键`
//! - 去重不得新建状态、命中可共享 → `A18-去重-共享不新建`
//! - **非法组合拒绝**（含正确组合建议） → `A18-非法-拒绝带建议`
//! - 非法组合的错误码分类正确 → `A18-非法-错误码分类`
//! - 拒绝**零静默入账**（谁/何时/何码/何格式） → `A18-非法-入账可归因`
//! - **运行时兜底**（缺失→运行时构建） → `A18-兜底-运行时构建`
//! - 库膨胀→淘汰（LRU + 容量封顶） → `A18-淘汰-LRU封顶`
//! - 预置项永不被淘汰 → `A18-淘汰-预置不动`
//! - **采样器与纹理格式兼容矩阵** → `A18-矩阵-无缺格`
//! - 矩阵能力声明自洽（整数/深度/不可过滤浮点/无mip） → `A18-矩阵-能力自洽`
//! - 矩阵参与门禁（按格式筛预置） → `A18-矩阵-参与门禁`
//! - **各向异性档位说明**（几倍采样画质差多少） → `A18-各向异性-档位量化`
//! - 各向异性收益递减显式写明 + 未在册倍率标注外推 → `A18-各向异性-递减与外推`
//! - **E10 基线冻结对齐**（三态区分：对齐/漂移/未登记） → `A18-基线-三态区分`
//! - 预置集变更走流程（ADR + 双签 + 抬版本） → `A18-基线-变更走流程`
//! - **去重命中统计**（省了多少可查） → `A18-统计-去重收益可查`
//! - 预置热度可查（冷预置是裁剪候选） → `A18-统计-预置热度`
//! - **状态表读屏可达** → `A18-读屏-状态表`
//! - 性能声明 O(状态)（线性扫描、无嵌套二次方） → `A18-性能-O状态`

use super::vea18_sampler::*;
use crate::checks::CheckSet;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// VE-F0018 域自检。
pub fn run_vea18_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vea18");

    // ---- 预置全集 ----

    // 判据：预置全集覆盖过滤三档、寻址四档、各向异性多档、比较采样。
    {
        let filters = [Filter::Point, Filter::Linear, Filter::Bicubic];
        let addrs = [
            AddressMode::Repeat,
            AddressMode::MirrorRepeat,
            AddressMode::ClampToEdge,
            AddressMode::ClampToBorder,
        ];
        let mut cover = true;
        for f in filters.iter() {
            cover &= PRESET_TABLE.iter().any(|r| r.filter == *f);
        }
        for a in addrs.iter() {
            cover &= PRESET_TABLE.iter().any(|r| r.address[0] == *a);
        }
        for tier in [2u32, 4, 8, 16].iter() {
            cover &= PRESET_TABLE.iter().any(|r| r.aniso == *tier);
        }
        cover &= PRESET_TABLE.iter().any(|r| r.compare.is_some());
        set.add("A18-预置-全集覆盖", cover && PRESET_TABLE.len() >= 12, "");
    }

    // 判据：每条预置自身合法、名字唯一、用途说明在册（预置集是基线，不许含糊）。
    {
        let mut legal = true;
        let mut named = true;
        for row in PRESET_TABLE.iter() {
            let d = row.to_desc();
            legal &= validate(&d, TexFormat::Rgba8Unorm, DEFAULT_MAX_ANISOTROPY).is_ok();
            named &= !row.use_note.is_empty() && !row.key_name.is_empty();
        }
        let mut keys: Vec<&str> = PRESET_TABLE.iter().map(|r| r.key_name).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        set.add("A18-预置-自身合法", legal && named && keys.len() == before, "");
    }

    // ---- 状态去重 ----

    // 判据：去重键做相关性裁剪——只在不起作用字段上不同的两个描述符共享一状态。
    {
        let base = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::Repeat; 3])
            .with_lod(0.0, MIP_LOD_TOP, 0.0);
        let mut differ_irrelevant = base.clone();
        differ_irrelevant.border_color = [0.25, 0.5, 0.75, 1.0]; // 寻址无 ClampToBorder → 无效
        let mut differ_relevant = base.clone();
        differ_relevant.lod_bias = 0.5; // LOD 偏置在LOD 组非全零时起作用
        set.add(
            "A18-去重-相关性裁剪键",
            base.canonical_key() == differ_irrelevant.canonical_key()
                && base.key_hash() == differ_irrelevant.key_hash()
                && base.canonical_key() != differ_relevant.canonical_key(),
            "",
        );
    }

    // 判据：同参共享——第二次解析走去重而非新建；统计省下的状态与字节。
    {
        let mut lib = SamplerLibrary::new();
        let odd = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::ClampToEdge; 3])
            .with_anisotropy(4)
            .with_lod(0.0, 3.0, 0.25);
        let first = lib.resolve(&odd, TexFormat::Rgba8Unorm).expect("表外合法组合应兜底构建");
        assert!(matches!(first.origin, Origin::Runtime(_)), "首次必为运行时构建");
        let mut alias = odd.clone();
        alias.border_color = [7.0, 7.0, 7.0, 7.0]; // 无效字段差异
        let second = lib.resolve(&alias, TexFormat::Rgba8Unorm).expect("同参应共享");
        let shared = matches!(second.origin, Origin::Dedup(_)) && second.slot == first.slot;
        set.add(
            "A18-去重-共享不新建",
            shared && lib.runtime_len() == 1 && lib.stats.runtime_built == 1 && lib.stats.dedup_hits == 1,
            "",
        );
    }

    // ---- 非法组合拒绝 ----

    // 判据：每个拒绝都附「为什么非法」+「正确组合建议」，拒绝不是死胡同。
    {
        let mut all_suggested = true;
        let probes: [(SamplerDesc, TexFormat); 4] = [
            (
                SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::ClampToEdge; 3]),
                TexFormat::R8Uint,
            ),
            (
                SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::Repeat; 3]).with_anisotropy(3),
                TexFormat::Rgba8Unorm,
            ),
            (
                SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::ClampToBorder; 3]),
                TexFormat::Rgba8Unorm,
            ),
            (
                SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::ClampToEdge; 3]).with_lod(4.0, 2.0, 0.0),
                TexFormat::Rgba8Unorm,
            ),
        ];
        for (d, f) in probes.iter() {
            match validate(d, *f, DEFAULT_MAX_ANISOTROPY) {
                Ok(_) => all_suggested = false,
                Err(r) => {
                    all_suggested &= !r.reason.is_empty() && !r.suggestion.is_empty();
                    // 建议要么指向一个在**本请求格式下真能过校验**的预置，
                    // 要么明说"没有可用预置"——死胡同建议等于没指路。
                    if r.preset_hint.is_empty() {
                        all_suggested &= r.suggestion.contains("没有可用预置");
                    } else {
                        match preset_desc(r.preset_hint) {
                            None => all_suggested = false,
                            Some(p) => all_suggested &= validate(&p, *f, DEFAULT_MAX_ANISOTROPY).is_ok(),
                        }
                    }
                }
            }
        }
        set.add("A18-非法-拒绝带建议", all_suggested, "");
    }

    // 判据：非法组合错误码分类正确（每类错有专属码，不共用一码糊弄）。
    {
        let lin = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::ClampToEdge; 3]);
        let code = |d: &SamplerDesc, f: TexFormat, cap: u32| -> &'static str {
            match validate(d, f, cap) {
                Ok(_) => "E_NONE",
                Err(r) => r.code,
            }
        };
        let c_int = code(&lin, TexFormat::R8Uint, DEFAULT_MAX_ANISOTROPY);
        let c_depth = code(&lin, TexFormat::Depth32Float, DEFAULT_MAX_ANISOTROPY);
        let c_float = code(&lin, TexFormat::R32Float, DEFAULT_MAX_ANISOTROPY);
        let c_pow2 = code(
            &SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::Repeat; 3]).with_anisotropy(3),
            TexFormat::Rgba8Unorm,
            DEFAULT_MAX_ANISOTROPY,
        );
        let c_cap = code(
            &SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::Repeat; 3]).with_anisotropy(16),
            TexFormat::Rgba8Unorm,
            4,
        );
        let c_point = code(
            &SamplerDesc::new(Filter::Point, MipFilter::Nearest, [AddressMode::Repeat; 3]).with_anisotropy(4),
            TexFormat::Rgba8Unorm,
            DEFAULT_MAX_ANISOTROPY,
        );
        let c_border = code(
            &SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::ClampToBorder; 3]),
            TexFormat::Rgba8Unorm,
            DEFAULT_MAX_ANISOTROPY,
        );
        let c_lod = code(
            &SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::ClampToEdge; 3]).with_lod(4.0, 2.0, 0.0),
            TexFormat::Rgba8Unorm,
            DEFAULT_MAX_ANISOTROPY,
        );
        let c_mip = code(
            &SamplerDesc::new(Filter::Linear, MipFilter::Nearest, [AddressMode::Repeat; 3]).with_lod(1.0, 2.0, 0.0),
            TexFormat::R8Unorm1d,
            DEFAULT_MAX_ANISOTROPY,
        );
        let c_bicubic = code(
            &SamplerDesc::new(Filter::Bicubic, MipFilter::Nearest, [AddressMode::ClampToEdge; 3])
                .with_compare(CompareFunc::LessEqual),
            TexFormat::Depth32Float,
            DEFAULT_MAX_ANISOTROPY,
        );
        // 深度 PCF 合法（不是一律拒绝深度线性）。
        let pcf_ok = validate(&lin.with_compare(CompareFunc::LessEqual), TexFormat::Depth32Float, DEFAULT_MAX_ANISOTROPY).is_ok();
        let distinct = {
            let codes = [c_int, c_depth, c_float, c_pow2, c_cap, c_point, c_border, c_lod, c_mip, c_bicubic];
            let mut uniq: Vec<&str> = codes.to_vec();
            uniq.sort_unstable();
            uniq.dedup();
            uniq.len() == codes.len()
        };
        set.add(
            "A18-非法-错误码分类",
            c_int == "E_FILTER_INTEGER"
                && c_depth == "E_FILTER_DEPTH_NO_COMPARE"
                && c_float == "E_FILTER_UNFILTERABLE"
                && c_pow2 == "E_ANISO_NOT_POW2"
                && c_cap == "E_ANISO_OVER_CAP"
                && c_point == "E_ANISO_POINT_FILTER"
                && c_border == "E_BORDER_NOT_ENABLED"
                && c_lod == "E_LOD_RANGE_INVERTED"
                && c_mip == "E_MIP_UNSUPPORTED"
                && c_bicubic == "E_COMPARE_BICUBIC"
                && pcf_ok
                && distinct,
            "",
        );
    }

    // 判据：拒绝零静默入账（tick/请求摘要/错误码/格式/建议齐备），且不留下状态。
    {
        let mut lib = SamplerLibrary::new();
        let bad = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::ClampToEdge; 3]);
        let r = lib.resolve(&bad, TexFormat::R8Uint);
        let rec = lib.rejections.first();
        let accounted = r.is_err()
            && lib.stats.rejections == 1
            && lib.rejections.len() == 1
            && rec.map(|x| {
                x.code == "E_FILTER_INTEGER" && x.format == TexFormat::R8Uint && !x.request.is_empty() && !x.preset_hint.is_empty()
            })
            .unwrap_or(false);
        set.add(
            "A18-非法-入账可归因",
            accounted && lib.runtime_len() == 0 && lib.states.is_empty(),
            "",
        );
    }

    // ---- 运行时兜底 ----

    // 判据：预置未覆盖的合法组合走运行时构建，不报错、不静默丢弃。
    {
        let mut lib = SamplerLibrary::new();
        let odd = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::MirrorRepeat; 3])
            .with_anisotropy(8)
            .with_lod(0.5, 2.5, -0.25);
        let r = lib.resolve(&odd, TexFormat::Rgba8Unorm).expect("合法组合必须兜底");
        let built = matches!(r.origin, Origin::Runtime(_)) && lib.runtime_len() == 1 && lib.stats.runtime_built == 1;
        // 兜底状态可查（描述符完整留档，含 LOD 偏置这类非平凡参数）。
        let kept = lib
            .states
            .first()
            .map(|s| s.desc == odd && s.key == odd.canonical_key() && s.refs == 1)
            .unwrap_or(false);
        // 释放引用可归因。
        let rel = lib.release(r.slot);
        set.add("A18-兜底-运行时构建", built && kept && rel, "");
    }

    // ---- 库膨胀淘汰 ----

    // 判据：动态区容量封顶；溢出时按 LRU 淘汰最冷项。
    {
        let mut lib = SamplerLibrary::new();
        lib.set_device_max_anisotropy(8);
        // 偏置非零 → 键与任何预置都不撞，保证 CAP 条全部走运行时构建。
        for i in 0..LIBRARY_CAP {
            let d = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::Repeat; 3])
                .with_lod(0.0, 1.0 + i as f32, i as f32 * 0.25);
            assert!(lib.resolve(&d, TexFormat::Rgba8Unorm).is_ok(), "第 {} 条应兜底", i);
        }
        assert_eq!(lib.runtime_len(), LIBRARY_CAP);
        // 最冷项是槽0（最早创建、last_used 最小、refs=1）。
        let cold_before = lib.states[0].desc.max_lod;
        let fresh = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::Repeat; 3]).with_lod(0.0, 42.0, 77.5);
        assert!(lib.resolve(&fresh, TexFormat::Rgba8Unorm).is_ok());
        let capped = lib.runtime_len() == LIBRARY_CAP && lib.runtime_free() == 0;
        let evicted_cold = !lib.states.iter().any(|s| s.desc.max_lod == cold_before);
        set.add(
            "A18-淘汰-LRU封顶",
            capped && lib.stats.evictions == 1 && evicted_cold && lib.stats.eviction_saved_bytes() == SAMPLER_STATE_BYTES,
            "",
        );
    }

    // 判据：预置是基线，淘汰只发生在动态区——预置永不被淘汰。
    {
        let mut lib = SamplerLibrary::new();
        let preset_len = PRESET_TABLE.len();
        for i in 0..(LIBRARY_CAP + 4) {
            let d = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::Repeat; 3])
                .with_lod(0.0, 1.0 + i as f32, 0.0);
            let _ = lib.resolve(&d, TexFormat::Rgba8Unorm);
        }
        let p = preset_desc("trilinear_repeat").expect("预置在册");
        let r = lib.resolve(&p, TexFormat::Rgba8Unorm).expect("预置应始终可用");
        set.add(
            "A18-淘汰-预置不动",
            PRESET_TABLE.len() == preset_len && matches!(r.origin, Origin::Preset(_)) && r.slot == usize::MAX,
            "",
        );
    }

    // ---- 兼容矩阵 ----

    // 判据：矩阵无缺格——未登记格式按最保守处理（缺格会让未声明能力被默认放行）。
    {
        let fmts = [
            TexFormat::Rgba8Unorm,
            TexFormat::Bc1,
            TexFormat::Bc3,
            TexFormat::Bc7,
            TexFormat::R8Uint,
            TexFormat::R32Float,
            TexFormat::Rgba32Float,
            TexFormat::Depth32Float,
            TexFormat::Depth24UnormStencil8,
            TexFormat::R8Unorm1d,
        ];
        let full = fmts.iter().all(|f| FORMAT_MATRIX.iter().any(|(m, _)| m == f)) && FORMAT_MATRIX.len() == fmts.len();
        set.add("A18-矩阵-无缺格", full, "");
    }

    // 判据：矩阵能力声明自洽——整数不可过滤、深度仅比较可线性、不可过滤浮点显式关闭。
    {
        let mut coherent = true;
        for (f, c) in FORMAT_MATRIX.iter() {
            if c.integer {
                coherent &= !c.linear_filter && !c.linear_compare;
            }
            if c.depth {
                coherent &= !c.linear_filter && c.linear_compare;
            }
            coherent &= f.caps() == *c;
        }
        coherent &= !TexFormat::R32Float.caps().linear_filter;
        coherent &= !TexFormat::Rgba32Float.caps().linear_filter;
        coherent &= !TexFormat::R8Unorm1d.caps().mips;
        coherent &= TexFormat::Rgba8Unorm.caps().linear_filter;
        set.add("A18-矩阵-能力自洽", coherent, "");
    }

    // 判据：矩阵参与门禁——同一预置对不同格式结论不同（矩阵在筛，不是摆设）。
    {
        let mut lib = SamplerLibrary::new();
        let lin = preset_desc("trilinear_repeat").expect("预置在册");
        let pt = preset_desc("nearest_clamp").expect("预置在册");
        let color_ok = lib.resolve(&lin, TexFormat::Rgba8Unorm).is_ok();
        let int_rejected = lib.resolve(&lin, TexFormat::R8Uint).is_err();
        let int_pt_ok = lib.resolve(&pt, TexFormat::R8Uint).is_ok();
        // 阴影预置：深度格式放行，颜色格式走运行时构建（其比较语义对颜色格式不成立）。
        let pcf = preset_desc("shadow_pcf_linear").expect("预置在册");
        let depth_ok = lib.resolve(&pcf, TexFormat::Depth32Float).is_ok();
        set.add(
            "A18-矩阵-参与门禁",
            color_ok && int_rejected && int_pt_ok && depth_ok,
            "",
        );
    }

    // ---- 各向异性档位说明 ----

    // 判据：档位量化「几倍采样画质差多少」——锐度递增、带宽递增、说明在册。
    {
        let mut ordered = true;
        let mut prev_sharp = 0u32;
        let mut prev_bw = 0u32;
        for (tier, sharp, bw, note) in ANISO_TIERS.iter() {
            ordered &= *sharp > prev_sharp && *bw > prev_bw && !note.is_empty() && *tier > 0;
            prev_sharp = *sharp;
            prev_bw = *bw;
        }
        let queried = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::Repeat; 3])
            .with_anisotropy(4)
            .aniso_tier();
        ordered &= queried.0 == 4 && queried.1 == 80 && !queried.2.is_empty();
        set.add("A18-各向异性-档位量化", ordered, "");
    }

    // 判据：收益递减显式写明（不值就别开）；未在册倍率标注外推不冒充在册数据。
    {
        let linear = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::Repeat; 3]);
        let (_, s8, note8) = linear.clone().with_anisotropy(8).aniso_tier();
        let (_, s16, note16) = linear.clone().with_anisotropy(16).aniso_tier();
        let (_, _, note32) = linear.with_anisotropy(32).aniso_tier();
        set.add(
            "A18-各向异性-递减与外推",
            s16 - s8 <= 6 && note16.contains("收益递减") && note32.contains("外推") && !note8.is_empty(),
            "",
        );
    }

    // ---- E10 基线冻结对齐 ----

    // 判据：三态严格区分（对齐 / 漂移须重签 / 未登记）——处置方向相反者不共用码。
    {
        let actual = compute_preset_hash();
        let st = verify_baseline();
        let (right_tag, only_one_true) = match st {
            BaselineStatus::Unregistered { actual: a } => (a == actual && PRESET_BASELINE_HASH == 0, !st.is_aligned()),
            BaselineStatus::Aligned => (PRESET_BASELINE_HASH == actual, st.is_aligned()),
            BaselineStatus::Drifted { expected, actual: a } => (expected != a && actual == a, !st.is_aligned()),
        };
        // 指纹必须随预置表内容变化而变化（不是常量装饰）。
        let mut probe = PRESET_TABLE[0].to_desc();
        let h0 = probe.key_hash();
        probe.lod_bias = 0.125;
        let sensitive = probe.key_hash() != h0;
        set.add(
            "A18-基线-三态区分",
            right_tag && only_one_true && !st.describe().is_empty() && sensitive,
            "",
        );
    }

    // 判据：预置集变更走流程——ADR + 双签（两人不同）+ 抬版本号 + 摘要齐全，缺一不受理。
    {
        let mut lib = SamplerLibrary::new();
        let good = ChangeRequest {
            summary: "新增 aniso32 预置档".to_string(),
            adr: "ADR-VE-0181".to_string(),
            signer_a: "render-a".to_string(),
            signer_b: "render-b".to_string(),
            target_version: PRESET_BASELINE_VERSION + 1,
        };
        let accepted = matches!(lib.queue_preset_change(good.clone()), ChangeOutcome::Accepted { .. });

        let mut reject_code = |r: ChangeRequest| -> &'static str {
            match lib.queue_preset_change(r.clone()) {
                ChangeOutcome::Rejected { code, .. } => code,
                ChangeOutcome::Accepted { .. } => "E_NONE",
            }
        };
        let mut no_adr = good.clone();
        no_adr.adr = String::new();
        let mut same_signer = good.clone();
        same_signer.signer_b = same_signer.signer_a.clone();
        let mut no_b = good.clone();
        no_b.signer_b = String::new();
        let mut low_ver = good.clone();
        low_ver.target_version = PRESET_BASELINE_VERSION;
        let mut no_sum = good.clone();
        no_sum.summary = String::new();

        let gated = reject_code(no_adr) == "E_CHANGE_NO_ADR"
            && reject_code(same_signer) == "E_CHANGE_SAME_SIGNER"
            && reject_code(no_b) == "E_CHANGE_NO_DUAL_SIGN"
            && reject_code(low_ver) == "E_CHANGE_VERSION_NOT_RAISED"
            && reject_code(no_sum) == "E_CHANGE_NO_SUMMARY";
        set.add(
            "A18-基线-变更走流程",
            accepted && gated && lib.changes.len() == 1,
            "",
        );
    }

    // ---- 统计可查 ----

    // 判据：去重省了多少可查（状态数 + 折算字节）；预置热度可查。
    {
        let mut lib = SamplerLibrary::new();
        let odd = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::ClampToEdge; 3]).with_lod(0.0, 2.5, 0.1);
        for _ in 0..3 {
            let mut alias = odd.clone();
            alias.border_color = [1.0, 2.0, 3.0, 4.0]; // 无效字段
            assert!(lib.resolve(&alias, TexFormat::Rgba8Unorm).is_ok());
        }
        let p = preset_desc("aniso8_repeat").expect("预置在册");
        assert!(lib.resolve(&p, TexFormat::Rgba8Unorm).is_ok());
        let s = lib.stats;
        // 3 次请求：1 次运行时构建 + 2 次去重；另1 次预置命中。
        let counted = s.runtime_built == 1
            && s.dedup_hits == 2
            && s.preset_hits == 1
            && s.total_resolves() == 4
            && s.dedup_saved_states() == 2
            && s.dedup_saved_bytes() == 2 * SAMPLER_STATE_BYTES
            && s.preset_hit_pct() == 25;
        let coldest = lib.coldest_presets(3);
        // 热度可查 = 真断言排序契约，不是恒真式：
        //   1) 返回条数正确；
        //   2) 按命中数**升序**（最冷在前——裁剪候选的语义方向不能反）；
        //   3) 唯一被命中的预置（aniso8_repeat，1 次）**不得**出现在最冷三项里
        //      （它比所有 0 次的预置热；排到最冷位说明排序坏了）。
        let sorted_asc = coldest.windows(2).all(|w| w[0].1 <= w[1].1);
        let hot_excluded = !coldest.iter().any(|(k, _)| *k == "aniso8_repeat");
        let heat = coldest.len() == 3 && sorted_asc && hot_excluded;
        set.add("A18-统计-去重收益可查", counted && heat, "");
    }

    // ---- 读屏可达 ----

    // 判据：状态表逐行可达（汇总 + 每预置 + 每运行时状态），人话摘要含关键数。
    {
        let mut lib = SamplerLibrary::new();
        let pcf = preset_desc("shadow_pcf_linear").expect("预置在册");
        assert!(lib.resolve(&pcf, TexFormat::Depth32Float).is_ok());
        let odd = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::ClampToEdge; 3]).with_lod(0.0, 2.5, 0.1);
        assert!(lib.resolve(&odd, TexFormat::Rgba8Unorm).is_ok());
        assert!(lib.resolve(&odd, TexFormat::Rgba8Unorm).is_ok());

        let rows = lib.state_table_rows();
        let shape = rows.len() == 3 + PRESET_TABLE.len() + 1;
        let nonempty = rows.iter().all(|r| !r.is_empty());
        let head = rows[0].contains("容量") && rows[1].contains("去重共享 1") && rows[2].contains("省下 1 个状态");
        let s = lib.screen_text();
        let spoken = s.contains("采样器状态库") && s.contains("去重省 1 个状态") && s.contains("基线");
        set.add("A18-读屏-状态表", shape && nonempty && head && spoken, "");
    }

    // ---- 性能声明 O(状态) ----

    // 判据：性能逐项分解 O(状态)——预置扫描与动态扫描均单层线性，无嵌套二次方。
    //
    // 本项**实测真实工作量**，不用「n × 常数 = n × 常数 × 4」这类自证式算术
    // （那只是验证了乘法律，与解析路径无关）。做法：用 [`scan_work`] 计数器
    // 分别统计「可过滤格式」与「不可过滤格式」下的预置判定次数。
    //
    // 为什么这两个格式的对比能抓住二次方：不可过滤格式（R8_UINT）下几乎每条
    // 预置都判失败。旧实现用 `validate` 筛兼容性，每次失败再进 `suggest_preset`
    // 全表扫一遍 → 判定次数 ≈ 预置数²；修复后用 `classify` 纯判定 →
    // 判定次数 ≈ 预置数。故「不可过滤格式 ÷ 可过滤格式」应≈ 1（线性），
    // 一旦退化成二次方，该比值会随预置数放大而显著 > 1。
    {
        let scan_work = |fmt: TexFormat| -> u32 {
            let mut lib = SamplerLibrary::new();
            lib.preset_scan_work = 0;
            // 在该格式下反复解析表外合法组合：每次解析都要扫一遍预置表。
            let base = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::ClampToEdge; 3]);
            let probes: &[SamplerDesc] = if fmt == TexFormat::R8Uint {
                // 整数格式只准 Point→ 用 Point 变体确保请求本身合法（否则门1
                // 就拒了，压根走不到预置扫描，测不到门2 的工作量）。
                &[SamplerDesc::new(Filter::Point, MipFilter::Linear, [AddressMode::ClampToEdge; 3])]
            } else {
                &[base]
            };
            for d in probes.iter() {
                for i in 0..8usize {
                    let mut v = d.clone();
                    v.lod_bias = 0.01 * (i as f32 + 1.0); // 表外组合 → 必走门2 扫描
                    let _ = lib.resolve(&v, fmt);
                }
            }
            lib.preset_scan_work
        };

        let filterable = scan_work(TexFormat::Rgba8Unorm);
        let unfilterable = scan_work(TexFormat::R8Uint);

        // 线性判据：两格式的预置判定次数应同量级（比值 ≤ 4）。
        // 二次方实现下，不可过滤格式因「每次失败再全表扫」而 ≈ 预置数 × 预置数，
        // 与可过滤格式拉开一个预置数倍（18×）的差距，比值必然远超 4。
        let ratio_ok = filterable > 0 && unfilterable <= filterable * 4;

        // 并保留真实解析的功能断言：32 次表外合法组合全部兜底成功、动态区不超容。
        let mut lib = SamplerLibrary::new();
        let mut built = 0usize;
        for i in 0..32usize {
            let d = SamplerDesc::new(Filter::Linear, MipFilter::Linear, [AddressMode::Repeat; 3])
                .with_lod(0.0, 1.0 + i as f32, 0.0);
            if lib.resolve(&d, TexFormat::Rgba8Unorm).is_ok() {
                built += 1;
            }
        }
        set.add(
            "A18-性能-无嵌套二次方",
            ratio_ok && built == 32 && lib.runtime_len() <= LIBRARY_CAP,
            "",
        );
        // 旧项名保留（聚合台账按名引用），语义收敛为「工作量线性 + 容量封顶」。
        set.add(
            "A18-性能-O状态",
            ratio_ok && built == 32 && lib.runtime_len() <= LIBRARY_CAP && SAMPLER_STATE_BYTES > 0,
            "",
        );
    }

    set
}