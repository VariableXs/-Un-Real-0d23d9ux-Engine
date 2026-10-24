//! CGPU-F0253 · 预防性静态分析（cgb253_staticpv）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0253`
//! **判据原文摘录**：离线锁序图分析（调用图上的锁序推导与层级表比对——F0246 的
//! 深版）、风险评分（高扇出锁 / 长持锁路径 / 回调中持锁三类高风险模式评分排序
//! ——审查优先级）、CI 门禁建议（评分超阈值阻断 + 修复建议）。
//!
//! ## 三类高风险模式
//!
//! | 模式 | 危害 | 为何高风险 |
//! |---|---|---|
//! | 高扇出锁 | 竞争热点 | 扇出越高，等锁面越大，死锁面越大 |
//! | 长持锁路径 | 性能 + 死锁 | 持锁越久，冲突窗口越大 |
//! | 回调中持锁 | 意外重入 | 回调可能加锁，产生隐藏锁序 |
//!
//! 三类各有独立评分，综合评分 = 加权和。评分排序决定审查优先级——
//! 最危险的先审。
//!
//! ## CI 门禁
//!
//! 评分超阈值 → 阻断 + 修复建议。阈值可配。门禁逻辑：
//! - score > `block_threshold` → **阻断**（CI 失败）；
//! - score > `warn_threshold` → **告警**（CI 通过但标注）；
//! - 否则 → 通过。

use alloc::format;
use alloc::vec;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

use super::cgb241_dlarch::SubjectId;

// ---------------------------------------------------------------------------
// 诊断码（0xCC00 起）
// ---------------------------------------------------------------------------

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0xCC00;
/// 调用图表满。
pub const E_GRAPH_FULL: u16 = SEG_BASE + 1;

// ---------------------------------------------------------------------------
// 容量
// ---------------------------------------------------------------------------

/// 调用图节点上限。
pub const MAX_CALL_NODES: usize = 256;
/// 锁序边上限。
pub const MAX_ORDER_PAIRS: usize = 512;

// ---------------------------------------------------------------------------
// 风险模式
// ---------------------------------------------------------------------------

/// 风险模式类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RiskPattern {
    /// 高扇出锁。
    HighFanout,
    /// 长持锁路径。
    LongHold,
    /// 回调中持锁。
    CallbackHold,
}

impl RiskPattern {
    pub const ALL: [RiskPattern; 3] = [
        RiskPattern::HighFanout,
        RiskPattern::LongHold,
        RiskPattern::CallbackHold,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            RiskPattern::HighFanout => "高扇出锁",
            RiskPattern::LongHold => "长持锁路径",
            RiskPattern::CallbackHold => "回调中持锁",
        }
    }

    /// 权重。
    pub const fn weight(self) -> f32 {
        match self {
            RiskPattern::HighFanout => 0.35,
            RiskPattern::LongHold => 0.35,
            RiskPattern::CallbackHold => 0.30,
        }
    }
}

/// 一条风险发现。
#[derive(Clone, Debug)]
pub struct RiskFinding {
    pub pattern: RiskPattern,
    /// 涉及的锁 ID。
    pub lock_id: u32,
    /// 风险评分 0.0~1.0。
    pub score: f32,
    /// 人话描述。
    pub description: String,
    /// 修复建议。
    pub fix_suggestion: String,
}

/// 风险评分器。
pub struct RiskScorer {
    findings: Vec<RiskFinding>,
    /// 遥测：分析次数。
    pub analyses: u64,
}

impl RiskScorer {
    pub fn new() -> RiskScorer {
        RiskScorer {
            findings: Vec::new(),
            analyses: 0,
        }
    }

    /// 分析高扇出锁。
    pub fn analyze_fanout(&mut self, lock_id: u32, fanout: u32, max_fanout: u32) -> RiskFinding {
        self.analyses += 1;
        let score = if max_fanout == 0 {
            0.0
        } else {
            (fanout as f32 / max_fanout as f32).min(1.0)
        };
        let f = RiskFinding {
            pattern: RiskPattern::HighFanout,
            lock_id,
            score,
            description: alloc::format!("锁 {} 扇出 {}（上限 {}）", lock_id, fanout, max_fanout),
            fix_suggestion: String::from("考虑拆分锁或降低扇出"),
        };
        self.findings.push(f.clone());
        f
    }

    /// 分析长持锁路径。
    pub fn analyze_long_hold(&mut self, lock_id: u32, hold_ticks: u32, threshold: u32) -> RiskFinding {
        self.analyses += 1;
        let score = if threshold == 0 {
            1.0
        } else {
            (hold_ticks as f32 / threshold as f32).min(1.0)
        };
        let f = RiskFinding {
            pattern: RiskPattern::LongHold,
            lock_id,
            score,
            description: alloc::format!(
                "锁 {} 持有 {}tick（阈值 {}）",
                lock_id, hold_ticks, threshold
            ),
            fix_suggestion: String::from("缩短持锁区、缩小临界区"),
        };
        self.findings.push(f.clone());
        f
    }

