//! UNX-F49069 延迟告警面（阈值越限事件显性化——三要素呈现）· 0x73380
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定）：
//!   UNX-F49069 延迟告警面（阈值越限事件显性化——三要素呈现）:
//!          ① 纯功能代码 ≥300 行；
//!          ② 判据原文：UNX-F49070-J1 合流 ×10 事件可查；
//!             判据锚定 体验十三·总日志中心。
//!   UNX-F49070 账本与体验日志合流（告警事件进总日志中心可查）:
//!          ① 纯功能代码 ≥320 行；
//!          ② 判据原文：UNX-F49071-J1 记录 ×10 轮在册；
//!             判据锚定 §AI-62 §6 攻坚③。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（告警面）：越限 → 三要素 → 去重 → 入板。
//! 越限先于三要素：没有「观测 > 阈值」这一事实，三要素只是三个彼此无关的数字；
//! 三要素先于去重：先确认一条告警自身成立（段/阈值/观测齐全），
//!   再去谈它与历史是否重复——否则会把「造错的告警」当成「重复的告警」丢掉；
//! 去重先于入板：板序是运营的阅读面，重复条目一旦进板就会污染事件计数；
//! 入板先于合流：只有板内次序已定，合流后的总日志才有一致的 (日, 序) 前缀。
//! 闸门次序（合流）：合流 → 次序 → 联查 → 复跑。
//! 合流先于次序：先把十轮的告警逐轮铺进总日志，才谈得上「轮内有序、轮间递进」；
//! 次序先于联查：事件号可查的前提是记录不重不丢，次序正是这两件事的守卫；
//! 联查先于复跑：先证明每个事件在十轮里都能被查到，复跑才有对照基准；
//! 复跑先于成文：十次重建摘要逐字一致，才允许把合流结果写进运营记录。
//! 关键差异：告警面只回答「哪一段越限、越了多少、该不该叫人」；
//! 合流面只回答「这条告警进了总日志没有、十轮之后还查不查得到」。
//! 两本账摘要域独立：告警摘要折 (id,seg,metric,thr,obs)，日志摘要折 (round,day,seq,kind,evt)；
//! 二者都不含摘要自身，否则一位数字的篡改就能把摘要一并带走。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x73380，B14 族第五槽）。
pub const SEG_BASE: u32 = 0x73380;

/// 延迟链四段。
pub const SEGMENTS: usize = 4;
/// 三档分位口径。
pub const METRICS: usize = 3;
/// 合流轮数（判据口径 ×10）。
pub const ROUNDS: u32 = 10;
/// 合流轮数上限（防失控复跑）。
pub const ROUND_LIMIT: u32 = 100;
/// 记账日数。
pub const DAYS: u32 = 10;
/// 单日序位上限。
pub const SEQS_PER_DAY: u32 = 64;
/// 去重窗口（最近 id 数）。
pub const DEDUP_WINDOW: usize = 8;
/// 告警板容量。
pub const BOARD_CAP: usize = 16;
/// 总日志容量（十轮 × 板满仍有富余）。
pub const LOG_CAP: usize = 128;
/// 计时值上限（1 秒，纳秒）。
pub const NS_LIMIT: u32 = 1_000_000_000;
/// 千分比刻度。
pub const PERMILLE: u32 = 1_000;
/// 严重度升级比（观测 ≥ 阈值 ×1250‰ 判重）。
pub const CRITICAL_RATIO_BP: u32 = 1_250;
/// 告警账行标签。
pub const ALARM_TAG: &str = "ALRM";
/// 合流账行标签。
pub const LOG_TAG: &str = "XLOG";
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 B14 告警/合流诊断码";
/// 段名（下标即段号）。
pub const SEG_NAMES: [&str; SEGMENTS] = ["interrupt", "dispatch", "compose", "present"];
/// 四段告警阈值（纳秒）。
pub const THRESHOLD_NS: [u32; SEGMENTS] = [1_000, 1_600, 3_600, 6_000];
/// 分位名（下标即口径序号）。
pub const METRIC_NAMES: [&str; METRICS] = ["p50", "p95", "p99"];
/// 告警行数字键（固定列序）。
pub const ALARM_KEYS: [&str; 6] = ["id", "seg", "metric", "thr", "obs", "sev"];
/// 日志行数字键（固定列序）。
pub const LOG_KEYS: [&str; 5] = ["round", "day", "seq", "kind", "evt"];
/// 告警行字段数（tag + 6 键 + digest）。
pub const ALARM_FIELDS: usize = 8;
/// 日志行字段数（tag + 5 键 + digest）。
pub const LOG_FIELDS: usize = 7;

// ── 诊断码 ───────────────────────────────────────────────────────
/// 告警/合流诊断码（14 条，互不共用说明）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlarmCode {
    /// 段号越出 0..3
    SegBad,
    /// 分位口径越出 0..2
    MetricBad,
    /// 阈值不在 (0, 1s]
    ThresholdBad,
    /// 观测不越限或超出 1s
    ObservedBad,
    /// 事件号为零
    IdBad,
    /// 日/序越界
    SeqBad,
    /// 去重窗口内的重复事件
    DedupBad,
    /// 板内或日志内次序倒退
    OrderBad,
    /// 摘要与字段不符
    DigestBad,
    /// 账行键序、数字或分隔符非法
    RenderBad,
    /// 告警板或总日志容量已满
    CapacityBad,
    /// 合流轮次越界
    RoundBad,
    /// 复跑轮数为零
    ReplayBad,
    /// 复跑轮数超出上限或计数溢出
    Overflow,
}

impl AlarmCode {
    /// 诊断码数值。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 说明。
    pub fn message(self) -> &'static str {
        detail(self.val())
    }

    /// 全码表。
    pub const ALL: [AlarmCode; 14] = [
        AlarmCode::SegBad,
        AlarmCode::MetricBad,
        AlarmCode::ThresholdBad,
        AlarmCode::ObservedBad,
        AlarmCode::IdBad,
        AlarmCode::SeqBad,
        AlarmCode::DedupBad,
        AlarmCode::OrderBad,
        AlarmCode::DigestBad,
        AlarmCode::RenderBad,
        AlarmCode::CapacityBad,
        AlarmCode::RoundBad,
        AlarmCode::ReplayBad,
        AlarmCode::Overflow,
    ];
}

/// 码值到说明；段外一律未知，不泄漏邻段文案。
pub fn detail(code: u32) -> &'static str {
    match code.wrapping_sub(SEG_BASE) {
        1 => "告警段号越出 0..3",
        2 => "分位口径越出 0..2",
        3 => "告警阈值不在 (0, 1s]",
        4 => "观测值未越限或超出 1s",
        5 => "事件号为零",
        6 => "记账日或日序越界",
        7 => "去重窗口内重复事件",
        8 => "告警板或总日志次序倒退",
        9 => "摘要与字段不符",
        10 => "账行键序/数字/分隔符非法",
        11 => "告警板或总日志容量已满",
        12 => "合流轮次越出 0..10",
        13 => "复跑轮数为零",
        14 => "复跑轮数超限或计数溢出",
        _ => UNKNOWN,
    }
}

