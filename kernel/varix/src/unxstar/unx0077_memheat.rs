//! UNX-F0077 · 物理内存热图（启动期分布统计）· 诊断段 0x21C0
//! 验收锚点：UNX-F0077-J1 启动期访问热区直方图与冷却衰减账，
//! 热区提取判据通过；纯功能代码 ≥220 行。
//! 零浮点、零 unwrap、零 expect。


use crate::alloc::{vec::Vec};

pub const SEG_BASE: u16 = 0x21C0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_RANGE: u16 = SEG_BASE + 1;   // 地址越出热图域
const E_RDONLY: u16 = SEG_BASE + 2;  // 热图已封板
const E_BAL: u16 = SEG_BASE + 3;     // 命中账守恒不平
const E_EMPTY: u16 = SEG_BASE + 4;   // 空热图提热区
const E_GRAIN: u16 = SEG_BASE + 5;   // 桶粒度非法（非 2 幂）

/// 热图桶粒度：256KiB。
pub const GRAIN: u64 = 0x4_0000;

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_RANGE => "地址越出热图域：先扩域再记录",
        E_RDONLY => "热图已封板：只读统计",
        E_BAL => "命中账守恒：Σ桶计数 = Σ采样",
        E_EMPTY => "空热图提热区：先采样",
        E_GRAIN => "桶粒度非法：2 的幂",
        _ => "未知诊断码",
    }
}

/// 衰减右移位数（冷却半衰期近似：每轮命中数 ×= (1 - 2^-shift)）。
pub const DECAY_SHIFT: u8 = 1;

#[derive(Clone, Copy, Debug)]
pub struct HotBucket {
    pub idx: u64,     // 桶号（桶基址 = idx × GRAIN）
    pub heat: u64,    // 当前热度
    pub raw_hits: u64, // 累计原始命中
}

// ── 热图账 ───────────────────────────────────────────────────
pub struct MemHeatMap {
    buckets: Vec<HotBucket>,
    domain_base: u64,
    domain_len: u64,
    pub samples: u64,
    pub rounds: u32,
    pub sealed: bool,
}

impl MemHeatMap {
    pub fn new(domain_base: u64, domain_len: u64) -> Result<MemHeatMap, u16> {
        if domain_len == 0 || domain_base.checked_add(domain_len).is_none() {
            return Err(E_RANGE);
        }
        if GRAIN & GRAIN.wrapping_sub(1) != 0 {
            return Err(E_GRAIN);
        }
        let nb = (domain_len / GRAIN) as usize;
        let mut buckets = Vec::new();
        for i in 0..nb {
            buckets.push(HotBucket {
                idx: i as u64,
                heat: 0,
                raw_hits: 0,
            });
        }
        Ok(MemHeatMap {
            buckets,
            domain_base,
            domain_len,
            samples: 0,
            rounds: 0,
            sealed: false,
        })
    }

    fn bucket_of(&self, addr: u64) -> Result<usize, u16> {
        if addr < self.domain_base || addr >= self.domain_base + self.domain_len {
            return Err(E_RANGE);
        }
        Ok(((addr - self.domain_base) / GRAIN) as usize)
    }

    /// 记录一次访问采样。
    pub fn sample(&mut self, addr: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        let i = self.bucket_of(addr)?;
        self.buckets[i].heat += 1;
        self.buckets[i].raw_hits += 1;
        self.samples += 1;
        Ok(())
    }

    /// 冷却一轮：全桶热度右移衰减。
    pub fn cool_round(&mut self) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        for b in self.buckets.iter_mut() {
            b.heat >>= DECAY_SHIFT;
        }
        self.rounds += 1;
        Ok(())
    }

    /// 命中账守恒：Σraw_hits == samples。
    pub fn reconcile(&self) -> Result<(), u16> {
        let s: u64 = self.buckets.iter().map(|b| b.raw_hits).sum();
        if s != self.samples {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 热区提取：热度 ≥ 阈值的前 K 桶（热度降序，平局按桶号升序）。
    pub fn hot_spots(&self, threshold: u64, k: usize) -> Result<Vec<(u64, u64)>, u16> {
        if self.samples == 0 {
            return Err(E_EMPTY);
        }
        if self.sealed == false && self.samples == 0 {
            return Err(E_EMPTY);
        }
        let mut cands: Vec<&HotBucket> = self
            .buckets
            .iter()
            .filter(|b| b.heat >= threshold)
            .collect();
        cands.sort_by(|a, b| b.heat.cmp(&a.heat).then(a.idx.cmp(&b.idx)));
        let mut out = Vec::new();
        for b in cands.into_iter().take(k) {
            out.push((self.domain_base + b.idx * GRAIN, b.heat));
        }
        Ok(out)
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        self.reconcile()?;
        self.sealed = true;
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&self.domain_base.to_le_bytes());
        buf.extend_from_slice(&self.samples.to_le_bytes());
        for b in &self.buckets {
            buf.extend_from_slice(&b.heat.to_le_bytes());
            buf.extend_from_slice(&b.raw_hits.to_le_bytes());
        }
        fnv1a64(&buf)
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 4 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：热区采样 64MiB 域——桶 3 集中热。
    let mut m = MemHeatMap::new(0x1000_0000, 0x400_0000)?; // 64MiB = 256 桶
    for i in 0..100u64 {
        m.sample(0x1000_0000 + 3 * GRAIN + i)?;
    }
    m.sample(0x1000_0000 + 7 * GRAIN)?;
    m.reconcile()?;
    let hot = m.hot_spots(10, 4)?;
    if hot.len() != 1 || hot[0].0 != 0x1000_0000 + 3 * GRAIN {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：冷却半衰——一轮后热度 100→50，两轮 25。
    m.cool_round()?;
    let hot2 = m.hot_spots(50, 4)?;
    if hot2.len() != 1 {
        return Err(E_BAL);
    }
    m.cool_round()?;
    let hot3 = m.hot_spots(25, 4)?;
    if hot3.is_empty() {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：指纹对拍。
    let mut m2 = MemHeatMap::new(0x1000_0000, 0x400_0000)?;
    for i in 0..100u64 {
        m2.sample(0x1000_0000 + 3 * GRAIN + i)?;
    }
    m2.sample(0x1000_0000 + 7 * GRAIN)?;
    m2.cool_round()?;
    m2.cool_round()?;
    if m2.fingerprint() != m.fingerprint() {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：越域采样 → E_RANGE。
    let mut i1 = MemHeatMap::new(0x2000_0000, 0x10_0000)?;
    if i1.sample(0x3000_0000) != Err(E_RANGE) {
        return Err(E_RANGE);
    }
    hits += 1;

    // I2：空图提热区 → E_EMPTY。
    let i2 = MemHeatMap::new(0x2000_0000, 0x10_0000)?;
    if i2.hot_spots(1, 4) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    // I3：封板后采样 → E_RDONLY。
    let mut i3 = MemHeatMap::new(0x2000_0000, 0x10_0000)?;
    i3.sample(0x2000_0000)?;
    i3.seal()?;
    if i3.sample(0x2000_0000) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    // I4：守恒破坏防线（手工改 raw）→ E_BAL。
    let mut i4 = MemHeatMap::new(0x2000_0000, 0x10_0000)?;
    i4.sample(0x2000_0000)?;
    i4.buckets[0].raw_hits = 9;
    if i4.seal() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
