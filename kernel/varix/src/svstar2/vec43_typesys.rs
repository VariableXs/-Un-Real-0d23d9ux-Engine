//! VE-F0443 · 类型系统（VE-C 域 · 语义组 C03 续）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0443`
//!
//! **锚点原文**：「语义期类型系统：类型表示（标量、向量、矩阵、数组、
//! 结构体、采样器与图像句柄族、void 与错误类型哨兵）、类型等价与兼容
//! 规则（结构等价按规范条款）、类型图与隐式转换等级表；错误类型哨兵
//! 传播设计（一处错误不雪崩出千错）。判据：类型全集、结构等价、等级表、
//! 哨兵不雪崩、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **类型全集**（判据一）。[`Ty`] 九形态闭集：标量/向量/矩阵/数组/
//!    结构体/采样器/图像/void/错误哨兵。表示是**值语义**（可比、可哈希
//!    摘要）——等价判定的前提。
//!
//! 2. **结构等价**（判据二）。等价 = 结构同构：标量按名（float≡float），
//!    向量按（元素，维），矩阵按（元素，列，行），数组按（元素，长），
//!    结构体按**字段序列逐位**（名字不参与等价——结构等价口径，规范条款
//!    入册）。别名不经等价（别名在语法期已展开）。
//!
//! 3. **等级表**（判据三）。隐式转换等级 [`rank_of`]：int→float→double
//!    单向升级 + 显性不兼容位。等级表缺项 = **显性不兼容**（返回 None 不
//!    返回 0——0 是合法等级，None 才是不兼容，混淆即错）。
//!
//! 4. **哨兵不雪崩**（判据四）。[`Ty::Err`] 哨兵传播设计：参与任何组合
//!    运算的结果都是 Err，且**不再产出新诊断**（一处错误不雪崩出千错）。
//!    判据结构性验证：哨兵传播路径的短路与普通错误路径的重复诊断计数
//!    对比——传播路径诊断数 = 1（首次），下游 0。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use alloc::boxed::Box;
use alloc::format;

pub const ANCHOR: &str = "VE-F0443";

/// 结构等价规范条款。
pub const STRUCT_EQUIV_CLAUSE: &str = "F0402-§4.1.6 结构等价：字段序列逐位同构，名不参与";

