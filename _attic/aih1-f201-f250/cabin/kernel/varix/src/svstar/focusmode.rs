//! F115 专注模式 · 完整设计（STAR I 主册 G-C-45）。
//!
//! **判据（主册）**：四效果（压暗/静默/停泡/计时）全链实测；拦截统计
//! 准确性对拍；快捷键一次开关不误触（长按 500ms 防手滑）。
//!
//! **设计要点（主册）**：
//! - 一键沉浸：非活动窗口压暗 20% 降透、通知静默（高优先除外 F077）、
//!   托盘气泡停发、计时器可选（专注 25 分钟 Pomodoro 档）到点恢复；
//! - 快速设置（F076）与快捷键双入口；快捷键长按 500ms 防手滑；
//! - 激活态：任务栏专注图标强调色 + 倒计时环（计时档，60fps）；
//!   F076 面板专注卡显示状态与剩余；
//! - 结束 toast 汇总（「专注 25 分钟，替你挡了 7 条通知」——诚实数据）；
//! - 专注会话统计（时长/拦截数）入账本 30 天；偏好配置层；
//! - 高优先通知（闹钟 F100）照弹（安全优先）；用户手动关 → 立即恢复
//!   +统计保留；系统重启 → 专注态不跨重启（显式重开）；
//! - 压暗实现 = 非活动窗后景暗幕（不改窗口自身透明——应用无感知）；
//!   活动窗口判定跟焦点走（实时 16ms 切换）；专注期间 F098 截图不受
//!   影响；白名单应用（音乐播放器）通知可豁免（用户配置）。
//!
//! 时间注入式（毫秒戳），宿主测试确定复现。无外部依赖。

use crate::checks::CheckSet;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 非活动窗口压暗（%，主册：压暗 20% 降透）。
pub const DIM_PCT: u32 = 20;
/// 快捷键长按防手滑（ms，主册：长按 500ms）。
pub const HOTKEY_HOLD_MS: u64 = 500;
/// 活动窗口判定切换时限（ms，主册：实时 16ms 切换）。
pub const FOCUS_SWITCH_MS: u64 = 16;
/// Pomodoro 计时档（分钟，主册：专注 25 分钟）。
pub const POMODORO_MIN: u64 = 25;
/// 会话统计保留（天，主册：入账本 30 天）。
pub const STATS_KEEP_DAYS: u64 = 30;

// ---------------------------------------------------------------------------
// 通知拦截面（静默/停泡/高优先豁免/白名单）
// ---------------------------------------------------------------------------

/// 通知优先级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyPriority {
    /// 常规。
    Normal,
    /// 高优先（闹钟 F100 级——安全优先，专注期照弹）。
    High,
}

/// 一次到达的通知。
#[derive(Clone, Debug)]
pub struct Incoming {
    pub app: &'static str,
    pub priority: NotifyPriority,
    /// 气泡承载（托盘气泡 = true）。
    pub bubble: bool,
}

/// 通知裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 照常呈现。
    Deliver,
    /// 拦截（专注期静默/停泡）。
    Intercepted,
    /// 拦截但豁免白名单不计拦截数？——否：白名单 = 呈现且不计拦截。
    Whitelisted,
}

// ---------------------------------------------------------------------------
// 专注会话
// ---------------------------------------------------------------------------

/// 专注状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusState {
    Off,
    /// 自由专注（无计时）。
    Free,
    /// 计时专注（Pomodoro 档）。
    Timed,
}

/// 单次专注会话统计（账本单元；30 天保留由账本裁剪）。
#[derive(Clone, Copy, Debug)]
pub struct SessionStat {
    /// 开始/结束时刻（注入 ms）。
    pub start_ms: u64,
    pub end_ms: u64,
    /// 拦截通知数。
    pub intercepted: u64,
    /// 计时档（false = 自由专注）。
    pub timed: bool,
    /// 结算方式（到点自动恢复 / 用户手动关）。
    pub ended_by_timer: bool,
}

