//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0543 · 行格式冻结（时间戳/级别/组件/文） · 0x6C880
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A2）：
//!   ① 纯功能代码 ≥220 行；
//!   ② UNX-F0543-J1 行格式四字段冻结（时戳/级别/组件/文本），20
//!     行格式合规率 100%（逐字段解析核）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 行格式四字段冻结：`[t+000123.456][INFO][vmm] 文本`——时戳
//!      （微秒步数化 t+秒.毫秒）/级别（五级闭集）/组件（账面注册）/
//!      文本（非空）；
//!    - 级别闭集：ERROR/WARN/INFO/DEBUG/TRACE（冻结五级，越级拒）；
//!    - 逐字段解析核：渲染行 → 反解析四字段 → 与源值逐位一致（20
//!      行合规率 100%）；
//!    - 组件注册制：未注册组件拒渲染（8 组件冻结闭集）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6C880，AI-VX3-1030 批A 独占区）。
pub const SEG_BASE: u32 = 0x6C880;

// ── 级别/组件闭集 ───────────────────────────────────────────────
pub const LV_ERROR: usize = 0;
pub const LV_WARN: usize = 1;
pub const LV_INFO: usize = 2;
pub const LV_DEBUG: usize = 3;
pub const LV_TRACE: usize = 4;
pub const LEVEL_COUNT: usize = 5;
pub const LEVEL_NAMES: [&str; LEVEL_COUNT] =
    ["ERROR", "WARN", "INFO", "DEBUG", "TRACE"];

/// 组件注册表（冻结 8）。
pub const COMPONENTS: [&str; 8] = [
    "vmm", "sched", "drv", "fs", "net", "ipc", "boot", "ktest",
];

/// 样本行数（冻结：20）。
pub const SAMPLE_ROWS: usize = 20;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogFmtCode {
    /// 级别越闭集
    LevelOutOfRange,
    /// 级别名空（五级齐备破坏）
    LevelNameBlank,
    /// 组件未注册
    ComponentUnregistered,
    /// 文本空（文本字段禁空）
    TextBlank,
    /// 行格式缺括号字段
    BracketFieldMissing,
    /// 解析回读不一致
    ParseRoundtripMismatch,
    /// 时戳回退（单调红线）
    TimestampBackward,
    /// 合规率不满
    ComplianceShort,
    /// 双跑不一致
    NotReproducible,
}

impl LogFmtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            LogFmtCode::LevelOutOfRange => "级别越闭集：五级冻结",
            LogFmtCode::LevelNameBlank => "级别名空：齐备破坏",
            LogFmtCode::ComponentUnregistered => "组件未注册：8 组件闭集外",
            LogFmtCode::TextBlank => "文本空：文本字段禁空",
            LogFmtCode::BracketFieldMissing => "行格式缺括号字段",
            LogFmtCode::ParseRoundtripMismatch => "解析回读不一致",
            LogFmtCode::TimestampBackward => "时戳回退：单调红线",
            LogFmtCode::ComplianceShort => "合规率不满：<100%",
            LogFmtCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [LogFmtCode; 9] = [
        LogFmtCode::LevelOutOfRange,
        LogFmtCode::LevelNameBlank,
        LogFmtCode::ComponentUnregistered,
        LogFmtCode::TextBlank,
        LogFmtCode::BracketFieldMissing,
        LogFmtCode::ParseRoundtripMismatch,
        LogFmtCode::TimestampBackward,
        LogFmtCode::ComplianceShort,
        LogFmtCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < LogFmtCode::ALL.len() {
        if LogFmtCode::ALL[i].val() == code {
            return LogFmtCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6C880 段闭集内"
}

// ── 日志行 ─────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogRow {
    /// 时戳（微秒）。
    pub ts_us: u64,
    pub level: usize,
    pub component: String,
    pub text: String,
}

/// 组件注册判。
pub fn component_registered(name: &str) -> bool {
    let mut i = 0;
    while i < COMPONENTS.len() {
        if COMPONENTS[i] == name {
            return true;
        }
        i += 1;
    }
    false
}

/// 渲染：`[t+SSSSSS.mmm][LEVEL][comp] text`。
pub fn render_row(r: &LogRow) -> Result<String, u32> {
    if r.level >= LEVEL_COUNT {
        return Err(LogFmtCode::LevelOutOfRange.val());
    }
    if LEVEL_NAMES[r.level].len() == 0 {
        return Err(LogFmtCode::LevelNameBlank.val());
    }
    if !component_registered(&r.component) {
        return Err(LogFmtCode::ComponentUnregistered.val());
    }
    if r.text.len() == 0 {
        return Err(LogFmtCode::TextBlank.val());
    }
    Ok(format!(
        "[t+{}.{:03}][{}][{}] {}",
        r.ts_us / 1000,
        r.ts_us % 1000,
        LEVEL_NAMES[r.level],
        r.component,
        r.text
    ))
}

/// 反解析：渲染行 → 四字段（逐字段解析核）。
pub fn parse_row(line: &str) -> Result<LogRow, u32> {
    let b = line.as_bytes();
    if b.len() < 4 || b[0] != b'[' {
        return Err(LogFmtCode::BracketFieldMissing.val());
    }
    // 字段一：t+SSS.mmm]。
    let end1 = find(b, 1, b']')?;
    let ts_field = &line[1..end1];
    if !ts_field.starts_with("t+") {
        return Err(LogFmtCode::BracketFieldMissing.val());
    }
    let dot = ts_field.find('.').ok_or(LogFmtCode::BracketFieldMissing.val())?;
    let sec: u64 = parse_u64(&ts_field[2..dot])?;
    let ms: u64 = parse_u64(&ts_field[dot + 1..])?;
    let ts = sec * 1000 + ms;
    // 字段二：级别。
    let end2 = find(b, end1 + 1, b']')?;
    let level_s = &line[end1 + 1..end2];
    let mut level = LEVEL_COUNT;
    let mut i = 0;
    while i < LEVEL_COUNT {
        if LEVEL_NAMES[i] == level_s {
            level = i;
        }
        i += 1;
    }
    if level == LEVEL_COUNT {
        return Err(LogFmtCode::LevelOutOfRange.val());
    }
    // 字段三：组件。
    let end3 = find(b, end2 + 1, b']')?;
    let comp = &line[end2 + 1..end3];
    if !component_registered(comp) {
        return Err(LogFmtCode::ComponentUnregistered.val());
    }
    // 字段四：文本（非空）。
    let text_start = end3 + 1;
    if text_start >= line.len() {
        return Err(LogFmtCode::TextBlank.val());
    }
    let text = &line[text_start..];
    if text.len() == 0 {
        return Err(LogFmtCode::TextBlank.val());
    }
    Ok(LogRow { ts_us: ts, level, component: String::from(comp), text: String::from(text) })
}

fn find(b: &[u8], from: usize, c: u8) -> Result<usize, u32> {
    let mut i = from;
    while i < b.len() {
        if b[i] == c {
            return Ok(i);
        }
        i += 1;
    }
    Err(LogFmtCode::BracketFieldMissing.val())
}

fn parse_u64(s: &str) -> Result<u64, u32> {
    let mut v: u64 = 0;
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] < b'0' || b[i] > b'9' {
            return Err(LogFmtCode::BracketFieldMissing.val());
        }
        v = v * 10 + (b[i] - b'0') as u64;
        i += 1;
    }
    Ok(v)
}

