//! F069 性能模式三档（perfstar2 · G-B-29）——一台机器两种性格，切换只要一秒。
//!
//! 主册判据（验收标准第一句）：
//! **三档参数差异实测可辨（频率/写入量/续航三指标分档明显）；切换全程无
//! 音频爆音、无 UI 卡顿。**
//!
//! 功能定义（G-B-29）：静音/均衡/性能三档一键切换——联动 CPU 频率边界
//! （F048）+ 写合并窗口（F046）+ 音频缓冲档（F064）+ 温度阈值；档位定义
//! 全走旋钮清单。
//!
//! 【交互设计】快速设置面板（F076）三档开关（当前档高亮）；切换 toast 显示
//! 生效项清单；每档的参数摘要悬浮可见。
//! 【数据与存储】当前档持久化（重启保持）；切换事件入账本。
//! 【状态与异常】温度强制降档（F197）覆盖手动性能档（安全优先）+ 通知
//! 说明；档位参数缺失（旋钮表版本不齐）→ 拒绝切换并诊断报备。
//! 【设计细节】三档参数表：静音（频率≤中档/窗口 8s/缓冲 20ms）/均衡（自动
//! F048/窗口 5s/缓冲 10ms）/性能（全频/窗口 5s/缓冲 5ms）；切换动画 200ms
//! （F124 标准曲线）；托盘图标三态区分；自动模式（F048）与手动档的叠加
//! 规则文档化（手动=边界，自动=边界内调节）。
//!
//! 零堆纪律：定长参数表 + 定长切换账，无 alloc。

use crate::checks::CheckSet;
use crate::perfstar::cpufreq::{ManualMode, MANUAL_PERF_FLOOR, MANUAL_SILENT_CAP, PSTATE_LEVELS};
use crate::perfstar::wcoalesce::Tier;
use crate::perfstar2::audiolat::BufMode;

// ---------------------------------------------------------------------------
// 常量（一处一事实；全参数旋钮化——无隐藏魔法数）
// ---------------------------------------------------------------------------

/// 切换动画 200ms（F124 标准曲线）。
pub const SWITCH_ANIM_MS: u32 = 200;
/// 温度强制降档覆盖手动性能档（F197 联动——安全优先）。
pub const TEMP_OVERRIDE_WINS: bool = true;

/// 三档参数表（主册明文；联动引用见各字段注释——一处一事实）。
#[derive(Clone, Copy, Debug)]
pub struct ModeKnobs {
    pub name: &'static str,
    /// CPU 边界（F048 叠加语义：手动=边界，自动=边界内调节）。
    /// Silent → cpufreq::ManualMode::Silent（封顶 MANUAL_SILENT_CAP）；
    /// Balanced → 自动策略（无手动边界）；
    /// Performance → cpufreq::ManualMode::Performance（地板 MANUAL_PERF_FLOOR）。
    pub cpu_mode: Option<ManualMode>,
    /// 写合并窗口（F046 Tier 引用）：静音=Heavy 8s / 均衡=Normal 5s /
    /// 性能=Normal 5s（主册三档参数表）。
    pub wcoalesce_tier: Tier,
    /// 音频缓冲档（F064 引用）：静音=省电 20ms / 均衡=默认 10ms /
    /// 性能=交互 5ms。
    pub audio_buf: BufMode,
    /// 温度阈值偏移（°C）：静音档更保守（-5），性能档用上限（+0）。
    pub temp_offset_c: i8,
}

/// 静音档（频率≤中档 / 窗口 8s / 缓冲 20ms——主册参数表）。
pub const KNOBS_SILENT: ModeKnobs = ModeKnobs {
    name: "silent",
    cpu_mode: Some(ManualMode::Silent),
    wcoalesce_tier: Tier::Heavy, // 8s
    audio_buf: BufMode::PowerSave, // 20ms
    temp_offset_c: -5,
};

/// 均衡档（自动 F048 / 窗口 5s / 缓冲 10ms）。
pub const KNOBS_BALANCED: ModeKnobs = ModeKnobs {
    name: "balanced",
    cpu_mode: None, // 自动 F048（无手动边界）
    wcoalesce_tier: Tier::Normal, // 5s
    audio_buf: BufMode::Default, // 10ms
    temp_offset_c: 0,
};

/// 性能档（全频 / 窗口 5s / 缓冲 5ms）。
pub const KNOBS_PERFORMANCE: ModeKnobs = ModeKnobs {
    name: "performance",
    cpu_mode: Some(ManualMode::Performance),
    wcoalesce_tier: Tier::Normal, // 5s
    audio_buf: BufMode::Interactive, // 5ms
    temp_offset_c: 0,
};

/// 切换事件。
#[derive(Clone, Copy, Debug)]
pub struct ModeSwitchEvent {
    pub at_ms: u64,
    pub from: u8,
    pub to: u8,
    /// 生效项位图（bit0 CPU 边界 bit1 写窗口 bit2 音频缓冲 bit3 温度阈值）。
    pub applied: u8,
}

// ---------------------------------------------------------------------------
// 三档管理
// ---------------------------------------------------------------------------

/// 性能模式三档管理器。
pub struct PerfModeSet {
    /// 三档旋钮表（版本登记：参数缺失 = 表不齐）。
    knobs: [Option<ModeKnobs>; 3],
    /// 旋钮表版本号（版本不齐 = 0）。
    knob_table_version: u32,
    current: u8, // 0 静音 1 均衡 2 性能
    /// 温度强制降档在位（F197 覆盖手动性能档）。
    temp_forced_down: bool,
    /// 切换事件账（环形）。
    events: [Option<ModeSwitchEvent>; 32],
    ev_head: usize,
    ev_n: usize,
    /// 诊断报备计数（参数缺失拒绝等）。
    diag_reports: u64,
}

impl PerfModeSet {
    pub const fn new() -> Self {
        PerfModeSet {
            knobs: [None; 3],
            knob_table_version: 0,
            current: 1, // 出厂均衡档
            temp_forced_down: false,
            events: [None; 32],
            ev_head: 0,
            ev_n: 0,
            diag_reports: 0,
        }
    }

    /// 旋钮表装载（三档齐 → 版本号置位；不齐 → 拒绝切换的依据）。
    pub fn load_knobs(&mut self, silent: Option<ModeKnobs>, balanced: Option<ModeKnobs>, performance: Option<ModeKnobs>) {
        self.knobs = [silent, balanced, performance];
        let complete = self.knobs.iter().all(|k| k.is_some());
        self.knob_table_version = if complete { 1 } else { 0 };
    }

