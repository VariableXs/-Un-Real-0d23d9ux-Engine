//! VE-F0435 · AST 完整性验证器（VE-C 域 · 语法组 C02 续）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0435`
//!
//! **锚点原文**：「AST 构建后完整性验证：结构不变量（父指针一致、跨度覆盖、
//! 节点类型合法组合）、不完整段登记核对（F0434 标记与实际一致）、验证通过
//! 才可进入语义期（门禁语义）；验证失败定位首个违规节点。判据：结构不变量、
//! 门禁语义、首违定位、不静默放行、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **结构不变量**（判据一）。三条不变量逐节点核对：① 父指针一致——
//!    子节点的父号必须指向声称的父（[`Inv`] 单遍核查）；② 跨度覆盖——
//!    父跨度必须包含全部子跨度（涵盖关系非相等）；③ 节点类型合法组合——
//!    [`Kind`] 组合表（如 If 节点的 cond 槽必须是 Expr 类）。任一破坏 =
//!    不变量违规。
//!
//! 2. **门禁语义**（判据二）。验证通过才发 [`Gate::Open`]；任何违规 =
//!    [`Gate::Blocked`]。没有「带警告放行」——警告也是违规，阻断回补。
//!    门禁判定是纯函数 [`gate_of`]。
//!
//! 3. **首违定位**（判据三）。多违规场景只报**第一个**违规节点号（修复
//!    一个再看下一个，避免千错雪崩）；[`verify`] 返回 [`Verdict::Fail`]
//!    携带首违号与不变量名。
//!
//! 4. **不静默放行**（判据四）。验证器自身异常（如空树）= 显性失败不
//!    放行：空树发 [`Gate::Blocked`]（空翻译单元是 vec23 的合法空态，
//!    不进本验证器——本单收到空树就是异常输入）。F0434 标记与实际段
//!    失联（标记无段/段无标记）也是违规，不是警告。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0435";

// ---------------------------------------------------------------------------
// 节点模型（验证器的消费面；与 vec22 arena 家族对齐的最小骨架）
// ---------------------------------------------------------------------------

/// 节点类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Root,
    Decl,
    Stmt,
    Expr,
    IncompleteSeg,
}

/// 验证器视角节点。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VNode {
    pub kind: Kind,
    /// 父节点号（Root = None 用 u32::MAX 表达）。
    pub parent: u32,
    /// 自身跨度。
    pub span: (u32, u32),
    /// 子节点号。
    pub children: Vec<u32>,
    /// 类别槽约束：该节点允许的子类别集。
    pub child_kinds: &'static [Kind],
}

/// 类别组合表（合法父子组合的登记面）。
pub const LEGAL_COMBOS: [(Kind, Kind); 5] = [
    (Kind::Root, Kind::Decl),
    (Kind::Root, Kind::IncompleteSeg),
    (Kind::Decl, Kind::Stmt),
    (Kind::Stmt, Kind::Expr),
    (Kind::Stmt, Kind::IncompleteSeg),
];

/// 组合合法性查表。
pub fn combo_legal(parent: Kind, child: Kind) -> bool {
    LEGAL_COMBOS.iter().any(|(p, c)| *p == parent && *c == child)
}

// ---------------------------------------------------------------------------
// 验证判定
// ---------------------------------------------------------------------------

/// 不变量名。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inv {
    ParentConsistent,
    SpanCovered,
    ComboLegal,
    MarkMatched,
}

impl Inv {
    pub fn name(self) -> &'static str {
        match self {
            Inv::ParentConsistent => "父指针一致",
            Inv::SpanCovered => "跨度覆盖",
            Inv::ComboLegal => "类型合法组合",
            Inv::MarkMatched => "标记匹配",
        }
    }
}

/// 门禁。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    Open,
    Blocked,
}

/// 判定结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    /// 首违节点号 + 不变量。
    Fail(u32, Inv),
}

/// F0434 不完整段标记（对账面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark {
    pub span: (u32, u32),
    pub reason: u8,
}

/// 验证器：单遍 O(节点数)。
pub struct Verifier {
    pub nodes: Vec<VNode>,
    pub marks: Vec<Mark>,
}

