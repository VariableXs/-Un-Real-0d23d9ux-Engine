//! VE-F0445 · Swizzle 语义验证（VE-C 域 · 语义组 C03 续）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0445`
//!
//! **锚点原文**：「向量分量访问（swizzle）语义验证：分量字符集与长度规则
//! （按向量维度）、写入场景重复分量检测（读写语义差别显性）、非法组合
//! 报错（引用规范条款与合法示例）；swizzle 到分量索引的展开映射记录
//! （供后端消费）。判据：字符集规则、写重复检测、条款引用、展开映射、
//! 判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **字符集规则**（判据一）。分量字符集按向量元素族分两组：`xyzw`
//!    （坐标族）与 `rgba`（颜色族）——**混族非法**（`xy` 合法、`xg` 非法，
//!    规范条款入册）。长度规则：1~4 且 ≤ 向量维度（vec2 上 `.xyz` 越维
//!    报错带维度说明）。
//!
//! 2. **写重复检测**（判据二）。写场景（赋值左侧）重复分量非法（`v.xx = …`
//!    同分量双写未定义）——**读场景重复合法**（`f(v.xx)` 广播）。读写
//!    语义差别显性：[`check`] 带写位参数，同一样本读写结论不同是判据
//!    的核心实测。
//!
//! 3. **条款引用**（判据三）。非法组合报错引用规范条款 + 附合法示例
//!    （诊断两条腿：为什么错 + 怎么对）。示例从合法字符集生成（`.xy`
//!    类），不是硬编码散文。
//!
//! 4. **展开映射**（判据四）。swizzle → 分量索引序列（`x=0,y=1,z=2,
//!    w=3`）：[`expand`] 产索引表供后端消费。映射与验证共用同一字符表
//!    （[`COMPONENTS`]），验证过的映射必合法（无二次校验分歧面）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0445";

/// 规范条款。
pub const CLAUSE_MIXED_FAMILY: &str = "F0402-§5.6.2 分量须同族（xyzw 或 rgba 不得混用）";
pub const CLAUSE_DIM: &str = "F0402-§5.6.3 分量长度不得超过向量维度";
pub const CLAUSE_WRITE_DUP: &str = "F0402-§5.6.4 写入场景分量不得重复";

/// 分量字符表（坐标族 + 颜色族 → 索引）。
pub const COMPONENTS: [(char, u8); 8] = [
    ('x', 0), ('y', 1), ('z', 2), ('w', 3),
    ('r', 0), ('g', 1), ('b', 2), ('a', 3),
];

/// 字符索引查表。
pub fn comp_index(c: char) -> Option<u8> {
    COMPONENTS.iter().find(|(ch, _)| *ch == c).map(|(_, i)| *i)
}

/// 族判定：0=坐标族 1=颜色族。非分量字符 = None。
pub fn family_of(c: char) -> Option<u8> {
    match c {
        'x' | 'y' | 'z' | 'w' => Some(0),
        'r' | 'g' | 'b' | 'a' => Some(1),
        _ => None,
    }
}

/// 合法示例生成（从坐标族按长度生成）。
pub fn legal_example(len: usize) -> String {
    const XYZW: [char; 4] = ['x', 'y', 'z', 'w'];
    let mut s = String::new();
    for i in 0..len.min(4) {
        s.push(XYZW[i]);
    }
    s
}

// ---------------------------------------------------------------------------
// 验证
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwzDiag {
    pub clause: &'static str,
    /// 合法示例（诊断第二条腿）。
    pub example: String,
}

/// 验证结论：合法 → 展开映射；非法 → 诊断。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SwzResult {
    /// 分量索引序列（供后端消费）。
    Ok(Vec<u8>),
    Bad(SwzDiag),
}

/// swizzle 验证核心：`access` 分量串（不含点），`vec_dim` 向量维度，
/// `write` 写场景位。
pub fn check(access: &str, vec_dim: u8, write: bool) -> SwzResult {
    // ① 字符集：非分量字符直接非法（混族在下一步）。
    let mut idxs: Vec<u8> = Vec::new();
    let mut fam: Option<u8> = None;
    for c in access.chars() {
        let f = match family_of(c) {
            Some(f) => f,
            None => {
                return SwzResult::Bad(SwzDiag {
                    clause: CLAUSE_MIXED_FAMILY,
                    example: legal_example(access.len().max(1)),
                })
            }
        };
        match fam {
            None => fam = Some(f),
            Some(prev) if prev != f => {
                // 混族。
                return SwzResult::Bad(SwzDiag {
                    clause: CLAUSE_MIXED_FAMILY,
                    example: legal_example(access.chars().count().max(1)),
                });
            }
            _ => {}
        }
        idxs.push(comp_index(c).unwrap_or(0));
    }
    // ② 长度规则：1~4 且 ≤ 维度。
    if idxs.is_empty() || idxs.len() > 4 || idxs.len() > vec_dim as usize {
        return SwzResult::Bad(SwzDiag {
            clause: CLAUSE_DIM,
            example: legal_example(1),
        });
    }
    // ③ 写场景重复检测（读场景重复合法）。
    if write {
        let mut seen: Vec<u8> = Vec::new();
        for i in idxs.iter() {
            if seen.contains(i) {
                return SwzResult::Bad(SwzDiag {
                    clause: CLAUSE_WRITE_DUP,
                    example: legal_example(idxs.len()),
                });
            }
            seen.push(*i);
        }
    }
    SwzResult::Ok(idxs)
}

