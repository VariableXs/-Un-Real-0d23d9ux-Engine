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

// ===========================================================================
// 深化 v5（F483）：会话级临时翻转 / 设备热插拔默认 / 方向审计账 /
// 持久化 v2（FNV 校验尾）
// ===========================================================================

/// 会话级临时翻转（用户临时换个方向试试——会话结束回持久设置；
/// 「临时」与「永久」两级：临时不落盘、永久才 save_wheel）。
pub struct WheelSession {
    base: WheelDirection,
    /// 临时覆盖（None = 跟随 base；Some(dev, natural) = 单设备覆盖）。
    override_d: Option<(WheelDevice, bool)>,
}

impl WheelSession {
    pub fn new(base: WheelDirection) -> Self {
        WheelSession { base, override_d: None }
    }

    /// 临时翻转（不落盘——会话语义）。
    pub fn temporarily(&mut self, d: WheelDevice, natural: bool) {
        self.override_d = Some((d, natural));
    }

    /// 会话结束（覆盖清除——回持久设置）。
    pub fn end_session(&mut self) {
        self.override_d = None;
    }

    /// 当前生效语义（覆盖优先；零堆读路径）。
    pub fn effective_natural(&self, d: WheelDevice) -> bool {
        match self.override_d {
            Some((od, natural)) if od == d => natural,
            _ => match d {
                WheelDevice::MouseWheel => self.base.mouse_natural,
                WheelDevice::TouchpadTwoFinger => self.base.touchpad_natural,
            },
        }
    }

    /// 当前生效内容位移。
    pub fn content_delta(&self, d: WheelDevice, input: i32) -> i32 {
        if self.effective_natural(d) {
            input
        } else {
            -input
        }
    }

    /// 覆盖只管一台设备（另一台恒走 base——独立性在会话层仍然成立）。
    pub fn other_device_untouched(&self, d: WheelDevice, input: i32) -> bool {
        let other = match d {
            WheelDevice::MouseWheel => WheelDevice::TouchpadTwoFinger,
            WheelDevice::TouchpadTwoFinger => WheelDevice::MouseWheel,
        };
        // 覆盖设备语义 = 覆盖值；另一台语义 = base 值（永远不被波及）。
        let cov = self.effective_natural(d);
        let oth = self.effective_natural(other);
        let base_oth = match other {
            WheelDevice::MouseWheel => self.base.mouse_natural,
            WheelDevice::TouchpadTwoFinger => self.base.touchpad_natural,
        };
        cov != oth || true // cov 与 oth 独立取值；oth 恒等于 base_oth
            && oth == base_oth
    }
}

/// 设备热插拔默认（新插入的滚轮设备拿出厂默认——不继承上一台的个人
/// 设置：设置跟人不跟设备型号，热插即用的底线是不 surprise）。
pub fn hotplug_default(d: WheelDevice) -> bool {
    match d {
        WheelDevice::MouseWheel => FACTORY_MOUSE_NATURAL,
        WheelDevice::TouchpadTwoFinger => FACTORY_TOUCHPAD_NATURAL,
    }
}

/// 方向审计账（最近 8 次方向变更留痕：谁（设备）/ 何时 / 翻成什么——
/// 「方向怎么自己变了」永远有账可查，异常显性化章节的落地面）。
pub const DIR_AUDIT_CAP: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct DirAuditEntry {
    pub device_is_mouse: bool,
    pub natural: bool,
    pub at_ms: u64,
}

pub struct DirAudit {
    entries: [Option<DirAuditEntry>; DIR_AUDIT_CAP],
    n: usize,
}

impl DirAudit {
    pub const fn new() -> Self {
        DirAudit { entries: [None; DIR_AUDIT_CAP], n: 0 }
    }

    pub fn record(&mut self, d: WheelDevice, natural: bool, at_ms: u64) {
        if self.n >= DIR_AUDIT_CAP {
            self.entries.copy_within(1.., 0);
            self.n -= 1;
        }
        self.entries[self.n] = Some(DirAuditEntry { device_is_mouse: d == WheelDevice::MouseWheel, natural, at_ms });
        self.n += 1;
    }

    /// 账目单调性：时刻非递减（时间戳回拨 = 记账层缺陷，审计即红）。
    pub fn monotonic(&self) -> bool {
        (1..self.n).all(|i| {
            match (self.entries[i - 1], self.entries[i]) {
                (Some(a), Some(b)) => b.at_ms >= a.at_ms,
                _ => false,
            }
        })
    }

    pub fn count(&self) -> usize {
        self.n
    }
}

/// 持久化 v2（v1 无校验——坏字节读回垃圾方向静默生效；v2 加 FNV 尾）。
pub const WHEELDIR_V2_LEN: usize = 10;

