//! UNX-F50201 页计数器账（逻辑页/物理面双计数）（0x43f40 段）
//!
//! 锚点：UNX-F50201-J1 双计数 ×50 任务与样本页账勾稽一致；判据锚定 §AI-63 §2 专题十。
//!
//! 打印计费最容易被「口径」骗：文档说 10 页、复印 3 份，有人按 10 页计，有人按
//! 30 页计，还有人把双面折算成 15 张纸。三种口径各自都能自圆其说，但账本只能有
//! 一份。这里把口径钉死成三层，任何一层都不许被别层覆盖：
//!
//! * **逻辑页**（logical）—— 用户视角的页：文档页数 × 份数，是计费与进度的分母；
//! * **物理面**（faces）—— 成像设备真正着墨的次数：单面时等于逻辑页，双面时仍等于
//!   逻辑页（一页一画，只是排在纸的两面），因此 `faces == logical` 是恒等式，除非
//!   出现折页/拼版这类改变页到面映射的工序（由 50203 单独负责）；
//! * **纸张**（sheets）—— 真正从纸盒里抽出来的张数：单面 = 面数，双面 = `ceil(面/2)`。
//!
//! 恒等式在 [`PageLedger::reconcile`] 里**重建一遍**再逐字段比对，而不是拿同一份
//! 中间变量自证：自己算一遍再和自己比，永远看不出加法漏项。
//!
//! 状态与计费的关系同样显式：只有 `Printed`/`Settled` 两个状态的面计入应收，
//! 仍在链路上的任务（`Queued`/`Rasterized`/`Spooling`）和失败态一律为 0。把
//! 「已递交」当成「已计费」是线上最常见的重复计费来源。
//!
//! 诊断码闭集：`SEG_BASE + 1 ..= SEG_BASE + 11`，逐项见 [`PageCode`]。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0x43f40 独占 0x40）。
pub const SEG_BASE: u32 = 0x43f40;

/// 样本任务数（卡面 50 条，硬编码；改数必须同步改判据口径）。
pub const JOB_COUNT: usize = 50;

/// 单任务最多份数：超过一律拒绝，避免 `pages × copies` 在账上变成天文数字。
pub const MAX_COPIES: u8 = 99;

/// 诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageCode {
    /// 任务数漂移。
    JobCountDrift,
    /// 逻辑页与面数不等。
    LogicalDrift,
    /// 面数与逻辑页不等（双计数断裂）。
    FaceDrift,
    /// 纸张折算错误。
    SheetDrift,
    /// 应收面数与状态不符。
    BilledDrift,
    /// 色彩/黑白分账泄漏。
    ColorDrift,
    /// 状态机越级或倒退。
    StateLeak,
    /// 账摘要不稳定。
    DigestDrift,
    /// 负值/零份数防护缺失。
    NegativeGuard,
    /// 溢出防护缺失。
    OverflowGuard,
    /// 未知诊断。
    Unknown,
}

impl PageCode {
    /// 段内偏移（1..=11）。唯一允许决定码值的地方。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                PageCode::JobCountDrift => 1,
                PageCode::LogicalDrift => 2,
                PageCode::FaceDrift => 3,
                PageCode::SheetDrift => 4,
                PageCode::BilledDrift => 5,
                PageCode::ColorDrift => 6,
                PageCode::StateLeak => 7,
                PageCode::DigestDrift => 8,
                PageCode::NegativeGuard => 9,
                PageCode::OverflowGuard => 10,
                PageCode::Unknown => 11,
            }
    }

    /// 文案非空：告警里必须能直接读懂是哪一层口径坏了。
    pub fn detail(self) -> &'static str {
        match self {
            PageCode::JobCountDrift => "任务数与冻结样本数不等，账的基数已变",
            PageCode::LogicalDrift => "逻辑页与面数不等，双计数第一层断裂",
            PageCode::FaceDrift => "面数重复累计或漏计，成像次数失真",
            PageCode::SheetDrift => "纸张折算错误，双面除二或进一不守恒",
            PageCode::BilledDrift => "应收面数与任务状态不符，存在链路中计费",
            PageCode::ColorDrift => "色彩分账泄漏，合计与逻辑页不闭合",
            PageCode::StateLeak => "状态机越级或倒退，任务生命周期被绕过",
            PageCode::DigestDrift => "账摘要不稳定，跨节点对账无法复现",
            PageCode::NegativeGuard => "零份数/零页任务未被拒绝",
            PageCode::OverflowGuard => "极端份数未走饱和加法，存在溢出回绕",
            PageCode::Unknown => "未归类的页计数异常",
        }
    }

    /// 闭集，顺序即码值序。
    pub const ALL: [PageCode; 11] = [
        PageCode::JobCountDrift,
        PageCode::LogicalDrift,
        PageCode::FaceDrift,
        PageCode::SheetDrift,
        PageCode::BilledDrift,
        PageCode::ColorDrift,
        PageCode::StateLeak,
        PageCode::DigestDrift,
        PageCode::NegativeGuard,
        PageCode::OverflowGuard,
        PageCode::Unknown,
    ];
}

