//! cgf08_certvalid — CGPU-F0807 认证有效期与重认证（认证随环境漂移的治理）。
//!
//! 判据原文（施工书锚点 CGPU-F0807）：有效期：重认证触发（驱动大版本/系统
//! 更新/引擎版本变更→重认证——触发规则表）；过期机制（认证随环境漂移过期
//! ——过期显性标注非静默）；重认证成本控制（增量重认证——只重测受影响项）；
//! 有效期公开。判据：触发规则、过期显性、增量、公开、判据。
//!
//! 职责定位：触发规则表（环境变更三类→重认证）+过期显性标注+增量重认证计
//! 划（只重测受影响项）+有效期公开卡。与 cgf07_certgrade（F0806 判级）分工
//! ——彼定级、此管级的时效与再触发。诊断码段 0x4Bxx。

use alloc::vec::Vec;

/// 模块版本。
pub const CERT_VALID_VERSION: &str = "F08-certvalid-v1";

/// 认证有效期基准（天）：驱动大版本触发全量重认证前的宽限。
pub const VALIDITY_DAYS: u32 = 90;

/// 重认证触发源闭集（判据"驱动大版本/系统更新/引擎版本变更"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TriggerKind {
    /// 驱动大版本变更。
    DriverMajor,
    /// 系统更新。
    SystemUpdate,
    /// 引擎版本变更。
    EngineBump,
}

impl TriggerKind {
    pub const ALL: [TriggerKind; 3] = [
        TriggerKind::DriverMajor,
        TriggerKind::SystemUpdate,
        TriggerKind::EngineBump,
    ];

    pub const fn zh(self) -> &'static str {
        match self {
            TriggerKind::DriverMajor => "驱动大版本变更",
            TriggerKind::SystemUpdate => "系统更新",
            TriggerKind::EngineBump => "引擎版本变更",
        }
    }

    /// 增量重认证范围（判据"只重测受影响项"——受影响场景闭集：
    /// 0=全量（A/B/C 全重测）1=A+B 2=C 3=B+C——规则表写死可对拍）。
    pub const fn retest_scope(self) -> &'static [u8] {
        match self {
            TriggerKind::DriverMajor => &[0, 1, 2], // 驱动动底层：全量
            TriggerKind::SystemUpdate => &[0, 1],   // 系统更新：A/B（合成主链路）
            TriggerKind::EngineBump => &[1, 2],     // 引擎变更：B/C（引擎负载档）
        }
    }
}

/// 认证状态（有效期视角）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CertFreshness {
    /// 有效（未过期未触发）。
    Valid,
    /// 过期（时效到——显性标注非静默）。
    Expired,
    /// 触发重认证（环境变更命中规则表）。
    Retriggered,
}

impl CertFreshness {
    pub const fn zh(self) -> &'static str {
        match self {
            CertFreshness::Valid => "有效",
            CertFreshness::Expired => "已过期(显性标注)",
            CertFreshness::Retriggered => "已触发重认证",
        }
    }
}

/// 诊断码（0x4Bxx 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// 0x4B01 认证已过期（使用方必须看到——非静默）。
    CvExpired,
    /// 0x4B02 环境变更触发重认证（增量计划已出）。
    CvRetriggered,
    /// 0x4B03 增量计划非法（重测范围空）。
    CvPlanEmpty,
    /// 0x4B04 有效期起算时间非法（未来时间戳）。
    CvClockInvalid,
    /// 0x4B05 重认证未完成前判级不可用。
    CvRegrading,
}

impl XcCode {
    /// 诊断详情（闭集逐码中文说明）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::CvExpired => "认证已过期：显性标注非静默——消费方按过期态处理",
            XcCode::CvRetriggered => "环境变更命中触发规则表：增量重认证计划已出",
            XcCode::CvPlanEmpty => "增量重认证计划为空：规则表配置缺陷",
            XcCode::CvClockInvalid => "起算时间非法：有效期不能从未来时间起算",
            XcCode::CvRegrading => "重认证未完成：判级不可用（防止用旧级掩盖漂移）",
        }
    }
}

/// 增量重认证计划行（判据"只重测受影响项"的落地单）。
#[derive(Clone, Debug)]
pub struct RetestPlan {
    /// 触发源。
    pub cause: TriggerKind,
    /// 需重测的场景序清单（三档基准 0/1/2 的子集）。
    pub scenes: Vec<u8>,
    /// 免测场景清单（未受影响——增量价值的显性化）。
    pub skipped: Vec<u8>,
}

/// 认证有效期管理器（判据本体）。
pub struct CertValidity {
    /// 起算日（天序——调用方注入时间轴）。
    start_day: u32,
    /// 当前判级是否可用（重认证中=不可用——CvRegrading 防线）。
    grade_usable: bool,
    /// 状态。
    state: CertFreshness,
    /// 命中的触发源。
    pub hit_trigger: Option<TriggerKind>,
    /// 增量计划。
    pub plan: Option<RetestPlan>,
    /// 立案计数。
    pub cases: u32,
}

impl CertValidity {
    /// 构造（起算日不得为未来——本模块时间轴由调用方推进，构造时给 0 基点）。
    pub fn new(start_day: u32, now_day: u32) -> Result<Self, XcCode> {
        if start_day > now_day {
            return Err(XcCode::CvClockInvalid);
        }
        Ok(CertValidity {
            start_day,
            grade_usable: true,
            state: CertFreshness::Valid,
            hit_trigger: None,
            plan: None,
            cases: 0,
        })
    }

    /// 当前状态。
    pub const fn state(&self) -> CertFreshness {
        self.state
    }

    /// 判级可用性（重认证中判级显性不可用）。
    pub const fn grade_usable(&self) -> bool {
        self.grade_usable
    }

