//! m7timelog — VARIX-M700 AI-23 系统时间与日志域 (F551~F575)
//!
//! 墙钟仲裁、单调时钟、UTC 纪律、时间戳链、结构化日志、环形缓冲、
//! 速率官、订阅总线、持久舱、脱敏、跳变协议、血缘标签、金样本、
//! 统计分账、自描述导出、测量仪、压缩舱、隐私分级、时间事件流、
//! 时钟健康分、压力剧本、回归走廊、文档、考古指南、年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点。

use crate::checks::CheckSet;

// ===========================================================================
// F551 — 墙钟仲裁官：RTC/NTP/PTP 多源
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum ClockSource {
    Rtc,
    Ntp,
    Ptp,
}

pub fn source_priority(s: ClockSource) -> u8 {
    match s {
        ClockSource::Ptp => 0,
        ClockSource::Ntp => 1,
        ClockSource::Rtc => 2,
    }
}

pub fn wall_clock_arbitrate(sources: [Option<(ClockSource, u64)>; 3]) -> Option<u64> {
    let mut best: Option<(u8, u64)> = None;
    for s in sources.iter().flatten() {
        let p = source_priority(s.0);
        match best {
            Some((bp, _)) if bp <= p => {}
            _ => best = Some((p, s.1)),
        }
    }
    best.map(|(_, t)| t)
}

// ===========================================================================
// F552 — 单调时钟宪法
// ===========================================================================

#[derive(Clone, Copy)]
pub struct MonoClock {
    pub base_ns: u64,
    pub last_ns: u64,
}

impl MonoClock {
    /// 单调性：读数永不回退。
    pub fn read(&mut self, raw_ns: u64) -> u64 {
        let t = self.base_ns.saturating_add(raw_ns);
        if t < self.last_ns {
            self.last_ns
        } else {
            self.last_ns = t;
            t
        }
    }
}

// ===========================================================================
// F553 — 时区内核态度：只存 UTC
// ===========================================================================

pub fn kernel_stores_utc(storage_offset_min: i32) -> bool {
    storage_offset_min == 0
}

// ===========================================================================
// F554 — 事件时间戳链
// ===========================================================================

pub const TS_FORMAT_VER: u8 = 2;

#[derive(Clone, Copy)]
pub struct Timestamped {
    pub seq: u64,
    pub mono_ns: u64,
    pub wall_s: u64,
}

pub fn ts_chain_valid(ts: &Timestamped, prev_seq: u64) -> bool {
    ts.seq == prev_seq + 1
}

// ===========================================================================
// F555 — 结构化日志信封
// ===========================================================================

pub const LOG_LEVELS: [&str; 5] = ["trace", "debug", "info", "warn", "error"];

#[derive(Clone, Copy)]
pub struct LogEnvelope {
    pub level: u8,   // 0~4
    pub domain: u8,
    pub module: u16,
    pub seq: u64,
}

pub fn envelope_valid(e: &LogEnvelope) -> bool {
    e.level <= 4 && e.seq > 0
}

// ===========================================================================
// F556 — 日志环形缓冲
// ===========================================================================

pub const LOG_RING_CAP: usize = 64;

pub struct LogRing {
    pub head: usize,
    pub count: usize,
}

impl LogRing {
    pub fn push(&mut self) {
        self.head = (self.head + 1) % LOG_RING_CAP;
        if self.count < LOG_RING_CAP {
            self.count += 1;
        }
    }
    pub fn full(&self) -> bool {
        self.count == LOG_RING_CAP
    }
}

// ===========================================================================
// F557 — 日志速率官
// ===========================================================================

pub const LOG_RATE_LIMIT_PER_S: u32 = 1000;

pub fn log_rate_admit(issued_this_s: u32) -> bool {
    issued_this_s < LOG_RATE_LIMIT_PER_S
}

/// 相同内容聚合计数。
pub fn log_aggregate(last_key: u16, this_key: u16, last_count: u32) -> u32 {
    if last_key == this_key {
        last_count + 1
    } else {
        1
    }
}

// ===========================================================================
// F558 — 日志订阅总线
// ===========================================================================

pub const SUBSCRIBER_CAP: usize = 8;

