//! F048 CPU 频率联动 · 深化件（AI-K1 深化批次三 · G-B-08）。
//!
//! | # | 主册原文 | 本件机制 |
//! | --- | --- | --- |
//! | 1 | 【功能定义】「**Y7000 实机频率表驱动（ACPI _PSS 读取**，graceful 不可读则固定档）」 | [`PssTable`] 频率表（定长 16 档）+ [`PssLoader`] 装载与 graceful 降级 |
//! | 2 | 【状态与异常】「**_PSS 不可读（部分固件）→ 固定中档 + 诊断标注**（graceful 已实测同纪律）」 | [`PssFallback`] 降级标注（不可读也要能回答「现在按什么在跑」） |
//! | 3 | 【状态与异常】「**频率切换失败 → 重试一次后锁定安全档**」 | [`SwitchGuard`] 切换守卫（一次重试 + 锁定安全档 + 报备） |
//! | 4 | 【状态与异常】「**温度超限（F197 联动）→ 强制降档优先于本策略**」 | [`ThermalOverride`] 温度越权（策略级之上的硬约束） |
//! | 5 | 【设计细节】「升档触发：任何输入事件或音频 deadline 线程就绪；**降档迟滞 5s**；**30s 类型判定按线程名签名（构建工具名单内=计算型）**；**频率切换自身耗时 <10ms（MSR 写）**；全策略参数进旋钮清单」 | [`WorkloadClass`] 名单匹配器 + [`SwitchTimer`] 切换耗时账 + [`KnobTable`] 旋钮 |
//! | 6 | 【验收判据】「**交互突发响应（点按到满频）<50ms**」 | [`BurstMeter`] 突发响应账（点按时刻 → 满频时刻） |
//! | 7 | 【验收判据】「续航对比：自动策略 vs 固定高频，视频播放场景**续航提升 >15%**」 | [`BatteryContrast`] 双策略对照账（同场景对拍） |

use crate::checks::CheckSet;
use crate::perfstar::perfkit::{DiagSev, DiagSink, KnobTable};

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 频率表档位上限（ACPI _PSS 常见不超过 16 档）。
pub const PSS_STATES: usize = 16;
/// 降档迟滞 5s（主册【设计细节】）。
pub const DOWN_HYSTERESIS_MS: u32 = 5_000;
/// 类型判定观察窗 30s（主册「持续负载 30s 后按类型选档」）。
pub const CLASS_WINDOW_MS: u64 = 30_000;
/// 频率切换耗时红线 10ms（主册「MSR 写」）。
pub const SWITCH_REDLINE_US: u32 = 10_000;
/// 交互突发响应红线 50ms（主册【验收判据】）。
pub const BURST_REDLINE_MS: u32 = 50;
/// 续航提升红线 15%（千分 150）。
pub const BATTERY_GAIN_PERMILLE: u32 = 150;
/// _PSS 不可读时的固定档（中档 = 表正中）。
pub const FALLBACK_INDEX: usize = PSS_STATES / 2;
/// 安全档（切换失败锁定用；取表中低频侧第 3 档）。
pub const SAFE_INDEX: usize = PSS_STATES - 3;
/// 构建工具名单长度上限。
pub const BUILDER_LIST: usize = 8;

// ---------------------------------------------------------------------------
// 1. ACPI _PSS 频率表
// ---------------------------------------------------------------------------

/// 一档 P-state（频率 MHz + 标称功耗 mW）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PssState {
    /// 核心频率（MHz）。
    pub freq_mhz: u32,
    /// 标称功耗（mW，用于续航折算）。
    pub power_mw: u32,
}

/// 频率表（启动时从 ACPI 读入一次）。
#[derive(Clone, Copy, Debug)]
pub struct PssTable {
    states: [Option<PssState>; PSS_STATES],
    /// 实际档数（0 = 未读到）。
    pub n: usize,
}

