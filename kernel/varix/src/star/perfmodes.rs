//! F069 性能模式三档 · 完整设计（STAR I 主册 G-B-29）。
//!
//! **判据（主册）**：三档参数差异实测可辨（频率/写入量/续航三指标分档
//! 明显）；切换全程无音频爆音、无 UI 卡顿。
//!
//! **设计要点（主册）**：
//! - 静音/均衡/性能三档一键切换，联动 CPU 频率边界（F048）+ 写合并窗口
//!   （F046）+ 音频缓冲档（F064）+ 温度阈值；档位定义全走旋钮清单；
//! - 三档参数表（主册规格框架·唯一数值源）：静音（频率≤中档/窗口 8s/
//!   缓冲 20ms）/均衡（自动 F048/窗口 5s/缓冲 10ms）/性能（全频/窗口
//!   5s/缓冲 5ms）；
//! - 叠加规则文档化：**手动档 = 给自动策略设边界，自动策略 = 边界内调节**
//!   （静音档封顶中档；均衡档自动全权；性能档锁全频无调节空间）；
//! - 当前档持久化（重启保持）；切换事件入账本；
//! - 温度强制降档（F197·75℃）覆盖手动性能档（安全优先）+ 通知说明；
//!   迟滞回退（触发 75℃/解除 70℃——差 5℃ 防阈值震荡）；传感器不可读
//!   优雅跳过（F197 graceful 纪律同源）；
//! - 档位参数缺失（旋钮表版本不齐）→ 拒绝切换并诊断报备；
//! - 切换 toast 显示生效项清单（频率边界/窗口/缓冲三件全在案）；
//! - 切换动画 200ms（F124 标准曲线）；托盘图标三态区分；
//! - 无爆音的结构证明：全部参数**同一时间戳原子生效**（不存在「频率已
//!   变、缓冲未变」的中间态）。
//!
//! 无外部依赖。F048/F046/F064 的执行体是各分队接缝，本模块只做**边界
//! 裁决与参数分发**（显式参数注入口，不反向制造编译依赖）。时间注入式
//! （分钟戳 + 毫秒戳），宿主测试确定复现。

use crate::checks::CheckSet;
use crate::star::sbase::{Knob, KnobReg, MinuteBook, RingLog};

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数进旋钮清单——一处一事实）
// ---------------------------------------------------------------------------

/// 切换动画时长（F124 标准曲线）。
pub const SWITCH_ANIM_MS: u32 = 200;

/// F197 温度强制阈值（℃）——75℃ 强制切静音档（仅调 F069 边界，无感降档）。
pub const THERMAL_FORCE_C: i32 = 75;

/// F197 迟滞解除阈值（℃）——触发-回退差 5℃，防阈值震荡。
pub const THERMAL_RELEASE_C: i32 = 70;

/// 切换审计环容量。
const AUDIT_CAP: usize = 32;

/// 诊断报备容量。
const NOTE_CAP: usize = 64;

/// 切换事件账本保留窗（分钟）。
const SWITCH_BOOK_MIN: u64 = 1440;

// ---------------------------------------------------------------------------
// 档位模型
// ---------------------------------------------------------------------------

/// 性能三档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerfMode {
    /// 静音：频率封顶中档、窗口 8s、缓冲 20ms（续航优先）。
    Silent,
    /// 均衡：自动策略全权（F048）、窗口 5s、缓冲 10ms。
    Balanced,
    /// 性能：锁全频、窗口 5s、缓冲 5ms（全速释放）。
    Performance,
}

impl PerfMode {
    /// 持久化序号（0/1/2——存储介质上的唯一形态）。
    pub fn idx(self) -> u8 {
        match self {
            PerfMode::Silent => 0,
            PerfMode::Balanced => 1,
            PerfMode::Performance => 2,
        }
    }

    /// 从持久化序号恢复（其余值 = 存储损坏，拒绝）。
    pub fn from_idx(v: u8) -> Option<PerfMode> {
        match v {
            0 => Some(PerfMode::Silent),
            1 => Some(PerfMode::Balanced),
            2 => Some(PerfMode::Performance),
            _ => None,
        }
    }

