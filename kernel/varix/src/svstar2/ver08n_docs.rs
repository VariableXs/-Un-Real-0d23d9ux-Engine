//! VE-F3514 · 色温场景文档 —— 使用指南 + 语言手册 + 诚实健康表述 + CI 防漂移。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3514`
//!
//! # 职责（锚点原文拆解）
//!
//! - **使用指南**（[`GuideBook`]）：色温系统的使用文档以**结构化
//!   条目**存在本册（标题/正文/锚定 API 三元组），文档不是散落的
//!   字符串——每条都有锚定对象，锚定对象改名文档即失锚。
//! - **语言手册**：F3508 场景定义语言的手册——三节语法、值域、
//!   示例，与语言实现同源（示例必须能被 [`super::ver08h_scenedsl::parse_def`]
//!   解析通过，CI 防漂移的兑现形式）。
//! - **诚实健康表述**（[`HealthClaim`]）：护眼效果的表述**说有据的
//!   不说吓人的**——色温的护眼证据有限，休息比调色温更重要。
//!   话术白名单：每条健康文案带证据等级（强/有限/无），「无」级
//!   文案禁止使用断言句式。免责边界写进文档本体。
//! - **CI 防漂移**（[`DocCI`]）：文档条目按锚定对象签名核对，
//!   失锚/手册示例解析失败/健康话术越界，CI 检出即阻断收口。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 漂移 | CI 阻断 |
//! | 手册过时 | 复审（失锚标记） |
//! | 缺失 | 收口阻断 |
//!
//! # 复杂度
//!
//! 全部 O(条目数)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5810 起）
// ---------------------------------------------------------------------------

/// 色温文档域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocCode {
    /// 文档条目失锚（锚定对象已改名/删除）。
    AnchorLost = 5810,
    /// 手册示例解析失败。
    SampleBroken = 5811,
    /// 健康话术越界（无证据级文案用了断言句式）。
    ClaimOverreach = 5812,
    /// 收口必备文档缺失。
    DocMissing = 5813,
}

impl DocCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            DocCode::AnchorLost => "文档条目失锚",
            DocCode::SampleBroken => "手册示例解析失败",
            DocCode::ClaimOverreach => "健康话术越界",
            DocCode::DocMissing => "收口必备文档缺失",
        }
    }

    /// 全阻断（文档是收口六件之一）。
    pub fn blocking(self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// 二、条目与锚定
// ---------------------------------------------------------------------------

/// 必备文档类（收口六件核对用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocKind {
    /// 系统使用指南。
    Guide,
    /// 场景语言手册。
    LanguageManual,
    /// 健康建议说明。
    HealthNote,
}

/// 一条文档条目。
#[derive(Clone, Debug)]
pub struct DocEntry {
    /// 类别。
    pub kind: DocKind,
    /// 标题。
    pub title: &'static str,
    /// 正文。
    pub body: &'static str,
    /// 锚定对象（符号名，如 "ver08a_kelvin::KelvinEngine"）。
    pub anchor: &'static str,
}

/// 文档册。
#[derive(Default)]
pub struct GuideBook {
    pub entries: Vec<DocEntry>,
}

