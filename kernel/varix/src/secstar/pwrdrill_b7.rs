//! F180 断电演练 · 批次七深化（v7）——夜窗计算器、连败恢复建议、
//! 电池消耗模型、演练报告帧。零堆、no_std。

use crate::checks::CheckSet;

/// 夜窗起点（分钟——23:00）。
pub const NIGHT_START_MIN: u32 = 23 * 60;
/// 夜窗时长（分钟——跨越 0 点到 00:30）。
pub const NIGHT_SPAN_MIN: u32 = 90;
/// 连败升级线（连续失败夜数——3 败必人工）。
pub const STREAK_ESCALATE: u32 = 3;
/// 电池消耗模型容量 ‰ 基准。
pub const BATTERY_FULL_PERMILLE: u32 = 1_000;
/// 演练报告帧长（14B）。
pub const REPORT_FRAME_LEN: usize = 14;

/// 夜窗计算：绝对分钟 → 是否在夜窗内（跨 0 点环形判定）。
pub fn in_night_window(abs_min: u32) -> bool {
    let m = abs_min % 1_440;
    let end = (NIGHT_START_MIN + NIGHT_SPAN_MIN) % 1_440;
    if NIGHT_START_MIN + NIGHT_SPAN_MIN < 1_440 {
        m >= NIGHT_START_MIN && m < NIGHT_START_MIN + NIGHT_SPAN_MIN
    } else {
        m >= NIGHT_START_MIN || m < end
    }
}

/// 距夜窗开始还有多少分钟（已在内 → 0；用于「今晚几点跑」的倒计时）。
pub fn minutes_until_window(abs_min: u32) -> u32 {
    if in_night_window(abs_min) {
        return 0;
    }
    let m = abs_min % 1_440;
    let delta = (NIGHT_START_MIN + 1_440 - m) % 1_440;
    delta
}

/// 连败跟踪：连续失败夜数、达到升级线即触发（触发后清零重计——
/// 升级动作是一次性的，不是每晚重复）。
#[derive(Clone, Copy)]
pub struct StreakEscalator {
    streak: u32,
    pub escalations: u32,
}

impl StreakEscalator {
    pub const fn new() -> StreakEscalator {
        StreakEscalator { streak: 0, escalations: 0 }
    }

    pub fn on_night(&mut self, passed: bool) -> bool {
        if passed {
            self.streak = 0;
            return false;
        }
        self.streak += 1;
        if self.streak >= STREAK_ESCALATE {
            self.escalations += 1;
            self.streak = 0;
            return true;
        }
        false
    }

    pub fn current_streak(&self) -> u32 {
        self.streak
    }
}

/// 恢复建议生成：按连败长度给不同建议（人话三要素的「下一步」）。
/// 返回静态建议串（0=无败、1-2=观察、3+=人工介入）。
pub fn recovery_advice(current_streak: u32) -> &'static str {
    match current_streak {
        0 => "一切正常，无需处理",
        1 | 2 => "连败中——明晚自动重试，无需操作",
        _ => "连续失败已达升级线——请检查电源与盘健康",
    }
}

/// 电池消耗模型：演练夜耗电 = 基础 + 时长×功率；充满时间 = 余量/充电率。
/// 全部千分位（1_000 = 100%）。
#[derive(Clone, Copy)]
pub struct BatteryModel {
    /// 演练基础耗电 ‰（一次启动关机循环的固定开销）。
    pub base_permille: u32,
    /// 每分钟耗电 ‰（演练期间）。
    pub per_min_permille: u32,
    /// 充电每分钟恢复 ‰。
    pub charge_per_min_permille: u32,
}

impl BatteryModel {
    pub const DEFAULT: BatteryModel = BatteryModel { base_permille: 20, per_min_permille: 2, charge_per_min_permille: 5 };

    /// 一次 N 分钟演练后的余量（下限钳 0——电池不为负）。
    pub fn after_drill(&self, start_permille: u32, drill_min: u32) -> u32 {
        let cost = self.base_permille + self.per_min_permille.saturating_mul(drill_min);
        start_permille.saturating_sub(cost)
    }

    /// 演练前安全检查：余量足以完成演练且留 200‰ 底线（低电不演练——
    /// 演练是找病，不是制造事故）。
    pub fn safe_to_drill(&self, start_permille: u32, drill_min: u32) -> bool {
        let cost = self.base_permille + self.per_min_permille.saturating_mul(drill_min);
        start_permille >= cost + 200
    }

