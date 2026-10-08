//! F172 自检可视化 · 批次八（v8）——套件轮换调度、结果保留策略、
//! 历史趋势账、可视化色板语义帧。零堆、no_std。

use crate::checks::CheckSet;

/// 套件数。
pub const SUITES: usize = 4;
/// 轮换周期（次——每 N 次会话轮换重点套件）。
pub const ROTATION_PERIOD: usize = 3;
/// 历史保留次数。
pub const HISTORY_CAP: usize = 12;
/// 色板帧长（10B）。
pub const PALETTE_FRAME_LEN: usize = 10;

/// 套件轮换调度：重点套件按会话序轮换（0→1→2→3→0…），
/// 其余套件降频抽查（每轮换周期抽 1 次）。
pub fn focus_suite(session_index: usize) -> usize {
    session_index % SUITES
}

/// 套件是否本会话必跑：重点套件必跑；其余每 ROTATION_PERIOD 次抽一次。
pub fn must_run(session_index: usize, suite: usize) -> bool {
    if suite >= SUITES {
        return false;
    }
    if suite == focus_suite(session_index) {
        return true;
    }
    session_index % ROTATION_PERIOD == 0 // 抽查拍点
}

/// 结果保留策略：失败结果优先保留（败因是证据），通过结果采样保留。
/// 环满 → 优先驱逐最旧的通过记录（败记录留到最后）。
#[derive(Clone, Copy)]
pub struct RetentionPolicy {
    passed: [bool; HISTORY_CAP],
    occupied: [bool; HISTORY_CAP],
    pub n: usize,
    pub evicted_passed: u32,
    pub evicted_failed: u32,
}

impl RetentionPolicy {
    pub const fn new() -> RetentionPolicy {
        RetentionPolicy { passed: [false; HISTORY_CAP], occupied: [false; HISTORY_CAP], n: 0, evicted_passed: 0, evicted_failed: 0 }
    }

    /// 记录一次结果（满容时按策略驱逐）。
    pub fn record(&mut self, passed: bool) {
        if self.n < HISTORY_CAP {
            self.passed[self.n] = passed;
            self.occupied[self.n] = true;
            self.n += 1;
            return;
        }
        // 满：找最旧的通过记录驱逐；全是失败 → 驱逐最旧失败（环必须动）。
        let mut victim = None;
        for i in 0..HISTORY_CAP {
            if self.occupied[i] && self.passed[i] {
                victim = Some(i);
                break;
            }
        }
        let v = victim.unwrap_or(0);
        if self.passed[v] {
            self.evicted_passed += 1;
        } else {
            self.evicted_failed += 1;
        }
        self.passed[v] = passed;
    }

    /// 失败留存数。
    pub fn failed_kept(&self) -> u32 {
        let mut c = 0;
        for i in 0..HISTORY_CAP {
            if self.occupied[i] && !self.passed[i] {
                c += 1;
            }
        }
        c
    }

    /// 留存总数（账面口径）。
    pub fn total_kept(&self) -> u32 {
        self.passed_kept() + self.failed_kept()
    }

    pub fn passed_kept(&self) -> u32 {
        let mut c = 0;
        for i in 0..HISTORY_CAP {
            if self.occupied[i] && self.passed[i] {
                c += 1;
            }
        }
        c
    }
}

/// 历史趋势账：最近 N 次通过率序列（‰），下滑检测（连续 3 次下滑）。
#[derive(Clone, Copy)]
pub struct TrendLedger {
    rates: [u32; HISTORY_CAP],
    pub n: usize,
}

impl TrendLedger {
    pub const fn new() -> TrendLedger {
        TrendLedger { rates: [0; HISTORY_CAP], n: 0 }
    }

    pub fn sample(&mut self, pass_rate_permille: u32) {
        if self.n < HISTORY_CAP {
            self.rates[self.n] = pass_rate_permille;
            self.n += 1;
        } else {
            for i in 1..HISTORY_CAP {
                self.rates[i - 1] = self.rates[i];
            }
            self.rates[HISTORY_CAP - 1] = pass_rate_permille;
        }
    }

