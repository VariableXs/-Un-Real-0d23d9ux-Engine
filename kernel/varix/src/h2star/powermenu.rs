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

// ---------------------------------------------------------------------------
// 深化批次二：关机执行链状态机（B-2902 分段账）
// ---------------------------------------------------------------------------

/// 关机链段位（B-2902「软件关机完整链」的分段口径——每段留痕，
/// 断在哪段账上就写哪段：断电演练（F180）按段对账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainPhase {
    /// 段一：动作请求已受理（账目先行）。
    Requested,
    /// 段二：准备（保存点抢救 F284 联动、后台任务收口 F369 询问）。
    Preparing,
    /// 段三：提交（不可撤销点——此后断电也走完整账）。
    Committed,
    /// 段四：完成（断电前最后一条账）。
    Done,
}

/// 一条执行链账（动作 + 段位 + 时刻）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChainEntry {
    pub action: PowerAction,
    pub phase: ChainPhase,
    pub at_s: u64,
}

/// 关机执行链（每次动作一条链——四段全绿才算「完整链」）。
#[derive(Default)]
pub struct ShutdownChain {
    pub entries: Vec<ChainEntry>,
    aborted: bool,
}

impl ShutdownChain {
    pub fn new() -> ShutdownChain {
        ShutdownChain { entries: Vec::new(), aborted: false }
    }

    /// 推进段位（顺序强制：只许走下一段，不许跳段——B-2902 完整
    /// 链的机判：跳段=链断，账目不认）。
    pub fn advance(&mut self, action: PowerAction, to: ChainPhase, at_s: u64) -> bool {
        if self.aborted {
            return false;
        }
        let expect = match self.entries.last().map(|e| e.phase) {
            None => ChainPhase::Requested,
            Some(last) => match last {
                ChainPhase::Requested => ChainPhase::Preparing,
                ChainPhase::Preparing => ChainPhase::Committed,
                ChainPhase::Committed => ChainPhase::Done,
                ChainPhase::Done => return false,
            },
        };
        if to != expect {
            return false;
        }
        self.entries.push(ChainEntry { action, phase: to, at_s });
        true
    }

    /// 提交前撤销（Preparing 段可退——「取消」永远是安全出路）。
    pub fn abort_before_commit(&mut self) -> bool {
        let ok = self
            .entries
            .last()
            .map(|e| e.phase == ChainPhase::Requested || e.phase == ChainPhase::Preparing)
            .unwrap_or(false);
        if ok {
            self.aborted = true;
        }
        ok
    }

    /// 完整链判定（四段齐=完整；账目断在半程=断电演练的对账输入）。
    pub fn complete(&self) -> bool {
        !self.aborted
            && self.entries.len() == 4
            && self.entries.iter().enumerate().all(|(i, e)| {
                e.phase
                    == [ChainPhase::Requested, ChainPhase::Preparing, ChainPhase::Committed, ChainPhase::Done][i]
            })
    }

    pub fn is_aborted(&self) -> bool {
        self.aborted
    }
}

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

    /// 双入口竞态防线：同一动作在 `window_min` 分钟内先后触发（双击+
    /// 快捷键、或两路入口同时点）→ 第二笔拒绝（只执行一次，防重复
    /// 关机请求——幂等闸；分钟戳粒度与账目一致）。
    pub fn trigger_deduped(&mut self, action: PowerAction, source: &'static str, at_min: u64, window_min: u64) -> bool {
        if let Some(last) = self.ledger.iter().rev().find(|e| e.action == action) {
            if at_min.saturating_sub(last.at_min) < window_min {
                return false;
            }
        }
        self.exec(action, source, at_min);
        true
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
    // --- 深化二：关机执行链四段状态机。 ---
    let mut chain = ShutdownChain::new();
    set.add(
        "F275 chain skip rejected",
        !chain.advance(PowerAction::Shutdown, ChainPhase::Committed, 1),
        "no jumping phases",
    );
    let four = chain.advance(PowerAction::Shutdown, ChainPhase::Requested, 1)
        && chain.advance(PowerAction::Shutdown, ChainPhase::Preparing, 2)
        && chain.advance(PowerAction::Shutdown, ChainPhase::Committed, 3)
        && chain.advance(PowerAction::Shutdown, ChainPhase::Done, 4);
    set.add("F275 chain four phases", four && chain.complete(), "B-2902 full chain");
    set.add("F275 chain locked after done", !chain.advance(PowerAction::Shutdown, ChainPhase::Done, 5), "terminal");
    // 提交前可退（取消是安全出路）；提交后不可退。
    let mut c2 = ShutdownChain::new();
    let _ = c2.advance(PowerAction::Reboot, ChainPhase::Requested, 1);
    let abortable = c2.abort_before_commit();
    set.add("F275 abort before commit", abortable && c2.is_aborted() && !c2.complete(), "cancel safe");
    let mut c3 = ShutdownChain::new();
    let _ = c3.advance(PowerAction::Sleep, ChainPhase::Requested, 1);
    let _ = c3.advance(PowerAction::Sleep, ChainPhase::Preparing, 2);
    let _ = c3.advance(PowerAction::Sleep, ChainPhase::Committed, 3);
    set.add("F275 abort after commit", !c3.abort_before_commit(), "committed is final");
    // --- 深化二：双入口幂等闸（窗口内重复触发拒绝）。 ---
    let mut svc2 = PowerService::new();
    let first = svc2.trigger_deduped(PowerAction::Shutdown, "开始菜单", 100, 5);
    let dupe = svc2.trigger_deduped(PowerAction::Shutdown, "桌面电源键", 102, 5);
    let later = svc2.trigger_deduped(PowerAction::Shutdown, "开始菜单", 106, 5);
    set.add(
        "F275 dedup idempotent",
        first && !dupe && later && svc2.ledger.len() == 2,
        "one request one run",
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
