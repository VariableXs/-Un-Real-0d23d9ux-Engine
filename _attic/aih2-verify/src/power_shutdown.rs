//! 关机软件收尾链与"可拔电"账目（B-2902 · MD2 篇 29.2）。
//!
//! # 硬序（篇 29.2 逐字）
//!
//! 保全（快照与草稿）→ 冲刷（篇 2.4 全序列）→ 通知链（服务收到结束通知
//! 逐个优雅收尾，超时强收）→ ACPI S5。"可以拔电了"画面的出现条件是
//! **冲刷完成加通知链收束**——画面的每一秒都有账可查（账本保留到下次
//! 启动报告）。缺一环，画面就不许出现：数据安全红线的电源面。
//!
//! # 与 WP-102 / WP-203 的边界
//!
//! 冲刷的步序协议与预算在 WP-102 冻结（[`crate::handoff::flush`]），真实
//! 执行器（ext4 日志、FAT 冲刷、块队列）由 WP-203 存储栈接线——本模块把
//! 关机链的**账**先立起来：每个相位都有 verbatim 记录（Ok / Skipped(理由) /
//! Forced / Failed(原因)），`Skipped` 必须带人话理由（"无挂载即无脏数据"
//! 是如实陈述，"差不多就行"不是）。服务注册表现状为零，通知链以空集
//! 全绿记账——链路就位，随服务增长挂入，不假装有服务在收尾。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 关机相位（篇 29.2 硬序，序号即步序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownPhase {
    /// 保全：快照与草稿。
    Preserve,
    /// 冲刷：篇 2.4 全序列（执行器 WP-203 接线）。
    Flush,
    /// 通知链：服务逐个优雅收尾，超时强收。
    NotifyChain,
    /// S5：ACPI 断电（硬件阶梯）。
    S5,
}

impl ShutdownPhase {
    pub fn name(self) -> &'static str {
        match self {
            ShutdownPhase::Preserve => "保全",
            ShutdownPhase::Flush => "冲刷",
            ShutdownPhase::NotifyChain => "通知链",
            ShutdownPhase::S5 => "S5",
        }
    }

    pub const ALL: [ShutdownPhase; 4] = [
        ShutdownPhase::Preserve,
        ShutdownPhase::Flush,
        ShutdownPhase::NotifyChain,
        ShutdownPhase::S5,
    ];
}

/// 单相位结论（账本的最小记账单位——人话，可审计）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhaseVerdict {
    /// 正常完成（附实际耗时毫秒）。
    Ok,
    /// 如实跳过（附理由：无挂载=无脏数据可冲、零服务注册=空集全绿）。
    Skipped(&'static str),
    /// 超时强收（通知链专用：等满了预算，按账标记强收后继续）。
    Forced,
    /// 失败（附原因）——后面相位不再执行，画面绝不出现。
    Failed(&'static str),
}

/// 单相位账目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhaseAccount {
    pub ms: u64,
    pub verdict: PhaseVerdict,
}

/// 关机账本：四相位逐段记账。"可拔电"的判定就从这份账出——
/// 不看感觉，看账。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShutdownLedger {
    pub preserve: Option<PhaseAccount>,
    pub flush: Option<PhaseAccount>,
    pub notify: Option<PhaseAccount>,
    pub s5: Option<PhaseAccount>,
}

impl ShutdownLedger {
    pub fn new() -> ShutdownLedger {
        ShutdownLedger::default()
    }

    pub fn record(&mut self, phase: ShutdownPhase, ms: u64, verdict: PhaseVerdict) {
        let slot = match phase {
            ShutdownPhase::Preserve => &mut self.preserve,
            ShutdownPhase::Flush => &mut self.flush,
            ShutdownPhase::NotifyChain => &mut self.notify,
            ShutdownPhase::S5 => &mut self.s5,
        };
        *slot = Some(PhaseAccount { ms, verdict });
    }

    /// 一相是否"收束"：Ok、有理由的 Skipped、（通知链的）Forced 都算——
    /// 只有 Failed 是真失败。
    fn settled(acc: Option<PhaseAccount>) -> bool {
        match acc {
            Some(PhaseAccount { verdict: PhaseVerdict::Failed(_), .. }) => false,
            Some(_) => true,
            None => false,
        }
    }

    /// "可以拔电了"画面的出现条件（篇 29.2 逐字）：冲刷完成 + 通知链
    /// 收束——保全与 S5 不在画面条件里（保全失败记 Failed 同样拦住；
    /// S5 是画面之后的动作）。
    pub fn unpluggable(&self) -> bool {
        Self::settled(self.preserve) && Self::settled(self.flush) && Self::settled(self.notify)
    }

    /// 总账判定：四相全部收束（关机链走完）。
    pub fn complete(&self) -> bool {
        ShutdownPhase::ALL.iter().all(|p| {
            let acc = match p {
                ShutdownPhase::Preserve => self.preserve,
                ShutdownPhase::Flush => self.flush,
                ShutdownPhase::NotifyChain => self.notify,
                ShutdownPhase::S5 => self.s5,
            };
            Self::settled(acc)
        })
    }

