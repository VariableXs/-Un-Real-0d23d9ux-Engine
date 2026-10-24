//! VE-F3582 · E 域接口冻结清单 —— 三列清单 + F 域预清点 + 限期标记 + 漂移图谱。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3582`
//!
//! # 职责（锚点原文拆解）
//!
//! - **三列清单**（[`FreezeEntry`]）：冻结日期 / 冻结人 / 消费方。
//! - **F 域预清点**（[`PreRegister`]）：下一域图像编解码预登记。
//! - **限期标记**：未冻结的标记限期。
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
// 一、诊断码（E21 段续：6820 起）
// ---------------------------------------------------------------------------

/// 冻结清单诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreezeCode {
    /// 未冻结（限期）。
    NotFrozen = 6820,
    /// 预清点缺。
    PreRegisterMissing = 6821,
    /// 漂移。
    Drift = 6822,
}

impl FreezeCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            FreezeCode::NotFrozen => "接口未冻结限期",
            FreezeCode::PreRegisterMissing => "F 域预清点缺",
            FreezeCode::Drift => "冻结清单漂移",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 二、三列清单
// ---------------------------------------------------------------------------

/// 冻结条目。
#[derive(Clone, Copy, Debug)]
pub struct FreezeEntry {
    /// API id。
    pub api: u32,
    /// 冻结日期（日序号）。
    pub freeze_day: u32,
    /// 冻结人 id。
    pub freezer: u32,
    /// 消费方 id。
    pub consumer: u32,
}

impl FreezeEntry {
    /// 三列齐备。
    pub const fn complete(&self) -> bool {
        self.freeze_day != 0 && self.freezer != 0 && self.consumer != 0
    }
}

/// 冻结清单。
pub struct FreezeList {
    entries: Vec<FreezeEntry>,
}

impl FreezeList {
    /// 新建。
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// 加条目。
    pub fn add(&mut self, e: FreezeEntry) {
        self.entries.push(e);
    }

    /// 未冻结数。
    pub fn unfrozen_count(&self) -> usize {
        self.entries.iter().filter(|e| !e.complete()).count()
    }

    /// 已冻结数。
    pub fn frozen_count(&self) -> usize {
        self.entries.iter().filter(|e| e.complete()).count()
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

impl Default for FreezeList {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 三、F 域预清点
// ---------------------------------------------------------------------------

/// 预登记项。
#[derive(Clone, Copy, Debug)]
pub struct PreRegister {
    /// 预登记 id。
    pub id: u32,
    /// 名称 id。
    pub name_id: u32,
}

/// 预清点器。
pub struct PreClearance {
    items: Vec<PreRegister>,
}

impl PreClearance {
    /// 新建。
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
        }
    }

    /// 预登记一项。
    pub fn register(&mut self, id: u32, name_id: u32) {
        self.items.push(PreRegister { id, name_id });
    }

    /// 预清点数。
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl Default for PreClearance {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 四、判据
// ---------------------------------------------------------------------------

/// 冻结清单判据集。
pub fn run_freeze_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver12b");

    // 1. 三列齐备。
    let e = FreezeEntry {
        api: 1,
        freeze_day: 100,
        freezer: 7,
        consumer: 3,
    };
    chk.add("6820-三列齐", e.complete(), "");

    // 2. 缺列不完整。
    let bad = FreezeEntry {
        api: 2,
        freeze_day: 0,
        freezer: 7,
        consumer: 3,
    };
    chk.add("6821-缺列", !bad.complete(), "");

    // 3. 清单统计。
    let mut l = FreezeList::new();
    l.add(e);
    l.add(bad);
    chk.add("6822-清单统计", l.frozen_count() == 1 && l.unfrozen_count() == 1, "");

    // 4. F 域预清点。
    let mut p = PreClearance::new();
    p.register(1, 10);
    p.register(2, 20);
    chk.add("6823-预清点", p.count() == 2, "");

    // 5. 空预清点。
    let empty = PreClearance::new();
    chk.add("6824-空预清点", empty.is_empty(), "");

    chk
}