/// 段位守卫：14 码恰好占满 [SEG_BASE+1, SEG_BASE+14]，且互不重复。
pub fn segment_guard() -> bool {
    let lo = SEG_BASE + 0x01;
    let hi = SEG_BASE + 0x0E;
    let mut i = 0usize;
    while i < AlarmCode::ALL.len() {
        let v = match AlarmCode::ALL.get(i) {
            Some(c) => c.val(),
            None => return false,
        };
        if v < lo || v > hi {
            return false;
        }
        let mut j = i + 1;
        while j < AlarmCode::ALL.len() {
            if let Some(other) = AlarmCode::ALL.get(j) {
                if other.val() == v {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 把 64 位摘要折入 64 位累加器（高低两半各折一次）。
fn fold_digest(h: u64, v: u64) -> u64 {
    p_core::fold_u32(p_core::fold_u32(h, (v >> 32) as u32), v as u32)
}

// ── 分位与严重度 ─────────────────────────────────────────────────
/// 三档分位口径。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Metric {
    /// 中位
    P50,
    /// 尾部 95%
    P95,
    /// 尾部 99%
    P99,
}

impl Metric {
    /// 口径序号。
    pub fn index(self) -> u8 {
        match self {
            Metric::P50 => 0,
            Metric::P95 => 1,
            Metric::P99 => 2,
        }
    }

    /// 口径名。
    pub fn name(self) -> &'static str {
        match METRIC_NAMES.get(self.index() as usize) {
            Some(v) => v,
            None => "?",
        }
    }

    /// 由序号反查；越界为 MetricBad。
    pub fn from_index(i: u8) -> Result<Metric, u32> {
        match i {
            0 => Ok(Metric::P50),
            1 => Ok(Metric::P95),
            2 => Ok(Metric::P99),
            _ => Err(AlarmCode::MetricBad.val()),
        }
    }

    /// 由名反查。
    pub fn from_name(name: &str) -> Option<Metric> {
        let mut i = 0usize;
        while i < METRIC_NAMES.len() {
            if let Some(v) = METRIC_NAMES.get(i) {
                if *v == name {
                    return Metric::from_index(i as u8).ok();
                }
            }
            i += 1;
        }
        None
    }
}

/// 告警严重度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    /// 越限但未到 1.25 倍
    Warn,
    /// 越限达到 1.25 倍
    Critical,
}

impl Severity {
    /// 序号。
    pub fn index(self) -> u8 {
        match self {
            Severity::Warn => 0,
            Severity::Critical => 1,
        }
    }

    /// 名。
    pub fn name(self) -> &'static str {
        match self {
            Severity::Warn => "warn",
            Severity::Critical => "critical",
        }
    }

    /// 由名反查。
    pub fn from_name(name: &str) -> Option<Severity> {
        if name == "warn" {
            Some(Severity::Warn)
        } else if name == "critical" {
            Some(Severity::Critical)
        } else {
            None
        }
    }

    /// 由阈值/观测判定；观测等于阈值时按未越限的保守档落在 Warn。
    pub fn for_levels(threshold_ns: u32, observed_ns: u32) -> Severity {
        let lhs = (observed_ns as u64).saturating_mul(PERMILLE as u64);
        let rhs = (threshold_ns as u64).saturating_mul(CRITICAL_RATIO_BP as u64);
        if lhs >= rhs {
            Severity::Critical
        } else {
            Severity::Warn
        }
    }
}

// ── 告警事件 ─────────────────────────────────────────────────────
/// 单条告警：编号 + 段 + 口径 + 阈值 + 观测 + 记账位置 + 摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AlarmEvent {
    /// 事件号（非零）。
    pub id: u32,
    /// 段号（0..3）。
    pub seg: u8,
    /// 分位口径。
    pub metric: Metric,
    /// 阈值（纳秒）。
    pub threshold_ns: u32,
    /// 观测（纳秒，必须越限）。
    pub observed_ns: u32,
    /// 记账日。
    pub day: u32,
    /// 日序。
    pub seq: u32,
    /// 摘要（折 id/seg/metric/thr/obs）。
    pub digest: u64,
}

/// 告警摘要：只折行内可见字段，日/序由板内次序另行守恒。
pub fn alarm_digest(id: u32, seg: u8, metric: Metric, threshold_ns: u32, observed_ns: u32) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, id);
    h = p_core::fold_byte(h, seg);
    h = p_core::fold_byte(h, metric.index());
    h = p_core::fold_u32(h, threshold_ns);
    h = p_core::fold_u32(h, observed_ns);
    h
}

impl AlarmEvent {
    /// 校验并构造。
    pub fn new(
        id: u32,
        seg: u8,
        metric: Metric,
        threshold_ns: u32,
        observed_ns: u32,
        day: u32,
        seq: u32,
    ) -> Result<AlarmEvent, u32> {
        if id == 0 {
            return Err(AlarmCode::IdBad.val());
        }
        if (seg as usize) >= SEGMENTS {
            return Err(AlarmCode::SegBad.val());
        }
        if threshold_ns == 0 || threshold_ns > NS_LIMIT {
            return Err(AlarmCode::ThresholdBad.val());
        }
        if observed_ns <= threshold_ns || observed_ns > NS_LIMIT {
            return Err(AlarmCode::ObservedBad.val());
        }
        if day >= DAYS || seq >= SEQS_PER_DAY {
            return Err(AlarmCode::SeqBad.val());
        }
        let digest = alarm_digest(id, seg, metric, threshold_ns, observed_ns);
        Ok(AlarmEvent {
            id,
            seg,
            metric,
            threshold_ns,
            observed_ns,
            day,
            seq,
            digest,
        })
    }

    /// 严重度。
    pub fn severity(&self) -> Severity {
        Severity::for_levels(self.threshold_ns, self.observed_ns)
    }

    /// 是否越限。
    pub fn exceeded(&self) -> bool {
        self.observed_ns > self.threshold_ns
    }

    /// 越限量（纳秒）。
    pub fn excess_ns(&self) -> u32 {
        self.observed_ns.saturating_sub(self.threshold_ns)
    }

    /// 观测/阈值（千分比）。
    pub fn ratio_bp(&self) -> u32 {
        if self.threshold_ns == 0 {
            return 0;
        }
        let bp = (self.observed_ns as u64)
            .saturating_mul(PERMILLE as u64)
            .checked_div(self.threshold_ns as u64)
            .unwrap_or(0);
        if bp > u32::MAX as u64 {
            u32::MAX
        } else {
            bp as u32
        }
    }

    /// 三要素齐全：段名可读、阈值非零、观测确实越限。
    pub fn three_elements(&self) -> bool {
        (self.seg as usize) < SEGMENTS
            && self.threshold_ns > 0
            && self.observed_ns > self.threshold_ns
            && !self.metric.name().is_empty()
    }

    /// 记账位置键（日×64 + 序）。
    pub fn order_key(&self) -> u32 {
        self.day
            .saturating_mul(SEQS_PER_DAY)
            .saturating_add(self.seq)
    }

    /// 摘要自洽。
    pub fn verify(&self) -> bool {
        self.digest
            == alarm_digest(
                self.id,
                self.seg,
                self.metric,
                self.threshold_ns,
                self.observed_ns,
            )
    }
}

// ── 去重窗口 ─────────────────────────────────────────────────────
/// 最近事件号的滑动窗口（满窗逐出最旧一条）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DedupWindow {
    ids: Vec<u32>,
}

impl DedupWindow {
    /// 空窗。
    pub fn new() -> DedupWindow {
        DedupWindow { ids: Vec::new() }
    }

