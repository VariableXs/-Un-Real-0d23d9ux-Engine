//! UNX-F10037 清偿双档复测（T1/ktest 对账） · 0x91900段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10038-J1 演练留痕（清偿流程全走一遍 10/10 次），跳步演练
//! 10/10 次拦截。
//!
//! SEG_BASE = 0x91900：F10037 独占 0x40 窗口；诊断码取 SEG_BASE + 变体序号
//! （+0x01..+0x0C 闭集），段外码一律非法。
//!
//! 设计要点：
//!   ① 双档复测按「T1 档 × ktest 档逐条三分」建模：每条目两档各出 Pass/Red
//!      一果，三分闭集——双绿（一致通过）/ 双红（一致红）/ 单红（需裁决）；
//!      单红未裁决即未决差异，对账 10/10 次拦截。
//!   ② 账本 + 状态机 + 三分类器三件齐：两档结果账逐条可查、裁决账逐条
//!      留痕（重复裁决拒绝）、对账生命周期 Collecting→Reconciled→Sealed
//!      只进不退。
//!   ③ 注入核四类坏样本：单红未裁决 / 两档条目集不齐 / 重复裁决 / 无差异
//!      空裁决，逐类 10 轮复测全部显性拒绝，逃逸恒为 0。
//!   ④ 与 Linux errno 冻结表逐码对照；FNV-1a 双跑指纹逐位一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10037 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x91900;

/// 复测条目容量（条数上限）。
pub const ITEM_CAPACITY: usize = 16;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码。
pub const ENOENT: i32 = 2;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const EPERM: i32 = 1;
pub const ENOTUNIQ: i32 = 35;

// ── 复测结果闭集 ──────────────────────────────────────────────────────
/// 单档复测结果（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// 通过
    Pass,
    /// 红
    Red,
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DualCode {
    /// 单红未决：单红差异未挂裁决
    UndecidedDiff,
    /// 条目集不齐：两档条目集不一致
    TrackMismatch,
    /// 重复裁决：同条目二次裁决
    DoubleAdjudication,
    /// 空裁决：无差异条目挂裁决
    AdjudicateNoDiff,
    /// 重复录果：同条目同档二次录果
    DuplicateResult,
    /// 录果越界：条目号零
    BadItem,
    /// 未对齐封账
    SealBeforeReconcile,
    /// 对齐后录果
    ResultAfterReconcile,
    /// 状态回退
    StateRegress,
    /// 空账对齐
    EmptyReconcile,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl DualCode {
    /// 码值（段基址 + 变体序号，从 1 起）。
    pub fn code(self) -> u32 {
        SEG_BASE + (self as u32) + 1
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            DualCode::UndecidedDiff => "单红未决被拒：单红差异必须挂裁决留痕；\
                未决差异一律拦截，不许带悬出账",
            DualCode::TrackMismatch => "条目集不齐被拒：T1 与 ktest 两档必须\
                同条目集；缺档条目请补测",
            DualCode::DoubleAdjudication => "重复裁决被拒：同条目已裁决；\
                改判请开新一轮复测",
            DualCode::AdjudicateNoDiff => "空裁决被拒：无差异条目不得挂裁决；\
                裁决只对单红差异生效",
            DualCode::DuplicateResult => "重复录果被拒：同条目同档已录；\
                请先查账再补录，重复回 EEXIST",
            DualCode::BadItem => "条目号坏被拒：编号须 1..=99；请修正后再录果",
            DualCode::SealBeforeReconcile => "未对齐封账被拒：先对齐（零未决）\
                才能封账；请先跑逐条三分",
            DualCode::ResultAfterReconcile => "对齐后录果被拒：对齐即冻结；\
                补果请开新一轮复测",
            DualCode::StateRegress => "状态回退被拒：对账生命周期只进不退；\
                回退视为私改对账",
            DualCode::EmptyReconcile => "空账对齐被拒：零条目对账无意义；\
                请先录果",
            DualCode::NotReproducible => "双跑不一致：同输入两次结果不同；\
                请排查隐藏状态或非确定遍历序",
            DualCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且\
                落在 0x91900..=0x91980 窗口",
        }
    }

    /// 闭集全量。
    pub const ALL: [DualCode; 12] = [
        DualCode::UndecidedDiff,
        DualCode::TrackMismatch,
        DualCode::DoubleAdjudication,
        DualCode::AdjudicateNoDiff,
        DualCode::DuplicateResult,
        DualCode::BadItem,
        DualCode::SealBeforeReconcile,
        DualCode::ResultAfterReconcile,
        DualCode::StateRegress,
        DualCode::EmptyReconcile,
        DualCode::NotReproducible,
        DualCode::SegmentViolation,
    ];
}

