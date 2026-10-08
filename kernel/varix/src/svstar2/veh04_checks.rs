//! VE-F1404 · 域自检（判据逐条对应，见 `veh04_sendsidechain.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - Send 共享 → `H04-Send-*`
//! - Insert → `H04-Insert-*`（含 Send/Insert 选型指南）
//! - 侧链 → `H04-侧链-*`
//! - 尾音保护 → `H04-尾音-*`
//! - 自动化 → `H04-自动化-*`

use super::veh04_sendsidechain::*;
use crate::checks::CheckSet;

use alloc::{format, vec};

/// VE-F1404 域自检。
pub fn run_veh04_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veh04");

    // ---- 判据：Send 共享（N 路一个实例，逐路电平，收益入账）----

    {
        let mut bus = SendBus::new();
        bus.register_effect(1, "混响", 2.0);
        let mut errs = 0;
        for src in 100..=108u32 {
            let r = bus.add_send(src, 1, 300 + (src - 100) as u32 * 50);
            if r.is_err() {
                errs += 1;
            }
        }
        // 未登记实例拒绝；同源同效果重复拒绝；电平钳制
        let unregistered = bus.add_send(200, 99, 500).is_err();
        let _ = bus.add_send(100, 1, 300);
        let dup = bus.add_send(100, 1, 400).is_err();
        let clamped = {
            let _ = bus.add_send(300, 1, 2000);
            bus.paths
                .iter()
                .find(|p| p.source_node == 300)
                .map(|p| p.level_permille == 1000)
                .unwrap_or(false)
        };
        let n = bus.consumers_of(1);
        let savings = bus.sharing_savings_pct(1);
        set.add(
            "H04-Send-N路一实例逐路电平",
            errs == 0
                && n == 10
                && savings == 90
                && unregistered
                && dup
                && clamped
                && (bus.send_gain(100, 1) - 0.3).abs() < 1e-9,
            "",
        );
    }

    // ---- 判据：Insert（链序即处理序）+ 选型指南 ----

    {
        let mut chain = InsertChain::new(7);
        let _ = chain.append(10);
        let _ = chain.append(12);
        let mid = chain.insert_at(1, 11);
        let dup = chain.append(10).is_err();
        let order_ok = chain.processing_order() == [10, 11, 12];
        // 选型指南：并行共享 vs 串行独占两句话可分
        let guide_send = SelectionGuide::advise(true, false).contains("Send");
        let guide_insert = SelectionGuide::advise(false, true).contains("Insert");
        set.add(
            "H04-Insert-链序与选型",
            mid.is_ok() && dup && order_ok && guide_send && guide_insert,
            "",
        );
    }

    // ---- 判据：侧链（ducking 快起慢放、压制封顶、合法性校验）----

    {
        // ducking：触发包络升 → 压制快（attack）；回落 → 恢复慢（release）
        let mut trigger = vec![0.0; 20];
        for t in trigger.iter_mut().take(10) {
            *t = 1.0;
        }
        let curve = ducking_curve(&trigger, 12.0, 2, 20);
        let attacked = curve[4] < -6.0; // attack=2 步：4 tick 内压过 -6dB
        let peak = curve.iter().cloned().fold(0.0f64, f64::max);
        let capped = peak.abs() <= 12.0 + 1e-9; // 压制封顶
        // release：触发归零后 10 tick 仍未完全恢复（慢放）
        let after = &curve[10..];
        let slow_release = after[9] < -0.5; // 10 tick 后仍显著压制中
        set.add(
            "H04-侧链-ducking快起慢放封顶",
            attacked && capped && slow_release,
            "",
        );
    }
    {
        // 侧链路由合法性：自激环 / 未登记参数 / 超封顶 / release<attack 全拒
        let r1 = validate_sidechain(&SidechainRoute {
            trigger_node: 5,
            target_node: 5,
            param: "gain",
            max_reduction_db: 10,
            attack_steps: 2,
            release_steps: 20,
        })
        .is_err();
        let r2 = validate_sidechain(&SidechainRoute {
            trigger_node: 5,
            target_node: 6,
            param: "unknown_param",
            max_reduction_db: 10,
            attack_steps: 2,
            release_steps: 20,
        })
        .is_err();
        let r3 = validate_sidechain(&SidechainRoute {
            trigger_node: 5,
            target_node: 6,
            param: "gain",
            max_reduction_db: 100,
            attack_steps: 2,
            release_steps: 20,
        })
        .is_err();
        let r4 = validate_sidechain(&SidechainRoute {
            trigger_node: 5,
            target_node: 6,
            param: "gain",
            max_reduction_db: 10,
            attack_steps: 20,
            release_steps: 2,
        })
        .is_err();
        let ok = validate_sidechain(&SidechainRoute {
            trigger_node: 5,
            target_node: 6,
            param: "eq_highshelf",
            max_reduction_db: 8,
            attack_steps: 2,
            release_steps: 20,
        })
        .is_ok();
        set.add("H04-侧链-路由合法性四拒一过", r1 && r2 && r3 && r4 && ok, "");
    }

    // ---- 判据：尾音保护（RT60 衰减到 -60dB，硬切被拦）----

    {
        let mut pool = TailPool::new();
        // 无尾音效果（干声类）直接释放
        pool.retire(1, 0.0, 48_000);
        // 混响 RT60=2s @48k = 96000 ticks
        pool.retire(2, 2.0, 48_000);
        let total = 2.0 * 48_000.0;
        let t2 = pool.tails.iter().find(|t| t.effect_id == 2).unwrap();
        let half_db_ok = (t2.level_db() - 0.0).abs() < 0.001; // 未衰减时 ~0dB
        // 重复 retire = 试图硬切，拦截
        pool.retire(2, 2.0, 48_000);
        let blocked = pool.hard_cut_blocked == 1;
        // 推进一半：-30dB；推进到结束：释放
        pool.advance(total as u64 / 2);
        let mid_db = pool
            .tails
            .iter()
            .find(|t| t.effect_id == 2)
            .map(|t| t.level_db())
            .unwrap_or(0.0);
        let released_total_before = pool.released_total;
        let drained = pool.advance(total as u64);
        set.add(
            "H04-尾音-自然衰减不硬切",
            pool.tails.is_empty()
                && blocked
                && half_db_ok
                && (mid_db + 30.0).abs() < 0.5
                && drained == 1
                && pool.released_total == released_total_before + 1,
            "",
        );
    }

    // ---- 判据：自动化（分段线性 + 末点保持，F1324 同核）----

    {
        let mut rack = AutomationRack::new();
        rack.attach(Envelope::new(
            9,
            "gain",
            vec![
                EnvelopePoint { tick: 0, value: 0.0 },
                EnvelopePoint { tick: 100, value: 1.0 },
                EnvelopePoint { tick: 200, value: 0.5 },
            ],
        ));
        // 分段线性：中点精确
        let mid = rack.eval(9, "gain", 50).unwrap();
        let mid2 = rack.eval(9, "gain", 150).unwrap();
        // 末点保持
        let hold = rack.eval(9, "gain", 10_000).unwrap();
        // 首点前保持
        let pre = rack.eval(9, "gain", 0).unwrap();
        // 摘除后无包络
        let removed = rack.detach(9, "gain");
        let none_after = rack.eval(9, "gain", 50).is_none();
        set.add(
            "H04-自动化-线性插值与保持",
            (mid - 0.5).abs() < 1e-9
                && (mid2 - 0.75).abs() < 1e-9
                && (hold - 0.5).abs() < 1e-9
                && pre == 0.0
                && removed
                && none_after,
            "",
        );
    }

    // ---- 读屏与确定性 ----

    {
        let s = RoutingSummary::render(2, 10, 3, 1, 1, 4);
        set.add(
            "H04-读屏-路由摘要可播",
            s.contains("共享效果实例") && s.contains("侧链") && s.contains("尾音") && s.contains("自动化"),
            "",
        );
    }
    {
        let run = || {
            let mut bus = SendBus::new();
            bus.register_effect(1, "延迟", 0.8);
            let _ = bus.add_send(1, 1, 500);
            let _ = bus.add_send(2, 1, 250);
            let mut pool = TailPool::new();
            pool.retire(1, 0.8, 48_000);
            pool.advance(10_000);
            let mut rack = AutomationRack::new();
            rack.attach(Envelope::new(
                9,
                "gain",
                vec![EnvelopePoint { tick: 0, value: 0.0 }, EnvelopePoint { tick: 10, value: 1.0 }],
            ));
            (
                bus.sharing_savings_pct(1),
                pool.tails.len(),
                pool.released_total,
                rack.eval(9, "gain", 7),
                ducking_curve(&[0.0, 1.0, 0.0], 10.0, 2, 20),
                RoutingSummary::render(1, 2, 0, 0, 0, 1),
            )
        };
        let a = run();
        let b = run();
        set.add("H04-确定-同操作同账面", a == b, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    /// 域自检必须全绿——红项即施工未完成。
    #[test]
    fn veh04_checks_all_green() {
        let set = run_veh04_checks();
        let (passed, failed) = set.tally();
        if !set.all_passed() {
            let (items, n) = set.red_items();
            let mut msg = format!("VE-H04 域自检红项：{}/{} 绿", passed, passed + failed);
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

    /// RT60 数学：尾音总 tick = rt60 × 采样率，衰减到 -60dB 才释放。
    #[test]
    fn tail_math_rt60() {
        let t = TailNode {
            effect_id: 1,
            rt60_s: 1.5,
            sample_rate: 48_000,
            elapsed_ticks: 0,
        };
        assert_eq!(t.tail_total_ticks(), 72_000);
        let mut pool = TailPool::new();
        pool.retire(1, 1.5, 48_000);
        pool.advance(72_000);
        assert!(pool.tails.is_empty(), "衰减满程必须释放");
        assert_eq!(pool.released_total, 1);
    }

    /// ducking 的快起慢放：attack 内压制快于 release 内恢复。
    #[test]
    fn ducking_fast_attack_slow_release() {
        let mut trigger = vec![0.0; 60];
        for t in trigger.iter_mut().take(20) {
            *t = 1.0;
        }
        let curve = ducking_curve(&trigger, 20.0, 2, 40);
        let attack_depth = -curve[5];
        let peak = curve.iter().cloned().fold(0.0f64, f64::min); // 最深压制（负 dB）
        let release_recovery = -curve[50];
        assert!(attack_depth > 5.0, "attack=2 步：5 tick 内应深压");
        assert!(
            release_recovery < -peak * 0.5,
            "release=40 步：30 tick 后压制应恢复过半（剩 {}dB < 峰值一半 {}dB）",
            release_recovery,
            -peak * 0.5
        );
    }
}
