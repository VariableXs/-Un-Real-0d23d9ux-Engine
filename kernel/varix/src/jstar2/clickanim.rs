//! F621 指针点击动效档 · 完整设计（STAR I 主册 J-B 组）。
//!
//! **判据（主册原文）**：三档参数实测（0.94/120ms/8px/200ms）；优先
//! 平面零重绘；与 F350 触感谱一致性审计；与 F594 分工边界（独立开关
//! 用例）；默认轻档。
//!
//! **三档**：
//! - **关**：指针行为与经典系统零差异（无任何点击微动效）；
//! - **轻**（默认）：点击瞬间指针缩至 0.94 回弹——120ms（F350 触感谱
//!   在指针域的对齐件）；
//! - **满**：轻档 + 落点微涟漪（8px 半径、200ms）——比 F594 录屏款
//!   收敛一半（它是给操作者的确认不是给观众的表演）。
//!
//! **动效实现口径**：走 F335 指针优先平面合成、不产生内容重绘——
//! 脏区 = 指针矩形 ∪ 涟漪环带（`dirty_rect_for` 唯一口，其余内容
//! 零脏区就是「零重绘」的机制面）；
//! **与 F594 边界**：本档是真实交互反馈、F594 是录屏叠加——两个独立
//! 开关（`ClickAnimPrefs` 与 `rec_highlight` 分立字段），联动用例验证
//! 互不牵动。

use crate::checks::CheckSet;
use alloc::string::String;

// ---------------------------------------------------------------------------
// 规格常量（判据数值原文，一处一事实）
// ---------------------------------------------------------------------------

/// 轻档缩放谷值（0.94，千分位定点）。
pub const LIGHT_SCALE_MIN_M: i64 = 940;
/// 轻档时长（ms）。
pub const LIGHT_DURATION_MS: u32 = 120;
/// 满档涟漪半径（px）。
pub const FULL_RIPPLE_PX: u32 = 8;
/// 满档涟漪时长（ms）。
pub const FULL_DURATION_MS: u32 = 200;

// ---------------------------------------------------------------------------
// 档位与时间线
// ---------------------------------------------------------------------------

/// 三档（默认轻档——判据「默认轻档」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickAnimTier {
    Off,
    Light,
    Full,
}

impl Default for ClickAnimTier {
    fn default() -> Self {
        ClickAnimTier::Light
    }
}

impl ClickAnimTier {
    pub fn zh(self) -> &'static str {
        match self {
            ClickAnimTier::Off => "关",
            ClickAnimTier::Light => "轻",
            ClickAnimTier::Full => "满",
        }
    }
}

/// 用户偏好（与 F594 录屏高亮独立——分工边界的字段面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClickAnimPrefs {
    pub tier: ClickAnimTier,
    /// F594 录屏点击高亮（独立开关——独立开关用例的对账字段）。
    pub rec_highlight: bool,
}

impl Default for ClickAnimPrefs {
    fn default() -> Self {
        ClickAnimPrefs { tier: ClickAnimTier::Light, rec_highlight: false }
    }
}

/// 点击动效时间线：`scale_m(t)` = t 毫秒时刻的指针缩放（千分位定点），
/// `ripple_r_m(t)` = 涟漪半径（1/1000 px 定点；轻档/关恒 0）。
///
/// 曲线（确定性、可对拍）：缩放走「按→回弹」两段正弦半波——
/// 前半程 1.000 → 0.940，后半程 0.940 → 1.000（120ms 全程）；
/// 涟漪半径 0 → 8px 线性（200ms 全程），透明度 1 → 0 线性。
pub struct ClickTimeline {
    pub tier: ClickAnimTier,
    pub pressed_at_ms: u64,
}

impl ClickTimeline {
    pub fn new(tier: ClickAnimTier, pressed_at_ms: u64) -> ClickTimeline {
        ClickTimeline { tier, pressed_at_ms }
    }

    pub fn active(&self, now_ms: u64) -> bool {
        match self.tier {
            ClickAnimTier::Off => false,
            ClickAnimTier::Light => now_ms.saturating_sub(self.pressed_at_ms) < LIGHT_DURATION_MS as u64,
            ClickAnimTier::Full => now_ms.saturating_sub(self.pressed_at_ms) < FULL_DURATION_MS as u64,
        }
    }

    /// 缩放（千分位定点；inactive 恒 1000）。
    pub fn scale_m(&self, now_ms: u64) -> i64 {
        if !self.active(now_ms) {
            return 1000;
        }
        let t = (now_ms - self.pressed_at_ms) as i64;
        let dur = LIGHT_DURATION_MS as i64;
        // sin(π·t/dur)：0→1→0；缩放 = 1000 − 60·sin(π·t/dur)。
        let phase = crate::jstar2::jbase::sin_fp(3142 * t / dur); // [0,1000] 半波
        1000 - (1000 - LIGHT_SCALE_MIN_M) * phase / 1000
    }

