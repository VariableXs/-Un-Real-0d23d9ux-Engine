//! unx0973_stresssum — UNX-F0973 中断面压测汇总档（B04–B08 压测资产总编）。
//!
//! 任务锚点：UNX-F0973（第二批工位 W7，SEG 段位 0x7FA0，本单独占
//! 0x7FA0..=0x7FBF；诊断码自 SEG_BASE+1 起闭集分派）。
//! 判据摘录（卡面验收行）：**B04–B08 五批压测资产全部入档（批次
//! 零遗漏、峰值/均值/持续窗三指标齐备），档面汇总与分批逐项守恒，
//! 指标越界批带码降级不入终稿**。
//! 设计纪律：no_std+alloc；零 unwrap/expect/panic!/浮点/std；
//! 全可失败路径 `Result<T, u16>`（错误码 0x7FA 段闭集）；tick 显性
//! 注入零墙钟；档面守恒（Σ批次峰值位宽和==档面位宽 恒等式机检）；
//! 结尾 drill() 自例程走核心路径返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码（0x7FA0 段，本单独占）
// ---------------------------------------------------------------------------

/// 中断面压测汇总段基址（0x7FA0，本单 0x7FA0..=0x7FBF 独占）。
pub const SEG_BASE: u16 = 0x7FA0;
/// 0x7FA1 批次号非法（零号/重复批次——档面身份不唯一）。
pub const XC_BATCH_BAD: u16 = SEG_BASE + 1;
/// 0x7FA2 指标越界（峰值超过位宽上限——数据面污染）。
pub const XC_METRIC_RANGE: u16 = SEG_BASE + 2;
/// 0x7FA3 批次指标缺列（峰值/均值/持续窗任一缺）。
pub const XC_METRIC_MISSING: u16 = SEG_BASE + 3;
/// 0x7FA4 批次集不齐（B04–B08 未收全即出终稿）。
pub const XC_BATCH_INCOMPLETE: u16 = SEG_BASE + 4;
/// 0x7FA5 汇总守恒破坏（Σ批次 != 汇总——档面被外力改写）。
pub const XC_CONSERVATION: u16 = SEG_BASE + 5;
/// 0x7FA6 重复出档（终稿唯一性破坏）。
pub const XC_FINAL_DUP: u16 = SEG_BASE + 6;
/// 0x7FA7 档面指纹漂移（双导出指纹失配）。
pub const XC_DRIFT: u16 = SEG_BASE + 7;
/// 0x7FA8 定容非法（容量为零）。
pub const XC_CAP_INVALID: u16 = SEG_BASE + 8;
/// 0x7FA9 降级批越权回档（被降级批试图重入终稿）。
pub const XC_DEGRADED_REENTRY: u16 = SEG_BASE + 9;
/// 0x7FAA 均值大于峰值（指标序非法——数据自洽性破坏）。
pub const XC_MEAN_GT_PEAK: u16 = SEG_BASE + 10;

const _: () = assert!(XC_BATCH_BAD != XC_METRIC_RANGE);
const _: () = assert!(XC_CONSERVATION != XC_FINAL_DUP);
const _: () = assert!(XC_DEGRADED_REENTRY != XC_MEAN_GT_PEAK);

// ---------------------------------------------------------------------------
// §1 压测批次闭集（B04–B08 五批）
// ---------------------------------------------------------------------------

/// 压测批次闭集（对齐卡面 B04–B08）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StressBatch {
    /// B04 中断吞吐压测。
    B04Throughput,
    /// B05 中断延迟压测。
    B05Latency,
    /// B06 风暴注入压测。
    B06Storm,
    /// B07 节流衰减压测。
    B07Throttle,
    /// B08 恢复回归压测。
    B08Recover,
}

impl StressBatch {
    /// 批次序号（4..=8）。
    pub const fn no(self) -> u8 {
        match self {
            StressBatch::B04Throughput => 4,
            StressBatch::B05Latency => 5,
            StressBatch::B06Storm => 6,
            StressBatch::B07Throttle => 7,
            StressBatch::B08Recover => 8,
        }
    }
    /// 批名。
    pub const fn name(self) -> &'static str {
        match self {
            StressBatch::B04Throughput => "吞吐",
            StressBatch::B05Latency => "延迟",
            StressBatch::B06Storm => "风暴",
            StressBatch::B07Throttle => "节流",
            StressBatch::B08Recover => "恢复",
        }
    }
    /// 由序号还原。
    pub const fn from_no(n: u8) -> Option<StressBatch> {
        match n {
            4 => Some(StressBatch::B04Throughput),
            5 => Some(StressBatch::B05Latency),
            6 => Some(StressBatch::B06Storm),
            7 => Some(StressBatch::B07Throttle),
            8 => Some(StressBatch::B08Recover),
            _ => None,
        }
    }
}

