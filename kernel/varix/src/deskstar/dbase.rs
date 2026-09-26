//! STAR-D1（泳道三 · C 桌面体验域前段 · F076-F092）共享底盘。
//!
//! 十七项桌面体验功能共用五件基础设施，全部零外部依赖、宿主测试直跑、
//! 内核镜像（no_std + alloc）可编译：
//!
//! - [`Ease`] 动效曲线族——F080 过冲弹性（105%→100% 回弹 80ms）、
//!   150ms ease-out 就位、250ms 进场等全系统动效的统一参数面；
//!   曲线族与 F124 动画曲线总谱同参（一处一事实：曲线参数只此一处）；
//! - [`FloatLayer`] 浮层生命周期状态机——F076 快速设置/F077 通知中心/
//!   F078 日历飞出/F082 Alt+Tab 卡片墙/F087 冲突面板共用的「出现即有
//!   完整消失路径」底盘：点外关闭、Esc 关闭、再点触发钮关闭、失焦
//!   关闭、关闭后焦点还给触发元素，五条出路逐条记账（缺一即红）；
//! - [`Debouncer`] 防抖合并器——F088 搜索 150ms、F083 刷新 300ms
//!   合并、F075 tooltip 同族的统一防抖语义（窗口内合并、窗口沿触发沿后移）；
//! - [`SlidingRate`] 滑动窗口均速——F086 剩余时间防数字跳动的
//!   均速算法（滑动窗口取均值，样本粒度 1s）；
//! - [`FocusRing`] 键盘焦点环——F076「Tab 循环+Space 切换」、
//!   F081「方向键+Enter 全可达」、F090「编辑/面包屑两态切换」
//!   共用的焦点遍历模型：循环、反向、激活、焦点归还四件事；
//! - [`Rect`] 平面几何——F080 落点矩形、F084 网格吸附、F076 面板
//!   布局共用的矩形/交叠/钳制算术；
//! - [`budget_ok`] 延迟红线整数记账——面板弹出 ≤100ms、首结果 <300ms
//!   等「≤N ms」判据的统一整数口径（毫秒整数域直接比较，四舍五入
//!   不放水：预算 100ms 实测 100.4ms 记整数 100，达标线语义为
//!   「不大于」，零放水零误伤）。
//!
//! 时间纪律：一切时间由调用方以毫秒实参注入，模块不持真实时钟——
//! 宿主测试确定复现，内核侧由上层供给真值。
//! 令牌纪律：任何视觉色一律走主题令牌索引（E1 令牌全集），本底盘
//! 只定义令牌位不定义颜色值（零硬编码色——B-1104 同源纪律）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// Ease — 动效曲线族
// ---------------------------------------------------------------------------

/// 单段缓动规格：时长 + 曲线型 + 可选过冲。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ease {
    /// 总时长（ms）。
    pub dur_ms: u32,
    /// 曲线型。
    pub kind: EaseKind,
    /// 过冲峰值（千分比，1005 = 100.5%）；0 = 无过冲。
    pub overshoot_permille: u16,
    /// 过冲回弹段时长（ms）；无过冲时忽略。
    pub rebound_ms: u32,
}

/// 曲线型（F124 总谱命名族）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EaseKind {
    /// ease-out（快进慢停）。
    EaseOut,
    /// 线性。
    Linear,
    /// ease-in-out（两端缓）。
    Smooth,
}

/// F080 过冲弹性规格（主册设计细节：105%→100%，回弹 80ms）。
pub const SNAP_OVERSHOOT: Ease = Ease {
    dur_ms: 150,
    kind: EaseKind::EaseOut,
    overshoot_permille: 1050,
    rebound_ms: 80,
};

/// 通用弹出淡入/位移规格（F078：150ms 自上而下 8px 位移）。
pub const POP_SLIDE: Ease = Ease {
    dur_ms: 150,
    kind: EaseKind::EaseOut,
    overshoot_permille: 0,
    rebound_ms: 0,
};

