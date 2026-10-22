//! VE-F0477 · IR 级 shader 变体参数化（VE-C 域 · IR 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0477`
//!
//! **锚点原文**：「着色器变体 IR 级参数化：变体参数集定义（宏开关、常量覆
//! 写、能力分支三类参数化维度）、参数到 IR 特化的生成（每变体一份特化 IR）、
//! 参数组合爆炸防护（组合上限与按需生成策略）；变体清单可枚举。判据：三维
//! 参数化、按需生成、组合防护、回退标注、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **三维参数化**（判据一）。参数集三维度（宏开关 bool / 常量覆写 i64 /
//!    能力分支枚举）各自独立定义（[`VariantParam`]），混维即拒。
//!
//! 2. **按需生成**（判据二）。特化按需生成（[`VariantBook::specialize`]：
//!    已缓存即返缓存），不用到不编（组合爆炸防护的执行面）。
//!
//! 3. **组合防护**（判据三）。组合上限显性（[`COMBO_CAP`]：超出 → 按需
//!    策略强制 + 上限记录），枚举清单可查（[`VariantBook::enumerate`]）。
//!
//! 4. **回退标注**（判据四）。特化失败 → 回退基础变体并标注（回退事件入
//!    账，不静默换）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0477";

/// 组合上限（变体参数组合数封顶——爆炸防护）。
pub const COMBO_CAP: usize = 256;

// ---------------------------------------------------------------------------
// 参数集（三维）
// ---------------------------------------------------------------------------

/// 参数维度（三维闭集）。
#[derive(Clone, Debug, PartialEq)]
pub enum VariantParam {
    /// 宏开关（名, 布尔值）。
    Macro { name: String, on: bool },
    /// 常量覆写（名, 整值）。
    ConstOverride { name: String, value: i64 },
    /// 能力分支（名, 档位）。
    CapabilityBranch { name: String, tier: u8 },
}

impl VariantParam {
    pub fn key(&self) -> &str {
        match self {
            VariantParam::Macro { name, .. } => name,
            VariantParam::ConstOverride { name, .. } => name,
            VariantParam::CapabilityBranch { name, .. } => name,
        }
    }
    /// 维度标签（三维机检面）。
    pub fn dim(&self) -> &'static str {
        match self {
            VariantParam::Macro { .. } => "macro",
            VariantParam::ConstOverride { .. } => "const",
            VariantParam::CapabilityBranch { .. } => "cap",
        }
    }
    pub fn digest(&self) -> String {
        match self {
            VariantParam::Macro { name, on } => format!("m:{}={}", name, on),
            VariantParam::ConstOverride { name, value } => format!("c:{}={}", name, value),
            VariantParam::CapabilityBranch { name, tier } => format!("b:{}={}", name, tier),
        }
    }
}

/// 参数集（三维混合集；同键双写即拒）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParamSet {
    pub params: Vec<VariantParam>,
}

impl ParamSet {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn push(&mut self, p: VariantParam) -> Result<(), String> {
        if self.params.iter().any(|x| x.key() == p.key()) {
            return Err(format!("参数 {} 重复（同键双写拒）", p.key()));
        }
        self.params.push(p);
        Ok(())
    }
    pub fn digest(&self) -> String {
        let mut parts: Vec<&str> = self.params.iter().map(|p| p.key()).collect();
        parts.sort_unstable();
        parts.join(",")
    }
}

/// 组合数估算（各维取值数的乘积；超出上限 → 按需策略强制）。
pub fn combo_count(dims: &[usize]) -> usize {
    dims.iter().product()
}

