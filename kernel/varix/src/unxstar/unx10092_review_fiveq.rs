//! UNX-F10092 双判据评审清单（评审五问）· 0x926C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10093-J1 预期差对接核（J2 红走预期差登记 10/10 次），绕登记 10/10 次拦截
//!
//! SEG_BASE = 0x926C0（本卡独占 0x40 窗口，下一卡 0x92700）；
//! 诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 注册表：评审五问闭集注册表（Q1..Q5 逐问注册，问号与文案绑定，
//!      闭集外问号一律拒绝），问题注册是作答的前置。
//!   ② 状态机：评审态机 未评审→部分作答→五问全→裁决通过，缺问接受
//!      / 锁后作答均显性拒绝。
//!   ③ 账本：评审账本 ReviewLedger，逐条目逐问留痕（条目号+问号+裁决），
//!      五问全满且全过方可裁决通过。
//!   ④ 注入核：缺问接受 / 重复作答 / 未知条目 / 锁后作答 四类坏样本
//!      逐类 10/10 次拒绝，注入账 注入=检出+逃逸，逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10092 独占 0x40 窗口，下一卡 0x92700）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x926C0;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 评审账本容量上限（条目数）。
pub const REVIEW_CAP: usize = 64;

/// 评审问题数（五问闭集固定）。
pub const QUESTION_COUNT: usize = 5;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FiveQCode {
    /// 未知问号（五问闭集外）
    UnknownQuestion,
    /// 缺问作答（问槽为空时引用）
    MissingAnswer,
    /// 同问重复作答
    DupAnswer,
    /// 未知条目
    UnknownEntry,
    /// 缺问强行裁决
    PrematureAccept,
    /// 评审已锁定后作答
    LockedReview,
    /// 裁决前未过问全数通过
    FailedGate,
    /// 条目号为零（非法键）
    ZeroEntry,
    /// 拒绝后账本被改动
    RejectedTrace,
    /// 问号标签往返不一致
    QuestionRoundTrip,
    /// 裁决成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
}

impl FiveQCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            FiveQCode::UnknownQuestion => "未知问号被拒：评审只认 Q1..Q5 五问闭集；\
                请改用注册表内的问号",
            FiveQCode::MissingAnswer => "缺问作答被拒：引用裁决前该问必须已有留痕；\
                请先逐问作答",
            FiveQCode::DupAnswer => "重复作答被拒：同条目同问只能留痕一次；\
                改判请走重开评审而非覆盖",
            FiveQCode::UnknownEntry => "未知条目：账本中无此条目号；请先注册条目再评审",
            FiveQCode::PrematureAccept => "缺问裁决被拒：五问全留痕前不得裁决通过；\
                缺问评审必须逐次拦截",
            FiveQCode::LockedReview => "锁定后被拒：评审裁决通过后账面只读；\
                追加作答请走重测新期",
            FiveQCode::FailedGate => "裁决门未过：存在未通过的问，不得整体通过；\
                请先处置红问",
            FiveQCode::ZeroEntry => "条目号为零：零号是保留键，禁止入账；请换用正数条目号",
            FiveQCode::RejectedTrace => "拒绝留痕被拒：被拒操作不得改动任何已留痕问；\
                请排查拒绝路径是否误提交",
            FiveQCode::QuestionRoundTrip => "问号往返不一致：label 再解析必须回到原问；\
                请检查闭集文案与解析器是否同步",
            FiveQCode::EmptySummary => "空成文被拒：裁决小结不得为空串；请检查条目是否在账",
            FiveQCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [FiveQCode; 12] = [
        FiveQCode::UnknownQuestion,
        FiveQCode::MissingAnswer,
        FiveQCode::DupAnswer,
        FiveQCode::UnknownEntry,
        FiveQCode::PrematureAccept,
        FiveQCode::LockedReview,
        FiveQCode::FailedGate,
        FiveQCode::ZeroEntry,
        FiveQCode::RejectedTrace,
        FiveQCode::QuestionRoundTrip,
        FiveQCode::EmptySummary,
        FiveQCode::SegmentViolation,
    ];
}

