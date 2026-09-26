//! F193 安全模式（secstar2 · G-G-23）——系统永远给自己留一条能走进去的门。
//!
//! **判据（主册）**：进入-修复-退出全链录屏；最小集白名单外功能全部灰置
//! 且可解释；连续异常关机询问触发实测。
//!
//! **功能定义（主册 G-G-23）**：救援模式：禁第三方驱动/默认主题（E1 旁路）/
//! 最小服务集/低分辨率保底——从 Limine 图形选单（F171）第三枚隐藏条目
//! （按住 Shift 点 VARIX 或菜单底部小字提示）进入；进入与退出路径都文档化。
//!
//! 【交互设计】安全模式桌面：右下角常驻黄条「安全模式 · 有限功能」（不可关
//! ——模式标识就是身份）；功能面最小集（设置中心/资源管理器/卸载通道/
//! 诊断中心）；退出=正常重启即出（无残留）；进入提示条附「为什么我在安全
//! 模式」帮助链。
//! 【数据与存储】模式标记内核参数（F192 降级族 safe-mode——一处一事实）；
//! 无额外持久态。
//! 【状态与异常】连续两次异常关机 → 下次启动自动询问「进入安全模式？」
//! （Windows 惯例语义——防循环崩）；安全模式下更新禁用（防半态更新）。
//! 【设计细节】黄条文案含原因（「检测到连续异常关机，已建议安全模式」——
//! 每次进入都有理由）；最小集白名单 12 项固定（清单公开 F126）；低分辨率
//! 保底=1024×768 GOP 直绘（合成器降级路径——F056 脏区机制旁路）；隐藏
//! 条目发现性：选单底部 10px 小字「按住 Shift 点击 VARIX 进入安全模式」。
//!
//! 依赖锚点：F056（合成器旁路）、F126（清单公开）、F171（选单）、F192（参数驱动）。

use crate::checks::CheckSet;
use crate::star::sbase::RingLog;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 黄条常驻文案（模式标识就是身份——不可关）。
pub const BANNER_TEXT: &str = "安全模式 · 有限功能";

/// 黄条原因文案（连续异常关机路径——每次进入都有理由）。
pub const BANNER_REASON_ABNORMAL: &str = "检测到连续异常关机，已建议安全模式";

/// 选单底部小字（发现性——可发现但不打扰）。
pub const MENU_HINT_TEXT: &str = "按住 Shift 点击 VARIX 进入安全模式";
/// 小字字号：10px。
pub const MENU_HINT_PX: u32 = 10;

/// 低分辨率保底（GOP 直绘）。
pub const SAFE_RES_W: u32 = 1024;
pub const SAFE_RES_H: u32 = 768;

/// 连续异常关机询问阈值（Windows 惯例——两次）。
pub const ABNORMAL_STRIKES: u32 = 2;

/// 帮助链 ID（「为什么我在安全模式」）。
pub const HELP_LINK: &str = "help:why-safe-mode";

/// 更新禁用文案（防半态更新——诚实说明）。
pub const UPDATE_BLOCKED_TEXT: &str = "安全模式下更新已停用（防止更新到一半的状态）";

/// 最小集白名单（12 项固定——清单公开 F126；白名单外全部灰置且可解释）。
pub const MIN_SET: [&str; 12] = [
    "settings",      // 设置中心
    "explorer",      // 资源管理器
    "uninstaller",   // 卸载通道
    "diagnostics",   // 诊断中心
    "terminal",      // 终端（修复动线需要）
    "recovery",      // 恢复环境入口（F198 链）
    "taskbar",       // 任务栏（桌面骨架）
    "window-mgr",    // 窗口管理（合成器降级路径）
    "theme-default", // 默认主题（E1 旁路——仅默认令牌）
    "ime-base",      // 基础输入（中文修复场景可用）
    "clipboard",     // 剪贴板（取证搬运）
    "log-export",    // 日志导出（求助材料）
];

