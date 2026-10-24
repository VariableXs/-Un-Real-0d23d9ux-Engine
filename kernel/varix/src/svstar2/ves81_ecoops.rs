//! VE-F3681 · 生态运营总纲（VE-S 域 · 批次 R05 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3681`
//!
//! **职责定位（锚点原文）**：生态运营总纲——
//! - **四板块**：市场 / 社区 / 经济 / 治理（R05 = 生态运营组）；
//! - **健康原则**：生态健康优先于规模（刷量/僵尸资产撑规模 = 自欺——健康度 > 规模）；
//! - **双中心平衡**：创作者与用户双中心（偏一方伤另一方）；
//! - **运营透明**：推荐算法 / 分成比例 / 处罚标准全公开（规则黑箱 = 信任崩——公开断言）。
//!
//! **判据（锚点原文）**：四板块映射、三原则声明、透明断言、健康度审计、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 刷量容忍→P0（红线实测——健康度审计） | [`C81Code::FAKE_VOLUME`] |
//! | 规则黑箱→P1（透明红线实测） | [`C81Code::RULE_BLACKBOX`] |
//! | 偏废一方→平衡评审（复述） | [`C81Code::CENTER_SKEW`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3681 诊断码段（独占 0x81xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C81Code(pub u16);

impl C81Code {
    /// 刷量容忍（P0 红线实测）。
    pub const FAKE_VOLUME: C81Code = C81Code(0x8101);
    /// 规则黑箱（P1 透明红线）。
    pub const RULE_BLACKBOX: C81Code = C81Code(0x8102);
    /// 双中心偏废。
    pub const CENTER_SKEW: C81Code = C81Code(0x8103);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C81Code::FAKE_VOLUME => "刷量/僵尸资产撑规模（P0 红线实测）。建议：健康度审计。",
            C81Code::RULE_BLACKBOX => "运营规则黑箱（P1 透明红线）。建议：全量公开。",
            C81Code::CENTER_SKEW => "双中心偏废。建议：平衡评审。",
            _ => "未知诊断码。",
        }
    }
}

// ── 四板块 ──────────────────────────────────────────────────────────

/// 运营板块。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpsBoard {
    /// 市场。
    Market,
    /// 社区。
    Community,
    /// 经济。
    Economy,
    /// 治理。
    Governance,
}

impl OpsBoard {
    pub const ALL: [OpsBoard; 4] = [
        OpsBoard::Market,
        OpsBoard::Community,
        OpsBoard::Economy,
        OpsBoard::Governance,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            OpsBoard::Market => "市场",
            OpsBoard::Community => "社区",
            OpsBoard::Economy => "经济",
            OpsBoard::Governance => "治理",
        }
    }
}

/// 四板块映射。
pub struct BoardMap {
    mapped: Vec<OpsBoard>,
}

impl BoardMap {
    pub fn new() -> Self {
        Self { mapped: Vec::new() }
    }

    pub fn map(&mut self, b: OpsBoard) {
        if !self.mapped.contains(&b) {
            self.mapped.push(b);
        }
    }

    /// 四板块齐备。
    pub fn complete(&self) -> bool {
        OpsBoard::ALL.iter().all(|b| self.mapped.contains(b))
    }

    pub fn mapped_count(&self) -> usize {
        self.mapped.len()
    }
}

// ── 健康度审计 ──────────────────────────────────────────────────────

/// 健康指标。
#[derive(Clone, Copy)]
pub struct HealthMetric {
    /// 真实活跃。
    pub real_active: u32,
    /// 疑似刷量。
    pub fake_volume: u32,
    /// 僵尸资产。
    pub zombie_assets: u32,
}

/// 健康度审计器（健康度 > 规模）。
pub struct HealthAudit {
    metrics: Vec<HealthMetric>,
    fake_hits: u32,
    last_code: C81Code,
    has_error: bool,
}

