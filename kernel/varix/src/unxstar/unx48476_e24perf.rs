//! UNX-F48476 · E24 性能面（消息处理 ≤0.5ms 采样）
//!
//! 判据原文：UNX-F48476-J1 —— 协议消息处理器在确定性成本模型下逐条
//! 采样：单条处理成本不得超过预算 BUDGET（0.5ms 等价操作数 500）；
//! 1000 条混合负载 P95 与 Max 双双达标；超标即记罚单（罚单必须为零）；
//! 大载荷（256 字节级）消息同样在预算内；成本账守恒（各桶计数之和 ==
//! 总样本数）；同种子重放采样序列一致。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；时间以
//! 操作计数模拟（确定性，无真实时钟依赖）；fuzz 用确定性 LCG。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xE540;

pub const E_OVER_BUDGET: u16 = SEG_BASE + 1;
pub const E_LEDGER_BAD: u16 = SEG_BASE + 2;
pub const E_NONDET: u16 = SEG_BASE + 3;
pub const E_NO_SAMPLE: u16 = SEG_BASE + 4;
pub const E_P95_MISS: u16 = SEG_BASE + 5;

/// 预算：0.5ms 等价操作数。
pub const BUDGET: u32 = 500;
/// 采样条数。
pub const SAMPLE_N: usize = 1000;

/// 协议消息：类型 + 载荷。
#[derive(Clone)]
pub struct Msg {
    pub kind: u8,
    pub payload: Vec<u8>,
}

impl Msg {
    /// 确定性成本模型：固定开销 + 每字节计费 + 类型系数。
    pub fn cost(&self) -> u32 {
        let kind_fee: u32 = match self.kind {
            0 => 20,  // 心跳：轻
            1 => 40,  // 输入事件：中
            2 => 60,  // 属性变更：中重
            3 => 80,  // 焦点迁移：重
            4 => 100, // 大同步：最重
            _ => 50,
        };
        let bytes_fee: u32 = (self.payload.len() as u32) * 1;
        let mix_fee: u32 = (self.payload.iter().fold(0u32, |a, b| a + (*b as u32)) % 97);
        10 + kind_fee + bytes_fee + mix_fee
    }
}

/// 采样记录。
#[derive(Clone, PartialEq, Eq)]
pub struct Sample {
    pub idx: usize,
    pub cost: u32,
    pub kind: u8,
    pub fine: bool,
}

/// 成本桶：0..=250 / 251..=400 / 401..=BUDGET / 超 BUDGET。
pub const BUCKETS: usize = 4;

pub struct PerfHarness {
    pub samples: Vec<Sample>,
    pub buckets: [u64; BUCKETS],
}

impl PerfHarness {
    pub fn new() -> PerfHarness {
        PerfHarness { samples: Vec::new(), buckets: [0; BUCKETS] }
    }

    /// 处理一条消息：计成本、采样、入桶、判罚。
    pub fn process(&mut self, idx: usize, m: &Msg) -> Sample {
        let c = m.cost();
        let fine = c <= BUDGET;
        let s = Sample { idx, cost: c, kind: m.kind, fine };
        let b = if c <= 250 {
            0
        } else if c <= 400 {
            1
        } else if c <= BUDGET {
            2
        } else {
            3
        };
        self.buckets[b] += 1;
        self.samples.push(s.clone());
        s
    }

