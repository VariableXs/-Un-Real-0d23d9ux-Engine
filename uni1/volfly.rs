//! F422 音量图标浮层 · 完整设计（STAR I 主册 G-I-22）。
//!
//! **判据（主册）**：拖动实时生效判据（<50ms 音频响应）；设备名与
//! F241 一致；下拉切换；混音器跳转；浮层几何（图标上方居中）。＋通12。
//!
//! 设计：音量浮层语义核——拖条实时生效账（每步注入音频延迟，<50ms
//! 判线）；设备名同源（F241 注入同一字符串）；设备下拉切换；混音器
//! 入口（F157）；浮层几何计算（图标上方居中，屏幕边缘收敛）。
//!
//! v5 纵深：静音位独立（图标静音 ≠ 滑条 0——静音是状态不是数值）；
//! 拖动即解除静音；拖动取消（Esc/脱手 → 恢复起拖值）；音量快照
//! round-trip（重启恢复上次值）；混音器应用级音量表（钳制 + 容量上限）。
//!
//! v8 纵深：设备热插拔回退账（当前设备从清单消失 → 自动回落默认
//! 扬声器，事件显性入账——静默失声比切换更害人）；音量步进键（±5%
//! 步进，连击限速 80ms——拧爆音量不给窗口）；拖动超时恢复（起拖
//! 10s 无动作自动恢复起拖值——脱手不留半截状态）。

use crate::checks::CheckSet;

use alloc::vec::Vec;
use crate::uni1::ubase::{LayerStack, LayerTier};

/// 拖动实时生效判线（ms）。
pub const LIVE_APPLY_MS: u64 = 50;

/// 混音器应用级音量表容量上限。
pub const APP_MIXER_CAP: usize = 16;

/// 音量步进键幅度（千分比）。
pub const STEP_PERMILLE: u64 = 50;

/// 步进连击限速（ms）——低于此间隔的重复步进拒绝。
pub const STEP_DEBOUNCE_MS: u64 = 80;

/// 拖动无动作超时（ms）——超时自动恢复起拖值。
pub const DRAG_STALL_TIMEOUT_MS: u64 = 10_000;

/// 音量浮层核。
pub struct VolumeFlyout {
    pub open: bool,
    pub volume_permille: u64,
    /// 当前输出设备名（F241 同源注入——唯一真值来自外面）。
    pub device_name: &'static str,
    /// 可切设备清单。
    pub devices: Vec<&'static str>,
    /// 实时生效账：拖动步数与超线步数。
    pub drag_steps: u64,
    pub drag_over_budget: u64,
    /// 浮层栈（Esc/外点关闭走 F424 语义）。
    pub layers: LayerStack,
    /// 静音位（与滑条 0 独立）。
    pub muted: bool,
    /// 起拖值（取消恢复用）。
    drag_saved: Option<u64>,
    /// 拖动取消账。
    pub drag_cancels: u64,
    /// 应用级混音（F157 数据面）：应用 → 音量千分比。
    pub app_volumes: Vec<(&'static str, u64)>,
    /// 设备回落账：热插拔导致自动切换的次数。
    pub device_fallbacks: u64,
    /// 步进限速：上次步进时刻（None = 无历史不防抖）。
    last_step_at_ms: Option<u64>,
    /// 步进拒绝账。
    pub step_rejects: u64,
    /// 起拖时刻（超时恢复判据）。
    drag_started_at_ms: Option<u64>,
}

impl VolumeFlyout {
    pub fn new(device_name: &'static str, devices: Vec<&'static str>) -> VolumeFlyout {
        VolumeFlyout {
            open: false,
            volume_permille: 400,
            device_name,
            devices,
            drag_steps: 0,
            drag_over_budget: 0,
            layers: LayerStack::new(),
            muted: false,
            drag_saved: None,
            drag_cancels: 0,
            app_volumes: Vec::new(),
            device_fallbacks: 0,
            last_step_at_ms: None,
            step_rejects: 0,
            drag_started_at_ms: None,
        }
    }

