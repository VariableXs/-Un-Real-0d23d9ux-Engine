//! F209 错误提示文案三要素 · 判据实装（H 基础通用域）。
//!
//! **判据锚**：主册 F209「错误提示文案三要素」。
//!
//! **验收标准（主册第一句）**：全系统错误文案清单化审计（每条标注三要素
//! 齐/缺）；缺要素项清零后才可发布。
//!
//! **判据要点（主册）**：错误提示只许一个格式——【发生了什么】+【为什么】
//! +【现在能做什么】；「无法打开文件（格式不支持，可安装 .7z 解码器或在
//! 应用内另存为 .zip）」合格；「操作失败」不合格；**禁止没有出路的错误
//! 框**；技术错误码折叠在「详细信息」展开区（默认收起）；图标、标题、
//! 正文层级按乙字号四档排（抽查 20 条对基线表）。
//!
//! **设计要点**：
//! - 三要素承载 [`ThreeParts`]：what/why/next 三字段**非空校验**是硬门；
//!   构造器 [`ErrorBuilder`] 在 `build()` 强制三齐（缺一即拒）——「没有
//!   出路的错误框」在构造期被拒绝，而不是靠审计事后兜底；
//! - 技术错误码 + 详细文本进 [`FoldState`] 折叠区：默认收起，展开后错误
//!   码才可见——[`ErrorCard::render_layout`] 产出图标/标题/正文/行动/
//!   折叠提示/明细六种角色的行，折叠态不渲染明细行；
//! - 字号基线 [`FontTier`] 乙字号四档（20/16/14/12 px），基线表常量
//!   [`FONT_BASELINE_PX`]，[`ErrorFaceRegistry::font_sample_audit`] 逐条
//!   对表（抽查上限 [`FONT_SAMPLE_N`] = 20 条，主册「抽查 20 条」）；
//! - 审计器 [`ErrorFaceRegistry`]：登记制（容量 [`AUDIT_CAP`]、满则逐出
//!   最旧），逐条产出 [`AuditRow`]（每条标注三要素齐/缺），缺要素清单
//!   [`ErrorFaceRegistry::missing_ids`] 非空即未达发布门。
//!
//! **依赖锚点**：`crate::checks::CheckSet`（自检面）。时间注入式：折叠
//! 交互毫秒戳由调用方给，本模块不持时钟。零堆热路径纪律：非空校验与
//! 折叠切换不分配；Vec/String 只用于登记册与渲染面且有容量上限。

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 乙字号一档 20px——主册 F209：「图标、标题、正文层级按乙字号四档排」
/// （四档取值 20/16/14/12 为乙字号表实装定值）。
pub const TIER1_PX: u16 = 20;

/// 乙字号二档 16px（小标题/强调行）。
pub const TIER2_PX: u16 = 16;

/// 乙字号三档 14px（正文）。
pub const TIER3_PX: u16 = 14;

/// 乙字号四档 12px（辅助信息/折叠提示）。
pub const TIER4_PX: u16 = 12;

/// 乙字号四档基线表（档位序号 → px）——主册 F209 字号抽查的对表基准。
pub const FONT_BASELINE_PX: [u16; 4] = [TIER1_PX, TIER2_PX, TIER3_PX, TIER4_PX];

/// 字号抽查条数上限——主册 F209：「文案字号逐条对基线表（抽查 20 条）」。
pub const FONT_SAMPLE_N: usize = 20;

/// 错误框图标边长 px（乙字号表图标档实装定值）。
pub const ICON_PX: u16 = 24;

/// 折叠区标题字面——主册 F209：「技术错误码折叠在『详细信息』展开区」。
pub const FOLD_HINT: &str = "详细信息";

/// 登记册容量（错误面登记上限，满则逐出最旧——容量上限与淘汰纪律）。
pub const AUDIT_CAP: usize = 256;

// ---------------------------------------------------------------------------
// 乙字号四档
// ---------------------------------------------------------------------------

/// 乙字号四档档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontTier {
    /// 一档 20px：错误框大标题。
    T1,
    /// 二档 16px：小标题/图标行。
    T2,
    /// 三档 14px：正文。
    T3,
    /// 四档 12px：辅助信息。
    T4,
}

/// 档位 → 基线 px（查 [`FONT_BASELINE_PX`]）。
pub const fn tier_px(tier: FontTier) -> u16 {
    match tier {
        FontTier::T1 => FONT_BASELINE_PX[0],
        FontTier::T2 => FONT_BASELINE_PX[1],
        FontTier::T3 => FONT_BASELINE_PX[2],
        FontTier::T4 => FONT_BASELINE_PX[3],
    }
}

/// 字号对表校验：实际渲染 px 是否落在该档基线上。
pub const fn font_matches(tier: FontTier, px: u16) -> bool {
    tier_px(tier) == px
}

// ---------------------------------------------------------------------------
// 三要素与错误卡
// ---------------------------------------------------------------------------

/// 缺失的三要素之一。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    /// 【发生了什么】。
    What,
    /// 【为什么】。
    Why,
    /// 【现在能做什么】——缺此要素即「没有出路的错误框」，发布门硬禁。
    Next,
}

