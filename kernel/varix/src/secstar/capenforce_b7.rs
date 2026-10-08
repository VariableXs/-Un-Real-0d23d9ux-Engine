//! F177 能力执法 · 批次七深化（v7）——规则版本账、按应用拒计数环、
//! 通知合并帧、执法覆盖率对账。零堆、no_std。

use crate::checks::CheckSet;

/// 规则版本账容量。
pub const RULE_VERSION_CAP: usize = 8;
/// 拒计数环容量。
pub const DENY_RING_CAP: usize = 24;
/// 通知合并窗（ms）。
pub const MERGE_WINDOW_MS: u32 = 30_000;
/// 合并帧长（12B）。
pub const MERGE_FRAME_LEN: usize = 12;
/// 执法点数（与主层对齐）。
pub const COVERAGE_POINTS: usize = 4;

/// 规则版本账：规则集版本单调递增、生效窗口留痕（哪天用哪版可考）。
#[derive(Clone, Copy)]
pub struct RuleVersionLedger {
    versions: [u32; RULE_VERSION_CAP],
    since_days: [u32; RULE_VERSION_CAP],
    pub n: usize,
}

impl RuleVersionLedger {
    pub const fn new() -> RuleVersionLedger {
        RuleVersionLedger { versions: [0; RULE_VERSION_CAP], since_days: [0; RULE_VERSION_CAP], n: 0 }
    }

    /// 发布新版本：必须高于当前版（回退 = 拒——规则只进不退）。
    pub fn publish(&mut self, version: u32, day: u32) -> bool {
        if self.n > 0 && version <= self.versions[self.n - 1] {
            return false;
        }
        if self.n >= RULE_VERSION_CAP {
            return false;
        }
        self.versions[self.n] = version;
        self.since_days[self.n] = day;
        self.n += 1;
        true
    }

    /// 某天生效的版本（since ≤ day 的最后一条）。
    pub fn version_at(&self, day: u32) -> Option<u32> {
        if self.n == 0 {
            return None;
        }
        let mut v = self.versions[0];
        for i in 0..self.n {
            if self.since_days[i] <= day {
                v = self.versions[i];
            }
        }
        Some(v)
    }

    pub fn latest(&self) -> Option<u32> {
        self.versions.get(self.n.checked_sub(1).unwrap_or(0)).copied()
    }
}

/// 按应用拒计数环：环形记 (app_id, day)，回放窗口统计。
#[derive(Clone, Copy)]
pub struct DenyRing {
    app_ids: [u32; DENY_RING_CAP],
    days: [u32; DENY_RING_CAP],
    head: usize,
    pub n: usize,
}

impl DenyRing {
    pub const fn new() -> DenyRing {
        DenyRing { app_ids: [0; DENY_RING_CAP], days: [0; DENY_RING_CAP], head: 0, n: 0 }
    }

    pub fn record(&mut self, app_id: u32, day: u32) {
        if self.n < DENY_RING_CAP {
            self.app_ids[self.head] = app_id;
            self.days[self.head] = day;
            self.head = (self.head + 1) % DENY_RING_CAP;
            self.n += 1;
        } else {
            self.app_ids[self.head] = app_id;
            self.days[self.head] = day;
            self.head = (self.head + 1) % DENY_RING_CAP;
        }
    }

    /// 某应用最近 N 天拒数（窗口 [day-N+1, day]）。
    pub fn denies_in(&self, app_id: u32, day: u32, window: u32) -> u32 {
        let mut c = 0;
        for i in 0..self.n {
            if self.app_ids[i] == app_id {
                let d = self.days[i];
                if d <= day && day.saturating_sub(d) < window {
                    c += 1;
                }
            }
        }
        c
    }

    /// 环内总拒数（对账面）。
    pub fn total(&self) -> u32 {
        self.n as u32
    }
}

/// 通知合并器：同应用窗内多次执法合并一条（骚扰面的最后防线）。
#[derive(Clone, Copy)]
pub struct NotifyMerger {
    app_ids: [Option<u32>; DENY_RING_CAP],
    counts: [u32; DENY_RING_CAP],
    last_ms: [u32; DENY_RING_CAP],
    pub flushed: u32,
}

impl NotifyMerger {
    pub const fn new() -> NotifyMerger {
        NotifyMerger { app_ids: [None; DENY_RING_CAP], counts: [0; DENY_RING_CAP], last_ms: [0; DENY_RING_CAP], flushed: 0 }
    }

    fn slot_of(&self, app_id: u32) -> Option<usize> {
        (0..DENY_RING_CAP).find(|&i| self.app_ids[i] == Some(app_id))
    }

