//! F053 启动并行度 · 深化件（AI-K1 深化批次三 · G-B-13）。
//!
//! | # | 主册原文 | 本件机制 |
//! | --- | --- | --- |
//! | 1 | 【交互设计】「诊断中心「启动时间线」页：横向**甘特图逐链展示（每链每阶段起止）**；**与 8 秒目标差值标红**」 | [`Gantt`] 甘特图数据面（链 × 阶段起止 + 差值标红判定） |
//! | 2 | 【状态与异常】「**某链失败 → 后续依赖该链的阶段跳过并标注**（存储链失败仍可进安全模式 F193）」 | [`FailurePropagation`] 依赖传播（失败链的下游阶段标记跳过并说明） |
//! | 3 | 【状态与异常】「**并行竞争（两链抢同一资源）→ 锁等待归因入时间线**」 | [`LockWait`] 锁等待账（谁等谁、等多久、入哪条链） |
//! | 4 | 【设计细节】「**每链超时阈值独立（USB 3s 宽限/存储 1s 严格）**」 | [`TimeoutPolicy`] 逐链超时表（宽限/严格不同阈值，不统一拍一个数） |
//! | 5 | 【设计细节】「**动画启动点提前到内核段 80% 处**（画面先动起来，感知时间再减 0.5s）」 | [`AnimKickoff`] 动画起播点（按内核段进度触发，不按内核段结束） |
//! | 6 | 【设计细节】「四链依赖矩阵**文档化**（谁真依赖谁——多数「依赖」是历史串行的惯性）」 | [`DepMatrix`] 依赖矩阵（可查询、可审计，虚假依赖能被标出） |
//! | 7 | 【验收判据】「**8 秒线分解后每段预算在甘特图可见且实测偏差 <10%**」 | [`Budget8s`] 八秒预算分解与偏差判定 |

use crate::checks::CheckSet;
use crate::perfstar::perfkit::DiagSink;
use crate::perfstar::perfkit::DiagSev;

// ---------------------------------------------------------------------------
// 常量（主册【用户故事】【设计细节】）
// ---------------------------------------------------------------------------

/// 8 秒开机线（毫秒）。
pub const BOOT_BUDGET_MS: u32 = 8_000;
/// 固件段预算 4s（不可改）。
pub const SEG_FIRMWARE_MS: u32 = 4_000;
/// Limine 引导段预算 0.3s。
pub const SEG_BOOTLOADER_MS: u32 = 300;
/// 内核四链并行段预算 2.1s。
pub const SEG_KERNEL_MS: u32 = 2_100;
/// 动画段预算 1s。
pub const SEG_ANIM_MS: u32 = 1_000;
/// 偏差判据 10%（千分 100）。
pub const DEVIATION_REDLINE_PERMILLE: u32 = 100;
/// 并行加速判据：内核段 ≤ 串行版 60%（千分 600）。
pub const PARALLEL_REDLINE_PERMILLE: u32 = 600;
/// 四链。
pub const CHAINS: usize = 4;
/// 每链阶段数上限。
pub const STAGES: usize = 8;

/// 四链（主册【功能定义】ACPI/PCI 枚举/USB/存储）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chain {
    Acpi = 0,
    Pci = 1,
    Usb = 2,
    Storage = 3,
}

impl Chain {
    pub const fn name(self) -> &'static str {
        match self {
            Chain::Acpi => "ACPI",
            Chain::Pci => "PCI 枚举",
            Chain::Usb => "USB",
            Chain::Storage => "存储",
        }
    }
    /// 超时阈值毫秒（主册：USB 3s 宽限 / 存储 1s 严格）。
    pub const fn timeout_ms(self) -> u32 {
        match self {
            Chain::Acpi => 2_000,
            Chain::Pci => 2_000,
            Chain::Usb => 3_000,
            Chain::Storage => 1_000,
        }
    }
    /// 是否严格超时（存储 1s 严格；USB 3s 宽限）。
    pub const fn strict(self) -> bool {
        matches!(self, Chain::Storage)
    }
}

// ---------------------------------------------------------------------------
// 1. 依赖矩阵（文档化 + 虚假依赖可标）
// ---------------------------------------------------------------------------

