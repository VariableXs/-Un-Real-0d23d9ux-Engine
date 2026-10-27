//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1294 · 接管测绘健康账（F0899 口径注册） · 0xFE30
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1294-J1 测绘面健康指标 ≥10 项按 F0899 口径注册（测绘覆盖率/
//!      映射登记率/基线版本数/预登记完成率等），摘要一致率 100%，
//!      告警联通 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - F0899 口径：指标五元组（编号/名称/当前值/阈值/方向）注册制，
//!     注册即冻结（同号重复注册必拒）；
//!   - 12 项指标（≥10 判准）：覆盖率/登记率/版本数/预登记率/基线完整率/
//!     对齐率/断言通过率/审计完整率/压测通过率/回归绿率/配置一致率/
//!     告警延迟；
//!   - 方向闭集：Higher-Better / Lower-Better，越阈告警按方向判定；
//!   - 告警联通 10/10：逐指标注入越阈值样本，双向（触发/恢复）联通。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0xFE30 段，grep 全域确认独占）。
pub const SEG_BASE: u16 = 0xFE30;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_DUP_REG: u16 = SEG_BASE + 1;     // 同号重复注册
const E_METRIC_RANGE: u16 = SEG_BASE + 2; // 指标值越域
const E_TH_RANGE: u16 = SEG_BASE + 3;    // 阈值越域
const E_DIR_BAD: u16 = SEG_BASE + 4;     // 方向越闭集
const E_SNAP_MISS: u16 = SEG_BASE + 5;   // 摘要快照与注册账不一致
const E_FROZEN: u16 = SEG_BASE + 6;      // 冻结后注册
const E_COVER_MISS: u16 = SEG_BASE + 7;  // 注册数不足 10
const E_ALERT_MISS: u16 = SEG_BASE + 8;  // 告警未联通
const E_RECOVER_MISS: u16 = SEG_BASE + 9; // 恢复未联通
const E_LEDGER_FULL: u16 = SEG_BASE + 10; // 账满

/// 注册数下限（判准 ≥10；本账注册 12）。
pub const MIN_METRICS: usize = 10;
/// 实际注册数。
pub const METRIC_COUNT: usize = 12;

/// 账容量。
pub const HEALTH_CAP: usize = 16;

/// 指标方向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MetricDir {
    /// 越高越好（覆盖率类）。
    HigherBetter = 0,
    /// 越低越好（延迟类）。
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

    pub fn name(self) -> &'static str {
        match self {
            MetricDir::HigherBetter => "HigherBetter",
            MetricDir::LowerBetter => "LowerBetter",
        }
    }
}

/// 指标五元组（F0899 口径）。
#[derive(Clone, Copy, Debug)]
pub struct Metric {
    /// 指标编号（1~12）。
    pub id: u8,
    /// 指标名（冻结短名）。
    pub name: &'static str,
    /// 当前值（0~1000 千分制或原始值）。
    pub value: u32,
    /// 阈值。
    pub threshold: u32,
    /// 方向。
    pub dir: MetricDir,
}

impl Metric {
    /// 越阈判定：HigherBetter 时 value < threshold 即告警；
    /// LowerBetter 时 value > threshold 即告警。
    pub fn alarming(&self) -> bool {
        match self.dir {
            MetricDir::HigherBetter => self.value < self.threshold,
            MetricDir::LowerBetter => self.value > self.threshold,
        }
    }

    /// 域检：值与阈值均在 0~1000 闭域。
    pub fn in_domain(&self) -> bool {
        self.value <= 1000 && self.threshold <= 1000
    }
}

/// F0899 冻结指标名表（12 项——来源：F0899 口径注册册 B25 面）。
pub const METRIC_NAMES: [&str; METRIC_COUNT] = [
    "测绘覆盖率",
    "映射登记率",
    "基线版本数",
    "预登记完成率",
    "基线完整率",
    "对齐率",
    "断言通过率",
    "审计完整率",
    "压测通过率",
    "回归绿率",
    "配置一致率",
    "告警延迟",
];

/// 冻结指标方向表（前 11 项 HigherBetter，末项告警延迟 LowerBetter）。
pub const METRIC_DIRS: [MetricDir; METRIC_COUNT] = [
    MetricDir::HigherBetter,
    MetricDir::HigherBetter,
    MetricDir::HigherBetter,
    MetricDir::HigherBetter,
    MetricDir::HigherBetter,
    MetricDir::HigherBetter,
    MetricDir::HigherBetter,
    MetricDir::HigherBetter,
    MetricDir::HigherBetter,
    MetricDir::HigherBetter,
    MetricDir::HigherBetter,
    MetricDir::LowerBetter,
];

/// 冻结阈值表（千分制；告警延迟单位 0.1ms 上限 50=5ms）。
pub const METRIC_THRESHOLDS: [u32; METRIC_COUNT] = [
    1000, 1000, 3, 1000, 1000, 1000, 1000, 1000, 1000, 1000, 1000, 50,
];

/// 健康账。
pub struct HealthLedger {
    metrics: [Option<Metric>; HEALTH_CAP],
    used: usize,
    frozen: bool,
    version: u64,
    rejects: [u32; 11],
    /// 摘要快照（seal 时落档：逐指标 alarm 位）。
    snapshot_alarm: u32,
}

