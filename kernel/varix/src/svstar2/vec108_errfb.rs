//! VE-F0508 · GLSL 错误回传处理（VE-C 域 · GLSL 后端转译组 C06）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0508`
//!
//! **锚点原文**：「目标驱动编译错误的回传处理：驱动编译错误解析（错误行号
//! 到生成文本定位、再经映射回溯到 IR 指令与源码）、错误分类（生成器 bug、
//! 驱动怪癖、源码问题三类处置）、驱动怪癖规避登记（F0358 式规避表语义复
//! 用）。判据：双级回溯、三类处置、怪癖规避、缺口登记、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **双级回溯**（判据一）。错误行号 → 生成文本行 →（映射表）→ IR 指令
//!    →（源映射）→ 源码位置，四级链条逐级可查；断链显性标注。
//!
//! 2. **三类处置**（判据二）。生成器 bug（修生成器）、驱动怪癖（规避登记）、
//!    源码问题（回传用户）三分类逐条裁定，未分类错误不得归档。
//!
//! 3. **怪癖规避**（判据三）。怪癖登记表带复现条件与规避改写（F0358 语义
//!    复用），同类怪癖再次命中直接走规避。
//!
//! 4. **缺口登记**（判据四）。回溯断链（无映射行）登记缺口清单——缺口是
//!    可见债务，不是静默丢弃。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0508";

// ---------------------------------------------------------------------------
// 一、错误解析与回溯链
// ---------------------------------------------------------------------------

/// 驱动错误行（解析输入）。
#[derive(Clone, Debug)]
pub struct DrvErr {
    /// 生成文本中的行号（1 基）。
    pub line: u32,
    /// 驱动原始消息。
    pub msg: String,
}

/// 回溯链（四级）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Backtrack {
    /// 生成文本行号。
    pub gen_line: u32,
    /// IR 指令号（无映射 = None，断链）。
    pub ir: Option<u32>,
    /// 源码行（无映射 = None）。
    pub src: Option<u32>,
}

/// 行映射表：生成行 → IR 指令。
pub type LineMap = [(u32, u32); 0];

/// 双级回溯：行映射 + 源映射两级查表。
pub fn backtrack(gen_line: u32, line_map: &[(u32, u32)], src_map: &[(u32, u32)]) -> Backtrack {
    let ir = line_map.iter().find(|(g, _)| *g == gen_line).map(|(_, i)| *i);
    let src = match ir {
        Some(i) => src_map.iter().find(|(n, _)| *n == i).map(|(_, s)| *s),
        None => None,
    };
    Backtrack { gen_line, ir, src }
}

/// 断链判定。
pub fn broken(b: &Backtrack) -> bool {
    b.ir.is_none() || b.src.is_none()
}

// ---------------------------------------------------------------------------
// 二、三类处置
// ---------------------------------------------------------------------------

/// 错误分类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    GenBug,
    Quirk,
    SrcIssue,
}

/// 处置结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disp {
    FixGen,
    Evade,
    ToUser,
}

/// 分类 → 处置映射（三类各自唯一去向）。
pub fn dispatch(k: Kind) -> Disp {
    match k {
        Kind::GenBug => Disp::FixGen,
        Kind::Quirk => Disp::Evade,
        Kind::SrcIssue => Disp::ToUser,
    }
}

/// 已归档错误（必须带分类）。
#[derive(Clone, Debug)]
pub struct Filed {
    pub err: DrvErr,
    pub kind: Kind,
    pub track: Backtrack,
}

/// 错误归档账。
#[derive(Default)]
pub struct ErrBook {
    pub filed: Vec<Filed>,
    /// 回溯断链缺口登记。
    pub gaps: Vec<u32>,
}

impl ErrBook {
    pub fn new() -> ErrBook {
        ErrBook { filed: Vec::new(), gaps: Vec::new() }
    }

    /// 归档：未分类（kind 之外的态不存在于类型层）不归档——本函数只收已分类项。
    pub fn file(&mut self, err: DrvErr, kind: Kind, line_map: &[(u32, u32)], src_map: &[(u32, u32)]) {
        let track = backtrack(err.line, line_map, src_map);
        if broken(&track) {
            self.gaps.push(err.line);
        }
        self.filed.push(Filed { err, kind, track });
    }

    pub fn by_kind(&self, k: Kind) -> usize {
        self.filed.iter().filter(|f| f.kind == k).count()
    }
}

