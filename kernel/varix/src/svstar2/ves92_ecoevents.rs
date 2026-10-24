//! VE-F3692 · 生态活动系统（VE-S 域 · 批次 R05 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3692`
//!
//! **职责定位（锚点原文）**：生态活动系统——
//! - **活动模型**：赛程/评审/奖励三段（评审标准公开 + 评委回避——复用回避制度）；
//! - **活动奖励**：奖励兑现承诺（预算背书——跳票 = P0）；
//! - **活动公平**：防作弊刷票（复用反刷单源）；
//! - **活动沉淀**：作品归档 + 版权许可复用。
//!
//! **判据（锚点原文）**：三段模型、评审回避、奖励背书、反刷单源、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 评审不公→P1（复述红线实测） | [`C92Code::UNFAIR_REVIEW`] |
//! | 奖励跳票→P0（背书实测） | [`C92Code::PRIZE_DEFAULT`] |
//! | 刷票容忍→P0（复述） | [`C92Code::VOTE_RIG`] |
//! | 版权纠纷→仲裁复用（复述） | [`C92Code::COPY_DISPUTE`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3692 诊断码段（独占 0x8Cxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C92Code(pub u16);

impl C92Code {
    /// 评审不公（P1 红线实测）。
    pub const UNFAIR_REVIEW: C92Code = C92Code(0x8C01);
    /// 奖励跳票（P0 背书实测）。
    pub const PRIZE_DEFAULT: C92Code = C92Code(0x8C02);
    /// 刷票（P0）。
    pub const VOTE_RIG: C92Code = C92Code(0x8C03);
    /// 版权纠纷。
    pub const COPY_DISPUTE: C92Code = C92Code(0x8C04);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C92Code::UNFAIR_REVIEW => "评审不公（P1 红线实测）。建议：回避重裁。",
            C92Code::PRIZE_DEFAULT => "奖励跳票（P0 背书实测）。建议：预算背书兑付。",
            C92Code::VOTE_RIG => "刷票检出（P0）。建议：清票反刷。",
            C92Code::COPY_DISPUTE => "版权纠纷。建议：走仲裁。",
            _ => "未知诊断码。",
        }
    }
}

// ── 三段模型 ────────────────────────────────────────────────────────

/// 活动段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 赛程。
    Contest,
    /// 评审。
    Review,
    /// 奖励。
    Reward,
}

impl Phase {
    pub const ALL: [Phase; 3] = [Phase::Contest, Phase::Review, Phase::Reward];
}

/// 活动。
pub struct Activity {
    phases_done: Vec<Phase>,
    pub prize_amount: i64,
    /// 是否预算背书。
    pub prize_backed: bool,
    /// 是否已兑现。
    pub prize_paid: bool,
    default_hits: u32,
    last_code: C92Code,
    has_error: bool,
}