    /// 是否命中。
    pub fn contains(&self, id: u32) -> bool {
        let mut i = 0usize;
        while i < self.ids.len() {
            if let Some(v) = self.ids.get(i) {
                if *v == id {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 记入；满窗先逐出最旧。
    pub fn remember(&mut self, id: u32) {
        while self.ids.len() >= DEDUP_WINDOW {
            if self.ids.is_empty() {
                break;
            }
            self.ids.remove(0);
        }
        self.ids.push(id);
    }

    /// 窗内条数。
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// 空窗判定。
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// 窗容。
    pub fn capacity(&self) -> usize {
        DEDUP_WINDOW
    }
}

// ── 告警板 ───────────────────────────────────────────────────────
/// 告警板：有序、去重、带链式摘要。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AlarmBoard {
    events: Vec<AlarmEvent>,
    dedup: DedupWindow,
    last_key: u32,
    has_last: bool,
    rejected: u32,
    digest: u64,
}

impl AlarmBoard {
    /// 空板。
    pub fn new() -> AlarmBoard {
        AlarmBoard {
            events: Vec::new(),
            dedup: DedupWindow::new(),
            last_key: 0,
            has_last: false,
            rejected: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 入板：自身成立 → 不重复 → 次序递进 → 有余量。
    pub fn push(&mut self, e: AlarmEvent) -> Result<(), u32> {
        if !e.verify() {
            self.bump_rejected();
            return Err(AlarmCode::DigestBad.val());
        }
        if self.dedup.contains(e.id) {
            self.bump_rejected();
            return Err(AlarmCode::DedupBad.val());
        }
        let key = e.order_key();
        if self.has_last && key <= self.last_key {
            self.bump_rejected();
            return Err(AlarmCode::OrderBad.val());
        }
        if self.events.len() >= BOARD_CAP {
            self.bump_rejected();
            return Err(AlarmCode::CapacityBad.val());
        }
        self.events.push(e);
        self.dedup.remember(e.id);
        self.last_key = key;
        self.has_last = true;
        self.digest = fold_digest(self.digest, e.digest);
        Ok(())
    }

    fn bump_rejected(&mut self) {
        self.rejected = self.rejected.saturating_add(1);
    }

    /// 板内条数。
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 空板判定。
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 按下标取事件。
    pub fn get(&self, i: usize) -> Option<&AlarmEvent> {
        self.events.get(i)
    }

    /// 按事件号查。
    pub fn find(&self, id: u32) -> Option<&AlarmEvent> {
        let mut i = 0usize;
        while i < self.events.len() {
            if let Some(e) = self.events.get(i) {
                if e.id == id {
                    return Some(e);
                }
            }
            i += 1;
        }
        None
    }

    /// 拒收计数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 去重窗条数。
    pub fn dedup_len(&self) -> usize {
        self.dedup.len()
    }

    /// 链式摘要。
    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 板摘要重算。
    pub fn recompute(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        let mut i = 0usize;
        while i < self.events.len() {
            if let Some(e) = self.events.get(i) {
                h = fold_digest(h, e.digest);
            }
            i += 1;
        }
        h
    }

    /// 板自洽：逐条成立、键严格递增、摘要可重算。
    pub fn verify(&self) -> bool {
        let mut last: Option<u32> = None;
        let mut i = 0usize;
        while i < self.events.len() {
            match self.events.get(i) {
                Some(e) => {
                    if !e.verify() {
                        return false;
                    }
                    let key = e.order_key();
                    if let Some(prev) = last {
                        if key <= prev {
                            return false;
                        }
                    }
                    last = Some(key);
                }
                None => return false,
            }
            i += 1;
        }
        self.digest == self.recompute()
    }

    /// 按严重度计数。
    pub fn count_severity(&self, s: Severity) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.events.len() {
            if let Some(e) = self.events.get(i) {
                if e.severity() == s {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }
}

/// 告警板统计。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AlarmStats {
    /// 总条数。
    pub total: u32,
    /// 预警条数。
    pub warn: u32,
    /// 重警条数。
    pub critical: u32,
    /// 最大观测/阈值比（千分比）。
    pub max_ratio_bp: u32,
}

/// 统计一块板。
pub fn board_stats(board: &AlarmBoard) -> AlarmStats {
    let mut total = 0u32;
    let mut warn = 0u32;
    let mut critical = 0u32;
    let mut max_ratio_bp = 0u32;
    let mut i = 0usize;
    while i < board.len() {
        if let Some(e) = board.get(i) {
            total = total.saturating_add(1);
            match e.severity() {
                Severity::Warn => warn = warn.saturating_add(1),
                Severity::Critical => critical = critical.saturating_add(1),
            }
            let bp = e.ratio_bp();
            if bp > max_ratio_bp {
                max_ratio_bp = bp;
            }
        }
        i += 1;
    }
    AlarmStats {
        total,
        warn,
        critical,
        max_ratio_bp,
    }
}

// ── 合流日志 ─────────────────────────────────────────────────────
/// 日志条目类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogKind {
    /// 告警事件
    Alarm,
    /// 体验日志合流标记
    Merge,
}

impl LogKind {
    /// 序号。
    pub fn index(self) -> u8 {
        match self {
            LogKind::Alarm => 0,
            LogKind::Merge => 1,
        }
    }

    /// 名。
    pub fn name(self) -> &'static str {
        match self {
            LogKind::Alarm => "alarm",
            LogKind::Merge => "merge",
        }
    }

    /// 由名反查。
    pub fn from_name(name: &str) -> Option<LogKind> {
        if name == "alarm" {
            Some(LogKind::Alarm)
        } else if name == "merge" {
            Some(LogKind::Merge)
        } else {
            None
        }
    }
}

/// 单条日志记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LogRecord {
    /// 轮次。
    pub round: u32,
    /// 记账日。
    pub day: u32,
    /// 日序。
    pub seq: u32,
    /// 条目类型。
    pub kind: LogKind,
    /// 关联事件号。
    pub event_id: u32,
    /// 记录摘要。
    pub digest: u64,
}

/// 日志记录摘要：只折行内可见字段。
pub fn record_digest(round: u32, day: u32, seq: u32, kind: LogKind, event_id: u32) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, round);
    h = p_core::fold_u32(h, day);
    h = p_core::fold_u32(h, seq);
    h = p_core::fold_byte(h, kind.index());
    h = p_core::fold_u32(h, event_id);
    h
}

/// 总日志：轮内按 (日,序)、轮间按轮次严格递增。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MergedLog {
    records: Vec<LogRecord>,
    last_key: u64,
    has_last: bool,
    digest: u64,
}

impl MergedLog {
    /// 空日志。
    pub fn new() -> MergedLog {
        MergedLog {
            records: Vec::new(),
            last_key: 0,
            has_last: false,
            digest: p_core::FNV_OFF,
        }
    }

    /// 追加一条：轮内 (日,序) 与轮间轮次共同构成严格递增键。
    pub fn push(
        &mut self,
        round: u32,
        day: u32,
        seq: u32,
        kind: LogKind,
        event_id: u32,
    ) -> Result<(), u32> {
        if round >= ROUNDS {
            return Err(AlarmCode::RoundBad.val());
        }
        if day >= DAYS || seq >= SEQS_PER_DAY {
            return Err(AlarmCode::SeqBad.val());
        }
        if event_id == 0 {
            return Err(AlarmCode::IdBad.val());
        }
        let key = ((round as u64) << 32) | (day.saturating_mul(SEQS_PER_DAY).saturating_add(seq) as u64);
        if self.has_last && key <= self.last_key {
            return Err(AlarmCode::OrderBad.val());
        }
        if self.records.len() >= LOG_CAP {
            return Err(AlarmCode::CapacityBad.val());
        }
        let digest = record_digest(round, day, seq, kind, event_id);
        self.records.push(LogRecord {
            round,
            day,
            seq,
            kind,
            event_id,
            digest,
        });
        self.digest = fold_digest(self.digest, digest);
        self.last_key = key;
        self.has_last = true;
        Ok(())
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 空日志判定。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 按下标取。
    pub fn get(&self, i: usize) -> Option<&LogRecord> {
        self.records.get(i)
    }

    /// 链式摘要。
    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 摘要重算。
    pub fn recompute(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                h = fold_digest(h, r.digest);
            }
            i += 1;
        }
        h
    }

    /// 某事件出现次数。
    pub fn occurrences(&self, event_id: u32) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.event_id == event_id {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 事件是否可查。
    pub fn contains(&self, event_id: u32) -> bool {
        self.occurrences(event_id) > 0
    }

    /// 日志自洽：逐条摘要成立、键严格递增、链摘要可重算。
    pub fn verify(&self) -> bool {
        let mut last: Option<u64> = None;
        let mut i = 0usize;
        while i < self.records.len() {
            match self.records.get(i) {
                Some(r) => {
                    if r.digest
                        != record_digest(r.round, r.day, r.seq, r.kind, r.event_id)
                    {
                        return false;
                    }
                    let key = ((r.round as u64) << 32)
                        | (r.day.saturating_mul(SEQS_PER_DAY).saturating_add(r.seq) as u64);
                    if let Some(prev) = last {
                        if key <= prev {
                            return false;
                        }
                    }
                    last = Some(key);
                }
                None => return false,
            }
            i += 1;
        }
        self.digest == self.recompute()
    }
}

/// 合流：把板上告警逐轮铺进总日志。
pub fn merge_rounds(board: &AlarmBoard, rounds: u32) -> Result<MergedLog, u32> {
    if rounds == 0 {
        return Err(AlarmCode::ReplayBad.val());
    }
    if rounds > ROUND_LIMIT {
        return Err(AlarmCode::Overflow.val());
    }
    let mut log = MergedLog::new();
    let mut r = 0u32;
    while r < rounds {
        let mut i = 0usize;
        while i < board.len() {
            if let Some(e) = board.get(i) {
                log.push(r, e.day, e.seq, LogKind::Alarm, e.id)?;
            }
            i += 1;
        }
        r = r.saturating_add(1);
    }
    Ok(log)
}

/// 十轮合流（判据口径）。
pub fn merge_ten_rounds(board: &AlarmBoard) -> Result<MergedLog, u32> {
    merge_rounds(board, ROUNDS)
}

/// 复跑：连续重建 `times` 次，摘要逐次一致且每次自洽。
pub fn merge_replay(board: &AlarmBoard, times: u32) -> bool {
    if times == 0 {
        return false;
    }
    let mut want: Option<u64> = None;
    let mut t = 0u32;
    while t < times {
        match merge_rounds(board, ROUNDS) {
            Ok(log) => {
                if !log.verify() {
                    return false;
                }
                if log.len() != board.len().saturating_mul(ROUNDS as usize) {
                    return false;
                }
                match want {
                    None => want = Some(log.digest()),
                    Some(d) => {
                        if d != log.digest() {
                            return false;
                        }
                    }
                }
            }
            Err(_) => return false,
        }
        t = t.saturating_add(1);
    }
    want.is_some()
}

// ── 渲染与回读 ───────────────────────────────────────────────────
/// 16 位小写十六进制。
fn hex16(v: u64) -> String {
    format!("{:016x}", v)
}

/// 十进制无符号解析（拒绝空串、非数字、溢出）。
fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 10 {
        return None;
    }
    let mut v = 0u64;
    for b in s.bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        v = v.saturating_mul(10).saturating_add((b - b'0') as u64);
        if v > u32::MAX as u64 {
            return None;
        }
    }
    Some(v as u32)
}

/// 16 位十六进制解析。
fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut v = 0u64;
    for b in s.bytes() {
        let d = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return None,
        };
        v = (v << 4) | d as u64;
    }
    Some(v)
}