#[derive(Clone, Copy)]
pub struct LogSubscriber {
    pub id: u8,
    pub min_level: u8, // 只收 >= min_level
    pub domains: u16,  // bitmask
}

pub fn subscriber_interested(s: &LogSubscriber, e: &LogEnvelope) -> bool {
    e.level >= s.min_level && e.domain < 16 && s.domains & (1 << e.domain) != 0
}

// ===========================================================================
// F559 — 持久日志舱
// ===========================================================================

pub const PERSIST_FLUSH_INTERVAL_S: u32 = 5;

pub fn persist_due(idle_s: u32) -> bool {
    idle_s >= PERSIST_FLUSH_INTERVAL_S
}

// ===========================================================================
// F560 — 日志脱敏官
// ===========================================================================

/// 敏感字段（密码/token）替换为等长星号。
pub fn redact_len(len: usize) -> usize {
    len.min(64)
}

pub fn looks_sensitive(field: &[u8]) -> bool {
    const KEYS: [&[u8]; 3] = [b"password", b"token", b"secret"];
    let lower: [u8; 16] = {
        let mut t = [0u8; 16];
        for (i, &b) in field.iter().take(16).enumerate() {
            t[i] = b.to_ascii_lowercase();
        }
        t
    };
    KEYS.iter().any(|k| lower.starts_with(k))
}

// ===========================================================================
// F561 — 时钟跳变协议
// ===========================================================================

pub const JUMP_WARN_S: u64 = 2;

#[derive(Clone, Copy, PartialEq)]
pub enum JumpKind {
    Small,
    Step,
    Backwards,
}

pub fn classify_jump(delta_ns: i64) -> JumpKind {
    if delta_ns < 0 {
        JumpKind::Backwards
    } else if delta_ns as u64 > JUMP_WARN_S * 1_000_000_000 {
        JumpKind::Step
    } else {
        JumpKind::Small
    }
}

// ===========================================================================
// F562 — 计时器血缘标签
// ===========================================================================

#[derive(Clone, Copy)]
pub struct TimerTag {
    pub timer_id: u32,
    pub owner_domain: u32,
    pub created_seq: u64,
}

pub fn timer_orphan(t: &TimerTag, live_domains: &[u32], n: usize) -> bool {
    let n = n.min(live_domains.len());
    !live_domains[..n].contains(&t.owner_domain)
}

// ===========================================================================
// F563 — 日志金样本
// ===========================================================================

pub const GOLDEN_BOOT_LINES: [&str; 5] =
    ["kernel: clocks calibrated", "kernel: vfs mounted", "shell: session start", "net: link up", "boot: complete"];

pub fn golden_match(line: &str, idx: usize) -> bool {
    idx < GOLDEN_BOOT_LINES.len() && GOLDEN_BOOT_LINES[idx] == line
}

// ===========================================================================
// F564 — 日志统计分账
// ===========================================================================

#[derive(Clone, Copy)]
pub struct LogStat {
    pub domain: u8,
    pub count: u64,
}

pub fn top_log_domains(stats: &mut [LogStat], n: usize) -> usize {
    let n = n.min(stats.len());
    for i in 1..n {
        let key = stats[i];
        let mut j = i;
        while j > 0 && stats[j - 1].count < key.count {
            stats[j] = stats[j - 1];
            j -= 1;
        }
        stats[j] = key;
    }
    n
}

// ===========================================================================
// F565 — 日志自描述导出
// ===========================================================================

pub struct LogConfig {
    pub ring_cap: usize,
    pub rate_limit: u32,
    pub persist_interval_s: u32,
}

pub fn log_config_exportable(c: &LogConfig) -> bool {
    c.ring_cap >= LOG_RING_CAP && c.rate_limit <= 100_000 && c.persist_interval_s >= 1
}

// ===========================================================================
// F566 — 时间测量仪
// ===========================================================================

pub struct Stopwatch {
    pub running: bool,
    pub start_ns: u64,
    pub acc_ns: u64,
}

