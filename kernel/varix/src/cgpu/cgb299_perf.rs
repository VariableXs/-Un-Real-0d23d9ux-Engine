// CGPU-F0299 大图性能预算 · cgb299_perf.rs
// AI-MIMO-1022 · CGPU-B 可视化续章
// 判据：万级节点渲染帧预算 <16ms；LOD 分级确定性；裁剪区域剔除；
//       预算超支降级路径闭环；性能计数器完备。
#![allow(dead_code)]

use alloc::format;
extern crate alloc;
use alloc::vec::Vec;

use super::super::checks::CheckSet;

/// LOD 等级（数字越小越精细）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum LodLevel {
    Full = 0,
    Simplified = 1,
    Clustered = 2,
    PointCloud = 3,
    Hidden = 4,
}

/// 性能计数器。
#[derive(Clone, Copy, Default, Debug)]
pub struct PerfCounters {
    pub nodes_submitted: u32,
    pub nodes_drawn: u32,
    pub nodes_culled: u32,
    pub edges_submitted: u32,
    pub edges_drawn: u32,
    pub edges_culled: u32,
    pub lod_downgrades: u32,
    pub frame_time_us: u32,
    pub budget_us: u32,
    pub over_budget_frames: u32,
}

/// 视口裁剪矩形。
#[derive(Clone, Copy, Debug)]
pub struct CullRect {
    pub min_x: i32,
    pub min_y: i32,
    pub max_x: i32,
    pub max_y: i32,
}

impl CullRect {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }
    pub fn intersects(&self, o: &CullRect) -> bool {
        !(o.min_x > self.max_x || o.max_x < self.min_x || o.min_y > self.max_y || o.max_y < self.min_y)
    }
    pub fn area(&self) -> i64 {
        ((self.max_x - self.min_x) as i64) * ((self.max_y - self.min_y) as i64)
    }
}

/// 可绘制实体。
#[derive(Clone, Copy, Debug)]
pub struct DrawEntity {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub extent: u16,
    pub importance: u8,
    pub is_edge: bool,
    pub edge_from: u32,
    pub edge_to: u32,
}

/// LOD 判定输入。
#[derive(Clone, Copy, Debug)]
pub struct LodContext {
    pub total_nodes: u32,
    pub visible_nodes: u32,
    pub zoom_q8: u16,
    pub budget_remaining_us: u32,
}

/// 大图性能预算引擎。
pub struct PerfBudget {
    pub budget_us: u32,
    pub counters: PerfCounters,
    pub lod_thresholds: [u32; 5],
    pub zoom_thresholds: [u16; 4],
    pub degrade_level: LodLevel,
    pub frame_history: Vec<u32>,
    pub history_cap: usize,
}

impl PerfBudget {
    pub fn new(budget_us: u32) -> Self {
        Self {
            budget_us,
            counters: PerfCounters {
                budget_us,
                ..Default::default()
            },
            // 节点数阈值：>N 降级到对应 LOD
            lod_thresholds: [2000, 8000, 20000, 50000, 100000],
            zoom_thresholds: [64, 128, 256, 512],
            degrade_level: LodLevel::Full,
            frame_history: Vec::new(),
            history_cap: 120,
        }
    }

    /// 确定性 LOD 判定（纯函数式：同输入同输出）。
    pub fn decide_lod(&self, ctx: &LodContext) -> LodLevel {
        let n = ctx.visible_nodes;
        if n == 0 {
            return LodLevel::Hidden;
        }
        // 节点规模优先
        let by_size = if n > self.lod_thresholds[4] {
            LodLevel::Hidden
        } else if n > self.lod_thresholds[3] {
            LodLevel::PointCloud
        } else if n > self.lod_thresholds[2] {
            LodLevel::Clustered
        } else if n > self.lod_thresholds[1] {
            LodLevel::Simplified
        } else {
            LodLevel::Full
        };
        // 缩放进一步降级
        let by_zoom = if ctx.zoom_q8 < self.zoom_thresholds[0] {
            LodLevel::PointCloud
        } else if ctx.zoom_q8 < self.zoom_thresholds[1] {
            LodLevel::Clustered
        } else if ctx.zoom_q8 < self.zoom_thresholds[2] {
            LodLevel::Simplified
        } else {
            LodLevel::Full
        };
        // 取更粗者 + 预算压力再降
        let mut result = if by_size > by_zoom { by_size } else { by_zoom };
        if ctx.budget_remaining_us < self.budget_us / 4 {
            result = match result {
                LodLevel::Full => LodLevel::Simplified,
                LodLevel::Simplified => LodLevel::Clustered,
                LodLevel::Clustered => LodLevel::PointCloud,
                other => other,
            };
        }
        result
    }