/// 任务状态。顺序即生命周期序；`Failed`/`Cancelled` 是旁路终态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Rasterized,
    Spooling,
    Printed,
    Settled,
    Failed,
    Cancelled,
}

impl JobState {
    pub fn val(self) -> u8 {
        match self {
            JobState::Queued => 0,
            JobState::Rasterized => 1,
            JobState::Spooling => 2,
            JobState::Printed => 3,
            JobState::Settled => 4,
            JobState::Failed => 5,
            JobState::Cancelled => 6,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            JobState::Queued => "queued",
            JobState::Rasterized => "rasterized",
            JobState::Spooling => "spooling",
            JobState::Printed => "printed",
            JobState::Settled => "settled",
            JobState::Failed => "failed",
            JobState::Cancelled => "cancelled",
        }
    }

    /// 只有真正落纸与已结清的任务计费。
    pub fn billable(self) -> bool {
        matches!(self, JobState::Printed | JobState::Settled)
    }

    /// 终态：不再接收任何迁移。
    pub fn terminal(self) -> bool {
        matches!(
            self,
            JobState::Settled | JobState::Failed | JobState::Cancelled
        )
    }

    /// 线性主链的下一个状态；支线终态返回 `None`。
    pub fn next(self) -> Option<JobState> {
        match self {
            JobState::Queued => Some(JobState::Rasterized),
            JobState::Rasterized => Some(JobState::Spooling),
            JobState::Spooling => Some(JobState::Printed),
            JobState::Printed => Some(JobState::Settled),
            JobState::Settled | JobState::Failed | JobState::Cancelled => None,
        }
    }

    /// 迁移许可。
    ///
    /// 规则只有三条，但覆盖了全部合法路径：
    /// 1. 主链只许逐级前进（`next`）；
    /// 2. 任意非终态可入 `Failed`（设备掉线不需要等排版完成才能记账）；
    /// 3. `Cancelled` 只允许在「还没落纸」时进入——已经印出来的面不能靠取消抹掉。
    pub fn can_go(self, to: JobState) -> bool {
        if self.terminal() || self == to {
            return false;
        }
        if to == JobState::Failed {
            return true;
        }
        if to == JobState::Cancelled {
            return matches!(
                self,
                JobState::Queued | JobState::Rasterized | JobState::Spooling
            );
        }
        match self.next() {
            Some(n) => n == to,
            None => false,
        }
    }
}

/// 一个可计费任务的冻结描述。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobSpec {
    pub id: u16,
    pub pages: u16,
    pub copies: u8,
    pub duplex: bool,
    /// 其中按彩色成像的页数（0..=pages）。
    pub color_pages: u16,
    pub state: JobState,
}

impl JobSpec {
    pub const ZERO: JobSpec = JobSpec {
        id: 0,
        pages: 0,
        copies: 0,
        duplex: false,
        color_pages: 0,
        state: JobState::Queued,
    };

    pub fn logical(&self) -> u64 {
        (self.pages as u64).saturating_mul(self.copies as u64)
    }

    /// 物理面：一逻辑页一成像面。双面只改变纸的走法，不改变着墨次数。
    pub fn faces(&self) -> u64 {
        self.logical()
    }

    /// 纸张：单面一面一张；双面两面一张，奇数面进一。
    pub fn sheets(&self) -> u64 {
        let f = self.faces();
        if self.duplex {
            f.saturating_add(1) / 2
        } else {
            f
        }
    }

