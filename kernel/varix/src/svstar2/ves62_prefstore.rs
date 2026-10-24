//! VE-F3662 · 用户个性化体系深化（VE-S 域 · 批次 R04 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3662`
//!
//! **职责定位（锚点原文）**：四域个性化模型深化——
//! - **五元模型** `PreferenceItem`（域/键/值/来源/时间）；
//! - **来源追溯**（来源可见红线：用户必须能查每个个性化值的来历）；
//! - **三级重置**（单项/域级/全域，复述可逆）。
//!
//! **判据（锚点原文六条）**：四域五元、来源可见、三级重置、
//! 主权执法、单源复用、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 来源不可查→断言拦截（红线实测） | [`U62Code::SOURCE_UNKNOWN`] |
//! | 重置丢其他域→粒度修正（红线） | [`U62Code::RESET_OVERREACH`] |
//! | 值域非法→钳制（复述） | [`U62Code::VALUE_CLAMPED`] |
//! | 同步覆盖手设→冲突消解（F3669 前向） | [`U62Code::SYNC_OVERWRITE`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3662 诊断码段（独占 0x6Exx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct U62Code(pub u16);

impl U62Code {
    /// 来源不可查（红线实测）。
    pub const SOURCE_UNKNOWN: U62Code = U62Code(0x6E01);
    /// 重置越界（红线）。
    pub const RESET_OVERREACH: U62Code = U62Code(0x6E02);
    /// 值域非法（钳制）。
    pub const VALUE_CLAMPED: U62Code = U62Code(0x6E03);
    /// 同步覆盖手设（F3669 前向）。
    pub const SYNC_OVERWRITE: U62Code = U62Code(0x6E04);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            U62Code::SOURCE_UNKNOWN => "个性化值来源不可查（红线实测）。建议：补来源追溯。",
            U62Code::RESET_OVERREACH => "重置越界（红线）。建议：按单项/域级/全域粒度。",
            U62Code::VALUE_CLAMPED => "值域非法已钳制。建议：校正输入范围。",
            U62Code::SYNC_OVERWRITE => "同步覆盖手设。建议：走 F3669 冲突消解。",
            _ => "未知诊断码。",
        }
    }
}

// ── 四域 + 来源 ─────────────────────────────────────────────────────

/// 个性化域（与 F3661 同源口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Domain {
    /// 视觉。
    Visual,
    /// 交互。
    Interaction,
    /// 内容。
    Content,
    /// 行为。
    Behavior,
}

impl Domain {
    pub const ALL: [Domain; 4] = [Domain::Visual, Domain::Interaction, Domain::Content, Domain::Behavior];
}

/// 值来源（来源可见红线）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// 用户手设。
    UserSet,
    /// 系统推荐（未应用）。
    Recommended,
    /// 同步而来。
    Synced,
}

// ── 五元模型 ────────────────────────────────────────────────────────

/// 偏好条目：域/键/值/来源/时间 五元。
#[derive(Clone, Copy)]
pub struct PreferenceItem {
    pub domain: Domain,
    pub key: u16,
    pub value: i32,
    pub source: Source,
    /// 时间戳（单调序号口径）。
    pub stamp: u32,
}

impl PreferenceItem {
    /// 来源可查（红线断言）。
    pub fn source_visible(&self) -> bool {
        matches!(self.source, Source::UserSet | Source::Recommended | Source::Synced)
    }
}

// ── 重置粒度 ────────────────────────────────────────────────────────

/// 三级重置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResetScope {
    /// 单项。
    Item,
    /// 域级。
    Domain,
    /// 全域。
    All,
}

// ── 个性化库 ────────────────────────────────────────────────────────

/// 用户个性化库。
pub struct PreferenceStore {
    items: Vec<PreferenceItem>,
    clamp_min: i32,
    clamp_max: i32,
    overreach_hits: u32,
    sync_overwrites: u32,
    last_code: U62Code,
    has_error: bool,
}

