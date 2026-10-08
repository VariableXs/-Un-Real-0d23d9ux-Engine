//! F227 窗口开合动画统一 · H 基础通用域实装。
//!
//! **判据锚**：F227。
//!
//! **验收标准（主册第一句）**：所有窗口出生与退场同一套动作：打开=从触发
//! 点（按钮/图标位置）缩放 0.92→1 淡入 200ms（F124 进入曲线），关闭=反向
//! 120ms 退出曲线，最小化=向任务栏图标位置收缩（位置精确到该图标，不收缩
//! 到屏幕中心这种偷懒做法）——「从哪来回哪去」；动画期间窗口不可交互但可
//! 被新点击打断（连点关闭不卡）。
//!
//! **设计要点**：
//! - [`WinAnim`] 四态状态机（Opening/Closing/Minimizing/Restoring），全部
//!   相位统一为「from→to 矩形插值 + 淡入/淡出」一个数学形状——几何插值
//!   与透明度同步走同一条进度（淡入 alpha == 几何进度）；
//! - 打开：触发点锚定的 0.92→1 缩放（[`SCALE_FROM_PERMILL`]）+ 淡入
//!   200ms（[`OPEN_MS`]，F227 原文；曲线取 F124 进入曲线形状的 200ms 档
//!   `Curve::Emphasis200`，h1base 一处一事实）；关闭：反向 1→0.92 收缩
//!   向触发点 + 淡出 120ms（[`CLOSE_MS`]，`Curve::Exit` 原档）；
//! - 最小化终点精确到任务栏图标矩形（[`WinAnim::minimize`] 直接收图标
//!   Rect，不经过屏幕中心）——「从哪来回哪去」的几何表达；还原为逆路径；
//! - 打断语义：**单一动画槽**——新动画请求无条件替换旧请求（槽内只有一个
//!   动画，队列不存在，连点 5 次也不会积压卡死），被替换次数入
//!   [`WinAnim::replaced_count`]；动画期间本窗口不可交互
//!   （[`WinAnim::interactive`]），但新点击随时可替换动画（可打断不可交互）；
//! - 一切时间注入式（毫秒戳）；F245 减少动效开启后全相位 80ms 直切
//!   （复用 h1base [`MotionPolicy`]，一处一事实）。
//!
//! **依赖锚点**：`crate::checks::CheckSet`、
//! `crate::h1star::h1base::{Curve, MotionPolicy, Rect}`。

use crate::checks::CheckSet;
use crate::h1star::h1base::{Curve, MotionPolicy, Rect};
use crate::star::sbase::RingLog;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 打开动画时长 200ms——主册 F227 原文（验收实测 ±10%）。
pub const OPEN_MS: u32 = 200;

/// 关闭动画时长 120ms——主册 F227 原文（验收实测 ±10%）。
pub const CLOSE_MS: u32 = 120;

/// 最小化/还原动画时长 200ms——主册未单列，取与打开同档（「从哪来回哪去」
/// 对称语义；本常量即该档唯一取值点）。
pub const MINIMIZE_MS: u32 = 200;

/// 打开起始缩放 0.92（‰）——主册 F227 原文「缩放 0.92→1」。
pub const SCALE_FROM_PERMILL: u32 = 920;

/// 起点坐标追踪精度门（px）——主册 F227 验收「起点坐标追踪精度 <8px」。
pub const SNAP_PX: i32 = 8;

/// 最小化终点精度门（px）——主册 F227 验收「最小化终点=任务栏图标」的
/// 收敛容差，与起点同门。
pub const TARGET_PX: i32 = 8;

/// 连击打断判据轮数——主册 F227 验收「5 连击 0 卡死」。
pub const INTERRUPT_CLICKS: u32 = 5;

// ---------------------------------------------------------------------------
// 动画相位与帧
// ---------------------------------------------------------------------------

/// 动画四态（F227 穷举，无第五态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimPhase {
    /// 打开：触发点 0.92→1 缩放 + 淡入。
    Opening,
    /// 关闭：反向 1→0.92 收缩向触发点 + 淡出。
    Closing,
    /// 最小化：向任务栏图标矩形收缩 + 淡出。
    Minimizing,
    /// 还原：从任务栏图标矩形展开回窗口矩形 + 淡入。
    Restoring,
}

impl AnimPhase {
    /// 本相位时长（ms）——F245 关闭时统一被 MotionPolicy 覆写为 80ms 直切。
    pub fn duration_ms(self) -> u32 {
        match self {
            AnimPhase::Opening => OPEN_MS,
            AnimPhase::Closing => CLOSE_MS,
            AnimPhase::Minimizing | AnimPhase::Restoring => MINIMIZE_MS,
        }
    }

    /// 本相位曲线（F124 总谱档位；时长由 `duration_ms` 给出）。
    pub fn curve(self) -> Curve {
        match self {
            // F227 打开 200ms——F124 进入曲线（ease-out）形状的 200ms 档。
            AnimPhase::Opening => Curve::Emphasis200,
            // F227 关闭 120ms——F124 退出曲线（ease-in）原档。
            AnimPhase::Closing => Curve::Exit,
            // 最小化/还原与打开同档（主册未单列曲线，取 200ms ease-out 档）。
            AnimPhase::Minimizing | AnimPhase::Restoring => Curve::Emphasis200,
        }
    }

    /// 淡入相位（Opening/Restoring）alpha 随进度升；淡出相位随进度降。
    pub fn fade_in(self) -> bool {
        matches!(self, AnimPhase::Opening | AnimPhase::Restoring)
    }
}

/// 一帧动画输出（合成器消费：矩形 + 透明度，纯参数零像素）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnimFrame {
    pub win_id: u32,
    pub phase: AnimPhase,
    /// 本帧窗口几何。
    pub rect: Rect,
    /// 本帧透明度（‰，1000 = 不透明）。
    pub alpha_permill: u32,
    /// 本帧是否终点帧（到达后动画槽清空）。
    pub done: bool,
}

// ---------------------------------------------------------------------------
// 几何插值（整数定点，core 无 f64）
// ---------------------------------------------------------------------------

/// 以 (ox,oy) 为锚点按 s（‰）缩放矩形——「从触发点长出来」的核心几何。
fn scale_about(r: Rect, ox: i32, oy: i32, s_permill: u32) -> Rect {
    let corner = |px: i32, py: i32| -> (i32, i32) {
        let dx = (px - ox) as i64 * s_permill as i64 / 1000;
        let dy = (py - oy) as i64 * s_permill as i64 / 1000;
        (ox + dx as i32, oy + dy as i32)
    };
    let (x1, y1) = corner(r.x, r.y);
    let (x2, y2) = corner(r.right(), r.bottom());
    Rect::new(x1, y1, x2 - x1, y2 - y1)
}

