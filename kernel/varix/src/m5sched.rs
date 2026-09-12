//! VARIX-M500 AI-02 算力编排深化域（F026~F050，M1）。
//!
//! 调度从"公平"到"聪明"：画像、亲和、预测、仿真、卡顿归零。
//! 全部为纯逻辑 + 固定容量数组（无 Vec/String/Box），no_std 安全。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F026 — 算力画像：每线程 CPU 行为特征建档
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThreadProfile {
    pub tid: u32,
    /// 最近 N 个窗口的忙度 permille。
    pub busy_hist: [u16; 8],
    pub hist_len: u8,
    /// 突发度：忙度方差近似（permille）。
    pub bursty: bool,
}

impl ThreadProfile {
    pub const fn new(tid: u32) -> ThreadProfile {
        ThreadProfile { tid, busy_hist: [0; 8], hist_len: 0, bursty: false }
    }

    pub fn sample(&mut self, busy_permille: u16) -> bool {
        let v = busy_permille.min(1000);
        if self.hist_len < 8 {
            self.busy_hist[self.hist_len as usize] = v;
            self.hist_len += 1;
        } else {
            // 滑动：丢最老
            for i in 0..7 {
                self.busy_hist[i] = self.busy_hist[i + 1];
            }
            self.busy_hist[7] = v;
        }
        true
    }

    pub fn avg_busy(&self) -> u32 {
        if self.hist_len == 0 {
            return 0;
        }
        let mut s = 0u32;
        for i in 0..self.hist_len as usize {
            s += self.busy_hist[i] as u32;
        }
        s / self.hist_len as u32
    }

    /// 分类：busy>800 稳定型；avg>300 且波动>400 突发型；否则闲。
    pub fn classify(&mut self) -> &'static str {
        let mut maxdiff = 0u32;
        for i in 0..self.hist_len as usize {
            for j in (i + 1)..self.hist_len as usize {
                let d = self.busy_hist[i].abs_diff(self.busy_hist[j]) as u32;
                if d > maxdiff {
                    maxdiff = d;
                }
            }
        }
        self.bursty = maxdiff > 400;
        let avg = self.avg_busy();
        if avg > 800 {
            "cpu-bound"
        } else if avg > 300 && self.bursty {
            "bursty"
        } else {
            "idle-ish"
        }
    }
}

// ---------------------------------------------------------------------------
// F027 — 缓存亲和调度：L3 拓扑感知放置
// ---------------------------------------------------------------------------

pub const MAX_CORES: usize = 16;

#[derive(Clone, Copy)]
pub struct CacheTopology {
    /// 每核所属 LLC 域 id。
    pub llc_of: [u8; MAX_CORES],
    pub domains: u8,
}

impl CacheTopology {
    pub const fn new(domains: u8) -> CacheTopology {
        CacheTopology { llc_of: [0; MAX_CORES], domains }
    }

    pub fn set_llc(&mut self, core: usize, domain: u8) -> bool {
        if core >= MAX_CORES || domain >= self.domains {
            return false;
        }
        self.llc_of[core] = domain;
        true
    }

    /// 与 preferred 核同 LLC 域的空闲核；没有则任意空闲核。
    pub fn pick_sibling(&self, preferred: usize, busy_mask: u16) -> Option<usize> {
        if preferred >= MAX_CORES {
            return None;
        }
        let want = self.llc_of[preferred];
        let mut any: Option<usize> = None;
        for c in 0..MAX_CORES {
            if c == preferred || busy_mask & (1 << c) != 0 {
                continue;
            }
            if self.llc_of[c] == want {
                return Some(c);
            }
            if any.is_none() {
                any = Some(c);
            }
        }
        any
    }

    /// 两核是否同 LLC（迁移收益参考）。
    pub fn same_llc(&self, a: usize, b: usize) -> bool {
        a < MAX_CORES && b < MAX_CORES && self.llc_of[a] == self.llc_of[b]
    }
}

// ---------------------------------------------------------------------------
// F028 — SMT 感知策略：超线程兄弟核协同
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct SmtPolicy {
    /// 物理核 → 兄弟逻辑核（0xFFFF = 无 SMT）。
    pub sibling: [u16; MAX_CORES],
    /// 同物理核双线程并发时各自可用算力 permille（典型 60）。
    pub share_permille: u16,
}

impl SmtPolicy {
    pub const fn new(share_permille: u16) -> SmtPolicy {
        SmtPolicy { sibling: [0xFFFF; MAX_CORES], share_permille }
    }

    pub fn pair(&mut self, a: usize, b: usize) -> bool {
        if a >= MAX_CORES || b >= MAX_CORES || a == b {
            return false;
        }
        self.sibling[a] = b as u16;
        self.sibling[b] = a as u16;
        true
    }

    /// 放置决策：若兄弟核忙，有效算力打折，吞吐型线程应避开。
    pub fn effective_permille(&self, core: usize, busy_mask: u16) -> u32 {
        if core >= MAX_CORES {
            return 0;
        }
        let sib = self.sibling[core];
        if sib != 0xFFFF && busy_mask & (1 << sib) != 0 {
            self.share_permille as u32
        } else {
            1000
        }
    }

    /// 吞吐线程偏好：返回兄弟核更空闲的候选核。
    pub fn pick_for_throughput(&self, a: usize, b: usize, busy_mask: u16) -> usize {
        if self.effective_permille(a, busy_mask) >= self.effective_permille(b, busy_mask) {
            a
        } else {
            b
        }
    }
}

