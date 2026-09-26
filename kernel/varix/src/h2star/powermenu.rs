//! F275 开始菜单电源菜单 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：三+一项清单；两路入口同账目审计；长按行为文档化；
//! 软件关机完整链（B-2902 判据引用）。
//!
//! **设计要点（主册）**：开始菜单电源按钮点开三件：睡眠、关机、重启
//! ——加一个「高级」（二级菜单：安全模式引导 F193、恢复环境 F198 入口）；
//! 独立关机按钮（桌面电源键）与此菜单同走一套电源账目（B-2902），长按
//! 物理电源键=硬件级兜底（文档化，提示用户先走软件关机）；菜单项执行
//! 前无确认（开始菜单到电源按钮需两击，误触成本已足够高）。
//!
//! 实装：电源动作清单（三+一项唯一源）；动作路由（两路入口——开始
//! 菜单/桌面电源键——汇入同一账目管道，B-2902 判据引用）；长按行为
//! 文档（提示文案唯一源）；执行前无确认（路由直发——结构保证）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 电源动作（三+一项）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerAction {
    Sleep,
    Shutdown,
    Reboot,
    /// 高级二级菜单入口。
    Advanced,
}

/// 高级二级项（F193/F198 入口）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdvancedItem {
    SafeModeBoot,  // F193
    RecoveryEnv,   // F198
}

/// 主菜单三+一项清单（唯一源）。
pub const POWER_MENU: [PowerAction; 4] = [
    PowerAction::Sleep,
    PowerAction::Shutdown,
    PowerAction::Reboot,
    PowerAction::Advanced,
];

/// 高级二级清单（唯一源）。
pub const ADVANCED_MENU: [AdvancedItem; 2] = [AdvancedItem::SafeModeBoot, AdvancedItem::RecoveryEnv];

/// 电源动作账目条目（B-2902 同账——两路入口合流管道的输出）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PowerLedgerEntry {
    pub action: PowerAction,
    /// 入口：「开始菜单」或「桌面电源键」。
    pub source: &'static str,
    pub at_min: u64,
}

/// 电源服务：两路入口 → 同一账目管道。
pub struct PowerService {
    pub ledger: Vec<PowerLedgerEntry>,
}

/// 长按物理电源键的文档化提示（判据「长按行为文档化」——文案唯一源）。
pub const LONG_PRESS_DOC: &str = "长按物理电源键是硬件级兜底——请先使用软件关机（本菜单），账目才走全";

impl PowerService {
    pub fn new() -> PowerService {
        PowerService { ledger: Vec::new() }
    }

    /// 触发动作（开始菜单入口）——执行前**无确认**（两击门槛已够，
    /// 判据的结构保证：本函数直接落账执行，没有任何确认分支）。
    pub fn trigger_from_startmenu(&mut self, action: PowerAction, at_min: u64) {
        self.exec(action, "开始菜单", at_min);
    }

    /// 触发动作（桌面电源键入口）——同账目管道（同账审计判据）。
    pub fn trigger_from_deskpower(&mut self, action: PowerAction, at_min: u64) {
        self.exec(action, "桌面电源键", at_min);
    }

    fn exec(&mut self, action: PowerAction, source: &'static str, at_min: u64) {
        self.ledger.push(PowerLedgerEntry { action, source, at_min });
        // 实际执行（睡眠/关机/重启信号）由 power 底盘接管——本层保证
        // 账目先行（B-2902：先记账后动作，断电账不丢）。
    }

    /// 高级二级菜单展开。
    pub fn advanced_items() -> Vec<AdvancedItem> {
        ADVANCED_MENU.to_vec()
    }

    /// 两路同账审计：同动作两入口的账目条目动作一致（合流验证）。
    pub fn same_lane(&self, action: PowerAction) -> bool {
        self.ledger.iter().filter(|e| e.action == action).count() >= 2
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_powermenu_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F275");
    // 三+一项清单。
    set.add(
        "F275 menu list",
        POWER_MENU.len() == 4
            && POWER_MENU[0] == PowerAction::Sleep
            && POWER_MENU[3] == PowerAction::Advanced,
        "sleep/shutdown/reboot/advanced",
    );
    // 高级二级：F193+F198 两入口。
    let adv = PowerService::advanced_items();
    set.add(
        "F275 advanced submenu",
        adv.len() == 2
            && adv[0] == AdvancedItem::SafeModeBoot
            && adv[1] == AdvancedItem::RecoveryEnv,
        "safe mode + recovery",
    );
    // 两路入口同账目审计。
    let mut svc = PowerService::new();
    svc.trigger_from_startmenu(PowerAction::Shutdown, 100);
    svc.trigger_from_deskpower(PowerAction::Shutdown, 105);
    set.add(
        "F275 same ledger both lanes",
        svc.ledger.len() == 2
            && svc.same_lane(PowerAction::Shutdown)
            && svc.ledger[0].source != svc.ledger[1].source,
        "one account pipe",
    );
    // 执行前无确认：触发即落账（无确认分支可走）。
    set.add(
        "F275 no confirm gate",
        svc.ledger[0].at_min == 100,
        "trigger=execute",
    );
    // 长按行为文档化。
    set.add(
        "F275 long press doc",
        LONG_PRESS_DOC.contains("软件关机") && LONG_PRESS_DOC.contains("兜底"),
        "documented",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f275_power_lanes() {
        let set = run_powermenu_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F275 自检红 {f}/{p}");
    }

    #[test]
    fn ledger_order_stable() {
        // 账目时序不可乱——先记先落（B-2902 先记账后动作）。
        let mut svc = PowerService::new();
        svc.trigger_from_startmenu(PowerAction::Reboot, 1);
        svc.trigger_from_startmenu(PowerAction::Sleep, 2);
        assert_eq!(svc.ledger[0].at_min, 1);
        assert_eq!(svc.ledger[1].at_min, 2);
    }
}
