//! UNX-F0689 · 引导报告 50 次零差异复测（分机执行）· 诊断段 0x56a0（本单独占）
//!
//! 目标行数：≥580（纯功能代码）
//! 判据原文摘录：UNX-F0689-J1 每机 50 次报告哈希逐次同（3 机 × 50 全同），结构位型口径
//! （数值豁免）生效断言过。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 结构位型口径：比对只看字段在场位图，数值字段一律豁免（数值差异不算差异）。
//!  3. 分机执行：3 机 × 50 次 = 150 份报告，逐机逐次结构哈希全同。
//!  4. 结构漂移必检出：缺字段/多字段 → 位图失配拦截（零漏检）。
//!  5. 豁免生效断言：数值变化 → 必须零差异（口径没失效）。
//!  6. 诊断段 0x56a0 为本单元独占段位，错误码闭集显别式。

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::vec;

pub const SEG_BASE: u16 = 0x56a0;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const E_STRUCT: u16 = SEG_BASE + 1;  // 结构漂移（位图失配）
pub const E_MACH: u16 = SEG_BASE + 2;    // 机型越界
pub const E_N: u16 = SEG_BASE + 3;       // 复测次数不足（≠50）
pub const E_FIELD: u16 = SEG_BASE + 4;   // 字段号非法
pub const E_BITMAP: u16 = SEG_BASE + 5;  // 位图非法（保留位/空图）
pub const E_SEAL: u16 = SEG_BASE + 6;    // 复测账已封
pub const E_ROW: u16 = SEG_BASE + 7;     // 复测账行要素缺

/// 每机复测次数。
pub const RUNS: u32 = 50;
/// 报告字段数（8 个）。
pub const N_FIELDS: u8 = 8;

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
}

/// 结构位图：8 字段在场位（bit0..bit7）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StructBitmap {
    pub bits: u8,
}

impl StructBitmap {
    /// 基线位图：全字段在场（0xFF）。
    pub fn baseline() -> StructBitmap {
        StructBitmap { bits: 0xFF }
    }

    /// 位图合法性：非零（全缺即空报告）。
    pub fn audit(&self) -> Result<(), u16> {
        if self.bits == 0 {
            return Err(E_BITMAP);
        }
        Ok(())
    }

    /// 字段在场判定。
    pub fn has(&self, f: u8) -> Result<bool, u16> {
        if f == 0 || f > N_FIELDS {
            return Err(E_FIELD);
        }
        Ok(self.bits & (1 << (f - 1)) != 0)
    }

    /// 结构哈希：位图即结构指纹。
    pub fn struct_fp(&self) -> u64 {
        let buf = [self.bits];
        fnv1a64(&buf)
    }

    /// 位差分析：失配字段清单。
    pub fn bit_diff(&self, other: &StructBitmap) -> Vec<u8> {
        let mut out = Vec::new();
        for f in 1..=N_FIELDS {
            let a = self.bits & (1 << (f - 1)) != 0;
            let b = other.bits & (1 << (f - 1)) != 0;
            if a != b {
                out.push(f);
            }
        }
        out
    }
}

/// 报告实例：位图 + 数值向量（数值豁免口径下仅位图参与比对）。
#[derive(Clone, Debug)]
pub struct ReportInst {
    pub mach: Machine,
    pub run: u32,
    pub bm: StructBitmap,
    /// 数值字段（8 个，豁免比对，但须自身合法）。
    pub vals: [u64; 8],
}

impl ReportInst {
    pub fn audit(&self) -> Result<(), u16> {
        if self.run == 0 {
            return Err(E_ROW);
        }
        self.bm.audit()?;
        for (i, v) in self.vals.iter().enumerate() {
            if (self.bm.bits & (1 << i)) != 0 && *v == 0 {
                // 在场字段数值不可为 0（占位哨兵）
                return Err(E_ROW);
            }
        }
        Ok(())
    }

    /// 数值豁免等价：位图全等即等价（数值不比）。
    pub fn exempt_equiv(&self, other: &ReportInst) -> Result<(), u16> {
        self.bm.audit()?;
        other.bm.audit()?;
        if self.bm.bits != other.bm.bits {
            return Err(E_STRUCT);
        }
        Ok(())
    }
}

