//! UNX-F48538 —— E27 性能面（热切换处理 ≤5ms 采样）
//! 判据原文：UNX-F48538-J1 热切换（接入/移除/换装）处理耗时按采样记账，
//! 单笔处理预算 ≤5000μs：采样批次逐笔不超预算、批均值 ≤ 预算的一半、
//! 超预算笔全额记账且占比受控、采样守恒（笔数一致）、指纹可复核。
//! 判据落地（drill 场景）：R1 采样器基准（正常样本全收）；R2 逐笔预算硬顶；
//! R3 批均值 ≤ 预算/2；R4 超预算占比受控（≤ 1%）；R5 采样守恒；
//! R6 采样时戳单调；R7 分位数复算（P95 ≤ 预算）；R8 指纹稳定；
//! R9 呈现行齐全；R10 零耗时样本不崩。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xF0A0;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_SAMPLE: u16 = SEG_BASE + 1; // 采样记录字段越界
const E_BUDGET: u16 = SEG_BASE + 2; // 单笔超预算硬顶
const E_MEAN: u16 = SEG_BASE + 3; // 批均值超预算一半
const E_RATIO: u16 = SEG_BASE + 4; // 超预算占比超 1%
const E_CONSERV: u16 = SEG_BASE + 5; // 采样守恒破缺
const E_ORDER: u16 = SEG_BASE + 6; // 采样时戳非单调
const E_P95: u16 = SEG_BASE + 7; // P95 复算超预算
const E_FP: u16 = SEG_BASE + 8; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 9; // 呈现行缺段
const E_ZERO: u16 = SEG_BASE + 10; // 零耗时样本处理异常

/// 单笔热切换处理预算（微秒）。
pub const BUDGET_US: u32 = 5000;
/// 超预算占比上限（放大 100 倍整数，1% = 100）。
pub const OVER_RATIO_CAP_X100: u64 = 100;
/// 采样批容量。
pub const BATCH_N: u32 = 200;

/// 一笔采样：时戳 / 处理耗时（μs）/ 事件类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sample {
    pub at: u32,
    pub cost_us: u32,
    pub kind: u8,
}

/// FNV-1a 64 位（与本域其余哨兵同口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG（批内统一系数）。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 采样批：记录 + 超预算笔数。
pub struct SampleBatch {
    pub rows: Vec<Sample>,
    pub over: u32,
}

impl SampleBatch {
    pub fn new() -> SampleBatch {
        SampleBatch {
            rows: vec![],
            over: 0,
        }
    }

    /// 灌入一笔：字段齐备（事件类别 1..=3），超预算记账。
    pub fn feed(&mut self, s: Sample) -> Result<(), u16> {
        if s.kind < 1 || s.kind > 3 {
            return Err(E_SAMPLE);
        }
        if s.cost_us > BUDGET_US {
            self.over += 1;
        }
        self.rows.push(s);
        Ok(())
    }

    /// 采样守恒：记录笔数 + 0 == 恰好每笔都入账（本卡口径：笔数可对账）。
    pub fn conserved(&self, injected: u32) -> bool {
        self.rows.len() as u32 == injected
    }

    /// 批均值（μs，向下取整）。
    pub fn mean_us(&self) -> u64 {
        if self.rows.is_empty() {
            return 0;
        }
        let mut sum: u64 = 0;
        let mut i = 0usize;
        while i < self.rows.len() {
            sum += self.rows[i].cost_us as u64;
            i += 1;
        }
        sum / self.rows.len() as u64
    }

    /// 超预算占比（放大 100 倍）。
    pub fn over_ratio_x100(&self) -> u64 {
        if self.rows.is_empty() {
            return 0;
        }
        (self.over as u64) * 100 / self.rows.len() as u64
    }

