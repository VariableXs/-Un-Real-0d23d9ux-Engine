//! ves386_s21regress — VE-F4146 · i18n 回归体系
//! 回归集分层三频（冒烟/日归/全量）+ 命中率不回退断言（TM 命中/缓存命中/字形覆盖三率）
//! + 回归报告自动归因（率回退→红门+归因报告；归因不明→立案人工；频次漂移→对版）
//! + 读屏覆盖率回归断言（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xDFC0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RgCode {
    Ok = 0,
    RateRegressed = SEG_BASE + 0x01,
    FreqDrift = SEG_BASE + 0x02,
    AttributionUnknown = SEG_BASE + 0x03,
}

/// 三频分层。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Freq { Smoke = 0, Daily = 1, Full = 2 }

impl Freq {
    pub const ALL: [Freq; 3] = [Freq::Smoke, Freq::Daily, Freq::Full];
    pub fn name(self) -> &'static str {
        match self { Freq::Smoke => "smoke", Freq::Daily => "daily", Freq::Full => "full" }
    }
}

/// 三率断言源（TM 命中/缓存命中/字形覆盖）+ 读屏覆盖率（域本色第四率）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RateKind { TmHit = 0, CacheHit = 1, GlyphCover = 2, ScreenReaderCover = 3 }

impl RateKind {
    /// 三核心率。
    pub const CORE: [RateKind; 3] =
        [RateKind::TmHit, RateKind::CacheHit, RateKind::GlyphCover];
    pub fn name(self) -> &'static str {
        match self {
            RateKind::TmHit => "tm-hit", RateKind::CacheHit => "cache-hit",
            RateKind::GlyphCover => "glyph-cover", RateKind::ScreenReaderCover => "sr-cover",
        }
    }
}

/// 率基线（万分比）。
#[derive(Clone, Copy)]
pub struct RateSample { pub kind: RateKind, pub bp: u32 }

/// 命中率不回退断言器：当前样本对基线，任一核心率回退超带 → 红门。
pub struct RateGate {
    pub baseline: Vec<RateSample>,
    pub band_bp: u32, // 允许回退带（万分比）
}

impl RateGate {
    pub fn new(band_bp: u32) -> Self { RateGate { baseline: Vec::new(), band_bp } }

    pub fn set_baseline(&mut self, s: RateSample) {
        match self.baseline.iter_mut().find(|x| x.kind == s.kind) {
            Some(x) => *x = s,
            None => self.baseline.push(s),
        }
    }

    /// 断言：当前率 ≥ 基线 - 带。
    pub fn assert(&self, cur: RateSample) -> Result<(), RgCode> {
        match self.baseline.iter().find(|b| b.kind == cur.kind) {
            Some(b) => {
                let floor = b.bp.saturating_sub(self.band_bp);
                if cur.bp >= floor { Ok(()) } else { Err(RgCode::RateRegressed) }
            }
            None => Ok(()), // 无基线不判
        }
    }

    /// 回退归因：给出回退率维度的归因报告（返回该率名）。
    pub fn attribute(&self, cur: RateSample) -> Option<&'static str> {
        if self.assert(cur).is_err() { Some(cur.kind.name()) } else { None }
    }
}

/// 回归报告：三层频次登记 + 归因记录 + 人工立案。
pub struct RegressReport {
    pub freqs: Vec<(Freq, u32)>,          // (频, 用例数)
    pub attributions: Vec<&'static str>,  // 归因报告
    pub manual_cases: Vec<&'static str>,  // 归因不明→人工立案
    pub freq_drift: bool,
}

impl RegressReport {
    pub fn new() -> Self {
        RegressReport { freqs: Vec::new(), attributions: Vec::new(), manual_cases: Vec::new(), freq_drift: false }
    }

    pub fn put_freq(&mut self, f: Freq, n: u32) {
        match self.freqs.iter_mut().find(|x| x.0 == f) {
            Some(x) => x.1 = n,
            None => self.freqs.push((f, n)),
        }
    }

    pub fn count(&self, f: Freq) -> u32 {
        self.freqs.iter().find(|x| x.0 == f).map(|x| x.1).unwrap_or(0)
    }

    /// 三频齐备且规模有序：smoke ≤ daily ≤ full。
    pub fn layered(&self) -> bool {
        self.count(Freq::Smoke) <= self.count(Freq::Daily)
            && self.count(Freq::Daily) <= self.count(Freq::Full)
            && self.count(Freq::Full) > 0
    }

    /// 归因入档；归因不明 → 人工立案。
    pub fn record(&mut self, attribution: Option<&'static str>, issue: &'static str) {
        match attribution {
            Some(a) => { if !self.attributions.contains(&a) { self.attributions.push(a); } }
            None => { if !self.manual_cases.contains(&issue) { self.manual_cases.push(issue); } }
        }
    }