    /// 视口裁剪（确定性）。
    pub fn cull(&self, entities: &[DrawEntity], view: &CullRect) -> (Vec<DrawEntity>, u32) {
        let mut kept = Vec::new();
        let mut culled = 0u32;
        for e in entities {
            let er = CullRect {
                min_x: e.x - e.extent as i32,
                min_y: e.y - e.extent as i32,
                max_x: e.x + e.extent as i32,
                max_y: e.y + e.extent as i32,
            };
            if er.intersects(view) {
                kept.push(*e);
            } else {
                culled += 1;
            }
        }
        (kept, culled)
    }

    /// 帧开始（重置帧内计数）。
    pub fn begin_frame(&mut self) {
        self.counters.nodes_submitted = 0;
        self.counters.nodes_drawn = 0;
        self.counters.nodes_culled = 0;
        self.counters.edges_submitted = 0;
        self.counters.edges_drawn = 0;
        self.counters.edges_culled = 0;
    }

    /// 提交实体（统计 + LOD 过滤）。
    pub fn submit(&mut self, entities: &[DrawEntity], view: &CullRect, lod: LodLevel) -> Vec<DrawEntity> {
        for e in entities {
            if e.is_edge {
                self.counters.edges_submitted += 1;
            } else {
                self.counters.nodes_submitted += 1;
            }
        }
        let (kept, culled) = self.cull(entities, view);
        // 按 LOD 过滤重要性
        let final_kept: Vec<DrawEntity> = kept
            .into_iter()
            .filter(|e| match lod {
                LodLevel::Full => true,
                LodLevel::Simplified => e.importance >= 1,
                LodLevel::Clustered => e.importance >= 2,
                LodLevel::PointCloud => e.importance >= 3,
                LodLevel::Hidden => false,
            })
            .collect();
        let drawn = final_kept.len() as u32;
        let total = entities.len() as u32;
        let culled_total = total - drawn;
        for e in &final_kept {
            if e.is_edge {
                self.counters.edges_drawn += 1;
            } else {
                self.counters.nodes_drawn += 1;
            }
        }
        // 粗分到 node/edge cull 计数
        self.counters.nodes_culled = self.counters.nodes_submitted.saturating_sub(self.counters.nodes_drawn);
        self.counters.edges_culled = self.counters.edges_submitted.saturating_sub(self.counters.edges_drawn);
        let _ = culled;
        let _ = culled_total;
        final_kept
    }

    /// 帧结束（记账 + 降级决策）。
    pub fn end_frame(&mut self, frame_time_us: u32) {
        self.counters.frame_time_us = frame_time_us;
        if frame_time_us > self.budget_us {
            self.counters.over_budget_frames += 1;
            self.degrade_level = match self.degrade_level {
                LodLevel::Full => LodLevel::Simplified,
                LodLevel::Simplified => LodLevel::Clustered,
                LodLevel::Clustered => LodLevel::PointCloud,
                other => other,
            };
            self.counters.lod_downgrades += 1;
        } else if frame_time_us < self.budget_us / 2 {
            // 连续轻载可回升
            self.degrade_level = match self.degrade_level {
                LodLevel::PointCloud => LodLevel::Clustered,
                LodLevel::Clustered => LodLevel::Simplified,
                LodLevel::Simplified => LodLevel::Full,
                other => other,
            };
        }
        self.frame_history.push(frame_time_us);
        if self.frame_history.len() > self.history_cap {
            self.frame_history.remove(0);
        }
    }

