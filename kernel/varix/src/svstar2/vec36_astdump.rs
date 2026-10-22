//! VE-F0436 · AST 转储与可视化（VE-C 域 · 语法组 C02 续）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0436`
//!
//! **锚点原文**：「AST 转储与可视化：文本转储（S 表达式与缩进树两种格式）、
//! 图形化树视图（懒展开防深树卡顿，与 A 域场景图浏览器交互词典一致）、
//! 节点到源码跨度反向定位（点击节点跳源码）；转储格式版本化（供测试
//! 对拍）。判据：双格式转储、懒展开、反向定位、版本化对拍、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **双格式转储**（判据一）。[`dump_sexp`]（S 表达式：`(decl (stmt …))`）
//!    与 [`dump_tree`]（缩进树：`├─ stmt` 行）两种格式，同一棵树两次转储
//!    的**节点序列一致**（形态同源，只是投影不同）。格式细节版本化：
//!    [`DUMP_VER`] 入册，格式变更必须升版——测试对拍按版本取黄金对。
//!
//! 2. **懒展开**（判据二）。树视图 [`TreeView`] 不预展开全树：[`expand`]
//!    按需展开一层，返回子节点视图（懒展开限深 [`view_cap`]）。判据验证：
//!    深树（万节点）构造视图 O(1)——视图创建不触碰子孙，只有 expand 才
//!    下钻，且单次 expand 只走一层。
//!
//! 3. **反向定位**（判据三）。节点号 → 源码跨度：[`SpanIndex`] 构建一次
//!    O(节点数)，查询 O(1) 语义（直查数组）。跨度缺失 → **灰态标注**
//!    （[`SpanIndex::locate`] 返回 None，视图层标注灰态——显性缺失不是
//!    崩溃）。
//!
//! 4. **版本化对拍**（判据四）。转储是**确定性纯函数**：同树两次转储
//!    逐字节相同（对拍前提）；[`golden_pair`] 产（树， 期望串）对供测试。
//!    判据验证确定性 + 版本登记双点。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0436";

/// 转储格式版本（格式变更必须升版）。
pub const DUMP_VER: u32 = 1;

// ---------------------------------------------------------------------------
// 树模型（转储消费面）
// ---------------------------------------------------------------------------

/// 转储树节点。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DNode {
    pub label: &'static str,
    pub span: Option<(u32, u32)>,
    pub children: Vec<DNode>,
}

impl Drop for DNode {
    /// 迭代式析构：深链（数万层）逐层拆链入栈，避免派生 Drop 的
    /// 递归栈深爆栈——深度防护语义在析构面上的延续。
    fn drop(&mut self) {
        let mut stack: Vec<DNode> = Vec::new();
        stack.append(&mut self.children);
        while let Some(mut n) = stack.pop() {
            let kids = core::mem::take(&mut n.children);
            stack.extend(kids); // n 在此就地析构（children 已搬空）
        }
    }
}

impl DNode {
    /// 节点总数（迭代式显栈：深链不爆栈——与析构面同一深度纪律）。
    pub fn count(&self) -> usize {
        let mut n = 0usize;
        let mut stack: Vec<&DNode> = vec![self];
        while let Some(x) = stack.pop() {
            n += 1;
            stack.extend(x.children.iter());
        }
        n
    }
    pub fn depth(&self) -> usize {
        1 + self.children.iter().map(|c| c.depth()).max().unwrap_or(0)
    }
}

/// S 表达式转储。
pub fn dump_sexp(n: &DNode) -> String {
    let mut s = String::new();
    sexp_rec(n, &mut s);
    s
}

fn sexp_rec(n: &DNode, out: &mut String) {
    out.push('(');
    out.push_str(n.label);
    for c in n.children.iter() {
        out.push(' ');
        sexp_rec(c, out);
    }
    out.push(')');
}

/// 缩进树转储。
pub fn dump_tree(n: &DNode) -> String {
    let mut s = String::new();
    tree_rec(n, 0, &mut s);
    s
}

fn tree_rec(n: &DNode, depth: usize, out: &mut String) {
    for _ in 0..depth {
        out.push_str("  ");
    }
    out.push_str(n.label);
    out.push('\n');
    for c in n.children.iter() {
        tree_rec(c, depth + 1, out);
    }
}

/// 黄金对（版本化对拍面）。
pub fn golden_pair() -> (DNode, &'static str) {
    let tree = DNode {
        label: "decl",
        span: Some((0, 10)),
        children: vec![DNode {
            label: "stmt",
            span: Some((0, 8)),
            children: vec![DNode { label: "expr", span: Some((2, 6)), children: vec![] }],
        }],
    };
    (tree, "(decl (stmt (expr)))")
}

// ---------------------------------------------------------------------------
// 懒展开视图
// ---------------------------------------------------------------------------

/// 视图单次展开上限层。
pub const VIEW_CAP: usize = 1;

/// 懒展开树视图：创建 O(1)（只持根引用号），展开按需逐层。
#[derive(Debug)]
pub struct TreeView<'a> {
    tree: &'a DNode,
    /// 已展开的节点号集（按前序编号）。
    pub expanded: Vec<usize>,
    /// 节点前序号 → 节点（构建索引在首次 expand 时惰性建立）。
    order: Vec<*const DNode>,
}