    /// 总耗时（账上出现过的相位之和）。
    pub fn total_ms(&self) -> u64 {
        [self.preserve, self.flush, self.notify, self.s5]
            .iter()
            .flatten()
            .map(|a| a.ms)
            .sum()
    }

    /// 人话账目（诊断面 / 下次启动报告的原料）。
    pub fn describe(&self) -> String {
        let mut out = String::from("关机账目：");
        for p in ShutdownPhase::ALL {
            let acc = match p {
                ShutdownPhase::Preserve => self.preserve,
                ShutdownPhase::Flush => self.flush,
                ShutdownPhase::NotifyChain => self.notify,
                ShutdownPhase::S5 => self.s5,
            };
            match acc {
                Some(a) => {
                    let v = match a.verdict {
                        PhaseVerdict::Ok => "完成".to_string(),
                        PhaseVerdict::Skipped(w) => format!("跳过（{}）", w),
                        PhaseVerdict::Forced => "超时强收".to_string(),
                        PhaseVerdict::Failed(w) => format!("失败（{}）", w),
                    };
                    out.push_str(&format!("{} {}ms/{}", p.name(), a.ms, v));
                }
                None => out.push_str(&format!("{} 未记账", p.name())),
            }
            out.push_str("；");
        }
        if self.unpluggable() {
            out.push_str(UNPLUG_LINE);
        } else {
            out.push_str("不可拔电——账上有一环没收束");
        }
        out
    }
}

/// "可以拔电了"画面行（篇 29.2：出现条件=冲刷完成加通知链收束）。
pub const UNPLUG_LINE: &str = "可以拔电了——数据已安全";

/// 固件不回应时的诚实指引（29.1：用户在"正在关机"画面停留超过十秒）。
pub const FIRMWARE_TIMEOUT_LINE: &str = "固件未响应，可长按电源强制关机——数据已安全";

// ---------------------------------------------------------------------------
// 通知链：服务逐个优雅收尾，超时强收
// ---------------------------------------------------------------------------

/// 通知链成员：一个要被"结束通知"叫醒的服务收尾步。
pub trait NotifyStep {
    /// 服务名（账目指名道姓）。
    fn name(&self) -> &'static str;
    /// 优雅收尾。`budget_ms` 是分给这一步的剩余预算；返回实际耗时，
    /// Err 是优雅收尾失败（调用方按账记 Forced/Failed）。
    fn shutdown(&mut self, budget_ms: u64) -> Result<u64, &'static str>;
}

/// 通知链结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotifyOutcome {
    /// 每步账目：(服务名, 耗时毫秒, 结论)。
    pub steps: Vec<(&'static str, u64, PhaseVerdict)>,
    pub total_ms: u64,
    /// 链是否收束（全部步 Ok/Forced/Skipped——任何 Failed 都没收束）。
    pub settled: bool,
}

impl NotifyOutcome {
    /// 强收计数（账面可见的"没优雅走完"的服务数）。
    pub fn forced_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|(_, _, v)| *v == PhaseVerdict::Forced)
            .count()
    }
}

/// 跑通知链：逐个优雅收尾，单步预算从总预算切片，超时**强收**——
/// 记账后继续下一步（强收的账要留名，绝不悄悄吞掉）。总预算耗尽后
/// 剩余步全部按 Forced 记账收束。
pub fn run_notify_chain(steps: &mut [&mut dyn NotifyStep], budget_ms: u64) -> NotifyOutcome {
    let mut out = NotifyOutcome { steps: Vec::new(), total_ms: 0, settled: true };
    let mut spent: u64 = 0;
    for s in steps.iter_mut() {
        let name = s.name();
        if spent >= budget_ms {
            // 总预算耗尽：剩余服务按强收记账（诚实——没等它优雅走完）。
            out.steps.push((name, 0, PhaseVerdict::Forced));
            continue;
        }
        let slice = budget_ms - spent;
        match s.shutdown(slice) {
            Ok(cost) => {
                let cost = cost.min(slice);
                spent = spent.saturating_add(cost);
                out.total_ms = spent;
                out.steps.push((name, cost, PhaseVerdict::Ok));
            }
            Err(why) => {
                // 单步优雅收尾失败：超时强收，账留名，链继续。
                out.steps.push((name, 0, PhaseVerdict::Forced));
                let _ = why;
            }
        }
    }
    out.settled = out
        .steps
        .iter()
        .all(|(_, _, v)| !matches!(v, PhaseVerdict::Failed(_)));
    out
}