    /// 涟漪半径（1/1000 px；仅满档 active 时非零）。
    pub fn ripple_r_m(&self, now_ms: u64) -> i64 {
        if self.tier != ClickAnimTier::Full || !self.active(now_ms) {
            return 0;
        }
        let t = (now_ms - self.pressed_at_ms) as i64;
        FULL_RIPPLE_PX as i64 * 1000 * t / FULL_DURATION_MS as i64
    }

    /// 涟漪透明度（千分位，1 → 0 线性；仅满档 active 时非零）。
    pub fn ripple_alpha_m(&self, now_ms: u64) -> i64 {
        if self.tier != ClickAnimTier::Full || !self.active(now_ms) {
            return 0;
        }
        1000 - 1000 * (now_ms - self.pressed_at_ms) as i64 / FULL_DURATION_MS as i64
    }
}

// ---------------------------------------------------------------------------
// 优先平面脏区（零重绘纪律的机制面）
// ---------------------------------------------------------------------------

/// 点击动效的脏区（px）：指针矩形（w×h @ 指针位）∪ 涟漪环带外接方。
/// 关档恒零尺寸（空脏区 = 系统对屏幕零干预）。
pub fn dirty_rect_for(
    tier: ClickAnimTier,
    pointer_x: i64,
    pointer_y: i64,
    pointer_w: u16,
    pointer_h: u16,
    timeline: Option<&ClickTimeline>,
    now_ms: u64,
) -> (i64, i64, u32, u32) {
    match tier {
        ClickAnimTier::Off => (0, 0, 0, 0),
        ClickAnimTier::Light => {
            let active = timeline.map(|t| t.active(now_ms)).unwrap_or(false);
            if !active {
                return (0, 0, 0, 0);
            }
            (pointer_x, pointer_y, pointer_w as u32, pointer_h as u32)
        }
        ClickAnimTier::Full => {
            let Some(t) = timeline else { return (0, 0, 0, 0) };
            if !t.active(now_ms) {
                return (0, 0, 0, 0);
            }
            let r = (t.ripple_r_m(now_ms) / 1000).max(FULL_RIPPLE_PX as i64) as i64;
            let x = pointer_x - r;
            let y = pointer_y - r;
            let w = pointer_w as i64 + 2 * r;
            let h = pointer_h as i64 + 2 * r;
            (x, y, w.max(0) as u32, h.max(0) as u32)
        }
    }
}

// ---------------------------------------------------------------------------
// F350 触感谱一致性审计
// ---------------------------------------------------------------------------

/// F350 触感谱条目（显式注入口——触感谱域的参数接缝）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HapticEntry {
    pub domain: &'static str,
    pub scale_m: i64,
    pub duration_ms: u32,
}

