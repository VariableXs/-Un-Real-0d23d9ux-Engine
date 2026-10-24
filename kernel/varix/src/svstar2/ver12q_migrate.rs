//! VE-F3597 · E 域迁移对拍 —— 跨版本对拍 + 差异归因 + 开放导出。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3597`
//!
//! # 职责（锚点原文拆解）
//!
//! E 域跨版本迁移对拍。
//!
//! # 复杂度
//!
//! O(对拍对)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E21 段续：7120 起）
// ---------------------------------------------------------------------------

/// 迁移对拍诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MigrateCode {
    /// 对拍差异。
    DiffFound = 7120,
    /// 归因缺。
    AttributionMissing = 7121,
}

impl MigrateCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            MigrateCode::DiffFound => "迁移对拍差异",
            MigrateCode::AttributionMissing => "差异归因缺失",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 二、对拍器
// ---------------------------------------------------------------------------

/// 对拍对。
#[derive(Clone, Copy, Debug)]
pub struct MigratePair {
    /// 旧版本值。
    pub old_val: u32,
    /// 新版本值。
    pub new_val: u32,
    /// 归因 id（0 = 未归因）。
    pub attribution: u32,
}

/// 迁移对拍器。
pub struct MigrateDiffer {
    pairs: Vec<MigratePair>,
    /// 差异数。
    pub diffs: u32,
}

impl MigrateDiffer {
    /// 新建。
    pub fn new() -> Self {
        Self {
            pairs: Vec::new(),
            diffs: 0,
        }
    }

    /// 对拍一对。
    pub fn compare(&mut self, p: MigratePair) -> Result<(), MigrateCode> {
        self.pairs.push(p);
        if p.old_val != p.new_val {
            if p.attribution == 0 {
                return Err(MigrateCode::AttributionMissing);
            }
            self.diffs = self.diffs.saturating_add(1);
            return Err(MigrateCode::DiffFound);
        }
        Ok(())
    }

    /// 对拍对数。
    pub fn pair_count(&self) -> usize {
        self.pairs.len()
    }
}

impl Default for MigrateDiffer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 三、判据
// ---------------------------------------------------------------------------

/// 迁移对拍判据集。
pub fn run_migrate_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver12q");

    // 1. 一致过。
    let mut d = MigrateDiffer::new();
    let same = MigratePair {
        old_val: 42,
        new_val: 42,
        attribution: 0,
    };
    chk.add("7120-一致过", d.compare(same).is_ok(), "");

    // 2. 差异 + 归因。
    let diff = MigratePair {
        old_val: 1,
        new_val: 2,
        attribution: 5,
    };
    chk.add(
        "7121-差异归因",
        d.compare(diff).err() == Some(MigrateCode::DiffFound) && d.diffs == 1,
        "",
    );

    // 3. 归因缺失。
    let no_attr = MigratePair {
        old_val: 1,
        new_val: 2,
        attribution: 0,
    };
    chk.add(
        "7122-归因缺",
        d.compare(no_attr).err() == Some(MigrateCode::AttributionMissing),
        "",
    );

    // 4. 对数。
    chk.add("7123-对数", d.pair_count() == 3, "");

    chk
}
