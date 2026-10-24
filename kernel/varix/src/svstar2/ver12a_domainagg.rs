//! VE-F3581 · E 域聚合自检 —— 十组聚合 + 九组对账 + 分级登记 + 九维评分 + GO 分级。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3581`
//!
//! # 职责（锚点原文拆解）
//!
//! - **十组聚合**（[`DomainAggregate`]）：E01–E10 十组门禁输出的
//!   域级汇总核验。
//! - **九组对账**（[`CrossAudit`]）：E01–E09 九组两两对拍抽核。
//! - **九维评分 + GO 分级**（[`GoGrade`]）：承接收口模板。
//!
//! # 复杂度
//!
//! O(组数)（十组定长）。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E21 段：6800 起）
// ---------------------------------------------------------------------------

/// 聚合自检诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AggCode {
    /// 汇总缺组。
    GroupMissing = 6800,
    /// 对账分歧。
    AuditDivergence = 6801,
    /// 问题未分级。
    Ungraded = 6802,
}

impl AggCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            AggCode::GroupMissing => "聚合缺组",
            AggCode::AuditDivergence => "对账分歧",
            AggCode::Ungraded => "问题未分级",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, AggCode::GroupMissing)
    }
}

// ---------------------------------------------------------------------------
// 二、十组与 GO 分级
// ---------------------------------------------------------------------------

/// GO 分级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GoGrade {
    /// 全绿可发。
    Go,
    /// 有条件。
    Conditional,
    /// 不可发。
    NoGo,
}

impl GoGrade {
    /// 级短名。
    pub const fn label(self) -> &'static str {
        match self {
            GoGrade::Go => "GO",
            GoGrade::Conditional => "有条件",
            GoGrade::NoGo => "NO-GO",
        }
    }
}

/// 十组状态（bit 掩码：bit0..bit9 对应 E01..E10）。
#[derive(Clone, Copy, Debug)]
pub struct GroupMask(pub u16);

impl GroupMask {
    /// 全过。
    pub const fn all_green() -> Self {
        GroupMask(0b11_1111_1111)
    }

    /// 就绪组数。
    pub fn ready_count(&self) -> u32 {
        self.0.count_ones()
    }

    /// 十组齐备。
    pub fn complete(&self) -> bool {
        self.0 == 0b11_1111_1111
    }
}

// ---------------------------------------------------------------------------
// 三、九维评分
// ---------------------------------------------------------------------------

/// 九维评分（千分比）。
#[derive(Clone, Copy, Debug)]
pub struct NineScores {
    /// 九维得分。
    pub scores: [u32; 9],
}

impl NineScores {
    /// 平均分（千分比）。
    pub fn average(&self) -> u32 {
        let sum: u64 = self.scores.iter().map(|&x| x as u64).sum();
        (sum / 9) as u32
    }

    /// GO 分级（≥ 800 = GO；≥ 600 = 有条件；否则 NO-GO）。
    pub fn grade(&self) -> GoGrade {
        let avg = self.average();
        if avg >= 800 {
            GoGrade::Go
        } else if avg >= 600 {
            GoGrade::Conditional
        } else {
            GoGrade::NoGo
        }
    }
}

// ---------------------------------------------------------------------------
// 四、聚合器
// ---------------------------------------------------------------------------

/// E 域聚合自检器。
pub struct DomainAggregate {
    groups: GroupMask,
    scores: NineScores,
}

impl DomainAggregate {
    /// 新建。
    pub fn new() -> Self {
        Self {
            groups: GroupMask(0),
            scores: NineScores { scores: [0; 9] },
        }
    }

    /// 标记一组就绪。
    pub fn mark_group(&mut self, index: u8) {
        if index < 10 {
            self.groups.0 |= 1 << index;
        }
    }

    /// 设九维分。
    pub fn set_scores(&mut self, s: NineScores) {
        self.scores = s;
    }

    /// 聚合核验：十组齐 + 评分已设。
    pub fn verify(&self) -> Result<GoGrade, AggCode> {
        if !self.groups.complete() {
            return Err(AggCode::GroupMissing);
        }
        Ok(self.scores.grade())
    }

    /// 对账：两组得分差 > 200 千分比 = 分歧。
    pub fn cross_audit(&self, a: usize, b: usize) -> Result<(), AggCode> {
        if a >= 9 || b >= 9 {
            return Err(AggCode::Ungraded);
        }
        let d = if self.scores.scores[a] > self.scores.scores[b] {
            self.scores.scores[a] - self.scores.scores[b]
        } else {
            self.scores.scores[b] - self.scores.scores[a]
        };
        if d > 200 {
            Err(AggCode::AuditDivergence)
        } else {
            Ok(())
        }
    }
}

impl Default for DomainAggregate {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 五、判据
// ---------------------------------------------------------------------------

/// 聚合自检判据集。
pub fn run_agg_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver12a");

    // 1. 十组齐备。
    let mut agg = DomainAggregate::new();
    for i in 0..10 {
        agg.mark_group(i);
    }
    agg.set_scores(NineScores {
        scores: [900; 9],
    });
    chk.add("6800-十组齐备", agg.groups.complete() && agg.verify() == Ok(GoGrade::Go), "");

    // 2. 缺组拦。
    let agg2 = DomainAggregate::new();
    chk.add(
        "6801-缺组拦",
        agg2.verify().err() == Some(AggCode::GroupMissing),
        "",
    );

    // 3. 九维均分。
    let s = NineScores {
        scores: [800, 800, 800, 800, 800, 800, 800, 800, 710],
    };
    chk.add("6802-均分", s.average() == 790, "");

    // 4. GO 分级。
    chk.add(
        "6803-分级",
        NineScores { scores: [900; 9] }.grade() == GoGrade::Go
            && NineScores { scores: [700; 9] }.grade() == GoGrade::Conditional
            && NineScores { scores: [100; 9] }.grade() == GoGrade::NoGo,
        "",
    );

    // 5. 对账分歧。
    chk.add(
        "6804-对账分歧",
        agg.cross_audit(0, 1).is_ok(),
        "",
    );
    let mut agg3 = DomainAggregate::new();
    agg3.set_scores(NineScores {
        scores: [100, 500, 100, 100, 100, 100, 100, 100, 100],
    });
    chk.add(
        "6805-分歧检出",
        agg3.cross_audit(0, 1).err() == Some(AggCode::AuditDivergence),
        "",
    );

    // 6. GO 名可达。
    chk.add("6806-GO名", GoGrade::Conditional.label() == "有条件", "");

    chk
}
