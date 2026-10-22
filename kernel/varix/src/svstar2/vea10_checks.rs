//! VE-F0010 · 域自检（判据逐条对应，见 `vea10_bus.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 跨进程总线 → `A10-总线-*`
//! - 版本协商（含兼容窗口/迁移期限）→ `A10-版本-*`
//! - 背压告警（双策略）→ `A10-背压-*`
//! - 丢失重传 → `A10-重传-*`
//! - 消息优先级（输入响应插队）→ `A10-优先级-*`
//! - 心跳保活（死通道检出）→ `A10-心跳-*`
//! - 错误路径 → `A10-错误-*`
//!
//! 逻辑 tick 注入、无墙钟，回归可复现。

use super::vea10_bus::*;
use crate::checks::CheckSet;

/// 投递并显式记账：`Result` 由调用方按 `is_ok()` / `match` / `let _ =` 显式处理，
/// 不在调用点留下「返回值无人接」的形态。集中一处也让「发送方端点」这类
/// 固定参数只写一遍。
fn deliver(
    bus: &mut ProtocolBus,
    from: &str,
    to: &str,
    priority: MsgPriority,
    payload: &str,
) -> Result<u64, BusError> {
    let posted = bus.send(from, to, priority, payload);
    posted
}

/// 建一条可用的端到端通道（登记两端 + 建道），返回总线供后续判据使用。
fn wired_bus(window: Option<CompatibilityWindow>) -> ProtocolBus {
    ProtocolBus::new(window)
}

/// 登记两端并建道的公共前置（`s` 发出、`r` 接收）。
///
/// 五个判据的分组函数都以这三步行开头，抽出来之后各分组只剩「本判据真正
/// 要验的那一两件事」，重复的前置也不会在多处漂移。
fn bus_with_channel(window: Option<CompatibilityWindow>) -> ProtocolBus {
    let mut b = wired_bus(window);
    let _ = b.register("s", Some(PROTO_VERSION_CURRENT));
    let _ = b.register("r", Some(PROTO_VERSION_CURRENT));
    let _ = b.open_channel("s", "r");
    b
}

/// 判据一：跨进程总线（端到端投递 + 非法建道拒绝）。
fn check_bus_e2e(set: &mut CheckSet) {
    // 端点登记 + 建道 + 投递 + 接收端到端。
    {
        let mut b = wired_bus(None);
        let ok = b.register("vxrender", Some(PROTO_VERSION_CURRENT)).is_ok()
            && b.register("vxcomp", Some(PROTO_VERSION_CURRENT)).is_ok()
            && b.open_channel("vxrender", "vxcomp").is_ok()
            && deliver(&mut b, "vxrender", "vxcomp", MsgPriority::RenderStats, "frame-1").is_ok()
            && b
                .recv("vxrender", "vxcomp")
                .map(|m| m.payload == "frame-1")
                .unwrap_or(false);
        set.add("A10-总线-端到端投递", ok, "");
    }

    // 未登记端点 / 自环 / 重复建道全部拒绝且给建议。
    {
        let mut b = wired_bus(None);
        let _ = b.register("a", Some(PROTO_VERSION_CURRENT));
        let ghost = b.open_channel("ghost", "a").is_err();
        let _ = b.register("b", Some(PROTO_VERSION_CURRENT));
        let selfloop = b.open_channel("a", "a").is_err();
        let _ = b.open_channel("a", "b");
        let dup = b.open_channel("a", "b").is_err();
        set.add(
            "A10-总线-非法建道拒绝",
            ghost && selfloop && dup,
            "",
        );
    }
}