    /// 点击图标开浮层（几何：图标上方居中）。
    pub fn toggle(&mut self, icon: (i32, i32), layer_h: i32, screen_h: i32) -> (i32, i32) {
        self.open = !self.open;
        if self.open {
            self.layers.open(LayerTier::Popup, "vol-flyout");
        } else {
            let _ = self.layers.close_named("vol-flyout");
        }
        Self::geometry(icon, layer_h, screen_h)
    }

    /// 浮层几何：图标上方居中；顶越界收敛到屏内（贴顶）。
    pub fn geometry(icon: (i32, i32), layer_h: i32, screen_h: i32) -> (i32, i32) {
        let x = icon.0; // 水平以图标中心对齐（宽度由渲染层处理，此处为锚点）
        let y = icon.1 - layer_h;
        (x, y.max(0).min(screen_h - layer_h))
    }

    /// 拖条实时生效：每步注入音频路径延迟（<50ms 判线的逐步账）。
    /// 拖动即解除静音——用户在拖音量，就是要声音。
    pub fn drag_to(&mut self, permille: u64, audio_latency_ms: u64) -> u64 {
        self.volume_permille = permille.clamp(0, 1_000);
        if self.muted {
            self.muted = false;
        }
        self.drag_steps += 1;
        if audio_latency_ms > LIVE_APPLY_MS {
            self.drag_over_budget += 1;
        }
        self.volume_permille
    }

    /// 起拖记账（取消恢复的基准 + 超时判据起点）。
    pub fn begin_drag(&mut self, now_ms: u64) {
        self.drag_saved = Some(self.volume_permille);
        self.drag_started_at_ms = Some(now_ms);
    }

    /// 拖动取消：恢复起拖值（Esc/松手在条外——不给半截状态）。
    pub fn cancel_drag(&mut self) -> bool {
        self.drag_started_at_ms = None;
        match self.drag_saved.take() {
            Some(v) => {
                self.volume_permille = v;
                self.drag_cancels += 1;
                true
            }
            None => false,
        }
    }

    /// 拖动无动作超时：起拖后超窗未动 → 自动恢复起拖值（脱手不留
    /// 半截状态）。返回是否触发了恢复。
    pub fn drag_stalled(&mut self, now_ms: u64) -> bool {
        match self.drag_saved {
            Some(_) => match self.drag_started_at_ms {
                Some(t0) if now_ms.saturating_sub(t0) >= DRAG_STALL_TIMEOUT_MS => {
                    let _ = self.cancel_drag();
                    true
                }
                _ => false,
            },
            None => false,
        }
    }

    /// 音量步进键（±5%）：幅值钳到边界；连击限速（80ms 内重复拒绝
    /// 并记账——防拧爆）；方向用 i32 符号表达。
    pub fn step_volume(&mut self, dir: i32, now_ms: u64) -> Option<u64> {
        if let Some(t) = self.last_step_at_ms {
            if now_ms.saturating_sub(t) < STEP_DEBOUNCE_MS {
                self.step_rejects += 1;
                return None;
            }
        }
        let cur = self.volume_permille as i64;
        let next = if dir >= 0 { cur + STEP_PERMILLE as i64 } else { cur - STEP_PERMILLE as i64 };
        self.last_step_at_ms = Some(now_ms);
        self.volume_permille = next.clamp(0, 1_000) as u64;
        Some(self.volume_permille)
    }

    /// 设备热插拔：设备清单变更后，若当前设备从清单消失 → 自动回落
    /// 默认扬声器（首项），事件显性入账。返回是否发生了回落。
    pub fn device_plug_event(&mut self, new_devices: Vec<&'static str>) -> bool {
        let gone = !new_devices.iter().any(|d| *d == self.device_name);
        self.devices = new_devices;
        if gone {
            if let Some(first) = self.devices.first() {
                self.device_name = *first;
                self.device_fallbacks += 1;
            }
        }
        gone
    }

    /// 静音切换（图标右键——状态位翻转，滑条值不动）。
    pub fn toggle_mute(&mut self) -> bool {
        self.muted = !self.muted;
        self.muted
    }

    /// 应用级混音音量：钳制入档；表满拒绝（不静默挤掉别人）。
    pub fn set_app_volume(&mut self, name: &'static str, permille: u64) -> bool {
        let v = permille.clamp(0, 1_000);
        if let Some(entry) = self.app_volumes.iter_mut().find(|(n, _)| *n == name) {
            entry.1 = v;
            return true;
        }
        if self.app_volumes.len() >= APP_MIXER_CAP {
            return false;
        }
        self.app_volumes.push((name, v));
        true
    }

