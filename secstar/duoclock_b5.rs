//! F182 双域时钟同步 · 批次五深化（secstar · G-G-12）。
//!
//! 批次五功能面（与 b3「估计与校正」、b4「时区与告警」互补，本批管
//! 「帧收发与 RTC 健康」）：
//! - [`HandoffFrameLedger`]：交接帧收发账——10 轮收发字节数/丢帧检出
//!   （协议面：谁没回帧一目了然——丢帧不静默）；
//! - [`SlewLimiter`]：平滑限幅——单拍校正强制 ≤100ms（slew 语义的
//!   执行面：任何调用方都超不了步长——限幅不是约定是钳制）；
//! - [`RtcHealth`]：RTC 健康账——失效/恢复事件 + 失效时长
//!   （推断态的出现与终结都有账——黄标的来龙去脉）；
//! - [`sync_due`]：到期判定——上次同步 + 间隔 → 是否该对表。
//!
//! 零堆纪律：定长环 + 定长账，无 alloc。

use super::duoclock::{Confidence, SNAPSHOT_LEN};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 交接帧收发账
// ---------------------------------------------------------------------------

/// 交接轮数（10 轮——主册口径）。
pub const HANDOFF_FRAMES: usize = 10;

pub struct HandoffFrameLedger {
    sent: [bool; HANDOFF_FRAMES],
    acked: [bool; HANDOFF_FRAMES],
    pub sent_bytes: u64,
    pub ack_bytes: u64,
}

impl HandoffFrameLedger {
    pub const fn new() -> HandoffFrameLedger {
        HandoffFrameLedger { sent: [false; HANDOFF_FRAMES], acked: [false; HANDOFF_FRAMES], sent_bytes: 0, ack_bytes: 0 }
    }

    /// 记发送（SNAPSHOT_LEN 字节）。
    pub fn on_send(&mut self, round: usize) -> bool {
        if round >= HANDOFF_FRAMES || self.sent[round] {
            return false;
        }
        self.sent[round] = true;
        self.sent_bytes += SNAPSHOT_LEN as u64;
        true
    }

    /// 记回执（16B 应答帧）。
    pub fn on_ack(&mut self, round: usize) -> bool {
        if round >= HANDOFF_FRAMES || !self.sent[round] || self.acked[round] {
            return false; // 未发先 ack / 重复 ack = 协议违纪，拒收
        }
        self.acked[round] = true;
        self.ack_bytes += SNAPSHOT_LEN as u64;
        true
    }

    /// 丢帧清单：发了没回执的轮号列表。
    pub fn lost_rounds(&self, out: &mut [usize; HANDOFF_FRAMES]) -> usize {
        let mut k = 0;
        for r in 0..HANDOFF_FRAMES {
            if self.sent[r] && !self.acked[r] && k < out.len() {
                out[k] = r;
                k += 1;
            }
        }
        k
    }

    /// 协议完成：10 发 10 回（字节账 = 320B）。
    pub fn protocol_complete(&self) -> bool {
        self.sent_bytes == self.ack_bytes && self.sent_bytes == (HANDOFF_FRAMES * SNAPSHOT_LEN) as u64
    }
}

// ---------------------------------------------------------------------------
// 平滑限幅
// ---------------------------------------------------------------------------

/// 单拍校正绝对上限（ms——slew 步长的强制钳制）。
pub const SLEW_CLAMP_MS: i64 = 100;

/// 限幅：任何请求步长被钳到 ±100ms（限幅是钳制不是约定）。
pub fn clamp_slew(requested_ms: i64) -> i64 {
    requested_ms.clamp(-SLEW_CLAMP_MS, SLEW_CLAMP_MS)
}

/// 多拍收敛上界：偏差 D 需要 ceil(|D|/100) 拍（进度可预期面）。
pub fn slew_ticks_needed(offset_ms: i64) -> u64 {
    (offset_ms.abs() as u64).div_ceil(SLEW_CLAMP_MS as u64)
}

// ---------------------------------------------------------------------------
// RTC 健康账
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RtcEvent {
    pub at_ms: u64,
    pub failed: bool, // true=失效 false=恢复
}