// ---------------------------------------------------------------------------
// 三、怪癖规避表（F0358 语义复用）
// ---------------------------------------------------------------------------

/// 怪癖条目。
#[derive(Clone, Copy, Debug)]
pub struct Quirk {
    /// 驱动标识。
    pub drv: &'static str,
    /// 复现条件（消息特征）。
    pub cond: &'static str,
    /// 规避改写。
    pub evade: &'static str,
}

/// 怪癖正本表。
pub const QUIRKS: [Quirk; 3] = [
    Quirk { drv: "drv-a", cond: "pow(0,0)", evade: "改写为三元选择 pow_guard" },
    Quirk { drv: "drv-b", cond: "for 循环内 texLod", evade: "Lod 提升出循环" },
    Quirk { drv: "drv-c", cond: "高精度 mod(x,y)", evade: "改写为 x - y*floor(x/y)" },
];

/// 怪癖命中：消息含复现条件即命中。
pub fn quirk_hit(drv: &str, msg: &str) -> Option<&'static Quirk> {
    QUIRKS.iter().find(|q| q.drv == drv && msg.contains(q.cond))
}

/// 命中后处置：直接走规避（不再归档为 GenBug）。
pub fn quirk_route(drv: &str, msg: &str, default: Kind) -> (Disp, Option<&'static str>) {
    match quirk_hit(drv, msg) {
        Some(q) => (Disp::Evade, Some(q.evade)),
        None => (dispatch(default), None),
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec108_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec108");

    // 族一：双级回溯。
    let lm = [(1u32, 10u32), (2, 11), (5, 20)];
    let sm = [(10u32, 100u32), (11, 101), (20, 200)];
    let t1 = backtrack(2, &lm, &sm);
    s.add("四级链通", t1 == Backtrack { gen_line: 2, ir: Some(11), src: Some(101) }, "行→IR→源");
    s.add("行无映射断链", broken(&backtrack(9, &lm, &sm)), "IR 级断");
    s.add("IR 无源断链", broken(&backtrack(1, &lm, &sm)) == false, "有源不断");
    let sm2 = [(10u32, 100u32)];
    s.add("源级断链", broken(&backtrack(2, &lm, &sm2)), "缺源映射");

    // 族二：三类处置。
    s.add("bug→修生成器", dispatch(Kind::GenBug) == Disp::FixGen, "类一");
    s.add("怪癖→规避", dispatch(Kind::Quirk) == Disp::Evade, "类二");
    s.add("源码→回传用户", dispatch(Kind::SrcIssue) == Disp::ToUser, "类三");

    // 族三：归档与缺口登记。
    let mut eb = ErrBook::new();
    eb.file(DrvErr { line: 2, msg: "syntax".to_string() }, Kind::GenBug, &lm, &sm);
    eb.file(DrvErr { line: 5, msg: "internal".to_string() }, Kind::Quirk, &lm, &sm);
    eb.file(DrvErr { line: 9, msg: "unknown".to_string() }, Kind::SrcIssue, &lm, &sm);
    s.add("归档三件", eb.filed.len() == 3, "全部入账");
    s.add("分类可数", eb.by_kind(Kind::GenBug) == 1 && eb.by_kind(Kind::Quirk) == 1, "按类检索");
    s.add("断链缺口登记", eb.gaps.contains(&9), "9 号行无映射");
    s.add("全链不断无缺口", eb.gaps.len() == 1, "缺口只此一件");

    // 族四：怪癖规避。
    s.add("怪癖命中", quirk_hit("drv-a", "error at pow(0,0)").is_some(), "复现条件匹配");
    s.add("驱动不符不命中", quirk_hit("drv-z", "pow(0,0)").is_none(), "逐驱动登记");
    let (d1, e1) = quirk_route("drv-c", "highp mod(x,y) failed", Kind::GenBug);
    s.add("命中走规避", d1 == Disp::Evade && e1 == Some("改写为 x - y*floor(x/y)"), "规避直达");
    let (d2, e2) = quirk_route("drv-a", "ordinary failure", Kind::GenBug);
    s.add("未命中走默认", d2 == Disp::FixGen && e2.is_none(), "分类兜底");
    s.add("怪癖表三件带规避", QUIRKS.len() == 3 && QUIRKS.iter().all(|q| !q.evade.is_empty()), "正本规模");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bt() {
        let t = backtrack(1, &[(1, 10)], &[(10, 100)]);
        assert_eq!(t.src, Some(100));
        assert!(!broken(&t));
    }
}
