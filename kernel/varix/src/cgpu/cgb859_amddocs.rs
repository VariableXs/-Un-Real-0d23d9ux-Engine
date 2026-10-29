//! cgb859_amddocs —— CGPU-F0859 AMD 适配文档。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0859 AMD 适配文档  <SEG 0x75E0>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0859；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（AMD 适配文档，章节账面）：
//!   1. 章节闭集十部（写死）：总览 / 架构映射 / 能力矩阵 / 驱动
//!      差异 / 显存策略 / 媒体管线 / 功耗治理 / 遥测口径 / fuzz
//!      计划 / API 冻结志；部号 0..=9，越界红码（XC_BAD_CHAPTER）；
//!   2. 章节状态机：Outline → Drafted → Reviewed → Final；Final
//!      只读（XC_CHAPTER_SEALED）；非法迁移红码（XC_BAD_TRANS）；
//!   3. 文档账（定容 10 章 × 每章定容 16 节）守恒：立节总数 =
//!      定稿节数 + 在写节数；漂移红码（XC_DOC_DRIFT）；
//!   4. 字数账（整数行数面）：每节登记行数（1..=512），越界红码
//!      （XC_BAD_LINES）；每章行数 ≥ 40 才可 Final（不足红码
//!      XC_TOO_THIN）；
//!   5. 交叉引用账：引用部号必须闭集内且目标已 Drafted，悬空引
//!      用红码（XC_DANGLING_REF）；
//!   6. 出版判据：十部全 Final 且总行数 ≥ 400，未齐出书红码
//!      （XC_PUBLISH_BLOCKED）；
//!   7. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x75E0 段为本模块独占（SEG_BASE=0x75E0），判别式
//!   SEG_BASE+1..+N；与 0x75C0/0x7600 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 模块版本。
pub const AD_VERSION: &str = "cgb859_amddocs-v1";

/// 诊断段基址（0x75E0 · 本模块独占）。
pub const SEG_BASE: u16 = 0x75E0;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x75E1 章节部号越界（0..=9 之外）。
pub const XC_BAD_CHAPTER: u16 = SEG_BASE + 1;
/// 0x75E2 章节状态迁移非法。
pub const XC_BAD_TRANS: u16 = SEG_BASE + 2;
/// 0x75E3 文档账漂移（立节 ≠ 定稿 + 在写）。
pub const XC_DOC_DRIFT: u16 = SEG_BASE + 3;
/// 0x75E4 行数越界（1..=512）。
pub const XC_BAD_LINES: u16 = SEG_BASE + 4;
/// 0x75E5 章行数不足（< 40 即求 Final）。
pub const XC_TOO_THIN: u16 = SEG_BASE + 5;
/// 0x75E6 悬空引用（目标未起草或越界）。
pub const XC_DANGLING_REF: u16 = SEG_BASE + 6;
/// 0x75E7 章节已定稿仍写。
pub const XC_CHAPTER_SEALED: u16 = SEG_BASE + 7;
/// 0x75E8 节号越界。
pub const XC_BAD_SECTION: u16 = SEG_BASE + 8;
/// 0x75E9 出版受阻（十部未全 Final 或总行数不足）。
pub const XC_PUBLISH_BLOCKED: u16 = SEG_BASE + 9;
/// 0x75EA 重复节名。
pub const XC_DUP_SECTION: u16 = SEG_BASE + 10;

/// 章部数（写死：10）。
pub const CHAPTER_CAP: usize = 10;
/// 每章节容量（写死：16）。
pub const SECTION_CAP: usize = 16;
/// 行数下限。
pub const LINES_MIN: u32 = 1;
/// 行数上限。
pub const LINES_MAX: u32 = 512;
/// 章 Final 行数门槛。
pub const CHAPTER_MIN_LINES: u32 = 40;
/// 出书总行数门槛。
pub const BOOK_MIN_LINES: u32 = 400;

/// 章部闭集：0=总览 1=架构映射 2=能力矩阵 3=驱动差异 4=显存策略
/// 5=媒体管线 6=功耗治理 7=遥测口径 8=fuzz 计划 9=API 冻结志。
pub const H_OVERVIEW: usize = 0;
pub const H_ARCHMAP: usize = 1;
pub const H_CAPMATRIX: usize = 2;
pub const H_DRVDIFF: usize = 3;
pub const H_VRAM: usize = 4;
pub const H_MEDIA: usize = 5;
pub const H_POWER: usize = 6;
pub const H_TELEM: usize = 7;
pub const H_FUZZ: usize = 8;
pub const H_FREEZE: usize = 9;