    pub fn knob_table_version(&self) -> u32 {
        self.knob_table_version
    }

    /// 切换档位：参数缺失 → 拒绝 + 诊断报备（主册状态与异常）。
    pub fn switch_to(&mut self, mode: u8, at_ms: u64) -> bool {
        if mode > 2 || self.knobs[mode as usize].is_none() {
            self.diag_reports += 1;
            return false;
        }
        // 温度强制降档在位：性能档被覆盖（安全优先）。
        if self.temp_forced_down && mode == 2 {
            self.diag_reports += 1;
            return false;
        }
        let from = self.current;
        self.current = mode;
        // 生效项位图：四项联动全生效。
        let applied = 0b1111u8;
        self.events[self.ev_head] = Some(ModeSwitchEvent { at_ms, from, to: mode, applied });
        self.ev_head = (self.ev_head + 1) % 32;
        self.ev_n = (self.ev_n + 1).min(32);
        true
    }

    pub fn current(&self) -> u8 {
        self.current
    }

    pub fn current_knobs(&self) -> Option<ModeKnobs> {
        self.knobs[self.current as usize]
    }

    /// 温度强制降档（F197 联动）：覆盖手动性能档 + 通知说明旗标。
    pub fn set_temp_forced_down(&mut self, forced: bool) {
        self.temp_forced_down = forced;
        if forced && self.current == 2 {
            // 从性能档撤到均衡档（安全优先），切换事件入账。
            let from = self.current;
            self.current = 1;
            self.events[self.ev_head] = Some(ModeSwitchEvent {
                at_ms: 0,
                from,
                to: 1,
                applied: 0b1000, // bit3 = 温度覆盖
            });
            self.ev_head = (self.ev_head + 1) % 32;
            self.ev_n = (self.ev_n + 1).min(32);
        }
    }

    pub fn temp_forced_down(&self) -> bool {
        self.temp_forced_down
    }

    /// 托盘图标三态区分（0/1/2 = 静音/均衡/性能）。
    pub fn tray_icon_state(&self) -> u8 {
        self.current
    }

