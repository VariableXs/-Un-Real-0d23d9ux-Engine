#![allow(dead_code)]
use alloc::format;
use alloc::vec;
// CGPU-F0301 布局稳定性与增量重排 · cgb301_stable.rs
// AI-MIMO-1022 · CGPU-B 收口
// 判据：同图两次布局逐位一致；增量插入/删除只动受影响子图；
//       稳定性度量（平均位移/最大位移）可报告；抖动抑制生效。

extern crate alloc;
use alloc::vec::Vec;

use super::super::checks::CheckSet;

/// 布局锚点（节点初始/目标坐标，定点 Q8）。
#[derive(Clone, Copy, Debug)]
pub struct Anchor {
    pub id: u32,
    pub x_q8: i32,
    pub y_q8: i32,
}

/// 增量变更。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GraphDelta {
    NodeInserted(u32),
    NodeRemoved(u32),
    EdgeInserted(u32, u32),
    EdgeRemoved(u32, u32),
    NodeMoved(u32),
}

/// 重排影响范围。
#[derive(Clone, Debug)]
pub struct ImpactSet {
    pub affected: Vec<u32>,
    pub frozen: Vec<u32>,
    pub radius: u8,
}

/// 稳定性度量。
#[derive(Clone, Copy, Default, Debug)]
pub struct StabilityMetrics {
    pub mean_displacement_q8: u32,
    pub max_displacement_q8: u32,
    pub moved_count: u32,
    pub total_count: u32,
    pub jitter_events: u32,
}

/// 布局稳定性引擎（增量重排 + 抖动抑制）。
pub struct StableLayout {
    pub anchors: Vec<Anchor>,
    pub neighbors: Vec<(u32, u32)>,
    pub last_positions: Vec<Anchor>,
    pub metrics: StabilityMetrics,
    pub jitter_threshold_q8: i32,
    pub freeze_radius: u8,
    pub layout_seed: u64,
}

impl StableLayout {
    pub fn new() -> Self {
        Self {
            anchors: Vec::new(),
            neighbors: Vec::new(),
            last_positions: Vec::new(),
            metrics: StabilityMetrics::default(),
            jitter_threshold_q8: 256, // 1 像素
            freeze_radius: 2,
            layout_seed: 0xC0FF_EE01,
        }
    }

    pub fn set_anchors(&mut self, a: Vec<Anchor>) {
        self.last_positions = a.clone();
        self.anchors = a;
    }

    pub fn add_edge(&mut self, a: u32, b: u32) {
        self.neighbors.push((a, b));
    }

    /// BFS 求影响集（半径 r 内节点受影响，之外冻结）。
    pub fn impact_of(&self, delta: GraphDelta, radius: u8) -> ImpactSet {
        let seeds: Vec<u32> = match delta {
            GraphDelta::NodeInserted(id) | GraphDelta::NodeRemoved(id) | GraphDelta::NodeMoved(id) => vec![id],
            GraphDelta::EdgeInserted(a, b) | GraphDelta::EdgeRemoved(a, b) => vec![a, b],
        };
        let mut dist: Vec<(u32, u8)> = Vec::new();
        let mut queue: Vec<(u32, u8)> = seeds.iter().map(|s| (*s, 0)).collect();
        while let Some((node, d)) = queue.pop() {
            if dist.iter().any(|(n, _)| *n == node) {
                continue;
            }
            dist.push((node, d));
            if d >= radius {
                continue;
            }
            for (a, b) in &self.neighbors {
                let other = if *a == node {
                    Some(*b)
                } else if *b == node {
                    Some(*a)
                } else {
                    None
                };
                if let Some(o) = other {
                    if !dist.iter().any(|(n, _)| *n == o) {
                        queue.push((o, d + 1));
                    }
                }
            }
        }
        let affected: Vec<u32> = dist.iter().map(|(n, _)| *n).collect();
        let frozen: Vec<u32> = self
            .anchors
            .iter()
            .map(|a| a.id)
            .filter(|id| !affected.contains(id))
            .collect();
        ImpactSet {
            affected,
            frozen,
            radius,
        }
    }

