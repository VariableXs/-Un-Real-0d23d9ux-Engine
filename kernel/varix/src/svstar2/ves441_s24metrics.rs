//! ves441_s24metrics — VE-F4207 · 一致性度量体系
//! 度量：一致率（合规项/总检项）/冲突密度/修复时效/回归率四主指标×域维度切分；
//! 指标口径冻结+计算可复算；与家族度量同构对拍；无障碍一致率单列（域本色主场指标）；数据合成。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1400;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MtCode {
    Ok = 0,
    CaliberDrift = SEG_BASE + 0x01, // 0x1401
    SourceBroken = SEG_BASE + 0x02, // 0x1402 数据源断
    RecalcMismatch = SEG_BASE + 0x03, // 0x1403
}

/// 四主指标。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Metric { Consistency = 0, ConflictDensity = 1, FixLatency = 2, RegressRate = 3 }

impl Metric {
    pub const ALL: [Metric; 4] = [
        Metric::Consistency, Metric::ConflictDensity, Metric::FixLatency, Metric::RegressRate];
    pub fn name(self) -> &'static str {
        match self {
            Metric::Consistency => "consistency", Metric::ConflictDensity => "conflict-density",
            Metric::FixLatency => "fix-latency", Metric::RegressRate => "regress-rate",
        }
    }
}

/// 域维度切分条目（合规项/总检项 + a11y 单列——域本色）。
pub struct DimRow { pub domain: &'static str, pub pass: u32, pub total: u32, pub a11y_pass: u32, pub a11y_total: u32 }

/// 度量册。
pub struct MetricBook {
    pub rows: Vec<DimRow>,
    pub frozen: bool,
    pub calibers: Vec<(Metric, u32)>, // 口径版本（冻结须一致）
}

impl MetricBook {
    pub fn new() -> Self { MetricBook { rows: Vec::new(), frozen: false, calibers: Vec::new() } }
    pub fn put_row(&mut self, domain: &'static str, pass: u32, total: u32, ap: u32, at: u32) {
        self.rows.retain(|r| r.domain != domain);
        self.rows.push(DimRow { domain, pass, total, a11y_pass: ap, a11y_total: at });
    }
    pub fn set_caliber(&mut self, m: Metric, ver: u32) {
        self.calibers.retain(|(mm, _)| *mm != m);
        self.calibers.push((m, ver));
    }
    /// 一致率（bp）：某域或全量。
    pub fn consistency_bp(&self, domain: Option<&str>) -> Option<u32> {
        let (p, t) = match domain {
            Some(d) => { let r = self.rows.iter().find(|r| r.domain == d)?; (r.pass, r.total) }
            None => (self.rows.iter().map(|r| r.pass).sum(), self.rows.iter().map(|r| r.total).sum()),
        };
        if t == 0 { return None; } // 数据源断→显性空态
        Some(p * 10_000 / t)
    }
    /// 无障碍一致率单列（域本色）。
    pub fn a11y_consistency_bp(&self) -> Option<u32> {
        let p: u32 = self.rows.iter().map(|r| r.a11y_pass).sum();
        let t: u32 = self.rows.iter().map(|r| r.a11y_total).sum();
        if t == 0 { return None; }
        Some(p * 10_000 / t)
    }
    /// 口径冻结：四主指标齐且版本一致。
    pub fn freeze(&mut self, ver: u32) -> Result<(), MtCode> {
        for m in Metric::ALL {
            match self.calibers.iter().find(|(mm, _)| *mm == m) {
                None => return Err(MtCode::CaliberDrift),
                Some((_, v)) => if *v != ver { return Err(MtCode::CaliberDrift); },
            }
        }
        self.frozen = true;
        Ok(())
    }
    /// 复算：一致率可由同数据二次计算复现（可复算断言）。
    pub fn recalc(&self) -> Result<bool, MtCode> {
        let a = self.consistency_bp(None).ok_or(MtCode::SourceBroken)?;
        let p: u64 = self.rows.iter().map(|r| r.pass as u64).sum();
        let t: u64 = self.rows.iter().map(|r| r.total as u64).sum();
        if t == 0 { return Err(MtCode::SourceBroken); }
        Ok(a as u64 == p * 10_000 / t)
    }
}

// ───────────── 深化块 D441-A · 采样批账本（批次×域×指标） ─────────────

/// 一条度量采样入账：批次 + 指标 + 原始值（bp 或 ms）+ 域序号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SampleRow {
    pub batch: u16,
    pub metric: Metric,
    pub raw: u32,
    pub domain_idx: u8,
}