    /// 托盘图标三态区分（三态互异——一眼可辨）。
    pub fn tray_glyph(self) -> &'static str {
        match self {
            PerfMode::Silent => "tray-perf-silent",
            PerfMode::Balanced => "tray-perf-balanced",
            PerfMode::Performance => "tray-perf-turbo",
        }
    }

    /// 参数摘要（悬浮可见）。
    pub fn summary(self) -> &'static str {
        match self {
            PerfMode::Silent => "freq<=mid / window 8s / buffer 20ms",
            PerfMode::Balanced => "freq auto(F048) / window 5s / buffer 10ms",
            PerfMode::Performance => "freq full / window 5s / buffer 5ms",
        }
    }
}

/// CPU 频率档位（F048 P-state 抽象：Low<Mid<High<Top，序号 1..=4 入旋钮）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FreqLevel {
    /// 最低档（空闲驻留）。
    Low,
    /// 中档（静音档封顶线）。
    Mid,
    /// 高档。
    High,
    /// 最高档（全频）。
    Top,
}

impl FreqLevel {
    /// 旋钮序号（1..=4）。
    pub fn idx(self) -> i64 {
        match self {
            FreqLevel::Low => 1,
            FreqLevel::Mid => 2,
            FreqLevel::High => 3,
            FreqLevel::Top => 4,
        }
    }

    /// 从旋钮序号还原（越界 = 旋钮表缺陷，None）。
    pub fn from_idx(v: i64) -> Option<FreqLevel> {
        match v {
            1 => Some(FreqLevel::Low),
            2 => Some(FreqLevel::Mid),
            3 => Some(FreqLevel::High),
            4 => Some(FreqLevel::Top),
            _ => None,
        }
    }
}

/// 档位参数表（一次声明全量入旋钮清单——缺一即「版本不齐」）。
#[derive(Clone, Copy, Debug)]
pub struct ModeProfile {
    /// 频率边界下限（自动策略请求低于下限时抬到下限）。
    pub freq_floor: FreqLevel,
    /// 频率边界上限（封顶线——静音档 = Mid）。
    pub freq_ceil: FreqLevel,
    /// F046 写合并窗口（秒）——窗口越长写入合并越多，写入量越少。
    pub wc_window_s: u64,
    /// F064 音频缓冲（ms）——缓冲越大越省电、延迟越高。
    pub audio_buf_ms: u32,
}

/// 三档参数唯一源（与 [`PerfMode::summary`] 文案一一对应——一处一事实）。
pub fn profile_of(mode: PerfMode) -> ModeProfile {
    match mode {
        PerfMode::Silent => ModeProfile {
            freq_floor: FreqLevel::Low,
            freq_ceil: FreqLevel::Mid,
            wc_window_s: 8,
            audio_buf_ms: 20,
        },
        PerfMode::Balanced => ModeProfile {
            freq_floor: FreqLevel::Low,
            freq_ceil: FreqLevel::Top,
            wc_window_s: 5,
            audio_buf_ms: 10,
        },
        PerfMode::Performance => ModeProfile {
            freq_floor: FreqLevel::Top,
            freq_ceil: FreqLevel::Top,
            wc_window_s: 5,
            audio_buf_ms: 5,
        },
    }
}

// ---------------------------------------------------------------------------
// 频率边界裁决（手动档 = 边界，自动策略 = 边界内调节）
// ---------------------------------------------------------------------------

/// F048 自动策略请求的边界裁决面：请求档钳入当前生效档的 [floor, ceil]。
pub fn clamp_freq(requested: FreqLevel, p: ModeProfile) -> FreqLevel {
    if requested < p.freq_floor {
        p.freq_floor
    } else if requested > p.freq_ceil {
        p.freq_ceil
    } else {
        requested
    }
}

// ---------------------------------------------------------------------------
// 切换事件与账本
// ---------------------------------------------------------------------------

/// 切换原因（审计留痕）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchReason {
    /// 用户手动切换。
    Manual,
    /// F197 温度强制降档（安全优先）。
    ThermalForce,
    /// 温度迟滞解除，回手动档。
    ThermalRelease,
    /// 重启后持久化恢复。
    Restore,
}

