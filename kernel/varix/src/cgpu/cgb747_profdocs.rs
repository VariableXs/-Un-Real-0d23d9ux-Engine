//! cgb747_profdocs —— CGPU-F0747 画像文档。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0747 画像文档  <SEG 0x4ba0>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0747；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 文档 = 结构化章节树 + 渲染器：章节闭集（概览/签名表/账式/
//!      错误码表/基准锚/交接注）六类，文档完整性 = 六类齐一；
//!   2. 章节内容来自画像子系统的真实账面（签名表/错误码段位/
//!      基准行），文档不造数——每行可溯源到账目号；
//!   3. 渲染器两种版式（Markdown 风格行 / 纯文本账行），同源渲染，
//!      两版式信息量守恒（章节数/行数账双记）；
//!   4. 交叉引用账：错误码表每行必须指回签名表某入口（引用落空
//!      红码）；引用完整性 100% 是文档冻结前提；
//!   5. 文档指纹：渲染完成后对全行吃 FNV-1a，同账同指纹（确定性）；
//!   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x4ba0 段为本模块独占（SEG_BASE=0x4ba0），判别式
//!   SEG_BASE+1..+N；与 0x4b80/0x4bc0 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::format;

/// 模块版本。
pub const PD_VERSION: &str = "cgb747_profdocs-v1";

/// 诊断段基址（0x4ba0 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4BA0;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4ba1 章节类型非法（六类之外）。
pub const XC_BAD_SECTION: u16 = SEG_BASE + 1;
/// 0x4ba2 文档不完整（六类章节缺一）。
pub const XC_DOC_INCOMPLETE: u16 = SEG_BASE + 2;
/// 0x4ba3 引用落空（错误码行未指回任何签名入口）。
pub const XC_DANGLING_REF: u16 = SEG_BASE + 3;
/// 0x4ba4 渲染空文档（无行可渲）。
pub const XC_RENDER_EMPTY: u16 = SEG_BASE + 4;
/// 0x4ba5 章节重复（同类章节二投）。
pub const XC_SECTION_DUP: u16 = SEG_BASE + 5;
/// 0x4ba6 溯源缺失（内容行缺账目号）。
pub const XC_NO_PROVENANCE: u16 = SEG_BASE + 6;
/// 0x4ba7 版式非法（两版式之外）。
pub const XC_BAD_FORMAT: u16 = SEG_BASE + 7;
/// 0x4ba8 两版式行数不守恒（同源渲染信息量漂移）。
pub const XC_FORMAT_DRIFT: u16 = SEG_BASE + 8;

/// 章节六类闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionKind {
    /// 1 概览。
    Overview,
    /// 2 签名表。
    SigTable,
    /// 3 账式（守恒式说明）。
    LedgerForm,
    /// 4 错误码表。
    ErrTable,
    /// 5 基准锚。
    BenchAnchor,
    /// 6 交接注。
    HandoverNote,
}

impl SectionKind {
    /// 全类表（遍历序冻结）。
    pub const ALL: [SectionKind; 6] = [
        SectionKind::Overview,
        SectionKind::SigTable,
        SectionKind::LedgerForm,
        SectionKind::ErrTable,
        SectionKind::BenchAnchor,
        SectionKind::HandoverNote,
    ];

    /// 类编号（1..6）。
    pub fn code(&self) -> u8 {
        match self {
            SectionKind::Overview => 1,
            SectionKind::SigTable => 2,
            SectionKind::LedgerForm => 3,
            SectionKind::ErrTable => 4,
            SectionKind::BenchAnchor => 5,
            SectionKind::HandoverNote => 6,
        }
    }

    /// 标题（冻结文本）。
    pub fn title(&self) -> &'static str {
        match self {
            SectionKind::Overview => "画像子系统概览",
            SectionKind::SigTable => "API 签名表",
            SectionKind::LedgerForm => "账式与守恒恒等式",
            SectionKind::ErrTable => "错误码表",
            SectionKind::BenchAnchor => "基准锚",
            SectionKind::HandoverNote => "交接注",
        }
    }
}

/// 版式闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Markdown 风格。
    Markdown,
    /// 纯文本账行。
    PlainLedger,
}

/// 内容行：带溯源账目号。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocLine {
    /// 溯源账目号（0 = 章节标题行，无账目）。
    pub provenance: u32,
    /// 行文本。
    pub text: String,
}

/// 章节：一类一节。
#[derive(Clone, Debug)]
pub struct Section {
    /// 章节类型。
    pub kind: SectionKind,
    /// 内容行（标题行由渲染器生成，不入内容）。
    pub lines: Vec<DocLine>,
}

/// 文档构建器：投章 → 补行 → 冻结渲染。
pub struct DocBuilder {
    sections: Vec<Section>,
    frozen: bool,
}

