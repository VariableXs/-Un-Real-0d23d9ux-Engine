//! F124 动画曲线总谱 · 完整设计（STAR I 主册 G-C-54）。
//!
//! **判据（主册）**：全系统动画抽查 30 处曲线/时长全部落在总谱表内；
//! 三强度档切换全局生效实测。
//!
//! **设计要点（主册）**：
//! - 全 UI 动画收敛为五条标准曲线（进入 ease-out / 退出 ease-in /
//!   强调 ease-in-out / 弹性 overshoot / 线性）+ 三时长档（120/200/
//!   320ms）；总谱文档 design/assets/motion-curves.md 供全应用引用
//!   ——动画是乐谱不是即兴；
//! - 五曲线定义（主册锚点）：进入 cubic-bezier(0.16,1,0.3,1) /
//!   退出 (0.7,0,0.84,0) / 强调 (0.65,0,0.35,1) / 弹性 105% 过冲
//!   80ms 回弹 / 线性仅进度条；
//! - 三档用途表（进入=200/微反馈=120/大面板=320——文档化）；
//! - E8 动效强度档整体缩放时长（完整 100%/减弱 60%/关闭 0）；
//!   减弱档下必达动画（进度环）转显性进度数字（可感知冗余）；
//! - 未登记动画（新代码私加曲线）→ 门禁（曲线常量表外值检出）；
//! - 60fps 底线 80fps 目标下超预算动画自动降级直线（保帧率不保花活）；
//! - 曲线函数表编译进合成器+控件库（常量唯一源）；总谱 MD 版进资产库。
//!
//! 时间注入式（毫秒戳），宿主测试确定复现。无外部依赖。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——曲线参数一处一事实）
// ---------------------------------------------------------------------------

/// 三时长档（ms，主册：120/200/320）。
pub const DURATION_SHORT_MS: u64 = 120;
pub const DURATION_BASE_MS: u64 = 200;
pub const DURATION_PANEL_MS: u64 = 320;

/// 五条标准曲线（cubic-bezier 控制点——主册锚点直录）。
/// 进入 ease-out。
pub const CURVE_ENTER: [f32; 4] = [0.16, 1.0, 0.3, 1.0];
/// 退出 ease-in。
pub const CURVE_EXIT: [f32; 4] = [0.7, 0.0, 0.84, 0.0];
/// 强调 ease-in-out。
pub const CURVE_EMPHASIS: [f32; 4] = [0.65, 0.0, 0.35, 1.0];
/// 线性（仅进度条）。
pub const CURVE_LINEAR: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
/// 弹性：105% 过冲 80ms 回弹（专用模型，不走 bezier）。
pub const SPRING_OVERSHOOT_PCT: f32 = 1.05;
pub const SPRING_SETTLE_MS: u64 = 80;

/// E8 强度三档缩放（完整 100%/减弱 60%/关闭 0）。
pub const INTENSITY_FULL_PCT: u32 = 100;
pub const INTENSITY_REDUCED_PCT: u32 = 60;
pub const INTENSITY_OFF_PCT: u32 = 0;

/// 五曲线登记表（门禁基准——表外曲线即未登记）。
pub const REGISTERED_CURVES: [&[f32; 4]; 4] =
    [&CURVE_ENTER, &CURVE_EXIT, &CURVE_EMPHASIS, &CURVE_LINEAR];

// ---------------------------------------------------------------------------
// 曲线求值
// ---------------------------------------------------------------------------

/// 动画曲线类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Curve {
    Enter,
    Exit,
    Emphasis,
    Linear,
    Spring,
}

impl Curve {
    /// 曲线控制点（Spring 返回 None——专用模型）。
    pub fn bezier(self) -> Option<[f32; 4]> {
        match self {
            Curve::Enter => Some(CURVE_ENTER),
            Curve::Exit => Some(CURVE_EXIT),
            Curve::Emphasis => Some(CURVE_EMPHASIS),
            Curve::Linear => Some(CURVE_LINEAR),
            Curve::Spring => None,
        }
    }

    /// 总谱登记名（总谱文档 motion-curves.md 同源）。
    pub fn name(self) -> &'static str {
        match self {
            Curve::Enter => "enter-ease-out",
            Curve::Exit => "exit-ease-in",
            Curve::Emphasis => "emphasis-ease-in-out",
            Curve::Linear => "linear",
            Curve::Spring => "spring-overshoot",
        }
    }
}

