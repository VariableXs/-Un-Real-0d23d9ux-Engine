//! m600boot — VARIX-M600 AI-01 启动与秒开域 (F001~F025)
//!
//! 引导预算拆解器/冷启动火焰图/预读预测器/惰性服务树/秒回唤醒管线/
//! 引导动画导演/启动阶段并行编排/关键路径零等待/服务延迟分级/
//! 启动沙盘重放/固件握手优化/页缓存预热器/启动降级阶梯/快速自检通道/
//! 引导故障黑匣子/首帧渲染直通/驱动并行探测/启动耗时契约/会话快照恢复/
//! 冷热数据分层预载/引导语音旁白/无障碍启动序列/启动隐私模式/
//! 休眠镜像压缩器/启动终检官。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部用毫秒整数与 permille）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F001 — 引导预算拆解器：总预算按阶段权重拆分到各阶段
// ===========================================================================

/// 预算阶段：firmware / kernel / drivers / services / first-frame。
pub const BUDGET_STAGES: usize = 5;
/// 上面各阶段的 permille 权重，总和恒为 1000。
pub const BUDGET_WEIGHTS: [u16; BUDGET_STAGES] = [80, 320, 180, 300, 120];

pub fn budget_alloc(total_ms: u32) -> [u32; BUDGET_STAGES] {
    let mut out = [0u32; BUDGET_STAGES];
    let mut used = 0u32;
    for i in 0..BUDGET_STAGES {
        if i == BUDGET_STAGES - 1 {
            out[i] = total_ms - used;
        } else {
            out[i] = (total_ms as u64 * BUDGET_WEIGHTS[i] as u64 / 1000) as u32;
            used += out[i];
        }
    }
    out
}

// ===========================================================================
// F002 — 冷启动火焰图：按阶段聚合计时样本
// ===========================================================================

pub const FLAME_MAX: usize = 16;

#[derive(Clone, Copy)]
pub struct FlameEntry {
    pub stage: u8,
    pub start_ms: u32,
    pub dur_ms: u32,
}

#[derive(Clone, Copy)]
pub struct FlameGraph {
    pub entries: [FlameEntry; FLAME_MAX],
    pub count: usize,
}

impl FlameGraph {
    pub const fn new() -> FlameGraph {
        FlameGraph { entries: [FlameEntry { stage: 0, start_ms: 0, dur_ms: 0 }; FLAME_MAX], count: 0 }
    }
    pub fn record(&mut self, stage: u8, start_ms: u32, dur_ms: u32) {
        if self.count < FLAME_MAX {
            self.entries[self.count] = FlameEntry { stage, start_ms, dur_ms };
            self.count += 1;
        }
    }
    /// 某阶段的累计耗时。
    pub fn stage_total(&self, stage: u8) -> u32 {
        let mut t = 0u32;
        for i in 0..self.count {
            if self.entries[i].stage == stage {
                t += self.entries[i].dur_ms;
            }
        }
        t
    }
    /// 最耗时的阶段下标（火焰图最宽的一格）。
    pub fn widest_stage(&self, stages: usize) -> usize {
        let mut best = 0usize;
        let mut best_t = 0u32;
        for s in 0..stages {
            let t = self.stage_total(s as u8);
            if t > best_t {
                best_t = t;
                best = s;
            }
        }
        best
    }
}

// ===========================================================================
// F003 — 预读预测器：按上次访问轨迹预测下一次要预读的页
// ===========================================================================

pub const TRACE_MAX: usize = 8;

#[derive(Clone, Copy)]
pub struct ReadPredictor {
    /// 最近访问的块号轨迹（新到旧）。
    pub trace: [u64; TRACE_MAX],
    pub len: usize,
}

impl ReadPredictor {
    pub const fn new() -> ReadPredictor {
        ReadPredictor { trace: [0; TRACE_MAX], len: 0 }
    }
    pub fn observe(&mut self, block: u64) {
        // 去重上移：重复命中先移除旧位置。
        for i in 0..self.len {
            if self.trace[i] == block {
                for j in (1..=i).rev() {
                    self.trace[j] = self.trace[j - 1];
                }
                self.trace[0] = block;
                return;
            }
        }
        if self.len < TRACE_MAX {
            for i in (1..=self.len).rev() {
                self.trace[i] = self.trace[i - 1];
            }
            self.len += 1;
        } else {
            for i in (1..TRACE_MAX).rev() {
                self.trace[i] = self.trace[i - 1];
            }
        }
        self.trace[0] = block;
    }
    /// 顺序步长预测：若最近两块呈等差，预测下一块。
    pub fn next_sequential(&self) -> Option<u64> {
        if self.len >= 2 {
            let d = self.trace[0] as i64 - self.trace[1] as i64;
            if d == 1 {
                return Some(self.trace[0] + 1);
            }
        }
        None
    }
}

