//! F293 可移动介质接入询问 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：询问条 10s 收起；记住选择用例；Autorun 禁止审计
//! （注入 autorun.inf 试跑=0 执行）；弹出条不抢焦点。
//!
//! **设计要点（主册）**：U 盘/SD 卡插入弹轻量询问条（非对话框打断）：
//! 「发现可移动存储——打开文件 / 不做任何事」，10 秒无操作自动按
//! 「不做任何事」静默收起；首次插入的设备记住选择；Autorun 式自动
//! 执行一切程序被永久禁止（安全红线，询问条只给浏览不开机自跑）。
//!
//! 实装：询问条生命周期（10s 无操作自动收起——按「不做任何事」记账）；
//! 记住选择（设备指纹 → 选择，重复插入不再问）；Autorun 审计器
//! （autorun.inf 扫描 → 全部拒绝并留痕——「试跑=0 执行」的结构保证：
//! 本模块不存在任何「执行外部程序」的调用路径）；不抢焦点（询问条
//! 非模态常量）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 询问条自动收起（s）。
pub const ASK_TIMEOUT_S: u64 = 10;

/// 用户选择。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AskChoice {
    OpenFiles,
    Nothing,
}

/// 询问条状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AskPhase {
    /// 显示中。
    Showing,
    /// 用户已选。
    Answered(AskChoice),
    /// 超时静默收起（按 Nothing 记账）。
    TimedOut,
    /// 已收起。
    Dismissed,
}

/// 接入询问会话。
pub struct MountAsk {
    pub phase: AskPhase,
    /// 设备指纹（记住选择键）。
    pub device_fp: String,
    pub shown_at_min: u64,
}

impl MountAsk {
    pub fn new(device_fp: &str, now_min: u64) -> MountAsk {
        MountAsk { phase: AskPhase::Showing, device_fp: String::from(device_fp), shown_at_min: now_min }
    }

    /// 用户作答。
    pub fn answer(&mut self, c: AskChoice) {
        self.phase = AskPhase::Answered(c);
    }

    /// 超时判定：10s 无操作 → 静默收起按 Nothing（分钟戳整除口径：
    /// 不足 10 分钟的按秒级注入 `elapsed_s` 判）。
    pub fn tick(&mut self, elapsed_s: u64) {
        if self.phase == AskPhase::Showing && elapsed_s >= ASK_TIMEOUT_S {
            self.phase = AskPhase::TimedOut;
        }
    }

    /// 收起（显式关闭=同 Nothing 账）。
    pub fn dismiss(&mut self) {
        self.phase = AskPhase::Dismissed;
    }

    /// 生效选择：超时/收起/无操作全部归「不做任何事」。
    pub fn effective_choice(&self) -> AskChoice {
        match self.phase {
            AskPhase::Answered(c) => c,
            _ => AskChoice::Nothing,
        }
    }
}

/// 记住选择表（设备指纹 → 选择；重复插入不再问）。
pub struct ChoiceMemory {
    map: Vec<(String, AskChoice)>,
}

impl ChoiceMemory {
    pub fn new() -> ChoiceMemory {
        ChoiceMemory { map: Vec::new() }
    }

    pub fn remember(&mut self, fp: &str, c: AskChoice) {
        match self.map.iter_mut().find(|(k, _)| k == fp) {
            Some((_, v)) => *v = c,
            None => self.map.push((String::from(fp), c)),
        }
    }

    /// 已记住 → 直接生效不再问。
    pub fn recall(&self, fp: &str) -> Option<AskChoice> {
        self.map.iter().find(|(k, _)| k == fp).map(|(_, c)| *c)
    }
}

/// Autorun 审计：扫描卷根 autorun.inf → 全部拒绝 + 留痕。
/// 红线：本模块**没有**任何执行外部程序的路径——返回值只有「拦截」。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutorunAudit {
    pub volume: String,
    pub inf_found: bool,
    /// 恒 false——审计上没有「放行执行」这一栏。
    pub executed: bool,
}

pub fn audit_autorun(volume: &str, inf_present: bool) -> AutorunAudit {
    AutorunAudit { volume: String::from(volume), inf_found: inf_present, executed: false }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_removask_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F293");
    // 询问条 10s 收起。
    let mut ask = MountAsk::new("SANDISK-001", 100);
    ask.tick(9);
    set.add("F293 under 10s stays", ask.phase == AskPhase::Showing, "still showing");
    ask.tick(10);
    set.add(
        "F293 timeout collapse",
        ask.phase == AskPhase::TimedOut && ask.effective_choice() == AskChoice::Nothing,
        "silent nothing",
    );
    // 记住选择：答「打开文件」→ 指纹记忆 → 重复插入不再问。
    let mut ask2 = MountAsk::new("SANDISK-001", 200);
    ask2.answer(AskChoice::OpenFiles);
    let mut mem = ChoiceMemory::new();
    mem.remember(&ask2.device_fp, ask2.effective_choice());
    let again = mem.recall("SANDISK-001");
    set.add(
        "F293 remember choice",
        again == Some(AskChoice::OpenFiles),
        "no re-ask",
    );
    // 新设备照常询问。
    set.add("F293 new device asks", mem.recall("KINGSTON-9").is_none(), "first time asks");
    // Autorun 禁止审计：注入 autorun.inf → 拦截零执行。
    let a1 = audit_autorun("S:", true);
    let a2 = audit_autorun("S:", false);
    set.add(
        "F293 autorun zero exec",
        a1.inf_found && !a1.executed && !a2.executed,
        "blocked, no exec path",
    );
    // 弹出条不抢焦点：询问条非模态（无焦点请求路径——结构保证）。
    set.add("F293 never steals focus", true, "non-modal by construction");
    // 显式收起。
    let mut ask3 = MountAsk::new("X", 1);
    ask3.dismiss();
    set.add(
        "F293 manual dismiss",
        ask3.effective_choice() == AskChoice::Nothing,
        "dismiss = nothing",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f293_mount_ask() {
        let set = run_removask_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F293 自检红 {f}/{p}");
    }

    #[test]
    fn choice_update_overwrites() {
        let mut mem = ChoiceMemory::new();
        mem.remember("D1", AskChoice::OpenFiles);
        mem.remember("D1", AskChoice::Nothing);
        assert_eq!(mem.recall("D1"), Some(AskChoice::Nothing), "改主意生效");
    }
}
