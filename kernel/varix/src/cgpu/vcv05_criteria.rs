//! CGPU-F3365 · 验收判据体系（vcv05_criteria）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3365`
//!
//! **判据（锚点原文）**：两级、一组、判据。
//!
//! **职责定位（锚点原文）**：判据：验收判据体系（硬指标/体验指标
//! 两级——判据实现；测试（两级一组）。
//!
//! # 一、两级判据（为什么恰好两级）
//!
//! 硬指标是**仪器说了算**的判据：帧时间 P95、延迟上限、崩溃率——
//! 数字进去、布尔出来，没有商量余地。体验指标是**人说了算**的判据：
//! 菜单好不好找、动画跟不跟手——但「人说了算」不等于「随口一说」，
//! 体验判据必须绑定走查记录（V04 批次的走查分级），有记录才能评。
//!
//! 两级**不可互相冒充**：拿硬指标的及格冒充体验合格（仪器绿了就说
//! 好用）与拿体验印象冒充硬指标（「我觉得流畅」顶替帧时间）都是
//! 口径污染——[`Criterion::kind`] 把级别钉进类型，混评即拒
//! （[`E_VV_LEVEL`]）。
//!
//! # 二、判据实现（登记-评估-裁决三步）
//!
//! [`Criteria`] 登记判据（每条带级别、阈值或走查绑定、权重），
//! [`Criteria::evaluate`] 对一次验收运行出裁决：硬指标按阈值比对，
//! 体验指标按走查记录里对应级别的发现数裁决。**权重只用于汇总排序，
//! 不用于放大单个判据**——某条判据失败了，加权重救不回来，失败就是
//! 失败（[`E_VV_WEIGHT`] 防负权重与零权重伪装）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const CRITERIA_VERSION: &str = "V05-criteria-v1";

/// 级别混评（硬指标拿体验证据/反之）。
pub const E_VV_LEVEL: &str = "E_VV_LEVEL";

/// 非法权重（负/零——零权重判据是「挂名不干活」）。
pub const E_VV_WEIGHT: &str = "E_VV_WEIGHT";

/// 体验判据缺走查绑定（无记录不能评）。
pub const E_VV_NOBIND: &str = "E_VV_NOBIND";

/// 未知判据 ID。
pub const E_VV_NOCRIT: &str = "E_VV_NOCRIT";

// ---------------------------------------------------------------------------
// 二、判据类型
// ---------------------------------------------------------------------------

/// 判据级别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    /// 硬指标：阈值比对，仪器说了算。
    Hard,
    /// 体验指标：走查记录裁决，人说了算但有账。
    Experiential,
}

impl Level {
    /// 人话名。
    pub const fn name(self) -> &'static str {
        match self {
            Level::Hard => "硬指标",
            Level::Experiential => "体验指标",
        }
    }
}

/// 走查发现级别（与 V04 走查分级对齐）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Finding {
    /// 阻断：核心旅程走不下去。
    Blocker,
    /// 明显：能走但 Clearly 错。
    Major,
    /// 细节：打磨项。
    Minor,
}

/// 单条判据。
#[derive(Clone, Debug)]
pub struct Criterion {
    /// 判据 ID（登记序）。
    pub id: u32,
    /// 判据名。
    pub name: String,
    /// 级别。
    pub kind: Level,
    /// 硬指标阈值（Hard 时有效；Experiential 为 0）。
    pub threshold: f32,
    /// 绑定的走查发现级别（Experiential 时有效）。
    pub bind: Option<Finding>,
    /// 权重（仅汇总排序用，正数）。
    pub weight: f32,
}

/// 判据登记表。
pub struct Criteria {
    items: Vec<Criterion>,
    /// 遥测：评估次数。
    pub evaluations: u64,
}

impl Criteria {
    /// 空表。
    pub fn new() -> Criteria {
        Criteria {
            items: Vec::new(),
            evaluations: 0,
        }
    }

