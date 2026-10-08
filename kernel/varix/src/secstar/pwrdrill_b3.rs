//! F180 断电演练自动化 · 批次三深化（secstar · G-G-10）。
//!
//! 批次三功能面（主册判据「4 周 400 轮零漏跑 / 双盲一致 / 三态覆盖」纵深）：
//! - [`MissDetector`]：漏跑检测器——期望夜数 vs 实际夜账对拍（零漏跑
//!   判据的执行面：漏一夜立即可见，不等月末对账）；
//! - [`StreakStats`]：连绿统计——当前连绿/最长连绿/总绿率三数（周报
//!   WeekRow 的趋势消费面）；
//! - [`ConfigFrame`]：调度配置持久帧——夜窗起点/轮数/使能位带校验和
//!   encode/decode（撕裂拒收——配置坏了不如实跑错了）；
//! - [`stagger_week_feasible`]：整周错峰检查——夜窗与 F061 性能检查器
//!   七天逐日无交叠（错峰不是只查一天）。
//!
//! 零堆纪律：定长帧 + 定长账，无 alloc。

use super::pwrdrill::{WeekRow, NIGHT_WINDOW_SPAN_MIN, NIGHT_WINDOW_START_MIN, TOTAL_ROUNDS_4W};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 漏跑检测器
// ---------------------------------------------------------------------------

/// 每夜账摘要（调度器的期望消费面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NightAttendance {
    pub night: u32,
    pub ran: bool,
}

/// 漏跑检测：给定 28 夜出勤账 → 漏跑夜列表（位图）+ 是否全勤。
pub struct MissDetector {
    expected_nights: u32,
    misses: [bool; 28],
}

impl MissDetector {
    pub const fn new(expected_nights: u32) -> MissDetector {
        MissDetector { expected_nights, misses: [false; 28] }
    }

    /// 对拍出勤账：漏夜置位。返回漏跑总数。
    pub fn audit(&mut self, rows: &[NightAttendance]) -> usize {
        let mut missed = 0;
        for i in 0..28 {
            let expect = (i as u32) < self.expected_nights;
            let ran = rows.iter().any(|r| r.night == i as u32 && r.ran);
            self.misses[i] = expect && !ran;
            if self.misses[i] {
                missed += 1;
            }
        }
        missed
    }

    /// 第 i 夜是否漏（定位——修复面要指哪修哪）。
    pub fn missed_at(&self, night: usize) -> bool {
        self.misses.get(night).copied().unwrap_or(false)
    }

    pub fn all_present(&self) -> bool {
        !self.misses.iter().any(|m| *m)
    }
}

// ---------------------------------------------------------------------------
// 连绿统计
// ---------------------------------------------------------------------------

/// 连绿三数：当前连绿 / 最长连绿 / 总绿率千分比。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreakStats {
    pub current: usize,
    pub longest: usize,
    pub green_permille: u32,
}

/// 从周行序列算连绿三数（WeekRow::is_red 是主层唯一真相源——不重造红绿）。
pub fn streak_stats(rows: &[WeekRow]) -> StreakStats {
    let mut cur = 0usize;
    let mut longest = 0usize;
    let mut green = 0usize;
    for r in rows {
        if !r.is_red() {
            cur += 1;
            green += 1;
            if cur > longest {
                longest = cur;
            }
        } else {
            cur = 0;
        }
    }
    let permille = if rows.is_empty() {
        0
    } else {
        (green * 1_000 / rows.len()) as u32
    };
    StreakStats { current: cur, longest, green_permille: permille }
}

// ---------------------------------------------------------------------------
// 调度配置持久帧（带校验和）
// ---------------------------------------------------------------------------

/// 配置帧：[0..2) 魔数 "VC" · [2] 夜窗起点分钟低 8 位 · [3] 起点高 8 位 ·
/// [4] 窗宽分钟 · [5] 使能位 · [6..8) 校验和（前 6 字节 FNV-16 折叠）。
pub const CONFIG_FRAME_LEN: usize = 8;

/// FNV-1a 折叠 16bit 校验和。
fn checksum16(data: &[u8]) -> u16 {
    let mut h: u32 = 0x811c9dc5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    ((h >> 16) ^ h) as u16
}

/// 编码配置帧（起点 1440 上限诚实拒——非法配置不出门）。
pub fn encode_config(start_min: u32, span_min: u32, enabled: bool, out: &mut [u8; CONFIG_FRAME_LEN]) -> bool {
    if start_min >= 1440 || span_min == 0 || span_min > 720 {
        return false;
    }
    out[0] = b'V';
    out[1] = b'C';
    out[2] = (start_min & 0xFF) as u8;
    out[3] = (start_min >> 8) as u8;
    out[4] = span_min as u8;
    out[5] = enabled as u8;
    let c = checksum16(&out[..6]);
    out[6] = (c & 0xFF) as u8;
    out[7] = (c >> 8) as u8;
    true
}

