//! VE-F2004 · 域自检（判据逐条对应，见 `vek04_bloom.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 三段管线（提取→mip 模糊→合成）→ `K04-三段-*`
//! - mip 级模糊链（5-7 级可选；dual-kawase 或高斯下采样-上采样对）→ `K04-链-*`
//! - F1849 源标记契约兑现（掩码提升提取物理准确性）→ `K04-兑现-*`
//! - 光敏联动（输出时序入 F1962 扫描，三源之一的光照动效源扩展）→ `K04-光敏-*`
//! - 错误路径六条（帧边界 / 量纲钳制 / 显存降级 / 契约缺失显性 / 软钳制）→
//!   `K04-错误-*`
//! - 管线序（bloom 在 TM 前，F2001 序声明执行）→ `K04-序位-*`
//! - RT 全走 F2003 池（分配请求 + 存活区间 + 帧边界回收）→ `K04-池-*`
//!
//! 零墙钟、零 IO，回归可复现。

use super::vek04_bloom::*;
use crate::checks::CheckSet;

use alloc::vec;
use alloc::vec::Vec;

/// 近似相等。
fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

/// 近似相等（权重和级，浮点误差更宽）。
fn close_w(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-5
}

/// VE-F2004 域自检。
pub fn run_vek04_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vek04");

    // ---- 第一段：明亮提取 ----

    // 判据：亮度阈值 + 软膝函数。低于阈值的像素提取贡献为 0；高于阈值的
    // 贡献非零且被归一化（提取率 ≤ 1）。
    {
        let p = BloomParams {
            threshold: 1.0,
            soft_knee: 0.0, // 硬阈值形态便于逐值断言
            mask_relax: 0.0,
            ..Default::default()
        };
        let dark = extract(HdrColor::new(0.5, 0.5, 0.5), 0.0, &p);
        let hot = extract(HdrColor::new(4.0, 4.0, 4.0), 0.0, &p);
        // 亮度 0.5 < 阈值 1.0 → 贡献 0
        // 亮度 4.0 ≥ 阈值 → 贡献 = 归一化后比例（(4-1)/4 = 0.75）
        set.add(
            "K04-三段-提取阈值判定与归一化",
            close(dark.rate, 0.0)
                && close(dark.color.r, 0.0)
                && close(hot.rate, 0.75)
                && close(hot.color.r, 3.0)
                && hot.rate <= 1.0,
            "",
        );
    }

    // 判据：软膝函数——过渡带内二次上升，两端斜率连续；knee=0 退化为硬阈值。
    {
        let knee = 0.5;
        let thr = 1.0;
        // 带外（低于 thr-knee）：贡献 0
        let below = soft_knee_contribution(0.3, thr, knee);
        // 带内（thr-knee 到 thr+knee 区间）：二次曲线
        let mid = soft_knee_contribution(1.0, thr, knee);
        let mid_expect = {
            let br = 1.0 - 1.0 + 0.5;
            br * br / (4.0 * 0.5)
        };
        // 带外（高于 thr+knee）：线性段 = b - thr
        let above = soft_knee_contribution(3.0, thr, knee);
        // knee=0 → 硬阈值：贡献 = b - thr
        let hard = soft_knee_contribution(3.0, thr, 0.0);
        // 单调性：贡献随亮度不减
        let mono = soft_knee_contribution(1.2, thr, knee) >= mid
            && soft_knee_contribution(2.0, thr, knee) >= soft_knee_contribution(1.2, thr, knee);
        set.add(
            "K04-三段-软膝二次过渡带与硬阈值退化",
            close(below, 0.0)
                && close(mid, mid_expect)
                && close(above, 2.0)
                && close(hard, 2.0)
                && mono,
            "",
        );
    }

    // 判据：提取端软钳制防炸帧（HDR 极亮值）——拐点以下恒等，以上渐近压缩；
    // 非有限输入被清洗。
    {
        let below = soft_clamp(SOFT_CLAMP_KNEE - 10.0);
        let at_knee = soft_clamp(SOFT_CLAMP_KNEE);
        let huge = soft_clamp(1.0e9);
        let nan_in = HdrColor::new(f32::NAN, f32::NEG_INFINITY, f32::INFINITY);
        set.add(
            "K04-错误-软钳制拐点恒等与渐近压缩",
            close(below, SOFT_CLAMP_KNEE - 10.0)
                && close(at_knee, SOFT_CLAMP_KNEE)
                && huge < SOFT_CLAMP_MAX * 2.0
                && huge.is_finite()
                && nan_in.r == 0.0
                && nan_in.g == 0.0
                && nan_in.b == SOFT_CLAMP_MAX,
            "",
        );
    }

    // ---- 第二段：mip 模糊链 ----

    // 判据：dual-kawase 分离核——下采样核与上采样核权重和**恒为 1.0**
    // （求和不为 1 会逐级漂移，整链变暗/变亮，是最难在单帧看出的缺陷）。
    {
        let ds = kernel_weight_sum(&DOWN_KERNEL);
        let us = kernel_weight_sum(&UP_KERNEL);
        set.add(
            "K04-链-分离核权重和恒等",
            close_w(ds, 1.0) && close_w(us, 1.0),
            "",
        );
    }

    // 判据：核抽头布局——down 为中心+四角（5 抽），up 为中心+轴向+对角
    // （9 抽，补偿降采样相位偏移，否则 mip 链留方块化）。
    {
        let down_center = DOWN_KERNEL[0];
        let down_ok = down_center.dx == 0
            && down_center.dy == 0
            && close(down_center.weight, 0.5)
            && DOWN_KERNEL.len() == 5;
        let up_ok = UP_KERNEL.len() == 9
            && close(UP_KERNEL[0].weight, 0.25)
            && UP_KERNEL.iter().skip(1).all(|t| t.dx == 0 || t.dy == 0 || {
                // 对角抽头权重为轴向的一半
                t.weight <= 0.125
            });
        set.add("K04-链-核抽头布局与相位补偿", down_ok && up_ok, "");
    }

    // 判据：单级模糊的能量守恒（纯色平场过 down 核，输出仍为该色——
    // 权重和为 1 的直接行为验证；采边夹取不产生暗环）。
    {
        let flat = |_x: i32, _y: i32| HdrColor::new(2.0, 2.0, 2.0);
        let out = blur_tap(&flat, 8, 8, false);
        let out_up = blur_tap(&flat, 8, 8, true);
        set.add(
            "K04-链-平场能量守恒（采边夹取无暗环）",
            close(out.r, 2.0) && close(out.g, 2.0) && close(out.b, 2.0)
                && close(out_up.r, 2.0),
            "",
        );
    }

    // 判据：mip 尺寸半分辨率起步逐级减半，下限钳到 1（不缩到 0）。
    {
        let d0 = mip_dim(1920, 0);
        let d1 = mip_dim(1920, 1);
        let d2 = mip_dim(1920, 2);
        // 极端尺寸：1px 主缓冲，7 级也不能出现 0
        let tiny = (0..8).map(|l| mip_dim(1, l)).collect::<Vec<_>>();
        let odd = mip_dim(1921, 0); // 奇数上取整 /2 = 961
        set.add(
            "K04-链-半分辨率起步与下限钳制",
            d0 == 960 && d1 == 480 && d2 == 240
                && tiny.iter().all(|d| *d >= MIP_DIM_FLOOR)
                && odd == 961,
            "",
        );
    }

    // 判据：级数 5-7 级可选（域内可配），[1,7] 越界钳制；单级为合法下界
    // （F2015 低档 Bloom 单级形态）。
    {
        let lv8 = BloomParams { mip_levels: 8, ..Default::default() }.resolve();
        let lv0 = BloomParams { mip_levels: 0, ..Default::default() }.resolve();
        let lv5 = BloomParams { mip_levels: 5, ..Default::default() }.resolve();
        let lv7 = BloomParams { mip_levels: 7, ..Default::default() }.resolve();
        set.add(
            "K04-链-级数可配5-7与越界钳制",
            lv5.params.mip_levels == 5
                && lv7.params.mip_levels == 7
                && lv8.params.mip_levels == MIP_LEVELS_MAX
                && lv8.diags.has(DiagCode::MipLevelsOutOfRange)
                && lv0.params.mip_levels == MIP_LEVELS_MIN,
            "",
        );
    }

    // 判据：半分辨率链使成本减半（主优化点实测）——像素面积比是几何级数，
    // 7 级时总面积为全分辨率的约 1/3（优于"减半"）。
    {
        let r5 = half_res_cost_ratio(5);
        let r7 = half_res_cost_ratio(7);
        set.add(
            "K04-链-半分辨率成本比（几何级数）",
            r5 < 0.35 && r7 < 0.34 && r7 > r5 && close(r7, 1.0 / 3.0),
            "",
        );
    }

    // ---- 第三段：合成 ----

    // 判据：多级加权叠加 + 强度全局参数。强度 0 → 场景原样（泛光关闭）。
    {
        let p = BloomParams {
            intensity: 0.0,
            composite_decay: 0.8,
            ..Default::default()
        };
        let scene = HdrColor::new(1.0, 1.0, 1.0);
        let mips = vec![HdrColor::new(0.5, 0.5, 0.5); 5];
        let off = composite(scene, &mips, &p);
        let p1 = BloomParams { intensity: 1.0, ..p };
        let on = composite(scene, &mips, &p1);
        set.add(
            "K04-三段-强度全局参数与叠加语义",
            close(off.r, 1.0)
                && on.r > 1.0
                && close(on.r - 1.0, 0.5),
            "",
        );
    }

    // 判据：合成权重归一化——`w_i = decay^i` 归一化后和为 1，否则强度与衰减
    // 耦合（两个滑杆互相偷值）。
    {
        let w5 = composite_weights(5, 0.8);
        let sum: f32 = w5.iter().sum();
        let mono = w5.windows(2).all(|p| p[0] >= p[1]);
        let w0 = composite_weights(5, 0.0);
        let w0_sum: f32 = w0.iter().sum();
        set.add(
            "K04-三段-合成权重归一化与单调衰减",
            close(sum, 1.0) && mono && close(w0_sum, 1.0) && close(w0[0], 1.0),
            "",
        );
    }

    // ---- F1849 源标记契约兑现 ----

    // 判据：源标记掩码提升提取的物理准确性——掩码区的**有效提取阈值**严格
    // 低于非掩码区（F1849 兑现的落点）。
    {
        let p = BloomParams {
            threshold: 2.0,
            mask_relax: 0.75,
            ..Default::default()
        };
        let thr_no_mask = effective_threshold(p.threshold, 0.0, p.mask_relax);
        let thr_full = effective_threshold(p.threshold, 1.0, p.mask_relax);
        let thr_half = effective_threshold(p.threshold, 0.5, p.mask_relax);
        set.add(
            "K04-兑现-掩码区阈值放宽且严格更低",
            thr_full < thr_no_mask
                && close(thr_full, 0.5)
                && thr_half < thr_no_mask
                && thr_half > thr_full,
            "",
        );
    }

    // 判据：契约兑现的实际效果——同一像素在掩码区被提取、在非掩码区不被
    // 提取（亮度介于两个有效阈值之间）。
    {
        let p = BloomParams {
            threshold: 1.0,
            soft_knee: 0.0,
            mask_relax: 1.0,
            ..Default::default()
        };
        let c = HdrColor::new(0.5, 0.5, 0.5); // 亮度 0.5
        let in_mask = extract(c, 1.0, &p); // 掩码区阈值 0 → 提取
        let out_mask = extract(c, 0.0, &p); // 非掩码区阈值 1 → 不提取
        set.add(
            "K04-兑现-掩码区提升提取物理准确性",
            in_mask.rate > 0.0 && close(out_mask.rate, 0.0),
            "",
        );
    }

    // 判据：契约描述校验——半分辨率且 present 才兑现；缺失/非法一律退化为
    // 纯亮度提取 + 显性告警（**不静默**）。
    {
        let p = BloomParams::default().resolve().params;
        let mut d = DiagBag::new();
        let ok = resolve_source_mask(
            Some(SourceMaskRt { width: 960, height: 540, present: true }),
            1920,
            1080,
            &p,
            &mut d,
        );
        let mut d2 = DiagBag::new();
        let missing = resolve_source_mask(None, 1920, 1080, &p, &mut d2);
        let mut d3 = DiagBag::new();
        let invalid = resolve_source_mask(
            Some(SourceMaskRt { width: 800, height: 600, present: true }),
            1920,
            1080,
            &p,
            &mut d3,
        );
        let mut d4 = DiagBag::new();
        let not_present = resolve_source_mask(
            Some(SourceMaskRt { width: 960, height: 540, present: false }),
            1920,
            1080,
            &p,
            &mut d4,
        );
        set.add(
            "K04-错误-契约缺失显性告警且退化纯亮度",
            ok.honored
                && ok.effective_threshold < p.threshold
                && d.is_empty()
                && !missing.honored
                && close(missing.effective_threshold, p.threshold)
                && d2.has(DiagCode::SourceMaskMissing)
                && !invalid.honored
                && d3.has(DiagCode::SourceMaskInvalid)
                && !not_present.honored
                && d4.has(DiagCode::SourceMaskMissing)
                && missing.diag == Some(DiagCode::SourceMaskMissing)
                && invalid.diag == Some(DiagCode::SourceMaskInvalid),
            "",
        );
    }

    // ---- 错误路径：量纲钳制 ----

    // 判据：提取阈值 NaN/负 → F1802 量纲钳制（NaN 走兜底值而非 0；负钳到 0）。
    {
        let nan_r = BloomParams { threshold: f32::NAN, ..Default::default() }.resolve();
        let neg_r = BloomParams { threshold: -5.0, ..Default::default() }.resolve();
        let inf_r = BloomParams { threshold: f32::INFINITY, ..Default::default() }.resolve();
        let big_r = BloomParams { threshold: 1.0e9, ..Default::default() }.resolve();
        set.add(
            "K04-错误-阈值量纲钳制（NaN兜底/负归零/超上界）",
            close(nan_r.params.threshold, THRESHOLD_FALLBACK_CD_M2)
                && nan_r.diags.has(DiagCode::ThresholdNonFinite)
                && close(neg_r.params.threshold, THRESHOLD_MIN_CD_M2)
                && neg_r.diags.has(DiagCode::ThresholdNegative)
                && close(inf_r.params.threshold, THRESHOLD_FALLBACK_CD_M2)
                && big_r.params.threshold <= THRESHOLD_MAX_CD_M2
                && big_r.diags.has(DiagCode::ThresholdClampedHigh),
            "",
        );
    }

    // 判据：参数越界钳制全覆盖（软膝/强度/衰减/放宽），且钳制留诊断
    // （静默钳制会让"设错了"和"没设"混为一谈）。
    {
        let r = BloomParams {
            threshold: 1.0,
            soft_knee: -3.0,
            mip_levels: 5,
            intensity: 99.0,
            composite_decay: 5.0,
            mask_relax: -1.0,
        }
        .resolve();
        let no_degrade = r.diags.no_degradation(); // 均为"已纠正"级，不阻断
        set.add(
            "K04-错误-参数越界钳制与纠正级诊断",
            r.params.soft_knee == SOFT_KNEE_MIN
                && r.params.intensity == INTENSITY_MAX
                && r.params.composite_decay == COMPOSITE_DECAY_MAX
                && r.params.mask_relax == 0.0
                && r.diags.has(DiagCode::SoftKneeOutOfRange)
                && r.diags.has(DiagCode::IntensityOutOfRange)
                && r.diags.has(DiagCode::CompositeDecayOutOfRange)
                && r.diags.has(DiagCode::MaskRelaxOutOfRange)
                && no_degrade,
            "",
        );
    }

    // ---- F2003 池对接与显存降级 ----

    // 判据：RT 全走 F2003 池——分配请求含尺寸/格式/存活区间，帧边界统一回收。
    {
        let mut d = DiagBag::new();
        let plan = plan_mip_chain(1920, 1080, 5, u64::MAX, &mut d);
        let dims = plan.dims();
        let roles_ok = plan.requests[0].role == "bloom_extract"
            && plan.requests[1..].iter().all(|r| r.role == "bloom_mip");
        let fmt_ok = plan
            .requests
            .iter()
            .all(|r| r.format == RtFormat::Rgba16f);
        // 末级 mip 活到帧末（live_to = u16::MAX），中间级活到下一级消费
        let interval_ok = plan.requests.last().unwrap().live_to == u16::MAX
            && plan.requests[0].live_to == 1;
        let reclaimed = reclaim_all(&plan);
        set.add(
            "K04-池-分配请求含存活区间且帧边界回收",
            plan.levels == 5
                && dims.len() == 5
                && dims[0] == (960, 540)
                && roles_ok
                && fmt_ok
                && interval_ok
                && reclaimed.len() == 5
                && d.is_empty(),
            "",
        );
    }

    // 判据：显存超配额 → 自动降级数 + 告警（显性降级，链不崩）。
    //
    // 降级语义三段（本条的核心契约）：
    //  a. 配额充裕 → 足额建链，无告警；
    //  b. 配额偏紧 → 级数下调至**确实入配额**，且必有告警（降级是显性事件）；
    //  c. 配额紧到连 mip0 都装不下 → 保底留 1 级并**继续告警**——
    //     本条只降级不禁用（"是否关闭 Bloom"归 F2015 档位表的职责），
    //     故此处允许 total_bytes 仍超配额，但必须留痕，不许静默超配。
    //
    // 配额样本的选取受一条链内事实约束：mip0（960×540×RGBA16F ≈ 4.15MB）
    // 独占 7 级链总量（≈5.53MB）的约 75%——各级面积按 1/4 递减，mip0 之外
    // 六级加起来才占 1/4。故"降级后仍远低于原链"与"降级后入给定配额"这两条
    // 不能用同一个激进的配额去验，否则会误判成实现缺陷。
    {
        // 三段各用独立诊断袋——共用一个袋子会让 a 段的"无告警"断言读到
        // b/c 段写入的降级告果，恒假。
        let mut d_rich = DiagBag::new();
        let mut d_tight = DiagBag::new();
        let mut d_starved = DiagBag::new();
        let full = plan_mip_chain(1920, 1080, 7, u64::MAX, &mut DiagBag::new());
        let rich = plan_mip_chain(1920, 1080, 7, full.total_bytes, &mut d_rich);
        // 78% 配额：足不进7 级、但足进 1 级 → 应恰好降到 1 级且入配额
        let tight_quota = full.total_bytes * 78 / 100;
        let half_budget = plan_mip_chain(1920, 1080, 7, tight_quota, &mut d_tight);
        // 配额枯竭（远小于 mip0）→ 保底 1 级 + 告警留痕，允许仍超配额
        let starved = plan_mip_chain(1920, 1080, 5, 1, &mut d_starved);
        set.add(
            "K04-错误-显存超配额自动降级并告警",
            // a 配额充裕：足额、无告警
            rich.levels == 7
                && rich.total_bytes == full.total_bytes
                && d_rich.is_empty()
                // b 配额偏紧：降级 + 告警 + 降后确实入配额
                && half_budget.levels < 7
                && half_budget.total_bytes <= tight_quota
                && half_budget.total_bytes < full.total_bytes
                && d_tight.has(DiagCode::MipChainOverQuota)
                // c 配额枯竭：保底 1 级 + 告警留痕（不静默超配、不禁用整链）
                && starved.levels == MIP_LEVELS_MIN
                && starved.total_bytes > 1
                && d_starved.has(DiagCode::MipChainOverQuota),
            "",
        );
    }

    // ---- 帧边界提交（F1762 规则） ----

    // 判据：mip 级数切换帧边界生效——帧内请求只写待决槽（留挂起诊断），
    // 帧边界才换链。
    {
        let resolved = BloomParams { mip_levels: 5, ..Default::default() }.resolve();
        let mut chain = new_chain(1920, 1080, &resolved, u64::MAX);
        let before = chain.active_levels;
        chain.request_levels(7);
        let pending_visible = chain.has_pending()
            && chain.active_levels == before
            && chain.diags.has(DiagCode::LevelSwitchPending);
        let swapped = chain.commit_frame_boundary(u64::MAX, None);
        set.add(
            "K04-错误-级数切换帧边界生效（F1762）",
            pending_visible && swapped && chain.active_levels == 7 && !chain.has_pending(),
            "",
        );
    }

    // 判据：帧边界提交同时处理分辨率变化（尺寸变 → 必须重分配）。
    {
        let resolved = BloomParams { mip_levels: 5, ..Default::default() }.resolve();
        let mut chain = new_chain(1920, 1080, &resolved, u64::MAX);
        let swapped = chain.commit_frame_boundary(u64::MAX, Some((1280, 720)));
        let dims = chain.plan().dims();
        set.add(
            "K04-错误-分辨率变化经帧边界重分配",
            swapped && chain.main_size == (1280, 720) && dims[0] == (640, 360),
            "",
        );
    }

    // ---- 光敏联动（F1962） ----

    // 判据：泛光作为光照动效源扩展入 F1962 扫描——面积扩张率与时间变化率
    // 均被交出；超阈即标记需扫描。
    {
        let p = BloomParams { intensity: 2.0, ..Default::default() };
        let dims = mip_chain_dims(1920, 1080, 5);
        let s = photosensitive_sample(&p, &dims, 1.0, 2.0);
        let ratio = area_spread_ratio(&dims);
        // 5 级链：末级 60×33 / mip0 960×540 —— 面积比约为 1/262
        // （1080 侧减半到 67 再减半到 33，向上取整使各级并非精确 1/4，
        //  故此处按实测尺寸核对而非按 1/256 的理想值断言）
        let expect = (60.0f32 * 33.0) / (960.0 * 540.0);
        set.add(
            "K04-光敏-三要素交出与超阈标记",
            close(ratio, expect)
                && dims.len() == 5
                && dims[4] == (60, 33)
                && s.intensity > 0.0
                && s.temporal_delta > 0.9
                && s.needs_scan(1.5, 0.01, 0.35)
                && s.screen_text().contains("光敏扫描"),
            "",
        );
    }

    // 判据：单级链无面积扩张（面积扩张率为 0——链长才是扩张的来源）。
    {
        let dims = mip_chain_dims(1920, 1080, 1);
        let s = photosensitive_sample(&BloomParams::default(), &dims, 1.0, 1.0);
        set.add(
            "K04-光敏-单级链无面积扩张",
            close(s.area_spread, 0.0) && close(s.temporal_delta, 0.0),
            "",
        );
    }

    // ---- 管线序断言（F2001 序声明执行） ----

    // 判据：Bloom 在 TM 前、LUT 前；位错即拒绝（静默出错误图更难排查）。
    {
        let ok = assert_slot(PipelineSlot::BeforeToneMap);
        let after_tm = assert_slot(PipelineSlot::AfterToneMap);
        let after_enc = assert_slot(PipelineSlot::AfterEncode);
        set.add(
            "K04-序位-TM前合法与越位拒绝",
            ok.is_ok()
                && after_tm.is_err()
                && after_enc.is_err()
                && PIPELINE_ORDER_DOC.contains("Bloom")
                && PIPELINE_ORDER_DOC.contains("TM 之前")
                && PIPELINE_ORDER_DOC.contains("LUT 之前"),
            "",
        );
    }

    // ---- 诊断三要素与显性纪律 ----

    // 判据：每条诊断给足三要素（码/严重级/下一步）；降级级不阻断帧
    // （效果层崩了会带走全链，降级出图 + 显性告警是唯一正确姿态）。
    {
        let d = Diagnostic::new(DiagCode::SourceMaskMissing, 2.0);
        let text = d.describe();
        let all_hints = DiagCode::SourceMaskMissing.next_hint().contains("F1849")
            && DiagCode::MipChainOverQuota.next_hint().contains("F1776")
            && DiagCode::ThresholdNonFinite.next_hint().contains("F1802");
        set.add(
            "K04-诊断-三要素齐备且降级不阻断帧",
            text.contains("BLOOM_SOURCE_MASK_MISSING")
                && text.contains("已降级")
                && text.contains('→')
                && all_hints
                && !Severity::Degraded.blocks_frame()
                && !Severity::Corrected.blocks_frame()
                && F1849_REDEMPTION_DOC.contains("VE-F2004"),
            "",
        );
    }

    // 判据：三级语义串读（提取 → 链 → 合成）端到端跑通，输出恒为有限值。
    {
        let p = BloomParams { threshold: 1.0, soft_knee: 0.5, intensity: 1.0, ..Default::default() };
        let chain = new_chain(1920, 1080, &p.resolve(), u64::MAX);
        let scene = HdrColor::new(0.8, 0.8, 0.8);
        // 模拟：提取 →逐级模糊（平场）→ 合成
        let ex = extract(scene, 0.5, &p);
        let flat = |_x: i32, _y: i32| ex.color;
        let dims = mip_chain_dims(1920, 1080, chain.active_levels);
        let mut mips: Vec<HdrColor> = Vec::new();
        for dim in dims.iter() {
            mips.push(blur_tap(&flat, dim.0 as i32, dim.1 as i32, false));
        }
        let out = composite(scene, &mips, &p);
        let txt = screen_text(&chain, &p);
        set.add(
            "K04-三段-端到端全链有限且可读",
            out.r.is_finite()
                && out.g.is_finite()
                && out.b.is_finite()
                && out.r >= scene.r - 1e-4
                && txt.contains("泛光")
                && chain.weights(0.8).len() == chain.active_levels as usize,
            "",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vek04_domain_all_green() {
        let set = run_vek04_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed() && !set.truncated(),
            "VE-F2004 域自检红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }

    /// 分离核权重和是本条最隐蔽的缺陷源（逐级漂移），单独钉死。
    #[test]
    fn kernel_weight_sums_exactly_one() {
        assert!((kernel_weight_sum(&DOWN_KERNEL) - 1.0).abs() < 1e-6);
        assert!((kernel_weight_sum(&UP_KERNEL) - 1.0).abs() < 1e-6);
    }

    /// 软钳制单调且有界（防炸帧的最后一道）。
    ///
    /// 采样点用**加法递进**而非乘法：乘法从 0 起会恒为 0（死循环），
    /// 且对数刻度对单调性验证没有额外价值。
    #[test]
    fn soft_clamp_monotonic_and_bounded() {
        let mut prev = soft_clamp(SOFT_CLAMP_KNEE - 10.0);
        let mut x = SOFT_CLAMP_KNEE - 10.0;
        while x < 1.0e6 {
            x += 1.0e4;
            let v = soft_clamp(x);
            assert!(v >= prev - 1e-3, "软钳制非单调：x={} v={} prev={}", x, v, prev);
            assert!(
                v <= SOFT_CLAMP_MAX * 1.001,
                "软钳制越界：x={} v={}",
                x,
                v
            );
            prev = v;
        }
    }

    /// 合成权重恒为 1（强度/衰减解耦的根基）。
    #[test]
    fn composite_weights_always_sum_one() {
        for lv in 1..=MIP_LEVELS_MAX {
            for d in [0.0f32, 0.3, 0.5, 0.8, 1.0] {
                let w = composite_weights(lv, d);
                assert_eq!(w.len(), lv as usize);
                let s: f32 = w.iter().sum();
                assert!((s - 1.0).abs() < 1e-4, "lv={} d={} 和={}", lv, d, s);
            }
        }
    }
}
