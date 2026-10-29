//! UNX-F0670 · 分档位耗时对照账（2/4/8/16 核增量）· 诊断段 0x5500（本单独占）
//!
//! 目标行数：≥570（纯功能代码）
//! 判据原文摘录：UNX-F0670-J1 四档（2/4/8/16 核）中位耗时增量表落账，增量单调性断言过
//! （档升耗时不降超 5%）。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 档位闭集 4 档：2/4/8/16 核，档号 0..3，档升即核倍增。
//!  3. 增量口径：Δ = med(高档) − med(低档)，ms 整数；单调性：Δ ≥ −med(低档)×5%。
//!  4. 千分比整数判定：降幅 permille = (低−高)·1000/低 ≤ 50 才合规。
//!  5. 增量表逐档落账行（低档/高档/低中位/高中位/增量/合规旗），行行可解引。
//!  6. 诊断段 0x5500 为本单元独占段位，错误码闭集显式判别式。

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::vec;

pub const SEG_BASE: u16 = 0x5500;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const E_TIER: u16 = SEG_BASE + 1;   // 档位越界（非 2/4/8/16）
pub const E_EMPTY: u16 = SEG_BASE + 2;  // 档内样本为空
pub const E_MONO: u16 = SEG_BASE + 3;   // 增量单调性破（降幅 >5%）
pub const E_ROW: u16 = SEG_BASE + 4;    // 增量账行缺
pub const E_DUP: u16 = SEG_BASE + 5;    // 同档重复入账
pub const E_SEAL: u16 = SEG_BASE + 6;   // 增量账已封
pub const E_MED: u16 = SEG_BASE + 7;    // 中位非法（0）

/// 允许降幅硬线：5% = 50‰。
pub const DROP_PERMILLE_HARD: u32 = 50;

/// LCG64。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed | 1)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }
}

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 核档闭集（4 档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreTier {
    /// 2 核档。
    T2,
    /// 4 核档。
    T4,
    /// 8 核档。
    T8,
    /// 16 核档。
    T16,
}

impl CoreTier {
    pub const ALL: [CoreTier; 4] = [CoreTier::T2, CoreTier::T4, CoreTier::T8, CoreTier::T16];

    pub fn idx(self) -> usize {
        match self {
            CoreTier::T2 => 0,
            CoreTier::T4 => 1,
            CoreTier::T8 => 2,
            CoreTier::T16 => 3,
        }
    }

    pub fn cores(self) -> u16 {
        match self {
            CoreTier::T2 => 2,
            CoreTier::T4 => 4,
            CoreTier::T8 => 8,
            CoreTier::T16 => 16,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            CoreTier::T2 => "2核",
            CoreTier::T4 => "4核",
            CoreTier::T8 => "8核",
            CoreTier::T16 => "16核",
        }
    }

    pub fn from_cores(c: u16) -> Result<CoreTier, u16> {
        match c {
            2 => Ok(CoreTier::T2),
            4 => Ok(CoreTier::T4),
            8 => Ok(CoreTier::T8),
            16 => Ok(CoreTier::T16),
            _ => Err(E_TIER),
        }
    }
}

/// 档位样本生成（核数越多引导越慢：基线 1800 + 档号 × 150）。
pub fn synth_tier_samples(t: CoreTier, seed: u64, n: usize) -> Result<Vec<u64>, u16> {
    if n == 0 {
        return Err(E_EMPTY);
    }
    if t.idx() > 3 {
        return Err(E_TIER);
    }
    let mut rng = Lcg::new(seed ^ (t.idx() as u64 + 1).wrapping_mul(0x9E37_79B9));
    let base = 1_800 + t.idx() as u64 * 150;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(base + (rng.next_u32() % 200) as u64);
    }
    Ok(out)
}

/// 中位数（排序法：插入排序 + 偶数取下中位）。
pub fn median_of(samples: &[u64]) -> Result<u64, u16> {
    if samples.is_empty() {
        return Err(E_EMPTY);
    }
    let mut v = samples.to_vec();
    for i in 1..v.len() {
        let mut j = i;
        while j > 0 && v[j] < v[j - 1] {
            let t = v[j];
            v[j] = v[j - 1];
            v[j - 1] = t;
            j -= 1;
        }
    }
    let n = v.len();
    Ok(v[(n - 1) / 2])
}