    /// 执法事件入账：新应用开槽计 1；已有 → 窗内累加、窗外冲账 +1 flushed。
    /// 返回 Some(当前累计数) = 应展示的合并计数；窗外 = Some(1)。
    pub fn on_enforce(&mut self, app_id: u32, now_ms: u32) -> u32 {
        if let Some(i) = self.slot_of(app_id) {
            if now_ms.saturating_sub(self.last_ms[i]) < MERGE_WINDOW_MS {
                self.counts[i] += 1;
                self.last_ms[i] = now_ms;
                return self.counts[i];
            }
            // 窗外：旧账冲出（flushed），重开 1。
            self.flushed += 1;
            self.counts[i] = 1;
            self.last_ms[i] = now_ms;
            return 1;
        }
        // 新槽：找空位；满则驱逐累计最小（合并优先保高频骚扰源）。
        let slot = match (0..DENY_RING_CAP).find(|&i| self.app_ids[i].is_none()) {
            Some(i) => i,
            None => {
                let mut min = 0;
                for i in 1..DENY_RING_CAP {
                    if self.counts[i] < self.counts[min] {
                        min = i;
                    }
                }
                self.flushed += 1;
                min
            }
        };
        self.app_ids[slot] = Some(app_id);
        self.counts[slot] = 1;
        self.last_ms[slot] = now_ms;
        1
    }

    /// 冲账：把所有累计 ≥2 的槽 flush 出去（窗口到期统一上报）。
    pub fn flush_all(&mut self) -> u32 {
        let mut out = 0;
        for i in 0..DENY_RING_CAP {
            if self.app_ids[i].is_some() && self.counts[i] >= 2 {
                out += self.counts[i];
                self.counts[i] = 0;
                self.app_ids[i] = None;
                self.flushed += 1;
            }
        }
        out
    }
}

/// 执法覆盖率：四执法点各最近拦截数 → 覆盖 ‰（有点拦截的点/总点）。
pub fn coverage_permille(active_points: &[bool; COVERAGE_POINTS]) -> u32 {
    let mut on = 0;
    for &a in active_points.iter() {
        if a {
            on += 1;
        }
    }
    on * 1_000 / COVERAGE_POINTS as u32
}

/// 通知合并帧（12B）：
/// [0..2) "CE" · [2..4) 应用 id LE · [4..6) 合并计数 LE · [6..8) 窗口 ms/1000 LE ·
/// [8..10) 严重度(0..2) LE · [10..12) 校验和（前 10B FNV-16）。
pub fn fnv16(data: &[u8]) -> u16 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    }
    (h & 0xFFFF) as u16
}

pub fn encode_merge(app_id: u16, count: u16, window_s: u16, severity: u16, out: &mut [u8; MERGE_FRAME_LEN]) -> bool {
    if severity > 2 {
        return false;
    }
    out[0] = b'C';
    out[1] = b'E';
    out[2..4].copy_from_slice(&app_id.to_le_bytes());
    out[4..6].copy_from_slice(&count.to_le_bytes());
    out[6..8].copy_from_slice(&window_s.to_le_bytes());
    out[8..10].copy_from_slice(&severity.to_le_bytes());
    let c = fnv16(&out[..10]);
    out[10] = (c & 0xFF) as u8;
    out[11] = (c >> 8) as u8;
    true
}

pub fn decode_merge(frame: &[u8; MERGE_FRAME_LEN]) -> Option<(u16, u16, u16, u16)> {
    if frame[0] != b'C' || frame[1] != b'E' {
        return None;
    }
    let want = (frame[11] as u16) << 8 | frame[10] as u16;
    if fnv16(&frame[..10]) != want {
        return None;
    }
    Some((
        u16::from_le_bytes(frame[2..4].try_into().ok()?),
        u16::from_le_bytes(frame[4..6].try_into().ok()?),
        u16::from_le_bytes(frame[6..8].try_into().ok()?),
        u16::from_le_bytes(frame[8..10].try_into().ok()?),
    ))
}