impl GuideBook {
    /// 空册。
    pub fn new() -> Self {
        Self::default()
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 挂条目。
    pub fn mount(&mut self, e: DocEntry) {
        self.entries.push(e);
    }

    /// 锚点核对：现存符号表里找不到锚即失锚。
    pub fn check_anchors(&self, symbols: &[&str]) -> Result<(), DocCode> {
        for e in &self.entries {
            if !symbols.contains(&e.anchor) {
                return Err(DocCode::AnchorLost);
            }
        }
        Ok(())
    }

    /// 必备类核对：三类文档各至少一条，缺失即收口阻断。
    pub fn check_required(&self) -> Result<(), DocCode> {
        for k in [DocKind::Guide, DocKind::LanguageManual, DocKind::HealthNote] {
            if !self.entries.iter().any(|e| e.kind == k) {
                return Err(DocCode::DocMissing);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、语言手册（与实现同源）
// ---------------------------------------------------------------------------

/// 手册示例（必须能被 F3508 解析器吃下）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ManualSample {
    /// 示例名。
    pub name: &'static str,
    /// 示例文本（三节）。
    pub text: &'static str,
}

/// 手册示例集（CI 逐条跑解析器）。
pub const MANUAL_SAMPLES: [ManualSample; 3] = [
    ManualSample {
        name: "工作场景",
        text: "schedule: 08:00-18:00\ncurve: 6500\ntrigger: clock",
    },
    ManualSample {
        name: "夜间场景",
        text: "schedule: 22:00-06:00\ncurve: 2700\ntrigger: clock",
    },
    ManualSample {
        name: "阅读场景（联动专注）",
        text: "schedule: 19:00-22:00\ncurve: 4000\ntrigger: focus:1",
    },
];

/// 手册 CI：逐条示例过解析器，任一失败即漂移。
pub fn check_manual() -> Result<usize, DocCode> {
    let mut ok = 0;
    for s in &MANUAL_SAMPLES {
        let lines: Vec<&str> = s.text.lines().collect();
        match super::ver08h_scenedsl::parse_def("ci", &lines) {
            super::ver08h_scenedsl::DefOutcome::Ok(_) => ok += 1,
            super::ver08h_scenedsl::DefOutcome::Bad { .. } => {
                return Err(DocCode::SampleBroken)
            }
        }
    }
    Ok(ok)
}

// ---------------------------------------------------------------------------
// 四、健康话术白名单
// ---------------------------------------------------------------------------

/// 证据等级。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Evidence {
    /// 证据充分（可断言）。
    Strong,
    /// 证据有限（只可陈述研究现状）。
    Limited,
    /// 无证据（只可说「尚无证据」）。
    None,
}

/// 一条健康话术。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HealthClaim {
    /// 文案。
    pub text: &'static str,
    /// 证据等级。
    pub evidence: Evidence,
    /// 是否断言句式（「X 能 Y」式）。
    pub assertive: bool,
}

/// 话术核对：无证据级禁止断言；有限证据级只许陈述式。
pub fn check_claims(claims: &[HealthClaim]) -> Result<(), DocCode> {
    for c in claims {
        let ok = match c.evidence {
            Evidence::Strong => true,
            Evidence::Limited => !c.assertive,
            Evidence::None => !c.assertive,
        };
        if !ok {
            return Err(DocCode::ClaimOverreach);
        }
    }
    Ok(())
}

/// 内置健康话术（诚实基线）。
pub const BUILTIN_CLAIMS: [HealthClaim; 3] = [
    HealthClaim {
        text: "降低夜间色温可能减少蓝光暴露，其对睡眠的益处证据有限",
        evidence: Evidence::Limited,
        assertive: false,
    },
    HealthClaim {
        text: "定时休息与保持用眼距离比调色温更重要",
        evidence: Evidence::Strong,
        assertive: true,
    },
    HealthClaim {
        text: "尚无证据表明色温调节能治疗任何眼部疾病",
        evidence: Evidence::None,
        assertive: false,
    },
];

/// 免责边界文本（写进文档本体）。
pub const HEALTH_DISCLAIMER: &str =
    "色温功能的护眼效果表述以证据等级标注为准；本功能不构成医疗建议。";

// ---------------------------------------------------------------------------
// 五、CI 门
// ---------------------------------------------------------------------------

/// 文档 CI 门：锚点/手册/话术/必备类四查。
#[derive(Default)]
pub struct DocCI {
    pub runs: u32,
}

impl DocCI {
    /// 全查。`symbols` 是现役符号表白名单（模拟符号表）。
    pub fn run(&mut self, book: &GuideBook, symbols: &[&str]) -> Result<usize, DocCode> {
        book.check_required()?;
        book.check_anchors(symbols)?;
        let n = check_manual()?;
        check_claims(&BUILTIN_CLAIMS)?;
        self.runs += 1;
        Ok(book.len() + n)
    }
}

// ---------------------------------------------------------------------------
// 六、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_book() -> GuideBook {
        let mut b = GuideBook::new();
        b.mount(DocEntry {
            kind: DocKind::Guide,
            title: "色温使用指南",
            body: "如何开启夜览与场景",
            anchor: "ver08a_kelvin::KelvinEngine",
        });
        b.mount(DocEntry {
            kind: DocKind::LanguageManual,
            title: "场景定义语言手册",
            body: "三节语法",
            anchor: "ver08h_scenedsl::parse_def",
        });
        b.mount(DocEntry {
            kind: DocKind::HealthNote,
            title: "护眼说明",
            body: HEALTH_DISCLAIMER,
            anchor: "ver08n_docs::HEALTH_DISCLAIMER",
        });
        b
    }

    #[test]
    fn required_docs_gate() {
        let mut b = GuideBook::new();
        assert_eq!(b.check_required(), Err(DocCode::DocMissing));
        b = sample_book();
        assert!(b.check_required().is_ok());
    }

    #[test]
    fn anchor_lost_detected() {
        let b = sample_book();
        assert_eq!(
            b.check_anchors(&["ver08a_kelvin::KelvinEngine"]),
            Err(DocCode::AnchorLost)
        );
        let symbols = [
            "ver08a_kelvin::KelvinEngine",
            "ver08h_scenedsl::parse_def",
            "ver08n_docs::HEALTH_DISCLAIMER",
        ];
        assert!(b.check_anchors(&symbols).is_ok());
    }

    #[test]
    fn manual_samples_parse() {
        assert_eq!(check_manual().unwrap(), MANUAL_SAMPLES.len());
    }

    #[test]
    fn overreach_rejected() {
        let bad = [HealthClaim {
            text: "调色温能治近视",
            evidence: Evidence::None,
            assertive: true,
        }];
        assert_eq!(check_claims(&bad), Err(DocCode::ClaimOverreach));
        assert!(check_claims(&BUILTIN_CLAIMS).is_ok());
    }
}
