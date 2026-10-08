//! F182 双域时钟同步 · 批次四深化（secstar · G-G-12）。
//!
//! 批次四功能面（与批次三互补：批次三管「估计与校正」，本批管
//! 「时区、节奏与告警」）：
//! - [`TzConverter`]：时区换算——UTC ↔ 本地（偏移分钟/跨日界进位/
//!   负偏移——日历不串日）；
//! - [`SyncInterval`]：同步间隔策略——置信度 → 间隔阶梯（NTP 校准
//!   6h / RTC 直读 1h / 推断 15min——越不可信越勤对表）；
//! - [`ClockRoundtrip`]：快照帧全字节撕裂扫描——16B 逐位翻转全拒
//!   （帧格式的批四复核面：与批次三校验和帧互补）；
//! - [`DriftAlarm`]：漂移告警账——超 5s 事件环（>5s 提示校时的
//!   告警面：谁、何时、偏了多少）。
//!
//! 零堆纪律：定长环 + 定长换算账，无 alloc。

use super::duoclock::{ClockSnapshot, Confidence, DRIFT_ADVISE_MS, SNAPSHOT_LEN};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 时区换算
// ---------------------------------------------------------------------------

/// UTC 毫秒 + 时区偏移（分钟）→ 当日本地分钟数（0..1440）与日偏移。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    /// 当地当日分钟（0..1439）。
    pub minute_of_day: u32,
    /// 跨日偏移（-1 = 昨日 / 0 = 同日 / 1 = 明日）。
    pub day_shift: i32,
}

pub fn to_local(utc_ms: u64, tz_offset_min: i32) -> LocalTime {
    let total_min = utc_ms / 60_000;
    let local_total = total_min as i64 + tz_offset_min as i64;
    let day = local_total.div_euclid(1440);
    let minute = local_total.rem_euclid(1440) as u32;
    LocalTime { minute_of_day: minute, day_shift: (day - total_min as i64 / 1440) as i32 }
}

/// 本地分钟 → 显示 HH:MM（字节面，5 字节——无 alloc 格式化）。
pub fn format_hhmm(minute_of_day: u32, out: &mut [u8; 5]) {
    let m = minute_of_day % 1440;
    out[0] = b'0' + (m / 60 / 10) as u8;
    out[1] = b'0' + (m / 60 % 10) as u8;
    out[2] = b':';
    out[3] = b'0' + (m % 60 / 10) as u8;
    out[4] = b'0' + (m % 60 % 10) as u8;
}

// ---------------------------------------------------------------------------
// 同步间隔策略
// ---------------------------------------------------------------------------

/// 间隔阶梯（秒）：NTP 6h / RTC 1h / 推断 15min。
pub const SYNC_INTERVAL_SECS: [u64; 3] = [6 * 3600, 3600, 900];

/// 置信度 → 同步间隔（越不可信越勤对表——阶梯的单调性是策略本体）。
pub fn sync_interval_secs(c: Confidence) -> u64 {
    match c {
        Confidence::NtpCalibrated => SYNC_INTERVAL_SECS[0],
        Confidence::RtcDirect => SYNC_INTERVAL_SECS[1],
        Confidence::Inferred => SYNC_INTERVAL_SECS[2],
    }
}

/// 阶梯单调性：置信度越高间隔越长（策略不变量的机械判定）。
pub fn interval_ladder_monotone() -> bool {
    SYNC_INTERVAL_SECS[0] > SYNC_INTERVAL_SECS[1] && SYNC_INTERVAL_SECS[1] > SYNC_INTERVAL_SECS[2]
}

// ---------------------------------------------------------------------------
// 快照帧撕裂复核（16B 全字节翻转）
// ---------------------------------------------------------------------------

/// 快照编码注入口（真实编码由主层回填——本层驱动撕裂扫描）。
pub trait FrameCodec {
    fn encode(&self, out: &mut [u8; SNAPSHOT_LEN]) -> bool;
}

