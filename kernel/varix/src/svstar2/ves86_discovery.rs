//! VE-F3686 · 编辑推荐与发现（VE-S 域 · 批次 R05 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3686`
//!
//! **职责定位（锚点原文）**：编辑推荐与发现——
//! - **发现渠道**：编辑精选 / 分类导航 / 搜索 / 个性化推荐四渠道（复用 F3666 推荐原则）；
//! - **编辑推荐**：人工精选 + 标注（商业推广伪装编辑推荐 = 欺骗——伪装红线 P0）；
//! - **搜索公平**：搜索排序不卖位（竞价排名 = 生态毒瘤——卖位禁令 P0）；
//! - **新创作者曝光**：新晋扶持位（复述多样性）。
//!
//! **判据（锚点原文）**：四渠道、标注断言、纯净断言、扶持位、伪装 P0、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 伪装检出→P0（红线实测——推广标识审计） | [`C86Code::FAKE_EDITORS_PICK`] |
//! | 卖位检出→P0（红线实测——排序独立性） | [`C86Code::PAID_RANKING`] |
//! | 标准黑箱→公开（复述） | [`C86Code::CRITERIA_HIDDEN`] |
//! | 扶持失效→多样断言（复用） | [`C86Code::NEWBIE_STARVED`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3686 诊断码段（独占 0x86xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C86Code(pub u16);

impl C86Code {
    /// 推广伪装编辑推荐（P0 红线）。
    pub const FAKE_EDITORS_PICK: C86Code = C86Code(0x8601);
    /// 搜索卖位（P0 红线）。
    pub const PAID_RANKING: C86Code = C86Code(0x8602);
    /// 标准黑箱。
    pub const CRITERIA_HIDDEN: C86Code = C86Code(0x8603);
    /// 扶持失效。
    pub const NEWBIE_STARVED: C86Code = C86Code(0x8604);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C86Code::FAKE_EDITORS_PICK => "商业推广伪装编辑推荐（P0 红线实测）。建议：显著标注。",
            C86Code::PAID_RANKING => "搜索卖位（P0 红线实测）。建议：排序独立性修复。",
            C86Code::CRITERIA_HIDDEN => "推荐标准黑箱。建议：公开标准。",
            C86Code::NEWBIE_STARVED => "新晋扶持失效。建议：补扶持位。",
            _ => "未知诊断码。",
        }
    }
}

// ── 四渠道 ──────────────────────────────────────────────────────────

/// 发现渠道。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Channel {
    /// 编辑精选。
    EditorsPick,
    /// 分类导航。
    Category,
    /// 搜索。
    Search,
    /// 个性化推荐。
    Personalized,
}

impl Channel {
    pub const ALL: [Channel; 4] = [
        Channel::EditorsPick,
        Channel::Category,
        Channel::Search,
        Channel::Personalized,
    ];
}

/// 渠道注册表。
pub struct ChannelMap {
    mapped: Vec<Channel>,
}

impl ChannelMap {
    pub fn new() -> Self {
        Self { mapped: Vec::new() }
    }

    pub fn map(&mut self, c: Channel) {
        if !self.mapped.contains(&c) {
            self.mapped.push(c);
        }
    }

    /// 四渠道齐备。
    pub fn complete(&self) -> bool {
        Channel::ALL.iter().all(|c| self.mapped.contains(c))
    }

    pub fn mapped_count(&self) -> usize {
        self.mapped.len()
    }
}

// ── 标注断言 ────────────────────────────────────────────────────────

/// 推荐位内容。
#[derive(Clone, Copy)]
pub struct PickItem {
    pub id: u16,
    /// 是否商业推广。
    pub is_promo: bool,
    /// 是否显著标注。
    pub labeled: bool,
    /// 编辑标准是否公开。
    pub criteria_public: bool,
}

/// 标注断言器（伪装红线 P0）。
pub struct LabelAssert {
    items: Vec<PickItem>,
    fake_hits: u32,
    hidden_hits: u32,
    last_code: C86Code,
    has_error: bool,
}

impl LabelAssert {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            fake_hits: 0,
            hidden_hits: 0,
            last_code: C86Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C86Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn add(&mut self, item: PickItem) -> usize {
        self.items.push(item);
        self.items.len()
    }

    /// 伪装审计：推广必须标注。
    pub fn audit_label(&mut self) -> Result<bool, C86Code> {
        let mut ok = true;
        for it in &self.items {
            if it.is_promo && !it.labeled {
                ok = false;
                break;
            }
        }
        if ok {
            self.last_code = C86Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fake_hits += 1;
            self.last_code = C86Code::FAKE_EDITORS_PICK;
            self.has_error = true;
            Err(C86Code::FAKE_EDITORS_PICK)
        }
    }

    /// 标准公开审计。
    pub fn audit_criteria(&mut self) -> Result<bool, C86Code> {
        if self.items.iter().all(|i| i.criteria_public) {
            self.last_code = C86Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.hidden_hits += 1;
            self.last_code = C86Code::CRITERIA_HIDDEN;
            self.has_error = true;
            Err(C86Code::CRITERIA_HIDDEN)
        }
    }

    pub fn fake_hits(&self) -> u32 {
        self.fake_hits
    }

    pub fn hidden_hits(&self) -> u32 {
        self.hidden_hits
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }
}

// ── 搜索纯净 ────────────────────────────────────────────────────────

/// 排序条目。
#[derive(Clone, Copy)]
pub struct RankEntry {
    pub id: u16,
    /// 是否付费买位。
    pub paid: bool,
    /// 相关性分。
    pub relevance: u16,
}

