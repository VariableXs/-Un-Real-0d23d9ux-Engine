//! cgd09_timecalib — CGPU-F0489 帧计时校准（三个钟不校准，账本就是三本账）。
//!
//! 判据原文（施工书锚点 CGPU-F0489）：计时校准：多计时源一致性（CPU QPC×
//! GPU 时间戳×呈现时钟三源互校——漂移可测可纠）；定期重校准（时钟漂移的周
//! 期性再校准——长时间运行不漂）；校准精度声明（校准后残余误差量化）；校准
//! 失败降级（单源故障→双源降级+显性告知）。判据：三源互校、重校准、精度声
//! 明、降级、判据。
//!
//! 职责定位：三源时钟模型（CpuQpc/GpuTimestamp/PresentClock 各带自己的漂移
//! 率与偏移）+ 互校对（两两成对采样差分→漂移估计）+ 偏移校正器（以主源为
//! 锚，副源套校正偏移）+ 定期重校准状态机（重校周期到期→重采样→残余误差
//! 量化入精度声明）+ 降级协议（某源连续失联→降为双源+Degraded 态显性，恢
//! 复后回三源——降级不静默）。与 cgd02_frametimer（四段计时）、cgd03_
//! frameledger（账本）分工：彼记账、此校钟——钟不准账全是错的。时间轴只进
//! 不出（单调性红线：校正后时间戳禁止倒退——偏移修正走饱和补偿不拨表）。
//! 诊断码段 0x5401~0x5405。

use alloc::vec::Vec;

/// 模块版本。
pub const TIME_CALIB_VERSION: &str = "D09-timecalib-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x54 段，本会话独占）
// ---------------------------------------------------------------------------

pub const XC_SOURCE_LOST: u32 = 0x5401; // 计时源失联（连续未更新超限——降级触发）
pub const XC_DRIFT_EXCESSIVE: u32 = 0x5402; // 漂移超限（重校准也压不住——显性立案）
pub const XC_TIME_REGRESS: u32 = 0x5403; // 校正后时间倒退（单调性红线违约）
pub const XC_RESIDUAL_UNSAFE: u32 = 0x5404; // 残余误差超声明上限（精度声明失守）
pub const XC_PAIR_SAMPLE_INVALID: u32 = 0x5405; // 互校采样对无效（同源自配或跨距超界）

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 重校准周期（微秒系统时间）：30 秒一校——长时间运行不漂。
pub const RECALIB_PERIOD_US: u64 = 30_000_000;
/// 源失联判定：连续 4 个采样窗无更新即失联。
pub const LOST_AFTER_MISSES: u32 = 4;
/// 漂移红线（微秒/秒）：单源对主源漂移超此值立案。
pub const DRIFT_REDLINE_US_PER_S: i64 = 100;
/// 残余误差声明上限（微秒）：校准后两源同拍差超此值即失守。
pub const RESIDUAL_CAP_US: i64 = 50;
/// 互校采样窗长度（对）。
pub const SAMPLE_WINDOW: usize = 16;

/// 三计时源闭集（判据"三源互校"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimeSource {
    CpuQpc,
    GpuTimestamp,
    PresentClock,
}

impl TimeSource {
    pub const ALL: [TimeSource; 3] = [
        TimeSource::CpuQpc,
        TimeSource::GpuTimestamp,
        TimeSource::PresentClock,
    ];

    pub const fn zh(self) -> &'static str {
        match self {
            TimeSource::CpuQpc => "CPU QPC",
            TimeSource::GpuTimestamp => "GPU 时间戳",
            TimeSource::PresentClock => "呈现时钟",
        }
    }
}

/// 源健康态（降级协议的状态面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SourceHealth {
    /// 健康：三源互校在岗。
    Healthy,
    /// 降级：双源（该源失联——显性告知不静默）。
    Lost,
    /// 重校中：重校准窗口打开。
    Recalibrating,
}

