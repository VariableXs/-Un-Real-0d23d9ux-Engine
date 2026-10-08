//! VE-F1402 · 域自检（判据逐条对应，见 `veh02_audioarch.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 多消费者 → `H02-多消费者-*`
//! - 会话隔离 → `H02-隔离-*`
//! - 四类别 → `H02-类别-*`
//! - 生命周期 → `H02-生命周期-*`
//! - 故障域 → `H02-故障域-*`
//!
//! 注：H01 前缀已被 F1401（边界契约）占用，本项（F1402）自检用 H02 前缀。

use super::veh02_audioarch::*;
use crate::checks::CheckSet;

use alloc::{format, vec};

/// VE-F1402 域自检。
pub fn run_veh02_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veh02");

    // ---- 判据：多消费者 ----

    {
        let r = ServiceRationale::official();
        let four = r.consumers.len() == 4
            && r.consumers.iter().map(|c| c.kind).collect::<Vec<_>>()
                == vec![ConsumerKind::Media, ConsumerKind::System, ConsumerKind::System, ConsumerKind::Game];
        // 服务层各自独立（F1401 契约），共享核声明显性
        let layers: Vec<&str> = r.consumers.iter().map(|c| c.service_layer).collect();
        let independent = layers.iter().all(|l| l.contains("独立"));
        let shared = r.shared_core_statement().contains("共享") && r.shared_core_statement().contains("独立");
        set.add(
            "H02-多消费者-四消费者服务化",
            four && independent && shared && r.singleton_engine(),
            "",
        );
    }
    // 四消费者同引擎：全部能开立会话且互为独立会话
    {
        let mut eng = AudioEngineService::new();
        let s1 = eng.admit("G07 播放链", ConsumerKind::Media);
        let s2 = eng.admit("系统音效", ConsumerKind::System);
        let s3 = eng.admit("通知提醒", ConsumerKind::System);
        let s4 = eng.admit("游戏场景", ConsumerKind::Game);
        let ids = [s1, s2, s3, s4];
        set.add(
            "H02-多消费者-同引擎分会话",
            ids.iter().all(|id| eng.sessions.get(*id).is_some())
                && eng.sessions.len() == 4
                && ids[0] != ids[1] && ids[1] != ids[2],
            "",
        );
    }

    // ---- 判据：会话隔离 ----

    // 故障注入：一崩不连坐
    {
        let mut eng = AudioEngineService::new();
        let a = eng.admit("G07 播放链", ConsumerKind::Media);
        let b = eng.admit("游戏场景", ConsumerKind::Game);
        let _ = eng.sessions.get_mut(a).unwrap().request_voice();
        let _ = eng.sessions.get_mut(a).unwrap().request_voice();
        let _ = eng.sessions.get_mut(b).unwrap().request_voice();
        eng.sessions.get_mut(a).unwrap().inject_fault();
        let isolated = eng.sessions.isolation_holds_after_fault_of(a)
            && eng.sessions.get(b).unwrap().active_voices == 1
            && eng.sessions.get(b).unwrap().state == SessionState::Active;
        // 故障会话自身的声部申请被拦截（三要素）
        let blocked = eng
            .sessions
            .get_mut(a)
            .unwrap()
            .request_voice()
            .as_ref()
            .err()
            .map(|e| e.code == "E_SESSION_FAULT" && e.is_complete())
            .unwrap_or(false);
        set.add("H02-隔离-一崩不连坐", isolated && blocked, "");
    }
    // 配额分账：A 超配额拒绝不消耗 B 的配额（F1416 挂点）
    {
        let mut eng = AudioEngineService::new();
        let a = eng.admit("A", ConsumerKind::Communication); // 配额 8
        let b = eng.admit("B", ConsumerKind::Communication);
        let mut rejected = 0;
        for _ in 0..10 {
            match eng.sessions.get_mut(a).unwrap().request_voice() {
                Ok(_) => {}
                Err(e) => {
                    if e.code == "E_QUOTA_EXCEEDED" {
                        rejected += 1;
                    }
                }
            }
        }
        let b_ok = eng.sessions.get_mut(b).unwrap().request_voice().is_ok();
        set.add(
            "H02-隔离-配额按会话分账",
            rejected == 2
                && eng.sessions.get(a).unwrap().active_voices == 8
                && b_ok
                && eng.sessions.get(b).unwrap().active_voices == 1,
            "",
        );
    }

    // ---- 判据：四类别（类别决定路由与策略）----

    {
        // 策略表逐项：延迟预算、断连处置、保温资格互有区分
        let m = policy_of(ConsumerKind::Media);
        let c = policy_of(ConsumerKind::Communication);
        let s = policy_of(ConsumerKind::System);
        let g = policy_of(ConsumerKind::Game);
        let budgets = m.latency_budget_us == 40_000
            && c.latency_budget_us == 10_000
            && s.latency_budget_us == 80_000
            && g.latency_budget_us == 20_000;
        let rebuild = m.rebuild_on_disconnect && c.rebuild_on_disconnect
            && g.rebuild_on_disconnect && !s.rebuild_on_disconnect;
        let warm = m.keep_warm_eligible && c.keep_warm_eligible && g.keep_warm_eligible
            && !s.keep_warm_eligible;
        let quota = g.default_voice_quota == 128 && m.default_voice_quota == 64
            && s.default_voice_quota == 16 && c.default_voice_quota == 8;
        set.add(
            "H02-类别-策略表逐项区分",
            budgets && rebuild && warm && quota,
            "",
        );
    }
    // 类别错了体验就错：通信延迟预算必须严于系统
    {
        let c = policy_of(ConsumerKind::Communication);
        let s = policy_of(ConsumerKind::System);
        set.add(
            "H02-类别-通信严于系统",
            c.latency_budget_us < s.latency_budget_us
                && c.reconnect_priority < s.reconnect_priority,
            "",
        );
    }

    // ---- 判据：生命周期（两模式 + 保温）----

    {
        // 按需模式：空闲超时即睡
        let mut eng = AudioEngineService::new();
        eng.lifecycle.idle_timeout_us = 5_000_000;
        eng.advance(6_000_000); // 无会话 → 睡
        let slept = eng.lifecycle.asleep && eng.lifecycle.sleep_count == 1;
        // 来活即起
        eng.admit("G07 播放链", ConsumerKind::Media);
        let woke = !eng.lifecycle.asleep && eng.lifecycle.wake_count == 1;
        set.add("H02-生命周期-按需睡起", slept && woke, "");
    }
    // 保温：有保温资格活跃会话时不睡（高频消费者保温）
    {
        let mut eng = AudioEngineService::new();
        eng.admit("G07 播放链", ConsumerKind::Media); // 保温资格
        eng.advance(10_000_000);
        let warm_holds = !eng.lifecycle.asleep;
        // 系统类不保温：只有系统类会话时照睡
        let mut eng2 = AudioEngineService::new();
        eng2.admit("系统音效", ConsumerKind::System); // 无保温资格
        eng2.advance(10_000_000);
        let system_sleeps = eng2.lifecycle.asleep && eng2.lifecycle.sleep_count == 1;
        set.add("H02-生命周期-保温资格生效", warm_holds && system_sleeps, "");
    }
    // 常驻模式永不睡 + 策略可配（切回按需后恢复睡起语义）
    {
        let mut eng = AudioEngineService::new();
        eng.lifecycle.set_mode(LifecycleMode::Resident);
        eng.advance(100_000_000);
        let resident = !eng.lifecycle.asleep;
        eng.lifecycle.set_mode(LifecycleMode::OnDemand);
        eng.advance(106_000_000);
        let back_on_demand = eng.lifecycle.asleep;
        set.add("H02-生命周期-常驻与可配", resident && back_on_demand, "");
    }

    // ---- 判据：故障域（自动重启 + 重连协议）----

    {
        // 崩溃 → 自动重启（退避指数、封顶 1s）→ 会话全部转断连
        let mut eng = AudioEngineService::new();
        let a = eng.admit("G07 播放链", ConsumerKind::Media);
        let b = eng.admit("通知提醒", ConsumerKind::System);
        let rec = eng.crash_drill();
        let all_disc = [a, b]
            .iter()
            .all(|id| eng.sessions.get(*id).unwrap().state == SessionState::Disconnected);
        // 二次及以后崩溃退避增长且封顶
        let rec2 = eng.crash_drill();
        let rec3 = eng.crash_drill();
        let mut rec_cap = rec3.clone();
        for _ in 0..6 {
            rec_cap = eng.crash_drill();
        }
        set.add(
            "H02-故障域-重启退避封顶",
            rec.backoff_us == 50_000
                && rec2.backoff_us == 100_000
                && rec3.backoff_us == 200_000
                && rec_cap.backoff_us == RESTART_BACKOFF_CAP_US
                && all_disc
                && eng.fault.engine_alive,
            "",
        );
    }
    // 重连协议：可重建类别重建成功（新会话）、系统类即发即弃、协议不匹配拒绝
    {
        let mut eng = AudioEngineService::new();
        let _ = eng.admit("G07 播放链", ConsumerKind::Media);
        eng.crash_drill();
        let before = eng.sessions.len();
        let v1 = eng.fault.reconnect(
            &mut eng.sessions,
            &ReconnectRequest {
                consumer: "G07 播放链",
                kind: ConsumerKind::Media,
                state_version: RECONNECT_PROTOCOL_VERSION,
            },
        );
        let v2 = eng.fault.reconnect(
            &mut eng.sessions,
            &ReconnectRequest {
                consumer: "通知提醒",
                kind: ConsumerKind::System,
                state_version: RECONNECT_PROTOCOL_VERSION,
            },
        );
        let v3 = eng.fault.reconnect(
            &mut eng.sessions,
            &ReconnectRequest {
                consumer: "游戏场景",
                kind: ConsumerKind::Game,
                state_version: 99,
            },
        );
        let rebuilt = matches!(v1, ReconnectVerdict::Rebuilt { .. });
        let new_id = match &v1 {
            ReconnectVerdict::Rebuilt { new_session } => *new_session,
            _ => 0,
        };
        set.add(
            "H02-故障域-重连协议三裁决",
            rebuilt
                && v2 == ReconnectVerdict::NotRebuildable
                && v3 == ReconnectVerdict::ProtocolMismatch
                && eng.sessions.len() == before + 1
                && eng.sessions.get(new_id).unwrap().state == SessionState::Rebuilding,
            "",
        );
    }
    // 重连顺序：通信最优先（类别决定路由）
    {
        let order = FaultDomain::reconnect_order(&[
            ConsumerKind::System,
            ConsumerKind::Game,
            ConsumerKind::Media,
            ConsumerKind::Communication,
        ]);
        set.add(
            "H02-故障域-重连顺序按优先级",
            order.first() == Some(&ConsumerKind::Communication)
                && order.last() == Some(&ConsumerKind::System),
            "",
        );
    }

    // ---- 读屏与确定性 ----

    {
        let mut eng = AudioEngineService::new();
        eng.admit("G07 播放链", ConsumerKind::Media);
        let s = eng.a11y_summary();
        set.add(
            "H02-读屏-服务摘要可播",
            s.contains("音频引擎服务") && s.contains("会话") && s.contains("重启"),
            "",
        );
    }
    {
        let run = || {
            let mut eng = AudioEngineService::new();
            let a = eng.admit("G07 播放链", ConsumerKind::Media);
            let _ = eng.sessions.get_mut(a).unwrap().request_voice();
            eng.advance(6_000_000);
            let rec = eng.crash_drill();
            let v = eng.fault.reconnect(
                &mut eng.sessions,
                &ReconnectRequest {
                    consumer: "G07 播放链",
                    kind: ConsumerKind::Media,
                    state_version: RECONNECT_PROTOCOL_VERSION,
                },
            );
            (eng.a11y_summary(), rec.backoff_us, format!("{:?}", v))
        };
        let x = run();
        let y = run();
        set.add("H02-确定-同操作同账面", x == y, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    /// 域自检必须全绿——红项即施工未完成。
    #[test]
    fn veh02_checks_all_green() {
        let set = run_veh02_checks();
        let (passed, failed) = set.tally();
        if !set.all_passed() {
            let (items, n) = set.red_items();
            let mut msg = format!("VE-H02 域自检红项：{}/{} 绿", passed, passed + failed);
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

    /// 隔离铁律：会话故障只影响本会话。
    #[test]
    fn fault_isolation() {
        let mut eng = AudioEngineService::new();
        let a = eng.admit("A", ConsumerKind::Media);
        let b = eng.admit("B", ConsumerKind::Game);
        eng.sessions.get_mut(a).unwrap().inject_fault();
        assert!(eng.sessions.isolation_holds_after_fault_of(a));
        assert!(eng.sessions.get_mut(b).unwrap().request_voice().is_ok());
        assert!(eng.sessions.get_mut(a).unwrap().request_voice().is_err());
    }

    /// 生命周期：默认按需 + 保温资格防睡。
    #[test]
    fn lifecycle_on_demand_and_keep_warm() {
        let mut eng = AudioEngineService::new();
        assert_eq!(eng.lifecycle.mode, LifecycleMode::OnDemand);
        eng.admit("G07 播放链", ConsumerKind::Media);
        eng.advance(60_000_000);
        assert!(!eng.lifecycle.asleep, "保温会话在场不许睡");
        eng.sessions.close(1);
        eng.advance(70_000_000);
        assert!(eng.lifecycle.asleep, "无保温会话后空闲即睡");
    }

    /// 故障域：退避封顶 + 系统类不重建。
    #[test]
    fn fault_domain_contract() {
        let mut eng = AudioEngineService::new();
        eng.crash_drill();
        eng.crash_drill();
        let rec = eng.crash_drill();
        assert_eq!(rec.backoff_us, 200_000);
        let v = eng.fault.reconnect(
            &mut eng.sessions,
            &ReconnectRequest {
                consumer: "系统音效",
                kind: ConsumerKind::System,
                state_version: RECONNECT_PROTOCOL_VERSION,
            },
        );
        assert_eq!(v, ReconnectVerdict::NotRebuildable);
    }
}
