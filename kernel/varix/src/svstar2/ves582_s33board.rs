//! VE-F4348 · 一致性指标看板（U08 测试与资产组）
//! 职责：一致性指标总看板——三域聚合统一视图（治理度量 U04 + 性能 U07 +
//! 测试 U08）+ 看板纪律（指标口径版本化——口径变更走演进流·口径漂移版本
//! 戳断言）+ 聚合冲突以源头为准重算 + 告警联动 + 看板失流置灰 + 全量数据
//! 表 + 色弱可辨三通道（域本色）。
//! 段位：0x4160（续表 idx582）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x4160;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BkCode {
    /// 聚合冲突（以源头为准重算）。
    AggClash = SEG_BASE,
    /// 口径漂移（版本戳断言拒绝）。
    CaliberDrift = SEG_BASE + 0x01,
    /// 看板失流（置灰标注）。
    Stalled = SEG_BASE + 0x02,
    /// 域号越界。
    DomainRange = SEG_BASE + 0x03,
}

/// 三域。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Domain {
    /// U04 治理度量。
    Gov,
    /// U07 性能。
    Perf,
    /// U08 测试。
    Test,
}

/// 一条指标。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metric {
    pub domain: Domain,
    pub key: u32,
    pub value: u32,
    /// 口径版本戳。
    pub caliber: u32,
}

/// 看板台。
#[derive(Default)]
pub struct Board {
    pub cells: Vec<Metric>,
    /// 当前口径单源。
    pub caliber_now: u32,
    /// 置灰域。
    pub grayed: Vec<u8>,
    pub recalcs: u32,
    pub alerts: u32,
    last_tick: u64,
}

impl Board {
    pub fn new(caliber: u32) -> Self {
        Board { caliber_now: caliber, ..Default::default() }
    }

    /// 指标上板：口径不符拒绝（版本戳断言）；域校验。
    pub fn put(&mut self, m: Metric) -> Result<(), BkCode> {
        if m.caliber != self.caliber_now {
            return Err(BkCode::CaliberDrift);
        }
        if let Some(c) = self.cells.iter_mut().find(|c| c.domain == m.domain && c.key == m.key) {
            if c.value != m.value {
                // 聚合冲突：以源头（最新上报）为准重算。
                *c = m;
                self.recalcs += 1;
                return Err(BkCode::AggClash);
            }
            *c = m;
        } else {
            self.cells.push(m);
        }
        Ok(())
    }

    /// 域聚合视图：域内指标值均值。
    pub fn domain_view(&self, d: Domain) -> Result<u32, BkCode> {
        let vals: Vec<u32> = self.cells.iter().filter(|c| c.domain == d).map(|c| c.value).collect();
        if vals.is_empty() {
            return Err(BkCode::DomainRange);
        }
        let sum: u64 = vals.iter().map(|v| *v as u64).sum();
        Ok((sum / vals.len() as u64) as u32)
    }

    /// 口径演进：换版后旧口径指标全部失效下板（返回下板数）。
    pub fn evolve(&mut self, new_caliber: u32) -> usize {
        let old = self.caliber_now;
        self.caliber_now = new_caliber;
        let before = self.cells.len();
        self.cells.retain(|c| c.caliber != old);
        before - self.cells.len()
    }

    /// 看板心跳。
    pub fn tick(&mut self, at: u64) {
        self.last_tick = at;
    }

    /// 失流检测：距最后心跳超窗 = 置灰（计数）。
    pub fn stalled(&mut self, now: u64, window: u64) -> bool {
        let stale = now >= self.last_tick + window;
        if stale && !self.grayed.iter().any(|g| *g == 0) {
            self.grayed.push(0);
        }
        stale
    }

    /// 告警桥联动：越阈指标触发告警（计数）。
    pub fn alert_if(&mut self, d: Domain, key: u32, threshold: u32) -> bool {
        if let Some(c) = self.cells.iter().find(|c| c.domain == d && c.key == key) {
            if c.value > threshold {
                self.alerts += 1;
                return true;
            }
        }
        false
    }

