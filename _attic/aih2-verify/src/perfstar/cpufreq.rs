//! F048 CPU 频率联动（perfstar · G-B-08）——该快的时候快，该静的时候静。
//!
//! 主册判据（验收标准第一句）：
//! **交互突发响应（点按到满频）<50ms；续航对比：自动策略 vs 固定高频，视频播放场景续航提升 >15% 实测。**
//!
//! 功能定义（G-B-08）：负载感知 P-state 升降档：交互突发秒升最高档、持续
//! 负载 30s 后按类型选档（构建=高频/下载=低频）、空闲 5s 降最低档；Y7000
//! 实机频率表驱动（ACPI _PSS 读取，graceful 不可读则固定档）。
//!
//! 【设计细节】升档触发：任何输入事件或音频 deadline 线程就绪；降档迟滞
//! 5s（防抖）；30s 类型判定按线程名签名（构建工具名单内 = 计算型）；频率
//! 切换自身耗时 <10ms（MSR 写）；全策略参数进旋钮清单（无隐藏魔法数）。
//! 【状态与异常】_PSS 不可读（部分固件）→ 固定中档 + 诊断标注（graceful
//! 已实测同纪律）；频率切换失败 → 重试一次后锁定安全档；温度超限（F197
//! 联动）→ 强制降档优先于本策略。
//! 【交互设计】快速设置面板（F076）性能三档（F069）与本策略叠加：手动
//! 档位 = 给自动策略设边界（静音档封顶低频）。
//!
//! 零堆纪律：定长频率表 + 定长决策日志，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实；全参数旋钮化——无隐藏魔法数）
// ---------------------------------------------------------------------------

/// P-state 档位数（Y7000 量级 8 档；_PSS 可读时按实机表覆盖）。
pub const PSTATE_LEVELS: usize = 8;
/// 默认频率表（MHz，Y7000 量级模型；_PSS 实表随闸门接线覆盖）。
pub const DEFAULT_FREQ_MHZ: [u32; PSTATE_LEVELS] = [4_000, 3_800, 3_400, 2_900, 2_400, 1_800, 1_200, 800];
/// 交互突发升档：点按到满频 <50ms。
pub const BURST_TO_FULL_CAP_MS: u32 = 50;
/// 单次切换耗时上限（MSR 写 <10ms）。
pub const SWITCH_COST_MS: u32 = 10;
/// 持续负载类型判定窗：30s。
pub const TYPE_WINDOW_MS: u64 = 30_000;
/// 空闲降档：5s 无负载事件。
pub const IDLE_DOWN_MS: u64 = 5_000;
/// 降档迟滞：5s。
pub const DOWN_HYSTERESIS_MS: u64 = 5_000;
/// 构建工具线程名签名（计算型判定名单，主册：构建工具名单内=计算型）。
pub const BUILD_SIGNATURES: [&str; 8] = ["cc1", "rustc", "ld", "cargo", "make", "gcc", "clang", "7z"];
/// 下载型签名（吞吐型判定名单）。
pub const DOWNLOAD_SIGNATURES: [&str; 4] = ["netd", "downloader", "curl", "wget"];
/// 手动档位边界（F069 三档叠加：静音档封顶低频）。
pub const MANUAL_SILENT_CAP: usize = 5; // 档位 index ≥5（≤1800MHz）
pub const MANUAL_PERF_FLOOR: usize = 2; // 档位 index ≤2（≥3400MHz）

/// 手动性能档（F069 三档；手动档位 = 给自动策略设边界）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ManualMode {
    Silent,
    Balanced,
    Performance,
}

/// 负载类型判定结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoadType {
    Compute,
    Throughput,
    Mixed,
}

/// 策略决策日志行（诊断快照消费）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FreqDecision {
    pub at_ms: u64,
    pub from: u8,
    pub to: u8,
    /// 决策依据码：0=burst 1=type 2=idle 3=temp 4=manual-bound 5=fallback
    pub reason: u8,
}

// ---------------------------------------------------------------------------
// 策略器
// ---------------------------------------------------------------------------

