//! cgd26_telemetry — CGPU-F0506 抖动遥测（五指标进 B08 总线/按模式分列）。
//!
//! 判据原文（施工书锚点 CGPU-F0506）：遥测：抖动谱/均衡器策略命中/抑制率/
//! 残余抖动/缓冲深度分布——五指标进 B08 总线；按呈现模式分列。
//! 判据：五指标、分列、判据。
//!
//! 模型：五指标采样器（谱桶 8 档/策略命中计数/抑制率滑动窗/残余 RMS/缓冲
//! 深度直方 5 档），三模式各持一份互不串账；快照打包成 B08 载荷（定长六
//! 字段），分列对账保证三列齐备且总和一致。

#![allow(dead_code)]

use alloc::vec::Vec;

/// 诊断码段 0x9900（cgpu 域内唯一）。
pub const XC_TEL_BUCKET_OOB: u32 = 0x9901;
pub const XC_TEL_MODE_UNSET: u32 = 0x9902;
pub const XC_TEL_CROSS_LEAK: u32 = 0x9903;
pub const XC_TEL_SNAPSHOT_SHORT: u32 = 0x9904;
pub const XC_TEL_RATE_BAD: u32 = 0x9905;

/// 谱桶数（µs 边界：0/1/2/4/8/16/32/64/∞）。
pub const BUCKETS: usize = 8;

/// 谱桶边界（µs，含下限）：桶 i 接 [b[i], b[i+1])，末桶接 ≥64。
pub const BUCKET_EDGES: [u32; BUCKETS] = [0, 1, 2, 4, 8, 16, 32, 64];

/// 抖动值落桶号。
pub fn bucket_of(us: u32) -> usize {
    let mut i = BUCKETS - 1;
    while i > 0 {
        if us >= BUCKET_EDGES[i] {
            return i;
        }
        i -= 1;
    }
    0
}

/// 缓冲深度直方档：1/2/3/4/≥5。
pub fn depth_slot(d: u8) -> usize {
    match d {
        1 => 0,
        2 => 1,
        3 => 2,
        4 => 3,
        _ => 4,
    }
}

/// 单模式遥测列。
#[derive(Debug)]
pub struct ModeColumn {
    /// 谱桶计数。
    pub spectrum: [u32; BUCKETS],
    /// 策略命中计数（索引=SupPolicy as usize，0..3）。
    pub policy_hits: [u32; 3],
    /// 抑制率滑动窗（千分比，最近 64 样本）。
    pub rate_win: Vec<u32>,
    /// 残余抖动平方和（µs²，RMS 用）。
    resid_sq: u64,
    /// 残余样本数。
    resid_n: u32,
    /// 缓冲深度直方。
    pub depth_hist: [u32; 5],
    /// 采样总数。
    pub samples: u32,
}

impl ModeColumn {
    /// 空列。
    pub fn new() -> Self {
        ModeColumn {
            spectrum: [0; BUCKETS],
            policy_hits: [0; 3],
            rate_win: Vec::new(),
            resid_sq: 0,
            resid_n: 0,
            depth_hist: [0; 5],
            samples: 0,
        }
    }

    /// 谱采样。
    pub fn sample_spectrum(&mut self, jitter_us: u32) {
        self.spectrum[bucket_of(jitter_us)] += 1;
        self.samples += 1;
    }

    /// 策略命中记录（越界档拒）。
    pub fn hit_policy(&mut self, p: usize) -> Result<(), u32> {
        if p >= 3 {
            return Err(XC_TEL_BUCKET_OOB);
        }
        self.policy_hits[p] += 1;
        Ok(())
    }

    /// 抑制率入窗（千分比须 ≤1000，超界拒）。
    pub fn push_rate(&mut self, permille: u32) -> Result<(), u32> {
        if permille > 1000 {
            return Err(XC_TEL_RATE_BAD);
        }
        self.rate_win.push(permille);
        if self.rate_win.len() > 64 {
            self.rate_win.remove(0);
        }
        Ok(())
    }

    /// 窗内平均抑制率（千分比）。
    pub fn avg_rate(&self) -> u32 {
        if self.rate_win.is_empty() {
            return 0;
        }
        let mut s = 0u64;
        let mut i = 0;
        while i < self.rate_win.len() {
            s += self.rate_win[i] as u64;
            i += 1;
        }
        (s / self.rate_win.len() as u64) as u32
    }

    /// 残余采样。
    pub fn sample_resid(&mut self, us: u32) {
        self.resid_sq += (us as u64) * (us as u64);
        self.resid_n += 1;
    }

    /// 残余 RMS（µs）。
    pub fn resid_rms(&self) -> u32 {
        if self.resid_n == 0 {
            return 0;
        }
        let mean_sq = self.resid_sq / self.resid_n as u64;
        // 整数平方根。
        let mut g = mean_sq;
        if g >= 2 {
            let mut n = (g + 1) / 2;
            while n < g {
                g = n;
                n = (g + mean_sq / g) / 2;
            }
        }
        g as u32
    }