    /// 连续下滑（最近 3 个采样严格递减）→ true；不足 3 → None。
    pub fn declining(&self) -> Option<bool> {
        if self.n < 3 {
            return None;
        }
        let a = self.rates[self.n - 3] as i64;
        let b = self.rates[self.n - 2] as i64;
        let c = self.rates[self.n - 1] as i64;
        Some(a > b && b > c)
    }

    /// 最新通过率。
    pub fn latest(&self) -> Option<u32> {
        if self.n == 0 {
            return None;
        }
        Some(self.rates[self.n - 1])
    }
}

/// 可视化色板语义帧（10B）：
/// [0..2) "VP" · [2..4) 主题版本 LE · [4..6) 语义色数 LE ·
/// [6..8) 对比度最低对差 ‰ LE · [8..10) 校验和（前 8B FNV-16）。
pub fn fnv16(data: &[u8]) -> u16 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    }
    (h & 0xFFFF) as u16
}

pub fn encode_palette(theme_ver: u16, semantic_colors: u16, min_contrast_permille: u16, out: &mut [u8; PALETTE_FRAME_LEN]) -> bool {
    if min_contrast_permille < 300 {
        return false; // 对比度低于 300‰ = 不可读（色板不能出厂）
    }
    out[0] = b'V';
    out[1] = b'P';
    out[2..4].copy_from_slice(&theme_ver.to_le_bytes());
    out[4..6].copy_from_slice(&semantic_colors.to_le_bytes());
    out[6..8].copy_from_slice(&min_contrast_permille.to_le_bytes());
    let c = fnv16(&out[..8]);
    out[8] = (c & 0xFF) as u8;
    out[9] = (c >> 8) as u8;
    true
}

pub fn decode_palette(frame: &[u8; PALETTE_FRAME_LEN]) -> Option<(u16, u16, u16)> {
    if frame[0] != b'V' || frame[1] != b'P' {
        return None;
    }
    let want = (frame[9] as u16) << 8 | frame[8] as u16;
    if fnv16(&frame[..8]) != want {
        return None;
    }
    Some((
        u16::from_le_bytes(frame[2..4].try_into().ok()?),
        u16::from_le_bytes(frame[4..6].try_into().ok()?),
        u16::from_le_bytes(frame[6..8].try_into().ok()?),
    ))
}

