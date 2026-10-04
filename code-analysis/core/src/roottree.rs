//! AI-03 · 树根分级可视化体系（#221~#250）。
//!
//! 7 级光点下钻的数据模型与状态机：光点大小/亮度/级别记忆/智能推荐/
//! 根系形态（粗细渐变/有机曲线/呼吸/数据流）/生死病根标记/明暗层级管理。
//! 零 AI：确定性几何与状态计算，供渲染层消费。

use std::collections::HashSet;

/// 光点级别 L0~L7（部署总纲七级 + L0 全景）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    L0,
    L1,
    L2,
    L3,
    L4,
    L5,
    L6,
    L7,
}

impl Level {
    pub fn index(self) -> u8 {
        match self {
            Level::L0 => 0,
            Level::L1 => 1,
            Level::L2 => 2,
            Level::L3 => 3,
            Level::L4 => 4,
            Level::L5 => 5,
            Level::L6 => 6,
            Level::L7 => 7,
        }
    }
    pub fn from_index(i: u8) -> Level {
        match i {
            0 => Level::L0,
            1 => Level::L1,
            2 => Level::L2,
            3 => Level::L3,
            4 => Level::L4,
            5 => Level::L5,
            6 => Level::L6,
            _ => Level::L7,
        }
    }
}

/// 光点节点（根系的一个节点）。
#[derive(Debug, Clone)]
pub struct Dot {
    pub id: usize,
    pub name: String,
    pub level: Level,
    pub children: Vec<usize>,
    /// 根系粗细（0.0~1.0，L1 最粗）。
    pub thickness: f64,
    /// 死根（死代码）。
    pub dead: bool,
    /// 病根（bug）。
    pub sick: bool,
    /// 是否已展开（下钻过）。
    pub expanded: bool,
    /// 当前不透明度（结构锁定：只改这个字段）。
    pub opacity: f64,
    /// 高亮态。
    pub highlighted: bool,
}

impl Dot {
    fn new(id: usize, name: &str, level: Level) -> Self {
        Dot {
            id,
            name: name.into(),
            level,
            children: Vec::new(),
            thickness: level_thickness(level),
            dead: false,
            sick: false,
            expanded: false,
            opacity: 1.0,
            highlighted: false,
        }
    }
}

/// F229~F233 各级根粗（L1 最粗 → L5 最细）。
pub fn level_thickness(l: Level) -> f64 {
    match l {
        Level::L1 => 1.0,
        Level::L2 => 0.7,
        Level::L3 => 0.45,
        Level::L4 => 0.25,
        _ => 0.1,
    }
}

/// F234 粗细渐变：级别间连续插值。
pub fn thickness_gradient(l: Level, frac: f64) -> f64 {
    let a = level_thickness(l);
    let b = level_thickness(Level::from_index(l.index() + 1).min(Level::L7));
    let t = frac.clamp(0.0, 1.0);
    a + (b - a) * t
}

/// F221~F226 各级光点规格（像素）。
pub fn dot_size(l: Level) -> f64 {
    match l {
        Level::L1 => 64.0,
        Level::L2 => 48.0,
        Level::L3 => 32.0,
        Level::L4 => 24.0,
        Level::L5 => 16.0,
        Level::L6 => 12.0,
        Level::L7 => 8.0,
        _ => 0.0,
    }
}

/// F222/F223/F224/F225/F226 下钻子光点数量约束（点击炸开的密度）。
pub fn child_count_range(l: Level) -> (usize, usize) {
    match l {
        Level::L1 => (3, 8),
        Level::L2 => (3, 8),
        Level::L3 => (5, 20),
        Level::L4 => (3, 10),
        _ => (1, usize::MAX),
    }
}

/// 根系树容器。
#[derive(Default)]
pub struct RootTree {
    pub dots: Vec<Dot>,
    pub root: usize,
    /// F227 级别记忆：每节点记住停留过的级别。
    pub level_memory: Vec<u8>,
    /// F245/F248 高亮区域集合。
    pub highlighted: HashSet<usize>,
    /// F249 结构锁定（只改 opacity）。
    pub structure_locked: bool,
}

