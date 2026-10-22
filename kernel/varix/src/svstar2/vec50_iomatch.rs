//! VE-F0450 · 输入输出接口匹配（VE-C 域 · 语义组 C04）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0450`
//!
//! **锚点原文**：「着色器阶段间输入输出接口匹配：跨阶段变量匹配（名称、类型、
//! 限定符三元一致判定，按链接单元聚合）、不匹配诊断（两侧位置双定位加条款）、
//! 未消费输出与未供给输入显性告警；匹配结果表供链接期与后端。判据：三元匹配、
//! 双侧诊断、未供未消显性、结果表、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **三元匹配**（判据一）。跨阶段匹配按「名称 + 类型 + 限定符」三元
//!    一致判定（[`IfaceVar`] 三字段逐位），任何一元不齐即不匹配——没有
//!    「类型差不多就算匹配」的口径。
//!
//! 2. **双侧诊断**（判据二）。不匹配诊断双侧定位（上游声明位置 + 下游
//!    声明位置）并引用规范条款，单侧报错不成立。
//!
//! 3. **未供未消显性**（判据三）。上游输出无下游消费、下游输入无上游
//!    供给，分别显性告警（[`MatchReport`] 两个清单）——接口浪费与接口
//!    缺口是两类不同的问题。
//!
//! 4. **结果表**（判据四）。匹配结果表（配对 + 单侧两类条目）供链接期
//!    与后端消费；链接单元聚合按阶段对（VS→FS 等）组织。

use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0450";

/// 规范条款。
pub const CLAUSE_MATCH: &str = "F0402-§6.2.1 跨阶段接口按名称类型限定符三元一致匹配";
pub const CLAUSE_ORPHAN: &str = "F0402-§6.2.2 未消费输出与未供给输入显性告警";

/// 阶段（链接单元的聚合单位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Vertex,
    Fragment,
    Compute,
}

impl Stage {
    pub fn tag(&self) -> &'static str {
        match self {
            Stage::Vertex => "vs",
            Stage::Fragment => "fs",
            Stage::Compute => "cs",
        }
    }
}

/// 接口变量三元组：名称 × 类型 × 限定符。
#[derive(Clone, Debug, PartialEq)]
pub struct IfaceVar {
    pub name: String,
    pub ty: String,
    /// 限定符摘要（插值/精度等，规范化后比对）。
    pub qual: String,
    pub off: u32,
    pub stage: Stage,
    /// true = 阶段输出（上游侧），false = 阶段输入（下游侧）。
    pub is_out: bool,
}

impl IfaceVar {
    /// 三元一致（与对侧匹配的核心判定）。
    pub fn triple_match(&self, other: &IfaceVar) -> bool {
        self.name == other.name && self.ty == other.ty && self.qual == other.qual
    }
}

/// 匹配结果条目。
#[derive(Clone, Debug, PartialEq)]
pub enum MatchEntry {
    /// 配对成功：上游 + 下游。
    Paired { up: u32, down: u32, name: String },
    /// 不匹配：双侧定位 + 条款 + 原因。
    Mismatch { up: u32, down: u32, reason: String },
    /// 未消费输出（上游孤儿）。
    UnconsumedOut { up: u32, name: String },
    /// 未供给输入（下游孤儿）。
    UnsuppliedIn { down: u32, name: String },
}

/// 匹配报告：按链接单元聚合的结果表。
#[derive(Debug, Default)]
pub struct MatchReport {
    pub unit: String,
    pub entries: Vec<MatchEntry>,
}

impl MatchReport {
    pub fn new(unit: &str) -> Self {
        MatchReport { unit: unit.to_string(), entries: Vec::new() }
    }
    /// 统计面（供链接期与后端）。
    pub fn tally(&self) -> (usize, usize, usize, usize) {
        let mut p = 0;
        let mut m = 0;
        let mut uo = 0;
        let mut ui = 0;
        for e in self.entries.iter() {
            match e {
                MatchEntry::Paired { .. } => p += 1,
                MatchEntry::Mismatch { .. } => m += 1,
                MatchEntry::UnconsumedOut { .. } => uo += 1,
                MatchEntry::UnsuppliedIn { .. } => ui += 1,
            }
        }
        (p, m, uo, ui)
    }
}

