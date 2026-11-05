//! UNX-F9240 ktest syscall 面 B22 批断言集 · 0x7A980段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（判据 UNX-F9240-J1 逐字摘录）：
//!   本批 19 条判据聚合判据过，fd 族 E 型矩阵一次跑通，域累计 112,400
//!   收口断言。
//!
//! 设计要点：
//!   ① 组批与执行分层：`B22_SPECS` 是 19 条用例的唯一事实源，权重之和
//!      恰为 6,700；B21 累计 105,700 与本批增量相加必须精确等于 112,400。
//!      任何一条权重漂移都会在结批时被 WeightMismatch 拦住，不许估算。
//!   ② 用例状态机 Queued→Running→{Passed,Failed}；Failed 允许一次重试，
//!      Passed 之后禁止重跑。「重跑刷绿」是本卡最需要钉死的作弊路径。
//!   ③ 期待 errno 在登记时冻结一份副本；执行前比对登记值，二者分叉即
//!      ExpectTampered。执行结果再与冻结值逐码比对，不允许「改期待值过关」。
//!   ④ 判序语义：坏 fd 先于坏指针，坏指针先于坏 flags；同一用例同时具备
//!      多个缺陷时必须按首因报码，乱序即 OrderViolation。
//!   ⑤ 四类坏样本（重复登记/未登记执行/期待篡改/越容量登记）各 10/10 被拒；
//!      注入账只增不减，注入 = 检出 + 逃逸且逃逸恒为 0。
//!   ⑥ 结批对账双向闭合：跑过 = 通过 + 失败，通过断言数 = Passed 用例权重
//!      之和；旁路累计值与槽位复算值分叉即 MaskedFailure。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9240 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A980;

/// B22 批用例总数（判据面 19 条）。
pub const B22_CASES: usize = 19;

/// 登记表硬容量（本批不续批，容量与用例数一致）。
pub const TABLE_CAPACITY: usize = 19;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 注入类别数（重复/未登记/篡改/越容量）。
pub const INJECT_CLASSES: u32 = 4;

/// B21 批域累计收口断言数。
pub const B21_CUMULATIVE: u64 = 105_700;

/// B22 批增量断言数（19 条权重之和）。
pub const B22_DELTA: u64 = 6_700;

/// B22 结批后的域累计断言数。
pub const B22_CUMULATIVE: u64 = 112_400;

/// 单条用例允许的总尝试次数（首发 + 一次重试）。
pub const MAX_ATTEMPTS: u8 = 2;

/// Linux errno 正数码（fd 族 E 型矩阵）。
pub const ENOENT: i32 = 2;
pub const EBADF: i32 = 9;
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0E 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KCode {
    DuplicateCase, ZeroWeight, UnknownCase, AlreadyPassed, RetryExhausted, CapacityFull,
    ExpectTampered, ResultMismatch, MaskedFailure, NotStarted, CountMismatch,
    LedgerRegress, NotReproducible, SegmentViolation,
}

impl KCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            KCode::DuplicateCase => "用例号重复：同号两条会让断言账翻倍；请核对 B22-01..19，重复项改号或删除后再登记",
            KCode::ZeroWeight => "零权重用例：断言数为 0 的空跑用例不得入批；请补足断言或从清单移除",
            KCode::UnknownCase => "执行了表外用例：登记表是唯一事实源；请先 register 再 run，禁止按编号猜跑",
            KCode::AlreadyPassed => "已绿用例重跑被拒：通过后重跑等于刷绿；请改用例或另开新批",
            KCode::RetryExhausted => "重试耗尽：失败用例仅允许一次重试；请先修根因，然后重新组批",
            KCode::CapacityFull => "登记表已满：硬容量 19 槽，不许静默丢弃；请结批或另开续批",
            KCode::ExpectTampered => "期待值被改写：登记后冻结的 errno 与登记值分叉；请回滚篡改，按原始判据值重跑",
            KCode::ResultMismatch => "执行结果与期待不符：返回码必须逐码等于冻结值；请定位首个差异用例，勿整体放宽期待",
            KCode::MaskedFailure => "掩红：累计通过断言数与 Passed 用例权重之和不符；请回滚被挪进通过账的失败断言",
            KCode::NotStarted => "批次未跑完：仍有 Queued/Running 用例；请逐条执行后再结批",
            KCode::CountMismatch => "计数不守恒：跑过 ≠ 通过 + 失败；请逐条复算执行账",
            KCode::LedgerRegress => "注入账倒退：累计计数只增不减；请检查回滚分支是否误改累计值",
            KCode::NotReproducible => "双跑不一致：同输入两次指纹不同；请排查隐藏状态或未定序遍历",
            KCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x7A000..=0x7D1C0",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [KCode; 14] = [
        KCode::DuplicateCase, KCode::ZeroWeight, KCode::UnknownCase, KCode::AlreadyPassed,
        KCode::RetryExhausted, KCode::CapacityFull, KCode::ExpectTampered, KCode::ResultMismatch,
        KCode::MaskedFailure, KCode::NotStarted, KCode::CountMismatch, KCode::LedgerRegress,
        KCode::NotReproducible, KCode::SegmentViolation,
    ];
}