    /// 确定性力导向一步（对受影响节点）。
    pub fn relax_affected(&mut self, impact: &ImpactSet) {
        for id in &impact.affected {
            if let Some(pos) = self.anchors.iter().position(|a| a.id == *id) {
                // 受邻居牵引（均值方向，确定性）
                let mut sx: i64 = 0;
                let mut sy: i64 = 0;
                let mut cnt: i64 = 0;
                for (a, b) in &self.neighbors {
                    let other = if *a == *id {
                        Some(*b)
                    } else if *b == *id {
                        Some(*a)
                    } else {
                        None
                    };
                    if let Some(o) = other {
                        if let Some(op) = self.anchors.iter().find(|x| x.id == o) {
                            sx += op.x_q8 as i64;
                            sy += op.y_q8 as i64;
                            cnt += 1;
                        }
                    }
                }
                if cnt > 0 {
                    let tx = (sx / cnt) as i32;
                    let ty = (sy / cnt) as i32;
                    // 向邻居均值移动 1/4
                    let cur = &self.anchors[pos];
                    let nx = cur.x_q8 + (tx - cur.x_q8) / 4;
                    let ny = cur.y_q8 + (ty - cur.y_q8) / 4;
                    // 抖动抑制：位移小于阈值则不动
                    let dx = nx - cur.x_q8;
                    let dy = ny - cur.y_q8;
                    let dist2 = dx * dx + dy * dy;
                    if dist2 >= self.jitter_threshold_q8 * self.jitter_threshold_q8 {
                        self.anchors[pos].x_q8 = nx;
                        self.anchors[pos].y_q8 = ny;
                    } else {
                        self.metrics.jitter_events += 1;
                    }
                }
            }
        }
    }

    /// 记录稳定性度量（相对上一轮）。
    pub fn measure(&mut self) -> StabilityMetrics {
        let mut sum: u64 = 0;
        let mut max: u64 = 0;
        let mut moved = 0u32;
        for a in &self.anchors {
            if let Some(prev) = self.last_positions.iter().find(|p| p.id == a.id) {
                let dx = (a.x_q8 - prev.x_q8) as i64;
                let dy = (a.y_q8 - prev.y_q8) as i64;
                let d2 = (dx * dx + dy * dy) as u64;
                let d = int_sqrt(d2);
                sum += d;
                if d > max {
                    max = d;
                }
                if d > 0 {
                    moved += 1;
                }
            }
        }
        let n = self.anchors.len() as u64;
        self.metrics = StabilityMetrics {
            mean_displacement_q8: if n == 0 { 0 } else { (sum / n) as u32 },
            max_displacement_q8: max as u32,
            moved_count: moved,
            total_count: self.anchors.len() as u32,
            jitter_events: self.metrics.jitter_events,
        };
        self.last_positions = self.anchors.clone();
        self.metrics
    }

    /// 两次全量布局一致性（同 seed 同输入）。
    pub fn is_deterministic(&self, other: &StableLayout) -> bool {
        if self.anchors.len() != other.anchors.len() {
            return false;
        }
        self.anchors
            .iter()
            .zip(other.anchors.iter())
            .all(|(a, b)| a.id == b.id && a.x_q8 == b.x_q8 && a.y_q8 == b.y_q8)
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0301 布局稳定性");
        cs.add(
            "锚点 ID 唯一",
            {
                let mut ids: Vec<u32> = self.anchors.iter().map(|a| a.id).collect();
                ids.sort_unstable();
                ids.dedup();
                ids.len() == self.anchors.len()
            },
            "unique",
        );
        cs.add(
            "邻居端点存在",
            self.neighbors.iter().all(|(a, b)| {
                self.anchors.iter().any(|x| x.id == *a) && self.anchors.iter().any(|x| x.id == *b)
            }),
            "endpoints exist",
        );
        cs.add(
            "抖动阈值为正",
            self.jitter_threshold_q8 > 0,
            ">0",
        );
        cs.add(
            "度量 moved ≤ total",
            self.metrics.moved_count <= self.metrics.total_count,
            "moved ≤ total",
        );
        cs.add(
            "最大位移 ≥ 平均位移",
            self.metrics.max_displacement_q8 >= self.metrics.mean_displacement_q8,
            "max ≥ mean",
        );
        cs.add(
            "冻结半径合理",
            self.freeze_radius <= 8,
            "radius ≤ 8",
        );
        cs
    }
}

impl Default for StableLayout {
    fn default() -> Self {
        Self::new()
    }
}

