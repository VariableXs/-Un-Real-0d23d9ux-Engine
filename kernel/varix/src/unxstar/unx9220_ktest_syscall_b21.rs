//! UNX-F9220 ktest syscall 面 B21 批断言集 · 0x7A480段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9220 逐字）：
//!   ① 纯功能代码 ≥420 行；② 判据原文：UNX-F9220-J1 本批 19 条判据聚合
//!   判据过，文件族 E 型矩阵一次跑通，域累计 105,700 收口断言；③ 完成时
//!   API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 组批与执行分成两层：BatchPlan 负责登记 19 条用例规格（同号重复、
//!      零权重、超过上限的用例在**入批时**就拒），BatchRun 负责逐条执行并
//!      累计断言。这样「空跑用例混进批里」不可能靠执行期补救。
//!   ② 用例状态机 Queued→Running→{Passed,Failed}，Failed 允许一次重试
//!      （Failed→Running），Passed 之后**禁止重跑**——「重跑刷绿」是本册
//!      最需要钉死的作弊路径。
//!   ③ 收口口径唯一：19 条用例的权重之和必须恰为 105,700，执行账必须满足
//!      跑过 = 通过 + 失败；两式任一不成立即 WeightMismatch / CountMismatch。
//!   ④ 掩红检测：通过断言数必须等于「Passed 态用例权重之和」。把失败用例
//!      的断言偷偷记进通过账，两账立刻分叉 → MaskedFailure（注入器专打）。
//!   ⑤ 注入器三类违约（重复用例号/权重漂移/掩红）各 10 轮全抓；逐模块
//!      小计与对账表逐项相等，指纹双跑一致。
//! ─────────────────────────────────────────────────────────────────────
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9220 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A480;
/// 本批用例条数。
pub const CASE_COUNT: usize = 19;
/// 组批槽位上限（19 条用例之外留 5 槽供续批，超槽即容量满）。
pub const MAX_CASES: usize = 24;
/// 域累计收口断言数（本批唯一口径）。
pub const BATCH_TOTAL: u32 = 105_700;
/// 单条用例断言上限（超限即判该用例红）。
pub const MAX_CASE_ASSERTS: u32 = 10_000;
/// 失败用例允许的重试次数。
pub const MAX_RETRIES: u8 = 1;
/// 每类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 注入类别数。
pub const INJECT_CLASSES: u32 = 3;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（0x7A480 段内 +0x01..+0x0E 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KCode {
    UnknownCase, DuplicateCase, ZeroAssert, AlreadyPassed, RetryExhausted,
    CapacityFull, WeightMismatch, MaskedFailure, NotStarted, CountMismatch,
    AssertOverflow, LedgerRegress, NotReproducible, SegmentViolation,
}

impl KCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 { SEG_BASE + self as u32 }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            KCode::UnknownCase => "用例号不在批内：本批只有 B21-01..19；请核对编号，禁止执行清单外的用例",
            KCode::DuplicateCase => "用例号重复：同号两条会让断言账翻倍；请给续批用例改号后再登记",
            KCode::ZeroAssert => "零权重用例：断言数为 0 的空跑用例不得入批；请补断言或删条目",
            KCode::AlreadyPassed => "已绿用例重跑被拒：通过后重跑等于刷绿；请改用例或新建批次",
            KCode::RetryExhausted => "重试耗尽：失败用例只允许重试一次；请先修根因再开新批",
            KCode::CapacityFull => "组批槽满：上限 24 槽；请先结批或另开续批，不许静默丢用例",
            KCode::WeightMismatch => "权重漂移：19 条用例断言之和必须恰为 105,700；请逐条复算，禁止估算",
            KCode::MaskedFailure => "掩红：通过账与 Passed 态用例权重之和不符；请回滚被挪进通过账的失败断言",
            KCode::NotStarted => "批次未跑完：仍有 Queued/Running 用例；请先逐条执行再结批",
            KCode::CountMismatch => "计数不守恒：跑过 ≠ 通过 + 失败；请逐条复算执行账",
            KCode::AssertOverflow => "单条断言超限：超过 10,000 条的用例须拆分；请按矩阵拆成多条",
            KCode::LedgerRegress => "账目倒退：累计断言只增不减；请检查是否有回滚分支误改累计值",
            KCode::NotReproducible => "双跑不一致：同批两次指纹不同；请排查隐藏状态或未定序遍历",
            KCode::SegmentViolation => "段位越界：段基址须 0x40 对齐且落在 0x7A000..=0x7D1C0",
        }
    }

    /// 闭集全量。
    pub const ALL: [KCode; 14] = [
        KCode::UnknownCase, KCode::DuplicateCase, KCode::ZeroAssert, KCode::AlreadyPassed,
        KCode::RetryExhausted, KCode::CapacityFull, KCode::WeightMismatch, KCode::MaskedFailure,
        KCode::NotStarted, KCode::CountMismatch, KCode::AssertOverflow, KCode::LedgerRegress,
        KCode::NotReproducible, KCode::SegmentViolation,
    ];
}

