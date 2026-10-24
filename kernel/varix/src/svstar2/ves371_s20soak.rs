//! ves371_s20soak — VE-F4131 · T07 i18n 长稳
//! 72 小时长稳方案：语种随机切换循环+资源加载释放循环+并发会话模拟三负载线 + 泄漏/劣化/碎片三类指标全程采样 + 通过线（内存曲线回归带内+无劣化趋势）+ 中途崩溃最小复现立案 + 读屏会话长开档（域本色）+ 采样失真重跑。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xD7C0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SkCode {
    Ok = 0,
    LeakTrend = SEG_BASE + 0x01,
    CrashMidway = SEG_BASE + 0x02,
    SampleDistort = SEG_BASE + 0x03,
}

pub const SOAK_HOURS: u32 = 72;
/// 回归带（万分比）：内存曲线末值偏离首值带内即过。
pub const REGRESSION_BAND_BP: u32 = 300;

/// 三负载线。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoadLine { LangSwitchLoop = 0, LoadFreeLoop = 1, ConcurrentSessions = 2 }

impl LoadLine {
    pub const ALL: [LoadLine; 3] = [
        LoadLine::LangSwitchLoop, LoadLine::LoadFreeLoop, LoadLine::ConcurrentSessions];
    pub fn name(self) -> &'static str {
        match self { LoadLine::LangSwitchLoop => "lang-switch-loop",
            LoadLine::LoadFreeLoop => "load-free-loop", LoadLine::ConcurrentSessions => "concurrent" }
    }
}

/// 长稳方案：三负载开 + 时长 + 读屏长开档（域本色）。
pub struct SoakPlan { pub lines: [bool; 3], pub hours: u32, pub sr_session_pinned: bool }

impl SoakPlan {
    pub fn new() -> Self {
        SoakPlan { lines: [true; 3], hours: SOAK_HOURS, sr_session_pinned: true }
    }
    pub fn complete(&self) -> bool {
        self.hours >= SOAK_HOURS && self.lines.iter().all(|l| *l) && self.sr_session_pinned
    }
}

/// 采样序列与三类指标判定。
pub struct Sampler {
    /// 内存采样（KB，按小时）。
    pub mem: Vec<u32>,
    /// 每千次切换耗时（劣化指标）。
    pub switch_ms_per_k: Vec<u32>,
    /// 碎片率（万分比）。
    pub frag_bp: Vec<u32>,
    pub crashed: bool,
    pub crash_repro: Option<&'static str>,
}

impl Sampler {
    pub fn new() -> Self {
        Sampler { mem: Vec::new(), switch_ms_per_k: Vec::new(), frag_bp: Vec::new(), crashed: false, crash_repro: None }
    }

    pub fn sample(&mut self, mem_kb: u32, switch_ms: u32, frag: u32) {
        self.mem.push(mem_kb);
        self.switch_ms_per_k.push(switch_ms);
        self.frag_bp.push(frag);
    }

    /// 泄漏趋势：末段均值 > 首段均值 × (1+带) → 泄漏。
    pub fn leak_trend(&self) -> bool {
        if self.mem.len() < 4 { return false; }
        let n = self.mem.len();
        let head: u64 = self.mem[..n / 4].iter().map(|v| *v as u64).sum::<u64>() * 4 / (n as u64);
        let tail: u64 = self.mem[n - n / 4..].iter().map(|v| *v as u64).sum::<u64>() * 4 / (n as u64);
        tail * 10_000 > head * ((10_000 + REGRESSION_BAND_BP) as u64)
    }

    /// 劣化趋势：切换耗时单调上升超带。
    pub fn degrade_trend(&self) -> bool {
        match (self.switch_ms_per_k.first(), self.switch_ms_per_k.last()) {
            (Some(f), Some(l)) => l * 10_000 > f * (10_000 + REGRESSION_BAND_BP),
            _ => false,
        }
    }

    /// 碎片超阈。
    pub fn frag_over(&self, floor_bp: u32) -> bool {
        self.frag_bp.last().map_or(false, |v| *v > floor_bp)
    }