#[inline(never)]
pub fn run_selftestviz_b8_checks() -> CheckSet {
    let mut cs = CheckSet::new("F172-b8");

    // 1) 轮换：会话 0..5 重点套件 0,1,2,3,0,1（周期 4 循环）。
    cs.add(
        "rotation_cycle",
        focus_suite(0) == 0 && focus_suite(1) == 1 && focus_suite(3) == 3 && focus_suite(4) == 0 && focus_suite(5) == 1,
        "",
    );

    // 2) 必跑判定：重点必跑；非重点在 session%3==0 抽查拍点必跑。
    cs.add(
        "must_run_two_paths",
        must_run(1, 1)
            && !must_run(1, 0)
            && must_run(0, 2)
            && must_run(3, 3)
            && must_run(3, 0) // 抽查拍点：session 3 % 3 == 0
            && !must_run(4, 1)
            && must_run(6, 1),
        "",
    );

    // 3) 越界套件拒：suite 9 → 不跑（调度面守门）。
    cs.add("must_run_oob_false", !must_run(0, 9), "");

    // 4) 保留策略：先 10 过 + 2 败，再录 1 过 → 挤掉最旧通过（败全留）。
    let mut p = RetentionPolicy::new();
    for _ in 0..10 {
        p.record(true);
    }
    p.record(false);
    p.record(false);
    p.record(true);
    cs.add(
        "retention_evicts_oldest_pass",
        p.evicted_passed == 1 && p.evicted_failed == 0 && p.failed_kept() == 2,
        "",
    );

    // 5) 全败环：全失败时新记录挤最旧失败（环必须动——不留死数据）。
    let mut p2 = RetentionPolicy::new();
    for _ in 0..HISTORY_CAP {
        p2.record(false);
    }
    p2.record(false);
    cs.add("retention_all_failed_still_moves", p2.evicted_failed == 1 && p2.failed_kept() == HISTORY_CAP as u32, "");

    // 6) 保留统计：通过留存 = 挤后余量（账面口径）。
    cs.add(
        "retention_counts",
        p.passed_kept() == 10 && p.n == HISTORY_CAP,
        "",
    );

    // 7) 趋势下滑：900→700→500 连续 3 降 → true；平稳 → false。
    let mut t = TrendLedger::new();
    t.sample(900);
    t.sample(700);
    t.sample(500);
    let mut t2 = TrendLedger::new();
    t2.sample(800);
    t2.sample(800);
    t2.sample(800);
    cs.add(
        "trend_decline_detection",
        t.declining() == Some(true) && t2.declining() == Some(false),
        "",
    );

    // 8) 趋势不足 3 不猜：2 采样 → None（空账同）。
    let mut t3 = TrendLedger::new();
    t3.sample(500);
    t3.sample(400);
    cs.add(
        "trend_insufficient_honest",
        t3.declining().is_none() && TrendLedger::new().latest().is_none(),
        "",
    );

    // 9) 趋势环回卷：12 满后再采 → 最旧出窗、latest 恒新。
    let mut t4 = TrendLedger::new();
    for i in 0..15u32 {
        t4.sample(i * 100);
    }
    cs.add("trend_window_wraps", t4.n == HISTORY_CAP && t4.latest() == Some(1_400), "");

    // 10) 色板帧 round-trip：合法主题往返一致。
    let mut f = [0u8; PALETTE_FRAME_LEN];
    let ok = encode_palette(3, 6, 700, &mut f);
    cs.add("palette_roundtrip", ok && decode_palette(&f) == Some((3, 6, 700)) && f[0] == b'V', "");

    // 11) 色板出厂门：对比度 <300‰ 拒（不可读色板不出厂）+ 撕裂拒。
    let mut f2 = [0u8; PALETTE_FRAME_LEN];
    let unreadable = !encode_palette(3, 6, 299, &mut f2);
    let mut tear_ok = true;
    for i in 0..PALETTE_FRAME_LEN {
        let mut t = f;
        t[i] ^= 0x49;
        if decode_palette(&t).is_some() {
            tear_ok = false;
        }
    }
    cs.add("palette_gates", unreadable && tear_ok, "");

    // 12) 常量自洽：套件 4、轮换 3、历史 12。
    cs.add("b8_constants", SUITES == 4 && ROTATION_PERIOD == 3 && HISTORY_CAP == 12, "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_covers_all_suites() {
        // 一个轮换周期内四套件各当一次重点（调度公平性不变量）。
        let mut seen = [false; SUITES];
        for s in 0..SUITES {
            seen[focus_suite(s)] = true;
        }
        assert!(seen.iter().all(|x| *x));
    }

    #[test]
    fn retention_mixed_stream() {
        // 混合流压力：过败交错 30 条 → 败全留、过被挤到只剩预算内。
        let mut p = RetentionPolicy::new();
        for i in 0..30u32 {
            p.record(i % 5 == 4);
        }
        assert!(p.failed_kept() >= 4);
        assert_eq!(p.n, HISTORY_CAP);
        assert!(p.total_kept() == HISTORY_CAP as u32);
    }

    #[test]
    fn trend_decline_with_plateau_not_declining() {
        // 平台期打断下滑：900→700→700→500 → 最近 3 个非严格递减 → false。
        let mut t = TrendLedger::new();
        t.sample(900);
        t.sample(700);
        t.sample(700);
        t.sample(500);
        assert_eq!(t.declining(), Some(false));
    }
}