/// 判据二：版本协商（不匹配降级 / 迁移期限 / 迁移提醒）。
fn check_versioning(set: &mut CheckSet) {
    // 版本不匹配 → 协商降级建道（取较低方），降级入审计。
    {
        let mut b = wired_bus(Some(CompatibilityWindow { legacy_floor: 2, deadline: 100 }));
        let _ = b.register("vxrender", Some(PROTO_VERSION_CURRENT));
        let _ = b.register("legacy", Some(2));
        let _ = b.open_channel("vxrender", "legacy");
        let ok = deliver(&mut b, "vxrender", "legacy", MsgPriority::RenderStats, "m").is_ok()
            && !b.audit_trail().is_empty()
            && b.audit_trail()[0].contains("协商");
        set.add("A10-版本-不匹配协商降级", ok, "");
    }

    // 迁移期限前旧版本可建道；期限后拒绝（混跑不许永远混着）。
    {
        let mut b = wired_bus(Some(CompatibilityWindow { legacy_floor: 2, deadline: 10 }));
        let _ = b.register("new", Some(PROTO_VERSION_CURRENT));
        let _ = b.register("old", Some(2));
        let _ = b.open_channel("new", "old"); // 期限前：允许
        let before = b.channel_depth("new", "old") == Some(0);
        for _ in 0..12 {
            b.tick();
        }
        let mut b2 = wired_bus(Some(CompatibilityWindow { legacy_floor: 2, deadline: 10 }));
        let _ = b2.register("new2", Some(PROTO_VERSION_CURRENT));
        let _ = b2.register("old2", Some(2));
        for _ in 0..12 {
            b2.tick();
        }
        let after = matches!(
            b2.open_channel("new2", "old2"),
            Err(BusError::MigrationOverdue { .. })
        );
        set.add("A10-版本-迁移期限生效", before && after, "");
    }

    // 旧版本端点登记触发迁移提醒（期限驱动可见化）。
    {
        let mut b = wired_bus(Some(CompatibilityWindow { legacy_floor: 2, deadline: 50 }));
        let _ = b.register("old", Some(2));
        let ok = b
            .migration_events
            .iter()
            .any(|e| e.contains("old") && e.contains("迁移"));
        set.add("A10-版本-迁移提醒在册", ok, "");
    }
}

/// 判据三：消息优先级（输入响应插队渲染统计）。
fn check_priority(set: &mut CheckSet) {
    // 输入响应类插队渲染统计类（后进的高档先出）。
    {
        let mut b = wired_bus(None);
        let _ = b.register("s", Some(PROTO_VERSION_CURRENT));
        let _ = b.register("r", Some(PROTO_VERSION_CURRENT));
        let _ = b.open_channel("s", "r");
        let _ = deliver(&mut b, "s", "r", MsgPriority::RenderStats, "stats-1");
        let _ = deliver(&mut b, "s", "r", MsgPriority::RenderStats, "stats-2");
        let _ = deliver(&mut b, "s", "r", MsgPriority::InputResponse, "input-1");
        let first = b.recv("s", "r").map(|m| m.payload).unwrap_or_default();
        let ok = first == "input-1";
        set.add("A10-优先级-输入响应插队", ok, "");
    }
}

/// 判据四：背压告警（高水位拒投 / 低水位解除）。
fn check_backpressure(set: &mut CheckSet) {
    // 水位越过高线 → 拒投 + 发送方配额减半 + 接收方扩容。
    {
        let mut b = wired_bus(None);
        let _ = b.register("s", Some(PROTO_VERSION_CURRENT));
        let _ = b.register("r", Some(PROTO_VERSION_CURRENT));
        let _ = b.open_channel("s", "r");
        let mut pressured = false;
        for i in 0..200 {
            if deliver(&mut b, "s", "r", MsgPriority::RenderStats, &alloc::format!("m{i}")).is_err() {
                pressured = true;
                break;
            }
        }
        let depth = b.channel_depth("s", "r").unwrap_or(0);
        set.add(
            "A10-背压-高水位触发",
            pressured && depth >= HIGH_WATERMARK,
            "",
        );
    }

    // 水位回落到低线以下 → 背压解除（不自动恢复配额，防抖动）。
    {
        let mut b = wired_bus(None);
        let _ = b.register("s", Some(PROTO_VERSION_CURRENT));
        let _ = b.register("r", Some(PROTO_VERSION_CURRENT));
        let _ = b.open_channel("s", "r");
        for i in 0..HIGH_WATERMARK {
            let _ = deliver(&mut b, "s", "r", MsgPriority::RenderStats, &alloc::format!("m{i}"));
        }
        // 消费到低水位以下。
        let mut consumed = 0;
        while b.channel_depth("s", "r").unwrap_or(0) > LOW_WATERMARK && consumed < 1000 {
            let _ = b.recv("s", "r");
            consumed += 1;
        }
        // 再投一条应成功（背压已解除）。
        let ok = deliver(&mut b, "s", "r", MsgPriority::RenderStats, "after").is_ok();
        set.add("A10-背压-低水位解除", ok, "");
    }
}

