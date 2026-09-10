//! AI-15 系统服务域（F351~F375）。
//!
//! IPC + message bus, the name/time/timer/log/config infrastructure, the
//! package manager with A/B updates and rollback, the user-facing services
//! (notifications, clipboard, drag&drop, theme, fonts, help, OOBE, permission
//! prompts), the event bus, service health/dependency/degradation and the
//! service self-test.
//!
//! Every service is a fixed-capacity object: services must survive without
//! the allocator (a service that cannot start because memory is tight is a
//! boot failure, not a degraded experience).

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F351 — IPC 框架
// ---------------------------------------------------------------------------

/// Capability bits — an IPC send needs the target's `receive` bit.
pub const CAP_SEND: u32 = 1 << 0;
pub const CAP_RECEIVE: u32 = 1 << 1;
pub const CAP_ADMIN: u32 = 1 << 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpcKind {
    Call,
    Reply,
    Event,
    Stream,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IpcEnvelope {
    pub from: u32,
    pub to: &'static str,
    pub kind: IpcKind,
    pub payload_len: usize,
}

/// Capability-gated send: the sender must hold every bit the target requires.
pub fn ipc_allowed(sender_caps: u32, required: u32) -> bool {
    sender_caps & required == required
}

pub const IPC_MSG_MAX: usize = 4096;

pub fn ipc_size_ok(envelope: IpcEnvelope) -> bool {
    envelope.payload_len <= IPC_MSG_MAX
}

// ---------------------------------------------------------------------------
// F352 — 消息总线
// ---------------------------------------------------------------------------

pub const MAX_SUBSCRIBERS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Topic {
    System,
    Power,
    Display,
    Audio,
    Network,
    Storage,
    Input,
}

#[derive(Clone, Copy, Debug)]
pub struct BusSubscription {
    pub topic: Topic,
    pub name: &'static str,
    pub pid: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct MessageBus {
    subs: [Option<BusSubscription>; MAX_SUBSCRIBERS],
    count: usize,
    /// Total messages delivered — the health monitor (F372) watches this.
    pub delivered: u32,
    pub dropped: u32,
}

impl MessageBus {
    pub const fn new() -> MessageBus {
        MessageBus { subs: [None; MAX_SUBSCRIBERS], count: 0, delivered: 0, dropped: 0 }
    }

    pub fn subscribe(&mut self, sub: BusSubscription) -> bool {
        if self.count >= MAX_SUBSCRIBERS {
            self.dropped += 1;
            return false;
        }
        self.subs[self.count] = Some(sub);
        self.count += 1;
        true
    }

    pub fn unsubscribe(&mut self, pid: u32) -> usize {
        let mut removed = 0usize;
        let mut write = 0usize;
        for read in 0..self.count {
            if let Some(s) = self.subs[read] {
                if s.pid == pid {
                    removed += 1;
                    continue;
                }
                self.subs[write] = Some(s);
                write += 1;
            }
        }
        for i in write..self.count {
            self.subs[i] = None;
        }
        self.count = write;
        removed
    }

    /// Publish to a topic; returns how many subscribers were notified.
    pub fn publish(&mut self, topic: Topic) -> usize {
        let mut n = 0usize;
        for i in 0..self.count {
            if let Some(s) = self.subs[i] {
                if s.topic == topic {
                    n += 1;
                }
            }
        }
        self.delivered += n as u32;
        n
    }

    pub fn subscriber_count(&self, topic: Topic) -> usize {
        (0..self.count)
            .filter(|i| self.subs[*i].map(|s| s.topic == topic).unwrap_or(false))
            .count()
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// F353 — 名字注册服务
// ---------------------------------------------------------------------------

pub const MAX_NAMES: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NameEntry {
    pub name: &'static str,
    pub pid: u32,
    pub alive: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterOutcome {
    Registered,
    Taken,
    Updated,
}

#[derive(Clone, Copy, Debug)]
pub struct NameRegistry {
    entries: [Option<NameEntry>; MAX_NAMES],
    count: usize,
}

impl NameRegistry {
    pub const fn new() -> NameRegistry {
        NameRegistry { entries: [None; MAX_NAMES], count: 0 }
    }

    /// Register a well-known name. A dead owner is replaced (crash restart);
    /// a live owner blocks the name.
    pub fn register(&mut self, name: &'static str, pid: u32) -> RegisterOutcome {
        for i in 0..self.count {
            if let Some(mut e) = self.entries[i] {
                if e.name == name {
                    if e.alive {
                        return RegisterOutcome::Taken;
                    }
                    e.pid = pid;
                    e.alive = true;
                    self.entries[i] = Some(e);
                    return RegisterOutcome::Updated;
                }
            }
        }
        if self.count >= MAX_NAMES {
            return RegisterOutcome::Taken;
        }
        self.entries[self.count] = Some(NameEntry { name, pid, alive: true });
        self.count += 1;
        RegisterOutcome::Registered
    }

    pub fn lookup(&self, name: &str) -> Option<u32> {
        for i in 0..self.count {
            if let Some(e) = self.entries[i] {
                if e.name == name && e.alive {
                    return Some(e.pid);
                }
            }
        }
        None
    }

    pub fn mark_dead(&mut self, pid: u32) -> usize {
        let mut n = 0usize;
        for i in 0..self.count {
            if let Some(mut e) = self.entries[i] {
                if e.pid == pid && e.alive {
                    e.alive = false;
                    self.entries[i] = Some(e);
                    n += 1;
                }
            }
        }
        n
    }

    pub fn remove(&mut self, name: &str) -> bool {
        for i in 0..self.count {
            if self.entries[i].map(|e| e.name == name).unwrap_or(false) {
                self.entries[i] = None;
                // Compact.
                if i + 1 < self.count {
                    self.entries[i] = self.entries[self.count - 1];
                }
                self.entries[self.count - 1] = None;
                self.count -= 1;
                return true;
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// F354 — 时间服务
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockService {
    pub monotonic_ns: u64,
    pub wall_ns: u64,
    /// Drift correction in parts-per-billion applied per second.
    pub drift_ppb: i32,
}

impl ClockService {
    pub const fn new() -> ClockService {
        ClockService { monotonic_ns: 0, wall_ns: 0, drift_ppb: 0 }
    }

    /// Advance both clocks. The monotonic clock never jumps backwards and
    /// ignores drift (that is the point of monotonic time).
    pub fn advance(&mut self, elapsed_ns: u64) {
        self.monotonic_ns = self.monotonic_ns.saturating_add(elapsed_ns);
        let correction = (elapsed_ns as i64 * self.drift_ppb as i64) / 1_000_000_000;
        let wall = self.wall_ns as i64 + elapsed_ns as i64 + correction;
        self.wall_ns = wall.max(0) as u64;
    }

    /// Apply a wall-clock step (NTP / RTC set) without touching monotonic.
    pub fn set_wall(&mut self, wall_ns: u64) {
        self.wall_ns = wall_ns;
    }

    pub fn drift_ms_per_day(&self) -> i32 {
        ((self.drift_ppb as i64) / 1_000_000 * 86_400_000 / 1000) as i32
    }
}

// ---------------------------------------------------------------------------
// F355 — 定时器服务
// ---------------------------------------------------------------------------

pub const TIMER_SLOTS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimerEntry {
    pub id: u32,
    /// Absolute tick at which the timer fires.
    pub deadline_tick: u64,
    pub period_ticks: u64,
    pub active: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct TimerWheel {
    entries: [Option<TimerEntry>; TIMER_SLOTS],
    count: usize,
    fired: u32,
}

impl TimerWheel {
    pub const fn new() -> TimerWheel {
        TimerWheel { entries: [None; TIMER_SLOTS], count: 0, fired: 0 }
    }

    pub fn schedule(&mut self, id: u32, now_tick: u64, delay_ticks: u64, period_ticks: u64) -> bool {
        if self.count >= TIMER_SLOTS || delay_ticks == 0 {
            return false;
        }
        self.entries[self.count] = Some(TimerEntry {
            id,
            deadline_tick: now_tick.saturating_add(delay_ticks),
            period_ticks,
            active: true,
        });
        self.count += 1;
        true
    }

    pub fn cancel(&mut self, id: u32) -> bool {
        for i in 0..self.count {
            if let Some(mut e) = self.entries[i] {
                if e.id == id && e.active {
                    e.active = false;
                    self.entries[i] = Some(e);
                    return true;
                }
            }
        }
        false
    }

    /// Fire every timer whose deadline has passed; one-shot timers go
    /// inactive, periodic ones re-arm. Returns the count fired.
    pub fn fire(&mut self, now_tick: u64) -> usize {
        let mut n = 0usize;
        for i in 0..self.count {
            if let Some(mut e) = self.entries[i] {
                if e.active && now_tick >= e.deadline_tick {
                    n += 1;
                    self.fired += 1;
                    if e.period_ticks == 0 {
                        e.active = false;
                    } else {
                        e.deadline_tick = e.deadline_tick.saturating_add(e.period_ticks);
                        while e.deadline_tick <= now_tick {
                            e.deadline_tick = e.deadline_tick.saturating_add(e.period_ticks);
                        }
                    }
                    self.entries[i] = Some(e);
                }
            }
        }
        n
    }

    pub fn active_count(&self) -> usize {
        (0..self.count)
            .filter(|i| self.entries[*i].map(|e| e.active).unwrap_or(false))
            .count()
    }

    pub fn fired_total(&self) -> u32 {
        self.fired
    }
}

// ---------------------------------------------------------------------------
// F356 — 日志服务
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogRoute {
    pub module: &'static str,
    pub min_level: LogLevel,
    /// Messages per second allowed from this module.
    pub rate_per_sec: u32,
}

pub const MAX_LOG_ROUTES: usize = 12;

#[derive(Clone, Copy, Debug)]
pub struct LogRouter {
    routes: [Option<LogRoute>; MAX_LOG_ROUTES],
    count: usize,
    pub default_level: LogLevel,
    pub suppressed: u32,
}

impl LogRouter {
    pub const fn new(default_level: LogLevel) -> LogRouter {
        LogRouter {
            routes: [None; MAX_LOG_ROUTES],
            count: 0,
            default_level,
            suppressed: 0,
        }
    }

    pub fn set_route(&mut self, route: LogRoute) {
        for i in 0..self.count {
            if let Some(mut r) = self.routes[i] {
                if r.module == route.module {
                    r.min_level = route.min_level;
                    r.rate_per_sec = route.rate_per_sec;
                    self.routes[i] = Some(r);
                    return;
                }
            }
        }
        if self.count < MAX_LOG_ROUTES {
            self.routes[self.count] = Some(route);
            self.count += 1;
        }
    }

    pub fn threshold(&self, module: &str) -> LogLevel {
        for i in 0..self.count {
            if let Some(r) = self.routes[i] {
                if r.module == module {
                    return r.min_level;
                }
            }
        }
        self.default_level
    }

    /// Level gate + rate gate (F472 throttle lives one layer up).
    pub fn should_emit(&self, module: &str, level: LogLevel, per_sec_used: u32) -> bool {
        if level < self.threshold(module) {
            return false;
        }
        for i in 0..self.count {
            if let Some(r) = self.routes[i] {
                if r.module == module {
                    return per_sec_used < r.rate_per_sec.max(1);
                }
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// F357 — 配置服务
// ---------------------------------------------------------------------------

pub const MAX_CONFIG_KEYS: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigValue {
    Int(i64),
    Bool(bool),
    Flag,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfigEntry {
    pub key: &'static str,
    pub value: ConfigValue,
    pub version: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct ConfigStore {
    entries: [Option<ConfigEntry>; MAX_CONFIG_KEYS],
    count: usize,
    /// Bumped on every change — subscribers compare this (F358).
    pub generation: u32,
}

impl ConfigStore {
    pub const fn new() -> ConfigStore {
        ConfigStore { entries: [None; MAX_CONFIG_KEYS], count: 0, generation: 0 }
    }

    pub fn set(&mut self, key: &'static str, value: ConfigValue) -> bool {
        for i in 0..self.count {
            if let Some(mut e) = self.entries[i] {
                if e.key == key {
                    e.value = value;
                    e.version += 1;
                    self.entries[i] = Some(e);
                    self.generation += 1;
                    return true;
                }
            }
        }
        if self.count >= MAX_CONFIG_KEYS {
            return false;
        }
        self.entries[self.count] =
            Some(ConfigEntry { key, value, version: 1 });
        self.count += 1;
        self.generation += 1;
        true
    }

    pub fn get(&self, key: &str) -> Option<ConfigValue> {
        for i in 0..self.count {
            if let Some(e) = self.entries[i] {
                if e.key == key {
                    return Some(e.value);
                }
            }
        }
        None
    }

    pub fn get_int(&self, key: &str) -> Option<i64> {
        match self.get(key)? {
            ConfigValue::Int(v) => Some(v),
            ConfigValue::Bool(b) => Some(b as i64),
            ConfigValue::Flag => Some(0),
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// F358 — 状态同步
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VersionVector {
    pub service: u32,
    pub version: u64,
    pub generation: u32,
}

/// A vector is newer when its generation advanced, or (same generation) its
/// version grew — this keeps peers from applying stale state.
pub fn vector_is_newer(candidate: VersionVector, current: VersionVector) -> bool {
    if candidate.service != current.service {
        return false;
    }
    candidate.generation > current.generation
        || (candidate.generation == current.generation && candidate.version > current.version)
}

pub fn vector_merge(a: VersionVector, b: VersionVector) -> VersionVector {
    if a.service != b.service {
        return a;
    }
    VersionVector {
        service: a.service,
        version: a.version.max(b.version),
        generation: a.generation.max(b.generation),
    }
}

// ---------------------------------------------------------------------------
// F359 — 包管理器
// ---------------------------------------------------------------------------

pub const MAX_PKGS: usize = 8;
pub const MAX_DEPS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Package {
    pub name: &'static str,
    pub version: u32,
    pub deps: [&'static str; MAX_DEPS],
    pub dep_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolveError {
    MissingDependency(&'static str),
    Duplicate(&'static str),
    Cycle,
    TooMany,
}

/// Resolve an install order for `pkgs` (dependencies first).
/// `order` receives indices into `pkgs`.
pub fn resolve_packages(pkgs: &[Package], order: &mut [usize]) -> Result<usize, ResolveError> {
    let n = pkgs.len();
    if n > order.len() || n > MAX_PKGS {
        return Err(ResolveError::TooMany);
    }
    for i in 0..n {
        for j in 0..i {
            if pkgs[i].name == pkgs[j].name {
                return Err(ResolveError::Duplicate(pkgs[i].name));
            }
        }
        for d in 0..pkgs[i].dep_count {
            let dep = pkgs[i].deps[d];
            if !pkgs.iter().any(|p| p.name == dep) {
                return Err(ResolveError::MissingDependency(dep));
            }
        }
    }
    let mut emitted = [false; MAX_PKGS];
    let mut count = 0usize;
    while count < n {
        let mut progressed = false;
        for i in 0..n {
            if emitted[i] {
                continue;
            }
            let ready = (0..pkgs[i].dep_count).all(|d| {
                let dep = pkgs[i].deps[d];
                (0..n).any(|j| pkgs[j].name == dep && emitted[j])
            });
            if ready {
                emitted[i] = true;
                order[count] = i;
                count += 1;
                progressed = true;
            }
        }
        if !progressed {
            return Err(ResolveError::Cycle);
        }
    }
    Ok(count)
}

// ---------------------------------------------------------------------------
// F360/F361 — A/B 更新与回滚
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbSlot {
    /// 0 or 1 — the slot currently booted.
    pub active: u8,
    pub bootable: [bool; 2],
    /// Remaining boot attempts before the slot is declared bad.
    pub tries_left: [u8; 2],
    pub max_tries: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbAction {
    Stay,
    Switch,
    Rollback,
    Refuse,
}

impl AbSlot {
    pub const fn new() -> AbSlot {
        AbSlot { active: 0, bootable: [true, true], tries_left: [3, 3], max_tries: 3 }
    }

    pub fn inactive(&self) -> u8 {
        1 - self.active
    }

    /// Stage an update into the inactive slot.
    pub fn stage(&mut self) -> bool {
        let other = self.inactive() as usize;
        if !self.bootable[other] {
            return false;
        }
        self.tries_left[other] = self.max_tries;
        true
    }

    /// Boot attempt on the active slot: consumes one try.
    /// Returns the action the boot loader must take.
    pub fn boot_attempt(&mut self) -> AbAction {
        let active = self.active as usize;
        if self.tries_left[active] > 0 {
            self.tries_left[active] -= 1;
            return AbAction::Stay;
        }
        // Tries exhausted: the slot never reached "successful boot".
        self.bootable[active] = false;
        let other = self.inactive() as usize;
        if self.bootable[other] {
            self.active = other as u8;
            self.tries_left[other] = self.max_tries;
            AbAction::Rollback
        } else {
            AbAction::Refuse
        }
    }

    /// The running system proved healthy — reset the try counter.
    pub fn mark_successful(&mut self) {
        let active = self.active as usize;
        self.tries_left[active] = self.max_tries;
    }

    pub fn switch_to_inactive(&mut self) -> AbAction {
        let other = self.inactive() as usize;
        if !self.bootable[other] {
            return AbAction::Refuse;
        }
        self.active = other as u8;
        self.tries_left[other] = self.max_tries;
        AbAction::Switch
    }
}

/// F361: auto-rollback decision for a slot that failed to reach userspace.
pub fn should_rollback(failed_boots: u8, max_tries: u8, other_slot_bootable: bool) -> bool {
    failed_boots >= max_tries && other_slot_bootable
}

// ---------------------------------------------------------------------------
// F362 — 崩溃报告
// ---------------------------------------------------------------------------

pub const MAX_CRASHES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrashRecord {
    pub app: &'static str,
    /// Hash of the faulting instruction + module — the dedupe key.
    pub signature: u64,
    pub count: u32,
    pub last_ms: u32,
    /// Always false: zero-telemetry promise (F247/F422).
    pub uploaded: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct CrashStore {
    records: [Option<CrashRecord>; MAX_CRASHES],
    count: usize,
}

impl CrashStore {
    pub const fn new() -> CrashStore {
        CrashStore { records: [None; MAX_CRASHES], count: 0 }
    }

    /// Record a crash; identical signatures increment the counter instead of
    /// growing the store (crash loops must not fill memory).
    pub fn record(&mut self, app: &'static str, signature: u64, now_ms: u32) -> bool {
        for i in 0..self.count {
            if let Some(mut r) = self.records[i] {
                if r.app == app && r.signature == signature {
                    r.count += 1;
                    r.last_ms = now_ms;
                    self.records[i] = Some(r);
                    return true;
                }
            }
        }
        if self.count >= MAX_CRASHES {
            return false;
        }
        self.records[self.count] = Some(CrashRecord {
            app,
            signature,
            count: 1,
            last_ms: now_ms,
            uploaded: false,
        });
        self.count += 1;
        true
    }

    pub fn find(&self, app: &str, signature: u64) -> Option<CrashRecord> {
        for i in 0..self.count {
            if let Some(r) = self.records[i] {
                if r.app == app && r.signature == signature {
                    return Some(r);
                }
            }
        }
        None
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn any_uploaded(&self) -> bool {
        (0..self.count).any(|i| self.records[i].map(|r| r.uploaded).unwrap_or(false))
    }
}

// ---------------------------------------------------------------------------
// F363 — 通知服务
// ---------------------------------------------------------------------------

pub const MAX_NOTIFICATIONS: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Urgency {
    Low,
    Normal,
    High,
    Critical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Notification {
    pub id: u32,
    pub app: &'static str,
    pub urgency: Urgency,
    pub actions: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct NotificationCenter {
    items: [Option<Notification>; MAX_NOTIFICATIONS],
    count: usize,
    pub dnd: bool,
}

impl NotificationCenter {
    pub const fn new() -> NotificationCenter {
        NotificationCenter { items: [None; MAX_NOTIFICATIONS], count: 0, dnd: false }
    }

    /// Post a notification. Returns `true` when it should banner immediately
    /// (Low urgency is silently collected into the centre).
    pub fn post(&mut self, n: Notification) -> Result<bool, &'static str> {
        if self.count >= MAX_NOTIFICATIONS {
            return Err("notification centre full");
        }
        // Coalesce: the same app at the same urgency replaces its entry.
        for i in 0..self.count {
            if let Some(mut old) = self.items[i] {
                if old.app == n.app && old.urgency == n.urgency {
                    old.id = n.id;
                    self.items[i] = Some(old);
                    return Ok(!self.dnd && n.urgency >= Urgency::High);
                }
            }
        }
        self.items[self.count] = Some(n);
        self.count += 1;
        let interrupting = !self.dnd
            && (n.urgency >= Urgency::High || (n.urgency == Urgency::Critical));
        Ok(interrupting)
    }

    pub fn dismiss(&mut self, id: u32) -> bool {
        for i in 0..self.count {
            if self.items[i].map(|n| n.id == id).unwrap_or(false) {
                self.items[i] = None;
                if i + 1 < self.count {
                    self.items[i] = self.items[self.count - 1];
                }
                self.items[self.count - 1] = None;
                self.count -= 1;
                return true;
            }
        }
        false
    }

    pub fn by_app(&self, app: &str) -> usize {
        (0..self.count)
            .filter(|i| self.items[*i].map(|n| n.app == app).unwrap_or(false))
            .count()
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// F364/F365 — 剪贴板与拖放
// ---------------------------------------------------------------------------

pub const CAP_CLIP_ENTRIES: usize = 8;
pub const CLIP_HISTORY: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipKind {
    Text,
    Html,
    Image,
    Files,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipEntry {
    pub kind: ClipKind,
    pub owner: &'static str,
    pub bytes: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct ClipboardStore {
    history: [Option<ClipEntry>; CLIP_HISTORY],
    head: usize,
    count: usize,
    pub current: Option<ClipEntry>,
}

impl ClipboardStore {
    pub const fn new() -> ClipboardStore {
        ClipboardStore { history: [None; CLIP_HISTORY], head: 0, count: 0, current: None }
    }

    pub fn set(&mut self, entry: ClipEntry) -> bool {
        if entry.bytes > CAP_CLIP_ENTRIES * 1024 * 1024 {
            return false;
        }
        self.history[self.head] = Some(entry);
        self.head = (self.head + 1) % CLIP_HISTORY;
        if self.count < CLIP_HISTORY {
            self.count += 1;
        }
        self.current = Some(entry);
        true
    }

    /// Newest-first history access.
    pub fn history(&self, index: usize) -> Option<ClipEntry> {
        if index >= self.count {
            return None;
        }
        let pos = (self.head + CLIP_HISTORY - 1 - index) % CLIP_HISTORY;
        self.history[pos]
    }

    pub fn history_len(&self) -> usize {
        self.count
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DragPayload {
    pub kinds: u32,
    pub bytes: usize,
}

pub const DRAG_TEXT: u32 = 1 << 0;
pub const DRAG_FILES: u32 = 1 << 1;
pub const DRAG_IMAGE: u32 = 1 << 2;
pub const DRAG_URL: u32 = 1 << 3;

/// A drop is allowed when the target accepts one of the payload kinds.
pub fn drag_allowed(target_accepts: u32, payload: DragPayload) -> bool {
    target_accepts & payload.kinds != 0 && payload.bytes <= 64 << 20
}

// ---------------------------------------------------------------------------
// F366/F367 — 主题与字体服务
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemeTokens {
    /// 0xRRGGBB.
    pub accent: u32,
    pub radius_px: u8,
    pub spacing_px: u8,
    pub motion_ms: u16,
    /// Blur strength in permille of the base sheet.
    pub blur_permille: u16,
}

impl ThemeTokens {
    pub const fn base() -> ThemeTokens {
        ThemeTokens { accent: 0x3B82F6, radius_px: 10, spacing_px: 8, motion_ms: 180, blur_permille: 1000 }
    }
}

/// Overlay merge: a `Some`-like field in the overlay (non-zero) wins.
pub fn merge_tokens(base: ThemeTokens, overlay: ThemeTokens) -> ThemeTokens {
    ThemeTokens {
        accent: if overlay.accent != 0 { overlay.accent } else { base.accent },
        radius_px: if overlay.radius_px != 0 { overlay.radius_px } else { base.radius_px },
        spacing_px: if overlay.spacing_px != 0 { overlay.spacing_px } else { base.spacing_px },
        motion_ms: if overlay.motion_ms != 0 { overlay.motion_ms } else { base.motion_ms },
        blur_permille: if overlay.blur_permille != 0 { overlay.blur_permille } else { base.blur_permille },
    }
}

pub const MAX_FAMILIES: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct FontRegistry {
    families: [Option<&'static str>; MAX_FAMILIES],
    count: usize,
    fallback: [Option<&'static str>; MAX_FAMILIES],
    fallback_count: usize,
}

impl FontRegistry {
    pub const fn new() -> FontRegistry {
        FontRegistry {
            families: [None; MAX_FAMILIES],
            count: 0,
            fallback: [None; MAX_FAMILIES],
            fallback_count: 0,
        }
    }

    pub fn register(&mut self, family: &'static str) -> bool {
        if self.count >= MAX_FAMILIES || self.families[..self.count].contains(&Some(family)) {
            return false;
        }
        self.families[self.count] = Some(family);
        self.count += 1;
        true
    }

    pub fn push_fallback(&mut self, family: &'static str) -> bool {
        if self.fallback_count >= MAX_FAMILIES {
            return false;
        }
        self.fallback[self.fallback_count] = Some(family);
        self.fallback_count += 1;
        true
    }

    /// Resolve a requested family, falling back down the chain (never `None`
    /// when at least one family is registered).
    pub fn resolve(&self, want: &str) -> Option<&'static str> {
        for i in 0..self.count {
            if self.families[i] == Some(want) {
                return self.families[i];
            }
        }
        for i in 0..self.fallback_count {
            if let Some(f) = self.fallback[i] {
                if self.has(f) {
                    return Some(f);
                }
            }
        }
        if self.count > 0 {
            self.families[0]
        } else {
            None
        }
    }

    fn has(&self, family: &str) -> bool {
        self.families[..self.count].contains(&Some(family))
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// F368/F369/F370 — 帮助、首运行向导、权限提示
// ---------------------------------------------------------------------------

pub const MAX_HELP_TOPICS: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HelpTopic {
    pub id: &'static str,
    pub context: &'static str,
    pub body_len: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct HelpIndex {
    topics: [Option<HelpTopic>; MAX_HELP_TOPICS],
    count: usize,
}

impl HelpIndex {
    pub const fn new() -> HelpIndex {
        HelpIndex { topics: [None; MAX_HELP_TOPICS], count: 0 }
    }

    pub fn add(&mut self, topic: HelpTopic) -> bool {
        if self.count >= MAX_HELP_TOPICS {
            return false;
        }
        self.topics[self.count] = Some(topic);
        self.count += 1;
        true
    }

    /// Contextual lookup: exact context match first, then any topic.
    pub fn lookup(&self, context: &str) -> Option<HelpTopic> {
        let mut any: Option<HelpTopic> = None;
        for i in 0..self.count {
            if let Some(t) = self.topics[i] {
                if t.context == context {
                    return Some(t);
                }
                if any.is_none() {
                    any = Some(t);
                }
            }
        }
        any
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

/// OOBE steps as a bitmask — resumable across a reboot.
pub const OOBE_STEP_LANGUAGE: u8 = 1 << 0;
pub const OOBE_STEP_ACCOUNT: u8 = 1 << 1;
pub const OOBE_STEP_THEME: u8 = 1 << 2;
pub const OOBE_STEP_NETWORK: u8 = 1 << 3;
pub const OOBE_STEP_DONE: u8 = 1 << 4;
pub const OOBE_ALL: u8 = 0x1F;

pub fn oobe_next(completed: u8) -> Option<u8> {
    let order = [
        OOBE_STEP_LANGUAGE,
        OOBE_STEP_ACCOUNT,
        OOBE_STEP_THEME,
        OOBE_STEP_NETWORK,
        OOBE_STEP_DONE,
    ];
    order.iter().copied().find(|s| completed & s == 0)
}

pub fn oobe_progress_permille(completed: u8) -> u16 {
    (completed & OOBE_ALL).count_ones() as u16 * 1000 / 5
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptAnswer {
    Allow,
    Deny,
    AllowOnce,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PermissionPrompt {
    pub capability: &'static str,
    pub answer: PromptAnswer,
    pub remembered: bool,
}

/// A remembered "deny" must never be silently bypassed by a retry.
pub fn prompt_decision(prompt: PermissionPrompt, retry: bool) -> bool {
    match prompt.answer {
        PromptAnswer::Allow => true,
        PromptAnswer::AllowOnce => !retry,
        PromptAnswer::Deny => !prompt.remembered && retry,
    }
}

// ---------------------------------------------------------------------------
// F371 — 事件广播
// ---------------------------------------------------------------------------

pub const MAX_EVENT_LISTENERS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventPriority {
    Idle,
    Normal,
    Foreground,
    Critical,
}

#[derive(Clone, Copy, Debug)]
pub struct EventListener {
    pub name: &'static str,
    pub priority: EventPriority,
    pub alive: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct EventBus {
    listeners: [Option<EventListener>; MAX_EVENT_LISTENERS],
    count: usize,
    pub broadcasts: u32,
}

impl EventBus {
    pub const fn new() -> EventBus {
        EventBus { listeners: [None; MAX_EVENT_LISTENERS], count: 0, broadcasts: 0 }
    }

    pub fn listen(&mut self, listener: EventListener) -> bool {
        if self.count >= MAX_EVENT_LISTENERS {
            return false;
        }
        self.listeners[self.count] = Some(listener);
        self.count += 1;
        true
    }

    /// Broadcast order is priority-first (critical listeners must never be
    /// queued behind an idle one), stable within a priority.
    pub fn broadcast_order(&self, out: &mut [usize]) -> usize {
        let mut n = 0usize;
        for priority in [
            EventPriority::Critical,
            EventPriority::Foreground,
            EventPriority::Normal,
            EventPriority::Idle,
        ] {
            for i in 0..self.count {
                if let Some(l) = self.listeners[i] {
                    if l.alive && l.priority == priority && n < out.len() {
                        out[n] = i;
                        n += 1;
                    }
                }
            }
        }
        n
    }

    pub fn broadcast(&mut self, out: &mut [usize]) -> usize {
        let n = self.broadcast_order(out);
        self.broadcasts += 1;
        n
    }
}

// ---------------------------------------------------------------------------
// F372/F373/F374 — 健康监测、依赖图、降级策略
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceHealth {
    pub name: &'static str,
    pub last_heartbeat_ms: u32,
    pub timeout_ms: u32,
    pub restarts: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthAction {
    Healthy,
    Restart,
    /// Too many restarts: drop to the degraded path (F374).
    Degrade,
}

impl ServiceHealth {
    pub fn tick(&mut self, now_ms: u32) -> HealthAction {
        if now_ms.saturating_sub(self.last_heartbeat_ms) <= self.timeout_ms {
            return HealthAction::Healthy;
        }
        self.restarts += 1;
        if self.restarts > 3 {
            HealthAction::Degrade
        } else {
            // Restart with backoff: give the service a longer grace window.
            self.timeout_ms = self.timeout_ms.saturating_mul(2);
            self.last_heartbeat_ms = now_ms;
            HealthAction::Restart
        }
    }
}

pub const MAX_SERVICES: usize = 10;

#[derive(Clone, Copy, Debug)]
pub struct DependencyGraph {
    names: [Option<&'static str>; MAX_SERVICES],
    deps: [Option<[usize; 4]>; MAX_SERVICES],
    dep_counts: [usize; MAX_SERVICES],
    count: usize,
}

impl DependencyGraph {
    pub const fn new() -> DependencyGraph {
        DependencyGraph {
            names: [None; MAX_SERVICES],
            deps: [None; MAX_SERVICES],
            dep_counts: [0; MAX_SERVICES],
            count: 0,
        }
    }

    pub fn add(&mut self, name: &'static str, dep_indices: &[usize]) -> bool {
        if self.count >= MAX_SERVICES || dep_indices.len() > 4 {
            return false;
        }
        let mut d = [0usize; 4];
        for (i, v) in dep_indices.iter().enumerate() {
            if *v >= self.count {
                return false; // only previously-added services
            }
            d[i] = *v;
        }
        self.names[self.count] = Some(name);
        self.deps[self.count] = Some(d);
        self.dep_counts[self.count] = dep_indices.len();
        self.count += 1;
        true
    }

    /// Topological start order. Cycles cannot form because dependencies must
    /// already exist, but the guard stays for future dynamic edits.
    pub fn start_order(&self, out: &mut [usize]) -> Result<usize, &'static str> {
        let mut emitted = [false; MAX_SERVICES];
        let mut n = 0usize;
        while n < self.count {
            let mut progressed = false;
            for i in 0..self.count {
                if emitted[i] {
                    continue;
                }
                let ready = match self.deps[i] {
                    Some(d) => (0..self.dep_counts[i]).all(|k| emitted[d[k]]),
                    None => true,
                };
                if ready {
                    emitted[i] = true;
                    if n < out.len() {
                        out[n] = i;
                    }
                    n += 1;
                    progressed = true;
                }
            }
            if !progressed {
                return Err("dependency cycle");
            }
        }
        Ok(n)
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        (0..self.count).find(|i| self.names[*i] == Some(name))
    }

    pub fn count(&self) -> usize {
        self.count
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DegradePolicy {
    pub service: &'static str,
    /// Dependencies that must be healthy for the full path.
    pub required: [&'static str; 2],
    pub required_count: usize,
    /// Fallback service used when a dependency is missing.
    pub fallback: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradePlan {
    Full,
    UseFallback(&'static str),
    Unavailable,
}

pub fn degrade_plan(policy: DegradePolicy, dep_healthy: [bool; 2]) -> DegradePlan {
    let missing = (0..policy.required_count).any(|i| !dep_healthy[i]);
    if !missing {
        return DegradePlan::Full;
    }
    if policy.fallback.is_empty() {
        DegradePlan::Unavailable
    } else {
        DegradePlan::UseFallback(policy.fallback)
    }
}

// ---------------------------------------------------------------------------
// F375 — 服务自检
// ---------------------------------------------------------------------------

pub fn run_service_checks() -> CheckSet {
    let mut set = CheckSet::new("service");

    set.add(
        "F351 ipc",
        ipc_allowed(CAP_SEND | CAP_RECEIVE, CAP_SEND)
            && !ipc_allowed(CAP_SEND, CAP_ADMIN)
            && ipc_size_ok(IpcEnvelope { from: 1, to: "svc", kind: IpcKind::Call, payload_len: 512 })
            && !ipc_size_ok(IpcEnvelope {
                from: 1,
                to: "svc",
                kind: IpcKind::Stream,
                payload_len: IPC_MSG_MAX + 1,
            }),
        "capabilities",
    );

    let mut bus = MessageBus::new();
    bus.subscribe(BusSubscription { topic: Topic::Power, name: "a", pid: 1 });
    bus.subscribe(BusSubscription { topic: Topic::Power, name: "b", pid: 2 });
    bus.subscribe(BusSubscription { topic: Topic::Audio, name: "c", pid: 3 });
    let delivered = bus.publish(Topic::Power);
    let removed = bus.unsubscribe(2);
    let after = bus.publish(Topic::Power);
    set.add(
        "F352 message bus",
        delivered == 2 && removed == 1 && after == 1 && bus.subscriber_count(Topic::Audio) == 1,
        "pub/sub",
    );

    let mut names = NameRegistry::new();
    let first = names.register("display", 10);
    let second = names.register("display", 11);
    names.mark_dead(10);
    let third = names.register("display", 12);
    set.add(
        "F353 name service",
        first == RegisterOutcome::Registered
            && second == RegisterOutcome::Taken
            && third == RegisterOutcome::Updated
            && names.lookup("display") == Some(12)
            && names.remove("display")
            && names.lookup("display").is_none(),
        "registry",
    );

    let mut clock = ClockService { monotonic_ns: 0, wall_ns: 1_000_000_000, drift_ppb: 0 };
    clock.advance(500);
    let mono = clock.monotonic_ns;
    clock.set_wall(42);
    set.add(
        "F354 time service",
        mono == 500 && clock.monotonic_ns == 500 && clock.wall_ns == 42,
        "monotonic guard",
    );

    let mut wheel = TimerWheel::new();
    wheel.schedule(1, 100, 10, 0);
    wheel.schedule(2, 100, 10, 20);
    let first_fire = wheel.fire(110);
    let idle = wheel.fire(120);
    let periodic = wheel.fire(130);
    set.add(
        "F355 timer service",
        first_fire == 2 && idle == 0 && periodic == 1 && wheel.cancel(2) && wheel.active_count() == 0,
        "wheel",
    );

    let mut router = LogRouter::new(LogLevel::Info);
    router.set_route(LogRoute { module: "net", min_level: LogLevel::Warn, rate_per_sec: 5 });
    set.add(
        "F356 log service",
        router.should_emit("net", LogLevel::Error, 0)
            && !router.should_emit("net", LogLevel::Info, 0)
            && !router.should_emit("net", LogLevel::Error, 5)
            && router.should_emit("fs", LogLevel::Info, 999),
        "level + rate gate",
    );

    let mut cfg = ConfigStore::new();
    cfg.set("theme.accent", ConfigValue::Int(0x3B82F6));
    let g1 = cfg.generation;
    cfg.set("theme.accent", ConfigValue::Int(0xFF0000));
    set.add(
        "F357 config service",
        cfg.get_int("theme.accent") == Some(0xFF0000)
            && cfg.generation == g1 + 1
            && cfg.get("missing").is_none()
            && cfg.len() == 1,
        "kv + generation",
    );

    let v1 = VersionVector { service: 1, version: 5, generation: 2 };
    let v2 = VersionVector { service: 1, version: 6, generation: 2 };
    let v3 = VersionVector { service: 2, version: 9, generation: 9 };
    set.add(
        "F358 state sync",
        vector_is_newer(v2, v1)
            && !vector_is_newer(v1, v2)
            && !vector_is_newer(v3, v1)
            && vector_merge(v1, v2).version == 6,
        "vectors",
    );

    let pkgs = [
        Package { name: "base", version: 1, deps: [""; MAX_DEPS], dep_count: 0 },
        Package { name: "shell", version: 2, deps: ["base", "", "", ""], dep_count: 1 },
        Package { name: "ui", version: 3, deps: ["shell", "base", "", ""], dep_count: 2 },
    ];
    let mut order = [0usize; MAX_PKGS];
    let resolved = resolve_packages(&pkgs, &mut order);
    let dup = resolve_packages(
        &[pkgs[0], Package { name: "base", version: 9, ..pkgs[0] }],
        &mut order,
    );
    let missing = resolve_packages(&[pkgs[1]], &mut order);
    set.add(
        "F359 package manager",
        resolved == Ok(3)
            && order[0] == 0
            && order[2] == 2
            && dup == Err(ResolveError::Duplicate("base"))
            && missing == Err(ResolveError::MissingDependency("base")),
        "resolve",
    );

    let mut ab = AbSlot::new();
    let staged = ab.stage();
    let switch = ab.switch_to_inactive();
    let active_after = ab.active;
    let mut attempt = AbAction::Stay;
    for _ in 0..4 {
        attempt = ab.boot_attempt();
    }
    set.add(
        "F360/F361 ab update",
        staged
            && switch == AbAction::Switch
            && active_after == 1
            && attempt == AbAction::Rollback
            && ab.active == 0
            && should_rollback(3, 3, true)
            && !should_rollback(3, 3, false),
        "a/b + rollback",
    );
    let mut healthy = AbSlot::new();
    healthy.boot_attempt();
    healthy.mark_successful();
    set.add("F361 successful boot", healthy.tries_left[0] == healthy.max_tries, "try reset");

    let mut crashes = CrashStore::new();
    crashes.record("app", 0xABCD, 100);
    crashes.record("app", 0xABCD, 200);
    crashes.record("app", 0x1234, 300);
    set.add(
        "F362 crash report",
        crashes.len() == 2
            && crashes.find("app", 0xABCD).map(|r| r.count) == Some(2)
            && !crashes.any_uploaded(),
        "local only",
    );

    let mut nc = NotificationCenter::new();
    let low = nc.post(Notification { id: 1, app: "mail", urgency: Urgency::Low, actions: 0 });
    let high = nc.post(Notification { id: 2, app: "vault", urgency: Urgency::High, actions: 2 });
    // Coalesced into the existing vault entry; the result is intentionally
    // discarded because the entry already bannered.
    let _ = nc.post(Notification { id: 3, app: "vault", urgency: Urgency::High, actions: 2 });
    nc.dnd = true;
    let dnd = nc.post(Notification { id: 4, app: "chat", urgency: Urgency::Critical, actions: 1 });
    set.add(
        "F363 notifications",
        low == Ok(false)
            && high == Ok(true)
            && nc.by_app("vault") == 1
            && dnd == Ok(false)
            && nc.dismiss(3)
            && nc.len() == 2,
        "coalesce + dnd",
    );

    let mut clip = ClipboardStore::new();
    let ok = clip.set(ClipEntry { kind: ClipKind::Text, owner: "editor", bytes: 1024 });
    let too_big = clip.set(ClipEntry {
        kind: ClipKind::Image,
        owner: "editor",
        bytes: CAP_CLIP_ENTRIES * 1024 * 1024 + 1,
    });
    set.add(
        "F364 clipboard",
        ok && !too_big && clip.history_len() == 1
            && clip.history(0).map(|e| e.owner) == Some("editor")
            && clip.history(1).is_none(),
        "clipboard",
    );

    set.add(
        "F365 drag drop",
        drag_allowed(DRAG_TEXT | DRAG_URL, DragPayload { kinds: DRAG_URL, bytes: 100 })
            && !drag_allowed(DRAG_TEXT, DragPayload { kinds: DRAG_FILES, bytes: 10 })
            && !drag_allowed(DRAG_TEXT, DragPayload { kinds: DRAG_TEXT, bytes: 128 << 20 }),
        "negotiation",
    );

    let base = ThemeTokens::base();
    let overlay = ThemeTokens { accent: 0xFF0000, radius_px: 0, spacing_px: 0, motion_ms: 0, blur_permille: 0 };
    let merged = merge_tokens(base, overlay);
    set.add(
        "F366 theme service",
        merged.accent == 0xFF0000 && merged.radius_px == base.radius_px && merged.motion_ms == 180,
        "overlay layering",
    );

    let mut fonts = FontRegistry::new();
    fonts.register("Varix Sans");
    fonts.register("Noto Sans CJK");
    fonts.push_fallback("Noto Sans CJK");
    set.add(
        "F367 font service",
        fonts.resolve("Varix Sans") == Some("Varix Sans")
            && fonts.resolve("missing") == Some("Noto Sans CJK")
            && fonts.len() == 2
            && !fonts.register("Varix Sans"),
        "fallback chain",
    );
    let empty = FontRegistry::new();
    set.add("F367 empty registry", empty.resolve("x").is_none(), "no crash");

    let mut help = HelpIndex::new();
    help.add(HelpTopic { id: "power", context: "power", body_len: 120 });
    help.add(HelpTopic { id: "net", context: "network", body_len: 200 });
    set.add(
        "F368 help service",
        help.lookup("network").map(|t| t.id) == Some("net")
            && help.lookup("unknown").is_some()
            && help.len() == 2,
        "contextual help",
    );

    let mut completed = OOBE_STEP_LANGUAGE;
    let mut steps = 0;
    while let Some(step) = oobe_next(completed) {
        completed |= step;
        steps += 1;
        if steps > 8 {
            break;
        }
    }
    set.add(
        "F369 first run wizard",
        steps == 4 && completed == OOBE_ALL && oobe_progress_permille(OOBE_ALL) == 1000
            && oobe_next(OOBE_ALL).is_none(),
        "resumable oobe",
    );

    set.add(
        "F370 permission prompt",
        prompt_decision(PermissionPrompt { capability: "camera", answer: PromptAnswer::Allow, remembered: true }, false)
            && !prompt_decision(
                PermissionPrompt { capability: "camera", answer: PromptAnswer::Deny, remembered: true },
                true,
            )
            && prompt_decision(
                PermissionPrompt { capability: "camera", answer: PromptAnswer::AllowOnce, remembered: false },
                false,
            ),
        "prompt policy",
    );

    let mut busv = EventBus::new();
    busv.listen(EventListener { name: "idle", priority: EventPriority::Idle, alive: true });
    busv.listen(EventListener { name: "crit", priority: EventPriority::Critical, alive: true });
    busv.listen(EventListener { name: "dead", priority: EventPriority::Critical, alive: false });
    let mut order2 = [0usize; MAX_EVENT_LISTENERS];
    let n = busv.broadcast_order(&mut order2);
    set.add(
        "F371 event broadcast",
        n == 2 && order2[0] == 1 && order2[1] == 0 && busv.broadcast(&mut order2) == 2,
        "priority order",
    );

    let mut health = ServiceHealth { name: "audio", last_heartbeat_ms: 0, timeout_ms: 100, restarts: 0 };
    let h1 = health.tick(500);
    let mut degraded = HealthAction::Healthy;
    for i in 0..6 {
        degraded = health.tick(10_000 + i * 100_000);
    }
    set.add(
        "F372 health monitor",
        h1 == HealthAction::Restart && degraded == HealthAction::Degrade,
        "restart + degrade",
    );

    let mut graph = DependencyGraph::new();
    graph.add("klog", &[]);
    graph.add("devmgr", &[0]);
    graph.add("display", &[0, 1]);
    let mut sorder = [0usize; MAX_SERVICES];
    let sres = graph.start_order(&mut sorder);
    set.add(
        "F373 dependency graph",
        sres == Ok(3) && sorder[0] == 0 && sorder[2] == 2 && graph.index_of("devmgr") == Some(1)
            && !graph.add("orphan", &[9]),
        "start order",
    );

    let policy = DegradePolicy {
        service: "audio",
        required: ["hda", "mixer"],
        required_count: 2,
        fallback: "null-sink",
    };
    set.add(
        "F374 degradation",
        degrade_plan(policy, [true, true]) == DegradePlan::Full
            && degrade_plan(policy, [true, false]) == DegradePlan::UseFallback("null-sink")
            && degrade_plan(DegradePolicy { fallback: "", ..policy }, [false, false])
                == DegradePlan::Unavailable,
        "policy",
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
    fn f352_bus_capacity() {
        let mut b = MessageBus::new();
        for i in 0..MAX_SUBSCRIBERS {
            assert!(b.subscribe(BusSubscription { topic: Topic::System, name: "s", pid: i as u32 }));
        }
        assert!(!b.subscribe(BusSubscription { topic: Topic::System, name: "s", pid: 99 }));
        assert_eq!(b.dropped, 1);
        assert_eq!(b.publish(Topic::System), MAX_SUBSCRIBERS);
        assert_eq!(b.unsubscribe(99), 0);
    }

    #[test]
    fn f353_name_registry_capacity() {
        let mut r = NameRegistry::new();
        for i in 0..MAX_NAMES {
            let name: &'static str = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p"][i];
            assert_eq!(r.register(name, i as u32), RegisterOutcome::Registered);
        }
        assert_eq!(r.register("zz", 99), RegisterOutcome::Taken);
        assert_eq!(r.mark_dead(3), 1);
        assert_eq!(r.mark_dead(3), 0);
    }

    #[test]
    fn f354_clock_drift() {
        let mut c = ClockService { monotonic_ns: 0, wall_ns: 0, drift_ppb: 1_000_000_000 };
        c.advance(1_000_000_000);
        assert_eq!(c.monotonic_ns, 1_000_000_000);
        assert_eq!(c.wall_ns, 2_000_000_000); // +1 s of drift correction
        assert_eq!(c.drift_ms_per_day(), 86_400_000);
    }

    #[test]
    fn f355_timer_edges() {
        let mut w = TimerWheel::new();
        assert!(!w.schedule(1, 0, 0, 0));
        assert!(w.schedule(1, 0, 5, 0));
        assert!(!w.cancel(9));
        assert_eq!(w.fire(4), 0);
        assert_eq!(w.fire(5), 1);
        assert_eq!(w.fire(5), 0);
        assert_eq!(w.active_count(), 0);
    }

    #[test]
    fn f356_router_defaults() {
        let r = LogRouter::new(LogLevel::Warn);
        assert_eq!(r.threshold("anything"), LogLevel::Warn);
        assert!(!r.should_emit("x", LogLevel::Info, 0));
        assert!(r.should_emit("x", LogLevel::Error, 0));
    }

    #[test]
    fn f359_package_cycle_detected() {
        let a = Package { name: "a", version: 1, deps: ["b", "", "", ""], dep_count: 1 };
        let b = Package { name: "b", version: 1, deps: ["a", "", "", ""], dep_count: 1 };
        let mut order = [0usize; MAX_PKGS];
        assert_eq!(resolve_packages(&[a, b], &mut order), Err(ResolveError::Cycle));
        let none: [Package; 0] = [];
        assert_eq!(resolve_packages(&none, &mut order), Ok(0));
    }

    #[test]
    fn f360_ab_slot_exhaustion() {
        let mut ab = AbSlot::new();
        assert_eq!(ab.inactive(), 1);
        assert!(ab.stage());
        let mut dead = AbSlot::new();
        dead.bootable[1] = false;
        assert!(!dead.stage());
        assert_eq!(dead.switch_to_inactive(), AbAction::Refuse);
        for _ in 0..ab.max_tries {
            assert_eq!(ab.boot_attempt(), AbAction::Stay);
        }
        assert_eq!(ab.boot_attempt(), AbAction::Rollback);
        assert_eq!(ab.active, 1);
    }

    #[test]
    fn f362_crash_store_capacity() {
        let mut s = CrashStore::new();
        for i in 0..MAX_CRASHES {
            assert!(s.record("app", i as u64, 0));
        }
        assert!(!s.record("app", 999, 0));
        assert!(s.record("app", 0, 500)); // dedupe path still works
        assert_eq!(s.find("app", 0).map(|r| r.count), Some(2));
        assert_eq!(s.find("app", 777), None);
    }

    #[test]
    fn f363_centre_full() {
        let mut c = NotificationCenter::new();
        for i in 0..MAX_NOTIFICATIONS {
            let name: &'static str = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"][i];
            assert!(c
                .post(Notification { id: i as u32, app: name, urgency: Urgency::Normal, actions: 0 })
                .is_ok());
        }
        assert!(c
            .post(Notification { id: 99, app: "z", urgency: Urgency::Low, actions: 0 })
            .is_err());
        assert!(!c.dismiss(1234));
    }

    #[test]
    fn f364_clipboard_history_wraps() {
        let mut c = ClipboardStore::new();
        for i in 0..CLIP_HISTORY + 2 {
            c.set(ClipEntry { kind: ClipKind::Text, owner: "o", bytes: i });
        }
        assert_eq!(c.history_len(), CLIP_HISTORY);
        assert_eq!(c.history(0).map(|e| e.bytes), Some(CLIP_HISTORY + 1));
        assert!(c.history(CLIP_HISTORY).is_none());
    }

    #[test]
    fn f367_font_registry_capacity() {
        let mut f = FontRegistry::new();
        for i in 0..MAX_FAMILIES {
            let name: &'static str = ["a", "b", "c", "d", "e", "f", "g", "h"][i];
            assert!(f.register(name));
        }
        assert!(!f.register("z"));
        assert_eq!(f.resolve("z"), Some("a"));
    }

    #[test]
    fn f373_graph_rejects_forward_deps() {
        let mut g = DependencyGraph::new();
        assert!(!g.add("a", &[0])); // self-reference before it exists
        assert!(g.add("a", &[]));
        assert!(!g.add("b", &[1])); // not added yet
        assert!(g.add("b", &[0]));
        let mut out = [0usize; MAX_SERVICES];
        assert_eq!(g.start_order(&mut out), Ok(2));
    }

    #[test]
    fn f374_degradation_matrix() {
        let p = DegradePolicy { service: "s", required: ["a", "b"], required_count: 1, fallback: "f" };
        assert_eq!(degrade_plan(p, [true, true]), DegradePlan::Full);
        assert_eq!(degrade_plan(p, [false, true]), DegradePlan::UseFallback("f"));
    }

    #[test]
    fn f375_self_test_passes() {
        let set = run_service_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("service self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 24);
    }
}
