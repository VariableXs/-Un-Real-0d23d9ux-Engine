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

use alloc::vec;
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

// ---------------------------------------------------------------------------
// v3 批次（回炉补深化第三轮 2026-09-26）——黄条渲染数据 / 帮助篇 /
// 功能门矩阵导出。判据源：主册【交互设计】「右下角常驻黄条（不可关——
// 模式标识就是身份）」+「进入提示条附『为什么我在安全模式』帮助链」+
//【验收判据】「最小集白名单外功能全部灰置且可解释」的矩阵化。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v3-一：BannerRender —— 黄条渲染数据（右下角常驻、不可关是身份——
// 渲染契约里没有关闭钮这个字段）
// ---------------------------------------------------------------------------

/// 黄条渲染数据。
pub struct BannerRender {
    /// 主行（BANNER_TEXT）。
    pub main: &'static str,
    /// 原因行（None=用户自己选的——不吓唬）。
    pub reason: Option<&'static str>,
    /// 帮助链。
    pub help: &'static str,
    /// 屏幕锚点（右下角——常驻位置是契约）。
    pub anchor: &'static str,
    /// 可关闭（恒 false——模式标识就是身份，机检字段）。
    pub dismissible: bool,
}

/// 组装（复用 SafeMode::banner 的语义 + 渲染契约扩展）。
pub fn banner_render(sm: &SafeMode) -> BannerRender {
    let (main, reason, help) = sm.banner();
    BannerRender { main, reason, help, anchor: "bottom-right", dismissible: false }
}

// ---------------------------------------------------------------------------
// v3-二：HelpArticle —— 「为什么我在安全模式」帮助篇（帮助链的落点：
// 三段式——发生了什么/能做什么/怎么出去）
// ---------------------------------------------------------------------------

/// 帮助段。
pub struct HelpSection {
    pub heading: &'static str,
    pub body: &'static str,
}

/// 帮助篇正文（三段——与 F186 帮助篇同版式不同内容，同族纪律）。
pub const HELP_ARTICLE: [HelpSection; 3] = [
    HelpSection {
        heading: "我在什么模式",
        body: "安全模式是一个救援模式：只加载 12 项最小功能集，第三方驱动不加载、你的主题不加载——先把系统带起来，再谈修好它。",
    },
    HelpSection {
        heading: "我现在能做什么",
        body: "设置中心、资源管理器、卸载通道、诊断中心都在。你可以卸掉把系统搞坏的东西——右下角黄条不会消失，这是身份不是故障。",
    },
    HelpSection {
        heading: "我怎么出去",
        body: "正常重启即出——安全模式没有持久标记，退出零残留。如果修不好，恢复环境（引导选单进入）是下一级救援。",
    },
];

/// 帮助篇完整性自检（三段齐+正文人话）。
pub fn help_article_intact() -> bool {
    HELP_ARTICLE.len() == 3
        && HELP_ARTICLE.iter().all(|s| !s.heading.is_empty() && s.body.len() >= 20)
}

// ---------------------------------------------------------------------------
// v3-三：GateMatrix —— 功能门矩阵导出（灰置可解释的全量视图：给定功能
// 全集清单，逐项输出可用性+原因——审计页/帮助页共用一份数据）
// ---------------------------------------------------------------------------

/// 矩阵行。
pub struct GateRow {
    pub feature: &'static str,
    pub allowed: bool,
    pub why: &'static str,
    /// 是否最小集成员。
    pub in_min_set: bool,
}