impl HealthAudit {
    pub fn new() -> Self {
        Self {
            metrics: Vec::new(),
            fake_hits: 0,
            last_code: C81Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C81Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn ingest(&mut self, m: HealthMetric) {
        self.metrics.push(m);
    }

    /// 刷量审计：fake 不得超过 real 的 5%。
    pub fn audit(&mut self) -> Result<bool, C81Code> {
        let mut ok = true;
        for m in &self.metrics {
            if m.fake_volume * 20 > m.real_active {
                ok = false;
                break;
            }
        }
        if ok {
            self.last_code = C81Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fake_hits += 1;
            self.last_code = C81Code::FAKE_VOLUME;
            self.has_error = true;
            Err(C81Code::FAKE_VOLUME)
        }
    }

    /// 健康分：real - fake - zombie。
    pub fn health_score(&self) -> i64 {
        self.metrics
            .iter()
            .map(|m| m.real_active as i64 - m.fake_volume as i64 - m.zombie_assets as i64)
            .sum()
    }

    pub fn fake_hits(&self) -> u32 {
        self.fake_hits
    }

    pub fn metric_count(&self) -> usize {
        self.metrics.len()
    }
}

// ── 三原则声明 ──────────────────────────────────────────────────────

/// 运营原则。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpsPrinciple {
    /// 健康优先于规模。
    HealthFirst,
    /// 双中心平衡。
    DualCenter,
    /// 运营透明。
    Transparency,
}

impl OpsPrinciple {
    pub const ALL: [OpsPrinciple; 3] =
        [OpsPrinciple::HealthFirst, OpsPrinciple::DualCenter, OpsPrinciple::Transparency];
}

/// 原则声明器。
pub struct PrincipleDecl {
    declared: Vec<OpsPrinciple>,
}

impl PrincipleDecl {
    pub fn new() -> Self {
        Self { declared: Vec::new() }
    }

    pub fn declare(&mut self, p: OpsPrinciple) {
        if !self.declared.contains(&p) {
            self.declared.push(p);
        }
    }

    /// 三原则齐备。
    pub fn all_declared(&self) -> bool {
        OpsPrinciple::ALL.iter().all(|p| self.declared.contains(p))
    }

    pub fn count(&self) -> usize {
        self.declared.len()
    }
}

// ── 透明断言 ────────────────────────────────────────────────────────

/// 公开规则项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PublicRule {
    /// 推荐算法。
    RecAlgo,
    /// 分成比例。
    RevenueShare,
    /// 处罚标准。
    PenaltyStd,
}

impl PublicRule {
    pub const ALL: [PublicRule; 3] =
        [PublicRule::RecAlgo, PublicRule::RevenueShare, PublicRule::PenaltyStd];
}

/// 透明公开断言器。
pub struct TransparencyAssert {
    published: Vec<PublicRule>,
    blackbox_hits: u32,
    last_code: C81Code,
    has_error: bool,
}