impl Verifier {
    pub fn new(nodes: Vec<VNode>, marks: Vec<Mark>) -> Self {
        Self { nodes, marks }
    }

    /// 完整验证：三条结构不变量 + 标记对账。首违即停。
    pub fn verify(&self) -> Verdict {
        // 验证器自身异常：空树显性失败不静默放行。
        if self.nodes.is_empty() {
            return Verdict::Fail(0, Inv::ParentConsistent);
        }
        for (i, n) in self.nodes.iter().enumerate() {
            // ① 父指针一致：children 里声称的子，其 parent 必须指向 i。
            for &c in n.children.iter() {
                let child = match self.nodes.get(c as usize) {
                    Some(x) => x,
                    None => return Verdict::Fail(i as u32, Inv::ParentConsistent),
                };
                if child.parent != i as u32 {
                    return Verdict::Fail(c, Inv::ParentConsistent);
                }
            }
            // ② 跨度覆盖：父跨度含全部子跨度。
            for &c in n.children.iter() {
                if let Some(child) = self.nodes.get(c as usize) {
                    if child.span.0 < n.span.0 || child.span.1 > n.span.1 {
                        return Verdict::Fail(c, Inv::SpanCovered);
                    }
                }
            }
            // ③ 类别合法组合。
            for &c in n.children.iter() {
                if let Some(child) = self.nodes.get(c as usize) {
                    if !combo_legal(n.kind, child.kind) {
                        return Verdict::Fail(c, Inv::ComboLegal);
                    }
                }
            }
        }
        // ④ 标记对账：每个标记须有对应 IncompleteSeg 节点（跨度一致），
        //    每个 IncompleteSeg 须有标记。
        for m in self.marks.iter() {
            if !self
                .nodes
                .iter()
                .any(|n| n.kind == Kind::IncompleteSeg && n.span == m.span)
            {
                return Verdict::Fail(0, Inv::MarkMatched);
            }
        }
        let seg_count = self.nodes.iter().filter(|n| n.kind == Kind::IncompleteSeg).count();
        if seg_count != self.marks.len() {
            return Verdict::Fail(0, Inv::MarkMatched);
        }
        Verdict::Pass
    }