    /// 音量快照（重启恢复——持久化联动）。
    pub fn volume_snapshot(&self) -> u64 {
        self.volume_permille
    }

    pub fn restore_volume(&mut self, v: u64) -> u64 {
        self.volume_permille = v.clamp(0, 1_000);
        self.volume_permille
    }

    /// 设备下拉切换（清单外拒绝；设备名与 F241 同源——切换后同值）。
    pub fn switch_device(&mut self, name: &str) -> bool {
        if self.devices.iter().any(|d| *d == name) {
            self.device_name = match self.devices.iter().find(|d| **d == name) {
                Some(d) => d,
                None => return false,
            };
            true
        } else {
            false
        }
    }

    /// 混音器入口（F157 跳转）。
    pub fn mixer_jump(&self) -> &'static str {
        "f157.mixer"
    }

    /// Esc/外点关闭（F424 语义）。
    pub fn esc_close(&mut self) -> bool {
        if !self.open {
            return false;
        }
        let _ = self.layers.close_named("vol-flyout");
        self.open = false;
        true
    }
}

pub fn run_volfly_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F422");
    let mut v = VolumeFlyout::new("扬声器 (Y7000)", alloc::vec!["扬声器 (Y7000)", "耳机", "HDMI 输出"]);
    // 开浮层 + 几何（图标上方居中）。
    let g = v.toggle((1800, 1040), 200, 1080);
    set.add(
        "f422-geometry-above",
        v.open && g == (1800, 840),
        "",
    );
    // 顶越界收敛。
    set.add("f422-geometry-clamped", VolumeFlyout::geometry((100, 50), 200, 1080) == (100, 0), "");
    // 拖动实时生效：三步全 <50ms。
    set.add(
        "f422-drag-live",
        v.drag_to(600, 20) == 600 && v.drag_to(850, 40) == 850 && v.drag_to(999, 45) == 999 && v.drag_over_budget == 0,
        "",
    );
    // 超线如实记账。
    let _ = v.drag_to(300, 90);
    set.add("f422-drag-over-logged", v.drag_over_budget == 1, "");
    // 设备名与 F241 同源（注入同一字符串，读回一致）。
    set.add("f422-device-same-source", v.device_name == "扬声器 (Y7000)", "");
    // 下拉切换（清单内/外）。
    set.add(
        "f422-device-switch",
        v.switch_device("耳机") && v.device_name == "耳机" && !v.switch_device("蓝牙音箱"),
        "",
    );
    // 混音器入口。
    set.add("f422-mixer-jump", v.mixer_jump() == "f157.mixer", "");
    // Esc 关闭 + 再关无动作。
    set.add("f422-esc-close", v.esc_close() && !v.open && !v.esc_close(), "");
    // v5：静音位独立——静音翻转不动滑条值；拖动即解除静音。
    let mut w = VolumeFlyout::new("扬声器", alloc::vec!["扬声器"]);
    set.add("f422-mute-toggle", !w.muted && w.toggle_mute() && w.muted, "");
    set.add(
        "f422-mute-keeps-slider",
        w.volume_permille == 400 && w.toggle_mute() == false && w.volume_permille == 400,
        "",
    );
    w.muted = true;
    set.add("f422-drag-unmutes", w.drag_to(500, 20) == 500 && !w.muted, "");
    // v5：拖动取消——恢复起拖值（不给半截状态）。
    w.volume_permille = 700;
    w.begin_drag(1_000);
    w.volume_permille = 200;
    set.add(
        "f422-drag-cancel-restores",
        w.cancel_drag() && w.volume_permille == 700 && w.drag_cancels == 1 && !w.cancel_drag(),
        "",
    );
    // v8：拖动无动作超时——超窗自动恢复，窗内不误恢复，无起拖不动作。
    w.volume_permille = 700;
    w.begin_drag(2_000);
    w.volume_permille = 300;
    set.add("f422-drag-stall-in-window", !w.drag_stalled(9_999) && w.volume_permille == 300, "");
    set.add(
        "f422-drag-stall-restores",
        w.drag_stalled(12_001) && w.volume_permille == 700 && !w.drag_stalled(12_002),
        "",
    );
    // v8：音量步进键——±5% 步进、边界钳制、连击限速记账。
    let mut s = VolumeFlyout::new("扬声器", alloc::vec!["扬声器"]);
    set.add(
        "f422-step-volume",
        s.step_volume(1, 0) == Some(450)
            && s.step_volume(1, 500) == Some(500)
            && s.step_volume(-1, 600) == Some(450)
            && s.step_volume(1, 620).is_none()
            && s.step_rejects == 1,
        "",
    );
    s.volume_permille = 990;
    let _ = s.step_volume(1, 2_000);
    s.volume_permille = 10;
    let _ = s.step_volume(-1, 3_000);
    set.add("f422-step-clamped", s.volume_permille == 0 && s.last_step_at_ms.is_some(), "");
    // v8：设备热插拔回退——当前设备消失回落默认扬声器（首项）+ 显性入账。
    let mut h = VolumeFlyout::new("耳机", alloc::vec!["耳机", "扬声器"]);
    let _ = h.switch_device("耳机");
    set.add(
        "f422-hotplug-fallback",
        h.device_plug_event(alloc::vec!["扬声器"]) && h.device_name == "扬声器" && h.device_fallbacks == 1,
        "",
    );
    set.add("f422-hotplug-noop", !h.device_plug_event(alloc::vec!["扬声器", "耳机"]) && h.device_fallbacks == 1, "");
    // v5：应用级混音——钳制 + 容量上限拒绝（不静默挤掉）。
    set.add(
        "f422-app-volume-clamped",
        w.set_app_volume("浏览器", 800)
            && w.set_app_volume("游戏", 1_500)
            && w.app_volumes.iter().find(|(n, _)| *n == "游戏").map(|(_, v)| *v) == Some(1_000),
        "",
    );
    const FILLERS: [&str; APP_MIXER_CAP] = [
        "占位0", "占位1", "占位2", "占位3", "占位4", "占位5", "占位6", "占位7",
        "占位8", "占位9", "占位10", "占位11", "占位12", "占位13", "占位14", "占位15",
    ];
    for name in FILLERS {
        let _ = w.set_app_volume(name, 100);
    }
    set.add("f422-app-volume-cap", !w.set_app_volume("第 17 个", 100), "");
    // v5：音量快照 round-trip（重启恢复）。
    let snap = w.volume_snapshot();
    w.volume_permille = 0;
    set.add("f422-volume-roundtrip", w.restore_volume(snap) == 700, "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_bottom_edge_safe() {
        // 图标在底部任务栏：浮层向上展开不越底。
        let g = VolumeFlyout::geometry((1900, 1070), 200, 1080);
        assert_eq!(g, (1900, 870));
    }

    #[test]
    fn volume_clamped() {
        let mut v = VolumeFlyout::new("spk", alloc::vec!["spk"]);
        assert_eq!(v.drag_to(1_500, 10), 1_000);
        assert_eq!(v.drag_to(5, 10), 5);
    }

    #[test]
    fn cancel_drag_without_begin_is_false() {
        let mut v = VolumeFlyout::new("spk", alloc::vec!["spk"]);
        assert!(!v.cancel_drag());
        assert_eq!(v.drag_cancels, 0);
    }

    #[test]
    fn step_debounce_first_press_passes() {
        // 首击（无历史）不防抖——t=0 步进必须生效。
        let mut v = VolumeFlyout::new("spk", alloc::vec!["spk"]);
        assert_eq!(v.step_volume(1, 0), Some(450));
        assert_eq!(v.step_rejects, 0);
    }

    #[test]
    fn stall_without_drag_is_noop() {
        let mut v = VolumeFlyout::new("spk", alloc::vec!["spk"]);
        assert!(!v.drag_stalled(99_999));
    }

    #[test]
    fn hotplug_keeps_device_when_present() {
        let mut v = VolumeFlyout::new("耳机", alloc::vec!["扬声器", "耳机"]);
        assert!(!v.device_plug_event(alloc::vec!["耳机", "扬声器"]));
        assert_eq!(v.device_name, "耳机");
        assert_eq!(v.device_fallbacks, 0);
    }
}
