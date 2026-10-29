//! UNX-F0672 · 耗时基线审计账（优化前后 diff）· 诊断段 0x5540（本单独占）
//!
//! 目标行数：≥570（纯功能代码）
//! 判据原文摘录：UNX-F0672-J1 现值 vs 工程首测基线 diff 账落簿（逐段差值+归因注记），
//! 基线账只读零改写。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 基线账只读：任何写路径一律 E_RDONLY，diff 以基线为不可变基准。
//!  3. diff 账行四要素：段号/基线值/现值/差值，归因注记列闭集三类。
//!  4. 归因注记规则：降 ≥100‰ → 优化生效；升 ≥100‰ → 回归退化；否则波动带内。
//!  5. diff 行数守恒 = 段数 × 机数，行行可解引。
//!  6. 诊断段 0x5540 为本单元独占段位，错误码闭集显式判别式。

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::vec;

pub const SEG_BASE: u16 = 0x5540;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const E_RDONLY: u16 = SEG_BASE + 1; // 基线账写尝试（零改写拦截）
pub const E_SEG: u16 = SEG_BASE + 2;    // 段号越界
pub const E_NOTE: u16 = SEG_BASE + 3;   // 归因注记与差值不符
pub const E_ROW: u16 = SEG_BASE + 4;    // diff 账行要素缺
pub const E_BASE0: u16 = SEG_BASE + 5;  // 基线值为 0（分母非法）
pub const E_SEAL: u16 = SEG_BASE + 6;   // diff 账已封
pub const E_N: u16 = SEG_BASE + 7;      // diff 行数不足

/// 归因硬线：100‰（±10%）。
pub const NOTE_PERMILLE: u32 = 100;
/// 段数。
pub const N_SEG: usize = 4;
/// 工程首测基线判据号（账源引用）。
pub const BASELINE_CRIT: u16 = 0x0123;

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 归因注记闭集三类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttrNote {
    /// 优化生效：现值较基线降 ≥100‰。
    Optimized,
    /// 回归退化：现值较基线升 ≥100‰。
    Regressed,
    /// 波动带内：|Δ| < 100‰。
    Noise,
}

impl AttrNote {
    pub fn name(self) -> &'static str {
        match self {
            AttrNote::Optimized => "优化生效",
            AttrNote::Regressed => "回归退化",
            AttrNote::Noise => "波动带内",
        }
    }
}

/// 归因规则（纯函数）：差值符号 × 千分比幅度 → 注记。
pub fn attr_of(base: u64, cur: u64) -> Result<(i64, u32, AttrNote), u16> {
    if base == 0 {
        return Err(E_BASE0);
    }
    let delta = cur as i64 - base as i64;
    let mag = if delta < 0 { (-delta) as u64 } else { delta as u64 };
    let pm = (mag * 1000 / base) as u32;
    let note = if delta < 0 && pm >= NOTE_PERMILLE {
        AttrNote::Optimized
    } else if delta > 0 && pm >= NOTE_PERMILLE {
        AttrNote::Regressed
    } else {
        AttrNote::Noise
    };
    Ok((delta, pm, note))
}

/// 工程首测基线（只读快照）：4 段 × 3 机，值写死（来源注：工程首测册 F0123）。
pub struct Baseline {
    /// [机][段] ms。
    pub vals: [[u64; 4]; 3],
}

impl Baseline {
    pub fn first_shot() -> Baseline {
        Baseline {
            vals: [
                [650, 520, 480, 350], // M-A
                [700, 560, 510, 380], // M-B
                [760, 600, 550, 420], // M-C
            ],
        }
    }

    pub fn get(&self, m: usize, seg: usize) -> Result<u64, u16> {
        if m > 2 || seg > 3 {
            return Err(E_SEG);
        }
        let v = self.vals[m][seg];
        if v == 0 {
            return Err(E_BASE0);
        }
        Ok(v)
    }

    /// 只读守卫：写路径一律拦截（零改写）。
    pub fn try_write(&self, _m: usize, _seg: usize, _v: u64) -> Result<(), u16> {
        Err(E_RDONLY)
    }

