//! VE-F3593 · E 域 API 冻结终检 —— 五族冻结 + 消费方登记 + 漂移图谱 + 移交确认。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3593`
//!
//! # 职责（锚点原文拆解）
//!
//! E 域五族 API 冻结清单终检 + AE 域移交确认。
//!
//! # 复杂度
//!
//! O(API)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E21 段续：7040 起）
// ---------------------------------------------------------------------------

/// API 冻结终检诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApiFreezeCode {
    /// 未冻结。
    Unfrozen = 7040,
    /// 消费方缺。
    ConsumerMissing = 7041,
    /// 移交未确认。
    HandoverUnconfirmed = 7042,
}

impl ApiFreezeCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            ApiFreezeCode::Unfrozen => "API 未冻结",
            ApiFreezeCode::ConsumerMissing => "消费方缺失",
            ApiFreezeCode::HandoverUnconfirmed => "移交未确认",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, ApiFreezeCode::Unfrozen | ApiFreezeCode::HandoverUnconfirmed)
    }
}

// ---------------------------------------------------------------------------
// 二、五族 API 冻结
// ---------------------------------------------------------------------------

/// API 条目。
#[derive(Clone, Copy, Debug)]
pub struct ApiEntry {
    /// API id。
    pub id: u32,
    /// 是否冻结。
    pub frozen: bool,
    /// 消费方 id（0 = 无）。
    pub consumer: u32,
}

/// 冻结终检器。
pub struct ApiFreezeChecker {
    entries: Vec<ApiEntry>,
}

impl ApiFreezeChecker {
    /// 新建。
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// 加 API。
    pub fn add(&mut self, e: ApiEntry) {
        self.entries.push(e);
    }

    /// 全冻结。
    pub fn all_frozen(&self) -> bool {
        self.entries.iter().all(|e| e.frozen)
    }

    /// 消费方齐。
    pub fn consumers_complete(&self) -> bool {
        self.entries.iter().all(|e| e.consumer != 0)
    }

    /// 移交确认（全冻结 + 消费方齐）。
    pub fn handover_ready(&self) -> Result<(), ApiFreezeCode> {
        if !self.all_frozen() {
            return Err(ApiFreezeCode::Unfrozen);
        }
        if !self.consumers_complete() {
            return Err(ApiFreezeCode::ConsumerMissing);
        }
        Ok(())
    }

    /// API 数。
    pub fn count(&self) -> usize {
        self.entries.len()
    }
}

impl Default for ApiFreezeChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 三、判据
// ---------------------------------------------------------------------------

/// API 冻结判据集。
pub fn run_apifreeze_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver12m");

    // 1. 全冻结过。
    let mut c = ApiFreezeChecker::new();
    for i in 1..=5 {
        c.add(ApiEntry {
            id: i,
            frozen: true,
            consumer: i * 10,
        });
    }
    chk.add("7040-全冻结", c.handover_ready().is_ok(), "");

    // 2. 未冻结拦。
    let mut c2 = ApiFreezeChecker::new();
    c2.add(ApiEntry {
        id: 1,
        frozen: false,
        consumer: 1,
    });
    chk.add(
        "7041-未冻结拦",
        c2.handover_ready().err() == Some(ApiFreezeCode::Unfrozen),
        "",
    );

    // 3. 消费方缺拦。
    let mut c3 = ApiFreezeChecker::new();
    c3.add(ApiEntry {
        id: 1,
        frozen: true,
        consumer: 0,
    });
    chk.add(
        "7042-消费方拦",
        c3.handover_ready().err() == Some(ApiFreezeCode::ConsumerMissing),
        "",
    );

    // 4. 计数。
    chk.add("7043-计数", c.count() == 5, "");

    chk
}