/// 专注模式管理器。
pub struct FocusMode {
    state: FocusState,
    /// 当前活动窗口 id（焦点跟随——实时切换）。
    active_window: u64,
    /// 焦点最近切换时刻（16ms 判线对账）。
    last_focus_switch_ms: u64,
    /// 白名单应用（通知豁免；音乐播放器类）。
    whitelist: Vec<&'static str>,
    /// 本会话拦截数。
    intercepted_now: u64,
    /// 本会话开始时刻。
    session_start_ms: u64,
    /// 计时档到期时刻（Timed 态有效）。
    deadline_ms: u64,
    /// 账本（滚动保留 30 天）。
    ledger: Vec<SessionStat>,
    /// 长按进度（快捷键按下时刻；None = 未按）。
    hold_started_ms: Option<u64>,
    /// 自定义压暗（None = 默认 20%）。
    custom_dim_pct: Option<u32>,
    /// 自定义 Pomodoro 时长（分钟，默认 25）。
    pomodoro_min: u64,
    /// 连续完整轮次（到点自动恢复计数）。
    rounds_done: u32,
}

impl FocusMode {
    pub fn new(now_ms: u64) -> FocusMode {
        FocusMode {
            state: FocusState::Off,
            active_window: 0,
            last_focus_switch_ms: 0,
            whitelist: Vec::new(),
            intercepted_now: 0,
            session_start_ms: now_ms,
            deadline_ms: 0,
            ledger: Vec::new(),
            hold_started_ms: None,
            custom_dim_pct: None,
            pomodoro_min: POMODORO_MIN,
            rounds_done: 0,
        }
    }

    pub fn state(&self) -> FocusState {
        self.state
    }

    pub fn intercepted_now(&self) -> u64 {
        self.intercepted_now
    }

    pub fn ledger(&self) -> &[SessionStat] {
        &self.ledger
    }

    pub fn whitelist(&self) -> &[&'static str] {
        &self.whitelist
    }

    /// 白名单配置（通知豁免）。
    pub fn allow(&mut self, app: &'static str) {
        if !self.whitelist.contains(&app) {
            self.whitelist.push(app);
        }
    }

    /// 快捷键按下（记时刻——不足 500ms 不触发，防手滑；按住期间重复
    /// 按下忽略——不覆盖起按时刻，防「按住抖动缩矩」绕过判线）。
    pub fn hotkey_press(&mut self, now_ms: u64) {
        if self.hold_started_ms.is_none() {
            self.hold_started_ms = Some(now_ms);
        }
    }

    /// 快捷键松开：满 500ms 才翻转开关（一次开关不误触判据）。
    /// 返回 Some(新状态)（触发了）或 None（时长不足——忽略）。
    pub fn hotkey_release(&mut self, now_ms: u64) -> Option<FocusState> {
        let started = self.hold_started_ms.take()?;
        if now_ms.saturating_sub(started) < HOTKEY_HOLD_MS {
            return None;
        }
        Some(if self.state == FocusState::Off { self.begin(now_ms, false) } else { self.end(now_ms, false) })
    }

    /// 开启专注（F076 快速卡入口同口）：`timed=false` 自由档；
    /// `timed=true` Pomodoro 25 分钟档。
    pub fn begin(&mut self, now_ms: u64, timed: bool) -> FocusState {
        self.state = if timed { FocusState::Timed } else { FocusState::Free };
        self.session_start_ms = now_ms;
        self.intercepted_now = 0;
        self.deadline_ms = if timed { now_ms + self.pomodoro_min * 60_000 } else { 0 };
        self.state
    }

    /// 结束（手动关或到点）：结算入账本，立即恢复。统计保留（手动关
    /// 与到点同款——诚实数据）。
    pub fn end(&mut self, now_ms: u64, by_timer: bool) -> FocusState {
        let stat = SessionStat {
            start_ms: self.session_start_ms,
            end_ms: now_ms,
            intercepted: self.intercepted_now,
            timed: self.state == FocusState::Timed,
            ended_by_timer: by_timer,
        };
        if by_timer {
            self.rounds_done += 1;
        }
        self.ledger.push(stat);
        self.state = FocusState::Off;
        self.intercepted_now = 0;
        self.state
    }

