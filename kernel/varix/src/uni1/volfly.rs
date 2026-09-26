//! F422 音量图标浮层 · 完整设计（STAR I 主册 G-I-22）。
//!
//! **判据（主册）**：拖动实时生效判据（<50ms 音频响应）；设备名与
//! F241 一致；下拉切换；混音器跳转；浮层几何（图标上方居中）。＋通12。
//!
//! 设计：音量浮层语义核——拖条实时生效账（每步注入音频延迟，<50ms
//! 判线）；设备名同源（F241 注入同一字符串）；设备下拉切换；混音器
//! 入口（F157）；浮层几何计算（图标上方居中，屏幕边缘收敛）。

use crate::checks::CheckSet;

use alloc::vec::Vec;
use crate::uni1::ubase::{LayerStack, LayerTier};

/// 拖动实时生效判线（ms）。
pub const LIVE_APPLY_MS: u64 = 50;

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
    pub fn drag_to(&mut self, permille: u64, audio_latency_ms: u64) -> u64 {
        self.volume_permille = permille.clamp(0, 1_000);
        self.drag_steps += 1;
        if audio_latency_ms > LIVE_APPLY_MS {
            self.drag_over_budget += 1;
        }
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
}
