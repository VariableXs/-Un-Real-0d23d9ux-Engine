//! VE-F3757 · 创作无障碍文化
//!
//! 锚点：创作无障碍文化（F3158 模式创作版复刻：文化层（无障碍创作倡导
//! （创作社区倡导（复述倡导红线：无障碍不是负担是创作自由（倡导复述）；
//! 培训（创作者无障碍培训（培训内容公开（复述公开红线）；激励（无障碍创作
//! 评选（评选标准公开（复述标准红线）；社区（无障碍创作者社区（参与门槛
//! 无障碍（复述门槛红线）。
//!
//! 数据结构：倡导复述；培训公开；评选公开；门槛复述。
//! 错误路径与降级矩阵：倡导缺失→补（复述）；培训私藏→公开（红线实测）；
//! 标准不公开→拒绝评选（红线）；门槛高→修正（复述红线）。
//! 跨批对接点：F3158/F3175 单源复用声明；F3751 豁免对端；F3759 收口对端。
//!
//! 诊断码段：独占 0x94xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x94xx）
// ─────────────────────────────────────────────────────────────

/// F3757 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C8Code(pub u16);

impl C8Code {
    /// 培训内容私藏——公开红线。
    pub const TRAINING_PRIVATE: C8Code = C8Code(0x9401);
    /// 评选标准不公开——标准红线。
    pub const CRITERIA_PRIVATE: C8Code = C8Code(0x9402);
    /// 参与门槛过高——门槛红线。
    pub const BARRIER_HIGH: C8Code = C8Code(0x9403);
    /// 倡导缺失——倡导复述。
    pub const ADVOCACY_MISS: C8Code = C8Code(0x9404);
    /// 培训未覆盖——覆盖复述。
    pub const TRAINING_GAP: C8Code = C8Code(0x9405);
    /// 激励缺失——激励复述。
    pub const INCENTIVE_MISS: C8Code = C8Code(0x9406);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x9401 => "培训内容私藏，公开红线不通过。",
            0x9402 => "评选标准不公开，标准红线不通过。",
            0x9403 => "参与门槛过高，门槛红线不通过。",
            0x9404 => "倡导缺失，文化层复述不完整。",
            0x9405 => "培训覆盖缺口，覆盖复述不完整。",
            0x9406 => "激励机制缺失，激励复述不完整。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 倡导与培训
// ─────────────────────────────────────────────────────────────

/// 倡导记录。
#[derive(Clone, Debug, Default)]
pub struct Advocacy {
    /// 是否有倡导声明。
    pub stated: bool,
    /// 声明是否合规（无障碍=创作自由）。
    pub compliant: bool,
}

impl Advocacy {
    /// 构造。
    pub fn new() -> Advocacy {
        Advocacy {
            stated: false,
            compliant: false,
        }
    }

    /// 发表合规倡导。
    pub fn publish_compliant(&mut self) {
        self.stated = true;
        self.compliant = true;
    }

    /// 合规。
    pub fn ok(&self) -> bool {
        self.stated && self.compliant
    }
}

/// 培训内容。
#[derive(Clone, Debug, Default)]
pub struct Training {
    /// 是否公开。
    pub public: bool,
    /// 覆盖主题数。
    pub topics: u32,
}

impl Training {
    /// 构造。
    pub fn new() -> Training {
        Training {
            public: false,
            topics: 0,
        }
    }

    /// 公开。
    pub fn publish(&mut self) {
        self.public = true;
    }

    /// 添加主题。
    pub fn add_topic(&mut self) -> u32 {
        self.topics += 1;
        self.topics
    }

    /// 公开红线合规：公开 + 有内容。
    pub fn public_ok(&self) -> bool {
        self.public && self.topics > 0
    }

    /// 覆盖合规（至少 3 主题）。
    pub fn coverage_ok(&self) -> bool {
        self.topics >= 3
    }
}

// ─────────────────────────────────────────────────────────────
// 评选与激励
// ─────────────────────────────────────────────────────────────

/// 评选标准。
#[derive(Clone, Debug, Default)]
pub struct AwardCriteria {
    /// 是否公开。
    pub public: bool,
    /// 条目数。
    pub items: u32,
}

impl AwardCriteria {
    /// 构造。
    pub fn new() -> AwardCriteria {
        AwardCriteria {
            public: false,
            items: 0,
        }
    }

    /// 公开。
    pub fn publish(&mut self) {
        self.public = true;
    }

    /// 添加条目。
    pub fn add_item(&mut self) -> u32 {
        self.items += 1;
        self.items
    }

    /// 合规：公开 + 有条目。
    pub fn ok(&self) -> bool {
        self.public && self.items > 0
    }
}

/// 激励机制。
#[derive(Clone, Debug, Default)]
pub struct Incentive {
    /// 是否有评选。
    pub has_award: bool,
    /// 是否有展示位。
    pub has_showcase: bool,
}

impl Incentive {
    /// 构造。
    pub fn new() -> Incentive {
        Incentive {
            has_award: false,
            has_showcase: false,
        }
    }