impl Which {
    /// 要素名（审计清单标注用）。
    pub const fn label(self) -> &'static str {
        match self {
            Which::What => "发生了什么",
            Which::Why => "为什么",
            Which::Next => "现在能做什么",
        }
    }
}

/// 错误提示三要素：【发生了什么】+【为什么】+【现在能做什么】。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreeParts {
    /// 发生了什么（一句话陈述事实，禁「操作失败」式空话）。
    pub what: String,
    /// 为什么（原因，可含格式/条件说明）。
    pub why: String,
    /// 现在能做什么（出路：可执行动作或替代路径）。
    pub next: String,
}

impl ThreeParts {
    /// 首个缺失的要素（按 what → why → next 顺序报）。
    pub fn missing(&self) -> Option<Which> {
        if self.what.is_empty() {
            Some(Which::What)
        } else if self.why.is_empty() {
            Some(Which::Why)
        } else if self.next.is_empty() {
            Some(Which::Next)
        } else {
            None
        }
    }

    /// 三要素齐备判定。
    pub fn is_complete(&self) -> bool {
        self.missing().is_none()
    }
}

/// 渲染行角色（错误框层级：图标/标题/正文/行动/折叠提示/明细）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// 图标行。
    Icon,
    /// 标题行（【发生了什么】）。
    Title,
    /// 正文行（【为什么】）。
    Body,
    /// 行动行（【现在能做什么】）。
    Action,
    /// 「详细信息」折叠提示（常驻，点击展开）。
    FoldHint,
    /// 明细行（仅展开态渲染；承载技术错误码与详细文本）。
    Detail,
}

/// 一条渲染行：角色 + 档位 + 实际 px + 文本。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutLine {
    pub role: Role,
    pub tier: FontTier,
    /// 该行实际使用的字号 px（审计对 [`FONT_BASELINE_PX`] 的取材）。
    pub px: u16,
    pub text: String,
}

/// 折叠区状态：默认收起——主册 F209「默认收起」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FoldState {
    pub open: bool,
}

impl FoldState {
    /// 默认收起态。
    pub const fn collapsed() -> FoldState {
        FoldState { open: false }
    }

    /// 切换展开/收起。
    pub const fn toggled(self) -> FoldState {
        FoldState { open: !self.open }
    }
}

/// 错误卡：一条登记在册的错误提示面。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorCard {
    /// 登记号（审计清单回溯键）。
    pub id: u32,
    pub parts: ThreeParts,
    /// 技术错误码（折叠区可见，收起态不出现在版面）。
    pub code: u32,
    /// 详细文本（折叠区可见）。
    pub detail: String,
    /// 标题行实际 px（审计对基线表）。
    pub title_px: u16,
    /// 正文/行动行实际 px。
    pub body_px: u16,
    /// 辅助行实际 px。
    pub aux_px: u16,
}

impl ErrorCard {
    /// 直接构造（审计注入面/迁移面）：不做三要素校验，允许登记残缺卡，
    /// 由 [`ErrorFaceRegistry`] 审计揪出。正常产出路径一律走
    /// [`ErrorBuilder`]（构造期强制三齐）。
    #[allow(clippy::too_many_arguments)]
    pub fn unchecked(
        id: u32,
        what: String,
        why: String,
        next: String,
        code: u32,
        detail: String,
        title_px: u16,
        body_px: u16,
        aux_px: u16,
    ) -> ErrorCard {
        ErrorCard {
            id,
            parts: ThreeParts { what, why, next },
            code,
            detail,
            title_px,
            body_px,
            aux_px,
        }
    }

    /// 首个缺失要素。
    pub fn missing(&self) -> Option<Which> {
        self.parts.missing()
    }

    /// 三要素齐备。
    pub fn is_complete(&self) -> bool {
        self.parts.is_complete()
    }

    /// 错误码在当前折叠态下是否可见（收起态永远不可见——主册「默认收起」）。
    pub fn code_visible(&self, fold: FoldState) -> bool {
        fold.open
    }

    /// 三档字号是否全部落在乙字号基线表上。
    pub fn font_ok(&self) -> bool {
        font_matches(FontTier::T1, self.title_px)
            && font_matches(FontTier::T3, self.body_px)
            && font_matches(FontTier::T4, self.aux_px)
    }

    /// 产出错误框版面：图标 → 标题 → 正文 → 行动 → 折叠提示（→ 明细，
    /// 仅展开态）。技术错误码只出现在明细行里。
    pub fn render_layout(&self, fold: FoldState) -> Vec<LayoutLine> {
        let mut out = Vec::new();
        out.push(LayoutLine {
            role: Role::Icon,
            tier: FontTier::T2,
            px: ICON_PX,
            text: String::new(),
        });
        out.push(LayoutLine {
            role: Role::Title,
            tier: FontTier::T1,
            px: self.title_px,
            text: self.parts.what.clone(),
        });
        out.push(LayoutLine {
            role: Role::Body,
            tier: FontTier::T3,
            px: self.body_px,
            text: self.parts.why.clone(),
        });
        out.push(LayoutLine {
            role: Role::Action,
            tier: FontTier::T3,
            px: self.body_px,
            text: self.parts.next.clone(),
        });
        out.push(LayoutLine {
            role: Role::FoldHint,
            tier: FontTier::T4,
            px: self.aux_px,
            text: String::from(FOLD_HINT),
        });
        if fold.open {
            let mut d = String::from("code=");
            push_dec(&mut d, self.code);
            d.push(' ');
            d.push_str(&self.detail);
            out.push(LayoutLine {
                role: Role::Detail,
                tier: FontTier::T4,
                px: self.aux_px,
                text: d,
            });
        }
        out
    }
}