/// 依赖关系类型（主册「多数『依赖』是历史串行的惯性」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepKind {
    /// 真依赖（数据/资源依赖，去掉会出错）。
    Real,
    /// 惯性串行（历史如此，实际可并行）。
    Inertial,
}

/// 依赖矩阵（4×4，定长）。
pub struct DepMatrix {
    /// `kind[a][b]` = None 表示无依赖；Some(k) 表示 a 依赖 b。
    kind: [[Option<DepKind>; CHAINS]; CHAINS],
    /// 已声明的依赖条目数。
    pub declared: usize,
}

impl DepMatrix {
    pub const fn new() -> Self {
        DepMatrix { kind: [[None; CHAINS]; CHAINS], declared: 0 }
    }
    /// 声明一条依赖（a 依赖 b）。
    pub fn declare(&mut self, a: Chain, b: Chain, kind: DepKind) {
        if self.kind[a as usize][b as usize].is_none() {
            self.declared += 1;
        }
        self.kind[a as usize][b as usize] = Some(kind);
    }
    /// 查询依赖（无依赖返回 None）。
    pub fn dep(&self, a: Chain, b: Chain) -> Option<DepKind> {
        self.kind[a as usize][b as usize]
    }
    /// a 是否可以与 b 并行（无真依赖即可并行——惯性依赖不阻挡）。
    pub fn can_parallel(&self, a: Chain, b: Chain) -> bool {
        !matches!(self.dep(a, b), Some(DepKind::Real)) && !matches!(self.dep(b, a), Some(DepKind::Real))
    }
    /// 惯性依赖条数（可消除的串行——优化空间的可视化）。
    pub fn inertial_count(&self) -> usize {
        let mut n = 0;
        for a in 0..CHAINS {
            for b in 0..CHAINS {
                if matches!(self.kind[a][b], Some(DepKind::Inertial)) {
                    n += 1;
                }
            }
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 2. 超时策略（逐链独立）
// ---------------------------------------------------------------------------

/// 超时裁定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeoutVerdict {
    /// 未超时。
    Ok,
    /// 超时：按链的严格/宽限语义处置。
    TimedOut {
        /// 超出多少毫秒。
        over_ms: u32,
        /// 严格链：判失败；宽限链：仅告警继续。
        fatal: bool,
    },
}

/// 逐链超时策略（主册「每链超时阈值独立」）。
pub struct TimeoutPolicy;

impl TimeoutPolicy {
    /// 裁定某链是否超时（严格链超了判失败，宽限链仅告警）。
    pub fn judge(c: Chain, elapsed_ms: u32) -> TimeoutVerdict {
        let limit = c.timeout_ms();
        if elapsed_ms <= limit {
            return TimeoutVerdict::Ok;
        }
        TimeoutVerdict::TimedOut { over_ms: elapsed_ms - limit, fatal: c.strict() }
    }
    /// 各链阈值表（诊断面展示用——不各自抄数字）。
    pub const fn table() -> [(Chain, u32, bool); CHAINS] {
        [
            (Chain::Acpi, 2_000, false),
            (Chain::Pci, 2_000, false),
            (Chain::Usb, 3_000, false),
            (Chain::Storage, 1_000, true),
        ]
    }
}

// ---------------------------------------------------------------------------
// 3. 失败传播（依赖链的下游跳过并标注）
// ---------------------------------------------------------------------------

/// 阶段状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageState {
    /// 正常执行。
    Ran,
    /// 因上游失败而跳过（必须带原因——不是「默默没跑」）。
    Skipped {
        /// 导致跳过的上游链。
        due_to: Chain,
    },
    /// 执行失败。
    Failed,
}

impl StageState {
    pub fn text(&self) -> &'static str {
        match self {
            StageState::Ran => "已执行",
            StageState::Skipped { .. } => "已跳过（上游失败）",
            StageState::Failed => "执行失败",
        }
    }
}

/// 失败传播记录（主册「后续依赖该链的阶段跳过并标注」）。
pub struct FailurePropagation {
    /// 各链是否失败。
    failed: [bool; CHAINS],
    /// 各链被跳过的下游阶段数。
    pub skipped_stages: [u32; CHAINS],
    /// 是否仍可进安全模式（存储链失败仍可进 F193）。
    pub safe_mode_ok: bool,
}

