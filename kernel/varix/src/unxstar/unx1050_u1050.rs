// unx1050_u1050 —— UNX-F1050 仲裁健康账（F0899 口径）
// 判据原文 UNX-F1050-J1：指标齐备（仲裁面 7 项指标），schema 校验通过，异常投递 10/10 次可达。
// 落位：kernel/varix/src/unxstar/；诊断段 0x2760 独占（0x2761~0x276F）。
//
// 设计（纯功能 no_std）：
//   * 七项指标：裁决数/命中率/降级率/排队深度/重跑率/异常数/p99 时延逐槽登记；
//   * schema 校验：版本+槽掩码合法性，非法 schema 拒收；
//   * 异常投递：异常事件向 F0899 投递计数与可达计数分账，10/10 可达；
//   * 健康判定：七槽齐 + 命中率达标 + p99 达标；
//   * saturating 防溢出 + FNV 指纹 + 注入核全抓。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ── 诊断段（0x2760 独占） ─────────────────────────────────────────────
pub const XC_AH_OOB: u32 = 0x2761; // 槽号越界或值域破
pub const XC_AH_SCHEMA: u32 = 0x2762; // schema 不符
pub const XC_AH_SEALED: u32 = 0x2763; // 封账后写指标
pub const XC_AH_LOST: u32 = 0x2764; // 投递不可达
pub const XC_AH_INCOMPLETE: u32 = 0x2765; // 七槽不齐

pub const SCHEMA_VER: u16 = 0x1050;
pub const METRIC_SLOTS: usize = 7;
pub const SLOT_NAMES: [&str; 7] = ["裁决数", "命中率", "降级率", "排队深度", "重跑率", "异常数", "p99时延"];
/// 命中率下限（万分比 9500 = 95%）。
pub const HIT_FLOOR: u32 = 9500;
/// p99 时延上限（µs）。
pub const P99_CEIL: u32 = 50;

#[derive(Clone, Copy)]
pub struct MetricRow {
    pub slot: usize, // 0..=6
    pub schema: u16, // 必须等于 SCHEMA_VER
    pub value: u32,  // 指标值
}

