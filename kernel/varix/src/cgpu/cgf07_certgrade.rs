//! cgf07_certgrade — CGPU-F0806 认证分级（四级认证判据量化与公开）。
//!
//! 判据原文（施工书锚点 CGPU-F0806）：认证分级：四级认证（金：全场景 80 帧/
//! 银：主要场景 80 帧/铜：降质后达标/不认证——每级判据量化）；分级公开（认
//! 证等级随硬件公开可查——F0808 透明）；分级与降质链联动（铜级→O 域降质配
//! 方预载——联动声明）；分级争议处理（复核流程预留）。判据：四级判据、公开、
//! 联动、复核、判据。
//!
//! 职责定位：四级判据量化+分级裁决+公开卡+铜级降质配方预载清单+复核工单状
//! 态机。与 cgf05_certsuite（F0805 考卷体系）分工——彼是考卷定义/冻结/分发，
//! 此是拿考卷结果判级与后续治理（消费 SuitePackage.grade 结论形态的实测数）。
//! 降质配方只出预载清单（O 域消费执行）。诊断码段 0x46xx。

use alloc::vec::Vec;

/// 模块版本。
pub const CERT_GRADE_VERSION: &str = "F07-certgrade-v1";

/// 认证四级闭集（判据"金/银/铜/不认证"）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum CertLevel {
    /// 金：全场景 80 帧原画质。
    Gold,
    /// 银：主要场景 80 帧原画质。
    Silver,
    /// 铜：降质后达标。
    Bronze,
    /// 不认证：降质后仍不达标。
    None,
}

impl CertLevel {
    pub const ALL: [CertLevel; 4] = [
        CertLevel::Gold,
        CertLevel::Silver,
        CertLevel::Bronze,
        CertLevel::None,
    ];

    pub const fn ordinal(self) -> usize {
        match self {
            CertLevel::Gold => 0,
            CertLevel::Silver => 1,
            CertLevel::Bronze => 2,
            CertLevel::None => 3,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            CertLevel::Gold => "金认证（全场景80帧原画质）",
            CertLevel::Silver => "银认证（主要场景80帧原画质）",
            CertLevel::Bronze => "铜认证（降质后达标）",
            CertLevel::None => "不认证（降质后仍不达标）",
        }
    }

    /// 铜级预载的 O 域降质配方清单（判据"铜级→O 域降质配方预载"——
    /// 配方名闭集声明引用，O 域消费执行；金/银/无级空清单）。
    pub const fn degrade_presets(&self) -> &'static [&'static str] {
        match self {
            CertLevel::Gold | CertLevel::Silver => &[],
            CertLevel::Bronze => &[
                "O-动图刷新率减半",
                "O-非焦点页面低频合成",
                "O-阴影质量降档",
            ],
            CertLevel::None => &[],
        }
    }
}

/// 单场景判级证据（考卷实测回填）。
#[derive(Clone, Copy, Debug)]
pub struct SceneEvidence {
    /// 场景（对齐三档基准：0=A 1=B 2=C）。
    pub scene_ordinal: u8,
    /// 原画质 P95（µs）。
    pub p95_native_us: u32,
    /// 降质后 P95（µs；未跑降质=同原画质值）。
    pub p95_degraded_us: u32,
}

impl SceneEvidence {
    /// 原画质达标（P95 ≤ 12.5ms）。
    pub const fn native_ok(&self) -> bool {
        self.p95_native_us <= 12_500
    }

    /// 降质后达标。
    pub const fn degraded_ok(&self) -> bool {
        self.p95_degraded_us <= 12_500
    }
}

/// 诊断码（0x46xx 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// 0x4601 级别闭集外。
    CgLevelUnknown,
    /// 0x4602 判级证据不足（三档基准场景证据必须齐备）。
    CgEvidenceMissing,
    /// 0x4603 复核工单已结（不可重复提单）。
    CgRecheckClosed,
    /// 0x4604 铜级降质配方清单为空（联动声明被破坏——显性）。
    CgDowngradeMismatch,
    /// 0x4605 复核结论与证据矛盾（复核改级必须有新证据支撑）。
    CgRecheckContradiction,
}