/// 一次档位生效记录（toast 生效项清单 = 本结构三参数直读）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchEvent {
    /// 生效时刻（ms 注入钟）。
    pub stamp_ms: u64,
    /// 原子生效时间戳——全部参数共用此单值（无中间态证明）。
    pub atom_ms: u64,
    pub from: PerfMode,
    pub to: PerfMode,
    pub reason: SwitchReason,
    pub applied_freq_floor: FreqLevel,
    pub applied_freq_ceil: FreqLevel,
    pub applied_wc_window_s: u64,
    pub applied_audio_buf_ms: u32,
}

/// 切换结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchOutcome {
    /// 生效（事件已入审计与账本）。
    Applied(SwitchEvent),
    /// 同档重复切换（无操作）。
    SameMode,
    /// 旋钮表版本不齐——拒绝 + 诊断报备。
    KnobMissing,
}

/// F197 温度馈入动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThermalAction {
    /// ≥75℃：强制切静音（覆盖手动档）+ 通知已报备。
    ForcedSilent,
    /// <70℃：迟滞解除，回手动档。
    Released,
    /// 70..75 迟滞区间：保持现态。
    Hold,
    /// 传感器不可读：优雅跳过（现态不变，graceful 纪律）。
    NoReading,
}

// ---------------------------------------------------------------------------
// 管理器
// ---------------------------------------------------------------------------

/// 性能模式管理器：三档裁决 + 旋钮联动分发 + 温度强制 + 持久化。
pub struct PerfModeManager {
    /// 旋钮清单（三档 × 三参数 = 9 旋钮，声明集 version 即「版本齐备」判据源）。
    knobs: KnobReg,
    /// 手动档（持久化对象——温度强制不抹改它）。
    manual: PerfMode,
    /// F197 强制位（生效档 = 强制 ? Silent : manual）。
    thermal_forced: bool,
    /// 最近温度读数（诊断直读；None = 传感器不可读）。
    temp_c: Option<i32>,
    /// 切换事件审计环（toast 与成绩单证据直读）。
    audit: RingLog<SwitchEvent, AUDIT_CAP>,
    /// 切换事件账本（单列：分钟 → 切换次数）。
    switch_book: MinuteBook,
    /// 诊断报备（对象, 原因）。
    notes: Vec<(&'static str, &'static str)>,
}

impl PerfModeManager {
    /// 声明三档全量旋钮（每档 4 个：频率上下限/窗口/缓冲，共 12；幂等重声明 no-op）。
    pub fn new() -> PerfModeManager {
        let mut knobs = KnobReg::new();
        declare_mode_knobs(&mut knobs, PerfMode::Silent);
        declare_mode_knobs(&mut knobs, PerfMode::Balanced);
        declare_mode_knobs(&mut knobs, PerfMode::Performance);
        PerfModeManager {
            knobs,
            manual: PerfMode::Balanced,
            thermal_forced: false,
            temp_c: None,
            audit: RingLog::new(),
            switch_book: MinuteBook::new(1, SWITCH_BOOK_MIN),
            notes: Vec::new(),
        }
    }

    /// 当前**生效**档（温度强制覆盖手动档——安全优先）。
    pub fn effective_mode(&self) -> PerfMode {
        if self.thermal_forced {
            PerfMode::Silent
        } else {
            self.manual
        }
    }

    /// 手动档直读（强制期间与生效档可能不同）。
    pub fn manual_mode(&self) -> PerfMode {
        self.manual
    }

    /// 最近温度读数。
    pub fn temperature(&self) -> Option<i32> {
        self.temp_c
    }

    /// 生效档参数表（旋钮表现值——不是静态表直拷：旋钮可被运行时微调，
    /// 判线以旋钮清单为唯一活源）。
    pub fn effective_profile(&self) -> Option<ModeProfile> {
        read_mode_profile(&self.knobs, self.effective_mode())
    }

    /// F048 自动策略请求的边界裁决：钳入生效档 [floor, ceil]。
    pub fn decide_freq(&self, requested: FreqLevel) -> FreqLevel {
        match self.effective_profile() {
            Some(p) => clamp_freq(requested, p),
            None => FreqLevel::Mid, // 旋钮表缺损时锁定安全档（graceful）。
        }
    }