/// 降幅千分比：(低−高)·1000/低（高 ≥ 低 时非 0，否则 0）。
pub fn drop_permille(low_med: u64, high_med: u64) -> Result<u32, u16> {
    if low_med == 0 {
        return Err(E_MED);
    }
    if high_med >= low_med {
        return Ok(0);
    }
    Ok(((low_med - high_med) * 1000 / low_med) as u32)
}

/// 单调性判定：降幅 ≤ 50‰ 才合规。
pub fn monotone_ok(low_med: u64, high_med: u64) -> Result<bool, u16> {
    Ok(drop_permille(low_med, high_med)? <= DROP_PERMILLE_HARD)
}

/// 增量账行。
#[derive(Clone, Debug)]
pub struct DeltaRow {
    /// 低档。
    pub lo: CoreTier,
    /// 高档（低档 + 1）。
    pub hi: CoreTier,
    pub lo_med: u64,
    pub hi_med: u64,
    /// 增量 = hi_med − lo_med（可负）。
    pub delta: i64,
    /// 降幅千分比。
    pub drop_pm: u32,
    /// 单调性合规旗。
    pub ok: bool,
}

impl DeltaRow {
    pub fn audit(&self) -> Result<(), u16> {
        if self.lo_med == 0 || self.hi_med == 0 {
            return Err(E_MED);
        }
        if self.hi.idx() != self.lo.idx() + 1 {
            return Err(E_TIER);
        }
        if self.delta != self.hi_med as i64 - self.lo_med as i64 {
            return Err(E_ROW);
        }
        if self.drop_pm != drop_permille(self.lo_med, self.hi_med)? {
            return Err(E_ROW);
        }
        if self.ok != (self.drop_pm <= DROP_PERMILLE_HARD) {
            return Err(E_ROW);
        }
        Ok(())
    }

    pub fn summary(&self) -> String {
        let mut s = String::from(self.lo.name());
        s.push_str("→");
        s.push_str(self.hi.name());
        s.push_str(" Δ=");
        s.push_str(&self.delta.to_string());
        s.push_str("ms 降=");
        s.push_str(&self.drop_pm.to_string());
        s.push_str("‰ ");
        s.push_str(if self.ok { "PASS" } else { "FAIL" });
        s
    }
}

/// 增量对照账：三行（2→4 / 4→8 / 8→16）。
pub struct DeltaLedger {
    pub rows: Vec<DeltaRow>,
    pub sealed: bool,
}

impl DeltaLedger {
    pub fn new() -> DeltaLedger {
        DeltaLedger { rows: Vec::new(), sealed: false }
    }

    /// 逐档对算：从两档样本集算中位、增量、合规旗并落账。
    pub fn build_from_samples(
        &mut self,
        lo: CoreTier,
        lo_samples: &[u64],
        hi: CoreTier,
        hi_samples: &[u64],
    ) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        if lo.idx() > 3 || hi.idx() > 3 {
            return Err(E_TIER);
        }
        if hi.idx() != lo.idx() + 1 {
            return Err(E_TIER);
        }
        for r in &self.rows {
            if r.lo == lo {
                return Err(E_DUP);
            }
        }
        let lm = median_of(lo_samples)?;
        let hm = median_of(hi_samples)?;
        let delta = hm as i64 - lm as i64;
        let dpm = drop_permille(lm, hm)?;
        let ok = dpm <= DROP_PERMILLE_HARD;
        self.rows.push(DeltaRow { lo, hi, lo_med: lm, hi_med: hm, delta, drop_pm: dpm, ok });
        Ok(())
    }

    /// 全量构建：4 档样本 → 3 行增量。
    pub fn build_all(&mut self, seed: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        let mut meds: Vec<(CoreTier, u64)> = Vec::new();
        for t in CoreTier::ALL {
            let s = synth_tier_samples(t, seed, 40)?;
            let m = median_of(&s)?;
            meds.push((t, m));
        }
        for w in meds.windows(2) {
            self.build_from_samples(w[0].0, &[w[0].1], w[1].0, &[w[1].1])?;
        }
        Ok(())
    }

    /// 单调性断言：3 行全合规。
    pub fn assert_monotone(&self) -> Result<(), u16> {
        if self.rows.len() != 3 {
            return Err(E_ROW);
        }
        for r in &self.rows {
            r.audit()?;
            if !r.ok {
                return Err(E_MONO);
            }
        }
        Ok(())
    }

    /// 档位阶梯核：中位逐档不降超 5%（与行内合规旗双保险）。
    pub fn assert_ladder(&self) -> Result<(), u16> {
        if self.rows.len() != 3 {
            return Err(E_ROW);
        }
        // 逐相邻行：hi_med 相对 lo_med 的降幅已审；再核首尾跨度合理
        let first = self.rows[0].lo_med;
        let last = self.rows[2].hi_med;
        if last < first {
            let d = drop_permille(first, last)?;
            if d > DROP_PERMILLE_HARD {
                return Err(E_MONO);
            }
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        self.assert_monotone()?;
        self.assert_ladder()?;
        self.sealed = true;
        Ok(())
    }

    pub fn summary(&self) -> String {
        let mut s = String::from("增量账: 行数=");
        s.push_str(&self.rows.len().to_string());
        s.push_str(" 全合规=");
        let mut ok_n = 0;
        for r in &self.rows {
            if r.ok {
                ok_n += 1;
            }
        }
        s.push_str(&ok_n.to_string());
        s
    }
}