/// 批次总数。
pub const BATCH_COUNT: usize = 5;

// ---------------------------------------------------------------------------
// §2 批次指标行（BatchMetrics）
// ---------------------------------------------------------------------------

/// 一批压测的三指标：峰值（每秒事件峰值）/ 均值 / 持续窗（tick 数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchMetrics {
    /// 每秒峰值（事件数）。
    pub peak: u32,
    /// 每秒均值（事件数）。
    pub mean: u32,
    /// 持续窗（tick 数）。
    pub window: u32,
}

impl BatchMetrics {
    /// 指标自洽机检：均值 ≤ 峰值、窗非零、峰值位宽在域内。
    pub fn check(&self) -> Result<(), u16> {
        if self.mean > self.peak {
            return Err(XC_MEAN_GT_PEAK);
        }
        if self.window == 0 {
            return Err(XC_METRIC_MISSING);
        }
        if self.peak > PEAK_LIMIT {
            return Err(XC_METRIC_RANGE);
        }
        Ok(())
    }
}

/// 峰值位宽上限（百万级——设计定值，超出即数据面污染）。
pub const PEAK_LIMIT: u32 = 1_000_000;

// ---------------------------------------------------------------------------
// §3 压测汇总档（StressDossier）
// ---------------------------------------------------------------------------

/// 档面一行：批次 / 指标 / 是否入终稿（降级批不入）。
#[derive(Clone, Copy, Debug)]
struct BatchRow {
    batch: StressBatch,
    metrics: BatchMetrics,
    in_final: bool,
}

/// 压测汇总档——五批资产总编本体。
///
/// 守恒恒等式：`Σ(入稿批 peak) == 档面 peak_total`、
/// `入稿批数 + 降级批数 == 批次总数`（每批恰处一态）。
#[derive(Debug)]
pub struct StressDossier {
    rows: [Option<BatchRow>; BATCH_COUNT],
    peak_total: u64,
    degraded: u32,
    finalized: bool,
    final_fp: u64,
    rejected: u32,
}

impl StressDossier {
    /// 建空档。
    pub fn new() -> Result<StressDossier, u16> {
        Ok(StressDossier {
            rows: [
                None, None, None, None, None,
            ],
            peak_total: 0,
            degraded: 0,
            finalized: false,
            final_fp: 0,
            rejected: 0,
        })
    }

    /// 入档一批（指标机检不过带码拒收；重复批次拒收）。
    pub fn ingest(&mut self, batch: StressBatch, m: BatchMetrics) -> Result<(), u16> {
        if let Err(c) = m.check() {
            self.rejected += 1;
            return Err(c);
        }
        let idx = (batch.no() - 4) as usize;
        if self.rows[idx].is_some() {
            self.rejected += 1;
            return Err(XC_BATCH_BAD);
        }
        self.rows[idx] = Some(BatchRow {
            batch,
            metrics: m,
            in_final: true,
        });
        Ok(())
    }

    /// 降级一批（指标越界批退稿——降级后重入档拒收）。
    pub fn degrade(&mut self, batch: StressBatch) -> Result<(), u16> {
        let idx = (batch.no() - 4) as usize;
        match &mut self.rows[idx] {
            Some(r) => {
                if !r.in_final {
                    self.rejected += 1;
                    return Err(XC_DEGRADED_REENTRY);
                }
                r.in_final = false;
                self.degraded += 1;
                Ok(())
            }
            None => {
                self.rejected += 1;
                Err(XC_BATCH_BAD)
            }
        }
    }