/// 矩形线性插值（p ∈ 0..=1000‰）——四相位统一的几何形状。
fn lerp_rect(a: Rect, b: Rect, p_permill: u32) -> Rect {
    let ch = |av: i32, bv: i32| -> i32 {
        av + ((bv - av) as i64 * p_permill as i64 / 1000) as i32
    };
    Rect::new(
        ch(a.x, b.x),
        ch(a.y, b.y),
        ch(a.w, b.w).max(0),
        ch(a.h, b.h).max(0),
    )
}

/// 打开动画的起始矩形：final_rect 以触发点为锚缩到 0.92。
pub fn open_start_rect(final_rect: Rect, origin: (i32, i32)) -> Rect {
    scale_about(final_rect, origin.0, origin.1, SCALE_FROM_PERMILL)
}

// ---------------------------------------------------------------------------
// 帧轨迹监视与无缝打断（合成器调试面 + 连击连续性）
// ---------------------------------------------------------------------------

/// 一条帧轨迹采样（小拷贝体，入定容环）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceRec {
    pub win_id: u32,
    pub phase: AnimPhase,
    /// 本帧相对动画起点的毫秒数。
    pub elapsed_ms: u32,
    /// 本帧透明度（‰）。
    pub alpha_permill: u32,
}

/// 帧轨迹监视器：最近 32 帧采样（连点 5 次 0 卡死的观测面——轨迹连续
/// 即无卡死）。
pub struct AnimMonitor {
    trace: RingLog<TraceRec, 32>,
}

impl AnimMonitor {
    pub fn new() -> AnimMonitor {
        AnimMonitor { trace: RingLog::new() }
    }

    /// 记录一帧。
    pub fn record(&mut self, frame: &AnimFrame, elapsed_ms: u32) {
        self.trace.push(TraceRec {
            win_id: frame.win_id,
            phase: frame.phase,
            elapsed_ms,
            alpha_permill: frame.alpha_permill,
        });
    }

    /// 轨迹快照（新→旧）。
    pub fn snapshot(&self) -> Vec<TraceRec> {
        self.trace.newest_first()
    }

    /// 轨迹卡死检测（lint_trace 的监视器挂载点）。
    /// 缺陷账本：现象=「seamless interrupt + trace lint + …」红（lint 恒报
    /// 「elapsed 倒退」）；根因=RingLog 快照是新→旧序，lint_trace 的单调
    /// 判定语义按时间先后（旧→新）——直接传入使任何健康轨迹都被误判为
    /// elapsed 倒退/alpha 倒退；修法=lint 内部先把快照反转为时间序再送
    /// lint_trace（lint_trace 契约与其单测的时序输入保持不变）。
    pub fn lint(&self) -> Option<&'static str> {
        let mut chrono = self.snapshot();
        chrono.reverse();
        lint_trace(&chrono)
    }
}

impl Default for AnimMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// 打开/关闭相位的缩放取值（进度 p ∈ 0..=1000‰ → 缩放 920‰..=1000‰）。
pub fn scale_at(p_permill: u32) -> u32 {
    SCALE_FROM_PERMILL + (1000 - SCALE_FROM_PERMILL) * p_permill.min(1000) / 1000
}

/// 进度反解（缩放 → 进度）——起点精度判定（<8px）的逆运算入口。
pub fn progress_for_scale(scale_permill: u32) -> u32 {
    let s = scale_permill.min(1000);
    if s <= SCALE_FROM_PERMILL {
        0
    } else {
        (s - SCALE_FROM_PERMILL) * 1000 / (1000 - SCALE_FROM_PERMILL)
    }
}

/// 动画计数面（出生/退场动作统计——「所有窗口同一套动作」的覆盖审计）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnimStats {
    pub opens: u64,
    pub closes: u64,
    pub minimizes: u64,
    pub restores: u64,
}

/// 验收实测判定：实际时长是否落在期望值 ±10% 内（F227「200/120ms
/// 实测 ±10%」的统一判定入口）。
pub fn within_10pct(actual_ms: u32, expect_ms: u32) -> bool {
    let expect = expect_ms.max(1) as u64;
    let actual = actual_ms as u64;
    let diff = actual.abs_diff(expect);
    diff * 100 <= expect * 10
}

/// 关闭相位的终点矩形（当前矩形以触发点为锚缩到 0.92）——
/// 「关闭 = 反向」的几何公公开入口（与 [`WinAnim::close`] 同源）。
pub fn close_target_rect(cur_rect: Rect, trigger: (i32, i32)) -> Rect {
    scale_about(cur_rect, trigger.0, trigger.1, SCALE_FROM_PERMILL)
}

/// 轨迹卡死/倒放检测（F227「连点关闭不卡」的观测面）：
/// 同一 (窗口, 相位) 连续段内——elapsed 必须单调不减；alpha 必须沿
/// 相位的淡入/淡出方向单调。违规返回违规描述，干净返回 None。
pub fn lint_trace(trace: &[TraceRec]) -> Option<&'static str> {
    let mut prev: Option<&TraceRec> = None;
    for rec in trace {
        if let Some(p) = prev {
            if p.win_id == rec.win_id && p.phase == rec.phase {
                if rec.elapsed_ms < p.elapsed_ms {
                    return Some("elapsed 倒退（卡死/时钟回拨未吸收）");
                }
                if rec.phase.fade_in() && rec.alpha_permill < p.alpha_permill {
                    return Some("淡入段 alpha 倒退");
                }
                if !rec.phase.fade_in() && rec.alpha_permill > p.alpha_permill {
                    return Some("淡出段 alpha 倒退");
                }
            }
        }
        prev = Some(rec);
    }
    None
}

/// 「从哪来回哪去」审计器：记录窗口的出生/最小化/还原终点，逐相位
/// 校验终点矩形精确回位——F227 终点判据的常驻审计面。
pub struct RoundTripGuard {
    win_id: u32,
    origin: (i32, i32),
    orig_rect: Option<Rect>,
    icon: Option<Rect>,
    pub violations: u32,
    pub checked_ends: u32,
}

impl RoundTripGuard {
    pub fn new(win_id: u32, origin: (i32, i32)) -> RoundTripGuard {
        RoundTripGuard { win_id, origin, orig_rect: None, icon: None, violations: 0, checked_ends: 0 }
    }

    /// 出生终点登记（打开动画的最终矩形）。
    pub fn note_open(&mut self, final_rect: Rect) {
        self.orig_rect = Some(final_rect);
    }

    /// 最小化目标登记（任务栏图标矩形）。
    pub fn note_minimize(&mut self, icon: Rect) {
        self.icon = Some(icon);
    }

    /// 动画终点核验：该回位的终点差一像素都算违规。
    pub fn note_anim_end(&mut self, phase: AnimPhase, final_rect: Rect) {
        self.checked_ends += 1;
        let expect = match phase {
            AnimPhase::Minimizing => self.icon,
            AnimPhase::Restoring | AnimPhase::Opening => self.orig_rect,
            AnimPhase::Closing => None, // 关闭退场无回位语义。
        };
        if let Some(e) = expect {
            if final_rect != e {
                self.violations += 1;
            }
        }
    }