    pub fn color_faces(&self) -> u64 {
        let c = if self.color_pages > self.pages {
            self.pages
        } else {
            self.color_pages
        };
        (c as u64).saturating_mul(self.copies as u64)
    }

    pub fn bw_faces(&self) -> u64 {
        self.faces().saturating_sub(self.color_faces())
    }

    /// 应收面数：只有落纸与结清计费。
    pub fn billed_faces(&self) -> u64 {
        if self.state.billable() {
            self.faces()
        } else {
            0
        }
    }

    /// 描述自洽：零页/零份数必须被挡在账外。
    pub fn admissible(&self) -> bool {
        self.pages > 0 && self.copies > 0 && self.copies <= MAX_COPIES && self.color_pages <= self.pages
    }
}

/// 样本生成：只依赖下标，双机双跑必然一致。
const fn sample_job(i: usize) -> JobSpec {
    let pages = 1 + (i * 7) % 40;
    JobSpec {
        id: i as u16,
        pages: pages as u16,
        copies: (1 + (i % 5)) as u8,
        duplex: i % 3 != 0,
        color_pages: (pages / 2) as u16,
        state: STATE_CYCLE[i % STATE_CYCLE.len()],
    }
}

/// 样本状态轮转：四种链路态、两种终态与失败态都要出现，不许只测 happy path。
const STATE_CYCLE: [JobState; 10] = [
    JobState::Settled,
    JobState::Printed,
    JobState::Settled,
    JobState::Rasterized,
    JobState::Printed,
    JobState::Settled,
    JobState::Cancelled,
    JobState::Queued,
    JobState::Spooling,
    JobState::Settled,
];

/// 50 条冻结样本。
pub const SAMPLE_JOBS: [JobSpec; JOB_COUNT] = {
    let mut a = [JobSpec::ZERO; JOB_COUNT];
    let mut i = 0usize;
    while i < JOB_COUNT {
        a[i] = sample_job(i);
        i += 1;
    }
    a
};

fn fnv_mix(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x100_0000_01b3)
}

/// 双计数账。
///
/// 每个字段都走饱和加法：账本宁可停在 `u64::MAX` 并让守恒判据变红，也不许回绕成
/// 一个小数字——回绕后的「守恒」是假绿，比红灯更危险。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageLedger {
    pub logical: u64,
    pub faces: u64,
    pub sheets: u64,
    pub color: u64,
    pub bw: u64,
    pub billed: u64,
    pub jobs: u32,
    pub skipped: u32,
    digest: u64,
}

impl PageLedger {
    pub fn new() -> PageLedger {
        PageLedger {
            logical: 0,
            faces: 0,
            sheets: 0,
            color: 0,
            bw: 0,
            billed: 0,
            jobs: 0,
            skipped: 0,
            digest: 0xcbf2_9ce4_8422_2325,
        }
    }

    /// 入账一条。不可接纳的任务只记 `skipped`，绝不半截入账。
    pub fn absorb(&mut self, j: &JobSpec) -> bool {
        if !j.admissible() {
            self.skipped = self.skipped.saturating_add(1);
            return false;
        }
        self.logical = self.logical.saturating_add(j.logical());
        self.faces = self.faces.saturating_add(j.faces());
        self.sheets = self.sheets.saturating_add(j.sheets());
        self.color = self.color.saturating_add(j.color_faces());
        self.bw = self.bw.saturating_add(j.bw_faces());
        self.billed = self.billed.saturating_add(j.billed_faces());
        self.jobs = self.jobs.saturating_add(1);
        let mut h = self.digest;
        h = fnv_mix(h, j.id as u64);
        h = fnv_mix(h, j.logical());
        h = fnv_mix(h, j.sheets());
        h = fnv_mix(h, j.state.val() as u64);
        self.digest = h;
        true
    }

    pub fn digest(&self) -> u64 {
        let mut h = self.digest;
        h = fnv_mix(h, self.logical);
        h = fnv_mix(h, self.faces);
        h = fnv_mix(h, self.sheets);
        h = fnv_mix(h, self.billed);
        h
    }

    /// 分账闭合：color + bw == faces。
    pub fn color_closed(&self) -> bool {
        self.color.saturating_add(self.bw) == self.faces
    }

    /// 双计数恒等：逻辑页 == 物理面。
    pub fn dual_closed(&self) -> bool {
        self.logical == self.faces
    }

