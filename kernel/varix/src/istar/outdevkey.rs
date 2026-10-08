//! F591 输出设备切换热键 · 完整设计（STAR I 主册 I 域批次八）。
//!
//! **判据（主册）**：循环序与清单一致；OSD 显示；默认无绑定；防爆音；
//! 绑定持久化（F244 注册表）。
//!
//! **设计要点（主册）**：
//! - 音频输出快速循环键（可自定义，默认无——避免误触）：绑定后按键在
//!   输出设备间循环（耳机→扬声器→蓝牙……F241 清单序），OSD 显示当前
//!   切到谁（F240 形制）；会议党一键从扬声器切耳机。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 防爆音过渡时长（ms，F241 判据复用——切换间隙淡出淡入）。
pub const ANTI_POP_FADE_MS: u32 = 40;

/// OSD 显示时长（ms，F240 形制）。
pub const OSD_MS: u64 = 1_500;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 输出设备（F241 清单序——枚举序即循环序）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutDevice {
    pub name: String,
    /// 在线位（离线设备跳过不循环——清单序在在线子集上滚动）。
    pub online: bool,
}

/// 输出切换热键引擎。
pub struct OutDevHotkey {
    /// 设备清单（F241 同源——顺序即循环序）。
    devices: Vec<OutDevice>,
    /// 当前激活序。
    active: usize,
    /// 绑定键位（None = 默认无绑定——避免误触）。
    binding: Option<String>,
    /// 防爆音过渡剩余 ms。
    fade_left: u32,
    /// OSD 剩余 ms。
    osd_left: u64,
    /// 切换次数账。
    switches: u32,
}

impl OutDevHotkey {
    pub fn new() -> OutDevHotkey {
        OutDevHotkey {
            devices: Vec::new(),
            active: 0,
            binding: None,
            fade_left: 0,
            osd_left: 0,
            switches: 0,
        }
    }

    /// 设备清单同步（F241 设置页清单序）。
    pub fn sync_devices(&mut self, names: &[&str]) {
        let old: Vec<(String, bool)> = self
            .devices
            .iter()
            .map(|d| (d.name.clone(), d.online))
            .collect();
        self.devices = names
            .iter()
            .map(|n| OutDevice {
                name: String::from(*n),
                online: old
                    .iter()
                    .find(|(o, _)| o == n)
                    .map(|(_, on)| *on)
                    .unwrap_or(true),
            })
            .collect();
        // 激活序钳到界内。
        self.active = self.active.min(self.devices.len().saturating_sub(1));
    }

    /// 设备在线态更新。
    pub fn set_online(&mut self, name: &str, on: bool) -> bool {
        match self.devices.iter_mut().find(|d| d.name == name) {
            Some(d) => {
                d.online = on;
                true
            }
            None => false,
        }
    }

    /// 绑定热键（持久化由 F244 注册表承接——本账记录绑定值）。
    pub fn bind(&mut self, key: &str) {
        self.binding = Some(String::from(key));
    }

    /// 解绑（恢复默认无绑定）。
    pub fn unbind(&mut self) {
        self.binding = None;
    }

    pub fn binding(&self) -> Option<&str> {
        self.binding.as_deref()
    }

    /// 当前设备。
    pub fn current(&self) -> Option<&str> {
        self.devices.get(self.active).map(|d| d.name.as_str())
    }

    /// 热键按下：循环到下一个在线设备（循环序 = 清单序的在线子集）。
    ///
    /// 默认无绑定时按键被上层忽略（本函数只被已绑定的键位触发——
    /// 结构证据：调用方必须先验 binding）。
    pub fn hotkey_pressed(&mut self) -> Option<String> {
        if self.binding.is_none() || self.devices.is_empty() {
            return None;
        }
        let n = self.devices.len();
        let mut i = self.active;
        for _ in 0..n {
            i = (i + 1) % n;
            if self.devices[i].online {
                self.active = i;
                self.fade_left = ANTI_POP_FADE_MS;
                self.osd_left = OSD_MS;
                self.switches += 1;
                return Some(self.devices[i].name.clone());
            }
        }
        None // 全离线——无路可切（诚实 None）。
    }

    /// 防爆音过渡推进（切换后淡出淡入 40ms 内不出声）。
    pub fn tick(&mut self, ms: u64) {
        self.fade_left = self.fade_left.saturating_sub(ms as u32);
        self.osd_left = self.osd_left.saturating_sub(ms);
    }

    /// 防爆音窗口中（静音过渡判据）。
    pub fn anti_pop_active(&self) -> bool {
        self.fade_left > 0
    }