// ---------------------------------------------------------------------------
// 测试：账本全绿/失败拦画面/强收留名/空集全绿/文案就位
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpluggable_requires_flush_and_notify_settled() {
        let mut l = ShutdownLedger::new();
        // 什么都没记：不可拔电。
        assert!(!l.unpluggable());
        l.record(ShutdownPhase::Preserve, 0, PhaseVerdict::Skipped("no drafts"));
        assert!(!l.unpluggable(), "冲刷没收束就不许拔电");
        l.record(ShutdownPhase::Flush, 3, PhaseVerdict::Skipped("no mounted fs"));
        assert!(!l.unpluggable(), "通知链没收束也不许拔电");
        l.record(ShutdownPhase::NotifyChain, 1, PhaseVerdict::Ok);
        assert!(l.unpluggable());
        // S5 不在画面条件里（画面在硬件阶梯之前）。
        assert!(l.unpluggable() && l.s5.is_none());
    }

    #[test]
    fn failed_phase_blocks_unplug() {
        let mut l = ShutdownLedger::new();
        l.record(ShutdownPhase::Preserve, 2, PhaseVerdict::Ok);
        l.record(ShutdownPhase::Flush, 5, PhaseVerdict::Failed("block queue stuck"));
        l.record(ShutdownPhase::NotifyChain, 1, PhaseVerdict::Ok);
        assert!(!l.unpluggable());
        assert!(!l.complete());
        assert!(l.describe().contains("失败（block queue stuck）"));
        assert!(l.describe().contains("不可拔电"));
    }

    #[test]
    fn complete_requires_all_four() {
        let mut l = ShutdownLedger::new();
        l.record(ShutdownPhase::Preserve, 0, PhaseVerdict::Ok);
        l.record(ShutdownPhase::Flush, 4, PhaseVerdict::Ok);
        l.record(ShutdownPhase::NotifyChain, 2, PhaseVerdict::Ok);
        assert!(!l.complete(), "S5 没记账不算走完");
        l.record(ShutdownPhase::S5, 9, PhaseVerdict::Ok);
        assert!(l.complete());
        assert_eq!(l.total_ms(), 15);
    }

    struct OkStep(u64);
    impl NotifyStep for OkStep {
        fn name(&self) -> &'static str {
            "ok-step"
        }
        fn shutdown(&mut self, budget_ms: u64) -> Result<u64, &'static str> {
            Ok(self.0.min(budget_ms))
        }
    }

    struct BadStep;
    impl NotifyStep for BadStep {
        fn name(&self) -> &'static str {
            "bad-step"
        }
        fn shutdown(&mut self, _budget_ms: u64) -> Result<u64, &'static str> {
            Err("stuck device")
        }
    }

    #[test]
    fn notify_chain_empty_set_is_done() {
        // 零服务注册：空集全绿（如实——不假装有服务在收尾）。
        let mut empty: [&mut dyn NotifyStep; 0] = [];
        let o = run_notify_chain(&mut empty, 1000);
        assert!(o.settled);
        assert_eq!(o.total_ms, 0);
        assert_eq!(o.steps.len(), 0);
    }

    #[test]
    fn notify_chain_forced_is_named_in_ledger() {
        // 超时强收：账留名（bad-step），链收束不中断。
        let mut a = OkStep(3);
        let mut b = BadStep;
        let mut c = OkStep(1);
        let o = run_notify_chain(&mut [&mut a, &mut b, &mut c], 1000);
        assert!(o.settled);
        assert_eq!(o.forced_count(), 1);
        assert_eq!(o.steps[0], ("ok-step", 3, PhaseVerdict::Ok));
        assert_eq!(o.steps[1], ("bad-step", 0, PhaseVerdict::Forced));
        assert_eq!(o.steps[2], ("ok-step", 1, PhaseVerdict::Ok));
        assert_eq!(o.total_ms, 4);
    }

    #[test]
    fn notify_chain_budget_exhaust_forces_rest() {
        // 总预算耗尽：剩余步全部按强收记账（剩余预算切片为 0）。
        let mut a = OkStep(100);
        let mut b = OkStep(1);
        let o = run_notify_chain(&mut [&mut a, &mut b], 50);
        assert!(o.settled);
        assert_eq!(o.steps[0], ("ok-step", 50, PhaseVerdict::Ok));
        assert_eq!(o.steps[1], ("ok-step", 0, PhaseVerdict::Forced));
    }

    #[test]
    fn copy_paste_screens_lines() {
        // 文案常量就位（篇 29.1/29.2 逐字要求的人话）。
        assert!(UNPLUG_LINE.contains("可以拔电"));
        assert!(UNPLUG_LINE.contains("数据已安全"));
        assert!(FIRMWARE_TIMEOUT_LINE.contains("固件未响应"));
        assert!(FIRMWARE_TIMEOUT_LINE.contains("长按电源"));
        assert!(FIRMWARE_TIMEOUT_LINE.contains("数据已安全"));
    }

    #[test]
    fn describe_reports_unplug_line_when_green() {
        let mut l = ShutdownLedger::new();
        l.record(ShutdownPhase::Preserve, 0, PhaseVerdict::Skipped("no drafts"));
        l.record(ShutdownPhase::Flush, 2, PhaseVerdict::Skipped("no mounted fs"));
        l.record(ShutdownPhase::NotifyChain, 0, PhaseVerdict::Ok);
        l.record(ShutdownPhase::S5, 8, PhaseVerdict::Ok);
        let d = l.describe();
        assert!(d.contains("保全") && d.contains("跳过"));
        assert!(d.contains(UNPLUG_LINE));
        assert!(d.contains("8ms"));
    }
}