/// 度量采样账本：定容量 + 幂等闸（同批同指标同域去重）+ 守恒 + 分位。
pub struct SampleLedger {
    pub recs: Vec<SampleRow>,
    pub cap: usize,
}

impl SampleLedger {
    pub fn new(cap: usize) -> Self { SampleLedger { recs: Vec::new(), cap } }
    /// 入账：满→显性 Err；同键重复→幂等 false。
    pub fn push(&mut self, r: SampleRow) -> Result<bool, MtCode> {
        if self.recs.len() >= self.cap { return Err(MtCode::SourceBroken); }
        if self.recs.iter().any(|x| x.batch == r.batch && x.metric == r.metric && x.domain_idx == r.domain_idx) {
            return Ok(false);
        }
        self.recs.push(r);
        Ok(true)
    }
    /// 守恒恒等式：∑raw ≡ ∑(按四指标分列求和)。
    pub fn conserves(&self) -> bool {
        let total: u64 = self.recs.iter().map(|r| r.raw as u64).sum();
        let mut by_metric: u64 = 0;
        for m in Metric::ALL {
            by_metric += self.recs.iter().filter(|r| r.metric == m)
                .map(|r| r.raw as u64).sum::<u64>();
        }
        total == by_metric
    }
    /// 指标值分位（整数排序取点，禁浮点）。
    pub fn metric_quantile(&self, m: Metric, q: u32) -> u32 {
        let mut vals: Vec<u32> = self.recs.iter().filter(|r| r.metric == m)
            .map(|r| r.raw).collect();
        if vals.is_empty() { return 0; }
        vals.sort_unstable();
        let n = vals.len();
        let idx = ((q as usize).max(1) * n + 99) / 100;
        vals[idx.min(n) - 1]
    }
    /// 指标均值（整数，禁浮点）。
    pub fn metric_mean(&self, m: Metric) -> u32 {
        let vals: Vec<u32> = self.recs.iter().filter(|r| r.metric == m)
            .map(|r| r.raw).collect();
        if vals.is_empty() { return 0; }
        let sum: u64 = vals.iter().map(|v| *v as u64).sum();
        (sum / vals.len() as u64) as u32
    }
    /// 指标覆盖投影：四主指标中已采样指标数。
    pub fn metric_coverage(&self) -> usize {
        let mut ms: Vec<u8> = self.recs.iter().map(|r| r.metric as u8).collect();
        ms.sort_unstable();
        ms.dedup();
        ms.len()
    }
    /// 批次去重计数。
    pub fn batch_count(&self) -> usize {
        let mut bs: Vec<u16> = self.recs.iter().map(|r| r.batch).collect();
        bs.sort_unstable();
        bs.dedup();
        bs.len()
    }
}

// ───────────── 深化块 D441-B · 边界探针 ─────────────

/// 探针 P1：越界分位号/空指标→显性 Err。
pub fn probe_metric_quantile(lg: &SampleLedger, m: Metric, q: u32) -> Result<u32, MtCode> {
    if q == 0 || q > 100 { return Err(MtCode::CaliberDrift); }
    if lg.recs.iter().filter(|r| r.metric == m).count() == 0 { return Err(MtCode::SourceBroken); }
    Ok(lg.metric_quantile(m, q))
}

/// 探针 P2：total==0 域行不可入册（除零防线，显性拒绝）。
pub fn probe_zero_total(total: u32) -> Result<(), MtCode> {
    if total == 0 { return Err(MtCode::SourceBroken); }
    Ok(())
}

// ───────────── 深化块 D441-C · LCG 注入回归 ─────────────

/// 8 行确定性 LCG。
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    fn pick(&mut self, n: u32) -> u32 { ((self.next() >> 33) as u32) % n }
}

/// 注入回归：N 轮随机指标缺一注入（四指标缺一），freeze 必抓
/// （CaliberDrift 100%）；负对照：四指标全冻零误报。返回 (轮数, 抓获, 误报)。
pub fn lcg_caliber_gap_regression(rounds: usize) -> (usize, usize, usize) {
    let mut g = Lcg(0x41_41_41_41);
    let mut caught = 0usize;
    let mut false_pos = 0usize;
    for _ in 0..rounds {
        let drop = g.pick(4) as usize;
        let mut bk = MetricBook::new();
        for (i, m) in Metric::ALL.iter().enumerate() {
            if i != drop { bk.set_caliber(*m, 3); }
        }
        if bk.freeze(3) == Err(MtCode::CaliberDrift) { caught += 1; } else { false_pos += 1; }
        // 负对照：四指标全冻零误报。
        let mut bk_ok = MetricBook::new();
        for m in Metric::ALL { bk_ok.set_caliber(m, 3); }
        if bk_ok.freeze(3).is_err() { false_pos += 1; }
    }
    (rounds, caught, false_pos)
}