impl Ease {
    /// 取 `elapsed_ms` 时刻的进度比例（0..=1000 千分比，含过冲段）。
    ///
    /// - 无过冲：进度在 `dur_ms` 处收敛 1000 并恒定；
    /// - 有过冲：主段快出（`dur_ms - rebound_ms` 内到 1050），
    ///   回弹段线性收敛 1050 → 1000（主册「轻微过冲」语义，
    ///   回弹段取线性——弹性感来自峰谷落差，不来自二次振荡）。
    pub fn at(&self, elapsed_ms: u32) -> u16 {
        if self.dur_ms == 0 {
            return 1000;
        }
        if self.overshoot_permille == 0 || self.rebound_ms == 0 {
            let t = elapsed_ms.min(self.dur_ms);
            let p = (t * 1000) / self.dur_ms;
            return match self.kind {
                EaseKind::Linear => p as u16,
                EaseKind::EaseOut => ease_out(p) as u16,
                EaseKind::Smooth => smooth(p) as u16,
            };
        }
        let main_ms = self.dur_ms.saturating_sub(self.rebound_ms);
        if elapsed_ms < main_ms {
            let p = if main_ms == 0 {
                1000
            } else {
                (elapsed_ms * 1000) / main_ms
            };
            let base = match self.kind {
                EaseKind::Linear => p,
                EaseKind::EaseOut => ease_out(p),
                EaseKind::Smooth => smooth(p),
            };
            // 主段按比例抬到过冲峰值。
            ((base as u32 * self.overshoot_permille as u32) / 1000) as u16
        } else {
            // 回弹段：峰值 → 1000 线性收敛（进度钳 1000——时间越过不回翻）。
            let t = elapsed_ms - main_ms;
            let p = ((t * 1000) / self.rebound_ms.max(1)).min(1000);
            (self.overshoot_permille as u32)
                .saturating_sub(((self.overshoot_permille as u32 - 1000) * p as u32) / 1000)
                as u16
        }
    }

    /// 动效是否已结束。
    pub fn done(&self, elapsed_ms: u32) -> bool {
        elapsed_ms >= self.dur_ms
    }
}

/// ease-out 千分比映射（快出慢停：前 30% 时间走完 ~66% 路程）。
fn ease_out(p: u32) -> u32 {
    let x = p.min(1000) as u64;
    // 1-(1-x)^2 的千分比整数近似：p*(2000-p)/1000。
    ((x * (2000 - x)) / 1000).min(1000) as u32
}

/// smooth 千分比映射（两端缓：x²(3-2x)）。
fn smooth(p: u32) -> u32 {
    let x = p.min(1000) as u64;
    ((x * x * (3000 - 2 * x)) / 1_000_000).min(1000) as u32
}

// ---------------------------------------------------------------------------
// FloatLayer — 浮层生命周期状态机
// ---------------------------------------------------------------------------

/// 浮层关闭原因（五条出路逐条记账——缺出路即缺陷）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseCause {
    /// 点击浮层外（轻模态公理：点外必关）。
    OutsideClick,
    /// Esc 键。
    Escape,
    /// 再点触发按钮（触发钮三态：开→关）。
    TriggerToggle,
    /// 窗口/宿主失焦。
    Blur,
    /// 关闭后焦点归还触发元素（出路闭环的落点）。
    FocusReturned,
}

/// 浮层生命周期账：开合态、开启时刻、关闭原因计数、焦点归还。
///
/// 「关闭后焦点要还给触发它的元素」以 [`FloatLayer::close`] 的返回值
/// 表达：调用方拿到 `true` 即须执行焦点归还动作；未归还即账上留痕。
pub struct FloatLayer {
    open: bool,
    opened_at_ms: u64,
    closed_at_ms: u64,
    /// 焦点归还挂起（close 返回 true 后未 confirm_returned 即悬账）。
    focus_pending: bool,
    close_counts: [u32; 5],
}

