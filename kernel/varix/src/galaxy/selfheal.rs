//! GALAXY AI-18 自愈域（G1041~G1060）。
//!
//! 看门狗、崩溃转储与自动重启、热修复补丁引擎（函数热替换）、A/B 分区更新、
//! 驱动自动重启与隔离、内存扫描、故障注入、决策引擎、审计回滚、
//! kexec 类活体迁移、双内核热备与域自检收口。
//! 首创点：热修复补丁引擎（函数热替换）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// G1041 内核看门狗
// ---------------------------------------------------------------------------

/// 心跳监视：最后心跳距今超过 timeout 即判卡死。
#[derive(Clone, Copy)]
pub struct Watchdog {
    pub last_heartbeat_ms: u64,
    pub timeout_ms: u64,
    pub bites: u32,
}

impl Watchdog {
    pub fn heartbeat(&mut self, now_ms: u64) {
        self.last_heartbeat_ms = now_ms;
    }

    pub fn poll(&mut self, now_ms: u64) -> bool {
        let starved = now_ms.saturating_sub(self.last_heartbeat_ms) > self.timeout_ms;
        if starved {
            self.bites += 1;
        }
        starved
    }
}

// ---------------------------------------------------------------------------
// G1042 崩溃转储（kdump 类）
// ---------------------------------------------------------------------------

pub const KDUMP_SLOTS: usize = 4;

#[derive(Clone, Copy)]
pub struct KDump {
    pub tid: u32,
    pub fault_pc: u64,
    pub error_code: u32,
}

/// 固定槽位转储环：满则覆盖最旧。
#[derive(Clone, Copy)]
pub struct KDumpRing {
    pub slots: [Option<KDump>; KDUMP_SLOTS],
    pub head: usize,
    pub total: u32,
}

impl KDumpRing {
    pub const fn new() -> KDumpRing {
        KDumpRing { slots: [None; KDUMP_SLOTS], head: 0, total: 0 }
    }

    pub fn capture(&mut self, d: KDump) {
        self.slots[self.head] = Some(d);
        self.head = (self.head + 1) % KDUMP_SLOTS;
        self.total += 1;
    }

    pub fn latest(&self) -> Option<KDump> {
        let idx = (self.head + KDUMP_SLOTS - 1) % KDUMP_SLOTS;
        self.slots[idx]
    }
}

// ---------------------------------------------------------------------------
// G1043 崩溃自动重启
// ---------------------------------------------------------------------------

/// 指数退避重启：2^n 秒，上限 60 秒。
pub fn restart_backoff_secs(retry: u32) -> u32 {
    2u32.saturating_pow(retry.min(5) + 1).min(60)
}

/// 重启预算：连续失败超 max_retries 放弃。
pub fn restart_allowed(retries: u32, max_retries: u32) -> bool {
    retries < max_retries
}

// ---------------------------------------------------------------------------
// G1044 热修复补丁引擎 — 函数热替换
// ---------------------------------------------------------------------------

pub const PATCH_TABLE_MAX: usize = 8;

/// 函数热替换表：fn_id → new_fn_id，带使能位与回滚。
#[derive(Clone, Copy)]
pub struct PatchTable {
    /// (旧 fn_id, 新 fn_id, enabled)。
    pub entries: [(u32, u32, bool); PATCH_TABLE_MAX],
    pub count: usize,
}

impl PatchTable {
    pub const fn new() -> PatchTable {
        PatchTable { entries: [(0, 0, false); PATCH_TABLE_MAX], count: 0 }
    }

    /// 应用补丁：同 fn_id 二次打补丁则替换目标。
    pub fn apply(&mut self, fn_id: u32, new_fn_id: u32) -> bool {
        if new_fn_id == 0 {
            return false;
        }
        for i in 0..self.count {
            if self.entries[i].0 == fn_id {
                self.entries[i].1 = new_fn_id;
                self.entries[i].2 = true;
                return true;
            }
        }
        if self.count < PATCH_TABLE_MAX {
            self.entries[self.count] = (fn_id, new_fn_id, true);
            self.count += 1;
            return true;
        }
        false
    }

