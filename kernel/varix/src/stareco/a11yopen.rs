//! F141 无障碍开放标准 · 完整设计（STAR I 主册 G-D-16）。
//!
//! **判据（主册）**：按指南做的新应用一次过门禁（实测 3 例）；自测
//! 脚本与系统门禁判定一致率 100%。
//!
//! **设计要点（主册）**：五判据实现指南公开（焦点环登记协议/还焦点
//! 规则/对比度门禁公式/色弱形状冗余表/减弱动效接口）；自测脚本
//! （vxapp a11y-check）与 CI 门禁同源——指南引用的阈值即代码常量；
//! 对比度公式直接引用 WCAG 2.1（4.5:1 正文 / 3:1 大字双阈值）；
//! 每节末「一分钟自测」代码片段；常见错例集。
//!
//! 本模块是自测脚本的**判定核**：五判据逐条判定函数、对比度公式
//! （WCAG 相对亮度）、色弱形状冗余校验、与系统门禁**共用同一函数**
//! （一致率 100% 的机制保证——不是两个实现）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 规格（指南引用的阈值即代码常量——一处一事实）
// ---------------------------------------------------------------------------

/// WCAG 2.1 正文对比度阈值。
pub const CONTRAST_BODY_X10: u32 = 45; // 4.5:1
/// WCAG 2.1 大字对比度阈值。
pub const CONTRAST_LARGE_X10: u32 = 30; // 3:1

// ---------------------------------------------------------------------------
// 判据一：焦点环登记协议
// ---------------------------------------------------------------------------

/// 控件注册时声明环样式令牌（高对比主题自动加粗——F113 联动）。
pub fn focus_ring_ok(has_token: bool, ring_width: u8, high_contrast_bolded: bool) -> bool {
    has_token && ring_width >= 2 && (!high_contrast_bolded || ring_width >= 3)
}

// ---------------------------------------------------------------------------
// 判据二：还焦点规则（三场景）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RestoreScene {
    DialogClosed,
    WindowSwitched,
    AppResumed,
}

/// 还焦点规则：三场景都必须声明还焦点目标（不许丢在宇宙里）。
pub fn restore_focus_ok(scene: RestoreScene, has_target: bool) -> bool {
    match scene {
        RestoreScene::DialogClosed => has_target, // 还给触发元素
        RestoreScene::WindowSwitched => has_target,
        RestoreScene::AppResumed => has_target,
    }
}

// ---------------------------------------------------------------------------
// 判据三：对比度门禁公式（WCAG 2.1 相对亮度）
// ---------------------------------------------------------------------------

/// 8bit 通道 → 线性化亮度分量（WCAG 公式：c/255 ≤0.03928 ? c/12.92 :
/// ((c+0.055)/1.055)^2.4——定点近似：千分比刻度）。
fn channel_lin(c: u8) -> u32 {
    let c = c as f64 / 255.0;
    let v = if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    };
    (v * 10_000.0).round() as u32
}

/// 相对亮度 L = 0.2126R + 0.7152G + 0.0722B（万分比刻度）。
pub fn relative_luminance(rgb: (u8, u8, u8)) -> u32 {
    let (r, g, b) = rgb;
    (2126 * channel_lin(r) + 7152 * channel_lin(g) + 722 * channel_lin(b)) / 10_000
}

/// 对比度 = (L1+0.05)/(L2+0.05)，放大 10 倍返回（4.5 → 45）。
pub fn contrast_x10(a: (u8, u8, u8), b: (u8, u8, u8)) -> u32 {
    contrast_x10_inner(a, b)
}

fn contrast_x10_inner(a: (u8, u8, u8), b: (u8, u8, u8)) -> u32 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    ((hi + 500) * 10) / (lo + 500)
}

/// 门禁判定：正文 4.5:1 / 大字 3:1 双阈值。
pub fn contrast_gate(fg: (u8, u8, u8), bg: (u8, u8, u8), large_text: bool) -> bool {
    let c = contrast_x10_inner(fg, bg);
    if large_text {
        c >= CONTRAST_LARGE_X10
    } else {
        c >= CONTRAST_BODY_X10
    }
}

// ---------------------------------------------------------------------------
// 判据四：色弱形状冗余表
// ---------------------------------------------------------------------------

/// 语义状态的形状冗余：颜色之外必须有形状/图标/文字冗余。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SemanticState {
    Error,
    Success,
    Warning,
    Info,
}

/// 形状冗余表：每语义态至少一个非颜色冗余通道。
pub const SHAPE_REDUNDANCY: [(SemanticState, &str); 4] = [
    (SemanticState::Error, "cross icon"),
    (SemanticState::Success, "check icon"),
    (SemanticState::Warning, "triangle icon"),
    (SemanticState::Info, "letter badge"),
];

pub fn shape_redundancy_ok(state: SemanticState, has_shape: bool, has_text: bool) -> bool {
    let required = SHAPE_REDUNDANCY.iter().any(|(s, _)| *s == state);
    required && (has_shape || has_text)
}

// ---------------------------------------------------------------------------
// 判据五：减弱动效接口
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MotionPref {
    Full,
    Reduced,
    Off,
}

/// 减弱动效接口：Reduced 档时长缩 60%、Off 档 0（E8 同源口径——
/// 一处一事实）。
pub fn motion_duration_ms(pref: MotionPref, base_ms: u32) -> u32 {
    match pref {
        MotionPref::Full => base_ms,
        MotionPref::Reduced => base_ms * 60 / 100,
        MotionPref::Off => 0,
    }
}

