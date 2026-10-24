#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xDB80）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xDB80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CDB8Code(pub u16);

impl CDB8Code {
    pub const DRIFT: Self = Self(SEG_BASE);           // 漂移
    pub const DUTY_MISSING: Self = Self(SEG_BASE + 1); // 义务章缺
    pub const BOOTSTRAP_FAIL: Self = Self(SEG_BASE + 2); // 自举不过
    pub const MAZE_OVER: Self = Self(SEG_BASE + 3);    // 迷宫超 2 跳
    pub const DOC_MANUAL: Self = Self(SEG_BASE + 4);   // 手写文档

    pub fn code(self) -> u16 { self.0 }
    pub fn name(self) -> &'static str {
        match self {
            Self::DRIFT => "漂移",
            Self::DUTY_MISSING => "义务章缺",
            Self::BOOTSTRAP_FAIL => "自举不过",
            Self::MAZE_OVER => "迷宫超2跳",
            Self::DOC_MANUAL => "手写文档",
            _ => "未知",
        }
    }
    pub const ALL: [Self; 5] = [
        Self::DRIFT, Self::DUTY_MISSING, Self::BOOTSTRAP_FAIL,
        Self::MAZE_OVER, Self::DOC_MANUAL,
    ];
}

// ── 三册结构 ────────────────────────────────────────────────────────

/// 文档册。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocVolume {
    /// 创作者册（字幕/描述制作规范）。
    Creator,
    /// 开发者册（API 接入指南）。
    Developer,
    /// 用户册（字幕样式个性化指南）。
    User,
}

impl DocVolume {
    pub const ALL: [Self; 3] = [Self::Creator, Self::Developer, Self::User];
    pub fn name(self) -> &'static str {
        match self {
            Self::Creator => "创作者册",
            Self::Developer => "开发者册",
            Self::User => "用户册",
        }
    }
}

/// 文档章。
#[derive(Debug, Clone)]
pub struct DocChapter {
    pub volume: DocVolume,
    pub title: &'static str,
    pub is_duty: bool,
    pub hops_from_root: u8,
}

impl DocChapter {
    /// 2 跳红线：从根到章 ≤2 跳。
    pub fn nav_ok(&self) -> bool { self.hops_from_root <= 2 }
}

/// 文档册实例。
#[derive(Debug, Clone)]
pub struct MediaDocBook {
    pub chapters: Vec<DocChapter>,
    pub generated: bool,
}

impl MediaDocBook {
    pub fn new() -> Self { Self { chapters: Vec::new(), generated: true } }

    /// 义务前置：创作者册必须有义务章。
    pub fn duty_ok(&self) -> bool {
        self.chapters.iter().any(|c| c.volume == DocVolume::Creator && c.is_duty)
    }

    /// 三册齐全。
    pub fn volumes_complete(&self) -> bool {
        DocVolume::ALL.iter().all(|v| self.chapters.iter().any(|c| c.volume == *v))
    }

    /// 导航 2 跳。
    pub fn nav_ok(&self) -> bool {
        self.chapters.iter().all(|c| c.nav_ok())
    }

    /// 生成单源。
    pub fn generated_ok(&self) -> bool { self.generated }
}

// ── 双重自举 ────────────────────────────────────────────────────────

/// 自举审查。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootstrapReview {
    pub self_check_passed: bool,
    pub peer_check_passed: bool,
}

impl BootstrapReview {
    /// 双重自举。
    pub fn ok(&self) -> bool { self.self_check_passed && self.peer_check_passed }
}

// ── 同源对账 ────────────────────────────────────────────────────────

/// 同源清单项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceOfTruthItem {
    pub key: &'static str,
    pub doc_version: &'static str,
    pub code_version: &'static str,
}

impl SourceOfTruthItem {
    /// 对账一致。
    pub fn consistent(&self) -> bool { self.doc_version == self.code_version }
}

/// 对账器。
#[derive(Debug, Clone)]
pub struct Reconciler {
    pub items: Vec<SourceOfTruthItem>,
}

impl Reconciler {
    /// 全部一致。
    pub fn all_consistent(&self) -> bool {
        self.items.iter().all(|i| i.consistent())
    }
}

// ── 判据集 ──────────────────────────────────────────────────────────