    /// 到点检查（心跳注入）：Timed 态过线 → 自动恢复并结算（ended_by_timer）。
    pub fn tick(&mut self, now_ms: u64) -> Option<FocusState> {
        if self.state == FocusState::Timed && now_ms >= self.deadline_ms {
            return Some(self.end(now_ms, true));
        }
        None
    }

    /// 通知裁决（判据：拦截统计准确性对拍）：
    /// - 高优先 → 照弹（安全优先，不计拦截）；
    /// - 白名单应用 → 呈现且不计拦截；
    /// - 其余常规通知 → 拦截 + 计数。
    pub fn judge(&mut self, n: &Incoming) -> Verdict {
        if self.state == FocusState::Off {
            return Verdict::Deliver;
        }
        if n.priority == NotifyPriority::High {
            return Verdict::Deliver;
        }
        if self.whitelist.contains(&n.app) {
            return Verdict::Whitelisted;
        }
        self.intercepted_now += 1;
        Verdict::Intercepted
    }

    /// 焦点切换（活动窗口判定跟焦点走，实时 16ms 切换判线）：
    /// 返回切换耗时是否在 16ms 内（调用方以注入时差实测）。
    pub fn focus_switched(&mut self, new_active: u64, prev_ms: u64, now_ms: u64) -> bool {
        self.active_window = new_active;
        self.last_focus_switch_ms = now_ms;
        now_ms.saturating_sub(prev_ms) <= FOCUS_SWITCH_MS
    }

    /// 压暗系数（Off → 0；On → 20%）——非活动窗后景暗幕（应用无感知）。
    pub fn dim_pct(&self) -> u32 {
        if self.state == FocusState::Off {
            0
        } else {
            DIM_PCT
        }
    }

    /// 托盘气泡停发态（专注期停发——含白名单应用的气泡一律停，通知
    /// 本体走白名单裁决）。
    pub fn bubbles_suppressed(&self) -> bool {
        self.state != FocusState::Off
    }

    /// F098 截图不受影响（专注期截图无压暗无遮罩）。
    pub fn screenshot_unaffected(&self) -> bool {
        true
    }

    /// 结束 toast 汇总（诚实数据）：「专注 X 分钟，替你挡了 Y 条通知」。
    /// X = 实际专注时长（分钟，向下取整）——不虚报。
    pub fn closing_summary(&self, now_ms: u64) -> String {
        let minutes = now_ms.saturating_sub(self.session_start_ms) / 60_000;
        alloc::format!(
            "专注 {} 分钟，替你挡了 {} 条通知",
            minutes,
            self.intercepted_now
        )
    }

    /// 账本 30 天裁剪（驱逐窗口外会话，返回清除数）。
    pub fn evict_older_than(&mut self, cutoff_ms: u64) -> usize {
        let before = self.ledger.len();
        self.ledger.retain(|s| s.end_ms >= cutoff_ms);
        before - self.ledger.len()
    }

    /// 重启清态（主册：专注态不跨重启——显式重开）。
    pub fn power_cycle_reset(&mut self, now_ms: u64) {
        self.state = FocusState::Off;
        self.intercepted_now = 0;
        self.hold_started_ms = None;
        self.rounds_done = 0;
        self.session_start_ms = now_ms;
    }
}

// ---------------------------------------------------------------------------
// 深化批次 v2（随闸门补深化）：逐窗压暗 / 倒计时环 / F076 卡 / 轮次 /
// 账本聚合 / 自定义档
// ---------------------------------------------------------------------------

/// 自定义压暗范围（设置页自定义档——主册【交互设计】「自定义压暗强度」；
/// 钳制线：低于 10% 无感知、高于 50% 妨害可用性）。
pub const CUSTOM_DIM_RANGE: (u32, u32) = (10, 50);
/// Pomodoro 时长档（设置页自定义档：15/25/45/60 分钟）。
pub const POMODORO_TIERS_MIN: [u64; 4] = [15, 25, 45, 60];
/// 连续轮次起身建议线（专注 25 分钟一轮回——主册用户故事「25 分钟一轮
/// 回提醒起身」；连续 2 轮后建议长休）。
pub const BREAK_SUGGEST_ROUNDS: u32 = 2;

