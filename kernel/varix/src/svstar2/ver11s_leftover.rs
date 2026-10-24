//! VE-F3579 · E09 组遗留登记 —— 遗留登记 + 扩展计划 + 老化催办 + 可追溯 + E10 对接。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3579`
//!
//! # 职责（锚点原文拆解）
//!
//! - **两列遗留**：词典引擎覆盖度扩展计划 / 跨域对拍样本扩充。
//! - **老化催办** + 可追溯 + E10 聚合对接确认。
//!
//! # 复杂度
//!
//! O(遗留)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E20 段续：6760 起）
// ---------------------------------------------------------------------------

/// E09 遗留诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Leftover9Code {
    /// 登记缺失。
    NotRegistered = 6760,
    /// 计划缺失。
    PlanMissing = 6761,
    /// 老化催办。
    Aged = 6762,
}

impl Leftover9Code {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            Leftover9Code::NotRegistered => "遗留未登记",
            Leftover9Code::PlanMissing => "计划缺失",
            Leftover9Code::Aged => "老化催办",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        false
    }
}

/// 老化阈值（天）。
pub const AGING9_DAYS: u32 = 30;

// ---------------------------------------------------------------------------
// 二、遗留册
// ---------------------------------------------------------------------------

/// 遗留条目。
#[derive(Clone, Debug)]
pub struct Leftover9 {
    /// id。
    pub id: u32,
    /// 描述。
    pub desc: &'static str,
    /// 计划。
    pub plan: &'static str,
    /// 年龄（天）。
    pub age_days: u32,
}

impl Leftover9 {
    /// 老化判定。
    pub const fn aged(&self) -> bool {
        self.age_days > AGING9_DAYS
    }
}

/// 遗留册。
pub struct Leftover9Book {
    entries: Vec<Leftover9>,
    next_id: u32,
}

impl Leftover9Book {
    /// 新建（预登两列）。
    pub fn standard() -> Self {
        let entries = alloc::vec![
            Leftover9 {
                id: 1,
                desc: "词典引擎覆盖度待扩展",
                plan: "F 域续扩四类规则到 200 条",
                age_days: 0,
            },
            Leftover9 {
                id: 2,
                desc: "跨域对拍样本待扩充",
                plan: "F 域补 50 对控件样本",
                age_days: 0,
            },
        ];
        Self {
            entries,
            next_id: 3,
        }
    }

    /// 补登。
    pub fn register(
        &mut self,
        desc: &'static str,
        plan: &'static str,
    ) -> Result<u32, Leftover9Code> {
        if desc.is_empty() {
            return Err(Leftover9Code::NotRegistered);
        }
        if plan.is_empty() {
            return Err(Leftover9Code::PlanMissing);
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.entries.push(Leftover9 {
            id,
            desc,
            plan,
            age_days: 0,
        });
        Ok(id)
    }

    /// 老化清单。
    pub fn overdue(&self) -> Vec<u32> {
        self.entries
            .iter()
            .filter(|e| e.aged())
            .map(|e| e.id)
            .collect()
    }

    /// E10 对接。
    pub fn confirm_e10(&self) -> (usize, usize) {
        (self.entries.len(), self.overdue().len())
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

// ---------------------------------------------------------------------------
// 三、判据
// ---------------------------------------------------------------------------

/// E09 遗留判据集。
pub fn run_leftover9_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver11s");

    // 1. 两列已登。
    let mut b = Leftover9Book::standard();
    chk.add("6760-两列已登", b.len() == 2, "");

    // 2. 补登。
    chk.add("6761-补登", b.register("额外项", "有计划").is_ok(), "");

    // 3. 空计划拒。
    chk.add(
        "6762-空计划拒",
        b.register("x", "").err() == Some(Leftover9Code::PlanMissing),
        "",
    );

    // 4. 未老化。
    chk.add("6763-未老化", b.overdue().is_empty(), "");

    // 5. E10 对接。
    let (n, aged) = b.confirm_e10();
    chk.add("6764-E10对接", n == 3 && aged == 0, "");

    // 6. 阈值常量。
    chk.add("6765-阈值", AGING9_DAYS == 30, "");

    chk
}