impl XcCode {
    /// 诊断详情（闭集逐码中文说明）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::CgLevelUnknown => "认证级别闭集外：只认 金/银/铜/不认证",
            XcCode::CgEvidenceMissing => "判级证据不足：三档基准场景证据必须齐备",
            XcCode::CgRecheckClosed => "复核工单已结：同一争议不可重复提单",
            XcCode::CgDowngradeMismatch => "铜级降质配方预载清单为空：联动声明被破坏",
            XcCode::CgRecheckContradiction => "复核结论与证据矛盾：改级必须附新证据",
        }
    }
}

/// 复核工单态（判据"复核流程预留"——状态机存在、人工裁决）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecheckState {
    /// 已提单待复核。
    Submitted,
    /// 复核中（重跑考卷）。
    Regrading,
    /// 已结（附结论）。
    Resolved,
}

/// 复核工单。
#[derive(Clone, Debug)]
pub struct RecheckTicket {
    /// 工单号。
    pub seq: u32,
    /// 原判级。
    pub orig: CertLevel,
    /// 工单态。
    pub state: RecheckState,
    /// 复核后的新证据（Resolved 时必须非空——改级有据）。
    pub new_evidence: Vec<SceneEvidence>,
    /// 复核结论级（Resolved 时有效）。
    pub concl: Option<CertLevel>,
}

/// 认证分级器（判据本体）。
pub struct CertGrader {
    /// 已判级（公开卡承载）。
    level: Option<CertLevel>,
    /// 判级证据。
    evidence: Vec<SceneEvidence>,
    /// 复核工单账。
    pub tickets: Vec<RecheckTicket>,
    /// 工单序发生器。
    ticket_seq: u32,
    /// 公开卡序号。
    pub card_seq: u64,
    /// 立案计数。
    pub cases: u32,
}

impl CertGrader {
    pub fn new() -> Self {
        CertGrader {
            level: None,
            evidence: Vec::new(),
            tickets: Vec::new(),
            ticket_seq: 0,
            card_seq: 0,
            cases: 0,
        }
    }

    /// 回填场景证据（三档齐备才可判级）。
    pub fn submit_evidence(&mut self, e: SceneEvidence) {
        match self.evidence.iter_mut().find(|x| x.scene_ordinal == e.scene_ordinal) {
            Some(x) => *x = e, // 同场景覆盖（重跑以最新为准）
            None => self.evidence.push(e),
        }
    }

    /// 判级核心（纯函数——不碰 self：grade 与 resolve_recheck 共用同一
    /// 量化逻辑，杜绝两套判据漂移）。
    fn grade_core(evidence: &[SceneEvidence]) -> Result<CertLevel, XcCode> {
        if evidence.len() < 3 {
            return Err(XcCode::CgEvidenceMissing);
        }
        let get = |ord: u8| -> SceneEvidence {
            *evidence
                .iter()
                .find(|x| x.scene_ordinal == ord)
                .unwrap_or(&SceneEvidence {
                    scene_ordinal: ord,
                    p95_native_us: u32::MAX,
                    p95_degraded_us: u32::MAX,
                })
        };
        let a = get(0);
        let b = get(1);
        let c = get(2);
        if a.native_ok() && b.native_ok() && c.native_ok() {
            Ok(CertLevel::Gold)
        } else if a.native_ok() && b.native_ok() {
            Ok(CertLevel::Silver)
        } else if a.degraded_ok() && b.degraded_ok() && c.degraded_ok() {
            Ok(CertLevel::Bronze)
        } else {
            Ok(CertLevel::None)
        }
    }

