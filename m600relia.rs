//! m600relia — VARIX-M600 AI-04 可靠自愈域 (F076~F100)
//!
//! 崩溃现场全息/三分钟自愈环/沙盒化看门狗/服务化重启谱系/状态检查点库/
//! 灰度回滚舱/故障注入演习场/内核补丁热应用/健康分模型/异常指纹库/
//! 自愈剧本引擎/降级决策树/影子进程探测/数据抢救模式/文件系统自修复/
//! 驱动隔离舱/死锁舞者/内存腐蚀纠察/崩溃聚类报告/恢复演练日历/
//! 信任链自检/原子升级通道/回滚时间机器/故障博物馆/自愈年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F076 — 崩溃现场全息：IP/SP + 8 个通用寄存器一次定格
// ===========================================================================

pub const CRASH_REGS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrashScene {
    pub ip: u64,
    pub sp: u64,
    pub regs: [u64; CRASH_REGS],
    pub captured: bool,
}

impl CrashScene {
    pub const fn empty() -> CrashScene {
        CrashScene { ip: 0, sp: 0, regs: [0; CRASH_REGS], captured: false }
    }
}

/// 定格现场：IP 为 0 视为无效现场不予定格。
pub fn capture_scene(ip: u64, sp: u64, regs: &[u64; CRASH_REGS]) -> CrashScene {
    if ip == 0 {
        return CrashScene::empty();
    }
    CrashScene { ip, sp, regs: *regs, captured: true }
}

/// 现场可用于归因：已定格且 IP 非零。
pub fn scene_valid(s: &CrashScene) -> bool {
    s.captured && s.ip != 0
}

// ===========================================================================
// F077 — 三分钟自愈环：180s 窗口内最多 3 次自愈尝试
// ===========================================================================

pub const HEAL_WINDOW_MS: u32 = 180_000;
pub const HEAL_MAX_ATTEMPTS: u32 = 3;

#[derive(Clone, Copy, Debug, Default)]
pub struct HealRing {
    pub attempts: u32,
    pub window_ms: u32,
}

impl HealRing {
    /// 一次自愈尝试；窗口满 3 次后拒绝，窗口走完自动清零。
    pub fn attempt(&mut self) -> bool {
        if self.window_ms >= HEAL_WINDOW_MS {
            self.attempts = 0;
            self.window_ms = 0;
        }
        if self.attempts >= HEAL_MAX_ATTEMPTS {
            return false;
        }
        self.attempts += 1;
        true
    }

    /// 时间流逝。
    pub fn tick(&mut self, ms: u32) {
        self.window_ms += ms;
    }
}

// ===========================================================================
// F078 — 沙盒化看门狗：只能观测与喂狗，逾期即失效锁定
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct Watchdog {
    pub timeout_ms: u32,
    pub since_feed_ms: u32,
    pub expired: bool,
    pub pokes: u32,
}

impl Watchdog {
    pub const fn new(timeout_ms: u32) -> Watchdog {
        Watchdog { timeout_ms, since_feed_ms: 0, expired: false, pokes: 0 }
    }

    /// 喂狗：喂进来先看是否已逾期，逾期则锁定并拒绝。
    pub fn feed(&mut self, elapsed_ms: u32) -> bool {
        if self.expired {
            return false;
        }
        self.since_feed_ms += elapsed_ms;
        if self.since_feed_ms > self.timeout_ms {
            self.expired = true;
            return false;
        }
        self.since_feed_ms = 0;
        true
    }

    /// 沙盒内探测：看门狗失效后不再响应。
    pub fn poke(&mut self) -> bool {
        if self.expired {
            return false;
        }
        self.pokes += 1;
        true
    }
}

// ===========================================================================
// F079 — 服务化重启谱系：同代重启最多 5 次，越线升级上报
// ===========================================================================

pub const RESTART_GEN_MAX: u32 = 5;

#[derive(Clone, Copy, Debug, Default)]
pub struct RestartLineage {
    pub gen: u32,
    pub escalated: bool,
}

impl RestartLineage {
    /// 下一代重启；超过 5 代拒绝并标记升级。
    pub fn restart(&mut self) -> bool {
        if self.gen >= RESTART_GEN_MAX {
            self.escalated = true;
            return false;
        }
        self.gen += 1;
        true
    }
}

// ===========================================================================
// F080 — 状态检查点库：4 槽轮转，最新覆盖最旧
// ===========================================================================

pub const CHECKPOINT_SLOTS: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct CheckpointStore {
    vals: [u64; CHECKPOINT_SLOTS],
    head: usize,
    count: usize,
}

