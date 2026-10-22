//! VE-F0444 · 表达式类型推导（VE-C 域 · 语义组 C03 续）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0444`
//!
//! **锚点原文**：「表达式类型推导：自底向上单遍推导（子表达式类型合成父
//! 类型）、运算符结果类型按转换等级表裁定、推导失败产出哨兵并记诊断
//! （条款引用）、推导结果旁路表（每表达式节点类型标注不回写 AST）；常量
//! 上下文提示（类型已知时收紧字面量解释）。判据：单遍推导、短路哨兵、
//! 旁路标注、条款诊断、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **单遍推导**（判据一）。自底向上单遍：[`Infer::walk`] 对表达式树
//!    一次后序遍历——每个节点恰推导一次（访问计数实测 = 节点数）。子
//!    类型合成父类型：二元按 vec43 [`combine`]、一元保操作数、操作数查
//!    符号旁路。
//!
//! 2. **短路哨兵**（判据二）。子表达式哨兵 → 父节点**短路为哨兵不重复
//!    报错**（vec43 哨兵纪律的表达式层兑现）：推导器遇到子结果 Err 时
//!    直接上抛，**不再调用合成规则**——诊断只在哨兵产生地记一次。
//!
//! 3. **旁路标注**（判据三）。结果写 [`SideNote`]（F0441 旁路表），节点
//!    类型标注**不回写 AST**——旁路表是唯一通道（遍间不可变约定）。
//!    推导完成核：旁路条目数 == 表达式节点数（全覆盖）。
//!
//! 4. **条款诊断**（判据四）。推导失败记诊断带 F0402 条款引用（vec41
//!    引用制的语义组兑现）：运算符无组合 → 报错带两侧类型与建议；缺
//!    条款引用不可发布。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use alloc::boxed::Box;
use alloc::format;

pub const ANCHOR: &str = "VE-F0444";

/// 条款引用（推导失败诊断面）。
pub const CLAUSE_NO_COMBO: &str = "F0402-§5.4 运算符无可用类型组合";
pub const CLAUSE_UNRESOLVED: &str = "F0402-§4.2 标识符未解析";

