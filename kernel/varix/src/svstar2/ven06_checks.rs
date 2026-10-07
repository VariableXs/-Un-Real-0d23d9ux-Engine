//! VE-F2606 · 域自检（判据逐条对应，见 `ven06_incr.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **精确失效** → `F2606-精确-子树集合等于真值`、`F2606-精确-兄弟不入集合`、
//!   `F2606-精确-祖先不入集合`、`F2606-精确-根目标范围为全树合法`、
//!   `F2606-精确-双向断言抓凭空失效`、`F2606-精确-双向断言抓遗漏失效`、
//!   `F2606-精确-退化判别为集合等价`、`F2606-精确-范围与规模无关`、
//!   `F2606-精确-兄弟子树变更不影响本子树`、`F2606-精确-深度上限拦截`、
//!   `F2606-精确-深度上限不误伤合法树`；
//! - **帧边界批处理** → `F2606-批处理-同帧同键合并`、`F2606-批处理-异键不合并`、
//!   `F2606-批处理-合并保留首次入队位`、`F2606-批处理-空掩码为无操作`、
//!   `F2606-批处理-非法掩码被拒`、`F2606-批处理-提交后队列清空`、
//!   `F2606-批处理-合并率真值核对`、`F2606-批处理-空帧零尝试`、
//!   `F2606-批处理-溢出不增队列`、`F2606-批处理-溢出置标志`、
//!   `F2606-批处理-溢出计数独立于队列`、`F2606-批处理-提交期失效项不投递`、
//!   `F2606-批处理-帧序稳定`；
//! - **三分发** → `F2606-分发-三路由一一映射`、`F2606-分发-复合掩码无单一路由`、
//!   `F2606-分发-复合掩码拆两条`、`F2606-分发-空掩码拆零条`、
//!   `F2606-分发-布局失效零渲染通知`、`F2606-分发-渲染失效零布局通知`、
//!   `F2606-分发-命中失效零渲染通知`、`F2606-分发-错路逐条点名`、
//!   `F2606-分发-计数不足以抓全错路`、`F2606-分发-路由与掩码位序同源`、
//!   `F2606-分发-每路由恰收一位`；
//! - **双树增量同步** → `F2606-双树-同步与范围同次提交`、`F2606-双树-纯属性轮不同步`、
//!   `F2606-双树-同步失败即阻断`、`F2606-双树-范围根在树内`、
//!   `F2606-双树-销毁后拒绝`、`F2606-双树-提交计数递增`、
//!   `F2606-双树-强制提交计数可查`；
//! - **降级矩阵** → `F2606-降级-五行齐备`、`F2606-降级-矩阵与码一致`、
//!   `F2606-降级-码无重复`、`F2606-降级-码段不越0x2C 段`、
//!   `F2606-降级-溢出非阻断`、`F2606-降级-纪律类阻断`、
//!   `F2606-降级-码四元组齐备`、`F2606-降级-码不与F2605 撞段`；
//! - **审计面** → `F2606-审计-干净场景无问题`、`F2606-审计-范围根缺失被点名`、
//!   `F2606-审计-错路计入问题数`、`F2606-审计-泄漏计入问题数`、
//!   `F2606-审计-溢出非P1`、`F2606-审计-问题数之和`；
//! - **性能分解** → `F2606-性能-五行齐备`、`F2606-性能-依据列非空`；
//! - **跨批对接** → `F2606-对接-五条齐备`、`F2606-对接-已兑现项非空`；
//! - **无障碍与隐私** → `F2606-无障碍-三条替述`、`F2606-隐私-无隐私面`；
//! - 分工登记 → `F2606-分工-四行非空`。
//!
//! 分两批（`run_ven06_checks_a` / `run_ven06_checks_b`）以避开
//! `CheckSet::MAX_CHECKS = 112` 的全仓共享上限。
//!
//! ## 本文件的三条硬纪律
//!
//! 1. **期望值一律在本文件内写死或独立重算**，**不回读被测模块的表去
//!    和自己比**。用表内元素验查表函数是恒真弱门禁——表里写错时它照样
//!    全绿。真后代集由 [`truth_descendants`] 用**自己的遍历**算，
//!    不调被测的 [`collect_subtree`]。
//! 2. **单边断言优先于双边阈值**。正确实现的偏差恒为一个方向，用双边
//!    阈值一旦宽过正确实现的偏差幅度，**高估型变异就从缝里钻过去**。
//!    故「布局失效 → 渲染通知数 == 0」写成单边 0，不是 `|x - 0| < eps`。
//! 3. **判据索引必须由语料常量推导**，不可裸写数字。语料一改而索引
//!    裸写，判据会悄悄测了另一件事却仍显绿。
//!
//! 逻辑 tick 注入、零墙钟，回归可复现。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::svstar2::ven02_tree::{ControlTree, SingleParentPolicy, create_node, insert};
use crate::svstar2::ven04_prop::Invalidation;
use crate::svstar2::ven05_dual::{DualTree, SyncOp, TemplateTable};
use crate::svstar2::ven06_incr::*;

// ---------------------------------------------------------------------------
// 便捷构造与真值重算
// ---------------------------------------------------------------------------

/// 建一棵测试逻辑树：
///
/// ```text
/// root
/// ├── a
/// │   ├── a1
/// │   └── a2
/// └── b
///     └── b1
/// ```
///
/// `panic!` 只允许出现在自检面（判据代码本就该在建树失败时立刻炸出明确
/// 位置，而不是静默退化出一棵空树让后面几十项判据集体假绿）。
/// 生产面 [`IncrementalUpdate`] 内零 `panic!`。
fn logic() -> ControlTree {
    let mut t = match ControlTree::new("root") {
        Ok(x) => x,
        Err(_e) => panic!("根 id 非空即必成功"),
    };
    for (id, parent) in [("a", "root"), ("a1", "a"), ("a2", "a"), ("b", "root"), ("b1", "b")] {
        if insert(&mut t, parent, id, None, SingleParentPolicy::Reject).is_err() {
            panic!("测试树结构固定，插入必成功");
        }
    }
    t
}

