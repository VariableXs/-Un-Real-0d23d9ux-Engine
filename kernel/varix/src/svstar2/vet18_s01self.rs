//! VE-F3819 · S01 预备自查（VE-T 域 · 无障碍渲染 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3819`
//!
//! **判据（锚点原文）**：十维 S 版、四条件、断链 P0、复用复刻、移交核收、判据。
//!
//! **本条是什么**：S01 组级预备自查——十维矩阵（S 定制：架构/管线/
//! 高对比/焦点/大字号/替代/读屏协同/纹理/光敏/认知）三色制度；
//! GO 预判四条件（十维红零+断链零+预算绿+策略公开）；风险册；移交四件套。
//!
//! 诊断码段：独占 0xCCxx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xCCxx）
// ─────────────────────────────────────────────────────────────

/// F3819 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CCCode(pub u16);

impl CCCode {
    /// 红未清——阻断。
    pub const RED_OPEN: CCCode = CCCode(0xCC01);
    /// 判定缺输入——NO-GO。
    pub const NO_INPUT: CCCode = CCCode(0xCC02);
    /// 断链检出——P0。
    pub const BREAK_P0: CCCode = CCCode(0xCC03);
    /// 黄超期——转红。
    pub const YELLOW_AGED: CCCode = CCCode(0xCC04);
    /// 十维缺维。
    pub const DIM_MISS: CCCode = CCCode(0xCC05);
    /// 移交件缺。
    pub const PACK_MISS: CCCode = CCCode(0xCC06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xCC01 => "十维红项未清，阻断 GO。",
            0xCC02 => "判定输入缺失，判 NO-GO。",
            0xCC03 => "断链检出，P0。",
            0xCC04 => "黄项超期转红。",
            0xCC05 => "十维矩阵缺维。",
            0xCC06 => "移交四件套缺件。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 十维 S 版矩阵
// ─────────────────────────────────────────────────────────────

/// S 维（十维）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SDim {
    /// 架构。
    Arch,
    /// 管线。
    Pipeline,
    /// 高对比。
    HighContrast,
    /// 焦点。
    Focus,
    /// 大字号。
    LargeFont,
    /// 替代。
    Substitute,
    /// 读屏协同。
    ScreenLoop,
    /// 纹理。
    Texture,
    /// 光敏。
    PhotoSafe,
    /// 认知。
    Cognitive,
}

impl SDim {
    /// 全集。
    pub const ALL: [SDim; 10] = [
        SDim::Arch,
        SDim::Pipeline,
        SDim::HighContrast,
        SDim::Focus,
        SDim::LargeFont,
        SDim::Substitute,
        SDim::ScreenLoop,
        SDim::Texture,
        SDim::PhotoSafe,
        SDim::Cognitive,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            SDim::Arch => 0,
            SDim::Pipeline => 1,
            SDim::HighContrast => 2,
            SDim::Focus => 3,
            SDim::LargeFont => 4,
            SDim::Substitute => 5,
            SDim::ScreenLoop => 6,
            SDim::Texture => 7,
            SDim::PhotoSafe => 8,
            SDim::Cognitive => 9,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            SDim::Arch => "架构",
            SDim::Pipeline => "管线",
            SDim::HighContrast => "高对比",
            SDim::Focus => "焦点",
            SDim::LargeFont => "大字号",
            SDim::Substitute => "替代",
            SDim::ScreenLoop => "读屏协同",
            SDim::Texture => "纹理",
            SDim::PhotoSafe => "光敏",
            SDim::Cognitive => "认知",
        }
    }
}

/// 三色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Color {
    /// 绿。
    Green,
    /// 黄。
    Yellow,
    /// 红。
    Red,
}

/// 十维矩阵。
#[derive(Clone, Debug)]
pub struct TenMatrix {
    colors: [Color; 10],
    /// 黄项账龄（超期转红）。
    ages: [u16; 10],
}

impl TenMatrix {
    /// 新建（全绿）。
    pub const fn new() -> Self {
        TenMatrix {
            colors: [Color::Green; 10],
            ages: [0; 10],
        }
    }

    /// 置色。
    pub fn set(&mut self, d: SDim, c: Color) {
        self.colors[d.ordinal()] = c;
    }

    /// 红项数。
    pub fn red_count(&self) -> usize {
        self.colors.iter().filter(|c| **c == Color::Red).count()
    }

    /// 黄项数。
    pub fn yellow_count(&self) -> usize {
        self.colors.iter().filter(|c| **c == Color::Yellow).count()
    }

    /// 红零。
    pub fn red_zero(&self) -> bool {
        self.red_count() == 0
    }

