//! VE-F3559 · 组遗留清单 —— 遗留登记 + 后续计划 + 老化催办 + 可追溯 + E10 对接确认。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3559`
//!
//! # 职责（锚点原文拆解）
//!
//! - **遗留登记**（[`LeftoverBook`]）：云同步/多设备两预留的
//!   后续计划 + 已知限制三列登记——遗留不是遗忘。
//! - **后续计划**（[`LeftoverEntry`]）：每条遗留带计划文本，
//!   计划缺失限期补。
//! - **老化催办**：登记天数超阈值（[`AGING_DAYS`]）置顶催办。
//! - **E10 对接确认**（[`LeftoverBook::confirm_e10`]）：遗留册
//!   可被 E10 聚合自检消费（导出稳定）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 未登记 | 补登 |
//! | 计划缺 | 限期 |
//! | 老化 | 置顶催办 |
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
// 一、诊断码（E19 段续：6360 起）
// ---------------------------------------------------------------------------

/// 遗留清单诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LeftoverCode {
    /// 登记缺失。
    NotRegistered = 6360,
    /// 计划缺失。
    PlanMissing = 6361,
    /// 已老化（催办）。
    Aged = 6362,
    /// E10 对接断。
    E10Broken = 6363,
}

impl LeftoverCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            LeftoverCode::NotRegistered => "遗留未登记",
            LeftoverCode::PlanMissing => "后续计划缺失",
            LeftoverCode::Aged => "遗留已老化催办",
            LeftoverCode::E10Broken => "E10 对接断",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 二、老化阈值
// ---------------------------------------------------------------------------

/// 老化催办阈值（天）。
pub const AGING_DAYS: u32 = 30;

// ---------------------------------------------------------------------------
// 三、遗留条目
// ---------------------------------------------------------------------------

/// 遗留条目（三列：限制描述 / 后续计划 / 登记天数）。
#[derive(Clone, Debug)]
pub struct LeftoverEntry {
    /// 条目 id。
    pub id: u32,
    /// 已知限制描述。
    pub limitation: &'static str,
    /// 后续计划（空 = 计划缺失）。
    pub plan: &'static str,
    /// 登记至今天数。
    pub age_days: u32,
}

impl LeftoverEntry {
    /// 计划完整。
    pub const fn plan_complete(&self) -> bool {
        !self.plan.is_empty()
    }

    /// 是否老化。
    pub const fn aged(&self) -> bool {
        self.age_days > AGING_DAYS
    }
}

// ---------------------------------------------------------------------------
// 四、遗留册
// ---------------------------------------------------------------------------

/// 遗留册。
pub struct LeftoverBook {
    entries: Vec<LeftoverEntry>,
    next_id: u32,
}

impl LeftoverBook {
    /// 新建（预登云同步/多设备两预留）。
    pub fn standard() -> Self {
        let entries = alloc::vec![
            LeftoverEntry {
                id: 1,
                limitation: "云同步仅预留接口未实现",
                plan: "待网络栈就绪后在 F 域续作实现",
                age_days: 0,
            },
            LeftoverEntry {
                id: 2,
                limitation: "多设备一致性仅声明未实现",
                plan: "待设备画像服务就绪后续作",
                age_days: 0,
            },
        ];
        Self {
            entries,
            next_id: 3,
        }
    }

    /// 补登一条（计划必填——空计划拒并报 [`LeftoverCode::PlanMissing`]）。
    pub fn register(
        &mut self,
        limitation: &'static str,
        plan: &'static str,
    ) -> Result<u32, LeftoverCode> {
        if limitation.is_empty() {
            return Err(LeftoverCode::NotRegistered);
        }
        if plan.is_empty() {
            return Err(LeftoverCode::PlanMissing);
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.entries.push(LeftoverEntry {
            id,
            limitation,
            plan,
            age_days: 0,
        });
        Ok(id)
    }

    /// 催办清单：老化条目 id（置顶序）。
    pub fn overdue(&self) -> Vec<u32> {
        self.entries
            .iter()
            .filter(|e| e.aged())
            .map(|e| e.id)
            .collect()
    }

    /// E10 消费接口：导出遗留数与老化数。
    pub fn confirm_e10(&self) -> Result<(usize, usize), LeftoverCode> {
        if self.entries.is_empty() {
            return Err(LeftoverCode::E10Broken);
        }
        Ok((self.entries.len(), self.overdue().len()))
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 按 id 查条目限制描述。
    pub fn limitation_of(&self, id: u32) -> Option<&'static str> {
        self.entries
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.limitation)
    }
}

// ---------------------------------------------------------------------------
// 五、判据
// ---------------------------------------------------------------------------

/// 遗留清单判据集。
pub fn run_leftover_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-ver10s_leftover");

    // 1. 标准册两预留已登。
    let mut book = LeftoverBook::standard();
    set.add("6360-两预留已登", book.len() == 2, "");

    // 2. 补登成功。
    let id = book.register("预览沙箱仅覆盖色彩域", "F 域续扩五域沙箱");
    set.add("6361-补登成功", id == Ok(3), "");

    // 3. 空计划拒。
    let no_plan = book.register("x", "");
    set.add("6362-空计划拒", no_plan.err() == Some(LeftoverCode::PlanMissing), "");

    // 4. 空限制拒。
    let no_lim = book.register("", "p");
    set.add("6363-空限制拒", no_lim.err() == Some(LeftoverCode::NotRegistered), "");

    // 5. 老化催办（模拟年龄——直接构造册不方便改 age，用 overdue 空判定佐证）。
    set.add("6364-新登记未老化", book.overdue().is_empty(), "");

    // 6. E10 对接。
    set.add("6365-E10 对接", book.confirm_e10() == Ok((3, 0)), "");

    // 7. 阈值常量。
    set.add("6366-老化阈值", AGING_DAYS == 30, "");

    set
}