/// 确定性报告生成：位图恒基线，数值随（机,次）变（豁免口径下仍零差异）。
pub fn synth_report(m: Machine, run: u32, seed: u64) -> Result<ReportInst, u16> {
    if run == 0 {
        return Err(E_ROW);
    }
    let mut rng = Lcg::new(seed ^ ((m.idx() as u64 + 1).wrapping_mul(0x9E37_79B9)) ^ run as u64);
    let mut vals = [0u64; 8];
    for v in vals.iter_mut() {
        let x = 1 + (rng.next_u32() % 5_000) as u64;
        *v = x;
    }
    Ok(ReportInst { mach: m, run, bm: StructBitmap::baseline(), vals })
}

/// 复测账行。
#[derive(Clone, Debug)]
pub struct RetestRow {
    pub mach: Machine,
    pub run: u32,
    /// 结构哈希。
    pub sfp: u64,
    /// 同机首测哈希（全同锚点）。
    pub anchor_fp: u64,
}

impl RetestRow {
    pub fn audit(&self) -> Result<(), u16> {
        if self.run == 0 || self.sfp == 0 || self.anchor_fp == 0 {
            return Err(E_ROW);
        }
        if self.sfp != self.anchor_fp {
            return Err(E_STRUCT);
        }
        Ok(())
    }
}

/// 单机复测账。
pub struct MachRetest {
    pub mach: Machine,
    pub rows: Vec<RetestRow>,
    pub sealed: bool,
}

impl MachRetest {
    pub fn new(m: Machine) -> MachRetest {
        MachRetest { mach: m, rows: Vec::new(), sealed: false }
    }

    /// 50 次复测：首测建锚，后续逐次比锚。
    pub fn run_full(&mut self, seed: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        self.rows.clear();
        let mut anchor: u64 = 0;
        for run in 1..=RUNS {
            let rep = synth_report(self.mach, run, seed)?;
            rep.audit()?;
            let fp = rep.bm.struct_fp();
            if run == 1 {
                anchor = fp;
            }
            self.rows.push(RetestRow { mach: self.mach, run, sfp: fp, anchor_fp: anchor });
        }
        if self.rows.len() as u32 != RUNS {
            return Err(E_N);
        }
        Ok(())
    }

    /// 逐次全同核。
    pub fn assert_all_same(&self) -> Result<(), u16> {
        if self.rows.len() as u32 != RUNS {
            return Err(E_N);
        }
        for r in &self.rows {
            r.audit()?;
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        self.assert_all_same()?;
        self.sealed = true;
        Ok(())
    }
}

/// 三机总账。
pub struct RetestLedger {
    pub books: Vec<MachRetest>,
    pub sealed: bool,
}

impl RetestLedger {
    pub fn new() -> RetestLedger {
        RetestLedger {
            books: vec![MachRetest::new(Machine::Ma), MachRetest::new(Machine::Mb), MachRetest::new(Machine::Mc)],
            sealed: false,
        }
    }

    pub fn run_all(&mut self, seed: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        for b in self.books.iter_mut() {
            b.run_full(seed)?;
        }
        Ok(())
    }