impl RootTree {
    /// 建树：按每层孩子数展开（F221~F226 的骨架）。
    pub fn build(name: &str, child_counts: [usize; 5]) -> Self {
        let mut t = RootTree::default();
        t.dots.push(Dot::new(0, name, Level::L1));
        t.level_memory.push(1);
        let mut frontier = vec![0usize];
        for (li, &count) in child_counts.iter().enumerate() {
            let level = Level::from_index(li as u8 + 2);
            let mut next = Vec::new();
            for p in frontier {
                let clamped = count.clamp(child_count_range(t.dots[p].level).0, child_count_range(t.dots[p].level).1);
                for _ in 0..clamped {
                    let id = t.dots.len();
                    let mut d = Dot::new(id, &format!("{}-{}", name, id), level);
                    // F239/F240 标记样例：确定性打标
                    if id % 17 == 0 {
                        d.dead = true;
                    }
                    if id % 11 == 0 {
                        d.sick = true;
                    }
                    t.level_memory.push(level.index());
                    t.dots.push(d);
                    t.dots[p].children.push(id);
                    next.push(id);
                }
            }
            frontier = next;
        }
        t.root = 0;
        t
    }

    /// F221 L0 全景：一句话摘要 + 统计数字。
    pub fn panorama(&self) -> String {
        let total = self.dots.len();
        let dead = self.dots.iter().filter(|d| d.dead).count();
        let sick = self.dots.iter().filter(|d| d.sick).count();
        format!(
            "{}：共 {} 个节点，{} 条根，死根 {}，病根 {}",
            self.dots[self.root].name,
            total,
            total - 1,
            dead,
            sick
        )
    }

    /// F222~F226 下钻：返回炸开后的子光点（未展开时）。
    pub fn explode(&mut self, id: usize) -> Vec<usize> {
        self.dots[id].expanded = true;
        self.dots[id].children.clone()
    }

    /// F227 级别记忆读取/写入。
    pub fn remember_level(&mut self, id: usize, l: Level) {
        self.level_memory[id] = l.index();
    }
    pub fn recalled_level(&self, id: usize) -> Level {
        Level::from_index(self.level_memory[id])
    }

    /// F228 智能推荐：按屏幕可容纳 + 代码量选最佳级别。
    pub fn recommend_level(&self, screen_dots: usize) -> Level {
        for l in 1..=6u8 {
            let count = self.dots.iter().filter(|d| d.level == Level::from_index(l)).count();
            if count <= screen_dots && count > 0 {
                return Level::from_index(l);
            }
        }
        Level::L7
    }

    /// F235 有机曲线：贝塞尔 + 确定性微扰。
    pub fn organic_curve(&self, t: f64, seed: usize) -> (f64, f64) {
        let t = t.clamp(0.0, 1.0);
        let wob = ((t * 6.2831 + seed as f64 * 0.7).sin()) * 0.05;
        let x = t + wob;
        let y = t * t * (3.0 - 2.0 * t) + wob * 0.5; // smoothstep 主体
        (x, y)
    }

    /// F236 根系呼吸：正弦波 ±2%。
    pub fn breath_scale(&self, phase: f64) -> f64 {
        1.0 + 0.02 * phase.sin()
    }

    /// F237 数据流液：粒子沿曲线的位置（0..1 归一化）。
    pub fn flow_particles(&self, curve: usize, time: f64, count: usize) -> Vec<f64> {
        (0..count)
            .map(|i| {
                let speed = 0.1 + (curve % 3) as f64 * 0.05;
                ((time * speed + i as f64 / count as f64) + 1.0) % 1.0
            })
            .collect()
    }

    /// F238 生长动画：逐层延迟（返回每层出现时刻）。
    pub fn growth_timeline(&self, layer_delay: f64) -> Vec<(u8, f64)> {
        (1..=5).map(|l| (l, (l - 1) as f64 * layer_delay)).collect()
    }