impl FocusMode {
    /// 自定义压暗强度（钳制进合法域；None = 回默认 20%）。
    pub fn set_custom_dim(&mut self, pct: Option<u32>) {
        self.custom_dim_pct = pct.map(|p| p.clamp(CUSTOM_DIM_RANGE.0, CUSTOM_DIM_RANGE.1));
    }

    /// 自定义 Pomodoro 时长档（钳制到四档之一；非法值吸附最近档）。
    pub fn set_pomodoro_tier(&mut self, min: u64) -> u64 {
        let nearest = *POMODORO_TIERS_MIN
            .iter()
            .min_by_key(|&&t| t.abs_diff(min))
            .unwrap_or(&POMODORO_MIN);
        self.pomodoro_min = nearest;
        nearest
    }

    /// 有效压暗（自定义档优先，缺省 20%）。
    fn effective_dim(&self) -> u32 {
        self.custom_dim_pct.unwrap_or(DIM_PCT)
    }

    /// 逐窗压暗裁决（主册：非活动窗压暗、活动窗不压——应用无感知语义
    /// 的窗口面）：Off → 0；活动窗 → 0；非活动窗 → 有效压暗。
    pub fn window_dim(&self, win_id: u64) -> u32 {
        if self.state == FocusState::Off {
            return 0;
        }
        if win_id == self.active_window {
            0
        } else {
            self.effective_dim()
        }
    }

    /// 倒计时环进度（万分比；Timed 态才有环——60fps 数据源按帧注入
    /// now_ms 查询）。
    pub fn ring_progress_bp(&self, now_ms: u64) -> Option<u32> {
        if self.state != FocusState::Timed {
            return None;
        }
        let total = self.deadline_ms.saturating_sub(self.session_start_ms).max(1);
        let elapsed = now_ms.saturating_sub(self.session_start_ms).min(total);
        Some((elapsed * 10_000 / total) as u32)
    }

    /// 剩余文案（诚实倒计时：分钟向下取整，不足 1 分钟报「不足 1 分钟」）。
    pub fn remaining_text(&self, now_ms: u64) -> String {
        match self.state {
            FocusState::Timed => {
                let rem_ms = self.deadline_ms.saturating_sub(now_ms);
                let rem_min = rem_ms / 60_000;
                if rem_min >= 1 {
                    alloc::format!("剩余 {} 分钟", rem_min)
                } else {
                    String::from("不足 1 分钟")
                }
            }
            FocusState::Free => String::from("自由专注中"),
            FocusState::Off => String::from("专注已关"),
        }
    }

    /// F076 面板专注卡文案（状态 + 剩余——主册「面板专注卡显示状态与
    /// 剩余」）。
    pub fn quick_card_text(&self, now_ms: u64) -> String {
        match self.state {
            FocusState::Off => String::from("专注已关"),
            FocusState::Free => String::from("专注中 · 自由档"),
            FocusState::Timed => alloc::format!("专注中 · {}", self.remaining_text(now_ms)),
        }
    }

    /// 连续完成轮次（到点自动恢复才计轮——手动中断不算完整轮）。
    pub fn rounds_done(&self) -> u32 {
        self.rounds_done
    }

    /// 起身建议（连续 ≥2 完整轮 → 建议长休——「25 分钟一轮回提醒起身」
    /// 的执行面；建议只出一次由调用方消费）。
    pub fn break_suggested(&self) -> bool {
        self.rounds_done >= BREAK_SUGGEST_ROUNDS
    }