/// RTC 健康账（16 事件环——黄标的来龙去脉）。
pub struct RtcHealth {
    events: [Option<RtcEvent>; 16],
    head: usize,
    pub n: usize,
    pub fail_count: u32,
    pub recover_count: u32,
}

impl RtcHealth {
    pub const fn new() -> RtcHealth {
        RtcHealth { events: [const { None }; 16], head: 0, n: 0, fail_count: 0, recover_count: 0 }
    }

    /// 记事件（失效→恢复交替校验：重复失效/无失效恢复 = 违纪拒收）。
    pub fn record(&mut self, at_ms: u64, failed: bool) -> bool {
        let last_failed = self.last_state();
        let valid = match (last_failed, failed) {
            (None, true) => true,       // 首次失效
            (Some(true), false) => true, // 失效后恢复
            (Some(false), true) => true, // 恢复后再失效
            _ => false,                  // 重复失效 / 无失效恢复
        };
        if !valid {
            return false;
        }
        if failed {
            self.fail_count += 1;
        } else {
            self.recover_count += 1;
        }
        if self.n < 16 {
            self.n += 1;
        }
        self.events[self.head] = Some(RtcEvent { at_ms, failed });
        self.head = (self.head + 1) % 16;
        true
    }

    fn last_state(&self) -> Option<bool> {
        if self.n == 0 {
            return None;
        }
        self.events[(self.head + 16 - 1) % 16].map(|e| e.failed)
    }

    /// 最近一次失效时长（未在失效中 → None——恢复后不谎报时长）。
    pub fn last_failure_duration(&self, now_ms: u64) -> Option<u64> {
        if self.last_state() != Some(true) {
            return None; // 已恢复/从未失效 → 无现行失效
        }
        // 找最近一次失效时刻。
        for i in 0..self.n {
            let idx = (self.head + 16 - 1 - i) % 16;
            if let Some(e) = self.events[idx] {
                if e.failed {
                    return Some(now_ms.saturating_sub(e.at_ms));
                }
            }
        }
        None
    }

    /// 置信度建议：失效中（末事件=failed）→ Inferred（黄标），恢复后
    /// → 恢复前级别。
    pub fn confidence_now(&self, restored: Confidence) -> Confidence {
        if self.last_state() == Some(true) {
            Confidence::Inferred
        } else {
            restored
        }
    }
}

// ---------------------------------------------------------------------------
// 到期判定
// ---------------------------------------------------------------------------

/// 到期判定：上次同步 + 间隔 ≤ now → 该对表了。
pub fn sync_due(last_sync_ms: u64, interval_ms: u64, now_ms: u64) -> bool {
    now_ms.saturating_sub(last_sync_ms) >= interval_ms
}