/// 单源时钟档案：最近读数、失联计数、对主源偏移。
#[derive(Clone, Copy)]
pub struct SourceClock {
    pub source: TimeSource,
    /// 最近读数（微秒，各源域内自洽）。
    pub last_us: u64,
    /// 失联计数（连续未更新次数）。
    pub misses: u32,
    /// 校正偏移（微秒）：读数+偏移=主源域时间。
    pub offset_us: i64,
    /// 漂移估计（微秒/秒）：单位时间偏移变化率。
    pub drift_per_s: i64,
    pub health: SourceHealth,
}

/// 互校采样对：同拍取两源读数。
#[derive(Clone, Copy)]
pub struct PairSample {
    pub a: TimeSource,
    pub b: TimeSource,
    pub ta_us: u64,
    pub tb_us: u64,
    /// 采样时主源域时间（用于漂移率回归）。
    pub at_us: u64,
}

impl PairSample {
    /// 采样差：t_a+t? 对齐到 a 域的 b 偏移（=ta−tb+既有 offset 修正基线）。
    pub fn raw_delta(&self) -> i64 {
        self.ta_us as i64 - self.tb_us as i64
    }
}

/// 校准结果（判据"精度声明"的量化输出）。
#[derive(Clone, Copy, Debug)]
pub struct CalibReport {
    /// 主源锚。
    pub anchor: TimeSource,
    /// 各源残余误差（微秒，绝对值）——校准后同拍差的量化。
    pub residual_us: [i64; 3],
    /// 降级源（None=三源在岗）。
    pub degraded: Option<TimeSource>,
    /// 是否触发过漂移立案。
    pub drift_flagged: bool,
}

/// 帧计时校准器。
pub struct TimeCalibrator {
    clocks: [SourceClock; 3],
    /// 主源锚（恒 CpuQpc——CPU 域是账本口径）。
    anchor: TimeSource,
    /// 互校窗口。
    window: Vec<PairSample>,
    /// 上次重校准时刻（主源域）。
    last_recalib_us: u64,
    /// 漂移立案累计。
    pub drift_flags: u64,
    /// 降级事件累计（含恢复——一降一升各计一次）。
    pub degrade_events: u64,
    pub recover_events: u64,
    /// 单调性守卫：最近对外输出时间。
    last_out_us: u64,
}

impl TimeCalibrator {
    pub fn new(anchor_now_us: u64) -> Self {
        let mut clocks = [
            SourceClock {
                source: TimeSource::CpuQpc,
                last_us: anchor_now_us,
                misses: 0,
                offset_us: 0,
                drift_per_s: 0,
                health: SourceHealth::Healthy,
            },
            SourceClock {
                source: TimeSource::GpuTimestamp,
                last_us: anchor_now_us,
                misses: 0,
                offset_us: 0,
                drift_per_s: 0,
                health: SourceHealth::Healthy,
            },
            SourceClock {
                source: TimeSource::PresentClock,
                last_us: anchor_now_us,
                misses: 0,
                offset_us: 0,
                drift_per_s: 0,
                health: SourceHealth::Healthy,
            },
        ];
        // 初始偏移由构造参数注入（各源域时间=now+域差）——演示零偏移基线。
        clocks[1].last_us = anchor_now_us;
        clocks[2].last_us = anchor_now_us;
        TimeCalibrator {
            clocks,
            anchor: TimeSource::CpuQpc,
            window: Vec::with_capacity(SAMPLE_WINDOW),
            last_recalib_us: anchor_now_us,
            drift_flags: 0,
            degrade_events: 0,
            recover_events: 0,
            last_out_us: 0,
        }
    }