impl Activity {
    pub fn new(prize_amount: i64, prize_backed: bool) -> Self {
        Self {
            phases_done: Vec::new(),
            prize_amount,
            prize_backed,
            prize_paid: false,
            default_hits: 0,
            last_code: C92Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C92Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn complete_phase(&mut self, p: Phase) {
        if !self.phases_done.contains(&p) {
            self.phases_done.push(p);
        }
    }

    /// 三段齐备。
    pub fn complete(&self) -> bool {
        Phase::ALL.iter().all(|p| self.phases_done.contains(p))
    }

    /// 奖励背书断言：未背书不得开赛。
    pub fn assert_backed(&mut self) -> Result<bool, C92Code> {
        if self.prize_backed {
            self.last_code = C92Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.default_hits += 1;
            self.last_code = C92Code::PRIZE_DEFAULT;
            self.has_error = true;
            Err(C92Code::PRIZE_DEFAULT)
        }
    }

    /// 兑现。
    pub fn pay(&mut self) -> Result<i64, C92Code> {
        if !self.prize_backed {
            self.default_hits += 1;
            self.last_code = C92Code::PRIZE_DEFAULT;
            self.has_error = true;
            return Err(C92Code::PRIZE_DEFAULT);
        }
        self.prize_paid = true;
        self.last_code = C92Code(0);
        self.has_error = false;
        Ok(self.prize_amount)
    }

    pub fn phase_count(&self) -> usize {
        self.phases_done.len()
    }

    pub fn default_hits(&self) -> u32 {
        self.default_hits
    }
}

// ── 评审回避 ────────────────────────────────────────────────────────

/// 评委。
#[derive(Clone, Copy)]
pub struct Judge {
    pub id: u8,
    pub conflicted: bool,
    pub recused: bool,
}

/// 评审器（回避复用）。
pub struct ReviewBoard {
    judges: Vec<Judge>,
    /// 标准是否公开。
    pub criteria_public: bool,
    unfair_hits: u32,
    last_code: C92Code,
    has_error: bool,
}

impl ReviewBoard {
    pub fn new(criteria_public: bool) -> Self {
        Self {
            judges: Vec::new(),
            criteria_public,
            unfair_hits: 0,
            last_code: C92Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C92Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn seat(&mut self, j: Judge) -> usize {
        self.judges.push(j);
        self.judges.len()
    }

    /// 公正断言：公开标准 + 利益回避。
    pub fn assert_fair(&mut self) -> Result<bool, C92Code> {
        let recused_ok = self.judges.iter().all(|j| !j.conflicted || j.recused);
        if self.criteria_public && recused_ok {
            self.last_code = C92Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.unfair_hits += 1;
            self.last_code = C92Code::UNFAIR_REVIEW;
            self.has_error = true;
            Err(C92Code::UNFAIR_REVIEW)
        }
    }

    pub fn unfair_hits(&self) -> u32 {
        self.unfair_hits
    }

    pub fn judge_count(&self) -> usize {
        self.judges.len()
    }
}

// ── 反刷票 ──────────────────────────────────────────────────────────

/// 票型。
#[derive(Clone, Copy)]
pub struct Vote {
    pub id: u32,
    pub source: u16,
    pub tick: u32,
}

/// 刷票检测（反刷单源）。
pub struct VoteGuard {
    votes: Vec<Vote>,
    rig_hits: u32,
    last_code: C92Code,
    has_error: bool,
}

impl VoteGuard {
    pub fn new() -> Self {
        Self {
            votes: Vec::new(),
            rig_hits: 0,
            last_code: C92Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C92Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn cast(&mut self, v: Vote) -> usize {
        self.votes.push(v);
        self.votes.len()
    }

    /// 刷票审计：同源同窗 ≥5 票即刷。
    pub fn audit(&mut self) -> Result<bool, C92Code> {
        let mut ok = true;
        for v in &self.votes {
            let same = self
                .votes
                .iter()
                .filter(|x| x.source == v.source && x.tick / 10 == v.tick / 10)
                .count();
            if same >= 5 {
                ok = false;
                break;
            }
        }
        if ok {
            self.last_code = C92Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.rig_hits += 1;
            self.last_code = C92Code::VOTE_RIG;
            self.has_error = true;
            Err(C92Code::VOTE_RIG)
        }
    }

    pub fn rig_hits(&self) -> u32 {
        self.rig_hits
    }

    pub fn vote_count(&self) -> usize {
        self.votes.len()
    }
}

// ── 版权归档 ────────────────────────────────────────────────────────

/// 归档作品。
#[derive(Clone, Copy)]
pub struct ArchiveEntry {
    pub id: u16,
    /// 许可是否齐备。
    pub licensed: bool,
    /// 是否有纠纷。
    pub disputed: bool,
}

/// 归档器（版权复用）。
pub struct PrizeArchive {
    entries: Vec<ArchiveEntry>,
    dispute_hits: u32,
    last_code: C92Code,
    has_error: bool,
}

impl PrizeArchive {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            dispute_hits: 0,
            last_code: C92Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C92Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn archive(&mut self, e: ArchiveEntry) -> usize {
        self.entries.push(e);
        self.entries.len()
    }

    /// 纠纷审计：有纠纷即报。
    pub fn audit(&mut self) -> Result<bool, C92Code> {
        if self.entries.iter().any(|e| e.disputed) {
            self.dispute_hits += 1;
            self.last_code = C92Code::COPY_DISPUTE;
            self.has_error = true;
            Err(C92Code::COPY_DISPUTE)
        } else {
            self.last_code = C92Code(0);
            self.has_error = false;
            Ok(true)
        }
    }

    /// 许可齐备数。
    pub fn licensed_count(&self) -> usize {
        self.entries.iter().filter(|e| e.licensed).count()
    }

    pub fn dispute_hits(&self) -> u32 {
        self.dispute_hits
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves92_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves92_ecoevents");
    {
        let mut a = Activity::new(1000, true);
        a.complete_phase(Phase::Contest);
        a.complete_phase(Phase::Review);
        a.complete_phase(Phase::Reward);
        chk(&mut set, "C3692-01", a.complete() && a.phase_count() == 3, "err");
    }
    {
        let mut a = Activity::new(1000, true);
        a.complete_phase(Phase::Contest);
        chk(&mut set, "C3692-02", !a.complete(), "err");
    }
    {
        let mut a = Activity::new(1000, false);
        let res = a.assert_backed();
        chk(&mut set, "C3692-03", res.err() == Some(C92Code::PRIZE_DEFAULT), "err");
    }
    {
        let mut a = Activity::new(500, true);
        let res = a.pay();
        chk(&mut set, "C3692-04", res == Ok(500) && a.prize_paid, "err");
    }
    {
        let mut b = ReviewBoard::new(true);
        b.seat(Judge { id: 1, conflicted: true, recused: true });
        b.seat(Judge { id: 2, conflicted: false, recused: false });
        chk(&mut set, "C3692-05", b.assert_fair() == Ok(true), "err");
    }
    {
        let mut b = ReviewBoard::new(true);
        b.seat(Judge { id: 1, conflicted: true, recused: false });
        let res = b.assert_fair();
        chk(&mut set, "C3692-06", res.err() == Some(C92Code::UNFAIR_REVIEW), "err");
    }
    {
        let mut b = ReviewBoard::new(false);
        b.seat(Judge { id: 1, conflicted: false, recused: false });
        let res = b.assert_fair();
        chk(&mut set, "C3692-07", res.err() == Some(C92Code::UNFAIR_REVIEW), "err");
    }
    {
        let mut g = VoteGuard::new();
        g.cast(Vote { id: 1, source: 1, tick: 10 });
        g.cast(Vote { id: 2, source: 2, tick: 10 });
        chk(&mut set, "C3692-08", g.audit() == Ok(true), "err");
    }
    {
        let mut g = VoteGuard::new();
        for i in 0..5u32 {
            g.cast(Vote { id: i, source: 9, tick: 10 + i });
        }
        let res = g.audit();
        chk(&mut set, "C3692-09", res.err() == Some(C92Code::VOTE_RIG), "err");
    }
    {
        let mut ar = PrizeArchive::new();
        ar.archive(ArchiveEntry { id: 1, licensed: true, disputed: false });
        chk(&mut set, "C3692-10", ar.audit() == Ok(true) && ar.licensed_count() == 1, "err");
    }
    {
        let mut ar = PrizeArchive::new();
        ar.archive(ArchiveEntry { id: 1, licensed: true, disputed: true });
        let res = ar.audit();
        chk(&mut set, "C3692-11", res.err() == Some(C92Code::COPY_DISPUTE), "err");
    }
    {
        let mut a = Activity::new(0, true);
        chk(&mut set, "C3692-12", a.assert_backed() == Ok(true), "err");
    }
    set
}