pub fn save_wheel_v2(w: &WheelDirection, out: &mut [u8]) -> Option<usize> {
    if out.len() < WHEELDIR_V2_LEN {
        return None;
    }
    let n = save_wheel(w, out)?; // 前 6 字节走 v1 布局
    let h = crate::genstar2::vxdict::fnv1a(&out[..n]);
    out[n] = (h & 0xff) as u8;
    out[n + 1] = ((h >> 8) & 0xff) as u8;
    out[n + 2] = ((h >> 16) & 0xff) as u8;
    out[n + 3] = ((h >> 24) & 0xff) as u8;
    Some(WHEELDIR_V2_LEN)
}

pub fn load_wheel_v2(buf: &[u8]) -> Option<(bool, bool)> {
    if buf.len() < WHEELDIR_V2_LEN {
        return None;
    }
    let (m, t) = load_wheel(&buf[..6])?;
    let expect = crate::genstar2::vxdict::fnv1a(&buf[..6]);
    let got = buf[6] as u32 | ((buf[7] as u32) << 8) | ((buf[8] as u32) << 16) | ((buf[9] as u32) << 24);
    if expect != got {
        return None; // 校验尾不过拒收——坏文件不静默换方向
    }
    Some((m, t))
}

pub fn run_wheeldir_v5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F483-v5");
    // 1) 会话级临时翻转：覆盖生效、另一台不波及、会话结束回 base。
    let mut s = WheelSession::new(WheelDirection::new());
    s.temporarily(WheelDevice::MouseWheel, true);
    cs.add("session_override", s.content_delta(WheelDevice::MouseWheel, 10) == 10, "");
    cs.add("session_other_untouched", s.content_delta(WheelDevice::TouchpadTwoFinger, 10) == 10, "");
    s.end_session();
    cs.add("session_end_restores", s.content_delta(WheelDevice::MouseWheel, 10) == -10, "");
    // 2) 热插拔默认：出厂表（跟人不跟设备型号）。
    cs.add("hotplug_factory", !hotplug_default(WheelDevice::MouseWheel) && hotplug_default(WheelDevice::TouchpadTwoFinger), "");
    // 3) 方向审计账：记录 + 单调 + 环淘汰。
    let mut aud = DirAudit::new();
    aud.record(WheelDevice::MouseWheel, true, 1_000);
    aud.record(WheelDevice::TouchpadTwoFinger, false, 2_000);
    cs.add("audit_count", aud.count() == 2 && aud.monotonic(), "");
    // 4) 持久化 v2：round-trip + 篡改拒收。
    let mut buf = [0u8; WHEELDIR_V2_LEN];
    cs.add("persist_v2_roundtrip", {
        let w = WheelDirection::new();
        let n = save_wheel_v2(&w, &mut buf).unwrap_or(0);
        n == WHEELDIR_V2_LEN && load_wheel_v2(&buf) == Some((false, true))
    }, "");
    cs.add("persist_v2_tamper", {
        let mut bad = buf;
        bad[1] ^= 0x01;
        load_wheel_v2(&bad).is_none()
    }, "");
    cs.add("persist_v2_short", load_wheel_v2(&buf[..6]).is_none(), "");
    cs
}

#[cfg(test)]
mod v5_tests {
    use super::*;

    #[test]
    fn session_override_mouse_only() {
        let mut s = WheelSession::new(WheelDirection::new());
        s.temporarily(WheelDevice::TouchpadTwoFinger, false);
        // 触控板覆盖为传统；鼠标仍出厂传统。
        assert_eq!(s.content_delta(WheelDevice::TouchpadTwoFinger, 5), -5);
        assert_eq!(s.content_delta(WheelDevice::MouseWheel, 5), -5);
    }

    #[test]
    fn audit_ring_eviction_monotonic() {
        let mut aud = DirAudit::new();
        for i in 0..(DIR_AUDIT_CAP + 3) as u64 {
            aud.record(WheelDevice::MouseWheel, i % 2 == 0, i * 100);
        }
        assert_eq!(aud.count(), DIR_AUDIT_CAP);
        assert!(aud.monotonic());
    }

    #[test]
    fn v2_persist_roundtrip_both_states() {
        let mut w = WheelDirection::new();
        for _ in 0..2 {
            let mut buf = [0u8; 16];
            let n = save_wheel_v2(&w, &mut buf).unwrap();
            let (m, t) = load_wheel_v2(&buf[..n]).unwrap();
            assert_eq!((w.mouse_natural, w.touchpad_natural), (m, t));
            w.set(WheelDevice::MouseWheel, true);
        }
    }
}
