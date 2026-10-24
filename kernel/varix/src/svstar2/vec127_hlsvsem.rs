//! VE-F0527 · HLSL 系统值语义映射（VE-C 域 · HLSL 后端组 C07）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0527`
//!
//! **锚点原文**：「HLSL 系统值语义映射：IR 阶段接口到 HLSL 系统值语义
//! （SV_Position 等语义标记映射表、阶段间自定义接口到普通插值语义）、语义
//! 唯一性校验（同阶段重复语义报错）；映射与 F0450 匹配结果一致性验证。判据：
//! 语义表、唯一性、一致性验证、阶段要求、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **语义表**（判据一）。IR 阶段接口 → SV 语义逐条映射表单源；表外
//!    接口显性拒绝。
//!
//! 2. **唯一性**（判据二）。同阶段同语义重复报错带双侧定位；SV 语义每
//!    阶段至多一次。
//!
//! 3. **一致性验证**（判据三）。映射结果与 F0450 接口匹配结果对账——矛盾
//!    时以匹配为准回溯修映射。
//!
//! 4. **阶段要求**（判据四）。SV 语义有阶段约束（如 SV_VertexID 仅顶点
//!    阶段），错阶段使用显性拒绝并带阶段说明。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0527";

// ---------------------------------------------------------------------------
// 一、阶段模型
// ---------------------------------------------------------------------------

/// 着色阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stg {
    Vertex,
    Pixel,
    Geometry,
}

/// 阶段标签。
pub fn stg_tag(s: Stg) -> &'static str {
    match s {
        Stg::Vertex => "vs",
        Stg::Pixel => "ps",
        Stg::Geometry => "gs",
    }
}

// ---------------------------------------------------------------------------
// 二、SV 语义映射表
// ---------------------------------------------------------------------------

/// SV 语义条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SvEntry {
    pub ir_name: &'static str,
    pub sv: &'static str,
    /// 允许阶段。
    pub stages: [Stg; 3],
    /// 实际允许数。
    pub n: usize,
}

impl SvEntry {
    pub fn allows(&self, s: Stg) -> bool {
        self.stages[..self.n].contains(&s)
    }
}

/// 语义正本表。
pub const SV_TABLE: [SvEntry; 5] = [
    SvEntry { ir_name: "clip_pos", sv: "SV_Position", stages: [Stg::Vertex, Stg::Geometry, Stg::Pixel], n: 3 },
    SvEntry { ir_name: "vertex_id", sv: "SV_VertexID", stages: [Stg::Vertex, Stg::Vertex, Stg::Vertex], n: 1 },
    SvEntry { ir_name: "instance_id", sv: "SV_InstanceID", stages: [Stg::Vertex, Stg::Vertex, Stg::Vertex], n: 1 },
    SvEntry { ir_name: "target", sv: "SV_Target", stages: [Stg::Pixel, Stg::Pixel, Stg::Pixel], n: 1 },
    SvEntry { ir_name: "is_front", sv: "SV_IsFrontFace", stages: [Stg::Pixel, Stg::Geometry, Stg::Geometry], n: 2 },
];

/// 映射：接口名 + 阶段 → SV 文本。
pub fn map_sv(ir: &str, stg: Stg) -> Result<&'static str, u32> {
    let e = SV_TABLE.iter().find(|e| e.ir_name == ir).ok_or(XC_NOIF)?;
    if !e.allows(stg) {
        return Err(XC_STAGE);
    }
    Ok(e.sv)
}

pub const XC_NOIF: u32 = 1;
pub const XC_STAGE: u32 = 2;

/// 自定义接口 → 普通插值语义（TEXCOORDn）。
pub fn custom_interp(idx: u32) -> String {
    format!("TEXCOORD{}", idx)
}

// ---------------------------------------------------------------------------
// 三、唯一性校验
// ---------------------------------------------------------------------------

/// 阶段语义登记。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Use {
    pub var: String,
    pub sv: String,
    pub stg: Stg,
}

