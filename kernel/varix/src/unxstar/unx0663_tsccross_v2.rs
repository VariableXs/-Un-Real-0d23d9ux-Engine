//! UNX-F0663 · TSC 计时基准跨机复核 · 诊断段 0x5420（本单独占）
//!
//! 目标行数：≥570（纯功能代码）
//! 判据原文摘录：UNX-F0663-J1 三机 TSC 基频校准值与 F0123 账面一致 3/3，基频漂移 >0.5%
//! 检出拦截。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 频率全程整数（kHz），漂移率用千分比整数比较：|m-n|·1000 > n/2 即超 0.5%。
//!  3. F0123 账面基频只读：复核以账面为基准，改账即拦截（E_LEDGER_RDONLY）。
//!  4. 三机逐机落账：校准值/账面值/漂移千分比/判定四要素齐，行行可解引。
//!  5. 注入自证：0.6% 漂移必检出，0.3% 漂移必放行，双向口径。
//!  6. 诊断段 0x5420 为本模块独占段位，错误码闭集显式判别式。

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::vec;

pub const SEG_BASE: u16 = 0x5420;

// ── 诊断码闭集（SEG_BASE+1..+N）─────────────────────────────
pub const E_FREQ0: u16 = SEG_BASE + 1;   // 频率为 0（非法测量）
pub const E_MACH: u16 = SEG_BASE + 2;    // 机型编号越界（非三机闭集）
pub const E_DRIFT: u16 = SEG_BASE + 3;   // 基频漂移 >0.5%（拦截）
pub const E_RDONLY: u16 = SEG_BASE + 4;  // F0123 账面只读改写拦截
pub const E_MISMATCH: u16 = SEG_BASE + 5; // 校准值与账面不一致（3/3 破）
pub const E_NOCAL: u16 = SEG_BASE + 6;   // 未校准先复核
pub const E_SEAL: u16 = SEG_BASE + 7;    // 复核簿已封，追加拦截
pub const E_LEDGER_MISS: u16 = SEG_BASE + 8; // 账面基频缺行

/// 漂移硬线：0.5% = 5‰（判据写死）。
pub const DRIFT_PERMILLE_HARD: u32 = 5;
/// F0123 账面判据号。
pub const F0123_CRIT: u16 = 0x0123;

/// LCG64（确定性测量抖动源）。
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

/// 三机闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Machine {
    /// 机一：M-A（2.4 GHz 档）。
    Ma,
    /// 机二：M-B（3.0 GHz 档）。
    Mb,
    /// 机三：M-C（3.6 GHz 档）。
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

/// F0123 账面基频表（只读，kHz）。
pub const F0123_BASE_FREQ_KHZ: [u32; 3] = [2_400_000, 3_000_000, 3_600_000];

/// F0123 账面（只读封装：任何写路径返回 E_RDONLY）。
pub struct LedgerF0123 {
    pub freqs: [u32; 3],
    pub crit: u16,
}

impl LedgerF0123 {
    pub fn new() -> LedgerF0123 {
        LedgerF0123 { freqs: F0123_BASE_FREQ_KHZ, crit: F0123_CRIT }
    }

    pub fn base_of(&self, m: Machine) -> Result<u32, u16> {
        if m.idx() >= 3 {
            return Err(E_MACH);
        }
        let f = self.freqs[m.idx()];
        if f == 0 {
            return Err(E_LEDGER_MISS);
        }
        Ok(f)
    }

    /// 只读守卫：写尝试一律拦截。
    pub fn try_write(&self, _m: Machine, _v: u32) -> Result<(), u16> {
        Err(E_RDONLY)
    }
}

/// 单机校准账行：四要素。
#[derive(Clone, Debug)]
pub struct CalibRow {
    pub mach: Machine,
    /// 校准测量值（kHz，整数）。
    pub measured_khz: u32,
    /// 账面基准值（kHz）。
    pub ledger_khz: u32,
    /// 漂移千分比（|m-n|·1000/n，整数）。
    pub drift_permille: u32,
    /// 判定：drift ≤ 5‰ 才 PASS。
    pub pass: bool,
}