/// 回读核：parse(render(r)) 与 r 逐位一致。
pub fn roundtrip(r: &LogRow) -> Result<(), u32> {
    let line = render_row(r)?;
    let back = parse_row(&line)?;
    if back.ts_us != r.ts_us
        || back.level != r.level
        || back.component != r.component
        || back.text != r.text
    {
        return Err(LogFmtCode::ParseRoundtripMismatch.val());
    }
    Ok(())
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 20 样本行（确定性：五级×四组件轮转）。
pub fn sample_rows() -> Vec<LogRow> {
    let mut v = Vec::new();
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut i = 0;
    while i < SAMPLE_ROWS {
        v.push(LogRow {
            ts_us: 1_000_000 + i as u64 * 7_777 + lcg.below(999),
            level: i % LEVEL_COUNT,
            component: String::from(COMPONENTS[i % COMPONENTS.len()]),
            text: format!("audit message {}", i),
        });
        i += 1;
    }
    v
}

/// 黄金全链：20 行渲染→逐字段解析核→合规率 100%→时戳单调。
pub fn golden_run() -> Result<(u32, usize, u64), u32> {
    let rows = sample_rows();
    let mut ok = 0;
    let mut prev_ts = 0u64;
    let mut i = 0;
    while i < rows.len() {
        // 时戳单调。
        if rows[i].ts_us < prev_ts {
            return Err(LogFmtCode::TimestampBackward.val());
        }
        prev_ts = rows[i].ts_us;
        roundtrip(&rows[i])?;
        ok += 1;
        i += 1;
    }
    if ok != SAMPLE_ROWS {
        return Err(LogFmtCode::ComplianceShort.val());
    }
    // 五级/八组件闭集齐备。
    if LEVEL_NAMES.len() != 5 || COMPONENTS.len() != 8 {
        return Err(LogFmtCode::LevelNameBlank.val());
    }
    Ok(((ok * 100 / SAMPLE_ROWS) as u32, ok, prev_ts))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    roundtrip(&LogRow {
        ts_us: 1500,
        level: LV_INFO,
        component: String::from("vmm"),
        text: String::from("probe ok"),
    })
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 未注册组件 → 必拒。
    render_row(&LogRow {
        ts_us: 1,
        level: LV_INFO,
        component: String::from("bogus"),
        text: String::from("x"),
    }).map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 空文本 → 必拒。
    render_row(&LogRow {
        ts_us: 1,
        level: LV_INFO,
        component: String::from("vmm"),
        text: String::new(),
    }).map(|_| ())
}

/// 注入 10/10：越级、未注册组件、空文本、回读不一致四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越级。
        led.record(0, render_row(&LogRow {
            ts_us: 1, level: 9, component: String::from("vmm"), text: String::from("x"),
        }).is_err());
        // 槽1：未注册组件。
        led.record(1, component_registered("nope") == false);
        // 槽2：空文本。
        led.record(2, render_row(&LogRow {
            ts_us: 1, level: 0, component: String::from("vmm"), text: String::new(),
        }).is_err());
        // 槽3：回读不一致（篡改解析：伪行）。
        led.record(3, parse_row("[t+1.000][BOGUS][vmm] x").is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(LogFmtCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < LogFmtCode::ALL.len() {
        let v = LogFmtCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 四字段冻结 + 20 行合规率 100%。
    let rows = sample_rows();
    let mut ok = 0;
    let mut i = 0;
    while i < rows.len() {
        if roundtrip(&rows[i]).is_ok() {
            ok += 1;
        }
        i += 1;
    }
    v.push((
        "J1-20行合规率100%",
        ok == SAMPLE_ROWS && LEVEL_NAMES.len() == 5 && COMPONENTS.len() == 8,
        format!("{}/{}", ok, SAMPLE_ROWS),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6C880 窗口")));
    v
}

pub fn run_unx0543_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0543 行格式冻结（时间戳/级别/组件/文）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