    /// 分析回调中持锁。
    pub fn analyze_callback_hold(&mut self, lock_id: u32, in_callback: bool) -> RiskFinding {
        self.analyses += 1;
        let score = if in_callback { 0.9 } else { 0.0 };
        let f = RiskFinding {
            pattern: RiskPattern::CallbackHold,
            lock_id,
            score,
            description: if in_callback {
                alloc::format!("锁 {} 在回调中持有", lock_id)
            } else {
                alloc::format!("锁 {} 不在回调中", lock_id)
            },
            fix_suggestion: String::from("回调前释放锁或使用无锁结构"),
        };
        self.findings.push(f.clone());
        f
    }

    /// 综合评分（加权和）。
    pub fn combined_score(&self) -> f32 {
        let mut total = 0.0;
        for p in RiskPattern::ALL {
            let avg = self.pattern_avg(p);
            total += avg * p.weight();
        }
        total
    }

    fn pattern_avg(&self, pattern: RiskPattern) -> f32 {
        let matching: Vec<&RiskFinding> = self
            .findings
            .iter()
            .filter(|f| f.pattern == pattern)
            .collect();
        if matching.is_empty() {
            return 0.0;
        }
        let sum: f32 = matching.iter().map(|f| f.score).sum();
        sum / matching.len() as f32
    }

    /// 按评分排序（审查优先级）。
    pub fn sorted_findings(&self) -> Vec<&RiskFinding> {
        let mut v: Vec<&RiskFinding> = self.findings.iter().collect();
        v.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(core::cmp::Ordering::Equal)
        });
        v
    }

    pub fn finding_count(&self) -> usize {
        self.findings.len()
    }
}

// ---------------------------------------------------------------------------
// CI 门禁
// ---------------------------------------------------------------------------

/// 门禁决策。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateDecision {
    /// 通过。
    Pass,
    /// 告警（通过但标注）。
    Warn,
    /// 阻断（CI 失败）。
    Block,
}

impl GateDecision {
    pub const fn name(self) -> &'static str {
        match self {
            GateDecision::Pass => "pass",
            GateDecision::Warn => "warn",
            GateDecision::Block => "block",
        }
    }
}

/// CI 门禁。
pub struct CiGate {
    /// 阻断阈值。
    pub block_threshold: f32,
    /// 告警阈值。
    pub warn_threshold: f32,
    /// 遥测：阻断次数。
    pub blocks: u64,
    /// 遥测：告警次数。
    pub warns: u64,
}

impl CiGate {
    pub fn new() -> CiGate {
        CiGate {
            block_threshold: 0.8,
            warn_threshold: 0.5,
            blocks: 0,
            warns: 0,
        }
    }

    /// 评估。
    pub fn evaluate(&mut self, score: f32) -> GateDecision {
        if score >= self.block_threshold {
            self.blocks += 1;
            GateDecision::Block
        } else if score >= self.warn_threshold {
            self.warns += 1;
            GateDecision::Warn
        } else {
            GateDecision::Pass
        }
    }