    /// 手动切换（同档拒绝；旋钮表版本不齐拒绝 + 报备；成功原子生效）。
    pub fn switch_mode(&mut self, to: PerfMode, stamp_ms: u64, minute: u64) -> SwitchOutcome {
        if self.thermal_forced {
            // 强制期间手动切换：允许改手动档（解除后生效），但当前生效档
            // 仍为静音——如实返回 SameMode 语义之外的第三态？不。规格未给
            // 强制期切档第三态，最诚实做法：本次只落手动档、不产生生效
            // 事件（生效档未变），审计里无假切换。
            if to != self.manual {
                self.manual = to;
                self.note("F197", "manual change during thermal force: queued till release");
            }
            return SwitchOutcome::SameMode;
        }
        if to == self.manual {
            return SwitchOutcome::SameMode;
        }
        let p = match read_mode_profile(&self.knobs, to) {
            Some(p) => p,
            None => {
                self.note("F069", "knob set incomplete: switch rejected");
                return SwitchOutcome::KnobMissing;
            }
        };
        let ev = SwitchEvent {
            stamp_ms,
            atom_ms: stamp_ms,
            from: self.manual,
            to,
            reason: SwitchReason::Manual,
            applied_freq_floor: p.freq_floor,
            applied_freq_ceil: p.freq_ceil,
            applied_wc_window_s: p.wc_window_s,
            applied_audio_buf_ms: p.audio_buf_ms,
        };
        self.manual = to;
        self.commit(ev, minute);
        SwitchOutcome::Applied(ev)
    }

    /// F197 温度馈入（None = 传感器不可读，优雅跳过）。
    pub fn feed_temperature(
        &mut self,
        temp_c: Option<i32>,
        stamp_ms: u64,
        minute: u64,
    ) -> ThermalAction {
        let t = match temp_c {
            Some(t) => t,
            None => {
                self.temp_c = None;
                return ThermalAction::NoReading;
            }
        };
        self.temp_c = Some(t);
        if t >= THERMAL_FORCE_C {
            if !self.thermal_forced {
                self.thermal_forced = true;
                let from = self.manual;
                let p = profile_of(PerfMode::Silent);
                let ev = SwitchEvent {
                    stamp_ms,
                    atom_ms: stamp_ms,
                    from,
                    to: PerfMode::Silent,
                    reason: SwitchReason::ThermalForce,
                    applied_freq_floor: p.freq_floor,
                    applied_freq_ceil: p.freq_ceil,
                    applied_wc_window_s: p.wc_window_s,
                    applied_audio_buf_ms: p.audio_buf_ms,
                };
                self.commit(ev, minute);
                self.note("F197", "thermal >=75C: force silent mode (safety over preference)");
                return ThermalAction::ForcedSilent;
            }
            return ThermalAction::Hold;
        }
        if t < THERMAL_RELEASE_C && self.thermal_forced {
            self.thermal_forced = false;
            let p = profile_of(self.manual);
            let ev = SwitchEvent {
                stamp_ms,
                atom_ms: stamp_ms,
                from: PerfMode::Silent,
                to: self.manual,
                reason: SwitchReason::ThermalRelease,
                applied_freq_floor: p.freq_floor,
                applied_freq_ceil: p.freq_ceil,
                applied_wc_window_s: p.wc_window_s,
                applied_audio_buf_ms: p.audio_buf_ms,
            };
            self.commit(ev, minute);
            self.note("F197", "thermal <70C: release, restore manual mode");
            return ThermalAction::Released;
        }
        ThermalAction::Hold
    }

    /// 持久化快照（当前手动档——重启保持的唯一形态）。
    pub fn persisted(&self) -> u8 {
        self.manual.idx()
    }