    /// 缓冲深度采样。
    pub fn sample_depth(&mut self, d: u8) {
        self.depth_hist[depth_slot(d)] += 1;
    }

    /// 主档深度（命中最多档，并列取小）。
    pub fn dominant_depth_slot(&self) -> usize {
        let mut best = 0;
        let mut i = 1;
        while i < 5 {
            if self.depth_hist[i] > self.depth_hist[best] {
                best = i;
            }
            i += 1;
        }
        best
    }
}

impl Default for ModeColumn {
    fn default() -> Self {
        Self::new()
    }
}

/// B08 快照载荷（定长六字段）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TelSnapshot {
    /// 模式号（0/1/2）。
    pub mode: u8,
    /// 主谱桶。
    pub top_bucket: u8,
    /// 主策略档。
    pub top_policy: u8,
    /// 平均抑制率（千分比）。
    pub avg_rate: u32,
    /// 残余 RMS（µs）。
    pub resid_rms: u32,
    /// 主档缓冲深度（1..5 档号）。
    pub depth_slot: u8,
}

impl TelSnapshot {
    /// 载荷字段数。
    pub const FIELDS: usize = 6;

    /// 打包成 u32 数组（B08 总线载荷）。
    pub fn pack(&self) -> [u32; Self::FIELDS] {
        [
            self.mode as u32,
            self.top_bucket as u32,
            self.top_policy as u32,
            self.avg_rate,
            self.resid_rms,
            self.depth_slot as u32,
        ]
    }
}

/// 遥测总线：三模式分列。
pub struct TelBus {
    cols: [ModeColumn; 3],
    /// 列是否被采过（分列对账用）。
    touched: [bool; 3],
}

impl TelBus {
    /// 空总线，三列独立。
    pub fn new() -> Self {
        TelBus {
            cols: [ModeColumn::new(), ModeColumn::new(), ModeColumn::new()],
            touched: [false; 3],
        }
    }

    /// 按模式取列（模式号 0..2，越界拒）。
    pub fn col(&mut self, mode: usize) -> Result<&mut ModeColumn, u32> {
        if mode >= 3 {
            return Err(XC_TEL_MODE_UNSET);
        }
        self.touched[mode] = true;
        Ok(&mut self.cols[mode])
    }

    /// 快照：从指定列提取。列未采过拒。
    pub fn snapshot(&self, mode: usize) -> Result<TelSnapshot, u32> {
        if mode >= 3 {
            return Err(XC_TEL_MODE_UNSET);
        }
        if !self.touched[mode] {
            return Err(XC_TEL_MODE_UNSET);
        }
        let c = &self.cols[mode];
        // 主谱桶。
        let mut tb = 0;
        let mut i = 1;
        while i < BUCKETS {
            if c.spectrum[i] > c.spectrum[tb] {
                tb = i;
            }
            i += 1;
        }
        // 主策略档。
        let mut tp = 0;
        let mut i = 1;
        while i < 3 {
            if c.policy_hits[i] > c.policy_hits[tp] {
                tp = i;
            }
            i += 1;
        }
        Ok(TelSnapshot {
            mode: mode as u8,
            top_bucket: tb as u8,
            top_policy: tp as u8,
            avg_rate: c.avg_rate(),
            resid_rms: c.resid_rms(),
            depth_slot: c.dominant_depth_slot() as u8,
        })
    }

    /// 分列对账：三列样本总和守恒（写入方在列间不得搬移）。
    pub fn total_samples(&self) -> u32 {
        let mut t = 0;
        let mut i = 0;
        while i < 3 {
            t += self.cols[i].samples;
            i += 1;
        }
        t
    }