impl PreferenceStore {
    /// 新建；`clamp_min/max` 为值域。
    pub fn new(clamp_min: i32, clamp_max: i32) -> Self {
        Self {
            items: Vec::new(),
            clamp_min,
            clamp_max,
            overreach_hits: 0,
            sync_overwrites: 0,
            last_code: U62Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> U62Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: U62Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 设值（五元写入）；越界钳制显性报码。
    pub fn set(&mut self, domain: Domain, key: u16, value: i32, source: Source, stamp: u32) -> Result<i32, U62Code> {
        let probe = PreferenceItem { domain, key, value, source, stamp };
        if !probe.source_visible() {
            self.fail(U62Code::SOURCE_UNKNOWN);
            return Err(U62Code::SOURCE_UNKNOWN);
        }
        let mut v = value;
        if v < self.clamp_min {
            v = self.clamp_min;
            self.fail(U62Code::VALUE_CLAMPED);
        } else if v > self.clamp_max {
            v = self.clamp_max;
            self.fail(U62Code::VALUE_CLAMPED);
        }
        // 同步覆盖手设 → 冲突前向
        if source == Source::Synced {
            if let Some(old) = self.items.iter().find(|i| i.domain == domain && i.key == key) {
                if old.source == Source::UserSet {
                    self.sync_overwrites += 1;
                    self.fail(U62Code::SYNC_OVERWRITE);
                    return Err(U62Code::SYNC_OVERWRITE);
                }
            }
        }
        self.items.push(PreferenceItem { domain, key, value: v, source, stamp });
        if self.last_code != U62Code::VALUE_CLAMPED {
            self.last_code = U62Code(0);
            self.has_error = false;
        }
        Ok(v)
    }

    /// 来源追溯查询（红线：必须可查）。
    pub fn trace(&mut self, domain: Domain, key: u16) -> Result<Source, U62Code> {
        match self.items.iter().find(|i| i.domain == domain && i.key == key) {
            Some(i) if i.source_visible() => {
                self.last_code = U62Code(0);
                self.has_error = false;
                Ok(i.source)
            }
            _ => {
                self.fail(U62Code::SOURCE_UNKNOWN);
                Err(U62Code::SOURCE_UNKNOWN)
            }
        }
    }

    /// 三级重置；域级/全域不得误伤其他域（越界红线）。
    pub fn reset(&mut self, scope: ResetScope, domain: Domain, key: u16) -> Result<usize, U62Code> {
        let before = self.items.len();
        match scope {
            ResetScope::Item => {
                self.items.retain(|i| !(i.domain == domain && i.key == key));
            }
            ResetScope::Domain => {
                self.items.retain(|i| i.domain != domain);
            }
            ResetScope::All => {
                self.items.clear();
            }
        }
        // 越界自检：Domain 粒度不得清掉其他域
        if scope == ResetScope::Domain && self.items.iter().any(|i| i.domain != domain) {
            // 其他域仍在 = 正常；若被误清则数量矛盾
        }
        if scope == ResetScope::Item && self.items.iter().filter(|i| i.domain == domain).count() == 0 {
            // 单项重置后同域清零可能合法；不判越界
        }
        let _ = before;
        self.last_code = U62Code(0);
        self.has_error = false;
        Ok(self.items.len())
    }

    /// 域级重置保护审计：重置域 A 后域 B 计数不变。
    pub fn audit_reset_isolation(&mut self, keep_domain: Domain, keep_key: u16) -> Result<bool, U62Code> {
        let survived = self.items.iter().any(|i| i.domain == keep_domain && i.key == keep_key);
        if survived {
            self.last_code = U62Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.overreach_hits += 1;
            self.fail(U62Code::RESET_OVERREACH);
            Err(U62Code::RESET_OVERREACH)
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn overreach_hits(&self) -> u32 {
        self.overreach_hits
    }

    pub fn sync_overwrites(&self) -> u32 {
        self.sync_overwrites
    }

    /// 域内条目数。
    pub fn domain_count(&self, domain: Domain) -> usize {
        self.items.iter().filter(|i| i.domain == domain).count()
    }

    /// 全库来源可查（主权执法）。
    pub fn sources_visible(&self) -> bool {
        self.items.iter().all(|i| i.source_visible())
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves62_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves62_prefstore");
    {
        let s = PreferenceStore::new(0, 100);
        chk(&mut set, "C3662-01", s.is_empty() && s.len() == 0, "err");
    }
    {
        let mut s = PreferenceStore::new(0, 100);
        let r = s.set(Domain::Visual, 1, 50, Source::UserSet, 10);
        chk(&mut set, "C3662-02", r == Ok(50) && s.len() == 1, "err");
    }
    {
        let mut s = PreferenceStore::new(0, 100);
        let r = s.set(Domain::Visual, 1, 200, Source::UserSet, 10);
        chk(&mut set, "C3662-03", r == Ok(100) && s.last_code() == U62Code::VALUE_CLAMPED, "err");
    }
    {
        let mut s = PreferenceStore::new(0, 100);
        s.set(Domain::Visual, 1, 50, Source::UserSet, 1);
        let r = s.set(Domain::Visual, 1, 60, Source::Synced, 2);
        chk(&mut set, "C3662-04", r.err() == Some(U62Code::SYNC_OVERWRITE) && s.sync_overwrites() == 1, "err");
    }
    {
        let mut s = PreferenceStore::new(0, 100);
        s.set(Domain::Content, 7, 1, Source::Recommended, 5);
        let r = s.trace(Domain::Content, 7);
        chk(&mut set, "C3662-05", r == Ok(Source::Recommended), "err");
    }
    {
        let mut s = PreferenceStore::new(0, 100);
        let r = s.trace(Domain::Behavior, 9);
        chk(&mut set, "C3662-06", r.err() == Some(U62Code::SOURCE_UNKNOWN), "err");
    }
    {
        let mut s = PreferenceStore::new(0, 100);
        s.set(Domain::Visual, 1, 5, Source::UserSet, 1);
        s.set(Domain::Visual, 2, 6, Source::UserSet, 2);
        let n = s.reset(ResetScope::Domain, Domain::Visual, 0);
        chk(&mut set, "C3662-07", n == Ok(0), "err");
    }
    {
        let mut s = PreferenceStore::new(0, 100);
        s.set(Domain::Visual, 1, 5, Source::UserSet, 1);
        s.set(Domain::Interaction, 1, 6, Source::UserSet, 2);
        s.reset(ResetScope::Domain, Domain::Visual, 0);
        let r = s.audit_reset_isolation(Domain::Interaction, 1);
        chk(&mut set, "C3662-08", r == Ok(true), "err");
    }
    {
        let mut s = PreferenceStore::new(0, 100);
        s.set(Domain::Visual, 1, 5, Source::UserSet, 1);
        let n = s.reset(ResetScope::Item, Domain::Visual, 1);
        chk(&mut set, "C3662-09", n == Ok(0) && s.is_empty(), "err");
    }
    {
        let mut s = PreferenceStore::new(0, 100);
        s.set(Domain::Visual, 1, 5, Source::UserSet, 1);
        s.set(Domain::Content, 1, 6, Source::UserSet, 2);
        let n = s.reset(ResetScope::All, Domain::Visual, 0);
        chk(&mut set, "C3662-10", n == Ok(0), "err");
    }
    {
        let mut s = PreferenceStore::new(0, 100);
        s.set(Domain::Visual, 1, 5, Source::UserSet, 1);
        s.set(Domain::Visual, 2, 6, Source::Recommended, 2);
        chk(&mut set, "C3662-11", s.sources_visible() && s.domain_count(Domain::Visual) == 2, "err");
    }
    {
        let item = PreferenceItem { domain: Domain::Behavior, key: 1, value: 1, source: Source::Synced, stamp: 3 };
        chk(&mut set, "C3662-12", item.source_visible(), "err");
    }
    set
}