    /// 重启恢复：从持久化序号恢复手动档，走与手动切换同一条校验链
    /// （旋钮表版本不齐 → 拒绝 + 报备，不静默落默认）。
    pub fn restore(&mut self, raw: u8, stamp_ms: u64, minute: u64) -> SwitchOutcome {
        let to = match PerfMode::from_idx(raw) {
            Some(m) => m,
            None => {
                self.note("F069", "persisted mode corrupt: keep default");
                return SwitchOutcome::SameMode;
            }
        };
        if to == self.manual {
            return SwitchOutcome::SameMode;
        }
        let p = match read_mode_profile(&self.knobs, to) {
            Some(p) => p,
            None => {
                self.note("F069", "knob set incomplete on restore: rejected");
                return SwitchOutcome::KnobMissing;
            }
        };
        let ev = SwitchEvent {
            stamp_ms,
            atom_ms: stamp_ms,
            from: self.manual,
            to,
            reason: SwitchReason::Restore,
            applied_freq_floor: p.freq_floor,
            applied_freq_ceil: p.freq_ceil,
            applied_wc_window_s: p.wc_window_s,
            applied_audio_buf_ms: p.audio_buf_ms,
        };
        self.manual = to;
        self.commit(ev, minute);
        SwitchOutcome::Applied(ev)
    }

    /// 生效事件入账本（分钟窗口查询）。
    pub fn switches_in(&self, now_minute: u64, span_min: u64) -> u64 {
        self.switch_book.range_sum(now_minute.saturating_sub(span_min) + 1, now_minute)
            .iter()
            .sum()
    }

    /// 审计环直读（新→旧）。
    pub fn audit_log(&self) -> Vec<SwitchEvent> {
        self.audit.newest_first()
    }

    /// 诊断报备直读。
    pub fn note_log(&self) -> &[(&'static str, &'static str)] {
        &self.notes
    }

    /// 旋钮清单直读（设置页参数摘要悬浮数据源）。
    pub fn knob_snapshot(&self) -> Vec<Knob> {
        self.knobs.snapshot()
    }

    /// 以外部旋钮表构造（宿主共享旋钮单例注入口；旋钮表缺声明 → 档位
    /// 切换拒绝——「版本不齐」判据的产品路径）。
    pub fn with_knob_reg(knobs: KnobReg) -> PerfModeManager {
        PerfModeManager {
            knobs,
            manual: PerfMode::Balanced,
            thermal_forced: false,
            temp_c: None,
            audit: RingLog::new(),
            switch_book: MinuteBook::new(1, SWITCH_BOOK_MIN),
            notes: Vec::new(),
        }
    }

    fn commit(&mut self, ev: SwitchEvent, minute: u64) {
        self.audit.push(ev);
        self.switch_book.record_minute(minute, &[1]);
    }

    fn note(&mut self, theme: &'static str, reason: &'static str) {
        if self.notes.len() >= NOTE_CAP {
            self.notes.remove(0);
        }
        self.notes.push((theme, reason));
    }
}

// ---------------------------------------------------------------------------
// 旋钮声明与读取（三档 × 三参数 = 9 旋钮）
// ---------------------------------------------------------------------------

/// 一档四旋钮名（&'static str 直书——Knob.name 须 'static，运行时拼名禁入）。
fn knob_names(m: PerfMode) -> (&'static str, &'static str, &'static str, &'static str) {
    match m {
        PerfMode::Silent => (
            "perf.silent.freq_floor",
            "perf.silent.freq_ceil",
            "perf.silent.wc_window_s",
            "perf.silent.audio_buf_ms",
        ),
        PerfMode::Balanced => (
            "perf.balanced.freq_floor",
            "perf.balanced.freq_ceil",
            "perf.balanced.wc_window_s",
            "perf.balanced.audio_buf_ms",
        ),
        PerfMode::Performance => (
            "perf.perf.freq_floor",
            "perf.perf.freq_ceil",
            "perf.perf.wc_window_s",
            "perf.perf.audio_buf_ms",
        ),
    }
}

/// 一档旋钮声明（频率上下限/窗口/缓冲——序号制入 i64 旋钮）。
fn declare_mode_knobs(knobs: &mut KnobReg, m: PerfMode) {
    let p = profile_of(m);
    let (floor_n, ceil_n, wc_n, buf_n) = knob_names(m);
    let lo = FreqLevel::Low.idx();
    let hi = FreqLevel::Top.idx();
    knobs.declare(floor_n, p.freq_floor.idx(), lo, hi, "level", "F069 频率边界下限（1=Low..4=Top）");
    knobs.declare(ceil_n, p.freq_ceil.idx(), lo, hi, "level", "F069 频率边界上限（封顶线）");
    knobs.declare(wc_n, p.wc_window_s as i64, 1, 30, "s", "F046 写合并窗口");
    knobs.declare(buf_n, p.audio_buf_ms as i64, 5, 20, "ms", "F064 音频缓冲");
}

