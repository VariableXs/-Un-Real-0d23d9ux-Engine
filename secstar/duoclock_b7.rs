//! F182 双时钟 · 批次七深化（v7）——NTP 往返质量账、slew 队列、
//! 时间显示格式化、对账快照差异表。零堆、no_std。

use crate::checks::CheckSet;

/// 往返样本环容量。
pub const RTT_CAP: usize = 8;
/// 优良 RTT 线（ms——低于此算优质样本）。
pub const RTT_GOOD_MS: u32 = 50;
/// slew 队列容量。
pub const SLEW_QUEUE_CAP: usize = 8;
/// 单步 slew 步长上限（ms——与主层 slew 语义对齐）。
pub const SLEW_STEP_MS: i64 = 100;

/// NTP 往返质量账：样本环 + 优良率 + 丢包面。
#[derive(Clone, Copy)]
pub struct RttLedger {
    rtts: [u32; RTT_CAP],
    lost: u32,
    head: usize,
    pub n: usize,
}

impl RttLedger {
    pub const fn new() -> RttLedger {
        RttLedger { rtts: [0; RTT_CAP], lost: 0, head: 0, n: 0 }
    }

    pub fn record(&mut self, rtt_ms: u32) {
        if self.n < RTT_CAP {
            self.rtts[self.head] = rtt_ms;
            self.head = (self.head + 1) % RTT_CAP;
            self.n += 1;
        } else {
            self.rtts[self.head] = rtt_ms;
            self.head = (self.head + 1) % RTT_CAP;
        }
    }

    pub fn on_timeout(&mut self) {
        self.lost += 1;
    }

    /// 优良率 ‰（环内 RTT < 线的占比）。
    pub fn good_permille(&self) -> Option<u32> {
        if self.n == 0 {
            return None;
        }
        let mut good = 0u32;
        for &r in self.rtts.iter().take(self.n) {
            if r < RTT_GOOD_MS {
                good += 1;
            }
        }
        Some(good * 1_000 / self.n as u32)
    }

    /// 均往返（零堆两遍）。
    pub fn mean_rtt(&self) -> Option<u32> {
        if self.n == 0 {
            return None;
        }
        let mut sum = 0u64;
        for &r in self.rtts.iter().take(self.n) {
            sum += r as u64;
        }
        Some((sum / self.n as u64) as u32)
    }

    pub fn timeouts(&self) -> u32 {
        self.lost
    }

    /// 服务质量判定：优良率 ≥500‰ 且超时率 ≤200‰ → 可用（NTP 源准入线）。
    pub fn usable(&self) -> Option<bool> {
        let g = self.good_permille()?;
        let total = self.n as u32 + self.lost;
        let lost_permille = if total == 0 { 0 } else { self.lost * 1_000 / total };
        Some(g >= 500 && lost_permille <= 200)
    }
}

/// slew 队列：大偏差不能一步跳（跳钟伤单调度），拆成 ≤100ms 的步序列。
/// 队列存每步的目标时刻偏移；消费端逐拍执行。
#[derive(Clone, Copy)]
pub struct SlewQueue {
    steps: [i64; SLEW_QUEUE_CAP],
    pub n: usize,
    pub dropped: u32,
}

impl SlewQueue {
    pub const fn new() -> SlewQueue {
        SlewQueue { steps: [0; SLEW_QUEUE_CAP], n: 0, dropped: 0 }
    }

    /// 为 offset 偏差生成步序列（每步 ≤ SLEW_STEP_MS，方向随符号）。
    /// 步数 = ceil(|off| / 100)；超出队列容量 → 诚实拒绝并计 dropped。
    pub fn plan(&mut self, offset_ms: i64) -> bool {
        let abs = offset_ms.unsigned_abs();
        let steps_needed = ((abs + SLEW_STEP_MS.unsigned_abs() - 1) / SLEW_STEP_MS.unsigned_abs()) as usize;
        if steps_needed > SLEW_QUEUE_CAP {
            self.dropped += 1;
            return false;
        }
        self.n = steps_needed;
        let sign = if offset_ms < 0 { -1i64 } else { 1 };
        for i in 0..steps_needed {
            let remaining = abs - i as u64 * SLEW_STEP_MS.unsigned_abs();
            self.steps[i] = sign * remaining.min(SLEW_STEP_MS.unsigned_abs()) as i64;
        }
        true
    }

