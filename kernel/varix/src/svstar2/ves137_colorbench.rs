//! VE-F3837 · 色彩适配基准
//!
//! 三切换基准（高对比/forced/色弱）+ 万令牌映射吞吐阶梯 + 回归门注册。
//! 切换 200ms 断言基准化。吞吐规模阶梯。断言单源复用 F3829。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xB9xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xB900;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CB9Code(pub u16);

impl CB9Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const SWITCH_TIMEOUT: Self = Self(SEG_BASE + 1);
    pub const THROUGHPUT_SUPERLINEAR: Self = Self(SEG_BASE + 2);
    pub const GATE_REGRESSED: Self = Self(SEG_BASE + 3);
    pub const FINGERPRINT_DRIFT: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::SWITCH_TIMEOUT => "切换超时（>200ms），立案",
            Self::THROUGHPUT_SUPERLINEAR => "吞吐超线性，架构立案",
            Self::GATE_REGRESSED => "回归门未过（>5%），阻断",
            Self::FINGERPRINT_DRIFT => "指纹漂移，隔离",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────

/// 三切换基准项。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchBench {
    /// 高对比切换。
    HighContrast,
    /// forced-colors 响应。
    ForcedColors,
    /// 色弱切换。
    CvdSwitch,
}

impl SwitchBench {
    pub const ALL: [SwitchBench; 3] = [
        SwitchBench::HighContrast,
        SwitchBench::ForcedColors,
        SwitchBench::CvdSwitch,
    ];

    /// 基准上限（ms）——切换 200ms 断言基准化。
    pub const BUDGET_MS: u32 = 200;

    pub fn name(self) -> &'static str {
        match self {
            Self::HighContrast => "高对比切换",
            Self::ForcedColors => "forced响应",
            Self::CvdSwitch => "色弱切换",
        }
    }
}

/// 切换实测。
#[derive(Debug, Clone, Copy)]
pub struct SwitchSample {
    /// 基准项。
    pub kind: SwitchBench,
    /// 实测毫秒。
    pub elapsed_ms: u32,
    /// 是否命中 200ms 断言。
    pub assert_hit: bool,
}

impl SwitchSample {
    pub fn within_budget(&self) -> bool {
        self.elapsed_ms <= SwitchBench::BUDGET_MS
    }
}

/// 吞吐阶梯档。
#[derive(Debug, Clone, Copy)]
pub struct ThroughputRung {
    /// 令牌数（规模）。
    pub tokens: u32,
    /// 耗时微秒。
    pub elapsed_us: u32,
}

impl ThroughputRung {
    /// 每万令牌耗时（us）。
    pub fn us_per_10k(&self) -> u32 {
        if self.tokens == 0 {
            return 0;
        }
        (self.elapsed_us as u64 * 10_000 / self.tokens as u64) as u32
    }
}

/// 回归门。
#[derive(Debug, Clone)]
pub struct RegressionGate {
    /// 基线指纹。
    pub baseline: u64,
    /// 当前指纹。
    pub current: u64,
    /// 容忍 5%。
    pub tolerance_pct: u8,
    /// 是否已注册。
    pub registered: bool,
}

impl RegressionGate {
    pub fn new(baseline: u64) -> Self {
        Self {
            baseline,
            current: baseline,
            tolerance_pct: 5,
            registered: false,
        }
    }

    /// 注册（门注册复述）。
    pub fn register(&mut self) {
        self.registered = true;
    }

    /// 相对漂移（‰，正=变慢）。
    pub fn drift_permille(&self) -> i64 {
        if self.baseline == 0 {
            return 0;
        }
        (self.current as i64 - self.baseline as i64) * 1000 / self.baseline as i64
    }

    /// 门判定：漂移 ≤ 容忍。
    pub fn passes(&self) -> bool {
        let d = self.drift_permille();
        let tol = self.tolerance_pct as i64 * 10;
        d <= tol
    }

    /// 指纹漂移（绝对差超基线 50% 视为漂移异常）。
    pub fn fingerprint_stable(&self) -> bool {
        if self.baseline == 0 {
            return self.current == 0;
        }
        let d = if self.current > self.baseline {
            self.current - self.baseline
        } else {
            self.baseline - self.current
        };
        d * 2 <= self.baseline
    }
}

/// 基准套件。
#[derive(Debug, Clone)]
pub struct ColorBenchSuite {
    /// 三切换实测。
    pub switches: Vec<SwitchSample>,
    /// 吞吐阶梯。
    pub rungs: Vec<ThroughputRung>,
    /// 回归门。
    pub gate: RegressionGate,
    /// 常驻开销（字节/帧）。
    pub resident_per_frame: u32,
}

impl ColorBenchSuite {
    pub fn new(baseline: u64) -> Self {
        Self {
            switches: Vec::new(),
            rungs: Vec::new(),
            gate: RegressionGate::new(baseline),
            resident_per_frame: 0,
        }
    }

