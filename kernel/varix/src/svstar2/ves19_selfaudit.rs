//! VE-F3619 · R01 预备自查（VE-S 域 · 批次 S01 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3619`
//!
//! **判据（锚点原文六条）**：九维 R 版、四条件、保真维、八用复刻、
//! 移交核收、判据。逐条落位：
//! - **九维 R 版**：架构/模型/工作流/对接/预览/验证/预算/无障碍/安全
//!   九维 × 三色（绿/黄/红），[`NineDimMatrix`]。红未清 → 阻断。
//! - **四条件**：GO 预判四条件 R 版（九维红零 + 保真抽验绿 + 许可审计
//!   零 + 收敛断言绿）——[`GoJudge`] 四条件缺一 → NO-GO。
//! - **保真维**：保真抽验独立成维（红线联动，未验 → 补验）。
//! - **八用复刻**：F2978 模式八用（复述制度 R 版）。
//! - **移交核收**：四件套移交清单（[`Handover`]）。
//! - **判据**：`run_ves19_checks` 逐条映射。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 红未清→阻断（复述） | [`V13Code::RED_OPEN`]：存在红维即阻断 |
//! | 判定缺输入→NO-GO（复述） | [`V13Code::JUDGE_INPUT_MISS`] |
//! | 保真未验→补验（红线联动） | [`V13Code::FIDELITY_UNVERIFIED`] |
//! | 黄超期→转红（复述） | [`V13Code::YELLOW_EXPIRED`] |
//!
//! **码段**：独占 **0x43xx**。
//!
//! **性能逐项分解**：矩阵 O(九维)；判定 O(1)；登记 O(风险)；
//! 移交 O(件)。
//!
//! **跨批对接点**：F2978 模式八用；五板块数据源；F3620 收口输入——
//! 钉死于 [`DOCKS`]。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::string::String;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3619 诊断码段（独占 0x43xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V13Code(pub u16);

impl V13Code {
    /// 存在红维未清。
    pub const RED_OPEN: V13Code = V13Code(0x4301);
    /// 判定输入缺失。
    pub const JUDGE_INPUT_MISS: V13Code = V13Code(0x4302);
    /// 保真未验证。
    pub const FIDELITY_UNVERIFIED: V13Code = V13Code(0x4303);
    /// 黄超期转红。
    pub const YELLOW_EXPIRED: V13Code = V13Code(0x4304);
    /// 移交件缺。
    pub const HANDOVER_MISS: V13Code = V13Code(0x4305);
    /// 九维缺一。
    pub const DIM_GAP: V13Code = V13Code(0x4306);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V13Code::RED_OPEN => {
                "九维矩阵存在红维未清（阻断）。建议：逐条清理红项后重新自查。"
            }
            V13Code::JUDGE_INPUT_MISS => {
                "GO 判定输入缺失（保守 NO-GO）。建议：补齐四条件输入后重判。"
            }
            V13Code::FIDELITY_UNVERIFIED => {
                "保真抽验未执行（红线联动）。建议：先完成保真抽验再提交 GO。"
            }
            V13Code::YELLOW_EXPIRED => {
                "黄项超期自动转红。建议：限期清理黄项避免升级。"
            }
            V13Code::HANDOVER_MISS => {
                "移交四件套缺件。建议：补齐移交清单。"
            }
            V13Code::DIM_GAP => {
                "九维缺一。建议：补全九维数据源。"
            }
            _ => "未知诊断码。",
        }
    }
}

// ── 九维矩阵 ────────────────────────────────────────────────────────

/// 自查维度（九维 R 版）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dim {
    /// 架构。
    Arch,
    /// 模型。
    Model,
    /// 工作流。
    Workflow,
    /// 对接。
    Dock,
    /// 预览。
    Preview,
    /// 验证。
    Verify,
    /// 预算。
    Budget,
    /// 无障碍。
    A11y,
    /// 安全。
    Security,
}

impl Dim {
    /// 九维全集。
    pub fn all() -> [Dim; 9] {
        [
            Dim::Arch,
            Dim::Model,
            Dim::Workflow,
            Dim::Dock,
            Dim::Preview,
            Dim::Verify,
            Dim::Budget,
            Dim::A11y,
            Dim::Security,
        ]
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Dim::Arch => "架构",
            Dim::Model => "模型",
            Dim::Workflow => "工作流",
            Dim::Dock => "对接",
            Dim::Preview => "预览",
            Dim::Verify => "验证",
            Dim::Budget => "预算",
            Dim::A11y => "无障碍",
            Dim::Security => "安全",
        }
    }
}