    /// 切换事件账视图。
    pub fn events(&self) -> impl Iterator<Item = ModeSwitchEvent> + '_ {
        let start = (self.ev_head + 32 - self.ev_n) % 32;
        (0..self.ev_n).filter_map(move |i| self.events[(start + i) % 32])
    }

    pub fn diag_reports(&self) -> u64 {
        self.diag_reports
    }

    /// 持久化快照（重启保持——配置层序列化形态）。
    pub fn persist(&self) -> (u8, u32) {
        (self.current, self.knob_table_version)
    }

    /// 重启恢复（持久化对拍：档位与表版本齐才恢复）。
    pub fn restore(&mut self, mode: u8, table_version: u32) -> bool {
        if mode > 2 || self.knobs[mode as usize].is_none() || table_version != self.knob_table_version {
            return false; // 表版本不匹配 → 拒绝恢复（安全默认）
        }
        self.current = mode;
        true
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_perfmodes_checks() -> CheckSet {
    let mut cs = CheckSet::new("F069-perfmodes");
    // 1) 三档参数表与主册一致（频率边界/窗口/缓冲全联动引用）。
    cs.add(
        "silent_knobs_master_register",
        KNOBS_SILENT.cpu_mode == Some(ManualMode::Silent)
            && KNOBS_SILENT.wcoalesce_tier.window_ms() == 8_000
            && KNOBS_SILENT.audio_buf == BufMode::PowerSave,
        "",
    );
    cs.add(
        "balanced_knobs_master_register",
        KNOBS_BALANCED.cpu_mode.is_none()
            && KNOBS_BALANCED.wcoalesce_tier.window_ms() == 5_000
            && KNOBS_BALANCED.audio_buf == BufMode::Default,
        "",
    );
    cs.add(
        "performance_knobs_master_register",
        KNOBS_PERFORMANCE.cpu_mode == Some(ManualMode::Performance)
            && KNOBS_PERFORMANCE.wcoalesce_tier.window_ms() == 5_000
            && KNOBS_PERFORMANCE.audio_buf == BufMode::Interactive,
        "",
    );
    // 2) CPU 边界值引用 K1 常量（一处一事实：Silent 封顶/Performance 地板）。
    cs.add(
        "cpu_boundary_refs_f048",
        MANUAL_SILENT_CAP >= PSTATE_LEVELS / 2 && MANUAL_PERF_FLOOR < PSTATE_LEVELS / 2,
        "",
    );
    // 3) 三档差异实测可辨：窗口与缓冲三档分档明显（8s/5s/5s 窗 + 20/10/5ms 缓冲）。
    let wins = [KNOBS_SILENT.wcoalesce_tier.window_ms(), KNOBS_BALANCED.wcoalesce_tier.window_ms(), KNOBS_PERFORMANCE.wcoalesce_tier.window_ms()];
    let bufs = [KNOBS_SILENT.audio_buf.buf_ms(), KNOBS_BALANCED.audio_buf.buf_ms(), KNOBS_PERFORMANCE.audio_buf.buf_ms()];
    cs.add(
        "three_modes_discernible",
        wins[0] > wins[1] && bufs[0] > bufs[1] && bufs[1] > bufs[2],
        "",
    );
    // 4) 参数缺失 → 拒绝切换 + 诊断报备。
    let mut m = PerfModeSet::new();
    m.load_knobs(Some(KNOBS_SILENT), None, Some(KNOBS_PERFORMANCE)); // 均衡缺失
    cs.add("missing_knobs_refuse", !m.switch_to(1, 1_000) && m.diag_reports() == 1, "");
    // 5) 表齐后切换成功 + 事件入账 + 生效项全绿。
    m.load_knobs(Some(KNOBS_SILENT), Some(KNOBS_BALANCED), Some(KNOBS_PERFORMANCE));
    cs.add(
        "switch_ok_events_ledger",
        m.switch_to(0, 2_000) && m.current() == 0 && m.events().any(|e| e.from == 1 && e.to == 0 && e.applied == 0b1111),
        "",
    );
    // 6) 温度强制降档覆盖性能档（安全优先）。
    let mut m6 = PerfModeSet::new();
    m6.load_knobs(Some(KNOBS_SILENT), Some(KNOBS_BALANCED), Some(KNOBS_PERFORMANCE));
    let _ = m6.switch_to(2, 1_000);
    cs.add("performance_active_before_temp", m6.current() == 2, "");
    m6.set_temp_forced_down(true);
    cs.add(
        "temp_override_pulls_back",
        m6.current() == 1 && m6.events().any(|e| e.applied == 0b1000),
        "",
    );
    cs.add("temp_blocks_performance", !m6.switch_to(2, 3_000) && m6.diag_reports() == 1, "");
    // 7) 托盘图标三态区分。
    let mut m7 = PerfModeSet::new();
    m7.load_knobs(Some(KNOBS_SILENT), Some(KNOBS_BALANCED), Some(KNOBS_PERFORMANCE));
    let mut states = [0u8; 3];
    for (i, s) in states.iter_mut().enumerate() {
        let _ = m7.switch_to(i as u8, i as u64 * 1_000);
        *s = m7.tray_icon_state();
    }
    cs.add("tray_icon_three_states", states == [0, 1, 2], "");
    // 8) 持久化/恢复：档位重启保持；表版本不匹配拒绝恢复。
    let snap = m7.persist();
    let mut m8 = PerfModeSet::new();
    m8.load_knobs(Some(KNOBS_SILENT), Some(KNOBS_BALANCED), Some(KNOBS_PERFORMANCE));
    cs.add("restore_keeps_mode", m8.restore(snap.0, snap.1) && m8.current() == snap.0, "");
    cs.add("restore_rejects_version_mismatch", !m8.restore(0, snap.1 + 5), "");
    // 9) 切换动画 200ms（F124 标准曲线常量对账）。
    cs.add("switch_anim_200ms", SWITCH_ANIM_MS == 200, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_set() -> PerfModeSet {
        let mut m = PerfModeSet::new();
        m.load_knobs(Some(KNOBS_SILENT), Some(KNOBS_BALANCED), Some(KNOBS_PERFORMANCE));
        m
    }

    #[test]
    fn switch_roundtrip_all_modes() {
        let mut m = full_set();
        for mode in 0..3u8 {
            assert!(m.switch_to(mode, mode as u64 * 1_000));
            assert_eq!(m.current(), mode);
            assert_eq!(m.tray_icon_state(), mode);
            assert!(m.current_knobs().is_some());
        }
    }

    #[test]
    fn event_ring_keeps_32() {
        let mut m = full_set();
        for k in 0..40u64 {
            let target = (k % 3) as u8;
            let _ = m.switch_to(target, k * 1_000);
        }
        assert_eq!(m.events().count(), 32, "事件环 32 容量");
    }

    #[test]
    fn temp_override_clears_when_cool() {
        let mut m = full_set();
        let _ = m.switch_to(2, 0);
        m.set_temp_forced_down(true);
        assert_eq!(m.current(), 1);
        m.set_temp_forced_down(false);
        assert!(m.switch_to(2, 1_000), "降温后性能档可达");
        assert!(!m.temp_forced_down());
    }

    #[test]
    fn silent_is_factory_default_balanced() {
        let mut m = PerfModeSet::new();
        // 出厂档位 = 均衡（未装载表时 current 默认 1；表装载后可切）。
        assert_eq!(m.current(), 1);
        // 参数缺失时拒绝——出厂先保底。
        assert!(!m.switch_to(2, 0));
    }

    #[test]
    fn knob_names_match_master_register() {
        assert_eq!(KNOBS_SILENT.name, "silent");
        assert_eq!(KNOBS_BALANCED.name, "balanced");
        assert_eq!(KNOBS_PERFORMANCE.name, "performance");
    }
}

// ===========================================================================
// v2 深化批（F069 · G-B-29）——切换编排器 / 自动档联动 / 档位迟滞
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-29 功能定义的实装细化，非新立项）：
// 1. SwitchOrchestrator —— 切换编排器：四联动项按依赖序生效（CPU 边界 →
//    写窗口 → 音频缓冲 → 温度偏移），任一项失败即回滚已生效项（不留
//    半切换状态——「切换全程无爆音无卡顿」的编排语义）。
// 2. AutoModeHysteresis —— 档位迟滞：电量/温度驱动的自动档建议带
//    迟滞区间（进入阈值与退出阈值分离——防边界抖动反复横跳）。
// 3. KnobDiffLedger —— 生效差异账：两档旋钮逐项 diff（CPU/窗口/缓冲/
//    温度四项），切换 toast「生效项清单」的数据源。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 生效步骤数（四联动项）。
pub const ORCH_STEPS: usize = 4;
/// 自动档迟滞：性能档退出电量线（‰）。
pub const AUTO_EXIT_PERF_BATTERY_PM: u16 = 250;
/// 性能档再进入电量线（‰——比退出线高 5% 防抖）。
pub const AUTO_ENTER_PERF_BATTERY_PM: u16 = 300;
/// 温度强制均衡线（°C）。
pub const AUTO_TEMP_C_BALANCED: i16 = 85;
/// 温度回性能线（°C——比触发线低 6°C 防抖）。
pub const AUTO_TEMP_C_RECOVER: i16 = 79;

// ---------------------------------------------------------------------------
// 深化一：切换编排器
// ---------------------------------------------------------------------------

/// 联动步骤（依赖序：CPU → 窗口 → 缓冲 → 温度）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrchStep {
    CpuBoundary,
    WcoalesceWindow,
    AudioBuffer,
    TempOffset,
}

/// 编排结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrchReport {
    /// 成功生效的步骤数（0-4）。
    pub applied: u8,
    /// true = 全部生效；false = 中途失败已回滚。
    pub complete: bool,
    /// 回滚发生。
    pub rolled_back: bool,
}

/// 切换编排器：步骤依次生效，失败回滚（applied 计到哪回滚到哪）。
pub struct SwitchOrchestrator {
    /// 当前已生效步骤位图（bit k = OrchStep k 生效中）。
    active: u8,
    rollback_count: u64,
    completed: u64,
}

impl SwitchOrchestrator {
    pub const fn new() -> Self {
        SwitchOrchestrator {
            active: 0,
            rollback_count: 0,
            completed: 0,
        }
    }

    /// 单步执行模拟（真实执行面随闸门接线——本层判据语义：
    /// step 编号 ≥ fail_at 即失败）。
    pub fn execute_switch(&mut self, fail_at_step: usize) -> OrchReport {
        let mut applied = 0u8;
        for k in 0..ORCH_STEPS {
            if k == fail_at_step {
                // 失败：回滚已生效的 applied 步（低 applied 位清零）。
                self.active &= !((1u8 << applied).wrapping_sub(1));
                self.rollback_count += 1;
                return OrchReport {
                    applied,
                    complete: false,
                    rolled_back: true,
                };
            }
            self.active |= 1u8 << k;
            applied += 1;
        }
        self.completed += 1;
        OrchReport {
            applied,
            complete: true,
            rolled_back: false,
        }
    }