impl Stopwatch {
    pub fn start(&mut self, now_ns: u64) {
        if !self.running {
            self.running = true;
            self.start_ns = now_ns;
        }
    }
    pub fn stop(&mut self, now_ns: u64) {
        if self.running {
            self.acc_ns += now_ns.saturating_sub(self.start_ns);
            self.running = false;
        }
    }
    pub fn elapsed_ns(&self) -> u64 {
        self.acc_ns
    }
}

// ===========================================================================
// F567 — 日志压缩舱
// ===========================================================================

/// 极简 RLE：连续重复字节压成 (byte, count)。
pub fn rle_compressed_size(data: &[u8], n: usize) -> usize {
    let n = n.min(data.len());
    if n == 0 {
        return 0;
    }
    let mut runs = 1usize;
    for i in 1..n {
        if data[i] != data[i - 1] {
            runs += 1;
        }
    }
    runs * 2
}

pub fn compression_worthwhile(data: &[u8], n: usize) -> bool {
    let n = n.min(data.len());
    n > 0 && rle_compressed_size(data, n) < n
}

// ===========================================================================
// F568 — 日志隐私分级
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum PrivacyClass {
    Public,
    Internal,
    Personal,
    Restricted,
}

pub fn export_approved(class: PrivacyClass, approver_ok: bool) -> bool {
    match class {
        PrivacyClass::Public | PrivacyClass::Internal => true,
        PrivacyClass::Personal | PrivacyClass::Restricted => approver_ok,
    }
}

// ===========================================================================
// F569 — 时间事件流
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum TimeEvent {
    Sync,
    Jump,
    DriftWarn,
    SourceLost,
}

pub fn time_event_notify(e: TimeEvent) -> bool {
    matches!(e, TimeEvent::Sync | TimeEvent::Jump | TimeEvent::DriftWarn | TimeEvent::SourceLost)
}

// ===========================================================================
// F570 — 时钟健康分
// ===========================================================================

pub struct ClockHealth {
    pub jumps_24h: u32,
    pub drift_ppb: u32, // 十亿分之漂移
}

impl ClockHealth {
    pub fn grade(&self) -> u8 {
        if self.jumps_24h == 0 && self.drift_ppb <= 100 {
            0
        } else if self.jumps_24h <= 2 && self.drift_ppb <= 1_000 {
            1
        } else {
            2
        }
    }
}

// ===========================================================================
// F571 — 日志压力剧本
// ===========================================================================

pub const LOG_STRESS_PLAYS: [&str; 4] = ["10k-s flood", "burst-1ms", "ring-wrap", "subscriber-slow"];

pub fn log_stress_known(name: &str) -> bool {
    LOG_STRESS_PLAYS.iter().any(|p| *p == name)
}

// ===========================================================================
// F572 — 日志回归走廊
// ===========================================================================

pub const LOG_CORRIDOR_CASES: [&str; 5] =
    ["envelope-format", "ring-wrap", "rate-limit", "redact-sensitive", "persist-flush"];

pub fn log_corridor_pass(results: &[bool; 5]) -> bool {
    results.iter().all(|&r| r)
}

// ===========================================================================
// F573 — 时间文档生成器
// ===========================================================================

pub const TIME_DOC_SECTIONS: [&str; 5] = ["clock-discipline", "mono-guarantees", "log-format", "redaction", "troubleshoot"];

pub fn time_doc_complete(marks: u8) -> bool {
    marks as u32 == (1u32 << TIME_DOC_SECTIONS.len()) - 1
}

// ===========================================================================
// F574 — 日志考古指南
// ===========================================================================

pub const DIG_STEPS: [&str; 5] =
    ["find-last-boot", "locate-first-error", "trace-seq-chain", "correlate-timestamps", "check-rate-aggregate"];

pub fn dig_step_known(name: &str) -> bool {
    DIG_STEPS.iter().any(|s| *s == name)
}

// ===========================================================================
// F575 — 时间域年报
// ===========================================================================

pub struct TimeYearbook {
    pub log_lines: u64,
    pub rate_limited: u64,
    pub jumps: u32,
    pub sync_events: u32,
}