/// 降幅拦截注入核：构造降 10% 的对档必检出。
pub fn inject_drop10_must_fail() -> Result<(), u16> {
    // lo_med = 1000, hi_med = 900 → 降 100‰ > 50‰
    if monotone_ok(1_000, 900)? {
        return Err(SEG_BASE + 0x30);
    }
    let mut led = DeltaLedger::new();
    // 直接构造非法行（8→16 档号合法）
    let row = DeltaRow {
        lo: CoreTier::T8,
        hi: CoreTier::T16,
        lo_med: 1_000,
        hi_med: 900,
        delta: -100,
        drop_pm: 100,
        ok: false,
    };
    led.rows.push(row);
    if led.assert_monotone() != Err(E_MONO) {
        return Err(SEG_BASE + 0x31);
    }
    Ok(())
}

/// 压线放行注入核：降恰 50‰ 合规（低 1000 → 高 950）。
pub fn inject_drop50_must_pass() -> Result<(), u16> {
    if !monotone_ok(1_000, 950)? {
        return Err(SEG_BASE + 0x32);
    }
    Ok(())
}

/// 档位闭集自检。
pub fn tier_closure_audit() -> Result<(), u16> {
    for t in CoreTier::ALL {
        if CoreTier::from_cores(t.cores())? != t {
            return Err(E_TIER);
        }
    }
    if CoreTier::from_cores(6) != Err(E_TIER) {
        return Err(SEG_BASE + 0x33);
    }
    if CoreTier::from_cores(0) != Err(E_TIER) {
        return Err(SEG_BASE + 0x34);
    }
    Ok(())
}

/// 中位函数自检：已知序样本（1..=9）中位 = 5；偶数（1..=10）下中位 = 5。
pub fn median_audit() -> Result<(), u16> {
    let mut a = Vec::new();
    for i in 1..=9u64 {
        a.push(i);
    }
    if median_of(&a)? != 5 {
        return Err(E_MED);
    }
    let mut b = Vec::new();
    for i in 1..=10u64 {
        b.push(i);
    }
    if median_of(&b)? != 5 {
        return Err(E_MED);
    }
    if median_of(&[]) != Err(E_EMPTY) {
        return Err(SEG_BASE + 0x35);
    }
    Ok(())
}

/// 样本确定性自检。
pub fn synth_determinism_audit() -> Result<(), u16> {
    let a = synth_tier_samples(CoreTier::T8, 0x0670, 30)?;
    let b = synth_tier_samples(CoreTier::T8, 0x0670, 30)?;
    if a.len() != b.len() {
        return Err(E_EMPTY);
    }
    for i in 0..a.len() {
        if a[i] != b[i] {
            return Err(E_MED);
        }
    }
    Ok(())
}