/// 解码配置帧：魔数/校验和/范围三关全过才放行（撕裂必拒）。
pub fn decode_config(frame: &[u8; CONFIG_FRAME_LEN]) -> Option<(u32, u32, bool)> {
    if frame[0] != b'V' || frame[1] != b'C' {
        return None;
    }
    let want = (frame[7] as u16) << 8 | frame[6] as u16;
    if checksum16(&frame[..6]) != want {
        return None;
    }
    let start = frame[2] as u32 | (frame[3] as u32) << 8;
    let span = frame[4] as u32;
    if start >= 1440 || span == 0 || span > 720 {
        return None;
    }
    Some((start, span, frame[5] != 0))
}

// ---------------------------------------------------------------------------
// 整周错峰检查（F061 扩展到七天）
// ---------------------------------------------------------------------------

/// F061 性能检查器日窗（起点分钟 + 窗宽分钟）——七天每天一窗。
pub type DailyWindow = (u32, u32);

/// 两窗是否交叠（模 1440 环形——跨日界窗也能判对）。
fn windows_overlap(a: DailyWindow, b: DailyWindow) -> bool {
    let norm = |s: u32, w: u32| (s % 1440, w);
    let (as_, aw) = norm(a.0, a.1);
    let (bs, bw) = norm(b.0, b.1);
    let overlap = |s1: u32, w1: u32, s2: u32, w2: u32| -> bool {
        let e1 = s1 + w1;
        let e2 = s2 + w2;
        // 环形判定：两窗不交叠 ⇔ 一窗终点 ≤ 另窗起点（含跨 0 点翻转）。
        if e1 <= 1440 && e2 <= 1440 {
            !(s2 >= e1 || s1 >= e2)
        } else {
            // 有窗跨 0 点：拆两段判
            let split = |s: u32, e: u32| -> [(u32, u32); 2] {
                if e > 1440 {
                    [(s, 1440 - s), (0, e - 1440)]
                } else {
                    [(s, e - s), (0, 0)]
                }
            };
            let p1 = split(s1, e1);
            let p2 = split(s2, e2);
            let mut hit = false;
            for (sa, wa) in p1 {
                for (sb, wb) in p2 {
                    if wa > 0 && wb > 0 {
                        let e_a = sa + wa;
                        let e_b = sb + wb;
                        if !(sb >= e_a || sa >= e_b) {
                            hit = true;
                        }
                    }
                }
            }
            hit
        }
    };
    overlap(as_, aw, bs, bw)
}