/// 按固定标签与键序切行，返回各键值 + 摘要串。
fn row_fields<'a>(row: &'a str, tag: &str, keys: &[&str]) -> Option<Vec<&'a str>> {
    let mut it = row.split('|');
    if it.next() != Some(tag) {
        return None;
    }
    let mut vals: Vec<&'a str> = Vec::with_capacity(keys.len() + 1);
    let mut i = 0usize;
    while i < keys.len() {
        let part = it.next()?;
        let (k, v) = part.split_once('=')?;
        if k != *keys.get(i)? {
            return None;
        }
        vals.push(v);
        i += 1;
    }
    let tail = it.next()?;
    let (dk, dv) = tail.split_once('=')?;
    if dk != "digest" || it.next().is_some() {
        return None;
    }
    vals.push(dv);
    Some(vals)
}

/// 渲染一条告警账行。
pub fn render_alarm(e: &AlarmEvent) -> String {
    format!(
        "{}|id={}|seg={}|metric={}|thr={}|obs={}|sev={}|digest={}",
        ALARM_TAG,
        e.id,
        e.seg,
        e.metric.name(),
        e.threshold_ns,
        e.observed_ns,
        e.severity().name(),
        hex16(e.digest)
    )
}

/// 回读一条告警账行：键序、范围、严重度一致性、摘要重算、往返恒等。
pub fn audit_alarm(row: &str) -> bool {
    match row_fields(row, ALARM_TAG, &ALARM_KEYS) {
        Some(v) => {
            let id = match v.first().and_then(|s| parse_u32(s)) {
                Some(x) => x,
                None => return false,
            };
            let seg = match v.get(1).and_then(|s| parse_u32(s)) {
                Some(x) => x,
                None => return false,
            };
            let metric = match v.get(2).and_then(|s| Metric::from_name(s)) {
                Some(m) => m,
                None => return false,
            };
            let thr = match v.get(3).and_then(|s| parse_u32(s)) {
                Some(x) => x,
                None => return false,
            };
            let obs = match v.get(4).and_then(|s| parse_u32(s)) {
                Some(x) => x,
                None => return false,
            };
            let sev = match v.get(5).and_then(|s| Severity::from_name(s)) {
                Some(x) => x,
                None => return false,
            };
            let digest = match v.get(6).and_then(|s| parse_hex16(s)) {
                Some(x) => x,
                None => return false,
            };
            if id == 0 || (seg as usize) >= SEGMENTS || thr == 0 || thr > NS_LIMIT {
                return false;
            }
            if obs <= thr || obs > NS_LIMIT || sev != Severity::for_levels(thr, obs) {
                return false;
            }
            if digest != alarm_digest(id, seg as u8, metric, thr, obs) {
                return false;
            }
            let e = AlarmEvent {
                id,
                seg: seg as u8,
                metric,
                threshold_ns: thr,
                observed_ns: obs,
                day: 0,
                seq: 0,
                digest,
            };
            render_alarm(&e) == row
        }
        None => false,
    }
}

/// 导出整块板的告警账行（板序）。
pub fn alarm_rows(board: &AlarmBoard) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(board.len());
    let mut i = 0usize;
    while i < board.len() {
        if let Some(e) = board.get(i) {
            out.push(render_alarm(e));
        }
        i += 1;
    }
    out
}

/// 导出行集合在 `rounds` 次导出间逐字一致。
pub fn rows_stable(board: &AlarmBoard, rounds: u32) -> bool {
    if rounds == 0 {
        return false;
    }
    let first = alarm_rows(board);
    let mut t = 1u32;
    while t < rounds {
        let again = alarm_rows(board);
        if again.len() != first.len() {
            return false;
        }
        let mut i = 0usize;
        while i < first.len() {
            if first.get(i) != again.get(i) {
                return false;
            }
            i += 1;
        }
        t = t.saturating_add(1);
    }
    true
}

/// 渲染一条日志账行。
pub fn render_log(r: &LogRecord) -> String {
    format!(
        "{}|round={}|day={}|seq={}|kind={}|evt={}|digest={}",
        LOG_TAG,
        r.round,
        r.day,
        r.seq,
        r.kind.name(),
        r.event_id,
        hex16(r.digest)
    )
}

/// 回读一条日志账行。
pub fn audit_log(row: &str) -> bool {
    match row_fields(row, LOG_TAG, &LOG_KEYS) {
        Some(v) => {
            let round = match v.first().and_then(|s| parse_u32(s)) {
                Some(x) => x,
                None => return false,
            };
            let day = match v.get(1).and_then(|s| parse_u32(s)) {
                Some(x) => x,
                None => return false,
            };
            let seq = match v.get(2).and_then(|s| parse_u32(s)) {
                Some(x) => x,
                None => return false,
            };
            let kind = match v.get(3).and_then(|s| LogKind::from_name(s)) {
                Some(x) => x,
                None => return false,
            };
            let evt = match v.get(4).and_then(|s| parse_u32(s)) {
                Some(x) => x,
                None => return false,
            };
            let digest = match v.get(5).and_then(|s| parse_hex16(s)) {
                Some(x) => x,
                None => return false,
            };
            if round >= ROUNDS || day >= DAYS || seq >= SEQS_PER_DAY || evt == 0 {
                return false;
            }
            if digest != record_digest(round, day, seq, kind, evt) {
                return false;
            }
            let r = LogRecord {
                round,
                day,
                seq,
                kind,
                event_id: evt,
                digest,
            };
            render_log(&r) == row
        }
        None => false,
    }
}

/// 导出整本日志的账行（日志序）。
pub fn log_rows(log: &MergedLog) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(log.len());
    let mut i = 0usize;
    while i < log.len() {
        if let Some(r) = log.get(i) {
            out.push(render_log(r));
        }
        i += 1;
    }
    out
}

// ── 合成 ─────────────────────────────────────────────────────────
/// 合成一块六条告警的板：位置递增、编号唯一、全部越限。
pub fn synth_alarms(seed: u64) -> Result<AlarmBoard, u32> {
    let mut board = AlarmBoard::new();
    let mut rng = p_core::Lcg::new(seed ^ 0x414C_524D_0009);
    let count = 6u32;
    let mut i = 0u32;
    while i < count {
        let seg = rng.below(SEGMENTS as u32) as usize;
        let thr = match THRESHOLD_NS.get(seg) {
            Some(v) => *v,
            None => return Err(AlarmCode::SegBad.val()),
        };
        let metric = Metric::from_index(rng.below(METRICS as u32) as u8)?;
        let over_cap = (thr / 2).max(1);
        let over = 1 + ((rng.next() >> 40) as u32) % over_cap;
        let obs = thr.saturating_add(over).min(NS_LIMIT);
        let day = i / 4;
        let seq = i % 4;
        let id = 0x6900 + i;
        let e = AlarmEvent::new(id, seg as u8, metric, thr, obs, day, seq)?;
        board.push(e)?;
        i = i.saturating_add(1);
    }
    Ok(board)
}