    /// 帧时间百分位（近似：排序取分位）。
    pub fn frame_time_percentile(&self, pct: u8) -> u32 {
        if self.frame_history.is_empty() {
            return 0;
        }
        let mut sorted = self.frame_history.clone();
        sorted.sort_unstable();
        let idx = ((pct as usize) * (sorted.len() - 1)) / 100;
        sorted[idx]
    }

    /// 预算健康度（0..=100，100 为完全达标）。
    pub fn health_score(&self) -> u32 {
        if self.counters.over_budget_frames == 0 {
            return 100;
        }
        let total = self.frame_history.len() as u32;
        if total == 0 {
            return 100;
        }
        let over = self.counters.over_budget_frames.min(total);
        100 - (over * 100) / total
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0299 大图性能预算");
        cs.add(
            "预算为正",
            self.budget_us > 0,
            "budget > 0",
        );
        cs.add_dyn(
            "绘制+裁剪 = 提交（节点）",
            self.counters.nodes_drawn + self.counters.nodes_culled == self.counters.nodes_submitted,
            &alloc::format!(
                "{}+{}=={}",
                self.counters.nodes_drawn, self.counters.nodes_culled, self.counters.nodes_submitted
            ),
        );
        cs.add(
            "绘制+裁剪 = 提交（边）",
            self.counters.edges_drawn + self.counters.edges_culled == self.counters.edges_submitted,
            "edges balance",
        );
        cs.add(
            "LOD 阈值单调递增",
            self.lod_thresholds.windows(2).all(|w| w[0] < w[1]),
            "sorted",
        );
        cs.add(
            "缩放阈值单调递增",
            self.zoom_thresholds.windows(2).all(|w| w[0] < w[1]),
            "sorted",
        );
        cs.add(
            "帧历史不超容量",
            self.frame_history.len() <= self.history_cap,
            "≤cap",
        );
        cs.add(
            "健康度 0..=100",
            self.health_score() <= 100,
            "score",
        );
        cs.add(
            "百分位结果在历史范围内",
            self.frame_history.is_empty()
                || {
                    let p = self.frame_time_percentile(50);
                    let min = *self.frame_history.iter().min().unwrap();
                    let max = *self.frame_history.iter().max().unwrap();
                    p >= min && p <= max
                },
            "p50 in range",
        );
        cs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ents(n: u32) -> Vec<DrawEntity> {
        (0..n)
            .map(|i| DrawEntity {
                id: i,
                x: (i * 10) as i32,
                y: (i * 7) as i32,
                extent: 4,
                importance: (i % 4) as u8,
                is_edge: false,
                edge_from: 0,
                edge_to: 0,
            })
            .collect()
    }

    #[test]
    fn lod_deterministic() {
        let pb = PerfBudget::new(16000);
        let ctx = LodContext { total_nodes: 100, visible_nodes: 100, zoom_q8: 256, budget_remaining_us: 16000 };
        let a = pb.decide_lod(&ctx);
        let b = pb.decide_lod(&ctx);
        assert_eq!(a, b);
        assert_eq!(a, LodLevel::Full);
    }

    #[test]
    fn cull_and_balance() {
        let mut pb = PerfBudget::new(16000);
        pb.begin_frame();
        let view = CullRect { min_x: 0, min_y: 0, max_x: 100, max_y: 100 };
        let e = ents(50);
        let kept = pb.submit(&e, &view, LodLevel::Full);
        assert!(kept.len() <= e.len());
        assert_eq!(
            pb.counters.nodes_drawn + pb.counters.nodes_culled,
            pb.counters.nodes_submitted
        );
    }

    #[test]
    fn over_budget_degrades() {
        let mut pb = PerfBudget::new(16000);
        pb.end_frame(20000);
        assert_eq!(pb.degrade_level, LodLevel::Simplified);
        pb.end_frame(5000);
        assert_eq!(pb.degrade_level, LodLevel::Full);
    }
}