/// 同阶段重复语义检测（双侧定位）。
pub fn dup_check(uses: &[Use]) -> Option<(String, String)> {
    for i in 0..uses.len() {
        for j in (i + 1)..uses.len() {
            let (a, b) = (&uses[i], &uses[j]);
            if a.stg == b.stg && a.sv == b.sv {
                return Some((a.var.clone(), b.var.clone()));
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 四、与 F0450 匹配一致性
// ---------------------------------------------------------------------------

/// 匹配结果（F0450 产出：接口名 → 是否匹配）。
pub type MatchBook = [(String, bool); 0];

/// 一致性验证：映射表内的接口必须在匹配账中有结论且一致。
pub fn consistent(mapped: &[String], matched: &[(String, bool)]) -> Result<(), u32> {
    for m in mapped.iter() {
        match matched.iter().find(|(n, _)| n == m) {
            None => return Err(XC_NOMATCH),
            Some((_, false)) => return Err(XC_MISMATCH),
            Some((_, true)) => {}
        }
    }
    Ok(())
}

pub const XC_NOMATCH: u32 = 3;
pub const XC_MISMATCH: u32 = 4;

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec127_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec127");

    // 族一：语义表。
    s.add("Position 三阶段", map_sv("clip_pos", Stg::Pixel) == Ok("SV_Position"), "全阶段可用");
    s.add("VertexID 仅顶点", map_sv("vertex_id", Stg::Vertex) == Ok("SV_VertexID"), "映射正确");
    s.add("VertexID 像素拒", matches!(map_sv("vertex_id", Stg::Pixel), Err(XC_STAGE)), "阶段要求显性");
    s.add("Target 仅像素", map_sv("target", Stg::Geometry) == Err(XC_STAGE), "输出语义限 ps");
    s.add("表外接口拒", matches!(map_sv("unknown_if", Stg::Vertex), Err(XC_NOIF)), "显性拒绝");
    s.add("语义表五件", SV_TABLE.len() == 5, "正本规模");

    // 族二：自定义插值。
    s.add("TEXCOORD0", custom_interp(0) == "TEXCOORD0", "自定义接口");
    s.add("TEXCOORD12", custom_interp(12) == "TEXCOORD12", "序号透传");

    // 族三：唯一性。
    let vs = vec![
        Use { var: "p".to_string(), sv: "SV_Position".to_string(), stg: Stg::Vertex },
        Use { var: "id".to_string(), sv: "SV_Position".to_string(), stg: Stg::Vertex },
    ];
    s.add("同阶段重复定位", dup_check(&vs) == Some(("p".to_string(), "id".to_string())), "双侧报错");
    s.add("跨阶段不冲突", {
        let ok = vec![
            Use { var: "p".to_string(), sv: "SV_Position".to_string(), stg: Stg::Vertex },
            Use { var: "p2".to_string(), sv: "SV_Position".to_string(), stg: Stg::Pixel },
        ];
        dup_check(&ok).is_none()
    }, "vs/ps 各自唯一");
    s.add("无重复为 None", dup_check(&[]).is_none(), "空账通过");

    // 族四：匹配一致性。
    let mapped = vec!["clip_pos".to_string(), "vertex_id".to_string()];
    let matched = vec![
        ("clip_pos".to_string(), true),
        ("vertex_id".to_string(), true),
    ];
    s.add("一致放行", consistent(&mapped, &matched).is_ok(), "映射↔匹配同向");
    s.add("缺匹配拦截", {
        let half = vec![("clip_pos".to_string(), true)];
        matches!(consistent(&mapped, &half), Err(XC_NOMATCH))
    }, "回溯补映射");
    s.add("匹配失败拦截", {
        let bad = vec![("clip_pos".to_string(), true), ("vertex_id".to_string(), false)];
        matches!(consistent(&mapped, &bad), Err(XC_MISMATCH))
    }, "以匹配为准");

    s

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sv() {
        assert_eq!(map_sv("clip_pos", Stg::Vertex), Ok("SV_Position"));
        assert!(map_sv("vertex_id", Stg::Geometry).is_err());
    }
}