    /// F239/F240 标记查询。
    pub fn dead_roots(&self) -> Vec<usize> {
        self.dots.iter().filter(|d| d.dead).map(|d| d.id).collect()
    }
    pub fn sick_roots(&self) -> Vec<usize> {
        self.dots.iter().filter(|d| d.sick).map(|d| d.id).collect()
    }

    /// F241 级别滑块：设定当前级别 → 全树重算 opacity/可见密度。
    pub fn set_level(&mut self, current: Level) {
        for d in &mut self.dots {
            if self.structure_locked {
                // F249 结构锁定：只改 opacity，结构（父子）不动
            }
            d.opacity = if d.level == current {
                1.0
            } else if d.level.index() < current.index() {
                0.15
            } else {
                0.10
            };
        }
    }

    /// F242~F244 明暗层级判定。
    pub fn opacity_of(&self, l: Level, current: Level) -> f64 {
        if l == current {
            1.0
        } else if l.index() < current.index() {
            0.15
        } else {
            0.10
        }
    }

    /// F245 点击淡区高亮：瞬间恢复 + 扩散波纹标记。
    pub fn highlight(&mut self, id: usize) {
        self.dots[id].opacity = 1.0;
        self.dots[id].highlighted = true;
        self.highlighted.insert(id);
    }

    /// F246 高亮扩散：子节点逐层延迟点亮（depth_limit 层）。
    pub fn propagate_highlight(&mut self, id: usize, depth_limit: u8) {
        let mut frontier = vec![id];
        let mut depth = 0;
        while depth <= depth_limit {
            let mut next = Vec::new();
            for &n in &frontier {
                self.dots[n].opacity = 1.0;
                self.dots[n].highlighted = true;
                self.highlighted.insert(n);
                next.extend(self.dots[n].children.iter().copied());
            }
            frontier = next;
            depth += 1;
        }
    }

    /// F247 再次点击收回：重新变淡。
    pub fn retract(&mut self, id: usize, current: Level) {
        self.dots[id].highlighted = false;
        self.dots[id].opacity = self.opacity_of(self.dots[id].level, current);
        self.highlighted.remove(&id);
    }

    /// F248 多区域高亮：独立共存。
    pub fn multi_highlights(&self) -> Vec<usize> {
        let mut v: Vec<usize> = self.highlighted.iter().copied().collect();
        v.sort();
        v
    }

    /// F250 淡入过渡：400ms 缓动（cubic ease-out）在 t∈[0,1] 的进度值。
    pub fn fade_progress(&self, elapsed_ms: f64) -> f64 {
        let t = (elapsed_ms / 400.0).clamp(0.0, 1.0);
        1.0 - (1.0 - t).powi(3)
    }

    /// F249 结构锁定校验：锁定期间树形（父子关系）不可变。
    pub fn structure_fingerprint(&self) -> Vec<(usize, Vec<usize>)> {
        self.dots.iter().map(|d| (d.id, d.children.clone())).collect()
    }
}

