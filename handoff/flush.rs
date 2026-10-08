//! 冲刷管线与五步时序账（篇 2.4 + WD-040）。
//!
//! # 冲刷顺序是硬性的（篇 2.4 逐字）
//!
//! 应用层缓冲冲刷（vx-SDK 的会话结束回调）→ ext4 日志提交 → 交接分区
//! FAT 表冲刷 → 块层队列排空 → 最后才允许进入武装状态。每一步有超时
//! （合计预算十五秒，MD1 第 21.5 节的全程二十五秒里给它一半多——数据
//! 安全的时间要舍得给），超时则**停在 flushing 状态报错**，绝不"差不多
//! 就重启"（B-203）。
//!
//! # 与存储栈的边界
//!
//! WP-102 定的是协议面：步序、预算、超时归宿。真实执行器（ext4 日志、
//! FAT 冲刷、块队列）由 WP-203 存储栈接线——本模块以 [`FlushStep`] trait
//! 收口执行器，测试用假步注入时序；实机冲刷面随 WP-203 对练。
//!
//! # WD-040 交接五步时序账
//!
//! 保全→冲刷→闸门→写变量→重启，全程 ≤ 25s 且有画面。[`FiveStepLedger`]
//! 把五段耗时记成可审计的账（screen 模块的帧日志与之对账）。

use alloc::format;
use alloc::string::String;

/// 冲刷总预算（十五秒——数据安全的时间要舍得给）。
pub const FLUSH_BUDGET_MS: u64 = 15_000;
/// WD-040 交接全程预算（二十五秒）。
pub const HANDOFF_TOTAL_BUDGET_MS: u64 = 25_000;

/// 冲刷步标识（篇 2.4 的硬顺序，序号即步序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlushStepId {
    /// 应用层缓冲冲刷（vx-SDK 的会话结束回调）。
    AppBuffers,
    /// ext4 日志提交。
    Ext4Commit,
    /// 交接分区 FAT 表冲刷。
    FatFlush,
    /// 块层队列排空。
    BlockDrain,
}

impl FlushStepId {
    pub fn name(self) -> &'static str {
        match self {
            FlushStepId::AppBuffers => "应用层缓冲冲刷",
            FlushStepId::Ext4Commit => "ext4 日志提交",
            FlushStepId::FatFlush => "交接分区 FAT 表冲刷",
            FlushStepId::BlockDrain => "块层队列排空",
        }
    }
}

/// 冲刷步执行失败的原因（人话）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlushErr {
    /// 这一步在自己的预算内没做完——管线停，状态停在 flushing。
    Timeout,
    /// 执行失败（IO 错误等）。
    Failed(&'static str),
}

/// 冲刷步执行器 trait。`budget_ms` 是分给这一步的剩余预算；返回实际
/// 耗时。真实执行器由 WP-203 接线；测试注入假步。
pub trait FlushStep {
    fn id(&self) -> FlushStepId;
    /// 返回 Ok(耗时毫秒) 或 Err。实现方可在预算耗尽时返回
    /// [`FlushErr::Timeout`]；管线还会用返回的耗时做总额守门——两道闸
    /// 都不可绕过。
    fn run(&mut self, budget_ms: u64) -> Result<u64, FlushErr>;
}

/// 冲刷结果（B-203 的可观测面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlushOutcome {
    /// 四步全部在预算内完成。
    Done {
        /// 每一步的耗时（按步序）。
        per_step_ms: [u64; 4],
        total_ms: u64,
    },
    /// 超时：停在 flushing 报错，绝不"差不多就重启"。
    TimedOut { step: FlushStepId, spent_ms: u64 },
    /// 执行失败：同样停在 flushing 报错。
    Failed { step: FlushStepId, err: FlushErr },
}

/// 跑冲刷管线：按硬序执行，累计耗时守 15s 总预算。任一步超时/失败
/// 即停（后续步不再执行），报错并指认是哪一步——冲刷的账要记到步。
/// （`steps` 按篇 2.4 硬序传入四步。）
pub fn run_flush(steps: &mut [&mut dyn FlushStep]) -> FlushOutcome {
    let mut spent: u64 = 0;
    let mut per_step: [u64; 4] = [0; 4];
    for (slot, s) in steps.iter_mut().enumerate() {
        let id = s.id();
        let remaining = FLUSH_BUDGET_MS.saturating_sub(spent);
        let budget = remaining.max(1);
        match s.run(budget) {
            Ok(cost) => {
                if slot < 4 {
                    per_step[slot] = cost;
                }
                spent = spent.saturating_add(cost);
                if spent > FLUSH_BUDGET_MS {
                    // 两道闸的第二道：步自称没超时，但总额出线一样停。
                    return FlushOutcome::TimedOut { step: id, spent_ms: spent };
                }
            }
            Err(FlushErr::Timeout) => {
                return FlushOutcome::TimedOut { step: id, spent_ms: spent };
            }
            Err(e) => {
                return FlushOutcome::Failed { step: id, err: e };
            }
        }
    }
    FlushOutcome::Done { per_step_ms: per_step, total_ms: spent }
}

