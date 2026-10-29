//! UNX-F48655 —— I33 性能面（首启 IME 就绪 ≤3s 采样，试产校准定）
//! 判据原文：UNX-F48655-J1 10 次首启采样落账；判据锚定 附则五。
//! 判据落地（drill 场景）：R1 采样 ×10 落账；R2 阈值判定；R3 排序单调；
//! R4 P99 计算；R5 序号唯一；R6 超限显性化；R7 阈值冻结；R8 指纹；
//! R9 呈现行；R10 计数守恒。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；阈值冻结常数（试产校准定：允许带校准计划调整）。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0900;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_COUNT: u16 = SEG_BASE + 1; // 采样数不足 10
const E_LIMIT: u16 = SEG_BASE + 2; // 就绪时长越限未显性化
const E_SORT: u16 = SEG_BASE + 3; // 排序结果非单调
const E_P99: u16 = SEG_BASE + 4; // 分位数计算漂移
const E_SEQ: u16 = SEG_BASE + 5; // 采样序号重复
const E_STATE: u16 = SEG_BASE + 6; // 账面状态非法
const E_FREEZE: u16 = SEG_BASE + 7; // 阈值常数漂移
const E_FP: u16 = SEG_BASE + 8; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 9; // 呈现行缺段
const E_SANE: u16 = SEG_BASE + 10; // 账面自洽性破坏

/// 首启 IME 就绪阈值（ms，试产校准定）。
pub const READY_LIMIT_MS: u32 = 3000;
/// 判据要求的采样次数。
pub const SAMPLE_N: usize = 10;
/// 首启状态闭集：1 冷启 / 2 温启。
pub const BOOT_COLD: u8 = 1;
pub const BOOT_WARM: u8 = 2;

/// FNV-1a 64 位。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
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

/// 一笔首启就绪采样。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BootSample {
    pub seq: u32,
    pub boot_kind: u8,
    pub ready_ms: u32,
}

impl BootSample {
    /// 是否越限。
    pub fn over(&self) -> bool {
        self.ready_ms > READY_LIMIT_MS
    }
}

/// 插入排序（升序，no_std 自实现，避免依赖切片排序）。
pub fn sort_u32(v: &mut Vec<u32>) {
    let n = v.len();
    let mut i = 1usize;
    while i < n {
        let key = v[i];
        let mut j = i;
        while j > 0 && v[j - 1] > key {
            v[j] = v[j - 1];
            j -= 1;
        }
        v[j] = key;
        i += 1;
    }
}

/// 首启采样账。
pub struct BootSampler {
    pub samples: Vec<BootSample>,
    pub next_seq: u32,
}

impl BootSampler {
    pub fn new() -> BootSampler {
        BootSampler { samples: vec![], next_seq: 1 }
    }

    /// 登记一笔采样：序号自动分配，启动类别闭集校验。
    pub fn record(&mut self, boot_kind: u8, ready_ms: u32) -> Result<u32, u16> {
        if boot_kind != BOOT_COLD && boot_kind != BOOT_WARM {
            return Err(E_STATE);
        }
        let seq = self.next_seq;
        self.samples.push(BootSample { seq, boot_kind, ready_ms });
        self.next_seq += 1;
        Ok(seq)
    }

    /// 就绪时长向量（升序副本）。
    pub fn sorted_ms(&self) -> Vec<u32> {
        let mut v: Vec<u32> = vec![];
        let mut i = 0usize;
        while i < self.samples.len() {
            v.push(self.samples[i].ready_ms);
            i += 1;
        }
        sort_u32(&mut v);
        v
    }

    /// P99（最近邻取位：idx = ceil(0.99*n)-1）。
    pub fn p99(&self) -> u32 {
        let v = self.sorted_ms();
        if v.is_empty() {
            return 0;
        }
        let n = v.len();
        let idx = (n * 99 + 99) / 100;
        let idx = if idx == 0 { 0 } else { idx - 1 };
        let idx = if idx >= n { n - 1 } else { idx };
        v[idx]
    }

    /// 越限笔数。
    pub fn over_count(&self) -> u32 {
        let mut c: u32 = 0;
        let mut i = 0usize;
        while i < self.samples.len() {
            if self.samples[i].over() {
                c += 1;
            }
            i += 1;
        }
        c
    }

