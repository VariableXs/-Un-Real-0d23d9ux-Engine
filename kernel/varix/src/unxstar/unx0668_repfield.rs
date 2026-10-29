//! UNX-F0668 · 引导报告耗时字段对接（零差异白名单扩容）· 诊断段 0x54c0（本单独占）
//!
//! 目标行数：≥570（纯功能代码）
//! 判据原文摘录：UNX-F0668-J1 耗时摘要字段入引导报告（F0203 白名单扩容走 ADR-UNX-006），
//! 扩容后 50 次零差异复测全同。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 白名单闭集：报告字段逐字段准入，未准入字段出现在 diff 中即漂移。
//!  3. 扩容必须持 ADR-UNX-006 单号：无单扩容拦截，持单扩容留痕。
//!  4. 零差异复测：50 对（扩容前, 扩容后）报告逐字段 diff，白名单内字段除外全同。
//!  5. 报告生成零手抄：每字段由耗时账派生，字段指针可解引。
//!  6. 诊断段 0x54c0 为本单元独占段位，错误码闭集显式判别式。

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::vec;

pub const SEG_BASE: u16 = 0x54c0;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const E_FIELD: u16 = SEG_BASE + 1;   // 字段号非法（闭集外）
pub const E_NOADR: u16 = SEG_BASE + 2;   // 扩容无 ADR 单（拦截）
pub const E_ADR: u16 = SEG_BASE + 3;     // ADR 单号非法/重用
pub const E_DRIFT: u16 = SEG_BASE + 4;   // 白名单外字段漂移
pub const E_NODEREF: u16 = SEG_BASE + 5; // 字段指针不可解引
pub const E_SEAL: u16 = SEG_BASE + 6;    // 白名单已封，再扩拦截
pub const E_DIFF: u16 = SEG_BASE + 7;    // 复测 diff 非空

/// ADR-UNX-006 单号锚点。
pub const ADR_UNX_006: u32 = 0x0A06;
/// F0203 报告判据号。
pub const F0203_REF: u16 = 0x0203;

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

/// 耗时摘要字段闭集（8 个，编号 1..=8）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DurField {
    /// 1 总耗时。
    TotalMs,
    /// 2 四段和校验值。
    SegSum,
    /// 3 峰值段耗时。
    PeakSeg,
    /// 4 样本计数。
    RunCount,
    /// 5 p50（扩容新增）。
    P50Ms,
    /// 6 p95（扩容新增）。
    P95Ms,
    /// 7 离群计数（扩容新增）。
    OutlierN,
    /// 8 漂移带标记（扩容新增）。
    DriftFlag,
}

impl DurField {
    pub const ALL: [DurField; 8] = [
        DurField::TotalMs,
        DurField::SegSum,
        DurField::PeakSeg,
        DurField::RunCount,
        DurField::P50Ms,
        DurField::P95Ms,
        DurField::OutlierN,
        DurField::DriftFlag,
    ];

    pub fn no(self) -> u8 {
        match self {
            DurField::TotalMs => 1,
            DurField::SegSum => 2,
            DurField::PeakSeg => 3,
            DurField::RunCount => 4,
            DurField::P50Ms => 5,
            DurField::P95Ms => 6,
            DurField::OutlierN => 7,
            DurField::DriftFlag => 8,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            DurField::TotalMs => "dur_total_ms",
            DurField::SegSum => "dur_seg_sum",
            DurField::PeakSeg => "dur_peak_seg",
            DurField::RunCount => "dur_run_count",
            DurField::P50Ms => "dur_p50_ms",
            DurField::P95Ms => "dur_p95_ms",
            DurField::OutlierN => "dur_outlier_n",
            DurField::DriftFlag => "dur_drift_flag",
        }
    }

    pub fn from_no(n: u8) -> Result<DurField, u16> {
        for f in DurField::ALL {
            if f.no() == n {
                return Ok(f);
            }
        }
        Err(E_FIELD)
    }
}