/// 搜索纯净断言（卖位禁令 P0）。
pub struct SearchPurity {
    entries: Vec<RankEntry>,
    paid_hits: u32,
    last_code: C86Code,
    has_error: bool,
}

impl SearchPurity {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            paid_hits: 0,
            last_code: C86Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C86Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn add(&mut self, e: RankEntry) {
        self.entries.push(e);
    }

    /// 纯净断言：无付费买位。
    pub fn assert_pure(&mut self) -> Result<bool, C86Code> {
        if self.entries.iter().all(|e| !e.paid) {
            self.last_code = C86Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.paid_hits += 1;
            self.last_code = C86Code::PAID_RANKING;
            self.has_error = true;
            Err(C86Code::PAID_RANKING)
        }
    }

    /// 按相关性排序（纯净排序）。
    pub fn sorted_ids(&self) -> Vec<u16> {
        let mut v: Vec<&RankEntry> = self.entries.iter().collect();
        v.sort_by(|a, b| b.relevance.cmp(&a.relevance));
        v.iter().map(|e| e.id).collect()
    }

    pub fn paid_hits(&self) -> u32 {
        self.paid_hits
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

// ── 新晋扶持 ────────────────────────────────────────────────────────

/// 扶持位分配器（多样性复用）。
pub struct NewbieBoost {
    /// 总曝光位。
    total_slots: u16,
    /// 已给新晋的位。
    newbie_slots: u16,
    starve_hits: u32,
    last_code: C86Code,
    has_error: bool,
}

impl NewbieBoost {
    pub fn new(total_slots: u16) -> Self {
        Self {
            total_slots,
            newbie_slots: 0,
            starve_hits: 0,
            last_code: C86Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C86Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn give(&mut self, n: u16) {
        self.newbie_slots += n;
    }

    /// 多样断言：新晋位 ≥10%。
    pub fn assert_diverse(&mut self) -> Result<bool, C86Code> {
        if self.total_slots == 0 || self.newbie_slots * 10 >= self.total_slots {
            self.last_code = C86Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.starve_hits += 1;
            self.last_code = C86Code::NEWBIE_STARVED;
            self.has_error = true;
            Err(C86Code::NEWBIE_STARVED)
        }
    }

    pub fn starve_hits(&self) -> u32 {
        self.starve_hits
    }

    pub fn newbie_ratio_bp(&self) -> u16 {
        if self.total_slots == 0 {
            return 0;
        }
        (self.newbie_slots as u32 * 10000 / self.total_slots as u32) as u16
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves86_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves86_discovery");
    {
        let mut m = ChannelMap::new();
        for c in Channel::ALL {
            m.map(c);
        }
        chk(&mut set, "C3686-01", m.complete() && m.mapped_count() == 4, "err");
    }
    {
        let mut m = ChannelMap::new();
        m.map(Channel::Search);
        chk(&mut set, "C3686-02", !m.complete(), "err");
    }
    {
        let mut a = LabelAssert::new();
        a.add(PickItem { id: 1, is_promo: true, labeled: true, criteria_public: true });
        chk(&mut set, "C3686-03", a.audit_label() == Ok(true), "err");
    }
    {
        let mut a = LabelAssert::new();
        a.add(PickItem { id: 1, is_promo: true, labeled: false, criteria_public: true });
        let res = a.audit_label();
        chk(&mut set, "C3686-04", res.err() == Some(C86Code::FAKE_EDITORS_PICK), "err");
    }
    {
        let mut a = LabelAssert::new();
        a.add(PickItem { id: 1, is_promo: false, labeled: false, criteria_public: true });
        chk(&mut set, "C3686-05", a.audit_label() == Ok(true), "err");
    }
    {
        let mut a = LabelAssert::new();
        a.add(PickItem { id: 1, is_promo: false, labeled: true, criteria_public: false });
        let res = a.audit_criteria();
        chk(&mut set, "C3686-06", res.err() == Some(C86Code::CRITERIA_HIDDEN), "err");
    }
    {
        let mut s = SearchPurity::new();
        s.add(RankEntry { id: 1, paid: false, relevance: 90 });
        s.add(RankEntry { id: 2, paid: false, relevance: 80 });
        chk(&mut set, "C3686-07", s.assert_pure() == Ok(true), "err");
    }
    {
        let mut s = SearchPurity::new();
        s.add(RankEntry { id: 1, paid: true, relevance: 99 });
        let res = s.assert_pure();
        chk(&mut set, "C3686-08", res.err() == Some(C86Code::PAID_RANKING), "err");
    }
    {
        let mut s = SearchPurity::new();
        s.add(RankEntry { id: 2, paid: false, relevance: 50 });
        s.add(RankEntry { id: 1, paid: false, relevance: 90 });
        let ids = s.sorted_ids();
        chk(&mut set, "C3686-09", ids.len() == 2 && ids[0] == 1, "err");
    }
    {
        let mut n = NewbieBoost::new(100);
        n.give(15);
        chk(&mut set, "C3686-10", n.assert_diverse() == Ok(true), "err");
    }
    {
        let mut n = NewbieBoost::new(100);
        n.give(2);
        let res = n.assert_diverse();
        chk(&mut set, "C3686-11", res.err() == Some(C86Code::NEWBIE_STARVED), "err");
    }
    {
        let mut n = NewbieBoost::new(50);
        n.give(5);
        chk(&mut set, "C3686-12", n.newbie_ratio_bp() == 1000, "err");
    }
    set
}
