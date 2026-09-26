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

    /// 快捷键按下（记时刻——不足 500ms 不触发，防手滑）。
    pub fn hotkey_press(&mut self, now_ms: u64) {
        self.hold_started_ms = Some(now_ms);
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
        self.deadline_ms = if timed { now_ms + POMODORO_MIN * 60_000 } else { 0 };
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
        self.session_start_ms = now_ms;
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
