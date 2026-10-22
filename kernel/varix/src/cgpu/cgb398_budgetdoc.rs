//! CGPU-F0398 · 预算文档总纲（目标 320 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 文档总纲：C01-C05 文档索引 + 预算域架构总图（模型/仲裁/场景/预测/
//!   治理五层）；术语表；API 总手册；调参手册（池比例/权重/阈值调优指南）。
//! - 判据：索引总图、术语、总手册、调参手册、判据。
//!
//! 设计纪律：
//! - 文档索引是机器可查的目录账：每组 → 文档条目（锚点号/主题/行数档），
//!   索引完整性 = C01-C05 每组至少一条且锚点号单调不重复。
//! - 架构总图五层依赖闭合：模型→仲裁→场景→预测→治理，跨层引用只允许
//!   下行依赖（治理可引预测，预测不可引治理）——层次颠倒即红。
//! - 术语表：术语唯一、定义非空、术语互引不悬空。
//! - 调参手册：每参数带合法区间/步长/影响层；区间校验器供运行期钳制。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0C10 独占。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0C10，全仓独占）。
pub const SEG_BASE: u16 = 0x0C10;

pub const E_IDX_INCOMPLETE: u16 = SEG_BASE + 1; // 索引组缺口
pub const E_IDX_DUP: u16 = SEG_BASE + 2; // 锚点号重复
pub const E_LAYER_BACKREF: u16 = SEG_BASE + 3; // 总图层次颠倒
pub const E_TERM_DUP: u16 = SEG_BASE + 4; // 术语重名
pub const E_TERM_DANGLE: u16 = SEG_BASE + 5; // 术语互引悬空
pub const E_PARAM_RANGE: u16 = SEG_BASE + 6; // 调参区间非法（lo>hi 或步长不整除）

/// 五层（模型/仲裁/场景/预测/治理）——索引与总图的统一坐标。
pub const LAYER_MODEL: u8 = 0;
pub const LAYER_ARBITER: u8 = 1;
pub const LAYER_SCENE: u8 = 2;
pub const LAYER_PREDICT: u8 = 3;
pub const LAYER_GOVERN: u8 = 4;
pub const LAYER_COUNT: usize = 5;

/// 层名（总图渲染用）。
pub fn layer_name(l: u8) -> &'static str {
    match l {
        LAYER_MODEL => "模型",
        LAYER_ARBITER => "仲裁",
        LAYER_SCENE => "场景",
        LAYER_PREDICT => "预测",
        _ => "治理",
    }
}

/// 跨层依赖合法：只允许下行（大编号层可依赖小编号层）。
pub fn layer_dep_legal(from: u8, to: u8) -> bool {
    from < LAYER_COUNT as u8 && to < LAYER_COUNT as u8 && from > to
}

/// 一条文档索引条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocEntry {
    /// 所属组（C01..C05）。
    pub group: u8,
    /// 锚点号（F0xxx 后三位即可，组内单调）。
    pub anchor: u32,
    pub title: String,
    /// 主要落在哪一层。
    pub layer: u8,
}

/// 文档索引账。
#[derive(Clone, Debug, Default)]
pub struct DocIndex {
    pub entries: Vec<DocEntry>,
}

impl DocIndex {
    pub fn add(&mut self, group: u8, anchor: u32, title: &str, layer: u8) {
        self.entries.push(DocEntry {
            group,
            anchor,
            title: title.to_string(),
            layer,
        });
    }

    /// 索引完整性：C01-C05 每组至少一条；锚点号全域不重复。
    pub fn validate(&self) -> Result<(), u16> {
        for g in 1..=5u8 {
            if !self.entries.iter().any(|e| e.group == g) {
                return Err(E_IDX_INCOMPLETE);
            }
        }
        for i in 0..self.entries.len() {
            for j in (i + 1)..self.entries.len() {
                if self.entries[i].anchor == self.entries[j].anchor {
                    return Err(E_IDX_DUP);
                }
            }
        }
        // 层坐标必须落在五层内。
        for e in &self.entries {
            if e.layer >= LAYER_COUNT as u8 {
                return Err(E_LAYER_BACKREF);
            }
        }
        Ok(())
    }

    /// 总图：按层分桶的条目计数（渲染输入）。
    pub fn layer_buckets(&self) -> [usize; LAYER_COUNT] {
        let mut b = [0usize; LAYER_COUNT];
        for e in &self.entries {
            b[e.layer as usize] += 1;
        }
        b
    }
}

/// 术语表条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Term {
    pub name: String,
    pub definition: String,
    /// 定义中引用的其他术语（互引边）。
    pub refs: Vec<String>,
}

/// 术语表。
#[derive(Clone, Debug, Default)]
pub struct Glossary {
    pub terms: Vec<Term>,
}

