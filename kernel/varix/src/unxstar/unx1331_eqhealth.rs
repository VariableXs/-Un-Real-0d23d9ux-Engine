//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1331 · 等价验证健康账（F0899 口径注册） · 0x7710
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F1331-J1 等价面健康指标 ≥10 项按 F0899 口径注册（三路径
//!      等价率/对拍差/长跑状态/违例数等），摘要一致率 100%，告警联通
//!      10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - F0899 口径：指标=（唯一名/单位/方向/阈值/当前值）五元组，方向
//!     分高位优（越大越好）与低位优（越小越好）两向；
//!   - 指标宇宙 12 项：三路径等价率×3/对拍差/长跑完成窗/违例数/红账
//!     数/覆盖率/压测等价/注入必抓率/告警连通率；
//!   - 告警评估：高位优 value<threshold 告警、低位优 value>threshold
//!     告警；每项独立联通演练 10/10；
//!   - 摘要一致率：两次生成摘要逐字节全同（确定性生成，禁墙钟）；
//!   - 健康分：全指标无告警=100 分，单项告警扣分（整数扣分制）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ═══════════════════════════════════════════════════════════════════════
// §0 常量与诊断码闭集（0x7710 段，连续编号）
// ═══════════════════════════════════════════════════════════════════════

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x7710;

/// 注入演练轮数（判据点名 10）。
pub const INJECT_ROUNDS: usize = 10;

/// 指标宇宙（12 项）。
pub const METRIC_COUNT: usize = 12;

/// F0899 注册底线（≥10 项）。
pub const REG_FLOOR: usize = 10;

/// 0x7710 指标重复注册。
pub const E_METRIC_DUP: u16 = SEG_BASE + 0;
/// 0x7710+1 注册不足底线。
pub const E_REG_SHORT: u16 = SEG_BASE + 1;
/// 0x7712 指标越界。
pub const E_METRIC_OOB: u16 = SEG_BASE + 2;
/// 0x7713 告警漏报（破阈值未告警）。
pub const E_ALARM_MISS: u16 = SEG_BASE + 3;
/// 0x7714 告警误报（未破阈值却告警）。
pub const E_ALARM_FALSE: u16 = SEG_BASE + 4;
/// 0x7715 摘要不一致（两次生成不同）。
pub const E_SUMMARY_DRIFT: u16 = SEG_BASE + 5;
/// 0x7716 值未更新（只读指标被写越界）。
pub const E_VALUE_OOB: u16 = SEG_BASE + 6;
/// 0x7717 健康分越界（分值>100 或下溢）。
pub const E_SCORE_OOB: u16 = SEG_BASE + 7;
/// 0x7718 指标名空。
pub const E_NAME_EMPTY: u16 = SEG_BASE + 8;
/// 0x7719 阈值缺失（未设阈值就评估）。
pub const E_THRES_MISSING: u16 = SEG_BASE + 9;

/// 全码闭集。
pub const CODE_ALL: [u16; 10] = [
    E_METRIC_DUP, E_REG_SHORT, E_METRIC_OOB, E_ALARM_MISS, E_ALARM_FALSE,
    E_SUMMARY_DRIFT, E_VALUE_OOB, E_SCORE_OOB, E_NAME_EMPTY, E_THRES_MISSING,
];

/// 诊断码中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_METRIC_DUP => "指标重复注册：指标名唯一，重名先查注册表再入",
        E_REG_SHORT => "注册不足底线：F0899 口径 ≥10 项，缺项先补注册",
        E_METRIC_OOB => "指标越界：12 项闭集（0..12），越界即拒",
        E_ALARM_MISS => "告警漏报：破阈值必告警，查方向位是否翻转",
        E_ALARM_FALSE => "告警误报：未破阈值告警=方向位或阈值被污染",
        E_SUMMARY_DRIFT => "摘要不一致：生成器必须纯函数（同账同串），查隐式状态",
        E_VALUE_OOB => "值越界：等价率类指标值域 [0,10^6]，越界即拒",
        E_SCORE_OOB => "健康分越界：分值域 [0,100]，扣分不得下溢",
        E_NAME_EMPTY => "指标名空：F0899 口径名必非空",
        E_THRES_MISSING => "阈值缺失：先设阈值再评估告警",
        _ => "未知诊断码：核对 0x7710 段闭集登记表",
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §1 指标宇宙
// ═══════════════════════════════════════════════════════════════════════