#[inline(never)]
pub fn run_capenforce_b7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F177-b7");

    // 1) 版本单调：1→2 可、2→1 回退拒、2→2 平版拒（规则只进不退）。
    let mut v = RuleVersionLedger::new();
    let p1 = v.publish(1, 0);
    let p2 = v.publish(2, 10);
    let back = v.publish(1, 20);
    let flat = v.publish(2, 20);
    cs.add("rule_version_monotone", p1 && p2 && !back && !flat && v.n == 2, "");

    // 2) 版本回溯：day15 生效 v1、day25 生效 v2（历史可考——哪天哪版有账）。
    cs.add(
        "rule_version_lookup",
        v.version_at(5) == Some(1) && v.version_at(15) == Some(2) && v.latest() == Some(2),
        "",
    );

    // 3) 拒计数环：app7 三天三拒 → 3 日窗 3、2 日窗 2（窗口语义）。
    let mut r = DenyRing::new();
    r.record(7, 10);
    r.record(7, 11);
    r.record(7, 12);
    cs.add(
        "deny_ring_window",
        r.denies_in(7, 12, 3) == 3 && r.denies_in(7, 12, 2) == 2 && r.total() == 3,
        "",
    );

    // 4) 拒环按应用归因：app8 不背 app7 的账。
    cs.add("deny_ring_isolated", r.denies_in(8, 12, 3) == 0, "");

    // 5) 通知合并：窗内 3 次执法 → 合并计数 3（一条通知说清三次）。
    let mut m = NotifyMerger::new();
    let c1 = m.on_enforce(9, 0);
    let c2 = m.on_enforce(9, 10_000);
    let c3 = m.on_enforce(9, 20_000);
    cs.add("merge_window_accumulates", c1 == 1 && c2 == 2 && c3 == 3, "");

    // 6) 窗外冲账：40s 后再来 → 旧账 flushed、新账从 1 起。
    let c4 = m.on_enforce(9, 55_000);
    cs.add("merge_outside_flushes", c4 == 1 && m.flushed == 1, "");

    // 7) 分应用隔离合并：app9 的窗不并 app10（谁的事谁的通知）。
    let c5 = m.on_enforce(10, 56_000);
    cs.add("merge_per_app", c5 == 1, "");

    // 8) 统一冲账：累计 ≥2 的槽 flush（app10=2 → 冲出）、单次槽留存。
    m.on_enforce(10, 57_000); // app10 计 2
    m.on_enforce(11, 57_000); // app11 计 1（不动）
    let flushed_total = m.flush_all();
    cs.add(
        "merge_flush_all",
        flushed_total == 2 && m.flushed == 2,
        "",
    );

    // 9) 覆盖率：2/4 点活跃 = 500‰（覆盖面量化）。
    let active = [true, false, true, false];
    let all = [true, true, true, true];
    let none = [false, false, false, false];
    cs.add(
        "coverage_permille",
        coverage_permille(&active) == 500 && coverage_permille(&all) == 1_000 && coverage_permille(&none) == 0,
        "",
    );

    // 10) 合并帧 round-trip：合法四元组往返一致。
    let mut f = [0u8; MERGE_FRAME_LEN];
    assert!(encode_merge(9, 3, 30, 2, &mut f));
    cs.add(
        "merge_frame_roundtrip",
        decode_merge(&f) == Some((9, 3, 30, 2)) && f[0] == b'C' && f[1] == b'E',
        "",
    );

    // 11) 合并帧守门：severity>2 拒；撕裂拒（12 字节逐一）。
    let mut f2 = [0u8; MERGE_FRAME_LEN];
    let bad = !encode_merge(9, 3, 30, 3, &mut f2);
    let mut tear_ok = true;
    for i in 0..MERGE_FRAME_LEN {
        let mut t = f;
        t[i] ^= 0x2D;
        if decode_merge(&t).is_some() {
            tear_ok = false;
        }
    }
    cs.add("merge_frame_guards", bad && tear_ok, "");

    // 12) 常量自洽：窗 30s、环 24、点 4。
    cs.add(
        "b7_constants",
        MERGE_WINDOW_MS == 30_000 && DENY_RING_CAP == 24 && COVERAGE_POINTS == 4,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_version_full_history() {
        // 多版本链：1→3→5 全留痕、逐日回溯全对（历史不是覆盖式）。
        let mut v = RuleVersionLedger::new();
        v.publish(1, 0);
        v.publish(3, 10);
        v.publish(5, 20);
        assert_eq!(v.version_at(5), Some(1));
        assert_eq!(v.version_at(15), Some(3));
        assert_eq!(v.version_at(25), Some(5));
    }

    #[test]
    fn deny_ring_wraps_loss_oldest() {
        // 环回卷：24 满 → 第 25 条挤出最旧（近期拒数失忆最旧的——诚实口径）。
        let mut r = DenyRing::new();
        for d in 0..25u32 {
            r.record(1, d);
        }
        assert_eq!(r.total(), DENY_RING_CAP as u32);
        // 天 24 窗 24：天数 1..24 在窗（0 出窗）→ 24 条。
        assert_eq!(r.denies_in(1, 24, 24), 24);
    }

    #[test]
    fn merge_evicts_lowest_count_on_full() {
        // 满容驱逐累计最小：24 槽占满后第 25 应用挤掉计数 1 的槽（保高频源）。
        let mut m = NotifyMerger::new();
        for i in 0..DENY_RING_CAP as u32 {
            m.on_enforce(i, 1_000);
        }
        m.on_enforce(0, 2_000); // app0 升到 2
        m.on_enforce(99, 2_000); // 新应用——挤掉某个计数 1 的槽
        assert!(m.flushed == 1);
        // app0 计数 2 仍在（不是被驱逐者）。
        let mut found0 = false;
        for i in 0..DENY_RING_CAP {
            if m.app_ids[i] == Some(0) {
                found0 = true;
            }
        }
        assert!(found0);
    }
}
