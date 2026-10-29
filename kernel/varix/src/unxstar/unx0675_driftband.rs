//! UNX-F0675 · 长稳耗时漂移监测（连续引导漂移带）· 诊断段 0x55a0（本单独占）
//!
//! 目标行数：≥580（纯功能代码）
//! 判据原文摘录：UNX-F0675-J1 连续 200 次引导耗时漂移带（滑动窗口 20 次 p75 带）落账，
//! 带外漂移检出 100% 且漂移归因两类路由。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 漂移带：首窗（20 次）p75 为上带、p25 为下带，后续窗口逐窗比对。
//!  3. 带外判定：窗 p75 > 上带 + 容差 或 窗 p25 < 下带 − 容差（容差 50‰ 整数）。
//!  4. 归因两类：向上漂移（热漂）/ 向下漂移（冷漂），二选一路由，零模糊。
//!  5. 带外必立案：逐窗监测账行（窗号/带值/观测/方向），零静默。
//!  6. 诊断段 0x55a0 为本单元独占段位，错误码闭集显式判别式。

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::vec;

pub const SEG_BASE: u16 = 0x55a0;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const E_N: u16 = SEG_BASE + 1;      // 样本量不足（≠200）
pub const E_WINDOW: u16 = SEG_BASE + 2; // 窗口非法（≠20 或为 0）
pub const E_BAND: u16 = SEG_BASE + 3;   // 漂移带非法（带倒挂）
pub const E_UNATTR: u16 = SEG_BASE + 4; // 带外未归因（零静默拦截）
pub const E_ROW: u16 = SEG_BASE + 5;    // 监测账行要素缺
pub const E_SEAL: u16 = SEG_BASE + 6;   // 监测账已封
pub const E_FALSE: u16 = SEG_BASE + 7;  // 带内误报

/// 连续引导次数。
pub const N_BOOTS: usize = 200;
/// 滑动窗口宽。
pub const WINDOW: usize = 20;
/// 容差（千分比）。
pub const TOL_PM: u32 = 50;

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

/// 排序法分位（冒泡 + 下标）。
pub fn percentile(samples: &[u64], permille: u32) -> Result<u64, u16> {
    if samples.is_empty() {
        return Err(E_WINDOW);
    }
    if permille > 1000 {
        return Err(E_BAND);
    }
    let mut v = samples.to_vec();
    let n = v.len();
    for i in 0..n {
        for j in 0..(n - 1 - i) {
            if v[j] > v[j + 1] {
                let t = v[j];
                v[j] = v[j + 1];
                v[j + 1] = t;
            }
        }
    }
    let idx = ((n as u64 - 1) * permille as u64 / 1000) as usize;
    Ok(v[idx])
}

/// 漂移带。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriftBand {
    pub lo: u64,
    pub hi: u64,
}

impl DriftBand {
    /// 带审计：lo ≤ hi 且非零。
    pub fn audit(&self) -> Result<(), u16> {
        if self.lo == 0 || self.lo > self.hi {
            return Err(E_BAND);
        }
        Ok(())
    }

    /// 带内判定（含 ±TOL‰ 容差）。
    pub fn contains(&self, x: u64) -> bool {
        let tol_hi = self.hi + self.hi as u64 * TOL_PM as u64 / 1000;
        let tol_lo = self.lo.saturating_sub(self.lo * TOL_PM as u64 / 1000);
        x >= tol_lo && x <= tol_hi
    }
}

/// 首窗建带：p25 下带 / p75 上带。
pub fn build_band(first_window: &[u64]) -> Result<DriftBand, u16> {
    if first_window.len() != WINDOW {
        return Err(E_WINDOW);
    }
    let lo = percentile(first_window, 250)?;
    let hi = percentile(first_window, 750)?;
    let b = DriftBand { lo, hi };
    b.audit()?;
    Ok(b)
}

/// 漂移方向闭集两类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriftDir {
    /// 向上漂移（热漂）：观测 > 上带 + 容差。
    Up,
    /// 向下漂移（冷漂）：观测 < 下带 − 容差。
    Down,
}