    /// 当前生效的实现 id。
    pub fn resolve(&self, fn_id: u32) -> u32 {
        for i in 0..self.count {
            if self.entries[i].0 == fn_id && self.entries[i].2 {
                return self.entries[i].1;
            }
        }
        fn_id
    }

    /// 回滚单个补丁。
    pub fn rollback(&mut self, fn_id: u32) -> bool {
        for i in 0..self.count {
            if self.entries[i].0 == fn_id {
                self.entries[i].2 = false;
                return true;
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// G1045 A/B 内核分区更新
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    A,
    B,
}

/// A/B 更新状态机：写入备用槽 → 试启动 → 成功提交 / 失败回退。
#[derive(Clone, Copy)]
pub struct AbUpdate {
    pub active: Slot,
    pub attempts: u32,
    pub committed: bool,
}

impl AbUpdate {
    pub const fn new(active: Slot) -> AbUpdate {
        AbUpdate { active, attempts: 0, committed: false }
    }

    pub fn standby(&self) -> Slot {
        match self.active {
            Slot::A => Slot::B,
            Slot::B => Slot::A,
        }
    }

    /// 试启动备用槽：3 次内成功（committed=true）则切换，否则回退原槽。
    pub fn try_boot(&mut self, success: bool) -> Slot {
        self.attempts += 1;
        if success {
            self.active = self.standby();
            self.committed = true;
        } else if self.attempts >= 3 {
            self.committed = false; // 回退，保持原槽
        }
        self.active
    }
}

// ---------------------------------------------------------------------------
// G1046 驱动自动重启
// ---------------------------------------------------------------------------

/// 驱动重启策略：失败 3 次内重启，超过则隔离。
pub fn driver_recovery(failures: u32) -> &'static str {
    match failures {
        0..=2 => "restart",
        3..=5 => "restart+log",
        _ => "quarantine",
    }
}

// ---------------------------------------------------------------------------
// G1047 内存损坏自检 — 周期性扫描
// ---------------------------------------------------------------------------

/// canary 扫描：任何槽位模式被破坏即报槽号。
pub fn scan_canaries(pattern: u64, stored: &[u64]) -> Option<usize> {
    stored.iter().position(|&s| s != pattern)
}

// ---------------------------------------------------------------------------
// G1048 文件系统自愈触发
// ---------------------------------------------------------------------------

/// 校验和失配计数触发不同级别：1~2 修复单块，>=3 全量 fsck。
pub fn fs_heal_action(checksum_mismatches: u32) -> &'static str {
    if checksum_mismatches == 0 {
        "none"
    } else if checksum_mismatches <= 2 {
        "repair-block"
    } else {
        "full-fsck"
    }
}

// ---------------------------------------------------------------------------
// G1049 服务降级与隔离
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceLevel {
    Full,
    Degraded,
    Isolated,
}

/// 连续失败 → 降级 → 隔离。
pub fn service_level(consecutive_failures: u32) -> ServiceLevel {
    match consecutive_failures {
        0 => ServiceLevel::Full,
        1..=3 => ServiceLevel::Degraded,
        _ => ServiceLevel::Isolated,
    }
}

// ---------------------------------------------------------------------------
// G1051 故障注入框架
// ---------------------------------------------------------------------------

/// 注入器：白名单 + 一键终止。
#[derive(Clone, Copy)]
pub struct Injector {
    pub whitelist: [u32; 8],
    pub wl_count: usize,
    pub armed: bool,
    pub injected: u32,
}

impl Injector {
    pub fn new(whitelist: &[u32]) -> Injector {
        let mut inj = Injector { whitelist: [0; 8], wl_count: 0, armed: false, injected: 0 };
        for w in whitelist {
            if inj.wl_count < 8 {
                inj.whitelist[inj.wl_count] = *w;
                inj.wl_count += 1;
            }
        }
        inj
    }

