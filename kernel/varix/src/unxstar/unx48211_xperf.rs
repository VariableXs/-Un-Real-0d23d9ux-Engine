//! unx48211_xperf — UNX-F48211 Xwayland 性能面（XIM→preedit 段延迟采样，试产校准定）（诊断段 0xBB40）。
//!
//! 判据原文：
//!   J1 10^4 采样 P99 落账；判据锚定 附则五 延迟账。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 采样口径：XIM 事件进桥 → preedit 段可绘，全程延迟用「逻辑
//!      tick」度量（u32 单调 tick，无真实时钟依赖——no_std 无时钟，
//!      试产校准时由上层把 tick 换算为毫秒）。
//!   2. ×10^4 采样：万次端到端采样入环形账，环形账封顶 16K 格，
//!      满覆盖最旧（内存红线）。
//!   3. P99 落账：排序副本取第 99 百分位（下取整位序），P50/P99/最大
//!      三值落账；落账后指纹封存，复读必须一致。
//!   4. 延迟上界哨兵：单样本超 4096 tick 记违例账（不丢样本，照入
//!      分布，只加违例标记）；违例率入账。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec;      // vec! 宏（与 Vec 类型是两套名字——坑 2 三件套）
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBB40;

const E_EMPTY_SAMPLE: u16 = SEG_BASE + 1; // 无样本取分位
const E_CAP_BLOW: u16 = SEG_BASE + 2;     // 环形账越界
const E_SEAL_MISMATCH: u16 = SEG_BASE + 3; // 落账指纹不符
const E_BAD_TICK: u16 = SEG_BASE + 4;     // 采样对 (进,出) 逆序
const E_VIOL_LOST: u16 = SEG_BASE + 5;    // 违例账与样本账不平
const E_GUARD_UNBAL: u16 = SEG_BASE + 6;  // 采样账不平

pub const RING_CAP: usize = 16 * 1024;
pub const VIOL_TH: u32 = 4096; // 单样本违例阈值（tick）

/// 一次端到端延迟采样。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub t_in: u32,  // XIM 事件进桥 tick
    pub t_out: u32, // preedit 可绘 tick
}

impl Sample {
    pub fn latency(&self) -> Result<u32, u16> {
        if self.t_out < self.t_in {
            return Err(E_BAD_TICK);
        }
        Ok(self.t_out - self.t_in)
    }
}

/// 分位落账（封存后不可变）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LatReport {
    pub n: u32,
    pub p50: u32,
    pub p99: u32,
    pub max: u32,
    pub viol: u32, // 违例样本数
    pub seal: u32, // 指纹
}

/// 延迟采样总账。
#[derive(Debug)]
pub struct XPerf {
    ring: Vec<u32>,   // 延迟值环形账
    ring_len: usize,  // 逻辑长度（含覆盖计数）
    head: usize,      // 下一写入位
    viol: u32,
    report: Option<LatReport>,
    total_in: u32,
}

impl XPerf {
    pub fn new() -> Self {
        XPerf {
            ring: Vec::new(),
            ring_len: 0,
            head: 0,
            viol: 0,
            report: None,
            total_in: 0,
        }
    }

    /// 环形账初始化（一次性，容量 RING_CAP）。
    pub fn init(&mut self) -> Result<(), u16> {
        if self.ring.is_empty() {
            self.ring = vec![0u32; RING_CAP];
        }
        Ok(())
    }

    /// 采样入账：延迟入环，超阈值记违例。
    pub fn sample(&mut self, s: Sample) -> Result<u32, u16> {
        let lat = s.latency()?;
        self.total_in += 1;
        if lat > VIOL_TH {
            self.viol += 1;
        }
        if self.ring.is_empty() {
            // 未 init 的降级路径：直接线性账（仍有界：超 16K 拒收）。
            if self.ring_len >= RING_CAP {
                return Err(E_CAP_BLOW);
            }
            self.ring.push(lat);
            self.ring_len += 1;
            self.head = self.ring_len % RING_CAP;
            return Ok(lat);
        }
        let slot = self.head % RING_CAP;
        self.ring[slot] = lat;
        self.head = (self.head + 1) % RING_CAP;
        if self.ring_len < RING_CAP {
            self.ring_len += 1;
        }
        Ok(lat)
    }

