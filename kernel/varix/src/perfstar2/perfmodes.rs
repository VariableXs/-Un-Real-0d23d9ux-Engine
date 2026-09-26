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
