// unx1229_u1229 —— UNX-F1229 Draining 态延迟账（排空窗口时长分布）
// 判据原文 UNX-F1229-J1：延迟账全过（逐窗时长分桶直方图 8 桶、Σ桶=窗数守恒、
// P50/P95 分位上界可查、超阈窗立案、指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xb770 独占（0xb770~0xb7af）。
//
// 设计（纯功能 no_std）：
//   * 时长登记：逐排空窗（窗号、时长 tick），8 桶直方图（上界 16/32/64/128/
//     256/512/1024/∞）；
//   * 分位上界：P50/P95 = 直方图累计恰过半/95% 的桶上界（整数化诚实上界）；
//   * 超阈立案：时长 > 阈值（1024）逐窗立案（阈值告警账）；
//   * 守恒：Σ桶计数 = 窗总数；FNV 链指纹冻结。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xb770 独占） ─────────────────────────────────────────────
pub const XC_LD_SEALED: u32 = 0xB771; // 封账后登记
pub const XC_LD_FULL: u32 = 0xB772; // 窗账容量满
pub const XC_LD_ZERO_DUR: u32 = 0xB773; // 零时长窗（非法）
pub const XC_LD_DUP_WNO: u32 = 0xB774; // 窗号重复

/// 桶上界表（最后一桶为 ∞）。
pub const BUCKET_UP: [u64; 7] = [16, 32, 64, 128, 256, 512, 1024];
/// 超阈告警线。
pub const WARN_DUR: u64 = 1024;