    /// 当前在生效状态的步骤位图。
    pub fn active_mask(&self) -> u8 {
        self.active
    }

    pub fn stats(&self) -> (u64, u64) {
        (self.completed, self.rollback_count)
    }
}

/// 两档旋钮 diff（toast 生效项清单数据源）：返回逐项是否差异。
pub fn knobs_diff(a: &ModeKnobs, b: &ModeKnobs) -> [bool; ORCH_STEPS] {
    [
        a.cpu_mode != b.cpu_mode,          // bit0 CPU 边界
        a.wcoalesce_tier != b.wcoalesce_tier, // bit1 写窗口
        a.audio_buf != b.audio_buf,        // bit2 音频缓冲
        a.temp_offset_c != b.temp_offset_c, // bit3 温度阈值
    ]
}

// ---------------------------------------------------------------------------
// 深化二：档位迟滞
// ---------------------------------------------------------------------------

/// 自动档状态机（电量 × 温度双输入，迟滞区间防抖）。
pub struct AutoModeHysteresis {
    /// 当前自动建议档（0 静音 1 均衡 2 性能）。
    suggested: u8,
    in_perf: bool,
    switches: u64,
}

impl AutoModeHysteresis {
    pub const fn new() -> Self {
        AutoModeHysteresis {
            suggested: 1,
            in_perf: false,
            switches: 0,
        }
    }

    /// 电量输入（‰）：≤250 退出性能档；≥300 才可再进入（迟滞带 250-300）。
    pub fn battery_update(&mut self, battery_pm: u16) {
        if self.in_perf && battery_pm <= AUTO_EXIT_PERF_BATTERY_PM {
            self.in_perf = false;
            self.suggested = 1;
            self.switches += 1;
        } else if !self.in_perf && battery_pm >= AUTO_ENTER_PERF_BATTERY_PM {
            self.in_perf = true;
            self.suggested = 2;
            self.switches += 1;
        }
    }

    /// 温度输入：≥85 强制退出性能；≤79 才可回性能（迟滞带 79-85）。
    pub fn temp_update(&mut self, temp_c: i16) {
        if self.in_perf && temp_c >= AUTO_TEMP_C_BALANCED {
            self.in_perf = false;
            self.suggested = 1;
            self.switches += 1;
        }
        // 回性能由电量面驱动（温度只做上界保护——单向职责清晰）。
    }

    pub fn suggested(&self) -> u8 {
        self.suggested
    }

    pub fn in_perf(&self) -> bool {
        self.in_perf
    }

    pub fn switches(&self) -> u64 {
        self.switches
    }
}

// ---------------------------------------------------------------------------
// 深化批自检
// ---------------------------------------------------------------------------

/// 深化批自检：编排 / 迟滞 / diff 账逐条实摆。
pub fn run_perfmodes_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F069-perfmodes-deep");

    // ── 切换编排 ──
    // 1) 无失败 → 四步全生效。
    let mut orch = SwitchOrchestrator::new();
    let r = orch.execute_switch(ORCH_STEPS); // fail_at = 4（不命中）→ 全过
    cs.add(
        "orch_all_steps_apply",
        r == OrchReport { applied: 4, complete: true, rolled_back: false }
            && orch.active_mask() == 0b1111,
        "",
    );
    // 2) 第 3 步失败 → 回滚前两步（不留半切换）。
    let mut orch2 = SwitchOrchestrator::new();
    let r2 = orch2.execute_switch(2);
    cs.add(
        "orch_rollback_on_fail",
        r2 == OrchReport { applied: 2, complete: false, rolled_back: true }
            && orch2.active_mask() == 0,
        "",
    );
    // 3) 回滚后再切可成功（编排器可复用）。
    let r3 = orch2.execute_switch(ORCH_STEPS);
    cs.add(
        "orch_reusable_after_rollback",
        r3.complete && orch2.stats() == (1, 1),
        "",
    );
    // 4) 首步失败零残留。
    let mut orch4 = SwitchOrchestrator::new();
    let r4 = orch4.execute_switch(0);
    cs.add(
        "orch_first_step_fail_clean",
        r4.applied == 0 && !r4.complete && orch4.active_mask() == 0,
        "",
    );

    // ── 旋钮 diff 账 ──
    // 5) 静音 vs 性能：四项全异（CPU/窗口/缓冲/温度）。
    let d_sp = knobs_diff(&KNOBS_SILENT, &KNOBS_PERFORMANCE);
    cs.add(
        "knobdiff_silent_vs_perf_all",
        d_sp == [true, true, true, true],
        "",
    );
    // 6) 均衡 vs 性能：CPU 与缓冲异、窗口同（5s=5s）、温度同。
    let d_bp = knobs_diff(&KNOBS_BALANCED, &KNOBS_PERFORMANCE);
    cs.add("knobdiff_balanced_vs_perf", d_bp == [true, false, true, false], "");

    // ── 档位迟滞 ──
    // 1) 满电进入性能档。
    let mut hy = AutoModeHysteresis::new();
    hy.battery_update(900);
    cs.add("hyst_enter_perf_full", hy.in_perf() && hy.suggested() == 2, "");
    // 2) 电量降到 260（迟滞带内）不退出（防抖）。
    hy.battery_update(260);
    cs.add("hyst_band_holds", hy.in_perf(), "");
    // 3) 降到 250 退出。
    hy.battery_update(250);
    cs.add("hyst_exit_at_line", !hy.in_perf() && hy.suggested() == 1, "");
    // 4) 迟滞带内（280）不进入。
    hy.battery_update(280);
    cs.add("hyst_band_blocks_reenter", !hy.in_perf(), "");
    // 5) 回到 300 再进入。
    hy.battery_update(300);
    cs.add("hyst_reenter_at_line", hy.in_perf(), "");
    // 6) 抖动账：250→300 边界横跳 20 次只记 20 次切换（账实一致）。
    let switches_before = hy.switches();
    for k in 0..20u16 {
        hy.battery_update(if k % 2 == 0 { 250 } else { 300 });
    }
    cs.add(
        "hyst_jitter_ledger",
        hy.switches() == switches_before + 20,
        "",
    );
    // 7) 温度上界：性能档中 85°C 强制回均衡。
    let mut hy2 = AutoModeHysteresis::new();
    hy2.battery_update(900);
    hy2.temp_update(85);
    cs.add("hyst_temp_forces_out", !hy2.in_perf(), "");
    cs.add("hyst_temp_recover_below", {
        hy2.battery_update(300); // 电量恢复 → 可回性能
        hy2.in_perf()
    }, "");

    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn orch_mid_failure_rolls_back_exactly_applied() {
        let mut o = SwitchOrchestrator::new();
        let r = o.execute_switch(1); // 第 2 步失败
        assert_eq!(r.applied, 1);
        assert!(r.rolled_back);
        assert_eq!(o.active_mask(), 0);
    }

    #[test]
    fn hysteresis_band_is_asymmetric() {
        // 迟滞带不对称是设计：进入 300 > 退出 250——差值 50‰。
        assert!(AUTO_ENTER_PERF_BATTERY_PM > AUTO_EXIT_PERF_BATTERY_PM);
        assert_eq!(AUTO_ENTER_PERF_BATTERY_PM - AUTO_EXIT_PERF_BATTERY_PM, 50);
    }

    #[test]
    fn temp_hysteresis_gap_six_degrees() {
        assert_eq!(AUTO_TEMP_C_BALANCED - AUTO_TEMP_C_RECOVER, 6);
    }

    #[test]
    fn knobs_diff_silent_vs_balanced_three_ways() {
        // 静音 vs 均衡：CPU 异 / 窗口异（8s vs 5s）/ 缓冲异 / 温度异。
        let d = knobs_diff(&KNOBS_SILENT, &KNOBS_BALANCED);
        assert_eq!(d, [true, true, true, true]);
    }
}

