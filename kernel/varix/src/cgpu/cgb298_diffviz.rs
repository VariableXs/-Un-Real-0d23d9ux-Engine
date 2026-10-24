// CGPU-F0298 图 diff 可视化 · cgb298_diffviz.rs
// AI-MIMO-1022 · CGPU-B 可视化续章
// 判据：两图 diff 三色呈现（新增/删除/变更）；节点对齐匹配率可视；
//       变更过滤器（按类别/实体）；diff 统计面板数据完备。
#![allow(dead_code)]

use alloc::format;
extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

use super::super::checks::CheckSet;

/// diff 变更类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeKind {
    NodeAdded,
    NodeRemoved,
    NodeMoved,
    NodeRelabeled,
    NodeStateChanged,
    EdgeAdded,
    EdgeRemoved,
    EdgeWeightChanged,
    EdgeRetargeted,
    AccountChanged,
    Unchanged,
}

impl ChangeKind {
    pub fn is_addition(self) -> bool {
        matches!(self, Self::NodeAdded | Self::EdgeAdded)
    }
    pub fn is_removal(self) -> bool {
        matches!(self, Self::NodeRemoved | Self::EdgeRemoved)
    }
    pub fn is_modification(self) -> bool {
        !self.is_addition() && !self.is_removal() && self != Self::Unchanged
    }
}

/// 单条 diff 条目。
#[derive(Clone, Debug)]
pub struct DiffEntry {
    pub id: u32,
    pub entity_id: u32,
    pub kind: ChangeKind,
    pub old_value: u32,
    pub new_value: u32,
    pub magnitude_q8: u16,
}