impl TransparencyAssert {
    pub fn new() -> Self {
        Self {
            published: Vec::new(),
            blackbox_hits: 0,
            last_code: C81Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C81Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn publish(&mut self, r: PublicRule) {
        if !self.published.contains(&r) {
            self.published.push(r);
        }
    }

    /// 公开断言：三规则全公开。
    pub fn assert_public(&mut self) -> Result<bool, C81Code> {
        if PublicRule::ALL.iter().all(|r| self.published.contains(r)) {
            self.last_code = C81Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.blackbox_hits += 1;
            self.last_code = C81Code::RULE_BLACKBOX;
            self.has_error = true;
            Err(C81Code::RULE_BLACKBOX)
        }
    }

    pub fn published_count(&self) -> usize {
        self.published.len()
    }

    pub fn blackbox_hits(&self) -> u32 {
        self.blackbox_hits
    }
}

// ── 双中心平衡 ──────────────────────────────────────────────────────

/// 双中心平衡器。
pub struct DualCenter {
    /// 创作者侧得分。
    pub creator_score: i32,
    /// 用户侧得分。
    pub user_score: i32,
    skew_hits: u32,
    last_code: C81Code,
    has_error: bool,
}

impl DualCenter {
    pub fn new() -> Self {
        Self {
            creator_score: 0,
            user_score: 0,
            skew_hits: 0,
            last_code: C81Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C81Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 平衡评审：两侧差值 ≤30%。
    pub fn review_balance(&mut self) -> Result<bool, C81Code> {
        let hi = self.creator_score.max(self.user_score);
        let lo = self.creator_score.min(self.user_score);
        if hi == 0 || (hi - lo) * 10 <= hi * 3 {
            self.last_code = C81Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.skew_hits += 1;
            self.last_code = C81Code::CENTER_SKEW;
            self.has_error = true;
            Err(C81Code::CENTER_SKEW)
        }
    }

    pub fn skew_hits(&self) -> u32 {
        self.skew_hits
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves81_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves81_ecoops");
    {
        let mut m = BoardMap::new();
        for b in OpsBoard::ALL {
            m.map(b);
        }
        chk(&mut set, "C3681-01", m.complete() && m.mapped_count() == 4, "err");
    }
    {
        let mut m = BoardMap::new();
        m.map(OpsBoard::Market);
        chk(&mut set, "C3681-02", !m.complete(), "err");
    }
    {
        let mut h = HealthAudit::new();
        h.ingest(HealthMetric { real_active: 1000, fake_volume: 10, zombie_assets: 5 });
        chk(&mut set, "C3681-03", h.audit() == Ok(true), "err");
    }
    {
        let mut h = HealthAudit::new();
        h.ingest(HealthMetric { real_active: 100, fake_volume: 50, zombie_assets: 0 });
        let res = h.audit();
        chk(&mut set, "C3681-04", res.err() == Some(C81Code::FAKE_VOLUME), "err");
    }
    {
        let mut h = HealthAudit::new();
        h.ingest(HealthMetric { real_active: 100, fake_volume: 10, zombie_assets: 5 });
        chk(&mut set, "C3681-05", h.health_score() == 85, "err");
    }
    {
        let mut d = PrincipleDecl::new();
        d.declare(OpsPrinciple::HealthFirst);
        d.declare(OpsPrinciple::DualCenter);
        d.declare(OpsPrinciple::Transparency);
        chk(&mut set, "C3681-06", d.all_declared(), "err");
    }
    {
        let mut d = PrincipleDecl::new();
        d.declare(OpsPrinciple::HealthFirst);
        chk(&mut set, "C3681-07", !d.all_declared() && d.count() == 1, "err");
    }
    {
        let mut t = TransparencyAssert::new();
        t.publish(PublicRule::RecAlgo);
        t.publish(PublicRule::RevenueShare);
        t.publish(PublicRule::PenaltyStd);
        chk(&mut set, "C3681-08", t.assert_public() == Ok(true), "err");
    }
    {
        let mut t = TransparencyAssert::new();
        t.publish(PublicRule::RecAlgo);
        let res = t.assert_public();
        chk(&mut set, "C3681-09", res.err() == Some(C81Code::RULE_BLACKBOX), "err");
    }
    {
        let mut c = DualCenter::new();
        c.creator_score = 50;
        c.user_score = 48;
        chk(&mut set, "C3681-10", c.review_balance() == Ok(true), "err");
    }
    {
        let mut c = DualCenter::new();
        c.creator_score = 100;
        c.user_score = 10;
        let res = c.review_balance();
        chk(&mut set, "C3681-11", res.err() == Some(C81Code::CENTER_SKEW), "err");
    }
    {
        let names_ok = OpsBoard::ALL.iter().all(|b| !b.name().is_empty());
        chk(&mut set, "C3681-12", names_ok, "err");
    }
    set
}
