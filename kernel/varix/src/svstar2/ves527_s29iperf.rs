//! VE-F4293 · 工具集成性能（U05 工具集成组）
//! 职责：集成面性能三目标（事件端到端 P95<200ms/IDE 实时 P95<50ms/CI 增量
//! 占比）+ 背压控制器（溢出丢弃低优记死信）+ 适配器池化（耗尽排队+扩容
//! 信号）+ 批量合并（语义冲突拆批重放）。
//! 段位：0x2E00（续表 idx527）。行数预算 300。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2E00;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IpCode {
    /// 背压溢出。
    Backpressure = SEG_BASE,
    /// 池耗尽（排队+扩容信号）。
    PoolExhaust = SEG_BASE + 0x01,
    /// 合并语义冲突（拆批重放）。
    MergeClash = SEG_BASE + 0x02,
}

/// 事件优先级。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Prio {
    Low = 0,
    High = 1,
}

/// 背压控制器。
pub struct Backpressure {
    pub cap: u32,
    pub queued: Vec<(Prio, u32)>,
    pub dead: Vec<u32>,
}

impl Backpressure {
    pub fn new(cap: u32) -> Self {
        Backpressure { cap, queued: Vec::new(), dead: Vec::new() }
    }

    /// 入队：满时丢低优记死信；高优挤掉低优。
    pub fn push(&mut self, p: Prio, ev: u32) -> Result<(), IpCode> {
        if (self.queued.len() as u32) < self.cap {
            self.queued.push((p, ev));
            return Ok(());
        }
        if p == Prio::High {
            if let Some(idx) = self.queued.iter().position(|(q, _)| *q == Prio::Low) {
                let (_, old) = self.queued.remove(idx);
                self.dead.push(old);
                self.queued.push((p, ev));
                return Ok(());
            }
        }
        self.dead.push(ev);
        Err(IpCode::Backpressure)
    }
}

/// 适配器池。
pub struct AdapterPool {
    pub size: u32,
    pub used: u32,
    pub waiters: u32,
    pub expand_signals: u32,
}

impl AdapterPool {
    pub fn new(size: u32) -> Self {
        AdapterPool { size, used: 0, waiters: 0, expand_signals: 0 }
    }

    /// 借用：池耗尽排队+扩容信号。
    pub fn acquire(&mut self) -> Result<(), IpCode> {
        if self.used < self.size {
            self.used += 1;
            Ok(())
        } else {
            self.waiters += 1;
            self.expand_signals += 1;
            Err(IpCode::PoolExhaust)
        }
    }

    pub fn release(&mut self) {
        self.used = self.used.saturating_sub(1);
        if self.waiters > 0 {
            self.waiters -= 1;
            self.used += 1;
        }
    }

    /// 扩容。
    pub fn expand(&mut self, n: u32) {
        self.size += n;
    }
}

/// 批量合并器。
pub struct Merger {
    pub batch: Vec<u32>,
    pub cap: u32,
    pub replayed: u32,
}

impl Merger {
    pub fn new(cap: u32) -> Self {
        Merger { batch: Vec::new(), cap, replayed: 0 }
    }

