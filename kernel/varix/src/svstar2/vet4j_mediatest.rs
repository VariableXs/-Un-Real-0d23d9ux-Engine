#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xDA80）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xDA80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CDA8Code(pub u16);

impl CDA8Code {
    pub const REALDEVICE_MISSING: Self = Self(SEG_BASE);  // 真机缺
    pub const GRID_GAP: Self = Self(SEG_BASE + 1);        // 缺格
    pub const LATENCY_LIE: Self = Self(SEG_BASE + 2);     // 延迟虚
    pub const DRIFT: Self = Self(SEG_BASE + 3);           // 漂移
    pub const CORPUS_GAP: Self = Self(SEG_BASE + 4);      // 语料缺

    pub fn code(self) -> u16 { self.0 }
    pub fn name(self) -> &'static str {
        match self {
            Self::REALDEVICE_MISSING => "真机缺",
            Self::GRID_GAP => "缺格",
            Self::LATENCY_LIE => "延迟虚",
            Self::DRIFT => "漂移",
            Self::CORPUS_GAP => "语料缺",
            _ => "未知",
        }
    }
    pub const ALL: [Self; 5] = [
        Self::REALDEVICE_MISSING, Self::GRID_GAP, Self::LATENCY_LIE,
        Self::DRIFT, Self::CORPUS_GAP,
    ];
}

// ── 五类断言 ────────────────────────────────────────────────────────

/// 媒体无障碍五类断言。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaAssertKind {
    /// 字幕断言。
    Subtitle,
    /// 描述标记。
    DescMark,
    /// 延迟诚实。
    LatencyHonest,
    /// 控制等价。
    CtrlEquiv,
    /// 紧急保真。
    UrgentFidelity,
}

impl MediaAssertKind {
    pub const ALL: [Self; 5] = [
        Self::Subtitle, Self::DescMark, Self::LatencyHonest, Self::CtrlEquiv, Self::UrgentFidelity,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Subtitle => "字幕断言",
            Self::DescMark => "描述标记",
            Self::LatencyHonest => "延迟诚实",
            Self::CtrlEquiv => "控制等价",
            Self::UrgentFidelity => "紧急保真",
        }
    }
}

/// 断言结果。
#[derive(Debug, Clone)]
pub struct MediaAssertResult {
    pub kind: MediaAssertKind,
    pub passed: bool,
}

// ── 媒体场景集 ──────────────────────────────────────────────────────

/// 媒体类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind { Live, Vod, RealtimeCollab }

impl MediaKind {
    pub const ALL: [Self; 3] = [Self::Live, Self::Vod, Self::RealtimeCollab];
}

/// 网络态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkState { Good, Fair, Poor }

impl NetworkState {
    pub const ALL: [Self; 3] = [Self::Good, Self::Fair, Self::Poor];
}

/// 媒体场景格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaScenarioCell {
    pub kind: MediaKind,
    pub net: NetworkState,
}

// ── 真机测试 ────────────────────────────────────────────────────────

/// 真机测试记录（P0 硬门）。
#[derive(Debug, Clone)]
pub struct RealDeviceTest {
    pub screen_reader: &'static str,
    pub platform: &'static str,
    pub passed: bool,
    pub executed: bool,
}

impl RealDeviceTest {
    /// 真机红线：已执行 + 通过。
    pub fn redline_ok(&self) -> bool { self.executed && self.passed }
}

// ── 语料 ────────────────────────────────────────────────────────────

/// 字幕时序语料。
#[derive(Debug, Clone)]
pub struct SubtitleCorpus {
    pub entries: Vec<(u64, u64)>, // (start_ms, end_ms)
}

impl SubtitleCorpus {
    /// 语料有效：全部 end > start。
    pub fn valid(&self) -> bool { self.entries.iter().all(|(s, e)| e > s) }
}

/// 转写语料。
#[derive(Debug, Clone)]
pub struct TranscriptCorpus {
    pub lines: Vec<&'static str>,
}

impl TranscriptCorpus {
    pub fn nonempty(&self) -> bool { !self.lines.is_empty() }
}

// ── 媒体测试套件 ────────────────────────────────────────────────────

/// 媒体无障碍测试套件。
#[derive(Debug, Clone)]
pub struct MediaTestSuite {
    pub results: Vec<MediaAssertResult>,
    pub scenarios: Vec<MediaScenarioCell>,
    pub real_device: Vec<RealDeviceTest>,
}

impl MediaTestSuite {
    pub fn new() -> Self { Self { results: Vec::new(), scenarios: Vec::new(), real_device: Vec::new() } }

    /// 生成场景矩阵（3×3=9 格）。
    pub fn gen_scenarios(&mut self) {
        self.scenarios.clear();
        for mk in MediaKind::ALL {
            for ns in NetworkState::ALL {
                self.scenarios.push(MediaScenarioCell { kind: mk, net: ns });
            }
        }
    }

    /// 场景完备。
    pub fn scenario_complete(&self) -> bool { self.scenarios.len() == 9 }