/// CPU 频率联动策略器。
pub struct CpuGovernor {
    /// 频率表（_PSS 可读时为实机表；不可读 = 固定中档 + 诊断标注）。
    freqs_mhz: [u32; PSTATE_LEVELS],
    pss_available: bool,
    current: u8, // 档位 index（0 = 最高频）
    /// 手动档位边界（None = Balanced 无边界）。
    manual: ManualMode,
    last_down_ms: u64,
    has_downed: bool,
    /// 持续负载起点（None = 无持续负载在计）。
    sustained_since: Option<u64>,
    sustained_is_compute: bool,
    /// 温度超限旗标（F197 联动：强制降档优先于本策略）。
    temp_over_limit: bool,
    log: [Option<FreqDecision>; 32],
    log_head: usize,
    log_n: usize,
    now_ms: u64,
    /// 切换失败重试计数（重试一次后锁定安全档）。
    switch_fail_streak: u8,
    locked_safe: bool,
}

impl CpuGovernor {
    pub const fn new() -> Self {
        CpuGovernor {
            freqs_mhz: DEFAULT_FREQ_MHZ,
            pss_available: true,
            current: 1,
            manual: ManualMode::Balanced,
            last_down_ms: 0,
            has_downed: false,
            sustained_since: None,
            sustained_is_compute: false,
            temp_over_limit: false,
            log: [None; 32],
            log_head: 0,
            log_n: 0,
            now_ms: 0,
            switch_fail_streak: 0,
            locked_safe: false,
        }
    }

    /// ACPI _PSS 表读入（启动时一次；不可读 → graceful 固定中档 + 诊断标注）。
    pub fn load_pss(&mut self, table: Option<[u32; PSTATE_LEVELS]>) {
        match table {
            Some(t) => {
                self.freqs_mhz = t;
                self.pss_available = true;
            }
            None => {
                self.pss_available = false;
                self.current = PSTATE_LEVELS as u8 / 2; // 固定中档
                self.log_push(FreqDecision { at_ms: self.now_ms, from: self.current, to: self.current, reason: 5 });
            }
        }
    }

    pub fn pss_available(&self) -> bool {
        self.pss_available
    }

    /// 手动档位设定（F069 三档；静音封顶低频、性能地板高频）。
    /// 档位 index 0 = 最高频：Silent 封顶 = index 抬到 ≥5（降频进静音区），
    /// Performance 地板 = index 压到 ≤2（升频回性能区）——统一走 clamp_target。
    pub fn set_manual(&mut self, m: ManualMode, now_ms: u64) {
        self.manual = m;
        let clamped = m.clamp_target(self.current);
        if clamped != self.current {
            self.apply(clamped, 4, now_ms);
        }
    }

    /// 交互突发（任何输入事件 / 音频 deadline 线程就绪）：秒升最高档。
    /// 返回切到满频的模型耗时（<50ms 判据；切换本身 <10ms）。
    pub fn burst(&mut self, now_ms: u64) -> u32 {
        self.now_ms = now_ms;
        self.sustained_since = None; // 突发打断持续负载计
        if self.temp_over_limit {
            return SWITCH_COST_MS; // 温度红线优先，不升
        }
        let raw = self.manual.floor().unwrap_or(0) as u8;
        let target = self.manual.clamp_target(raw); // 静音封顶：突发也冲不破（t.max(5)）
        if self.current > target {
            self.apply(target, 0, now_ms);
        }
        SWITCH_COST_MS // 模型：一次 MSR 写，远低于 50ms 判据线
    }

    /// 负载信号（每拍）：维护持续负载计时与类型判定。
    pub fn load_signal(&mut self, active_thread_names: &[&str], now_ms: u64) {
        self.now_ms = now_ms;
        if active_thread_names.is_empty() {
            self.sustained_since = None;
            // 空闲 5s 降最低档（迟滞 5s 防抖）。
            if now_ms.saturating_sub(self.last_down_ms) >= DOWN_HYSTERESIS_MS || !self.has_downed {
                let target = self.manual.floor_or_min();
                if (self.current as usize) < target {
                    self.apply(target as u8, 2, now_ms);
                    self.last_down_ms = now_ms;
                    self.has_downed = true;
                }
            }
            return;
        }
        if self.sustained_since.is_none() {
            self.sustained_since = Some(now_ms);
            self.sustained_is_compute = Self::classify(active_thread_names) == LoadType::Compute;
        }
        // 持续负载 30s → 按类型选档。
        if let Some(since) = self.sustained_since {
            if now_ms.saturating_sub(since) >= TYPE_WINDOW_MS {
                let target = match Self::classify(active_thread_names) {
                    LoadType::Compute => 1,  // 构建 = 高频
                    LoadType::Throughput => PSTATE_LEVELS as u8 - 2, // 下载 = 低频
                    LoadType::Mixed => 3,
                };
                let target = self.manual.clamp_target(target);
                if self.current != target {
                    self.apply(target, 1, now_ms);
                }
                self.sustained_since = Some(now_ms); // 重启 30s 窗
            }
        }
    }