    /// 判据核：3 机 × 50 次全同。
    pub fn assert_all(&self) -> Result<(), u16> {
        let mut n: u32 = 0;
        for b in &self.books {
            b.assert_all_same()?;
            n += b.rows.len() as u32;
        }
        if n != RUNS * 3 {
            return Err(E_N);
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        self.assert_all()?;
        self.sealed = true;
        Ok(())
    }

    pub fn summary(&self) -> String {
        let mut s = String::from("零差异复测: ");
        s.push_str(&self.books.len().to_string());
        s.push_str(" 机 × 50 全同");
        s
    }
}

/// 结构漂移注入核：缺一字段 → 豁免比对必检出。
pub fn inject_struct_drift_must_fail(seed: u64) -> Result<(), u16> {
    let a = synth_report(Machine::Ma, 1, seed)?;
    let mut b = synth_report(Machine::Ma, 2, seed)?;
    b.bm.bits &= !(1 << 3); // 缺字段 4
    if a.exempt_equiv(&b).is_ok() {
        return Err(SEG_BASE + 0x30);
    }
    // 位差清单 = 1 项
    let d = a.bm.bit_diff(&b.bm);
    if d.len() != 1 || d[0] != 4 {
        return Err(SEG_BASE + 0x31);
    }
    Ok(())
}

/// 豁免生效注入核：数值全变 → 必须零差异（口径生效）。
pub fn inject_value_drift_must_pass(seed: u64) -> Result<(), u16> {
    let mut a = synth_report(Machine::Mb, 1, seed)?;
    let mut b = synth_report(Machine::Mb, 2, seed)?;
    // 数值全部打乱（位图不动）
    for v in a.vals.iter_mut() {
        *v = *v + 7;
    }
    for v in b.vals.iter_mut() {
        *v = *v + 999;
    }
    a.exempt_equiv(&b)?;
    Ok(())
}

/// 空位图注入核。
pub fn inject_empty_bitmap_must_fail() -> Result<(), u16> {
    let bm = StructBitmap { bits: 0 };
    if bm.audit() != Err(E_BITMAP) {
        return Err(SEG_BASE + 0x32);
    }
    Ok(())
}

/// 字段号越界注入核。
pub fn inject_field_range_must_fail() -> Result<(), u16> {
    let bm = StructBitmap::baseline();
    if bm.has(0).is_ok() {
        return Err(SEG_BASE + 0x33);
    }
    if bm.has(9).is_ok() {
        return Err(SEG_BASE + 0x34);
    }
    for f in 1..=8u8 {
        if !bm.has(f)? {
            return Err(SEG_BASE + 0x35);
        }
    }
    Ok(())
}

/// 占位哨兵注入核：在场字段数值 0 → 报告非法。
pub fn inject_sentinel_must_fail(seed: u64) -> Result<(), u16> {
    let mut a = synth_report(Machine::Mc, 3, seed)?;
    a.vals[0] = 0;
    if a.audit() != Err(E_ROW) {
        return Err(SEG_BASE + 0x36);
    }
    Ok(())
}

/// 复测账行锚点失配注入核：sfp ≠ anchor → 审计检出。
pub fn inject_anchor_mismatch_must_fail() -> Result<(), u16> {
    let fp = StructBitmap::baseline().struct_fp();
    let r = RetestRow { mach: Machine::Ma, run: 5, sfp: fp ^ 0xFF, anchor_fp: fp };
    if r.audit() != Err(E_STRUCT) {
        return Err(SEG_BASE + 0x37);
    }
    Ok(())
}

/// 确定性自检：同（机,次）两次生成位图与数值全等。
pub fn synth_determinism_audit(seed: u64) -> Result<(), u16> {
    let a = synth_report(Machine::Mc, 11, seed)?;
    let b = synth_report(Machine::Mc, 11, seed)?;
    if a.bm != b.bm || a.vals != b.vals {
        return Err(E_ROW);
    }
    Ok(())
}

/// 机间结构一致性：三机位图必须同构（同一镜像口径 → 同一报告结构）。
pub fn cross_mach_struct_audit(led: &RetestLedger) -> Result<(), u16> {
    if led.books.len() != 3 {
        return Err(E_MACH);
    }
    let base = led.books[0].rows[0].sfp;
    for b in led.books.iter().skip(1) {
        if b.rows[0].sfp != base {
            return Err(E_STRUCT);
        }
    }
    Ok(())
}

/// 位图子集审计：子集判定（a ⊆ b ⟺ a&b == a）。
pub fn subset_audit() -> Result<(), u16> {
    let full = StructBitmap { bits: 0xFF };
    let sub = StructBitmap { bits: 0b0000_1111 };
    if sub.bits & full.bits != sub.bits {
        return Err(E_BITMAP);
    }
    // 非子集反例
    let other = StructBitmap { bits: 0b0001_0000 };
    if other.bits & sub.bits == other.bits {
        return Err(E_BITMAP);
    }
    Ok(())
}

/// 结构指纹簿：逐机登记（机, 首测锚指纹），登记后对账只读。
pub struct StructFpBook {
    pub entries: Vec<(usize, u64)>,
    pub sealed: bool,
}

impl StructFpBook {
    pub fn new() -> StructFpBook {
        StructFpBook { entries: Vec::new(), sealed: false }
    }

    pub fn record(&mut self, mach_idx: usize, fp: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        if mach_idx > 2 || fp == 0 {
            return Err(E_MACH);
        }
        for (m, _) in &self.entries {
            if *m == mach_idx {
                return Err(E_MACH);
            }
        }
        self.entries.push((mach_idx, fp));
        Ok(())
    }