pub fn run_f3874_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("vet4l_mediadoc");
    fn chk(set: &mut crate::checks::CheckSet, label: &'static str, cond: bool, note: &'static str) {
        set.add(label, cond, note);
    }

    // 1 三册枚举
    {
        chk(&mut set, "C3874-01", DocVolume::ALL.len() == 3, "err");
    }
    // 2 义务前置
    {
        let mut b = MediaDocBook::new();
        b.chapters.push(DocChapter { volume: DocVolume::Creator, title: "duty", is_duty: true, hops_from_root: 1 });
        chk(&mut set, "C3874-02", b.duty_ok(), "err");
    }
    // 3 义务缺失
    {
        let mut b = MediaDocBook::new();
        b.chapters.push(DocChapter { volume: DocVolume::Creator, title: "intro", is_duty: false, hops_from_root: 1 });
        chk(&mut set, "C3874-03", !b.duty_ok(), "err");
    }
    // 4 三册齐全
    {
        let mut b = MediaDocBook::new();
        for v in DocVolume::ALL {
            b.chapters.push(DocChapter { volume: v, title: "t", is_duty: true, hops_from_root: 1 });
        }
        chk(&mut set, "C3874-04", b.volumes_complete(), "err");
    }
    // 5 2 跳导航
    {
        let c = DocChapter { volume: DocVolume::User, title: "t", is_duty: false, hops_from_root: 2 };
        chk(&mut set, "C3874-05", c.nav_ok(), "err");
    }
    // 6 迷宫超 2 跳
    {
        let c = DocChapter { volume: DocVolume::User, title: "t", is_duty: false, hops_from_root: 3 };
        chk(&mut set, "C3874-06", !c.nav_ok(), "err");
    }
    // 7 生成单源
    {
        let b = MediaDocBook::new();
        chk(&mut set, "C3874-07", b.generated_ok(), "err");
    }
    // 8 双重自举
    {
        let r = BootstrapReview { self_check_passed: true, peer_check_passed: true };
        chk(&mut set, "C3874-08", r.ok(), "err");
    }
    // 9 自举不过
    {
        let r = BootstrapReview { self_check_passed: true, peer_check_passed: false };
        chk(&mut set, "C3874-09", !r.ok(), "err");
    }
    // 10 同源对账一致
    {
        let r = Reconciler { items: vec![SourceOfTruthItem { key: "api", doc_version: "v1", code_version: "v1" }] };
        chk(&mut set, "C3874-10", r.all_consistent(), "err");
    }
    // 11 对账漂移
    {
        let r = Reconciler { items: vec![SourceOfTruthItem { key: "api", doc_version: "v1", code_version: "v2" }] };
        chk(&mut set, "C3874-11", !r.all_consistent(), "err");
    }
    // 12 三册名
    {
        chk(&mut set, "C3874-12", DocVolume::Creator.name() == "创作者册", "err");
    }
    // 13 空册无义务
    {
        let b = MediaDocBook::new();
        chk(&mut set, "C3874-13", !b.duty_ok(), "err");
    }
    // 14 全册导航 OK
    {
        let mut b = MediaDocBook::new();
        b.chapters.push(DocChapter { volume: DocVolume::Creator, title: "a", is_duty: true, hops_from_root: 1 });
        b.chapters.push(DocChapter { volume: DocVolume::Developer, title: "b", is_duty: false, hops_from_root: 2 });
        chk(&mut set, "C3874-14", b.nav_ok(), "err");
    }
    // 15 导航失败
    {
        let mut b = MediaDocBook::new();
        b.chapters.push(DocChapter { volume: DocVolume::User, title: "deep", is_duty: false, hops_from_root: 5 });
        chk(&mut set, "C3874-15", !b.nav_ok(), "err");
    }
    // 16 册缺一
    {
        let mut b = MediaDocBook::new();
        b.chapters.push(DocChapter { volume: DocVolume::Creator, title: "a", is_duty: true, hops_from_root: 1 });
        chk(&mut set, "C3874-16", !b.volumes_complete(), "err");
    }
    // 17 空对账全一致
    {
        let r = Reconciler { items: Vec::new() };
        chk(&mut set, "C3874-17", r.all_consistent(), "err");
    }
    // 18 半自举
    {
        let r = BootstrapReview { self_check_passed: false, peer_check_passed: false };
        chk(&mut set, "C3874-18", !r.ok(), "err");
    }

    set
}