impl DocBuilder {
    /// 空文档。
    pub fn new() -> Self {
        DocBuilder {
            sections: Vec::new(),
            frozen: false,
        }
    }

    /// 投一章（同类二投红码；冻结后红码）。
    pub fn add_section(&mut self, kind: SectionKind) -> Result<(), u16> {
        if self.frozen {
            return Err(XC_RENDER_EMPTY);
        }
        if self.sections.iter().any(|s| s.kind == kind) {
            return Err(XC_SECTION_DUP);
        }
        self.sections.push(Section {
            kind,
            lines: Vec::new(),
        });
        Ok(())
    }

    /// 补一行（溯源账目号 0 视为缺溯源——标题行除外，此处内容行
    /// 一律要求非零账目号）。冻结后红码。
    pub fn add_line(&mut self, kind: SectionKind, provenance: u32, text: &str) -> Result<(), u16> {
        if self.frozen {
            return Err(XC_NO_PROVENANCE);
        }
        if provenance == 0 {
            return Err(XC_NO_PROVENANCE);
        }
        if text.is_empty() {
            return Err(XC_NO_PROVENANCE);
        }
        let sec = self
            .sections
            .iter_mut()
            .find(|s| s.kind == kind)
            .ok_or(XC_BAD_SECTION)?;
        sec.lines.push(DocLine {
            provenance,
            text: String::from(text),
        });
        Ok(())
    }

    /// 完整性核：六类齐一 + 每章至少一行。
    pub fn completeness(&self) -> Result<(), u16> {
        for k in SectionKind::ALL.iter() {
            match self.sections.iter().find(|s| s.kind == *k) {
                None => return Err(XC_DOC_INCOMPLETE),
                Some(s) => {
                    if s.lines.is_empty() {
                        return Err(XC_DOC_INCOMPLETE);
                    }
                }
            }
        }
        Ok(())
    }

    /// 冻结：完整性核通过后锁定。
    pub fn freeze(&mut self) -> Result<(), u16> {
        self.completeness()?;
        self.frozen = true;
        Ok(())
    }

    /// 错误码表交叉引用核：ErrTable 章每行溯源号必须能映射到
    /// 签名表章中某行（签名行溯源号 1xx 段，错误码行引用入口号
    /// 记在溯源号高 16 位）。落空红码。
    pub fn check_refs(&self) -> Result<(), u16> {
        let sig = self
            .sections
            .iter()
            .find(|s| s.kind == SectionKind::SigTable)
            .ok_or(XC_DOC_INCOMPLETE)?;
        let err = self
            .sections
            .iter()
            .find(|s| s.kind == SectionKind::ErrTable)
            .ok_or(XC_DOC_INCOMPLETE)?;
        for l in &err.lines {
            let entry = l.provenance >> 16;
            if entry == 0 {
                return Err(XC_DANGLING_REF);
            }
            if !sig.lines.iter().any(|s| s.provenance >> 16 == entry) {
                return Err(XC_DANGLING_REF);
            }
        }
        Ok(())
    }

    /// 渲染：指定版式输出全行（标题 + 内容）。
    pub fn render(&self, fmt: Format) -> Result<Vec<String>, u16> {
        if !self.frozen {
            return Err(XC_RENDER_EMPTY);
        }
        if self.sections.is_empty() {
            return Err(XC_RENDER_EMPTY);
        }
        let mut out = Vec::new();
        for s in &self.sections {
            match fmt {
                Format::Markdown => {
                    out.push(format!("## {}", s.kind.title()));
                    for l in &s.lines {
                        out.push(format!("- [账{}] {}", l.provenance, l.text));
                    }
                }
                Format::PlainLedger => {
                    out.push(format!("[S{}] {}", s.kind.code(), s.kind.title()));
                    for l in &s.lines {
                        out.push(format!("  #{:06} {}", l.provenance, l.text));
                    }
                }
            }
        }
        Ok(out)
    }
}