// ===========================================================================
// F004 — 惰性服务树：服务可标记 deferred，仅关键服务急启
// ===========================================================================

pub const SVC_MAX: usize = 12;

#[derive(Clone, Copy)]
pub struct LazyService {
    pub id: u8,
    pub critical: bool,
    pub deferred: bool,
    pub started: bool,
}

pub fn lazy_start_plan(svcs: &mut [LazyService; SVC_MAX], count: usize) -> usize {
    let mut immediate = 0usize;
    for s in svcs.iter_mut().take(count) {
        if s.critical {
            s.started = true;
            immediate += 1;
        } else {
            s.deferred = true;
        }
    }
    immediate
}

// ===========================================================================
// F005 — 秒回唤醒管线：唤醒后按序回放最短动作集
// ===========================================================================

pub const WAKE_ACTIONS: [&str; 5] = ["restore-power", "enable-display", "resume-input", "resume-net", "unlock-session"];

#[derive(Clone, Copy)]
pub struct WakePipeline {
    pub done_mask: u8,
    pub total_ms: u32,
}

/// 逐步执行：返回当前应执行的动作下标；全部完成返回 None。
pub fn wake_next(p: &WakePipeline) -> Option<usize> {
    if p.done_mask >= (1u8 << WAKE_ACTIONS.len()) - 1 {
        return None;
    }
    for i in 0..WAKE_ACTIONS.len() {
        if p.done_mask & (1 << i) == 0 {
            return Some(i);
        }
    }
    None
}

/// 唤醒预算（毫秒）：输入恢复必须早于网络恢复，检查顺序正确性。
pub fn wake_order_valid(done_before_input: u8) -> bool {
    // resume-input(2) 必须在 resume-net(3) 之前完成。
    let input_done = done_before_input & (1 << 2) != 0;
    let net_done = done_before_input & (1 << 3) != 0;
    input_done || !net_done
}

// ===========================================================================
// F006 — 引导动画导演：进度条按实际阶段推进（不允许超前进度）
// ===========================================================================

#[derive(Clone, Copy)]
pub struct BootAnimator {
    pub stages_done: u8,
    pub total_stages: u8,
    pub fake_progress: bool,
}

pub fn animator_progress(a: &BootAnimator) -> u16 {
    if a.fake_progress && a.stages_done < a.total_stages {
        // 诚实原则：拒绝超前绘制，压回真实进度。
        return ((a.stages_done as u32 * 1000) / a.total_stages as u32) as u16;
    }
    ((a.stages_done as u32 * 1000) / a.total_stages as u32) as u16
}

// ===========================================================================
// F007 — 启动阶段并行编排：无依赖的阶段可以同时启动
// ===========================================================================

/// deps: 每阶段最多 2 个前置（u8::MAX 表示无）；返回可并行启动的阶段集合位图。
pub fn parallel_ready(deps: &[[u8; 2]; BUDGET_STAGES], done_mask: u8) -> u8 {
    let mut ready = 0u8;
    for i in 0..BUDGET_STAGES {
        if done_mask & (1 << i) != 0 {
            continue;
        }
        let mut blocked = false;
        for d in 0..2 {
            let dep = deps[i][d];
            if dep != u8::MAX && done_mask & (1 << dep) == 0 {
                blocked = true;
            }
        }
        if !blocked {
            ready |= 1 << i;
        }
    }
    ready
}

// ===========================================================================
// F008 — 关键路径零等待：关键阶段允许重试上限为 0（立即失败降级）
// ===========================================================================

#[derive(Clone, Copy)]
pub struct CritPathOp {
    pub stage: u8,
    pub retries_allowed: u8,
}

pub fn crit_path_zero_wait(op: CritPathOp, fail_now: bool) -> u8 {
    // 关键路径阶段：等待=0，一次失败立刻返回降级码 0xFF。
    if op.stage <= 4 && fail_now && op.retries_allowed == 0 {
        return 0xFF;
    }
    op.stage
}

// ===========================================================================
// F009 — 服务延迟分级：按可容忍延迟分三档
// ===========================================================================

pub const LATENCY_TIERS: [&str; 3] = ["t0-blocking", "t1-eager", "t2-deferrable"];

pub fn latency_tier_of(tolerable_ms: u32) -> u8 {
    if tolerable_ms <= 100 {
        0
    } else if tolerable_ms <= 2000 {
        1
    } else {
        2
    }
}