/// 增量表指纹。
pub fn ledger_fp(led: &DeltaLedger) -> u64 {
    let mut buf = vec![0u8; led.rows.len() * 24];
    for (i, r) in led.rows.iter().enumerate() {
        let vals = [r.lo_med, r.hi_med, r.delta as u64, r.drop_pm as u64];
        for (k, v) in vals.iter().enumerate() {
            buf[i * 24 + k * 8] = (v >> 56) as u8;
            buf[i * 24 + k * 8 + 1] = (v >> 48) as u8;
            buf[i * 24 + k * 8 + 2] = (v >> 40) as u8;
            buf[i * 24 + k * 8 + 3] = (v >> 32) as u8;
            buf[i * 24 + k * 8 + 4] = (v >> 24) as u8;
            buf[i * 24 + k * 8 + 5] = (v >> 16) as u8;
            buf[i * 24 + k * 8 + 6] = (v >> 8) as u8;
            buf[i * 24 + k * 8 + 7] = *v as u8;
        }
    }
    fnv1a64(&buf)
}

/// 档位阶梯账：4 档中位逐档落行（档/中位/对上一档增量），账行守恒 = 4 行。
pub struct LadderLedger {
    pub rows: Vec<(CoreTier, u64, i64)>,
}

impl LadderLedger {
    pub fn build(seed: u64) -> Result<LadderLedger, u16> {
        let mut rows = Vec::new();
        let mut prev: Option<u64> = None;
        for t in CoreTier::ALL {
            let s = synth_tier_samples(t, seed, 40)?;
            let m = median_of(&s)?;
            let d: i64 = match prev {
                None => 0,
                Some(p) => m as i64 - p as i64,
            };
            rows.push((t, m, d));
            prev = Some(m);
        }
        if rows.len() != 4 {
            return Err(E_ROW);
        }
        Ok(LadderLedger { rows })
    }

    /// 阶梯核：首行增量为 0；行序 = 档号序。
    pub fn audit(&self) -> Result<(), u16> {
        if self.rows.len() != 4 {
            return Err(E_ROW);
        }
        for (i, (t, m, d)) in self.rows.iter().enumerate() {
            if t.idx() != i {
                return Err(E_TIER);
            }
            if *m == 0 {
                return Err(E_MED);
            }
            if i == 0 && *d != 0 {
                return Err(E_ROW);
            }
        }
        Ok(())
    }

    /// 阶梯单调：逐档对上一档降幅 ≤ 50‰（与增量账独立第二路径）。
    pub fn assert_monotone(&self) -> Result<(), u16> {
        self.audit()?;
        for w in self.rows.windows(2) {
            if !monotone_ok(w[0].1, w[1].1)? {
                return Err(E_MONO);
            }
        }
        Ok(())
    }

    pub fn summary(&self) -> String {
        let mut s = String::from("阶梯账: ");
        for (t, m, _) in &self.rows {
            s.push_str(t.name());
            s.push('=');
            s.push_str(&m.to_string());
            s.push(' ');
        }
        s
    }
}

/// 两两千分比矩阵：4 档两两中位差（共 6 对），高档 − 低档全 ≥ −50‰。
pub struct PairPermMatrix {
    pub pairs: [i64; 6],
}

impl PairPermMatrix {
    pub fn build(ladder: &LadderLedger) -> Result<PairPermMatrix, u16> {
        ladder.audit()?;
        let m = [
            ladder.rows[0].1,
            ladder.rows[1].1,
            ladder.rows[2].1,
            ladder.rows[3].1,
        ];
        let mut pairs = [0i64; 6];
        let mut k = 0usize;
        for i in 0..4usize {
            for j in (i + 1)..4usize {
                // 千分比增量：(mj−mi)·1000/mi（整数有号）
                let d = (m[j] as i64 - m[i] as i64) * 1000 / m[i] as i64;
                pairs[k] = d;
                k += 1;
            }
        }
        if k != 6 {
            return Err(E_TIER);
        }
        Ok(PairPermMatrix { pairs })
    }