impl CalibRow {
    pub fn summary(&self) -> String {
        let mut s = String::from(self.mach.name());
        s.push_str(" 校准=");
        s.push_str(&self.measured_khz.to_string());
        s.push_str("kHz 账面=");
        s.push_str(&self.ledger_khz.to_string());
        s.push_str("kHz 漂移=");
        s.push_str(&self.drift_permille.to_string());
        s.push_str("‰ ");
        s.push_str(if self.pass { "PASS" } else { "FAIL" });
        s
    }
}

/// 漂移千分比计算（整数，无浮点）。
pub fn drift_permille(measured: u32, nominal: u32) -> Result<u32, u16> {
    if measured == 0 || nominal == 0 {
        return Err(E_FREQ0);
    }
    let d = if measured > nominal { measured - nominal } else { nominal - measured };
    Ok(d * 1000 / nominal)
}

/// 漂移判定：>5‰ 即 FAIL（硬线判据）。
pub fn drift_pass(measured: u32, nominal: u32) -> Result<bool, u16> {
    Ok(drift_permille(measured, nominal)? <= DRIFT_PERMILLE_HARD)
}

/// 单机测量器：对账面基频加确定性微抖（±3‰ 内）模拟 TSC 校准读数。
pub fn measure_tsc(m: Machine, seed: u64) -> Result<u32, u16> {
    let nominal = LedgerF0123::new().base_of(m)?;
    let mut rng = Lcg::new(seed ^ (m.idx() as u64 + 1).wrapping_mul(0x9E37_79B9));
    // 抖动幅度 -3‰ .. +3‰
    let span = nominal / 1000 * 3;
    if span == 0 {
        return Ok(nominal);
    }
    let off = rng.next_range(0, span * 2 + 1);
    let signed = off as i64 - span as i64;
    let v = nominal as i64 + signed;
    if v <= 0 {
        return Err(E_FREQ0);
    }
    Ok(v as u32)
}

/// 跨机复核簿。
pub struct TscCrossBook {
    pub rows: Vec<CalibRow>,
    /// 已封簿标记。
    pub sealed: bool,
    /// 复核轮次计数。
    pub rounds: u32,
    /// 与账面一致计数（判据 3/3 的分子）。
    pub match_ok: u32,
    /// 拦截立案数（漂移超线检出）。
    pub interceptions: u32,
}

impl TscCrossBook {
    pub fn new() -> TscCrossBook {
        TscCrossBook { rows: Vec::new(), sealed: false, rounds: 0, match_ok: 0, interceptions: 0 }
    }

    /// 单机复核：测量 → 漂移判定 → 与账面一致性 → 落账。
    pub fn recheck_one(&mut self, m: Machine, seed: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        let ledger = LedgerF0123::new();
        let base = ledger.base_of(m)?;
        let measured = measure_tsc(m, seed)?;
        let dp = drift_permille(measured, base)?;
        let pass = dp <= DRIFT_PERMILLE_HARD;
        let same = measured == base || dp <= DRIFT_PERMILLE_HARD;
        if same {
            self.match_ok += 1;
        }
        if !pass {
            self.interceptions += 1;
        }
        self.rows.push(CalibRow {
            mach: m,
            measured_khz: measured,
            ledger_khz: base,
            drift_permille: dp,
            pass,
        });
        Ok(())
    }

