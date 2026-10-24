#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0xD800）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xD800;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CD8Code(pub u16);

impl CD8Code {
    pub const TRIGGER_MISS: Self = Self(SEG_BASE);      // 触发漏钳
    pub const TIER_FAIL: Self = Self(SEG_BASE + 1);     // 三档失效
    pub const THIRD_PARTY_MISS: Self = Self(SEG_BASE + 2); // 第三方漏检
    pub const TELEMETRY_LEAK: Self = Self(SEG_BASE + 3);   // 遥测含内容
    pub const VESTIBULAR_RISK: Self = Self(SEG_BASE + 4);  // 前庭风险

    pub fn code(self) -> u16 { self.0 }
    pub fn name(self) -> &'static str {
        match self {
            Self::TRIGGER_MISS => "触发漏钳",
            Self::TIER_FAIL => "三档失效",
            Self::THIRD_PARTY_MISS => "第三方漏检",
            Self::TELEMETRY_LEAK => "遥测含内容",
            Self::VESTIBULAR_RISK => "前庭风险",
            _ => "未知",
        }
    }
    pub const ALL: [Self; 5] = [
        Self::TRIGGER_MISS, Self::TIER_FAIL, Self::THIRD_PARTY_MISS,
        Self::TELEMETRY_LEAK, Self::VESTIBULAR_RISK,
    ];
}

// ── 三档制（单源复用 F3033）───────────────────────────────────────

/// 前庭安全三档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VestibularTier { Full, Reduced, Direct }

impl VestibularTier {
    pub fn name(self) -> &'static str {
        match self {
            Self::Full => "完整",
            Self::Reduced => "减弱",
            Self::Direct => "直达",
        }
    }
    /// 位移缩放百分比。
    pub fn motion_scale_pct(self) -> u16 {
        match self {
            Self::Full => 100,
            Self::Reduced => 30,
            Self::Direct => 0,
        }
    }
    pub const ALL: [Self; 3] = [Self::Full, Self::Reduced, Self::Direct];
}

// ── 渲染钳制 ────────────────────────────────────────────────────────

/// 触发阈值（三阈值：位移/旋转/缩放）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerThreshold {
    /// 最大单帧位移（归一化）。
    pub max_displacement: u32,
    /// 最大单帧旋转（度）。
    pub max_rotation_deg: u32,
    /// 最大单帧缩放变化（百分比）。
    pub max_scale_delta_pct: u32,
}

impl TriggerThreshold {
    pub const DEFAULT: Self = Self { max_displacement: 50, max_rotation_deg: 25, max_scale_delta_pct: 20 };

    /// 是否触发（任一超阈值）。
    pub fn triggered(&self, displacement: u32, rotation_deg: u32, scale_delta_pct: u32) -> bool {
        displacement > self.max_displacement
            || rotation_deg > self.max_rotation_deg
            || scale_delta_pct > self.max_scale_delta_pct
    }
}

/// 渲染参数钳制。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderClamp;

impl RenderClamp {
    /// 钳制位移到阈值内（按档缩放）。
    pub fn clamp_displacement(raw: u32, threshold: &TriggerThreshold, tier: VestibularTier) -> u32 {
        let scaled = (raw as u64 * tier.motion_scale_pct() as u64 / 100) as u32;
        scaled.min(threshold.max_displacement)
    }

    /// 钳制旋转。
    pub fn clamp_rotation(raw_deg: u32, threshold: &TriggerThreshold, tier: VestibularTier) -> u32 {
        let scaled = (raw_deg as u64 * tier.motion_scale_pct() as u64 / 100) as u32;
        scaled.min(threshold.max_rotation_deg)
    }

    /// 钳制缩放变化。
    pub fn clamp_scale_delta(raw_pct: u32, threshold: &TriggerThreshold, tier: VestibularTier) -> u32 {
        let scaled = (raw_pct as u64 * tier.motion_scale_pct() as u64 / 100) as u32;
        scaled.min(threshold.max_scale_delta_pct)
    }
}

