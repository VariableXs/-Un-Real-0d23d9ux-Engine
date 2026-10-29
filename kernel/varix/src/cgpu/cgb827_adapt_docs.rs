//! cgb827_adapt_docs — CGPU-F0827 Intel 适配文档。
//!
//! 判据原文（验收行摘录）：Intel 适配文档：章节树登记（章-节两级定容结
//! 构）；锚点核验（每节须挂诊断段锚点，无锚即缺）；覆盖核验（能力面全
//! 须有对应文档节——能力集与节集对照）； completeness 守恒账（节数 =
//! 有锚节数 + 缺锚节数，章节数与登记数一致）。判据：两级树、锚点核验、
//! 覆盖核验、守恒账。
//!
//! 诊断段 0x71E0（本单独占）。
//!
//! 设计纪律：no_std+alloc，禁 unwrap/expect/panic/浮点/std；失败路径
//! Result<_, u16> 闭集错误码；定容章-节树；守恒每步可验；drill() 自例
//! 程返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MOD_VERSION: &str = "W3-cgb827-v1";

/// 诊断段基（本单独占 0x71E0）。
pub const SEG_BASE: u16 = 0x71E0;

/// 章数超容。
pub const XC_CHAPTER_FULL: u16 = SEG_BASE + 1;
/// 节数超容（单章节数上限）。
pub const XC_SECTION_FULL: u16 = SEG_BASE + 2;
/// 章不存在（登记节时章号越界）。
pub const XC_CHAPTER_MISS: u16 = SEG_BASE + 3;
/// 节重名（同章内节名重复）。
pub const XC_SECTION_DUP: u16 = SEG_BASE + 4;
/// 锚点缺失（核验时发现无锚节——须补锚）。
pub const XC_ANCHOR_MISSING: u16 = SEG_BASE + 5;
/// 覆盖缺口（能力面无对应文档节）。
pub const XC_COVERAGE_GAP: u16 = SEG_BASE + 6;
/// 文档账漂移（节数守恒不成立）。
pub const XC_DOC_DRIFT: u16 = SEG_BASE + 7;

/// 章数上限。
pub const CHAPTER_CAP: usize = 8;
/// 单章节数上限。
pub const SECTION_CAP: usize = 8;
/// 节名定容字节。
pub const NAME_CAP: usize = 24;

/// 能力面闭集（判据"覆盖核验"的对照域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Facet {
    /// 共享内存。
    Shm,
    /// 媒体联动。
    Media,
    /// 功耗。
    Power,
    /// 遥测。
    Telemetry,
    /// fuzz。
    Fuzz,
    /// API 冻结。
    Freeze,
}

impl Facet {
    /// 面 ID（文档节以 facet_id 关联能力面）。
    pub fn id(self) -> u16 {
        match self {
            Facet::Shm => 1,
            Facet::Media => 2,
            Facet::Power => 3,
            Facet::Telemetry => 4,
            Facet::Fuzz => 5,
            Facet::Freeze => 6,
        }
    }
    /// 面名。
    pub fn name(self) -> &'static str {
        match self {
            Facet::Shm => "shm",
            Facet::Media => "media",
            Facet::Power => "power",
            Facet::Telemetry => "telemetry",
            Facet::Fuzz => "fuzz",
            Facet::Freeze => "freeze",
        }
    }
}

/// 文档节。
#[derive(Clone, Debug)]
pub struct Section {
    /// 节名。
    pub title: String,
    /// 诊断段锚点（0 表示未挂锚）。
    pub seg_anchor: u16,
    /// 关联能力面（0 表示未关联）。
    pub facet: u16,
    /// 字数规模（行数账——文档密度核验用）。
    pub lines: u32,
}

/// 文档章。
#[derive(Clone, Debug)]
pub struct Chapter {
    /// 章名。
    pub title: String,
    /// 章内节。
    pub sections: Vec<Section>,
}

/// 适配文档树。
pub struct DocTree {
    /// 树名。
    pub name: String,
    /// 章集（定容）。
    chapters: Vec<Chapter>,
    /// 缺锚节数（守恒项）。
    pub anchor_missing: u32,
    /// 累计登记节。
    pub registered: u32,
}

/// 守恒恒等式：registered == Σ章节数 == 有锚节数 + anchor_missing。
/// 登记与补锚后 verify() 复核三式一致。
impl DocTree {
    /// 构造。
    pub fn new(name: &str) -> Self {
        DocTree { name: name.to_string(), chapters: Vec::new(), anchor_missing: 0, registered: 0 }
    }