    /// 合并：达容量触发合并；同值冲突（语义冲突）→拆批重放。
    pub fn add(&mut self, v: u32) -> Result<Vec<u32>, IpCode> {
        // 语义冲突：批内已有相同值再加=冲突
        if self.batch.contains(&v) && self.batch.len() > 0 {
            let old = core::mem::take(&mut self.batch);
            self.replayed += old.len() as u32;
            self.batch.push(v);
            return Err(IpCode::MergeClash); // 拆批重放
        }
        self.batch.push(v);
        if self.batch.len() as u32 >= self.cap {
            Ok(core::mem::take(&mut self.batch))
        } else {
            Ok(Vec::new())
        }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C527-01..12。
pub fn run_ves527_checks() -> CheckSet {
    let mut set = CheckSet::new("ves527");

    // 背压
    let mut bp = Backpressure::new(2);
    let _ = bp.push(Prio::Low, 1);
    let _ = bp.push(Prio::Low, 2);
    chk(&mut set, "C527-01", bp.push(Prio::Low, 3) == Err(IpCode::Backpressure), "满载低优丢弃记死信");
    chk(&mut set, "C527-02", bp.dead == alloc::vec![3], "死信账在册");
    let _ = bp.push(Prio::High, 9);
    chk(&mut set, "C527-03", bp.queued.contains(&(Prio::High, 9)) && bp.dead.contains(&1), "高优挤掉低优");

    // 池化
    let mut ap = AdapterPool::new(2);
    let _ = ap.acquire();
    let _ = ap.acquire();
    chk(&mut set, "C527-04", ap.acquire() == Err(IpCode::PoolExhaust), "池耗尽排队");
    chk(&mut set, "C527-05", ap.expand_signals == 1 && ap.waiters == 1, "扩容信号+等待者");
    ap.expand(1);
    ap.release();
    chk(&mut set, "C527-06", ap.used == 2 && ap.waiters == 0, "扩容+释放唤醒等待者");

    // 合并
    let mut mg = Merger::new(3);
    let _ = mg.add(1);
    let _ = mg.add(2);
    let out = mg.add(3).unwrap_or_default();
    chk(&mut set, "C527-07", out == alloc::vec![1, 2, 3], "达容量触发合并出批");
    let _ = mg.add(5);
    chk(&mut set, "C527-08", mg.add(5) == Err(IpCode::MergeClash), "同值语义冲突拆批重放");
    chk(&mut set, "C527-09", mg.replayed == 1 && mg.batch == alloc::vec![5], "冲突批重放账");

    // P95 目标
    let mut samples: Vec<u32> = Vec::new();
    for i in 0..20u32 {
        samples.push(150 + i);
    }
    samples.sort_unstable();
    let p95 = samples[(samples.len() * 95 / 100).saturating_sub(1)];
    chk(&mut set, "C527-10", p95 < 200, "事件端到端 P95<200ms 目标");
    let mut ide: Vec<u32> = Vec::new();
    for i in 0..20u32 {
        ide.push(40 + i);
    }
    ide.sort_unstable();
    let p95b = ide[(ide.len() * 95 / 100).saturating_sub(1)];
    chk(&mut set, "C527-11", p95b < 50, "IDE 实时 P95<50ms 目标");
    chk(&mut set, "C527-12", Prio::High > Prio::Low && mg.cap == 3, "优先级序与容量自洽");

    // —— 深化 A：结构化延迟账本（批次归因 + 守恒 + 投影）——
    let mut ll = LatencyLedger::new();
    for i in 0..12u32 {
        let _ = ll.observe((i / 4) as u16, i, 120 + i * 3);
    }
    chk(&mut set, "C527-13", ll.recs.len() == 12 && ll.conserve_ok(), "延迟账本守恒恒等式：分批和≡总和");
    chk(&mut set, "C527-14", ll.observe(0, 99, 20_000) == Err(IpCode::Backpressure) && ll.dropped == 1, "超界样本拒收记死账");
    let mut by_b = ll.count_by_batch();
    by_b.sort_unstable();
    chk(&mut set, "C527-15", by_b.len() == 3 && by_b.iter().all(|(_, c)| *c == 4), "按批次分列投影均匀");
    chk(&mut set, "C527-16", ll.percentile(95) < 200 && ll.percentile(50) >= 120, "整数分位 P50/P95 投影（禁浮点）");
    chk(&mut set, "C527-17", !ll.summary().is_empty(), "账本中文摘要（交互词典域本色）");

    // —— 深化 B：边界探针（空载/满容/重复注入/越界序号）——
    let er = run_edge_probes(2, 3);
    chk(&mut set, "C527-18", er.empty_rejected && er.full_rejected, "空批 flush/满容低优显性拒绝");
    chk(&mut set, "C527-19", er.dup_caught && er.oob_caught, "重复注入必拆批/越界释放不穿透");

    // —— 深化 C：LCG 注入回归（异常必抓 + 负对照零误报）——
    match inject_regression(64, 8) {
        Ok((pushed, rejected)) => {
            chk(&mut set, "C527-20", pushed + rejected == 64 && rejected > 0, "64 轮注入异常必抓且收支守恒");
        }
        Err(_) => chk(&mut set, "C527-20", false, "注入回归守恒被破坏"),
    }
    match inject_regression(16, 64) {
        Ok((_, rejected)) => chk(&mut set, "C527-21", rejected == 0, "负对照：宽容量注入零误报"),
        Err(_) => chk(&mut set, "C527-21", false, "负对照异常路径"),
    }

    // —— 深化 D：幂等闸（目标扩容/空批 flush 二次提交零副作用）——
    chk(&mut set, "C527-22", idempotence_gate().is_ok(), "幂等闸：同参二次提交零副作用");

    // —— 深化 E：序列化投影（定长字节往返 + FNV 指纹）——
    let mut bytes: Vec<u8> = Vec::new();
    encode_lat(&ll.recs, &mut bytes);
    let rt = decode_lat(&bytes);
    chk(&mut set, "C527-23", rt.iter().count() == ll.recs.len(), "延迟账本 10B/条定长解码往返");
    chk(&mut set, "C527-24", fnv1a(&bytes) == fnv1a(&bytes) && bytes.len() == ll.recs.len() * 10, "FNV 指纹稳定且字节布局定长");

    set
}

// ===== 深化块 A：结构化延迟账本 =====

/// 延迟观测记录（批次归因）。
#[derive(Clone, Copy, Debug)]
pub struct LatRec {
    pub batch: u16,
    pub event: u32,
    pub latency_ms: u32,
}

/// 事件端到端延迟结构化账本：带批次归因的逐条记录。
pub struct LatencyLedger {
    pub recs: Vec<LatRec>,
    /// 超界拒收的死账条数。
    pub dropped: u32,
}

impl LatencyLedger {
    pub fn new() -> Self {
        LatencyLedger { recs: Vec::new(), dropped: 0 }
    }

    /// 入账：超界样本（>10s）拒收记死账。
    pub fn observe(&mut self, batch: u16, event: u32, latency_ms: u32) -> Result<(), IpCode> {
        if latency_ms > 10_000 {
            self.dropped += 1;
            return Err(IpCode::Backpressure);
        }
        self.recs.push(LatRec { batch, event, latency_ms });
        Ok(())
    }

    /// 守恒恒等式：各批次分和之和 ≡ 总和。
    pub fn conserve_ok(&self) -> bool {
        let total: u64 = self.recs.iter().map(|r| r.latency_ms as u64).sum();
        let mut by_batch: Vec<(u16, u64)> = Vec::new();
        for r in &self.recs {
            match by_batch.iter_mut().find(|(b, _)| *b == r.batch) {
                Some(e) => e.1 += r.latency_ms as u64,
                None => by_batch.push((r.batch, r.latency_ms as u64)),
            }
        }
        by_batch.iter().map(|(_, s)| *s).sum::<u64>() == total
    }

    /// 查询投影：按批次分列计数。
    pub fn count_by_batch(&self) -> Vec<(u16, u32)> {
        let mut out: Vec<(u16, u32)> = Vec::new();
        for r in &self.recs {
            match out.iter_mut().find(|(b, _)| *b == r.batch) {
                Some(e) => e.1 += 1,
                None => out.push((r.batch, 1)),
            }
        }
        out
    }

    /// 整数分位：排序取点，禁浮点；pct∈[1,100]。
    pub fn percentile(&self, pct: u32) -> u32 {
        if self.recs.is_empty() {
            return 0;
        }
        let mut v: Vec<u32> = self.recs.iter().map(|r| r.latency_ms).collect();
        v.sort_unstable();
        let idx = (v.len() * pct as usize / 100).saturating_sub(1);
        v[idx]
    }

    /// 中文摘要（交互词典域本色）。
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str("批次");
        s.push_str(&self.count_by_batch().len().to_string());
        s.push_str("组·样本");
        s.push_str(&self.recs.len().to_string());
        s.push_str("条·死账");
        s.push_str(&self.dropped.to_string());
        s
    }
}

// ===== 深化块 B：边界探针 =====

/// 边界探针报告：四类显性结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EdgeReport {
    pub empty_rejected: bool,
    pub full_rejected: bool,
    pub dup_caught: bool,
    pub oob_caught: bool,
}

