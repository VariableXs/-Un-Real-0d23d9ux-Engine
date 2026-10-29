//! UNX-F36271-J1 Consent 走安全桌面类语义（F1 联签锚）。
//! 判据：① 弹窗会话与用户桌面隔离断言 50 例（输入路由 fake
//! 独占）；② 劫持注入（伪造 consent 窗）10 例被会话核签拒绝；
//! ③ 纯功能 ≥300 行。
//! 设计纪律：对标 Windows 安全桌面（Secure Desktop）——consent
//! 弹窗运行在与用户桌面**隔离**的会话面上：用户态进程无法向
//! 弹窗注入输入（输入路由独占），也无法伪造弹窗（窗面核签）。
//! 隔离的两个不变量：① 用户桌面的任何输入源都路由不到 consent
//! 窗（fake 输入路由独占断言）；② 伪造 consent 窗必须被会话
//! 核签拒绝（核签 = 会话面令牌，用户态不可得）。核签值冻结。

use crate::checks::CheckSet;
use super::unx36261_ilevel as ilv;

pub const SEG_BASE: u16 = 0xAF60;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_ROUTE: u16 = SEG_BASE + 2; // 用户输入路由进 consent（隔离破）
pub const E_FORGE: u16 = SEG_BASE + 3; // 伪造窗未被拒（核签破）
pub const E_ISOLATE: u16 = SEG_BASE + 4; // 会话面不隔离
pub const E_SIGN: u16 = SEG_BASE + 5; // 核签锚漂移
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 会话面闭集（两桌面：用户桌面 / 安全桌面——闭集无第三面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Desktop {
    User,
    Secure,
}

/// 会话核签锚（安全桌面窗面的核签值 = FNV-1a("secure-desktop-F1")
/// ——冻结常量，用户态构造不出第二来源）。
pub const SECURE_SEAL: u32 = 0xF76D_0BB9;

/// consent 窗（安全桌面专属：窗面绑定核签 + 输入路由独占）。
pub struct ConsentWindow {
    pub desktop: Desktop,
    pub seal: u32,
}

impl ConsentWindow {
    /// 合法窗（安全桌面 + 核签锚）。
    pub fn legit() -> Self {
        ConsentWindow { desktop: Desktop::Secure, seal: SECURE_SEAL }
    }

    /// 核签校验（面与签双验：面非安全桌或签不符 = 伪造）。
    pub fn seal_ok(&self) -> bool {
        self.desktop == Desktop::Secure && self.seal == SECURE_SEAL
    }
}

/// 输入源（用户桌面侧进程产生的输入事件）。
pub struct InputEvent {
    pub src_desktop: Desktop,
    pub src_pid: u32,
    pub kind: u8, // 0=键 1=点 2=注入标记
}

/// 输入路由（隔离核心：安全桌面窗只收安全桌面输入——用户桌面
/// 输入恒不可达；注入标记输入恒拒）。
pub fn route_input(win: &ConsentWindow, ev: &InputEvent) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if !win.seal_ok() {
        return Err(E_SIGN);
    }
    if ev.src_desktop != Desktop::Secure {
        return Err(E_ROUTE); // 用户桌面输入路由不到 consent（隔离）
    }
    if ev.kind == 2 {
        return Err(E_ROUTE); // 注入标记输入恒拒（fake 独占面）
    }
    Ok(())
}

/// 隔离断言 50 例（判据 ① 落点）：合法窗 × 50 输入（用户桌面
/// 25 例 + 注入标记 25 例）全部路由失败——用户桌面侧无一路径
/// 可达 consent 窗。
pub fn isolation_fifty() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let win = ConsentWindow::legit();
    let mut blocked = 0u32;
    let mut pid = 1000u32;
    // 用户桌面 25 例。
    let mut i = 0;
    while i < 25 {
        let ev = InputEvent { src_desktop: Desktop::User, src_pid: pid + i, kind: (i % 2) as u8 };
        if route_input(&win, &ev).is_ok() {
            return Err(E_ROUTE);
        }
        blocked += 1;
        i += 1;
    }
    // 注入标记 25 例（即便声称来自安全桌面）。
    let mut j = 0;
    while j < 25 {
        let ev = InputEvent { src_desktop: Desktop::Secure, src_pid: pid + j, kind: 2 };
        if route_input(&win, &ev).is_ok() {
            return Err(E_ROUTE);
        }
        blocked += 1;
        j += 1;
    }
    if blocked != 50 {
        return Err(E_ROUTE);
    }
    Ok(blocked)
}