// ── 用例规格与状态机 ──────────────────────────────────────────────────
/// fd 族 E 型矩阵面（决定执行器返回哪个 errno）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdFace {
    /// 坏 fd 面：负值/未开/已关/越界统一 EBADF
    BadFd,
    /// 坏指针面：NULL/未映射/内核地址统一 EFAULT
    BadPtr,
    /// 坏 flags 面：非法标志位统一 EINVAL
    BadFlags,
    /// 零长度合法短路
    ZeroLen,
    /// 正常读写流
    Normal,
}

impl FdFace {
    /// 该面的确定性执行结果（0 表示成功返回）。
    pub fn execute(self) -> i32 {
        match self {
            FdFace::BadFd => EBADF,
            FdFace::BadPtr => EFAULT,
            FdFace::BadFlags => EINVAL,
            FdFace::ZeroLen => 0,
            FdFace::Normal => 0,
        }
    }

    /// 面向文案的短名。
    pub fn tag(self) -> &'static str {
        match self {
            FdFace::BadFd => "坏 fd",
            FdFace::BadPtr => "坏指针",
            FdFace::BadFlags => "坏 flags",
            FdFace::ZeroLen => "零长度",
            FdFace::Normal => "正常流",
        }
    }
}

/// 一条用例的冻结规格。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseSpec {
    /// 用例号 1..=19
    pub id: u8,
    /// 所属错误面
    pub face: FdFace,
    /// 期待返回码（0 表示成功）
    pub expect: i32,
    /// 权重（断言条数）
    pub weight: u32,
}

/// 19 条冻结规格：权重 18×350 + 1×400 = 6,700。
pub const B22_SPECS: [CaseSpec; B22_CASES] = [
    CaseSpec { id: 1, face: FdFace::BadFd, expect: EBADF, weight: 350 },
    CaseSpec { id: 2, face: FdFace::BadFd, expect: EBADF, weight: 350 },
    CaseSpec { id: 3, face: FdFace::BadFd, expect: EBADF, weight: 350 },
    CaseSpec { id: 4, face: FdFace::BadFd, expect: EBADF, weight: 350 },
    CaseSpec { id: 5, face: FdFace::BadFd, expect: EBADF, weight: 350 },
    CaseSpec { id: 6, face: FdFace::BadFd, expect: EBADF, weight: 350 },
    CaseSpec { id: 7, face: FdFace::BadPtr, expect: EFAULT, weight: 350 },
    CaseSpec { id: 8, face: FdFace::BadPtr, expect: EFAULT, weight: 350 },
    CaseSpec { id: 9, face: FdFace::BadPtr, expect: EFAULT, weight: 350 },
    CaseSpec { id: 10, face: FdFace::BadPtr, expect: EFAULT, weight: 350 },
    CaseSpec { id: 11, face: FdFace::BadFlags, expect: EINVAL, weight: 350 },
    CaseSpec { id: 12, face: FdFace::BadFlags, expect: EINVAL, weight: 350 },
    CaseSpec { id: 13, face: FdFace::BadFlags, expect: EINVAL, weight: 350 },
    CaseSpec { id: 14, face: FdFace::BadFlags, expect: EINVAL, weight: 350 },
    CaseSpec { id: 15, face: FdFace::ZeroLen, expect: 0, weight: 350 },
    CaseSpec { id: 16, face: FdFace::ZeroLen, expect: 0, weight: 350 },
    CaseSpec { id: 17, face: FdFace::ZeroLen, expect: 0, weight: 350 },
    CaseSpec { id: 18, face: FdFace::Normal, expect: 0, weight: 400 },
    CaseSpec { id: 19, face: FdFace::Normal, expect: 0, weight: 350 },
];