// ── 评审五问注册表 ────────────────────────────────────────────────────
/// 五问闭集（注册表固定序，问号 1..=5）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuestionKind {
    /// Q1：正判据达成路径是否可复测
    Q1ReproJ1,
    /// Q2：反判据误用路径是否可复现
    Q2ReproJ2,
    /// Q3：双档证据（T1/ktest）是否齐备
    Q3DualEvidence,
    /// Q4：边界与守恒是否逐项处理
    Q4Boundary,
    /// Q5：红向是否挂预期差或豁免依据
    Q5ExDiff,
}

impl QuestionKind {
    /// 问号（1..=5）。
    pub fn no(self) -> u8 {
        match self {
            QuestionKind::Q1ReproJ1 => 1,
            QuestionKind::Q2ReproJ2 => 2,
            QuestionKind::Q3DualEvidence => 3,
            QuestionKind::Q4Boundary => 4,
            QuestionKind::Q5ExDiff => 5,
        }
    }

    /// 问面文案（成文用，非空）。
    pub fn label(self) -> &'static str {
        match self {
            QuestionKind::Q1ReproJ1 => "正判据可复测",
            QuestionKind::Q2ReproJ2 => "反判据可复现",
            QuestionKind::Q3DualEvidence => "双档证据齐备",
            QuestionKind::Q4Boundary => "边界守恒逐项",
            QuestionKind::Q5ExDiff => "红向预期差挂账",
        }
    }

    /// 闭集全量（固定序）。
    pub const ALL: [QuestionKind; 5] = [
        QuestionKind::Q1ReproJ1,
        QuestionKind::Q2ReproJ2,
        QuestionKind::Q3DualEvidence,
        QuestionKind::Q4Boundary,
        QuestionKind::Q5ExDiff,
    ];

    /// 按问号反查（1..=5），闭集外返回 None。
    pub fn by_no(n: u8) -> Option<QuestionKind> {
        match n {
            1 => Some(QuestionKind::Q1ReproJ1),
            2 => Some(QuestionKind::Q2ReproJ2),
            3 => Some(QuestionKind::Q3DualEvidence),
            4 => Some(QuestionKind::Q4Boundary),
            5 => Some(QuestionKind::Q5ExDiff),
            _ => None,
        }
    }

    /// 问号解析：形制 `Q<n>`，n 必须落在 1..=5；其余（含 Q0/Q6/裸数字）拒绝。
    pub fn parse_question(tag: &str) -> Result<QuestionKind, FiveQCode> {
        let rest = match tag.strip_prefix('Q') {
            Some(r) => r,
            None => return Err(FiveQCode::UnknownQuestion),
        };
        if rest.len() != 1 {
            return Err(FiveQCode::UnknownQuestion);
        }
        let b = match rest.as_bytes().first() {
            Some(b) => *b,
            None => return Err(FiveQCode::UnknownQuestion),
        };
        if !b.is_ascii_digit() {
            return Err(FiveQCode::UnknownQuestion);
        }
        match QuestionKind::by_no(b - b'0') {
            Some(q) => Ok(q),
            None => Err(FiveQCode::UnknownQuestion),
        }
    }
}

/// 裁决二值：通过 / 未过。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 该问通过
    Pass,
    /// 该问未过
    Fail,
}

impl Verdict {
    /// 中文裁决名。
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Pass => "过",
            Verdict::Fail => "红",
        }
    }
}

// ── 评审态机 ──────────────────────────────────────────────────────────
/// 条目评审态：Unreviewed → Partial → Complete → Accepted。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReviewStage {
    /// 未评审：零问留痕
    Unreviewed,
    /// 部分作答：1..=4 问留痕
    Partial,
    /// 五问全：留痕齐但未裁决
    Complete,
    /// 裁决通过：账面锁定只读
    Accepted,
}

impl ReviewStage {
    /// 态序（0..=3）。
    pub fn rank(self) -> u8 {
        match self {
            ReviewStage::Unreviewed => 0,
            ReviewStage::Partial => 1,
            ReviewStage::Complete => 2,
            ReviewStage::Accepted => 3,
        }
    }

    /// 由留痕数推导（0/1..=4/5）。
    pub fn from_count(n: usize) -> ReviewStage {
        if n == 0 {
            ReviewStage::Unreviewed
        } else if n >= QUESTION_COUNT {
            ReviewStage::Complete
        } else {
            ReviewStage::Partial
        }
    }
}