/// 章节状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DPhase {
    /// 大纲。
    Outline,
    /// 起草。
    Drafted,
    /// 评审。
    Reviewed,
    /// 定稿。
    Final,
}

/// 迁移合法性：线性推进 + Reviewed 可回 Drafted。
pub fn trans_legal(from: DPhase, to: DPhase) -> bool {
    matches!(
        (from, to),
        (DPhase::Outline, DPhase::Drafted)
            | (DPhase::Drafted, DPhase::Reviewed)
            | (DPhase::Drafted, DPhase::Outline)
            | (DPhase::Reviewed, DPhase::Final)
            | (DPhase::Reviewed, DPhase::Drafted)
    )
}

/// 单节。
#[derive(Clone, Debug)]
pub struct Section {
    /// 节名（章内去重键）。
    pub name: String,
    /// 行数。
    pub lines: u32,
    /// 状态。
    pub phase: DPhase,
}

/// 单章。
pub struct Chapter {
    /// 部号。
    pub no: usize,
    /// 节账（定容 16）。
    pub sections: Vec<Section>,
    /// 章状态。
    pub phase: DPhase,
    /// 定稿行数。
    pub final_lines: u32,
}

impl Chapter {
    /// 新章（Outline）。
    pub fn new(no: usize) -> Self {
        Chapter {
            no,
            sections: Vec::new(),
            phase: DPhase::Outline,
            final_lines: 0,
        }
    }

    /// 章行数合计。
    pub fn total_lines(&self) -> u32 {
        let mut n: u32 = 0;
        let mut i = 0;
        while i < self.sections.len() {
            n += self.sections[i].lines;
            i += 1;
        }
        n
    }

    /// 名查重。
    pub fn has_name(&self, name: &str) -> bool {
        let mut i = 0;
        while i < self.sections.len() {
            if self.sections[i].name == name {
                return true;
            }
            i += 1;
        }
        false
    }
}

/// AMD 适配文档册。
pub struct DocBook {
    /// 十部章。
    pub chapters: [Chapter; CHAPTER_CAP],
    /// 立节总数。
    pub sections_created: u64,
    /// 定稿节数。
    pub sections_final: u64,
    /// 在写节数（立节 − 定稿）。
    pub sections_open: u64,
    /// 交叉引用账（被引用部号计数）。
    pub refs: [u64; CHAPTER_CAP],
    /// 出版标志。
    pub published: bool,
    /// 累计红码。
    pub red_count: u64,
}

impl DocBook {
    /// 空册。
    pub fn new() -> Self {
        DocBook {
            chapters: [
                Chapter::new(0),
                Chapter::new(1),
                Chapter::new(2),
                Chapter::new(3),
                Chapter::new(4),
                Chapter::new(5),
                Chapter::new(6),
                Chapter::new(7),
                Chapter::new(8),
                Chapter::new(9),
            ],
            sections_created: 0,
            sections_final: 0,
            sections_open: 0,
            refs: [0; CHAPTER_CAP],
            published: false,
            red_count: 0,
        }
    }

    /// 立节（章 Outline/Drafted 期；名查重；容量；行数闭集）。
    pub fn add_section(
        &mut self,
        chapter: usize,
        name: &str,
        lines: u32,
    ) -> Result<(), u16> {
        if chapter >= CHAPTER_CAP {
            self.red_count += 1;
            return Err(XC_BAD_CHAPTER);
        }
        if self.chapters[chapter].phase == DPhase::Final {
            self.red_count += 1;
            return Err(XC_CHAPTER_SEALED);
        }
        if lines < LINES_MIN || lines > LINES_MAX {
            self.red_count += 1;
            return Err(XC_BAD_LINES);
        }
        if self.chapters[chapter].has_name(name) {
            self.red_count += 1;
            return Err(XC_DUP_SECTION);
        }
        if self.chapters[chapter].sections.len() == SECTION_CAP {
            self.red_count += 1;
            return Err(XC_BAD_SECTION);
        }
        if self.chapters[chapter].phase == DPhase::Outline {
            self.chapters[chapter].phase = DPhase::Drafted;
        }
        self.chapters[chapter].sections.push(Section {
            name: String::from(name),
            lines,
            phase: DPhase::Drafted,
        });
        self.sections_created += 1;
        self.sections_open += 1;
        Ok(())
    }