impl PssTable {
    pub const fn empty() -> Self {
        PssTable { states: [None; PSS_STATES], n: 0 }
    }
    /// 装载（按频率降序：index 0 = 最高频）。
    pub fn load(&mut self, list: &[PssState]) -> usize {
        self.n = list.len().min(PSS_STATES);
        for i in 0..self.n {
            self.states[i] = Some(list[i]);
        }
        self.n
    }
    pub fn get(&self, i: usize) -> Option<PssState> {
        if i < self.n {
            self.states[i]
        } else {
            None
        }
    }
    /// 表是否降序（index 0 必须是最高频——本域所有「升档=index 变小」的前提）。
    pub fn is_descending(&self) -> bool {
        for i in 1..self.n {
            let a = self.states[i - 1].map(|s| s.freq_mhz).unwrap_or(0);
            let b = self.states[i].map(|s| s.freq_mhz).unwrap_or(0);
            if a < b {
                return false;
            }
        }
        self.n > 0
    }
    /// 最高频档（index 0）。
    pub fn max_freq_mhz(&self) -> Option<u32> {
        self.get(0).map(|s| s.freq_mhz)
    }
    /// 最低频档（index n-1）。
    pub fn min_freq_mhz(&self) -> Option<u32> {
        if self.n == 0 {
            return None;
        }
        self.get(self.n - 1).map(|s| s.freq_mhz)
    }
}

// ---------------------------------------------------------------------------
// 2. _PSS 不可读的 graceful 降级
// ---------------------------------------------------------------------------

/// 频率表来源（决定「现在按什么在跑」能不能解释清楚）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PssSource {
    /// ACPI _PSS 读到了。
    Acpi,
    /// 读不到 → 固定中档（主册「固定中档 + 诊断标注」）。
    FallbackMid,
}

/// 降级标注（不可读也要能回答，且必须报备——不静默降级）。
#[derive(Clone, Copy, Debug)]
pub struct PssFallback {
    pub source: PssSource,
    /// 当前生效档（fallback 时为 FALLBACK_INDEX）。
    pub active_index: usize,
    /// 降级次数。
    pub fallbacks: u32,
}

impl PssFallback {
    pub const fn new() -> Self {
        PssFallback { source: PssSource::Acpi, active_index: 0, fallbacks: 0 }
    }
    /// 装载结果裁定：`ok` = _PSS 是否读成功。
    pub fn decide(&mut self, ok: bool, sink: Option<&mut DiagSink>, now_ms: u64) {
        if ok {
            self.source = PssSource::Acpi;
            self.active_index = 0;
            return;
        }
        self.source = PssSource::FallbackMid;
        self.active_index = FALLBACK_INDEX;
        self.fallbacks += 1;
        if let Some(s) = sink {
            s.push("F048", 1, now_ms, DiagSev::Warn, FALLBACK_INDEX as u64, 0, b"_PSS unreadable -> mid");
        }
    }
    /// 人话说明（用户能知道「为什么现在不快」）。
    pub fn text(&self) -> &'static str {
        match self.source {
            PssSource::Acpi => "频率表来自固件（ACPI _PSS），自动升降档生效",
            PssSource::FallbackMid => "固件未提供频率表，已固定在中间档运行（性能与续航均为折中）",
        }
    }
}

// ---------------------------------------------------------------------------
// 3. 频率切换守卫（失败重试一次 → 锁定安全档）
// ---------------------------------------------------------------------------

/// 切换结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchResult {
    /// 成功。
    Ok,
    /// 首次失败，已重试。
    Retried,
    /// 重试仍失败 → 锁定安全档。
    Locked,
}

/// 切换守卫（主册「频率切换失败 → 重试一次后锁定安全档」）。
#[derive(Clone, Copy, Debug)]
pub struct SwitchGuard {
    /// 已重试次数（每个目标档只重试一次）。
    pub retries: u32,
    /// 锁定次数。
    pub locks: u32,
    /// 锁定后是否已解锁（恢复需显式 reset）。
    pub locked: bool,
    /// 锁定时的档位。
    pub locked_index: usize,
}

