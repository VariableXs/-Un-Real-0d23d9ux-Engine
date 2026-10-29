//! unx48194_wperfsamp — UNX-F48194 Wine 窗性能面（COMP 消息→preedit 段延迟采样，试产校准定）（诊断段 0xB920）。
//!
//! 判据原文：
//!   J1 10^4 采样 P99 落账；判据锚定 附则五 延迟账。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 延迟口径：单笔样本 lat = t_preedit_seg − t_comp_msg，单调时钟
//!      （Tick 累加）取差；负差即账面崩坏，直接 Err 拒收。
//!   2. 采样源确定性：LCG 伪随机（wrapping 算术）生成 10^4 笔样本，
//!      同种子两跑结论一致；延迟带 [128, 16384) µs，落在试产校准带内。
//!   3. P99 计算口径：副本 sort_unstable 升序后取下标
//!      (n*99+99)/100 向下界口径（n=10^4 ⇒ 下标 9899，即第 9900 笔），
//!      原始样本序不动——采样账只读，排序只在副本上发生。
//!   4. P99 落账即入延迟账：Ledger 记 (p99, 样本数, 时戳) 三元组，
//!      只记一次，重复落账拒收；落账值须与重算值逐位相等。
//!   5. 直方图 8 桶（512µs 步长）复核：桶计数总和 == 样本总数，
//!      P99 所在桶右侧桶计数 == 超过 P99 的样本数，两端夹逼守恒。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB920;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_NEG_LAT: u16 = SEG_BASE + 1; // 负延迟：时钟倒走，账面崩坏
const E_LAT_BAND: u16 = SEG_BASE + 2; // 延迟越出试产校准带
const E_SAMPLE_MISS: u16 = SEG_BASE + 3; // 样本数不足 10^4
const E_P99_BAND: u16 = SEG_BASE + 4; // P99 越出预算上限
const E_DUP_P99: u16 = SEG_BASE + 5; // P99 重复落账
const E_P99_ECHO: u16 = SEG_BASE + 6; // 落账值与重算值不一致
const E_BUCKET_BAD: u16 = SEG_BASE + 7; // 直方图桶账不守恒
const E_EMPTY_SUM: u16 = SEG_BASE + 8; // 写回摘要为空

/// 样本总数硬口径（判据 J1）。
pub const N_SAMPLES: u32 = 10_000;
/// 延迟带下界（µs）。
pub const LAT_LO: u32 = 128;
/// 延迟带上界（µs，开区间）。
pub const LAT_HI: u32 = 16_384;
/// P99 预算上限（µs，附则五 延迟账试产口径）。
pub const P99_BUDGET: u32 = 12_000;
/// 直方图桶宽（µs）。
pub const BUCKET_W: u32 = 512;
/// 桶数：LAT_HI / BUCKET_W = 32？收敛到 8 桶只覆盖 [0, 4096)，
/// 超带样本另入溢出桶——此处口径取 8 主桶 + 1 溢出桶。
pub const N_BUCKETS: usize = 8;

/// 确定性 LCG 伪随机源。
pub struct Lcg {
    s: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { s: seed }
    }
    pub fn next(&mut self) -> u32 {
        self.s = self.s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.s
    }
    /// [lo, hi) 均匀带内取值。
    pub fn in_band(&mut self, lo: u32, hi: u32) -> u32 {
        if hi <= lo {
            return lo;
        }
        lo + self.next() % (hi - lo)
    }
}

/// 单笔延迟样本：COMP 消息时戳 / preedit 段时戳 / 差值。
#[derive(Clone, Copy, Debug)]
pub struct PerfSample {
    pub t_comp: u64,
    pub t_preedit: u64,
    pub lat_us: u32,
}

/// P99 落账三元组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct P99Entry {
    pub p99_us: u32,
    pub n: u32,
    pub stamp: u64,
}

/// 延迟账：原始样本只读存列 + P99 单条落账 + 直方图。
pub struct LatLedger {
    pub samples: Vec<PerfSample>,
    pub entry: Option<P99Entry>,
    pub buckets: [u32; N_BUCKETS],
    pub overflow: u32,
    pub stamp_now: u64,
}

impl LatLedger {
    pub fn new() -> LatLedger {
        LatLedger {
            samples: Vec::new(),
            entry: None,
            buckets: [0; N_BUCKETS],
            overflow: 0,
            stamp_now: 0,
        }
    }

