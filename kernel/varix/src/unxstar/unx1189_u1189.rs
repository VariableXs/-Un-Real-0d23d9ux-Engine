// unx1189_u1189 —— UNX-F1189 核间中断延迟账（跨核投递延迟分布与预算对齐）
// 判据原文 UNX-F1189-J1：延迟账全过（IPI 投递延迟逐例分桶 8 桶、Σ桶=例数
// 守恒、P95 上界与预算对齐、超预算立案、指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0x8d70 独占（0x8d70~0x8daf）。
//
// 设计（纯功能 no_std）：
//   * 投递账：逐 IPI（源核/目标核/延迟 ns），8 桶直方图（上界 100/200/400/
//     800/1600/3200/6400/∞ ns）；
//   * 预算对齐：P95 上界 ≤ 预算（6400ns）达标判定；超预算逐例立案；
//   * 守恒：Σ桶 = 例数；自投递（源=目标）拒；
//   * FNV 链指纹冻结，封账后拒登记。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x8d70 独占） ─────────────────────────────────────────────
pub const XC_IL_SEALED: u32 = 0x8D71; // 封账后登记
pub const XC_IL_SELF: u32 = 0x8D72; // 自投递
pub const XC_IL_ZERO_LAT: u32 = 0x8D73; // 零延迟（非法）
pub const XC_IL_FULL: u32 = 0x8D74; // 账容量满

/// 桶上界表（ns，末桶 ∞）。
pub const IPI_BUCKET_UP: [u64; 7] = [100, 200, 400, 800, 1600, 3200, 6400];
/// 投递预算（ns）。
pub const IPI_BUDGET_NS: u64 = 6400;

/// 投递行。
#[derive(Clone, Copy)]
pub struct IpiRow {
    pub from_core: u8,
    pub to_core: u8,
    pub lat_ns: u64,
}

/// 延迟账。
pub struct IpiLatLedger {
    cap: usize,
    rows: Vec<IpiRow>,
    buckets: [u32; 8],
    over_budget: u32,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl IpiLatLedger {
    pub fn new(cap: usize) -> IpiLatLedger {
        IpiLatLedger {
            cap,
            rows: vec![],
            buckets: [0; 8],
            over_budget: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    fn bucket_of(lat: u64) -> usize {
        for (i, up) in IPI_BUCKET_UP.iter().enumerate() {
            if lat <= *up {
                return i;
            }
        }
        7
    }

    /// 登记一例投递延迟。
    pub fn log(&mut self, from_core: u8, to_core: u8, lat_ns: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_IL_SEALED, "封账后登记"));
            return Err(XC_IL_SEALED);
        }
        if self.rows.len() >= self.cap {
            self.rejects.push((XC_IL_FULL, "账容量满"));
            return Err(XC_IL_FULL);
        }
        if from_core == to_core {
            self.rejects.push((XC_IL_SELF, "自投递"));
            return Err(XC_IL_SELF);
        }
        if lat_ns == 0 {
            self.rejects.push((XC_IL_ZERO_LAT, "零延迟"));
            return Err(XC_IL_ZERO_LAT);
        }
        let b = Self::bucket_of(lat_ns);
        self.buckets[b] = self.buckets[b].saturating_add(1);
        if lat_ns > IPI_BUDGET_NS {
            self.over_budget = self.over_budget.saturating_add(1);
        }
        self.rows.push(IpiRow { from_core, to_core, lat_ns });
        Ok(())
    }

    /// 守恒：Σ桶 = 例数。
    pub fn conserved(&self) -> bool {
        let mut s: u32 = 0;
        for b in self.buckets.iter() {
            s = s.saturating_add(*b);
        }
        s as usize == self.rows.len()
    }