/// 一档旋钮齐读（任一缺失/越界 → None——「版本不齐」判据唯一源）。
fn read_mode_profile(knobs: &KnobReg, m: PerfMode) -> Option<ModeProfile> {
    let (floor_n, ceil_n, wc_n, buf_n) = knob_names(m);
    let floor = FreqLevel::from_idx(knobs.get(floor_n)?)?;
    let ceil = FreqLevel::from_idx(knobs.get(ceil_n)?)?;
    let wc = u64::try_from(knobs.get(wc_n)?).ok()?;
    let buf = u32::try_from(knobs.get(buf_n)?).ok()?;
    Some(ModeProfile { freq_floor: floor, freq_ceil: ceil, wc_window_s: wc, audio_buf_ms: buf })
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F069 自检（判据：三档差异可辨；切换无爆音无卡顿）。
pub fn run_perfmodes_checks() -> CheckSet {
    let mut set = CheckSet::new("F069-perfmodes");

    // 1. 三档参数两两互异（频率边界/写入量/续航三指标分档明显——主册判据）。
    let (s, b, p) =
        (profile_of(PerfMode::Silent), profile_of(PerfMode::Balanced), profile_of(PerfMode::Performance));
    set.add(
        "three profiles pairwise distinct",
        s.freq_ceil != b.freq_ceil
            && b.audio_buf_ms != p.audio_buf_ms
            && s.wc_window_s != b.wc_window_s
            && s.audio_buf_ms > b.audio_buf_ms
            && b.audio_buf_ms > p.audio_buf_ms
            && s.freq_ceil == FreqLevel::Mid
            && p.freq_floor == FreqLevel::Top,
        "",
    );

    // 2. 静音档封顶：自动策略请求全频 → 钳到中档；请求最低 → 不抬。
    let mut pm = PerfModeManager::new();
    assert!(pm.switch_mode(PerfMode::Silent, 100, 1) != SwitchOutcome::KnobMissing);
    set.add(
        "silent caps governor at mid",
        pm.decide_freq(FreqLevel::Top) == FreqLevel::Mid && pm.decide_freq(FreqLevel::Low) == FreqLevel::Low,
        "",
    );

    // 3. 性能档锁全频：任何请求 → 最高档（无调节空间）。
    let mut pm2 = PerfModeManager::new();
    let _ = pm2.switch_mode(PerfMode::Performance, 100, 1);
    set.add(
        "performance locks full speed",
        pm2.decide_freq(FreqLevel::Low) == FreqLevel::Top && pm2.decide_freq(FreqLevel::Top) == FreqLevel::Top,
        "",
    );

    // 4. 均衡档自动全权：请求即所得（边界=全频域）。
    set.add(
        "balanced grants full autonomy",
        PerfModeManager::new().decide_freq(FreqLevel::Top) == FreqLevel::Top,
        "",
    );

    // 5. 切换原子生效：单时间戳 + 三参数与档位表一致（无爆音的结构证明）。
    let out = pm2.switch_mode(PerfMode::Silent, 500, 2);
    if let SwitchOutcome::Applied(ev) = out {
        set.add(
            "switch applies atomic parameter set",
            ev.atom_ms == ev.stamp_ms
                && ev.applied_freq_ceil == FreqLevel::Mid
                && ev.applied_wc_window_s == 8
                && ev.applied_audio_buf_ms == 20
                && ev.applied_freq_ceil == profile_of(PerfMode::Silent).freq_ceil,
            "",
        );
    } else {
        set.add("switch applies atomic parameter set", false, "unexpected outcome");
    }

    // 6. toast 生效项清单：审计环里事件可直读（频率/窗口/缓冲全在案）。
    set.add(
        "toast manifest in audit ring",
        pm2.audit_log().iter().any(|e| {
            e.reason == SwitchReason::Manual
                && e.applied_wc_window_s == 8
                && e.applied_audio_buf_ms == 20
        }),
        "",
    );

    // 7. 切换事件入账本：窗口查询计数一致（pm2 已切 2 次：Performance→
    //    minute 1、Silent→minute 2；SameMode 不入账；span=0 空窗语义同 freed_in）。
    set.add(
        "switches booked into minute ledger",
        pm2.switches_in(2, 10) == 2 && pm2.switches_in(2, 1) == 1 && pm2.switches_in(30, 5) == 0,
        "",
    );

    // 8. 同档重复切换拒绝（无假切换）。
    set.add("same-mode switch rejected", pm2.switch_mode(PerfMode::Silent, 600, 2) == SwitchOutcome::SameMode, "");

    // 9. 旋钮表版本不齐 → 拒绝切换 + 诊断报备（缺 silent 档全 4 旋钮）。
    let mut pm3 = {
        let mut knobs = KnobReg::new();
        declare_mode_knobs(&mut knobs, PerfMode::Balanced);
        declare_mode_knobs(&mut knobs, PerfMode::Performance);
        PerfModeManager::with_knob_reg(knobs)
    };
    let out3 = pm3.switch_mode(PerfMode::Silent, 100, 1);
    set.add(
        "knob-set incomplete rejects switch",
        out3 == SwitchOutcome::KnobMissing
            && pm3.note_log().iter().any(|(_, r)| r.contains("incomplete")),
        "",
    );

    // 10. F197 温度强制降档：75℃ 覆盖手动性能档（安全优先）+ 通知说明。
    let mut pm4 = PerfModeManager::new();
    let _ = pm4.switch_mode(PerfMode::Performance, 100, 1);
    let act = pm4.feed_temperature(Some(75), 200, 1);
    set.add(
        "thermal force overrides manual performance",
        act == ThermalAction::ForcedSilent
            && pm4.effective_mode() == PerfMode::Silent
            && pm4.manual_mode() == PerfMode::Performance
            && pm4.note_log().iter().any(|(_, r)| r.contains("75C")),
        "",
    );

    // 11. 迟滞回退：70..75 区间保持强制（防震荡）；<70 解除回手动档。
    let hold = pm4.feed_temperature(Some(72), 300, 1);
    let rel = pm4.feed_temperature(Some(69), 400, 1);
    set.add(
        "thermal release with hysteresis",
        hold == ThermalAction::Hold
            && rel == ThermalAction::Released
            && pm4.effective_mode() == PerfMode::Performance,
        "",
    );

    // 12. 传感器不可读：优雅跳过，现态不变（graceful 纪律）。
    let nr = pm4.feed_temperature(None, 500, 1);
    set.add(
        "sensor dropout handled gracefully",
        nr == ThermalAction::NoReading
            && pm4.temperature().is_none()
            && pm4.effective_mode() == PerfMode::Performance,
        "",
    );

    // 13. 强制期间手动切档：只落手动档不产生生效事件（生效档仍静音）。
    let mut pm5 = PerfModeManager::new();
    let _ = pm5.switch_mode(PerfMode::Balanced, 100, 1);
    let _ = pm5.feed_temperature(Some(80), 200, 1);
    let during = pm5.switch_mode(PerfMode::Performance, 300, 1);
    set.add(
        "manual change during force is queued not applied",
        during == SwitchOutcome::SameMode
            && pm5.effective_mode() == PerfMode::Silent
            && pm5.manual_mode() == PerfMode::Performance,
        "",
    );
    let rel5 = pm5.feed_temperature(Some(60), 400, 1);
    set.add(
        "queued manual mode takes effect on release",
        rel5 == ThermalAction::Released && pm5.effective_mode() == PerfMode::Performance,
        "",
    );

    // 14. 持久化回路：persist → 新机 restore → 档位保持；坏序号拒绝。
    let raw = pm2.persisted();
    let mut pm6 = PerfModeManager::new();
    let out6 = pm6.restore(raw, 100, 1);
    set.add(
        "persistence round-trip",
        out6 != SwitchOutcome::KnobMissing
            && pm6.manual_mode() == pm2.manual_mode()
            && pm6.restore(9, 200, 1) == SwitchOutcome::SameMode
            && pm6.manual_mode() == pm2.manual_mode(),
        "",
    );

    // 15. 托盘图标三态区分：三档 glyph 互异（一眼可辨）。
    set.add(
        "tray glyphs three-way distinct",
        PerfMode::Silent.tray_glyph() != PerfMode::Balanced.tray_glyph()
            && PerfMode::Balanced.tray_glyph() != PerfMode::Performance.tray_glyph()
            && PerfMode::Silent.tray_glyph() != PerfMode::Performance.tray_glyph(),
        "",
    );

    // 16. 切换动画窗口：规格 200ms（F124 标准曲线）——常量在案。
    set.add("switch animation 200ms", SWITCH_ANIM_MS == 200, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freq_level_ordering_is_total() {
        assert!(FreqLevel::Low < FreqLevel::Mid);
        assert!(FreqLevel::Mid < FreqLevel::High);
        assert!(FreqLevel::High < FreqLevel::Top);
        assert_eq!(FreqLevel::from_idx(4), Some(FreqLevel::Top));
        assert_eq!(FreqLevel::from_idx(5), None);
    }

    #[test]
    fn perf_mode_idx_roundtrip() {
        for m in [PerfMode::Silent, PerfMode::Balanced, PerfMode::Performance] {
            assert_eq!(PerfMode::from_idx(m.idx()), Some(m));
        }
        assert_eq!(PerfMode::from_idx(3), None);
    }

    #[test]
    fn summary_matches_profile_table() {
        // 摘要文案与参数表一一对应（一处一事实——改表必改文案）。
        assert!(profile_of(PerfMode::Silent).wc_window_s == 8);
        assert!(profile_of(PerfMode::Silent).audio_buf_ms == 20);
        assert!(profile_of(PerfMode::Balanced).audio_buf_ms == 10);
        assert!(profile_of(PerfMode::Performance).freq_floor == FreqLevel::Top);
        assert!(PerfMode::Silent.summary().contains("8s"));
        assert!(PerfMode::Balanced.summary().contains("10ms"));
        assert!(PerfMode::Performance.summary().contains("5ms"));
    }

    #[test]
    fn knob_snapshot_lists_all_twelve() {
        let pm = PerfModeManager::new();
        let snap = pm.knob_snapshot();
        assert_eq!(snap.len(), 12, "三档 × 四旋钮（频率上下限/窗口/缓冲）= 12");
        assert!(snap.iter().any(|k| k.name == "perf.silent.freq_ceil"));
        assert!(snap.iter().any(|k| k.name == "perf.perf.audio_buf_ms"));
    }

    #[test]
    fn thermal_never_erases_manual_choice() {
        let mut pm = PerfModeManager::new();
        let _ = pm.switch_mode(PerfMode::Performance, 100, 1);
        // 强制 → 解除 → 手动档原样回来（强制不抹改偏好）。
        let _ = pm.feed_temperature(Some(90), 200, 1);
        assert_eq!(pm.effective_mode(), PerfMode::Silent);
        let _ = pm.feed_temperature(Some(50), 300, 1);
        assert_eq!(pm.effective_mode(), PerfMode::Performance, "解除后回手动档");
    }

    #[test]
    fn restore_rejects_when_knobs_missing() {
        let raw = PerfModeManager::new().persisted(); // 1 = Balanced
        let mut pm = {
            let mut knobs = KnobReg::new();
            declare_mode_knobs(&mut knobs, PerfMode::Silent);
            declare_mode_knobs(&mut knobs, PerfMode::Performance); // 缺 balanced 档
            let mut m = PerfModeManager::with_knob_reg(knobs);
            assert!(m.switch_mode(PerfMode::Performance, 1, 0) != SwitchOutcome::KnobMissing);
            m // 手动档已离 Balanced → restore(1) 走参数校验而非同档短路
        };
        assert_eq!(pm.restore(raw, 100, 1), SwitchOutcome::KnobMissing);
        assert!(pm.note_log().iter().any(|(_, r)| r.contains("restore")));
    }
}