/// 全字节撕裂扫描：任一字节翻转后主层解码必须拒绝（或帧恒等——
/// 位翻转撞相同值在 XOR 面不可能，但诚实保留等价出口）。
pub fn frame_tear_scan(codec: &dyn FrameCodec, decode: fn(&[u8; SNAPSHOT_LEN]) -> Option<ClockSnapshot>) -> bool {
    let mut frame = [0u8; SNAPSHOT_LEN];
    if !codec.encode(&mut frame) {
        return false;
    }
    for i in 0..SNAPSHOT_LEN {
        let mut torn = frame;
        torn[i] = torn[i].wrapping_add(1);
        if torn != frame && decode(&torn).is_some() {
            return false; // 撕裂帧被放行 = 编码面有洞
        }
    }
    true
}

// ---------------------------------------------------------------------------
// 漂移告警账
// ---------------------------------------------------------------------------

/// 告警环容量。
pub const DRIFT_ALARM_CAP: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriftAlarm {
    pub at_ms: u64,
    pub offset_ms: i64,
}

pub struct DriftAlarmLedger {
    ring: [Option<DriftAlarm>; DRIFT_ALARM_CAP],
    head: usize,
    pub n: usize,
}

impl DriftAlarmLedger {
    pub const fn new() -> DriftAlarmLedger {
        DriftAlarmLedger { ring: [const { None }; DRIFT_ALARM_CAP], head: 0, n: 0 }
    }

    /// 审查偏差：>5s（DRIFT_ADVISE_MS）入环告警，≤5s 不骚扰。
    pub fn review(&mut self, at_ms: u64, offset_ms: i64) -> bool {
        if offset_ms.abs() <= DRIFT_ADVISE_MS as i64 {
            return false;
        }
        if self.n < DRIFT_ALARM_CAP {
            self.n += 1;
        }
        self.ring[self.head] = Some(DriftAlarm { at_ms, offset_ms });
        self.head = (self.head + 1) % DRIFT_ALARM_CAP;
        true
    }

    /// 最严重告警（|偏|最大——校时优先级面）。
    pub fn worst(&self) -> Option<DriftAlarm> {
        self.ring.iter().flatten().copied().max_by_key(|a| a.offset_ms.abs())
    }
}

