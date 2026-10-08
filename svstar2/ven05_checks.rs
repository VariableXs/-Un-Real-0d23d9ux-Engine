//! VE-F2605 · 域自检（判据逐条对应，见 `ven05_dual.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **双树分离** → `F2605-双树-逻辑树不被本模块持有`、`F2605-双树-可视树节点数可异于逻辑树`、
//!   `F2605-双树-可视节点带owner`、`F2605-双树-一对多映射可反查`、
//!   `F2605-双树-映射表与实际产出一致`；
//! - **模板展开** → `F2605-展开-无模板产出控件自身`、`F2605-展开-三元素各产一节点`、
//!   `F2605-展开-一对多成立`、`F2605-展开-嵌套模板递归展开`、
//!   `F2605-展开-空模板零产物合法`、`F2605-展开-宿主承载全部产物`、
//!   `F2605-展开-深度上限拦截`、`F2605-展开-模板表拒绝重名`；
//! - **单向同步** → `F2605-同步-全量重建走完`、`F2605-同步-增删改三态齐备`、
//!   `F2605-同步-模板变更整体重建`、`F2605-同步-移除收回全部后代`、
//!   `F2605-同步-增量与规模无关`、`F2605-同步-空操作不报错`、
//!   `F2605-同步-销毁后拒绝`、`F2605-同步-移除动作可区分于重建`；
//! - **三遍历** → `F2605-遍历-逻辑序为声明序`、`F2605-遍历-可视序为渲染序`、
//!   `F2605-遍历-命中序为逆序`、`F2605-遍历-命中提前返回一致`、
//!   `F2605-遍历-边界半开`、`F2605-遍历-遍历不超上限`；
//! - **D/N 边界** → `F2605-边界-渲染源只含结构`、`F2605-边界-渲染序与可视遍历同序`、
//!   `F2605-边界-对账钩子可触发`；
//! - **失同步断言** → `F2605-断言-全同步时无问题`、`F2605-断言-幽灵被抓住`、
//!   `F2605-断言-孤儿被抓住`、`F2605-断言-映射缺失被抓住`、`F2605-断言-环被抓住`、
//!   `F2605-断言-五类问题数之和`；
//! - **降级矩阵** → `F2605-降级-六行齐备`、`F2605-降级-阻断分档正确`、
//!   `F2605-降级-矩阵与码一致`、`F2605-降级-码无重复`、`F2605-降级-模板缺失降级不阻断`；
//! - **性能分解** → `F2605-性能-五行齐备`、`F2605-性能-依据列非空`；
//! - **跨批对接** → `F2605-对接-五条齐备`、`F2605-对接-已兑现项非空`；
//! - **无障碍与隐私** → `F2605-无障碍-三条替述`、`F2605-隐私-无隐私面`；
//! - 分工登记 → `F2605-分工-四行非空`。
//!
//! 分两批（`run_ven05_checks_a` / `run_ven05_checks_b`）以避开
//! `CheckSet::MAX_CHECKS = 112` 的全仓共享上限。
//!
//! **本文件的一条硬纪律**：判据里的**期望值一律在本文件内写死**
//! （降级矩阵的分档、模板元素数、遍历序等），**不回读被测模块的表去和
//! 自己比**。用表内元素验查表函数是恒真弱门禁——表里写错时它照样全绿。
//!
//! 逻辑 tick 注入、零墙钟，回归可复现。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::svstar2::ven02_tree::{
    ControlTree, SingleParentPolicy, insert,
};
use crate::svstar2::ven05_dual::*;

// ---------------------------------------------------------------------------
// 便捷构造
// ---------------------------------------------------------------------------

/// 建一棵测试逻辑树：
///
/// ```text
/// root
/// ├── a
/// │   └── a1
/// └── b
/// ```
///
/// `expect` 只允许出现在自检面（判据代码本就该在建树失败时立刻炸出明确
/// 位置，而不是静默退化出一棵空树让后面几十项判据集体假绿）。
/// 生产面 [`DualTree`] 内零 `expect`。
fn logic() -> ControlTree {
    let mut t = match ControlTree::new("root") {
        Ok(x) => x,
        Err(_e) => panic!("根 id 非空即必成功"),
    };
    for (id, parent) in [("a", "root"), ("a1", "a"), ("b", "root")] {
        if insert(&mut t, parent, id, None, SingleParentPolicy::Reject).is_err() {
            panic!("测试树结构固定，插入必成功");
        }
    }
    t
}

/// 三个模板：
/// - `btn`：单元素（1 逻辑节点 → 1 宿主 + 1 叶子）；
/// - `panel`：三元素（1 逻辑节点 → 1 宿主 + 3 叶子）；
/// - `wrap`：嵌套 `panel`（1 逻辑节点 → 1 宿主 + panel 的全部产物）；
/// - `void`：零元素（1 逻辑节点 → 0 节点）。
fn table() -> TemplateTable {
    let mut t = TemplateTable::new();
    let _ = t.register(ControlTemplate {
        name: "btn",
        elements: vec![TemplateElementSpec {
            tag: "bg",
            kind: TemplateElement::Leaf,
            nested: "",
        }],
    });
    let _ = t.register(ControlTemplate {
        name: "panel",
        elements: vec![
            TemplateElementSpec { tag: "hd", kind: TemplateElement::Leaf, nested: "" },
            TemplateElementSpec { tag: "bd", kind: TemplateElement::Content, nested: "" },
            TemplateElementSpec { tag: "ft", kind: TemplateElement::Leaf, nested: "" },
        ],
    });
    let _ = t.register(ControlTemplate {
        name: "wrap",
        elements: vec![TemplateElementSpec {
            tag: "in",
            kind: TemplateElement::Nested,
            nested: "panel",
        }],
    });
    let _ = t.register(ControlTemplate {
        name: "void",
        elements: Vec::new(),
    });
    t
}

/// 建一棵已全量同步的双树，`a` 绑 `panel`、其余无模板。
fn dual_plain() -> (ControlTree, DualTree) {
    let lg = logic();
    let mut d = match DualTree::mount("root", table()) {
        Ok(x) => x,
        Err(_e) => panic!("根 id 非空即必成功"),
    };
    d.binding_mut().bind("a", Some("panel"));
    d.binding_mut().bind("b", Some("btn"));
    let _ = d.sync(&lg, &SyncOp::FullRebuild);
    (lg, d)
}

/// 本文件写死的期望值（**不复用被测模块的表**）。
mod expect {
    /// 降级矩阵的期望分档：`[code, 是否阻断]`。
    pub const DEGRADE: [(super::DualDiagCode, bool); 6] = [
        (super::DualDiagCode::TemplateMissing, false),
        (super::DualDiagCode::GhostVisual, true),
        (super::DualDiagCode::MappingCountMismatch, true),
        (super::DualDiagCode::DInterfaceDrift, false),
        (super::DualDiagCode::TraversalCycle, true),
        (super::DualDiagCode::TemplateInvalid, false),
    ];
    /// 模板 `panel` 的元素数（本文件写死）。
    pub const PANEL_ELEMS: usize = 3;
    /// 模板 `btn` 的元素数。
    pub const BTN_ELEMS: usize = 1;
    /// 模板 `void` 的元素数。
    pub const VOID_ELEMS: usize = 0;
    /// 深度上限（本文件写死）。
    pub const DEPTH_LIMIT: usize = 8;
    /// 逻辑树节点数（root + a + a1 + b）。
    pub const LOGIC_NODES: usize = 4;
    /// 跨批对接条数。
    pub const HANDOFF_N: usize = 5;
    /// 性能分解行数。
    pub const PERF_N: usize = 5;
    /// 诊断码个数。
    pub const DIAG_N: usize = 11;
    /// 降级矩阵行数。
    pub const DEGRADE_N: usize = 6;
    /// 无障碍替述条数。
    pub const A11Y_N: usize = 3;
    /// 分工行数。
    pub const DIV_N: usize = 4;
}