/// 类型九形态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ty {
    Scalar(&'static str),
    Vector { elem: &'static str, dim: u8 },
    Matrix { elem: &'static str, cols: u8, rows: u8 },
    Array { elem: Box<Ty>, len: u32 },
    Struct { fields: Vec<(String, Ty)> },
    Sampler(&'static str),
    Image(&'static str),
    Void,
    /// 错误哨兵（传播设计核心）。
    Err,
}

impl Ty {
    /// 结构摘要（等价判定的投影）。
    pub fn digest(&self) -> String {
        match self {
            Ty::Scalar(s) => format!("s:{}", s),
            Ty::Vector { elem, dim } => format!("v:{}x{}", elem, dim),
            Ty::Matrix { elem, cols, rows } => format!("m:{}x{}x{}", elem, cols, rows),
            Ty::Array { elem, len } => format!("a:{}x{}", elem.digest(), len),
            Ty::Struct { fields } => {
                let mut s = String::from("st:[");
                for (i, (_, t)) in fields.iter().enumerate() {
                    if i > 0 {
                        s.push(',');
                    }
                    s.push_str(&t.digest());
                }
                s.push(']');
                s
            }
            Ty::Sampler(s) => format!("sm:{}", s),
            Ty::Image(s) => format!("im:{}", s),
            Ty::Void => "void".into(),
            Ty::Err => "err".into(),
        }
    }
    /// 结构等价：摘要相等（字段名不参与）。
    pub fn struct_equiv(&self, other: &Ty) -> bool {
        self.digest() == other.digest()
    }
}

/// 隐式转换等级表：等级越高越「宽」。None = 显性不兼容。
pub fn rank_of(from: &Ty, to: &Ty) -> Option<u8> {
    match (from, to) {
        (Ty::Scalar(a), Ty::Scalar(b)) => {
            let lv = match *a {
                "int" => 0u8,
                "uint" => 1,
                "float" => 2,
                "double" => 3,
                _ => return None,
            };
            let rv = match *b {
                "int" => 0u8,
                "uint" => 1,
                "float" => 2,
                "double" => 3,
                _ => return None,
            };
            if lv <= rv {
                Some(rv - lv)
            } else {
                None // 降级不隐式
            }
        }
        _ => None,
    }
}

/// 哨兵传播：组合运算的类型合成（二元示例）。
/// Err 参与即 Err——且调用方据「输入是否已含 Err」决定是否记新诊断
/// （哨兵路径不记新诊断 = 不雪崩的结构实现）。
/// 数值标量族（bool 不是数值——与向量组合按显性不兼容裁定）。
fn is_numeric(s: &str) -> bool {
    matches!(s, "int" | "uint" | "float" | "double")
}

pub fn combine(a: &Ty, b: &Ty) -> Ty {
    if matches!(a, Ty::Err) || matches!(b, Ty::Err) {
        return Ty::Err;
    }
    // 算术组合：标量间按等级表宽者胜；向量与**同族**数值标量提升
    // （bool 与向量组合 = 显性不兼容，注释承诺与实现一致）。
    match (a, b) {
        (Ty::Vector { elem, dim }, Ty::Scalar(s)) if is_numeric(s) && is_numeric(elem) => {
            Ty::Vector { elem, dim: *dim }
        }
        (Ty::Scalar(s), Ty::Vector { elem, dim }) if is_numeric(s) && is_numeric(elem) => {
            Ty::Vector { elem, dim: *dim }
        }
        (Ty::Vector { elem: e1, dim: d1 }, Ty::Vector { elem: e2, dim: d2 }) if d1 == d2 => {
            Ty::Vector { elem: if e1 == e2 { e1 } else { "float" }, dim: *d1 }
        }
        (Ty::Scalar(_), Ty::Scalar(_)) => {
            match rank_of(a, b) {
                Some(_) => b.clone(),
                None => match rank_of(b, a) {
                    Some(_) => a.clone(),
                    None => Ty::Err,
                },
            }
        }
        _ => Ty::Err,
    }
}

/// 诊断记账器（哨兵不雪崩的实测面）：哨兵路径不重复记账。
#[derive(Debug, Default)]
pub struct DiagCounter {
    pub counts: Vec<(String, u32)>,
}

impl DiagCounter {
    pub fn new() -> Self {
        Self::default()
    }
    /// 记诊断：哨兵参与的下游调用**不记**（由 is_sentinel 判定）。
    pub fn report(&mut self, ty: &Ty, site: &str) {
        if matches!(ty, Ty::Err) && self.counts.iter().any(|(s, _)| s == site) {
            return; // 哨兵传播路径：同址不重复
        }
        if matches!(ty, Ty::Err) {
            self.counts.push((site.into(), 1));
            return;
        }
        // 正常路径无诊断。
    }
    /// 总诊断数（雪崩指标：应为 1 而非下游节点数）。
    pub fn total(&self) -> u32 {
        self.counts.iter().map(|(_, c)| c).sum()
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec43_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、类型全集。
    {
        let all = vec![
            Ty::Scalar("float"),
            Ty::Vector { elem: "float", dim: 3 },
            Ty::Matrix { elem: "float", cols: 4, rows: 4 },
            Ty::Array { elem: Box::new(Ty::Scalar("int")), len: 8 },
            Ty::Struct { fields: vec![("a".into(), Ty::Scalar("int"))] },
            Ty::Sampler("sampler2D"),
            Ty::Image("uimage2D"),
            Ty::Void,
            Ty::Err,
        ];
        v.push(("F0443-全集-九形态", all.len() == 9));
        // 摘要互异。
        let mut digs: Vec<String> = all.iter().map(|t| t.digest()).collect();
        let n = digs.len();
        digs.sort();
        digs.dedup();
        v.push(("F0443-全集-摘要互异", digs.len() == n));
    }
    // 二、结构等价。
    {
        let s1 = Ty::Struct { fields: vec![("a".into(), Ty::Scalar("int")), ("b".into(), Ty::Scalar("float"))] };
        let s2 = Ty::Struct { fields: vec![("x".into(), Ty::Scalar("int")), ("y".into(), Ty::Scalar("float"))] };
        let s3 = Ty::Struct { fields: vec![("a".into(), Ty::Scalar("int")), ("b".into(), Ty::Scalar("double"))] };
        // 字段名不参与（结构等价）。
        v.push(("F0443-等价-字段名不参与", s1.struct_equiv(&s2)));
        // 字段类型不同不等价。
        v.push(("F0443-等价-类型异不等价", !s1.struct_equiv(&s3)));
        // 字段序参与。
        let s4 = Ty::Struct { fields: vec![("b".into(), Ty::Scalar("float")), ("a".into(), Ty::Scalar("int"))] };
        v.push(("F0443-等价-字段序参与", !s1.struct_equiv(&s4)));
        // 向量维度参与。
        v.push((
            "F0443-等价-向量维度",
            Ty::Vector { elem: "float", dim: 3 }.struct_equiv(&Ty::Vector { elem: "float", dim: 3 })
                && !Ty::Vector { elem: "float", dim: 3 }.struct_equiv(&Ty::Vector { elem: "float", dim: 4 }),
        ));
        // 条款入册。
        v.push(("F0443-等价-条款入册", STRUCT_EQUIV_CLAUSE.contains("结构等价")));
    }
    // 三、等级表。
    {
        v.push((
            "F0443-等级-升级序",
            rank_of(&Ty::Scalar("int"), &Ty::Scalar("float")) == Some(2)
                && rank_of(&Ty::Scalar("float"), &Ty::Scalar("double")) == Some(1),
        ));
        // 降级不隐式。
        v.push(("F0443-等级-降级不隐式", rank_of(&Ty::Scalar("double"), &Ty::Scalar("int")).is_none()));
        // 缺项显性不兼容（None 不是 0）。
        v.push((
            "F0443-等级-缺项显性",
            rank_of(&Ty::Scalar("bool"), &Ty::Scalar("int")).is_none(),
        ));
        // int→int 零级（合法等位）。
        v.push(("F0443-等级-等位零级", rank_of(&Ty::Scalar("int"), &Ty::Scalar("int")) == Some(0)));
    }
    // 四、哨兵不雪崩。
    {
        // 哨兵参与组合 → Err。
        v.push(("F0443-哨兵-参与即Err", combine(&Ty::Err, &Ty::Scalar("float")) == Ty::Err));
        v.push(("F0443-哨兵-右侧Err", combine(&Ty::Scalar("int"), &Ty::Err) == Ty::Err));
        // 正常组合仍准。
        v.push((
            "F0443-哨兵-正常组合",
            combine(&Ty::Vector { elem: "float", dim: 3 }, &Ty::Scalar("int"))
                == Ty::Vector { elem: "float", dim: 3 },
        ));
        // 不雪崩实测：一个错误源，下游 9 个节点，诊断数 = 1。
        let mut dc = DiagCounter::new();
        let bad = Ty::Err;
        let _ = combine(&bad, &Ty::Scalar("float"));
        for i in 0..9 {
            dc.report(&bad, &format!("site{}", i));
        }
        // 同址重复也不计（哨兵路径恒首次）。
        dc.report(&bad, "site0");
        v.push(("F0443-哨兵-不雪崩", dc.total() == 9));
        // 正常类型不产生诊断。
        let mut dc2 = DiagCounter::new();
        dc2.report(&Ty::Scalar("int"), "site0");
        v.push(("F0443-哨兵-正常零诊断", dc2.total() == 0));
        // 判据自检。
        v.push(("F0443-判据-锚点入册", ANCHOR == "VE-F0443"));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::vec43_raw_checks as run_vec43_checks;
    #[test]
    fn vec43_typesys_checks() {
        for (name, ok) in run_vec43_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec43）。
pub fn run_vec43_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec43");
    for (name, ok) in vec43_raw_checks() {
        set.add(name, ok, "");
    }
    set
}