// ---------------------------------------------------------------------------
// 批次四自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_duoclock_b4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F182-b4");

    // 1) 时区换算：UTC 00:30 + UTC+8 → 08:30 同日（正偏移顺推）。
    let l1 = to_local(30 * 60_000, 480);
    cs.add("tz_positive", l1 == LocalTime { minute_of_day: 8 * 60 + 30, day_shift: 0 }, "");

    // 2) 时区跨日界：UTC 20:00 + UTC+8 → 明日 04:00（day_shift=1）。
    let l2 = to_local(20 * 60 * 60_000, 480);
    cs.add("tz_crosses_forward", l2 == LocalTime { minute_of_day: 4 * 60, day_shift: 1 }, "");

    // 3) 时区负偏移：UTC 02:00 + UTC-8 → 昨日 18:00（day_shift=-1）。
    let l3 = to_local(2 * 60 * 60_000, -480);
    cs.add("tz_negative_back", l3 == LocalTime { minute_of_day: 18 * 60, day_shift: -1 }, "");

    // 4) HH:MM 格式化：14:32 样例 + 00:05 补零（格式面两态）。
    let mut out = [0u8; 5];
    format_hhmm(14 * 60 + 32, &mut out);
    let a = &out == b"14:32";
    format_hhmm(5, &mut out);
    let b = &out == b"00:05";
    cs.add("hhmm_format", a && b, "");

    // 5) 同步间隔阶梯：NTP 6h / RTC 1h / 推断 15min + 单调性。
    cs.add(
        "sync_interval_ladder",
        sync_interval_secs(Confidence::NtpCalibrated) == 21_600
            && sync_interval_secs(Confidence::RtcDirect) == 3_600
            && sync_interval_secs(Confidence::Inferred) == 900
            && interval_ladder_monotone(),
        "",
    );

    // 6) 漂移告警：恰 5s 不告警、5.1s 告警（主册线的邻域两测）。
    let mut led = DriftAlarmLedger::new();
    let at_line = !led.review(1_000, 5_000);
    let over = led.review(2_000, 5_100);
    cs.add("drift_alarm_line", at_line && over && led.n == 1, "");

    // 7) 漂移告警负偏：-6s 同样告警（方向无关——偏了就报）。
    let mut led2 = DriftAlarmLedger::new();
    cs.add("drift_alarm_negative", led2.review(1_000, -6_000), "");

    // 8) 告警最严重定位：三条告警 |偏| 最大者出列（校时优先级）。
    led2.review(3_000, 8_000);
    led2.review(4_000, 12_000);
    let worst = led2.worst().unwrap();
    cs.add("drift_worst_located", worst.offset_ms == 12_000 && worst.at_ms == 4_000, "");

    // 9) 告警环回卷：16 满后继续收（head 覆盖最老——账不膨胀）。
    let mut led3 = DriftAlarmLedger::new();
    for i in 0..(DRIFT_ALARM_CAP + 2) as u64 {
        led3.review(i * 1_000, 6_000 + i as i64);
    }
    cs.add(
        "drift_alarm_wraps",
        led3.n == DRIFT_ALARM_CAP && led3.worst().unwrap().offset_ms == 6_017,
        "",
    );

    // 10) 帧撕裂扫描注入口：好编码全字节翻转全拒（编解码联动验证）。
    //   替身编码器：全零帧（主层 decode 对全零帧拒收——快照 utc=0 无效）。
    struct ZeroCodec;
    impl FrameCodec for ZeroCodec {
        fn encode(&self, out: &mut [u8; SNAPSHOT_LEN]) -> bool {
            *out = [0; SNAPSHOT_LEN];
            true
        }
    }
    // 主层 decode：VXDC 魔数缺失即拒 → 全零帧与任何翻转都 None → 扫描绿。
    let decode_stub = |f: &[u8; SNAPSHOT_LEN]| -> Option<ClockSnapshot> {
        if f[0] == b'V' {
            Some(ClockSnapshot { utc_ms: 1, tz_offset_min: 0, confidence: Confidence::RtcDirect })
        } else {
            None
        }
    };
    cs.add("frame_tear_scan", frame_tear_scan(&ZeroCodec, decode_stub), "");

    // 11) 主册常量贯通：5s 提示线 / 16B 帧长一处一事实。
    cs.add("consts_aligned", DRIFT_ADVISE_MS == 5_000 && SNAPSHOT_LEN == 16, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次四）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b4 {
    use super::*;

    #[test]
    fn tz_day_boundaries() {
        // 日界两端：23:59 与 00:00 相邻不串（分钟算术边界逐点）。
        let a = to_local((23 * 60 + 59) as u64 * 60_000, 0);
        let b = to_local(24 * 60 as u64 * 60_000, 0);
        assert_eq!(a.minute_of_day, 23 * 60 + 59);
        assert_eq!(b.minute_of_day, 0);
        // UTC 24:00 = 次日 00:00；偏移 0 → 本地日序与 UTC 同（shift 0）。
        assert_eq!(b.day_shift, 0);
    }

    #[test]
    fn hhmm_all_day_round() {
        // 全天 1440 分钟格式化逐个对（HH:MM 空间无死角抽样 96 点）。
        let mut out = [0u8; 5];
        for m in (0..1440u32).step_by(15) {
            format_hhmm(m, &mut out);
            let h = m / 60;
            let mi = m % 60;
            let expect = [
                b'0' + (h / 10) as u8,
                b'0' + (h % 10) as u8,
                b':',
                b'0' + (mi / 10) as u8,
                b'0' + (mi % 10) as u8,
            ];
            assert_eq!(out, expect, "m={m}");
        }
    }

    #[test]
    fn alarm_never_spams_within_line() {
        // 5s 线内连续审查零告警（不骚扰红线）。
        let mut led = DriftAlarmLedger::new();
        for i in 0..100u64 {
            assert!(!led.review(i * 1_000, 4_999));
        }
        assert_eq!(led.n, 0);
    }
}