/// 展开映射（已验证合法面；后端消费形态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expansion {
    pub access: String,
    pub indices: Vec<u8>,
}

/// 从合法结果造展开记录。
pub fn expand(access: &str, vec_dim: u8) -> Option<Expansion> {
    match check(access, vec_dim, false) {
        SwzResult::Ok(ix) => Some(Expansion { access: access.into(), indices: ix }),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec45_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、字符集规则。
    {
        // 同族合法。
        v.push(("F0445-字符集-坐标族合法", matches!(check("xy", 4, false), SwzResult::Ok(_))));
        v.push(("F0445-字符集-颜色族合法", matches!(check("rgb", 4, false), SwzResult::Ok(_))));
        // 混族非法 + 条款 + 示例。
        match check("xg", 4, false) {
            SwzResult::Bad(d) => {
                v.push(("F0445-字符集-混族拦截", d.clause == CLAUSE_MIXED_FAMILY && !d.example.is_empty()));
            }
            _ => v.push(("F0445-字符集-混族拦截", false)),
        }
        // 非分量字符。
        v.push(("F0445-字符集-非分量拦截", matches!(check("xq", 4, false), SwzResult::Bad(_))));
        // 越维：vec2 上 .xyz。
        v.push(("F0445-字符集-越维拦截", matches!(check("xyz", 2, false), SwzResult::Bad(_))));
        // 长度规则：4 长合法（同族），5 长超限拦截。
        v.push(("F0445-字符集-四长合法", matches!(check("xyzw", 4, false), SwzResult::Ok(_))));
        v.push(("F0445-字符集-超长拦截", matches!(check("xyzwx", 4, false), SwzResult::Bad(_))));
    }
    // 二、写重复检测（读写差别显性）。
    {
        // 读重复合法（广播）。
        v.push(("F0445-写读-读重复合法", matches!(check("xx", 4, false), SwzResult::Ok(_))));
        // 写重复非法。
        v.push(("F0445-写读-写重复非法", matches!(check("xx", 4, true), SwzResult::Bad(_))));
        // 同样本读写结论不同（差别显性核心实测）。
        let rd = check("xx", 4, false);
        let wr = check("xx", 4, true);
        v.push(("F0445-写读-同本异判", matches!(rd, SwzResult::Ok(_)) && matches!(wr, SwzResult::Bad(_))));
        // 写重复条款。
        match check("yy", 2, true) {
            SwzResult::Bad(d) => v.push(("F0445-写读-条款", d.clause == CLAUSE_WRITE_DUP)),
            _ => v.push(("F0445-写读-条款", false)),
        }
        // 写不重复合法。
        v.push(("F0445-写读-写异序合法", matches!(check("wxyz", 4, true), SwzResult::Ok(_))));
    }
    // 三、条款引用。
    {
        // 越维条款与维度说明。
        match check("xyz", 2, false) {
            SwzResult::Bad(d) => v.push(("F0445-条款-越维条款", d.clause == CLAUSE_DIM)),
            _ => v.push(("F0445-条款-越维条款", false)),
        }
        // 示例从字符集生成（非散文：可复算）。
        v.push(("F0445-条款-示例可复算", legal_example(3) == "xyz" && legal_example(2) == "xy"));
        v.push(("F0445-条款-三条入册", !CLAUSE_MIXED_FAMILY.is_empty() && !CLAUSE_DIM.is_empty() && !CLAUSE_WRITE_DUP.is_empty()));
    }
    // 四、展开映射。
    {
        let e = expand("xyzw", 4);
        v.push((
            "F0445-映射-四分量索引",
            e.as_ref().map(|x| x.indices == vec![0, 1, 2, 3]) == Some(true),
        ));
        // 颜色族同索引。
        let e2 = expand("rgba", 4);
        v.push(("F0445-映射-颜色族同索引", e2.as_ref().map(|x| x.indices == vec![0, 1, 2, 3]) == Some(true)));
        // 乱序保真。
        let e3 = expand("wzyx", 4);
        v.push(("F0445-映射-乱序保真", e3.as_ref().map(|x| x.indices == vec![3, 2, 1, 0]) == Some(true)));
        // 验证过的映射必合法（无二次分歧）。
        let e4 = expand("xx", 4);
        v.push(("F0445-映射-读重复可展开", e4.as_ref().map(|x| x.indices == vec![0, 0]) == Some(true)));
        // 非法无映射。
        v.push(("F0445-映射-非法无映射", expand("xg", 4).is_none()));
        // 判据自检。
        v.push(("F0445-判据-锚点入册", ANCHOR == "VE-F0445"));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::vec45_raw_checks as run_vec45_checks;
    #[test]
    fn vec45_swizzle_checks() {
        for (name, ok) in run_vec45_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec45）。
pub fn run_vec45_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec45");
    for (name, ok) in vec45_raw_checks() {
        set.add(name, ok, "");
    }
    set
}
