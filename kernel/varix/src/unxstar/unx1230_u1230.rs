// unx1230_u1230 —— UNX-F1230 Draining 态与 F0969 对账底座联动验证
// 判据原文 UNX-F1230-J1：联动验证全过（Draining 窗账上报底座双向核验：
// 窗号连续性、消费数预算带、Σ消费守恒；一致率 100%；指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xb7b0 独占（0xb7b0~0xb7ef）。
//
// 设计（纯功能 no_std）：
//   * 底座侧核账器（语义对齐 F0969 双模对账）：窗号连续（跳号立案）、
//     消费数 ∈ [0, 预算]（越带立案）、Σ消费 = 上报总数（账实双向核验）；
//   * Draining 侧上报：逐窗（窗号、消费数）推送底座，双向各记一份；
//   * 一致率：双侧账逐窗对拍（号与数全等），100% 达标；
//   * FNV 链指纹冻结，封账后拒上报。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xb7b0 独占） ─────────────────────────────────────────────
pub const XC_LN_SEALED: u32 = 0xB7B1; // 封账后上报
pub const XC_LN_GAP_WNO: u32 = 0xB7B2; // 窗号跳号
pub const XC_LN_OVER_BUDGET: u32 = 0xB7B3; // 消费数越预算带
pub const XC_LN_SUM_MISMATCH: u32 = 0xB7B4; // Σ消费≠上报总数
pub const XC_LN_SIDE_DIFF: u32 = 0xB7B5; // 双侧账对拍差异
pub const XC_LN_FULL: u32 = 0xB7B6; // 上报账容量满

/// 底座核账器。
pub struct ReconBase {
    budget: u32,
    expect_wno: u32,
    /// 底座侧账：(窗号, 消费数)。
    base_rows: Vec<(u32, u32)>,
    sum_reported: u64,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl ReconBase {
    pub fn new(budget: u32) -> ReconBase {
        ReconBase {
            budget,
            expect_wno: 1,
            base_rows: vec![],
            sum_reported: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 上报一窗。
    pub fn report(&mut self, wno: u32, drained: u32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_LN_SEALED, "封账后上报"));
            return Err(XC_LN_SEALED);
        }
        if wno != self.expect_wno {
            self.rejects.push((XC_LN_GAP_WNO, "窗号跳号"));
            return Err(XC_LN_GAP_WNO);
        }
        if drained > self.budget {
            self.rejects.push((XC_LN_OVER_BUDGET, "消费数越预算带"));
            return Err(XC_LN_OVER_BUDGET);
        }
        self.base_rows.push((wno, drained));
        self.sum_reported = self.sum_reported.saturating_add(drained as u64);
        self.expect_wno = self.expect_wno.saturating_add(1);
        Ok(())
    }

    /// 双向核验：调用侧给出 Draining 侧账（窗号、消费数）与总消费数。
    pub fn verify(&mut self, side_rows: &[(u32, u32)], side_total: u64) -> Result<bool, u32> {
        if self.sealed {
            self.rejects.push((XC_LN_SEALED, "封账后核验"));
            return Err(XC_LN_SEALED);
        }
        // 双侧对拍：行数、逐行号、逐行数全等
        if side_rows.len() != self.base_rows.len() {
            self.rejects.push((XC_LN_SIDE_DIFF, "双侧账行数差异"));
            return Err(XC_LN_SIDE_DIFF);
        }
        for (i, &(w, d)) in side_rows.iter().enumerate() {
            let (bw, bd) = self.base_rows[i];
            if bw != w || bd != d {
                self.rejects.push((XC_LN_SIDE_DIFF, "双侧账逐行差异"));
                return Err(XC_LN_SIDE_DIFF);
            }
        }
        // Σ核验：双侧各自 Σ 与上报总数一致
        let mut bs: u64 = 0;
        for &(_, d) in &self.base_rows {
            bs = bs.saturating_add(d as u64);
        }
        if bs != self.sum_reported || side_total != self.sum_reported {
            self.rejects.push((XC_LN_SUM_MISMATCH, "Σ消费≠上报总数"));
            return Err(XC_LN_SUM_MISMATCH);
        }
        Ok(true)
    }

    pub fn base_rows_len(&self) -> usize {
        self.base_rows.len()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_LN_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        for &(w, d) in &self.base_rows {
            h ^= (w as u64).rotate_left(7) ^ (d as u64).rotate_left(31);
            h = h.wrapping_mul(0x0100_0193);
        }
        h = h.wrapping_mul(0x0100_0193) ^ self.sum_reported;
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// Draining 侧联动账。
pub struct DrainSide {
    cap: usize,
    rows: Vec<(u32, u32)>,
    total: u64,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl DrainSide {
    pub fn new(cap: usize) -> DrainSide {
        DrainSide { cap, rows: vec![], total: 0, sealed: false, rejects: Vec::new() }
    }

    /// 记一窗并同步上报底座。
    pub fn push(&mut self, base: &mut ReconBase, wno: u32, drained: u32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_LN_SEALED, "封账后上报"));
            return Err(XC_LN_SEALED);
        }
        if self.rows.len() >= self.cap {
            self.rejects.push((XC_LN_FULL, "上报账容量满"));
            return Err(XC_LN_FULL);
        }
        base.report(wno, drained)?;
        self.rows.push((wno, drained));
        self.total = self.total.saturating_add(drained as u64);
        Ok(())
    }