// ── 自检 ─────────────────────────────────────────────────────────
/// 八项检查。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::with_capacity(8);

    // 1) 告警显性化：六条告警全部越限、三要素齐全、严重度分档、编号唯一。
    let ok;
    let mut info = String::from("告警板构造失败");
    match synth_alarms(0x6901) {
        Ok(board) => {
            let stats = board_stats(&board);
            let mut all = !board.is_empty()
                && board.len() <= BOARD_CAP
                && board.verify()
                && stats.total == board.len() as u32
                && stats.warn.saturating_add(stats.critical) == stats.total
                && stats.max_ratio_bp > PERMILLE
                && board.dedup_len() == board.len().min(DEDUP_WINDOW);
            let mut seen: Vec<u32> = Vec::new();
            let mut i = 0usize;
            while i < board.len() {
                match board.get(i) {
                    Some(e) => {
                        if !e.three_elements() || !e.exceeded() || e.ratio_bp() <= PERMILLE {
                            all = false;
                        }
                        if seen.contains(&e.id) {
                            all = false;
                        }
                        seen.push(e.id);
                    }
                    None => all = false,
                }
                i += 1;
            }
            ok = all;
            info = format!(
                "events={} warn={} crit={} max={}‰",
                stats.total, stats.warn, stats.critical, stats.max_ratio_bp
            );
        }
        Err(_) => ok = false,
    }
    out.push(("F49069/告警显性化", ok, info));

    // 2) 告警拒收：构造期七类坏输入各自命中专属码。
    let mut cases = 0u32;
    let mut hits = 0u32;
    let expectations = [
        (
            AlarmEvent::new(0, 0, Metric::P50, 1_000, 1_100, 0, 0).err(),
            AlarmCode::IdBad.val(),
        ),
        (
            AlarmEvent::new(1, 4, Metric::P50, 1_000, 1_100, 0, 0).err(),
            AlarmCode::SegBad.val(),
        ),
        (
            AlarmEvent::new(1, 0, Metric::P50, 0, 1_100, 0, 0).err(),
            AlarmCode::ThresholdBad.val(),
        ),
        (
            AlarmEvent::new(1, 0, Metric::P50, NS_LIMIT + 1, 1_100, 0, 0).err(),
            AlarmCode::ThresholdBad.val(),
        ),
        (
            AlarmEvent::new(1, 0, Metric::P50, 1_000, 1_000, 0, 0).err(),
            AlarmCode::ObservedBad.val(),
        ),
        (
            AlarmEvent::new(1, 0, Metric::P50, 1_000, NS_LIMIT + 1, 0, 0).err(),
            AlarmCode::ObservedBad.val(),
        ),
        (
            AlarmEvent::new(1, 0, Metric::P50, 1_000, 1_100, DAYS, 0).err(),
            AlarmCode::SeqBad.val(),
        ),
        (
            AlarmEvent::new(1, 0, Metric::P50, 1_000, 1_100, 0, SEQS_PER_DAY).err(),
            AlarmCode::SeqBad.val(),
        ),
        (Metric::from_index(3).err(), AlarmCode::MetricBad.val()),
        (Metric::from_index(9).err(), AlarmCode::MetricBad.val()),
    ];
    let mut k = 0usize;
    while k < expectations.len() {
        cases = cases.saturating_add(1);
        if let Some((got, want)) = expectations.get(k) {
            if *got == Some(*want) {
                hits = hits.saturating_add(1);
            }
        }
        k += 1;
    }
    out.push((
        "F49069/告警拒收",
        hits == cases && cases == 10,
        format!("codes={}/{}", hits, cases),
    ));

    // 3) 账行回读：整板导行逐条往返；截断与扩写一律拒收；十次导出一致。
    let ok;
    let mut info = String::from("导行失败");
    match synth_alarms(0x6903) {
        Ok(board) => {
            let rows = alarm_rows(&board);
            let mut all = !rows.is_empty() && rows.len() == board.len();
            let mut i = 0usize;
            while i < rows.len() {
                match rows.get(i) {
                    Some(row) => {
                        if !audit_alarm(row) {
                            all = false;
                        }
                        let mut longer = String::from(row.as_str());
                        longer.push('x');
                        if audit_alarm(&longer) {
                            all = false;
                        }
                    }
                    None => all = false,
                }
                i += 1;
            }
            if audit_alarm("") {
                all = false;
            }
            all = all && rows_stable(&board, ROUNDS) && board.verify();
            ok = all;
            info = format!("rows={} stable={}", rows.len(), rows_stable(&board, ROUNDS));
        }
        Err(_) => ok = false,
    }
    out.push(("F49069/账行回读", ok, info));

    // 4) 合流十轮：每条告警在十轮中各出现一次，记录总数守恒、次序自洽。
    let ok;
    let mut info = String::from("合流失败");
    match synth_alarms(0x6904) {
        Ok(board) => match merge_ten_rounds(&board) {
            Ok(log) => {
                let mut all = log.verify()
                    && log.len() == board.len().saturating_mul(ROUNDS as usize)
                    && !log.is_empty();
                let mut i = 0usize;
                while i < board.len() {
                    match board.get(i) {
                        Some(e) => {
                            if log.occurrences(e.id) != ROUNDS {
                                all = false;
                            }
                            if !log.contains(e.id) {
                                all = false;
                            }
                        }
                        None => all = false,
                    }
                    i += 1;
                }
                let mut r = 0usize;
                while r < log.len() {
                    match log.get(r) {
                        Some(rec) => {
                            if rec.kind != LogKind::Alarm || rec.event_id == 0 {
                                all = false;
                            }
                        }
                        None => all = false,
                    }
                    r += 1;
                }
                ok = all;
                info = format!(
                    "events={} records={} rounds={}",
                    board.len(),
                    log.len(),
                    ROUNDS
                );
            }
            Err(_) => ok = false,
        },
        Err(_) => ok = false,
    }
    out.push(("F49070/合流十轮", ok, info));

    // 5) 次序守恒：键必须严格递增；轮/日/序越界与容量满各有专属码。
    let mut ok = true;
    let mut bits = String::new();
    let mut log = MergedLog::new();
    if log.push(0, 0, 0, LogKind::Alarm, 1).is_err() {
        ok = false;
        bits.push_str("first refused; ");
    }
    if log.push(0, 0, 0, LogKind::Merge, 2).err() != Some(AlarmCode::OrderBad.val()) {
        ok = false;
        bits.push_str("order pass; ");
    }
    if log.push(0, 0, 1, LogKind::Merge, 2).is_err() {
        ok = false;
        bits.push_str("next refused; ");
    }
    if log.push(ROUNDS, 0, 2, LogKind::Alarm, 3).err() != Some(AlarmCode::RoundBad.val()) {
        ok = false;
        bits.push_str("round pass; ");
    }
    if log.push(0, DAYS, 2, LogKind::Alarm, 3).err() != Some(AlarmCode::SeqBad.val()) {
        ok = false;
        bits.push_str("day pass; ");
    }
    if log.push(0, 0, SEQS_PER_DAY, LogKind::Alarm, 3).err() != Some(AlarmCode::SeqBad.val()) {
        ok = false;
        bits.push_str("seq pass; ");
    }
    if log.push(0, 0, 2, LogKind::Alarm, 0).err() != Some(AlarmCode::IdBad.val()) {
        ok = false;
        bits.push_str("id pass; ");
    }
    let mut full = MergedLog::new();
    let mut hit_cap = false;
    let mut r = 0u32;
    while r < ROUNDS && !hit_cap {
        let mut q = 0u32;
        while q < SEQS_PER_DAY {
            match full.push(r, 0, q, LogKind::Merge, q + 1) {
                Ok(()) => {}
                Err(code) => {
                    if code == AlarmCode::CapacityBad.val() {
                        hit_cap = true;
                    } else {
                        ok = false;
                    }
                    break;
                }
            }
            q = q.saturating_add(1);
        }
        r = r.saturating_add(1);
    }
    if !hit_cap || full.len() != LOG_CAP {
        ok = false;
        bits.push_str("cap miss; ");
    }
    if ok && bits.is_empty() {
        bits = format!("keys/limits ok cap={}", full.len());
    }
    out.push(("F49070/次序守恒", ok, bits));

    // 6) 去重窗口：窗口内重复拒收；逐出后旧号可再次入板；拒收计数守恒。
    let mut board = AlarmBoard::new();
    let mut all = true;
    match AlarmEvent::new(1, 0, Metric::P50, 1_000, 1_100, 0, 0) {
        Ok(e) => {
            if board.push(e).is_err() {
                all = false;
            }
            if board.push(e).err() != Some(AlarmCode::DedupBad.val()) {
                all = false;
            }
            let mut i = 2u32;
            while i <= 9 {
                match AlarmEvent::new(i, 0, Metric::P95, 1_000, 1_200, 0, i - 1) {
                    Ok(x) => {
                        if board.push(x).is_err() {
                            all = false;
                        }
                    }
                    Err(_) => all = false,
                }
                i = i.saturating_add(1);
            }
            match AlarmEvent::new(1, 0, Metric::P99, 1_000, 1_300, 0, 9) {
                Ok(x) => {
                    if board.push(x).is_err() {
                        all = false;
                    }
                }
                Err(_) => all = false,
            }
        }
        Err(_) => all = false,
    }
    let ok_all = all
        && board.rejected() == 1
        && board.len() == 10
        && board.dedup_len() == DEDUP_WINDOW
        && board.verify();
    let info = format!(
        "len={} rejected={} window={}",
        board.len(),
        board.rejected(),
        board.dedup_len()
    );
    out.push(("F49070/去重窗口", ok_all, info));

    // 7) 复跑终验：十次重建摘要一致；零轮/超限轮各自被拒；导行全数可回读。
    let mut ok = false;
    let mut info = String::from("复跑失败");
    if let Ok(board) = synth_alarms(0x6907) {
        let replay = merge_replay(&board, ROUNDS);
        let zero = merge_rounds(&board, 0).err() == Some(AlarmCode::ReplayBad.val());
        let over = merge_rounds(&board, ROUND_LIMIT + 1).err()
            == Some(AlarmCode::Overflow.val());
        let mut stable = true;
        match merge_ten_rounds(&board) {
            Ok(log) => {
                let rows_a = log_rows(&log);
                let rows_b = log_rows(&log);
                if rows_a.len() != rows_b.len() || rows_a.is_empty() {
                    stable = false;
                }
                let mut i = 0usize;
                while i < rows_a.len() {
                    if rows_a.get(i) != rows_b.get(i) {
                        stable = false;
                    }
                    match rows_a.get(i) {
                        Some(row) => {
                            if !audit_log(row) {
                                stable = false;
                            }
                        }
                        None => stable = false,
                    }
                    i += 1;
                }
                if !log.verify() {
                    stable = false;
                }
            }
            Err(_) => stable = false,
        }
        ok = replay && zero && over && stable;
        info = format!(
            "replay={} zero={} over={} rows={}",
            replay, zero, over, stable
        );
    }
    out.push(("F49070/复跑终验", ok, info));

    // 8) 异常码守卫：14 码满窗、段外说明不泄漏、坏账行全拒、分档边界不漂移。
    let mut ok = segment_guard();
    let mut bits = String::new();
    if !ok {
        bits.push_str("guard red; ");
    }
    if detail(SEG_BASE) != UNKNOWN || detail(SEG_BASE + 15) != UNKNOWN {
        ok = false;
        bits.push_str("detail leak; ");
    }
    let sev_ok = Severity::for_levels(1_000, 1_249) == Severity::Warn
        && Severity::for_levels(1_000, 1_250) == Severity::Critical
        && Severity::for_levels(1_000, 1_000) == Severity::Warn;
    if !sev_ok {
        ok = false;
        bits.push_str("severity drift; ");
    }
    let bad_alarm = [
        "",
        "ALRM|id=1|seg=0|metric=p50|thr=1000|obs=1100|sev=warn",
        "ALRM|id=1|seg=0|metric=p50|thr=1000|obs=1100|sev=warn|digest=00000000000000zz",
        "ALRM|seg=0|id=1|metric=p50|thr=1000|obs=1100|sev=warn|digest=0000000000000000",
        "ALRM|id=1|seg=0|metric=p50|thr=1000|obs=1100|sev=warn|digest=0000000000000000|x=1",
        "XLOG|round=0|day=0|seq=0|kind=alarm|evt=1|digest=0000000000000000",
    ];
    let mut accepted = 0u32;
    let mut k = 0usize;
    while k < bad_alarm.len() {
        if let Some(row) = bad_alarm.get(k) {
            if audit_alarm(row) {
                accepted = accepted.saturating_add(1);
            }
        }
        k += 1;
    }
    let bad_log = [
        "XLOG|round=0|day=0|seq=0|kind=alarm|evt=1",
        "XLOG|round=0|day=0|seq=0|kind=alarm|evt=1|digest=00000000000000zz",
        "LOG|round=0|day=0|seq=0|kind=alarm|evt=1|digest=0000000000000000",
        "XLOG|round=10|day=0|seq=0|kind=alarm|evt=1|digest=0000000000000000",
    ];
    let mut log_accepted = 0u32;
    let mut k = 0usize;
    while k < bad_log.len() {
        if let Some(row) = bad_log.get(k) {
            if audit_log(row) {
                log_accepted = log_accepted.saturating_add(1);
            }
        }
        k += 1;
    }
    if accepted != 0 || log_accepted != 0 {
        ok = false;
        bits.push_str("bad row accepted; ");
    }
    if ok && bits.is_empty() {
        bits = format!(
            "14 codes/bad={} log_bad={} ok",
            bad_alarm.len(),
            bad_log.len()
        );
    }
    out.push(("F49069/异常码守卫", ok, bits));

    out
}