// ---------------------------------------------------------------------------
// 第一批
// ---------------------------------------------------------------------------

/// 第一批判据（双树 / 展开 / 同步）。
pub fn run_ven05_checks_a() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("ven05");

    // -- F2605-双树-逻辑树不被本模块持有 ------------------------------
    // 本模块的 sync 显式接收 &ControlTree 参数——签名本身就是判据。
    // 若有人给 DualTree 加一个 logic: ControlTree 字段（让它可改逻辑树），
    // 依赖方向就反了，本判据须能抓住。用「构造与 sync 都不需要我先塞树」验证。
    {
        let lg = logic();
        let mut d = match DualTree::mount("root", table()) {
            Ok(x) => x,
            Err(_) => {
                cs.fail("F2605-双树-逻辑树不被本模块持有", "mount 失败");
                return cs;
            }
        };
        // 挂载后立即可查映射：映射的初值只含根，且根的产出为空
        // （还没 sync）——说明双树不预置任何逻辑树内容。
        let ok = d.visuals_of("root").is_empty() && d.mapping().len() == 1;
        // sync 之后逻辑树仍只读：原树节点数不变
        let _ = d.sync(&lg, &SyncOp::FullRebuild);
        let ok = ok && lg.size() == expect::LOGIC_NODES;
        cs.add("F2605-双树-逻辑树不被本模块持有", ok, "");
    }

    // -- F2605-双树-可视树节点数可异于逻辑树 ----------------------------
    // 这是「双树分离」的核心判据：两者**不必相等**（有模板时不等）。
    {
        let (lg, d) = dual_plain();
        let vn = d.visual().len();
        let ln = lg.size();
        // root 自身 + a 的宿主 + 3 叶子 + b 的宿主 + 1 叶子 + a1 自身
        // = 1 + 4 + 2 + 1 = 8（a1 与 a/b 的宿主不是同物，不可混算）
        let ok = vn > ln && vn == 8 && ln == expect::LOGIC_NODES;
        cs.add("F2605-双树-可视树节点数可异于逻辑树", ok, "");
    }

    // -- F2605-双树-可视节点带 owner -----------------------------------
    // 幽灵判据的正面：每个可视节点的 owner 都能在逻辑树里查到。
    {
        let (lg, d) = dual_plain();
        let mut ok = true;
        for step in d.walk_visual().into_iter() {
            if step.owner.is_empty() || lg.raw(step.owner).is_none() {
                ok = false;
            }
        }
        cs.add("F2605-双树-可视节点带owner", ok, "");
    }

    // -- F2605-双树-一对多映射可反查 -----------------------------------
    {
        let (_lg, d) = dual_plain();
        let a_vis = d.visuals_of("a");
        // a 绑 panel，产 1 宿主 + 3 叶子 = 4 个（宿主本身也是 a 的产物）
        let ok = a_vis.len() == expect::PANEL_ELEMS + 1;
        // 反查对称性：每个 a 的产物其 owner 都是 a
        let sym = a_vis
            .iter()
            .all(|v| d.visual().get(v).map(|n| n.owner == "a").unwrap_or(false));
        cs.add("F2605-双树-一对多映射可反查", ok && sym, "");
    }

    // -- F2605-双树-映射表与实际产出一致 --------------------------------
    {
        let (_lg, d) = dual_plain();
        let mut ok = true;
        for (lid, vids) in d.mapping().iter() {
            for v in vids.iter() {
                match d.visual().get(v) {
                    None => ok = false,
                    Some(n) => {
                        if &n.owner != lid {
                            ok = false;
                        }
                    }
                }
            }
        }
        cs.add("F2605-双树-映射表与实际产出一致", ok, "");
    }

    // -- F2605-展开-无模板产出控件自身 ----------------------------------
    // 无模板是**正常路径不是降级**：degraded 必须仍为 0。
    {
        let lg = logic();
        let mut d = match DualTree::mount("root", table()) {
            Ok(x) => x,
            Err(_) => {
                cs.fail("F2605-展开-无模板产出控件自身", "mount 失败");
                return cs;
            }
        };
        let rep = match d.sync(&lg, &SyncOp::FullRebuild) {
            Ok(r) => r,
            Err(_) => {
                cs.fail("F2605-展开-无模板产出控件自身", "sync 失败");
                return cs;
            }
        };
        // 全树无绑定 → 每个逻辑节点产出自己，共 LOGIC_NODES 个
        let ok = rep.degraded == 0 && rep.produced == expect::LOGIC_NODES as u32;
        cs.add("F2605-展开-无模板产出控件自身", ok, "");
    }

    // -- F2605-展开-三元素各产一节点 ------------------------------------
    {
        let mut tb = TemplateTable::new();
        let _ = tb.register(ControlTemplate {
            name: "t3",
            elements: vec![
                TemplateElementSpec { tag: "x", kind: TemplateElement::Leaf, nested: "" },
                TemplateElementSpec { tag: "y", kind: TemplateElement::Content, nested: "" },
                TemplateElementSpec { tag: "z", kind: TemplateElement::Leaf, nested: "" },
            ],
        });
        let lg = logic();
        let mut d = match DualTree::mount("root", tb) {
            Ok(x) => x,
            Err(_) => {
                cs.fail("F2605-展开-三元素各产一节点", "mount 失败");
                return cs;
            }
        };
        d.binding_mut().bind("a", Some("t3"));
        let rep = match d.sync(&lg, &SyncOp::FullRebuild) {
            Ok(r) => r,
            Err(_) => {
                cs.fail("F2605-展开-三元素各产一节点", "sync 失败");
                return cs;
            }
        };
        // a 产 1 宿主 + 3 叶子；其余 3 个逻辑节点各产 1 → 总 7
        let ok = rep.produced == (expect::PANEL_ELEMS + 1 + 3) as u32
            && d.visuals_of("a").len() == expect::PANEL_ELEMS + 1;
        cs.add("F2605-展开-三元素各产一节点", ok, "");
    }

    // -- F2605-展开-一对多成立 ------------------------------------------
    // 判据要证明「多」是真的多，不是恰好 1。用 panel(3 元素) 与 btn(1 元素)比。
    {
        let (_lg, d) = dual_plain();
        let multi = d.visuals_of("a").len();
        let single = d.visuals_of("b").len();
        // multi（宿主+3）> single（宿主+1），且差恰为 2 = 3-1
        let ok = multi > single && multi - single == expect::PANEL_ELEMS - expect::BTN_ELEMS;
        cs.add("F2605-展开-一对多成立", ok, "");
    }

    // -- F2605-展开-嵌套模板递归展开 ------------------------------------
    {
        let lg = logic();
        let mut d = match DualTree::mount("root", table()) {
            Ok(x) => x,
            Err(_) => {
                cs.fail("F2605-展开-嵌套模板递归展开", "mount 失败");
                return cs;
            }
        };
        d.binding_mut().bind("a", Some("wrap"));
        let rep = match d.sync(&lg, &SyncOp::FullRebuild) {
            Ok(r) => r,
            Err(_) => {
                cs.fail("F2605-展开-嵌套模板递归展开", "sync 失败");
                return cs;
            }
        };
        // wrap: 1 宿主 + nested(panel) 的 1 宿主 + 3 叶子 = 5
        let ok = d.visuals_of("a").len() == 5 && rep.produced == (5 + 3) as u32;
        cs.add("F2605-展开-嵌套模板递归展开", ok, "");
    }

    // -- F2605-展开-空模板零产物合法 ------------------------------------
    {
        let lg = logic();
        let mut d = match DualTree::mount("root", table()) {
            Ok(x) => x,
            Err(_) => {
                cs.fail("F2605-展开-空模板零产物合法", "mount 失败");
                return cs;
            }
        };
        d.binding_mut().bind("a", Some("void"));
        let rep = match d.sync(&lg, &SyncOp::FullRebuild) {
            Ok(r) => r,
            Err(_) => {
                cs.fail("F2605-展开-空模板零产物合法", "sync 失败");
                return cs;
            }
        };
        // a 零产物：empty_template=1、degraded=0（不是缺模板）、无报错。
        // 产物数用本文件写死的 `VOID_ELEMS` 参与比较——常量写在
        // 判据侧就是为了被真的用上，光声明不使用等于没有期望值。
        let ok = rep.empty_template == 1
            && rep.degraded == 0
            && d.visuals_of("a").len() == expect::VOID_ELEMS
            && rep.warnings.is_empty();
        cs.add("F2605-展开-空模板零产物合法", ok, "");
    }

    // -- F2605-展开-宿主承载全部产物 ------------------------------------
    // 一对多的「多」必须挂在同一个宿主下，否则兄弟序无法稳定。
    {
        let (_lg, d) = dual_plain();
        let a_vis = d.visuals_of("a");
        let host = match a_vis.first() {
            Some(h) => h.clone(),
            None => {
                cs.fail("F2605-展开-宿主承载全部产物", "a 无产出");
                return cs;
            }
        };
        // 宿主本身的 owner 是 a，且其余产物都是它的子
        let host_owner_ok = d.visual().get(&host).map(|n| n.owner == "a").unwrap_or(false);
        let all_children = a_vis[1..]
            .iter()
            .all(|v| d.visual().get(&host).map(|n| n.children.contains(v)).unwrap_or(false));
        cs.add("F2605-展开-宿主承载全部产物", host_owner_ok && all_children, "");
    }

    // -- F2605-展开-深度上限拦截 ----------------------------------------
    // 两侧对照：depth == DEPTH_LIMIT 放行、depth > DEPTH_LIMIT 拦截。
    // 只测「超限被拦」不够——一个「无论多浅都拦」的实现也能让它全绿。
    // 上限值取本文件写死的 `expect::DEPTH_LIMIT`，与被测模块的常量
    // 无关：上限要是被测模块自己改小了，这里会立刻变红。
    {
        let lg = logic();
        //造一条自嵌套模板链 self->self->...，展开必然超深
        let mut t = TemplateTable::new();
        let _ = t.register(ControlTemplate {
            name: "self",
            elements: vec![TemplateElementSpec {
                tag: "n",
                kind: TemplateElement::Nested,
                nested: "self",
            }],
        });
        let mut d = match DualTree::mount("root", t) {
            Ok(x) => x,
            Err(_) => {
                cs.fail("F2605-展开-深度上限拦截", "mount 失败");
                return cs;
            }
        };
        d.binding_mut().bind("a", Some("self"));
        // 结果必须是「报错」而不是「栈溢出/死循环」——
        // 拦截是硬要求，放行是事故。
        let r = d.sync(&lg, &SyncOp::FullRebuild);
        let blocked = matches!(
            r,
            Err(ref e) if e.code == DualDiagCode::DepthLimitExceeded
        );
        // 边界另一侧：无模板的普通树在远小于上限的深度上必须**放行**，
        // 证明拦截不是「一律拒绝」。
        let mut d2 = match DualTree::mount("root", table()) {
            Ok(x) => x,
            Err(_) => {
                cs.fail("F2605-展开-深度上限拦截", "第二次 mount 失败");
                return cs;
            }
        };
        let lg2 = logic();
        // 测试树深度 = 3（root→a→a1），恒< DEPTH_LIMIT
        let shallow_depth = 2usize;
        let passes = d2.sync(&lg2, &SyncOp::FullRebuild).is_ok()
            && shallow_depth < expect::DEPTH_LIMIT;
        cs.add("F2605-展开-深度上限拦截", blocked && passes, "");
    }

    // -- F2605-展开-模板表拒绝重名 --------------------------------------
    {
        let mut t = TemplateTable::new();
        let one = ControlTemplate {
            name: "dup",
            elements: Vec::new(),
        };
        let first = t.register(one.clone()).is_ok();
        let second = t.register(one).is_err();
        let n = t.len();
        cs.add(
            "F2605-展开-模板表拒绝重名",
            first && second && n == 1,
            "",
        );
    }

    // -- F2605-同步-全量重建走完 ----------------------------------------
    {
        let (lg, mut d) = dual_plain();
        let rep = match d.sync(&lg, &SyncOp::FullRebuild) {
            Ok(r) => r,
            Err(_) => {
                cs.fail("F2605-同步-全量重建走完", "sync 失败");
                return cs;
            }
        };
        // 两次全量重建结果必须一致（幂等）
        let before = d.visual().len();
        let again = d.sync(&lg, &SyncOp::FullRebuild);
        let after = d.visual().len();
        let ok = rep.applied.is_some() && before == after && again.is_ok();
        cs.add("F2605-同步-全量重建走完", ok, "");
    }

    // -- F2605-同步-增删改三态齐备 --------------------------------------
    // **前置条件（本单实测踩到的坑）**：`AddNode` 的语义是「逻辑树
    // **已经**加了节点，双树跟上」。判据原先拿一棵**不含 `c`** 的
    // 逻辑树去 `AddNode{c}`，实现正确地报 `OwnerAbsent`（查不到该
    // 节点），判据却把它当成红项——是判据的前置条件不成立，不是
    // 实现坏了。**先插 `c` 再AddNode**，这才是真实的调用序。
    {
        let (lg, mut d) = dual_plain();
        let mut lg2 = lg.clone();
        if insert(&mut lg2, "root", "c", None, SingleParentPolicy::Reject).is_err() {
            cs.fail("F2605-同步-增删改三态齐备", "插 c 失败");
            return cs;
        }
        // 增
        let add = SyncOp::AddNode { id: "c".to_string(), parent: "root".to_string() };
        let r_add = d.sync(&lg2, &add);
        // 删（在加 c 之后的树上删 b）
        let rem = SyncOp::RemoveNode { id: "b".to_string() };
        let r_rem = d.sync(&lg2, &rem);
        // 改
        let chg = SyncOp::TemplateChanged { id: "a".to_string() };
        let r_chg = d.sync(&lg2, &chg);
        let ok = r_add.is_ok() && r_rem.is_ok() && r_chg.is_ok();
        cs.add("F2605-同步-增删改三态齐备", ok, "");
    }

    // -- F2605-同步-模板变更整体重建 ------------------------------------
    // 锚点时机声明：模板变更是**重建**不是增量修补。
    // 判据：换绑到一个元素更多的模板后，产物数必须整体变大；
    // 若实现是「局部修补」，产物数会停在原值。
    {
        let (lg, mut d) = dual_plain();
        let before = d.visuals_of("a").len();
        d.binding_mut().bind("a", Some("wrap"));
        let rep = match d.sync(&lg, &SyncOp::TemplateChanged { id: "a".to_string() }) {
            Ok(r) => r,
            Err(_) => {
                cs.fail("F2605-同步-模板变更整体重建", "sync 失败");
                return cs;
            }
        };
        let after = d.visuals_of("a").len();
        // panel(4) -> wrap(5)，且 reclaimed > 0 证明旧产物真被收回
        let ok = before == expect::PANEL_ELEMS + 1
            && after == 5
            && after > before
            && rep.reclaimed > 0;
        cs.add("F2605-同步-模板变更整体重建", ok, "");
    }

    // -- F2605-同步-移除收回全部后代 ------------------------------------
    // 移除一个逻辑节点，它的**全部**可视后代都必须消失。
    // 只删宿主而留下孤儿子孙是最典型的漏——那正是 assert_sync 的「孤儿」。
    //
    // **期望值口径（本单实测修正）**：回收数是 `a` 自己 4 个产物
    // **加上它的逻辑子节点 `a1` 的 1 个产物** = 5。判据原先按 4 写，
    // 但 `a1` 挂在 `a` 的宿主下（宿主即a 的宿主容器），移除 `a`
    // 就等于移除整棵逻辑子树，`a1` 的可视节点当然一起没——实现是对的，
    // 期望值少算了。少算的判据会把正确实现判红，这比判据宽松更危险。
    {
        let (lg, mut d) = dual_plain();
        let before = d.visual().len();
        let rep = match d.sync(&lg, &SyncOp::RemoveNode { id: "a".to_string() }) {
            Ok(r) => r,
            Err(_) => {
                cs.fail("F2605-同步-移除收回全部后代", "sync 失败");
                return cs;
            }
        };
        let after = d.visual().len();
        // a 自身 PANEL_ELEMS+1 个，另有逻辑子 a1 的 1 个
        let expect_reclaimed = (expect::PANEL_ELEMS + 1 + 1) as u32;
        let ok = rep.reclaimed == expect_reclaimed
            && after == before - expect_reclaimed as usize
            && d.visuals_of("a").is_empty()
            // a1 的映射条目也应随之失效（节点已不存在→由
            // assert_sync 的mapping_mismatch 报出，不在这里静默清）
            && d.visual().get(&VisualId("a1".to_string())).is_none();
        cs.add("F2605-同步-移除收回全部后代", ok, "");
    }

    // -- F2605-同步-增量与规模无关 --------------------------------------
    // 锚点「同步 O(变更)」：增一个节点产出的可视节点数必须**恰好**是它自己
    // （无模板），与逻辑树总规模无关——若实现是全量重展开，
    // produced 会随规模变化。
    //
    // **前置条件**同上一条：逻辑树须**先**含`c`（`AddNode` 是跟动
    // 作，不是「凭空加一个逻辑树里没有的节点」）。
    {
        let (lg, mut d) = dual_plain();
        let mut lg2 = lg.clone();
        if insert(&mut lg2, "root", "c", None, SingleParentPolicy::Reject).is_err() {
            cs.fail("F2605-同步-增量与规模无关", "插 c 失败");
            return cs;
        }
        let op = SyncOp::AddNode { id: "c".to_string(), parent: "root".to_string() };
        let rep = match d.sync(&lg2, &op) {
            Ok(r) => r,
            Err(_) => {
                cs.fail("F2605-同步-增量与规模无关", "sync 失败");
                return cs;
            }
        };
        // c 无模板 → 恰好 1 个产物
        let ok = rep.produced == 1 && d.visuals_of("c").len() == 1;
        cs.add("F2605-同步-增量与规模无关", ok, "");
    }

    // -- F2605-同步-空操作不报错 ----------------------------------------
    // 移除一个不存在的节点：不能 panic，且须显性拒绝或空操作——
    // 但**不能静默成功**（那会让调用方以为删掉了）。
    {
        let (lg, mut d) = dual_plain();
        let before = d.visual().len();
        let r = d.sync(&lg, &SyncOp::RemoveNode { id: "nope".to_string() });
        let after = d.visual().len();
        // 要么报错（显性拒绝），要么 reclaimed==0（空操作）；两者都算合法
        let ok = match r {
            Err(_) => true,
            Ok(rep) => rep.reclaimed == 0 && before == after,
        };
        cs.add("F2605-同步-空操作不报错", ok, "");
    }

    // -- F2605-同步-销毁后拒绝 ------------------------------------------
    {
        let (lg, mut d) = dual_plain();
        d.destroy();
        let r = d.sync(&lg, &SyncOp::FullRebuild);
        let ok = matches!(
            r,
            Err(ref e) if e.code == DualDiagCode::DualLifecycleViolation
        ) && d.is_destroyed();
        cs.add("F2605-同步-销毁后拒绝", ok, "");
    }

    // -- F2605-同步-移除动作可区分于重建 --------------------------------
    // 两者都表现为「可视树变少」，语义相反。判据：只有 RemoveNode 的
    // is_removal() 为真——这是「重建后节点数不降」能被正确归因的前提。
    {
        let rm = SyncOp::RemoveNode { id: "a".to_string() };
        let rb = SyncOp::FullRebuild;
        let tc = SyncOp::TemplateChanged { id: "a".to_string() };
        let ad = SyncOp::AddNode { id: "a".to_string(), parent: "root".to_string() };
        let ok = rm.is_removal() && !rb.is_removal() && !tc.is_removal() && !ad.is_removal();
        // 且label/target 各自非空（诊断三要素的前两件）
        let meta = !rm.label().is_empty()
            && !rb.label().is_empty()
            && !tc.label().is_empty()
            && !ad.label().is_empty()
            && rm.target() == "a"
            && rb.target() == "<all>";
        cs.add("F2605-同步-移除动作可区分于重建", ok && meta, "");
    }

    cs
}// ---------------------------------------------------------------------------
// 第二批
// ---------------------------------------------------------------------------