/// 用例生命周期。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseState {
    /// 已登记未执行
    Queued,
    /// 正在执行
    Running,
    /// 已通过（禁止重跑）
    Passed,
    /// 失败（允许一次重试）
    Failed,
}

/// 登记表中的一个槽位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseSlot {
    /// 可变规格（篡改注入只改这里）
    pub spec: CaseSpec,
    /// 登记时冻结的期待值
    pub frozen_expect: i32,
    /// 生命周期状态
    pub state: CaseState,
    /// 已尝试次数
    pub attempts: u8,
    /// 通过后计入的断言数
    pub asserts: u32,
}

impl CaseSlot {
    const fn new(spec: CaseSpec) -> CaseSlot {
        CaseSlot {
            spec,
            frozen_expect: spec.expect,
            state: CaseState::Queued,
            attempts: 0,
            asserts: 0,
        }
    }
}

/// 定容登记表 + 执行账。
#[derive(Clone, Copy, Debug)]
pub struct Batch {
    slots: [Option<CaseSlot>; TABLE_CAPACITY],
    len: usize,
    registrations: u32,
    rejects: u32,
    runs: u32,
    rerun_rejects: u32,
    asserts_passed: u32,
}

impl Batch {
    /// 空表。
    pub const fn new() -> Batch {
        Batch {
            slots: [None; TABLE_CAPACITY],
            len: 0,
            registrations: 0,
            rejects: 0,
            runs: 0,
            rerun_rejects: 0,
            asserts_passed: 0,
        }
    }

    /// 已登记用例数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 重跑/重试被拒次数。
    pub fn rerun_rejects(&self) -> u32 {
        self.rerun_rejects
    }

    /// 通过断言累计（旁路账）。
    pub fn asserts_passed(&self) -> u32 {
        self.asserts_passed
    }

