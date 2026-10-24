//! VE-F3793 · R 域 20 维度验收评分（R10 收口组 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3793`
//!
//! **判据（锚点原文）**：R 版扩展、R 版校准、证据三列、改进项入清账、判据。
//!
//! **本条是什么**：20 维评分（16 标准 + R 四维：协作深度/市场活力/
//! 双维无障碍/心流达成）；八组映射；权重解析；R 版校准法；证据三列
//! （分项/证据/状态）；改进项闭环入清账。
//!
//! 诊断码段：独占 0xB8xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xB8xx）
// ─────────────────────────────────────────────────────────────

/// F3793 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct B8Code(pub u16);

impl B8Code {
    /// 维度缺——补评。
    pub const DIM_MISS: B8Code = B8Code(0xB801);
    /// 低于线——定级。
    pub const BELOW_LINE: B8Code = B8Code(0xB802);
    /// 证据缺/重复——修。
    pub const EVIDENCE_BAD: B8Code = B8Code(0xB803);
    /// 改进项无闭环——不满分。
    pub const IMPROVE_OPEN: B8Code = B8Code(0xB804);
    /// 权重解析失败。
    pub const WEIGHT_BAD: B8Code = B8Code(0xB805);
    /// 20 维不齐。
    pub const TOTAL_MISS: B8Code = B8Code(0xB806);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xB801 => "维度缺评，需补评。",
            0xB802 => "维度分低于门槛，需定级。",
            0xB803 => "证据缺失或重复，需修正。",
            0xB804 => "改进项未闭环，不得满分。",
            0xB805 => "权重解析失败。",
            0xB806 => "20 维评分不齐。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 20 维（16 标准 + R 四维）
// ─────────────────────────────────────────────────────────────

/// 维度枚举（20 项）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dim {
    /// 一致性。
    Consistency,
    /// 可扩展。
    Extensible,
    /// 兼容。
    Compat,
    /// 性能。
    Perf,
    /// 稳定。
    Stable,
    /// 安全。
    Secure,
    /// 可观测。
    Observable,
    /// 可测试。
    Testable,
    /// 文档。
    Docs,
    /// 无障碍。
    A11y,
    /// 国际化。
    I18n,
    /// 可维护。
    Maintain,
    /// 可治理。
    Govern,
    /// 演进。
    Evolve,
    /// 开放。
    Open,
    /// 质量。
    Quality,
    /// 协作深度（R）。
    CollabDepth,
    /// 市场活力（R）。
    MarketVitality,
    /// 双维无障碍（R）。
    DualA11y,
    /// 心流达成（R）。
    FlowAttain,
}

impl Dim {
    /// 全集。
    pub const ALL: [Dim; 20] = [
        Dim::Consistency,
        Dim::Extensible,
        Dim::Compat,
        Dim::Perf,
        Dim::Stable,
        Dim::Secure,
        Dim::Observable,
        Dim::Testable,
        Dim::Docs,
        Dim::A11y,
        Dim::I18n,
        Dim::Maintain,
        Dim::Govern,
        Dim::Evolve,
        Dim::Open,
        Dim::Quality,
        Dim::CollabDepth,
        Dim::MarketVitality,
        Dim::DualA11y,
        Dim::FlowAttain,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Dim::Consistency => 0,
            Dim::Extensible => 1,
            Dim::Compat => 2,
            Dim::Perf => 3,
            Dim::Stable => 4,
            Dim::Secure => 5,
            Dim::Observable => 6,
            Dim::Testable => 7,
            Dim::Docs => 8,
            Dim::A11y => 9,
            Dim::I18n => 10,
            Dim::Maintain => 11,
            Dim::Govern => 12,
            Dim::Evolve => 13,
            Dim::Open => 14,
            Dim::Quality => 15,
            Dim::CollabDepth => 16,
            Dim::MarketVitality => 17,
            Dim::DualA11y => 18,
            Dim::FlowAttain => 19,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Dim::Consistency => "一致性",
            Dim::Extensible => "可扩展",
            Dim::Compat => "兼容",
            Dim::Perf => "性能",
            Dim::Stable => "稳定",
            Dim::Secure => "安全",
            Dim::Observable => "可观测",
            Dim::Testable => "可测试",
            Dim::Docs => "文档",
            Dim::A11y => "无障碍",
            Dim::I18n => "国际化",
            Dim::Maintain => "可维护",
            Dim::Govern => "可治理",
            Dim::Evolve => "演进",
            Dim::Open => "开放",
            Dim::Quality => "质量",
            Dim::CollabDepth => "协作深度",
            Dim::MarketVitality => "市场活力",
            Dim::DualA11y => "双维无障碍",
            Dim::FlowAttain => "心流达成",
        }
    }

    /// 是否 R 扩展维。
    pub fn is_r_ext(self) -> bool {
        self.ordinal() >= 16
    }
}