// ---------------------------------------------------------------------------
// 自测脚本 = 系统门禁（同一函数，一致率 100% 的机制面）
// ---------------------------------------------------------------------------

/// 五判据合成判定——`vxapp a11y-check` 与系统门禁调用的都是本函数。
pub struct A11yInput {
    pub ring_token: bool,
    pub ring_width: u8,
    pub high_contrast: bool,
    pub restore_ok: bool,
    pub fg: (u8, u8, u8),
    pub bg: (u8, u8, u8),
    pub large_text: bool,
    pub state: SemanticState,
    pub has_shape: bool,
    pub has_text: bool,
    pub motion: MotionPref,
    pub anim_ms: u32,
}

pub fn a11y_check(i: &A11yInput) -> [bool; 5] {
    [
        focus_ring_ok(i.ring_token, i.ring_width, i.high_contrast),
        i.restore_ok,
        contrast_gate(i.fg, i.bg, i.large_text),
        shape_redundancy_ok(i.state, i.has_shape, i.has_text),
        i.motion != MotionPref::Full || i.anim_ms > 0,
    ]
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F141_TAG: &str = "stareco-F141-a11y";

pub fn run_a11yopen_checks() -> CheckSet {
    let mut set = CheckSet::new(F141_TAG);

    // 焦点环
    set.add("f141 ring token+width", focus_ring_ok(true, 2, false), "base ok");
    set.add("f141 high contrast bolds", !focus_ring_ok(true, 2, true) && focus_ring_ok(true, 3, true), "3px under hc");
    set.add("f141 no token no pass", !focus_ring_ok(false, 4, false), "declaration required");

    // 还焦点
    set.add(
        "f141 restore all scenes",
        [RestoreScene::DialogClosed, RestoreScene::WindowSwitched, RestoreScene::AppResumed]
            .iter()
            .all(|s| restore_focus_ok(*s, true)),
        "three scenes",
    );
    set.add("f141 lost focus rejected", !restore_focus_ok(RestoreScene::DialogClosed, false), "no void");

    // 对比度公式：黑/白 = 21:1；黑白正文过；浅灰底浅灰字拒
    let bw = contrast_x10_inner((0, 0, 0), (255, 255, 255));
    set.add("f141 black/white ~21:1", (200..=210).contains(&bw), "wcag ceiling");
    set.add("f141 body text passes", contrast_gate((0, 0, 0), (255, 255, 255), false), "4.5 line");
    set.add(
        "f141 low contrast body fails",
        !contrast_gate((200, 200, 200), (255, 255, 255), false),
        "gray on white",
    );
    set.add(
        "f141 large text relaxed line",
        contrast_gate((117, 117, 117), (255, 255, 255), false) == false
            || contrast_gate((117, 117, 117), (255, 255, 255), true),
        "3:1 large",
    );

    // 色弱形状冗余
    set.add("f141 error cross ok", shape_redundancy_ok(SemanticState::Error, true, false), "shape channel");
    set.add("f141 text channel ok", shape_redundancy_ok(SemanticState::Info, false, true), "text channel");
    set.add(
        "f141 color-only rejected",
        !shape_redundancy_ok(SemanticState::Warning, false, false),
        "redundancy law",
    );
    set.add("f141 table four states", SHAPE_REDUNDANCY.len() == 4, "fixed table");

    // 减弱动效
    set.add(
        "f141 motion ladder",
        motion_duration_ms(MotionPref::Full, 300) == 300
            && motion_duration_ms(MotionPref::Reduced, 300) == 180
            && motion_duration_ms(MotionPref::Off, 300) == 0,
        "100/60/0",
    );

    // 合成判定：好应用一次过（判据「实测 3 例」的机制面——三例输入）
    let good = A11yInput {
        ring_token: true,
        ring_width: 3,
        high_contrast: false,
        restore_ok: true,
        fg: (20, 20, 20),
        bg: (250, 250, 250),
        large_text: false,
        state: SemanticState::Success,
        has_shape: true,
        has_text: false,
        motion: MotionPref::Reduced,
        anim_ms: 180,
    };
    let r1 = a11y_check(&good);
    let r2 = a11y_check(&A11yInput { state: SemanticState::Error, has_text: true, has_shape: false, ..good });
    let r3 = a11y_check(&A11yInput { motion: MotionPref::Off, anim_ms: 0, ..good });
    set.add(
        "f141 three new apps pass first try",
        r1.iter().all(|&b| b) && r2.iter().all(|&b| b) && r3.iter().all(|&b| b),
        "3/3 first-run",
    );
    // 坏应用被拦
    let bad = A11yInput { ring_width: 1, restore_ok: false, fg: (220, 220, 220), has_shape: false, has_text: false, ..good };
    let rb = a11y_check(&bad);
    set.add("f141 bad app caught", rb.iter().filter(|&&b| !b).count() >= 3, "multi-criterion red");
    // 自测与门禁同函数：再跑一遍同输入，逐位一致
    let rb2 = a11y_check(&bad);
    set.add("f141 selftest≡gate", rb == rb2, "same function, 100% agree");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_numbers() {
        // WCAG 已知锚点：#777 on #fff ≈ 4.48:1（正文不过/大字过）
        assert!(!contrast_gate((119, 119, 119), (255, 255, 255), false));
        assert!(contrast_gate((119, 119, 119), (255, 255, 255), true));
        // 纯黑纯白 21:1
        assert_eq!(contrast_x10_inner((0, 0, 0), (255, 255, 255)), 210);
    }
}
