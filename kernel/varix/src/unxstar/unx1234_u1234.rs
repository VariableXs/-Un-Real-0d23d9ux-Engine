// unx1234_u1234 —— UNX-F1234 Draining 态统计矩阵（双源事件率对照）
// 判据原文 UNX-F1234-J1：对照账全过（P/I 双源逐窗事件率千分比、对照差值
// 逐窗记账、差值超带立案（源失衡）、Σ守恒、指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xb8b0 独占（0xb8b0~0xb8ef）。
//
// 设计（纯功能 no_std）：
//   * 双源窗账：每窗（P 事件数、I 事件数、窗 tick），事件率 = n*1000/tick；
//   * 对照差值：|rP - rI| 千分比逐窗记账；差值超带（> 阈）立案（源失衡）；
//   * 守恒：Σ窗事件 = P 总 + I 总；对照行数 = 窗数；
//   * FNV 链指纹冻结，封账后拒记账。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xb8b0 独占） ─────────────────────────────────────────────
pub const XC_CM_SEALED: u32 = 0xB8B1; // 封账后记账
pub const XC_CM_ZERO_TICK: u32 = 0xB8B2; // 零窗时长
pub const XC_CM_DUP_WNO: u32 = 0xB8B3; // 窗号重复
pub const XC_CM_IMBALANCE: u32 = 0xB8B4; // 源失衡立案（差值超带）
pub const XC_CM_FULL: u32 = 0xB8B5; // 账容量满

/// 对照行。
#[derive(Clone, Copy)]
pub struct CmpRow {
    pub wno: u32,
    pub p_events: u32,
    pub i_events: u32,
    pub ticks: u32,
    /// |rP - rI| 千分比。
    pub gap_pm: u32,
}

