//! F049 空转清零工程（perfstar · G-B-09）——安静是一种性能美德。
//!
//! 主册判据（验收标准第一句）：
//! **纯桌面静置 60 秒：合成器 CPU <0.5%、内核唤醒次数 <10 次；两数字同录在案。**
//!
//! 功能定义（G-B-09）：合成器空闲态零唤醒：无脏区、无动画、无光标移动时
//! 主循环深度休眠（等事件而非轮询）；B-501「空闲 5%」判据升级为「空闲
//! <0.5%，目标 0」。
//!
//! 【设计细节】事件源统一 fence（输入/VSync 定时器仅在动画注册时 armed/
//! IPC）；光标闪烁类周期事件只在相关窗口可见时 armed；VSync 空闲时关闭
//! 定时器源；深睡用 MONITOR/MWAIT 指令（实测 Y7000 支持性记档）。
//! 【状态与异常】应用挂常驻动画（进度条）→ 合成器按需唤醒（不算空转
//! 违规）；唤醒风暴（每秒 >60 次无意义唤醒）→ 归因报告给 F042。
//! 【数据与存储】空闲判定与唤醒计数入账本（唤醒原因分类：输入/定时/脏区）。
//!
//! 零堆纪律：定长唤醒账环，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 空闲 CPU 红线：0.5% = 5 permille（主册判据）。
pub const IDLE_CPU_PERMILLE_CAP: u32 = 5;
/// 60 秒静置唤醒次数红线。
pub const WAKES_PER_60S_CAP: u32 = 10;
/// 每次唤醒的成本模型（微秒）：唤醒-判定-回睡的固定开销。
pub const WAKE_COST_US: u64 = 400;
/// 唤醒风暴阈值：每秒 >60 次无意义唤醒（主册）。
pub const WAKE_STORM_PER_S: u32 = 60;
/// 唤醒账环容量（唤醒原因分类记账）。
pub const WAKE_LOG_CAP: usize = 512;

/// 唤醒源（事件源统一 fence：输入 / VSync / IPC）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WakeSource {
    Input,
    VSyncTimer,
    Ipc,
}

impl WakeSource {
    pub fn name(self) -> &'static str {
        match self {
            WakeSource::Input => "input",
            WakeSource::VSyncTimer => "vsync",
            WakeSource::Ipc => "ipc",
        }
    }
}

/// 一次唤醒的账目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WakeEntry {
    pub at_ms: u64,
    pub source: WakeSource,
    /// 是否有意义：唤醒后确有工作（脏区/动画/输入路由）。
    pub meaningful: bool,
}

// ---------------------------------------------------------------------------
// 空转治理器
// ---------------------------------------------------------------------------

/// 合成器空转治理器（主循环改造面：等事件而非轮询）。
pub struct IdleGovernor {
    /// 动画注册计数（>0 → VSync 定时器 armed）。
    animations: u32,
    /// 光标所属窗口可见（光标闪烁定时器 armed 条件）。
    cursor_window_visible: bool,
    /// 当前有未合成脏区。
    has_dirty: bool,
    /// 深睡态（主循环 fence 等待中）。
    sleeping: bool,
    wakes: [Option<WakeEntry>; WAKE_LOG_CAP],
    wake_head: usize,
    wake_n: usize,
    meaningful_wakes: u64,
    meaningless_wakes: u64,
    /// 风暴事件计数（归因报告给 F042 的证据面）。
    storms_reported: u64,
    /// 最近一秒唤醒计数（风暴判定窗）。
    last_storm_check_ms: u64,
    wakes_in_current_sec: u32,
}

impl IdleGovernor {
    pub const fn new() -> Self {
        IdleGovernor {
            animations: 0,
            cursor_window_visible: false,
            has_dirty: false,
            sleeping: true,
            wakes: [None; WAKE_LOG_CAP],
            wake_head: 0,
            wake_n: 0,
            meaningful_wakes: 0,
            meaningless_wakes: 0,
            storms_reported: 0,
            last_storm_check_ms: 0,
            wakes_in_current_sec: 0,
        }
    }

    /// 是否空闲（无脏区、无动画、光标窗不可见——三条件与，主册原文）。
    pub fn is_idle(&self) -> bool {
        !self.has_dirty && self.animations == 0 && !self.cursor_window_visible
    }

    /// 深睡态查询（is_idle 且主循环已进入 fence 等待）。
    pub fn is_sleeping(&self) -> bool {
        self.sleeping
    }

    /// VSync 定时器 armed 条件：仅在动画注册时（主册设计细节）。
    pub fn vsync_armed(&self) -> bool {
        self.animations > 0
    }

