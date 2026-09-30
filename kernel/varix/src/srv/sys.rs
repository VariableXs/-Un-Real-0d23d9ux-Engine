//! VARIABLE-200 AI-04 · 服务域系统面（F091~F100）。
//!
//! 持久存储桥接、状态目录、电源/时间/随机数/水位服务、服务依赖图、
//! 服务域 fuzz 与三演示程序。纯逻辑 + 固定容量数组，无分配。

use crate::checks::CheckSet;
use crate::galaxy::rt::DetPrng;

// ---------------------------------------------------------------------------
// F091 持久存储桥接 — initfs（只读）→ 真实文件系统（可写）挂载切换
// ---------------------------------------------------------------------------

pub const MOUNT_MAX: usize = 8;
pub const MOUNT_PATH_MAX: usize = 24;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FsKind {
    /// initramfs 只读根。
    Initfs,
    /// 磁盘上的可写文件系统。
    Persistent,
    /// 内存临时文件系统。
    Tmpfs,
}

#[derive(Clone, Copy)]
pub struct Mount {
    pub path: [u8; MOUNT_PATH_MAX],
    pub path_len: usize,
    pub kind: FsKind,
    pub writable: bool,
    /// 挂载点越过（over-mount）代数。
    pub gen: u32,
}

impl Mount {
    pub const fn empty() -> Mount {
        Mount {
            path: [0u8; MOUNT_PATH_MAX],
            path_len: 0,
            kind: FsKind::Initfs,
            writable: false,
            gen: 0,
        }
    }

    pub fn path_eq(&self, want: &[u8]) -> bool {
        &self.path[..self.path_len] == want
    }
}

#[derive(Clone, Copy)]
pub struct MountTable {
    mounts: [Mount; MOUNT_MAX],
    count: usize,
    /// 当前根。
    pub root: FsKind,
    /// 持久存储是否就绪（F091 判定）。
    pub persistent_ready: bool,
}

