//! VE-F0009 · 域自检（判据逐条对应，见 `vea09_recovery.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 三段恢复 → `A09-三段-*`
//! - 原因分类（含驱动版本绑定）→ `A09-原因-*`
//! - 软渲兜底 → `A09-兜底-*`
//! - 诊断包 → `A09-诊断-*`
//! - 重复丢失熔断 → `A09-熔断-*`
//! - 诚实进度 / 读屏播报 → `A09-进度-*`、`A09-读屏-*`
//! - 丢失演练语料（三类场景注入）→ `A09-演练-*`
//! - 错误路径（非法迁移/进度超额/未完成宣告/降级无因）→ `A09-错误-*`
//!
//! 逻辑 tick 注入、无墙钟，回归可复现。

use super::vea09_recovery::*;
use crate::checks::CheckSet;

use alloc::string::ToString;

/// VE-F0009 域自检。
pub fn run_vea09_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vea09");

    // ---- 判据一：三段恢复 ----

    // 三段路径实测：检测 → 重建（逐资源推进）→ 恢复 → 回 Normal。
    {
        let mut m = RecoveryMachine::new();
        m.tick();
        let ok = m
            .on_loss(LossEvent::new(LossReason::DriverReset, "drv-551.23", 1).expect("事件合法"), 3)
            .is_ok()
            && m.state() == RecoveryState::LossDetected
            && m.begin_rebuild().is_ok()
            && m.state() == RecoveryState::Rebuilding
            && (0..3).all(|_| m.rebuild_one().is_ok())
            && m.finish_recovery().is_ok()
            && m.state() == RecoveryState::Normal;
        set.add("A09-三段-检测重建恢复", ok, "");
    }

    // 诚实进度：进度单调不减且未收满不得宣告恢复。
    {
        let mut m = RecoveryMachine::new();
        let _ = m.on_loss(LossEvent::new(LossReason::SleepWake, "drv-1.0", 1).expect("事件合法"), 4);
        let _ = m.begin_rebuild();
        let _ = m.rebuild_one();
        let _ = m.rebuild_one();
        let premature = m.finish_recovery().is_err(); // 2/4 宣告恢复被拒
        let _ = m.rebuild_one();
        let _ = m.rebuild_one();
        let complete = m.finish_recovery().is_ok() && m.state() == RecoveryState::Normal;
        set.add("A09-进度-未满不得宣告", premature && complete, "");
    }

    // 进度超额推进被拒（记账 bug 的防御闸）。
    {
        let mut m = RecoveryMachine::new();
        let _ = m.on_loss(LossEvent::new(LossReason::HotUnplug, "drv-1.0", 1).expect("事件合法"), 2);
        let _ = m.begin_rebuild();
        let _ = m.rebuild_one();
        let _ = m.rebuild_one();
        let ok = matches!(m.rebuild_one(), Err(RecoveryError::ProgressOverflow { .. }));
        set.add("A09-进度-超额推进拒绝", ok, "");
    }

    // ---- 判据二：原因分类（含驱动版本绑定）----

    // 丢失记录绑定驱动版本（账目里可按版本统计）。
    {
        let mut m = RecoveryMachine::new();
        let _ = m.on_loss(LossEvent::new(LossReason::DriverReset, "drv-551.23", 1).expect("事件合法"), 1);
        let ok = m.loss_log.len() == 1 && m.loss_log[0].driver_version == "drv-551.23";
        set.add("A09-原因-版本绑定在册", ok, "");
    }

    // 无驱动版本的丢失事件被拒（无版本绑定 = 无法归因 = 不收）。
    {
        let ok = matches!(
            LossEvent::new(LossReason::DriverReset, "", 1),
            Err(RecoveryError::MissingDriverVersion { .. })
        );
        set.add("A09-原因-缺版本拒绝", ok, "");
    }

    // 四类原因全可登记（三类点名 + 未知兜底）。
    {
        let ok = [
            LossReason::DriverReset,
            LossReason::SleepWake,
            LossReason::HotUnplug,
            LossReason::Unknown,
        ]
        .iter()
        .all(|r| LossEvent::new(*r, "drv-1.0", 1).is_ok());
        set.add("A09-原因-四类全收", ok, "");
    }

    // ---- 判据三：软渲兜底 ----

    // 重建失败 → 软渲兜底（带原因，用户告知性能预期）。
    {
        let mut m = RecoveryMachine::new();
        let _ = m.on_loss(LossEvent::new(LossReason::DriverReset, "drv-1.0", 1).expect("事件合法"), 2);
        let _ = m.begin_rebuild();
        let no_reason = m.fallback_to_software("").is_err();
        let _ = m.fallback_to_software("资源重创连续失败");
        let ok = no_reason
            && m.state() == RecoveryState::SoftwareFallback
            && m.user_notice
                .as_ref()
                .map(|n| n.contains("软件渲染") && n.contains("预期"))
                .unwrap_or(false);
        set.add("A09-兜底-软渲切换告知", ok, "");
    }

    // 软渲兜底后的再丢失可以走重建（降级不是终点）。
    {
        let mut m = RecoveryMachine::new();
        let _ = m.on_loss(LossEvent::new(LossReason::DriverReset, "drv-1.0", 1).expect("事件合法"), 1);
        let _ = m.begin_rebuild();
        let _ = m.fallback_to_software("重创失败");
        let _ = m.on_loss(LossEvent::new(LossReason::DriverReset, "drv-1.0", 5).expect("事件合法"), 1);
        let ok = m.state() == RecoveryState::LossDetected;
        set.add("A09-兜底-降级后可再恢复", ok, "");
    }

    // ---- 判据四：诊断包 ----

    // 未知原因丢失自动采集诊断包（现场最小集，不猜结论）。
    {
        let mut m = RecoveryMachine::new();
        let _ = m.on_loss(LossEvent::new(LossReason::Unknown, "drv-2.0", 3).expect("事件合法"), 2);
        let ok = m.diag_packs.len() == 1
            && m.diag_packs[0].event.reason == LossReason::Unknown
            && m.diag_packs[0].event.driver_version == "drv-2.0"
            && !m.diag_packs[0].snapshot.is_empty();
        set.add("A09-诊断-未知原因采集", ok, "");
    }

    // 已知原因不产诊断包（采集有边界，不是什么丢失都打包）。
    {
        let mut m = RecoveryMachine::new();
        let _ = m.on_loss(LossEvent::new(LossReason::SleepWake, "drv-2.0", 1).expect("事件合法"), 1);
        set.add("A09-诊断-已知原因不打包", m.diag_packs.is_empty(), "");
    }

    // ---- 判据五：重复丢失熔断 ----

    // 连续丢失三次 → 最小模式 + 用户告知（锚点点名行为）。
    {
        let mut m = RecoveryMachine::new();
        let ok = (0..3)
            .all(|i| {
                m.on_loss(LossEvent::new(LossReason::DriverReset, "drv-1.0", i + 1).expect("事件合法"), 2)
                    .is_ok()
            })
            && m.state() == RecoveryState::MinimalMode
            && m.user_notice
                .as_ref()
                .map(|n| n.contains("最小模式") && n.contains("3 次"))
                .unwrap_or(false);
        set.add("A09-熔断-三次转最小模式", ok, "");
    }

    // 熔断可复位：稳定期后计数清零，恢复路径重新可用。
    {
        let mut m = RecoveryMachine::new();
        let _ = m.on_loss(LossEvent::new(LossReason::DriverReset, "drv-1.0", 1).expect("事件合法"), 1);
        let _ = m.begin_rebuild();
        let _ = m.rebuild_one();
        let _ = m.finish_recovery();
        for _ in 0..(STABLE_TICKS_TO_CLEAR + 1) {
            m.tick();
        }
        let ok = m.on_loss(LossEvent::new(LossReason::DriverReset, "drv-1.0", 50).expect("事件合法"), 1).is_ok()
            && m.state() != RecoveryState::MinimalMode; // 计数已清零，单次丢失不熔断
        set.add("A09-熔断-稳定期复位", ok, "");
    }

    // ---- 丢失演练语料（五场景全绿）----

    {
        let ok = DRILL_SCENARIOS.iter().all(|s| run_drill(*s, "drv-drill-1.0").1);
        set.add("A09-演练-五场景全绿", ok, "");
    }

    // ---- 读屏播报（无障碍面）----

    {
        let p = RebuildProgress { done: 2, total: 5 };
        let msg = p.announce(RecoveryState::Rebuilding);
        let ok = msg.contains("重建") && msg.contains("2/5") && !msg.is_empty();
        set.add("A09-读屏-进度播报带数", ok, "");
    }

    // ---- 错误路径：非法迁移 ----

    {
        let mut m = RecoveryMachine::new();
        // Normal 直接宣告恢复 = 跳段，必须拒绝。
        let ok = m.finish_recovery().is_err()
            && matches!(
                m.finish_recovery(),
                Err(RecoveryError::IllegalTransition { .. })
            );
        set.add("A09-错误-跳段拒绝", ok, "");
    }

    // 审计账全量留痕（零静默：迁移路径可回放）。
    {
        let mut m = RecoveryMachine::new();
        let _ = m.on_loss(LossEvent::new(LossReason::DriverReset, "drv-1.0", 1).expect("事件合法"), 1);
        let _ = m.begin_rebuild();
        let _ = m.rebuild_one();
        let _ = m.finish_recovery();
        let ok = m.audit_trail().len() >= 4; // 丢失/重建/恢复/回Normal 各一条
        set.add("A09-审计-迁移全留痕", ok, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea09_checks_all_green() {
        let set = run_vea09_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0009 自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }

    /// 状态机在内核 Rust 侧可执行：三段恢复端到端 + 演练语料全绿。
    #[test]
    fn vea09_recovery_end_to_end() {
        let results: Vec<(DrillScenario, bool)> =
            DRILL_SCENARIOS.iter().map(|s| run_drill(*s, "drv-e2e-1.0")).collect();
        assert!(results.iter().all(|(_, ok)| *ok), "演练语料存在红项：{:?}", results);
    }
}