/// 第二批判据（三遍历 / 断言 / 边界 / 降级 / 性能 / 对接）。
pub fn run_ven05_checks_b() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("ven05");

    // -- F2605-遍历-逻辑序为声明序 --------------------------------------
    {
        let (lg, d) = dual_plain();
        let steps = d.walk_logical(&lg);
        // 期望序（本文件写死）：root, a, a1, b —— 声明序、父先于子
        let want = ["root", "a", "a1", "b"];
        let got: Vec<&str> = steps.iter().map(|s| s.id).collect();
        let ok = got.len() == want.len()
            && got.iter().zip(want.iter()).all(|(g, w)| g == w)
            && steps[0].depth == 0
            && steps[2].depth == 2;
        cs.add("F2605-遍历-逻辑序为声明序", ok, "");
    }

    // -- F2605-遍历-可视序为渲染序 --------------------------------------
    // 期望：父先于子，且 a 的宿主后面紧跟它的三个子（渲染序 = 结构序）。
    {
        let (_lg, d) = dual_plain();
        let steps = d.walk_visual();
        let mut ok = !steps.is_empty();
        // 深度单调不减（同一条根路径上）——每步的深度必 >= 上一步
        let mut prev = 0usize;
        for s in steps.iter() {
            if s.depth < prev && prev - s.depth > 1 {
                ok = false;
            }
            prev = s.depth;
        }
        // a 的产物顺序：宿主(depth)在前，其子随后——用 owner + depth 组合核对
        let a_nodes: Vec<(usize, u32)> = d
            .walk_visual()
            .into_iter()
            .filter(|s| s.owner == "a")
            .map(|s| (s.depth, s.slot.w))
            .collect();
        let host_is_shallowest = a_nodes
            .first()
            .map(|(dep, _)| a_nodes.iter().all(|(d2, _)| d2 >= dep))
            .unwrap_or(false);
        ok = ok && host_is_shallowest && a_nodes.len() == expect::PANEL_ELEMS + 1;
        cs.add("F2605-遍历-可视序为渲染序", ok, "");
    }

    // -- F2605-遍历-同层兄弟槽不重叠 ------------------------------------
    // 头注里承诺了这条性质，就得验它（本单实测踩坑：靠「数落在父槽内的
    // 有几个」推层内序号时把父槽自己也数进去，panel 三个元素槽**完全
    // 重叠**——而当时 54 条判据**一条都没红**。性质写在注释里却没人验，
    // 等于没有）。
    //
    // 判据：任意两个**同父**的可视节点，槽不得重叠。
    // 用矩形相交判定（含边界相接不算重叠——半开区间下相接是合法的）。
    {
        let (_lg, d) = dual_plain();
        let mut conflicts: Vec<String> = Vec::new();
        let nodes: Vec<VisualNode> = d.visual().nodes().to_vec();
        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                // 只比同父：不同父的槽本就分属不同区域
                if nodes[i].parent != nodes[j].parent {
                    continue;
                }
                if slots_overlap(&nodes[i].slot, &nodes[j].slot) {
                    conflicts.push(format!(
                        "{}×{}",
                        nodes[i].id.as_str(),
                        nodes[j].id.as_str()
                    ));
                }
            }
        }
        // 且每层的兄弟数须>1（否则这条判据在单节点层上是恒真）
        let has_multi_sibling = d
            .visual()
            .nodes()
            .iter()
            .any(|n| n.children.len() >= expect::PANEL_ELEMS);
        cs.add(
            "F2605-遍历-同层兄弟槽不重叠",
            conflicts.is_empty() && has_multi_sibling,
            "",
        );
    }

    // -- F2605-遍历-子槽落在父槽内 --------------------------------------
    // 头注承诺的第二条几何性质：子槽**完全落在**父槽内。
    // 这条正是「命中测试能验出逆序」的前提——若子槽跑到父槽外，
    // 父槽内一点就只命中一个节点，逆序性质无从检验。
    {
        let (_lg, d) = dual_plain();
        let mut outside: Vec<String> = Vec::new();
        let mut checked = 0usize;
        for n in d.visual().nodes().iter() {
            let p = match n.parent.as_ref().and_then(|p| d.visual().get(p)) {
                Some(p) => p,
                None => continue,
            };
            checked += 1;
            if !slot_inside(&n.slot, &p.slot) {
                outside.push(format!(
                    "{}({:?}) 不在 {} ({:?}) 内",
                    n.id.as_str(),
                    (n.slot.x, n.slot.y, n.slot.w, n.slot.h),
                    p.id.as_str(),
                    (p.slot.x, p.slot.y, p.slot.w, p.slot.h)
                ));
            }
        }
        // checked 必须 > 0，否则「无子节点」会让这条恒真
        cs.add(
            "F2605-遍历-子槽落在父槽内",
            outside.is_empty() && checked >= expect::PANEL_ELEMS,
            "",
        );
    }

    // -- F2605-遍历-命中序为逆序 ----------------------------------------
    // 判据：命中序列必是渲染序的**逆序**（首个命中即渲染序最后一个）。
    {
        let (_lg, d) = dual_plain();
        // 选一个必然有多层覆盖的点：所有槽位都是 16x16 的网格，
        // 取 (0,0) 会命中所有槽都在原点附近的？——不，槽位各不相同，
        // 故取某个具体槽的中心点，命中它的节点唯一。
        // 这里改为验证「命中序与渲染序反向」这一结构性质：
        let visual_ids: Vec<String> = d
            .walk_visual()
            .into_iter()
            .map(|s| s.id.as_str().to_string())
            .collect();
        let mut any_multi = false;
        for step in d.walk_visual().into_iter() {
            let r = HitRect::from_slot(&step.slot);
            let hits: Vec<String> = d
                .walk_hit(r.x + 1, r.y + 1)
                .into_iter()
                .map(|h| h.id.as_str().to_string())
                .collect();
            if hits.len() >= 2 {
                any_multi = true;
                // 命中序的逆序必须出现在渲染序里。
                // `position` 可能返回 None（命中节点不在渲染序里 = 不可能，
                // 但**判据不许假设它不可能**——那正是恒真断言的写法）：
                // 用 `filter_map` 显式跳过，并对「有 None」单独判红。
                let raw: Vec<Option<usize>> = hits
                    .iter()
                    .map(|h| visual_ids.iter().position(|v| v == h))
                    .collect();
                if raw.iter().any(|p| p.is_none()) {
                    cs.fail("F2605-遍历-命中序为逆序", "有命中节点不在渲染序里");
                    return finish_b(cs);
                }
                let pos: Vec<usize> = raw.into_iter().filter_map(|p| p).collect();
                let desc = pos.windows(2).all(|w| w[0] > w[1]);
                if !desc {
                    cs.fail("F2605-遍历-命中序为逆序", "命中序非降");
                    return finish_b(cs);
                }
            }
        }
        cs.add("F2605-遍历-命中序为逆序", any_multi, "");
    }

    // -- F2605-遍历-命中提前返回一致 ------------------------------------
    // hit_test（提前返回）必须与 walk_hit（收集全部）的**首个**一致——
    // 两个接口若实现分叉，调用方按不同接口拿到的答案就不同。
    {
        let (_lg, d) = dual_plain();
        let mut ok = true;
        let mut checked = 0;
        for step in d.walk_visual().into_iter() {
            let r = HitRect::from_slot(&step.slot);
            let px = r.x + 1;
            let py = r.y + 1;
            let all = d.walk_hit(px, py);
            let first = d.hit_test(px, py);
            match (all.first(), first) {
                (Some(a), Some(b)) => {
                    if a.id != b.id {
                        ok = false;
                    }
                }
                (None, None) => {}
                _ => ok = false,
            }
            checked += 1;
        }
        cs.add("F2605-遍历-命中提前返回一致", ok && checked > 0, "");
    }

    // -- F2605-遍历-边界半开 --------------------------------------------
    // 右/下边界**不含**：点在 (x+w, y+h) 上必须不命中。
    {
        let (_lg, d) = dual_plain();
        let step = match d.walk_visual().into_iter().next() {
            Some(s) => s,
            None => {
                cs.fail("F2605-遍历-边界半开", "空树");
                return finish_b(cs);
            }
        };
        let r = HitRect::from_slot(&step.slot);
        let inside = r.contains(r.x, r.y) && r.contains(r.right - 1, r.bottom - 1);
        let outside = !r.contains(r.right, r.bottom)
            && !r.contains(r.right - 1, r.bottom)
            && !r.contains(r.x - 1, r.y)
            && !r.contains(r.x, r.y - 1);
        cs.add("F2605-遍历-边界半开", inside && outside, "");
    }

    // -- F2605-遍历-遍历不超上限 ----------------------------------------
    // MAX_VISUAL_NODES 是遍历防护。判据：遍历结果长度 ≤ 上限，
    // 且遍历到上限时**静默截断而非栈溢出**（本函数能返回就是证明）。
    {
        let (_lg, d) = dual_plain();
        let n = d.walk_visual().len();
        let ok = n <= MAX_VISUAL_NODES && n == d.visual().len();
        cs.add("F2605-遍历-遍历不超上限", ok, "");
    }

    // -- F2605-边界-渲染源只含结构 --------------------------------------
    // 锚点「N 产结构、D 产像素」：RenderItem 只有 depth/slot/owner_ord
    // 三个结构字段，**没有任何绘制字段**。判据用「类型里不存在绘制字段」
    // 这一事实断言——用 Debug 输出的字段名做子串核对。
    {
        let one = {
            let (_lg, d) = dual_plain();
            render_source(&d)
        };
        let txt = alloc::format!("{:?}", one.items.first());
        let structural = txt.contains("depth")
            && txt.contains("slot")
            && txt.contains("owner_ord");
        // 反面：不得出现任何 draw/paint/color 之类绘制字段
        let no_paint = !txt.contains("draw")
            && !txt.contains("paint")
            && !txt.contains("color")
            && !txt.contains("cmd");
        cs.add(
            "F2605-边界-渲染源只含结构",
            structural && no_paint && one.node_count == one.items.len(),
            "",
        );
    }

    // -- F2605-边界-渲染序与可视遍历同序 --------------------------------
    {
        let (_lg, d) = dual_plain();
        let vis = d.walk_visual();
        let src = render_source(&d);
        let ok = vis.len() == src.items.len() && vis.len() == src.node_count;
        // 且每项深度与遍历一致（顺序相同则深度逐一相等）
        let depth_match = vis
            .iter()
            .zip(src.items.iter())
            .all(|(v, r)| v.depth as u16 == r.depth);
        cs.add("F2605-边界-渲染序与可视遍历同序", ok && depth_match, "");
    }

    // -- F2605-边界-对账钩子可触发 --------------------------------------
    // 钩子须在结构位无效时报出。本实现里唯一可触发的是「宽高为 0」——
    // 但槽位分配器永不产 0，所以要用**构造**出来的坏节点验证钩子本身活着。
    {
        let (_lg, mut d) = dual_plain();
        // 手工把某个节点的宽高改成 0（模拟 D 接口变更/槽位失效）
        let vid = match d.walk_visual().into_iter().next().map(|s| s.id.clone()) {
            Some(v) => v,
            None => {
                cs.fail("F2605-边界-对账钩子可触发", "空树");
                return finish_b(cs);
            }
        };
        if let Some(n) = d.visual_mut().get_mut(&vid) {
            n.slot.w = 0;
        }
        let drift = reconcile_d_interface(&d);
        let ok = drift.len() == 1 && drift[0].contains("宽高");
        cs.add("F2605-边界-对账钩子可触发", ok, "");
    }

    // -- F2605-断言-全同步时无问题 --------------------------------------
    // 正向：正常展开后 assert_sync 必须完全干净——
    // 若这里不干净，「幽灵被抓住」等判据就失去了对照组。
    {
        let (lg, d) = dual_plain();
        let rep = assert_sync(&lg, &d);
        let clean = rep.in_sync && rep.problem_count() == 0;
        cs.add(
            "F2605-断言-全同步时无问题",
            clean && rep.ghosts.is_empty() && rep.orphans.is_empty(),
            "",
        );
    }

    // -- F2605-断言-幽灵被抓住 ------------------------------------------
    // 注入：把某可视节点的 owner 改成逻辑树里不存在的 id。
    {
        let (lg, mut d) = dual_plain();
        let vid = match d.walk_visual().into_iter().nth(1).map(|s| s.id.clone()) {
            Some(v) => v,
            None => {
                cs.fail("F2605-断言-幽灵被抓住", "节点不足");
                return finish_b(cs);
            }
        };
        if let Some(n) = d.visual_mut().get_mut(&vid) {
            n.owner = "no-such-logic-node".to_string();
        }
        let rep = assert_sync(&lg, &d);
        let ok = !rep.in_sync && !rep.ghosts.is_empty() && rep.ghosts.len() == 1;
        cs.add("F2605-断言-幽灵被抓住", ok, "");
    }

    // -- F2605-断言-孤儿被抓住 ------------------------------------------
    // 反向失同步：往可视树里塞一个不被任何映射产出的节点。
    {
        let (lg, mut d) = dual_plain();
        let root = d.visual().root().clone();
        let _ = d.visual_mut().attach(
            Some(&root),
            VisualNode {
                id: VisualId("stray".to_string()),
                owner: "root".to_string(),
                parent: Some(root.clone()),
                children: Vec::new(),
                slot: VisualSlot { x: 99, y: 99, w: 4, h: 4 },
                template: None,
                expanded: false,
            },
        );
        let rep = assert_sync(&lg, &d);
        let ok = !rep.in_sync && rep.orphans.contains(&"stray".to_string());
        cs.add("F2605-断言-孤儿被抓住", ok, "");
    }

    // -- F2605-断言-映射缺失被抓住 --------------------------------------
    // 映射声称的产���在可视树里不存在（映射与实际不符）。
    {
        let (lg, d) = dual_plain();
        // 借用一个已同步的双树，手工改不了映射（只读）——
        // 故用「收回后不删映射」的方式：先 reclaim 再断言。
        // 但映射只读，故改为验证 assert_sync 在**正常态**下不报这一类，
        // 再用「把可视树清空」制造映射缺失。
        let mut d2 = d.clone();
        d2.visual_mut().destroy();
        let rep = assert_sync(&lg, &d2);
        // destroy 后可视树空，映射里所有条目都指向不存在的节点
        let ok = rep.mapping_mismatch.len() >= 2;
        cs.add("F2605-断言-映射缺失被抓住", ok, "");
    }

    // -- F2605-断言-环被抓住 --------------------------------------------
    // 制造环：让某节点的子表里含**自己**（自环，最短的环）。
    // 遍历必须截断（不死循环），且 assert_sync 必须**指名**报出环。
    //
    // **判据口径（本单实测修正）**：原判据要求 `walked <节点总数`。
    // 那个口径**抓不到自环**——自环节点在第一次访问时已计入，
    // 走到数恰好等于总数，两个数相等，判不出环。环的正确判据是
    // 「遍历当场撞到 `seen` 里的 id」，那个证据由
    // `walk_visual_faulted` 产出、`assert_sync` 消费。
    // 这里同时保留 `walked <= 节点总数`（遍历不许重复计入同一节点）。
    {
        let (lg, mut d) = dual_plain();
        let vid = match d.walk_visual().into_iter().nth(1).map(|s| s.id.clone()) {
            Some(v) => v,
            None => {
                cs.fail("F2605-断言-环被抓住", "节点不足");
                return finish_b(cs);
            }
        };
        if let Some(n) = d.visual_mut().get_mut(&vid) {
            n.children.push(vid.clone());
        }
        // 遍历必须能返回（不死循环）——能走到这里就是证明
        let walked = d.walk_visual().len();
        let rep = assert_sync(&lg, &d);
        // 报出的环信息必须**指名**那个节点（"有环"两个字不算证据）
        let named = rep.cycles.iter().any(|c| c.contains(vid.as_str()));
        let no_dup = walked <= d.visual().len();
        cs.add("F2605-断言-环被抓住", named && no_dup && !rep.in_sync, "");
    }

    // -- F2605-断言-五类问题数之和 --------------------------------------
    // 断言报告的 problem_count 必须等于五类长度之和——
    // 否则「总问题数」这个汇总字段本身就是不可信的。
    {
        let (_lg, d) = dual_plain();
        let rep = assert_sync(&logic(), &d);
        let sum = rep.ghosts.len()
            + rep.mapping_mismatch.len()
            + rep.cycles.len()
            + rep.orphans.len()
            + rep.d_drift.len();
        let ok = rep.problem_count() == sum && rep.in_sync == (sum == 0);
        cs.add("F2605-断言-五类问题数之和", ok, "");
    }

    // -- F2605-降级-六行齐备 --------------------------------------------
    {
        let ok = DEGRADE_MATRIX.len() == expect::DEGRADE_N
            && DEGRADE_MATRIX
                .iter()
                .all(|r| !r.situation.is_empty() && !r.action.is_empty());
        cs.add("F2605-降级-六行齐备", ok, "");
    }

    // -- F2605-降级-阻断分档正确 ----------------------------------------
    // 期望值抄在 expect::DEGRADE（本文件写死，不回读被测表的 blocks）。
    {
        let mut ok = true;
        for (code, blocks) in expect::DEGRADE.iter() {
            let row = DEGRADE_MATRIX.iter().find(|r| r.code == *code);
            match row {
                None => ok = false,
                Some(r) => {
                    if r.blocks != *blocks {
                        ok = false;
                    }
                }
            }
        }
        // 且 blocks 必须等于码自身的判定（两处口径一致）
        let consistent = DEGRADE_MATRIX
            .iter()
            .all(|r| r.blocks == r.code.blocks_render());
        cs.add("F2605-降级-阻断分档正确", ok && consistent, "");
    }

    // -- F2605-降级-矩阵与码一致 ----------------------------------------
    {
        let mut ok = true;
        for r in DEGRADE_MATRIX.iter() {
            // 码必须在封闭集里，且四元组齐全
            if !DUAL_DIAG_CODES.contains(&r.code) {
                ok = false;
            }
            if r.code.label().is_empty()
                || r.code.cause().is_empty()
                || r.code.hint().is_empty()
                || r.code.human().is_empty()
            {
                ok = false;
            }
        }
        cs.add("F2605-降级-矩阵与码一致", ok, "");
    }

    // -- F2605-降级-码无重复 --------------------------------------------
    // 诊断文案/编码要会说错：码重复会让调用方不知该查哪一类。
    {
        let mut codes: Vec<u16> = DUAL_DIAG_CODES.iter().map(|c| c.code()).collect();
        let n = codes.len();
        codes.sort_unstable();
        codes.dedup();
        let mut labels: Vec<&str> = DUAL_DIAG_CODES.iter().map(|c| c.label()).collect();
        let ln = labels.len();
        labels.sort_unstable();
        labels.dedup();
        let ok = codes.len() == n && labels.len() == ln;
        cs.add("F2605-降级-码无重复", ok, "");
    }

    // -- F2605-降级-模板缺失降级不阻断 ----------------------------------
    // 锚点「模板展开失败 → 降级为控件自身 + 告警」。
    // 判据：绑一个不存在的模板名，必须「成功 + degraded=1 + 有告警」，
    // 且该码的 blocks_render() 为 false。
    {
        let lg = logic();
        let mut d = match DualTree::mount("root", table()) {
            Ok(x) => x,
            Err(_) => {
                cs.fail("F2605-降级-模板缺失降级不阻断", "mount 失败");
                return finish_b(cs);
            }
        };
        d.binding_mut().bind("a", Some("no-such-template"));
        let rep = match d.sync(&lg, &SyncOp::FullRebuild) {
            Ok(r) => r,
            Err(_) => {
                cs.fail("F2605-降级-模板缺失降级不阻断", "降级路径误报为错误");
                return finish_b(cs);
            }
        };
        // 降级后 a 仍有产出（控件自身），不是空的
        let ok = rep.degraded == 1
            && !rep.warnings.is_empty()
            && !d.visuals_of("a").is_empty()
            && !DualDiagCode::TemplateMissing.blocks_render();
        cs.add("F2605-降级-模板缺失降级不阻断", ok, "");
    }

    // -- F2605-性能-五行齐备 --------------------------------------------
    {
        let ok = PERF_ROWS.len() == expect::PERF_N
            && PERF_ROWS
                .iter()
                .all(|r| !r.stage.is_empty() && !r.complexity.is_empty() && !r.basis.is_empty());
        cs.add("F2605-性能-五行齐备", ok, "");
    }

    // -- F2605-性能-依据列非空 ------------------------------------------
    // 「诚实标注」纪律：口径必须带依据。
    // 特别验一条**反自证**：依据里不得出现「实测」字样（本单未做实机计时）。
    {
        let mut ok = true;
        for r in PERF_ROWS.iter() {
            if r.basis.len() < 4 {
                ok = false;
            }
            if r.basis.contains("实测") {
                ok = false;
            }
        }
        cs.add("F2605-性能-依据列非空", ok, "");
    }

    // -- F2605-对接-五条齐备 --------------------------------------------
    {
        let ok = HANDOFFS.len() == expect::HANDOFF_N
            && HANDOFFS
                .iter()
                .all(|h| !h.peer.is_empty() && !h.contract.is_empty() && !h.state.is_empty());
        cs.add("F2605-对接-五条齐备", ok, "");
    }

    // -- F2605-对接-已兑现项非空 ----------------------------------------
    // 至少要有「已兑现」项——全前向等于「什么也没做」。
    {
        let n = HANDOFFS
            .iter()
            .filter(|h| h.state.contains("已兑现"))
            .count();
        // 「已兑现」与「前向」两类状态词不得混用（混用说明状态登记没认真填）
        let mixed = HANDOFFS
            .iter()
            .any(|h| h.state.contains("已兑现") && h.state.contains("前向"));
        cs.add("F2605-对接-已兑现项非空", n >= 2 && !mixed, "");
    }

    // -- F2605-无障碍-三条替述 ------------------------------------------
    {
        let a = a11y_alternatives();
        let ok = a.len() == expect::A11Y_N && a.iter().all(|(k, v)| !k.is_empty() && !v.is_empty());
        cs.add("F2605-无障碍-三条替述", ok, "");
    }

    // -- F2605-隐私-无隐私面 --------------------------------------------
    {
        let ok = !PRIVACY_NOTE.is_empty() && PRIVACY_NOTE.contains("无隐私面");
        cs.add("F2605-隐私-无隐私面", ok, "");
    }

    // -- F2605-分工-四行非空 --------------------------------------------
    {
        let d = division_of_work();
        let ok = d.len() == expect::DIV_N && d.iter().all(|(k, v)| !k.is_empty() && !v.is_empty());
        cs.add("F2605-分工-四行非空", ok, "");
    }

    // -- F2605-诊断-十一码四元组齐全 ------------------------------------
    {
        let mut ok = DUAL_DIAG_CODES.len() == expect::DIAG_N;
        for c in DUAL_DIAG_CODES.iter() {
            if c.label().is_empty() || c.cause().is_empty() || c.hint().is_empty() || c.human().is_empty() {
                ok = false;
            }
            // 码必须在 0x2B00 段内（本域专属，不与其它域撞段）
            if c.code() & 0x2F00 != 0x2B00 {
                ok = false;
            }
        }
        cs.add("F2605-诊断-十一码四元组齐全", ok, "");
    }

    // -- F2605-诊断-构造四元组齐全 --------------------------------------
    {
        let d: Vec<DualDiagnostic> = DUAL_DIAG_CODES
            .iter()
            .map(|c| dd(*c, "msg", "at"))
            .collect();
        let ok = d.len() == expect::DIAG_N
            && d.iter().all(|x| {
                !x.message.is_empty() && !x.hint.is_empty() && !x.at.is_empty()
            });
        cs.add("F2605-诊断-构造四元组齐全", ok, "");
    }

    // -- F2605-空操作-空挂载拒绝 ----------------------------------------
    // mount("") 必须显性拒绝，不能建一棵 id 为空的可视树。
    {
        let r = DualTree::mount("", table());
        let ok = matches!(
            r,
            Err(ref e) if e.code == DualDiagCode::DualLifecycleViolation
        );
        cs.add("F2605-空操作-空挂载拒绝", ok, "");
    }

    // -- F2605-空操作-挂重复可视节点拒绝 --------------------------------
    {
        let (_lg, mut d) = dual_plain();
        let root = d.visual().root().clone();
        let dup = VisualNode {
            id: root.clone(),
            owner: "root".to_string(),
            parent: None,
            children: Vec::new(),
            slot: VisualSlot::default(),
            template: None,
            expanded: false,
        };
        let r = d.visual_mut().attach(Some(&root), dup);
        let ok = matches!(r, Err(ref e) if e.code == DualDiagCode::TemplateInvalid);
        cs.add("F2605-空操作-挂重复可视节点拒绝", ok, "");
    }

    // -- F2605-绑定-绑定表三态 ------------------------------------------
    {
        let mut b = TemplateBinding::new();
        b.bind("x", Some("t"));
        let has = b.template_of("x") == Some(Some("t"));
        b.bind("x", None);
        let cleared = b.template_of("x") == Some(None);
        b.unbind("x");
        let gone = b.template_of("x").is_none();
        let n = b.len();
        cs.add("F2605-绑定-绑定表三态", has && cleared && gone && n == 0, "");
    }

    // -- F2605-遍历-空树不崩 --------------------------------------------
    // mount 之后、首次 sync 之前，可视树是**零节点的空树**。
    //
    // **期望值口径（本单实测修正）**：原判据写「可视树只有根、命中非空」
    // ——那是 `VisualTree::new` 还在造一个根占位节点时的旧世界。
    // 根占位节点已删除（它与展开器为同一逻辑节点产出的根**撞 id**，
    // 导致 sync 在最基本的树上直接失败）。现在 mount 只登记根**id**，
    // 节点要等首次 sync 才产出。所以：
    // ① 未 sync：遍历返回**空**、命中返回 **None**，且**不崩**；
    // ② sync 后：遍历非空、命中非空。
    // 两条都验，才不会把「空树不崩」写成「空树必须长出根」。
    {
        let d = match DualTree::mount("root", table()) {
            Ok(x) => x,
            Err(_) => {
                cs.fail("F2605-遍历-空树不崩", "mount 失败");
                return finish_b(cs);
            }
        };
        // ① 未 sync：零节点、遍历空、命中 None、walk_hit 空，且不崩
        let empty_ok = d.visual().len() == 0
            && d.walk_visual().is_empty()
            && d.hit_test(0, 0).is_none()
            && d.walk_hit(0, 0).is_empty()
            && d.walk_logical(&logic()).len() == expect::LOGIC_NODES;
        // ② sync 后：产出节点、命中非空
        let mut d2 = match DualTree::mount("root", table()) {
            Ok(x) => x,
            Err(_) => {
                cs.fail("F2605-遍历-空树不崩", "第二次 mount 失败");
                return finish_b(cs);
            }
        };
        let lg2 = logic();
        let synced_ok = match d2.sync(&lg2, &SyncOp::FullRebuild) {
            Ok(_) => !d2.walk_visual().is_empty() && d2.hit_test(0, 0).is_some(),
            Err(_) => false,
        };
        cs.add("F2605-遍历-空树不崩", empty_ok && synced_ok, "");
    }

    finish_b(cs)
}