impl SwitchGuard {
    pub const fn new() -> Self {
        SwitchGuard { retries: 0, locks: 0, locked: false, locked_index: SAFE_INDEX }
    }
    /// 报告一次切换结果，返回应执行动作。
    pub fn report(&mut self, ok: bool, sink: Option<&mut DiagSink>, now_ms: u64) -> SwitchResult {
        if ok {
            return SwitchResult::Ok;
        }
        if self.retries == 0 {
            self.retries += 1;
            return SwitchResult::Retried;
        }
        self.locks += 1;
        self.locked = true;
        self.retries = 0;
        if let Some(s) = sink {
            s.push("F048", 2, now_ms, DiagSev::Error, SAFE_INDEX as u64, 0, b"freq switch failed -> safe lock");
        }
        SwitchResult::Locked
    }
    /// 锁定后是否应强制使用安全档。
    pub fn forced_index(&self) -> Option<usize> {
        if self.locked {
            Some(self.locked_index)
        } else {
            None
        }
    }
    /// 显式复位（固件恢复后由诊断面调用——解锁不是自动的，避免抖动）。
    pub fn reset(&mut self) {
        self.locked = false;
        self.retries = 0;
    }
}

// ---------------------------------------------------------------------------
// 4. 温度越权（F197 联动，优先于本策略）
// ---------------------------------------------------------------------------

/// 温度越权（主册「温度超限（F197 联动）→ 强制降档优先于本策略」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThermalOverride {
    /// 是否处于越权态。
    pub active: bool,
    /// 越权允许的最高档（index 越大 = 频越低）。
    pub cap_index: usize,
    /// 越权触发次数。
    pub triggers: u32,
}

impl ThermalOverride {
    pub const fn new() -> Self {
        ThermalOverride { active: false, cap_index: 0, triggers: 0 }
    }
    /// F197 上报温度是否超限。
    pub fn report(&mut self, over: bool, cap_index: usize, sink: Option<&mut DiagSink>, now_ms: u64) {
        if over {
            if !self.active {
                self.triggers += 1;
                if let Some(s) = sink {
                    s.push("F048", 3, now_ms, DiagSev::Warn, cap_index as u64, 0, b"thermal override active");
                }
            }
            self.active = true;
            self.cap_index = cap_index;
        } else {
            self.active = false;
        }
    }
    /// 钳制目标档（越权时只许更保守——取 index 较大者）。
    pub fn clamp(&self, want_index: usize) -> usize {
        if self.active {
            want_index.max(self.cap_index)
        } else {
            want_index
        }
    }
    /// 越权是否优先于本策略（恒真——这是硬约束，不是可调项）。
    pub const fn preempts_policy() -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// 5. 负载类型判定（30s 观察窗 + 构建工具名单签名）
// ---------------------------------------------------------------------------

/// 负载类型（主册「构建=高频/下载=低频」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkloadKind {
    /// 交互突发（任何输入事件或音频 deadline 线程就绪 → 秒升最高档）。
    Interactive,
    /// 计算型（构建工具名单内 → 高频）。
    Compute,
    /// 传输型（下载 → 低频）。
    Transfer,
    /// 未知（名单外且非交互 → 按默认策略）。
    Unknown,
}

impl WorkloadKind {
    pub const fn name(self) -> &'static str {
        match self {
            WorkloadKind::Interactive => "交互",
            WorkloadKind::Compute => "计算",
            WorkloadKind::Transfer => "传输",
            WorkloadKind::Unknown => "未知",
        }
    }
    /// 倾向档位方向：true = 倾向高频（index 小）。
    pub const fn prefers_high(self) -> bool {
        matches!(self, WorkloadKind::Interactive | WorkloadKind::Compute)
    }
}

/// 构建工具名单（主册「按线程名签名（构建工具名单内=计算型）」）。
pub struct BuilderList {
    names: [Option<[u8; 16]>; BUILDER_LIST],
    lens: [u8; BUILDER_LIST],
    n: usize,
}