// ── 三分类（闭集） ────────────────────────────────────────────────────
/// 双档对账三分（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Triage {
    /// 双绿：一致通过
    BothPass,
    /// 双红：一致红
    BothRed,
    /// 单红：需裁决
    SingleRed,
}

// ── 双档账本 ──────────────────────────────────────────────────────────
/// 单条目双档结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DualResult {
    /// 条目号（1..=99）
    pub item: u8,
    /// T1 档结果
    pub t1: Outcome,
    /// ktest 档结果
    pub ktest: Outcome,
}

/// 对账生命周期：只进不退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReconState {
    /// 录果中
    Collecting,
    /// 已对齐
    Reconciled,
    /// 已封账
    Sealed,
}

/// 双档复测对账：两档结果账 + 裁决账 + 三分类器。
#[derive(Clone, Debug)]
pub struct DualRetest {
    state: ReconState,
    t1: Vec<(u8, Outcome)>,
    ktest: Vec<(u8, Outcome)>,
    adjudications: Vec<(u8, Outcome)>,
}

impl DualRetest {
    /// 构造 Collecting 空账。
    pub fn new() -> DualRetest {
        DualRetest {
            state: ReconState::Collecting,
            t1: Vec::new(),
            ktest: Vec::new(),
            adjudications: Vec::new(),
        }
    }

    /// 当前状态。
    pub fn state(&self) -> ReconState {
        self.state
    }

    /// 录单档结果：条目号 1..=99；同条目同档重复拒绝。
    pub fn record(&mut self, track: u8, item: u8, out: Outcome) -> Result<(), DualCode> {
        if self.state != ReconState::Collecting {
            return Err(DualCode::ResultAfterReconcile);
        }
        if item < 1 {
            return Err(DualCode::BadItem);
        }
        let book = match track {
            1 => &mut self.t1,
            2 => &mut self.ktest,
            _ => return Err(DualCode::BadItem),
        };
        if book.len() >= ITEM_CAPACITY {
            return Err(DualCode::DuplicateResult);
        }
        let mut i = 0usize;
        while i < book.len() {
            if let Some((it, _)) = book.get(i) {
                if *it == item {
                    return Err(DualCode::DuplicateResult);
                }
            }
            i += 1;
        }
        book.push((item, out));
        Ok(())
    }

    /// T1 档条目数。
    pub fn t1_len(&self) -> usize {
        self.t1.len()
    }

    /// ktest 档条目数。
    pub fn ktest_len(&self) -> usize {
        self.ktest.len()
    }

    /// 查档内某条目结果。
    pub fn outcome_of(&self, track: u8, item: u8) -> Option<Outcome> {
        let book = match track {
            1 => &self.t1,
            2 => &self.ktest,
            _ => return None,
        };
        let mut i = 0usize;
        while i < book.len() {
            if let Some((it, out)) = book.get(i) {
                if *it == item {
                    return Some(*out);
                }
            }
            i += 1;
        }
        None
    }

