//! AI-20 开发者生态域（F476~F500）。
//!
//! The tooling that keeps the kernel honest: the QEMU test chain and its
//! headless serial assertions, the CI pipeline, the in-kernel debugger with
//! symbols/profiler/flame graph/scheduler and memory tracing, the log viewer,
//! the API stability & versioning promises, the SDK with driver/app samples
//! and tutorial ordering, the contribution/review/changelog discipline, the
//! compatibility matrix, the release fanfare and the ecosystem self-test.

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F476/F477 — QEMU 测试链与 headless 串口断言
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QemuConfig {
    pub machine: &'static str,
    pub cpu: &'static str,
    pub memory_mb: u32,
    pub kernel: &'static str,
    pub headless: bool,
    pub no_reboot: bool,
}

impl Default for QemuConfig {
    fn default() -> QemuConfig {
        QemuConfig {
            machine: "q35",
            cpu: "max",
            memory_mb: 2048,
            kernel: "target/x86_64-unknown-none/release/varix",
            headless: true,
            no_reboot: true,
        }
    }
}

/// Render the QEMU command line into `out`; returns bytes written.
pub fn qemu_command(cfg: QemuConfig, out: &mut [u8]) -> usize {
    let mut n = 0usize;
    let push = |s: &str, out: &mut [u8], n: &mut usize| {
        for &b in s.as_bytes() {
            if *n < out.len() {
                out[*n] = b;
                *n += 1;
            }
        }
    };
    let push_num = |mut v: u32, out: &mut [u8], n: &mut usize| {
        let mut digits = [0u8; 10];
        let mut w = 0usize;
        if v == 0 {
            digits[0] = b'0';
            w = 1;
        }
        while v > 0 && w < digits.len() {
            digits[w] = b'0' + (v % 10) as u8;
            v /= 10;
            w += 1;
        }
        while w > 0 {
            w -= 1;
            if *n < out.len() {
                out[*n] = digits[w];
                *n += 1;
            }
        }
    };
    push("qemu-system-x86_64 -machine ", out, &mut n);
    push(cfg.machine, out, &mut n);
    push(" -cpu ", out, &mut n);
    push(cfg.cpu, out, &mut n);
    push(" -m ", out, &mut n);
    push_num(cfg.memory_mb, out, &mut n);
    push(" -kernel ", out, &mut n);
    push(cfg.kernel, out, &mut n);
    if cfg.headless {
        push(" -display none -serial stdio", out, &mut n);
    }
    if cfg.no_reboot {
        push(" -no-reboot", out, &mut n);
    }
    n
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SerialExpect {
    /// Marker the boot must print, e.g. `"SELF-TEST 12/12 PASS"`.
    pub expect: &'static str,
    pub timeout_ms: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SerialVerdict {
    Found,
    /// Boot finished without the marker.
    Missing,
    /// The timeout elapsed first (boot hang).
    Timeout,
}

/// Match a captured serial log against the expectation.
/// `boot_finished` tells us the log is complete rather than truncated.
pub fn serial_assert(log: &str, expect: SerialExpect, boot_finished: bool) -> SerialVerdict {
    if log.contains(expect.expect) {
        SerialVerdict::Found
    } else if boot_finished {
        SerialVerdict::Missing
    } else {
        SerialVerdict::Timeout
    }
}

// ---------------------------------------------------------------------------
// F478 — CI 流水线
// ---------------------------------------------------------------------------

pub const MAX_STAGES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageKind {
    Build,
    HostTests,
    KernelElf,
    QemuBoot,
    Lint,
    Format,
    Package,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PipelineStage {
    pub kind: StageKind,
    pub required: bool,
    pub passed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PipelineResult {
    pub ran: u8,
    pub failed_required: u8,
    /// Fail-fast: stages after the first required failure never run.
    pub stopped_early: bool,
    pub green: bool,
}

pub fn run_pipeline(stages: &[PipelineStage]) -> PipelineResult {
    let mut ran = 0u8;
    let mut failed = 0u8;
    let mut stopped = false;
    for s in stages.iter().take(MAX_STAGES) {
        ran += 1;
        if !s.passed && s.required {
            failed += 1;
            stopped = true;
            break;
        }
    }
    PipelineResult {
        ran,
        failed_required: failed,
        stopped_early: stopped,
        green: failed == 0,
    }
}

// ---------------------------------------------------------------------------
// F479/F480/F486 — 调试器、符号管理、断点服务
// ---------------------------------------------------------------------------

pub const MAX_BREAKPOINTS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugState {
    Running,
    Paused,
    Halted,
}

#[derive(Clone, Copy, Debug)]
pub struct Debugger {
    pub state: DebugState,
    pub pc: u64,
    breakpoints: [u64; MAX_BREAKPOINTS],
    bp_count: usize,
    pub steps: u32,
}

impl Debugger {
    pub const fn new() -> Debugger {
        Debugger { state: DebugState::Halted, pc: 0, breakpoints: [0; MAX_BREAKPOINTS], bp_count: 0, steps: 0 }
    }

    pub fn add_breakpoint(&mut self, addr: u64) -> bool {
        if self.bp_count >= MAX_BREAKPOINTS || self.breakpoints[..self.bp_count].contains(&addr) {
            return false;
        }
        self.breakpoints[self.bp_count] = addr;
        self.bp_count += 1;
        true
    }

    pub fn remove_breakpoint(&mut self, addr: u64) -> bool {
        for i in 0..self.bp_count {
            if self.breakpoints[i] == addr {
                if i + 1 < self.bp_count {
                    self.breakpoints[i] = self.breakpoints[self.bp_count - 1];
                }
                self.bp_count -= 1;
                return true;
            }
        }
        false
    }

    pub fn breakpoint_count(&self) -> usize {
        self.bp_count
    }

    /// Single-step: always lands paused (a step is not a run).
    pub fn step(&mut self) {
        self.pc = self.pc.wrapping_add(4);
        self.steps += 1;
        self.state = DebugState::Paused;
    }

    /// Run forward until a breakpoint address or `limit` steps.
    /// Returns the address hit, or `None` if the limit was reached.
    pub fn run_until(&mut self, limit: u32) -> Option<u64> {
        self.state = DebugState::Running;
        for _ in 0..limit {
            self.pc = self.pc.wrapping_add(4);
            if self.breakpoints[..self.bp_count].contains(&self.pc) {
                self.state = DebugState::Paused;
                return Some(self.pc);
            }
        }
        self.state = DebugState::Halted;
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub addr: u64,
    pub name: &'static str,
    pub file: &'static str,
    pub line: u32,
}

pub const MAX_SYMBOLS: usize = 24;

/// Sorted-by-address symbol table with binary search (crash address → code).
#[derive(Clone, Copy, Debug)]
pub struct SymbolTable {
    symbols: [Option<Symbol>; MAX_SYMBOLS],
    count: usize,
}

impl SymbolTable {
    pub const fn new() -> SymbolTable {
        SymbolTable { symbols: [None; MAX_SYMBOLS], count: 0 }
    }

    /// Insert keeping the table ordered; rejects duplicates.
    pub fn insert(&mut self, symbol: Symbol) -> bool {
        if self.count >= MAX_SYMBOLS {
            return false;
        }
        for i in 0..self.count {
            if self.symbols[i].map(|s| s.addr == symbol.addr).unwrap_or(false) {
                return false;
            }
        }
        let mut pos = self.count;
        for i in 0..self.count {
            if self.symbols[i].map(|s| s.addr > symbol.addr).unwrap_or(false) {
                pos = i;
                break;
            }
        }
        let mut i = self.count;
        while i > pos {
            self.symbols[i] = self.symbols[i - 1];
            i -= 1;
        }
        self.symbols[pos] = Some(symbol);
        self.count += 1;
        true
    }

    /// Nearest symbol at or below `addr`.
    pub fn resolve(&self, addr: u64) -> Option<Symbol> {
        let mut lo = 0usize;
        let mut hi = self.count;
        let mut best: Option<Symbol> = None;
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.symbols[mid] {
                Some(s) if s.addr <= addr => {
                    best = Some(s);
                    lo = mid + 1;
                }
                _ => hi = mid,
            }
        }
        best
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BreakpointHit {
    pub addr: u64,
    pub hits: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct BreakpointService {
    hits: [Option<BreakpointHit>; MAX_BREAKPOINTS],
    count: usize,
}

impl BreakpointService {
    pub const fn new() -> BreakpointService {
        BreakpointService { hits: [None; MAX_BREAKPOINTS], count: 0 }
    }

    pub fn arm(&mut self, addr: u64) -> bool {
        if self.count >= MAX_BREAKPOINTS
            || (0..self.count).any(|i| self.hits[i].map(|h| h.addr == addr).unwrap_or(false))
        {
            return false;
        }
        self.hits[self.count] = Some(BreakpointHit { addr, hits: 0 });
        self.count += 1;
        true
    }

    pub fn disarm(&mut self, addr: u64) -> bool {
        for i in 0..self.count {
            if self.hits[i].map(|h| h.addr == addr).unwrap_or(false) {
                if i + 1 < self.count {
                    self.hits[i] = self.hits[self.count - 1];
                }
                self.hits[self.count - 1] = None;
                self.count -= 1;
                return true;
            }
        }
        false
    }

    /// Note a trap; returns the running hit count for that address.
    pub fn on_trap(&mut self, addr: u64) -> Option<u32> {
        for i in 0..self.count {
            if let Some(mut h) = self.hits[i] {
                if h.addr == addr {
                    h.hits += 1;
                    self.hits[i] = Some(h);
                    return Some(h.hits);
                }
            }
        }
        None
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// F481/F482/F483/F484/F485 — 剖析器、火焰图、调度/内存追踪、ftrace
// ---------------------------------------------------------------------------

pub const MAX_FUNCS: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct Profiler {
    samples: [u32; MAX_FUNCS],
    names: [Option<&'static str>; MAX_FUNCS],
    count: usize,
    pub total: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotspot {
    pub name: &'static str,
    pub samples: u32,
    /// Share in permille of the total sample count.
    pub permille: u16,
}

impl Profiler {
    pub const fn new() -> Profiler {
        Profiler { samples: [0; MAX_FUNCS], names: [None; MAX_FUNCS], count: 0, total: 0 }
    }

    pub fn register(&mut self, name: &'static str) -> bool {
        if self.count >= MAX_FUNCS {
            return false;
        }
        self.names[self.count] = Some(name);
        self.count += 1;
        true
    }

    pub fn sample(&mut self, name: &str) {
        for i in 0..self.count {
            if self.names[i] == Some(name) {
                self.samples[i] += 1;
                self.total += 1;
                return;
            }
        }
    }

    /// Hottest function, or `None` when nothing was sampled.
    pub fn hottest(&self) -> Option<Hotspot> {
        if self.total == 0 {
            return None;
        }
        let mut best = 0usize;
        for i in 1..self.count {
            if self.samples[i] > self.samples[best] {
                best = i;
            }
        }
        Some(Hotspot {
            name: self.names[best].unwrap_or("?"),
            samples: self.samples[best],
            permille: (self.samples[best] as u64 * 1000 / self.total as u64) as u16,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlameNode {
    /// Folded stack prefix, e.g. "main;sched;pick".
    pub frame: &'static str,
    pub samples: u32,
}

/// Flame graph node from a folded (stack;stack) record — the frame's own
/// weight is its sample count minus its children's, but for rendering we only
/// need the inclusive count.
pub fn folded_record(stack: &str, samples: u32, out: &mut [u8]) -> usize {
    let mut n = 0usize;
    for &b in stack.as_bytes() {
        if n < out.len() {
            out[n] = b;
            n += 1;
        }
    }
    if n < out.len() {
        out[n] = b' ';
        n += 1;
    }
    let mut digits = [0u8; 10];
    let mut w = 0usize;
    let mut v = samples;
    if v == 0 {
        digits[0] = b'0';
        w = 1;
    }
    while v > 0 && w < digits.len() {
        digits[w] = b'0' + (v % 10) as u8;
        v /= 10;
        w += 1;
    }
    while w > 0 {
        w -= 1;
        if n < out.len() {
            out[n] = digits[w];
            n += 1;
        }
    }
    n
}

const SCHED_TRACE_CAP: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SchedEvent {
    pub task: &'static str,
    pub start_us: u64,
    pub end_us: u64,
    pub cpu: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct SchedTrace {
    events: [Option<SchedEvent>; SCHED_TRACE_CAP],
    count: usize,
}

impl SchedTrace {
    pub const fn new() -> SchedTrace {
        SchedTrace { events: [None; SCHED_TRACE_CAP], count: 0 }
    }

    pub fn push(&mut self, event: SchedEvent) -> bool {
        if self.count >= SCHED_TRACE_CAP {
            return false;
        }
        self.events[self.count] = Some(event);
        self.count += 1;
        true
    }

    /// Total runtime attributed to `task`.
    pub fn runtime_us(&self, task: &str) -> u64 {
        (0..self.count)
            .filter_map(|i| self.events[i])
            .filter(|e| e.task == task)
            .map(|e| e.end_us.saturating_sub(e.start_us))
            .sum()
    }

    /// Switch counts per CPU — a load-balance sanity check.
    pub fn switches_on(&self, cpu: u8) -> usize {
        (0..self.count)
            .filter_map(|i| self.events[i])
            .filter(|e| e.cpu == cpu)
            .count()
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

const MEM_TRACE_CAP: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocRecord {
    pub site: &'static str,
    pub size: u32,
    /// Positive = allocation, negative = free.
    pub delta: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct AllocTrace {
    records: [Option<AllocRecord>; MEM_TRACE_CAP],
    count: usize,
    pub live_bytes: i64,
    pub peak_bytes: i64,
}

impl AllocTrace {
    pub const fn new() -> AllocTrace {
        AllocTrace {
            records: [None; MEM_TRACE_CAP],
            count: 0,
            live_bytes: 0,
            peak_bytes: 0,
        }
    }

    /// `delta` signs the size: +1 allocation, -1 free.
    pub fn record(&mut self, rec: AllocRecord) -> bool {
        if self.count < MEM_TRACE_CAP {
            self.records[self.count] = Some(rec);
            self.count += 1;
        }
        let signed = if rec.delta < 0 {
            -(rec.size as i64)
        } else {
            rec.size as i64
        };
        self.live_bytes += signed;
        if self.live_bytes > self.peak_bytes {
            self.peak_bytes = self.live_bytes;
        }
        true
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

/// F485: ftrace-style event stream with per-event enable flags and a ring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceEvent {
    Syscall,
    Irq,
    Sched,
    Block,
    Net,
}

pub const MAX_TRACE_EVENTS: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct EventTracer {
    enabled: u8,
    events: [Option<(TraceEvent, u64)>; MAX_TRACE_EVENTS],
    head: usize,
    count: usize,
    dropped: u32,
}

impl EventTracer {
    pub const fn new() -> EventTracer {
        EventTracer {
            enabled: 0,
            events: [None; MAX_TRACE_EVENTS],
            head: 0,
            count: 0,
            dropped: 0,
        }
    }

    pub fn enable(&mut self, event: TraceEvent, on: bool) {
        let bit = 1u8 << (event as u8);
        if on {
            self.enabled |= bit;
        } else {
            self.enabled &= !bit;
        }
    }

    pub fn is_enabled(&self, event: TraceEvent) -> bool {
        self.enabled & (1u8 << (event as u8)) != 0
    }

    /// Disabled events are skipped before they reach the ring — tracing must
    /// cost nothing when it is off.
    pub fn emit(&mut self, event: TraceEvent, stamp: u64) -> bool {
        if !self.is_enabled(event) {
            return false;
        }
        if self.count == MAX_TRACE_EVENTS {
            self.dropped += 1;
            self.head = (self.head + 1) % MAX_TRACE_EVENTS;
            self.count -= 1;
        }
        self.events[self.head] = Some((event, stamp));
        self.head = (self.head + 1) % MAX_TRACE_EVENTS;
        self.count += 1;
        true
    }

    /// Newest-first access.
    pub fn get(&self, index: usize) -> Option<(TraceEvent, u64)> {
        if index >= self.count {
            return None;
        }
        let pos = (self.head + MAX_TRACE_EVENTS - 1 - index) % MAX_TRACE_EVENTS;
        self.events[pos]
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn dropped(&self) -> u32 {
        self.dropped
    }
}

// ---------------------------------------------------------------------------
// F487 — 日志查看器
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogLine {
    pub level: u8,
    pub module: &'static str,
    pub text: &'static str,
}

/// Viewer filter: level floor + optional module + optional substring.
pub fn log_matches(line: LogLine, min_level: u8, module: Option<&str>, needle: &str) -> bool {
    if line.level < min_level {
        return false;
    }
    if let Some(m) = module {
        if line.module != m {
            return false;
        }
    }
    needle.is_empty() || line.text.contains(needle)
}

pub fn log_filter_count(lines: &[LogLine], min_level: u8, module: Option<&str>, needle: &str) -> usize {
    lines
        .iter()
        .filter(|l| log_matches(**l, min_level, module, needle))
        .count()
}

// ---------------------------------------------------------------------------
// F488 — 文档站
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocPage {
    pub slug: &'static str,
    pub title: &'static str,
    pub order: u8,
}

/// Prev/next navigation for the doc site (ordered by `order`).
pub fn doc_neighbours(pages: &[DocPage], slug: &str) -> Option<(Option<&'static str>, Option<&'static str>)> {
    let mut idx: Option<usize> = None;
    for (i, p) in pages.iter().enumerate() {
        if p.slug == slug {
            idx = Some(i);
            break;
        }
    }
    let i = idx?;
    let prev = if i > 0 { Some(pages[i - 1].slug) } else { None };
    let next = pages.get(i + 1).map(|p| p.slug);
    Some((prev, next))
}

// ---------------------------------------------------------------------------
// F489/F490 — API 稳定承诺与版本化接口
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stability {
    /// Frozen: breaking it requires a major version.
    Stable,
    Experimental,
    Deprecated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApiContract {
    pub name: &'static str,
    pub since: u16,
    pub stability: Stability,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiVerdict {
    Ok,
    /// A stable API was changed — refuse the release.
    BreakingStable(&'static str),
    Deprecated(&'static str),
}

pub fn check_api_change(contract: ApiContract, signature_changed: bool) -> ApiVerdict {
    match contract.stability {
        Stability::Stable if signature_changed => ApiVerdict::BreakingStable(contract.name),
        Stability::Deprecated => ApiVerdict::Deprecated(contract.name),
        _ => ApiVerdict::Ok,
    }
}

/// Syscall ABI negotiation between a program and the kernel (F490).
pub fn syscall_abi(kernel_min: u16, kernel_max: u16, program_want: u16) -> Option<u16> {
    if program_want >= kernel_min && program_want <= kernel_max {
        Some(program_want)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// F491/F492/F493/F494 — SDK、示例驱动、示例应用、教程体系
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SdkPackage {
    pub headers: u8,
    pub libs: u8,
    pub examples: u8,
    pub abi: u16,
    pub commit: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SdkVerdict {
    Complete,
    MissingHeaders,
    MissingLibs,
    MissingExamples,
    Unversioned,
}

pub fn sdk_verdict(pkg: SdkPackage) -> SdkVerdict {
    if pkg.abi == 0 || pkg.commit.is_empty() {
        return SdkVerdict::Unversioned;
    }
    if pkg.headers == 0 {
        return SdkVerdict::MissingHeaders;
    }
    if pkg.libs == 0 {
        return SdkVerdict::MissingLibs;
    }
    if pkg.examples == 0 {
        return SdkVerdict::MissingExamples;
    }
    SdkVerdict::Complete
}

/// Example-driver template lint: the three entry points from the SDK contract.
pub const DRIVER_TEMPLATE_ENTRIES: [&'static str; 3] = ["probe", "start", "stop"];

pub fn template_lint(source: &str, required: &[&'static str]) -> usize {
    required.iter().filter(|r| !source.contains(**r)).count()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TutorialChapter {
    pub id: &'static str,
    /// Chapters that must be read first (indices into the chapter list).
    pub prerequisites: [usize; 3],
    pub prereq_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TutorialError {
    MissingPrerequisite(&'static str),
    Cycle,
}

/// Tutorial order must be a valid topological order of the prerequisites.
pub fn tutorial_order(chapters: &[TutorialChapter], out: &mut [usize]) -> Result<usize, TutorialError> {
    let n = chapters.len();
    if n > out.len() {
        return Err(TutorialError::Cycle);
    }
    for (i, c) in chapters.iter().enumerate() {
        for p in 0..c.prereq_count {
            if c.prerequisites[p] >= n {
                return Err(TutorialError::MissingPrerequisite(c.id));
            }
            let _ = i;
        }
    }
    let mut emitted = [false; MAX_FUNCS];
    let mut count = 0usize;
    while count < n {
        let mut progressed = false;
        for i in 0..n {
            if emitted[i] {
                continue;
            }
            let ready = (0..chapters[i].prereq_count).all(|k| emitted[chapters[i].prerequisites[k]]);
            if ready {
                emitted[i] = true;
                out[count] = i;
                count += 1;
                progressed = true;
            }
        }
        if !progressed {
            return Err(TutorialError::Cycle);
        }
    }
    Ok(count)
}

// ---------------------------------------------------------------------------
// F495/F496/F497 — 贡献指南、审查清单、变更日志纪律
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitError {
    BadPrefix,
    MissingFeatureId,
    EmptySubject,
    TooLong,
}

/// The house commit format: `varix500(ai-XX): Fxxx <功能名>`.
pub fn lint_commit(message: &str) -> Result<(), CommitError> {
    let line = message.lines().next().unwrap_or("");
    if line.len() > 100 {
        return Err(CommitError::TooLong);
    }
    if !line.starts_with("varix500(") {
        return Err(CommitError::BadPrefix);
    }
    let close = match line.find("): ") {
        Some(i) => i,
        None => return Err(CommitError::BadPrefix),
    };
    let subject = &line[close + 3..];
    if subject.trim().is_empty() {
        return Err(CommitError::EmptySubject);
    }
    // Feature id F001..F500 must appear in the subject.
    let id_ok = subject
        .as_bytes()
        .windows(4)
        .any(|w| w[0] == b'F' && w[1..].iter().all(|c| c.is_ascii_digit()));
    if !id_ok {
        return Err(CommitError::MissingFeatureId);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChangeStats {
    pub files: u16,
    pub added: u32,
    pub removed: u32,
    pub tests_added: u16,
    pub docs_touched: bool,
}

pub const REVIEW_CHECKLIST: [&'static str; 6] = [
    "no new warnings",
    "has reproducible acceptance",
    "no cross-domain edits",
    "shared files HEAD verified",
    "tests cover the new path",
    "rollback path exists",
];

/// A change fails review when it adds code without tests, or removes more
/// than it adds in a large change (a rewrite disguised as a fix).
pub fn review_verdict(stats: ChangeStats) -> (u8, bool) {
    let mut passed = 0u8;
    if stats.tests_added > 0 {
        passed += 1;
    }
    if stats.added >= stats.removed {
        passed += 1;
    }
    if stats.files <= 20 {
        passed += 1;
    }
    if stats.docs_touched || stats.added < 200 || stats.tests_added > 0 {
        passed += 1;
    }
    if stats.removed == 0 || stats.tests_added > 0 {
        passed += 1;
    }
    if stats.files > 0 {
        passed += 1;
    }
    (passed, passed == REVIEW_CHECKLIST.len() as u8)
}

/// F497: every entry must reference a feature id and a status marker.
pub fn changelog_lint(entry: &str) -> bool {
    let has_id = entry
        .as_bytes()
        .windows(4)
        .any(|w| w[0] == b'F' && w[1..].iter().all(|c| c.is_ascii_digit()));
    let has_marker = entry.contains('✅') || entry.contains('🔶') || entry.contains('⬜');
    has_id && has_marker && !entry.trim().is_empty()
}

// ---------------------------------------------------------------------------
// F498 — 兼容性测试矩阵
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompatMatrix {
    pub machines: u8,
    pub features: u16,
    /// Bit per machine holding the highest verified feature index.
    pub verified: [u16; 8],
}

impl CompatMatrix {
    pub const fn new(machines: u8, features: u16) -> CompatMatrix {
        CompatMatrix { machines, features, verified: [0; 8] }
    }

    pub fn mark(&mut self, machine: usize, feature_count: u16) -> bool {
        if machine >= self.machines as usize || machine >= self.verified.len() {
            return false;
        }
        self.verified[machine] = self.verified[machine].max(feature_count.min(self.features));
        true
    }

    /// Coverage in permille over the full matrix.
    pub fn coverage_permille(&self) -> u16 {
        if self.machines == 0 || self.features == 0 {
            return 0;
        }
        let total: u32 = (0..self.machines as usize)
            .map(|i| self.verified[i] as u32)
            .sum();
        let capacity = self.machines as u32 * self.features as u32;
        ((total as u64 * 1000) / capacity as u64) as u16
    }

    /// Machines that have not verified a given feature.
    pub fn gaps(&self, feature_index: u16) -> u8 {
        (0..self.machines as usize)
            .filter(|i| self.verified[*i] <= feature_index)
            .count() as u8
    }
}

// ---------------------------------------------------------------------------
// F499 — 发版礼炮
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: &'static str,
    pub feature_count: u16,
    pub notes_len: usize,
    pub easter_egg: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseVerdict {
    Ready,
    MissingNotes,
    NotAllFeatures,
    BadVersion,
}

pub fn release_verdict(release: Release, expected_features: u16) -> ReleaseVerdict {
    if release.version.is_empty() || !release.version.contains('.') {
        return ReleaseVerdict::BadVersion;
    }
    if release.feature_count < expected_features {
        return ReleaseVerdict::NotAllFeatures;
    }
    if release.notes_len == 0 {
        return ReleaseVerdict::MissingNotes;
    }
    ReleaseVerdict::Ready
}

// ---------------------------------------------------------------------------
// F500 — 生态自检
// ---------------------------------------------------------------------------

pub fn run_deveco_checks() -> CheckSet {
    let mut set = CheckSet::new("deveco");

    let cfg = QemuConfig::default();
    let mut cmd = [0u8; 256];
    let n = qemu_command(cfg, &mut cmd);
    let text = core::str::from_utf8(&cmd[..n]).unwrap_or("");
    set.add(
        "F476 qemu test chain",
        n > 0
            && text.starts_with("qemu-system-x86_64")
            && text.contains("-machine q35")
            && text.contains("-m 2048")
            && text.contains("-serial stdio")
            && text.contains("-no-reboot"),
        "command line",
    );
    set.add(
        "F476 truncated buffer",
        qemu_command(cfg, &mut [0u8; 8]) == 8,
        "bounded",
    );

    set.add(
        "F477 serial assertions",
        serial_assert("boot ok SELF-TEST 12/12 PASS", SerialExpect {
            expect: "SELF-TEST 12/12 PASS",
            timeout_ms: 10_000,
        }, true) == SerialVerdict::Found
            && serial_assert("boot ok", SerialExpect { expect: "MISS", timeout_ms: 1 }, true)
                == SerialVerdict::Missing
            && serial_assert("partial", SerialExpect { expect: "MISS", timeout_ms: 1 }, false)
                == SerialVerdict::Timeout,
        "headless verdicts",
    );

    let stages = [
        PipelineStage { kind: StageKind::Build, required: true, passed: true },
        PipelineStage { kind: StageKind::HostTests, required: true, passed: true },
        PipelineStage { kind: StageKind::KernelElf, required: true, passed: true },
        PipelineStage { kind: StageKind::QemuBoot, required: true, passed: true },
    ];
    let green = run_pipeline(&stages);
    let red = run_pipeline(&[
        PipelineStage { kind: StageKind::Build, required: true, passed: true },
        PipelineStage { kind: StageKind::HostTests, required: true, passed: false },
        PipelineStage { kind: StageKind::Lint, required: false, passed: true },
    ]);
    set.add(
        "F478 ci pipeline",
        green.green && green.ran == 4 && !green.stopped_early
            && !red.green && red.failed_required == 1 && red.stopped_early && red.ran == 2,
        "fail fast",
    );

    let mut dbg = Debugger::new();
    dbg.add_breakpoint(0x1000);
    dbg.pc = 0xFF0;
    let hit = dbg.run_until(16);
    let stepped = {
        dbg.step();
        dbg.steps
    };
    set.add(
        "F479/F486 debugger",
        dbg.breakpoint_count() == 1
            && hit == Some(0x1000)
            && dbg.state == DebugState::Paused
            && stepped == 1
            &&!dbg.add_breakpoint(0x1000)
            && dbg.remove_breakpoint(0x1000)
            && dbg.breakpoint_count() == 0,
        "breakpoints",
    );
    let mut missed = Debugger::new();
    missed.pc = 0;
    set.add("F479 run limit", missed.run_until(2).is_none() && missed.state == DebugState::Halted, "halt");

    let mut svc = BreakpointService::new();
    let armed = svc.arm(0x2000);
    let dup = svc.arm(0x2000);
    let first_hit = svc.on_trap(0x2000);
    let second_hit = svc.on_trap(0x2000);
    set.add(
        "F486 breakpoint service",
        armed
            && !dup
            && first_hit == Some(1)
            && second_hit == Some(2)
            && svc.disarm(0x2000)
            && svc.len() == 0
            && svc.on_trap(0x2000).is_none(),
        "hit counting",
    );

    let mut syms = SymbolTable::new();
    syms.insert(Symbol { addr: 0x2000, name: "main", file: "main.rs", line: 10 });
    syms.insert(Symbol { addr: 0x1000, name: "boot", file: "boot.rs", line: 5 });
    syms.insert(Symbol { addr: 0x3000, name: "sched", file: "sched.rs", line: 20 });
    set.add(
        "F480 symbol management",
        syms.len() == 3
            && syms.resolve(0x1800).map(|s| s.name) == Some("boot")
            && syms.resolve(0x2FFF).map(|s| s.name) == Some("main")
            && syms.resolve(0xFFFF).map(|s| s.name) == Some("sched")
            && syms.resolve(0x10).is_none()
            && !syms.insert(Symbol { addr: 0x2000, name: "dup", file: "x", line: 0 }),
        "sorted resolve",
    );

    let mut prof = Profiler::new();
    prof.register("pick");
    prof.register("switch");
    for _ in 0..8 {
        prof.sample("pick");
    }
    for _ in 0..2 {
        prof.sample("switch");
    }
    prof.sample("unknown");
    set.add(
        "F481 profiler",
        prof.total == 10
            && prof.hottest().map(|h| (h.name, h.samples, h.permille)) == Some(("pick", 8, 800)),
        "hotspot",
    );
    set.add("F481 empty profiler", Profiler::new().hottest().is_none(), "no samples");

    let mut folded = [0u8; 64];
    let fn_bytes = folded_record("main;sched;pick", 42, &mut folded);
    set.add(
        "F482 flame graph",
        fn_bytes > 0
            && core::str::from_utf8(&folded[..fn_bytes]).unwrap_or("") == "main;sched;pick 42",
        "folded record",
    );

    let mut trace = SchedTrace::new();
    let _ = trace.push(SchedEvent { task: "idle", start_us: 0, end_us: 1000, cpu: 0 });
    let _ = trace.push(SchedEvent { task: "ui", start_us: 1000, end_us: 1500, cpu: 0 });
    let _ = trace.push(SchedEvent { task: "ui", start_us: 1600, end_us: 2000, cpu: 1 });
    set.add(
        "F483 scheduler tracer",
        trace.runtime_us("ui") == 900
            && trace.switches_on(0) == 2
            && trace.switches_on(1) == 1
            && trace.runtime_us("nope") == 0,
        "runtime attribution",
    );

    let mut alloc = AllocTrace::new();
    alloc.record(AllocRecord { site: "dma", size: 4096, delta: 1 });
    alloc.record(AllocRecord { site: "dma", size: 1024, delta: -1 });
    set.add(
        "F484 memory tracer",
        alloc.len() == 2 && alloc.live_bytes > 0 && alloc.peak_bytes >= alloc.live_bytes,
        "live bytes",
    );

    let mut tracer = EventTracer::new();
    let off = tracer.emit(TraceEvent::Syscall, 1);
    tracer.enable(TraceEvent::Syscall, true);
    let on = tracer.emit(TraceEvent::Syscall, 2);
    tracer.enable(TraceEvent::Syscall, false);
    let off_again = tracer.emit(TraceEvent::Syscall, 3);
    set.add(
        "F485 ftrace events",
        !off
            && on
            && !off_again
            && tracer.len() == 1
            && tracer.get(0) == Some((TraceEvent::Syscall, 2))
            && !tracer.is_enabled(TraceEvent::Irq),
        "event gate",
    );
    let mut storm = EventTracer::new();
    storm.enable(TraceEvent::Sched, true);
    for i in 0..MAX_TRACE_EVENTS + 3 {
        storm.emit(TraceEvent::Sched, i as u64);
    }
    set.add(
        "F485 tracer bounds",
        storm.len() == MAX_TRACE_EVENTS && storm.dropped() > 0,
        "ring bound",
    );

    let lines = [
        LogLine { level: 3, module: "net", text: "link up" },
        LogLine { level: 1, module: "net", text: "rx pause" },
        LogLine { level: 3, module: "fs", text: "mount failed" },
    ];
    set.add(
        "F487 log viewer",
        log_filter_count(&lines, 2, None, "") == 2
            && log_filter_count(&lines, 0, Some("net"), "") == 2
            && log_filter_count(&lines, 0, None, "failed") == 1
            && log_filter_count(&lines, 4, None, "") == 0,
        "filter",
    );

    let pages = [
        DocPage { slug: "intro", title: "Intro", order: 1 },
        DocPage { slug: "boot", title: "Boot", order: 2 },
        DocPage { slug: "api", title: "API", order: 3 },
    ];
    set.add(
        "F488 doc site",
        doc_neighbours(&pages, "boot") == Some((Some("intro"), Some("api")))
            && doc_neighbours(&pages, "intro") == Some((None, Some("boot")))
            && doc_neighbours(&pages, "api") == Some((Some("boot"), None))
            && doc_neighbours(&pages, "nope").is_none(),
        "navigation",
    );

    set.add(
        "F489/F490 api stability",
        check_api_change(ApiContract { name: "sys_read", since: 1, stability: Stability::Stable }, false)
            == ApiVerdict::Ok
            && check_api_change(ApiContract { name: "sys_read", since: 1, stability: Stability::Stable }, true)
                == ApiVerdict::BreakingStable("sys_read")
            && check_api_change(ApiContract { name: "old", since: 1, stability: Stability::Deprecated }, false)
                == ApiVerdict::Deprecated("old")
            && syscall_abi(2, 4, 3) == Some(3)
            && syscall_abi(2, 4, 5).is_none(),
        "versioning",
    );

    let sdk = SdkPackage { headers: 12, libs: 3, examples: 6, abi: 2, commit: "abc1234" };
    set.add(
        "F491 sdk",
        sdk_verdict(sdk) == SdkVerdict::Complete
            && sdk_verdict(SdkPackage { headers: 0, ..sdk }) == SdkVerdict::MissingHeaders
            && sdk_verdict(SdkPackage { libs: 0, ..sdk }) == SdkVerdict::MissingLibs
            && sdk_verdict(SdkPackage { examples: 0, ..sdk }) == SdkVerdict::MissingExamples
            && sdk_verdict(SdkPackage { abi: 0, ..sdk }) == SdkVerdict::Unversioned,
        "completeness",
    );

    let driver_src = "fn probe() {}\nfn start() {}\nfn stop() {}\n";
    let app_src = "fn main() {}\n";
    set.add(
        "F492/F493 examples",
        template_lint(driver_src, &DRIVER_TEMPLATE_ENTRIES) == 0
            && template_lint("fn probe() {}", &DRIVER_TEMPLATE_ENTRIES) == 2
            && template_lint(app_src, &["main"]) == 0
            && template_lint(app_src, &["main", "argv"]) == 1,
        "template lint",
    );

    let chapters = [
        TutorialChapter { id: "intro", prerequisites: [0; 3], prereq_count: 0 },
        TutorialChapter { id: "first-driver", prerequisites: [0, 0, 0], prereq_count: 1 },
        TutorialChapter { id: "dma", prerequisites: [1, 0, 0], prereq_count: 1 },
    ];
    let mut order = [0usize; MAX_FUNCS];
    let ordered = tutorial_order(&chapters, &mut order);
    let bad = tutorial_order(
        &[TutorialChapter { id: "x", prerequisites: [9, 0, 0], prereq_count: 1 }],
        &mut order,
    );
    set.add(
        "F494 tutorial system",
        ordered == Ok(3)
            && order[0] == 0
            && order[2] == 2
            && bad == Err(TutorialError::MissingPrerequisite("x")),
        "prerequisites",
    );

    set.add(
        "F495 contribution guide",
        lint_commit("varix500(ai-19): F451 ISR 竞态审计").is_ok()
            && lint_commit("varix500(ai-19): improve things") == Err(CommitError::MissingFeatureId)
            && lint_commit("feat: something") == Err(CommitError::BadPrefix)
            && lint_commit("varix500(ai-01): ") == Err(CommitError::EmptySubject),
        "commit format",
    );

    let good = ChangeStats { files: 3, added: 400, removed: 10, tests_added: 12, docs_touched: false };
    let bad_stats = ChangeStats { files: 40, added: 10, removed: 500, tests_added: 0, docs_touched: false };
    set.add(
        "F496 review checklist",
        review_verdict(good).1
            && !review_verdict(bad_stats).1
            && review_verdict(bad_stats).0 < REVIEW_CHECKLIST.len() as u8,
        "review gate",
    );

    set.add(
        "F497 changelog discipline",
        changelog_lint("✅ F451 ISR 竞态审计")
            && !changelog_lint("F451 ISR 竞态审计")
            && !changelog_lint("✅ no id")
            && changelog_lint("⬜ F500 生态自检"),
        "every entry traced",
    );

    let mut matrix = CompatMatrix::new(4, 500);
    matrix.mark(0, 500);
    matrix.mark(1, 400);
    matrix.mark(2, 500);
    matrix.mark(3, 100);
    set.add(
        "F498 compatibility matrix",
        matrix.coverage_permille() == 750
            && matrix.gaps(499) == 2
            && matrix.gaps(99) == 0
            && !matrix.mark(9, 1)
            && CompatMatrix::new(0, 1).coverage_permille() == 0,
        "coverage",
    );

    let release = Release { version: "0.1.0", feature_count: 500, notes_len: 400, easter_egg: true };
    set.add(
        "F499 release fanfare",
        release_verdict(release, 500) == ReleaseVerdict::Ready
            && release.easter_egg
            && release_verdict(Release { version: "0.1", ..release }, 500) == ReleaseVerdict::Ready
            && release_verdict(Release { version: "nope", ..release }, 500) == ReleaseVerdict::BadVersion
            && release_verdict(Release { feature_count: 499, ..release }, 500)
                == ReleaseVerdict::NotAllFeatures
            && release_verdict(Release { notes_len: 0, ..release }, 500)
                == ReleaseVerdict::MissingNotes,
        "release gate",
    );

    set
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f476_config_variants() {
        let mut out = [0u8; 256];
        let n = qemu_command(
            QemuConfig { headless: false, no_reboot: false, memory_mb: 512, ..QemuConfig::default() },
            &mut out,
        );
        let text = core::str::from_utf8(&out[..n]).unwrap();
        assert!(!text.contains("-serial"));
        assert!(!text.contains("-no-reboot"));
        assert!(text.contains("-m 512"));
    }

    #[test]
    fn f478_optional_failure_does_not_stop() {
        let r = run_pipeline(&[
            PipelineStage { kind: StageKind::Build, required: true, passed: true },
            PipelineStage { kind: StageKind::Lint, required: false, passed: false },
            PipelineStage { kind: StageKind::Package, required: true, passed: true },
        ]);
        assert!(r.green);
        assert_eq!(r.ran, 3);
        assert!(!r.stopped_early);
    }

    #[test]
    fn f479_breakpoint_capacity() {
        let mut d = Debugger::new();
        for i in 0..MAX_BREAKPOINTS {
            assert!(d.add_breakpoint(0x100 * (i as u64 + 1)));
        }
        assert!(!d.add_breakpoint(0x999));
        assert!(d.remove_breakpoint(0x100));
        assert!(!d.remove_breakpoint(0x100));
    }

    #[test]
    fn f480_symbols_stay_ordered() {
        let mut t = SymbolTable::new();
        for i in 0..MAX_SYMBOLS {
            assert!(t.insert(Symbol {
                addr: (MAX_SYMBOLS - i) as u64 * 0x100,
                name: "s",
                file: "f",
                line: i as u32
            }));
        }
        assert!(!t.insert(Symbol { addr: 0x100, name: "dup", file: "f", line: 0 }));
        let resolved = t.resolve(u64::MAX).expect("resolve");
        assert!(resolved.addr >= 0x100);
    }

    #[test]
    fn f487_log_filter_module_and_level() {
        let line = LogLine { level: 2, module: "usb", text: "enumerated" };
        assert!(log_matches(line, 2, None, ""));
        assert!(!log_matches(line, 3, None, ""));
        assert!(log_matches(line, 0, Some("usb"), "enum"));
        assert!(!log_matches(line, 0, Some("net"), ""));
        assert!(!log_matches(line, 0, None, "nope"));
    }

    #[test]
    fn f488_neighbours_edges() {
        let pages = [DocPage { slug: "a", title: "A", order: 1 }];
        assert_eq!(doc_neighbours(&pages, "a"), Some((None, None)));
        assert!(doc_neighbours(&[], "a").is_none());
    }

    #[test]
    fn f495_commit_length_limit() {
        let long = format!("varix500(ai-01): F001 {}", "x".repeat(120));
        assert_eq!(lint_commit(&long), Err(CommitError::TooLong));
    }

    #[test]
    fn f497_markers_accepted() {
        assert!(changelog_lint("🔶 F100 x"));
        assert!(!changelog_lint(""));
        assert!(!changelog_lint("✅ done"));
    }

    #[test]
    fn f498_matrix_gaps_and_marks() {
        let mut m = CompatMatrix::new(2, 10);
        assert_eq!(m.coverage_permille(), 0);
        assert_eq!(m.gaps(0), 2);
        assert!(m.mark(0, 10));
        assert_eq!(m.gaps(0), 1);
        assert_eq!(m.gaps(9), 1);
        assert!(!m.mark(5, 1));
        assert_eq!(m.coverage_permille(), 500);
    }

    #[test]
    fn f499_release_order_of_checks() {
        let r = Release { version: "", feature_count: 0, notes_len: 0, easter_egg: false };
        assert_eq!(release_verdict(r, 500), ReleaseVerdict::BadVersion);
        let no_notes = Release { version: "1.0", feature_count: 1, notes_len: 0, easter_egg: false };
        assert_eq!(release_verdict(no_notes, 1), ReleaseVerdict::MissingNotes);
    }

    #[test]
    fn f500_self_test_passes() {
        let set = run_deveco_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("deveco self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 24);
    }
}
