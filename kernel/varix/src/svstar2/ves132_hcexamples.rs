//! VE-F3832 · 高对比示例
//!
//! 高对比主题示例库。六域全覆盖示例。示例可运行断言。示例与文档联动。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xB4xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xB400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CB4Code(pub u16);

impl CB4Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const EXAMPLE_MISSING: Self = Self(SEG_BASE + 1);
    pub const NOT_RUNNABLE: Self = Self(SEG_BASE + 2);
    pub const DOC_UNLINKED: Self = Self(SEG_BASE + 3);
    pub const QUALITY_LOW: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::EXAMPLE_MISSING => "示例缺域，需补",
            Self::NOT_RUNNABLE => "示例不可运行",
            Self::DOC_UNLINKED => "示例与文档未联动",
            Self::QUALITY_LOW => "示例质量不达标",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 示例条目
#[derive(Debug, Clone)]
pub struct Example {
    pub id: u32,
    pub domain: u8,
    pub runnable: bool,
    pub doc_linked: bool,
    pub quality_score: u16,
}

/// 示例库
#[derive(Debug, Clone)]
pub struct ExampleLib {
    pub examples: Vec<Example>,
    pub min_quality: u16,
}

impl ExampleLib {
    pub fn new(min_quality: u16) -> Self {
        Self { examples: Vec::new(), min_quality }
    }

    pub fn add(&mut self, ex: Example) {
        self.examples.push(ex);
    }

    /// 六域覆盖断言
    pub fn assert_six_domains(&self) -> Result<(), CB4Code> {
        for d in 0..6u8 {
            if !self.examples.iter().any(|e| e.domain == d) {
                return Err(CB4Code::EXAMPLE_MISSING);
            }
        }
        Ok(())
    }

    /// 可运行断言
    pub fn assert_runnable(&self) -> Result<(), CB4Code> {
        if self.examples.iter().all(|e| e.runnable) {
            Ok(())
        } else {
            Err(CB4Code::NOT_RUNNABLE)
        }
    }

    /// 文档联动断言
    pub fn assert_doc_linked(&self) -> Result<(), CB4Code> {
        if self.examples.iter().all(|e| e.doc_linked) {
            Ok(())
        } else {
            Err(CB4Code::DOC_UNLINKED)
        }
    }

    /// 质量断言
    pub fn assert_quality(&self) -> Result<(), CB4Code> {
        if self.examples.iter().all(|e| e.quality_score >= self.min_quality) {
            Ok(())
        } else {
            Err(CB4Code::QUALITY_LOW)
        }
    }

    pub fn count(&self) -> usize {
        self.examples.len()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves132_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves132_hcexamples");

    fn full_lib() -> ExampleLib {
        let mut lib = ExampleLib::new(70);
        for d in 0..6u8 {
            lib.add(Example { id: d as u32, domain: d, runnable: true, doc_linked: true, quality_score: 90 });
        }
        lib
    }

    // 01 六域覆盖
    {
        let lib = full_lib();
        chk(&mut set, "C3832-01", lib.assert_six_domains().is_ok(), "err");
    }
    // 02 缺域
    {
        let mut lib = ExampleLib::new(70);
        lib.add(Example { id: 1, domain: 0, runnable: true, doc_linked: true, quality_score: 90 });
        chk(&mut set, "C3832-02", lib.assert_six_domains() == Err(CB4Code::EXAMPLE_MISSING), "err");
    }
    // 03 不可运行
    {
        let mut lib = full_lib();
        lib.examples[0].runnable = false;
        chk(&mut set, "C3832-03", lib.assert_runnable() == Err(CB4Code::NOT_RUNNABLE), "err");
    }
    // 04 可运行
    {
        let lib = full_lib();
        chk(&mut set, "C3832-04", lib.assert_runnable().is_ok(), "err");
    }
    // 05 文档未联动
    {
        let mut lib = full_lib();
        lib.examples[1].doc_linked = false;
        chk(&mut set, "C3832-05", lib.assert_doc_linked() == Err(CB4Code::DOC_UNLINKED), "err");
    }
    // 06 文档联动
    {
        let lib = full_lib();
        chk(&mut set, "C3832-06", lib.assert_doc_linked().is_ok(), "err");
    }
    // 07 质量低
    {
        let mut lib = full_lib();
        lib.examples[2].quality_score = 50;
        chk(&mut set, "C3832-07", lib.assert_quality() == Err(CB4Code::QUALITY_LOW), "err");
    }
    // 08 质量达标
    {
        let lib = full_lib();
        chk(&mut set, "C3832-08", lib.assert_quality().is_ok(), "err");
    }
    // 09 计数
    {
        let lib = full_lib();
        chk(&mut set, "C3832-09", lib.count() == 6, "err");
    }
    // 10 解释非空
    {
        chk(&mut set, "C3832-10", !CB4Code::EXAMPLE_MISSING.explain().is_empty(), "err");
    }

    set
}