impl FailurePropagation {
    pub const fn new() -> Self {
        FailurePropagation { failed: [false; CHAINS], skipped_stages: [0; CHAINS], safe_mode_ok: true }
    }
    /// 标记某链失败，并沿依赖矩阵传播跳过。
    pub fn mark_failed(&mut self, c: Chain, deps: &DepMatrix, mut sink: Option<&mut DiagSink>, now_ms: u64) {
        self.failed[c as usize] = true;
        for a in 0..CHAINS {
            if a == c as usize {
                continue;
            }
            let all: [Chain; CHAINS] = [Chain::Acpi, Chain::Pci, Chain::Usb, Chain::Storage];
            if matches!(deps.dep(all[a], c), Some(DepKind::Real)) {
                self.skipped_stages[a] += 1;
                if let Some(s) = sink.as_deref_mut() {
                    s.push("F053", 1, now_ms, DiagSev::Warn, a as u64, c as u64, b"stage skipped: upstream failed");
                }
            }
        }
        // 存储链失败仍可进安全模式（主册原文）；其他链失败不影响该结论
        if c == Chain::Storage {
            self.safe_mode_ok = true;
        }
    }
    /// 某链是否失败。
    pub fn is_failed(&self, c: Chain) -> bool {
        self.failed[c as usize]
    }
    /// 某链某阶段的状态（依赖上游是否失败）。
    pub fn stage_state(&self, c: Chain, deps: &DepMatrix) -> StageState {
        if self.failed[c as usize] {
            return StageState::Failed;
        }
        let all: [Chain; CHAINS] = [Chain::Acpi, Chain::Pci, Chain::Usb, Chain::Storage];
        for b in 0..CHAINS {
            if self.failed[b] && matches!(deps.dep(c, all[b]), Some(DepKind::Real)) {
                return StageState::Skipped { due_to: all[b] };
            }
        }
        StageState::Ran
    }
}

// ---------------------------------------------------------------------------
// 4. 锁等待归因（并行竞争）
// ---------------------------------------------------------------------------

/// 一次锁等待记录（主册「锁等待归因入时间线」）。
#[derive(Clone, Copy, Debug)]
pub struct LockWait {
    /// 等待方链。
    pub waiter: Chain,
    /// 持锁方链。
    pub holder: Chain,
    /// 等待时长（毫秒）。
    pub ms: u32,
}

/// 锁等待账（定长 16 条，覆盖最旧）。
pub struct LockWaitLog {
    ring: [Option<LockWait>; 16],
    head: usize,
    filled: usize,
    /// 累计等待毫秒（各链）。
    pub total_ms: [u32; CHAINS],
    /// 记录总数（含被覆盖）。
    pub total: u64,
}

impl LockWaitLog {
    pub const fn new() -> Self {
        LockWaitLog { ring: [None; 16], head: 0, filled: 0, total_ms: [0; CHAINS], total: 0 }
    }
    pub fn note(&mut self, w: LockWait) {
        self.ring[self.head] = Some(w);
        self.head = (self.head + 1) % 16;
        self.filled = (self.filled + 1).min(16);
        self.total_ms[w.waiter as usize] = self.total_ms[w.waiter as usize].saturating_add(w.ms);
        self.total += 1;
    }
    pub fn snapshot(&self, out: &mut [LockWait]) -> usize {
        let n = self.filled.min(out.len());
        let start = (self.head + 16 - n) % 16;
        for i in 0..n {
            if let Some(w) = self.ring[(start + i) % 16] {
                out[i] = w;
            }
        }
        n
    }
    /// 某链因锁等待损失的时间（调参依据：哪条链被卡得最狠）。
    pub fn lost_ms(&self, c: Chain) -> u32 {
        self.total_ms[c as usize]
    }
    /// 是否存在锁等待（无竞争 = 依赖图设计合理）。
    pub fn any_contention(&self) -> bool {
        self.total > 0
    }
}