    /// 频次对版：三频名单单源（漂移标记）。
    pub fn check_freq_drift(&mut self, found: &[&'static str]) -> bool {
        let expect: Vec<&'static str> = Freq::ALL.iter().map(|f| f.name()).collect();
        self.freq_drift = found != expect.as_slice();
        self.freq_drift
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves386_checks() -> CheckSet {
    let mut set = CheckSet::new("ves386");

    // C386-01 三频分层建齐且有序。
    let mut rep = RegressReport::new();
    rep.put_freq(Freq::Smoke, 30);
    rep.put_freq(Freq::Daily, 300);
    rep.put_freq(Freq::Full, 1200);
    chk(&mut set, "C386-01", rep.layered(), "三频 30/300/1200 有序");

    // C386-02 三率基线挂载（含读屏覆盖率域本色第四率）。
    let mut gate = RateGate::new(100); // 允许回退 100bp
    gate.set_baseline(RateSample { kind: RateKind::TmHit, bp: 7200 });
    gate.set_baseline(RateSample { kind: RateKind::CacheHit, bp: 9000 });
    gate.set_baseline(RateSample { kind: RateKind::GlyphCover, bp: 9500 });
    gate.set_baseline(RateSample { kind: RateKind::ScreenReaderCover, bp: 10000 });
    chk(&mut set, "C386-02", gate.baseline.len() == 4, "四率基线（含域本色）");

    // C386-03 不回退断言：在带内通过。
    chk(&mut set, "C386-03", gate.assert(RateSample { kind: RateKind::TmHit, bp: 7150 }).is_ok(), "回退 50bp 带内过");

    // C386-04 率回退 → 红门。
    chk(&mut set, "C386-04", gate.assert(RateSample { kind: RateKind::CacheHit, bp: 8800 })
        == Err(RgCode::RateRegressed), "回退 200bp 红门");

    // C386-05 自动归因：回退率定位。
    chk(&mut set, "C386-05", gate.attribute(RateSample { kind: RateKind::CacheHit, bp: 8800 })
        == Some("cache-hit"), "归因 cache-hit");

    // C386-06 归因入档。
    rep.record(gate.attribute(RateSample { kind: RateKind::CacheHit, bp: 8800 }), "case-x");
    chk(&mut set, "C386-06", rep.attributions == vec!["cache-hit"] && rep.manual_cases.is_empty(), "归因入档");

    // C386-07 归因不明 → 人工立案。
    rep.record(None, "full-run-unknown-drop");
    chk(&mut set, "C386-07", rep.manual_cases == vec!["full-run-unknown-drop"], "人工立案一条");

    // C386-08 无基线率不判（新率首跑）。
    chk(&mut set, "C386-08", gate.assert(RateSample { kind: RateKind::TmHit, bp: 0 }) == Err(RgCode::RateRegressed)
        && gate.baseline.iter().any(|b| b.kind == RateKind::GlyphCover), "有基线零值即红");

    // C386-09 三率名单单源 + 域本色率单列。
    chk(&mut set, "C386-09", RateKind::CORE.len() == 3
        && RateKind::CORE[0].name() == "tm-hit" && RateKind::CORE[2].name() == "glyph-cover"
        && RateKind::ScreenReaderCover.name() == "sr-cover", "三率+域本色单源");

    // C386-10 频次对版：名单漂移检出。
    chk(&mut set, "C386-10", rep.check_freq_drift(&["smoke", "full"]) && rep.freq_drift
        && !rep.check_freq_drift(&["smoke", "daily", "full"]), "频次漂移检出");

    // C386-11 三频名单单源。
    chk(&mut set, "C386-11", Freq::ALL.len() == 3 && Freq::ALL[0].name() == "smoke"
        && Freq::ALL[2].name() == "full", "三频单源");

    // C386-12 零带门：不允许任何回退。
    let tight = RateGate { baseline: gate.baseline.clone(), band_bp: 0 };
    chk(&mut set, "C386-12", tight.assert(RateSample { kind: RateKind::TmHit, bp: 7200 }).is_ok()
        && tight.assert(RateSample { kind: RateKind::TmHit, bp: 7199 }).is_err(), "零带回退即红");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves386_smoke() {
        let set = run_ves386_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves386 red: {:?}", red);
    }

    #[test]
    fn ves386_no_baseline_pass() {
        let g = RateGate::new(50);
        assert!(g.assert(RateSample { kind: RateKind::GlyphCover, bp: 0 }).is_ok());
        assert!(g.attribute(RateSample { kind: RateKind::GlyphCover, bp: 0 }).is_none());
    }
}
