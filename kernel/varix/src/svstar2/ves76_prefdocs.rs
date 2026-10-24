//! VE-F3676 · 个性化文档（VE-S 域 · 批次 R04 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3676`
//!
//! **职责定位（锚点原文）**：个性化文档——
//! - **用户册**：个性化能做什么 / 四权入口（可发现复述）；
//! - **开发者册**：个性化 API / 扩展个性化项——扩展项义务：新个性化项必须登记隐私级别
//!   （隐私登记红线：无隐私级别不可发布——登记硬门）；
//! - **配置指南**复述；同源纪律复述；2 跳地图复用。
//!
//! **判据（锚点原文）**：隐私登记硬门、四权文档、2 跳复用、同源复述、家族管道、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 隐私未登记→不可发布（红线实测） | [`C76Code::UNREGISTERED`] |
//! | 手册漂移→对账（复述） | [`C76Code::DOC_DRIFT`] |
//! | FAQ 过时→复审（复述） | [`C76Code::FAQ_STALE`] |
//! | 迷宫→2 跳（复用红线） | [`C76Code::MAZE`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3676 诊断码段（独占 0x7Cxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C76Code(pub u16);

impl C76Code {
    /// 隐私未登记不可发布（登记硬门）。
    pub const UNREGISTERED: C76Code = C76Code(0x7C01);
    /// 手册漂移。
    pub const DOC_DRIFT: C76Code = C76Code(0x7C02);
    /// FAQ 过时。
    pub const FAQ_STALE: C76Code = C76Code(0x7C03);
    /// 导航迷宫（>2 跳）。
    pub const MAZE: C76Code = C76Code(0x7C04);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C76Code::UNREGISTERED => "个性化项未登记隐私级别（登记硬门）。建议：登记后发布。",
            C76Code::DOC_DRIFT => "手册漂移。建议：同源对账。",
            C76Code::FAQ_STALE => "FAQ 过时。建议：复审。",
            C76Code::MAZE => "导航超 2 跳（复用红线）。建议：收敛路径。",
            _ => "未知诊断码。",
        }
    }
}

// ── 隐私登记硬门 ────────────────────────────────────────────────────

/// 隐私级别。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum PrivacyLevel {
    /// 公开偏好。
    Public = 0,
    /// 个人偏好。
    Personal = 1,
    /// 敏感偏好。
    Sensitive = 2,
}

/// 扩展个性化项登记。
#[derive(Clone, Copy)]
pub struct PrefItemReg {
    pub key: u16,
    pub level: Option<PrivacyLevel>,
    pub published: bool,
}

/// 隐私登记硬门。
pub struct PrivacyGate {
    items: Vec<PrefItemReg>,
    blocked: u32,
    last_code: C76Code,
    has_error: bool,
}

impl PrivacyGate {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            blocked: 0,
            last_code: C76Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C76Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 登记个性化项的隐私级别。
    pub fn register(&mut self, key: u16, level: PrivacyLevel) -> Result<bool, C76Code> {
        if let Some(e) = self.items.iter_mut().find(|e| e.key == key) {
            e.level = Some(level);
        } else {
            self.items.push(PrefItemReg { key, level: Some(level), published: false });
        }
        self.last_code = C76Code(0);
        self.has_error = false;
        Ok(true)
    }

    /// 发布闸：无隐私级别不可发布。
    pub fn publish(&mut self, key: u16) -> Result<bool, C76Code> {
        let has_level = self
            .items
            .iter()
            .any(|e| e.key == key && e.level.is_some());
        if has_level {
            if let Some(e) = self.items.iter_mut().find(|e| e.key == key) {
                e.published = true;
            }
            self.last_code = C76Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.blocked += 1;
            self.last_code = C76Code::UNREGISTERED;
            self.has_error = true;
            Err(C76Code::UNREGISTERED)
        }
    }

    pub fn blocked(&self) -> u32 {
        self.blocked
    }

    pub fn published_count(&self) -> usize {
        self.items.iter().filter(|e| e.published).count()
    }