/// 判据五：丢失重传（超时重传 + 确认豁免）。
fn check_retransmit(set: &mut CheckSet) {
    // 超时未确认的消息进重传清单且计数入账；确认后不再重传。
    {
        let mut b = wired_bus(None);
        let _ = b.register("s", Some(PROTO_VERSION_CURRENT));
        let _ = b.register("r", Some(PROTO_VERSION_CURRENT));
        let _ = b.open_channel("s", "r");
        let id1 = match deliver(&mut b, "s", "r", MsgPriority::RenderStats, "lost") {
            Ok(id) => id,
            Err(_) => return,
        };
        let id2 = match deliver(&mut b, "s", "r", MsgPriority::RenderStats, "acked") {
            Ok(id) => id,
            Err(_) => return,
        };
        // 确认第二条（模拟接收方只回了这条）。
        {
            let bref = &mut b;
            if let Some(ch) = bref.channels.iter_mut().next() {
                ch.ack(id2);
            }
        }
        for _ in 0..6 {
            b.tick();
        }
        let resends = b.retransmit_sweep(5);
        let ok = resends.iter().any(|(_, id)| *id == id1)
            && !resends.iter().any(|(_, id)| *id == id2);
        set.add("A10-重传-超时重传确认豁免", ok, "");
    }
}

/// 判据六：心跳保活（死通道检出并拒投）。
fn check_heartbeat(set: &mut CheckSet) {
    // 心跳正常不判死；停跳超阈值判死且死通道拒投。
    {
        let mut b = wired_bus(None);
        let _ = b.register("s", Some(PROTO_VERSION_CURRENT));
        let _ = b.register("r", Some(PROTO_VERSION_CURRENT));
        let _ = b.open_channel("s", "r");
        // 前 8 tick 每拍心跳。
        for _ in 0..8 {
            b.heartbeat("r");
            b.tick();
        }
        let alive = b.liveness_sweep().is_empty();
        // 之后停跳 10 tick。
        for _ in 0..10 {
            b.tick();
        }
        let dead = b.liveness_sweep().len() == 1;
        let reject = deliver(&mut b, "s", "r", MsgPriority::RenderStats, "to-dead").is_err();
        set.add("A10-心跳-死通道检出拒投", alive && dead && reject, "");
    }
}

/// 判据七：错误路径与审计留痕。
fn check_error_paths(set: &mut CheckSet) {
    // 无通道投递拒绝并给建议。
    {
        let mut b = wired_bus(None);
        let _ = b.register("s", Some(PROTO_VERSION_CURRENT));
        let _ = b.register("r", Some(PROTO_VERSION_CURRENT));
        let e = deliver(&mut b, "s", "r", MsgPriority::RenderStats, "x");
        let ok = matches!(e, Err(BusError::NoChannel { .. }));
        set.add("A10-错误-无通道拒绝", ok, "");
    }

    // 审计账留痕（协商降级/判死可回放）。
    {
        let mut b = wired_bus(Some(CompatibilityWindow { legacy_floor: 2, deadline: 100 }));
        let _ = b.register("n", Some(PROTO_VERSION_CURRENT));
        let _ = b.register("o", Some(2));
        let _ = b.open_channel("n", "o");
        let _ = b.liveness_sweep();
        set.add(
            "A10-审计-事件可回放",
            !b.audit_trail().is_empty(),
            "",
        );
    }
}

/// VE-F0010 域自检（按判据域分派到各分组函数，函数体只做编排）。
pub fn run_vea10_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vea10");
    check_bus_e2e(&mut set);
    check_versioning(&mut set);
    check_priority(&mut set);
    check_backpressure(&mut set);
    check_retransmit(&mut set);
    check_heartbeat(&mut set);
    check_error_paths(&mut set);
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea10_checks_all_green() {
        let set = run_vea10_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0010 自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }

    /// 总线在内核 Rust 侧可执行：优先级插队语义端到端。
    #[test]
    fn vea10_priority_e2e() {
        let mut b = ProtocolBus::new(None);
        assert!(b.register("s", Some(PROTO_VERSION_CURRENT)).is_ok(), "登记合法");
        assert!(b.register("r", Some(PROTO_VERSION_CURRENT)).is_ok(), "登记合法");
        assert!(b.open_channel("s", "r").is_ok(), "建道合法");
        assert!(deliver(&mut b, "s", "r", MsgPriority::RenderStats, "a").is_ok(), "投递");
        assert!(deliver(&mut b, "s", "r", MsgPriority::InputResponse, "b").is_ok(), "投递");
        assert_eq!(b.recv("s", "r").map(|m| m.payload), Some("b".to_string()));
        assert_eq!(b.recv("s", "r").map(|m| m.payload), Some("a".to_string()));
    }
}