/// 冲刷超时/失败时的用户可见文案（ flushing 状态的报告面）。
pub fn flush_report(o: &FlushOutcome) -> String {
    match o {
        FlushOutcome::Done { total_ms, .. } => {
            format!("安全保存完成（{} 毫秒）", total_ms)
        }
        FlushOutcome::TimedOut { step, spent_ms } => format!(
            "正在安全保存未能在时限内完成：{} 超时（已用 {} 毫秒 / 预算 {} 毫秒）——请勿断电，系统停在冲刷状态等待处置，绝不带病重启",
            step.name(),
            spent_ms,
            FLUSH_BUDGET_MS
        ),
        FlushOutcome::Failed { step, err } => format!(
            "安全保存失败：{} 出错（{}）——请勿断电，系统停在冲刷状态等待处置",
            step.name(),
            match err {
                FlushErr::Timeout => "预算内未完成",
                FlushErr::Failed(w) => w,
            }
        ),
    }
}

// ---------------------------------------------------------------------------
// WD-040 交接五步时序账（保全→冲刷→闸门→写变量→重启，全程 ≤ 25s）
// ---------------------------------------------------------------------------

/// 五步时序账：每段耗时 + 总账判定。交接会话跑完一段记一段。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FiveStepLedger {
    pub preserve_ms: u64,
    pub flush_ms: u64,
    pub gate_ms: u64,
    pub arm_ms: u64,
    pub reboot_ms: u64,
}

impl FiveStepLedger {
    pub fn total_ms(&self) -> u64 {
        self.preserve_ms
            .saturating_add(self.flush_ms)
            .saturating_add(self.gate_ms)
            .saturating_add(self.arm_ms)
            .saturating_add(self.reboot_ms)
    }

    /// WD-040 达标判定：全程 ≤ 25s。"且有画面"由 screen 模块的帧日志
    /// 对账（四帧覆盖五步，见 screen::FrameLog::covers_ledger）。
    pub fn within_wd040(&self) -> bool {
        self.total_ms() <= HANDOFF_TOTAL_BUDGET_MS
    }