// ---------------------------------------------------------------------------
// 5. 动画起播点（内核段 80%）
// ---------------------------------------------------------------------------

/// 动画起播判定（主册「动画启动点提前到内核段 80% 处」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kickoff {
    /// 未到起播点。
    Pending,
    /// 到点起播。
    Fire,
    /// 已起播（不重复触发）。
    Done,
}

/// 动画起播器：按内核段进度触发，不按内核段结束（画面先动起来）。
#[derive(Clone, Copy, Debug)]
pub struct AnimKickoff {
    /// 起播进度千分（主册 80%）。
    pub at_permille: u32,
    fired: bool,
    /// 起播时刻（None = 尚未起播）。
    pub fired_at_ms: Option<u64>,
}

impl AnimKickoff {
    pub const fn new() -> Self {
        AnimKickoff { at_permille: 800, fired: false, fired_at_ms: None }
    }
    /// 内核段进度推进（`done_ms`/`total_ms`）。
    pub fn tick(&mut self, done_ms: u32, total_ms: u32, now_ms: u64) -> Kickoff {
        if self.fired {
            return Kickoff::Done;
        }
        if total_ms == 0 {
            return Kickoff::Pending;
        }
        let perm = ((done_ms as u64 * 1000) / total_ms as u64) as u32;
        if perm >= self.at_permille {
            self.fired = true;
            self.fired_at_ms = Some(now_ms);
            return Kickoff::Fire;
        }
        Kickoff::Pending
    }
    /// 感知时间收益：提前量 = 内核段剩余时长（主册「感知时间再减 0.5s」）。
    pub fn perceived_gain_ms(&self, done_ms: u32, total_ms: u32) -> u32 {
        if !self.fired {
            return 0;
        }
        total_ms.saturating_sub(done_ms)
    }
}

// ---------------------------------------------------------------------------
// 6. 甘特图与 8 秒预算
// ---------------------------------------------------------------------------

/// 甘特图一行（一链的一个阶段）。
#[derive(Clone, Copy, Debug)]
pub struct GanttRow {
    pub chain: Chain,
    /// 阶段序号。
    pub stage: u8,
    pub start_ms: u32,
    pub end_ms: u32,
    pub state: StageState,
}

/// 甘特图（定长 4×8）。
pub struct Gantt {
    rows: [Option<GanttRow>; CHAINS * STAGES],
    n: usize,
}

impl Gantt {
    pub const fn new() -> Self {
        Gantt { rows: [None; CHAINS * STAGES], n: 0 }
    }
    pub fn push(&mut self, r: GanttRow) -> bool {
        if self.n >= CHAINS * STAGES {
            return false;
        }
        self.rows[self.n] = Some(r);
        self.n += 1;
        true
    }
    pub fn rows(&self, out: &mut [GanttRow]) -> usize {
        let n = self.n.min(out.len());
        for i in 0..n {
            if let Some(r) = self.rows[i] {
                out[i] = r;
            }
        }
        n
    }
    /// 四链并行段**时长**（最早开始 → 最晚结束）。
    ///
    /// 并行段的长度是最长链，不是四链之和；取「最晚结束 − 最早开始」而非
    /// 绝对时刻——否则拿绝对时刻跟串行时长比，比值会虚假放大（此处吃过亏）。
    pub fn kernel_span_ms(&self) -> u32 {
        let mut latest = 0u32;
        let mut earliest = u32::MAX;
        for i in 0..self.n {
            if let Some(r) = self.rows[i] {
                if r.end_ms > latest {
                    latest = r.end_ms;
                }
                if r.start_ms < earliest {
                    earliest = r.start_ms;
                }
            }
        }
        if earliest == u32::MAX {
            return 0;
        }
        latest.saturating_sub(earliest)
    }
    /// 串行版耗时（各链耗时之和——并行度对拍基准）。
    pub fn serial_span_ms(&self) -> u32 {
        let mut sum = [0u32; CHAINS];
        for i in 0..self.n {
            if let Some(r) = self.rows[i] {
                let c = r.chain as usize;
                sum[c] += r.end_ms.saturating_sub(r.start_ms);
            }
        }
        sum.iter().copied().fold(0u32, u32::saturating_add)
    }
    pub fn len(&self) -> usize {
        self.n
    }
}