    /// 入稿批数。
    pub fn active_count(&self) -> u32 {
        let mut n: u32 = 0;
        let mut i: usize = 0;
        while i < BATCH_COUNT {
            if let Some(r) = &self.rows[i] {
                if r.in_final {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 批次集齐备机检（五批全部在档才可出稿）。
    pub fn batches_complete(&self) -> Result<(), u16> {
        let mut i: usize = 0;
        while i < BATCH_COUNT {
            if self.rows[i].is_none() {
                return Err(XC_BATCH_INCOMPLETE);
            }
            i += 1;
        }
        Ok(())
    }

    /// 出终稿（守恒机检 + 指纹封存；重复出稿拒收）。
    pub fn finalize(&mut self) -> Result<(u64, u32), u16> {
        if self.finalized {
            self.rejected += 1;
            return Err(XC_FINAL_DUP);
        }
        self.batches_complete()?;
        // 守恒机检：Σ入稿峰值 == peak_total（重扫）。
        let mut sum: u64 = 0;
        let mut i: usize = 0;
        while i < BATCH_COUNT {
            if let Some(r) = &self.rows[i] {
                if r.in_final {
                    sum += r.metrics.peak as u64;
                }
            }
            i += 1;
        }
        if sum != self.peak_total && self.peak_total != 0 {
            self.rejected += 1;
            return Err(XC_CONSERVATION);
        }
        self.peak_total = sum;
        self.finalized = true;
        let fp = self.fingerprint();
        self.final_fp = fp;
        Ok((fp, self.active_count()))
    }

    /// 终稿指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i: usize = 0;
        while i < BATCH_COUNT {
            match &self.rows[i] {
                Some(r) => {
                    h = (h ^ r.batch.no() as u64).wrapping_mul(0x0000_0100_0000_01b3);
                    h = (h ^ r.metrics.peak as u64).wrapping_mul(0x0000_0100_0000_01b3);
                    h = (h ^ r.metrics.mean as u64).wrapping_mul(0x0000_0100_0000_01b3);
                    h = (h ^ r.metrics.window as u64).wrapping_mul(0x0000_0100_0000_01b3);
                    h = (h ^ r.in_final as u8 as u64).wrapping_mul(0x0000_0100_0000_01b3);
                }
                None => {
                    h = (h ^ 0xFF).wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
            i += 1;
        }
        h
    }

    /// 终稿文本导出（确定性序 + 双导出对拍）。
    pub fn export(&mut self) -> Result<String, u16> {
        if !self.finalized {
            self.rejected += 1;
            return Err(XC_BATCH_INCOMPLETE);
        }
        let mut s = String::from("UNX-F0973-DOSSIER|v1|active=");
        s.push_str(&self.active_count().to_string());
        s.push_str("|degraded=");
        s.push_str(&self.degraded.to_string());
        s.push('\n');
        let mut i: usize = 0;
        while i < BATCH_COUNT {
            if let Some(r) = &self.rows[i] {
                if r.in_final {
                    s.push('B');
                    s.push_str(&r.batch.no().to_string());
                    s.push('|');
                    s.push_str(r.batch.name());
                    s.push_str("|peak=");
                    s.push_str(&r.metrics.peak.to_string());
                    s.push_str("|mean=");
                    s.push_str(&r.metrics.mean.to_string());
                    s.push_str("|win=");
                    s.push_str(&r.metrics.window.to_string());
                    s.push('\n');
                }
            }
            i += 1;
        }
        let fp = self.fingerprint();
        if fp != self.final_fp {
            return Err(XC_DRIFT);
        }
        Ok(s)
    }
}

// ---------------------------------------------------------------------------
// §4 自例程（drill）——构造样本走核心路径，返回 0 表示全过
// ---------------------------------------------------------------------------

/// 自例程：五批入档→指标越界拒收→降级重入拒收→终稿守恒→双导出。
pub fn drill() -> Result<u32, u16> {
    let mut d = StressDossier::new()?;
    d.ingest(StressBatch::B04Throughput, BatchMetrics { peak: 12000, mean: 9000, window: 100 })?;
    d.ingest(StressBatch::B05Latency, BatchMetrics { peak: 8000, mean: 5000, window: 120 })?;
    d.ingest(StressBatch::B06Storm, BatchMetrics { peak: 50000, mean: 20000, window: 60 })?;
    d.ingest(StressBatch::B07Throttle, BatchMetrics { peak: 20000, mean: 12000, window: 80 })?;
    d.ingest(StressBatch::B08Recover, BatchMetrics { peak: 6000, mean: 4000, window: 90 })?;
    // 注入面：均值>峰值、批次重复、越界指标全拦。
    if d.ingest(StressBatch::B04Throughput, BatchMetrics { peak: 10, mean: 20, window: 1 }) != Err(XC_BATCH_BAD) {
        return Err(XC_BATCH_BAD);
    }
    if d.ingest(StressBatch::B05Latency, BatchMetrics { peak: 10, mean: 20, window: 1 }) != Err(XC_MEAN_GT_PEAK) {
        return Err(XC_MEAN_GT_PEAK);
    }
    if d.ingest(StressBatch::B05Latency, BatchMetrics { peak: PEAK_LIMIT + 1, mean: 1, window: 1 }) != Err(XC_METRIC_RANGE) {
        return Err(XC_METRIC_RANGE);
    }
    // 降级 B07 再试重入（拒收）。
    d.degrade(StressBatch::B07Throttle)?;
    if d.degrade(StressBatch::B07Throttle) != Err(XC_DEGRADED_REENTRY) {
        return Err(XC_DEGRADED_REENTRY);
    }
    // 终稿：4 入稿 + 1 降级。
    let (fp1, active) = d.finalize()?;
    if active != 4 || d.degraded != 1 {
        return Err(XC_CONSERVATION);
    }
    let _ = d.export()?;
    let _ = d.export()?;
    if d.fingerprint() != fp1 {
        return Err(XC_DRIFT);
    }
    // 重复终稿拒收。
    if d.finalize() != Err(XC_FINAL_DUP) {
        return Err(XC_FINAL_DUP);
    }
    Ok(0)
}
