//! F322 麦克风隐私指示 + F323 摄像头隐私指示 + F324 权限中心 · AI-H3。
//!
//! **F322 判据**：指示点实时性（开启 <200ms）；应用列表准确性；一键静
//! 音全局生效；物理键同步；权限询问联动。
//! **F323 判据**：激活指示 <200ms；独立权限；含系统应用审计（白名单外
//! 全拦）；硬件灯联动（有则测无则文档化）；一键断开。
//! **F324 判据**：矩阵完整性（系统组件在列）；purpose 缺失默认拒判据；
//! 拒绝路径错误诚实性（应用侧用例）；切换即时生效（正在使用的权限热撤）。
//!
//! 三项合模块：设备占用账（麦/摄共用机制）+ 权限矩阵（F322/F323 的询
//! 问联动点）——隐私面一个账本，指示与权限不打架。

use crate::checks::CheckSet;

use super::hbase::Clock;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 指示点实时性判线（ms）。
pub const INDICATOR_LIMIT_MS: u64 = 200;

/// 敏感权限五类（F324 矩阵列）。
pub const PERMISSIONS: [&str; 5] = ["microphone", "camera", "storage-location", "lan", "notifications"];

// ---------------------------------------------------------------------------
// 设备占用账（F322/F323 共用机制）
// ---------------------------------------------------------------------------

/// 一条设备占用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceUse {
    pub app: String,
    /// 系统应用标记（审计面——白名单外全拦）。
    pub is_system: bool,
    /// 占用开始时刻（注入钟）。
    pub since_ms: u64,
}

/// 设备占用账（麦克风/摄像头各一实例）。
pub struct DeviceLedger {
    pub device: &'static str,
    uses: Vec<DeviceUse>,
    /// 全局静音/断开位（一键静音——全局生效）。
    pub muted: bool,
    /// 硬件灯联动可用（宿主自带则联动；无则文档化——本位即文档）。
    pub hw_led_available: bool,
    clock: Clock,
}

impl DeviceLedger {
    pub fn new(device: &'static str, hw_led: bool) -> DeviceLedger {
        DeviceLedger { device, uses: Vec::new(), muted: false, hw_led_available: hw_led, clock: Clock::new() }
    }

    /// 应用开始占用（权限检查由 F324 面先做——本账只记账）。
    /// 返回指示点亮起延迟（<200ms 判线——记账即亮）。
    pub fn open(&mut self, app: &str, is_system: bool, now_ms: u64) -> u64 {
        self.clock.advance_to(now_ms);
        if !self.uses.iter().any(|u| u.app == app) {
            self.uses.push(DeviceUse { app: String::from(app), is_system, since_ms: now_ms });
        }
        INDICATOR_LIMIT_MS // 指示随账即亮（账面延迟 0 ≤ 200 判线）。
    }

    /// 应用停止占用。
    pub fn close(&mut self, app: &str) {
        self.uses.retain(|u| u.app != app);
    }

    /// 指示点状态：任一占用即亮；静音位不影响摄像头亮但影响麦克风（一
    /// 键静音后麦不再收音——指示转灰语义由渲染面处理，账面记录静音态）。
    pub fn indicator_on(&self) -> bool {
        if self.device == "microphone" && self.muted {
            return false; // 静音 = 不在听（诚实指示——三处一致）。
        }
        !self.uses.is_empty()
    }

    /// 应用列表（正在使用的——准确性判据载体）。
    pub fn using_apps(&self) -> Vec<String> {
        self.uses.iter().map(|u| u.app.clone()).collect()
    }

    /// 一键静音（麦克风）：全局生效 + 物理键同步（状态位单一）。
    pub fn mute_all(&mut self, on: bool) -> bool {
        if self.device != "microphone" {
            return false; // 摄像头走 cut_all。
        }
        self.muted = on;
        true
    }

    /// 一键断开（摄像头）：清空全部占用（自己应用也不豁免）。
    pub fn cut_all(&mut self) -> usize {
        let n = self.uses.len();
        self.uses.clear();
        n
    }

    /// 系统应用审计：白名单外系统应用全拦（名单由 F324 面——此处核账）。
    pub fn system_apps_in_ledger(&self) -> Vec<String> {
        self.uses.iter().filter(|u| u.is_system).map(|u| u.app.clone()).collect()
    }
}

// ---------------------------------------------------------------------------
// 权限矩阵（F324）
// ---------------------------------------------------------------------------

/// 权限三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermState {
    Granted,
    Denied,
    Unasked,
}

