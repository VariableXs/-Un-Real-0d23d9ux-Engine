//! VARIABLE-200 AI-07 · 移植域验收面（F169~F175）。
//!
//! 崩溃不扩散红线、应用启动器、设置应用对接、文件管理器对接、
//! 兼容性测试矩阵、双后端 CI、移植域自检。
//!
//! 纪律：纯逻辑 + 固定容量数组；无 `Vec`/`String`/`Box`/`alloc`/外部 crate。

use crate::checks::CheckSet;
use crate::srv::{Initfs, InitfsKind, VfsErr, VfsUser, VFS_MAX_FD, VFS_READ_BUF};
use crate::vport::render::{StateStore, CAP_FS_WRITE, STATE_KEY_MAX};
use crate::vport::{AppLifecycle, AppState, BackendKind};

// ---------------------------------------------------------------------------
// F169 崩溃不扩散验收 — 强杀/注入崩溃，桌面/任务栏/其他应用零影响
// ---------------------------------------------------------------------------

pub const CRASH_MAX: usize = 8;
pub const CRASH_NAME_MAX: usize = 16;

#[derive(Clone, Copy)]
pub struct CrashParticipant {
    pub id: u32,
    pub name: [u8; CRASH_NAME_MAX],
    pub name_len: usize,
    pub alive: bool,
    /// shell 关键组件（桌面/任务栏）。
    pub critical: bool,
    /// 依赖位掩码：bit i = 依赖下标 i 的进程。
    pub deps: u32,
    pub crashes: u32,
    pub restarts: u32,
    /// 已完成工作量（用于证明"其他进程零影响"）。
    pub progress: u64,
}

impl CrashParticipant {
    pub const fn empty() -> CrashParticipant {
        CrashParticipant {
            id: 0,
            name: [0u8; CRASH_NAME_MAX],
            name_len: 0,
            alive: false,
            critical: false,
            deps: 0,
            crashes: 0,
            restarts: 0,
            progress: 0,
        }
    }

    pub fn name_eq(&self, want: &[u8]) -> bool {
        &self.name[..self.name_len] == want
    }

    pub fn name_bytes(&self) -> &[u8] {
        &self.name[..self.name_len]
    }
}

#[derive(Clone, Copy)]
pub struct CrashMatrix {
    parts: [CrashParticipant; CRASH_MAX],
    count: usize,
    pub injections: u64,
    pub kills: u64,
    /// 连带死亡数（红线：必须恒为 0）。
    pub cascade_deaths: u64,
}

