//! 深化层四 · F131 上游回馈通道（2026-09-27 深化批次四 · g 层）。
//!
//! 贡献者档案与信任分级、上游健康度雷达（版本落后量）、里程碑看板
//! （PR 队列聚合数据面）、PR 描述模板渲染器、回馈优先级评分矩阵。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 贡献者档案：PR 战绩 → 信任分级（新人/常客/资深）
// ---------------------------------------------------------------------------

pub struct ContributorProfile {
    pub name: &'static str,
    pub merged: u32,
    pub rejected: u32,
    pub first_day: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrustTier {
    Newcomer,
    Regular,
    Senior,
}

impl ContributorProfile {
    /// 分级：≥1 merged 即 Regular；≥5 merged 且拒绝率 <40% 升 Senior。
    pub fn tier(&self) -> TrustTier {
        if self.merged >= 5 && self.rejected * 10 < self.merged * 4 {
            TrustTier::Senior
        } else if self.merged >= 1 {
            TrustTier::Regular
        } else {
            TrustTier::Newcomer
        }
    }

    /// 送审加权：Senior 的 PR 可跳过一轮预审（机器面：预审豁免名单）。
    pub fn pre_review_exempt(&self) -> bool {
        self.tier() == TrustTier::Senior
    }
}

// ---------------------------------------------------------------------------
// 上游健康度雷达：版本落后量 + 同步陈旧度
// ---------------------------------------------------------------------------

pub struct UpstreamHealth {
    pub repo: &'static str,
    /// 我方基线上游版本。
    pub our_base_ver: u32,
    /// 上游当前版本。
    pub their_ver: u32,
    pub last_sync_day: u32,
}

pub const SYNC_STALE_DAYS: u32 = 180;

impl UpstreamHealth {
    pub fn behind(&self) -> u32 {
        self.their_ver.saturating_sub(self.our_base_ver)
    }

    pub fn stale(&self, today: u32) -> bool {
        today > self.last_sync_day + SYNC_STALE_DAYS
    }

    /// 雷达档位：绿(落后≤2 且新鲜)/黄(其一越线)/红(双双越线)。
    pub fn radar(&self, today: u32) -> u8 {
        let behind_bad = self.behind() > 2;
        match (behind_bad, self.stale(today)) {
            (false, false) => 0,
            (true, true) => 2,
            _ => 1,
        }
    }
}

// ---------------------------------------------------------------------------
// 里程碑看板：PR 队列按阶段聚合 + 周吞吐
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum QueueStage {
    Draft,
    InReview,
    Merged,
    Rejected,
}

pub struct MilestoneBoard {
    /// (PR 编号, 阶段, 更新日)
    items: alloc::vec::Vec<(u32, QueueStage, u32)>,
}

impl MilestoneBoard {
    pub fn new() -> MilestoneBoard {
        MilestoneBoard { items: alloc::vec::Vec::new() }
    }

    pub fn upsert(&mut self, id: u32, stage: QueueStage, day: u32) {
        match self.items.iter_mut().find(|(i, _, _)| *i == id) {
            Some(e) => {
                e.1 = stage;
                e.2 = day;
            }
            None => self.items.push((id, stage, day)),
        }
    }

    pub fn count(&self, stage: QueueStage) -> usize {
        self.items.iter().filter(|(_, s, _)| *s == stage).count()
    }

    /// [week_start, week_start+7) 内 Merged 数（周吞吐）。
    pub fn weekly_throughput(&self, week_start: u32) -> usize {
        self.items
            .iter()
            .filter(|(_, s, d)| *s == QueueStage::Merged && *d >= week_start && *d < week_start + 7)
            .count()
    }