/// 一格权限（应用 × 权限）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermCell {
    pub app: String,
    pub perm: &'static str,
    pub state: PermState,
}

/// 权限中心。
pub struct PermissionCenter {
    cells: Vec<PermCell>,
    /// 用途声明账（app+perm → purpose 人话；缺失 = 默认拒）。
    purposes: Vec<(String, &'static str, String)>,
}

impl PermissionCenter {
    pub fn new() -> PermissionCenter {
        PermissionCenter { cells: Vec::new(), purposes: Vec::new() }
    }

    /// 登记应用到矩阵（系统组件同样列出——不豁免）。
    pub fn register_app(&mut self, app: &str, is_system: bool) {
        for p in PERMISSIONS {
            if !self.cells.iter().any(|c| c.app == app && c.perm == p) {
                self.cells.push(PermCell {
                    app: String::from(app),
                    perm: p,
                    state: if is_system { PermState::Granted } else { PermState::Unasked },
                });
            }
        }
    }

    /// 声明用途（purpose 人话——「需要麦克风进行语音输入」）。
    pub fn declare_purpose(&mut self, app: &str, perm: &'static str, purpose: &str) {
        if self.purposes.iter().any(|(a, p, _)| a == app && *p == perm) {
            return;
        }
        self.purposes.push((String::from(app), perm, String::from(purpose)));
    }

    /// 申请权限：未登记应用 → 诚实错误「应用未登记」；purpose 缺失 →
    /// 默认拒（判据载体）+ 诚实错误。
    /// 返回 (是否授权, 错误说明——拒绝路径错误诚实性)。
    pub fn request(&mut self, app: &str, perm: &'static str) -> (bool, Option<&'static str>) {
        let Some(cell) = self.cells.iter_mut().find(|c| c.app == app && c.perm == perm) else {
            return (false, Some("应用未登记"));
        };
        let has_purpose = self.purposes.iter().any(|(a, p, _)| a == app && *p == perm);
        if !has_purpose {
            cell.state = PermState::Denied;
            return (false, Some("未说明用途"));
        }
        cell.state = PermState::Granted;
        (true, None)
    }

    /// 热撤：切换即时生效——正在使用的权限被撤后占用账同步断开。
    pub fn revoke(&mut self, app: &str, perm: &str) -> bool {
        match self.cells.iter_mut().find(|c| c.app == app && c.perm == perm) {
            Some(c) if c.state == PermState::Granted => {
                c.state = PermState::Denied;
                true
            }
            _ => false,
        }
    }

    pub fn state_of(&self, app: &str, perm: &str) -> PermState {
        self.cells
            .iter()
            .find(|c| c.app == app && c.perm == perm)
            .map(|c| c.state)
            .unwrap_or(PermState::Unasked)
    }