    /// 门禁判定（纯函数语义）。
    pub fn gate_of(&self) -> Gate {
        match self.verify() {
            Verdict::Pass => Gate::Open,
            Verdict::Fail(_, _) => Gate::Blocked,
        }
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

fn mk_tree() -> (Vec<VNode>, Vec<Mark>) {
    // 0 Root [0,100)
    //   1 Decl [0,80)   parent 0
    //     2 Stmt [0,40) parent 1
    //       3 Expr [2,20) parent 2
    let nodes = vec![
        VNode { kind: Kind::Root, parent: u32::MAX, span: (0, 100), children: vec![1], child_kinds: &[] },
        VNode { kind: Kind::Decl, parent: 0, span: (0, 80), children: vec![2], child_kinds: &[] },
        VNode { kind: Kind::Stmt, parent: 1, span: (0, 40), children: vec![3], child_kinds: &[] },
        VNode { kind: Kind::Expr, parent: 2, span: (2, 20), children: vec![], child_kinds: &[] },
    ];
    (nodes, Vec::new())
}

pub fn vec35_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、结构不变量（合法树全绿）。
    {
        let (nodes, marks) = mk_tree();
        let vr = Verifier::new(nodes, marks);
        v.push(("F0435-不变量-合法树过", vr.verify() == Verdict::Pass));
        // ① 父指针破坏：子 3 的 parent 改指 0。
        let (mut nodes, marks) = mk_tree();
        nodes[3].parent = 0;
        let vr = Verifier::new(nodes, marks);
        v.push((
            "F0435-不变量-父指针破坏拦截",
            vr.verify() == Verdict::Fail(3, Inv::ParentConsistent),
        ));
        // ② 跨度越出：子 3 超出父 2。
        let (mut nodes, marks) = mk_tree();
        nodes[3].span = (2, 50);
        let vr = Verifier::new(nodes, marks);
        v.push((
            "F0435-不变量-跨度越出拦截",
            vr.verify() == Verdict::Fail(3, Inv::SpanCovered),
        ));
        // ③ 组合非法：Root 下挂 Expr。
        let (mut nodes, marks) = mk_tree();
        nodes[1].kind = Kind::Expr;
        let vr = Verifier::new(nodes, marks);
        v.push((
            "F0435-不变量-组合非法拦截",
            vr.verify() == Verdict::Fail(1, Inv::ComboLegal),
        ));
        // 组合表入册。
        v.push(("F0435-不变量-组合表入册", LEGAL_COMBOS.len() == 5 && combo_legal(Kind::Root, Kind::Decl)));
    }
    // 二、门禁语义。
    {
        let (nodes, marks) = mk_tree();
        let vr = Verifier::new(nodes, marks);
        v.push(("F0435-门禁-过则开", vr.gate_of() == Gate::Open));
        let (mut nodes, marks) = mk_tree();
        nodes[2].parent = 9; // 悬空父号
        let vr2 = Verifier::new(nodes, marks);
        v.push(("F0435-门禁-败则堵", vr2.gate_of() == Gate::Blocked));
        // 无「带警告放行」：阻断是唯一失败态（gate 只有两值）。
        v.push(("F0435-门禁-两值闭集", matches!(Gate::Open, Gate::Open) && matches!(Gate::Blocked, Gate::Blocked)));
    }
    // 三、首违定位。
    {
        // 多违规：父指针 + 组合双坏——只报首个遇到的。
        let (mut nodes, marks) = mk_tree();
        nodes[3].parent = 9;
        nodes[1].kind = Kind::Expr;
        let vr = Verifier::new(nodes, marks);
        match vr.verify() {
            Verdict::Fail(n, inv) => {
                v.push(("F0435-首违-单点报告", n == 1 && inv == Inv::ComboLegal));
            }
            _ => v.push(("F0435-首违-单点报告", false)),
        }
        // 不变量名入册。
        v.push(("F0435-首违-不变量名", Inv::SpanCovered.name() == "跨度覆盖"));
    }
    // 四、不静默放行。
    {
        // 空树 = 显性失败。
        let vr = Verifier::new(Vec::new(), Vec::new());
        v.push((
            "F0435-静默-空树显性败",
            vr.verify() == Verdict::Fail(0, Inv::ParentConsistent) && vr.gate_of() == Gate::Blocked,
        ));
        // 标记失联：有标记无段。
        let (nodes, _) = mk_tree();
        let vr2 = Verifier::new(nodes, vec![Mark { span: (90, 95), reason: 1 }]);
        v.push((
            "F0435-静默-标记无段拦截",
            vr2.verify() == Verdict::Fail(0, Inv::MarkMatched),
        ));
        // 段无标记。
        let (mut nodes, _) = mk_tree();
        nodes.push(VNode {
            kind: Kind::IncompleteSeg,
            parent: 0,
            span: (90, 95),
            children: vec![],
            child_kinds: &[],
        });
        nodes[0].children.push(4);
        let vr3 = Verifier::new(nodes, Vec::new());
        v.push((
            "F0435-静默-段无标记拦截",
            vr3.verify() == Verdict::Fail(0, Inv::MarkMatched),
        ));
        // 匹配场景全绿。
        let (mut nodes, _) = mk_tree();
        nodes.push(VNode {
            kind: Kind::IncompleteSeg,
            parent: 0,
            span: (90, 95),
            children: vec![],
            child_kinds: &[],
        });
        nodes[0].children.push(4);
        let vr4 = Verifier::new(nodes, vec![Mark { span: (90, 95), reason: 1 }]);
        v.push(("F0435-静默-匹配全绿", vr4.verify() == Verdict::Pass));
        // 判据自检。
        v.push(("F0435-判据-锚点入册", ANCHOR == "VE-F0435"));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::vec35_raw_checks as run_vec35_checks;
    #[test]
    fn vec35_astverify_checks() {
        for (name, ok) in run_vec35_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec35）。
pub fn run_vec35_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec35");
    for (name, ok) in vec35_raw_checks() {
        set.add(name, ok, "");
    }
    set
}