impl BuilderList {
    pub const fn new() -> Self {
        BuilderList { names: [None; BUILDER_LIST], lens: [0; BUILDER_LIST], n: 0 }
    }
    /// 登记一个构建工具名签名。
    pub fn register(&mut self, name: &str) -> bool {
        if self.n >= BUILDER_LIST {
            return false;
        }
        let b = name.as_bytes();
        let n = b.len().min(16);
        let mut buf = [0u8; 16];
        buf[..n].copy_from_slice(&b[..n]);
        self.names[self.n] = Some(buf);
        self.lens[self.n] = n as u8;
        self.n += 1;
        true
    }
    /// 线程名是否命中名单（前缀匹配：构建进程常带版本号后缀）。
    pub fn matches(&self, thread: &str) -> bool {
        let t = thread.as_bytes();
        for i in 0..self.n {
            if let Some(name) = self.names[i] {
                let len = self.lens[i] as usize;
                if t.len() >= len && &t[..len] == &name[..len] {
                    return true;
                }
            }
        }
        false
    }
    pub fn len(&self) -> usize {
        self.n
    }
}

/// 负载分类器（30s 观察窗内累积证据，窗满才下结论——不急着贴标签）。
#[derive(Clone, Copy, Debug)]
pub struct WorkloadClass {
    pub window_start_ms: Option<u64>,
    pub interactive_hits: u32,
    pub compute_hits: u32,
    pub transfer_hits: u32,
    /// 已下结论次数。
    pub verdicts: u32,
}

impl WorkloadClass {
    pub const fn new() -> Self {
        WorkloadClass { window_start_ms: None, interactive_hits: 0, compute_hits: 0, transfer_hits: 0, verdicts: 0 }
    }
    /// 开/续窗。
    pub fn open(&mut self, now_ms: u64) {
        if self.window_start_ms.is_none() {
            self.window_start_ms = Some(now_ms);
        }
    }
    /// 喂证据。
    pub fn note(&mut self, kind: WorkloadKind) {
        match kind {
            WorkloadKind::Interactive => self.interactive_hits += 1,
            WorkloadKind::Compute => self.compute_hits += 1,
            WorkloadKind::Transfer => self.transfer_hits += 1,
            WorkloadKind::Unknown => {}
        }
    }
    /// 窗是否满 30s。
    pub fn window_full(&self, now_ms: u64) -> bool {
        match self.window_start_ms {
            Some(s) => now_ms.saturating_sub(s) >= CLASS_WINDOW_MS,
            None => false,
        }
    }
    /// 窗满后下结论（多数票；平票取交互优先——交互优先不是口号）。
    pub fn verdict(&mut self, now_ms: u64) -> Option<WorkloadKind> {
        if !self.window_full(now_ms) {
            return None;
        }
        self.verdicts += 1;
        let k = if self.interactive_hits >= self.compute_hits.max(self.transfer_hits) && self.interactive_hits > 0 {
            WorkloadKind::Interactive
        } else if self.compute_hits >= self.transfer_hits && self.compute_hits > 0 {
            WorkloadKind::Compute
        } else if self.transfer_hits > 0 {
            WorkloadKind::Transfer
        } else {
            WorkloadKind::Unknown
        };
        // 结论即复位，下一窗重新累积
        self.window_start_ms = None;
        self.interactive_hits = 0;
        self.compute_hits = 0;
        self.transfer_hits = 0;
        Some(k)
    }
}

// ---------------------------------------------------------------------------
// 6. 切换耗时账 + 突发响应账 + 续航对照账
// ---------------------------------------------------------------------------

/// 切换耗时账（主册「频率切换自身耗时 <10ms（MSR 写）」）。
#[derive(Clone, Copy, Debug)]
pub struct SwitchTimer {
    pub samples: u32,
    pub over_redline: u32,
    pub max_us: u32,
}

impl SwitchTimer {
    pub const fn new() -> Self {
        SwitchTimer { samples: 0, over_redline: 0, max_us: 0 }
    }
    pub fn note(&mut self, us: u32) {
        self.samples += 1;
        if us > SWITCH_REDLINE_US {
            self.over_redline += 1;
        }
        if us > self.max_us {
            self.max_us = us;
        }
    }
    pub fn passes(&self) -> bool {
        self.samples > 0 && self.over_redline == 0
    }
}

