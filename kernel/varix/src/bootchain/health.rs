//! UNREAL-X AI-01 · 族0002 Bootchain 健康度（X00026~X00050）。
//!
//! 阶段加权评分：五阶段（固件/引导器/内核/初始化/会话）各有权重，
//! 每阶段打 0~100 分，加权合成为总分并映射到五档健康档位。
//! 纯逻辑 + 固定数组；非法输入钳制回默认，绝不 panic。

use crate::checks::CheckSet;

/// 健康度阶段数（固定）。
pub const STAGE_COUNT: usize = 5;
/// 单阶段权重总和守恒校验用：100。
pub const WEIGHT_SUM: u32 = 100;

/// 健康档位：≥5 档独立可交付。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HealthTier {
    Excellent,
    Good,
    Fair,
    Poor,
    Critical,
}

impl HealthTier {
    /// 总分 → 档位。阈值：85/70/50/30。
    pub fn from_score(score: u32) -> HealthTier {
        match score {
            85..=100 => HealthTier::Excellent,
            70..=84 => HealthTier::Good,
            50..=69 => HealthTier::Fair,
            30..=49 => HealthTier::Poor,
            _ => HealthTier::Critical,
        }
    }

    /// 档位中文名（面向设置页）。
    pub fn label(self) -> &'static str {
        match self {
            HealthTier::Excellent => "优",
            HealthTier::Good => "良",
            HealthTier::Fair => "中",
            HealthTier::Poor => "差",
            HealthTier::Critical => "危",
        }
    }
}

/// 五阶段固定下标。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Firmware = 0,
    Loader = 1,
    Kernel = 2,
    Init = 3,
    Session = 4,
}

impl Stage {
    pub fn index(self) -> usize {
        self as usize
    }

    /// 默认权重：15/25/30/20/10，总和 = 100。
    pub fn default_weight(self) -> u32 {
        match self {
            Stage::Firmware => 15,
            Stage::Loader => 25,
            Stage::Kernel => 30,
            Stage::Init => 20,
            Stage::Session => 10,
        }
    }

    pub const ALL: [Stage; STAGE_COUNT] = [
        Stage::Firmware,
        Stage::Loader,
        Stage::Kernel,
        Stage::Init,
        Stage::Session,
    ];
}

/// 健康度评分器。
#[derive(Clone, Copy, Debug)]
pub struct HealthScorer {
    pub weights: [u32; STAGE_COUNT],
    pub scores: [Option<u8>; STAGE_COUNT],
    /// 被钳制的非法输入次数（护栏可观测）。
    pub clamped: u32,
}

impl HealthScorer {
    pub const fn new() -> HealthScorer {
        HealthScorer {
            weights: [15, 25, 30, 20, 10],
            scores: [None; STAGE_COUNT],
            clamped: 0,
        }
    }

    /// 调整权重：整体钳制到 [0, 100]，调用方需保证和为 100 才评分有效。
    pub fn set_weight(&mut self, stage: Stage, weight: u32) {
        if weight > 100 {
            self.clamped += 1;
            self.weights[stage.index()] = 100;
        } else {
            self.weights[stage.index()] = weight;
        }
    }

    /// 权重和是否守恒（=100）。
    pub fn weights_conserved(&self) -> bool {
        self.weights.iter().fold(0u32, |a, w| a.saturating_add(*w)) == WEIGHT_SUM
    }

    /// 打分：>100 或未列阶段 → 记钳制并忽略。
    pub fn score(&mut self, stage: Stage, value: u8) -> bool {
        if value > 100 {
            self.clamped += 1;
            return false;
        }
        self.scores[stage.index()] = Some(value);
        true
    }

    /// 单阶段读取；未打分返回 0（保守）。
    pub fn stage_score(&self, stage: Stage) -> u8 {
        self.scores[stage.index()].unwrap_or(0)
    }

    /// 加权总分（未打分阶段按 0 计）。
    pub fn total(&self) -> u32 {
        let mut acc = 0u32;
        for s in Stage::ALL {
            let v = self.stage_score(s) as u32;
            acc = acc.saturating_add(v.saturating_mul(self.weights[s.index()]));
        }
        acc / WEIGHT_SUM
    }

    /// 健康档位。
    pub fn tier(&self) -> HealthTier {
        HealthTier::from_score(self.total())
    }