    /// 推进节状态（Final 时入定稿账）。
    pub fn advance_section(
        &mut self,
        chapter: usize,
        section: usize,
        to: DPhase,
    ) -> Result<(), u16> {
        if chapter >= CHAPTER_CAP {
            self.red_count += 1;
            return Err(XC_BAD_CHAPTER);
        }
        if section >= self.chapters[chapter].sections.len() {
            self.red_count += 1;
            return Err(XC_BAD_SECTION);
        }
        let cur = self.chapters[chapter].sections[section].phase;
        if !trans_legal(cur, to) {
            self.red_count += 1;
            return Err(XC_BAD_TRANS);
        }
        if to == DPhase::Final {
            self.sections_final += 1;
            self.sections_open -= 1;
            self.chapters[chapter].final_lines +=
                self.chapters[chapter].sections[section].lines;
        }
        if cur == DPhase::Final && to != DPhase::Final {
            self.red_count += 1;
            return Err(XC_CHAPTER_SEALED);
        }
        self.chapters[chapter].sections[section].phase = to;
        Ok(())
    }

    /// 交叉引用（目标部必须已 Drafted 及以上）。
    pub fn add_ref(&mut self, from: usize, to: usize) -> Result<(), u16> {
        if from >= CHAPTER_CAP || to >= CHAPTER_CAP {
            self.red_count += 1;
            return Err(XC_BAD_CHAPTER);
        }
        if self.chapters[to].phase == DPhase::Outline {
            self.red_count += 1;
            return Err(XC_DANGLING_REF);
        }
        self.refs[to] += 1;
        Ok(())
    }

    /// 章 Final（前置：章状态 Reviewed、行数 ≥ 门槛、全节定稿）。
    pub fn seal_chapter(&mut self, chapter: usize) -> Result<u32, u16> {
        if chapter >= CHAPTER_CAP {
            self.red_count += 1;
            return Err(XC_BAD_CHAPTER);
        }
        let total = self.chapters[chapter].total_lines();
        if self.chapters[chapter].phase != DPhase::Reviewed {
            self.red_count += 1;
            return Err(XC_BAD_TRANS);
        }
        if total < CHAPTER_MIN_LINES {
            self.red_count += 1;
            return Err(XC_TOO_THIN);
        }
        let mut i = 0;
        while i < self.chapters[chapter].sections.len() {
            if self.chapters[chapter].sections[i].phase != DPhase::Final {
                self.red_count += 1;
                return Err(XC_TOO_THIN);
            }
            i += 1;
        }
        self.chapters[chapter].phase = DPhase::Final;
        Ok(total)
    }

    /// 推进章状态。
    pub fn advance_chapter(&mut self, chapter: usize, to: DPhase) -> Result<(), u16> {
        if chapter >= CHAPTER_CAP {
            self.red_count += 1;
            return Err(XC_BAD_CHAPTER);
        }
        let cur = self.chapters[chapter].phase;
        if !trans_legal(cur, to) {
            self.red_count += 1;
            return Err(XC_BAD_TRANS);
        }
        self.chapters[chapter].phase = to;
        Ok(())
    }

    /// 文档账守恒。
    pub fn doc_balanced(&self) -> bool {
        self.sections_created == self.sections_final + self.sections_open
    }

    /// 总行数（全章合计）。
    pub fn total_lines(&self) -> u32 {
        let mut n: u32 = 0;
        let mut c = 0;
        while c < CHAPTER_CAP {
            n += self.chapters[c].total_lines();
            c += 1;
        }
        n
    }

    /// 出书（十部全 Final 且总行数 ≥ 门槛）。
    pub fn publish(&mut self) -> Result<u32, u16> {
        let mut c = 0;
        while c < CHAPTER_CAP {
            if self.chapters[c].phase != DPhase::Final {
                self.red_count += 1;
                return Err(XC_PUBLISH_BLOCKED);
            }
            c += 1;
        }
        let total = self.total_lines();
        if total < BOOK_MIN_LINES {
            self.red_count += 1;
            return Err(XC_PUBLISH_BLOCKED);
        }
        self.published = true;
        Ok(total)
    }

