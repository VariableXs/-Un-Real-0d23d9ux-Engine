//! CGPU-F0449 · 边界场景总览（目标 340 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 边界总览：五类边界场景清单（低资源设备/应急资源枯竭/启动早期冷
//!   启动/离线软渲/热极限与电量临界——预算系统的极端工况全景）；边界
//!   设计原则（边界场景提前定义行为——绝不临场发挥）；与 O 域降级链的
//!   关系（O 域管降质参数表、本域管预算侧配合——边界分工声明）；演练
//!   承诺（每类边界定期演练）。判据：五类清单、提前定义、分工声明、
//!   演练承诺、判据。
//!
//! 设计纪律：
//! - 五类清单常量入册（低资源/应急/冷启动/离线/热电池临界）——
//!   每类带：触发条件、预算侧行为、专属配方引用（后续 F0450~F0455
//!   逐类落地）、演练间隔。清单是数据不是散文——`EdgeCatalog` 可查询。
//! - 提前定义原则的自证：每类边界的行为必须在清单中非空（空行为 =
//!   临场发挥的温床，拒入册）。
//! - O 域分工声明：本域条目显式携带 `owns: 预算侧` 标注；O 域的
//!   降质参数表不在本册（越界条目拒收——`E_EDGE_BOUND`）。
//! - 演练承诺：每类演练间隔常量化 + 到期可查（due 判定）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0F40 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0F40，全仓独占）。
pub const SEG_BASE: u16 = 0x0F40;

pub const E_EDGE_EMPTY: u16 = SEG_BASE + 1; // 边界行为空（临场发挥风险）
pub const E_EDGE_BOUND: u16 = SEG_BASE + 2; // 越界条目（O 域职责）
pub const E_EDGE_KIND: u16 = SEG_BASE + 3; // 边界类非法
pub const E_EDGE_DUE: u16 = SEG_BASE + 4; // 演练到期未练

/// 五类边界场景 ID。
pub const EDGE_LOWRES: u8 = 0;
pub const EDGE_EMERGENCY: u8 = 1;
pub const EDGE_COLDSTART: u8 = 2;
pub const EDGE_OFFLINE: u8 = 3;
pub const EDGE_THERMAL_BATT: u8 = 4;
pub const EDGE_KINDS: usize = 5;

pub fn edge_name(k: u8) -> &'static str {
    match k {
        EDGE_LOWRES => "低资源设备",
        EDGE_EMERGENCY => "应急资源枯竭",
        EDGE_COLDSTART => "启动早期冷启动",
        EDGE_OFFLINE => "离线软渲",
        _ => "热极限与电量临界",
    }
}

/// 演练间隔（天，按类）。
pub const DRILL_INTERVAL_DAYS: [u64; EDGE_KINDS] = [30, 7, 3, 30, 14];

/// 边界条目（预算侧职责清单内）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeEntry {
    pub kind: u8,
    /// 触发条件（预定义——非空）。
    pub trigger: &'static str,
    /// 预算侧行为（预定义——非空）。
    pub budget_action: &'static str,
    /// 配方引用（F0450~F0455 落地模块）。
    pub recipe_ref: &'static str,
    /// 上次演练日（天序）。
    pub last_drill_day: u64,
}

/// 边界目录。
#[derive(Debug, Default)]
pub struct EdgeCatalog {
    entries: Vec<EdgeEntry>,
    /// 当前天序。
    pub today: u64,
}

impl EdgeCatalog {
    pub fn new(today: u64) -> Self {
        Self {
            entries: Vec::new(),
            today,
        }
    }

    /// 入册：五类之一 + 行为非空 + 职责在预算侧（O 域降质参数表拒收）。
    pub fn enroll(
        &mut self,
        kind: u8,
        trigger: &'static str,
        budget_action: &'static str,
        recipe_ref: &'static str,
    ) -> Result<(), u16> {
        if kind as usize >= EDGE_KINDS {
            return Err(E_EDGE_KIND);
        }
        if trigger.is_empty() || budget_action.is_empty() || recipe_ref.is_empty() {
            return Err(E_EDGE_EMPTY);
        }
        // 分工声明：O 域的降质参数表不是本册条目。
        if trigger.contains("降质参数表") || budget_action.contains("降质参数表") {
            return Err(E_EDGE_BOUND);
        }
        // 同类替换（最新定义生效）。
        if let Some(e) = self.entries.iter_mut().find(|e| e.kind == kind) {
            e.trigger = trigger;
            e.budget_action = budget_action;
            e.recipe_ref = recipe_ref;
            return Ok(());
        }
        self.entries.push(EdgeEntry {
            kind,
            trigger,
            budget_action,
            recipe_ref,
            last_drill_day: self.today,
        });
        Ok(())
    }

    /// 五类清单完整性：全部入册且条目非空。
    pub fn five_kinds_complete(&self) -> Result<(), u16> {
        for k in 0..EDGE_KINDS as u8 {
            match self.entries.iter().find(|e| e.kind == k) {
                Some(e) if !e.budget_action.is_empty() => {}
                _ => return Err(E_EDGE_EMPTY),
            }
        }
        Ok(())
    }

    /// 演练到期判定：距上次演练 ≥ 间隔 → 到期。
    pub fn drill_due(&self, kind: u8) -> Result<bool, u16> {
        if kind as usize >= EDGE_KINDS {
            return Err(E_EDGE_KIND);
        }
        let e = self.entries.iter().find(|e| e.kind == kind).ok_or(E_EDGE_KIND)?;
        Ok(self.today.saturating_sub(e.last_drill_day) >= DRILL_INTERVAL_DAYS[kind as usize])
    }

