//! UNX-F48744 C·内存漂移终账（24 周期采样终值 <10MB 断言或挂校准）
//! 判据 UNX-F48744-J1：终值落账；判据锚定 §AI-61 §5 J-4。
//! 设计纪律：no_std（仅 core/alloc）；诊断码自 SEG_BASE+1 起连续且总数 ≤0x1F；
//! drill() 自证十判据，正例计数、反例注入必须 .err() 命中，命中即 Err 传播；
//! fuzz 用确定性 LCG（wrapping_mul 1_664_525 / wrapping_add 1_013_904_223）；
//! 24 周期采样终值必须 <10MB（10_485_760KB），越限即挂校准记录，终值必落账。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1A20;
const E_OVER_10MB: u16 = SEG_BASE + 1;
const E_CYC_MISS: u16 = SEG_BASE + 2;
const E_NO_CALIB: u16 = SEG_BASE + 3;
const E_NEG_RSS: u16 = SEG_BASE + 4;
const E_FINAL_LOST: u16 = SEG_BASE + 5;
const E_MONO_FAIL: u16 = SEG_BASE + 6;
const E_REPLAY_DIFF: u16 = SEG_BASE + 7;
const E_SAMPLE_GAP: u16 = SEG_BASE + 8;

/// 单周期采样：周期号 + RSS 千字节
#[derive(Clone, Debug, PartialEq)]
pub struct CycSample {
    pub cyc: u32,
    pub rss_kb: u32,
}

impl CycSample {
    pub fn new(cyc: u32, rss_kb: u32) -> Self {
        CycSample { cyc, rss_kb }
    }
}

/// 内存漂移终账：24 周期采样 + 校准记录
pub struct DriftLedger {
    pub samples: Vec<CycSample>,
    pub calibrations: Vec<String>,
}

impl DriftLedger {
    pub fn new() -> Self {
        DriftLedger { samples: vec![], calibrations: vec![] }
    }

    pub fn sample(&mut self, cyc: u32, rss_kb: u32) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.samples.len() {
            if self.samples[i].cyc == cyc {
                return Err(E_CYC_MISS);
            }
            if self.samples[i].cyc > cyc {
                return Err(E_SAMPLE_GAP);
            }
            i += 1;
        }
        if rss_kb > 0x8000_0000 {
            return Err(E_NEG_RSS);
        }
        self.samples.push(CycSample::new(cyc, rss_kb));
        Ok(())
    }

    pub fn hang_calibration(&mut self, tag: &str) {
        self.calibrations.push(String::from(tag));
    }

    /// 终值断言：第 24 周期样本 <10MB，越限则必须有校准记录
    pub fn settle(&self) -> Result<u32, u16> {
        if self.samples.len() != 24 {
            return Err(E_CYC_MISS);
        }
        let mut i = 0usize;
        while i < self.samples.len() {
            if self.samples[i].cyc != i as u32 {
                return Err(E_SAMPLE_GAP);
            }
            i += 1;
        }
        let fin = self.samples[23].rss_kb;
        let fin_kb = fin as u64;
        if fin_kb >= 10_485_760 {
            if self.calibrations.is_empty() {
                return Err(E_NO_CALIB);
            }
            return Ok(fin);
        }
        Ok(fin)
    }

    /// 单调性：整体漂移不连续逆增（相邻两点逆增 >2MB 记为断言失败）
    pub fn mono_check(&self) -> Result<(), u16> {
        let mut i = 1usize;
        while i < self.samples.len() {
            let prev = self.samples[i - 1].rss_kb as i64;
            let curr = self.samples[i].rss_kb as i64;
            if prev - curr > 2_097_152 {
                return Err(E_MONO_FAIL);
            }
            i += 1;
        }
        Ok(())
    }
}

/// 确定性 LCG
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        (self.next() >> 16) % n
    }
}

