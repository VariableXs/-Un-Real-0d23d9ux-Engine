//! m700gpu — VARIX-M700 AI-15 GPU 驱动域 (F351~F375)
//!
//! GPU 抽象宪法/命令缓冲谱/显存管理官/多后端仲裁/GPU 崩溃隔离舱/
//! 围栏时间线/着色器预处理舱/GPU 能力档案/GPU 帧预算官/显存压力谱/
//! GPU 统计分账/GPU 命令回放/GPU fuzz 桩/硬件光标通道/GPU 电源契约/
//! GPU 错误分类官/虚拟 GPU 舱/GPU 基准剧本/显存泄露纠察/GPU 时间轴/
//! GPU 回归走廊/后端移植侦察/GPU 健康分/GPU 文档生成器/GPU 域年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F351 — GPU 抽象宪法：后端必备入口
// ===========================================================================

/// 后端 vtable：probe/submit/fence/vram/shutdown 缺一不可（0 = 缺席）。
#[derive(Clone, Copy, Debug)]
pub struct GpuVtable {
    pub probe: u32,
    pub submit: u32,
    pub fence: u32,
    pub vram: u32,
    pub shutdown: u32,
}

pub const GPU_VTABLE_ENTRIES: usize = 5;

impl GpuVtable {
    pub fn constitutional(&self) -> bool {
        self.probe != 0 && self.submit != 0 && self.fence != 0 && self.vram != 0 && self.shutdown != 0
    }
}

// ===========================================================================
// F352 — 命令缓冲谱：命令入队与溢出
// ===========================================================================

pub const CMD_BUF_CAP: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuCmd {
    Draw(u32),
    Bind(u32),
    Clear(u32),
    Barrier,
}

#[derive(Clone, Copy, Debug)]
pub struct CmdBuffer {
    cmds: [Option<GpuCmd>; CMD_BUF_CAP],
    len: usize,
    pub overflow: u32,
}

impl CmdBuffer {
    pub const fn new() -> CmdBuffer {
        CmdBuffer { cmds: [None; CMD_BUF_CAP], len: 0, overflow: 0 }
    }