    /// 全部已登记项齐备。
    pub fn all_registered(&self) -> bool {
        self.items.iter().all(|e| e.level.is_some())
    }
}

// ── 两册结构 ────────────────────────────────────────────────────────

/// 册别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BookKind {
    /// 用户册。
    User,
    /// 开发者册。
    Dev,
}

/// 文档章节。
#[derive(Clone, Copy)]
pub struct DocChapter {
    pub book: BookKind,
    /// 四权入口是否已写（用户册义务）。
    pub has_rights: bool,
    /// API 是否已写（开发者册义务）。
    pub has_api: bool,
}

/// 两册文档结构。
pub struct DocPair {
    chapters: Vec<DocChapter>,
    drift_hits: u32,
    last_code: C76Code,
    has_error: bool,
}

impl DocPair {
    pub fn new() -> Self {
        Self {
            chapters: Vec::new(),
            drift_hits: 0,
            last_code: C76Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C76Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn add_chapter(&mut self, ch: DocChapter) -> usize {
        self.chapters.push(ch);
        self.chapters.len()
    }

    /// 用户册四权文档齐备。
    pub fn rights_documented(&self) -> bool {
        self.chapters
            .iter()
            .filter(|c| c.book == BookKind::User)
            .any(|c| c.has_rights)
    }

    /// 开发者册 API 齐备。
    pub fn api_documented(&self) -> bool {
        self.chapters
            .iter()
            .filter(|c| c.book == BookKind::Dev)
            .any(|c| c.has_api)
    }

    /// 同源对账：两册都有内容即绿。
    pub fn reconcile(&mut self) -> Result<bool, C76Code> {
        let has_user = self.chapters.iter().any(|c| c.book == BookKind::User);
        let has_dev = self.chapters.iter().any(|c| c.book == BookKind::Dev);
        if has_user && has_dev {
            self.last_code = C76Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.drift_hits += 1;
            self.last_code = C76Code::DOC_DRIFT;
            self.has_error = true;
            Err(C76Code::DOC_DRIFT)
        }
    }

    pub fn chapter_count(&self) -> usize {
        self.chapters.len()
    }

    pub fn drift_hits(&self) -> u32 {
        self.drift_hits
    }
}

// ── 2 跳地图 ────────────────────────────────────────────────────────

/// 导航地图（2 跳复用红线）。
pub struct NavMap {
    /// (起点, 终点, 跳数)。
    routes: Vec<(u16, u16, u8)>,
    maze_hits: u32,
    last_code: C76Code,
    has_error: bool,
}

impl NavMap {
    pub fn new() -> Self {
        Self {
            routes: Vec::new(),
            maze_hits: 0,
            last_code: C76Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C76Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn add_route(&mut self, from: u16, to: u16, hops: u8) {
        self.routes.push((from, to, hops));
    }

    /// 2 跳断言：所有路由 ≤2 跳。
    pub fn assert_two_hop(&mut self) -> Result<bool, C76Code> {
        if self.routes.iter().all(|(_, _, h)| *h <= 2) {
            self.last_code = C76Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.maze_hits += 1;
            self.last_code = C76Code::MAZE;
            self.has_error = true;
            Err(C76Code::MAZE)
        }
    }

    pub fn route_count(&self) -> usize {
        self.routes.len()
    }

    pub fn maze_hits(&self) -> u32 {
        self.maze_hits
    }
}

// ── FAQ 复审 ────────────────────────────────────────────────────────

/// FAQ 条目。
#[derive(Clone, Copy)]
pub struct FaqEntry {
    pub id: u16,
    /// 是否在有效期内（复审通过）。
    pub reviewed: bool,
}

/// FAQ 复审器。
pub struct FaqReview {
    entries: Vec<FaqEntry>,
    stale_hits: u32,
    last_code: C76Code,
    has_error: bool,
}

impl FaqReview {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            stale_hits: 0,
            last_code: C76Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C76Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn add(&mut self, id: u16, reviewed: bool) {
        self.entries.push(FaqEntry { id, reviewed });
    }

    /// 复审审计：全部 reviewed。
    pub fn audit(&mut self) -> Result<bool, C76Code> {
        if self.entries.iter().all(|e| e.reviewed) {
            self.last_code = C76Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.stale_hits += 1;
            self.last_code = C76Code::FAQ_STALE;
            self.has_error = true;
            Err(C76Code::FAQ_STALE)
        }
    }

    /// 补审未过的。
    pub fn review_all(&mut self) -> usize {
        let mut n = 0;
        for e in &mut self.entries {
            if !e.reviewed {
                e.reviewed = true;
                n += 1;
            }
        }
        n
    }

    pub fn stale_hits(&self) -> u32 {
        self.stale_hits
    }

    pub fn count(&self) -> usize {
        self.entries.len()
    }
}

// ── 家族管道 ────────────────────────────────────────────────────────

/// 家族管道（复述）。
pub struct FamilyPipeline {
    stage_count: u8,
}

impl FamilyPipeline {
    pub fn new(stage_count: u8) -> Self {
        Self { stage_count }
    }

    /// 管道完整性：至少两段（用户册+开发者册）。
    pub fn intact(&self) -> bool {
        self.stage_count >= 2
    }

    pub fn stage_count(&self) -> u8 {
        self.stage_count
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves76_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves76_prefdocs");
    {
        let g = PrivacyGate::new();
        chk(&mut set, "C3676-01", g.all_registered() && g.published_count() == 0, "err");
    }
    {
        let mut g = PrivacyGate::new();
        let res = g.publish(1);
        chk(&mut set, "C3676-02", res.err() == Some(C76Code::UNREGISTERED), "err");
    }
    {
        let mut g = PrivacyGate::new();
        g.register(1, PrivacyLevel::Public);
        let res = g.publish(1);
        chk(&mut set, "C3676-03", res == Ok(true) && g.published_count() == 1, "err");
    }
    {
        let mut g = PrivacyGate::new();
        g.register(1, PrivacyLevel::Sensitive);
        chk(&mut set, "C3676-04", g.all_registered(), "err");
    }
    {
        let mut d = DocPair::new();
        d.add_chapter(DocChapter { book: BookKind::User, has_rights: true, has_api: false });
        d.add_chapter(DocChapter { book: BookKind::Dev, has_rights: false, has_api: true });
        chk(
            &mut set,
            "C3676-05",
            d.rights_documented() && d.api_documented() && d.reconcile() == Ok(true),
            "err",
        );
    }
    {
        let mut d = DocPair::new();
        d.add_chapter(DocChapter { book: BookKind::User, has_rights: true, has_api: false });
        let res = d.reconcile();
        chk(&mut set, "C3676-06", res.err() == Some(C76Code::DOC_DRIFT), "err");
    }
    {
        let mut n = NavMap::new();
        n.add_route(1, 2, 1);
        n.add_route(1, 3, 2);
        let res = n.assert_two_hop();
        chk(&mut set, "C3676-07", res == Ok(true), "err");
    }
    {
        let mut n = NavMap::new();
        n.add_route(1, 9, 5);
        let res = n.assert_two_hop();
        chk(&mut set, "C3676-08", res.err() == Some(C76Code::MAZE), "err");
    }
    {
        let mut f = FaqReview::new();
        f.add(1, true);
        f.add(2, true);
        chk(&mut set, "C3676-09", f.audit() == Ok(true), "err");
    }
    {
        let mut f = FaqReview::new();
        f.add(1, true);
        f.add(2, false);
        let res = f.audit();
        chk(&mut set, "C3676-10", res.err() == Some(C76Code::FAQ_STALE), "err");
    }
    {
        let mut f = FaqReview::new();
        f.add(1, false);
        f.add(2, false);
        let n = f.review_all();
        chk(&mut set, "C3676-11", n == 2 && f.audit() == Ok(true), "err");
    }
    {
        let p = FamilyPipeline::new(2);
        let p2 = FamilyPipeline::new(1);
        chk(&mut set, "C3676-12", p.intact() && !p2.intact(), "err");
    }
    set
}