    /// 成本账守恒：桶和 == 样本数。
    pub fn audit_ledger(&self) -> Result<(), u16> {
        let sum: u64 = self.buckets.iter().fold(0u64, |a, b| a + b);
        if sum != self.samples.len() as u64 {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 罚单：超预算样本数（判据要求为 0）。
    pub fn violations(&self) -> u32 {
        let mut v = 0;
        for s in self.samples.iter() {
            if !s.fine {
                v += 1;
            }
        }
        v
    }

    /// P95：按成本升序第 95 百分位样本成本。
    pub fn p95(&self) -> Result<u32, u16> {
        if self.samples.is_empty() {
            return Err(E_NO_SAMPLE);
        }
        let mut costs: Vec<u32> = Vec::new();
        for s in self.samples.iter() {
            costs.push(s.cost);
        }
        sort_u32(&mut costs);
        let rank = (costs.len() * 95) / 100;
        if rank >= costs.len() {
            return Ok(costs[costs.len() - 1]);
        }
        Ok(costs[rank])
    }

    /// Max。
    pub fn max_cost(&self) -> Result<u32, u16> {
        let mut m: u32 = 0;
        let mut hit = false;
        for s in self.samples.iter() {
            if !hit || s.cost > m {
                m = s.cost;
                hit = true;
            }
        }
        if !hit {
            return Err(E_NO_SAMPLE);
        }
        Ok(m)
    }
}

/// no_std 插入排序（样本量小，O(n²) 可接受且确定性）。
pub fn sort_u32(a: &mut Vec<u32>) {
    let n = a.len();
    for i in 1..n {
        let mut j = i;
        while j > 0 && a[j - 1] > a[j] {
            let tmp = a[j - 1];
            a[j - 1] = a[j];
            a[j] = tmp;
            j -= 1;
        }
    }
}

/// 确定性 LCG。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.st
    }
    pub fn below(&mut self, b: u64) -> u64 {
        if b == 0 {
            return 0;
        }
        self.next() % b
    }
}

/// 构造混合负载：心跳 40% / 输入 30% / 属性 15% / 焦点 10% / 大同步 5%。
pub fn gen_workload(n: usize, seed: u64) -> Vec<Msg> {
    let mut g = Lcg::new(seed);
    let mut out = Vec::new();
    for _ in 0..n {
        let r = g.below(100);
        let kind: u8 = if r < 40 {
            0
        } else if r < 70 {
            1
        } else if r < 85 {
            2
        } else if r < 95 {
            3
        } else {
            4
        };
        let plen = match kind {
            0 => 0,
            1 => g.below(16) as usize,
            2 => g.below(64) as usize,
            3 => g.below(128) as usize,
            _ => g.below(256) as usize,
        };
        let mut payload = Vec::with_capacity(plen);
        for _ in 0..plen {
            payload.push((g.next() & 0xFF) as u8);
        }
        out.push(Msg { kind, payload });
    }
    out
}

/// R1 全采样达标：1000 条混合负载零罚单。
pub fn r1_all_within_budget() -> Result<(), u16> {
    let wl = gen_workload(SAMPLE_N, 0x48476_0001);
    let mut h = PerfHarness::new();
    for i in 0..wl.len() {
        h.process(i, &wl[i]);
    }
    if h.samples.len() != SAMPLE_N {
        return Err(E_NO_SAMPLE);
    }
    if h.violations() != 0 {
        return Err(E_OVER_BUDGET);
    }
    Ok(())
}

/// R2 P95 与 Max 双达标。
pub fn r2_p95_max() -> Result<(), u16> {
    let wl = gen_workload(SAMPLE_N, 0x48476_0002);
    let mut h = PerfHarness::new();
    for i in 0..wl.len() {
        h.process(i, &wl[i]);
    }
    let p = h.p95()?;
    let m = h.max_cost()?;
    if p > BUDGET {
        return Err(E_P95_MISS);
    }
    if m > BUDGET {
        return Err(E_OVER_BUDGET);
    }
    Ok(())
}

/// R3 成本账守恒：四桶之和 == 样本数。
pub fn r3_ledger() -> Result<(), u16> {
    let wl = gen_workload(500, 0x48476_0003);
    let mut h = PerfHarness::new();
    for i in 0..wl.len() {
        h.process(i, &wl[i]);
    }
    h.audit_ledger()
}