/// 十进制无格式化写入（no_std 面不引 format!，渲染零堆热路径纪律）。
fn push_dec(s: &mut String, mut v: u32) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = 0usize;
    while v > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
}

// ---------------------------------------------------------------------------
// 构造器：三齐硬门
// ---------------------------------------------------------------------------

/// 构造失败：缺哪个要素。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildError {
    Missing(Which),
}

/// 错误卡构造器——`build()` 强制三要素齐备，缺一即拒。
///
/// 「禁止没有出路的错误框」由此落点：`next` 未给就是构造期错误，
/// 残缺卡根本产不出来（审计面另有 `unchecked` 注入口，供迁移数据审计）。
pub struct ErrorBuilder {
    id: u32,
    what: Option<String>,
    why: Option<String>,
    next: Option<String>,
    code: u32,
    detail: String,
    title_px: u16,
    body_px: u16,
    aux_px: u16,
}

impl ErrorBuilder {
    /// 起一个构造：字号默认取乙字号基线表。
    pub fn new(id: u32) -> ErrorBuilder {
        ErrorBuilder {
            id,
            what: None,
            why: None,
            next: None,
            code: 0,
            detail: String::new(),
            title_px: tier_px(FontTier::T1),
            body_px: tier_px(FontTier::T3),
            aux_px: tier_px(FontTier::T4),
        }
    }

    /// 【发生了什么】。
    pub fn what(mut self, s: String) -> ErrorBuilder {
        self.what = Some(s);
        self
    }

    /// 【为什么】。
    pub fn why(mut self, s: String) -> ErrorBuilder {
        self.why = Some(s);
        self
    }

    /// 【现在能做什么】——缺此要素 build 即失败。
    pub fn next(mut self, s: String) -> ErrorBuilder {
        self.next = Some(s);
        self
    }

    /// 技术错误码（折叠区）。
    pub fn code(mut self, c: u32) -> ErrorBuilder {
        self.code = c;
        self
    }

    /// 详细文本（折叠区）。
    pub fn detail(mut self, s: String) -> ErrorBuilder {
        self.detail = s;
        self
    }

    /// 覆盖标题行字号（默认基线；离基线值会被字号审计揪出）。
    pub fn title_px(mut self, px: u16) -> ErrorBuilder {
        self.title_px = px;
        self
    }

    /// 覆盖正文行字号。
    pub fn body_px(mut self, px: u16) -> ErrorBuilder {
        self.body_px = px;
        self
    }

    /// 覆盖辅助行字号。
    pub fn aux_px(mut self, px: u16) -> ErrorBuilder {
        self.aux_px = px;
        self
    }