/// 8 秒预算分解（主册【用户故事】四段）。
pub struct Budget8s;

impl Budget8s {
    /// 四段预算（固件/Limine/内核/动画）。
    pub const fn segments() -> [(&'static str, u32); 4] {
        [("固件", SEG_FIRMWARE_MS), ("Limine", SEG_BOOTLOADER_MS), ("内核四链并行", SEG_KERNEL_MS), ("动画", SEG_ANIM_MS)]
    }
    /// 预算总和（应等于 8 秒线——差 0.6s 是主册口径内的余量，如实登记）。
    pub const fn total_ms() -> u32 {
        SEG_FIRMWARE_MS + SEG_BOOTLOADER_MS + SEG_KERNEL_MS + SEG_ANIM_MS
    }
    /// 实测与预算的偏差千分（单段）。
    pub fn deviation_permille(actual_ms: u32, budget_ms: u32) -> u32 {
        if budget_ms == 0 {
            return 0;
        }
        let d = actual_ms as i64 - budget_ms as i64;
        ((d.abs() * 1000) / budget_ms as i64) as u32
    }
    /// 单段是否达标（<10%）。
    pub fn segment_passes(actual_ms: u32, budget_ms: u32) -> bool {
        Self::deviation_permille(actual_ms, budget_ms) < DEVIATION_REDLINE_PERMILLE
    }
    /// 全段达标（逐段判定，不看总量——总量达标可能是几段互相抵消）。
    pub fn all_segments_pass(actual: &[u32; 4]) -> bool {
        let budgets = [SEG_FIRMWARE_MS, SEG_BOOTLOADER_MS, SEG_KERNEL_MS, SEG_ANIM_MS];
        (0..4).all(|i| Self::segment_passes(actual[i], budgets[i]))
    }
    /// 并行加速比（内核段并行 / 串行，应 ≤60%）。
    pub fn parallel_ratio_permille(parallel_ms: u32, serial_ms: u32) -> u32 {
        if serial_ms == 0 {
            return 1000;
        }
        ((parallel_ms as u64 * 1000) / serial_ms as u64) as u32
    }
    pub const fn parallel_redline_permille() -> u32 {
        PARALLEL_REDLINE_PERMILLE
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F053-bootpar-ext");
    // 1) 逐链超时阈值独立（USB 3s 宽限 / 存储 1s 严格）。
    cs.add(
        "per_chain_timeout",
        Chain::Usb.timeout_ms() == 3_000 && !Chain::Usb.strict() && Chain::Storage.timeout_ms() == 1_000 && Chain::Storage.strict(),
        "",
    );
    // 2) 超时裁定：宽限链仅告警，严格链判失败。
    cs.add(
        "timeout_verdict_fatal_only_for_strict",
        TimeoutPolicy::judge(Chain::Usb, 3_500) == TimeoutVerdict::TimedOut { over_ms: 500, fatal: false }
            && TimeoutPolicy::judge(Chain::Storage, 1_200) == TimeoutVerdict::TimedOut { over_ms: 200, fatal: true }
            && TimeoutPolicy::judge(Chain::Storage, 1_000) == TimeoutVerdict::Ok,
        "",
    );
    cs.add("timeout_table_complete", TimeoutPolicy::table().len() == 4, "");
    // 3) 依赖矩阵：真依赖阻挡并行，惯性依赖不阻挡。
    let mut dm = DepMatrix::new();
    dm.declare(Chain::Storage, Chain::Pci, DepKind::Real); // 存储真依赖 PCI
    dm.declare(Chain::Usb, Chain::Acpi, DepKind::Inertial); // USB 对 ACPI 是惯性
    cs.add(
        "dep_matrix_real_vs_inertial",
        dm.dep(Chain::Storage, Chain::Pci) == Some(DepKind::Real)
            && !dm.can_parallel(Chain::Storage, Chain::Pci)
            && dm.can_parallel(Chain::Usb, Chain::Pci)
            && dm.inertial_count() == 1
            && dm.declared == 2,
        "",
    );
    // 惯性依赖是可消除的串行（优化空间可视化）
    cs.add("inertial_not_blocking", dm.can_parallel(Chain::Usb, Chain::Acpi), "");
    // 4) 失败传播：真依赖下游跳过并标注；存储链失败仍可进安全模式。
    let mut sink = DiagSink::new();
    let mut fp = FailurePropagation::new();
    fp.mark_failed(Chain::Pci, &dm, Some(&mut sink), 1_000);
    cs.add(
        "failure_propagates_with_label",
        fp.is_failed(Chain::Pci)
            && fp.stage_state(Chain::Storage, &dm) == StageState::Skipped { due_to: Chain::Pci }
            && fp.stage_state(Chain::Storage, &dm).text() == "已跳过（上游失败）"
            && fp.skipped_stages[Chain::Storage as usize] == 1
            && sink.count(DiagSev::Warn) == 1,
        "",
    );
    // 惯性依赖的下游不跳过（不是真依赖）
    fp.mark_failed(Chain::Acpi, &dm, None, 2_000);
    cs.add("inertial_dep_not_skipped", fp.stage_state(Chain::Usb, &dm) == StageState::Ran, "");
    // 存储链失败仍可进安全模式（F193）
    let mut fp2 = FailurePropagation::new();
    fp2.mark_failed(Chain::Storage, &dm, None, 0);
    cs.add("storage_failure_still_safe_mode", fp2.safe_mode_ok && fp2.is_failed(Chain::Storage), "");
    // 5) 锁等待归因入时间线。
    let mut lw = LockWaitLog::new();
    lw.note(LockWait { waiter: Chain::Storage, holder: Chain::Pci, ms: 120 });
    lw.note(LockWait { waiter: Chain::Storage, holder: Chain::Pci, ms: 80 });
    cs.add(
        "lock_wait_attributed",
        lw.any_contention() && lw.lost_ms(Chain::Storage) == 200 && lw.lost_ms(Chain::Usb) == 0 && lw.total == 2,
        "",
    );
    let mut out = [LockWait { waiter: Chain::Acpi, holder: Chain::Acpi, ms: 0 }; 4];
    cs.add("lock_wait_snapshot", lw.snapshot(&mut out) == 2 && out[0].ms == 120, "");
    // 6) 动画起播点：内核段 80% 触发（不按段结束）。
    let mut ak = AnimKickoff::new();
    let k1 = ak.tick(1_000, 2_100, 5_000); // 47%
    let k2 = ak.tick(1_680, 2_100, 6_000); // 80% → 起播
    let k3 = ak.tick(2_100, 2_100, 7_000); // 已起播
    cs.add(
        "anim_kickoff_at_80pct",
        k1 == Kickoff::Pending && k2 == Kickoff::Fire && k3 == Kickoff::Done && ak.fired_at_ms == Some(6_000),
        "",
    );
    cs.add("anim_perceived_gain", ak.perceived_gain_ms(1_680, 2_100) == 420, "");
    // 未起播不算收益（不粉饰）
    cs.add("anim_gain_zero_before_fire", AnimKickoff::new().perceived_gain_ms(100, 2_100) == 0, "");
    // 7) 甘特图：并行段 = 最长链；串行基准 = 各链之和。
    let mut g = Gantt::new();
    g.push(GanttRow { chain: Chain::Acpi, stage: 0, start_ms: 4_300, end_ms: 5_000, state: StageState::Ran });
    g.push(GanttRow { chain: Chain::Pci, stage: 0, start_ms: 4_300, end_ms: 5_600, state: StageState::Ran });
    g.push(GanttRow { chain: Chain::Usb, stage: 0, start_ms: 4_300, end_ms: 5_400, state: StageState::Ran });
    g.push(GanttRow { chain: Chain::Storage, stage: 0, start_ms: 4_300, end_ms: 5_200, state: StageState::Ran });
    cs.add(
        "gantt_parallel_vs_serial",
        g.kernel_span_ms() == 1_300 && g.serial_span_ms() == 700 + 1_300 + 1_100 + 900,
        "",
    );
    // 并行加速比 1300/4000 = 325‰ ≤ 600‰（判据达成）
    cs.add(
        "parallel_ratio_meets_60pct",
        Budget8s::parallel_ratio_permille(g.kernel_span_ms(), g.serial_span_ms()) == 325
            && Budget8s::parallel_ratio_permille(g.kernel_span_ms(), g.serial_span_ms()) <= Budget8s::parallel_redline_permille(),
        "",
    );
    // 8) 8 秒预算分解与逐段偏差判定（逐段判定，不看总量）。
    cs.add(
        "budget8s_segments",
        Budget8s::segments().len() == 4 && Budget8s::total_ms() == 7_400 && SEG_FIRMWARE_MS == 4_000 && SEG_KERNEL_MS == 2_100,
        "",
    );
    cs.add(
        "budget8s_deviation",
        Budget8s::deviation_permille(2_200, 2_100) == 47
            && Budget8s::segment_passes(2_200, 2_100)
            && !Budget8s::segment_passes(2_400, 2_100)
            && Budget8s::deviation_permille(4_000, 4_000) == 0,
        "",
    );
    // 总量达标但单段超标 → 不达标（防互相抵消）
    cs.add(
        "budget8s_per_segment_not_total",
        !Budget8s::all_segments_pass(&[4_000, 300, 2_400, 700]) && Budget8s::all_segments_pass(&[4_000, 300, 2_100, 1_000]),
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inertial_dependency_is_the_optimization_space() {
        let mut d = DepMatrix::new();
        d.declare(Chain::Usb, Chain::Acpi, DepKind::Inertial);
        d.declare(Chain::Pci, Chain::Acpi, DepKind::Real);
        assert_eq!(d.inertial_count(), 1);
        // USB 可与 ACPI 并行（惯性依赖不阻挡），PCI 不行（真依赖）
        assert!(d.can_parallel(Chain::Usb, Chain::Acpi));
        assert!(!d.can_parallel(Chain::Pci, Chain::Acpi));
    }

    #[test]
    fn redeclaring_a_dependency_does_not_double_count() {
        let mut d = DepMatrix::new();
        d.declare(Chain::Usb, Chain::Acpi, DepKind::Inertial);
        d.declare(Chain::Usb, Chain::Acpi, DepKind::Real); // 改判为真依赖
        assert_eq!(d.declared, 1);
        assert_eq!(d.dep(Chain::Usb, Chain::Acpi), Some(DepKind::Real));
    }

    #[test]
    fn skipped_stage_always_names_its_cause() {
        let mut d = DepMatrix::new();
        d.declare(Chain::Storage, Chain::Pci, DepKind::Real);
        let mut f = FailurePropagation::new();
        f.mark_failed(Chain::Pci, &d, None, 0);
        match f.stage_state(Chain::Storage, &d) {
            StageState::Skipped { due_to } => assert_eq!(due_to, Chain::Pci),
            other => panic!("应为 Skipped，实为 {:?}", other),
        }
    }

    #[test]
    fn anim_kickoff_fires_only_once() {
        let mut a = AnimKickoff::new();
        assert_eq!(a.tick(2_000, 2_100, 1_000), Kickoff::Fire);
        assert_eq!(a.tick(2_100, 2_100, 2_000), Kickoff::Done);
        assert_eq!(a.fired_at_ms, Some(1_000), "起播时刻只记第一次");
    }

    #[test]
    fn gantt_span_uses_longest_chain_not_sum() {
        let mut g = Gantt::new();
        for c in [Chain::Acpi, Chain::Pci, Chain::Usb, Chain::Storage] {
            g.push(GanttRow { chain: c, stage: 0, start_ms: 0, end_ms: 1_000, state: StageState::Ran });
        }
        assert_eq!(g.kernel_span_ms(), 1_000, "并行段 = 最长链，不是四链之和");
        assert_eq!(g.serial_span_ms(), 4_000);
    }

    #[test]
    fn budget_deviation_is_absolute_not_signed() {
        // 提前完成也算偏差（提前太多说明预算估错了，不是好事）
        assert_eq!(Budget8s::deviation_permille(1_500, 2_100), 285);
        assert_eq!(Budget8s::deviation_permille(2_700, 2_100), 285);
    }
}