    /// 当前账内样本快照（时序无关副本，供排序取分位）。
    pub fn snapshot(&self) -> Vec<u32> {
        let n = if self.ring_len < RING_CAP { self.ring_len } else { RING_CAP };
        let mut out: Vec<u32> = Vec::new();
        if self.ring.is_empty() {
            // 降级线性账
            for i in 0..self.ring_len.min(RING_CAP) {
                out.push(self.ring[i]);
            }
            return out;
        }
        if self.ring_len < RING_CAP {
            for i in 0..self.ring_len {
                out.push(self.ring[i]);
            }
        } else {
            // 满环：head 起按逻辑序读一整圈。
            for i in 0..RING_CAP {
                out.push(self.ring[(self.head + i) % RING_CAP]);
            }
        }
        let _ = n;
        out
    }

    /// 分位落账：P50/P99/最大 + 违例数 + 指纹封存。
    pub fn report(&mut self) -> Result<LatReport, u16> {
        let snap = self.snapshot();
        if snap.is_empty() {
            return Err(E_EMPTY_SAMPLE);
        }
        // 插入排序副本（n≤16K，确定性；不引外部分配器语义）。
        let mut v = snap;
        for i in 1..v.len() {
            let key = v[i];
            let mut j = i;
            while j > 0 && v[j - 1] > key {
                v[j] = v[j - 1];
                j -= 1;
            }
            v[j] = key;
        }
        let n = v.len() as u32;
        let pick = |p: u32| -> u32 {
            // 下取整位序：idx = floor(p*(n-1)/100)。
            let idx = ((p as u64) * ((n - 1) as u64) / 100) as usize;
            v[idx]
        };
        let p50 = pick(50);
        let p99 = pick(99);
        let max = v[v.len() - 1];
        let viol_now = self.viol;
        let seal = self.seal_fp(p50, p99, max, viol_now, n);
        let rep = LatReport {
            n,
            p50,
            p99,
            max,
            viol: viol_now,
            seal,
        };
        self.report = Some(rep);
        Ok(rep)
    }

    fn seal_fp(&self, p50: u32, p99: u32, max: u32, viol: u32, n: u32) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for x in [p50, p99, max, viol, n, self.total_in] {
            h ^= x;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    /// 落账复读一致性：报告存在时指纹必须可复算。
    pub fn verify(&self) -> Result<(), u16> {
        match self.report {
            Some(r) => {
                let s = self.seal_fp(r.p50, r.p99, r.max, r.viol, r.n);
                if s != r.seal {
                    return Err(E_SEAL_MISMATCH);
                }
                if r.n == 0 {
                    return Err(E_EMPTY_SAMPLE);
                }
                Ok(())
            }
            None => Err(E_EMPTY_SAMPLE),
        }
    }

    /// 违例账平衡：违例数 ≤ 总样本数。
    pub fn audit(&self) -> Result<(), u16> {
        if self.viol > self.total_in {
            return Err(E_VIOL_LOST);
        }
        if self.ring_len > RING_CAP {
            return Err(E_CAP_BLOW);
        }
        Ok(())
    }

    pub fn total_in(&self) -> u32 {
        self.total_in
    }
    pub fn viol(&self) -> u32 {
        self.viol
    }
}

/// 自例程判据：落实 J1「10^4 采样 P99 落账」全链。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 场景：×10^4 采样 —— 确定性延迟序列（LCG 混合固定模式），
    //        全部入账后 P99 落账、指纹封存、复读一致。
    let mut p = XPerf::new();
    p.init()?;
    let mut seed: u32 = 0x005E_ED01;
    let mut max_seen = 0u32;
    for i in 0..10_000u32 {
        // 确定性混合：主体 1~90 tick + 1% 长尾 200~600 tick。
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let base = (seed >> 9) % 90;
        let lat = if i % 100 == 0 { 200 + (seed >> 11) % 400 } else { 1 + base };
        if lat > max_seen {
            max_seen = lat;
        }
        let s = Sample {
            t_in: i * 10,
            t_out: i * 10 + lat,
        };
        p.sample(s)?;
    }
    p.audit()?;
    let rep = p.report()?;
    if rep.n != RING_CAP as u32 {
        return Err(E_CAP_BLOW); // 万样本 ≤ 16K 容量：全保留
    }
    if rep.max != max_seen {
        return Err(E_GUARD_UNBAL);
    }
    if rep.p50 == 0 || rep.p99 < rep.p50 || rep.max < rep.p99 {
        return Err(E_GUARD_UNBAL); // 分位单调性
    }
    p.verify()?;
    hits += 1;