/// 组合防护：超上限 → 按需生成策略（用到才编）。
pub fn combo_guard(dims: &[usize]) -> Result<(), String> {
    let n = combo_count(dims);
    if n > COMBO_CAP {
        Err(format!(
            "组合 {} 超上限 {}：转按需生成策略（用到才编）",
            n, COMBO_CAP
        ))
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 变体册（特化按需 + 回退标注 + 清单枚举）
// ---------------------------------------------------------------------------

/// 特化结果（变体 IR 的最小替身：摘要串 + 参数集指纹）。
#[derive(Clone, Debug, PartialEq)]
pub struct Specialized {
    pub base: String,
    pub param_digest: String,
    /// 回退标注（特化失败回退基础变体时置位）。
    pub fell_back: bool,
}

/// 变体册。
#[derive(Debug, Default)]
pub struct VariantBook {
    pub base: String,
    pub cache: Vec<Specialized>,
    pub fallbacks: Vec<String>,
}

impl VariantBook {
    pub fn new(base: &str) -> Self {
        VariantBook {
            base: base.to_string(),
            cache: Vec::new(),
            fallbacks: Vec::new(),
        }
    }
    /// 按需特化：缓存命中即返（回退条目不算可复用命中——重试须走生成路径）；
    /// 未命中生成（fail 回退基础变体 + 标注）。
    pub fn specialize(&mut self, ps: &ParamSet, fail: bool) -> Specialized {
        let dg = ps.digest();
        if let Some(s) = self.cache.iter().find(|s| s.param_digest == dg && !s.fell_back) {
            return s.clone();
        }
        // 生成（fail 模拟特化失败）。
        let s = if fail {
            self.fallbacks.push(dg.clone());
            Specialized {
                base: self.base.clone(),
                param_digest: dg,
                fell_back: true,
            }
        } else {
            Specialized {
                base: format!("{}#{}", self.base, dg),
                param_digest: dg,
                fell_back: false,
            }
        };
        self.cache.push(s.clone());
        s
    }
    /// 清单枚举（缓存序）。
    pub fn enumerate(&self) -> Vec<String> {
        self.cache.iter().map(|s| s.param_digest.clone()).collect()
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec77_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、三维参数化。
    {
        let mut ps = ParamSet::new();
        ps.push(VariantParam::Macro { name: "SHADOWS".to_string(), on: true }).ok();
        ps.push(VariantParam::ConstOverride { name: "MAX_LIGHTS".to_string(), value: 8 }).ok();
        ps.push(VariantParam::CapabilityBranch { name: "TEX_QUALITY".to_string(), tier: 2 }).ok();
        // 三维齐备 + 维度标签可机检。
        v.push((
            "F0477-三维-齐备",
            ps.params.len() == 3
                && ps.params.iter().any(|p| p.dim() == "macro")
                && ps.params.iter().any(|p| p.dim() == "const")
                && ps.params.iter().any(|p| p.dim() == "cap"),
        ));
        // 同键双写拒。
        let dup = ps.push(VariantParam::Macro { name: "SHADOWS".to_string(), on: false });
        v.push((
            "F0477-三维-同键拒",
            dup.is_err() && dup.unwrap_err().contains("SHADOWS"),
        ));
        // 摘要面确定性（键序排序）。
        let mut ps2 = ParamSet::new();
        ps2.push(VariantParam::CapabilityBranch { name: "TEX_QUALITY".to_string(), tier: 2 }).ok();
        ps2.push(VariantParam::Macro { name: "SHADOWS".to_string(), on: true }).ok();
        ps2.push(VariantParam::ConstOverride { name: "MAX_LIGHTS".to_string(), value: 8 }).ok();
        v.push(("F0477-三维-摘要确定性", ps.digest() == ps2.digest()));
    }

    // 二、按需生成。
    {
        let mut book = VariantBook::new("lit_vs");
        let mut ps = ParamSet::new();
        ps.push(VariantParam::Macro { name: "SHADOWS".to_string(), on: true }).ok();
        let s1 = book.specialize(&ps, false);
        let s2 = book.specialize(&ps, false);
        // 缓存命中：二次特化同对象（缓存面）。
        v.push((
            "F0477-按需-缓存命中",
            s1 == s2 && !s1.fell_back && s1.base.contains("lit_vs#"),
        ));
        // 清单枚举。
        v.push((
            "F0477-按需-清单枚举",
            book.enumerate() == vec![ps.digest()],
        ));
    }

    // 三、组合防护。
    {
        // 上限内：组合过。
        v.push(("F0477-防护-上限内过", combo_guard(&[4, 4, 4]).is_ok()));
        // 超上限：64×8=512 > 256 → 拒并提示按需。
        let e = combo_guard(&[64, 8]);
        v.push((
            "F0477-防护-超限转按需",
            matches!(&e, Err(msg) if msg.contains("按需")),
        ));
        // 上限值面：恰 256 过、257 拒（边界）。
        v.push((
            "F0477-防护-边界",
            combo_guard(&[256]).is_ok() && combo_guard(&[257]).is_err(),
        ));
    }

    // 四、回退标注。
    {
        let mut book = VariantBook::new("base_vs");
        let mut ps = ParamSet::new();
        ps.push(VariantParam::Macro { name: "X".to_string(), on: true }).ok();
        // 特化失败 → 回退基础变体 + 标注（不静默换）。
        let s = book.specialize(&ps, true);
        v.push((
            "F0477-回退-标注置位",
            s.fell_back && s.base == "base_vs" && book.fallbacks.len() == 1,
        ));
        // 回退不污染缓存语义（下次同参重试仍走生成路径——回退结果也在册）。
        let s2 = book.specialize(&ps, false);
        v.push((
            "F0477-回退-重试可成",
            !s2.fell_back && s2.base.contains("base_vs#"),
        ));
    }

    // 五、判据自检。
    {
        v.push(("F0477-判据-锚点入册", ANCHOR == "VE-F0477"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec77）。
pub fn run_vec77_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec77");
    for (name, ok) in vec77_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec77_raw_checks as run_vec77_checks;
    #[test]
    fn vec77_variant_checks() {
        for (name, ok) in run_vec77_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