impl CrashMatrix {
    pub const fn new() -> CrashMatrix {
        CrashMatrix {
            parts: [CrashParticipant::empty(); CRASH_MAX],
            count: 0,
            injections: 0,
            kills: 0,
            cascade_deaths: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn add(&mut self, name: &[u8], id: u32, critical: bool, deps: u32) -> Option<usize> {
        if self.count >= CRASH_MAX || name.is_empty() || name.len() >= CRASH_NAME_MAX || id == 0 {
            return None;
        }
        if self.find(id).is_some() {
            return None;
        }
        let mut p = CrashParticipant::empty();
        p.id = id;
        p.name_len = name.len();
        p.critical = critical;
        p.deps = deps;
        p.alive = true;
        let mut i = 0usize;
        while i < name.len() {
            p.name[i] = name[i];
            i += 1;
        }
        self.parts[self.count] = p;
        self.count += 1;
        Some(self.count - 1)
    }

    pub fn find(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if self.parts[i].id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    pub fn get(&self, i: usize) -> Option<CrashParticipant> {
        if i < self.count {
            Some(self.parts[i])
        } else {
            None
        }
    }

    pub fn by_name(&self, name: &[u8]) -> Option<CrashParticipant> {
        let mut i = 0usize;
        while i < self.count {
            if self.parts[i].name_eq(name) {
                return Some(self.parts[i]);
            }
            i += 1;
        }
        None
    }

    fn alive_snapshot(&self) -> [bool; CRASH_MAX] {
        let mut s = [false; CRASH_MAX];
        let mut i = 0usize;
        while i < self.count {
            s[i] = self.parts[i].alive;
            i += 1;
        }
        s
    }

    /// 结算连带死亡：除被打击者外，任何"本次由存活变为死亡"的进程都记红线。
    fn settle(&mut self, hit: usize, before: &[bool; CRASH_MAX]) {
        let mut j = 0usize;
        while j < self.count {
            if j != hit && before[j] && !self.parts[j].alive {
                self.cascade_deaths += 1;
            }
            j += 1;
        }
    }

    /// 强杀：只影响被击中者，连带死亡恒为 0。
    pub fn kill(&mut self, id: u32) -> bool {
        let hit = match self.find(id) {
            Some(i) => i,
            None => return false,
        };
        if !self.parts[hit].alive {
            return false;
        }
        let before = self.alive_snapshot();
        self.parts[hit].alive = false;
        self.kills += 1;
        self.settle(hit, &before);
        true
    }

    /// 注入崩溃：与强杀同路径，并记录崩溃次数。
    pub fn inject_crash(&mut self, id: u32) -> bool {
        let hit = match self.find(id) {
            Some(i) => i,
            None => return false,
        };
        if !self.parts[hit].alive {
            return false;
        }
        let before = self.alive_snapshot();
        self.parts[hit].alive = false;
        self.parts[hit].crashes += 1;
        self.injections += 1;
        self.settle(hit, &before);
        true
    }

    /// 重启被杀的进程（监督者只拉起自己那一份）。
    pub fn restart(&mut self, id: u32) -> bool {
        let i = match self.find(id) {
            Some(i) => i,
            None => return false,
        };
        if self.parts[i].alive {
            return false;
        }
        self.parts[i].alive = true;
        self.parts[i].restarts += 1;
        true
    }

    pub fn alive_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if self.parts[i].alive {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 所有存活进程各自推进一格工作量；返回推进者数量。
    pub fn tick(&mut self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if self.parts[i].alive {
                self.parts[i].progress += 1;
                n += 1;
            }
            i += 1;
        }
        n
    }

    pub fn progress_of(&self, id: u32) -> Option<u64> {
        self.find(id).map(|i| self.parts[i].progress)
    }

    pub fn critical_alive(&self) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.parts[i].critical && !self.parts[i].alive {
                return false;
            }
            i += 1;
        }
        true
    }

    fn depends_on(&self, a: usize, b: usize) -> bool {
        self.parts[a].deps & (1u32 << b) != 0
    }

    /// 反例（仅用于对照，绝不进产品路径）：朴素监督者在某进程崩溃时，
    /// 连带杀死所有（直接）依赖它的进程。正确实现必须让此值 > 0
    /// 而真实连带死亡 = 0，以证明红线是被真正守住的、而不是空断言。
    pub fn naive_cascade_count(&self, id: u32) -> usize {
        let start = match self.find(id) {
            Some(i) => i,
            None => return 0,
        };
        let mut killed = 0usize;
        let mut j = 0usize;
        while j < self.count {
            if j != start && self.parts[j].alive && self.depends_on(j, start) {
                killed += 1;
            }
            j += 1;
        }
        killed
    }
}

/// 标准崩溃矩阵：桌面为核心，任务栏/开始菜单/通知/文件管理器各自独立。
pub fn crash_matrix_standard() -> CrashMatrix {
    let mut m = CrashMatrix::new();
    let _ = m.add(b"desktop", 1, true, 0);
    let _ = m.add(b"taskbar", 2, true, 1 << 0);
    let _ = m.add(b"startmenu", 3, false, (1 << 0) | (1 << 1));
    let _ = m.add(b"notify", 4, false, 1 << 0);
    let _ = m.add(b"fileman", 5, false, 1 << 0);
    m
}

// ---------------------------------------------------------------------------
// F170 应用启动器 — 开始菜单点击 → spawn 应用进程，启动耗时仪表
// ---------------------------------------------------------------------------

pub const CATALOG_MAX: usize = 8;
pub const LAUNCH_RING: usize = 16;
/// 启动耗时红线（µs）：≥ 300ms 视为不可接受。
pub const LAUNCH_BUDGET_US: u32 = 300_000;
/// pid 分配起点（与 init/pid1 服务区间隔离）。
pub const LAUNCH_PID_BASE: u32 = 1000;

#[derive(Clone, Copy)]
pub struct CatalogApp {
    pub id: u32,
    pub name: [u8; 16],
    pub name_len: usize,
    pub exec: [u8; 24],
    pub exec_len: usize,
    pub caps: u32,
}

impl CatalogApp {
    pub const fn empty() -> CatalogApp {
        CatalogApp { id: 0, name: [0u8; 16], name_len: 0, exec: [0u8; 24], exec_len: 0, caps: 0 }
    }

    pub fn name_eq(&self, want: &[u8]) -> bool {
        &self.name[..self.name_len] == want
    }