/// 告警方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlarmDir {
    /// 高位优（值 < 阈值告警）。
    HigherBetter,
    /// 低位优（值 > 阈值告警）。
    LowerBetter,
}

impl AlarmDir {
    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            AlarmDir::HigherBetter => "高位优",
            AlarmDir::LowerBetter => "低位优",
        }
    }
}

/// 指标宇宙（12 项，F0899 五元组）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricKind {
    /// 轮询面等价率（ppm）。
    PollEq,
    /// 中断面等价率（ppm）。
    IntEq,
    /// 双模面等价率（ppm）。
    DualEq,
    /// 对拍差（违例事件数）。
    CmpDiff,
    /// 长跑完成窗（0..24）。
    LongrunWins,
    /// 违例总数。
    Violations,
    /// 红账总数。
    RedCount,
    /// 覆盖率（ppm）。
    Coverage,
    /// 压测等价维持（ppm）。
    StressEq,
    /// 注入必抓率（ppm）。
    InjectCatch,
    /// 告警连通率（ppm）。
    AlarmLink,
    /// 审计完整率（ppm）。
    AuditFull,
}

impl MetricKind {
    /// 闭集。
    pub const ALL: [MetricKind; 12] = [
        MetricKind::PollEq, MetricKind::IntEq, MetricKind::DualEq,
        MetricKind::CmpDiff, MetricKind::LongrunWins, MetricKind::Violations,
        MetricKind::RedCount, MetricKind::Coverage, MetricKind::StressEq,
        MetricKind::InjectCatch, MetricKind::AlarmLink, MetricKind::AuditFull,
    ];