    /// 弹出一步（FIFO）。
    pub fn pop(&mut self) -> Option<i64> {
        if self.n == 0 {
            return None;
        }
        let v = self.steps[0];
        for i in 1..self.n {
            self.steps[i - 1] = self.steps[i];
        }
        self.n -= 1;
        Some(v)
    }

    /// 全部步之和 == 原偏差（拆分不丢量——守恒判据）。
    pub fn conserves_offset(&self) -> i64 {
        let mut s = 0i64;
        for &x in self.steps.iter().take(self.n) {
            s += x;
        }
        s
    }
}

/// 时间显示格式化：UTC ms → "HH:MM:SS"（模日换算，定宽）。
pub fn format_hms(utc_ms: u64) -> ([u8; 8], usize) {
    let secs = (utc_ms / 1_000) % 86_400;
    let h = secs / 3_600;
    let m = (secs % 3_600) / 60;
    let s = secs % 60;
    let mut out = [b'0'; 8];
    out[0] = b'0' + (h / 10) as u8;
    out[1] = b'0' + (h % 10) as u8;
    out[2] = b':';
    out[3] = b'0' + (m / 10) as u8;
    out[4] = b'0' + (m % 10) as u8;
    out[5] = b':';
    out[6] = b'0' + (s / 10) as u8;
    out[7] = b'0' + (s % 10) as u8;
    (out, 8)
}

/// 对账快照差异：双时钟各报一个时刻 → 差值分级（与主层 advise 线对齐）。
/// 返回 0=绿（<50ms）1=黄（<250ms）2=红（≥250ms）。
pub fn snapshot_delta_grade(utc_a_ms: u64, utc_b_ms: u64) -> u8 {
    let d = utc_a_ms.abs_diff(utc_b_ms);
    if d < 50 {
        0
    } else if d < 250 {
        1
    } else {
        2
    }
}

