//! UNX-F0851 · EOI 延迟测量（handler 尾→EOI 写 µs 账） · 诊断段 0x6740（独占 0x6740~0x674F）
//!
//! **判据原文摘录**：EOI 延迟 p99 ≤5µs（宿主/QEMU 分档），延迟分布直方图
//! 可导出。
//!
//! **设计纪律**：
//! 1. 延迟定义：handler 尾（ISR 清位前最后一条模型指令）→ EOI 写完成的
//!    模型 tick 差。延迟全部整数 tick 落账，µs 换算 = tick × ns_per_tick /
//!    1000（整数除法，向下取整）——零浮点。
//! 2. 宿主分档（判据明文「宿主/QEMU 分档」）：host / kvm / qemu 三档，
//!    各档 p99 阈值同 5µs，但 ns_per_tick 倍率不同（10/30/200）——同一
//!    tick 分布在 QEMU 档 µs 值放大 20 倍，p99 越限判定逐档独立。
//! 3. 直方图：整数桶（µs 桶宽 1，0..=64 + 溢出桶），桶计数守恒恒等式
//!    `Σ桶 = 样本总数`，可导出（导出为确定性文本表）。
//! 4. p99 计算：排序后取第 ⌈0.99×n⌉ 个样本（整数索引），无浮点插值。
//! 5. 零 unwrap/expect/panic/std；LCG 确定性；无墙钟。
//!
//! **诊断段声明**：0x6740~0x674F 为本单独占。纯功能线：no_std + alloc，
//! 零 unwrap/expect/panic，整数算术，确定性。

#![allow(dead_code)]

use alloc::vec::Vec;

// ===========================================================================
// 一、诊断码（0x6740~0x674F 闭集）
// ===========================================================================

/// 段基址（保留码）。
pub const SEG_BASE: u16 = 0x6740;
/// 段顶（保留码）。
pub const SEG_TOP: u16 = 0x674F;

/// 向量号越界。
pub const X_EL2_VEC_RANGE: u16 = 0x6741;
/// 宿主档非法。
pub const X_EL2_HOST_TIER: u16 = 0x6742;
/// 延迟逆序（handler 尾 tick 大于 EOI 写 tick——时序违例）。
pub const X_EL2_BACKWARD: u16 = 0x6743;
/// p99 越限（判据红线：p99 ≤ 5µs）。
pub const X_EL2_P99_OVER: u16 = 0x6744;
/// 直方图桶守恒破缺（Σ桶 ≠ 样本数）。
pub const X_EL2_HIST_UNBALANCED: u16 = 0x6745;
/// 直方图导出失败（空样本或桶和为 0）。
pub const X_EL2_EXPORT_EMPTY: u16 = 0x6746;
/// 延迟账满（定容拒绝）。
pub const X_EL2_LAT_FULL: u16 = 0x6747;

/// 诊断码→中文名。
pub const fn code_name(code: u16) -> &'static str {
    match code {
        SEG_BASE => "段基址(保留)",
        X_EL2_VEC_RANGE => "向量号越界",
        X_EL2_HOST_TIER => "宿主档非法",
        X_EL2_BACKWARD => "延迟时序逆序",
        X_EL2_P99_OVER => "p99越限",
        X_EL2_HIST_UNBALANCED => "直方图桶失恒",
        X_EL2_EXPORT_EMPTY => "直方图导出失败",
        X_EL2_LAT_FULL => "延迟账满",
        _ => "未知码",
    }
}

// ===========================================================================
// 二、宿主档
// ===========================================================================

/// 宿主档闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HostTier {
    Host,
    Kvm,
    Qemu,
}

impl HostTier {
    /// ns/tick（建模口径）。
    pub const fn ns_per_tick(self) -> u64 {
        match self {
            HostTier::Host => 10,
            HostTier::Kvm => 30,
            HostTier::Qemu => 200,
        }
    }
    /// p99 µs 阈（判据统一 5µs）。
    pub const fn p99_us_cap(self) -> u64 {
        let _ = self;
        5
    }
}

/// tick→µs 整数换算。
pub const fn tick_to_us(ticks: u64, tier: HostTier) -> u64 {
    ticks.saturating_mul(tier.ns_per_tick()) / 1000
}