    /// 分位上界（q 0~100）。
    pub fn percentile_upper(&self, q: u32) -> u64 {
        let n = self.rows.len() as u64;
        if n == 0 {
            return 0;
        }
        let need = (n.saturating_mul(q as u64) + 99) / 100;
        let mut acc: u64 = 0;
        for (i, b) in self.buckets.iter().enumerate() {
            acc = acc.saturating_add(*b as u64);
            if acc >= need {
                return if i < 7 { IPI_BUCKET_UP[i] } else { 0 };
            }
        }
        0
    }

    /// P95 与预算对齐。
    pub fn p95_in_budget(&self) -> bool {
        let p = self.percentile_upper(95);
        p != 0 && p <= IPI_BUDGET_NS
    }

    /// 均值。
    pub fn lat_avg(&self) -> u64 {
        let n = self.rows.len() as u64;
        if n == 0 {
            return 0;
        }
        let mut s: u64 = 0;
        for r in &self.rows {
            s = s.saturating_add(r.lat_ns);
        }
        s / n
    }

    /// 峰值。
    pub fn lat_peak(&self) -> u64 {
        let mut m: u64 = 0;
        for r in &self.rows {
            if r.lat_ns > m {
                m = r.lat_ns;
            }
        }
        m
    }

    pub fn over_budget(&self) -> u32 {
        self.over_budget
    }
    pub fn rows_len(&self) -> usize {
        self.rows.len()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_IL_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        for r in &self.rows {
            h ^= (r.from_core as u64).rotate_left(5)
                ^ (r.to_core as u64).rotate_left(17)
                ^ r.lat_ns.rotate_left(31);
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

/// 标准场景：20 例投递（构造分布）+ 分桶 + P95 预算对齐。
pub fn ipi_lat_scenario() -> Result<(usize, u64, u64), u32> {
    let mut l = IpiLatLedger::new(32);
    // 分布：16 例 ≤400ns、3 例 ≤1600ns、1 例 5000ns（全预算内）
    let lats: [u64; 20] = [
        80, 120, 150, 200, 220, 260, 300, 340, 380, 400,
        420, 500, 700, 900, 1200, 1500, 1600, 2000, 3000, 5000,
    ];
    for (k, &lat) in lats.iter().enumerate() {
        l.log((k % 4) as u8, ((k + 1) % 4) as u8, lat)?;
    }
    if !l.conserved() {
        return Err(XC_IL_SELF);
    }
    if !l.p95_in_budget() {
        return Err(XC_IL_ZERO_LAT);
    }
    let fp = l.seal()?;
    Ok((l.rows_len(), l.lat_avg(), fp))
}

/// 注入核：六类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）自投递必拒
    {
        let mut l = IpiLatLedger::new(4);
        if l.log(1, 1, 100) == Err(XC_IL_SELF) {
            caught += 1;
        }
    }
    // 2）零延迟必拒
    {
        let mut l = IpiLatLedger::new(4);
        if l.log(0, 1, 0) == Err(XC_IL_ZERO_LAT) {
            caught += 1;
        }
    }
    // 3）封账后登记必拒
    {
        let mut l = IpiLatLedger::new(4);
        let _ = l.log(0, 1, 100);
        let _ = l.seal();
        if l.log(1, 2, 100) == Err(XC_IL_SEALED) {
            caught += 1;
        }
    }
    // 4）容量满必拒
    {
        let mut l = IpiLatLedger::new(1);
        let _ = l.log(0, 1, 100);
        if l.log(1, 2, 100) == Err(XC_IL_FULL) {
            caught += 1;
        }
    }
    // 5）超预算立案（>6400）
    {
        let mut l = IpiLatLedger::new(4);
        let _ = l.log(0, 1, 7000);
        if l.over_budget() == 1 {
            caught += 1;
        }
    }
    // 6）守恒与峰值
    {
        let mut l = IpiLatLedger::new(8);
        let _ = l.log(0, 1, 100);
        let _ = l.log(1, 2, 300);
        if l.conserved() && l.lat_peak() == 300 {
            caught += 1;
        }
    }
    (caught, N)
}