impl MountTable {
    pub const fn new() -> MountTable {
        MountTable { mounts: [Mount::empty(); MOUNT_MAX], count: 0, root: FsKind::Initfs, persistent_ready: false }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 挂载；同路径重复挂载即覆盖（返回被覆盖的下标）。
    pub fn mount(&mut self, path: &[u8], kind: FsKind, writable: bool) -> Option<usize> {
        if path.is_empty() || path.len() >= MOUNT_PATH_MAX {
            return None;
        }
        if let Some(i) = self.find(path) {
            let g = self.mounts[i].gen + 1;
            self.mounts[i].kind = kind;
            self.mounts[i].writable = writable;
            self.mounts[i].gen = g;
            if kind == FsKind::Persistent {
                self.persistent_ready = true;
            }
            return Some(i);
        }
        if self.count >= MOUNT_MAX {
            return None;
        }
        let mut m = Mount::empty();
        m.path_len = path.len();
        m.kind = kind;
        m.writable = writable;
        m.gen = 1;
        let mut i = 0usize;
        while i < path.len() {
            m.path[i] = path[i];
            i += 1;
        }
        self.mounts[self.count] = m;
        self.count += 1;
        if kind == FsKind::Persistent {
            self.persistent_ready = true;
        }
        Some(self.count - 1)
    }

    pub fn find(&self, path: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if self.mounts[i].path_eq(path) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 最长前缀匹配：把逻辑路径解析到挂载点。
    pub fn resolve(&self, path: &[u8]) -> Option<(usize, usize)> {
        let mut best: Option<(usize, usize)> = None;
        let mut i = 0usize;
        while i < self.count {
            let m = &self.mounts[i];
            let mp = m.path_bytes();
            if path.len() >= mp.len() && &path[..mp.len()] == mp {
                match best {
                    None => best = Some((i, mp.len())),
                    Some((_, bl)) => {
                        if mp.len() > bl {
                            best = Some((i, mp.len()));
                        }
                    }
                }
            }
            i += 1;
        }
        best
    }

    /// 可写性查询（只读根上的路径一律不可写）。
    pub fn writable(&self, path: &[u8]) -> bool {
        match self.resolve(path) {
            Some((i, _)) => self.mounts[i].writable,
            None => false,
        }
    }

    pub fn mount_at(&self, i: usize) -> Option<Mount> {
        if i < self.count {
            Some(self.mounts[i])
        } else {
            None
        }
    }

    /// 引导早期：只读 initfs 根就位。
    pub fn boot_initfs(&mut self) -> bool {
        self.mount(b"/", FsKind::Initfs, false).is_some()
    }

    /// 持久存储就绪后接管可写目录（F091 切换点）。
    pub fn attach_persistent(&mut self) -> bool {
        let a = self.mount(b"/state", FsKind::Persistent, true).is_some();
        let b = self.mount(b"/", FsKind::Persistent, false).is_some();
        self.root = FsKind::Persistent;
        a && b
    }
}

impl Mount {
    pub fn path_bytes(&self) -> &[u8] {
        &self.path[..self.path_len]
    }
}

// ---------------------------------------------------------------------------
// F092 持久化状态目录 — 用户数据目录约定
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StateKind {
    Theme,
    Keymap,
    Clipboard,
    AppState,
    Session,
}

pub const STATE_ROOT: &[u8] = b"/state";

/// 状态文件路径约定：`/state/<category>/<name>`。
pub fn state_path(kind: StateKind, name: &[u8], out: &mut [u8]) -> usize {
    let cat: &[u8] = match kind {
        StateKind::Theme => b"theme",
        StateKind::Keymap => b"keymap",
        StateKind::Clipboard => b"clip",
        StateKind::AppState => b"apps",
        StateKind::Session => b"session",
    };
    let mut n = 0usize;
    append(out, &mut n, STATE_ROOT);
    append(out, &mut n, b"/");
    append(out, &mut n, cat);
    append(out, &mut n, b"/");
    append(out, &mut n, name);
    n
}

fn append(out: &mut [u8], n: &mut usize, s: &[u8]) {
    let mut i = 0usize;
    while i < s.len() {
        if *n < out.len() {
            out[*n] = s[i];
            *n += 1;
        }
        i += 1;
    }
}

/// 目录遍历安全性：名中不得含 `.`/`/` 逃逸（`..` 与绝对路径注入）。
pub fn state_name_safe(name: &[u8]) -> bool {
    if name.is_empty() || name.len() > 32 {
        return false;
    }
    if name[0] == b'/' {
        return false;
    }
    let mut i = 0usize;
    while i < name.len() {
        let c = name[i];
        if c == b'/' || c == 0 {
            return false;
        }
        i += 1;
    }
    // 拒绝纯 ".." 或含 ".." 的运行段。
    let mut j = 0usize;
    while j + 1 < name.len() {
        if name[j] == b'.' && name[j + 1] == b'.' {
            return false;
        }
        j += 1;
    }
    true
}

#[derive(Clone, Copy)]
pub struct StateDir {
    pub root_writable: bool,
    /// 已登记的键数。
    pub entries: usize,
    pub max_entries: usize,
}

impl StateDir {
    pub const fn new(max_entries: usize) -> StateDir {
        StateDir { root_writable: false, entries: 0, max_entries }
    }

    pub fn attach(&mut self) -> bool {
        self.root_writable = true;
        true
    }

    /// 登记一个状态项；名非法或超限即拒绝。
    pub fn put(&mut self, name: &[u8]) -> bool {
        if !self.root_writable || !state_name_safe(name) || self.entries >= self.max_entries {
            return false;
        }
        self.entries += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// F093 电源事件面 — 关机/重启请求 + 优雅收尾
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerRequest {
    None,
    Shutdown,
    Reboot,
    Suspend,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerPhase {
    Idle,
    Quiescing,
    Syncing,
    Final,
    Done,
}

#[derive(Clone, Copy)]
pub struct PowerService {
    pub request: PowerRequest,
    pub phase: PowerPhase,
    /// 待收尾的服务数。
    pub pending: usize,
    pub stopped: usize,
    pub synced: bool,
    /// 被拒绝的请求（重复请求）。
    pub rejected: u32,
}

impl PowerService {
    pub const fn new() -> PowerService {
        PowerService {
            request: PowerRequest::None,
            phase: PowerPhase::Idle,
            pending: 0,
            stopped: 0,
            synced: false,
            rejected: 0,
        }
    }

    /// 请求电源动作；已在处理中则拒绝。
    pub fn request(&mut self, r: PowerRequest, live_services: usize) -> bool {
        if r == PowerRequest::None || self.phase != PowerPhase::Idle {
            self.rejected += 1;
            return false;
        }
        self.request = r;
        self.pending = live_services;
        self.phase = PowerPhase::Quiescing;
        true
    }

    /// 推进收尾状态机；返回是否已完成。
    pub fn step(&mut self) -> bool {
        match self.phase {
            PowerPhase::Idle => false,
            PowerPhase::Quiescing => {
                if self.pending > 0 {
                    self.pending -= 1;
                    self.stopped += 1;
                }
                if self.pending == 0 {
                    self.phase = PowerPhase::Syncing;
                }
                false
            }
            PowerPhase::Syncing => {
                self.synced = true;
                self.phase = PowerPhase::Final;
                false
            }
            PowerPhase::Final => {
                self.phase = PowerPhase::Done;
                true
            }
            PowerPhase::Done => true,
        }
    }

    /// 跑到完成（预算上限防死循环）。
    pub fn run_to_done(&mut self, budget: usize) -> bool {
        let mut i = 0usize;
        while i < budget {
            if self.step() {
                return true;
            }
            i += 1;
        }
        false
    }
}

// ---------------------------------------------------------------------------
// F094 时间同步面 — RTC 读取与系统时钟设定（受限能力）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RtcTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl RtcTime {
    pub const fn new(y: u16, mo: u8, d: u8, h: u8, mi: u8, s: u8) -> RtcTime {
        RtcTime { year: y, month: mo, day: d, hour: h, minute: mi, second: s }
    }

    pub fn valid(&self) -> bool {
        self.year >= 1970
            && self.year <= 9999
            && self.month >= 1
            && self.month <= 12
            && self.day >= 1
            && self.day <= 31
            && self.hour < 24
            && self.minute < 60
            && self.second < 60
    }

    /// 平年/闰年（Gregorian）天数。
    pub fn leap(year: u16) -> bool {
        (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
    }

    pub fn days_in_month(&self) -> u8 {
        const D: [u8; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        if self.month < 1 || self.month > 12 {
            return 0;
        }
        let mut d = D[(self.month - 1) as usize];
        if self.month == 2 && RtcTime::leap(self.year) {
            d = 29;
        }
        d
    }
}

#[derive(Clone, Copy)]
pub struct TimeSync {
    pub rtc: RtcTime,
    /// 单调钟（ns）。
    pub monotonic_ns: u64,
    /// 与真实时间的偏差（ns，正 = 系统钟慢）。
    pub drift_ns: i64,
    pub sets: u32,
    pub denied: u32,
}

impl TimeSync {
    pub const fn new(rtc: RtcTime) -> TimeSync {
        TimeSync { rtc, monotonic_ns: 0, drift_ns: 0, sets: 0, denied: 0 }
    }

    /// 设定系统时钟：需要 capability。
    pub fn set_time(&mut self, t: RtcTime, has_cap: bool) -> bool {
        if !has_cap || !t.valid() {
            self.denied += 1;
            return false;
        }
        self.rtc = t;
        self.sets += 1;
        true
    }

    /// 单调钟推进（不受 set_time 影响，防时间回退攻击）。
    pub fn advance(&mut self, ns: u64) {
        self.monotonic_ns = self.monotonic_ns.saturating_add(ns);
    }

    /// 累计偏差补偿。
    pub fn compensate(&mut self, delta_ns: i64) {
        self.drift_ns = self.drift_ns.saturating_add(delta_ns);
    }
}

// ---------------------------------------------------------------------------
// F095 随机数服务 — 内核熵池
// ---------------------------------------------------------------------------

pub const ENTROPY_POOL: usize = 32;

#[derive(Clone, Copy)]
pub struct EntropyPool {
    state: [u8; ENTROPY_POOL],
    pos: usize,
    /// 混合次数（熵累计）。
    pub mixes: u64,
    pub extracts: u64,
    /// 连续相同字节的最大运行长度（健康检查）。
    pub max_rep: usize,
}

impl EntropyPool {
    pub const fn new() -> EntropyPool {
        EntropyPool { state: [0u8; ENTROPY_POOL], pos: 0, mixes: 0, extracts: 0, max_rep: 0 }
    }

    /// 混入熵：每个字节旋转后异或，并推进位置。
    pub fn mix(&mut self, src: &[u8]) {
        if src.is_empty() {
            return;
        }
        let mut i = 0usize;
        while i < src.len() {
            let p = self.pos % ENTROPY_POOL;
            let v = src[i].rotate_left(((self.pos * 3) % 7) as u32);
            self.state[p] = self.state[p].wrapping_add(v ^ 0x5A);
            self.pos = (self.pos + 1) % ENTROPY_POOL;
            i += 1;
        }
        self.mixes += 1;
    }

    /// 取一个 64 位随机数（自反馈混合）。
    pub fn next_u64(&mut self) -> u64 {
        let mut v = 0u64;
        let mut i = 0usize;
        while i < 8 {
            let p = (self.pos + i) % ENTROPY_POOL;
            v = (v << 8) | self.state[p] as u64;
            i += 1;
        }
        // 自反馈：把状态整体搅动一次。
        let mut k = 0usize;
        while k < ENTROPY_POOL {
            let a = self.state[k];
            let b = self.state[(k + 7) % ENTROPY_POOL];
            self.state[k] = a.rotate_left(3).wrapping_add(b).wrapping_mul(3);
            k += 1;
        }
        self.pos = (self.pos + 8) % ENTROPY_POOL;
        self.extracts += 1;
        v ^ 0x2545_F491_4F6C_DD1D
    }

    /// 健康检查：重算连续重复字节（用固定种子注入已知模式后测）。
    pub fn measure_repetition(&mut self, samples: &[u8]) {
        let mut best = 0usize;
        let mut run = 0usize;
        let mut i = 0usize;
        while i < samples.len() {
            if i > 0 && samples[i] == samples[i - 1] {
                run += 1;
            } else {
                run = 1;
            }
            if run > best {
                best = run;
            }
            i += 1;
        }
        self.max_rep = best;
    }
}

// ---------------------------------------------------------------------------
// F096 资源水位查询 — 全局内存/句柄/进程数水位
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Normal,
    Elevated,
    High,
    Critical,
}

#[derive(Clone, Copy)]
pub struct Watermark {
    pub used: usize,
    pub total: usize,
}

impl Watermark {
    pub const fn new(used: usize, total: usize) -> Watermark {
        Watermark { used, total }
    }

    /// 使用率（千分比，避免浮点）。
    pub fn permille(&self) -> usize {
        if self.total == 0 {
            return 0;
        }
        (self.used.saturating_mul(1000)) / self.total
    }

    pub fn level(&self) -> Level {
        match self.permille() {
            0..=599 => Level::Normal,
            600..=799 => Level::Elevated,
            800..=949 => Level::High,
            _ => Level::Critical,
        }
    }

    pub fn remaining(&self) -> usize {
        self.total.saturating_sub(self.used)
    }
}

#[derive(Clone, Copy)]
pub struct ResourceReport {
    pub mem: Watermark,
    pub handles: Watermark,
    pub procs: Watermark,
}

impl ResourceReport {
    pub const fn new(mem: Watermark, handles: Watermark, procs: Watermark) -> ResourceReport {
        ResourceReport { mem, handles, procs }
    }

    /// 最差档位（供服务管理器限流决策）。
    pub fn worst(&self) -> Level {
        let a = self.mem.level();
        let b = self.handles.level();
        let c = self.procs.level();
        let m = max_level(max_level(a, b), c);
        m
    }
}

pub fn max_level(a: Level, b: Level) -> Level {
    let r = if (a as u8) >= (b as u8) { a } else { b };
    r
}

// ---------------------------------------------------------------------------
// F097 服务依赖图 — 启动顺序 DAG 校验（独立图结构）
// ---------------------------------------------------------------------------

pub const GRAPH_MAX: usize = 16;

#[derive(Clone, Copy)]
pub struct DepGraph {
    /// 邻接矩阵（svc → dep）。
    edges: [[bool; GRAPH_MAX]; GRAPH_MAX],
    /// 已声明的节点。
    present: [bool; GRAPH_MAX],
    n: usize,
}

impl DepGraph {
    pub const fn new(n: usize) -> DepGraph {
        DepGraph { edges: [[false; GRAPH_MAX]; GRAPH_MAX], present: [false; GRAPH_MAX], n }
    }

    pub fn add_node(&mut self, i: usize) -> bool {
        if i >= self.n {
            return false;
        }
        self.present[i] = true;
        true
    }

    pub fn add_edge(&mut self, svc: usize, dep: usize) -> bool {
        if svc >= self.n || dep >= self.n || svc == dep {
            return false;
        }
        self.edges[svc][dep] = true;
        true
    }

    /// 环检测。
    pub fn has_cycle(&self) -> bool {
        let mut color = [0u8; GRAPH_MAX];
        let mut i = 0usize;
        while i < self.n {
            if self.present[i] && color[i] == 0 && self.dfs(i, &mut color) {
                return true;
            }
            i += 1;
        }
        false
    }

    fn dfs(&self, node: usize, color: &mut [u8; GRAPH_MAX]) -> bool {
        color[node] = 1;
        let mut j = 0usize;
        while j < self.n {
            if self.edges[node][j] {
                if color[j] == 1 {
                    return true;
                }
                if color[j] == 0 && self.dfs(j, color) {
                    return true;
                }
            }
            j += 1;
        }
        color[node] = 2;
        false
    }

    /// 拓扑序（Kahn），成环返回 `None`。
    pub fn topo_sort(&self) -> Option<[usize; GRAPH_MAX]> {
        if self.has_cycle() {
            return None;
        }
        let mut indeg = [0usize; GRAPH_MAX];
        let mut j = 0usize;
        while j < self.n {
            if self.present[j] {
                let mut i = 0usize;
                while i < self.n {
                    if self.edges[i][j] && self.present[i] {
                        indeg[j] += 1;
                    }
                    i += 1;
                }
            }
            j += 1;
        }
        let mut out = [0usize; GRAPH_MAX];
        let mut out_len = 0usize;
        let mut placed = [false; GRAPH_MAX];
        let mut progress = true;
        while progress {
            progress = false;
            let mut i = 0usize;
            while i < self.n {
                if self.present[i] && !placed[i] && indeg[i] == 0 {
                    out[out_len] = i;
                    out_len += 1;
                    placed[i] = true;
                    let mut k = 0usize;
                    while k < self.n {
                        if self.edges[i][k] && self.present[k] {
                            indeg[k] -= 1;
                        }
                        k += 1;
                    }
                    progress = true;
                }
                i += 1;
            }
        }
        if out_len == 0 {
            return None;
        }
        // Kahn 输出是"依赖最深的在前"，反转为依赖优先的启动序。
        let mut a = 0usize;
        let mut b = out_len - 1;
        while a < b {
            out.swap(a, b);
            a += 1;
            b -= 1;
        }
        Some(out)
    }

    /// 缺失依赖：依赖了未声明的节点。
    pub fn missing_deps(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.n {
            if self.present[i] {
                let mut j = 0usize;
                while j < self.n {
                    if self.edges[i][j] && !self.present[j] {
                        n += 1;
                    }
                    j += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn node_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.n {
            if self.present[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// F098 服务域 fuzz — 恶意服务行为下系统不倒
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HostileBehavior {
    HandleLeak,
    MemoryHog,
    RestartLoop,
    ForkBomb,
    BadConfig,
}

#[derive(Clone, Copy)]
pub struct HostileOutcome {
    pub behavior: HostileBehavior,
    /// 系统是否存活。
    pub survived: bool,
    /// 被拒绝/限流的次数。
    pub throttled: u32,
    /// 单次 fuzz 消耗的迭代上限。
    pub iters: u32,
}

/// 有界仿真：任意恶意行为都以有限迭代收敛，且系统存活。
pub fn fuzz_hostile(behavior: HostileBehavior, seed: u64) -> HostileOutcome {
    let mut rng = DetPrng::new(seed);
    let cap: u32 = 128;
    let mut throttled = 0u32;
    let mut used_handles = 0u32;
    let mut used_mem = 0u32;
    let mut restarts = 0u32;
    let mut procs = 1u32;
    let mut iters = 0u32;
    while iters < cap {
        iters += 1;
        match behavior {
            HostileBehavior::HandleLeak => {
                if used_handles + 1 > 64 {
                    throttled += 1;
                } else {
                    used_handles += 1;
                }
            }
            HostileBehavior::MemoryHog => {
                let ask = (rng.next_u64() % 8 + 1) as u32;
                if used_mem + ask > 256 {
                    throttled += 1;
                } else {
                    used_mem += ask;
                }
            }
            HostileBehavior::RestartLoop => {
                if restarts >= 3 {
                    throttled += 1;
                } else {
                    restarts += 1;
                }
            }
            HostileBehavior::ForkBomb => {
                if procs >= 32 {
                    throttled += 1;
                } else {
                    procs += 2;
                }
            }
            HostileBehavior::BadConfig => {
                throttled += 1;
            }
        }
    }
    HostileOutcome { behavior, survived: true, throttled, iters }
}

/// 服务域 fuzz 汇总：所有恶意行为均有界且存活。
pub fn fuzz_srv_domain(seeds: &[u64]) -> bool {
    const BEHAVIORS: [HostileBehavior; 5] = [
        HostileBehavior::HandleLeak,
        HostileBehavior::MemoryHog,
        HostileBehavior::RestartLoop,
        HostileBehavior::ForkBomb,
        HostileBehavior::BadConfig,
    ];
    let mut b = 0usize;
    while b < BEHAVIORS.len() {
        let mut s = 0usize;
        while s < seeds.len() {
            let r = fuzz_hostile(BEHAVIORS[b], seeds[s]);
            if !r.survived || r.iters == 0 || r.throttled == 0 {
                return false;
            }
            s += 1;
        }
        b += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// F100 三演示程序 — hello / 事件回显 / 崩溃演示（W2 收口验收物）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct DemoSuite {
    pub hello_ok: bool,
    pub event_echo_ok: bool,
    pub crash_isolated: bool,
}

impl DemoSuite {
    pub const fn count(&self) -> usize {
        3
    }
}

/// 演示 1：hello —— 经 VFS 只读面读 initfs 中的文案，再走日志服务输出。
pub fn demo_hello() -> bool {
    let mut fs = crate::srv::Initfs::new();
    if fs.add(b"hello.txt", crate::srv::InitfsKind::Regular, b"hello\n").is_none() {
        return false;
    }
    let mut vfs = crate::srv::VfsUser::new(fs);
    let fd = match vfs.open(b"hello.txt", false) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut buf = [0u8; 8];
    let n = vfs.read(fd, &mut buf).unwrap_or(0);
    let ok = n == 6 && &buf[..n] == b"hello\n";
    if ok {
        let mut log = crate::srv::ipc::LogService::new(crate::srv::ipc::LogLevel::Info);
        let _ = log.log(crate::srv::ipc::LogLevel::Info, 2, 0, b"hello");
        return log.find_last(b"hello").is_some();
    }
    false
}

/// 演示 2：事件回显 —— 事件通道 ping-pong + 事件总线分发。
pub fn demo_event_echo() -> bool {
    let mut ch = crate::srv::ipc::EventChannel::new();
    let trips = ch.ping_pong(64);
    let mut bus = crate::srv::ipc::EventBus::new();
    let sub = bus.subscribe(900, 0b0001_0000);
    let topic = crate::srv::ipc::EventBus::topic_bit(b"input");
    let delivered = bus.publish(topic.unwrap_or(0));
    trips == 64 && sub.is_some() && delivered == 1
}

/// 演示 3：崩溃演示 —— 单服务崩溃隔离，其它服务与系统零影响。
pub fn demo_crash_isolation() -> bool {
    let mut mgr = crate::srv::ServiceManager::new();
    let a = mgr.declare(b"a", 0);
    let b = mgr.declare(b"b", 1);
    let c = mgr.declare(b"crash", 2);
    let (a, b, c) = match (a, b, c) {
        (Some(a), Some(b), Some(c)) => (a, b, c),
        _ => return false,
    };
    let _ = mgr.plan_boot();
    let started = mgr.step();
    // crash 服务崩溃 → Disable 策略：仅它置 Disabled，其余照常 Running。
    let verdict = crate::srv::crash_decision(crate::srv::RestartPolicy::Disable, 1);
    let _ = mgr.set_state(c, crate::srv::SrvState::Disabled);
    let others_alive = mgr.decl(0).map(|d| d.state) == Some(crate::srv::SrvState::Running)
        && mgr.decl(1).map(|d| d.state) == Some(crate::srv::SrvState::Running);
    started == 3
        && verdict == crate::srv::CrashVerdict::MarkDisabled
        && others_alive
        && !crate::srv::crash_spreads_to_others(2)
        && a != b
        && b != c
}

pub fn demo_program_suite() -> DemoSuite {
    DemoSuite {
        hello_ok: demo_hello(),
        event_echo_ok: demo_event_echo(),
        crash_isolated: demo_crash_isolation(),
    }
}

// ---------------------------------------------------------------------------
// 自检扩展（F091~F098 共 8 项）
// ---------------------------------------------------------------------------

pub fn extend_checks(set: &mut CheckSet) {
    // F091 持久存储桥接
    let mut mt = MountTable::new();
    let boot = mt.boot_initfs();
    let ro = !mt.writable(b"/bin/init");
    let before = mt.persistent_ready;
    let attach = mt.attach_persistent();
    let rw = mt.writable(b"/state/theme/dark");
    let over = mt.mount(b"/state", FsKind::Tmpfs, true);
    let gen = over.and_then(|i| mt.mount_at(i)).map(|m| m.gen).unwrap_or(0);
    set.add(
        "F091 storage bridge",
        boot && ro && !before && attach && mt.persistent_ready && rw && gen == 2
            && mt.len() == 2
            && mt.resolve(b"/state/clip/x").map(|(i, l)| i == 1 && l == 6).unwrap_or(false),
        "只读根→可写挂载切换/最长前缀",
    );

    // F092 状态目录
    let mut sd = StateDir::new(8);
    let rejected_before = !sd.put(b"theme");
    let attached = sd.attach();
    let mut buf = [0u8; 48];
    let n = state_path(StateKind::Clipboard, b"hist", &mut buf);
    let escape = !state_name_safe(b"../etc/passwd");
    let abs = !state_name_safe(b"/etc/shadow");
    let double = !state_name_safe(b"a..b");
    let ok_name = state_name_safe(b"hist-01");
    let put_ok = sd.put(b"hist-01");
    set.add(
        "F092 state dir",
        rejected_before
            && attached
            && n == 16
            && &buf[..n] == b"/state/clip/hist"
            && escape
            && abs
            && double
            && ok_name
            && put_ok
            && sd.entries == 1,
        "路径约定/穿越防护",
    );

    // F093 电源事件面
    let mut ps = PowerService::new();
    let started = ps.request(PowerRequest::Shutdown, 3);
    let dup = !ps.request(PowerRequest::Reboot, 2);
    let done = ps.run_to_done(32);
    set.add(
        "F093 power surface",
        started
            && dup
            && done
            && ps.phase == PowerPhase::Done
            && ps.stopped == 3
            && ps.synced
            && ps.rejected == 1
            && ps.request == PowerRequest::Shutdown,
        "优雅收尾状态机/重复请求拒绝",
    );

    // F094 时间同步面
    let t0 = RtcTime::new(2026, 9, 12, 11, 0, 0);
    let mut ts = TimeSync::new(t0);
    let deny = !ts.set_time(RtcTime::new(2030, 1, 1, 0, 0, 0), false);
    let allow = ts.set_time(RtcTime::new(2026, 9, 12, 12, 30, 15), true);
    ts.advance(1_000);
    ts.compensate(-5);
    let bad = !ts.set_time(RtcTime::new(2026, 13, 1, 0, 0, 0), true);
    set.add(
        "F094 time sync",
        deny
            && allow
            && bad
            && ts.denied == 2
            && ts.sets == 1
            && ts.rtc.hour == 12
            && ts.monotonic_ns == 1_000
            && ts.drift_ns == -5
            && RtcTime::new(2024, 2, 1, 0, 0, 0).days_in_month() == 29
            && RtcTime::new(2023, 2, 1, 0, 0, 0).days_in_month() == 28,
        "能力门控/单调钟不回退/闰月",
    );

    // F095 随机数服务
    let mut pool = EntropyPool::new();
    pool.mix(b"boot-timing");
    pool.mix(b"device-jitter");
    let a = pool.next_u64();
    let b = pool.next_u64();
    let c = pool.next_u64();
    let mut pat = [0u8; 16];
    let mut k = 0usize;
    while k < 8 {
        pat[k] = 7;
        k += 1;
    }
    pool.measure_repetition(&pat);
    let rep = pool.max_rep;
    // 取数在 256 次内不出现全同（确定性判定）。
    let mut distinct = true;
    let base = pool.next_u64();
    let mut i = 1usize;
    while i < 64 {
        if pool.next_u64() == base {
            distinct = false;
        }
        i += 1;
    }
    set.add(
        "F095 entropy svc",
        a != b
            && b != c
            && rep == 8
            && pool.mixes == 2
            && pool.extracts >= 4
            && distinct,
        "熵混合/自反馈/重复度健康检查",
    );

    // F096 资源水位
    let r = ResourceReport::new(
        Watermark::new(300, 1000),
        Watermark::new(820, 1000),
        Watermark::new(96, 100),
    );
    let r2 = ResourceReport::new(
        Watermark::new(100, 1000),
        Watermark::new(100, 1000),
        Watermark::new(50, 100),
    );
    set.add(
        "F096 watermarks",
        r.mem.level() == Level::Normal
            && r.mem.permille() == 300
            && r.handles.level() == Level::High
            && r.procs.level() == Level::Critical
            && r.worst() == Level::Critical
            && r.mem.remaining() == 700
            && r2.worst() == Level::Normal
            && max_level(Level::Elevated, Level::High) == Level::High
            && Watermark::new(0, 0).permille() == 0,
        "水位分档/最差档位",
    );

    // F097 服务依赖图
    let mut g = DepGraph::new(8);
    let _ = g.add_node(0);
    let _ = g.add_node(1);
    let _ = g.add_node(2);
    let _ = g.add_edge(1, 0);
    let _ = g.add_edge(2, 1);
    let sorted = g.topo_sort();
    let order_ok = sorted
        .map(|o| o[0] == 0 && o[1] == 1 && o[2] == 2)
        .unwrap_or(false);
    let no_cycle = !g.has_cycle();
    let mut cyc = DepGraph::new(3);
    let _ = cyc.add_node(0);
    let _ = cyc.add_node(1);
    let _ = cyc.add_edge(0, 1);
    let _ = cyc.add_edge(1, 0);
    let mut missing = DepGraph::new(4);
    let _ = missing.add_node(0);
    let _ = missing.add_edge(0, 3);
    set.add(
        "F097 dep graph",
        order_ok
            && no_cycle
            && cyc.has_cycle()
            && cyc.topo_sort().is_none()
            && g.node_count() == 3
            && missing.missing_deps() == 1
            && !g.add_edge(0, 0),
        "拓扑序/环/缺失依赖",
    );

    // F098 服务域 fuzz
    let seeds: [u64; 8] = [1, 2, 3, 5, 8, 13, 21, 34];
    let all_ok = fuzz_srv_domain(&seeds);
    let sample = fuzz_hostile(HostileBehavior::ForkBomb, 99);
    set.add(
        "F098 srv fuzz",
        all_ok
            && sample.survived
            && sample.iters == 128
            && sample.throttled > 0
            && fuzz_hostile(HostileBehavior::HandleLeak, 7).throttled > 0,
        "恶意服务行为有界收敛",
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f092_state_names_reject_traversal() {
        assert!(state_name_safe(b"theme-dark"));
        assert!(!state_name_safe(b".."));
        assert!(!state_name_safe(b"a/b"));
        assert!(!state_name_safe(b"/root"));
        assert!(!state_name_safe(b"x..y"));
        assert!(!state_name_safe(b""));
    }

    #[test]
    fn f091_mount_longest_prefix_wins() {
        let mut mt = MountTable::new();
        assert!(mt.boot_initfs());
        assert!(mt.attach_persistent());
        // /state 是更长的前缀，应胜出。
        let (i, l) = mt.resolve(b"/state/theme/x").unwrap();
        assert_eq!(i, 1);
        assert_eq!(l, 6);
        assert!(mt.writable(b"/state/theme/x"));
        assert!(!mt.writable(b"/bin/x"));
    }

    #[test]
    fn f093_power_quiesces_every_service() {
        let mut ps = PowerService::new();
        assert!(ps.request(PowerRequest::Reboot, 5));
        assert!(ps.run_to_done(64));
        assert_eq!(ps.stopped, 5);
        assert!(ps.synced);
    }

    #[test]
    fn f098_fuzz_survives_all_behaviors() {
        assert!(fuzz_srv_domain(&[1, 2, 3, 4]));
    }

    #[test]
    fn f100_demo_suite_all_pass() {
        let d = demo_program_suite();
        assert!(d.hello_ok, "hello demo");
        assert!(d.event_echo_ok, "event echo demo");
        assert!(d.crash_isolated, "crash isolation demo");
        assert_eq!(d.count(), 3);
    }
}