    /// OSD 在显。
    pub fn osd_visible(&self) -> bool {
        self.osd_left > 0
    }

    /// 切换次数账。
    pub fn switch_count(&self) -> u32 {
        self.switches
    }
}

impl Default for OutDevHotkey {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_outdevkey_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 循环序与清单一致：耳机→扬声器→蓝牙→耳机（F241 清单序）。
    let mut h = OutDevHotkey::new();
    h.sync_devices(&["耳机", "扬声器", "蓝牙耳机"]);
    h.bind("Ctrl+Alt+O");
    let a = h.hotkey_pressed(); // 扬声器
    let b = h.hotkey_pressed(); // 蓝牙耳机
    let c = h.hotkey_pressed(); // 回耳机（循环）
    set.add(
        "cycle follows list order",
        a.as_deref() == Some("扬声器") && b.as_deref() == Some("蓝牙耳机") && c.as_deref() == Some("耳机") && h.switch_count() == 3,
        "",
    );

    // 2. 默认无绑定：未绑定时按键零动作（避免误触）。
    let mut h2 = OutDevHotkey::new();
    h2.sync_devices(&["耳机", "扬声器"]);
    set.add(
        "default unbound no action",
        h2.binding().is_none() && h2.hotkey_pressed().is_none(),
        "",
    );

    // 3. OSD 显示：切换即显 1.5s；期满自收。
    let mut h3 = OutDevHotkey::new();
    h3.sync_devices(&["耳机", "扬声器"]);
    h3.bind("O");
    h3.hotkey_pressed();
    let shown = h3.osd_visible();
    h3.tick(OSD_MS);
    set.add(
        "osd shows and self dismisses",
        shown && !h3.osd_visible() && OSD_MS == 1_500,
        "",
    );

    // 4. 防爆音：切换后 40ms 过渡窗内静音；期满恢复。
    let mut h4 = OutDevHotkey::new();
    h4.sync_devices(&["耳机", "扬声器"]);
    h4.bind("O");
    h4.hotkey_pressed();
    let popping = h4.anti_pop_active();
    h4.tick(ANTI_POP_FADE_MS as u64);
    set.add(
        "anti pop fade window",
        popping && !h4.anti_pop_active() && ANTI_POP_FADE_MS == 40,
        "",
    );

    // 5. 离线设备跳过：蓝牙离线时循环耳机→扬声器→耳机（在线子集滚动）。
    let mut h5 = OutDevHotkey::new();
    h5.sync_devices(&["耳机", "扬声器", "蓝牙耳机"]);
    h5.set_online("蓝牙耳机", false);
    h5.bind("O");
    let s1 = h5.hotkey_pressed(); // 扬声器
    let s2 = h5.hotkey_pressed(); // 跳过蓝牙回耳机
    set.add(
        "offline devices skipped",
        s1.as_deref() == Some("扬声器") && s2.as_deref() == Some("耳机"),
        "",
    );

    // 6. 全离线诚实 None（无路可切不瞎切）。
    let mut h6 = OutDevHotkey::new();
    h6.sync_devices(&["耳机"]);
    h6.set_online("耳机", false);
    h6.bind("O");
    set.add("all offline honest none", h6.hotkey_pressed().is_none(), "");

    // 7. 绑定持久化对账：绑定/解绑值可读（F244 注册表承接面）。
    h6.bind("F9");
    let bound = h6.binding() == Some("F9");
    h6.unbind();
    set.add("binding persistence ledger", bound && h6.binding().is_none(), "");

    // 8. 设备清单重排后激活序钳界（清单变化不越界）。
    let mut h7 = OutDevHotkey::new();
    h7.sync_devices(&["a", "b", "c"]);
    h7.hotkey_pressed();
    h7.sync_devices(&["a"]);
    set.add(
        "active index clamped on sync",
        h7.current() == Some("a"),
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_device_cycle_self() {
        let mut h = OutDevHotkey::new();
        h.sync_devices(&["仅耳机"]);
        h.bind("O");
        assert_eq!(h.hotkey_pressed().as_deref(), Some("仅耳机"));
        assert_eq!(h.hotkey_pressed().as_deref(), Some("仅耳机"));
    }

    #[test]
    fn new_device_defaults_online() {
        let mut h = OutDevHotkey::new();
        h.sync_devices(&["a"]);
        h.sync_devices(&["a", "b"]);
        assert!(h.devices.iter().find(|d| d.name == "b").unwrap().online);
    }

    #[test]
    fn set_online_unknown_false() {
        let mut h = OutDevHotkey::new();
        assert!(!h.set_online("无", true));
    }
}