impl TimeYearbook {
    pub fn rate_limited_permille(&self) -> u16 {
        if self.log_lines == 0 {
            return 0;
        }
        (((self.rate_limited as u64) * 1000 / self.log_lines as u64).min(1000)) as u16
    }
    pub fn clock_stable(&self) -> bool {
        self.jumps <= 4 && self.sync_events >= 1
    }
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m7timelog_checks() -> CheckSet {
    let mut set = CheckSet::new("m7timelog");

    // F551 仲裁
    let picked = wall_clock_arbitrate([
        Some((ClockSource::Rtc, 100)),
        Some((ClockSource::Ntp, 200)),
        Some((ClockSource::Ptp, 300)),
    ]);
    set.add(
        "F551 arbitrate",
        picked == Some(300) && wall_clock_arbitrate([None, None, None]).is_none(),
        "ptp wins / none",
    );

    // F552 单调
    let mut mc = MonoClock { base_ns: 1000, last_ns: 1000 };
    let a = mc.read(500);
    let b = mc.read(200); // 回拨被钳制
    set.add(
        "F552 monotonic",
        a == 1500 && b == 1500 && mc.last_ns == 1500,
        "no regress",
    );

    // F553 UTC
    set.add("F553 utc only", kernel_stores_utc(0) && !kernel_stores_utc(60), "offset must be 0");

    // F554 时间戳链
    let ts = Timestamped { seq: 5, mono_ns: 1, wall_s: 1 };
    set.add(
        "F554 ts chain",
        ts_chain_valid(&ts, 4) && !ts_chain_valid(&ts, 5) && TS_FORMAT_VER == 2,
        "seq+1",
    );

    // F555 信封
    let e = LogEnvelope { level: 3, domain: 2, module: 7, seq: 9 };
    set.add(
        "F555 envelope",
        envelope_valid(&e) && !envelope_valid(&LogEnvelope { level: 9, domain: 0, module: 0, seq: 1 }),
        "level bound",
    );

    // F556 环形缓冲
    let mut r = LogRing { head: 0, count: 0 };
    for _ in 0..LOG_RING_CAP + 5 {
        r.push();
    }
    set.add(
        "F556 ring",
        r.full() && r.count == LOG_RING_CAP && r.head == 5,
        "wrap",
    );

    // F557 速率官
    set.add(
        "F557 rate",
        log_rate_admit(999) && !log_rate_admit(1000)
            && log_aggregate(7, 7, 3) == 4 && log_aggregate(7, 8, 3) == 1,
        "limit + aggregate",
    );

    // F558 订阅
    let s = LogSubscriber { id: 1, min_level: 2, domains: 0b0100 };
    set.add(
        "F558 subscribe",
        subscriber_interested(&s, &LogEnvelope { level: 3, domain: 2, module: 0, seq: 1 })
            && !subscriber_interested(&s, &LogEnvelope { level: 1, domain: 2, module: 0, seq: 2 }),
        "level+domain filter",
    );

    // F559 持久
    set.add(
        "F559 persist",
        persist_due(5) && !persist_due(4) && PERSIST_FLUSH_INTERVAL_S == 5,
        "interval",
    );

    // F560 脱敏
    set.add(
        "F560 redact",
        looks_sensitive(b"Password1") && !looks_sensitive(b"user")
            && redact_len(100) == 64 && redact_len(8) == 8,
        "detect + cap",
    );

    // F561 跳变
    set.add(
        "F561 jump",
        classify_jump(1_000_000) == JumpKind::Small
            && classify_jump(5_000_000_000) == JumpKind::Step
            && classify_jump(-1) == JumpKind::Backwards,
        "3 kinds",
    );

    // F562 血缘
    let tag = TimerTag { timer_id: 1, owner_domain: 42, created_seq: 3 };
    let live = [1u32, 2, 3];
    set.add(
        "F562 lineage",
        timer_orphan(&tag, &live, 3) && !timer_orphan(&TimerTag { timer_id: 2, owner_domain: 2, created_seq: 1 }, &live, 3),
        "owner must be live",
    );

    // F563 金样本
    set.add(
        "F563 golden",
        golden_match("boot: complete", 4) && !golden_match("boot: complete", 0),
        "ordered match",
    );

    // F564 分账
    let mut st = [
        LogStat { domain: 1, count: 10 },
        LogStat { domain: 2, count: 90 },
        LogStat { domain: 3, count: 40 },
    ];
    top_log_domains(&mut st, 3);
    set.add("F564 stats", st[0].domain == 2 && st[2].domain == 1, "ranked");

    // F565 导出
    let lc = LogConfig { ring_cap: LOG_RING_CAP, rate_limit: 1000, persist_interval_s: 5 };
    set.add(
        "F565 export",
        log_config_exportable(&lc) && !log_config_exportable(&LogConfig { ring_cap: 8, rate_limit: 1000, persist_interval_s: 5 }),
        "config bounds",
    );

    // F566 测量仪
    let mut sw = Stopwatch { running: false, start_ns: 0, acc_ns: 0 };
    sw.start(100);
    sw.stop(400);
    sw.start(1000);
    sw.stop(1500);
    set.add("F566 stopwatch", sw.elapsed_ns() == 800 && !sw.running, "accumulates");

    // F567 压缩
    let runs = [7u8; 32];
    let mixed = [1u8, 2, 3, 4];
    set.add(
        "F567 compress",
        compression_worthwhile(&runs, 32) && !compression_worthwhile(&mixed, 4),
        "rle gate",
    );

    // F568 隐私分级
    set.add(
        "F568 privacy",
        export_approved(PrivacyClass::Internal, false)
            && !export_approved(PrivacyClass::Personal, false)
            && export_approved(PrivacyClass::Restricted, true),
        "approval gates",
    );

    // F569 事件流
    set.add(
        "F569 time events",
        time_event_notify(TimeEvent::Jump) && time_event_notify(TimeEvent::SourceLost),
        "4 events",
    );

    // F570 健康分
    let ch = ClockHealth { jumps_24h: 0, drift_ppb: 50 };
    let chb = ClockHealth { jumps_24h: 10, drift_ppb: 5000 };
    set.add("F570 clock health", ch.grade() == 0 && chb.grade() == 2, "drift+jump");

    // F571 压力
    set.add(
        "F571 stress",
        log_stress_known("ring-wrap") && !log_stress_known("wild"),
        "4 plays",
    );

    // F572 走廊
    set.add(
        "F572 corridor",
        log_corridor_pass(&[true; 5]) && !log_corridor_pass(&[true, true, true, false, true]),
        "5 cases",
    );

    // F573 文档
    set.add(
        "F573 docs",
        time_doc_complete(0b1_1111) && !time_doc_complete(0b1_1110),
        "5 sections",
    );

    // F574 考古
    set.add(
        "F574 dig guide",
        dig_step_known("locate-first-error") && !dig_step_known("pray"),
        "5 steps",
    );

    // F575 年报
    let yb = TimeYearbook { log_lines: 100_000, rate_limited: 2_000, jumps: 1, sync_events: 50 };
    set.add(
        "F575 yearbook",
        yb.rate_limited_permille() == 20 && yb.clock_stable(),
        "rate + stability",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f552_monotonic_chain() {
        let mut mc = MonoClock { base_ns: 0, last_ns: 0 };
        assert_eq!(mc.read(10), 10);
        assert_eq!(mc.read(5), 10);
        assert_eq!(mc.read(20), 20);
    }

    #[test]
    fn f560_redact_case_insensitive() {
        assert!(looks_sensitive(b"TOKEN_VALUE"));
        assert!(looks_sensitive(b"secretKey"));
        assert!(!looks_sensitive(b"count"));
    }

    #[test]
    fn f567_rle_sizes() {
        assert_eq!(rle_compressed_size(&[5u8; 10], 10), 2);
        assert_eq!(rle_compressed_size(&[], 0), 0);
    }

    #[test]
    fn f566_stopwatch_ignore_repeated_start() {
        let mut sw = Stopwatch { running: false, start_ns: 0, acc_ns: 0 };
        sw.start(0);
        sw.start(100); // 忽略
        sw.stop(300);
        assert_eq!(sw.elapsed_ns(), 300);
    }

    #[test]
    fn f575_domain_selfcheck_all_pass() {
        let set = run_m7timelog_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        for i in 0..set.len() {
            let c = set.get(i).unwrap();
            assert!(c.passed, "{} | {}", c.name, c.detail);
        }
    }
}