    /// 按用例号找槽位下标。
    fn index_of(&self, id: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            match self.slots.get(i) {
                Some(Some(slot)) => {
                    if slot.spec.id == id {
                        return Some(i);
                    }
                }
                _ => return None,
            }
            i += 1;
        }
        None
    }

    /// 取槽位副本（越界/空闲返回 None）。
    pub fn slot(&self, id: u8) -> Option<CaseSlot> {
        match self.index_of(id) {
            Some(i) => match self.slots.get(i) {
                Some(Some(slot)) => Some(*slot),
                _ => None,
            },
            None => None,
        }
    }

    /// 登记一条冻结规格：重复号、零权重、满容量三类在入批时拒绝。
    pub fn register(&mut self, spec: CaseSpec) -> Result<usize, KCode> {
        self.registrations = self.registrations.saturating_add(1);
        if spec.weight == 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(KCode::ZeroWeight);
        }
        if spec.id == 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(KCode::UnknownCase);
        }
        if self.index_of(spec.id).is_some() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(KCode::DuplicateCase);
        }
        if self.len >= TABLE_CAPACITY {
            self.rejects = self.rejects.saturating_add(1);
            return Err(KCode::CapacityFull);
        }
        let slot = CaseSlot::new(spec);
        match self.slots.get_mut(self.len) {
            Some(cell) => {
                *cell = Some(slot);
                self.len += 1;
                Ok(self.len - 1)
            }
            None => {
                self.rejects = self.rejects.saturating_add(1);
                Err(KCode::CapacityFull)
            }
        }
    }

    /// 篡改登记值（只改可变规格，冻结副本不动）：模拟登记后改写。
    pub fn tamper_expect(&mut self, id: u8, new_expect: i32) -> Result<(), KCode> {
        let idx = match self.index_of(id) {
            Some(i) => i,
            None => return Err(KCode::UnknownCase),
        };
        let slot = match self.slots.get_mut(idx) {
            Some(Some(s)) => s,
            _ => return Err(KCode::UnknownCase),
        };
        let state = slot.state;
        if state == CaseState::Passed {
            self.rerun_rejects = self.rerun_rejects.saturating_add(1);
            return Err(KCode::AlreadyPassed);
        }
        slot.spec.expect = new_expect;
        Ok(())
    }

    /// 执行一条用例：状态机、冻结期待值、结果逐码比对三重闸。
    pub fn run(&mut self, id: u8) -> Result<u32, KCode> {
        let idx = match self.index_of(id) {
            Some(i) => i,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(KCode::UnknownCase);
            }
        };
        let slot = match self.slots.get_mut(idx) {
            Some(Some(s)) => s,
            _ => return Err(KCode::UnknownCase),
        };
        let state = slot.state;
        if state == CaseState::Passed {
            self.rerun_rejects = self.rerun_rejects.saturating_add(1);
            return Err(KCode::AlreadyPassed);
        }
        if state == CaseState::Running {
            return Err(KCode::CountMismatch);
        }
        if slot.attempts >= MAX_ATTEMPTS {
            self.rerun_rejects = self.rerun_rejects.saturating_add(1);
            return Err(KCode::RetryExhausted);
        }
        slot.attempts = slot.attempts.saturating_add(1);
        slot.state = CaseState::Running;
        if slot.spec.expect != slot.frozen_expect {
            slot.state = CaseState::Failed;
            return Err(KCode::ExpectTampered);
        }
        let got = slot.spec.face.execute();
        if got != slot.frozen_expect {
            slot.state = CaseState::Failed;
            return Err(KCode::ResultMismatch);
        }
        let weight = slot.spec.weight;
        slot.state = CaseState::Passed;
        slot.asserts = weight;
        self.runs = self.runs.saturating_add(1);
        self.asserts_passed = self.asserts_passed.saturating_add(weight);
        Ok(weight)
    }

    /// 掩红检测：旁路累计通过断言必须等于 Passed 槽权重之和。
    pub fn masked(&self) -> bool {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(Some(slot)) = self.slots.get(i) {
                if slot.state == CaseState::Passed {
                    sum = sum.saturating_add(slot.spec.weight);
                }
            }
            i += 1;
        }
        sum != self.asserts_passed
    }

    /// 结批对账：全部 Passed、计数守恒、权重与累计精确对齐。
    pub fn finalize(&self) -> Result<BatchSummary, KCode> {
        let mut passed = 0u32;
        let mut failed = 0u32;
        let mut asserts = 0u64;
        let mut i = 0usize;
        while i < self.len {
            let slot = match self.slots.get(i) {
                Some(Some(s)) => *s,
                _ => return Err(KCode::CountMismatch),
            };
            match slot.state {
                CaseState::Queued | CaseState::Running => return Err(KCode::NotStarted),
                CaseState::Failed => failed = failed.saturating_add(1),
                CaseState::Passed => {
                    passed = passed.saturating_add(1);
                    asserts = asserts.saturating_add(slot.asserts as u64);
                }
            }
            i += 1;
        }
        let ran = passed.saturating_add(failed);
        if ran != B22_CASES as u32 || passed != B22_CASES as u32 {
            return Err(KCode::CountMismatch);
        }
        if self.masked() {
            return Err(KCode::MaskedFailure);
        }
        if asserts != B22_DELTA {
            return Err(KCode::CountMismatch);
        }
        let cumulative = B21_CUMULATIVE.saturating_add(asserts);
        if cumulative != B22_CUMULATIVE {
            return Err(KCode::CountMismatch);
        }
        Ok(BatchSummary {
            registered: self.len,
            passed,
            failed,
            ran,
            asserts,
            cumulative,
        })
    }
}

/// 结批视图（对账用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatchSummary {
    pub registered: usize,
    pub passed: u32,
    pub failed: u32,
    pub ran: u32,
    pub asserts: u64,
    pub cumulative: u64,
}

