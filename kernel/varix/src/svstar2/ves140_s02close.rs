//! VE-F3840 · S02 组收口双签
//!
//! 接口总账十族 + 映射断言 + 20 项自检 + 联动核验四证据 + 双签。
//! 缺色硬门。自动门硬门。两轮上限。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xBCxx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xBC00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CBCCode(pub u16);

impl CBCCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const MAPPING_MISSING: Self = Self(SEG_BASE + 1);
    pub const COLOR_HARD_GATE: Self = Self(SEG_BASE + 2);
    pub const AUTO_GATE_BROKEN: Self = Self(SEG_BASE + 3);
    pub const LINK_DIVERGE: Self = Self(SEG_BASE + 4);
    pub const DUALSIGN_MISSING: Self = Self(SEG_BASE + 5);
    pub const ROUND_LIMIT: Self = Self(SEG_BASE + 6);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::MAPPING_MISSING => "映射缺，阻断",
            Self::COLOR_HARD_GATE => "缺色硬门（P0）触发，阻断",
            Self::AUTO_GATE_BROKEN => "自动门破，阻断",
            Self::LINK_DIVERGE => "联动分歧，两轮上限",
            Self::DUALSIGN_MISSING => "双签缺失，不生效",
            Self::ROUND_LIMIT => "两轮上限，冻结",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────

/// 接口十族。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiFamily {
    /// 三层架构。
    ThreeTier,
    /// forced 映射。
    ForcedMap,
    /// 主题推导。
    ThemeDerive,
    /// 色弱重映射。
    CvdRemap,
    /// 安全色板。
    SafePalette,
    /// 实时引擎。
    RealtimeEngine,
    /// 文字策略。
    TextPolicy,
    /// 替代库。
    AltLibrary,
    /// 调试器。
    Debugger,
    /// 基准。
    Bench,
}

impl ApiFamily {
    pub const ALL: [ApiFamily; 10] = [
        ApiFamily::ThreeTier,
        ApiFamily::ForcedMap,
        ApiFamily::ThemeDerive,
        ApiFamily::CvdRemap,
        ApiFamily::SafePalette,
        ApiFamily::RealtimeEngine,
        ApiFamily::TextPolicy,
        ApiFamily::AltLibrary,
        ApiFamily::Debugger,
        ApiFamily::Bench,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::ThreeTier => "三层架构",
            Self::ForcedMap => "forced映射",
            Self::ThemeDerive => "主题推导",
            Self::CvdRemap => "色弱重映射",
            Self::SafePalette => "安全色板",
            Self::RealtimeEngine => "实时引擎",
            Self::TextPolicy => "文字策略",
            Self::AltLibrary => "替代库",
            Self::Debugger => "调试器",
            Self::Bench => "基准",
        }
    }
}

/// 双签角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignRole {
    /// 内部。
    Internal,
    /// 外部。
    External,
}

/// 双签。
#[derive(Debug, Clone, Copy)]
pub struct DualSign {
    pub internal: bool,
    pub external: bool,
}

impl DualSign {
    pub fn new() -> Self {
        Self {
            internal: false,
            external: false,
        }
    }

    pub fn sign(&mut self, r: SignRole) {
        match r {
            SignRole::Internal => self.internal = true,
            SignRole::External => self.external = true,
        }
    }

    pub fn complete(&self) -> bool {
        self.internal && self.external
    }
}

impl Default for DualSign {
    fn default() -> Self {
        Self::new()
    }
}

/// 联动证据四类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkEvidence {
    /// 缺色 P0 零发生。
    ColorZero,
    /// 自动门全绿。
    AutoGateGreen,
    /// 同源断言绿。
    SourceAssertGreen,
    /// 四态矩阵证据。
    FourStateMatrix,
}

impl LinkEvidence {
    pub const ALL: [LinkEvidence; 4] = [
        LinkEvidence::ColorZero,
        LinkEvidence::AutoGateGreen,
        LinkEvidence::SourceAssertGreen,
        LinkEvidence::FourStateMatrix,
    ];
}

/// 收口器。
#[derive(Debug, Clone)]
pub struct S02Closeout {
    /// 十族签名表。
    pub families: Vec<(ApiFamily, bool)>,
    /// 映射断言。
    pub mapping_ok: bool,
    /// 自检计数（目标 20）。
    pub self_checks: u8,
    /// 联动证据。
    pub evidence: Vec<LinkEvidence>,
    /// 双签。
    pub sign: DualSign,
    /// 协商轮次。
    pub rounds: u8,
}

impl S02Closeout {
    pub fn new() -> Self {
        let mut families = Vec::new();
        for f in ApiFamily::ALL.iter() {
            families.push((*f, false));
        }
        Self {
            families,
            mapping_ok: false,
            self_checks: 0,
            evidence: Vec::new(),
            sign: DualSign::new(),
            rounds: 0,
        }
    }

    /// 十族齐备且全部签名。
    pub fn families_signed(&self) -> bool {
        ApiFamily::ALL
            .iter()
            .all(|f| self.families.iter().any(|(k, v)| *k == *f && *v))
    }