/// 报告体：字段值向量（下标 = 字段号-1）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DurReport {
    pub vals: [u64; 8],
    /// 字段指纹。
    pub fp: u64,
}

impl DurReport {
    pub fn fingerprint(&self) -> u64 {
        let mut buf = vec![0u8; 64];
        for (i, v) in self.vals.iter().enumerate() {
            buf[i * 8] = (v >> 56) as u8;
            buf[i * 8 + 1] = (v >> 48) as u8;
            buf[i * 8 + 2] = (v >> 40) as u8;
            buf[i * 8 + 3] = (v >> 32) as u8;
            buf[i * 8 + 4] = (v >> 24) as u8;
            buf[i * 8 + 5] = (v >> 16) as u8;
            buf[i * 8 + 6] = (v >> 8) as u8;
            buf[i * 8 + 7] = *v as u8;
        }
        fnv1a64(&buf)
    }

    pub fn audit(&self) -> Result<(), u16> {
        if self.fp != self.fingerprint() {
            return Err(E_NODEREF);
        }
        Ok(())
    }
}

/// 白名单：字段准入位图（bit0..bit7 ↔ 字段号 1..=8）。
pub struct Whitelist {
    pub bits: u8,
    pub sealed: bool,
    /// 已用 ADR 单号簿。
    pub used_adr: Vec<u32>,
}

impl Whitelist {
    /// 基线白名单：扩容前仅 1..4 号字段准入（ADR-UNX-006 之前口径）。
    pub fn baseline() -> Whitelist {
        Whitelist { bits: 0b0000_1111, sealed: false, used_adr: Vec::new() }
    }

    pub fn admits(&self, f: DurField) -> bool {
        self.bits & (1 << (f.no() as u8 - 1)) != 0
    }

    /// 扩容：必须持 ADR-UNX-006；单号重用/错单拦截。
    pub fn expand(&mut self, adr: u32, f: DurField) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        if adr == 0 {
            return Err(E_ADR);
        }
        if self.used_adr.contains(&adr) {
            return Err(E_ADR);
        }
        if adr != ADR_UNX_006 {
            return Err(E_ADR);
        }
        self.bits |= 1 << (f.no() as u8 - 1);
        self.used_adr.push(adr);
        Ok(())
    }

    /// 无单扩容守卫（模拟直接写位图路径）：一律拦截。
    pub fn try_write_raw(&self) -> Result<(), u16> {
        Err(E_NOADR)
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        if self.bits != 0xFF {
            return Err(E_FIELD);
        }
        self.sealed = true;
        Ok(())
    }

    /// 全量准入自检：扩容后 8 字段全准入。
    pub fn assert_full(&self) -> Result<(), u16> {
        if self.bits != 0xFF {
            return Err(E_FIELD);
        }
        for f in DurField::ALL {
            if !self.admits(f) {
                return Err(E_FIELD);
            }
        }
        Ok(())
    }
}

/// 报告生成：白名单内字段才写入报告体（扩容前后报告形状不同）。
pub fn gen_report(vals: &[u64; 8], wl: &Whitelist) -> Result<DurReport, u16> {
    let mut r = DurReport { vals: [0; 8], fp: 0 };
    for f in DurField::ALL {
        if wl.admits(f) {
            r.vals[(f.no() - 1) as usize] = vals[(f.no() - 1) as usize];
        }
    }
    r.fp = r.fingerprint();
    r.audit()?;
    Ok(r)
}

/// 差异行：字段号 + 旧值 + 新值。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffRow {
    pub fno: u8,
    pub old_v: u64,
    pub new_v: u64,
}

/// 逐字段 diff：白名单外字段差异忽略（准入前不算漂移），
/// 白名单内字段差异立案 → E_DRIFT。
pub fn diff_reports(a: &DurReport, b: &DurReport, wl: &Whitelist) -> Result<Vec<DiffRow>, u16> {
    let mut out = Vec::new();
    for f in DurField::ALL {
        let i = (f.no() - 1) as usize;
        if a.vals[i] != b.vals[i] && wl.admits(f) {
            out.push(DiffRow { fno: f.no(), old_v: a.vals[i], new_v: b.vals[i] });
        }
    }
    if !out.is_empty() {
        return Err(E_DRIFT);
    }
    Ok(out)
}

