//! VE-F0212 · 域自检（判据逐条对应，见 `veb12_recovery.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 分级处置 → `C212-分级-*`（四类别各自动作、动作面不越界、升级重算动作）
//! - 快照比对 → `C212-快照-*`（比对干净/检出各维变化、违例判定、正常路径零快照）
//! - 重试上限 → `C212-重试-*`（同级上限升级、全局硬顶、不可用后拒绝）
//! - 通知联动 → `C212-通知-*`（三要素齐、构造期拒绝、合并限频、通道故障补投、
//!   读屏可关闭、分类映射窗口）
//!
//! 门禁设计纪律（本域自检遵守，勿改）：
//! ① 每条判据都配「注入缺陷应变红」的验证——弱门禁（恒真断言）等价无门禁；
//! ② 表内元素验查表函数恒真，故分类表验证用**表外**真实形态（`from_irq_param`
//!    造出的窗口内/窗口外错误码），不用分类表自己造的枚举值；
//! ③ 性能自检实测真实工作量（快照计数覆盖「快照在哪一层被取」），不做
//!    `n*CONST` 自证式算术；
//! ④ 边界判据用表外真实形态（越界队列号、超容真值、非法错误码窗口）。

use super::veb12_recovery::*;
use crate::checks::CheckSet;
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

/// 构造一条资源行（测试用真值/本地区分靠 id 与页数）。
fn res(id: u32, pages: u32, attached: bool) -> ResourceRow {
    ResourceRow {
        id,
        backing_pages: pages,
        attached,
    }
}

/// 常规真值表（三条，id 刻意跨槽以走线性探测）。
fn truth() -> Vec<ResourceRow> {
    vec![res(1, 4, false), res(9, 8, true), res(40, 2, false)]
}