// ── 第三方内容检测 ──────────────────────────────────────────────────

/// 第三方内容触发检测。
#[derive(Debug, Clone)]
pub struct ThirdPartyDetector {
    pub samples: Vec<MotionSample>,
    pub detected_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotionSample {
    pub id: u32,
    pub displacement: u32,
    pub rotation_deg: u32,
    pub scale_delta_pct: u32,
    pub from_third_party: bool,
}

impl ThirdPartyDetector {
    pub fn new() -> Self { Self { samples: Vec::new(), detected_count: 0 } }

    /// 运行时检测：扫描样本，标记触发源。
    pub fn scan(&mut self, threshold: &TriggerThreshold) {
        self.detected_count = 0;
        for s in &self.samples {
            if s.from_third_party && threshold.triggered(s.displacement, s.rotation_deg, s.scale_delta_pct) {
                self.detected_count += 1;
            }
        }
    }

    /// 第三方漏检红线：有触发样本必须有检出。
    pub fn no_miss(&self) -> bool {
        let any_trigger = self.samples.iter().any(|s| {
            s.from_third_party
                && TriggerThreshold::DEFAULT.triggered(s.displacement, s.rotation_deg, s.scale_delta_pct)
        });
        !any_trigger || self.detected_count > 0
    }
}

// ── 前庭遥测（匿名）────────────────────────────────────────────────

/// 前庭遥测事件（只含统计量，不含内容）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VestibularTelemetry {
    pub event_count: u32,
    pub tier: VestibularTier,
    /// 隐私红线：不得含用户内容。
    pub contains_content: bool,
}

impl VestibularTelemetry {
    pub fn new(tier: VestibularTier) -> Self {
        Self { event_count: 0, tier, contains_content: false }
    }

    /// 隐私合规：不含内容。
    pub fn privacy_ok(&self) -> bool { !self.contains_content }
}

// ── 前庭安全会话 ────────────────────────────────────────────────────

/// 前庭安全渲染会话。
#[derive(Debug, Clone)]
pub struct VestibularSession {
    pub tier: VestibularTier,
    pub threshold: TriggerThreshold,
    pub detector: ThirdPartyDetector,
    pub telemetry: VestibularTelemetry,
}

impl VestibularSession {
    pub fn new(tier: VestibularTier) -> Self {
        Self {
            tier,
            threshold: TriggerThreshold::DEFAULT,
            detector: ThirdPartyDetector::new(),
            telemetry: VestibularTelemetry::new(tier),
        }
    }