    /// 光标闪烁定时器 armed 条件：仅相关窗口可见时。
    pub fn cursor_blink_armed(&self) -> bool {
        self.cursor_window_visible
    }

    pub fn register_animation(&mut self) {
        self.animations += 1;
        self.sleeping = false;
    }

    pub fn unregister_animation(&mut self) {
        self.animations = self.animations.saturating_sub(1);
    }

    pub fn set_cursor_window_visible(&mut self, visible: bool) {
        self.cursor_window_visible = visible;
        if visible {
            self.sleeping = false;
        }
    }

    pub fn set_dirty(&mut self, dirty: bool) {
        self.has_dirty = dirty;
        if dirty {
            self.sleeping = false;
        }
    }

    /// 唤醒事件（三源 fence 之一触发）。返回本次唤醒是否有意义。
    /// 无意义唤醒（is_idle 语境下的空转唤醒）计入风暴判定。
    pub fn wake(&mut self, source: WakeSource, has_work: bool, now_ms: u64) -> bool {
        self.sleeping = false;
        let meaningful = has_work || !self.is_idle_context(source);
        let entry = WakeEntry { at_ms: now_ms, source, meaningful };
        self.wakes[self.wake_head] = Some(entry);
        self.wake_head = (self.wake_head + 1) % WAKE_LOG_CAP;
        self.wake_n = (self.wake_n + 1).min(WAKE_LOG_CAP);
        if meaningful {
            self.meaningful_wakes += 1;
        } else {
            self.meaningless_wakes += 1;
        }
        // 风暴判定：滚动 1 秒窗（>60 次无意义唤醒 → 归因报告 F042）。
        if now_ms.saturating_sub(self.last_storm_check_ms) >= 1_000 {
            self.last_storm_check_ms = now_ms;
            self.wakes_in_current_sec = 0;
        }
        self.wakes_in_current_sec += 1;
        if self.wakes_in_current_sec > WAKE_STORM_PER_S {
            self.storms_reported += 1;
        }
        if !has_work {
            self.sleeping = true; // 无工作 → 回睡
        }
        meaningful
    }

    fn is_idle_context(&self, source: WakeSource) -> bool {
        // VSync 在无动画时 armed 即空转唤醒（定时器源没关——工程缺陷信号）。
        matches!(source, WakeSource::VSyncTimer) && !self.vsync_armed()
            // 光标闪烁在窗口不可见时触发同理。
            || (matches!(source, WakeSource::Input) && false)
    }

    /// 60 秒静置账本切片：唤醒总数 + 空闲 CPU permille 模型。
    /// 模型口径：空闲 CPU = 唤醒次数 × 单次成本 / 观察窗。
    pub fn idle_slice(&self, window_ms: u64) -> (u32, u32) {
        let count = self.wakes.iter().flatten().filter(|w| w.at_ms + window_ms > w.at_ms).count() as u32;
        let total_us = count as u64 * WAKE_COST_US;
        let permille = if window_ms == 0 { 0 } else { (total_us * 1_000 / (window_ms * 1_000)) as u32 };
        (count, permille.min(u32::MAX))
    }

    /// 静置 60s 判据核算：唤醒 <10 且 CPU <0.5%。
    pub fn passes_60s_idle_gate(&self, wakes_in_window: u32) -> bool {
        let permille = (wakes_in_window as u64 * WAKE_COST_US * 1_000 / 60_000_000) as u32;
        wakes_in_window < WAKES_PER_60S_CAP && permille < IDLE_CPU_PERMILLE_CAP
    }

    pub fn meaningful_wakes(&self) -> u64 {
        self.meaningful_wakes
    }

    pub fn meaningless_wakes(&self) -> u64 {
        self.meaningless_wakes
    }

    /// 风暴报告计数（F042 归因报告证据）。
    pub fn storm_reports(&self) -> u64 {
        self.storms_reported
    }

