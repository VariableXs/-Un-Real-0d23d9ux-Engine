//! VE-F3691 · 新人成长体系（VE-S 域 · 批次 R05 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3691`
//!
//! **职责定位（锚点原文）**：新人成长体系——
//! - **成长路径**：新手任务里程碑（激励目标不可达 = 画饼——可达红线，达成率审计）；
//! - **导师机制**：老带新 + 导师认证（劣质导师误导新人——导师认证断言）；
//! - **成长保护**：新人流量扶持期（扶持期公开）；
//! - **成长透明**：进度可视化。
//!
//! **判据（锚点原文）**：里程碑可达、导师认证、扶持期公开、进度可见、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 里程碑不可达→P1（红线实测） | [`C91Code::MILESTONE_UNREACHABLE`] |
//! | 劣质导师→认证撤销（复述红线） | [`C91Code::MENTOR_UNQUALIFIED`] |
//! | 扶持暗箱→公开（复述） | [`C91Code::BOOST_HIDDEN`] |
//! | 进度不实→断言（复述） | [`C91Code::PROGRESS_FAKE`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3691 诊断码段（独占 0x8Bxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C91Code(pub u16);

impl C91Code {
    /// 里程碑不可达（P1 红线实测）。
    pub const MILESTONE_UNREACHABLE: C91Code = C91Code(0x8B01);
    /// 劣质导师。
    pub const MENTOR_UNQUALIFIED: C91Code = C91Code(0x8B02);
    /// 扶持暗箱。
    pub const BOOST_HIDDEN: C91Code = C91Code(0x8B03);
    /// 进度不实。
    pub const PROGRESS_FAKE: C91Code = C91Code(0x8B04);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C91Code::MILESTONE_UNREACHABLE => "里程碑不可达（P1 红线实测）。建议：调低门槛或补资源。",
            C91Code::MENTOR_UNQUALIFIED => "导师未认证/劣质。建议：认证撤销。",
            C91Code::BOOST_HIDDEN => "扶持暗箱。建议：公开扶持期。",
            C91Code::PROGRESS_FAKE => "进度不实。建议：按真实数据断言。",
            _ => "未知诊断码。",
        }
    }
}

// ── 里程碑 ──────────────────────────────────────────────────────────

/// 里程碑。
#[derive(Clone, Copy)]
pub struct Milestone {
    pub id: u16,
    /// 目标值。
    pub target: u32,
    /// 已达成人数。
    pub achieved: u32,
    /// 参与总人数。
    pub participants: u32,
}

/// 里程碑审计（可达红线）。
pub struct MilestoneAudit {
    items: Vec<Milestone>,
    unreachable_hits: u32,
    last_code: C91Code,
    has_error: bool,
}

impl MilestoneAudit {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            unreachable_hits: 0,
            last_code: C91Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C91Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn add(&mut self, m: Milestone) -> usize {
        self.items.push(m);
        self.items.len()
    }

    /// 达成率（万分比）。
    pub fn achieve_rate_bp(&self, id: u16) -> u32 {
        self.items
            .iter()
            .find(|m| m.id == id)
            .map(|m| {
                if m.participants == 0 {
                    0
                } else {
                    m.achieved * 10000 / m.participants
                }
            })
            .unwrap_or(0)
    }

    /// 可达审计：达成率 <20% 即立案。
    pub fn audit_reachable(&mut self) -> Result<bool, C91Code> {
        let mut ok = true;
        for m in &self.items {
            if m.participants > 0 && m.achieved * 5 < m.participants {
                ok = false;
                break;
            }
        }
        if ok {
            self.last_code = C91Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.unreachable_hits += 1;
            self.last_code = C91Code::MILESTONE_UNREACHABLE;
            self.has_error = true;
            Err(C91Code::MILESTONE_UNREACHABLE)
        }
    }

    pub fn unreachable_hits(&self) -> u32 {
        self.unreachable_hits
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }
}

// ── 导师认证 ────────────────────────────────────────────────────────

/// 导师。
#[derive(Clone, Copy)]
pub struct Mentor {
    pub id: u8,
    /// 是否认证。
    pub certified: bool,
    /// 教学质量分（0-100）。
    pub quality: u8,
}

/// 导师管理（认证断言）。
pub struct MentorRegistry {
    mentors: Vec<Mentor>,
    unqualified_hits: u32,
    last_code: C91Code,
    has_error: bool,
}

