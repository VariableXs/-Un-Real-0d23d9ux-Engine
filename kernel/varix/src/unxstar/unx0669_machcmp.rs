//! UNX-F0669 · 分机型耗时对照账（三机横比）· 诊断段 0x54e0（本单独占）
//!
//! 目标行数：≥580（纯功能代码）
//! 判据原文摘录：UNX-F0669-J1 三机五数横比表落账（3 行 × 5 数列 + 差异注记列），
//! 同镜像同档口径声明零歧义。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 横比表结构：3 行（机）× 5 列（五数）+ 差异注记列，行行列列全落账。
//!  3. 差异注记：以基准机（M-A）为分母的千分比整数，零浮点。
//!  4. 口径声明三查（同镜像哈希/同核档/同采集器版本）不齐 → 拒比（零歧义）。
//!  5. 横比守恒：每行五数与上游五数账逐数可解引对表。
//!  6. 诊断段 0x54e0 为本单元独占段位，错误码闭集显式判别式。

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::vec;

pub const SEG_BASE: u16 = 0x54e0;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const E_CAL: u16 = SEG_BASE + 1;    // 口径声明不齐（拒比）
pub const E_ROW: u16 = SEG_BASE + 2;    // 横比表行缺
pub const E_COL: u16 = SEG_BASE + 3;    // 五数列缺/值非法
pub const E_NOTE: u16 = SEG_BASE + 4;   // 差异注记列缺/算错
pub const E_SEAL: u16 = SEG_BASE + 5;   // 对照账已封
pub const E_MACH: u16 = SEG_BASE + 6;   // 机型越界
pub const E_DUP: u16 = SEG_BASE + 7;    // 同机重复入表

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
    /// 机一（基准机）。
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
}

/// 五数向量（列序固定：min/p25/med/p75/max）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FiveVec {
    pub v: [u64; 5],
}

impl FiveVec {
    /// 列单调：min ≤ p25 ≤ med ≤ p75 ≤ max。
    pub fn assert_monotone(&self) -> Result<(), u16> {
        for i in 0..4 {
            if self.v[i] > self.v[i + 1] {
                return Err(E_COL);
            }
        }
        if self.v[0] == 0 {
            return Err(E_COL);
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf = vec![0u8; 40];
        for (i, x) in self.v.iter().enumerate() {
            buf[i * 8] = (x >> 56) as u8;
            buf[i * 8 + 1] = (x >> 48) as u8;
            buf[i * 8 + 2] = (x >> 40) as u8;
            buf[i * 8 + 3] = (x >> 32) as u8;
            buf[i * 8 + 4] = (x >> 24) as u8;
            buf[i * 8 + 5] = (x >> 16) as u8;
            buf[i * 8 + 6] = (x >> 8) as u8;
            buf[i * 8 + 7] = *x as u8;
        }
        fnv1a64(&buf)
    }

    /// 与上游五数账逐数对表。
    pub fn diff_from(&self, other: &FiveVec) -> Vec<usize> {
        let mut d = Vec::new();
        for i in 0..5 {
            if self.v[i] != other.v[i] {
                d.push(i);
            }
        }
        d
    }
}

/// 口径声明：三查（镜像哈希/核档/采集器版本），全齐才可横比。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caliper {
    /// 镜像哈希（同镜像比对前提）。
    pub mirror_hash: u64,
    /// 核档（2/4/8/16）。
    pub core_tier: u8,
    /// 采集器版本号。
    pub probe_ver: u16,
}

impl Caliper {
    /// 三查全同判定：任一不同 → 口径歧义，拒比。
    pub fn same(&self, other: &Caliper) -> bool {
        self.mirror_hash == other.mirror_hash
            && self.core_tier == other.core_tier
            && self.probe_ver == other.probe_ver
    }

    /// 口径合法性：镜像哈希非零、核档 ∈ {2,4,8,16}、版本非零。
    pub fn audit(&self) -> Result<(), u16> {
        if self.mirror_hash == 0 || self.probe_ver == 0 {
            return Err(E_CAL);
        }
        if self.core_tier != 2 && self.core_tier != 4 && self.core_tier != 8 && self.core_tier != 16
        {
            return Err(E_CAL);
        }
        Ok(())
    }
}