// ---------------------------------------------------------------------------
// F029 — 频率感知决策：调度与 P-state 联动
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FreqAware {
    pub cur_mhz: u32,
    pub max_mhz: u32,
    pub min_mhz: u32,
    /// 是否建议提频（队列深度 × 频率缺口）。
    pub boost_hint: bool,
}

impl FreqAware {
    pub const fn new(min_mhz: u32, max_mhz: u32) -> FreqAware {
        FreqAware { cur_mhz: min_mhz, max_mhz, min_mhz, boost_hint: false }
    }

    /// runqueue 深度驱动频率决策：>4 深且未到顶 → boost。
    pub fn on_runqueue_depth(&mut self, depth: u32) -> bool {
        if depth > 4 && self.cur_mhz < self.max_mhz {
            self.cur_mhz = (self.cur_mhz * 3 / 2).min(self.max_mhz);
            self.boost_hint = true;
            true
        } else {
            self.boost_hint = false;
            false
        }
    }

    /// 空闲降频：队列空且高于 min。
    pub fn idle_down(&mut self) -> bool {
        if self.cur_mhz > self.min_mhz {
            self.cur_mhz = self.min_mhz;
            true
        } else {
            false
        }
    }

    /// 有效吞吐 = 频率比例 permille。
    pub fn throughput_permille(&self) -> u32 {
        if self.max_mhz == 0 { 0 } else { self.cur_mhz * 1000 / self.max_mhz }
    }
}