    /// 人话账目（诊断面）。
    pub fn describe(&self) -> String {
        format!(
            "交接五步时序：保全 {}ms + 冲刷 {}ms + 闸门 {}ms + 写变量 {}ms + 重启 {}ms = {}ms（预算 {}ms，{}）",
            self.preserve_ms,
            self.flush_ms,
            self.gate_ms,
            self.arm_ms,
            self.reboot_ms,
            self.total_ms(),
            HANDOFF_TOTAL_BUDGET_MS,
            if self.within_wd040() { "达标" } else { "超预算" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 假步：耗固定时长，可选注入超时/失败。
    struct Fake {
        id: FlushStepId,
        cost_ms: u64,
        err: Option<FlushErr>,
        /// 记录管线实际分给本步的预算（验证预算分配面）。
        seen_budget: alloc::vec::Vec<u64>,
    }
    impl Fake {
        fn ok(id: FlushStepId, cost_ms: u64) -> Fake {
            Fake { id, cost_ms, err: None, seen_budget: alloc::vec::Vec::new() }
        }
        fn err(id: FlushStepId, e: FlushErr) -> Fake {
            Fake { id, cost_ms: 0, err: Some(e), seen_budget: alloc::vec::Vec::new() }
        }
    }
    impl FlushStep for Fake {
        fn id(&self) -> FlushStepId {
            self.id
        }
        fn run(&mut self, budget_ms: u64) -> Result<u64, FlushErr> {
            self.seen_budget.push(budget_ms);
            match self.err {
                Some(e) => Err(e),
                None => Ok(self.cost_ms),
            }
        }
    }

    fn hard_order(ids: &[FlushStepId]) -> bool {
        ids == [
            FlushStepId::AppBuffers,
            FlushStepId::Ext4Commit,
            FlushStepId::FatFlush,
            FlushStepId::BlockDrain,
        ]
    }

    #[test]
    fn b203_all_fast_steps_done() {
        let mut a = Fake::ok(FlushStepId::AppBuffers, 300);
        let mut b = Fake::ok(FlushStepId::Ext4Commit, 1_200);
        let mut c = Fake::ok(FlushStepId::FatFlush, 400);
        let mut d = Fake::ok(FlushStepId::BlockDrain, 100);
        let out = run_flush(&mut [&mut a, &mut b, &mut c, &mut d]);
        match out {
            FlushOutcome::Done { per_step_ms, total_ms } => {
                assert_eq!(per_step_ms, [300, 1_200, 400, 100]);
                assert_eq!(total_ms, 2_000);
            }
            other => panic!("预期 Done，实际 {:?}", other),
        }
        // 执行序是硬性的：四步按篇 2.4 顺序跑（用收到的预算序列旁证——
        // 每一步收到的预算递减，说明是按序累计扣减）。
        let budgets = |f: &Fake| f.seen_budget.clone();
        let seq = [budgets(&a), budgets(&b), budgets(&c), budgets(&d)];
        assert_eq!(seq[0], alloc::vec![FLUSH_BUDGET_MS], "第一步拿全额预算");
        assert!(seq[1][0] < seq[0][0] && seq[2][0] < seq[1][0] && seq[3][0] < seq[2][0]);
        assert!(hard_order(&[a.id, b.id, c.id, d.id]));
    }

    #[test]
    fn b203_step_timeout_stops_pipeline() {
        // 第二步超时：第三/四步不再执行，指认到步。
        let mut a = Fake::ok(FlushStepId::AppBuffers, 500);
        let mut b = Fake::err(FlushStepId::Ext4Commit, FlushErr::Timeout);
        let mut c = Fake::ok(FlushStepId::FatFlush, 100);
        let mut d = Fake::ok(FlushStepId::BlockDrain, 100);
        let out = run_flush(&mut [&mut a, &mut b, &mut c, &mut d]);
        assert_eq!(
            out,
            FlushOutcome::TimedOut { step: FlushStepId::Ext4Commit, spent_ms: 500 }
        );
        assert!(c.seen_budget.is_empty(), "超时后续步不得执行");
        assert!(d.seen_budget.is_empty());
        // 报错文案：停 flushing、请勿断电、绝不带病重启。
        let report = flush_report(&out);
        assert!(report.contains("ext4 日志提交"));
        assert!(report.contains("请勿断电"));
        assert!(report.contains("绝不带病重启"));
    }

    #[test]
    fn b203_total_budget_enforced_even_if_steps_lie() {
        // 步自称各自没超时，但总额出线（4000×4 = 16000 > 15000）——
        // 第二道闸在总额处拦停，指认最后越过线的一步。
        let mut steps: alloc::vec::Vec<Fake> = [
            FlushStepId::AppBuffers,
            FlushStepId::Ext4Commit,
            FlushStepId::FatFlush,
            FlushStepId::BlockDrain,
        ]
        .iter()
        .map(|id| Fake::ok(*id, 4_000))
        .collect();
        let mut refs: alloc::vec::Vec<&mut dyn FlushStep> =
            steps.iter_mut().map(|s| s as &mut dyn FlushStep).collect();
        let out = run_flush(&mut refs);
        match out {
            FlushOutcome::TimedOut { step, spent_ms } => {
                assert_eq!(spent_ms, 12_000 + 4_000, "第四步越线（3×4000+4000=16000）");
                assert_eq!(step, FlushStepId::BlockDrain);
            }
            other => panic!("预期 TimedOut，实际 {:?}", other),
        }
    }

    #[test]
    fn b203_io_failure_stops_pipeline() {
        let mut a = Fake::ok(FlushStepId::AppBuffers, 100);
        let mut b = Fake::err(FlushStepId::Ext4Commit, FlushErr::Failed("日志区只读"));
        let mut c = Fake::ok(FlushStepId::FatFlush, 100);
        let mut d = Fake::ok(FlushStepId::BlockDrain, 100);
        let out = run_flush(&mut [&mut a, &mut b, &mut c, &mut d]);
        assert_eq!(
            out,
            FlushOutcome::Failed { step: FlushStepId::Ext4Commit, err: FlushErr::Failed("日志区只读") }
        );
        let report = flush_report(&out);
        assert!(report.contains("日志区只读") && report.contains("请勿断电"));
    }

    #[test]
    fn wd040_ledger_within_and_over() {
        // WD-040 实测形态：2.0 + 4.0 + 0.3 + 3.5 + 2.5 = 12.3s ≤ 25s。
        let ok = FiveStepLedger {
            preserve_ms: 2_000,
            flush_ms: 4_000,
            gate_ms: 300,
            arm_ms: 3_500,
            reboot_ms: 2_500,
        };
        assert!(ok.within_wd040());
        assert_eq!(ok.total_ms(), 12_300);
        assert!(ok.describe().contains("达标"));
        // 超预算形态：25.1s 出线。
        let over = FiveStepLedger {
            preserve_ms: 10_000,
            flush_ms: 15_000,
            gate_ms: 50,
            arm_ms: 30,
            reboot_ms: 20,
        };
        assert!(!over.within_wd040());
        assert!(over.describe().contains("超预算"));
        // 预算关系：冲刷预算 15s ⊂ 全程 25s（"给它一半多"）。
        assert!(FLUSH_BUDGET_MS > HANDOFF_TOTAL_BUDGET_MS / 2);
    }
}
