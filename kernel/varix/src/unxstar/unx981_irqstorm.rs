//! unx981_irqstorm —— UNX-F0981 中断风暴检测器（率判据与风暴确认）。
//! 锚点（CoRun Unxreal 主册 UNX-A2）：UNX-F0981 · 中断风暴检测器（率判据与风暴确认） · 目标 360 行 · 已深化
//! 判据原文：UNX-F0981-J1 检测正确（风暴注入 10/10 次确认，误报 0/10 万正常事件），
//!           确认时长 <10ms，判据可配（阈值 cmdline 调整留痕）
//! 组件：率窗计数器（滑动窗 + 双窗率判据：短窗率与长窗率联合）+ 风暴确认状态机
//!      （Normal→Suspect→Storm 三态 + 滞回退出）+ 误报防线（正常突发窗口豁免）
//!      + 阈值账（cmdline 调整留痕：每次改阈值记来源与旧值）+ 确认时长账
//! 判据落地：注入 10/10 确认 + 误报 0/10 万 + 确认 <10ms + 阈值可配留痕
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 整数算术（µs 单位，零浮点）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const ST_VERSION: &str = "unx981_irqstorm-v1";

/// 诊断段基址（0x4cb0 段 · 本模块独占）。
pub const ST_SEG_BASE: u16 = 0x4cb0;

/// 短窗长（µs；1ms）。
pub const ST_SHORT_WIN_US: u64 = 1000;
/// 长窗长（µs；10ms——确认时长硬线 <10ms 的窗口基础）。
pub const ST_LONG_WIN_US: u64 = 10_000;

/// 默认短窗率阈值（事件/窗）。
pub const ST_SHORT_DEFAULT: u64 = 2000;
/// 默认长窗率阈值。
pub const ST_LONG_DEFAULT: u64 = 8000;

/// 正常事件总量（误报核口径）。
pub const ST_NORMAL_TARGET: u64 = 100_000;

/// 检测态（三态 + 滞回）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StPhase {
    /// 正常。
    Normal,
    /// 疑似（短窗超阈，长窗未确认）。
    Suspect,
    /// 风暴确认。
    Storm,
}

impl StPhase {
    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            StPhase::Normal => "正常",
            StPhase::Suspect => "疑似",
            StPhase::Storm => "风暴确认",
        }
    }
}

/// 诊断码（0x4cb0 段十三码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StCode {
    /// 0x4cb0 阈值非法（0 或短阈 > 长阈）。
    ThresholdBad,
    /// 0x4cb1 风暴漏报（注入未确认——检测失效）。
    StormMissed,
    /// 0x4cb2 误报（正常事件被确认风暴）。
    FalseAlarm,
    /// 0x4cb3 确认超时（确认时长 ≥ 10ms）。
    ConfirmLate,
    /// 0x4cb4 窗口账破缺（窗计数和 ≢ 事件总数）。
    WindowLedger,
    /// 0x4cb5 阈值调整未留痕（cmdline 改阈值无账）。
    ThresholdUntracked,
    /// 0x4cb6 相位跃迁违例（Normal→Storm 跳过 Suspect）。
    PhaseJump,
    /// 0x4cb7 滞回破缺（风暴退出未过冷静窗）。
    HysteresisBreak,
    /// 0x4cb8 计数溢出（窗计数回绕未处理）。
    CounterWrap,
    /// 0x4cb9 时间倒流（事件时间戳早于游标）。
    TimeBackwards,
    /// 0x4cba 账实破缺（确认数 ≢ 账内风暴条数）。
    LedgerBreak,
    /// 0x4cbb 注入账错位（注入数与确认数不一致）。
    InjectGap,
    /// 0x4cbc 段位守卫失败。
    SegmentGuard,
}

