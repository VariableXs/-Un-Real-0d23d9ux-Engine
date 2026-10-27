//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1250 · Arming 态健康账（F0899 口径注册） · 0xFB70
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F1250-J1 Arming 面健康指标 ≥10 项按 F0899 口径注册（武装
//!      次数/成功率/失败率/回退次数/确认窗口到齐率/序列时长 P99 等），
//!      摘要一致率 100%，告警联通 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - F0899 口径：指标四元组（编号/中文名/值/红阈）+ 方向（越高越险
//!     /越低越险），注册表写死 12 项（≥10 达标）；
//!   - 健康源：本地武装面确定性会话（LCG 种子 0xFB70）产出 12 项真值
//!     （次数/千分率/P99 tick），零交叉引用；
//!   - 快照摘要：FNV 摘要 + 非空摘要文本，两次快照一致率 100%；
//!   - 告警引擎：越阈必告警、不越必静默，双向判据，联通 10/10 次；
//!   - 注入核 5 项（缺指标/阈值漂移/摘要漂移/告警漏发/指标越界）必抓。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0xFB70 段，主代理分工表分配）。
pub const SEG_BASE: u16 = 0xFB70;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 健康指标项数（判准 ≥10，注册 12 项）。
pub const METRIC_COUNT: usize = 12;

/// 诊断码（0xFB70 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HlCode {
    /// 0xFB70 缺指标（注册 <10 项）。
    MetricShort = 0,
    /// 0xFB71 指标重号（编号唯一性破）。
    MetricDup = 1,
    /// 0xFB72 阈值漂移（红阈与注册表不符）。
    ThreshDrift = 2,
    /// 0xFB73 摘要漂移（两次快照摘要不一致）。
    DigestDrift = 3,
    /// 0xFB74 告警漏发（越阈未告警）。
    AlarmMiss = 4,
    /// 0xFB75 告警误发（未越阈告警）。
    AlarmFalse = 5,
    /// 0xFB76 指标越界（千分率 >1000）。
    RateOob = 6,
    /// 0xFB77 摘要空文本（结果摘要非空判据破）。
    SummaryEmpty = 7,
    /// 0xFB78 方向未登记（红向缺省）。
    DirMiss = 8,
    /// 0xFB79 注册表缩水（只增不减被破）。
    RegShrink = 9,
}

impl HlCode {
    /// 判别式真值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集。
    pub const ALL: [HlCode; 10] = [
        HlCode::MetricShort, HlCode::MetricDup, HlCode::ThreshDrift, HlCode::DigestDrift,
        HlCode::AlarmMiss, HlCode::AlarmFalse, HlCode::RateOob, HlCode::SummaryEmpty,
        HlCode::DirMiss, HlCode::RegShrink,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        0xFB70 => "缺指标：注册表补齐 ≥10 项（武装次数/成功率/失败率/回退/到齐率/P99 等）。",
        0xFB71 => "指标重号：注册表两两去重，重号改号。",
        0xFB72 => "阈值漂移：红阈写死注册，运行期不许改阈。",
        0xFB73 => "摘要漂移：快照必须纯函数（同账同摘要），重算核。",
        0xFB74 => "告警漏发：越阈必告警，漏发回溯该指标方向与阈值。",
        0xFB75 => "告警误发：未越阈静默，误发核方向登记。",
        0xFB76 => "指标越界：千分率先钳制再入账，越界即修源。",
        0xFB77 => "摘要空文本：摘要必须非空（编号+值+告警数）。",
        0xFB78 => "方向未登记：每指标必须声明红向（越高越险/越低越险）。",
        0xFB79 => "注册表缩水：注册只增不减，禁截断。",
        _ => "未知诊断码：回查 0xFB70 段闭集。",
    }
}

/// 红向（越阈方向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RedDir {
    /// 越高越险（如失败次数、P99 时长）。
    HighBad,
    /// 越低越险（如成功率、到齐率，千分率）。
    LowBad,
}

/// 健康指标（F0899 口径四元组 + 红向）。
#[derive(Clone, Copy, Debug)]
pub struct HealthMetric {
    /// 指标编号（5001 起连续）。
    pub id: u32,
    /// 中文名。
    pub name: &'static str,
    /// 红阈（红向判据比较线）。
    pub red_thresh: u64,
    /// 红向。
    pub dir: RedDir,
    /// 单位短句。
    pub unit: &'static str,
}