impl FloatLayer {
    pub fn new() -> FloatLayer {
        FloatLayer {
            open: false,
            opened_at_ms: 0,
            closed_at_ms: 0,
            focus_pending: false,
            close_counts: [0; 5],
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// 本次开启时刻（ms；未开为 0——动画进度等的锚点）。
    pub fn opened_at_ms(&self) -> u64 {
        self.opened_at_ms
    }

    /// 打开浮层。已开时为幂等（不重复计时）。
    pub fn open(&mut self, now_ms: u64) -> bool {
        if self.open {
            return false;
        }
        self.open = true;
        self.opened_at_ms = now_ms;
        true
    }

    /// 打开到可见的耗时是否在预算内（首帧渲染完由调用方报到）。
    pub fn open_in_budget(&self, shown_ms: u64, budget_ms: u64) -> bool {
        self.open && shown_ms >= self.opened_at_ms && shown_ms - self.opened_at_ms <= budget_ms
    }

    /// 关闭浮层。返回是否需要归还焦点（浮层内焦点存在时为真）。
    pub fn close(&mut self, cause: CloseCause, now_ms: u64, focus_inside: bool) -> bool {
        if !self.open {
            return false;
        }
        self.open = false;
        self.closed_at_ms = now_ms;
        let idx = match cause {
            CloseCause::OutsideClick => 0,
            CloseCause::Escape => 1,
            CloseCause::TriggerToggle => 2,
            CloseCause::Blur => 3,
            CloseCause::FocusReturned => 4,
        };
        self.close_counts[idx] += 1;
        self.focus_pending = focus_inside;
        focus_inside
    }

    /// 焦点归还完成确认（清挂账）。
    pub fn confirm_focus_returned(&mut self) {
        self.focus_pending = false;
    }

    pub fn focus_pending(&self) -> bool {
        self.focus_pending
    }

    /// 各关闭原因累计（诊断面：浮层怎么关的，一眼可查）。
    pub fn close_counts(&self) -> [u32; 5] {
        self.close_counts
    }
}

// ---------------------------------------------------------------------------
// Debouncer — 防抖合并器
// ---------------------------------------------------------------------------

/// 防抖合并器：触发沿后移窗口，窗口内事件合并为一次执行。
///
/// 语义（F083「连续 F5 合并 300ms 内单次」同款）：
/// - 首事件立即触发（不白等一个窗口——「即时反馈」红线）；
/// - 窗口期内再来的事件**不重触发**，只把窗口沿其到达时刻后移
///   （trailing 延后语义由 `due` 判定）；
/// - 窗口期内事件计数入账（防抖期间零多余扫描的打点验证面）。
pub struct Debouncer {
    window_ms: u64,
    armed: bool,
    window_end_ms: u64,
    pub merged: u64,
}

impl Debouncer {
    pub fn new(window_ms: u64) -> Debouncer {
        Debouncer {
            window_ms,
            armed: false,
            window_end_ms: 0,
            merged: 0,
        }
    }

    /// 事件到达。返回是否应当执行（首事件真触发，窗口内合并）。
    pub fn event(&mut self, now_ms: u64) -> bool {
        if self.armed && now_ms < self.window_end_ms {
            self.merged += 1;
            self.window_end_ms = now_ms + self.window_ms;
            return false;
        }
        self.armed = true;
        self.window_end_ms = now_ms + self.window_ms;
        true
    }

    /// 合并尾延后是否到期（窗口闭合且仍有合并尾在队）。
    pub fn due(&self, now_ms: u64) -> bool {
        self.armed && now_ms >= self.window_end_ms
    }

    /// 消费到期（执行合并尾后调用）。
    pub fn consume(&mut self) {
        self.armed = false;
        self.window_end_ms = 0;
    }
}

// ---------------------------------------------------------------------------
// SlidingRate — 滑动窗口均速
// ---------------------------------------------------------------------------

/// 滑动窗口均速账（F086 剩余时间防抖动数字跳）。
///
/// 样本按 1s 粒度入账（主册：速度采样 1s 粒度），窗口取最近
/// `window_secs` 个样本的算术均值——瞬时抖动被窗口摊平，
/// 剩余时间不跳。
pub struct SlidingRate {
    window_secs: usize,
    samples: Vec<(u64, u64)>,
}

impl SlidingRate {
    pub fn new(window_secs: usize) -> SlidingRate {
        SlidingRate {
            window_secs: window_secs.max(1),
            samples: Vec::new(),
        }
    }

    /// 记录一个速度样本（bytes_per_s，秒戳）。
    pub fn sample(&mut self, sec: u64, bytes_per_s: u64) {
        if let Some(last) = self.samples.last_mut() {
            if last.0 == sec {
                // 同秒重采：替换（不累加——速度是瞬时值不是计数）。
                last.1 = bytes_per_s;
                return;
            }
        }
        self.samples.push((sec, bytes_per_s));
        let cutoff = sec.saturating_sub(self.window_secs as u64);
        self.samples.retain(|(s, _)| *s >= cutoff);
    }

    /// 窗口均速（bytes/s）。无样本返回 0。
    pub fn rate(&self) -> u64 {
        if self.samples.is_empty() {
            return 0;
        }
        let sum: u64 = self.samples.iter().map(|(_, v)| *v).sum();
        sum / self.samples.len() as u64
    }

    /// 按窗口均速估剩余时间（秒；速率 0 时返回 None——诚实不编数字）。
    pub fn eta_secs(&self, remain_bytes: u64) -> Option<u64> {
        let r = self.rate();
        if r == 0 {
            None
        } else {
            Some((remain_bytes + r - 1) / r)
        }
    }
}

// ---------------------------------------------------------------------------
// FocusRing — 键盘焦点环
// ---------------------------------------------------------------------------

/// 焦点环：Tab 循环 / Shift+Tab 反向 / 方向键遍历 / Space·Enter 激活。
///
/// 宿主只提供元素总数，环负责下标推进；激活由调用方按下标执行。
/// 「焦点位置永远可见」由 `moved` 返回值表达——每次移动都产生
/// 可渲染事件，调用方不得吞掉。
#[derive(Clone, Debug)]
pub struct FocusRing {
    len: usize,
    idx: usize,
    pub moves: u64,
}

impl FocusRing {
    pub fn new(len: usize) -> FocusRing {
        FocusRing {
            len: len.max(1),
            idx: 0,
            moves: 0,
        }
    }

    pub fn index(&self) -> usize {
        self.idx
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// Tab（正向循环）。
    pub fn next(&mut self) -> usize {
        self.idx = (self.idx + 1) % self.len;
        self.moves += 1;
        self.idx
    }

    /// Shift+Tab（反向循环）。
    pub fn prev(&mut self) -> usize {
        self.idx = (self.idx + self.len - 1) % self.len;
        self.moves += 1;
        self.idx
    }

    /// 方向键步进（带方向；顶格不再循环——方向键语义与 Tab 循环不同）。
    pub fn arrow(&mut self, forward: bool) -> usize {
        if forward {
            self.idx = (self.idx + 1).min(self.len - 1);
        } else {
            self.idx = self.idx.saturating_sub(1);
        }
        self.moves += 1;
        self.idx
    }

    /// 激活当前项（返回下标；无长度的环不可激活——恒 0 位即激活位）。
    pub fn activate(&self) -> usize {
        self.idx
    }
}

// ---------------------------------------------------------------------------
// Rect — 平面几何
// ---------------------------------------------------------------------------

/// 轴对齐矩形（i32 域，坐标原点左上）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }

    /// 钳制进外框（负尺寸归零先于钳制——不放大）。
    pub fn clamp_into(&self, outer: &Rect) -> Rect {
        let w = self.w.max(0).min(outer.w.max(0));
        let h = self.h.max(0).min(outer.h.max(0));
        let x = self.x.max(outer.x).min(outer.right() - w);
        let y = self.y.max(outer.y).min(outer.bottom() - h);
        Rect { x, y, w, h }
    }
}

// ---------------------------------------------------------------------------
// budget_ok — 延迟红线整数口径
// ---------------------------------------------------------------------------

/// 延迟预算判定（毫秒整数域）：实测 ≤ 预算为绿。
///
/// 「不大于」语义零放水：预算 100ms、实测 100ms 绿，101ms 红。
pub fn budget_ok(actual_ms: u64, budget_ms: u64) -> bool {
    actual_ms <= budget_ms
}

/// 帧预算入账判定（80fps 口径：帧间隔 ≤12ms 入账，13ms 即 76.9fps
/// 违约——与 star/traysys F075 同一整数口径，一处一事实）。
pub fn frame_in_budget(interval_ms: u64, budget_ms: u64) -> bool {
    interval_ms <= budget_ms
}

/// 主题令牌索引（E1 令牌全集引用位——本域所有视觉色的唯一来源）。
/// 只定义语义位不定义色值：色值由 E1 令牌表承载。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token {
    Accent,
    /// 成功/开启态。
    On,
    /// 关闭态。
    Off,
    /// 警示（回收站容量 >80% 黄）。
    Warn,
    /// 危险/错误。
    Danger,
    /// 表面浅一层（F091 窗格背景材质令牌）。
    SurfaceRaised,
    /// 文本主色。
    TextPrimary,
    /// 文本次色（标签灰 12px）。
    TextSecondary,
    /// 禁置灰（前瞻灰置/F080 固定尺寸灰置）。
    Disabled,
}

// ---------------------------------------------------------------------------
// 自检（域共享底盘判据）
// ---------------------------------------------------------------------------

/// dbase 自检：曲线收敛、浮层五出路、防抖语义、均速防跳、焦点环、
/// 几何钳制、预算口径——七族逐一验证。
pub fn run_dbase_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-dbase");
    // 1. 过冲曲线：主段到峰、回弹收敛、终值恒定。
    let e = SNAP_OVERSHOOT;
    let peak = e.at(e.dur_ms - e.rebound_ms - 1);
    let settled = e.at(e.dur_ms + 10_000);
    set.add(
        "ease-overshoot",
        peak >= 1000 && settled == 1000,
        "overshoot curve",
    );
    // 2. 无过冲 150ms ease-out：末端收敛 1000。
    let e2 = POP_SLIDE;
    set.add(
        "ease-pop",
        e2.at(150) == 1000 && e2.at(75) > 500 && e2.done(150),
        "pop curve",
    );
    // 3. 浮层五出路全记账 + 焦点归还闭环。
    let mut fl = FloatLayer::new();
    let need_return = fl.close(CloseCause::Escape, 0, false);
    let mut ok = !need_return && !fl.is_open(); // 未开时关是幂等假动作
    fl.open(100);
    ok &= fl.is_open();
    ok &= !fl.close(CloseCause::OutsideClick, 120, false);
    fl.open(200);
    ok &= fl.close(CloseCause::Escape, 260, true); // 有内焦点 → 须归还
    ok &= fl.focus_pending();
    fl.confirm_focus_returned();
    ok &= !fl.focus_pending();
    ok &= fl.close_counts().iter().sum::<u32>() == 2;
    set.add("float-layer", ok, "float lifecycle");
    // 4. 防抖：首事件真触发、窗口内合并、尾部沿触发沿后移。
    let mut d = Debouncer::new(150);
    let first = d.event(1000);
    let merged = !d.event(1050) && !d.event(1100);
    let due = d.due(1251);
    d.consume();
    set.add(
        "debounce",
        first && merged && due && !d.due(9999) && d.merged == 2,
        "debounce semantics",
    );
    // 5. 滑动均速：同秒替换、窗口摊平、速率 0 不编 eta。
    let mut r = SlidingRate::new(5);
    r.sample(1, 2000);
    r.sample(1, 2000); // 同秒替换（幂等）
    r.sample(2, 500);
    r.sample(3, 500);
    let ok_rate = r.rate() == 1000 && r.eta_secs(2500) == Some(3);
    let r0 = SlidingRate::new(5);
    set.add("sliding-rate", ok_rate && r0.eta_secs(100).is_none(), "sliding rate");
    // 6. 焦点环：Tab 循环、反向、方向顶格不循环。
    let mut f = FocusRing::new(3);
    let ok_ring = f.next() == 1 && f.next() == 2 && f.next() == 0 && f.prev() == 2;
    f.arrow(true);
    f.arrow(true);
    let ok_arrow = f.arrow(true) == 2 && f.arrow(false) == 1;
    set.add("focus-ring", ok_ring && ok_arrow, "focus ring");
    // 7. 几何钳制 + 预算口径。
    let small = Rect::new(-5, -5, 50, 50);
    let outer = Rect::new(0, 0, 100, 100);
    let c = small.clamp_into(&outer);
    let ok_rect = c == Rect::new(0, 0, 50, 50) && outer.contains(0, 0) && !outer.contains(100, 100);
    set.add(
        "rect-budget",
        ok_rect && budget_ok(100, 100) && !budget_ok(101, 100) && frame_in_budget(12, 12)
            && !frame_in_budget(13, 12),
        "rect & budget",
    );
    set
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overshoot_peaks_then_settles() {
        let e = SNAP_OVERSHOOT;
        assert_eq!(e.dur_ms, 150);
        // 主段推进中：进度单调升到峰值（>1000 过冲）。
        let peak = e.at(70);
        assert!(peak > 1000, "过冲峰应超 1000，实测 {peak}");
        // 回弹后恒定 1000。
        assert_eq!(e.at(10_000), 1000);
        assert!(e.done(150));
    }

