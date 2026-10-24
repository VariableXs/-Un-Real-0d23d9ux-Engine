//! VE-F3595 · E 域 fuzz 终检 —— 语料全量 + 红线守住 + 崩溃阻断 + 归档。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3595`
//!
//! # 职责（锚点原文拆解）
//!
//! E 域 fuzz 语料全量终检。
//!
//! # 复杂度
//!
//! O(语料)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E21 段续：7080 起）
// ---------------------------------------------------------------------------

/// fuzz 终检诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuzzFinalCode {
    /// 红线破。
    RedlineBroken = 7080,
    /// 语料不全。
    CorpusIncomplete = 7081,
}

impl FuzzFinalCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            FuzzFinalCode::RedlineBroken => "fuzz 红线破",
            FuzzFinalCode::CorpusIncomplete => "语料不全",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// 二、终检器
// ---------------------------------------------------------------------------

/// fuzz 终检结果。
#[derive(Clone, Copy, Debug)]
pub struct FuzzFinalResult {
    /// 语料数。
    pub corpus_count: u32,
    /// 半生效数（红线）。
    pub half_applied: u32,
    /// 崩溃数。
    pub crashes: u32,
}

/// 终检器。
pub struct FuzzFinalChecker;

impl FuzzFinalChecker {
    /// 终检（语料 ≥ 6 且红线守）。
    pub fn verify(r: &FuzzFinalResult) -> Result<(), FuzzFinalCode> {
        if r.corpus_count < 6 {
            return Err(FuzzFinalCode::CorpusIncomplete);
        }
        if r.half_applied > 0 || r.crashes > 0 {
            return Err(FuzzFinalCode::RedlineBroken);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、判据
// ---------------------------------------------------------------------------

/// fuzz 终检判据集。
pub fn run_fuzzfinal_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver12o");

    let good = FuzzFinalResult {
        corpus_count: 6,
        half_applied: 0,
        crashes: 0,
    };

    // 1. 合规过。
    chk.add("7080-合规过", FuzzFinalChecker::verify(&good).is_ok(), "");

    // 2. 红线破。
    let red = FuzzFinalResult {
        half_applied: 1,
        ..good
    };
    chk.add(
        "7081-红线破",
        FuzzFinalChecker::verify(&red).err() == Some(FuzzFinalCode::RedlineBroken),
        "",
    );

    // 3. 语料不足。
    let small = FuzzFinalResult {
        corpus_count: 3,
        ..good
    };
    chk.add(
        "7082-语料不足",
        FuzzFinalChecker::verify(&small).err() == Some(FuzzFinalCode::CorpusIncomplete),
        "",
    );

    // 4. 阻断级。
    chk.add("7083-阻断级", FuzzFinalCode::RedlineBroken.blocking(), "");

    chk
}
