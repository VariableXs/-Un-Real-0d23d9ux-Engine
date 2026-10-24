//! UNX-F0090 · 物理基址选择策略 · 诊断段 0x2260
//! 验收锚点：UNX-F0090-J1 内核镜像物理落位在可用区内的策略
//! （首适配/最佳适配/对齐优先）三径与越区拒绝判据通过；
//! 纯功能代码 ≥240 行。零浮点、零 unwrap、零 expect。


use crate::alloc::{vec::Vec};

pub const SEG_BASE: u16 = 0x2260;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_STRAT: u16 = SEG_BASE + 1;   // 策略非法
const E_RANGE: u16 = SEG_BASE + 2;   // 区间倒序/回绕
const E_NOFIT: u16 = SEG_BASE + 3;   // 无区可容纳镜像
const E_ALIGN: u16 = SEG_BASE + 4;   // 落位未按对齐粒度
const E_RDONLY: u16 = SEG_BASE + 5;  // 策略已定案
const E_BAL: u16 = SEG_BASE + 6;     // 账守恒不平
const E_EMPTY: u16 = SEG_BASE + 7;   // 无可用区

/// 物理落位对齐：2MiB。
pub const PHYS_ALIGN: u64 = 0x20_0000;

/// 策略闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlacementStrategy {
    /// 首适配：第一个能容纳的可用区。
    FirstFit,
    /// 最佳适配：碎片最小的可用区。
    BestFit,
    /// 对齐优先：落位后前置空洞最小（对齐浪费最少）。
    AlignFirst,
}

impl PlacementStrategy {
    pub fn name(&self) -> &'static str {
        match self {
            PlacementStrategy::FirstFit => "首适配",
            PlacementStrategy::BestFit => "最佳适配",
            PlacementStrategy::AlignFirst => "对齐优先",
        }
    }
    pub fn from_id(v: u8) -> Result<PlacementStrategy, u16> {
        match v {
            1 => Ok(PlacementStrategy::FirstFit),
            2 => Ok(PlacementStrategy::BestFit),
            3 => Ok(PlacementStrategy::AlignFirst),
            _ => Err(E_STRAT),
        }
    }
}

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
        E_STRAT => "策略非法：三策略闭集外",
        E_RANGE => "区间倒序或回绕",
        E_NOFIT => "无区可容纳镜像：镜像过大或全区碎片化",
        E_ALIGN => "落位未按 2MiB 对齐",
        E_RDONLY => "策略已定案：一次性决策",
        E_BAL => "账守恒：评估区数 = 命中数 + 容不下数",
        E_EMPTY => "无可用区：先建区账",
        _ => "未知诊断码",
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Zone {
    pub base: u64,
    pub len: u64,
}

impl Zone {
    pub fn end(&self) -> u64 {
        self.base + self.len
    }
}

/// 单区落位：对齐上取整后的可容纳判定与浪费账。
/// 返回 Some((落位基址, 头浪费))。
pub fn try_place(z: &Zone, image: u64) -> Result<Option<(u64, u64)>, u16> {
    if image == 0 {
        return Err(E_BAL);
    }
    if z.len.checked_add(z.base).is_none() {
        return Err(E_RANGE);
    }
    let a = (z.base + PHYS_ALIGN - 1) & !(PHYS_ALIGN - 1);
    if a >= z.end() {
        return Ok(None);
    }
    if z.end() - a < image {
        return Ok(None);
    }
    Ok(Some((a, a - z.base)))
}

// ── 落位策略账 ───────────────────────────────────────────────
pub struct PlacementLedger {
    pub zones: Vec<Zone>,
    pub strategy: PlacementStrategy,
    pub decided: Option<(u64, u64)>, // (落位基址, 所在区下标)
    pub sealed: bool,
}

impl PlacementLedger {
    pub fn new(strategy: PlacementStrategy) -> PlacementLedger {
        PlacementLedger {
            zones: Vec::new(),
            strategy,
            decided: None,
            sealed: false,
        }
    }

    pub fn add_zone(&mut self, base: u64, len: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if len == 0 || base.checked_add(len).is_none() {
            return Err(E_RANGE);
        }
        self.zones.push(Zone { base, len });
        Ok(())
    }