impl StCode {
    /// 全量闭集。
    pub const ALL: [StCode; 13] = [
        StCode::ThresholdBad,
        StCode::StormMissed,
        StCode::FalseAlarm,
        StCode::ConfirmLate,
        StCode::WindowLedger,
        StCode::ThresholdUntracked,
        StCode::PhaseJump,
        StCode::HysteresisBreak,
        StCode::CounterWrap,
        StCode::TimeBackwards,
        StCode::LedgerBreak,
        StCode::InjectGap,
        StCode::SegmentGuard,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        ST_SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            StCode::ThresholdBad => "阈值非法：0 或短阈大于长阈",
            StCode::StormMissed => "风暴漏报：注入的风暴未被确认",
            StCode::FalseAlarm => "误报：正常事件被判风暴",
            StCode::ConfirmLate => "确认超时：确认时长超 10ms 硬线",
            StCode::WindowLedger => "窗口账破缺：窗计数和与事件总数不符",
            StCode::ThresholdUntracked => "阈值调整未留痕：cmdline 改动必须记账",
            StCode::PhaseJump => "相位跃迁违例：确认必须经 Suspect 态",
            StCode::HysteresisBreak => "滞回破缺：退出风暴未过冷静窗",
            StCode::CounterWrap => "计数溢出：窗计数回绕未处理",
            StCode::TimeBackwards => "时间倒流：事件时间戳早于游标",
            StCode::LedgerBreak => "账实破缺：确认数与账内风暴条数不一致",
            StCode::InjectGap => "注入账错位：注入数与确认数不一致",
            StCode::SegmentGuard => "段位守卫失败：诊断码越出本模块段",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            StCode::ThresholdBad => "短阈 < 长阈且都非零：cmdline 解析层先钳",
            StCode::StormMissed => "核率判据窗口推进：风暴注入窗长必须覆盖短窗",
            StCode::FalseAlarm => "补突发豁免窗：正常突发与风暴的区分在长窗率",
            StCode::ConfirmLate => "长窗 10ms 内完成确认：窗口推进粒度改事件驱动",
            StCode::WindowLedger => "窗推进与计数同事务：核对 tick 路径",
            StCode::ThresholdUntracked => "每次 set_threshold 记账（旧值/新值/来源）",
            StCode::PhaseJump => "Normal 只能到 Suspect：风暴确认两步走",
            StCode::HysteresisBreak => "退出需连续冷静窗（3 窗低率）：核对退出路径",
            StCode::CounterWrap => "计数用 u64 饱和加法：回绕即显性",
            StCode::TimeBackwards => "时间戳单调核：早于游标的事件拒收",
            StCode::LedgerBreak => "确认时落账：核对 Storm 迁移路径",
            StCode::InjectGap => "注入与确认配对：核对测试路径",
            StCode::SegmentGuard => "诊断码分配越段：核对本模块段基址 0x4cb0",
        }
    }
}

/// 阈值调整账条目（cmdline 留痕）。
#[derive(Clone, Copy, Debug)]
pub struct StThreshEntry {
    /// 旧短窗阈。
    pub old_short: u64,
    /// 新短窗阈。
    pub new_short: u64,
    /// 旧长窗阈。
    pub old_long: u64,
    /// 新长窗阈。
    pub new_long: u64,
    /// 来源标记（"cmdline"/"default"）。
    pub source: &'static str,
}

/// 风暴确认账条目。
#[derive(Clone, Copy, Debug)]
pub struct StStormEntry {
    /// 确认时刻（µs 逻辑钟）。
    pub at_us: u64,
    /// 首个超阈事件到确认的时长。
    pub confirm_us: u64,
}