// ===========================================================================
// F010 — 启动沙盘重放：同一事件序列重放得到同一状态指纹
// ===========================================================================

pub fn sandbox_fingerprint(events: &[u8]) -> u64 {
    // FNV-1a 变体，可重放校验用。
    let mut h: u64 = 0xcbf29ce484222325;
    for &e in events {
        h ^= e as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ===========================================================================
// F011 — 固件握手优化：跳过固件重复提供的阶段
// ===========================================================================

/// firmware_caps 位图：bit0=已提供内存图 bit1=已提供帧缓冲 bit2=已计时。
pub fn firmware_skip_mask(caps: u8) -> u8 {
    let mut skip = 0u8;
    if caps & 0b001 != 0 {
        skip |= 0b00001; // 跳过 firmware 阶段的内存探测
    }
    if caps & 0b010 != 0 {
        skip |= 0b10000; // 跳过首帧的重新初始化
    }
    skip
}

// ===========================================================================
// F012 — 页缓存预热器：按热度队列预载页块
// ===========================================================================

pub const WARM_MAX: usize = 8;

#[derive(Clone, Copy)]
pub struct PageWarmer {
    pub blocks: [u64; WARM_MAX],
    pub heat: [u8; WARM_MAX],
    pub count: usize,
}

impl PageWarmer {
    pub const fn new() -> PageWarmer {
        PageWarmer { blocks: [0; WARM_MAX], heat: [0; WARM_MAX], count: 0 }
    }
    pub fn touch(&mut self, block: u64) {
        for i in 0..self.count {
            if self.blocks[i] == block {
                self.heat[i] = self.heat[i].saturating_add(1);
                return;
            }
        }
        if self.count < WARM_MAX {
            self.blocks[self.count] = block;
            self.heat[self.count] = 1;
            self.count += 1;
        }
    }
    /// 预载顺序：热度降序的块号列表（写入 out，返回数量）。
    pub fn preload_order(&self, out: &mut [u64; WARM_MAX]) -> usize {
        let mut idx = [0usize; WARM_MAX];
        for i in 0..self.count {
            idx[i] = i;
        }
        // 插入排序按热度降序。
        for i in 1..self.count {
            let mut j = i;
            while j > 0 && self.heat[idx[j - 1]] < self.heat[idx[j]] {
                idx.swap(j, j - 1);
                j -= 1;
            }
        }
        for i in 0..self.count {
            out[i] = self.blocks[idx[i]];
        }
        self.count
    }
}

// ===========================================================================
// F013 — 启动降级阶梯：预算超时逐级砍非关键阶段
// ===========================================================================

/// 返回超时后应跳过的阶段位图（从最高阶段号往下砍，保 firmware/kernel）。
pub fn degrade_ladder(elapsed_ms: u32, budget_ms: u32) -> u8 {
    if elapsed_ms <= budget_ms {
        return 0;
    }
    let ratio_permille = if budget_ms == 0 { 1000 } else { (elapsed_ms as u64 * 1000 / budget_ms as u64) as u64 };
    if ratio_permille <= 1100 {
        0b11000 // 砍 services + first-frame 动画
    } else if ratio_permille <= 1500 {
        0b11100
    } else {
        0b11110 // 只保 firmware
    }
}

// ===========================================================================
// F014 — 快速自检通道：自检分两速，快检失败不阻塞引导
// ===========================================================================

#[derive(Clone, Copy)]
pub struct FastSelftest {
    pub quick_ms: u32,
    pub deep_ms: u32,
    pub quick_failed: bool,
}

pub fn selftest_gate(st: &FastSelftest, budget_left_ms: u32) -> u8 {
    // 0=跳过 1=快检 2=深检。深检只在预算充足且快检通过时进行。
    if st.quick_ms + st.deep_ms <= budget_left_ms && !st.quick_failed {
        2
    } else if st.quick_ms <= budget_left_ms {
        1
    } else {
        0
    }
}

// ===========================================================================
// F015 — 引导故障黑匣子：滚动记录最后 N 条引导事件
// ===========================================================================

pub const BB_MAX: usize = 8;

#[derive(Clone, Copy)]
pub struct BlackBox {
    pub ring: [u16; BB_MAX],
    pub head: usize,
    pub len: usize,
    pub dropped: usize,
}

impl BlackBox {
    pub const fn new() -> BlackBox {
        BlackBox { ring: [0; BB_MAX], head: 0, len: 0, dropped: 0 }
    }
    pub fn log(&mut self, code: u16) {
        self.ring[self.head] = code;
        self.head = (self.head + 1) % BB_MAX;
        if self.len < BB_MAX {
            self.len += 1;
        } else {
            self.dropped += 1;
        }
    }
    /// 按时间顺序导出（旧→新）。
    pub fn export(&self, out: &mut [u16; BB_MAX]) -> usize {
        for i in 0..self.len {
            out[i] = self.ring[(self.head + BB_MAX - self.len + i) % BB_MAX];
        }
        self.len
    }
}

// ===========================================================================
// F016 — 首帧渲染直通：首帧禁止走完整合成器路径
// ===========================================================================

pub const FIRST_FRAME_DIRECT: bool = true;

/// 返回首帧是否允许走直通（要求平台已由固件提供帧缓冲）。
pub fn first_frame_direct_allowed(firmware_fb: bool) -> bool {
    FIRST_FRAME_DIRECT && firmware_fb
}

// ===========================================================================
// F017 — 驱动并行探测：驱动探测互不阻塞
// ===========================================================================

pub const DRV_PROBE_MAX: usize = 8;

#[derive(Clone, Copy)]
pub struct ProbeJob {
    pub drv_id: u8,
    pub timeout_ms: u16,
    pub done: bool,
}

/// 并行探测：每个作业限时独立，任一超时不影响其余。返回完成数。
pub fn probe_parallel(jobs: &mut [ProbeJob; DRV_PROBE_MAX], count: usize, elapsed_ms: u16) -> usize {
    let mut done = 0usize;
    for j in jobs.iter_mut().take(count) {
        if !j.done && elapsed_ms >= j.timeout_ms {
            j.done = true; // 超时也视为该作业结束（失败形态）
        }
        if j.done {
            done += 1;
        }
    }
    done
}

// ===========================================================================
// F018 — 启动耗时契约：每阶段有承诺上限，违约留痕
// ===========================================================================

#[derive(Clone, Copy)]
pub struct StageContract {
    pub promised_ms: u32,
    pub actual_ms: u32,
    pub breach_recorded: bool,
}

pub fn contract_check(c: &mut StageContract) -> bool {
    let ok = c.actual_ms <= c.promised_ms;
    if !ok {
        c.breach_recorded = true;
    }
    ok
}

// ===========================================================================
// F019 — 会话快照恢复：快照有效即可跳过冷启动
// ===========================================================================

#[derive(Clone, Copy)]
pub struct SessionSnapshot {
    pub magic_ok: bool,
    pub version: u16,
    pub crc_ok: bool,
}

pub const SNAPSHOT_VERSION: u16 = 2;

pub fn snapshot_restorable(s: &SessionSnapshot) -> bool {
    s.magic_ok && s.version <= SNAPSHOT_VERSION && s.crc_ok
}

// ===========================================================================
// F020 — 冷热数据分层预载：热层进内存，冷层留盘
// ===========================================================================

pub fn tier_of(access_count_30d: u32, size_kb: u32) -> u8 {
    // 热：访问多且小；温：访问多但大；冷：访问少。
    if access_count_30d >= 10 && size_kb <= 4096 {
        0 // hot
    } else if access_count_30d >= 10 {
        1 // warm
    } else {
        2 // cold
    }
}

// ===========================================================================
// F021 — 引导语音旁白：旁白不阻塞、可整体关闭
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Narrator {
    pub enabled: bool,
    pub queue: [u8; 4],
    pub qlen: usize,
    pub spoken: usize,
}

impl Narrator {
    pub const fn new(enabled: bool) -> Narrator {
        Narrator { enabled, queue: [0; 4], qlen: 0, spoken: 0 }
    }
    pub fn enqueue(&mut self, clip: u8) {
        if self.enabled && self.qlen < 4 {
            self.queue[self.qlen] = clip;
            self.qlen += 1;
        }
    }
    pub fn tick(&mut self) -> Option<u8> {
        if !self.enabled || self.qlen == 0 {
            return None;
        }
        let clip = self.queue[0];
        for i in 1..self.qlen {
            self.queue[i - 1] = self.queue[i];
        }
        self.qlen -= 1;
        self.spoken += 1;
        Some(clip)
    }
}

// ===========================================================================
// F022 — 无障碍启动序列：高对比/大字模式在首帧前生效
// ===========================================================================

#[derive(Clone, Copy)]
pub struct A11yBoot {
    pub high_contrast: bool,
    pub large_text: bool,
    pub screen_reader: bool,
}

/// 首帧前必须应用的无障碍位图（越早生效越不算"补丁式无障碍"）。
pub fn a11y_first_frame_mask(a: &A11yBoot) -> u8 {
    let mut m = 0u8;
    if a.high_contrast {
        m |= 0b001;
    }
    if a.large_text {
        m |= 0b010;
    }
    if a.screen_reader {
        m |= 0b100;
    }
    m
}

// ===========================================================================
// F023 — 启动隐私模式：隐私启动不加载上次会话、不恢复剪贴板
// ===========================================================================

pub fn privacy_boot_restore_mask(privacy: bool) -> u8 {
    if privacy {
        0 // 什么都不恢复
    } else {
        0b11 // 恢复会话 + 剪贴板
    }
}

// ===========================================================================
// F024 — 休眠镜像压缩器：镜像按页压缩，比例用 permille 记录
// ===========================================================================

pub fn image_compress_ratio(raw_pages: u32, stored_pages: u32) -> u16 {
    if raw_pages == 0 {
        return 1000;
    }
    ((stored_pages as u64 * 1000 / raw_pages as u64) as u16).min(1000)
}

// ===========================================================================
// F025 — 启动终检官：引导结束后一次完整体检
// ===========================================================================

pub fn boot_final_verdict(
    contracts_kept: bool,
    snapshot_used: bool,
    snapshot_valid: bool,
    panics: u32,
    budget_ms: u32,
) -> bool {
    // 快照恢复路径不对照启动预算计违约；崩溃一次即不合格。
    if panics > 0 {
        return false;
    }
    if snapshot_used {
        return snapshot_valid;
    }
    contracts_kept && budget_ms <= 8000
}

// ===========================================================================
// 域自检（≥25 项）
// ===========================================================================

pub fn run_m600boot_checks() -> CheckSet {
    let mut set = CheckSet::new("m600boot");

    // F001
    let b = budget_alloc(8000);
    let sum: u32 = b.iter().sum();
    set.add("F001 budget alloc sums to total", sum == 8000, "sum mismatch");
    set.add("F001 weights sum to 1000", BUDGET_WEIGHTS.iter().sum::<u16>() == 1000, "weights");

    // F002
    let mut fg = FlameGraph::new();
    fg.record(1, 100, 400);
    fg.record(1, 500, 200);
    fg.record(2, 700, 100);
    let w = fg.widest_stage(3);
    set.add("F002 flame widest stage", w == 1 && fg.stage_total(1) == 600, "stage wrong");

    // F003
    let mut p = ReadPredictor::new();
    p.observe(10);
    p.observe(11);
    let seq = p.next_sequential();
    p.observe(11);
    set.add("F003 predictor sequential", seq == Some(12), "seq wrong");
    set.add("F003 predictor dedup", p.len == 2 && p.trace[0] == 11, "dedup wrong");

    // F004
    let mut svcs = [LazyService { id: 0, critical: true, deferred: false, started: false }; SVC_MAX];
    svcs[1].critical = false;
    let imm = lazy_start_plan(&mut svcs, 4);
    let s1_started = svcs[1].deferred && !svcs[1].started;
    set.add("F004 lazy plan", imm == 3 && s1_started, "plan wrong");
    set.add("F004 non-critical deferred", s1_started, "defer wrong");

    // F005
    let wp = WakePipeline { done_mask: 0b0111, total_ms: 1200 };
    let nxt = wake_next(&wp);
    let ord = wake_order_valid(0b0111);
    set.add("F005 wake next action", nxt == Some(3), "next wrong");
    set.add("F005 wake order", ord, "order wrong");
    set.add("F005 wake complete", wake_next(&WakePipeline { done_mask: 0b11111, total_ms: 0 }).is_none(), "complete wrong");

    // F006
    let a = animator_progress(&BootAnimator { stages_done: 2, total_stages: 4, fake_progress: true });
    set.add("F006 animator honest", a == 500, "progress wrong");

    // F007
    let deps = [[u8::MAX; 2], [0, u8::MAX], [u8::MAX; 2], [1, u8::MAX], [u8::MAX; 2]];
    let ready = parallel_ready(&deps, 0b00001);
    set.add("F007 parallel ready", ready == 0b10110, "ready wrong");

    // F008
    let r = crit_path_zero_wait(CritPathOp { stage: 2, retries_allowed: 0 }, true);
    set.add("F008 zero wait degrade", r == 0xFF, "degrade wrong");

    // F009
    set.add("F009 latency tiers", latency_tier_of(50) == 0 && latency_tier_of(500) == 1 && latency_tier_of(5000) == 2, "tier wrong");

    // F010
    let f1 = sandbox_fingerprint(&[1, 2, 3]);
    let f2 = sandbox_fingerprint(&[1, 2, 3]);
    let f3 = sandbox_fingerprint(&[3, 2, 1]);
    set.add("F010 replay deterministic", f1 == f2 && f1 != f3, "fingerprint");

    // F011
    set.add("F011 firmware skip", firmware_skip_mask(0b011) == 0b10001, "skip wrong");

    // F012
    let mut pw = PageWarmer::new();
    pw.touch(100);
    pw.touch(100);
    pw.touch(200);
    let mut order = [0u64; WARM_MAX];
    let n = pw.preload_order(&mut order);
    set.add("F012 warm order", n == 2 && order[0] == 100 && order[1] == 200, "order wrong");

    // F013
    set.add("F013 ladder none", degrade_ladder(8000, 8000) == 0, "ladder wrong");
    set.add("F013 ladder mild", degrade_ladder(8600, 8000) == 0b11000, "ladder mild");
    set.add("F013 ladder severe", degrade_ladder(14000, 8000) == 0b11110, "ladder severe");

    // F014
    let st = FastSelftest { quick_ms: 100, deep_ms: 500, quick_failed: false };
    set.add("F014 deep when affordable", selftest_gate(&st, 1000) == 2, "gate wrong");
    set.add("F014 quick when tight", selftest_gate(&st, 300) == 1, "gate quick");
    set.add("F014 skip when broke", selftest_gate(&st, 50) == 0, "gate skip");

    // F015
    let mut bb = BlackBox::new();
    bb.log(1);
    bb.log(2);
    bb.log(3);
    let mut out = [0u16; BB_MAX];
    let n = bb.export(&mut out);
    set.add("F015 blackbox order", n == 3 && out[0] == 1 && out[2] == 3, "export wrong");

    // F016
    set.add("F016 first frame direct", first_frame_direct_allowed(true), "direct wrong");
    set.add("F016 no fb fallback", !first_frame_direct_allowed(false), "fallback wrong");

    // F017
    let mut jobs = [ProbeJob { drv_id: 0, timeout_ms: 100, done: false }; DRV_PROBE_MAX];
    jobs[0].timeout_ms = 50;
    jobs[1].timeout_ms = 500;
    let d = probe_parallel(&mut jobs, 3, 200);
    set.add("F017 probe isolation", d == 2 && !jobs[1].done, "probe wrong");

    // F018
    let mut c = StageContract { promised_ms: 500, actual_ms: 600, breach_recorded: false };
    let kept = contract_check(&mut c);
    set.add("F018 contract breach", !kept && c.breach_recorded, "breach wrong");

    // F019
    set.add("F019 snapshot valid", snapshot_restorable(&SessionSnapshot { magic_ok: true, version: 2, crc_ok: true }), "valid wrong");
    set.add("F019 snapshot bad crc", !snapshot_restorable(&SessionSnapshot { magic_ok: true, version: 2, crc_ok: false }), "crc wrong");

    // F020
    set.add("F020 tier hot", tier_of(50, 100) == 0, "hot wrong");
    set.add("F020 tier warm", tier_of(50, 9999) == 1, "warm wrong");
    set.add("F020 tier cold", tier_of(1, 100) == 2, "cold wrong");

    // F021
    let mut nar = Narrator::new(false);
    nar.enqueue(1);
    set.add("F021 narrator off", nar.tick().is_none(), "off wrong");
    let mut nar2 = Narrator::new(true);
    nar2.enqueue(2);
    nar2.enqueue(3);
    let c1 = nar2.tick();
    let c2 = nar2.tick();
    set.add("F021 narrator queue", c1 == Some(2) && c2 == Some(3), "queue wrong");

    // F022
    let mask = a11y_first_frame_mask(&A11yBoot { high_contrast: true, large_text: false, screen_reader: true });
    set.add("F022 a11y mask", mask == 0b101, "mask wrong");

    // F023
    set.add("F023 privacy restore", privacy_boot_restore_mask(true) == 0 && privacy_boot_restore_mask(false) == 0b11, "privacy wrong");

    // F024
    set.add("F024 compress ratio", image_compress_ratio(100, 40) == 400, "ratio wrong");

    // F025
    set.add("F025 verdict pass", boot_final_verdict(true, false, false, 0, 7000), "verdict wrong");
    set.add("F025 verdict panic", !boot_final_verdict(true, false, false, 1, 7000), "panic wrong");
    set.add("F025 verdict snapshot", boot_final_verdict(false, true, true, 0, 99999), "snapshot wrong");

    set
}

// ===========================================================================
// 单测
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f001_budget_matches_total() {
        assert_eq!(budget_alloc(8000).iter().sum::<u32>(), 8000);
        assert_eq!(budget_alloc(100).iter().sum::<u32>(), 100);
    }

    #[test]
    fn f002_flame_aggregates() {
        let mut fg = FlameGraph::new();
        fg.record(0, 0, 80);
        fg.record(1, 80, 320);
        fg.record(1, 400, 10);
        assert_eq!(fg.stage_total(1), 330);
        assert_eq!(fg.widest_stage(2), 1);
    }

    #[test]
    fn f003_predictor_sequence_and_dedup() {
        let mut p = ReadPredictor::new();
        p.observe(7);
        p.observe(8);
        p.observe(8);
        assert_eq!(p.len, 2);
        assert_eq!(p.next_sequential(), Some(9));
        p.observe(100);
        assert_eq!(p.trace[0], 100);
    }

    #[test]
    fn f004_lazy_plan_flags() {
        let mut s = [LazyService { id: 0, critical: false, deferred: false, started: false }; SVC_MAX];
        s[0].critical = true;
        assert_eq!(lazy_start_plan(&mut s, 3), 1);
        assert!(s[0].started && !s[1].started && s[1].deferred);
    }

    #[test]
    fn f005_wake_pipeline_order() {
        let p = WakePipeline { done_mask: 0, total_ms: 900 };
        assert_eq!(wake_next(&p), Some(0));
        assert!(wake_order_valid(0b0100));
        assert!(!wake_order_valid(0b1000));
        assert!(wake_next(&WakePipeline { done_mask: 0b11111, total_ms: 0 }).is_none());
    }

    #[test]
    fn f006_animator_never_fakes() {
        assert_eq!(animator_progress(&BootAnimator { stages_done: 0, total_stages: 4, fake_progress: true }), 0);
        assert_eq!(animator_progress(&BootAnimator { stages_done: 4, total_stages: 4, fake_progress: false }), 1000);
    }

    #[test]
    fn f007_parallel_ready_respects_deps() {
        let deps = [[u8::MAX; 2], [0, u8::MAX], [0, 1], [u8::MAX; 2], [u8::MAX; 2]];
        assert_eq!(parallel_ready(&deps, 0), 0b11001);
        assert_eq!(parallel_ready(&deps, 0b00001), 0b11010);
        assert_eq!(parallel_ready(&deps, 0b11111), 0);
    }

    #[test]
    fn f008_critical_path_immediate_fail() {
        assert_eq!(crit_path_zero_wait(CritPathOp { stage: 3, retries_allowed: 0 }, true), 0xFF);
        assert_eq!(crit_path_zero_wait(CritPathOp { stage: 3, retries_allowed: 0 }, false), 3);
    }

    #[test]
    fn f009_tier_boundaries() {
        assert_eq!(latency_tier_of(0), 0);
        assert_eq!(latency_tier_of(100), 0);
        assert_eq!(latency_tier_of(101), 1);
        assert_eq!(latency_tier_of(2000), 1);
        assert_eq!(latency_tier_of(2001), 2);
    }

    #[test]
    fn f010_fingerprint_deterministic() {
        assert_eq!(sandbox_fingerprint(&[9, 9]), sandbox_fingerprint(&[9, 9]));
        assert_ne!(sandbox_fingerprint(&[9, 9]), sandbox_fingerprint(&[9, 8]));
        assert_eq!(sandbox_fingerprint(&[]), 0xcbf29ce484222325);
    }

    #[test]
    fn f011_caps_to_skip_mapping() {
        assert_eq!(firmware_skip_mask(0), 0);
        assert_eq!(firmware_skip_mask(0b001), 0b00001);
        assert_eq!(firmware_skip_mask(0b010), 0b10000);
        assert_eq!(firmware_skip_mask(0b011), 0b10001);
    }

    #[test]
    fn f012_warmer_orders_by_heat() {
        let mut w = PageWarmer::new();
        w.touch(10);
        w.touch(20);
        w.touch(20);
        w.touch(20);
        w.touch(30);
        let mut o = [0u64; WARM_MAX];
        assert_eq!(w.preload_order(&mut o), 3);
        assert_eq!(&o[..3], &[20, 10, 30]);
    }

    #[test]
    fn f013_ladder_by_ratio() {
        assert_eq!(degrade_ladder(8000, 8000), 0);
        assert_eq!(degrade_ladder(8800, 8000), 0b11000);
        assert_eq!(degrade_ladder(12000, 8000), 0b11100);
        assert_eq!(degrade_ladder(16000, 8000), 0b11110);
    }

    #[test]
    fn f014_gate_tiers() {
        let st = FastSelftest { quick_ms: 100, deep_ms: 500, quick_failed: false };
        assert_eq!(selftest_gate(&st, 1000), 2);
        assert_eq!(selftest_gate(&st, 400), 1);
        assert_eq!(selftest_gate(&st, 10), 0);
        let bad = FastSelftest { quick_ms: 100, deep_ms: 500, quick_failed: true };
        assert_eq!(selftest_gate(&bad, 1000), 1);
    }

    #[test]
    fn f015_blackbox_ring_wrap() {
        let mut bb = BlackBox::new();
        for i in 0..(BB_MAX as u16 + 3) {
            bb.log(i + 1);
        }
        let mut o = [0u16; BB_MAX];
        assert_eq!(bb.export(&mut o), BB_MAX);
        assert_eq!(o[0], 4);
        assert_eq!(o[BB_MAX - 1], BB_MAX as u16 + 3);
        assert_eq!(bb.dropped, 3);
    }

    #[test]
    fn f016_first_frame_rules() {
        assert!(first_frame_direct_allowed(true));
        assert!(!first_frame_direct_allowed(false));
    }

    #[test]
    fn f017_probe_timeout_independent() {
        let mut j = [ProbeJob { drv_id: 0, timeout_ms: 100, done: false }; DRV_PROBE_MAX];
        j[2].timeout_ms = 1000;
        assert_eq!(probe_parallel(&mut j, 4, 300), 3);
        assert!(j[0].done && j[1].done && !j[2].done && j[3].done);
    }

    #[test]
    fn f018_contract_records_breach() {
        let mut ok = StageContract { promised_ms: 300, actual_ms: 200, breach_recorded: false };
        assert!(contract_check(&mut ok));
        assert!(!ok.breach_recorded);
        let mut bad = StageContract { promised_ms: 100, actual_ms: 200, breach_recorded: false };
        assert!(!contract_check(&mut bad));
        assert!(bad.breach_recorded);
    }

    #[test]
    fn f019_snapshot_gates() {
        assert!(snapshot_restorable(&SessionSnapshot { magic_ok: true, version: 1, crc_ok: true }));
        assert!(!snapshot_restorable(&SessionSnapshot { magic_ok: false, version: 1, crc_ok: true }));
        assert!(!snapshot_restorable(&SessionSnapshot { magic_ok: true, version: 3, crc_ok: true }));
        assert!(!snapshot_restorable(&SessionSnapshot { magic_ok: true, version: 1, crc_ok: false }));
    }

    #[test]
    fn f020_tiering() {
        assert_eq!(tier_of(10, 4096), 0);
        assert_eq!(tier_of(9, 100), 2);
        assert_eq!(tier_of(10, 5000), 1);
    }

    #[test]
    fn f021_narrator_respects_switch() {
        let mut n = Narrator::new(false);
        n.enqueue(5);
        assert!(n.tick().is_none());
        let mut n = Narrator::new(true);
        n.enqueue(5);
        n.enqueue(6);
        assert_eq!(n.tick(), Some(5));
        assert_eq!(n.tick(), Some(6));
        assert!(n.tick().is_none());
        assert_eq!(n.spoken, 2);
    }

    #[test]
    fn f022_a11y_mask() {
        assert_eq!(a11y_first_frame_mask(&A11yBoot { high_contrast: true, large_text: true, screen_reader: true }), 0b111);
        assert_eq!(a11y_first_frame_mask(&A11yBoot { high_contrast: false, large_text: false, screen_reader: false }), 0);
    }

    #[test]
    fn f023_privacy_mask() {
        assert_eq!(privacy_boot_restore_mask(true), 0);
        assert_eq!(privacy_boot_restore_mask(false), 0b11);
    }

    #[test]
    fn f024_ratio_edges() {
        assert_eq!(image_compress_ratio(0, 0), 1000);
        assert_eq!(image_compress_ratio(10, 10), 1000);
        assert_eq!(image_compress_ratio(4, 1), 250);
    }

    #[test]
    fn f025_final_verdicts() {
        assert!(boot_final_verdict(true, false, false, 0, 8000));
        assert!(!boot_final_verdict(true, false, false, 0, 8001));
        assert!(!boot_final_verdict(true, false, false, 1, 100));
        assert!(boot_final_verdict(false, true, true, 0, 0));
        assert!(!boot_final_verdict(false, true, false, 0, 0));
    }

    #[test]
    fn domain_self_test_must_pass() {
        let set = run_m600boot_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("m600boot self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 25);
    }
}
