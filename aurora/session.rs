//! AURORA-1000 AI-15 · 会话与启动（A351~A375，W2）
//!
//! 首创点：会话与启动（登录 / 会话管理 / 自启动 / 锁屏恢复 / 崩溃恢复 / 快速启动，
//! 秒级进桌面）。本模块是纯逻辑域：登录状态机、口令验证（盐 + 多轮简单散列纯函数）、
//! 会话表、自启动清单、窗口布局快照与崩溃重放、快速启动阶段计时、用户切换、访客会话、
//! 空闲锁定、性能预算、域自检与模糊测试边界，全部以固定容量数组 + usize 计数实现，
//! 无分配、无 unsafe、无宏、无泛型魔法，可直接在宿主机测试套件中无硬件地运行。

use crate::checks::CheckSet;

pub const SESSION_DOMAIN: &str = "aurora-session";

// 固定容量上限（仅用数组 + 计数，禁止 Vec/String/Box）。
pub const MAX_SESSIONS: usize = 8;
pub const MAX_AUTOSTART: usize = 16;
pub const MAX_WINDOWS: usize = 12;
pub const MAX_PREHEAT: usize = 8;

// 登录失败冷却策略（A351）。
pub const MAX_FAILS: u8 = 5;
pub const COOLDOWN_MS: u32 = 30_000;

