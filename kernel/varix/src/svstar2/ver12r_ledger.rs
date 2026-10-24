//! VE-F3598 · E 域台账 —— 域级台账 + 变更可追溯 + 状态机 + 开放导出。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3598`
//!
//! # 职责（锚点原文拆解）
//!
//! E 域变更台账（可追溯、可审计、可导出）。
//!
//! # 复杂度
//!
//! O(条目)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E21 段续：7140 起）
// ---------------------------------------------------------------------------

/// 台账诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LedgerCode {
    /// 条目缺失。
    EntryMissing = 7140,
    /// 状态非法。
    StateInvalid = 7141,
}

impl LedgerCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            LedgerCode::EntryMissing => "台账条目缺失",
            LedgerCode::StateInvalid => "台账状态非法",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 二、台账
// ---------------------------------------------------------------------------

/// 台账状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LedgerState {
    /// 草稿。
    Draft,
    /// 已提交。
    Submitted,
    /// 已归档。
    Archived,
}

impl LedgerState {
    /// 状态短名。
    pub const fn label(self) -> &'static str {
        match self {
            LedgerState::Draft => "草稿",
            LedgerState::Submitted => "已提交",
            LedgerState::Archived => "已归档",
        }
    }
}

/// 台账条目。
#[derive(Clone, Copy, Debug)]
pub struct LedgerEntry {
    /// id。
    pub id: u32,
    /// 状态。
    pub state: LedgerState,
}

/// 台账。
pub struct DomainLedger {
    entries: Vec<LedgerEntry>,
    next_id: u32,
}

impl DomainLedger {
    /// 新建。
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
        }
    }

    /// 登记（草稿态）。
    pub fn register(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.entries.push(LedgerEntry {
            id,
            state: LedgerState::Draft,
        });
        id
    }

    /// 状态推进（草稿→提交→归档）。
    pub fn advance(&mut self, id: u32) -> Result<LedgerState, LedgerCode> {
        for e in self.entries.iter_mut() {
            if e.id == id {
                e.state = match e.state {
                    LedgerState::Draft => LedgerState::Submitted,
                    LedgerState::Submitted => LedgerState::Archived,
                    LedgerState::Archived => return Err(LedgerCode::StateInvalid),
                };
                return Ok(e.state);
            }
        }
        Err(LedgerCode::EntryMissing)
    }

    /// 已归档数。
    pub fn archived_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.state == LedgerState::Archived)
            .count()
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for DomainLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 三、判据
// ---------------------------------------------------------------------------

/// 台账判据集。
pub fn run_ledger_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver12r");

    // 1. 登记。
    let mut l = DomainLedger::new();
    let id = l.register();
    chk.add("7140-登记", id == 1 && l.len() == 1, "");

    // 2. 状态推进。
    let s1 = l.advance(id);
    let s2 = l.advance(id);
    chk.add(
        "7141-状态推进",
        s1 == Ok(LedgerState::Submitted) && s2 == Ok(LedgerState::Archived),
        "",
    );

    // 3. 归档后不可再进。
    chk.add(
        "7142-终态拦",
        l.advance(id).err() == Some(LedgerCode::StateInvalid),
        "",
    );

    // 4. 未知 id。
    chk.add(
        "7143-未知拦",
        l.advance(99).err() == Some(LedgerCode::EntryMissing),
        "",
    );

    // 5. 归档计数。
    chk.add("7144-归档计数", l.archived_count() == 1, "");

    // 6. 状态名。
    chk.add("7145-状态名", LedgerState::Draft.label() == "草稿", "");

    chk
}
