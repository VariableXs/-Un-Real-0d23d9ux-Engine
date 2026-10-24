//! VE-F3758 · 创作无障碍市场
//!
//! 锚点：创作无障碍市场（F3159 模式创作版复刻：市场标注（无障碍创作资产
//! 市场标注（标注红线复述：无标注=不可上架（上架硬门）；市场分类（无障碍
//! 资产分类筛选（分类复述）；市场评价（无障碍维度评价（评价复述）；
//! 市场推荐（无障碍资产优先推荐（推荐复述）。
//!
//! 数据结构：标注硬门；分类复述；评价复述；推荐复述。
//! 错误路径与降级矩阵：无标注→拒上架（硬门实测）；分类缺→补（复述红线）；
//! 评价缺→补（复述）；推荐不公→修正（复述红线）。
//! 跨批对接点：F3159/F3175 单源复用声明；F3756 资产对端；F3760 收口对端。
//!
//! 诊断码段：独占 0x95xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x95xx）
// ─────────────────────────────────────────────────────────────

/// F3758 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct M8Code(pub u16);

impl M8Code {
    /// 无标注上架——硬门。
    pub const UNLABELED_LISTING: M8Code = M8Code(0x9501);
    /// 分类缺失——分类复述。
    pub const CATEGORY_MISS: M8Code = M8Code(0x9502);
    /// 评价缺失——评价复述。
    pub const REVIEW_MISS: M8Code = M8Code(0x9503);
    /// 推荐不公——推荐复述红线。
    pub const RECOMMEND_UNFAIR: M8Code = M8Code(0x9504);
    /// 标注失实——证据红线。
    pub const LABEL_FALSE: M8Code = M8Code(0x9505);
    /// 筛选无效——筛选复述。
    pub const FILTER_BROKEN: M8Code = M8Code(0x9506);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x9501 => "无无障碍标注即上架，硬门拦截。",
            0x9502 => "无障碍分类缺失，分类复述不完整。",
            0x9503 => "无障碍维度评价缺失，评价复述不完整。",
            0x9504 => "推荐不公平，无障碍资产未优先，推荐红线不通过。",
            0x9505 => "标注失实，证据红线不通过。",
            0x9506 => "无障碍筛选无效，筛选复述不完整。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 市场资产与标注硬门
// ─────────────────────────────────────────────────────────────

/// 市场资产。
#[derive(Clone, Debug)]
pub struct MarketAsset {
    /// 资产 id。
    pub id: u32,
    /// 无障碍标注。
    pub a11y_labeled: bool,
    /// 标注是否有效（有证据）。
    pub label_valid: bool,
    /// 无障碍分类 id（0=未分类）。
    pub a11y_category: u32,
    /// 无障碍评价分（0-100，0=未评价）。
    pub a11y_score: u8,
    /// 是否已上架。
    pub listed: bool,
}

impl MarketAsset {
    /// 构造。
    pub fn new(id: u32) -> MarketAsset {
        MarketAsset {
            id,
            a11y_labeled: false,
            label_valid: false,
            a11y_category: 0,
            a11y_score: 0,
            listed: false,
        }
    }

    /// 上架硬门合规：有标注 + 标注有效。
    pub fn can_list(&self) -> bool {
        self.a11y_labeled && self.label_valid
    }

    /// 尝试上架。
    pub fn list(&mut self) -> bool {
        if !self.can_list() {
            return false;
        }
        self.listed = true;
        true
    }

    /// 分类合规。
    pub fn category_ok(&self) -> bool {
        self.a11y_category != 0
    }

    /// 评价合规。
    pub fn review_ok(&self) -> bool {
        self.a11y_score > 0
    }
}

// ─────────────────────────────────────────────────────────────
// 市场分类与筛选
// ─────────────────────────────────────────────────────────────

/// 分类表。
#[derive(Clone, Debug, Default)]
pub struct CategoryTable {
    /// 已注册分类数。
    registered: u32,
}

impl CategoryTable {
    /// 构造。
    pub fn new() -> CategoryTable {
        CategoryTable { registered: 0 }
    }

    /// 注册分类。
    pub fn register(&mut self) -> u32 {
        self.registered += 1;
        self.registered
    }

    /// 分类数。
    pub fn count(&self) -> u32 {
        self.registered
    }

    /// 至少有一个无障碍分类。
    pub fn has_a11y(&self) -> bool {
        self.registered > 0
    }
}

/// 筛选器。
#[derive(Clone, Debug, Default)]
pub struct A11yFilter {
    /// 是否启用无障碍筛选。
    pub enabled: bool,
    /// 筛选结果数。
    pub results: u32,
}

impl A11yFilter {
    /// 构造。
    pub fn new() -> A11yFilter {
        A11yFilter {
            enabled: false,
            results: 0,
        }
    }