impl Glossary {
    pub fn define(&mut self, name: &str, definition: &str, refs: &[&str]) -> Result<(), u16> {
        if self.terms.iter().any(|t| t.name == name) {
            return Err(E_TERM_DUP);
        }
        self.terms.push(Term {
            name: name.to_string(),
            definition: definition.to_string(),
            refs: refs.iter().map(|s| s.to_string()).collect(),
        });
        Ok(())
    }

    /// 术语完整性：定义非空 + 互引不悬空（引用的术语必须在册）。
    pub fn validate(&self) -> Result<(), u16> {
        for t in &self.terms {
            if t.definition.is_empty() {
                return Err(E_TERM_DANGLE);
            }
            for r in &t.refs {
                if !self.terms.iter().any(|o| &o.name == r) {
                    return Err(E_TERM_DANGLE);
                }
            }
        }
        Ok(())
    }

    pub fn lookup(&self, name: &str) -> Option<&Term> {
        self.terms.iter().find(|t| t.name == name)
    }
}

/// 调参手册条目：池比例/权重/阈值三类参数的合法区间与影响层。
#[derive(Clone, Debug)]
pub struct Tunable {
    pub name: String,
    /// 影响层（五层坐标）。
    pub layer: u8,
    /// 合法闭区间（千分位）。
    pub lo: u32,
    pub hi: u32,
    /// 步长（值必须落在 lo + k*step 网格上）。
    pub step: u32,
    /// 调优指南要点（手册文本行）。
    pub guide: String,
}

impl Tunable {
    pub fn new(name: &str, layer: u8, lo: u32, hi: u32, step: u32, guide: &str) -> Self {
        Tunable {
            name: name.to_string(),
            layer,
            lo,
            hi,
            step,
            guide: guide.to_string(),
        }
    }

    /// 区间自检：lo<=hi、步长>0、区间宽度可被步长整除。
    pub fn range_valid(&self) -> Result<(), u16> {
        if self.lo > self.hi || self.step == 0 {
            return Err(E_PARAM_RANGE);
        }
        if (self.hi - self.lo) % self.step != 0 {
            return Err(E_PARAM_RANGE);
        }
        if self.layer >= LAYER_COUNT as u8 {
            return Err(E_LAYER_BACKREF);
        }
        Ok(())
    }

    /// 值合法性：落在区间且对齐网格。
    pub fn accepts(&self, v: u32) -> bool {
        v >= self.lo && v <= self.hi && (v - self.lo) % self.step == 0
    }

    /// 运行期钳制：越界值吸附到最近网格点（不报错——调参容错路径）。
    pub fn clamp(&self, v: u32) -> u32 {
        let c = v.clamp(self.lo, self.hi);
        let over = (c - self.lo) % self.step;
        let down = c - over;
        let up = (down + self.step).min(self.hi);
        if c - down <= up - c {
            down
        } else {
            up
        }
    }
}

/// API 总手册：指向冻结总册的手册级摘要（组 → 手册章节映射）。
#[derive(Clone, Debug)]
pub struct ApiHandbook {
    /// 章节名（六族各一章 + 总纲一章）。
    pub chapters: Vec<String>,
    /// 章节对应的冻结家族号（0..6；总纲章用 255）。
    pub families: Vec<u8>,
}

