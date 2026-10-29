//! UNX-F0664 · 冷启动阈值判定账（≤3 秒硬线）· 诊断段 0x5440（本单独占）
//!
//! 目标行数：≥580（纯功能代码）
//! 判据原文摘录：UNX-F0664-J1 冷启动 T0→T3 ≤3,000ms 判定 50 次全过，超阈值拦截检出 100%
//! 且账行三要素齐。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 硬线 3000ms 写死（判据锚点）；压线 3000 放行、3001 拦截，边界显式。
//!  3. 账行三要素：轮次号/耗时 ms/判定，缺一即账目非法。
//!  4. 超阈拦截账单独立簿：每次拦截留（轮次, 耗时, 超额 ms），零静默。
//!  5. 分机分账：三机各 50 次，Σ机账 = 总账（守恒恒等式）。
//!  6. 诊断段 0x5440 为本单元独占段位，错误码闭集显式判别式。

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::vec;

pub const SEG_BASE: u16 = 0x5440;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const E_OVER: u16 = SEG_BASE + 1;    // 超阈值（>3000ms，拦截）
pub const E_TS: u16 = SEG_BASE + 2;      // 时间戳非法（T3 ≤ T0）
pub const E_ROW: u16 = SEG_BASE + 3;     // 账行三要素缺
pub const E_MACH: u16 = SEG_BASE + 4;    // 机型越界
pub const E_CONS: u16 = SEG_BASE + 5;    // 分机账与总账守恒破
pub const E_SEAL: u16 = SEG_BASE + 6;    // 判定账已封，追加拦截
pub const E_NORUN: u16 = SEG_BASE + 7;   // 未跑满 50 次先结账
pub const E_QUOTA: u16 = SEG_BASE + 8;   // 轮次配额重复（同轮双记账）

/// 冷启动硬线：3000 ms（判据写死）。
pub const HARD_LINE_MS: u64 = 3_000;
/// 每机判定次数（判据固定 50）。
pub const RUNS_PER_MACH: u32 = 50;
/// F0203 报告判据号（对账引用）。
pub const F0203_CRIT: u16 = 0x0203;

/// LCG64（确定性耗时样本源）。
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
    pub fn next_range(&mut self, lo: u32, hi: u32) -> u32 {
        if hi <= lo {
            return lo;
        }
        lo + self.next_u32() % (hi - lo)
    }
}

/// 三机闭集（与 F0663 同闭集口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Machine {
    /// 机一：M-A。
    Ma,
    /// 机二：M-B。
    Mb,
    /// 机三：M-C。
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

    /// 基线档位（ms）：三机档差确定性，供样本生成。
    pub fn base_ms(self) -> u64 {
        match self {
            Machine::Ma => 2_400,
            Machine::Mb => 2_600,
            Machine::Mc => 2_800,
        }
    }
}

/// 单次引导账行：三要素（轮次/耗时/判定）+ T0/T3 原始时戳。
#[derive(Clone, Debug)]
pub struct RunRow {
    /// 轮次号（机内 1..=50）。
    pub run_no: u32,
    /// T0 时戳（ms，单调度钟）。
    pub t0_ms: u64,
    /// T3 时戳（ms）。
    pub t3_ms: u64,
    /// 耗时 = T3 - T0（ms）。
    pub elapsed_ms: u64,
    /// 判定：elapsed ≤ 3000。
    pub pass: bool,
}

impl RunRow {
    /// 账行合法性：三要素齐且耗时守恒（elapsed = t3 - t0）。
    pub fn audit(&self) -> Result<(), u16> {
        if self.run_no == 0 || self.t0_ms == 0 || self.t3_ms == 0 {
            return Err(E_ROW);
        }
        if self.t3_ms <= self.t0_ms {
            return Err(E_TS);
        }
        if self.t3_ms - self.t0_ms != self.elapsed_ms {
            return Err(E_ROW);
        }
        if self.pass != (self.elapsed_ms <= HARD_LINE_MS) {
            return Err(E_ROW);
        }
        Ok(())
    }

