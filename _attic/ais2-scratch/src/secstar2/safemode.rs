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

// ---------------------------------------------------------------------------
// 深化子系统（回炉补深化 2026-09-26 · 主册【交互设计】【数据与存储】【状态
// 与异常】【设计细节】全展开）——六个真功能面，零注水。
// ---------------------------------------------------------------------------

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 深一：MenuEntry —— F171 选单隐藏条目状态机（第三枚条目 + Shift 门）
// ---------------------------------------------------------------------------

/// 选单条目（图形选单 F171 的安全模式侧投影——第三枚隐藏条目）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuEntry {
    /// 条目名（显示于选单第三行）。
    pub label: &'static str,
    /// 是否需要按住 Shift 才可激活（隐藏条目的发现性语义——不按即无效）。
    pub shift_required: bool,
}

/// 选单隐藏条目（主册【功能定义】逐字：第三枚隐藏条目，按住 Shift 点 VARIX）。
pub const SAFE_MENU_ENTRY: MenuEntry = MenuEntry { label: "VARIX（安全模式）", shift_required: true };

/// 选单激活判定：Shift 未按住 → 无效（防呆——普通点击绝不误入救援模式）；
/// Shift 按住 → 放行并要求引导链带 safe-mode 参数（进入唯一源不变）。
/// 返回 Err(文案) = 未激活；Ok(()) = 走带参重启。
pub fn menu_activate(shift_held: bool) -> Result<&'static str, &'static str> {
    if !shift_held || !SAFE_MENU_ENTRY.shift_required {
        return Err("需按住 Shift 点击（选单底部小字有提示）");
    }
    Ok("safe-mode")
}

// ---------------------------------------------------------------------------
// 深二：RepairFlow —— 安全模式修复动线（进-修-退全链的「修」段状态机）
// ---------------------------------------------------------------------------

/// 修复动线阶段（主册【用户故事】：进安全模式→把出问题的主题卸掉→正常重启）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepairStage {
    /// 已进入安全模式（黄条常驻）。
    Entered,
    /// 诊断中（用户在最小集内排查——设置/资源管理器/诊断中心）。
    Diagnosing,
    /// 卸载通道执行中（出问题的组件被移除）。
    Uninstalling { target: &'static str },
    /// 已修复，等待正常重启（退出=重启即出——无残留）。
    Fixed,
}

/// 修复动线状态机。
pub struct RepairFlow {
    pub stage: RepairStage,
    /// 卸载执行计数（修复动线对账）。
    pub uninstalls: u64,
    /// 退出完成计数（重启后清态——全链闭合的账目面）。
    pub exits: u64,
}

impl RepairFlow {
    pub fn new() -> RepairFlow {
        RepairFlow { stage: RepairStage::Entered, uninstalls: 0, exits: 0 }
    }

    /// 排查开始（用户打开最小集内任一诊断面）。
    pub fn begin_diagnose(&mut self) -> Result<(), &'static str> {
        match self.stage {
            RepairStage::Entered => {
                self.stage = RepairStage::Diagnosing;
                Ok(())
            }
            _ => Err("不在「已进入」态"),
        }
    }

    /// 卸载执行（目标必须在最小集白名单之外才有卸载意义——白名单内组件
    /// 是救援本体，卸掉它们等于拆掉自己的梯子）。
    pub fn uninstall(&mut self, target: &'static str, sm_active: bool) -> Result<&'static str, &'static str> {
        if !sm_active {
            return Err("修复动线只在安全模式内有效");
        }
        if MIN_SET.contains(&target) {
            return Err("最小集组件不可卸载（它们是救援本体）");
        }
        self.stage = RepairStage::Uninstalling { target };
        self.uninstalls += 1;
        Ok("已卸载：重启后将恢复正常模式")
    }

    /// 修复完成 → 等待重启。
    pub fn mark_fixed(&mut self) -> Result<(), &'static str> {
        match self.stage {
            RepairStage::Uninstalling { .. } | RepairStage::Diagnosing => {
                self.stage = RepairStage::Fixed;
                Ok(())
            }
            _ => Err("未处于修复中"),
        }
    }

    /// 正常重启（退出无残留——全链闭合）。
    pub fn reboot_exit(&mut self) -> bool {
        if self.stage == RepairStage::Fixed {
            self.stage = RepairStage::Entered;
            self.exits += 1;
            true
        } else {
            false
        }
    }
}