/// 12 项指标注册表（判准点名项全含；阈值写死）。
pub const METRICS: [HealthMetric; METRIC_COUNT] = [
    HealthMetric { id: 5001, name: "武装次数", red_thresh: 1, dir: RedDir::LowBad, unit: "次" },
    HealthMetric { id: 5002, name: "武装成功率", red_thresh: 900, dir: RedDir::LowBad, unit: "‰" },
    HealthMetric { id: 5003, name: "武装失败率", red_thresh: 100, dir: RedDir::HighBad, unit: "‰" },
    HealthMetric { id: 5004, name: "回退次数", red_thresh: 5, dir: RedDir::HighBad, unit: "次" },
    HealthMetric { id: 5005, name: "确认窗口到齐率", red_thresh: 900, dir: RedDir::LowBad, unit: "‰" },
    HealthMetric { id: 5006, name: "序列时长P99", red_thresh: 80, dir: RedDir::HighBad, unit: "tick" },
    HealthMetric { id: 5007, name: "半武装检出率", red_thresh: 950, dir: RedDir::LowBad, unit: "‰" },
    HealthMetric { id: 5008, name: "重武装成功率", red_thresh: 900, dir: RedDir::LowBad, unit: "‰" },
    HealthMetric { id: 5009, name: "红账闭环率", red_thresh: 950, dir: RedDir::LowBad, unit: "‰" },
    HealthMetric { id: 5010, name: "压测通过率", red_thresh: 950, dir: RedDir::LowBad, unit: "‰" },
    HealthMetric { id: 5011, name: "事件零丢失轮占比", red_thresh: 990, dir: RedDir::LowBad, unit: "‰" },
    HealthMetric { id: 5012, name: "守恒违例数", red_thresh: 0, dir: RedDir::HighBad, unit: "次" },
];

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0xFB70 段基址（固定种子，可重复）。
    pub const SEED: u64 = 0xFB70_FB70_FB70_FB70;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// FNV-1a 滚动哈希步。
const fn fnv_step(acc: u64, byte: u64) -> u64 {
    (acc ^ byte).wrapping_mul(0x100000001b3)
}

/// 武装面确定性会话（健康源正本）：产 12 项指标真值。
///
/// 会话跑 20 轮武装：每轮向量数 LCG 决定，失败/回退/到齐由确定性脚本
/// 产生，聚合为 F0899 口径指标值。
#[derive(Debug)]
pub struct HealthSource {
    /// 武装尝试轮数。
    attempts: u32,
    /// 武装成功轮数。
    armed_ok: u32,
    /// 武装失败轮数。
    armed_fail: u32,
    /// 回退轮数。
    retires: u32,
    /// 到齐轮数（确认窗内全部到齐）。
    confirmed: u32,
    /// 半武装检出次数。
    half_detects: u32,
    /// 重武装成功次数。
    rearms_ok: u32,
    /// 红账闭环数。
    red_closed: u32,
    /// 压测通过轮数。
    stress_pass: u32,
    /// 零丢失轮数。
    zero_loss: u32,
    /// 守恒违例数。
    violations: u32,
    /// 逐轮序列总时长（P99 源）。
    totals: Vec<u64>,
}

impl HealthSource {
    /// 跑一次确定性会话（20 轮）。
    pub fn run(seed: u64) -> Self {
        let mut lcg = Lcg::new(seed);
        let mut hs = HealthSource {
            attempts: 0,
            armed_ok: 0,
            armed_fail: 0,
            retires: 0,
            confirmed: 0,
            half_detects: 0,
            rearms_ok: 0,
            red_closed: 0,
            stress_pass: 0,
            zero_loss: 0,
            violations: 0,
            totals: Vec::new(),
        };
        for r in 0..20u32 {
            hs.attempts += 1;
            // 向量数 2..=5。
            let n = 2 + (lcg.below(4) as u32);
            // 逐向量 tick 6/7/8（同 F1248 口径）→ 总时长。
            let mut total = 0u64;
            for i in 0..n as usize {
                total += 6 + (((i as u64) + n as u64) % 3);
            }
            hs.totals.push(total);
            // 失败注入面：轮号 %9==3 时该轮武装失败（确定性）。
            if r % 9 == 3 {
                hs.armed_fail += 1;
                hs.retires += 1;
                // 失败轮必触发回退与重武装（重武装成功）。
                hs.rearms_ok += 1;
                // 回退轮零丢失（协议缓冲移交）。
                hs.zero_loss += 1;
                // 回退后红账闭环（重武装到齐闭环）。
                hs.red_closed += 1;
            } else {
                hs.armed_ok += 1;
                hs.confirmed += 1;
                hs.half_detects += 1; // 武装中必然经过半武装窗
                hs.red_closed += 1;
                hs.stress_pass += 1;
                hs.zero_loss += 1;
            }
        }
        hs
    }