    /// 开章。
    pub fn add_chapter(&mut self, title: &str) -> Result<usize, u16> {
        if self.chapters.len() >= CHAPTER_CAP {
            return Err(XC_CHAPTER_FULL);
        }
        if title.len() > NAME_CAP {
            return Err(XC_CHAPTER_MISS);
        }
        self.chapters.push(Chapter { title: title.to_string(), sections: Vec::new() });
        Ok(self.chapters.len() - 1)
    }

    /// 章数。
    pub fn chapter_count(&self) -> usize {
        self.chapters.len()
    }

    /// 登记节（判据"两级树"）。
    pub fn add_section(&mut self, ch: usize, title: &str, seg: u16, facet: u16, lines: u32) -> Result<(), u16> {
        if ch >= self.chapters.len() {
            return Err(XC_CHAPTER_MISS);
        }
        if title.len() > NAME_CAP {
            return Err(XC_SECTION_FULL);
        }
        let c = &mut self.chapters[ch];
        if c.sections.len() >= SECTION_CAP {
            return Err(XC_SECTION_FULL);
        }
        for s in c.sections.iter() {
            if s.title == title {
                return Err(XC_SECTION_DUP);
            }
        }
        let anchored = seg != 0;
        if !anchored {
            self.anchor_missing += 1;
        }
        c.sections.push(Section { title: title.to_string(), seg_anchor: seg, facet, lines });
        self.registered += 1;
        self.verify()?;
        Ok(())
    }

    /// 补锚（判据"锚点核验"整改动作）。
    pub fn anchor(&mut self, ch: usize, sec: usize, seg: u16) -> Result<(), u16> {
        if ch >= self.chapters.len() {
            return Err(XC_CHAPTER_MISS);
        }
        if seg == 0 {
            return Err(XC_ANCHOR_MISSING);
        }
        let c = &mut self.chapters[ch];
        if sec >= c.sections.len() {
            return Err(XC_CHAPTER_MISS);
        }
        if c.sections[sec].seg_anchor == 0 {
            if self.anchor_missing == 0 {
                return Err(XC_DOC_DRIFT);
            }
            self.anchor_missing -= 1;
        }
        c.sections[sec].seg_anchor = seg;
        self.verify()?;
        Ok(())
    }

    /// 有锚节数。
    pub fn anchored_count(&self) -> u32 {
        let mut n = 0u32;
        for c in self.chapters.iter() {
            for s in c.sections.iter() {
                if s.seg_anchor != 0 {
                    n += 1;
                }
            }
        }
        n
    }

    /// 总节数。
    pub fn section_count(&self) -> u32 {
        let mut n = 0u32;
        for c in self.chapters.iter() {
            n += c.sections.len() as u32;
        }
        n
    }

    /// 覆盖核验（判据"能力面全须有对应文档节"）。
    pub fn coverage(&self) -> Result<(), u16> {
        let facets = [Facet::Shm, Facet::Media, Facet::Power, Facet::Telemetry, Facet::Fuzz, Facet::Freeze];
        for f in facets.iter() {
            let mut hit = false;
            'outer: for c in self.chapters.iter() {
                for s in c.sections.iter() {
                    if s.facet == f.id() {
                        hit = true;
                        break 'outer;
                    }
                }
            }
            if !hit {
                return Err(XC_COVERAGE_GAP);
            }
        }
        Ok(())
    }

    /// 文档密度核验：总行数须达门槛（判据"行数账"）。
    pub fn density(&self, min_lines: u32) -> Result<u32, u16> {
        let mut total = 0u32;
        for c in self.chapters.iter() {
            for s in c.sections.iter() {
                total += s.lines;
            }
        }
        if total < min_lines {
            return Err(XC_COVERAGE_GAP);
        }
        Ok(total)
    }

    /// 守恒校验。
    pub fn verify(&self) -> Result<(), u16> {
        let secs = self.section_count();
        if secs != self.registered {
            return Err(XC_DOC_DRIFT);
        }
        if self.anchored_count() + self.anchor_missing != secs {
            return Err(XC_DOC_DRIFT);
        }
        Ok(())
    }

    /// 审计摘要。
    pub fn audit(&self) -> String {
        let mut s = String::new();
        s.push_str("cgb827[");
        s.push_str(&self.name);
        s.push_str("] chapters=");
        s.push_str(&self.chapter_count().to_string());
        s.push_str(" sections=");
        s.push_str(&self.section_count().to_string());
        s.push_str(" no_anchor=");
        s.push_str(&self.anchor_missing.to_string());
        s
    }
}