    pub fn push(&mut self, cmd: GpuCmd) -> bool {
        if self.len < CMD_BUF_CAP {
            self.cmds[self.len] = Some(cmd);
            self.len += 1;
            true
        } else {
            self.overflow += 1;
            false
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn cmd_at(&self, i: usize) -> Option<GpuCmd> {
        if i < self.len {
            self.cmds[i]
        } else {
            None
        }
    }

    pub fn clear(&mut self) {
        let mut i = 0;
        while i < self.len {
            self.cmds[i] = None;
            i += 1;
        }
        self.len = 0;
    }
}

// ===========================================================================
// F353 — 显存管理官：定容区间分配
// ===========================================================================

pub const VRAM_TOTAL_MB: u32 = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VramBlock {
    pub offset_mb: u32,
    pub size_mb: u32,
}

impl VramBlock {
    pub fn end_mb(&self) -> u32 {
        self.offset_mb + self.size_mb
    }

    pub fn overlaps(&self, other: &VramBlock) -> bool {
        self.offset_mb < other.end_mb() && other.offset_mb < self.end_mb()
    }
}

/// 显存申请：非零、对齐 4MB、不与已分配块重叠、不越界。
pub fn vram_allocate(allocated: &[VramBlock], want: VramBlock) -> bool {
    if want.size_mb == 0 || want.size_mb % 4 != 0 || want.end_mb() > VRAM_TOTAL_MB {
        return false;
    }
    allocated.iter().all(|b| !b.overlaps(&want))
}

// ===========================================================================
// F354 — 多后端仲裁：按能力命中数选后端
// ===========================================================================

pub const GPU_CAP_2D: u32 = 1;
pub const GPU_CAP_3D: u32 = 2;
pub const GPU_CAP_COMPUTE: u32 = 4;
pub const GPU_CAP_VIDEO: u32 = 8;

/// 需求位与后端能力位的交集大小即得分。
pub fn backend_score(backend_caps: u32, required: u32) -> u32 {
    (backend_caps & required).count_ones()
}

/// 挑出得分最高的后端（全 0 返回 None）。
pub fn best_backend(backends: &[u32], required: u32) -> Option<usize> {
    let mut best: Option<(usize, u32)> = None;
    for (i, &caps) in backends.iter().enumerate() {
        let s = backend_score(caps, required);
        if s > 0 {
            match best {
                Some((_, bs)) if bs >= s => {}
                _ => best = Some((i, s)),
            }
        }
    }
    best.map(|(i, _)| i)
}

// ===========================================================================
// F355 — GPU 崩溃隔离舱：上下文级复位
// ===========================================================================

pub const GPU_CONTEXTS: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct GpuContexts {
    pub alive: [bool; GPU_CONTEXTS],
    pub resets: u32,
}

impl GpuContexts {
    pub const fn new() -> GpuContexts {
        GpuContexts { alive: [true; GPU_CONTEXTS], resets: 0 }
    }

    /// 隔离复位：只杀指定上下文，其余存活。
    pub fn isolate_reset(&mut self, ctx: usize) -> bool {
        if ctx >= GPU_CONTEXTS || !self.alive[ctx] {
            return false;
        }
        self.alive[ctx] = false;
        self.resets += 1;
        true
    }

    pub fn survivors(&self) -> u32 {
        self.alive.iter().filter(|a| **a).count() as u32
    }
}

// ===========================================================================
// F356 — 围栏时间线：单调围栏号与完成集合
// ===========================================================================

pub const FENCE_TIMELINE_CAP: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct FenceTimeline {
    pub submitted: u64,
    pub signaled: [bool; FENCE_TIMELINE_CAP], // 按提交序取模
    pub completed: u64,
}

impl FenceTimeline {
    pub const fn new() -> FenceTimeline {
        FenceTimeline { submitted: 0, signaled: [false; FENCE_TIMELINE_CAP], completed: 0 }
    }

    pub fn submit(&mut self) -> u64 {
        self.submitted += 1;
        self.submitted
    }

    /// 完成围栏：只允许按序完成（completed+1），乱序拒绝。
    pub fn signal(&mut self, fence: u64) -> bool {
        if fence != self.completed + 1 || fence > self.submitted {
            return false;
        }
        self.signaled[(fence as usize - 1) % FENCE_TIMELINE_CAP] = true;
        self.completed = fence;
        true
    }

    pub fn is_signaled(&self, fence: u64) -> bool {
        fence <= self.completed
    }
}

// ===========================================================================
// F357 — 着色器预处理舱：头校验与注释剥离
// ===========================================================================

pub const SHADER_MAGIC: [u8; 2] = [0x56, 0x58]; // "VX"
pub const SHADER_VERSION: u8 = 2;

/// 头校验：前两字节为魔数，第三字节为版本。
pub fn shader_header_ok(src: &[u8]) -> bool {
    src.len() >= 3 && src[0] == SHADER_MAGIC[0] && src[1] == SHADER_MAGIC[1] && src[2] == SHADER_VERSION
}

/// 注释剥离：统计 `//` 到行尾的字节数（不实际复制）。
pub fn strip_comment_bytes(src: &[u8]) -> usize {
    let mut removed = 0usize;
    let mut i = 0usize;
    while i + 1 < src.len() {
        if src[i] == b'/' && src[i + 1] == b'/' {
            removed += src.len() - i;
            break;
        }
        i += 1;
    }
    removed
}

// ===========================================================================
// F358 — GPU 能力档案：特性位图
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuCaps {
    pub flags: u32,
}

impl GpuCaps {
    pub const fn new(flags: u32) -> GpuCaps {
        GpuCaps { flags }
    }

    pub fn has(&self, cap: u32) -> bool {
        self.flags & cap == cap
    }

    pub fn satisfies(&self, required: u32) -> bool {
        self.flags & required == required
    }
}

// ===========================================================================
// F359 — GPU 帧预算官：逐阶段预算合成
// ===========================================================================

pub const FRAME_BUDGET_US: u32 = 16_600; // 60fps

/// 各阶段（顶点/光栅/计算/回读）耗时之和不得超帧预算。
pub fn frame_within_budget(stage_us: &[u32; 4]) -> bool {
    let mut total = 0u32;
    for &s in stage_us.iter() {
        total += s;
    }
    total <= FRAME_BUDGET_US
}

/// 阶段超支 permille（相对帧预算）。
pub fn frame_overshoot_permille(stage_us: &[u32; 4]) -> u32 {
    let mut total = 0u32;
    for &s in stage_us.iter() {
        total += s;
    }
    if total <= FRAME_BUDGET_US {
        0
    } else {
        (total - FRAME_BUDGET_US) * 1000 / FRAME_BUDGET_US
    }
}

// ===========================================================================
// F360 — 显存压力谱：水位 + LRU 逐出候选
// ===========================================================================

pub const VRAM_HIGH_PERMILLE: u32 = 850;

#[derive(Clone, Copy, Debug)]
pub struct VramResident {
    pub block: VramBlock,
    pub last_used_frame: u32,
}

/// 压力判定：占用 permille 超高水位。
pub fn vram_pressure(used_permille: u32) -> bool {
    used_permille >= VRAM_HIGH_PERMILLE
}

/// LRU：选出最久未用的驻留块索引（空表返回 None）。
pub fn lru_victim(resident: &[VramResident]) -> Option<usize> {
    let mut best: Option<(usize, u32)> = None;
    for (i, r) in resident.iter().enumerate() {
        match best {
            Some((_, bf)) if bf <= r.last_used_frame => {}
            _ => best = Some((i, r.last_used_frame)),
        }
    }
    best.map(|(i, _)| i)
}

// ===========================================================================
// F361 — GPU 统计分账：按队列分账
// ===========================================================================

pub const GPU_QUEUES: usize = 4;

#[derive(Clone, Copy, Debug, Default)]
pub struct QueueStat {
    pub submits: u64,
    pub errors: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct GpuStatsBook {
    pub queues: [QueueStat; GPU_QUEUES],
}

impl GpuStatsBook {
    pub const fn new() -> GpuStatsBook {
        GpuStatsBook { queues: [QueueStat { submits: 0, errors: 0 }; GPU_QUEUES] }
    }

    pub fn total_submits(&self) -> u64 {
        self.queues.iter().map(|q| q.submits).sum()
    }

    pub fn error_permille(&self) -> u32 {
        let total = self.total_submits();
        if total == 0 {
            return 0;
        }
        let errs: u64 = self.queues.iter().map(|q| q.errors).sum();
        (errs * 1000 / total) as u32
    }
}

// ===========================================================================
// F362 — GPU 命令回放：录制/校验/重放一致
// ===========================================================================

/// 命令序列指纹：逐条 cmd 的判别式与载荷折叠（FNV 风格，无乘法溢出顾虑用 wrapping）。
pub fn cmd_fingerprint(cmds: &[GpuCmd]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for c in cmds {
        let (tag, payload) = match *c {
            GpuCmd::Draw(v) => (1u32, v),
            GpuCmd::Bind(v) => (2, v),
            GpuCmd::Clear(v) => (3, v),
            GpuCmd::Barrier => (4, 0),
        };
        h ^= tag;
        h = h.wrapping_mul(0x0100_0193);
        h ^= payload;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 回放一致性：指纹相同即视为同一命令流。
pub fn replay_fingerprint_match(a: &[GpuCmd], b: &[GpuCmd]) -> bool {
    cmd_fingerprint(a) == cmd_fingerprint(b)
}

// ===========================================================================
// F363 — GPU fuzz 桩：畸形命令必须被拒
// ===========================================================================

/// fuzz 门：payload 不得为 0，tag 合法范围 1..=4。
pub fn fuzz_cmd_ok(tag: u32, payload: u32) -> bool {
    (1..=4).contains(&tag) && payload != 0
}

/// 确定性变异样本。
pub fn fuzz_gpu_sample(seed: u32, out: &mut [u32; 4]) {
    let mut x = seed | 1;
    for slot in out.iter_mut() {
        x = x.wrapping_mul(747_796_405).wrapping_add(2_891_337_673);
        *slot = x;
    }
}

// ===========================================================================
// F364 — 硬件光标通道：位置与热点
// ===========================================================================

pub const CURSOR_SIZE_PX: u32 = 64;

#[derive(Clone, Copy, Debug)]
pub struct HwCursor {
    pub x: i32,
    pub y: i32,
    pub hotspot_x: u32,
    pub hotspot_y: u32,
}

impl HwCursor {
    /// 热点必须在光标图内；位置可以为负（跨屏边缘）。
    pub fn valid(&self) -> bool {
        self.hotspot_x < CURSOR_SIZE_PX && self.hotspot_y < CURSOR_SIZE_PX
    }

    /// 夹取到可见区域 [0, w) × [0, h)。
    pub fn clamped(&self, screen_w: u32, screen_h: u32) -> (i32, i32) {
        let cx = self.x.clamp(0, screen_w as i32 - 1);
        let cy = self.y.clamp(0, screen_h as i32 - 1);
        (cx, cy)
    }
}

// ===========================================================================
// F365 — GPU 电源契约：P 状态迁移
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuPower {
    P0, // 全速
    P1,
    P2,
    P8, // 深睡
}

pub fn gpu_power_transition_ok(from: GpuPower, to: GpuPower) -> bool {
    use GpuPower::*;
    matches!((from, to), (P0, P1) | (P1, P2) | (P2, P8) | (P2, P1) | (P1, P0) | (P8, P2))
}

/// P8 唤醒延迟契约：固定 50ms。
pub const GPU_WAKE_FROM_P8_MS: u32 = 50;

// ===========================================================================
// F366 — GPU 错误分类官
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuError {
    ContextHung,
    PageFault(u32), // 出错地址
    Timeout,
    PowerFail,
}

/// 可通过复位恢复： hung / timeout 可复位；页故障只逐出上下文；电源故障要重载。
pub fn gpu_error_resettable(e: GpuError) -> bool {
    matches!(e, GpuError::ContextHung | GpuError::Timeout)
}

// ===========================================================================
// F367 — 虚拟 GPU 舱：显存分区租户
// ===========================================================================

pub const VGPU_TENANTS: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct VgpuPartition {
    pub tenant_caps_mb: [u32; VGPU_TENANTS],
    pub tenant_used_mb: [u32; VGPU_TENANTS],
}

impl VgpuPartition {
    pub const fn new() -> VgpuPartition {
        VgpuPartition { tenant_caps_mb: [0; VGPU_TENANTS], tenant_used_mb: [0; VGPU_TENANTS] }
    }

    /// 划分配额：总配额不得超显存总量。
    pub fn partition(&mut self, caps: &[u32; VGPU_TENANTS]) -> bool {
        let mut total = 0u32;
        for &c in caps.iter() {
            total += c;
        }
        if total > VRAM_TOTAL_MB {
            return false;
        }
        self.tenant_caps_mb = *caps;
        true
    }

    /// 租户申请：配额内才批。
    pub fn tenant_alloc(&mut self, tenant: usize, mb: u32) -> bool {
        if tenant >= VGPU_TENANTS {
            return false;
        }
        let used = self.tenant_used_mb[tenant] + mb;
        if used > self.tenant_caps_mb[tenant] {
            return false;
        }
        self.tenant_used_mb[tenant] = used;
        true
    }
}

// ===========================================================================
// F368 — GPU 基准剧本：整数得分
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct BenchCase {
    pub name: &'static str,
    pub frames: u32,
    pub ms_used: u32,
}

/// 单项得分 = 帧 × 1000 / 毫秒（帧/秒 ×1000，定点 fps）。
pub fn bench_score(c: &BenchCase) -> u32 {
    if c.ms_used == 0 {
        return 0;
    }
    c.frames * 1000 / c.ms_used
}

/// 全部场景帧率不低于 30fps。
pub fn bench_all_30fps(cases: &[BenchCase]) -> bool {
    !cases.is_empty() && cases.iter().all(|c| bench_score(c) >= 30)
}

// ===========================================================================
// F369 — 显存泄露纠察：分配/释放配平
// ===========================================================================

pub const VRAM_LIVE_CAP: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct VramLeakWatch {
    live: [bool; VRAM_LIVE_CAP],
    pub allocs: u64,
    pub frees: u64,
}

impl VramLeakWatch {
    pub const fn new() -> VramLeakWatch {
        VramLeakWatch { live: [false; VRAM_LIVE_CAP], allocs: 0, frees: 0 }
    }

    pub fn alloc(&mut self) -> Option<usize> {
        for i in 0..VRAM_LIVE_CAP {
            if !self.live[i] {
                self.live[i] = true;
                self.allocs += 1;
                return Some(i);
            }
        }
        None
    }

    pub fn free(&mut self, slot: usize) -> bool {
        if slot >= VRAM_LIVE_CAP || !self.live[slot] {
            return false;
        }
        self.live[slot] = false;
        self.frees += 1;
        true
    }

    pub fn live_count(&self) -> u32 {
        self.live.iter().filter(|l| **l).count() as u32
    }

    /// 泄露 = 历史分配多于历史释放且仍有驻留。
    pub fn leaking(&self) -> bool {
        self.live_count() > 0
    }
}

// ===========================================================================
// F370 — GPU 时间轴：事件有序性
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuTimelineEvent {
    pub at_us: u64,
    pub tag: u32,
}

/// 时间轴合法：严格递增时间戳。
pub fn timeline_ordered(events: &[GpuTimelineEvent]) -> bool {
    let mut i = 1usize;
    while i < events.len() {
        if events[i].at_us <= events[i - 1].at_us {
            return false;
        }
        i += 1;
    }
    true
}

/// 两事件区间是否重叠（按 tag 区间）。
pub fn timeline_overlap(a: &GpuTimelineEvent, b: &GpuTimelineEvent, span_us: u64) -> bool {
    let (s, e) = (a.at_us, a.at_us + span_us);
    let (s2, e2) = (b.at_us, b.at_us + span_us);
    s < e2 && s2 < e
}

// ===========================================================================
// F371 — GPU 回归走廊
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct GpuScenario {
    pub name: &'static str,
    pub passed: bool,
}

pub fn gpu_regression_green(scenarios: &[GpuScenario]) -> bool {
    !scenarios.is_empty() && scenarios.iter().all(|s| s.passed && !s.name.is_empty())
}

// ===========================================================================
// F372 — 后端移植侦察：移植能力清单
// ===========================================================================

/// 移植侦察：目标后端必须满足全部硬性能力位。
pub const PORT_HARD_REQUIREMENTS: u32 = GPU_CAP_2D | GPU_CAP_3D;

pub fn port_ready(caps: u32) -> bool {
    caps & PORT_HARD_REQUIREMENTS == PORT_HARD_REQUIREMENTS
}

/// 软性能力覆盖率 permille（侦察报告用）。
pub fn port_soft_coverage_permille(caps: u32, soft_wanted: u32) -> u32 {
    let soft = soft_wanted & !PORT_HARD_REQUIREMENTS;
    if soft == 0 {
        return 1000;
    }
    (caps & soft).count_ones() * 1000 / soft.count_ones()
}

// ===========================================================================
// F373 — GPU 健康分
// ===========================================================================

/// 健康分 = 1000 - 复位×30 - 错误率‰×3，下限 0。
pub fn gpu_health(submits: u64, errors: u64, resets: u32) -> u32 {
    let err_pm = if submits == 0 { 0 } else { (errors * 1000 / submits) as u32 };
    1000u32
        .saturating_sub(resets.saturating_mul(30))
        .saturating_sub(err_pm.saturating_mul(3))
}

// ===========================================================================
// F374 — GPU 文档生成器
// ===========================================================================

pub const GPU_DOC_SECTIONS: [&str; 5] = ["vtable", "cmd-format", "vram-layout", "fences", "power"];

pub fn gpu_doc_complete(filled: u32) -> bool {
    filled >= GPU_DOC_SECTIONS.len() as u32
}

// ===========================================================================
// F375 — GPU 域年报
// ===========================================================================

pub const GPU_REPORT_SECTIONS: [&str; 5] = ["backends", "vram", "crashes", "bench", "fuzz"];

pub fn gpu_report_complete(filled: u32) -> bool {
    filled >= GPU_REPORT_SECTIONS.len() as u32
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m700gpu_checks() -> CheckSet {
    let mut set = CheckSet::new("m700gpu");

    // F351 GPU 抽象宪法
    let full = GpuVtable { probe: 1, submit: 2, fence: 3, vram: 4, shutdown: 5 };
    let missing = GpuVtable { probe: 1, submit: 2, fence: 0, vram: 4, shutdown: 5 };
    set.add("F351 vtable full", full.constitutional(), "five entries");
    set.add("F351 vtable missing", !missing.constitutional(), "fence required");

    // F352 命令缓冲谱
    let mut cb = CmdBuffer::new();
    let p1 = cb.push(GpuCmd::Bind(7));
    let p2 = cb.push(GpuCmd::Draw(64));
    let len_two = cb.len();
    let mut i = 0;
    while cb.push(GpuCmd::Clear(0xFF)) {
        i += 1;
    }
    let overflow_now = cb.overflow;
    set.add(
        "F352 cmd push",
        p1 && p2 && len_two == 2 && cb.cmd_at(1) == Some(GpuCmd::Draw(64)),
        "in order",
    );
    set.add(
        "F352 cmd overflow",
        i == CMD_BUF_CAP - 2 && overflow_now == 1 && cb.len() == CMD_BUF_CAP,
        "filled to cap",
    );
    cb.push(GpuCmd::Barrier);
    set.add("F352 cmd drop on full", cb.overflow == 2 && cb.len() == CMD_BUF_CAP, "no silent growth");

    // F353 显存管理官
    let allocated = [VramBlock { offset_mb: 0, size_mb: 64 }, VramBlock { offset_mb: 128, size_mb: 32 }];
    set.add(
        "F353 vram alloc ok",
        vram_allocate(&allocated, VramBlock { offset_mb: 64, size_mb: 16 }),
        "hole fits",
    );
    set.add(
        "F353 vram alloc bad",
        !vram_allocate(&allocated, VramBlock { offset_mb: 48, size_mb: 32 })
            && !vram_allocate(&allocated, VramBlock { offset_mb: 100, size_mb: 2 })
            && !vram_allocate(&allocated, VramBlock { offset_mb: 250, size_mb: 8 }),
        "overlap/align/bounds",
    );

    // F354 多后端仲裁
    let backends = [GPU_CAP_2D, GPU_CAP_2D | GPU_CAP_3D, GPU_CAP_3D | GPU_CAP_COMPUTE | GPU_CAP_VIDEO];
    set.add(
        "F354 backend score",
        backend_score(backends[1], GPU_CAP_2D | GPU_CAP_3D) == 2,
        "both match",
    );
    set.add(
        "F354 best backend",
        best_backend(&backends, GPU_CAP_3D) == Some(1),
        "first capable on tie",
    );
    set.add(
        "F354 no capable backend",
        best_backend(&[GPU_CAP_2D], GPU_CAP_COMPUTE).is_none(),
        "none matched",
    );

    // F355 GPU 崩溃隔离舱
    let mut ctx = GpuContexts::new();
    let reset1 = ctx.isolate_reset(2);
    let survivors_after = ctx.survivors();
    set.add(
        "F355 isolate reset",
        reset1 && survivors_after == 3 && ctx.resets == 1,
        "one context down",
    );
    set.add(
        "F355 no double reset",
        !ctx.isolate_reset(2) && ctx.resets == 1,
        "already dead",
    );

    // F356 围栏时间线
    let mut tl = FenceTimeline::new();
    let f1 = tl.submit();
    let f2 = tl.submit();
    let early = tl.signal(2);
    let in_order = tl.signal(f1);
    let completed_one = tl.completed;
    set.add(
        "F356 fence submit",
        f1 == 1 && f2 == 2,
        "monotonic ids",
    );
    set.add(
        "F356 fence in-order signal",
        !early && in_order && completed_one == 1 && tl.is_signaled(1) && !tl.is_signaled(2),
        "no reordering",
    );

    // F357 着色器预处理舱
    let good_shader = [0x56u8, 0x58, 2, 0x01, 0x02];
    let bad_shader = [0x56u8, 0x58, 9, 0x01];
    set.add(
        "F357 shader header",
        shader_header_ok(&good_shader) && !shader_header_ok(&bad_shader) && !shader_header_ok(&[0x56, 0x58]),
        "magic+version+len",
    );
    let commented = b"draw(); // trail comment";
    set.add(
        "F357 comment strip",
        strip_comment_bytes(commented) == 16,
        "from // to end",
    );

    // F358 GPU 能力档案
    let caps = GpuCaps::new(GPU_CAP_2D | GPU_CAP_3D | GPU_CAP_COMPUTE);
    set.add(
        "F358 caps query",
        caps.has(GPU_CAP_3D) && !caps.has(GPU_CAP_VIDEO),
        "bit check",
    );
    set.add(
        "F358 caps satisfy",
        caps.satisfies(GPU_CAP_2D | GPU_CAP_3D) && !caps.satisfies(GPU_CAP_COMPUTE | GPU_CAP_VIDEO),
        "subset rule",
    );

    // F359 GPU 帧预算官
    let fast = [2000u32, 8000, 3000, 2000];
    let slow = [6000u32, 8000, 6000, 2000];
    set.add(
        "F359 frame budget ok",
        frame_within_budget(&fast) && !frame_within_budget(&slow),
        "16.6ms line",
    );
    set.add(
        "F359 overshoot permille",
        frame_overshoot_permille(&fast) == 0 && frame_overshoot_permille(&slow) > 0,
        "0 or positive",
    );

    // F360 显存压力谱
    let resident = [
        VramResident { block: VramBlock { offset_mb: 0, size_mb: 16 }, last_used_frame: 90 },
        VramResident { block: VramBlock { offset_mb: 16, size_mb: 16 }, last_used_frame: 10 },
        VramResident { block: VramBlock { offset_mb: 32, size_mb: 16 }, last_used_frame: 50 },
    ];
    set.add(
        "F360 pressure gate",
        vram_pressure(850) && !vram_pressure(849),
        "850‰ threshold",
    );
    set.add("F360 lru victim", lru_victim(&resident) == Some(1), "oldest frame first");
    set.add("F360 lru empty", lru_victim(&[]).is_none(), "nothing resident");

    // F361 GPU 统计分账
    let mut book = GpuStatsBook::new();
    book.queues[0].submits = 700;
    book.queues[1].submits = 300;
    book.queues[1].errors = 5;
    let err_pm_now = book.error_permille();
    set.add(
        "F361 stats totals",
        book.total_submits() == 1000 && err_pm_now == 5,
        "5‰ errors",
    );
    set.add("F361 stats empty", GpuStatsBook::new().error_permille() == 0, "no div-zero");

    // F362 GPU 命令回放
    let run_a = [GpuCmd::Bind(1), GpuCmd::Draw(16), GpuCmd::Barrier];
    let run_b = [GpuCmd::Bind(1), GpuCmd::Draw(16), GpuCmd::Barrier];
    let run_c = [GpuCmd::Bind(1), GpuCmd::Draw(32), GpuCmd::Barrier];
    let fp = cmd_fingerprint(&run_a);
    set.add(
        "F362 fingerprint stable",
        fp == cmd_fingerprint(&run_b) && fp != cmd_fingerprint(&run_c),
        "payload sensitive",
    );
    set.add("F362 replay match", replay_fingerprint_match(&run_a, &run_b), "same stream");

    // F363 GPU fuzz 桩
    set.add(
        "F363 fuzz gate",
        fuzz_cmd_ok(1, 64) && !fuzz_cmd_ok(0, 64) && !fuzz_cmd_ok(5, 64) && !fuzz_cmd_ok(3, 0),
        "tag range, nonzero payload",
    );
    let mut s1 = [0u32; 4];
    let mut s2 = [0u32; 4];
    fuzz_gpu_sample(7, &mut s1);
    fuzz_gpu_sample(7, &mut s2);
    set.add("F363 fuzz deterministic", s1 == s2, "same seed same stream");

    // F364 硬件光标通道
    let cur = HwCursor { x: 100, y: 50, hotspot_x: 10, hotspot_y: 2 };
    let bad_cur = HwCursor { x: 0, y: 0, hotspot_x: 64, hotspot_y: 0 };
    set.add(
        "F364 cursor valid",
        cur.valid() && !bad_cur.valid(),
        "hotspot inside",
    );
    set.add(
        "F364 cursor clamp",
        HwCursor { x: -5, y: 2000, hotspot_x: 1, hotspot_y: 1 }.clamped(1920, 1080) == (0, 1079),
        "edge clamp",
    );

    // F365 GPU 电源契约
    set.add(
        "F365 power ladder",
        gpu_power_transition_ok(GpuPower::P0, GpuPower::P1)
            && gpu_power_transition_ok(GpuPower::P2, GpuPower::P8)
            && gpu_power_transition_ok(GpuPower::P8, GpuPower::P2)
            && !gpu_power_transition_ok(GpuPower::P0, GpuPower::P8),
        "stepwise only",
    );
    set.add("F365 wake latency", GPU_WAKE_FROM_P8_MS == 50, "contract ms");

    // F366 GPU 错误分类官
    set.add(
        "F366 error classes",
        gpu_error_resettable(GpuError::ContextHung) && gpu_error_resettable(GpuError::Timeout)
            && !gpu_error_resettable(GpuError::PageFault(0xDEAD))
            && !gpu_error_resettable(GpuError::PowerFail),
        "reset subset",
    );

    // F367 虚拟 GPU 舱
    let mut vg = VgpuPartition::new();
    let split = vg.partition(&[64, 64, 64, 64]);
    let a1 = vg.tenant_alloc(0, 64);
    let a2 = vg.tenant_alloc(0, 1);
    set.add(
        "F367 vgpu partition",
        split && a1 && vg.tenant_used_mb[0] == 64,
        "quota granted",
    );
    set.add(
        "F367 vgpu quota cap",
        !a2 && !vg.partition(&[100, 100, 100, 100]),
        "over quota & over total",
    );

    // F368 GPU 基准剧本
    let benches = [
        BenchCase { name: "triad", frames: 60, ms_used: 1000 },
        BenchCase { name: "compute", frames: 25, ms_used: 1000 },
    ];
    set.add(
        "F368 bench score",
        bench_score(&benches[0]) == 60 && bench_score(&benches[1]) == 25,
        "integer fps",
    );
    set.add(
        "F368 bench 30fps bar",
        !bench_all_30fps(&benches) && bench_all_30fps(&[benches[0]]),
        "25fps fails",
    );

    // F369 显存泄露纠察
    let mut lw = VramLeakWatch::new();
    let h1 = lw.alloc();
    let h2 = lw.alloc();
    let live_two = lw.live_count();
    lw.free(h1.unwrap());
    set.add(
        "F369 leak watch live",
        h1.is_some() && h2.is_some() && live_two == 2 && lw.live_count() == 1,
        "alloc/free slots",
    );
    set.add(
        "F369 leak detected",
        lw.leaking() && lw.allocs == 2 && lw.frees == 1,
        "residency mismatch",
    );
    lw.free(h2.unwrap());
    set.add("F369 leak cleared", !lw.leaking() && lw.allocs == lw.frees, "balanced");

    // F370 GPU 时间轴
    let events = [
        GpuTimelineEvent { at_us: 100, tag: 1 },
        GpuTimelineEvent { at_us: 300, tag: 2 },
        GpuTimelineEvent { at_us: 900, tag: 3 },
    ];
    let unordered = [
        GpuTimelineEvent { at_us: 100, tag: 1 },
        GpuTimelineEvent { at_us: 100, tag: 2 },
    ];
    set.add("F370 timeline ordered", timeline_ordered(&events), "strictly increasing");
    set.add(
        "F370 timeline tie caught",
        !timeline_ordered(&unordered),
        "no ties allowed",
    );
    let e1 = GpuTimelineEvent { at_us: 0, tag: 9 };
    let e2 = GpuTimelineEvent { at_us: 500, tag: 9 };
    set.add(
        "F370 overlap",
        timeline_overlap(&e1, &e2, 600) && !timeline_overlap(&e1, &e2, 400),
        "span dependent",
    );

    // F371 GPU 回归走廊
    let green = [
        GpuScenario { name: "fence-storm", passed: true },
        GpuScenario { name: "vram-fragment", passed: true },
    ];
    let red = [GpuScenario { name: "ctx-reset", passed: false }];
    set.add("F371 regression green", gpu_regression_green(&green), "all pass");
    set.add(
        "F371 regression red",
        !gpu_regression_green(&red) && !gpu_regression_green(&[]),
        "fail & empty rejected",
    );

    // F372 后端移植侦察
    set.add(
        "F372 port hard reqs",
        port_ready(GPU_CAP_2D | GPU_CAP_3D | GPU_CAP_COMPUTE) && port_ready(GPU_CAP_2D | GPU_CAP_3D),
        "hard satisfied",
    );
    set.add(
        "F372 port missing hard",
        !port_ready(GPU_CAP_2D | GPU_CAP_COMPUTE),
        "3d required",
    );
    set.add(
        "F372 port soft coverage",
        port_soft_coverage_permille(GPU_CAP_2D | GPU_CAP_3D | GPU_CAP_COMPUTE, GPU_CAP_COMPUTE | GPU_CAP_VIDEO) == 500,
        "1 of 2 soft caps",
    );

    // F373 GPU 健康分
    set.add("F373 gpu healthy", gpu_health(10_000, 0, 0) == 1000, "flawless");
    set.add(
        "F373 gpu degraded",
        gpu_health(10_000, 100, 2) == 1000 - 60 - 30,
        "reset+error demerits",
    );

    // F374 GPU 文档生成器
    set.add(
        "F374 gpu doc",
        GPU_DOC_SECTIONS.len() == 5 && gpu_doc_complete(5) && !gpu_doc_complete(3),
        "sections complete",
    );

    // F375 GPU 域年报
    set.add(
        "F375 gpu report",
        GPU_REPORT_SECTIONS.len() == 5 && gpu_report_complete(5) && !gpu_report_complete(2),
        "sections complete",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f352_cmd_buffer_overflow() {
        let mut cb = CmdBuffer::new();
        for i in 0..CMD_BUF_CAP {
            assert!(cb.push(GpuCmd::Draw(i as u32)));
        }
        assert!(!cb.push(GpuCmd::Barrier));
        assert_eq!(cb.overflow, 1);
        cb.clear();
        assert!(cb.push(GpuCmd::Barrier));
    }

    #[test]
    fn f353_vram_bounds() {
        let none: [VramBlock; 0] = [];
        assert!(vram_allocate(&none, VramBlock { offset_mb: 0, size_mb: 256 }));
        assert!(!vram_allocate(&none, VramBlock { offset_mb: 0, size_mb: 260 })); // 越界
        assert!(!vram_allocate(&none, VramBlock { offset_mb: 0, size_mb: 3 })); // 未对齐
    }

    #[test]
    fn f356_fence_strict_order() {
        let mut tl = FenceTimeline::new();
        let f1 = tl.submit(); // 1
        let f2 = tl.submit(); // 2
        assert!(!tl.signal(f2)); // 未完成 f1 前不许 f2
        assert!(tl.signal(f1));
        assert!(tl.is_signaled(f1));
        assert!(!tl.is_signaled(f2));
    }

    #[test]
    fn f362_fingerprint_payload_sensitive() {
        let a = [GpuCmd::Draw(1), GpuCmd::Draw(2)];
        let b = [GpuCmd::Draw(2), GpuCmd::Draw(1)];
        assert_ne!(cmd_fingerprint(&a), cmd_fingerprint(&b)); // 顺序敏感
    }

    #[test]
    fn f367_vgpu_quota_math() {
        let mut vg = VgpuPartition::new();
        assert!(vg.partition(&[100, 50, 50, 56])); // 恰好 256
        assert!(vg.tenant_alloc(1, 50));
        assert!(!vg.tenant_alloc(1, 1));
        assert!(vg.tenant_alloc(3, 56));
        assert!(!vg.tenant_alloc(3, 1));
    }

    #[test]
    fn f375_domain_selfcheck_all_pass() {
        let set = run_m700gpu_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