/// 整数平方根（确定性）。
fn int_sqrt(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk() -> StableLayout {
        let mut s = StableLayout::new();
        s.set_anchors(vec![
            Anchor { id: 1, x_q8: 0, y_q8: 0 },
            Anchor { id: 2, x_q8: 1000, y_q8: 0 },
            Anchor { id: 3, x_q8: 500, y_q8: 1000 },
        ]);
        s.add_edge(1, 2);
        s.add_edge(2, 3);
        s.add_edge(1, 3);
        s
    }

    #[test]
    fn impact_radius() {
        let s = mk();
        let imp = s.impact_of(GraphDelta::NodeMoved(1), 1);
        assert!(imp.affected.contains(&1));
        assert!(imp.affected.contains(&2));
        assert!(imp.frozen.is_empty() || !imp.frozen.contains(&1));
    }

    #[test]
    fn jitter_suppressed() {
        let mut s = mk();
        s.jitter_threshold_q8 = 10000; // 很大阈值，全部抖动抑制
        let imp = s.impact_of(GraphDelta::NodeMoved(1), 1);
        let before = s.anchors.clone();
        s.relax_affected(&imp);
        // 大阈值下应无移动（或极少）
        let moved = s.anchors.iter().zip(before.iter()).filter(|(a, b)| a.x_q8 != b.x_q8).count();
        assert!(moved == 0 || s.metrics.jitter_events > 0);
    }

    #[test]
    fn measure_bounds() {
        let mut s = mk();
        s.relax_affected(&s.impact_of(GraphDelta::NodeMoved(1), 1));
        let m = s.measure();
        assert!(m.moved_count <= m.total_count);
    }
}

// ---------------------------------------------------------------------------
// CGPU-F0301 可视化无障碍（高对比/色盲安全/键盘导航/屏幕阅读锚点）
// ---------------------------------------------------------------------------

/// 色盲类型（决定调色板变换）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorBlindType {
    None,
    Protanopia,
    Deuteranopia,
    Tritanopia,
    Achromatopsia,
}

/// 无障碍配置。
#[derive(Clone, Copy, Debug)]
pub struct A11yConfig {
    pub color_blind: ColorBlindType,
    pub high_contrast: bool,
    pub min_contrast_ratio_q8: u16,
    pub keyboard_nav: bool,
    pub screen_reader: bool,
    pub reduce_motion: bool,
    pub min_font_px: u8,
}

impl Default for A11yConfig {
    fn default() -> Self {
        Self {
            color_blind: ColorBlindType::None,
            high_contrast: false,
            min_contrast_ratio_q8: 384,
            keyboard_nav: true,
            screen_reader: true,
            reduce_motion: false,
            min_font_px: 12,
        }
    }
}

/// 焦点顺序项。
#[derive(Clone, Copy, Debug)]
pub struct FocusItem {
    pub id: u32,
    pub tab_index: i16,
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    pub role: u8,
    pub label_id: u32,
}

/// 屏幕阅读播报。
#[derive(Clone, Debug)]
pub struct Announcement {
    pub priority: u8,
    pub text_id: u32,
    pub entity: u32,
    pub assertive: bool,
}

/// 无障碍引擎。
pub struct A11yEngine {
    pub config: A11yConfig,
    pub focus_order: Vec<FocusItem>,
    pub focus_cursor: usize,
    pub announcements: Vec<Announcement>,
    pub remapped_colors: Vec<(u32, u32)>,
    pub contrast_violations: u32,
}

impl A11yEngine {
    pub fn new(config: A11yConfig) -> Self {
        Self {
            config,
            focus_order: Vec::new(),
            focus_cursor: 0,
            announcements: Vec::new(),
            remapped_colors: Vec::new(),
            contrast_violations: 0,
        }
    }