    /// 时戳单调。
    pub fn monotonic(&self) -> bool {
        let mut i = 1usize;
        while i < self.rows.len() {
            if self.rows[i - 1].at > self.rows[i].at {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// P95 复算：升序副本取第 95 百分位（索引 n*95/100）。
pub fn p95_us(rows: &[Sample]) -> u64 {
    if rows.is_empty() {
        return 0;
    }
    let mut costs: Vec<u32> = vec![];
    let mut i = 0usize;
    while i < rows.len() {
        costs.push(rows[i].cost_us);
        i += 1;
    }
    // 插入排序（n ≤ 数百，无递归无分配风暴）。
    let mut i = 1usize;
    while i < costs.len() {
        let mut j = i;
        while j > 0 && costs[j - 1] > costs[j] {
            let t = costs[j - 1];
            costs[j - 1] = costs[j];
            costs[j] = t;
            j -= 1;
        }
        i += 1;
    }
    let idx = (costs.len() as u64 * 95 / 100) as usize;
    costs[idx] as u64
}

/// 采样批指纹。
pub fn batch_fp(b: &SampleBatch) -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < b.rows.len() {
        buf.extend_from_slice(&b.rows[i].at.to_le_bytes());
        buf.extend_from_slice(&b.rows[i].cost_us.to_le_bytes());
        buf.push(b.rows[i].kind);
        i += 1;
    }
    buf.extend_from_slice(&b.over.to_le_bytes());
    fnv1a64(&buf)
}

/// 采样行（呈现面）：n=<笔数> mean=<μs> over=<n> p95=<μs>。
pub fn batch_line(b: &SampleBatch) -> String {
    let mut s = String::new();
    s.push_str("n=");
    s.push_str(&(b.rows.len() as u32).to_string());
    s.push_str(" mean=");
    s.push_str(&b.mean_us().to_string());
    s.push_str(" over=");
    s.push_str(&b.over.to_string());
    s.push_str(" p95=");
    s.push_str(&p95_us(&b.rows).to_string());
    s
}

/// 生成采样批：LCG 主耗时 800..3200μs，每 100 笔掺 1 笔贴边 4999μs。
pub fn gen_batch(mut lcg: Lcg, n: u32) -> SampleBatch {
    let mut b = SampleBatch::new();
    let mut i = 0u32;
    while i < n {
        let cost = if i % 100 == 99 {
            BUDGET_US - 1 // 贴边不超预算
        } else {
            800 + lcg.below(2400)
        };
        b.feed(Sample {
            at: i * 10,
            cost_us: cost,
            kind: 1 + (i % 3) as u8,
        })
        .ok();
        i += 1;
    }
    b
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 采样器基准：正常样本（耗时 < 预算）全收且 over 记零。
    let mut b0 = SampleBatch::new();
    let mut t = 0u32;
    while t < 8 {
        b0.feed(Sample {
            at: t,
            cost_us: 1000 + t,
            kind: 1 + (t % 3) as u8,
        })?;
        t += 1;
    }
    if b0.over != 0 || b0.rows.len() != 8 {
        return Err(E_SAMPLE);
    }
    hits += 1;

    // R2/R3/R4 200 笔主批：逐笔不超硬顶、均值 ≤ 预算/2、超预算占比 0。
    let mut lcg = Lcg::new(0x48538);
    let b = gen_batch(Lcg::new(lcg.next_u32()), BATCH_N);
    let mut i = 0usize;
    while i < b.rows.len() {
        if b.rows[i].cost_us > BUDGET_US {
            return Err(E_BUDGET);
        }
        if b.rows[i].kind < 1 || b.rows[i].kind > 3 {
            return Err(E_SAMPLE);
        }
        i += 1;
    }
    if b.mean_us() > (BUDGET_US / 2) as u64 {
        return Err(E_MEAN);
    }
    if b.over_ratio_x100() > OVER_RATIO_CAP_X100 {
        return Err(E_RATIO);
    }
    hits += 1;

    // R5 采样守恒：批内笔数 == 注入笔数；超预算记账与 over 字段一致。
    if !b.conserved(BATCH_N) {
        return Err(E_CONSERV);
    }
    let mut manual_over = 0u32;
    let mut i = 0usize;
    while i < b.rows.len() {
        if b.rows[i].cost_us > BUDGET_US {
            manual_over += 1;
        }
        i += 1;
    }
    if manual_over != b.over {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R6 采样时戳单调：主批非降序；乱序批被检出。
    if !b.monotonic() {
        return Err(E_ORDER);
    }
    let mut bad = SampleBatch::new();
    bad.feed(Sample { at: 20, cost_us: 100, kind: 1 })?;
    bad.feed(Sample { at: 10, cost_us: 100, kind: 1 })?;
    if bad.monotonic() {
        return Err(E_ORDER);
    }
    hits += 1;

    // R7 P95 复算：主批 P95 ≤ 预算；构造超预算批后 P95 复算仍可检出。
    if p95_us(&b.rows) > BUDGET_US as u64 {
        return Err(E_P95);
    }
    let mut hot = SampleBatch::new();
    let mut t = 0u32;
    while t < 100 {
        hot.feed(Sample {
            at: t,
            cost_us: BUDGET_US + 100, // 全批超预算
            kind: 1,
        })?;
        t += 1;
    }
    if p95_us(&hot.rows) <= BUDGET_US as u64 {
        return Err(E_P95);
    }
    if hot.over != 100 {
        return Err(E_BUDGET);
    }
    hits += 1;

    // R8 指纹稳定：主批两次指纹一致且非零。
    let f1 = batch_fp(&b);
    let f2 = batch_fp(&b);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R9 呈现行齐全：n=/mean=/over=/p95= 四段在位。
    let line = batch_line(&b);
    if !has_kw(line.as_bytes(), b"n=")
        || !has_kw(line.as_bytes(), b"mean=")
        || !has_kw(line.as_bytes(), b"over=")
        || !has_kw(line.as_bytes(), b"p95=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 零耗时样本不崩：全零耗时批入账、均值 0、P95 0、守恒成立。
    let mut z = SampleBatch::new();
    let mut t = 0u32;
    while t < 16 {
        z.feed(Sample { at: t, cost_us: 0, kind: 1 + (t % 3) as u8 })?;
        t += 1;
    }
    if z.mean_us() != 0 || p95_us(&z.rows) != 0 || !z.conserved(16) {
        return Err(E_ZERO);
    }
    hits += 1;

    Ok(hits)
}

/// 子串判定（朴素扫描，统一口径）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}