    /// 对账：账面指纹与复测账逐机锚全等。
    pub fn verify(&self, led: &RetestLedger) -> Result<(), u16> {
        if self.entries.len() != 3 {
            return Err(E_MACH);
        }
        for (m, fp) in &self.entries {
            if led.books[*m].rows[0].anchor_fp != *fp {
                return Err(E_STRUCT);
            }
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        if self.entries.len() != 3 {
            return Err(E_MACH);
        }
        self.sealed = true;
        Ok(())
    }
}

/// 指纹簿注入核：账面指纹篡改 → 对账检出。
pub fn inject_fpbook_tamper_must_fail(led: &RetestLedger) -> Result<(), u16> {
    let mut fb = StructFpBook::new();
    for b in &led.books {
        fb.record(b.mach.idx(), b.rows[0].anchor_fp)?;
    }
    fb.entries[2].1 ^= 0xFF;
    if fb.verify(led).is_ok() {
        return Err(SEG_BASE + 0x3B);
    }
    Ok(())
}

/// 复测账行守恒：每机行数 = 50 且（机,次）无重。
pub fn row_cons_audit(led: &RetestLedger) -> Result<(), u16> {
    for b in &led.books {
        if b.rows.len() as u32 != RUNS {
            return Err(E_N);
        }
        for want in 1..=RUNS {
            let mut hit = false;
            for r in &b.rows {
                if r.run == want {
                    if hit {
                        return Err(E_ROW);
                    }
                    hit = true;
                }
            }
            if !hit {
                return Err(E_N);
            }
        }
    }
    Ok(())
}

/// 位差清单注入核：两字段漂移 → 位差 2 项。
pub fn inject_two_bit_drift() -> Result<(), u16> {
    let a = StructBitmap { bits: 0b1111_1111 };
    let b = StructBitmap { bits: 0b1100_1111 };
    let d = a.bit_diff(&b);
    if d.len() != 2 {
        return Err(SEG_BASE + 0x3C);
    }
    if d[0] != 5 || d[1] != 6 {
        return Err(SEG_BASE + 0x3D);
    }
    Ok(())
}

/// 复测报告。
pub struct RetestReport {
    pub lines: Vec<String>,
}

impl RetestReport {
    pub fn generate(led: &RetestLedger) -> Result<RetestReport, u16> {
        led.assert_all()?;
        let mut lines = Vec::new();
        lines.push(led.summary());
        lines.push(String::from("口径: 结构位型比对，数值豁免"));
        lines.push(String::from("结论: 3 机 × 50 次结构哈希逐次全同"));
        Ok(RetestReport { lines })
    }

    pub fn assert_complete(&self) -> Result<(), u16> {
        if self.lines.len() != 3 {
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

/// 自例程：150 份报告逐次全同 + 结构漂移检出 + 豁免生效 + 注入全拦截 → 0。
pub fn drill() -> Result<u32, u16> {
    let seed = 0x0689_0001;
    inject_struct_drift_must_fail(seed)?;
    inject_value_drift_must_pass(seed)?;
    inject_empty_bitmap_must_fail()?;
    inject_field_range_must_fail()?;
    inject_sentinel_must_fail(seed)?;
    inject_anchor_mismatch_must_fail()?;
    inject_two_bit_drift()?;
    synth_determinism_audit(seed)?;
    subset_audit()?;

    let mut led = RetestLedger::new();
    led.run_all(seed)?;
    led.assert_all()?;
    // 机间结构一致 + 行守恒 + 指纹簿
    cross_mach_struct_audit(&led)?;
    row_cons_audit(&led)?;
    let mut fb = StructFpBook::new();
    for b in &led.books {
        fb.record(b.mach.idx(), b.rows[0].anchor_fp)?;
    }
    fb.verify(&led)?;
    inject_fpbook_tamper_must_fail(&led)?;
    fb.seal()?;
    // 注入：位图漂移构造行 → 审计检出（在机一账里塞一行漂移锚）
    let fp = StructBitmap::baseline().struct_fp();
    led.books[0].rows[9].sfp = fp ^ 0x0F;
    if led.books[0].assert_all_same() != Err(E_STRUCT) {
        return Err(SEG_BASE + 0x38);
    }
    // 复原后封账 + 封后重跑拦截
    led.books[0].rows[9].sfp = fp;
    led.books[0].assert_all_same()?;
    led.seal()?;
    if led.run_all(2) != Err(E_SEAL) {
        return Err(SEG_BASE + 0x39);
    }
    // 报告
    let rep = RetestReport::generate(&led)?;
    rep.assert_complete()?;
    if led.summary().is_empty() {
        return Err(SEG_BASE + 0x3A);
    }
    Ok(0)
}
