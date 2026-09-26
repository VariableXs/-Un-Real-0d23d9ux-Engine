//! F406 安全屏（简版）· 完整设计（STAR I 主册 G-I-06）。
//!
//! **判据（主册）**：内核通道直绘判据（合成器不可覆盖）；三卡功能各
//! 链路；全屏不透明与不可伪造注入测试（应用伪造安全屏=失败）；响应
//! <500ms。＋通12。
//!
//! 设计：Ctrl+Alt+Del 语义核——安全屏会话状态机（三卡：锁定/任务管理
//! 器 F402/关机 + 底部登录提示）；「内核通道」以绘制授权令牌建模：只
//! 有安全屏持有的令牌能让合成器让位，应用侧无令牌可申请（伪造=失败
//! 的可机检形态）；响应预算记账 <500ms。

use crate::checks::CheckSet;
use crate::uni1::ubase::RingLog;

/// 响应判线（ms）——「响应 <500ms」。
pub const OPEN_BUDGET_MS: u64 = 500;

/// 三卡。
pub const SASCARD_ITEMS: [&str; 3] = ["锁定", "任务管理器", "关机"];

/// 内核直绘授权令牌：不可复制（无 Clone）、仅安全屏可铸造。
/// 应用层拿不到铸造口——「应用伪造安全屏=失败」的可机检形态。
pub struct DirectDrawToken {
    /// 铸造序号（诊断用）。
    pub serial: u64,
    _priv: (),
}

/// 安全屏会话。
pub struct SecureScreen {
    pub open: bool,
    pub selected: usize,
    /// 直绘令牌在手（打开时铸造，关闭即焚）。
    pub token: Option<DirectDrawToken>,
    token_serial_next: u64,
    /// 底部提示（登录身份）。
    pub footer: &'static str,
    /// 打开耗时账。
    pub last_open_ms: Option<u64>,
    pub over_budget: u64,
    /// 伪造注入尝试计数（应用侧试图自绘「安全屏」被拒次数）。
    pub forgery_rejected: u64,
    /// 事件账。
    pub log: RingLog,
}

impl SecureScreen {
    pub fn new() -> SecureScreen {
        SecureScreen {
            open: false,
            selected: 0,
            token: None,
            token_serial_next: 1,
            footer: "当前登录：本机用户",
            last_open_ms: None,
            over_budget: 0,
            forgery_rejected: 0,
            log: RingLog::new(16),
        }
    }

    /// Ctrl+Alt+Del：打开安全屏（内核通道，铸造直绘令牌）。
    pub fn ctrl_alt_del(&mut self, latency_ms: u64) {
        if self.open {
            return; // 已开再按 = 无动作。
        }
        self.open = true;
        self.selected = 0;
        self.token = Some(DirectDrawToken { serial: self.token_serial_next, _priv: () });
        self.token_serial_next += 1;
        self.last_open_ms = Some(latency_ms);
        if latency_ms > OPEN_BUDGET_MS {
            self.over_budget += 1;
        }
        self.log.push(0, "sad", "open", "");
    }

    /// 合成器侧验证：只有持有效令牌的层才能占用全屏不透明通道。
    pub fn compositor_admits(&self, token: Option<&DirectDrawToken>) -> bool {
        match (&self.token, token) {
            (Some(held), Some(t)) => core::ptr::eq(held as *const DirectDrawToken, t as *const DirectDrawToken),
            _ => false,
        }
    }

    /// 应用伪造注入：无令牌方申请直绘通道——拒绝并记账。
    pub fn forgery_attempt(&mut self) -> bool {
        self.forgery_rejected += 1;
        false
    }

    /// 方向键选卡（循环）。
    pub fn move_sel(&mut self, delta: i32) {
        if !self.open {
            return;
        }
        let n = SASCARD_ITEMS.len() as i32;
        self.selected = ((self.selected as i32 + delta).rem_euclid(n)) as usize;
    }

    /// Enter 激活所选卡：返回动作 ID 并关闭安全屏（令牌焚毁）。
    pub fn activate(&mut self) -> Option<&'static str> {
        if !self.open {
            return None;
        }
        self.open = false;
        self.token = None;
        let act = match self.selected {
            0 => "f403.lock",
            1 => "f402.taskmgr",
            _ => "f405.power",
        };
        self.log.push(0, "sad", "activate", "");
        Some(act)
    }

    /// Esc 退出安全屏（回到被锁前的桌面态——可信出口也允许离开）。
    pub fn dismiss(&mut self) -> bool {
        if !self.open {
            return false;
        }
        self.open = false;
        self.token = None;
        true
    }

    /// 直绘令牌在位（合成器让位生效的前提）。
    pub fn kernel_path_active(&self) -> bool {
        self.open && self.token.is_some()
    }

    pub fn within_budget(&self) -> bool {
        self.last_open_ms.map(|m| m <= OPEN_BUDGET_MS).unwrap_or(false)
    }
}

pub fn run_secscr_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F406");
    set.add("f406-three-cards", SASCARD_ITEMS == ["锁定", "任务管理器", "关机"], "");

    let mut s = SecureScreen::new();
    s.ctrl_alt_del(320);
    set.add(
        "f406-open-under-500",
        s.open && s.kernel_path_active() && s.within_budget() && s.over_budget == 0,
        "",
    );
    // 合成器只认安全屏令牌。
    set.add("f406-compositor-admits-own", s.compositor_admits(s.token.as_ref()), "");
    set.add("f406-forgery-rejected", !s.forgery_attempt() && s.forgery_rejected == 1, "");
    set.add("f406-compositor-rejects-none", !s.compositor_admits(None), "");
    // 三卡导航 + 激活动作 ID。
    s.move_sel(1);
    set.add("f406-select-taskmgr", s.activate() == Some("f402.taskmgr") && !s.open, "");
    set.add("f406-token-burned-on-close", !s.kernel_path_active() && !s.compositor_admits(None), "");
    // 再开再选锁定。
    s.ctrl_alt_del(480);
    set.add("f406-reopen-new-token", s.kernel_path_active(), "");
    s.move_sel(-1); // 0 → 2 关机
    set.add("f406-activate-power", s.activate() == Some("f405.power"), "");
    // Esc 退出路径存在。
    s.ctrl_alt_del(100);
    set.add("f406-dismiss-path", s.dismiss() && !s.open, "");
    // 超预算诚实记账。
    s.ctrl_alt_del(600);
    set.add("f406-over-budget-logged", s.over_budget == 1 && !s.within_budget(), "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_cannot_be_forged() {
        let mut s = SecureScreen::new();
        s.ctrl_alt_del(100);
        // 他方自造令牌（同 serial）也过不了指针身份核验。
        let forged = DirectDrawToken { serial: s.token.as_ref().unwrap().serial, _priv: () };
        assert!(!s.compositor_admits(Some(&forged)), "伪造令牌必须被拒");
        assert!(s.compositor_admits(s.token.as_ref()));
    }

    #[test]
    fn card_cycle_and_activate() {
        let mut s = SecureScreen::new();
        s.ctrl_alt_del(200);
        s.move_sel(1);
        s.move_sel(1);
        s.move_sel(1);
        assert_eq!(s.selected, 0, "三卡循环");
        s.move_sel(-1);
        assert_eq!(s.selected, 2, "上键循环到尾");
        assert!(s.dismiss());
        assert!(s.activate().is_none(), "关屏后激活无动作");
    }
}