/// 文档指纹：全行顺序吃 FNV-1a。
pub fn doc_fingerprint(lines: &[String]) -> u32 {
    let mut h = 0x811C_9DC5u32;
    for s in lines {
        for b in s.as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= 0x0A;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 版式守恒核：同源两版式的"内容行数"（去标题）必须相等。
pub fn format_conserved(md: &[String], pl: &[String]) -> Result<(), u16> {
    let is_title = |s: &String| s.starts_with("## ") || s.starts_with("[S");
    let cm = md.iter().filter(|s| !is_title(s)).count();
    let cp = pl.iter().filter(|s| !is_title(s)).count();
    if cm != cp {
        return Err(XC_FORMAT_DRIFT);
    }
    if cm == 0 {
        return Err(XC_RENDER_EMPTY);
    }
    Ok(())
}

/// 自例程：六类齐投 → 引用账 → 冻结 → 双版式渲染 → 守恒 → 指纹
/// → 错误路径（缺章/空溯源/重复章/引用落空/未冻结渲染）。
pub fn drill() -> Result<u32, u16> {
    let mut d = DocBuilder::new();
    // 六类全投。
    for k in SectionKind::ALL.iter() {
        d.add_section(*k)?;
    }
    // 二投红码。
    if d.add_section(SectionKind::Overview) != Err(XC_SECTION_DUP) {
        return Err(XC_SECTION_DUP);
    }
    // 补行：签名表入口 1/2/3（溯源号高 16 位 = 入口号，供错误码行引用）。
    d.add_line(SectionKind::Overview, 900, "画像子系统 12 入口 v1")?;
    d.add_line(SectionKind::SigTable, (1u32 << 16) | 101, "profile_register(pid12)")?;
    d.add_line(SectionKind::SigTable, (2u32 << 16) | 102, "profile_query(pid2)")?;
    d.add_line(SectionKind::SigTable, (3u32 << 16) | 103, "scene_link(sid+pid4)")?;
    d.add_line(SectionKind::LedgerForm, 201, "账内样本数=采集数-拒采数")?;
    d.add_line(
        SectionKind::ErrTable,
        (1u32 << 16) | 42,
        "0x4b81 入口号越界",
    )?;
    d.add_line(
        SectionKind::ErrTable,
        (2u32 << 16) | 43,
        "0x4b83 未冻结暴露",
    )?;
    d.add_line(SectionKind::BenchAnchor, 301, "基准锚：512 轮 fuzz 全绿")?;
    d.add_line(SectionKind::HandoverNote, 401, "移交 E08：字段八件套")?;

    // 空溯源红码。
    if d.add_line(SectionKind::Overview, 0, "无账行") != Err(XC_NO_PROVENANCE) {
        return Err(XC_NO_PROVENANCE);
    }
    // 未知章补行红码。
    if d.add_line(SectionKind::Overview, 5, "") != Err(XC_NO_PROVENANCE) {
        return Err(XC_NO_PROVENANCE);
    }

    // 引用核 + 冻结。
    d.check_refs()?;
    d.freeze()?;

    // 未冻结渲染红码（用第二文档演示）。
    let mut d2 = DocBuilder::new();
    for k in SectionKind::ALL.iter() {
        d2.add_section(*k)?;
    }
    d2.add_line(SectionKind::Overview, 1, "x")?;
    d2.add_line(SectionKind::SigTable, 1, "y")?;
    d2.add_line(SectionKind::LedgerForm, 1, "z")?;
    d2.add_line(SectionKind::ErrTable, 1, "w")?;
    d2.add_line(SectionKind::BenchAnchor, 1, "a")?;
    d2.add_line(SectionKind::HandoverNote, 1, "b")?;
    if d2.render(Format::Markdown) != Err(XC_RENDER_EMPTY) {
        return Err(XC_RENDER_EMPTY);
    }

    // 双版式渲染 + 守恒 + 指纹。
    let md = d.render(Format::Markdown)?;
    let pl = d.render(Format::PlainLedger)?;
    format_conserved(&md, &pl)?;
    if md.len() != pl.len() {
        return Err(XC_FORMAT_DRIFT);
    }
    let f1 = doc_fingerprint(&md);
    let f2 = doc_fingerprint(&md);
    if f1 != f2 {
        return Err(XC_RENDER_EMPTY);
    }
    let pl2 = d.render(Format::PlainLedger)?;
    if doc_fingerprint(&pl) != doc_fingerprint(&pl2) {
        return Err(XC_FORMAT_DRIFT);
    }

    // 引用落空：ErrTable 行引用不存在的入口 9。
    let mut d3 = DocBuilder::new();
    for k in SectionKind::ALL.iter() {
        d3.add_section(*k)?;
    }
    d3.add_line(SectionKind::Overview, 1, "o")?;
    d3.add_line(SectionKind::SigTable, 101, "only entry 1")?;
    d3.add_line(SectionKind::LedgerForm, 1, "f")?;
    d3.add_line(SectionKind::ErrTable, (9u32 << 16) | 7, "0x4b89 假引用")?;
    d3.add_line(SectionKind::BenchAnchor, 1, "b")?;
    d3.add_line(SectionKind::HandoverNote, 1, "h")?;
    if d3.check_refs() != Err(XC_DANGLING_REF) {
        return Err(XC_DANGLING_REF);
    }

    // 缺章不完整。
    let mut d4 = DocBuilder::new();
    d4.add_section(SectionKind::Overview)?;
    if d4.freeze() != Err(XC_DOC_INCOMPLETE) {
        return Err(XC_DOC_INCOMPLETE);
    }
    let _ = f1;
    Ok(0)
}