impl CheckpointStore {
    pub const fn new() -> CheckpointStore {
        CheckpointStore { vals: [0; CHECKPOINT_SLOTS], head: 0, count: 0 }
    }

    /// 存一个检查点；满后轮转覆盖最旧。
    pub fn save(&mut self, v: u64) {
        self.vals[self.head] = v;
        self.head = (self.head + 1) % CHECKPOINT_SLOTS;
        if self.count < CHECKPOINT_SLOTS {
            self.count += 1;
        }
    }

    /// 最新检查点。
    pub fn latest(&self) -> Option<u64> {
        if self.count == 0 {
            return None;
        }
        Some(self.vals[(self.head + CHECKPOINT_SLOTS - 1) % CHECKPOINT_SLOTS])
    }

    /// 往前数第 n 个（0 为最新）。
    pub fn nth_ago(&self, n: usize) -> Option<u64> {
        if n >= self.count {
            return None;
        }
        Some(self.vals[(self.head + CHECKPOINT_SLOTS - 1 - n) % CHECKPOINT_SLOTS])
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F081 — 灰度回滚舱：金丝雀失败超 20‰ 即回滚归零
// ===========================================================================

pub const CANARY_FAIL_LIMIT_PERMILLE: u32 = 20;

#[derive(Clone, Copy, Debug, Default)]
pub struct GrayRollout {
    pub percent: u32,
}

impl GrayRollout {
    /// 推进一步：健康则翻倍（封顶 100），越限回滚归零。
    pub fn step(&mut self, fail_permille: u32) -> u32 {
        if fail_permille > CANARY_FAIL_LIMIT_PERMILLE {
            self.percent = 0;
            return 0;
        }
        self.percent *= 2;
        if self.percent > 100 {
            self.percent = 100;
        }
        self.percent
    }

    pub fn rolled_back(&self) -> bool {
        self.percent == 0
    }
}

// ===========================================================================
// F082 — 故障注入演习场：每第 n 次操作注入一次故障
// ===========================================================================

/// 操作序号从 1 起算；nth 为 0 视为未配置注入。
pub const fn inject_nth(op_index: u64, nth: u64) -> bool {
    if nth == 0 {
        return false;
    }
    op_index != 0 && op_index % nth == 0
}

// ===========================================================================
// F083 — 内核补丁热应用：校验和、静默、回退路径三关齐过
// ===========================================================================

pub const fn patch_apply(crc_ok: bool, quiesced: bool, fallback_valid: bool) -> bool {
    crc_ok && quiesced && fallback_valid
}

// ===========================================================================
// F084 — 健康分模型：CPU 一半、内存三分之一、错误全额扣分
// ===========================================================================

pub const HEALTH_FULL_PERMILLE: u32 = 1000;

/// score = 1000 - cpu/2 - mem/3 - err（各项均 permille，饱和不下溢）。
pub const fn health_score_permille(cpu_p: u32, mem_p: u32, err_p: u32) -> u32 {
    let cost = cpu_p / 2 + mem_p / 3;
    let score = HEALTH_FULL_PERMILLE.saturating_sub(cost).saturating_sub(err_p);
    score
}

// ===========================================================================
// F085 — 异常指纹库：kind+site 定格指纹，注册去重
// ===========================================================================

pub const FINGERPRINT_CAP: usize = 8;

/// 指纹 = kind 左移 32 位拼 site。
pub const fn anomaly_fingerprint(kind: u8, site: u32) -> u64 {
    ((kind as u64) << 32) | site as u64
}

#[derive(Clone, Copy, Debug)]
pub struct FingerprintLib {
    sigs: [u64; FINGERPRINT_CAP],
    count: usize,
    pub dup_rejected: u32,
}

impl FingerprintLib {
    pub const fn new() -> FingerprintLib {
        FingerprintLib { sigs: [0; FINGERPRINT_CAP], count: 0, dup_rejected: 0 }
    }

    pub fn contains(&self, sig: u64) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.sigs[i] == sig {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 登记新指纹：重复拒绝记账，库满拒绝。
    pub fn register(&mut self, sig: u64) -> bool {
        if self.contains(sig) {
            self.dup_rejected += 1;
            return false;
        }
        if self.count >= FINGERPRINT_CAP {
            return false;
        }
        self.sigs[self.count] = sig;
        self.count += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F086 — 自愈剧本引擎：顺序执行，遇失败步即停
// ===========================================================================

/// 返回成功完成的步数；遇第一处 false 停下。
pub fn run_playbook(steps: &[bool]) -> u32 {
    let mut done = 0u32;
    let mut i = 0usize;
    while i < steps.len() {
        if !steps[i] {
            return done;
        }
        done += 1;
        i += 1;
    }
    done
}

// ===========================================================================
// F087 — 降级决策树：错误率与电量共同决定降级档
// ===========================================================================

pub const DEGRADE_ERR_HIGH_PERMILLE: u32 = 500;
pub const DEGRADE_ERR_MID_PERMILLE: u32 = 200;
pub const DEGRADE_BATT_LOW_PERMILLE: u32 = 200;
pub const DEGRADE_BATT_MID_PERMILLE: u32 = 400;

pub const fn degrade_tier(err_permille: u32, battery_permille: u32) -> u32 {
    if err_permille >= DEGRADE_ERR_HIGH_PERMILLE || battery_permille < DEGRADE_BATT_LOW_PERMILLE {
        3
    } else if err_permille >= DEGRADE_ERR_MID_PERMILLE
        || battery_permille < DEGRADE_BATT_MID_PERMILLE
    {
        2
    } else if err_permille > 0 {
        1
    } else {
        0
    }
}

// ===========================================================================
// F088 — 影子进程探测：影子输出与主输出逐次对拍
// ===========================================================================

#[derive(Clone, Copy, Debug, Default)]
pub struct ShadowProbe {
    pub checks: u32,
    pub divergences: u32,
}

impl ShadowProbe {
    pub fn compare(&mut self, primary_out: u64, shadow_out: u64) -> bool {
        self.checks += 1;
        if primary_out == shadow_out {
            true
        } else {
            self.divergences += 1;
            false
        }
    }

    pub fn healthy(&self) -> bool {
        self.divergences == 0
    }
}

// ===========================================================================
// F089 — 数据抢救模式：关键数据优先，预算先紧着关键
// ===========================================================================

/// 抢救配额：关键数据先足额保障，剩余预算才给普通数据。
pub const fn rescue_grant(critical: u32, normal: u32, budget: u32) -> u32 {
    let crit_taken = if critical > budget { budget } else { critical };
    let rest = budget - crit_taken;
    let normal_taken = if normal > rest { rest } else { normal };
    crit_taken + normal_taken
}

/// 关键数据是否有保障：预算至少覆盖全部关键数据。
pub const fn rescue_critical_covered(critical: u32, budget: u32) -> bool {
    critical <= budget
}

// ===========================================================================
// F090 — 文件系统自修复：日志顺序重放 + 自动修复开关
// ===========================================================================

pub const FS_JOURNAL_CAP: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct FsJournal {
    entries: [u32; FS_JOURNAL_CAP],
    len: usize,
}

impl FsJournal {
    pub const fn new() -> FsJournal {
        FsJournal { entries: [0; FS_JOURNAL_CAP], len: 0 }
    }

    /// 追加一条日志；满则拒绝。
    pub fn append(&mut self, entry: u32) -> bool {
        if self.len >= FS_JOURNAL_CAP {
            return false;
        }
        self.entries[self.len] = entry;
        self.len += 1;
        true
    }

    /// 按序重放并清空，返回重放条数。
    pub fn replay(&mut self) -> u32 {
        let n = self.len as u32;
        self.len = 0;
        n
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

/// 自动修复：开启时修复全部错误，关闭时一个不修。
pub const fn fsck_fix(errors: u32, auto_mode: bool) -> u32 {
    if auto_mode {
        errors
    } else {
        0
    }
}

// ===========================================================================
// F091 — 驱动隔离舱：连崩 3 次即隔离
// ===========================================================================

pub const DRIVER_CRASH_QUARANTINE: u32 = 3;

#[derive(Clone, Copy, Debug, Default)]
pub struct DriverCell {
    pub crashes: u32,
    pub quarantined: bool,
}

impl DriverCell {
    /// 记一次崩溃；达到 3 次进入隔离并保持。
    pub fn on_crash(&mut self) -> bool {
        if self.quarantined {
            return false;
        }
        self.crashes += 1;
        if self.crashes >= DRIVER_CRASH_QUARANTINE {
            self.quarantined = true;
        }
        !self.quarantined
    }

    pub fn revive(&mut self) -> bool {
        if self.quarantined {
            self.quarantined = false;
            self.crashes = 0;
            true
        } else {
            false
        }
    }
}

// ===========================================================================
// F092 — 死锁舞者：全局锁序约定，升序加锁、同锁重入拒绝
// ===========================================================================

/// 按 id 升序获取才安全；相等视为重入，一律拒绝。
pub const fn lock_order_ok(first: u32, second: u32) -> bool {
    first < second
}

// ===========================================================================
// F093 — 内存腐蚀纠察：金丝雀字节 + ECC 可纠正上限
// ===========================================================================

pub const CANARY_BYTE: u8 = 0xA5;
pub const ECC_CORRECTABLE_MAX: u32 = 3;

pub const fn canary_ok(byte: u8) -> bool {
    byte == CANARY_BYTE
}

/// ECC 错误在可纠正上限内即健康，越线判致命。
pub const fn ecc_verdict(ecc_errors: u32) -> bool {
    ecc_errors <= ECC_CORRECTABLE_MAX
}

// ===========================================================================
// F094 — 崩溃聚类报告：按指纹归簇，簇数即故障面
// ===========================================================================

/// 统计不同指纹数（簇数）。
pub fn distinct_clusters(sigs: &[u64]) -> usize {
    let mut clusters = 0usize;
    let mut i = 0usize;
    while i < sigs.len() {
        let mut seen = false;
        let mut j = 0usize;
        while j < i {
            if sigs[j] == sigs[i] {
                seen = true;
            }
            j += 1;
        }
        if !seen {
            clusters += 1;
        }
        i += 1;
    }
    clusters
}

/// 最大簇的规模。
pub fn biggest_cluster(sigs: &[u64]) -> usize {
    let mut best = 0usize;
    let mut i = 0usize;
    while i < sigs.len() {
        let mut n = 0usize;
        let mut j = 0usize;
        while j < sigs.len() {
            if sigs[j] == sigs[i] {
                n += 1;
            }
            j += 1;
        }
        if n > best {
            best = n;
        }
        i += 1;
    }
    best
}

// ===========================================================================
// F095 — 恢复演练日历：30 天一练，逾期记账
// ===========================================================================

pub const DRILL_INTERVAL_DAYS: u32 = 30;

pub const fn drill_due(days_since_last: u32) -> bool {
    days_since_last >= DRILL_INTERVAL_DAYS
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DrillCalendar {
    pub drills_done: u32,
    pub overdue: u32,
}

impl DrillCalendar {
    /// 记一次演练日：到期即逾期记账（无论是否真练了），练了另记一笔。
    pub fn mark_day(&mut self, days_since_last: u32, drilled: bool) {
        if drill_due(days_since_last) {
            self.overdue += 1;
        }
        if drilled {
            self.drills_done += 1;
        }
    }
}

// ===========================================================================
// F096 — 信任链自检：链上任何一环破即全链不可信
// ===========================================================================

pub fn trust_chain(links: &[bool]) -> bool {
    let mut i = 0usize;
    while i < links.len() {
        if !links[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 第一处断裂位置；无断裂返回 None。
pub fn first_break(links: &[bool]) -> Option<usize> {
    let mut i = 0usize;
    while i < links.len() {
        if !links[i] {
            return Some(i);
        }
        i += 1;
    }
    None
}

// ===========================================================================
// F097 — 原子升级通道：A/B 双槽，只切向有效槽，当前槽常留
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct UpgradeChannel {
    pub active: u8,
    pub valid: [bool; 2],
}

impl UpgradeChannel {
    pub const fn new() -> UpgradeChannel {
        UpgradeChannel { active: 0, valid: [true, false] }
    }

    /// 标记备用槽有效（升级写入并校验通过）。
    pub fn mark_valid(&mut self, slot: u8) -> bool {
        if slot < 2 && slot != self.active {
            self.valid[slot as usize] = true;
            true
        } else {
            false
        }
    }

    /// 原子切换到另一槽；目标槽无效则原地不动。
    pub fn switch_slot(&mut self) -> bool {
        let other = (1 - self.active) as usize;
        if self.valid[other] {
            self.active = other as u8;
            true
        } else {
            false
        }
    }
}

// ===========================================================================
// F098 — 回滚时间机器：保留最近 4 个快照，任意回拨
// ===========================================================================

pub const TIME_MACHINE_SNAPS: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct TimeMachine {
    snaps: [u64; TIME_MACHINE_SNAPS],
    len: usize,
}

impl TimeMachine {
    pub const fn new() -> TimeMachine {
        TimeMachine { snaps: [0; TIME_MACHINE_SNAPS], len: 0 }
    }

    /// 存快照；满则挤掉最旧。
    pub fn push(&mut self, v: u64) {
        if self.len < TIME_MACHINE_SNAPS {
            self.snaps[self.len] = v;
            self.len += 1;
        } else {
            let mut i = 1usize;
            while i < TIME_MACHINE_SNAPS {
                self.snaps[i - 1] = self.snaps[i];
                i += 1;
            }
            self.snaps[TIME_MACHINE_SNAPS - 1] = v;
        }
    }

    /// 往前数第 n 个快照（0 为最新）；超出记忆返回 None。
    pub fn rollback(&self, n: usize) -> Option<u64> {
        if n >= self.len {
            return None;
        }
        Some(self.snaps[self.len - 1 - n])
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

// ===========================================================================
// F099 — 故障博物馆：签名去重收藏，容量 16
// ===========================================================================

pub const MUSEUM_CAP: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct FaultMuseum {
    sigs: [u64; MUSEUM_CAP],
    count: usize,
    pub dup_rejected: u32,
}

impl FaultMuseum {
    pub const fn new() -> FaultMuseum {
        FaultMuseum { sigs: [0; MUSEUM_CAP], count: 0, dup_rejected: 0 }
    }

    pub fn exhibited(&self, sig: u64) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.sigs[i] == sig {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 收藏一件故障；重复拒绝记账，馆满拒绝。
    pub fn exhibit(&mut self, sig: u64) -> bool {
        if self.exhibited(sig) {
            self.dup_rejected += 1;
            return false;
        }
        if self.count >= MUSEUM_CAP {
            return false;
        }
        self.sigs[self.count] = sig;
        self.count += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F100 — 自愈年报：章节完备性 + 年度总量账
// ===========================================================================

pub const RELI_REPORT_SECTIONS: [&str; 5] =
    ["crash_scenes", "heal_loop", "checkpoints", "rollouts", "drills"];

#[derive(Clone, Copy, Debug, Default)]
pub struct HealYear {
    pub crash_scenes: u64,
    pub heal_attempts: u64,
    pub rollbacks: u32,
}

impl HealYear {
    pub fn record_scene(&mut self) {
        self.crash_scenes += 1;
    }
    pub fn record_heal(&mut self) {
        self.heal_attempts += 1;
    }
    pub fn record_rollback(&mut self) {
        self.rollbacks += 1;
    }
}

pub fn relia_report_complete(sections_filled: u32) -> bool {
    sections_filled >= RELI_REPORT_SECTIONS.len() as u32
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m600relia_checks() -> CheckSet {
    let mut set = CheckSet::new("m600relia");

    // F076 崩溃现场全息
    let regs = [0xDEADu64, 1, 2, 3, 4, 5, 6, 7];
    let scene = capture_scene(0x1000, 0x8000, &regs);
    let reg0 = scene.regs[0];
    set.add(
        "F076 scene capture",
        scene_valid(&scene) && scene.ip == 0x1000 && reg0 == 0xDEAD,
        "ip/sp/regs frozen",
    );
    let bad = capture_scene(0, 0x8000, &regs);
    set.add(
        "F076 scene rejects null ip",
        !scene_valid(&bad) && !bad.captured,
        "ip 0 refused",
    );

    // F077 三分钟自愈环
    let mut hr = HealRing::default();
    let h1 = hr.attempt();
    let h2 = hr.attempt();
    let h3 = hr.attempt();
    let attempts3 = hr.attempts;
    let h4 = hr.attempt();
    let attempts_after4 = hr.attempts;
    set.add(
        "F077 heal ring cap",
        h1 && h2 && h3 && attempts3 == 3 && !h4 && attempts_after4 == 3,
        "3 tries then stall",
    );
    hr.tick(HEAL_WINDOW_MS);
    let h5 = hr.attempt();
    let attempts_fresh = hr.attempts;
    set.add(
        "F077 heal ring reset",
        h5 && attempts_fresh == 1,
        "window elapsed re-arms",
    );

    // F078 沙盒化看门狗
    let mut wd = Watchdog::new(1000);
    let f1 = wd.feed(600);
    let since_mid = wd.since_feed_ms;
    let f2 = wd.feed(500);
    let f3 = wd.feed(1100);
    let expired_after = wd.expired;
    set.add(
        "F078 watchdog timeout",
        f1 && since_mid == 0 && f2 && !f3 && expired_after,
        "1100ms exceeds 1000ms",
    );
    let poke_dead = wd.poke();
    set.add("F078 watchdog locked", !poke_dead, "expired dog ignores pokes");
    let mut wd2 = Watchdog::new(1000);
    let f3 = wd2.feed(400);
    let p1 = wd2.poke();
    let pokes1 = wd2.pokes;
    set.add(
        "F078 watchdog alive",
        f3 && p1 && pokes1 == 1 && !wd2.expired,
        "healthy dog responds",
    );

    // F079 服务化重启谱系
    let mut rl = RestartLineage::default();
    let mut i = 0;
    let mut all_ok = true;
    while i < 5 {
        if !rl.restart() {
            all_ok = false;
        }
        i += 1;
    }
    let gen5 = rl.gen;
    let r6 = rl.restart();
    set.add(
        "F079 restart lineage",
        all_ok && gen5 == 5 && !r6 && rl.escalated,
        "five gens then escalate",
    );

    // F080 状态检查点库
    let mut cps = CheckpointStore::new();
    let empty = cps.latest();
    cps.save(1);
    cps.save(2);
    cps.save(3);
    let latest3 = cps.latest();
    let ago1 = cps.nth_ago(1);
    set.add(
        "F080 checkpoint latest",
        empty.is_none() && latest3 == Some(3) && ago1 == Some(2) && cps.len() == 3,
        "lifo reads",
    );
    cps.save(4);
    cps.save(5);
    let latest5 = cps.latest();
    let oldest = cps.nth_ago(3);
    let beyond = cps.nth_ago(4);
    set.add(
        "F080 checkpoint rotate",
        cps.len() == 4 && latest5 == Some(5) && oldest == Some(2) && beyond.is_none(),
        "oldest overwritten first",
    );

    // F081 灰度回滚舱
    let mut roll = GrayRollout { percent: 1 };
    let s1 = roll.step(0);
    let s2 = roll.step(0);
    set.add(
        "F081 rollout doubling",
        s1 == 2 && s2 == 4 && !roll.rolled_back(),
        "healthy canary doubles",
    );
    let s3 = roll.step(21);
    set.add(
        "F081 rollout rollback",
        s3 == 0 && roll.rolled_back(),
        "21 permille fails the gate",
    );
    let mut roll2 = GrayRollout { percent: 64 };
    let s4 = roll2.step(0);
    set.add("F081 rollout cap", s4 == 100, "doubles clamp at 100");

    // F082 故障注入演习场
    set.add(
        "F082 inject cadence",
        inject_nth(4, 4) && !inject_nth(3, 4) && inject_nth(8, 4),
        "every 4th op trips",
    );
    set.add(
        "F082 inject guards",
        !inject_nth(5, 0) && !inject_nth(0, 4),
        "no config or op 0, no fault",
    );

    // F083 内核补丁热应用
    set.add(
        "F083 patch gate",
        patch_apply(true, true, true) && !patch_apply(false, true, true)
            && !patch_apply(true, false, true) && !patch_apply(true, true, false),
        "all three conditions hold",
    );

    // F084 健康分模型
    set.add(
        "F084 health full",
        health_score_permille(0, 0, 0) == 1000 && health_score_permille(400, 0, 0) == 800,
        "cpu weighs half",
    );
    set.add(
        "F084 health blended",
        health_score_permille(400, 300, 100) == 600,
        "200+100+100 deducted",
    );
    set.add(
        "F084 health floor",
        health_score_permille(1000, 1000, 1000) == 0,
        "saturates at zero",
    );

    // F085 异常指纹库
    let fp = anomaly_fingerprint(3, 77);
    set.add(
        "F085 fingerprint packing",
        fp == ((3u64 << 32) | 77) && anomaly_fingerprint(0, 0) == 0,
        "kind:site packed",
    );
    let mut lib = FingerprintLib::new();
    let r1 = lib.register(fp);
    let r_dup = lib.register(fp);
    let dup_after = lib.dup_rejected;
    let mut j = 0;
    while j < 7 {
        lib.register(anomaly_fingerprint(0, (j + 1) as u32));
        j += 1;
    }
    let lib_len = lib.len();
    let r_over = lib.register(anomaly_fingerprint(9, 9));
    set.add(
        "F085 fingerprint dedupe cap",
        r1 && !r_dup && dup_after == 1 && lib_len == 8 && !r_over,
        "dedupe then capacity",
    );

    // F086 自愈剧本引擎
    set.add(
        "F086 playbook abort",
        run_playbook(&[true, true, false, true]) == 2,
        "stops at failed step",
    );
    set.add(
        "F086 playbook full and empty",
        run_playbook(&[true, true, true]) == 3 && run_playbook(&[]) == 0,
        "boundary lengths",
    );

    // F087 降级决策树
    set.add(
        "F087 degrade error side",
        degrade_tier(0, 1000) == 0 && degrade_tier(100, 1000) == 1
            && degrade_tier(200, 1000) == 2 && degrade_tier(500, 1000) == 3,
        "errors climb tiers",
    );
    set.add(
        "F087 degrade battery side",
        degrade_tier(0, 399) == 2 && degrade_tier(0, 199) == 3,
        "battery forces tiers",
    );

    // F088 影子进程探测
    let mut probe = ShadowProbe::default();
    let m1 = probe.compare(5, 5);
    let m2 = probe.compare(5, 6);
    let div1 = probe.divergences;
    set.add(
        "F088 shadow divergence",
        m1 && !m2 && div1 == 1 && !probe.healthy(),
        "mismatch booked",
    );
    let mut probe2 = ShadowProbe::default();
    probe2.compare(1, 1);
    probe2.compare(2, 2);
    let checks2 = probe2.checks;
    set.add("F088 shadow healthy", checks2 == 2 && probe2.healthy(), "clean twin");

    // F089 数据抢救模式
    set.add(
        "F089 rescue priority",
        rescue_grant(3, 7, 5) == 5 && rescue_grant(3, 7, 2) == 2,
        "criticals first, budget binds",
    );
    set.add(
        "F089 rescue coverage",
        rescue_critical_covered(3, 5) && !rescue_critical_covered(5, 3),
        "budget must cover criticals",
    );

    // F090 文件系统自修复
    let mut jr = FsJournal::new();
    jr.append(11);
    jr.append(12);
    jr.append(13);
    let before = jr.len();
    let replayed = jr.replay();
    let after = jr.len();
    set.add(
        "F090 journal replay",
        before == 3 && replayed == 3 && after == 0,
        "replay drains in order",
    );
    let mut jr2 = FsJournal::new();
    let mut k = 0;
    while k < FS_JOURNAL_CAP {
        jr2.append(k as u32);
        k += 1;
    }
    let jr_len = jr2.len();
    let over = jr2.append(99);
    set.add(
        "F090 journal cap",
        jr_len == FS_JOURNAL_CAP && !over,
        "8 entries max",
    );
    set.add(
        "F090 fsck auto mode",
        fsck_fix(5, true) == 5 && fsck_fix(5, false) == 0,
        "auto flag gates fixes",
    );

    // F091 驱动隔离舱
    let mut cell = DriverCell::default();
    let c1 = cell.on_crash();
    let c2 = cell.on_crash();
    let crashes2 = cell.crashes;
    let c3 = cell.on_crash();
    let quarantined = cell.quarantined;
    let c4 = cell.on_crash();
    set.add(
        "F091 driver quarantine",
        c1 && c2 && crashes2 == 2 && !c3 && quarantined && !c4,
        "third crash seals the cell",
    );
    let rev = cell.revive();
    let crashes0 = cell.crashes;
    let rev_again = cell.revive();
    set.add(
        "F091 driver revive",
        rev && crashes0 == 0 && !rev_again && !cell.quarantined,
        "revive clears once",
    );

    // F092 死锁舞者
    set.add(
        "F092 lock order",
        lock_order_ok(1, 2) && !lock_order_ok(2, 1) && !lock_order_ok(3, 3),
        "ascending only, no reentry",
    );

    // F093 内存腐蚀纠察
    set.add(
        "F093 canary byte",
        canary_ok(0xA5) && !canary_ok(0x5A) && !canary_ok(0),
        "0xA5 untouched",
    );
    set.add(
        "F093 ecc verdict",
        ecc_verdict(0) && ecc_verdict(3) && !ecc_verdict(4),
        "correctable up to 3",
    );

    // F094 崩溃聚类报告
    let sigs = [5u64, 5, 7, 9, 7];
    set.add(
        "F094 cluster count",
        distinct_clusters(&sigs) == 3 && distinct_clusters(&[]) == 0,
        "distinct fingerprints",
    );
    set.add("F094 biggest cluster", biggest_cluster(&sigs) == 2, "5 and 7 tie at two");

    // F095 恢复演练日历
    set.add(
        "F095 drill due",
        !drill_due(29) && drill_due(30) && drill_due(45),
        "30-day cadence",
    );
    let mut cal = DrillCalendar::default();
    cal.mark_day(45, true);
    cal.mark_day(5, true);
    cal.mark_day(30, false);
    let overdue_n = cal.overdue;
    let drills_n = cal.drills_done;
    set.add(
        "F095 drill calendar",
        overdue_n == 2 && drills_n == 2,
        "overdue booked regardless",
    );

    // F096 信任链自检
    let good_chain = [true, true, true];
    let broken_chain = [true, false, true];
    let brk = first_break(&broken_chain);
    set.add(
        "F096 trust chain",
        trust_chain(&good_chain) && !trust_chain(&broken_chain) && brk == Some(1),
        "one break poisons all",
    );
    set.add("F096 trust intact", first_break(&good_chain).is_none(), "no break found");

    // F097 原子升级通道
    let mut ch = UpgradeChannel::new();
    let sw_bad = ch.switch_slot();
    let mark = ch.mark_valid(1);
    let sw1 = ch.switch_slot();
    let active1 = ch.active;
    set.add(
        "F097 atomic switch",
        !sw_bad && mark && sw1 && active1 == 1,
        "only into valid slots",
    );
    let mark_active = ch.mark_valid(1);
    let sw2 = ch.switch_slot();
    let active2 = ch.active;
    set.add(
        "F097 atomic round trip",
        !mark_active && sw2 && active2 == 0,
        "a-slot stays valid for return",
    );

    // F098 回滚时间机器
    let mut tm = TimeMachine::new();
    let mut v = 1u64;
    while v <= 5 {
        tm.push(v);
        v += 1;
    }
    let tm_len = tm.len();
    let r0 = tm.rollback(0);
    let r3 = tm.rollback(3);
    let r4 = tm.rollback(4);
    set.add(
        "F098 time machine memory",
        tm_len == 4 && r0 == Some(5) && r3 == Some(2) && r4.is_none(),
        "keeps last 4 only",
    );
    tm.push(6);
    let r0b = tm.rollback(0);
    let r3b = tm.rollback(3);
    set.add(
        "F098 time machine shift",
        r0b == Some(6) && r3b == Some(3),
        "oldest evicted on push",
    );

    // F099 故障博物馆
    let mut mus = FaultMuseum::new();
    let e1 = mus.exhibit(1);
    let e_dup = mus.exhibit(1);
    let dup_after = mus.dup_rejected;
    let mut w = 2u64;
    while w <= 16 {
        mus.exhibit(w);
        w += 1;
    }
    let mus_len = mus.len();
    let e_over = mus.exhibit(99);
    set.add(
        "F099 museum dedupe cap",
        e1 && !e_dup && dup_after == 1 && mus_len == 16 && !e_over && mus.exhibited(7),
        "16 exhibits, dupes refused",
    );

    // F100 自愈年报
    let mut year = HealYear::default();
    year.record_scene();
    year.record_scene();
    year.record_heal();
    year.record_heal();
    year.record_heal();
    year.record_rollback();
    set.add(
        "F100 relia yearbook",
        RELI_REPORT_SECTIONS.len() == 5 && year.crash_scenes == 2
            && year.heal_attempts == 3 && year.rollbacks == 1,
        "yearly totals accounted",
    );
    set.add(
        "F100 relia report sections",
        relia_report_complete(5) && !relia_report_complete(4),
        "five sections required",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f077_heal_ring_window() {
        let mut hr = HealRing::default();
        assert!(hr.attempt());
        assert!(hr.attempt());
        assert!(hr.attempt());
        assert!(!hr.attempt());
        assert_eq!(hr.attempts, 3);
        hr.tick(HEAL_WINDOW_MS);
        assert!(hr.attempt());
        assert_eq!(hr.attempts, 1);
    }

    #[test]
    fn f080_checkpoint_round_robin() {
        let mut cps = CheckpointStore::new();
        let mut i = 1u64;
        while i <= 6 {
            cps.save(i);
            i += 1;
        }
        assert_eq!(cps.len(), 4);
        assert_eq!(cps.latest(), Some(6));
        assert_eq!(cps.nth_ago(3), Some(3));
        assert!(cps.nth_ago(4).is_none());
    }

    #[test]
    fn f084_health_score_boundaries() {
        assert_eq!(health_score_permille(0, 0, 0), 1000);
        assert_eq!(health_score_permille(400, 300, 100), 600);
        assert_eq!(health_score_permille(1000, 1000, 1000), 0);
        assert_eq!(health_score_permille(2000, 0, 0), 0);
    }

    #[test]
    fn f091_driver_cell_lifecycle() {
        let mut cell = DriverCell::default();
        assert!(cell.on_crash());
        assert!(cell.on_crash());
        assert!(!cell.on_crash());
        assert!(cell.quarantined);
        assert!(cell.revive());
        assert_eq!(cell.crashes, 0);
        assert!(!cell.revive());
    }

    #[test]
    fn f098_time_machine_keeps_four() {
        let mut tm = TimeMachine::new();
        let mut i = 1u64;
        while i <= 5 {
            tm.push(i);
            i += 1;
        }
        assert_eq!(tm.rollback(0), Some(5));
        assert_eq!(tm.rollback(3), Some(2));
        assert!(tm.rollback(4).is_none());
        tm.push(6);
        assert_eq!(tm.rollback(3), Some(3));
    }

    #[test]
    fn m600relia_selfcheck_all_pass() {
        let set = run_m600relia_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