impl DriftDir {
    pub fn name(self) -> &'static str {
        match self {
            DriftDir::Up => "向上漂移",
            DriftDir::Down => "向下漂移",
        }
    }

    /// 方向判定（纯函数，零模糊）。
    pub fn of(band: &DriftBand, x: u64) -> Result<Option<DriftDir>, u16> {
        band.audit()?;
        if band.contains(x) {
            return Ok(None);
        }
        if x > band.hi {
            return Ok(Some(DriftDir::Up));
        }
        Ok(Some(DriftDir::Down))
    }
}

/// 200 次确定性样本流（可注入漂移段）。
pub fn synth_series(seed: u64, drift_at: Option<(usize, u64)>) -> Result<Vec<u64>, u16> {
    if drift_at.is_some() && drift_at.unwrap_or((0, 0)).0 >= N_BOOTS {
        return Err(E_N);
    }
    let mut rng = Lcg::new(seed);
    let mut out = Vec::with_capacity(N_BOOTS);
    for i in 0..N_BOOTS {
        let mut x = 2_200 + (rng.next_u32() % 200) as u64;
        if let Some((at, delta)) = drift_at {
            if i >= at {
                x += delta;
            }
        }
        out.push(x);
    }
    Ok(out)
}

/// 监测账行。
#[derive(Clone, Debug)]
pub struct MonitorRow {
    /// 窗号（1 起）。
    pub win_no: u32,
    /// 窗 p25。
    pub p25: u64,
    /// 窗 p75。
    pub p75: u64,
    /// 带外观测（首值出带即记）。
    pub outlier_val: u64,
    /// 方向（带内 None）。
    pub dir: Option<DriftDir>,
}

impl MonitorRow {
    pub fn audit(&self) -> Result<(), u16> {
        if self.win_no == 0 || self.p25 > self.p75 {
            return Err(E_ROW);
        }
        if self.dir.is_none() && self.outlier_val != 0 {
            return Err(E_ROW);
        }
        if self.dir.is_some() && self.outlier_val == 0 {
            return Err(E_ROW);
        }
        Ok(())
    }
}

/// 漂移监测器。
pub struct DriftMonitor {
    pub band: DriftBand,
    pub rows: Vec<MonitorRow>,
    pub sealed: bool,
}

impl DriftMonitor {
    pub fn new(series: &[u64]) -> Result<DriftMonitor, u16> {
        if series.len() != N_BOOTS {
            return Err(E_N);
        }
        let band = build_band(&series[..WINDOW])?;
        Ok(DriftMonitor { band, rows: Vec::new(), sealed: false })
    }

    /// 滑窗扫描：窗步 10（半重叠），每窗判 p75/p25 出带。
    pub fn scan(&mut self, series: &[u64]) -> Result<usize, u16> {
        if series.len() != N_BOOTS {
            return Err(E_N);
        }
        if self.sealed {
            return Err(E_SEAL);
        }
        self.rows.clear();
        let mut win_no: u32 = 0;
        let mut start = WINDOW; // 首窗为建带窗，从第二窗起监测
        while start + WINDOW <= N_BOOTS {
            win_no += 1;
            let w = &series[start..start + WINDOW];
            let p25 = percentile(w, 250)?;
            let p75 = percentile(w, 750)?;
            // 窗代表值：p75 与 p25 各判一次，先出带者记
            let mut dir = None;
            let mut ov: u64 = 0;
            if let Some(d) = DriftDir::of(&self.band, p75)? {
                dir = Some(d);
                ov = p75;
            } else if let Some(d) = DriftDir::of(&self.band, p25)? {
                dir = Some(d);
                ov = p25;
            }
            self.rows.push(MonitorRow { win_no, p25, p75, outlier_val: ov, dir });
            start += WINDOW / 2;
        }
        Ok(self.rows.iter().filter(|r| r.dir.is_some()).count())
    }