    /// 合规：至少一项激励。
    pub fn ok(&self) -> bool {
        self.has_award || self.has_showcase
    }
}

// ─────────────────────────────────────────────────────────────
// 参与门槛
// ─────────────────────────────────────────────────────────────

/// 参与门槛。
#[derive(Clone, Debug, Default)]
pub struct Barrier {
    /// 门槛等级（0-10，≤3 合规）。
    pub level: u8,
    /// 是否有无障碍通道。
    pub accessible_path: bool,
}

impl Barrier {
    /// 构造。
    pub fn new() -> Barrier {
        Barrier {
            level: 1,
            accessible_path: true,
        }
    }

    /// 合规：低门槛 + 无障碍通道。
    pub fn ok(&self) -> bool {
        self.level <= 3 && self.accessible_path
    }

    /// 演练：抬高门槛。
    pub fn inject_high(&mut self) {
        self.level = 8;
    }

    /// 修正。
    pub fn fix(&mut self) {
        self.level = 1;
        self.accessible_path = true;
    }
}

// ─────────────────────────────────────────────────────────────
// 主编排器
// ─────────────────────────────────────────────────────────────

/// 创作无障碍文化审查器。
#[derive(Clone, Debug, Default)]
pub struct CultureAuditor {
    advocacy: Advocacy,
    training: Training,
    criteria: AwardCriteria,
    incentive: Incentive,
    barrier: Barrier,
}

impl CultureAuditor {
    /// 构造。
    pub fn new() -> CultureAuditor {
        CultureAuditor {
            advocacy: Advocacy::new(),
            training: Training::new(),
            criteria: AwardCriteria::new(),
            incentive: Incentive::new(),
            barrier: Barrier::new(),
        }
    }

    /// 取倡导（可变）。
    pub fn advocacy_mut(&mut self) -> &mut Advocacy {
        &mut self.advocacy
    }

    /// 取培训（可变）。
    pub fn training_mut(&mut self) -> &mut Training {
        &mut self.training
    }

    /// 取评选（可变）。
    pub fn criteria_mut(&mut self) -> &mut AwardCriteria {
        &mut self.criteria
    }

    /// 取激励（可变）。
    pub fn incentive_mut(&mut self) -> &mut Incentive {
        &mut self.incentive
    }

    /// 取门槛（可变）。
    pub fn barrier_mut(&mut self) -> &mut Barrier {
        &mut self.barrier
    }

    /// 全量审查。
    pub fn audit(&self) -> Vec<C8Code> {
        let mut bad = Vec::new();
        if !self.training.public_ok() {
            bad.push(C8Code::TRAINING_PRIVATE);
        }
        if !self.criteria.ok() {
            bad.push(C8Code::CRITERIA_PRIVATE);
        }
        if !self.barrier.ok() {
            bad.push(C8Code::BARRIER_HIGH);
        }
        if !self.advocacy.ok() {
            bad.push(C8Code::ADVOCACY_MISS);
        }
        if !self.training.coverage_ok() {
            bad.push(C8Code::TRAINING_GAP);
        }
        if !self.incentive.ok() {
            bad.push(C8Code::INCENTIVE_MISS);
        }
        bad
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3757 判据集。
pub fn run_vr08q_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3757");

    // 0x9401 培训公开
    {
        let mut t = Training::new();
        t.add_topic();
        out.add("0x9401-私藏拦", !t.public_ok(), "");
        t.publish();
        out.add("0x9401-公开后过", t.public_ok(), "");
    }

    // 0x9402 评选标准
    {
        let mut c = AwardCriteria::new();
        c.add_item();
        out.add("0x9402-未公开拦", !c.ok(), "");
        c.publish();
        out.add("0x9402-公开后过", c.ok(), "");
    }

    // 0x9403 门槛
    {
        let mut b = Barrier::new();
        out.add("0x9403-默认合规", b.ok(), "");
        b.inject_high();
        out.add("0x9403-过高检出", !b.ok(), "");
        b.fix();
        out.add("0x9403-修正后过", b.ok(), "");
    }

    // 0x9404 倡导
    {
        let mut a = Advocacy::new();
        out.add("0x9404-缺倡导", !a.ok(), "");
        a.publish_compliant();
        out.add("0x9404-发布后过", a.ok(), "");
    }

    // 0x9405 覆盖
    {
        let mut t = Training::new();
        t.add_topic();
        out.add("0x9405-覆盖不足", !t.coverage_ok(), "");
        t.add_topic();
        t.add_topic();
        out.add("0x9405-三主题过", t.coverage_ok(), "");
    }

    // 0x9406 激励
    {
        let mut i = Incentive::new();
        out.add("0x9406-无激励", !i.ok(), "");
        i.has_award = true;
        out.add("0x9406-有评选过", i.ok(), "");
    }

    out
}