// ───────────── 深化块 D441-D · 序列化投影 + FNV 指纹 ─────────────

/// SampleRow 定长 9 字节：batch(2,LE)+metric(1)+domain(1)+raw(4,LE)+保留(1)。
pub fn encode_sample(r: &SampleRow) -> Vec<u8> {
    let mut out = Vec::with_capacity(9);
    out.extend_from_slice(&r.batch.to_le_bytes());
    out.push(r.metric as u8);
    out.push(r.domain_idx);
    out.extend_from_slice(&r.raw.to_le_bytes());
    out.push(0);
    out
}

/// 解码：错长/坏指标号→显性 Err。
pub fn decode_sample(b: &[u8]) -> Result<SampleRow, MtCode> {
    if b.len() != 9 { return Err(MtCode::SourceBroken); }
    let metric = match Metric::ALL.iter().find(|m| **m as u8 == b[2]) {
        Some(m) => *m,
        None => return Err(MtCode::CaliberDrift),
    };
    let mut batch = [0u8; 2];
    batch.copy_from_slice(&b[0..2]);
    let mut raw = [0u8; 4];
    raw.copy_from_slice(&b[4..8]);
    Ok(SampleRow { batch: u16::from_le_bytes(batch), metric, domain_idx: b[3], raw: u32::from_le_bytes(raw) })
}

/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for x in bytes { h ^= *x as u32; h = h.wrapping_mul(0x0100_0193); }
    h
}

/// 度量册快照指纹：域行数据+冻结位+口径版本序列化后 FNV。
pub fn metricbook_fingerprint(bk: &MetricBook) -> u32 {
    let mut bytes = Vec::new();
    for r in &bk.rows {
        bytes.extend_from_slice(r.domain.as_bytes());
        bytes.extend_from_slice(&r.pass.to_le_bytes());
        bytes.extend_from_slice(&r.total.to_le_bytes());
        bytes.extend_from_slice(&r.a11y_pass.to_le_bytes());
        bytes.push(bk.frozen as u8);
    }
    for (m, v) in &bk.calibers {
        bytes.push(*m as u8);
        bytes.push(*v as u8);
    }
    fnv1a(&bytes)
}