    pub fn rows(&self) -> &Vec<(u32, u32)> {
        &self.rows
    }
    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_LN_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x811C_9DC5;
        for &(w, d) in &self.rows {
            h ^= (w as u64).rotate_left(11) ^ (d as u64).rotate_left(23);
            h = h.wrapping_mul(0x0100_0193);
        }
        h = h.wrapping_mul(0x0100_0193) ^ self.total;
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准联动场景：8 窗上报 + 双向核验一致率 100%。
pub fn link_scenario() -> Result<(usize, u64, u64), u32> {
    let mut base = ReconBase::new(512);
    let mut side = DrainSide::new(16);
    // 8 窗消费（预算内递减）
    let durs: [u32; 8] = [400, 380, 350, 300, 260, 200, 120, 60];
    for (i, d) in durs.iter().enumerate() {
        side.push(&mut base, (i + 1) as u32, *d)?;
    }
    let ok = base.verify(side.rows(), side.total())?;
    if !ok {
        return Err(XC_LN_SIDE_DIFF);
    }
    let fpb = base.seal()?;
    let fps = side.seal()?;
    Ok((base.base_rows_len(), side.total(), fpb ^ fps))
}

// ── 深化块：批量上报、行读取与差异定位 ─────────────────────────────
impl ReconBase {
    /// 批量上报：窗号从 expect 连续递增。
    pub fn report_batch(&mut self, counts: &[u32]) -> Result<usize, u32> {
        let mut n = 0usize;
        for &c in counts {
            let w = self.expect_wno;
            self.report(w, c)?;
            n += 1;
        }
        Ok(n)
    }
    /// 行读取。
    pub fn row_at(&self, i: usize) -> (u32, u32) { self.base_rows[i] }
    /// 已核窗数。
    pub fn verified_windows(&self) -> u32 { self.expect_wno - 1 }
    /// 满窗率（消费恰等于预算的窗占比，基点）。
    pub fn full_rate_bps(&self) -> u32 {
        if self.base_rows.is_empty() { return 0; }
        let full = self.base_rows.iter().filter(|&&(_, d)| d == self.budget).count();
        ((full as u64 * 10000) / self.base_rows.len() as u64) as u32
    }
}

impl DrainSide {
    /// 差异定位：与底座账逐行比对，返回首个差异行号（0=一致）。
    pub fn diff_locate(&self, base: &ReconBase) -> usize {
        let b = base.base_rows_len();
        let n = if self.rows.len() < b { self.rows.len() } else { b };
        for i in 0..n {
            if self.rows[i] != base.row_at(i) { return i + 1; }
        }
        0
    }
}

/// 注入核：七类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 7;

    // 1）窗号跳号必拒
    {
        let mut b = ReconBase::new(512);
        let _ = b.report(1, 10);
        if b.report(3, 10) == Err(XC_LN_GAP_WNO) {
            caught += 1;
        }
    }
    // 2）消费越预算带必拒
    {
        let mut b = ReconBase::new(100);
        if b.report(1, 101) == Err(XC_LN_OVER_BUDGET) {
            caught += 1;
        }
    }
    // 3）封账后上报必拒
    {
        let mut b = ReconBase::new(512);
        let _ = b.report(1, 10);
        let _ = b.seal();
        if b.report(2, 10) == Err(XC_LN_SEALED) {
            caught += 1;
        }
    }
    // 4）双侧行数差异必抓
    {
        let mut b = ReconBase::new(512);
        let _ = b.report(1, 10);
        let side: Vec<(u32, u32)> = vec![];
        if b.verify(&side, 10).is_err() {
            caught += 1;
        }
    }
    // 5）Σ消费≠上报总数必抓
    {
        let mut b = ReconBase::new(512);
        let _ = b.report(1, 10);
        let side: Vec<(u32, u32)> = vec![(1, 10)];
        if b.verify(&side, 99).is_err() {
            caught += 1;
        }
    }
    // 6）双侧一致正例
    {
        let mut b = ReconBase::new(512);
        let _ = b.report(1, 7);
        let side: Vec<(u32, u32)> = vec![(1, 7)];
        if b.verify(&side, 7) == Ok(true) {
            caught += 1;
        }
    }
    // 7）侧账容量满必拒
    {
        let mut b = ReconBase::new(512);
        let mut s = DrainSide::new(1);
        let _ = s.push(&mut b, 1, 1);
        if s.push(&mut b, 2, 1) == Err(XC_LN_FULL) {
            caught += 1;
        }
    }
    (caught, N)
}