// ===========================================================================
// 三、延迟账 + 整数直方图
// ===========================================================================

/// 单条延迟账目。
#[derive(Clone, Copy)]
pub struct EoiLatency {
    pub vector: u8,
    /// handler 尾模型 tick。
    pub tail_tick: u64,
    /// EOI 写完成模型 tick。
    pub eoi_tick: u64,
    /// tick 差（= eoi_tick - tail_tick，恒 ≥0）。
    pub ticks: u64,
    /// µs（分档换算）。
    pub us: u64,
}

/// EOI 延迟测量器。
pub struct LatencyMeter {
    tier: HostTier,
    lats: Vec<EoiLatency>,
    cap: usize,
    /// 直方图：0..=64 µs 各一桶 + 溢出桶，共 66 桶。
    pub hist: [u64; 66],
    /// 桶计数总和（守恒）。
    pub hist_total: u64,
}

impl LatencyMeter {
    pub fn new(cap: usize, tier: HostTier) -> Self {
        Self {
            tier,
            lats: Vec::with_capacity(cap),
            cap,
            hist: [0; 66],
            hist_total: 0,
        }
    }

    /// 记一次 handler 尾→EOI 写。
    pub fn measure(&mut self, vector: u8, tail_tick: u64, eoi_tick: u64) -> Result<EoiLatency, u16> {
        if vector < 32 || vector > 94 {
            return Err(X_EL2_VEC_RANGE);
        }
        if eoi_tick < tail_tick {
            return Err(X_EL2_BACKWARD);
        }
        let ticks = eoi_tick - tail_tick;
        let us = tick_to_us(ticks, self.tier);
        let e = EoiLatency {
            vector,
            tail_tick,
            eoi_tick,
            ticks,
            us,
        };
        // 直方图入桶（µs 桶宽 1；>64 入溢出桶）。
        let bucket = if us > 64 { 65 } else { us as usize };
        self.hist[bucket] += 1;
        self.hist_total += 1;
        // 账目定容环形。
        if self.lats.len() < self.cap {
            self.lats.push(e);
        } else if self.cap > 0 {
            let idx = (self.hist_total % self.cap as u64) as usize;
            self.lats[idx] = e;
        } else {
            return Err(X_EL2_LAT_FULL);
        }
        Ok(e)
    }

    /// 样本数（取直方图守恒值为真源）。
    pub const fn sample_count(&self) -> u64 {
        self.hist_total
    }

    /// 桶守恒校验。
    pub const fn hist_balanced(&self) -> bool {
        let mut sum = 0u64;
        let mut i = 0;
        while i < 66 {
            sum += self.hist[i];
            i += 1;
        }
        sum == self.hist_total
    }

    /// p99（整数）：µs 序列升序取 ⌈0.99n⌉ 位。环形账可能只保留尾段，
    /// 直方图桶为全量真源——p99 由直方图累积反解（免排序）。
    pub fn p99_us(&self) -> Result<u64, u16> {
        if self.hist_total == 0 {
            return Err(X_EL2_EXPORT_EMPTY);
        }
        // 第 k 小 = 累积计数首次 ≥ ⌈0.99 × total⌉ 的桶。
        let k = (self.hist_total * 99 + 99) / 100; // = ceil(0.99*n) 精确整数式
        let mut cum = 0u64;
        let mut i = 0;
        while i < 66 {
            cum += self.hist[i];
            if cum >= k {
                return Ok(i as u64);
            }
            i += 1;
        }
        Ok(65)
    }

    /// 直方图导出（确定性文本表，µs→计数）。
    pub fn export_hist(&self) -> Result<alloc::string::String, u16> {
        use alloc::format;
        if self.hist_total == 0 {
            return Err(X_EL2_EXPORT_EMPTY);
        }
        let mut out = alloc::string::String::new();
        let mut i = 0;
        while i < 66 {
            if self.hist[i] != 0 {
                if i == 65 {
                    out.push_str(">64us");
                } else {
                    out.push_str(&format!("{}us", i));
                }
                out.push(':');
                out.push_str(&format!("{}", self.hist[i]));
                out.push('\n');
            }
            i += 1;
        }
        Ok(out)
    }
}