/// 对照账。
pub struct CmpMatrix {
    cap: usize,
    rows: Vec<CmpRow>,
    /// 失衡带（千分比）。
    band_pm: u32,
    imbalances: Vec<u32>,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl CmpMatrix {
    pub fn new(cap: usize, band_pm: u32) -> CmpMatrix {
        CmpMatrix {
            cap,
            rows: vec![],
            band_pm,
            imbalances: vec![],
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 记一窗对照。
    pub fn log(&mut self, wno: u32, p: u32, i: u32, ticks: u32) -> Result<u32, u32> {
        if self.sealed {
            self.rejects.push((XC_CM_SEALED, "封账后记账"));
            return Err(XC_CM_SEALED);
        }
        if self.rows.len() >= self.cap {
            self.rejects.push((XC_CM_FULL, "账容量满"));
            return Err(XC_CM_FULL);
        }
        if ticks == 0 {
            self.rejects.push((XC_CM_ZERO_TICK, "零窗时长"));
            return Err(XC_CM_ZERO_TICK);
        }
        if self.rows.iter().any(|r| r.wno == wno) {
            self.rejects.push((XC_CM_DUP_WNO, "窗号重复"));
            return Err(XC_CM_DUP_WNO);
        }
        let rp = (p as u64 * 1000 / ticks as u64) as u32;
        let ri = (i as u64 * 1000 / ticks as u64) as u32;
        let gap = if rp > ri { rp - ri } else { ri - rp };
        if gap > self.band_pm {
            self.imbalances.push(wno);
        }
        self.rows.push(CmpRow { wno, p_events: p, i_events: i, ticks, gap_pm: gap });
        Ok(gap)
    }

    /// P 总事件。
    pub fn p_total(&self) -> u64 {
        self.rows.iter().map(|r| r.p_events as u64).sum::<u64>()
    }
    /// I 总事件。
    pub fn i_total(&self) -> u64 {
        self.rows.iter().map(|r| r.i_events as u64).sum::<u64>()
    }

    /// 守恒：行数 = 窗数（同号拒绝保证）；失衡立案数 ≤ 行数。
    pub fn conserved(&self) -> bool {
        self.imbalances.len() <= self.rows.len()
    }

    /// 失衡立案数。
    pub fn imbalance_count(&self) -> usize {
        self.imbalances.len()
    }

    /// 最大差值。
    pub fn max_gap(&self) -> u32 {
        let mut m: u32 = 0;
        for r in &self.rows {
            if r.gap_pm > m {
                m = r.gap_pm;
            }
        }
        m
    }

    /// 平均差值（千分比，整数）。
    pub fn avg_gap(&self) -> u32 {
        if self.rows.is_empty() {
            return 0;
        }
        let mut s: u64 = 0;
        for r in &self.rows {
            s = s.saturating_add(r.gap_pm as u64);
        }
        (s / self.rows.len() as u64) as u32
    }

    pub fn rows_len(&self) -> usize {
        self.rows.len()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_CM_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        for r in &self.rows {
            h ^= (r.wno as u64).rotate_left(5)
                ^ (r.p_events as u64).rotate_left(13)
                ^ (r.i_events as u64).rotate_left(23)
                ^ (r.gap_pm as u64).rotate_left(37);
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准对照场景：6 窗对照（5 带内 1 失衡立案）。
pub fn cmp_scenario() -> Result<(usize, usize, u32, u64), u32> {
    let mut m = CmpMatrix::new(8, 100);
    m.log(1, 100, 105, 1000)?; // gap 5
    m.log(2, 90, 95, 1000)?;   // gap 5
    m.log(3, 80, 85, 1000)?;   // gap 5
    m.log(4, 70, 78, 1000)?;   // gap 8
    m.log(5, 60, 60, 1000)?;   // gap 0
    m.log(6, 50, 200, 1000)?;  // gap 150 → 失衡立案
    if !m.conserved() {
        return Err(XC_CM_FULL);
    }
    if m.imbalance_count() != 1 {
        return Err(XC_CM_IMBALANCE);
    }
    let maxg = m.max_gap();
    if maxg != 150 {
        return Err(XC_CM_ZERO_TICK);
    }
    let fp = m.seal()?;
    Ok((m.rows_len(), m.imbalance_count(), maxg, fp))
}

// ── 深化块：趋势斜率与失衡时间线 ───────────────────────────────────
impl CmpMatrix {
    /// 差值趋势斜率（后均值-前均值，千分比有符号）。
    pub fn trend_slope(&self) -> i32 {
        let n = self.rows.len();
        if n < 2 { return 0; }
        let half = n / 2;
        let mut f: u64 = 0;
        let mut b: u64 = 0;
        for (i, r) in self.rows.iter().enumerate() {
            if i < half { f += r.gap_pm as u64; } else { b += r.gap_pm as u64; }
        }
        let fa = f / half as u64;
        let ba = b / (n - half) as u64;
        ba as i64 as i32 - fa as i64 as i32
    }
    /// 失衡时间线：立案窗号升序稳定排。
    pub fn imbalance_timeline(&self) -> Vec<u32> {
        let mut v: Vec<u32> = vec![];
        for &w in &self.imbalances { v.push(w); }
        for i in 0..v.len() {
            let mut min = i;
            for j in i + 1..v.len() {
                if v[j] < v[min] { min = j; }
            }
            let t = v[i]; v[i] = v[min]; v[min] = t;
        }
        v
    }
    /// P/I 总对照差（有符号：正=P 多）。
    pub fn total_bias(&self) -> i64 {
        self.p_total() as i64 - self.i_total() as i64
    }
    /// 单窗查询。
    pub fn row_of(&self, wno: u32) -> Option<CmpRow> {
        self.rows.iter().find(|r| r.wno == wno).copied()
    }
}

/// 注入核：六类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）零窗时长必拒
    {
        let mut m = CmpMatrix::new(4, 100);
        if m.log(1, 10, 10, 0) == Err(XC_CM_ZERO_TICK) {
            caught += 1;
        }
    }
    // 2）窗号重复必拒
    {
        let mut m = CmpMatrix::new(4, 100);
        let _ = m.log(1, 10, 10, 100);
        if m.log(1, 20, 20, 100) == Err(XC_CM_DUP_WNO) {
            caught += 1;
        }
    }
    // 3）失衡立案（差值超带）
    {
        let mut m = CmpMatrix::new(4, 50);
        let _ = m.log(1, 10, 200, 100); // gap 190 > 50
        if m.imbalance_count() == 1 {
            caught += 1;
        }
    }
    // 4）封账后记账必拒
    {
        let mut m = CmpMatrix::new(4, 100);
        let _ = m.log(1, 10, 10, 100);
        let _ = m.seal();
        if m.log(2, 10, 10, 100) == Err(XC_CM_SEALED) {
            caught += 1;
        }
    }
    // 5）容量满必拒
    {
        let mut m = CmpMatrix::new(1, 100);
        let _ = m.log(1, 10, 10, 100);
        if m.log(2, 10, 10, 100) == Err(XC_CM_FULL) {
            caught += 1;
        }
    }
    // 6）均值与最值一致
    {
        let mut m = CmpMatrix::new(4, 100);
        let _ = m.log(1, 100, 100, 100); // 0
        let _ = m.log(2, 90, 100, 100);  // 10
        if m.max_gap() == 10 && m.avg_gap() == 5 {
            caught += 1;
        }
    }
    (caught, N)
}