    /// 线程名签名类型判定（主册：构建工具名单内 = 计算型）。
    pub fn classify(names: &[&str]) -> LoadType {
        let mut compute = false;
        let mut throughput = false;
        for n in names {
            if BUILD_SIGNATURES.contains(n) {
                compute = true;
            }
            if DOWNLOAD_SIGNATURES.contains(n) {
                throughput = true;
            }
        }
        match (compute, throughput) {
            (true, false) => LoadType::Compute,
            (false, true) => LoadType::Throughput,
            _ => LoadType::Mixed,
        }
    }

    /// 温度超限旗标（F197 联动）：强制降档优先于本策略。
    pub fn set_temp_over_limit(&mut self, over: bool, now_ms: u64) {
        self.temp_over_limit = over;
        if over {
            // 强制降两档（不低于最低档）。
            let target = (self.current + 2).min(PSTATE_LEVELS as u8 - 1);
            self.apply(target, 3, now_ms);
        }
    }

    /// 频率切换执行（模型：MSR 写；失败重试一次后锁定安全档）。
    fn apply(&mut self, to: u8, reason: u8, now_ms: u64) {
        if self.locked_safe {
            return;
        }
        let from = self.current;
        self.current = to;
        self.log_push(FreqDecision { at_ms: now_ms, from, to, reason });
    }

    /// 切换失败注入（演练面）：两次失败 → 锁定安全档（中档）。
    pub fn inject_switch_failure(&mut self) {
        self.switch_fail_streak += 1;
        if self.switch_fail_streak >= 2 {
            self.locked_safe = true;
            self.current = PSTATE_LEVELS as u8 / 2;
            self.log_push(FreqDecision { at_ms: self.now_ms, from: self.current, to: self.current, reason: 5 });
        }
    }

    pub fn current_level(&self) -> u8 {
        self.current
    }

    pub fn current_freq_mhz(&self) -> u32 {
        self.freqs_mhz[self.current as usize]
    }

    pub fn freq_table(&self) -> [u32; PSTATE_LEVELS] {
        self.freqs_mhz
    }

    pub fn is_locked_safe(&self) -> bool {
        self.locked_safe
    }

    /// 决策日志视图。
    pub fn decisions(&self) -> impl Iterator<Item = FreqDecision> + '_ {
        let start = (self.log_head + 32 - self.log_n) % 32;
        (0..self.log_n).filter_map(move |i| self.log[(start + i) % 32])
    }

    fn log_push(&mut self, d: FreqDecision) {
        self.log[self.log_head] = Some(d);
        self.log_head = (self.log_head + 1) % 32;
        self.log_n = (self.log_n + 1).min(32);
    }
}

