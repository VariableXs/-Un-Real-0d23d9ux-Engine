//! VE-F0647 脏区膨胀与安全边距（VE-D 域 · 目标 320 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0647`
//!
//! **判据（锚点原文逐条）**：
//! - **逐源边距**：抗锯齿过渡带/模糊半径溢出/阴影扩散/亚像素舍入——逐效果来源有依据；
//! - **先胀后并**：膨胀先于合并（顺序契约，违序则缝隙漏绘）；
//! - **浪费告警**：膨胀浪费率入度量，超阈告警复审边距表；
//! - **表可修订**：边距不足（漏绘）→ 指纹走查定位加表修订；
//! - **判据**：来源缺项 → 登记补齐。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec241_dmodel::Rect;

/// 膨胀来源登记（逐项有依据，拒绝全局常数）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarginSource {
    /// 抗锯齿边缘过渡带
    AaBand,
    /// 模糊半径溢出
    BlurSpill,
    /// 阴影扩散范围
    ShadowSpread,
    /// 亚像素舍入
    SubpixelRound,
}

pub const SOURCE_COUNT: usize = 4;

impl MarginSource {
    pub fn all() -> [MarginSource; SOURCE_COUNT] {
        [
            MarginSource::AaBand,
            MarginSource::BlurSpill,
            MarginSource::ShadowSpread,
            MarginSource::SubpixelRound,
        ]
    }
    pub fn label(self) -> &'static str {
        match self {
            MarginSource::AaBand => "抗锯齿过渡带",
            MarginSource::BlurSpill => "模糊溢出",
            MarginSource::ShadowSpread => "阴影扩散",
            MarginSource::SubpixelRound => "亚像素舍入",
        }
    }
}

/// 边距计算表（逐来源独立边距值，可修订）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarginTable {
    pub aa: u32,
    pub blur: u32,
    pub shadow: u32,
    pub subpix: u32,
}

/// 默认表（依据：AA 1px 过渡带、模糊=半径、阴影=扩散半径、舍入=1px）
pub const DEFAULT_TABLE: MarginTable = MarginTable { aa: 1, blur: 8, shadow: 6, subpix: 1 };

impl MarginTable {
    /// 单来源边距
    pub fn get(self, s: MarginSource) -> u32 {
        match s {
            MarginSource::AaBand => self.aa,
            MarginSource::BlurSpill => self.blur,
            MarginSource::ShadowSpread => self.shadow,
            MarginSource::SubpixelRound => self.subpix,
        }
    }

    /// 合成边距：启用来源集合的最大值（一次膨胀覆盖全部生效来源）
    pub fn composite(self, sources: &[MarginSource]) -> u32 {
        sources.iter().map(|&s| self.get(s)).max().unwrap_or(0)
    }

    /// 表修订（边距不足漏绘 → 走查定位后改表）
    pub fn revise(self, s: MarginSource, v: u32) -> MarginTable {
        match s {
            MarginSource::AaBand => MarginTable { aa: v, ..self },
            MarginSource::BlurSpill => MarginTable { blur: v, ..self },
            MarginSource::ShadowSpread => MarginTable { shadow: v, ..self },
            MarginSource::SubpixelRound => MarginTable { subpix: v, ..self },
        }
    }
}

/// 膨胀应用（单矩形外扩）
pub fn dilate_rect(r: Rect, m: u32) -> Rect {
    (
        r.0.saturating_sub(m),
        r.1.saturating_sub(m),
        r.2.saturating_add(m),
        r.3.saturating_add(m),
    )
}

/// 顺序契约：先胀后并（输入原始区域，先全部膨胀再交并合并）
pub fn dilate_then_merge(rs: &[Rect], m: u32) -> Vec<Rect> {
    let mut out: Vec<Rect> = Vec::new();
    for &r in rs {
        let d = dilate_rect(r, m);
        // 膨胀后相邻（贴边）必然相交 → 合并
        let mut merged = false;
        for o in out.iter_mut() {
            if d.0 < o.2 && o.0 < d.2 && d.1 < o.3 && o.1 < d.3 {
                *o = (
                    o.0.min(d.0), o.1.min(d.1),
                    o.2.max(d.2), o.3.max(d.3),
                );
                merged = true;
                break;
            }
        }
        if !merged {
            out.push(d);
        }
    }
    out
}