    /// 串账检查：未触碰列必须全零。
    pub fn untouched_clean(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if !self.touched[i] {
                let c = &self.cols[i];
                if c.samples != 0 || c.resid_n != 0 {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

impl Default for TelBus {
    fn default() -> Self {
        Self::new()
    }
}

/// 遥测总门面。
#[derive(Debug, Default)]
pub struct TelEngine;

impl TelEngine {
    /// 自检一：谱桶落位——边界值进正确桶。
    pub fn selfcheck_buckets(&self) -> Result<(), u32> {
        let expect: [usize; 9] = [0, 1, 2, 3, 4, 5, 6, 7, 7];
        let vals: [u32; 9] = [0, 1, 2, 4, 8, 16, 32, 64, 1000];
        let mut i = 0;
        while i < 9 {
            if bucket_of(vals[i]) != expect[i] {
                return Err(XC_TEL_BUCKET_OOB);
            }
            i += 1;
        }
        Ok(())
    }

    /// 自检二：深度档落位。
    pub fn selfcheck_depth(&self) -> Result<(), u32> {
        if depth_slot(1) != 0 || depth_slot(2) != 1 || depth_slot(3) != 2 || depth_slot(4) != 3 {
            return Err(XC_TEL_BUCKET_OOB);
        }
        let mut d = 5;
        while d < 200 {
            if depth_slot(d) != 4 {
                return Err(XC_TEL_BUCKET_OOB);
            }
            d += 10;
        }
        Ok(())
    }

    /// 自检三：五指标单列齐备——谱/命中/率/残余/深度逐一采样并读出。
    pub fn selfcheck_five(&self) -> Result<(), u32> {
        let mut bus = TelBus::new();
        {
            let c = bus.col(0)?;
            let mut i = 0;
            while i < 10 {
                c.sample_spectrum(if i % 3 == 0 { 20 } else { 1 });
                c.sample_resid(if i % 3 == 0 { 6 } else { 1 });
                c.sample_depth(2);
                i += 1;
            }
            c.hit_policy(1)?;
            c.push_rate(700)?;
            c.push_rate(900)?;
        }
        let s = bus.snapshot(0)?;
        if s.top_bucket != 6 {
            // 20µs×4 次落桶 6，1µs×6 次落桶 1 → 主桶 6。
            return Err(XC_TEL_BUCKET_OOB);
        }
        if s.top_policy != 1 {
            return Err(XC_TEL_BUCKET_OOB);
        }
        if s.avg_rate != 800 {
            return Err(XC_TEL_RATE_BAD);
        }
        if s.resid_rms == 0 {
            return Err(XC_TEL_SNAPSHOT_SHORT);
        }
        if s.depth_slot != 0 {
            return Err(XC_TEL_BUCKET_OOB);
        }
        // 率越界拒。
        if bus.col(0)?.push_rate(1001).is_ok() {
            return Err(XC_TEL_RATE_BAD);
        }
        // 策略档越界拒。
        if bus.col(0)?.hit_policy(3).is_ok() {
            return Err(XC_TEL_BUCKET_OOB);
        }
        Ok(())
    }

    /// 自检四：分列独立——三列互不串账、未触碰列干净、未触碰快照拒。
    pub fn selfcheck_split(&self) -> Result<(), u32> {
        let mut bus = TelBus::new();
        {
            let c = bus.col(1)?;
            let mut i = 0;
            while i < 5 {
                c.sample_spectrum(4);
                i += 1;
            }
        }
        if bus.total_samples() != 5 {
            return Err(XC_TEL_CROSS_LEAK);
        }
        if !bus.untouched_clean() {
            return Err(XC_TEL_CROSS_LEAK);
        }
        if bus.snapshot(0).is_ok() || bus.snapshot(2).is_ok() {
            return Err(XC_TEL_MODE_UNSET);
        }
        if bus.snapshot(1).is_err() {
            return Err(XC_TEL_MODE_UNSET);
        }
        // 模式号越界拒。
        if bus.snapshot(3).is_ok() {
            return Err(XC_TEL_MODE_UNSET);
        }
        Ok(())
    }

    /// 自检五：B08 载荷打包与判据收口。
    pub fn selfcheck_bus(&self) -> Result<(), u32> {
        let mut bus = TelBus::new();
        {
            let c = bus.col(2)?;
            c.sample_spectrum(100);
            c.hit_policy(2)?;
            c.push_rate(500)?;
            c.sample_resid(16);
            c.sample_depth(5);
        }
        let s = bus.snapshot(2)?;
        let packed = s.pack();
        if packed.len() != TelSnapshot::FIELDS {
            return Err(XC_TEL_SNAPSHOT_SHORT);
        }
        if packed[0] != 2 || packed[1] != 7 || packed[2] != 2 || packed[5] != 4 {
            return Err(XC_TEL_SNAPSHOT_SHORT);
        }
        // 载荷往返一致。
        let s2 = TelSnapshot {
            mode: s.mode,
            top_bucket: s.top_bucket,
            top_policy: s.top_policy,
            avg_rate: s.avg_rate,
            resid_rms: s.resid_rms,
            depth_slot: s.depth_slot,
        };
        if s2.pack() != packed {
            return Err(XC_TEL_SNAPSHOT_SHORT);
        }
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x9900..0x9A00 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_TEL_BUCKET_OOB,
            XC_TEL_MODE_UNSET,
            XC_TEL_CROSS_LEAK,
            XC_TEL_SNAPSHOT_SHORT,
            XC_TEL_RATE_BAD,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x9900 || codes[i] >= 0x9A00 {
                return Err(codes[i]);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(codes[i]);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cgd26_selfchecks() {
        let e = TelEngine;
        e.selfcheck_buckets().unwrap();
        e.selfcheck_depth().unwrap();
        e.selfcheck_five().unwrap();
        e.selfcheck_split().unwrap();
        e.selfcheck_bus().unwrap();
        assert_eq!(TelEngine::segment_guard(), Ok(()));
    }
}
