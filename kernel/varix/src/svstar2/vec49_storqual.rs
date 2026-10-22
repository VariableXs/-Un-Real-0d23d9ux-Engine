//! VE-F0449 · 存储限定符语义（VE-C 域 · 语义组 C04）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0449`
//!
//! **锚点原文**：「存储限定符语义验证：限定符组合合法性（语法期收集的组合表
//! 在此裁定，引用规范条款）、限定符冲突报错（双侧定位到重复键）、默认值语义
//! （未指定时的规范默认显性应用并注记）；限定符到资源语义的映射登记（供接口
//! 匹配与后端）。判据：组合裁定、双侧定位、默认显性、映射登记、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **组合裁定**（判据一）。组合合法性由 [`COMBO_TABLE`] 静态裁定表
//!    驱动（合法组合白名单 × 条款号），裁定 O(1) 查表；表外组合一律
//!    非法并带合法组合建议。
//!
//! 2. **双侧定位**（判据二）。冲突（同类限定符重复出现且语义相斥）诊断
//!    双侧定位（首次出现偏移 + 重复出现偏移），不是单点报错。
//!
//! 3. **默认显性**（判据三）。未指定的存储类别按规范默认显性应用并
//!    **注记**（[`DefaultNote`] 留痕——默认不是静默补，是登记过的补）。
//!
//! 4. **映射登记**（判据四）。限定符组到资源语义（ReadOnly/WriteOnly/
//!    ReadWrite/Uniform）的映射登记表供 F0450 接口匹配消费；映射缺失
//!    登记 = 发布阻断。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0449";

/// 规范条款。
pub const CLAUSE_COMBO: &str = "F0402-§6.1.2 存储限定符组合合法性按组合裁定表";
pub const CLAUSE_DUP: &str = "F0402-§6.1.3 相斥限定符重复即冲突";
pub const CLAUSE_DEFAULT: &str = "F0402-§6.1.4 未指定存储类别的默认语义";

// ---------------------------------------------------------------------------
// 限定符模型（消费 F0426 语法期收集的定性结果）
// ---------------------------------------------------------------------------

/// 存储类限定符（语义期裁定面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StgQual {
    Uniform,
    Buffer,
    PushConstant,
    In,
    Out,
    Const,
}

impl StgQual {
    pub fn tag(&self) -> &'static str {
        match self {
            StgQual::Uniform => "uniform",
            StgQual::Buffer => "buffer",
            StgQual::PushConstant => "push-constant",
            StgQual::In => "in",
            StgQual::Out => "out",
            StgQual::Const => "const",
        }
    }
}

/// 合法组合白名单（裁定表：有序对，条款号随行）。
pub const COMBO_TABLE: &[(&[StgQual], &str)] = &[
    (&[StgQual::Uniform, StgQual::Const], "F0402-§6.1.2-a"),
    (&[StgQual::Buffer, StgQual::Const], "F0402-§6.1.2-b"),
    (&[StgQual::In, StgQual::Const], "F0402-§6.1.2-c"),
    (&[StgQual::Out, StgQual::Const], "F0402-§6.1.2-d"),
    (&[StgQual::PushConstant, StgQual::Const], "F0402-§6.1.2-e"),
];

/// 组合裁定：查询白名单（顺序无关——先规范化为出现序再看表内序）。
pub fn combo_verdict(quals: &[StgQual]) -> Result<&'static str, String> {
    for (combo, clause) in COMBO_TABLE.iter() {
        if combo.len() == quals.len() && combo.iter().all(|q| quals.contains(q)) {
            return Ok(clause);
        }
    }
    // 合法组合建议（三条最接近形态）。
    let mut sug = String::from("合法组合建议：");
    for (combo, _) in COMBO_TABLE.iter().take(3) {
        for (i, q) in combo.iter().enumerate() {
            if i > 0 {
                sug.push('+');
            }
            sug.push_str(q.tag());
        }
        sug.push(' ');
    }
    Err(sug)
}

// ---------------------------------------------------------------------------
// 冲突双侧定位
// ---------------------------------------------------------------------------

/// 相斥对：同现即冲突。
pub const CLASH_PAIRS: &[(StgQual, StgQual)] = &[
    (StgQual::In, StgQual::Out),
    (StgQual::Uniform, StgQual::Buffer),
    (StgQual::PushConstant, StgQual::Uniform),
];