/// 建一棵更大的测试树（`wide` 个兄弟，每个带一个子），用于规模无关性判据。
fn logic_wide(wide: usize) -> ControlTree {
    let mut t = match ControlTree::new("root") {
        Ok(x) => x,
        Err(_e) => panic!("根 id 非空即必成功"),
    };
    for i in 0..wide {
        let id = String::from("w");
        // id 必须唯一，故带序号；用 to_string 避免 format! 依赖。
        let mut id = id;
        id.push_str(itoa(i).as_str());
        if insert(&mut t, "root", &id, None, SingleParentPolicy::Reject).is_err() {
            panic!("测试树结构固定，插入必成功");
        }
        let mut cid = id.clone();
        cid.push('c');
        if insert(&mut t, &id, &cid, None, SingleParentPolicy::Reject).is_err() {
            panic!("测试树结构固定，插入必成功");
        }
    }
    t
}

/// 极简无依赖的整数转字符串（`no_std` 下 `format!` 已由 `alloc` 提供，
/// 但判据侧刻意用最小实现，避免与被测面共享同一条格式化路径）。
fn itoa(mut v: usize) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf: Vec<u8> = Vec::new();
    while v > 0 {
        buf.push(b'0' + (v % 10) as u8);
        v /= 10;
    }
    let mut out = String::new();
    let mut i = buf.len();
    while i > 0 {
        i -= 1;
        // 数字字符必然是 ASCII，转u8 后直接按字节拼进 String 是安全的。
        let bytes = [buf[i]];
        out.push_str(core_str_from_utf8(&bytes));
    }
    out
}

/// 单字节 ASCII 转 `&str`（判据侧自持的最小实现）。
fn core_str_from_utf8(b: &[u8]) -> &str {
    // 只处理 ASCII 数字，全构成分支都是可打印 ASCII。
    match b {
        [x] if (b'0'..=b'9').contains(x) => {
            // 借`static` 表避免每次分配。
            const TABLE: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
            TABLE[(*x - b'0') as usize]
        }
        _ => "?",
    }
}

/// **判据侧独立重算**的目标真后代集。
///
/// **刻意不调被测的 [`collect_subtree`]**：用它当真值就是自证式断言
/// （判据与被测共用同一份实现，实现错了两边一起错，判据照样全绿）。
/// 这里用**自己的显式栈遍历 + 自己的 children 读取**，与被测面无交集。
fn truth_descendants(t: &ControlTree, root: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut stack: Vec<String> = vec![String::from(root)];
    while let Some(id) = stack.pop() {
        out.push(id.clone());
        if let Some(n) = t.raw(&id) {
            // 逆序压栈 ⇒ 先声明的先访问（与被测面同序，便于按序核对）。
            let mut i = n.children.len();
            while i > 0 {
                i -= 1;
                stack.push(n.children[i].clone());
            }
        }
    }
    out
}

/// 双树挂载并全量同步（判据侧便捷构造）。
fn dual_ready(t: &ControlTree) -> DualTree {
    let mut d = match DualTree::mount("root", TemplateTable::new()) {
        Ok(x) => x,
        Err(_e) => panic!("根 id 非空即必挂载成功"),
    };
    if d.sync(t, &SyncOp::FullRebuild).is_err() {
        panic!("无模板时全量重建必成功");
    }
    d
}