    /// 序号唯一性。
    pub fn seq_unique(&self) -> bool {
        let mut i = 0usize;
        while i < self.samples.len() {
            let mut j = i + 1;
            while j < self.samples.len() {
                if self.samples[i].seq == self.samples[j].seq {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }
}

/// 账面指纹。
pub fn sampler_fp(s: &BootSampler) -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < s.samples.len() {
        buf.extend_from_slice(&s.samples[i].seq.to_le_bytes());
        buf.extend_from_slice(&s.samples[i].boot_kind.to_le_bytes());
        buf.extend_from_slice(&s.samples[i].ready_ms.to_le_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// 呈现行：n=<笔数> p99=<ms> over=<越限笔数> state=<pass|over>。
pub fn sampler_line(s: &BootSampler) -> String {
    let mut out = String::new();
    out.push_str("n=");
    out.push_str(&s.samples.len().to_string());
    out.push_str(" p99=");
    out.push_str(&s.p99().to_string());
    out.push_str(" over=");
    out.push_str(&s.over_count().to_string());
    if s.over_count() == 0 {
        out.push_str(" state=pass");
    } else {
        out.push_str(" state=over");
    }
    out
}

/// 子串判定（朴素扫描）。
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

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 采样 ×10 落账：LCG 生成 800..2800ms，全部入账。
    let mut s = BootSampler::new();
    let mut g = Lcg::new(0x486_55);
    let mut t = 0usize;
    while t < SAMPLE_N {
        let ms = 800 + g.below(2000);
        let kind = if t % 2 == 0 { BOOT_COLD } else { BOOT_WARM };
        s.record(kind, ms)?;
        t += 1;
    }
    if s.samples.len() != SAMPLE_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R2 阈值判定：全样本 ≤3000ms（构造域保证），越限计数为 0。
    if s.over_count() != 0 {
        return Err(E_LIMIT);
    }
    hits += 1;

    // R3 排序单调：升序向量逐位不减。
    let v = s.sorted_ms();
    let mut i = 1usize;
    while i < v.len() {
        if v[i - 1] > v[i] {
            return Err(E_SORT);
        }
        i += 1;
    }
    hits += 1;

    // R4 P99 计算：P99 == 排序后最大值（n=10 时取末位）。
    let p = s.p99();
    if p != v[v.len() - 1] || p == 0 {
        return Err(E_P99);
    }
    hits += 1;

    // R5 序号唯一：seq 1..10 互异。
    if !s.seq_unique() {
        return Err(E_SEQ);
    }
    hits += 1;

    // R6 超限显性化：注入 5000ms → over_count=1、state=over。
    let mut s6 = BootSampler::new();
    let mut i: u32 = 0;
    while i < 3 {
        s6.record(BOOT_COLD, 1200 + i * 100)?;
        i += 1;
    }
    s6.record(BOOT_WARM, 5000)?;
    if s6.over_count() != 1 {
        return Err(E_LIMIT);
    }
    let l6 = sampler_line(&s6);
    if !has_kw(l6.as_bytes(), b"state=over") {
        return Err(E_STATE);
    }
    hits += 1;

    // R7 阈值冻结：READY_LIMIT_MS == 3000。
    if READY_LIMIT_MS != 3000 {
        return Err(E_FREEZE);
    }
    hits += 1;

    // R8 指纹稳定。
    let f1 = sampler_fp(&s);
    let f2 = sampler_fp(&s);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R9 呈现行：n=/p99=/over= 关键字齐。
    let line = sampler_line(&s);
    if !has_kw(line.as_bytes(), b"n=10") || !has_kw(line.as_bytes(), b"p99=")
        || !has_kw(line.as_bytes(), b"state=pass")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 计数守恒：next_seq == 笔数+1，空账 p99=0。
    if s.next_seq != s.samples.len() as u32 + 1 {
        return Err(E_SANE);
    }
    let empty = BootSampler::new();
    if empty.p99() != 0 || empty.samples.len() != 0 {
        return Err(E_SANE);
    }
    hits += 1;

    Ok(hits)
}