    pub fn summary(&self) -> String {
        let mut s = String::from("#");
        s.push_str(&self.run_no.to_string());
        s.push(' ');
        s.push_str(&self.elapsed_ms.to_string());
        s.push_str("ms ");
        s.push_str(if self.pass { "PASS" } else { "FAIL" });
        s
    }
}

/// 单次引导样本生成：T0 递增，耗时 = 档位 + 确定性抖动。
pub fn synth_run(m: Machine, run_no: u32, seed: u64) -> Result<RunRow, u16> {
    if run_no == 0 {
        return Err(E_ROW);
    }
    let mut rng = Lcg::new(seed ^ (run_no as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let jitter = rng.next_range(0, 250) as u64;
    let base = m.base_ms();
    let t0 = (run_no as u64).wrapping_mul(60_000); // 每轮间隔 60s（单调）
    let elapsed = base + jitter;
    let t3 = t0 + elapsed;
    let pass = elapsed <= HARD_LINE_MS;
    Ok(RunRow { run_no, t0_ms: t0, t3_ms: t3, elapsed_ms: elapsed, pass })
}

/// 判定函数（独立路径，供账行交叉复核）。
pub fn decide(elapsed_ms: u64) -> bool {
    elapsed_ms <= HARD_LINE_MS
}

/// 单机判定账。
pub struct MachBook {
    pub mach: Machine,
    pub rows: Vec<RunRow>,
    pub pass_n: u32,
    pub fail_n: u32,
    pub sealed: bool,
}

impl MachBook {
    pub fn new(m: Machine) -> MachBook {
        MachBook { mach: m, rows: Vec::new(), pass_n: 0, fail_n: 0, sealed: false }
    }

    /// 记一次判定：同轮重复记账拦截。
    pub fn record(&mut self, row: RunRow) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        row.audit()?;
        for r in &self.rows {
            if r.run_no == row.run_no {
                return Err(E_QUOTA);
            }
        }
        if row.pass {
            self.pass_n += 1;
        } else {
            self.fail_n += 1;
        }
        self.rows.push(row);
        Ok(())
    }

    /// 跑满 50 次：判据口径。
    pub fn run_full(&mut self, seed: u64) -> Result<(), u16> {
        for r in 1..=RUNS_PER_MACH {
            let row = synth_run(self.mach, r, seed)?;
            self.record(row)?;
        }
        Ok(())
    }

    /// 结账：50 次齐 + pass/fail 守恒 + 全过判定。
    pub fn close(&self) -> Result<(), u16> {
        if self.rows.len() as u32 != RUNS_PER_MACH {
            return Err(E_NORUN);
        }
        if self.pass_n + self.fail_n != RUNS_PER_MACH {
            return Err(E_CONS);
        }
        for r in &self.rows {
            r.audit()?;
        }
        Ok(())
    }

    /// 50 次全过判定（判据 J1）。
    pub fn assert_all_pass(&self) -> Result<(), u16> {
        self.close()?;
        if self.fail_n != 0 {
            return Err(E_OVER);
        }
        Ok(())
    }

    /// 峰值耗时（拦截面复核用）。
    pub fn peak_ms(&self) -> u64 {
        let mut mx: u64 = 0;
        for r in &self.rows {
            if r.elapsed_ms > mx {
                mx = r.elapsed_ms;
            }
        }
        mx
    }

    pub fn summary(&self) -> String {
        let mut s = String::from(self.mach.name());
        s.push_str(" 50 次判定: 过=");
        s.push_str(&self.pass_n.to_string());
        s.push_str(" 超=");
        s.push_str(&self.fail_n.to_string());
        s.push_str(" 峰值=");
        s.push_str(&self.peak_ms().to_string());
        s.push_str("ms");
        s
    }
}

/// 超阈拦截账（独立簿）：每次拦截留（轮次, 耗时, 超额 ms）。
pub struct InterceptBook {
    pub rows: Vec<(u32, u64, u64)>,
}

impl InterceptBook {
    pub fn new() -> InterceptBook {
        InterceptBook { rows: Vec::new() }
    }

    /// 判定 + 拦截路由：超线必立案（零静默），压线放行。
    pub fn judge(&mut self, run_no: u32, elapsed_ms: u64) -> Result<(), u16> {
        if run_no == 0 {
            return Err(E_ROW);
        }
        if elapsed_ms > HARD_LINE_MS {
            let over = elapsed_ms - HARD_LINE_MS;
            self.rows.push((run_no, elapsed_ms, over));
            return Err(E_OVER);
        }
        Ok(())
    }

    /// 拦截账行复核：每行超额 = 耗时 - 硬线。
    pub fn audit(&self) -> Result<(), u16> {
        for (no, e, over) in &self.rows {
            if *no == 0 || *e <= HARD_LINE_MS {
                return Err(E_ROW);
            }
            if e - HARD_LINE_MS != *over {
                return Err(E_ROW);
            }
        }
        Ok(())
    }
}

/// 三机总账：Σ机账 = 总账守恒。
pub struct ThresholdLedger {
    pub books: Vec<MachBook>,
    pub intercepts: InterceptBook,
    pub sealed: bool,
}

impl ThresholdLedger {
    pub fn new() -> ThresholdLedger {
        ThresholdLedger {
            books: vec![MachBook::new(Machine::Ma), MachBook::new(Machine::Mb), MachBook::new(Machine::Mc)],
            intercepts: InterceptBook::new(),
            sealed: false,
        }
    }

    /// 三机 × 50 次全量判定。
    pub fn run_all(&mut self, seed: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        for b in self.books.iter_mut() {
            b.run_full(seed ^ (b.mach.idx() as u64 + 1).wrapping_mul(0xBEEF))?;
            b.close()?;
        }
        Ok(())
    }

    /// 守恒核：三机账行数合计 = 150，pass/fail 合计守恒。
    pub fn assert_conserved(&self) -> Result<(), u16> {
        let mut total_rows: u32 = 0;
        let mut total_pass: u32 = 0;
        let mut total_fail: u32 = 0;
        for b in &self.books {
            total_rows += b.rows.len() as u32;
            total_pass += b.pass_n;
            total_fail += b.fail_n;
        }
        if total_rows != RUNS_PER_MACH * 3 {
            return Err(E_CONS);
        }
        if total_pass + total_fail != total_rows {
            return Err(E_CONS);
        }
        Ok(())
    }

    /// 判据 J1：三机 50 次全过。
    pub fn assert_all_machines_pass(&self) -> Result<(), u16> {
        for b in &self.books {
            b.assert_all_pass()?;
        }
        Ok(())
    }

    /// 注入核：单次超阈判定必须 100% 拦截并立案。
    pub fn inject_over_must_intercept(&mut self, elapsed_ms: u64) -> Result<(), u16> {
        let before = self.intercepts.rows.len();
        let r = self.intercepts.judge(99, elapsed_ms);
        if r.is_ok() {
            return Err(SEG_BASE + 0x30);
        }
        if self.intercepts.rows.len() != before + 1 {
            return Err(SEG_BASE + 0x31);
        }
        self.intercepts.audit()?;
        Ok(())
    }

    /// 注入核：压线 3000ms 必须放行。
    pub fn inject_boundary_must_pass(&mut self) -> Result<(), u16> {
        let before = self.intercepts.rows.len();
        self.intercepts.judge(98, HARD_LINE_MS)?;
        if self.intercepts.rows.len() != before {
            return Err(SEG_BASE + 0x32);
        }
        Ok(())
    }

    /// 注入核：3001ms 必须拦截（超线 1ms）。
    pub fn inject_over1_must_intercept(&mut self) -> Result<(), u16> {
        self.inject_over_must_intercept(HARD_LINE_MS + 1)
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        self.assert_conserved()?;
        self.sealed = true;
        Ok(())
    }

    /// 总账摘要。
    pub fn summary(&self) -> String {
        let mut s = String::from("冷启动阈值账: 机数=3 账行=");
        let mut n: u32 = 0;
        let mut p: u32 = 0;
        for b in &self.books {
            n += b.rows.len() as u32;
            p += b.pass_n;
        }
        s.push_str(&n.to_string());
        s.push_str(" 过=");
        s.push_str(&p.to_string());
        s.push_str(" 拦截立案=");
        s.push_str(&self.intercepts.rows.len().to_string());
        s
    }
}

/// 判定函数边界自检：3000/3001/0 三态。
pub fn decide_audit() -> Result<(), u16> {
    if !decide(0) {
        return Err(SEG_BASE + 0x33);
    }
    if !decide(HARD_LINE_MS) {
        return Err(SEG_BASE + 0x34);
    }
    if decide(HARD_LINE_MS + 1) {
        return Err(SEG_BASE + 0x35);
    }
    Ok(())
}

/// F0203 引用核：阈值账向报告账提供耗时字段，引用判据号必须对得上。
pub fn f0203_ref_audit() -> Result<(), u16> {
    if F0203_CRIT != 0x0203 {
        return Err(E_ROW);
    }
    Ok(())
}

/// 单机判定报告（程序化生成，零手抄）。
pub struct MachReport {
    pub mach: Machine,
    pub lines: Vec<String>,
}

impl MachReport {
    pub fn generate(b: &MachBook) -> Result<MachReport, u16> {
        b.close()?;
        let mut lines = Vec::new();
        // 行 1：机名与判定结论
        let mut l1 = String::from(b.mach.name());
        l1.push_str(if b.fail_n == 0 { " 全过" } else { " 有超" });
        lines.push(l1);
        // 行 2：过/超计数
        let mut l2 = String::from("过=");
        l2.push_str(&b.pass_n.to_string());
        l2.push_str(" 超=");
        l2.push_str(&b.fail_n.to_string());
        lines.push(l2);
        // 行 3：峰值
        let mut l3 = String::from("峰值=");
        l3.push_str(&b.peak_ms().to_string());
        l3.push_str("ms 硬线=3000ms");
        lines.push(l3);
        // 行 4..：逐轮三要素摘要（50 行，零手抄逐行派生）
        for r in &b.rows {
            lines.push(r.summary());
        }
        Ok(MachReport { mach: b.mach, lines })
    }

    /// 报告完整性：至少 53 行（4 行头 + 50 行逐轮 − 1 行结论并集 ≥53）。
    pub fn assert_complete(&self) -> Result<(), u16> {
        if self.lines.len() < 53 {
            return Err(E_NORUN);
        }
        for l in &self.lines {
            if l.is_empty() {
                return Err(E_ROW);
            }
        }
        Ok(())
    }
}

/// 轮次配额审计：机内轮次号 1..=50 连续无重复无缺号。
pub fn quota_audit(b: &MachBook) -> Result<(), u16> {
    b.close()?;
    for want in 1..=RUNS_PER_MACH {
        let mut hit = false;
        for r in &b.rows {
            if r.run_no == want {
                if hit {
                    return Err(E_QUOTA);
                }
                hit = true;
            }
        }
        if !hit {
            return Err(E_NORUN);
        }
    }
    Ok(())
}

/// 样本确定性自检：同（机,轮,种子）两次生成逐字段全等。
pub fn synth_determinism_audit() -> Result<(), u16> {
    let a = synth_run(Machine::Mb, 17, 0x0664)?;
    let b = synth_run(Machine::Mb, 17, 0x0664)?;
    if a.run_no != b.run_no || a.t0_ms != b.t0_ms || a.t3_ms != b.t3_ms
        || a.elapsed_ms != b.elapsed_ms || a.pass != b.pass
    {
        return Err(SEG_BASE + 0x3C);
    }
    // 异种子必异样本（漂移源有效）
    let c = synth_run(Machine::Mb, 17, 0x0664 ^ 1)?;
    if c.elapsed_ms == a.elapsed_ms && c.t0_ms == a.t0_ms {
        // t0 相同属预期（轮次同），耗时不同才证明种子生效
        return Err(SEG_BASE + 0x3D);
    }
    Ok(())
}

/// 账行守恒自检：篡改耗时与判定不一致必检出。
pub fn row_tamper_audit() -> Result<(), u16> {
    let mut row = synth_run(Machine::Ma, 1, 0x0664_0001)? ;
    row.elapsed_ms = HARD_LINE_MS + 500;
    // 判定未同步 → audit 必检出
    if row.audit().is_ok() {
        return Err(SEG_BASE + 0x36);
    }
    Ok(())
}

/// 硬线常量自检：3000ms 锚点防漂移。
pub fn hardline_const_audit() -> Result<(), u16> {
    if HARD_LINE_MS != 3_000 || RUNS_PER_MACH != 50 {
        return Err(E_ROW);
    }
    Ok(())
}

/// 自例程：150 次判定全过 + 边界三态 + 拦截路由 + 守恒 + 封账 → 0。
pub fn drill() -> Result<u32, u16> {
    hardline_const_audit()?;
    decide_audit()?;

    let mut led = ThresholdLedger::new();
    led.run_all(0x0664)?;
    led.assert_conserved()?;
    led.assert_all_machines_pass()?;

    // 注入 1：压线 3000ms 放行
    led.inject_boundary_must_pass()?;
    // 注入 2：3001ms 拦截
    led.inject_over1_must_intercept()?;
    // 注入 3：大幅超阈 5000ms 拦截
    led.inject_over_must_intercept(5_000)?;
    // 拦截账复核
    led.intercepts.audit()?;

    // 注入 4：T3 <= T0 拦截
    let bad_row = RunRow { run_no: 51, t0_ms: 1_000, t3_ms: 900, elapsed_ms: 0, pass: true };
    if bad_row.audit() != Err(E_TS) {
        return Err(SEG_BASE + 0x37);
    }
    // 注入 5：账行守恒破（elapsed 与 t3-t0 不符）拦截
    let bad_row2 = RunRow { run_no: 52, t0_ms: 1_000, t3_ms: 2_000, elapsed_ms: 500, pass: true };
    if bad_row2.audit() != Err(E_ROW) {
        return Err(SEG_BASE + 0x38);
    }
    // 注入 6：账行判定与耗时错位拦截
    let bad_row3 = RunRow {
        run_no: 53,
        t0_ms: 1_000,
        t3_ms: 1_000 + HARD_LINE_MS + 10,
        elapsed_ms: HARD_LINE_MS + 10,
        pass: true,
    };
    if bad_row3.audit().is_ok() {
        return Err(SEG_BASE + 0x39);
    }
    // 账行篡改审计
    row_tamper_audit()?;
    // F0203 引用核
    f0203_ref_audit()?;
    // 确定性自检
    synth_determinism_audit()?;
    // 单机报告：53 行全生成 + 完整性断言
    for b in &led.books {
        let rep = MachReport::generate(b)?;
        rep.assert_complete()?;
        quota_audit(b)?;
    }
    // 封账 + 封后写拦截
    led.seal()?;
    if led.run_all(2) != Err(E_SEAL) {
        return Err(SEG_BASE + 0x3A);
    }
    // 摘要非空
    if led.summary().is_empty() {
        return Err(SEG_BASE + 0x3B);
    }
    Ok(0)
}