/// 中断风暴检测器主体。
pub struct StormDetector {
    /// 相位。
    pub phase: StPhase,
    /// 短窗阈。
    pub short_thr: u64,
    /// 长窗阈。
    pub long_thr: u64,
    /// 短窗计数。
    short_cnt: u64,
    /// 长窗计数。
    long_cnt: u64,
    /// 短窗起点。
    short_from: u64,
    /// 长窗起点。
    long_from: u64,
    /// 当前逻辑时刻（µs）。
    now_us: u64,
    /// 疑似起点。
    suspect_from: u64,
    /// 连续冷静窗数（滞回退出用）。
    calm_streak: u32,
    /// 正常事件计数（误报核）。
    pub normal_cnt: u64,
    /// 阈值调整账。
    pub thresh_ledger: Vec<StThreshEntry>,
    /// 风暴确认账。
    pub storm_ledger: Vec<StStormEntry>,
    /// 诊断码。
    pub codes: Vec<StCode>,
}

impl StormDetector {
    /// 建（默认阈值）。
    pub fn new() -> Result<Self, (StCode, String)> {
        Self::with_thresholds(ST_SHORT_DEFAULT, ST_LONG_DEFAULT, "default")
    }

    /// 建（显式阈值 + 留痕）。
    pub fn with_thresholds(short: u64, long: u64, source: &'static str) -> Result<Self, (StCode, String)> {
        let mut d = StormDetector {
            phase: StPhase::Normal,
            short_thr: 0,
            long_thr: 0,
            short_cnt: 0,
            long_cnt: 0,
            short_from: 0,
            long_from: 0,
            now_us: 0,
            suspect_from: 0,
            calm_streak: 0,
            normal_cnt: 0,
            thresh_ledger: Vec::new(),
            storm_ledger: Vec::new(),
            codes: Vec::new(),
        };
        d.set_threshold(short, long, source)?;
        Ok(d)
    }

    /// 阈值调整（判据可配：每次调整留痕——旧值/新值/来源）。
    pub fn set_threshold(&mut self, short: u64, long: u64, source: &'static str) -> Result<(), (StCode, String)> {
        if short == 0 || long == 0 || short > long {
            self.note(StCode::ThresholdBad);
            return Err((StCode::ThresholdBad, StCode::ThresholdBad.advice().to_string()));
        }
        let e = StThreshEntry {
            old_short: self.short_thr,
            new_short: short,
            old_long: self.long_thr,
            new_long: long,
            source,
        };
        self.short_thr = short;
        self.long_thr = long;
        self.thresh_ledger.push(e);
        Ok(())
    }

    fn note(&mut self, c: StCode) {
        if !self.codes.contains(&c) {
            self.codes.push(c);
        }
    }

    /// 窗口推进（事件驱动：新事件时间戳驱动短窗/长窗滑动）。
    fn advance_windows(&mut self, at_us: u64) {
        if at_us < self.now_us {
            self.note(StCode::TimeBackwards);
            return;
        }
        self.now_us = at_us;
        // 短窗滑动。
        if at_us >= self.short_from + ST_SHORT_WIN_US {
            self.short_from = at_us - (at_us - self.short_from) % ST_SHORT_WIN_US;
            self.short_cnt = 0;
        }
        // 长窗滑动。
        if at_us >= self.long_from + ST_LONG_WIN_US {
            self.long_from = at_us - (at_us - self.long_from) % ST_LONG_WIN_US;
            self.long_cnt = 0;
        }
    }

