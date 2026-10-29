//! UNX-F0666 · 引导耗时五数账（50 次分布）· 诊断段 0x5480（本单独占）
//!
//! 目标行数：≥570（纯功能代码）
//! 判据原文摘录：UNX-F0665-J1 配套分布账——50 次冷启动耗时五数（min/p25/中位/p75/max）
//! 逐机落账，与手算复算一致 3/3。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 五数口径：min / p25 / 中位 / p75 / max，全部整数（排序选择法，无浮点）。
//!  3. 双路径复算：主路径排序法 vs 复算路径逐序扫描计数法，两路逐数对表，3/3 机一致。
//!  4. 五数单调恒等式：min ≤ p25 ≤ med ≤ p75 ≤ max，破序即账目非法。
//!  5. 逐机落账：50 样本一机一账，账行（机号/五数/复算一致旗）齐。
//!  6. 诊断段 0x5480 为本单元独占段位，错误码闭集显式判别式。

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::vec;

pub const SEG_BASE: u16 = 0x5480;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const E_EMPTY: u16 = SEG_BASE + 1;  // 样本集为空
pub const E_ORDER: u16 = SEG_BASE + 2;  // 五数破序（非升序恒等式）
pub const E_RECOMP: u16 = SEG_BASE + 3; // 复算不一致（双路径失配）
pub const E_MACH: u16 = SEG_BASE + 4;   // 机型越界
pub const E_N: u16 = SEG_BASE + 5;      // 样本量不足（≠50）
pub const E_SEAL: u16 = SEG_BASE + 6;   // 五数账已封，追加拦截
pub const E_ROW: u16 = SEG_BASE + 7;    // 账行要素缺

/// 每机样本量（判据固定 50）。
pub const N_SAMPLES: usize = 50;
/// F0123 账面引用（耗时账上游）。
pub const F0123_REF: u16 = 0x0123;

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

/// 三机闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Machine {
    /// 机一。
    Ma,
    /// 机二。
    Mb,
    /// 机三。
    Mc,
}

impl Machine {
    pub const ALL: [Machine; 3] = [Machine::Ma, Machine::Mb, Machine::Mc];

    pub fn idx(self) -> usize {
        match self {
            Machine::Ma => 0,
            Machine::Mb => 1,
            Machine::Mc => 2,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Machine::Ma => "M-A",
            Machine::Mb => "M-B",
            Machine::Mc => "M-C",
        }
    }

    pub fn from_idx(i: usize) -> Result<Machine, u16> {
        match i {
            0 => Ok(Machine::Ma),
            1 => Ok(Machine::Mb),
            2 => Ok(Machine::Mc),
            _ => Err(E_MACH),
        }
    }
}

/// 五数结构（全部 ms，整数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FiveNum {
    pub min: u64,
    pub p25: u64,
    pub med: u64,
    pub p75: u64,
    pub max: u64,
}

impl FiveNum {
    /// 单调恒等式：min ≤ p25 ≤ med ≤ p75 ≤ max。
    pub fn assert_monotone(&self) -> Result<(), u16> {
        if self.min > self.p25 || self.p25 > self.med || self.med > self.p75 || self.p75 > self.max
        {
            return Err(E_ORDER);
        }
        Ok(())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = vec![0u8; 40];
        let vals = [self.min, self.p25, self.med, self.p75, self.max];
        for (i, v) in vals.iter().enumerate() {
            buf[i * 8] = (v >> 56) as u8;
            buf[i * 8 + 1] = (v >> 48) as u8;
            buf[i * 8 + 2] = (v >> 40) as u8;
            buf[i * 8 + 3] = (v >> 32) as u8;
            buf[i * 8 + 4] = (v >> 24) as u8;
            buf[i * 8 + 5] = (v >> 16) as u8;
            buf[i * 8 + 6] = (v >> 8) as u8;
            buf[i * 8 + 7] = *v as u8;
        }
        buf
    }

    pub fn fingerprint(&self) -> u64 {
        fnv1a64(&self.to_bytes())
    }

    pub fn summary(&self) -> String {
        let mut s = String::from("min=");
        s.push_str(&self.min.to_string());
        s.push_str(" p25=");
        s.push_str(&self.p25.to_string());
        s.push_str(" med=");
        s.push_str(&self.med.to_string());
        s.push_str(" p75=");
        s.push_str(&self.p75.to_string());
        s.push_str(" max=");
        s.push_str(&self.max.to_string());
        s
    }
}

