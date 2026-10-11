//! VE-F0003 续 · 域自检与回归用例
//!
//! 与 `vea03_sync` 同属一项功能（VE-F0003），拆文件只为避开超长单文件。
//! 本文件只放自检构造与 `#[cfg(test)]` 用例，逻辑全在 `vea03_sync`。

use super::vea03_sync::{
    DEADLOCK_STALL_TICKS, MAX_PRIMITIVES, PrimitiveKind, SyncManager, WaitEdge, WaitOutcome,
    WaitRequest, WaiterIdentity, default_timeout, detect_deadlock, new_event, new_fence,
    new_semaphore, resolve_timeout, stall_warning,
};

use crate::checks::CheckSet;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 构造两端互持的等待链（死锁检测用）。
fn deadlock_chain() -> Vec<WaitEdge> {
    alloc::vec![
        WaitEdge {
            from: "render-a".into(),
            waiting_on: "fence-b".into(),
            blocked_by: Some("render-b".into()),
            waited_ticks: 10,
        },
        WaitEdge {
            from: "render-b".into(),
            waiting_on: "fence-a".into(),
            blocked_by: Some("render-a".into()),
            waited_ticks: 12,
        },
        WaitEdge {
            from: "upload-c".into(),
            waiting_on: "fence-a".into(),
            blocked_by: Some("render-a".into()),
            waited_ticks: 3,
        },
        WaitEdge {
            from: "present-d".into(),
            waiting_on: "fence-b".into(),
            blocked_by: Some("render-b".into()),
            waited_ticks: 1,
        },
    ]
}

/// 白名单变体：把 render-a 换成 composer（按固定顺序申请，成环也不该报）。
fn whitelisted_chain() -> Vec<WaitEdge> {
    alloc::vec![
        WaitEdge {
            from: "composer".into(),
            waiting_on: "fence-b".into(),
            blocked_by: Some("render-b".into()),
            waited_ticks: 10,
        },
        WaitEdge {
            from: "render-b".into(),
            waiting_on: "fence-a".into(),
            blocked_by: Some("composer".into()),
            waited_ticks: 12,
        },
        WaitEdge {
            from: "upload-c".into(),
            waiting_on: "fence-a".into(),
            blocked_by: Some("composer".into()),
            waited_ticks: 3,
        },
        WaitEdge {
            from: "present-d".into(),
            waiting_on: "fence-b".into(),
            blocked_by: Some("render-b".into()),
            waited_ticks: 1,
        },
    ]
}

/// 无环等待链。
fn acyclic_chain() -> Vec<WaitEdge> {
    alloc::vec![
        WaitEdge {
            from: "a".into(),
            waiting_on: "f1".into(),
            blocked_by: Some("b".into()),
            waited_ticks: 5,
        },
        WaitEdge {
            from: "b".into(),
            waiting_on: "f2".into(),
            blocked_by: None,
            waited_ticks: 5,
        },
        WaitEdge {
            from: "c".into(),
            waiting_on: "f3".into(),
            blocked_by: None,
            waited_ticks: 5,
        },
        WaitEdge {
            from: "d".into(),
            waiting_on: "f4".into(),
            blocked_by: None,
            waited_ticks: 5,
        },
    ]
}

fn req_of(kind: PrimitiveKind, who: &str, why: &str) -> WaitRequest {
    WaitRequest::with_default(kind, WaiterIdentity::new(who, why))
}

