//! VE-F0013 · 域自检（判据逐条对应，见 `vea13_softfall.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 完整软渲栈（GPU→完整软渲→最小合成，逐级降级） → `A13-栈-四级降级`
//! - 软渲染崩溃→最小合成模式 → `A13-栈-崩溃熔断最小模式`
//! - 最小合成模式含恢复路径（GPU 回来了能爬回去） → `A13-恢复-逐级爬回`
//! - 爬回不许带病（栈复核） → `A13-恢复-带病爬回拦截`
//! - 诚实能力（能做什么不能做什么） → `A13-能力-三态清单`
//! - 着色器软件后端覆盖度声明 → `A13-能力-着色器覆盖度显式`
//! - 能力虚标→修正 → `A13-能力-虚标修正`
//! - 保底承诺（最小模式保底 fps） → `A13-承诺-保底帧率`
//! - 回退含用户预期管理（一次说清不渐渐变卡） → `A13-预期-一次说清`
//! - 回归门禁含跨 CPU 架构矩阵 → `A13-门禁-跨架构矩阵`
//! - 每版全量跑通最小场景集 → `A13-门禁-最小场景集全量`
//! - A09 恢复状态机衔接 → `A13-衔接-A09契约`
//! - 能力清单读屏可达 → `A13-读屏-能力清单`
//!
//! 逻辑时钟注入、零墙钟，回归可复现。

use super::vea13_softfall::*;
use crate::checks::CheckSet;