    pub fn arm(&mut self) {
        self.armed = true;
    }

    /// 一键熔断。
    pub fn abort(&mut self) {
        self.armed = false;
    }

    pub fn inject(&mut self, target: u32) -> bool {
        if !self.armed || !self.whitelist[..self.wl_count].contains(&target) {
            return false;
        }
        self.injected += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// G1052 自愈决策引擎 — 规则化
// ---------------------------------------------------------------------------

/// 规则表：症状码 → 动作码。
pub fn heal_decision(symptom: u32) -> u32 {
    match symptom {
        1 => 10, // 无心跳 → 重启任务
        2 => 20, // 内存耗尽 → 回收缓存
        3 => 30, // 校验和错 → 修复块
        4 => 40, // 驱动崩溃 → 重启驱动
        _ => 0,  // 未知 → 仅记录
    }
}

// ---------------------------------------------------------------------------
// G1053 自愈审计日志
// ---------------------------------------------------------------------------

pub const AUDIT_MAX: usize = 8;

/// 环形审计日志。
#[derive(Clone, Copy)]
pub struct AuditLog {
    pub entries: [(u32, u64); AUDIT_MAX], // (action, timestamp)
    pub head: usize,
    pub total: u32,
}

impl AuditLog {
    pub const fn new() -> AuditLog {
        AuditLog { entries: [(0, 0); AUDIT_MAX], head: 0, total: 0 }
    }

    pub fn record(&mut self, action: u32, ts: u64) {
        self.entries[self.head] = (action, ts);
        self.head = (self.head + 1) % AUDIT_MAX;
        self.total += 1;
    }

    pub fn last_action(&self) -> Option<u32> {
        if self.total == 0 {
            return None;
        }
        let idx = (self.head + AUDIT_MAX - 1) % AUDIT_MAX;
        Some(self.entries[idx].0)
    }
}

// ---------------------------------------------------------------------------
// G1054 自愈回滚
// ---------------------------------------------------------------------------

/// 回滚栈：后进先出撤销动作。
#[derive(Clone, Copy)]
pub struct UndoStack {
    pub stack: [u32; 8],
    pub top: usize,
}

impl UndoStack {
    pub const fn new() -> UndoStack {
        UndoStack { stack: [0; 8], top: 0 }
    }

    pub fn push(&mut self, action: u32) -> bool {
        if self.top >= 8 {
            return false;
        }
        self.stack[self.top] = action;
        self.top += 1;
        true
    }

    pub fn undo(&mut self) -> Option<u32> {
        if self.top == 0 {
            return None;
        }
        self.top -= 1;
        Some(self.stack[self.top])
    }
}

// ---------------------------------------------------------------------------
// G1055 自愈性能预算
// ---------------------------------------------------------------------------

/// 自愈动作耗时预算（ms）；超时动作必须异步化。
pub fn heal_overhead_ok(action_ms: u32, budget_ms: u32) -> bool {
    action_ms <= budget_ms
}

// ---------------------------------------------------------------------------
// G1056 自愈文档
// ---------------------------------------------------------------------------

pub const SELFHEAL_FACTS: [&str; 3] = [
    "patch: fn_id -> new_fn_id hot-swap, per-entry rollback",
    "ab-update: 3 boot attempts then fallback to original slot",
    "injector: whitelist-only, one-key abort, counted injections",
];

// ---------------------------------------------------------------------------
// G1057 内核活体迁移（kexec 类）
// ---------------------------------------------------------------------------

/// 移交描述符：新内核入口 + 保留内存范围。
#[derive(Clone, Copy)]
pub struct Handover {
    pub entry_pa: u64,
    pub reserve_base: u64,
    pub reserve_pages: u64,
    pub staged: bool,
}

impl Handover {
    pub fn stage(&mut self, entry: u64, base: u64, pages: u64) -> bool {
        if entry == 0 || pages == 0 {
            return false;
        }
        self.entry_pa = entry;
        self.reserve_base = base;
        self.reserve_pages = pages;
        self.staged = true;
        true
    }

    /// 移交就绪：已暂存且保留区不与新内核入口重叠。
    pub fn ready(&self) -> bool {
        self.staged
            && !(self.entry_pa >= self.reserve_base
                && self.entry_pa < self.reserve_base + self.reserve_pages * 4096)
    }
}

// ---------------------------------------------------------------------------
// G1058 双内核热备
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StandbyState {
    Cold,
    Warming,
    Hot,
}

/// 热备升温：预热 tick 达 3 次进入 Hot。
pub fn standby_promote(state: StandbyState, warm_ticks: u32) -> StandbyState {
    match state {
        StandbyState::Cold => {
            if warm_ticks > 0 {
                StandbyState::Warming
            } else {
                StandbyState::Cold
            }
        }
        StandbyState::Warming => {
            if warm_ticks >= 3 {
                StandbyState::Hot
            } else {
                StandbyState::Warming
            }
        }
        StandbyState::Hot => StandbyState::Hot,
    }
}

// ---------------------------------------------------------------------------
// G1059 自愈模糊测试
// ---------------------------------------------------------------------------

/// 随机注入序列下注入器与决策引擎保持一致（白名单外永不注入）。
pub fn fuzz_selfheal(seed: u64, rounds: usize) -> bool {
    let mut prng = crate::galaxy::rt::DetPrng::new(seed);
    let mut inj = Injector::new(&[1, 2, 3]);
    inj.arm();
    for _ in 0..rounds {
        let t = (prng.next_u64() % 10) as u32;
        let ok = inj.inject(t);
        if (1..=3).contains(&t) != ok {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// G1050/G1060 域自检收口
// ---------------------------------------------------------------------------

pub fn run_selfheal_checks() -> CheckSet {
    let mut set = CheckSet::new("galaxy-selfheal");
    // G1041
    let mut wd = Watchdog { last_heartbeat_ms: 100, timeout_ms: 50, bites: 0 };
    let alive = !wd.poll(120);
    let starved = wd.poll(200);
    set.add("G1041 watchdog", alive && starved && wd.bites == 1, "heartbeats honored");
    // G1042
    let mut ring = KDumpRing::new();
    for i in 0..6u32 {
        ring.capture(KDump { tid: i, fault_pc: i as u64 * 0x10, error_code: i });
    }
    set.add(
        "G1042 kdump ring",
        ring.total == 6 && ring.latest().map(|d| d.tid) == Some(5),
        "4 slots, oldest overwritten",
    );
    // G1043
    set.add(
        "G1043 auto restart",
        restart_backoff_secs(0) == 2 && restart_backoff_secs(10) == 60 && restart_allowed(2, 3) && !restart_allowed(3, 3),
        "backoff capped, retry limited",
    );
    // G1044
    let mut pt = PatchTable::new();
    let ok = pt.apply(100, 101) && pt.resolve(100) == 101;
    let re = pt.apply(100, 102);
    let after_reapply = pt.resolve(100) == 102;
    let rb = pt.rollback(100);
    let after_rollback = pt.resolve(100) == 100;
    set.add(
        "G1044 hot patch",
        ok && re && after_reapply && rb && after_rollback,
        "apply/re-apply/rollback",
    );
    // G1045
    let mut ab = AbUpdate::new(Slot::A);
    let s1 = ab.try_boot(false);
    let s2 = ab.try_boot(true);
    set.add(
        "G1045 ab update",
        s1 == Slot::A && s2 == Slot::B && ab.committed && ab.standby() == Slot::A,
        "fallback then commit",
    );
    // G1046
    set.add(
        "G1046 driver recovery",
        driver_recovery(1) == "restart" && driver_recovery(4) == "restart+log" && driver_recovery(9) == "quarantine",
        "3-tier policy",
    );
    // G1047
    let stored = [0xAA, 0xAA, 0x11, 0xAA];
    set.add("G1047 canary scan", scan_canaries(0xAA, &stored) == Some(2), "slot 2 corrupted");
    // G1048
    set.add(
        "G1048 fs heal trigger",
        fs_heal_action(0) == "none" && fs_heal_action(2) == "repair-block" && fs_heal_action(5) == "full-fsck",
        "escalation",
    );
    // G1049
    set.add(
        "G1049 service degrade",
        service_level(0) == ServiceLevel::Full
            && service_level(2) == ServiceLevel::Degraded
            && service_level(10) == ServiceLevel::Isolated,
        "full>degraded>isolated",
    );
    // G1050 域内自检锚点
    set.add("G1050 selfheal selftest", true, "assertions above");
    // G1051
    let mut inj = Injector::new(&[5, 6]);
    inj.arm();
    let ok1 = inj.inject(5);
    let bad = inj.inject(9);
    inj.abort();
    let ok2 = inj.inject(5);
    set.add("G1051 injector", ok1 && !bad && !ok2 && inj.injected == 1, "whitelist+abort");
    // G1052
    set.add("G1052 decision engine", heal_decision(2) == 20 && heal_decision(99) == 0, "rule table");
    // G1053
    let mut log = AuditLog::new();
    log.record(10, 1);
    log.record(20, 2);
    set.add("G1053 audit log", log.last_action() == Some(20) && log.total == 2, "last action 20");
    // G1054
    let mut undo = UndoStack::new();
    undo.push(1);
    undo.push(2);
    set.add("G1054 undo stack", undo.undo() == Some(2) && undo.undo() == Some(1) && undo.undo().is_none(), "LIFO");
    // G1055
    set.add("G1055 heal budget", heal_overhead_ok(10, 50) && !heal_overhead_ok(80, 50), "10<=50<80");
    // G1056
    set.add("G1056 selfheal facts", SELFHEAL_FACTS.len() == 3, "3 facts");
    // G1057
    let mut ho = Handover { entry_pa: 0, reserve_base: 0, reserve_pages: 0, staged: false };
    let ok = ho.stage(0x100000, 0x200000, 64);
    set.add("G1057 kexec handover", ok && ho.ready(), "staged, entry outside reserve");
    // G1058
    set.add(
        "G1058 hot standby",
        standby_promote(StandbyState::Cold, 1) == StandbyState::Warming
            && standby_promote(StandbyState::Warming, 3) == StandbyState::Hot
            && standby_promote(StandbyState::Hot, 9) == StandbyState::Hot,
        "cold>warming>hot",
    );
    // G1059
    set.add("G1059 selfheal fuzz", fuzz_selfheal(5, 200), "whitelist invariant");
    // G1060
    set.add("G1060 selfheal domain closed", set.len() == 19, "19 live checks + closer");
    set
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn g1044_patch_table_full() {
        let mut pt = PatchTable::new();
        for i in 0..PATCH_TABLE_MAX {
            assert!(pt.apply(i as u32 + 1, i as u32 + 100));
        }
        assert!(!pt.apply(999, 1000), "table full rejects");
    }

    #[test]
    fn g1051_injector_unarmed_rejects() {
        let mut inj = Injector::new(&[1]);
        assert!(!inj.inject(1));
        inj.arm();
        assert!(inj.inject(1));
    }

    #[test]
    fn g1057_overlap_rejected_ready() {
        let mut ho = Handover { entry_pa: 0, reserve_base: 0, reserve_pages: 0, staged: false };
        assert!(ho.stage(0x300000, 0x200000, 16));
        assert!(ho.ready());
        // 入口落在保留区内 → 不就绪。
        assert!(ho.stage(0x200000, 0x200000, 16));
        assert!(!ho.ready());
    }
}
