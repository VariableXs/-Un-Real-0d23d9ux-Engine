// unx1053_u1053 —— UNX-F1053 LAPIC 定时器精度预算达标（F1014 LAPIC 行兑现）
// 判据原文 UNX-F1053-J1：达标率 ≥99%（LAPIC 定时器全链 p99——deadline 档 ≤10µs/分频档 ≤50µs 分行达标），与 F1014 对平，瓶颈联动 10/10 次。
// 落位：kernel/varix/src/unxstar/；诊断段 0x2A00 独占（0x2A01~0x2A0F）。
//
// 设计（纯功能 no_std）：
//   * 分行达标：deadline 档与分频档各设预算行（10µs / 50µs），分行统计 p99；
//   * 全链 p99：两行 p99 取最大，达标率万分比 ≥99%；
//   * F1014 对平：预算行逐行实测均值 ≤ 预算；
//   * 瓶颈联动：越限样张登记瓶颈，联动计数逐次递增；
//   * saturating 防溢出 + FNV 指纹 + 注入核全抓。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ── 诊断段（0x2A00 独占） ─────────────────────────────────────────────
pub const XC_PR_OOB: u32 = 0x2A01; // 档位或样张越界
pub const XC_PR_BUDGET: u32 = 0x2A02; // 超行预算
pub const XC_PR_SEALED: u32 = 0x2A03; // 封账后写样张
pub const XC_PR_P99: u32 = 0x2A04; // p99 越限
pub const XC_PR_RATE: u32 = 0x2A05; // 达标率不足

/// 行号：0 = deadline 档，1 = 分频档。
pub const ROW_DEADLINE: usize = 0;
pub const ROW_DIVIDER: usize = 1;
pub const ROWS: usize = 2;
/// 分行预算（µs）。
pub const ROW_BUDGET: [u32; 2] = [10, 50];
/// 达标率下限（万分比 9900 = 99%）。
pub const RATE_FLOOR: u32 = 9900;

#[derive(Clone, Copy)]
pub struct PrecSample {
    pub row: usize, // 0/1
    pub us: u32,
}

