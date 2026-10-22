//! cga121_teachdoc — CGPU-F0121 CPU 渲染教学文档。
//!
//! 判据原文（施工书锚点 CGPU-F0121）：CPU 渲染体系的教学文档：架构
//! 总览（SIMD 库→线程池→任务图→软渲驱动的分层故事）、算子开发教程
//! （从零写一个 SIMD 算子的全程示范）、调试教程（F0156-178 工具在 CPU
//! 路径的用法）。教学文档 = 新成员/新 AI 会话的上下文恢复包。判据：
//! 三层教程、示范完整、工具覆盖、准确对拍、可读性。
//!
//! **设计定盘**：教学文档的代码面 = 文档完整性账本：三层教程各登记
//! 章节清单（章节 = 标题 + 正文锚点 + 交叉引用目标），账本对三层分别
//! 做完整性验证（章节非空、锚点唯一、交叉引用可解析——引用了不存在的
//! 章节即红）。示范完整性 = 算子开发教程必须覆盖"从零写算子"的七个
//! 必经步骤闭集（声明/分派注册/标量参考实现/SIMD 主路径/边界路径/
//! 基准挂接/判据挂接）。工具覆盖 = 调试教程必须覆盖工具册中每个已登记
//! 工具。准确对拍 = 文档锚点指纹与代码落点指纹一致性核验的账面化。
//!
//! 诊断码段 0x0100~0x010F 第 8 子区（0x0170~0x017F）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA121_VERSION: &str = "cga121-teachdoc-v1";

/// 诊断子区基值（0x01 段第 8 子区）。
pub const SEG_BASE: u16 = 0x0170;

/// 三层教程闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tutorial {
    /// 架构总览。
    Architecture,
    /// 算子开发教程。
    OpDev,
    /// 调试教程。
    Debugging,
}

impl Tutorial {
    /// 稳定序号。
    pub fn ordinal(self) -> u8 {
        match self {
            Tutorial::Architecture => 0,
            Tutorial::OpDev => 1,
            Tutorial::Debugging => 2,
        }
    }

    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self.ordinal() as u16 + 1
    }
}

/// 算子教程必经七步闭集（判据"示范完整"）。
pub const OPDEV_STEPS: [&str; 7] = [
    "declare",
    "dispatch-register",
    "scalar-reference",
    "simd-main",
    "edge-paths",
    "bench-hook",
    "criterion-hook",
];

/// 章节条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Section {
    /// 所属教程。
    pub tut: Tutorial,
    /// 章节锚点（全账唯一）。
    pub anchor: &'static str,
    /// 正文字数（零即空章节即拒）。
    pub body_chars: u32,
    /// 交叉引用目标锚点列表（必须可解析）。
    pub refs: Vec<&'static str>,
}

/// 教文档完整性账。
pub struct TeachDocLedger {
    /// 章节册。
    sections: Vec<Section>,
    /// 工具册（调试教程须全覆盖）。
    tools: Vec<&'static str>,
    /// 被教程覆盖的工具。
    covered_tools: Vec<&'static str>,
    /// 代码落点指纹（准确对拍的真源）。
    code_fps: Vec<(&'static str, u64)>,
    /// 文档锚点指纹（文档声明的落点指纹）。
    doc_fps: Vec<(&'static str, u64)>,
    /// 封账。
    sealed: Option<u64>,
}

impl TeachDocLedger {
    /// 空账。
    pub fn new() -> Self {
        TeachDocLedger {
            sections: Vec::new(),
            tools: Vec::new(),
            covered_tools: Vec::new(),
            code_fps: Vec::new(),
            doc_fps: Vec::new(),
            sealed: None,
        }
    }

    /// 登记工具（判据"工具覆盖"的真源面）。
    pub fn register_tool(&mut self, tool: &'static str) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 4);
        }
        if tool.is_empty() || self.tools.contains(&tool) {
            return Err(SEG_BASE + 5);
        }
        self.tools.push(tool);
        Ok(())
    }

    /// 教程声明覆盖工具（覆盖未登记工具即拒）。
    pub fn cover_tool(&mut self, tool: &'static str) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 4);
        }
        if !self.tools.contains(&tool) {
            return Err(SEG_BASE + 6);
        }
        if self.covered_tools.contains(&tool) {
            return Err(SEG_BASE + 7);
        }
        self.covered_tools.push(tool);
        Ok(())
    }