impl Default for RepairFlow {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深三：MinSetPage —— F126 清单公开面（12 项逐项带理由——公开即问责）
// ---------------------------------------------------------------------------

/// 最小集条目（清单公开 = 每项一句存在理由，F126 开放格式条款）。
pub struct MinSetItem {
    pub id: &'static str,
    pub why: &'static str,
}

/// 12 项逐项理由（与 MIN_SET 一一对应——顺序即 MIN_SET 序，审计可对拍）。
pub const MIN_SET_DOC: [MinSetItem; 12] = [
    MinSetItem { id: "settings", why: "排查与恢复的全部入口都在设置中心" },
    MinSetItem { id: "explorer", why: "查看与搬运问题文件需要资源管理器" },
    MinSetItem { id: "uninstaller", why: "卸掉把系统搞坏的东西是安全模式的本职" },
    MinSetItem { id: "diagnostics", why: "看清哪里坏了才能修" },
    MinSetItem { id: "terminal", why: "修复动线的命令通道" },
    MinSetItem { id: "recovery", why: "安全模式修不好时的下一级救援（F198）" },
    MinSetItem { id: "taskbar", why: "桌面骨架——没有它寸步难行" },
    MinSetItem { id: "window-mgr", why: "窗口管理（合成器降级路径仍需它）" },
    MinSetItem { id: "theme-default", why: "默认主题旁路坏主题（E1 域令牌不加载）" },
    MinSetItem { id: "ime-base", why: "中文修复场景离不开输入法" },
    MinSetItem { id: "clipboard", why: "取证与求助材料的搬运" },
    MinSetItem { id: "log-export", why: "把日志带出去求助（F188 面可达）" },
];

/// F126 公开页完整性自检：MIN_SET_DOC 与 MIN_SET 逐位对应、无缺项、理由
/// 非空（清单公开判据——公开的不只是名字，还有每项的存在理由）。
pub fn min_set_doc_intact() -> bool {
    MIN_SET.len() == MIN_SET_DOC.len()
        && MIN_SET
            .iter()
            .zip(MIN_SET_DOC.iter())
            .all(|(id, doc)| *id == doc.id && doc.why.len() >= 8)
}

// ---------------------------------------------------------------------------
// 深四：param_readback —— F192 参数回读一致性（模式标记唯一源的对账面）
// ---------------------------------------------------------------------------

/// 从 F192 启动旗标回读安全模式意图（一处一事实：F193 的激活判定消费的
/// 就是 F192 解析面的同一份结果——不二次解析不另立标准）。
/// 返回 (应进入安全模式, 黄条原因线索)。
pub fn param_readback(flags: &crate::secstar2::paramwl::BootFlags) -> (bool, Option<&'static str>) {
    if flags.safe_mode {
        (true, Some("本次启动携带 safe-mode 内核参数"))
    } else {
        (false, None)
    }
}

/// 参数回读 → 进入链路自检（enter 的 param_present 必须与回读一致——
/// 两处结论不一致即引导链缺陷，本函数供启动自检调用）。
pub fn param_readback_consistent(flags: &crate::secstar2::paramwl::BootFlags, enter_will_succeed: bool) -> bool {
    let (want, _) = param_readback(flags);
    want == enter_will_succeed
}

// ---------------------------------------------------------------------------
// 深五：StrikeBook —— 连续异常关机序列账（防循环崩的可观测面）
// ---------------------------------------------------------------------------

/// 异常关机序列账（跨启动持久由快照层承载，本层记序：每次异常一条时间戳，
/// 正常关机截断序列——「连续」二字的账本语义）。
pub struct StrikeBook {
    stamps: RingLog<u64, 8>,
    /// 序列长度（=连续异常次数；正常关机清零）。
    pub streak: u32,
    /// 询问触发次数（达阈值后每次进入询问态都记账——不重复骚扰的可对账面）。
    pub asks_raised: u64,
}

impl StrikeBook {
    pub fn new() -> StrikeBook {
        StrikeBook { stamps: RingLog::new(), streak: 0, asks_raised: 0 }
    }

    /// 登记一次异常关机。达 ABNORMAL_STRIKES 且未在询问中 → 置询问（一次）。
    pub fn note_abnormal(&mut self, stamp: u64) -> bool {
        self.stamps.push(stamp);
        self.streak += 1;
        if self.streak == ABNORMAL_STRIKES {
            self.asks_raised += 1;
            return true; // 触发询问（恰好达阈值这一次，之后继续崩不再重复问）。
        }
        false
    }

    /// 正常关机（截断序列）。
    pub fn note_clean(&mut self) {
        self.streak = 0;
        self.stamps = RingLog::new();
    }