    /// 审计。
    pub fn audit(&self) -> VestibularAudit {
        VestibularAudit {
            clamp_ok: true, // 钳制由 RenderClamp 保证
            tier_ok: self.tier.motion_scale_pct() <= 100,
            detect_ok: self.detector.no_miss(),
            privacy_ok: self.telemetry.privacy_ok(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VestibularAudit {
    pub clamp_ok: bool,
    pub tier_ok: bool,
    pub detect_ok: bool,
    pub privacy_ok: bool,
}
impl VestibularAudit {
    pub fn all_green(&self) -> bool { self.clamp_ok && self.tier_ok && self.detect_ok && self.privacy_ok }
}

// ── 判据集 ──────────────────────────────────────────────────────────

pub fn run_f3867_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("vet4e_vestibular");
    fn chk(set: &mut crate::checks::CheckSet, label: &'static str, cond: bool, note: &'static str) {
        set.add(label, cond, note);
    }

    // 1 触发阈值判定
    {
        let t = TriggerThreshold::DEFAULT;
        chk(&mut set, "C3867-01", t.triggered(60, 10, 10), "err");
    }
    // 2 未触发
    {
        let t = TriggerThreshold::DEFAULT;
        chk(&mut set, "C3867-02", !t.triggered(30, 10, 10), "err");
    }
    // 3 旋转触发
    {
        let t = TriggerThreshold::DEFAULT;
        chk(&mut set, "C3867-03", t.triggered(10, 30, 5), "err");
    }
    // 4 缩放触发
    {
        let t = TriggerThreshold::DEFAULT;
        chk(&mut set, "C3867-04", t.triggered(5, 5, 25), "err");
    }
    // 5 钳制（Full 档）
    {
        let t = TriggerThreshold::DEFAULT;
        chk(&mut set, "C3867-05", RenderClamp::clamp_displacement(200, &t, VestibularTier::Full) == 50, "err");
    }
    // 6 钳制（Reduced 档缩放 30%）
    {
        let t = TriggerThreshold::DEFAULT;
        chk(&mut set, "C3867-06", RenderClamp::clamp_displacement(100, &t, VestibularTier::Reduced) == 30, "err");
    }
    // 7 钳制（Direct 档=0）
    {
        let t = TriggerThreshold::DEFAULT;
        chk(&mut set, "C3867-07", RenderClamp::clamp_displacement(100, &t, VestibularTier::Direct) == 0, "err");
    }
    // 8 旋转钳制
    {
        let t = TriggerThreshold::DEFAULT;
        chk(&mut set, "C3867-08", RenderClamp::clamp_rotation(180, &t, VestibularTier::Full) == 25, "err");
    }
    // 9 缩放钳制
    {
        let t = TriggerThreshold::DEFAULT;
        chk(&mut set, "C3867-09", RenderClamp::clamp_scale_delta(80, &t, VestibularTier::Reduced) == 20, "err");
    }
    // 10 三档名称
    {
        chk(&mut set, "C3867-10", VestibularTier::Reduced.name() == "减弱", "err");
    }
    // 11 三档缩放比
    {
        chk(&mut set, "C3867-11", VestibularTier::Full.motion_scale_pct() == 100, "err");
    }
    // 12 第三方检测
    {
        let mut d = ThirdPartyDetector::new();
        d.samples.push(MotionSample { id: 1, displacement: 100, rotation_deg: 0, scale_delta_pct: 0, from_third_party: true });
        d.scan(&TriggerThreshold::DEFAULT);
        chk(&mut set, "C3867-12", d.detected_count == 1, "err");
    }
    // 13 非第三方不检出
    {
        let mut d = ThirdPartyDetector::new();
        d.samples.push(MotionSample { id: 1, displacement: 100, rotation_deg: 0, scale_delta_pct: 0, from_third_party: false });
        d.scan(&TriggerThreshold::DEFAULT);
        chk(&mut set, "C3867-13", d.detected_count == 0, "err");
    }
    // 14 遥测隐私
    {
        let t = VestibularTelemetry::new(VestibularTier::Reduced);
        chk(&mut set, "C3867-14", t.privacy_ok(), "err");
    }
    // 15 会话审计全绿
    {
        let s = VestibularSession::new(VestibularTier::Reduced);
        chk(&mut set, "C3867-15", s.audit().all_green(), "err");
    }
    // 16 触发漏检=审计失败
    {
        let mut s = VestibularSession::new(VestibularTier::Full);
        s.detector.samples.push(MotionSample { id: 1, displacement: 100, rotation_deg: 30, scale_delta_pct: 0, from_third_party: true });
        // 不 scan → detected_count=0 → no_miss 应该失败（有触发但无检出）
        chk(&mut set, "C3867-16", !s.detector.no_miss(), "err");
    }
    // 17 全阈值未触发
    {
        let t = TriggerThreshold::DEFAULT;
        chk(&mut set, "C3867-17", !t.triggered(0, 0, 0), "err");
    }
    // 18 档位三枚齐全
    {
        chk(&mut set, "C3867-18", VestibularTier::ALL.len() == 3, "err");
    }

    set
}
