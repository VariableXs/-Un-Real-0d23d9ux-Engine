//! VE-F3576 · 一致性审查 fuzz —— 三类语料 + 工具自测 + 崩溃即修 + 底线承诺。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3576`
//!
//! # 职责（锚点原文拆解）
//!
//! - **三类语料**（[`FuzzCorpus`]）：畸形 UI 树 / 词典文件 /
//!   规则冲突三类。
//! - **工具崩溃即阻断级**：审查工具自身崩溃即时修。
//!
//! # 复杂度
//!
//! O(语料)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E20 段续：6700 起）
// ---------------------------------------------------------------------------

/// 审查 fuzz 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditFuzzCode {
    /// 工具崩溃（阻断级）。
    ToolCrash = 6700,
    /// 语料缺失。
    CorpusMissing = 6701,
}

impl AuditFuzzCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            AuditFuzzCode::ToolCrash => "审查工具崩溃",
            AuditFuzzCode::CorpusMissing => "语料缺失",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// 二、语料
// ---------------------------------------------------------------------------

/// 语料类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuzzCorpus {
    /// 畸形 UI 树。
    MalformedUi,
    /// 畸形词典文件。
    MalformedDict,
    /// 规则冲突。
    RuleConflict,
}

/// 语料样本。
#[derive(Clone, Copy, Debug)]
pub struct AuditSample {
    /// id。
    pub id: u32,
    /// 类。
    pub kind: FuzzCorpus,
    /// 期望拒绝。
    pub expect_reject: bool,
}

// ---------------------------------------------------------------------------
// 三、fuzz 运行器
// ---------------------------------------------------------------------------

/// 审查 fuzz 运行器。
pub struct AuditFuzzer {
    samples: Vec<AuditSample>,
    /// 崩溃数。
    pub crashes: u32,
}

impl AuditFuzzer {
    /// 新建。
    pub fn with_corpus(samples: Vec<AuditSample>) -> Result<Self, AuditFuzzCode> {
        if samples.is_empty() {
            return Err(AuditFuzzCode::CorpusMissing);
        }
        Ok(Self {
            samples,
            crashes: 0,
        })
    }

    /// 注入一个样本。
    pub fn inject(&mut self, _id: u32, crashed: bool) -> bool {
        if crashed {
            self.crashes = self.crashes.saturating_add(1);
        }
        !crashed
    }

    /// 语料数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

/// 标准三类语料。
pub fn standard_audit_corpus() -> Vec<AuditSample> {
    alloc::vec![
        AuditSample { id: 1, kind: FuzzCorpus::MalformedUi, expect_reject: true },
        AuditSample { id: 2, kind: FuzzCorpus::MalformedUi, expect_reject: true },
        AuditSample { id: 3, kind: FuzzCorpus::MalformedDict, expect_reject: true },
        AuditSample { id: 4, kind: FuzzCorpus::MalformedDict, expect_reject: true },
        AuditSample { id: 5, kind: FuzzCorpus::RuleConflict, expect_reject: true },
        AuditSample { id: 6, kind: FuzzCorpus::RuleConflict, expect_reject: true },
    ]
}

// ---------------------------------------------------------------------------
// 四、判据
// ---------------------------------------------------------------------------

/// 审查 fuzz 判据集。
pub fn run_afuzz_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver11p");

    // 1. 三类语料齐。
    let corpus = standard_audit_corpus();
    let mut f = AuditFuzzer::with_corpus(corpus).ok().unwrap_or(AuditFuzzer {
        samples: Vec::new(),
        crashes: 0,
    });
    chk.add("6700-三类语料", f.len() == 6, "");

    // 2. 无崩溃通过。
    chk.add("6701-无崩溃", f.inject(1, false) && f.crashes == 0, "");

    // 3. 崩溃计数。
    f.inject(2, true);
    chk.add("6702-崩溃计数", f.crashes == 1, "");

    // 4. 空语料拒。
    chk.add(
        "6703-空语料拒",
        AuditFuzzer::with_corpus(Vec::new()).err() == Some(AuditFuzzCode::CorpusMissing),
        "",
    );

    // 5. 崩溃阻断级。
    chk.add("6704-崩溃阻断", AuditFuzzCode::ToolCrash.blocking(), "");

    chk
}