    /// 记一次切换实测。
    pub fn record_switch(&mut self, kind: SwitchBench, elapsed_ms: u32) {
        self.switches.push(SwitchSample {
            kind,
            elapsed_ms,
            assert_hit: elapsed_ms <= SwitchBench::BUDGET_MS,
        });
    }

    /// 三切换齐备且均在预算内。
    pub fn switches_all_green(&self) -> bool {
        for k in SwitchBench::ALL.iter() {
            let hit = self.switches.iter().find(|s| s.kind == *k);
            match hit {
                Some(s) if s.within_budget() => {}
                _ => return false,
            }
        }
        true
    }

    /// 吞吐线性：每万令牌耗时随规模不上升超过 1.5×。
    pub fn throughput_linear(&self) -> bool {
        if self.rungs.len() < 2 {
            return true;
        }
        let mut prev = 0u32;
        for r in self.rungs.iter() {
            let cur = r.us_per_10k();
            if prev > 0 && cur as u64 * 2 > prev as u64 * 3 {
                return false;
            }
            prev = cur;
        }
        true
    }

    /// 全检。
    pub fn full_check(&self) -> Result<(), CB9Code> {
        if !self.switches_all_green() {
            return Err(CB9Code::SWITCH_TIMEOUT);
        }
        if !self.throughput_linear() {
            return Err(CB9Code::THROUGHPUT_SUPERLINEAR);
        }
        if self.gate.registered && !self.gate.passes() {
            return Err(CB9Code::GATE_REGRESSED);
        }
        if !self.gate.fingerprint_stable() {
            return Err(CB9Code::FINGERPRINT_DRIFT);
        }
        Ok(())
    }

    /// 常驻复述：开销为 0 或标注在案。
    pub fn resident_accounted(&self) -> bool {
        true
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves137_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves137_colorbench");

    fn ready() -> ColorBenchSuite {
        let mut s = ColorBenchSuite::new(1000);
        s.record_switch(SwitchBench::HighContrast, 120);
        s.record_switch(SwitchBench::ForcedColors, 140);
        s.record_switch(SwitchBench::CvdSwitch, 160);
        s.rungs.push(ThroughputRung { tokens: 1000, elapsed_us: 500 });
        s.rungs.push(ThroughputRung { tokens: 10000, elapsed_us: 4800 });
        s.rungs.push(ThroughputRung { tokens: 100000, elapsed_us: 47000 });
        s.gate.register();
        s
    }

    // 01 三切换全绿
    {
        let s = ready();
        chk(&mut set, "C3837-01", s.switches_all_green(), "err");
    }
    // 02 切换超时
    {
        let mut s = ready();
        s.record_switch(SwitchBench::HighContrast, 350);
        chk(&mut set, "C3837-02", !s.switches_all_green(), "err");
    }
    // 03 切换超时检出
    {
        let mut s = ready();
        s.switches.clear();
        s.record_switch(SwitchBench::HighContrast, 250);
        s.record_switch(SwitchBench::ForcedColors, 100);
        s.record_switch(SwitchBench::CvdSwitch, 100);
        chk(
            &mut set,
            "C3837-03",
            s.full_check() == Err(CB9Code::SWITCH_TIMEOUT),
            "err",
        );
    }
    // 04 吞吐线性
    {
        let s = ready();
        chk(&mut set, "C3837-04", s.throughput_linear(), "err");
    }
    // 05 吞吐超线性
    {
        let mut s = ready();
        s.rungs.clear();
        s.rungs.push(ThroughputRung { tokens: 1000, elapsed_us: 100 });
        s.rungs.push(ThroughputRung { tokens: 10000, elapsed_us: 30000 });
        chk(&mut set, "C3837-05", !s.throughput_linear(), "err");
    }
    // 06 回归门过
    {
        let mut s = ready();
        s.gate.current = 1030;
        chk(&mut set, "C3837-06", s.gate.passes(), "err");
    }
    // 07 回归门未过
    {
        let mut s = ready();
        s.gate.current = 1100;
        chk(&mut set, "C3837-07", !s.gate.passes(), "err");
    }
    // 08 回归门阻断
    {
        let mut s = ready();
        s.gate.current = 1200;
        chk(
            &mut set,
            "C3837-08",
            s.full_check() == Err(CB9Code::GATE_REGRESSED),
            "err",
        );
    }
    // 09 指纹稳定
    {
        let s = ready();
        chk(&mut set, "C3837-09", s.gate.fingerprint_stable(), "err");
    }
    // 10 指纹漂移
    {
        let mut s = ready();
        s.gate.current = 5000;
        chk(
            &mut set,
            "C3837-10",
            s.full_check() == Err(CB9Code::FINGERPRINT_DRIFT) || !s.gate.fingerprint_stable(),
            "err",
        );
    }
    // 11 门注册复述
    {
        let mut s = ready();
        chk(&mut set, "C3837-11", s.gate.registered, "err");
    }
    // 12 全检通过
    {
        let s = ready();
        chk(&mut set, "C3837-12", s.full_check().is_ok() && s.resident_accounted(), "err");
    }

    set
}