/// 差异注记：以基准机同列值 If 为分母的千分比。
pub fn delta_permille(x: u64, base: u64) -> Result<u32, u16> {
    if base == 0 {
        return Err(E_NOTE);
    }
    if x >= base {
        Ok(((x - base) * 1000 / base) as u32)
    } else {
        Ok(((base - x) * 1000 / base) as u32)
    }
}

/// 横比行：机型 + 五数列 + 差异注记列（5 个千分比）。
#[derive(Clone, Debug)]
pub struct CmpRow {
    pub mach: Machine,
    pub five: FiveVec,
    /// 差异注记列（对基准机，基准机自身全 0）。
    pub notes: [u32; 5],
}

impl CmpRow {
    pub fn audit(&self) -> Result<(), u16> {
        self.five.assert_monotone()?;
        Ok(())
    }

    pub fn summary(&self) -> String {
        let mut s = String::from(self.mach.name());
        s.push_str(" [");
        for i in 0..5 {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&self.five.v[i].to_string());
        }
        s.push_str("] Δ=");
        for i in 0..5 {
            if i > 0 {
                s.push('/');
            }
            s.push_str(&self.notes[i].to_string());
        }
        s.push('‰');
        s
    }
}

/// 五数确定性生成（每机档位不同，口径内）。
pub fn synth_five(m: Machine, seed: u64) -> Result<FiveVec, u16> {
    let mut rng = Lcg::new(seed ^ (m.idx() as u64 + 1).wrapping_mul(0x2545_0001));
    let base: u64 = match m {
        Machine::Ma => 2_000,
        Machine::Mb => 2_200,
        Machine::Mc => 2_450,
    };
    let min = base + (rng.next_u32() % 50) as u64;
    let p25 = min + 60 + (rng.next_u32() % 40) as u64;
    let med = p25 + 70 + (rng.next_u32() % 40) as u64;
    let p75 = med + 80 + (rng.next_u32() % 40) as u64;
    let max = p75 + 90 + (rng.next_u32() % 40) as u64;
    Ok(FiveVec { v: [min, p25, med, p75, max] })
}

/// 三机横比账。
pub struct CrossLedger {
    /// 口径声明（入表即锁）。
    pub caliper: Caliper,
    pub rows: Vec<CmpRow>,
    pub sealed: bool,
}

impl CrossLedger {
    pub fn new(caliper: Caliper) -> Result<CrossLedger, u16> {
        caliper.audit()?;
        Ok(CrossLedger { caliper, rows: Vec::new(), sealed: false })
    }

    /// 入表：口径必须与账面口径全同（零歧义），同机重复拦截。
    pub fn enroll(&mut self, m: Machine, seed: u64, cal: &Caliper) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        if !self.caliper.same(cal) {
            return Err(E_CAL);
        }
        for r in &self.rows {
            if r.mach == m {
                return Err(E_DUP);
            }
        }
        let five = synth_five(m, seed)?;
        five.assert_monotone()?;
        self.rows.push(CmpRow { mach: m, five, notes: [0; 5] });
        Ok(())
    }

    /// 差异注记列回填：以行 0（基准机）为分母，逐列算千分比。
    pub fn fill_notes(&mut self) -> Result<(), u16> {
        if self.rows.len() != 3 {
            return Err(E_ROW);
        }
        let base = self.rows[0].five;
        base.assert_monotone()?;
        let mut all_notes: [[u32; 5]; 3] = [[0; 5]; 3];
        for (i, r) in self.rows.iter().enumerate() {
            for c in 0..5 {
                all_notes[i][c] = delta_permille(r.five.v[c], base.v[c])?;
            }
        }
        for (i, r) in self.rows.iter_mut().enumerate() {
            r.notes = all_notes[i];
        }
        Ok(())
    }

    /// 注记核：基准机注记全 0；其余机至少一列非 0（档位必异）。
    pub fn assert_notes(&self) -> Result<(), u16> {
        if self.rows.is_empty() {
            return Err(E_ROW);
        }
        for c in 0..5 {
            if self.rows[0].notes[c] != 0 {
                return Err(E_NOTE);
            }
        }
        for r in self.rows.iter().skip(1) {
            let mut any = false;
            for c in 0..5 {
                if r.notes[c] > 0 {
                    any = true;
                }
            }
            if !any {
                return Err(E_NOTE);
            }
        }
        Ok(())
    }

    /// 三机齐表核。
    pub fn assert_full(&self) -> Result<(), u16> {
        if self.rows.len() != 3 {
            return Err(E_ROW);
        }
        for (i, r) in self.rows.iter().enumerate() {
            if r.mach.idx() != i {
                return Err(E_ROW);
            }
            r.audit()?;
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        self.assert_full()?;
        self.assert_notes()?;
        self.sealed = true;
        Ok(())
    }

    /// 表摘要。
    pub fn summary(&self) -> String {
        let mut s = String::from("三机横比: 行数=");
        s.push_str(&self.rows.len().to_string());
        s.push_str(" 口径: 镜像=");
        s.push_str(&(self.caliper.mirror_hash % 100_000).to_string());
        s.push_str(" 核档=");
        s.push_str(&self.caliper.core_tier.to_string());
        s
    }
}