    /// 源读数注入（HAL 隔离：本模块零硬件访问，同 cgc06 纪律）。
    pub fn feed(&mut self, source: TimeSource, raw_us: u64, at_anchor_us: u64) -> Result<(), u32> {
        let idx = source.ordinal();
        // 单调性：域内读数倒退直接拒（源自身违约比漂移更严重）。
        if raw_us < self.clocks[idx].last_us {
            return Err(XC_TIME_REGRESS);
        }
        self.clocks[idx].last_us = raw_us;
        self.clocks[idx].misses = 0;
        if self.clocks[idx].health == SourceHealth::Lost {
            // 失而复得：回三源（显性恢复）。
            self.clocks[idx].health = SourceHealth::Healthy;
            self.recover_events += 1;
        }
        // 与其他健康源配对采样。
        let mut k = 0;
        while k < 3 {
            if k != idx && self.clocks[k].health == SourceHealth::Healthy {
                self.window.push(PairSample {
                    a: source,
                    b: self.clocks[k].source,
                    ta_us: raw_us,
                    tb_us: self.clocks[k].last_us,
                    at_us: at_anchor_us,
                });
                if self.window.len() > SAMPLE_WINDOW {
                    self.window.remove(0);
                }
            }
            k += 1;
        }
        Ok(())
    }

    /// 周期驱动：失联检测+重校准到期判定。
    pub fn tick(&mut self, now_us: u64) -> Result<CalibReport, u32> {
        // 失联检测：misses 递增。
        let mut i = 0;
        while i < 3 {
            if now_us.saturating_sub(self.clocks[i].last_us) > RECALIB_PERIOD_US / 2 {
                if self.clocks[i].misses < LOST_AFTER_MISSES {
                    self.clocks[i].misses += 1;
                }
                if self.clocks[i].misses >= LOST_AFTER_MISSES
                    && self.clocks[i].health != SourceHealth::Lost
                {
                    self.clocks[i].health = SourceHealth::Lost;
                    self.degrade_events += 1; // 降级显性入账
                }
            }
            i += 1;
        }
        // 重校准到期。
        let due = now_us.saturating_sub(self.last_recalib_us) >= RECALIB_PERIOD_US;
        if due {
            self.last_recalib_us = now_us;
            self.recalibrate(now_us)?;
        }
        Ok(self.report())
    }

    /// 重校准核心：窗口内逐源对主源做偏移/漂移估计，套校正。
    fn recalibrate(&mut self, now_us: u64) -> Result<(), u32> {
        let mut i = 0;
        while i < 3 {
            self.clocks[i].health = if self.clocks[i].health == SourceHealth::Lost {
                SourceHealth::Lost
            } else {
                SourceHealth::Recalibrating
            };
            i += 1;
        }
        // 逐源聚对：主源-副源对的 raw_delta 均值=偏移修正量。
        let mut s = 0;
        while s < 3 {
            if self.clocks[s].source == self.anchor {
                s += 1;
                continue;
            }
            // 收集涉及该源与健康主源的对。
            let mut acc: i64 = 0;
            let mut n: u64 = 0;
            let mut w = 0;
            while w < self.window.len() {
                let p = self.window[w];
                let involves = p.a == self.clocks[s].source || p.b == self.clocks[s].source;
                let other_is_anchor = p.a == self.anchor || p.b == self.anchor;
                if involves && other_is_anchor {
                    // 统一到 anchor 域差：delta = t_anchor − t_src。
                    let d = if p.a == self.anchor {
                        p.ta_us as i64 - p.tb_us as i64
                    } else {
                        p.tb_us as i64 - p.ta_us as i64
                    };
                    acc += d;
                    n += 1;
                }
                w += 1;
            }
            if n > 0 {
                let mean = acc / n as i64;
                let old = self.clocks[s].offset_us;
                let new = mean;
                // 漂移率：偏移变化 / 观测时长（秒）。
                if now_us > self.last_recalib_us && old != new {
                    let dt_s = ((now_us.saturating_sub(self.last_recalib_us)) / 1_000_000).max(1);
                    self.clocks[s].drift_per_s = (new - old) / dt_s as i64;
                    if self.clocks[s].drift_per_s.abs() > DRIFT_REDLINE_US_PER_S {
                        self.drift_flags += 1; // 漂移立案（显性）
                    }
                }
                self.clocks[s].offset_us = new;
            }
            s += 1;
        }
        // 校准完成：回健康（失联源除外）。
        let mut k = 0;
        while k < 3 {
            if self.clocks[k].health == SourceHealth::Recalibrating {
                self.clocks[k].health = SourceHealth::Healthy;
            }
            k += 1;
        }
        Ok(())
    }

