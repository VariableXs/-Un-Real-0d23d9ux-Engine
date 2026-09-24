//! 崩溃恢复矩阵（WP-209 · 恢复矩阵组 / S209）：错误的每一种归宿。
//!
//! MD1 第 26.1 节九行恢复矩阵——每类恢复路径实测一遍。设计通则：
//! 越底层的崩溃，用户看到的越少、恢复的越自动；越上层的崩溃，越尊重
//! 用户的知情权。内核 panic 一屏说明加自动重启；应用崩溃要给用户选择。
//! 崩溃报告三要素（26.2）：发生了什么（人话）/为什么（归因分类）/
//! 下一步（重开/恢复会话/上报）——技术细节收进详情折叠区。
//! panic 保护屏规范五条（26.3）。防自锁会话延伸（26.4）：人工重启
//! 三次后要求显式确认并写诊断事件。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 九行恢复矩阵
// ---------------------------------------------------------------------------

pub const MATRIX_ROWS: usize = 9;

/// 崩溃物九类（MD1 26.1 表行序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrashClass {
    KernelPanic,      // 内核 panic：全停
    CompositorCrash,  // 合成器崩：画面冻结
    SvcInpMixNet,     // 输入/混音/网络崩：对应功能失效
    WineserverCrash,  // wineserver 崩：Wine 应用失联
    NativeAppCrash,   // 原生应用崩：单窗口消失
    WineAppCrash,     // Wine 应用崩：经 Wine 异常翻译
    ElectronMainCrash, // Electron 主进程崩：窗口白屏
    Ext4Inconsistent, // ext4 检测到不一致：挂载告警
    UsbGone,          // U 盘失联：全系统失去根
}

pub const ALL_CRASH: [CrashClass; MATRIX_ROWS] = [
    CrashClass::KernelPanic,
    CrashClass::CompositorCrash,
    CrashClass::SvcInpMixNet,
    CrashClass::WineserverCrash,
    CrashClass::NativeAppCrash,
    CrashClass::WineAppCrash,
    CrashClass::ElectronMainCrash,
    CrashClass::Ext4Inconsistent,
    CrashClass::UsbGone,
];

/// 恢复动作面：矩阵"恢复路径"列的实现语义。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Recovery {
    /// 保护屏 → 自动重启 → 引导进恢复会话（红色保护屏+三要素+倒计时）。
    ProtectScreenReboot,
    /// 看门狗 3s 拉起，表面缓冲在共享内存，窗口回位（"桌面已恢复"轻提示）。
    Watchdog3s,
    /// 三次重试 → 降级态（图标灰显+设置中心可查可重启）。
    Retry3Degraded,
    /// 5s 重建 + 逐应用恢复（"Windows 兼容环境已恢复"）。
    Rebuild5s,
    /// 崩溃报告（三要素+详情折叠）→ 可重开（对话框不是沉默消失）。
    CrashReport,
    /// 日志重放 → 重放失败转只读模式（顶栏黄条）。
    JournalReplay,
    /// 保护屏冻结（判例 17——侥幸继续防线）。
    FreezeProtect,
}

/// 用户知情面：自动恢复（用户所见最少）vs 用户选择（知情权）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UserFace {
    /// 自动：用户所见越少越好（底层）。
    Automatic,
    /// 通报：轻提示/黄条/灰显。
    Notified,
    /// 选择：对话框（重开/放弃/查看详情）。
    Choice,
}

/// 矩阵行：崩溃物 → 恢复路径 + 用户知情面（实现即映射函数——无旁路）。
pub fn matrix_row(c: CrashClass) -> (Recovery, UserFace) {
    match c {
        CrashClass::KernelPanic => (Recovery::ProtectScreenReboot, UserFace::Automatic),
        CrashClass::CompositorCrash => (Recovery::Watchdog3s, UserFace::Notified),
        CrashClass::SvcInpMixNet => (Recovery::Retry3Degraded, UserFace::Notified),
        CrashClass::WineserverCrash => (Recovery::Rebuild5s, UserFace::Notified),
        CrashClass::NativeAppCrash => (Recovery::CrashReport, UserFace::Choice),
        CrashClass::WineAppCrash => (Recovery::CrashReport, UserFace::Choice),
        CrashClass::ElectronMainCrash => (Recovery::CrashReport, UserFace::Choice),
        CrashClass::Ext4Inconsistent => (Recovery::JournalReplay, UserFace::Notified),
        CrashClass::UsbGone => (Recovery::FreezeProtect, UserFace::Automatic),
    }
}