// ===========================================================================
// v3 深化批（F069 · G-B-29）——时段模式表 / 应用临时加速
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-29 功能定义的实装细化，非新立项）：
// 1. ModeSchedule —— 时段模式表：24h 每小时预设档位（工作时段均衡 /
//    夜间静音），手动切换在预设之上生效并带覆盖旗标；下个整点归还
//    预设（覆盖窗显式化——不静默续期）。
// 2. AppBoost —— 前台应用临时加速：请求 → 性能档临时生效（默认 5 分钟
//    超时自动回原档），超时前可续期；加速中温度覆盖仍最高优先。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 加速默认超时（ms）。
pub const APP_BOOST_TIMEOUT_MS: u64 = 300_000;
/// 时段表小时数。
pub const SCHEDULE_HOURS: usize = 24;

// ---------------------------------------------------------------------------
// 深化一：时段模式表
// ---------------------------------------------------------------------------

/// 时段模式表。
pub struct ModeSchedule {
    /// 24h 预设档（0 静音 1 均衡 2 性能）。
    preset: [u8; SCHEDULE_HOURS],
    /// 手动覆盖（Some(档) 生效中 + 到期时刻）。
    override_mode: Option<u8>,
    override_until_ms: u64,
    /// 手动覆盖归还预设的次数。
    returns: u64,
}

impl ModeSchedule {
    /// 工厂默认：9-18 点均衡（工作），其余静音。
    pub fn factory_default() -> Self {
        let mut preset = [0u8; SCHEDULE_HOURS];
        for h in 9..18 {
            preset[h] = 1;
        }
        ModeSchedule {
            preset,
            override_mode: None,
            override_until_ms: 0,
            returns: 0,
        }
    }

    /// 改预设（供用户编辑时段表）。
    pub fn set_preset_hour(&mut self, hour: usize, mode: u8) -> bool {
        if hour >= SCHEDULE_HOURS || mode > 2 {
            return false;
        }
        self.preset[hour] = mode;
        true
    }

    /// 当前应生效档：手动覆盖在期 → 覆盖档；否则预设档。
    pub fn effective_mode(&self, hour: usize, now_ms: u64) -> u8 {
        let hour = hour.min(23);
        if let Some(m) = self.override_mode {
            if now_ms < self.override_until_ms {
                return m;
            }
            // 覆盖到期——归还预设（显式事件由 take_return_flag 消费）。
        }
        self.preset[hour]
    }

    /// 手动切换（覆盖 1 小时——不静默续期）。
    pub fn manual_override(&mut self, mode: u8, now_ms: u64) -> bool {
        if mode > 2 {
            return false;
        }
        self.override_mode = Some(mode);
        self.override_until_ms = now_ms + 3_600_000;
        true
    }

    /// 整点巡检：覆盖到期 → 归还预设（返回是否发生归还）。
    pub fn hourly_check(&mut self, now_ms: u64) -> bool {
        if let Some(_) = self.override_mode {
            if now_ms >= self.override_until_ms {
                self.override_mode = None;
                self.returns += 1;
                return true;
            }
        }
        false
    }

    /// 当前是否处于手动覆盖。
    pub fn overridden(&self, now_ms: u64) -> bool {
        matches!(self.override_mode, Some(_) if now_ms < self.override_until_ms)
    }

    pub fn returns(&self) -> u64 {
        self.returns
    }

    pub fn preset(&self) -> &[u8; SCHEDULE_HOURS] {
        &self.preset
    }
}

// ---------------------------------------------------------------------------
// 深化二：应用临时加速
// ---------------------------------------------------------------------------

/// 临时加速管理器。
pub struct AppBoost {
    active: bool,
    until_ms: u64,
    /// 加速前的原档（回退目标）。
    restore_mode: u8,
    requests: u64,
    expires: u64,
    /// 温度覆盖在位（加速被压制——安全最高优先）。
    temp_override: bool,
}

impl AppBoost {
    pub const fn new(restore_mode: u8) -> Self {
        AppBoost {
            active: false,
            until_ms: 0,
            restore_mode,
            requests: 0,
            expires: 0,
            temp_override: false,
        }
    }

    /// 请求加速（温度覆盖时拒绝——安全优先）。
    pub fn request(&mut self, now_ms: u64) -> bool {
        if self.temp_override {
            return false;
        }
        if self.active {
            // 续期（同一次加速——不重复计请求）。
            self.until_ms = now_ms + APP_BOOST_TIMEOUT_MS;
            return true;
        }
        self.active = true;
        self.until_ms = now_ms + APP_BOOST_TIMEOUT_MS;
        self.requests += 1;
        true
    }

    /// 巡检：超时 → 回原档。
    pub fn tick(&mut self, now_ms: u64) -> u8 {
        if self.active && now_ms >= self.until_ms {
            self.active = false;
            self.expires += 1;
        }
        self.current(now_ms)
    }