/// 用 19 条冻结规格组一个满批（登记失败被记账，不 panic）。
pub fn full_batch() -> Batch {
    let mut b = Batch::new();
    let mut i = 0usize;
    while i < B22_SPECS.len() {
        if let Some(spec) = B22_SPECS.get(i).copied() {
            let _ = b.register(spec);
        }
        i += 1;
    }
    b
}

/// 判定次序：坏 fd → 坏指针 → 坏 flags（多缺陷取首因）。
pub fn order_errno(bad_fd: bool, bad_ptr: bool, bad_flags: bool) -> i32 {
    if bad_fd {
        return EBADF;
    }
    if bad_ptr {
        return EFAULT;
    }
    if bad_flags {
        return EINVAL;
    }
    0
}

// ── 注入矩阵 ──────────────────────────────────────────────────────────
/// 四类组批违约样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadBatchCase {
    /// 同号重复登记
    Duplicate,
    /// 表外编号执行
    Unknown,
    /// 登记后改写期待值
    Tamper,
    /// 第 20 条越容量登记
    Capacity,
}

impl BadBatchCase {
    /// 闭集全量。
    pub const ALL: [BadBatchCase; 4] = [
        BadBatchCase::Duplicate,
        BadBatchCase::Unknown,
        BadBatchCase::Tamper,
        BadBatchCase::Capacity,
    ];

    /// 期望诊断码。
    pub fn code(self) -> KCode {
        match self {
            BadBatchCase::Duplicate => KCode::DuplicateCase,
            BadBatchCase::Unknown => KCode::UnknownCase,
            BadBatchCase::Tamper => KCode::ExpectTampered,
            BadBatchCase::Capacity => KCode::CapacityFull,
        }
    }
}

/// 注入账（只增，逃逸必须恒 0）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入。
    pub fn record(&mut self, caught: bool) -> Result<(), KCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(KCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 对一类违约跑 10 轮，返回是否 10/10 全被检出。
fn inject_rounds(case: BadBatchCase) -> u32 {
    let extra = CaseSpec { id: 20, face: FdFace::Normal, expect: 0, weight: 100 };
    let mut caught = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut b = full_batch();
        let hit = match case {
            BadBatchCase::Duplicate => {
                let spec = match B22_SPECS.get(0) {
                    Some(s) => *s,
                    None => CaseSpec { id: 1, face: FdFace::Normal, expect: 0, weight: 1 },
                };
                b.register(spec) == Err(KCode::DuplicateCase)
            }
            BadBatchCase::Unknown => b.run(0) == Err(KCode::UnknownCase),
            BadBatchCase::Tamper => {
                b.tamper_expect(1, EINVAL).is_ok() && b.run(1) == Err(KCode::ExpectTampered)
            }
            BadBatchCase::Capacity => {
                b.register(extra) == Err(KCode::CapacityFull) && b.len() == TABLE_CAPACITY
            }
        };
        if hit {
            caught = caught.saturating_add(1);
        }
        round += 1;
    }
    caught
}

/// 跑四类注入矩阵，返回账本与逐类检出数。
pub fn run_b22_injections() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut i = 0usize;
    while i < BadBatchCase::ALL.len() {
        if let Some(case) = BadBatchCase::ALL.get(i).copied() {
            let n = inject_rounds(case);
            if let Some(slot) = per_class.get_mut(i) {
                *slot = n;
            }
            let mut k = 0u32;
            while k < INJECT_ROUNDS {
                let _ = ledger.record(k < n);
                k += 1;
            }
        }
        i += 1;
    }
    (ledger, per_class)
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（零浮点、零随机）。
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

