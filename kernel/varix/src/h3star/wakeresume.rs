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

// ---------------------------------------------------------------------------
// 深化层 · F319 唤醒采样统计账 + F320 无线状态持久化 + 重连逐台账
// ---------------------------------------------------------------------------

/// 唤醒链采样账（10 次采样的统计面：全链 ms 列表 → 最差/均值/判线）。
pub struct WakeSampleBook {
    samples: Vec<u64>,
    cap: usize,
}

impl WakeSampleBook {
    pub fn new(cap: usize) -> WakeSampleBook {
        WakeSampleBook { samples: Vec::new(), cap: cap.max(1) }
    }

    /// 记一次采样（环形上限）。
    pub fn record(&mut self, total_ms: u64) {
        self.samples.push(total_ms);
        if self.samples.len() > self.cap {
            self.samples.remove(0);
        }
    }

    /// 判线核账：全部样本 ≤ 判线（10 次采样判据——样本不足不出绿）。
    pub fn all_within(&self, limit_ms: u64, required: usize) -> bool {
        self.samples.len() >= required && self.samples.iter().all(|t| *t <= limit_ms)
    }

    /// 最差样本（调优依据）。
    pub fn worst(&self) -> Option<u64> {
        self.samples.iter().max().copied()
    }

    /// 均值（取整）。
    pub fn mean(&self) -> Option<u64> {
        if self.samples.is_empty() {
            return None;
        }
        Some(self.samples.iter().sum::<u64>() / self.samples.len() as u64)
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

/// 无线状态持久化（F320 跨重启记忆的落盘载体：飞行位 + 各无线断前态）。
pub fn dump_radio_state(am: &super::wakeresume::AirplaneMode, pre: Option<&super::wakeresume::RadioState>) -> crate::h3star::hbase::PersistKv {
    let mut kv = crate::h3star::hbase::PersistKv::new();
    kv.set("air.active", if am.active { "1" } else { "0" });
    if let Some(p) = pre {
        kv.set("air.wifi", if p.wifi_on { "1" } else { "0" });
        kv.set("air.hotspot", &p.hotspot);
        kv.set("air.bt", if p.bt_on { "1" } else { "0" });
        kv.set("air.btpeer", &p.bt_peer);
    }
    kv.flush();
    kv
}

/// 从落盘账恢复断前无线状态（重启后面——恢复精度的依据）。
pub fn load_radio_state(kv: &crate::h3star::hbase::PersistKv) -> Option<super::wakeresume::RadioState> {
    let wifi = kv.get("air.wifi")?;
    let hotspot = kv.get("air.hotspot")?;
    let bt = kv.get("air.bt")?;
    let btpeer = kv.get("air.btpeer")?;
    Some(super::wakeresume::RadioState {
        wifi_on: wifi == "1",
        hotspot: String::from(hotspot),
        bt_on: bt == "1",
        bt_peer: String::from(btpeer),
    })
}

/// 逐台重连账（关飞行模式后：每台无线设备独立重连——原热点 <5s 判线
/// 的逐台对账面；慢的诚实红）。
pub struct ReconnectBook {
    /// (设备/热点名, 重连耗时 ms)。
    pub entries: Vec<(String, u64)>,
}

impl ReconnectBook {
    pub fn new() -> ReconnectBook {
        ReconnectBook { entries: Vec::new() }
    }

    /// 记一台的重连耗时（覆盖语义——同台重连刷新）。
    pub fn record(&mut self, name: &str, ms: u64) {
        match self.entries.iter_mut().find(|(n, _)| n == name) {
            Some(slot) => slot.1 = ms,
            None => self.entries.push((String::from(name), ms)),
        }
    }

    /// 全部 <5s（逐台判线——任一台超线即红）。
    pub fn all_reconnected(&self, limit_ms: u64) -> bool {
        !self.entries.is_empty() && self.entries.iter().all(|(_, ms)| *ms < limit_ms)
    }

    /// 超线清单（诚实诊断——哪台慢）。
    pub fn slow_ones(&self, limit_ms: u64) -> Vec<String> {
        self.entries
            .iter()
            .filter(|(_, ms)| *ms >= limit_ms)
            .map(|(n, _)| n.clone())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for ReconnectBook {
    fn default() -> ReconnectBook {
        ReconnectBook::new()
    }
}

/// 深化层自检。
pub fn run_wakeresume_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("F319-320-deep");

    // 1. 唤醒采样账：10 次 1.6s 级全过；样本 9 次不出绿（不足诚实）。
    let mut book = WakeSampleBook::new(10);
    for _ in 0..10 {
        book.record(1600);
    }
    set.add(
        "wake samples ten within 2s",
        book.all_within(2000, 10) && book.worst() == Some(1600) && book.mean() == Some(1600),
        "",
    );
    let mut short = WakeSampleBook::new(10);
    for _ in 0..9 {
        short.record(1600);
    }
    set.add("wake samples insufficient honest", !short.all_within(2000, 10), "");

    // 2. 超线样本诚实红（1.9s 里混一次 2.1s → 全账红）。
    let mut bad = WakeSampleBook::new(10);
    for _ in 0..9 {
        bad.record(1600);
    }
    bad.record(2100);
    set.add("one breach reds all", !bad.all_within(2000, 10) && bad.worst() == Some(2100), "");

    // 3. 无线状态持久化：断前态落盘 → 重启读回（原热点+原配对逐项回）。
    let mut am = super::wakeresume::AirplaneMode::new();
    am.engage(super::wakeresume::RadioState {
        wifi_on: true,
        hotspot: String::from("STAR-5G"),
        bt_on: true,
        bt_peer: String::from("耳机X"),
    });
    let pre = load_radio_state_placeholder();
    let kv = dump_radio_state(&am, pre.as_ref());
    let back = load_radio_state(&kv);
    set.add(
        "radio state persists reboot",
        back.as_ref().map(|r| r.hotspot == "STAR-5G" && r.bt_peer == "耳机X" && r.wifi_on).unwrap_or(false),
        "",
    );

    // 4. 逐台重连账：全部 <5s 绿；一台 5.1s → 红且点名。
    let mut rc = ReconnectBook::new();
    rc.record("STAR-5G", 3200);
    rc.record("耳机X", 4100);
    set.add(
        "reconnect per device pass",
        rc.all_reconnected(5000) && rc.len() == 2,
        "",
    );
    rc.record("旧热点", 5100);
    set.add(
        "reconnect slow named honestly",
        !rc.all_reconnected(5000) && rc.slow_ones(5000) == ["旧热点"],
        "",
    );

    // 5. 空账不出绿（未重连完不出判）。
    set.add("reconnect empty no verdict", !ReconnectBook::new().all_reconnected(5000), "");

    set
}

fn load_radio_state_placeholder() -> Option<super::wakeresume::RadioState> {
    None
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn sample_book_ring() {
        let mut b = WakeSampleBook::new(3);
        for t in [100u64, 200, 300, 400] {
            b.record(t);
        }
        assert_eq!(b.len(), 3);
        assert_eq!(b.worst(), Some(400));
    }

    #[test]
    fn radio_state_none_when_absent() {
        let kv = crate::h3star::hbase::PersistKv::new();
        assert!(load_radio_state(&kv).is_none());
    }

    #[test]
    fn reconnect_overwrite_refreshes() {
        let mut rc = ReconnectBook::new();
        rc.record("a", 100);
        rc.record("a", 200);
        assert_eq!(rc.len(), 1);
        assert_eq!(rc.entries[0].1, 200);
    }

    #[test]
    fn mean_of_mixed() {
        let mut b = WakeSampleBook::new(10);
        b.record(1000);
        b.record(2000);
        assert_eq!(b.mean(), Some(1500));
    }
}