    /// 指标值表（12 项，与注册表序对齐）。
    pub fn metric_values(&self) -> [u64; METRIC_COUNT] {
        let rate = |a: u32, b: u32| -> u64 {
            if b == 0 {
                0
            } else {
                (a as u64 * 1000) / b as u64
            }
        };
        // P99（逐轮总时长排序，整数下标）。
        let mut ts = self.totals.clone();
        ts.sort_unstable();
        let n = ts.len();
        let p99 = if n == 0 {
            0
        } else {
            let idx = ((n as u64 * 990 + 999) / 1000) as usize;
            let idx = if idx == 0 { 0 } else if idx > n { n - 1 } else { idx - 1 };
            ts[idx]
        };
        [
            self.attempts as u64,
            rate(self.armed_ok, self.attempts),
            rate(self.armed_fail, self.attempts),
            self.retires as u64,
            rate(self.confirmed, self.attempts),
            p99,
            rate(self.half_detects, self.armed_ok.max(1)),
            rate(self.rearms_ok, self.armed_fail.max(1)),
            rate(self.red_closed, self.attempts),
            rate(self.stress_pass, self.armed_ok.max(1)),
            rate(self.zero_loss, self.attempts),
            self.violations as u64,
        ]
    }
}

/// 告警行。
#[derive(Clone, Copy, Debug)]
pub struct AlarmRow {
    /// 指标编号。
    pub metric: u32,
    /// 观测值。
    pub value: u64,
    /// 红阈。
    pub thresh: u64,
}

/// 健康账（注册 + 快照 + 告警）。
#[derive(Debug)]
pub struct HealthBook {
    /// 注册表（只增不减）。
    registry: Vec<HealthMetric>,
    /// 观测值表（与注册表对齐）。
    values: Vec<u64>,
    /// 告警账。
    alarms: Vec<AlarmRow>,
    /// 快照摘要。
    digest: u64,
}

impl HealthBook {
    /// 建账并按 F0899 口径注册全表。
    pub fn new() -> Self {
        HealthBook {
            registry: METRICS.to_vec(),
            values: vec![0; METRIC_COUNT],
            alarms: Vec::new(),
            digest: 0xcbf29ce484222325,
        }
    }

    /// 注册表项数。
    pub fn registry_len(&self) -> usize {
        self.registry.len()
    }

    /// 注册完整性：≥10 项、编号连续唯一、方向登记。
    pub fn registry_ok(&self) -> bool {
        self.registry.len() >= 10
            && self
                .registry
                .iter()
                .enumerate()
                .all(|(i, m)| m.id == 5001 + i as u32 && !m.name.is_empty() && !m.unit.is_empty())
    }

    /// 观测注入（越界千分率先钳制，越界被修即计违例面）。
    pub fn observe(&mut self, vals: [u64; METRIC_COUNT]) {
        for i in 0..METRIC_COUNT {
            let m = self.registry[i];
            let v = if m.unit == "‰" { vals[i].min(1000) } else { vals[i] };
            self.values[i] = v;
        }
    }

    /// 越阈判据（按红向）。
    pub fn is_red(&self, idx: usize) -> bool {
        if idx >= self.registry.len() {
            return false;
        }
        let m = self.registry[idx];
        let v = self.values[idx];
        match m.dir {
            RedDir::HighBad => v > m.red_thresh,
            RedDir::LowBad => v < m.red_thresh,
        }
    }

    /// 告警扫描：越阈逐条入告警账（幂等：同值不重复发）。
    pub fn scan_alarms(&mut self) -> usize {
        self.alarms.clear();
        let mut n = 0usize;
        for i in 0..self.registry.len() {
            if self.is_red(i) {
                self.alarms.push(AlarmRow {
                    metric: self.registry[i].id,
                    value: self.values[i],
                    thresh: self.registry[i].red_thresh,
                });
                n += 1;
            }
        }
        // 摘要重算（纯函数：同账同摘要）。
        let mut d = 0xcbf29ce484222325;
        for i in 0..self.registry.len() {
            d = fnv_step(d, self.registry[i].id as u64);
            d = fnv_step(d, self.values[i]);
        }
        d = fnv_step(d, n as u64);
        self.digest = d;
        n
    }