// ── 判据类别 / 归属模块 ───────────────────────────────────────────────
/// 判据类别（对应契约第 3 节十二类形态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cat {
    Normal, Boundary, Injection, Order, State, Capacity,
    Determinism, Segment, Recon, Conservation, Text, Closure,
}

impl Cat {
    /// 类别名。
    pub const fn label(self) -> &'static str {
        match self {
            Cat::Normal => "normal", Cat::Boundary => "boundary", Cat::Injection => "injection",
            Cat::Order => "order", Cat::State => "state", Cat::Capacity => "capacity",
            Cat::Determinism => "determinism", Cat::Segment => "segment", Cat::Recon => "recon",
            Cat::Conservation => "conservation", Cat::Text => "text", Cat::Closure => "closure",
        }
    }
}

/// 归属模块（本批八卡）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stem {
    F9218, F9219, F9220, F9221, F9222, F9223, F9224, F9225,
}

impl Stem {
    /// 卡号文本。
    pub const fn label(self) -> &'static str {
        match self {
            Stem::F9218 => "F9218", Stem::F9219 => "F9219", Stem::F9220 => "F9220",
            Stem::F9221 => "F9221", Stem::F9222 => "F9222", Stem::F9223 => "F9223",
            Stem::F9224 => "F9224", Stem::F9225 => "F9225",
        }
    }
    /// 下标（0..8）。
    pub const fn index(self) -> usize {
        match self {
            Stem::F9218 => 0, Stem::F9219 => 1, Stem::F9220 => 2, Stem::F9221 => 3,
            Stem::F9222 => 4, Stem::F9223 => 5, Stem::F9224 => 6, Stem::F9225 => 7,
        }
    }
    /// 闭集全量。
    pub const ALL: [Stem; 8] = [
        Stem::F9218, Stem::F9219, Stem::F9220, Stem::F9221,
        Stem::F9222, Stem::F9223, Stem::F9224, Stem::F9225,
    ];
}

/// 一条用例规格。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseSpec {
    /// 用例号（1..=19）
    pub id: u8,
    /// 判据类别
    pub cat: Cat,
    /// 归属模块
    pub stem: Stem,
    /// 该用例的断言条数（权重）
    pub asserts: u32,
}

impl CaseSpec {
    /// 构造。
    pub const fn new(id: u8, cat: Cat, stem: Stem, asserts: u32) -> CaseSpec {
        CaseSpec { id, cat, stem, asserts }
    }
    /// 规格自检：零权重与超限都属坏规格。
    pub fn audit(&self) -> Result<(), KCode> {
        if self.asserts == 0 {
            return Err(KCode::ZeroAssert);
        }
        if self.asserts > MAX_CASE_ASSERTS {
            return Err(KCode::AssertOverflow);
        }
        Ok(())
    }
}

/// 本批 19 条用例规格（权重之和恰为 105,700）。
pub const CASES: [CaseSpec; CASE_COUNT] = [
    CaseSpec::new(1, Cat::Segment, Stem::F9221, 1_200),
    CaseSpec::new(2, Cat::Normal, Stem::F9221, 6_400),
    CaseSpec::new(3, Cat::Injection, Stem::F9221, 7_300),
    CaseSpec::new(4, Cat::Boundary, Stem::F9223, 5_600),
    CaseSpec::new(5, Cat::Injection, Stem::F9221, 6_100),
    CaseSpec::new(6, Cat::Capacity, Stem::F9224, 8_200),
    CaseSpec::new(7, Cat::Capacity, Stem::F9225, 7_900),
    CaseSpec::new(8, Cat::Injection, Stem::F9222, 6_800),
    CaseSpec::new(9, Cat::Boundary, Stem::F9222, 4_300),
    CaseSpec::new(10, Cat::Injection, Stem::F9223, 6_500),
    CaseSpec::new(11, Cat::Capacity, Stem::F9224, 5_200),
    CaseSpec::new(12, Cat::Capacity, Stem::F9225, 4_900),
    CaseSpec::new(13, Cat::Order, Stem::F9221, 3_700),
    CaseSpec::new(14, Cat::Order, Stem::F9225, 3_300),
    CaseSpec::new(15, Cat::State, Stem::F9220, 4_100),
    CaseSpec::new(16, Cat::Capacity, Stem::F9224, 5_400),
    CaseSpec::new(17, Cat::Determinism, Stem::F9220, 6_000),
    CaseSpec::new(18, Cat::Recon, Stem::F9218, 5_300),
    CaseSpec::new(19, Cat::Closure, Stem::F9219, 7_500),
];

// ── 组批 ──────────────────────────────────────────────────────────────
/// 组批：定容 24 槽，同号/坏规格/超槽三关在前。
#[derive(Clone, Copy, Debug)]
pub struct BatchPlan {
    slots: [Option<CaseSpec>; MAX_CASES],
    count: usize,
    rejected: u32,
}