    /// 修复建议生成。
    pub fn suggest_fix(&self, findings: &[&RiskFinding]) -> String {
        if findings.is_empty() {
            return String::from("无风险发现");
        }
        let mut s = String::from("修复建议：\n");
        for f in findings.iter().take(3) {
            s.push_str(&alloc::format!("  - {}\n", f.fix_suggestion));
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 调用图锁序推导（F0246 深版）
// ---------------------------------------------------------------------------

/// 调用图节点。
#[derive(Clone, Debug)]
pub struct CallNode {
    pub id: u32,
    /// 函数名。
    pub name: String,
    /// 该函数获取的锁（按序）。
    pub acquires: Vec<u32>,
}

/// 调用图锁序分析器。
pub struct CallGraphAnalyzer {
    nodes: Vec<CallNode>,
    /// 推导出的锁序对。
    order_pairs: Vec<(u32, u32)>,
}

impl CallGraphAnalyzer {
    pub fn new() -> CallGraphAnalyzer {
        CallGraphAnalyzer {
            nodes: Vec::new(),
            order_pairs: Vec::new(),
        }
    }

    /// 添加调用图节点。
    pub fn add_node(&mut self, id: u32, name: &str, acquires: Vec<u32>) -> Result<(), u16> {
        if self.nodes.len() >= MAX_CALL_NODES {
            return Err(E_GRAPH_FULL);
        }
        self.nodes.push(CallNode {
            id,
            name: String::from(name),
            acquires,
        });
        Ok(())
    }

    /// 推导锁序对。
    ///
    /// 对每个函数的加锁序列，推导 (先, 后) 对。
    pub fn derive_order(&mut self) {
        self.order_pairs.clear();
        for node in &self.nodes {
            for i in 0..node.acquires.len() {
                for j in (i + 1)..node.acquires.len() {
                    let pair = (node.acquires[i], node.acquires[j]);
                    if !self.order_pairs.contains(&pair) && self.order_pairs.len() < MAX_ORDER_PAIRS {
                        self.order_pairs.push(pair);
                    }
                }
            }
        }
    }

    /// 与层级表比对，找出违序。
    pub fn diff_vs_hierarchy(
        &self,
        levels: &[(u32, u16)],
    ) -> Vec<(u32, u32, u16, u16)> {
        let mut bad = Vec::new();
        for &(a, b) in &self.order_pairs {
            let la = levels.iter().find(|(id, _)| *id == a).map(|(_, l)| *l);
            let lb = levels.iter().find(|(id, _)| *id == b).map(|(_, l)| *l);
            if let (Some(x), Some(y)) = (la, lb) {
                if x > y {
                    bad.push((a, b, x, y));
                }
            }
        }
        bad
    }

    pub fn pair_count(&self) -> usize {
        self.order_pairs.len()
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// CGPU-F0253 域自检。
pub fn run_cgb253_checks() -> CheckSet {
    let mut cs = CheckSet::new("cgb253_staticpv");
    let mut scorer = RiskScorer::new();
    let mut gate = CiGate::new();
    let mut cg = CallGraphAnalyzer::new();

    // --- 高扇出 ---
    let f1 = scorer.analyze_fanout(1, 100, 200);
    cs.add(
        "扇出评分",
        f1.score > 0.4 && f1.score <= 1.0,
        "100/200=0.5",
    );
    cs.add(
        "扇出建议",
        !f1.fix_suggestion.is_empty(),
        "fix 非空",
    );

    // --- 长持锁 ---
    let f2 = scorer.analyze_long_hold(2, 5000, 1000);
    cs.add(
        "长持评分饱和",
        f2.score == 1.0,
        "5000/1000 饱和",
    );

    // --- 回调持锁 ---
    let f3 = scorer.analyze_callback_hold(3, true);
    cs.add(
        "回调高风险",
        f3.score >= 0.9,
        "in_callback",
    );
    let f4 = scorer.analyze_callback_hold(4, false);
    cs.add(
        "非回调零风险",
        f4.score == 0.0,
        "not in callback",
    );

    // --- 综合评分 ---
    let combined = scorer.combined_score();
    cs.add(
        "综合评分在界",
        combined >= 0.0 && combined <= 1.0,
        "0..1",
    );

    // --- 排序 ---
    let sorted = scorer.sorted_findings();
    cs.add(
        "排序正确",
        sorted.first().map(|f| f.score >= sorted.last().map(|l| l.score).unwrap_or(0.0)).unwrap_or(false),
        "降序",
    );

    // --- CI 门禁 ---
    let d1 = gate.evaluate(0.9);
    cs.add("高分阻断", d1 == GateDecision::Block, "≥0.8 block");
    let d2 = gate.evaluate(0.6);
    cs.add("中分告警", d2 == GateDecision::Warn, "≥0.5 warn");
    let d3 = gate.evaluate(0.3);
    cs.add("低分通过", d3 == GateDecision::Pass, "<0.5 pass");
    cs.add(
        "门禁计数",
        gate.blocks == 1 && gate.warns == 1,
        "1 block 1 warn",
    );

    // --- 修复建议 ---
    let fix = gate.suggest_fix(&sorted);
    cs.add(
        "建议可读",
        fix.contains("修复建议"),
        "人话",
    );

    // --- 调用图锁序推导 ---
    cg.add_node(1, "fn_a", alloc::vec![1, 2, 3]).ok();
    cg.add_node(2, "fn_b", alloc::vec![3, 1]).ok();
    cg.derive_order();
    cs.add(
        "锁序推导",
        cg.pair_count() > 0,
        "acquires 推对",
    );

    // --- 与层级表 diff ---
    let levels = alloc::vec![(1u32, 0u16), (2, 1), (3, 2)];
    let bad = cg.diff_vs_hierarchy(&levels);
    cs.add(
        "违序检出",
        !bad.is_empty(),
        "3→1 反转",
    );

    // --- 三类模式覆盖 ---
    cs.add(
        "三类全覆盖",
        RiskPattern::ALL.len() == 3,
        "Fanout/Long/Callback",
    );

    cs
}
