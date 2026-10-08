//! F180 断电演练自动化 · 批次五深化（secstar · G-G-10）。
//!
//! 批次五功能面（达成率 48%——主攻批次。与 b3「漏跑与配置」、b4
//! 「节奏与统计」互补，本批管「轮账与取证」）：
//! - [`RoundBitLedger`]：轮级账位图压缩——100 轮 × (三查 3bit + 恢复
//!   耗时分档 2bit) 每轮 1 字节（一夜账 100B——位面紧凑不失真）；
//! - [`FailForensics`]：失败轮取证——三查哪个坏 → 取证清单（fsck/
//!   蜂巢/引导链分道取现场）；
//! - [`ScheduleAdherence`]：排程遵从——计划时刻 vs 实际时刻偏差账
//!   （偏差 >10 分钟 = 迟跑留痕——夜窗纪律的可审计面）；
//! - [`RecoveryTrend`]：周恢复均时趋势——4 周均时序列 + 方向判定
//!   （变好/变平/变坏——趋势面给维护排期提供依据）。
//!
//! 零堆纪律：位图账 + 定长序列，无 alloc。

use super::pwrdrill::{NIGHT_WINDOW_SPAN_MIN, NIGHT_WINDOW_START_MIN, ROUNDS_PER_NIGHT};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 轮级账（位图压缩）
// ---------------------------------------------------------------------------

/// 恢复耗时分档（2bit）：0=<1s · 1=<3s · 2=<5s · 3=≥5s（超线档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecvBucket {
    Sub1s = 0,
    Sub3s = 1,
    Sub5s = 2,
    Over = 3,
}

pub fn recv_bucket(ms: u32) -> RecvBucket {
    if ms < 1_000 {
        RecvBucket::Sub1s
    } else if ms < 3_000 {
        RecvBucket::Sub3s
    } else if ms < 5_000 {
        RecvBucket::Sub5s
    } else {
        RecvBucket::Over
    }
}

/// 单轮账字节：bit0-2 = 三查坏位（fsck/蜂巢/引导链）· bit3-4 = 恢复档。
pub fn round_byte(fsck_bad: bool, hive_bad: bool, chain_bad: bool, recover_ms: u32) -> u8 {
    let mut b = 0u8;
    if fsck_bad {
        b |= 1;
    }
    if hive_bad {
        b |= 2;
    }
    if chain_bad {
        b |= 4;
    }
    b |= (recv_bucket(recover_ms) as u8) << 3;
    b
}

/// 解码：坏查数与恢复档（取证入口）。
pub fn round_decode(b: u8) -> (usize, RecvBucket) {
    let bad = (b & 0b111).count_ones() as usize;
    (bad, match (b >> 3) & 0b11 {
        0 => RecvBucket::Sub1s,
        1 => RecvBucket::Sub3s,
        2 => RecvBucket::Sub5s,
        _ => RecvBucket::Over,
    })
}

/// 一夜账：100 轮字节表。
pub type NightLedger = [u8; ROUNDS_PER_NIGHT];

/// 夜账坏轮数。
pub fn night_bad_rounds(ledger: &NightLedger) -> usize {
    ledger.iter().filter(|b| *b & 0b111 != 0).count()
}

// ---------------------------------------------------------------------------
// 失败轮取证
// ---------------------------------------------------------------------------

/// 取证清单行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForensicsRow {
    pub round: usize,
    pub fsck: bool,
    pub hive: bool,
    pub chain: bool,
}

/// 从夜账生成取证清单（只收坏轮——好轮不取证）。
pub fn forensics(ledger: &NightLedger, out: &mut [Option<ForensicsRow>; ROUNDS_PER_NIGHT]) -> usize {
    let mut k = 0;
    for (i, b) in ledger.iter().enumerate() {
        if *b & 0b111 != 0 {
            out[k] = Some(ForensicsRow { round: i, fsck: b & 1 != 0, hive: b & 2 != 0, chain: b & 4 != 0 });
            k += 1;
        }
    }
    k
}

/// 三查取证分道统计（哪个子系统最常出事——修复排期依据）。
pub fn forensics_tally(rows: &[Option<ForensicsRow>; ROUNDS_PER_NIGHT], k: usize) -> (usize, usize, usize) {
    let mut fsck = 0;
    let mut hive = 0;
    let mut chain = 0;
    for r in rows[..k].iter().flatten() {
        fsck += r.fsck as usize;
        hive += r.hive as usize;
        chain += r.chain as usize;
    }
    (fsck, hive, chain)
}

// ---------------------------------------------------------------------------
// 排程遵从
// ---------------------------------------------------------------------------

/// 迟跑判定：实际时刻超出计划时刻 10 分钟 → 迟跑留痕。
pub const LATE_THRESHOLD_MIN: u32 = 10;

pub fn is_late(planned_min: u32, actual_min: u32) -> bool {
    actual_min.saturating_sub(planned_min % 1440) > LATE_THRESHOLD_MIN
}

/// 夜窗合法性：计划时刻必须落在夜窗 [START, START+SPAN) 内（错峰纪律）。
pub fn in_night_window(planned_min: u32) -> bool {
    let m = planned_min % 1440;
    m >= NIGHT_WINDOW_START_MIN && m < NIGHT_WINDOW_START_MIN + NIGHT_WINDOW_SPAN_MIN
}

// ---------------------------------------------------------------------------
// 周恢复均时趋势
// ---------------------------------------------------------------------------

/// 趋势方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trend {
    Improving,
    Flat,
    Worsening,
}