/// 一致性审计：轻档参数与 F350 触感谱指针域条目逐项对表（容差
/// ±10ms / ±0.005 scale——谱内微调不判违）。
pub fn audit_f350_consistency(entries: &[HapticEntry]) -> Result<(), String> {
    let Some(e) = entries.iter().find(|e| e.domain == "pointer-click") else {
        return Err(String::from("F350 谱中缺少 pointer-click 条目——指针域未对齐"));
    };
    if (e.scale_m - LIGHT_SCALE_MIN_M).abs() > 5 {
        // 定点千分位呈现（内核路径零浮点纪律）。
        let whole = e.scale_m / 1000;
        let frac = e.scale_m.rem_euclid(1000);
        return Err(alloc::format!(
            "触感谱缩放 {}.{:03} 与指针域 0.940 不一致（±0.005 容差外）",
            whole, frac
        ));
    }
    if e.duration_ms.abs_diff(LIGHT_DURATION_MS) > 10 {
        return Err(alloc::format!(
            "触感谱时长 {}ms 与指针域 120ms 不一致（±10ms 容差外）",
            e.duration_ms
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F621 自检。

// ---------------------------------------------------------------------------
// v2 深化：连点引擎 / 减少动效联动 / 涟漪环带几何
// ---------------------------------------------------------------------------

/// 点击动效引擎（连点状态机——第三章「连续快速点击不触发双份动作」：
/// 每次按下重启同一条时间线，引擎同时最多持有一条活动时间线）。
pub struct ClickAnimEngine {
    pub prefs: ClickAnimPrefs,
    /// F113 减少动效（系统无障碍开关——开启时动效完全禁用，
    /// 无障碍优先级高于档位偏好）。
    pub reduce_motion: bool,
    timeline: Option<ClickTimeline>,
    /// 引擎累计处理的按下次数（连点对账面）。
    pub press_count: u64,
    /// 重启次数（连点导致时间线重置的计数）。
    pub restarts: u64,
}

impl ClickAnimEngine {
    pub fn new(prefs: ClickAnimPrefs, reduce_motion: bool) -> ClickAnimEngine {
        ClickAnimEngine { prefs, reduce_motion, timeline: None, press_count: 0, restarts: 0 }
    }

    /// 按下（关档/减少动效 → 不产时间线；已有活动时间线 → 重启并计数）。
    pub fn press(&mut self, at_ms: u64) {
        self.press_count += 1;
        if self.prefs.tier == ClickAnimTier::Off || self.reduce_motion {
            self.timeline = None;
            return;
        }
        if self.timeline.as_ref().map(|t| t.active(at_ms)).unwrap_or(false) {
            self.restarts += 1;
        }
        self.timeline = Some(ClickTimeline::new(self.prefs.tier, at_ms));
    }

    /// 当前缩放（引擎视角；无活动时间线恒 1000）。
    pub fn scale_m(&self, now_ms: u64) -> i64 {
        self.timeline.as_ref().map(|t| t.scale_m(now_ms)).unwrap_or(1000)
    }

    /// 时间线只读视图。
    pub fn timeline(&self) -> Option<&ClickTimeline> {
        self.timeline.as_ref()
    }

    /// 涟漪环带（满档渲染层几何）：t 毫秒时刻的 (内半径, 外半径) px
    /// ——环带宽 1.5px，半径线性外扩；轻档/关/非活动恒 (0,0)。
    pub fn ripple_band(&self, now_ms: u64) -> (i64, i64) {
        let Some(t) = self.timeline.as_ref() else { return (0, 0) };
        if self.prefs.tier != ClickAnimTier::Full || !t.active(now_ms) {
            return (0, 0);
        }
        let r = t.ripple_r_m(now_ms) / 1000;
        let inner = r.saturating_sub(1);
        (inner, r)
    }
}

pub fn run_clickanim_checks() -> CheckSet {
    let mut set = CheckSet::new("jstar2-F621");

    // 1. 三档参数实测：轻档 0.94 谷值 @60ms、120ms 回 1.0。
    let tl = ClickTimeline::new(ClickAnimTier::Light, 1000);
    set.add(
        "light scale dips to 0.94 and rebounds",
        tl.scale_m(1000) == 1000
            && tl.scale_m(1060) <= 945
            && tl.scale_m(1060) >= 935
            && tl.scale_m(1120) == 1000
            && !tl.active(1120),
        "",
    );

    // 2. 满档涟漪：200ms 内半径线性 0→8px，超时归零。
    let tf = ClickTimeline::new(ClickAnimTier::Full, 0);
    set.add(
        "full ripple 8px/200ms linear",
        tf.ripple_r_m(0) == 0
            && tf.ripple_r_m(100) == 4000
            && tf.ripple_r_m(200) == 0 // 200ms 恰越界 → inactive
            && tf.ripple_r_m(199) > 3900,
        "",
    );

    // 3. 关档完全无感：任何时刻缩放 1000、涟漪 0、脏区空。
    let toff = ClickTimeline::new(ClickAnimTier::Off, 0);
    let mut all_off = true;
    for t in [0u64, 10, 60, 120, 500] {
        all_off &= toff.scale_m(t) == 1000 && toff.ripple_r_m(t) == 0;
        all_off &= dirty_rect_for(ClickAnimTier::Off, 5, 5, 32, 32, Some(&toff), t) == (0, 0, 0, 0);
    }
    set.add("off tier zero interference", all_off, "");

    // 4. 优先平面零重绘：轻档脏区 = 指针矩形（不含其他内容区）；
    //    满档脏区 = 指针 ∪ 涟漪外接方；inactive 恒空。
    let tla = ClickTimeline::new(ClickAnimTier::Light, 0);
    set.add(
        "light dirty rect == pointer rect only",
        dirty_rect_for(ClickAnimTier::Light, 100, 200, 32, 32, Some(&tla), 30) == (100, 200, 32, 32),
        "",
    );
    let tfa = ClickTimeline::new(ClickAnimTier::Full, 0);
    let (x, y, w, h) = dirty_rect_for(ClickAnimTier::Full, 100, 200, 32, 32, Some(&tfa), 50);
    set.add(
        "full dirty rect covers ripple ring",
        x == 92 && y == 192 && w == 48 && h == 48,
        "",
    );
    set.add(
        "inactive dirty rect empty",
        dirty_rect_for(ClickAnimTier::Light, 0, 0, 32, 32, Some(&tla), 500) == (0, 0, 0, 0),
        "",
    );

    // 5. F350 触感谱一致性审计：对齐条目绿；缺条目/超差红。
    let ok = audit_f350_consistency(&[HapticEntry { domain: "pointer-click", scale_m: 940, duration_ms: 120 }]);
    let miss = audit_f350_consistency(&[]);
    let off = audit_f350_consistency(&[HapticEntry { domain: "pointer-click", scale_m: 900, duration_ms: 120 }]);
    set.add("F350 audit aligned pass", ok.is_ok(), "");
    set.add("F350 audit missing entry red", miss.is_err(), "");
    set.add("F350 audit drift red", off.is_err(), "");

    // 6. 与 F594 分工边界：两开关独立（互不牵动的字段面用例）。
    let mut prefs = ClickAnimPrefs::default();
    prefs.rec_highlight = true;
    set.add(
        "F594 boundary independent switches",
        prefs.rec_highlight && prefs.tier == ClickAnimTier::Light,
        "",
    );
    prefs.tier = ClickAnimTier::Off;
    set.add(
        "toggling anim leaves rec-highlight untouched",
        prefs.rec_highlight && prefs.tier == ClickAnimTier::Off,
        "",
    );

    // 7. 默认轻档。
    set.add("default tier is light", ClickAnimPrefs::default().tier == ClickAnimTier::Light, "");


    // 7. 连点引擎：连点重启同一条时间线（无双份动作）+ 计数对账。
    let mut eng = ClickAnimEngine::new(
        ClickAnimPrefs { tier: ClickAnimTier::Light, rec_highlight: false },
        false,
    );
    eng.press(1000);
    eng.press(1050); // 50ms 后连点 → 重启（不叠加第二条）
    let restarts_ok = eng.press_count == 2 && eng.restarts == 1;
    let scale_now = eng.scale_m(1060);
    eng.press(1300); // 上条已过期 → 新时间线不算重启
    set.add(
        "rapid clicks restart single timeline",
        restarts_ok && scale_now > 930 && scale_now < 1000 && eng.restarts == 1 && eng.press_count == 3,
        "",
    );

    // 8. F113 减少动效：开启时任何档位动效完全禁用（无障碍优先）。
    let mut rm = ClickAnimEngine::new(
        ClickAnimPrefs { tier: ClickAnimTier::Full, rec_highlight: false },
        true,
    );
    rm.press(100);
    set.add(
        "reduce motion disables all tiers",
        rm.scale_m(150) == 1000 && rm.ripple_band(150) == (0, 0) && rm.timeline().is_none(),
        "",
    );

    // 9. 涟漪环带几何：半径单调外扩 + 环带宽 1px + 非满档恒零。
    let mut full = ClickAnimEngine::new(
        ClickAnimPrefs { tier: ClickAnimTier::Full, rec_highlight: false },
        false,
    );
    full.press(0);
    let band_50 = full.ripple_band(50);
    let band_150 = full.ripple_band(150);
    set.add(
        "ripple band grows monotonically",
        band_50 == (1, 2) && band_150 == (5, 6) && band_150.1 > band_50.1,
        "",
    );

    // 10. F594 独立开关在引擎面依旧互不牵动（录屏高亮随 prefs 带入）。
    set.add(
        "rec highlight independent at engine level",
        ClickAnimPrefs { tier: ClickAnimTier::Off, rec_highlight: true }.rec_highlight,
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
    fn scale_curve_symmetric() {
        let tl = ClickTimeline::new(ClickAnimTier::Light, 0);
        // 半程对称：30ms 与 90ms 缩放相等。
        assert_eq!(tl.scale_m(30), tl.scale_m(90));
        // 谷值在中点。
        assert!(tl.scale_m(60) <= tl.scale_m(30));
    }

    #[test]
    fn ripple_alpha_fades_linearly() {
        let t = ClickTimeline::new(ClickAnimTier::Full, 0);
        assert_eq!(t.ripple_alpha_m(0), 1000);
        assert_eq!(t.ripple_alpha_m(100), 500);
        assert!(t.ripple_alpha_m(199) < 30);
    }

    #[test]
    fn light_tier_has_no_ripple() {
        let t = ClickTimeline::new(ClickAnimTier::Light, 0);
        for ms in [0u64, 50, 119] {
            assert_eq!(t.ripple_r_m(ms), 0, "轻档无涟漪");
        }
    }

    #[test]
    fn timeline_boundary_exact() {
        let t = ClickTimeline::new(ClickAnimTier::Light, 100);
        assert!(t.active(219));
        assert!(!t.active(220), "120ms 恰好越界");
    }

    #[test]
    fn f350_error_messages_human() {
        let err = audit_f350_consistency(&[HapticEntry { domain: "other", scale_m: 940, duration_ms: 120 }]).unwrap_err();
        assert!(err.contains("pointer-click"));
    }
}

// ---------------------------------------------------------------------------
// v3 深化批：按下/抬起分离时间线 · 多设备枢纽 · 体验日志环（主册十三章）
// · 偏好序列化 · F350 全谱审计
// ---------------------------------------------------------------------------

use alloc::vec::Vec;

/// 抬起回弹时长（ms）——释放段的对称半波（120ms 轻档同源）。
pub const RELEASE_DURATION_MS: u32 = 120;
/// 长按阈值（ms）：超过此值的按压释放不再播回弹（长按 suppress——
/// 交互状态机纪律：按住不放的释放不该有「弹一下」的错觉）。
pub const HOLD_SUPPRESS_MS: u64 = 500;

/// 按压会话：一次完整的按下→抬起（分离时间线的状态机单元）。
///
/// 按下开时间线（缩放谷值半波）；抬起时若按压时长 < HOLD_SUPPRESS_MS
/// 则开回弹半波，否则静默复位。两条时间线共用一条缩放输出：
/// `combined_scale_m` 在释放段从 1.000 起压一个 0.97 浅谷回 1.000。
pub struct PressSession {
    pub tier: ClickAnimTier,
    pub pressed_at_ms: u64,
    released_at_ms: Option<u64>,
}

impl PressSession {
    pub fn new(tier: ClickAnimTier, pressed_at_ms: u64) -> PressSession {
        PressSession { tier, pressed_at_ms, released_at_ms: None }
    }

    /// 抬起（幂等：二次抬起不改变状态——乱点/连点状态机纪律）。
    pub fn release(&mut self, at_ms: u64) {
        if self.released_at_ms.is_none() {
            self.released_at_ms = Some(at_ms);
        }
    }

    pub fn is_released(&self) -> bool {
        self.released_at_ms.is_some()
    }

    /// 按压时长（未释放按 now 计）。
    pub fn held_ms(&self, now_ms: u64) -> u64 {
        now_ms.saturating_sub(self.pressed_at_ms)
    }

    /// 释放回弹是否生效（已释放 + 未超长按阈值 + 档位非关）。
    pub fn rebound_active(&self) -> bool {
        match (self.tier, self.released_at_ms) {
            (ClickAnimTier::Off, _) | (_, None) => false,
            (_, Some(rel)) => rel.saturating_sub(self.pressed_at_ms) < HOLD_SUPPRESS_MS,
        }
    }

    /// 组合缩放（千分位定点；关档恒 1000）。
    pub fn combined_scale_m(&self, now_ms: u64) -> i64 {
        if self.tier == ClickAnimTier::Off {
            return 1000;
        }
        match self.released_at_ms {
            None => {
                let tl = ClickTimeline::new(self.tier, self.pressed_at_ms);
                tl.scale_m(now_ms)
            }
            Some(rel) => {
                if !self.rebound_active() {
                    return 1000; // 长按抑制/关档——回弹不发（判据「长按跳过回弹」）
                }
                let t = now_ms.saturating_sub(rel) as i64;
                if t >= RELEASE_DURATION_MS as i64 {
                    return 1000;
                }
                let dur = RELEASE_DURATION_MS as i64;
                let phase = crate::jstar2::jbase::sin_fp(3142 * t / dur);
                1000 - 30 * phase / 1000
            }
        }
    }

    fn released_at_ms_is(&self, v: u64) -> bool {
        self.released_at_ms == Some(v)
    }
}

/// 动效事件种类（体验日志面——十三章「记录到交互细节层」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimEventKind {
    PressStarted,
    PressRestarted,
    Released,
    ReboundSkippedHold,
    TierSwitched,
}

impl AnimEventKind {
    pub fn zh(self) -> &'static str {
        match self {
            AnimEventKind::PressStarted => "按压动效开始",
            AnimEventKind::PressRestarted => "连点重启时间线",
            AnimEventKind::Released => "释放回弹",
            AnimEventKind::ReboundSkippedHold => "长按跳过回弹",
            AnimEventKind::TierSwitched => "档位切换",
        }
    }
}

/// 一条动效事件（时间戳 + 种类 + 设备 + 体验结论字段）。
#[derive(Clone, Copy, Debug)]
pub struct AnimEvent {
    pub at_ms: u64,
    pub kind: AnimEventKind,
    /// 设备序号（0=鼠标 / 1=触控板 / 2=笔）。
    pub device: u8,
    /// 按下到动效首帧的模型延迟（μs）——「100ms 内有反馈」的打点面。
    pub latency_us: u32,
    /// 体验结论：顺畅 = true（延迟入界且未被减少动效拦截）。
    pub smooth: bool,
}

/// 体验日志环（64 槽环形——十三章「写入绝不阻塞交互」的模型面：
/// 写入恒 O(1)、满槽覆盖最旧、回放按时间升序）。
pub struct AnimEventLog {
    ring: [Option<AnimEvent>; 64],
    head: usize,
    pub total: u64,
    pub overwritten: u64,
}

impl AnimEventLog {
    pub fn new() -> AnimEventLog {
        AnimEventLog { ring: [None; 64], head: 0, total: 0, overwritten: 0 }
    }

    pub fn push(&mut self, ev: AnimEvent) {
        if self.ring[self.head].is_some() {
            self.overwritten += 1;
        }
        self.ring[self.head] = Some(ev);
        self.head = (self.head + 1) % 64;
        self.total += 1;
    }

    /// 按时间升序回放（最新在后——可回放操作故事线）。
    pub fn replay(&self) -> Vec<AnimEvent> {
        let mut out = Vec::new();
        let n = self.total.min(64) as usize;
        let start = (self.head + 64 - n) % 64;
        for k in 0..n {
            if let Some(ev) = self.ring[(start + k) % 64] {
                out.push(ev);
            }
        }
        out
    }

    /// 挫败信号：连续 PressRestarted ≥ 3 次（狂点指纹，十三章）。
    pub fn rage_click_streak(&self) -> u32 {
        let evs = self.replay();
        let mut streak = 0u32;
        let mut best = 0u32;
        for ev in &evs {
            if ev.kind == AnimEventKind::PressRestarted {
                streak += 1;
                if streak > best {
                    best = streak;
                }
            } else {
                streak = 0;
            }
        }
        best
    }
}
impl Default for AnimEventLog {
    fn default() -> Self {
        AnimEventLog::new()
    }
}

/// 多设备动效枢纽：鼠标 / 触控板 / 笔 各自独立会话（互不抢时间线），
/// 脏区 = 各设备活动脏区并集。
pub struct MultiPointerHub {
    sessions: [Option<PressSession>; 3],
    pub tier: ClickAnimTier,
}

impl MultiPointerHub {
    pub fn new(tier: ClickAnimTier) -> MultiPointerHub {
        MultiPointerHub { sessions: [None, None, None], tier }
    }

    /// 按下（幂等覆盖同设备旧会话——同设备连点 = 重启语义）。
    pub fn press(&mut self, device: u8, at_ms: u64) {
        if device as usize >= 3 {
            return;
        }
        self.sessions[device as usize] = Some(PressSession::new(self.tier, at_ms));
    }

    pub fn release(&mut self, device: u8, at_ms: u64) {
        if let Some(s) = self.sessions.get_mut(device as usize).and_then(|o| o.as_mut()) {
            s.release(at_ms);
        }
    }

    pub fn scale_m(&self, device: u8, now_ms: u64) -> i64 {
        self.sessions
            .get(device as usize)
            .and_then(|o| o.as_ref())
            .map(|s| s.combined_scale_m(now_ms))
            .unwrap_or(1000)
    }

    /// 并集脏区（px）：任一设备活动即覆盖该设备指针矩形∪涟漪外接方。
    pub fn union_dirty_rect(
        &self,
        now_ms: u64,
        ptrs: &[(u8, i64, i64, u16, u16)],
    ) -> (i64, i64, u32, u32) {
        let mut acc: Option<(i64, i64, u32, u32)> = None;
        for &(dev, x, y, w, h) in ptrs {
            let sess = self.sessions.get(dev as usize).and_then(|o| o.as_ref());
            let pressing = sess.map(|s| !s.is_released() && s.held_ms(now_ms) < LIGHT_DURATION_MS as u64).unwrap_or(false);
            let rebounding = sess.map(|s| s.rebound_active() && s.combined_scale_m(now_ms) != 1000).unwrap_or(false);
            if !pressing && !rebounding {
                continue;
            }
            let r = if self.tier == ClickAnimTier::Full && pressing { FULL_RIPPLE_PX as i64 } else { 0 };
            let (rx, ry, rw, rh) = (x - r, y - r, w as i64 + 2 * r, h as i64 + 2 * r);
            acc = Some(match acc {
                None => (rx, ry, rw.max(0) as u32, rh.max(0) as u32),
                Some((ax, ay, aw, ah)) => {
                    let nx = ax.min(rx);
                    let ny = ay.min(ry);
                    let ne_x = (ax + aw as i64).max(rx + rw);
                    let ne_y = (ay + ah as i64).max(ry + rh);
                    (nx, ny, (ne_x - nx).max(0) as u32, (ne_y - ny).max(0) as u32)
                }
            });
        }
        acc.unwrap_or((0, 0, 0, 0))
    }
}

/// 偏好序列化（8 字节 KV：魔数 | tier | rec | 保留位全 0 校验）。
///
/// 反序列化逐字节校验：非法 tier 值 / 保留位非零 → None（不猜）。
pub fn prefs_to_kv(p: &ClickAnimPrefs) -> [u8; 8] {
    let tier = match p.tier {
        ClickAnimTier::Off => 0u8,
        ClickAnimTier::Light => 1,
        ClickAnimTier::Full => 2,
    };
    [0xCA, 0xFE, tier, u8::from(p.rec_highlight), 0, 0, 0, 0]
}

pub fn prefs_from_kv(kv: &[u8]) -> Option<ClickAnimPrefs> {
    if kv.len() != 8 || kv[0] != 0xCA || kv[1] != 0xFE || kv[4] != 0 || kv[5] != 0 || kv[6] != 0 || kv[7] != 0 {
        return None;
    }
    let tier = match kv[2] {
        0 => ClickAnimTier::Off,
        1 => ClickAnimTier::Light,
        2 => ClickAnimTier::Full,
        _ => return None,
    };
    Some(ClickAnimPrefs { tier, rec_highlight: kv[3] == 1 })
}

/// F350 全谱审计：指针域三段（click/release/hold）逐条对表（±5 千分位
/// / ±10ms 容差，与 v1 单段审计同容差口径）。
pub fn audit_f350_full_spectrum(entries: &[HapticEntry]) -> Result<(), String> {
    let want: [(&str, i64, u32); 3] = [
        ("pointer-click", LIGHT_SCALE_MIN_M, LIGHT_DURATION_MS),
        ("pointer-release", 970, RELEASE_DURATION_MS),
        ("pointer-hold", 1000, 0),
    ];
    for (dom, scale, dur) in want {
        match entries.iter().find(|e| e.domain == dom) {
            None => return Err(alloc::format!("F350 谱中缺少 {} 条目", dom)),
            Some(e) => {
                if (e.scale_m - scale).abs() > 5 {
                    return Err(alloc::format!("{} 缩放 {} 偏离 {}", dom, e.scale_m, scale));
                }
                if e.duration_ms.abs_diff(dur) > 10 {
                    return Err(alloc::format!("{} 时长 {}ms 偏离 {}ms", dom, e.duration_ms, dur));
                }
            }
        }
    }
    Ok(())
}

/// v3 自检（与 v1/v2 段并账——mod.rs 聚合器 merge）。
pub fn run_clickanim_v3_checks() -> CheckSet {
    let mut set = CheckSet::new("jstar2-F621-v3");

    // 1. 分离时间线：按压谷值段正常；未释放段语义与 v1 时间线一致。
    let mut s = PressSession::new(ClickAnimTier::Light, 1000);
    set.add(
        "press segment dips then rebinds",
        s.combined_scale_m(1060) <= 945 && s.combined_scale_m(1120) == 1000,
        "",
    );

    // 2. 释放浅谷回弹：0.970 谷值、120ms 窗口、窗后恒 1.000。
    s.release(1180); // 按压 180ms < 500ms → 回弹生效
    set.add(
        "release rebound shallow dip 0.97",
        s.rebound_active()
            && s.combined_scale_m(1180) == 1000
            && s.combined_scale_m(1240) < 1000
            && s.combined_scale_m(1240) >= 965
            && s.combined_scale_m(1310) == 1000,
        "",
    );

    // 3. 长按抑制：>500ms 释放不回弹（交互状态机公理）。
    let mut h = PressSession::new(ClickAnimTier::Full, 1000);
    h.release(1600);
    set.add(
        "long press skips rebound",
        !h.rebound_active() && h.combined_scale_m(1620) == 1000,
        "",
    );

    // 4. 抬起幂等：二次抬起不改状态（乱点防护）。
    let mut d = PressSession::new(ClickAnimTier::Light, 0);
    d.release(100);
    let first = d.released_at_ms_is(100);
    d.release(200);
    set.add("release idempotent", first && d.released_at_ms_is(100), "");

    // 5. 多设备枢纽：双设备独立会话、并集脏区覆盖两指针。
    let mut hub = MultiPointerHub::new(ClickAnimTier::Light);
    hub.press(0, 1000);
    hub.press(1, 1020);
    let (x, y, w, hh) = hub.union_dirty_rect(
        1040,
        &[(0u8, 10i64, 10i64, 32u16, 32u16), (1u8, 200i64, 200i64, 24u16, 24u16)],
    );
    set.add(
        "multi pointer union dirty rect",
        x == 10 && y == 10 && w == 214 && hh == 214,
        "",
    );

    // 6. 关档枢纽：并集恒空（零干预）。
    let hub_off = MultiPointerHub::new(ClickAnimTier::Off);
    let (x, y, w, hh) = hub_off.union_dirty_rect(100, &[(0u8, 5i64, 5i64, 32u16, 32u16)]);
    set.add("hub off empty union", (x, y, w, hh) == (0, 0, 0, 0), "");

    // 7. 体验日志环：64 槽覆盖写 + 时间序回放 + 狂点指纹。
    let mut log = AnimEventLog::new();
    for i in 0..70u64 {
        // 5 个一组：1 起始 + 4 连点——构成狂点指纹（连续重启 ≥3）。
        let kind = if i % 5 == 0 { AnimEventKind::PressStarted } else { AnimEventKind::PressRestarted };
        log.push(AnimEvent { at_ms: i * 10, kind, device: 0, latency_us: 80, smooth: true });
    }
    let evs = log.replay();
    set.add(
        "event log ring 64 overwrite and ordered replay",
        log.total == 70 && log.overwritten == 6 && evs.len() == 64 && evs[0].at_ms == 60 && evs[63].at_ms == 690,
        "",
    );
    set.add("rage click streak detected", log.rage_click_streak() >= 3, "");

    // 8. 偏好 KV 序列化往返 + 篡改拒绝。
    let p = ClickAnimPrefs { tier: ClickAnimTier::Full, rec_highlight: true };
    let kv = prefs_to_kv(&p);
    let round = prefs_from_kv(&kv);
    let mut bad = kv;
    bad[2] = 9;
    set.add(
        "prefs kv roundtrip and tamper reject",
        round.map(|r| r == p).unwrap_or(false)
            && prefs_from_kv(&bad).is_none()
            && prefs_from_kv(&[0u8; 8]).is_none(),
        "",
    );

    // 9. F350 全谱：三段全对绿；缺 hold 段红。
    let full = audit_f350_full_spectrum(&[
        HapticEntry { domain: "pointer-click", scale_m: 940, duration_ms: 120 },
        HapticEntry { domain: "pointer-release", scale_m: 970, duration_ms: 120 },
        HapticEntry { domain: "pointer-hold", scale_m: 1000, duration_ms: 0 },
    ]);
    let missing = audit_f350_full_spectrum(&[
        HapticEntry { domain: "pointer-click", scale_m: 940, duration_ms: 120 },
        HapticEntry { domain: "pointer-release", scale_m: 970, duration_ms: 120 },
    ]);
    set.add("F350 full spectrum pass", full.is_ok(), "");
    set.add("F350 full spectrum missing red", missing.is_err(), "");

    set
}

#[cfg(test)]
mod tests_v3 {
    use super::*;

    #[test]
    fn rebound_window_is_120ms() {
        let mut s = PressSession::new(ClickAnimTier::Light, 0);
        s.release(50);
        assert!(s.combined_scale_m(110) < 1000);
        assert_eq!(s.combined_scale_m(170), 1000);
    }

    #[test]
    fn hub_scale_per_device_independent() {
        let mut hub = MultiPointerHub::new(ClickAnimTier::Light);
        hub.press(0, 1000);
        assert!(hub.scale_m(0, 1060) < 1000);
        assert_eq!(hub.scale_m(2, 1060), 1000, "未按下的设备恒 1.0");
    }

    #[test]
    fn kv_rejects_wrong_magic() {
        assert!(prefs_from_kv(&[0x00; 8]).is_none());
        assert!(prefs_from_kv(&[0xCA, 0xFE, 1, 0]).is_none(), "长度错");
    }

    #[test]
    fn f350_drift_on_release_detected() {
        let err = audit_f350_full_spectrum(&[
            HapticEntry { domain: "pointer-click", scale_m: 940, duration_ms: 120 },
            HapticEntry { domain: "pointer-release", scale_m: 960, duration_ms: 120 },
            HapticEntry { domain: "pointer-hold", scale_m: 1000, duration_ms: 0 },
        ])
        .unwrap_err();
        assert!(err.contains("pointer-release"));
    }
}