// ===========================================================================
// A351 登录界面 / 登录状态机
// LockScreen → Authenticating → Active；失败 MAX_FAILS 次 → Cooldown 冷却计时。
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthState {
    Locked,        // 锁屏 / 登录界面
    Authenticating,
    Active,
    Cooldown,      // 失败过多，冷却中
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginResult {
    Granted,
    Denied,
    Cooldown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoginMachine {
    pub state: AuthState,
    pub fail_count: u8,
    pub cooldown_remaining_ms: u32,
    pub user_id: u8,
}

pub fn login_new() -> LoginMachine {
    LoginMachine {
        state: AuthState::Locked,
        fail_count: 0,
        cooldown_remaining_ms: 0,
        user_id: 0,
    }
}

/// 提交一次口令哈希尝试。比较候选哈希与存储哈希，相等则进入 Active，
/// 否则失败计数 +1；达到上限进入 Cooldown 并启动冷却计时。
pub fn login_submit(m: &mut LoginMachine, candidate: &[u8; 32], stored: &[u8; 32]) -> LoginResult {
    if m.state == AuthState::Active {
        return LoginResult::Granted;
    }
    if m.state == AuthState::Cooldown {
        return LoginResult::Cooldown;
    }
    m.state = AuthState::Authenticating;
    if *candidate == *stored {
        m.state = AuthState::Active;
        m.fail_count = 0;
        LoginResult::Granted
    } else {
        m.fail_count = m.fail_count.saturating_add(1);
        if m.fail_count >= MAX_FAILS {
            m.state = AuthState::Cooldown;
            m.cooldown_remaining_ms = COOLDOWN_MS;
            LoginResult::Cooldown
        } else {
            m.state = AuthState::Locked;
            LoginResult::Denied
        }
    }
}

/// 推进冷却计时；归零后回到 Locked 并清零失败计数，允许重试。
pub fn login_cooldown_tick(m: &mut LoginMachine, dt_ms: u32) {
    if m.state == AuthState::Cooldown {
        m.cooldown_remaining_ms = m.cooldown_remaining_ms.saturating_sub(dt_ms);
        if m.cooldown_remaining_ms == 0 {
            m.state = AuthState::Locked;
            m.fail_count = 0;
        }
    }
}

// ===========================================================================
// A352 口令验证（盐 + 多轮简单散列纯函数，定长 [u8; 32]）
// FNV-1a 起种，常数混淆迭代，输出固定 32 字节哈希；相同输入必得相同输出。
// ===========================================================================

pub fn hash_passphrase(pass: &[u8], salt: &[u8; 8], rounds: u32) -> [u8; 32] {
    let mut out = [0u8; 32];
    // FNV-1a 32 位初值，先用盐起种。
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < 8 {
        h ^= salt[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    let mut round = 0u32;
    while round < rounds {
        let mut j = 0usize;
        while j < pass.len() {
            h ^= pass[j] as u32;
            h = h.wrapping_mul(0x0100_0193);
            // 常数混淆迭代，打散低位相关性。
            h ^= h >> 15;
            h = h.wrapping_mul(0x2C1B_3C6D);
            h ^= h >> 12;
            j += 1;
        }
        let bytes = h.to_le_bytes();
        out[round as usize % 32] ^= bytes[0];
        out[(round as usize + 1) % 32] ^= bytes[1];
        out[(round as usize + 2) % 32] ^= bytes[2];
        out[(round as usize + 3) % 32] ^= bytes[3];
        round = round.wrapping_add(1);
    }
    out
}

/// 建模验证：对候选口令做哈希并与存储哈希比对。
pub fn verify_passphrase(candidate: &[u8], salt: &[u8; 8], rounds: u32, stored: &[u8; 32]) -> bool {
    hash_passphrase(candidate, salt, rounds) == *stored
}

// ===========================================================================
// A353 会话管理（会话表 [Session; 8]，用户 id / 状态 / 启动时间戳）
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    Active,
    Suspended,
    Locked,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Session {
    pub user_id: u8,
    pub state: SessionState,
    /// 锁屏前的原状态，解锁时恢复（A357）。
    pub saved_state: SessionState,
    pub start_stamp: u64,
    pub is_guest: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct SessionTable {
    slots: [Session; MAX_SESSIONS],
    count: usize,
}

impl SessionTable {
    pub const fn new() -> SessionTable {
        SessionTable {
            slots: [Session {
                user_id: 0,
                state: SessionState::Closed,
                saved_state: SessionState::Closed,
                start_stamp: 0,
                is_guest: false,
            }; MAX_SESSIONS],
            count: 0,
        }
    }

    /// 开新会话；优先复用已 Closed 槽位，否则在表尾追加；满则返回 None。
    pub fn session_open(&mut self, user_id: u8, stamp: u64, is_guest: bool) -> Option<usize> {
        let mut reuse: Option<usize> = None;
        let mut i = 0usize;
        while i < self.count {
            if self.slots[i].state == SessionState::Closed {
                reuse = Some(i);
                break;
            }
            i += 1;
        }
        let fresh = Session {
            user_id,
            state: SessionState::Active,
            saved_state: SessionState::Active,
            start_stamp: stamp,
            is_guest,
        };
        if let Some(idx) = reuse {
            self.slots[idx] = fresh;
            Some(idx)
        } else if self.count < MAX_SESSIONS {
            self.slots[self.count] = fresh;
            self.count += 1;
            Some(self.count - 1)
        } else {
            None
        }
    }

    pub fn session_close(&mut self, idx: usize) {
        if idx < self.count {
            self.slots[idx].state = SessionState::Closed;
            self.slots[idx].saved_state = SessionState::Closed;
        }
    }

    pub fn session_get(&self, idx: usize) -> Option<Session> {
        if idx < self.count {
            Some(self.slots[idx])
        } else {
            None
        }
    }

    pub fn session_find_active(&self) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if self.slots[i].state == SessionState::Active {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// A354 自启动管理（[AutoStart; 16]，按优先级排序依次拉起，失败跳过计数）
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoStart {
    pub name: &'static str,
    pub priority: u8, // 越小越先拉起
    pub enabled: bool,
    pub fails: bool,  // 模拟该条目启动失败
    pub launched: bool,
    pub fail_count: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct AutoStartList {
    items: [AutoStart; MAX_AUTOSTART],
    count: usize,
}

impl AutoStartList {
    pub const fn new() -> AutoStartList {
        AutoStartList {
            items: [AutoStart {
                name: "",
                priority: 0,
                enabled: false,
                fails: false,
                launched: false,
                fail_count: 0,
            }; MAX_AUTOSTART],
            count: 0,
        }
    }

    /// 按优先级插入（升序）。满则返回 false。
    pub fn autostart_add(&mut self, item: AutoStart) -> bool {
        if self.count >= MAX_AUTOSTART {
            return false;
        }
        let mut pos = self.count;
        let mut i = 0usize;
        while i < self.count {
            if self.items[i].priority > item.priority {
                pos = i;
                break;
            }
            i += 1;
        }
        let mut j = self.count;
        while j > pos {
            self.items[j] = self.items[j - 1];
            j -= 1;
        }
        self.items[pos] = item;
        self.count += 1;
        true
    }

    /// 按优先级依次拉起；返回 (成功数, 失败跳过数)。禁用项不计入。
    pub fn autostart_run(&mut self) -> (usize, usize) {
        let mut launched = 0usize;
        let mut skipped = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if !self.items[i].enabled {
                i += 1;
                continue;
            }
            if self.items[i].fails {
                let mut it = self.items[i];
                it.fail_count = it.fail_count.saturating_add(1);
                self.items[i] = it;
                skipped += 1;
            } else {
                let mut it = self.items[i];
                it.launched = true;
                self.items[i] = it;
                launched += 1;
            }
            i += 1;
        }
        (launched, skipped)
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// A355 会话保存 / 窗口布局快照（定长数组 + 校验和）
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowSlot {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    pub z: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub windows: [WindowSlot; MAX_WINDOWS],
    pub count: usize,
    pub checksum: u32,
}

/// 窗口字节的 CRC-32（与 power 域同算法，域内自包含实现）。
fn window_checksum(windows: &[WindowSlot], count: usize) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    let mut i = 0usize;
    while i < count {
        let w = windows[i];
        let bytes = [
            w.x as u8,
            (w.x >> 8) as u8,
            w.y as u8,
            (w.y >> 8) as u8,
            w.w as u8,
            (w.w >> 8) as u8,
            w.h as u8,
            (w.h >> 8) as u8,
            w.z,
        ];
        let mut k = 0usize;
        while k < bytes.len() {
            crc ^= bytes[k] as u32;
            let mut bit = 0u32;
            while bit < 8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
                bit += 1;
            }
            k += 1;
        }
        i += 1;
    }
    !crc
}

pub fn snapshot_capture(windows: &[WindowSlot]) -> SessionSnapshot {
    let mut snap = SessionSnapshot {
        windows: [WindowSlot { x: 0, y: 0, w: 0, h: 0, z: 0 }; MAX_WINDOWS],
        count: 0,
        checksum: 0,
    };
    let n = windows.len().min(MAX_WINDOWS);
    let mut i = 0usize;
    while i < n {
        snap.windows[i] = windows[i];
        i += 1;
    }
    snap.count = n;
    snap.checksum = window_checksum(&snap.windows, n);
    snap
}

impl SessionSnapshot {
    pub fn snapshot_verify(&self) -> bool {
        window_checksum(&self.windows, self.count) == self.checksum
    }
}

// ===========================================================================
// A356 快速启动优化（预热清单 + 启动模式选择）
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreheatItem {
    pub name: &'static str,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootMode {
    Fast,  // 预热命中，秒级进桌面
    Normal,
}

#[derive(Clone, Copy, Debug)]
pub struct PreheatList {
    items: [PreheatItem; MAX_PREHEAT],
    count: usize,
}

impl PreheatList {
    pub const fn new() -> PreheatList {
        PreheatList {
            items: [PreheatItem { name: "", enabled: false }; MAX_PREHEAT],
            count: 0,
        }
    }

    pub fn preheat_add(&mut self, item: PreheatItem) -> bool {
        if self.count >= MAX_PREHEAT {
            return false;
        }
        self.items[self.count] = item;
        self.count += 1;
        true
    }

    pub fn enabled_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if self.items[i].enabled {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// 冷启动或预热为空则退化为 Normal；否则走 Fast 快速启动。
pub fn choose_boot_mode(preheat_enabled: usize, cold: bool) -> BootMode {
    if cold || preheat_enabled == 0 {
        BootMode::Normal
    } else {
        BootMode::Fast
    }
}

// ===========================================================================
// A357 锁屏与解锁（锁屏冻结会话状态，解锁恢复原状态）
// ===========================================================================

pub fn session_lock(table: &mut SessionTable, idx: usize) {
    if let Some(mut s) = table.session_get(idx) {
        if s.state == SessionState::Active {
            s.saved_state = s.state;
            s.state = SessionState::Locked;
            if idx < MAX_SESSIONS {
                table.slots[idx] = s;
            }
        }
    }
}

pub fn session_unlock(table: &mut SessionTable, idx: usize) {
    if let Some(mut s) = table.session_get(idx) {
        if s.state == SessionState::Locked {
            s.state = s.saved_state;
            if idx < MAX_SESSIONS {
                table.slots[idx] = s;
            }
        }
    }
}

// ===========================================================================
// A358 崩溃后桌面恢复（快照重放 + 校验和判定）
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoverVerdict {
    Restored, // 校验通过，重放窗口布局
    Corrupt,  // 校验失败，丢弃快照
    Empty,    // 无快照，全新会话
}

pub fn crash_recover(snap: &SessionSnapshot) -> RecoverVerdict {
    if snap.count == 0 {
        RecoverVerdict::Empty
    } else if !snap.snapshot_verify() {
        RecoverVerdict::Corrupt
    } else {
        RecoverVerdict::Restored
    }
}

// ===========================================================================
// A359 注销 / 重启 / 关机流程（电源会话状态机）
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerFlow {
    None,
    Logout,
    Restart,
    Shutdown,
}

#[derive(Clone, Copy, Debug)]
pub struct SessionPowerFlow {
    pub pending: PowerFlow,
    pub closing: bool,
}

pub fn power_flow_new() -> SessionPowerFlow {
    SessionPowerFlow { pending: PowerFlow::None, closing: false }
}

/// 发起流程：先标记 closing 并清理全部会话，再置 pending。
pub fn power_flow_request(f: &mut SessionPowerFlow, kind: PowerFlow, table: &mut SessionTable) {
    let mut i = 0usize;
    while i < table.len() {
        table.session_close(i);
        i += 1;
    }
    f.closing = true;
    f.pending = kind;
}

// ===========================================================================
// A360 多用户会话（用户切换：会话挂起 / 恢复）
// ===========================================================================

pub fn session_suspend(table: &mut SessionTable, idx: usize) {
    if let Some(mut s) = table.session_get(idx) {
        if s.state == SessionState::Active {
            s.state = SessionState::Suspended;
            if idx < MAX_SESSIONS {
                table.slots[idx] = s;
            }
        }
    }
}

pub fn session_resume(table: &mut SessionTable, idx: usize) {
    if let Some(mut s) = table.session_get(idx) {
        if s.state == SessionState::Suspended {
            s.state = SessionState::Active;
            if idx < MAX_SESSIONS {
                table.slots[idx] = s;
            }
        }
    }
}

// ===========================================================================
// A361 访客会话（受限权限位）
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Perms {
    pub can_install: bool,
    pub can_shutdown: bool,
    pub can_access_fs: bool,
}

pub fn guest_perms() -> Perms {
    Perms {
        can_install: false,
        can_shutdown: false,
        can_access_fs: false,
    }
}

pub fn user_perms() -> Perms {
    Perms {
        can_install: true,
        can_shutdown: true,
        can_access_fs: true,
    }
}

// ===========================================================================
// A362 会话性能预算（启动阶段秒级预算判定）
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhaseVerdict {
    Within,
    Over,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootBudget {
    pub boot_budget_ms: u32,
    pub login_budget_ms: u32,
    pub desktop_budget_ms: u32,
    pub total_redline_ms: u32,
}

pub fn boot_phase_verdict(measured: u32, budget: u32) -> PhaseVerdict {
    if measured <= budget {
        PhaseVerdict::Within
    } else {
        PhaseVerdict::Over
    }
}

pub fn boot_total_within(total: u32, redline: u32) -> bool {
    total <= redline
}

/// boot→login→desktop 各阶段预算 + 总红线判定。
pub fn boot_perf_verdict(
    boot_ms: u32,
    login_ms: u32,
    desktop_ms: u32,
    b: &BootBudget,
) -> (PhaseVerdict, PhaseVerdict, PhaseVerdict, bool) {
    let total = boot_ms + login_ms + desktop_ms;
    (
        boot_phase_verdict(boot_ms, b.boot_budget_ms),
        boot_phase_verdict(login_ms, b.login_budget_ms),
        boot_phase_verdict(desktop_ms, b.desktop_budget_ms),
        boot_total_within(total, b.total_redline_ms),
    )
}

// ===========================================================================
// A363 会话安全（空闲锁定计时器）
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdleGuard {
    pub idle_ms: u32,
    pub timeout_ms: u32,
    pub locked: bool,
}

pub fn idle_new(timeout_ms: u32) -> IdleGuard {
    IdleGuard { idle_ms: 0, timeout_ms, locked: false }
}

/// 有活动则清零计时；空闲超阈值则锁屏并返回 true（仅首次触发）。
pub fn idle_tick(g: &mut IdleGuard, active: bool, dt_ms: u32) -> bool {
    if active {
        g.idle_ms = 0;
        return false;
    }
    g.idle_ms = g.idle_ms.saturating_add(dt_ms);
    if !g.locked && g.idle_ms >= g.timeout_ms {
        g.locked = true;
        true
    } else {
        false
    }
}

// ===========================================================================
// A364 会话自检收口（不变量聚合，返回违规数）
// ===========================================================================

pub fn session_invariants(table: &SessionTable) -> u8 {
    let mut violations = 0u8;
    let mut i = 0usize;
    while i < table.count {
        let s = table.slots[i];
        // 不变量：Active 会话的 saved_state 不得为 Closed。
        if s.state == SessionState::Active && s.saved_state == SessionState::Closed {
            violations += 1;
        }
        // 不变量：管理员(user_id==0)不得标记为访客。
        if s.user_id == 0 && s.is_guest {
            violations += 1;
        }
        i += 1;
    }
    // 不变量：同一用户至多一个前台 Active 会话（不同用户可并行）。
    let mut a = 0usize;
    while a < table.count {
        let mut b = a + 1;
        while b < table.count {
            if table.slots[a].state == SessionState::Active
                && table.slots[b].state == SessionState::Active
                && table.slots[a].user_id == table.slots[b].user_id
            {
                violations += 1;
            }
            b += 1;
        }
        a += 1;
    }
    violations
}

// ===========================================================================
// A365 会话可观测（会话域指标计数）
// ===========================================================================

pub fn session_metrics(table: &SessionTable) -> (usize, usize, usize, usize) {
    let mut active = 0usize;
    let mut suspended = 0usize;
    let mut locked = 0usize;
    let mut guest = 0usize;
    let mut i = 0usize;
    while i < table.count {
        let s = table.slots[i];
        match s.state {
            SessionState::Active => active += 1,
            SessionState::Suspended => suspended += 1,
            SessionState::Locked => locked += 1,
            SessionState::Closed => {}
        }
        if s.is_guest {
            guest += 1;
        }
        i += 1;
    }
    (active, suspended, locked, guest)
}

// ===========================================================================
// A366 会话文档（域描述符）
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocInfo {
    pub domain: &'static str,
    pub first: u16,
    pub last: u16,
    pub count: u8,
}

pub fn session_doc() -> DocInfo {
    DocInfo { domain: SESSION_DOMAIN, first: 351, last: 375, count: 25 }
}

// ===========================================================================
// A367 启动进度真实（不得伪造 100%）
// ===========================================================================

pub fn boot_progress_pct(boot_done: bool, login_done: bool, desktop_done: bool) -> u8 {
    if !boot_done {
        0
    } else if !login_done {
        34
    } else if !desktop_done {
        67
    } else {
        100
    }
}

/// 进度若为 100，必须确实已完成桌面阶段（防伪造）。
pub fn boot_progress_is_real(pct: u8, desktop_done: bool) -> bool {
    (pct == 100) == desktop_done
}

pub fn boot_progress_monotonic(prev: u8, next: u8) -> bool {
    next >= prev
}

// ===========================================================================
// A368 会话域自检收口（典型场景下子检查计数）
// ===========================================================================

pub fn session_domain_tally() -> (usize, usize) {
    let mut table = SessionTable::new();
    table.session_open(1, 100, false);
    table.session_open(2, 200, true);
    let inv = session_invariants(&table);
    let (active, suspended, _locked, guest) = session_metrics(&table);
    let total = 4usize;
    let mut passed = 0usize;
    if inv == 0 {
        passed += 1;
    }
    if active == 2 {
        passed += 1;
    }
    if suspended == 0 {
        passed += 1;
    }
    if guest == 1 {
        passed += 1;
    }
    (passed, total)
}

// ===========================================================================
// A369 会话与启动自检（端到端场景必须全部通过）
// ===========================================================================

pub fn session_startup_selfcheck() -> bool {
    let salt = [7u8, 6, 5, 4, 3, 2, 1, 0];
    let stored = hash_passphrase(b"pw", &salt, 64);
    let mut m = login_new();
    let cand = hash_passphrase(b"pw", &salt, 64);
    if login_submit(&mut m, &cand, &stored) != LoginResult::Granted {
        return false;
    }
    let mut table = SessionTable::new();
    if table.session_open(1, 1000, false).is_none() {
        return false;
    }
    let wins = [WindowSlot { x: 10, y: 20, w: 100, h: 200, z: 1 }];
    let snap = snapshot_capture(&wins);
    if crash_recover(&snap) != RecoverVerdict::Restored {
        return false;
    }
    session_lock(&mut table, 0);
    if table.session_get(0).unwrap().state != SessionState::Locked {
        return false;
    }
    session_unlock(&mut table, 0);
    if table.session_get(0).unwrap().state != SessionState::Active {
        return false;
    }
    session_invariants(&table) == 0
}

// ===========================================================================
// A370 会话与启动性能预算（典型启动须达标）
// ===========================================================================

pub fn session_startup_perf_budget() -> bool {
    let b = BootBudget {
        boot_budget_ms: 2000,
        login_budget_ms: 3000,
        desktop_budget_ms: 4000,
        total_redline_ms: 10000,
    };
    let (p1, p2, p3, total) = boot_perf_verdict(1500, 2500, 3000, &b);
    p1 == PhaseVerdict::Within
        && p2 == PhaseVerdict::Within
        && p3 == PhaseVerdict::Within
        && total
}

// ===========================================================================
// A371 会话与启动可观测（混合场景指标）
// ===========================================================================

pub fn session_startup_metrics() -> (usize, usize, usize, usize) {
    let mut table = SessionTable::new();
    table.session_open(1, 1, false);
    let g = table.session_open(2, 2, true).unwrap();
    session_lock(&mut table, g);
    session_metrics(&table)
}

// ===========================================================================
// A372 会话与启动模糊测试（伪随机事件灌入登录状态机，不变量始终成立）
// ===========================================================================

pub fn session_startup_fuzz() -> bool {
    let mut m = login_new();
    let stored = [0u8; 32];
    let mut seed: u64 = 0x1234_5678_9ABC_DEF0;
    let mut ok = true;
    let mut step = 0u32;
    while step < 400 {
        if m.state == AuthState::Cooldown {
            login_cooldown_tick(&mut m, 2000);
        }
        // 确定性 LCG 伪随机：低位决定本次是否为正确口令。
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let correct = (seed & 1) == 1;
        let cand = if correct { stored } else { [0xFFu8; 32] };
        let r = login_submit(&mut m, &cand, &stored);
        if m.fail_count > MAX_FAILS {
            ok = false;
        }
        if m.state == AuthState::Cooldown && m.fail_count != MAX_FAILS {
            ok = false;
        }
        if r == LoginResult::Granted && m.state != AuthState::Active {
            ok = false;
        }
        // 模拟一次注销回到可重试状态。
        if r == LoginResult::Granted {
            m.state = AuthState::Locked;
            m.fail_count = 0;
        }
        step += 1;
    }
    ok
}

// ===========================================================================
// A373 会话与启动文档（完整功能编号表 351..375）
// ===========================================================================

pub fn session_startup_doc() -> [u16; 25] {
    let mut arr = [0u16; 25];
    let mut i = 0usize;
    while i < 25 {
        arr[i] = 351 + i as u16;
        i += 1;
    }
    arr
}

// ===========================================================================
// A374 会话与启动降级链（子系统失效时逐级退避）
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeLevel {
    Full,    // 全部可用
    Reduced, // 快速启动失效 → 普通启动
    Minimal, // 锁屏失效 → 自动登录
    Safe,    // 崩溃恢复失效 → 全新会话
}

pub fn session_startup_degrade(fast_boot_ok: bool, lock_ok: bool, recover_ok: bool) -> DegradeLevel {
    if !recover_ok {
        DegradeLevel::Safe
    } else if !lock_ok {
        DegradeLevel::Minimal
    } else if !fast_boot_ok {
        DegradeLevel::Reduced
    } else {
        DegradeLevel::Full
    }
}

// ===========================================================================
// A375 会话与启动域自检收口（返回完整 CheckSet，≥25 项全真）
// ===========================================================================

pub fn run_session_checks() -> CheckSet {
    let mut set = CheckSet::new(SESSION_DOMAIN);

    // A351 登录状态机
    let mut lm = login_new();
    set.add("A351 login initial", lm.state == AuthState::Locked && lm.fail_count == 0, "locked");
    let ok_hash = [0u8; 32];
    let right = login_submit(&mut lm, &ok_hash, &ok_hash);
    set.add("A351 login granted", right == LoginResult::Granted && lm.state == AuthState::Active, "active");
    let mut lm2 = login_new();
    let bad = login_submit(&mut lm2, &[0xFFu8; 32], &ok_hash);
    set.add("A351 login denied", bad == LoginResult::Denied && lm2.fail_count == 1, "fail+1");
    let mut lm3 = login_new();
    let mut denials = 0u8;
    for _ in 0..MAX_FAILS {
        if login_submit(&mut lm3, &[0xAAu8; 32], &ok_hash) == LoginResult::Denied {
            denials += 1;
        }
    }
    set.add(
        "A351 cooldown",
        denials == MAX_FAILS - 1 && lm3.state == AuthState::Cooldown && lm3.cooldown_remaining_ms == COOLDOWN_MS,
        "cooldown",
    );
    login_cooldown_tick(&mut lm3, COOLDOWN_MS);
    set.add("A351 cooldown reset", lm3.state == AuthState::Locked && lm3.fail_count == 0, "reset");

    // A352 口令散列
    let salt_a = [1u8, 2, 3, 4, 5, 6, 7, 8];
    let salt_b = [8u8, 7, 6, 5, 4, 3, 2, 1];
    let h1 = hash_passphrase(b"secret", &salt_a, 64);
    let h2 = hash_passphrase(b"secret", &salt_a, 64);
    let h3 = hash_passphrase(b"other", &salt_a, 64);
    let h4 = hash_passphrase(b"secret", &salt_b, 64);
    set.add("A352 hash stable", h1 == h2, "deterministic");
    set.add("A352 hash differs pass", h1 != h3, "distinct input");
    set.add("A352 hash differs salt", h1 != h4, "distinct salt");
    set.add("A352 verify", verify_passphrase(b"secret", &salt_a, 64, &h1) && !verify_passphrase(b"x", &salt_a, 64, &h1), "verify");

    // A353 会话表
    let mut t = SessionTable::new();
    let a = t.session_open(1, 10, false);
    let b_idx = t.session_open(2, 20, false);
    set.add("A353 open+find", a == Some(0) && b_idx == Some(1) && t.session_find_active() == Some(0), "open");
    let mut full = SessionTable::new();
    let mut opened = 0usize;
    for u in 0..MAX_SESSIONS + 3 {
        if full.session_open(u as u8, u as u64, false).is_some() {
            opened += 1;
        }
    }
    set.add("A353 table bound", opened == MAX_SESSIONS && full.session_open(99, 0, false).is_none(), "capacity");
    full.session_close(0);
    set.add("A353 reuse closed", full.session_open(7, 0, false) == Some(0), "reuse");

    // A354 自启动
    let mut al = AutoStartList::new();
    al.autostart_add(AutoStart { name: "b", priority: 2, enabled: true, fails: false, launched: false, fail_count: 0 });
    al.autostart_add(AutoStart { name: "a", priority: 1, enabled: true, fails: false, launched: false, fail_count: 0 });
    al.autostart_add(AutoStart { name: "c", priority: 3, enabled: false, fails: true, launched: false, fail_count: 0 });
    al.autostart_add(AutoStart { name: "dfail", priority: 0, enabled: true, fails: true, launched: false, fail_count: 0 });
    set.add("A354 sorted", al.items[0].name == "dfail" && al.items[1].name == "a" && al.items[2].name == "b", "priority order");
    let (launched, skipped) = al.autostart_run();
    set.add("A354 run", launched == 2 && skipped == 1, "launched/skipped");
    set.add("A354 disabled skip", !al.items[3].launched && al.items[3].fail_count == 0, "disabled ignored");
    let mut al_full = AutoStartList::new();
    let mut added_all = true;
    for i in 0..MAX_AUTOSTART + 3 {
        let it = AutoStart { name: "x", priority: i as u8, enabled: true, fails: false, launched: false, fail_count: 0 };
        if !al_full.autostart_add(it) && i < MAX_AUTOSTART {
            added_all = false;
        }
    }
    set.add("A354 list bound", added_all && al_full.len() == MAX_AUTOSTART, "capacity");

    // A355 会话快照
    let wins = [
        WindowSlot { x: 0, y: 0, w: 800, h: 600, z: 0 },
        WindowSlot { x: 10, y: 10, w: 400, h: 300, z: 1 },
    ];
    let snap = snapshot_capture(&wins);
    set.add("A355 capture", snap.count == 2 && snap.snapshot_verify(), "capture+checksum");

    // A356 快速启动
    set.add(
        "A356 boot mode",
        choose_boot_mode(3, false) == BootMode::Fast
            && choose_boot_mode(0, false) == BootMode::Normal
            && choose_boot_mode(3, true) == BootMode::Normal,
        "fast/normal",
    );

    // A357 锁屏/解锁
    let mut t2 = SessionTable::new();
    t2.session_open(1, 1, false);
    session_lock(&mut t2, 0);
    let locked_ok = t2.session_get(0).map(|s| s.state == SessionState::Locked).unwrap_or(false);
    session_unlock(&mut t2, 0);
    let unlocked_ok = t2.session_get(0).map(|s| s.state == SessionState::Active).unwrap_or(false);
    set.add("A357 lock/unlock", locked_ok && unlocked_ok, "restore state");

    // A358 崩溃恢复
    set.add("A358 recover", crash_recover(&snap) == RecoverVerdict::Restored, "restored");
    let mut bad = snap;
    bad.checksum = bad.checksum.wrapping_add(1);
    set.add("A358 corrupt", crash_recover(&bad) == RecoverVerdict::Corrupt, "checksum fail");
    let empty = SessionSnapshot { windows: [WindowSlot { x: 0, y: 0, w: 0, h: 0, z: 0 }; MAX_WINDOWS], count: 0, checksum: 0 };
    set.add("A358 empty", crash_recover(&empty) == RecoverVerdict::Empty, "no snapshot");

    // A359 注销/重启/关机
    let mut f = power_flow_new();
    power_flow_request(&mut f, PowerFlow::Restart, &mut t2);
    set.add("A359 flow", f.pending == PowerFlow::Restart && f.closing && t2.len() > 0, "closing");

    // A360 用户切换
    let mut t3 = SessionTable::new();
    t3.session_open(1, 1, false);
    t3.session_open(2, 2, false);
    session_suspend(&mut t3, 0);
    let susp = t3.session_get(0).map(|s| s.state == SessionState::Suspended).unwrap_or(false);
    session_resume(&mut t3, 0);
    let res = t3.session_get(0).map(|s| s.state == SessionState::Active).unwrap_or(false);
    set.add("A360 switch", susp && res, "suspend/resume");

    // A361 访客会话
    let gp = guest_perms();
    let up = user_perms();
    set.add("A361 perms", !gp.can_install && !gp.can_shutdown && up.can_access_fs, "guest limited");

    // A362 性能预算
    set.add("A362 budget", session_startup_perf_budget(), "within budgets");

    // A363 空闲锁定
    let mut g = idle_new(5000);
    let fired = !idle_tick(&mut g, true, 6000) && idle_tick(&mut g, false, 6000);
    set.add("A363 idle lock", fired && g.locked, "timeout locks");

    // A364 不变量
    set.add("A364 invariants", session_invariants(&t2) == 0, "no violations");

    // A365 可观测
    let (act, susp_n, lock_n, guest_n) = session_metrics(&t3);
    set.add("A365 metrics", act + susp_n + lock_n == t3.len() && guest_n == 0, "counts sum");

    // A366/A373 文档编号
    let doc = session_doc();
    let ids = session_startup_doc();
    set.add(
        "A366 doc",
        doc.count == 25 && doc.first == 351 && doc.last == 375,
        "descriptor",
    );
    set.add("A373 ids", ids[0] == 351 && ids[24] == 375, "351..375");

    // A367 进度真实
    set.add(
        "A367 progress",
        boot_progress_pct(false, false, false) == 0
            && boot_progress_pct(true, true, true) == 100
            && boot_progress_is_real(100, true)
            && !boot_progress_is_real(100, false)
            && boot_progress_monotonic(34, 67),
        "no fake 100%",
    );

    // A368/A369/A371/A372 域级自检聚合
    let (p, total) = session_domain_tally();
    set.add("A368 tally", p == total, "all pass");
    set.add("A369 e2e", session_startup_selfcheck(), "end to end");
    let m = session_startup_metrics();
    set.add("A371 metrics", m.0 == 1 && m.3 == 1, "mixed scene");
    set.add("A372 fuzz", session_startup_fuzz(), "400 steps");

    // A374 降级链
    set.add(
        "A374 degrade",
        session_startup_degrade(true, true, true) == DegradeLevel::Full
            && session_startup_degrade(false, true, true) == DegradeLevel::Reduced
            && session_startup_degrade(true, false, true) == DegradeLevel::Minimal
            && session_startup_degrade(true, true, false) == DegradeLevel::Safe,
        "ladder",
    );

    // A375 收口：计数与全真
    let (passed, failed) = set.tally();
    set.add(
        "A375 closure",
        failed == 0 && passed >= 25 && !set.truncated(),
        "closure",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a351_login_flow() {
        let mut lm = login_new();
        let h = [1u8; 32];
        assert_eq!(login_submit(&mut lm, &h, &h), LoginResult::Granted);
        assert_eq!(lm.state, AuthState::Active);
    }

    #[test]
    fn a351_cooldown_after_fails() {
        let mut lm = login_new();
        let stored = [2u8; 32];
        for _ in 0..MAX_FAILS - 1 {
            assert_eq!(login_submit(&mut lm, &[3u8; 32], &stored), LoginResult::Denied);
        }
        assert_eq!(login_submit(&mut lm, &[3u8; 32], &stored), LoginResult::Cooldown);
        assert_eq!(lm.cooldown_remaining_ms, COOLDOWN_MS);
    }

    #[test]
    fn a351_cooldown_blocks_then_resets() {
        let mut lm = login_new();
        lm.state = AuthState::Cooldown;
        lm.cooldown_remaining_ms = COOLDOWN_MS;
        assert_eq!(login_submit(&mut lm, &[0u8; 32], &[0u8; 32]), LoginResult::Cooldown);
        login_cooldown_tick(&mut lm, COOLDOWN_MS);
        assert_eq!(lm.state, AuthState::Locked);
        assert_eq!(lm.fail_count, 0);
    }

    #[test]
    fn a352_hash_deterministic() {
        let salt = [9u8; 8];
        assert_eq!(hash_passphrase(b"abc", &salt, 32), hash_passphrase(b"abc", &salt, 32));
    }

    #[test]
    fn a352_hash_differs_by_pass_and_salt() {
        let s1 = [1u8; 8];
        let s2 = [2u8; 8];
        let h = hash_passphrase(b"p", &s1, 16);
        assert_ne!(h, hash_passphrase(b"q", &s1, 16));
        assert_ne!(h, hash_passphrase(b"p", &s2, 16));
    }

    #[test]
    fn a352_verify_roundtrip() {
        let salt = [5u8; 8];
        let h = hash_passphrase(b"hunter2", &salt, 8);
        assert!(verify_passphrase(b"hunter2", &salt, 8, &h));
        assert!(!verify_passphrase(b"hunter3", &salt, 8, &h));
    }

    #[test]
    fn a353_open_find_close_reuse() {
        let mut t = SessionTable::new();
        assert_eq!(t.session_open(1, 0, false), Some(0));
        assert_eq!(t.session_open(2, 0, false), Some(1));
        assert_eq!(t.session_find_active(), Some(0));
        t.session_close(0);
        assert_eq!(t.session_open(3, 0, false), Some(0));
    }

    #[test]
    fn a353_table_capacity() {
        let mut t = SessionTable::new();
        for i in 0..MAX_SESSIONS {
            assert!(t.session_open(i as u8, 0, false).is_some());
        }
        assert!(t.session_open(99, 0, false).is_none());
    }

    #[test]
    fn a354_autostart_priority_order() {
        let mut al = AutoStartList::new();
        al.autostart_add(AutoStart { name: "late", priority: 9, enabled: true, fails: false, launched: false, fail_count: 0 });
        al.autostart_add(AutoStart { name: "early", priority: 1, enabled: true, fails: false, launched: false, fail_count: 0 });
        assert_eq!(al.items[0].name, "early");
        assert_eq!(al.items[1].name, "late");
    }

    #[test]
    fn a354_autostart_run_skip_fail_and_disabled() {
        let mut al = AutoStartList::new();
        al.autostart_add(AutoStart { name: "ok", priority: 0, enabled: true, fails: false, launched: false, fail_count: 0 });
        al.autostart_add(AutoStart { name: "bad", priority: 1, enabled: true, fails: true, launched: false, fail_count: 0 });
        al.autostart_add(AutoStart { name: "off", priority: 2, enabled: false, fails: false, launched: false, fail_count: 0 });
        let (l, s) = al.autostart_run();
        assert_eq!((l, s), (1, 1));
        assert!(!al.items[2].launched);
    }

    #[test]
    fn a355_snapshot_checksum_matches() {
        let w = [WindowSlot { x: 1, y: 2, w: 3, h: 4, z: 5 }];
        let s = snapshot_capture(&w);
        assert!(s.snapshot_verify());
        assert_eq!(s.count, 1);
    }

    #[test]
    fn a355_snapshot_truncates_to_capacity() {
        let mut w = [WindowSlot { x: 0, y: 0, w: 0, h: 0, z: 0 }; MAX_WINDOWS + 5];
        for (i, slot) in w.iter_mut().enumerate() {
            slot.x = i as u16;
        }
        let s = snapshot_capture(&w);
        assert_eq!(s.count, MAX_WINDOWS);
        assert!(s.snapshot_verify());
    }

    #[test]
    fn a356_boot_mode_choice() {
        assert_eq!(choose_boot_mode(2, false), BootMode::Fast);
        assert_eq!(choose_boot_mode(0, false), BootMode::Normal);
        assert_eq!(choose_boot_mode(2, true), BootMode::Normal);
    }

    #[test]
    fn a357_lock_unlock_restores_state() {
        let mut t = SessionTable::new();
        t.session_open(1, 0, false);
        session_lock(&mut t, 0);
        assert_eq!(t.session_get(0).unwrap().state, SessionState::Locked);
        session_unlock(&mut t, 0);
        assert_eq!(t.session_get(0).unwrap().state, SessionState::Active);
    }

    #[test]
    fn a358_recover_verdicts() {
        let w = [WindowSlot { x: 0, y: 0, w: 10, h: 10, z: 0 }];
        let snap = snapshot_capture(&w);
        assert_eq!(crash_recover(&snap), RecoverVerdict::Restored);
        let mut corrupt = snap;
        corrupt.windows[0].x = 42;
        assert_eq!(crash_recover(&corrupt), RecoverVerdict::Corrupt);
        let empty = SessionSnapshot { windows: [WindowSlot { x: 0, y: 0, w: 0, h: 0, z: 0 }; MAX_WINDOWS], count: 0, checksum: 0 };
        assert_eq!(crash_recover(&empty), RecoverVerdict::Empty);
    }

    #[test]
    fn a359_power_flow_closes_sessions() {
        let mut t = SessionTable::new();
        t.session_open(1, 0, false);
        let mut f = power_flow_new();
        power_flow_request(&mut f, PowerFlow::Shutdown, &mut t);
        assert!(f.closing);
        assert_eq!(f.pending, PowerFlow::Shutdown);
        assert_eq!(t.session_find_active(), None);
    }

    #[test]
    fn a360_suspend_resume() {
        let mut t = SessionTable::new();
        t.session_open(1, 0, false);
        session_suspend(&mut t, 0);
        assert_eq!(t.session_get(0).unwrap().state, SessionState::Suspended);
        session_resume(&mut t, 0);
        assert_eq!(t.session_get(0).unwrap().state, SessionState::Active);
    }

    #[test]
    fn a361_guest_perms_restricted() {
        let g = guest_perms();
        assert!(!g.can_install && !g.can_shutdown && !g.can_access_fs);
        assert!(user_perms().can_install);
    }

    #[test]
    fn a362_boot_budget_verdicts() {
        let b = BootBudget { boot_budget_ms: 100, login_budget_ms: 100, desktop_budget_ms: 100, total_redline_ms: 400 };
        let (p1, p2, p3, total) = boot_perf_verdict(50, 150, 50, &b);
        assert_eq!(p1, PhaseVerdict::Within);
        assert_eq!(p2, PhaseVerdict::Over);
        assert_eq!(p3, PhaseVerdict::Within);
        assert!(total);
    }

    #[test]
    fn a363_idle_lock_fires_once() {
        let mut g = idle_new(1000);
        assert!(!idle_tick(&mut g, false, 500));
        assert!(idle_tick(&mut g, false, 600));
        assert!(!idle_tick(&mut g, false, 600));
        assert!(g.locked);
    }

    #[test]
    fn a363_activity_resets_idle() {
        let mut g = idle_new(1000);
        assert!(!idle_tick(&mut g, false, 900));
        assert!(!idle_tick(&mut g, true, 900));
        assert_eq!(g.idle_ms, 0);
    }

    #[test]
    fn a364_invariants_clean_table() {
        let mut t = SessionTable::new();
        t.session_open(0, 0, false);
        t.session_open(1, 0, true);
        session_suspend(&mut t, 1);
        assert_eq!(session_invariants(&t), 0);
    }

    #[test]
    fn a365_metrics_counts() {
        let mut t = SessionTable::new();
        t.session_open(1, 0, false);
        let g = t.session_open(2, 0, true).unwrap();
        session_lock(&mut t, g);
        let (a, s, l, gu) = session_metrics(&t);
        assert_eq!((a, s, l, gu), (1, 0, 1, 1));
    }

    #[test]
    fn a366_doc_descriptor() {
        let d = session_doc();
        assert_eq!(d.domain, "aurora-session");
        assert_eq!((d.first, d.last, d.count), (351, 375, 25));
    }

    #[test]
    fn a367_progress_monotonic_and_real() {
        assert_eq!(boot_progress_pct(false, false, false), 0);
        assert_eq!(boot_progress_pct(true, false, false), 34);
        assert_eq!(boot_progress_pct(true, true, false), 67);
        assert_eq!(boot_progress_pct(true, true, true), 100);
        assert!(boot_progress_is_real(100, true));
        assert!(!boot_progress_is_real(100, false));
        assert!(boot_progress_monotonic(0, 34));
        assert!(!boot_progress_monotonic(100, 67));
    }

    #[test]
    fn a368_domain_tally_all_pass() {
        let (p, total) = session_domain_tally();
        assert_eq!(p, total);
        assert_eq!(total, 4);
    }

    #[test]
    fn a369_startup_selfcheck_end_to_end() {
        assert!(session_startup_selfcheck());
    }

    #[test]
    fn a370_perf_budget_typical_boot() {
        assert!(session_startup_perf_budget());
    }

    #[test]
    fn a371_startup_metrics_mixed() {
        let (a, _s, _l, g) = session_startup_metrics();
        assert_eq!(a, 1);
        assert_eq!(g, 1);
    }

    #[test]
    fn a372_fuzz_400_steps_invariants() {
        assert!(session_startup_fuzz());
    }

    #[test]
    fn a373_doc_ids_contiguous() {
        let ids = session_startup_doc();
        for (i, id) in ids.iter().enumerate() {
            assert_eq!(*id, (351 + i) as u16);
        }
    }

    #[test]
    fn a374_degrade_ladder() {
        assert_eq!(session_startup_degrade(true, true, true), DegradeLevel::Full);
        assert_eq!(session_startup_degrade(false, true, true), DegradeLevel::Reduced);
        assert_eq!(session_startup_degrade(true, false, true), DegradeLevel::Minimal);
        assert_eq!(session_startup_degrade(true, true, false), DegradeLevel::Safe);
    }

    #[test]
    fn a375_checkset_all_pass() {
        let set = run_session_checks();
for i in 0..set.len() { if let Some(c) = set.get(i) { if !c.passed { println!("DIAG a375 fail: {} : {}", c.name, c.detail); } } }
        assert!(set.len() >= 25);
        assert!(!set.truncated());
        assert!(set.all_passed(), "session self-check must pass");
    }
}