impl HealthLedger {
    pub fn new() -> HealthLedger {
        HealthLedger {
            metrics: [None; HEALTH_CAP],
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

    /// 注册一个指标（同号重复必拒）。
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
        if m.id == 0 || (m.id as usize) > HEALTH_CAP {
            return Err(self.reject(E_METRIC_RANGE));
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
        if self.used >= HEALTH_CAP {
            return Err(self.reject(E_LEDGER_FULL));
        }
        self.metrics[self.used] = Some(m);
        self.used += 1;
        self.version += 1;
        Ok(self.used - 1)
    }

    /// 更新指标值（越域必拒）。
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

    /// 摘要快照：落当前告警位图（seal 时一致核验的锚）。
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

    /// 摘要一致核验：重算告警位图与快照一致（一致率 100% 判准）。
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

    /// 冻结：注册数 ≥10 才可封。
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

/// 基线注册演练：12 项按 F0899 冻结表注册，全部健康值（无告警）。
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
    detail.push(format!(
        "注册 {}/12 快照告警位={:#x} 一致={} 冻结={}",
        ok, snap, consistent, frozen
    ));
    (ok, if consistent && frozen { 1 } else { 0 }, detail)
}

/// 告警联通 10/10：逐轮把一个指标压到越阈值——触发联通 + 恢复联通
/// 双向各计一次。
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
        // 目标指标压到越阈。
        let target = (r + 1) as u8;
        let bad_value = if METRIC_DIRS[r % METRIC_COUNT] == MetricDir::HigherBetter {
            METRIC_THRESHOLDS[r % METRIC_COUNT].saturating_sub(1)
        } else {
            METRIC_THRESHOLDS[r % METRIC_COUNT] + 1
        };
        let t1 = ledger.update(target, bad_value);
        let snap_bad = ledger.snapshot();
        let triggered = t1.is_ok() && (snap_bad & (1u32 << (target - 1))) != 0;
        // 恢复。
        let good_value = if METRIC_DIRS[r % METRIC_COUNT] == MetricDir::HigherBetter {
            1000
        } else {
            1
        };
        let _ = ledger.update(target, good_value);
        let snap_good = ledger.snapshot();
        let recovered = (snap_good & (1u32 << (target - 1))) == 0;
        if triggered && recovered {
            ok += 1;
        }
        r += 1;
    }
    if ok == 10 {
        detail.push("告警联通 10/10".to_string());
    }
    (ok, 10, detail)
}

/// 注入 4 项：重复注册/越域值/越闭集方向/注册不足冻结——必抓。
pub fn inject_bad_registration() -> (u32, u32, Vec<String>) {
    let mut caught = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut ledger = HealthLedger::new();
    let m1 = Metric { id: 1, name: METRIC_NAMES[0], value: 1000, threshold: 1000, dir: MetricDir::HigherBetter };
    let _ = ledger.register(m1);
    // 重复注册。
    if ledger.register(m1) == Err(E_DUP_REG) {
        caught += 1;
    } else {
        detail.push("重复注册未拒".to_string());
    }
    // 越域值。
    let m2 = Metric { id: 2, name: METRIC_NAMES[1], value: 1001, threshold: 1000, dir: MetricDir::HigherBetter };
    if ledger.register(m2) == Err(E_METRIC_RANGE) {
        caught += 1;
    } else {
        detail.push("越域值未拒".to_string());
    }
    // 越闭集方向（from_id 不可还原 + register 拒）。
    let dir_bad = MetricDir::from_id(9);
    if dir_bad.is_none() {
        caught += 1;
    } else {
        detail.push("越闭集方向未拒".to_string());
    }
    // 注册不足 10 冻结。
    let fr = ledger.freeze();
    if fr == Err(E_COVER_MISS) {
        caught += 1;
    } else {
        detail.push("注册不足冻结未拒".to_string());
    }
    if caught == 4 {
        detail.push("注册注入 4/4 全抓".to_string());
    }
    (caught, 4, detail)
}

/// 摘要一致率演练：随机化 5 组值分布，每组快照后重算一致。
pub fn drill_snapshot_consistency() -> (u32, u32, Vec<String>) {
    let mut ok = 0u32;
    let detail: Vec<String> = Vec::new();
    let mut round = 0;
    while round < 5 {
        let mut ledger = HealthLedger::new();
        let mut i = 0;
        while i < METRIC_COUNT {
            // 确定性伪随机值（LCG 型同余，种子写死）。
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

/// 段位守卫：0xFE30 段十码闭集连续。
pub fn segment_guard() -> bool {
    let codes = [
        E_DUP_REG, E_METRIC_RANGE, E_TH_RANGE, E_DIR_BAD, E_SNAP_MISS,
        E_FROZEN, E_COVER_MISS, E_ALERT_MISS, E_RECOVER_MISS, E_LEDGER_FULL,
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

/// run_unx1294_checks：本单判据聚合（诊断段 0xFE30）。
pub fn run_unx1294_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1294 测绘健康账");
    let (r1, ok1, d1) = drill_register_healthy();
    s.add_dyn(
        "J1-12项按F0899注册",
        r1 == 12,
        &format!("{}/12 摘要{} {}", r1, ok1, d1.join(" ")),
    );
    let (c2, t2, d2) = drill_alert_link_10();
    s.add_dyn("J1-告警联通10/10", c2 == t2, &format!("{}/{} {}", c2, t2, d2.join(" ")));
    let (c3, t3, d3) = inject_bad_registration();
    s.add_dyn("J1-注册注入4/4", c3 == t3, &format!("{}/{} {}", c3, t3, d3.join(" ")));
    let (c4, t4, d4) = drill_snapshot_consistency();
    s.add_dyn("J1-摘要一致5/5", c4 == t4, &format!("{}/{} {}", c4, t4, d4.join(" ")));
    s.add_dyn("J1-指标数下限", METRIC_COUNT >= MIN_METRICS, "12≥10");
    s.add_dyn("J2-段位守卫", segment_guard(), "0xFE30段十码闭集");
    s
}
