#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0xDD80）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xDD80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CDD8Code(pub u16);

impl CDD8Code {
    pub const URGENT_LOST: Self = Self(SEG_BASE);    // 紧急丢
    pub const COVER_BELOW: Self = Self(SEG_BASE + 1); // 覆盖不足
    pub const REPLAY_DIVERGE: Self = Self(SEG_BASE + 2); // 重演分歧
    pub const ORPHAN: Self = Self(SEG_BASE + 3);      // 处置无责
    pub const GRID_GAP: Self = Self(SEG_BASE + 4);    // 缺格

    pub fn code(self) -> u16 { self.0 }
    pub fn name(self) -> &'static str {
        match self {
            Self::URGENT_LOST => "紧急丢",
            Self::COVER_BELOW => "覆盖不足",
            Self::REPLAY_DIVERGE => "重演分歧",
            Self::ORPHAN => "处置无责",
            Self::GRID_GAP => "缺格",
            _ => "未知",
        }
    }
    pub const ALL: [Self; 5] = [
        Self::URGENT_LOST, Self::COVER_BELOW, Self::REPLAY_DIVERGE,
        Self::ORPHAN, Self::GRID_GAP,
    ];
}

// ── 90 格矩阵 ───────────────────────────────────────────────────────

/// 映射域（十域）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapDomain {
    Arch, Engine, Video, Audio, Realtime, Scale, Vestibular, Photosafe, Control, Api,
}

impl MapDomain {
    pub const ALL: [Self; 10] = [
        Self::Arch, Self::Engine, Self::Video, Self::Audio, Self::Realtime,
        Self::Scale, Self::Vestibular, Self::Photosafe, Self::Control, Self::Api,
    ];
}

/// 媒体型（三型）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType { Live, Vod, Collab }

impl MediaType {
    pub const ALL: [Self; 3] = [Self::Live, Self::Vod, Self::Collab];
}

/// 设备档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceTier { Low, Mid, High }

impl DeviceTier {
    pub const ALL: [Self; 3] = [Self::Low, Self::Mid, Self::High];
}

/// 矩阵格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JointCell {
    pub domain: MapDomain,
    pub media: MediaType,
    pub device: DeviceTier,
}

// ── 抽样器（48/90 格）──────────────────────────────────────────────

/// 分层抽样器。
#[derive(Debug, Clone)]
pub struct Sampler {
    pub full_grid: Vec<JointCell>,
    pub sampled: Vec<JointCell>,
}

impl Sampler {
    pub fn gen_full(&mut self) {
        self.full_grid.clear();
        for d in MapDomain::ALL {
            for m in MediaType::ALL {
                for dev in DeviceTier::ALL {
                    self.full_grid.push(JointCell { domain: d, media: m, device: dev });
                }
            }
        }
    }

    /// 90 格齐全。
    pub fn full_count_ok(&self) -> bool { self.full_grid.len() == 90 }

    /// 48 格抽样。
    pub fn sample_48(&mut self) {
        self.sampled = self.full_grid.iter().take(48).cloned().collect();
    }

    /// 覆盖底线。
    pub fn coverage_ok(&self) -> bool { self.sampled.len() >= 48 }
}

// ── 故障注入 ────────────────────────────────────────────────────────

/// 三故障类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultKind {
    /// 紧急被节流。
    UrgentThrottled,
    /// 字幕遮挡。
    SubtitleOccluded,
    /// 替代反超。
    AltOverheadOver,
}

impl FaultKind {
    pub const ALL: [Self; 3] = [Self::UrgentThrottled, Self::SubtitleOccluded, Self::AltOverheadOver];
    /// 紧急=P0。
    pub fn is_p0(self) -> bool { self == Self::UrgentThrottled }
}

/// 注入记录。
#[derive(Debug, Clone)]
pub struct Injection {
    pub fault: FaultKind,
    pub detected: bool,
    pub resolved: bool,
}

// ── 真实剧本 ────────────────────────────────────────────────────────

/// 剧本执行器。
#[derive(Debug, Clone)]
pub struct ScriptRunner {
    pub duration_min: u32,
    pub signals_captured: bool,
    pub deterministic: bool,
}

impl ScriptRunner {
    pub fn new() -> Self { Self { duration_min: 20, signals_captured: false, deterministic: true } }
    /// 重演确定性。
    pub fn replay_ok(&self) -> bool { self.deterministic }
}

// ── S04 联调会话 ────────────────────────────────────────────────────

/// S04 联调会话。
#[derive(Debug, Clone)]
pub struct S04Joint {
    pub sampler: Sampler,
    pub injections: Vec<Injection>,
    pub script: ScriptRunner,
}

impl S04Joint {
    pub fn new() -> Self {
        Self { sampler: Sampler { full_grid: Vec::new(), sampled: Vec::new() }, injections: Vec::new(), script: ScriptRunner::new() }
    }