impl MentorRegistry {
    pub fn new() -> Self {
        Self {
            mentors: Vec::new(),
            unqualified_hits: 0,
            last_code: C91Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C91Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn enroll(&mut self, m: Mentor) -> usize {
        self.mentors.push(m);
        self.mentors.len()
    }

    /// 认证断言：上岗导师必须认证且质量 ≥60。
    pub fn assert_qualified(&mut self) -> Result<bool, C91Code> {
        let mut ok = true;
        for m in &self.mentors {
            if !m.certified || m.quality < 60 {
                ok = false;
                break;
            }
        }
        if ok {
            self.last_code = C91Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.unqualified_hits += 1;
            self.last_code = C91Code::MENTOR_UNQUALIFIED;
            self.has_error = true;
            Err(C91Code::MENTOR_UNQUALIFIED)
        }
    }

    /// 撤销劣质导师。
    pub fn revoke(&mut self, id: u8) -> bool {
        if let Some(pos) = self.mentors.iter().position(|m| m.id == id) {
            self.mentors.remove(pos);
            true
        } else {
            false
        }
    }

    pub fn unqualified_hits(&self) -> u32 {
        self.unqualified_hits
    }

    pub fn mentor_count(&self) -> usize {
        self.mentors.len()
    }
}

// ── 扶持期 ──────────────────────────────────────────────────────────

/// 扶持期声明。
pub struct BoostWindow {
    pub start_tick: u32,
    pub end_tick: u32,
    /// 是否公开。
    pub published: bool,
}

/// 扶持管理（公开复述）。
pub struct BoostManager {
    windows: Vec<BoostWindow>,
    hidden_hits: u32,
    last_code: C91Code,
    has_error: bool,
}

impl BoostManager {
    pub fn new() -> Self {
        Self {
            windows: Vec::new(),
            hidden_hits: 0,
            last_code: C91Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C91Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn declare(&mut self, w: BoostWindow) -> usize {
        self.windows.push(w);
        self.windows.len()
    }

    /// 公开审计：扶持期必须公开。
    pub fn audit_public(&mut self) -> Result<bool, C91Code> {
        if self.windows.iter().all(|w| w.published) {
            self.last_code = C91Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.hidden_hits += 1;
            self.last_code = C91Code::BOOST_HIDDEN;
            self.has_error = true;
            Err(C91Code::BOOST_HIDDEN)
        }
    }

    /// 当前是否在扶持期。
    pub fn in_window(&self, now: u32) -> bool {
        self.windows
            .iter()
            .any(|w| now >= w.start_tick && now <= w.end_tick)
    }

    pub fn hidden_hits(&self) -> u32 {
        self.hidden_hits
    }

    pub fn window_count(&self) -> usize {
        self.windows.len()
    }
}

// ── 进度可视化 ──────────────────────────────────────────────────────

/// 进度记录。
#[derive(Clone, Copy)]
pub struct Progress {
    pub id: u16,
    /// 显示进度（百分比）。
    pub displayed: u8,
    /// 真实进度。
    pub actual: u8,
}

/// 进度可视化（不实断言）。
pub struct ProgressView {
    items: Vec<Progress>,
    fake_hits: u32,
    last_code: C91Code,
    has_error: bool,
}

impl ProgressView {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            fake_hits: 0,
            last_code: C91Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C91Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn put(&mut self, p: Progress) -> usize {
        self.items.push(p);
        self.items.len()
    }

    /// 真实断言：显示进度必须等于真实进度。
    pub fn assert_truthful(&mut self) -> Result<bool, C91Code> {
        let mut ok = true;
        for p in &self.items {
            if p.displayed != p.actual {
                ok = false;
                break;
            }
        }
        if ok {
            self.last_code = C91Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fake_hits += 1;
            self.last_code = C91Code::PROGRESS_FAKE;
            self.has_error = true;
            Err(C91Code::PROGRESS_FAKE)
        }
    }

    pub fn fake_hits(&self) -> u32 {
        self.fake_hits
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves91_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves91_newbiegrow");
    {
        let mut a = MilestoneAudit::new();
        a.add(Milestone { id: 1, target: 10, achieved: 50, participants: 100 });
        chk(&mut set, "C3691-01", a.audit_reachable() == Ok(true), "err");
    }
    {
        let mut a = MilestoneAudit::new();
        a.add(Milestone { id: 1, target: 10, achieved: 5, participants: 100 });
        let res = a.audit_reachable();
        chk(&mut set, "C3691-02", res.err() == Some(C91Code::MILESTONE_UNREACHABLE), "err");
    }
    {
        let mut a = MilestoneAudit::new();
        a.add(Milestone { id: 1, target: 10, achieved: 50, participants: 100 });
        chk(&mut set, "C3691-03", a.achieve_rate_bp(1) == 5000, "err");
    }
    {
        let mut r = MentorRegistry::new();
        r.enroll(Mentor { id: 1, certified: true, quality: 80 });
        chk(&mut set, "C3691-04", r.assert_qualified() == Ok(true), "err");
    }
    {
        let mut r = MentorRegistry::new();
        r.enroll(Mentor { id: 1, certified: false, quality: 90 });
        let res = r.assert_qualified();
        chk(&mut set, "C3691-05", res.err() == Some(C91Code::MENTOR_UNQUALIFIED), "err");
    }
    {
        let mut r = MentorRegistry::new();
        r.enroll(Mentor { id: 1, certified: true, quality: 40 });
        let res = r.assert_qualified();
        chk(&mut set, "C3691-06", res.err() == Some(C91Code::MENTOR_UNQUALIFIED), "err");
    }
    {
        let mut r = MentorRegistry::new();
        r.enroll(Mentor { id: 5, certified: true, quality: 40 });
        let removed = r.revoke(5);
        chk(&mut set, "C3691-07", removed && r.mentor_count() == 0, "err");
    }
    {
        let mut b = BoostManager::new();
        b.declare(BoostWindow { start_tick: 0, end_tick: 100, published: true });
        chk(&mut set, "C3691-08", b.audit_public() == Ok(true) && b.in_window(50), "err");
    }
    {
        let mut b = BoostManager::new();
        b.declare(BoostWindow { start_tick: 0, end_tick: 100, published: false });
        let res = b.audit_public();
        chk(&mut set, "C3691-09", res.err() == Some(C91Code::BOOST_HIDDEN), "err");
    }
    {
        let mut v = ProgressView::new();
        v.put(Progress { id: 1, displayed: 50, actual: 50 });
        chk(&mut set, "C3691-10", v.assert_truthful() == Ok(true), "err");
    }
    {
        let mut v = ProgressView::new();
        v.put(Progress { id: 1, displayed: 90, actual: 30 });
        let res = v.assert_truthful();
        chk(&mut set, "C3691-11", res.err() == Some(C91Code::PROGRESS_FAKE), "err");
    }
    {
        let mut b = BoostManager::new();
        b.declare(BoostWindow { start_tick: 0, end_tick: 10, published: true });
        chk(&mut set, "C3691-12", !b.in_window(50), "err");
    }
    set
}