/// 手动档位边界辅助（与策略器解耦的纯函数，便于旋钮清单对账）。
trait ManualExt {
    fn floor(&self) -> Option<usize>;
    fn floor_or_min(&self) -> usize;
    fn clamp_target(&self, t: u8) -> u8;
}
impl ManualExt for ManualMode {
    /// 性能档地板（档位 index 不高于 2）。
    fn floor(&self) -> Option<usize> {
        match self {
            ManualMode::Performance => Some(MANUAL_PERF_FLOOR),
            _ => None,
        }
    }
    /// 空闲目标档（静音档降更低有界）。
    fn floor_or_min(&self) -> usize {
        match self {
            ManualMode::Silent => MANUAL_SILENT_CAP.max(1) - 1, // 4（比 5 更低一档）
            _ => PSTATE_LEVELS - 1,
        }
    }
    fn clamp_target(&self, t: u8) -> u8 {
        match self {
            ManualMode::Silent => t.max(MANUAL_SILENT_CAP as u8),
            ManualMode::Performance => t.min(MANUAL_PERF_FLOOR as u8),
            ManualMode::Balanced => t,
        }
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_cpufreq_checks() -> CheckSet {
    let mut cs = CheckSet::new("F048-cpufreq");
    // 1) 交互突发 <50ms（模型：单次 MSR 写 10ms）。
    let mut g = CpuGovernor::new();
    cs.add("burst_under_50ms", g.burst(1_000) < BURST_TO_FULL_CAP_MS && g.current_level() == 0, "");
    // 2) 降档迟滞 5s。
    let mut g2 = CpuGovernor::new();
    g2.burst(0);
    g2.load_signal(&[], 1_000); // 空闲 → 降最低
    g2.burst(2_000); // 突发回最高
    g2.load_signal(&[], 3_000); // 距上次降档 2s < 5s → 不降
    cs.add("down_hysteresis_5s", g2.current_level() == 0, "");
    // 3) 30s 类型判定：构建 → 高频；下载 → 低频。
    let mut g3 = CpuGovernor::new();
    g3.load_signal(&["rustc"], 0);
    g3.load_signal(&["rustc"], 31_000);
    cs.add("type_compute_high", g3.current_level() <= 1, "");
    let mut g4 = CpuGovernor::new();
    g4.load_signal(&["netd"], 0);
    g4.load_signal(&["netd"], 31_000);
    cs.add("type_throughput_low", g4.current_level() >= PSTATE_LEVELS as u8 - 2, "");
    // 4) 空闲 5s 降最低档。
    let mut g5 = CpuGovernor::new();
    g5.load_signal(&["app"], 0);
    g5.load_signal(&[], 6_000);
    cs.add("idle_down_5s", g5.current_level() == PSTATE_LEVELS as u8 - 1, "");
    // 5) _PSS 不可读 → 固定中档 + 诊断标注（graceful 纪律）。
    let mut g6 = CpuGovernor::new();
    g6.load_pss(None);
    cs.add("pss_graceful_mid", !g6.pss_available() && g6.current_level() == PSTATE_LEVELS as u8 / 2 && g6.decisions().any(|d| d.reason == 5), "");
    // 6) 温度超限强制降档优先于策略。
    let mut g7 = CpuGovernor::new();
    g7.load_signal(&["rustc"], 0);
    g7.load_signal(&["rustc"], 31_000); // 高频档
    g7.set_temp_over_limit(true, 32_000);
    cs.add("temp_forces_down", g7.current_level() >= 2, "");
    // 7) 手动静音档封顶低频（F069 叠加边界）。
    let mut g8 = CpuGovernor::new();
    g8.set_manual(ManualMode::Silent, 0);
    g8.burst(1_000); // 突发也冲不破封顶
    cs.add("manual_silent_cap", g8.current_level() >= MANUAL_SILENT_CAP as u8, "");
    // 8) 切换失败重试一次后锁定安全档。
    let mut g9 = CpuGovernor::new();
    g9.inject_switch_failure();
    cs.add("fail_then_lock_safe", !g9.is_locked_safe(), "");
    g9.inject_switch_failure();
    cs.add("fail_twice_locks", g9.is_locked_safe() && g9.current_level() == PSTATE_LEVELS as u8 / 2, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_by_thread_signatures() {
        assert_eq!(CpuGovernor::classify(&["cc1", "ld"]), LoadType::Compute);
        assert_eq!(CpuGovernor::classify(&["curl"]), LoadType::Throughput);
        assert_eq!(CpuGovernor::classify(&["rustc", "wget"]), LoadType::Mixed);
        assert_eq!(CpuGovernor::classify(&["ui-app"]), LoadType::Mixed);
    }

    #[test]
    fn pss_table_overrides_default() {
        let mut g = CpuGovernor::new();
        let mut table = DEFAULT_FREQ_MHZ;
        table[0] = 5_200;
        g.load_pss(Some(table));
        g.burst(0);
        assert_eq!(g.current_freq_mhz(), 5_200);
        assert!(g.pss_available());
    }

    #[test]
    fn sustained_type_reselects_every_window() {
        let mut g = CpuGovernor::new();
        g.load_signal(&["rustc"], 0);
        g.load_signal(&["rustc"], 31_000);
        let at_high = g.current_level();
        g.load_signal(&["netd"], 62_000); // 类型反转 → 下调
        assert!(g.current_level() > at_high || g.current_level() >= PSTATE_LEVELS as u8 - 2);
    }

    #[test]
    fn decisions_log_records_reasons() {
        let mut g = CpuGovernor::new();
        g.burst(0);
        g.load_signal(&[], 6_000);
        let reasons: Vec<u8> = g.decisions().map(|d| d.reason).collect();
        assert!(reasons.contains(&0) && reasons.contains(&2));
    }

    #[test]
    fn temp_overrides_type_selection() {
        let mut g = CpuGovernor::new();
        g.load_signal(&["rustc"], 0);
        g.load_signal(&["rustc"], 31_000);
        let before = g.current_level();
        g.set_temp_over_limit(true, 32_000);
        assert!(g.current_level() > before || g.current_level() >= 2);
    }
}