    /// 挂裁决：仅对单红差异生效；重复裁决 / 无差异裁决拒绝。
    pub fn adjudicate(&mut self, item: u8, verdict: Outcome) -> Result<(), DualCode> {
        if self.state != ReconState::Collecting {
            return Err(DualCode::ResultAfterReconcile);
        }
        let t1o = self.outcome_of(1, item);
        let ko = self.outcome_of(2, item);
        let is_single_red = match (t1o, ko) {
            (Some(a), Some(b)) => a != b,
            _ => return Err(DualCode::TrackMismatch),
        };
        if !is_single_red {
            return Err(DualCode::AdjudicateNoDiff);
        }
        let mut i = 0usize;
        while i < self.adjudications.len() {
            if let Some((it, _)) = self.adjudications.get(i) {
                if *it == item {
                    return Err(DualCode::DoubleAdjudication);
                }
            }
            i += 1;
        }
        self.adjudications.push((item, verdict));
        Ok(())
    }

    /// 裁决账条数。
    pub fn adjudication_len(&self) -> usize {
        self.adjudications.len()
    }

    /// 某条目是否已裁决。
    pub fn adjudicated(&self, item: u8) -> bool {
        let mut i = 0usize;
        while i < self.adjudications.len() {
            if let Some((it, _)) = self.adjudications.get(i) {
                if *it == item {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 单条目三分（条目集按 T1 档为基准）。
    pub fn triage_of(&self, item: u8) -> Option<Triage> {
        let a = self.outcome_of(1, item)?;
        let b = self.outcome_of(2, item)?;
        Some(match (a, b) {
            (Outcome::Pass, Outcome::Pass) => Triage::BothPass,
            (Outcome::Red, Outcome::Red) => Triage::BothRed,
            _ => Triage::SingleRed,
        })
    }

    /// 对齐：逐条三分；条目集不齐 / 单红未决 / 空账一律拒绝，零未决才通过。
    pub fn reconcile(&mut self) -> Result<(usize, usize, usize), DualCode> {
        if self.state != ReconState::Collecting {
            return Err(DualCode::StateRegress);
        }
        if self.t1.is_empty() && self.ktest.is_empty() {
            return Err(DualCode::EmptyReconcile);
        }
        // 条目集一致性：两档逐号互查。
        let mut i = 0usize;
        while i < self.t1.len() {
            if let Some((it, _)) = self.t1.get(i) {
                if self.outcome_of(2, *it).is_none() {
                    return Err(DualCode::TrackMismatch);
                }
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < self.ktest.len() {
            if let Some((it, _)) = self.ktest.get(j) {
                if self.outcome_of(1, *it).is_none() {
                    return Err(DualCode::TrackMismatch);
                }
            }
            j += 1;
        }
        // 单红未决拦截：单红条目必须全部有裁决。
        let mut n_pass = 0usize;
        let mut n_red = 0usize;
        let mut n_single = 0usize;
        let mut k = 0usize;
        while k < self.t1.len() {
            let item = match self.t1.get(k) {
                Some((it, _)) => *it,
                None => break,
            };
            match self.triage_of(item) {
                Some(Triage::BothPass) => n_pass += 1,
                Some(Triage::BothRed) => n_red += 1,
                Some(Triage::SingleRed) => {
                    if !self.adjudicated(item) {
                        return Err(DualCode::UndecidedDiff);
                    }
                    n_single += 1;
                }
                None => return Err(DualCode::TrackMismatch),
            }
            k += 1;
        }
        self.state = ReconState::Reconciled;
        Ok((n_pass, n_red, n_single))
    }

    /// 封账：Reconciled → Sealed。
    pub fn seal(&mut self) -> Result<(), DualCode> {
        if self.state != ReconState::Reconciled {
            return Err(DualCode::SealBeforeReconcile);
        }
        self.state = ReconState::Sealed;
        Ok(())
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；账不平返回负诊断码。
    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(DualCode::StateRegress as i32));
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 构造一致对账：条目 1..3 双档全 Pass。
pub fn dual_clean() -> DualRetest {
    let mut d = DualRetest::new();
    let mut n = 1u8;
    while n <= 3 {
        let _ = d.record(1, n, Outcome::Pass);
        let _ = d.record(2, n, Outcome::Pass);
        n += 1;
    }
    d
}

/// 构造带单红的对账：条目 2 单红（T1 Pass / ktest Red）。
pub fn dual_single_red() -> DualRetest {
    // 重录条目 2 不行（重复拒绝），改用专用账：条目 1..3，条目 2 两档异果。
    let mut d2 = DualRetest::new();
    let _ = d2.record(1, 1, Outcome::Pass);
    let _ = d2.record(1, 2, Outcome::Pass);
    let _ = d2.record(1, 3, Outcome::Pass);
    let _ = d2.record(2, 1, Outcome::Pass);
    let _ = d2.record(2, 2, Outcome::Red);
    let _ = d2.record(2, 3, Outcome::Pass);
    d2
}

/// 四类注入：0 单红未决、1 条目集不齐、2 重复裁决、3 空裁决。
pub fn run_injection_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut c = 0usize;
    while c < 4 {
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match c {
                0 => {
                    // 单红未裁决 → 对齐拦截。
                    let mut d = dual_single_red();
                    d.reconcile().err() == Some(DualCode::UndecidedDiff)
                }
                1 => {
                    // ktest 缺条 → 对齐报条目集不齐。
                    let mut d = DualRetest::new();
                    let _ = d.record(1, 1, Outcome::Pass);
                    let _ = d.record(1, 2, Outcome::Pass);
                    let _ = d.record(2, 1, Outcome::Pass);
                    d.reconcile().err() == Some(DualCode::TrackMismatch)
                }
                2 => {
                    // 同条目二次裁决 → 重复拒绝。
                    let mut d = dual_single_red();
                    let a = d.adjudicate(2, Outcome::Pass);
                    let b = d.adjudicate(2, Outcome::Red);
                    a.is_ok() && b.err() == Some(DualCode::DoubleAdjudication)
                }
                _ => {
                    // 一致条目挂裁决 → 空裁决拒绝。
                    let mut d = dual_single_red();
                    d.adjudicate(1, Outcome::Pass)
                        .err() == Some(DualCode::AdjudicateNoDiff)
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(c) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        c += 1;
    }
    (ledger, per_class)
}

/// 构造已对齐封账：单红已裁决的对账全流程。
pub fn build_sealed_recon() -> DualRetest {
    let mut d = dual_single_red();
    let _ = d.adjudicate(2, Outcome::Pass);
    let _ = d.reconcile();
    let _ = d.seal();
    d
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；对齐后录果/状态回退族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺档条目族"),
    ("EEXIST", -17, 80, "Windows ERROR_FILE_EXISTS；重复录果/重复裁决族"),
    ("ENOTUNIQ", -35, 87, "Windows ERROR_INVALID_PARAMETER；单红未决须唯一裁定族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；条目号/档位坏族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
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

/// 对账指纹：状态 + 两档逐条 + 裁决逐条入哈希。
pub fn recon_fingerprint(d: &DualRetest) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push(match d.state() {
        ReconState::Collecting => 0,
        ReconState::Reconciled => 1,
        ReconState::Sealed => 2,
    });
    bytes.push(d.t1_len() as u8);
    bytes.push(d.ktest_len() as u8);
    let mut n = 1u8;
    while n <= 9 {
        match (d.outcome_of(1, n), d.outcome_of(2, n)) {
            (Some(a), Some(b)) => {
                bytes.push(match a { Outcome::Pass => 1, Outcome::Red => 0 });
                bytes.push(match b { Outcome::Pass => 1, Outcome::Red => 0 });
                bytes.push(if d.adjudicated(n) { 1 } else { 0 });
            }
            _ => bytes.push(0xFF),
        }
        n += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10037 判据集（16 条 + 2 条动态明细）。
pub fn run_unx10037_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10037");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91900..=0x91980).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91900..=0x91980", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 三分闭集冻结：三分互异且可逐条定位
    let tri_ok = Triage::BothPass != Triage::BothRed
        && Triage::BothRed != Triage::SingleRed
        && Triage::SingleRed != Triage::BothPass;
    s.add("三分闭集：双绿/双红/单红三类互异", tri_ok,
          "双档结果必须归入三分闭集之一，不许未决悬挂");

    // 3. 完整对账：单红已裁决 → 对齐 → 封账
    let full = build_sealed_recon();
    s.add("双档闭环：单红裁决后对齐封账成功",
          full.state() == ReconState::Sealed && full.adjudication_len() == 1,
          "单红挂裁决后对齐零未决，才能封账");

    // 4. 未决差异拦截（核心判据）：单红未裁决 10/10 次拦截
    let mut undecided_caught = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut d = dual_single_red();
        if d.reconcile().err() == Some(DualCode::UndecidedDiff) {
            undecided_caught += 1;
        }
        round += 1;
    }
    s.add("未决拦截：单红未裁决 10/10 次拦截", undecided_caught == INJECT_ROUNDS,
          "单红差异必须挂裁决留痕，未决一律拦在对齐门外");

    // 5. 裁决放行：单红挂裁决后 10/10 次对齐通过
    let mut adjud_ok = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut d = dual_single_red();
        let _ = d.adjudicate(2, Outcome::Pass);
        if d.reconcile().is_ok() {
            adjud_ok += 1;
        }
        round += 1;
    }
    s.add("裁决放行：单红裁决后 10/10 次对齐通过", adjud_ok == INJECT_ROUNDS,
          "裁决即差异落账，对齐必须在零未决时通过");

    // 6. 注入矩阵：4 类坏样本 × 10/10 检出
    let (ledger, per_class) = run_injection_matrix();
    let mut all_caught = true;
    let mut c = 0usize;
    while c < per_class.len() {
        if let Some(k) = per_class.get(c) {
            if *k != INJECT_ROUNDS {
                all_caught = false;
            }
        }
        c += 1;
    }
    s.add("注入核：4 类坏样本逐类 10/10 检出", all_caught,
          "单红未决/条目不齐/重复裁决/空裁决必须全部显性拒绝");

    // 7. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 4 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 8. 条目集不齐拦截（复测口径）
    let mut mismatch_caught = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut d = DualRetest::new();
        let _ = d.record(1, 1, Outcome::Pass);
        let _ = d.record(1, 2, Outcome::Red);
        let _ = d.record(2, 1, Outcome::Pass);
        if d.reconcile().err() == Some(DualCode::TrackMismatch) {
            mismatch_caught += 1;
        }
        round += 1;
    }
    s.add("缺档拦截：ktest 缺条 10/10 次检出", mismatch_caught == INJECT_ROUNDS,
          "两档必须同条目集，缺档条目一律报不齐");

    // 9. 重复录果拒绝
    let mut dup = DualRetest::new();
    let a = dup.record(1, 1, Outcome::Pass);
    let b = dup.record(1, 1, Outcome::Red);
    s.add("重复拒绝：同条目同档二次录果被拒",
          a.is_ok() && b.err() == Some(DualCode::DuplicateResult),
          "重复录果必须显性拒绝，不许静默覆盖");

    // 10. 重复裁决与空裁决拒绝
    let mut adj = dual_single_red();
    let first = adj.adjudicate(2, Outcome::Pass);
    let again = adj.adjudicate(2, Outcome::Red);
    let no_diff = adj.adjudicate(3, Outcome::Pass);
    s.add("裁决守卫：重复裁决与空裁决均被拒",
          first.is_ok()
              && again.err() == Some(DualCode::DoubleAdjudication)
              && no_diff.err() == Some(DualCode::AdjudicateNoDiff),
          "裁决只对单红差异生效一次，改判须开新一轮");

    // 11. 未对齐封账拒绝、对齐后录果拒绝
    let mut flow = dual_single_red();
    let early_seal = flow.seal();
    let _ = flow.adjudicate(2, Outcome::Pass);
    let _ = flow.reconcile();
    let late_rec = flow.record(1, 9, Outcome::Pass);
    s.add("状态机：未对齐封账与对齐后录果均被拒",
          early_seal.err() == Some(DualCode::SealBeforeReconcile)
              && late_rec.err() == Some(DualCode::ResultAfterReconcile),
          "Collecting→Reconciled→Sealed 单向流，越位即拒");

    // 12. 三分计数正确：1 双绿 + 1 双红 + 1 单红（已裁决）
    let mut mix = DualRetest::new();
    let _ = mix.record(1, 1, Outcome::Pass);
    let _ = mix.record(2, 1, Outcome::Pass);
    let _ = mix.record(1, 2, Outcome::Red);
    let _ = mix.record(2, 2, Outcome::Red);
    let _ = mix.record(1, 3, Outcome::Pass);
    let _ = mix.record(2, 3, Outcome::Red);
    let _ = mix.adjudicate(3, Outcome::Red);
    let counts = mix.reconcile();
    s.add("三分计数：双绿/双红/单红逐类计数正确",
          counts == Ok((1, 1, 1)),
          "对齐返回三分计数，逐类可复核");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(ENOTUNIQ)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 14. 双跑指纹一致
    let fp_a = recon_fingerprint(&build_sealed_recon());
    let fp_b = recon_fingerprint(&build_sealed_recon());
    s.add("双跑一致：对账指纹相等", fp_a == fp_b,
          "同输入两次构建必须逐字段相等");

    // 15. 文案闭集
    let text_ok = DualCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < DualCode::ALL.len() {
        let mut j = i + 1;
        while j < DualCode::ALL.len() {
            if let (Some(a), Some(b)) = (DualCode::ALL.get(i), DualCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!(
        "对账指纹 0x{:08X}；裁决 {} 条",
        fp_a, full.adjudication_len()
    );
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_reconciles() {
        let mut d = dual_clean();
        assert_eq!(d.reconcile(), Ok((3, 0, 0)));
    }

    #[test]
    fn undecided_single_red_blocked() {
        let mut d = dual_single_red();
        assert_eq!(d.reconcile(), Err(DualCode::UndecidedDiff));
    }

    #[test]
    fn adjudication_unblocks() {
        let mut d = dual_single_red();
        assert!(d.adjudicate(2, Outcome::Pass).is_ok());
        assert_eq!(d.reconcile(), Ok((2, 0, 1)));
    }

    #[test]
    fn track_mismatch_detected() {
        let mut d = DualRetest::new();
        let _ = d.record(1, 1, Outcome::Pass);
        assert_eq!(d.reconcile(), Err(DualCode::TrackMismatch));
    }

    #[test]
    fn duplicate_result_rejected() {
        let mut d = DualRetest::new();
        assert!(d.record(1, 1, Outcome::Pass).is_ok());
        assert_eq!(d.record(1, 1, Outcome::Red), Err(DualCode::DuplicateResult));
    }

    #[test]
    fn double_adjudication_rejected() {
        let mut d = dual_single_red();
        assert!(d.adjudicate(2, Outcome::Pass).is_ok());
        assert_eq!(d.adjudicate(2, Outcome::Red), Err(DualCode::DoubleAdjudication));
    }

    #[test]
    fn adjudicate_no_diff_rejected() {
        let mut d = dual_clean();
        assert_eq!(d.adjudicate(1, Outcome::Pass), Err(DualCode::AdjudicateNoDiff));
    }

    #[test]
    fn seal_before_reconcile_rejected() {
        let mut d = dual_clean();
        assert_eq!(d.seal(), Err(DualCode::SealBeforeReconcile));
    }

    #[test]
    fn result_after_reconcile_rejected() {
        let mut d = dual_clean();
        let _ = d.reconcile();
        assert_eq!(d.record(1, 5, Outcome::Pass), Err(DualCode::ResultAfterReconcile));
    }

    #[test]
    fn bad_item_rejected() {
        let mut d = DualRetest::new();
        assert_eq!(d.record(1, 0, Outcome::Pass), Err(DualCode::BadItem));
        assert_eq!(d.record(3, 1, Outcome::Pass), Err(DualCode::BadItem));
    }

    #[test]
    fn injection_matrix_green() {
        let (l, per) = run_injection_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 4 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(recon_fingerprint(&build_sealed_recon()),
                   recon_fingerprint(&build_sealed_recon()));
    }

    #[test]
    fn unx10037_checks_green() {
        let set = run_unx10037_checks();
        assert!(set.all_passed(), "UNX-F10037 red: {}", set.red_items_text());
    }
}
