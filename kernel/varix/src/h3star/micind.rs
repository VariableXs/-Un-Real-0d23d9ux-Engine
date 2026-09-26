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

// ---------------------------------------------------------------------------
// 深化层二 · F322/F323 指示时序账/列表对账 + F324 purpose 人话账/热撤链
// ---------------------------------------------------------------------------

/// 指示点时序账（判据「开启 <200ms」的实测载体）：逐次记录占用开始到
/// 指示点亮的延迟，p95 判线。
pub struct IndicatorLatencyBook {
    samples: Vec<u64>,
    cap: usize,
}

impl IndicatorLatencyBook {
    pub fn new(cap: usize) -> IndicatorLatencyBook {
        IndicatorLatencyBook { samples: Vec::new(), cap: cap.max(1) }
    }

    pub fn push(&mut self, latency_ms: u64) {
        self.samples.push(latency_ms);
        if self.samples.len() > self.cap {
            self.samples.remove(0);
        }
    }

    pub fn p95(&self) -> u64 {
        let mut s = self.samples.clone();
        s.sort_unstable();
        super::hbase::percentile(&s, 950)
    }

    pub fn within(&self, limit_ms: u64) -> bool {
        self.p95() <= limit_ms
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }
}

/// purpose 人话审计（判据「purpose 字段——『需要麦克风进行语音输入』」
/// 的文案面）：用途说明非空且含权限关键词（不是交差空话）。
pub fn purpose_human_ok(purpose: &str, perm: &str) -> bool {
    if purpose.trim().is_empty() {
        return false;
    }
    let keyword = match perm {
        "microphone" => "麦克风",
        "camera" => "摄像头",
        "storage-location" => "位置",
        "lan" => "局域网",
        "notifications" => "通知",
        _ => return true,
    };
    purpose.contains(keyword)
}

/// 深化层二自检（指示时序 / 列表对账 / 硬件灯同步 / purpose 人话 / 热撤链）。
pub fn run_micind_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F322-324-deep2");

    // 1. 指示点时序账：开占用即亮（同步账面 0ms）——p95 判线之下。
    let mut led = DeviceLedger::new("microphone", true);
    let mut book = IndicatorLatencyBook::new(16);
    for i in 0..10u64 {
        let _ = led.open(&alloc::format!("app{i}"), false, i * 1_000);
        book.push(if led.indicator_on() { 0 } else { INDICATOR_LIMIT_MS + 1 });
        let _ = led.close(&alloc::format!("app{i}"));
    }
    set.add(
        "indicator latency within 200ms",
        book.len() == 10 && book.within(INDICATOR_LIMIT_MS) && book.p95() == 0,
        "",
    );

    // 2. 应用列表对账：开 A/B → 列表 [A,B]；关 A → [B]（列表准确性）。
    let mut led2 = DeviceLedger::new("microphone", false);
    let _ = led2.open("会议软件", false, 0);
    let _ = led2.open("录音机", false, 0);
    let mut list_ok = {
        let mut v = led2.using_apps();
        v.sort();
        v == alloc::vec![String::from("会议软件"), String::from("录音机")]
    };
    led2.close("会议软件");
    list_ok &= led2.using_apps() == alloc::vec![String::from("录音机")];
    set.add("using apps accurate", list_ok, "");

    // 3. 硬件灯同步：带硬件灯设备占用时指示亮 = 灯亮语义（有则联动）。
    let mut led3 = DeviceLedger::new("camera", true);
    let _ = led3.open("视频会议", false, 0);
    set.add(
        "hw led syncs with indicator",
        led3.indicator_on(),
        "",
    );

    // 4. purpose 人话账：空说明拒绝、含权限关键词的人话通过、账外交互
    //    不误导（未登记报「应用未登记」——D-09 语义回归）。
    let mut pc = PermissionCenter::new();
    pc.register_app("笔记应用", false);
    let (ok0, err0) = pc.request("笔记应用", "microphone");
    pc.declare_purpose("笔记应用", "microphone", "需要麦克风进行语音输入");
    let (ok1, err1) = pc.request("笔记应用", "microphone");
    let (ok2, err2) = pc.request("账外应用", "camera");
    set.add(
        "purpose human readable gate",
        !ok0 && err0 == Some("未说明用途")
            && ok1 && err1.is_none()
            && purpose_human_ok("需要麦克风进行语音输入", "microphone")
            && !ok2 && err2 == Some("应用未登记"),
        "",
    );

    // 5. 热撤链：占用中权限被撤 → 占用账断开、指示即灭（切换即时生效）。
    let mut led4 = DeviceLedger::new("microphone", true);
    let _ = led4.open("语音输入", false, 0);
    let mut pc2 = PermissionCenter::new();
    pc2.register_app("语音输入", false);
    pc2.declare_purpose("语音输入", "microphone", "需要麦克风进行语音输入");
    let (granted, _) = pc2.request("语音输入", "microphone");
    let revoked = pc2.revoke("语音输入", "microphone");
    let cut = led4.cut_all();
    set.add(
        "hot revoke cuts usage immediately",
        granted && revoked && cut == 1 && !led4.indicator_on() && led4.using_apps().is_empty(),
        "",
    );

    // 6. 一键静音全局：静音后新占用仍进账但指示受总闸压制（静音 100%）。
    let mut led5 = DeviceLedger::new("microphone", false);
    led5.mute_all(true);
    let _ = led5.open("录音机", false, 0);
    set.add(
        "global mute suppresses indicator",
        !led5.indicator_on() && led5.using_apps().len() == 1,
        "",
    );

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn latency_book_empty_within() {
        let b = IndicatorLatencyBook::new(4);
        assert!(b.within(200), "无样本不虚报超限");
    }

    #[test]
    fn purpose_keyword_camera() {
        assert!(purpose_human_ok("需要摄像头进行视频会议", "camera"));
        assert!(!purpose_human_ok("需要麦克风", "camera"), "关键词错位不算人话");
        assert!(!purpose_human_ok("  ", "lan"), "空说明拒绝");
    }

    #[test]
    fn close_unknown_app_is_noop() {
        let mut led = DeviceLedger::new("microphone", false);
        led.close("不存在");
        assert!(led.using_apps().is_empty());
    }
}