/// 延迟账。
pub struct LatencyLedger {
    cap: usize,
    wnos: Vec<u32>,
    durs: Vec<u64>,
    buckets: [u32; 8],
    warned: Vec<u32>,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl LatencyLedger {
    pub fn new(cap: usize) -> LatencyLedger {
        LatencyLedger {
            cap,
            wnos: vec![],
            durs: vec![],
            buckets: [0; 8],
            warned: vec![],
            sealed: false,
            rejects: Vec::new(),
        }
    }

    fn bucket_of(d: u64) -> usize {
        for (i, up) in BUCKET_UP.iter().enumerate() {
            if d <= *up {
                return i;
            }
        }
        7
    }

    /// 登记一窗时长。
    pub fn log(&mut self, wno: u32, dur: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_LD_SEALED, "封账后登记"));
            return Err(XC_LD_SEALED);
        }
        if self.durs.len() >= self.cap {
            self.rejects.push((XC_LD_FULL, "窗账容量满"));
            return Err(XC_LD_FULL);
        }
        if dur == 0 {
            self.rejects.push((XC_LD_ZERO_DUR, "零时长窗"));
            return Err(XC_LD_ZERO_DUR);
        }
        if self.wnos.contains(&wno) {
            self.rejects.push((XC_LD_DUP_WNO, "窗号重复"));
            return Err(XC_LD_DUP_WNO);
        }
        let b = Self::bucket_of(dur);
        self.buckets[b] = self.buckets[b].saturating_add(1);
        if dur > WARN_DUR {
            self.warned.push(wno);
        }
        self.wnos.push(wno);
        self.durs.push(dur);
        Ok(())
    }

    /// 守恒：Σ桶 = 窗数。
    pub fn conserved(&self) -> bool {
        let mut s: u32 = 0;
        for b in self.buckets.iter() {
            s = s.saturating_add(*b);
        }
        s as usize == self.durs.len()
    }

    /// 分位上界：累计占比恰 ≥ q（0~100）的最小桶上界；落在 ∞ 桶则返回 1024 后界 +1 语义（返回 0 表示 >1024）。
    pub fn percentile_upper(&self, q: u32) -> u64 {
        let n = self.durs.len() as u64;
        if n == 0 {
            return 0;
        }
        let need = (n.saturating_mul(q as u64)).div_ceil(100);
        let mut acc: u64 = 0;
        for (i, b) in self.buckets.iter().enumerate() {
            acc = acc.saturating_add(*b as u64);
            if acc >= need {
                if i < 7 {
                    return BUCKET_UP[i];
                }
                return 0; // ∞ 桶：诚实标注超界
            }
        }
        0
    }

    /// P50/P95 上界。
    pub fn p50(&self) -> u64 {
        self.percentile_upper(50)
    }
    pub fn p95(&self) -> u64 {
        self.percentile_upper(95)
    }

    /// 超阈立案数。
    pub fn warned_count(&self) -> usize {
        self.warned.len()
    }

    /// 时长总和。
    pub fn dur_total(&self) -> u64 {
        let mut s: u64 = 0;
        for d in &self.durs {
            s = s.saturating_add(*d);
        }
        s
    }

    pub fn win_count(&self) -> usize {
        self.durs.len()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_LD_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        for (&w, &d) in self.wnos.iter().zip(self.durs.iter()) {
            h ^= (w as u64).rotate_left(5) ^ d.rotate_left(19);
            h = h.wrapping_mul(0x0100_0193);
        }
        for b in self.buckets.iter() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准场景：20 窗时长序列（递增构造）+ 分桶 + 分位可查。
pub fn latency_scenario() -> Result<(usize, u64, u64, u64), u32> {
    let mut l = LatencyLedger::new(32);
    // 时长序列：8,16,24,...,160（20 窗）
    for k in 0..20u32 {
        let dur = 8 + k * 8;
        l.log(k + 1, dur as u64)?;
    }
    if !l.conserved() {
        return Err(XC_LD_ZERO_DUR);
    }
    let p50 = l.p50();
    let p95 = l.p95();
    if p50 == 0 || p95 < p50 {
        return Err(XC_LD_DUP_WNO);
    }
    let fp = l.seal()?;
    Ok((l.win_count(), l.dur_total(), p95, fp))
}

// ── 深化块：桶分布导出与窗表 ───────────────────────────────────────
impl LatencyLedger {
    /// 桶分布导出：8 桶 (上界语义, 计数)。
    pub fn bucket_export(&self) -> [(u64, u32); 8] {
        let mut out = [(0u64, 0u32); 8];
        for i in 0..8 {
            let up = if i < 7 { BUCKET_UP[i] } else { 0 };
            out[i] = (up, self.buckets[i]);
        }
        out
    }
    /// 稳定序窗表（按窗号升序）。
    pub fn win_table(&self) -> Vec<(u32, u64)> {
        let mut v: Vec<(u32, u64)> = vec![];
        for i in 0..self.wnos.len() {
            v.push((self.wnos[i], self.durs[i]));
        }
        for i in 0..v.len() {
            let mut min = i;
            for j in i + 1..v.len() {
                if v[j].0 < v[min].0 { min = j; }
            }
            let t = v[i]; v[i] = v[min]; v[min] = t;
        }
        v
    }
    /// 平均时长。
    pub fn dur_avg(&self) -> u64 {
        let n = self.win_count() as u64;
        if n == 0 { 0 } else { self.dur_total() / n }
    }
    /// 峰值时长。
    pub fn dur_peak(&self) -> u64 {
        let mut m: u64 = 0;
        for d in &self.durs {
            if *d > m { m = *d; }
        }
        m
    }
    /// 超阈窗号清单（稳定序）。
    pub fn warned_list(&self) -> Vec<u32> {
        let mut v: Vec<u32> = vec![];
        for &w in &self.warned { v.push(w); }
        v
    }
}

/// 注入核：六类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）零时长必拒
    {
        let mut l = LatencyLedger::new(8);
        if l.log(1, 0) == Err(XC_LD_ZERO_DUR) {
            caught += 1;
        }
    }
    // 2）窗号重复必拒
    {
        let mut l = LatencyLedger::new(8);
        let _ = l.log(1, 10);
        if l.log(1, 20) == Err(XC_LD_DUP_WNO) {
            caught += 1;
        }
    }
    // 3）封账后登记必拒
    {
        let mut l = LatencyLedger::new(8);
        let _ = l.log(1, 10);
        let _ = l.seal();
        if l.log(2, 10) == Err(XC_LD_SEALED) {
            caught += 1;
        }
    }
    // 4）容量满必拒
    {
        let mut l = LatencyLedger::new(1);
        let _ = l.log(1, 10);
        if l.log(2, 10) == Err(XC_LD_FULL) {
            caught += 1;
        }
    }
    // 5）超阈立案（>1024 入告警账）
    {
        let mut l = LatencyLedger::new(8);
        let _ = l.log(1, 2000);
        if l.warned_count() == 1 {
            caught += 1;
        }
    }
    // 6）守恒（Σ桶=窗数）
    {
        let mut l = LatencyLedger::new(8);
        let _ = l.log(1, 16);
        let _ = l.log(2, 17);
        let _ = l.log(3, 5000);
        if l.conserved() {
            caught += 1;
        }
    }
    (caught, N)
}
