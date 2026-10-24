//! VE-F3833 · 高对比文档
//!
//! 三册结构：开发者册/用户册/标准册。义务前置。双重自举。2 跳复用。家族管道。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xB5xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xB500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CB5Code(pub u16);

impl CB5Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const DRIFT: Self = Self(SEG_BASE + 1);
    pub const OBLIGATION_MISSING: Self = Self(SEG_BASE + 2);
    pub const BOOTSTRAP_FAIL: Self = Self(SEG_BASE + 3);
    pub const MAZE: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::DRIFT => "文档漂移，需对账",
            Self::OBLIGATION_MISSING => "义务章缺，需审查（红线）",
            Self::BOOTSTRAP_FAIL => "自举不过，需修（复用红线实测）",
            Self::MAZE => "文档迷宫：超过 2 跳",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 三册
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handbook {
    Developer,
    User,
    Standard,
}

impl Handbook {
    pub const ALL: [Handbook; 3] = [Self::Developer, Self::User, Self::Standard];
}

/// 文档章节
#[derive(Debug, Clone)]
pub struct DocSection {
    pub handbook: Handbook,
    pub obligation: bool,
    pub hops: u8,
    pub published: bool,
}

// ── 主逻辑 ──────────────────────────────────────────────────────────
pub struct HcDocs {
    sections: Vec<DocSection>,
    bootstrap_ok: bool,
}

impl HcDocs {
    pub fn new() -> Self {
        Self { sections: Vec::new(), bootstrap_ok: true }
    }

    pub fn add_section(&mut self, s: DocSection) {
        self.sections.push(s);
    }

    /// 三册复刻断言
    pub fn assert_three_books(&self) -> Result<(), CB5Code> {
        for h in Handbook::ALL {
            if !self.sections.iter().any(|s| s.handbook == h) {
                return Err(CB5Code::OBLIGATION_MISSING);
            }
        }
        Ok(())
    }

    /// 义务前置断言
    pub fn assert_obligations(&self) -> Result<(), CB5Code> {
        if self.sections.iter().any(|s| s.obligation && !s.published) {
            Err(CB5Code::OBLIGATION_MISSING)
        } else {
            Ok(())
        }
    }

    /// 2 跳断言
    pub fn assert_two_hop(&self) -> Result<(), CB5Code> {
        if self.sections.iter().any(|s| s.hops > 2) {
            Err(CB5Code::MAZE)
        } else {
            Ok(())
        }
    }

    /// 自举断言
    pub fn set_bootstrap(&mut self, ok: bool) {
        self.bootstrap_ok = ok;
    }

    pub fn assert_bootstrap(&self) -> Result<(), CB5Code> {
        if self.bootstrap_ok {
            Ok(())
        } else {
            Err(CB5Code::BOOTSTRAP_FAIL)
        }
    }

    /// 对账
    pub fn assert_drift(&self, expected: usize) -> Result<(), CB5Code> {
        let actual = self.sections.iter().filter(|s| s.published).count();
        if actual == expected {
            Ok(())
        } else {
            Err(CB5Code::DRIFT)
        }
    }
}

impl Default for HcDocs {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves133_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves133_hcdocs");

    fn full_docs() -> HcDocs {
        let mut d = HcDocs::new();
        for h in Handbook::ALL {
            d.add_section(DocSection { handbook: h, obligation: true, hops: 1, published: true });
        }
        d
    }

    // 01 三册齐
    {
        let d = full_docs();
        chk(&mut set, "C3833-01", d.assert_three_books().is_ok(), "err");
    }
    // 02 缺册
    {
        let mut d = HcDocs::new();
        d.add_section(DocSection { handbook: Handbook::Developer, obligation: true, hops: 1, published: true });
        chk(&mut set, "C3833-02", d.assert_three_books() == Err(CB5Code::OBLIGATION_MISSING), "err");
    }
    // 03 义务缺
    {
        let mut d = full_docs();
        d.sections[0].published = false;
        chk(&mut set, "C3833-03", d.assert_obligations() == Err(CB5Code::OBLIGATION_MISSING), "err");
    }
    // 04 2 跳
    {
        let mut d = full_docs();
        d.sections[1].hops = 3;
        chk(&mut set, "C3833-04", d.assert_two_hop() == Err(CB5Code::MAZE), "err");
    }
    // 05 自举失败
    {
        let mut d = full_docs();
        d.set_bootstrap(false);
        chk(&mut set, "C3833-05", d.assert_bootstrap() == Err(CB5Code::BOOTSTRAP_FAIL), "err");
    }
    // 06 自举正常
    {
        let d = full_docs();
        chk(&mut set, "C3833-06", d.assert_bootstrap().is_ok(), "err");
    }
    // 07 对账漂移
    {
        let d = full_docs();
        chk(&mut set, "C3833-07", d.assert_drift(5) == Err(CB5Code::DRIFT), "err");
    }
    // 08 对账一致
    {
        let d = full_docs();
        chk(&mut set, "C3833-08", d.assert_drift(3).is_ok(), "err");
    }
    // 09 义务全发布
    {
        let d = full_docs();
        chk(&mut set, "C3833-09", d.assert_obligations().is_ok(), "err");
    }
    // 10 解释非空
    {
        chk(&mut set, "C3833-10", !CB5Code::DRIFT.explain().is_empty(), "err");
    }

    set
}