    /// 停滞清单：InReview 且更新日距今 >21 天（看板必答"什么卡了"）。
    pub fn stalled(&self, today: u32) -> alloc::vec::Vec<u32> {
        self.items
            .iter()
            .filter(|(_, s, d)| *s == QueueStage::InReview && today > *d + 21)
            .map(|(i, _, _)| *i)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// PR 描述模板渲染器：三节必填（动机/方法/测试），字节预算 ≤4096
// ---------------------------------------------------------------------------

pub struct PrDescription {
    pub motivation: &'static str,
    pub approach: &'static str,
    pub testing: &'static str,
}

impl PrDescription {
    pub fn validate(&self) -> Result<(), &'static str> {
        for (name, s) in [
            ("动机", self.motivation),
            ("方法", self.approach),
            ("测试", self.testing),
        ] {
            if s.trim().is_empty() {
                return Err("描述缺节");
            }
            let _ = name;
        }
        let total = self.motivation.len() + self.approach.len() + self.testing.len();
        if total > 4096 {
            return Err("描述超 4096 字节预算");
        }
        Ok(())
    }

    /// 渲染行序列（节标题 + 正文），确定性输出。
    pub fn render_lines(&self) -> alloc::vec::Vec<&'static str> {
        alloc::vec!["## 动机", self.motivation, "## 方法", self.approach, "## 测试", self.testing]
    }
}

// ---------------------------------------------------------------------------
// 回馈优先级评分矩阵：安全 4 / 正确性风险 0-2×2 / 性能收益 0-2×1.5（整数口径）
// ---------------------------------------------------------------------------

pub fn priority_score(security: bool, correctness_risk: u8, perf_gain: u8) -> u32 {
    let sec = if security { 40 } else { 0 };
    let cr = correctness_risk.min(2) as u32 * 20;
    let pg = perf_gain.min(2) as u32 * 15;
    sec + cr + pg
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F131G_TAG: &str = "stareco-F131-deep4";

pub fn run_f131_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new(F131G_TAG);

    // 贡献者分级
    let newcomer = ContributorProfile { name: "n1", merged: 0, rejected: 2, first_day: 10 };
    let regular = ContributorProfile { name: "n2", merged: 2, rejected: 1, first_day: 10 };
    let senior = ContributorProfile { name: "n3", merged: 6, rejected: 1, first_day: 10 };
    set.add(
        "f131g tiers",
        newcomer.tier() == TrustTier::Newcomer
            && regular.tier() == TrustTier::Regular
            && senior.tier() == TrustTier::Senior,
        "三级判定",
    );
    set.add(
        "f131g exempt",
        senior.pre_review_exempt() && !regular.pre_review_exempt(),
        "资深豁免预审",
    );
    // 高拒绝率压制 Senior：6 merged 5 rejected → 50%≥40% → 降 Regular
    let risky = ContributorProfile { name: "n4", merged: 6, rejected: 5, first_day: 10 };
    set.add("f131g reject gate", risky.tier() == TrustTier::Regular, "高拒绝率压制");

    // 健康雷达
    let h1 = UpstreamHealth { repo: "r", our_base_ver: 8, their_ver: 9, last_sync_day: 1 };
    let h2 = UpstreamHealth { repo: "r", our_base_ver: 1, their_ver: 9, last_sync_day: 1 };
    set.add("f131g radar green", h1.radar(100) == 0, "落后 1 新鲜=绿");
    set.add("f131g radar red", h2.radar(400) == 2, "落后 8 陈旧=红");
    set.add("f131g radar yellow", h2.radar(100) == 1, "落后 8 新鲜=黄");

    // 看板
    let mut b = MilestoneBoard::new();
    b.upsert(1, QueueStage::Draft, 10);
    b.upsert(2, QueueStage::InReview, 50);
    b.upsert(3, QueueStage::Merged, 60);
    b.upsert(4, QueueStage::Merged, 62);
    b.upsert(2, QueueStage::Merged, 65);
    set.add("f131g count", b.count(QueueStage::Merged) == 3 && b.count(QueueStage::Draft) == 1, "聚合计数");
    set.add("f131g throughput", b.weekly_throughput(59) == 3 && b.weekly_throughput(10) == 0, "周吞吐窗口 [59,66) 三单");
    // 停滞清单：5 号长挂 InReview——72 天时超 21 天线，60 天时未超。
    b.upsert(5, QueueStage::InReview, 40);
    set.add("f131g stalled", b.stalled(80) == alloc::vec![5] && b.stalled(60).is_empty(), "停滞 21 天线");

    // 描述模板
    let ok_desc = PrDescription { motivation: "m", approach: "a", testing: "t" };
    set.add("f131g desc ok", ok_desc.validate().is_ok() && ok_desc.render_lines().len() == 6, "三节齐渲染");
    let bad_desc = PrDescription { motivation: "m", approach: " ", testing: "t" };
    set.add("f131g desc missing", bad_desc.validate().is_err(), "空节拒绝");

    // 优先级矩阵
    set.add("f131g prio sec", priority_score(true, 0, 0) == 40, "安全项 40 分");
    set.add(
        "f131g prio order",
        priority_score(true, 2, 2) > priority_score(true, 2, 0)
            && priority_score(false, 0, 0) == 0,
        "风险与收益计分",
    );

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn board_upsert_semantics() {
        let mut b = MilestoneBoard::new();
        b.upsert(1, QueueStage::InReview, 10);
        b.upsert(1, QueueStage::Rejected, 20);
        assert_eq!(b.count(QueueStage::InReview), 0);
        assert_eq!(b.count(QueueStage::Rejected), 1);
        // 停滞只看 InReview：Rejected 不停滞。
        assert!(b.stalled(100).is_empty());
    }
}