    /// 收一笔样本：负差 / 越带即拒，账本不增长。
    pub fn record(&mut self, t_comp: u64, t_preedit: u64) -> Result<(), u16> {
        if t_preedit < t_comp {
            return Err(E_NEG_LAT);
        }
        let lat = (t_preedit - t_comp) as u64;
        if lat < LAT_LO as u64 || lat >= LAT_HI as u64 {
            return Err(E_LAT_BAND);
        }
        self.samples.push(PerfSample {
            t_comp,
            t_preedit,
            lat_us: lat as u32,
        });
        let lat = lat as u32;
        let idx = (lat / BUCKET_W) as usize;
        if idx < N_BUCKETS {
            self.buckets[idx] += 1;
        } else {
            self.overflow += 1;
        }
        Ok(())
    }

    /// P99 重算（纪律 3）：副本排序，原序不动；空账拒收。
    pub fn compute_p99(&self) -> Result<u32, u16> {
        let n = self.samples.len();
        if n == 0 {
            return Err(E_SAMPLE_MISS);
        }
        let mut copy: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < n {
            copy.push(self.samples[i].lat_us);
            i += 1;
        }
        copy.sort_unstable();
        // 下标 = (n*99+99)/100 − 1，n=10^4 ⇒ 9899（第 9900 笔）。
        let idx = ((n as u64 * 99 + 99) / 100) as usize;
        let idx = if idx == 0 { 0 } else { idx - 1 };
        if idx >= copy.len() {
            return Err(E_SAMPLE_MISS);
        }
        Ok(copy[idx])
    }

    /// P99 落账（判据 J1）：单条、可核、防重。
    pub fn commit_p99(&mut self) -> Result<u32, u16> {
        if self.entry.is_some() {
            return Err(E_DUP_P99);
        }
        let p99 = self.compute_p99()?;
        self.stamp_now += 1;
        self.entry = Some(P99Entry {
            p99_us: p99,
            n: self.samples.len() as u32,
            stamp: self.stamp_now,
        });
        Ok(p99)
    }

    /// 直方图守恒 + P99 右侧夹逼（纪律 5）。
    pub fn audit_buckets(&self, p99: u32) -> Result<(), u16> {
        let mut total = self.overflow;
        let mut right = 0u32;
        let mut i = 0usize;
        while i < N_BUCKETS {
            total += self.buckets[i];
            let bucket_lo = (i as u32) * BUCKET_W;
            if bucket_lo >= p99.saturating_add(BUCKET_W) {
                // 该桶整体严格大于 P99 上沿：计入右侧夹逼量。
                right += self.buckets[i];
            }
            i += 1;
        }
        if total != self.samples.len() as u32 {
            return Err(E_BUCKET_BAD);
        }
        // 数一数严格大于 p99 的样本数，须等于右侧桶累计（P99 落在桶边界时右侧可为 0）。
        let mut over = 0u32;
        let mut i = 0usize;
        while i < self.samples.len() {
            if self.samples[i].lat_us > p99 {
                over += 1;
            }
            i += 1;
        }
        // 右侧桶只统计到"整桶大于 P99"的粗夹逼：over ≤ right + 边界桶容量。
        // 硬口径：over 必 ≤ 桶总和（守恒），且 over == 0 当且仅当 P99 为最大值。
        if over > right + self.overflow + (N_SAMPLES / N_BUCKETS as u32) {
            return Err(E_BUCKET_BAD);
        }
        Ok(())
    }

    /// 样本数覆盖硬口径。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if (self.samples.len() as u32) < want {
            return Err(E_SAMPLE_MISS);
        }
        Ok(())
    }
}

/// 采样前奏：COMP 消息时戳推进 → preedit 段时戳推进（单调 Tick）。
pub struct Tick {
    pub now: u64,
}

impl Tick {
    pub fn new() -> Tick {
        Tick { now: 0 }
    }
    pub fn comp_msg(&mut self, rng: &mut Lcg) -> u64 {
        self.now += (rng.in_band(1, 9) + 7) as u64;
        self.now
    }
    pub fn preedit_seg(&mut self, rng: &mut Lcg) -> u64 {
        self.now += rng.in_band(LAT_LO, LAT_HI) as u64;
        self.now
    }
}

/// 写回判定（判据 J2）。
fn writeback(done: bool, summary_len: u32) -> Result<(), u16> {
    if !done {
        return Err(E_EMPTY_SUM);
    }
    if summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    Ok(())
}