    /// 通过线：无泄漏+无劣化+未崩+采样足量。
    pub fn pass(&self) -> Result<(), SkCode> {
        if self.crashed { return Err(SkCode::CrashMidway); }
        if self.mem.len() < SOAK_HOURS as usize { return Err(SkCode::SampleDistort); }
        if self.leak_trend() { return Err(SkCode::LeakTrend); }
        if self.degrade_trend() { return Err(SkCode::LeakTrend); }
        Ok(())
    }

    pub fn crash(&mut self, repro: &'static str) {
        self.crashed = true;
        self.crash_repro = Some(repro);
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves371_checks() -> CheckSet {
    let mut set = CheckSet::new("ves371");

    // C371-01 长稳方案：72h+三负载+读屏长开（域本色）。
    let plan = SoakPlan::new();
    chk(&mut set, "C371-01", plan.complete() && SOAK_HOURS == 72, "方案齐备");

    // C371-02 健康采样过线。
    let mut s = Sampler::new();
    for h in 0..72 {
        s.sample(40_000 + (h % 5) * 100, 120 + (h % 3), 200 + (h % 4));
    }
    chk(&mut set, "C371-02", s.pass().is_ok(), "72h 采样带内过线");

    // C371-03 泄漏趋势检出。
    let mut s2 = Sampler::new();
    for h in 0..72 {
        s2.sample(40_000 + h * 30, 120, 200);
    }
    chk(&mut set, "C371-03", s2.leak_trend() && s2.pass() == Err(SkCode::LeakTrend), "线性增长 => 泄漏");

    // C371-04 劣化趋势检出。
    let mut s3 = Sampler::new();
    for h in 0..72 {
        s3.sample(40_000, 120 + h, 200);
    }
    chk(&mut set, "C371-04", s3.degrade_trend(), "切换耗时单调涨");

    // C371-05 采样不足（失真）→ 重跑。
    let mut s4 = Sampler::new();
    for h in 0..40 { s4.sample(40_000, 120, 200); }
    chk(&mut set, "C371-05", s4.pass() == Err(SkCode::SampleDistort), "40h < 72h 失真");

    // C371-06 中途崩溃 → 最小复现立案。
    let mut s5 = Sampler::new();
    for h in 0..72 { s5.sample(40_000, 120, 200); }
    s5.crash("repro-soak-42h");
    chk(&mut set, "C371-06", s5.pass() == Err(SkCode::CrashMidway) && s5.crash_repro == Some("repro-soak-42h"), "崩溃立案带复现");

    // C371-07 回归带常量单源。
    chk(&mut set, "C371-07", REGRESSION_BAND_BP == 300, "回归带 3%");

    // C371-08 碎片阈值判定。
    let mut s6 = Sampler::new();
    for h in 0..72 { s6.sample(40_000, 120, if h == 71 { 1_200 } else { 200 }); }
    chk(&mut set, "C371-08", s6.frag_over(1_000), "尾样碎片 12% > 10%");

    // C371-09 三负载名单单源。
    chk(&mut set, "C371-09", LoadLine::ALL.len() == 3 && LoadLine::ALL[2].name() == "concurrent", "三负载单源");

    // C371-10 泄漏判定数学：尾/首 = 1.02 带内。
    let mut s7 = Sampler::new();
    for h in 0..72 { s7.sample(40_000 + h / 2, 120, 200); } // +36KB 尾段
    chk(&mut set, "C371-10", !s7.leak_trend(), "+36KB/40000 ≈ 0.09% 带内");

    // C371-11 通过线三判齐（无崩+足量+带内）。
    chk(&mut set, "C371-11", s7.pass().is_ok(), "健康尾段过线");

    // C371-12 读屏长开档标记（域本色）。
    chk(&mut set, "C371-12", plan.sr_session_pinned, "读屏会话长开");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves371_smoke() {
        let set = run_ves371_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves371 red: {:?}", red);
    }

    #[test]
    fn ves371_short() {
        let mut s = Sampler::new();
        for h in 0..10 { s.sample(40_000, 120, 200); }
        assert_eq!(s.pass(), Err(SkCode::SampleDistort));
    }
}