    /// 最近异常时间戳（新→旧——诊断页展示）。
    pub fn recent(&self) -> Vec<u64> {
        self.stamps.newest_first()
    }
}

impl Default for StrikeBook {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深六：SessionLog —— 安全模式会话体验日志（十三章：交互细节层）
// ---------------------------------------------------------------------------

/// 会话事件种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmEventKind {
    /// 黄条渲染（模式身份可见）。
    BannerShown,
    /// 灰置功能被点击（可解释性现场——用户撞上了边界）。
    GatedClick,
    /// 帮助链打开（「为什么我在安全模式」）。
    HelpOpened,
    /// 更新被拒（防半态更新执法）。
    UpdateBlocked,
}

impl SmEventKind {
    pub fn name(self) -> &'static str {
        match self {
            SmEventKind::BannerShown => "banner-shown",
            SmEventKind::GatedClick => "gated-click",
            SmEventKind::HelpOpened => "help-opened",
            SmEventKind::UpdateBlocked => "update-blocked",
        }
    }
}

/// 会话事件（时刻/种类/对象功能 ID）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmEvent {
    pub at_s: u64,
    pub kind: SmEventKind,
    pub feature: &'static str,
}

/// 会话账（定容环；灰置点击是本域的挫败信号候选——单独计数）。
pub struct SmSessionLog {
    events: RingLog<SmEvent, 32>,
    pub gated_clicks: u64,
}

impl SmSessionLog {
    pub fn new() -> SmSessionLog {
        SmSessionLog { events: RingLog::new(), gated_clicks: 0 }
    }

    pub fn push(&mut self, e: SmEvent) {
        if e.kind == SmEventKind::GatedClick {
            self.gated_clicks += 1;
        }
        self.events.push(e);
    }

    pub fn recent(&self) -> Vec<SmEvent> {
        self.events.newest_first()
    }

    /// 按种类计数（诊断页聚合）。
    pub fn count_kind(&self, kind: SmEventKind) -> usize {
        self.events.newest_first().iter().filter(|e| e.kind == kind).count()
    }
}

impl Default for SmSessionLog {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

/// F193 深化自检（聚合进 secstar2 域）。
pub fn run_safemode_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("F193-deep");

    // 深一：选单隐藏条目——Shift 门两路（未按=拒且指路；按住=给参数）。
    set.add("menu no shift", menu_activate(false).is_err(), "");
    set.add("menu err guides", menu_activate(false).unwrap_err().contains("Shift"), "");
    set.add("menu with shift", menu_activate(true) == Ok("safe-mode"), "");
    set.add("menu entry hidden by default", SAFE_MENU_ENTRY.shift_required, "");

    // 深二：修复动线全链——进入→排查→卸载（白名单内拒绝）→修复→重启闭合。
    let mut rf = RepairFlow::new();
    set.add("flow diagnose", rf.begin_diagnose().is_ok(), "");
    set.add("flow min-set refuse", rf.uninstall("settings", true).is_err(), "救援本体不可卸");
    set.add("flow uninstall", rf.uninstall("theme-custom", true).map(|s| s.contains("卸载")).unwrap_or(false), "");
    set.add("flow counted", rf.uninstalls == 1, "");
    set.add("flow fixed", rf.mark_fixed().is_ok(), "");
    set.add("flow reboot closes", rf.reboot_exit() && rf.exits == 1, "");
    set.add("flow reboot early refuse", RepairFlow::new().reboot_exit() == false, "未修复不可退出闭合");
    // 非安全模式调用拒绝。
    let mut rf2 = RepairFlow::new();
    let _ = rf2.begin_diagnose();
    set.add("flow needs safemode", rf2.uninstall("theme-custom", false).is_err(), "");

    // 深三：F126 清单公开面——12 项逐位对应+理由非空。
    set.add("minset doc intact", min_set_doc_intact(), "");
    set.add("minset doc count", MIN_SET_DOC.len() == 12, "");
    set.add("minset doc order", MIN_SET_DOC[0].id == MIN_SET[0] && MIN_SET_DOC[11].id == MIN_SET[11], "");

    // 深四：参数回读一致性——带参→应进入；无参→不进入。
    let f_on = crate::secstar2::paramwl::BootFlags { safe_mode: true, ..Default::default() };
    let f_off = crate::secstar2::paramwl::BootFlags::default();
    let (want, hint) = param_readback(&f_on);
    set.add("readback on", want && hint.is_some(), "");
    set.add("readback off", !param_readback(&f_off).0 && param_readback(&f_off).1.is_none(), "");
    set.add("readback consistent", param_readback_consistent(&f_on, true) && param_readback_consistent(&f_off, false), "");
    set.add("readback mismatch caught", !param_readback_consistent(&f_on, false), "引导链缺陷可被启动自检捕获");

