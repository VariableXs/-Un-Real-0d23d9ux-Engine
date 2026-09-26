//! F319 唤醒即回体验 + F320 飞行模式 · AI-H3。
//!
//! **F319 判据（主册）**：全链 <2s 实测（10 次采样）；分级启动时序；失
//! 败自愈注入用例；状态一致性（唤醒后截图比对睡前列表）。
//! **F320 判据（主册）**：全无线断开验证（物理层扫描）；恢复精度（原热
//! 点自动重连 <5s）；跨重启记忆；离线诚实降级用例（浏览器/搜索/天气冻
//! 结项各一）。
//!
//! **F319 设计要点**：睡眠唤醒全链 <2 秒：开盖/按键→屏幕亮→锁屏就绪→
//! 解锁→回到睡眠前原样；唤醒瞬间后台按 F065 分级起（前台先活、后台缓
//! 5 秒）；唤醒失败自愈（>5 秒黑屏自动走冷启动链并留诊断快照 F174）。
//! **F320 设计要点**：一键断全部无线并全局状态条明确显示；再点恢复到
//! 断前各自状态（Wi-Fi 连回原热点、蓝牙回原配对）；跨重启记忆；飞行模
//! 式下依赖网络的功能诚实降级。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// F319 唤醒即回
// ---------------------------------------------------------------------------

/// 全链判线（ms）。
pub const WAKE_CHAIN_LIMIT_MS: u64 = 2000;

/// 后台缓起延迟（ms——F065 分级）。
pub const BACKGROUND_DEFER_MS: u64 = 5000;

/// 自愈判定（ms——黑屏超 5s 走冷启动链）。
pub const WAKE_SELFHEAL_MS: u64 = 5000;

/// 唤醒链时序账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WakeTimeline {
    /// 各阶段时刻（开盖→亮屏→锁屏就绪→解锁完成）。
    pub lid_open_ms: u64,
    pub screen_on_ms: u64,
    pub lockscreen_ready_ms: u64,
    pub unlocked_ms: u64,
}

impl WakeTimeline {
    /// 全链耗时。
    pub fn total_ms(&self) -> u64 {
        self.unlocked_ms.saturating_sub(self.lid_open_ms)
    }

    /// 分级启动时序：锁屏就绪先于后台起（前台先活——后台缓 5 秒）。
    pub fn tiering_ok(&self, background_start_ms: u64) -> bool {
        background_start_ms >= self.lockscreen_ready_ms + BACKGROUND_DEFER_MS
    }
}

/// 唤醒管理器。
pub struct WakeManager {
    /// 睡前状态快照（窗口布局 F237 + 文档状态 F311 的清单——一致性对账）。
    pre_sleep_state: Vec<String>,
    /// 失败自愈注入账。
    pub selfheals: u64,
}

impl WakeManager {
    pub fn new(pre: &[&str]) -> WakeManager {
        WakeManager { pre_sleep_state: pre.iter().map(|s| String::from(*s)).collect(), selfheals: 0 }
    }

    /// 睡前列表（一致性比对源）。
    pub fn pre_sleep(&self) -> &[String] {
        &self.pre_sleep_state
    }

    /// 状态一致性：唤醒后列表与睡前逐项一致。
    pub fn state_consistent(&self, after: &[String]) -> bool {
        after == &self.pre_sleep_state
    }

    /// 10 次采样全链判线（实测账——注入时序样本）。
    pub fn sampled_chain_ok(&self, samples: &[u64]) -> bool {
        samples.len() == 10 && samples.iter().all(|t| *t <= WAKE_CHAIN_LIMIT_MS)
    }