/// 自例程：构造正/负样本走核心路径；全过返回 0，失败返回错误码。
pub fn drill() -> Result<u32, u16> {
    let mut d = DocTree::new("w3-drill-827");

    // 负样本 1：空树登记节——章不存在。
    match d.add_section(0, "x", 0x7100, 1, 40) {
        Err(code) if code == XC_CHAPTER_MISS => {}
        Ok(_) => return Err(XC_CHAPTER_MISS),
        Err(other) => return Err(other),
    }

    // 开两章。
    let c0 = d.add_chapter("内存与媒体")?;
    let c1 = d.add_chapter("观测与治理")?;
    if d.chapter_count() != 2 {
        return Err(XC_CHAPTER_FULL);
    }

    // 负样本 2：同章重名节——应拒。
    d.add_section(c0, "共享内存", 0x7100, Facet::Shm.id(), 60)?;
    match d.add_section(c0, "共享内存", 0x7100, Facet::Shm.id(), 60) {
        Err(code) if code == XC_SECTION_DUP => {}
        Ok(_) => return Err(XC_SECTION_DUP),
        Err(other) => return Err(other),
    }

    // 登记无锚节（补锚路径样本）。
    d.add_section(c0, "媒体联动", 0, Facet::Media.id(), 50)?;
    d.add_section(c1, "功耗画像", 0x7140, Facet::Power.id(), 45)?;
    d.add_section(c1, "遥测", 0x7180, Facet::Telemetry.id(), 40)?;
    d.add_section(c1, "fuzz", 0x71A0, Facet::Fuzz.id(), 40)?;
    d.add_section(c1, "API 冻结", 0x71C0, Facet::Freeze.id(), 35)?;

    // 核验：应有 1 个缺锚节。
    if d.anchor_missing != 1 {
        return Err(XC_ANCHOR_MISSING);
    }
    d.verify()?;

    // 负样本 3：覆盖核验——此时全有覆盖，先验证通过。
    d.coverage()?;
    // 制造缺口：新建一节并删除其 facet 关联不可行（闭集），改用未覆盖面验证路径：
    // 已覆盖 6 面中的 1~5（Shm/Media/Power/Telemetry/Fuzz/Freeze 全在），故通过。
    // 为走缺口路径，构造第二棵树仅覆盖部分面。
    let mut d2 = DocTree::new("w3-drill-827-partial");
    let cc = d2.add_chapter("薄章")?;
    d2.add_section(cc, "仅内存", 0x7100, Facet::Shm.id(), 30)?;
    match d2.coverage() {
        Err(code) if code == XC_COVERAGE_GAP => {}
        Ok(_) => return Err(XC_COVERAGE_GAP),
        Err(other) => return Err(other),
    }

    // 补锚整改。
    d.anchor(c0, 1, 0x7120)?;
    if d.anchor_missing != 0 {
        return Err(XC_ANCHOR_MISSING);
    }
    // 负样本 4：补锚重复推进守恒——对已有锚节再补同段应幂等不改缺锚账。
    d.anchor(c0, 1, 0x7120)?;
    if d.anchor_missing != 0 {
        return Err(XC_DOC_DRIFT);
    }
    // 负样本 5：0 段锚——应拒。
    match d.anchor(c0, 0, 0) {
        Err(code) if code == XC_ANCHOR_MISSING => {}
        Ok(_) => return Err(XC_ANCHOR_MISSING),
        Err(other) => return Err(other),
    }

    // 密度核验：总行数 60+50+45+40+40+35 = 270；门槛 200 过、门槛 300 缺。
    let total = d.density(200)?;
    if total != 270 {
        return Err(XC_DOC_DRIFT);
    }
    match d.density(300) {
        Err(code) if code == XC_COVERAGE_GAP => {}
        Ok(_) => return Err(XC_COVERAGE_GAP),
        Err(other) => return Err(other),
    }
    d.verify()?;
    if d.section_count() != 6 {
        return Err(XC_DOC_DRIFT);
    }
    Ok(0)
}
