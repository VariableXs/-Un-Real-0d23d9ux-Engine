//! F182 双域时钟同步 · 批次六深化（secstar · G-G-12）。
//!
//! 批次六功能面（与 b3/b4/b5 互补，本批管「会话账与漂移曲线」）：
//! - [`SyncSession`]：同步会话账——发起/完成/失败三结局 + 耗时
//!   （对表不是瞬间的：一次对表是一个有始有终的会话）；
//! - [`DriftChart`]：漂移曲线数据点——30 点滑动导出（趋势可视化的
//!   数据面：一串点，不是一句「有点偏」）；
//! - [`leap_second_mark`]：闰秒标记——23:59:60 语义占位（极UTC 时刻
//!   的正确定义：闰秒存在过这件事不许被时间轴洗掉）。
//!
//! 零堆纪律：定长环 + 定长点表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 同步会话账
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncOutcome {
    Completed,
    /// 源不可达（NTP 服务器超时）。
    SourceTimeout,
    /// 回执校验失败（帧坏/校验和不符）。
    FrameBad,
}

pub struct SyncSession {
    pub outcome: SyncOutcome,
    pub elapsed_ms: u64,
    pub frames_exchanged: u32,
}

/// 会话账（20 会话环）。
pub const SESSION_CAP: usize = 20;

pub struct SyncSessionLog {
    ring: [Option<SyncSession>; SESSION_CAP],
    head: usize,
    pub n: usize,
}

impl SyncSessionLog {
    pub const fn new() -> SyncSessionLog {
        SyncSessionLog { ring: [const { None }; SESSION_CAP], head: 0, n: 0 }
    }

    pub fn push(&mut self, s: SyncSession) {
        if self.n < SESSION_CAP {
            self.n += 1;
        }
        self.ring[self.head] = Some(s);
        self.head = (self.head + 1) % SESSION_CAP;
    }

    /// 成功率 ‰（Completed / 全部）。
    pub fn success_permille(&self) -> u32 {
        if self.n == 0 {
            return 0;
        }
        let ok = self.ring.iter().flatten().filter(|s| s.outcome == SyncOutcome::Completed).count();
        (ok * 1_000 / self.n) as u32
    }

    /// 失败原因分型计数（修复面要知道败在哪）。
    pub fn failure_counts(&self) -> (usize, usize) {
        let timeout = self.ring.iter().flatten().filter(|s| s.outcome == SyncOutcome::SourceTimeout).count();
        let frame = self.ring.iter().flatten().filter(|s| s.outcome == SyncOutcome::FrameBad).count();
        (timeout, frame)
    }

    /// 均耗时（仅成功会话——失败的耗时不算对表能力；两遍扫描零堆）。
    pub fn mean_completed_ms(&self) -> Option<u64> {
        let mut count = 0u64;
        let mut sum = 0u64;
        for s in self.ring.iter().flatten() {
            if s.outcome == SyncOutcome::Completed {
                count += 1;
                sum += s.elapsed_ms;
            }
        }
        if count == 0 {
            return None;
        }
        Some(sum / count)
    }
}

// ---------------------------------------------------------------------------
// 漂移曲线数据点
// ---------------------------------------------------------------------------

/// 点容量。
pub const DRIFT_POINTS: usize = 30;

pub struct DriftChart {
    points: [i64; DRIFT_POINTS], // 偏差 ms
    n: usize,
    head: usize,
}

impl DriftChart {
    pub const fn new() -> DriftChart {
        DriftChart { points: [0; DRIFT_POINTS], n: 0, head: 0 }
    }

    pub fn push(&mut self, offset_ms: i64) {
        self.points[self.head] = offset_ms;
        self.head = (self.head + 1) % DRIFT_POINTS;
        if self.n < DRIFT_POINTS {
            self.n += 1;
        }
    }

    pub fn point(&self, i: usize) -> Option<i64> {
        if i >= self.n {
            return None;
        }
        let start = if self.n == DRIFT_POINTS { self.head } else { 0 };
        Some(self.points[(start + i) % DRIFT_POINTS])
    }

    /// 窗内最大绝对偏差（曲线的包络）。
    pub fn envelope(&self) -> i64 {
        (0..self.n).map(|i| self.point(i).unwrap().abs()).max().unwrap_or(0)
    }