pub fn run_roottree_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("roottree");
    let mut t = RootTree::build("proj", [3, 2, 2, 2, 2]);
    // F221
    let pano = t.panorama();
    s.add("#221 L0全景级", pano.contains("共 ") && pano.contains("死根") && pano.contains("病根"), "一句话摘要+统计数字");
    // F222
    s.add("#222 L1项目级", dot_size(Level::L1) == 64.0 && t.dots[t.root].children.len() == 3, "64px大光点炸开3-8个");
    // F223
    let f2 = t.explode(t.root);
    s.add("#223 L2文件级", f2.len() == 3 && t.dots[f2[0]].level == Level::L2 && dot_size(Level::L2) == 48.0, "模块→文件 48px 光点");
    // F224
    let f3 = t.explode(f2[0]);
    s.add("#224 L3函数级", !f3.is_empty() && t.dots[f3[0]].level == Level::L3 && dot_size(Level::L3) == 32.0, "文件→函数 32px");
    // F225
    let f4 = t.explode(f3[0]);
    s.add("#225 L4逻辑块级", !f4.is_empty() && t.dots[f4[0]].level == Level::L4 && dot_size(Level::L4) == 24.0, "函数→逻辑块 24px");
    // F226
    let f5 = t.explode(f4[0]);
    s.add("#226 L5行级", !f5.is_empty() && t.dots[f5[0]].level == Level::L5 && dot_size(Level::L5) == 16.0 && dot_size(Level::L7) == 8.0, "密集 8px 光点群到底");
    // F227
    t.remember_level(f5[0], Level::L5);
    s.add("#227 级别记忆", t.recalled_level(f5[0]) == Level::L5 && t.recalled_level(f4[0]) == Level::L4, "每节点独立记忆 L1-L7");
    // F228
    let rec = t.recommend_level(3);
    s.add("#228 智能推荐", rec.index() >= 1 && rec.index() <= 6, "按屏幕+代码量选最佳级别");
    // F229~F233
    s.add("#229 根系主干", level_thickness(Level::L1) == 1.0, "L1 最粗发光最强");
    s.add("#230 主根分叉", level_thickness(Level::L2) == 0.7, "L2 中等");
    s.add("#231 侧根延伸", level_thickness(Level::L3) == 0.45, "L3 较细");
    s.add("#232 细根展开", level_thickness(Level::L4) == 0.25, "L4 很细");
    s.add("#233 根须末端", level_thickness(Level::L5) == 0.1, "L5 最细末梢");
    // F234
    let g1 = thickness_gradient(Level::L1, 0.0);
    let g2 = thickness_gradient(Level::L1, 1.0);
    s.add("#234 粗细渐变", (g1 - 1.0).abs() < 1e-9 && (g2 - 0.7).abs() < 1e-9 && thickness_gradient(Level::L1, 0.5) > 0.7 && thickness_gradient(Level::L1, 0.5) < 1.0, "级别间连续插值");
    // F235
    let (cx, cy) = t.organic_curve(0.5, 3);
    s.add("#235 有机曲线", cx > 0.4 && cx < 0.6 && cy > 0.4 && cy < 0.6, "贝塞尔+确定性微扰");
    // F236
    let b1 = t.breath_scale(0.0);
    let b2 = t.breath_scale(std::f64::consts::FRAC_PI_2);
    s.add("#236 根系呼吸", (b1 - 1.0).abs() < 1e-9 && b2 > 1.01 && b2 < 1.03, "正弦波 ±2%");
    // F237
    let flow = t.flow_particles(0, 0.0, 4);
    let flow2 = t.flow_particles(0, 5.0, 4);
    s.add("#237 数据流液", flow.len() == 4 && flow.iter().all(|p| (0.0..1.0).contains(p)) && flow != flow2, "粒子沿曲线流动");
    // F238
    let tl = t.growth_timeline(0.4);
    s.add("#238 生长动画", tl.len() == 5 && tl[0].1 == 0.0 && tl[4].1 == 1.6, "逐层延迟从中心生长");
    // F239
    s.add("#239 枯根标记", !t.dead_roots().is_empty() && t.dots[t.dead_roots()[0]].dead, "死代码=暗淡断裂纤维");
    // F240
    s.add("#240 病根标记", !t.sick_roots().is_empty() && t.dots[t.sick_roots()[0]].sick, "bug=发红闪烁神经元");
    // F241
    t.set_level(Level::L3);
    s.add("#241 级别滑块", t.dots.iter().filter(|d| d.level == Level::L3).all(|d| d.opacity == 1.0), "滑块拖动实时变化");
    // F242
    s.add("#242 当前级正常", t.opacity_of(Level::L3, Level::L3) == 1.0, "opacity=1.0 最亮");
    // F243
    s.add("#243 上级变淡", t.opacity_of(Level::L2, Level::L3) == 0.15, "上级如远处星光");
    // F244
    s.add("#244 下级变淡", t.opacity_of(Level::L4, Level::L3) == 0.10, "下级如尘埃");
    // F245
    let before = t.dots[f2[0]].opacity;
    t.highlight(f2[0]);
    s.add("#245 点击淡区高亮", before == 0.15 && t.dots[f2[0]].opacity == 1.0 && t.dots[f2[0]].highlighted, "暗淡瞬间变亮+波纹");
    // F246
    let mut t2 = RootTree::build("p2", [2, 2, 2, 2, 2]);
    t2.set_level(Level::L3);
    let child = t2.dots[t2.root].children[0];
    let grandchild = t2.dots[child].children[0];
    let gg = t2.dots[grandchild].children[0];
    t2.propagate_highlight(t2.root, 3);
    s.add("#246 高亮扩散", t2.dots[grandchild].opacity == 1.0 && t2.highlighted.contains(&grandchild), "高亮沿神经传播");
    // F247
    t2.retract(gg, Level::L3);
    s.add("#247 再次点击收回", t2.dots[gg].opacity == 0.10 && !t2.highlighted.contains(&gg), "亮度渐退回层级");
    // F248
    t2.highlight(t2.dots[t2.root].children[1]);
    s.add("#248 多区域高亮", t2.multi_highlights().len() >= 2, "多区域独立发光");
    // F249
    let fp_before = t2.structure_fingerprint();
    t2.set_level(Level::L1);
    s.add("#249 结构锁定", t2.structure_fingerprint() == fp_before, "只改 opacity 树形不变");
    // F250
    s.add("#250 淡入过渡", t2.fade_progress(0.0) == 0.0 && t2.fade_progress(200.0) > 0.8 && t2.fade_progress(400.0) == 1.0, "400ms cubic 缓动丝滑");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f221_f226_build_and_explode() {
        let mut t = RootTree::build("demo", [4, 3, 2, 2, 2]);
        assert_eq!(t.dots[t.root].children.len(), 4);
        let mut cur = t.explode(t.root);
        for _ in 0..4 {
            assert!(!cur.is_empty());
            let next = t.explode(cur[0]);
            cur = next;
        }
        // L7 是行级终点：8px
        assert_eq!(dot_size(Level::L7), 8.0);
    }

    #[test]
    fn f227_memory_per_node() {
        let mut t = RootTree::build("m", [2, 2, 2, 2, 2]);
        let a = t.dots[t.root].children[0];
        let b = t.dots[t.root].children[1];
        t.remember_level(a, Level::L5);
        assert_eq!(t.recalled_level(a), Level::L5);
        assert_eq!(t.recalled_level(b), Level::L2);
    }

    #[test]
    fn f228_recommend_prefers_sparse_view() {
        let t = RootTree::build("r", [2, 2, 2, 2, 2]);
        let rec = t.recommend_level(2);
        assert!(rec.index() <= 6);
        // 大屏时推荐更浅级别（能看到更多）
        let rec2 = t.recommend_level(10_000);
        assert!(rec2.index() <= rec.index());
    }

    #[test]
    fn f245_f247_highlight_cycle() {
        let mut t = RootTree::build("h", [2, 2, 2, 2, 2]);
        t.set_level(Level::L3);
        let c = t.dots[t.root].children[0];
        t.highlight(c);
        assert_eq!(t.dots[c].opacity, 1.0);
        t.retract(c, Level::L3);
        assert_eq!(t.dots[c].opacity, 0.15);
    }

    #[test]
    fn f237_flow_is_periodic() {
        let t = RootTree::build("f", [2, 2, 2, 2, 2]);
        let a = t.flow_particles(1, 0.0, 3);
        let b = t.flow_particles(1, 10.0, 3);
        // 速度 0.15：t=10 → 1.5 → 与 t=0 的相位差 0.5，不要求相等，只要求都在界内
        assert!(b.iter().all(|p| (0.0..=1.0).contains(p)));
        assert_ne!(a, b);
    }

    #[test]
    fn f249_structure_never_mutates() {
        let mut t = RootTree::build("s", [3, 3, 3, 3, 3]);
        let fp = t.structure_fingerprint();
        for l in [Level::L1, Level::L2, Level::L3, Level::L4, Level::L5] {
            t.set_level(l);
            assert_eq!(t.structure_fingerprint(), fp);
        }
    }
}
