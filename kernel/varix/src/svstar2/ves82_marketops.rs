//! VE-F3682 · 主题市场运营深化（VE-S 域 · 批次 R05 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3682`
//!
//! **职责定位（锚点原文）**：主题市场运营深化——
//! - **运营指标**：DAU / 转化 / 留存 / 满意度四指标（口径公开——口径暗改 = 数据作恶，
//!   口径变更 ADR）；
//! - **运营活动**：促销 / 专题 / 新档期（活动前评估——活动扭曲生态 = 短期繁荣长期伤）；
//! - **库存管理**：下架资产用户已装处理（下架 ≠ 卸载——存量红线）；
//! - **定价指导**：创作者定价参考（定价自由——复述主权）。
//!
//! **判据（锚点原文）**：口径红线、活动评估前置、存量红线、定价主权、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 口径暗改→P0（红线实测——变更日志审计） | [`C82Code::METRIC_TAMPER`] |
//! | 活动无评估→拦截（红线实测） | [`C82Code::ACTIVITY_SKIP`] |
//! | 下架断供→P1（存量红线实测） | [`C82Code::STOCK_CUT`] |
//! | 指导强推→主权纠正（复述） | [`C82Code::PRICE_FORCE`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3682 诊断码段（独占 0x82xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C82Code(pub u16);

impl C82Code {
    /// 口径暗改（P0 红线实测）。
    pub const METRIC_TAMPER: C82Code = C82Code(0x8201);
    /// 活动无评估。
    pub const ACTIVITY_SKIP: C82Code = C82Code(0x8202);
    /// 下架断供（P1 存量红线）。
    pub const STOCK_CUT: C82Code = C82Code(0x8203);
    /// 定价强推。
    pub const PRICE_FORCE: C82Code = C82Code(0x8204);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C82Code::METRIC_TAMPER => "指标口径暗改（P0 红线实测）。建议：变更走 ADR。",
            C82Code::ACTIVITY_SKIP => "活动无前置评估。建议：拦截上线。",
            C82Code::STOCK_CUT => "下架断供已装资产（P1 存量红线）。建议：已装继续可用。",
            C82Code::PRICE_FORCE => "定价指导强推。建议：纠正为参考。",
            _ => "未知诊断码。",
        }
    }
}

// ── 四指标（口径 ADR） ─────────────────────────────────────────────

/// 运营指标。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Metric {
    /// 日活。
    Dau,
    /// 转化。
    Conversion,
    /// 留存。
    Retention,
    /// 满意度。
    Satisfaction,
}

impl Metric {
    pub const ALL: [Metric; 4] =
        [Metric::Dau, Metric::Conversion, Metric::Retention, Metric::Satisfaction];
}

/// 口径记录（ADR）。
#[derive(Clone, Copy)]
pub struct MetricDef {
    pub metric: Metric,
    /// 当前口径版本。
    pub version: u8,
    /// 变更是否走 ADR。
    pub adr_ok: bool,
}

/// 指标口径管理（口径红线）。
pub struct MetricRegistry {
    defs: Vec<MetricDef>,
    tamper_hits: u32,
    last_code: C82Code,
    has_error: bool,
}

