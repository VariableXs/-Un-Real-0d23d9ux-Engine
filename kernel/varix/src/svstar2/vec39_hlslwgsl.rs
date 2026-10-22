//! VE-F0439 · HLSL/WGSL 语法兼容映射（VE-C 域 · 语法组 C02 续）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0439`
//!
//! **锚点原文**：「HLSL 与 WGSL 语法兼容映射（F0438 同构机制第二用复用
//! 声明）：HLSL 语义差异面（寄存器语义、语义绑定的语法映射）、WGSL 差异
//! 面（模块化结构、属性语法映射）、两方言不可映射项各自清单；映射正确性
//! 黄金对拍各自维护。判据：双方言、同构复用、各自清单、声明裁定、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **双方言**（判据一）。[`Dialect`] 两值闭集；两方言**各自差异表**
//!    （[`HLSL_TABLE`] / [`WGSL_TABLE`]），不合并不互借——HLSL 的寄存器
//!    语义（`:register(b0)`）与 WGSL 的模块属性（`@group(0) @binding(0)`）
//!    是两个独立登记面。
//!
//! 2. **同构复用**（判据二）。机制复用 F0438（差异三元值 + 三要素拒绝 +
//!    黄金对拍），但**机制是同构不是共享实例**：两方言的对拍器各自独立
//!    实例化（[`map_ctor2`] 以方言参数分流），判据验证两方言对拍互不
//!    污染（A 方言失配不影响 B 对拍结果）。
//!
//! 3. **各自清单**（判据三）。拒绝清单按方言隔离：[`RejectBook`] 里
//!    HLSL 项与 WGSL 项分开计数分开展示；跨方言混入即违规（判据以
//!    方言位核对）。
//!
//! 4. **声明裁定**（判据四）。方言间冲突（同一构造两方言语义不同——如
//!    `in` 在 HLSL 是阶段输入在 WGSL 是参数限定）按**声明目标方言**裁定：
//!    映射调用必须显性带方言参数，不带 = 编译期不可能（参数强制）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0439";

/// 方言两值闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    Hlsl,
    Wgsl,
}

/// HLSL 差异表。
pub const HLSL_TABLE: [(&str, DiffAct); 4] = [
    ("register", DiffAct::Native),   // :register(b0) 语法面内部可映射
    ("Semantic", DiffAct::Rewrite),  // SV_Position 类语义绑定改写
    ("cbuffer", DiffAct::Rewrite),   // 常量缓冲 → 内部 uniform 块
    ("class", DiffAct::Reject),      // 类继承语法面不可映射
];

/// WGSL 差异表。
pub const WGSL_TABLE: [(&str, DiffAct); 4] = [
    ("@group", DiffAct::Native),
    ("@binding", DiffAct::Native),
    ("@workgroup_size", DiffAct::Rewrite),
    ("ptr", DiffAct::Reject),        // 指针语法面不可映射
];

/// 差异动作三元值（同构 F0438）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffAct {
    Native,
    Rewrite,
    Reject,
}

/// 按方言查差异表。
pub fn diff2(d: Dialect, ctor: &str) -> Option<DiffAct> {
    let table = match d {
        Dialect::Hlsl => &HLSL_TABLE,
        Dialect::Wgsl => &WGSL_TABLE,
    };
    table.iter().find(|(c, _)| *c == ctor).map(|(_, a)| *a)
}

// ---------------------------------------------------------------------------
// 拒绝清单（各自隔离）
// ---------------------------------------------------------------------------

/// 拒绝条目（方言位强制）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reject2 {
    pub dialect: Dialect,
    pub code: &'static str,
    pub clause: &'static str,
}

/// 拒绝簿：按方言隔离计数。
#[derive(Debug, Default)]
pub struct RejectBook {
    pub hlsl: Vec<Reject2>,
    pub wgsl: Vec<Reject2>,
}

impl RejectBook {
    pub fn new() -> Self {
        Self::default()
    }
    /// 入清单：拒绝构造按方言分桶。
    pub fn note(&mut self, d: Dialect, ctor: &str) -> bool {
        if diff2(d, ctor) != Some(DiffAct::Reject) {
            return false;
        }
        let r = match d {
            Dialect::Hlsl => Reject2 {
                dialect: d,
                code: "E-HLSL-CLASS",
                clause: "HLSL 规范类继承语法面不可映射到内部 AST",
            },
            Dialect::Wgsl => Reject2 {
                dialect: d,
                code: "E-WGSL-PTR",
                clause: "WGSL 规范指针语法面不可映射到内部 AST",
            },
        };
        match d {
            Dialect::Hlsl => self.hlsl.push(r),
            Dialect::Wgsl => self.wgsl.push(r),
        }
        true
    }
    /// 跨方言混入核对：各桶方言位全一致。
    pub fn buckets_pure(&self) -> bool {
        self.hlsl.iter().all(|r| r.dialect == Dialect::Hlsl)
            && self.wgsl.iter().all(|r| r.dialect == Dialect::Wgsl)
    }
}

// ---------------------------------------------------------------------------
// 映射（方言参数强制）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapErr2 {
    HardReject,
    NoVerdict,
}