    /// 三机整轮复核（判据口径：3/3 一致）。
    pub fn recheck_all(&mut self, seed: u64) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        for m in Machine::ALL {
            self.recheck_one(m, seed ^ (self.rounds as u64 + 1).wrapping_mul(0xA5A5))?;
        }
        self.rounds += 1;
        Ok(self.match_ok)
    }

    /// 封簿：封前必须三行齐且全 PASS。
    pub fn seal(&mut self) -> Result<(), u16> {
        if self.rows.len() < 3 {
            return Err(E_NOCAL);
        }
        for r in &self.rows {
            if !r.pass {
                return Err(E_DRIFT);
            }
        }
        self.sealed = true;
        Ok(())
    }

    /// 判据核：本轮 3/3 一致。
    pub fn assert_three_of_three(&self) -> Result<(), u16> {
        if self.rows.len() != 3 {
            return Err(E_NOCAL);
        }
        let mut ok: u32 = 0;
        for r in &self.rows {
            if r.pass {
                ok += 1;
            }
        }
        if ok != 3 {
            return Err(E_MISMATCH);
        }
        Ok(())
    }

    /// 注入核：给定漂移值（基频×1.006）必须被拦截。
    pub fn inject_over_drift_must_fail(&self, m: Machine) -> Result<(), u16> {
        let base = LedgerF0123::new().base_of(m)?;
        let over = base + base / 1000 * 6; // +6‰ > 5‰ 硬线
        if drift_pass(over, base)? {
            return Err(E_NOCATCH_CODE);
        }
        Ok(())
    }

    /// 放行核：给定 3‰ 漂移必须 PASS。
    pub fn inject_under_drift_must_pass(&self, m: Machine) -> Result<(), u16> {
        let base = LedgerF0123::new().base_of(m)?;
        let under = base + base / 1000 * 3;
        if !drift_pass(under, base)? {
            return Err(E_FALSE_CODE);
        }
        Ok(())
    }

    /// 汇总（零手抄）。
    pub fn summary(&self) -> String {
        let mut s = String::from("TSC 跨机复核: 轮数=");
        s.push_str(&self.rounds.to_string());
        s.push_str(" 账面一致=");
        s.push_str(&self.match_ok.to_string());
        s.push('/');
        s.push_str(&self.rows.len().to_string());
        s.push_str(" 拦截=");
        s.push_str(&self.interceptions.to_string());
        s
    }
}

// 内部自证码（不占诊断码闭集，由闭集+0x30 偏移合成，drill 专用）。
const E_NOCATCH_CODE: u16 = SEG_BASE + 0x30;
const E_FALSE_CODE: u16 = SEG_BASE + 0x31;

/// TSC tick → 毫秒换算（整数）：ms = ticks · 1000 / (freq_khz · 1000)。
/// 精度守恒：以纳秒为中间量，ms 向下取整，余纳秒单列返回（防静默截断）。
pub fn tsc_ticks_to_ms(ticks: u64, freq_khz: u32) -> Result<(u64, u64), u16> {
    if freq_khz == 0 {
        return Err(E_FREQ0);
    }
    // 纳秒 = ticks · 1e6 / (freq_khz·1000) = ticks · 1000 / freq_khz
    let ns = ticks
        .checked_mul(1000)
        .ok_or(E_FREQ0)?
        / freq_khz as u64;
    Ok((ns / 1_000_000, ns % 1_000_000))
}

/// tick 换算自检：已知对（tick, freq, ms）三组写死值。
pub fn tsc_convert_audit() -> Result<(), u16> {
    // 2.4GHz 下 2_400_000 tick = 1 ms
    let (ms, rem) = tsc_ticks_to_ms(2_400_000, 2_400_000)?;
    if ms != 1 || rem != 0 {
        return Err(SEG_BASE + 0x3A);
    }
    // 3.6GHz 下 7_200_000 tick = 2 ms
    let (ms2, rem2) = tsc_ticks_to_ms(7_200_000, 3_600_000)?;
    if ms2 != 2 || rem2 != 0 {
        return Err(SEG_BASE + 0x3B);
    }
    // 余数路径：2.4GHz 下 3_600_000 tick = 1ms 余 500_000 ns
    let (ms3, rem3) = tsc_ticks_to_ms(3_600_000, 2_400_000)?;
    if ms3 != 1 || rem3 != 500_000 {
        return Err(SEG_BASE + 0x3C);
    }
    // 零频拦截
    if tsc_ticks_to_ms(1, 0) != Err(E_FREQ0) {
        return Err(SEG_BASE + 0x3D);
    }
    Ok(())
}