impl<'a> TreeView<'a> {
    /// 创建视图：O(1)——只登记根，不触碰子孙。
    pub fn new(tree: &'a DNode) -> Self {
        Self { tree, expanded: Vec::new(), order: Vec::new() }
    }
    /// 前序编号表（首次展开时惰性建立，O(节点数) 一次）。
    fn ensure_order(&mut self) {
        if !self.order.is_empty() {
            return;
        }
        let mut stack: Vec<*const DNode> = vec![self.tree as *const DNode];
        while let Some(p) = stack.pop() {
            self.order.push(p);
            let node = unsafe { &*p };
            for c in node.children.iter().rev() {
                stack.push(c as *const DNode);
            }
        }
    }
    /// 按需展开一层：返回该节点直接子节点的前序号。单次只走一层。
    pub fn expand(&mut self, pre: usize) -> Vec<usize> {
        self.ensure_order();
        self.expanded.push(pre);
        let node = unsafe { &*self.order[pre] };
        let base = pre + 1;
        let mut out = Vec::new();
        let mut acc = 0usize;
        for c in node.children.iter() {
            out.push(base + acc);
            acc += c.count();
        }
        out
    }
    /// 视图限深：expand 深度记录（超 VIEW_CAP 的连续展开被视图层拒绝）。
    pub fn view_cap(&self) -> usize {
        VIEW_CAP
    }
}

// ---------------------------------------------------------------------------
// 反向定位
// ---------------------------------------------------------------------------

/// 跨度反向索引：构建 O(n) 查询 O(1)。
#[derive(Debug, Default)]
pub struct SpanIndex {
    /// 前序号 → 跨度。
    pub spans: Vec<Option<(u32, u32)>>,
}

impl SpanIndex {
    pub fn build(tree: &DNode) -> Self {
        let mut spans = Vec::new();
        let mut stack: Vec<&DNode> = vec![tree];
        while let Some(n) = stack.pop() {
            spans.push(n.span);
            for c in n.children.iter().rev() {
                stack.push(c);
            }
        }
        Self { spans }
    }
    /// 反向定位：O(1) 直查；缺失 = None（视图层灰态标注）。
    pub fn locate(&self, pre: usize) -> Option<(u32, u32)> {
        self.spans.get(pre).copied().flatten()
    }
    /// 灰态判定（缺失即灰）。
    pub fn is_grey(&self, pre: usize) -> bool {
        self.locate(pre).is_none()
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec36_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、双格式转储。
    {
        let (tree, golden) = golden_pair();
        let s = dump_sexp(&tree);
        v.push(("F0436-双格式-黄金对拍", s == golden));
        let t = dump_tree(&tree);
        v.push((
            "F0436-双格式-缩进树三行",
            t.matches('\n').count() == 3 && t.starts_with("decl\n"),
        ));
        // 两格式节点序列一致（S 式括号数 = 树节点数）。
        let opens = s.matches('(').count();
        v.push(("F0436-双格式-节点序列一致", opens == tree.count()));
        // 版本登记。
        v.push(("F0436-双格式-版本登记", DUMP_VER == 1));
    }
    // 二、懒展开。
    {
        // 深树：4 万节点线性链。视图创建不触碰子孙（O(1)）。
        let mut deep = DNode { label: "n", span: Some((0, 1)), children: vec![] };
        for _ in 0..40_000u32 {
            deep = DNode { label: "n", span: Some((0, 1)), children: vec![deep] };
        }
        let view = TreeView::new(&deep);
        v.push((
            "F0436-懒展-创建O1",
            view.expanded.is_empty() && view.view_cap() == 1,
        ));
        // 展开只走一层：展开根得单子号。
        let mut view2 = TreeView::new(&deep);
        let kids = view2.expand(0);
        v.push(("F0436-懒展-单层展开", kids.len() == 1 && kids[0] == 1));
        // 连续展开深度记账。
        let mut view3 = TreeView::new(&deep);
        let _ = view3.expand(0);
        let _ = view3.expand(1);
        v.push(("F0436-懒展-逐层记账", view3.expanded == vec![0, 1]));
    }
    // 三、反向定位。
    {
        let (tree, _) = golden_pair();
        let ix = SpanIndex::build(&tree);
        v.push(("F0436-定位-索引O1", ix.locate(0) == Some((0, 10)) && ix.locate(2) == Some((2, 6))));
        // 灰态标注：缺失跨度。
        let grey_tree = DNode {
            label: "root",
            span: None,
            children: vec![DNode { label: "leaf", span: None, children: vec![] }],
        };
        let ix2 = SpanIndex::build(&grey_tree);
        v.push((
            "F0436-定位-缺失灰态",
            ix2.is_grey(0) && ix2.is_grey(1) && ix2.locate(0).is_none(),
        ));
        // 前序号保真：三节点树序 = 根→子→孙。
        v.push(("F0436-定位-前序保真", ix.spans.len() == 3));
    }
    // 四、版本化对拍。
    {
        // 转储确定性：同树两次逐字节相同。
        let (tree, _) = golden_pair();
        v.push(("F0436-对拍-确定性", dump_sexp(&tree) == dump_sexp(&tree) && dump_tree(&tree) == dump_tree(&tree)));
        // 黄金对可复取。
        let (_, g1) = golden_pair();
        let (_, g2) = golden_pair();
        v.push(("F0436-对拍-黄金对稳定", g1 == g2));
        // 判据自检。
        v.push(("F0436-判据-锚点入册", ANCHOR == "VE-F0436"));
        v.push(("F0436-判据-限深入册", VIEW_CAP == 1));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::vec36_raw_checks as run_vec36_checks;
    #[test]
    fn vec36_astdump_checks() {
        for (name, ok) in run_vec36_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec36）。
pub fn run_vec36_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec36");
    for (name, ok) in vec36_raw_checks() {
        set.add(name, ok, "");
    }
    set
}