    /// 基线指纹（封账对账锚点）。
    pub fn fingerprint(&self) -> u64 {
        let mut buf = vec![0u8; 96];
        for (i, row) in self.vals.iter().enumerate() {
            for (j, v) in row.iter().enumerate() {
                let k = (i * 4 + j) * 8;
                buf[k] = (v >> 56) as u8;
                buf[k + 1] = (v >> 48) as u8;
                buf[k + 2] = (v >> 40) as u8;
                buf[k + 3] = (v >> 32) as u8;
                buf[k + 4] = (v >> 24) as u8;
                buf[k + 5] = (v >> 16) as u8;
                buf[k + 6] = (v >> 8) as u8;
                buf[k + 7] = *v as u8;
            }
        }
        fnv1a64(&buf)
    }
}

/// 现值采集（确定性生成：相对基线的确定性偏移模式）。
pub fn synth_current(m: usize, seed: u64) -> Result<[u64; 4], u16> {
    if m > 2 {
        return Err(E_SEG);
    }
    let base = Baseline::first_shot();
    // 偏移模式：段 0 降 200‰（优化生效），段 1 升 40‰（波动），段 2 升 150‰（退化），段 3 降 30‰（波动）
    let mut out = [0u64; 4];
    for seg in 0..4 {
        let b = base.get(m, seg)?;
        let pm: i64 = match seg {
            0 => -200,
            1 => 40,
            2 => 150,
            _ => -30,
        };
        let v = (b as i64 + b as i64 * pm / 1000) + seed as i64 % 3;
        if v <= 0 {
            return Err(E_BASE0);
        }
        out[seg] = v as u64;
    }
    Ok(out)
}

/// diff 账行：四要素 + 归因注记。
#[derive(Clone, Debug)]
pub struct DiffRow {
    pub mach: usize,
    pub seg: usize,
    pub base_ms: u64,
    pub cur_ms: u64,
    pub delta: i64,
    /// 差值千分比幅度。
    pub pm: u32,
    pub note: AttrNote,
}

impl DiffRow {
    pub fn audit(&self) -> Result<(), u16> {
        if self.mach > 2 || self.seg > 3 {
            return Err(E_SEG);
        }
        if self.base_ms == 0 || self.cur_ms == 0 {
            return Err(E_BASE0);
        }
        if self.delta != self.cur_ms as i64 - self.base_ms as i64 {
            return Err(E_ROW);
        }
        let (_, pm, note) = attr_of(self.base_ms, self.cur_ms)?;
        if pm != self.pm || note != self.note {
            return Err(E_NOTE);
        }
        Ok(())
    }

    pub fn summary(&self) -> String {
        let mut s = String::from("M");
        s.push_str(&self.mach.to_string());
        s.push_str(" 段");
        s.push_str(&self.seg.to_string());
        s.push_str(" 基线=");
        s.push_str(&self.base_ms.to_string());
        s.push_str(" 现值=");
        s.push_str(&self.cur_ms.to_string());
        s.push_str(" Δ=");
        s.push_str(&self.delta.to_string());
        s.push_str("ms ");
        s.push_str(self.note.name());
        s
    }
}

/// diff 账：12 行（3 机 × 4 段）。
pub struct DiffLedger {
    pub rows: Vec<DiffRow>,
    pub sealed: bool,
}

impl DiffLedger {
    pub fn new() -> DiffLedger {
        DiffLedger { rows: Vec::new(), sealed: false }
    }

    /// 全量落簿：基线 × 现值逐段 diff。
    pub fn build(&mut self, seed: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        self.rows.clear();
        let base = Baseline::first_shot();
        for m in 0..3usize {
            let cur = synth_current(m, seed)?;
            for seg in 0..4usize {
                let b = base.get(m, seg)?;
                let (delta, pm, note) = attr_of(b, cur[seg])?;
                self.rows.push(DiffRow {
                    mach: m,
                    seg,
                    base_ms: b,
                    cur_ms: cur[seg],
                    delta,
                    pm,
                    note,
                });
            }
        }
        Ok(())
    }