// ---------------------------------------------------------------------------
// 状态机
// ---------------------------------------------------------------------------

/// 安全模式来源（每次进入都有理由——黄条文案随来源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryReason {
    /// 用户从选单隐藏条目进入（Shift+点击）。
    ManualMenu,
    /// 连续异常关机自动询问后进入。
    AfterAbnormal,
    /// 内核参数直进（F192 safe-mode 旗标——一处一事实）。
    KernelParam,
}

impl EntryReason {
    /// 黄条原因行（ManualMenu 无额外原因——用户自己选的）。
    pub fn reason_text(self) -> Option<&'static str> {
        match self {
            EntryReason::ManualMenu => None,
            EntryReason::AfterAbnormal => Some(BANNER_REASON_ABNORMAL),
            EntryReason::KernelParam => Some("本次启动携带 safe-mode 内核参数"),
        }
    }
}

/// 功能可用性判定（灰置且可解释——白名单外不是消失是灰置）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeatureGate {
    /// 功能 ID。
    pub feature: &'static str,
    /// 安全模式下可用。
    pub allowed: bool,
    /// 灰置原因（allowed=true 时为空串）。
    pub why: &'static str,
}

/// 安全模式状态机。
pub struct SafeMode {
    /// 模式激活。
    pub active: bool,
    /// 进入原因。
    pub reason: Option<EntryReason>,
    /// 连续异常关机计数（跨启动持久——由快照/配置层承载，本层记账）。
    pub abnormal_strikes: u32,
    /// 询问已呈现（下次启动询问「进入安全模式？」——只问一次）。
    pub ask_pending: bool,
    /// 本次启动异常关机登记（drain 语义——登记后由持久层落盘）。
    abnormal_log: RingLog<u64, 8>,
}

impl SafeMode {
    pub fn new() -> SafeMode {
        SafeMode {
            active: false,
            reason: None,
            abnormal_strikes: 0,
            ask_pending: false,
            abnormal_log: RingLog::new(),
        }
    }

    /// **进入**（判据「进入-修复-退出全链」第一步）。
    /// `param_present`：本次启动是否携带 F192 safe-mode 参数（一处一事实——
    /// 模式标记唯一源是内核参数，UI 层选择最终也落到重启带参）。
    pub fn enter(&mut self, reason: EntryReason, param_present: bool) -> Result<(), &'static str> {
        if !param_present {
            return Err("safe-mode 未见于内核参数（模式标记唯一源是 F192 降级族）");
        }
        self.active = true;
        self.reason = Some(reason);
        Ok(())
    }

    /// **异常关机登记**：连击计数；达阈值 → 下次启动询问置位。
    pub fn note_abnormal_shutdown(&mut self, stamp: u64) {
        self.abnormal_log.push(stamp);
        self.abnormal_strikes += 1;
        if self.abnormal_strikes >= ABNORMAL_STRIKES {
            self.ask_pending = true;
        }
    }

    /// 正常关机（清计数——只有连续异常才触发询问）。
    pub fn note_clean_shutdown(&mut self) {
        self.abnormal_strikes = 0;
        self.ask_pending = false;
    }

    /// 下次启动询问判定（「进入安全模式？」弹层——只问一次）。
    pub fn should_ask_next_boot(&self) -> bool {
        self.ask_pending
    }

    /// 询问被用户接受 → 下一跳带参重启（返回要写入引导参数的旗标）。
    pub fn accept_ask(&mut self) -> &'static str {
        self.ask_pending = false;
        "safe-mode"
    }

    /// 询问被拒绝（用户选正常启动——清态不纠缠）。
    pub fn decline_ask(&mut self) {
        self.ask_pending = false;
        self.abnormal_strikes = 0;
    }

    /// **功能门**（判据「白名单外灰置且可解释」）。
    pub fn gate(&self, feature: &'static str) -> FeatureGate {
        if !self.active {
            return FeatureGate { feature, allowed: true, why: "" };
        }
        if MIN_SET.contains(&feature) {
            FeatureGate { feature, allowed: true, why: "" }
        } else {
            FeatureGate {
                feature,
                allowed: false,
                why: "安全模式最小集之外（12 项白名单见设置-关于）",
            }
        }
    }

    /// 更新通道总闸（安全模式下更新禁用——防半态更新）。
    pub fn update_allowed(&self) -> Result<(), &'static str> {
        if self.active {
            Err(UPDATE_BLOCKED_TEXT)
        } else {
            Ok(())
        }
    }

    /// 低分辨率保底判定（合成器降级路径——F056 脏区机制旁路）。
    pub fn display_fallback(&self) -> (u32, u32, bool) {
        if self.active {
            (SAFE_RES_W, SAFE_RES_H, true) // true=GOP 直绘
        } else {
            (0, 0, false)
        }
    }

    /// **退出**（正常重启即出——无残留）：清运行态；参数由重启动作自然
    /// 消失（无持久标记——【数据与存储】无额外持久态）。
    pub fn exit_via_reboot(&mut self) {
        self.active = false;
        self.reason = None;
        self.abnormal_strikes = 0;
        self.ask_pending = false;
    }

    /// 黄条完整文案（主行+原因行+帮助链）。
    pub fn banner(&self) -> (&'static str, Option<&'static str>, &'static str) {
        (BANNER_TEXT, self.reason.and_then(|r| r.reason_text()), HELP_LINK)
    }
}