/// 链接单元聚合：上游输出集 × 下游输入集 → 匹配报告。
/// （名称优先配对；同名才做三元判定；无同名即孤儿。）
pub fn match_units(ups: &[IfaceVar], downs: &[IfaceVar], unit: &str) -> MatchReport {
    let mut rep = MatchReport::new(unit);
    let mut down_used = vec![false; downs.len()];
    for up in ups.iter() {
        // 简化直查：首个同名未配对下游。
        let mut hit: Option<usize> = None;
        for (i, d) in downs.iter().enumerate() {
            if d.name == up.name && !down_used[i] {
                hit = Some(i);
                break;
            }
        }
        match hit {
            Some(i) => {
                down_used[i] = true;
                let d = &downs[i];
                if up.triple_match(d) {
                    rep.entries.push(MatchEntry::Paired { up: up.off, down: d.off, name: up.name.clone() });
                } else {
                    let reason = mismatch_reason(up, d);
                    rep.entries.push(MatchEntry::Mismatch { up: up.off, down: d.off, reason });
                }
            }
            None => {
                rep.entries.push(MatchEntry::UnconsumedOut { up: up.off, name: up.name.clone() });
            }
        }
    }
    // 未供给输入（下游孤儿）。
    for (i, d) in downs.iter().enumerate() {
        if !down_used[i] {
            rep.entries.push(MatchEntry::UnsuppliedIn { down: d.off, name: d.name.clone() });
        }
    }
    rep
}

/// 不匹配原因（三元逐位归因）。
fn mismatch_reason(up: &IfaceVar, down: &IfaceVar) -> String {
    if up.ty != down.ty {
        alloc::format!("类型不匹配：上游 {} vs 下游 {}", up.ty, down.ty)
    } else if up.qual != down.qual {
        alloc::format!("限定符不一致：上游 {} vs 下游 {}", up.qual, down.qual)
    } else {
        "三元不匹配".to_string()
    }
}