/// 口径歧义注入核：任一查不同 → 拒比。
pub fn inject_caliber_mismatch_must_reject() -> Result<(), u16> {
    let base = Caliper { mirror_hash: 0xABCD, core_tier: 4, probe_ver: 12 };
    let bad_mirror = Caliper { mirror_hash: 0xABCE, core_tier: 4, probe_ver: 12 };
    let bad_tier = Caliper { mirror_hash: 0xABCD, core_tier: 8, probe_ver: 12 };
    let bad_probe = Caliper { mirror_hash: 0xABCD, core_tier: 4, probe_ver: 13 };
    if base.same(&bad_mirror) || base.same(&bad_tier) || base.same(&bad_probe) {
        return Err(E_CAL);
    }
    // 基准自身恒同
    if !base.same(&base) {
        return Err(E_CAL);
    }
    Ok(())
}

/// 注记算术自检：已知对（x=1100, base=1000）→ 100‰；(900,1000) → 100‰。
pub fn delta_audit() -> Result<(), u16> {
    if delta_permille(1_100, 1_000)? != 100 {
        return Err(E_NOTE);
    }
    if delta_permille(900, 1_000)? != 100 {
        return Err(E_NOTE);
    }
    if delta_permille(1_000, 1_000)? != 0 {
        return Err(E_NOTE);
    }
    if delta_permille(5, 0) != Err(E_NOTE) {
        return Err(SEG_BASE + 0x30);
    }
    Ok(())
}

/// 五数生成确定性自检。
pub fn synth_determinism_audit() -> Result<(), u16> {
    let a = synth_five(Machine::Mb, 0x0669)?;
    let b = synth_five(Machine::Mb, 0x0669)?;
    if a != b {
        return Err(E_ROW);
    }
    a.assert_monotone()?;
    Ok(())
}

/// 上游对表注入核：行五数被篡改 → diff_from 非空可检出。
pub fn inject_tamper_must_detect() -> Result<(), u16> {
    let x = synth_five(Machine::Ma, 7)?;
    let mut y = x;
    y.v[2] += 1;
    let d = x.diff_from(&y);
    if d.len() != 1 || d[0] != 2 {
        return Err(E_ROW);
    }
    Ok(())
}

/// 列极值审计：五数每列取三机最大/最小，极值必须落在基准机或最慢机上
/// （单调档位假设：Mc 档最高 → 五数各列极值应在 Ma（min）与 Mc（max））。
pub fn column_extreme_audit(led: &CrossLedger) -> Result<(), u16> {
    led.assert_full()?;
    for c in 0..5 {
        let mut lo: u64 = u64::MAX;
        let mut hi: u64 = 0;
        let mut lo_i = 0usize;
        let mut hi_i = 0usize;
        for (i, r) in led.rows.iter().enumerate() {
            if r.five.v[c] < lo {
                lo = r.five.v[c];
                lo_i = i;
            }
            if r.five.v[c] > hi {
                hi = r.five.v[c];
                hi_i = i;
            }
        }
        if lo_i != 0 {
            return Err(E_NOTE);
        }
        if hi_i != 2 {
            return Err(E_NOTE);
        }
        let _ = (lo, hi);
    }
    Ok(())
}

