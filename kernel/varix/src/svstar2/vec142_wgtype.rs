//! VE-F0542 · WGSL 类型映射（VE-C 域 · WGSL 后端组 C08）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0542`
//!
//! **锚点原文**：「IR 类型到 WGSL 类型映射：标量向量矩阵映射（WGSL 强类型
//! 无隐式转换——显性转换指令生成语义登记）、数组与结构映射（stride 语义
//! 显性化）、资源类型映射（address space 维度联动 F0547）；映射黄金对拍独立
//! 维护。判据：强类型显性转换、stride 显性、维度联动、黄金对拍、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **显性转换**（判据一）。WGSL 无隐式转换——IR 隐式场景（混算 i32/f32）
//!    生成侧插入显性转换文本（f32(x)/i32(x)）并登记。
//!
//! 2. **stride 显性**（判据二）。数组 stride 按规范规则裁定并随类型文本
//!    显性（stride 歧义不猜）。
//!
//! 3. **维度联动**（判据三）。资源类型与 address space 联动登记（供 F0547
//!    消费），联动缺失即拦截。
//!
//! 4. **黄金对拍**（判据四）。WGSL 黄金对独立维护（与 GLSL/HLSL 分册）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0542";

// ---------------------------------------------------------------------------
// 一、基本映射
// ---------------------------------------------------------------------------

pub const TYPE_MAP: [(&str, &str); 8] = [
    ("f32", "f32"),
    ("v2f", "vec2<f32>"),
    ("v4f", "vec4<f32>"),
    ("i32", "i32"),
    ("v2i", "vec2<i32>"),
    ("u32", "u32"),
    ("m4x4", "mat4x4<f32>"),
    ("bool", "bool"),
];

pub fn map_ty(ir: &str) -> Result<&'static str, u32> {
    TYPE_MAP.iter().find(|(i, _)| *i == ir).map(|(_, w)| *w).ok_or(XC_NOTY)
}

pub const XC_NOTY: u32 = 1;

// ---------------------------------------------------------------------------
// 二、显性转换插入
// ---------------------------------------------------------------------------

/// 混算场景探测：操作数类型集含 f32 与 i32 即隐式。
pub fn mixed(ops: &[&str]) -> bool {
    ops.contains(&"f32") && ops.contains(&"i32")
}

/// 显性转换文本生成（WGSL 语法：f32(x) / i32(x)）。
pub fn cast_to(target: &str, x: &str) -> String {
    format!("{}({})", target, x)
}

/// 转换登记。
#[derive(Default)]
pub struct CastLog {
    pub casts: Vec<String>,
}

impl CastLog {
    pub fn new() -> CastLog {
        CastLog { casts: Vec::new() }
    }

    /// 隐式场景 → 插入显性转换并登记。
    pub fn resolve(&mut self, ops: &[&str], target: &str, x: &str) -> String {
        if mixed(ops) {
            let c = cast_to(target, x);
            self.casts.push(c.clone());
            c
        } else {
            x.to_string()
        }
    }

    pub fn len(&self) -> usize {
        self.casts.len()
    }
}

// ---------------------------------------------------------------------------
// 三、数组 stride 与结构
// ---------------------------------------------------------------------------

/// 元素尺寸 → stride（WGSL 默认对齐：f32 族 4，vec4<f32> 16）。
pub fn stride_of(elem: u32) -> u32 {
    match elem {
        4 | 8 | 16 => elem,
        _ => (elem + 15) / 16 * 16, // 向上对齐到 16
    }
}

/// 数组类型文本（stride 显性）。
pub fn array_ty(elem: &str, n: u32, elem_size: u32) -> String {
    format!("array<{}, {}> /* stride={} */", elem, n, stride_of(elem_size))
}

// ---------------------------------------------------------------------------
// 四、资源维度联动
// ---------------------------------------------------------------------------

/// 资源型 → 要求的 address space（供 F0547）。
pub fn res_space(res: &str) -> Result<&'static str, u32> {
    match res {
        "uniform-buf" => Ok("uniform"),
        "storage-buf" => Ok("storage"),
        "texture" => Ok("texture_2d"),
        "sampler" => Ok("sampler"),
        _ => Err(XC_NORES),
    }
}

pub const XC_NORES: u32 = 2;

// ---------------------------------------------------------------------------
// 五、WGSL 黄金对
// ---------------------------------------------------------------------------

pub struct WGolden {
    pub ir: &'static str,
    pub wgsl: &'static str,
}

pub const W_GOLDENS: [WGolden; 4] = [
    WGolden { ir: "f32", wgsl: "f32" },
    WGolden { ir: "v4f", wgsl: "vec4<f32>" },
    WGolden { ir: "m4x4", wgsl: "mat4x4<f32>" },
    WGolden { ir: "bool", wgsl: "bool" },
];

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec142_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec142");

    // 族一：基本映射。
    s.add("标量", map_ty("f32") == Ok("f32"), "同形");
    s.add("向量", map_ty("v4f") == Ok("vec4<f32>"), "尖括号形态");
    s.add("矩阵", map_ty("m4x4") == Ok("mat4x4<f32>"), "mat 形态");
    s.add("表外拒", matches!(map_ty("f64"), Err(XC_NOTY)), "缺项拦截");
    s.add("映射八件", TYPE_MAP.len() == 8, "正本规模");

    // 族二：显性转换。
    s.add("混算探测", mixed(&["f32", "i32"]), "隐式场景");
    s.add("同型不混", !mixed(&["f32", "f32"]), "健康场景");
    let mut cl = CastLog::new();
    let r = cl.resolve(&["f32", "i32"], "f32", "x");
    s.add("显性转换插入", r == "f32(x)", "强类型合规");
    s.add("转换登记", cl.len() == 1, "可审计");
    s.add("无混不转", cl.resolve(&["f32", "f32"], "f32", "y") == "y" && cl.len() == 1, "不冗余转");
    s.add("转换文本形态", cast_to("i32", "f") == "i32(f)", "WGSL 语法");

    // 族三：stride。
    s.add("对齐尺寸直通", stride_of(16) == 16, "vec4 一槽");
    s.add("非对齐升槽", stride_of(12) == 16, "向上取整");
    s.add("stride 显性标注", array_ty("f32", 8, 4).contains("stride=4"), "文本内显性");
    s.add("数组形态", array_ty("vec4<f32>", 3, 16).starts_with("array<vec4<f32>, 3>"), "WGSL 数组语法");

    // 族四：维度联动。
    s.add("uniform 联动", res_space("uniform-buf") == Ok("uniform"), "F0547 消费");
    s.add("storage 联动", res_space("storage-buf") == Ok("storage"), "可写空间");
    s.add("纹理联动", res_space("texture") == Ok("texture_2d"), "纹理空间");
    s.add("表外资源拒", matches!(res_space("push-const"), Err(XC_NORES)), "缺项拦截");

    // 族五：黄金对拍。
    for g in W_GOLDENS.iter() {
        s.add("WGSL 黄金", map_ty(g.ir) == Ok(g.wgsl), g.ir);
    }
    s.add("黄金四对", W_GOLDENS.len() == 4, "独立正本");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map() {
        assert_eq!(map_ty("v2f"), Ok("vec2<f32>"));
        assert_eq!(stride_of(12), 16);
    }
}