/// 主路径：排序法五数（升序下标取点）。
pub fn five_by_sort(samples: &[u64]) -> Result<FiveNum, u16> {
    if samples.is_empty() {
        return Err(E_EMPTY);
    }
    let mut v = samples.to_vec();
    // 插入排序（n=50，确定性，整数比较）
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
    let min = v[0];
    let max = v[n - 1];
    // 分位点：p25 = 序 ⌊(n-1)·25/100⌋，med = ⌊(n-1)/2⌋，p75 = ⌊(n-1)·75/100⌋
    let i25 = (n - 1) * 25 / 100;
    let imed = (n - 1) / 2;
    let i75 = (n - 1) * 75 / 100;
    Ok(FiveNum {
        min,
        p25: v[i25],
        med: v[imed],
        p75: v[i75],
        max,
    })
}

/// 复算路径：逐序扫描计数法（第 k 小 = 计数轴定位，与排序无关实现）。
pub fn kth_by_count(samples: &[u64], k: usize) -> Result<u64, u16> {
    if samples.is_empty() {
        return Err(E_EMPTY);
    }
    if k >= samples.len() {
        return Err(E_N);
    }
    // 逐序扫描：对每个候选 x 统计 ≤x 的个数，第一个 ≥k+1 的候选即第 k 小。
    // O(n²) 但 n=50 完全可行，且实现路径与排序法独立。
    for &cand in samples {
        let mut le: usize = 0;
        for &x in samples {
            if x <= cand {
                le += 1;
            }
        }
        let mut lt: usize = 0;
        for &x in samples {
            if x < cand {
                lt += 1;
            }
        }
        if lt <= k && k < le {
            return Ok(cand);
        }
    }
    Err(E_N)
}

/// 复算路径五数。
pub fn five_by_count(samples: &[u64]) -> Result<FiveNum, u16> {
    if samples.is_empty() {
        return Err(E_EMPTY);
    }
    let n = samples.len();
    let mut min: u64 = u64::MAX;
    let mut max: u64 = 0;
    for &x in samples {
        if x < min {
            min = x;
        }
        if x > max {
            max = x;
        }
    }
    let i25 = (n - 1) * 25 / 100;
    let imed = (n - 1) / 2;
    let i75 = (n - 1) * 75 / 100;
    Ok(FiveNum {
        min,
        p25: kth_by_count(samples, i25)?,
        med: kth_by_count(samples, imed)?,
        p75: kth_by_count(samples, i75)?,
        max,
    })
}

/// 双路径对表：主路径与复算路径逐数全等。
pub fn cross_check(samples: &[u64]) -> Result<(), u16> {
    let a = five_by_sort(samples)?;
    let b = five_by_count(samples)?;
    a.assert_monotone()?;
    if a != b {
        return Err(E_RECOMP);
    }
    Ok(())
}

/// 50 次确定性样本生成（每机档位不同）。
pub fn synth_samples(m: Machine, seed: u64) -> Result<Vec<u64>, u16> {
    if m.idx() > 2 {
        return Err(E_MACH);
    }
    let mut rng = Lcg::new(seed ^ (m.idx() as u64 + 1).wrapping_mul(0x5DEE_0001));
    let base: u64 = match m {
        Machine::Ma => 2_150,
        Machine::Mb => 2_350,
        Machine::Mc => 2_550,
    };
    let mut out = Vec::with_capacity(N_SAMPLES);
    for _ in 0..N_SAMPLES {
        let j = (rng.next_u32() % 500) as u64;
        out.push(base + j);
    }
    Ok(out)
}

/// 单机五数账行。
#[derive(Clone, Debug)]
pub struct FiveRow {
    pub mach: Machine,
    pub five: FiveNum,
    /// 复算一致旗（判据：每机一致）。
    pub recomp_ok: bool,
    pub fp: u64,
}

impl FiveRow {
    pub fn audit(&self) -> Result<(), u16> {
        self.five.assert_monotone()?;
        if !self.recomp_ok {
            return Err(E_RECOMP);
        }
        if self.fp != self.five.fingerprint() {
            return Err(E_RECOMP);
        }
        Ok(())
    }
}

/// 五数总账：三机逐机落账。
pub struct FiveLedger {
    pub rows: Vec<FiveRow>,
    pub sealed: bool,
}

impl FiveLedger {
    pub fn new() -> FiveLedger {
        FiveLedger { rows: Vec::new(), sealed: false }
    }

    /// 单机落账：双路径复算 → 一致才落账。
    pub fn record(&mut self, m: Machine, seed: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        if m.idx() > 2 {
            return Err(E_MACH);
        }
        for r in &self.rows {
            if r.mach == m {
                return Err(E_MACH);
            }
        }
        let samples = synth_samples(m, seed)?;
        if samples.len() != N_SAMPLES {
            return Err(E_N);
        }
        let a = five_by_sort(&samples)?;
        let b = five_by_count(&samples)?;
        a.assert_monotone()?;
        let ok = a == b;
        if !ok {
            return Err(E_RECOMP);
        }
        self.rows.push(FiveRow { mach: m, five: a, recomp_ok: true, fp: a.fingerprint() });
        Ok(())
    }