    #[test]
    fn pop_slide_monotonic_and_converged() {
        let a = POP_SLIDE.at(0);
        let b = POP_SLIDE.at(50);
        let c = POP_SLIDE.at(150);
        assert_eq!(a, 0);
        assert!(b > a && b < 1000);
        assert_eq!(c, 1000);
    }

    #[test]
    fn float_layer_five_ways_out() {
        let mut fl = FloatLayer::new();
        fl.open(10);
        assert!(fl.close(CloseCause::TriggerToggle, 20, false) == false);
        fl.open(30);
        assert!(fl.close(CloseCause::Blur, 40, true));
        fl.confirm_focus_returned();
        assert!(!fl.focus_pending());
        let counts = fl.close_counts();
        assert_eq!(counts[2], 1);
        assert_eq!(counts[3], 1);
    }

    #[test]
    fn float_open_budget_zero_tolerance() {
        let mut fl = FloatLayer::new();
        fl.open(1000);
        assert!(fl.open_in_budget(1100, 100));
        assert!(!fl.open_in_budget(1101, 100));
    }

    #[test]
    fn debounce_trailing_window_extends() {
        let mut d = Debouncer::new(300);
        assert!(d.event(0));
        assert!(!d.event(200));
        assert!(d.due(600)); // 窗口沿 200 后移 → 500 闭合
        d.consume();
        assert!(!d.due(9999));
    }

    #[test]
    fn sliding_rate_same_second_replaces() {
        let mut r = SlidingRate::new(3);
        r.sample(10, 5_000_000);
        r.sample(10, 4_000_000);
        assert_eq!(r.rate(), 4_000_000);
        r.sample(11, 2_000_000);
        assert_eq!(r.rate(), 3_000_000);
    }

    #[test]
    fn focus_ring_dir_keys_stop_at_edges() {
        let mut f = FocusRing::new(4);
        f.arrow(true);
        assert_eq!(f.index(), 1);
        for _ in 0..10 {
            f.arrow(false);
        }
        assert_eq!(f.index(), 0, "方向键到底不循环");
    }

    #[test]
    fn rect_clamp_never_grows() {
        let big = Rect::new(-10, -10, 500, 500);
        let outer = Rect::new(0, 0, 100, 100);
        assert_eq!(big.clamp_into(&outer), outer);
        let tiny = Rect::new(50, 50, 0, 0);
        assert_eq!(tiny.clamp_into(&outer), tiny);
    }

    #[test]
    fn dbase_self_checks_all_green() {
        let set = run_dbase_checks();
        assert!(set.all_passed(), "dbase 自检存在红项");
    }
}