/// VE-F0013 域自检。
pub fn run_vea13_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vea13");

    // ---- 完整软渲栈：四级降级 ----

    {
        let mut f = SoftFallback::new();
        f.on_gpu_lost("显卡全灭");
        let l1_ok = f.level == SoftLevel::FullSoft && f.expectation_served;
        f.on_soft_crash("合成");
        f.on_soft_crash("光栅");
        let l2_ok = f.level == SoftLevel::MinCompose;
        set.add("A13-栈-四级降级", l1_ok && l2_ok, "");
    }

    // 判据：软渲染崩溃→最小合成模式（阈值语义：连续 2 次）。
    {
        let mut f = SoftFallback::new();
        f.on_gpu_lost("驱动崩溃");
        f.on_soft_crash("单次崩溃");
        let one = f.level == SoftLevel::FullSoft;
        f.on_soft_crash("第二次崩溃");
        let two = f.level == SoftLevel::MinCompose
            && f.crashes.iter().all(|(_, c, _)| *c == "E_SOFT_CRASH")
            && f.events.iter().any(|(_, c, _)| *c == "E_MIN_MODE");
        set.add("A13-栈-崩溃熔断最小模式", one && two, "");
    }

    // ---- 最小模式恢复路径 ----

    // 判据：GPU 回来了能爬回去（L2→L1→L0 逐级，不许跳级）。
    {
        let mut f = SoftFallback::new();
        f.on_gpu_lost("拔卡");
        f.on_soft_crash("a");
        f.on_soft_crash("b");
        assert_eq!(f.level, SoftLevel::MinCompose);
        let s1 = f.on_gpu_recovered();
        let mid = f.level == SoftLevel::FullSoft && s1 == SoftLevel::FullSoft;
        let s2 = f.on_gpu_recovered();
        let done = s2 == SoftLevel::Gpu
            && f.events.iter().any(|(_, c, _)| *c == "E_CLIMB_L1")
            && f.events.iter().any(|(_, c, _)| *c == "E_CLIMB_L0");
        set.add("A13-恢复-逐级爬回", mid && done, "");
    }

    // 判据：爬回不许带病——软渲栈带崩溃时 L1→L0 被拦截。
    {
        let mut f = SoftFallback::new();
        f.on_gpu_lost("x");
        f.on_soft_crash("a");
        f.on_soft_crash("b");
        f.on_gpu_recovered(); // L2→L1（此时崩溃已清零，构造带病态需再崩一次但不必再降级）
        f.on_soft_crash("c");
        f.on_soft_crash("d"); // 再次达阈值 → 重新进最小模式
        let refell = f.level == SoftLevel::MinCompose;
        f.on_gpu_recovered(); // L2→L1
        f.on_soft_crash("e"); // 带一次未清崩溃
        let blocked = matches!(
            f.on_gpu_recovered(),
            SoftLevel::FullSoft
        ) && f.events.iter().any(|(_, c, _)| *c == "E_CLIMB_BLOCKED");
        f.confirm_soft_stack_healthy();
        let healed = f.on_gpu_recovered() == SoftLevel::Gpu;
        set.add("A13-恢复-带病爬回拦截", refell && blocked && healed, "");
    }

    // ---- 诚实能力 ----

    // 判据：能力三态清单在册（支持/降级/不支持逐项诚实标注）。
    {
        let caps = shader_backend_capabilities();
        let has_all = caps.iter().any(|c| c.level == CapLevel::Supported)
            && caps.iter().any(|c| c.level == CapLevel::Degraded)
            && caps.iter().any(|c| c.level == CapLevel::Unsupported);
        set.add("A13-能力-三态清单", has_all && caps.len() >= 6, "");
    }

    // 判据：着色器软件后端覆盖度显式（跑不了的写明：FP64/RT/视频硬解）。
    {
        let caps = shader_backend_capabilities();
        let fp64 = caps.iter().find(|c| c.name == "双精度运算");
        let rt = caps.iter().find(|c| c.name == "光线追踪");
        let sm = caps.iter().find(|c| c.name == "着色器模型");
        let ok = matches!(fp64.map(|c| c.level), Some(CapLevel::Unsupported))
            && matches!(rt.map(|c| c.level), Some(CapLevel::Unsupported))
            && matches!(sm.map(|c| c.level), Some(CapLevel::Degraded))
            && fp64.map(|c| !c.detail.is_empty()).unwrap_or(false);
        set.add("A13-能力-着色器覆盖度显式", ok, "");
    }

    // 判据：能力虚标→修正（声明高于实测即 E_CAPABILITY_INFLATED 强制修正）。
    {
        let mut f = SoftFallback::new();
        f.correct_capability("光线追踪", CapLevel::Unsupported, CapLevel::Supported);
        let inflated = f.corrections.len() == 1
            && f.events.iter().any(|(_, c, _)| *c == "E_CAPABILITY_INFLATED");
        f.correct_capability("多显示器合成", CapLevel::Supported, CapLevel::Supported);
        let honest_noop = f.corrections.len() == 1;
        set.add("A13-能力-虚标修正", inflated && honest_noop, "");
    }

    // ---- 保底承诺与预期管理 ----

    // 判据：保底承诺数字化（最小模式 15fps 承诺在账）。
    {
        let promised = MIN_MODE_FPS_PROMISE == 15 && SOFT_EXPECT_FPS == 30;
        let mut f = SoftFallback::new();
        f.on_gpu_lost("x");
        f.on_soft_crash("a");
        f.on_soft_crash("b");
        let t = f.expectation_text();
        set.add(
            "A13-承诺-保底帧率",
            promised && t.contains("15") && t.contains("最小合成模式"),
            "",
        );
    }

    // 判据：回退含用户预期管理——进入即一次性告知（一次说清不渐渐变卡）。
    {
        let mut f = SoftFallback::new();
        let before = !f.expectation_served;
        f.on_gpu_lost("拔出");
        let served_once = f.expectation_served;
        f.on_soft_crash("a");
        f.on_soft_crash("b");
        let still_once = f.expectation_served;
        let text_ok = f.expectation_text().contains("一次说清")
            && f.expectation_text().contains("保底不是享受");
        set.add("A13-预期-一次说清", before && served_once && still_once && text_ok, "");
    }

    // 判据：性能劣化→预期告知（实测 < 承诺80%触发，同档一次，账本入账）。
    {
        let mut f = SoftFallback::new();
        f.on_gpu_lost("x");
        let fired = f.report_fps(20).is_some();
        let once = f.report_fps(15).is_none() && f.degrade_notices.len() == 1;
        let healthy = f.report_fps(30).is_none();
        let in_ledger = f.events.iter().any(|(_, c, _)| *c == "E_PERF_DEGRADED");
        set.add(
            "A13-预期-劣化告知",
            fired && once && healthy && in_ledger && DEGRADE_FPS_MARGIN_PCT == 80,
            "",
        );
    }

    // ---- 回归门禁 ----

    // 判据：回归门禁含跨 CPU 架构矩阵（x64/ARM64 全量）。
    {
        let g = RegressionGate::run(|_a, _s| true);
        let arches = g.results.len() == GATE_ARCHES.len()
            && g.results.iter().map(|r| r.arch).collect::<Vec<_>>() == GATE_ARCHES.to_vec()
            && g.all_green();
        set.add("A13-门禁-跨架构矩阵", arches, "");
    }

    // 判据：每版全量跑通最小场景集（6 场景 × 每架构；红项逐条点名不静默）。
    {
        let g = RegressionGate::run(|a, s| !(a == "x64" && s == "图像"));
        let red = !g.all_green() && g.report().contains("图像=红");
        let g2 = RegressionGate::run(|_a, _s| true);
        let full = g2.results.iter().all(|r| r.scenes.len() == MIN_SCENE_SET.len())
            && g2.all_green();
        set.add("A13-门禁-最小场景集全量", red && full, "");
    }

    // ---- A09 衔接 ----

    // 判据：跨批对接点显式（A09 恢复状态机衔接以契约版本常量在册，
    // 崩溃熔断阈值与 A09「连续丢失转最小模式」语义同构）。
    {
        set.add(
            "A13-衔接-A09契约",
            A09_LINK >= 1 && CRASH_FUSE_LIMIT == 2,
            "",
        );
    }

    // ---- 读屏可达 ----

    {
        let f = SoftFallback::new();
        let cap = f.capability_screen_text();
        let exp = f.expectation_text();
        set.add(
            "A13-读屏-能力清单",
            cap.contains("软渲染能力清单") && exp.contains("GPU 正常"),
            "",
        );
    }

    set
}