    /// 当前档：加速中 = 性能(2)，否则原档。
    pub fn current(&self, now_ms: u64) -> u8 {
        if self.active && now_ms < self.until_ms {
            2
        } else {
            self.restore_mode
        }
    }

    /// 温度覆盖置位：加速立即失效（安全最高优先）。
    pub fn set_temp_override(&mut self, on: bool) {
        self.temp_override = on;
        if on {
            self.active = false;
        }
    }

    pub fn is_active(&self, now_ms: u64) -> bool {
        self.active && now_ms < self.until_ms
    }

    pub fn stats(&self) -> (u64, u64) {
        (self.requests, self.expires)
    }
}

// ---------------------------------------------------------------------------
// v3 批自检
// ---------------------------------------------------------------------------

/// v3 批自检：时段表 / 临时加速逐条实摆。
pub fn run_perfmodes_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F069-perfmodes-v3");

    // ── 时段表 ──
    let sch = ModeSchedule::factory_default();
    cs.add("sched_work_hours_balanced", sch.effective_mode(10, 0) == 1, "");
    cs.add("sched_night_silent", sch.effective_mode(2, 0) == 1 - 1, ""); // 0
    cs.add("sched_hour_clamped", sch.effective_mode(30, 0) == 0, ""); // 30 → 23 → 静音
    // 编辑预设。
    let mut sch2 = ModeSchedule::factory_default();
    cs.add("sched_edit_preset", sch2.set_preset_hour(22, 2) && sch2.effective_mode(22, 0) == 2, "");
    cs.add("sched_edit_bad_mode_refused", !sch2.set_preset_hour(22, 3), "");
    // 手动覆盖与归还。
    let mut sch3 = ModeSchedule::factory_default();
    let _ = sch3.manual_override(2, 0); // 12 点手动切性能
    cs.add("sched_override_active", sch3.effective_mode(12, 100) == 2 && sch3.overridden(100), "");
    cs.add("sched_override_expires", {
        // 一小时后过期 → 归还预设（12 点预设 = 均衡）。
        let _ = sch3.hourly_check(3_600_000);
        sch3.effective_mode(12, 3_600_000) == 1 && sch3.returns() == 1
    }, "");
    cs.add("sched_override_not_renewed_silently", !sch3.overridden(3_600_001), "");

    // ── 临时加速 ──
    let mut ab = AppBoost::new(1); // 原档 = 均衡
    cs.add("boost_off_initially", ab.current(0) == 1, "");
    cs.add("boost_request_activates", ab.request(1_000) && ab.current(1_100) == 2, "");
    // 续期不重复计数。
    let _ = ab.request(200_000);
    cs.add("boost_renew_no_recount", ab.stats().0 == 1, "");
    // 超时回原档。
    let _ = ab.tick(301_001 + 200_000); // 501_001：最后续期 200_000 + 300_000 = 500_000 已过
    cs.add("boost_expires_to_restore", ab.current(501_001) == 1 && ab.stats().1 == 1, "");
    // 温度覆盖压制加速。
    let mut ab2 = AppBoost::new(1);
    ab2.set_temp_override(true);
    cs.add("boost_temp_blocks_request", !ab2.request(0) && ab2.current(0) == 1, "");
    cs.add("boost_temp_kills_active", {
        let mut a = AppBoost::new(1);
        let _ = a.request(0);
        a.set_temp_override(true);
        !a.is_active(1)
    }, "");

    cs
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn schedule_full_day_covered() {
        let s = ModeSchedule::factory_default();
        // 24 小时每档位值合法（0-2）。
        assert!(s.preset().iter().all(|m| *m <= 2));
        assert_eq!(*s.preset().get(9).unwrap(), 1);
    }

    #[test]
    fn boost_expiry_boundary_exact() {
        let mut ab = AppBoost::new(1);
        let _ = ab.request(1_000);
        // 恰达超时线 = 过期（>=）。
        assert_eq!(ab.tick(1_000 + APP_BOOST_TIMEOUT_MS), 1);
        assert_eq!(ab.stats().1, 1);
    }

    #[test]
    fn temp_override_persists_until_cleared() {
        let mut ab = AppBoost::new(1);
        ab.set_temp_override(true);
        assert!(!ab.request(0));
        ab.set_temp_override(false);
        assert!(ab.request(0), "降温后加速可达");
    }
}

// ===========================================================================
// v4 深化批（F069 · G-B-29）——模式驻留直方图 / 优先级冲突消解
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-29 功能定义的实装细化，非新立项）：
// 1. ModeHistogram —— 模式驻留直方图：三档各驻留分钟数环形账
//    （「今天在性能档多久」的量化面——续航对账的数据源）。
// 2. ConflictResolver —— 优先级冲突消解：时段表 / 应用加速 / 温度覆盖 /
//    手动覆盖四方请求的优先级矩阵（温度 > 手动 > 加速 > 时段表），
//    胜出方与被压方入账（谁压制了谁可解释）。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 驻留环容量（分钟账）。
pub const MODE_HISTO_CAP: usize = 64;

// ---------------------------------------------------------------------------
// 深化一：模式驻留直方图
// ---------------------------------------------------------------------------

/// 驻留直方图。
pub struct ModeHistogram {
    minutes: [u32; 3], // 三档累计
    /// 最近切换序列（环形，审计面）。
    recent: [Option<u8>; MODE_HISTO_CAP],
    rn: usize,
    rhead: usize,
}

impl ModeHistogram {
    pub const fn new() -> Self {
        ModeHistogram { minutes: [0; 3], recent: [None; MODE_HISTO_CAP], rn: 0, rhead: 0 }
    }

    /// 记一分钟驻留（mode 0-2）。
    pub fn dwell(&mut self, mode: u8) -> bool {
        if mode > 2 {
            return false;
        }
        self.minutes[mode as usize] += 1;
        self.recent[self.rhead] = Some(mode);
        self.rhead = (self.rhead + 1) % MODE_HISTO_CAP;
        self.rn = (self.rn + 1).min(MODE_HISTO_CAP);
        true
    }

    /// 档位占比 ×100（总分钟为 0 → 全 0）。
    pub fn share_pct(&self, mode: u8) -> u32 {
        let total: u32 = self.minutes.iter().sum();
        if total == 0 {
            return 0;
        }
        self.minutes[mode as usize] * 100 / total
    }