    // R2 场景：P99 位序核对 —— 100 个已知样本（1..=100），
    //        P99 = floor(99*99/100)+1 = 99（下取整位序自洽）。
    let mut p2 = XPerf::new();
    p2.init()?;
    for i in 1..=100u32 {
        p2.sample(Sample { t_in: 0, t_out: i })?;
    }
    let r2 = p2.report()?;
    if r2.n != 100 || r2.p50 != 50 || r2.p99 != 99 || r2.max != 100 {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    // R3 场景：违例哨兵 —— 注入 3 个超阈样本，违例账=3 且样本照入分布。
    let mut p3 = XPerf::new();
    p3.init()?;
    for i in 1..=50u32 {
        p3.sample(Sample { t_in: 0, t_out: i })?;
    }
    for i in 1..=3u32 {
        p3.sample(Sample { t_in: 0, t_out: VIOL_TH + i })?;
    }
    let r3 = p3.report()?;
    if p3.viol() != 3 || r3.viol != 3 {
        return Err(E_VIOL_LOST);
    }
    if r3.max <= VIOL_TH {
        return Err(E_VIOL_LOST); // 违例样本必须在分布里（不丢）
    }
    p3.audit()?;
    hits += 1;

    // R4 场景：逆序采样对拒收 —— t_out < t_in 显性报错不折算。
    let mut p4 = XPerf::new();
    match p4.sample(Sample { t_in: 100, t_out: 99 }) {
        Err(E_BAD_TICK) => {}
        _ => return Err(E_BAD_TICK),
    }
    if p4.total_in() != 0 {
        return Err(E_GUARD_UNBAL); // 拒收样本不计入
    }
    hits += 1;

    // R5 场景：空账取分位拒收。
    let mut p5 = XPerf::new();
    match p5.report() {
        Err(E_EMPTY_SAMPLE) => {}
        _ => return Err(E_EMPTY_SAMPLE),
    }
    match p5.verify() {
        Err(E_EMPTY_SAMPLE) => {}
        _ => return Err(E_EMPTY_SAMPLE),
    }
    hits += 1;

    // R6 场景：环形覆盖 —— 容量 16K，灌 20K 样本，账内应为最近 16K，
    //        最早 4K 被覆盖（逻辑长度封顶）。
    let mut p6 = XPerf::new();
    p6.init()?;
    for i in 0..20_000u32 {
        p6.sample(Sample { t_in: i, t_out: i + 5 })?; // 全部延迟 5
    }
    p6.audit()?;
    let r6 = p6.report()?;
    if r6.n != RING_CAP as u32 || r6.p99 != 5 || r6.max != 5 {
        return Err(E_CAP_BLOW);
    }
    hits += 1;

    // R7 场景：落账后指纹复读 —— verify 多次调用结果一致。
    for _ in 0..3 {
        p6.verify()?;
    }
    hits += 1;

    // R8 场景：降级线性账（未 init）小样本可跑，超 16K 拒收。
    let mut p8 = XPerf::new();
    for i in 0..100u32 {
        p8.sample(Sample { t_in: 0, t_out: 2 })?;
    }
    let r8 = p8.report()?;
    if r8.n != 100 || r8.p99 != 2 {
        return Err(E_GUARD_UNBAL);
    }
    let mut p9 = XPerf::new();
    for _ in 0..RING_CAP {
        p9.sample(Sample { t_in: 0, t_out: 1 })?;
    }
    match p9.sample(Sample { t_in: 0, t_out: 2 }) {
        Err(E_CAP_BLOW) => {}
        _ => return Err(E_CAP_BLOW),
    }
    hits += 1;

    // R9 场景：分位单调性全谱 —— 单调序列 1..=1000 的 P50 ≤ P99 ≤ max 恒成立。
    let mut pa = XPerf::new();
    pa.init()?;
    for i in 1..=1000u32 {
        pa.sample(Sample { t_in: 0, t_out: i })?;
    }
    let ra = pa.report()?;
    if !(ra.p50 <= ra.p99 && ra.p99 <= ra.max) {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    // R10 场景：确定性 —— 同延迟序列两次全跑，报告逐字段相等。
    let run_once = |seed: u32| -> Result<LatReport, u16> {
        let mut px = XPerf::new();
        px.init()?;
        let mut s2 = seed;
        for i in 0..5_000u32 {
            s2 = s2.wrapping_mul(1103515245).wrapping_add(12345);
            let lat = 1 + (s2 >> 9) % 120;
            px.sample(Sample { t_in: i, t_out: i + lat })?;
        }
        px.report()
    };
    let x = run_once(0x00C0_FEE0)?;
    let y = run_once(0x00C0_FEE0)?;
    if x != y {
        return Err(E_SEAL_MISMATCH);
    }
    hits += 1;

    Ok(hits)
}