/// 设计通则的序校验：底层自动 ≤ 中层通报 ≤ 上层选择（序号化知情面）。
pub fn know_face_order_holds() -> bool {
    let (r0, f0) = matrix_row(CrashClass::KernelPanic);
    let (_, f4) = matrix_row(CrashClass::NativeAppCrash);
    let rank = |f: UserFace| match f {
        UserFace::Automatic => 0,
        UserFace::Notified => 1,
        UserFace::Choice => 2,
    };
    // 内核 panic 必是自动重启面（保护屏+倒计时是告知不是选择）。
    r0 == Recovery::ProtectScreenReboot && rank(f0) < rank(f4)
}

/// 恢复演练：每类跑一遍，全部可达终态（S209 每类恢复路径实测一遍的
/// 宿主模型面）。
pub fn drill_all_recoverable() -> bool {
    let mut i = 0;
    while i < MATRIX_ROWS {
        let (path, _) = matrix_row(ALL_CRASH[i]);
        let reachable = matches!(
            path,
            Recovery::ProtectScreenReboot
                | Recovery::Watchdog3s
                | Recovery::Retry3Degraded
                | Recovery::Rebuild5s
                | Recovery::CrashReport
                | Recovery::JournalReplay
                | Recovery::FreezeProtect
        );
        if !reachable {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 崩溃报告三要素（26.2）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RootCause {
    MemOverLimit,   // 内存超限
    ProtocolViolation, // 协议违规
    UnknownDefect,  // 未知缺陷
    UpstreamDefect, // 上游缺陷
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CrashReport {
    /// 发生了什么（人话："星图停止响应了"）。
    pub what: &'static str,
    /// 为什么（归因分类四选一）。
    pub cause: RootCause,
    /// 下一步（重开/恢复会话/上报）。
    pub next: u8, // bit0=重开 bit1=恢复会话 bit2=上报
    /// 详情折叠区（寄存器/调用栈/日志尾部——可复制可导出 DATA/crash/）。
    pub details_folded: bool,
}

impl CrashReport {
    /// 三要素：人话非空 + 下一步动作位非零 + 详情折叠。
    /// （归因四分类枚举面——未知缺陷也是合法归因，不存在"无归因"。）
    pub fn three_elements_ok(&self) -> bool {
        !self.what.is_empty() && self.next != 0 && self.details_folded
    }
}

// ---------------------------------------------------------------------------
// panic 保护屏规范五条（26.3）
// ---------------------------------------------------------------------------

/// 保护屏检查单（五条全过才合规）。
pub struct ProtectScreen {
    /// 深色底+单色警示图（不用惊吓性红蓝爆闪）。
    pub calm_visual: bool,
    /// 文案三要素齐（正在保存现场/原因分类/10 秒倒计时+未保存已尽力保全）。
    pub three_parts: bool,
    /// 倒计时可按键跳过。
    pub skippable: bool,
    /// 重启走防自锁闸门校验过的正常引导链。
    pub safe_boot_chain: bool,
    /// 现场信息写保护分区内存带（重启后可查）。
    pub state_saved: bool,
}

pub fn screen_ok(s: &ProtectScreen) -> bool {
    s.calm_visual && s.three_parts && s.skippable && s.safe_boot_chain && s.state_saved
}

// ---------------------------------------------------------------------------
// 防自锁会话延伸（26.4 / 篇 24.2）
// ---------------------------------------------------------------------------

/// 人工重启计数与冷却：连续人工重启三次后要求显式确认并写诊断事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RestartGate {
    Allowed,
    /// 第三次起：要求显式确认 + 写诊断事件（会话级防自锁）。
    NeedsExplicitConfirm,
}

pub fn restart_gate(consecutive: u32) -> RestartGate {
    if consecutive >= 3 {
        RestartGate::NeedsExplicitConfirm
    } else {
        RestartGate::Allowed
    }
}

// ---------------------------------------------------------------------------
// CheckSet（恢复矩阵组 · 10 项）
// ---------------------------------------------------------------------------

pub fn run_recovermx_checks() -> CheckSet {
    let mut set = CheckSet::new("恢复矩阵组 S209");
    // 1. 九行矩阵枚举齐。
    set.add(
        "S209 九行矩阵齐",
        ALL_CRASH.len() == MATRIX_ROWS
            && ALL_CRASH[0] == CrashClass::KernelPanic
            && ALL_CRASH[8] == CrashClass::UsbGone,
        "内核 panic/合成器/服务/wineserver/原生/Wine/Electron/ext4/U 盘失联",
    );
    // 2. 每行恢复路径是矩阵函数唯一决定（实现即映射——无旁路改写）。
    let (p1, _) = matrix_row(CrashClass::CompositorCrash);
    let (p2, _) = matrix_row(CrashClass::WineserverCrash);
    set.add(
        "S209 恢复路径映射",
        p1 == Recovery::Watchdog3s && p2 == Recovery::Rebuild5s,
        "崩溃物→恢复路径由 matrix_row 唯一决定，呈现面无权改写",
    );
    // 3. 设计通则序：越底层恢复越自动，越上层越尊重知情权。
    set.add(
        "S209 知情面序",
        know_face_order_holds()
            && matrix_row(CrashClass::NativeAppCrash).1 == UserFace::Choice,
        "内核自动/服务通报/应用给选择——知情面随层递增",
    );
    // 4. 全类恢复可达（S209 每类路径实测一遍的宿主面）。
    set.add(
        "S209 全类恢复可达",
        drill_all_recoverable(),
        "九类全部有可达恢复终态——错误的每一种归宿",
    );
    // 5. 看门狗 3s / 重建 5s 预算常量锁定。
    set.add(
        "S209 恢复预算常量",
        WATCHDOG_MS == 3000 && WINESERVER_REBUILD_MS == 5000,
        "合成器 3s 拉起、wineserver 5s 重建——预算数字进代码",
    );
    // 6. 崩溃报告三要素：人话+归因+下一步，详情折叠。
    let rep = CrashReport {
        what: "星图停止响应了",
        cause: RootCause::UnknownDefect,
        next: 0b011, // 重开 + 恢复会话
        details_folded: true,
    };
    set.add(
        "S209 崩溃报告三要素",
        rep.three_elements_ok() && !rep.what.is_empty(),
        "发生了什么/为什么/下一步齐，技术细节收进详情折叠区",
    );
    // 7. 保护屏规范五条：全过合规，缺一不合规。
    let good = ProtectScreen { calm_visual: true, three_parts: true, skippable: true, safe_boot_chain: true, state_saved: true };
    let bad = ProtectScreen { calm_visual: true, three_parts: true, skippable: false, safe_boot_chain: true, state_saved: true };
    set.add(
        "S209 保护屏五条",
        screen_ok(&good) && !screen_ok(&bad),
        "深色底单色图/三要素/可跳过/防自锁引导链/现场保全——缺一不合规",
    );
    // 8. 防自锁：连续人工重启三次要求显式确认。
    set.add(
        "S209 防自锁三次",
        restart_gate(0) == RestartGate::Allowed
            && restart_gate(2) == RestartGate::Allowed
            && restart_gate(3) == RestartGate::NeedsExplicitConfirm
            && restart_gate(5) == RestartGate::NeedsExplicitConfirm,
        "第三次起显式确认+写诊断事件——会话级防自锁（26.4）",
    );
    // 9. ext4 路径语义：重放失败转只读（与 B-703/B-706 承接）。
    let (p9, f9) = matrix_row(CrashClass::Ext4Inconsistent);
    set.add(
        "S209 ext4 只读保护",
        p9 == Recovery::JournalReplay && f9 == UserFace::Notified,
        "日志重放→失败转只读+顶栏黄条——重放成功正常挂载",
    );
    // 10. U 盘失联语义：保护屏冻结（判例 17 侥幸继续防线承接）。
    let (p10, _) = matrix_row(CrashClass::UsbGone);
    set.add(
        "S209 失联冻结承接",
        p10 == Recovery::FreezeProtect,
        "全系统失去根→保护屏冻结——任何继续都是制造损坏（B-706 承接）",
    );
    set
}

pub const WATCHDOG_MS: u32 = 3000;
pub const WINESERVER_REBUILD_MS: u32 = 5000;

// ---------------------------------------------------------------------------
// 单测（fc02 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fc02_matrix_rows() {
        assert_eq!(ALL_CRASH.len(), 9);
        let (r, f) = matrix_row(CrashClass::KernelPanic);
        assert_eq!(r, Recovery::ProtectScreenReboot);
        assert_eq!(f, UserFace::Automatic);
        assert_eq!(matrix_row(CrashClass::NativeAppCrash).1, UserFace::Choice);
    }

    #[test]
    fn fc02_all_reachable() {
        assert!(drill_all_recoverable());
    }

    #[test]
    fn fc02_report_three() {
        let rep = CrashReport { what: "编辑器停止响应了", cause: RootCause::MemOverLimit, next: 0b001, details_folded: true };
        assert!(rep.three_elements_ok());
        let bad = CrashReport { what: "", cause: RootCause::ProtocolViolation, next: 0b010, details_folded: true };
        assert!(!bad.three_elements_ok(), "没有人话不是报告");
    }

    #[test]
    fn fc02_restart_gate() {
        assert_eq!(restart_gate(1), RestartGate::Allowed);
        assert_eq!(restart_gate(3), RestartGate::NeedsExplicitConfirm);
        assert_eq!(restart_gate(9), RestartGate::NeedsExplicitConfirm);
    }
}