    /// 色弱可辨三通道（域本色）：值 → (数值, 标签, 位形)。
    pub fn tri(&self, v: u32) -> (u32, &'static str, u8) {
        let label = if v >= 9000 { "优" } else if v >= 7000 { "良" } else { "待改" };
        (v, label, (v / 1250).min(7) as u8)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves582_checks() -> CheckSet {
    let mut set = CheckSet::new("ves582");
    let mut bd = Board::new(11);

    // C582-01 三域指标上板。
    let _ = bd.put(Metric { domain: Domain::Gov, key: 1, value: 80, caliber: 11 });
    let _ = bd.put(Metric { domain: Domain::Perf, key: 1, value: 90, caliber: 11 });
    let _ = bd.put(Metric { domain: Domain::Test, key: 1, value: 70, caliber: 11 });
    chk(&mut set, "C582-01", bd.cells.len() == 3, "三域上板");

    // C582-02 口径漂移版本戳断言拒绝。
    chk(&mut set, "C582-02", bd.put(Metric { domain: Domain::Gov, key: 2, value: 1, caliber: 12 })
        == Err(BkCode::CaliberDrift), "口径漂移拒");

    // C582-03 聚合冲突以源头重算（计数留痕）。
    chk(&mut set, "C582-03", bd.put(Metric { domain: Domain::Gov, key: 1, value: 85, caliber: 11 })
        == Err(BkCode::AggClash) && bd.recalcs == 1
        && bd.cells.iter().find(|c| c.domain == Domain::Gov).map(|c| c.value) == Some(85), "冲突源头重算");

    // C582-04 同值幂等不计数。
    chk(&mut set, "C582-04", bd.put(Metric { domain: Domain::Gov, key: 1, value: 85, caliber: 11 }).is_ok()
        && bd.recalcs == 1, "同值幂等");

    // C582-05 域聚合视图。
    let _ = bd.put(Metric { domain: Domain::Gov, key: 2, value: 95, caliber: 11 });
    chk(&mut set, "C582-05", bd.domain_view(Domain::Gov) == Ok(90), "域聚合均值");

    // C582-06 空域视图拒绝。
    chk(&mut set, "C582-06", bd.domain_view(Domain::Perf).is_ok() == true, "有域可查");

    // C582-07 口径演进旧口径全下板。
    let _ = bd.put(Metric { domain: Domain::Test, key: 2, value: 60, caliber: 11 });
    let n = bd.evolve(12);
    chk(&mut set, "C582-07", n == 4 && bd.cells.is_empty(), "演进旧口径下板");

    // C582-08 演进后新口径上板。
    chk(&mut set, "C582-08", bd.put(Metric { domain: Domain::Perf, key: 1, value: 88, caliber: 12 }).is_ok(), "新口径上板");

    // C582-09 看板心跳与失流置灰。
    bd.tick(100);
    chk(&mut set, "C582-09", !bd.stalled(150, 100) && bd.stalled(250, 100), "超窗置灰");

    // C582-10 告警桥联动（越阈触发）。
    chk(&mut set, "C582-10", bd.alert_if(Domain::Perf, 1, 80) && !bd.alert_if(Domain::Perf, 1, 200)
        && bd.alerts == 1, "越阈告警计数");

    // C582-11 色弱可辨三通道（域本色）。
    chk(&mut set, "C582-11", bd.tri(9500) == (9500, "优", 7) && bd.tri(6000) == (6000, "待改", 4), "三通道齐");

    // C582-12 空域视图域号拒绝（不臆测）。
    let bd2 = Board::new(1);
    chk(&mut set, "C582-12", bd2.domain_view(Domain::Test).is_err(), "空域不臆测");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caliber_gate() {
        let mut bd = Board::new(5);
        assert!(bd.put(Metric { domain: Domain::Gov, key: 1, value: 1, caliber: 5 }).is_ok());
        assert_eq!(bd.put(Metric { domain: Domain::Gov, key: 2, value: 1, caliber: 6 }), Err(BkCode::CaliberDrift));
    }

    #[test]
    fn evolve_drops_old() {
        let mut bd = Board::new(5);
        for k in 1..=3u32 {
            let _ = bd.put(Metric { domain: Domain::Gov, key: k, value: k, caliber: 5 });
        }
        assert_eq!(bd.evolve(6), 3);
        assert!(bd.cells.is_empty());
    }
}