    /// 启用筛选。
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// 筛选合规：启用 + 有结果。
    pub fn ok(&self) -> bool {
        self.enabled && self.results > 0
    }
}

// ─────────────────────────────────────────────────────────────
// 市场评价与推荐
// ─────────────────────────────────────────────────────────────

/// 评价维度。
#[derive(Clone, Debug, Default)]
pub struct A11yReview {
    /// 评价条数。
    pub count: u32,
    /// 平均分（万分制）。
    pub avg_bp: u16,
}

impl A11yReview {
    /// 构造。
    pub fn new() -> A11yReview {
        A11yReview {
            count: 0,
            avg_bp: 0,
        }
    }

    /// 添加评价。
    pub fn add(&mut self, score_bp: u16) {
        let total = self.avg_bp as u64 * self.count as u64 + score_bp as u64;
        self.count += 1;
        self.avg_bp = (total / self.count as u64) as u16;
    }

    /// 合规：有评价。
    pub fn ok(&self) -> bool {
        self.count > 0
    }
}

/// 推荐引擎。
#[derive(Clone, Debug, Default)]
pub struct RecommendEngine {
    /// 无障碍资产优先权重（万分制）。
    pub a11y_weight_bp: u16,
}

impl RecommendEngine {
    /// 构造（默认 0）。
    pub fn new() -> RecommendEngine {
        RecommendEngine {
            a11y_weight_bp: 0,
        }
    }

    /// 合规：无障碍权重 > 0（优先推荐）。
    pub fn fair(&self) -> bool {
        self.a11y_weight_bp > 0
    }

    /// 设权重。
    pub fn set_weight(&mut self, bp: u16) {
        self.a11y_weight_bp = bp;
    }
}

// ─────────────────────────────────────────────────────────────
// 主编排器
// ─────────────────────────────────────────────────────────────

/// 创作无障碍市场审查器。
#[derive(Clone, Debug, Default)]
pub struct MarketAuditor {
    assets: Vec<MarketAsset>,
    categories: CategoryTable,
    filter: A11yFilter,
    review: A11yReview,
    recommend: RecommendEngine,
}

impl MarketAuditor {
    /// 构造。
    pub fn new() -> MarketAuditor {
        MarketAuditor {
            assets: Vec::new(),
            categories: CategoryTable::new(),
            filter: A11yFilter::new(),
            review: A11yReview::new(),
            recommend: RecommendEngine::new(),
        }
    }

    /// 入场（可变引用回调式）。
    pub fn add_asset(&mut self, a: MarketAsset) {
        self.assets.push(a);
    }

    /// 取分类（可变）。
    pub fn categories_mut(&mut self) -> &mut CategoryTable {
        &mut self.categories
    }

    /// 取筛选（可变）。
    pub fn filter_mut(&mut self) -> &mut A11yFilter {
        &mut self.filter
    }

    /// 取评价（可变）。
    pub fn review_mut(&mut self) -> &mut A11yReview {
        &mut self.review
    }

    /// 取推荐（可变）。
    pub fn recommend_mut(&mut self) -> &mut RecommendEngine {
        &mut self.recommend
    }

    /// 全量审查。
    pub fn audit(&self) -> Vec<M8Code> {
        let mut bad = Vec::new();
        let unlisted = self.assets.iter().filter(|a| a.listed && !a.can_list()).count();
        if unlisted > 0 {
            bad.push(M8Code::UNLABELED_LISTING);
        }
        if !self.categories.has_a11y() {
            bad.push(M8Code::CATEGORY_MISS);
        }
        if !self.review.ok() {
            bad.push(M8Code::REVIEW_MISS);
        }
        if !self.recommend.fair() {
            bad.push(M8Code::RECOMMEND_UNFAIR);
        }
        if !self.filter.ok() {
            bad.push(M8Code::FILTER_BROKEN);
        }
        bad
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3758 判据集。
pub fn run_vr08r_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3758");

    // 0x9501 上架硬门
    {
        let mut a = MarketAsset::new(1);
        out.add("0x9501-无标注拒上架", !a.list() && !a.listed, "");
        a.a11y_labeled = true;
        a.label_valid = true;
        out.add("0x9501-标注后可上架", a.list() && a.listed, "");
    }

    // 0x9502 分类
    {
        let mut c = CategoryTable::new();
        out.add("0x9502-无分类拦", !c.has_a11y(), "");
        c.register();
        out.add("0x9502-有分类过", c.has_a11y(), "");
    }

    // 0x9503 评价
    {
        let mut r = A11yReview::new();
        out.add("0x9503-无评价拦", !r.ok(), "");
        r.add(8000);
        out.add("0x9503-有评价过", r.ok(), "");
    }

    // 0x9504 推荐公平
    {
        let mut e = RecommendEngine::new();
        out.add("0x9504-零权重拦", !e.fair(), "");
        e.set_weight(1000);
        out.add("0x9504-有权重过", e.fair(), "");
    }

    // 0x9506 筛选
    {
        let mut f = A11yFilter::new();
        out.add("0x9506-未启用拦", !f.ok(), "");
        f.enable();
        out.add("0x9506-无结果拦", !f.ok(), "");
        f.results = 5;
        out.add("0x9506-有结果过", f.ok(), "");
    }

    out
}