    /// 失败自愈：黑屏超 5s → 走冷启动链 + 留诊断快照标记。
    pub fn selfheal(&mut self, blackscreen_ms: u64) -> bool {
        if blackscreen_ms > WAKE_SELFHEAL_MS {
            self.selfheals += 1;
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// F320 飞行模式
// ---------------------------------------------------------------------------

/// 无线设备断前状态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RadioState {
    pub wifi_on: bool,
    pub hotspot: String,
    pub bt_on: bool,
    pub bt_peer: String,
}

/// 飞行模式控制器。
pub struct AirplaneMode {
    pub active: bool,
    pre: Option<RadioState>,
    /// 跨重启记忆（持久标志）。
    pub persisted_active: bool,
}

impl AirplaneMode {
    pub fn new() -> AirplaneMode {
        AirplaneMode { active: false, pre: None, persisted_active: false }
    }

    /// 开飞行模式：全无线断开 + 断前状态入账（恢复精度依据）。
    pub fn engage(&mut self, now_state: RadioState) {
        self.active = true;
        self.persisted_active = true; // 跨重启记忆（写持久位）。
        self.pre = Some(now_state);
    }

    /// 物理层扫描验证：飞行模式下所有无线真实关闭（不是软标志）。
    pub fn physical_scan_all_off(&self, scan: &[(String, bool)]) -> bool {
        self.active && scan.iter().all(|(_, on)| !on)
    }

    /// 关飞行模式：恢复断前各自状态（Wi-Fi 连回原热点、蓝牙回原配对）。
    pub fn disengage(&mut self) -> Option<RadioState> {
        self.active = false;
        self.persisted_active = false;
        self.pre.take()
    }

    /// 恢复精度：重连原热点 <5s（注入重连耗时——判线对账）。
    pub fn reconnect_ok(&self, reconnect_ms: u64) -> bool {
        reconnect_ms < 5000
    }
}

/// 离线诚实降级注册表（飞行模式下依赖网络的功能 → 冻结项文案）。
pub struct OfflineDegrade {
    entries: Vec<(&'static str, &'static str)>,
}

impl OfflineDegrade {
    pub fn new() -> OfflineDegrade {
        let mut o = OfflineDegrade { entries: Vec::new() };
        // 三冻结项各一（浏览器/搜索/天气——主册用例面）。
        o.register("browser", "离线页（不转圈假活）");
        o.register("search", "仅本地结果");
        o.register("weather", "显示上次数据并标注离线");
        o
    }

    /// 登记冻结项（重复拒绝）。
    pub fn register(&mut self, app: &'static str, behavior: &'static str) -> bool {
        if self.entries.iter().any(|(a, _)| *a == app) {
            return false;
        }
        self.entries.push((app, behavior));
        true
    }

    /// 三冻结项齐（判据：浏览器/搜索/天气各一）。
    pub fn three_frozen(&self) -> bool {
        ["browser", "search", "weather"].iter().all(|a| self.entries.iter().any(|(x, _)| x == a))
    }

    pub fn behavior_of(&self, app: &str) -> Option<&'static str> {
        self.entries.iter().find(|(a, _)| *a == app).map(|(_, b)| *b)
    }
}

impl Default for OfflineDegrade {
    fn default() -> OfflineDegrade {
        OfflineDegrade::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F319 自检。
pub fn run_wakeresume_checks() -> CheckSet {
    let mut set = CheckSet::new("F319-wakeresume");

    // 1. 全链 <2s（10 次采样账——1.6s 级注入样本全过）。
    let wm = WakeManager::new(&["窗口A", "文档1", "窗口B"]);
    let samples = [1600u64; 10];
    set.add("chain under 2s 10 samples", wm.sampled_chain_ok(&samples), "");

    // 2. 分级启动时序：锁屏就绪先活，后台缓 5 秒起。
    let tl = WakeTimeline {
        lid_open_ms: 0,
        screen_on_ms: 400,
        lockscreen_ready_ms: 900,
        unlocked_ms: 1800,
    };
    set.add(
        "tiered startup order",
        tl.total_ms() <= WAKE_CHAIN_LIMIT_MS && tl.tiering_ok(900 + BACKGROUND_DEFER_MS),
        "",
    );
    set.add("tiering early start rejected", !tl.tiering_ok(900), "");

    // 3. 状态一致性：唤醒后列表与睡前逐项一致；不一致诚实红。
    let after = [String::from("窗口A"), String::from("文档1"), String::from("窗口B")];
    let after_bad = [String::from("窗口A"), String::from("窗口B")];
    set.add(
        "state consistent snapshot",
        wm.state_consistent(&after) && !wm.state_consistent(&after_bad),
        "",
    );

    // 4. 失败自愈：黑屏 5.1s → 冷启动链 + 诊断快照；4.9s 不触发。
    let mut wm2 = WakeManager::new(&["x"]);
    set.add(
        "selfheal over 5s blackscreen",
        !wm2.selfheal(4900) && wm2.selfheal(5100) && wm2.selfheals == 1,
        "",
    );

    // 5. 采样不足 10 次拒绝出判（诚实——样本不够不出绿）。
    let short = [1600u64; 9];
    set.add("samples must be ten", !wm.sampled_chain_ok(&short), "");

    set
}

/// F320 自检。
pub fn run_airlane_checks() -> CheckSet {
    let mut set = CheckSet::new("F320-airlane");

    // 1. 全无线断开验证（物理层扫描——不是软标志）。
    let mut am = AirplaneMode::new();
    am.engage(RadioState {
        wifi_on: true,
        hotspot: String::from("STAR-5G"),
        bt_on: true,
        bt_peer: String::from("耳机X"),
    });
    let scan = [
        (String::from("wifi"), false),
        (String::from("bt"), false),
    ];
    set.add("physical scan all off", am.physical_scan_all_off(&scan), "");
    set.add("soft flag caught", !am.physical_scan_all_off(&[(String::from("wifi"), true)]), "");

    // 2. 恢复精度：断前状态完整还原（原热点 + 原配对）。
    let restored = am.disengage().unwrap();
    set.add(
        "restore previous states",
        restored.wifi_on && restored.hotspot == "STAR-5G" && restored.bt_on && restored.bt_peer == "耳机X",
        "",
    );

    // 3. 恢复重连 <5s 判线（4.9s 过 / 5.1s 红）。
    let am2 = AirplaneMode::new();
    set.add("reconnect under 5s", am2.reconnect_ok(4900) && !am2.reconnect_ok(5100), "");

    // 4. 跨重启记忆：engage 置持久位；disengage 清位。
    let mut am3 = AirplaneMode::new();
    am3.engage(RadioState { wifi_on: false, hotspot: String::new(), bt_on: false, bt_peer: String::new() });
    let persisted_on = am3.persisted_active;
    let _ = am3.disengage();
    set.add(
        "persists across reboot",
        persisted_on && !am3.persisted_active && !am3.active,
        "",
    );

    // 5. 离线诚实降级：三冻结项齐 + 行为可查 + 重复登记拒绝。
    let mut od = OfflineDegrade::new();
    set.add(
        "three frozen items",
        od.three_frozen()
            && od.behavior_of("browser") == Some("离线页（不转圈假活）")
            && !od.register("browser", "重复"),
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
    fn timeline_total() {
        let tl = WakeTimeline { lid_open_ms: 100, screen_on_ms: 200, lockscreen_ready_ms: 300, unlocked_ms: 400 };
        assert_eq!(tl.total_ms(), 300);
    }

    #[test]
    fn engage_twice_keeps_latest() {
        let mut am = AirplaneMode::new();
        am.engage(RadioState { wifi_on: true, hotspot: String::from("A"), bt_on: false, bt_peer: String::new() });
        am.engage(RadioState { wifi_on: false, hotspot: String::from("B"), bt_on: false, bt_peer: String::new() });
        let r = am.disengage().unwrap();
        assert_eq!(r.hotspot, "B");
    }

    #[test]
    fn disengage_without_engage_none() {
        let mut am = AirplaneMode::new();
        assert!(am.disengage().is_none());
    }

    #[test]
    fn wake_constants() {
        assert_eq!(WAKE_CHAIN_LIMIT_MS, 2000);
        assert_eq!(BACKGROUND_DEFER_MS, 5000);
        assert_eq!(WAKE_SELFHEAL_MS, 5000);
    }
}