    /// 真机 P0：至少一条真机测试已执行且通过。
    pub fn real_device_p0(&self) -> bool {
        self.real_device.iter().any(|t| t.redline_ok())
    }

    /// 五类全过。
    pub fn all_pass(&self) -> bool {
        self.results.len() == 5 && self.results.iter().all(|r| r.passed)
    }
}

// ── 延迟诚实复检 ────────────────────────────────────────────────────

/// 延迟测量 vs 显示。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DelayCheck {
    pub measured_ms: u32,
    pub displayed_ms: u32,
}

impl DelayCheck {
    /// 诚实：偏差 ≤500ms 且显示不低于实测太多。
    pub fn honest(&self) -> bool {
        let diff = if self.measured_ms > self.displayed_ms {
            self.measured_ms - self.displayed_ms
        } else {
            self.displayed_ms - self.measured_ms
        };
        diff <= 500
    }
}

// ── 判据集 ──────────────────────────────────────────────────────────

pub fn run_f3872_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("vet4j_mediatest");
    fn chk(set: &mut crate::checks::CheckSet, label: &'static str, cond: bool, note: &'static str) {
        set.add(label, cond, note);
    }

    // 1 五类齐全
    {
        chk(&mut set, "C3872-01", MediaAssertKind::ALL.len() == 5, "err");
    }
    // 2 断言名
    {
        chk(&mut set, "C3872-02", MediaAssertKind::LatencyHonest.name() == "延迟诚实", "err");
    }
    // 3 场景矩阵
    {
        let mut s = MediaTestSuite::new();
        s.gen_scenarios();
        chk(&mut set, "C3872-03", s.scenario_complete(), "err");
    }
    // 4 媒体三型
    {
        chk(&mut set, "C3872-04", MediaKind::ALL.len() == 3, "err");
    }
    // 5 网络三态
    {
        chk(&mut set, "C3872-05", NetworkState::ALL.len() == 3, "err");
    }
    // 6 真机 P0 通过
    {
        let mut s = MediaTestSuite::new();
        s.real_device.push(RealDeviceTest { screen_reader: "nvda", platform: "win", passed: true, executed: true });
        chk(&mut set, "C3872-06", s.real_device_p0(), "err");
    }
    // 7 真机 P0 失败（未执行）
    {
        let mut s = MediaTestSuite::new();
        s.real_device.push(RealDeviceTest { screen_reader: "nvda", platform: "win", passed: true, executed: false });
        chk(&mut set, "C3872-07", !s.real_device_p0(), "err");
    }
    // 8 真机 P0 失败（未通过）
    {
        let mut s = MediaTestSuite::new();
        s.real_device.push(RealDeviceTest { screen_reader: "nvda", platform: "win", passed: false, executed: true });
        chk(&mut set, "C3872-08", !s.real_device_p0(), "err");
    }
    // 9 语料有效
    {
        let c = SubtitleCorpus { entries: vec![(0, 100), (100, 200)] };
        chk(&mut set, "C3872-09", c.valid(), "err");
    }
    // 10 语料无效
    {
        let c = SubtitleCorpus { entries: vec![(200, 100)] };
        chk(&mut set, "C3872-10", !c.valid(), "err");
    }
    // 11 转写语料非空
    {
        let c = TranscriptCorpus { lines: vec!["hello", "world"] };
        chk(&mut set, "C3872-11", c.nonempty(), "err");
    }
    // 12 转写语料空
    {
        let c = TranscriptCorpus { lines: Vec::new() };
        chk(&mut set, "C3872-12", !c.nonempty(), "err");
    }
    // 13 延迟诚实
    {
        let d = DelayCheck { measured_ms: 300, displayed_ms: 280 };
        chk(&mut set, "C3872-13", d.honest(), "err");
    }
    // 14 延迟虚报
    {
        let d = DelayCheck { measured_ms: 3000, displayed_ms: 100 };
        chk(&mut set, "C3872-14", !d.honest(), "err");
    }
    // 15 五类全过
    {
        let mut s = MediaTestSuite::new();
        for k in MediaAssertKind::ALL {
            s.results.push(MediaAssertResult { kind: k, passed: true });
        }
        chk(&mut set, "C3872-15", s.all_pass(), "err");
    }
    // 16 五类缺项
    {
        let mut s = MediaTestSuite::new();
        s.results.push(MediaAssertResult { kind: MediaAssertKind::Subtitle, passed: true });
        chk(&mut set, "C3872-16", !s.all_pass(), "err");
    }
    // 17 场景格数 9
    {
        let mut s = MediaTestSuite::new();
        s.gen_scenarios();
        chk(&mut set, "C3872-17", s.scenarios.len() == 9, "err");
    }
    // 18 延迟边界（刚好 500ms）
    {
        let d = DelayCheck { measured_ms: 500, displayed_ms: 0 };
        chk(&mut set, "C3872-18", d.honest(), "err");
    }

    set
}