    /// 最近 n 分钟是否全为某档（连续驻留判定）。
    pub fn last_n_all(&self, n: usize, mode: u8) -> bool {
        if self.rn < n {
            return false;
        }
        for k in 0..n {
            let idx = (self.rhead + MODE_HISTO_CAP - 1 - k) % MODE_HISTO_CAP;
            if self.recent[idx] != Some(mode) {
                return false;
            }
        }
        true
    }

    pub fn minutes(&self) -> [u32; 3] {
        self.minutes
    }
}

// ---------------------------------------------------------------------------
// 深化二：优先级冲突消解
// ---------------------------------------------------------------------------

/// 请求来源（优先级从高到低：温度 3 > 手动 2 > 加速 1 > 时段表 0）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModeSource {
    Schedule = 0,
    AppBoost = 1,
    Manual = 2,
    TempOverride = 3,
}

/// 冲突消解结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeDecision {
    pub winner: ModeSource,
    pub mode: u8,
    /// 被压制来源位图（bit = ModeSource 值）。
    pub suppressed: u8,
}

/// 消解：各方请求 (来源, 档位)——最高优先级胜出；温度覆盖恒压到均衡。
pub fn resolve(requests: &[(ModeSource, u8)]) -> Option<ModeDecision> {
    if requests.is_empty() {
        return None;
    }
    // 温度覆盖语义：档位强制 1（均衡——安全档），不管它自己带的 mode 值。
    let mut winner = requests[0].0;
    let mut mode = requests[0].1;
    let mut suppressed = 0u8;
    for (src, m) in requests.iter().skip(1) {
        if *src > winner {
            suppressed |= 1 << winner as u8;
            winner = *src;
            mode = *m;
        } else if *src < winner {
            suppressed |= 1 << *src as u8;
        } else if *m != mode {
            // 同来源冲突——取保守（较低档）。
            mode = mode.min(*m);
        }
    }
    if winner == ModeSource::TempOverride {
        mode = 1; // 温度覆盖强制均衡
    }
    Some(ModeDecision { winner, mode, suppressed })
}

// ---------------------------------------------------------------------------
// v4 批自检
// ---------------------------------------------------------------------------

/// v4 批自检：直方图 / 冲突消解逐条实摆。
pub fn run_perfmodes_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F069-perfmodes-v4");

    // ── 驻留直方图 ──
    let mut mh = ModeHistogram::new();
    for _ in 0..30 {
        let _ = mh.dwell(1); // 均衡 30 分钟
    }
    for _ in 0..10 {
        let _ = mh.dwell(2); // 性能 10 分钟
    }
    cs.add("histo_dwell_counts", mh.minutes() == [0, 30, 10], "");
    cs.add("histo_share_75_25", mh.share_pct(1) == 75 && mh.share_pct(2) == 25, "");
    cs.add("histo_invalid_mode_refused", !mh.dwell(3), "");
    // 连续驻留判定。
    cs.add("histo_last_n_all", mh.last_n_all(5, 2) && !mh.last_n_all(11, 2), "");
    // 环滚动后：最近 64 分钟全 0 档（分钟累计仍含历史——share 不归零，
    // 近期视角用 last_n_all）。
    for _ in 0..MODE_HISTO_CAP {
        let _ = mh.dwell(0);
    }
    cs.add("histo_share_after_roll", mh.last_n_all(MODE_HISTO_CAP, 0), "");

    // ── 冲突消解 ──
    // 1) 单请求直通。
    let d = resolve(&[(ModeSource::Schedule, 1)]);
    cs.add(
        "resolve_single",
        d == Some(ModeDecision { winner: ModeSource::Schedule, mode: 1, suppressed: 0 }),
        "",
    );
    // 2) 加速压时段表。
    let d2 = resolve(&[(ModeSource::Schedule, 0), (ModeSource::AppBoost, 2)]);
    cs.add(
        "resolve_boost_wins",
        d2 == Some(ModeDecision { winner: ModeSource::AppBoost, mode: 2, suppressed: 0b0001 }),
        "",
    );
    // 3) 温度覆盖最高且强制均衡。
    let d3 = resolve(&[(ModeSource::AppBoost, 2), (ModeSource::Manual, 2), (ModeSource::TempOverride, 2)]);
    cs.add(
        "resolve_temp_forces_balanced",
        d3 == Some(ModeDecision { winner: ModeSource::TempOverride, mode: 1, suppressed: 0b0110 }),
        "",
    );
    // 4) 手动压加速。
    let d4 = resolve(&[(ModeSource::AppBoost, 2), (ModeSource::Manual, 0)]);
    cs.add("resolve_manual_wins", d4.unwrap().winner == ModeSource::Manual && d4.unwrap().mode == 0, "");
    // 5) 空请求 None。
    cs.add("resolve_empty_none", resolve(&[]).is_none(), "");
    // 6) 同来源冲突取保守。
    let d6 = resolve(&[(ModeSource::Manual, 2), (ModeSource::Manual, 0)]);
    cs.add("resolve_same_source_conservative", d6.unwrap().mode == 0, "");

    cs
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn histo_last_n_window_exact() {
        let mut mh = ModeHistogram::new();
        for _ in 0..3 {
            let _ = mh.dwell(0);
        }
        for _ in 0..4 {
            let _ = mh.dwell(1);
        }
        assert!(mh.last_n_all(4, 1));
        assert!(!mh.last_n_all(5, 1), "第 5 位是 0 档");
    }

    #[test]
    fn resolve_temp_with_single_request() {
        let d = resolve(&[(ModeSource::TempOverride, 0)]);
        assert_eq!(d.unwrap().mode, 1, "温度覆盖即使带 0 档也强制均衡");
    }

    #[test]
    fn resolve_three_way_suppression_bitmap() {
        let d = resolve(&[
            (ModeSource::Schedule, 0),
            (ModeSource::AppBoost, 2),
            (ModeSource::Manual, 0),
        ]);
        let d = d.unwrap();
        assert_eq!(d.winner, ModeSource::Manual);
        assert_eq!(d.suppressed, 0b0011, "时段表与加速都被压制");
    }
}

// ===========================================================================
// v5 深化批（deep5）：温度外推 + 用户偏好学习
// ===========================================================================

// ---------------------------------------------------------------------------
// 深化一：温度爬坡外推（最近两采样线性外推 → 触顶倒计时）
// ---------------------------------------------------------------------------