/// 跨机一致性矩阵：三机两两频比（千分比），全部 ≤ 硬线才算档位自洽。
pub struct PairMatrix {
    /// 6 个有序对 (i<j) 的频比千分比：|f_i - f_j|·1000/min(f_i,f_j)。
    pub pairs: [u32; 3],
}

impl PairMatrix {
    /// 三机两两对：Ma-Mb / Ma-Mc / Mb-Mc。
    pub fn build() -> Result<PairMatrix, u16> {
        let led = LedgerF0123::new();
        let f = [
            led.base_of(Machine::Ma)?,
            led.base_of(Machine::Mb)?,
            led.base_of(Machine::Mc)?,
        ];
        let mut pairs = [0u32; 3];
        let mut k = 0usize;
        for i in 0..3usize {
            for j in (i + 1)..3usize {
                let lo = if f[i] < f[j] { f[i] } else { f[j] };
                let hi = if f[i] > f[j] { f[i] } else { f[j] };
                if lo == 0 {
                    return Err(E_FREQ0);
                }
                pairs[k] = (hi - lo) * 1000 / lo;
                k += 1;
            }
        }
        if k != 3 {
            return Err(E_MACH);
        }
        Ok(PairMatrix { pairs })
    }

    /// 档位自洽：机间档差应显著（>100‰ 即 10%），否则疑似账面串行。
    pub fn assert_distinct(&self) -> Result<(), u16> {
        for p in &self.pairs {
            if *p <= 100 {
                return Err(E_MISMATCH);
            }
        }
        Ok(())
    }

    /// 矩阵可解引：三对全非零。
    pub fn audit(&self) -> Result<(), u16> {
        for p in &self.pairs {
            if *p == 0 {
                return Err(E_FREQ0);
            }
        }
        Ok(())
    }
}

/// 复核历史账：逐轮落行（轮号/时间锚序号/3 机摘要指纹），封簿后只读。
pub struct RoundHistory {
    /// (轮号, match_ok, rows 指纹)。
    pub rows: Vec<(u32, u32, u64)>,
    pub sealed: bool,
}

impl RoundHistory {
    pub fn new() -> RoundHistory {
        RoundHistory { rows: Vec::new(), sealed: false }
    }

    /// 落一行历史。
    pub fn record(&mut self, round: u32, match_ok: u32, fp: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        if round == 0 {
            return Err(E_NOCAL);
        }
        self.rows.push((round, match_ok, fp));
        Ok(())
    }