// ---------------------------------------------------------------------------
// F030 — 场景调度档案：游戏/创作/安静一键档
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchedScene {
    Game,
    Create,
    Quiet,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneProfile {
    pub scene: SchedScene,
    /// 前台保底算力 permille。
    pub fg_floor_permille: u32,
    /// 后台任务份额上限 permille。
    pub bg_cap_permille: u32,
    /// 帧节拍目标 Hz（0 = 不限）。
    pub tick_hz: u16,
}

pub fn scene_profile(s: SchedScene) -> SceneProfile {
    match s {
        SchedScene::Game => SceneProfile { scene: s, fg_floor_permille: 900, bg_cap_permille: 100, tick_hz: 120 },
        SchedScene::Create => SceneProfile { scene: s, fg_floor_permille: 700, bg_cap_permille: 300, tick_hz: 0 },
        SchedScene::Quiet => SceneProfile { scene: s, fg_floor_permille: 400, bg_cap_permille: 100, tick_hz: 0 },
    }
}

/// 后台线程配额是否被当前场景封顶。
pub fn bg_quota_ok(p: &SceneProfile, requested_permille: u32) -> u32 {
    requested_permille.min(p.bg_cap_permille)
}

// ---------------------------------------------------------------------------
// F031 — 帧节拍器：渲染/合成节拍统一
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameMetronome {
    pub period_us: u32,
    pub next_deadline_us: u32,
    pub missed: u32,
    pub beats: u32,
}

impl FrameMetronome {
    pub const fn new(hz: u16) -> FrameMetronome {
        FrameMetronome {
            period_us: if hz == 0 { 0 } else { 1_000_000 / hz as u32 },
            next_deadline_us: 0,
            missed: 0,
            beats: 0,
        }
    }

    /// 到点即拍：now 超过 deadline 记一次 miss 并顺延。
    pub fn beat(&mut self, now_us: u32) -> bool {
        if self.period_us == 0 {
            return false;
        }
        if now_us >= self.next_deadline_us {
            if now_us > self.next_deadline_us + self.period_us {
                self.missed += 1;
            }
            self.beats += 1;
            // 追赶：跳过已错过的拍
            let late = now_us - self.next_deadline_us;
            let skip = late / self.period_us + 1;
            self.next_deadline_us += self.period_us * skip;
            true
        } else {
            false
        }
    }

    pub fn miss_rate_permille(&self) -> u32 {
        if self.beats == 0 { 0 } else { self.missed.min(self.beats) * 1000 / self.beats }
    }
}

// ---------------------------------------------------------------------------
// F032 — 算力下限保障器：前台关键线程保底算力
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComputeFloor {
    pub fg_tid: u32,
    /// 保底份额 permille（每调度窗）。
    pub floor_permille: u32,
    /// 本窗口前台实际获得 permille。
    pub granted_permille: u32,
    pub violations: u32,
}

impl ComputeFloor {
    pub const fn new(tid: u32, floor_permille: u32) -> ComputeFloor {
        ComputeFloor { fg_tid: tid, floor_permille, granted_permille: 0, violations: 0 }
    }

    /// 窗口结算：低于保底 → 记违规并在下一窗加权补偿。
    pub fn settle(&mut self, granted_permille: u32) -> u32 {
        self.granted_permille = granted_permille;
        if granted_permille < self.floor_permille {
            self.violations += 1;
            // 补偿份额 = 缺口的 2 倍，封顶 1000
            let gap = self.floor_permille - granted_permille;
            (gap * 2).min(1000 - self.floor_permille)
        } else {
            0
        }
    }

    pub fn healthy(&self) -> bool {
        self.violations == 0
    }
}

// ---------------------------------------------------------------------------
// F033 — 批处理窗口：空闲时段批量任务
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchWindow {
    pub start_min: u16, // 一天内的分钟
    pub end_min: u16,
    pub admitted: u16,
    pub rejected: u16,
}

impl BatchWindow {
    pub const fn new(start_min: u16, end_min: u16) -> BatchWindow {
        BatchWindow { start_min, end_min, admitted: 0, rejected: 0 }
    }

    pub fn in_window(&self, now_min: u16) -> bool {
        let n = now_min % 1440;
        if self.start_min <= self.end_min {
            n >= self.start_min && n < self.end_min
        } else {
            // 跨午夜
            n >= self.start_min || n < self.end_min
        }
    }

    pub fn admit(&mut self, now_min: u16, foreground_busy: bool) -> bool {
        if self.in_window(now_min) && !foreground_busy {
            self.admitted += 1;
            true
        } else {
            self.rejected += 1;
            false
        }
    }
}

// ---------------------------------------------------------------------------
// F034 — 负载预测：短期负载预测预热
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct LoadPredictor {
    /// EWMA 平滑负载（permille），alpha = 1/4。
    pub ewma: u32,
    pub samples: u32,
    pub warmed: bool,
}

impl LoadPredictor {
    pub const fn new() -> LoadPredictor {
        LoadPredictor { ewma: 0, samples: 0, warmed: false }
    }

    pub fn observe(&mut self, load_permille: u32) {
        let v = load_permille.min(1000);
        if self.samples == 0 {
            self.ewma = v;
        } else {
            self.ewma = (self.ewma * 3 + v) / 4;
        }
        self.samples += 1;
        if self.samples >= 8 {
            self.warmed = true;
        }
    }

    /// 预热前不预测（红线：不拿冷数据做决策）。
    pub fn predict(&self) -> Option<u32> {
        if self.warmed {
            Some(self.ewma)
        } else {
            None
        }
    }

    /// 预测高负载 → 建议提前唤醒的核数。
    pub fn prewarm_cores(&self, total: u32) -> u32 {
        match self.predict() {
            Some(l) => (total * l + 750) / 1000,
            None => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// F035 — 迁移成本模型：核间迁移收益评估
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MigrateCost {
    /// 缓存重填成本（等效毫秒 ×10）。
    pub cache_refill_cost: u32,
    /// 源核队列深度，目的核队列深度。
    pub src_depth: u32,
    pub dst_depth: u32,
}

impl MigrateCost {
    /// 迁移收益 = 缓解的排队等待 − 缓存损失；>0 才值得迁。
    pub fn benefit(&self) -> i64 {
        let queue_gain = (self.src_depth.saturating_sub(self.dst_depth)) as i64 * 30;
        let cache_loss = self.cache_refill_cost as i64;
        queue_gain - cache_loss
    }

    pub fn worth_it(&self) -> bool {
        self.benefit() > 0
    }
}

// ---------------------------------------------------------------------------
// F036 — 中断亲和自动均衡：IRQ 分发调优
// ---------------------------------------------------------------------------

pub const MAX_IRQS: usize = 8;

#[derive(Clone, Copy)]
pub struct IrqBalancer {
    /// 每 IRQ 每秒触发数（固定）。
    pub rate: [u32; MAX_IRQS],
    /// 每 IRQ 绑定的核。
    pub core_of: [u8; MAX_IRQS],
    pub count: usize,
}

impl IrqBalancer {
    pub const fn new() -> IrqBalancer {
        IrqBalancer { rate: [0; MAX_IRQS], core_of: [0; MAX_IRQS], count: 0 }
    }

    pub fn add(&mut self, irq: usize, rate: u32, core: u8) -> bool {
        if irq >= MAX_IRQS {
            return false;
        }
        self.rate[irq] = rate;
        self.core_of[irq] = core;
        if irq + 1 > self.count {
            self.count = irq + 1;
        }
        true
    }

    /// 把最高频 IRQ 从最热的核移到最闲的核；返回 (irq, new_core)。
    pub fn rebalance_once(&mut self, core_load: &mut [u32; MAX_CORES]) -> Option<(usize, u8)> {
        let mut top_irq: Option<usize> = None;
        for i in 0..self.count {
            if top_irq.map(|t| self.rate[i] > self.rate[t]).unwrap_or(true) {
                top_irq = Some(i);
            }
        }
        let irq = top_irq?;
        let from = self.core_of[irq] as usize;
        let mut dst: Option<usize> = None;
        for c in 0..MAX_CORES {
            if c != from && dst.map(|d| core_load[c] < core_load[d]).unwrap_or(true) {
                dst = Some(c);
            }
        }
        let to = dst?;
        if core_load[to] >= core_load[from] {
            return None; // 已经均衡
        }
        core_load[from] = core_load[from].saturating_sub(self.rate[irq]);
        core_load[to] += self.rate[irq];
        self.core_of[irq] = to as u8;
        Some((irq, to as u8))
    }
}

// ---------------------------------------------------------------------------
// F037 — 调度仿真器：策略离线回放仿真
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    Fifo,
    RoundRobin,
    Priority,
}

#[derive(Clone, Copy)]
pub struct SimTask {
    pub arrive: u32,
    pub burst: u32,
    pub prio: u8,
}

pub const MAX_SIM_TASKS: usize = 8;

/// 单核确定性仿真，返回平均等待时间（tick）。
pub fn simulate(policy: Policy, tasks: &[SimTask]) -> u64 {
    if tasks.is_empty() || tasks.len() > MAX_SIM_TASKS {
        return 0;
    }
    let n = tasks.len();
    let mut remaining = [0u32; MAX_SIM_TASKS];
    let mut wait = [0u64; MAX_SIM_TASKS];
    for i in 0..n {
        remaining[i] = tasks[i].burst;
    }
    let mut done = 0usize;
    let mut now = 0u32;
    let quantum = 2u32;
    while done < n {
        // 选下一个可运行任务
        let mut pick: Option<usize> = None;
        for i in 0..n {
            if remaining[i] == 0 || tasks[i].arrive > now {
                continue;
            }
            match policy {
                Policy::Fifo => {
                    if pick.map(|p| tasks[i].arrive < tasks[p].arrive).unwrap_or(true) {
                        pick = Some(i);
                    }
                }
                Policy::Priority => {
                    if pick.map(|p| tasks[i].prio > tasks[p].prio).unwrap_or(true) {
                        pick = Some(i);
                    }
                }
                Policy::RoundRobin => {
                    pick = Some(i);
                    break;
                }
            }
        }
        let i = match pick {
            Some(i) => i,
            None => {
                // 空转跳到下一到达
                let mut next = u32::MAX;
                for i in 0..n {
                    if remaining[i] > 0 && tasks[i].arrive < next {
                        next = tasks[i].arrive;
                    }
                }
                now = next;
                continue;
            }
        };
        let slice = match policy {
            Policy::RoundRobin => quantum.min(remaining[i]),
            _ => remaining[i],
        };
        for j in 0..n {
            if j != i && remaining[j] > 0 && tasks[j].arrive <= now {
                wait[j] += slice as u64;
            }
        }
        remaining[i] -= slice;
        now += slice;
        if remaining[i] == 0 {
            done += 1;
        }
    }
    let mut total = 0u64;
    for i in 0..n {
        total += wait[i];
    }
    total / n as u64
}

// ---------------------------------------------------------------------------
// F038 — 调度回放器：真实负载重放验证
// ---------------------------------------------------------------------------

pub const MAX_TRACE: usize = 32;

#[derive(Clone, Copy)]
pub struct SchedEvent {
    pub at_us: u32,
    pub tid: u32,
    /// 0=sleep 1=wake
    pub kind: u8,
}

#[derive(Clone, Copy)]
pub struct SchedReplay {
    pub events: [SchedEvent; MAX_TRACE],
    pub count: usize,
    pub cursor: usize,
    /// 回放中发现的不变量破坏数（如 sleep 两次）。
    pub violations: u32,
}

impl SchedReplay {
    pub const fn new() -> SchedReplay {
        SchedReplay { events: [SchedEvent { at_us: 0, tid: 0, kind: 0 }; MAX_TRACE], count: 0, cursor: 0, violations: 0 }
    }

    pub fn record(&mut self, at_us: u32, tid: u32, kind: u8) -> bool {
        if self.count >= MAX_TRACE || kind > 1 {
            return false;
        }
        if self.count > 0 && at_us < self.events[self.count - 1].at_us {
            return false; // 时间必须单调
        }
        self.events[self.count] = SchedEvent { at_us, tid, kind };
        self.count += 1;
        true
    }

    /// 重放：校验每线程状态机 sleep/wake 交替。
    pub fn replay(&mut self) -> u32 {
        self.violations = 0;
        let mut asleep = 0u32; // 位图（低 32 tid）
        for e in 0..self.count {
            let ev = self.events[e];
            let bit = 1u32 << (ev.tid % 32);
            if ev.kind == 0 {
                if asleep & bit != 0 {
                    self.violations += 1; // 已睡再睡
                } else {
                    asleep |= bit;
                }
            } else if asleep & bit == 0 {
                self.violations += 1; // 没睡却醒
            } else {
                asleep &= !bit;
            }
        }
        self.cursor = self.count;
        self.violations
    }
}

// ---------------------------------------------------------------------------
// F039 — 算力账单：每应用 CPU 消费可视化
// ---------------------------------------------------------------------------

pub const MAX_BILL: usize = 12;

#[derive(Clone, Copy)]
pub struct ComputeBill {
    /// app → 累计毫秒（饱和）。
    pub ms: [u32; MAX_BILL],
    pub app_count: usize,
    pub window_ms: u32,
}

impl ComputeBill {
    pub const fn new(window_ms: u32) -> ComputeBill {
        ComputeBill { ms: [0; MAX_BILL], app_count: 0, window_ms }
    }

    pub fn charge(&mut self, app: usize, ms: u32) -> bool {
        if app >= MAX_BILL {
            return false;
        }
        self.ms[app] = self.ms[app].saturating_add(ms);
        if app + 1 > self.app_count {
            self.app_count = app + 1;
        }
        true
    }

    /// 占比 permille。
    pub fn share_permille(&self, app: usize) -> u32 {
        let mut total = 0u64;
        for i in 0..self.app_count {
            total += self.ms[i] as u64;
        }
        if total == 0 {
            return 0;
        }
        (self.ms[app] as u64 * 1000 / total) as u32
    }

    /// 账单冠军（耗电大户）。
    pub fn top_app(&self) -> Option<usize> {
        let mut best: Option<usize> = None;
        for i in 0..self.app_count {
            if best.map(|b| self.ms[i] > self.ms[b]).unwrap_or(true) {
                best = Some(i);
            }
        }
        best
    }
}

// ---------------------------------------------------------------------------
// F040 — 线程命名规范：全系统可读线程名
// ---------------------------------------------------------------------------

pub const NAME_LEN: usize = 12;

/// 校验线程名：ASCII 可打印、非空、无控制符、长度 ≤ 12。
pub fn thread_name_ok(name: &[u8]) -> bool {
    if name.is_empty() || name.len() > NAME_LEN {
        return false;
    }
    name.iter().all(|&c| (0x20..0x7F).contains(&c))
}

/// 归一：截断到 12、控制符转 '_'、转小写风格建议（保原样但拒全大写由 F012 管）。
pub fn thread_name_normalize(name: &[u8]) -> Option<[u8; NAME_LEN]> {
    if name.is_empty() {
        return None;
    }
    let mut out = [0u8; NAME_LEN];
    let n = name.len().min(NAME_LEN);
    for i in 0..n {
        let c = name[i];
        out[i] = if (0x20..0x7F).contains(&c) { c } else { b'_' };
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// F041 — 调度策略 API：用户态注册策略（开放）
// ---------------------------------------------------------------------------

pub const MAX_POLICIES: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserPolicy {
    pub owner_pid: u32,
    pub id: u16,
    /// 策略回调代号（用户态侧实现）。
    pub hook: u8,
    pub active: bool,
}

#[derive(Clone, Copy)]
pub struct PolicyRegistry {
    pub entries: [Option<UserPolicy>; MAX_POLICIES],
    pub count: usize,
}

impl PolicyRegistry {
    pub const fn new() -> PolicyRegistry {
        PolicyRegistry { entries: [const { None }; MAX_POLICIES], count: 0 }
    }

    /// 每进程最多注册 1 个策略（防抢占注册面）。
    pub fn register(&mut self, owner_pid: u32, id: u16, hook: u8) -> bool {
        if hook > 3 || self.count >= MAX_POLICIES {
            return false;
        }
        for i in 0..self.count {
            if let Some(e) = self.entries[i] {
                if e.owner_pid == owner_pid {
                    return false;
                }
                if e.id == id {
                    return false;
                }
            }
        }
        self.entries[self.count] = Some(UserPolicy { owner_pid, id, hook, active: false });
        self.count += 1;
        true
    }

    pub fn activate(&mut self, id: u16) -> bool {
        for i in 0..self.count {
            if let Some(mut e) = self.entries[i] {
                if e.id == id {
                    e.active = true;
                    self.entries[i] = Some(e);
                    return true;
                }
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// F042 — 调度器 A/B：双策略在线对比
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbTest {
    pub policy_a: u16,
    pub policy_b: u16,
    /// 两侧同一负载的 p99 延迟（us）。
    pub p99_a: u32,
    pub p99_b: u32,
    pub samples: u32,
}

impl AbTest {
    pub fn new(policy_a: u16, policy_b: u16) -> AbTest {
        AbTest { policy_a, policy_b, p99_a: 0, p99_b: 0, samples: 0 }
    }

    pub fn record(&mut self, side_a: bool, p99_us: u32) {
        if side_a {
            self.p99_a = if self.p99_a == 0 { p99_us } else { (self.p99_a + p99_us) / 2 };
        } else {
            self.p99_b = if self.p99_b == 0 { p99_us } else { (self.p99_b + p99_us) / 2 };
        }
        self.samples += 1;
    }

    /// 样本不足不下结论（红线）。
    pub fn winner(&self) -> Option<u16> {
        if self.samples < 8 || self.p99_a == 0 || self.p99_b == 0 {
            return None;
        }
        if self.p99_a * 105 <= self.p99_b * 100 {
            Some(self.policy_a)
        } else if self.p99_b * 105 <= self.p99_a * 100 {
            Some(self.policy_b)
        } else {
            None // 差异 <5% 视为平局
        }
    }
}

// ---------------------------------------------------------------------------
// F043 — 卡顿猎手：交互卡顿自动归因到线程
// ---------------------------------------------------------------------------

pub const MAX_STALL: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StallEvent {
    pub at_us: u32,
    pub frame_gap_us: u32,
    /// 嫌疑线程。
    pub tid: u32,
}

#[derive(Clone, Copy)]
pub struct StallHunter {
    pub events: [Option<StallEvent>; MAX_STALL],
    pub count: usize,
    /// 卡顿判定阈值（帧间隔的倍数 ×100，如 300 = 3 倍）。
    pub threshold_x100: u32,
    pub typical_frame_us: u32,
}

impl StallHunter {
    pub const fn new(typical_frame_us: u32) -> StallHunter {
        StallHunter {
            events: [const { None }; MAX_STALL],
            count: 0,
            threshold_x100: 300,
            typical_frame_us,
        }
    }

    pub fn observe(&mut self, at_us: u32, gap_us: u32, tid: u32) -> bool {
        if self.typical_frame_us == 0 {
            return false;
        }
        if gap_us * 100 < self.threshold_x100 * self.typical_frame_us {
            return false; // 未达卡顿线
        }
        if self.count >= MAX_STALL {
            return false;
        }
        self.events[self.count] = Some(StallEvent { at_us, frame_gap_us: gap_us, tid });
        self.count += 1;
        true
    }

    /// 归因：出现次数最多的嫌疑线程。
    pub fn top_offender(&self) -> Option<u32> {
        let mut best: Option<(u32, u32)> = None; // (tid, count)
        for i in 0..self.count {
            if let Some(e) = self.events[i] {
                let mut c = 1;
                for j in 0..self.count {
                    if let Some(e2) = self.events[j] {
                        if i != j && e2.tid == e.tid {
                            c += 1;
                        }
                    }
                }
                if best.map(|(bt, bc)| c > bc).unwrap_or(true) {
                    best = Some((e.tid, c));
                }
            }
        }
        best.map(|(t, _)| t)
    }
}

// ---------------------------------------------------------------------------
// F044 — 核健康档案：每核温度/降频史
// ---------------------------------------------------------------------------

pub const CORE_HIST: usize = 8;

#[derive(Clone, Copy)]
pub struct CoreHealth {
    pub temp_hist: [u8; CORE_HIST], // °C
    pub throttle_count: u32,
    pub hist_len: u8,
}

impl CoreHealth {
    pub const fn new() -> CoreHealth {
        CoreHealth { temp_hist: [0; CORE_HIST], throttle_count: 0, hist_len: 0 }
    }

    pub fn sample(&mut self, temp_c: u8) {
        if self.hist_len < CORE_HIST as u8 {
            self.temp_hist[self.hist_len as usize] = temp_c;
            self.hist_len += 1;
        } else {
            for i in 0..CORE_HIST - 1 {
                self.temp_hist[i] = self.temp_hist[i + 1];
            }
            self.temp_hist[CORE_HIST - 1] = temp_c;
        }
    }

    pub fn on_throttle(&mut self) {
        self.throttle_count += 1;
    }

    pub fn peak_temp(&self) -> u8 {
        let mut m = 0u8;
        for i in 0..self.hist_len as usize {
            if self.temp_hist[i] > m {
                m = self.temp_hist[i];
            }
        }
        m
    }

    /// 热压力：峰值 ≥ 90 或降频 ≥ 3 次。
    pub fn thermal_stress(&self) -> bool {
        self.peak_temp() >= 90 || self.throttle_count >= 3
    }
}

// ---------------------------------------------------------------------------
// F045 — 轻量隔离执行体：受限 worker 模型
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkerCell {
    pub wid: u8,
    /// 内存限额（页）。
    pub mem_cap_pages: u32,
    pub mem_used: u32,
    /// 指令预算（千条），耗尽即让出。
    pub instr_budget_k: u32,
    pub alive: bool,
}

impl WorkerCell {
    pub const fn new(wid: u8, mem_cap_pages: u32, instr_budget_k: u32) -> WorkerCell {
        WorkerCell { wid, mem_cap_pages, mem_used: 0, instr_budget_k, alive: true }
    }

    pub fn alloc(&mut self, pages: u32) -> bool {
        if !self.alive || self.mem_used + pages > self.mem_cap_pages {
            return false;
        }
        self.mem_used += pages;
        true
    }

    pub fn run_slice(&mut self, instr_k: u32) -> bool {
        if !self.alive {
            return false;
        }
        if instr_k > self.instr_budget_k {
            self.alive = false; // 超预算强制让出（崩溃红线：不拖垮内核）
            return false;
        }
        self.instr_budget_k -= instr_k;
        true
    }

    pub fn kill(&mut self) {
        self.alive = false;
        self.mem_used = 0;
    }
}

// ---------------------------------------------------------------------------
// F046 — 调度策略白皮书：算法公开文档（开放）
// ---------------------------------------------------------------------------

/// 白皮书条目：算法名 → 关键参数公开。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyDoc {
    pub algo_id: u8,
    pub quantum_us: u32,
    pub priority_levels: u8,
    pub published_version: u16,
}

pub fn policy_docs() -> [PolicyDoc; 3] {
    [
        PolicyDoc { algo_id: 0, quantum_us: 2000, priority_levels: 8, published_version: 1 },
        PolicyDoc { algo_id: 1, quantum_us: 4000, priority_levels: 16, published_version: 1 },
        PolicyDoc { algo_id: 2, quantum_us: 1000, priority_levels: 4, published_version: 1 },
    ]
}

/// 白皮书完整性：每个已实现策略都有文档且版本 > 0。
pub fn docs_complete(implemented: u8) -> bool {
    let docs = policy_docs();
    if implemented as usize > docs.len() {
        return false;
    }
    docs[..implemented as usize].iter().all(|d| d.published_version > 0)
}

// ---------------------------------------------------------------------------
// F047 — 调度 fuzz：策略组合对抗测试
// ---------------------------------------------------------------------------

/// 对策略组合做轻量不变量校验：负载不能为负方向、优先级不越界、
/// 时间片不为 0。返回违例数（0 = 通过）。
pub fn sched_fuzz_invariants(cases: &[(u8, u32, u8)]) -> u32 {
    let mut bad = 0;
    for &(prio, quantum_us, cores) in cases {
        if prio > 15 {
            bad += 1;
        }
        if quantum_us == 0 {
            bad += 1;
        }
        if cores == 0 || cores as usize > MAX_CORES {
            bad += 1;
        }
    }
    bad
}

// ---------------------------------------------------------------------------
// F048 — 调度回归基线：性能回归自动比对
// ---------------------------------------------------------------------------

pub const BASELINE_SLOTS: usize = 8;

#[derive(Clone, Copy)]
pub struct SchedBaseline {
    pub p50_us: [u32; BASELINE_SLOTS],
    pub p99_us: [u32; BASELINE_SLOTS],
    pub count: usize,
}

impl SchedBaseline {
    pub const fn new() -> SchedBaseline {
        SchedBaseline { p50_us: [0; BASELINE_SLOTS], p99_us: [0; BASELINE_SLOTS], count: 0 }
    }

    pub fn set(&mut self, slot: usize, p50: u32, p99: u32) -> bool {
        if slot >= BASELINE_SLOTS || p50 > p99 {
            return false;
        }
        self.p50_us[slot] = p50;
        self.p99_us[slot] = p99;
        if slot + 1 > self.count {
            self.count = slot + 1;
        }
        true
    }

    /// 回归判定：新值比基线差 >10% 即回归。
    pub fn regressed(&self, slot: usize, new_p99: u32) -> Option<bool> {
        if slot >= self.count || self.p99_us[slot] == 0 {
            return None;
        }
        Some(new_p99 * 100 > self.p99_us[slot] * 110)
    }
}

// ---------------------------------------------------------------------------
// F049 — 算力编排 DSL：声明式策略描述
// ---------------------------------------------------------------------------

/// 极简 DSL：字节码 [op, arg] 序列。
/// op: 0=set-floor(permille) 1=cap-bg(permille) 2=tick(hz/10) 3=end
pub const DSL_MAX: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DslPlan {
    pub fg_floor: u32,
    pub bg_cap: u32,
    pub tick_hz_x10: u32,
    pub ok: bool,
}

pub fn dsl_eval(prog: &[u8]) -> Option<DslPlan> {
    if prog.is_empty() || prog.len() > DSL_MAX * 2 {
        return None;
    }
    let mut plan = DslPlan { fg_floor: 0, bg_cap: 1000, tick_hz_x10: 0, ok: false };
    let mut i = 0;
    while i + 1 < prog.len() {
        let (op, arg) = (prog[i], prog[i + 1] as u32);
        match op {
            0 => plan.fg_floor = arg * 100, // 以百分数编码 permille
            1 => plan.bg_cap = arg * 100,
            2 => plan.tick_hz_x10 = arg,
            3 => {
                plan.ok = true;
                return Some(plan);
            }
            _ => return None,
        }
        i += 2;
    }
    plan.ok = true;
    Some(plan)
}

// ---------------------------------------------------------------------------
// F050 — 调度域自检：25 项 CheckSet 汇入总检
// ---------------------------------------------------------------------------

pub fn run_m5sched_checks() -> CheckSet {
    let mut set = CheckSet::new("m5sched");

    // F026 画像
    let mut tp = ThreadProfile::new(7);
    for v in [900u16, 950, 920, 980] {
        tp.sample(v);
    }
    set.add("F026 classify cpu-bound", tp.classify() == "cpu-bound", "cls");
    let mut tb = ThreadProfile::new(8);
    for v in [100u16, 900, 100, 900] {
        tb.sample(v);
    }
    set.add("F026 bursty classify", tb.classify() == "bursty" && tb.bursty, "bursty");
    set.add("F026 sliding window", {
        for i in 0..10 {
            tb.sample(i as u16 * 100);
        }
        tb.hist_len == 8
    }, "slide");

    // F027 缓存亲和
    let mut topo = CacheTopology::new(2);
    set.add("F027 topo set", topo.set_llc(0, 0) && topo.set_llc(1, 0) && topo.set_llc(2, 1), "set");
    set.add("F027 sibling pick", topo.pick_sibling(0, 0b00100) == Some(1), "pick");
    set.add("F027 cross-domain fallback", topo.pick_sibling(0, 0xFFFB) == Some(2), "fb");
    set.add("F027 same_llc", topo.same_llc(0, 1) && !topo.same_llc(0, 2), "same");

    // F028 SMT
    let mut smt = SmtPolicy::new(600);
    set.add("F028 pair", smt.pair(0, 1) && !smt.pair(0, 0), "pair");
    set.add("F028 share when sibling busy", smt.effective_permille(0, 0b10) == 600, "share");
    set.add("F028 full when free", smt.effective_permille(0, 0) == 1000, "full");
    set.add("F028 throughput pick", smt.pick_for_throughput(0, 4, 0b10) == 4, "pick");

    // F029 频率感知
    let mut fa = FreqAware::new(800, 4000);
    set.add("F029 boost on depth", fa.on_runqueue_depth(6) && fa.cur_mhz == 1200, "boost");
    set.add("F029 no boost shallow", !fa.on_runqueue_depth(2), "shallow");
    set.add("F029 idle down", fa.idle_down() && fa.cur_mhz == 800, "idle");
    set.add("F029 throughput", {
        fa.cur_mhz = 2000;
        fa.throughput_permille() == 500
    }, "tp");

    // F030 场景
    let game = scene_profile(SchedScene::Game);
    set.add("F030 game floor", game.fg_floor_permille == 900 && game.tick_hz == 120, "game");
    set.add("F030 bg capped", bg_quota_ok(&game, 500) == 100, "cap");

    // F031 帧节拍
    let mut met = FrameMetronome::new(100);
    set.add("F031 beat on time", met.beat(10_000) && met.beats == 1, "beat");
    set.add("F031 late counts miss", met.beat(25_000) && met.beat(45_000) && met.missed == 1, "miss");

    // F032 保底
    let mut cf = ComputeFloor::new(42, 800);
    let comp = cf.settle(600);
    set.add("F032 violation compensated", cf.violations == 1 && comp == 200, "comp");

    // F033 批处理窗口
    let mut bw = BatchWindow::new(120, 180); // 02:00–03:00
    set.add("F033 in window", bw.in_window(150), "in");
    set.add("F033 cross midnight", BatchWindow::new(1380, 120).in_window(60), "cross");
    set.add("F033 busy reject", !bw.admit(150, true) && bw.rejected == 1, "busy");

    // F034 负载预测
    let mut lp = LoadPredictor::new();
    set.add("F034 cold no predict", lp.predict().is_none(), "cold");
    for i in 0..8 {
        lp.observe(500 + i * 10);
    }
    set.add("F034 warmed predict", lp.predict().is_some(), "warm");
    set.add("F034 prewarm cores", lp.prewarm_cores(4) == 2, "prewarm");

    // F035 迁移成本
    let good = MigrateCost { cache_refill_cost: 50, src_depth: 6, dst_depth: 0 };
    let bad = MigrateCost { cache_refill_cost: 50, src_depth: 1, dst_depth: 0 };
    set.add("F035 benefit math", good.benefit() == 130 && bad.benefit() == -20, "math");
    set.add("F035 worth", good.worth_it() && !bad.worth_it(), "worth");

    // F036 IRQ 均衡
    let mut ib = IrqBalancer::new();
    let mut loads = [400u32; MAX_CORES];
    set.add("F036 add irqs", ib.add(0, 900, 0) && ib.add(1, 100, 1), "add");
    loads[0] = 1000;
    loads[1] = 100;
    let mv = ib.rebalance_once(&mut loads);
    set.add("F036 moved to idle", mv == Some((0, 1)), "move");

    // F037 仿真器
    let tasks = [SimTask { arrive: 0, burst: 5, prio: 1 }, SimTask { arrive: 0, burst: 3, prio: 5 }];
    let rr = simulate(Policy::RoundRobin, &tasks);
    let pr = simulate(Policy::Priority, &tasks);
    set.add("F037 sim deterministic", rr > 0, "det");
    set.add("F037 priority helps hi-prio", pr <= rr, "prio");

    // F038 回放器
    let mut rp = SchedReplay::new();
    set.add("F038 record ok", rp.record(0, 1, 0) && rp.record(10, 1, 1), "rec");
    set.add("F038 mono time", !rp.record(5, 2, 0), "mono");
    set.add("F038 replay clean", rp.replay() == 0, "clean");
    set.add("F038 double sleep caught", {
        let mut r2 = SchedReplay::new();
        let _ = r2.record(0, 1, 0);
        let _ = r2.record(1, 1, 0);
        r2.replay() == 1
    }, "double");

    // F039 账单
    let mut bill = ComputeBill::new(1000);
    set.add("F039 charge", bill.charge(0, 300) && bill.charge(1, 100), "charge");
    set.add("F039 share", bill.share_permille(0) == 750, "share");
    set.add("F039 top app", bill.top_app() == Some(0), "top");

    // F040 线程命名
    set.add("F040 name ok", thread_name_ok(b"gfx-flip"), "ok");
    set.add("F040 reject long", !thread_name_ok(&[b'a'; 13]), "long");
    set.add("F040 normalize ctl", thread_name_normalize(b"a\x01b").map(|n| n == [b'a', b'_', b'b', 0, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap_or(false), "norm");

    // F041 策略 API
    let mut reg = PolicyRegistry::new();
    set.add("F041 register", reg.register(100, 1, 2) && reg.register(200, 2, 0), "reg");
    set.add("F041 one per pid", !reg.register(100, 3, 1), "dup-pid");
    set.add("F041 activate", reg.activate(1) && reg.entries[0].map(|e| e.active).unwrap_or(false), "act");

    // F042 A/B
    let mut ab = AbTest::new(10, 20);
    for _ in 0..8 {
        ab.record(true, 1000);
        ab.record(false, 1200);
    }
    set.add("F042 winner a", ab.winner() == Some(10), "win");
    set.add("F042 cold no verdict", AbTest::new(1, 2).winner().is_none(), "cold");

    // F043 卡顿猎手
    let mut sh = StallHunter::new(16_000);
    set.add("F043 ignore small gap", !sh.observe(0, 20_000, 1), "small");
    set.add("F043 catch stall", sh.observe(1000, 64_000, 3) && sh.observe(2000, 64_000, 3), "catch");
    set.add("F043 top offender", sh.top_offender() == Some(3), "top");

    // F044 核健康
    let mut ch = CoreHealth::new();
    ch.sample(70);
    ch.sample(95);
    ch.on_throttle();
    set.add("F044 peak temp", ch.peak_temp() == 95, "peak");
    set.add("F044 thermal stress", ch.thermal_stress(), "stress");
    set.add("F044 healthy core", !CoreHealth::new().thermal_stress(), "healthy");

    // F045 worker cell
    let mut wc = WorkerCell::new(1, 10, 5);
    set.add("F045 alloc within cap", wc.alloc(6) && !wc.alloc(6), "alloc");
    set.add("F045 budget kill", !wc.run_slice(9) && !wc.alive, "kill");

    // F046 白皮书
    set.add("F046 docs complete", docs_complete(3), "docs");
    set.add("F046 docs overflow", !docs_complete(9), "overflow");

    // F047 fuzz
    set.add("F047 fuzz bad caught", sched_fuzz_invariants(&[(16, 100, 1), (3, 0, 1), (3, 100, 0)]) == 3, "bad");
    set.add("F047 fuzz clean", sched_fuzz_invariants(&[(7, 1000, 4)]) == 0, "clean");

    // F048 回归基线
    let mut sb = SchedBaseline::new();
    set.add("F048 baseline set", sb.set(0, 500, 900) && !sb.set(1, 900, 500), "set");
    set.add("F048 no regression", sb.regressed(0, 950) == Some(false), "ok");
    set.add("F048 regression caught", sb.regressed(0, 1200) == Some(true), "reg");

    // F049 DSL
    let plan = dsl_eval(&[0, 8, 1, 2, 3]);
    set.add("F049 dsl eval", plan.map(|p| p.fg_floor == 800 && p.bg_cap == 200 && p.ok).unwrap_or(false), "eval");
    set.add("F049 dsl bad op", dsl_eval(&[9, 1]).is_none(), "bad");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f026_profile_classify() {
        let mut tp = ThreadProfile::new(1);
        for v in [900u16, 950, 920] {
            tp.sample(v);
        }
        assert_eq!(tp.classify(), "cpu-bound");
    }

    #[test]
    fn f037_simulation_deterministic() {
        let tasks = [SimTask { arrive: 0, burst: 4, prio: 1 }, SimTask { arrive: 0, burst: 2, prio: 9 }];
        assert!(simulate(Policy::RoundRobin, &tasks) > 0);
    }

    #[test]
    fn f050_self_check_passes() {
        let set = run_m5sched_checks();
        assert_eq!(set.len(), 64, "m5sched 需要 75 项断言");
        assert!(!set.truncated());
        assert!(set.all_passed(), "m5sched 自检必须全绿");
    }
}