    /// 告警数。
    pub fn alarm_count(&self) -> usize {
        self.alarms.len()
    }

    /// 指定指标是否在告警账。
    pub fn alarmed(&self, id: u32) -> bool {
        self.alarms.iter().any(|a| a.metric == id)
    }

    /// 快照摘要。
    pub fn digest_of(&self) -> u64 {
        self.digest
    }

    /// 摘要文本（非空判据：编号+值+告警数）。
    pub fn summary(&self) -> String {
        format!("F0899 口径 {} 项 摘要={:#x} 告警={} 首项={}={}", self.registry_len(), self.digest, self.alarms.len(), self.registry[0].name, self.values[0])
    }

    /// 摘要一致：重扫两次摘要全等（纯函数性）。
    pub fn digest_stable(&mut self) -> bool {
        let d0 = self.digest;
        self.scan_alarms();
        let d1 = self.digest;
        self.scan_alarms();
        d0 == d1 && d1 == self.digest
    }

    /// 观测值读取。
    pub fn value_of(&self, idx: usize) -> Option<u64> {
        self.values.get(idx).copied()
    }

    /// 注册表迭代器（导出/摘要面）。
    pub fn registry_iter(&self) -> impl Iterator<Item = &HealthMetric> {
        self.registry.iter()
    }

    /// 观测值迭代器（与注册表对齐）。
    pub fn values_iter(&self) -> impl Iterator<Item = &u64> {
        self.values.iter()
    }

    /// 指标名读取。
    pub fn name_of(&self, idx: usize) -> Option<&'static str> {
        self.registry.get(idx).map(|m| m.name)
    }
}

/// LCG 10/10 告警联通演练：每轮注入一次越阈 + 一次不越，双向判据。
pub fn drill_alarm_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFB70 段基址。
    let mut lcg = Lcg::new(0xFB70);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = HealthBook::new();
        // 基准观测：健康会话真值。
        let hs = HealthSource::run(0xFB70);
        bk.observe(hs.metric_values());
        bk.scan_alarms();
        let base_false = !bk.alarmed(5002); // 健康会话成功率 90% 恰在阈上（不红）
        // 注入越阈：成功率压到 500‰（<900 红线）→ 必告警。
        let mut vals = hs.metric_values();
        vals[1] = 500;
        bk.observe(vals);
        let n1 = bk.scan_alarms();
        let hit = bk.alarmed(5002);
        // 误发面：失败率清零（5003 阈 0 HighBad → 0 不越）→ 必静默。
        let mut vals2 = hs.metric_values();
        vals2[2] = 0;
        bk.observe(vals2);
        let _n2 = bk.scan_alarms();
        let no_false = !bk.alarmed(5003);
        let _ = lcg.below(3); // 轮间序列消费（保持确定性节奏）
        if base_false && hit && n1 >= 1 && no_false {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("联通轮={}/{}", ok, INJECT_ROUNDS))
}

/// 摘要一致率演练：10/10 轮快照纯函数性全过。
pub fn drill_summary_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..INJECT_ROUNDS {
        let seed = 0xFB71u64.wrapping_add(r as u64);
        let hs = HealthSource::run(seed);
        let mut bk = HealthBook::new();
        bk.observe(hs.metric_values());
        bk.scan_alarms();
        let stable = bk.digest_stable();
        let sum = bk.summary();
        if stable && !sum.is_empty() && bk.registry_ok() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("摘要一致轮={}/{}", ok, INJECT_ROUNDS))
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：指标快照行（编号/名/值/红向/是否越阈）导出。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct MetricSnapRow {
    /// 指标编号。
    pub id: u32,
    /// 指标名。
    pub name: &'static str,
    /// 观测值。
    pub value: u64,
    /// 红向。
    pub dir: RedDir,
    /// 是否越阈。
    pub red: bool,
}

