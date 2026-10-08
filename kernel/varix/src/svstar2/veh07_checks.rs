//! VE-F1407 · 域自检（判据逐条对应，见 `veh07_fade.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 曲线库 → `H07-曲线-*`
//! - 复合包络 → `H07-包络-*`
//! - 编排 → `H07-编排-*`
//! - 场景统一 → `H07-场景-*`
//! - 防重叠 → `H07-防重叠-*`

use super::veh07_fade::*;
use crate::checks::CheckSet;

use alloc::{format, vec};

/// VE-F1407 域自检。
pub fn run_veh07_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veh07");

    // ---- 判据：曲线库（端点精确、值域 [0,1]、单调可对拍）----

    {
        let curves = [
            FadeCurve::Linear,
            FadeCurve::EqualPower,
            FadeCurve::Exponential,
            FadeCurve::SCurve,
            FadeCurve::Bezier { x1: 0.25, y1: 0.1, x2: 0.75, y2: 0.9 },
        ];
        let mut ok = true;
        for c in curves.iter() {
            // 端点精确
            if c.at(0.0) != 0.0 || c.at(1.0) != 1.0 {
                ok = false;
            }
            // 值域 [0,1] 且采样点单调不减（fade 的定义域纪律）
            let mut prev = -1.0f64;
            for i in 0..=100u32 {
                let v = c.at(i as f64 / 100.0);
                if !(0.0..=1.0).contains(&v) || v < prev {
                    ok = false;
                }
                prev = v;
            }
        }
        // 曲线间形态互异（等功率中点 > 线性中点，指数中点 < 线性中点）
        let ep_mid = FadeCurve::EqualPower.at(0.5);
        let lin_mid = FadeCurve::Linear.at(0.5);
        let exp_mid = FadeCurve::Exponential.at(0.5);
        let shape_ok = ep_mid > lin_mid && exp_mid < lin_mid && lin_mid == 0.5;
        set.add("H07-曲线-五曲线端点值域单调", ok && shape_ok, "");
    }

    // ---- 判据：复合包络（淡入-保持-淡出，段边界与曲线可配）----

    {
        let env = CompositeEnvelope::notify_fade(100, 50, 200);
        // 总时长 = 350
        let total_ok = env.total_ticks() == 350;
        // 淡入中点：约 0.707（等功率）
        let in_mid = env.eval(50);
        // 保持段：1.0（含段边界 100 与 150）
        let hold_ok = env.eval(100) == 1.0 && env.eval(150) == 1.0;
        // 淡出终点：0
        let end = env.eval(350);
        // 段边界可配：自定义包络（线性淡入到 0.6 → 淡出到 0）
        let custom = CompositeEnvelope {
            start_gain: 0.0,
            segments: vec![
                FadeSegment { duration_ticks: 10, target_gain: 0.6, curve: FadeCurve::Linear },
                FadeSegment { duration_ticks: 10, target_gain: 0.0, curve: FadeCurve::Linear },
            ],
        };
        let custom_mid = custom.eval(5);
        set.add(
            "H07-包络-三段复合段边界可配",
            total_ok
                && hold_ok
                && (end - 0.0).abs() < 1e-9
                && (in_mid - 0.7071).abs() < 0.01
                && (custom_mid - 0.3).abs() < 1e-9,
            "",
        );
    }

    // ---- 判据：编排（交叉淡化生命周期）----

    {
        let mut orch = CrossfadeOrchestrator::new(10, 11, 50, FadeCurve::EqualPower);
        orch.start(0);
        let started = orch.service.events.iter().any(|e| e.contains("编排开始") && e.contains("同步触发"));
        // 进行到一半：A 淡到约 0.293（等功率中点 0.707 的补），B 升到约 0.707
        for t in 0..25u64 {
            orch.tick(t);
        }
        let a_mid = orch.service.current_gain(10);
        let b_mid = orch.service.current_gain(11);
        let cross_ok = (a_mid - 0.2929).abs() < 0.02 && (b_mid - 0.7071).abs() < 0.02;
        // 完成：A=0、B=1、完成回调有账
        for t in 25..60u64 {
            orch.tick(t);
        }
        let done = orch.done()
            && orch.service.current_gain(10) == 0.0
            && orch.service.current_gain(11) == 1.0
            && orch.service.completed_total == 2;
        set.add(
            "H07-编排-交叉生命周期",
            started && cross_ok && done,
            "",
        );
    }

    // ---- 判据：场景统一（四场景全走服务 + lint 拦服务外直写）----

    {
        let mut svc = FadeService::new(OverlapPolicy::Replace);
        let scenes = [
            Scene::PlaybackStartStop,
            Scene::SfxTrigger,
            Scene::SceneSwitch,
            Scene::Notification,
        ];
        for (i, sc) in scenes.iter().enumerate() {
            svc.request(
                FadeRequest {
                    target: 100 + i as u32,
                    duration_ticks: 10,
                    curve: FadeCurve::Linear,
                    target_gain: 0.5,
                    scene: *sc,
                },
                0,
            );
        }
        let all_via = svc.events.iter().filter(|e| e.contains("fade 开始")).count() == 4;
        // lint：服务外直写被记违规
        let mut lint = VolumeLint::new();
        let calls = vec![
            (1u32, true, Scene::PlaybackStartStop),
            (2u32, false, Scene::SfxTrigger),
        ];
        let n = lint.lint(&calls);
        set.add(
            "H07-场景-四场景统一与lint",
            all_via && n == 1 && lint.violations[0].0 == 2,
            "",
        );
    }

    // ---- 判据：防重叠（Queue 串行 / Replace 覆盖，策略可配）----

    {
        // Queue：同目标第二请求排队，当前完成后串行接续
        let mut svc = FadeService::new(OverlapPolicy::Queue);
        let mk = |g: f64| FadeRequest {
            target: 1,
            duration_ticks: 10,
            curve: FadeCurve::Linear,
            target_gain: g,
            scene: Scene::PlaybackStartStop,
        };
        svc.request(mk(0.0), 0);
        svc.request(mk(1.0), 0);
        let queued = svc.queue_len(1) == 1 && svc.queued_total == 1;
        for t in 0..10u64 {
            svc.tick(t);
        }
        let mid_gain = svc.current_gain(1);
        for t in 10..20u64 {
            svc.tick(t);
        }
        let final_gain = svc.current_gain(1);
        let serial_ok = queued
            && (mid_gain - 0.0).abs() < 1e-9
            && (final_gain - 1.0).abs() < 1e-9
            && svc.completed_total == 2;
        set.add("H07-防重叠-Queue串行接续", serial_ok, "");
    }
    {
        // Replace：新请求立即覆盖，无音量抖动（同 tick 只有一笔增益变更）
        let mut svc = FadeService::new(OverlapPolicy::Replace);
        let mk = |g: f64| FadeRequest {
            target: 2,
            duration_ticks: 100,
            curve: FadeCurve::Linear,
            target_gain: g,
            scene: Scene::Notification,
        };
        svc.request(mk(0.0), 0);
        svc.request(mk(1.0), 0);
        let replaced = svc.replaced_total == 1;
        // 防抖检查：每 tick 同目标至多一笔增益变更（重叠 = 抖动 = 缺陷）
        let mut jitter_free = true;
        for t in 0..5u64 {
            let before = svc.gain_log.iter().filter(|(tgt, tick, _)| *tgt == 2 && *tick == t).count();
            svc.tick(t);
            let after = svc.gain_log.iter().filter(|(tgt, tick, _)| *tgt == 2 && *tick == t).count();
            if after - before > 1 {
                jitter_free = false;
            }
        }
        set.add(
            "H07-防重叠-Replace覆盖无抖动",
            replaced && jitter_free && svc.is_active(2),
            "",
        );
    }

    // ---- 读屏与确定性 ----

    {
        let mut svc = FadeService::new(OverlapPolicy::Queue);
        svc.request(
            FadeRequest {
                target: 9,
                duration_ticks: 10,
                curve: FadeCurve::EqualPower,
                target_gain: 1.0,
                scene: Scene::Notification,
            },
            0,
        );
        let s = svc.events.join("；");
        set.add(
            "H07-读屏-事件账可播",
            s.contains("fade 开始") && s.contains("通知进出") && s.contains("等功率"),
            "",
        );
    }
    {
        let run = || {
            let mut orch = CrossfadeOrchestrator::new(1, 2, 20, FadeCurve::SCurve);
            orch.start(0);
            for t in 0..20u64 {
                orch.tick(t);
            }
            (
                orch.service.current_gain(1),
                orch.service.current_gain(2),
                orch.service.events.join("；"),
            )
        };
        let a = run();
        let b = run();
        set.add("H07-确定-同操作同账面", a == b, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{format, vec};

    /// 域自检必须全绿——红项即施工未完成。
    #[test]
    fn veh07_checks_all_green() {
        let set = run_veh07_checks();
        let (passed, failed) = set.tally();
        if !set.all_passed() {
            let (items, n) = set.red_items();
            let mut msg = format!("VE-H07 域自检红项：{}/{} 绿", passed, passed + failed);
            for it in items.iter().take(n) {
                if let Some(c) = it {
                    if !c.passed {
                        msg.push_str(&format!("\n  [红] {} — {}", c.name, c.detail));
                    }
                }
            }
            panic!("{}", msg);
        }
    }

    /// 防重叠铁律：同目标并发 fade 在两种策略下都不产生音量抖动。
    #[test]
    fn overlap_never_jitters() {
        for policy in [OverlapPolicy::Queue, OverlapPolicy::Replace] {
            let mut svc = FadeService::new(policy);
            let mk = |g: f64| FadeRequest {
                target: 7,
                duration_ticks: 20,
                curve: FadeCurve::Linear,
                target_gain: g,
                scene: Scene::SceneSwitch,
            };
            svc.request(mk(0.0), 0);
            svc.request(mk(0.4), 0);
            svc.request(mk(1.0), 0);
            for t in 0..60u64 {
                let before = svc.gain_log.iter().filter(|(tg, tk, _)| *tg == 7 && *tk == t).count();
                svc.tick(t);
                let after = svc.gain_log.iter().filter(|(tg, tk, _)| *tg == 7 && *tk == t).count();
                assert!(after - before <= 1, "策略 {:?} 在 tick {} 出现多笔增益变更", policy, t);
            }
            assert_eq!(svc.current_gain(7), 1.0);
        }
    }

    /// 复合包络端到端：通知音淡入-保持-淡出全程无跳变。
    #[test]
    fn envelope_smooth() {
        let env = CompositeEnvelope::notify_fade(100, 50, 200);
        let mut prev = env.eval(0);
        for t in 1..=350u64 {
            let g = env.eval(t);
            assert!((g - prev).abs() <= 0.05, "tick {} 处包络跳变 {}", t, (g - prev).abs());
            prev = g;
        }
    }
}