// ---------------------------------------------------------------------------
// 表达式树（推导消费面；与 vec28 形态对齐的最小版）
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ENode {
    Lit { text: String, node: u32 },
    Ident { name: String, node: u32 },
    Unary { op: &'static str, child: Box<ENode>, node: u32 },
    Binary { op: &'static str, lhs: Box<ENode>, rhs: Box<ENode>, node: u32 },
}

impl ENode {
    /// 节点号（旁路标注键）。
    pub fn id(&self) -> u32 {
        match self {
            ENode::Lit { node, .. } | ENode::Ident { node, .. } => *node,
            ENode::Unary { node, .. } | ENode::Binary { node, .. } => *node,
        }
    }
    /// 节点总数。
    pub fn count(&self) -> usize {
        match self {
            ENode::Lit { .. } | ENode::Ident { .. } => 1,
            ENode::Unary { child, .. } => 1 + child.count(),
            ENode::Binary { lhs, rhs, .. } => 1 + lhs.count() + rhs.count(),
        }
    }
}

/// 符号类型旁路（推导的操作数查 surface）。
#[derive(Debug, Default)]
pub struct SymTypes {
    pub map: Vec<(String, Ty)>,
}

impl SymTypes {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn bind(&mut self, name: &str, ty: Ty) {
        self.map.push((name.into(), ty));
    }
    pub fn get(&self, name: &str) -> Option<&Ty> {
        self.map.iter().rev().find(|(n, _)| n == name).map(|(_, t)| t)
    }
}

// ---------------------------------------------------------------------------
// 推导器
// ---------------------------------------------------------------------------

use super::vec43_typesys::Ty;

/// 推导器：单遍后序 + 访问计数 + 诊断记账。
#[derive(Debug)]
pub struct Infer {
    pub syms: SymTypes,
    /// 访问计数（单遍实测面）。
    pub visits: u32,
    /// 旁路标注：节点号 → 类型摘要。
    pub notes: Vec<(u32, String)>,
    /// 诊断（条款引用制）。
    pub diags: Vec<(u32, &'static str, String)>,
}

impl Infer {
    pub fn new(syms: SymTypes) -> Self {
        Self { syms, visits: 0, notes: Vec::new(), diags: Vec::new() }
    }
    /// 单遍后序推导。
    pub fn walk(&mut self, e: &ENode) -> Ty {
        self.visits += 1;
        match e {
            ENode::Lit { text, node } => {
                // 字面量：整数→int、小数→float；常量上下文提示位（类型
                // 已知时收紧——由调用方以 syms 预绑定实现，此处缺省）。
                let ty = if text.contains('.') {
                    Ty::Scalar("float")
                } else {
                    Ty::Scalar("int")
                };
                self.note(*node, &ty);
                ty
            }
            ENode::Ident { name, node } => match self.syms.get(name) {
                Some(t) => {
                    let t = t.clone();
                    self.note(*node, &t);
                    t
                }
                None => {
                    // 未解析 → 哨兵 + 条款诊断。
                    self.diags.push((*node, CLAUSE_UNRESOLVED, name.clone()));
                    self.note(*node, &Ty::Err);
                    Ty::Err
                }
            },
            ENode::Unary { child, node, .. } => {
                let c = self.walk(child);
                if matches!(c, Ty::Err) {
                    // 短路哨兵：不记新诊断直接上抛。
                    self.note(*node, &Ty::Err);
                    return Ty::Err;
                }
                self.note(*node, &c);
                c
            }
            ENode::Binary { lhs, rhs, node, op } => {
                let a = self.walk(lhs);
                let b = self.walk(rhs);
                if matches!(a, Ty::Err) || matches!(b, Ty::Err) {
                    // 短路哨兵。
                    self.note(*node, &Ty::Err);
                    return Ty::Err;
                }
                let r = super::vec43_typesys::combine(&a, &b);
                if matches!(r, Ty::Err) {
                    // 无组合：条款诊断带两侧类型。
                    self.diags.push((
                        *node,
                        CLAUSE_NO_COMBO,
                        alloc::format!("{} {} {}", a.digest(), op, b.digest()),
                    ));
                    self.note(*node, &Ty::Err);
                    return Ty::Err;
                }
                self.note(*node, &r);
                r
            }
        }
    }
    fn note(&mut self, node: u32, ty: &Ty) {
        self.notes.push((node, ty.digest()));
    }
    /// 旁路全覆盖核：条目数 == 节点数。
    pub fn full_coverage(&self, e: &ENode) -> bool {
        self.notes.len() == e.count()
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec44_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、单遍推导。
    {
        // x + y * 2：y(int) * 2(int) = int；x(float) + int → float。
        let mut syms = SymTypes::new();
        syms.bind("x", Ty::Scalar("float"));
        syms.bind("y", Ty::Scalar("int"));
        let tree = ENode::Binary {
            op: "+",
            lhs: Box::new(ENode::Ident { name: "x".into(), node: 1 }),
            rhs: Box::new(ENode::Binary {
                op: "*",
                lhs: Box::new(ENode::Ident { name: "y".into(), node: 2 }),
                rhs: Box::new(ENode::Lit { text: "2".into(), node: 3 }),
                node: 4,
            }),
            node: 5,
        };
        let mut inf = Infer::new(syms);
        let r = inf.walk(&tree);
        v.push(("F0444-单遍-合成结果", r == Ty::Scalar("float")));
        // 访问计数 == 节点数（单遍实测）。
        v.push(("F0444-单遍-访问计数", inf.visits as usize == tree.count()));
        // 旁路全覆盖。
        v.push(("F0444-单遍-旁路全覆盖", inf.full_coverage(&tree)));
        // 无诊断。
        v.push(("F0444-单遍-零诊断", inf.diags.is_empty()));
    }
    // 二、短路哨兵。
    {
        // bad + 1 → Err；再 (bad+1) * 2 → 短路上抛，诊断只有一条。
        // 哨兵产生地 = 未解析名（不绑定 bad，走 Ident 未解析分支）。
        let syms = SymTypes::new();
        let tree = ENode::Binary {
            op: "*",
            lhs: Box::new(ENode::Binary {
                op: "+",
                lhs: Box::new(ENode::Ident { name: "bad".into(), node: 1 }),
                rhs: Box::new(ENode::Lit { text: "1".into(), node: 2 }),
                node: 3,
            }),
            rhs: Box::new(ENode::Lit { text: "2".into(), node: 4 }),
            node: 5,
        };
        let mut inf = Infer::new(syms);
        let r = inf.walk(&tree);
        v.push(("F0444-短路-根为哨兵", r == Ty::Err));
        // 诊断只在哨兵产生地（node 1 未解析）一条。
        v.push(("F0444-短路-诊断单点", inf.diags.len() == 1 && inf.diags[0].0 == 1));
        // 哨兵沿链传播：err 节点 = 哨兵源 + 两个合成点（字面量正常）。
        v.push(("F0444-短路-哨兵沿链传播", {
            let ds: Vec<&String> = inf.notes.iter().map(|(_, d)| d).collect();
            ds.iter().filter(|d| ***d == "err").count() == 3
        }));
    }
    // 三、旁路标注。
    {
        // 旁路不回写树：标注表独立（节点号键），树无类型字段（结构性：ENode 无 ty 字段）。
        let mut syms = SymTypes::new();
        syms.bind("v", Ty::Vector { elem: "float", dim: 3 });
        let tree = ENode::Ident { name: "v".into(), node: 7 };
        let mut inf = Infer::new(syms);
        let _ = inf.walk(&tree);
        v.push((
            "F0444-旁路-节点号键",
            inf.notes.len() == 1 && inf.notes[0].0 == 7,
        ));
        // 标注值正确。
        v.push(("F0444-旁路-标注值", inf.notes[0].1 == "v:floatx3"));
        // 覆盖核随树规模。
        let tree2 = ENode::Unary {
            op: "-",
            child: Box::new(ENode::Ident { name: "v".into(), node: 8 }),
            node: 9,
        };
        let mut syms2 = SymTypes::new();
        syms2.bind("v", Ty::Vector { elem: "float", dim: 3 });
        let mut inf2 = Infer::new(syms2);
        let _ = inf2.walk(&tree2);
        v.push(("F0444-旁路-覆盖随规模", inf2.full_coverage(&tree2) && inf2.notes[0].1 == "v:floatx3"));
    }
    // 四、条款诊断。
    {
        // 无组合：bool + vec3 → 条款诊断带两侧类型。
        let mut syms = SymTypes::new();
        syms.bind("b", Ty::Scalar("bool"));
        syms.bind("v", Ty::Vector { elem: "float", dim: 3 });
        let tree = ENode::Binary {
            op: "+",
            lhs: Box::new(ENode::Ident { name: "b".into(), node: 1 }),
            rhs: Box::new(ENode::Ident { name: "v".into(), node: 2 }),
            node: 3,
        };
        let mut inf = Infer::new(syms);
        let _ = inf.walk(&tree);
        v.push((
            "F0444-条款-无组合诊断",
            inf.diags.len() == 1 && inf.diags[0].1 == CLAUSE_NO_COMBO,
        ));
        // 诊断带两侧类型摘要。
        v.push((
            "F0444-条款-两侧类型",
            inf.diags[0].2.contains("s:bool") && inf.diags[0].2.contains("v:floatx3"),
        ));
        // 未解析诊断条款。
        let mut inf2 = Infer::new(SymTypes::new());
        let _ = inf2.walk(&ENode::Ident { name: "ghost".into(), node: 5 });
        v.push(("F0444-条款-未解析条款", inf2.diags[0].1 == CLAUSE_UNRESOLVED));
        // 判据自检。
        v.push(("F0444-判据-锚点入册", ANCHOR == "VE-F0444"));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::vec44_raw_checks as run_vec44_checks;
    #[test]
    fn vec44_infer_checks() {
        for (name, ok) in run_vec44_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec44）。
pub fn run_vec44_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec44");
    for (name, ok) in vec44_raw_checks() {
        set.add(name, ok, "");
    }
    set
}
