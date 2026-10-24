//! VE-F3661 · R04 开工与运行深化总纲（VE-S 域 · 批次 R04 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3661`
//!
//! **职责定位（锚点原文）**：R04 运行深化组总纲——
//! - **前后半场声明**：R01-R03 创作产出之后的运行侧深化（个性化/生态运营/
//!   质量/性能/无障碍五组）——前半场造工具、后半场深运行；
//! - **个性化原则**：用户主权（推荐与应用分离红线：默认推荐 ≠ 默认应用）、
//!   可逆（一键还原）、可携带（数据可导出迁移）。
//!
//! **判据（锚点原文六条）**：前后半场、三原则、推荐分离、
//! 可逆、可携带、判据。
//!
//! **错误路径与降级矩阵（锚点原文三条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 擅自应用→P1（分离红线实测） | [`R61Code::AUTO_APPLY`] |
//! | 不可逆检出→架构修正（红线实测） | [`R61Code::IRREVERSIBLE`] |
//! | 携带锁死→P0（开放红线） | [`R61Code::PORTABILITY_LOCK`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3661 诊断码段（独占 0x6Dxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct R61Code(pub u16);

impl R61Code {
    /// 推荐被擅自应用（P1 分离红线）。
    pub const AUTO_APPLY: R61Code = R61Code(0x6D01);
    /// 个性化不可逆（红线实测）。
    pub const IRREVERSIBLE: R61Code = R61Code(0x6D02);
    /// 携带锁死（P0 开放红线）。
    pub const PORTABILITY_LOCK: R61Code = R61Code(0x6D03);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            R61Code::AUTO_APPLY => "推荐被擅自应用（分离红线 P1）。建议：推荐与应用分离。",
            R61Code::IRREVERSIBLE => "个性化不可逆（红线实测）。建议：补一键还原。",
            R61Code::PORTABILITY_LOCK => "携带锁死（P0 开放红线）。建议：开放导出迁移。",
            _ => "未知诊断码。",
        }
    }
}

// ── 五组总纲 ────────────────────────────────────────────────────────

/// R04 五组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    /// 个性化。
    Personal,
    /// 生态运营。
    Ecosystem,
    /// 质量。
    Quality,
    /// 性能。
    Perf,
    /// 无障碍。
    A11y,
}

impl Group {
    pub const ALL: [Group; 5] = [
        Group::Personal,
        Group::Ecosystem,
        Group::Quality,
        Group::Perf,
        Group::A11y,
    ];
}

// ── 四域个性化 ──────────────────────────────────────────────────────

/// 个性化域（四域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Domain {
    /// 视觉（主题/风格/密度）。
    Visual,
    /// 交互（布局/快捷键/手势）。
    Interaction,
    /// 内容（首页模块/默认排序）。
    Content,
    /// 行为（自动化偏好/默认动作）。
    Behavior,
}

impl Domain {
    pub const ALL: [Domain; 4] = [Domain::Visual, Domain::Interaction, Domain::Content, Domain::Behavior];
}

// ── 三原则 ──────────────────────────────────────────────────────────

/// 个性化原则。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Principle {
    /// 用户主权（推荐≠应用）。
    Sovereignty,
    /// 可逆（一键还原）。
    Reversible,
    /// 可携带（导出迁移）。
    Portable,
}

impl Principle {
    pub const ALL: [Principle; 3] = [Principle::Sovereignty, Principle::Reversible, Principle::Portable];
}

// ── 总纲面板 ────────────────────────────────────────────────────────

/// R04 总纲。
pub struct R04Charter {
    groups: Vec<Group>,
    domains: Vec<Domain>,
    principles: Vec<Principle>,
    /// 推荐是否已被应用（分离红线执法位）。
    recommend_applied: bool,
    reversible: bool,
    portable: bool,
    last_code: R61Code,
    has_error: bool,
}