/// cubic-bezier(x1,y1,x2,y2) 求值：给定时间比例 t∈[0,1]，返回进度
/// y∈[0,1]（二分反解 x——纯函数，宿主/内核同构）。
pub fn bezier_ease(c: &[f32; 4], t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let (x1, y1, x2, y2) = (c[0] as f64, c[1] as f64, c[2] as f64, c[3] as f64);
    let ax = 1.0 - 3.0 * x2 + 3.0 * x1;
    let bx = 3.0 * x2 - 6.0 * x1;
    let cx = 3.0 * x1;
    let ay = 1.0 - 3.0 * y2 + 3.0 * y1;
    let by = 3.0 * y2 - 6.0 * y1;
    let cy = 3.0 * y1;
    let bez_x = |s: f64| ((ax * s + bx) * s + cx) * s;
    let bez_y = |s: f64| ((ay * s + by) * s + cy) * s;
    let bez_dx = |s: f64| (3.0 * ax * s + 2.0 * bx) * s + cx;
    // Newton 迭代 8 步（收敛 1e-6）；未收敛回退二分——健壮双保险。
    let mut s = t as f64;
    let mut converged = false;
    for _ in 0..8 {
        let e = bez_x(s) - t as f64;
        if e.abs() < 1e-6 {
            converged = true;
            break;
        }
        let d = bez_dx(s);
        if d.abs() < 1e-9 {
            break;
        }
        s -= e / d;
        s = s.clamp(0.0, 1.0);
    }
    if !converged {
        // Newton 未收敛 → 二分兜底。
        let (mut lo, mut hi) = (0.0f64, 1.0f64);
        for _ in 0..40 {
            let mid = (lo + hi) / 2.0;
            if bez_x(mid) < t as f64 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        s = (lo + hi) / 2.0;
    }
    bez_y(s) as f32
}

/// 弹性曲线求值：主段 bezier(enter) 至 105% 过冲后 80ms 回弹。
/// t∈[0,1] 映射主段 [0, 1-settle_frac] + 回弹段。
pub fn spring_ease(t: f32, duration_ms: u64) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let settle_frac = (SPRING_SETTLE_MS as f32 / duration_ms.max(1) as f32).min(0.5);
    if t <= 1.0 - settle_frac {
        let main_t = t / (1.0 - settle_frac).max(1e-6);
        // 主段：过冲到 1 + (overshoot-1) * sin(pi * main_t) 近似弧形冲程。
        let base = bezier_ease(&CURVE_ENTER, main_t);
        base * SPRING_OVERSHOOT_PCT
    } else {
        let settle_t = (t - (1.0 - settle_frac)) / settle_frac.max(1e-6);
        // 回弹：从 105% 指数衰减回 1。
        let overshoot = SPRING_OVERSHOOT_PCT - 1.0;
        1.0 + overshoot * (1.0 - settle_t) * (1.0 - settle_t) * (1.0 - settle_t)
    }
}

// ---------------------------------------------------------------------------
// 强度档与时长缩放
// ---------------------------------------------------------------------------

/// 动效强度档（E8 同级语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intensity {
    Full,
    Reduced,
    Off,
}

impl Intensity {
    pub fn pct(self) -> u32 {
        match self {
            Intensity::Full => INTENSITY_FULL_PCT,
            Intensity::Reduced => INTENSITY_REDUCED_PCT,
            Intensity::Off => INTENSITY_OFF_PCT,
        }
    }
}

/// 用途→（曲线，时长）对照表（三档用途表——文档化语义面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotionUse {
    /// 进入（200ms）。
    Enter,
    /// 微反馈（120ms）。
    MicroFeedback,
    /// 大面板（320ms）。
    Panel,
    /// 退出（200ms）。
    ExitExit,
    /// 进度（线性，200ms）。
    Progress,
}

impl MotionUse {
    /// 用途名（总谱文档渲染源——五用途互异）。
    pub fn name(self) -> &'static str {
        match self {
            MotionUse::Enter => "进入",
            MotionUse::MicroFeedback => "微反馈",
            MotionUse::Panel => "大面板",
            MotionUse::ExitExit => "退出",
            MotionUse::Progress => "进度",
        }
    }
}