    /// 单调段判定：最近 5 点同向（持续走偏的早期信号）。
    pub fn trending(&self) -> Option<bool> {
        // Some(true)=走正 Some(false)=走负 None=无持续趋势。
        if self.n < 5 {
            return None;
        }
        let idx = |k: usize| -> i64 { self.point(self.n - 5 + k).unwrap() };
        let up = (0..4).all(|k| idx(k + 1) > idx(k));
        let down = (0..4).all(|k| idx(k + 1) < idx(k));
        if up {
            Some(true)
        } else if down {
            Some(false)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// 闰秒标记
// ---------------------------------------------------------------------------

/// 闰秒语义占位：23:59:60 是合法时刻标记（时间轴不留洞——存在过的
/// 秒不许被洗掉；本层只做标记面，处理面归 POSIX 层）。
pub fn is_leap_second(minute_of_day: u32, second: u8) -> bool {
    minute_of_day == 23 * 60 + 59 && second == 60
}

// ---------------------------------------------------------------------------
// 批次六自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_duoclock_b6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F182-b6");

    // 1) 会话账：三结局入环、成功率（8 成 2 败 = 800‰）。
    let mut l = SyncSessionLog::new();
    for i in 0..8u64 {
        l.push(SyncSession { outcome: SyncOutcome::Completed, elapsed_ms: 100 + i, frames_exchanged: 4 });
    }
    l.push(SyncSession { outcome: SyncOutcome::SourceTimeout, elapsed_ms: 5_000, frames_exchanged: 0 });
    l.push(SyncSession { outcome: SyncOutcome::FrameBad, elapsed_ms: 200, frames_exchanged: 2 });
    cs.add("session_success_rate", l.success_permille() == 800 && l.n == 10, "");

    // 2) 失败分型：超时 1 / 帧坏 1（修复面知道败在哪）。
    cs.add("session_failure_split", l.failure_counts() == (1, 1), "");

    // 3) 均耗时只算成功：成功均 103ms（失败耗时不冒充对表能力）。
    cs.add("session_mean_completed", l.mean_completed_ms() == Some(103), "");

    // 4) 空账诚实：0 会话 → 全零。
    let e = SyncSessionLog::new();
    cs.add("session_empty_honest", e.success_permille() == 0 && e.mean_completed_ms().is_none(), "");

    // 5) 曲线时序读取：推 35 点读最近 30（保序）。
    let mut c = DriftChart::new();
    for i in 0..35i64 {
        c.push(i);
    }
    cs.add("chart_time_order", c.point(0) == Some(5) && c.point(29) == Some(34), "");

    // 6) 包络：最大绝对偏差（±面）。
    let mut c2 = DriftChart::new();
    for v in [3i64, -7, 5, -12, 4] {
        c2.push(v);
    }
    cs.add("chart_envelope", c2.envelope() == 12, "");

    // 7) 趋势判定：连升 5 点 → 走正（持续走偏早期信号）。
    let mut c3 = DriftChart::new();
    for v in [0i64, 1, 2, 3, 4, 5] {
        c3.push(v);
    }
    cs.add("chart_trending_up", c3.trending() == Some(true), "");

    // 8) 无趋势：锯齿 → None（不把噪声当趋势）。
    let mut c4 = DriftChart::new();
    for v in [0i64, 3, 1, 4, 2, 5] {
        c4.push(v);
    }
    cs.add("chart_no_trend", c4.trending().is_none(), "");

    // 9) 闰秒标记：23:59:60 真、23:59:59 假、00:00:60 假（语义逐点）。
    cs.add(
        "leap_second",
        is_leap_second(23 * 60 + 59, 60) && !is_leap_second(23 * 60 + 59, 59) && !is_leap_second(0, 60),
        "",
    );

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次六）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b6 {
    use super::*;

    #[test]
    fn session_ring_wraps() {
        // 20 会话环回卷：第 21 个挤掉最老（账不膨胀）。
        let mut l = SyncSessionLog::new();
        for i in 0..(SESSION_CAP + 1) as u64 {
            l.push(SyncSession { outcome: SyncOutcome::Completed, elapsed_ms: i, frames_exchanged: 4 });
        }
        assert_eq!(l.n, SESSION_CAP);
        assert_eq!(l.mean_completed_ms(), Some(10)); // 最近 20 个均值 (1+20)/2
    }

    #[test]
    fn chart_trending_down() {
        // 连降 5 点 → 走负（校准起效的信号也是趋势）。
        let mut c = DriftChart::new();
        for v in [10i64, 8, 6, 4, 2, 0] {
            c.push(v);
        }
        assert_eq!(c.trending(), Some(false));
    }

    #[test]
    fn envelope_never_negative() {
        // 全零曲线包络 0（无偏差无包络）。
        let mut c = DriftChart::new();
        for _ in 0..10 {
            c.push(0);
        }
        assert_eq!(c.envelope(), 0);
    }
}