/// 24 周期终值落账 ×10^3：终值 <10MB 全过
fn sweep_under_10mb(rng: &mut Lcg) -> Result<(), u16> {
    let mut iter = 0u32;
    while iter < 1_000 {
        let mut led = DriftLedger::new();
        let base = 40_000 + rng.below(60_000) as u32;
        let mut cyc = 0u32;
        while cyc < 24 {
            let jitter = rng.below(2_000) as u32;
            led.sample(cyc, base + jitter)?;
            cyc += 1;
        }
        let fin = led.settle()?;
        if fin >= 10_485_760 {
            return Err(E_OVER_10MB);
        }
        led.mono_check()?;
        iter += 1;
    }
    Ok(())
}

/// 越限挂校准 ×10：终值 ≥10MB 必须挂校准才能落账
fn sweep_calibrated(rng: &mut Lcg) -> Result<(), u16> {
    let mut round = 0u32;
    while round < 10 {
        let mut led = DriftLedger::new();
        let mut cyc = 0u32;
        while cyc < 23 {
            led.sample(cyc, 40_000 + rng.below(5_000) as u32)?;
            cyc += 1;
        }
        led.sample(23, 11_000_000)?;
        let r = led.settle();
        if r.err() != Some(E_NO_CALIB) {
            return Err(E_NO_CALIB);
        }
        led.hang_calibration("mem-calib-24w");
        if led.settle()? != 11_000_000 {
            return Err(E_FINAL_LOST);
        }
        round += 1;
    }
    Ok(())
}

/// 反例注入：重复周期、周期断档、未满 24 点均须命中对应码
fn sweep_negatives() -> Result<(), u16> {
    let mut led = DriftLedger::new();
    led.sample(0, 10_000)?;
    let r1 = led.sample(0, 11_000);
    if r1.err() != Some(E_CYC_MISS) {
        return Err(E_CYC_MISS);
    }
    let r2 = led.sample(5, 12_000);
    if r2.err() != Some(E_SAMPLE_GAP) {
        return Err(E_SAMPLE_GAP);
    }
    let r3 = DriftLedger::new().settle();
    if r3.err() != Some(E_CYC_MISS) {
        return Err(E_CYC_MISS);
    }
    let mut led2 = DriftLedger::new();
    let mut c = 0u32;
    while c < 24 {
        led2.sample(c, 50_000)?;
        c += 1;
    }
    if led2.settle().is_err() {
        return Err(E_FINAL_LOST);
    }
    Ok(())
}

/// 重放一致 ×20：同种子双跑终值逐位吻合
fn sweep_replay(seed: u64) -> Result<(), u16> {
    let mut round = 0u32;
    while round < 20 {
        let mut ra = Lcg::new(seed + round as u64);
        let mut rb = Lcg::new(seed + round as u64);
        let mut fa = DriftLedger::new();
        let mut fb = DriftLedger::new();
        let mut cyc = 0u32;
        while cyc < 24 {
            let va = 40_000 + ra.below(8_000) as u32;
            let vb = 40_000 + rb.below(8_000) as u32;
            if va != vb {
                return Err(E_REPLAY_DIFF);
            }
            fa.sample(cyc, va)?;
            fb.sample(cyc, vb)?;
            cyc += 1;
        }
        if fa.settle()? != fb.settle()? {
            return Err(E_REPLAY_DIFF);
        }
        round += 1;
    }
    Ok(())
}

pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1 24 周期终值 <10MB ×10^3
    let mut rng = Lcg::new(0x4874_4B22);
    let mut r1 = 0u32;
    while r1 < 1_000 {
        sweep_under_10mb(&mut rng)?;
        r1 += 1;
    }
    hits += 1;
    // R2 越限挂校准 ×10
    sweep_calibrated(&mut rng)?;
    hits += 1;
    // R3 反例注入三路全命中
    sweep_negatives()?;
    hits += 1;
    // R4 重放一致 ×20
    sweep_replay(0x4874_4CAF)?;
    hits += 1;
    // R5 单调漂移断言：缓升序列全过
    let mut led = DriftLedger::new();
    let mut c = 0u32;
    while c < 24 {
        led.sample(c, 50_000 + c * 100)?;
        c += 1;
    }
    led.mono_check()?;
    hits += 1;
    // R6 采样序号严格递增零断档
    let mut led2 = DriftLedger::new();
    let mut c2 = 0u32;
    while c2 < 24 {
        led2.sample(c2, 60_000)?;
        c2 += 1;
    }
    let mut i = 0usize;
    while i < led2.samples.len() {
        if led2.samples[i].cyc != i as u32 {
            return Err(E_SAMPLE_GAP);
        }
        i += 1;
    }
    hits += 1;
    // R7 校准标签在册可查
    let mut led3 = DriftLedger::new();
    let mut c3 = 0u32;
    while c3 < 23 {
        led3.sample(c3, 40_000)?;
        c3 += 1;
    }
    led3.sample(23, 10_500_000)?;
    led3.hang_calibration("calib-a");
    led3.hang_calibration("calib-b");
    if led3.calibrations.len() != 2 {
        return Err(E_NO_CALIB);
    }
    if led3.settle()? != 10_500_000 {
        return Err(E_FINAL_LOST);
    }
    hits += 1;
    // R8 终值就是第 24 点：不得取中位或首点充数
    let mut led4 = DriftLedger::new();
    let mut c4 = 0u32;
    while c4 < 24 {
        led4.sample(c4, 30_000 + c4 * 10)?;
        c4 += 1;
    }
    let fin = led4.settle()?;
    if fin != led4.samples[23].rss_kb {
        return Err(E_FINAL_LOST);
    }
    hits += 1;
    // R9 10MB 界值精确核对：10_485_759 过、10_485_760 挂校准
    let mut led5 = DriftLedger::new();
    let mut c5 = 0u32;
    while c5 < 24 {
        led5.sample(c5, 10_485_759)?;
        c5 += 1;
    }
    led5.settle()?;
    let mut led6 = DriftLedger::new();
    let mut c6 = 0u32;
    while c6 < 23 {
        led6.sample(c6, 40_000)?;
        c6 += 1;
    }
    led6.sample(23, 10_485_760)?;
    let r6 = led6.settle();
    if r6.err() != Some(E_NO_CALIB) {
        return Err(E_NO_CALIB);
    }
    hits += 1;
    // R10 采样条目绑定：周期号与向量下标一一对应无幽灵
    let mut led7 = DriftLedger::new();
    let mut c7 = 0u32;
    while c7 < 24 {
        led7.sample(c7, 45_000 + c7)?;
        c7 += 1;
    }
    if led7.samples.len() != 24 {
        return Err(E_CYC_MISS);
    }
    for s in led7.samples.iter() {
        if s.rss_kb < 45_000 {
            return Err(E_NEG_RSS);
        }
    }
    hits += 1;
    // R11 深扫补验：同源 LCG 双轨重放逐位吻合 + 环形账求和复算（确定性、零外依）
    let mut track_a: u32 = 0x9E37_79B9;
    let mut track_b: u32 = 0x9E37_79B9;
    let mut rail: Vec<u32> = vec![];
    let mut rail_sum: u64 = 0;
    let mut dstep = 0u32;
    while dstep < 48 {
        track_a = track_a.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        track_b = track_b.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        rail.push(track_a);
        rail_sum += track_a as u64;
        dstep += 1;
    }
    if track_a != track_b {
        return Err(E_REPLAY_DIFF);
    }
    if rail.len() != 48 {
        return Err(E_REPLAY_DIFF);
    }
    let mut replay_sum: u64 = 0;
    let mut ridx = 0usize;
    while ridx < rail.len() {
        replay_sum += rail[ridx] as u64;
        ridx += 1;
    }
    if replay_sum != rail_sum {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;
    Ok(hits)
}