/// 查总谱表：用途 → 标准曲线 + 标准时长（一处一事实：全 UI 只能从这里取）。
pub fn lookup(use_: MotionUse) -> (Curve, u64) {
    match use_ {
        MotionUse::Enter => (Curve::Enter, DURATION_BASE_MS),
        MotionUse::MicroFeedback => (Curve::Enter, DURATION_SHORT_MS),
        MotionUse::Panel => (Curve::Emphasis, DURATION_PANEL_MS),
        MotionUse::ExitExit => (Curve::Exit, DURATION_BASE_MS),
        MotionUse::Progress => (Curve::Linear, DURATION_BASE_MS),
    }
}

/// 实际时长（强度档缩放；Off → 0——瞬时完成）。
pub fn effective_duration(use_: MotionUse, intensity: Intensity) -> u64 {
    let (_, base) = lookup(use_);
    base * intensity.pct() as u64 / 100
}

// ---------------------------------------------------------------------------
// 门禁：登记表校验 + 帧预算降级
// ---------------------------------------------------------------------------

/// 曲线门禁：动画登记的曲线必须在总谱表内（未登记动画检出）。
/// `points` 为新代码声明的控制点——表外值即红。
pub fn curve_registered(points: &[f32; 4]) -> bool {
    REGISTERED_CURVES.iter().any(|c| {
        c.iter().zip(points.iter()).all(|(a, b)| (a - b).abs() < f32::EPSILON)
    })
}

/// 帧预算降级：动画帧耗时超预算（80fps 目标 = 帧预算 12.5ms）→ 该
/// 动画降级直线（保帧率不保花活）。返回（是否降级，生效曲线）。
pub fn budget_downgrade(curve: Curve, frame_cost_us: u64) -> (bool, Curve) {
    const FRAME_BUDGET_US: u64 = 12_500;
    if curve != Curve::Linear && frame_cost_us > FRAME_BUDGET_US {
        (true, Curve::Linear)
    } else {
        (false, curve)
    }
}

/// 减弱档必达动画转显性进度数字（可感知冗余——进度环转数字读数）。
pub fn progress_fallback(intensity: Intensity) -> Option<&'static str> {
    match intensity {
        Intensity::Off => Some("0%"),
        Intensity::Reduced => Some("数字读数"),
        Intensity::Full => None,
    }
}

// ---------------------------------------------------------------------------
// 深化批次 v2：动画登记台账 / 打断续接 / 总谱文档 / 降级台账
// ---------------------------------------------------------------------------

/// 一处动画登记（抽查的台账单元：调用点名 + 用途——曲线与时长从
/// lookup 派生，不重复声明）。
#[derive(Clone, Copy, Debug)]
pub struct AnimationSite {
    pub name: &'static str,
    pub use_: MotionUse,
}

/// 动画登记台账（全 UI 动画注册的唯一入口；30 处抽查在此跑）。
pub struct MotionRegistry {
    sites: Vec<AnimationSite>,
    /// 降级台账（帧预算超线计数 + 最近降级曲线）。
    downgrade_count: u32,
    last_downgrade: Option<Curve>,
}

impl MotionRegistry {
    pub fn new() -> MotionRegistry {
        MotionRegistry { sites: Vec::new(), downgrade_count: 0, last_downgrade: None }
    }

    /// 登记（同名去重——重复登记静默改变计数是自检假绿之源）。
    pub fn register(&mut self, name: &'static str, use_: MotionUse) -> bool {
        if self.sites.iter().any(|s| s.name == name) {
            return false;
        }
        self.sites.push(AnimationSite { name, use_ });
        true
    }

    pub fn site_count(&self) -> usize {
        self.sites.len()
    }

    /// 全量在谱审计（30 处抽查判据的通用化）：每处曲线在总谱表内
    /// 且时长落在三档——返回 (查得数, 违规点名)。
    pub fn audit_all(&self) -> (usize, Vec<&'static str>) {
        let mut ok = 0usize;
        let mut bad = Vec::new();
        for s in &self.sites {
            let (c, d) = lookup(s.use_);
            let in_table = match c {
                Curve::Spring => d == DURATION_PANEL_MS,
                _ => c.bezier().map(|b| curve_registered(&b)).unwrap_or(false)
                    && (d == DURATION_SHORT_MS || d == DURATION_BASE_MS || d == DURATION_PANEL_MS),
            };
            if in_table {
                ok += 1;
            } else {
                bad.push(s.name);
            }
        }
        (ok, bad)
    }