// ── 评审账本 ──────────────────────────────────────────────────────────
/// 单条目评审记录：五问留痕（None = 未作答）+ 锁定位。
#[derive(Clone, Copy, Debug)]
pub struct ReviewRecord {
    /// 条目号
    pub entry: u32,
    /// 五问裁决槽（下标 = 问号-1）
    pub slots: [Option<Verdict>; QUESTION_COUNT],
    /// 裁决锁定
    pub locked: bool,
}

/// 评审账本：逐条目逐问留痕；缺问裁决 / 锁后作答 / 重复作答逐次拒绝。
#[derive(Clone, Debug)]
pub struct ReviewLedger {
    records: Vec<ReviewRecord>,
    cap: usize,
}

impl ReviewLedger {
    /// 空账构造（容量 REVIEW_CAP）。
    pub fn new() -> ReviewLedger {
        ReviewLedger { records: Vec::new(), cap: REVIEW_CAP }
    }

    /// 注册条目：零号拒绝、重复拒绝；注册即 Unreviewed。
    pub fn register(&mut self, entry: u32) -> Result<(), FiveQCode> {
        if entry == 0 {
            return Err(FiveQCode::ZeroEntry);
        }
        if self.records.len() >= self.cap {
            return Err(FiveQCode::RejectedTrace);
        }
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.entry == entry {
                    return Err(FiveQCode::UnknownEntry);
                }
            }
            i += 1;
        }
        self.records.push(ReviewRecord {
            entry,
            slots: [None; QUESTION_COUNT],
            locked: false,
        });
        Ok(())
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 作答：问号必须解析自五问闭集；重复作答 / 锁后作答拒绝。
    pub fn answer(
        &mut self,
        entry: u32,
        tag: &str,
        v: Verdict,
    ) -> Result<(), FiveQCode> {
        let q = QuestionKind::parse_question(tag)?;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get_mut(i) {
                if r.entry == entry {
                    if r.locked {
                        return Err(FiveQCode::LockedReview);
                    }
                    let slot = match q.no() {
                        n if n >= 1 && (n as usize) <= QUESTION_COUNT => (n - 1) as usize,
                        _ => return Err(FiveQCode::UnknownQuestion),
                    };
                    if slot >= QUESTION_COUNT {
                        return Err(FiveQCode::UnknownQuestion);
                    }
                    if r.slots[slot].is_some() {
                        return Err(FiveQCode::DupAnswer);
                    }
                    r.slots[slot] = Some(v);
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(FiveQCode::UnknownEntry)
    }

    /// 留痕数（指定条目）。
    pub fn answered(&self, entry: u32) -> Result<usize, FiveQCode> {
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.entry == entry {
                    let mut n = 0usize;
                    let mut k = 0usize;
                    while k < QUESTION_COUNT {
                        if r.slots[k].is_some() {
                            n += 1;
                        }
                        k += 1;
                    }
                    return Ok(n);
                }
            }
            i += 1;
        }
        Err(FiveQCode::UnknownEntry)
    }

    /// 裁决通过：五问全留痕且全过才接受并锁定；否则逐因拒绝。
    pub fn accept(&mut self, entry: u32) -> Result<(), FiveQCode> {
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get_mut(i) {
                if r.entry == entry {
                    let mut n = 0usize;
                    let mut all_pass = true;
                    let mut k = 0usize;
                    while k < QUESTION_COUNT {
                        // 留痕数按“已作答”计（过/红都算留痕）；红问单独失守全过门。
                        if r.slots[k].is_some() {
                            n += 1;
                            if r.slots[k] == Some(Verdict::Fail) {
                                all_pass = false;
                            }
                        }
                        k += 1;
                    }
                    if n < QUESTION_COUNT {
                        return Err(FiveQCode::PrematureAccept);
                    }
                    if !all_pass {
                        return Err(FiveQCode::FailedGate);
                    }
                    r.locked = true;
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(FiveQCode::UnknownEntry)
    }

    /// 条目评审态读取。
    pub fn stage(&self, entry: u32) -> Result<ReviewStage, FiveQCode> {
        let n = self.answered(entry)?;
        if self.is_locked(entry) {
            return Ok(ReviewStage::Accepted);
        }
        Ok(ReviewStage::from_count(n))
    }

    /// 锁定位读取（账外条目返回 false）。
    pub fn is_locked(&self, entry: u32) -> bool {
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.entry == entry {
                    return r.locked;
                }
            }
            i += 1;
        }
        false
    }

    /// 全账审计：所有已注册条目必须五问全留痕（缺问评审 = 0 条）。
    pub fn all_complete(&self) -> bool {
        let mut i = 0usize;
        while i < self.records.len() {
            let entry = match self.records.get(i) {
                Some(r) => r.entry,
                None => return false,
            };
            match self.answered(entry) {
                Ok(n) if n == QUESTION_COUNT => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

// ── 注入核：坏评审样本矩阵 ────────────────────────────────────────────
/// 四类坏评审样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadReview {
    /// 缺问强行裁决
    PrematureAccept,
    /// 同问重复作答
    DupAnswer,
    /// 未知条目作答
    UnknownEntry,
    /// 锁后作答
    LockedReview,
}

impl BadReview {
    /// 闭集全量。
    pub const ALL: [BadReview; 4] = [
        BadReview::PrematureAccept,
        BadReview::DupAnswer,
        BadReview::UnknownEntry,
        BadReview::LockedReview,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadReview::PrematureAccept => "缺问强行裁决",
            BadReview::DupAnswer => "同问重复作答",
            BadReview::UnknownEntry => "账外条目作答",
            BadReview::LockedReview => "锁定后追加作答",
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
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

    /// 记一次注入结果；账失守恒返回 RejectedTrace。
    pub fn record(&mut self, caught: bool) -> Result<(), FiveQCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(FiveQCode::RejectedTrace);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏评审注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
pub fn run_bad_review_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadReview::ALL.len() {
        let case = match BadReview::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadReview::PrematureAccept => {
                    let mut l = ReviewLedger::new();
                    let ok1 = l.register(1).is_ok();
                    ok1 && l.answer(1, "Q1", Verdict::Pass).is_ok()
                        && l.accept(1).err() == Some(FiveQCode::PrematureAccept)
                }
                BadReview::DupAnswer => {
                    let mut l = ReviewLedger::new();
                    let ok1 = l.register(2).is_ok();
                    ok1 && l.answer(2, "Q2", Verdict::Pass).is_ok()
                        && l.answer(2, "Q2", Verdict::Fail).err() == Some(FiveQCode::DupAnswer)
                }
                BadReview::UnknownEntry => {
                    let mut l = ReviewLedger::new();
                    let ok1 = l.register(3).is_ok();
                    ok1 && l.answer(99, "Q1", Verdict::Pass).err() == Some(FiveQCode::UnknownEntry)
                }
                BadReview::LockedReview => {
                    let mut l = ReviewLedger::new();
                    let ok1 = l.register(4).is_ok();
                    let mut k = 0usize;
                    while k < QUESTION_COUNT {
                        let tag = match QuestionKind::by_no((k + 1) as u8) {
                            Some(q) => match q.no() {
                                1 => "Q1",
                                2 => "Q2",
                                3 => "Q3",
                                4 => "Q4",
                                _ => "Q5",
                            },
                            None => break,
                        };
                        let _ = l.answer(4, tag, Verdict::Pass);
                        k += 1;
                    }
                    ok1 && l.accept(4).is_ok()
                        && l.answer(4, "Q1", Verdict::Pass).err() == Some(FiveQCode::LockedReview)
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── 裁决成文（纯函数） ────────────────────────────────────────────────
/// 裁决成文：「E<n> 答=<k>/5;裁决=<通过|未过>」；账外先拒。
pub fn compose_review_summary(l: &ReviewLedger, entry: u32) -> Result<String, FiveQCode> {
    if l.is_empty() {
        return Err(FiveQCode::EmptySummary);
    }
    let n = l.answered(entry)?;
    let verdict = if n == QUESTION_COUNT {
        // 全留痕才看全过与否；缺问一律未过。
        let mut all_pass = true;
        let mut i = 0usize;
        while i < l.records.len() {
            if let Some(r) = l.records.get(i) {
                if r.entry == entry {
                    let mut k = 0usize;
                    while k < QUESTION_COUNT {
                        if r.slots[k] == Some(Verdict::Fail) {
                            all_pass = false;
                        }
                        k += 1;
                    }
                }
            }
            i += 1;
        }
        if all_pass { "通过" } else { "未过" }
    } else {
        "未过"
    };
    let out = format!("E{} 答={}/{};裁决={}", entry, n, QUESTION_COUNT, verdict);
    if out.is_empty() {
        return Err(FiveQCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；未知问号/零号条目族"),
    ("ENODATA", -61, 232, "Windows ERROR_NO_DATA；缺问/未作答族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复作答族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；锁定后作答族"),
    ("EBUSY", -16, 33, "Windows ERROR_LOCK_VIOLATION；裁决未齐仍持有评审族"),
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

/// 规范语料双跑指纹：三条目注册 → 逐问留痕 → 两条裁决 → 逐条成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = ReviewLedger::new();
    let _ = l.register(301);
    let _ = l.register(302);
    let _ = l.register(303);
    let mut k = 0usize;
    while k < QUESTION_COUNT {
        if let Some(q) = QuestionKind::ALL.get(k) {
            let tag = match q.no() {
                1 => "Q1",
                2 => "Q2",
                3 => "Q3",
                4 => "Q4",
                _ => "Q5",
            };
            let _ = l.answer(301, tag, Verdict::Pass);
            let _ = l.answer(302, tag, Verdict::Pass);
            // 303 留 Q5 缺问。
            if k < QUESTION_COUNT - 1 {
                let _ = l.answer(303, tag, Verdict::Pass);
            }
        }
        k += 1;
    }
    let _ = l.accept(301);
    let _ = l.accept(302);
    let _ = l.accept(303);
    let mut bytes: Vec<u8> = Vec::new();
    let mut e = 0u32;
    while e < 3 {
        match compose_review_summary(&l, 301 + e) {
            Ok(s) => {
                bytes.extend_from_slice(s.as_bytes());
                bytes.push(b';');
            }
            Err(_) => {}
        }
        e += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10092 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10092_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10092");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x926C0..=0x926FF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 五问注册表：问号与文案一一绑定且非空
    let reg_ok = QuestionKind::ALL.len() == QUESTION_COUNT
        && QuestionKind::ALL.iter().all(|q| q.no() >= 1 && !q.label().is_empty());
    s.add("五问注册表：Q1..Q5 闭集注册且问面文案全非空", reg_ok,
          "问题注册是作答前置，问号与文案必须绑定");

    // 3. 问号解析器：Q1..Q5 全通过，闭集外 10/10 拒绝
    let parse_ok = QuestionKind::parse_question("Q1") == Ok(QuestionKind::Q1ReproJ1)
        && QuestionKind::parse_question("Q5") == Ok(QuestionKind::Q5ExDiff);
    let mut amb_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if QuestionKind::parse_question("Q0").is_ok()
            || QuestionKind::parse_question("Q6").is_ok()
            || QuestionKind::parse_question("q1").is_ok()
            || QuestionKind::parse_question("").is_ok()
            || QuestionKind::parse_question("QQ").is_ok()
        {
            amb_ok = false;
        }
        r += 1;
    }
    s.add("问号解析器：Q1..Q5 可解析且闭集外 10/10 拒绝", parse_ok && amb_ok,
          "问号歧义必须逐次显性拒绝，不许静默归并到相邻问");

    // 4. 条目注册：零号与重复拒绝
    let mut l = ReviewLedger::new();
    let reg_all = l.register(11).is_ok() && l.register(12).is_ok() && l.register(13).is_ok();
    s.add("条目注册：三条入账、零号与重复显性拒绝",
          reg_all && l.len() == 3
              && l.register(0).err() == Some(FiveQCode::ZeroEntry)
              && l.register(11).is_err(),
          "零号是保留键；重复注册是账目事故，必须拒绝");

    // 5. 五问留痕：逐条目逐问作答到满
    let mut fill_ok = true;
    let mut k = 0usize;
    while k < QUESTION_COUNT {
        if let Some(q) = QuestionKind::ALL.get(k) {
            let tag = match q.no() {
                1 => "Q1",
                2 => "Q2",
                3 => "Q3",
                4 => "Q4",
                _ => "Q5",
            };
            if l.answer(11, tag, Verdict::Pass).is_err() {
                fill_ok = false;
            }
        }
        k += 1;
    }
    s.add("五问留痕：单条目五问逐问作答到满", fill_ok && l.answered(11) == Ok(5),
          "逐问留痕是评审证据链，缺一问即缺问评审");

    // 6. 裁决通过：五问全过才锁定
    let acc_ok = l.accept(11).is_ok() && l.is_locked(11)
        && l.stage(11) == Ok(ReviewStage::Accepted);
    s.add("裁决通过：五问全过接受并锁定", acc_ok,
          "裁决 = 五问全过；接受即锁定只读");

    // 7. 注入核：四类坏评审样本各 10/10 拒绝
    let (inj, per_class) = run_bad_review_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏评审样本各 10/10 拒绝", all_caught,
          "缺问裁决/重复作答/账外作答/锁后作答必须逐类逐次拒绝");

    // 8. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadReview::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 9. 缺问裁决拦截：4/5 问裁决被拒且留痕不变
    let mut l2 = ReviewLedger::new();
    let _ = l2.register(21);
    let mut k2 = 0usize;
    while k2 < QUESTION_COUNT - 1 {
        if let Some(q) = QuestionKind::ALL.get(k2) {
            let tag = match q.no() {
                1 => "Q1",
                2 => "Q2",
                3 => "Q3",
                4 => "Q4",
                _ => "Q5",
            };
            let _ = l2.answer(21, tag, Verdict::Pass);
        }
        k2 += 1;
    }
    let reject_ok = l2.accept(21).err() == Some(FiveQCode::PrematureAccept);
    let trace_ok = l2.answered(21) == Ok(QUESTION_COUNT - 1);
    s.add("缺问拦截：4/5 问裁决被拒且留痕不变", reject_ok && trace_ok,
          "缺问评审必须逐次拦截，被拒裁决不得改动留痕");

    // 10. 红问门：存在未过问不得整体通过
    let mut l3 = ReviewLedger::new();
    let _ = l3.register(31);
    let mut k3 = 0usize;
    while k3 < QUESTION_COUNT {
        if let Some(q) = QuestionKind::ALL.get(k3) {
            let tag = match q.no() {
                1 => "Q1",
                2 => "Q2",
                3 => "Q3",
                4 => "Q4",
                _ => "Q5",
            };
            let v = if k3 == 3 { Verdict::Fail } else { Verdict::Pass };
            let _ = l3.answer(31, tag, v);
        }
        k3 += 1;
    }
    s.add("红问门：五问全留痕但有红问时裁决拒绝", l3.accept(31).err() == Some(FiveQCode::FailedGate),
          "整体通过必须全绿；红问必须先处置再裁决");

    // 11. 态机推导：留痕数推导评审态
    let stage_ok = ReviewStage::from_count(0) == ReviewStage::Unreviewed
        && ReviewStage::from_count(2) == ReviewStage::Partial
        && ReviewStage::from_count(5) == ReviewStage::Complete
        && l.stage(12) == Ok(ReviewStage::Unreviewed);
    s.add("评审态机：留痕数正确推导 未评审/部分/五问全", stage_ok,
          "评审态由留痕数与锁定位唯一推导，不允许旁路置态");

    // 12. 全账审计：缺问条目暴露（l3 五问全留痕含红问，属完整留痕）
    let audit_ok = !l2.all_complete() && l3.all_complete() && !l.all_complete()
        && {
            let mut l4 = ReviewLedger::new();
            let _ = l4.register(41);
            let mut k4 = 0usize;
            while k4 < QUESTION_COUNT {
                if let Some(q) = QuestionKind::ALL.get(k4) {
                    let tag = match q.no() {
                        1 => "Q1",
                        2 => "Q2",
                        3 => "Q3",
                        4 => "Q4",
                        _ => "Q5",
                    };
                    let _ = l4.answer(41, tag, Verdict::Pass);
                }
                k4 += 1;
            }
            l4.all_complete()
        };
    s.add("全账审计：缺问条目暴露、五问全账翻绿", audit_ok,
          "缺问评审 = 0 条是收口前置，审计必须逐条目核对");

    // 13. 成文非空且含三标记 + 双跑一致
    let sum = compose_review_summary(&l, 11);
    let text_ok = match &sum {
        Ok(t) => t.contains("答=5/5") && t.contains("裁决=通过"),
        Err(_) => false,
    };
    s.add("裁决成文：答数与裁决齐备且两次成文逐字节一致",
          text_ok && sum == compose_review_summary(&l, 11),
          "成文必须携带 答/裁决 标记且为纯函数");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(61) && agrees_with_linux(17)
        && agrees_with_linux(1) && agrees_with_linux(16)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < FiveQCode::ALL.len() {
        let mut j = i + 1;
        while j < FiveQCode::ALL.len() {
            if let (Some(a), Some(b)) = (FiveQCode::ALL.get(i), FiveQCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = FiveQCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 问号往返 + 双跑指纹
    let rt_ok = QuestionKind::ALL.iter().all(|q| {
        let tag = match q.no() {
            1 => "Q1",
            2 => "Q2",
            3 => "Q3",
            4 => "Q4",
            _ => "Q5",
        };
        QuestionKind::parse_question(tag) == Ok(*q)
    });
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("往返与指纹：问号标签可回解析且双跑哈希相等", rt_ok && f1 == f2,
          "同语料两次哈希必须逐位一致，隐藏状态即失败");

    // 动态明细（≤8）：注入账快照 + 指纹 + 成文长度
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let sum_len = match &sum { Ok(t) => t.len(), Err(_) => 0 };
    let sum_text = format!("裁决小结字节数 {}", sum_len);
    s.add_dyn("成文快照", sum_len > 0, &sum_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn question_closed_set() {
        assert_eq!(QuestionKind::parse_question("Q3"), Ok(QuestionKind::Q3DualEvidence));
        assert!(QuestionKind::parse_question("Q6").is_err());
        assert_eq!(QuestionKind::parse_question("").err(), Some(FiveQCode::UnknownQuestion));
    }

    #[test]
    fn register_guards() {
        let mut l = ReviewLedger::new();
        assert!(l.register(1).is_ok());
        assert_eq!(l.register(0).err(), Some(FiveQCode::ZeroEntry));
        assert!(l.register(1).is_err());
    }

    #[test]
    fn dup_answer_rejected() {
        let mut l = ReviewLedger::new();
        let _ = l.register(2);
        assert!(l.answer(2, "Q1", Verdict::Pass).is_ok());
        assert_eq!(l.answer(2, "Q1", Verdict::Fail).err(), Some(FiveQCode::DupAnswer));
        assert_eq!(l.answered(2), Ok(1));
    }

    #[test]
    fn premature_accept_blocked() {
        let mut l = ReviewLedger::new();
        let _ = l.register(3);
        let _ = l.answer(3, "Q1", Verdict::Pass);
        assert_eq!(l.accept(3).err(), Some(FiveQCode::PrematureAccept));
        assert!(!l.is_locked(3));
    }

    #[test]
    fn failed_gate_blocks_accept() {
        let mut l = ReviewLedger::new();
        let _ = l.register(4);
        let _ = l.answer(4, "Q1", Verdict::Pass);
        let _ = l.answer(4, "Q2", Verdict::Pass);
        let _ = l.answer(4, "Q3", Verdict::Fail);
        let _ = l.answer(4, "Q4", Verdict::Pass);
        let _ = l.answer(4, "Q5", Verdict::Pass);
        assert_eq!(l.accept(4).err(), Some(FiveQCode::FailedGate));
    }

    #[test]
    fn full_pass_locks() {
        let mut l = ReviewLedger::new();
        let _ = l.register(5);
        let _ = l.answer(5, "Q1", Verdict::Pass);
        let _ = l.answer(5, "Q2", Verdict::Pass);
        let _ = l.answer(5, "Q3", Verdict::Pass);
        let _ = l.answer(5, "Q4", Verdict::Pass);
        let _ = l.answer(5, "Q5", Verdict::Pass);
        assert!(l.accept(5).is_ok());
        assert_eq!(l.answer(5, "Q1", Verdict::Pass).err(), Some(FiveQCode::LockedReview));
        assert_eq!(l.stage(5), Ok(ReviewStage::Accepted));
    }

    #[test]
    fn bad_review_matrix_green() {
        let (inj, per) = run_bad_review_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn summary_verdicts() {
        let mut l = ReviewLedger::new();
        let _ = l.register(6);
        assert!(compose_review_summary(&l, 6).is_ok());
        let _ = l.answer(6, "Q1", Verdict::Pass);
        let s = compose_review_summary(&l, 6).unwrap_or_default();
        assert!(s.contains("答=1/5"));
        assert!(s.contains("未过"));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(16));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(61), -61);
    }

    #[test]
    fn unx10092_checks_green() {
        let set = run_unx10092_checks();
        assert!(set.all_passed(), "UNX-F10092 red: {}", set.red_items_text());
    }
}