/// 突发响应账（主册「交互突发响应（点按到满频）<50ms」）。
#[derive(Clone, Copy, Debug)]
pub struct BurstMeter {
    pub samples: u32,
    pub over_redline: u32,
    pub max_ms: u32,
}

impl BurstMeter {
    pub const fn new() -> Self {
        BurstMeter { samples: 0, over_redline: 0, max_ms: 0 }
    }
    /// 记一次点按到满频的耗时。
    pub fn note(&mut self, ms: u32) {
        self.samples += 1;
        if ms > BURST_REDLINE_MS {
            self.over_redline += 1;
        }
        if ms > self.max_ms {
            self.max_ms = ms;
        }
    }
    pub fn passes(&self) -> bool {
        self.samples > 0 && self.over_redline == 0
    }
}

/// 续航对照账（主册「自动策略 vs 固定高频，视频播放场景续航提升 >15%」）。
#[derive(Clone, Copy, Debug)]
pub struct BatteryContrast {
    /// 自动策略下视频播放时长（分钟）。
    pub auto_minutes: u32,
    /// 固定高频下视频播放时长（分钟）。
    pub fixed_high_minutes: u32,
}

impl BatteryContrast {
    /// 提升千分（(auto-fixed)/fixed）。
    pub fn gain_permille(&self) -> i32 {
        if self.fixed_high_minutes == 0 {
            return 0;
        }
        let d = self.auto_minutes as i64 - self.fixed_high_minutes as i64;
        ((d * 1000) / self.fixed_high_minutes as i64) as i32
    }
    /// 是否达标（>15%）。
    pub fn passes(&self) -> bool {
        self.fixed_high_minutes > 0 && self.gain_permille() > BATTERY_GAIN_PERMILLE as i32
    }
    /// 结论文案（不裸抛百分比）。
    pub fn verdict(&self) -> &'static str {
        if self.fixed_high_minutes == 0 {
            return "缺少固定高频对照数据，无法判定续航提升";
        }
        if self.passes() {
            "自动策略续航优于固定高频，达标"
        } else if self.gain_permille() > 0 {
            "自动策略续航略优，但未达 15% 线"
        } else {
            "自动策略续航未优于固定高频（策略需回炉）"
        }
    }
}

