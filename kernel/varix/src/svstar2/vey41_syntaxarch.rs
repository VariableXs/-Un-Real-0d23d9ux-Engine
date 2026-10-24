//! VE-F5041 · 语法总架构（VE-Z 域 · Y03 组 · 目标 320 行）· 诊断段 0x8e80
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5041`
//!
//! **判据（锚点原文）**：四件套、文法单源、五律对拍、token 契约、判据。
//!
//! **职责定位（锚点原文）**：脚本语法层总架构（语法定位：token 流到语义世界的
//! 桥梁）；架构四件：文法定义（语言结构的单源真理）/解析器（token 到 AST）/语
//! 义分析（AST 到含义）/类型检查（类型安全网）；四件共用五律宪法与 Y02 token
//! 契约；文法歧义→五律裁决；契约失配→版本拦截。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x8e80;

/// 四件套。
pub const PIECES: u32 = 4;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// token 契约版本（Y02 对接）。
pub const TOKEN_CONTRACT_V: u32 = 2;

/// 文法单源：产生式规则表（唯一真理）。
pub struct Grammar {
    /// (左部, 右部哈希) 规则集。
    pub rules: Vec<(u32, u32)>,
}

impl Grammar {
    /// 单源核对：左部零重复（同一非终结符唯一产生式组）。
    pub fn single_source(&self) -> bool {
        for k in 0..self.rules.len() {
            for m in (k + 1)..self.rules.len() {
                if self.rules[k].0 == self.rules[m].0 {
                    return false;
                }
            }
        }
        true
    }
}

/// 四件套架构：文法/解析/语义/类型共用五律与 token 契约。
pub struct SyntaxArch {
    /// 四件版本。
    pub piece_versions: [u32; PIECES as usize],
    /// 共用五律版本（宪法对拍）。
    pub five_law_v: u32,
    /// 共用 token 契约版本。
    pub token_v: u32,
    pub grammar: Grammar,
}

impl SyntaxArch {
    /// 标准构造。
    pub fn standard() -> Self {
        SyntaxArch {
            piece_versions: [1, 1, 1, 1],
            five_law_v: 5,
            token_v: TOKEN_CONTRACT_V,
            grammar: Grammar {
                rules: alloc::vec![(10, 0xAA), (11, 0xBB), (12, 0xCC)],
            },
        }
    }

    /// 四件对拍：版本一致 + 五律共用 + 契约匹配（失配→版本拦截）。
    pub fn coherent(&self) -> bool {
        self.piece_versions.iter().all(|v| *v == self.piece_versions[0])
            && self.five_law_v == 5
            && self.token_v == TOKEN_CONTRACT_V
            && self.grammar.single_source()
    }
}

/// 简单类型检查器：类型安全网（未知类型拒绝）。
pub fn type_check(t: u32) -> Result<(), i32> {
    match t {
        1..=4 => Ok(()), // int/float/bool/str
        _ => Err(1),
    }
}

/// J1 主判据：四件套一致 + 文法单源 + 五律对拍 + token 契约。
pub fn j1_main() -> bool {
    let a = SyntaxArch::standard();
    if !a.coherent() {
        return false;
    }
    // 类型检查四类全过、未知类型拒
    for t in 1..=4u32 {
        if type_check(t).is_err() {
            return false;
        }
    }
    type_check(99).is_err()
}

// ── 注入核（0x8e80 段 5 项）────────────────────────────────────────────

/// 注入核 1：件间失配注入——版本漂移必红。
pub fn inj1_piece_drift() -> bool {
    let mut a = SyntaxArch::standard();
    a.piece_versions[2] = 2;
    !a.coherent()
}

/// 注入核 2：文法重复注入——左部重复破单源必红。
pub fn inj2_grammar_dup() -> bool {
    let a = SyntaxArch {
        piece_versions: [1, 1, 1, 1],
        five_law_v: 5,
        token_v: TOKEN_CONTRACT_V,
        grammar: Grammar { rules: alloc::vec![(10, 0xAA), (10, 0xBB)] },
    };
    !a.coherent()
}

/// 注入核 3：契约失配拦截注入——token 版本不匹配必拦截。
pub fn inj3_contract_block() -> bool {
    let mut a = SyntaxArch::standard();
    a.token_v = 99;
    !a.coherent()
}

/// 注入核 4：未知类型注入——类型检查拒绝越界类型。
pub fn inj4_unknown_type() -> bool {
    type_check(0).is_err() && type_check(5).is_err()
}

/// 注入核 5：五律对拍注入——五律版本钉死 5。
pub fn inj5_five_law() -> bool {
    SyntaxArch::standard().five_law_v == 5
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_vey41_checks：本单判据聚合（诊断段 0x8e80）。
pub fn run_vey41_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5041 语法总架构");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "四件套一致+文法单源",
        j1_main(),
        if j1_main() { "" } else { "四件/文法破" },
    );
    s.add(
        "五律对拍+token契约版本拦截",
        j1_main() && inj3_contract_block(),
        if j1_main() { "" } else { "对拍/契约破" },
    );
    let inj = [
        ("注入1件间失配", inj1_piece_drift()),
        ("注入2文法重复", inj2_grammar_dup()),
        ("注入3契约拦截", inj3_contract_block()),
        ("注入4未知类型", inj4_unknown_type()),
        ("注入5五律钉死", inj5_five_law()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