impl R04Charter {
    pub fn new() -> Self {
        Self {
            groups: Vec::new(),
            domains: Vec::new(),
            principles: Vec::new(),
            recommend_applied: false,
            reversible: true,
            portable: true,
            last_code: R61Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> R61Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: R61Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 登记组。
    pub fn declare_group(&mut self, g: Group) -> usize {
        if !self.groups.contains(&g) {
            self.groups.push(g);
        }
        self.groups.len()
    }

    /// 五组齐。
    pub fn groups_complete(&self) -> bool {
        Group::ALL.iter().all(|g| self.groups.contains(g))
    }

    /// 登记个性化域。
    pub fn declare_domain(&mut self, d: Domain) -> usize {
        if !self.domains.contains(&d) {
            self.domains.push(d);
        }
        self.domains.len()
    }

    /// 四域齐。
    pub fn domains_complete(&self) -> bool {
        Domain::ALL.iter().all(|d| self.domains.contains(d))
    }

    /// 声明原则。
    pub fn declare_principle(&mut self, p: Principle) -> usize {
        if !self.principles.contains(&p) {
            self.principles.push(p);
        }
        self.principles.len()
    }

    /// 推荐与应用分离执法：`apply=true` 且未获用户确认即红线。
    pub fn offer_recommendation(&mut self, user_approved: bool, apply: bool) -> Result<bool, R61Code> {
        if apply && !user_approved {
            self.recommend_applied = true;
            self.fail(R61Code::AUTO_APPLY);
            return Err(R61Code::AUTO_APPLY);
        }
        self.recommend_applied = apply && user_approved;
        self.last_code = R61Code(0);
        self.has_error = false;
        Ok(true)
    }

    /// 可逆审计。
    pub fn assert_reversible(&mut self, reversible: bool) -> Result<bool, R61Code> {
        self.reversible = reversible;
        if reversible {
            self.last_code = R61Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(R61Code::IRREVERSIBLE);
            Err(R61Code::IRREVERSIBLE)
        }
    }

    /// 可携带审计。
    pub fn assert_portable(&mut self, portable: bool) -> Result<bool, R61Code> {
        self.portable = portable;
        if portable {
            self.last_code = R61Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(R61Code::PORTABILITY_LOCK);
            Err(R61Code::PORTABILITY_LOCK)
        }
    }

    /// 三原则齐。
    pub fn principles_complete(&self) -> bool {
        Principle::ALL.iter().all(|p| self.principles.contains(p))
    }

    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    pub fn domain_count(&self) -> usize {
        self.domains.len()
    }

    pub fn auto_applied(&self) -> bool {
        self.recommend_applied
    }

    /// 前后半场声明：五组齐 + 四域齐 + 三原则齐。
    pub fn charter_green(&self) -> bool {
        self.groups_complete() && self.domains_complete() && self.principles_complete()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves61_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves61_r04charter");
    {
        let c = R04Charter::new();
        chk(&mut set, "C3661-01", c.group_count() == 0 && c.domain_count() == 0, "err");
    }
    {
        let mut c = R04Charter::new();
        for g in Group::ALL.iter() {
            c.declare_group(*g);
        }
        chk(&mut set, "C3661-02", c.groups_complete(), "err");
    }
    {
        let mut c = R04Charter::new();
        for d in Domain::ALL.iter() {
            c.declare_domain(*d);
        }
        chk(&mut set, "C3661-03", c.domains_complete() && c.domain_count() == 4, "err");
    }
    {
        let mut c = R04Charter::new();
        let r = c.offer_recommendation(false, true);
        chk(&mut set, "C3661-04", r.err() == Some(R61Code::AUTO_APPLY), "err");
    }
    {
        let mut c = R04Charter::new();
        let r = c.offer_recommendation(true, true);
        chk(&mut set, "C3661-05", r == Ok(true), "err");
    }
    {
        let mut c = R04Charter::new();
        let r = c.offer_recommendation(false, false);
        chk(&mut set, "C3661-06", r == Ok(true) && !c.auto_applied(), "err");
    }
    {
        let mut c = R04Charter::new();
        let r = c.assert_reversible(true);
        chk(&mut set, "C3661-07", r == Ok(true), "err");
    }
    {
        let mut c = R04Charter::new();
        let r = c.assert_reversible(false);
        chk(&mut set, "C3661-08", r.err() == Some(R61Code::IRREVERSIBLE), "err");
    }
    {
        let mut c = R04Charter::new();
        let r = c.assert_portable(true);
        chk(&mut set, "C3661-09", r == Ok(true), "err");
    }
    {
        let mut c = R04Charter::new();
        let r = c.assert_portable(false);
        chk(&mut set, "C3661-10", r.err() == Some(R61Code::PORTABILITY_LOCK), "err");
    }
    {
        let mut c = R04Charter::new();
        for p in Principle::ALL.iter() {
            c.declare_principle(*p);
        }
        chk(&mut set, "C3661-11", c.principles_complete(), "err");
    }
    {
        let mut c = R04Charter::new();
        for g in Group::ALL.iter() {
            c.declare_group(*g);
        }
        for d in Domain::ALL.iter() {
            c.declare_domain(*d);
        }
        for p in Principle::ALL.iter() {
            c.declare_principle(*p);
        }
        chk(&mut set, "C3661-12", c.charter_green(), "err");
    }
    set
}