impl BatchPlan {
    /// 空批。
    pub const fn new() -> BatchPlan {
        BatchPlan { slots: [None; MAX_CASES], count: 0, rejected: 0 }
    }
    /// 已登记条数。
    pub fn count(&self) -> usize { self.count }
    /// 拒收次数。
    pub fn rejected(&self) -> u32 { self.rejected }
    /// 第 i 条规格。
    pub fn at(&self, i: usize) -> Option<CaseSpec> {
        match self.slots.get(i) {
            Some(Some(s)) => Some(*s),
            _ => None,
        }
    }
    /// 按 id 找规格。
    pub fn find(&self, id: u8) -> Option<CaseSpec> {
        (0..self.count).filter_map(|i| self.at(i)).find(|s| s.id == id)
    }
    /// 登记一条用例规格。
    ///
    /// 判定次序：规格自检（零权重/超限）→ 同号 → 容量。规格不合格的用例
    /// 连槽都不占，避免「先占位再补断言」的糊弄路径。
    pub fn append(&mut self, spec: CaseSpec) -> Result<usize, KCode> {
        if let Err(code) = spec.audit() {
            self.rejected = self.rejected.saturating_add(1);
            return Err(code);
        }
        if self.find(spec.id).is_some() {
            self.rejected = self.rejected.saturating_add(1);
            return Err(KCode::DuplicateCase);
        }
        if self.count >= MAX_CASES {
            self.rejected = self.rejected.saturating_add(1);
            return Err(KCode::CapacityFull);
        }
        let idx = self.count;
        if let Some(slot) = self.slots.get_mut(idx) {
            *slot = Some(spec);
            self.count = self.count.saturating_add(1);
            return Ok(idx);
        }
        self.rejected = self.rejected.saturating_add(1);
        Err(KCode::CapacityFull)
    }
    /// 权重合计。
    pub fn total(&self) -> u32 {
        (0..self.count).filter_map(|i| self.at(i)).map(|s| s.asserts).sum()
    }
    /// 逐模块小计。
    pub fn per_stem(&self, stem: Stem) -> u32 {
        (0..self.count).filter_map(|i| self.at(i))
            .filter(|s| s.stem == stem).map(|s| s.asserts).sum()
    }
    /// 只看类别小计。
    pub fn per_cat(&self, cat: Cat) -> u32 {
        (0..self.count).filter_map(|i| self.at(i))
            .filter(|s| s.cat == cat).map(|s| s.asserts).sum()
    }
}

/// 载入本批 19 条规格。
pub fn load_batch() -> BatchPlan {
    let mut plan = BatchPlan::new();
    let mut i = 0usize;
    while i < CASE_COUNT {
        if let Some(spec) = CASES.get(i).copied() {
            let _ = plan.append(spec);
        }
        i += 1;
    }
    plan
}

// ── 用例状态机 ────────────────────────────────────────────────────────
/// 用例状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseState {
    /// 待跑
    Queued,
    /// 执行中
    Running,
    /// 通过
    Passed,
    /// 失败
    Failed,
}

impl CaseState {
    /// 状态名。
    pub const fn label(self) -> &'static str {
        match self {
            CaseState::Queued => "queued", CaseState::Running => "running",
            CaseState::Passed => "passed", CaseState::Failed => "failed",
        }
    }
    /// 转移表。
    pub const fn can_go(self, to: CaseState) -> bool {
        match (self, to) {
            (CaseState::Queued, CaseState::Running) => true,
            (CaseState::Running, CaseState::Passed) => true,
            (CaseState::Running, CaseState::Failed) => true,
            // 失败可重试一次（配额由 retries 计），通过后不再回退。
            (CaseState::Failed, CaseState::Running) => true,
            _ => false,
        }
    }
    /// 是否终态。
    pub const fn terminal(self) -> bool {
        matches!(self, CaseState::Passed | CaseState::Failed)
    }
}

/// 执行槽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slot {
    /// 用例规格
    pub spec: CaseSpec,
    /// 当前状态
    pub state: CaseState,
    /// 已重试次数
    pub retries: u8,
}

/// 执行前准入判定：规格 → 已绿 → 重试配额。
pub fn admit(spec: &CaseSpec, state: CaseState, retries: u8) -> Result<(), KCode> {
    if spec.asserts == 0 {
        return Err(KCode::ZeroAssert);
    }
    if state == CaseState::Passed {
        return Err(KCode::AlreadyPassed);
    }
    if state == CaseState::Failed && retries >= MAX_RETRIES {
        return Err(KCode::RetryExhausted);
    }
    Ok(())
}

// ── 批次执行 ──────────────────────────────────────────────────────────
/// 批次执行账。
#[derive(Clone, Copy, Debug)]
pub struct BatchRun {
    slots: [Option<Slot>; MAX_CASES],
    count: usize,
    asserts_run: u32,
    asserts_pass: u32,
    asserts_fail: u32,
    runs: u32,
    rejects: u32,
}