/// 行指纹登记簿：每行（机, 五数指纹, 注记指纹）登记，登记后对账只读。
pub struct RowFpBook {
    pub entries: Vec<(usize, u64, u64)>,
    pub sealed: bool,
}

impl RowFpBook {
    pub fn new() -> RowFpBook {
        RowFpBook { entries: Vec::new(), sealed: false }
    }

    pub fn record(&mut self, mach_idx: usize, five_fp: u64, note_fp: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        if mach_idx > 2 || five_fp == 0 {
            return Err(E_MACH);
        }
        for (m, _, _) in &self.entries {
            if *m == mach_idx {
                return Err(E_DUP);
            }
        }
        self.entries.push((mach_idx, five_fp, note_fp));
        Ok(())
    }

    /// 对账核：账面指纹与横比表现算指纹逐行全等。
    pub fn verify(&self, led: &CrossLedger) -> Result<(), u16> {
        if self.entries.len() != led.rows.len() {
            return Err(E_ROW);
        }
        for (i, (m, ff, nf)) in self.entries.iter().enumerate() {
            let r = &led.rows[i];
            if r.mach.idx() != *m {
                return Err(E_ROW);
            }
            if r.five.fingerprint() != *ff {
                return Err(E_ROW);
            }
            // 注记指纹：5 个千分比拼装
            let mut buf = vec![0u8; 20];
            for (c, v) in r.notes.iter().enumerate() {
                buf[c * 4] = (*v >> 24) as u8;
                buf[c * 4 + 1] = (*v >> 16) as u8;
                buf[c * 4 + 2] = (*v >> 8) as u8;
                buf[c * 4 + 3] = *v as u8;
            }
            if fnv1a64(&buf) != *nf {
                return Err(E_NOTE);
            }
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        if self.entries.len() != 3 {
            return Err(E_ROW);
        }
        self.sealed = true;
        Ok(())
    }
}

/// 横比表口径行（表头）：声明行（镜像/核档/版本）程序化渲染。
pub fn header_line(led: &CrossLedger) -> Result<String, u16> {
    led.caliper.audit()?;
    let mut s = String::from("口径: 镜像=");
    s.push_str(&(led.caliper.mirror_hash % 1_000_000).to_string());
    s.push_str(" 核档=");
    s.push_str(&led.caliper.core_tier.to_string());
    s.push_str("核 采集器v");
    s.push_str(&led.caliper.probe_ver.to_string());
    s.push_str(" 同镜像同档声明: 成立");
    Ok(s)
}

/// 口径行自检：渲染行含声明尾串。
pub fn header_audit() -> Result<(), u16> {
    let cal = Caliper { mirror_hash: 0x0669, core_tier: 2, probe_ver: 3 };
    let mut led = CrossLedger::new(cal)?;
    for m in Machine::ALL {
        led.enroll(m, 11, &cal)?;
    }
    led.fill_notes()?;
    let h = header_line(&led)?;
    if !h.ends_with("成立") {
        return Err(E_CAL);
    }
    Ok(())
}

/// 横比报告（逐行派生，零手抄）。
pub struct CrossReport {
    pub lines: Vec<String>,
}

impl CrossReport {
    pub fn generate(led: &CrossLedger) -> Result<CrossReport, u16> {
        led.assert_full()?;
        led.assert_notes()?;
        let mut lines = Vec::new();
        lines.push(led.summary());
        for r in &led.rows {
            lines.push(r.summary());
        }
        Ok(CrossReport { lines })
    }