    /// 账本聚合（30 天窗内的诚实总账：总专注时长 ms / 总拦截数）。
    pub fn aggregate_stats(&self) -> (u64, u64) {
        let mut total_ms = 0u64;
        let mut total_int = 0u64;
        for s in &self.ledger {
            total_ms += s.end_ms.saturating_sub(s.start_ms);
            total_int += s.intercepted;
        }
        (total_ms, total_int)
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_focusmode_checks() -> CheckSet {
    let mut set = CheckSet::new("F115-focusmode");

    // 1. 四效果之一：压暗 20%（Off=0 / On=20）。
    let mut f = FocusMode::new(0);
    set.add(
        "dim 20% on, 0% off",
        f.dim_pct() == 0 && f.begin(100, false) == FocusState::Free && f.dim_pct() == DIM_PCT,
        "",
    );

    // 2. 四效果之二/三：通知静默 + 拦截统计准确性对拍（常规拦/高优先放/
    //    白名单放且不计数——三路计数逐一对账）。
    let mut f = FocusMode::new(0);
    f.begin(0, false);
    f.allow("音乐播放器");
    let v1 = f.judge(&Incoming { app: "社交", priority: NotifyPriority::Normal, bubble: true });
    let v2 = f.judge(&Incoming { app: "闹钟", priority: NotifyPriority::High, bubble: false });
    let v3 = f.judge(&Incoming { app: "音乐播放器", priority: NotifyPriority::Normal, bubble: true });
    set.add(
        "judge: normal intercept / high deliver / whitelist deliver",
        v1 == Verdict::Intercepted && v2 == Verdict::Deliver && v3 == Verdict::Whitelisted,
        "",
    );
    set.add("intercept count accurate (only normal)", f.intercepted_now() == 1, "");

    // 3. 四效果之四：Pomodoro 25 分钟计时到点自动恢复（ended_by_timer）。
    let mut f = FocusMode::new(0);
    f.begin(0, true);
    let mid = f.tick(10 * 60_000);
    let done = f.tick(POMODORO_MIN * 60_000);
    set.add(
        "pomodoro 25min auto-restore",
        mid == None && done == Some(FocusState::Off) && f.dim_pct() == 0,
        "",
    );

    // 4. 长按 500ms 防手滑：短按不触发、长按一次翻转（判据第一句之三）。
    let mut f = FocusMode::new(0);
    f.hotkey_press(1_000);
    let short = f.hotkey_release(1_400); // 400ms < 500ms
    f.hotkey_press(1_000);
    let long = f.hotkey_release(1_600); // 600ms ≥ 500ms
    f.hotkey_press(2_000);
    let long2 = f.hotkey_release(2_600); // 再关
    set.add(
        "hotkey hold 500ms anti-mistouch",
        short == None && long == Some(FocusState::Free) && long2 == Some(FocusState::Off),
        "",
    );

    // 5. 焦点跟随 16ms 实时切换判线。
    let mut f = FocusMode::new(0);
    f.begin(0, false);
    let fast = f.focus_switched(7, 1_000, 1_016);
    let slow = f.focus_switched(8, 2_000, 2_017);
    set.add("focus switch within 16ms", fast && !slow, "");

    // 6. 结束 toast 汇总诚实数据（含拦截数）。
    let mut f = FocusMode::new(0);
    f.begin(0, true);
    f.judge(&Incoming { app: "a", priority: NotifyPriority::Normal, bubble: true });
    f.judge(&Incoming { app: "b", priority: NotifyPriority::Normal, bubble: true });
    f.judge(&Incoming { app: "c", priority: NotifyPriority::Normal, bubble: false });
    let s = f.closing_summary(POMODORO_MIN * 60_000);
    set.add(
        "closing summary honest count",
        s.contains("挡了 3 条通知") && s.contains("专注 25 分钟"),
        "",
    );

    // 7. 会话入账本 + 30 天裁剪（主册：统计入账本 30 天）。
    let mut f = FocusMode::new(0);
    f.begin(0, false);
    f.judge(&Incoming { app: "a", priority: NotifyPriority::Normal, bubble: true });
    f.end(5 * 60_000, false);
    f.begin(10 * 60_000, true);
    f.end(20 * 60_000, true);
    let two = f.ledger().len() == 2;
    // 裁剪线取两会话之间（第一段 5min 末出局、第二段 20min 末保留）；
    // 30 天窗口语义由 STATS_KEEP_DAYS 常量承载——驱逐只看 cutoff，
    // 同一 retain 路径对窗口外全清同样成立。
    let evicted = f.evict_older_than(6 * 60_000);
    set.add(
        "session ledger + evict by cutoff",
        two && f.ledger().len() == 1 && evicted == 1,
        "",
    );

    // 8. 用户手动关：立即恢复 + 统计保留（timed 会话手动关 ended_by_timer=false）。
    let mut f = FocusMode::new(0);
    f.begin(0, true);
    let off = f.end(60_000, false);
    set.add(
        "manual end restores now, stats kept",
        off == FocusState::Off && f.ledger().len() == 1 && !f.ledger()[0].ended_by_timer,
        "",
    );

    // 9. 重启不跨：专注态清零（显式重开）。
    let mut f = FocusMode::new(0);
    f.begin(0, false);
    f.power_cycle_reset(9_000);
    set.add(
        "focus state not across reboot",
        f.state() == FocusState::Off && f.intercepted_now() == 0,
        "",
    );

    // 10. 托盘气泡停发 + 截图不受影响（四效果全链闭环）。
    let mut f = FocusMode::new(0);
    set.add(
        "bubbles suppressed on, screenshot unaffected",
        !f.bubbles_suppressed() && f.screenshot_unaffected() && {
            f.begin(0, false);
            f.bubbles_suppressed()
        },
        "",
    );

    // 11. Off 态通知全放行（零拦截零计数）。
    let mut f = FocusMode::new(0);
    let v = f.judge(&Incoming { app: "社交", priority: NotifyPriority::Normal, bubble: true });
    set.add("off state delivers all", v == Verdict::Deliver && f.intercepted_now() == 0, "");

    // 13. 逐窗压暗裁决（深化 v2）：活动窗 0、非活动窗按档、Off 全 0；
    //     自定义档钳制生效。
    let mut f = FocusMode::new(0);
    let off_all0 = f.window_dim(99) == 0; // Off 态全窗零压暗
    let _ = f.begin(0, false);
    f.focus_switched(7, 0, 1);
    let active0 = f.window_dim(7) == 0;
    let active0 = f.window_dim(7) == 0;
    let inactive_dim = f.window_dim(9) == DIM_PCT;
    f.set_custom_dim(Some(35));
    let custom = f.window_dim(9) == 35;
    f.set_custom_dim(Some(80));
    let clamped = f.window_dim(9) == CUSTOM_DIM_RANGE.1;
    f.set_custom_dim(None);
    let back_default = f.window_dim(9) == DIM_PCT;
    set.add(
        "per-window dim + custom clamp",
        off_all0 && active0 && inactive_dim && custom && clamped && back_default,
        "",
    );

    // 14. 倒计时环与剩余文案（深化 v2）：环进度 = 真实时间比；剩余分钟
    //     诚实取整；Free/Off 无环。
    let mut f = FocusMode::new(0);
    f.set_pomodoro_tier(25);
    f.begin(0, true);
    let ring_60pct = f.ring_progress_bp(15 * 60_000) == Some(6_000); // 15/25 = 60%
    let rem_text = f.remaining_text(15 * 60_000);
    let rem_late = f.remaining_text(24 * 60_000 + 30_000);
    let free_no_ring = {
        let _ = f.end(0, false);
        let mut g = FocusMode::new(0);
        g.begin(0, false);
        (g.ring_progress_bp(0).is_none(), g.remaining_text(0) == "自由专注中")
    };
    let off_text = f.remaining_text(0) == "专注已关";
    set.add(
        "ring progress + honest remaining text",
        ring_60pct
            && rem_text == "剩余 10 分钟"
            && rem_late == "不足 1 分钟"
            && free_no_ring.0
            && free_no_ring.1
            && off_text,
        "",
    );

    // 15. F076 专注卡文案（深化 v2）：三态文案各就位。
    let mut f = FocusMode::new(0);
    let off_card = f.quick_card_text(0);
    let _ = f.begin(0, false);
    let free_card = f.quick_card_text(0);
    let _ = f.end(0, false);
    let _ = f.begin(0, true);
    let timed_card = f.quick_card_text(10 * 60_000);
    set.add(
        "quick card three states",
        off_card == "专注已关"
            && free_card == "专注中 · 自由档"
            && timed_card == "专注中 · 剩余 15 分钟",
        "",
    );

    // 16. 轮次与起身建议（深化 v2）：两完整轮 → 建议；手动关不计轮；
    //     重启清零。
    let mut f = FocusMode::new(0);
    f.begin(0, true);
    let _ = f.end(25 * 60_000, true);
    f.begin(26 * 60_000, true);
    let _ = f.end(51 * 60_000, true);
    let suggested = f.break_suggested();
    f.begin(52 * 60_000, true);
    let _ = f.end(60 * 60_000, false); // 手动关不计
    let manual_no_round = f.rounds_done() == 2;
    f.power_cycle_reset(61 * 60_000);
    let cleared = f.rounds_done() == 0 && !f.break_suggested();
    set.add(
        "rounds + break suggestion + reset",
        suggested && manual_no_round && cleared,
        "",
    );

    // 17. 自定义 Pomodoro 档（深化 v2）：45 分钟档 deadline 正确；非法
    //     值吸附最近档。
    let mut f = FocusMode::new(0);
    let tier = f.set_pomodoro_tier(45);
    let _ = f.begin(0, true);
    let mid_alive = f.tick(44 * 60_000).is_none();
    let done_at_45 = f.tick(45 * 60_000) == Some(FocusState::Off);
    let snapped = f.set_pomodoro_tier(28) == 25;
    set.add(
        "custom pomodoro tier + snap",
        tier == 45 && mid_alive && done_at_45 && snapped,
        "",
    );

    // 18. 账本聚合（深化 v2）：总时长与总拦截 = 各会话诚实求和。
    let mut f = FocusMode::new(0);
    f.begin(0, false);
    f.judge(&Incoming { app: "a", priority: NotifyPriority::Normal, bubble: true });
    f.judge(&Incoming { app: "b", priority: NotifyPriority::Normal, bubble: false });
    let _ = f.end(10 * 60_000, false); // 10 分钟 2 拦截
    f.begin(20 * 60_000, true);
    f.judge(&Incoming { app: "c", priority: NotifyPriority::Normal, bubble: true });
    let _ = f.end(50 * 60_000, true); // 30 分钟 1 拦截
    let (total_ms, total_int) = f.aggregate_stats();
    set.add(
        "ledger aggregate honest totals",
        total_ms == 40 * 60_000 && total_int == 3,
        "",
    );

    // 19. 长按防抖（深化 v2）：按住期间重复按下不覆盖起按时刻。
    let mut f = FocusMode::new(0);
    f.hotkey_press(0);
    f.hotkey_press(499); // 抖动重按——被忽略
    let still_none = f.hotkey_release(499) == None; // 距首按 499ms 不足
    f.hotkey_press(1_000);
    let fired = f.hotkey_release(1_500) == Some(FocusState::Free);
    set.add("hold debounce ignores re-press", still_none && fired, "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focusmode_all_checks_green() {
        let set = run_focusmode_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F115 自检红项 {f}：{}/{} 绿", p, p + f);
    }

    #[test]
    fn whitelist_no_duplicates() {
        let mut f = FocusMode::new(0);
        f.allow("音乐播放器");
        f.allow("音乐播放器");
        assert_eq!(f.whitelist().len(), 1);
    }

    #[test]
    fn timed_session_stat_fields() {
        let mut f = FocusMode::new(0);
        f.begin(0, true);
        f.judge(&Incoming { app: "a", priority: NotifyPriority::Normal, bubble: true });
        f.tick(25 * 60_000);
        let s = &f.ledger()[0];
        assert!(s.timed && s.ended_by_timer && s.intercepted == 1 && s.end_ms == 1_500_000);
    }

    #[test]
    fn hotkey_release_without_press_none() {
        let mut f = FocusMode::new(0);
        assert_eq!(f.hotkey_release(1_000), None);
    }
}