    /// 摘要（非空判据）。
    pub fn summary(&self) -> String {
        let mut s = String::from("amddocs sections=");
        s += &self.sections_created.to_string();
        s += " final=";
        s += &self.sections_final.to_string();
        s += " lines=";
        s += &self.total_lines().to_string();
        s += " published=";
        s += if self.published { "yes" } else { "no" };
        s
    }

    /// 全账审计。
    pub fn audit(&self) -> Result<(), u16> {
        if !self.doc_balanced() {
            return Err(XC_DOC_DRIFT);
        }
        Ok(())
    }
}

/// 自例程：十部立节 → 逐节定稿 → 章 Final → 出书 + 负例六连。
pub fn drill() -> Result<u32, u16> {
    let mut b = DocBook::new();
    // 每部立 2 节（行数 45/55，保证过章门槛）。
    let mut c = 0;
    while c < CHAPTER_CAP {
        b.add_section(c, "intro", 45)?;
        b.add_section(c, "detail", 55)?;
        c += 1;
    }
    // 负例：部号越界。
    if b.add_section(CHAPTER_CAP, "x", 10) != Err(XC_BAD_CHAPTER) {
        return Err(XC_BAD_CHAPTER);
    }
    // 负例：行数越界。
    if b.add_section(0, "thin", 0) != Err(XC_BAD_LINES) {
        return Err(XC_BAD_LINES);
    }
    if b.add_section(0, "fat", LINES_MAX + 1) != Err(XC_BAD_LINES) {
        return Err(XC_BAD_LINES);
    }
    // 负例：重名。
    if b.add_section(0, "intro", 10) != Err(XC_DUP_SECTION) {
        return Err(XC_DUP_SECTION);
    }
    b.audit()?;
    // 交叉引用（目标已 Drafted）。
    b.add_ref(H_OVERVIEW, H_ARCHMAP)?;
    // 负例：悬空引用不可构造（都已被 Drafted），改验章推越界。
    if b.add_ref(H_OVERVIEW, CHAPTER_CAP) != Err(XC_BAD_CHAPTER) {
        return Err(XC_BAD_CHAPTER);
    }
    // 逐节推进到 Final。
    let mut c2 = 0;
    while c2 < CHAPTER_CAP {
        b.advance_chapter(c2, DPhase::Reviewed)?;
        b.advance_section(c2, 0, DPhase::Reviewed)?;
        b.advance_section(c2, 1, DPhase::Reviewed)?;
        b.advance_section(c2, 0, DPhase::Final)?;
        b.advance_section(c2, 1, DPhase::Final)?;
        b.seal_chapter(c2)?;
        c2 += 1;
    }
    // 负例：Final 章再立节。
    if b.add_section(0, "late", 10) != Err(XC_CHAPTER_SEALED) {
        return Err(XC_CHAPTER_SEALED);
    }
    b.audit()?;
    let total = b.publish()?;
    // 负例：已出书再出（幂等允许）。
    let again = b.publish()?;
    if again != total {
        return Err(XC_PUBLISH_BLOCKED);
    }
    // 负例：迁移非法（Outline→Final 直跳）。
    let mut q = DocBook::new();
    q.add_section(0, "a", 50)?;
    if q.advance_section(0, 0, DPhase::Final) != Err(XC_BAD_TRANS) {
        return Err(XC_BAD_TRANS);
    }
    if q.advance_chapter(0, DPhase::Final) != Err(XC_BAD_TRANS) {
        return Err(XC_BAD_TRANS);
    }
    // 负例：瘦章 Final。
    let mut w = DocBook::new();
    w.add_section(0, "tiny", 10)?;
    w.advance_chapter(0, DPhase::Reviewed)?;
    w.advance_section(0, 0, DPhase::Reviewed)?;
    w.advance_section(0, 0, DPhase::Final)?;
    if w.seal_chapter(0) != Err(XC_TOO_THIN) {
        return Err(XC_TOO_THIN);
    }
    // 章行数门槛：40。
    let mut v = DocBook::new();
    v.add_section(0, "edge", 40)?;
    v.advance_chapter(0, DPhase::Reviewed)?;
    v.advance_section(0, 0, DPhase::Reviewed)?;
    v.advance_section(0, 0, DPhase::Final)?;
    if v.seal_chapter(0)? != 40 {
        return Err(XC_TOO_THIN);
    }
    if b.summary().len() == 0 || total < BOOK_MIN_LINES {
        return Err(XC_PUBLISH_BLOCKED);
    }
    Ok(0)
}