impl ApiHandbook {
    /// 七章标准手册（六族 + 总纲）。
    pub fn standard() -> Self {
        ApiHandbook {
            chapters: [
                "总纲",
                "账户族",
                "申请族",
                "仲裁族",
                "交易族",
                "配方族",
                "预测族",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            families: vec![255, 0, 1, 2, 3, 4, 5],
        }
    }

    /// 手册完整性：七章齐、族号覆盖 0-5 各一次、章名非空。
    pub fn validate(&self) -> Result<(), u16> {
        if self.chapters.len() != 7 || self.families.len() != 7 {
            return Err(E_IDX_INCOMPLETE);
        }
        for c in &self.chapters {
            if c.is_empty() {
                return Err(E_IDX_INCOMPLETE);
            }
        }
        let mut seen = [false; 6];
        for f in &self.families {
            if *f == 255 {
                continue;
            }
            if (*f as usize) < 6 {
                if seen[*f as usize] {
                    return Err(E_IDX_DUP);
                }
                seen[*f as usize] = true;
            }
        }
        if !seen.iter().all(|s| *s) {
            return Err(E_IDX_INCOMPLETE);
        }
        Ok(())
    }
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a01_索引总图_C01到C05齐全且锚点不重() {
        let mut idx = DocIndex::default();
        idx.add(1, 3257, "C01 模型层文档", LAYER_MODEL);
        idx.add(2, 3320, "C02 仲裁层文档", LAYER_ARBITER);
        idx.add(3, 3357, "C03 场景层文档", LAYER_SCENE);
        idx.add(4, 3377, "C04 预测层文档", LAYER_PREDICT);
        idx.add(5, 3385, "C05 治理层文档", LAYER_GOVERN);
        assert_eq!(idx.validate(), Ok(()));
        assert_eq!(idx.layer_buckets(), [1, 1, 1, 1, 1]);
        // 缺组红。
        let mut idx2 = idx.clone();
        idx2.entries.retain(|e| e.group != 3);
        assert_eq!(idx2.validate(), Err(E_IDX_INCOMPLETE));
        // 锚点重复红。
        let mut idx3 = idx.clone();
        idx3.add(5, 3257, "重复锚点", LAYER_GOVERN);
        assert_eq!(idx3.validate(), Err(E_IDX_DUP));
    }

    #[test]
    fn a02_总图层次_跨层引用只下行() {
        // 治理(4)→预测(3) 合法；预测(3)→治理(4) 颠倒。
        assert!(layer_dep_legal(LAYER_GOVERN, LAYER_PREDICT));
        assert!(layer_dep_legal(LAYER_SCENE, LAYER_MODEL));
        assert!(!layer_dep_legal(LAYER_PREDICT, LAYER_GOVERN));
        assert!(!layer_dep_legal(LAYER_MODEL, LAYER_ARBITER));
        // 自依赖不允许。
        assert!(!layer_dep_legal(LAYER_ARBITER, LAYER_ARBITER));
        // 越界层坐标拒绝。
        let mut idx = DocIndex::default();
        for g in 1..=5u8 {
            idx.add(g, 3000 + g as u32, "t", LAYER_MODEL);
        }
        idx.entries[0].layer = 9;
        assert_eq!(idx.validate(), Err(E_LAYER_BACKREF));
        // 层名表完整。
        assert_eq!(layer_name(0), "模型");
        assert_eq!(layer_name(4), "治理");
    }

    #[test]
    fn a03_术语表_唯一定义非空互引不悬空() {
        let mut g = Glossary::default();
        g.define("预算", "周期内资源使用的上限约束", &[]).ok();
        g.define("配方", "场景化的预算权重组合", &["预算"]).ok();
        assert_eq!(g.validate(), Ok(()));
        // 重名。
        assert_eq!(g.define("预算", "重复定义", &[]), Err(E_TERM_DUP));
        // 悬空互引。
        let mut g2 = g.clone();
        g2.define("串号", "引用不存在的术语", &["不存在术语"]).ok();
        assert_eq!(g2.validate(), Err(E_TERM_DANGLE));
        // 查命中。
        assert!(g.lookup("配方").is_some());
        assert!(g.lookup("无").is_none());
    }

    #[test]
    fn a04_调参手册_区间网格与钳制() {
        let t = Tunable::new("池比例.预留池", LAYER_ARBITER, 50, 150, 10, "预留池占比调优");
        assert_eq!(t.range_valid(), Ok(()));
        // 合法网格值。
        assert!(t.accepts(50) && t.accepts(120) && t.accepts(150));
        assert!(!t.accepts(55) && !t.accepts(49) && !t.accepts(151));
        // 钳制吸附最近网格点。
        assert_eq!(t.clamp(53), 50);
        assert_eq!(t.clamp(55), 50);
        assert_eq!(t.clamp(56), 60);
        assert_eq!(t.clamp(0), 50); // 下界吸附
        assert_eq!(t.clamp(9999), 150); // 上界吸附
        // 非法区间。
        let bad = Tunable::new("x", 0, 100, 50, 10, "");
        assert_eq!(bad.range_valid(), Err(E_PARAM_RANGE));
        let bad2 = Tunable::new("y", 0, 0, 100, 0, "");
        assert_eq!(bad2.range_valid(), Err(E_PARAM_RANGE));
        let bad3 = Tunable::new("z", 0, 0, 95, 10, "");
        assert_eq!(bad3.range_valid(), Err(E_PARAM_RANGE));
    }

    #[test]
    fn a05_总手册_七章六族覆盖() {
        let h = ApiHandbook::standard();
        assert_eq!(h.validate(), Ok(()));
        // 六族各覆盖一次。
        let mut f = h.families.clone();
        f.sort();
        assert_eq!(f, vec![0, 1, 2, 3, 4, 5, 255]);
        // 缺章红。
        let mut h2 = ApiHandbook::standard();
        h2.chapters.pop();
        assert_eq!(h2.validate(), Err(E_IDX_INCOMPLETE));
        // 族重复红。
        let mut h3 = ApiHandbook::standard();
        h3.families[1] = 2;
        assert_eq!(h3.validate(), Err(E_IDX_DUP));
    }
}