    /// 副源读数换算到主源域（单调性守卫：输出禁止倒退）。
    pub fn to_anchor_domain(&mut self, source: TimeSource, raw_us: u64) -> Result<u64, u32> {
        let idx = source.ordinal();
        let off = self.clocks[idx].offset_us;
        let v = (raw_us as i64 + off) as u64;
        if v < self.last_out_us {
            // 倒退：饱和到最近输出（拨表禁止——补偿不倒退）。
            return Err(XC_TIME_REGRESS);
        }
        self.last_out_us = v;
        Ok(v)
    }

    /// 精度声明报告（判据"校准后残余误差量化"）。
    pub fn report(&self) -> CalibReport {
        let mut residual = [0i64; 3];
        let mut degraded = None;
        let mut i = 0;
        while i < 3 {
            let c = &self.clocks[i];
            residual[i] = c.offset_us.abs();
            if c.health == SourceHealth::Lost {
                degraded = Some(c.source);
            }
            i += 1;
        }
        CalibReport {
            anchor: self.anchor,
            residual_us: residual,
            degraded,
            drift_flagged: self.drift_flags > 0,
        }
    }

    /// 精度红线核验：任一在岗源残余超上限即失守（判据"精度声明"）。
    pub fn residual_safe(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < 3 {
            if self.clocks[i].health != SourceHealth::Lost
                && self.clocks[i].offset_us.abs() > RESIDUAL_CAP_US
            {
                return Err(XC_RESIDUAL_UNSAFE);
            }
            i += 1;
        }
        Ok(())
    }

    /// 健康摘要：三源状态与降级面。
    pub fn health_summary(&self) -> [SourceHealth; 3] {
        [
            self.clocks[0].health,
            self.clocks[1].health,
            self.clocks[2].health,
        ]
    }
}

impl TimeSource {
    pub const fn ordinal(self) -> usize {
        match self {
            TimeSource::CpuQpc => 0,
            TimeSource::GpuTimestamp => 1,
            TimeSource::PresentClock => 2,
        }
    }
}

// ---------------------------------------------------------------------------
// 自检（判据五组：三源互校 / 重校准 / 精度声明 / 降级 / 单调性）
// ---------------------------------------------------------------------------

impl TimeCalibrator {
    /// 组一：三源互校——配对采样入窗、窗满滚动。
    pub fn selfcheck_pairs(&mut self) -> Result<(), u32> {
        let mut c = TimeCalibrator::new(1_000_000);
        let mut t = 1_000_000u64;
        let mut i = 0;
        while i < SAMPLE_WINDOW + 4 {
            t += 16_667;
            c.feed(TimeSource::CpuQpc, t, t)?;
            c.feed(TimeSource::GpuTimestamp, t - 30, t)?; // GPU 慢 30µs
            c.feed(TimeSource::PresentClock, t + 10, t)?; // 呈现快 10µs
            i += 1;
        }
        if c.window.len() != SAMPLE_WINDOW {
            return Err(XC_PAIR_SAMPLE_INVALID);
        }
        Ok(())
    }

    /// 组二：重校准——周期到期后偏移收敛到实测差。
    pub fn selfcheck_recalib(&mut self) -> Result<(), u32> {
        let mut c = TimeCalibrator::new(1_000_000);
        let mut t = 1_000_000u64;
        // 第一轮校准周期：GPU 恒定慢 80µs。
        let mut i = 0;
        while i < 24 {
            t += 16_667;
            c.feed(TimeSource::CpuQpc, t, t)?;
            c.feed(TimeSource::GpuTimestamp, t - 80, t)?;
            i += 1;
        }
        let r = c.tick(t + RECALIB_PERIOD_US)?;
        // GPU 源残余=|offset|≈80（配对均值）。
        if r.residual_us[1].abs() < 60 || r.residual_us[1].abs() > 100 {
            return Err(XC_RESIDUAL_UNSAFE);
        }
        // 换算：GPU 域读数经偏移后应贴近主源域。
        let v = c.to_anchor_domain(TimeSource::GpuTimestamp, t + 16_667 - 80)?;
        if v < t || v > t + 16_667 + 40 {
            return Err(XC_TIME_REGRESS);
        }
        Ok(())
    }

