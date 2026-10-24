//! VE-F0534 · HLSL 已知限制清单（VE-C 域 · HLSL 后端组 C07）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0534`
//!
//! **锚点原文**：「HLSL 后端已知限制清单（F0510 同构机制复用）：限制项三类
//! 登记（不支持构造、降级语义、工具链版本依赖），每项含影响与替代；与 F0457
//! 语义拒绝清单分工（后端限制 vs 语义兼容限制）；版本化随宣告发布。判据：
//! 三类限制、分工边界、不承诺、版本化、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **三类限制**（判据一）。不支持构造 / 降级语义 / 工具链版本依赖三类
//!    逐项登记（影响 × 替代必填）。
//!
//! 2. **分工边界**（判据二）。语义兼容类（F0457 拒绝清单域）不进本清单；
//!    越界条目即分工违规。
//!
//! 3. **不承诺**（判据三）。受限项查询返回无承诺；缺省不放行。
//!
//! 4. **版本化**（判据四）。清单版本随条目增删自动升版，宣告发布按版本。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0534";

/// 限制类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitKind {
    NoCtor,
    Degrade,
    ToolDep,
}

/// 限制条目。
#[derive(Clone, Debug)]
pub struct Limit {
    pub kind: LimitKind,
    pub ctor: String,
    pub impact: String,
    pub alt: String,
    /// 工具链版本依赖类：最低工具链版本。
    pub min_tool: Option<u32>,
}

/// 清单。
pub struct LimitBook {
    pub ver: u32,
    items: Vec<Limit>,
}

impl LimitBook {
    pub fn new(ver: u32) -> LimitBook {
        LimitBook { ver, items: Vec::new() }
    }

    /// 登记：影响与替代必填；版本依赖类须带最低版本。
    pub fn reg(&mut self, kind: LimitKind, ctor: &str, impact: &str, alt: &str, min_tool: Option<u32>) -> Result<(), u32> {
        if ctor.is_empty() {
            return Err(XC_NOCTOR);
        }
        if impact.is_empty() || alt.is_empty() {
            return Err(XC_INCOMPLETE);
        }
        if kind == LimitKind::ToolDep && min_tool.is_none() {
            return Err(XC_NOTOOLVER);
        }
        if self.items.iter().any(|l| l.ctor == ctor) {
            return Err(XC_DUP);
        }
        self.items.push(Limit { kind, ctor: ctor.to_string(), impact: impact.to_string(), alt: alt.to_string(), min_tool });
        self.ver += 1;
        Ok(())
    }

    /// 不承诺查询。
    pub fn promise(&self, ctor: &str) -> Result<(), u32> {
        if self.items.iter().any(|l| l.ctor == ctor) {
            Err(XC_NOPROMISE)
        } else {
            Ok(())
        }
    }

    /// 版本依赖查询：工具链版本低于最低要求即受限。
    pub fn tool_ok(&self, ctor: &str, tool_ver: u32) -> bool {
        match self.items.iter().find(|l| l.ctor == ctor) {
            Some(l) => match l.min_tool {
                Some(v) => tool_ver >= v,
                None => true,
            },
            None => true,
        }
    }

    pub fn count(&self, k: LimitKind) -> usize {
        self.items.iter().filter(|l| l.kind == k).count()
    }
}

pub const XC_NOCTOR: u32 = 1;
pub const XC_INCOMPLETE: u32 = 2;
pub const XC_DUP: u32 = 3;
pub const XC_NOPROMISE: u32 = 4;
pub const XC_NOTOOLVER: u32 = 5;

/// 分工边界：语义兼容域条目（归 F0457）。
pub fn semantic_domain(ctor: &str) -> bool {
    ctor.starts_with("sem-") || ctor.starts_with("interp-")
}

pub fn division_ok(book: &LimitBook) -> bool {
    book.items.iter().all(|l| !semantic_domain(&l.ctor))
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec134_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec134");

    // 族一：三类登记。
    let mut b = LimitBook::new(1);
    s.add("空构造拒", matches!(b.reg(LimitKind::NoCtor, "", "i", "a", None), Err(XC_NOCTOR)), "实名必填");
    s.add("版本依赖缺版本拒", matches!(b.reg(LimitKind::ToolDep, "wave", "i", "a", None), Err(XC_NOTOOLVER)), "版本必填");
    let _ = b.reg(LimitKind::NoCtor, "mesh-shader", "传统管线不支持", "SM6 独立路径", None);
    let _ = b.reg(LimitKind::Degrade, "wave-vote", "fxc 无波原语", "手写归约降级", None);
    let _ = b.reg(LimitKind::ToolDep, "shader-model-66", "需 dxc 新版", "升级工具链", Some(2023));
    s.add("三类各一件", b.count(LimitKind::NoCtor) == 1 && b.count(LimitKind::Degrade) == 1 && b.count(LimitKind::ToolDep) == 1, "三清单齐");
    s.add("重复拒", matches!(b.reg(LimitKind::NoCtor, "mesh-shader", "i", "a", None), Err(XC_DUP)), "一构造一登记");
    s.add("登记升版", b.ver == 4, "三次登记三升");

    // 族二：不承诺。
    s.add("受限无承诺", matches!(b.promise("mesh-shader"), Err(XC_NOPROMISE)), "缺省不放行");
    s.add("清单外可用", b.promise("texture").is_ok(), "非受限可用");

    // 族三：版本依赖语义。
    s.add("版本达标可用", b.tool_ok("shader-model-66", 2023), "2023 >= 2023");
    s.add("版本不足受限", !b.tool_ok("shader-model-66", 2022), "2022 < 2023");
    s.add("非依赖项不受版本限", b.tool_ok("mesh-shader", 2000), "None = 不限");
    s.add("未知构造不受限", b.tool_ok("other", 0), "清单外放行");

    // 族四：分工边界。
    s.add("语义域识别", semantic_domain("sem-interp") && semantic_domain("interp-flat"), "F0457 域");
    s.add("分工校验过", division_ok(&b), "无越界条目");
    let mut bad = LimitBook::new(1);
    let _ = bad.reg(LimitKind::NoCtor, "sem-flat", "i", "a", None);
    s.add("越界拦截", !division_ok(&bad), "语义域归 F0457");

    s

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn promise() {
        let mut b = LimitBook::new(1);
        let _ = b.reg(LimitKind::Degrade, "x", "i", "a", None);
        assert!(matches!(b.promise("x"), Err(XC_NOPROMISE)));
    }
}