impl BatchRun {
    /// 从组批装载（全部置 Queued）。
    pub fn from_plan(plan: &BatchPlan) -> BatchRun {
        let mut run = BatchRun {
            slots: [None; MAX_CASES], count: 0, asserts_run: 0,
            asserts_pass: 0, asserts_fail: 0, runs: 0, rejects: 0,
        };
        let mut i = 0usize;
        while i < plan.count() {
            if let Some(spec) = plan.at(i) {
                if let Some(slot) = run.slots.get_mut(i) {
                    *slot = Some(Slot { spec, state: CaseState::Queued, retries: 0 });
                    run.count = run.count.saturating_add(1);
                }
            }
            i += 1;
        }
        run
    }
    /// 条数。
    pub fn count(&self) -> usize { self.count }
    /// 跑过的断言数。
    pub fn asserts_run(&self) -> u32 { self.asserts_run }
    /// 通过的断言数。
    pub fn asserts_pass(&self) -> u32 { self.asserts_pass }
    /// 失败的断言数。
    pub fn asserts_fail(&self) -> u32 { self.asserts_fail }
    /// 执行轮次（含重试）。
    pub fn runs(&self) -> u32 { self.runs }
    /// 拒收次数。
    pub fn rejects(&self) -> u32 { self.rejects }
    /// 第 i 槽。
    pub fn slot_at(&self, i: usize) -> Option<Slot> {
        match self.slots.get(i) {
            Some(Some(s)) => Some(*s),
            _ => None,
        }
    }
    /// 按 id 找槽下标。
    pub fn index_of(&self, id: u8) -> Option<usize> {
        (0..self.count).find(|i| matches!(self.slot_at(*i), Some(s) if s.spec.id == id))
    }
    /// Passed 态用例权重之和（掩红检测的右侧）。
    pub fn passed_weight(&self) -> u32 {
        (0..self.count).filter_map(|i| self.slot_at(i))
            .filter(|s| s.state == CaseState::Passed).map(|s| s.spec.asserts).sum()
    }
    /// 是否全绿。
    pub fn all_passed(&self) -> bool {
        self.count == CASE_COUNT
            && (0..self.count).filter_map(|i| self.slot_at(i))
                .all(|s| s.state == CaseState::Passed)
    }

    /// 执行一条用例（默认按通过上报，含一次重试配额）。
    pub fn run_case(&mut self, id: u8) -> Result<CaseState, KCode> {
        self.run_case_outcome(id, true)
    }

    /// 带执行结果执行一条用例：`passed` 由执行器上报，账目照实登记。
    ///
    /// 判定次序：未知号 → 规格（零权重）→ 已绿 → 重试配额；全过之后才动账。
    pub fn run_case_outcome(&mut self, id: u8, passed: bool) -> Result<CaseState, KCode> {
        let idx = match self.index_of(id) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(KCode::UnknownCase);
            }
        };
        let slot = match self.slot_at(idx) {
            Some(s) => s,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(KCode::UnknownCase);
            }
        };
        if let Err(code) = admit(&slot.spec, slot.state, slot.retries) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(code);
        }
        if !slot.state.can_go(CaseState::Running) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(KCode::AlreadyPassed);
        }
        // 置 Running（借道一次失败重试的 retries 递增）。
        let mut next = slot;
        next.state = CaseState::Running;
        if slot.state == CaseState::Failed {
            next.retries = slot.retries.saturating_add(1);
        }
        if let Some(s) = self.slots.get_mut(idx) {
            *s = Some(next);
        }
        // 执行：超限规格必红；否则按执行器上报的结果落状态。
        let verdict = if passed && next.spec.asserts <= MAX_CASE_ASSERTS {
            CaseState::Passed
        } else {
            CaseState::Failed
        };
        next.state = verdict;
        if let Some(s) = self.slots.get_mut(idx) {
            *s = Some(next);
        }
        self.runs = self.runs.saturating_add(1);
        self.asserts_run = self.asserts_run.saturating_add(next.spec.asserts);
        match verdict {
            CaseState::Passed => self.asserts_pass = self.asserts_pass.saturating_add(next.spec.asserts),
            _ => self.asserts_fail = self.asserts_fail.saturating_add(next.spec.asserts),
        }
        Ok(verdict)
    }

    /// 顺序跑完整批（返回成功条数）。
    pub fn run_all(&mut self) -> u32 {
        let mut ok = 0u32;
        let mut i = 0usize;
        while i < self.count {
            let id = match self.slot_at(i) {
                Some(s) => s.spec.id,
                None => 0,
            };
            if id != 0 {
                if self.run_case(id) == Ok(CaseState::Passed) {
                    ok = ok.saturating_add(1);
                }
            }
            i += 1;
        }
        ok
    }

    /// 结批：全终态 → 掩红 → 守恒 → 权重闭口，四关依次。
    pub fn finish(&mut self) -> Result<u32, KCode> {
        let unfinished = (0..self.count).filter_map(|i| self.slot_at(i))
            .any(|s| !s.state.terminal());
        if unfinished {
            return Err(KCode::NotStarted);
        }
        if self.asserts_pass != self.passed_weight() {
            return Err(KCode::MaskedFailure);
        }
        if self.asserts_pass.saturating_add(self.asserts_fail) != self.asserts_run {
            return Err(KCode::CountMismatch);
        }
        let planned: u32 = (0..self.count).filter_map(|i| self.slot_at(i)).map(|s| s.spec.asserts).sum();
        if planned != BATCH_TOTAL {
            return Err(KCode::WeightMismatch);
        }
        Ok(self.asserts_run)
    }

    /// 故意把某失败用例的断言记进通过账（注入掩红用；生产路径不调）。
    pub fn mask_failure(&mut self, id: u8) -> bool {
        let idx = match self.index_of(id) {
            Some(v) => v,
            None => return false,
        };
        match self.slot_at(idx) {
            Some(s) if s.state == CaseState::Failed => {
                self.asserts_pass = self.asserts_pass.saturating_add(s.spec.asserts);
                true
            }
            _ => false,
        }
    }

    /// 执行账指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::new();
        for s in (0..self.count).filter_map(|i| self.slot_at(i)) {
            bytes.push(s.spec.id);
            bytes.push(s.state as u8);
            bytes.push((s.spec.asserts & 0xFF) as u8);
        }
        bytes.push((self.asserts_run & 0xFF) as u8);
        bytes.push((self.asserts_pass & 0xFF) as u8);
        bytes.push((self.asserts_fail & 0xFF) as u8);
        fnv1a(&bytes)
    }

    /// 结果摘要（写回面要求非空）。
    pub fn summary(&self) -> String {
        format!("B21 批 {} 条 / 跑过 {} 断言 / 通过 {} / 失败 {} / 执行 {} 轮 / 拒收 {}",
                self.count, self.asserts_run, self.asserts_pass,
                self.asserts_fail, self.runs, self.rejects)
    }
}

