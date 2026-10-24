//! VE-F3843 · 文本渲染读屏标注
//!
//! 渲染文本→语义标注（语言/层级/角色/拼写四标注）。覆盖 100% 红线。
//! 同帧同步。结构语义。信息黑洞红线。

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

// ── 诊断码段（0xBFxx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xBF00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CBFCode(pub u16);

impl CBFCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const COVERAGE_SHORT: Self = Self(SEG_BASE + 1);
    pub const SYNC_BROKEN: Self = Self(SEG_BASE + 2);
    pub const STRUCT_MISSING: Self = Self(SEG_BASE + 3);
    pub const BUDGET_EXCEEDED: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::COVERAGE_SHORT => "标注覆盖<100%（信息黑洞红线）",
            Self::SYNC_BROKEN => "同帧同步破（P0）",
            Self::STRUCT_MISSING => "结构语义缺（P1）",
            Self::BUDGET_EXCEEDED => "标注开销超预算",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────

/// 文本角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextRole {
    /// 段落。
    Paragraph,
    /// 标题。
    Heading,
    /// 列表项。
    ListItem,
    /// 表格单元。
    TableCell,
}

impl TextRole {
    pub fn name(self) -> &'static str {
        match self {
            Self::Paragraph => "段落",
            Self::Heading => "标题",
            Self::ListItem => "列表项",
            Self::TableCell => "表格单元",
        }
    }
}

/// 四标注模型。
#[derive(Debug, Clone)]
pub struct TextAnnotation {
    /// 节点 id。
    pub node_id: u32,
    /// 语言（BCP-47 简化）。
    pub language: String,
    /// 层级（标题 1..6，其他 0）。
    pub level: u8,
    /// 角色。
    pub role: TextRole,
    /// 拼写检查开启。
    pub spellcheck: bool,
    /// 帧号（同帧同步断言）。
    pub frame: u64,
}

impl TextAnnotation {
    /// 四标注齐备。
    pub fn complete(&self) -> bool {
        !self.language.is_empty() && (self.role != TextRole::Heading || self.level >= 1)
    }
}

/// 文本块（渲染侧）。
#[derive(Debug, Clone)]
pub struct RenderedText {
    pub node_id: u32,
    pub content: String,
    pub frame: u64,
    /// 是否已标注。
    pub annotated: bool,
}

/// 标注器。
#[derive(Debug, Clone)]
pub struct TextAnnotator {
    /// 标注表。
    pub annotations: Vec<TextAnnotation>,
    /// 渲染文本块。
    pub texts: Vec<RenderedText>,
    /// 预算（每帧标注数上限）。
    pub budget_per_frame: u32,
}

impl TextAnnotator {
    pub fn new(budget: u32) -> Self {
        Self {
            annotations: Vec::new(),
            texts: Vec::new(),
            budget_per_frame: budget,
        }
    }

    /// 覆盖率（标注数 / 文本数，×100）。
    pub fn coverage_pct(&self) -> u32 {
        if self.texts.is_empty() {
            return 100;
        }
        let hit = self.texts.iter().filter(|t| t.annotated).count();
        (hit as u32 * 100 / self.texts.len() as u32)
    }

    /// 覆盖红线：100%。
    pub fn coverage_ok(&self) -> bool {
        self.coverage_pct() == 100
    }

    /// 同帧：标注帧 == 文本帧。
    pub fn sync_intact(&self) -> bool {
        for a in self.annotations.iter() {
            let t = self.texts.iter().find(|t| t.node_id == a.node_id);
            match t {
                Some(t) if t.frame == a.frame => {}
                _ => return false,
            }
        }
        true
    }

    /// 结构语义：标题层级 ≥1。
    pub fn struct_intact(&self) -> bool {
        self.annotations
            .iter()
            .all(|a| a.role != TextRole::Heading || a.level >= 1)
    }

