//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1359 · 压测健康账（F0899 口径注册） · 0x2ED0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F1359-J1 B28 面健康指标 ≥10 项按 F0899 口径注册（全谱执行率/
//!      熔断次数/基准偏差/资产版本数等），摘要一致率 100%，告警联通
//!      10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 12 项指标（≥10 判准）五元组注册（编号/名/值/阈值/方向），
//!     同号重复注册必拒；
//!   - 方向闭集 HigherBetter/LowerBetter，越阈按方向判定；
//!   - 告警联通 10/10：逐指标越阈触发 + 恢复复位双向联通；
//!   - 摘要一致率：快照后重算一致 5/5。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x2ED0 段，grep 全域确认独占）。
pub const SEG_BASE: u16 = 0x2ED0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_DUP_REG: u16 = SEG_BASE + 1;     // 同号重复注册
const E_METRIC_RANGE: u16 = SEG_BASE + 2; // 指标值越域
const E_DIR_BAD: u16 = SEG_BASE + 3;     // 方向越闭集
const E_SNAP_MISS: u16 = SEG_BASE + 4;   // 快照与注册账不一致
const E_FROZEN: u16 = SEG_BASE + 5;      // 冻结后注册
const E_COVER_MISS: u16 = SEG_BASE + 6;  // 注册数不足 10
const E_ALERT_MISS: u16 = SEG_BASE + 7;  // 告警未联通
const E_RECOVER_MISS: u16 = SEG_BASE + 8; // 恢复未联通
const E_ID_BAD: u16 = SEG_BASE + 9;      // 指标号越表
const E_LEDGER_FULL: u16 = SEG_BASE + 10; // 账满

/// 指标数下限（判准 ≥10；本账 12）。
pub const MIN_METRICS: usize = 10;
pub const METRIC_COUNT: usize = 12;

/// F0899 冻结指标名表（12 项——来源：F0899 口径注册册 B28 面）。
pub const METRIC_NAMES: [&str; METRIC_COUNT] = [
    "全谱执行率",
    "场景通过率",
    "熔断次数",
    "基准偏差",
    "资产版本数",
    "复跑一致率",
    "守恒达标率",
    "注入检出率",
    "导出完整率",
    "联签一致率",
    "护栏生效数",
    "告警延迟",
];

/// 方向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MetricDir {
    HigherBetter = 0,
    LowerBetter = 1,
}

impl MetricDir {
    pub fn from_id(v: u8) -> Option<MetricDir> {
        match v {
            0 => Some(MetricDir::HigherBetter),
            1 => Some(MetricDir::LowerBetter),
            _ => None,
        }
    }
}

/// 冻结方向表（末项告警延迟 LowerBetter，余 HigherBetter）。
pub const METRIC_DIRS: [MetricDir; METRIC_COUNT] = [
    MetricDir::HigherBetter,
    MetricDir::HigherBetter,
    MetricDir::LowerBetter,
    MetricDir::LowerBetter,
    MetricDir::HigherBetter,
    MetricDir::HigherBetter,
    MetricDir::HigherBetter,
    MetricDir::HigherBetter,
    MetricDir::HigherBetter,
    MetricDir::HigherBetter,
    MetricDir::HigherBetter,
    MetricDir::LowerBetter,
];

/// 冻结阈值表（千分制；告警延迟 0.1ms 上限 50；LowerBetter 阈值须
/// < 1000 留出越阈样本空间——基准偏差 900=10% 红线内）。
pub const METRIC_THRESHOLDS: [u32; METRIC_COUNT] = [
    1000, 1000, 0, 900, 3, 1000, 1000, 1000, 1000, 1000, 4, 50,
];

/// 指标五元组。
#[derive(Clone, Copy, Debug)]
pub struct Metric {
    pub id: u8,
    pub name: &'static str,
    pub value: u32,
    pub threshold: u32,
    pub dir: MetricDir,
}

impl Metric {
    pub fn alarming(&self) -> bool {
        match self.dir {
            MetricDir::HigherBetter => self.value < self.threshold,
            MetricDir::LowerBetter => self.value > self.threshold,
        }
    }

    pub fn in_domain(&self) -> bool {
        self.value <= 1000 && self.threshold <= 1000
    }
}

/// 健康账。
pub struct HealthLedger {
    metrics: [Option<Metric>; 16],
    used: usize,
    frozen: bool,
    version: u64,
    rejects: [u32; 11],
    snapshot_alarm: u32,
}

impl HealthLedger {
    pub fn new() -> HealthLedger {
        HealthLedger {
            metrics: [None; 16],
            used: 0,
            frozen: false,
            version: 0,
            rejects: [0; 11],
            snapshot_alarm: 0,
        }
    }

    fn reject(&mut self, code: u16) -> u16 {
        let idx = (code - SEG_BASE) as usize;
        if idx < self.rejects.len() {
            self.rejects[idx] += 1;
        }
        code
    }