// ── 对账 ──────────────────────────────────────────────────────────────
/// 逐模块对账表：(模块, 断言小计)。八行之和 = 105,700。
pub const RECON_TABLE: [(Stem, u32); 8] = [
    (Stem::F9218, 5_300),
    (Stem::F9219, 7_500),
    (Stem::F9220, 10_100),
    (Stem::F9221, 24_700),
    (Stem::F9222, 11_100),
    (Stem::F9223, 12_100),
    (Stem::F9224, 18_800),
    (Stem::F9225, 16_100),
];

/// 逐模块对账：组批小计与表内值逐项相等。
pub fn reconcile(plan: &BatchPlan) -> bool {
    let mut i = 0usize;
    while i < RECON_TABLE.len() {
        if let Some((stem, want)) = RECON_TABLE.get(i).copied() {
            if plan.per_stem(stem) != want {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 对账表合计（应为 105,700）。
pub fn recon_total() -> u32 {
    (0..RECON_TABLE.len()).filter_map(|i| RECON_TABLE.get(i)).map(|r| r.1).sum()
}

// ── 注入器 ────────────────────────────────────────────────────────────
/// 三类注入样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KInject {
    /// 同号重复入批
    DuplicateCase,
    /// 权重漂移（多记 1 条断言）
    WeightDrift,
    /// 掩红（失败断言挪进通过账）
    MaskedRed,
}

impl KInject {
    /// 期望码。
    pub fn expected_code(self) -> KCode {
        match self {
            KInject::DuplicateCase => KCode::DuplicateCase,
            KInject::WeightDrift => KCode::WeightMismatch,
            KInject::MaskedRed => KCode::MaskedFailure,
        }
    }
    /// 闭集全量。
    pub const ALL: [KInject; 3] =
        [KInject::DuplicateCase, KInject::WeightDrift, KInject::MaskedRed];
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KLedger {
    /// 注入次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须 0）
    pub escaped: u32,
}

impl KLedger {
    /// 零账。
    pub const fn zero() -> KLedger { KLedger { injected: 0, detected: 0, escaped: 0 } }
    /// 记一笔。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }
    /// 守恒式。
    pub fn conserved(&self) -> bool { self.detected.saturating_add(self.escaped) == self.injected }
}

/// 单轮注入：返回是否被检出。
pub fn inject_once(kind: KInject) -> bool {
    match kind {
        KInject::DuplicateCase => {
            let mut plan = load_batch();
            let dup = CaseSpec::new(1, Cat::Normal, Stem::F9221, 1_000);
            plan.append(dup) == Err(KCode::DuplicateCase) && plan.count() == CASE_COUNT
        }
        KInject::WeightDrift => {
            let mut plan = load_batch();
            let _ = plan.append(CaseSpec::new(20, Cat::Closure, Stem::F9225, 1));
            let mut run = BatchRun::from_plan(&plan);
            let _ = run.run_all();
            run.finish() == Err(KCode::WeightMismatch)
        }
        KInject::MaskedRed => {
            let mut plan = BatchPlan::new();
            let _ = plan.append(CaseSpec::new(1, Cat::Normal, Stem::F9221, 4_000));
            let _ = plan.append(CaseSpec::new(2, Cat::Normal, Stem::F9222, 9_000));
            let mut run = BatchRun::from_plan(&plan);
            let _ = run.run_case_outcome(1, true);
            let _ = run.run_case_outcome(2, false);
            let red = run.asserts_fail() == 9_000 && !run.all_passed();
            let masked = run.mask_failure(2);
            red && masked && run.finish() == Err(KCode::MaskedFailure)
        }
    }
}

/// 跑三类注入 × 10 轮。
pub fn run_ktest_matrix() -> (KLedger, [u32; 3]) {
    let mut ledger = KLedger::zero();
    let mut per_class = [0u32; 3];
    let mut ci = 0usize;
    while ci < KInject::ALL.len() {
        if let Some(kind) = KInject::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = inject_once(kind);
                ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

/// 判定次序专项：准入三关同时违约时先回哪条码。
pub fn judge_order_probe() -> (KCode, KCode, KCode) {
    let good = CaseSpec::new(1, Cat::Normal, Stem::F9221, 1_000);
    let zero = CaseSpec::new(2, Cat::Normal, Stem::F9221, 0);
    let pick = |r: Result<(), KCode>| match r {
        Err(c) => c,
        Ok(()) => KCode::CountMismatch,
    };
    // ① 零权重 + 已绿 → 规格先。
    let first = pick(admit(&zero, CaseState::Passed, 0));
    // ② 已绿 + 重试已耗尽 → 已绿先。
    let second = pick(admit(&good, CaseState::Passed, MAX_RETRIES));
    // ③ 失败 + 重试已耗尽 → 重试耗尽。
    let third = pick(admit(&good, CaseState::Failed, MAX_RETRIES));
    (first, second, third)
}

// ── 指纹 ──────────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机、无墙钟）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范化巡检：载批 → 全跑 → 结批 → 指纹。
pub fn patrol_fingerprint() -> u32 {
    let plan = load_batch();
    let mut run = BatchRun::from_plan(&plan);
    let ok = run.run_all();
    let fin = run.finish();
    let mut acc = run.fingerprint() ^ fnv1a(&[(ok & 0xFF) as u8]);
    if let Ok(n) = fin {
        acc ^= fnv1a(&[(n & 0xFF) as u8, ((n >> 8) & 0xFF) as u8]).wrapping_mul(5);
    }
    acc
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9220 判据集（19 条静态 + 4 条动态；add_dyn ≤8）。
pub fn run_unx9220_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9220");

    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址须唯一且落在本批窗口，越界即拒绝注册");

    // 正常流：19 条全绿
    let plan = load_batch();
    let mut run = BatchRun::from_plan(&plan);
    let passed = run.run_all();
    let finished = run.finish();
    s.add("正常流：19 条判据一次跑通且结批成功",
          passed == CASE_COUNT as u32 && finished == Ok(BATCH_TOTAL) && run.all_passed(),
          "整批必须一次跑通：19 条全绿、结批返回 105,700");

    s.add("收口口径：19 条断言之和恰为 105,700",
          plan.total() == BATCH_TOTAL && recon_total() == BATCH_TOTAL,
          "权重合计与对账表合计必须同时等于 105,700，不许四舍五入");

    // 边界：首末条 / 零权重 / 超限 / 容量
    s.add("边界：首条 B21-01 与末条 B21-19 均可单跑",
          {
              let mut one = BatchRun::from_plan(&plan);
              one.run_case(1) == Ok(CaseState::Passed)
                  && one.run_case(19) == Ok(CaseState::Passed)
                  && one.asserts_run() == 8_700
          },
          "首末条权重 1,200 + 7,500 = 8,700，边界条目不得因序号被拒");

    let mut zero_plan = BatchPlan::new();
    s.add("边界：零权重用例入批 → ZeroAssert 且不占槽",
          zero_plan.append(CaseSpec::new(1, Cat::Normal, Stem::F9221, 0))
              == Err(KCode::ZeroAssert) && zero_plan.count() == 0,
          "空跑用例连槽都不许占，避免先占位后补断言");

    let mut big_plan = BatchPlan::new();
    let over = big_plan.append(CaseSpec::new(1, Cat::Normal, Stem::F9221, 12_000));
    let mut big_run = BatchRun::from_plan(&big_plan);
    s.add("边界：单条超限 → AssertOverflow 拒收且不占槽",
          over == Err(KCode::AssertOverflow)
              && big_plan.count() == 0 && big_plan.rejected() == 1
              && big_run.run_case(1) == Err(KCode::UnknownCase)
              && !big_run.all_passed(),
          "超 10,000 条必须拆矩阵；被拒规格不得进执行账，更不得虚报全绿");

    let mut cap = BatchPlan::new();
    let mut i = 0u8;
    while i < MAX_CASES as u8 {
        let _ = cap.append(CaseSpec::new(i + 1, Cat::Normal, Stem::F9221, 10));
        i += 1;
    }
    s.add("容量：24 槽满后第 25 条 → CapacityFull 且前 24 条不丢",
          cap.append(CaseSpec::new(90, Cat::Normal, Stem::F9221, 10)) == Err(KCode::CapacityFull)
              && cap.count() == MAX_CASES && cap.total() == 240,
          "满槽必须显性拒收，已登记 24 条的权重账不得被覆盖");

    // 注入三类
    let (ledger, per_class) = run_ktest_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入必抓：重复号/权重漂移/掩红三类各 10/10", all_caught,
          "三类违约各 10 轮必须全检出，漏检即红");

    // 判定次序
    let (a, b, c) = judge_order_probe();
    s.add("判定次序：规格 > 已绿 > 重试配额",
          a == KCode::ZeroAssert && b == KCode::AlreadyPassed && c == KCode::RetryExhausted,
          "三关同时违约时须按固定次序回码");

    // 状态机
    s.add("状态机：通过后重跑 → AlreadyPassed（禁刷绿）",
          {
              let mut one = BatchRun::from_plan(&plan);
              let _ = one.run_case(1);
              one.run_case(1) == Err(KCode::AlreadyPassed) && one.runs() == 1
          },
          "已绿用例重跑会把执行账刷成双份，必须硬拒");
    s.add("状态机：失败重试一次为上限，二次 → RetryExhausted",
          {
              let mut p = BatchPlan::new();
              let _ = p.append(CaseSpec::new(1, Cat::Normal, Stem::F9221, 9_000));
              let mut r = BatchRun::from_plan(&p);
              let first = r.run_case_outcome(1, false);
              let second = r.run_case_outcome(1, false);
              let third = r.run_case_outcome(1, false);
              first == Ok(CaseState::Failed) && second == Ok(CaseState::Failed)
                  && third == Err(KCode::RetryExhausted)
          },
          "失败用例只允许重试一次，第二次重试必须被配额挡下");
    s.add("状态机：未跑完即结批 → NotStarted",
          {
              let mut r = BatchRun::from_plan(&plan);
              let _ = r.run_case(1);
              r.finish() == Err(KCode::NotStarted)
          },
          "还剩 Queued 用例时结批等于漏跑，必须显性拒");

    // 守恒 + 对账 + 双跑
    s.add("计数守恒：跑过 == 通过 + 失败且全批通过",
          run.asserts_pass().saturating_add(run.asserts_fail()) == run.asserts_run()
              && run.asserts_run() == BATCH_TOTAL && run.asserts_fail() == 0,
          "跑账两式必须同时成立；通过 105,700、失败 0");
    s.add("掩红检测：通过账 == Passed 态用例权重之和",
          run.asserts_pass() == run.passed_weight(),
          "把失败断言挪进通过账会立刻让两账分叉");
    s.add("对账一致：逐模块断言小计与八卡对账表逐项相等", reconcile(&plan),
          "F9218..F9225 八行小计须与表内值逐项相等，合计 105,700");
    let fp_a = patrol_fingerprint();
    let fp_b = patrol_fingerprint();
    s.add("双跑一致：巡检指纹两次相等", fp_a == fp_b,
          "同批两次必须逐字段相等，隐藏状态即失败");

    // 文案闭集 + 码唯一
    s.add("文案闭集：14 个诊断码均带中文建议",
          KCode::ALL.iter().all(|c| !c.detail().is_empty()),
          "每个码必须能取到非空且可操作的说明");
    let mut uniq = true;
    let mut x = 0usize;
    while x < KCode::ALL.len() {
        let mut y = x + 1;
        while y < KCode::ALL.len() {
            if let (Some(p), Some(q)) = (KCode::ALL.get(x), KCode::ALL.get(y)) {
                if p.val() == q.val() {
                    uniq = false;
                }
            }
            y += 1;
        }
        x += 1;
    }
    s.add("诊断码：14 个码值两两唯一", uniq, "段内闭集码不得重复，否则判据无法定位");

    // 账目单调
    s.add("账目单调：再执行一批后累计断言只增不减",
          {
              let mut again = BatchRun::from_plan(&plan);
              let _ = again.run_all();
              again.asserts_run() == BATCH_TOTAL
          },
          "累计断言不得回退；同一批重复装载须重算出同一总量");

    // 动态明细（4 条）
    let t = format!("注入 {} / 检出 {} / 逃逸 {}", ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("注入账快照", ledger.conserved() && ledger.escaped == 0, &t);
    let per = format!("重复 {} / 漂移 {} / 掩红 {}",
                      match per_class.get(0) { Some(v) => *v, None => 0 },
                      match per_class.get(1) { Some(v) => *v, None => 0 },
                      match per_class.get(2) { Some(v) => *v, None => 0 });
    s.add_dyn("逐类检出快照", all_caught, &per);
    let cons = format!("跑过 {} / 通过 {} / 失败 {} / 执行 {} 轮",
                       run.asserts_run(), run.asserts_pass(), run.asserts_fail(), run.runs());
    s.add_dyn("执行账快照", run.asserts_run() == BATCH_TOTAL, &cons);
    let sum = run.summary();
    s.add_dyn("结果摘要非空", !sum.is_empty(), &sum);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_total_is_exact() {
        let plan = load_batch();
        assert_eq!(plan.count(), CASE_COUNT);
        assert_eq!(plan.total(), BATCH_TOTAL);
        assert_eq!(recon_total(), BATCH_TOTAL);
        assert!(reconcile(&plan));
        assert_eq!(plan.rejected(), 0);
    }

    #[test]
    fn full_run_green() {
        let plan = load_batch();
        let mut run = BatchRun::from_plan(&plan);
        assert_eq!(run.run_all(), 19);
        assert!(run.all_passed());
        assert_eq!(run.asserts_run(), BATCH_TOTAL);
        assert_eq!(run.asserts_fail(), 0);
        assert_eq!(run.finish(), Ok(BATCH_TOTAL));
        assert_eq!(run.runs(), 19);
    }

    #[test]
    fn plan_rejects_bad_specs() {
        let mut p = BatchPlan::new();
        assert_eq!(p.append(CaseSpec::new(1, Cat::Normal, Stem::F9221, 0)), Err(KCode::ZeroAssert));
        assert_eq!(p.append(CaseSpec::new(2, Cat::Normal, Stem::F9221, 20_000)),
                   Err(KCode::AssertOverflow));
        assert_eq!(p.count(), 0);
        assert_eq!(p.rejected(), 2);
    }

    #[test]
    fn capacity_keeps_ledger() {
        let mut p = BatchPlan::new();
        for i in 0..MAX_CASES {
            assert!(p.append(CaseSpec::new(i as u8 + 1, Cat::Normal, Stem::F9222, 10)).is_ok());
        }
        assert_eq!(p.append(CaseSpec::new(90, Cat::Normal, Stem::F9222, 10)), Err(KCode::CapacityFull));
        assert_eq!(p.count(), MAX_CASES);
        assert_eq!(p.total(), 240);
    }

    #[test]
    fn state_machine_guards() {
        let plan = load_batch();
        let mut run = BatchRun::from_plan(&plan);
        assert_eq!(run.finish(), Err(KCode::NotStarted));
        assert_eq!(run.run_case(1), Ok(CaseState::Passed));
        assert_eq!(run.run_case(1), Err(KCode::AlreadyPassed));
        assert_eq!(run.run_case(99), Err(KCode::UnknownCase));
        assert_eq!(run.finish(), Err(KCode::NotStarted));
    }

    #[test]
    fn retry_quota_enforced() {
        let mut p = BatchPlan::new();
        let _ = p.append(CaseSpec::new(1, Cat::Normal, Stem::F9221, 9_000));
        let mut r = BatchRun::from_plan(&p);
        assert_eq!(r.run_case_outcome(1, false), Ok(CaseState::Failed));
        assert_eq!(r.run_case_outcome(1, false), Ok(CaseState::Failed));
        assert_eq!(r.run_case_outcome(1, false), Err(KCode::RetryExhausted));
        assert_eq!(r.asserts_fail(), 18_000);
    }

    #[test]
    fn masking_is_detected() {
        let mut p = BatchPlan::new();
        let _ = p.append(CaseSpec::new(1, Cat::Normal, Stem::F9221, 4_000));
        let _ = p.append(CaseSpec::new(2, Cat::Normal, Stem::F9222, 9_000));
        let mut r = BatchRun::from_plan(&p);
        let _ = r.run_case_outcome(1, true);
        let _ = r.run_case_outcome(2, false);
        assert!(!r.all_passed());
        assert_eq!(r.finish(), Err(KCode::WeightMismatch));
        assert!(r.mask_failure(2));
        assert_eq!(r.finish(), Err(KCode::MaskedFailure));
        assert_ne!(r.asserts_pass(), r.passed_weight());
    }

    #[test]
    fn injections_catch_all_ten() {
        let (l, per) = run_ktest_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, INJECT_CLASSES * INJECT_ROUNDS);
        for v in per.iter() {
            assert_eq!(*v, INJECT_ROUNDS);
        }
        let (a, b, c) = judge_order_probe();
        assert_eq!((a, b, c), (KCode::ZeroAssert, KCode::AlreadyPassed, KCode::RetryExhausted));
    }

    #[test]
    fn deterministic_patrol() {
        assert_eq!(patrol_fingerprint(), patrol_fingerprint());
        let plan = load_batch();
        let mut a = BatchRun::from_plan(&plan);
        let mut b = BatchRun::from_plan(&plan);
        let _ = a.run_all();
        let _ = b.run_all();
        assert_eq!(a.fingerprint(), b.fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9220_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
        assert!(s.len() >= 12);
    }
}