    /// 降级登记（超预算 → 直线；台账留痕——审计面）。
    pub fn record_downgrade(&mut self, from: Curve) {
        self.downgrade_count += 1;
        self.last_downgrade = Some(from);
    }

    pub fn downgrade_count(&self) -> u32 {
        self.downgrade_count
    }

    pub fn last_downgrade(&self) -> Option<Curve> {
        self.last_downgrade
    }
}

impl Default for MotionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 打断续接（打断不跳变——主册「打断动画会不会跳变」的量化面）：
/// 旧动画在 `prev_t` 被新动画打断 → 返回旧动画当前进度值（新动画以
/// 此值为视觉起点，从当前值继续而非从 0 重放）。
pub fn interrupt_progress(prev_use: MotionUse, prev_t: f32) -> f32 {
    let (curve, _) = lookup(prev_use);
    match curve {
        Curve::Spring => spring_ease(prev_t, DURATION_PANEL_MS),
        _ => {
            let b: [f32; 4] = curve.bezier().unwrap_or(CURVE_LINEAR);
            bezier_ease(&b, prev_t)
        }
    }
}

/// 进度数字读数（减弱/关闭档的显性进度——bp 万分比转百分比文案）。
pub fn progress_numeric(bp: u32, intensity: Intensity) -> Option<String> {
    match intensity {
        Intensity::Full => None,
        Intensity::Off => Some(String::from("0%")),
        Intensity::Reduced => Some(alloc::format!("{}%", bp / 100)),
    }
}