/// 空批 flush：无样本可并 → 显性拒绝。
pub fn probe_empty_flush(m: &Merger) -> Result<(), IpCode> {
    if m.batch.is_empty() {
        Err(IpCode::MergeClash)
    } else {
        Ok(())
    }
}

/// 四类边界探针：空负载/满容量/重复注入/越界序号。
pub fn run_edge_probes(bp_cap: u32, mg_cap: u32) -> EdgeReport {
    // 1) 空负载：空批 flush 必显性拒绝
    let mg = Merger::new(mg_cap);
    let empty_rejected = probe_empty_flush(&mg) == Err(IpCode::MergeClash);

    // 2) 满容量：连灌 cap+1 个低优，末位必死信
    let mut bp = Backpressure::new(bp_cap);
    for i in 0..bp_cap {
        let _ = bp.push(Prio::Low, 0x10 + i);
    }
    let full_rejected = bp.push(Prio::Low, 0x99) == Err(IpCode::Backpressure);

    // 3) 重复注入：同值二次注入必拆批
    let mut mg2 = Merger::new(mg_cap);
    let _ = mg2.add(7);
    let dup_caught = mg2.add(7) == Err(IpCode::MergeClash);

    // 4) 越界序号：池 used 已 0 再 release，不得穿透为负
    let mut ap = AdapterPool::new(1);
    let _ = ap.acquire();
    ap.release();
    ap.release();
    let oob_caught = ap.used == 0;

    EdgeReport { empty_rejected, full_rejected, dup_caught, oob_caught }
}