    /// 行数守恒：12 行（3 × 4）。
    pub fn assert_full(&self) -> Result<(), u16> {
        if self.rows.len() != 3 * N_SEG {
            return Err(E_N);
        }
        for r in &self.rows {
            r.audit()?;
        }
        Ok(())
    }

    /// 归因注记分布核：优化/退化/波动三类至少各 1 行（偏移模式保证）。
    pub fn assert_notes_spread(&self) -> Result<(), u16> {
        self.assert_full()?;
        let mut has_o = false;
        let mut has_r = false;
        let mut has_n = false;
        for r in &self.rows {
            match r.note {
                AttrNote::Optimized => has_o = true,
                AttrNote::Regressed => has_r = true,
                AttrNote::Noise => has_n = true,
            }
        }
        if !has_o || !has_r || !has_n {
            return Err(E_NOTE);
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        self.assert_full()?;
        self.sealed = true;
        Ok(())
    }

    pub fn summary(&self) -> String {
        let mut s = String::from("基线 diff 账: 行数=");
        s.push_str(&self.rows.len().to_string());
        s.push_str("（3 机 × 4 段）");
        s
    }
}

/// 归因规则自检：已知三对值逐对核对注记。
pub fn attr_rule_audit() -> Result<(), u16> {
    // 降 200‰ → 优化
    let (d, pm, n) = attr_of(1_000, 800)?;
    if d != -200 || pm != 200 || n != AttrNote::Optimized {
        return Err(E_NOTE);
    }
    // 升 100‰ 压线 → 退化
    let (d2, pm2, n2) = attr_of(1_000, 1_100)?;
    if d2 != 100 || pm2 != 100 || n2 != AttrNote::Regressed {
        return Err(E_NOTE);
    }
    // 升 99‰ → 波动
    let (_, _, n3) = attr_of(1_000, 1_099)?;
    if n3 != AttrNote::Noise {
        return Err(E_NOTE);
    }
    // 降 99‰ → 波动
    let (_, _, n4) = attr_of(1_000, 901)?;
    if n4 != AttrNote::Noise {
        return Err(E_NOTE);
    }
    // 零基线拦截
    if attr_of(0, 100) != Err(E_BASE0) {
        return Err(SEG_BASE + 0x30);
    }
    Ok(())
}

/// 基线只读注入核：写必拦截；指纹两次现算全等；篡改可检出。
pub fn baseline_readonly_audit() -> Result<(), u16> {
    let b = Baseline::first_shot();
    if b.try_write(0, 0, 999) != Err(E_RDONLY) {
        return Err(SEG_BASE + 0x31);
    }
    let f1 = b.fingerprint();
    let f2 = b.fingerprint();
    if f1 != f2 {
        return Err(SEG_BASE + 0x32);
    }
    let mut tamper = Baseline::first_shot();
    tamper.vals[2][1] += 1;
    if tamper.fingerprint() == f1 {
        return Err(SEG_BASE + 0x33);
    }
    // 引用判据号
    if BASELINE_CRIT != 0x0123 {
        return Err(E_SEG);
    }
    Ok(())
}

/// diff 篡改注入核：改 delta 不改值 → 审计检出；改注记 → 审计检出。
pub fn diff_tamper_audit() -> Result<(), u16> {
    let mut led = DiffLedger::new();
    led.build(0x0672)?;
    led.rows[3].delta += 1;
    if led.rows[3].audit().is_ok() {
        return Err(SEG_BASE + 0x34);
    }
    let mut led2 = DiffLedger::new();
    led2.build(0x0672)?;
    led2.rows[5].note = AttrNote::Optimized;
    if led2.rows[5].audit().is_ok() {
        return Err(SEG_BASE + 0x35);
    }
    Ok(())
}

/// 样本确定性自检。
pub fn synth_determinism_audit() -> Result<(), u16> {
    let a = synth_current(1, 0x0672)?;
    let b = synth_current(1, 0x0672)?;
    if a != b {
        return Err(E_ROW);
    }
    Ok(())
}

/// 段名闭集（与守恒核同口径）。
pub const SEG_NAMES: [&str; 4] = ["loader", "init", "enum", "service"];

/// 段名可解引自检。
pub fn seg_names_audit() -> Result<(), u16> {
    if SEG_NAMES.len() != N_SEG {
        return Err(E_SEG);
    }
    for n in SEG_NAMES {
        if n.is_empty() {
            return Err(E_ROW);
        }
    }
    Ok(())
}

/// 机级聚合账：每机净差值（Σ四段 delta）与主导注记。
#[derive(Clone, Debug)]
pub struct MachAgg {
    pub mach: usize,
    /// Σ四段 delta（ms）。
    pub net_delta: i64,
    /// 优化行数 / 退化行数 / 波动行数。
    pub n_opt: u32,
    pub n_reg: u32,
    pub n_noise: u32,
}

impl MachAgg {
    /// 主导注记判定：退化 > 优化 → 回归主导；优化 > 退化 → 优化主导；相等 → 持平。
    pub fn dominant(&self) -> AttrNote {
        if self.n_reg > self.n_opt {
            AttrNote::Regressed
        } else if self.n_opt > self.n_reg {
            AttrNote::Optimized
        } else {
            AttrNote::Noise
        }
    }