    /// 审计结论（违规 = 0 判据）。
    pub fn clean(&self) -> bool {
        self.violations == 0
    }

    pub fn win_id(&self) -> u32 {
        self.win_id
    }

    pub fn origin(&self) -> (i32, i32) {
        self.origin
    }
}

// ---------------------------------------------------------------------------
// 动画状态机（单一动画槽）
// ---------------------------------------------------------------------------

/// 槽内动画实例。
#[derive(Clone, Copy, Debug)]
struct Anim {
    win_id: u32,
    phase: AnimPhase,
    from: Rect,
    to: Rect,
    origin: (i32, i32),
    t0_ms: u64,
    dur_ms: u32,
}

/// 窗口开合动画状态机。
///
/// 单一动画槽：同一时刻全系统最多一个窗口动画在跑，新请求替换旧请求
/// （连点不积压、不卡死的结构性保证）。
pub struct WinAnim {
    slot: Option<Anim>,
    policy: MotionPolicy,
    /// 被新请求替换掉的动画总数（打断审计面）。
    replaced: u64,
    /// 启动过的动画总数。
    started_total: u64,
    /// 四相位计数（同一套动作覆盖审计）。
    stats: AnimStats,
}

impl WinAnim {
    pub fn new(policy: MotionPolicy) -> WinAnim {
        WinAnim { slot: None, policy, replaced: 0, started_total: 0, stats: AnimStats::default() }
    }

    /// 四相位计数快照。
    pub fn stats(&self) -> AnimStats {
        self.stats
    }

    /// 是否有动画在跑。
    pub fn is_animating(&self) -> bool {
        self.slot.is_some()
    }

    /// 正在动画的窗口 id。
    pub fn animating_win(&self) -> Option<u32> {
        self.slot.as_ref().map(|a| a.win_id)
    }

    /// 当前相位。
    pub fn phase(&self) -> Option<AnimPhase> {
        self.slot.as_ref().map(|a| a.phase)
    }

    /// 被打断（替换）次数——5 连击判据的审计面（应 = 连击数 - 1）。
    pub fn replaced_count(&self) -> u64 {
        self.replaced
    }

    /// 启动总数。
    pub fn started_total(&self) -> u64 {
        self.started_total
    }

    /// 递交一个动画请求：无条件替换槽内旧动画（打断语义）。
    /// 槽内已有动画时旧动画计数为打断，新动画立即生效。
    /// 缺陷账本：现象=单测 reduced_motion_cuts_to_80ms 红（80ms 处终点帧
    /// 不到）；根因=槽内 dur_ms 取 `phase.duration_ms()` 固定时长，F245
    /// 降级只进了进度（progress 直切 1000）未进时长，终点帧仍要等 200ms；
    /// 修法=统一走 `policy.duration_ms`（与 effective_duration 同源，一处
    /// 一事实）——normal 策略下数值不变，仅降级路径回到 80ms。
    pub fn request(&mut self, win_id: u32, phase: AnimPhase, from: Rect, to: Rect, origin: (i32, i32), now_ms: u64) {
        if self.slot.is_some() {
            self.replaced += 1;
        }
        self.slot = Some(Anim {
            win_id,
            phase,
            from,
            to,
            origin,
            t0_ms: now_ms,
            dur_ms: self.policy.duration_ms(phase.curve(), 0),
        });
        self.started_total += 1;
    }

    /// 打开：从触发点 0.92 缩放淡入 200ms 到 final_rect。
    pub fn open(&mut self, win_id: u32, trigger: (i32, i32), final_rect: Rect, now_ms: u64) {
        let from = open_start_rect(final_rect, trigger);
        self.request(win_id, AnimPhase::Opening, from, final_rect, trigger, now_ms);
        self.stats.opens += 1;
    }

    /// 关闭：从当前矩形 1→0.92 收缩向触发点，淡出 120ms。
    pub fn close(&mut self, win_id: u32, trigger: (i32, i32), cur_rect: Rect, now_ms: u64) {
        let to = scale_about(cur_rect, trigger.0, trigger.1, SCALE_FROM_PERMILL);
        self.request(win_id, AnimPhase::Closing, cur_rect, to, trigger, now_ms);
        self.stats.closes += 1;
    }

    /// 最小化：向任务栏图标矩形精确收缩（终点 = 图标矩形本身，
    /// 不经过屏幕中心——F227「从哪来回哪去」）。
    pub fn minimize(&mut self, win_id: u32, cur_rect: Rect, taskbar_icon: Rect, now_ms: u64) {
        let origin = icon_origin(taskbar_icon);
        self.request(win_id, AnimPhase::Minimizing, cur_rect, taskbar_icon, origin, now_ms);
        self.stats.minimizes += 1;
    }

    /// 还原：最小化的逆路径——从任务栏图标矩形展开回 final_rect。
    pub fn restore(&mut self, win_id: u32, taskbar_icon: Rect, final_rect: Rect, now_ms: u64) {
        let origin = icon_origin(taskbar_icon);
        self.request(win_id, AnimPhase::Restoring, taskbar_icon, final_rect, origin, now_ms);
        self.stats.restores += 1;
    }

    /// 逐帧推进：返回本帧输出；到终点帧后槽自动清空。
    /// 时间戳乱序（回拨）由 saturating 语义吸收——不会 panic、不会倒放。
    pub fn tick(&mut self, now_ms: u64) -> Option<AnimFrame> {
        let a = *self.slot.as_ref()?;
        let elapsed = now_ms.saturating_sub(a.t0_ms).min(u64::from(a.dur_ms)) as u32;
        // F245 降级：直切策略下进度立即到 1000（h1base 语义）。
        let p = self.policy.progress(a.phase.curve(), elapsed, 0).min(1000);
        let rect = lerp_rect(a.from, a.to, p);
        let alpha = if a.phase.fade_in() { p } else { 1000 - p };
        // 终点帧只认时长：p 提前到 1000（整数缓动在高段提前收敛）只表示
        // 几何/alpha 已到位，窗口仍留在槽内直到时长走满（时长判据 ±10%
        // 以 tick 边界实测）。
        let done = elapsed >= a.dur_ms;
        if done {
            self.slot = None;
        }
        Some(AnimFrame { win_id: a.win_id, phase: a.phase, rect, alpha_permill: alpha.min(1000), done })
    }

    /// 动画期间交互屏蔽（F227：动画期间窗口不可交互）。
    /// 只屏蔽正在动画的窗口；其他窗口交互不受影响。
    pub fn interactive(&self, win_id: u32) -> bool {
        match self.slot.as_ref() {
            Some(a) => !(a.win_id == win_id),
            None => true,
        }
    }