// ---------------------------------------------------------------------------
// 批次五自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_duoclock_b5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F182-b5");

    // 1) 帧账：10 发 10 回 → 协议完成、字节账 320（协议面全清）。
    let mut l = HandoffFrameLedger::new();
    for r in 0..HANDOFF_FRAMES {
        assert!(l.on_send(r));
    }
    for r in 0..HANDOFF_FRAMES {
        assert!(l.on_ack(r));
    }
    cs.add("frame_protocol_complete", l.protocol_complete() && l.sent_bytes == 160 && l.ack_bytes == 160, "");

    // 2) 丢帧检出：第 3 轮没回 → 丢帧清单恰一项（丢帧不静默）。
    let mut l2 = HandoffFrameLedger::new();
    for r in 0..HANDOFF_FRAMES {
        l2.on_send(r);
    }
    for r in 0..HANDOFF_FRAMES {
        if r != 3 {
            l2.on_ack(r);
        }
    }
    let mut lost = [0usize; HANDOFF_FRAMES];
    let k = l2.lost_rounds(&mut lost);
    cs.add("frame_lost_detected", k == 1 && lost[0] == 3, "");

    // 3) 协议违纪拒：未发先 ack / 重复 ack 都拒（协议不是摆设）。
    let mut l3 = HandoffFrameLedger::new();
    let ack_first = l3.on_ack(0);
    l3.on_send(0);
    l3.on_ack(0);
    let dup_ack = l3.on_ack(0);
    cs.add("frame_violation_rejected", !ack_first && !dup_ack, "");

    // 4) 平滑限幅：500ms 请求钳到 100、-300 钳到 -100、50 原样（三态）。
    cs.add(
        "slew_clamp",
        clamp_slew(500) == 100 && clamp_slew(-300) == -100 && clamp_slew(50) == 50,
        "",
    );

    // 5) 收敛拍数：500ms → 5 拍、99ms → 1 拍、0 → 0（进度可预期）。
    cs.add(
        "slew_ticks",
        slew_ticks_needed(500) == 5 && slew_ticks_needed(99) == 1 && slew_ticks_needed(0) == 0,
        "",
    );

    // 6) RTC 健康序列：失效→恢复合法（黄标来龙去脉在账）。
    let mut r = RtcHealth::new();
    let f = r.record(1_000, true);
    let rec = r.record(11_000, false);
    cs.add(
        "rtc_fail_recover",
        f && rec && r.fail_count == 1 && r.recover_count == 1 && r.n == 2,
        "",
    );

    // 7) RTC 违纪拒：重复失效 / 无失效恢复（状态机不胡来）。
    let mut r2 = RtcHealth::new();
    r2.record(1_000, true);
    let dup_fail = r2.record(2_000, true);
    let bad_recover = RtcHealth::new().record(1_000, false);
    cs.add("rtc_violation_rejected", !dup_fail && !bad_recover, "");

    // 8) RTC 失效时长：失效中算时长、恢复后 None（不谎报）。
    let mut r3 = RtcHealth::new();
    r3.record(1_000, true);
    let during = r3.last_failure_duration(6_000) == Some(5_000);
    r3.record(8_000, false);
    let after = r3.last_failure_duration(9_000).is_none();
    cs.add("rtc_duration_honest", during && after, "");

    // 9) RTC 置信度建议：失效中 → Inferred 黄标；恢复 → 回原级。
    cs.add(
        "rtc_confidence",
        r3.confidence_now(Confidence::NtpCalibrated) == Confidence::NtpCalibrated
            && r2.confidence_now(Confidence::NtpCalibrated) == Confidence::Inferred,
        "",
    );

    // 10) 到期判定：恰到期该对表、未到不骚扰（两态逐点）。
    cs.add(
        "sync_due",
        sync_due(0, 3_600_000, 3_600_000) && !sync_due(0, 3_600_000, 3_599_999),
        "",
    );

    // 11) 主册常量贯通：帧长 16B 一处一事实。
    cs.add("consts_aligned", SNAPSHOT_LEN == 16 && SLEW_CLAMP_MS == 100, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次五）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b5 {
    use super::*;

    #[test]
    fn rtc_multi_cycles() {
        // 多轮失效-恢复交替：计数与状态推进全对（黄标多次来去）。
        let mut r = RtcHealth::new();
        assert!(r.record(0, true));
        assert!(r.record(5_000, false));
        assert!(r.record(60_000, true));
        assert!(r.record(66_000, false));
        assert_eq!((r.fail_count, r.recover_count), (2, 2));
        assert_eq!(r.confidence_now(Confidence::RtcDirect), Confidence::RtcDirect);
    }

    #[test]
    fn frame_ledger_ignores_ack_without_send_but_counts_after() {
        // 违纪 ack 不记账不污染后续合法流（协议面健壮性）。
        let mut l = HandoffFrameLedger::new();
        assert!(!l.on_ack(2));
        assert!(l.on_send(2));
        assert!(l.on_ack(2));
        assert_eq!(l.sent_bytes, 16);
        assert_eq!(l.ack_bytes, 16);
    }

    #[test]
    fn slew_ticks_large_offsets() {
        // 大偏差拍数：10s → 100 拍（5s 线之上走 step——此处为 slew 上界复核）。
        assert_eq!(slew_ticks_needed(10_000), 100);
        assert_eq!(slew_ticks_needed(1), 1);
    }
}