    /// 判级（判据四级量化主路径）：
    /// 金=三档全 native_ok；银=A/B（主要场景）native_ok；铜=降质后三档全
    /// degraded_ok；无级=降质后仍不达标。证据不全显性拒。
    pub fn grade(&mut self) -> Result<CertLevel, XcCode> {
        if self.evidence.len() < 3 {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::CgEvidenceMissing);
        }
        let lv = Self::grade_core(&self.evidence)?;
        // 联动自检：铜级必须有预载配方（判据"分级与降质链联动"）
        if lv == CertLevel::Bronze && lv.degrade_presets().is_empty() {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::CgDowngradeMismatch);
        }
        self.level = Some(lv);
        Ok(lv)
    }

    /// 当前判级。
    pub const fn level(&self) -> Option<CertLevel> {
        self.level
    }

    /// 分级公开卡（判据"认证等级随硬件公开可查——F0808 透明"）。
    pub fn public_card(&mut self, hw_id: u32) -> Result<PublicCertCard, XcCode> {
        let lv = self.level.ok_or(XcCode::CgEvidenceMissing)?;
        self.card_seq = self.card_seq.saturating_add(1);
        Ok(PublicCertCard {
            hw_id,
            level: lv,
            scenes: self
                .evidence
                .iter()
                .map(|e| (e.scene_ordinal, e.native_ok(), e.degraded_ok()))
                .collect(),
            presets: lv.degrade_presets(),
            version: CERT_GRADE_VERSION,
            card_seq: self.card_seq,
        })
    }

    /// 提复核工单（判据"分级争议处理"——同争议重复提单拒）。
    pub fn open_recheck(&mut self) -> Result<u32, XcCode> {
        if let Some(t) = self.tickets.last() {
            if t.state != RecheckState::Resolved {
                self.cases = self.cases.saturating_add(1);
                return Err(XcCode::CgRecheckClosed);
            }
        }
        let orig = self.level.ok_or(XcCode::CgEvidenceMissing)?;
        self.ticket_seq = self.ticket_seq.saturating_add(1);
        self.tickets.push(RecheckTicket {
            seq: self.ticket_seq,
            orig,
            state: RecheckState::Submitted,
            new_evidence: Vec::new(),
            concl: None,
        });
        Ok(self.ticket_seq)
    }

    /// 复核落判（判据"复核结论与证据矛盾"防线：改级必须附新证据——
    /// 新证据重跑判级逻辑，结论必须与原级不同才构成改级且证据自洽）。
    pub fn resolve_recheck(&mut self, seq: u32, new_evidence: Vec<SceneEvidence>) -> Result<CertLevel, XcCode> {
        let t = self
            .tickets
            .iter_mut()
            .find(|t| t.seq == seq)
            .ok_or(XcCode::CgRecheckClosed)?;
        if t.state == RecheckState::Resolved {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::CgRecheckClosed);
        }
        if new_evidence.len() < 3 {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::CgRecheckContradiction);
        }
        // 以新证据重跑同一判级逻辑（grade_core 纯函数——无借用冲突）
        let lv = Self::grade_core(&new_evidence)?;
        t.state = RecheckState::Resolved;
        t.new_evidence = new_evidence;
        t.concl = Some(lv);
        self.level = Some(lv); // 复核结论生效（改级有据）
        Ok(lv)
    }

    /// 单行摘要。
    pub fn summary(&self) -> alloc::string::String {
        let lv = match self.level {
            Some(l) => l.zh(),
            None => "未判级",
        };
        alloc::format!(
            "认证分级{} 判级{} 证据{} 工单{} 公开卡{} 立案{}",
            CERT_GRADE_VERSION,
            lv,
            self.evidence.len(),
            self.tickets.len(),
            self.card_seq,
            self.cases,
        )
    }
}

/// 公开认证卡（判据"分级公开"载体——随硬件公开可查）。
#[derive(Clone, Debug)]
pub struct PublicCertCard {
    /// 硬件 id。
    pub hw_id: u32,
    /// 认证级。
    pub level: CertLevel,
    /// 场景明细（场景序, 原画质达标, 降质后达标）。
    pub scenes: Vec<(u8, bool, bool)>,
    /// 铜级预载配方（非铜级为空）。
    pub presets: &'static [&'static str],
    /// 版本。
    pub version: &'static str,
    /// 卡序。
    pub card_seq: u64,
}

impl PublicCertCard {
    /// 单行摘要。
    pub fn summary(&self) -> alloc::string::String {
        alloc::format!(
            "认证卡#{} 硬件{} {} 场景{} 项 预载配方{} {}",
            self.card_seq,
            self.hw_id,
            self.level.zh(),
            self.scenes.len(),
            self.presets.len(),
            self.version,
        )
    }
}
