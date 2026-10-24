//! VE-F3586 · E 域文档生成 —— 自动聚合 + 单一源 + 漂移检出 + CI 守护 + 目录完整。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3586`
//!
//! # 职责（锚点原文拆解）
//!
//! - **自动聚合**（[`DocAggregator`]）：十组文档聚合为域手册。
//! - **单一源**：文档漂移自动检出。
//! - **目录完整性断言**：十组一章不缺。
//!
//! # 复杂度
//!
//! O(文档)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E21 段续：6900 起）
// ---------------------------------------------------------------------------

/// 文档生成诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GenCode {
    /// 漂移。
    Drift = 6900,
    /// 源缺失。
    SourceMissing = 6901,
    /// 目录缺章。
    ChapterMissing = 6902,
}

impl GenCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            GenCode::Drift => "文档漂移",
            GenCode::SourceMissing => "文档源缺失",
            GenCode::ChapterMissing => "目录缺章",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, GenCode::Drift | GenCode::ChapterMissing)
    }
}

// ---------------------------------------------------------------------------
// 二、聚合器
// ---------------------------------------------------------------------------

/// 章节（十组各一章）。
#[derive(Clone, Copy, Debug)]
pub struct Chapter {
    /// 组 id（0..=9）。
    pub group: u8,
    /// 内容指纹。
    pub hash: u64,
}

/// 文档聚合器。
pub struct DocAggregator {
    chapters: Vec<Chapter>,
    /// 基准指纹。
    pub baseline_hash: u64,
}

impl DocAggregator {
    /// 新建。
    pub fn new(baseline_hash: u64) -> Self {
        Self {
            chapters: Vec::new(),
            baseline_hash,
        }
    }

    /// 加一章。
    pub fn add_chapter(&mut self, c: Chapter) {
        self.chapters.push(c);
    }

    /// 目录完整性：十章各一。
    pub fn toc_complete(&self) -> bool {
        for g in 0..10u8 {
            if !self.chapters.iter().any(|c| c.group == g) {
                return false;
            }
        }
        true
    }

    /// 内容指纹（全部章节 hash 累计）。
    pub fn content_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for c in &self.chapters {
            h ^= c.hash;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    /// 漂移检出。
    pub fn verify(&self) -> Result<(), GenCode> {
        if !self.toc_complete() {
            return Err(GenCode::ChapterMissing);
        }
        if self.content_hash() != self.baseline_hash {
            return Err(GenCode::Drift);
        }
        Ok(())
    }

    /// 章数。
    pub fn chapter_count(&self) -> usize {
        self.chapters.len()
    }
}

// ---------------------------------------------------------------------------
// 三、判据
// ---------------------------------------------------------------------------

/// 文档生成判据集。
pub fn run_gen_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver12f");

    // 1. 十章齐。
    let mut agg = DocAggregator::new(0);
    for g in 0..10u8 {
        agg.add_chapter(Chapter {
            group: g,
            hash: g as u64 + 1,
        });
    }
    chk.add("6900-十章齐", agg.toc_complete() && agg.chapter_count() == 10, "");

    // 2. 缺章拦。
    let mut agg2 = DocAggregator::new(0);
    agg2.add_chapter(Chapter {
        group: 0,
        hash: 1,
    });
    chk.add(
        "6901-缺章拦",
        agg2.verify().err() == Some(GenCode::ChapterMissing),
        "",
    );

    // 3. 漂移拦。
    let mut agg3 = DocAggregator::new(999);
    for g in 0..10u8 {
        agg3.add_chapter(Chapter {
            group: g,
            hash: 1,
        });
    }
    chk.add(
        "6902-漂移拦",
        agg3.verify().err() == Some(GenCode::Drift),
        "",
    );

    // 4. 一致过。
    let real_hash = agg.content_hash();
    let mut agg4 = DocAggregator::new(real_hash);
    for g in 0..10u8 {
        agg4.add_chapter(Chapter {
            group: g,
            hash: g as u64 + 1,
        });
    }
    chk.add("6903-一致过", agg4.verify().is_ok(), "");

    chk
}