    /// 完成演练。
    pub fn mark_drilled(&mut self, kind: u8) -> Result<(), u16> {
        let e = self
            .entries
            .iter_mut()
            .find(|e| e.kind == kind)
            .ok_or(E_EDGE_KIND)?;
        e.last_drill_day = self.today;
        Ok(())
    }

    /// 分工声明自证：本册条目全部是预算侧行为（不含 O 域降质参数）。
    pub fn owns_budget_side(&self) -> bool {
        self.entries
            .iter()
            .all(|e| !e.budget_action.contains("降质参数表"))
    }

    pub fn entry_of(&self, kind: u8) -> Result<&EdgeEntry, u16> {
        self.entries.iter().find(|e| e.kind == kind).ok_or(E_EDGE_KIND)
    }

    pub fn count(&self) -> usize {
        self.entries.len()
    }
}

// ==========================================================================
// 判据（施工书：五类清单、提前定义、分工声明、演练承诺、判据）
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> EdgeCatalog {
        let mut c = EdgeCatalog::new(100);
        c.enroll(EDGE_LOWRES, "设备档=低端", "轻量预算模型+配额压缩", "F0450").unwrap();
        c.enroll(EDGE_EMERGENCY, "资源水位<5%", "三级弃车协议", "F0451").unwrap();
        c.enroll(EDGE_COLDSTART, "无画像无统计", "保守默认值表+首帧直通", "F0452").unwrap();
        c.enroll(EDGE_OFFLINE, "GPU 离线", "CPU 预算全分配", "F0453").unwrap();
        c.enroll(EDGE_THERMAL_BATT, "热降档/电量临界", "分级压缩+三级紧缩", "F0454/F0455").unwrap();
        c
    }

    #[test]
    fn a01_五类清单_全景入册() {
        let c = catalog();
        assert_eq!(c.count(), 5);
        assert_eq!(c.five_kinds_complete(), Ok(()));
        // 名称表覆盖。
        for k in 0..EDGE_KINDS as u8 {
            assert!(!edge_name(k).is_empty());
        }
        // 缺类不完整。
        let mut c2 = EdgeCatalog::new(1);
        c2.enroll(EDGE_LOWRES, "t", "a", "F0450").unwrap();
        assert_eq!(c2.five_kinds_complete(), Err(E_EDGE_EMPTY));
    }

    #[test]
    fn a02_提前定义_空行为拒入册() {
        let mut c = EdgeCatalog::new(1);
        // 触发条件空拒。
        assert_eq!(c.enroll(EDGE_LOWRES, "", "a", "F0450"), Err(E_EDGE_EMPTY));
        // 行为空拒。
        assert_eq!(c.enroll(EDGE_LOWRES, "t", "", "F0450"), Err(E_EDGE_EMPTY));
        // 配方引用空拒。
        assert_eq!(c.enroll(EDGE_LOWRES, "t", "a", ""), Err(E_EDGE_EMPTY));
        // 非法类拒。
        assert_eq!(c.enroll(9, "t", "a", "F0450"), Err(E_EDGE_KIND));
        assert_eq!(c.count(), 0);
    }

    #[test]
    fn a03_分工声明_O域条目拒收() {
        let mut c = EdgeCatalog::new(1);
        // O 域降质参数表条目拒。
        assert_eq!(
            c.enroll(EDGE_LOWRES, "触发:降质参数表", "行为", "F0450"),
            Err(E_EDGE_BOUND)
        );
        assert_eq!(
            c.enroll(EDGE_LOWRES, "t", "改降质参数表", "F0450"),
            Err(E_EDGE_BOUND)
        );
        // 正常条目全部预算侧。
        let c2 = catalog();
        assert!(c2.owns_budget_side());
        assert_eq!(c2.entry_of(EDGE_EMERGENCY).map(|e| e.recipe_ref), Ok("F0451"));
    }

    #[test]
    fn a04_演练承诺_间隔与到期() {
        let mut c = catalog(); // last_drill = 100。
        // 应急间隔 7 天：107 天到期。
        assert_eq!(c.drill_due(EDGE_EMERGENCY), Ok(false));
        c.today = 107;
        assert_eq!(c.drill_due(EDGE_EMERGENCY), Ok(true));
        c.mark_drilled(EDGE_EMERGENCY).ok();
        assert_eq!(c.drill_due(EDGE_EMERGENCY), Ok(false));
        // 冷启动间隔 3 天。
        c.today = 103;
        assert_eq!(c.drill_due(EDGE_COLDSTART), Ok(true));
        // 未入册类查询拒。
        let mut c3 = EdgeCatalog::new(1);
        assert_eq!(c3.drill_due(EDGE_OFFLINE), Err(E_EDGE_KIND));
        assert_eq!(c3.drill_due(9), Err(E_EDGE_KIND));
    }

    #[test]
    fn a05_同类替换_最新定义生效() {
        let mut c = EdgeCatalog::new(1);
        c.enroll(EDGE_LOWRES, "触发旧", "行为旧", "F0450").unwrap();
        c.enroll(EDGE_LOWRES, "触发新", "行为新", "F0450").unwrap();
        assert_eq!(c.count(), 1); // 替换不叠加。
        assert_eq!(c.entry_of(EDGE_LOWRES).map(|e| e.trigger), Ok("触发新"));
        assert_eq!(c.entry_of(EDGE_LOWRES).map(|e| e.budget_action), Ok("行为新"));
    }
}