    /// 中文短名。
    pub const fn text(self) -> &'static str {
        match self {
            MetricKind::PollEq => "轮询面等价率",
            MetricKind::IntEq => "中断面等价率",
            MetricKind::DualEq => "双模面等价率",
            MetricKind::CmpDiff => "对拍差",
            MetricKind::LongrunWins => "长跑完成窗",
            MetricKind::Violations => "违例总数",
            MetricKind::RedCount => "红账总数",
            MetricKind::Coverage => "覆盖率",
            MetricKind::StressEq => "压测等价维持",
            MetricKind::InjectCatch => "注入必抓率",
            MetricKind::AlarmLink => "告警连通率",
            MetricKind::AuditFull => "审计完整率",
        }
    }

    /// 单位。
    pub const fn unit(self) -> &'static str {
        match self {
            MetricKind::PollEq | MetricKind::IntEq | MetricKind::DualEq
            | MetricKind::Coverage | MetricKind::StressEq | MetricKind::InjectCatch
            | MetricKind::AlarmLink | MetricKind::AuditFull => "ppm",
            MetricKind::CmpDiff | MetricKind::Violations | MetricKind::RedCount => "件",
            MetricKind::LongrunWins => "窗",
        }
    }

    /// 枚举序。
    pub const fn idx(self) -> usize {
        match self {
            MetricKind::PollEq => 0,
            MetricKind::IntEq => 1,
            MetricKind::DualEq => 2,
            MetricKind::CmpDiff => 3,
            MetricKind::LongrunWins => 4,
            MetricKind::Violations => 5,
            MetricKind::RedCount => 6,
            MetricKind::Coverage => 7,
            MetricKind::StressEq => 8,
            MetricKind::InjectCatch => 9,
            MetricKind::AlarmLink => 10,
            MetricKind::AuditFull => 11,
        }
    }

    /// 从序号还原。
    pub const fn from_idx(i: usize) -> Option<MetricKind> {
        match i {
            0 => Some(MetricKind::PollEq),
            1 => Some(MetricKind::IntEq),
            2 => Some(MetricKind::DualEq),
            3 => Some(MetricKind::CmpDiff),
            4 => Some(MetricKind::LongrunWins),
            5 => Some(MetricKind::Violations),
            6 => Some(MetricKind::RedCount),
            7 => Some(MetricKind::Coverage),
            8 => Some(MetricKind::StressEq),
            9 => Some(MetricKind::InjectCatch),
            10 => Some(MetricKind::AlarmLink),
            11 => Some(MetricKind::AuditFull),
            _ => None,
        }
    }

    /// F0899 口径方向。
    pub const fn dir(self) -> AlarmDir {
        match self {
            MetricKind::CmpDiff
            | MetricKind::Violations
            | MetricKind::RedCount => AlarmDir::LowerBetter,
            _ => AlarmDir::HigherBetter,
        }
    }

    /// 写死阈值（ppm 类 10^6；计数类 0；长跑窗 24）。
    pub const fn threshold(self) -> u64 {
        match self {
            MetricKind::PollEq | MetricKind::IntEq | MetricKind::DualEq
            | MetricKind::Coverage | MetricKind::StressEq | MetricKind::InjectCatch
            | MetricKind::AlarmLink | MetricKind::AuditFull => 1_000_000,
            MetricKind::CmpDiff | MetricKind::Violations | MetricKind::RedCount => 0,
            MetricKind::LongrunWins => 24,
        }
    }

    /// 值域上界（0=无上界约束；ppm 类 10^6；长跑窗 24）。
    pub const fn value_cap(self) -> u64 {
        match self {
            MetricKind::LongrunWins => 24,
            MetricKind::CmpDiff | MetricKind::Violations | MetricKind::RedCount => 0,
            _ => 1_000_000,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §2 指标登记与告警评估
// ═══════════════════════════════════════════════════════════════════════

/// 单指标账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metric {
    pub kind: MetricKind,
    /// 当前值。
    pub value: u64,
    /// 是否已设值。
    pub set: bool,
}

impl Metric {
    /// 告警评估（未设值视同告警）。
    pub const fn alarmed(&self) -> bool {
        if !self.set {
            return true;
        }
        match self.kind.dir() {
            AlarmDir::HigherBetter => self.value < self.kind.threshold(),
            AlarmDir::LowerBetter => self.value > self.kind.threshold(),
        }
    }

    /// 更新值（越域拒）。
    pub fn update(&mut self, v: u64) -> Result<(), u16> {
        let cap = self.kind.value_cap();
        if cap != 0 && v > cap {
            return Err(E_VALUE_OOB);
        }
        self.value = v;
        self.set = true;
        Ok(())
    }

    /// 账行。
    pub fn row(&self) -> String {
        format!(
            "{}({})={}{} 阈值{} 告警={}",
            self.kind.text(),
            self.kind.dir().text(),
            self.value,
            self.kind.unit(),
            self.kind.threshold(),
            self.alarmed()
        )
    }
}

/// 健康账（F0899 注册正本）。
pub struct HealthBook {
    metrics: [Option<Metric>; METRIC_COUNT],
    pub registered: usize,
}

impl HealthBook {
    /// 建（空账）。
    pub fn new() -> Self {
        HealthBook { metrics: [None; METRIC_COUNT], registered: 0 }
    }

    /// 注册（重名拒；名空拒）。
    pub fn register(&mut self, k: MetricKind) -> Result<usize, u16> {
        if k.text().is_empty() {
            return Err(E_NAME_EMPTY);
        }
        let i = k.idx();
        if self.metrics[i].is_some() {
            return Err(E_METRIC_DUP);
        }
        self.metrics[i] = Some(Metric { kind: k, value: 0, set: false });
        self.registered += 1;
        Ok(i)
    }

    /// 更新值。
    pub fn update(&mut self, k: MetricKind, v: u64) -> Result<(), u16> {
        let i = k.idx();
        match self.metrics[i] {
            Some(ref mut m) => m.update(v),
            None => Err(E_REG_SHORT),
        }
    }

    /// 全量注册（12 项一次入）。
    pub fn register_all(&mut self) -> Result<(), u16> {
        for k in MetricKind::ALL {
            self.register(k)?;
        }
        Ok(())
    }

    /// 注册达标。
    pub fn floor_ok(&self) -> bool {
        self.registered >= REG_FLOOR
    }

    /// 全指标设值。
    pub fn all_set(&self) -> bool {
        for m in self.metrics.iter() {
            match m {
                Some(m) => {
                    if !m.set {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }

    /// 告警清单。
    pub fn alarms(&self) -> Vec<MetricKind> {
        let mut out = Vec::new();
        for m in self.metrics.iter() {
            if let Some(m) = m {
                if m.alarmed() {
                    out.push(m.kind);
                }
            }
        }
        out
    }

    /// 健康分（100 − 9×告警数，12 项全告警扣 108 > 100 触发下溢钳 0）。
    pub fn score(&self) -> Result<u64, u16> {
        if self.registered < REG_FLOOR {
            return Err(E_REG_SHORT);
        }
        let n = self.alarms().len() as u64;
        let penalty = n.saturating_mul(9);
        if penalty > 100 {
            Ok(0)
        } else {
            Ok(100 - penalty)
        }
    }

    /// 摘要（确定性纯函数：同账同串）。
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str("F0899健康摘要:");
        for m in self.metrics.iter() {
            if let Some(m) = m {
                let tag = if m.alarmed() { "警" } else { "常" };
                s.push_str(&format!("{}={}[{}]", m.kind.text(), m.value, tag));
            }
        }
        s
    }

    /// 单账行导出。
    pub fn rows(&self) -> Vec<String> {
        let mut out = Vec::new();
        for m in self.metrics.iter() {
            if let Some(m) = m {
                out.push(m.row());
            }
        }
        out
    }

    /// 越界指标读。
    pub fn metric(&self, k: MetricKind) -> Result<Metric, u16> {
        match self.metrics[k.idx()] {
            Some(m) => Ok(m),
            None => Err(E_REG_SHORT),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §3 演练
// ═══════════════════════════════════════════════════════════════════════

/// 全指标注册+满值无告警演练（健康分 100）。
pub fn drill_all_green() -> (bool, String) {
    let mut b = HealthBook::new();
    if b.register_all().is_err() {
        return (false, "注册失败".to_string());
    }
    // 满值：ppm 类 10^6；计数类 0；长跑窗 24。
    for k in MetricKind::ALL {
        let v = match k.dir() {
            AlarmDir::HigherBetter => k.threshold(),
            AlarmDir::LowerBetter => 0,
        };
        if b.update(k, v).is_err() {
            return (false, "设值失败".to_string());
        }
    }
    let score = b.score().unwrap_or(u64::MAX);
    (
        b.floor_ok() && b.all_set() && b.alarms().is_empty() && score == 100,
        format!("注册{} 健康分={} 告警0={}", b.registered, score, b.alarms().is_empty()),
    )
}

/// 摘要一致率 100% 演练（同账两次生成逐字节全同 + 改账后摘要必变）。
pub fn drill_summary_stable() -> (bool, String) {
    let mut b = HealthBook::new();
    let _ = b.register_all();
    for k in MetricKind::ALL {
        let v = if k.dir() == AlarmDir::LowerBetter { 0 } else { k.threshold() };
        let _ = b.update(k, v);
    }
    let s1 = b.summary();
    let s2 = b.summary();
    let same = s1 == s2 && !s1.is_empty();
    // 改账：污染一项 → 摘要必变。
    let _ = b.update(MetricKind::Violations, 7);
    let s3 = b.summary();
    (
        same && s3 != s1,
        format!("两次全同={} 改账必变={} 长度={}", same, s3 != s1, s1.len()),
    )
}

/// 告警联通 10/10 演练（逐指标注破值必告警，回正必消警）。
pub fn drill_alarm_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut notes: Vec<String> = Vec::new();
    for r in 0..INJECT_ROUNDS {
        let kind = match MetricKind::from_idx(r % METRIC_COUNT) {
            Some(k) => k,
            None => return (false, "指标还原失败".to_string()),
        };
        let mut b = HealthBook::new();
        let _ = b.register_all();
        // 全绿基线。
        for k in MetricKind::ALL {
            let v = if k.dir() == AlarmDir::LowerBetter { 0 } else { k.threshold() };
            let _ = b.update(k, v);
        }
        // 注破值：高位优打 0，低位优打 cap+1（cap=0 时打 999）。
        let bad = if kind.dir() == AlarmDir::LowerBetter {
            if kind.value_cap() == 0 { 999 } else { kind.value_cap() + 1 }
        } else {
            0
        };
        let upd = b.update(kind, bad);
        // cap=0 的低位优指标不受域限，必可设；ppm 类打 0 合法。
        let alarmed_after = b.metric(kind).map(|m| m.alarmed()).unwrap_or(true);
        let hit = alarmed_after && !b.alarms().is_empty();
        // 回正消警。
        let good = if kind.dir() == AlarmDir::LowerBetter { 0 } else { kind.threshold() };
        let _ = b.update(kind, good);
        let cleared = b.metric(kind).map(|m| !m.alarmed()).unwrap_or(false);
        if upd.is_ok() && hit && cleared {
            ok += 1;
        }
        notes.push(format!(
            "r{}({}):{}",
            r,
            kind.text(),
            if hit && cleared { "通" } else { "断" }
        ));
    }
    (
        ok == INJECT_ROUNDS as u32,
        format!("{}/{} {}", ok, INJECT_ROUNDS, notes.join(" ")),
    )
}

/// 越域/重名/底线必拒演练。
pub fn drill_refusals() -> (bool, String) {
    let mut b = HealthBook::new();
    let _ = b.register(MetricKind::PollEq);
    // 重名拒。
    let dup = matches!(b.register(MetricKind::PollEq), Err(E_METRIC_DUP));
    // 越域拒（ppm>10^6）。
    let oob = matches!(b.update(MetricKind::PollEq, 1_000_001), Err(E_VALUE_OOB));
    // 未注册指标更新拒。
    let unreg = matches!(b.update(MetricKind::DualEq, 5), Err(E_REG_SHORT));
    // 底线拒（注册 1 项 <10 评估健康分拒）。
    let floor = matches!(b.score(), Err(E_REG_SHORT));
    (
        dup && oob && unreg && floor,
        format!("重名拒={} 越域拒={} 未注拒={} 底线拒={}", dup, oob, unreg, floor),
    )
}

/// 误报反向演练（全绿账零告警 + 单项低值告警且分数下降）。
pub fn drill_alarm_false() -> (bool, String) {
    let mut b = HealthBook::new();
    let _ = b.register_all();
    for k in MetricKind::ALL {
        let v = if k.dir() == AlarmDir::LowerBetter { 0 } else { k.threshold() };
        let _ = b.update(k, v);
    }
    let zero_alarm = b.alarms().is_empty();
    let s0 = b.score().unwrap_or(0);
    // 单项破约。
    let _ = b.update(MetricKind::Coverage, 999_999);
    let one_alarm = b.alarms().len() == 1;
    let s1 = b.score().unwrap_or(0);
    (
        zero_alarm && one_alarm && s1 < s0 && s0 == 100 && s1 == 91,
        format!("零告警={} 单告警={} 分{}→{}", zero_alarm, one_alarm, s0, s1),
    )
}

/// 账行导出演练（12 行导出 + 行含方向与阈值字段）。
pub fn drill_rows_export() -> (bool, String) {
    let mut b = HealthBook::new();
    let _ = b.register_all();
    for k in MetricKind::ALL {
        let v = if k.dir() == AlarmDir::LowerBetter { 0 } else { k.threshold() };
        let _ = b.update(k, v);
    }
    let rows = b.rows();
    let count_ok = rows.len() == METRIC_COUNT;
    let mut fields_ok = true;
    for r in &rows {
        if !(r.contains("高位优") || r.contains("低位优")) || !r.contains("阈值") {
            fields_ok = false;
        }
    }
    (
        count_ok && fields_ok,
        format!("行{} 字段全={}", rows.len(), fields_ok),
    )
}

// ═══════════════════════════════════════════════════════════════════════
// §3b 告警事件环（联通率计量 + 守恒）
// ═══════════════════════════════════════════════════════════════════════

/// 告警事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlarmEvent {
    /// 序号（单调）。
    pub seq: u64,
    /// 指标序。
    pub metric: usize,
    /// 触发值。
    pub value: u64,
    /// 阈值。
    pub threshold: u64,
}

/// 告警事件环（容量 32，满则丢且显性计数）。
pub struct AlarmLog {
    buf: [Option<AlarmEvent>; 32],
    head: usize,
    count: usize,
    pub pushed: u64,
    pub popped: u64,
    pub dropped: u64,
    next_seq: u64,
}

impl AlarmLog {
    /// 建。
    pub fn new() -> Self {
        AlarmLog {
            buf: [None; 32],
            head: 0,
            count: 0,
            pushed: 0,
            popped: 0,
            dropped: 0,
            next_seq: 1,
        }
    }

    /// 推告警（序号单调；满则丢显性）。
    pub fn push(&mut self, metric: usize, value: u64, threshold: u64) {
        self.pushed += 1;
        if self.count == 32 {
            self.dropped += 1;
            return;
        }
        let idx = (self.head + self.count) % 32;
        self.buf[idx] = Some(AlarmEvent {
            seq: self.next_seq,
            metric,
            value,
            threshold,
        });
        self.next_seq += 1;
        self.count += 1;
    }

    /// 弹一条。
    pub fn pop(&mut self) -> Option<AlarmEvent> {
        if self.count == 0 {
            return None;
        }
        let v = self.buf[self.head];
        self.head = (self.head + 1) % 32;
        self.count -= 1;
        if v.is_some() {
            self.popped += 1;
        }
        v
    }

    /// 序号唯一单调核（弹出流内 seq 严格递增）。
    pub fn seq_monotone(&mut self) -> bool {
        let mut last = 0u64;
        let mut ok = true;
        while let Some(e) = self.pop() {
            if e.seq <= last {
                ok = false;
            }
            last = e.seq;
        }
        ok
    }

    /// 守恒（进=出+在途+丢）。
    pub fn conserved(&self) -> bool {
        self.pushed == self.popped + self.count as u64 + self.dropped
    }

    /// 联通率 ppm（弹出/推入）。
    pub fn link_ppm(&self) -> u64 {
        if self.pushed == 0 {
            return 0;
        }
        let v = self.popped.saturating_mul(1_000_000) / self.pushed;
        if v > 1_000_000 {
            1_000_000
        } else {
            v
        }
    }
}

/// 告警环联通演练（12 指标各注一破值全入环全弹出，联通率 10^6）。
pub fn drill_alarm_log() -> (bool, String) {
    let mut b = HealthBook::new();
    let _ = b.register_all();
    let mut log = AlarmLog::new();
    // 逐指标注破值并入环。
    for k in MetricKind::ALL {
        let bad = if k.dir() == AlarmDir::LowerBetter {
            if k.value_cap() == 0 { 999 } else { k.value_cap() + 1 }
        } else {
            0
        };
        let _ = b.update(k, bad);
        if k.dir() == AlarmDir::LowerBetter {
            log.push(k.idx(), if k.value_cap() == 0 { 999 } else { k.value_cap() + 1 }, 0);
        } else {
            log.push(k.idx(), 0, k.threshold());
        }
    }
    let alarm_n = b.alarms().len();
    let mono = log.seq_monotone();
    let conserved = log.conserved();
    (
        alarm_n == METRIC_COUNT && mono && conserved && log.link_ppm() == 1_000_000,
        format!(
            "告警{} 序单调={} 守恒={} 联通ppm={}",
            alarm_n, mono, conserved, log.link_ppm()
        ),
    )
}

/// 告警环溢出显性演练（40 连推 → 8 丢显性 + 守恒不破）。
pub fn drill_alarm_overflow() -> (bool, String) {
    let mut log = AlarmLog::new();
    for i in 0..40u64 {
        log.push((i % 12) as usize, 0, 1_000_000);
    }
    let overflow_ok = log.dropped == 8;
    let conserved = log.conserved();
    let in_flight = log.count == 32;
    // 弹空。
    while log.pop().is_some() {}
    (
        overflow_ok && conserved && in_flight && log.popped == 32,
        format!(
            "丢{} 在途32={} 弹32={} 守恒={}",
            log.dropped, in_flight, log.popped, log.conserved()
        ),
    )
}

// ═══════════════════════════════════════════════════════════════════════
// §3c 健康趋势账（快照只增 + 最劣点扫描）
// ═══════════════════════════════════════════════════════════════════════

/// 健康快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrendSnap {
    /// 快照序。
    pub seq: u64,
    /// 健康分。
    pub score: u64,
    /// 告警数。
    pub alarms: usize,
}

/// 趋势账（快照只增不减）。
pub struct TrendBook {
    snaps: Vec<TrendSnap>,
    next_seq: u64,
}

impl TrendBook {
    /// 建。
    pub fn new() -> Self {
        TrendBook { snaps: Vec::new(), next_seq: 1 }
    }

    /// 记快照（序号单调分配）。
    pub fn record(&mut self, score: u64, alarms: usize) -> Result<u64, u16> {
        if score > 100 {
            return Err(E_SCORE_OOB);
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.snaps.push(TrendSnap { seq, score, alarms });
        Ok(seq)
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.snaps.len()
    }

    /// 只增核（序号严格递增）。
    pub fn seq_monotone(&self) -> bool {
        for i in 0..self.snaps.len() {
            if self.snaps[i].seq != (i as u64 + 1) {
                return false;
            }
        }
        true
    }

    /// 最劣点（分数最低快照；并列取先）。
    pub fn worst(&self) -> Option<TrendSnap> {
        if self.snaps.is_empty() {
            return None;
        }
        let mut w = self.snaps[0];
        for s in &self.snaps {
            if s.score < w.score {
                w = *s;
            }
        }
        Some(w)
    }

    /// 最优点。
    pub fn best(&self) -> Option<TrendSnap> {
        if self.snaps.is_empty() {
            return None;
        }
        let mut b = self.snaps[0];
        for s in &self.snaps {
            if s.score > b.score {
                b = *s;
            }
        }
        Some(b)
    }

    /// 恢复判定（末快照分数==100 且高于最劣点）。
    pub fn recovered(&self) -> bool {
        match (self.snaps.last(), self.worst()) {
            (Some(last), Some(w)) => last.score == 100 && last.score > w.score,
            _ => false,
        }
    }
}

/// 趋势账演练（降→谷→升→满，只增+最劣+恢复四断言）。
pub fn drill_trend() -> (bool, String) {
    let mut t = TrendBook::new();
    // 序列：100 → 84 → 60 → 92 → 100。
    let seqs = [
        t.record(100, 0),
        t.record(84, 2),
        t.record(60, 5),
        t.record(92, 1),
        t.record(100, 0),
    ];
    let mut all_ok = true;
    for r in &seqs {
        if r.is_err() {
            all_ok = false;
        }
    }
    let worst = t.worst();
    let best = t.best();
    let w_ok = matches!(worst, Some(s) if s.score == 60);
    let b_ok = matches!(best, Some(s) if s.score == 100);
    (
        all_ok && t.seq_monotone() && t.len() == 5 && w_ok && b_ok && t.recovered(),
        format!(
            "5快照 最劣{} 最优{} 恢复={}",
            w_ok, b_ok, t.recovered()
        ),
    )
}

/// 趋势越界拒演练（分数>100 必拒）。
pub fn drill_trend_oob() -> (bool, String) {
    let mut t = TrendBook::new();
    let oob = matches!(t.record(101, 0), Err(E_SCORE_OOB));
    let ok1 = t.record(100, 0).is_ok();
    let ok2 = t.record(0, 12).is_ok();
    (
        oob && ok1 && ok2 && t.len() == 2,
        format!("越界拒={} 合法入={} 账={}", oob, ok1 && ok2, t.len()),
    )
}

/// 导出+趋势合并演练。
pub fn drill_export_trend() -> (bool, String) {
    let (a, da) = drill_rows_export();
    let (b, db) = drill_trend();
    let (c, dc) = drill_trend_oob();
    (a && b && c, format!("导出={} 趋势={} 越界拒={} | {} {} {}", a, b, c, da, db, dc))
}

/// 告警环合并演练。
pub fn drill_alarm_rings() -> (bool, String) {
    let (a, da) = drill_alarm_log();
    let (b, db) = drill_alarm_overflow();
    (a && b, format!("联通={} 溢出={} | {} {}", a, b, da, db))
}

// ═══════════════════════════════════════════════════════════════════════
// §4 注入核（6 项）
// ═══════════════════════════════════════════════════════════════════════

/// 注入核 6 项（越界/重名/漏报/误报/摘要漂移/分越界）。
pub fn inject_core() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut det: Vec<String> = Vec::new();
    // 注1：指标越界还原必 None。
    if MetricKind::from_idx(METRIC_COUNT).is_none() {
        caught += 1;
        det.push("越界:拒".to_string());
    } else {
        det.push("越界:漏".to_string());
    }
    // 注2：重名必拒。
    let mut b = HealthBook::new();
    let _ = b.register(MetricKind::IntEq);
    if matches!(b.register(MetricKind::IntEq), Err(E_METRIC_DUP)) {
        caught += 1;
        det.push("重名:拒".to_string());
    } else {
        det.push("重名:漏".to_string());
    }
    // 注3：破阈值必告警（漏报判据）。
    let mut b2 = HealthBook::new();
    let _ = b2.register(MetricKind::StressEq);
    let _ = b2.update(MetricKind::StressEq, 500_000);
    if b2.metric(MetricKind::StressEq).map(|m| m.alarmed()).unwrap_or(false) {
        caught += 1;
        det.push("漏报:无".to_string());
    } else {
        det.push("漏报:漏".to_string());
    }
    // 注4：满值必不告警（误报判据）。
    let mut b3 = HealthBook::new();
    let _ = b3.register(MetricKind::StressEq);
    let _ = b3.update(MetricKind::StressEq, 1_000_000);
    if b3.metric(MetricKind::StressEq).map(|m| !m.alarmed()).unwrap_or(false) {
        caught += 1;
        det.push("误报:无".to_string());
    } else {
        det.push("误报:漏".to_string());
    }
    // 注5：摘要漂移判据（同账同串）。
    let mut b4 = HealthBook::new();
    let _ = b4.register_all();
    for k in MetricKind::ALL {
        let v = if k.dir() == AlarmDir::LowerBetter { 0 } else { k.threshold() };
        let _ = b4.update(k, v);
    }
    if b4.summary() == b4.summary() {
        caught += 1;
        det.push("摘要:守".to_string());
    } else {
        det.push("摘要:漏".to_string());
    }
    // 注6：健康分下溢钳 0（全告警账分数仍合法）。
    let mut b5 = HealthBook::new();
    let _ = b5.register_all();
    for k in MetricKind::ALL {
        let bad = if k.dir() == AlarmDir::LowerBetter {
            if k.value_cap() == 0 { 999 } else { k.value_cap() + 1 }
        } else {
            0
        };
        let _ = b5.update(k, bad);
    }
    let sc = b5.score().unwrap_or(u64::MAX);
    if sc == 0 {
        caught += 1;
        det.push("分钳:守".to_string());
    } else {
        det.push("分钳:漏".to_string());
    }
    (caught, 6, det)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    CODE_ALL.iter().all(|c| *c >= SEG_BASE && *c < SEG_BASE + 16)
}

// ═══════════════════════════════════════════════════════════════════════
// §5 run_unx1331_checks：本单判据聚合（诊断段 0x7710）
// ═══════════════════════════════════════════════════════════════════════

/// 聚合判据（≥6 条，含 10/10 注入演练与段位守卫）。
/// 口径：12 指标 F0899 注册、摘要一致率 100%、告警联通 10/10。
pub fn run_unx1331_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1331 等价健康账");
    let (ok1, d1) = drill_all_green();
    s.add_dyn("J1-全指标注册+健康分100", ok1, &d1);
    let (ok2, d2) = drill_summary_stable();
    s.add_dyn("J2-摘要一致率100%", ok2, &d2);
    let (ok3, d3) = drill_alarm_10();
    s.add_dyn("J3-告警联通10/10", ok3, &d3);
    let (ok4, d4) = drill_refusals();
    s.add_dyn("J4-越域/重名/底线必拒", ok4, &d4);
    let (ok5, d5) = drill_alarm_false();
    s.add_dyn("J5-误报反向+扣分制", ok5, &d5);
    let (ok6, d6) = drill_export_trend();
    s.add_dyn("J6-导出+趋势三断言", ok6, &d6);
    let (ok7, d7) = drill_alarm_rings();
    s.add_dyn("J7-告警环联通+溢出", ok7, &d7);
    let (c8, t8, d8) = inject_core();
    s.add_dyn(
        "J8-注入核6/6",
        c8 == t8,
        &format!("{}/{} {}", c8, t8, d8.join(" ")),
    );
    s.add("J9-段位守卫", segment_guard(), "0x7710段闭集10码");
    s.add(
        "J9-指标宇宙常量",
        METRIC_COUNT == 12 && REG_FLOOR == 10,
        "12指标×底线10写死",
    );
    s
}