/// 三色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Color {
    /// 绿（达标）。
    Green,
    /// 黄（有风险，限期）。
    Yellow,
    /// 红（阻断）。
    Red,
}

/// 九维矩阵。
#[derive(Clone, Debug, Default)]
pub struct NineDimMatrix {
    rows: Vec<(Dim, Color, String)>,
    /// 黄项期限（轮次）。
    yellow_ttl: Vec<(Dim, u32)>,
}

impl NineDimMatrix {
    /// 空矩阵。
    pub fn new() -> NineDimMatrix {
        NineDimMatrix {
            rows: Vec::new(),
            yellow_ttl: Vec::new(),
        }
    }

    /// 定一维。
    pub fn set(&mut self, dim: Dim, color: Color, note: &str) {
        if let Some(slot) = self.rows.iter_mut().find(|(d, _, _)| *d == dim) {
            slot.1 = color;
            slot.2 = String::from(note);
        } else {
            self.rows.push((dim, color, String::from(note)));
        }
    }

    /// 给黄项设 TTL。
    pub fn set_yellow_ttl(&mut self, dim: Dim, rounds: u32) {
        if let Some(slot) = self.yellow_ttl.iter_mut().find(|(d, _)| *d == dim) {
            slot.1 = rounds;
        } else {
            self.yellow_ttl.push((dim, rounds));
        }
    }

    /// 黄超期转红。
    pub fn expire_yellows(&mut self) {
        for (dim, ttl) in self.yellow_ttl.clone() {
            if ttl == 0 {
                if let Some(slot) = self.rows.iter_mut().find(|(d, _, _)| *d == dim) {
                    if slot.1 == Color::Yellow {
                        slot.1 = Color::Red;
                    }
                }
            }
        }
    }

    /// 九维齐全审计。
    pub fn audit_dims(&self) -> Result<(), V13Code> {
        for d in Dim::all() {
            if !self.rows.iter().any(|(dd, _, _)| *dd == d) {
                return Err(V13Code::DIM_GAP);
            }
        }
        Ok(())
    }

    /// 红维清单。
    pub fn red_list(&self) -> Vec<Dim> {
        self.rows
            .iter()
            .filter(|(_, c, _)| *c == Color::Red)
            .map(|(d, _, _)| *d)
            .collect()
    }

    /// 红零审计。
    pub fn audit_red_zero(&self) -> Result<(), V13Code> {
        if self.red_list().is_empty() {
            Ok(())
        } else {
            Err(V13Code::RED_OPEN)
        }
    }
}

// ── GO 判定（四条件）────────────────────────────────────────────────

/// GO 判定输入。
#[derive(Clone, Debug)]
pub struct GoInput {
    /// 九维红零。
    pub red_zero: bool,
    /// 保真抽验绿。
    pub fidelity_green: bool,
    /// 许可审计零侵权。
    pub license_clean: bool,
    /// 收敛断言绿。
    pub convergence_green: bool,
}

/// GO 判定器。
#[derive(Clone, Debug, Default)]
pub struct GoJudge;

impl GoJudge {
    /// 判定。四条件缺一即 NO-GO；任一输入缺失（用 Option 语义时保守拒绝）。
    pub fn judge(input: &GoInput) -> Result<(), V13Code> {
        if input.red_zero
            && input.fidelity_green
            && input.license_clean
            && input.convergence_green
        {
            Ok(())
        } else if !input.fidelity_green {
            Err(V13Code::FIDELITY_UNVERIFIED)
        } else {
            Err(V13Code::JUDGE_INPUT_MISS)
        }
    }
}

// ── 风险册 ──────────────────────────────────────────────────────────

/// 一条风险。
#[derive(Clone, Debug)]
pub struct Risk {
    /// 风险 ID。
    pub id: u32,
    /// 描述。
    pub desc: String,
    /// 对应维度。
    pub dim: Dim,
    /// 是否已闭环。
    pub closed: bool,
}

/// 风险册。
#[derive(Clone, Debug, Default)]
pub struct RiskBook {
    items: Vec<Risk>,
}

impl RiskBook {
    /// 新建。
    pub fn new() -> RiskBook {
        RiskBook { items: Vec::new() }
    }

    /// 登记。
    pub fn register(&mut self, id: u32, dim: Dim, desc: &str) {
        self.items.push(Risk {
            id,
            desc: String::from(desc),
            dim,
            closed: false,
        });
    }