    /// 最薄弱阶段（未全打分时返回 None）。
    pub fn weakest(&self) -> Option<Stage> {
        let mut best: Option<(u8, Stage)> = None;
        for s in Stage::ALL {
            if self.scores[s.index()].is_none() {
                return None;
            }
            let v = self.stage_score(s);
            match best {
                Some((bv, _)) if bv <= v => {}
                _ => best = Some((v, s)),
            }
        }
        best.map(|(_, s)| s)
    }

    /// 快照导出（快照/迁移体系：字节化载荷，跨版本携带）。
    pub fn snapshot(&self) -> [u8; STAGE_COUNT] {
        let mut out = [0u8; STAGE_COUNT];
        for s in Stage::ALL {
            out[s.index()] = self.stage_score(s);
        }
        out
    }

    /// 从快照导入：越界值钳回 0 并记钳制（导入不崩溃）。
    pub fn restore(&mut self, snap: &[u8; STAGE_COUNT]) {
        for s in Stage::ALL {
            let v = snap[s.index()];
            if v > 100 {
                self.clamped += 1;
                self.scores[s.index()] = Some(0);
            } else {
                self.scores[s.index()] = Some(v);
            }
        }
    }

    /// 卸载净身：回到出厂（未打分 + 默认权重）。
    pub fn reset(&mut self) {
        *self = HealthScorer::new();
    }
}

/// 族0002 域自检。
pub fn run_health_checks() -> CheckSet {
    let mut set = CheckSet::new("bootchain.health");
    let mut h = HealthScorer::new();
    set.add("weights sum 100", h.weights_conserved(), "");
    set.add("default tier empty is critical", h.tier() == HealthTier::Critical, "");
    let scored = h.score(Stage::Kernel, 100);
    set.add("single score accepted", scored && h.stage_score(Stage::Kernel) == 100, "");
    let over = h.score(Stage::Loader, 101);
    set.add("over-100 clamped", !over && h.clamped == 1, "");
    set.add("weight cap 100", {
        h.set_weight(Stage::Loader, 250);
        h.weights[Stage::Loader.index()] == 100
    }, "");
    set.add("tier thresholds", {
        HealthTier::from_score(85) == HealthTier::Excellent
            && HealthTier::from_score(70) == HealthTier::Good
            && HealthTier::from_score(50) == HealthTier::Fair
            && HealthTier::from_score(30) == HealthTier::Poor
            && HealthTier::from_score(0) == HealthTier::Critical
    }, "");
    set.add("weakest needs full scores", h.weakest().is_none(), "");
    set.add("snapshot restore roundtrip", {
        let mut a = HealthScorer::new();
        for s in Stage::ALL {
            a.score(s, 88);
        }
        let snap = a.snapshot();
        let mut b = HealthScorer::new();
        b.restore(&snap);
        a.total() == b.total() && b.tier() == a.tier()
    }, "");
    set.add("restore corrupt clamps to 0", {
        let mut b = HealthScorer::new();
        b.restore(&[200, 0, 0, 0, 0]);
        b.clamped == 1 && b.stage_score(Stage::Firmware) == 0
    }, "");
    set.add("reset clean", {
        let mut b = HealthScorer::new();
        b.score(Stage::Init, 50);
        b.reset();
        b.stage_score(Stage::Init) == 0 && b.clamped == 0
    }, "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full(v: u8) -> HealthScorer {
        let mut h = HealthScorer::new();
        for s in Stage::ALL {
            h.score(s, v);
        }
        h
    }

    #[test]
    fn x00026_min_loop_scores_and_tiers() {
        assert_eq!(full(100).total(), 100);
        assert_eq!(full(100).tier(), HealthTier::Excellent);
        assert_eq!(full(60).total(), 60);
        assert_eq!(full(60).tier(), HealthTier::Fair);
        assert_eq!(full(20).tier(), HealthTier::Critical);
    }

    #[test]
    fn x00031_clamps_never_panic() {
        let mut h = HealthScorer::new();
        assert!(!h.score(Stage::Kernel, 255));
        h.set_weight(Stage::Kernel, 999);
        assert!(h.weights[Stage::Kernel.index()] == 100);
    }

    #[test]
    fn x00029_snapshot_migration_channel() {
        let a = full(77);
        let snap = a.snapshot();
        let mut b = HealthScorer::new();
        b.restore(&snap);
        assert_eq!(a.total(), b.total());
        assert_eq!(b.total(), 77);
    }

    #[test]
    fn x00035_reset_uninstall_clean() {
        let mut h = full(90);
        h.reset();
        assert_eq!(h.total(), 0);
        assert!(h.weights_conserved());
    }

    #[test]
    fn x00026_run_checks_pass() {
        assert!(run_health_checks().all_passed());
    }
}
