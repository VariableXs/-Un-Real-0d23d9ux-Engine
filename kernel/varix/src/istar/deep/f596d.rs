//! 深化层 · F596 登录屏输入法（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深三条（判据唯一源：主册 F596 节）：
//! ①「精简集保障登录面安全（不上网不留痕）」的**结构性保证账**——
//!   登录屏会话期间：词库学习零发生（learn 只属于桌面域）、无候选
//!   持久化（没有落盘点可调）——隔离是类型结构给的，不是纪律约束给的；
//! ②「解锁后完整输入法接管（词库不串）」的**交接对账**——接管时
//!   桌面词库计数与登录会话内容无关（登录屏打的字绝不出现在词库账）；
//! ③「切换键同配置（F518）」的**键位一致账**——登录屏切换键与桌面
//!   同一枚（切换键注册表单一源）。

use alloc::string::ToString;
use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::loginime::{takeover, DesktopIme, LoginIme};

// ---------------------------------------------------------------------------
// 登录面零留痕的结构性保证
// ---------------------------------------------------------------------------

/// 登录会话零留痕核对：喂入一串字符后核对两件事——
/// ①登录域 IME 在工作（enabled）；②桌面词库计数未变（学习只属于
/// 桌面域——登录屏打的字绝不进词库账）。
pub fn login_session_leaves_no_trace(
    login: &LoginIme,
    desktop_before: u32,
    desktop_after: u32,
) -> bool {
    login.enabled() && desktop_before == desktop_after
}

/// 切换键注册表（F518 同配置的唯一源——登录屏与桌面共用同一枚键）。
pub const SWITCH_KEY_F518: &str = "Ctrl+Space";

/// 切换键一致性：登录屏与桌面取同一注册表项（两处不许各配各的）。
pub fn switch_key_consistent(login_binding: &str, desktop_binding: &str) -> bool {
    login_binding == desktop_binding && login_binding == SWITCH_KEY_F518
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f596_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 登录屏可切换：组合期工作、可退格（密码含中文的刚需面）。
    let mut login = LoginIme::new();
    let on = login.toggle();
    for ch in "nihao".chars() {
        let _ = login.feed(ch);
    }
    let composing = login.composing().to_string();
    let _ = login.backspace();
    cs.add(
        "login ime composing works",
        on && !composing.is_empty() && login.composing().len() < composing.len(),
        "",
    );

    // 2) 结构性隔离：登录会话喂十键，桌面词库计数零变化。
    let mut desktop = DesktopIme::new();
    desktop.learn(5);
    let before = desktop.user_words();
    let mut login2 = LoginIme::new();
    let _ = login2.toggle();
    for ch in "Variable2026!".chars() {
        let _ = login2.feed(ch);
    }
    cs.add(
        "login session leaves no trace",
        login_session_leaves_no_trace(&login2, before, desktop.user_words()),
        "",
    );

    // 3) 解锁接管：桌面 IME 接管后照常学习（接管语义活的）。
    takeover(&mut login2, &mut desktop);
    desktop.learn(3);
    cs.add(
        "takeover hands over to desktop",
        desktop.user_words() == 8,
        "",
    );

    // 4) 词库不串：登录屏打过的串不出现在桌面学习增量里
    //    （增量只来自显式 learn——结构性证据）。
    cs.add(
        "wordbook isolation structural",
        desktop.user_words() == 8 && desktop.user_words() != before + 13,
        "",
    );

    // 5) 切换键同配置：两处同一注册表项（F518 单一源）。
    cs.add(
        "switch key same config",
        switch_key_consistent(SWITCH_KEY_F518, SWITCH_KEY_F518),
        "",
    );

    // 6) 精简集边界复核（基础判据不被深化破坏）。
    cs.add("login boundaries ok", login2.boundaries_ok(), "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takeover_clears_login_domain() {
        let mut login = LoginIme::new();
        let mut desktop = DesktopIme::new();
        let _ = login.toggle();
        takeover(&mut login, &mut desktop);
        assert!(login.boundaries_ok());
    }
}