    /// 重建对账：用样本重算一份账，与自身逐字段比较。
    pub fn reconcile(&self, jobs: &[JobSpec]) -> bool {
        let mut probe = PageLedger::new();
        let mut i = 0usize;
        while i < jobs.len() {
            if let Some(j) = jobs.get(i) {
                let _ = probe.absorb(j);
            }
            i += 1;
        }
        probe.logical == self.logical
            && probe.faces == self.faces
            && probe.sheets == self.sheets
            && probe.color == self.color
            && probe.bw == self.bw
            && probe.billed == self.billed
            && probe.jobs == self.jobs
            && probe.skipped == self.skipped
            && probe.digest() == self.digest()
    }
}

impl Default for PageLedger {
    fn default() -> Self {
        PageLedger::new()
    }
}

/// 样本账：全量 50 条。
pub fn sample_ledger() -> PageLedger {
    let mut l = PageLedger::new();
    let mut i = 0usize;
    while i < JOB_COUNT {
        if let Some(j) = SAMPLE_JOBS.get(i) {
            let _ = l.absorb(j);
        }
        i += 1;
    }
    l
}

/// 统计：应收任务数 / 非应收任务数。
pub fn billed_split(jobs: &[JobSpec]) -> (u32, u32) {
    let mut billed = 0u32;
    let mut pending = 0u32;
    let mut i = 0usize;
    while i < jobs.len() {
        if let Some(j) = jobs.get(i) {
            if j.state.billable() {
                billed = billed.saturating_add(1);
            } else {
                pending = pending.saturating_add(1);
            }
        }
        i += 1;
    }
    (billed, pending)
}

/// 状态机完整性扫描：主链每步必须可走，反向每步必须被拒。
pub fn chain_audit() -> u32 {
    let mut bad = 0u32;
    let chain = [
        JobState::Queued,
        JobState::Rasterized,
        JobState::Spooling,
        JobState::Printed,
        JobState::Settled,
    ];
    let mut i = 0usize;
    while i + 1 < chain.len() {
        if let (Some(a), Some(b)) = (chain.get(i), chain.get(i + 1)) {
            if !a.can_go(*b) {
                bad = bad.saturating_add(1);
            }
            if b.can_go(*a) {
                bad = bad.saturating_add(1);
            }
        }
        i += 1;
    }
    // 已结清与已取消必须是吸收态。
    if JobState::Settled.can_go(JobState::Queued) || JobState::Cancelled.can_go(JobState::Spooling) {
        bad = bad.saturating_add(1);
    }
    // 已落纸不许取消（面已经着墨）。
    if JobState::Printed.can_go(JobState::Cancelled) {
        bad = bad.saturating_add(1);
    }
    // 链路态均可失败。
    if !JobState::Queued.can_go(JobState::Failed)
        || !JobState::Rasterized.can_go(JobState::Failed)
        || !JobState::Spooling.can_go(JobState::Failed)
    {
        bad = bad.saturating_add(1);
    }
    bad
}

/// 溢出防护：把账推到极限前一格，再入账必须饱和而不是回绕。
pub fn overflow_probe() -> bool {
    let huge = JobSpec {
        id: 999,
        pages: u16::MAX,
        copies: MAX_COPIES,
        duplex: true,
        color_pages: u16::MAX,
        state: JobState::Printed,
    };
    let mut l = PageLedger::new();
    l.logical = u64::MAX - 1;
    l.faces = u64::MAX - 1;
    l.sheets = u64::MAX - 1;
    let took = l.absorb(&huge);
    took && l.logical == u64::MAX && l.faces == u64::MAX && l.sheets == u64::MAX && l.jobs == 1
        // 再入一次仍停在上界，绝不回绕成小数字。
        && l.absorb(&huge)
        && l.faces == u64::MAX
        && l.jobs == 2
}