    /// 充满所需分钟（ceil——保守报长不报短）。
    pub fn minutes_to_full(&self, current_permille: u32) -> u32 {
        if current_permille >= BATTERY_FULL_PERMILLE {
            return 0;
        }
        let need = BATTERY_FULL_PERMILLE - current_permille;
        (need + self.charge_per_min_permille - 1) / self.charge_per_min_permille
    }
}

/// 演练报告帧（14B）：
/// [0..2) 魔数 "PD" · [2..4) 演练夜号 LE · [4..6) 恢复耗时 ms LE ·
/// [6..8) 检查项通过数 LE · [8..10) 检查项总数 LE · [10..12) 电池余量 ‰ LE ·
/// [12..14) 校验和（前 12B FNV-16）。
pub fn fnv16(data: &[u8]) -> u16 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    }
    (h & 0xFFFF) as u16
}

pub fn encode_report(night: u16, recover_ms: u16, passed: u16, total: u16, battery_permille: u16, out: &mut [u8; REPORT_FRAME_LEN]) -> bool {
    if total == 0 {
        return false; // 零检查项的演练不算演练
    }
    out[0] = b'P';
    out[1] = b'D';
    out[2..4].copy_from_slice(&night.to_le_bytes());
    out[4..6].copy_from_slice(&recover_ms.to_le_bytes());
    out[6..8].copy_from_slice(&passed.to_le_bytes());
    out[8..10].copy_from_slice(&total.to_le_bytes());
    out[10..12].copy_from_slice(&battery_permille.to_le_bytes());
    let c = fnv16(&out[..12]);
    out[12] = (c & 0xFF) as u8;
    out[13] = (c >> 8) as u8;
    true
}

pub fn decode_report(frame: &[u8; REPORT_FRAME_LEN]) -> Option<(u16, u16, u16, u16, u16)> {
    if frame[0] != b'P' || frame[1] != b'D' {
        return None;
    }
    let want = (frame[13] as u16) << 8 | frame[12] as u16;
    if fnv16(&frame[..12]) != want {
        return None;
    }
    Some((
        u16::from_le_bytes(frame[2..4].try_into().ok()?),
        u16::from_le_bytes(frame[4..6].try_into().ok()?),
        u16::from_le_bytes(frame[6..8].try_into().ok()?),
        u16::from_le_bytes(frame[8..10].try_into().ok()?),
        u16::from_le_bytes(frame[10..12].try_into().ok()?),
    ))
}

/// 报告通过率 ‰（u128 防溢出）。
pub fn pass_rate_permille(passed: u16, total: u16) -> u32 {
    if total == 0 {
        return 0;
    }
    ((passed as u128 * 1_000) / total as u128) as u32
}

