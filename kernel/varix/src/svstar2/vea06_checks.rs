//! VE-F0006 · 域自检（判据逐条对应，见 `vea06_qsched.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 三队列、优先级仲裁、空转检测、保底序 → 基础四项
//! - 优先级反转防护（低优持锁高优等待的检测）→ `A06-反转-*`
//! - 空转检测含误报排除（真没活不算空转）→ `A06-空转-误报排除`
//! - 重平衡含迁移成本声明 → `A06-重平衡-*`
//! - 队列与线程绑定的核亲缘声明 → `A06-亲缘-*`
//! - 降级矩阵（空转→告警/饥饿→保底序/失衡→重平衡）→ 对应各项
//! - 无障碍读屏可达 → `A06-读屏-*`
//! - 零静默错误账本 → `A06-错误-*`
//!
//! 逻辑时钟注入、无墙钟，回归可复现。

use super::vea06_qsched::*;
use crate::checks::CheckSet;

use alloc::vec;
use alloc::vec::Vec;

/// 默认优先级的图形任务。
fn g_task(id: u64, prio: TaskPriority) -> CmdTask {
    CmdTask::new(id, QueueKind::Graphics, prio, 4096)
}

/// VE-F0006 域自检。
pub fn run_vea06_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vea06");

    // ---- 基础四判据 ----

    // 判据：三队列（任务按类别路由到图形/计算/拷贝三条独立队列）。
    {
        let mut s = QueueScheduler::new();
        let _ = s.enqueue(CmdTask::new(1, QueueKind::Graphics, TaskPriority::Interactive, 4096));
        let _ = s.enqueue(CmdTask::new(2, QueueKind::Compute, TaskPriority::Interactive, 4096));
        let _ = s.enqueue(CmdTask::new(3, QueueKind::Copy, TaskPriority::Interactive, 4096));
        let ok = s.queue(QueueKind::Graphics).pending.len() == 1
            && s.queue(QueueKind::Compute).pending.len() == 1
            && s.queue(QueueKind::Copy).pending.len() == 1;
        set.add("A06-三队列-按类路由", ok, "");
    }

    // 判据：优先级仲裁（同队列高优先级先派，同级 FIFO）。
    {
        let mut s = QueueScheduler::new();
        let _ = s.enqueue(g_task(1, TaskPriority::Background));
        let _ = s.enqueue(g_task(2, TaskPriority::Realtime));
        let _ = s.enqueue(g_task(3, TaskPriority::Interactive));
        let _ = s.enqueue(g_task(4, TaskPriority::Interactive)); // 同级 FIFO：3 先于 4
        let dispatched = s.dispatch();
        let ok = dispatched == vec![2u64] && s.queue(QueueKind::Graphics).pending.len() == 3;
        set.add("A06-仲裁-高优先级先派", ok, "");
    }

    // 判据：空转检测（有活不派 → 连续 SPIN_TICK_LIMIT tick 后告警）。
    {
        let mut s = QueueScheduler::new();
        let _ = s.enqueue(g_task(1, TaskPriority::Interactive));
        // 不派发，空转累计：阈值 8，超过即告警一次。
        let mut alarmed = false;
        for _ in 0..(SPIN_TICK_LIMIT + 2) {
            s.advance_tick();
            if s.queue(QueueKind::Graphics).spin_alarms > 0 {
                alarmed = true;
            }
        }
        let ok = alarmed
            && s.events().iter().any(|e| matches!(e, SchedEvent::SpinAlarm { pending: 1, .. }));
        set.add("A06-空转-有活不派告警", ok, "");
    }

    // 判据：保底序（Background 等待老化升级，不饿死）。
    {
        let mut s = QueueScheduler::new();
        let _ = s.enqueue(g_task(1, TaskPriority::Background));
        // 老化阈值 16：tick - 入队 > 16 时升级（第 17 tick 触发）。
        for _ in 0..AGING_TICKS + 1 {
            s.advance_tick();
        }
        let boosted = s
            .events()
            .iter()
            .any(|e| matches!(e, SchedEvent::AgedBoost { task_id: 1, .. }));
        let dispatched = s.dispatch();
        let ok = boosted
            && dispatched == vec![1u64]
            && s.running()[0].task.priority == TaskPriority::Interactive;
        set.add("A06-保底序-老化升级可派", ok, "");
    }

    // ---- 优先级反转防护 ----

    // 判据：低优持锁高优等待 → 检出并继承提级。
    {
        let mut s = QueueScheduler::new();
        // 低优任务持锁 R 运行中。
        let _ = s.enqueue(
            CmdTask::new(10, QueueKind::Graphics, TaskPriority::Background, 4096)
                .with_resource("R"),
        );
        let _ = s.dispatch(); // 任务 10 上核
        // 高优任务声明等待同一资源 R，排入队列。
        let _ = s.enqueue(
            CmdTask::new(11, QueueKind::Graphics, TaskPriority::Realtime, 4096)
                .with_resource("R"),
        );
        s.advance_tick();
        let inverted = s.events().iter().any(|e| matches!(
            e,
            SchedEvent::InversionBoost { waiter_id: 11, holder_id: 10, resource: "R", .. }
        ));
        // 继承：持有者有效优先级被抬到 Realtime。
        let inherited = s
            .running()
            .iter()
            .any(|r| r.task.id == 10 && r.effective_priority == TaskPriority::Realtime);
        set.add("A06-反转-检出并继承提级", inverted && inherited, "");
    }

    // ---- 空转误报排除 ----

    // 判据：空转检测含误报排除（真没活 / 执行槽忙 / 有意暂停 都不算空转）。
    {
        // 场景 1：真没活。
        let mut s = QueueScheduler::new();
        for _ in 0..SPIN_TICK_LIMIT + 2 {
            s.advance_tick();
        }
        let no_task = s.queue(QueueKind::Graphics).spin_alarms == 0;
        // 场景 2：执行槽忙（有任务在跑，队列里还有活的——不是调度器不派）。
        let mut s2 = QueueScheduler::new();
        let _ = s2.enqueue(g_task(1, TaskPriority::Interactive));
        let _ = s2.dispatch();
        let _ = s2.enqueue(g_task(2, TaskPriority::Background));
        for _ in 0..SPIN_TICK_LIMIT + 2 {
            s2.advance_tick();
        }
        let core_busy_not_spin = s2.queue(QueueKind::Graphics).spin_alarms == 0;
        // 场景 3：有意暂停。
        let mut s3 = QueueScheduler::new();
        let _ = s3.enqueue(g_task(1, TaskPriority::Interactive));
        s3.pause(QueueKind::Graphics);
        for _ in 0..SPIN_TICK_LIMIT + 2 {
            s3.advance_tick();
        }
        let paused_not_spin = s3.queue(QueueKind::Graphics).spin_alarms == 0;
        set.add(
            "A06-空转-误报排除",
            no_task && core_busy_not_spin && paused_not_spin,
            "",
        );
    }

    // ---- 重平衡 ----

    // 判据：重平衡含迁移成本声明（失衡时迁移，每条迁移声明成本）。
    {
        let mut s = QueueScheduler::new();
        // 计算队列塞 8 个任务（超载），拷贝队列留空。
        for i in 0..8u64 {
            let _ = s.enqueue(CmdTask::new(
                100 + i,
                QueueKind::Compute,
                TaskPriority::Background,
                MIGRATION_COST_UNIT * 2, // 成本可整除：每任务 3 tick（+1 规则）
            ));
        }
        let moved = s.rebalance();
        let events: Vec<&SchedEvent> = s
            .events()
            .iter()
            .filter(|e| matches!(e, SchedEvent::Rebalanced { .. }))
            .collect();
        let costs_declared = events.iter().all(|e| match e {
            SchedEvent::Rebalanced { cost_ticks, to, .. } => {
                *cost_ticks == 3 && *to == QueueKind::Copy
            }
            _ => false,
        });
        // 均值 = (8+0+0)/3 = 2，超载判定 8 > 4；迁移收敛到均值 → 迁 6 条。
        let ok = moved == 6 && events.len() == 6 && costs_declared;
        set.add("A06-重平衡-迁移成本声明", ok, "");
    }

    // 判据：图形任务永不迁移（核亲缘 Q4：重平衡不迁出图形队列）。
    {
        let mut s = QueueScheduler::new();
        for i in 0..10u64 {
            let _ = s.enqueue(g_task(200 + i, TaskPriority::Background));
        }
        let moved = s.rebalance();
        let ok = moved == 0 && s.queue(QueueKind::Graphics).pending.len() == 10;
        set.add("A06-重平衡-图形队列不迁出", ok, "");
    }

    // ---- 核亲缘 ----

    // 判据：核亲缘声明（文档在册 + 绑定生效 + 任务跑在绑定核上）。
    {
        let d = CORE_AFFINITY_DOC;
        let doc_ok = d.contains("Q1") && d.contains("0 号执行核") && d.contains("永不迁移");
        let mut s = QueueScheduler::new();
        let _ = s.enqueue(g_task(1, TaskPriority::Interactive));
        let _ = s.dispatch();
        let bound = s.queue(QueueKind::Graphics).core == 0
            && s.running()[0].core == QueueKind::Graphics.bound_core();
        // 计算任务不会跑上图形核（亲缘隔离：图形核忙时计算任务仍走自己的核）。
        let _ = s.enqueue(CmdTask::new(2, QueueKind::Compute, TaskPriority::Interactive, 4096));
        let _ = s.dispatch();
        let isolated = s.running().iter().any(|r| r.task.id == 2 && r.core == 1);
        set.add("A06-亲缘-声明与绑定生效", doc_ok && bound && isolated, "");
    }

    // ---- 队列满与错误账本 ----

    // 判据：队列满拒绝（零静默：不静默丢任务）+ 错误五元组入账。
    {
        let mut s = QueueScheduler::new();
        s.pause(QueueKind::Graphics); // 暂停防止 advance 清不掉 pending——不影响入队
        let mut last = Ok(());
        for i in 0..(MAX_QUEUE_DEPTH as u64 + 1) {
            last = s.enqueue(g_task(i, TaskPriority::Background));
        }
        let rejected = matches!(&last, Err(e) if e.code == "E_QUEUE_FULL")
            && s.errors().iter().any(|e| e.code == "E_QUEUE_FULL")
            && !s.errors()[0].next.is_empty();
        set.add("A06-错误-队列满拒绝入账", rejected, "");
    }

    // 判据：完成任务零静默（未知任务完成 → E_UNKNOWN_TASK）。
    {
        let mut s = QueueScheduler::new();
        let r = s.complete(999);
        let ok = matches!(r, Err(e) if e.code == "E_UNKNOWN_TASK")
            && s.errors().iter().any(|e| e.code == "E_UNKNOWN_TASK");
        set.add("A06-错误-未知任务完成", ok, "");
    }

    // ---- 派发与完成闭环 ----

    {
        let mut s = QueueScheduler::new();
        let _ = s.enqueue(g_task(1, TaskPriority::Interactive));
        let _ = s.enqueue(CmdTask::new(2, QueueKind::Copy, TaskPriority::Interactive, 4096));
        let d1 = s.dispatch();
        let c1 = s.complete(1);
        let d2 = s.dispatch(); // 图形队列已空、拷贝槽仍忙 → 无新派发
        let ok = d1 == vec![1u64, 2u64]
            && matches!(c1, Ok(t) if t.id == 1)
            && d2 == vec![]
            && s.running().len() == 1;
        set.add("A06-闭环-派发完成释放", ok, "");
    }

    // ---- 读屏可达 ----

    {
        let mut s = QueueScheduler::new();
        let _ = s.enqueue(g_task(1, TaskPriority::Interactive));
        let t = s.screen_text();
        set.add(
            "A06-读屏-队列状态可播",
            t.contains("图形队列") && t.contains("计算队列") && t.contains("拷贝队列")
                && t.contains("待派 1"),
            "",
        );
    }

    // ---- 事件读屏 ----

    {
        let e = SchedEvent::SpinAlarm {
            kind: QueueKind::Graphics,
            pending: 2,
            idle_ticks: 9,
            tick: 10,
        };
        set.add(
            "A06-读屏-事件文本可播",
            e.screen_text().contains("空转告警") && e.screen_text().contains("图形队列"),
            "",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea06_all_judgements_green() {
        let set = run_vea06_checks();
        let (passed, failed) = set.tally();
        let (reds, n) = set.red_items();
        let names: Vec<&'static str> = (0..n)
            .filter_map(|i| reds[i].as_ref().filter(|c| !c.passed).map(|c| c.name))
            .collect();
        assert!(
            set.all_passed(),
            "VE-F0006 自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            names
        );
    }

    #[test]
    fn vea06_priority_ordering_total() {
        // 优先级全序：Realtime > Interactive > Background。
        assert!(TaskPriority::Realtime > TaskPriority::Interactive);
        assert!(TaskPriority::Interactive > TaskPriority::Background);
    }
}