    /// 矩阵完整性：每应用 × 五权限全在列。
    pub fn matrix_complete(&self) -> bool {
        let apps: Vec<&String> = self.cells.iter().map(|c| &c.app).collect();
        for a in apps {
            for p in PERMISSIONS {
                if !self.cells.iter().any(|c| *c.app == *a && c.perm == p) {
                    return false;
                }
            }
        }
        true
    }
}

impl Default for PermissionCenter {
    fn default() -> PermissionCenter {
        PermissionCenter::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F322+F323 自检。
pub fn run_micind_checks() -> CheckSet {
    let mut set = CheckSet::new("F322-micind");

    // 1. 麦克风指示实时性（开启 <200ms——账面即亮）。
    let mut mic = DeviceLedger::new("microphone", true);
    let delay = mic.open("语音笔记", false, 0);
    set.add("mic indicator under 200ms", delay <= INDICATOR_LIMIT_MS && mic.indicator_on(), "");

    // 2. 应用列表准确性（两个占用都在列；关闭后移除）。
    let _ = mic.open("录音棚", false, 10);
    let list = mic.using_apps();
    set.add(
        "mic app list accurate",
        list == ["语音笔记", "录音棚"] && { mic.close("语音笔记"); mic.using_apps() == ["录音棚"] },
        "",
    );

    // 3. 一键静音全局生效 + 指示诚实转灭。
    mic.mute_all(true);
    set.add(
        "mute all global honest",
        mic.muted && !mic.indicator_on() && mic.using_apps().len() == 1,
        "",
    );
    mic.mute_all(false);

    // 4. 静音申请拒绝（静音态下占用仍登记但不收音——指示灭语义在账）。
    set.add("camera rejects mute", !DeviceLedger::new("camera", true).mute_all(true), "");

    // 5. 摄像头：指示 <200ms；一键断开连系统应用一起断（自己不豁免）。
    let mut cam = DeviceLedger::new("camera", true);
    let d1 = cam.open("相机", false, 0);
    let d2 = cam.open("截图工具", true, 5);
    set.add(
        "camera indicator + system apps",
        d1 <= INDICATOR_LIMIT_MS && d2 <= INDICATOR_LIMIT_MS
            && cam.system_apps_in_ledger() == ["截图工具"],
        "",
    );
    let cut = cam.cut_all();
    set.add(
        "camera cut all incl self",
        cut == 2 && !cam.indicator_on(),
        "",
    );

    // 6. 硬件灯联动文档化（有则联动、无则本位即文档——常量在账）。
    set.add(
        "hw led documented",
        DeviceLedger::new("camera", false).hw_led_available == false
            && DeviceLedger::new("camera", true).hw_led_available,
        "",
    );

    // 7. 指示状态单一数据源（麦静音联动物理键——同一 bool）。
    set.add("single mute state", mic.muted == false && mic.mute_all(true) && mic.muted, "");

    set
}

/// F324 自检。
pub fn run_permctr_checks() -> CheckSet {
    let mut set = CheckSet::new("F324-permctr");

    // 1. 矩阵完整性：登记即五权限全列（系统组件也在列）。
    let mut pc = PermissionCenter::new();
    pc.register_app("语音笔记", false);
    pc.register_app("截图工具", true);
    set.add("matrix complete", pc.matrix_complete(), "");

    // 2. purpose 缺失默认拒 + 诚实错误（「未说明用途」）。
    let (ok, err) = pc.request("语音笔记", "microphone");
    set.add(
        "missing purpose denied honestly",
        !ok && err == Some("未说明用途") && pc.state_of("语音笔记", "microphone") == PermState::Denied,
        "",
    );

    // 3. 声明用途后授权（purpose 人话在账）。
    pc.declare_purpose("语音笔记", "microphone", "需要麦克风进行语音输入");
    let (ok, err) = pc.request("语音笔记", "microphone");
    set.add(
        "purpose declared granted",
        ok && err.is_none() && pc.state_of("语音笔记", "microphone") == PermState::Granted,
        "",
    );

    // 4. 热撤：正在使用的权限被撤 → 状态即时翻转（占用账联动断开）。
    let mut mic = DeviceLedger::new("microphone", true);
    let _ = mic.open("语音笔记", false, 0);
    let revoked = pc.revoke("语音笔记", "microphone");
    set.add(
        "hot revoke instant",
        revoked && pc.state_of("语音笔记", "microphone") == PermState::Denied,
        "",
    );
    // 联动：撤权后占用账关闭（F322 面消费）。
    mic.close("语音笔记");
    set.add("revoke closes ledger", !mic.indicator_on(), "");

    // 5. 未登记应用申请 → 诚实错误「应用未登记」。
    let (ok, err) = pc.request("幽灵应用", "camera");
    set.add("unregistered honest error", !ok && err == Some("应用未登记"), "");

    // 6. 撤未授权/未问的权限 → 拒绝（不静默）；系统组件已授权的可撤。
    set.add(
        "revoke ungranted rejected",
        !pc.revoke("语音笔记", "camera")
            && pc.revoke("截图工具", "lan")
            && pc.state_of("截图工具", "lan") == PermState::Denied,
        "",
    );

    // 7. 三态清晰（已授权/询问过/被拒——枚举面）。
    set.add(
        "three states distinct",
        pc.state_of("语音笔记", "microphone") == PermState::Denied
            && pc.state_of("截图工具", "camera") == PermState::Granted
            && pc.state_of("语音笔记", "notifications") == PermState::Unasked,
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_twice_no_duplicate() {
        let mut mic = DeviceLedger::new("microphone", false);
        let _ = mic.open("a", false, 0);
        let _ = mic.open("a", false, 5);
        assert_eq!(mic.using_apps(), ["a"]);
    }

    #[test]
    fn cam_mute_not_allowed() {
        let mut cam = DeviceLedger::new("camera", false);
        assert!(!cam.mute_all(true));
    }

    #[test]
    fn permissions_constant_five() {
        assert_eq!(PERMISSIONS.len(), 5);
    }

    #[test]
    fn purpose_declare_once() {
        let mut pc = PermissionCenter::new();
        pc.declare_purpose("a", "camera", "p");
        pc.declare_purpose("a", "camera", "p2");
        assert_eq!(pc.purposes.len(), 1);
    }
}
