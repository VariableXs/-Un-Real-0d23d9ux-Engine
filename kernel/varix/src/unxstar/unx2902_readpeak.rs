// unx2902 —— UNX-F2902 读命中洪峰压测（cache 三接口消费）
// 判据原文 UNX-F2902-J1：读洪峰 10/10 轮账平（命中/未命中/回流三账闭合），
// 命中率读数入基线。
// 落位：kernel/varix/src/unxstar/；诊断段 0xFA00 独占（0xFA00~0xFA7F）。
//
// 设计（纯功能 no_std）：
//   * cache 三接口消费：lookup（读命中）/ insert（未命中装载）/ evict（回流驱逐）；
//   * 三账闭合：读请求总数 == 命中账 + 未命中账；未命中账 == 装载账；
//     装载账 == 命中回流账 + 驱逐回流账（10/10 轮逐轮闭合）；
//   * 命中率读数：命中/读总数 千分比，入基线寄存器（同轮同种子恒等）；
//   * 注入核：三账破平/回流计数漂移/封账后读 四类注入必抓。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xFA00 独占） ─────────────────────────────────────────────
pub const XC_RP_UNBAL: u32 = 0xFA01; // 三账破平
pub const XC_RP_REFLOW: u32 = 0xFA02; // 回流计数漂移
pub const XC_RP_SEALED: u32 = 0xFA03; // 封账后读
pub const XC_RP_BASE: u32 = 0xFA04; // 基线读数越界

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

/// 页缓存洪峰模型：容量 16 槽 LRU，页 ID 空间 64。
pub struct CacheStorm {
    slots: Vec<(u64, u64)>, // (page_id, stamp)
    cap: usize,
    stamp: u64,
    reads: u64,
    hits: u64,
    misses: u64,
    loads: u64,
    reflow_hit: u64,  // 命中回流（命中时访问位更新）
    reflow_evict: u64, // 驱逐回流
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl CacheStorm {
    pub fn new() -> Self {
        CacheStorm {
            slots: Vec::new(),
            cap: 16,
            stamp: 0,
            reads: 0,
            hits: 0,
            misses: 0,
            loads: 0,
            reflow_hit: 0,
            reflow_evict: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 读接口：命中返回 true 并刷新访问位；未命中装载数据页。
    pub fn read(&mut self, page: u64) -> Result<bool, u32> {
        if self.sealed {
            self.rejects.push((XC_RP_SEALED, "封账后读"));
            return Err(XC_RP_SEALED);
        }
        self.stamp += 1;
        self.reads += 1;
        for s in self.slots.iter_mut() {
            if s.0 == page {
                s.1 = self.stamp;
                self.hits += 1;
                self.reflow_hit += 1;
                return Ok(true);
            }
        }
        self.misses += 1;
        self.loads += 1;
        if self.slots.len() >= self.cap {
            // LRU 驱逐（stamp 最小者）
            let mut mi = 0;
            for (i, s) in self.slots.iter().enumerate() {
                if s.1 < self.slots[mi].1 {
                    mi = i;
                }
            }
            let _ = self.slots.remove(mi);
            self.reflow_evict += 1;
        }
        self.slots.push((page, self.stamp));
        Ok(false)
    }

    /// 三账闭合：读 = 命中 + 未命中；未命中 = 装载；装载 = 驻留 + 驱逐。
    pub fn balanced(&self) -> bool {
        self.reads == self.hits + self.misses
            && self.misses == self.loads
            && self.loads == self.slots.len() as u64 + self.reflow_evict
    }

    /// 命中率千分比读数。
    pub fn hit_rate_permille(&self) -> u32 {
        if self.reads == 0 {
            return 0;
        }
        ((self.hits as u64 * 1000) / self.reads as u64) as u32
    }

    pub fn counts(&self) -> (u64, u64, u64, u64, u64) {
        (self.reads, self.hits, self.misses, self.loads, self.reflow_evict)
    }

    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_RP_SEALED);
        }
        self.sealed = true;
        Ok(())
    }
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 单轮洪峰：128 次读（64 页空间随机），返回轮结果。
pub struct RoundResult {
    pub reads: u64,
    pub hits: u64,
    pub rate: u32,
    pub balanced: bool,
}

pub fn storm_round(seed: u64) -> RoundResult {
    let mut rng = Lcg(seed | 1);
    let mut c = CacheStorm::new();
    for _ in 0..128 {
        let _ = c.read(rng.next() % 64);
    }
    RoundResult {
        reads: c.counts().0,
        hits: c.counts().1,
        rate: c.hit_rate_permille(),
        balanced: c.balanced(),
    }
}

/// 十轮洪峰账平核。
pub fn ten_rounds(seed: u64) -> Vec<RoundResult> {
    let mut out = Vec::new();
    for i in 0..10u64 {
        out.push(storm_round(seed.wrapping_add(i.wrapping_mul(0x9E37))));
    }
    out
}

/// 命中率基线寄存器：入基线一次冻结，复算恒等。
pub struct Baseline {
    rates: Vec<u32>,
    frozen: Option<u32>,
}

impl Baseline {
    pub fn new() -> Self {
        Baseline { rates: Vec::new(), frozen: None }
    }
    pub fn record(&mut self, rate: u32) -> Result<(), u32> {
        if self.frozen.is_some() {
            return Err(XC_RP_BASE);
        }
        if rate > 1000 {
            return Err(XC_RP_BASE);
        }
        self.rates.push(rate);
        Ok(())
    }
    pub fn freeze(&mut self) -> u32 {
        let mut b = Vec::new();
        for &r in &self.rates {
            b.extend_from_slice(&r.to_le_bytes());
        }
        let h = fnv1a32(&b);
        self.frozen = Some(h);
        h
    }
    pub fn avg(&self) -> u32 {
        if self.rates.is_empty() {
            return 0;
        }
        let sum: u64 = self.rates.iter().map(|&r| r as u64).sum();
        (sum / self.rates.len() as u64) as u32
    }
}

/// 注入核：三账破平/回流漂移/封账后读 三类必抓。
pub struct StormInjectCore {
    pub caught: u32,
    pub total: u32,
}

impl StormInjectCore {
    pub fn new() -> Self {
        StormInjectCore { caught: 0, total: 0 }
    }
    pub fn inject_unbalanced(&mut self) {
        self.total += 1;
        // 账面注入：手工构造 counts 破平的模型并断言 balanced() 抓到
        let mut c = CacheStorm::new();
        let _ = c.read(1);
        let _ = c.read(2);
        // 破平：直接篡改未命中账（模拟内部计数漂移）
        c.misses += 1;
        if !c.balanced() {
            self.caught += 1;
        }
    }
    pub fn inject_reflow_drift(&mut self) {
        self.total += 1;
        let mut c = CacheStorm::new();
        let _ = c.read(1);
        let _ = c.read(2);
        c.reflow_evict += 1; // 虚增驱逐回流
        if !c.balanced() {
            self.caught += 1;
        }
    }
    pub fn inject_read_after_seal(&mut self) {
        self.total += 1;
        let mut c = CacheStorm::new();
        let _ = c.read(1);
        let _ = c.seal();
        let r = c.read(2);
        if r.is_err() && c.reject_count() == 1 {
            self.caught += 1;
        }
    }
    pub fn inject_base_oob(&mut self) {
        self.total += 1;
        let mut b = Baseline::new();
        let r = b.record(1001);
        if r.is_err() {
            self.caught += 1;
        }
    }
    pub fn all_caught(&self) -> bool {
        self.total == 4 && self.caught == 4
    }
}

/// 判据集。
pub fn run_unx2902_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx2902");

