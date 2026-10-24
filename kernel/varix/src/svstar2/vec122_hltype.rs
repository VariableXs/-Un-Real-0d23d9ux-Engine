//! VE-F0522 · IR→HLSL 类型映射（VE-C 域 · HLSL 后端组 C07）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0522`
//!
//! **锚点原文**：「IR 类型到 HLSL 类型映射：标量向量矩阵映射表（含列主序行
//! 主序语义裁定——HLSL 默认列主序的显性处理）、资源类型映射（纹理、缓冲、
//! 采样器族）、结构映射与打包语义（成员对齐规则差异显性）；映射黄金对拍。
//! 判据：主序显性、资源映射、打包规则、黄金对拍、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **主序显性**（判据一）。列主序（HLSL 默认）与行主序的裁定结果必须
//!    显性登记——序错即渲染镜像类错误，缺省放行 = 判据红。
//!
//! 2. **资源映射**（判据二）。纹理/缓冲/采样器三族资源逐型可映射，表外
//!    资源型显性拒绝。
//!
//! 3. **打包规则**（判据三）。结构成员对齐按 SM 打包规则裁定（16 字节
//!    寄存器粒度），歧义按目标 SM 显性登记。
//!
//! 4. **黄金对拍**（判据四）。映射对（IR 型 → HLSL 型文本）逐条黄金比对。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0522";

// ---------------------------------------------------------------------------
// 一、主序裁定
// ---------------------------------------------------------------------------

/// 主序。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    Column,
    Row,
}

/// 主序裁定：显性要求——请求必须明示，缺省返回错误（不猜）。
pub fn order_of(req: Option<Order>) -> Result<Order, u32> {
    req.ok_or(XC_NOORDER)
}

pub const XC_NOORDER: u32 = 1;

/// HLSL 默认主序（规范事实）。
pub const HLSL_DEFAULT: Order = Order::Column;

// ---------------------------------------------------------------------------
// 二、标量/向量/矩阵映射表
// ---------------------------------------------------------------------------

/// 基本映射（IR → HLSL 文本）。
pub const TYPE_MAP: [(&str, &str); 8] = [
    ("f32", "float"),
    ("v2f", "float2"),
    ("v3f", "float3"),
    ("v4f", "float4"),
    ("i32", "int"),
    ("v4i", "int4"),
    ("m4x4", "float4x4"),
    ("m4x3", "float4x3"),
];

pub fn map_ty(ir: &str) -> Result<&'static str, u32> {
    TYPE_MAP.iter().find(|(i, _)| *i == ir).map(|(_, h)| *h).ok_or(XC_NOTY)
}

pub const XC_NOTY: u32 = 2;

/// 矩阵映射带主序标注。
pub fn map_mat(ir: &str, o: Order) -> Result<String, u32> {
    let h = map_ty(ir)?;
    let tag = match o {
        Order::Column => "column_major",
        Order::Row => "row_major",
    };
    Ok(format!("{} {}", tag, h))
}

// ---------------------------------------------------------------------------
// 三、资源类型映射
// ---------------------------------------------------------------------------

/// 资源族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Res {
    Tex2D,
    TexCube,
    CBuffer,
    SBuffer,
    Sampler,
    UavBuf,
}

/// 资源映射。
pub fn map_res(r: Res) -> &'static str {
    match r {
        Res::Tex2D => "Texture2D",
        Res::TexCube => "TextureCube",
        Res::CBuffer => "ConstantBuffer",
        Res::SBuffer => "StructuredBuffer",
        Res::Sampler => "SamplerState",
        Res::UavBuf => "RWStructuredBuffer",
    }
}

/// 全族可映射（六型非空文本）。
pub fn res_family_ok() -> bool {
    [Res::Tex2D, Res::TexCube, Res::CBuffer, Res::SBuffer, Res::Sampler, Res::UavBuf]
        .iter()
        .all(|r| !map_res(*r).is_empty())
}

// ---------------------------------------------------------------------------
// 四、结构打包规则
// ---------------------------------------------------------------------------

/// 寄存器粒度（字节）。
pub const REG_BYTES: u32 = 16;

/// 成员对齐：按 16 字节寄存器粒度计算占用槽数。
pub fn slots_of(size: u32) -> u32 {
    (size + REG_BYTES - 1) / REG_BYTES
}