    /// 黄超期转红（账龄 >30 天）。
    pub fn age_tick(&mut self, d: SDim) {
        let i = d.ordinal();
        if self.colors[i] == Color::Yellow {
            self.ages[i] = self.ages[i].saturating_add(1);
            if self.ages[i] > 30 {
                self.colors[i] = Color::Red;
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────
// GO 四条件判定器
// ─────────────────────────────────────────────────────────────

/// 判定结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 通过。
    Go,
    /// 不通过。
    NoGo,
}

/// 四条件判定器。
#[derive(Clone, Debug)]
pub struct GoJudge {
    /// 十维红零。
    pub red_zero: bool,
    /// 断链零。
    pub break_zero: bool,
    /// 预算绿。
    pub budget_green: bool,
    /// 策略公开。
    pub policy_public: bool,
    /// 输入齐备。
    pub inputs_ok: bool,
}

impl GoJudge {
    /// 新建（缺输入）。
    pub const fn new() -> Self {
        GoJudge {
            red_zero: false,
            break_zero: false,
            budget_green: false,
            policy_public: false,
            inputs_ok: false,
        }
    }

    /// 条件通过数。
    pub fn pass_count(&self) -> u32 {
        let mut n = 0;
        if self.red_zero {
            n += 1;
        }
        if self.break_zero {
            n += 1;
        }
        if self.budget_green {
            n += 1;
        }
        if self.policy_public {
            n += 1;
        }
        n
    }

    /// 判定。
    pub fn judge(&self) -> Verdict {
        if !self.inputs_ok {
            return Verdict::NoGo;
        }
        if self.pass_count() == 4 {
            Verdict::Go
        } else {
            Verdict::NoGo
        }
    }
}

// ─────────────────────────────────────────────────────────────
// 风险册 + 移交四件套
// ─────────────────────────────────────────────────────────────

/// 风险项。
#[derive(Clone, Copy, Debug)]
pub struct RiskItem {
    /// 等级（0=低 1=中 2=高）。
    pub level: u8,
    /// 已登记责任方。
    pub owned: bool,
}

/// 风险册。
#[derive(Clone, Debug)]
pub struct RiskBook {
    items: Vec<RiskItem>,
}

impl RiskBook {
    /// 新建。
    pub fn new() -> Self {
        RiskBook { items: Vec::new() }
    }

    /// 登记。
    pub fn push(&mut self, r: RiskItem) {
        self.items.push(r);
    }

    /// 无主风险数。
    pub fn unowned(&self) -> usize {
        self.items.iter().filter(|r| !r.owned).count()
    }

    /// 高风险数。
    pub fn high_count(&self) -> usize {
        self.items.iter().filter(|r| r.level == 2).count()
    }

    /// 总数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl Default for RiskBook {
    fn default() -> Self {
        RiskBook::new()
    }
}

/// 移交件（四件套）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandPack {
    /// 自查报告。
    Report,
    /// 风险册。
    Risk,
    /// 十维矩阵。
    Matrix,
    /// 证据索引。
    Evidence,
}

impl HandPack {
    /// 全集。
    pub const ALL: [HandPack; 4] = [
        HandPack::Report,
        HandPack::Risk,
        HandPack::Matrix,
        HandPack::Evidence,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            HandPack::Report => 0,
            HandPack::Risk => 1,
            HandPack::Matrix => 2,
            HandPack::Evidence => 3,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            HandPack::Report => "自查报告",
            HandPack::Risk => "风险册",
            HandPack::Matrix => "十维矩阵",
            HandPack::Evidence => "证据索引",
        }
    }
}

/// 移交清单。
#[derive(Clone, Debug)]
pub struct Handover {
    packed: [bool; 4],
    /// 核收签。
    pub received: bool,
}

impl Handover {
    /// 新建。
    pub const fn new() -> Self {
        Handover {
            packed: [false; 4],
            received: false,
        }
    }

    /// 装件。
    pub fn pack(&mut self, p: HandPack) -> bool {
        let i = p.ordinal();
        if self.packed[i] {
            false
        } else {
            self.packed[i] = true;
            true
        }
    }

    /// 缺件数。
    pub fn missing(&self) -> usize {
        self.packed.iter().filter(|p| !**p).count()
    }

    /// 四件齐。
    pub fn complete(&self) -> bool {
        self.missing() == 0
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3819 判据集。
pub fn run_f3819_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3819");

    // 0xCC05 十维矩阵
    {
        let mut m = TenMatrix::new();
        set.add("0xCC05-初始红零", m.red_zero() && m.red_count() == 0, "");
        m.set(SDim::Focus, Color::Red);
        set.add("0xCC01-红检出", m.red_count() == 1 && !m.red_zero(), "");
        m.set(SDim::Focus, Color::Green);
        set.add("0xCC01-红清零", m.red_zero(), "");
    }

    // 0xCC04 黄超期转红
    {
        let mut m = TenMatrix::new();
        m.set(SDim::Texture, Color::Yellow);
        set.add("0xCC04-黄计数", m.yellow_count() == 1, "");
        for _ in 0..31 {
            m.age_tick(SDim::Texture);
        }
        set.add("0xCC04-超期转红", m.red_count() == 1, "");
    }

    // 0xCC02 GO 判定
    {
        let mut j = GoJudge::new();
        set.add("0xCC02-缺输入NoGo", j.judge() == Verdict::NoGo, "");
        j.inputs_ok = true;
        j.red_zero = true;
        j.break_zero = true;
        j.budget_green = true;
        j.policy_public = true;
        set.add("0xCC02-四条件Go", j.judge() == Verdict::Go && j.pass_count() == 4, "");
        j.budget_green = false;
        set.add("0xCC02-缺一条NoGo", j.judge() == Verdict::NoGo, "");
    }

    // 风险册
    {
        let mut r = RiskBook::new();
        set.add("0xCC01-空册", r.is_empty() && r.unowned() == 0, "");
        r.push(RiskItem {
            level: 2,
            owned: false,
        });
        r.push(RiskItem {
            level: 1,
            owned: true,
        });
        set.add("0xCC01-无主一", r.unowned() == 1, "");
        set.add("0xCC01-高风险一", r.high_count() == 1, "");
    }

    // 0xCC06 移交四件套
    {
        let mut h = Handover::new();
        set.add("0xCC06-初始缺四", h.missing() == 4, "");
        for p in HandPack::ALL.iter() {
            h.pack(*p);
        }
        set.add("0xCC06-四件齐", h.complete(), "");
        set.add("0xCC06-重装拒", !h.pack(HandPack::Report), "");
    }

    // 维序自洽
    {
        let mut ok = true;
        for (i, d) in SDim::ALL.iter().enumerate() {
            if d.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xCC05-维序自洽", ok, "");
    }

    set
}