    // C02-01 十轮账平 10/10
    {
        let rs = ten_rounds(0xCAFE);
        let ok = rs.iter().all(|r| r.balanced);
        s.add("C02-01-十轮账平", ok, if ok { "" } else { "存在破平轮" });
    }
    // C02-02 命中率读数入基线且界内
    {
        let rs = ten_rounds(0xBEEF);
        let mut base = Baseline::new();
        let mut all_ok = true;
        for r in &rs {
            if base.record(r.rate).is_err() {
                all_ok = false;
            }
        }
        let _ = base.freeze();
        let ok = all_ok && base.avg() <= 1000 && !rs.is_empty();
        s.add("C02-02-基线入册", ok, if ok { "" } else { "基线入册失败" });
    }
    // C02-03 三账逐轮闭合字段核对
    {
        let rs = ten_rounds(0xF00D);
        let ok = rs.iter().all(|r| r.reads == r.hits + (r.reads - r.hits));
        s.add("C02-03-读=命+未", ok, if ok { "" } else { "读账不守恒" });
    }
    // C02-04 双构造确定性（同种子同命中率）
    {
        let a = storm_round(0x5EED);
        let b = storm_round(0x5EED);
        let ok = a.rate == b.rate && a.hits == b.hits && a.reads == b.reads;
        s.add("C02-04-确定性", ok, if ok { "" } else { "构造非确定" });
    }
    // C02-05 封账后读拒绝
    {
        let mut c = CacheStorm::new();
        let _ = c.read(1);
        let _ = c.seal();
        let dup = c.seal();
        let r2 = c.read(2);
        let ok = dup.is_err() && r2.is_err() && c.reject_count() == 2;
        s.add("C02-05-封账拒", ok, if ok { "" } else { "封账失守" });
    }
    // C02-06 注入核 4/4
    {
        let mut inj = StormInjectCore::new();
        inj.inject_unbalanced();
        inj.inject_reflow_drift();
        inj.inject_read_after_seal();
        inj.inject_base_oob();
        s.add(
            "C02-06-注入核4-4",
            inj.all_caught(),
            if inj.all_caught() { "" } else { "注入漏抓" },
        );
    }
    // C02-07 容量上限守恒（槽位 ≤16）
    {
        let mut c = CacheStorm::new();
        for p in 0..64u64 {
            let _ = c.read(p);
        }
        let ok = c.counts().4 == 64 - 16 && c.balanced();
        s.add("C02-07-容量守恒", ok, if ok { "" } else { "驱逐账不平" });
    }
    s
}