    /// 自检满 20。
    pub fn self_check_full(&self) -> bool {
        self.self_checks >= 20
    }

    /// 联动四证据齐。
    pub fn evidence_complete(&self) -> bool {
        LinkEvidence::ALL.iter().all(|e| self.evidence.contains(e))
    }

    /// 映射缺检出。
    pub fn mapping_ok(&self) -> bool {
        self.mapping_ok
    }

    /// 全检（阻断语义）。
    pub fn full_check(&self) -> Result<(), CBCCode> {
        if !self.families_signed() {
            return Err(CBCCode::MAPPING_MISSING);
        }
        if !self.mapping_ok() {
            return Err(CBCCode::MAPPING_MISSING);
        }
        if !self.evidence.contains(&LinkEvidence::ColorZero) {
            return Err(CBCCode::COLOR_HARD_GATE);
        }
        if !self.evidence.contains(&LinkEvidence::AutoGateGreen) {
            return Err(CBCCode::AUTO_GATE_BROKEN);
        }
        if !self.evidence_complete() {
            return Err(CBCCode::LINK_DIVERGE);
        }
        if !self.sign.complete() {
            return Err(CBCCode::DUALSIGN_MISSING);
        }
        if self.rounds > 2 {
            return Err(CBCCode::ROUND_LIMIT);
        }
        Ok(())
    }

    /// 协商一轮。
    pub fn negotiate(&mut self) -> Result<(), CBCCode> {
        self.rounds += 1;
        if self.rounds > 2 {
            return Err(CBCCode::ROUND_LIMIT);
        }
        Ok(())
    }

    /// 签名某族。
    pub fn sign_family(&mut self, f: ApiFamily) {
        for e in self.families.iter_mut() {
            if e.0 == f {
                e.1 = true;
            }
        }
    }
}

impl Default for S02Closeout {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves140_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves140_s02close");

    fn ready() -> S02Closeout {
        let mut c = S02Closeout::new();
        for f in ApiFamily::ALL.iter() {
            c.sign_family(*f);
        }
        c.mapping_ok = true;
        c.self_checks = 20;
        for e in LinkEvidence::ALL.iter() {
            c.evidence.push(*e);
        }
        c.sign.sign(SignRole::Internal);
        c.sign.sign(SignRole::External);
        c
    }

    // 01 十族签名
    {
        let c = ready();
        chk(&mut set, "C3840-01", c.families_signed(), "err");
    }
    // 02 映射缺阻断
    {
        let mut c = ready();
        c.mapping_ok = false;
        chk(
            &mut set,
            "C3840-02",
            c.full_check() == Err(CBCCode::MAPPING_MISSING),
            "err",
        );
    }
    // 03 缺色硬门
    {
        let mut c = ready();
        c.evidence.clear();
        c.evidence.push(LinkEvidence::AutoGateGreen);
        chk(
            &mut set,
            "C3840-03",
            c.full_check() == Err(CBCCode::COLOR_HARD_GATE),
            "err",
        );
    }
    // 04 自动门硬门
    {
        let mut c = ready();
        c.evidence.clear();
        c.evidence.push(LinkEvidence::ColorZero);
        chk(
            &mut set,
            "C3840-04",
            c.full_check() == Err(CBCCode::AUTO_GATE_BROKEN),
            "err",
        );
    }
    // 05 联动分歧
    {
        let mut c = ready();
        c.evidence.clear();
        c.evidence.push(LinkEvidence::ColorZero);
        c.evidence.push(LinkEvidence::AutoGateGreen);
        chk(
            &mut set,
            "C3840-05",
            c.full_check() == Err(CBCCode::LINK_DIVERGE),
            "err",
        );
    }
    // 06 双签缺
    {
        let mut c = ready();
        c.sign = DualSign::new();
        chk(
            &mut set,
            "C3840-06",
            c.full_check() == Err(CBCCode::DUALSIGN_MISSING),
            "err",
        );
    }
    // 07 两轮上限
    {
        let mut c = ready();
        c.negotiate().ok();
        c.negotiate().ok();
        chk(
            &mut set,
            "C3840-07",
            c.negotiate() == Err(CBCCode::ROUND_LIMIT),
            "err",
        );
    }
    // 08 20 项自检
    {
        let mut c = ready();
        c.self_checks = 19;
        chk(&mut set, "C3840-08", !c.self_check_full(), "err");
    }
    // 09 四证据齐
    {
        let c = ready();
        chk(&mut set, "C3840-09", c.evidence_complete(), "err");
    }
    // 10 全检通过
    {
        let c = ready();
        chk(&mut set, "C3840-10", c.full_check().is_ok(), "err");
    }
    // 11 轮次超限
    {
        let mut c = ready();
        c.rounds = 3;
        chk(
            &mut set,
            "C3840-11",
            c.full_check() == Err(CBCCode::ROUND_LIMIT),
            "err",
        );
    }
    // 12 双签语义
    {
        let mut s = DualSign::new();
        s.sign(SignRole::Internal);
        let half = !s.complete();
        s.sign(SignRole::External);
        chk(&mut set, "C3840-12", half && s.complete(), "err");
    }

    set
}
