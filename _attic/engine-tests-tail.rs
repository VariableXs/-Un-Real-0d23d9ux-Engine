
// ---------------------------------------------------------------- 测试（Mock 后端，逻辑核心全覆盖）
//
// 测试脚手架纪律：sh / be / config / events 必须是相互独立的局部变量
// （经 &mut 结构体的字段借用会被闭包整体捕获）；事件闭包必须内联传参，
// 让期望签名（dyn for<'x> FnMut(&'x EngineEvent)）推断出高阶生命周期。

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock 后端：脚本化心跳（按次消费，耗尽后重复末值）+ 失败点 + 计数器。
    struct MockBackend {
        /// 心跳脚本：每次探针按序消费一个值；耗尽后重复末值；空脚本=恒 true。
        heartbeat_script: Vec<bool>,
        /// 在该阶段失败（stage key）。
        fail_at: Option<&'static str>,
        probes: u32,
        mounts: u32,
        preps: u32,
        powers: u32,
        hibernates: u32,
        resumes: u32,
        stops: u32,
    }

    impl MockBackend {
        fn fast() -> Self {
            MockBackend {
                heartbeat_script: Vec::new(),
                fail_at: None,
                probes: 0,
                mounts: 0,
                preps: 0,
                powers: 0,
                hibernates: 0,
                resumes: 0,
                stops: 0,
            }
        }
        fn script(parts: &[bool]) -> Self {
            let mut m = MockBackend::fast();
            m.heartbeat_script = parts.to_vec();
            m
        }
        fn probe(&mut self) -> bool {
            if self.heartbeat_script.is_empty() {
                return true;
            }
            let idx = (self.probes as usize).min(self.heartbeat_script.len() - 1);
            self.probes += 1;
            self.heartbeat_script[idx]
        }
    }

    impl EngineBackend for MockBackend {
        fn name(&self) -> &'static str {
            "mock"
        }
        fn poll_delay(&self) -> Duration {
            Duration::ZERO
        }
        fn mount(&mut self, _c: &EngineConfig) -> Result<(), String> {
            self.mounts += 1;
            if self.fail_at == Some(STAGE_VHDX_MOUNT) {
                return Err("模拟挂载失败".into());
            }
            Ok(())
        }
        fn vm_prepare(&mut self, _c: &EngineConfig) -> Result<(), String> {
            self.preps += 1;
            if self.fail_at == Some(STAGE_VM_CREATE) {
                return Err("模拟建机失败".into());
            }
            Ok(())
        }
        fn vm_power(&mut self, _c: &EngineConfig) -> Result<(), String> {
            self.powers += 1;
            if self.fail_at == Some(STAGE_VM_POWER) {
                return Err("模拟上电失败".into());
            }
            Ok(())
        }
        fn heartbeat_once(&mut self, _c: &EngineConfig) -> bool {
            self.probe()
        }
        fn hibernate(&mut self, _c: &EngineConfig) -> Result<(), String> {
            self.hibernates += 1;
            Ok(())
        }
        fn resume(&mut self, _c: &EngineConfig) -> Result<(), String> {
            self.resumes += 1;
            Ok(())
        }
        fn stop(&mut self, _c: &EngineConfig) -> Result<(), String> {
            self.stops += 1;
            Ok(())
        }
    }

    fn cfg() -> EngineConfig {
        EngineConfig::explicit(
            "X:\\Engine\\Base.vhdx".into(),
            "X:\\Engine\\Apps.vhdx".into(),
            "X:\\Engine\\User.vhdx".into(),
        )
    }

    fn no_abort() -> bool {
        false
    }

    fn kinds(events: &[EngineEvent]) -> Vec<String> {
        events.iter().map(|e| e.kind.clone()).collect()
    }

    #[test]
    fn launch_emits_five_stages_then_ready_with_monotonic_seq() {
        let mut sh = EngineShared::new();
        let mut be = MockBackend::fast();
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        )
        .unwrap();
        assert_eq!(sh.state, EngineState::Ready);
        assert_eq!(sh.launch_count, 1);
        // 五阶段键与前端 ENGINE_BOOT_STAGES 严格一致，顺序正确。
        let stages: Vec<&str> = events
            .iter()
            .filter(|e| e.kind == "boot-stage")
            .filter_map(|e| e.stage.as_deref())
            .collect();
        assert_eq!(
            stages,
            vec![
                STAGE_VHDX_MOUNT,
                STAGE_VM_CREATE,
                STAGE_VM_POWER,
                STAGE_AGENT_HEARTBEAT,
                STAGE_READY_HANDSHAKE
            ]
        );
        assert_eq!(kinds(&events).last().unwrap(), "ready");
        for (a, b) in events.iter().zip(events.iter().skip(1)) {
            assert!(b.seq > a.seq, "seq 必须单调: {} -> {}", a.seq, b.seq);
        }
    }

    #[test]
    fn wake_is_idempotent_no_double_launch() {
        let mut sh = EngineShared::new();
        let mut be = MockBackend::fast();
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        )
        .unwrap();
        let again = wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        )
        .unwrap();
        assert!(!again, "重复 wake 必须被幂等吸收");
        assert_eq!(sh.launch_count, 1, "launch_count 必须为 1（不双开）");
        assert_eq!(be.preps, 1, "VM 准备只发生一次");
        assert_eq!(kinds(&events).iter().filter(|k| *k == "ready").count(), 1);
    }

    #[test]
    fn launch_failure_transitions_to_failed_with_crashed_event() {
        for stage in [STAGE_VHDX_MOUNT, STAGE_VM_CREATE, STAGE_VM_POWER] {
            let mut sh = EngineShared::new();
            let mut be = MockBackend::fast();
            be.fail_at = Some(stage);
            let config = cfg();
            let mut events: Vec<EngineEvent> = Vec::new();
            let err = wake(
                &mut sh, &mut be, &config,
                &mut |ev: &EngineEvent| events.push(ev.clone()),
                &no_abort,
            )
            .unwrap_err();
            assert_eq!(err.code, "ENGINE_LAUNCH_FAILED");
            assert_eq!(sh.state, EngineState::Failed);
            assert_eq!(kinds(&events).last().unwrap(), "crashed");
            assert!(events.last().unwrap().reason.is_some(), "失败必须带三要素原因");
        }
    }

    #[test]
    fn heartbeat_timeout_crashes_honestly() {
        let mut sh = EngineShared::new();
        let mut be = MockBackend::script(&[false]); // 永不就绪（零延迟 → 150 次探针瞬时）
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        let err = wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        )
        .unwrap_err();
        assert_eq!(err.code, "ENGINE_LAUNCH_FAILED");
        assert_eq!(sh.state, EngineState::Failed);
        let last = events.last().unwrap();
        assert_eq!(last.kind, "crashed");
        let reason = last.reason.as_deref().unwrap_or("");
        assert!(reason.contains("超时"), "超时原因必须如实：{reason}");
    }

    #[test]
    fn failed_can_recover_via_wake_reset() {
        let mut sh = EngineShared::new();
        let mut be = MockBackend::fast();
        be.fail_at = Some(STAGE_VM_POWER);
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        let err = wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        );
        assert!(err.is_err());
        be.fail_at = None;
        wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        )
        .unwrap();
        assert_eq!(sh.state, EngineState::Ready);
        assert_eq!(sh.launch_count, 2);
    }

    #[test]
    fn sleep_resume_roundtrip_and_states() {
        let mut sh = EngineShared::new();
        let mut be = MockBackend::fast();
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        )
        .unwrap();
        sleep(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone())).unwrap();
        assert_eq!(sh.state, EngineState::Hibernated);
        assert_eq!(kinds(&events).last().unwrap(), "hibernated");
        resume(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone())).unwrap();
        assert_eq!(sh.state, EngineState::Ready);
        assert_eq!(kinds(&events).last().unwrap(), "awake");
        resume(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone())).unwrap(); // 幂等
        assert_eq!(sh.state, EngineState::Ready);
        assert_eq!(be.hibernates, 1);
        assert_eq!(be.resumes, 1);
    }

    #[test]
    fn stop_from_ready_and_closed_idempotent() {
        let mut sh = EngineShared::new();
        let mut be = MockBackend::fast();
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        )
        .unwrap();
        stop(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone())).unwrap();
        assert_eq!(sh.state, EngineState::Closed);
        assert_eq!(kinds(&events).last().unwrap(), "closed");
        stop(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone())).unwrap(); // 幂等
        assert_eq!(be.stops, 1);
    }

    #[test]
    fn illegal_transition_rejected() {
        let mut sh = EngineShared::new();
        let mut be = MockBackend::fast();
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        let err = sleep(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone()))
            .unwrap_err();
        assert_eq!(err.code, "ENGINE_STATE");
        assert_eq!(sh.state, EngineState::Closed, "非法转移不改变状态");
    }

    #[test]
    fn wake_from_hibernated_auto_resumes() {
        let mut sh = EngineShared::new();
        let mut be = MockBackend::fast();
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        )
        .unwrap();
        sleep(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone())).unwrap();
        let launched = wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        )
        .unwrap();
        assert!(!launched, "Hibernated 的 wake 走恢复语义");
        assert_eq!(sh.state, EngineState::Ready);
        assert_eq!(be.resumes, 1);
        assert_eq!(kinds(&events).last().unwrap(), "awake");
    }

    #[test]
    fn watchdog_recovers_via_single_restart_then_ready() {
        let mut sh = EngineShared::new();
        // 心跳脚本：看门狗 tick1/2/3 各失败一次；重启后的拉起探针成功（耗尽后重复末值 true）。
        let mut be = MockBackend::script(&[false, false, false, true]);
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        )
        .unwrap();
        assert_eq!(sh.state, EngineState::Ready);
        watchdog_tick(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone()), &no_abort);
        watchdog_tick(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone()), &no_abort);
        assert_eq!(sh.state, EngineState::Ready, "未达阈值前不处置");
        watchdog_tick(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone()), &no_abort);
        assert_eq!(sh.state, EngineState::Ready, "重启一次后应回到 Ready");
        let ks = kinds(&events);
        assert!(ks.contains(&"closed".to_string()), "重启前必须发 closed 事件");
        assert_eq!(ks.last().unwrap(), "ready");
        assert_eq!(be.preps, 2, "重启 = 第二次 VM 准备");
        assert_eq!(sh.launch_count, 2);
    }

    #[test]
    fn watchdog_gives_up_after_restart_budget() {
        let mut sh = EngineShared::new();
        let mut ready_be = MockBackend::fast(); // 初始就绪
        let mut dead_be = MockBackend::script(&[false]); // 之后永久失联（含重启后的拉起）
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        wake(
            &mut sh, &mut ready_be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        )
        .unwrap();
        assert_eq!(sh.state, EngineState::Ready);
        for _ in 0..MISSES_BEFORE_RESTART {
            watchdog_tick(&mut sh, &mut dead_be, &config, &mut |ev: &EngineEvent| events.push(ev.clone()), &no_abort);
        }
        assert_eq!(sh.state, EngineState::Failed, "重启仍失联 → Failed");
        assert_eq!(kinds(&events).last().unwrap(), "crashed");
        // Failed 态 tick = no-op。
        let n = events.len();
        watchdog_tick(&mut sh, &mut dead_be, &config, &mut |ev: &EngineEvent| events.push(ev.clone()), &no_abort);
        assert_eq!(events.len(), n);
    }

    #[test]
    fn watchdog_ignores_non_ready_states() {
        let mut sh = EngineShared::new();
        let mut be = MockBackend::fast();
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        watchdog_tick(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone()), &no_abort);
        assert_eq!(sh.state, EngineState::Closed);
        assert!(events.is_empty());
    }

    #[test]
    fn usb_removed_closes_session_and_emits_once() {
        let mut sh = EngineShared::new();
        let mut be = MockBackend::fast();
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &no_abort,
        )
        .unwrap();
        notify_usb_removed(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone()));
        assert_eq!(sh.state, EngineState::Closed);
        assert_eq!(kinds(&events).last().unwrap(), "usb-removed");
        assert!(be.stops >= 1, "拔盘必须尽力停止 VM/卸盘");
        let n = events.len();
        notify_usb_removed(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone())); // 幂等
        assert_eq!(events.len(), n);
    }

    #[test]
    fn abort_during_launch_leaves_no_crash() {
        let mut sh = EngineShared::new();
        let mut be = MockBackend::fast();
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        // 从第一步起即中止（模拟拉起中拔盘）：布线层收到 ABORTED 后收束
        // Closed + usb-removed（emit_removed_once），逻辑核心不伪造崩溃事件。
        let always_abort = || true;
        let err = wake(
            &mut sh, &mut be, &config,
            &mut |ev: &EngineEvent| events.push(ev.clone()),
            &always_abort,
        )
        .unwrap_err();
        assert_eq!(err.code, "ENGINE_ABORTED");
        assert!(!kinds(&events).contains(&"crashed".to_string()));
    }

    #[test]
    fn replay_buffer_bounded_and_ordered() {
        let mut sh = EngineShared::new();
        let mut be = MockBackend::fast();
        let config = cfg();
        let mut events: Vec<EngineEvent> = Vec::new();
        for _ in 0..20 {
            wake(
                &mut sh, &mut be, &config,
                &mut |ev: &EngineEvent| events.push(ev.clone()),
                &no_abort,
            )
            .unwrap();
            stop(&mut sh, &mut be, &config, &mut |ev: &EngineEvent| events.push(ev.clone())).unwrap();
        }
        assert!(sh.replay.len() <= 128, "重放缓冲必须有界");
        let seqs: Vec<u64> = sh.replay.iter().map(|e| e.seq).collect();
        let mut sorted = seqs.clone();
        sorted.sort_unstable();
        assert_eq!(seqs, sorted, "replay 缓冲内 seq 保持追加序");
    }

    #[test]
    fn transition_table_matches_spec() {
        use EngineState::*;
        assert!(Closed.can_go(Launching));
        assert!(Launching.can_go(Ready));
        assert!(Launching.can_go(Failed));
        assert!(Ready.can_go(Hibernating));
        assert!(Ready.can_go(Closed));
        assert!(Ready.can_go(Failed));
        assert!(Hibernating.can_go(Hibernated));
        assert!(Hibernated.can_go(Ready));
        assert!(Hibernated.can_go(Closed));
        assert!(Failed.can_go(Closed));
        assert!(!Closed.can_go(Ready));
        assert!(!Closed.can_go(Hibernating));
        assert!(!Ready.can_go(Launching));
        assert!(!Hibernated.can_go(Hibernating));
        assert!(!Failed.can_go(Ready));
    }

    #[cfg(feature = "vm-agent")]
    #[test]
    fn heartbeat_port_matches_vm_agent() {
        assert_eq!(HEARTBEAT_PORT, crate::vm_agent::HEARTBEAT_PORT);
    }

    #[test]
    fn event_serde_contract_matches_frontend() {
        // 前端 EngineStateMsg: { seq, kind, stage?, reason? } —— 可选字段缺席不破坏解析。
        let mut sh = EngineShared::new();
        emit_event(&mut sh, "boot-stage", Some(STAGE_VHDX_MOUNT), None, &mut |_| {});
        let json = serde_json::to_value(&sh.replay[0]).unwrap();
        assert_eq!(json["kind"], "boot-stage");
        assert_eq!(json["stage"], STAGE_VHDX_MOUNT);
        assert!(json.get("reason").is_none(), "无 reason 时键必须缺席");
        assert!(json["seq"].is_u64());
    }
}