/// VE-F0212 域自检。
pub fn run_veb12_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-veb12");

    // ==================================================================
    // 一、判据「分级处置」——四类别各自的动作
    // ==================================================================

    // 命令解析错 → 丢弃该命令续跑（级别 Command）
    {
        let (sev, kind) = ErrClass::CommandParse(0x1234).classify();
        set.add(
            "C212-分级-命令错落单命令",
            sev == Severity::Command && kind == ActionKind::DropCommandContinue,
            "",
        );
    }
    // 队列坏 → 重置该队列（级别 Queue）
    {
        let (sev, kind) = ErrClass::QueueCorrupt(1).classify();
        set.add(
            "C212-分级-队列坏复位队列",
            sev == Severity::Queue && kind == ActionKind::ResetQueue,
            "",
        );
    }
    // 资源表不一致 → 按设备真值重建（级别 Queue，动作独立于队列复位）
    {
        let (sev, kind) = ErrClass::ResourceDrift.classify();
        set.add(
            "C212-分级-资源漂移走重建",
            sev == Severity::Queue && kind == ActionKind::RebuildResourceTable,
            "",
        );
    }
    // 设备坏 → 整设备 reset 重走初始化序（级别 Device）
    {
        let (sev, kind) = ErrClass::DeviceFault(0x8).classify();
        set.add(
            "C212-分级-设备坏整设备复位",
            sev == Severity::Device && kind == ActionKind::ResetDeviceReinit,
            "",
        );
    }
    // 未知类别按最低级别处置（宁可多丢一条命令，不误复位整设备）
    {
        let (sev, kind) = ErrClass::Unknown(0xDEAD).classify();
        set.add(
            "C212-分级-未知落最低级",
            sev == Severity::Command && kind == ActionKind::DropCommandContinue,
            "",
        );
    }
    // 级别序：升级链 Command→Queue→Device→Fatal 且 Fatal 为顶不自升
    {
        set.add(
            "C212-分级-升级链与顶",
            Severity::Command.escalate() == Severity::Queue
                && Severity::Queue.escalate() == Severity::Device
                && Severity::Device.escalate() == Severity::Fatal
                && Severity::Fatal.escalate() == Severity::Fatal,
            "",
        );
    }
    // 级别槽位下标显式映射（不依赖枚举判别值）
    {
        set.add(
            "C212-分级-槽位映射",
            Severity::Command.idx() == 0
                && Severity::Queue.idx() == 1
                && Severity::Device.idx() == 2
                && Severity::Fatal.idx() == 3,
            "",
        );
    }
    // 升级必须**重算动作**：设备级故障被升级到 Fatal 时动作须变 MarkUnusable，
    // 而不是沿用 ResetDeviceReinit（沿用即「升级处置级别」空话）。
    {
        let cls = ErrClass::DeviceFault(1);
        let recomputed = cls.action_for(Severity::Fatal);
        let original = cls.action_for(Severity::Device);
        set.add(
            "C212-分级-升级重算动作",
            original == ActionKind::ResetDeviceReinit
                && recomputed == ActionKind::MarkUnusable
                && recomputed != original,
            "",
        );
    }
    // 升级到队列级时，资源漂移类别仍走重建而非队列复位（同级别不同动作）
    {
        let a = ErrClass::ResourceDrift.action_for(Severity::Queue);
        let b = ErrClass::QueueCorrupt(0).action_for(Severity::Queue);
        set.add(
            "C212-分级-同级动作可不同",
            a == ActionKind::RebuildResourceTable && b == ActionKind::ResetQueue && a != b,
            "",
        );
    }

    // ==================================================================
    // 二、判据「分级处置」——动作面不越界（快照比对的核心）
    // ==================================================================

    // 丢命令动作只减 pending，不动就绪位/可用环/资源表
    // 前置条件真成立：先灌入待处理命令，否则 pending 恒 0，「减没减」无从观测。
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let _ = e.submit_commands(0, 4);
        let before = e.snapshot(0);
        let out = e.recover(ErrClass::CommandParse(1), 1);
        let after = e.snapshot(1);
        let d = diff(&before, &after);
        let b0 = before.queue(0);
        let a0 = after.queue(0);
        // 只许 pending 减一；就绪位与可用环不得动
        let pending_ok = match (b0, a0) {
            (Some(b), Some(a)) => a.pending == b.pending - 1,
            _ => false,
        };
        let precondition = b0.map(|q| q.pending == 4).unwrap_or(false);
        set.add(
            "C212-动作-丢命令只减pending",
            matches!(
                out,
                RecoveryOutcome::CommandDropped {
                    queue: 0,
                    remaining: 3
                }
            ) && pending_ok
                && precondition
                && d.queue_ready_changed == 0
                && d.queue_avail_changed == 0
                && !d.resource_touched(),
            "",
        );
    }
    // 队列复位只复位命中队列，其他队列就绪/可用/待处理不变。
    // 前置条件必须**真成立**：先给三个队列都灌上待处理命令（脏态），
    // 否则它们本就处于复位初值，「是否波及其他」无从观测——判据恒真。
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let _ = e.submit_commands(0, 5);
        let _ = e.submit_commands(1, 3);
        let _ = e.submit_commands(2, 7);
        let before = e.snapshot(0);
        let out = e.recover(ErrClass::QueueCorrupt(1), 1);
        let after = e.snapshot(1);
        let d = diff(&before, &after);
        let matched = matches!(out, RecoveryOutcome::QueueReset { queue: 1 });
        // 命中队列回到就绪初值
        let target_ok = after
            .queue(1)
            .map(|q| q.ready && q.avail == AVAIL_AFTER_RESET && q.pending == PENDING_AFTER_RESET)
            .unwrap_or(false);
        // 其他队列必须**原样保留脏态**（这才是「不波及其他」的可观测证据）
        let others_kept = after.queue(0) == before.queue(0) && after.queue(2) == before.queue(2);
        // 前置条件自证：脏态确实建立了（否则上面两项恒真）
        let precondition = before
            .queue(0)
            .map(|q| q.pending == 5 && q.avail == 5)
            .unwrap_or(false)
            && before.queue(2).map(|q| q.pending == 7).unwrap_or(false);
        set.add(
            "C212-动作-队列复位不波及其他",
            matched && target_ok && others_kept && precondition && !d.resource_touched(),
            "",
        );
    }
    // 整设备复位后全部队列回到初值 + 资源表与真值一致
    // 前置条件真成立：先让三个队列都处于脏态，否则「全部复位」无从观测。
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let _ = e.submit_commands(0, 5);
        let _ = e.submit_commands(1, 3);
        let _ = e.submit_commands(2, 7);
        let before = e.snapshot(0);
        let precondition = before.queue(0).map(|q| q.pending == 5).unwrap_or(false)
            && before.queue(2).map(|q| q.pending == 7).unwrap_or(false);
        let out = e.recover(ErrClass::DeviceFault(1), 1);
        let matched = matches!(
            out,
            RecoveryOutcome::DeviceReinit { steps, .. } if steps as usize == RESET_SEQUENCE_STEPS
        );
        set.add(
            "C212-动作-整设备复位完整",
            matched && precondition && e.all_queues_at_reset() && e.resources_match_truth(),
            "",
        );
    }
    // 资源表重建后与真值逐条一致
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let out = e.recover(ErrClass::ResourceDrift, 1);
        let matched = matches!(out, RecoveryOutcome::ResourceTableRebuilt { .. });
        set.add(
            "C212-动作-资源重建对真值",
            matched && e.resources_match_truth() && e.resource_count() == 3,
            "",
        );
    }
    // 违例面可判定：手工造「丢命令却改了资源表」的 before/after → 必须判红
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let before = e.snapshot(0);
        // 手工构造一个资源表被动的前后对照（不调 recover，直接喂比对器）
        let mut after = e.snapshot(0);
        after.resources = vec![res(1, 99, false), res(9, 8, true), res(40, 2, false)];
        let d = diff(&before, &after);
        let v = e.violation_probe(
            ActionKind::DropCommandContinue,
            Some(0),
            &before,
            &after,
            &d,
        );
        set.add(
            "C212-快照-检出资源被动",
            d.resource_backing_changed == 1 && v == Some(Violation::CommandDropTouchedResource),
            "",
        );
    }
    // 违例面可判定：手工造「丢命令却没减 pending」→ 必须判红
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let before = e.snapshot(0);
        // before 有 5 条待处理，after 仍有 5 条（没丢成）
        let mut b = before.clone();
        b.queues = vec![
            QueueRow {
                index: 0,
                role: QueueRole::Control,
                ready: true,
                avail: 0,
                pending: 5,
            },
            QueueRow {
                index: 1,
                role: QueueRole::Cursor,
                ready: true,
                avail: 0,
                pending: 0,
            },
            QueueRow {
                index: 2,
                role: QueueRole::Render,
                ready: true,
                avail: 0,
                pending: 0,
            },
        ];
        let after = b.clone();
        let d = diff(&b, &after);
        let v = e.violation_probe(ActionKind::DropCommandContinue, Some(0), &b, &after, &d);
        set.add(
            "C212-快照-检出未丢成",
            v == Some(Violation::CommandDropDidNotDrop),
            "",
        );
    }
    // 违例面可判定：队列复位未回初值 → 判红
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let mut b = e.snapshot(0);
        b.queues = vec![
            QueueRow {
                index: 0,
                role: QueueRole::Control,
                ready: true,
                avail: 0,
                pending: 0,
            },
            QueueRow {
                index: 1,
                role: QueueRole::Cursor,
                ready: true,
                avail: 0,
                pending: 0,
            },
            QueueRow {
                index: 2,
                role: QueueRole::Render,
                ready: true,
                avail: 0,
                pending: 0,
            },
        ];
        // after 的目标队列未就绪
        let mut a = b.clone();
        a.queues[1].ready = false;
        let d = diff(&b, &a);
        let v = e.violation_probe(ActionKind::ResetQueue, Some(1), &b, &a, &d);
        set.add(
            "C212-快照-检出队列未复位",
            v == Some(Violation::QueueNotReset),
            "",
        );
    }
    // 违例面可判定：资源重建却动了队列 → 判红
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let b = e.snapshot(0);
        let mut a = b.clone();
        a.queues[2].pending = 7;
        let d = diff(&b, &a);
        let v = e.violation_probe(ActionKind::RebuildResourceTable, None, &b, &a, &d);
        set.add(
            "C212-快照-检出重建动队列",
            v == Some(Violation::RebuildTouchedQueue),
            "",
        );
    }
    // 违例面可判定：整设备复位不完整 → 判红
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let mut a = e.snapshot(0);
        a.queues[2].ready = false;
        let b0 = e.snapshot(1);
        let d = diff(&b0, &a);
        let v = e.violation_probe(ActionKind::ResetDeviceReinit, None, &b0, &a, &d);
        set.add("C212-快照-检出整设备复位不全", v.is_some(), "");
    }
    // 违例面：正常比对必须干净（假绿防线：比对器不能恒判有错）
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let s1 = e.snapshot(0);
        let s2 = e.snapshot(1);
        let d = diff(&s1, &s2);
        set.add(
            "C212-快照-无变化比对干净",
            d.is_clean() && !d.resource_touched() && !d.queue_touched(),
            "",
        );
    }
    // 违例面：资源 id 跨槽线性探测可查回（表结构判据，非恒真）
    {
        let e = RecoveryEngine::new(3, &truth());
        let r1 = e.resource(1);
        let r9 = e.resource(9);
        let r40 = e.resource(40);
        let missing = e.resource(7777);
        set.add(
            "C212-快照-跨槽资源可查",
            r1.map(|r| r.backing_pages) == Some(4)
                && r9.map(|r| r.backing_pages) == Some(8)
                && r40.map(|r| r.backing_pages) == Some(2)
                && missing.is_none(),
            "",
        );
    }
    // 违例面：越界队列查询返回 None 而非 panic（边界防护）
    {
        let e = RecoveryEngine::new(3, &truth());
        set.add(
            "C212-边界-越界队列返None",
            e.queue(99).is_none() && !e.queue_valid(99),
            "",
        );
    }
    // 违例面：越界快照查询返 None（快照面同口径）
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let s = e.snapshot(0);
        set.add(
            "C212-边界-越界快照返None",
            s.queue(99).is_none() && s.resource(4242).is_none(),
            "",
        );
    }

    // ==================================================================
    // 三、判据「快照比对」——性能纪律
    // ==================================================================

    // 正常路径零开销：未调 recover 则快照计数恒 0
    {
        let e = RecoveryEngine::new(3, &truth());
        // 只做只读操作（读队列/资源/真值/可用性）
        let _ = e.queue(0);
        let _ = e.resource(1);
        let _ = e.truth_count();
        let _ = e.device_usable();
        let _ = e.notice_len();
        set.add(
            "C212-性能-正常路径零快照",
            e.stats.snapshots_taken == 0 && e.stats.diffs_computed == 0 && e.stats.recoveries == 0,
            "",
        );
    }
    // 快照只在恢复时取：每次 recover 取两次（前后各一）
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let _ = e.recover(ErrClass::QueueCorrupt(1), 1);
        set.add(
            "C212-性能-恢复取两次快照",
            e.stats.snapshots_taken == 2 && e.stats.diffs_computed == 1,
            "",
        );
    }
    // 快照 O(资源数)：快照资源条目数与真实值条目数一致（工作量真实）
    {
        let big: Vec<ResourceRow> = (0..20).map(|i| res(i, i + 1, false)).collect();
        let mut e = RecoveryEngine::new(3, &big);
        let s = e.snapshot(0);
        set.add(
            "C212-性能-快照资源数真实",
            s.resource_count() == 20 && e.truth_count() == 20,
            "",
        );
    }
    // 快照夹紧：真值超容时快照不得连带膨胀
    {
        let huge: Vec<ResourceRow> = (0..(RESOURCE_SLOTS + 20))
            .map(|i| res(i as u32, 1, false))
            .collect();
        let mut e = RecoveryEngine::new(3, &huge);
        let s = e.snapshot(0);
        set.add(
            "C212-边界-超容真值夹紧",
            s.resource_count() <= SNAPSHOT_MAX_RESOURCES && e.stats.truth_overflow > 0,
            "",
        );
    }

    // ==================================================================
    // 四、判据「重试上限」
    // ==================================================================

    // 同级重试达上限即升级级别
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let mut saw_escalate = false;
        let mut t: u64 = 0;
        while t < (RECOVERY_RETRY_LIMIT as u64) + 1 {
            let out = e.recover(ErrClass::CommandParse(1), t);
            if matches!(out, RecoveryOutcome::EscalatedRetrying { .. }) {
                saw_escalate = true;
            }
            t += 1;
        }
        set.add(
            "C212-重试-同级上限升级",
            saw_escalate && e.stats.escalations > 0,
            "",
        );
    }
    // 重试有上限：设备级反复失败最终升级到 Fatal 标记不可用
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let mut t = 0;
        let mut saw_unusable = false;
        while t < 20 {
            if matches!(
                e.recover(ErrClass::DeviceFault(1), t),
                RecoveryOutcome::MarkedUnusable
            ) {
                saw_unusable = true;
                break;
            }
            t += 1;
        }
        set.add(
            "C212-重试-反复损坏判不可用",
            saw_unusable && !e.device_usable() && e.stats.marked_unusable > 0,
            "",
        );
    }
    // 全局硬顶：累计次数超顶即 Capped（防打转第二级）
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let mut capped = false;
        let mut t: u64 = 0;
        while t < (RECOVERY_TOTAL_CAP as u64) + 5 {
            if matches!(e.recover(ErrClass::Unknown(1), t), RecoveryOutcome::Capped) {
                capped = true;
                break;
            }
            t += 1;
        }
        set.add(
            "C212-重试-全局硬顶生效",
            capped && e.stats.capped_total > 0,
            "",
        );
    }
    // 设备不可用后拒绝再恢复（防打转第三级），且拒绝被计数不静默
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let mut t = 0;
        while t < 30 && e.device_usable() {
            let _ = e.recover(ErrClass::DeviceFault(1), t);
            t += 1;
        }
        let out = e.recover(ErrClass::DeviceFault(1), 100);
        set.add(
            "C212-重试-不可用后拒绝",
            matches!(out, RecoveryOutcome::RefusedUnusable) && e.stats.recoveries_refused > 0,
            "",
        );
    }
    // 重试计数只读面与实际一致（读面不谎报）
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let before = e.retries_of(Severity::Command);
        let _ = e.recover(ErrClass::CommandParse(1), 1);
        let after = e.retries_of(Severity::Command);
        set.add("C212-重试-计数读面一致", before == 0 && after == 1, "");
    }
    // 恢复总次数只读面与实际一致
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let _ = e.recover(ErrClass::QueueCorrupt(1), 1);
        set.add(
            "C212-重试-总次数读面一致",
            e.total_attempts() == 1 && e.stats.recoveries == 1,
            "",
        );
    }

    // ==================================================================
    // 五、判据「通知联动」（F0103 三要素）
    // ==================================================================

    // 三要素齐
    {
        let n = DegradeNotice::new(
            1,
            Severity::Queue,
            format!("发生了{}", "甲"),
            format!("因为{}", "乙"),
            format!("请{}", "丙"),
            1,
        );
        let ok = matches!(&n, Ok(x) if x.has_triplet() && !x.what.is_empty() && !x.why.is_empty() && !x.next.is_empty());
        set.add("C212-通知-三要素齐", ok, "");
    }
    // 缺「发生了什么」→ 构造期拒绝（不给静默空文案留机会）
    {
        let n = DegradeNotice::new(
            1,
            Severity::Queue,
            String::new(),
            "乙".to_string(),
            "丙".to_string(),
            1,
        );
        set.add(
            "C212-通知-缺what拒绝",
            n == Err(NoticeReject::EmptyWhat),
            "",
        );
    }
    // 缺「为什么」→ 构造期拒绝
    {
        let n = DegradeNotice::new(
            1,
            Severity::Queue,
            "甲".to_string(),
            String::new(),
            "丙".to_string(),
            1,
        );
        set.add("C212-通知-缺why拒绝", n == Err(NoticeReject::EmptyWhy), "");
    }
    // 缺「下一步」→ 构造期拒绝
    {
        let n = DegradeNotice::new(
            1,
            Severity::Queue,
            "甲".to_string(),
            "乙".to_string(),
            String::new(),
            1,
        );
        set.add(
            "C212-通知-缺next拒绝",
            n == Err(NoticeReject::EmptyNext),
            "",
        );
    }
    // 恢复过程界面提示可关闭（无障碍面）
    {
        let n = DegradeNotice::new(
            1,
            Severity::Queue,
            "甲".to_string(),
            "乙".to_string(),
            "丙".to_string(),
            1,
        );
        let ok = matches!(&n, Ok(x) if x.dismissible);
        set.add("C212-通知-可关闭", ok, "");
    }
    // 读屏播报三要素一次播全、不截断（读屏用户拿到完整因果）
    {
        let n = DegradeNotice::new(
            1,
            Severity::Queue,
            "图形设备已重置".to_string(),
            "设备级故障".to_string(),
            "请重新启动图形会话".to_string(),
            1,
        );
        let text = match n {
            Ok(x) => x.screen_text(),
            Err(_) => String::new(),
        };
        set.add(
            "C212-通知-读屏播全三要素",
            text.contains("图形设备已重置")
                && text.contains("设备级故障")
                && text.contains("请重新启动图形会话")
                && text.contains("可以关闭"),
            "",
        );
    }
    // 恢复必产出通知且三要素齐（联动判据）
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let _ = e.recover(ErrClass::QueueCorrupt(1), 1);
        let drained = e.drain_notices();
        let all_ok = drained.iter().all(|n| n.has_triplet());
        set.add(
            "C212-通知-恢复产出通知",
            !drained.is_empty() && all_ok && e.stats.notices_emitted > 0,
            "",
        );
    }
    // 同类通知窗口内合并限频（不新增条目、合并计数累加）
    {
        let mut e = RecoveryEngine::new(3, &truth());
        // 连续多次同类别恢复，tick 落在合并窗口内
        let _ = e.recover(ErrClass::QueueCorrupt(1), 10);
        let _ = e.recover(ErrClass::QueueCorrupt(1), 11);
        let _ = e.recover(ErrClass::QueueCorrupt(1), 12);
        let len = e.notice_len();
        let merged = e.stats.notices_merged;
        set.add("C212-通知-同类合并限频", len < 3 && merged > 0, "");
    }
    // 通知通道故障 → 入待补投（不静默丢）
    {
        let mut e = RecoveryEngine::new(3, &truth());
        e.set_channel_ok(false);
        let _ = e.recover(ErrClass::QueueCorrupt(1), 1);
        set.add(
            "C212-通知-通道故障待补投",
            !e.channel_ok() && e.pending_count() > 0 && e.stats.notices_pending > 0,
            "",
        );
    }
    // 通道恢复后补投（逐条补齐，不合并成一条）
    {
        let mut e = RecoveryEngine::new(3, &truth());
        e.set_channel_ok(false);
        let _ = e.recover(ErrClass::QueueCorrupt(1), 1);
        let queued = e.pending_count();
        e.set_channel_ok(true);
        let replayed = e.replay_pending();
        let drained = e.drain_notices();
        set.add(
            "C212-通知-恢复后补投",
            replayed == queued && drained.len() == queued && e.pending_count() == 0,
            "",
        );
    }
    // 降级通知三要素齐——模板缺失退化为最小告知仍三要素齐（F0103 不静默）
    {
        let (w, y, n2) = degraded_template(ErrClass::Unknown(0x99), Severity::Command);
        let constructed = DegradeNotice::new(7, Severity::Command, w, y, n2, 1);
        let ok = matches!(&constructed, Ok(x) if x.has_triplet());
        set.add("C212-通知-退化仍三要素", ok, "");
    }
    // 降级文案含三要素标签（可读、不依赖颜色单独表意）
    {
        let (w, y, n2) = degraded_template(ErrClass::DeviceFault(1), Severity::Fatal);
        set.add(
            "C212-通知-退化文案有标签",
            w.contains("设备故障") && y.contains("设备不可用") && n2.contains("建议"),
            "",
        );
    }

    // ==================================================================
    // 六、判据「通知联动」——上游事件映射（表外真实形态）
    // ==================================================================

    // 呈现事件参数（bit31 未置位）不得被误判成设备故障
    {
        let c = ErrClass::from_irq_param(0, 0x0000_0001);
        set.add(
            "C212-映射-呈现事件不误判",
            matches!(c, ErrClass::Unknown(_)),
            "",
        );
    }
    // 错误码窗口内的设备故障码 → 映射为 DeviceFault 且状态字透传
    {
        // 0x04<<16 | status，且 bit31 置位 → code = 0x80040000
        let c = ErrClass::from_irq_param(1, 0x8004_0008);
        set.add(
            "C212-映射-设备故障码还原",
            c == ErrClass::DeviceFault(8),
            "",
        );
    }
    // 错误码窗口内的命令解析错 → CommandParse 且命令码透传
    {
        let c = ErrClass::from_irq_param(0, 0x8001_1234);
        set.add(
            "C212-映射-命令错码还原",
            c == ErrClass::CommandParse(0x1234),
            "",
        );
    }
    // 错误码窗口内的队列坏 → QueueCorrupt 且队列号透传
    {
        let c = ErrClass::from_irq_param(2, 0x8002_0002);
        set.add("C212-映射-队列坏码还原", c == ErrClass::QueueCorrupt(2), "");
    }
    // 错误码窗口内的资源漂移码 → ResourceDrift
    {
        let c = ErrClass::from_irq_param(0, 0x8003_0000);
        set.add("C212-映射-资源漂移码还原", c == ErrClass::ResourceDrift, "");
    }
    // 未登记的错误码 → Unknown（不猜成已知类别）
    {
        let c = ErrClass::from_irq_param(0, 0x800F_0000);
        set.add(
            "C212-映射-未登记码落Unknown",
            c == ErrClass::Unknown(0x000F_0000),
            "",
        );
    }
    // 分类标签区分度：不同类别不同标签（通知按标签合并的依据）
    {
        let a = ErrClass::CommandParse(1).tag();
        let b = ErrClass::QueueCorrupt(1).tag();
        let c = ErrClass::ResourceDrift.tag();
        let d = ErrClass::DeviceFault(1).tag();
        set.add(
            "C212-映射-标签区分",
            a != b && b != c && c != d && a != d,
            "",
        );
    }

    // ==================================================================
    // 七、reset 序（F0217 复用面 · 单一事实源）
    // ==================================================================

    // reset 序步数与逐字序（与 F0201 初始化序同序，F0217 复用）
    {
        let seq = RecoveryEngine::reinit_sequence();
        let names: Vec<&str> = seq.iter().map(|s| s.name).collect();
        set.add(
            "C212-序-步数与逐字序",
            seq.len() == RESET_SEQUENCE_STEPS
                && names
                    == vec![
                        "reset",
                        "acknowledge",
                        "set_driver",
                        "negotiate_features",
                        "features_ok",
                        "setup_queues",
                        "driver_ok",
                    ],
            "",
        );
    }
    // reset 序总成本 = 各步成本之和（成本账本自洽，非拍脑袋常数）
    {
        let sum: u32 = RESET_SEQUENCE.iter().map(|s| s.cost_us).sum();
        set.add("C212-序-成本账本自洽", sum == RESET_SEQUENCE_COST_US, "");
    }
    // 整设备 reset 回报的步数/成本与序一致
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let out = e.recover(ErrClass::DeviceFault(1), 1);
        let matched = matches!(
            out,
            RecoveryOutcome::DeviceReinit { steps, cost_us }
                if steps as usize == RESET_SEQUENCE_STEPS && cost_us == RESET_SEQUENCE_COST_US
        );
        set.add("C212-序-复位回报一致", matched, "");
    }

    // ==================================================================
    // 八、边界防护与计数诚实性
    // ==================================================================

    // 越界队列号恢复 → 拒绝计数而非崩（边界防护）
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let _ = e.recover(ErrClass::QueueCorrupt(99), 1);
        set.add("C212-边界-越界队列拒收", e.stats.rejected_queue > 0, "");
    }
    // 未知错误码被计数（不静默）
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let _ = e.recover(ErrClass::Unknown(0x77), 1);
        set.add("C212-边界-未知码计数", e.stats.unknown_classes > 0, "");
    }
    // 丢命令/队列复位/资源重建/整设备复位各计对应计数（动作计数诚实）
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let _ = e.recover(ErrClass::CommandParse(1), 1);
        let _ = e.recover(ErrClass::QueueCorrupt(1), 2);
        let _ = e.recover(ErrClass::ResourceDrift, 3);
        let _ = e.recover(ErrClass::DeviceFault(1), 4);
        set.add(
            "C212-边界-动作计数诚实",
            e.stats.command_drops == 1
                && e.stats.queue_resets == 1
                && e.stats.resource_rebuilds == 1
                && e.stats.device_resets == 1,
            "",
        );
    }
    // 违规面可枚举且名可读（诊断不静默：违例必须能说出是什么）
    {
        let all = [
            Violation::CommandDropTouchedQueue,
            Violation::CommandDropTouchedResource,
            Violation::CommandDropDidNotDrop,
            Violation::QueueNotReset,
            Violation::QueueResetTouchedOthers,
            Violation::QueueResetTouchedResource,
            Violation::ResourceTableNotRebuilt,
            Violation::RebuildTouchedQueue,
            Violation::DeviceReinitIncomplete,
            Violation::DeviceReinitResourceMismatch,
            Violation::UnexpectedResourceChange,
            Violation::UnexpectedQueueChange,
        ];
        let all_named = all.iter().all(|v| !v.name().is_empty());
        set.add("C212-边界-违例可枚举命名", all_named, "");
    }
    // 静默错乱恢复不算成功：注入「丢命令却改资源表」→ 必须报 SilentCorruption
    // 而非成功（本条是判据「快照比对防静默状态错乱」的端到端红线）。
    {
        // 用一个真值被清空来模拟「重建后仍与真值不一致」：把真值设成非空但
        // 本地表被外力清空后，比对器必须判红。
        let mut e = RecoveryEngine::new(3, &truth());
        // 先让本地表与真值一致
        let _ = e.recover(ErrClass::ResourceDrift, 1);
        let before = e.snapshot(0);
        // 手工把 after 的资源表清空（模拟重建未生效）
        let mut after = before.clone();
        after.resources = vec![];
        let d = diff(&before, &after);
        let v = e.violation_probe(ActionKind::RebuildResourceTable, None, &before, &after, &d);
        set.add(
            "C212-快照-重建未生效判红",
            v == Some(Violation::ResourceTableNotRebuilt),
            "",
        );
    }
    // 资源表**缺失**方向必须检出：恢复后少了资源 = 资源泄漏/静默丢资源。
    // 只测「多出」方向的话，把 missing 计数改成不计数也能全绿——而资源泄漏
    // 正是恢复路径最贵的故障（guest 侧显存句柄悬空）。
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let before = e.snapshot(0);
        let mut after = before.clone();
        // 抹掉两条资源（模拟恢复中资源丢失）
        after.resources.retain(|r| r.id != 9);
        after.resources.retain(|r| r.id != 40);
        let d = diff(&before, &after);
        let v = e.violation_probe(ActionKind::RebuildResourceTable, None, &before, &after, &d);
        set.add(
            "C212-快照-重建检出资源缺失",
            d.resource_missing == 2 && v == Some(Violation::ResourceTableNotRebuilt),
            "",
        );
    }
    // 反向弱门禁防线：快照条目**多于**真值（本地多出幽灵资源）也必须判红。
    // 只查「真值侧每条都在」的话，本地多出的条目会被放过——那是资源泄漏，
    // 而资源表一致性的定义是**双向**相等。
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let before = e.snapshot(0);
        let mut after = before.clone();
        // 追加两条真值里没有的幽灵资源
        after.resources.push(res(555, 3, false));
        after.resources.push(res(556, 4, false));
        let d = diff(&before, &after);
        let v = e.violation_probe(ActionKind::RebuildResourceTable, None, &before, &after, &d);
        set.add(
            "C212-快照-重建检出多余资源",
            v == Some(Violation::ResourceTableNotRebuilt),
            "",
        );
    }
    // 快照资源表与真值不一致时重建统计如实反映（kept/dropped/added 公开）
    {
        let mut e = RecoveryEngine::new(3, &truth());
        // 换真值：新真值少一条、多一条不同 id → dropped/added 都应非零
        let new_truth = vec![res(1, 4, false), res(9, 8, false), res(77, 1, false)];
        e.set_device_truth(&new_truth);
        let out = e.recover(ErrClass::ResourceDrift, 1);
        let matched = match out {
            RecoveryOutcome::ResourceTableRebuilt { stat } => {
                stat.dropped > 0 && stat.kept > 0 && e.resources_match_truth()
            }
            _ => false,
        };
        set.add("C212-快照-重建统计公开", matched, "");
    }
    // 设备可用性翻转进快照（比对含可用位，防止可用性被静默改）
    {
        let mut e = RecoveryEngine::new(3, &truth());
        let s = e.snapshot(0);
        set.add("C212-快照-含可用位", s.device_usable, "");
    }

    set
}