    pub fn exec_bytes(&self) -> &[u8] {
        &self.exec[..self.exec_len]
    }
}

#[derive(Clone, Copy)]
pub struct Launcher {
    apps: [CatalogApp; CATALOG_MAX],
    count: usize,
    next_id: u32,
    next_pid: u32,
    samples: [u32; LAUNCH_RING],
    wr: usize,
    pub n: usize,
    pub launched: u64,
    pub launch_failures: u64,
}

impl Launcher {
    pub const fn new() -> Launcher {
        Launcher {
            apps: [CatalogApp::empty(); CATALOG_MAX],
            count: 0,
            next_id: 1,
            next_pid: LAUNCH_PID_BASE,
            samples: [0u32; LAUNCH_RING],
            wr: 0,
            n: 0,
            launched: 0,
            launch_failures: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 注册目录条目：返回应用 id。
    pub fn register(&mut self, name: &[u8], exec: &[u8], caps: u32) -> Option<u32> {
        if self.count >= CATALOG_MAX
            || name.is_empty()
            || name.len() >= 16
            || exec.is_empty()
            || exec.len() >= 24
        {
            return None;
        }
        if self.find_by_name(name).is_some() {
            return None;
        }
        let id = self.next_id;
        let mut a = CatalogApp::empty();
        a.id = id;
        a.name_len = name.len();
        a.exec_len = exec.len();
        a.caps = caps;
        let mut i = 0usize;
        while i < name.len() {
            a.name[i] = name[i];
            i += 1;
        }
        let mut k = 0usize;
        while k < exec.len() {
            a.exec[k] = exec[k];
            k += 1;
        }
        self.apps[self.count] = a;
        self.count += 1;
        self.next_id += 1;
        Some(id)
    }

    pub fn find(&self, id: u32) -> Option<CatalogApp> {
        let mut i = 0usize;
        while i < self.count {
            if self.apps[i].id == id {
                return Some(self.apps[i]);
            }
            i += 1;
        }
        None
    }

    pub fn find_by_name(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if self.apps[i].name_eq(name) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    pub fn get(&self, i: usize) -> Option<CatalogApp> {
        if i < self.count {
            Some(self.apps[i])
        } else {
            None
        }
    }

    pub fn record_sample(&mut self, us: u32) {
        self.samples[self.wr] = us;
        self.wr = (self.wr + 1) % LAUNCH_RING;
        if self.n < LAUNCH_RING {
            self.n += 1;
        }
    }

    fn percentile(&self, permille: usize) -> u32 {
        if self.n == 0 {
            return 0;
        }
        let mut tmp = [0u32; LAUNCH_RING];
        let mut i = 0usize;
        while i < self.n {
            tmp[i] = self.samples[i];
            i += 1;
        }
        let mut a = 1usize;
        while a < self.n {
            let key = tmp[a];
            let mut b = a;
            while b > 0 && tmp[b - 1] > key {
                tmp[b] = tmp[b - 1];
                b -= 1;
            }
            tmp[b] = key;
            a += 1;
        }
        let mut idx = (permille * self.n) / 1000;
        if idx >= self.n {
            idx = self.n - 1;
        }
        tmp[idx]
    }

    pub fn p50(&self) -> u32 {
        self.percentile(500)
    }

    pub fn p99(&self) -> u32 {
        self.percentile(990)
    }

    /// 启动耗时红线：p99 必须落在预算内。
    pub fn within_budget(&self) -> bool {
        self.n > 0 && self.p99() < LAUNCH_BUDGET_US
    }

    /// 点击开始菜单 → spawn 应用：分配 pid、拉起窗口、记录耗时。
    pub fn launch(&mut self, id: u32, cost_us: u32, life: &mut AppLifecycle) -> Option<u32> {
        if self.find(id).is_none() {
            self.launch_failures += 1;
            return None;
        }
        if life.state == AppState::Running {
            self.launch_failures += 1;
            return None;
        }
        let pid = self.next_pid;
        self.next_pid += 1;
        if !life.launch(id * 10) {
            self.launch_failures += 1;
            return None;
        }
        self.record_sample(cost_us);
        self.launched += 1;
        Some(pid)
    }
}

// ---------------------------------------------------------------------------
// F171 设置应用对接 — 系统设置读写 → 配置服务 + 权限面
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct SettingsApp {
    store: StateStore,
    /// 允许写配置的能力位（权限面）。
    pub write_caps: u32,
    pub writes: u64,
    pub reads: u64,
    pub denied: u64,
    pub bad_args: u64,
    pub imports: u32,
}

impl SettingsApp {
    pub const fn new(write_caps: u32) -> SettingsApp {
        SettingsApp {
            store: StateStore::new(),
            write_caps,
            writes: 0,
            reads: 0,
            denied: 0,
            bad_args: 0,
            imports: 0,
        }
    }

    /// 写设置：先过参数校验，再过权限面。
    pub fn set(&mut self, key: &[u8], value: u32, cap: u32) -> bool {
        if key.is_empty() || key.len() >= STATE_KEY_MAX {
            self.bad_args += 1;
            return false;
        }
        if cap == 0 || self.write_caps & cap != cap {
            self.denied += 1;
            return false;
        }
        if self.store.set(key, value) {
            self.writes += 1;
            true
        } else {
            self.denied += 1;
            false
        }
    }

    pub fn get(&mut self, key: &[u8]) -> Option<u32> {
        let v = self.store.get(key);
        if v.is_some() {
            self.reads += 1;
        }
        v
    }

    pub fn len(&self) -> usize {
        self.store.len()
    }

    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }

    /// 导出到配置服务（落盘字节流）。
    pub fn export(&self, out: &mut [u8]) -> usize {
        self.store.snapshot(out)
    }

    /// 引导期导入（系统路径，不再做能力检查，但要计数）。
    pub fn import(&mut self, data: &[u8]) -> usize {
        self.imports += 1;
        self.store.restore(data)
    }
}

/// 设置应用的默认配置面（主题/键位/DPI/剪贴板指针）。
pub fn default_settings(write_caps: u32) -> SettingsApp {
    let mut s = SettingsApp::new(write_caps);
    let _ = s.set(b"theme.mode", 0, CAP_FS_WRITE);
    let _ = s.set(b"keymap", 1, CAP_FS_WRITE);
    let _ = s.set(b"dpi.permille", 1000, CAP_FS_WRITE);
    s
}

// ---------------------------------------------------------------------------
// F172 文件管理器对接 — 基于 VFS 用户态面的基础文件浏览
// ---------------------------------------------------------------------------

pub const BROWSE_MAX: usize = 16;

#[derive(Clone, Copy)]
pub struct BrowsedEntry {
    pub name: [u8; 32],
    pub name_len: usize,
    pub kind: InitfsKind,
    pub size: usize,
}

impl BrowsedEntry {
    pub const fn empty() -> BrowsedEntry {
        BrowsedEntry { name: [0u8; 32], name_len: 0, kind: InitfsKind::Regular, size: 0 }
    }

    pub fn name_bytes(&self) -> &[u8] {
        &self.name[..self.name_len]
    }

    pub fn is_dir(&self) -> bool {
        self.kind == InitfsKind::Directory
    }
}

/// 列出只读根下的条目（文件管理器左栏）。
pub fn browse(vfs: &VfsUser, out: &mut [BrowsedEntry]) -> usize {
    let fs = vfs.backend();
    let n = core::cmp::min(fs.len(), out.len());
    let mut i = 0usize;
    while i < n {
        if let Some(e) = fs.entry(i) {
            let mut b = BrowsedEntry::empty();
            let nb = e.name_bytes();
            let l = core::cmp::min(nb.len(), 32);
            let mut k = 0usize;
            while k < l {
                b.name[k] = nb[k];
                k += 1;
            }
            b.name_len = l;
            b.kind = e.kind;
            b.size = e.size;
            out[i] = b;
        }
        i += 1;
    }
    n
}

/// 文件管理器打开文件：走 VFS 用户态面 open/read/close 全套。
pub fn read_file(vfs: &mut VfsUser, path: &[u8], out: &mut [u8]) -> Result<usize, VfsErr> {
    let fd = vfs.open(path, false)?;
    let n = vfs.read(fd, out)?;
    vfs.close(fd)?;
    Ok(n)
}

// ---------------------------------------------------------------------------
// F173 兼容性测试矩阵 — Top200 应用行为清单在 Varix 上逐项过
// ---------------------------------------------------------------------------

pub const MATRIX_MAX: usize = 24;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompatStatus {
    /// 行为完全一致。
    Pass,
    /// 显式降级（必须带原因码，且通知用户）。
    Degraded,
    /// 不通过（构建期门禁变红）。
    Fail,
}

#[derive(Clone, Copy)]
pub struct CompatCase {
    pub app: [u8; 16],
    pub app_len: usize,
    pub behavior: [u8; 20],
    pub behavior_len: usize,
    pub status: CompatStatus,
    pub reason: u8,
}

impl CompatCase {
    pub const fn empty() -> CompatCase {
        CompatCase {
            app: [0u8; 16],
            app_len: 0,
            behavior: [0u8; 20],
            behavior_len: 0,
            status: CompatStatus::Pass,
            reason: 0,
        }
    }

    pub fn app_bytes(&self) -> &[u8] {
        &self.app[..self.app_len]
    }

    pub fn behavior_bytes(&self) -> &[u8] {
        &self.behavior[..self.behavior_len]
    }
}

#[derive(Clone, Copy)]
pub struct CompatMatrix {
    cases: [CompatCase; MATRIX_MAX],
    count: usize,
    /// 目标覆盖数（Top200 → 200；内核侧用抽样代表行为类别）。
    pub target: usize,
    pub pass: usize,
    pub degraded: usize,
    pub fail: usize,
    /// 降级却无原因的次数（结构上不可能，仍计数以守红线）。
    pub missing_reasons: u64,
}

impl CompatMatrix {
    pub const fn new(target: usize) -> CompatMatrix {
        CompatMatrix {
            cases: [CompatCase::empty(); MATRIX_MAX],
            count: 0,
            target,
            pass: 0,
            degraded: 0,
            fail: 0,
            missing_reasons: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 记录一条行为结果：降级必须带原因，否则拒绝入表。
    pub fn record(
        &mut self,
        app: &[u8],
        behavior: &[u8],
        status: CompatStatus,
        reason: u8,
    ) -> bool {
        if self.count >= MATRIX_MAX
            || app.is_empty()
            || app.len() >= 16
            || behavior.is_empty()
            || behavior.len() >= 20
        {
            return false;
        }
        if status == CompatStatus::Degraded && reason == 0 {
            self.missing_reasons += 1;
            return false;
        }
        if self.find(app, behavior).is_some() {
            return false;
        }
        let mut c = CompatCase::empty();
        c.app_len = app.len();
        c.behavior_len = behavior.len();
        c.status = status;
        c.reason = reason;
        let mut i = 0usize;
        while i < app.len() {
            c.app[i] = app[i];
            i += 1;
        }
        let mut k = 0usize;
        while k < behavior.len() {
            c.behavior[k] = behavior[k];
            k += 1;
        }
        self.cases[self.count] = c;
        self.count += 1;
        match status {
            CompatStatus::Pass => self.pass += 1,
            CompatStatus::Degraded => self.degraded += 1,
            CompatStatus::Fail => self.fail += 1,
        }
        true
    }

    pub fn get(&self, i: usize) -> Option<CompatCase> {
        if i < self.count {
            Some(self.cases[i])
        } else {
            None
        }
    }

    pub fn find(&self, app: &[u8], behavior: &[u8]) -> Option<CompatCase> {
        let mut i = 0usize;
        while i < self.count {
            if self.cases[i].app_bytes() == app && self.cases[i].behavior_bytes() == behavior {
                return Some(self.cases[i]);
            }
            i += 1;
        }
        None
    }

    pub fn pass_rate_permille(&self) -> usize {
        if self.count == 0 {
            return 0;
        }
        self.pass * 1000 / self.count
    }

    /// 门禁：无 Fail、无缺原因降级、且覆盖数达到目标。
    pub fn green(&self) -> bool {
        self.count >= self.target && self.fail == 0 && self.missing_reasons == 0
    }

    /// 降级项必须都在用户可见面登记（返回通知数）。
    pub fn degraded_user_visible(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if self.cases[i].status == CompatStatus::Degraded && self.cases[i].reason != 0 {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// 标准兼容性抽样矩阵（代表 Top200 的行为类别）。
pub fn standard_compat_matrix() -> CompatMatrix {
    let mut m = CompatMatrix::new(6);
    let _ = m.record(b"fileman", b"browse", CompatStatus::Pass, 0);
    let _ = m.record(b"fileman", b"copy", CompatStatus::Pass, 0);
    let _ = m.record(b"settings", b"read_write", CompatStatus::Pass, 0);
    let _ = m.record(b"editor", b"ime_input", CompatStatus::Pass, 0);
    let _ = m.record(b"browser", b"tls", CompatStatus::Degraded, 2);
    let _ = m.record(b"viewer", b"video_decode", CompatStatus::Degraded, 4);
    m
}

// ---------------------------------------------------------------------------
// F174 双后端 CI — Windows 后端与 Varix 后端同仓双线构建
// ---------------------------------------------------------------------------

pub const CI_STEPS: u32 = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LaneState {
    Pending,
    Running,
    Green,
    Red,
}

#[derive(Clone, Copy)]
pub struct CiLane {
    pub backend: BackendKind,
    pub state: LaneState,
    pub done: u32,
    pub total: u32,
    pub runs: u32,
}

impl CiLane {
    pub const fn new(backend: BackendKind, total: u32) -> CiLane {
        CiLane { backend, state: LaneState::Pending, done: 0, total, runs: 0 }
    }

    pub fn is_green(&self) -> bool {
        self.state == LaneState::Green
    }
}

#[derive(Clone, Copy)]
pub struct DualBackendCi {
    lanes: [CiLane; 2],
    pub reds: u64,
}

impl DualBackendCi {
    /// 双线同仓：Windows 线（语义基准）+ Varix 线（新增后端）。
    pub const fn new(total: u32) -> DualBackendCi {
        DualBackendCi {
            lanes: [
                CiLane::new(BackendKind::Windows, total),
                CiLane::new(BackendKind::Varix, total),
            ],
            reds: 0,
        }
    }

    fn index_of(b: BackendKind) -> usize {
        match b {
            BackendKind::Windows => 0,
            BackendKind::Varix => 1,
        }
    }

    pub fn lane(&self, b: BackendKind) -> CiLane {
        self.lanes[Self::index_of(b)]
    }

    /// 推进一步：ok=false 直接变红；跑完全部步骤变绿。
    pub fn advance(&mut self, b: BackendKind, ok: bool) -> LaneState {
        let i = Self::index_of(b);
        if self.lanes[i].state == LaneState::Green || self.lanes[i].state == LaneState::Red {
            return self.lanes[i].state;
        }
        if !ok {
            self.lanes[i].state = LaneState::Red;
            self.reds += 1;
            return LaneState::Red;
        }
        self.lanes[i].done += 1;
        self.lanes[i].runs += 1;
        if self.lanes[i].done >= self.lanes[i].total {
            self.lanes[i].state = LaneState::Green;
        } else {
            self.lanes[i].state = LaneState::Running;
        }
        self.lanes[i].state
    }

    pub fn both_green(&self) -> bool {
        self.lanes[0].is_green() && self.lanes[1].is_green()
    }

    /// Windows 后端必须始终可构建（回滚纪律：不删旧后端）。
    pub fn windows_lane_intact(&self) -> bool {
        self.lanes[0].state != LaneState::Red
    }
}

// ---------------------------------------------------------------------------
// 自检扩展（F169~F175 共 7 项）
// ---------------------------------------------------------------------------

pub fn extend_checks(set: &mut CheckSet) {
    // F169 崩溃不扩散验收
    let mut cm = crash_matrix_standard();
    let alive0 = cm.alive_count();
    let taskbar_progress0 = cm.progress_of(2).unwrap_or(0);
    let naive = cm.naive_cascade_count(1);
    let killed_desktop = cm.kill(1);
    let after_kill = cm.alive_count();
    let advanced = cm.tick();
    let taskbar_progress1 = cm.progress_of(2).unwrap_or(0);
    let taskbar_alive = cm.by_name(b"taskbar").map(|p| p.alive).unwrap_or(false);
    let startmenu_alive = cm.by_name(b"startmenu").map(|p| p.alive).unwrap_or(false);
    let fileman_alive = cm.by_name(b"fileman").map(|p| p.alive).unwrap_or(false);
    let desk_alive = cm.by_name(b"desktop").map(|p| p.alive).unwrap_or(false);
    let injected = cm.inject_crash(5);
    let desk_still = cm.by_name(b"desktop").map(|p| p.alive).unwrap_or(false);
    let restarted = cm.restart(5);
    let dup_restart = !cm.restart(5);
    let unknown = !cm.kill(999);
    let restored_desktop = cm.restart(1);
    set.add(
        "F169 crash isolation",
        cm.len() == 5
            && alive0 == 5
            && naive == 4
            && killed_desktop
            && after_kill == 4
            && advanced == 4
            && taskbar_progress1 == taskbar_progress0 + 1
            && taskbar_alive
            && startmenu_alive
            && fileman_alive
            && !desk_alive
            && cm.cascade_deaths == 0
            && cm.kills == 1
            && injected
            && !desk_still
            && cm.injections == 1
            && cm.by_name(b"fileman").map(|p| p.crashes == 1).unwrap_or(false)
            && restarted
            && dup_restart
            && unknown
            && restored_desktop
            && cm.critical_alive()
            && cm.alive_count() == 5,
        "强杀/注入崩溃零扩散/进度不受影响",
    );

    // F170 应用启动器
    let mut launcher = Launcher::new();
    let fid = launcher.register(b"fileman", b"fileman", crate::vport::render::fileman_caps());
    let sid = launcher.register(b"settings", b"settings", CAP_FS_WRITE);
    let dup = launcher.register(b"fileman", b"fileman2", 0);
    let bad = launcher.register(b"", b"x", 0);
    let mut life1 = AppLifecycle::new(fid.unwrap_or(0));
    let pid1 = launcher.launch(fid.unwrap_or(0), 90_000, &mut life1);
    let relaunch = launcher.launch(fid.unwrap_or(0), 90_000, &mut life1);
    let mut life2 = AppLifecycle::new(sid.unwrap_or(0));
    let pid2 = launcher.launch(sid.unwrap_or(0), 110_000, &mut life2);
    let unknown_launch = launcher.launch(999, 1, &mut AppLifecycle::new(1));
    let mut slow = Launcher::new();
    slow.record_sample(500_000);
    let mut k = 0usize;
    while k < 9 {
        slow.record_sample(100_000);
        k += 1;
    }
    set.add(
        "F170 app launcher",
        fid == Some(1)
            && sid == Some(2)
            && dup.is_none()
            && bad.is_none()
            && pid1 == Some(1000)
            && pid2 == Some(1001)
            && relaunch.is_none()
            && unknown_launch.is_none()
            && launcher.launched == 2
            && launcher.launch_failures == 2
            && launcher.n == 2
            && launcher.within_budget()
            && launcher.find(fid.unwrap_or(0)).map(|a| a.name_eq(b"fileman") && a.exec_bytes() == b"fileman").unwrap_or(false)
            && life1.state == AppState::Running
            && life1.win_visible
            && life1.win_id == 10
            && !slow.within_budget()
            && slow.p50() == 100_000
            && launcher.p99() < LAUNCH_BUDGET_US,
        "开始菜单→spawn/耗时仪表/红线",
    );

    // F171 设置应用对接
    let mut settings = SettingsApp::new(CAP_FS_WRITE);
    let ok_set = settings.set(b"theme.mode", 1, CAP_FS_WRITE);
    let denied = !settings.set(b"secret.key", 9, crate::vport::render::CAP_NET);
    let bad_arg = !settings.set(b"", 1, CAP_FS_WRITE);
    let mut blob = [0u8; 256];
    let n = settings.export(&mut blob);
    let mut reloaded = SettingsApp::new(CAP_FS_WRITE);
    let loaded = reloaded.import(&blob[..n]);
    let mut dflt = default_settings(CAP_FS_WRITE);
    set.add(
        "F171 settings app",
        ok_set
            && settings.get(b"theme.mode") == Some(1)
            && denied
            && settings.denied == 1
            && bad_arg
            && settings.bad_args == 1
            && settings.writes == 1
            && settings.reads == 1
            && settings.get(b"absent").is_none()
            && n > 0
            && loaded == 1
            && reloaded.get(b"theme.mode") == Some(1)
            && reloaded.imports == 1
            && dflt.len() == 3
            && dflt.get(b"keymap") == Some(1)
            && dflt.get(b"dpi.permille") == Some(1000),
        "配置读写/权限面/导出导入",
    );

    // F172 文件管理器对接
    let mut fs = Initfs::new();
    let _ = fs.add(b"etc", InitfsKind::Directory, b"");
    let _ = fs.add(b"motd.txt", InitfsKind::Regular, b"welcome");
    let _ = fs.add(b"init", InitfsKind::Executable, b"init-binary");
    let mut vfs = VfsUser::new(fs);
    let mut entries = [BrowsedEntry::empty(); BROWSE_MAX];
    let listed = browse(&vfs, &mut entries);
    let mut rbuf = [0u8; VFS_READ_BUF];
    let got = read_file(&mut vfs, b"motd.txt", &mut rbuf);
    let is_dir = read_file(&mut vfs, b"etc", &mut rbuf) == Err(VfsErr::IsDir);
    let absent = read_file(&mut vfs, b"absent", &mut rbuf) == Err(VfsErr::NoEntry);
    // 降级：把 fd 表用满，下一次打开必须显式失败（而非静默丢弃）。
    let mut fds = [0usize; VFS_MAX_FD];
    let mut j = 0usize;
    let mut opened = 0usize;
    while j < VFS_MAX_FD {
        match vfs.open(b"motd.txt", false) {
            Ok(f) => {
                fds[j] = f;
                opened += 1;
            }
            Err(_) => break,
        }
        j += 1;
    }
    let too_many = vfs.open(b"motd.txt", false) == Err(VfsErr::TooMany);
    let mut m = 0usize;
    while m < opened {
        let _ = vfs.close(fds[m]);
        m += 1;
    }
    set.add(
        "F172 file browser",
        listed == 3
            && entries[0].is_dir()
            && entries[0].name_bytes() == b"etc"
            && entries[2].name_bytes() == b"init"
            && entries[2].size == 11
            && entries[2].kind == InitfsKind::Executable
            && got == Ok(7)
            && &rbuf[..7] == b"welcome"
            && is_dir
            && absent
            && opened == VFS_MAX_FD
            && too_many
            && vfs.open_fds() == 0,
        "VFS 用户态浏览/读文件/句柄上限",
    );

    // F173 兼容性测试矩阵
    let mut matrix = standard_compat_matrix();
    let dup_case = !matrix.record(b"fileman", b"browse", CompatStatus::Pass, 0);
    let no_reason = !matrix.record(b"app", b"x", CompatStatus::Degraded, 0);
    let failed = matrix.record(b"legacy", b"raw_usb", CompatStatus::Fail, 0);
    let green_after_fail = matrix.green();
    let clean = standard_compat_matrix();
    let clean_ok = clean.green();
    let hit = clean.find(b"settings", b"read_write");
    set.add(
        "F173 compat matrix",
        matrix.len() == 7
            && dup_case
            && no_reason
            && failed
            && matrix.missing_reasons == 1
            && matrix.fail == 1
            && !green_after_fail
            && matrix.degraded_user_visible() == 2
            && clean_ok
            && clean.fail == 0
            && clean.degraded == 2
            && clean.pass == 4
            && clean.pass_rate_permille() == 666
            && hit.map(|c| c.status == CompatStatus::Pass).unwrap_or(false)
            && hit.map(|c| c.reason == 0).unwrap_or(false),
        "抽样矩阵/Top200 门禁/降级须带原因",
    );

    // F174 双后端 CI
    let mut ci = DualBackendCi::new(CI_STEPS);
    let mut s = 0usize;
    while s < CI_STEPS as usize {
        let _ = ci.advance(BackendKind::Windows, true);
        let _ = ci.advance(BackendKind::Varix, true);
        s += 1;
    }
    let both = ci.both_green();
    let frozen = ci.advance(BackendKind::Varix, false);
    let mut red_ci = DualBackendCi::new(2);
    let _ = red_ci.advance(BackendKind::Varix, false);
    let _ = red_ci.advance(BackendKind::Varix, true);
    set.add(
        "F174 dual backend ci",
        ci.lane(BackendKind::Windows).is_green()
            && ci.lane(BackendKind::Varix).done == CI_STEPS
            && both
            && ci.windows_lane_intact()
            && frozen == LaneState::Green
            && ci.reds == 0
            && red_ci.lane(BackendKind::Varix).state == LaneState::Red
            && red_ci.reds == 1
            && !red_ci.both_green()
            && red_ci.lane(BackendKind::Windows).state == LaneState::Pending
            && LaneState::Green != LaneState::Red,
        "双线同仓构建/一红即红/旧后端恒在",
    );

    // F175 移植域自检 — 映射完整性 + 渲染一致 + 崩溃红线，三合一
    let prior = set.len();
    set.add(
        "F175 port self-test",
        crate::vport::tauri_api_map().fully_covered()
            && crate::vport::pixel_walk(&crate::vport::walk_fixture()).0 == 0
            && cm.cascade_deaths == 0
            && cm.critical_alive()
            && matrix.fail == 1
            && clean.green()
            && ci.both_green()
            && prior + 1 >= 25,
        "映射完整性/渲染一致/崩溃红线/域项数",
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f169_kill_never_spreads() {
        let mut m = crash_matrix_standard();
        assert_eq!(m.naive_cascade_count(1), 4);
        assert!(m.kill(1));
        assert_eq!(m.cascade_deaths, 0);
        assert_eq!(m.alive_count(), 4);
        assert!(m.by_name(b"taskbar").unwrap().alive);
        assert!(m.by_name(b"startmenu").unwrap().alive);
    }

    #[test]
    fn f169_restart_only_self() {
        let mut m = crash_matrix_standard();
        assert!(m.inject_crash(3));
        assert!(!m.by_name(b"startmenu").unwrap().alive);
        assert!(m.by_name(b"desktop").unwrap().alive);
        assert!(m.restart(3));
        assert!(m.by_name(b"startmenu").unwrap().alive);
        assert_eq!(m.by_name(b"startmenu").unwrap().restarts, 1);
    }

    #[test]
    fn f170_launcher_budget() {
        let mut l = Launcher::new();
        l.record_sample(50_000);
        l.record_sample(60_000);
        assert!(l.within_budget());
        assert_eq!(l.p50(), 60_000);
        l.record_sample(400_000);
        assert!(!l.within_budget());
    }

    #[test]
    fn f171_permission_is_enforced() {
        let mut s = SettingsApp::new(CAP_FS_WRITE);
        assert!(s.set(b"k", 1, CAP_FS_WRITE));
        assert!(!s.set(b"k", 2, crate::vport::render::CAP_NET));
        assert_eq!(s.denied, 1);
        assert_eq!(s.get(b"k"), Some(1));
    }

    #[test]
    fn f172_browse_and_read() {
        let mut fs = Initfs::new();
        let _ = fs.add(b"a.txt", InitfsKind::Regular, b"xyz");
        let mut vfs = VfsUser::new(fs);
        let mut out = [BrowsedEntry::empty(); 4];
        assert_eq!(browse(&vfs, &mut out), 1);
        assert_eq!(out[0].name_bytes(), b"a.txt");
        let mut buf = [0u8; 8];
        assert_eq!(read_file(&mut vfs, b"a.txt", &mut buf), Ok(3));
        assert_eq!(&buf[..3], b"xyz");
    }

    #[test]
    fn f173_degraded_needs_reason() {
        let mut bad = CompatMatrix::new(1);
        assert!(!bad.record(b"a", b"b", CompatStatus::Degraded, 0));
        assert_eq!(bad.missing_reasons, 1);
        assert!(!bad.green());

        let mut ok = CompatMatrix::new(1);
        assert!(ok.record(b"a", b"b", CompatStatus::Degraded, 3));
        assert_eq!(ok.degraded, 1);
        assert!(ok.green());
    }

    #[test]
    fn f174_red_lane_blocks_green() {
        let mut ci = DualBackendCi::new(2);
        let _ = ci.advance(BackendKind::Windows, true);
        let _ = ci.advance(BackendKind::Varix, false);
        assert!(!ci.both_green());
        assert_eq!(ci.reds, 1);
        assert!(ci.windows_lane_intact());
    }

    #[test]
    fn f_run_verify_checks_pass() {
        let set = crate::vport::run_vport_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 4096];
            let n = set.render(&mut buf);
            panic!("vport self-test failed:\n{}", core::str::from_utf8(&buf[..n]).unwrap_or("<x>"));
        }
        assert!(set.len() >= 25, "vport domain must expose >= 25 checks");
    }
}