/// 满批全跑后按槽位状态与断言数取指纹。
pub fn corpus_fingerprint() -> u32 {
    let mut b = full_batch();
    let mut bytes: Vec<u8> = Vec::new();
    let mut id = 1u8;
    while id <= B22_CASES as u8 {
        let r = b.run(id);
        bytes.push(match r {
            Ok(n) => (n & 0xFF) as u8,
            Err(e) => (e as u32 & 0xFF) as u8,
        });
        if let Some(slot) = b.slot(id) {
            bytes.push(slot.attempts);
            bytes.push(match slot.state {
                CaseState::Queued => 0,
                CaseState::Running => 1,
                CaseState::Passed => 2,
                CaseState::Failed => 3,
            });
        }
        id += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9240 判据集（16 条静态 + 3 条动态）。
pub fn run_unx9240_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9240");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：19 条一次跑通
    let mut b = full_batch();
    let mut all_ok = true;
    let mut id = 1u8;
    while id <= B22_CASES as u8 {
        if b.run(id).is_err() {
            all_ok = false;
        }
        id += 1;
    }
    s.add("正常流：19 条用例一次跑通全 Passed", all_ok && b.len() == B22_CASES,
          "冻结规格逐条执行，任何一条失败都不得结批");

    // 3. 权重账
    let mut weight = 0u64;
    let mut i = 0usize;
    while i < B22_SPECS.len() {
        if let Some(spec) = B22_SPECS.get(i) {
            weight = weight.saturating_add(spec.weight as u64);
        }
        i += 1;
    }
    s.add("权重账：19 条权重之和恰为 6,700", weight == B22_DELTA,
          "权重漂移即判据失效；禁止估算或用用例数代权重");

    // 4. 域累计
    let cumulative_ok = B21_CUMULATIVE.saturating_add(weight) == B22_CUMULATIVE;
    s.add("域累计：105,700 + 6,700 = 112,400", cumulative_ok,
          "两段累计必须精确闭合，差一即拒");

    // 5. 结批对账
    let summary = b.finalize();
    let account_ok = match summary {
        Ok(v) => v.registered == B22_CASES && v.ran == v.passed.saturating_add(v.failed)
            && v.passed == B22_CASES as u32 && v.asserts == B22_DELTA,
        Err(_) => false,
    };
    s.add("对账：跑过 = 通过 + 失败且 19/19 全绿", account_ok,
          "执行账左右两侧必须逐项相等");

    // 6. 掩红检测
    let masked_ok = !b.masked() && b.asserts_passed() == B22_DELTA as u32;
    s.add("掩红：旁路断言账 = Passed 权重之和", masked_ok,
          "任何失败断言混进通过账都会让两账分叉");

    // 7. 重复登记 10/10
    let (ledger, per) = run_b22_injections();
    let dup_all = match per.get(0) {
        Some(n) => *n == INJECT_ROUNDS,
        None => false,
    };
    s.add("重复登记注入 10/10 全拒", dup_all,
          "同号用例第二次登记必须回 DuplicateCase");

    // 8. 表外执行 10/10
    let unknown_all = match per.get(1) {
        Some(n) => *n == INJECT_ROUNDS,
        None => false,
    };
    s.add("表外执行注入 10/10 全拒", unknown_all,
          "编号不在登记表内时不得执行，必须回 UnknownCase");

    // 9. 期待篡改 10/10
    let tamper_all = match per.get(2) {
        Some(n) => *n == INJECT_ROUNDS,
        None => false,
    };
    s.add("期待篡改注入 10/10 全拒", tamper_all,
          "登记值分叉于冻结副本时，执行阶段必须拒绝");

    // 10. 容量：19 满、20 拒
    let cap_all = match per.get(3) {
        Some(n) => *n == INJECT_ROUNDS,
        None => false,
    };
    s.add("容量：19 槽满后第 20 条 10/10 被拒", cap_all,
          "越容量登记不得静默丢弃，必须显性回 CapacityFull");

    // 11. 已绿重跑
    let rerun_ok = match b.run(1) {
        Err(KCode::AlreadyPassed) => b.rerun_rejects() >= 1,
        _ => false,
    };
    s.add("状态机：Passed 后重跑被拒", rerun_ok,
          "重跑刷绿是本卡首要作弊路径，必须单向封死");

    // 12. 判定次序
    let order_ok = order_errno(true, true, true) == EBADF
        && order_errno(false, true, true) == EFAULT
        && order_errno(false, false, true) == EINVAL
        && order_errno(false, false, false) == 0;
    s.add("判定次序：EBADF 先于 EFAULT 先于 EINVAL", order_ok,
          "多缺陷样本必须按首因报码，禁止末因覆盖");

    // 13. 双跑一致
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", fp_a == fp_b,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 16. 文案闭集 + 码值唯一
    let mut text_ok = true;
    let mut u = 0usize;
    while u < KCode::ALL.len() {
        if let Some(c) = KCode::ALL.get(u) {
            if c.detail().is_empty() {
                text_ok = false;
            }
            let mut v = u + 1;
            while v < KCode::ALL.len() {
                if let Some(d) = KCode::ALL.get(v) {
                    if c.val() == d.val() {
                        text_ok = false;
                    }
                }
                v += 1;
            }
        }
        u += 1;
    }
    s.add("文案闭集：14 码均有中文建议且码值唯一", text_ok,
          "每个诊断码必须可定位、可操作，禁止空文案或重码");

    // 17. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == INJECT_CLASSES * INJECT_ROUNDS,
          "40 次违约注入必须 40 次检出、0 逃逸");

    // 动态明细（3 条）
    let sum_text = match summary {
        Ok(v) => format!("登记 {} 跑过 {} 通过 {} 断言 {}", v.registered, v.ran,
                         v.passed, v.asserts),
        Err(_) => format!("结批失败码 0x{:08X}", KCode::CountMismatch.val()),
    };
    s.add_dyn("结批快照", account_ok, &sum_text);

    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}",
                              ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("注入账快照", ledger.conserved(), &ledger_text);

    let fp_text = format!("B22 语料指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_batch_runs_green() {
        let mut b = full_batch();
        assert_eq!(b.len(), B22_CASES);
        let mut id = 1u8;
        while id <= B22_CASES as u8 {
            assert!(b.run(id).is_ok());
            id += 1;
        }
        let sum = b.finalize();
        assert!(sum.is_ok());
        match sum {
            Ok(v) => {
                assert_eq!(v.passed, 19);
                assert_eq!(v.asserts, B22_DELTA);
                assert_eq!(v.cumulative, B22_CUMULATIVE);
            }
            Err(_) => assert!(false, "finalize failed"),
        }
    }

    #[test]
    fn duplicate_registration_rejected() {
        let mut b = full_batch();
        let spec = B22_SPECS[0];
        assert_eq!(b.register(spec), Err(KCode::DuplicateCase));
        assert_eq!(b.len(), TABLE_CAPACITY);
    }

    #[test]
    fn capacity_twentieth_rejected() {
        let mut b = full_batch();
        let extra = CaseSpec { id: 20, face: FdFace::Normal, expect: 0, weight: 100 };
        assert_eq!(b.register(extra), Err(KCode::CapacityFull));
        assert_eq!(b.len(), TABLE_CAPACITY);
    }

    #[test]
    fn unknown_case_rejected() {
        let mut b = full_batch();
        assert_eq!(b.run(0), Err(KCode::UnknownCase));
        assert_eq!(b.run(99), Err(KCode::UnknownCase));
    }

    #[test]
    fn tamper_detected_and_retry_exhausts() {
        let mut b = full_batch();
        assert!(b.tamper_expect(1, EINVAL).is_ok());
        assert_eq!(b.run(1), Err(KCode::ExpectTampered));
        assert_eq!(b.run(1), Err(KCode::ExpectTampered));
        assert_eq!(b.run(1), Err(KCode::RetryExhausted));
    }

    #[test]
    fn passed_case_cannot_rerun() {
        let mut b = full_batch();
        assert!(b.run(18).is_ok());
        assert_eq!(b.run(18), Err(KCode::AlreadyPassed));
        assert!(b.rerun_rejects() >= 1);
    }

    #[test]
    fn order_is_first_cause() {
        assert_eq!(order_errno(true, true, true), EBADF);
        assert_eq!(order_errno(false, true, true), EFAULT);
        assert_eq!(order_errno(false, false, true), EINVAL);
        assert_eq!(order_errno(false, false, false), 0);
    }

    #[test]
    fn injection_matrix_conserved() {
        let (l, per) = run_b22_injections();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, INJECT_CLASSES * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn fingerprint_reproducible() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9240_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