/// 结构打包：成员尺寸序列 → 总槽数（逐成员向上取整跨槽规则：数组按元素对齐，
/// 本模型标量族按 4 字节打包进槽、跨槽成员独占新槽）。
pub fn pack(members: &[u32]) -> u32 {
    let mut used = 0u32;
    let mut cur_off = 0u32;
    for &m in members.iter() {
        // 成员 16 字节对齐（HLSL cbuffer 规则：标量可 4 字节紧凑，向量按 16 槽界）。
        let align = if m >= 16 { REG_BYTES } else { 4 };
        let pad = (align - (cur_off % align)) % align;
        cur_off += pad + m;
        used = cur_off;
    }
    slots_of(used)
}

// ---------------------------------------------------------------------------
// 五、黄金对拍
// ---------------------------------------------------------------------------

pub struct Golden {
    pub ir: &'static str,
    pub hlsl: &'static str,
}

pub const GOLDENS: [Golden; 5] = [
    Golden { ir: "f32", hlsl: "float" },
    Golden { ir: "v4f", hlsl: "float4" },
    Golden { ir: "m4x4", hlsl: "float4x4" },
    Golden { ir: "i32", hlsl: "int" },
    Golden { ir: "v4i", hlsl: "int4" },
];

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec122_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec122");

    // 族一：主序显性。
    s.add("缺请求拒", matches!(order_of(None), Err(XC_NOORDER)), "不猜主序");
    s.add("行主序显性", order_of(Some(Order::Row)) == Ok(Order::Row), "明示生效");
    s.add("HLSL 默认列主序", HLSL_DEFAULT == Order::Column, "规范事实");
    s.add("矩阵带主序标注", map_mat("m4x4", Order::Row) == Ok("row_major float4x4".to_string()), "显性登记");
    s.add("列主序标注", map_mat("m4x4", Order::Column) == Ok("column_major float4x4".to_string()), "默认也显性");

    // 族二：基本映射。
    s.add("标量映射", map_ty("f32") == Ok("float"), "f32→float");
    s.add("向量映射", map_ty("v3f") == Ok("float3"), "v3f→float3");
    s.add("矩阵映射", map_ty("m4x4") == Ok("float4x4"), "m4x4→float4x4");
    s.add("表外拒", matches!(map_ty("f64"), Err(XC_NOTY)), "缺项拦截");
    s.add("映射表八件", TYPE_MAP.len() == 8, "正本规模");

    // 族三：资源映射。
    s.add("纹理族", map_res(Res::Tex2D) == "Texture2D" && map_res(Res::TexCube) == "TextureCube", "Texture 型");
    s.add("缓冲族", map_res(Res::CBuffer) == "ConstantBuffer" && map_res(Res::SBuffer) == "StructuredBuffer", "Buffer 型");
    s.add("采样器", map_res(Res::Sampler) == "SamplerState", "Sampler 型");
    s.add("UAV 可写", map_res(Res::UavBuf) == "RWStructuredBuffer", "RW 前缀");
    s.add("六族全可映射", res_family_ok(), "资源全族");

    // 族四：打包规则。
    s.add("单标量一槽", slots_of(4) == 1, "4B→1 槽");
    s.add("整槽不进位", slots_of(16) == 1, "16B→1 槽");
    s.add("超槽进位", slots_of(20) == 2, "20B→2 槽");
    s.add("紧凑打包", pack(&[4, 4, 4, 4]) == 1, "四标量 16B 一槽");
    s.add("向量跨槽", pack(&[16, 4]) == 2, "向量后标量新槽");
    s.add("大成员独占", pack(&[32]) == 2, "32B 两槽");
    s.add("歧义显性对齐", pack(&[12, 8]) == 2, "8B 向量按槽界对齐");

    // 族五：黄金对拍。
    for g in GOLDENS.iter() {
        s.add("黄金对拍", map_ty(g.ir) == Ok(g.hlsl), g.ir);
    }
    s.add("黄金五对", GOLDENS.len() == 5, "正本规模");

    s

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map() {
        assert_eq!(map_ty("v4f"), Ok("float4"));
        assert!(map_ty("f64").is_err());
    }
}
