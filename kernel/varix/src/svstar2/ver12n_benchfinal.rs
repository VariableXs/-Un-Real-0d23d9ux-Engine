//! VE-F3594 · E 域基准终检 —— 口径一致 + 实测对拍 + 环境指纹 + 开放导出。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3594`
//!
//! # 职责（锚点原文拆解）
//!
//! E 域五段基准终检：口径/对拍/指纹/导出四线。
//!
//! # 复杂度
//!
//! O(基准)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E21 段续：7060 起）
// ---------------------------------------------------------------------------

/// 基准终检诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BenchFinalCode {
    /// 口径缺。
    ProtocolMissing = 7060,
    /// 对拍不符。
    CompareMismatch = 7061,
    /// 指纹缺。
    FingerprintMissing = 7062,
}

impl BenchFinalCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            BenchFinalCode::ProtocolMissing => "口径缺失",
            BenchFinalCode::CompareMismatch => "对拍不符",
            BenchFinalCode::FingerprintMissing => "指纹缺失",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(
            self,
            BenchFinalCode::ProtocolMissing | BenchFinalCode::FingerprintMissing
        )
    }
}

// ---------------------------------------------------------------------------
// 二、终检器
// ---------------------------------------------------------------------------

/// 基准结果。
#[derive(Clone, Copy, Debug)]
pub struct BenchResult {
    /// 基准值。
    pub value: u32,
    /// 实测值。
    pub measured: u32,
    /// 是否有口径。
    pub has_protocol: bool,
    /// 是否有指纹。
    pub has_fingerprint: bool,
}

/// 终检器。
pub struct BenchFinalChecker;

impl BenchFinalChecker {
    /// 终检一条。
    pub fn verify(r: &BenchResult) -> Result<(), BenchFinalCode> {
        if !r.has_protocol {
            return Err(BenchFinalCode::ProtocolMissing);
        }
        if !r.has_fingerprint {
            return Err(BenchFinalCode::FingerprintMissing);
        }
        if r.value != r.measured {
            return Err(BenchFinalCode::CompareMismatch);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、判据
// ---------------------------------------------------------------------------

/// 基准终检判据集。
pub fn run_benchfinal_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver12n");

    let good = BenchResult {
        value: 100,
        measured: 100,
        has_protocol: true,
        has_fingerprint: true,
    };

    // 1. 合规过。
    chk.add("7060-合规过", BenchFinalChecker::verify(&good).is_ok(), "");

    // 2. 口径缺拦。
    let no_proto = BenchResult {
        has_protocol: false,
        ..good
    };
    chk.add(
        "7061-口径拦",
        BenchFinalChecker::verify(&no_proto).err() == Some(BenchFinalCode::ProtocolMissing),
        "",
    );

    // 3. 对拍不符。
    let mismatch = BenchResult {
        measured: 200,
        ..good
    };
    chk.add(
        "7062-对拍不符",
        BenchFinalChecker::verify(&mismatch).err() == Some(BenchFinalCode::CompareMismatch),
        "",
    );

    // 4. 指纹缺拦。
    let no_fp = BenchResult {
        has_fingerprint: false,
        ..good
    };
    chk.add(
        "7063-指纹拦",
        BenchFinalChecker::verify(&no_fp).err() == Some(BenchFinalCode::FingerprintMissing),
        "",
    );

    chk
}