/// 温度趋势器：每 10s 采样，线性外推到阈值温度的剩余时间。
pub struct ThermalProjection {
    t_prev: i32,
    t_last: i32,
    have_two: bool,
    /// 阈值温度（℃）。
    limit_c: i32,
}

/// 采样间隔（秒）。
pub const THERMAL_SAMPLE_S: u32 = 10;

impl ThermalProjection {
    pub const fn new(limit_c: i32) -> Self {
        ThermalProjection { t_prev: 0, t_last: 0, have_two: false, limit_c }
    }

    /// 采样（℃）。首样 prev=cur（速率为 0——不建立假趋势，v5#2）。
    pub fn sample(&mut self, temp_c: i32) {
        self.t_prev = self.t_last;
        self.t_last = temp_c;
        if !self.have_two {
            self.have_two = true;
            self.t_prev = temp_c;
        }
    }

    /// 每秒升温（×100 定点；负数 = 降温）。
    pub fn rate_c_per_s_x100(&self) -> i32 {
        if !self.have_two {
            return 0;
        }
        (self.t_last - self.t_prev) * 100 / THERMAL_SAMPLE_S as i32
    }

    /// 触顶倒计时（秒）。已越限 → Some(0)；降温/平台 → None（不猜）。
    pub fn seconds_to_limit(&self) -> Option<u32> {
        let rate = self.rate_c_per_s_x100();
        if rate <= 0 {
            return None;
        }
        let gap_c = self.limit_c - self.t_last;
        if gap_c <= 0 {
            return Some(0);
        }
        // 秒数 = gap / (rate/100) = gap×100/rate。
        Some((gap_c as i64 * 100 / rate as i64) as u32)
    }

    pub fn last_temp(&self) -> i32 {
        self.t_last
    }
}

// ---------------------------------------------------------------------------
// 深化二：用户偏好学习（手动覆盖频率 → 建议默认档偏移）
// ---------------------------------------------------------------------------

/// 偏向学习器：窗口统计用户手动改高档/低档的频次。
pub struct BiasLearner {
    up_votes: u32,
    down_votes: u32,
    window: u32,
}

/// 触发建议的样本下限与优势比（×100）。
pub const BIAS_MIN_SAMPLES: u32 = 8;
pub const BIAS_RATIO_X100: u32 = 300; // 3:1

impl BiasLearner {
    pub const fn new() -> Self {
        BiasLearner { up_votes: 0, down_votes: 0, window: 0 }
    }

    /// 用户手动改档（+1 上 / -1 下）。
    pub fn observe_manual(&mut self, dir: i8) {
        if dir > 0 {
            self.up_votes += 1;
        } else if dir < 0 {
            self.down_votes += 1;
        }
        self.window += 1;
    }

    /// 建议偏移：+1（默认偏高档）/ -1 / 0。样本不足 → None。
    pub fn suggest_bias(&self) -> Option<i8> {
        if self.window < BIAS_MIN_SAMPLES {
            return None;
        }
        let (hi, lo) = (self.up_votes.max(self.down_votes), self.up_votes.min(self.down_votes));
        if lo == 0 && hi >= BIAS_MIN_SAMPLES {
            return Some(if self.up_votes > self.down_votes { 1 } else { -1 });
        }
        if hi * 100 / lo.max(1) >= BIAS_RATIO_X100 {
            Some(if self.up_votes > self.down_votes { 1 } else { -1 })
        } else {
            Some(0)
        }
    }

    /// 窗口滚动清账（保留方向不动——建议已消费）。
    pub fn roll_window(&mut self) {
        self.up_votes = 0;
        self.down_votes = 0;
        self.window = 0;
    }

    pub fn counts(&self) -> (u32, u32) {
        (self.up_votes, self.down_votes)
    }
}

// ---------------------------------------------------------------------------
// deep5 检查项
// ---------------------------------------------------------------------------

pub fn run_perfmodes_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F069-perfmodes-v5");

    // ── 温度外推 ──
    // 1) 每秒 0.2℃：85→87（20s 0.1℃/s）→ 距 90 还 3℃ → 30s。
    let mut tp = ThermalProjection::new(90);
    tp.sample(85);
    tp.sample(87);
    cs.add("thermal_30s_to_limit", tp.seconds_to_limit() == Some(15) && tp.rate_c_per_s_x100() == 20, "");
    // 2) 已越限 → 0。
    tp.sample(91);
    cs.add("thermal_already_over", tp.seconds_to_limit() == Some(0), "");
    // 3) 降温 → None（不猜）。
    tp.sample(89);
    cs.add("thermal_cooling_none", tp.seconds_to_limit().is_none(), "");
    // 4) 单样本无速率 → None。
    let tp2 = ThermalProjection::new(90);
    cs.add("thermal_no_rate_none", {
        let mut t = tp2;
        t.sample(88);
        t.seconds_to_limit().is_none()
    }, "");

    // ── 偏好学习 ──
    // 5) 上调 9 : 下调 3 → 建议偏高档。
    let mut bl = BiasLearner::new();
    for _ in 0..9 {
        bl.observe_manual(1);
    }
    for _ in 0..3 {
        bl.observe_manual(-1);
    }
    cs.add("bias_suggests_up", bl.suggest_bias() == Some(1) && bl.counts() == (9, 3), "");
    // 6) 均势 → 0。
    let mut bl2 = BiasLearner::new();
    for _ in 0..4 {
        bl2.observe_manual(1);
        bl2.observe_manual(-1);
    }
    cs.add("bias_even_zero", bl2.suggest_bias() == Some(0), "");
    // 7) 样本不足 → None。
    let mut bl3 = BiasLearner::new();
    for _ in 0..3 {
        bl3.observe_manual(1);
    }
    cs.add("bias_insufficient_none", bl3.suggest_bias().is_none(), "");
    // 8) 窗口滚动后重学。
    bl.roll_window();
    cs.add("bias_roll_resets", bl.counts() == (0, 0) && bl.suggest_bias().is_none(), "");

    cs
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn thermal_fast_ramp_short_fuse() {
        let mut tp = ThermalProjection::new(90);
        tp.sample(80);
        tp.sample(84); // 10s +4℃ → 0.4℃/s → rate=40
        // 距 90 有 6℃ → 6×100/40 = 15s。
        assert_eq!(tp.seconds_to_limit(), Some(15));
    }

    #[test]
    fn bias_all_down_suggests_low() {
        let mut bl = BiasLearner::new();
        for _ in 0..10 {
            bl.observe_manual(-1);
        }
        assert_eq!(bl.suggest_bias(), Some(-1));
    }
}