    /// 带外归因核：每条带外账行方向非空且与带几何一致。
    pub fn assert_attributed(&self) -> Result<(), u16> {
        for r in &self.rows {
            r.audit()?;
            if let Some(d) = r.dir {
                match d {
                    DriftDir::Up => {
                        if r.outlier_val <= self.band.hi {
                            return Err(E_UNATTR);
                        }
                    }
                    DriftDir::Down => {
                        if r.outlier_val >= self.band.lo {
                            return Err(E_UNATTR);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// 注入检出核：漂移注入样本必检出 ≥1 窗带外。
    pub fn inject_drift_must_catch(series: &[u64]) -> Result<(), u16> {
        let mut m = DriftMonitor::new(series)?;
        let n = m.scan(series)?;
        if n == 0 {
            return Err(E_UNATTR);
        }
        m.assert_attributed()?;
        Ok(())
    }

    /// 带内误报核：纯净样本必须零带外。
    pub fn inject_clean_must_pass(series: &[u64]) -> Result<(), u16> {
        let mut m = DriftMonitor::new(series)?;
        let n = m.scan(series)?;
        if n != 0 {
            return Err(E_FALSE);
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        self.assert_attributed()?;
        self.sealed = true;
        Ok(())
    }

    pub fn summary(&self) -> String {
        let mut s = String::from("漂移带=[");
        s.push_str(&self.band.lo.to_string());
        s.push(',');
        s.push_str(&self.band.hi.to_string());
        s.push_str("] 窗数=");
        s.push_str(&self.rows.len().to_string());
        s.push_str(" 带外=");
        let n = self.rows.iter().filter(|r| r.dir.is_some()).count();
        s.push_str(&n.to_string());
        s
    }
}

/// 方向归因注入核 A：后段 +600ms 上漂必检出且方向 = Up。
pub fn inject_up_must_catch() -> Result<(), u16> {
    let s = synth_series(0x0675, Some((60, 600)))?;
    let mut m = DriftMonitor::new(&s)?;
    let n = m.scan(&s)?;
    if n == 0 {
        return Err(E_UNATTR);
    }
    let mut any_up = false;
    for r in &m.rows {
        if r.dir == Some(DriftDir::Up) {
            any_up = true;
        }
    }
    if !any_up {
        return Err(E_UNATTR);
    }
    Ok(())
}

/// 方向归因注入核 B：后段下压必检出且方向 = Down。
pub fn inject_down_must_catch() -> Result<(), u16> {
    // 下压 300ms（基线带内波动 200，压出下带）
    let s = synth_series(0x0675, Some((120, 0)))?;
    // 手工下压后段
    let mut s2 = s;
    for i in 120..N_BOOTS {
        s2[i] = s2[i].saturating_sub(400);
    }
    let mut m = DriftMonitor::new(&s2)?;
    let n = m.scan(&s2)?;
    if n == 0 {
        return Err(E_UNATTR);
    }
    let mut any_down = false;
    for r in &m.rows {
        if r.dir == Some(DriftDir::Down) {
            any_down = true;
        }
    }
    if !any_down {
        return Err(E_UNATTR);
    }
    Ok(())
}

/// 分位函数自检：已知序（1..=20）p25=6、p75=15（0 起下标 4 与 14）。
pub fn percentile_audit() -> Result<(), u16> {
    let mut v = Vec::new();
    for i in 1..=20u64 {
        v.push(i);
    }
    if percentile(&v, 250)? != 6 {
        return Err(E_BAND);
    }
    if percentile(&v, 750)? != 15 {
        return Err(E_BAND);
    }
    if percentile(&[], 500) != Err(E_WINDOW) {
        return Err(SEG_BASE + 0x30);
    }
    if percentile(&v, 1001) != Err(E_BAND) {
        return Err(SEG_BASE + 0x31);
    }
    Ok(())
}

/// 带几何自检：x>hi → Up；x<lo → Down；带内 → None。
pub fn dir_audit() -> Result<(), u16> {
    let b = DriftBand { lo: 100, hi: 200 };
    b.audit()?;
    if DriftDir::of(&b, 250)?.is_none() {
        return Err(E_UNATTR);
    }
    if DriftDir::of(&b, 50)?.is_none() {
        return Err(E_UNATTR);
    }
    if DriftDir::of(&b, 150)?.is_some() {
        return Err(E_FALSE);
    }
    let bad = DriftBand { lo: 300, hi: 200 };
    if bad.audit() != Err(E_BAND) {
        return Err(SEG_BASE + 0x32);
    }
    Ok(())
}

/// 样本确定性自检。
pub fn synth_determinism_audit() -> Result<(), u16> {
    let a = synth_series(0x0675, None)?;
    let b = synth_series(0x0675, None)?;
    if a.len() != N_BOOTS || b.len() != N_BOOTS {
        return Err(E_N);
    }
    for i in 0..N_BOOTS {
        if a[i] != b[i] {
            return Err(E_ROW);
        }
    }
    Ok(())
}

/// 建带确定性自检：同序列两次建带全等。
pub fn band_determinism_audit(series: &[u64]) -> Result<(), u16> {
    let b1 = build_band(&series[..WINDOW])?;
    let b2 = build_band(&series[..WINDOW])?;
    if b1 != b2 {
        return Err(E_BAND);
    }
    b1.audit()?;
    Ok(())
}

/// 窗指纹簿：逐窗（窗号, p25, p75, 指纹）登记，对账只读。
pub struct WinFpBook {
    pub entries: Vec<(u32, u64, u64, u64)>,
    pub sealed: bool,
}

impl WinFpBook {
    pub fn new() -> WinFpBook {
        WinFpBook { entries: Vec::new(), sealed: false }
    }

    pub fn record(&mut self, win: u32, p25: u64, p75: u64, fp: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        if win == 0 || p25 > p75 || fp == 0 {
            return Err(E_ROW);
        }
        for (w, _, _, _) in &self.entries {
            if w == &win {
                return Err(E_ROW);
            }
        }
        self.entries.push((win, p25, p75, fp));
        Ok(())
    }

    /// 对账：与监测账逐窗全等（含指纹现算比对）。
    pub fn verify(&self, m: &DriftMonitor) -> Result<(), u16> {
        if self.entries.len() != m.rows.len() {
            return Err(E_ROW);
        }
        for (i, (w, p25, p75, fp)) in self.entries.iter().enumerate() {
            let r = &m.rows[i];
            if r.win_no != *w || r.p25 != *p25 || r.p75 != *p75 {
                return Err(E_ROW);
            }
            if win_fp(*w, *p25, *p75) != *fp {
                return Err(E_SEM_CODE);
            }
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        if self.entries.is_empty() {
            return Err(E_ROW);
        }
        self.sealed = true;
        Ok(())
    }
}

/// 窗指纹：三值拼装 FNV。
pub fn win_fp(win: u32, p25: u64, p75: u64) -> u64 {
    let mut buf = vec![0u8; 20];
    buf[0] = (win >> 24) as u8;
    buf[1] = (win >> 16) as u8;
    buf[2] = (win >> 8) as u8;
    buf[3] = win as u8;
    for (i, v) in [p25, p75].iter().enumerate() {
        for k in 0..8u8 {
            buf[4 + i * 8 + k as usize] = (v >> (56 - k * 8)) as u8;
        }
    }
    fnv1a64(&buf)
}

// 内部自证码（drill 专用，闭集+0x40 偏移）。
const E_SEM_CODE: u16 = SEG_BASE + 0x40;

/// 归因汇总：两类方向计数（向上/向下），零模糊（不存第三类）。
pub struct DirSummary {
    pub up: u32,
    pub down: u32,
}

impl DirSummary {
    pub fn count(m: &DriftMonitor) -> DirSummary {
        let mut s = DirSummary { up: 0, down: 0 };
        for r in &m.rows {
            match r.dir {
                Some(DriftDir::Up) => s.up += 1,
                Some(DriftDir::Down) => s.down += 1,
                None => {}
            }
        }
        s
    }

    /// 汇总守恒：两类和 = 带外窗数。
    pub fn conserved(&self, m: &DriftMonitor) -> Result<(), u16> {
        let out_n = m.rows.iter().filter(|r| r.dir.is_some()).count() as u32;
        if self.up + self.down != out_n {
            return Err(E_UNATTR);
        }
        Ok(())
    }

    pub fn summary(&self) -> String {
        let mut s = String::from("归因: ");
        s.push_str(DriftDir::Up.name());
        s.push('=');
        s.push_str(&self.up.to_string());
        s.push(' ');
        s.push_str(DriftDir::Down.name());
        s.push('=');
        s.push_str(&self.down.to_string());
        s
    }
}

/// 监测报告（逐窗派生）。
pub struct MonitorReport {
    pub lines: Vec<String>,
}

impl MonitorReport {
    pub fn generate(m: &DriftMonitor) -> Result<MonitorReport, u16> {
        m.assert_attributed()?;
        let mut lines = Vec::new();
        lines.push(m.summary());
        for r in &m.rows {
            let mut l = String::from("窗");
            l.push_str(&r.win_no.to_string());
            l.push_str(" p25=");
            l.push_str(&r.p25.to_string());
            l.push_str(" p75=");
            l.push_str(&r.p75.to_string());
            match r.dir {
                None => l.push_str(" 带内"),
                Some(d) => {
                    l.push(' ');
                    l.push_str(d.name());
                }
            }
            lines.push(l);
        }
        Ok(MonitorReport { lines })
    }

    pub fn assert_complete(&self) -> Result<(), u16> {
        if self.lines.len() < 2 {
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

/// 自例程：200 次建带 + 上下漂注入检出 + 带内零误报 + 注入全拦截 → 0。
pub fn drill() -> Result<u32, u16> {
    percentile_audit()?;
    dir_audit()?;
    synth_determinism_audit()?;

    // 带内纯净：零误报
    let clean = synth_series(0x0675, None)?;
    DriftMonitor::inject_clean_must_pass(&clean)?;
    // 上漂注入
    inject_up_must_catch()?;
    // 下漂注入
    inject_down_must_catch()?;

    // 主账：注入样本监测 + 归因 + 封账
    let s = synth_series(0x0675, Some((100, 700)))?;
    band_determinism_audit(&s)?;
    let mut m = DriftMonitor::new(&s)?;
    let n = m.scan(&s)?;
    if n == 0 {
        return Err(E_UNATTR);
    }
    m.assert_attributed()?;
    if m.summary().is_empty() {
        return Err(SEG_BASE + 0x33);
    }
    // 窗指纹簿：逐窗登记 + 对账 + 封簿后写拦截
    let mut fb = WinFpBook::new();
    for r in &m.rows {
        fb.record(r.win_no, r.p25, r.p75, win_fp(r.win_no, r.p25, r.p75))?;
    }
    fb.verify(&m)?;
    fb.seal()?;
    if fb.record(99, 1, 2, win_fp(99, 1, 2)) != Err(E_SEAL) {
        return Err(SEG_BASE + 0x37);
    }
    // 归因汇总：两类计数守恒
    let ds = DirSummary::count(&m);
    ds.conserved(&m)?;
    if ds.summary().is_empty() {
        return Err(SEG_BASE + 0x38);
    }
    m.seal()?;
    // 注入：封后扫描 → 拦截
    if m.scan(&s) != Err(E_SEAL) {
        return Err(SEG_BASE + 0x34);
    }
    // 注入：样本量不足 → 拦截
    let short = synth_series(1, None)?;
    if DriftMonitor::new(&short[..100]).err() != Some(E_N) {
        return Err(SEG_BASE + 0x35);
    }
    // 注入：窗口不足建带 → 拦截
    if build_band(&short[..10]) != Err(E_WINDOW) {
        return Err(SEG_BASE + 0x36);
    }
    // 报告
    let rep = MonitorReport::generate(&m)?;
    rep.assert_complete()?;
    Ok(0)
}
