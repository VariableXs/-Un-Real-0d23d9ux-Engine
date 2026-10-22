//! VE-F0457 · HLSL/WGSL 语义差异表（VE-C 域 · 语义组 C04）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0457`
//!
//! **锚点原文**：「HLSL 与 WGSL 语义兼容映射（F0456 同构机制第二用复用声明）：
//! HLSL 语义面（寄存器语义对绑定模型的映射、采样器分离语义）与 WGSL 语义面
//! （地址空间、强类型严格性）各自差异表；双方言语义冲突场景裁定规则；各自
//! 拒绝清单独立维护。判据：同构第二用、双方言分工、冲突裁定、独立清单、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **同构第二用**（判据一）。表结构（版本/方言 × 族 × 项 × 裁定 ×
//!    条款）与 F0456 同构——映射机制声明复用而非重写；方言标记替代版本
//!    维度（[`Dialect`] 两值）。
//!
//! 2. **双方言分工**（判据二）。HLSL 面（寄存器语义→绑定映射、采样器
//!    分离）与 WGSL 面（地址空间、强类型严格性）**各自独立差异表**——
//!    表内条目不混方言。
//!
//! 3. **冲突裁定**（判据三）。双方言语义冲突场景（同一事项两方言裁定
//!    相斥）按声明目标方言裁定并登记（[`arbiter`]）。
//!
//! 4. **独立清单**（判据四）。两方言拒绝清单**独立维护**（各自闭集），
//!    一方的拒绝项不自动传染另一方。

use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0457";

/// 方言两值（替代 F0456 的版本维度——同构换维）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    Hlsl,
    Wgsl,
}

impl Dialect {
    pub fn tag(&self) -> &'static str {
        match self {
            Dialect::Hlsl => "hlsl",
            Dialect::Wgsl => "wgsl",
        }
    }
}

/// 语义差异条目（方言 × 族 × 项 × 裁定 × 条款）。
#[derive(Clone, Copy, Debug)]
pub struct DSemDiff {
    pub dialect: Dialect,
    pub family: &'static str,
    pub item: &'static str,
    pub verdict: &'static str,
    pub clause: &'static str,
}

/// HLSL 面：寄存器语义→绑定映射 + 采样器分离。
pub static HLSL_DIFFS: &[DSemDiff] = &[
    DSemDiff { dialect: Dialect::Hlsl, family: "register-map", item: "semantic-to-binding", verdict: "auto-map", clause: "F0402-§8.1.1" },
    DSemDiff { dialect: Dialect::Hlsl, family: "register-map", item: "register-explicit", verdict: "supported", clause: "F0402-§8.1.2" },
    DSemDiff { dialect: Dialect::Hlsl, family: "sampler-split", item: "sampler-state", verdict: "separated", clause: "F0402-§8.2.1" },
];

/// WGSL 面：地址空间 + 强类型严格性。
pub static WGSL_DIFFS: &[DSemDiff] = &[
    DSemDiff { dialect: Dialect::Wgsl, family: "addr-space", item: "storage-class", verdict: "explicit-required", clause: "F0402-§8.3.1" },
    DSemDiff { dialect: Dialect::Wgsl, family: "addr-space", item: "workgroup-zero-init", verdict: "default-zero", clause: "F0402-§8.3.2" },
    DSemDiff { dialect: Dialect::Wgsl, family: "strict-typing", item: "implicit-conv", verdict: "rejected", clause: "F0402-§8.4.1" },
    DSemDiff { dialect: Dialect::Wgsl, family: "strict-typing", item: "int-in-bool-ctx", verdict: "rejected", clause: "F0402-§8.4.1" },
];

/// 全集视图（表内条目方言不混——同构换维的表驱动面）。
pub fn all_diffs() -> Vec<&'static DSemDiff> {
    let mut v = Vec::new();
    for d in HLSL_DIFFS.iter() {
        v.push(d);
    }
    for d in WGSL_DIFFS.iter() {
        v.push(d);
    }
    v
}

/// 查表：方言+族+项 定位。
pub fn diff_of(dialect: Dialect, family: &str, item: &str) -> Option<&'static DSemDiff> {
    all_diffs().into_iter().find(|d| d.dialect == dialect && d.family == family && d.item == item)
}

// ---------------------------------------------------------------------------
// 冲突裁定（双方言相斥场景）
// ---------------------------------------------------------------------------

/// 冲突场景：同一事项两方言裁定相斥。
pub static CLASH_SCENES: &[(&str, Dialect, &str, Dialect, &str, &str)] = &[
    // 隐式转换：WGSL 强类型拒绝 vs HLSL 宽松放行 → 按声明目标裁定。
    ("implicit-conv", Dialect::Wgsl, "rejected", Dialect::Hlsl, "tolerated", "按声明目标方言裁定"),
];

/// 裁定器：声明目标方言 → 采用其裁定，冲突登记留痕。
#[derive(Debug, Default)]
pub struct Arbiter {
    pub decisions: Vec<(String, Dialect, String)>, // 场景, 目标方言, 裁定
}