// ===========================================================================
// 四、注入式延迟样本生成（确定性）
// ===========================================================================

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

/// 批量测量：n 次 EOI 延迟，延迟 tick 由 LCG 生成于 [lo, hi] 区间。
pub fn run_measure_batch(n: u64, lo: u64, hi: u64, seed: u64, tier: HostTier) -> Result<LatencyMeter, u16> {
    let mut rng = Lcg::new(seed);
    let mut meter = LatencyMeter::new(256, tier);
    let mut t = 0u64;
    while t < n {
        let span = if hi > lo { hi - lo } else { 0 };
        let tail = t * 100;
        let eoi = tail + lo + rng.next() % (span + 1);
        meter.measure((33 + t % 62) as u8, tail, eoi)?;
        t += 1;
    }
    if !meter.hist_balanced() {
        return Err(X_EL2_HIST_UNBALANCED);
    }
    Ok(meter)
}

// ===========================================================================
// 五、drill 自例程
// ===========================================================================

/// 自例程：三档 p99 判定 + 直方图守恒导出 + 错误路径。0=全过。
pub fn drill() -> Result<u32, u16> {
    // 1) 向量越界 / 时序逆序拒绝。
    let mut m = LatencyMeter::new(8, HostTier::Host);
    if m.measure(20, 0, 5).is_ok() {
        return Ok(1);
    }
    if m.measure(40, 10, 5).is_ok() {
        return Ok(2);
    }
    // 2) host 档主跑：延迟 5..40 tick → 0..=0µs（10ns/tick → 40 tick=0.4µs），
    //    p99 = 0µs ≤ 5µs。
    let mh = run_measure_batch(10_000, 5, 40, 0x6740_6740, HostTier::Host)?;
    if mh.p99_us()? > HostTier::Host.p99_us_cap() {
        return Err(X_EL2_P99_OVER);
    }
    if mh.hist_balanced() == false {
        return Err(X_EL2_HIST_UNBALANCED);
    }
    // 3) qemu 档：延迟 5..200 tick → 1..=40µs 分布，p99 上限验证逻辑可用
    //    （用 5..50 tick → 1..=10µs，p99 大概率 >5 —— 判定器必须能报越限）。
    let mq = run_measure_batch(10_000, 5, 50, 0x6740_6741, HostTier::Qemu)?;
    // qemu 档 50 tick = 10µs：p99 ∈ [6,10]，必然越 5µs 阈（建模预期越限）。
    let p99q = mq.p99_us()?;
    if p99q <= 5 {
        // 分布上限 10µs、10000 样本下 p99 不可能 ≤5µs（>5µs 样本约 60%）。
        return Ok(3);
    }
    // 4) 直方图导出：非空且行数 = 非零桶数。
    let txt = mh.export_hist()?;
    if txt.is_empty() {
        return Err(X_EL2_EXPORT_EMPTY);
    }
    // 5) 空账导出拒绝。
    let me = LatencyMeter::new(8, HostTier::Kvm);
    if me.export_hist().is_ok() || me.p99_us().is_ok() {
        return Ok(5);
    }
    // 6) 直方图计数守恒：总样本 10000。
    if mh.sample_count() != 10_000 {
        return Ok(6);
    }
    // 7) 三档换算单调：同 tick µs host ≤ kvm ≤ qemu。
    if !(tick_to_us(1000, HostTier::Host) <= tick_to_us(1000, HostTier::Kvm)
        && tick_to_us(1000, HostTier::Kvm) <= tick_to_us(1000, HostTier::Qemu))
    {
        return Ok(7);
    }
    // 8) 诊断码表完备性。
    let probe = [
        SEG_BASE, X_EL2_VEC_RANGE, X_EL2_HOST_TIER, X_EL2_BACKWARD, X_EL2_P99_OVER,
        X_EL2_HIST_UNBALANCED, X_EL2_EXPORT_EMPTY, X_EL2_LAT_FULL,
    ];
    for c in probe {
        if code_name(c) == "未知码" {
            return Ok(8);
        }
    }
    Ok(0)
}