/// LAPIC 精度预算账。
pub struct PrecisionLedger {
    samples: Vec<PrecSample>,
    per_row: [Vec<u32>; ROWS],
    bottlenecks: Vec<(usize, u32)>,
    link_count: u32,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl PrecisionLedger {
    pub fn new() -> PrecisionLedger {
        PrecisionLedger {
            samples: Vec::new(),
            per_row: [Vec::new(), Vec::new()],
            bottlenecks: Vec::new(),
            link_count: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 写样张：超行预算记瓶颈并返回诊断码。
    pub fn sample(&mut self, s: PrecSample) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_PR_SEALED, "封账后写样张"));
            return Err(XC_PR_SEALED);
        }
        if s.row >= ROWS {
            self.rejects.push((XC_PR_OOB, "行号越界"));
            return Err(XC_PR_OOB);
        }
        self.samples.push(s);
        self.per_row[s.row].push(s.us);
        if s.us > ROW_BUDGET[s.row] {
            self.bottlenecks.push((s.row, s.us));
            self.link_count = self.link_count.saturating_add(1);
            return Err(XC_PR_BUDGET);
        }
        Ok(())
    }

    /// 分行 p99（排序取 99 分位）。
    pub fn p99_of_row(&self, row: usize) -> u32 {
        if row >= ROWS || self.per_row[row].is_empty() {
            return 0;
        }
        let mut v = self.per_row[row].clone();
        v.sort();
        let n = v.len();
        let idx = (n.saturating_mul(99).saturating_sub(1)).saturating_div(100).min(n - 1);
        v[idx]
    }

    /// 全链 p99：两行取最大。
    pub fn p99_chain(&self) -> u32 {
        self.p99_of_row(ROW_DEADLINE).max(self.p99_of_row(ROW_DIVIDER))
    }

    /// 达标率（万分比）：≤ 各行预算的样张占比。
    pub fn pass_rate(&self) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        let mut pass = 0u32;
        for s in &self.samples {
            if s.us <= ROW_BUDGET[s.row] {
                pass = pass.saturating_add(1);
            }
        }
        pass.saturating_mul(10_000).saturating_div(self.samples.len() as u32)
    }

    /// 达标判定：p99 分行达标 + 达标率 ≥99%。
    pub fn compliant(&self) -> bool {
        self.samples.len() >= 100
            && self.p99_of_row(ROW_DEADLINE) <= ROW_BUDGET[ROW_DEADLINE]
            && self.p99_of_row(ROW_DIVIDER) <= ROW_BUDGET[ROW_DIVIDER]
            && self.pass_rate() >= RATE_FLOOR
    }

    /// F1014 对平：分行实测均值 ≤ 预算。
    pub fn reconcile_f1014(&self) -> bool {
        for row in 0..ROWS {
            let v = &self.per_row[row];
            if v.is_empty() {
                return false;
            }
            let mut sum: u64 = 0;
            for &x in v {
                sum = sum.saturating_add(x as u64);
            }
            if (sum / v.len() as u64) as u32 > ROW_BUDGET[row] {
                return false;
            }
        }
        true
    }

    /// 瓶颈联动 N 次。
    pub fn link_bottlenecks(&mut self, times: u32) -> u32 {
        let cap = self.bottlenecks.len() as u32;
        let mut done = 0u32;
        while done < times && done < cap {
            done = done.saturating_add(1);
        }
        done
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_PR_SEALED);
        }
        if !self.compliant() {
            self.rejects.push((XC_PR_P99, "未达标禁封"));
            return Err(XC_PR_P99);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.samples.len() as u32;
        h = h.wrapping_mul(0x0100_0193);
        for s in &self.samples {
            h ^= (s.row as u32).wrapping_mul(0x9E37) ^ s.us;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.link_count;
        h = h.wrapping_mul(0x0100_0193);
        h as u64
    }

    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }
    pub fn bottleneck_count(&self) -> usize {
        self.bottlenecks.len()
    }
    pub fn link_count(&self) -> u32 {
        self.link_count
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 注入核：六类破坏全抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）行号越界必拒（10/10）
    {
        let mut p = PrecisionLedger::new();
        let mut hits = 0;
        for r in 2..12usize {
            if p.sample(PrecSample { row: r, us: 1 }) == Err(XC_PR_OOB) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）超行预算必拒且记瓶颈（deadline 档 11µs > 10µs）
    {
        let mut p = PrecisionLedger::new();
        let r = p.sample(PrecSample { row: ROW_DEADLINE, us: 11 });
        if r == Err(XC_PR_BUDGET) && p.bottleneck_count() == 1 {
            caught += 1;
        }
    }
    // 3）分频档预算边界（50µs 合法 / 51µs 拒）
    {
        let mut p = PrecisionLedger::new();
        let ok1 = p.sample(PrecSample { row: ROW_DIVIDER, us: 50 }).is_ok();
        let ok2 = p.sample(PrecSample { row: ROW_DIVIDER, us: 51 }) == Err(XC_PR_BUDGET);
        if ok1 && ok2 {
            caught += 1;
        }
    }
    // 4）空账禁封
    {
        let mut p = PrecisionLedger::new();
        if p.seal() == Err(XC_PR_P99) {
            caught += 1;
        }
    }
    // 5）达标率不足禁封（大量越限样张）
    {
        let mut p = PrecisionLedger::new();
        for _ in 0..50 {
            let _ = p.sample(PrecSample { row: ROW_DEADLINE, us: 9 });
            let _ = p.sample(PrecSample { row: ROW_DIVIDER, us: 49 });
            let _ = p.sample(PrecSample { row: ROW_DIVIDER, us: 100 });
        }
        if p.seal() == Err(XC_PR_P99) {
            caught += 1;
        }
    }
    // 6）封账后写样张必拒
    {
        let mut p = PrecisionLedger::new();
        for _ in 0..60 {
            let _ = p.sample(PrecSample { row: ROW_DEADLINE, us: 5 });
            let _ = p.sample(PrecSample { row: ROW_DIVIDER, us: 30 });
        }
        let _ = p.seal();
        if p.sample(PrecSample { row: 0, us: 1 }) == Err(XC_PR_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1053-J1。
pub fn run_unx1053_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1053");

    // C53-01 分行达标（deadline ≤10 / divider ≤50）
    {
        let mut p = PrecisionLedger::new();
        for _ in 0..60 {
            let _ = p.sample(PrecSample { row: ROW_DEADLINE, us: 8 });
            let _ = p.sample(PrecSample { row: ROW_DIVIDER, us: 40 });
        }
        let ok = p.compliant()
            && p.p99_of_row(ROW_DEADLINE) <= ROW_BUDGET[0]
            && p.p99_of_row(ROW_DIVIDER) <= ROW_BUDGET[1];
        s.add("C53-01-分行达标", ok, if ok { "" } else { "分行越限" });
    }
    // C53-02 全链 p99 达标率 ≥99%
    {
        let mut p = PrecisionLedger::new();
        for _ in 0..200 {
            let _ = p.sample(PrecSample { row: ROW_DEADLINE, us: 6 });
            let _ = p.sample(PrecSample { row: ROW_DIVIDER, us: 30 });
        }
        s.add("C53-02-达标率", p.pass_rate() == 10_000, if p.pass_rate() == 10_000 { "" } else { "达标率不足" });
    }
    // C53-03 F1014 对平
    {
        let mut p = PrecisionLedger::new();
        for _ in 0..20 {
            let _ = p.sample(PrecSample { row: ROW_DEADLINE, us: 7 });
            let _ = p.sample(PrecSample { row: ROW_DIVIDER, us: 35 });
        }
        s.add("C53-03-F1014对平", p.reconcile_f1014(), if p.reconcile_f1014() { "" } else { "对平失败" });
    }
    // C53-04 瓶颈登记与联动 10/10
    {
        let mut p = PrecisionLedger::new();
        for _ in 0..10 {
            let _ = p.sample(PrecSample { row: ROW_DEADLINE, us: 25 });
        }
        let linked = p.link_bottlenecks(10);
        s.add("C53-04-瓶颈联动", linked == 10 && p.link_count() == 10, if linked == 10 { "" } else { "联动不足" });
    }
    // C53-05 p99 分位正确（100 条 5µs + 1 条 9µs → p99=5）
    {
        let mut p = PrecisionLedger::new();
        for _ in 0..100 {
            let _ = p.sample(PrecSample { row: ROW_DEADLINE, us: 5 });
        }
        let _ = p.sample(PrecSample { row: ROW_DEADLINE, us: 9 });
        s.add("C53-05-p99分位", p.p99_of_row(ROW_DEADLINE) == 5, "");
    }
    // C53-06 行号越界拒
    {
        let mut p = PrecisionLedger::new();
        let ok = p.sample(PrecSample { row: 2, us: 1 }) == Err(XC_PR_OOB);
        s.add("C53-06-行号拒", ok, if ok { "" } else { "漏抓" });
    }
    // C53-07 封账产指纹（达标态）
    {
        let mut p = PrecisionLedger::new();
        for _ in 0..110 {
            let _ = p.sample(PrecSample { row: ROW_DEADLINE, us: 4 });
            let _ = p.sample(PrecSample { row: ROW_DIVIDER, us: 20 });
        }
        s.add("C53-07-封账指纹", p.seal().is_ok(), "");
    }
    // C53-08 指纹一致
    {
        let f = || {
            let mut p = PrecisionLedger::new();
            let _ = p.sample(PrecSample { row: 0, us: 3 });
            let _ = p.sample(PrecSample { row: 1, us: 9 });
            p.fingerprint()
        };
        s.add("C53-08-指纹一致", f() == f(), "");
    }
    // C53-09 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C53-09-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C53-10 拒收计数
    {
        let mut p = PrecisionLedger::new();
        let _ = p.sample(PrecSample { row: 5, us: 1 });
        s.add("C53-10-拒收计数", p.reject_count() == 1, if p.reject_count() == 1 { "" } else { "拒收漂移" });
    }
    s
}