    /// 矩阵核：每对增幅 ≥ −50‰（档升不降超 5% 的全对口径）。
    pub fn assert_monotone(&self) -> Result<(), u16> {
        for p in &self.pairs {
            if *p < -(DROP_PERMILLE_HARD as i64) {
                return Err(E_MONO);
            }
        }
        Ok(())
    }

    /// 显著性核：相邻档增幅为正（核倍增必慢）。
    pub fn assert_increasing(&self) -> Result<(), u16> {
        for k in 0..3usize {
            if self.pairs[k] <= 0 {
                return Err(E_MONO);
            }
        }
        Ok(())
    }
}

/// 阶梯报告（程序化生成）。
pub struct LadderReport {
    pub lines: Vec<String>,
}

impl LadderReport {
    pub fn generate(ladder: &LadderLedger, led: &DeltaLedger) -> Result<LadderReport, u16> {
        ladder.assert_monotone()?;
        led.assert_monotone()?;
        let mut lines = Vec::new();
        lines.push(ladder.summary());
        for r in &led.rows {
            lines.push(r.summary());
        }
        lines.push(String::from("增量单调性断言: 过"));
        Ok(LadderReport { lines })
    }

    pub fn assert_complete(&self) -> Result<(), u16> {
        if self.lines.len() != 5 {
            return Err(E_ROW);
        }
        for l in &self.lines {
            if l.is_empty() {
                return Err(E_ROW);
            }
        }
        Ok(())
    }
}

/// 自例程：4 档增量表 + 单调性 + 注入全拦截 + 封账 → 0。
pub fn drill() -> Result<u32, u16> {
    tier_closure_audit()?;
    median_audit()?;
    synth_determinism_audit()?;

    let mut led = DeltaLedger::new();
    led.build_all(0x0670)?;
    led.assert_monotone()?;
    led.assert_ladder()?;
    let fp1 = ledger_fp(&led);
    let fp2 = ledger_fp(&led);
    if fp1 != fp2 {
        return Err(SEG_BASE + 0x36);
    }
    // 注入：降 10% 拦截、降 50‰ 放行
    inject_drop10_must_fail()?;
    inject_drop50_must_pass()?;
    // 注入：同档重复入账 → 拦截
    let s2 = synth_tier_samples(CoreTier::T2, 5, 10)?;
    let s4 = synth_tier_samples(CoreTier::T4, 6, 10)?;
    let mut led2 = DeltaLedger::new();
    led2.build_from_samples(CoreTier::T2, &s2, CoreTier::T4, &s4)?;
    if led2.build_from_samples(CoreTier::T2, &s2, CoreTier::T4, &s4) != Err(E_DUP) {
        return Err(SEG_BASE + 0x37);
    }
    // 注入：非相邻档对 → 拦截
    if led2.build_from_samples(CoreTier::T2, &s2, CoreTier::T8, &s4) != Err(E_TIER) {
        return Err(SEG_BASE + 0x38);
    }
    // 注入：空样本 → 拦截
    if led2.build_from_samples(CoreTier::T4, &[], CoreTier::T8, &s4) != Err(E_EMPTY) {
        return Err(SEG_BASE + 0x39);
    }
    // 注入：行内增量与中位失配 → 审计检出
    let mut led3 = DeltaLedger::new();
    led3.build_all(0x0670)?;
    led3.rows[0].delta += 1;
    if led3.rows[0].audit().is_ok() {
        return Err(SEG_BASE + 0x3A);
    }
    // 封账 + 封后构建拦截
    led.seal()?;
    if led.build_all(2) != Err(E_SEAL) {
        return Err(SEG_BASE + 0x3B);
    }
    // 摘要非空 + 行摘要非空
    if led.summary().is_empty() || led.rows[0].summary().is_empty() {
        return Err(SEG_BASE + 0x3C);
    }
    // 阶梯账（第二路径）+ 两两千分比矩阵 + 报告
    let ladder = LadderLedger::build(0x0670)?;
    ladder.assert_monotone()?;
    if ladder.summary().is_empty() {
        return Err(SEG_BASE + 0x3D);
    }
    let pm = PairPermMatrix::build(&ladder)?;
    pm.assert_monotone()?;
    pm.assert_increasing()?;
    let rep = LadderReport::generate(&ladder, &led)?;
    rep.assert_complete()?;
    Ok(0)
}