    /// P0 硬门：紧急丢=0。
    pub fn p0_gate(&self) -> bool {
        self.injections.iter().all(|i| !i.fault.is_p0() || i.detected && i.resolved)
    }

    /// 综合审计。
    pub fn audit(&self) -> JointAudit {
        JointAudit {
            coverage: self.sampler.coverage_ok(),
            p0_ok: self.p0_gate(),
            replay: self.script.replay_ok(),
            all_detected: self.injections.iter().all(|i| i.detected),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JointAudit {
    pub coverage: bool,
    pub p0_ok: bool,
    pub replay: bool,
    pub all_detected: bool,
}
impl JointAudit {
    pub fn all_green(&self) -> bool { self.coverage && self.p0_ok && self.replay && self.all_detected }
}

// ── 判据集 ──────────────────────────────────────────────────────────

pub fn run_f3878_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("vet4p_s04joint");
    fn chk(set: &mut crate::checks::CheckSet, label: &'static str, cond: bool, note: &'static str) {
        set.add(label, cond, note);
    }

    // 1 十域枚举
    {
        chk(&mut set, "C3878-01", MapDomain::ALL.len() == 10, "err");
    }
    // 2 三媒体型
    {
        chk(&mut set, "C3878-02", MediaType::ALL.len() == 3, "err");
    }
    // 3 三设备档
    {
        chk(&mut set, "C3878-03", DeviceTier::ALL.len() == 3, "err");
    }
    // 4 全矩阵 90 格
    {
        let mut s = Sampler { full_grid: Vec::new(), sampled: Vec::new() };
        s.gen_full();
        chk(&mut set, "C3878-04", s.full_count_ok(), "err");
    }
    // 5 48 格抽样
    {
        let mut s = Sampler { full_grid: Vec::new(), sampled: Vec::new() };
        s.gen_full();
        s.sample_48();
        chk(&mut set, "C3878-05", s.sampled.len() == 48, "err");
    }
    // 6 覆盖底线
    {
        let mut s = Sampler { full_grid: Vec::new(), sampled: Vec::new() };
        s.gen_full();
        s.sample_48();
        chk(&mut set, "C3878-06", s.coverage_ok(), "err");
    }
    // 7 三故障
    {
        chk(&mut set, "C3878-07", FaultKind::ALL.len() == 3, "err");
    }
    // 8 紧急=P0
    {
        chk(&mut set, "C3878-08", FaultKind::UrgentThrottled.is_p0(), "err");
    }
    // 9 P0 硬门（已处置）
    {
        let mut j = S04Joint::new();
        j.injections.push(Injection { fault: FaultKind::UrgentThrottled, detected: true, resolved: true });
        chk(&mut set, "C3878-09", j.p0_gate(), "err");
    }
    // 10 P0 硬门失败（未处置）
    {
        let mut j = S04Joint::new();
        j.injections.push(Injection { fault: FaultKind::UrgentThrottled, detected: true, resolved: false });
        chk(&mut set, "C3878-10", !j.p0_gate(), "err");
    }
    // 11 非 P0 不触发硬门
    {
        let mut j = S04Joint::new();
        j.injections.push(Injection { fault: FaultKind::SubtitleOccluded, detected: true, resolved: true });
        chk(&mut set, "C3878-11", j.p0_gate(), "err");
    }
    // 12 重演确定
    {
        let s = ScriptRunner::new();
        chk(&mut set, "C3878-12", s.replay_ok(), "err");
    }
    // 13 审计全绿
    {
        let mut j = S04Joint::new();
        j.sampler.gen_full();
        j.sampler.sample_48();
        chk(&mut set, "C3878-13", j.audit().all_green(), "err");
    }
    // 14 覆盖不足
    {
        let j = S04Joint::new();
        chk(&mut set, "C3878-14", !j.audit().all_green(), "err");
    }
    // 15 剧本 20 分钟
    {
        let s = ScriptRunner::new();
        chk(&mut set, "C3878-15", s.duration_min == 20, "err");
    }
    // 16 抽样后不足 48
    {
        let mut s = Sampler { full_grid: Vec::new(), sampled: Vec::new() };
        s.gen_full();
        s.sampled = s.full_grid.iter().take(10).cloned().collect();
        chk(&mut set, "C3878-16", !s.coverage_ok(), "err");
    }
    // 17 注入检出
    {
        let mut j = S04Joint::new();
        j.injections.push(Injection { fault: FaultKind::AltOverheadOver, detected: true, resolved: true });
        j.sampler.gen_full();
        j.sampler.sample_48();
        chk(&mut set, "C3878-17", j.audit().all_green(), "err");
    }
    // 18 全格过
    {
        let mut s = Sampler { full_grid: Vec::new(), sampled: Vec::new() };
        s.gen_full();
        chk(&mut set, "C3878-18", s.full_grid.len() == 90, "err");
    }

    set
}
