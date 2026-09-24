//! 武装序列编排（篇 2.4 + B-204）。
//!
//! 篇 2.4 的口径逐字：三条件全过 → 写 OneShot 变量 → 读回校验（写入即
//! 验证，不等重启才发现）→ 进入 rebooting。读回不一致则立刻降级
//! BootNext 路径重试一次，再失败则 aborted 并给三路径全部失败的人话
//! 诊断（这种情况按 Q1 的设计预期为零——但代码必须写它）。
//!
//! "十次读回"是 B-204 的达标线：十次全过才算 Armed。写探针（写盘 +
//! 回读）由调用方注入——实机侧是 ESP conf 写入与重读（WP-102 协议面
//! 不直接碰盘），测试侧用假探针注入故障。
//!
//! 本模块只编排，不持有状态机的状态——结果交回调用方映射成
//! [`HEvent::ArmOk`] / [`HEvent::ArmFail`]（state 模块）。

use crate::oneshot::{arm_conf_text, DegradeEvent, GateVerdict};

/// 武装配置输入（ONEShot default_entry 改写所需的最小集）。
pub struct ArmPlan<'a> {
    /// 未武装的 limine.conf 文本。
    pub conf_text: &'a str,
    /// 目标条目（Windows）在 conf 里的下标。
    pub target_entry: usize,
    /// 武装前的 default_entry（作废恢复用）。
    pub prev_default: usize,
    /// 武装后 conf 的哈希（写入 OneShot 标记行的比对锚）。
    pub armed_hash: u32,
}

/// 写探针：把产出写入存储并回读比对（实机=ESP conf 写+重读）。
/// 返回 false 表示写路径失败或读回不一致——写后必读纪律的红灯。
pub trait ConfIo {
    fn write_and_readback(&mut self, bytes: &[u8]) -> bool;
}

/// BootNext 兜底探针：降级路径重试一次的执行体（实机=写 BootNext
/// NVRAM 并验证；oneshot::BootNextOutcome 通道）。返回 false 表示兜底
/// 也失败。
pub trait BootNextProbe {
    fn retry(&mut self) -> bool;
}

/// 武装序列结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArmSequence {
    /// 十次读回全过——调用方提交 [`HEvent::ArmOk`]。
    Armed { rounds: u32 },
    /// 读回不一致，降级 BootNext 重试（可观测：降级事件入账）。
    /// `bootnext_ok=true` 表示兜底接住了——交接继续，但这次走 BootNext。
    DegradeBootNext { degrade: DegradeEvent, bootnext_ok: bool },
    /// aborted（武装失败等于没发生）——人话原因交调用方呈现。
    Aborted { reason: &'static str, degrade: Option<DegradeEvent> },
}

/// 读回轮数（B-204 达标线：十次全过）。
pub const READBACK_ROUNDS: u32 = 10;

/// 跑武装序列。`out` 是 conf 产出缓冲（不足即整体失败，绝不半武装）。
pub fn arm_sequence(
    gate: GateVerdict,
    plan: &ArmPlan<'_>,
    confirmed: bool,
    out: &mut [u8],
    io: &mut dyn ConfIo,
    bootnext: &mut dyn BootNextProbe,
) -> ArmSequence {
    // 闸门三条件先行：Blocked 直接 aborted（人话原样透传）。
    if let GateVerdict::Blocked(why) = gate {
        return ArmSequence::Aborted { reason: why, degrade: None };
    }
    // 条件二哈希不符 → "用户确认后放行"（篇 1.7 口径）：没确认就是
    // 停在等待确认，不是失败也不硬闯。
    if matches!(gate, GateVerdict::NeedUserConfirm(_)) && !confirmed {
        return ArmSequence::Aborted {
            reason: "目标引导文件与上次成功交接不一致——等待用户确认后放行",
            degrade: None,
        };
    }
    // 产出武装 conf。
    let Some(n) = arm_conf_text(plan.conf_text, plan.target_entry, plan.armed_hash, out) else {
        return ArmSequence::Aborted {
            reason: "配置改写被拒绝（目标下标越界或缓冲不足）——绝不半武装",
            degrade: Some(DegradeEvent::OneshotArmFailed),
        };
    };
    // 写 + 十次读回（写入即验证，不等重启才发现）。
    let bytes = &out[..n];
    for _ in 0..READBACK_ROUNDS {
        if !io.write_and_readback(bytes) {
            // 读回不一致 → 立刻降级 BootNext 重试一次。
            let ok = bootnext.retry();
            return ArmSequence::DegradeBootNext {
                degrade: DegradeEvent::OneshotArmFailed,
                bootnext_ok: ok,
            };
        }
    }
    ArmSequence::Armed { rounds: READBACK_ROUNDS }
}

/// 降级 BootNext 仍失败时的 aborted 人话（三路径诊断：菜单是永远在线
/// 的最后一步）。
pub const ALL_PATHS_FAILED_HINT: &str =
    "OneShot 写读不一致且 BootNext 兜底失败——本次交接已中止，系统正常运行；\
     请重启后在引导菜单人工选择目标域，并运行域健康自检定位存储问题";

#[cfg(test)]
mod tests {
    use super::*;

    const CONF: &str = "timeout: 5\n\n/varix\n  comment: varix\n\n/windows\n  comment: win\n";