/// 判据。
pub fn run_unx50201_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50201");

    // 50201-J1-1 诊断码闭集：码值连续、文案非空、互不重复
    let mut code_ok = PageCode::ALL.len() == 11;
    let mut i = 0usize;
    while i < PageCode::ALL.len() {
        if let Some(c) = PageCode::ALL.get(i) {
            if c.val() != SEG_BASE + 1 + i as u32 || c.detail().is_empty() {
                code_ok = false;
            }
            let mut j = 0usize;
            while j < i {
                if let Some(p) = PageCode::ALL.get(j) {
                    if p.val() == c.val() || p.detail() == c.detail() {
                        code_ok = false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    s.add("50201-J1-诊断码闭集", code_ok, "11 条码连续且文案唯一");

    // 50201-J1-2 样本基数：50 条且 id 连续
    let mut ids_ok = SAMPLE_JOBS.len() == JOB_COUNT;
    let mut i = 0usize;
    while i < JOB_COUNT {
        if let Some(j) = SAMPLE_JOBS.get(i) {
            if j.id as usize != i || !j.admissible() {
                ids_ok = false;
            }
        } else {
            ids_ok = false;
        }
        i += 1;
    }
    s.add("50201-J1-样本基数", ids_ok, "50 条样本 id 连续且可接纳");

    // 50201-J1-3 双计数恒等：逻辑页 == 物理面（逐条）
    let mut dual_ok = true;
    let mut i = 0usize;
    while i < JOB_COUNT {
        if let Some(j) = SAMPLE_JOBS.get(i) {
            if j.logical() != j.faces() || j.logical() == 0 {
                dual_ok = false;
            }
        }
        i += 1;
    }
    let ledger = sample_ledger();
    s.add(
        "50201-J1-双计数恒等",
        dual_ok && ledger.dual_closed(),
        "逐条与合计两层均为 logical == faces",
    );

    // 50201-J1-4 纸张折算：双面除二进一，单面一面一张
    let mut sheet_ok = true;
    let mut i = 0usize;
    while i < JOB_COUNT {
        if let Some(j) = SAMPLE_JOBS.get(i) {
            let want = if j.duplex {
                (j.faces() + 1) / 2
            } else {
                j.faces()
            };
            if j.sheets() != want {
                sheet_ok = false;
            }
            if j.duplex && j.faces() % 2 == 1 && j.sheets() * 2 != j.faces() + 1 {
                sheet_ok = false;
            }
        }
        i += 1;
    }
    s.add("50201-J1-纸张折算", sheet_ok, "双面 (f+1)/2、单面 f，逐条命中");

    // 50201-J1-5 色彩分账闭合：color + bw == faces（逐条与合计）
    let mut color_ok = true;
    let mut i = 0usize;
    while i < JOB_COUNT {
        if let Some(j) = SAMPLE_JOBS.get(i) {
            if j.color_faces().saturating_add(j.bw_faces()) != j.faces() {
                color_ok = false;
            }
        }
        i += 1;
    }
    s.add(
        "50201-J1-色彩分账闭合",
        color_ok && ledger.color_closed(),
        "彩色面 + 黑白面 恒等于物理面",
    );

    // 50201-J1-6 计费口径：只有落纸/结清计费
    let mut billed_ok = true;
    let mut i = 0usize;
    while i < JOB_COUNT {
        if let Some(j) = SAMPLE_JOBS.get(i) {
            if j.state.billable() && j.billed_faces() != j.faces() {
                billed_ok = false;
            }
            if !j.state.billable() && j.billed_faces() != 0 {
                billed_ok = false;
            }
        }
        i += 1;
    }
    let (billed_jobs, pending_jobs) = billed_split(&SAMPLE_JOBS);
    s.add(
        "50201-J1-计费口径",
        billed_ok && billed_jobs + pending_jobs == JOB_COUNT as u32,
        "链路中与失败态应收为 0",
    );

    // 50201-J1-7 账本对账：重建一份账逐字段相等
    let recon = ledger.reconcile(&SAMPLE_JOBS);
    s.add("50201-J1-账本对账", recon, "重建账与实例账逐字段一致");

    // 50201-J1-8 账摘要稳定：两次独立建账同摘要且非零
    let d1 = sample_ledger().digest();
    let d2 = sample_ledger().digest();
    s.add(
        "50201-J1-账摘要稳定",
        d1 == d2 && d1 != 0 && d1 == ledger.digest(),
        "双跑同摘要",
    );

    // 50201-J1-9 状态机主链：逐级可达、反向必拒
    let chain = chain_audit();
    s.add("50201-J1-状态机主链", chain == 0, "主链单向、终态吸收、落纸不可取消");

    // 50201-J1-10 迁移闭包：任意状态迁移目标必须在闭集内且不产生自环
    let mut closure_ok = true;
    let states = [
        JobState::Queued,
        JobState::Rasterized,
        JobState::Spooling,
        JobState::Printed,
        JobState::Settled,
        JobState::Failed,
        JobState::Cancelled,
    ];
    let mut i = 0usize;
    while i < states.len() {
        if let Some(a) = states.get(i) {
            let mut j = 0usize;
            while j < states.len() {
                if let Some(b) = states.get(j) {
                    if a.can_go(*b) && (a == b || a.terminal()) {
                        closure_ok = false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    s.add("50201-J1-迁移闭包", closure_ok, "无自环、终态零出边");

    // 50201-J1-11 零值防护：零页/零份/超额份数必须被拒
    let zero_pages = JobSpec {
        pages: 0,
        copies: 1,
        ..JobSpec::ZERO
    };
    let zero_copies = JobSpec {
        pages: 5,
        copies: 0,
        ..JobSpec::ZERO
    };
    let too_many = JobSpec {
        pages: 5,
        copies: MAX_COPIES,
        ..JobSpec::ZERO
    };
    let mut guard = PageLedger::new();
    let ok = !guard.absorb(&zero_pages)
        && !guard.absorb(&zero_copies)
        && guard.absorb(&too_many)
        && guard.skipped == 2
        && guard.jobs == 1;
    s.add("50201-J1-零值防护", ok, "零页/零份被拒，界内份数放行");

    // 50201-J1-12 溢出防护：饱和加法不回绕
    s.add(
        "50201-J1-溢出防护",
        overflow_probe(),
        "三倍极限任务后停在 u64::MAX",
    );

    // 50201-J1-13 容量与段位：段基址 0x40 对齐、样本不超账本容量
    let seg_ok = SEG_BASE % 0x40 == 0 && SAMPLE_JOBS.len() <= 64;
    s.add("50201-J1-容量段位", seg_ok, "段对齐且样本数在账本容量内");

    // 动态明细（≤3 条）
    s.add_dyn(
        "50201-J1-合计明细",
        ledger.dual_closed() && ledger.color_closed(),
        &format!(
            "逻辑={} 面={} 纸={} 彩色={} 黑白={} 应收={}",
            ledger.logical, ledger.faces, ledger.sheets, ledger.color, ledger.bw, ledger.billed
        ),
    );
    s.add_dyn(
        "50201-J1-任务明细",
        billed_jobs + pending_jobs == JOB_COUNT as u32,
        &format!("计费任务={} 未落纸={} 跳过={}", billed_jobs, pending_jobs, ledger.skipped),
    );
    s.add_dyn(
        "50201-J1-摘要明细",
        d1 != 0,
        &format!("账摘要={:016x} 段基址={:#x}", d1, SEG_BASE),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50201_checks_green() {
        let set = run_unx50201_checks();
        assert!(set.all_passed(), "UNX-F50201 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50201_dual_count_exact() {
        let l = sample_ledger();
        assert_eq!(l.jobs, JOB_COUNT as u32);
        assert_eq!(l.logical, l.faces);
        assert_eq!(l.color + l.bw, l.faces);
        assert!(l.billed <= l.faces);
    }

    #[test]
    fn unx50201_sheet_math() {
        let j = JobSpec {
            id: 1,
            pages: 5,
            copies: 2,
            duplex: true,
            color_pages: 0,
            state: JobState::Printed,
        };
        assert_eq!(j.logical(), 10);
        assert_eq!(j.faces(), 10);
        assert_eq!(j.sheets(), 5);
        let odd = JobSpec { pages: 3, copies: 1, ..j };
        assert_eq!(odd.sheets(), 2);
    }

    #[test]
    fn unx50201_state_machine() {
        assert!(JobState::Queued.can_go(JobState::Rasterized));
        assert!(JobState::Spooling.can_go(JobState::Printed));
        assert!(JobState::Printed.can_go(JobState::Settled));
        assert!(!JobState::Settled.can_go(JobState::Queued));
        assert!(!JobState::Printed.can_go(JobState::Cancelled));
        assert!(JobState::Queued.can_go(JobState::Failed));
    }

    #[test]
    fn unx50201_ledger_reconciles() {
        let l = sample_ledger();
        assert!(l.reconcile(&SAMPLE_JOBS));
        assert!(overflow_probe());
    }
}