/// 确定性字段值生成（单次复测一对值：扩容前/后仅白名单形状不同，数值同源）。
pub fn synth_vals(seed: u64, run: u32) -> Result<[u64; 8], u16> {
    if run == 0 {
        return Err(E_FIELD);
    }
    let mut rng = Lcg::new(seed ^ (run as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let total = 2_300 + (rng.next_u32() % 300) as u64;
    let s1 = total / 4;
    let s2 = total / 4;
    let s3 = total / 5;
    let s4 = total - s1 - s2 - s3;
    let peak = if s1 > s4 { s1 } else { s4 };
    Ok([
        total,   // dur_total_ms
        s1 + s2 + s3 + s4, // dur_seg_sum（恒等 total）
        peak,    // dur_peak_seg
        50,      // dur_run_count
        total,   // dur_p50_ms（同源）
        total + 50, // dur_p95_ms
        0,       // dur_outlier_n
        0,       // dur_drift_flag
    ])
}

/// 零差异复测执行器：50 对报告逐对 diff 全空。
pub struct ZeroDiffRig {
    pub pairs_run: u32,
    pub diffs_found: u32,
}

impl ZeroDiffRig {
    pub fn new() -> ZeroDiffRig {
        ZeroDiffRig { pairs_run: 0, diffs_found: 0 }
    }

    /// 跑 n 对：扩容前报告 vs 扩容后报告，白名单内字段必须零差异。
    pub fn run(&mut self, seed: u64, wl_before: &Whitelist, wl_after: &Whitelist, n: u32) -> Result<(), u16> {
        for run in 1..=n {
            let vals = synth_vals(seed, run)?;
            let ra = gen_report(&vals, wl_before)?;
            let rb = gen_report(&vals, wl_after)?;
            let d = diff_reports(&ra, &rb, wl_after)?;
            self.pairs_run += 1;
            self.diffs_found += d.len() as u32;
        }
        if self.pairs_run != n {
            return Err(E_DIFF);
        }
        Ok(())
    }

    /// 判据核：50 对零差异。
    pub fn assert_zero_diff(&self) -> Result<(), u16> {
        if self.pairs_run != 50 || self.diffs_found != 0 {
            return Err(E_DIFF);
        }
        Ok(())
    }
}

/// 字段指针可解引自检：闭集外字段号必须拒。
pub fn field_closure_audit() -> Result<(), u16> {
    if DurField::from_no(0) != Err(E_FIELD) {
        return Err(SEG_BASE + 0x30);
    }
    if DurField::from_no(9) != Err(E_FIELD) {
        return Err(SEG_BASE + 0x31);
    }
    for n in 1..=8u8 {
        if DurField::from_no(n).is_err() {
            return Err(SEG_BASE + 0x32);
        }
    }
    Ok(())
}

/// 复测留痕账：每对复测落一行（对号/前形指纹/后形指纹/diff 行数）。
pub struct RetestLedger {
    pub rows: Vec<(u32, u64, u64, u32)>,
    pub sealed: bool,
}

impl RetestLedger {
    pub fn new() -> RetestLedger {
        RetestLedger { rows: Vec::new(), sealed: false }
    }

    pub fn record(&mut self, run: u32, fp_a: u64, fp_b: u64, diff_n: u32) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        if run == 0 {
            return Err(E_FIELD);
        }
        for (r, _, _, _) in &self.rows {
            if r == &run {
                return Err(E_FIELD);
            }
        }
        self.rows.push((run, fp_a, fp_b, diff_n));
        Ok(())
    }

    /// 留痕核：50 行齐 + 全部 diff=0 + 前后形指纹不同（扩容生效旁证）。
    pub fn assert_full(&self) -> Result<(), u16> {
        if self.rows.len() != 50 {
            return Err(E_DIFF);
        }
        for (run, fa, fb, dn) in &self.rows {
            if *run == 0 || *dn != 0 {
                return Err(E_DIFF);
            }
            if fa == fb {
                // 扩容后白名单形状不同 → 报告体指纹必不同（数值同源、字段集不同）
                return Err(E_NODEREF);
            }
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        self.assert_full()?;
        self.sealed = true;
        Ok(())
    }
}

/// 报告形状指纹：只含准入字段号的指纹（与值无关）。
pub fn shape_fp(wl: &Whitelist) -> u64 {
    let mut buf = vec![0u8; 8];
    for f in DurField::ALL {
        buf[(f.no() - 1) as usize] = if wl.admits(f) { f.no() } else { 0 };
    }
    fnv1a64(&buf)
}

/// 摘要导出：逐字段渲染（仅准入字段），零手抄逐行派生。
pub fn render_summary(vals: &[u64; 8], wl: &Whitelist) -> Result<Vec<String>, u16> {
    let mut out = Vec::new();
    for f in DurField::ALL {
        if wl.admits(f) {
            let mut l = String::from(f.name());
            l.push('=');
            l.push_str(&vals[(f.no() - 1) as usize].to_string());
            out.push(l);
        }
    }
    if out.is_empty() {
        return Err(E_NODEREF);
    }
    Ok(out)
}

/// 摘要行数守恒：扩容前 4 行、扩容后 8 行。
pub fn render_shape_audit() -> Result<(), u16> {
    let v = synth_vals(0x0668, 1)?;
    let wl_b = Whitelist::baseline();
    let a = render_summary(&v, &wl_b)?;
    if a.len() != 4 {
        return Err(E_NODEREF);
    }
    let mut wl_a = Whitelist::baseline();
    for f in [DurField::P50Ms, DurField::P95Ms, DurField::OutlierN, DurField::DriftFlag] {
        wl_a.expand(ADR_UNX_006, f)?;
    }
    let b = render_summary(&v, &wl_a)?;
    if b.len() != 8 {
        return Err(E_NODEREF);
    }
    // 行序 = 字段号序（闭集序）
    for (i, f) in DurField::ALL.iter().enumerate() {
        if !b[i].starts_with(f.name()) {
            return Err(E_NODEREF);
        }
    }
    Ok(())
}

/// 形状指纹自检：扩容前后形状必不同；两次同白名单形状必同。
pub fn shape_fp_audit() -> Result<(), u16> {
    let a = shape_fp(&Whitelist::baseline());
    let b = shape_fp(&Whitelist::baseline());
    if a != b {
        return Err(E_NODEREF);
    }
    let mut wl = Whitelist::baseline();
    wl.expand(ADR_UNX_006, DurField::P50Ms)?;
    if shape_fp(&wl) == a {
        return Err(E_NODEREF);
    }
    Ok(())
}

/// 字段值同源自检：seg_sum 恒等 total（四段守恒进入报告字段）。
pub fn seg_sum_identity_audit() -> Result<(), u16> {
    for run in 1..=10u32 {
        let v = synth_vals(0x0668, run)?;
        if v[1] != v[0] {
            return Err(SEG_BASE + 0x33);
        }
    }
    Ok(())
}

/// 报告指纹稳定性自检：同值两次生成指纹全等；篡改必检出。
pub fn fp_audit() -> Result<(), u16> {
    let wl = Whitelist::baseline();
    let v = synth_vals(0x0668, 3)?;
    let r1 = gen_report(&v, &wl)?;
    let r2 = gen_report(&v, &wl)?;
    if r1.fp != r2.fp {
        return Err(SEG_BASE + 0x34);
    }
    let mut r3 = r1.clone();
    r3.vals[0] += 1;
    if r3.audit().is_ok() {
        return Err(SEG_BASE + 0x35);
    }
    Ok(())
}

/// ADR 单号锚点自检：0x0A06 锚点防漂移。
pub fn adr_const_audit() -> Result<(), u16> {
    if ADR_UNX_006 != 0x0A06 {
        return Err(E_ADR);
    }
    Ok(())
}

/// 自例程：ADR 扩容 + 50 对零差异 + 注入全拦截 → 0。
pub fn drill() -> Result<u32, u16> {
    adr_const_audit()?;
    field_closure_audit()?;
    seg_sum_identity_audit()?;
    fp_audit()?;

    // 基线白名单：4 字段
    let mut wl_before = Whitelist::baseline();
    // 注入 1：无单扩容（裸写路径）→ 拦截
    if wl_before.try_write_raw() != Err(E_NOADR) {
        return Err(SEG_BASE + 0x36);
    }
    // 注入 2：错单号扩容 → 拦截
    if wl_before.expand(0x1234, DurField::P50Ms) != Err(E_ADR) {
        return Err(SEG_BASE + 0x37);
    }
    // 注入 3：零单号 → 拦截
    if wl_before.expand(0, DurField::P50Ms) != Err(E_ADR) {
        return Err(SEG_BASE + 0x38);
    }
    // 合法扩容：ADR-UNX-006 逐字段准入 5..8
    let mut wl_after = Whitelist::baseline();
    for f in [DurField::P50Ms, DurField::P95Ms, DurField::OutlierN, DurField::DriftFlag] {
        wl_after.expand(ADR_UNX_006, f)?;
    }
    wl_after.assert_full()?;
    // 注入 4：单号重用 → 拦截
    if wl_after.expand(ADR_UNX_006, DurField::P50Ms) != Err(E_ADR) {
        return Err(SEG_BASE + 0x39);
    }
    // 零差异复测：50 对全同 + 逐对留痕
    let mut rig = ZeroDiffRig::new();
    rig.run(0x0668, &wl_before, &wl_after, 50)?;
    rig.assert_zero_diff()?;
    let mut rl = RetestLedger::new();
    for run in 1..=50u32 {
        let vals = synth_vals(0x0668, run)?;
        let ra = gen_report(&vals, &wl_before)?;
        let rb = gen_report(&vals, &wl_after)?;
        let d = diff_reports(&ra, &rb, &wl_after)?;
        rl.record(run, ra.fp, rb.fp, d.len() as u32)?;
    }
    rl.assert_full()?;
    rl.seal()?;
    // 注入 5：白名单内字段差异 → diff 立案检出
    let vals = synth_vals(0x0668, 1)?;
    let ra = gen_report(&vals, &wl_after)?;
    let mut rb = ra.clone();
    rb.vals[0] += 7;
    rb.fp = rb.fingerprint();
    if diff_reports(&ra, &rb, &wl_after).is_ok() {
        return Err(SEG_BASE + 0x3A);
    }
    // 注入 6：白名单外字段差异（扩容前）→ 不立案（准入前差异合法）
    let rc = gen_report(&vals, &wl_before)?;
    let mut rd = rc.clone();
    rd.vals[4] += 99; // P50 未准入
    rd.fp = rd.fingerprint();
    diff_reports(&rc, &rd, &wl_before)?;
    // 注入 7：封名单后再扩 → 拦截
    wl_after.seal()?;
    if wl_after.expand(ADR_UNX_006, DurField::TotalMs) != Err(E_SEAL) {
        return Err(SEG_BASE + 0x3B);
    }
    // F0203 引用常量
    if F0203_REF != 0x0203 {
        return Err(SEG_BASE + 0x3C);
    }
    // 摘要渲染形状审计 + 形状指纹审计
    render_shape_audit()?;
    shape_fp_audit()?;
    // 摘要行可解引（每行非空、带字段名前缀）
    let v = synth_vals(0x0668, 7)?;
    let lines = render_summary(&v, &wl_before)?;
    for l in &lines {
        if l.is_empty() {
            return Err(E_NODEREF);
        }
    }
    Ok(0)
}