    /// 时效判定（判据"过期机制"：天序差 ≥ 有效期 → 过期——显性）。
    pub fn check_expiry(&mut self, now_day: u32) -> Result<CertFreshness, XcCode> {
        if now_day < self.start_day {
            return Err(XcCode::CvClockInvalid);
        }
        if now_day.saturating_sub(self.start_day) >= VALIDITY_DAYS {
            self.state = CertFreshness::Expired;
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::CvExpired); // 过期显性（非静默）
        }
        self.state = CertFreshness::Valid;
        Ok(self.state)
    }

    /// 环境变更上报（判据"触发规则表"主路径：命中→增量计划出单）。
    pub fn report_change(&mut self, kind: TriggerKind) -> Result<RetestPlan, XcCode> {
        let scope = kind.retest_scope();
        if scope.is_empty() {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::CvPlanEmpty);
        }
        // 增量计划：受影响项重测，其余显性免测
        let mut scenes = Vec::new();
        let mut skipped = Vec::new();
        for s in 0..3u8 {
            if scope.contains(&s) {
                scenes.push(s);
            } else {
                skipped.push(s);
            }
        }
        let plan = RetestPlan { cause: kind, scenes: scenes.clone(), skipped };
        self.hit_trigger = Some(kind);
        self.state = CertFreshness::Retriggered;
        self.grade_usable = false; // 重认证完成前判级不可用
        self.cases = self.cases.saturating_add(1);
        self.plan = Some(plan.clone());
        Err(XcCode::CvRetriggered) // 触发显性（计划随 Err 载出——调用方 capture）
    }

    /// 重认证完成回执（全部受影响场景重测通过后调用——判级恢复可用，
    /// 有效期从完成日起算）。
    pub fn complete_retest(&mut self, now_day: u32) -> Result<(), XcCode> {
        if self.plan.is_none() {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::CvPlanEmpty);
        }
        self.start_day = now_day;
        self.state = CertFreshness::Valid;
        self.grade_usable = true;
        self.hit_trigger = None;
        self.plan = None;
        Ok(())
    }

    /// 有效期公开卡（判据"有效期公开"：状态+剩余天数+触发源单点）。
    pub fn public_card(&self, now_day: u32) -> ValidityCard {
        let remain = VALIDITY_DAYS.saturating_sub(now_day.saturating_sub(self.start_day));
        ValidityCard {
            state: self.state,
            remain_days: remain,
            validity_days: VALIDITY_DAYS,
            trigger: self.hit_trigger,
            version: CERT_VALID_VERSION,
        }
    }

    /// 单行摘要。
    pub fn summary(&self) -> alloc::string::String {
        alloc::format!(
            "认证时效{} 状态{} 判级可用{} 立案{}",
            CERT_VALID_VERSION,
            self.state.zh(),
            self.grade_usable,
            self.cases,
        )
    }
}

/// 有效期公开卡（判据"有效期公开"载体）。
#[derive(Clone, Copy, Debug)]
pub struct ValidityCard {
    /// 状态。
    pub state: CertFreshness,
    /// 剩余天数（过期后为 0）。
    pub remain_days: u32,
    /// 有效期总长（天）。
    pub validity_days: u32,
    /// 命中触发源。
    pub trigger: Option<TriggerKind>,
    /// 版本。
    pub version: &'static str,
}

impl ValidityCard {
    /// 单行摘要。
    pub fn summary(&self) -> alloc::string::String {
        alloc::format!(
            "时效卡 状态{} 剩余{}/{}天 触发{} {}",
            self.state.zh(),
            self.remain_days,
            self.validity_days,
            self.trigger.map(|t| t.zh()).unwrap_or("无"),
            self.version,
        )
    }
}


/// 重认证进度账（深化：增量计划逐场景完成态——计划不是一张纸，
/// 每项重测都有状态与回执；全部完成才允许 complete_retest）。
#[derive(Clone, Debug)]
pub struct RetestItem {
    /// 场景序。
    pub scene: u8,
    /// 是否已重测通过。
    pub done: bool,
}

#[derive(Clone, Debug)]
pub struct RetestProgress {
    /// 逐项账。
    pub items: Vec<RetestItem>,
}

impl RetestProgress {
    /// 从计划构造（全部未完成起账）。
    pub fn from_plan(plan: &RetestPlan) -> Self {
        RetestProgress {
            items: plan.scenes.iter().map(|&s| RetestItem { scene: s, done: false }).collect(),
        }
    }

    /// 逐项回执。
    pub fn mark_done(&mut self, scene: u8) -> Result<(), XcCode> {
        match self.items.iter_mut().find(|i| i.scene == scene) {
            Some(i) => {
                i.done = true;
                Ok(())
            }
            None => Err(XcCode::CvPlanEmpty),
        }
    }

    /// 全部完成判定（complete_retest 的前置闸）。Vec 索引非 const——去 const。
    pub fn all_done(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.items.len() {
            if !self.items[i].done {
                ok = false;
            }
            i += 1;
        }
        ok && !self.items.is_empty()
    }

    /// 剩余项数。
    pub fn remaining(&self) -> u32 {
        let mut r = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            if !self.items[i].done {
                r += 1;
            }
            i += 1;
        }
        r
    }
}

/// 触发规则表快照（深化：三触发源→重测范围的完整快照——公开可查，
/// 规则透明是"有效期公开"的表级延伸）。
pub fn trigger_table() -> [(TriggerKind, usize); 3] {
    let mut t = [(TriggerKind::DriverMajor, 0); 3];
    for (i, k) in TriggerKind::ALL.iter().enumerate() {
        t[i] = (*k, k.retest_scope().len());
    }
    t
}