    pub fn audit(&self) -> Result<(), u16> {
        if self.mach > 2 {
            return Err(E_SEG);
        }
        if self.n_opt + self.n_reg + self.n_noise != N_SEG as u32 {
            return Err(E_N);
        }
        Ok(())
    }
}

/// 机级聚合落簿（3 行）。
pub struct AggLedger {
    pub rows: Vec<MachAgg>,
}

impl AggLedger {
    pub fn build(led: &DiffLedger) -> Result<AggLedger, u16> {
        led.assert_full()?;
        let mut rows = Vec::new();
        for m in 0..3usize {
            let mut agg = MachAgg {
                mach: m,
                net_delta: 0,
                n_opt: 0,
                n_reg: 0,
                n_noise: 0,
            };
            for r in &led.rows {
                if r.mach == m {
                    agg.net_delta += r.delta;
                    match r.note {
                        AttrNote::Optimized => agg.n_opt += 1,
                        AttrNote::Regressed => agg.n_reg += 1,
                        AttrNote::Noise => agg.n_noise += 1,
                    }
                }
            }
            agg.audit()?;
            rows.push(agg);
        }
        if rows.len() != 3 {
            return Err(E_N);
        }
        Ok(AggLedger { rows })
    }

    /// 聚合对账核：Σ机净差 = Σ行差值（跨层守恒）。
    pub fn hedge(&self, led: &DiffLedger) -> Result<(), u16> {
        let mut s1: i64 = 0;
        let mut s2: i64 = 0;
        for a in &self.rows {
            s1 += a.net_delta;
        }
        for r in &led.rows {
            s2 += r.delta;
        }
        if s1 != s2 {
            return Err(E_ROW);
        }
        Ok(())
    }

    pub fn summary(&self) -> String {
        let mut s = String::from("机级聚合: ");
        for a in &self.rows {
            s.push_str("M");
            s.push_str(&a.mach.to_string());
            s.push_str("净Δ=");
            s.push_str(&a.net_delta.to_string());
            s.push(' ');
        }
        s
    }
}

/// 退化立案簿：每个退化段独立立案（零静默吞退化）。
pub struct RegressCasebook {
    /// (机, 段, 升幅千分比)。
    pub cases: Vec<(usize, usize, u32)>,
    pub sealed: bool,
}

impl RegressCasebook {
    pub fn new() -> RegressCasebook {
        RegressCasebook { cases: Vec::new(), sealed: false }
    }

    /// 从 diff 账立案：所有 Regressed 行逐个入簿。
    pub fn file_from(&mut self, led: &DiffLedger) -> Result<usize, u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        led.assert_full()?;
        self.cases.clear();
        for r in &led.rows {
            if r.note == AttrNote::Regressed {
                self.cases.push((r.mach, r.seg, r.pm));
            }
        }
        Ok(self.cases.len())
    }