/// R4 大载荷预算内：kind=4 + 256 字节载荷，成本仍 ≤ BUDGET。
pub fn r4_big_payload() -> Result<(), u16> {
    let mut payload = Vec::new();
    let mut g = Lcg::new(0x48476_0004);
    for _ in 0..256 {
        payload.push((g.next() & 0xFF) as u8);
    }
    let m = Msg { kind: 4, payload };
    let c = m.cost();
    if c > BUDGET {
        return Err(E_OVER_BUDGET);
    }
    // 单调性：空载荷版本更便宜。
    let empty = Msg { kind: 4, payload: Vec::new() };
    if empty.cost() >= c {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R5 心跳最轻：kind=0 空载荷成本 < 其他全部 kind 的同长成本。
pub fn r5_heartbeat_lightest() -> Result<(), u16> {
    let hb = Msg { kind: 0, payload: Vec::new() };
    for k in 1..5u8 {
        let m = Msg { kind: k, payload: Vec::new() };
        if hb.cost() >= m.cost() {
            return Err(E_LEDGER_BAD);
        }
    }
    Ok(())
}

/// R6 确定性重放：同种子两次负载，采样成本序列全等。
pub fn r6_deterministic() -> Result<(), u16> {
    let wa = gen_workload(200, 0x48476_0005);
    let wb = gen_workload(200, 0x48476_0005);
    if wa.len() != wb.len() {
        return Err(E_NONDET);
    }
    for i in 0..wa.len() {
        if wa[i].cost() != wb[i].cost() {
            return Err(E_NONDET);
        }
    }
    Ok(())
}

/// R7 排序器健全：乱序数组排序后单调不减。
pub fn r7_sort_sane() -> Result<(), u16> {
    let mut a: Vec<u32> = Vec::new();
    let mut g = Lcg::new(0x48476_0006);
    for _ in 0..64 {
        a.push((g.next() % 1000) as u32);
    }
    sort_u32(&mut a);
    for i in 1..a.len() {
        if a[i - 1] > a[i] {
            return Err(E_NONDET);
        }
    }
    Ok(())
}

/// R8 P95 定义核对：手工算 rank 取样与实现一致。
pub fn r8_p95_definition() -> Result<(), u16> {
    let wl = gen_workload(400, 0x48476_0007);
    let mut h = PerfHarness::new();
    for i in 0..wl.len() {
        h.process(i, &wl[i]);
    }
    let mut costs: Vec<u32> = Vec::new();
    for s in h.samples.iter() {
        costs.push(s.cost);
    }
    sort_u32(&mut costs);
    let rank = (costs.len() * 95) / 100;
    let expect = if rank < costs.len() { costs[rank] } else { costs[costs.len() - 1] };
    if h.p95()? != expect {
        return Err(E_P95_MISS);
    }
    Ok(())
}

/// R9 罚单语义：构造超预算消息（kind=4 + 400 字节）必被抓罚。
pub fn r9_violation_caught() -> Result<(), u16> {
    let mut payload = Vec::new();
    let mut g = Lcg::new(0x48476_0008);
    for _ in 0..400 {
        payload.push(0xFFu8);
    }
    let m = Msg { kind: 4, payload };
    let mut h = PerfHarness::new();
    h.process(0, &m);
    if h.violations() != 1 {
        return Err(E_OVER_BUDGET);
    }
    if h.buckets[3] != 1 {
        return Err(E_LEDGER_BAD);
    }
    Ok(())
}

/// R10 空采样拒报：空 harness 的 p95/max 返回 E_NO_SAMPLE。
pub fn r10_empty_guard() -> Result<(), u16> {
    let h = PerfHarness::new();
    match h.p95() {
        Err(E_NO_SAMPLE) => {}
        _ => return Err(E_NO_SAMPLE),
    }
    match h.max_cost() {
        Err(E_NO_SAMPLE) => {}
        _ => return Err(E_NO_SAMPLE),
    }
    Ok(())
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_all_within_budget()?;
    hits += 1;
    r2_p95_max()?;
    hits += 1;
    r3_ledger()?;
    hits += 1;
    r4_big_payload()?;
    hits += 1;
    r5_heartbeat_lightest()?;
    hits += 1;
    r6_deterministic()?;
    hits += 1;
    r7_sort_sane()?;
    hits += 1;
    r8_p95_definition()?;
    hits += 1;
    r9_violation_caught()?;
    hits += 1;
    r10_empty_guard()?;
    hits += 1;
    Ok(hits)
}