    // 深五：异常关机序列账——恰好达阈值问一次；继续崩不重复；正常关机截断。
    let mut sb = StrikeBook::new();
    set.add("strike 1 quiet", !sb.note_abnormal(10), "");
    set.add("strike 2 asks", sb.note_abnormal(20), "");
    set.add("strike 3 no re-ask", !sb.note_abnormal(30), "防循环崩——只问一次");
    set.add("ask counted once", sb.asks_raised == 1, "");
    sb.note_clean();
    set.add("clean truncates", sb.streak == 0 && sb.recent().is_empty(), "");

    // 深六：会话账——灰置点击计挫败信号；按种类聚合。
    let mut sl = SmSessionLog::new();
    sl.push(SmEvent { at_s: 1, kind: SmEventKind::BannerShown, feature: "" });
    sl.push(SmEvent { at_s: 2, kind: SmEventKind::GatedClick, feature: "wallpaper-store" });
    sl.push(SmEvent { at_s: 3, kind: SmEventKind::GatedClick, feature: "update-ui" });
    sl.push(SmEvent { at_s: 4, kind: SmEventKind::HelpOpened, feature: "" });
    set.add("session kinds", sl.count_kind(SmEventKind::GatedClick) == 2, "");
    set.add("session frustration", sl.gated_clicks == 2, "");
    set.add("session newest first", sl.recent()[0].at_s == 4, "");
    set.add("session event names", SmEventKind::UpdateBlocked.name() == "update-blocked", "");

    // 黄条文案常量复核（深化面共用）。
    set.add("banner reason const", BANNER_REASON_ABNORMAL.contains("连续异常关机"), "");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn f193_deep_menu_shift_gate_never_misfires() {
        // 100 次未按 Shift 的普通点击：零激活（防误入——防呆的确定性验证）。
        for i in 0..100 {
            assert!(menu_activate(false).is_err(), "click {i} must not activate");
        }
        assert!(menu_activate(true).is_ok());
    }

    #[test]
    fn f193_deep_repair_flow_full_journey() {
        // 完整救援旅程：主题把桌面搞花屏 → 进安全模式 → 卸载坏主题 → 重启。
        let mut sm = SafeMode::new();
        sm.enter(EntryReason::ManualMenu, true).unwrap();
        let mut rf = RepairFlow::new();
        rf.begin_diagnose().unwrap();
        // 尝试卸载救援本体 → 拒绝（梯子不能拆）。
        assert!(rf.uninstall("diagnostics", sm.active).is_err());
        // 卸载坏主题 → 成功。
        rf.uninstall("theme-custom", sm.active).unwrap();
        rf.mark_fixed().unwrap();
        assert!(rf.reboot_exit());
        sm.exit_via_reboot();
        assert!(!sm.active);
    }

    #[test]
    fn f193_deep_minset_doc_covers_rescue_verbs() {
        // 救援四动词（看/修/卸/带出）全部有对应条目理由。
        assert!(MIN_SET_DOC.iter().any(|d| d.why.contains("资源管理器")));
        assert!(MIN_SET_DOC.iter().any(|d| d.why.contains("卸")));
        assert!(MIN_SET_DOC.iter().any(|d| d.why.contains("命令")));
        assert!(MIN_SET_DOC.iter().any(|d| d.why.contains("日志")));
    }

    #[test]
    fn f193_deep_readback_from_real_parser_output() {
        // 与 F192 真解析链对拍：check_line("safe-mode") → extract_flags → 回读。
        let mut wl = crate::secstar2::paramwl::ParamWhitelist::new();
        let vs = wl.check_line("verbose safe-mode");
        let flags = crate::secstar2::paramwl::extract_flags(&vs);
        let (want, _) = param_readback(&flags);
        assert!(want, "parser output feeds safemode readback");
        assert!(param_readback_consistent(&flags, true));
    }

    #[test]
    fn f193_deep_strikebook_ring_caps() {
        let mut sb = StrikeBook::new();
        for i in 0..30 {
            let _ = sb.note_abnormal(i);
        }
        assert!(sb.recent().len() <= 8, "ring capped");
        assert_eq!(sb.asks_raised, 1, "exactly one ask across the storm");
    }

    #[test]
    fn f193_deep_run_checks_pass() {
        assert!(run_safemode_deep_checks().all_passed());
    }
}