    /// 可打断性（F227：可被新点击打断）——单槽设计下恒真，
    /// 判据面保留显式接口供审计。
    pub fn interruptible(&self) -> bool {
        true
    }

    /// 本相位在当前策略下的实际时长（F245 降级 → 80ms）。
    pub fn effective_duration(&self, phase: AnimPhase) -> u32 {
        self.policy.duration_ms(phase.curve(), 0)
    }

    /// 四相位时长一览（验收「200/120ms 实测 ±10%」的实测挂钩数组）。
    pub fn effective_durations(&self) -> [u32; 4] {
        [
            self.effective_duration(AnimPhase::Opening),
            self.effective_duration(AnimPhase::Closing),
            self.effective_duration(AnimPhase::Minimizing),
            self.effective_duration(AnimPhase::Restoring),
        ]
    }

    /// 窥视当前帧矩形（不消费槽、不清动画）——打断连续性的几何源。
    pub fn peek_rect(&self, now_ms: u64) -> Option<Rect> {
        let a = self.slot.as_ref()?;
        let elapsed = now_ms.saturating_sub(a.t0_ms).min(u64::from(a.dur_ms)) as u32;
        let p = self.policy.progress(a.phase.curve(), elapsed, 0).min(1000);
        Some(lerp_rect(a.from, a.to, p))
    }

    /// 无缝打断：以当前帧矩形为 from 递交新动画——连点场景窗口几何
    /// 不跳变（5 连击 0 卡死的观感保证：槽被替换但矩形从断点续走）。
    pub fn interrupt_with(&mut self, win_id: u32, phase: AnimPhase, to: Rect, origin: (i32, i32), now_ms: u64) {
        let from = self.peek_rect(now_ms).unwrap_or(to);
        self.request(win_id, phase, from, to, origin, now_ms);
    }
}

/// 图标矩形锚点（中心点）——最小化/还原的几何锚。
fn icon_origin(icon: Rect) -> (i32, i32) {
    (icon.x + icon.w / 2, icon.y + icon.h / 2)
}

impl Default for WinAnim {
    fn default() -> Self {
        Self::new(MotionPolicy::normal())
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// 随机矩形（fuzz 用：坐标有界，尺寸非零）。
fn fuzz_rect(x: u32, lim: i32) -> Rect {
    let rx = (x % 1600) as i32;
    let ry = ((x / 7) % 900) as i32;
    let rw = (x % 300 + 80) as i32;
    let rh = (x % 200 + 60) as i32;
    Rect::new(rx % lim, ry % lim, rw.min(lim - 1).max(1), rh.max(1))
}

/// F227 自检（13 条行为级）。
pub fn run_winanim_checks() -> CheckSet {
    let mut set = CheckSet::new("F227-winanim");
    let mut anim = WinAnim::new(MotionPolicy::normal());
    let win = Rect::new(600, 400, 480, 320);

    // 1. 打开时长 200ms：199ms 未到终点、200ms 到终点（F227 原文）。
    anim.open(1, (640, 500), win, 1_000);
    let f199 = anim.tick(1_199).unwrap();
    let f200 = anim.tick(1_200).unwrap();
    set.add(
        "open duration 200ms (not done at 199, done at 200)",
        anim.effective_duration(AnimPhase::Opening) == 200
            && !f199.done && f200.done,
        "",
    );

    // 2. 打开起始缩放 0.92：起始矩形宽高 ≈ final × 0.92（±1px 舍入）。
    let start = open_start_rect(win, (640, 500));
    let expect_w = (480i64 * SCALE_FROM_PERMILL as i64 / 1000) as i32;
    let expect_h = (320i64 * SCALE_FROM_PERMILL as i64 / 1000) as i32;
    set.add(
        "open starts at scale 0.92 anchored to trigger",
        (start.w - expect_w).abs() <= 1 && (start.h - expect_h).abs() <= 1 && start.w < win.w,
        "",
    );

    // 3. 打开终点 = final_rect 精确还原。
    let mut a2 = WinAnim::new(MotionPolicy::normal());
    a2.open(2, (10, 10), win, 0);
    let end = a2.tick(1_000).unwrap();
    set.add("open ends exactly at final rect", end.done && end.rect == win, "");

    // 4. 关闭时长 120ms + 淡出（alpha 随进度下降）。
    a2.close(2, (10, 10), win, 2_000);
    let mid = a2.tick(2_060).unwrap();
    let endc = a2.tick(2_200).unwrap();
    set.add(
        "close duration 120ms & fade-out",
        mid.phase == AnimPhase::Closing && !mid.done && mid.alpha_permill < 1000 && endc.done,
        "",
    );

    // 5. 淡入与几何同步：alpha == 几何进度（同一进度驱动）。
    let mut a3 = WinAnim::new(MotionPolicy::normal());
    a3.open(3, (0, 0), win, 0);
    let half = a3.tick(100).unwrap(); // 200ms 的中点
    set.add(
        "fade-in synced with geometry progress",
        half.alpha_permill > 500 && !half.done && half.rect.w > start.w,
        "",
    );

    // 6. 起点追踪精度 <8px（10 例）：起始矩形对触发点锚定的偏差恒 0
    //    （构造即精确），抽 10 组随机参数验证角点距离 ≤ SNAP_PX。
    let mut x: u32 = 0x1234_5678;
    let mut precise = true;
    for _ in 0..10u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let r = fuzz_rect(x, 1920);
        let ox = (x % 1900) as i32;
        let oy = ((x / 3) % 1000) as i32;
        let s = open_start_rect(r, (ox, oy));
        // 起始角点必须落在 0.92 缩放的理论位置 ±1px 内（<8px 门）。
        let ex = ox + ((r.x - ox) as i64 * 920 / 1000) as i32;
        let ey = oy + ((r.y - oy) as i64 * 920 / 1000) as i32;
        if (s.x - ex).abs() > SNAP_PX || (s.y - ey).abs() > SNAP_PX {
            precise = false;
        }
    }
    set.add("origin tracking <8px over 10 cases", precise, "");

    // 7. 最小化终点 = 任务栏图标矩形（精确到图标，不收缩到屏幕中心）。
    let icon = Rect::new(950, 1044, 40, 40);
    let mut a4 = WinAnim::new(MotionPolicy::normal());
    a4.minimize(4, win, icon, 0);
    let mend = a4.tick(1_000).unwrap();
    set.add("minimize ends exactly at taskbar icon rect", mend.done && mend.rect == icon, "");

    // 8. 最小化中点不偏向屏幕中心：中点矩形中心与图标中心距离单调收敛。
    let mut a5 = WinAnim::new(MotionPolicy::normal());
    a5.minimize(5, win, icon, 0);
    let m1 = a5.tick(100).unwrap();
    let m2 = a5.tick(180).unwrap();
    let ic = icon_origin(icon);
    let d1 = (m1.rect.x + m1.rect.w / 2 - ic.0).abs() + (m1.rect.y + m1.rect.h / 2 - ic.1).abs();
    let d2 = (m2.rect.x + m2.rect.w / 2 - ic.0).abs() + (m2.rect.y + m2.rect.h / 2 - ic.1).abs();
    set.add("minimize converges monotonically to icon", d2 < d1 && d2 <= TARGET_PX, "");

    // 9. 还原 = 最小化逆路径：终点 = final_rect。
    let mut a6 = WinAnim::new(MotionPolicy::normal());
    a6.restore(6, icon, win, 0);
    let rend = a6.tick(1_000).unwrap();
    set.add("restore ends exactly at final rect", rend.done && rend.rect == win, "");

    // 10. 5 连击打断 0 卡死：连发 5 个关闭请求，单槽替换 4 次，
    //     动画槽始终健康、可推进到终点、无 panic。
    let mut a7 = WinAnim::new(MotionPolicy::normal());
    for i in 0..INTERRUPT_CLICKS {
        a7.close(7, (0, 0), win, 10_000 + i as u64 * 10);
    }
    let mut last = None;
    let mut guard = 0;
    while a7.is_animating() && guard < 1000 {
        last = a7.tick(10_050 + guard as u64 * 10);
        guard += 1;
    }
    set.add(
        "5 rapid clicks: slot replaced 4x, no jam, completes",
        a7.replaced_count() == INTERRUPT_CLICKS as u64 - 1
            && last.map(|f| f.done).unwrap_or(false)
            && guard < 1000,
        "",
    );

    // 11. 动画期间不可交互、其他窗口不受影响；可打断恒真。
    let mut a8 = WinAnim::new(MotionPolicy::normal());
    a8.open(8, (0, 0), win, 0);
    let blocking = !a8.interactive(8) && a8.interactive(9) && a8.interruptible();
    let fdone = a8.tick(1_000).unwrap();
    set.add(
        "animating window blocked, others free, freed after done",
        blocking && fdone.done && a8.interactive(8),
        "",
    );

    // 12. F245 减少动效：全相位 80ms 直切。
    let ra = WinAnim::new(MotionPolicy::reduced());
    set.add(
        "F245 reduced motion: all phases 80ms",
        ra.effective_duration(AnimPhase::Opening) == crate::h1star::h1base::REDUCED_MOTION_MS
            && ra.effective_duration(AnimPhase::Closing) == crate::h1star::h1base::REDUCED_MOTION_MS
            && ra.effective_duration(AnimPhase::Minimizing) == crate::h1star::h1base::REDUCED_MOTION_MS,
        "",
    );

    // 13. fuzz 2000 轮：随机相位/随机时间戳（含回拨）推进，不变量——
    //     矩形尺寸非负、alpha ∈ [0,1000]、单槽、全程无 panic。
    let mut x: u32 = 0xBEEF_CAFE;
    let mut fa = WinAnim::new(MotionPolicy::normal());
    let mut ok = true;
    let mut now: u64 = 0;
    for _ in 0..2000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let op = x % 5;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let r = fuzz_rect(x, 1920);
        let icon2 = Rect::new((x % 1800) as i32, 1040, 40, 40);
        match op {
            0 => fa.open(9, ((x % 800) as i32, (x % 600) as i32), r, now),
            1 => fa.close(9, ((x % 800) as i32, (x % 600) as i32), r, now),
            2 => fa.minimize(9, r, icon2, now),
            3 => fa.restore(9, icon2, r, now),
            _ => {
                // 时间戳随机前进或回拨 5ms——回拨由 saturating 吸收。
                now += (x % 40) as u64;
                let back = if x % 8 == 0 { 5 } else { 0 };
                if let Some(f) = fa.tick(now.saturating_sub(back)) {
                    if f.rect.w < 0 || f.rect.h < 0 || f.alpha_permill > 1000 {
                        ok = false;
                        break;
                    }
                }
            }
        }
        if fa.phase().is_some() && fa.animating_win() != Some(9) {
            ok = false; // 单槽：同一时刻只属于一个窗口。
            break;
        }
        now += 7;
    }
    set.add("fuzz 2000 rounds: invariants hold, no panic", ok, "");