    /// 闭环。
    pub fn close(&mut self, id: u32) {
        if let Some(r) = self.items.iter_mut().find(|r| r.id == id) {
            r.closed = true;
        }
    }

    /// 未闭环数。
    pub fn open_count(&self) -> usize {
        self.items.iter().filter(|r| !r.closed).count()
    }

    /// 总数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

// ── 移交四件套 ──────────────────────────────────────────────────────

/// 移交件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandoverItem {
    /// 接口总账。
    ApiLedger,
    /// 判据映射表。
    CheckMap,
    /// 风险册。
    RiskList,
    /// 联动证据。
    LinkEvidence,
}

impl HandoverItem {
    /// 四件套。
    pub fn all() -> [HandoverItem; 4] {
        [
            HandoverItem::ApiLedger,
            HandoverItem::CheckMap,
            HandoverItem::RiskList,
            HandoverItem::LinkEvidence,
        ]
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            HandoverItem::ApiLedger => "接口总账",
            HandoverItem::CheckMap => "判据映射表",
            HandoverItem::RiskList => "风险册",
            HandoverItem::LinkEvidence => "联动证据",
        }
    }
}

/// 移交清单。
#[derive(Clone, Debug, Default)]
pub struct Handover {
    present: Vec<HandoverItem>,
}

impl Handover {
    /// 新建。
    pub fn new() -> Handover {
        Handover {
            present: Vec::new(),
        }
    }

    /// 交付一件。
    pub fn deliver(&mut self, item: HandoverItem) {
        if !self.present.contains(&item) {
            self.present.push(item);
        }
    }

    /// 核收审计。
    pub fn audit(&self) -> Result<(), V13Code> {
        for i in HandoverItem::all() {
            if !self.present.contains(&i) {
                return Err(V13Code::HANDOVER_MISS);
            }
        }
        Ok(())
    }

    /// 缺件清单。
    pub fn missing(&self) -> Vec<HandoverItem> {
        HandoverItem::all()
            .iter()
            .filter(|i| !self.present.contains(i))
            .cloned()
            .collect()
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// F2978 模式八用索引。
pub const FAMILY_MODE_REUSE: u16 = 8;

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["VE-F2978", "FIVE-BOARDS", "VE-F3620"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves19_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3619");

    // 0x4301 九维全绿
    let mut m = NineDimMatrix::new();
    for d in Dim::all() {
        m.set(d, Color::Green, "达标");
    }
    out.add("0x4301-九维全绿", m.audit_dims().is_ok() && m.audit_red_zero().is_ok(), "");

    // 0x4302 红维阻断
    let mut m2 = NineDimMatrix::new();
    for d in Dim::all() {
        m2.set(d, Color::Green, "ok");
    }
    m2.set(Dim::Budget, Color::Red, "超预算");
    out.add("0x4302-红维阻断", m2.audit_red_zero().is_err(), "");

    // 0x4303 维缺检出
    let m3 = NineDimMatrix::new();
    out.add("0x4303-维缺检出", m3.audit_dims().is_err(), "");

    // 0x4304 黄超期转红
    let mut m4 = NineDimMatrix::new();
    for d in Dim::all() {
        m4.set(d, Color::Green, "ok");
    }
    m4.set(Dim::Preview, Color::Yellow, "待优化");
    m4.set_yellow_ttl(Dim::Preview, 0);
    m4.expire_yellows();
    out.add("0x4304-黄超期转红", m4.red_list().contains(&Dim::Preview), "");

    // 0x4305 GO 四条件过
    let gi = GoInput {
        red_zero: true,
        fidelity_green: true,
        license_clean: true,
        convergence_green: true,
    };
    out.add("0x4305-四条件 GO", GoJudge::judge(&gi).is_ok(), "");

    // 0x4306 保真未验拒
    let gi2 = GoInput {
        fidelity_green: false,
        ..gi.clone()
    };
    out.add("0x4306-保真未验拒", GoJudge::judge(&gi2).is_err(), "");

    // 0x4307 移交四件套齐
    let mut h = Handover::new();
    for i in HandoverItem::all() {
        h.deliver(i);
    }
    out.add("0x4307-移交齐", h.audit().is_ok(), "");

    // 0x4308 移交缺件
    let h2 = Handover::new();
    out.add("0x4308-移交缺件检出", h2.audit().is_err() && h2.missing().len() == 4, "");

    // 0x4309 风险闭环
    let mut rb = RiskBook::new();
    rb.register(1, Dim::Arch, "接口面过大");
    out.add("0x4309-风险登记", rb.open_count() == 1, "");

    out
}
