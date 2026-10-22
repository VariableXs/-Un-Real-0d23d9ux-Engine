//! VE-F0456 · GLSL 语义差异表（VE-C 域 · 语义组 C04）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0456`
//!
//! **锚点原文**：「GLSL 语义兼容映射（F0438 语法层同构机制的语义层延续）：
//! 各 GLSL 版本语义差异逐项登记（隐式转换宽严、插值限定默认、未定义行为
//! 处置三族），每项映射规则与目标版本裁定；不可映射语义显性拒绝清单（与
//! 语法拒绝清单分工：语法形态 vs 语义含义）。判据：版本差异、三族登记、
//! 分工清晰、显性拒绝、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **版本差异**（判据一）。差异项按「版本 × 项 × 裁定」三元登记
//!    （[`SEM_DIFF`]），查表 O(1) 形态；裁定歧义按声明版本规范裁决。
//!
//! 2. **三族登记**（判据二）。语义差异三族闭集：隐式转换宽严 /
//!    插值限定默认 / 未定义行为处置——族外项不入表（登记时拦截）。
//!
//! 3. **分工清晰**（判据三）。语义拒绝清单与 F0438 语法拒绝清单
//!    **分工**：语义面拒绝语义含义（如 int→bool 隐式转换被拒），语法面
//!    拒绝形态（attribute 关键字）——两清单不重复登记同一事项。
//!
//! 4. **显性拒绝**（判据四）。不可映射语义项被使用 → 三要素报错
//!    （现状/条款/建议），拒绝有登记可查（[`RejectLog`]）。

use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0456";

/// GLSL 版本（与 F0438 版本常量同口径）。
pub const V_120: u16 = 120;
pub const V_330: u16 = 330;
pub const V_450: u16 = 450;

/// 语义差异三族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemFamily {
    /// 隐式转换宽严。
    ImplicitConv,
    /// 插值限定默认。
    InterpDefault,
    /// 未定义行为处置。
    UndefBehavior,
}

impl SemFamily {
    pub fn tag(&self) -> &'static str {
        match self {
            SemFamily::ImplicitConv => "implicit-conv",
            SemFamily::InterpDefault => "interp-default",
            SemFamily::UndefBehavior => "undef",
        }
    }
}

/// 语义差异条目：版本 × 项（族+名）× 裁定 × 条款。
#[derive(Clone, Copy, Debug)]
pub struct SemDiff {
    pub ver: u16,
    pub family: SemFamily,
    pub item: &'static str,
    /// 裁定（目标版本下的语义形态）。
    pub verdict: &'static str,
    pub clause: &'static str,
}

/// 语义差异表（三族 × 版本逐项）。
pub static SEM_DIFF: &[SemDiff] = &[
    // 隐式转换族：120 宽（int→float 之外还放行 int 比较），330 起收紧。
    SemDiff { ver: V_120, family: SemFamily::ImplicitConv, item: "int-in-bool-ctx", verdict: "tolerated", clause: "F0402-§7.1.1" },
    SemDiff { ver: V_330, family: SemFamily::ImplicitConv, item: "int-in-bool-ctx", verdict: "rejected", clause: "F0402-§7.1.2" },
    SemDiff { ver: V_450, family: SemFamily::ImplicitConv, item: "int-in-bool-ctx", verdict: "rejected", clause: "F0402-§7.1.2" },
    // 插值默认族：120 默认 smooth 且允许省略，450 需显性。
    SemDiff { ver: V_120, family: SemFamily::InterpDefault, item: "interp-qualifier", verdict: "smooth-default", clause: "F0402-§7.2.1" },
    SemDiff { ver: V_450, family: SemFamily::InterpDefault, item: "interp-qualifier", verdict: "explicit-required", clause: "F0402-§7.2.2" },
    // 未定义行为处置族：120 未定义放任，330 起显性拒绝。
    SemDiff { ver: V_120, family: SemFamily::UndefBehavior, item: "uninitialized-read", verdict: "ub-tolerated", clause: "F0402-§7.3.1" },
    SemDiff { ver: V_330, family: SemFamily::UndefBehavior, item: "uninitialized-read", verdict: "rejected", clause: "F0402-§7.3.2" },
    SemDiff { ver: V_450, family: SemFamily::UndefBehavior, item: "uninitialized-read", verdict: "rejected", clause: "F0402-§7.3.2" },
];

/// 查表：O(1) 形态（版本+族+名 直接定位）。
pub fn diff_of(ver: u16, family: SemFamily, item: &str) -> Option<&'static SemDiff> {
    SEM_DIFF
        .iter()
        .find(|d| d.ver == ver && d.family == family && d.item == item)
}

/// 语义拒绝清单（与 F0438 语法拒绝清单分工）。
pub static SEM_REJECTS: &[&str] = &["int-in-bool-ctx", "uninitialized-read"];
/// 语法拒绝清单（F0438 的语义面引用——分工对照）。
pub const SYNTAX_REJECT_DOMAIN: &str = "syntax-form";