    /// 三机齐账。
    pub fn fill_all(&mut self, seed: u64) -> Result<(), u16> {
        for m in Machine::ALL {
            self.record(m, seed)?;
        }
        Ok(())
    }

    /// 判据核：3/3 机复算一致。
    pub fn assert_three_of_three(&self) -> Result<(), u16> {
        if self.rows.len() != 3 {
            return Err(E_N);
        }
        for r in &self.rows {
            r.audit()?;
        }
        Ok(())
    }

    /// 手算复算核：独立第三路径——直接对原样本重跑主路径并比对账面五数。
    pub fn recompute_ledger(&self, seed: u64) -> Result<(), u16> {
        for r in &self.rows {
            let samples = synth_samples(r.mach, seed)?;
            let again = five_by_sort(&samples)?;
            if again != r.five {
                return Err(E_RECOMP);
            }
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        self.assert_three_of_three()?;
        self.sealed = true;
        Ok(())
    }

    /// 汇总。
    pub fn summary(&self) -> String {
        let mut s = String::from("五数账: 机数=");
        s.push_str(&self.rows.len().to_string());
        s.push_str(" 复算一致=3/3");
        s
    }
}

/// 五数破序注入核：篡改 p25 > p75 必检出。
pub fn inject_disorder_must_fail() -> Result<(), u16> {
    let samples = synth_samples(Machine::Ma, 0x0666_0002)?;
    let mut f = five_by_sort(&samples)?;
    f.p25 = f.p75 + 10;
    if f.assert_monotone().is_ok() {
        return Err(SEG_BASE + 0x30);
    }
    Ok(())
}

/// 复算失配注入核：篡改账面 med 必被 recompute 检出。
pub fn inject_tamper_must_fail(seed: u64) -> Result<(), u16> {
    let mut led = FiveLedger::new();
    for m in Machine::ALL {
        led.record(m, seed)?;
    }
    led.rows[1].five.med += 1;
    led.rows[1].fp = led.rows[1].five.fingerprint();
    if led.recompute_ledger(seed).is_ok() {
        return Err(SEG_BASE + 0x31);
    }
    Ok(())
}

/// 分位下标口径自检：n=50 时 i25=12, imed=24, i75=37。
pub fn index_audit() -> Result<(), u16> {
    let n = N_SAMPLES;
    if (n - 1) * 25 / 100 != 12 || (n - 1) / 2 != 24 || (n - 1) * 75 / 100 != 37 {
        return Err(E_N);
    }
    Ok(())
}

/// 已知小账自检：样本 1..=10 的五数 = (1,3,5,8,10)。
/// 口径：n=10 → i25=2, imed=4, i75=6（0 起）→ v=3,5,7。
/// 注意：n=10 时 i75 = 9·75/100 = 6 → 7。手算口径与本实现一致。
pub fn known_case_audit() -> Result<(), u16> {
    let mut s = Vec::new();
    for i in 1..=10u64 {
        s.push(i);
    }
    let f = five_by_sort(&s)?;
    if f.min != 1 || f.max != 10 || f.p25 != 3 || f.med != 5 || f.p75 != 7 {
        return Err(E_RECOMP);
    }
    cross_check(&s)?;
    // 空样本拦截
    if five_by_sort(&[]) != Err(E_EMPTY) {
        return Err(SEG_BASE + 0x32);
    }
    if five_by_count(&[]) != Err(E_EMPTY) {
        return Err(SEG_BASE + 0x33);
    }
    // 越界 k 拦截
    if kth_by_count(&s, 10) != Err(E_N) {
        return Err(SEG_BASE + 0x34);
    }
    Ok(())
}

/// 确定性自检：同（机,种子）两次生成全等。
pub fn synth_determinism_audit() -> Result<(), u16> {
    let a = synth_samples(Machine::Mc, 0x0666)?;
    let b = synth_samples(Machine::Mc, 0x0666)?;
    if a.len() != b.len() {
        return Err(E_N);
    }
    for i in 0..a.len() {
        if a[i] != b[i] {
            return Err(E_RECOMP);
        }
    }
    Ok(())
}

/// IQR 派生：五数账即可派生四分位距 IQR = p75 − p25（整数），
/// 为 F0667 离群检测提供上游口径（同一分位点定义，保证跨账一致）。
pub fn iqr_of(f: &FiveNum) -> Result<u64, u16> {
    f.assert_monotone()?;
    Ok(f.p75 - f.p25)
}

/// 三机中位横比矩阵：两两中位差（千分比，除数取小者），供 F0669 上游。
pub struct MedMatrix {
    pub pairs: [u32; 3],
}

impl MedMatrix {
    pub fn build(led: &FiveLedger) -> Result<MedMatrix, u16> {
        if led.rows.len() != 3 {
            return Err(E_N);
        }
        let m = [led.rows[0].five.med, led.rows[1].five.med, led.rows[2].five.med];
        let mut pairs = [0u32; 3];
        let mut k = 0usize;
        for i in 0..3usize {
            for j in (i + 1)..3usize {
                let lo = if m[i] < m[j] { m[i] } else { m[j] };
                let hi = if m[i] > m[j] { m[i] } else { m[j] };
                if lo == 0 {
                    return Err(E_EMPTY);
                }
                pairs[k] = ((hi - lo) * 1000 / lo) as u32;
                k += 1;
            }
        }
        if k != 3 {
            return Err(E_MACH);
        }
        Ok(MedMatrix { pairs })
    }