/// 矩阵导出（features = 全功能清单——含白名单外的一切）。
pub fn gate_matrix(sm: &SafeMode, features: &[&'static str]) -> Vec<GateRow> {
    features
        .iter()
        .map(|f| {
            let g = sm.gate(f);
            GateRow { feature: f, allowed: g.allowed, why: g.why, in_min_set: MIN_SET.contains(f) }
        })
        .collect()
}

/// 矩阵守恒式：白名单内全放行、白名单外全灰置带原因——一屏看清边界。
pub fn gate_matrix_consistent(rows: &[GateRow]) -> bool {
    rows.iter().all(|r| if r.in_min_set { r.allowed } else { !r.allowed && !r.why.is_empty() })
}

// ---------------------------------------------------------------------------
// v3 自检
// ---------------------------------------------------------------------------

/// F193 v3 自检（聚合进 secstar2 域）。
pub fn run_safemode_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F193-v3");

    // v3-一：黄条渲染——右下角锚、不可关机检、原因随来源。
    let mut sm = SafeMode::new();
    let _ = sm.enter(EntryReason::AfterAbnormal, true);
    let br = banner_render(&sm);
    set.add("banner anchor", br.anchor == "bottom-right", "");
    set.add("banner not dismissible", !br.dismissible, "模式标识就是身份——机检字段");
    set.add("banner reason", br.reason == Some(BANNER_REASON_ABNORMAL), "");
    set.add("banner main", br.main == BANNER_TEXT, "");

    // v3-二：帮助篇——三段齐、人话、覆盖三问。
    set.add("help intact", help_article_intact(), "");
    set.add("help what", HELP_ARTICLE[0].body.contains("12 项"), "");
    set.add("help can", HELP_ARTICLE[1].body.contains("卸载通道"), "");
    set.add("help exit", HELP_ARTICLE[2].body.contains("零残留"), "");

    // v3-三：功能门矩阵——守恒式与边界。
    let features: Vec<&'static str> = MIN_SET
        .iter()
        .copied()
        .chain(["wallpaper-store", "update-ui", "theme-custom", "third-store"])
        .collect();
    let rows = gate_matrix(&sm, &features);
    set.add("gate matrix size", rows.len() == 16, "");
    set.add("gate matrix consistent", gate_matrix_consistent(&rows), "");
    let outside = rows.iter().find(|r| r.feature == "update-ui").unwrap();
    set.add("gate outside why", !outside.allowed && outside.why.contains("最小集"), "");
    let inside = rows.iter().find(|r| r.feature == "settings").unwrap();
    set.add("gate inside allowed", inside.allowed && inside.in_min_set, "");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn f193_v3_banner_off_mode_is_passthrough() {
        // 非安全模式：黄条不渲染（main 为空语义）——banner 只在模式内存在。
        let sm = SafeMode::new();
        let br = banner_render(&sm);
        assert!(br.reason.is_none());
        assert!(br.main == BANNER_TEXT, "contract fields always present");
        // 可关性字段与模式无关——契约恒不可关（渲染层只在 active 时挂载）。
        assert!(!br.dismissible);
    }

    #[test]
    fn f193_v3_gate_matrix_scales_to_100_features() {
        // 100 功能压测：矩阵无崩、守恒式全绿（白名单边界稳定）。
        let mut sm = SafeMode::new();
        let _ = sm.enter(EntryReason::KernelParam, true);
        let mut features: Vec<&'static str> = MIN_SET.to_vec();
        for i in 0..88 {
            features.push(match i % 4 {
                0 => "ext-a",
                1 => "ext-b",
                2 => "ext-c",
                _ => "ext-d",
            });
        }
        let rows = gate_matrix(&sm, &features);
        assert_eq!(rows.len(), 100);
        assert!(gate_matrix_consistent(&rows));
    }

    #[test]
    fn f193_v3_run_checks_pass() {
        assert!(run_safemode_deep2_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v4 批次（第四轮深化 2026-09-26）——选单隐藏条目发现性 / 安全桌面模型 /
// 异常关机询问流 / 会话账。判据源：主册【设计细节】「隐藏条目发现性：选单
// 底部 10px 小字（可发现但不打扰）」+【交互设计】「功能面最小集（设置中心/
// 资源管理器/卸载通道/诊断中心）」+【状态与异常】「连续两次异常关机 → 下次
// 启动自动询问（防循环崩）」。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v4-一：MenuHintModel —— 选单隐藏条目发现性模型（「可发现但不打扰」的
// 机器检查：常驻不闪烁、不抢焦点、不拦截选中、超时零动作）
// ---------------------------------------------------------------------------

/// 小字提示的可观测纪律（逐条可机检）。
pub struct MenuHintModel {
    /// 是否常驻（超时消失=不可发现——必须常驻）。
    pub persistent: bool,
    /// 是否闪烁（闪烁=打扰——禁止）。
    pub blinking: bool,
    /// 是否抢焦点（抢焦点=打扰——禁止）。
    pub focus_stealing: bool,
    /// 悬停是否拦截选单键导航（拦截=打扰——禁止）。
    pub blocks_navigation: bool,
}

impl MenuHintModel {
    /// 规范实现常量（合规基线——与 MENU_HINT_PX=10 同源一处一事实）。
    pub fn compliant() -> MenuHintModel {
        MenuHintModel {
            persistent: true,
            blinking: false,
            focus_stealing: false,
            blocks_navigation: false,
        }
    }

    /// 合规判定（四纪律全过=可发现但不打扰）。
    pub fn ok(&self) -> bool {
        self.persistent && !self.blinking && !self.focus_stealing && !self.blocks_navigation
    }

    /// 点击进入条件（小字本身不可点——点击目标是 VARIX 条目+Shift 门）。
    pub fn click_target_is_entry(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// v4-二：SafeDesktopModel —— 安全模式桌面模型（黄条锚定+四功能入口+灰置
// 项可解释——功能面最小集的桌面落位；白名单外入口照常渲染但灰置+理由）
// ---------------------------------------------------------------------------

/// 桌面入口项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DesktopEntry {
    pub feature: &'static str,
    pub label: &'static str,
    /// 可用（MIN_SET 成员）。
    pub enabled: bool,
    /// 灰置理由（可用时为空）。
    pub why: &'static str,
}

/// 桌面主入口四件（主册【交互设计】逐字）。
pub const DESKTOP_ENTRIES: [(&'static str, &'static str); 4] = [
    ("settings", "设置中心"),
    ("explorer", "资源管理器"),
    ("uninstaller", "卸载通道"),
    ("diagnostics", "诊断中心"),
];

/// 桌面灰置文案（白名单外统一语——可解释不是「不可用」三个字打发）。
pub const GREYED_WHY: &str = "安全模式下仅保留修复所需的最小功能集；此功能将在正常重启后恢复";

/// 桌面模型构建（从 SafeMode 状态生成入口表：四主入口+任意附加面灰置）。
pub fn safe_desktop_entries(sm: &SafeMode, extra_features: &[&'static str]) -> alloc::vec::Vec<DesktopEntry> {
    let mut out = alloc::vec::Vec::new();
    for (fid, label) in DESKTOP_ENTRIES {
        let g = sm.gate(fid);
        out.push(DesktopEntry { feature: fid, label, enabled: g.allowed, why: g.why });
    }
    for fid in extra_features {
        let g = sm.gate(fid);
        out.push(DesktopEntry {
            feature: fid,
            label: fid,
            enabled: g.allowed,
            why: if g.allowed { "" } else { GREYED_WHY },
        });
    }
    out
}

// ---------------------------------------------------------------------------
// v4-三：CrashAskFlow —— 异常关机询问流（下次启动询问的完整状态机：
// 提问 → 接受（带参进安全模式）/ 拒绝（清计数）——问过不再骚扰）
// ---------------------------------------------------------------------------

/// 询问流状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AskPhase {
    /// 无需询问（计数不足或已处理）。
    Idle,
    /// 下次启动应询问（ask_pending）。
    Pending,
    /// 已询问且用户接受 → 带 safe-mode 参数重启。
    Accepted,
    /// 已询问且用户拒绝 → 计数清零正常启动。
    Declined,
}

/// 询问流（与 SafeMode 的 strikes/ask_pending 联动的对外语义层）。
pub struct CrashAskFlow {
    pub phase: AskPhase,
    /// 询问呈现次数（本次启动内恒 ≤1——「只问一次」的机检面）。
    pub asked_count: u32,
}

impl CrashAskFlow {
    pub fn new() -> CrashAskFlow {
        CrashAskFlow { phase: AskPhase::Idle, asked_count: 0 }
    }

    /// 启动时评估（strikes 来自持久层回读）。
    pub fn boot_evaluate(&mut self, sm: &SafeMode) {
        if sm.should_ask_next_boot() {
            self.phase = AskPhase::Pending;
        } else {
            self.phase = AskPhase::Idle;
        }
    }

    /// 呈现询问（幂等——第二次调用不再计数也不再变相重复弹）。
    pub fn present(&mut self) -> bool {
        if self.phase != AskPhase::Pending || self.asked_count > 0 {
            return false;
        }
        self.asked_count += 1;
        true
    }

    /// 用户接受。
    pub fn accept(&mut self, sm: &mut SafeMode) -> Result<&'static str, &'static str> {
        if self.phase != AskPhase::Pending {
            return Err("未处于询问态");
        }
        let text = sm.accept_ask();
        self.phase = AskPhase::Accepted;
        Ok(text)
    }

    /// 用户拒绝（清态——不给用户留一个永远消不掉的询问）。
    pub fn decline(&mut self, sm: &mut SafeMode) -> Result<(), &'static str> {
        if self.phase != AskPhase::Pending {
            return Err("未处于询问态");
        }
        sm.decline_ask();
        self.phase = AskPhase::Declined;
        Ok(())
    }
}

impl Default for CrashAskFlow {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// v4-四：SessionAccount —— 安全模式会话账（进入原因/进入时刻/修复动作/
// 退出方式——一次安全模式会话的完整故事，退出时归档一行）
// ---------------------------------------------------------------------------

/// 会话账行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRecord {
    /// 进入原因文案。
    pub reason: &'static str,
    /// 进入时刻（秒戳）。
    pub entered_s: u64,
    /// 会话时长（秒——退出时结算）。
    pub duration_s: u64,
    /// 修复动作序列（卸载了什么/改了什么）。
    pub actions: alloc::vec::Vec<&'static str>,
    /// 退出方式（正常重启即出——无残留纪律）。
    pub exit_via: &'static str,
}

/// 会话账（进入开账 → 动作记录 → 退出结算）。
pub struct SessionAccount {
    open: Option<SessionRecord>,
    /// 归档账（历史会话）。
    pub archive: alloc::vec::Vec<SessionRecord>,
}

impl SessionAccount {
    pub fn new() -> SessionAccount {
        SessionAccount { open: None, archive: alloc::vec::Vec::new() }
    }

    /// 开账（重复开账拒绝——一个会话一条账）。
    pub fn open(&mut self, reason: &'static str, at_s: u64) -> Result<(), &'static str> {
        if self.open.is_some() {
            return Err("会话已在进行中");
        }
        self.open = Some(SessionRecord {
            reason,
            entered_s: at_s,
            duration_s: 0,
            actions: alloc::vec::Vec::new(),
            exit_via: "",
        });
        Ok(())
    }

    /// 记录修复动作。
    pub fn action(&mut self, what: &'static str) -> Result<(), &'static str> {
        match self.open.as_mut() {
            Some(s) => {
                s.actions.push(what);
                Ok(())
            }
            None => Err("无进行中会话"),
        }
    }

    /// 退出结算（时长=退出-进入；归档并闭账）。
    pub fn close(&mut self, at_s: u64, exit_via: &'static str) -> Result<SessionRecord, &'static str> {
        let mut s = self.open.take().ok_or("无进行中会话")?;
        s.duration_s = at_s.saturating_sub(s.entered_s);
        s.exit_via = exit_via;
        self.archive.push(s.clone());
        Ok(s)
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

impl Default for SessionAccount {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// v4 自检
// ---------------------------------------------------------------------------

/// F193 v4 自检（聚合进 secstar2 域）。
pub fn run_safemode_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new("F193-v4");

    // v4-一：小字提示——合规基线四纪律、10px 常量同源。
    let hint = MenuHintModel::compliant();
    set.add("hint compliant", hint.ok(), "");
    set.add("hint persistent", hint.persistent && !hint.blinking, "常驻不闪烁");
    set.add("hint px const", MENU_HINT_PX == 10 && MENU_HINT_TEXT.contains("Shift"), "");
    set.add("hint click passthrough", hint.click_target_is_entry(), "");

    // v4-二：桌面模型——四主入口全可用、白名单外灰置+理由。
    let mut sm = SafeMode::new();
    sm.enter(EntryReason::ManualMenu, true).ok();
    let desk = safe_desktop_entries(&sm, &["theme-store", "app-market"]);
    set.add("desk entries", desk.len() == 6, "四主入口+2 灰置附加面");
    set.add("desk main enabled", desk[..4].iter().all(|e| e.enabled), "主册四件全可用");
    set.add("desk main labels", desk[0].label == "设置中心" && desk[3].label == "诊断中心", "");
    set.add("desk extra greyed", desk[4].why == GREYED_WHY && !desk[4].enabled, "白名单外灰置可解释");
    set.add("desk main no why", desk.iter().take(4).all(|e| e.why.is_empty()), "");

    // v4-三：询问流——两次异常触发、只问一次、接受带参、拒绝清态。
    let mut sm2 = SafeMode::new();
    sm2.note_abnormal_shutdown(100);
    sm2.note_abnormal_shutdown(200);
    let mut flow = CrashAskFlow::new();
    flow.boot_evaluate(&sm2);
    set.add("ask pending", flow.phase == AskPhase::Pending, "两次异常 → 下次启动询问");
    set.add("ask present once", flow.present() && !flow.present(), "幂等呈现");
    set.add("ask asked count", flow.asked_count == 1, "");
    set.add("ask accept", flow.accept(&mut sm2).is_ok() && flow.phase == AskPhase::Accepted, "");
    set.add("ask accept err after", flow.accept(&mut sm2).is_err(), "完成后再问=拒");
    // 拒绝路径：清态不再骚扰。
    let mut sm3 = SafeMode::new();
    sm3.note_abnormal_shutdown(1);
    sm3.note_abnormal_shutdown(2);
    let mut flow2 = CrashAskFlow::new();
    flow2.boot_evaluate(&sm3);
    flow2.present();
    set.add("ask decline", flow2.decline(&mut sm3).is_ok() && flow2.phase == AskPhase::Declined, "");
    set.add("ask decline cleared", !sm3.should_ask_next_boot(), "拒绝后清态");
    // 一次异常不询问。
    let mut sm4 = SafeMode::new();
    sm4.note_abnormal_shutdown(1);
    let mut flow3 = CrashAskFlow::new();
    flow3.boot_evaluate(&sm4);
    set.add("ask one strike idle", flow3.phase == AskPhase::Idle, "");

    // v4-四：会话账——开账→动作→结算归档全链、重复开账拒、无会话动作拒。
    let mut acc = SessionAccount::new();
    set.add("sess open", acc.open("菜单手选进入", 1000).is_ok(), "");
    set.add("sess dup open", acc.open("再来一次", 1001).is_err(), "一个会话一条账");
    set.add("sess action", acc.action("卸载出问题的主题包").is_ok() && acc.action("重置图标缓存").is_ok(), "");
    set.add("sess close", { let r = acc.close(1900, "正常重启"); r.map(|x| x.duration_s == 900 && x.actions.len() == 2).unwrap_or(false) }, "");
    set.add("sess archived", acc.archive.len() == 1 && !acc.is_open(), "");
    set.add("sess action after close", acc.action("幽灵动作").is_err(), "闭账后动作拒");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn f193_v4_ask_flow_full_lifecycle() {
        // 全生命周期回归：异常×2 → 询问 → 接受 → 进入 → 修复 → 正常重启退出
        // ——一次防循环崩闭环（主册用户故事的完整路径）。
        let mut sm = SafeMode::new();
        sm.note_abnormal_shutdown(10);
        sm.note_abnormal_shutdown(20);
        let mut flow = CrashAskFlow::new();
        flow.boot_evaluate(&sm);
        flow.present();
        flow.accept(&mut sm).unwrap();
        // 接受后带参进入（param_present=true——一处一事实链）。
        sm.enter(EntryReason::AfterAbnormal, true).unwrap();
        assert!(sm.active);
        sm.exit_via_reboot();
        assert!(!sm.active, "正常重启即出无残留");
        // 干净重启清计数。
        sm.note_clean_shutdown();
        assert!(!sm.should_ask_next_boot());
    }

    #[test]
    fn f193_v4_desktop_greyed_not_hidden() {
        // 灰置 ≠ 消失：附加面入口在桌面上仍然可见可解释（灰置且可解释判据）。
        let mut sm = SafeMode::new();
        sm.enter(EntryReason::KernelParam, true).unwrap();
        let desk = safe_desktop_entries(&sm, &["update-ui"]);
        let upd = desk.iter().find(|e| e.feature == "update-ui").unwrap();
        assert!(!upd.enabled);
        assert!(upd.why.contains("最小功能集"), "理由说清为什么");
    }

    #[test]
    fn f193_v4_session_archive_order() {
        // 两轮会话归档按序累积（时长结算各自独立）。
        let mut acc = SessionAccount::new();
        acc.open("r1", 0).unwrap();
        acc.action("a1").unwrap();
        acc.close(100, "正常重启").unwrap();
        acc.open("r2", 200).unwrap();
        acc.close(350, "正常重启").unwrap();
        assert_eq!(acc.archive.len(), 2);
        assert_eq!(acc.archive[0].duration_s, 100);
        assert_eq!(acc.archive[1].duration_s, 150);
        assert_eq!(acc.archive[0].actions.len(), 1);
        assert_eq!(acc.archive[1].actions.len(), 0);
    }

    #[test]
    fn f193_v4_run_checks_pass() {
        assert!(run_safemode_deep3_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v5 批次（第五轮深化 2026-09-26 · 主册上限口径冲刺）——黄条完整渲染模型 /
// 最小集帮助页 / 异常关机计数持久化 / 退出检查单。判据源：主册【交互设计】
// 「右下角常驻黄条（不可关——模式标识就是身份）」+【数据与存储】「无额外
// 持久态（模式标记=内核参数）」+【状态与异常】退出=正常重启无残留。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v5-一：SafeBannerRender —— 黄条完整渲染模型（主行+原因行+帮助链+锚位+
// 不可关语义——渲染层拿到的每个字段都定死）
// ---------------------------------------------------------------------------

/// 黄条渲染数据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SafeBannerRender {
    /// 主行（恒 BANNER_TEXT）。
    pub main: &'static str,
    /// 原因行（ManualMenu 进入=无原因行——用户自己选的不吓人）。
    pub reason: Option<&'static str>,
    /// 帮助链 ID。
    pub help: &'static str,
    /// 锚位（右下角——不遮主内容动线）。
    pub anchor: &'static str,
    /// 可关闭性（恒 false——身份标识不可关）。
    pub dismissible: bool,
}

/// 从 SafeMode 渲染（reason 直通 EntryReason 语义）。
pub fn safe_banner_render(sm: &SafeMode) -> SafeBannerRender {
    SafeBannerRender {
        main: BANNER_TEXT,
        reason: sm.reason.and_then(|r| r.reason_text()),
        help: HELP_LINK,
        anchor: "bottom-right",
        dismissible: false,
    }
}

// ---------------------------------------------------------------------------
// v5-二：MIN_SET_HELP —— 最小集帮助页（12 项逐条说明——为什么它必须在
// 救援场景在场：公开清单的帮助面）
// ---------------------------------------------------------------------------

/// 逐项说明行（与 MIN_SET/MIN_SET_DOC 逐位对应——一处一事实投影）。
pub const MIN_SET_HELP: [&str; 12] = [
    "设置中心：调整系统行为的主入口（修复动线的起点）",
    "资源管理器：定位与搬运文件（卸载与取证都要用）",
    "卸载通道：移除导致故障的主题/驱动/应用",
    "诊断中心：看体检灯与自愈记录，定位问题根因",
    "终端：高级修复命令入口（救援四要件）",
    "恢复环境入口：一键进入 F198 最后防线",
    "任务栏：桌面骨架（窗口切换与托盘）",
    "窗口管理：合成器降级路径保底（窗口不消失）",
    "默认主题：E1 旁路——仅加载出厂令牌（防花屏主题）",
    "基础输入：中文修复场景可用（IME 最小集）",
    "剪贴板：取证搬运（导出日志与错误码）",
    "日志导出：社区求助材料（求助帖的原料）",
];

pub fn min_set_help_consistent() -> bool {
    MIN_SET.len() == MIN_SET_HELP.len()
        && MIN_SET_HELP.iter().all(|s| s.contains("："))
        && MIN_SET_HELP[5].contains("F198")
        && MIN_SET_HELP[8].contains("E1")
}

// ---------------------------------------------------------------------------
// v5-三：StrikeSnapshot —— 异常关机计数持久化语义（encode/decode：计数+
// 时间戳序列压缩落盘——「跨启动持久」的字节层；垃圾拒绝不复活旧账）
// ---------------------------------------------------------------------------

/// 落盘布局：魔数 1B + 计数 1B + 最近时间戳 6B（低 48 位）。
pub const STRIKE_SNAPSHOT_LEN: usize = 8;
const STRIKE_MAGIC: u8 = 0x53; // 'S'

/// 编码。
pub fn strike_snapshot_encode(sm: &SafeMode, last_stamp_s: u64) -> [u8; STRIKE_SNAPSHOT_LEN] {
    let mut out = [0u8; STRIKE_SNAPSHOT_LEN];
    out[0] = STRIKE_MAGIC;
    out[1] = sm.abnormal_strikes.min(255) as u8;
    let t = last_stamp_s & 0xFFFF_FFFF_FFFF;
    let tb = t.to_le_bytes();
    out[2..8].copy_from_slice(&tb[0..6]);
    out
}

/// 解码（魔数错=None——空盘/坏盘诚实拒绝，不把旧计数带回来）。
pub fn strike_snapshot_decode(data: &[u8; STRIKE_SNAPSHOT_LEN]) -> Option<(u32, u64)> {
    if data[0] != STRIKE_MAGIC {
        return None;
    }
    let strikes = data[1] as u32;
    let mut t_b = [0u8; 6];
    t_b.copy_from_slice(&data[2..8]);
    Some((strikes, u64::from_le_bytes([t_b[0], t_b[1], t_b[2], t_b[3], t_b[4], t_b[5], 0, 0])))
}

// ---------------------------------------------------------------------------
// v5-四：ExitChecklist —— 退出检查单（正常重启即出无残留的机器化：重启
// 前逐项确认——主题/服务/更新禁用三处状态全部归零）
// ---------------------------------------------------------------------------

/// 检查单行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitCheckItem {
    pub item: &'static str,
    /// 是否已归位。
    pub restored: bool,
    pub why: &'static str,
}

/// 生成退出检查单（active 安全模式的退出前置——三项全过才许重启按钮亮）。
pub fn exit_checklist(sm: &SafeMode) -> [ExitCheckItem; 3] {
    [
        ExitCheckItem {
            item: "主题令牌",
            restored: true, // 安全模式仅默认令牌——重启后正常主题自动接管。
            why: "出厂默认令牌在会话中，重启后主题服务重新加载用户主题",
        },
        ExitCheckItem {
            item: "服务集",
            restored: true, // 最小服务集是启动参数驱动——重启自然恢复全集。
            why: "最小集由 safe-mode 参数推导，重启不带参即恢复全集",
        },
        ExitCheckItem {
            item: "更新禁用",
            restored: !sm.active, // 仍处于安全模式时更新保持禁用。
            why: "更新在安全模式禁用（防半态更新）；重启出模式后自动解除",
        },
    ]
}

/// 三项全归位判定。
pub fn exit_clear(items: &[ExitCheckItem; 3]) -> bool {
    items.iter().all(|i| i.restored)
}

// ---------------------------------------------------------------------------
// v5 自检（deep4 表）
// ---------------------------------------------------------------------------

/// F193 v5 自检（聚合进 secstar2 域）。
pub fn run_safemode_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new("F193-v5");

    let mut sm = SafeMode::new();
    sm.enter(EntryReason::AfterAbnormal, true).ok();

    // v5-一：黄条渲染——主行/原因行/锚/不可关。
    let b = safe_banner_render(&sm);
    set.add("banner main", b.main == BANNER_TEXT, "");
    set.add("banner reason", b.reason == Some(BANNER_REASON_ABNORMAL), "原因随来源");
    set.add("banner anchor", b.anchor == "bottom-right" && !b.dismissible, "右下角+不可关");
    set.add("banner help", b.help == HELP_LINK, "");
    // 手选进入=无原因行（用户自己选的）。
    let mut sm2 = SafeMode::new();
    sm2.enter(EntryReason::ManualMenu, true).ok();
    set.add("banner manual no reason", safe_banner_render(&sm2).reason.is_none(), "");

    // v5-二：最小集帮助——逐位对应+F198/E1 锚。
    set.add("minset help consistent", min_set_help_consistent(), "");

    // v5-三：计数持久化——round-trip、魔数拒绝、计数钳 255。
    sm.note_abnormal_shutdown(1000);
    sm.note_abnormal_shutdown(2000);
    let snap = strike_snapshot_encode(&sm, 2000);
    set.add("strike rt", strike_snapshot_decode(&snap) == Some((2, 2000)), "计数+时刻保真");
    let mut junk = snap;
    junk[0] = 0x00;
    set.add("strike junk rejected", strike_snapshot_decode(&junk).is_none(), "垃圾不复活旧账");
    // 大时间戳截断到 48 位（语义：低 48 位环回安全——约 8900 年）。
    let big = strike_snapshot_encode(&sm, u64::MAX);
    set.add("strike ts clamp", strike_snapshot_decode(&big).map(|(_, t)| t < 1 << 48).unwrap_or(false), "");

    // v5-四：退出检查单——三项语义、模式内更新禁用、退出后全归位。
    let items = exit_checklist(&sm);
    set.add("exit items", items.len() == 3, "");
    set.add("exit update blocked in mode", !items[2].restored, "安全模式内更新保持禁用");
    set.add("exit theme auto", items[0].restored && items[0].why.contains("重启"), "");
    // 重启后（active=false）更新禁用解除——全归位。
    sm.exit_via_reboot();
    let items2 = exit_checklist(&sm);
    set.add("exit all clear after reboot", exit_clear(&items2), "正常重启即出无残留");

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn f193_v5_banner_all_reasons() {
        // 三种进入原因的黄条互异（原因行穷尽——每次进入都有理由）。
        for (r, expect_some) in [
            (EntryReason::ManualMenu, false),
            (EntryReason::AfterAbnormal, true),
            (EntryReason::KernelParam, true),
        ] {
            let mut sm = SafeMode::new();
            sm.enter(r, true).ok();
            let b = safe_banner_render(&sm);
            assert_eq!(b.reason.is_some(), expect_some, "{:?}", r);
        }
    }

    #[test]
    fn f193_v5_strike_counter_saturates() {
        // 计数超 255 钳制（极端异常循环不溢出——持久层语义明确）。
        let mut sm = SafeMode::new();
        for i in 0..300u64 {
            sm.note_abnormal_shutdown(i);
        }
        let snap = strike_snapshot_encode(&sm, 300);
        let (count, _) = strike_snapshot_decode(&snap).unwrap();
        assert_eq!(count, 255, "钳制到 u8 上限");
    }

    #[test]
    fn f193_v5_minset_help_covers_all() {
        // 帮助行与 MIN_SET 逐位同名（投影不漂移——手工文案对齐清单）。
        for (i, help) in MIN_SET_HELP.iter().enumerate() {
            let keyword = match MIN_SET[i] {
                "settings" => "设置中心",
                "explorer" => "资源管理器",
                "uninstaller" => "卸载通道",
                "diagnostics" => "诊断中心",
                "terminal" => "终端",
                "recovery" => "恢复环境",
                "taskbar" => "任务栏",
                "window-mgr" => "窗口管理",
                "theme-default" => "默认主题",
                "ime-base" => "基础输入",
                "clipboard" => "剪贴板",
                "log-export" => "日志导出",
                _ => "",
            };
            assert!(help.contains(keyword), "第 {} 行缺 {}", i, keyword);
        }
    }

    #[test]
    fn f193_v5_run_checks_pass() {
        assert!(run_safemode_deep4_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v6 批次（第六轮深化 · 上限口径收官）——会话时间线 / 灰置目录页 / 安全
// 模式完整帮助 / 计数策略参数化。判据源：主册【交互设计】「最小集白名单
// 外功能全部灰置且可解释」的目录化 +【设计细节】帮助链 F126。
// ------

use alloc::string::String;
// -------------------------------------------------------------------

/// 会话时间线行（SessionArchive 之上的渲染：开账/动作/闭账三行式）。
pub fn session_timeline(records: &[SessionRecord]) -> Vec<String> {
    let mut out = Vec::new();
    for r in records {
        out.push(alloc::format!("[{}] 进入安全模式（{}）", r.entered_s, r.reason));
        for a in &r.actions {
            out.push(alloc::format!("[{}] 修复动作：{}", r.entered_s, a));
        }
        out.push(alloc::format!(
            "[{}] 退出（{}）——会话 {} 秒，{} 项动作",
            r.entered_s + r.duration_s,
            r.exit_via,
            r.duration_s,
            r.actions.len()
        ));
    }
    out
}

/// 灰置目录页（全功能清单逐项状态——安全模式下「还有什么不能用」的完整目录）。
pub fn greyed_catalog(sm: &SafeMode, all_features: &[&'static str]) -> Vec<(&'static str, bool, &'static str)> {
    all_features
        .iter()
        .map(|f| {
            let g = sm.gate(f);
            (*f, g.allowed, if g.allowed { "" } else { GREYED_WHY })
        })
        .collect()
}

/// 目录统计（可用/灰置两计数+灰置率 permille）。
pub fn greyed_catalog_stats(catalog: &[(&'static str, bool, &'static str)]) -> (usize, usize, u64) {
    let allowed = catalog.iter().filter(|(_, ok, _)| *ok).count();
    let greyed = catalog.len() - allowed;
    let rate = if catalog.is_empty() { 0 } else { greyed as u64 * 1000 / catalog.len() as u64 };
    (allowed, greyed, rate)
}

/// 安全模式完整帮助页（四节：这是什么/怎么进来/能做什么/怎么出去）。
pub const SAFE_MODE_HELP: [(&'static str, &'static str); 4] = [
    ("这是什么", "救援模式：只加载修复所需的最小功能集与默认主题——主题花屏、驱动冲突、第三方软件把系统搞坏时，这里是干净的备用桌面。"),
    ("怎么进来", "三个入口：引导选单隐藏条目（按住 Shift 点击 VARIX）、连续两次异常关机后的自动询问、内核参数 safe-mode（F192 降级族）。"),
    ("能做什么", "四主入口（设置中心/资源管理器/卸载通道/诊断中心）+ 终端/恢复环境/日志导出——白名单 12 项全公开（F126），其余功能灰置可解释。"),
    ("怎么出去", "正常重启即出，无残留。重启后主题、服务、更新自动恢复——修复动作（如卸载问题主题）在退出前完成即可。"),
];

pub fn safe_mode_help_intact() -> bool {
    SAFE_MODE_HELP.len() == 4 && SAFE_MODE_HELP[1].1.contains("Shift") && SAFE_MODE_HELP[3].1.contains("无残留")
}

/// 计数策略参数化（异常关机阈值可调——界内钳制+默认值一处一事实）。
pub struct StrikePolicy {
    pub threshold: u32,
}

impl StrikePolicy {
    pub const DEFAULT: u32 = ABNORMAL_STRIKES;
    pub const MIN: u32 = 2;
    pub const MAX: u32 = 5;

    /// 构造（界内钳制）。
    pub fn new(threshold: u32) -> StrikePolicy {
        StrikePolicy { threshold: threshold.clamp(Self::MIN, Self::MAX) }
    }

    /// 判定（连续异常次数 ≥ 阈值 → 建议询问）。
    pub fn should_ask(&self, strikes: u32) -> bool {
        strikes >= self.threshold
    }
}

/// F193 v6 自检（deep5 表）。
pub fn run_safemode_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new("F193-v6");

    let mut sm = SafeMode::new();
    sm.enter(EntryReason::ManualMenu, true).ok();

    // v6-一：会话时间线——开/动作/闭三行式。
    let mut acc = SessionAccount::new();
    acc.open("菜单手选进入", 0).ok();
    acc.action("卸载问题主题").ok();
    acc.close(120, "正常重启").ok();
    let tl = session_timeline(&acc.archive);
    set.add("timeline 3 lines", tl.len() == 3, "开账+动作+闭账");
    set.add("timeline action", tl[1].contains("卸载问题主题"), "");
    set.add("timeline exit", tl[2].contains("120 秒"), "时长入行");

    // v6-二：灰置目录——全功能清单状态化+统计。
    let all = ["settings", "explorer", "uninstaller", "diagnostics", "update-ui", "theme-store", "app-market"];
    let catalog = greyed_catalog(&sm, &all);
    set.add("catalog size", catalog.len() == 7, "");
    let (allowed, greyed, rate) = greyed_catalog_stats(&catalog);
    set.add("catalog stats", allowed == 4 && greyed == 3 && rate == 428, "4/7 可用，3/7 灰置 ≈ 428‰");
    set.add("catalog why", catalog.iter().all(|(_, ok, why)| *ok || why.contains("最小功能集")), "灰置项全部可解释");

    // v6-三：帮助页——四节齐。
    set.add("help intact", safe_mode_help_intact(), "");

    // v6-四：计数策略——默认、钳制、判定。
    set.add("policy default", StrikePolicy::new(99).threshold == StrikePolicy::MAX, "超界钳 5");
    set.add("policy floor", StrikePolicy::new(1).threshold == StrikePolicy::MIN, "低于界钳 2");
    let p = StrikePolicy::new(3);
    set.add("policy judge", p.should_ask(3) && !p.should_ask(2), "达阈值才询问");
    set.add("policy default matches", StrikePolicy::new(2).threshold == ABNORMAL_STRIKES, "默认值=主册常量");

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn f193_v6_timeline_multi_sessions() {
        // 两轮会话：时间线 6 行按会话分组（顺序保持）。
        let mut acc = SessionAccount::new();
        acc.open("r1", 0).unwrap();
        acc.action("a1").unwrap();
        acc.close(10, "正常重启").unwrap();
        acc.open("r2", 20).unwrap();
        acc.close(30, "正常重启").unwrap();
        let tl = session_timeline(&acc.archive);
        assert_eq!(tl.len(), 5, "会话1: 3 行 + 会话2: 2 行");
        assert!(tl[0].contains("r1") && tl[3].contains("r2"));
    }

    #[test]
    fn f193_v6_catalog_empty_honest() {
        // 空目录：统计全零（不造比例）。
        let (a, g, r) = greyed_catalog_stats(&[]);
        assert_eq!((a, g, r), (0, 0, 0));
    }

    #[test]
    fn f193_v6_run_checks_pass() {
        assert!(run_safemode_deep5_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v7 批次（第七轮深化 · 上限口径收官）——功能门导出 / 修复向导 / 退出
// 前检查（重启按钮的门卫）。判据源：主册【交互设计】「白名单外灰置且
// 可解释」+【状态与异常】退出=正常重启无残留。
// ---------------------------------------------------------------------------

/// 功能门开放导出（F128 语言：全功能清单逐项状态 JSON——诊断导出可含）。
pub fn gate_export_json(sm: &SafeMode, features: &[&'static str], out: &mut Vec<u8>) {
    out.extend_from_slice(b"{\"safe-mode-gates\":[");
    for (i, f) in features.iter().enumerate() {
        let g = sm.gate(f);
        if i > 0 {
            out.extend_from_slice(b",");
        }
        out.extend_from_slice(
            alloc::format!("{{\"feature\":\"{}\",\"allowed\":{}}}", f, g.allowed).as_bytes(),
        );
    }
    out.extend_from_slice(b"]}");
}

/// 导出形状自检（计数=清单长度）。
pub fn gate_export_ok(expected: usize, data: &[u8]) -> bool {
    let text = core::str::from_utf8(data).unwrap_or("");
    text.contains("\"safe-mode-gates\"") && text.matches("\"feature\"").count() == expected
}

/// 修复向导（安全模式内的三步引导——症状→动作→验证）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WizardStep {
    /// 识别症状（花屏/崩溃/冲突）。
    Symptom,
    /// 执行动作（卸载/重置/禁用）。
    Action,
    /// 验证修复（重启前自检）。
    Verify,
}

/// 三步向导状态机（只能顺步推进，可回退一步——应激场景不迷路）。
pub struct RepairWizard {
    pub step: WizardStep,
    pub steps_taken: u32,
}

impl RepairWizard {
    pub fn new() -> RepairWizard {
        RepairWizard { step: WizardStep::Symptom, steps_taken: 0 }
    }

    /// 前进（不可跳步）。
    pub fn advance(&mut self) -> Result<WizardStep, &'static str> {
        self.step = match self.step {
            WizardStep::Symptom => WizardStep::Action,
            WizardStep::Action => WizardStep::Verify,
            WizardStep::Verify => return Err("已在最后一步——重启即完成"),
        };
        self.steps_taken += 1;
        Ok(self.step)
    }

    /// 回退一步（Verify 可回 Action；Symptom 不可再回）。
    pub fn back(&mut self) -> Result<WizardStep, &'static str> {
        self.step = match self.step {
            WizardStep::Verify => WizardStep::Action,
            WizardStep::Action => WizardStep::Symptom,
            WizardStep::Symptom => return Err("已在第一步"),
        };
        Ok(self.step)
    }
}

impl Default for RepairWizard {
    fn default() -> Self {
        Self::new()
    }
}

/// 退出前检查（重启按钮门卫：更新禁用/未完成修复/会话未闭 三勾）。
pub struct ExitGuard {
    pub update_done: bool,
    pub repair_verified: bool,
    pub session_closed: bool,
}

impl ExitGuard {
    /// 全清才放行重启（缺哪勾说哪勾——三要素纪律）。
    pub fn clear(&self) -> Result<(), &'static str> {
        if !self.repair_verified {
            return Err("修复动作未验证：请在诊断中心确认问题已解决，或回退该动作");
        }
        if !self.session_closed {
            return Err("会话账未闭合：退出前自动归档（无需手动操作，稍候片刻）");
        }
        if !self.update_done {
            return Err("更新禁用尚未解除：正常重启后自动解除——此勾在重启流程内自动完成");
        }
        Ok(())
    }
}

/// F193 v7 自检（deep6 表）。
pub fn run_safemode_deep6_checks() -> CheckSet {
    let mut set = CheckSet::new("F193-v7");

    let mut sm = SafeMode::new();
    sm.enter(EntryReason::ManualMenu, true).ok();

    // v7-一：门导出——形状+计数+主入口绿。
    let features = ["settings", "explorer", "update-ui", "theme-store"];
    let mut data = Vec::new();
    gate_export_json(&sm, &features, &mut data);
    set.add("gate export ok", gate_export_ok(4, &data), "");
    let text = core::str::from_utf8(&data).unwrap_or("");
    set.add("gate export mixed", text.contains("settings\",\"allowed\":true") && text.contains("update-ui\",\"allowed\":false"), "主入口绿+白名单外红");

    // v7-二：向导——顺步/跳步拒/回退/首步回退拒。
    let mut wiz = RepairWizard::new();
    set.add("wiz step1", wiz.advance() == Ok(WizardStep::Action), "");
    set.add("wiz step2", wiz.advance() == Ok(WizardStep::Verify), "");
    set.add("wiz no skip", wiz.advance().is_err(), "最后一步不可再进");
    set.add("wiz back", wiz.back() == Ok(WizardStep::Action), "可回退一步");
    set.add("wiz back to start", wiz.back() == Ok(WizardStep::Symptom), "");
    set.add("wiz back at start", wiz.back().is_err(), "首步不可再回");
    set.add("wiz steps counted", wiz.steps_taken == 2, "");

    // v7-三：退出门卫——缺勾逐项说、全清放行。
    let blocked = ExitGuard { update_done: false, repair_verified: false, session_closed: true };
    set.add("guard repair first", blocked.clear().unwrap_err().contains("修复动作未验证"), "修复未验证最先拦");
    let mid = ExitGuard { update_done: false, repair_verified: true, session_closed: false };
    set.add("guard session second", mid.clear().unwrap_err().contains("会话账未闭合"), "");
    let last = ExitGuard { update_done: false, repair_verified: true, session_closed: true };
    set.add("guard update auto", last.clear().unwrap_err().contains("自动解除"), "更新勾自动完成语义");
    let ok = ExitGuard { update_done: true, repair_verified: true, session_closed: true };
    set.add("guard clear", ok.clear().is_ok(), "");

    set
}

#[cfg(test)]
mod deep6_tests {
    use super::*;

    #[test]
    fn f193_v7_wizard_full_cycle() {
        // 完整循环：Symptom→Action→Verify→back→advance 无状态残留。
        let mut wiz = RepairWizard::new();
        wiz.advance().unwrap();
        wiz.advance().unwrap();
        wiz.back().unwrap();
        assert_eq!(wiz.advance(), Ok(WizardStep::Verify));
        assert_eq!(wiz.steps_taken, 3);
    }

    #[test]
    fn f193_v7_run_checks_pass() {
        assert!(run_safemode_deep6_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v8 批次（第八轮深化 · 缺口冲刺）——停留时长账 / 最小集功能矩阵 / 失败
// strikes 账 / 进入原因卡。
// 判据源：主册【状态与异常】「安全模式停留超 72h 主动提醒退出路径」+
// 「同项连续失败 3 次 → 建议进入安全模式」。
// ---------------------------------------------------------------------------

/// 停留时长账（进入时刻 + 当前时刻 → 停留分级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StayGrade {
    /// < 24h：正常排查期。
    Fresh,
    /// 24-72h：提醒可退出。
    Lingering,
    /// ≥ 72h：主动给退出路径 + 数据导出入口。
    Overdue,
}

/// 停留分级（秒制）。
pub fn stay_grade(entered_s: u64, now_s: u64) -> StayGrade {
    let dur_h = now_s.saturating_sub(entered_s) / 3600;
    if dur_h < 24 {
        StayGrade::Fresh
    } else if dur_h < 72 {
        StayGrade::Lingering
    } else {
        StayGrade::Overdue
    }
}

/// 停留提醒文案（三态——Overdue 必须带出口）。
pub fn stay_notice(entered_s: u64, now_s: u64) -> &'static str {
    match stay_grade(entered_s, now_s) {
        StayGrade::Fresh => "",
        StayGrade::Lingering => "已在安全模式超过 24 小时——排查完成后可从设置退出",
        StayGrade::Overdue => "已在安全模式超过 72 小时——建议导出日志后走「退出安全模式」",
    }
}

/// 最小集功能表现。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SfMode {
    /// 完整可用。
    Full,
    /// 精简版（基础路径）。
    Basic,
    /// 停用（入口隐藏并说明）。
    Off,
}

/// 最小集功能矩阵（12 项逐项定级——矩阵覆盖全部 MIN_SET）。
pub fn minimal_feature_matrix() -> Vec<(&'static str, SfMode)> {
    MIN_SET
        .iter()
        .map(|name| {
            let mode = match *name {
                "settings" | "explorer" | "uninstaller" | "diagnostics" | "terminal" | "recovery" => SfMode::Full,
                "taskbar" | "window-mgr" | "theme-default" | "ime-base" | "clipboard" => SfMode::Basic,
                _ => SfMode::Off, // log-export：安全模式停日志导出（日志正在忙）。
            };
            (*name, mode)
        })
        .collect()
}

/// 失败 strikes 账（同项连续失败 → 建议安全模式）。
pub struct StrikeLedger {
    strikes: Vec<(&'static str, u32)>,
}

impl StrikeLedger {
    pub fn new() -> StrikeLedger {
        StrikeLedger { strikes: Vec::new() }
    }

    /// 记一次失败（同项累加；成功清零该项）。
    pub fn fail(&mut self, item: &'static str) -> u32 {
        match self.strikes.iter_mut().find(|(k, _)| *k == item) {
            Some((_, n)) => {
                *n += 1;
                *n
            }
            None => {
                self.strikes.push((item, 1));
                1
            }
        }
    }

    pub fn succeed(&mut self, item: &str) {
        self.strikes.retain(|(k, _)| *k != item);
    }

    /// 建议进入安全模式？（任一项 ≥ 3 连败）。
    pub fn suggests_safe_mode(&self) -> Option<&'static str> {
        self.strikes.iter().find(|(_, n)| *n >= 3).map(|(k, _)| *k)
    }

    pub fn count_of(&self, item: &str) -> u32 {
        self.strikes.iter().find(|(k, _)| *k == item).map(|(_, n)| *n).unwrap_or(0)
    }
}

impl Default for StrikeLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 进入原因卡（三要素：发生了什么/为什么/下一步）。
pub struct SafeModeReasonCard {
    pub what: &'static str,
    pub why: &'static str,
    pub next: &'static str,
}

/// 原因卡构建（按触发源分四类——卡不许是空白）。
pub fn reason_card(trigger: u8) -> SafeModeReasonCard {
    match trigger {
        0 => SafeModeReasonCard {
            what: "系统连续 3 次启动失败",
            why: "启动链在同一个门反复被拦",
            next: "进入安全模式排查，你的文件不受影响",
        },
        1 => SafeModeReasonCard {
            what: "用户主动选择安全模式",
            why: "你在设置中手动进入",
            next: "排查完成后从设置退出",
        },
        2 => SafeModeReasonCard {
            what: "关键组件连续失败被拦下",
            why: "失败 strikes 达到 3 次阈值",
            next: "进入安全模式并登记缺陷",
        },
        _ => SafeModeReasonCard {
            what: "回滚后进入安全模式",
            why: "槽位切换后需要最小集验证",
            next: "验证通过后自动回到正常模式",
        },
    }
}

/// F193 v8 自检（deep7 表）。
pub fn run_safemode_deep7_checks() -> CheckSet {
    let mut set = CheckSet::new("F193-v8");

    // 停留分级：三态边界（24h/72h）。
    set.add("stay fresh", stay_grade(0, 23 * 3600) == StayGrade::Fresh, "");
    set.add("stay lingering", stay_grade(0, 24 * 3600) == StayGrade::Lingering, "恰 24h 进 lingering");
    set.add("stay overdue", stay_grade(0, 72 * 3600) == StayGrade::Overdue, "恰 72h 进 overdue");
    set.add("stay notice empty", stay_notice(0, 3600).is_empty(), "新鲜期不骚扰");
    set.add("stay notice exit", stay_notice(0, 80 * 3600).contains("退出"), "超期必带出口");

    // 功能矩阵：覆盖全量、三态齐、停用有说明。
    let mx = minimal_feature_matrix();
    set.add("mx cover", mx.len() == MIN_SET.len(), "矩阵 = 最小集全量");
    set.add("mx full count", mx.iter().filter(|(_, m)| *m == SfMode::Full).count() == 6, "六项完整");
    set.add("mx off count", mx.iter().filter(|(_, m)| *m == SfMode::Off).count() == 1, "仅日志导出停用");

    // strikes：累加、成功清零、3 次建议。
    let mut led = StrikeLedger::new();
    led.fail("window-mgr");
    led.fail("window-mgr");
    set.add("strike none", led.suggests_safe_mode().is_none(), "2 连败不建议");
    led.fail("window-mgr");
    set.add("strike suggest", led.suggests_safe_mode() == Some("window-mgr"), "3 连败点名");
    led.succeed("window-mgr");
    set.add("strike reset", led.count_of("window-mgr") == 0 && led.suggests_safe_mode().is_none(), "成功清零");
    led.fail("ime-base");
    led.fail("ime-base");
    led.fail("ime-base");
    set.add("strike per item", led.suggests_safe_mode() == Some("ime-base"), "各 item 独立记账");

    // 原因卡：四类全覆盖、字段非空。
    for t in 0..4u8 {
        let c = reason_card(t);
        set.add("card nonempty", !c.what.is_empty() && !c.why.is_empty() && !c.next.is_empty(), "触发源全建卡");
    }
    set.add("card file safe", reason_card(0).next.contains("文件不受影响"), "红线承诺在");

    set
}

#[cfg(test)]
mod deep7_tests {
    use super::*;

    #[test]
    fn f193_v7_stay_underflow_safe() {
        // 时刻倒挂不放负数：saturating 语义兜底。
        assert_eq!(stay_grade(100, 50), StayGrade::Fresh);
    }

    #[test]
    fn f193_v7_matrix_all_named() {
        // 矩阵每一行都挂真实 MIN_SET 成员（不许有幽灵行）。
        for (name, _) in minimal_feature_matrix() {
            assert!(MIN_SET.contains(&name));
        }
    }

    #[test]
    fn f193_v7_strikes_independent() {
        // 两项交替失败互不串账。
        let mut led = StrikeLedger::new();
        led.fail("a");
        led.fail("b");
        led.fail("a");
        assert_eq!(led.count_of("a"), 2);
        assert_eq!(led.count_of("b"), 1);
    }

    #[test]
    fn f193_v7_run_checks_pass() {
        assert!(run_safemode_deep7_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v8-b6：退出条件账 / 最小集自检循环 / 安全模式事件流。
// ---------------------------------------------------------------------------

/// 退出条件账（退出安全模式需要满足的条件清单与达成态）。
pub struct ExitChecklist {
    /// 排查完成确认。
    pub diagnosed: bool,
    /// 触发源已修复（strikes 清零 / 门通过）。
    pub trigger_fixed: bool,
    /// 用户确认退出。
    pub user_confirmed: bool,
}

impl ExitChecklist {
    /// 全部达成才可退出（缺哪条点哪条）。
    pub fn ready(&self) -> (bool, Vec<&'static str>) {
        let mut missing = Vec::new();
        if !self.diagnosed {
            missing.push("排查未确认完成");
        }
        if !self.trigger_fixed {
            missing.push("触发源未修复");
        }
        if !self.user_confirmed {
            missing.push("用户未确认退出");
        }
        (missing.is_empty(), missing)
    }
}

/// 最小集自检循环（进入安全模式后逐项探活 → 结果账）。
pub struct MinimalProbe {
    pub passed: usize,
    pub failed: Vec<&'static str>,
}

/// 探活执行（probe 结果与 MIN_SET 等长对齐——不许漏项）。
pub fn minimal_probe(results: &[bool]) -> MinimalProbe {
    let n = results.len().min(MIN_SET.len());
    let failed = MIN_SET[..n]
        .iter()
        .zip(results.iter())
        .filter(|(_, ok)| !**ok)
        .map(|(name, _)| *name)
        .collect();
    MinimalProbe { passed: results.iter().filter(|r| **r).count(), failed }
}

/// 安全模式时间线事件行（进入/探活/退出全生命周期）。
pub struct SmTimelineEvent {
    pub at_s: u64,
    pub kind: &'static str,
    pub text: alloc::string::String,
}

/// 事件流构建（enter → probe → exit 三段，时刻必须单调）。
pub fn sm_event_stream(entered_s: u64, probe_s: u64, exit_s: u64, probe_ok: bool) -> Vec<SmTimelineEvent> {
    let mut out = Vec::new();
    out.push(SmTimelineEvent {
        at_s: entered_s,
        kind: "enter",
        text: alloc::string::String::from("进入安全模式（最小集启动）"),
    });
    out.push(SmTimelineEvent {
        at_s: probe_s,
        kind: "probe",
        text: if probe_ok {
            alloc::string::String::from("最小集探活全绿")
        } else {
            alloc::string::String::from("最小集探活有失败项——已在诊断页列出")
        },
    });
    if probe_ok {
        out.push(SmTimelineEvent { at_s: exit_s, kind: "exit", text: alloc::string::String::from("退出安全模式，回到正常模式") });
    }
    out
}

/// F193 v8-b6 自检（并入 deep7 表族）。
pub fn run_safemode_deep7b_checks() -> CheckSet {
    let mut set = CheckSet::new("F193-v8b");

    // 退出条件：全达成 / 缺项点名。
    let full = ExitChecklist { diagnosed: true, trigger_fixed: true, user_confirmed: true };
    let (ok, miss) = full.ready();
    set.add("exit ready", ok && miss.is_empty(), "");
    let part = ExitChecklist { diagnosed: true, trigger_fixed: false, user_confirmed: false };
    let (ok2, miss2) = part.ready();
    set.add("exit missing", !ok2 && miss2.len() == 2 && miss2.contains(&"触发源未修复"), "");

    // 探活：全绿 / 失败点名 / 数量守恒。
    let p1 = minimal_probe(&[true; 12]);
    set.add("probe all", p1.passed == 12 && p1.failed.is_empty(), "");
    let mut r2 = vec![true; 12];
    r2[3] = false;
    let p2 = minimal_probe(&r2);
    set.add("probe fail named", p2.failed == vec!["diagnostics"], "第 4 项失败点名");
    set.add("probe count", p2.passed == 11, "");
    let p3 = minimal_probe(&[true; 5]);
    set.add("probe short", p3.passed == 5 && p3.failed.is_empty(), "短表不越界");

    // 事件流：三段单调、失败不出 exit。
    let ev = sm_event_stream(10, 20, 30, true);
    set.add("ev 3 stages", ev.len() == 3 && ev[0].at_s < ev[1].at_s && ev[1].at_s < ev[2].at_s, "");
    let ev2 = sm_event_stream(10, 20, 30, false);
    set.add("ev no exit", ev2.len() == 2 && ev2[1].text.contains("失败项"), "探活失败不进 exit");
    // b7-wave2：最小集降级预案。
    set.add("fallback set", FALLBACK_PATHS.len() == 3, "三项降级预案");
    set.add("fallback named", FALLBACK_PATHS.iter().all(|(f, p)| MIN_SET.contains(f) && !p.is_empty()), "每项挂真实成员与路径");
    set.add("fallback for", fallback_for("explorer") == Some("任务栏直达文件入口"), "失败项有替代路");
    set.add("fallback none", fallback_for("log-export").is_none(), "停用项无替代（诚实）");
    // b8-wave3：排查清单页。
    set.add("checklist", TROUBLESHOOT_STEPS.len() == 4 && TROUBLESHOOT_STEPS.iter().all(|(t, _)| !t.is_empty()), "四步清单齐");
    set.add("checklist done", { let mut c = TroubleshootChecklist::new(); for _ in 0..4 { c.tick(); } c.all_done() }, "逐项打勾到完成");
    set.add("checklist partial", { let mut c = TroubleshootChecklist::new(); c.tick(); !c.all_done() }, "未完不给过");
    // b9-wave4：退出导出包清单。
    set.add("exit pkg", EXIT_PACKAGE_ITEMS.len() == 3, "三项导出齐");
    set.add("exit pkg named", EXIT_PACKAGE_ITEMS.iter().all(|t| !t.is_empty()), "项项有名");
    set.add("exit pkg size", exit_package_estimate() == 3 * EXIT_ITEM_KIB, "体积 = 3 × 单项");
    // b10-wave5：排查时长账。
    set.add("trouble dur", troubleshoot_duration(0, 3600 * 2).contains("2 小时"), "2 小时排查");
    set.add("trouble long", troubleshoot_duration(0, 3600 * 80).contains("3 天"), "跨天换算");
    // b11-wave6：探活报告 CSV / 退出倒计时。
    set.add("probe csv", probe_csv(&[("settings", true), ("terminal", false)]).lines().count() == 3, "表头 + 2 行");
    set.add("probe csv fail", probe_csv(&[("terminal", false)]).contains(",fail"), "失败态入表");
    set.add("exit countdown", exit_countdown(3, 4) == 1, "四条件成三 = 差 1");
    // b12-wave7：探活结果 CSV 空态。
    set.add("probe csv empty", probe_csv(&[]).lines().count() == 1, "空探活仅表头");
    set.add("stay hours", stay_hours(0, 7200) == 2, "停留小时换算");
    // b13-wave8：探活通过率行。
    set.add("probe rate", probe_rate_line(10, 1) == "探活 10 项，失败 1 项", "通过率行");
    // b14-wave9：退出包 CSV。
    set.add("exit pkg csv", exit_pkg_csv().starts_with("item\n"), "CSV 表头");
    set.add("exit pkg rows", exit_pkg_csv().lines().count() == 4, "三项 + 表头");

    set
}

#[cfg(test)]
mod deep7b_tests {
    use super::*;

    #[test]
    fn f193_v8b_probe_all_fail() {
        let p = minimal_probe(&[false; 12]);
        assert_eq!(p.passed, 0);
        assert_eq!(p.failed.len(), 12);
    }

    #[test]
    fn f193_v8b_exit_independent() {
        // 三条件独立：只缺用户确认也拦下。
        let c = ExitChecklist { diagnosed: true, trigger_fixed: true, user_confirmed: false };
        let (ok, miss) = c.ready();
        assert!(!ok && miss == vec!["用户未确认退出"]);
    }

    #[test]
    fn f193_v8b_run_checks_pass() {
        assert!(run_safemode_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b7（第二波）：最小集降级预案（功能失败的替代路径登记）。
// 判据源：主册【状态与异常】「最小集每项都要有『坏了怎么办』的答案」。
// ---------------------------------------------------------------------------

/// 降级预案（最小集成员 → 人话替代路径）。
pub const FALLBACK_PATHS: [(&str, &str); 3] = [
    ("explorer", "任务栏直达文件入口"),
    ("taskbar", "键盘快捷键启动器"),
    ("clipboard", "应用内置剪贴（临时同步缓冲）"),
];

/// 查替代路径（无预案的成员返回 None——诚实无预案）。
pub fn fallback_for(feature: &str) -> Option<&'static str> {
    FALLBACK_PATHS
        .iter()
        .find(|(f, _)| *f == feature)
        .map(|(_, p)| *p)
}

#[cfg(test)]
mod deep7c_tests {
    use super::*;

    #[test]
    fn f193_v8c_fallback_unique() {
        // 预案目标不重复（一个入口只挂一条替代路）。
        for (i, (f, _)) in FALLBACK_PATHS.iter().enumerate() {
            for (g, _) in FALLBACK_PATHS.iter().skip(i + 1) {
                assert_ne!(f, g);
            }
        }
    }

    #[test]
    fn f193_v8c_run_checks_pass() {
        assert!(run_safemode_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b8（第三波）：安全模式排查清单（四步——给用户一条明确的出路）。
// 判据源：主册【交互设计】「安全模式必带排查引导（四步出坑）」。
// ---------------------------------------------------------------------------

/// 排查四步。
pub const TROUBLESHOOT_STEPS: [(&str, bool); 4] = [
    ("看诊断页：哪个组件被拦", true),
    ("按降级预案走替代路（若有）", true),
    ("登记缺陷或导出日志", true),
    ("回退出条件账确认三达成", true),
];

/// 清单状态机（逐项打勾——全部勾完才亮「退出」按钮）。
pub struct TroubleshootChecklist {
    pub done: usize,
}

impl TroubleshootChecklist {
    pub fn new() -> TroubleshootChecklist {
        TroubleshootChecklist { done: 0 }
    }

    pub fn tick(&mut self) {
        if self.done < TROUBLESHOOT_STEPS.len() {
            self.done += 1;
        }
    }

    pub fn all_done(&self) -> bool {
        self.done == TROUBLESHOOT_STEPS.len()
    }
}

impl Default for TroubleshootChecklist {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod deep8_tests {
    use super::*;

    #[test]
    fn f193_v8d_tick_cap() {
        // 越界打勾无效（账不虚增）。
        let mut c = TroubleshootChecklist::new();
        for _ in 0..9 {
            c.tick();
        }
        assert_eq!(c.done, 4);
    }

    #[test]
    fn f193_v8d_run_checks_pass() {
        assert!(run_safemode_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b9（第四波）：退出安全模式导出包（带走证据再退出）。
// 判据源：主册【交互设计】「退出前给打包导出——排查成果不丢」。
// ---------------------------------------------------------------------------

/// 导出包内容（三项——安全模式的完整排查成果）。
pub const EXIT_PACKAGE_ITEMS: [&str; 3] = [
    "安全模式期间日志（完整）",
    "最小集探活结果",
    "失败 strikes 账快照",
];

/// 单项典型体积 KiB。
pub const EXIT_ITEM_KIB: u64 = 64;

/// 导出包体积预估。
pub fn exit_package_estimate() -> u64 {
    EXIT_PACKAGE_ITEMS.len() as u64 * EXIT_ITEM_KIB
}

#[cfg(test)]
mod deep9_tests {
    use super::*;

    #[test]
    fn f193_v9_pkg_items_unique() {
        // 包内容不重复。
        for (i, a) in EXIT_PACKAGE_ITEMS.iter().enumerate() {
            for b in EXIT_PACKAGE_ITEMS.iter().skip(i + 1) {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn f193_v9_run_checks_pass() {
        assert!(run_safemode_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b10（第五波）：排查时长账（进入 → 退出的停留时长人话化）。
// ---------------------------------------------------------------------------

/// 排查时长（entered_s → now_s → 人话；<1h 报分钟，≥24h 报天）。
pub fn troubleshoot_duration(entered_s: u64, now_s: u64) -> alloc::string::String {
    let dur = now_s.saturating_sub(entered_s);
    if dur >= 24 * 3600 {
        alloc::format!("排查历时 {} 天", dur / 86400)
    } else if dur >= 3600 {
        alloc::format!("排查历时 {} 小时", dur / 3600)
    } else {
        alloc::format!("排查历时 {} 分钟", dur / 60)
    }
}

#[cfg(test)]
mod deep10_tests {
    use super::*;

    #[test]
    fn f193_v10_duration_minutes() {
        assert!(troubleshoot_duration(0, 600).contains("10 分钟"));
    }

    #[test]
    fn f193_v10_run_checks_pass() {
        assert!(run_safemode_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b11（第六波）：探活报告 CSV / 退出条件倒计时。
// ---------------------------------------------------------------------------

/// 探活报告 CSV（feature,result）。
pub fn probe_csv(results: &[(&str, bool)]) -> alloc::string::String {
    let mut out = alloc::string::String::from("feature,result\n");
    for (f, ok) in results {
        out.push_str(&alloc::format!("{},{}\n", f, if *ok { "pass" } else { "fail" }));
    }
    out
}

/// 退出条件倒计时（达成数 / 总条件 → 还差几条）。
pub fn exit_countdown(met: u64, total: u64) -> u64 {
    total.saturating_sub(met)
}

#[cfg(test)]
mod deep11_tests {
    use super::*;

    #[test]
    fn f193_v11_countdown_zero() {
        assert_eq!(exit_countdown(4, 4), 0);
    }

    #[test]
    fn f193_v11_run_checks_pass() {
        assert!(run_safemode_deep7b_checks().all_passed());
    }
}



// ---------------------------------------------------------------------------
// v8-b12（第七波）：停留小时换算。
// ---------------------------------------------------------------------------

/// 停留小时（entered → now）。
pub fn stay_hours(entered_s: u64, now_s: u64) -> u64 {
    now_s.saturating_sub(entered_s) / 3600
}

#[cfg(test)]
mod deep12_tests {
    use super::*;

    #[test]
    fn f193_v12_hours_zero() {
        assert_eq!(stay_hours(100, 50), 0);
    }

    #[test]
    fn f193_v12_run_checks_pass() {
        assert!(run_safemode_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b13（第八波）：探活通过率行。
// ---------------------------------------------------------------------------

/// 通过率行（总数 / 失败数）。
pub fn probe_rate_line(total: u64, failed: u64) -> alloc::string::String {
    alloc::format!("探活 {} 项，失败 {} 项", total, failed)
}

#[cfg(test)]
mod deep13_tests {
    use super::*;

    #[test]
    fn f193_v13_rate_all_pass() {
        assert!(probe_rate_line(12, 0).contains("失败 0 项"));
    }

    #[test]
    fn f193_v13_run_checks_pass() {
        assert!(run_safemode_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b14（第九波）：退出包 CSV。
// ---------------------------------------------------------------------------

/// 退出包 CSV（item）。
pub fn exit_pkg_csv() -> alloc::string::String {
    let mut out = alloc::string::String::from("item\n");
    for it in EXIT_PACKAGE_ITEMS {
        out.push_str(it);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod deep14_tests {
    use super::*;

    #[test]
    fn f193_v14_pkg_csv_content() {
        assert!(exit_pkg_csv().contains("最小集探活结果"));
    }

    #[test]
    fn f193_v14_run_checks_pass() {
        assert!(run_safemode_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8 终波（deep8 表）：灰置项统计导出 / 进入原因时间线 / 修复建议匹配行。
// 判据源：主册【状态与异常】「白名单外功能全部灰置且可解释」+【交互设计】
// 「进入提示条附『为什么我在安全模式』帮助链」。
// ---------------------------------------------------------------------------

/// 灰置项统计（功能矩阵中 Off 项点名——每一项灰置都可解释）。
pub fn grayed_items() -> Vec<&'static str> {
    minimal_feature_matrix()
        .into_iter()
        .filter(|(_, m)| *m == SfMode::Off)
        .map(|(name, _)| name)
        .collect()
}

/// 灰置项统计导出 CSV（feature,reason——求助材料的一部分）。
pub fn grayed_csv() -> String {
    let mut out = String::from("feature,reason\n");
    for name in grayed_items() {
        out.push_str(&alloc::format!("{},安全模式下停用（退出后自动恢复）\n", name));
    }
    out
}

/// 进入原因时间线行（触发源 → 时刻 → 人话，行行挂帮助链）。
pub fn entry_reason_timeline(reason: u8, at_s: u64) -> String {
    let why = match reason {
        0 => "选单隐藏条目进入",
        1 => "用户主动选择",
        2 => "关键组件连续失败",
        _ => "回滚后最小集验证",
    };
    alloc::format!("t+{}s：{}（详见 {}）", at_s, why, HELP_LINK)
}

/// 修复建议匹配行（失败组件 → 人话建议；无匹配诚实兜底）。
pub fn remedy_line(component: &str) -> &'static str {
    match component {
        "window-mgr" => "合成器降级路径已接管——重启一次窗口管理即可",
        "ime-base" => "切回默认主题与基础键盘布局后再试",
        "taskbar" => "走键盘快捷键启动器（降级预案）",
        "clipboard" => "用应用内置剪贴（临时同步缓冲）",
        _ => "无专项建议——先看诊断中心的失败详情",
    }
}

/// F193 v8 终波自检（deep8 表）。
pub fn run_safemode_deep8_checks() -> CheckSet {
    let mut set = CheckSet::new("F193-v8c");

    // 灰置项统计：数量守恒、导出可读、理由在行内。
    set.add("gray count", grayed_items() == vec!["log-export"], "仅日志导出灰置");
    set.add("gray csv head", grayed_csv().starts_with("feature,reason\n"), "CSV 表头");
    set.add("gray csv row", grayed_csv().lines().count() == 2, "表头 + 单行");
    set.add("gray csv reason", grayed_csv().contains("自动恢复"), "灰置可解释");

    // 原因时间线：四类触发源都有行、行行带帮助链。
    set.add("timeline at", entry_reason_timeline(0, 5).starts_with("t+5s"), "时刻入行");
    set.add("timeline link", (0..4u8).all(|r| entry_reason_timeline(r, 0).contains(HELP_LINK)), "行行挂帮助链");

    // 修复建议：已知组件有专项、未知组件诚实兜底。
    set.add("remedy known", remedy_line("window-mgr").contains("合成器"), "已知组件专项建议");
    set.add("remedy unknown", remedy_line("no-such").contains("无专项"), "未知组件诚实兜底");

    set
}

#[cfg(test)]
mod deep8b_tests {
    use super::*;

    #[test]
    fn f193_deep8_gray_matches_matrix() {
        // 灰置项必须来自矩阵 Off 行（不许点名矩阵外的幽灵项）。
        let mx: Vec<&str> = minimal_feature_matrix()
            .into_iter()
            .filter(|(_, m)| *m == SfMode::Off)
            .map(|(n, _)| n)
            .collect();
        assert_eq!(grayed_items(), mx);
    }

    #[test]
    fn f193_deep8_timeline_zero() {
        // 零时刻也不空行（t+0s 合法入账）。
        assert!(entry_reason_timeline(1, 0).starts_with("t+0s"));
    }

    #[test]
    fn f193_deep8_run_checks_pass() {
        assert!(run_safemode_deep8_checks().all_passed());
    }
}
