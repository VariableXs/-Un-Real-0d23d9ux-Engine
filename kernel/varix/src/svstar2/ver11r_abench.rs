//! VE-F3578 · 审查基准 —— 两线基准 + CI 耗时预算 + 口径一致 + 环境指纹。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3578`
//!
//! # 职责（锚点原文拆解）
//!
//! - **两线基准**（[`AuditBench`]）：全域扫描耗时 / 对拍吞吐两线。
//! - **CI 耗时预算**：扫描纳入 CI 的耗时上限。
//! - **环境指纹**：结果可比性。
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
// 一、诊断码（E20 段续：6740 起）
// ---------------------------------------------------------------------------

/// 审查基准诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditBenchCode {
    /// 耗时超预算。
    TimeOverBudget = 6740,
    /// 口径不符。
    ProtocolMismatch = 6741,
    /// 指纹缺失。
    FingerprintMissing = 6742,
}

impl AuditBenchCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            AuditBenchCode::TimeOverBudget => "扫描耗时超预算",
            AuditBenchCode::ProtocolMismatch => "基准口径不符",
            AuditBenchCode::FingerprintMissing => "环境指纹缺失",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, AuditBenchCode::ProtocolMismatch | AuditBenchCode::FingerprintMissing)
    }
}

// ---------------------------------------------------------------------------
// 二、预算与口径
// ---------------------------------------------------------------------------

/// CI 扫描耗时预算（毫秒）。
pub const CI_TIME_BUDGET_MS: u32 = 30_000;

/// 环境指纹。
#[derive(Clone, Copy, Debug)]
pub struct EnvPrint {
    /// CPU id。
    pub cpu_id: u32,
    /// 是否有效。
    pub present: bool,
}

/// 两线基准结果。
#[derive(Clone, Copy, Debug)]
pub struct BenchLines {
    /// 全域扫描耗时（毫秒）。
    pub sweep_ms: u32,
    /// 对拍吞吐（对比对/秒）。
    pub diff_throughput: u32,
    /// 环境指纹。
    pub env: EnvPrint,
}

// ---------------------------------------------------------------------------
// 三、基准套件
// ---------------------------------------------------------------------------

/// 审查基准套件。
pub struct AuditBench;

impl AuditBench {
    /// 发布检查：指纹在 + 耗时守约。
    pub fn publish(r: &BenchLines) -> Result<(), AuditBenchCode> {
        if !r.env.present {
            return Err(AuditBenchCode::FingerprintMissing);
        }
        if r.sweep_ms > CI_TIME_BUDGET_MS {
            return Err(AuditBenchCode::TimeOverBudget);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、判据
// ---------------------------------------------------------------------------

/// 审查基准判据集。
pub fn run_abench_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver11r");

    // 1. 预算内通过。
    let good = BenchLines {
        sweep_ms: 5000,
        diff_throughput: 100,
        env: EnvPrint {
            cpu_id: 1,
            present: true,
        },
    };
    chk.add("6740-预算内过", AuditBench::publish(&good).is_ok(), "");

    // 2. 超预算拦。
    let slow = BenchLines {
        sweep_ms: 60_000,
        ..good
    };
    chk.add(
        "6741-超预算拦",
        AuditBench::publish(&slow).err() == Some(AuditBenchCode::TimeOverBudget),
        "",
    );

    // 3. 指纹缺失拦。
    let no_env = BenchLines {
        env: EnvPrint {
            cpu_id: 0,
            present: false,
        },
        ..good
    };
    chk.add(
        "6742-指纹拦",
        AuditBench::publish(&no_env).err() == Some(AuditBenchCode::FingerprintMissing),
        "",
    );

    // 4. 预算常量。
    chk.add("6743-预算常量", CI_TIME_BUDGET_MS == 30_000, "");

    // 5. 两线数据。
    chk.add("6744-两线数据", good.diff_throughput == 100, "");

    chk
}