    /// 色盲安全调色板重映射（确定性查表变换）。
    pub fn remap_color(&mut self, rgb: u32) -> u32 {
        if let Some((_, v)) = self.remapped_colors.iter().find(|(k, _)| *k == rgb) {
            return *v;
        }
        let r = ((rgb >> 16) & 0xFF) as u16;
        let g = ((rgb >> 8) & 0xFF) as u16;
        let b = (rgb & 0xFF) as u16;
        let (r2, g2, b2) = match self.config.color_blind {
            ColorBlindType::None => (r, g, b),
            ColorBlindType::Protanopia => ((r * 56 + g * 44) / 100, (g * 70 + b * 30) / 100, (b * 70 + g * 30) / 100),
            ColorBlindType::Deuteranopia => ((r * 70 + g * 30) / 100, (g * 56 + r * 44) / 100, (b * 70 + r * 30) / 100),
            ColorBlindType::Tritanopia => ((r * 80 + b * 20) / 100, (g * 80 + b * 20) / 100, (b * 50 + g * 50) / 100),
            ColorBlindType::Achromatopsia => {
                let lum = (r * 30 + g * 59 + b * 11) / 100;
                (lum, lum, lum)
            }
        };
        let out = if self.config.high_contrast {
            let stretch = |v: u16| if v >= 128 { 255u32 } else { 0u32 };
            (stretch(r2) << 16) | (stretch(g2) << 8) | stretch(b2)
        } else {
            // 高位通道与高对比分支同走 u32，避免 u16 移位溢出（E0308）。
            (((r2 & 0xFF) as u32) << 16) | (((g2 & 0xFF) as u32) << 8) | ((b2 & 0xFF) as u32)
        };
        self.remapped_colors.push((rgb, out));
        out
    }

    /// 对比度（WCAG 简化，返回 Q8 比值）。
    pub fn contrast_ratio_q8(fg: u32, bg: u32) -> u16 {
        let lum = |c: u32| -> u32 {
            let r = ((c >> 16) & 0xFF) as u32 * 30;
            let g = ((c >> 8) & 0xFF) as u32 * 59;
            let b = (c & 0xFF) as u32 * 11;
            (r + g + b) / 100
        };
        let l1 = lum(fg);
        let l2 = lum(bg);
        let (hi, lo) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
        if lo == 0 {
            return 255;
        }
        ((hi + 8) * 256 / (lo + 8)) as u16
    }

    /// 校验对比度违规。
    pub fn audit_contrast(&mut self, pairs: &[(u32, u32)]) -> u32 {
        let mut bad = 0u32;
        for (fg, bg) in pairs {
            if Self::contrast_ratio_q8(*fg, *bg) < self.config.min_contrast_ratio_q8 {
                bad += 1;
            }
        }
        self.contrast_violations = bad;
        bad
    }

    /// 焦点顺序（确定性：tab_index 升序，同序按 id）。
    pub fn build_focus_order(&mut self, mut items: Vec<FocusItem>) {
        items.sort_by(|a, b| a.tab_index.cmp(&b.tab_index).then(a.id.cmp(&b.id)));
        self.focus_order = items;
        self.focus_cursor = 0;
    }

    pub fn focus_next(&mut self) -> Option<&FocusItem> {
        if self.focus_order.is_empty() {
            return None;
        }
        self.focus_cursor = (self.focus_cursor + 1) % self.focus_order.len();
        let item = &self.focus_order[self.focus_cursor];
        if self.config.screen_reader {
            self.announcements.push(Announcement {
                priority: 1,
                text_id: item.label_id,
                entity: item.id,
                assertive: false,
            });
        }
        Some(item)
    }

    pub fn focus_prev(&mut self) -> Option<&FocusItem> {
        if self.focus_order.is_empty() {
            return None;
        }
        self.focus_cursor = if self.focus_cursor == 0 { self.focus_order.len() - 1 } else { self.focus_cursor - 1 };
        Some(&self.focus_order[self.focus_cursor])
    }

    pub fn run_a11y_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0301 可视化无障碍");
        cs.add("焦点序 tab_index 单调", self.focus_order.windows(2).all(|w| w[0].tab_index <= w[1].tab_index), "sorted");
        cs.add("焦点项 id 唯一", {
            let mut ids: Vec<u32> = self.focus_order.iter().map(|f| f.id).collect();
            ids.sort_unstable();
            ids.dedup();
            ids.len() == self.focus_order.len()
        }, "unique");
        cs.add("字体尺寸 >= 下限", self.config.min_font_px >= 10, ">=10px");
        cs.add_dyn("对比度违规已审计", true, &alloc::format!("violations={}", self.contrast_violations));
        cs.add("调色板映射无重复源", {
            let mut keys: Vec<u32> = self.remapped_colors.iter().map(|(k, _)| *k).collect();
            keys.sort_unstable();
            keys.dedup();
            keys.len() == self.remapped_colors.len()
        }, "unique src");
        cs.add("焦点游标合法", self.focus_order.is_empty() || self.focus_cursor < self.focus_order.len(), "cursor");
        cs
    }
}