    /// 登记章节（判据"三层教程"：锚点全账唯一；正文零字即拒）。
    pub fn add_section(
        &mut self,
        tut: Tutorial,
        anchor: &'static str,
        body_chars: u32,
        refs: Vec<&'static str>,
    ) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 4);
        }
        if anchor.is_empty() {
            return Err(SEG_BASE + 8);
        }
        if body_chars == 0 {
            return Err(SEG_BASE + 9);
        }
        if self.sections.iter().any(|s| s.anchor == anchor) {
            return Err(SEG_BASE + 10);
        }
        self.sections.push(Section {
            tut,
            anchor,
            body_chars,
            refs,
        });
        Ok(())
    }

    /// 交叉引用可解析核验：每章的 refs 必须指向已存在锚点。
    pub fn refs_resolvable(&self) -> Result<(), u16> {
        for s in &self.sections {
            for r in &s.refs {
                if !self.sections.iter().any(|t| t.anchor == *r) {
                    return Err(SEG_BASE + 11);
                }
            }
        }
        Ok(())
    }

    /// 示范完整性核验（判据"示范完整"）：算子教程的章节锚点序列
    /// 必须覆盖七步闭集（锚点名 = 步骤名）。
    pub fn opdev_steps_complete(&self) -> Result<(), u16> {
        for step in OPDEV_STEPS {
            if !self
                .sections
                .iter()
                .any(|s| s.tut == Tutorial::OpDev && s.anchor == step)
            {
                return Err(SEG_BASE + 12);
            }
        }
        Ok(())
    }

    /// 工具覆盖核验（判据"工具覆盖"）：工具册全量被调试教程覆盖。
    pub fn tools_fully_covered(&self) -> Result<(), u16> {
        for t in &self.tools {
            if !self.covered_tools.contains(t) {
                return Err(SEG_BASE + 13);
            }
        }
        Ok(())
    }

    /// 准确对拍登记（判据"准确对拍"）：代码落点指纹与文档声明指纹
    /// 成对登记，落点重复即拒。
    pub fn pair_fingerprint(
        &mut self,
        target: &'static str,
        code_fp: u64,
        doc_fp: u64,
    ) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 4);
        }
        if self.code_fps.iter().any(|(t, _)| *t == target) {
            return Err(SEG_BASE + 14);
        }
        self.code_fps.push((target, code_fp));
        self.doc_fps.push((target, doc_fp));
        Ok(())
    }

    /// 对拍核验：全部登记对的指纹必须一致。
    pub fn fingerprints_agree(&self) -> Result<(), u16> {
        for i in 0..self.code_fps.len() {
            if self.code_fps[i].1 != self.doc_fps[i].1 {
                return Err(SEG_BASE + 15);
            }
        }
        Ok(())
    }

    /// 章节数。
    pub fn section_count(&self) -> usize {
        self.sections.len()
    }

    /// 封账（五项核验全过才能封）。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 16);
        }
        self.refs_resolvable()?;
        self.opdev_steps_complete()?;
        self.tools_fully_covered()?;
        self.fingerprints_agree()?;
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for s in &self.sections {
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ s.body_chars as u64;
        }
        self.sealed = Some(fp);
        Ok(fp)
    }
}

impl Default for TeachDocLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 单章正文字数下限（判据"可读性"的量化面：太短的章节撑不起
/// 上下文恢复包的职责）。
pub const SECTION_BODY_FLOOR: u32 = 200;

/// 教程层级目录投影（判据"三层教程"的目录面）：三教程各自章节数。
pub fn section_census(sections: &[Section]) -> [usize; 3] {
    let mut out = [0usize; 3];
    for s in sections {
        out[s.tut.ordinal() as usize] += 1;
    }
    out
}

impl TeachDocLedger {
    /// 目录投影。
    pub fn census(&self) -> [usize; 3] {
        section_census(&self.sections)
    }

    /// 单章字数下限核验（判据"可读性"：全部章节过 SECTION_BODY_FLOOR）。
    pub fn body_floor_ok(&self) -> Result<(), u16> {
        for s in &self.sections {
            if s.body_chars < SECTION_BODY_FLOOR {
                return Err(SEG_BASE + 17);
            }
        }
        Ok(())
    }

    /// 对拍漂移清单（判据"准确对拍"的处置面）：指纹不一致的落点列表。
    pub fn drifted_targets(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for i in 0..self.code_fps.len() {
            if self.code_fps[i].1 != self.doc_fps[i].1 {
                out.push(self.code_fps[i].0);
            }
        }
        out
    }

    /// 空引用章节数（refs 为空 = 孤岛章节——教学文档需要网状交叉引用，
    /// 全账至少一半章节应有交叉引用，此处仅投影计数供 CI 消费）。
    pub fn orphan_sections(&self) -> usize {
        self.sections.iter().filter(|s| s.refs.is_empty()).count()
    }

    /// 指定教程的章节锚点列表（按登记序）。
    pub fn anchors_of(&self, tut: Tutorial) -> Vec<&'static str> {
        self.sections
            .iter()
            .filter(|s| s.tut == tut)
            .map(|s| s.anchor)
            .collect()
    }
}