/// VE-F0003 域自检。逐条判据一项一 check。
pub fn run_vea03_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vea03");

    // 判据：三类原语齐备
    {
        let mut m = SyncManager::new();
        let f = m.register(new_fence("f1", "帧完成")).is_ok();
        let s = m.register(new_semaphore("s1", "显存额度", 2, 8)).is_ok();
        let e = m.register(new_event("e1", "焦点就绪", false, true)).is_ok();
        set.add(
            "A03-三类原语-齐备",
            f && s && e
                && m.get("f1").map(|p| p.kind) == Some(PrimitiveKind::Fence)
                && m.get("s1").map(|p| p.kind) == Some(PrimitiveKind::Semaphore)
                && m.get("e1").map(|p| p.kind) == Some(PrimitiveKind::Event),
            "",
        );
    }

    // 判据：信号量上限防膨胀
    {
        let mut m = SyncManager::new();
        let _ = m.register(new_semaphore("s", "额度", 0, 2));
        let ok1 = m.signal("s").is_ok() && m.signal("s").is_ok();
        let over = m.signal("s");
        set.add(
            "A03-信号量-上限防膨胀",
            ok1 && over.is_err() && m.get("s").map(|p| p.count) == Some(2),
            "",
        );
    }

    // 判据：同步语义显式（等谁 + 用途必填）
    {
        let mut m = SyncManager::new();
        let _ = m.register(new_fence("f", "围栏"));
        let no_who = m.wait("f", &req_of(PrimitiveKind::Fence, "", "用途"));
        let no_why = m.wait("f", &req_of(PrimitiveKind::Fence, "main", ""));
        set.add(
            "A03-语义显式-等谁与用途必填",
            no_who.is_err() && no_why.is_err(),
            "",
        );
    }

    // 判据：超时值的有据默认（每项带实测来源）
    {
        let has_src = [PrimitiveKind::Fence, PrimitiveKind::Semaphore, PrimitiveKind::Event]
            .iter()
            .all(|k| !default_timeout(*k).source.is_empty());
        set.add("A03-超时-有据默认带来源", has_src, "");
    }

    // 判据：显式超时未给来源 ⇒ 如实标注为拍脑袋
    {
        let r = WaitRequest {
            waiter: WaiterIdentity::new("main", "等帧"),
            timeout_ticks: 99,
            timeout_rationale: String::new(),
        };
        let (_, src) = resolve_timeout(&r, PrimitiveKind::Fence);
        set.add("A03-超时-无来源如实标注", src.contains("拍脑袋"), "");
    }

    // 判据：已就绪立即返回
    {
        let mut m = SyncManager::new();
        let _ = m.register(new_event("e", "焦点", true, false));
        let r = m.wait("e", &req_of(PrimitiveKind::Event, "main", "等焦点"));
        set.add(
            "A03-等待-就绪立即返回",
            matches!(r, Ok(WaitOutcome::Signaled { .. })),
            "",
        );
    }

    // 判据：等待链可视化（谁在等谁）
    {
        let mut m = SyncManager::new();
        let _ = m.register(new_fence("f1", "A"));
        let _ = m.register(new_fence("f2", "B"));
        let _ = m.set_holder("f2", Some("render-b"));
        let _ = m.wait("f2", &req_of(PrimitiveKind::Fence, "render-a", "等B"));
        let c = m.wait_chain();
        set.add(
            "A03-等待链-可视化",
            c.len() == 1
                && c[0].from == "render-a"
                && c[0].waiting_on == "f2"
                && c[0].blocked_by.as_deref() == Some("render-b"),
            "",
        );
    }

    // 判据：死锁成环判定
    {
        let v = detect_deadlock(&deadlock_chain());
        set.add(
            "A03-死锁-成环判定",
            v.deadlock && !v.cycle.is_empty() && !v.break_advice.is_empty(),
            "",
        );
    }

    // 判据：死锁误报白名单（已知安全模式不误报）
    {
        let v = detect_deadlock(&whitelisted_chain());
        set.add("A03-死锁-白名单不误报", !v.deadlock && v.whitelisted, "");
    }

    // 判据：链长不足门槛不检测
    {
        let short = alloc::vec![WaitEdge {
            from: "render-a".into(),
            waiting_on: "f".into(),
            blocked_by: Some("render-b".into()),
            waited_ticks: 5,
        }];
        set.add("A03-死锁-链短不检测", !detect_deadlock(&short).deadlock, "");
    }

    // 判据：无环不误报
    {
        set.add(
            "A03-死锁-无环不误报",
            !detect_deadlock(&acyclic_chain()).deadlock,
            "",
        );
    }

    // 判据：停滞告警（不成环但等待过久也告警）
    {
        let mut stale = deadlock_chain();
        for e in stale.iter_mut() {
            e.waited_ticks = DEADLOCK_STALL_TICKS;
            e.blocked_by = None;
        }
        set.add("A03-停滞-超时告警", stall_warning(&stale).is_some(), "");
    }

    // 判据：超时显式处置（带建议）
    {
        let mut m = SyncManager::new();
        let _ = m.register(new_fence("f", "未完成围栏"));
        let _ = m.set_holder("f", Some("other"));
        let rq = WaitRequest::with_timeout(WaiterIdentity::new("main", "等帧"), 1, "测试用短预算");
        let _ = m.wait("f", &rq);
        m.advance(5);
        let r = m.wait("f", &rq);
        let advised = match &r {
            Ok(WaitOutcome::Timeout { advice, .. }) => !advice.is_empty(),
            _ => false,
        };
        set.add("A03-超时-显式处置带建议", advised, "");
    }

    // 判据：销毁必通知等待者（规格点名义务）
    {
        let mut m = SyncManager::new();
        let _ = m.register(new_fence("f", "将被销毁"));
        let _ = m.set_holder("f", Some("other"));
        for w in ["main", "render-1", "render-2"].iter() {
            let _ = m.wait("f", &req_of(PrimitiveKind::Fence, w, "等帧"));
        }
        let r = m.destroy("f");
        let notified = matches!(&r, Ok(WaitOutcome::Destroyed { notified: 3, .. }));
        let advised = match &r {
            Ok(WaitOutcome::Destroyed { advice, .. }) => advice.contains("等待者"),
            _ => false,
        };
        set.add(
            "A03-销毁-通知等待者义务",
            notified && advised && m.wait_chain().is_empty(),
            "",
        );
    }

    // 判据：销毁后操作一律拒绝（语义混乱→拒绝）
    {
        let mut m = SyncManager::new();
        let _ = m.register(new_fence("f", "x"));
        let _ = m.destroy("f");
        let s = m.signal("f");
        let w = m.wait("f", &req_of(PrimitiveKind::Fence, "main", "等"));
        set.add("A03-销毁-后续操作拒绝", s.is_err() && w.is_err(), "");
    }

    // 判据：强制释放可打破死锁
    {
        let mut m = SyncManager::new();
        let _ = m.register(new_fence("fa", "A"));
        let _ = m.register(new_fence("fb", "B"));
        let _ = m.set_holder("fa", Some("render-a"));
        let _ = m.set_holder("fb", Some("render-b"));
        let _ = m.wait("fb", &req_of(PrimitiveKind::Fence, "render-a", "等B"));
        let _ = m.wait("fa", &req_of(PrimitiveKind::Fence, "render-b", "等A"));
        set.add(
            "A03-死锁-强制释放手术刀",
            m.force_release("render-a").is_ok(),
            "",
        );
    }

    // 判据：边界——上限 / 重复 id / 空 id 拒绝
    {
        let mut m = SyncManager::new();
        for i in 0..MAX_PRIMITIVES {
            let _ = m.register(new_fence(&format!("f{}", i), "x"));
        }
        let over = m.register(new_fence("overflow", "x"));
        let mut m2 = SyncManager::new();
        let _ = m2.register(new_fence("dup", "x"));
        let dup = m2.register(new_fence("dup", "y"));
        let empty = m2.register(new_fence("", "x"));
        set.add(
            "A03-边界-上限与重复拒绝",
            over.is_err() && dup.is_err() && empty.is_err(),
            "",
        );
    }

    // 判据：读屏可达（等待链与死锁状态可播报）
    {
        let mut m = SyncManager::new();
        let _ = m.register(new_fence("f1", "A"));
        let _ = m.register(new_semaphore("s1", "额度", 1, 4));
        let s = m.a11y_summary();
        set.add(
            "A03-读屏-同步状态可播",
            s.contains("同步原语") && s.contains("围栏") && s.contains("无等待"),
            "",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea03_checks_all_green() {
        let set = run_vea03_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-A03 域自检红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }

    #[test]
    fn three_primitive_kinds_registered() {
        let mut m = SyncManager::new();
        assert!(m.register(new_fence("f", "围栏")).is_ok());
        assert!(m.register(new_semaphore("s", "信号量", 1, 4)).is_ok());
        assert!(m.register(new_event("e", "事件", false, true)).is_ok());
        assert_eq!(m.get("f").map(|p| p.kind), Some(PrimitiveKind::Fence));
        assert_eq!(m.get("s").map(|p| p.kind), Some(PrimitiveKind::Semaphore));
        assert_eq!(m.get("e").map(|p| p.kind), Some(PrimitiveKind::Event));
    }

    #[test]
    fn semaphore_respects_cap() {
        let mut m = SyncManager::new();
        m.register(new_semaphore("s", "x", 0, 2)).unwrap();
        assert!(m.signal("s").is_ok());
        assert!(m.signal("s").is_ok());
        assert!(m.signal("s").is_err(), "超上限必须拒绝");
        assert_eq!(m.get("s").map(|p| p.count), Some(2));
    }

    #[test]
    fn wait_requires_who_and_why() {
        let mut m = SyncManager::new();
        m.register(new_fence("f", "x")).unwrap();
        assert!(m
            .wait("f", &req_of(PrimitiveKind::Fence, "", "用途"))
            .is_err());
        assert!(m
            .wait("f", &req_of(PrimitiveKind::Fence, "main", ""))
            .is_err());
    }

    #[test]
    fn defaults_carry_measured_source() {
        for k in [PrimitiveKind::Fence, PrimitiveKind::Semaphore, PrimitiveKind::Event] {
            assert!(!default_timeout(k).source.is_empty(), "{:?} 缺实测来源", k);
        }
    }

    #[test]
    fn deadlock_detected_on_cycle() {
        let v = detect_deadlock(&deadlock_chain());
        assert!(v.deadlock);
        assert!(!v.cycle.is_empty());
    }

    #[test]
    fn whitelist_suppresses_false_positive() {
        let v = detect_deadlock(&whitelisted_chain());
        assert!(!v.deadlock, "白名单内不应误报");
        assert!(v.whitelisted);
    }

    #[test]
    fn destroy_notifies_all_waiters() {
        let mut m = SyncManager::new();
        m.register(new_fence("f", "x")).unwrap();
        m.set_holder("f", Some("other")).unwrap();
        for w in ["main", "r1", "r2"].iter() {
            let _ = m.wait("f", &req_of(PrimitiveKind::Fence, w, "等帧"));
        }
        assert_eq!(m.wait_chain().len(), 3);
        match m.destroy("f").unwrap() {
            WaitOutcome::Destroyed { notified, .. } => assert_eq!(notified, 3),
            _ => panic!("应为销毁通知"),
        }
        assert!(m.wait_chain().is_empty(), "通知后等待链须清空");
    }

    #[test]
    fn destroyed_primitive_rejects_operations() {
        let mut m = SyncManager::new();
        m.register(new_fence("f", "x")).unwrap();
        let _ = m.destroy("f");
        assert!(m.signal("f").is_err());
        assert!(m.wait("f", &req_of(PrimitiveKind::Fence, "m", "等")).is_err());
    }

    #[test]
    fn wait_chain_is_human_readable() {
        let mut m = SyncManager::new();
        m.register(new_fence("f1", "A")).unwrap();
        m.register(new_fence("f2", "B")).unwrap();
        m.set_holder("f2", Some("render-b")).unwrap();
        let _ = m.wait("f2", &req_of(PrimitiveKind::Fence, "render-a", "等B"));
        let c = m.wait_chain();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].from, "render-a");
        assert_eq!(c[0].waiting_on, "f2");
        assert_eq!(c[0].blocked_by.as_deref(), Some("render-b"));
    }
}