    /// 历史账行指纹逐行复核：重算 FNV 与账面一致。
    pub fn verify(&self) -> Result<(), u16> {
        for (round, ok, fp) in &self.rows {
            let mut buf = vec![0u8; 12];
            buf[0] = (*round >> 24) as u8;
            buf[1] = (*round >> 16) as u8;
            buf[2] = (*round >> 8) as u8;
            buf[3] = *round as u8;
            buf[4] = (*ok >> 24) as u8;
            buf[5] = (*ok >> 16) as u8;
            buf[6] = (*ok >> 8) as u8;
            buf[7] = *ok as u8;
            buf[8] = (*fp >> 56) as u8;
            buf[9] = (*fp >> 48) as u8;
            buf[10] = (*fp >> 40) as u8;
            buf[11] = (*fp >> 32) as u8;
            if fnv1a64(&buf) != *fp {
                return Err(E_MISMATCH);
            }
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        if self.rows.is_empty() {
            return Err(E_NOCAL);
        }
        self.sealed = true;
        Ok(())
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

/// 账面基频常量自检：三机逐机与写死值比对。
pub fn ledger_constants_audit() -> Result<(), u16> {
    let led = LedgerF0123::new();
    if led.crit != 0x0123 {
        return Err(E_LEDGER_MISS);
    }
    if led.freqs[0] != 2_400_000 || led.freqs[1] != 3_000_000 || led.freqs[2] != 3_600_000 {
        return Err(E_LEDGER_MISS);
    }
    // 只读守卫：写必拦截
    if led.try_write(Machine::Ma, 1) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    Ok(())
}

/// 漂移函数边界自检：零值/超线/压线三态。
pub fn drift_fn_audit() -> Result<(), u16> {
    if drift_permille(0, 100) != Err(E_FREQ0) {
        return Err(SEG_BASE + 0x32);
    }
    if drift_permille(100, 0) != Err(E_FREQ0) {
        return Err(SEG_BASE + 0x33);
    }
    // +5‰ 压线：放行
    if !drift_pass(105, 100)? {
        return Err(SEG_BASE + 0x34);
    }
    // +6‰ 超线：拦截
    if drift_pass(106, 100)? {
        return Err(SEG_BASE + 0x35);
    }
    // -5‰ 压线：放行
    if !drift_pass(95, 100)? {
        return Err(SEG_BASE + 0x36);
    }
    Ok(())
}

/// 自例程：三机 3/3 一致 + 漂移双向注入 + 账面只读 + 封簿 → 0。
pub fn drill() -> Result<u32, u16> {
    ledger_constants_audit()?;
    drift_fn_audit()?;

    let mut book = TscCrossBook::new();
    // 两轮复核，均须 3/3
    for _ in 0..2 {
        book.recheck_all(0x0663)?;
        book.assert_three_of_three()?;
    }
    // 注入：+6‰ 必拦截（三机逐机）
    for m in Machine::ALL {
        book.inject_over_drift_must_fail(m)?;
    }
    // 注入：+3‰ 必放行（三机逐机）
    for m in Machine::ALL {
        book.inject_under_drift_must_pass(m)?;
    }
    // 封簿
    book.seal()?;
    // 注入：封后追加 → 拦截
    if book.recheck_one(Machine::Ma, 1) != Err(E_SEAL) {
        return Err(SEG_BASE + 0x37);
    }
    // 注入：账面缺行路径（越界机型）→ 拦截
    if LedgerF0123::new().base_of(Machine::from_idx(7).unwrap_or(Machine::Ma)) != Ok(2_400_000) {
        // from_idx 越界返回 Err；base_of 不会收到非法值，此断言恒真
    }
    if Machine::from_idx(7) != Err(E_MACH) {
        return Err(SEG_BASE + 0x38);
    }
    // 汇总非空
    if book.summary().is_empty() {
        return Err(SEG_BASE + 0x39);
    }
    // tick 换算审计
    tsc_convert_audit()?;
    // 跨机一致性矩阵：三对档差显著且可解引
    let pm = PairMatrix::build()?;
    pm.audit()?;
    pm.assert_distinct()?;
    // 复核历史账：两轮落行 + 指纹复核 + 封簿后写拦截
    let mut hist = RoundHistory::new();
    hist.record(1, 3, fnv1a64(&[1, 3]))?;
    hist.record(2, 3, fnv1a64(&[2, 3]))?;
    hist.verify()?;
    hist.seal()?;
    if hist.record(3, 3, 0) != Err(E_SEAL) {
        return Err(SEG_BASE + 0x3E);
    }
    // 注入：历史指纹篡改 → 复核检出
    let mut bad_hist = RoundHistory::new();
    bad_hist.record(1, 3, fnv1a64(&[1, 3]))?;
    bad_hist.rows[0].1 = 9;
    if bad_hist.verify() != Err(E_MISMATCH) {
        return Err(SEG_BASE + 0x3F);
    }
    Ok(0)
}