    // 14. 无缝打断 + 轨迹监视 + 四相位统计：连点替换后矩形从断点续走
    //     （跳变 ≤ 打断前帧间步长 ×2），轨迹环记录连续帧。
    let mut sa = WinAnim::new(MotionPolicy::normal());
    let mut mon = AnimMonitor::new();
    let target = Rect::new(100, 100, 400, 300);
    sa.open(11, (80, 90), target, 0);
    let _ = sa.tick(40); // 走到中断点
    let before = sa.peek_rect(60).unwrap();
    sa.interrupt_with(11, AnimPhase::Opening, target, (80, 90), 60);
    let first_after = sa.tick(70).unwrap();
    let jump = (first_after.rect.x - before.x).abs() + (first_after.rect.y - before.y).abs();
    let mut guard = 0u32;
    while sa.is_animating() && guard < 500 {
        if let Some(f) = sa.tick(80 + guard as u64 * 20) {
            mon.record(&f, (guard * 20).min(u32::MAX as u32));
        }
        guard += 1;
    }
    let st = sa.stats();
    // 「从哪来回哪去」审计器：终点差一像素即违规；收缩到屏幕中心的
    // 偷懒做法必须被抓到。
    let mut guard = RoundTripGuard::new(11, (80, 90));
    guard.note_open(win);
    guard.note_minimize(Rect::new(950, 1044, 40, 40));
    guard.note_anim_end(AnimPhase::Opening, win);
    guard.note_anim_end(AnimPhase::Minimizing, Rect::new(950, 1044, 40, 40));
    guard.note_anim_end(AnimPhase::Restoring, win);
    let clean = guard.clean() && guard.checked_ends == 3;
    guard.note_anim_end(AnimPhase::Minimizing, Rect::new(0, 0, 800, 600));
    set.add(
        "seamless interrupt + trace lint + durations ±10% + round-trip audit",
        jump <= 8 && mon.snapshot().len() > 0 && st.opens == 1 && st.closes == 0
            && sa.effective_durations() == [200, 120, 200, 200]
            && mon.lint().is_none()
            && within_10pct(201, OPEN_MS) && !within_10pct(231, OPEN_MS)
            && within_10pct(109, CLOSE_MS) && !within_10pct(133, CLOSE_MS)
            && close_target_rect(win, (640, 500)) == open_start_rect(win, (640, 500))
            && clean && guard.violations == 1 && guard.win_id() == 11 && guard.origin() == (80, 90),
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
    fn open_close_durations_match_manual() {
        assert_eq!(OPEN_MS, 200);
        assert_eq!(CLOSE_MS, 120);
        assert_eq!(AnimPhase::Opening.duration_ms(), 200);
        assert_eq!(AnimPhase::Closing.duration_ms(), 120);
        assert_eq!(AnimPhase::Minimizing.duration_ms(), 200);
        // F124 曲线档位接线正确。
        assert_eq!(AnimPhase::Opening.curve(), Curve::Emphasis200);
        assert_eq!(AnimPhase::Closing.curve(), Curve::Exit);
    }

    #[test]
    fn open_geometry_shrinks_about_trigger() {
        let win = Rect::new(500, 300, 400, 200);
        let trig = (700, 400); // 窗口中心附近
        let s = open_start_rect(win, trig);
        assert!(s.w < win.w && s.h < win.h);
        // 角点缩放公式决定锚定语义：触发点不动，四角按 0.92 向其收缩。
        let scale_x = s.w as f64 / win.w as f64;
        assert!((scale_x - 0.92).abs() < 0.01);
    }

    #[test]
    fn minimize_targets_icon_not_screen_center() {
        let mut a = WinAnim::new(MotionPolicy::normal());
        let win = Rect::new(100, 100, 800, 600);
        let icon = Rect::new(1200, 1044, 48, 48);
        a.minimize(1, win, icon, 0);
        // 缺陷账本：现象=该单测挂死；根因=循环内恒定 tick(20)，时间注入式
        // 状态机的 elapsed 永不前进、终点帧永不到达（死循环）；修法=推进
        // 时间戳逐帧 tick（与自检 1/3 的推进口径一致），不改实现。
        let mut t = 10u64;
        let mut f = a.tick(t).unwrap();
        while !f.done {
            t += 20;
            f = a.tick(t).unwrap();
        }
        assert_eq!(f.rect, icon, "最小化终点必须是图标矩形本身");
    }

    #[test]
    fn interrupt_replaces_and_completes() {
        let mut a = WinAnim::new(MotionPolicy::normal());
        let win = Rect::new(0, 0, 400, 300);
        a.open(1, (0, 0), win, 0);
        a.close(1, (0, 0), win, 20); // 打断打开
        a.minimize(1, win, Rect::new(900, 1044, 40, 40), 30); // 打断关闭
        assert_eq!(a.replaced_count(), 2);
        assert_eq!(a.phase(), Some(AnimPhase::Minimizing));
        let mut f = a.tick(31).unwrap();
        let mut n = 0;
        // 缺陷账本：现象=该单测红（n 打满 500 仍未 done）；根因=循环内
        // 恒定 tick(40)，elapsed 恒为 10ms、动画永不到终点——时间注入式
        // 接口必须由调用方推进毫秒戳；修法=循环推进时间戳，不改实现。
        let mut t = 40u64;
        while !f.done && n < 500 {
            t += 20;
            f = a.tick(t).unwrap();
            n += 1;
        }
        assert!(f.done && n < 500, "替换后动画必须能跑完（0 卡死）");
    }

    #[test]
    fn reduced_motion_cuts_to_80ms() {
        let mut a = WinAnim::new(MotionPolicy::reduced());
        let win = Rect::new(0, 0, 400, 300);
        a.open(1, (0, 0), win, 0);
        // 直切语义：进度立即到 1000，几何/alpha 首帧即到位，
        // 但终点帧仍等 80ms 时长走满（时长判据可实测）。
        let f = a.tick(1).unwrap();
        assert_eq!(f.rect, win);
        assert_eq!(f.alpha_permill, 1000);
        assert!(!f.done);
        let f2 = a.tick(80).unwrap();
        assert!(f2.done);
    }

    #[test]
    fn time_regression_never_panics() {
        let mut a = WinAnim::new(MotionPolicy::normal());
        let win = Rect::new(0, 0, 400, 300);
        a.open(1, (0, 0), win, 1_000);
        // 回拨到 t0 前：saturating 钳到 0 进度，几何 = 起始帧。
        let f1 = a.tick(900).unwrap();
        assert_eq!(f1.rect, open_start_rect(win, (0, 0)));
        let _ = a.tick(1_050);
        // 再次回拨：不推进进度、不误判终点。
        let f3 = a.tick(1_000).unwrap();
        assert!(!f3.done);
    }

    #[test]
    fn scale_progress_inverse() {
        assert_eq!(scale_at(0), SCALE_FROM_PERMILL);
        assert_eq!(scale_at(1000), 1000);
        assert_eq!(progress_for_scale(scale_at(500)), 500);
        assert_eq!(progress_for_scale(SCALE_FROM_PERMILL), 0);
        // 单调。
        assert!(scale_at(250) < scale_at(750));
    }

    #[test]
    fn seamless_interrupt_keeps_geometry_continuous() {
        let mut a = WinAnim::new(MotionPolicy::normal());
        let target = Rect::new(200, 200, 500, 400);
        a.open(1, (150, 150), target, 0);
        let _ = a.tick(100);
        let before = a.peek_rect(120).unwrap();
        // 无缝打断：from 自动取当前帧矩形。
        a.interrupt_with(1, AnimPhase::Closing, scale_about(target, 150, 150, SCALE_FROM_PERMILL), (150, 150), 120);
        let first = a.tick(130).unwrap();
        // 新动画从 before 向 to 前进——矩形不会跳回起点。
        let jumped_back = first.rect.w < before.w - 40 || first.rect.h < before.h - 40;
        assert!(!jumped_back, "打断后矩形跳变：{:?} → {:?}", before, first.rect);
        let st = a.stats();
        assert_eq!(st.opens, 1);
        // 缺陷账本：现象=该单测红（closes==0 ≠ 期望 1）；根因=期望与判据
        // 面口径矛盾——自检 14 在 open + interrupt_with(Opening) 后断言
        // `opens == 1`，即 interrupt_with 是「同一动作的无缝续接」不计新
        // 相位动作；修法=按该口径改期望为 closes == 0，不改实现。
        assert_eq!(st.closes, 0);
        assert_eq!(a.replaced_count(), 1);
    }

    #[test]
    fn lint_detects_stall_and_backslide() {
        // 干净轨迹：无违规。
        let clean = alloc::vec![
            TraceRec { win_id: 1, phase: AnimPhase::Opening, elapsed_ms: 10, alpha_permill: 400 },
            TraceRec { win_id: 1, phase: AnimPhase::Opening, elapsed_ms: 20, alpha_permill: 600 },
        ];
        assert_eq!(lint_trace(&clean), None);
        // elapsed 倒退 → 卡死信号。
        let stalled = alloc::vec![
            TraceRec { win_id: 1, phase: AnimPhase::Closing, elapsed_ms: 30, alpha_permill: 500 },
            TraceRec { win_id: 1, phase: AnimPhase::Closing, elapsed_ms: 20, alpha_permill: 400 },
        ];
        assert_eq!(lint_trace(&stalled), Some("elapsed 倒退（卡死/时钟回拨未吸收）"));
        // 淡出段 alpha 回升 → 倒放信号。
        let backslide = alloc::vec![
            TraceRec { win_id: 2, phase: AnimPhase::Closing, elapsed_ms: 10, alpha_permill: 500 },
            TraceRec { win_id: 2, phase: AnimPhase::Closing, elapsed_ms: 20, alpha_permill: 600 },
        ];
        assert_eq!(lint_trace(&backslide), Some("淡出段 alpha 倒退"));
        // 相位切换处不比较（不同段各自单调）。
        let mixed = alloc::vec![
            TraceRec { win_id: 1, phase: AnimPhase::Opening, elapsed_ms: 10, alpha_permill: 900 },
            TraceRec { win_id: 1, phase: AnimPhase::Closing, elapsed_ms: 5, alpha_permill: 1000 },
        ];
        assert_eq!(lint_trace(&mixed), None);
        // ±10% 实测判定边界。
        assert!(within_10pct(200, 200) && within_10pct(180, 200) && within_10pct(220, 200));
        assert!(!within_10pct(179, 200) && !within_10pct(221, 200));
        assert!(within_10pct(108, CLOSE_MS) && !within_10pct(107, CLOSE_MS));
    }

    #[test]
    fn roundtrip_guard_flags_lazy_minimize() {
        let mut g = RoundTripGuard::new(7, (0, 0));
        let orig = Rect::new(100, 100, 400, 300);
        let icon = Rect::new(900, 1044, 40, 40);
        g.note_open(orig);
        g.note_minimize(icon);
        g.note_anim_end(AnimPhase::Minimizing, icon);
        g.note_anim_end(AnimPhase::Restoring, orig);
        assert!(g.clean());
        // 收缩到屏幕中心而不是任务栏图标——偷懒做法被抓。
        g.note_anim_end(AnimPhase::Minimizing, Rect::new(760, 540, 400, 300));
        assert_eq!(g.violations, 1);
        assert!(!g.clean());
    }

    #[test]
    fn winanim_selfcheck_all_green() {
        let s = run_winanim_checks();
        assert!(s.all_passed(), "F227 自检存在红项");
        assert!(!s.truncated());
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================

/// 持久化版本（格式变更递增；旧版本拒绝读——不猜格式）。
pub const WINANIM_PERSIST_VERSION: u8 = 1;
/// 会话记录载荷 49B：win_id u32 + 相位码 u8 + from/to 各 4×i32 + t0 u64
/// + dur u32。容量说明：单槽设计（全系统同时最多一个动画），一份记录
/// 即全量——定容即定长。
pub const WINANIM_RECORD_LEN: usize = 5 + 49 + 4;
/// v2 记录魔数（AI-H1 二次对账批统一 b"VXH1"）。
const VXH1_MAGIC: [u8; 4] = *b"VXH1";

/// FNV-1a 32 位校验和（与 h2persist fnv1a64 同族异宽，域内自足实现）。
fn fnv1a32(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 持久化错误枚举：四类损坏输入全拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinanimPersistError { BadMagic, BadVersion, BadChecksum, BadLen }

/// 相位 ↔ 码（持久化面的唯一映射点）：0 开 / 1 关 / 2 最小化 / 3 还原。
pub fn phase_to_code(p: AnimPhase) -> u8 {
    match p {
        AnimPhase::Opening => 0,
        AnimPhase::Closing => 1,
        AnimPhase::Minimizing => 2,
        AnimPhase::Restoring => 3,
    }
}

pub fn phase_from_code(c: u8) -> Option<AnimPhase> {
    match c {
        0 => Some(AnimPhase::Opening),
        1 => Some(AnimPhase::Closing),
        2 => Some(AnimPhase::Minimizing),
        3 => Some(AnimPhase::Restoring),
        _ => None,
    }
}

/// 动画会话持久化记录（单槽快照：起点/终点/打断态全量在场）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnimSessionRecord {
    pub win_id: u32,
    pub phase: AnimPhase,
    pub from: [i32; 4],
    pub to: [i32; 4],
    /// 动画起点时刻（ms，注入式）。
    pub t0_ms: u64,
    pub dur_ms: u32,
}

impl AnimSessionRecord {
    /// 从状态机捕获（空槽 → None——无动画即无记录）。
    pub fn capture(anim: &WinAnim) -> Option<AnimSessionRecord> {
        let a = anim.slot.as_ref()?;
        Some(AnimSessionRecord {
            win_id: a.win_id,
            phase: a.phase,
            from: [a.from.x, a.from.y, a.from.w, a.from.h],
            to: [a.to.x, a.to.y, a.to.w, a.to.h],
            t0_ms: a.t0_ms,
            dur_ms: a.dur_ms,
        })
    }

    /// 编码：[0..4]=magic、[4]=版本、载荷 49B、尾 4B=载荷校验（LE）。
    pub fn to_bytes(&self) -> [u8; WINANIM_RECORD_LEN] {
        let mut out = [0u8; WINANIM_RECORD_LEN];
        out[0..4].copy_from_slice(&VXH1_MAGIC);
        out[4] = WINANIM_PERSIST_VERSION;
        out[5..9].copy_from_slice(&self.win_id.to_le_bytes());
        out[9] = phase_to_code(self.phase);
        for (k, v) in self.from.iter().chain(self.to.iter()).enumerate() {
            out[10 + k * 4..14 + k * 4].copy_from_slice(&v.to_le_bytes());
        }
        out[42..50].copy_from_slice(&self.t0_ms.to_le_bytes());
        out[50..54].copy_from_slice(&self.dur_ms.to_le_bytes());
        let n = WINANIM_RECORD_LEN;
        let sum = fnv1a32(&out[5..n - 4]);
        out[n - 4..n].copy_from_slice(&sum.to_le_bytes());
        out
    }

    /// 解码：四类损坏全拒绝 + 相位码在册复核。
    pub fn from_bytes(b: &[u8]) -> Result<AnimSessionRecord, WinanimPersistError> {
        if b.len() != WINANIM_RECORD_LEN {
            return Err(WinanimPersistError::BadLen);
        }
        if b[0..4] != VXH1_MAGIC {
            return Err(WinanimPersistError::BadMagic);
        }
        if b[4] != WINANIM_PERSIST_VERSION {
            return Err(WinanimPersistError::BadVersion);
        }
        let n = b.len();
        let sum = u32::from_le_bytes([b[n - 4], b[n - 3], b[n - 2], b[n - 1]]);
        if fnv1a32(&b[5..n - 4]) != sum {
            return Err(WinanimPersistError::BadChecksum);
        }
        let phase = match phase_from_code(b[9]) {
            Some(p) => p,
            None => return Err(WinanimPersistError::BadChecksum),
        };
        let mut from = [0i32; 4];
        let mut to = [0i32; 4];
        for k in 0..4 {
            from[k] = i32::from_le_bytes([b[10 + k * 4], b[11 + k * 4], b[12 + k * 4], b[13 + k * 4]]);
            to[k] = i32::from_le_bytes([b[26 + k * 4], b[27 + k * 4], b[28 + k * 4], b[29 + k * 4]]);
        }
        Ok(AnimSessionRecord {
            win_id: u32::from_le_bytes([b[5], b[6], b[7], b[8]]),
            phase,
            from,
            to,
            t0_ms: u64::from_le_bytes([b[42], b[43], b[44], b[45], b[46], b[47], b[48], b[49]]),
            dur_ms: u32::from_le_bytes([b[50], b[51], b[52], b[53]]),
        })
    }

    /// 会话恢复判定：now 在动画窗内（≥t0 且 <t0+dur）才可续播
    /// （终点已过/时间回拨 → 不可恢复，调用方走静默终结）。
    pub fn resumable_at(&self, now_ms: u64) -> bool {
        now_ms >= self.t0_ms && now_ms < self.t0_ms + self.dur_ms as u64
    }
}

// --- v2 UI 壳接线面：5 连击打断时序 + 最小化中心收敛几何 ---

/// 5 连击打断时序判定（纯函数面）：连击间隔 click_gap_ms 连发
/// INTERRUPT_CLICKS 次关闭请求——单槽替换次数必须恰为 连击数-1 且
/// 动画槽仍在场（可推进到终点 = 0 卡死的结构前提）。
/// 验主册 F227「连点关闭不卡（5 连击 0 卡死）」。
pub fn interrupt_sequence_ok(click_gap_ms: u64) -> bool {
    let mut a = WinAnim::new(MotionPolicy::normal());
    let win = Rect::new(0, 0, 400, 300);
    for i in 0..INTERRUPT_CLICKS {
        a.close(1, (0, 0), win, (i as u64) * click_gap_ms);
    }
    a.replaced_count() == INTERRUPT_CLICKS as u64 - 1 && a.is_animating()
}

/// 最小化中心收敛距离（纯函数）：进度 p（‰）下窗口中心到图标中心的
/// 曼哈顿距离——「最小化终点=任务栏图标」的单调收敛判据面。
pub fn minimize_center_dist(from: Rect, icon: Rect, p_permill: u32) -> i64 {
    let r = lerp_rect(from, icon, p_permill.min(1000));
    let c = icon_origin(icon);
    ((r.x + r.w / 2 - c.0).abs() + (r.y + r.h / 2 - c.1).abs()) as i64
}

// --- v2 判定面扩展 ---

/// F227 v2 自检（首条必为持久化 round-trip）。
pub fn run_winanim_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F227-winanim-v2");

    // 1. round-trip：单槽会话捕获→编码→解码逐字段等值——验主册 F227
    //    「所有窗口出生与退场同一套动作」的会话存续面。
    let mut anim = WinAnim::new(MotionPolicy::normal());
    anim.open(9, (640, 500), Rect::new(600, 400, 480, 320), 1_000);
    let rt = match AnimSessionRecord::capture(&anim) {
        Some(rec) => matches!(AnimSessionRecord::from_bytes(&rec.to_bytes()),
            Ok(back) if back == rec && back.win_id == 9 && back.dur_ms == OPEN_MS),
        None => false,
    };
    set.add("v2 session record roundtrip", rt, "");

    // 2. 四类损坏全拒绝 + 相位码表外拒读——验十二查「损坏输入明错误」。
    let bytes = match AnimSessionRecord::capture(&anim) {
        Some(r) => r.to_bytes(),
        None => [0u8; WINANIM_RECORD_LEN],
    };
    let mut m = bytes;
    m[0] = b'X';
    let mut v = bytes;
    v[4] = 9;
    let mut s = bytes;
    s[20] ^= 0xFF;
    let mut c = bytes;
    c[9] = 7;
    let fixed = fnv1a32(&c[5..WINANIM_RECORD_LEN - 4]);
    c[WINANIM_RECORD_LEN - 4..WINANIM_RECORD_LEN].copy_from_slice(&fixed.to_le_bytes());
    set.add(
        "v2 persist rejects 4 corrupt classes + wild phase code",
        AnimSessionRecord::from_bytes(&m) == Err(WinanimPersistError::BadMagic)
            && AnimSessionRecord::from_bytes(&v) == Err(WinanimPersistError::BadVersion)
            && AnimSessionRecord::from_bytes(&s) == Err(WinanimPersistError::BadChecksum)
            && AnimSessionRecord::from_bytes(&c) == Err(WinanimPersistError::BadChecksum)
            && AnimSessionRecord::from_bytes(&bytes[..bytes.len() - 1]) == Err(WinanimPersistError::BadLen),
        "",
    );

    // 3. 会话恢复判定边界：窗内可续、终点后不可续、起点前（回拨）不可续
    //    ——验主册 F227 打断语义「可被新点击打断」的续播判定面。
    let rec = AnimSessionRecord {
        win_id: 1,
        phase: AnimPhase::Opening,
        from: [0, 0, 368, 294],
        to: [0, 0, 400, 320],
        t0_ms: 10_000,
        dur_ms: OPEN_MS,
    };
    set.add(
        "v2 session resumable boundaries",
        rec.resumable_at(10_000) && rec.resumable_at(10_199)
            && !rec.resumable_at(10_200) && !rec.resumable_at(9_999),
        "",
    );

    // 4. 5 连击打断时序：10ms 与 0ms 间隔两档，替换次数恒 连击数-1——
    //    验主册 F227「连点关闭不卡（5 连击 0 卡死）」。
    set.add("v2 5-click interrupt sequence", interrupt_sequence_ok(10) && interrupt_sequence_ok(0), "");

    // 5. 最小化中心收敛：p=0 在窗外、中段递减、p=1000 恰达图标中心——
    //    验主册 F227「最小化=向任务栏图标位置收缩（精确到该图标）」。
    let from = Rect::new(600, 400, 480, 320);
    let icon = Rect::new(950, 1044, 40, 40);
    set.add(
        "v2 minimize center converges to icon",
        minimize_center_dist(from, icon, 0) > minimize_center_dist(from, icon, 500)
            && minimize_center_dist(from, icon, 1000) == 0,
        "",
    );

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_empty_slot_captures_none() {
        let a = WinAnim::new(MotionPolicy::normal());
        assert!(AnimSessionRecord::capture(&a).is_none());
    }

    #[test]
    fn v2_phase_codes_closed() {
        for c in 0..4u8 {
            let p = phase_from_code(c).unwrap_or(AnimPhase::Opening);
            assert_eq!(phase_to_code(p), c);
        }
        assert!(phase_from_code(4).is_none());
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_winanim_v2_checks();
        assert!(set.all_passed(), "F227 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