/// 四周均时 → 方向（末周比首周低 >10% = 改善；高 >10% = 恶化；其余平）。
pub fn recovery_trend(weekly_means: &[u64; 4]) -> Trend {
    let first = weekly_means[0].max(1);
    let last = weekly_means[3];
    if last * 100 < first * 90 {
        Trend::Improving
    } else if last * 100 > first * 110 {
        Trend::Worsening
    } else {
        Trend::Flat
    }
}

// ---------------------------------------------------------------------------
// 批次五自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_pwrdrill_b5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F180-b5");

    // 1) 轮账编码：全绿 4.2s → 字节 0b10_000（档 2）逐位对。
    let b0 = round_byte(false, false, false, 4_200);
    cs.add("round_byte_green", b0 == 0b10_000 && round_decode(b0) == (0, RecvBucket::Sub5s), "");

    // 2) 轮账坏位：fsck+chain 坏、1.5s → bit0|bit2|档1（位面压缩保真）。
    let b1 = round_byte(true, false, true, 1_500);
    cs.add(
        "round_byte_bad_bits",
        b1 == 0b01_101 && round_decode(b1) == (2, RecvBucket::Sub3s),
        "",
    );

    // 3) 恢复分档界：999/1000/2999/3000/4999/5000 六点逐档（边界逐点）。
    cs.add(
        "recv_bucket_edges",
        recv_bucket(999) == RecvBucket::Sub1s
            && recv_bucket(1_000) == RecvBucket::Sub3s
            && recv_bucket(2_999) == RecvBucket::Sub3s
            && recv_bucket(3_000) == RecvBucket::Sub5s
            && recv_bucket(4_999) == RecvBucket::Sub5s
            && recv_bucket(5_000) == RecvBucket::Over,
        "",
    );

    // 4) 夜账坏轮数：3 坏轮混 97 好轮 → 恰 3（一夜口径）。
    let mut ledger: NightLedger = [0; ROUNDS_PER_NIGHT];
    ledger[10] = round_byte(true, false, false, 1_000);
    ledger[50] = round_byte(false, true, false, 800);
    ledger[99] = round_byte(false, false, true, 6_000);
    cs.add("night_bad_count", night_bad_rounds(&ledger) == 3, "");

    // 5) 取证清单：3 行各自坏位对（取证面逐轮定位）。
    let mut out = [None; ROUNDS_PER_NIGHT];
    let k = forensics(&ledger, &mut out);
    let r10 = out[0].unwrap();
    cs.add(
        "forensics_rows",
        k == 3 && r10.round == 10 && r10.fsck && !r10.hive && !r10.chain,
        "",
    );

    // 6) 取证分道统计：fsck 1 / hive 1 / chain 1（修复排期依据）。
    let (f, h, c) = forensics_tally(&out, k);
    cs.add("forensics_tally", (f, h, c) == (1, 1, 1), "");

    // 7) 迟跑判定：迟 11 分钟留痕、恰 10 分钟不冤（阈值邻域）。
    cs.add(
        "late_threshold",
        is_late(90, 101) && !is_late(90, 100) && !is_late(90, 90),
        "",
    );

    // 8) 夜窗合法性：90-269 在窗、270 起 / 89 及以前出窗（窗界逐点）。
    cs.add(
        "night_window_bounds",
        in_night_window(90) && in_night_window(269) && !in_night_window(270) && !in_night_window(89),
        "",
    );

    // 9) 周趋势三向：5000→4000 改善、5000→5200 平、5000→6000 恶化。
    cs.add(
        "recovery_trend_three",
        recovery_trend(&[5_000, 4_800, 4_500, 4_000]) == Trend::Improving
            && recovery_trend(&[5_000, 5_000, 5_100, 5_200]) == Trend::Flat
            && recovery_trend(&[5_000, 5_500, 5_800, 6_000]) == Trend::Worsening,
        "",
    );

    // 10) 趋势阈值界：恰好 ±10% = 平（边界不冤不纵）。
    cs.add(
        "trend_boundary",
        recovery_trend(&[5_000, 5_000, 5_000, 4_500]) == Trend::Flat
            && recovery_trend(&[5_000, 5_000, 5_000, 5_500]) == Trend::Flat,
        "",
    );

    // 11) 主册常量贯通：夜窗 90 分起 / 180 分宽 / 100 轮一处一事实。
    cs.add(
        "consts_aligned",
        NIGHT_WINDOW_START_MIN == 90 && NIGHT_WINDOW_SPAN_MIN == 180 && ROUNDS_PER_NIGHT == 100,
        "",
    );

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次五）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b5 {
    use super::*;

    #[test]
    fn night_ledger_compact_size() {
        // 一夜账体积：100 字节（位面压缩的体积承诺——比 JSON 小 30 倍）。
        assert_eq!(core::mem::size_of::<NightLedger>(), 100);
    }

    #[test]
    fn forensics_all_green_empty() {
        // 全绿夜：取证清单零行、分道全零（好夜不制造工作）。
        let ledger = [0u8; ROUNDS_PER_NIGHT];
        let mut out = [None; ROUNDS_PER_NIGHT];
        assert_eq!(forensics(&ledger, &mut out), 0);
        assert_eq!(forensics_tally(&out, 0), (0, 0, 0));
    }

    #[test]
    fn late_wraps_midnight() {
        // 跨 0 点夜：本层 is_late 做线性减——跨日场景由上层归一
        // （实际时刻换算成当日分钟后喂入），这里验证归一后的行为。
        // 计划 269 分（04:29）、实际次日 00:05 = 环形差 96 分钟 →
        // 上层归一喂 269+96=365 → 迟跑留痕。
        assert!(is_late(269, 269 + 96));
        assert!(!is_late(269, 269 + 5));
    }
}