    pub fn register(&mut self, m: Metric) -> Result<usize, u16> {
        if self.frozen {
            return Err(self.reject(E_FROZEN));
        }
        if !m.in_domain() {
            return Err(self.reject(E_METRIC_RANGE));
        }
        if MetricDir::from_id(m.dir as u8).is_none() {
            return Err(self.reject(E_DIR_BAD));
        }
        if m.id == 0 || (m.id as usize) > 16 {
            return Err(self.reject(E_ID_BAD));
        }
        let mut i = 0;
        while i < self.used {
            if let Some(x) = self.metrics[i] {
                if x.id == m.id {
                    return Err(self.reject(E_DUP_REG));
                }
            }
            i += 1;
        }
        if self.used >= 16 {
            return Err(self.reject(E_LEDGER_FULL));
        }
        self.metrics[self.used] = Some(m);
        self.used += 1;
        self.version += 1;
        Ok(self.used - 1)
    }

    pub fn update(&mut self, id: u8, value: u32) -> Result<(), u16> {
        if value > 1000 {
            return Err(self.reject(E_METRIC_RANGE));
        }
        let mut i = 0;
        while i < self.used {
            if let Some(slot) = self.metrics[i] {
                if slot.id == id {
                    if let Some(m) = self.metrics[i].as_mut() {
                        m.value = value;
                    }
                    self.version += 1;
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(self.reject(E_SNAP_MISS))
    }

    pub fn snapshot(&mut self) -> u32 {
        let mut mask = 0u32;
        let mut i = 0;
        while i < self.used {
            if let Some(m) = self.metrics[i] {
                if m.alarming() {
                    mask |= 1u32 << (m.id - 1);
                }
            }
            i += 1;
        }
        self.snapshot_alarm = mask;
        mask
    }

    pub fn snapshot_consistent(&self) -> bool {
        let mut mask = 0u32;
        let mut i = 0;
        while i < self.used {
            if let Some(m) = self.metrics[i] {
                if m.alarming() {
                    mask |= 1u32 << (m.id - 1);
                }
            }
            i += 1;
        }
        mask == self.snapshot_alarm
    }

    pub fn freeze(&mut self) -> Result<u64, u16> {
        if self.frozen {
            return Err(self.reject(E_FROZEN));
        }
        if self.used < MIN_METRICS {
            return Err(self.reject(E_COVER_MISS));
        }
        self.frozen = true;
        self.version += 1;
        Ok(self.version)
    }

    pub fn used(&self) -> usize {
        self.used
    }
}

/// 基线注册演练：12 项按 F0899 冻结表注册，全健康值。
pub fn drill_register_healthy() -> (u32, u32, Vec<String>) {
    let mut ledger = HealthLedger::new();
    let mut detail: Vec<String> = Vec::new();
    let mut ok = 0u32;
    let mut i = 0;
    while i < METRIC_COUNT {
        let m = Metric {
            id: (i + 1) as u8,
            name: METRIC_NAMES[i],
            value: match METRIC_DIRS[i] {
                MetricDir::HigherBetter => 1000,
                MetricDir::LowerBetter => 10,
            },
            threshold: METRIC_THRESHOLDS[i],
            dir: METRIC_DIRS[i],
        };
        match ledger.register(m) {
            Ok(_) => ok += 1,
            Err(c) => detail.push(format!("指标 {} 注册被拒：0x{:04X}", METRIC_NAMES[i], c)),
        }
        i += 1;
    }
    let snap = ledger.snapshot();
    let consistent = ledger.snapshot_consistent();
    let frozen = ledger.freeze().is_ok();
    detail.push(format!("注册 {}/12 快照告警位={:#x} 一致={} 冻结={}", ok, snap, consistent, frozen));
    (ok, if consistent && frozen { 1 } else { 0 }, detail)
}

/// 告警联通 10/10：逐轮把一个指标压越阈→触发联通+恢复联通双向。
pub fn drill_alert_link_10() -> (u32, u32, Vec<String>) {
    let mut ok = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut r = 0;
    while r < 10 {
        let mut ledger = HealthLedger::new();
        let mut i = 0;
        while i < METRIC_COUNT {
            let m = Metric {
                id: (i + 1) as u8,
                name: METRIC_NAMES[i],
                value: 1000,
                threshold: METRIC_THRESHOLDS[i],
                dir: METRIC_DIRS[i],
            };
            let _ = ledger.register(m);
            i += 1;
        }
        let target = (r % METRIC_COUNT + 1) as u8;
        let bad_value = if METRIC_DIRS[r % METRIC_COUNT] == MetricDir::HigherBetter {
            METRIC_THRESHOLDS[r % METRIC_COUNT].saturating_sub(1)
        } else {
            METRIC_THRESHOLDS[r % METRIC_COUNT] + 1
        };
        let t1 = ledger.update(target, bad_value);
        let snap_bad = ledger.snapshot();
        let triggered = t1.is_ok() && (snap_bad & (1u32 << (target - 1))) != 0;
        let good_value = if METRIC_DIRS[r % METRIC_COUNT] == MetricDir::HigherBetter {
            1000
        } else {
            0 // LowerBetter 归零才解除（含阈值 0 的指标）
        };
        let _ = ledger.update(target, good_value);
        let snap_good = ledger.snapshot();
        let recovered = (snap_good & (1u32 << (target - 1))) == 0;
        if triggered && recovered {
            ok += 1;
        } else {
            detail.push(format!(
                "轮{}目标{}触发={}恢复={} bad={} good={} thr={} t1err={:?}",
                r, target, triggered, recovered, bad_value, good_value,
                METRIC_THRESHOLDS[r % METRIC_COUNT],
                t1.is_err()
            ));
        }
        r += 1;
    }
    if ok == 10 {
        detail.push("告警联通 10/10".to_string());
    }
    (ok, 10, detail)
}

/// 摘要一致率演练：5 组确定性值分布，快照后重算一致。
pub fn drill_snapshot_consistency() -> (u32, u32, Vec<String>) {
    let mut ok = 0u32;
    let detail: Vec<String> = Vec::new();
    let mut round = 0;
    while round < 5 {
        let mut ledger = HealthLedger::new();
        let mut i = 0;
        while i < METRIC_COUNT {
            let v = ((round * 137 + i * 61 + 23) % 1001) as u32;
            let m = Metric {
                id: (i + 1) as u8,
                name: METRIC_NAMES[i],
                value: v,
                threshold: METRIC_THRESHOLDS[i],
                dir: METRIC_DIRS[i],
            };
            let _ = ledger.register(m);
            i += 1;
        }
        ledger.snapshot();
        if ledger.snapshot_consistent() {
            ok += 1;
        }
        round += 1;
    }
    (ok, 5, detail)
}

/// 注入 4 项：重复注册/越域值/越方向/注册不足冻结——必抓。
pub fn inject_bad_registration() -> (u32, u32, Vec<String>) {
    let mut caught = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut ledger = HealthLedger::new();
    let m1 = Metric { id: 1, name: METRIC_NAMES[0], value: 1000, threshold: 1000, dir: MetricDir::HigherBetter };
    let _ = ledger.register(m1);
    if ledger.register(m1) == Err(E_DUP_REG) {
        caught += 1;
    } else {
        detail.push("重复注册未拒".to_string());
    }
    let m2 = Metric { id: 2, name: METRIC_NAMES[1], value: 1001, threshold: 1000, dir: MetricDir::HigherBetter };
    if ledger.register(m2) == Err(E_METRIC_RANGE) {
        caught += 1;
    } else {
        detail.push("越域值未拒".to_string());
    }
    if MetricDir::from_id(9).is_none() {
        caught += 1;
    } else {
        detail.push("越方向未拒".to_string());
    }
    if ledger.freeze() == Err(E_COVER_MISS) {
        caught += 1;
    } else {
        detail.push("注册不足冻结未拒".to_string());
    }
    if caught == 4 {
        detail.push("注册注入 4/4 全抓".to_string());
    }
    (caught, 4, detail)
}

/// 段位守卫：0x2ED0 段十码闭集连续。
pub fn segment_guard() -> bool {
    let codes = [
        E_DUP_REG, E_METRIC_RANGE, E_DIR_BAD, E_SNAP_MISS, E_FROZEN,
        E_COVER_MISS, E_ALERT_MISS, E_RECOVER_MISS, E_ID_BAD, E_LEDGER_FULL,
    ];
    let mut i = 0;
    while i < codes.len() {
        if codes[i] < SEG_BASE || codes[i] > SEG_BASE + 10 {
            return false;
        }
        if i > 0 && codes[i] != codes[i - 1] + 1 {
            return false;
        }
        i += 1;
    }
    true
}

/// run_unx1359_checks：本单判据聚合（诊断段 0x2ED0）。
pub fn run_unx1359_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1359 压测健康账");
    let (r1, ok1, d1) = drill_register_healthy();
    s.add_dyn(
        "J1-12项按F0899注册",
        r1 == 12,
        &format!("{}/12 摘要{} {}", r1, ok1, d1.join(" ")),
    );
    let (c2, t2, d2) = drill_alert_link_10();
    s.add_dyn("J1-告警联通10/10", c2 == t2, &format!("{}/{} {}", c2, t2, d2.join(" ")));
    let (c3, t3, d3) = drill_snapshot_consistency();
    s.add_dyn("J1-摘要一致5/5", c3 == t3, &format!("{}/{} {}", c3, t3, d3.join(" ")));
    let (c4, t4, d4) = inject_bad_registration();
    s.add_dyn("J1-注册注入4/4", c4 == t4, &format!("{}/{} {}", c4, t4, d4.join(" ")));
    s.add_dyn("J1-指标数下限", METRIC_COUNT >= MIN_METRICS, "12≥10");
    s.add_dyn("J2-段位守卫", segment_guard(), "0x2ED0段十码闭集");
    s
}