#[inline(never)]
pub fn run_pwrdrill_b7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F180-b7");

    // 1) 夜窗环形判定：23:15 在窗内、00:15 在窗内（跨 0 点）、12:00 不在。
    cs.add(
        "night_window_wraps",
        in_night_window(23 * 60 + 15) && in_night_window(15) && !in_night_window(12 * 60),
        "",
    );

    // 2) 窗界恰点：23:00 起点在窗、00:30 终点不在（半开区间 [起, 止)）。
    cs.add(
        "night_window_half_open",
        in_night_window(NIGHT_START_MIN) && !in_night_window(30),
        "",
    );

    // 3) 倒计时：12:00 → 660 分钟到 23:00（下一窗）、23:15 窗内 → 0。
    cs.add(
        "night_countdown",
        minutes_until_window(12 * 60) == 660 && minutes_until_window(23 * 60 + 15) == 0,
        "",
    );

    // 4) 连败升级：败×3 → 恰好 1 次升级、streak 清零；过 1 绿再 3 败 → 第 2 次。
    let mut e = StreakEscalator::new();
    let e1 = e.on_night(false);
    let e2 = e.on_night(false);
    let e3 = e.on_night(false);
    let esc_after_three = e.escalations;
    let streak_after_three = e.current_streak();
    let after_green = e.on_night(true);
    let e4 = e.on_night(false);
    let e5 = e.on_night(false);
    let e6 = e.on_night(false);
    cs.add(
        "streak_escalate_at_three",
        !e1 && !e2 && e3 && esc_after_three == 1 && streak_after_three == 0 && !after_green && !e4 && !e5 && e6 && e.escalations == 2,
        "",
    );

    // 5) 升级是脉冲不是常亮：第 4 败（新 streak=1）不再触发。
    let mut e2b = StreakEscalator::new();
    e2b.on_night(false);
    e2b.on_night(false);
    assert!(e2b.on_night(false));
    let fourth = e2b.on_night(false);
    cs.add("streak_pulse_not_latch", !fourth && e2b.escalations == 1, "");

    // 6) 恢复建议三档：0 败、1 败、3+ 败各有人话（下一步不缺位）。
    cs.add(
        "advice_three_tiers",
        recovery_advice(0).len() > 0 && recovery_advice(1) != recovery_advice(0) && recovery_advice(3) != recovery_advice(1),
        "",
    );

    // 7) 电池模型：30 分钟演练从 900‰ → 900-20-60=820‰（算术直核）。
    let bm = BatteryModel::DEFAULT;
    cs.add("battery_after_drill", bm.after_drill(900, 30) == 820, "");

    // 8) 低电守门：余量不够覆盖演练+200 底线 → 禁演（演练不制造事故）。
    let cost30 = bm.base_permille + bm.per_min_permille * 30; // 80
    cs.add(
        "battery_safety_gate",
        !bm.safe_to_drill(cost30 + 199, 30) && bm.safe_to_drill(cost30 + 200, 30),
        "",
    );

    // 9) 演练钳 0：超长演练不产生负余量。
    cs.add("battery_never_negative", bm.after_drill(100, 10_000) == 0, "");

    // 10) 充满时间 ceil：900‰ → 100‰/5 = 20 分整；901 → ceil(100/5)=20（991‰→2分）。
    cs.add(
        "battery_charge_ceil",
        bm.minutes_to_full(900) == 20 && bm.minutes_to_full(991) == 2 && bm.minutes_to_full(1_000) == 0,
        "",
    );

    // 11) 报告帧 round-trip：五字段编解码一致。
    let mut f = [0u8; REPORT_FRAME_LEN];
    assert!(encode_report(12, 4_500, 9, 10, 820, &mut f));
    let dec = decode_report(&f).unwrap();
    cs.add(
        "report_frame_roundtrip",
        dec == (12, 4_500, 9, 10, 820) && f[0] == b'P' && f[1] == b'D',
        "",
    );

    // 12) 报告帧撕裂拒：单字节翻转 → 校验和拦（14 字节逐一）。
    let mut torn_ok = true;
    for i in 0..REPORT_FRAME_LEN {
        let mut t = f;
        t[i] ^= 0x5A;
        if decode_report(&t).is_some() {
            torn_ok = false;
        }
    }
    cs.add("report_frame_tear_proof", torn_ok, "");

    // 13) 零检查项拒编码：total=0 → false（零分母不进帧）。
    let mut f2 = [0u8; REPORT_FRAME_LEN];
    cs.add("report_zero_total_rejected", !encode_report(1, 1, 0, 0, 500, &mut f2), "");

    // 14) 通过率：9/10 = 900‰、0 分母保护 0（u128 不溢出）。
    cs.add(
        "report_pass_rate",
        pass_rate_permille(9, 10) == 900 && pass_rate_permille(0, 0) == 0 && pass_rate_permille(16_000, 16_000) == 1_000,
        "",
    );

    // 15) 常量自洽：升级线 3、夜窗跨 0 点（起+跨 > 1440）。
    cs.add(
        "b7_constants",
        STREAK_ESCALATE == 3 && NIGHT_START_MIN + NIGHT_SPAN_MIN > 1_440 && REPORT_FRAME_LEN == 14,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_edges_all_24h() {
        // 全天逐分钟扫：恰 90 分钟在窗内（半开区间长度守恒）。
        let mut count = 0;
        for m in 0..1_440u32 {
            if in_night_window(m) {
                count += 1;
            }
        }
        assert_eq!(count, NIGHT_SPAN_MIN, "窗长必须恰为 90 分钟");
    }

    #[test]
    fn escalation_long_failure_run() {
        // 连败 9 夜：恰 3 次升级（3/6/9 各一次——脉冲节奏验证）。
        let mut e = StreakEscalator::new();
        let mut hits = 0;
        for _ in 0..9 {
            if e.on_night(false) {
                hits += 1;
            }
        }
        assert_eq!(hits, 3);
        assert_eq!(e.escalations, 3);
    }

    #[test]
    fn report_frame_boundary_values() {
        // 边界值 round-trip：全 0 字段与全 0xFFFF 字段都诚实往返。
        let mut f = [0u8; REPORT_FRAME_LEN];
        assert!(encode_report(0, 0, 0, 1, 0, &mut f));
        assert_eq!(decode_report(&f), Some((0, 0, 0, 1, 0)));
        assert!(encode_report(0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, &mut f));
        assert_eq!(decode_report(&f), Some((0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF)));
    }
}