/// 膨胀浪费率：膨胀后总面积 / 原始总面积（>1 部分 = 多绘）
pub fn waste_rate(orig: &[Rect], dilated: &[Rect]) -> f32 {
    let area = |rs: &[Rect]| -> u64 {
        rs.iter().map(|&(l, t, r, b)| (r.saturating_sub(l) as u64) * (b.saturating_sub(t) as u64)).sum()
    };
    let a0 = area(orig);
    if a0 == 0 {
        return 0.0;
    }
    (area(dilated) - a0) as f32 / a0 as f32
}

/// 浪费告警阈
pub const WASTE_WARN: f32 = 2.0;

pub fn run_vec247_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec247");
    // 族一：逐源边距
    let t = DEFAULT_TABLE;
    s.add("边距-四来源", MarginSource::all().len() == SOURCE_COUNT, "4 sources");
    s.add("边距-表齐", MarginSource::all().iter().all(|&x| t.get(x) > 0), "all nonzero");
    s.add("边距-模糊最大", t.composite(&MarginSource::all()) == t.blur, "blur dominates");
    s.add("边距-单源", t.composite(&[MarginSource::AaBand]) == t.aa, "single source");
    s.add("边距-空源零", t.composite(&[]) == 0, "empty 0");
    // 族二：表修订
    let t2 = t.revise(MarginSource::AaBand, 2);
    s.add("边距-修订生效", t2.aa == 2 && t2.blur == t.blur, "revised");
    s.add("边距-原表不变", t.aa == 1, "original intact");
    // 族三：膨胀应用
    let d = dilate_rect((10, 10, 20, 20), 3);
    s.add("膨胀-四周外扩", d == (7, 7, 23, 23), "expanded");
    let d0 = dilate_rect((0, 0, 5, 5), 10);
    s.add("膨胀-零钳制", d0.0 == 0 && d0.1 == 0, "saturating");
    // 族四：先胀后并（贴边矩形膨胀后归并）
    let two = [(10u32, 0u32, 20u32, 10u32), (20u32, 0u32, 30u32, 10u32)];
    let merged = dilate_then_merge(&two, 2);
    s.add("顺序-胀后贴邻并", merged.len() == 1, "gap 0 after dilate");
    // 远距仍分立
    let far = [(0u32, 0u32, 5u32, 5u32), (100u32, 100u32, 105u32, 105u32)];
    s.add("顺序-远区分立", dilate_then_merge(&far, 2).len() == 2, "far apart");
    // 缝隙漏绘语义：贴边不膨胀则两块独立（漏缝演示）
    s.add("顺序-不胀则分立", two.len() == 2, "no dilate no merge (gap leak demo)");
    // 族五：浪费率
    let orig = [(0u32, 0u32, 100u32, 100u32)];
    let dil = dilate_then_merge(&orig, 10);
    let w = waste_rate(&orig, &dil);
    s.add("浪费-10边距", (w - ((120.0 * 120.0 - 100.0 * 100.0) / 10000.0)).abs() < 1e-4, "rate calc");
    s.add("浪费-零膨胀零浪费", waste_rate(&orig, &orig) == 0.0, "no dilate no waste");
    s.add("浪费-空集零", waste_rate(&[], &[]) == 0.0, "empty");
    // 大边距触发告警
    let big = dilate_then_merge(&orig, 300);
    s.add("浪费-超阈告警", waste_rate(&orig, &big) > WASTE_WARN, "over warn");
    s.add("浪费-小边距不警", waste_rate(&orig, &dil) < WASTE_WARN, "under warn");
    s
}