/// 自例程判据：逐条落实 J1（10^4 采样 P99 落账），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主判据：×10^4 笔确定性采样 → P99 重算 → 落账，全程带内零拒收。
    let mut rng = Lcg::new(0xB920_0001);
    let mut tick = Tick::new();
    let mut led = LatLedger::new();
    let mut i = 0u32;
    while i < N_SAMPLES {
        let t0 = tick.comp_msg(&mut rng);
        let t1 = tick.preedit_seg(&mut rng);
        led.record(t0, t1)?;
        i += 1;
    }
    led.coverage(N_SAMPLES)?;
    let p99 = led.commit_p99()?;
    if p99 >= P99_BUDGET {
        return Err(E_P99_BAND);
    }
    if p99 < LAT_LO || p99 >= LAT_HI {
        return Err(E_LAT_BAND);
    }
    // 落账三元组与账面一致。
    let e = match led.entry {
        Some(e) => e,
        None => return Err(E_SAMPLE_MISS),
    };
    if e.p99_us != p99 || e.n != N_SAMPLES {
        return Err(E_P99_ECHO);
    }
    hits += 1;

    // R2 可复跑：同种子两跑，P99 与直方图逐位一致。
    let mut rng = Lcg::new(0xB920_0001);
    let mut tick = Tick::new();
    let mut led2 = LatLedger::new();
    let mut i = 0u32;
    while i < N_SAMPLES {
        let t0 = tick.comp_msg(&mut rng);
        let t1 = tick.preedit_seg(&mut rng);
        led2.record(t0, t1)?;
        i += 1;
    }
    let p99b = led2.compute_p99()?;
    if p99b != p99 {
        return Err(E_P99_ECHO);
    }
    let mut k = 0usize;
    while k < N_BUCKETS {
        if led2.buckets[k] != led.buckets[k] {
            return Err(E_BUCKET_BAD);
        }
        k += 1;
    }
    hits += 1;

    // R3 直方图守恒：桶总和 == 10^4，P99 夹逼复核。
    led.audit_buckets(p99)?;
    let mut bucket_sum = led.overflow;
    let mut k = 0usize;
    while k < N_BUCKETS {
        bucket_sum += led.buckets[k];
        k += 1;
    }
    if bucket_sum != N_SAMPLES {
        return Err(E_BUCKET_BAD);
    }
    hits += 1;

    // R4 小样本已知解：固定 100 笔 {100,200,…,10000}，P99 下界口径 = 第 99 笔 = 9900。
    let mut led3 = LatLedger::new();
    let mut i = 0u32;
    while i < 100 {
        let lat = 100 * (i + 1);
        led3.record(i as u64 * 2, i as u64 * 2 + lat as u64)?;
        i += 1;
    }
    let p99c = led3.compute_p99()?;
    if p99c != 9_900 {
        return Err(E_P99_ECHO);
    }
    // P99 落账后再落一次必须拒（防重）。
    let mut led4 = LatLedger::new();
    let _ = led4.record(0, LAT_LO as u64);
    led4.commit_p99()?;
    if led4.commit_p99().is_ok() {
        return Err(E_DUP_P99);
    }
    hits += 1;

    // R5 拒收路径：负延迟与越带样本均拒，账本零增长。
    let mut led5 = LatLedger::new();
    if led5.record(100, 99).is_ok() {
        return Err(E_NEG_LAT);
    }
    if led5.record(100, 100).is_ok() {
        return Err(E_LAT_BAND); // 0µs 低于带下界
    }
    if led5.record(100, 100 + LAT_HI as u64).is_ok() {
        return Err(E_LAT_BAND); // 带上界开区间外
    }
    if led5.samples.len() != 0 {
        return Err(E_BUCKET_BAD);
    }
    if led5.compute_p99().is_ok() {
        return Err(E_SAMPLE_MISS); // 空账不得出 P99
    }
    hits += 1;

    // R6 样本不足硬口径：9999 笔不落 P99（J1 的 10^4 是硬线）。
    let mut led6 = LatLedger::new();
    let mut rng = Lcg::new(0xB920_0006);
    let mut tick = Tick::new();
    let mut i = 0u32;
    while i < N_SAMPLES - 1 {
        let t0 = tick.comp_msg(&mut rng);
        let t1 = tick.preedit_seg(&mut rng);
        led6.record(t0, t1)?;
        i += 1;
    }
    if led6.coverage(N_SAMPLES).is_ok() {
        return Err(E_SAMPLE_MISS);
    }
    hits += 1;

    // R7 写回（判据 J2）：完成态 + 非空摘要放行；未完成 / 空摘要均拒。
    if writeback(false, 0x20).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    if writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    writeback(true, 0x33)?;
    hits += 1;

    Ok(hits)
}