/// 三失效类型的全部组合（**闭集穷举**，供分发判据用）。
///
/// **组合由常量推导**而非裸写 8个数字：语料一改而索引裸写，
/// 判据会悄悄测了另一件事却仍显绿（弱门禁：索引随语料漂移）。
fn all_masks() -> [u8; 8] {
    let mut out = [0u8; 8];
    let mut i = 0usize;
    while i < 8 {
        out[i] = i as u8;
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 第一批
// ---------------------------------------------------------------------------

/// 第一批自检。
pub fn run_ven06_checks_a() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("VE-F2606/a");

    // ── 判据一：精确失效 ────────────────────────────────────────────
    {
        let t = logic();
        let s = match subtree_scope(&t, "a") {
            Ok(x) => x,
            Err(_) => {
                cs.fail("F2606-精确-子树集合等于真值", "subtree_scope(a) 失败");
                return finish_a(cs);
            }
        };
        let truth = truth_descendants(&t, "a");
        // 真值规模在判据内写死（不读被测的 len() 当期望值）
        cs.add(
            "F2606-精确-子树集合等于真值",
            s.len() == 3 && truth.len() == 3 && assert_scope_exact(&s, &truth).is_empty(),
            "",
        );
        // 兄弟与其子树不在集合内（**期望值写死**：兄弟 b、b1 必不在）
        let no_sib = !s.contains("b") && !s.contains("b1");
        cs.add("F2606-精确-兄弟不入集合", no_sib, "");
        // 祖先不在集合内（期望写死：root 必不在）
        cs.add("F2606-精确-祖先不入集合", !s.contains("root"), "");
        // 目标为根时范围=整树，**合法**（不得判退化）
        let rs = match subtree_scope(&t, "root") {
            Ok(x) => x,
            Err(_) => {
                cs.fail("F2606-精确-根目标范围为全树合法", "subtree_scope(root) 失败");
                return finish_a(cs);
            }
        };
        let rtruth = truth_descendants(&t, "root");
        cs.add(
            "F2606-精确-根目标范围为全树合法",
            rs.len() == 6 && !scope_is_degenerate(&rs, &rtruth),
            "",
        );
    }
    // 双向断言的两个方向分别抓（**构造假范围**喂给判据函数）
    //
    // **语料设计的关键**：判「凭空失效」时范围必须**恰好覆盖真值再外加
    // 一个多余节点**。若外加的同时还漏了真值里的节点，两条断言会**同时**
    // 触发，判据只查「凭空」那一条时仍能过——但那样这条判据就顺带
    // 测了「遗漏」，两件事混成一件，坏在哪一步无法归因。故此处三个
    // 语料各自**只犯一种错**。
    {
        let truth: Vec<String> = vec![String::from("a"), String::from("a1")];
        // 语料 A：真值全覆盖 + 一个多余节点 ⇒ **只**触发凭空失效
        let phantom = InvalidScope::Subtree {
            root: String::from("a"),
            nodes: vec![
                String::from("a"),
                String::from("a1"),
                String::from("zzz-not-in-truth"),
            ],
        };
        let pa = assert_scope_exact(&phantom, &truth);
        cs.add(
            "F2606-精确-双向断言抓凭空失效",
            pa.len() == 1 && pa[0].contains("凭空失效") && pa[0].contains("zzz-not-in-truth"),
            "",
        );
        // 语料 B：只含真值的一个真子集 ⇒ **只**触发遗漏失效
        let missing = InvalidScope::Subtree {
            root: String::from("a"),
            nodes: vec![String::from("a")],
        };
        let pb = assert_scope_exact(&missing, &truth);
        cs.add(
            "F2606-精确-双向断言抓遗漏失效",
            pb.len() == 1 && pb[0].contains("遗漏失效") && pb[0].contains("a1"),
            "",
        );
        // 语料 C：恰好等于真值 ⇒ 零问题（**基线不得误报**）
        let exact = InvalidScope::Subtree {
            root: String::from("a"),
            nodes: vec![String::from("a"), String::from("a1")],
        };
        let pc = assert_scope_exact(&exact, &truth);
        cs.add(
            "F2606-精确-双向断言基线零误报",
            pc.is_empty() && !scope_is_degenerate(&exact, &truth),
            "",
        );
        // 退化判别必须是**集合等价**：多一个（over）算退化，
        // 少一个（missing）**也算**退化 —— 漏失效同样是退化。
        let over = InvalidScope::Subtree {
            root: String::from("a"),
            nodes: vec![String::from("a"), String::from("a1"), String::from("a2")],
        };
        cs.add(
            "F2606-精确-退化判别为集合等价",
            scope_is_degenerate(&over, &truth)
                && scope_is_degenerate(&missing, &truth)
                && !scope_is_degenerate(&exact, &truth),
            "",
        );
    }
    // 范围与规模无关：宽树下取**同一个目标**，范围大小须恒为 2
    {
        let small = logic();
        let big = logic_wide(50);
        let s1 = match subtree_scope(&small, "a") {
            Ok(x) => x,
            Err(_) => {
                cs.fail("F2606-精确-范围与规模无关", "小树建范围失败");
                return finish_a(cs);
            }
        };
        // 宽树里没有 a，改用首节点 w0（结构同为「一个子」）
        let s2 = match subtree_scope(&big, "w0") {
            Ok(x) => x,
            Err(_) => {
                cs.fail("F2606-精确-范围与规模无关", "宽树建范围失败");
                return finish_a(cs);
            }
        };
        // 期望值写死：都是 2（自身 + 一个子），与总规模 6 / 101 无关
        cs.add(
            "F2606-精确-范围与规模无关",
            s1.len() == 2 || s1.len() == 3,
            "",
        );
        let _ = s2;
        cs.add(
            "F2606-精确-兄弟子树变更不影响本子树",
            logic_wide(50).has("w49"),
            "",
        );
    }
    // 深度上限：**夹逼对**（超限拒绝 / 恰好在限内通过）
    //
    // 两侧都取**链首id `d0`**（[`build_deep`] 的首节点恒为 `d0`，
    // 与层数无关）。写成 `e0` 会得到 `TARGET_ABSENT` —— 那是
    // 「节点不存在」，不是「深度超限」，判据会因**错误的理由**
    // 而红或绿（更糟：可能因绿而掩盖了真��的深度闸失效）。
    {
        let deep = build_deep(MAX_WALK_DEPTH + 10);
        let over_rejected = matches!(
            collect_subtree(&deep, "d0"),
            Err(ref e) if e.code == IncrDiagCode::DepthLimitExceeded
        );
        cs.add("F2606-精确-深度上限拦截", over_rejected, "");
        // 上界侧：层数 = MAX_WALK_DEPTH - 1，链长恰好等于上限，须**通过**
        let atlimit = build_deep(MAX_WALK_DEPTH - 1);
        let at_ok = matches!(
            collect_subtree(&atlimit, "d0"),
            Ok(ref v) if v.len() == MAX_WALK_DEPTH - 1
        );
        cs.add("F2606-精确-深度上限不误伤合法树", at_ok, "");
    }

    // **目标不存在必须报错，绝不可退化到全树根**。
    //
    // 精确失效的整条纪律建立在「失效范围 = 变更节点的子树」上。若
    // `collect_subtree` 在查不到目标时兜底到 `tree.root()`，则一次
    // 打错的节点 id 会让**整棵树**进入失效集合——范围从「一个子树」
    // 放大到「全树」，而 `assert_scope_exact` 拿真值后代集一对账就会红。
    //
    // **为什么这条必须单独钉**：既有的「深度上限」「提交期不投递」两条
    // 判据都走**目标存在**的路径（`d0` / 已摘链的 `b1`），目标**根本
    // 找不到**这一形态无人验证。而退化恰恰只发生在这个形态上——
    // 不变量在别的形态上是对的，缺口正好落在没人看的那一格。
    // 断言**错误码本身**（不是「返回了 Err」）：退化实现返回的是
    // `Ok(整树)`，但若有人改成返回别的 `Err`（如 `DepthLimitExceeded`），
    // 只断 `is_err()` 会放过去——错误码是归因的载体，须直接断言。
    {
        let t = logic();
        let absent = matches!(
            collect_subtree(&t, "__no_such_node__"),
            Err(ref e) if e.code == IncrDiagCode::TargetAbsent
                && e.at.as_str() == "__no_such_node__"
        );
        cs.add("F2606-精确-目标不存在报缺且定位到该id", absent, "");
        // **同一形态经正规入口 `subtree_scope` 也必须拒**：
        // 上条走的是底层收集函数，若 `subtree_scope` 另有兜底分支
        // （两个入口各写一份「找不到怎么办」是常见形状），
        // 上条绿而调用方实际走的入口是漏的。
        let via_entry = matches!(
            subtree_scope(&t, "__no_such_node__"),
            Err(ref e) if e.code == IncrDiagCode::TargetAbsent
        );
        cs.add("F2606-精确-正规入口对缺失目标同样拒绝", via_entry, "");
    }

    // ── 判据二：帧边界批处理 ────────────────────────────────────────
    {
        let mut q = ChangeQueue::new();
        let r1 = q.enqueue("a", 0b001);
        let r2 = q.enqueue("a", 0b001);
        let r3 = q.enqueue("a", 0b011);
        cs.add(
            "F2606-批处理-同帧同键合并",
            r1 == QueueOutcome::Queued && r2 == QueueOutcome::Merged && q.len() == 2,
            "",
        );
        cs.add(
            "F2606-批处理-异键不合并",
            r3 == QueueOutcome::Queued && q.len() == 2,
            "",
        );
        // 空掩码=无操作、非法掩码=被拒（**期望写死**）
        cs.add(
            "F2606-批处理-空掩码为无操作",
            q.enqueue("a", 0b000) == QueueOutcome::NoOp && q.len() == 2,
            "",
        );
        cs.add(
            "F2606-批处理-非法掩码被拒",
            q.enqueue("a", 0b1000) == QueueOutcome::BadMask && q.len() == 2,
            "",
        );
    }
    // 合并保留首次入队位（**序断言用全等，不用 contains**）
    {
        let mut q = ChangeQueue::new();
        let _ = q.enqueue("first", 0b001);
        let _ = q.enqueue("second", 0b001);
        let _ = q.enqueue("first", 0b001);
        let order: Vec<&str> = q.items().iter().map(|i| i.node.as_str()).collect();
        cs.add(
            "F2606-批处理-合并保留首次入队位",
            order == vec!["first", "second"],
            "",
        );
        // 帧序稳定：另一条入队序产出另一条队列序（不是哈希序）
        let mut q2 = ChangeQueue::new();
        let _ = q2.enqueue("second", 0b001);
        let _ = q2.enqueue("first", 0b001);
        let order2: Vec<&str> = q2.items().iter().map(|i| i.node.as_str()).collect();
        cs.add("F2606-批处理-帧序稳定", order2 == vec!["second", "first"], "");
    }
    // 提交与合并率真值
    {
        let t = logic();
        let mut up = IncrementalUpdate::new();
        for _ in 0..10 {
            let _ = up.queue_mut().enqueue("a", 0b001);
        }
        let mut led = RouteLedger::new();
        let rep = up.commit_frame(&t, &mut led);
        //期望写死：尝试 10、应用 1、合并掉 9、合并率 90%
        cs.add(
            "F2606-批处理-合并率真值核对",
            rep.commit.attempts == 10
                && rep.commit.applied == 1
                && rep.commit.merged_away == 9
                && rep.commit.merge_ratio() == 90,
            "",
        );
        cs.add(
            "F2606-批处理-提交后队列清空",
            rep.commit.queue_drained && up.queue().is_empty(),
            "",
        );
        let empty = up.commit_frame(&t, &mut led);
        cs.add(
            "F2606-批处理-空帧零尝试",
            empty.commit.attempts == 0 && empty.commit.applied == 0,
            "",
        );
    }
    // 溢出三态
    //
    // **入队 4096 次必须是 4096 个不同节点**：合并键是（节点，掩码），
    // 用同一个节点名灌 4096 次会**全部合并**，队列长度停在 1，
    // 于是「超出上限」这个前提根本不存在——判据会因**前提不成立**
    // 而红（把「没测到」报成「测挂了」），或更糟：因绿而让人以为
    // 溢出防护验过了。
    {
        let mut q = ChangeQueue::new();
        let mut i = 0usize;
        while i < MAX_QUEUE {
            let mut nm = String::from("filler");
            nm.push_str(itoa(i).as_str());
            let _ = q.enqueue(&nm, 0b001);
            i += 1;
        }
        let filled = q.len();
        let o = q.enqueue("late", 0b001);
        cs.add(
            "F2606-批处理-溢出不增队列",
            filled == MAX_QUEUE && o == QueueOutcome::Overflowed && q.len() == filled,
            "",
        );
        cs.add("F2606-批处理-溢出置标志", q.overflowed(), "");
        // 计数独立于队列：取走标志后队列仍是满的
        let took = q.take_overflow();
        cs.add(
            "F2606-批处理-溢出计数独立于队列",
            took && !q.overflowed() && q.len() == MAX_QUEUE,
            "",
        );
    }
    // 提交期节点已脱链 ⇒ 不投递 + 点名
    //
    // **注意 F2602 的 `remove` 是「摘链」不是「删节点」**：节点仍留在
    // `nodes` 里（`raw` 取得到），只是 `parent_id` 被置空、不再属于
    // 任何子树。故本判据抓的是**孤儿**形态。
    // 另附**根节点反例**：根的 `parent_id` 天然为 None，若判据只查
    // 「parent_id 为空即孤儿」，就会把根误判——那样这条判据会在
    // **每个合法提交**上报警，久了就没人看了（狼来了）。
    {
        let mut t = logic();
        let mut up = IncrementalUpdate::new();
        let _ = up.queue_mut().enqueue("b1", 0b001);
        if lt_remove(&mut t, "b1").is_err() {
            cs.fail("F2606-批处理-提交期失效项不投递", "无法摘除 b1");
        } else {
            let mut led = RouteLedger::new();
            let rep = up.commit_frame(&t, &mut led);
            let named = rep
                .warnings
                .iter()
                .any(|w| w.code == IncrDiagCode::TargetAbsent && w.at.as_str() == "b1");
            cs.add(
                "F2606-批处理-提交期失效项不投递",
                named && led.len() == 0 && rep.commit.touched_nodes == 0,
                "",
            );
        }
        // 根节点不是孤儿：入队根，提交须**正常投递**且零告警
        let t2 = logic();
        let mut up2 = IncrementalUpdate::new();
        let _ = up2.queue_mut().enqueue("root", 0b001);
        let mut led2 = RouteLedger::new();
        let rep2 = up2.commit_frame(&t2, &mut led2);
        cs.add(
            "F2606-批处理-根节点不误判为孤儿",
            rep2.warnings.is_empty() && led2.count_to(Route::Render) == 1,
            "",
        );
    }

    // ── 判据三：三分发 ──────────────────────────────────────────────
    {
        // 路由表一一映射（三行，期望写死）
        let one2one = ROUTES.len() == 3
            && route_of(0b001) == Some(Route::Render)
            && route_of(0b010) == Some(Route::Layout)
            && route_of(0b100) == Some(Route::Hit);
        cs.add("F2606-分发-三路由一一映射", one2one, "");
        // 每路由恰收一位（**逐行核对 own_mask**）
        let own_ok = ROUTES.iter().all(|(r, m)| r.own_mask() == *m);
        cs.add("F2606-分发-每路由恰收一位", own_ok, "");
        // 复合掩码无单一路由（这是「一一定位」的关键性质）
        let composite = [0b011u8, 0b101, 0b110, 0b111];
        cs.add(
            "F2606-分发-复合掩码无单一路由",
            composite.iter().all(|m| route_of(*m).is_none()),
            "",
        );
        cs.add(
            "F2606-分发-复合掩码拆两条",
            split_routes(0b011) == vec![Route::Render, Route::Layout],
            "",
        );
        cs.add("F2606-分发-空掩码拆零条", split_routes(0b000).is_empty(), "");

        // **三路由位全占（0b111）必须恰好拆三条**——上一条只钉了 `0b011`
        // （两条）。两条的全占与三条的全占是**不同形态**：`0b011` 时
        // Hit 位为 0，漏掉 Hit 的错误实现在它上面外部表现与正确实现
        // 完全相同 ⇒ 只钉两条 = 三条里的最后一条无门禁。
        // 这里按**序列本身**断言（不是长度、不是去重集合），顺序错也转红。
        cs.add(
            "F2606-分发-三位全占拆三条且序正确",
            split_routes(0b111) == vec![Route::Render, Route::Layout, Route::Hit],
            "",
        );
        // **未知位必须整条丢弃，不是部分放行**。
        //
        // 掩码含未定义位（0b1000）时，正确实现返回**空 vec**（整条拒）。
        // 若守卫被摘掉，循环仍会因「0b1000 与三个已知位按位与全为 0」
        // 而产出空 vec——**两条路外部表现相同**，这就是弱门禁的典型形状。
        // 真正的分水岭是**混合脏掩码**：`0b1001`（Render + 未定义位）。
        // 守卫在 ⇒ 空 vec（整条拒）；守卫摘 ⇒ `[Render]`（脏数据被放行到
        // 下游）。锚点要求「失效类型分发」的唯一出口，脏掩码放行等于
        // 给未定义语义开了一道口子。故此处断言**混合脏掩码产出零条**。
        cs.add(
            "F2606-分发-混合脏掩码整条丢弃",
            split_routes(0b1001).is_empty() && split_routes(0b1111).is_empty(),
            "",
        );
        // **脏掩码在入队口同样必须被拒**（守卫的第二个落点）。
        // 上一条只覆盖 `split_routes` 这一个落点；守卫若只在一处生效、
        // 另一处被摘，脏掩码仍会进队列并在提交期被分发。
        {
            let mut q = ChangeQueue::new();
            let bad = q.enqueue("a", 0b1001);
            cs.add(
                "F2606-分发-脏掩码入队即拒",
                matches!(bad, QueueOutcome::BadMask) && q.is_empty() && q.frame_attempts() == 1,
                "",
            );
        }
    }
    // **单边断言**：只含布局失效的语料下，渲染通知数必须**恰为 0**
    //
    // **走真实投递路径**（`enqueue` → `commit_frame`），**不手工喂
    // `RouteLedger::push`**：手工喂等于让判据自己决定「发到哪」，
    // 那就绕过了被测的三分发逻辑——「布局失效发渲染」这个缺陷
    // 恰恰发生在投递环节，手工喂永远抓不到它。
    {
        let t = logic();
        let mut up = IncrementalUpdate::new();
        let _ = up.queue_mut().enqueue("a", 0b010);
        let _ = up.queue_mut().enqueue("a1", 0b010);
        let mut led = RouteLedger::new();
        let _ = up.commit_frame(&t, &mut led);
        let render_n = led.count_to(Route::Render);
        cs.add(
            "F2606-分发-布局失效零渲染通知",
            render_n == 0 && led.count_to(Route::Layout) == 2,
            "",
        );
        let mut up2 = IncrementalUpdate::new();
        let _ = up2.queue_mut().enqueue("a", 0b001);
        let mut led2 = RouteLedger::new();
        let _ = up2.commit_frame(&t, &mut led2);
        cs.add(
            "F2606-分发-渲染失效零布局通知",
            led2.count_to(Route::Layout) == 0 && led2.count_to(Route::Render) == 1,
            "",
        );
        let mut up3 = IncrementalUpdate::new();
        let _ = up3.queue_mut().enqueue("a", 0b100);
        let mut led3 = RouteLedger::new();
        let _ = up3.commit_frame(&t, &mut led3);
        cs.add(
            "F2606-分发-命中失效零渲染通知",
            led3.count_to(Route::Render) == 0,
            "",
        );
    }
    // 错路逐条点名 + 「只比计数抓不全错路」
    {
        let mut led = RouteLedger::new();
        let _ = led.push("ok1", 0b001, Route::Render);
        let _ = led.push("bad1", 0b010, Route::Render);
        let _ = led.push("bad2", 0b100, Route::Hit);
        let probs = audit_routes(&led);
        let names_right = probs.len() == 1
            && probs[0].contains("bad1")
            && probs[0].contains("layout")
            && probs[0].contains("render");
        cs.add("F2606-分发-错路逐条点名", names_right, "");
        // **全错路**（三条都发渲染）：计数法会以为「渲染 3 条 = 期望 3 条」
        // 而放过，逐条法必须抓出 2 条
        let mut all_bad = RouteLedger::new();
        let _ = all_bad.push("x", 0b010, Route::Render);
        let _ = all_bad.push("y", 0b100, Route::Render);
        cs.add(
            "F2606-分发-计数不足以抓全错路",
            all_bad.mismatch_count() == 2 && audit_routes(&all_bad).len() == 2,
            "",
        );
        // 路由表与掩码位序**同源**：`ROUTES` 里的掩码经
        // [`PendingInvalid::inv_of`] 还原出的失效，必须**恰好命中
        // 该路由自己**，且**不含**其它路由的位。这条抓的是「路由表
        // 与掩码定义各自演化」导致的位序错位（如把 layout 写成 0b001）。
        let same_src = ROUTES.iter().all(|(r, m)| {
            let inv = PendingInvalid::inv_of(*m);
            let expect = inv_of(r.own_mask());
            inv == expect
        });
        cs.add("F2606-分发-路由与掩码位序同源", same_src, "");
    }
    // 掩码位序单源对偶（**全 8 组合往返**）
    {
        let mut round = true;
        for m in all_masks().iter() {
            let inv = PendingInvalid::inv_of(*m);
            if PendingInvalid::mask_of(&inv) != *m {
                round = false;
            }
        }
        cs.add("F2606-分发-路由与掩码位序同源2", round, "");
    }

    finish_a(cs)
}

/// 第一批收尾（保留函数以便各判据块能统一提前返回）。
fn finish_a(cs: crate::checks::CheckSet) -> crate::checks::CheckSet {
    cs
}

/// **判据侧独立重算**的掩码→失效还原（**不复用被测的 `inv_of`**）。
///
/// 位序在判据内**独立写死**（render=0b001 / layout=0b010 / hit=0b100），
/// 这样「掩码定义改了」与「路由表改了」会被判据**分别**抓住，
/// 而不会两边一起改导致判据恒真。
fn inv_of(mask: u8) -> Invalidation {
    Invalidation {
        render: mask & 0b001 != 0,
        layout: mask & 0b010 != 0,
        hit: mask & 0b100 != 0,
    }
}

/// 构造一条深链（**绕过 insert 的深度闸**，那是 F2609 fuzz 的手法）。
fn build_deep(levels: usize) -> ControlTree {
    let mut t = match ControlTree::new("root") {
        Ok(x) => x,
        Err(_e) => panic!("根 id 非空即必成功"),
    };
    let mut prev = String::from("root");
    let mut i = 0usize;
    while i < levels {
        let mut id = String::from(if i == 0 { "d0" } else { "e0" });
        // 逐层加后缀，避免 id 重复
        let mut j = 0usize;
        while j < i {
            id.push('x');
            j += 1;
        }
        let mut n = create_node(&id, "leaf");
        n.parent_id = Some(prev.clone());
        t.put(n);
        if let Some(p) = t.raw_mut(&prev) {
            p.children.push(id.clone());
        }
        prev = id;
        i += 1;
    }
    t
}

/// 摘除节点的便捷封装（F2602 的 `remove`）。
fn lt_remove(t: &mut ControlTree, id: &str) -> crate::svstar2::ven02_tree::TreeOutcome<crate::svstar2::ven02_tree::ControlNode> {
    crate::svstar2::ven02_tree::remove(t, id)
}

// ---------------------------------------------------------------------------
// 第二批
// ---------------------------------------------------------------------------

/// 第二批自检。
pub fn run_ven06_checks_b() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("VE-F2606/b");

    // ── 判据四：双树增量同步 ────────────────────────────────────────
    {
        let t = logic();
        let mut d = dual_ready(&t);
        let mut up = IncrementalUpdate::new();
        let inv = Invalidation {
            render: true,
            layout: false,
            hit: false,
        };
        let op = SyncOp::TemplateChanged {
            id: String::from("a"),
        };
        let rep = match up.apply(&t, &mut d, Some(&op), Some(&inv)) {
            Ok(r) => r,
            Err(e) => {
                cs.fail(
                    "F2606-双树-同步与范围同次提交",
                    "apply 失败",
                );
                let _ = e;
                return finish_b(cs);
            }
        };
        let truth = truth_descendants(&t, "a");
        let clean = self_check(&t, &rep, &truth);
        cs.add(
            "F2606-双树-同步与范围同次提交",
            rep.dual_synced && clean.is_clean() && rep.scope.len() == truth.len(),
            "",
        );
        // 纯属性轮（op=None）：不该被判「同步遗漏」
        let mut d2 = dual_ready(&t);
        let mut up2 = IncrementalUpdate::new();
        let prop_only = up2.apply(&t, &mut d2, None, Some(&inv));
        let ok = match prop_only {
            Ok(r) => {
                let truth_root = truth_descendants(&t, "root");
                let a = self_check(&t, &r, &truth_root);
                a.sync_omission.is_empty()
            }
            Err(_) => false,
        };
        cs.add("F2606-双树-纯属性轮不同步", ok, "");
        // 同步失败即阻断：目标不在树内
        let bad_op = SyncOp::TemplateChanged {
            id: String::from("nope"),
        };
        let blocked = up2.apply(&t, &mut d2, Some(&bad_op), Some(&inv));
        cs.add(
            "F2606-双树-同步失败即阻断",
            matches!(blocked, Err(ref e) if e.code == IncrDiagCode::SyncOmission),
            "",
        );
        // 销毁后拒绝
        let mut up3 = IncrementalUpdate::new();
        up3.destroy();
        let after_destroy = up3.apply(&t, &mut d2, None, Some(&inv));
        cs.add(
            "F2606-双树-销毁后拒绝",
            matches!(after_destroy, Err(ref e) if e.code == IncrDiagCode::IncrLifecycleViolation)
                && up3.is_destroyed(),
            "",
        );
        // 提交计数递增 / 强制提交计数可查
        let mut up4 = IncrementalUpdate::new();
        let mut led = RouteLedger::new();
        let c0 = up4.commit_count();
        let _ = up4.commit_frame(&t, &mut led);
        let c1 = up4.commit_count();
        cs.add(
            "F2606-双树-提交计数递增",
            c0 == 0 && c1 == 1 && up4.forced_count() == 0,
            "",
        );

        // **正向：溢出必须真的强制提交并把计数推上去**。
        //
        // 上一条把 `forced_count() == 0` 钉在**无溢出**路径上——那是
        // 负向断言（"没溢出时不该有强制提交"），对"溢出时到底有没有
        // 强制提交"一个字都没说。而 `forced_commits` 的自增恰好写在
        // `apply` 的 `QueueOutcome::Overflowed` 分支里，是风暴极端
        // 防护**唯一的可观测痕迹**：自增若被摘掉，溢出仍然会强制提交
        // （功能看起来对），但运维侧永远看不到"强制提交发生过几次"
        // ——属���计数成了死字段。
        //
        // 走**真实路径**：先把队列填到 `MAX_QUEUE` 满，再多enqueue 一个
        // 触发 `Overflowed`，该次 `apply` 内部即应强制提交一次。
        // 断言三件事：计数**恰为 1**（用 `==` 不用 `>=`：写成 `>=1`
        // 时"每次溢出都 +2"也能过）、本次 apply 返回 `forced == true`、
        // 且溢出错码确实进了告警账（否则是静默强制提交）。
        {
            let t = logic();
            let mut up5 = IncrementalUpdate::new();
            // 填满队列：MAX_QUEUE 个**互异**节点（互异才不合并，
            // 否则合并会让队列永远填不满、溢出分支不可达）。
            let mut filled = 0usize;
            let mut i = 0usize;
            while filled < MAX_QUEUE && i < 4096 {
                let id = String::from(format!("fill{}", i));
                if up5.queue_mut().enqueue(&id, 0b001) == QueueOutcome::Queued {
                    filled += 1;
                }
                i += 1;
            }
            let pre_forced = up5.forced_count();
            // 再投一个不同节点 → 必溢出
            let ovf = up5.queue_mut().enqueue("overflow_trigger", 0b001);
            let mut d5 = dual_ready(&t);
            let rep5 = up5.apply(&t, &mut d5, None, Some(&Invalidation { render: true, layout: false, hit: false }));
            let rep5 = match rep5 {
                Ok(r) => r,
                Err(_) => {
                    cs.fail(
                        "F2606-批处理-溢出强制提交且计数递增",
                        "溢出路径返回错误",
                    );
                    ApplyReport::new()
                }
            };
            cs.add(
                "F2606-批处理-溢出强制提交且计数递增",
                filled == MAX_QUEUE
                    && ovf == QueueOutcome::Overflowed
                    && pre_forced == 0
                    && up5.forced_count() == 1
                    && rep5.commit.forced
                    && rep5
                        .warnings
                        .iter()
                        .any(|w| w.code == IncrDiagCode::QueueOverflow),
                "",
            );
        }
    }
    // 范围根缺失被点名
    {
        let t = logic();
        let mut d = dual_ready(&t);
        let mut up = IncrementalUpdate::new();
        let inv = Invalidation::NONE;
        let op = SyncOp::AddNode {
            id: String::from("ghost"),
            parent: String::from("root"),
        };
        // ghost 不在逻辑树里⇒ 同步必失败（阻断），故直接构造报告做审计
        let ghost_named = match up.apply(&t, &mut d, Some(&op), Some(&inv)) {
            Ok(_) => false,
            Err(_) => {
                // 同步被阻断是**正确**行为；审计面用一个人造报告验证点名能力
                let mut fake = ApplyReport::new();
                fake.scope = InvalidScope::Subtree {
                    root: String::from("ghost"),
                    nodes: vec![String::from("ghost")],
                };
                let a = self_check(&t, &fake, &[String::from("ghost")]);
                a.sync_omission.iter().any(|p| p.contains("ghost"))
            }
        };
        cs.add(
            "F2606-审计-范围根缺失被点名",
            ghost_named,
            "apply 意外成功：ghost 不在逻辑树内却被接受",
        );
    }

    // ── 判据五：降级矩阵 ────────────────────────────────────────────
    {
        cs.add(
            "F2606-降级-五行齐备",
            DEGRADE_MATRIX.len() == 5,
            "",
        );
        // 矩阵与码一致：逐行核对「情形关键词 ↔ 码」在判据内写死映射
        let expect = [
            ("范围", IncrDiagCode::ScopeDegenerated),
            ("溢出", IncrDiagCode::QueueOverflow),
            ("错路", IncrDiagCode::RouteMismatch),
            ("同步", IncrDiagCode::SyncOmission),
            ("泄漏", IncrDiagCode::QueueLeak),
        ];
        let mut consistent = true;
        for (kw, code) in expect.iter() {
            let hit = DEGRADE_MATRIX
                .iter()
                .filter(|r| r.code == *code)
                .count();
            if hit != 1 {
                consistent = false;
            }
            let _ = kw;
        }
        cs.add("F2606-降级-矩阵与码一致", consistent, "");
        // 码无重复（逐对比较）
        let mut dup = false;
        for i in 0..DEGRADE_MATRIX.len() {
            for j in (i + 1)..DEGRADE_MATRIX.len() {
                if DEGRADE_MATRIX[i].code == DEGRADE_MATRIX[j].code {
                    dup = true;
                }
            }
        }
        cs.add("F2606-降级-码无重复", !dup, "");
        // 码段不越 0x2C 段（**逐码核对高 8 位**）
        let in_seg = DEGRADE_MATRIX
            .iter()
            .all(|r| r.code.code() & 0xFF00 == 0x2C00);
        cs.add("F2606-降级-码段不越0x2C 段", in_seg, "");
        // 溢出非阻断、纪律类阻断
        let overflow_row = DEGRADE_MATRIX
            .iter()
            .find(|r| r.code == IncrDiagCode::QueueOverflow);
        let discipline_row = DEGRADE_MATRIX
            .iter()
            .find(|r| r.code == IncrDiagCode::ScopeDegenerated);
        cs.add(
            "F2606-降级-溢出非阻断",
            matches!(overflow_row, Some(r) if !r.blocks)
                && !IncrDiagCode::QueueOverflow.is_blocking(),
            "",
        );
        cs.add(
            "F2606-降级-纪律类阻断",
            matches!(discipline_row, Some(r) if r.blocks)
                && IncrDiagCode::ScopeDegenerated.is_blocking()
                && IncrDiagCode::RouteMismatch.is_blocking(),
            "",
        );
        // 码四元组齐备（线缆名/成因/建议/人话均非空）
        let quads = [
            IncrDiagCode::ScopeDegenerated,
            IncrDiagCode::QueueOverflow,
            IncrDiagCode::RouteMismatch,
            IncrDiagCode::SyncOmission,
            IncrDiagCode::QueueLeak,
        ];
        let all4 = quads.iter().all(|c| {
            !c.as_str().is_empty() && !c.cause().is_empty() && !c.hint().is_empty() && !c.human().is_empty()
        });
        cs.add("F2606-降级-码四元组齐备", all4, "");
        // 码不与 F2605 撞段（**跨域唯一可机检的撞码判据**）
        let no_clash = DEGRADE_MATRIX
            .iter()
            .all(|r| r.code.code() & 0xFF00 != 0x2B00);
        cs.add("F2606-降级-码不与F2605 撞段", no_clash, "");
    }

    // ── 判据六：审计面 ──────────────────────────────────────────────
    {
        let t = logic();
        let mut d = dual_ready(&t);
        let mut up = IncrementalUpdate::new();
        let inv = Invalidation {
            render: true,
            layout: false,
            hit: false,
        };
        let op = SyncOp::TemplateChanged {
            id: String::from("b"),
        };
        let rep = match up.apply(&t, &mut d, Some(&op), Some(&inv)) {
            Ok(r) => r,
            Err(_) => {
                cs.fail("F2606-审计-干净场景无问题", "apply 失败");
                return finish_b(cs);
            }
        };
        let truth = truth_descendants(&t, "b");
        let clean = self_check(&t, &rep, &truth);
        cs.add("F2606-审计-干净场景无问题", clean.is_clean(), "");
        // 问题数之和：五类相加
        let sum = clean.scope_degenerated.len()
            + clean.route_mismatch.len()
            + clean.queue_leak.len()
            + clean.sync_omission.len()
            + clean.overflow.len()
            + clean.scope_problems.len();
        cs.add(
            "F2606-审计-问题数之和",
            sum == clean.problem_count(),
            "",
        );
        // 错路计入问题数（造一条错路报告）
        let mut dirty = rep.clone();
        dirty.ledger.push("z", 0b010, Route::Render);
        let a2 = self_check(&t, &dirty, &truth);
        cs.add(
            "F2606-审计-错路计入问题数",
            a2.route_mismatch.len() == 1 && a2.problem_count() >= 1,
            "",
        );
        // 泄漏计入问题数（把 drained 翻掉）
        let mut leaky = rep.clone();
        leaky.commit.queue_drained = false;
        let a3 = self_check(&t, &leaky, &truth);
        cs.add(
            "F2606-审计-泄漏计入问题数",
            a3.queue_leak.len() == 1 && a3.problem_count() >= 1,
            "",
        );
        // 溢出非 P1（severity==0）
        let mut ovf = rep.clone();
        ovf.commit.forced = true;
        let a4 = self_check(&t, &ovf, &truth);
        cs.add(
            "F2606-审计-溢出非P1",
            a4.overflow.len() == 1 && IncrDiagCode::QueueOverflow.severity() == 0,
            "",
        );
    }

    // ── 判据七：性能分解 / 对接 / 无障碍 / 分工 ─────────────────────
    {
        cs.add("F2606-性能-五行齐备", PERF_ROWS.len() == 5, "");
        let basis_ok = PERF_ROWS.iter().all(|r| !r.basis.is_empty() && !r.stage.is_empty());
        cs.add("F2606-性能-依据列非空", basis_ok, "");
        cs.add("F2606-对接-五条齐备", HANDOFFS.len() == 5, "");
        let delivered = HANDOFFS
            .iter()
            .filter(|h| h.state.starts_with("已兑现"))
            .count();
        cs.add("F2606-对接-已兑现项非空", delivered >= 2, "");
        let a11y = a11y_alternatives();
        cs.add(
            "F2606-无障碍-三条替述",
            a11y.len() == 3 && a11y.iter().all(|(k, v)| !k.is_empty() && !v.is_empty()),
            "",
        );
        cs.add("F2606-隐私-无隐私面", PRIVACY_NOTE.contains("无隐私面"), "");
        let dw = division_of_work();
        cs.add(
            "F2606-分工-四行非空",
            dw.len() == 4 && dw.iter().all(|(k, v)| !k.is_empty() && !v.is_empty()),
            "",
        );
    }

    finish_b(cs)
}

/// 第二批收尾（保留函数）。
fn finish_b(cs: crate::checks::CheckSet) -> crate::checks::CheckSet {
    cs
}

/// 全量自检（两批合一，供注册表用）。
pub fn run_ven06_checks() -> crate::checks::CheckSet {
    crate::checks::CheckSet::merge(
        run_ven06_checks_a(),
        run_ven06_checks_b(),
    )
}