/// 评分档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// 红。
    Red,
    /// 黄。
    Yellow,
    /// 绿。
    Green,
}

/// 维度评分。
#[derive(Clone, Copy, Debug)]
pub struct DimScore {
    /// 分（0..100）。
    pub score: u8,
    /// 证据分项数。
    pub evidence: u8,
    /// 改进项已闭环。
    pub improved: bool,
    /// 权重（万分比）。
    pub weight_bp: u16,
}

impl DimScore {
    /// 新建。
    pub const fn new(score: u8, weight_bp: u16) -> Self {
        DimScore {
            score,
            evidence: 0,
            improved: false,
            weight_bp,
        }
    }

    /// 定级（≥85 绿 / ≥60 黄 / 红）。
    pub fn grade(self) -> Grade {
        if self.score >= 85 {
            Grade::Green
        } else if self.score >= 60 {
            Grade::Yellow
        } else {
            Grade::Red
        }
    }

    /// 证据三列齐（分项>0）。
    pub fn evidence_ok(self) -> bool {
        self.evidence > 0
    }

    /// 有效分（无闭环改进项时封顶 84）。
    pub fn effective(self) -> u8 {
        if self.improved {
            self.score
        } else if self.score > 84 {
            84
        } else {
            self.score
        }
    }
}

/// 评分册。
#[derive(Clone, Debug)]
pub struct ScoreBook {
    scores: Vec<(Dim, DimScore)>,
}

impl ScoreBook {
    /// 新建。
    pub fn new() -> Self {
        ScoreBook {
            scores: Vec::new(),
        }
    }

    /// 记分。
    pub fn put(&mut self, d: Dim, s: DimScore) {
        // 去重
        for e in self.scores.iter_mut() {
            if e.0 == d {
                e.1 = s;
                return;
            }
        }
        self.scores.push((d, s));
    }

    /// 已评数。
    pub fn count(&self) -> usize {
        self.scores.len()
    }

    /// 缺维数。
    pub fn missing(&self) -> usize {
        20 - self.count().min(20)
    }

    /// 红项数。
    pub fn red_count(&self) -> usize {
        self.scores
            .iter()
            .filter(|(_, s)| s.grade() == Grade::Red)
            .count()
    }

    /// 加权总分（万分制）。
    pub fn weighted_bp(&self) -> u32 {
        let mut acc = 0u32;
        let mut wsum = 0u32;
        for (_, s) in self.scores.iter() {
            acc += (s.effective() as u32) * (s.weight_bp as u32);
            wsum += s.weight_bp as u32;
        }
        if wsum == 0 {
            0
        } else {
            acc / wsum
        }
    }

    /// 证据缺数。
    pub fn evidence_missing(&self) -> usize {
        self.scores
            .iter()
            .filter(|(_, s)| !s.evidence_ok())
            .count()
    }

    /// 未闭环改进项数（有效分被封顶）。
    pub fn open_improve(&self) -> usize {
        self.scores
            .iter()
            .filter(|(_, s)| !s.improved && s.score > 84)
            .count()
    }
}

