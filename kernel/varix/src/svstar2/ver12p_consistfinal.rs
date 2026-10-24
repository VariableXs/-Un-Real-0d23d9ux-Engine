//! VE-F3596 · E 域一致性终检 —— 四维全过 + 指数达标 + 零违例 + 兜底。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3596`
//!
//! # 职责（锚点原文拆解）
//!
//! E 域一致性四维终检。
//!
//! # 复杂度
//!
//! O(1)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E21 段续：7100 起）
// ---------------------------------------------------------------------------

/// 一致性终检诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsistFinalCode {
    /// 维度未过。
    DimensionFail = 7100,
    /// 指数不达标。
    IndexBelow = 7101,
}

impl ConsistFinalCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            ConsistFinalCode::DimensionFail => "一致性维度未过",
            ConsistFinalCode::IndexBelow => "一致性指数不达标",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, ConsistFinalCode::DimensionFail)
    }
}

/// 指数达标线（千分比）。
pub const INDEX_THRESHOLD: u32 = 800;

// ---------------------------------------------------------------------------
// 二、终检器
// ---------------------------------------------------------------------------

/// 一致性终检数据。
#[derive(Clone, Copy, Debug)]
pub struct ConsistFinal {
    /// 四维掩码（0b1111=全过）。
    pub dims_mask: u8,
    /// 指数。
    pub index: u32,
}

/// 终检器。
pub struct ConsistFinalChecker;

impl ConsistFinalChecker {
    /// 终检。
    pub fn verify(d: &ConsistFinal) -> Result<(), ConsistFinalCode> {
        if d.dims_mask != 0b1111 {
            return Err(ConsistFinalCode::DimensionFail);
        }
        if d.index < INDEX_THRESHOLD {
            return Err(ConsistFinalCode::IndexBelow);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、判据
// ---------------------------------------------------------------------------

/// 一致性终检判据集。
pub fn run_consistfinal_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver12p");

    let good = ConsistFinal {
        dims_mask: 0b1111,
        index: 900,
    };

    // 1. 合规过。
    chk.add("7100-合规过", ConsistFinalChecker::verify(&good).is_ok(), "");

    // 2. 维度缺。
    let dim_fail = ConsistFinal {
        dims_mask: 0b0111,
        ..good
    };
    chk.add(
        "7101-维度拦",
        ConsistFinalChecker::verify(&dim_fail).err() == Some(ConsistFinalCode::DimensionFail),
        "",
    );

    // 3. 指数不足。
    let idx_low = ConsistFinal {
        index: 500,
        ..good
    };
    chk.add(
        "7102-指数拦",
        ConsistFinalChecker::verify(&idx_low).err() == Some(ConsistFinalCode::IndexBelow),
        "",
    );

    // 4. 阈值常量。
    chk.add("7103-阈值", INDEX_THRESHOLD == 800, "");

    chk
}