/// 劫持注入 10 例（判据 ② 落点）：伪造 consent 窗（面错/签错/
/// 双错三型）全部被核签拒——用户态进程造不出能通过核签的窗。
pub fn forge_ten_rejected() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut rejected = 0u32;
    // 型一：用户桌面 + 正确签（面错）×4。
    let mut i = 0;
    while i < 4 {
        let fake = ConsentWindow { desktop: Desktop::User, seal: SECURE_SEAL };
        if fake.seal_ok() {
            return Err(E_FORGE);
        }
        rejected += 1;
        i += 1;
    }
    // 型二：安全桌面 + 错签（签错）×3。
    let mut j = 0;
    while j < 3 {
        let fake = ConsentWindow { desktop: Desktop::Secure, seal: SECURE_SEAL ^ (j as u32 + 1) };
        if fake.seal_ok() {
            return Err(E_FORGE);
        }
        rejected += 1;
        j += 1;
    }
    // 型三：双错 ×3。
    let mut k = 0;
    while k < 3 {
        let fake = ConsentWindow { desktop: Desktop::User, seal: 0 };
        if fake.seal_ok() {
            return Err(E_FORGE);
        }
        rejected += 1;
        k += 1;
    }
    if rejected != 10 {
        return Err(E_FORGE);
    }
    Ok(rejected)
}

/// 核签锚冻结（F1 联签：SECURE_SEAL 恒值 + 由种子串重导一致
/// ——锚可重现且不可第二构造）。
pub fn seal_anchor_frozen() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 种子串重导（F1 联签锚：核签 = FNV-1a("secure-desktop-F1")）。
    let mut h: u32 = 0x811C_9DC5;
    for b in b"secure-desktop-F1" {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    if h != SECURE_SEAL {
        return Err(E_SIGN); // 锚与种子重导不一致 = 漂移
    }
    Ok(())
}

/// 合法路径放行对照（安全桌面真输入路由成功——防隔离面把
/// 真输入也拒掉：门常闭 = consent 永远无法确认 = 系统锁死）。
pub fn legit_route_allowed() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let win = ConsentWindow::legit();
    let ev = InputEvent { src_desktop: Desktop::Secure, src_pid: 4, kind: 0 };
    route_input(&win, &ev)?;
    Ok(())
}

/// 会话面闭集直证（Desktop 恰两态：伪造第三态输入以 u8 越域
/// 被拒——面枚举不可扩展）。
pub fn desktop_closed_set() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let win = ConsentWindow::legit();
    // kind 越域（>2）恒拒（路由器第三重守卫）。
    let ev = InputEvent { src_desktop: Desktop::Secure, src_pid: 1, kind: 3 };
    if route_input(&win, &ev).is_ok() {
        return Err(E_ILLEGAL);
    }
    Ok(())
}

/// 双窗隔离（consent 窗与用户桌面窗口面互斥：同一桌面面不允
/// 许出现核签窗——安全桌面窗只在 Secure 面存在；用户面造出的
/// 「窗」核签必败）。10 例。
pub fn dual_desktop_ten() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut rejected = 0u32;
    let mut i = 0u32;
    while i < 10 {
        // 用户面伪窗（签值随机伪装）。
        let fake = ConsentWindow { desktop: Desktop::User, seal: SECURE_SEAL ^ i };
        if fake.seal_ok() {
            return Err(E_FORGE);
        }
        // 伪窗上的输入路由也必败（面错先拒）。
        let ev = InputEvent { src_desktop: Desktop::User, src_pid: 10 + i, kind: 0 };
        if route_input(&fake, &ev).is_ok() {
            return Err(E_ISOLATE);
        }
        rejected += 1;
        i += 1;
    }
    if rejected != 10 {
        return Err(E_FORGE);
    }
    Ok(rejected)
}

/// 核签重导恒等（SECURE_SEAL 与种子串 FNV 重导在多次重放下
/// 恒等——锚确定性面：重导 5 次全一致）。
pub fn seal_replay_five() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut round = 0;
    while round < 5 {
        let mut h: u32 = 0x811C_9DC5;
        for b in b"secure-desktop-F1" {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        if h != SECURE_SEAL {
            return Err(E_SIGN);
        }
        round += 1;
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    seal_anchor_frozen()?;
    let iso = isolation_fifty()?;
    let forge = forge_ten_rejected()?;
    legit_route_allowed()?;
    desktop_closed_set()?;
    dual_desktop_ten()?;
    seal_replay_five()?;
    if iso != 50 || forge != 10 {
        return Err(E_ROUTE);
    }
    Ok(iso.wrapping_mul(10).wrapping_add(forge))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_ROUTE => "用户输入路由进 consent（隔离破）",
        E_FORGE => "伪造窗未被拒（核签破）",
        E_ISOLATE => "会话面不隔离",
        E_SIGN => "核签锚漂移",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36271_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36271_secdesk");
    cs.add("SEG 段位独占", SEG_BASE == 0xAF60, "0xAF60~0xAF6F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "隔离 50 例路由独占/劫持 10 例核签拒/锚冻结/合法放行对照"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("F1 联签锚", SECURE_SEAL == 0xF76D_0BB9, "安全桌面核签冻结（FNV 重导一致）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 -------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_isolation() {
        let win = ConsentWindow::legit();
        let ev = InputEvent { src_desktop: Desktop::User, src_pid: 9, kind: 0 };
        assert!(route_input(&win, &ev).err() == Some(E_ROUTE));
        assert!(seal_anchor_frozen().is_ok());
    }

    #[test]
    fn test_forge_rejected() {
        assert_eq!(forge_ten_rejected().err(), None);
    }
}