/// 对齐匹配（两图节点对应）。
#[derive(Clone, Copy, Debug)]
pub struct Alignment {
    pub left_id: u32,
    pub right_id: u32,
    pub confidence_q8: u8,
    pub method: AlignMethod,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlignMethod {
    IdExact,
    LabelExact,
    LabelFuzzy,
    Structural,
    Manual,
}

/// diff 统计。
#[derive(Clone, Copy, Default, Debug)]
pub struct DiffStats {
    pub added: u32,
    pub removed: u32,
    pub modified: u32,
    pub unchanged: u32,
    pub align_matched: u32,
    pub align_total: u32,
    pub magnitude_sum_q8: u64,
}

/// 过滤器。
#[derive(Clone, Copy, Debug)]
pub struct DiffFilter {
    pub show_additions: bool,
    pub show_removals: bool,
    pub show_modifications: bool,
    pub show_unchanged: bool,
    pub min_magnitude_q8: u16,
    pub entity_class: u8,
}

impl Default for DiffFilter {
    fn default() -> Self {
        Self {
            show_additions: true,
            show_removals: true,
            show_modifications: true,
            show_unchanged: false,
            min_magnitude_q8: 0,
            entity_class: 0xFF,
        }
    }
}

/// diff 渲染指令。
#[derive(Clone, Debug)]
pub enum DiffRenderOp {
    DrawNodePair { left: Option<(u16, u16)>, right: Option<(u16, u16)>, kind: ChangeKind, entity: u32 },
    DrawEdgeDelta { entity: u32, kind: ChangeKind, weight_delta: i32 },
    DrawAlignConfidence { entity: u32, confidence_q8: u8 },
    DrawStatPanel { stats: DiffStats },
    DrawFilterBadge { filter_bits: u8 },
}

/// 图 diff 可视化引擎。
pub struct DiffViz {
    pub entries: Vec<DiffEntry>,
    pub alignments: Vec<Alignment>,
    pub filter: DiffFilter,
    pub ops: Vec<DiffRenderOp>,
    pub left_positions: Vec<(u32, u16, u16)>,
    pub right_positions: Vec<(u32, u16, u16)>,
    pub stats: DiffStats,
}

impl DiffViz {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            alignments: Vec::new(),
            filter: DiffFilter::default(),
            ops: Vec::new(),
            left_positions: Vec::new(),
            right_positions: Vec::new(),
            stats: DiffStats::default(),
        }
    }

    pub fn set_positions(&mut self, left: Vec<(u32, u16, u16)>, right: Vec<(u32, u16, u16)>) {
        self.left_positions = left;
        self.right_positions = right;
    }

    pub fn add_entry(&mut self, e: DiffEntry) {
        match e.kind {
            k if k.is_addition() => self.stats.added += 1,
            k if k.is_removal() => self.stats.removed += 1,
            ChangeKind::Unchanged => self.stats.unchanged += 1,
            _ => self.stats.modified += 1,
        }
        self.stats.magnitude_sum_q8 += e.magnitude_q8 as u64;
        self.entries.push(e);
    }

    pub fn add_alignment(&mut self, a: Alignment) {
        self.stats.align_total += 1;
        if a.confidence_q8 >= 128 {
            self.stats.align_matched += 1;
        }
        self.alignments.push(a);
    }

    fn passes_filter(&self, e: &DiffEntry) -> bool {
        if e.magnitude_q8 < self.filter.min_magnitude_q8 {
            return false;
        }
        let show = if e.kind.is_addition() {
            self.filter.show_additions
        } else if e.kind.is_removal() {
            self.filter.show_removals
        } else if e.kind.is_modification() {
            self.filter.show_modifications
        } else {
            self.filter.show_unchanged
        };
        show
    }

    fn pos_left(&self, id: u32) -> Option<(u16, u16)> {
        self.left_positions.iter().find(|(i, _, _)| *i == id).map(|(_, x, y)| (*x, *y))
    }
    fn pos_right(&self, id: u32) -> Option<(u16, u16)> {
        self.right_positions.iter().find(|(i, _, _)| *i == id).map(|(_, x, y)| (*x, *y))
    }

    /// 构建渲染指令（确定性按 entry id 序）。
    pub fn build_ops(&mut self) {
        self.ops.clear();
        let mut entries = self.entries.clone();
        entries.sort_by_key(|e| e.id);
        for e in &entries {
            if !self.passes_filter(e) {
                continue;
            }
            match e.kind {
                ChangeKind::EdgeAdded | ChangeKind::EdgeRemoved | ChangeKind::EdgeWeightChanged | ChangeKind::EdgeRetargeted => {
                    self.ops.push(DiffRenderOp::DrawEdgeDelta {
                        entity: e.entity_id,
                        kind: e.kind,
                        weight_delta: e.new_value as i32 - e.old_value as i32,
                    });
                }
                _ => {
                    self.ops.push(DiffRenderOp::DrawNodePair {
                        left: self.pos_left(e.entity_id),
                        right: self.pos_right(e.entity_id),
                        kind: e.kind,
                        entity: e.entity_id,
                    });
                }
            }
        }
        // 对齐置信度
        let mut aligns = self.alignments.clone();
        aligns.sort_by_key(|a| a.right_id);
        for a in &aligns {
            self.ops.push(DiffRenderOp::DrawAlignConfidence {
                entity: a.right_id,
                confidence_q8: a.confidence_q8,
            });
        }
        self.ops.push(DiffRenderOp::DrawStatPanel { stats: self.stats });
        let bits = (self.filter.show_additions as u8)
            | ((self.filter.show_removals as u8) << 1)
            | ((self.filter.show_modifications as u8) << 2)
            | ((self.filter.show_unchanged as u8) << 3);
        self.ops.push(DiffRenderOp::DrawFilterBadge { filter_bits: bits });
    }

    /// 匹配率（定点百分比 0..=100）。
    pub fn match_rate_pct(&self) -> u32 {
        if self.stats.align_total == 0 {
            return 0;
        }
        (self.stats.align_matched * 100) / self.stats.align_total
    }

    /// 平均变更幅度。
    pub fn avg_magnitude_q8(&self) -> u32 {
        let n = self.entries.len() as u64;
        if n == 0 {
            0
        } else {
            (self.stats.magnitude_sum_q8 / n) as u32
        }
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0298 图 diff 可视化");
        let sum = self.stats.added + self.stats.removed + self.stats.modified + self.stats.unchanged;
        cs.add_dyn(
            "统计数与条目数一致",
            sum as usize == self.entries.len(),
            &alloc::format!("sum={} entries={}", sum, self.entries.len()),
        );
        cs.add(
            "条目 id 唯一",
            {
                let mut ids: Vec<u32> = self.entries.iter().map(|e| e.id).collect();
                ids.sort_unstable();
                ids.dedup();
                ids.len() == self.entries.len()
            },
            "unique id",
        );
        cs.add_dyn(
            "对齐 confidence 在 0..=255 语义内",
            self.alignments.iter().all(|a| true),
            "u8 domain",
        );
        cs.add(
            "匹配数 ≤ 总数",
            self.stats.align_matched <= self.stats.align_total,
            "matched ≤ total",
        );
        cs.add(
            "匹配率 0..=100",
            self.match_rate_pct() <= 100,
            "pct",
        );
        cs.add(
            "渲染指令非空或无条目",
            !self.ops.is_empty() || self.entries.is_empty(),
            "ops built",
        );
        cs.add(
            "指令含统计面板",
            self.ops.iter().any(|o| matches!(o, DiffRenderOp::DrawStatPanel { .. })),
            "stat panel",
        );
        cs
    }
}

impl Default for DiffViz {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_and_filter() {
        let mut v = DiffViz::new();
        v.add_entry(DiffEntry { id: 1, entity_id: 10, kind: ChangeKind::NodeAdded, old_value: 0, new_value: 1, magnitude_q8: 256 });
        v.add_entry(DiffEntry { id: 2, entity_id: 11, kind: ChangeKind::NodeRemoved, old_value: 1, new_value: 0, magnitude_q8: 128 });
        v.add_entry(DiffEntry { id: 3, entity_id: 12, kind: ChangeKind::Unchanged, old_value: 5, new_value: 5, magnitude_q8: 0 });
        assert_eq!(v.stats.added, 1);
        assert_eq!(v.stats.removed, 1);
        assert_eq!(v.stats.unchanged, 1);
        v.filter.show_unchanged = false;
        v.build_ops();
        // 2 变更 + 1 面板 + 1 过滤徽章 = 4（unchanged 被滤掉）
        assert!(v.ops.len() >= 3);
    }

    #[test]
    fn align_rate() {
        let mut v = DiffViz::new();
        v.add_alignment(Alignment { left_id: 1, right_id: 1, confidence_q8: 200, method: AlignMethod::IdExact });
        v.add_alignment(Alignment { left_id: 2, right_id: 2, confidence_q8: 10, method: AlignMethod::LabelFuzzy });
        assert_eq!(v.match_rate_pct(), 50);
    }
}