/// 域自检入口。
pub fn run_unx49069_checks() -> CheckSet {
    let mut set = CheckSet::new("UNX-F49069");
    let rows = extra_checks();
    let mut i = 0usize;
    while i < rows.len() {
        if let Some((name, passed, detail)) = rows.get(i) {
            set.add_dyn(name, *passed, detail.as_str());
        }
        i += 1;
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(seed: u64) -> AlarmBoard {
        match synth_alarms(seed) {
            Ok(v) => v,
            Err(_) => AlarmBoard::new(),
        }
    }

    #[test]
    fn code_vals_in_window() {
        assert_eq!(AlarmCode::SegBad.val(), SEG_BASE + 1);
        assert_eq!(AlarmCode::Overflow.val(), SEG_BASE + 14);
        assert!(segment_guard());
    }

    #[test]
    fn code_details_distinct() {
        let mut i = 0usize;
        while i < AlarmCode::ALL.len() {
            let a = AlarmCode::ALL[i];
            assert_ne!(a.message(), UNKNOWN);
            let mut j = i + 1;
            while j < AlarmCode::ALL.len() {
                assert_ne!(a.message(), AlarmCode::ALL[j].message());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn detail_unknown_outside_segment() {
        assert_eq!(detail(SEG_BASE), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 15), UNKNOWN);
        assert_eq!(detail(0), UNKNOWN);
        assert_eq!(detail(u32::MAX), UNKNOWN);
    }

    #[test]
    fn metric_roundtrip() {
        let mut i = 0u8;
        while i < METRICS as u8 {
            let m = Metric::from_index(i).expect("metric");
            assert_eq!(m.index(), i);
            assert_eq!(Metric::from_name(m.name()), Some(m));
            i += 1;
        }
        assert!(Metric::from_index(3).is_err());
        assert!(Metric::from_name("p999").is_none());
    }

    #[test]
    fn metric_names_distinct() {
        assert_eq!(Metric::P50.name(), "p50");
        assert_eq!(Metric::P95.name(), "p95");
        assert_eq!(Metric::P99.name(), "p99");
    }

    #[test]
    fn severity_names_roundtrip() {
        assert_eq!(Severity::Warn.index(), 0);
        assert_eq!(Severity::Critical.index(), 1);
        assert_eq!(Severity::from_name("warn"), Some(Severity::Warn));
        assert_eq!(Severity::from_name("critical"), Some(Severity::Critical));
        assert!(Severity::from_name("fatal").is_none());
    }

    #[test]
    fn severity_critical_boundary() {
        assert_eq!(Severity::for_levels(1_000, 1_249), Severity::Warn);
        assert_eq!(Severity::for_levels(1_000, 1_250), Severity::Critical);
        assert_eq!(Severity::for_levels(2_000, 2_500), Severity::Critical);
    }

    #[test]
    fn severity_warn_below() {
        assert_eq!(Severity::for_levels(1_000, 1_001), Severity::Warn);
        assert_eq!(Severity::for_levels(1_000, 1_000), Severity::Warn);
    }

    #[test]
    fn event_new_ok() {
        let e = AlarmEvent::new(7, 2, Metric::P95, 3_600, 4_000, 1, 2).expect("event");
        assert_eq!(e.id, 7);
        assert_eq!(e.seg, 2);
        assert_eq!(e.metric, Metric::P95);
        assert!(e.verify());
        assert!(e.three_elements());
        assert!(e.exceeded());
    }

    #[test]
    fn event_rejects_zero_id() {
        assert_eq!(
            AlarmEvent::new(0, 0, Metric::P50, 1_000, 1_100, 0, 0).err(),
            Some(AlarmCode::IdBad.val())
        );
    }

    #[test]
    fn event_rejects_bad_seg() {
        assert_eq!(
            AlarmEvent::new(1, 4, Metric::P50, 1_000, 1_100, 0, 0).err(),
            Some(AlarmCode::SegBad.val())
        );
    }

    #[test]
    fn event_rejects_threshold_zero() {
        assert_eq!(
            AlarmEvent::new(1, 0, Metric::P50, 0, 1_100, 0, 0).err(),
            Some(AlarmCode::ThresholdBad.val())
        );
    }

    #[test]
    fn event_rejects_threshold_over_limit() {
        assert_eq!(
            AlarmEvent::new(1, 0, Metric::P50, NS_LIMIT + 1, 1_100, 0, 0).err(),
            Some(AlarmCode::ThresholdBad.val())
        );
    }

    #[test]
    fn event_rejects_observed_not_exceeding() {
        assert_eq!(
            AlarmEvent::new(1, 0, Metric::P50, 1_000, 1_000, 0, 0).err(),
            Some(AlarmCode::ObservedBad.val())
        );
        assert_eq!(
            AlarmEvent::new(1, 0, Metric::P50, 1_000, 999, 0, 0).err(),
            Some(AlarmCode::ObservedBad.val())
        );
    }

    #[test]
    fn event_rejects_observed_over_limit() {
        assert_eq!(
            AlarmEvent::new(1, 0, Metric::P50, 1_000, NS_LIMIT + 1, 0, 0).err(),
            Some(AlarmCode::ObservedBad.val())
        );
    }

    #[test]
    fn event_rejects_day() {
        assert_eq!(
            AlarmEvent::new(1, 0, Metric::P50, 1_000, 1_100, DAYS, 0).err(),
            Some(AlarmCode::SeqBad.val())
        );
    }

    #[test]
    fn event_rejects_seq() {
        assert_eq!(
            AlarmEvent::new(1, 0, Metric::P50, 1_000, 1_100, 0, SEQS_PER_DAY).err(),
            Some(AlarmCode::SeqBad.val())
        );
    }

    #[test]
    fn event_ratio_and_excess() {
        let e = AlarmEvent::new(1, 1, Metric::P99, 1_600, 2_000, 0, 0).expect("event");
        assert_eq!(e.ratio_bp(), 1_250);
        assert_eq!(e.excess_ns(), 400);
        assert_eq!(e.severity(), Severity::Critical);
    }

    #[test]
    fn event_verify_detects_tamper() {
        let mut e = AlarmEvent::new(1, 0, Metric::P50, 1_000, 1_100, 0, 0).expect("event");
        e.observed_ns = 1_101;
        assert!(!e.verify());
    }

    #[test]
    fn event_order_key_increasing() {
        let a = AlarmEvent::new(1, 0, Metric::P50, 1_000, 1_100, 0, 0).expect("a");
        let b = AlarmEvent::new(2, 0, Metric::P50, 1_000, 1_100, 0, 1).expect("b");
        let c = AlarmEvent::new(3, 0, Metric::P50, 1_000, 1_100, 1, 0).expect("c");
        assert!(a.order_key() < b.order_key());
        assert!(b.order_key() < c.order_key());
    }

    #[test]
    fn dedup_empty() {
        let w = DedupWindow::new();
        assert!(w.is_empty());
        assert_eq!(w.len(), 0);
        assert_eq!(w.capacity(), DEDUP_WINDOW);
        assert!(!w.contains(1));
    }

    #[test]
    fn dedup_contains_and_len() {
        let mut w = DedupWindow::new();
        w.remember(3);
        w.remember(4);
        assert!(w.contains(3));
        assert!(w.contains(4));
        assert!(!w.contains(5));
        assert_eq!(w.len(), 2);
    }

    #[test]
    fn dedup_evicts_oldest() {
        let mut w = DedupWindow::new();
        let mut i = 1u32;
        while i <= DEDUP_WINDOW as u32 {
            w.remember(i);
            i += 1;
        }
        assert!(w.contains(1));
        w.remember(99);
        assert!(!w.contains(1));
        assert!(w.contains(99));
        assert_eq!(w.len(), DEDUP_WINDOW);
    }

    #[test]
    fn board_new_empty() {
        let b = AlarmBoard::new();
        assert!(b.is_empty());
        assert_eq!(b.len(), 0);
        assert_eq!(b.rejected(), 0);
        assert!(b.verify());
        assert_eq!(b.digest(), p_core::FNV_OFF);
        assert!(b.find(1).is_none());
        assert!(alarm_rows(&b).is_empty());
    }

    #[test]
    fn board_push_and_verify() {
        let mut b = AlarmBoard::new();
        let e = AlarmEvent::new(1, 0, Metric::P50, 1_000, 1_100, 0, 0).expect("e");
        assert!(b.push(e).is_ok());
        assert_eq!(b.len(), 1);
        assert!(b.verify());
        assert_ne!(b.digest(), p_core::FNV_OFF);
        assert_eq!(b.get(0), Some(&e));
    }

    #[test]
    fn board_find() {
        let b = board(0x6911);
        let mut i = 0usize;
        while i < b.len() {
            let e = b.get(i).expect("e");
            assert_eq!(b.find(e.id).map(|x| x.id), Some(e.id));
            i += 1;
        }
        assert!(b.find(0xFFFF_FFFF).is_none());
    }

    #[test]
    fn board_dedup_rejects() {
        let mut b = AlarmBoard::new();
        let e = AlarmEvent::new(1, 0, Metric::P50, 1_000, 1_100, 0, 0).expect("e");
        assert!(b.push(e).is_ok());
        assert_eq!(b.push(e).err(), Some(AlarmCode::DedupBad.val()));
        assert_eq!(b.rejected(), 1);
    }

    #[test]
    fn board_order_rejects() {
        let mut b = AlarmBoard::new();
        let a = AlarmEvent::new(1, 0, Metric::P50, 1_000, 1_100, 0, 1).expect("a");
        let c = AlarmEvent::new(2, 0, Metric::P50, 1_000, 1_100, 0, 0).expect("c");
        assert!(b.push(a).is_ok());
        assert_eq!(b.push(c).err(), Some(AlarmCode::OrderBad.val()));
        assert_eq!(b.rejected(), 1);
    }

    #[test]
    fn board_capacity() {
        let mut b = AlarmBoard::new();
        let mut i = 0u32;
        while i < BOARD_CAP as u32 {
            let day = i / 4;
            let seq = i % 4;
            let e = AlarmEvent::new(i + 1, 0, Metric::P50, 1_000, 1_100, day, seq).expect("e");
            assert!(b.push(e).is_ok());
            i += 1;
        }
        let extra = AlarmEvent::new(999, 0, Metric::P50, 1_000, 1_100, 4, 0).expect("x");
        assert_eq!(b.push(extra).err(), Some(AlarmCode::CapacityBad.val()));
        assert_eq!(b.len(), BOARD_CAP);
    }

    #[test]
    fn board_counts_severity() {
        let mut b = AlarmBoard::new();
        let w = AlarmEvent::new(1, 0, Metric::P50, 1_000, 1_100, 0, 0).expect("w");
        let c = AlarmEvent::new(2, 0, Metric::P50, 1_000, 1_300, 0, 1).expect("c");
        assert!(b.push(w).is_ok());
        assert!(b.push(c).is_ok());
        assert_eq!(b.count_severity(Severity::Warn), 1);
        assert_eq!(b.count_severity(Severity::Critical), 1);
    }

    #[test]
    fn board_stats_total() {
        let b = board(0x6921);
        let s = board_stats(&b);
        assert_eq!(s.total as usize, b.len());
        assert_eq!(s.warn + s.critical, s.total);
        assert!(s.max_ratio_bp > PERMILLE);
    }

    #[test]
    fn board_digest_stable() {
        let a = board(0x6931);
        let b = board(0x6931);
        assert_eq!(a, b);
        assert_eq!(a.digest(), b.digest());
        assert_eq!(a.recompute(), a.digest());
    }

    #[test]
    fn log_push_and_verify() {
        let mut log = MergedLog::new();
        assert!(log.push(0, 0, 0, LogKind::Alarm, 1).is_ok());
        assert!(log.push(0, 0, 1, LogKind::Merge, 2).is_ok());
        assert_eq!(log.len(), 2);
        assert!(log.verify());
        assert_eq!(log.occurrences(1), 1);
        assert!(log.contains(2));
        assert!(!log.contains(3));
    }

    #[test]
    fn log_order_rejects() {
        let mut log = MergedLog::new();
        assert!(log.push(0, 0, 1, LogKind::Alarm, 1).is_ok());
        assert_eq!(
            log.push(0, 0, 0, LogKind::Alarm, 2).err(),
            Some(AlarmCode::OrderBad.val())
        );
        assert_eq!(
            log.push(0, 0, 1, LogKind::Alarm, 2).err(),
            Some(AlarmCode::OrderBad.val())
        );
    }

    #[test]
    fn log_round_bounds() {
        let mut log = MergedLog::new();
        assert_eq!(
            log.push(ROUNDS, 0, 0, LogKind::Alarm, 1).err(),
            Some(AlarmCode::RoundBad.val())
        );
    }

    #[test]
    fn log_day_seq_bounds() {
        let mut log = MergedLog::new();
        assert_eq!(
            log.push(0, DAYS, 0, LogKind::Alarm, 1).err(),
            Some(AlarmCode::SeqBad.val())
        );
        assert_eq!(
            log.push(0, 0, SEQS_PER_DAY, LogKind::Alarm, 1).err(),
            Some(AlarmCode::SeqBad.val())
        );
    }

    #[test]
    fn log_id_zero() {
        let mut log = MergedLog::new();
        assert_eq!(
            log.push(0, 0, 0, LogKind::Alarm, 0).err(),
            Some(AlarmCode::IdBad.val())
        );
    }

    #[test]
    fn log_capacity() {
        let mut log = MergedLog::new();
        let mut r = 0u32;
        let mut hit = false;
        while r < ROUNDS && !hit {
            let mut q = 0u32;
            while q < SEQS_PER_DAY {
                if log.push(r, 0, q, LogKind::Merge, q + 1).is_err() {
                    hit = true;
                    break;
                }
                q += 1;
            }
            r += 1;
        }
        assert!(hit);
        assert_eq!(log.len(), LOG_CAP);
        assert!(log.verify());
    }

    #[test]
    fn log_kind_names() {
        assert_eq!(LogKind::Alarm.name(), "alarm");
        assert_eq!(LogKind::Merge.name(), "merge");
        assert_eq!(LogKind::from_name("alarm"), Some(LogKind::Alarm));
        assert_eq!(LogKind::from_name("merge"), Some(LogKind::Merge));
        assert!(LogKind::from_name("audit").is_none());
        assert_eq!(LogKind::Alarm.index(), 0);
        assert_eq!(LogKind::Merge.index(), 1);
    }

    #[test]
    fn merge_rounds_shape() {
        let b = board(0x6941);
        let log = merge_rounds(&b, ROUNDS).expect("log");
        assert_eq!(log.len(), b.len() * ROUNDS as usize);
        assert!(log.verify());
        assert_ne!(log.digest(), p_core::FNV_OFF);
    }

    #[test]
    fn merge_rounds_zero() {
        let b = board(0x6942);
        assert_eq!(
            merge_rounds(&b, 0).err(),
            Some(AlarmCode::ReplayBad.val())
        );
    }

    #[test]
    fn merge_rounds_overflow() {
        let b = board(0x6943);
        assert_eq!(
            merge_rounds(&b, ROUND_LIMIT + 1).err(),
            Some(AlarmCode::Overflow.val())
        );
    }

    #[test]
    fn merge_ten_rounds_occurrences() {
        let b = board(0x6944);
        let log = merge_ten_rounds(&b).expect("log");
        let mut i = 0usize;
        while i < b.len() {
            let e = b.get(i).expect("e");
            assert_eq!(log.occurrences(e.id), ROUNDS);
            i += 1;
        }
    }

    #[test]
    fn merge_replay_true() {
        let b = board(0x6945);
        assert!(merge_replay(&b, ROUNDS));
        assert!(!merge_replay(&b, 0));
    }

    #[test]
    fn merge_empty_board() {
        let b = AlarmBoard::new();
        let log = merge_ten_rounds(&b).expect("log");
        assert!(log.is_empty());
        assert!(log.verify());
        assert!(merge_replay(&b, 3));
    }

    #[test]
    fn render_alarm_roundtrip() {
        let b = board(0x6951);
        let rows = alarm_rows(&b);
        assert_eq!(rows.len(), b.len());
        let mut i = 0usize;
        while i < rows.len() {
            assert!(audit_alarm(rows[i].as_str()));
            i += 1;
        }
    }

    #[test]
    fn audit_alarm_rejects_tampered() {
        let b = board(0x6952);
        let row = match alarm_rows(&b).first() {
            Some(r) => String::from(r.as_str()),
            None => return,
        };
        let mut longer = row;
        longer.push('0');
        assert!(!audit_alarm(&longer));
        assert!(!audit_alarm(""));
        assert!(!audit_alarm("ALRM"));
    }

    #[test]
    fn audit_alarm_rejects_wrong_digest() {
        let b = board(0x6953);
        let row = match alarm_rows(&b).first() {
            Some(r) => String::from(r.as_str()),
            None => return,
        };
        let mut chars: Vec<char> = row.chars().collect();
        let n = chars.len();
        if n > 0 {
            chars[n - 1] = if chars[n - 1] == '0' { '1' } else { '0' };
        }
        let tampered: String = chars.into_iter().collect();
        assert!(!audit_alarm(&tampered));
    }

    #[test]
    fn rows_stable_ten() {
        let b = board(0x6954);
        assert!(rows_stable(&b, ROUNDS));
        assert!(!rows_stable(&b, 0));
    }

    #[test]
    fn render_log_roundtrip() {
        let b = board(0x6955);
        let log = merge_ten_rounds(&b).expect("log");
        let rows = log_rows(&log);
        assert_eq!(rows.len(), log.len());
        let mut i = 0usize;
        while i < rows.len() {
            assert!(audit_log(rows[i].as_str()));
            i += 1;
        }
    }

    #[test]
    fn audit_log_rejects_bad() {
        assert!(!audit_log(""));
        assert!(!audit_log("XLOG|round=0|day=0|seq=0|kind=alarm|evt=1"));
        assert!(!audit_log(
            "XLOG|round=0|day=0|seq=0|kind=alarm|evt=1|digest=000000000000000g"
        ));
        assert!(!audit_log(
            "XLOG|round=10|day=0|seq=0|kind=alarm|evt=1|digest=0000000000000000"
        ));
    }

    #[test]
    fn synth_deterministic() {
        let a = synth_alarms(0x6961).expect("a");
        let b = synth_alarms(0x6961).expect("b");
        assert_eq!(a, b);
        assert_eq!(a.digest(), b.digest());
    }

    #[test]
    fn synth_seed_sensitive() {
        let a = synth_alarms(0x6961).expect("a");
        let b = synth_alarms(0x6963).expect("b");
        assert_ne!(a.digest(), b.digest());
    }

    #[test]
    fn synth_ids_unique() {
        let b = board(0x6971);
        let mut ids: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < b.len() {
            let e = b.get(i).expect("e");
            assert!(!ids.contains(&e.id));
            ids.push(e.id);
            i += 1;
        }
    }

    #[test]
    fn synth_three_elements() {
        let b = board(0x6972);
        assert!(!b.is_empty());
        let mut i = 0usize;
        while i < b.len() {
            let e = b.get(i).expect("e");
            assert!(e.three_elements());
            assert!(e.verify());
            assert!(e.ratio_bp() > PERMILLE);
            i += 1;
        }
    }

    #[test]
    fn extreme_values_ok() {
        let e = AlarmEvent::new(
            1,
            3,
            Metric::P99,
            NS_LIMIT - 1,
            NS_LIMIT,
            DAYS - 1,
            SEQS_PER_DAY - 1,
        )
        .expect("extreme");
        assert!(e.verify());
        assert_eq!(e.excess_ns(), 1);
        assert_eq!(e.order_key(), (DAYS - 1) * SEQS_PER_DAY + SEQS_PER_DAY - 1);
    }

    #[test]
    fn checks_all_green() {
        let set = run_unx49069_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn checks_len_and_truncation() {
        let set = run_unx49069_checks();
        assert_eq!(set.len(), 8);
        assert!(!set.truncated());
        assert_eq!(set.passed_count(), 8);
        assert_eq!(set.failed_count(), 0);
    }

    #[test]
    fn checks_names_distinct() {
        let set = run_unx49069_checks();
        let mut names: Vec<&'static str> = Vec::new();
        for c in set.iter() {
            assert!(!names.contains(&c.name));
            names.push(c.name);
        }
        assert_eq!(names.len(), 8);
    }

    #[test]
    fn checks_detail_nonempty() {
        for (name, passed, det) in extra_checks() {
            assert!(passed, "{} red: {}", name, det);
            assert!(!det.is_empty());
        }
    }

    #[test]
    fn alarm_fields_parse_roundtrip() {
        let e = AlarmEvent::new(9, 2, Metric::P99, 3_600, 4_500, 0, 0).expect("e");
        let row = render_alarm(&e);
        let fields = row_fields(&row, ALARM_TAG, &ALARM_KEYS).expect("fields");
        assert_eq!(fields.len(), ALARM_KEYS.len() + 1);
        assert_eq!(fields.first().copied(), Some("9"));
        assert_eq!(fields.get(1).copied(), Some("2"));
        assert_eq!(fields.get(2).copied(), Some("p99"));
        assert_eq!(fields.get(5).copied(), Some("critical"));
    }

    #[test]
    fn log_fields_parse_roundtrip() {
        let r = LogRecord {
            round: 3,
            day: 4,
            seq: 5,
            kind: LogKind::Merge,
            event_id: 6,
            digest: record_digest(3, 4, 5, LogKind::Merge, 6),
        };
        let row = render_log(&r);
        let fields = row_fields(&row, LOG_TAG, &LOG_KEYS).expect("fields");
        assert_eq!(fields.len(), LOG_KEYS.len() + 1);
        assert_eq!(fields.first().copied(), Some("3"));
        assert_eq!(fields.get(3).copied(), Some("merge"));
        assert!(audit_log(&row));
    }

    #[test]
    fn row_fields_rejects_tag() {
        assert!(row_fields("", ALARM_TAG, &ALARM_KEYS).is_none());
        assert!(row_fields("NOPE|id=1", ALARM_TAG, &ALARM_KEYS).is_none());
        assert!(row_fields("ALRM|id=1", ALARM_TAG, &ALARM_KEYS).is_none());
    }

    #[test]
    fn merge_ten_rounds_digest_stable() {
        let b = board(0x6981);
        let a = merge_ten_rounds(&b).expect("a");
        let c = merge_ten_rounds(&b).expect("c");
        assert_eq!(a.digest(), c.digest());
        assert_eq!(a.len(), c.len());
    }

    #[test]
    fn board_rejected_counts() {
        let mut b = AlarmBoard::new();
        let e = AlarmEvent::new(1, 0, Metric::P50, 1_000, 1_100, 0, 0).expect("e");
        assert!(b.push(e).is_ok());
        assert!(b.push(e).is_err());
        assert!(b.push(e).is_err());
        assert_eq!(b.rejected(), 2);
        assert_eq!(b.len(), 1);
    }
}