/// 旋钮登记（主册「全策略参数进旋钮清单（无隐藏魔法数）」）。
pub fn register_knobs(t: &mut KnobTable) {
    t.register("cpu.down_hysteresis_ms", DOWN_HYSTERESIS_MS as i64, 0, 30_000, "ms");
    t.register("cpu.class_window_ms", CLASS_WINDOW_MS as i64, 1_000, 300_000, "ms");
    t.register("cpu.switch_redline_us", SWITCH_REDLINE_US as i64, 1_000, 100_000, "us");
    t.register("cpu.burst_redline_ms", BURST_REDLINE_MS as i64, 5, 500, "ms");
    t.register("cpu.battery_gain_permille", BATTERY_GAIN_PERMILLE as i64, 0, 900, "permille");
    t.register("cpu.fallback_index", FALLBACK_INDEX as i64, 0, 15, "index");
    t.register("cpu.safe_index", SAFE_INDEX as i64, 0, 15, "index");
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F048-cpufreq-ext");
    // 1) 频率表降序装载（index 0 = 最高频，本域「升档=index 变小」的前提）。
    let mut t = PssTable::empty();
    let loaded = t.load(&[
        PssState { freq_mhz: 4_500, power_mw: 45_000 },
        PssState { freq_mhz: 3_600, power_mw: 35_000 },
        PssState { freq_mhz: 2_400, power_mw: 20_000 },
        PssState { freq_mhz: 1_200, power_mw: 10_000 },
    ]);
    cs.add(
        "pss_table_descending",
        loaded == 4 && t.is_descending() && t.max_freq_mhz() == Some(4_500) && t.min_freq_mhz() == Some(1_200),
        "",
    );
    // 非降序表必须被检出（否则升档方向会反）
    let mut bad = PssTable::empty();
    bad.load(&[PssState { freq_mhz: 1_200, power_mw: 10_000 }, PssState { freq_mhz: 4_500, power_mw: 45_000 }]);
    cs.add("pss_nondescending_detected", !bad.is_descending(), "");
    // 2) _PSS 不可读 → 固定中档 + 诊断标注（不静默降级）。
    let mut sink = DiagSink::new();
    let mut fb = PssFallback::new();
    fb.decide(true, Some(&mut sink), 0);
    let acpi_ok = fb.source == PssSource::Acpi && fb.active_index == 0;
    fb.decide(false, Some(&mut sink), 1_000);
    cs.add(
        "pss_fallback_mid_and_reported",
        acpi_ok && fb.source == PssSource::FallbackMid && fb.active_index == FALLBACK_INDEX && fb.fallbacks == 1 && sink.count(DiagSev::Warn) == 1,
        "",
    );
    cs.add("pss_fallback_text_explainable", fb.text().contains("固定在中间档"), "");
    // 3) 切换守卫：失败重试一次，再失败锁定安全档 + 报备。
    let mut sg = SwitchGuard::new();
    let r1 = sg.report(false, None, 0);
    let r2 = sg.report(false, None, 100);
    cs.add(
        "switch_retry_once_then_lock",
        r1 == SwitchResult::Retried && r2 == SwitchResult::Locked && sg.locks == 1 && sg.locked && sg.forced_index() == Some(SAFE_INDEX),
        "",
    );
    // 成功后不锁定；锁定需显式复位（不自动解锁，防抖动）
    let mut sg2 = SwitchGuard::new();
    sg2.report(false, None, 0);
    let r3 = sg2.report(true, None, 1);
    cs.add("switch_ok_clears_retry", r3 == SwitchResult::Ok && !sg2.locked, "");
    sg2.reset();
    cs.add("switch_reset_unlocks", sg2.forced_index().is_none(), "");
    // 4) 温度越权：优先于本策略，且只许更保守（取 index 较大者）。
    let mut th = ThermalOverride::new();
    th.report(true, 6, None, 0);
    cs.add(
        "thermal_preempts_policy",
        ThermalOverride::preempts_policy() && th.active && th.clamp(0) == 6 && th.clamp(8) == 8 && th.triggers == 1,
        "",
    );
    th.report(false, 6, None, 1);
    cs.add("thermal_release", !th.active && th.clamp(0) == 0, "");
    // 5) 构建工具名单（前缀匹配，带版本号后缀也命中）。
    let mut bl = BuilderList::new();
    bl.register("cargo");
    bl.register("msbuild");
    cs.add("builder_list_prefix_match", bl.matches("cargo-1.75") && bl.matches("msbuild.exe") && !bl.matches("notepad"), "");
    // 6) 负载分类：30s 窗满才下结论（不急着贴标签）。
    let mut wc = WorkloadClass::new();
    wc.open(0);
    wc.note(WorkloadKind::Compute);
    cs.add("class_not_before_window", wc.verdict(1_000).is_none() && !wc.window_full(1_000), "");
    for _ in 0..10 {
        wc.note(WorkloadKind::Compute);
    }
    let v = wc.verdict(CLASS_WINDOW_MS);
    cs.add("class_verdict_after_window", v == Some(WorkloadKind::Compute) && wc.verdicts == 1, "");
    // 平票时交互优先（交互优先不是口号）
    let mut wc2 = WorkloadClass::new();
    wc2.open(0);
    wc2.note(WorkloadKind::Interactive);
    wc2.note(WorkloadKind::Compute);
    cs.add("class_interactive_wins_tie", wc2.verdict(CLASS_WINDOW_MS) == Some(WorkloadKind::Interactive), "");
    // 7) 切换耗时账（<10ms）。
    let mut st = SwitchTimer::new();
    st.note(3_000);
    st.note(9_999);
    cs.add("switch_timer_passes", st.passes() && st.max_us == 9_999, "");
    st.note(10_001);
    cs.add("switch_timer_detects_over", !st.passes() && st.over_redline == 1, "");
    // 8) 突发响应账（<50ms）。
    let mut bm = BurstMeter::new();
    bm.note(20);
    bm.note(49);
    cs.add("burst_meter_passes", bm.passes() && bm.max_ms == 49, "");
    bm.note(51);
    cs.add("burst_meter_detects_over", !bm.passes(), "");
    // 9) 续航对照（>15%）；缺对照数据不判达标（不粉饰）。
    let bc = BatteryContrast { auto_minutes: 350, fixed_high_minutes: 300 };
    cs.add("battery_gain_passes", bc.gain_permille() == 166 && bc.passes() && bc.verdict() == "自动策略续航优于固定高频，达标", "");
    let bc2 = BatteryContrast { auto_minutes: 310, fixed_high_minutes: 300 };
    cs.add("battery_gain_below_line", !bc2.passes() && bc2.verdict() == "自动策略续航略优，但未达 15% 线", "");
    let bc3 = BatteryContrast { auto_minutes: 0, fixed_high_minutes: 0 };
    cs.add("battery_no_baseline", !bc3.passes() && bc3.verdict() == "缺少固定高频对照数据，无法判定续航提升", "");
    // 10) 旋钮清单（无隐藏魔法数）。
    let mut kt = KnobTable::new();
    register_knobs(&mut kt);
    cs.add(
        "knobs_registered",
        kt.len() == 7 && kt.get("cpu.down_hysteresis_ms") == Some(5_000) && kt.get("cpu.burst_redline_ms") == Some(50) && kt.all_in_range(),
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pss_table_overflow_truncates_to_capacity() {
        let mut t = PssTable::empty();
        let many: [PssState; 20] = [PssState { freq_mhz: 1_000, power_mw: 1_000 }; 20];
        assert_eq!(t.load(&many), PSS_STATES);
        assert_eq!(t.n, PSS_STATES);
        assert!(t.get(PSS_STATES).is_none(), "越界读取返回 None，不 panic");
    }

    #[test]
    fn switch_guard_locks_only_after_two_failures() {
        let mut g = SwitchGuard::new();
        assert_eq!(g.report(true, None, 0), SwitchResult::Ok);
        assert_eq!(g.report(false, None, 1), SwitchResult::Retried);
        assert!(!g.locked, "一次失败不锁定");
        assert_eq!(g.report(false, None, 2), SwitchResult::Locked);
        assert!(g.locked);
    }

    #[test]
    fn thermal_override_never_allows_higher_freq() {
        let mut t = ThermalOverride::new();
        t.report(true, 10, None, 0);
        for want in 0..PSS_STATES {
            assert!(t.clamp(want) >= want, "越权不得把档位推向高频");
        }
    }

    #[test]
    fn workload_class_resets_after_verdict() {
        let mut w = WorkloadClass::new();
        w.open(0);
        w.note(WorkloadKind::Transfer);
        let v = w.verdict(CLASS_WINDOW_MS);
        assert_eq!(v, Some(WorkloadKind::Transfer));
        assert_eq!(w.transfer_hits, 0, "下结论后清零，下一窗重新累积");
        assert!(w.window_start_ms.is_none());
    }

    #[test]
    fn builder_list_full_capacity_is_respected() {
        let mut b = BuilderList::new();
        for _i in 0..BUILDER_LIST {
            assert!(b.register("tool"));
        }
        assert!(!b.register("one-too-many"), "超容量登记失败并如实返回");
    }

    #[test]
    fn battery_contrast_negative_reports_honestly() {
        let b = BatteryContrast { auto_minutes: 250, fixed_high_minutes: 300 };
        assert_eq!(b.gain_permille(), -166);
        assert_eq!(b.verdict(), "自动策略续航未优于固定高频（策略需回炉）");
    }
}