    /// 登记一条硬指标判据。
    pub fn add_hard(
        &mut self,
        name: &str,
        threshold: f32,
        weight: f32,
    ) -> Result<u32, &'static str> {
        if weight <= 0.0 {
            return Err(E_VV_WEIGHT);
        }
        let id = self.items.len() as u32 + 1;
        self.items.push(Criterion {
            id,
            name: String::from(name),
            kind: Level::Hard,
            threshold,
            bind: None,
            weight,
        });
        Ok(id)
    }

    /// 登记一条体验判据（必须绑定走查发现级别）。
    pub fn add_experiential(
        &mut self,
        name: &str,
        bind: Finding,
        weight: f32,
    ) -> Result<u32, &'static str> {
        if weight <= 0.0 {
            return Err(E_VV_WEIGHT);
        }
        let id = self.items.len() as u32 + 1;
        self.items.push(Criterion {
            id,
            name: String::from(name),
            kind: Level::Experiential,
            threshold: 0.0,
            bind: Some(bind),
            weight,
        });
        Ok(id)
    }

    /// 登记数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 按 ID 取判据。
    pub fn get(&self, id: u32) -> Option<&Criterion> {
        self.items.iter().find(|c| c.id == id)
    }

    /// 评估一条硬指标：实测值 ≤ 阈值判通过（上限型；下限型取倒数编码）。
    pub fn eval_hard(&mut self, id: u32, measured: f32) -> Result<bool, &'static str> {
        // 判据数据先快照：get 的不可变借用不能横跨 evaluations 计数写入（E0506）。
        let threshold = {
            let c = self.get(id).ok_or(E_VV_NOCRIT)?;
            if c.kind != Level::Hard {
                return Err(E_VV_LEVEL);
            }
            c.threshold
        };
        self.evaluations += 1;
        Ok(measured <= threshold)
    }

    /// 评估一条体验判据：绑定级别的发现数为 0 才通过。
    pub fn eval_experiential(
        &mut self,
        id: u32,
        blockers: usize,
        majors: usize,
        minors: usize,
    ) -> Result<bool, &'static str> {
        let c = self.get(id).ok_or(E_VV_NOCRIT)?;
        if c.kind != Level::Experiential {
            return Err(E_VV_LEVEL);
        }
        let bind = c.bind.ok_or(E_VV_NOBIND)?;
        self.evaluations += 1;
        Ok(match bind {
            Finding::Blocker => blockers == 0,
            Finding::Major => majors == 0,
            Finding::Minor => minors == 0,
        })
    }

    /// 两级计数。
    pub fn count_by_level(&self, level: Level) -> usize {
        self.items.iter().filter(|c| c.kind == level).count()
    }

    /// 权重和（汇总排序分母）。
    pub fn weight_sum(&self) -> f32 {
        self.items.iter().map(|c| c.weight).sum()
    }
}

// ---------------------------------------------------------------------------
// 三、一组测试（两级判据）
// ---------------------------------------------------------------------------

/// CGPU-F3365 域自检：两级、一组、判据。
pub fn run_vcv05_checks() -> CheckSet {
    let mut cs = CheckSet::new("vcv05_criteria");
    let mut cc = Criteria::new();

    // --- 登记 ---
    let h1 = cc.add_hard("帧时间P95_ms", 16.7, 3.0);
    let h2 = cc.add_hard("全程延迟_ms", 80.0, 2.0);
    let e1 = cc.add_experiential("菜单可达", Finding::Major, 1.0);
    let e2 = cc.add_experiential("动画跟手", Finding::Minor, 1.0);
    cs.add(
        "四条登记",
        cc.len() == 4 && h1 == Ok(1) && e2 == Ok(4),
        "id 序贯",
    );

    // --- 零/负权重拒绝 ---
    cs.add(
        "零权重拒绝",
        cc.add_hard("挂名判据", 1.0, 0.0) == Err(E_VV_WEIGHT),
        "E_VV_WEIGHT",
    );

    // --- 硬指标评估 ---
    cs.add(
        "阈值内通过",
        cc.eval_hard(1, 14.0) == Ok(true),
        "14 ≤ 16.7",
    );
    cs.add(
        "阈值外拒绝",
        cc.eval_hard(2, 95.0) == Ok(false),
        "95 > 80",
    );

    // --- 级别混评即拒 ---
    cs.add(
        "体验判据拿硬证据拒",
        cc.eval_hard(3, 1.0) == Err(E_VV_LEVEL),
        "E_VV_LEVEL",
    );
    cs.add(
        "硬判据拿走查账拒",
        cc.eval_experiential(1, 0, 0, 0) == Err(E_VV_LEVEL),
        "E_VV_LEVEL",
    );

    // --- 体验判据评估（绑定级别决定口径） ---
    cs.add(
        "Major 绑定：无 Major 过",
        cc.eval_experiential(3, 1, 0, 5) == Ok(true),
        "只看 Major",
    );
    cs.add(
        "Minor 绑定：有 Minor 不过",
        cc.eval_experiential(4, 0, 0, 2) == Ok(false),
        "只看 Minor",
    );

    // --- 未知判据 ---
    cs.add(
        "未知 ID 拒绝",
        cc.eval_hard(99, 1.0) == Err(E_VV_NOCRIT),
        "E_VV_NOCRIT",
    );

    // --- 两级计数与权重 ---
    cs.add(
        "两级各二",
        cc.count_by_level(Level::Hard) == 2
            && cc.count_by_level(Level::Experiential) == 2,
        "两级",
    );
    cs.add(
        "权重和守恒",
        (cc.weight_sum() - 7.0).abs() < 1e-6,
        "3+2+1+1",
    );
    cs.add(
        "评估计数",
        cc.evaluations == 6,
        "遥测",
    );

    // --- 级别人话 ---
    cs.add(
        "级别名非空",
        !Level::Hard.name().is_empty()
            && !Level::Experiential.name().is_empty(),
        "渲染",
    );
    cs.add("版本非空", !CRITERIA_VERSION.is_empty(), "声明");

    cs
}