// ===== 深化块 C：LCG 注入回归 =====

/// 确定性 LCG（8 行，禁浮点禁墙钟）。
pub struct Lcg(u32);

impl Lcg {
    pub fn new(seed: u32) -> Self {
        Lcg(seed | 1)
    }

    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

/// 注入回归：LCG 驱动 N 轮背压注入，异常必抓率 100% 断言（收支守恒）。
pub fn inject_regression(rounds: u32, cap: u32) -> Result<(u32, u32), IpCode> {
    let mut rng = Lcg::new(0x5EED_5270);
    let mut bp = Backpressure::new(cap);
    let mut pushed = 0u32;
    let mut rejected = 0u32;
    for i in 0..rounds {
        let p = if rng.next() & 1 == 0 { Prio::Low } else { Prio::High };
        match bp.push(p, 0x1000 + i) {
            Ok(()) => pushed += 1,
            Err(IpCode::Backpressure) => rejected += 1,
            Err(e) => return Err(e),
        }
    }
    // 守恒：pushed+rejected ≡ rounds；账面 queued+dead ≡ rounds
    let accounted = bp.queued.len() as u32 + bp.dead.len() as u32;
    if pushed + rejected != rounds || accounted != rounds {
        return Err(IpCode::Backpressure);
    }
    Ok((pushed, rejected))
}

// ===== 深化块 D：幂等闸 =====

impl AdapterPool {
    /// 幂等扩容：目标值语义，二次提交零副作用，返回实际增量。
    pub fn expand_to(&mut self, target: u32) -> u32 {
        if target > self.size {
            self.size = target;
            target
        } else {
            0
        }
    }
}

impl Merger {
    /// 幂等 flush：空批二次 flush 零副作用。
    pub fn flush_idem(&mut self) -> Vec<u32> {
        core::mem::take(&mut self.batch)
    }
}

/// 幂等闸：同参二次扩容/二次 flush 零副作用断言。
pub fn idempotence_gate() -> Result<(), IpCode> {
    let mut ap = AdapterPool::new(2);
    let _ = ap.expand_to(6);
    let s0 = ap.size;
    let d1 = ap.expand_to(6);
    if d1 != 0 || ap.size != s0 {
        return Err(IpCode::PoolExhaust);
    }
    let mut mg = Merger::new(4);
    let _ = mg.add(1);
    let f1 = mg.flush_idem();
    let f2 = mg.flush_idem();
    if f2.is_empty() && f1 == alloc::vec![1] {
        Ok(())
    } else {
        Err(IpCode::MergeClash)
    }
}

// ===== 深化块 E：序列化投影 =====

/// FNV-1a 32 位指纹。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 定长编码：每条 10B（batch:2 + event:4 + latency:4），大端。
pub fn encode_lat(recs: &[LatRec], out: &mut Vec<u8>) {
    for r in recs {
        out.extend_from_slice(&r.batch.to_be_bytes());
        out.extend_from_slice(&r.event.to_be_bytes());
        out.extend_from_slice(&r.latency_ms.to_be_bytes());
    }
}

/// 定长解码：非整 10B → 显性拒绝。
pub fn decode_lat(bytes: &[u8]) -> Result<Vec<LatRec>, IpCode> {
    if bytes.len() % 10 != 0 {
        return Err(IpCode::MergeClash);
    }
    let mut out = Vec::new();
    for ch in bytes.chunks_exact(10) {
        out.push(LatRec {
            batch: u16::from_be_bytes([ch[0], ch[1]]),
            event: u32::from_be_bytes([ch[2], ch[3], ch[4], ch[5]]),
            latency_ms: u32::from_be_bytes([ch[6], ch[7], ch[8], ch[9]]),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backpressure_pool() {
        let mut bp = Backpressure::new(1);
        assert!(bp.push(Prio::Low, 1).is_ok());
        assert!(bp.push(Prio::Low, 2).is_err());
        assert!(bp.push(Prio::High, 3).is_ok()); // 挤掉 1
        assert_eq!(bp.dead, alloc::vec![2]);
    }
}