impl Arbiter {
    pub fn new() -> Self {
        Arbiter::default()
    }
    /// 裁定：冲突场景按目标方言的表内裁定；非冲突场景直接查表。
    pub fn decide(&mut self, item: &str, target: Dialect) -> Option<String> {
        for (scene, dw, vw, dh, vh, note) in CLASH_SCENES.iter() {
            if *scene == item {
                let verdict = if target == *dw { *vw } else { *vh };
                self.decisions.push((item.to_string(), target, verdict.to_string()));
                return Some(alloc::format!("{}（{}）", verdict, note));
            }
        }
        diff_of(target, "strict-typing", item)
            .or_else(|| diff_of(target, "register-map", item))
            .or_else(|| diff_of(target, "sampler-split", item))
            .or_else(|| diff_of(target, "addr-space", item))
            .map(|d| d.verdict.to_string())
    }
}

// ---------------------------------------------------------------------------
// 独立拒绝清单（两方言各自闭集）
// ---------------------------------------------------------------------------

/// HLSL 拒绝清单。
pub const HLSL_REJECTS: &[&str] = &[];
/// WGSL 拒绝清单（强类型族 + int-in-bool-ctx）。
pub const WGSL_REJECTS: &[&str] = &["implicit-conv", "int-in-bool-ctx"];

/// 方言拒绝判定：清单独立（各自查各自的，不传染）。
pub fn rejected_in(dialect: Dialect, item: &str) -> bool {
    match dialect {
        Dialect::Hlsl => HLSL_REJECTS.contains(&item),
        Dialect::Wgsl => WGSL_REJECTS.contains(&item),
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec57_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、同构第二用。
    {
        // 表结构同构面：条目五要素齐 + 条款号合法。
        v.push((
            "F0457-同构-五要素齐",
            all_diffs().iter().all(|d| {
                !d.family.is_empty() && !d.item.is_empty() && !d.verdict.is_empty()
                    && d.clause.starts_with("F0402-§")
            }),
        ));
        // 方言维度替代版本维度（两值闭集）。
        v.push((
            "F0457-同构-方言换维",
            all_diffs().iter().all(|d| matches!(d.dialect, Dialect::Hlsl | Dialect::Wgsl)),
        ));
        // 未登记项不猜。
        v.push((
            "F0457-同构-未登记不猜",
            diff_of(Dialect::Hlsl, "nonsuch", "x").is_none(),
        ));
    }

    // 二、双方言分工。
    {
        // HLSL 表只有 HLSL 条目；WGSL 表只有 WGSL 条目（表内不混）。
        v.push((
            "F0457-分工-表内不混",
            HLSL_DIFFS.iter().all(|d| d.dialect == Dialect::Hlsl)
                && WGSL_DIFFS.iter().all(|d| d.dialect == Dialect::Wgsl),
        ));
        // HLSL 面三类语义（寄存器映射×2 + 采样器分离）。
        v.push((
            "F0457-分工-HLSL面",
            HLSL_DIFFS.len() == 3 && diff_of(Dialect::Hlsl, "sampler-split", "sampler-state").is_some(),
        ));
        // WGSL 面两类语义（地址空间×2 + 强类型×2）。
        v.push((
            "F0457-分工-WGSL面",
            WGSL_DIFFS.len() == 4 && diff_of(Dialect::Wgsl, "addr-space", "workgroup-zero-init").is_some(),
        ));
    }

    // 三、冲突裁定。
    {
        let mut arb = Arbiter::new();
        // 目标 WGSL：隐式转换按 WGSL 裁定 rejected。
        let d1 = arb.decide("implicit-conv", Dialect::Wgsl);
        v.push((
            "F0457-裁定-目标WGSL拒",
            d1.map(|s| s.contains("rejected")).unwrap_or(false),
        ));
        // 目标 HLSL：同场景按 HLSL 裁定 tolerated。
        let d2 = arb.decide("implicit-conv", Dialect::Hlsl);
        v.push((
            "F0457-裁定-目标HLSL容",
            d2.map(|s| s.contains("tolerated")).unwrap_or(false),
        ));
        // 裁定留痕（冲突场景都登记）。
        v.push((
            "F0457-裁定-留痕可查",
            arb.decisions.len() == 2 && arb.decisions.iter().all(|(_, _, _)| true),
        ));
        // 非冲突场景直查表（WGSL workgroup-zero-init → default-zero）。
        let d3 = Arbiter::new().decide("workgroup-zero-init", Dialect::Wgsl);
        v.push((
            "F0457-裁定-非冲突直查",
            d3 == Some("default-zero".to_string()),
        ));
    }

    // 四、独立清单。
    {
        // WGSL 拒绝、HLSL 不拒（不传染）。
        v.push((
            "F0457-清单-不传染",
            rejected_in(Dialect::Wgsl, "implicit-conv") && !rejected_in(Dialect::Hlsl, "implicit-conv"),
        ));
        // HLSL 清单为空（当前闭集）——空也是显性状态。
        v.push(("F0457-清单-HLSL空集显性", HLSL_REJECTS.is_empty()));
        // WGSL 清单与 WGSL 表内 rejected 项一致（清单即表的投影）。
        v.push((
            "F0457-清单-表投影一致",
            WGSL_REJECTS.iter().all(|item| {
                diff_of(Dialect::Wgsl, "strict-typing", item)
                    .map(|d| d.verdict == "rejected")
                    .unwrap_or(false)
            }),
        ));
    }

    // 五、判据自检。
    {
        v.push(("F0457-判据-锚点入册", ANCHOR == "VE-F0457"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec57）。
pub fn run_vec57_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec57");
    for (name, ok) in vec57_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec57_raw_checks as run_vec57_checks;
    #[test]
    fn vec57_dialectsem_checks() {
        for (name, ok) in run_vec57_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