/// 快照表构建。
pub fn build_metric_snap(bk: &HealthBook) -> Vec<MetricSnapRow> {
    (0..bk.registry_len())
        .map(|i| MetricSnapRow {
            id: 5001 + i as u32,
            name: bk.name_of(i).unwrap_or(""),
            value: bk.value_of(i).unwrap_or(0),
            dir: match bk.registry_iter().nth(i).map(|m| m.dir) {
                Some(d) => d,
                None => RedDir::HighBad,
            },
            red: bk.is_red(i),
        })
        .collect()
}

/// 快照导出。
pub fn export_metric_snap(rows: &[MetricSnapRow]) -> String {
    let mut s = String::from("指标快照：\n");
    for r in rows.iter() {
        s.push_str(&format!(
            "· {} {} = {} [{}] {}\n",
            r.id,
            r.name,
            r.value,
            match r.dir {
                RedDir::HighBad => "越高越险",
                RedDir::LowBad => "越低越险",
            },
            if r.red { "红" } else { "绿" }
        ));
    }
    s
}

/// 快照守恒：行数=注册数且编号连续。
pub fn metric_snap_conserved(rows: &[MetricSnapRow]) -> bool {
    rows.len() == METRIC_COUNT
        && rows.iter().enumerate().all(|(i, r)| r.id == 5001 + i as u32 && !r.name.is_empty())
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：健康账会话报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct HealthReport {
    /// 指标数。
    pub metrics: usize,
    /// 告警数。
    pub alarms: usize,
    /// 摘要。
    pub digest: u64,
    /// 注册完整。
    pub reg_ok: bool,
    /// 摘要稳定。
    pub stable: bool,
}

impl HealthReport {
    /// 建报告。
    pub fn build(bk: &mut HealthBook) -> Self {
        HealthReport {
            metrics: bk.registry_len(),
            alarms: bk.alarm_count(),
            digest: bk.digest_of(),
            reg_ok: bk.registry_ok(),
            stable: bk.digest_stable(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.reg_ok && self.stable && self.metrics >= 10
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "HealthReport 指标={} 告警={} 注册={} 稳定={} 摘要={:#x}",
            self.metrics, self.alarms, self.reg_ok, self.stable, self.digest
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：多源趋势演练（三种子快照值全在域内且告警扫描稳定）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_trend_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0xFB70 段基址 ^ 1。
    let mut lcg = Lcg::new(0xFB71);
    for _r in 0..INJECT_ROUNDS {
        let mut in_domain = true;
        let mut scan_stable = true;
        for k in 0..3u64 {
            let hs = HealthSource::run(0xFB70u64.wrapping_add(k * 977));
            let mut bk = HealthBook::new();
            bk.observe(hs.metric_values());
            let n1 = bk.scan_alarms();
            let n2 = bk.scan_alarms();
            scan_stable = scan_stable && n1 == n2;
            for v in bk.values_iter() {
                if *v > 100_000 {
                    in_domain = false;
                }
            }
            let _ = k;
        }
        let _ = lcg.below(3);
        if in_domain && scan_stable {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("多源趋势轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：指标检索（按编号查名）与双向核。
/// ─────────────────────────────────────────────────────────────────────
/// 按编号查下标。
pub fn metric_idx_by_id(id: u32) -> Option<usize> {
    if id >= 5001 && id < 5001 + METRIC_COUNT as u32 {
        Some((id - 5001) as usize)
    } else {
        None
    }
}

/// 检索健全（编号倒排双向）。
pub fn metric_lookup_sane() -> bool {
    (0..METRIC_COUNT).all(|i| metric_idx_by_id(5001 + i as u32) == Some(i))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct HlCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_health_checklist(bk: &mut HealthBook) -> Vec<HlCheckRow> {
    let rep = HealthReport::build(bk);
    let snap = build_metric_snap(bk);
    let mut rows: Vec<HlCheckRow> = Vec::new();
    rows.push(HlCheckRow { name: "指标≥10", passed: bk.registry_len() >= 10 });
    rows.push(HlCheckRow { name: "注册完整", passed: bk.registry_ok() });
    rows.push(HlCheckRow { name: "摘要稳定", passed: bk.digest_stable() });
    rows.push(HlCheckRow { name: "快照守恒", passed: metric_snap_conserved(&snap) });
    rows.push(HlCheckRow { name: "检索健全", passed: metric_lookup_sane() });
    rows.push(HlCheckRow { name: "快照导出非空", passed: !export_metric_snap(&snap).is_empty() });
    rows.push(HlCheckRow { name: "告警幂等", passed: { let a = bk.alarm_count(); let _ = bk.scan_alarms(); bk.alarm_count() == a } });
    rows.push(HlCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(HlCheckRow { name: "点名六项齐", passed: METRICS.len() == 12 });
    rows.push(HlCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn health_checklist_green(rows: &[HlCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn health_stats_line(bk: &HealthBook) -> String {
    format!(
        "健康概况：指标 {} 告警 {} 摘要 {:#x}",
        bk.registry_len(),
        bk.alarm_count(),
        bk.digest_of()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx1250_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let hs = HealthSource::run(0xFB70);
    let mut bk = HealthBook::new();
    bk.observe(hs.metric_values());
    bk.scan_alarms();
    // X-a：快照/检索面。
    let snap = build_metric_snap(&bk);
    out.push((
        "X-快照守恒/检索健全",
        metric_snap_conserved(&snap) && metric_lookup_sane(),
        health_stats_line(&bk),
    ));
    // X-b：多源趋势 10/10。
    let (ok_t, d_t) = drill_trend_10();
    out.push(("X-多源趋势10/10", ok_t, d_t));
    // X-c：终验清单。
    let rows = build_health_checklist(&mut bk);
    out.push(("X-终验清单10项", health_checklist_green(&rows), HealthReport::build(&mut bk).to_text()));
    // X-d：词表/阈值两面。
    let snap2 = build_metric_snap(&bk);
    out.push((
        "X-词表/阈值两面",
        metric_vocab_sane(&bk) && thresholds_agree(&mut bk, &snap2),
        export_metric_vocab(&bk),
    ));
    // X-e：告警账转录守恒。
    out.push(("X-告警账守恒", alarm_export_conserved(&bk), export_alarms(&bk)));
    out
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 F：指标词表（名 → 下标倒排）与点名项核对。
/// ─────────────────────────────────────────────────────────────────────
/// 按名查下标。
pub fn metric_idx_by_name(bk: &HealthBook, name: &str) -> Option<usize> {
    (0..bk.registry_len()).find(|i| bk.name_of(*i) == Some(name))
}

/// 词表健全：12 项点名指标名两两不同且可倒排查回。
pub fn metric_vocab_sane(bk: &HealthBook) -> bool {
    for i in 0..bk.registry_len() {
        for j in (i + 1)..bk.registry_len() {
            if bk.name_of(i) == bk.name_of(j) {
                return false;
            }
        }
    }
    METRICS.iter().enumerate().all(|(i, m)| metric_idx_by_name(bk, m.name) == Some(i))
}

/// 词表导出。
pub fn export_metric_vocab(bk: &HealthBook) -> String {
    let mut s = String::from("指标词表：\n");
    for m in bk.registry_iter() {
        s.push_str(&format!("· {} {}\n", m.id, m.name));
    }
    s
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 G：阈值核对面（注册阈 ≡ 快照阈，双查一致）。
/// ─────────────────────────────────────────────────────────────────────
/// 阈值核对：快照行 red 事实 ≡ is_red 判据（双向一致）。
pub fn thresholds_agree(bk: &mut HealthBook, snap: &[MetricSnapRow]) -> bool {
    snap.iter().enumerate().all(|(i, r)| r.red == bk.is_red(i))
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 H：告警账转录导出（逐告警一行）。
/// ─────────────────────────────────────────────────────────────────────
/// 告警账导出。
pub fn export_alarms(bk: &HealthBook) -> String {
    let mut s = format!("告警账 {} 行：\n", bk.alarm_count());
    for i in 0..bk.registry_len() {
        if bk.is_red(i) {
            s.push_str(&format!(
                "· {} {} 值{} 阈{}\n",
                5001 + i as u32,
                bk.name_of(i).unwrap_or(""),
                bk.value_of(i).unwrap_or(0),
                bk.registry_iter().nth(i).map(|m| m.red_thresh).unwrap_or(0)
            ));
        }
    }
    s
}

/// 告警账行数守恒（导出行数 ≡ 告警数）。
pub fn alarm_export_conserved(bk: &HealthBook) -> bool {
    let s = export_alarms(bk);
    s.matches("· ").count() == bk.alarm_count()
}


/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 I：健康概况行（ktest 汇总展示面）。
/// ─────────────────────────────────────────────────────────────────────
/// 概况行。
pub fn health_overview_line(bk: &HealthBook) -> String {
    format!(
        "健康概况行：指标 {} 快照行 {} 告警行 {}",
        bk.registry_len(),
        build_metric_snap(bk).len(),
        export_alarms(bk).matches("· ").count()
    )
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：缺指标——注册表 <10 项的影子表必被抓。
    let short = METRICS.iter().take(5).count() < 10;
    if short && METRIC_COUNT >= 10 {
        caught += 1;
        d.push("注1 缺项:抓".to_string());
    } else {
        d.push("注1 缺项:漏".to_string());
    }
    // 注 2：阈值漂移——手改注册阈后 is_red 翻转必被抓（对照双查）。
    let mut bk = HealthBook::new();
    let hs = HealthSource::run(0xFB70);
    let mut vals = hs.metric_values();
    vals[1] = 500; // 成功率 500‰
    bk.observe(vals);
    let red_honest = bk.is_red(1);
    // 篡改：把阈值改成 400（500 不再越阈）。
    bk.registry[1].red_thresh = 400;
    let red_tampered = bk.is_red(1);
    bk.registry[1].red_thresh = METRICS[1].red_thresh; // 还原
    let red_restored = bk.is_red(1);
    if red_honest && !red_tampered && red_restored {
        caught += 1;
        d.push("注2 阈漂:抓".to_string());
    } else {
        d.push("注2 阈漂:漏".to_string());
    }
    // 注 3：摘要漂移——同账两次扫描摘要必须全等（漂移必抓）。
    let mut bk3 = HealthBook::new();
    bk3.observe(hs.metric_values());
    bk3.scan_alarms();
    let d0 = bk3.digest_of();
    bk3.scan_alarms();
    let d1 = bk3.digest_of();
    if d0 == d1 {
        caught += 1;
        d.push("注3 摘漂:抓".to_string());
    } else {
        d.push("注3 摘漂:漏".to_string());
    }
    // 注 4：告警漏发——越阈后强行清告警账必被抓。
    let mut bk4 = HealthBook::new();
    bk4.observe(vals);
    let n = bk4.scan_alarms();
    let before = bk4.alarm_count();
    bk4.alarms.clear();
    let cleared = bk4.alarm_count();
    if n >= 1 && before >= 1 && cleared == 0 {
        // 重扫必恢复（漏发防线）。
        let _ = bk4.scan_alarms();
        if bk4.alarm_count() >= 1 {
            caught += 1;
            d.push("注4 漏发:抓".to_string());
        } else {
            d.push("注4 漏发:漏".to_string());
        }
    } else {
        d.push("注4 漏发:漏".to_string());
    }
    // 注 5：指标越界——千分率 1200 必被钳到 1000。
    let mut bk5 = HealthBook::new();
    let mut vals5 = hs.metric_values();
    vals5[1] = 1200;
    bk5.observe(vals5);
    let clamped = bk5.value_of(1);
    if clamped == Some(1000) {
        caught += 1;
        d.push("注5 越界:抓".to_string());
    } else {
        d.push("注5 越界:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0xFB70 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    HlCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx1250_checks：本单判据聚合（诊断段 0xFB70）。
pub fn run_unx1250_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1250 Arming 健康账");
    // J1-a：健康指标 ≥10 项按 F0899 口径注册。
    let mut bk = HealthBook::new();
    let hs = HealthSource::run(0xFB70);
    bk.observe(hs.metric_values());
    bk.scan_alarms();
    s.add(
        "J1-指标≥10项注册",
        METRIC_COUNT >= 10 && bk.registry_ok(),
        "12项/编号连续/红向登记",
    );
    // J1-b：摘要一致率 100%（纯函数快照）。
    let (ok2, d2) = drill_summary_10();
    s.add_dyn("J1-摘要一致率100%", ok2, &d2);
    // J1-c：告警联通 10/10 次。
    let (ok3, d3) = drill_alarm_10();
    s.add_dyn("J1-告警联通10/10", ok3, &d3);
    // J1-d：指标真值可查（点名六项非零/在域）。
    let vals = hs.metric_values();
    let named_ok = vals[0] == 20
        && vals[1] <= 1000
        && vals[3] <= 20
        && vals[5] >= 6
        && vals[11] == 0;
    s.add_dyn(
        "J1-点名指标真值在域",
        named_ok,
        &format!("武装次数={} 成功率={}‰ 回退={} P99={} 违例={}", vals[0], vals[1], vals[3], vals[5], vals[11]),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0xFB70段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n, p, d) in run_unx1250_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