    /// 收口：三要素任一为空/未给即拒。
    pub fn build(self) -> Result<ErrorCard, BuildError> {
        let what = self.what.unwrap_or_default();
        let why = self.why.unwrap_or_default();
        let next = self.next.unwrap_or_default();
        let parts = ThreeParts { what, why, next };
        match parts.missing() {
            Some(w) => Err(BuildError::Missing(w)),
            None => Ok(ErrorCard {
                id: self.id,
                parts,
                code: self.code,
                detail: self.detail,
                title_px: self.title_px,
                body_px: self.body_px,
                aux_px: self.aux_px,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// 审计器：清单化审计 + 缺要素清单 + 字号抽查
// ---------------------------------------------------------------------------

/// 一条审计行：每条错误面标注三要素齐/缺（主册「每条标注三要素齐/缺」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditRow {
    pub id: u32,
    pub has_what: bool,
    pub has_why: bool,
    pub has_next: bool,
}

impl AuditRow {
    /// 三要素是否全齐。
    pub const fn complete(&self) -> bool {
        self.has_what && self.has_why && self.has_next
    }
}

/// 审计汇总。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditReport {
    pub total: usize,
    pub complete: usize,
    pub missing_what: usize,
    pub missing_why: usize,
    pub missing_next: usize,
}

impl AuditReport {
    /// 发布门：缺要素项清零——主册「缺要素项清零后才可发布」。
    pub const fn publishable(&self) -> bool {
        self.missing_what == 0 && self.missing_why == 0 && self.missing_next == 0
    }
}

/// 字号抽查结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontAudit {
    pub checked: usize,
    pub mismatched: usize,
    /// 抽查条数上限（主册「抽查 20 条」）。
    pub sample_cap: usize,
}

impl FontAudit {
    /// 抽查全对基线表。
    pub const fn all_on_baseline(&self) -> bool {
        self.mismatched == 0
    }
}

/// 错误面登记册：全系统错误文案清单化审计的唯一户口。
pub struct ErrorFaceRegistry {
    cards: Vec<ErrorCard>,
    next_id: u32,
    /// 因容量满被逐出的最旧登记条数（淘汰纪律留痕）。
    pub evicted: u32,
}

impl ErrorFaceRegistry {
    pub fn new() -> ErrorFaceRegistry {
        ErrorFaceRegistry { cards: Vec::new(), next_id: 1, evicted: 0 }
    }

    pub fn len(&self) -> usize {
        self.cards.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    /// 登记一条错误面（分配登记号；容量满逐出最旧）。
    pub fn register(&mut self, mut card: ErrorCard) -> u32 {
        card.id = self.next_id;
        self.next_id += 1;
        if self.cards.len() >= AUDIT_CAP {
            self.cards.remove(0);
            self.evicted += 1;
        }
        let id = card.id;
        self.cards.push(card);
        id
    }

    /// 逐条审计行（清单化：每条标注三要素齐/缺）。
    pub fn audit_rows(&self) -> Vec<AuditRow> {
        let mut out = Vec::new();
        for c in &self.cards {
            out.push(AuditRow {
                id: c.id,
                has_what: !c.parts.what.is_empty(),
                has_why: !c.parts.why.is_empty(),
                has_next: !c.parts.next.is_empty(),
            });
        }
        out
    }

    /// 审计汇总。
    pub fn audit(&self) -> AuditReport {
        let mut r = AuditReport {
            total: self.cards.len(),
            complete: 0,
            missing_what: 0,
            missing_why: 0,
            missing_next: 0,
        };
        for c in &self.cards {
            match c.missing() {
                None => r.complete += 1,
                Some(Which::What) => r.missing_what += 1,
                Some(Which::Why) => r.missing_why += 1,
                Some(Which::Next) => r.missing_next += 1,
            }
        }
        r
    }

    /// 缺要素登记号清单——非空即未达发布门。
    pub fn missing_ids(&self) -> Vec<u32> {
        let mut out = Vec::new();
        for c in &self.cards {
            if c.missing().is_some() {
                out.push(c.id);
            }
        }
        out
    }

    /// 字号抽查：对登记卡逐条对乙字号基线表（上限 [`FONT_SAMPLE_N`] 条，
    /// 取最新登记的 20 条——抽查面向最近产出）。
    pub fn font_sample_audit(&self) -> FontAudit {
        let start = self.cards.len().saturating_sub(FONT_SAMPLE_N);
        let mut fa = FontAudit { checked: 0, mismatched: 0, sample_cap: FONT_SAMPLE_N };
        for c in &self.cards[start..] {
            fa.checked += 1;
            if !c.font_ok() {
                fa.mismatched += 1;
            }
        }
        fa
    }
}

impl Default for ErrorFaceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// 合格样张拆解：主册「无法打开文件（格式不支持，可安装 .7z 解码器或在
/// 应用内另存为 .zip）」→ 三要素各归其位。
fn sample_good() -> ThreeParts {
    ThreeParts {
        what: String::from("无法打开文件"),
        why: String::from("格式不支持"),
        next: String::from("可安装 .7z 解码器或在应用内另存为 .zip"),
    }
}

/// F209 自检（12 条行为级 + 1 条 fuzz）。
pub fn run_errthree_checks() -> CheckSet {
    let mut set = CheckSet::new("F209-errthree");

    // 1. 合格样张：三要素拆解齐备，build 通过。
    let g = sample_good();
    let card = ErrorBuilder::new(1)
        .what(g.what.clone())
        .why(g.why.clone())
        .next(g.next.clone())
        .code(0x8007_0037)
        .detail(String::from("0x80070037: 格式识别失败"))
        .build();
    set.add("good sample builds complete", card.is_ok() && card.as_ref().map(|c| c.is_complete()).unwrap_or(false), "");

    // 2. 不合格样张：「操作失败」单段空话 → 缺 why/next，构造期拒绝。
    let bad = ErrorBuilder::new(2).what(String::from("操作失败")).build();
    set.add(
        "\"操作失败\" rejected at build time",
        bad == Err(BuildError::Missing(Which::Why)),
        "",
    );

    // 3. 禁止没有出路的错误框：缺 next 即拒，即便 what/why 齐。
    let noexit = ErrorBuilder::new(3)
        .what(String::from("无法打开文件"))
        .why(String::from("格式不支持"))
        .build();
    set.add("missing next rejected (no dead-end)", noexit == Err(BuildError::Missing(Which::Next)), "");

    // 4. 折叠区默认收起：错误码不可见；展开后明细行出现且含错误码。
    let card = card.unwrap();
    let folded = card.render_layout(FoldState::collapsed());
    let open = card.render_layout(FoldState::collapsed().toggled());
    let detail_open = open.iter().any(|l| l.role == Role::Detail && l.text.contains("0x80070037"));
    set.add(
        "code hidden folded / visible expanded",
        !card.code_visible(FoldState::collapsed())
            && card.code_visible(FoldState { open: true })
            && !folded.iter().any(|l| l.role == Role::Detail)
            && detail_open,
        "",
    );

    // 5. 版面角色顺序：图标→标题→正文→行动→折叠提示（层级按乙字号四档）。
    let roles = [
        Role::Icon,
        Role::Title,
        Role::Body,
        Role::Action,
        Role::FoldHint,
    ];
    let order_ok = folded.len() == 5
        && folded.iter().zip(roles.iter()).all(|(l, r)| l.role == *r);
    set.add("layout role order icon/title/body/action/hint", order_ok, "");

    // 6. 乙字号四档基线表数值：20/16/14/12。
    set.add(
        "baseline table 20/16/14/12",
        FONT_BASELINE_PX == [20, 16, 14, 12] && tier_px(FontTier::T1) == 20,
        "",
    );

    // 7. 字号审计：默认基线构造的卡全对表。
    let fa = {
        let mut reg = ErrorFaceRegistry::new();
        reg.register(card.clone());
        reg.font_sample_audit()
    };
    set.add("font audit passes on-baseline card", fa.all_on_baseline() && fa.checked == 1, "");

    // 8. 字号审计抓离表值：标题 18px ≠ 基线 20px → mismatch。
    let off = ErrorCard::unchecked(
        0,
        String::from("发生了什么"),
        String::from("为什么"),
        String::from("现在能做什么"),
        1,
        String::new(),
        18,
        TIER3_PX,
        TIER4_PX,
    );
    let fa_bad = {
        let mut reg = ErrorFaceRegistry::new();
        reg.register(off);
        reg.font_sample_audit()
    };
    set.add(
        "font audit catches off-baseline px",
        fa_bad.mismatched == 1 && !fa_bad.all_on_baseline(),
        "",
    );

    // 9. 清单化审计：残缺卡逐条标注齐/缺。
    let mut reg = ErrorFaceRegistry::new();
    reg.register(ErrorCard::unchecked(
        0,
        String::new(),
        String::from("为什么"),
        String::from("现在能做什么"),
        0,
        String::new(),
        TIER1_PX,
        TIER3_PX,
        TIER4_PX,
    ));
    reg.register(ErrorCard::unchecked(
        0,
        String::from("发生了什么"),
        String::from("为什么"),
        String::new(),
        0,
        String::new(),
        TIER1_PX,
        TIER3_PX,
        TIER4_PX,
    ));
    reg.register(ErrorBuilder::new(0).what(g.what.clone()).why(g.why.clone()).next(g.next.clone()).build().unwrap());
    let rows = reg.audit_rows();
    let rep = reg.audit();
    set.add(
        "audit rows label per-part presence",
        rows.len() == 3
            && !rows[0].has_what
            && rows[0].has_why
            && rows[0].has_next
            && rows[1].has_what
            && !rows[1].has_next
            && rows[2].complete(),
        "",
    );

    // 10. 发布门：缺要素清单非空即不可发布；补齐后清零。
    let missing = reg.missing_ids();
    set.add(
        "publish gate blocks until missing cleared",
        !rep.publishable() && missing.len() == 2 && rep.missing_what == 1 && rep.missing_next == 1,
        "",
    );

    // 11. 容量纪律：登记超 AUDIT_CAP 逐出最旧，evicted 计数留痕。
    let mut full = ErrorFaceRegistry::new();
    for i in 0..(AUDIT_CAP + 3) as u32 {
        full.register(ErrorCard::unchecked(
            i,
            String::from("w"),
            String::from("w"),
            String::from("w"),
            i,
            String::new(),
            TIER1_PX,
            TIER3_PX,
            TIER4_PX,
        ));
    }
    set.add(
        "registry evicts oldest beyond cap",
        full.len() == AUDIT_CAP && full.evicted == 3 && full.audit().complete == AUDIT_CAP,
        "",
    );

    // 12. 抽查上限：登记 30 条（25 对表 + 5 离表），抽查只看最新 20 条 →
    //     mismatched = 5，checked = 20（主册「抽查 20 条」口径）。
    let mut s20 = ErrorFaceRegistry::new();
    for i in 0..30u32 {
        let off = i >= 25;
        s20.register(ErrorCard::unchecked(
            i,
            String::from("w"),
            String::from("w"),
            String::from("w"),
            i,
            String::new(),
            if off { 22 } else { TIER1_PX },
            TIER3_PX,
            TIER4_PX,
        ));
    }
    let fa20 = s20.font_sample_audit();
    set.add(
        "font sample audit caps at 20 newest",
        fa20.checked == FONT_SAMPLE_N && fa20.mismatched == 5,
        "",
    );

    // 13. fuzz（xors32 范式，2000 轮）：随机三要素空缺 + 随机离表字号——
    //     构造器拒绝与 brute force 一致、登记册审计计数与期望一致。
    //     缺陷账本：现象=fuzz 恒红（rep.total==256≠2000）；根因=fuzz 期望
    //     按全量 2000 轮累计，但登记册容量在册（AUDIT_CAP=256、满则逐出
    //     最旧——检查 11 正是验这条纪律），期望统计越出了登记域，属 fuzz
    //     期望构造越界而非实现错；修法=逐轮判据入档，期望只在「容量窗口」
    //     （仍留册的最新 AUDIT_CAP 轮）内统计，字号抽查同理只看最新
    //     FONT_SAMPLE_N 条（与 font_sample_audit 口径一致）。
    let mut x: u32 = 0x9E37_79B9;
    let mut builder_ok = true;
    let mut reg = ErrorFaceRegistry::new();
    // 每轮判据档案：(缺要素码 0..=3, 字号离表标记)。
    let mut verdicts: Vec<(u8, u8)> = Vec::new();
    for i in 0..2000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let e_what = x & 1;
        let e_why = (x >> 1) & 1;
        let e_next = (x >> 2) & 1;
        let px_bad = (x >> 3) & 1;
        let what = if e_what == 0 { String::new() } else { String::from("发生了什么") };
        let why = if e_why == 0 { String::new() } else { String::from("为什么") };
        let next = if e_next == 0 { String::new() } else { String::from("现在能做什么") };
        let title_px = if px_bad == 0 { TIER1_PX } else { TIER1_PX + 4 };

        // 构造器与三要素非空校验必须同判（builder_ok 护栏）。
        let b = ErrorBuilder::new(i)
            .what(what.clone())
            .why(why.clone())
            .next(next.clone())
            .title_px(title_px)
            .build();
        let parts = ThreeParts { what: what.clone(), why: why.clone(), next: next.clone() };
        let expect_err = parts.missing();
        if (b.is_err()) != expect_err.is_some()
            || (b.as_ref().err().copied() != expect_err.map(BuildError::Missing))
        {
            builder_ok = false;
        }

        // 逐轮判据入档（期望统计延后到容量窗口上做）。
        let miss_code = match parts.missing() {
            None => 0u8,
            Some(Which::What) => 1,
            Some(Which::Why) => 2,
            Some(Which::Next) => 3,
        };
        verdicts.push((miss_code, px_bad as u8));

        reg.register(ErrorCard::unchecked(
            i, what, why, next, i, String::new(), title_px, TIER3_PX, TIER4_PX,
        ));
    }
    // 期望只在留册窗口内统计（2000 轮 > AUDIT_CAP → 留册 = 最新 256 轮）。
    let retain = 2000usize.min(AUDIT_CAP);
    let win = &verdicts[verdicts.len() - retain..];
    let mut exp_complete = 0usize;
    let mut exp_mwhat = 0usize;
    let mut exp_mwhy = 0usize;
    let mut exp_mnext = 0usize;
    for &(mc, _) in win {
        match mc {
            0 => exp_complete += 1,
            1 => exp_mwhat += 1,
            2 => exp_mwhy += 1,
            _ => exp_mnext += 1,
        }
    }
    // 字号抽查只看最新 FONT_SAMPLE_N 条（主册「抽查 20 条」口径）。
    let sample_win = &win[win.len() - FONT_SAMPLE_N.min(retain)..];
    let exp_font_bad = sample_win.iter().filter(|(_, pb)| *pb != 0).count();
    let rep = reg.audit();
    let fa = reg.font_sample_audit();
    set.add(
        "fuzz 2000: builder gate & audit tallies consistent",
        builder_ok
            && rep.total == retain
            && rep.complete == exp_complete
            && rep.missing_what == exp_mwhat
            && rep.missing_why == exp_mwhy
            && rep.missing_next == exp_mnext
            && fa.checked == FONT_SAMPLE_N
            && fa.mismatched == exp_font_bad
            && reg.missing_ids().len() == exp_mwhat + exp_mwhy + exp_mnext,
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_sample_end_to_end() {
        let g = sample_good();
        let card = ErrorBuilder::new(7)
            .what(g.what)
            .why(g.why)
            .next(g.next)
            .code(42)
            .detail(String::from("底层细节"))
            .build()
            .unwrap();
        assert!(card.is_complete());
        assert!(card.font_ok());
        // 收起态 5 行，展开态 6 行且明细含错误码。
        assert_eq!(card.render_layout(FoldState::collapsed()).len(), 5);
        let open = card.render_layout(FoldState::collapsed().toggled());
        assert_eq!(open.len(), 6);
        assert!(open[5].text.contains("code=42"));
        assert!(open[5].text.contains("底层细节"));
    }

    #[test]
    fn builder_rejects_each_missing_part() {
        let w = || String::from("发生了什么");
        let y = || String::from("为什么");
        let n = || String::from("现在能做什么");
        assert_eq!(ErrorBuilder::new(1).build(), Err(BuildError::Missing(Which::What)));
        assert_eq!(ErrorBuilder::new(1).what(w()).build(), Err(BuildError::Missing(Which::Why)));
        assert_eq!(
            ErrorBuilder::new(1).what(w()).why(y()).build(),
            Err(BuildError::Missing(Which::Next))
        );
        assert!(ErrorBuilder::new(1).what(w()).why(y()).next(n()).build().is_ok());
    }

    #[test]
    fn fold_toggle_semantics() {
        let f = FoldState::collapsed();
        assert!(!f.open);
        assert!(f.toggled().open);
        // 二次切换回到收起。
        assert!(!f.toggled().toggled().open);
    }

    #[test]
    fn registry_audit_and_missing_ids() {
        let mut reg = ErrorFaceRegistry::new();
        reg.register(ErrorCard::unchecked(
            0,
            String::new(),
            String::new(),
            String::new(),
            0,
            String::new(),
            TIER1_PX,
            TIER3_PX,
            TIER4_PX,
        ));
        let rep = reg.audit();
        assert_eq!(rep.total, 1);
        assert_eq!(rep.missing_what, 1);
        assert!(!rep.publishable());
        assert_eq!(reg.missing_ids().len(), 1);
    }

    #[test]
    fn font_baseline_table_values() {
        assert_eq!(tier_px(FontTier::T1), 20);
        assert_eq!(tier_px(FontTier::T2), 16);
        assert_eq!(tier_px(FontTier::T3), 14);
        assert_eq!(tier_px(FontTier::T4), 12);
        assert!(font_matches(FontTier::T3, 14));
        assert!(!font_matches(FontTier::T3, 15));
    }

    #[test]
    fn fuzz_registry_never_panics_and_counts_hold() {
        let mut x: u32 = 0xDEAD_BEEF;
        let mut reg = ErrorFaceRegistry::new();
        for i in 0..3000u32 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let n = (x % 4) as usize; // 随机塞 0~3 个要素
            let what = if n >= 1 { String::from("w") } else { String::new() };
            let why = if n >= 2 { String::from("y") } else { String::new() };
            let next = if n >= 3 { String::from("n") } else { String::new() };
            let px = if x & 1 == 0 { TIER1_PX } else { 30 };
            reg.register(ErrorCard::unchecked(
                i, what, why, next, x, String::new(), px, TIER3_PX, TIER4_PX,
            ));
            assert!(reg.len() <= AUDIT_CAP);
        }
        let rep = reg.audit();
        assert_eq!(rep.total, AUDIT_CAP);
        assert_eq!(
            rep.complete + rep.missing_what + rep.missing_why + rep.missing_next,
            AUDIT_CAP
        );
    }

    #[test]
    fn selfcheck_all_green() {
        let set = run_errthree_checks();
        assert!(set.all_passed(), "F209 自检存在红项");
        assert!(!set.truncated());
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================

const VXH1_MAGIC: [u8; 4] = *b"VXH1";
const VXH1_VER: u8 = 1;

/// 损坏输入显性拒绝：magic/版本/校验/长度四类全拒（本记录全字段均在
/// 0..=255 合法域内，无需额外字段越界类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2CodecErr {
    BadMagic,
    BadVersion,
    BadLen,
    BadSum,
}

/// FNV-1a 32 位（校验和唯一实现点）。
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ---- 持久化 I/O 面：错误登记记录（三要素位图 + 错误码）----

/// 记录长：magic4 + ver1 + id4 + flags1 + code4 + sum4。
pub const ERREG_REC_LEN: usize = 4 + 1 + 4 + 1 + 4 + 4;

/// 三要素位图位定义（bit0=what、bit1=why、bit2=next，置位 = 非空）。
pub const FLAG_WHAT: u8 = 1;
pub const FLAG_WHY: u8 = 2;
pub const FLAG_NEXT: u8 = 4;

/// 错误登记的字节级记录：三要素齐/缺以位图承载、技术错误码随档——
/// 审计清单可整册迁移（主册 F209「错误文案清单化审计」的档案面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ErrRegRec {
    pub id: u32,
    /// 三要素位图（FLAG_WHAT/FLAG_WHY/FLAG_NEXT 组合）。
    pub flags: u8,
    pub code: u32,
}

impl ErrRegRec {
    /// 从错误卡取档（位图与 ThreeParts 非空校验同判——一处一事实）。
    pub fn of(card: &ErrorCard) -> ErrRegRec {
        let mut flags = 0u8;
        if !card.parts.what.is_empty() {
            flags |= FLAG_WHAT;
        }
        if !card.parts.why.is_empty() {
            flags |= FLAG_WHY;
        }
        if !card.parts.next.is_empty() {
            flags |= FLAG_NEXT;
        }
        ErrRegRec { id: card.id, flags, code: card.code }
    }

    pub fn to_bytes(&self) -> [u8; ERREG_REC_LEN] {
        let mut out = [0u8; ERREG_REC_LEN];
        out[..4].copy_from_slice(&VXH1_MAGIC);
        out[4] = VXH1_VER;
        out[5..9].copy_from_slice(&self.id.to_le_bytes());
        out[9] = self.flags;
        out[10..14].copy_from_slice(&self.code.to_le_bytes());
        let sum = fnv1a(&out[..14]).to_le_bytes();
        out[14..18].copy_from_slice(&sum);
        out
    }

    pub fn from_bytes(b: &[u8]) -> Result<ErrRegRec, V2CodecErr> {
        if b.len() != ERREG_REC_LEN {
            return Err(V2CodecErr::BadLen);
        }
        let mut mg = [0u8; 4];
        mg.copy_from_slice(&b[..4]);
        if mg != VXH1_MAGIC {
            return Err(V2CodecErr::BadMagic);
        }
        if b[4] != VXH1_VER {
            return Err(V2CodecErr::BadVersion);
        }
        let mut sum = [0u8; 4];
        sum.copy_from_slice(&b[14..18]);
        if fnv1a(&b[..14]) != u32::from_le_bytes(sum) {
            return Err(V2CodecErr::BadSum);
        }
        let mut id = [0u8; 4];
        id.copy_from_slice(&b[5..9]);
        let mut code = [0u8; 4];
        code.copy_from_slice(&b[10..14]);
        Ok(ErrRegRec { id: u32::from_le_bytes(id), flags: b[9], code: u32::from_le_bytes(code) })
    }
}

// ---- UI 壳接线面：折叠展开判定面（默认收起几何）----

/// 折叠提示行命中测试：点在「详细信息」行矩形内即请求切换展开/收起
/// （主册 F209「技术错误码折叠在『详细信息』展开区（默认收起）」的
/// 壳层入口——命中面与 FoldState::toggled 配对使用）。
pub fn fold_hit(hint_row: crate::h1star::h1base::Rect, x: i32, y: i32) -> bool {
    hint_row.contains(x, y)
}

/// 版面高度：收起态不含明细行，展开态多恰一行（默认收起几何判据——
/// 错误框初始高度恒不含错误码行，展开增量 = 一行明细）。
pub fn panel_height(rows: usize, row_h: i32, fold: FoldState) -> i32 {
    let extra = if fold.open { 1i32 } else { 0 };
    (rows as i32 + extra) * row_h
}

/// F209 v2 自检（首条恒为持久化 round-trip）。
pub fn run_errthree_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F209-errthree-v2");

    // 1. 持久化 round-trip：齐三要素 + 错误码的登记记录编码→解码还原。
    let card = match ErrorBuilder::new(1)
        .what(String::from("无法打开文件"))
        .why(String::from("格式不支持"))
        .next(String::from("可安装 .7z 解码器"))
        .code(0x8007_0037)
        .build()
    {
        Ok(c) => c,
        Err(_) => ErrorCard::unchecked(
            0,
            String::new(),
            String::new(),
            String::new(),
            0,
            String::new(),
            TIER1_PX,
            TIER3_PX,
            TIER4_PX,
        ),
    };
    let rec = ErrRegRec::of(&card);
    let bytes = rec.to_bytes();
    set.add(
        "v2 persist roundtrip err reg rec",
        ErrRegRec::from_bytes(&bytes) == Ok(rec)
            && rec.flags == (FLAG_WHAT | FLAG_WHY | FLAG_NEXT),
        "",
    );

    // 2. 损坏拒绝四类：magic/版本/长度/校验（坏档案不静默解析）。
    let mut bad1 = bytes;
    bad1[0] = b'X';
    let mut bad2 = bytes;
    bad2[4] = 9;
    let mut bad3 = bytes;
    bad3[10] ^= 0xFF;
    set.add(
        "v2 persist rejects corrupt err recs",
        ErrRegRec::from_bytes(&bad1) == Err(V2CodecErr::BadMagic)
            && ErrRegRec::from_bytes(&bad2) == Err(V2CodecErr::BadVersion)
            && ErrRegRec::from_bytes(&bad3) == Err(V2CodecErr::BadSum)
            && ErrRegRec::from_bytes(&bytes[..bytes.len() - 1]) == Err(V2CodecErr::BadLen),
        "",
    );

    // 3. 位图同判：残缺卡位图与 AuditRow 逐位一致（缺 what → 仅 why|next）。
    let partial = ErrorCard::unchecked(
        2,
        String::new(),
        String::from("为什么"),
        String::from("现在能做什么"),
        7,
        String::new(),
        TIER1_PX,
        TIER3_PX,
        TIER4_PX,
    );
    let rec2 = ErrRegRec::of(&partial);
    set.add(
        "v2 flags bitmap matches audit row",
        rec2.flags == FLAG_WHY | FLAG_NEXT
            && !(rec2.flags & FLAG_WHAT != 0)
            && ErrRegRec::of(&card).flags == FLAG_WHAT | FLAG_WHY | FLAG_NEXT,
        "",
    );

    // 4. 折叠命中面：点在「详细信息」行内命中、行外不命中（壳层入口）。
    let hint = crate::h1star::h1base::Rect::new(20, 90, 120, 18);
    set.add(
        "v2 fold hint hit test",
        fold_hit(hint, 30, 95) && !fold_hit(hint, 200, 95),
        "",
    );

    // 5. 默认收起几何：收起态高度不含明细行，展开恰多一行
    //    （验主册 F209「默认收起」的版面几何口径）。
    set.add(
        "v2 panel height collapsed vs expanded",
        panel_height(5, 18, FoldState::collapsed()) == 90
            && panel_height(5, 18, FoldState::collapsed().toggled()) == 108,
        "",
    );

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn err_rec_zero_code_roundtrip() {
        let rec = ErrRegRec { id: u32::MAX, flags: 0, code: 0 };
        assert_eq!(ErrRegRec::from_bytes(&rec.to_bytes()).unwrap(), rec);
    }

    #[test]
    fn fold_hit_edge_inside() {
        let r = crate::h1star::h1base::Rect::new(0, 0, 10, 10);
        assert!(fold_hit(r, 0, 0));
        assert!(!fold_hit(r, 10, 10), "开区间边界不含");
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_errthree_v2_checks();
        assert!(set.all_passed(), "F209 v2 自检存在红项");
        assert!(!set.truncated());
        assert!((4..=6).contains(&set.len()));
    }
}