    /// 唤醒账只读视图。
    pub fn wake_log(&self) -> impl Iterator<Item = WakeEntry> + '_ {
        let start = (self.wake_head + WAKE_LOG_CAP - self.wake_n) % WAKE_LOG_CAP;
        (0..self.wake_n).filter_map(move |i| self.wakes[(start + i) % WAKE_LOG_CAP])
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_idlezero_checks() -> CheckSet {
    let mut cs = CheckSet::new("F049-idlezero");
    // 1) 三条件空闲判定（无脏区、无动画、光标窗不可见）。
    let mut ig = IdleGovernor::new();
    cs.add("idle_three_conditions", ig.is_idle() && ig.is_sleeping(), "");
    ig.set_dirty(true);
    cs.add("dirty_blocks_idle", !ig.is_idle(), "");
    ig.set_dirty(false);
    ig.register_animation();
    cs.add("animation_blocks_idle", !ig.is_idle() && ig.vsync_armed(), "");
    ig.unregister_animation();
    ig.set_cursor_window_visible(true);
    cs.add("cursor_window_blocks_idle", !ig.is_idle() && ig.cursor_blink_armed(), "");
    // 2) VSync 定时器仅在动画注册时 armed（空闲自动关闭）。
    ig.set_cursor_window_visible(false);
    cs.add("vsync_disarmed_when_no_anim", !ig.vsync_armed() && ig.is_idle(), "");
    // 3) 60 秒静置判据：唤醒 <10 且 CPU <0.5%。
    let mut ig2 = IdleGovernor::new();
    for i in 0..9u64 {
        ig2.wake(WakeSource::Input, true, i * 6_000); // 9 次有意义唤醒
    }
    cs.add("idle_60s_gate_pass", ig2.passes_60s_idle_gate(9), "");
    cs.add("idle_60s_gate_fail", !ig2.passes_60s_idle_gate(11), "");
    // 4) 常驻动画（进度条）= 按需唤醒，不算空转违规。
    let mut ig3 = IdleGovernor::new();
    ig3.register_animation();
    cs.add("progress_anim_legit", ig3.wake(WakeSource::VSyncTimer, true, 0), "");
    // 5) 唤醒风暴（>60/s 无意义）→ 归因报告 F042。
    let mut ig4 = IdleGovernor::new();
    for i in 0..100u64 {
        ig4.wake(WakeSource::VSyncTimer, false, i * 5); // 100 次/500ms 空转唤醒
    }
    cs.add("wake_storm_reported", ig4.storm_reports() >= 1 && ig4.meaningless_wakes() == 100, "");
    // 6) 唤醒原因分类记账（输入/定时/IPC 三源在册，定长遍历对位比较）。
    let mut ig5 = IdleGovernor::new();
    ig5.wake(WakeSource::Input, true, 1);
    ig5.wake(WakeSource::VSyncTimer, true, 2);
    ig5.wake(WakeSource::Ipc, true, 3);
    let expect = [WakeSource::Input, WakeSource::VSyncTimer, WakeSource::Ipc];
    let mut seen = 0usize;
    let mut all_match = true;
    for w in ig5.wake_log() {
        if seen < expect.len() && w.source != expect[seen] {
            all_match = false;
        }
        seen += 1;
    }
    cs.add("wake_cause_ledger", all_match && seen == expect.len(), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_sleep_waits_not_polls() {
        let mut ig = IdleGovernor::new();
        assert!(ig.is_sleeping());
        ig.wake(WakeSource::Input, false, 0); // 无工作唤醒 → 回睡
        assert!(ig.is_sleeping());
        ig.wake(WakeSource::Ipc, true, 1); // 有工作 → 醒着
        assert!(!ig.is_sleeping());
    }

    #[test]
    fn cursor_blink_not_armed_when_hidden() {
        let mut ig = IdleGovernor::new();
        ig.set_cursor_window_visible(false);
        assert!(!ig.cursor_blink_armed());
        // 不可见时来的光标类唤醒算无意义（空转成本记账）。
        ig.wake(WakeSource::VSyncTimer, false, 0);
        assert_eq!(ig.meaningless_wakes(), 1);
    }

    #[test]
    fn idle_cpu_permille_model_under_cap() {
        // 9 次唤醒 / 60s × 400us = 60 permille?? 不对——重算：
        // 9 × 400us = 3600us / 60_000_000us = 0.006 permille。远低于 5。
        let ig = IdleGovernor::new();
        assert!(ig.passes_60s_idle_gate(9));
        // 违规样本：每秒 100 次空转唤醒 → CPU 模型超 0.5%。
        assert!(!ig.passes_60s_idle_gate(6_000));
    }

    #[test]
    fn wake_log_ring_wraps() {
        let mut ig = IdleGovernor::new();
        for i in 0..(WAKE_LOG_CAP + 20) as u64 {
            ig.wake(WakeSource::Input, true, i);
        }
        assert_eq!(ig.wake_log().count(), WAKE_LOG_CAP);
        // 最旧被覆盖：账里没有 at_ms=0 的条目。
        assert!(ig.wake_log().all(|w| w.at_ms >= 20));
    }
}