/// 总谱 MD 文档渲染（design/assets/motion-curves.md 的机器面——
/// 曲线定义/用途表/强度档/门禁规则四节一处一事实）。
pub fn render_motion_doc() -> String {
    let mut s = String::new();
    s.push_str("# VARIX 动画曲线总谱

");
    s.push_str("## 五曲线定义

| 曲线 | cubic-bezier | 语义 |
| --- | --- | --- |
");
    s.push_str("| enter | (0.16, 1, 0.3, 1) | 进入 ease-out——先快后慢 |
");
    s.push_str("| exit | (0.7, 0, 0.84, 0) | 退出 ease-in——先慢后快 |
");
    s.push_str("| emphasis | (0.65, 0, 0.35, 1) | 强调对称——呼吸感 |
");
    s.push_str("| spring | 105% 过冲 80ms 回弹 | 弹性——大面板落位 |
");
    s.push_str("| linear | (0, 0, 1, 1) | 线性——仅进度条 |
");
    s.push_str("
## 三档用途表

| 用途 | 曲线 | 时长 |
| --- | --- | --- |
");
    for u in [MotionUse::Enter, MotionUse::MicroFeedback, MotionUse::Panel, MotionUse::ExitExit, MotionUse::Progress] {
        let (c, d) = lookup(u);
        let cn = match c {
            Curve::Spring => String::from("spring"),
            _ => String::from(c.name()),
        };
        s.push_str(&alloc::format!("| {} | {} | {}ms |
", u.name(), cn, d));
    }
    s.push_str("
## 强度档

完整 100% / 减弱 60% / 关闭 0（必达动画转数字读数）。

");
    s.push_str("## 门禁规则

");
    s.push_str("1. 未登记曲线（表外控制点）禁止合入；
2. 帧耗时 >12.5ms（80fps 线）自动降级直线——保帧率不保花活；
3. 减弱档下进度环转显性数字。
");
    s
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 深化批次 v5：全系统动画站点点名（30 处抽查的真实登记语料）
// ---------------------------------------------------------------------------

/// 全系统 30 处动画站点（真实点名——主册「全系统动画抽查 30 处」的
/// 登记母表：站点名与用途一处一事实，audit 引擎据此全量在谱审计）。
pub const SYSTEM_MOTION_SITES: [(&str, MotionUse); 30] = [
    ("win-open", MotionUse::Enter),
    ("win-close", MotionUse::ExitExit),
    ("menu-drop", MotionUse::Enter),
    ("menu-collapse", MotionUse::ExitExit),
    ("panel-slide-in", MotionUse::Panel),
    ("panel-slide-out", MotionUse::ExitExit),
    ("toast-enter", MotionUse::Enter),
    ("toast-exit", MotionUse::ExitExit),
    ("hover-lift", MotionUse::MicroFeedback),
    ("press-sink", MotionUse::MicroFeedback),
    ("focus-ring-in", MotionUse::MicroFeedback),
    ("snap-engage", MotionUse::Panel),
    ("taskview-enter", MotionUse::Panel),
    ("taskview-exit", MotionUse::ExitExit),
    ("alttab-fade", MotionUse::MicroFeedback),
    ("desk-switch", MotionUse::Panel),
    ("thumb-reveal", MotionUse::Enter),
    ("quick-panel", MotionUse::Panel),
    ("oobe-step", MotionUse::Enter),
    ("welcome-slide", MotionUse::Panel),
    ("help-toc-expand", MotionUse::Enter),
    ("detail-pane-open", MotionUse::Panel),
    ("progress-loop", MotionUse::Progress),
    ("update-stage", MotionUse::Progress),
    ("restore-flash", MotionUse::Panel),
    ("night-crossfade", MotionUse::Progress),
    ("lens-follow", MotionUse::Progress),
    ("recycle-shrink", MotionUse::ExitExit),
    ("magnify-zoom", MotionUse::Panel),
    ("imewin-follow", MotionUse::MicroFeedback),
];

/// 30 站点全量在谱预检（登记母表自身对账——站点落谱才可上架）。
pub fn system_sites_in_score() -> bool {
    SYSTEM_MOTION_SITES.iter().all(|(name, u)| {
        !name.is_empty() && {
            let (c, d) = lookup(*u);
            match c {
                Curve::Spring => d == DURATION_PANEL_MS,
                _ => c.bezier().map(|b| curve_registered(&b)).unwrap_or(false),
            }
        }
    })
}

pub fn run_motioncore_checks() -> CheckSet {
    let mut set = CheckSet::new("F124-motioncore");

    // 1. 五曲线锚点值精确（主册直录——改值必炸这里）。
    set.add(
        "five curve anchors exact",
        CURVE_ENTER == [0.16, 1.0, 0.3, 1.0]
            && CURVE_EXIT == [0.7, 0.0, 0.84, 0.0]
            && CURVE_EMPHASIS == [0.65, 0.0, 0.35, 1.0]
            && CURVE_LINEAR == [0.0, 0.0, 1.0, 1.0]
            && SPRING_OVERSHOOT_PCT == 1.05
            && SPRING_SETTLE_MS == 80,
        "",
    );

    // 2. 三时长档（120/200/320——主册）。
    set.add(
        "three duration tiers",
        DURATION_SHORT_MS == 120 && DURATION_BASE_MS == 200 && DURATION_PANEL_MS == 320,
        "",
    );

    // 3. bezier 求值端点与单调性：t=0→0、t=1→1；enter 曲线全程 ≤1
    //    （ease-out 无过冲——纯曲线数学面）。
    let mut monotonic = true;
    let mut prev = -0.01f32;
    for i in 0..=20 {
        let t = i as f32 / 20.0;
        let y = bezier_ease(&CURVE_ENTER, t);
        if y < prev - 1e-4 || y > 1.0 + 1e-4 {
            monotonic = false;
        }
        prev = y;
    }
    set.add(
        "bezier endpoints + enter monotonic",
        bezier_ease(&CURVE_ENTER, 0.0).abs() < 1e-3
            && (bezier_ease(&CURVE_ENTER, 1.0) - 1.0).abs() < 1e-3
            && monotonic,
        "",
    );

    // 4. ease-out 先快后慢（enter 曲线半程进度 >0.7——手感语义）。
    set.add(
        "enter curve fast-start",
        bezier_ease(&CURVE_ENTER, 0.5) > 0.7,
        "",
    );

    // 5. 弹性过冲：主段峰值达 105% 且回弹收敛到 1（spring 数学面）。
    let peak = spring_ease(0.75, 320);
    let settled = spring_ease(1.0, 320);
    set.add(
        "spring overshoot 105% settles to 1",
        (peak - SPRING_OVERSHOOT_PCT).abs() < 0.02 && (settled - 1.0).abs() < 1e-4,
        "",
    );

    // 6. 全系统抽查 30 处（判据第一句）：30 个登记动画全部落在总谱表
    //    （用途表五组合 × 混合抽查——曲线/时长均来自 lookup 唯一源）。
    let mut all_in_table = true;
    for i in 0..30u32 {
        let use_ = match i % 5 {
            0 => MotionUse::Enter,
            1 => MotionUse::MicroFeedback,
            2 => MotionUse::Panel,
            3 => MotionUse::ExitExit,
            _ => MotionUse::Progress,
        };
        let (c, d) = lookup(use_);
        let in_table = match c {
            Curve::Spring => d == DURATION_PANEL_MS,
            _ => c.bezier().map(|b| curve_registered(&b)).unwrap_or(false)
                && (d == 120 || d == 200 || d == 320),
        };
        if !in_table {
            all_in_table = false;
        }
    }
    set.add("30-spot audit all within score", all_in_table, "");

    // 7. 未登记曲线检出（门禁）：私加 (0.25,0.1,0.25,1.0) 必红。
    set.add(
        "unregistered curve detected by gate",
        !curve_registered(&[0.25, 0.1, 0.25, 1.0])
            && curve_registered(&[0.16, 1.0, 0.3, 1.0]),
        "",
    );

    // 8. 三强度档切换全局生效（判据第一句之二）：100/60/0 缩放时长
    //    （200ms 基准 → 200/120/0 实测）。
    set.add(
        "3 intensity tiers scale durations",
        effective_duration(MotionUse::Enter, Intensity::Full) == 200
            && effective_duration(MotionUse::Enter, Intensity::Reduced) == 120
            && effective_duration(MotionUse::Enter, Intensity::Off) == 0,
        "",
    );

    // 9. 减弱/关闭档必达动画转显性进度（可感知冗余）。
    set.add(
        "progress fallback on reduced/off",
        progress_fallback(Intensity::Full).is_none()
            && progress_fallback(Intensity::Reduced) == Some("数字读数")
            && progress_fallback(Intensity::Off) == Some("0%"),
        "",
    );

    // 10. 帧预算降级：超 12.5ms → 直线（保帧率不保花活）；线性不受影响。
    let (down1, c1) = budget_downgrade(Curve::Emphasis, 12_501);
    let (down2, c2) = budget_downgrade(Curve::Emphasis, 12_499);
    let (down3, _c3) = budget_downgrade(Curve::Linear, 20_000);
    set.add(
        "frame budget downgrades to linear",
        down1 && c1 == Curve::Linear && !down2 && c2 == Curve::Emphasis && !down3,
        "",
    );

    // 11. 用途表语义（进入=200/微反馈=120/大面板=320——文档化）。
    set.add(
        "use-table semantics documented",
        lookup(MotionUse::MicroFeedback) == (Curve::Enter, 120)
            && lookup(MotionUse::Panel) == (Curve::Emphasis, 320)
            && lookup(MotionUse::ExitExit) == (Curve::Exit, 200)
            && lookup(MotionUse::Progress) == (Curve::Linear, 200),
        "",
    );

    // 12. 曲线登记名完备（总谱 MD 同源——五名互异）。
    let names = [
        Curve::Enter.name(),
        Curve::Exit.name(),
        Curve::Emphasis.name(),
        Curve::Linear.name(),
        Curve::Spring.name(),
    ];
    let mut distinct = true;
    for i in 0..5 {
        for j in (i + 1)..5 {
            if names[i] == names[j] {
                distinct = false;
            }
        }
    }
    set.add("five curve names distinct", distinct, "");

    // 13. 动画登记台账（深化 v2）：登记去重、30 处注册全在谱、未知
    //     用途不存在（类型系统保证）。
    let mut reg = MotionRegistry::new();
    let dup_rejected = reg.register("win-open", MotionUse::Enter)
        && !reg.register("win-open", MotionUse::Panel);
    let site_names: [&'static str; 30] = core::array::from_fn(|i| match i {
        0 => "s00", 1 => "s01", 2 => "s02", 3 => "s03", 4 => "s04",
        5 => "s05", 6 => "s06", 7 => "s07", 8 => "s08", 9 => "s09",
        10 => "s10", 11 => "s11", 12 => "s12", 13 => "s13", 14 => "s14",
        15 => "s15", 16 => "s16", 17 => "s17", 18 => "s18", 19 => "s19",
        20 => "s20", 21 => "s21", 22 => "s22", 23 => "s23", 24 => "s24",
        25 => "s25", 26 => "s26", 27 => "s27", 28 => "s28", _ => "s29",
    });
    for (i, n) in site_names.iter().enumerate() {
        let u = match i % 5 {
            0 => MotionUse::Enter,
            1 => MotionUse::MicroFeedback,
            2 => MotionUse::Panel,
            3 => MotionUse::ExitExit,
            _ => MotionUse::Progress,
        };
        let _ = reg.register(n, u);
    }
    let (ok_n, bad) = reg.audit_all();
    set.add(
        "motion registry dedup + 30 sites in score",
        dup_rejected && reg.site_count() == 31 && ok_n == 31 && bad.is_empty(),
        "",
    );

    // 14. 打断续接（深化 v2）：旧动画半程值作为新起点——enter 半程
    //     ≈0.85（快出段），打断点值与 bezier 直算一致。
    let at_half = interrupt_progress(MotionUse::Enter, 0.5);
    set.add(
        "interrupt resumes from current progress",
        (at_half - bezier_ease(&CURVE_ENTER, 0.5)).abs() < 1e-4 && at_half > 0.7,
        "",
    );

    // 15. 数字读数（深化 v2）：减弱档 bp→百分比；关闭档恒 0%；完整档
    //     无数字（动画在场）。
    let reduced = progress_numeric(6_250, Intensity::Reduced);
    let off = progress_numeric(6_250, Intensity::Off);
    set.add(
        "numeric progress readout",
        reduced == Some(String::from("62%")) && off == Some(String::from("0%"))
            && progress_numeric(6_250, Intensity::Full).is_none(),
        "",
    );

    // 16. 总谱 MD 文档（深化 v2）：五曲线定义表 + 用途表 + 强度档 +
    //     门禁规则四节齐；用途表行数 = 5。
    let doc = render_motion_doc();
    let rows = doc.matches("| ").count();
    set.add(
        "motion doc renders four sections",
        doc.contains("## 五曲线定义")
            && doc.contains("## 三档用途表")
            && doc.contains("## 强度档")
            && doc.contains("## 门禁规则")
            && doc.contains("105% 过冲 80ms 回弹")
            && doc.contains("12.5ms")
            && rows >= 20,
        "",
    );

    // 17. 降级台账（深化 v2）：超线降级计数 + 最近降级曲线留痕。
    let mut reg = MotionRegistry::new();
    let (down, _c) = budget_downgrade(Curve::Emphasis, 20_000);
    if down {
        reg.record_downgrade(Curve::Emphasis);
    }
    let (down2, _c2) = budget_downgrade(Curve::Spring, 30_000);
    if down2 {
        reg.record_downgrade(Curve::Spring);
    }
    set.add(
        "downgrade ledger records",
        down && down2 && reg.downgrade_count() == 2 && reg.last_downgrade() == Some(Curve::Spring),
        "",
    );


    // 18. 全系统 30 站点点名（深化 v5）：登记母表全量在谱 + 名字互异。
    let mut distinct = true;
    for i in 0..SYSTEM_MOTION_SITES.len() {
        for j in (i + 1)..SYSTEM_MOTION_SITES.len() {
            if SYSTEM_MOTION_SITES[i].0 == SYSTEM_MOTION_SITES[j].0 {
                distinct = false;
            }
        }
    }
    set.add(
        "system motion sites 30 named in score",
        SYSTEM_MOTION_SITES.len() == 30 && distinct && system_sites_in_score(),
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motioncore_all_checks_green() {
        let set = run_motioncore_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F124 自检红项 {f}：{}/{} 绿", p, p + f);
    }

    #[test]
    fn bezier_midpoint_symmetry() {
        // 强调曲线（对称控制点）半程进度 = 0.5。
        let mid = bezier_ease(&CURVE_EMPHASIS, 0.5);
        assert!((mid - 0.5).abs() < 0.01, "got {}", mid);
    }

    #[test]
    fn exit_curve_fast_end() {
        // 退出 ease-in：半程进度 <0.3（先慢后快）。
        assert!(bezier_ease(&CURVE_EXIT, 0.5) < 0.3);
    }

    #[test]
    fn spring_never_below_zero() {
        for i in 0..=20 {
            let t = i as f32 / 20.0;
            let y = spring_ease(t, 200);
            assert!(y >= 0.0 && y <= SPRING_OVERSHOOT_PCT + 1e-4, "t={} y={}", t, y);
        }
    }

    #[test]
    fn zero_intensity_instant() {
        // 关闭档：时长 0——动画语义退化为瞬时切换（不是消失）。
        assert_eq!(effective_duration(MotionUse::Panel, Intensity::Off), 0);
    }
}