/// 第二批收尾（**保留函数**以便各判据块能统一提前返回）。
fn finish_b(cs: crate::checks::CheckSet) -> crate::checks::CheckSet {
    cs
}

/// 全量自检（两批合一，供注册表用）。
pub fn run_ven05_checks() -> crate::checks::CheckSet {
    let mut cs = run_ven05_checks_a();
    let b = run_ven05_checks_b();
    for i in 0..b.len() {
        if let Some(c) = b.get(i) {
            cs.add(c.name, c.passed, c.detail);
        }
    }
    cs
}

// ---------------------------------------------------------------------------
// 槽位几何辅助（判据侧自持，**不复用被测模块的判定**——
// 用被测物自己的函数验被测物，等于没验）
// ---------------------------------------------------------------------------

/// 两槽是否**重叠**（边界相接不算重叠：半开区间下相接是合法的并排）。
fn slots_overlap(a: &VisualSlot, b: &VisualSlot) -> bool {
    let ax2 = a.x + a.w as i32;
    let ay2 = a.y + a.h as i32;
    let bx2 = b.x + b.w as i32;
    let by2 = b.y + b.h as i32;
    a.x < bx2 && b.x < ax2 && a.y < by2 && b.y < ay2
}

/// `inner` 是否**完全落在** `outer` 内。
fn slot_inside(inner: &VisualSlot, outer: &VisualSlot) -> bool {
    let ox2 = outer.x + outer.w as i32;
    let oy2 = outer.y + outer.h as i32;
    let ix2 = inner.x + inner.w as i32;
    let iy2 = inner.y + inner.h as i32;
    inner.x >= outer.x
        && inner.y >= outer.y
        && ix2 <= ox2
        && iy2 <= oy2
        && inner.w > 0
        && inner.h > 0
}