    /// 预算：单帧标注数 ≤ budget。
    pub fn within_budget(&self) -> bool {
        let mut counts: Vec<(u64, u32)> = Vec::new();
        for a in self.annotations.iter() {
            let mut found = false;
            for e in counts.iter_mut() {
                if e.0 == a.frame {
                    e.1 += 1;
                    found = true;
                    break;
                }
            }
            if !found {
                counts.push((a.frame, 1));
            }
        }
        counts.iter().all(|(_, c)| *c <= self.budget_per_frame)
    }

    /// 全检。
    pub fn full_check(&self) -> Result<(), CBFCode> {
        if !self.coverage_ok() {
            return Err(CBFCode::COVERAGE_SHORT);
        }
        if !self.sync_intact() {
            return Err(CBFCode::SYNC_BROKEN);
        }
        if !self.struct_intact() {
            return Err(CBFCode::STRUCT_MISSING);
        }
        if !self.within_budget() {
            return Err(CBFCode::BUDGET_EXCEEDED);
        }
        Ok(())
    }

    /// 标注一块文本。
    pub fn annotate(&mut self, node_id: u32, language: &str, level: u8, role: TextRole, frame: u64) {
        self.annotations.push(TextAnnotation {
            node_id,
            language: String::from(language),
            level,
            role,
            spellcheck: true,
            frame,
        });
        for t in self.texts.iter_mut() {
            if t.node_id == node_id {
                t.annotated = true;
                t.frame = frame;
            }
        }
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves143_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves143_textannot");

    fn ready() -> TextAnnotator {
        let mut a = TextAnnotator::new(8);
        a.texts.push(RenderedText {
            node_id: 1,
            content: String::from("标题"),
            frame: 10,
            annotated: true,
        });
        a.texts.push(RenderedText {
            node_id: 2,
            content: String::from("正文"),
            frame: 10,
            annotated: true,
        });
        a.annotate(1, "zh-Hans", 1, TextRole::Heading, 10);
        a.annotate(2, "zh-Hans", 0, TextRole::Paragraph, 10);
        a
    }

    // 01 覆盖 100%
    {
        let a = ready();
        chk(&mut set, "C3843-01", a.coverage_ok() && a.coverage_pct() == 100, "err");
    }
    // 02 覆盖不足（信息黑洞）
    {
        let mut a = ready();
        a.texts[1].annotated = false;
        chk(
            &mut set,
            "C3843-02",
            a.full_check() == Err(CBFCode::COVERAGE_SHORT),
            "err",
        );
    }
    // 03 同帧
    {
        let a = ready();
        chk(&mut set, "C3843-03", a.sync_intact(), "err");
    }
    // 04 同步破
    {
        let mut a = ready();
        a.annotations[0].frame = 11;
        chk(
            &mut set,
            "C3843-04",
            a.full_check() == Err(CBFCode::SYNC_BROKEN),
            "err",
        );
    }
    // 05 结构语义
    {
        let a = ready();
        chk(&mut set, "C3843-05", a.struct_intact(), "err");
    }
    // 06 结构缺
    {
        let mut a = ready();
        a.annotations[0].level = 0;
        chk(
            &mut set,
            "C3843-06",
            a.full_check() == Err(CBFCode::STRUCT_MISSING),
            "err",
        );
    }
    // 07 预算内
    {
        let a = ready();
        chk(&mut set, "C3843-07", a.within_budget(), "err");
    }
    // 08 预算超
    {
        let mut a = ready();
        a.budget_per_frame = 1;
        chk(
            &mut set,
            "C3843-08",
            a.full_check() == Err(CBFCode::BUDGET_EXCEEDED),
            "err",
        );
    }
    // 09 全检通过
    {
        let a = ready();
        chk(&mut set, "C3843-09", a.full_check().is_ok(), "err");
    }
    // 10 空集覆盖
    {
        let a = TextAnnotator::new(4);
        chk(&mut set, "C3843-10", a.coverage_ok(), "err");
    }
    // 11 四标注齐
    {
        let a = ready();
        chk(&mut set, "C3843-11", a.annotations.iter().all(|x| x.complete()), "err");
    }
    // 12 角色命名
    {
        chk(
            &mut set,
            "C3843-12",
            TextRole::Heading.name() == "标题" && TextRole::TableCell.name() == "表格单元",
            "err",
        );
    }

    set
}