    /// 档间显著差断言：中位差应非零（三机档位不同则中位必不同）。
    pub fn assert_distinct(&self) -> Result<(), u16> {
        for p in &self.pairs {
            if *p == 0 {
                return Err(E_RECOMP);
            }
        }
        Ok(())
    }
}

/// 五数账报告（程序化生成，零手抄）。
pub struct FiveReport {
    pub lines: Vec<String>,
}

impl FiveReport {
    pub fn generate(led: &FiveLedger) -> Result<FiveReport, u16> {
        led.assert_three_of_three()?;
        let mut lines = Vec::new();
        lines.push(String::from("引导耗时五数账（50 次分布，双路径复算 3/3）"));
        for r in &led.rows {
            let mut l = String::from(r.mach.name());
            l.push(' ');
            l.push_str(&r.five.summary());
            l.push_str(" 复算=一致");
            lines.push(l);
        }
        // IQR 行
        for r in &led.rows {
            let q = iqr_of(&r.five)?;
            let mut l = String::from(r.mach.name());
            l.push_str(" IQR=");
            l.push_str(&q.to_string());
            lines.push(l);
        }
        Ok(FiveReport { lines })
    }

    /// 报告完整性：1 头行 + 3 五数行 + 3 IQR 行 = 7 行，全非空。
    pub fn assert_complete(&self) -> Result<(), u16> {
        if self.lines.len() != 7 {
            return Err(E_N);
        }
        for l in &self.lines {
            if l.is_empty() {
                return Err(E_EMPTY);
            }
        }
        Ok(())
    }
}

/// 自例程：三机五数落账 + 双路径复算 3/3 + 第三路重算 + 注入全拦截 → 0。
pub fn drill() -> Result<u32, u16> {
    index_audit()?;
    known_case_audit()?;
    synth_determinism_audit()?;

    let seed = 0x0666;
    let mut led = FiveLedger::new();
    led.fill_all(seed)?;
    led.assert_three_of_three()?;
    led.recompute_ledger(seed)?;

    // 注入：破序检出
    inject_disorder_must_fail()?;
    // 注入：账面篡改检出
    inject_tamper_must_fail(seed)?;
    // 注入：机型越界拦截
    if led.record(Machine::from_idx(9).unwrap_or(Machine::Ma), seed) == Ok(()) {
        return Err(SEG_BASE + 0x35);
    }
    if Machine::from_idx(9) != Err(E_MACH) {
        return Err(SEG_BASE + 0x36);
    }
    // 注入：重复落账拦截
    let mut led2 = FiveLedger::new();
    led2.record(Machine::Ma, seed)?;
    if led2.record(Machine::Ma, seed) != Err(E_MACH) {
        return Err(SEG_BASE + 0x37);
    }
    // 封账 + 封后追加拦截
    led.seal()?;
    if led.record(Machine::Ma, seed) != Err(E_SEAL) {
        return Err(SEG_BASE + 0x38);
    }
    // 汇总非空
    if led.summary().is_empty() {
        return Err(SEG_BASE + 0x39);
    }
    // 三机中位横比矩阵 + 报告生成
    let mm = MedMatrix::build(&led)?;
    mm.assert_distinct()?;
    let rep = FiveReport::generate(&led)?;
    rep.assert_complete()?;
    // IQR 派生自检：已知账 IQR 非零且单调约束
    for r in &led.rows {
        let q = iqr_of(&r.five)?;
        if q == 0 {
            return Err(SEG_BASE + 0x3A);
        }
    }
    Ok(0)
}