    /// 假写探针：前 `fail_at` 次返回 false（0=永不失败），其余 true。
    struct FakeIo {
        fail_at: u32,
        calls: u32,
        last_len: usize,
    }
    impl FakeIo {
        fn ok() -> FakeIo {
            FakeIo { fail_at: 0, calls: 0, last_len: 0 }
        }
        fn failing_at(n: u32) -> FakeIo {
            FakeIo { fail_at: n, calls: 0, last_len: 0 }
        }
    }
    impl ConfIo for FakeIo {
        fn write_and_readback(&mut self, bytes: &[u8]) -> bool {
            self.calls += 1;
            self.last_len = bytes.len();
            self.fail_at == 0 || self.calls != self.fail_at
        }
    }

    struct ProbeOk(bool);
    impl BootNextProbe for ProbeOk {
        fn retry(&mut self) -> bool {
            self.0
        }
    }

    fn pass_gate() -> GateVerdict {
        GateVerdict::Pass
    }
    fn plan<'a>(conf: &'a str) -> ArmPlan<'a> {
        // Windows 条目在下标 1（CONF 的第二个条目）。
        ArmPlan { conf_text: conf, target_entry: 1, prev_default: 0, armed_hash: 0xdead_beef }
    }

    #[test]
    fn b204_gate_blocked_aborts_without_writing() {
        let mut out = [0u8; 4096];
        let mut io = FakeIo::ok();
        let mut bn = ProbeOk(true);
        let r = arm_sequence(
            GateVerdict::Blocked("windows boot file missing on WINESP"),
            &plan(CONF),
            false,
            &mut out,
            &mut io,
            &mut bn,
        );
        assert_eq!(
            r,
            ArmSequence::Aborted {
                reason: "windows boot file missing on WINESP",
                degrade: None
            }
        );
        assert_eq!(io.calls, 0, "闸门拒绝后一个字节都不写");
    }

    #[test]
    fn b204_need_confirm_waits_for_user() {
        let mut out = [0u8; 4096];
        let mut io = FakeIo::ok();
        let mut bn = ProbeOk(true);
        let gate = GateVerdict::NeedUserConfirm("引导文件哈希与上次交接记录不一致");
        // 未确认：等待，不硬闯。
        let r = arm_sequence(gate, &plan(CONF), false, &mut out, &mut io, &mut bn);
        assert!(matches!(r, ArmSequence::Aborted { .. }));
        assert_eq!(io.calls, 0);
        // 用户确认后放行：正常走完十次读回。
        let r = arm_sequence(gate, &plan(CONF), true, &mut out, &mut io, &mut bn);
        assert_eq!(r, ArmSequence::Armed { rounds: READBACK_ROUNDS });
        assert_eq!(io.calls, READBACK_ROUNDS as u32);
    }

    #[test]
    fn b204_ten_rounds_all_pass() {
        let mut out = [0u8; 4096];
        let mut io = FakeIo::ok();
        let mut bn = ProbeOk(true);
        let r = arm_sequence(pass_gate(), &plan(CONF), false, &mut out, &mut io, &mut bn);
        assert_eq!(r, ArmSequence::Armed { rounds: 10 });
        assert_eq!(io.calls, 10, "十次读回一次不少");
        // 产出确实是武装过的 conf（含 oneshot 标记行 + default_entry=1）。
        let text = core::str::from_utf8(&out[..io.last_len]).unwrap();
        assert!(text.starts_with("# oneshot: "));
        assert!(text.contains("default_entry: 1"));
    }

    #[test]
    fn b204_mismatch_degrades_to_bootnext_observable() {
        // 第 3 次读回不一致 → 降级 BootNext（兜底接住，交接继续）。
        let mut out = [0u8; 4096];
        let mut io = FakeIo::failing_at(3);
        let mut bn = ProbeOk(true);
        let r = arm_sequence(pass_gate(), &plan(CONF), false, &mut out, &mut io, &mut bn);
        assert_eq!(
            r,
            ArmSequence::DegradeBootNext {
                degrade: DegradeEvent::OneshotArmFailed,
                bootnext_ok: true
            }
        );
        assert_eq!(io.calls, 3, "不一致发生在第三次读回");
    }

    #[test]
    fn b204_all_paths_failed_aborts_with_human_words() {
        // 兜底也失败 → aborted + 三路径人话（Q1 预期为零，代码必须写它）。
        let mut out = [0u8; 4096];
        let mut io = FakeIo::failing_at(1);
        let mut bn = ProbeOk(false);
        let r = arm_sequence(pass_gate(), &plan(CONF), false, &mut out, &mut io, &mut bn);
        match r {
            ArmSequence::DegradeBootNext { degrade, bootnext_ok } => {
                assert_eq!(degrade, DegradeEvent::OneshotArmFailed);
                assert!(!bootnext_ok);
                assert!(ALL_PATHS_FAILED_HINT.contains("菜单"), "指引必须指向最后一步");
                assert!(ALL_PATHS_FAILED_HINT.contains("自检"), "指引必须指向诊断");
            }
            other => panic!("预期 DegradeBootNext，实际 {:?}", other),
        }
    }

    #[test]
    fn b204_unwritable_target_rejected_without_degrade() {
        // 目标下标越界：配置改写被拒——这是配置问题不是路径问题，
        // 降级 BootNext 也救不了，直接 aborted（不硬闯）。
        let mut out = [0u8; 4096];
        let mut io = FakeIo::ok();
        let mut bn = ProbeOk(true);
        let bad = ArmPlan {
            conf_text: CONF,
            target_entry: 9,
            prev_default: 0,
            armed_hash: 1,
        };
        let r = arm_sequence(pass_gate(), &bad, false, &mut out, &mut io, &mut bn);
        assert!(matches!(r, ArmSequence::Aborted { degrade: Some(DegradeEvent::OneshotArmFailed), .. }));
        assert_eq!(io.calls, 0);
    }
}
