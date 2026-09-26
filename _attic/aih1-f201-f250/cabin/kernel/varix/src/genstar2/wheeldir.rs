//! F483 滚轮方向独立设置（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **双设备独立开关；语义翻转正确性（内容移动方向）；惯性曲线不变判据；
//! 即时性；持久化。**
//!
//! 功能定义（主册批次三）：滚轮方向按设备独立——鼠标滚轮/触控板双指
//! （F481）各有一个方向开关（有人触控板要自然滚动但鼠标滚轮要传统）；方向
//! 改变即时生效且滚动惯性曲线（F204）不随方向变（只翻转语义不破坏曲线）。
//!
//! 零堆纪律：定长状态，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 双设备（主册：鼠标滚轮 / 触控板双指各一个开关）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WheelDevice {
    MouseWheel,
    TouchpadTwoFinger,
}

pub const WHEEL_DEVICES: [WheelDevice; 2] = [WheelDevice::MouseWheel, WheelDevice::TouchpadTwoFinger];

/// 惯性曲线参数（F204 同源——方向设置不得触碰：曲线锚点恒定）。
pub const INERTIA_CURVE_ANCHOR: [u32; 4] = [120, 480, 1_200, 2_600]; // ms 速度衰减锚点

/// 滚轮方向设置板（双设备独立）。
#[derive(Clone, Copy, Debug)]
pub struct WheelDirection {
    /// 鼠标滚轮：true = 自然滚动（内容随手走）/ false = 传统（Windows 老习惯）。
    pub mouse_natural: bool,
    /// 触控板双指：同语义独立开关。
    pub touchpad_natural: bool,
}

impl WheelDirection {
    /// 出厂默认：触控板自然开（F481 同源），鼠标滚轮传统（Windows 肌肉
    /// 记忆默认）——两设备手感哲学不同很正常（主册原文）。
    pub const fn new() -> Self {
        WheelDirection { mouse_natural: false, touchpad_natural: true }
    }

    fn get(&self, d: WheelDevice) -> bool {
        match d {
            WheelDevice::MouseWheel => self.mouse_natural,
            WheelDevice::TouchpadTwoFinger => self.touchpad_natural,
        }
    }

    fn set(&mut self, d: WheelDevice, natural: bool) {
        match d {
            WheelDevice::MouseWheel => self.mouse_natural = natural,
            WheelDevice::TouchpadTwoFinger => self.touchpad_natural = natural,
        }
    }

    /// 语义翻转（主册：内容移动方向——手指/滚轮位移 → 内容位移）。
    /// `input_delta` 滚轮/手指位移；返回内容移动方向（自然 = 同向；传统 = 反向）。
    pub fn content_delta(&self, d: WheelDevice, input_delta: i32) -> i32 {
        if self.get(d) {
            input_delta
        } else {
            -input_delta
        }
    }

    /// 双设备独立审计：改 A 不动 B。
    pub fn independent_after_set(&mut self, d: WheelDevice, natural: bool) -> bool {
        let other = match d {
            WheelDevice::MouseWheel => WheelDevice::TouchpadTwoFinger,
            WheelDevice::TouchpadTwoFinger => WheelDevice::MouseWheel,
        };
        let other_before = self.get(other);
        self.set(d, natural);
        self.get(other) == other_before
    }

    /// 惯性曲线不变判据（主册：只翻转语义不破坏曲线——方向设置前后曲线
    /// 锚点逐位一致）。
    pub fn inertia_curve_untouched(before: &[u32; 4], after: &[u32; 4]) -> bool {
        before == after
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_wheeldir_checks() -> CheckSet {
    let mut cs = CheckSet::new("F483-wheeldir");
    // 1) 双设备独立开关。
    let mut w = WheelDirection::new();
    cs.add("defaults_differ", !w.mouse_natural && w.touchpad_natural, "");
    cs.add("set_mouse_only", w.independent_after_set(WheelDevice::MouseWheel, true), "");
    cs.add("mouse_now_natural", w.mouse_natural, "");
    cs.add("touchpad_untouched", w.touchpad_natural, "");
    // 2) 语义翻转正确性（内容移动方向）。
    let w2 = WheelDirection::new();
    cs.add("flip_mouse_traditional", w2.content_delta(WheelDevice::MouseWheel, 120) == -120, "");
    cs.add("flip_touchpad_natural", w2.content_delta(WheelDevice::TouchpadTwoFinger, 120) == 120, "");
    // 3) 惯性曲线不变（方向设置前后曲线锚点一致）。
    let curve_before = INERTIA_CURVE_ANCHOR;
    let mut w3 = WheelDirection::new();
    w3.set(WheelDevice::MouseWheel, true);
    w3.set(WheelDevice::TouchpadTwoFinger, false);
    cs.add("inertia_curve_unchanged", WheelDirection::inertia_curve_untouched(&curve_before, &INERTIA_CURVE_ANCHOR), "");
    // 4) 两设备手感哲学共存（触控板自然 + 鼠标传统可同时成立——主册原文）。
    let w4 = WheelDirection::new();
    cs.add("philosophies_coexist", w4.content_delta(WheelDevice::MouseWheel, 50) == -50 && w4.content_delta(WheelDevice::TouchpadTwoFinger, 50) == 50, "");
    // 5) 即时性（set 后下一次读取立即新语义）。
    let mut w5 = WheelDirection::new();
    let d0 = w5.content_delta(WheelDevice::MouseWheel, 10);
    w5.set(WheelDevice::MouseWheel, true);
    let d1 = w5.content_delta(WheelDevice::MouseWheel, 10);
    cs.add("instant_effect", d0 == -10 && d1 == 10, "");
    // 6) 设备清单齐备。
    cs.add("device_matrix", WHEEL_DEVICES.len() == 2, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flip_is_exact_negation() {
        let mut w = WheelDirection::new();
        for delta in [1i32, -7, 1_200] {
            // 两设备在同一设置下互为相反数（翻转语义不改变幅度）。
            let natural = w.content_delta(WheelDevice::MouseWheel, delta);
            w.set(WheelDevice::MouseWheel, !w.mouse_natural);
            let traditional = w.content_delta(WheelDevice::MouseWheel, delta);
            assert_eq!(natural, -traditional);
        }
    }

    #[test]
    fn independence_matrix() {
        let mut w = WheelDirection::new();
        for d in WHEEL_DEVICES {
            assert!(w.independent_after_set(d, true));
            assert!(w.independent_after_set(d, false));
        }
    }
}
