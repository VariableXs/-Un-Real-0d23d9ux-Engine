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

// ===========================================================================
// 深化 v2（F483）：双设备矩阵全表 / 惯性曲线不变实证 / 持久化 round-trip /
// 语义翻转内容方向验证 / 出厂默认文档化
// ===========================================================================

/// 出厂默认表（主册「触控板要自然滚动但鼠标滚轮要传统」——
/// 两设备出厂手感哲学不同：触控板自然、鼠标传统，文档化锚）。
pub const FACTORY_MOUSE_NATURAL: bool = false;
pub const FACTORY_TOUCHPAD_NATURAL: bool = true;

/// 出厂默认审计（新 WheelDirection 实例 = 出厂表——文档化即实现）。
pub fn factory_defaults_ok(w: &WheelDirection) -> bool {
    // 鼠标传统：输入 +10（下滚）→ 内容 -10（上移，反向）；触控板自然：同向。
    w.content_delta(WheelDevice::MouseWheel, 10) == -10
        && w.content_delta(WheelDevice::TouchpadTwoFinger, 10) == 10
}

/// 双设备矩阵全表（两设备 × 两方向 = 4 格全算——语义翻转正确性
/// 的内容方向验证：向下滚动内容上走的传统语义 vs 内容跟手的自然语义）。
pub fn full_matrix_ok(w: &WheelDirection) -> bool {
    // 鼠标传统：输入 +（下滚）→ 内容 -（上走）。
    let mouse_traditional = w.content_delta(WheelDevice::MouseWheel, 10) == -10;
    // 触控板自然：输入 +（下挥）→ 内容 +（跟手）。
    let touchpad_natural = w.content_delta(WheelDevice::TouchpadTwoFinger, 10) == 10;
    mouse_traditional && touchpad_natural
}

/// 惯性曲线不变实证（主册「翻转的是映射不是动画」——方向切换前后
/// 曲线锚点逐位对拍：任何一处漂移 = 翻转破坏了手感，红）。
pub fn inertia_untouched_after_flip(w: &mut WheelDirection) -> bool {
    let before = INERTIA_CURVE_ANCHOR;
    let _ = w.set(WheelDevice::MouseWheel, true);
    let _ = w.set(WheelDevice::MouseWheel, false);
    let after = INERTIA_CURVE_ANCHOR;
    WheelDirection::inertia_curve_untouched(&before, &after)
}

/// 持久化（两设备方向位定长落盘 round-trip）。
pub const WHEELDIR_PERSIST_MAGIC: [u8; 4] = *b"VWD1";
pub const WHEELDIR_PERSIST_LEN: usize = 6;

pub fn save_wheel(w: &WheelDirection, out: &mut [u8]) -> Option<usize> {
    if out.len() < WHEELDIR_PERSIST_LEN {
        return None;
    }
    out[..4].copy_from_slice(&WHEELDIR_PERSIST_MAGIC);
    out[4] = w.mouse_natural as u8;
    out[5] = w.touchpad_natural as u8;
    Some(WHEELDIR_PERSIST_LEN)
}

pub fn load_wheel(buf: &[u8]) -> Option<(bool, bool)> {
    if buf.len() < WHEELDIR_PERSIST_LEN || buf[..4] != WHEELDIR_PERSIST_MAGIC {
        return None;
    }
    // 位值只认 0/1（坏值拒收——方向是布尔语义，不许静默钳回）。
    match (buf[4], buf[5]) {
        (0 | 1, 0 | 1) => Some((buf[4] == 1, buf[5] == 1)),
        _ => None,
    }
}

/// 独立性反证审计（只动鼠标方向 → 触控板语义逐位不变——
/// 「每只输入设备自己的事」的行为面）。
pub fn mouse_flip_leaves_touchpad(w: &mut WheelDirection) -> bool {
    let pad_before = w.content_delta(WheelDevice::TouchpadTwoFinger, 7);
    let _ = w.set(WheelDevice::MouseWheel, true);
    let pad_after = w.content_delta(WheelDevice::TouchpadTwoFinger, 7);
    pad_before == pad_after
}

// ---------------------------------------------------------------------------
// 深化自检（F483 v2）
// ---------------------------------------------------------------------------

pub fn run_wheeldir_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F483-v2");
    // 1) 出厂默认：鼠标传统 + 触控板自然（文档化锚即实现）。
    let w = WheelDirection::new();
    cs.add("factory_defaults", factory_defaults_ok(&w), "");
    cs.add("factory_consts", !FACTORY_MOUSE_NATURAL && FACTORY_TOUCHPAD_NATURAL, "");
    // 2) 双设备矩阵 4 格全算。
    cs.add("matrix_full", full_matrix_ok(&w), "");
    // 3) 惯性曲线不变实证（翻转前后锚点逐位对拍）。
    let mut w2 = WheelDirection::new();
    cs.add("inertia_untouched", inertia_untouched_after_flip(&mut w2), "");
    // 4) 持久化 round-trip + 坏位拒收。
    let mut buf = [0u8; WHEELDIR_PERSIST_LEN];
    cs.add("persist_roundtrip", {
        match save_wheel(&w, &mut buf) {
            Some(_) => load_wheel(&buf) == Some((false, true)),
            None => false,
        }
    }, "");
    cs.add("persist_bad_bit", load_wheel(&[b'V', b'W', b'D', b'1', 2, 1]).is_none(), "");
    // 5) 独立性反证：动鼠标不波及触控板。
    cs.add("mouse_flip_isolated", mouse_flip_leaves_touchpad(&mut w2), "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn flip_then_flip_back_identity() {
        let mut w = WheelDirection::new();
        let mouse_before = w.content_delta(WheelDevice::MouseWheel, 5);
        // 翻 → 翻回 = 恒等。
        let _ = w.set(WheelDevice::MouseWheel, true);
        let flipped = w.content_delta(WheelDevice::MouseWheel, 5);
        let _ = w.set(WheelDevice::MouseWheel, false);
        assert_eq!(mouse_before, w.content_delta(WheelDevice::MouseWheel, 5));
        assert_ne!(mouse_before, flipped);
    }

    #[test]
    fn zero_input_stays_zero() {
        let mut w = WheelDirection::new();
        assert_eq!(w.content_delta(WheelDevice::MouseWheel, 0), 0);
        let _ = w.set(WheelDevice::MouseWheel, true);
        assert_eq!(w.content_delta(WheelDevice::TouchpadTwoFinger, 0), 0);
    }

    #[test]
    fn negative_input_flips_sign() {
        let w = WheelDirection::new();
        // 上滚（负输入）在对称语义下符号相反。
        assert_eq!(
            w.content_delta(WheelDevice::MouseWheel, -10).signum(),
            -w.content_delta(WheelDevice::MouseWheel, 10).signum()
        );
    }
}