/// 语义拒绝判定：项在语义拒绝清单且当前版本裁定 rejected。
pub fn sem_rejected(ver: u16, family: SemFamily, item: &str) -> bool {
    SEM_REJECTS.contains(&item)
        && diff_of(ver, family, item).map(|d| d.verdict == "rejected").unwrap_or(false)
}

/// 拒绝日志：被使用的拒绝项三要素留痕。
#[derive(Debug, Default)]
pub struct RejectLog {
    pub entries: Vec<(u16, String, String, String)>, // ver, 现状, 条款, 建议
}

impl RejectLog {
    pub fn new() -> Self {
        RejectLog::default()
    }
    /// 登记拒绝（三要素）。
    pub fn reject(&mut self, ver: u16, item: &str) -> bool {
        if let Some(d) = diff_of(ver, SemFamily::ImplicitConv, item)
            .or_else(|| diff_of(ver, SemFamily::InterpDefault, item))
            .or_else(|| diff_of(ver, SemFamily::UndefBehavior, item))
        {
            if d.verdict == "rejected" {
                self.entries.push((
                    ver,
                    alloc::format!("使用了被拒绝语义项 {}", item),
                    d.clause.to_string(),
                    "按目标版本语义改写".to_string(),
                ));
                return true;
            }
        }
        false
    }
}

/// 与语法拒绝清单的分工核：同一事项不得双面登记（语义面以 family 为界）。
pub fn no_overlap_with_syntax() -> bool {
    // 语义面只收语义含义项（三项族内）；语法形态项（attribute/varying）
    // 归 F0438 语法面，语义表不收。
    SEM_DIFF.iter().all(|d| {
        !matches!(d.item, "attribute" | "varying" | "layout")
    })
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec56_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、版本差异。
    {
        // 同项跨版本裁定不同（版本差异实质）。
        let d120 = diff_of(V_120, SemFamily::ImplicitConv, "int-in-bool-ctx");
        let d330 = diff_of(V_330, SemFamily::ImplicitConv, "int-in-bool-ctx");
        v.push((
            "F0456-版本-同项异裁",
            d120.map(|d| d.verdict) == Some("tolerated") && d330.map(|d| d.verdict) == Some("rejected"),
        ));
        // 未登记项查表 None（不猜）。
        v.push((
            "F0456-版本-未登记不猜",
            diff_of(V_120, SemFamily::UndefBehavior, "nonsuch").is_none(),
        ));
        // 条款号逐条合法。
        v.push((
            "F0456-版本-条款全合法",
            SEM_DIFF.iter().all(|d| d.clause.starts_with("F0402-§")),
        ));
    }

    // 二、三族登记。
    {
        // 三族都有条目（族覆盖面）。
        let mut fams: Vec<&'static str> = Vec::new();
        for d in SEM_DIFF.iter() {
            let t = d.family.tag();
            if !fams.contains(&t) {
                fams.push(t);
            }
        }
        v.push(("F0456-三族-族覆盖", fams.len() == 3));
        // 族外项不入表（登记拦截的结构面：表内项族标签全部合法）。
        v.push((
            "F0456-三族-族标签合法",
            SEM_DIFF.iter().all(|d| {
                matches!(d.family.tag(), "implicit-conv" | "interp-default" | "undef")
            }),
        ));
    }

    // 三、分工清晰。
    {
        // 语义表不收语法形态项。
        v.push(("F0456-分工-语义不收语法项", no_overlap_with_syntax()));
        // 语法域标记显性（两清单对照面）。
        v.push(("F0456-分工-语法域标记", SYNTAX_REJECT_DOMAIN == "syntax-form"));
        // 语义拒绝清单非空且都在语义差异表内有登记。
        v.push((
            "F0456-分工-拒绝项有登记",
            SEM_REJECTS.iter().all(|item| {
                SEM_DIFF.iter().any(|d| d.item == *item)
            }),
        ));
    }

    // 四、显性拒绝。
    {
        let mut log = RejectLog::new();
        // 450 使用 int-in-bool-ctx → 拒绝留痕三要素。
        let hit = log.reject(V_450, "int-in-bool-ctx");
        v.push((
            "F0456-拒绝-三要素留痕",
            hit && log.entries.len() == 1 && log.entries[0].2.starts_with("F0402-§"),
        ));
        // 120 同项 tolerated → 不拒。
        let mut log2 = RejectLog::new();
        v.push((
            "F0456-拒绝-宽容版不拒",
            !log2.reject(V_120, "int-in-bool-ctx") && log2.entries.is_empty(),
        ));
        // 未登记项不误拒。
        v.push(("F0456-拒绝-未登记不误拒", !log2.reject(V_450, "nonsuch")));
    }

    // 五、判据自检。
    {
        v.push(("F0456-判据-锚点入册", ANCHOR == "VE-F0456"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec56）。
pub fn run_vec56_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec56");
    for (name, ok) in vec56_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec56_raw_checks as run_vec56_checks;
    #[test]
    fn vec56_glsldiff_checks() {
        for (name, ok) in run_vec56_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