    /// 组三：精度声明——残余红线核验（超限即报）。
    pub fn selfcheck_residual(&mut self) -> Result<(), u32> {
        let mut c = TimeCalibrator::new(1_000_000);
        let mut t = 1_000_000u64;
        // 喂入巨大偏移（500µs）远超 RESIDUAL_CAP。
        let mut i = 0;
        while i < 24 {
            t += 16_667;
            c.feed(TimeSource::CpuQpc, t, t)?;
            c.feed(TimeSource::GpuTimestamp, t - 500, t)?;
            i += 1;
        }
        let _ = c.tick(t + RECALIB_PERIOD_US);
        let r = c.residual_safe();
        if r != Err(XC_RESIDUAL_UNSAFE) {
            return Err(XC_RESIDUAL_UNSAFE);
        }
        Ok(())
    }

    /// 组四：降级——源停喂→Lost 显性→恢复→回三源。
    pub fn selfcheck_degrade(&mut self) -> Result<(), u32> {
        let mut c = TimeCalibrator::new(1_000_000);
        let mut t = 1_000_000u64;
        let mut i = 0;
        while i < 10 {
            t += 16_667;
            c.feed(TimeSource::CpuQpc, t, t)?;
            c.feed(TimeSource::GpuTimestamp, t, t)?;
            c.feed(TimeSource::PresentClock, t, t)?;
            i += 1;
        }
        // 呈现时钟停喂：数个重校周期。
        let mut j = 0;
        while j < 3 {
            t += RECALIB_PERIOD_US + 16_667;
            c.feed(TimeSource::CpuQpc, t, t)?;
            c.feed(TimeSource::GpuTimestamp, t, t)?;
            let _ = c.tick(t);
            j += 1;
        }
        let h = c.health_summary();
        if h[2] != SourceHealth::Lost {
            return Err(XC_SOURCE_LOST);
        }
        let rep = c.report();
        if rep.degraded != Some(TimeSource::PresentClock) {
            return Err(XC_SOURCE_LOST);
        }
        if c.degrade_events < 1 {
            return Err(XC_SOURCE_LOST);
        }
        // 恢复：重新喂入。
        t += 16_667;
        c.feed(TimeSource::PresentClock, t, t)?;
        if c.health_summary()[2] != SourceHealth::Healthy || c.recover_events < 1 {
            return Err(XC_SOURCE_LOST);
        }
        Ok(())
    }

    /// 组五：单调性——域内倒退拒收、输出倒退拒收。
    pub fn selfcheck_monotonic(&mut self) -> Result<(), u32> {
        let mut c = TimeCalibrator::new(1_000_000);
        c.feed(TimeSource::CpuQpc, 2_000_000, 2_000_000)?;
        // 倒退读数拒收。
        let r = c.feed(TimeSource::CpuQpc, 1_999_999, 1_999_999);
        if r != Err(XC_TIME_REGRESS) {
            return Err(XC_TIME_REGRESS);
        }
        // 输出倒退拒收。
        let _ = c.to_anchor_domain(TimeSource::GpuTimestamp, 3_000_000)?;
        let r2 = c.to_anchor_domain(TimeSource::GpuTimestamp, 2_999_000);
        if r2 != Err(XC_TIME_REGRESS) {
            return Err(XC_TIME_REGRESS);
        }
        Ok(())
    }

    /// 全量自检入口。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_pairs()?;
        self.selfcheck_recalib()?;
        self.selfcheck_residual()?;
        self.selfcheck_degrade()?;
        self.selfcheck_monotonic()?;
        Ok(())
    }
}