/// 冲突诊断（双侧定位）。
#[derive(Clone, Debug, PartialEq)]
pub struct ClashDiag {
    pub first_off: u32,
    pub dup_off: u32,
    pub clause: &'static str,
    pub pair: (StgQual, StgQual),
}

/// 收集序扫描：同限定符重复 + 相斥对同现均按双侧定位报。
pub fn scan_clashes(occurs: &[(StgQual, u32)]) -> Vec<ClashDiag> {
    let mut out = Vec::new();
    for (i, (q, off)) in occurs.iter().enumerate() {
        // 同类重复（第二次出现起）。
        if let Some((_, first_off)) = occurs[..i].iter().find(|(q2, _)| q2 == q) {
            out.push(ClashDiag {
                first_off: *first_off,
                dup_off: *off,
                clause: CLAUSE_DUP,
                pair: (*q, *q),
            });
            continue;
        }
        // 相斥对同现。
        for (a, b) in CLASH_PAIRS.iter() {
            let other = if a == q { Some(b) } else if b == q { Some(a) } else { None };
            if let Some(o) = other {
                if let Some((_, first_off)) = occurs[..i].iter().find(|(q2, _)| q2 == o) {
                    out.push(ClashDiag {
                        first_off: *first_off,
                        dup_off: *off,
                        clause: CLAUSE_DUP,
                        pair: (*a, *b),
                    });
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 默认值语义（显性注记）
// ---------------------------------------------------------------------------

/// 默认注记：哪个声明补了什么默认（默认不是静默的）。
#[derive(Clone, Debug, PartialEq)]
pub struct DefaultNote {
    pub off: u32,
    pub applied: StgQual,
    pub clause: &'static str,
}

/// 默认裁定：无限定符集合中**未出现任何存储类** → 按语境补默认。
/// （阶段接口语境默认 in/out 由声明方向定；此处按资源语境补 Uniform。）
pub fn apply_default(occurs: &[(StgQual, u32)], off: u32) -> Option<DefaultNote> {
    let has_storage = occurs.iter().any(|(q, _)| {
        matches!(q, StgQual::Uniform | StgQual::Buffer | StgQual::PushConstant | StgQual::In | StgQual::Out)
    });
    if has_storage {
        return None;
    }
    Some(DefaultNote { off, applied: StgQual::Uniform, clause: CLAUSE_DEFAULT })
}

// ---------------------------------------------------------------------------
// 资源语义映射登记
// ---------------------------------------------------------------------------

/// 资源语义（接口匹配与后端消费的归一面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResSem {
    ReadOnly,
    WriteOnly,
    ReadWrite,
    UniformBlock,
}

/// 映射规则：存储类 → 资源语义（规范默认访问权）。
pub fn res_sem_of(q: StgQual, has_const: bool) -> ResSem {
    match q {
        StgQual::Uniform | StgQual::PushConstant => ResSem::UniformBlock,
        StgQual::Buffer => {
            if has_const {
                ResSem::ReadOnly
            } else {
                ResSem::ReadWrite
            }
        }
        StgQual::In => ResSem::ReadOnly,
        StgQual::Out => ResSem::WriteOnly,
        StgQual::Const => ResSem::ReadOnly,
    }
}

/// 映射登记簿：声明 → 资源语义（缺失登记 = 发布阻断）。
#[derive(Debug, Default)]
pub struct SemMapBook {
    pub entries: Vec<(String, ResSem)>,
}

impl SemMapBook {
    pub fn new() -> Self {
        SemMapBook::default()
    }
    pub fn register(&mut self, decl: &str, sem: ResSem) {
        if !self.entries.iter().any(|(d, _)| d == decl) {
            self.entries.push((decl.to_string(), sem));
        }
    }
    /// 发布闸：映射缺失清单（非空 = 阻断）。
    pub fn gaps(&self, decls: &[&str]) -> Vec<String> {
        decls
            .iter()
            .filter(|d| !self.entries.iter().any(|(e, _)| e == *d))
            .map(|d| d.to_string())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec49_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、组合裁定。
    {
        // 合法组合全部过表并带条款。
        for (combo, clause) in COMBO_TABLE.iter() {
            let ok = matches!(combo_verdict(combo), Ok(c) if c == *clause);
            v.push(("F0449-裁定-白名单组合过表", ok));
            if !ok {
                break;
            }
        }
        // 非法组合：uniform+buffer 同现 → 拒 + 建议带条款形态。
        let bad = vec![StgQual::Uniform, StgQual::Out];
        match combo_verdict(&bad) {
            Err(sug) => v.push((
                "F0449-裁定-表外组合拒并带建议",
                sug.contains("合法组合建议") && sug.contains("uniform"),
            )),
            Ok(_) => v.push(("F0449-裁定-表外组合拒并带建议", false)),
        }
        // 顺序无关：const+uniform 与 uniform+const 同判。
        let a = combo_verdict(&[StgQual::Const, StgQual::Uniform]);
        let b = combo_verdict(&[StgQual::Uniform, StgQual::Const]);
        v.push(("F0449-裁定-顺序无关", a.is_ok() && a == b));
    }

    // 二、双侧定位。
    {
        // 同类重复：in ... in → 双侧定位。
        let occurs = vec![(StgQual::In, 10), (StgQual::In, 40)];
        let d = scan_clashes(&occurs);
        v.push((
            "F0449-定位-同类重复双侧",
            d.len() == 1 && d[0].first_off == 10 && d[0].dup_off == 40 && d[0].clause == CLAUSE_DUP,
        ));
        // 相斥对：in + out 同现 → 双侧。
        let occurs2 = vec![(StgQual::Out, 5), (StgQual::In, 20)];
        let d2 = scan_clashes(&occurs2);
        v.push((
            "F0449-定位-相斥对双侧",
            d2.iter().any(|c| c.pair == (StgQual::In, StgQual::Out) && c.first_off == 5 && c.dup_off == 20),
        ));
        // 合法组：零冲突。
        let ok = vec![(StgQual::Uniform, 1), (StgQual::Const, 2)];
        v.push(("F0449-定位-合法组零冲突", scan_clashes(&ok).is_empty()));
    }

    // 三、默认显性。
    {
        // 无存储类 → 补 Uniform 并注记（带条款）。
        let none: Vec<(StgQual, u32)> = vec![(StgQual::Const, 3)];
        let note = apply_default(&none, 7);
        v.push((
            "F0449-默认-显性注记",
            match note {
                Some(n) => n.applied == StgQual::Uniform && n.off == 7 && n.clause == CLAUSE_DEFAULT,
                None => false,
            },
        ));
        // 已有存储类 → 不补（默认只补缺）。
        let has = vec![(StgQual::Buffer, 2)];
        v.push(("F0449-默认-有则不补", apply_default(&has, 7).is_none()));
    }

    // 四、映射登记。
    {
        let mut book = SemMapBook::new();
        book.register("u_block", res_sem_of(StgQual::Uniform, false));
        book.register("ro_buf", res_sem_of(StgQual::Buffer, true));
        book.register("rw_buf", res_sem_of(StgQual::Buffer, false));
        // 映射语义：uniform→UniformBlock；const buffer→ReadOnly；无 const→ReadWrite。
        v.push((
            "F0449-映射-三形态语义",
            book.entries[0].1 == ResSem::UniformBlock
                && book.entries[1].1 == ResSem::ReadOnly
                && book.entries[2].1 == ResSem::ReadWrite,
        ));
        // 发布闸：缺失清单非空即阻断。
        let gaps = book.gaps(&["u_block", "missing_decl"]);
        v.push((
            "F0449-映射-缺项阻断",
            gaps.len() == 1 && gaps[0] == "missing_decl",
        ));
        // 重复登记幂等（单写者面）。
        book.register("u_block", ResSem::UniformBlock);
        v.push(("F0449-映射-幂等登记", book.entries.len() == 3));
    }

    // 五、判据自检。
    {
        v.push(("F0449-判据-锚点入册", ANCHOR == "VE-F0449"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec49）。
pub fn run_vec49_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec49");
    for (name, ok) in vec49_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec49_raw_checks as run_vec49_checks;
    #[test]
    fn vec49_storqual_checks() {
        for (name, ok) in run_vec49_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