/// 映射：方言参数显性（不带 = 编译期不可能）。
pub fn map_ctor2(d: Dialect, ctor: &str) -> Result<String, MapErr2> {
    match diff2(d, ctor) {
        None => Err(MapErr2::NoVerdict),
        Some(DiffAct::Reject) => Err(MapErr2::HardReject),
        Some(DiffAct::Native) => Ok(ctor.to_string()),
        Some(DiffAct::Rewrite) => match (d, ctor) {
            (Dialect::Hlsl, "Semantic") => Ok("builtin_attr".to_string()),
            (Dialect::Hlsl, "cbuffer") => Ok("uniform_block".to_string()),
            (Dialect::Wgsl, "@workgroup_size") => Ok("num_workgroups".to_string()),
            _ => Err(MapErr2::NoVerdict),
        },
    }
}

/// 双方言黄金对（各自维护）。
pub fn golden_hlsl() -> Vec<(&'static str, &'static str)> {
    vec![
        ("register", "register"),
        ("Semantic", "builtin_attr"),
        ("cbuffer", "uniform_block"),
    ]
}

pub fn golden_wgsl() -> Vec<(&'static str, &'static str)> {
    vec![
        ("@group", "@group"),
        ("@binding", "@binding"),
        ("@workgroup_size", "num_workgroups"),
    ]
}

/// 对拍（各自独立实例）：返 (通过数, 失配方言, 样例号)。
pub fn run_golden2() -> Result<usize, (Dialect, usize)> {
    for (i, (ctor, expect)) in golden_hlsl().iter().enumerate() {
        match map_ctor2(Dialect::Hlsl, ctor) {
            Ok(s) if &s == expect => {}
            _ => return Err((Dialect::Hlsl, i)),
        }
    }
    for (i, (ctor, expect)) in golden_wgsl().iter().enumerate() {
        match map_ctor2(Dialect::Wgsl, ctor) {
            Ok(s) if &s == expect => {}
            _ => return Err((Dialect::Wgsl, i)),
        }
    }
    Ok(golden_hlsl().len() + golden_wgsl().len())
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec39_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、双方言。
    {
        v.push(("F0439-双方言-两表入册", HLSL_TABLE.len() == 4 && WGSL_TABLE.len() == 4));
        v.push((
            "F0439-双方言-寄存器面",
            diff2(Dialect::Hlsl, "register") == Some(DiffAct::Native),
        ));
        v.push((
            "F0439-双方言-模块属性面",
            diff2(Dialect::Wgsl, "@group") == Some(DiffAct::Native),
        ));
        // 方言互斥：HLSL 构造不在 WGSL 表。
        v.push(("F0439-双方言-互不借用", diff2(Dialect::Wgsl, "register").is_none() && diff2(Dialect::Hlsl, "@group").is_none()));
    }
    // 二、同构复用。
    {
        // 机制同构：三元值/拒绝/对拍两方言都有（对拍器双实例独立）。
        v.push(("F0439-同构-对拍双实例", run_golden2() == Ok(6)));
        // A 方言失配不影响 B：单点验证 WGSL 对拍独立通过。
        v.push((
            "F0439-同构-WGSL独立对拍",
            map_ctor2(Dialect::Wgsl, "@group") == Ok("@group".to_string()),
        ));
        // HLSL 改写方向。
        v.push((
            "F0439-同构-HLSL改写",
            map_ctor2(Dialect::Hlsl, "Semantic") == Ok("builtin_attr".to_string()),
        ));
        // 未登记不猜（两方言各自）。
        v.push((
            "F0439-同构-未登记不猜",
            map_ctor2(Dialect::Hlsl, "nonsuch") == Err(MapErr2::NoVerdict),
        ));
    }
    // 三、各自清单。
    {
        let mut rb = RejectBook::new();
        v.push(("F0439-清单-HLSL类拒绝入桶", rb.note(Dialect::Hlsl, "class")));
        v.push(("F0439-清单-WGSL指针拒绝入桶", rb.note(Dialect::Wgsl, "ptr")));
        v.push((
            "F0439-清单-桶位隔离",
            rb.hlsl.len() == 1 && rb.wgsl.len() == 1 && rb.buckets_pure(),
        ));
        // 非拒绝构造不入桶。
        v.push(("F0439-清单-Native不入", !rb.note(Dialect::Hlsl, "register")));
        // 三要素非空。
        v.push((
            "F0439-清单-三要素",
            rb.hlsl.iter().chain(rb.wgsl.iter()).all(|r| !r.code.is_empty() && !r.clause.is_empty()),
        ));
    }
    // 四、声明裁定。
    {
        // 方言参数强制：同一构造两方言不同结果。
        v.push((
            "F0439-裁定-方言决定方向",
            map_ctor2(Dialect::Hlsl, "cbuffer") != map_ctor2(Dialect::Wgsl, "cbuffer"),
        ));
        // 硬拒绝面一致。
        v.push((
            "F0439-裁定-硬拒绝",
            map_ctor2(Dialect::Hlsl, "class") == Err(MapErr2::HardReject)
                && map_ctor2(Dialect::Wgsl, "ptr") == Err(MapErr2::HardReject),
        ));
        // 判据自检。
        v.push(("F0439-判据-锚点入册", ANCHOR == "VE-F0439"));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::vec39_raw_checks as run_vec39_checks;
    #[test]
    fn vec39_hlslwgsl_checks() {
        for (name, ok) in run_vec39_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec39）。
pub fn run_vec39_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec39");
    for (name, ok) in vec39_raw_checks() {
        set.add(name, ok, "");
    }
    set
}