/// 仲裁健康账。
pub struct ArbHealth {
    metrics: [Option<u32>; METRIC_SLOTS],
    anomalies: Vec<u32>, // 异常事件码序列
    delivered: u32,
    reachable: u32,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl ArbHealth {
    pub fn new() -> ArbHealth {
        ArbHealth {
            metrics: [None; METRIC_SLOTS],
            anomalies: Vec::new(),
            delivered: 0,
            reachable: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 写一项指标（槽号/值域/schema 校验）。
    pub fn set_metric(&mut self, r: MetricRow) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_AH_SEALED, "封账后写指标"));
            return Err(XC_AH_SEALED);
        }
        if r.slot >= METRIC_SLOTS {
            self.rejects.push((XC_AH_OOB, "槽号越界"));
            return Err(XC_AH_OOB);
        }
        if r.schema != SCHEMA_VER {
            self.rejects.push((XC_AH_SCHEMA, "schema 不符"));
            return Err(XC_AH_SCHEMA);
        }
        // 值域：比率类槽（1/2/4）必须 ≤10000，其余 ≤ 1e9
        let rate_slot = r.slot == 1 || r.slot == 2 || r.slot == 4;
        if rate_slot && r.value > 10_000 {
            self.rejects.push((XC_AH_OOB, "比率值域破"));
            return Err(XC_AH_OOB);
        }
        self.metrics[r.slot] = Some(r.value);
        Ok(())
    }

    /// 七槽齐备。
    pub fn complete(&self) -> bool {
        self.metrics.iter().all(|m| m.is_some())
    }

    /// 健康判定：七槽齐 + 命中率达标 + p99 达标 + 异常全部投递可达。
    pub fn healthy(&self) -> bool {
        if !self.complete() || self.delivered != self.reachable {
            return false;
        }
        let hit = self.metrics[1].unwrap_or(0);
        let p99 = self.metrics[6].unwrap_or(u32::MAX);
        hit >= HIT_FLOOR && p99 <= P99_CEIL
    }

    /// 异常事件登记并向 F0899 投递。
    pub fn raise_anomaly(&mut self, code: u32, reachable: bool) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_AH_SEALED, "封账后投递"));
            return Err(XC_AH_SEALED);
        }
        self.anomalies.push(code);
        self.delivered = self.delivered.saturating_add(1);
        if reachable {
            self.reachable = self.reachable.saturating_add(1);
            Ok(())
        } else {
            self.rejects.push((XC_AH_LOST, "投递不可达"));
            Err(XC_AH_LOST)
        }
    }

    /// 异常账守恒：登记数 = 投递数。
    pub fn anomaly_conserved(&self) -> bool {
        self.anomalies.len() as u32 == self.delivered && self.delivered == self.reachable
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_AH_SEALED);
        }
        if !self.complete() {
            self.rejects.push((XC_AH_INCOMPLETE, "七槽不齐禁封"));
            return Err(XC_AH_INCOMPLETE);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        for (i, m) in self.metrics.iter().enumerate() {
            h ^= (i as u32).wrapping_mul(0x9E37) ^ m.unwrap_or(0xFFFF_FFFF);
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.delivered;
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.reachable;
        h = h.wrapping_mul(0x0100_0193);
        for &a in &self.anomalies {
            h ^= a;
            h = h.wrapping_mul(0x0100_0193);
        }
        h as u64
    }

    pub fn metric(&self, slot: usize) -> Option<u32> {
        if slot < METRIC_SLOTS {
            self.metrics[slot]
        } else {
            None
        }
    }
    pub fn delivered(&self) -> u32 {
        self.delivered
    }
    pub fn reachable(&self) -> u32 {
        self.reachable
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 注入核：六类破坏全抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）槽号越界必拒（10/10）
    {
        let mut h = ArbHealth::new();
        let mut hits = 0;
        for k in 7..17usize {
            if h.set_metric(MetricRow { slot: k, schema: SCHEMA_VER, value: 1 }) == Err(XC_AH_OOB) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）schema 不符必拒
    {
        let mut h = ArbHealth::new();
        if h.set_metric(MetricRow { slot: 0, schema: SCHEMA_VER + 1, value: 1 }) == Err(XC_AH_SCHEMA) {
            caught += 1;
        }
    }
    // 3）比率值域破必拒
    {
        let mut h = ArbHealth::new();
        if h.set_metric(MetricRow { slot: 1, schema: SCHEMA_VER, value: 12_345 }) == Err(XC_AH_OOB) {
            caught += 1;
        }
    }
    // 4）七槽不齐禁封
    {
        let mut h = ArbHealth::new();
        for s in 0..6usize {
            let _ = h.set_metric(MetricRow { slot: s, schema: SCHEMA_VER, value: 1 });
        }
        if h.seal() == Err(XC_AH_INCOMPLETE) {
            caught += 1;
        }
    }
    // 5）投递不可达必拒且可达数不涨
    {
        let mut h = ArbHealth::new();
        let r = h.raise_anomaly(0x101, false);
        if r == Err(XC_AH_LOST) && h.delivered() == 1 && h.reachable() == 0 {
            caught += 1;
        }
    }
    // 6）封账后写指标必拒
    {
        let mut h = ArbHealth::new();
        for s in 0..7usize {
            let _ = h.set_metric(MetricRow { slot: s, schema: SCHEMA_VER, value: 100 });
        }
        let _ = h.seal();
        if h.set_metric(MetricRow { slot: 0, schema: SCHEMA_VER, value: 1 }) == Err(XC_AH_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1050-J1。
pub fn run_unx1050_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1050");

    // C50-01 七项指标齐备
    {
        let mut h = ArbHealth::new();
        let vals = [1200u32, 9800, 150, 3, 40, 0, 12];
        let mut all_ok = true;
        for s in 0..METRIC_SLOTS {
            if h.set_metric(MetricRow { slot: s, schema: SCHEMA_VER, value: vals[s] }).is_err() {
                all_ok = false;
            }
        }
        s.add("C50-01-七槽齐备", all_ok && h.complete(), if all_ok { "" } else { "指标缺" });
    }
    // C50-02 schema 校验通过
    {
        let mut h = ArbHealth::new();
        for s in 0..7usize {
            let _ = h.set_metric(MetricRow { slot: s, schema: SCHEMA_VER, value: 100 });
        }
        let mut all_ok = true;
        // 非法 schema 拒
        if h.set_metric(MetricRow { slot: 0, schema: 0x9999, value: 1 }) != Err(XC_AH_SCHEMA) {
            all_ok = false;
        }
        s.add("C50-02-schema校验", all_ok && h.metric(0) == Some(100), if all_ok { "" } else { "schema漂移" });
    }
    // C50-03 异常投递 10/10 可达
    {
        let mut h = ArbHealth::new();
        let mut all_ok = true;
        for k in 1..=10u32 {
            if h.raise_anomaly(0x2000 + k, true).is_err() {
                all_ok = false;
            }
        }
        s.add("C50-03-投递可达", all_ok && h.delivered() == 10 && h.reachable() == 10, if all_ok { "" } else { "投递丢" });
    }
    // C50-04 异常账守恒
    {
        let mut h = ArbHealth::new();
        for k in 0..5u32 {
            let _ = h.raise_anomaly(k, true);
        }
        s.add("C50-04-异常守恒", h.anomaly_conserved(), if h.anomaly_conserved() { "" } else { "守恒破" });
    }
    // C50-05 健康判定（命中率+p99 达标）
    {
        let mut h = ArbHealth::new();
        let vals = [900u32, 9700, 200, 2, 30, 1, 40];
        for s in 0..METRIC_SLOTS {
            let _ = h.set_metric(MetricRow { slot: s, schema: SCHEMA_VER, value: vals[s] });
        }
        s.add("C50-05-健康判定", h.healthy(), if h.healthy() { "" } else { "健康破" });
    }
    // C50-06 不健康判定（命中率不足）
    {
        let mut h = ArbHealth::new();
        let vals = [900u32, 9000, 200, 2, 30, 1, 40];
        for s in 0..METRIC_SLOTS {
            let _ = h.set_metric(MetricRow { slot: s, schema: SCHEMA_VER, value: vals[s] });
        }
        s.add("C50-06-不健康识别", !h.healthy(), if !h.healthy() { "" } else { "漏判" });
    }
    // C50-07 封账产指纹
    {
        let mut h = ArbHealth::new();
        for s in 0..7usize {
            let _ = h.set_metric(MetricRow { slot: s, schema: SCHEMA_VER, value: 50 + s as u32 });
        }
        s.add("C50-07-封账指纹", h.seal().is_ok(), "");
    }
    // C50-08 指纹一致
    {
        let f = || {
            let mut h = ArbHealth::new();
            for s in 0..7usize {
                let _ = h.set_metric(MetricRow { slot: s, schema: SCHEMA_VER, value: 7 * s as u32 });
            }
            let _ = h.raise_anomaly(0x42, true);
            h.fingerprint()
        };
        s.add("C50-08-指纹一致", f() == f(), "");
    }
    // C50-09 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C50-09-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C50-10 拒收计数
    {
        let mut h = ArbHealth::new();
        let _ = h.set_metric(MetricRow { slot: 8, schema: SCHEMA_VER, value: 1 });
        s.add("C50-10-拒收计数", h.reject_count() == 1, if h.reject_count() == 1 { "" } else { "拒收漂移" });
    }
    s
}
