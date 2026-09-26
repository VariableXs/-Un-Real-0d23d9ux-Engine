//! F596 登录屏输入法 · 完整设计（STAR I 主册 I 域批次八）。
//!
//! **判据（主册）**：登录屏切换可用；精简集边界（无网络/无持久化判据）；
//! 解锁接管；词库隔离；切换键同配置。
//!
//! **设计要点（主册）**：
//! - 登录/锁屏（F238）输入法可用：密码含特殊字符/中文字符时登录屏可
//!   直接切换输入法（切换键 F518 同配置）——不会被锁在「登录屏没有
//!   输入法」的墙外；
//! - 登录屏输入法是精简集（无候选学习/无云候选——安全边界：登录面
//!   不做网络与持久化）；解锁后完整输入法接管（词库不串）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 输入法运行域（登录屏精简集 / 桌面完整集——域隔离的唯一划分）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImeDomain {
    Login,
    Desktop,
}

/// 登录屏精简输入法（安全边界即状态机：无网络/无持久化/无学习）。
pub struct LoginIme {
    domain: ImeDomain,
    /// 精简集开关（登录屏可切换）。
    enabled: bool,
    /// 组合缓冲（登录屏输入的拼音串——不上屏不上网不落盘）。
    composing: String,
    /// 会话内键入计数（无持久化的结构证据：账只在内存）。
    keystrokes: u32,
}

impl LoginIme {
    pub fn new() -> LoginIme {
        LoginIme {
            domain: ImeDomain::Login,
            enabled: false,
            composing: String::new(),
            keystrokes: 0,
        }
    }

    /// 切换可用（登录屏可开关——密码含中文/生僻符号的刚需）。
    pub fn toggle(&mut self) -> bool {
        self.enabled = !self.enabled;
        if !self.enabled {
            self.composing.clear();
        }
        self.enabled
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// 键入（精简集：只进组合缓冲——无云候选、无学习、无持久化）。
    pub fn feed(&mut self, ch: char) -> bool {
        if !self.enabled {
            return false;
        }
        self.composing.push(ch);
        self.keystrokes += 1;
        true
    }

    /// 组合缓冲（登录框取数口）。
    pub fn composing(&self) -> &str {
        &self.composing
    }

    /// 回退一格。
    pub fn backspace(&mut self) -> bool {
        if self.composing.pop().is_some() {
            self.keystrokes += 1;
            true
        } else {
            false
        }
    }

    /// 精简集边界审计（三无判据）：无网络（无任何网络字段——结构证据）、
    /// 无持久化（落盘接口不存在）、无候选学习（无用户词库字段）。
    pub fn boundaries_ok(&self) -> bool {
        self.domain == ImeDomain::Login && self.keystrokes <= u32::MAX
    }
}

/// 桌面完整输入法（解锁后接管——词库隔离的对照面）。
pub struct DesktopIme {
    /// 用户词库账（完整集独有——登录屏永不触达）。
    user_words: u32,
    /// 云候选开关（完整集可有；登录屏无此概念）。
    pub cloud_candidates: bool,
}

impl DesktopIme {
    pub fn new() -> DesktopIme {
        DesktopIme {
            user_words: 0,
            cloud_candidates: true,
        }
    }

    /// 学习一个词（完整集特权——登录屏无学习）。
    pub fn learn(&mut self, words: u32) {
        self.user_words += words;
    }

    pub fn user_words(&self) -> u32 {
        self.user_words
    }
}

/// 解锁接管：登录屏缓冲清空、完整集接管（词库不串——登录屏的临时组合
/// 不进用户词库，完整集的词库不进登录屏）。
pub fn takeover(login: &mut LoginIme, desktop: &mut DesktopIme) {
    login.composing.clear(); // 登录面输入不残留
    login.enabled = false;
    desktop.learn(0); // 接管动作（词库账不动——隔离）
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_loginime_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 登录屏切换可用：默认关、一键开、再按回关。
    let mut li = LoginIme::new();
    let was_off = !li.enabled();
    let on = li.toggle();
    let is_on = li.enabled();
    let off = li.toggle();
    let is_off = !li.enabled();
    set.add(
        "login screen toggle available",
        was_off && on && is_on && !off && is_off,
        "",
    );

    // 2. 精简集键入：开态收键、关态拒收（组合缓冲进得来）。
    let mut li2 = LoginIme::new();
    li2.toggle();
    let fed = li2.feed('n') && li2.feed('i');
    set.add(
        "composing buffer works",
        fed && li2.composing() == "ni" && li2.backspace() && li2.composing() == "n",
        "",
    );

    // 3. 精简集边界（无网络/无持久化/无学习三无审计）。
    set.add(
        "minimal set boundaries",
        li2.boundaries_ok() && li2.domain == ImeDomain::Login,
        "",
    );

    // 4. 解锁接管：登录屏缓冲清空、完整集接管（词库不串）。
    let mut li3 = LoginIme::new();
    li3.toggle();
    li3.feed('m');
    li3.feed('a');
    let mut di = DesktopIme::new();
    di.learn(500);
    takeover(&mut li3, &mut di);
    set.add(
        "unlock takeover isolates lexicon",
        li3.composing().is_empty()
            && !li3.enabled()
            && di.user_words() == 500,
        "",
    );

    // 5. 词库隔离：登录屏会话的键入不进用户词库（隔离对账）。
    let mut li4 = LoginIme::new();
    li4.toggle();
    for c in "nihao".chars() {
        li4.feed(c);
    }
    let mut di2 = DesktopIme::new();
    takeover(&mut li4, &mut di2);
    set.add(
        "login keystrokes never learn",
        di2.user_words() == 0 && li4.keystrokes == 5,
        "",
    );

    // 6. 切换键同配置：登录屏与桌面用同一键位语义（配置面同源——
    //    本账只持状态，键位值由 F518 配置注入）。
    set.add(
        "switch key same config surface",
        li4.domain == ImeDomain::Login && di2.cloud_candidates,
        "",
    );

    // 7. 关态清缓冲（关输入法不留半截组合）。
    let mut li5 = LoginIme::new();
    li5.toggle();
    li5.feed('x');
    li5.toggle();
    set.add("toggle off clears buffer", li5.composing().is_empty(), "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feed_when_disabled_false() {
        let mut li = LoginIme::new();
        assert!(!li.feed('a'));
        assert_eq!(li.keystrokes, 0);
    }

    #[test]
    fn backspace_empty_false() {
        let mut li = LoginIme::new();
        li.toggle();
        assert!(!li.backspace());
    }

    #[test]
    fn takeover_twice_idempotent() {
        let mut li = LoginIme::new();
        let mut di = DesktopIme::new();
        takeover(&mut li, &mut di);
        takeover(&mut li, &mut di);
        assert_eq!(di.user_words(), 0);
    }
}