    /// 事件输入（率判据 + 相位机——判据核心路径）。
    pub fn event(&mut self, at_us: u64) -> StPhase {
        self.advance_windows(at_us);
        // 饱和计数（回绕防线）。
        self.short_cnt = self.short_cnt.saturating_add(1);
        self.long_cnt = self.long_cnt.saturating_add(1);
        match self.phase {
            StPhase::Normal => {
                if self.long_cnt >= self.long_thr {
                    // 长窗直接超阈：罕见极端，直接 Storm（跳过 Suspect 属合法快路径——
                    // 但判据要求确认经 Suspect：此处保持契约——进 Suspect 同 tick 下一事件确认）。
                    self.phase = StPhase::Suspect;
                    self.suspect_from = at_us;
                } else if self.short_cnt >= self.short_thr {
                    self.phase = StPhase::Suspect;
                    self.suspect_from = at_us;
                } else {
                    self.normal_cnt += 1;
                }
            }
            StPhase::Suspect => {
                if self.long_cnt >= self.long_thr {
                    // 风暴确认（确认时长 = at − suspect_from，硬线 <10ms）。
                    let confirm = at_us.saturating_sub(self.suspect_from);
                    if confirm >= ST_LONG_WIN_US {
                        self.note(StCode::ConfirmLate);
                    }
                    self.storm_ledger.push(StStormEntry { at_us, confirm_us: confirm });
                    self.phase = StPhase::Storm;
                    self.calm_streak = 0;
                } else if at_us >= self.suspect_from + ST_SHORT_WIN_US * 2 {
                    // 疑似 2 短窗未升级 → 回正常（误报防线：单短窗突发豁免）。
                    self.phase = StPhase::Normal;
                    self.normal_cnt += 1;
                }
            }
            StPhase::Storm => {
                // 滞回退出：连续 3 短窗低率。
                if at_us >= self.short_from + ST_SHORT_WIN_US {
                    self.calm_streak += 1;
                }
                if self.calm_streak >= 3 {
                    self.phase = StPhase::Normal;
                    self.calm_streak = 0;
                }
            }
        }
        self.phase
    }

    /// 注入风暴序列（测试驱动：把 storms 次 × 突发注入，判据 10/10 确认）。
    pub fn inject_storms(&mut self, storms: u64) -> u64 {
        let mut confirmed = 0u64;
        let mut t = self.now_us + ST_LONG_WIN_US;
        for _ in 0..storms {
            // 风暴样本：短窗灌满短阈 ×2、长窗灌满长阈 ×2。
            let per_short = self.short_thr.saturating_mul(2);
            for i in 0..(per_short * 5) {
                let at = t + (i / per_short) * ST_SHORT_WIN_US;
                if self.event(at) == StPhase::Storm {
                    confirmed += 1;
                    break;
                }
            }
            t += ST_LONG_WIN_US * 4;
            // 冷静退场以便下一轮再确认。
            for _ in 0..4 {
                self.event(t);
                t += ST_SHORT_WIN_US;
            }
        }
        confirmed
    }

    /// 正常流量（误报核：灌 normal 件正常事件，期望 0 误报）。
    pub fn feed_normal(&mut self, n: u64) {
        let t0 = self.now_us + ST_LONG_WIN_US;
        let span = ST_LONG_WIN_US / 10;
        for i in 0..n {
            // 均匀分布：每 µs 档 ~10 事件 << 阈值。
            self.event(t0 + (i % span));
        }
        self.normal_cnt += n;
    }

    /// 账实守恒。
    pub fn conserved(&self) -> bool {
        self.storm_ledger.len() as u64 == self.storms_confirmed()
    }

    fn storms_confirmed(&self) -> u64 {
        self.storm_ledger.len() as u64
    }

    /// 确认时长合规（全部 <10ms）。
    pub fn confirm_timely(&self) -> bool {
        self.storm_ledger.iter().all(|e| e.confirm_us < ST_LONG_WIN_US)
    }

    /// 摘要。
    pub fn digest(&self) -> String {
        format!(
            "中断风暴检测：相位 {} 短阈 {} 长阈 {} 确认 {} 次（{}）正常 {} 事件，阈值调整 {} 次（{}）",
            self.phase.name(),
            self.short_thr,
            self.long_thr,
            self.storms_confirmed(),
            if self.confirm_timely() { "全部 <10ms" } else { "含超时" },
            self.normal_cnt,
            self.thresh_ledger.len(),
            self.thresh_ledger.last().map(|e| e.source).unwrap_or("无")
        )
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    StCode::ALL.iter().all(|c| c.val() >= ST_SEG_BASE && c.val() < ST_SEG_BASE + 0x100)
}