    pub fn assert_complete(&self) -> Result<(), u16> {
        if self.lines.len() != 4 {
            return Err(E_ROW);
        }
        for l in &self.lines {
            if l.is_empty() {
                return Err(E_NOTE);
            }
        }
        Ok(())
    }
}

/// 自例程：口径三查 + 三机入表 + 注记回填 + 注入全拦截 + 报告 → 0。
pub fn drill() -> Result<u32, u16> {
    delta_audit()?;
    synth_determinism_audit()?;
    inject_caliber_mismatch_must_reject()?;
    inject_tamper_must_detect()?;

    let cal = Caliper { mirror_hash: 0x0669_5A17, core_tier: 4, probe_ver: 21 };
    let mut led = CrossLedger::new(cal)?;
    // 注入 1：口径不符入表 → 拒
    let cal2 = Caliper { mirror_hash: 0x0669_5A17, core_tier: 8, probe_ver: 21 };
    if led.enroll(Machine::Ma, 1, &cal2) != Err(E_CAL) {
        return Err(SEG_BASE + 0x31);
    }
    // 三机入表（同口径）
    for m in Machine::ALL {
        led.enroll(m, 0x0669, &cal)?;
    }
    // 注入 2：同机重复入表 → 拦截
    if led.enroll(Machine::Ma, 0x0669, &cal) != Err(E_DUP) {
        return Err(SEG_BASE + 0x32);
    }
    // 注记回填 + 核
    led.fill_notes()?;
    led.assert_notes()?;
    // 注入 3：五数破序注入 → 检出
    let bad = FiveVec { v: [100, 90, 120, 130, 140] };
    if bad.assert_monotone() != Err(E_COL) {
        return Err(SEG_BASE + 0x33);
    }
    // 封账 + 封后入表拦截
    led.seal()?;
    if led.enroll(Machine::Ma, 1, &cal) != Err(E_SEAL) {
        return Err(SEG_BASE + 0x34);
    }
    // 注入 4：注记列算错检出（篡改注记）
    let mut led2 = CrossLedger::new(cal)?;
    for m in Machine::ALL {
        led2.enroll(m, 0x0669, &cal)?;
    }
    led2.fill_notes()?;
    led2.rows[1].notes[2] = 0;
    if led2.assert_notes().is_ok() {
        return Err(SEG_BASE + 0x35);
    }
    // 报告生成
    let rep = CrossReport::generate(&led)?;
    rep.assert_complete()?;
    // 列极值审计 + 口径行渲染
    column_extreme_audit(&led)?;
    let h = header_line(&led)?;
    if h.is_empty() {
        return Err(SEG_BASE + 0x38);
    }
    // 行指纹登记簿：三行登记 + 对账 + 封簿后写拦截
    let mut fb = RowFpBook::new();
    for r in &led.rows {
        let mut buf = vec![0u8; 20];
        for (c, v) in r.notes.iter().enumerate() {
            buf[c * 4] = (*v >> 24) as u8;
            buf[c * 4 + 1] = (*v >> 16) as u8;
            buf[c * 4 + 2] = (*v >> 8) as u8;
            buf[c * 4 + 3] = *v as u8;
        }
        fb.record(r.mach.idx(), r.five.fingerprint(), fnv1a64(&buf))?;
    }
    fb.verify(&led)?;
    fb.seal()?;
    if fb.record(0, 1, 1) != Err(E_SEAL) {
        return Err(SEG_BASE + 0x39);
    }
    // 注入 6：账面指纹篡改 → 对账检出
    let mut fb2 = RowFpBook::new();
    for r in &led.rows {
        let mut buf = vec![0u8; 20];
        for (c, v) in r.notes.iter().enumerate() {
            buf[c * 4] = (*v >> 24) as u8;
            buf[c * 4 + 1] = (*v >> 16) as u8;
            buf[c * 4 + 2] = (*v >> 8) as u8;
            buf[c * 4 + 3] = *v as u8;
        }
        fb2.record(r.mach.idx(), r.five.fingerprint(), fnv1a64(&buf))?;
    }
    fb2.entries[1].1 ^= 0xFF;
    if fb2.verify(&led).is_ok() {
        return Err(SEG_BASE + 0x3A);
    }
    // 注入 7：非法口径构造 → 拦截
    let bad_cal = Caliper { mirror_hash: 0, core_tier: 4, probe_ver: 1 };
    if CrossLedger::new(bad_cal).err() != Some(E_CAL) {
        return Err(SEG_BASE + 0x36);
    }
    let bad_cal2 = Caliper { mirror_hash: 1, core_tier: 6, probe_ver: 1 };
    if CrossLedger::new(bad_cal2).err() != Some(E_CAL) {
        return Err(SEG_BASE + 0x37);
    }
    Ok(0)
}