/// 闰年判定（显示层日期推进用——格里高利规则）。
pub fn is_leap_year(y: u32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// 当年天数。
pub fn days_in_year(y: u32) -> u32 {
    if is_leap_year(y) {
        366
    } else {
        365
    }
}

#[inline(never)]
pub fn run_duoclock_b7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F182-b7");

    // 1) RTT 账：3 样本 2 优 1 差 → 优良率 667‰、均值可算。
    let mut r = RttLedger::new();
    r.record(10);
    r.record(20);
    r.record(200);
    cs.add(
        "rtt_good_rate",
        r.good_permille() == Some(666) && r.mean_rtt() == Some(76) && r.n == 3,
        "",
    );

    // 2) 空账诚实：无样本 → 全 None（不编造质量）。
    let r2 = RttLedger::new();
    cs.add(
        "rtt_empty_honest",
        r2.good_permille().is_none() && r2.mean_rtt().is_none() && r2.usable().is_none(),
        "",
    );

    // 3) 准入线：高优良+低超时 = 可用；高超时 = 不可用（双门同查）。
    let mut r3 = RttLedger::new();
    r3.record(10);
    r3.record(10);
    r3.record(10);
    r3.record(10);
    r3.on_timeout();
    let mut r4 = RttLedger::new();
    r4.record(500);
    r4.record(500);
    r4.record(500);
    r4.on_timeout();
    r4.on_timeout();
    cs.add(
        "rtt_usability_two_gates",
        r3.usable() == Some(true) && r4.usable() == Some(false),
        "",
    );

    // 4) 环回卷：10 样本 > 8 容量 → 只留最近 8（均值随新样本走）。
    let mut r5 = RttLedger::new();
    for i in 0..10u32 {
        r5.record(i * 100);
    }
    // 最近 8 个：200..900 → 均 550。
    cs.add("rtt_ring_wraps", r5.n == RTT_CAP && r5.mean_rtt() == Some(550), "");

    // 5) slew 拆分：350ms → 4 步（100×3+50）、步和守恒 == 350。
    let mut q = SlewQueue::new();
    let planned = q.plan(350);
    cs.add(
        "slew_split_conserves",
        planned && q.n == 4 && q.conserves_offset() == 350,
        "",
    );

    // 6) slew 负方向：-230ms → 3 步、和 == -230（方向随符号不丢号）。
    let mut q2 = SlewQueue::new();
    q2.plan(-230);
    cs.add("slew_negative_direction", q2.n == 3 && q2.conserves_offset() == -230, "");

    // 7) slew 弹出 FIFO：先弹的是最大步（100），最后余 50（顺序即节奏）。
    let mut q3 = SlewQueue::new();
    q3.plan(250);
    let s1 = q3.pop();
    let s2 = q3.pop();
    let s3 = q3.pop();
    let s4 = q3.pop();
    cs.add(
        "slew_fifo_pop",
        s1 == Some(100) && s2 == Some(100) && s3 == Some(50) && s4.is_none(),
        "",
    );

    // 8) slew 超容拒：>800ms 拆不完 8 步 → 拒 + dropped 计数（大偏移交 step 语义）。
    let mut q4 = SlewQueue::new();
    cs.add("slew_overflow_rejects", !q4.plan(1_000) && q4.dropped == 1, "");

    // 9) HH:MM:SS 格式化：0 → 00:00:00、正午 → 12:00:00、跨日取模。
    let (a, la) = format_hms(0);
    let (b, lb) = format_hms(12 * 3_600 * 1_000);
    let (c, lc) = format_hms(86_400 * 1_000 + 3_723_000); // 次日 01:02:03
    cs.add(
        "hms_format",
        la == 8 && &a[..] == b"00:00:00" && &b[..] == b"12:00:00" && lb == 8 && &c[..] == b"01:02:03" && lc == 8,
        "",
    );

    // 10) 快照差异三级：40ms 绿、100ms 黄、1s 红（线值逐点）。
    cs.add(
        "snapshot_delta_grades",
        snapshot_delta_grade(1_000, 1_040) == 0
            && snapshot_delta_grade(1_000, 1_100) == 1
            && snapshot_delta_grade(1_000, 2_000) == 2,
        "",
    );

    // 11) 闰年规则三点：普通四年闰、百年不闰、四百年闰。
    cs.add(
        "leap_rules",
        is_leap_year(2024) && !is_leap_year(1900) && is_leap_year(2000) && days_in_year(2024) == 366 && days_in_year(2023) == 365,
        "",
    );

    // 12) 常量自洽：优良线 50ms、步长 100ms、环 8（与主层 slew/advise 同族量级）。
    cs.add(
        "b7_constants",
        RTT_GOOD_MS == 50 && SLEW_STEP_MS == 100 && RTT_CAP == 8,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slew_zero_offset_no_steps() {
        // 零偏差零步（无事不扰——不生成空转队列）。
        let mut q = SlewQueue::new();
        assert!(q.plan(0));
        assert_eq!(q.n, 0);
        assert!(q.pop().is_none());
    }

    #[test]
    fn rtt_all_timeouts_unusable() {
        // 全超时：n=0 优良率 None → usable None（不是 false——没数据不下结论）。
        let mut r = RttLedger::new();
        r.on_timeout();
        r.on_timeout();
        assert!(r.usable().is_none());
        assert_eq!(r.timeouts(), 2);
    }

    #[test]
    fn hms_day_boundary_chain() {
        // 一天逐小时抽检：每小时整点格式正确（24 点抽样扫）。
        for h in 0..24u64 {
            let (buf, _) = format_hms(h * 3_600_000);
            let expect_h = format!("{:02}", h);
            assert_eq!(&buf[..2], expect_h.as_bytes());
            assert_eq!(&buf[2..3], b":");
        }
    }
}