/// 夜窗与 F061 七窗逐日对拍：任一夜交叠 → 不可行（错峰全周成立才算）。
pub fn stagger_week_feasible(f061_windows: &[DailyWindow; 7]) -> bool {
    for day in 0..7 {
        let night_start = (NIGHT_WINDOW_START_MIN + day * 0) % 1440;
        if windows_overlap((night_start, NIGHT_WINDOW_SPAN_MIN), f061_windows[day as usize]) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// 批次三自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_pwrdrill_b3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F180-b3");

    // 1) 漏跑检测：全勤 28 夜 → 零漏（判据绿面）。
    let mut det = MissDetector::new(28);
    let mut rows = [NightAttendance { night: 0, ran: true }; 28];
    for (i, r) in rows.iter_mut().enumerate() {
        r.night = i as u32;
    }
    cs.add("attendance_full_green", det.audit(&rows) == 0 && det.all_present(), "");

    // 2) 漏跑检出与定位：第 5、9 夜缺席 → 恰好 2 漏且位可查。
    let mut det2 = MissDetector::new(28);
    let mut rows2 = [NightAttendance { night: 0, ran: true }; 28];
    for (i, r) in rows2.iter_mut().enumerate() {
        r.night = i as u32;
        r.ran = i != 5 && i != 9;
    }
    cs.add("attendance_two_misses_located", det2.audit(&rows2) == 2 && det2.missed_at(5) && det2.missed_at(9) && !det2.missed_at(6), "");

    // 3) 出勤账缺夜 = 漏（没记录不等于跑过——审计口径）。
    let mut det3 = MissDetector::new(4);
    let rows3 = [NightAttendance { night: 0, ran: true }, NightAttendance { night: 1, ran: true }];
    cs.add("attendance_missing_counts", det3.audit(&rows3) == 2, "");

    // 4) 连绿统计（真算）：绿绿红绿绿绿 六周 → 当前 3 / 最长 3 / 绿率 833‰。
    //   WeekRow 直接构造——is_red 是主层唯一红绿源（missed/aborted/恢复
    //   均时超线），本层不重造判定。红行=恢复均时 6s 超 5s 线。
    let mk = |week: u32, red: bool| WeekRow {
        week,
        planned: 700,
        done: 700,
        missed: 0,
        pass: 700,
        fail: 0,
        aborted_nights: 0,
        recover_mean_ms: if red { 6_000 } else { 5_000 },
        blind_match_permille: 1_000,
    };
    let rows4 = [mk(1, false), mk(2, false), mk(3, true), mk(4, false), mk(5, false), mk(6, false)];
    let stats = streak_stats(&rows4);
    cs.add(
        "streak_shape_real",
        stats.current == 3 && stats.longest == 3 && stats.green_permille == 833,
        "",
    );

    // 4b) 连绿收尾红：红在末位 → 当前连绿归零、最长仍记历史峰值。
    let rows4b = [mk(1, false), mk(2, false), mk(3, true)];
    let stats_b = streak_stats(&rows4b);
    cs.add("streak_tail_red", stats_b.current == 0 && stats_b.longest == 2, "");

    // 5) 配置帧 round-trip：合法配置编解码零失真。
    let mut frame = [0u8; CONFIG_FRAME_LEN];
    let enc = encode_config(NIGHT_WINDOW_START_MIN, NIGHT_WINDOW_SPAN_MIN, true, &mut frame);
    let dec = decode_config(&frame);
    cs.add(
        "config_roundtrip",
        enc && dec == Some((NIGHT_WINDOW_START_MIN, NIGHT_WINDOW_SPAN_MIN, true)),
        "",
    );

    // 6) 撕裂必拒：改一个字节 → 校验和关拦下（配置不被静默污染）。
    let mut torn = frame;
    torn[4] ^= 0x01;
    cs.add("config_tear_rejected", decode_config(&torn).is_none(), "");

    // 7) 非法配置诚实拒：起点 ≥1440 / 窗宽 0 / 窗宽 >720 不出门。
    let mut f2 = [0u8; CONFIG_FRAME_LEN];
    cs.add(
        "config_range_reject",
        !encode_config(1440, 60, true, &mut f2) && !encode_config(90, 0, true, &mut f2) && !encode_config(90, 721, true, &mut f2),
        "",
    );

    // 8) 使能位往返：false 也编解码（停跑是合法配置不是错误）。
    let mut f3 = [0u8; CONFIG_FRAME_LEN];
    encode_config(90, 180, false, &mut f3);
    cs.add("config_disabled_roundtrip", decode_config(&f3) == Some((90, 180, false)), "");

    // 9) 整周错峰：F061 白天窗与夜窗全周无交叠 → 可行。
    let f061_day: [DailyWindow; 7] = [(12 * 60, 30), (12 * 60, 30), (12 * 60, 30), (12 * 60, 30), (12 * 60, 30), (12 * 60, 30), (12 * 60, 30)];
    cs.add("stagger_week_ok", stagger_week_feasible(&f061_day), "");

    // 10) 整周错峰反面：任一天 F061 窗撞夜窗 → 全周不可行（逐日不放过）。
    let mut clash = f061_day;
    clash[3] = (NIGHT_WINDOW_START_MIN + 60, 30); // 第 4 天撞进夜窗
    cs.add("stagger_week_clash", !stagger_week_feasible(&clash), "");

    // 11) 主册常量贯通：夜窗 90 分起 180 分宽、400 轮总量一处一事实。
    cs.add("consts_aligned", NIGHT_WINDOW_START_MIN == 90 && NIGHT_WINDOW_SPAN_MIN == 180 && TOTAL_ROUNDS_4W == 400, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次三）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b3 {
    use super::*;

    #[test]
    fn miss_detector_never_forgives() {
        // 全空出勤账 = 全漏（缺记录不可原谅——审计不从宽）。
        let mut det = MissDetector::new(28);
        assert_eq!(det.audit(&[]), 28);
        assert!(!det.all_present());
    }

    #[test]
    fn overlap_ring_semantics() {
        // 环形窗：23:50 起 30 分窗 跨 0 点 vs 00:05 起 10 分窗 → 交叠。
        // 22:00-23:00 vs 00:05-00:15 → 不交叠。
        let a = windows_overlap((23 * 60 + 50, 30), (5, 10));
        let b = windows_overlap((22 * 60, 60), (5, 10));
        assert!(a, "跨 0 点环窗应判交叠");
        assert!(!b, "远离窗不应判交叠");
    }

    #[test]
    fn config_survives_all_byte_tears() {
        // 全字节撕裂扫描：任何单字节翻转都被校验和关拦（8 字节逐一）。
        let mut frame = [0u8; CONFIG_FRAME_LEN];
        assert!(encode_config(90, 180, true, &mut frame));
        for i in 0..CONFIG_FRAME_LEN {
            let mut torn = frame;
            torn[i] ^= 0x55;
            assert!(decode_config(&torn).is_none(), "byte {i} 撕裂漏检");
        }
    }
}