/// 指纹十六进制投影。
pub fn fingerprint_hex(h: u32) -> String {
    let mut out = String::new();
    for i in 0..8 {
        let nib = ((h >> (28 - i * 4)) & 0xF) as u8;
        out.push(if nib < 10 { (b'0' + nib) as char } else { (b'a' + nib - 10) as char });
    }
    out
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves441_checks() -> CheckSet {
    let mut set = CheckSet::new("ves441");
    let mut bk = MetricBook::new();

    // C441-01 数据源断（空册）→显性空态 None。
    chk(&mut set, "C441-01", bk.consistency_bp(None).is_none(), "空态显性");

    // C441-02 一致率计算（数据合成）。
    bk.put_row("ui", 950, 1000, 180, 200);
    bk.put_row("cli", 470, 500, 90, 100);
    chk(&mut set, "C441-02", bk.consistency_bp(None) == Some(9466), "一致率 bp");

    // C441-03 域维度切分单查。
    chk(&mut set, "C441-03", bk.consistency_bp(Some("ui")) == Some(9500)
        && bk.consistency_bp(Some("nope")).is_none(), "域切分");

    // C441-04 无障碍一致率单列（域本色）。
    chk(&mut set, "C441-04", bk.a11y_consistency_bp() == Some(9000), "a11y 单列");

    // C441-05 口径缺指标→冻结拒。
    bk.set_caliber(Metric::Consistency, 3);
    chk(&mut set, "C441-05", bk.freeze(3) == Err(MtCode::CaliberDrift), "口径缺拒冻");

    // C441-06 口径版本漂移拒。
    for m in Metric::ALL { bk.set_caliber(m, 3); }
    bk.set_caliber(Metric::FixLatency, 2);
    chk(&mut set, "C441-06", bk.freeze(3) == Err(MtCode::CaliberDrift), "版本漂移拒");

    // C441-07 对版后冻结通过。
    bk.set_caliber(Metric::FixLatency, 3);
    chk(&mut set, "C441-07", bk.freeze(3).is_ok() && bk.frozen, "冻结通过");

    // C441-08 可复算断言。
    chk(&mut set, "C441-08", bk.recalc() == Ok(true), "可复算");

    // C441-09 家族同构对拍：四主指标名与家族一致。
    let fam = ["consistency", "conflict-density", "fix-latency", "regress-rate"];
    chk(&mut set, "C441-09", Metric::ALL.iter().all(|m| fam.contains(&m.name())), "家族同构");

    // C441-10 域行覆盖幂等。
    bk.put_row("ui", 960, 1000, 190, 200);
    chk(&mut set, "C441-10", bk.rows.len() == 2, "域行幂等");

    // C441-11 复算失配检出（数据篡改）。
    bk.rows[0].pass = 1;
    chk(&mut set, "C441-11", bk.recalc() == Ok(false), "篡改可检出");

    // C441-12 四主指标单源断言。
    chk(&mut set, "C441-12", Metric::ALL.len() == 4
        && Metric::ALL[0].name() == "consistency"
        && Metric::ALL[3].name() == "regress-rate", "四主指标单源");

    // ── 深化块 D441 判据 ──
    // D441-01 度量采样账本：两批四指标入账 + 守恒恒等式。
    let mut lg = SampleLedger::new(32);
    for (i, m) in Metric::ALL.iter().enumerate() {
        let _ = lg.push(SampleRow { batch: 1, metric: *m, raw: 9000 + i as u32 * 100, domain_idx: 0 });
        let _ = lg.push(SampleRow { batch: 2, metric: *m, raw: 9100 + i as u32 * 50, domain_idx: 1 });
    }
    chk(&mut set, "D441-01", lg.recs.len() == 8 && lg.conserves(), "采样账本守恒");

    // D441-02 幂等闸：同键二次提交零副作用。
    chk(&mut set, "D441-02", lg.push(SampleRow { batch: 1, metric: Metric::Consistency, raw: 1, domain_idx: 0 }) == Ok(false)
        && lg.recs.len() == 8, "采样幂等闸");

    // D441-03 指标分位与均值（整数）。
    chk(&mut set, "D441-03", lg.metric_quantile(Metric::Consistency, 50) == 9000
        && lg.metric_mean(Metric::Consistency) == 9050, "指标分位均值");

    // D441-04 边界探针：越界分位号/空指标→显性 Err。
    chk(&mut set, "D441-04", probe_metric_quantile(&lg, Metric::RegressRate, 0) == Err(MtCode::CaliberDrift)
        && probe_metric_quantile(&lg, Metric::RegressRate, 101) == Err(MtCode::CaliberDrift), "采样分位探针");

    // D441-05 边界探针：total==0 域行拒入（除零防线）。
    chk(&mut set, "D441-05", probe_zero_total(0) == Err(MtCode::SourceBroken)
        && probe_zero_total(1).is_ok(), "零总检探针");

    // D441-06 覆盖与批次投影。
    chk(&mut set, "D441-06", lg.metric_coverage() == 4 && lg.batch_count() == 2, "覆盖批次投影");

    // D441-07 LCG 口径缺一注入回归：16 轮 100% 抓 + 零误报。
    let (n, c, fp) = lcg_caliber_gap_regression(16);
    chk(&mut set, "D441-07", n == 16 && c == 16 && fp == 0, "口径缺注入全抓");

    // D441-08 序列化往返：采样行编码→解码恒等。
    let s0 = SampleRow { batch: 5, metric: Metric::FixLatency, raw: 42, domain_idx: 3 };
    chk(&mut set, "D441-08", decode_sample(&encode_sample(&s0)) == Ok(s0), "采样序列化往返");

    // D441-09 解码边界：错长/坏指标号→显性 Err。
    chk(&mut set, "D441-09", decode_sample(&[0u8; 8]) == Err(MtCode::SourceBroken)
        && decode_sample(&{ let mut v = encode_sample(&s0); v[2] = 0x0E; v }) == Err(MtCode::CaliberDrift), "采样解码边界");

    // D441-10 度量册快照指纹：改数即变 + 确定性 + hex 投影。
    let mut bk2 = MetricBook::new();
    bk2.put_row("ui", 950, 1000, 180, 200);
    let f0 = metricbook_fingerprint(&bk2);
    let f0b = metricbook_fingerprint(&bk2);
    bk2.put_row("cli", 470, 500, 90, 100);
    let f1 = metricbook_fingerprint(&bk2);
    let _ = bk2.freeze(1);
    let f2 = metricbook_fingerprint(&bk2);
    chk(&mut set, "D441-10", f0 == f0b && f0 != f1 && f1 != f2
        && fingerprint_hex(f2).len() == 8, "度量册指纹");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves441_smoke() {
        let set = run_ves441_checks();
        assert!(set.failed_count() == 0, "ves441 red: {}", set.red_items_text());
    }
}