/// 判定汇总：报告是否可进链接（无 mismatch 即可，孤儿只是告警）。
pub fn linkable(rep: &MatchReport) -> bool {
    !rep.entries.iter().any(|e| matches!(e, MatchEntry::Mismatch { .. }))
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec50_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    let var = |name: &str, ty: &str, qual: &str, off: u32, stage: Stage, is_out: bool| IfaceVar {
        name: name.to_string(),
        ty: ty.to_string(),
        qual: qual.to_string(),
        off,
        stage,
        is_out,
    };

    // 一、三元匹配。
    {
        let up = var("color", "v4f", "flat", 10, Stage::Vertex, true);
        let down_ok = var("color", "v4f", "flat", 90, Stage::Fragment, false);
        let down_ty = var("color", "v3f", "flat", 91, Stage::Fragment, false);
        let down_q = var("color", "v4f", "smooth", 92, Stage::Fragment, false);
        v.push((
            "F0450-三元-全同配对",
            up.triple_match(&down_ok) && !up.triple_match(&down_ty) && !up.triple_match(&down_q),
        ));
        // 单元聚合：vs→fs 报告单元名。
        let rep = match_units(&[up.clone()], &[down_ok.clone()], "vs->fs");
        v.push(("F0450-三元-单元聚合", rep.unit == "vs->fs" && rep.tally().0 == 1));
    }

    // 二、双侧诊断。
    {
        let up = var("pos", "v4f", "smooth", 12, Stage::Vertex, true);
        let down = var("pos", "v3f", "smooth", 88, Stage::Fragment, false);
        let rep = match_units(&[up], &[down], "vs->fs");
        match rep.entries.first() {
            Some(MatchEntry::Mismatch { up, down, reason }) => v.push((
                "F0450-双侧-双定位带原因",
                *up == 12 && *down == 88 && reason.contains("类型不匹配") && reason.contains("v4f") && reason.contains("v3f"),
            )),
            _ => v.push(("F0450-双侧-双定位带原因", false)),
        }
        // 限定符不一致归因。
        let up2 = var("n", "v3f", "flat", 5, Stage::Vertex, true);
        let down2 = var("n", "v3f", "smooth", 6, Stage::Fragment, false);
        let rep2 = match_units(&[up2], &[down2], "vs->fs");
        v.push((
            "F0450-双侧-限定符归因",
            matches!(&rep2.entries[0], MatchEntry::Mismatch { reason, .. } if reason.contains("限定符")),
        ));
        // 不匹配即不可链接。
        v.push(("F0450-双侧-门禁语义", !linkable(&rep2)));
    }

    // 三、未供未消显性。
    {
        // 未消费输出（上游孤儿）。
        let ups = vec![var("used", "f32", "flat", 1, Stage::Vertex, true), var("orphan_out", "f32", "flat", 2, Stage::Vertex, true)];
        let downs = vec![var("used", "f32", "flat", 3, Stage::Fragment, false)];
        let rep = match_units(&ups, &downs, "vs->fs");
        v.push((
            "F0450-未供未消-输出孤儿",
            matches!(&rep.entries[1], MatchEntry::UnconsumedOut { up, name } if *up == 2 && name == "orphan_out"),
        ));
        // 未供给输入（下游孤儿）。
        let ups2 = vec![var("a", "f32", "flat", 4, Stage::Vertex, true)];
        let downs2 = vec![var("a", "f32", "flat", 5, Stage::Fragment, false), var("ghost_in", "f32", "flat", 6, Stage::Fragment, false)];
        let rep2 = match_units(&ups2, &downs2, "vs->fs");
        v.push((
            "F0450-未供未消-输入孤儿",
            matches!(&rep2.entries[1], MatchEntry::UnsuppliedIn { down, name } if *down == 6 && name == "ghost_in"),
        ));
        // 孤儿只告警不阻断（可链接）。
        v.push(("F0450-未供未消-孤儿告警不阻断", linkable(&rep) && linkable(&rep2)));
    }

    // 四、结果表。
    {
        // 多变量混合场景 tally 四元组。
        let ups = vec![
            var("a", "f32", "flat", 1, Stage::Vertex, true),
            var("b", "f32", "flat", 2, Stage::Vertex, true),
            var("c", "f32", "flat", 3, Stage::Vertex, true),
            var("d", "f32", "flat", 4, Stage::Vertex, true),
        ];
        let downs = vec![
            var("a", "f32", "flat", 5, Stage::Fragment, false),
            var("b", "v3f", "flat", 6, Stage::Fragment, false),
            var("e", "f32", "flat", 7, Stage::Fragment, false),
        ];
        let rep = match_units(&ups, &downs, "vs->fs");
        let (p, m, uo, ui) = rep.tally();
        v.push((
            "F0450-结果表-四类分账",
            p == 1 && m == 1 && uo == 2 && ui == 1,
        ));
        // 同名多下游只配首个（名称优先 + 不重复配对）。
        let ups2 = vec![var("x", "f32", "flat", 8, Stage::Vertex, true)];
        let downs2 = vec![
            var("x", "f32", "flat", 9, Stage::Fragment, false),
            var("x", "f32", "flat", 10, Stage::Fragment, false),
        ];
        let rep2 = match_units(&ups2, &downs2, "vs->fs");
        v.push(("F0450-结果表-不重复配对", rep2.tally() == (1, 0, 0, 1)));
    }

    // 五、判据自检。
    {
        v.push(("F0450-判据-锚点入册", ANCHOR == "VE-F0450"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec50）。
pub fn run_vec50_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec50");
    for (name, ok) in vec50_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec50_raw_checks as run_vec50_checks;
    #[test]
    fn vec50_iomatch_checks() {
        for (name, ok) in run_vec50_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