    /// 立案核：案件数与 diff 账退化行数相等，升幅全部 ≥ 硬线。
    pub fn verify(&self, led: &DiffLedger) -> Result<(), u16> {
        let mut want: u32 = 0;
        for r in &led.rows {
            if r.note == AttrNote::Regressed {
                want += 1;
            }
        }
        if self.cases.len() as u32 != want {
            return Err(E_NOTE);
        }
        for (m, s, pm) in &self.cases {
            if *m > 2 || *s > 3 {
                return Err(E_SEG);
            }
            if *pm < NOTE_PERMILLE {
                return Err(E_NOTE);
            }
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        if self.cases.is_empty() {
            return Err(E_NOTE);
        }
        self.sealed = true;
        Ok(())
    }
}

/// 立案簿注入核：波动段误立案 → verify 检出（升幅 < 硬线）。
pub fn casebook_inject_audit(led: &DiffLedger) -> Result<(), u16> {
    let mut cb = RegressCasebook::new();
    cb.file_from(led)?;
    // 注入：加入一个波动行（pm=50）
    cb.cases.push((0, 3, 50));
    if cb.verify(led).is_ok() {
        return Err(SEG_BASE + 0x38);
    }
    Ok(())
}

/// diff 账报告（逐行派生 + 分布汇总）。
pub struct DiffReport {
    pub lines: Vec<String>,
}

impl DiffReport {
    pub fn generate(led: &DiffLedger) -> Result<DiffReport, u16> {
        led.assert_notes_spread()?;
        let mut lines = Vec::new();
        lines.push(led.summary());
        let mut n_o: u32 = 0;
        let mut n_r: u32 = 0;
        let mut n_n: u32 = 0;
        for r in &led.rows {
            lines.push(r.summary());
            match r.note {
                AttrNote::Optimized => n_o += 1,
                AttrNote::Regressed => n_r += 1,
                AttrNote::Noise => n_n += 1,
            }
        }
        let mut l = String::from("注记分布: 优化=");
        l.push_str(&n_o.to_string());
        l.push_str(" 退化=");
        l.push_str(&n_r.to_string());
        l.push_str(" 波动=");
        l.push_str(&n_n.to_string());
        lines.push(l);
        Ok(DiffReport { lines })
    }

    pub fn assert_complete(&self) -> Result<(), u16> {
        if self.lines.len() != 14 {
            return Err(E_N);
        }
        for l in &self.lines {
            if l.is_empty() {
                return Err(E_ROW);
            }
        }
        Ok(())
    }
}

/// 自例程：基线只读 + 12 行 diff 落簿 + 注记三态 + 注入全拦截 + 封账 → 0。
pub fn drill() -> Result<u32, u16> {
    seg_names_audit()?;
    attr_rule_audit()?;
    baseline_readonly_audit()?;
    synth_determinism_audit()?;

    let mut led = DiffLedger::new();
    led.build(0x0672)?;
    led.assert_full()?;
    led.assert_notes_spread()?;
    diff_tamper_audit()?;

    // 机级聚合：3 行 + 跨层守恒对账
    let agg = AggLedger::build(&led)?;
    agg.hedge(&led)?;
    if agg.summary().is_empty() {
        return Err(SEG_BASE + 0x39);
    }
    // 退化立案簿：立案 + 复核 + 误立案注入 + 封簿后写拦截
    let mut cb = RegressCasebook::new();
    let n = cb.file_from(&led)?;
    if n == 0 {
        return Err(E_NOTE);
    }
    cb.verify(&led)?;
    casebook_inject_audit(&led)?;
    cb.seal()?;
    if cb.file_from(&led) != Err(E_SEAL) {
        return Err(SEG_BASE + 0x3A);
    }
    // 注入：diff 行数不足 → 审计拦截
    let mut led2 = DiffLedger::new();
    led2.build(0x0672)?;
    led2.rows.pop();
    if led2.assert_full() != Err(E_N) {
        return Err(SEG_BASE + 0x36);
    }
    // 封账 + 封后重建拦截
    led.seal()?;
    if led.build(2) != Err(E_SEAL) {
        return Err(SEG_BASE + 0x37);
    }
    // 报告
    let rep = DiffReport::generate(&led)?;
    rep.assert_complete()?;
    Ok(0)
}