    /// 执行策略：评估全部区，按策略择一。
    pub fn decide(&mut self, image: u64) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if self.zones.is_empty() {
            return Err(E_EMPTY);
        }
        let mut best: Option<(u64, u64, u64)> = None; // (base, score, zone_idx)
        for (i, z) in self.zones.iter().enumerate() {
            match try_place(z, image)? {
                Some((base, head_waste)) => {
                    let score = match self.strategy {
                        // 首适配：区序即分（先到先得，越小越优）
                        PlacementStrategy::FirstFit => i as u64,
                        // 最佳适配：区尾余量越小越优
                        PlacementStrategy::BestFit => z.end() - (base + image),
                        // 对齐优先：头浪费越小越优
                        PlacementStrategy::AlignFirst => head_waste,
                    };
                    let take = match best {
                        None => true,
                        Some((_, bs, _)) => score < bs,
                    };
                    if take {
                        best = Some((base, score, i as u64));
                    }
                }
                None => continue,
            }
        }
        match best {
            Some((base, _, zi)) => {
                if base % PHYS_ALIGN != 0 {
                    return Err(E_ALIGN);
                }
                self.decided = Some((base, zi));
                self.sealed = true;
                Ok(base)
            }
            None => Err(E_NOFIT),
        }
    }

    /// 账守恒：全部区都被评估（无法直接观测，用下标核验）。
    pub fn reconcile(&self, evaluated: usize) -> Result<(), u16> {
        if evaluated > self.zones.len() {
            return Err(E_BAL);
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.push(match self.strategy {
            PlacementStrategy::FirstFit => 1,
            PlacementStrategy::BestFit => 2,
            PlacementStrategy::AlignFirst => 3,
        });
        if let Some((b, i)) = self.decided {
            buf.extend_from_slice(&b.to_le_bytes());
            buf.extend_from_slice(&i.to_le_bytes());
        }
        fnv1a64(&buf)
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 4 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    const IMG: u64 = 0x40_0000; // 4MiB 镜像

    // 三区：甲 [0x100000,0x600000) 5MiB（容 4MiB，落 0x200000，尾余 0）
    // 乙 [0x800000,0xA00000) 2MiB（容不下）
    // 丙 [0x1000000,0x1800000) 8MiB
    let zones = [
        (0x10_0000u64, 0x50_0000u64), // 5MiB
        (0x80_0000, 0x20_0000),       // 2MiB
        (0x100_0000, 0x80_0000),      // 8MiB
    ];

    // R1：首适配 → 甲区落位 0x20_0000。
    let mut f1 = PlacementLedger::new(PlacementStrategy::FirstFit);
    for (b, l) in zones {
        f1.add_zone(b, l)?;
    }
    let b1 = f1.decide(IMG)?;
    if b1 != 0x20_0000 {
        return Err(E_ALIGN);
    }
    hits += 1;

    // R2：最佳适配 → 甲区（尾余 0 最小）。
    let mut f2 = PlacementLedger::new(PlacementStrategy::BestFit);
    for (b, l) in zones {
        f2.add_zone(b, l)?;
    }
    let b2 = f2.decide(IMG)?;
    if b2 != 0x20_0000 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：对齐优先 → 甲区头浪费 0x10_0000 最小（乙容不下，丙 0）。
    // 甲：0x100000→0x200000 头浪费 1MiB；丙：0x1000000 已对齐头浪费 0
    // 对齐优先应选丙！
    let mut f3 = PlacementLedger::new(PlacementStrategy::AlignFirst);
    for (b, l) in zones {
        f3.add_zone(b, l)?;
    }
    let b3 = f3.decide(IMG)?;
    if b3 != 0x100_0000 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：全容不下 → E_NOFIT。
    let mut f4 = PlacementLedger::new(PlacementStrategy::FirstFit);
    f4.add_zone(0x10_0000, 0x20_0000)?; // 2MiB < 4MiB 镜像
    if f4.decide(IMG) != Err(E_NOFIT) {
        return Err(E_NOFIT);
    }
    hits += 1;

    // I1：策略越纲 → E_STRAT。
    if PlacementStrategy::from_id(9) != Err(E_STRAT) {
        return Err(E_STRAT);
    }
    hits += 1;

    // I2：零镜像 → E_BAL。
    let mut i2 = PlacementLedger::new(PlacementStrategy::FirstFit);
    i2.add_zone(0x10_0000, 0x80_0000)?;
    if i2.decide(0) != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    // I3：定案后再决定 → E_RDONLY。
    if f1.decide(IMG) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    // I4：空区账 → E_EMPTY。
    let mut i4 = PlacementLedger::new(PlacementStrategy::FirstFit);
    if i4.decide(IMG) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    Ok(hits)
}