impl MetricRegistry {
    pub fn new() -> Self {
        Self {
            defs: Vec::new(),
            tamper_hits: 0,
            last_code: C82Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C82Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 登记口径。
    pub fn register(&mut self, metric: Metric, version: u8, adr_ok: bool) -> Result<bool, C82Code> {
        if !adr_ok && self.defs.iter().any(|d| d.metric == metric) {
            self.tamper_hits += 1;
            self.last_code = C82Code::METRIC_TAMPER;
            self.has_error = true;
            return Err(C82Code::METRIC_TAMPER);
        }
        if let Some(d) = self.defs.iter_mut().find(|d| d.metric == metric) {
            d.version = version;
            d.adr_ok = adr_ok;
        } else {
            self.defs.push(MetricDef { metric, version, adr_ok });
        }
        self.last_code = C82Code(0);
        self.has_error = false;
        Ok(true)
    }

    /// 四指标齐备。
    pub fn all_registered(&self) -> bool {
        Metric::ALL.iter().all(|m| self.defs.iter().any(|d| d.metric == *m))
    }

    pub fn tamper_hits(&self) -> u32 {
        self.tamper_hits
    }

    pub fn def_count(&self) -> usize {
        self.defs.len()
    }
}

// ── 活动评估流 ──────────────────────────────────────────────────────

/// 运营活动。
#[derive(Clone, Copy)]
pub struct Activity {
    pub id: u16,
    /// 是否通过前置评估。
    pub evaluated: bool,
    /// 评估得分（0-100）。
    pub score: u8,
}

/// 活动评估器（活动评估前置）。
pub struct ActivityGate {
    activities: Vec<Activity>,
    skip_hits: u32,
    last_code: C82Code,
    has_error: bool,
}

impl ActivityGate {
    pub fn new() -> Self {
        Self {
            activities: Vec::new(),
            skip_hits: 0,
            last_code: C82Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C82Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 活动上线闸：必须前置评估且得分 ≥60。
    pub fn approve(&mut self, a: Activity) -> Result<bool, C82Code> {
        if !a.evaluated {
            self.skip_hits += 1;
            self.last_code = C82Code::ACTIVITY_SKIP;
            self.has_error = true;
            return Err(C82Code::ACTIVITY_SKIP);
        }
        self.activities.push(a);
        self.last_code = C82Code(0);
        self.has_error = false;
        Ok(a.score >= 60)
    }

    pub fn approved_count(&self) -> usize {
        self.activities.len()
    }

    pub fn skip_hits(&self) -> u32 {
        self.skip_hits
    }
}

// ── 存量协议 ────────────────────────────────────────────────────────

/// 已装资产状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StockState {
    /// 在架。
    Live,
    /// 下架但仍可用。
    DelistedUsable,
    /// 断供（违规）。
    CutOff,
}

/// 存量协议（下架 ≠ 卸载）。
pub struct StockProtocol {
    /// (资产 ID, 状态)。
    assets: Vec<(u16, StockState)>,
    cut_hits: u32,
    last_code: C82Code,
    has_error: bool,
}

impl StockProtocol {
    pub fn new() -> Self {
        Self {
            assets: Vec::new(),
            cut_hits: 0,
            last_code: C82Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C82Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn set_state(&mut self, id: u16, state: StockState) {
        if let Some(e) = self.assets.iter_mut().find(|(x, _)| *x == id) {
            e.1 = state;
        } else {
            self.assets.push((id, state));
        }
    }

    /// 存量断言：已装资产不得断供。
    pub fn assert_stock(&mut self) -> Result<bool, C82Code> {
        if self.assets.iter().any(|(_, s)| *s == StockState::CutOff) {
            self.cut_hits += 1;
            self.last_code = C82Code::STOCK_CUT;
            self.has_error = true;
            Err(C82Code::STOCK_CUT)
        } else {
            self.last_code = C82Code(0);
            self.has_error = false;
            Ok(true)
        }
    }

    pub fn cut_hits(&self) -> u32 {
        self.cut_hits
    }

    pub fn usable_count(&self) -> usize {
        self.assets
            .iter()
            .filter(|(_, s)| *s == StockState::Live || *s == StockState::DelistedUsable)
            .count()
    }
}

// ── 定价主权 ────────────────────────────────────────────────────────

/// 定价指导（参考不强推）。
pub struct PriceGuide {
    /// 建议价。
    pub suggested: i32,
    /// 创作者定价。
    pub creator_price: i32,
    force_hits: u32,
    last_code: C82Code,
    has_error: bool,
}

impl PriceGuide {
    pub fn new(suggested: i32) -> Self {
        Self {
            suggested,
            creator_price: 0,
            force_hits: 0,
            last_code: C82Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C82Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 创作者定价（主权）。
    pub fn set_price(&mut self, price: i32) {
        self.creator_price = price;
    }

    /// 强推检测：定价与建议不同却报强推。
    pub fn note_force(&mut self) {
        self.force_hits += 1;
        self.last_code = C82Code::PRICE_FORCE;
        self.has_error = true;
    }

    /// 主权断言：creator_price 被覆盖为 suggested 即强推。
    pub fn assert_free(&mut self) -> Result<bool, C82Code> {
        if self.creator_price == self.suggested && self.force_hits > 0 {
            self.last_code = C82Code::PRICE_FORCE;
            self.has_error = true;
            Err(C82Code::PRICE_FORCE)
        } else {
            self.last_code = C82Code(0);
            self.has_error = false;
            Ok(true)
        }
    }

    pub fn force_hits(&self) -> u32 {
        self.force_hits
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves82_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves82_marketops");
    {
        let mut r = MetricRegistry::new();
        r.register(Metric::Dau, 1, true);
        r.register(Metric::Conversion, 1, true);
        r.register(Metric::Retention, 1, true);
        r.register(Metric::Satisfaction, 1, true);
        chk(&mut set, "C3682-01", r.all_registered(), "err");
    }
    {
        let mut r = MetricRegistry::new();
        r.register(Metric::Dau, 1, true);
        let res = r.register(Metric::Dau, 2, false);
        chk(&mut set, "C3682-02", res.err() == Some(C82Code::METRIC_TAMPER), "err");
    }
    {
        let mut r = MetricRegistry::new();
        r.register(Metric::Dau, 1, true);
        let res = r.register(Metric::Dau, 2, true);
        chk(&mut set, "C3682-03", res == Ok(true) && r.def_count() == 1, "err");
    }
    {
        let mut g = ActivityGate::new();
        let res = g.approve(Activity { id: 1, evaluated: true, score: 80 });
        chk(&mut set, "C3682-04", res == Ok(true) && g.approved_count() == 1, "err");
    }
    {
        let mut g = ActivityGate::new();
        let res = g.approve(Activity { id: 1, evaluated: false, score: 0 });
        chk(&mut set, "C3682-05", res.err() == Some(C82Code::ACTIVITY_SKIP), "err");
    }
    {
        let mut g = ActivityGate::new();
        let res = g.approve(Activity { id: 1, evaluated: true, score: 30 });
        chk(&mut set, "C3682-06", res == Ok(false), "err");
    }
    {
        let mut s = StockProtocol::new();
        s.set_state(1, StockState::Live);
        s.set_state(2, StockState::DelistedUsable);
        chk(&mut set, "C3682-07", s.assert_stock() == Ok(true) && s.usable_count() == 2, "err");
    }
    {
        let mut s = StockProtocol::new();
        s.set_state(1, StockState::CutOff);
        let res = s.assert_stock();
        chk(&mut set, "C3682-08", res.err() == Some(C82Code::STOCK_CUT), "err");
    }
    {
        let mut p = PriceGuide::new(50);
        p.set_price(80);
        chk(&mut set, "C3682-09", p.assert_free() == Ok(true), "err");
    }
    {
        let mut p = PriceGuide::new(50);
        p.set_price(50);
        p.note_force();
        let res = p.assert_free();
        chk(&mut set, "C3682-10", res.err() == Some(C82Code::PRICE_FORCE), "err");
    }
    {
        let mut r = MetricRegistry::new();
        r.register(Metric::Dau, 1, true);
        chk(&mut set, "C3682-11", !r.all_registered(), "err");
    }
    {
        let mut s = StockProtocol::new();
        s.set_state(1, StockState::DelistedUsable);
        chk(&mut set, "C3682-12", s.usable_count() == 1 && s.assert_stock() == Ok(true), "err");
    }
    set
}