impl Default for SafeMode {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F193 自检（聚合进 secstar2 域）。
pub fn run_safemode_checks() -> CheckSet {
    let mut set = CheckSet::new("F193-safemode");

    // 白名单 12 项固定。
    set.add("min set 12", MIN_SET.len() == 12, "");
    set.add("min set no dup", {
        let mut dup = false;
        for i in 0..MIN_SET.len() {
            for j in (i + 1)..MIN_SET.len() {
                if MIN_SET[i] == MIN_SET[j] {
                    dup = true;
                }
            }
        }
        !dup
    }, "");

    // 进入：参数驱动（无参拒绝——模式标记唯一源）。
    let mut sm = SafeMode::new();
    set.add("enter needs param", sm.enter(EntryReason::ManualMenu, false).is_err(), "");
    set.add("enter ok", sm.enter(EntryReason::ManualMenu, true).is_ok(), "");
    set.add("active", sm.active, "");
    let (banner, reason, help) = sm.banner();
    set.add("banner text", banner == BANNER_TEXT, "");
    set.add("banner manual no reason", reason.is_none(), "");
    set.add("banner help", help == HELP_LINK, "");

    // 判据三：连续异常关机询问。
    let mut sm2 = SafeMode::new();
    sm2.note_abnormal_shutdown(100);
    set.add("one strike no ask", !sm2.should_ask_next_boot(), "");
    sm2.note_abnormal_shutdown(200);
    set.add("two strikes ask", sm2.should_ask_next_boot(), "");
    set.add("accept gives param", sm2.accept_ask() == "safe-mode", "");
    set.add("ask cleared", !sm2.should_ask_next_boot(), "");
    // 拒绝清态。
    sm2.note_abnormal_shutdown(300);
    sm2.note_abnormal_shutdown(400);
    sm2.decline_ask();
    set.add("decline resets", !sm2.should_ask_next_boot() && sm2.abnormal_strikes == 0, "");
    // 正常关机清计数。
    sm2.note_abnormal_shutdown(500);
    sm2.note_clean_shutdown();
    set.add("clean resets", sm2.abnormal_strikes == 0, "");

    // 原因文案（异常关机路径进入——黄条带理由）。
    let mut sm3 = SafeMode::new();
    sm3.note_abnormal_shutdown(1);
    sm3.note_abnormal_shutdown(2);
    set.add("enter after abnormal", sm3.enter(EntryReason::AfterAbnormal, true).is_ok(), "");
    let (_, reason3, _) = sm3.banner();
    set.add("reason text", reason3 == Some(BANNER_REASON_ABNORMAL), "");
    // 参数直进路径也有理由。
    let mut sm4 = SafeMode::new();
    let _ = sm4.enter(EntryReason::KernelParam, true);
    let (_, reason4, _) = sm4.banner();
    set.add("param reason", reason4.is_some(), "");

    // 判据二：白名单外灰置且可解释；白名单内放行。
    set.add("gate allows min", MIN_SET.iter().all(|f| sm3.gate(f).allowed), "");
    let g = sm3.gate("wallpaper-store");
    set.add("gate blocks outside", !g.allowed && !g.why.is_empty(), "");
    let g2 = sm3.gate("update-ui");
    set.add("gate blocks update-ui", !g2.allowed, "");

    // 更新禁用（防半态更新）。
    set.add("update blocked", sm3.update_allowed().is_err(), "");
    set.add("update allowed normal", SafeMode::new().update_allowed().is_ok(), "");

    // 低分辨率保底。
    let (w, h, gop) = sm3.display_fallback();
    set.add("safe res", w == 1024 && h == 768 && gop, "");
    set.add("normal res untouched", SafeMode::new().display_fallback() == (0, 0, false), "");

    // 退出无残留。
    sm3.exit_via_reboot();
    set.add("exit clean", !sm3.active && sm3.reason.is_none() && sm3.abnormal_strikes == 0, "");
    set.add("exit restores gates", sm3.gate("wallpaper-store").allowed, "");

    // 选单小字常量。
    set.add("menu hint", MENU_HINT_TEXT.contains("Shift") && MENU_HINT_PX == 10, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f193_enter_requires_param_even_after_ask() {
        let mut sm = SafeMode::new();
        sm.note_abnormal_shutdown(1);
        sm.note_abnormal_shutdown(2);
        // 询问被接受只给参数旗标——真正进入仍需引导链带上参数（一处一事实）。
        let flag = sm.accept_ask();
        assert_eq!(flag, "safe-mode");
        assert!(sm.enter(EntryReason::AfterAbnormal, false).is_err());
        assert!(sm.enter(EntryReason::AfterAbnormal, true).is_ok());
    }

    #[test]
    fn f193_strikes_persist_until_clean() {
        let mut sm = SafeMode::new();
        sm.note_abnormal_shutdown(1);
        sm.note_abnormal_shutdown(2);
        sm.note_abnormal_shutdown(3); // 三连崩：依然只问（阈值=2 不是每多崩一次多问）。
        assert!(sm.should_ask_next_boot());
        sm.note_clean_shutdown();
        sm.note_abnormal_shutdown(9);
        assert!(!sm.should_ask_next_boot(), "clean boot resets the streak");
    }

    #[test]
    fn f193_gate_off_mode_is_passthrough() {
        let sm = SafeMode::new();
        assert!(sm.gate("anything").allowed, "normal mode gates nothing");
        assert_eq!(sm.gate("anything").why, "");
    }

    #[test]
    fn f193_min_set_covers_rescue_paths() {
        // 救援动线四要件必须在最小集（设置/资源管理器/卸载/诊断）——主册
        // 【交互设计】逐字。
        for need in ["settings", "explorer", "uninstaller", "diagnostics"] {
            assert!(MIN_SET.contains(&need), "{need} must be in min set");
        }
    }

    #[test]
    fn f193_exit_via_reboot_then_reenter() {
        let mut sm = SafeMode::new();
        sm.enter(EntryReason::KernelParam, true).unwrap();
        sm.exit_via_reboot();
        assert!(!sm.active);
        sm.enter(EntryReason::ManualMenu, true).unwrap();
        assert!(sm.active && sm.reason == Some(EntryReason::ManualMenu));
    }

    #[test]
    fn f193_run_checks_pass() {
        assert!(run_safemode_checks().all_passed());
    }
}