impl Default for ScoreBook {
    fn default() -> Self {
        ScoreBook::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 权重解析
// ─────────────────────────────────────────────────────────────

/// 权重表（20 维权重，万分比总和须 10000）。
#[derive(Clone, Debug)]
pub struct WeightTable {
    w: [u16; 20],
}

impl WeightTable {
    /// 均权新建。
    pub const fn uniform() -> Self {
        WeightTable { w: [500; 20] }
    }

    /// 设权。
    pub fn set(&mut self, d: Dim, bp: u16) -> bool {
        self.w[d.ordinal()] = bp;
        true
    }

    /// 总和。
    pub fn sum(&self) -> u32 {
        self.w.iter().map(|x| *x as u32).sum()
    }

    /// 解析合法（总和 = 10000）。
    pub fn valid(&self) -> bool {
        self.sum() == 10_000
    }

    /// 取权。
    pub fn get(&self, d: Dim) -> u16 {
        self.w[d.ordinal()]
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3793 判据集。
pub fn run_vr10m_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3793");

    // 0xB806 20 维
    {
        let mut b = ScoreBook::new();
        set.add("0xB806-初始缺20", b.missing() == 20, "");
        for d in Dim::ALL.iter() {
            b.put(*d, DimScore::new(90, 500));
        }
        set.add("0xB806-20维齐", b.count() == 20 && b.missing() == 0, "");
        set.add("0xB802-红零", b.red_count() == 0, "");
    }

    // 0xB802 定级
    {
        set.add("0xB802-90绿", DimScore::new(90, 1).grade() == Grade::Green, "");
        set.add("0xB802-70黄", DimScore::new(70, 1).grade() == Grade::Yellow, "");
        set.add("0xB802-50红", DimScore::new(50, 1).grade() == Grade::Red, "");
    }

    // 0xB803 证据三列
    {
        let s = DimScore::new(90, 1);
        set.add("0xB803-零证据不过", !s.evidence_ok(), "");
        let mut s2 = DimScore::new(90, 1);
        s2.evidence = 3;
        set.add("0xB803-有证据过", s2.evidence_ok(), "");
    }

    // 0xB804 改进封顶
    {
        let s = DimScore::new(95, 1);
        set.add("0xB804-未闭环封顶", s.effective() == 84, "");
        let mut s2 = DimScore::new(95, 1);
        s2.improved = true;
        set.add("0xB804-闭环解封", s2.effective() == 95, "");
        let s3 = DimScore::new(50, 1);
        set.add("0xB804-低分不封", s3.effective() == 50, "");
    }

    // 0xB805 权重
    {
        let w = WeightTable::uniform();
        set.add("0xB805-均权和", w.sum() == 10_000, "");
        set.add("0xB805-均权合法", w.valid(), "");
        let mut w2 = WeightTable::uniform();
        w2.set(Dim::Perf, 600);
        set.add("0xB805-改权非法", !w2.valid(), "");
    }

    // 加权总分
    {
        let mut b = ScoreBook::new();
        b.put(Dim::Perf, DimScore::new(100, 1000));
        b.put(Dim::Docs, DimScore::new(0, 1000));
        set.add("0xB801-加权均分", b.weighted_bp() == 5000, "");
        b.put(Dim::Perf, DimScore::new(84, 1000));
        set.add("0xB804-封顶入账", b.weighted_bp() == 4200, "");
    }

    // 证据缺计数
    {
        let mut b = ScoreBook::new();
        let mut s = DimScore::new(90, 1);
        s.evidence = 1;
        b.put(Dim::Secure, s);
        b.put(Dim::Open, DimScore::new(90, 1));
        set.add("0xB803-一缺证据", b.evidence_missing() == 1, "");
    }

    // R 扩展维标记
    {
        set.add("0xB801-协作是R", Dim::CollabDepth.is_r_ext(), "");
        set.add("0xB801-心流是R", Dim::FlowAttain.is_r_ext(), "");
        set.add("0xB801-安全非R", !Dim::Secure.is_r_ext(), "");
    }

    // 维序自洽
    {
        let mut ok = true;
        for (i, d) in Dim::ALL.iter().enumerate() {
            if d.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xB806-维序自洽", ok, "");
    }

    set
}
